// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The surface: authenticate, limit, check the scope, check the phase, answer,
//! log.
//!
//! This is the shape the whole roadmap inherits (skeleton plan T9, risk R8), so
//! it is built before a single gameplay method hangs off it. Every call goes
//! through exactly these steps, in exactly this order:
//!
//! 1. **Authenticate.** No token, or a token this match never minted, revoked,
//!    or expired: [`crate::error::Code::Unauthenticated`].
//! 2. **Rate limit**, per token, counted in the host's ticks
//!    ([`crate::limit`]).
//! 3. **Resolve the method** against the schema's own list
//!    ([`crate::scopes`]). An unknown name never reaches a handler.
//! 4. **Check the scope** the schema's annotation names for that method. Not a
//!    table in this file -- T1's annotation, read back from the descriptor set.
//! 5. **Check the phase.** Planning is closed during the Push and the recap
//!    (spec section 12), and "a planning method" means one whose scope is `plan`
//!    or `plan.submit` -- derived from the same annotation rather than from a
//!    second list.
//! 6. **Answer**, with a `_status` footer on every result.
//! 7. **Log** the attempt, whatever the answer was ([`crate::audit`]).
//!
//! # The method slice at T9, and why it is this small
//!
//! T9 is the security surface; T13 is the method slice. What is implemented
//! here is the smallest set that exercises every step above under every scope
//! that gates a method:
//!
//! | Method | Scope | What it does at T9 |
//! |---|---|---|
//! | `get_status` | `observe` | Answers the `_status` footer, which is the whole message |
//! | `get_segment_feed` | `observe` | The real feed: fog-filtered, digested, cursored ([`crate::feed`]) |
//! | `save_notes` | `plan` | Stores the seat's own notebook, at the rules table's size |
//! | `list_drafts` | `plan` | Lists the seat's own drafts |
//! | `set_ready` | `plan.submit` | Sets the seat's ready flag |
//! | `list_templates` | `docs` | An empty listing: the template folder is T13's |
//!
//! Every other method of the schema answers
//! [`crate::error::Code::Internal`] naming T13. That is a deliberate reading of
//! the closed error set and it deserves its sentence: the set has no
//! `NOT_IMPLEMENTED`, `INVALID_ARGUMENT` would tell a client to fix a request
//! that is perfectly well formed, and `INTERNAL` is the one code whose
//! definition -- "the gateway failed; never used to report anything the caller
//! could have avoided" -- is actually true of a method this build does not serve
//! yet. The pull request records it as a question for the owner, who is holding
//! the six derived codes open until T13 anyway (`gateway.proto`, the
//! `GatewayError.Code` PLACEHOLDER).
//!
//! # Secrecy
//!
//! [`Surface::seat_state`] is the **only** way to a seat's notebook, drafts or
//! ready flag, and it takes the [`Subject`] asking. A subject that is not that
//! seat is refused, every time, for every method -- so `admin` cannot read
//! another seat's draft, and neither can another seat, and neither can a
//! spectator. There is no second path to the private store for a handler to
//! reach for.

use crate::audit::{AuditLog, Outcome};
use crate::error::Error;
use crate::feed::{Event, SegmentFeed, SnapshotId};
use crate::fog::{FogFilter, FogPolicy, Viewer, Vision};
use crate::limit::{Limits, RateLimiter};
use crate::rpc::{self, Request};
use crate::scopes::{self, Scope};
use crate::time::MatchTime;
use crate::token::{Grant, Handle, Subject, Token, TokenStore};
use pharmakos_proto::gp::api::v1::Method;
use pharmakos_proto::json::Json;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::tables::SeatId;

/// One saved draft, as [`crate::feed`] is to events: the shape, not the
/// contents. T13 fills the body in with a playbook.
///
/// The field names are `gp.api.v1.DraftSummary`'s, verbatim, which is what the
/// `struct_field_names` allowance below is for: the schema is the contract and
/// renaming a field here to please a lint would put a translation table between
/// this struct and the JSON it becomes.
#[derive(Clone, PartialEq, Eq, Debug)]
#[allow(
    clippy::struct_field_names,
    reason = "the names are gp.api.v1.DraftSummary's own"
)]
pub struct Draft {
    /// The draft's id, as the client names it.
    pub draft_id: String,
    /// What the seat called it.
    pub label: String,
    /// Which round it was saved in. A draft is bound to a match.
    pub round: u32,
}

/// One seat's private state.
///
/// Reached only through [`Surface::seat_state`] and
/// [`Surface::seat_state_mut`], both of which take the subject asking.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SeatState {
    /// Which seat this is.
    pub seat: SeatId,
    /// The private seat notebook (spec section 12, "Memory").
    pub notebook: String,
    /// Saved drafts, in the order they were saved.
    pub drafts: Vec<Draft>,
    /// Whether the seat has said it is ready.
    pub ready: bool,
}

impl SeatState {
    /// An empty private store for a seat.
    #[must_use]
    pub const fn new(seat: SeatId) -> SeatState {
        SeatState {
            seat,
            notebook: String::new(),
            drafts: Vec::new(),
            ready: false,
        }
    }
}

/// The gateway surface for one match.
#[derive(Debug)]
pub struct Surface {
    match_id: String,
    match_seed: u64,
    rules: RulesTable,
    time: MatchTime,
    tokens: TokenStore,
    fog: FogPolicy,
    feed: SegmentFeed,
    audit: AuditLog,
    limits: Limits,
    limiters: Vec<(Handle, RateLimiter)>,
    seats: Vec<SeatState>,
}

impl Surface {
    /// A surface for one match, with one private store per seat.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] for a match id
    /// [`crate::cache::MatchCache::valid_match_id`] refuses -- the same gate the
    /// filesystem uses, applied here so a match whose id could not be written
    /// down never starts.
    pub fn new(
        match_id: &str,
        match_seed: u64,
        rules: RulesTable,
        fog: FogPolicy,
        seats: &[SeatId],
    ) -> Result<Surface, Error> {
        if !crate::cache::MatchCache::valid_match_id(match_id) {
            return Err(Error::invalid(format!(
                "`{match_id}` is not a match id this gateway will host"
            )));
        }
        let mut slots: Vec<SeatState> = seats.iter().copied().map(SeatState::new).collect();
        slots.sort_by_key(|slot| slot.seat.raw());
        slots.dedup_by_key(|slot| slot.seat.raw());
        Ok(Surface {
            match_id: match_id.to_owned(),
            match_seed,
            rules,
            time: MatchTime::lobby(),
            tokens: TokenStore::new(),
            fog,
            feed: SegmentFeed::new(SnapshotId::of(match_seed, 0, 0)),
            audit: AuditLog::new(),
            limits: Limits::default(),
            limiters: Vec::new(),
            seats: slots,
        })
    }

    /// The match this surface hosts.
    #[must_use]
    pub fn match_id(&self) -> &str {
        &self.match_id
    }

    /// The match seed, which is also the map seed.
    #[must_use]
    pub const fn match_seed(&self) -> u64 {
        self.match_seed
    }

    /// Where the match is in time.
    #[must_use]
    pub const fn time(&self) -> MatchTime {
        self.time
    }

    /// The host moves the match on. The gateway never advances time itself: it
    /// has no clock to advance it by (AGENTS.md section 4.5).
    pub fn set_time(&mut self, time: MatchTime) {
        self.time = time;
    }

    /// The fog policy, for a host that is about to eliminate a seat or end the
    /// match.
    #[must_use]
    pub fn fog(&mut self) -> &mut FogPolicy {
        &mut self.fog
    }

    /// The token store, for the lobby.
    #[must_use]
    pub fn tokens(&mut self) -> &mut TokenStore {
        &mut self.tokens
    }

    /// The audit log.
    #[must_use]
    pub fn audit(&mut self) -> &mut AuditLog {
        &mut self.audit
    }

    /// Set the rate limits, for a host or a test that wants tighter ones than
    /// the PLACEHOLDER defaults.
    pub fn set_limits(&mut self, limits: Limits) {
        self.limits = limits;
        self.limiters.clear();
    }

    /// Start a new segment: a new feed, a new snapshot, and every cursor from
    /// the old segment now stale.
    pub fn begin_segment(&mut self, round: u32, segment: u32) {
        self.feed = SegmentFeed::new(SnapshotId::of(self.match_seed, round, segment));
    }

    /// Put an event on the segment's bus.
    ///
    /// # Errors
    ///
    /// As [`SegmentFeed::publish`].
    pub fn publish(&mut self, event: Event) -> Result<(), Error> {
        self.feed.publish(event)
    }

    /// The segment's feed, unfiltered. For the host and the watch rig, never for
    /// a client.
    #[must_use]
    pub const fn feed(&self) -> &SegmentFeed {
        &self.feed
    }

    /// One seat's private state, if `subject` is that seat.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::ForbiddenScope`] when the subject is anybody else
    /// -- another seat, a spectator, or `admin`, which "can never read another
    /// seat's playbooks, drafts or knowledge" (spec section 12) --  and
    /// [`crate::error::Code::NotFound`] when the match has no such seat.
    pub fn seat_state(&self, subject: Subject, seat: SeatId) -> Result<&SeatState, Error> {
        Surface::check_own_seat(subject, seat)?;
        self.seats
            .iter()
            .find(|slot| slot.seat == seat)
            .ok_or_else(|| Error::not_found(format!("this match has no seat {}", seat.raw())))
    }

    /// One seat's private state, to change, if `subject` is that seat.
    ///
    /// # Errors
    ///
    /// As [`Surface::seat_state`].
    pub fn seat_state_mut(
        &mut self,
        subject: Subject,
        seat: SeatId,
    ) -> Result<&mut SeatState, Error> {
        Surface::check_own_seat(subject, seat)?;
        self.seats
            .iter_mut()
            .find(|slot| slot.seat == seat)
            .ok_or_else(|| Error::not_found(format!("this match has no seat {}", seat.raw())))
    }

    /// The one secrecy gate.
    fn check_own_seat(subject: Subject, seat: SeatId) -> Result<(), Error> {
        if subject.seat() == Some(seat) {
            return Ok(());
        }
        Err(Error::forbidden(format!(
            "{} may not read or write seat {}'s playbooks, drafts, notebook or knowledge: they \
             never leave the gateway for anybody but that seat (spec section 12)",
            subject.render(),
            seat.raw()
        )))
    }

    /// Store a draft for a seat.
    ///
    /// The host's way in until T13 builds `save_draft`, and the reason the
    /// secrecy tests have something to fail to read. It takes the subject like
    /// every other path to the private store.
    ///
    /// # Errors
    ///
    /// As [`Surface::seat_state_mut`].
    pub fn store_draft(
        &mut self,
        subject: Subject,
        seat: SeatId,
        draft: Draft,
    ) -> Result<(), Error> {
        let state = self.seat_state_mut(subject, seat)?;
        state.drafts.retain(|held| held.draft_id != draft.draft_id);
        state.drafts.push(draft);
        Ok(())
    }

    /// Answer one JSON-RPC request.
    ///
    /// `token` is what the client sent -- `None` when it sent none at all.
    /// `vision` is the host's answer to "what can this seat see", which the fog
    /// filter asks and the gateway never computes.
    ///
    /// Always returns a JSON-RPC response object: a refusal is an answer, and
    /// every attempt is in the audit log either way.
    pub fn call<V: Vision>(
        &mut self,
        token: Option<&Token>,
        request: &Request,
        vision: &V,
    ) -> Json {
        let action = format!("call {}", request.method);
        let (handle, subject, scopes) = match self.identify(token) {
            Ok(triple) => triple,
            Err(error) => {
                self.audit
                    .refused(self.time.tick, None, None, action, &error);
                return rpc::failure(&request.id, &error);
            }
        };
        // The limiter runs after the token is known and is logged against it: a
        // rate-limited call is an *authenticated* call, and a log line that said
        // only "somebody went too fast" would be the least useful line in the
        // file.
        if let Err(error) = self.admit(handle, self.time.tick) {
            self.audit
                .refused(self.time.tick, Some(subject), Some(handle), action, &error);
            return rpc::failure(&request.id, &error);
        }

        let outcome = self.serve(handle, subject, scopes, request, vision);
        match outcome {
            Ok(result) => {
                self.audit.record(
                    self.time.tick,
                    Some(subject),
                    Some(handle),
                    action,
                    Outcome::Ok,
                );
                rpc::success(&request.id, result)
            }
            Err(error) => {
                self.audit
                    .refused(self.time.tick, Some(subject), Some(handle), action, &error);
                rpc::failure(&request.id, &error)
            }
        }
    }

    /// Step 1: who is this.
    fn identify(
        &self,
        token: Option<&Token>,
    ) -> Result<(Handle, Subject, scopes::ScopeSet), Error> {
        let token = token.ok_or_else(|| {
            Error::unauthenticated("this call carried no token; the lobby mints one per seat")
        })?;
        let grant: &Grant = self
            .tokens
            .authenticate(token, &self.match_id, self.time.tick)?;
        Ok((grant.handle, grant.subject, grant.scopes))
    }

    /// Count one call against the token's own budget, making the limiter on
    /// first use.
    ///
    /// Ordered storage, walked and kept in handle order: no hash map anywhere
    /// near the gateway's state (AGENTS.md section 4.4).
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::RateLimited`] from [`RateLimiter::admit`].
    fn admit(
        &mut self,
        handle: Handle,
        tick: pharmakos_sim::math::quantity::Tick,
    ) -> Result<(), Error> {
        if !self.limiters.iter().any(|(held, _)| *held == handle) {
            self.limiters
                .push((handle, RateLimiter::with_limits(self.limits)));
            self.limiters.sort_by_key(|(held, _)| held.raw());
        }
        let index = self
            .limiters
            .iter()
            .position(|(held, _)| *held == handle)
            .ok_or_else(|| Error::internal("the rate limiter lost a token it just made"))?;
        let (_, limiter) = self
            .limiters
            .get_mut(index)
            .ok_or_else(|| Error::internal("the rate limiter lost a token it just made"))?;
        limiter.admit(tick)
    }

    /// Steps 3 to 6.
    fn serve<V: Vision>(
        &mut self,
        _handle: Handle,
        subject: Subject,
        held: scopes::ScopeSet,
        request: &Request,
        vision: &V,
    ) -> Result<Json, Error> {
        let method = scopes::method_from_wire(&request.method)
            .ok_or_else(|| crate::error::unknown_method(&request.method))?;
        let needed = scopes::required(method).ok_or_else(|| {
            Error::internal(format!(
                "`{}` carries no required_scope annotation, so this build cannot say who may \
                 call it",
                request.method
            ))
        })?;
        if !held.holds(needed) {
            return Err(Error::forbidden(format!(
                "`{}` needs the `{}` scope, and this token holds `{}`",
                request.method,
                scopes::scope_wire_name(needed),
                held.render()
            )));
        }
        if planning(needed) && !self.time.planning_open() {
            return Err(Error::phase_closed(format!(
                "`{}` is a planning method, and planning is closed during the Push and the \
                 recap",
                request.method
            )));
        }

        let mut result = self.dispatch(method, subject, held, request, vision)?;
        if let Json::Object(entries) = &mut result {
            entries.push((String::from("_status"), self.time.footer()));
        }
        Ok(result)
    }

    /// Step 6: the method slice T9 needs to exercise the surface.
    fn dispatch<V: Vision>(
        &mut self,
        method: Method,
        subject: Subject,
        held: scopes::ScopeSet,
        request: &Request,
        vision: &V,
    ) -> Result<Json, Error> {
        match method {
            Method::GetStatus => Ok(Json::Object(vec![(
                String::from("status"),
                self.time.footer(),
            )])),
            Method::GetSegmentFeed => self.segment_feed(subject, held, request, vision),
            Method::SaveNotes => self.save_notes(subject, request),
            Method::ListDrafts => self.list_drafts(subject),
            Method::SetReady => self.set_ready(subject, request),
            Method::ListTemplates => Ok(Json::Object(vec![
                (String::from("templates"), Json::Array(Vec::new())),
                (String::from("next_cursor"), Json::String(String::new())),
            ])),
            other => Err(Error::internal(format!(
                "`{}` is in the schema and this build does not serve it yet: the method slice \
                 is T13's, on the surface T9 froze",
                scopes::method_wire_name(other)
            ))),
        }
    }

    /// `save_notes`: the private seat notebook, at the rules table's size.
    fn save_notes(&mut self, subject: Subject, request: &Request) -> Result<Json, Error> {
        let seat = subject
            .seat()
            .ok_or_else(|| Error::forbidden("only a seat has a notebook"))?;
        let notes = request
            .string_param("notes")?
            .ok_or_else(|| Error::invalid("`notes` is the notebook's new contents"))?;
        // Tuning values come from the rules table, never from a constant
        // (AGENTS.md section 12): `verifier.notebook_max_chars`, 4,000 today.
        let maximum = self
            .rules
            .message()
            .verifier
            .as_ref()
            .map(|verifier| verifier.notebook_max_chars)
            .ok_or_else(|| {
                Error::internal(
                    "this rules table has no verifier block, so the notebook has no size",
                )
            })?;
        let characters = u32::try_from(notes.chars().count()).unwrap_or(u32::MAX);
        if characters > maximum {
            return Err(Error::invalid(format!(
                "the notebook holds {maximum} characters and this is {characters}; it is never \
                 silently truncated"
            )));
        }
        let state = self.seat_state_mut(subject, seat)?;
        notes.clone_into(&mut state.notebook);
        Ok(Json::Object(vec![(
            String::from("characters"),
            Json::Number(characters.to_string()),
        )]))
    }

    /// `list_drafts`: the seat's own, and nobody else's.
    fn list_drafts(&self, subject: Subject) -> Result<Json, Error> {
        let seat = subject
            .seat()
            .ok_or_else(|| Error::forbidden("only a seat has drafts"))?;
        let state = self.seat_state(subject, seat)?;
        let drafts: Vec<Json> = state
            .drafts
            .iter()
            .map(|draft| {
                Json::Object(vec![
                    (
                        String::from("draft_id"),
                        Json::String(draft.draft_id.clone()),
                    ),
                    (String::from("label"), Json::String(draft.label.clone())),
                    (String::from("round"), Json::Number(draft.round.to_string())),
                ])
            })
            .collect();
        Ok(Json::Object(vec![
            (String::from("drafts"), Json::Array(drafts)),
            (String::from("next_cursor"), Json::String(String::new())),
        ]))
    }

    /// `set_ready`: the seat says the Lull may end.
    fn set_ready(&mut self, subject: Subject, request: &Request) -> Result<Json, Error> {
        let seat = subject
            .seat()
            .ok_or_else(|| Error::forbidden("only a seat is ready"))?;
        let ready = request.bool_param("ready")?.unwrap_or(true);
        let state = self.seat_state_mut(subject, seat)?;
        state.ready = ready;
        let footer = self.time.footer();
        Ok(Json::Object(vec![(String::from("status"), footer)]))
    }

    /// `get_segment_feed`: fog-filtered events, 60-second digests, an opaque
    /// cursor.
    fn segment_feed<V: Vision>(
        &self,
        subject: Subject,
        held: scopes::ScopeSet,
        request: &Request,
        vision: &V,
    ) -> Result<Json, Error> {
        let viewer = match subject {
            Subject::Seat(seat) => Viewer::Seat(seat),
            Subject::Spectator => Viewer::Spectator {
                nofog: held.holds(Scope::SpectateNofog),
            },
            Subject::Admin => Viewer::Admin,
        };
        let filter = FogFilter::new(&self.fog, vision);
        let cursor = request.string_param("cursor")?;
        let limit = request
            .integer_param("limit")?
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(crate::feed::MAX_PAGE_EVENTS);
        let page = self.feed.page(viewer, &filter, cursor, limit)?;

        let events: Vec<Json> = page
            .events
            .iter()
            .map(|event| {
                Json::Object(vec![
                    (
                        String::from("at_ms"),
                        Json::Number(event.at_ms.raw().to_string()),
                    ),
                    (
                        String::from("kind"),
                        Json::String(event.kind.name().to_owned()),
                    ),
                    (String::from("text"), Json::String(event.text.clone())),
                ])
            })
            .collect();
        let digests: Vec<Json> = page
            .digests
            .iter()
            .map(|digest| {
                let counts: Vec<Json> = digest
                    .counts
                    .iter()
                    .map(|(kind, count)| {
                        Json::Object(vec![
                            (String::from("kind"), Json::String(kind.name().to_owned())),
                            (String::from("count"), Json::Number(count.to_string())),
                        ])
                    })
                    .collect();
                Json::Object(vec![
                    (
                        String::from("from_ms"),
                        Json::Number(digest.from_ms.raw().to_string()),
                    ),
                    (
                        String::from("to_ms"),
                        Json::Number(digest.to_ms.raw().to_string()),
                    ),
                    (String::from("text"), Json::String(digest.text.clone())),
                    (String::from("counts"), Json::Array(counts)),
                ])
            })
            .collect();

        Ok(Json::Object(vec![
            (String::from("events"), Json::Array(events)),
            (String::from("digests"), Json::Array(digests)),
            (
                String::from("next_cursor"),
                Json::String(page.next_cursor.render()),
            ),
        ]))
    }
}

/// True for a scope whose methods are planning methods.
///
/// Derived from the scope rather than from a list of method names, so a method
/// added at T13 is phase-gated by the annotation it already carries.
const fn planning(scope: Scope) -> bool {
    matches!(scope, Scope::Plan | Scope::PlanSubmit)
}

#[cfg(test)]
mod tests {
    use super::{Draft, Surface};
    use crate::error::Code;
    use crate::fog::{Blind, FogPolicy};
    use crate::rpc;
    use crate::scopes::{Scope, ScopeSet};
    use crate::time::MatchTime;
    use crate::token::{Subject, Token};
    use pharmakos_proto::gp::api::v1::status::Phase;
    use pharmakos_proto::json::Json;
    use pharmakos_sim::math::quantity::{Ms, Tick};
    use pharmakos_sim::rules::RulesTable;
    use pharmakos_sim::tables::SeatId;
    use std::path::Path;

    fn rules() -> RulesTable {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("rules")
            .join("rules.v1.json");
        RulesTable::load(&path).expect("the shipped rules table")
    }

    fn surface() -> Surface {
        let mut surface = Surface::new(
            "m-0001",
            0x00ca_5cad_ed00_0001,
            rules(),
            FogPolicy::fogged(),
            &[SeatId::new(0), SeatId::new(1)],
        )
        .expect("a match id");
        surface.set_time(MatchTime {
            tick: Tick::new(10),
            phase: Phase::Lull,
            phase_remaining_ms: Ms::new(174_000),
            segment_length_ms: Ms::new(180_000),
            round: 1,
        });
        surface
    }

    fn seat_token(surface: &mut Surface, seat: u8) -> Token {
        let scopes = ScopeSet::of(&[Scope::Observe, Scope::Plan, Scope::PlanSubmit, Scope::Docs]);
        let (token, _) = surface
            .tokens()
            .mint(
                Subject::Seat(SeatId::new(seat)),
                "m-0001",
                scopes,
                Tick::ZERO,
            )
            .expect("minted");
        token
    }

    fn request(method: &str, params: &str) -> rpc::Request {
        let text = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"{method}","params":{params}}}"#);
        rpc::parse(&text).expect("well formed")
    }

    fn result(response: &Json) -> Json {
        response.get("result").cloned().unwrap_or(Json::Null)
    }

    fn code(response: &Json) -> String {
        response
            .get("error")
            .and_then(|error| error.get("data"))
            .and_then(|data| data.get("code"))
            .map_or_else(
                || String::from("<no error>"),
                |value| match value {
                    Json::String(text) => text.clone(),
                    other => format!("{other:?}"),
                },
            )
    }

    #[test]
    fn a_call_with_no_token_is_unauthenticated_and_is_still_logged() {
        let mut surface = surface();
        let response = surface.call(None, &request("get_status", "{}"), &Blind);
        assert_eq!(code(&response), "UNAUTHENTICATED");
        assert_eq!(surface.audit().entries().len(), 1);
    }

    #[test]
    fn every_result_carries_the_status_footer() {
        let mut surface = surface();
        let token = seat_token(&mut surface, 0);
        let response = surface.call(Some(&token), &request("get_status", "{}"), &Blind);
        let result = result(&response);
        let footer = result.get("_status").expect("the footer");
        assert_eq!(
            footer.get("phase"),
            Some(&Json::String(String::from("lull")))
        );
        assert_eq!(
            footer.get("segment_length_ms"),
            Some(&Json::Number(String::from("180000")))
        );
    }

    #[test]
    fn an_unknown_method_never_reaches_a_handler() {
        let mut surface = surface();
        let token = seat_token(&mut surface, 0);
        let response = surface.call(Some(&token), &request("connect", "{}"), &Blind);
        assert_eq!(code(&response), "INVALID_ARGUMENT");
    }

    #[test]
    fn a_method_needs_the_scope_the_schema_annotates_it_with() {
        let mut surface = surface();
        let (token, _) = surface
            .tokens()
            .mint(
                Subject::Seat(SeatId::new(0)),
                "m-0001",
                ScopeSet::of(&[Scope::Observe]),
                Tick::ZERO,
            )
            .expect("minted");
        let response = surface.call(
            Some(&token),
            &request("save_notes", r#"{"notes":"hello"}"#),
            &Blind,
        );
        assert_eq!(code(&response), "FORBIDDEN_SCOPE");
        // And the same token may read status, which needs `observe`.
        let response = surface.call(Some(&token), &request("get_status", "{}"), &Blind);
        assert!(response.get("result").is_some());
    }

    #[test]
    fn planning_is_closed_during_the_push_and_the_recap() {
        let mut surface = surface();
        let token = seat_token(&mut surface, 0);
        for phase in [Phase::Push, Phase::Recap, Phase::Lobby, Phase::Ended] {
            let mut time = surface.time();
            time.phase = phase;
            surface.set_time(time);
            let response = surface.call(
                Some(&token),
                &request("save_notes", r#"{"notes":"hello"}"#),
                &Blind,
            );
            assert_eq!(code(&response), "PHASE_CLOSED", "{phase:?}");
            // Reading is open in every phase.
            let response = surface.call(Some(&token), &request("get_status", "{}"), &Blind);
            assert!(response.get("result").is_some(), "{phase:?}");
        }
    }

    #[test]
    fn the_notebooks_size_comes_from_the_rules_table() {
        let mut surface = surface();
        let token = seat_token(&mut surface, 0);
        let maximum = rules()
            .message()
            .verifier
            .as_ref()
            .map(|verifier| verifier.notebook_max_chars)
            .expect("the verifier block");
        assert_eq!(
            maximum, 4_000,
            "item 90's row; if this moved, so did tuning"
        );

        let notes = "a".repeat(usize::try_from(maximum).expect("fits"));
        let response = surface.call(
            Some(&token),
            &request("save_notes", &format!(r#"{{"notes":"{notes}"}}"#)),
            &Blind,
        );
        assert_eq!(
            result(&response).get("characters"),
            Some(&Json::Number(maximum.to_string()))
        );

        let notes = "a".repeat(usize::try_from(maximum).expect("fits").saturating_add(1));
        let response = surface.call(
            Some(&token),
            &request("save_notes", &format!(r#"{{"notes":"{notes}"}}"#)),
            &Blind,
        );
        assert_eq!(
            code(&response),
            "INVALID_ARGUMENT",
            "never a silent truncation"
        );
    }

    #[test]
    fn a_seat_reads_its_own_drafts_and_no_others() {
        let mut surface = surface();
        surface
            .store_draft(
                Subject::Seat(SeatId::new(0)),
                SeatId::new(0),
                Draft {
                    draft_id: String::from("d1"),
                    label: String::from("east push"),
                    round: 1,
                },
            )
            .expect("its own");
        let token = seat_token(&mut surface, 1);
        let response = surface.call(Some(&token), &request("list_drafts", "{}"), &Blind);
        let drafts = result(&response).get("drafts").cloned().expect("a listing");
        assert_eq!(
            drafts,
            Json::Array(Vec::new()),
            "seat 1 has none of its own"
        );
    }

    #[test]
    fn a_method_this_build_does_not_serve_says_so_without_pretending_the_caller_erred() {
        let mut surface = surface();
        let token = seat_token(&mut surface, 0);
        let response = surface.call(Some(&token), &request("get_briefing", "{}"), &Blind);
        assert_eq!(code(&response), "INTERNAL");
    }

    #[test]
    fn the_rate_limiter_refuses_and_the_refusal_is_logged() {
        let mut surface = surface();
        surface.set_limits(crate::limit::Limits {
            per_tick: 2,
            per_window: 100,
            window_ticks: 100,
        });
        let token = seat_token(&mut surface, 0);
        for _ in 0..2 {
            let response = surface.call(Some(&token), &request("get_status", "{}"), &Blind);
            assert!(response.get("result").is_some());
        }
        let response = surface.call(Some(&token), &request("get_status", "{}"), &Blind);
        assert_eq!(code(&response), "RATE_LIMITED");
        let last = surface.audit().entries().last().cloned().expect("logged");
        assert_eq!(last.outcome.render(), "RATE_LIMITED");
    }

    #[test]
    fn two_tokens_have_two_budgets() {
        let mut surface = surface();
        surface.set_limits(crate::limit::Limits {
            per_tick: 1,
            per_window: 100,
            window_ticks: 100,
        });
        let first = seat_token(&mut surface, 0);
        let second = seat_token(&mut surface, 1);
        assert!(
            surface
                .call(Some(&first), &request("get_status", "{}"), &Blind)
                .get("result")
                .is_some()
        );
        assert!(
            surface
                .call(Some(&second), &request("get_status", "{}"), &Blind)
                .get("result")
                .is_some(),
            "one token's budget is not another's"
        );
        assert_eq!(
            code(&surface.call(Some(&first), &request("get_status", "{}"), &Blind)),
            "RATE_LIMITED"
        );
    }

    #[test]
    fn set_ready_records_the_seats_own_flag() {
        let mut surface = surface();
        let token = seat_token(&mut surface, 1);
        let response = surface.call(
            Some(&token),
            &request("set_ready", r#"{"ready":true}"#),
            &Blind,
        );
        assert!(response.get("result").is_some());
        let state = surface
            .seat_state(Subject::Seat(SeatId::new(1)), SeatId::new(1))
            .expect("its own");
        assert!(state.ready);
        let other = surface
            .seat_state(Subject::Seat(SeatId::new(1)), SeatId::new(0))
            .expect_err("not its own");
        assert_eq!(other.code, Code::ForbiddenScope);
    }

    #[test]
    fn a_seat_the_match_does_not_have_is_not_found() {
        let surface = surface();
        let error = surface
            .seat_state(Subject::Seat(SeatId::new(7)), SeatId::new(7))
            .expect_err("no such seat");
        assert_eq!(error.code, Code::NotFound);
    }

    #[test]
    fn a_match_id_the_filesystem_would_refuse_never_starts_a_match() {
        let error = Surface::new(
            "../escape",
            0,
            rules(),
            FogPolicy::fogged(),
            &[SeatId::new(0)],
        )
        .expect_err("refused");
        assert_eq!(error.code, Code::InvalidArgument);
    }
}
