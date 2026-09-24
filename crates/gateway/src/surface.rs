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
//! # The method slice
//!
//! T9 built the security surface with no match behind it; T13 puts one there
//! and hangs every method the skeleton's clients call off the same seven steps.
//! The handlers live in two submodules, and **neither of them may step the
//! match** -- that is [`crate::host`]'s, in four calls, and
//! `tests/confinement.rs` asserts it over this crate's own source text
//! (AGENTS.md section 3 rule 2, "no dry runs"). A handler may **compile** a
//! playbook, which is a pure function of the playbook and the rules table
//! ([`planning::compile_playbook`]); sealing the result into the match is the
//! host's.
//!
//! # A sealed playbook reaches the match
//!
//! T13 stopped at [`Sealed`]: a verified submission went into the seat's own
//! private store and the hosted world never heard of it, so a Push played out
//! with every commander standing still. T13b closes that (decisions-log item
//! 103 (1)). `submit_plan` compiles the playbook at the door and holds the
//! compiled [`Plan`] with the seal; [`Surface::begin_push`] files the safe
//! playbook for any seat that sealed nothing and then seals **every** seat's
//! plan into the runner, in ascending seat id, while the match is still in its
//! Lull.
//!
//! | Module | Methods |
//! |---|---|
//! | here | `get_status`, `wait_for`, `save_notes`, `set_ready`, `get_segment_feed` |
//! | [`knowledge`] | `get_briefing`, `get_recap`, `list_beacons`, `get_beacon`, `get_map_summary`, `get_economy_forecast`, `estimate_route` |
//! | [`planning`] | `get_schema`, `list_templates`, `instantiate_template`, `verify_plan`, `render_plan`, `patch_plan`, `save_draft`, `list_drafts`, `get_safe_plan`, `submit_plan` |
//! | [`watch`] | `get_view` |
//! | [`control`] | `end_lull`, `advance_push`, `end_recap`, `report_host_clock` |
//!
//! [`control`] is the **one** exception to the paragraph above, and T16a
//! changed the rule to say so rather than working around it (decisions-log
//! item 107 (4)): the four `admin` methods it serves drive the live match,
//! because the gateway reads no clock and something has to. They may reach the
//! match only through this module's own three driving methods, and
//! `tests/confinement.rs` holds that line.
//!
//! Read methods take spec section 12's `detail` budget ([`crate::detail`]). The
//! parameter, its three rungs, its wire spelling and the two salience rules are
//! T9's; the per-method order is each method's own and is stated at the method.
//!
//! Four methods of the schema still answer [`crate::error::Code::Internal`] --
//! `query_area`, `list_known_enemies`, `get_reports` and `get_capabilities` --
//! and that is not an omission: `gateway.proto` says they have no
//! request/response pair at all, because no skeleton client calls them and the
//! stage that introduces what each reports on (S3's knowledge store, S4's
//! licences) is the stage that will know what the request takes. The reading of
//! the closed error set is T9's and is unchanged: the set has no
//! `NOT_IMPLEMENTED`, `INVALID_ARGUMENT` would tell a client to fix a request
//! that is perfectly well formed, and `INTERNAL` is the one code whose
//! definition -- "the gateway failed; never used to report anything the caller
//! could have avoided" -- is true of a method this build does not serve.
//!
//! # Advice, and the tokens the host minted for itself
//!
//! T18a (decisions-log item 111). The built-in operator advises each seat it
//! does not play at the start of every Lull, through [`crate::serve`], and
//! [`Surface::file_advice`] files what it said into that seat's own store: its
//! safe playbook, verified FULL and compiled there, and its suggestions for the
//! editor's wizard. `get_safe_plan` and `instantiate_template{suggested}`
//! answer from that store and only to that seat; [`Surface::begin_push`] files
//! that seat's own safe playbook, or the gateway's fallback with an audit line
//! saying why. Tokens the host minted for itself are registered with
//! [`Surface::register_in_process`]: their own rate, and for an advisor a
//! scratch view feed. Both methods are host-side, and `tests/confinement.rs`
//! holds that no handler names either.
//!
//! # Secrecy
//!
//! [`Surface::seat_state`] is the **only** way to a seat's notebook, drafts or
//! ready flag, and it takes the [`Subject`] asking. A subject that is not that
//! seat is refused, every time, for every method -- so `admin` cannot read
//! another seat's draft, and neither can another seat, and neither can a
//! spectator. There is no second path to the private store for a handler to
//! reach for.

pub mod control;
pub mod knowledge;
pub mod planning;
pub mod watch;

use crate::advice::Advice;
use crate::audit::{AuditLog, Outcome};
use crate::error::Error;
use crate::feed::{Event, Kind, SegmentFeed, SnapshotId};
use crate::fog::{Audience, FogFilter, FogPolicy, Viewer, Vision};
use crate::host::Host;
use crate::limit::{Limits, RateLimiter};
use crate::rpc::{self, Request};
use crate::scopes::{self, Scope};
use crate::time::MatchTime;
use crate::token::{Grant, Handle, Subject, Token, TokenStore};
use crate::viewfeed::ViewFeed;
use pharmakos_proto::gp::api::v1::status::Phase;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::gp::api::v1::{Method, VerifyReport};
use pharmakos_proto::json::Json;
use pharmakos_sim::interpreter::Plan;
use pharmakos_sim::math::quantity::{Ms, Tick};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::runner::{MatchPhase, TickReport};
use pharmakos_sim::tables::SeatId;
use pharmakos_verifier::Scope as VerifierScope;

/// One saved draft: the summary `gp.api.v1.DraftSummary` carries, and the
/// playbook it is a draft of.
///
/// The three summary fields are the schema's names verbatim, which is what the
/// `struct_field_names` allowance below is for: the schema is the contract and
/// renaming a field here to please a lint would put a translation table between
/// this struct and the JSON it becomes. `playbook_jsonc` is **not** in that
/// message and never travels in a listing -- a draft's body leaves the gateway
/// only for the seat that saved it, and only when that seat asks for it.
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
    /// The playbook, as the author wrote it: JSONC, comments and all.
    pub playbook_jsonc: String,
}

/// What a seat has sealed: the latest verified submission, and the report that
/// verified it.
///
/// Decisions-log item 5: "the latest verified submission replaces the previous
/// one, any number of times until the timer ends", so there is one of these per
/// seat and never a history. The safe playbook is filed only if nothing
/// verified was ever submitted.
///
/// It carries the **compiled** [`Plan`] as well as the text, because the text
/// is what a seat reads back and the plan is what the match plays. The compile
/// happens once, at the door that can still tell somebody it failed
/// ([`crate::surface::planning::compile_playbook`]); by the time
/// [`Surface::begin_push`] seals this into the runner there is nothing left
/// that can go wrong with it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Sealed {
    /// The playbook, as submitted.
    pub playbook_jsonc: String,
    /// `report_hash` of the FULL report that accepted it. Eight big-endian
    /// bytes; the same value the pre-check returned, which is what spec
    /// section 11's "byte-identical" promise means.
    pub report_hash: Vec<u8>,
    /// Which round it was sealed in.
    pub round: u32,
    /// True when the gateway filed this itself because the Lull ended with
    /// nothing sealed (spec section 14).
    pub filed_by_the_gateway: bool,
    /// The playbook as the interpreter runs it.
    pub plan: Plan,
}

/// Last round's playbook, pre-loaded as a draft and **re-verified against the
/// new snapshot**.
///
/// Spec section 13, "Draft continuity": "each Lull opens with last round's
/// playbook pre-loaded as an editable draft, re-verified against the new
/// snapshot so anything the round invalidated shows as a diagnostic straight
/// away". The re-verification happens when the Lull opens, not when the editor
/// asks, which is what "straight away" means.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Continuity {
    /// The draft the playbook was pre-loaded as.
    pub draft_id: String,
    /// Whether it still qualifies against the new snapshot.
    pub qualifies: bool,
    /// How many diagnostics the re-verification produced.
    pub diagnostics: u32,
    /// `report_hash` of that re-verification.
    pub report_hash: Vec<u8>,
}

/// The longest `wait_for` timeout a caller may name, in game milliseconds.
///
/// `gateway.proto` says "the gateway caps it", so it does -- and the cap is a
/// refusal rather than a silent clamp, because a client that asked to wait ten
/// minutes and was told "fine" would then read a `fired: false` at ten seconds
/// as the answer to a ten-minute question.
///
/// Sixty seconds is one digest window ([`crate::feed::DIGEST_PERIOD`]), which
/// is the longest a `feed_digest` wait can sensibly be, and well over a Lull's
/// longest useful poll.
///
/// PLACEHOLDER: 60 000 is a working number. OWNER settles it at hardening with
/// the rate limits, which is where the question of how often a client may poll
/// actually lives.
pub const MAX_WAIT_MS: i32 = 60_000;

/// The draft id last round's playbook is pre-loaded under.
///
/// A fixed name rather than a counter: there is exactly one carried draft per
/// Lull and a seat that saved its own draft called `carried` in the round
/// before would otherwise find two.
pub const CARRIED_DRAFT_ID: &str = "carried";

/// What the built-in operator advised a seat this round, as the gateway
/// filed it ([`Surface::file_advice`]).
///
/// Private to the seat like everything else in [`SeatState`]: a seat is told
/// its own suggestions and its own safe playbook and nobody else's
/// (`advice_for_one_seat_never_reaches_another`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Advised {
    /// Which round it is for. A Lull opening forgets it
    /// ([`Surface::open_lull`]), and every reader checks the round as well.
    pub round: u32,
    /// What the operator said, as it said it, less any suggestion that would
    /// not instantiate ([`Surface::file_advice`] drops and audits those).
    pub advice: Advice,
    /// The operator's safe playbook, verified FULL and compiled when it was
    /// filed. `None` when it did not qualify or would not compile, in which
    /// case the gateway's fallback is what this seat is shown and filed, and
    /// the audit log says why.
    pub safe: Option<Sealed>,
}

/// How the host treats a token it minted for itself: a built-in seat's or an
/// advisor's ([`crate::serve`]).
///
/// Registered by the host with [`Surface::register_in_process`], which no
/// method handler reaches (`tests/confinement.rs`), because a client that
/// could register its own token would give itself a rate.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InProcess {
    /// The token's own rate limit, [`crate::limit::IN_PROCESS_LIMITS`] in the
    /// host. Counted exactly as a socket's is; only the numbers differ.
    pub limits: Limits,
    /// Answer this token's `get_view` from a **scratch copy** of the view
    /// feed, thrown away afterwards.
    ///
    /// An advisor calls as the seat it advises, and the view feed keeps its
    /// derived state per viewer rather than per token: entity handles minted
    /// in the order a viewer first sees each thing, and the stamp a sight
    /// change moves (decisions-log item 111, H15). An advisor's `get_view` on
    /// the real feed would therefore write into the human's own feed state.
    /// On a scratch copy it reads exactly what that seat may see and leaves
    /// the human's next answer byte for byte as it would have been
    /// (`the_advisor_leaves_the_humans_view_feed_as_it_found_it`).
    pub scratch_view: bool,
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
    /// The latest verified submission, if there is one.
    pub sealed: Option<Sealed>,
    /// What the carried draft's re-verification found, if there is one.
    pub continuity: Option<Continuity>,
    /// What the built-in operator advised this seat this round, if it
    /// advises this seat at all.
    pub advice: Option<Advised>,
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
            sealed: None,
            continuity: None,
            advice: None,
        }
    }

    /// The draft with this id, if the seat saved one.
    #[must_use]
    pub fn draft(&self, draft_id: &str) -> Option<&Draft> {
        self.drafts.iter().find(|draft| draft.draft_id == draft_id)
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
    /// The tokens the host minted for itself, and how each is treated. In
    /// handle order, like `limiters`.
    in_process: Vec<(Handle, InProcess)>,
    seats: Vec<SeatState>,
    /// The match, when one is hosted. `None` in the lobby, and in the tests
    /// that exercise the security surface without a world behind it.
    host: Option<Host>,
    /// The tick the current segment's feed was opened at. An event's `at_ms`
    /// is game milliseconds since this, which is what `gp.api.v1.Event.at_ms`
    /// means.
    feed_anchor: Tick,
    /// What the client last said was left of the Lull, in game milliseconds.
    /// The gateway never counts it down (decisions-log item 99).
    ///
    /// `None` until a client says: a host that has reported nothing is not the
    /// same as one reporting a Lull that has run out, and treating it as the
    /// second would put the gateway's tick a whole Lull ahead of the runner's
    /// before anybody had planned anything.
    lull_remaining_ms: Option<Ms>,
    /// Host time spent in the **current** phase, as `report_host_clock` last
    /// reported it. `None` until a client says, and cleared when the phase
    /// ends.
    ///
    /// Phase-neutral where `lull_remaining_ms` is a Lull's alone: a recap and
    /// an ended match spend no sim tick either, and a gateway whose tick stood
    /// still through both would hand every token
    /// [`crate::limit::CALLS_PER_TICK`] calls for the whole of each -- which
    /// is exactly where the full-map unlock lands (decisions-log item
    /// 107 (3)).
    reported_elapsed_ms: Option<Ms>,
    /// How much of **this** phase the client has reported spending, in ticks,
    /// as a high-water mark. A client that reports a *larger* remaining than
    /// before has not un-spent the time it already spent, so this never goes
    /// down within a phase. Reset to zero when the phase ends.
    phase_elapsed: u32,
    /// The ticks of host time that no sim tick covers: the sum of every Lull
    /// and every recap this match has finished. Added to the runner's tick for
    /// as long as the match lasts, which is what makes [`MatchTime::tick`]
    /// **monotonic** across a phase boundary. See [`Surface::sync_time`].
    lull_offset: u32,
    /// The view feed's derived, unhashed state ([`crate::viewfeed`]).
    views: ViewFeed,
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
            in_process: Vec::new(),
            seats: slots,
            host: None,
            feed_anchor: Tick::ZERO,
            lull_remaining_ms: None,
            reported_elapsed_ms: None,
            phase_elapsed: 0,
            lull_offset: 0,
            views: ViewFeed::new(),
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
    ///
    /// A token registered with [`Surface::register_in_process`] keeps its own.
    pub fn set_limits(&mut self, limits: Limits) {
        self.limits = limits;
        self.limiters.clear();
    }

    /// Mark a token as one the host minted for itself, with its own rate limit
    /// and, for an advisor, a scratch view feed ([`InProcess`]).
    ///
    /// **Host-side, and reached by no method handler**: `tests/confinement.rs`
    /// bans the name from every handler module. The token is still
    /// authenticated, scoped, phase-checked, fog-filtered and audited on every
    /// call; this changes the numbers its limiter counts against, and where an
    /// advisor's `get_view` is answered from, and nothing else.
    pub fn register_in_process(&mut self, handle: Handle, treatment: InProcess) {
        self.in_process.retain(|(held, _)| *held != handle);
        self.in_process.push((handle, treatment));
        self.in_process.sort_by_key(|(held, _)| held.raw());
        // A limiter made before the token was registered was made at the
        // default numbers; the next call makes it again at these.
        self.limiters.retain(|(held, _)| *held != handle);
    }

    /// How the host treats a token, if it minted it for itself.
    fn in_process_of(&self, handle: Handle) -> Option<InProcess> {
        self.in_process
            .iter()
            .find(|(held, _)| *held == handle)
            .map(|(_, treatment)| *treatment)
    }

    /// Start a new segment: a new feed, a new snapshot, and every cursor from
    /// the old segment now stale.
    pub fn begin_segment(&mut self, round: u32, segment: u32) {
        self.feed = SegmentFeed::new(SnapshotId::of(self.match_seed, round, segment));
        // The anchor is the SIM's tick, never [`MatchTime::tick`]: an event's
        // `at_ms` is game milliseconds since the segment began, and the sim
        // stamps its events with sim ticks. `MatchTime::tick` also counts the
        // Lull (see [`Surface::sync_time`]), and subtracting one from the other
        // would date every line of the feed by however long the Lull ran.
        self.feed_anchor = self
            .host
            .as_ref()
            .map_or(self.time.tick, |host| host.runner().tick());
    }

    // ---------------------------------------------------------------------
    // The match host
    // ---------------------------------------------------------------------

    /// Host a match on this surface.
    ///
    /// The opening events -- `match_started` and `lull_opened` -- are drained
    /// onto the first segment's feed here, so a client that connects before the
    /// first Push still reads how the match began.
    ///
    /// # The seat lists have to agree, and this is where that is checked
    ///
    /// A surface's seats come from [`Surface::new`] and a world's from the
    /// lobby's [`pharmakos_sim::world::WorldConfig`], and before T13b nothing
    /// compared them: a surface holding a seat the world has not got was inert,
    /// because nothing the surface did ever reached the world with a seat id.
    /// Sealing does. A mismatch would now be found at
    /// [`Surface::begin_push`] — as [`crate::error::Code::Internal`], one phase
    /// after it was made, with the safe playbooks already filed — so it is
    /// refused here instead, where the two lists are being put together and the
    /// message can name both counts.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] when a match is already hosted:
    /// a surface hosts one match, and replacing it would leave every cursor,
    /// every draft and every sealed playbook pointing at a world that is gone.
    /// And [`crate::error::Code::InvalidArgument`] for a host whose world does
    /// not have every seat this surface was built for.
    pub fn attach(&mut self, host: Host) -> Result<(), Error> {
        if self.host.is_some() {
            return Err(Error::invalid(
                "this surface already hosts a match; a second one is a second surface",
            ));
        }
        let in_world = host.world().seats().len();
        if let Some(beyond) = self
            .seats
            .iter()
            .find(|slot| u32::from(slot.seat.raw()) >= in_world)
        {
            return Err(Error::invalid(format!(
                "this surface was built for seat {} and the match it is being given has {in_world} \
                 seats: a seat the world has not got can be sealed for, and nothing would play it",
                beyond.seat.raw()
            )));
        }
        let round = host.runner().round();
        self.host = Some(host);
        self.sync_time();
        self.begin_segment(round, 0);
        // The view opens over the world it is a view of: the generated map is
        // encoded once here and every keyframe of the match is served from it.
        // Minting the view's id here is also what makes every cursor issued
        // before this call stale, which is the answer T17's restore needs.
        {
            let Surface {
                views,
                host,
                match_id,
                match_seed,
                ..
            } = self;
            if let Some(host) = host.as_ref() {
                views.attach(host.world().voxels(), match_id, *match_seed);
            }
        }
        self.absorb_events()
    }

    /// The view feed, to read. The watch handler is the only thing that writes
    /// it, and no client reaches it except through `get_view`.
    #[must_use]
    pub const fn views(&self) -> &ViewFeed {
        &self.views
    }

    /// Game milliseconds since the current segment opened -- the epoch
    /// `gp.api.v1.Event.at_ms` and `gp.api.v1.GetViewResponse.at_ms` share, so
    /// that a recording can interleave view frames and feed events on one
    /// clock (S7). Zero in a Lull, because a Lull spends no game time.
    #[must_use]
    pub fn segment_elapsed_ms(&self) -> Ms {
        self.host.as_ref().map_or(Ms::ZERO, |host| {
            Ms::from_ticks(host.runner().tick().since(self.feed_anchor))
        })
    }

    /// True when a match is hosted.
    #[must_use]
    pub const fn has_host(&self) -> bool {
        self.host.is_some()
    }

    /// The hosted match, to read.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when no match is hosted. `INTERNAL`
    /// rather than `PHASE_CLOSED`: a lobby that let a seat call a match method
    /// before starting a match is a host that went wrong, and nothing the
    /// caller did could have avoided it.
    pub fn host(&self) -> Result<&Host, Error> {
        self.host
            .as_ref()
            .ok_or_else(|| Error::internal("this gateway is not hosting a match"))
    }

    /// The hosted match, to drive.
    ///
    /// # Errors
    ///
    /// As [`Surface::host`].
    pub fn host_mut(&mut self) -> Result<&mut Host, Error> {
        self.host
            .as_mut()
            .ok_or_else(|| Error::internal("this gateway is not hosting a match"))
    }

    /// The client's answer to "how long is left in this Lull".
    ///
    /// The gateway reads no clock (AGENTS.md section 4.5, decisions-log item
    /// 99), so the Lull's timer lives on the walled side of the wall and
    /// reaches this crate as a call. During a Push the number is **not** this
    /// one: game time decides, and [`Surface::sync_time`] computes it from the
    /// segment's own ticks.
    pub fn set_phase_remaining_ms(&mut self, remaining: Ms) {
        self.lull_remaining_ms = Some(remaining);
        self.sync_time();
    }

    /// The host's own clock for the phase the match is in, as
    /// `report_host_clock` reports it.
    ///
    /// `elapsed` is host time spent in the **current** phase and is what moves
    /// [`MatchTime::tick`] in every phase that spends no sim tick;
    /// `remaining` is what the `_status` footer shows as left, which only a
    /// Lull has.
    ///
    /// # The input is client-attested, and is bounded rather than trusted
    ///
    /// The gateway has no clock to check this against (AGENTS.md section
    /// 4.5), and the rate limiter counts in the ticks this number moves. So
    /// the two properties a clock has are required rather than assumed:
    /// **monotone within a phase** -- time already spent is not un-spent --
    /// and **no single report further on than
    /// [`crate::surface::control::MAX_CLOCK_STEP_MS`]**, which turns "one call
    /// refills any number of windows" into "a client that stalled reports in
    /// steps". Both are refused rather than clamped, and the refusal is
    /// audited like any other.
    ///
    /// `remaining` is bounded by the same argument from the other end. It is
    /// not a budget the limiter spends, but it **is** what every seat reads
    /// off the `_status` footer as the time it has left to plan, so an
    /// unbounded one would let the lobby tell a seat its Lull lasts a day.
    /// The bound is the Lull's own declared length, `rules.match.lull_ms` --
    /// a tuning value read from the rules table and not a constant of this
    /// crate's (AGENTS.md section 12) -- and it is refused rather than
    /// clamped like the other three.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] for a negative, backwards or
    /// jumping clock, and for a countdown longer than the Lull itself.
    pub fn set_host_clock(&mut self, elapsed: Ms, remaining: Ms) -> Result<(), Error> {
        if elapsed.raw() < 0 || remaining.raw() < 0 {
            return Err(Error::invalid(
                "a clock is reported in game milliseconds and neither figure is ever negative",
            ));
        }
        let last = self.reported_elapsed_ms.unwrap_or(Ms::ZERO);
        if elapsed < last {
            return Err(Error::invalid(format!(
                "this phase's clock was last reported at {} ms and this report is {} ms: time \
                 already spent in a phase is not un-spent, and a backwards clock is refused \
                 rather than clamped",
                last.raw(),
                elapsed.raw()
            )));
        }
        let step = elapsed.raw().saturating_sub(last.raw());
        if step > crate::surface::control::MAX_CLOCK_STEP_MS {
            return Err(Error::invalid(format!(
                "one report may carry this phase's clock at most {} ms further on and this \
                 carries it {step} ms; after a long stall, report in steps",
                crate::surface::control::MAX_CLOCK_STEP_MS
            )));
        }
        let lull_ms = self.lull_ms();
        if remaining.raw() > lull_ms {
            return Err(Error::invalid(format!(
                "`remaining_ms` is what is left of a Lull and a Lull is {lull_ms} ms long; this \
                 reports {} ms left, which is refused rather than clamped",
                remaining.raw()
            )));
        }
        self.reported_elapsed_ms = Some(elapsed);
        if self.time.phase == Phase::Lull {
            self.lull_remaining_ms = Some(remaining);
        }
        self.sync_time();
        Ok(())
    }

    /// True when every seat of the match has said it is ready.
    ///
    /// What the host polls to decide whether to end the Lull early. The other
    /// way a Lull ends is the client's timer, which is why this is a question
    /// rather than an action.
    #[must_use]
    pub fn all_ready(&self) -> bool {
        !self.seats.is_empty() && self.seats.iter().all(|seat| seat.ready)
    }

    /// Open a Lull: carry last round's playbook forward, re-verify it against
    /// the new snapshot, and clear every ready flag.
    ///
    /// Spec section 13, "Draft continuity". The re-verification happens **here**
    /// rather than when the editor asks, because "straight away" is the whole
    /// promise: a player opening the editor should already be looking at what
    /// last round invalidated.
    ///
    /// Called by the host after [`Surface::end_recap`], and after
    /// [`Surface::attach`] for the opening Lull -- where it carries nothing,
    /// because there is no last round.
    ///
    /// # Errors
    ///
    /// As [`Surface::host`], and [`crate::error::Code::Internal`] when the
    /// frozen snapshot will not encode.
    pub fn open_lull(&mut self) -> Result<(), Error> {
        self.absorb_events()?;
        self.host_mut()?.refresh_routes()?;
        self.sync_time();
        let round = self.host()?.runner().round();
        let seats: Vec<SeatId> = self.seats.iter().map(|seat| seat.seat).collect();
        for seat in seats {
            self.carry_draft_forward(seat, round)?;
        }
        for seat in &mut self.seats {
            seat.ready = false;
            // Last round's advice was about last round's snapshot. The host
            // asks again once this Lull is open (`crate::serve`).
            seat.advice = None;
        }
        // A Lull opening is a moment a view can change without a tick: the
        // phase moved, and the recap's world is now the planning world.
        self.views.bump();
        Ok(())
    }

    /// Begin the Push: file the safe playbook for a seat that sealed nothing,
    /// **seal every seat's plan into the match**, open a fresh segment feed,
    /// and step the runner into the Push.
    ///
    /// Returns false when the runner was not in a Lull, **and does nothing at
    /// all in that case**.
    ///
    /// # The order of these five steps is the whole of the method
    ///
    /// 0. **Refuse out of phase, before touching anything.** The answer a host
    ///    that asked twice gets is `Ok(false)`, and it has been since T13. The
    ///    seal step below would otherwise reach it first and turn it into
    ///    [`crate::error::Code::Internal`] — blaming a seat for the host's
    ///    phase mistake — *after* the safe playbooks had been filed and the
    ///    running segment's feed replaced. The review found that; the guard is
    ///    the answer, and the `Internal` at step 4 keeps the case it is
    ///    actually described for: a refusal the phase did not predict.
    /// 1. **Drain** whatever the last phase left on the bus, onto the feed it
    ///    belongs to.
    /// 2. **File the safe playbook** for every seat whose seal is not this
    ///    round's (spec section 14). After this every seat of the match has a
    ///    [`Sealed`], so step 4 is unconditional.
    /// 3. **Open the segment's feed**, so that the `plan_sealed` lines land on
    ///    the segment they are the opening of rather than on the one that has
    ///    just closed.
    /// 4. **Seal**, in ascending seat id, while the runner is still in its Lull
    ///    — which is the only phase [`pharmakos_sim::runner::Runner::seal_playbook`]
    ///    accepts. The plans were compiled at submit, so nothing here can fail
    ///    for a reason a seat could have fixed: a refusal at this point is the
    ///    gateway's own bug and is [`crate::error::Code::Internal`]
    ///    (decisions-log item 103 (1)).
    /// 5. **Open the Push.**
    ///
    /// # Errors
    ///
    /// As [`Surface::host`], plus [`crate::error::Code::Internal`] when a
    /// filed safe playbook will not compile or the runner refuses a seal.
    pub fn begin_push(&mut self) -> Result<bool, Error> {
        if self.host()?.runner().phase() != MatchPhase::Lull {
            return Ok(false);
        }
        self.absorb_events()?;
        let round = self.host()?.runner().round();
        self.file_safe_playbooks(round)?;
        self.begin_segment(round, 0);
        let plans = self.plans_for_round();
        self.host_mut()?.seal_plans(plans)?;
        let started = self.host_mut()?.begin_push();
        if started {
            self.close_phase();
            self.views.bump();
        }
        self.sync_time();
        self.absorb_events()?;
        Ok(started)
    }

    /// Every seat's sealed plan, in ascending seat id.
    ///
    /// Ascending because the order a match is given its orders in is part of
    /// what makes a match reproducible (AGENTS.md section 4.6): the seals emit
    /// events and reset per-seat state, and "ties to the lowest seat id" is the
    /// project's standing convention. `self.seats` is held sorted by
    /// [`Surface::new`], and this sorts again rather than relying on that from
    /// a distance.
    ///
    /// A seat with nothing sealed is skipped rather than defaulted, which after
    /// [`Surface::file_safe_playbooks`] means no seat at all — but this is also
    /// what a caller gets before a match has a Lull behind it, and a `None`
    /// there is "nothing to file", not "file nothing".
    ///
    /// PLACEHOLDER: **this is also what a restore has to call.** A plan is an
    /// *input* and is not in a snapshot (T11: "the sim is a pure function of
    /// (map seed, playbooks, rules hash) and an input is not state"), so a
    /// runner restored from a save file has the interpreter's *state* back and
    /// no plans behind it — and would play the rest of the segment with every
    /// commander standing still, which is exactly the bug this task removes.
    /// The gateway is where the plans still are: this store. Saves and restores
    /// are **T17**'s and there is no restore path in this crate today, so there
    /// is nothing here to wire; when T17 adds one it re-seals from this
    /// function, after the restore and before the first tick, and the plan
    /// fingerprint T11's `Interpreter::restore` PLACEHOLDER asks for is what
    /// tells it the save and the store agree. **T17.**
    fn plans_for_round(&self) -> Vec<(SeatId, Plan)> {
        let mut plans: Vec<(SeatId, Plan)> = self
            .seats
            .iter()
            .filter_map(|seat| {
                seat.sealed
                    .as_ref()
                    .map(|sealed| (seat.seat, sealed.plan.clone()))
            })
            .collect();
        plans.sort_by_key(|(seat, _)| seat.raw());
        plans
    }

    /// One tick of the Push, with the events it produced on the feed, the fog
    /// policy brought up to date, and the view feed stamped.
    ///
    /// # Three things happen after the tick, and all three are one sentence
    ///
    /// **The fog policy follows the world.** Spec section 12 unlocks a seat's
    /// fog on elimination and everyone's at match end, and no token is
    /// reissued for either. Match end had a caller since T13;
    /// [`FogPolicy::eliminate`] had none at all until T16a, so a seat that
    /// lost its last beacon went on seeing a fogged world for the rest of the
    /// match.
    ///
    /// **The view feed is stamped.** `VoxelStore::settled()` is the sim's own
    /// answer to "which chunks did this tick write", refreshed inside the tick
    /// that wrote them, so the gateway asks rather than deriving. The stamp
    /// moves first and the chunks are marked at the new stamp, so a cursor
    /// issued before this tick is strictly older than anything it wrote.
    ///
    /// **Neither is hashed.** Both are derived state; see
    /// [`crate::viewfeed`].
    ///
    /// # Errors
    ///
    /// As [`Surface::host`].
    pub fn step(&mut self) -> Result<Option<TickReport>, Error> {
        let report = self.host_mut()?.step();
        self.sync_time();
        self.absorb_events()?;

        // Counted rather than collected, and read row by row rather than
        // gathered into a `Vec`: this is the hot path `advance_push` runs up
        // to 1 200 times in one call, on the one thread that owns the
        // surface, so it allocates nothing.
        let unlocked_before = self.fog.unlocked_count();
        let ended_before = self.fog.ended();
        if let Some(report) = report {
            if report.match_ended {
                self.fog.end_match();
            }
        }
        self.eliminate_the_fallen();
        let policy_moved =
            self.fog.unlocked_count() != unlocked_before || self.fog.ended() != ended_before;

        if report.is_some() || policy_moved {
            self.views.bump();
        }
        {
            let Surface { views, host, .. } = self;
            if let Some(host) = host.as_ref() {
                views.stamp(host.world().voxels().settled());
            }
        }
        Ok(report)
    }

    /// Lift the fog of every seat the world says is no longer standing.
    ///
    /// Read from [`pharmakos_sim::tables::SeatTable::is_alive`], which is the
    /// sim's own rule (T10: `Standing` / `Fallen` / `Unplaced`, and only
    /// `Fallen` is elimination), rather than from a rule of this crate's.
    /// [`FogPolicy::eliminate`] is idempotent, so a seat already unlocked
    /// costs a set lookup and nothing else.
    ///
    /// Does nothing when no match is hosted: a lobby has eliminated nobody.
    /// The two fields are borrowed apart rather than gathered through a
    /// `Vec`, because this runs once a tick.
    fn eliminate_the_fallen(&mut self) {
        let Surface { fog, host, .. } = self;
        let Some(host) = host.as_ref() else {
            return;
        };
        let seats = host.world().seats();
        for row in 0..seats.seats().len() {
            if seats.is_alive(row) {
                continue;
            }
            if let Some(raw) = seats.seats().get(row).copied() {
                fog.eliminate(SeatId::new(raw));
            }
        }
    }

    /// End the recap. Returns false when the runner was not in a recap.
    ///
    /// # Errors
    ///
    /// As [`Surface::host`].
    pub fn end_recap(&mut self) -> Result<bool, Error> {
        let ended = self.host_mut()?.end_recap();
        if ended && self.host()?.runner().phase() == MatchPhase::Ended {
            // Spec section 12 unlocks the fog at match end, and a match that
            // runs out of rounds ends **here** rather than on a tick:
            // `MatchState::close_recap` is what calls `decide` for
            // `RoundLimit`, so the last tick of the last segment reports
            // `match_ended: false` and [`Surface::step`]'s unlock never fires.
            // Found by T16a's own acceptance test; the one-tick rule's path
            // (a match decided *during* a Push) was already covered.
            self.fog.end_match();
        }
        if ended {
            // The recap's own reported clock is the recap's; the Lull that
            // opens next counts from whatever its client says. What the recap
            // spent is folded into the offset here, which is what keeps
            // `MatchTime::tick` monotonic across the boundary.
            self.close_phase();
            self.views.bump();
        }
        self.sync_time();
        self.absorb_events()?;
        Ok(ended)
    }

    /// The frozen planning snapshot's bytes, which are a hashed input of every
    /// verifier report (spec section 11).
    ///
    /// # Errors
    ///
    /// As [`Surface::host`], and [`crate::error::Code::Internal`] when the
    /// snapshot will not encode.
    pub fn snapshot_bytes(&self) -> Result<Vec<u8>, Error> {
        self.host()?
            .runner()
            .frozen()
            .snapshot()
            .to_bytes()
            .map_err(|error| {
                Error::internal(format!("the frozen snapshot would not encode: {error}"))
            })
    }

    /// One seat's sealed order.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::NoQualifyingPlan`] when the seat has sealed
    /// nothing -- the one code spec section 12 names for exactly this -- and
    /// [`crate::error::Code::ForbiddenScope`] when the subject is anybody but
    /// that seat.
    pub fn sealed_plan(&self, subject: Subject, seat: SeatId) -> Result<&Sealed, Error> {
        self.seat_state(subject, seat)?
            .sealed
            .as_ref()
            .ok_or_else(|| {
                Error::new(
                    crate::error::Code::NoQualifyingPlan,
                    format!("seat {} has sealed nothing this round", seat.raw()),
                )
            })
    }

    /// Read the match's own clock off the runner.
    ///
    /// Phase, round and the coming segment's length are facts of the match.
    /// `phase_remaining_ms` is two different things and is treated as two: in a
    /// **Push** it is game time, computed from the segment's own ticks; in a
    /// **Lull** or a recap it is the client's timer, because the gateway has no
    /// clock to count one with.
    ///
    /// # The tick counts the Lull too, and that is not a clock read
    ///
    /// A Lull consumes **no sim tick** (T10: the Lull and the recap end on the
    /// host's word), so a gateway whose tick were the runner's alone would sit
    /// at one tick for a whole three-minute Lull -- and everything this crate
    /// counts in ticks would sit with it. The rate limiter would give a seat
    /// [`crate::limit::CALLS_PER_TICK`] calls for the entire planning phase, to
    /// an editor that verifies on every edit (spec section 13); the audit log
    /// would stamp every call of the Lull with the same tick; a token's expiry
    /// would not move.
    ///
    /// So the tick here is the runner's **plus every tick of host time no sim
    /// tick covered**, derived from two numbers the gateway is given rather
    /// than from a clock it read: `rules.match.lull_ms`, which is the Lull's
    /// length, and the client's own answer to how much of it is left
    /// ([`Surface::set_phase_remaining_ms`]). That is decisions-log item 99's
    /// arrangement exactly -- "the Lull's timer stays on the client side of the
    /// wall" and what reaches the gateway is the host's answer, in ticks of
    /// host time it is given.
    ///
    /// # It is monotonic, and that is the whole of the arithmetic
    ///
    /// A Lull's elapsed ticks are **carried**, not recomputed: while a Lull
    /// runs they accumulate in `lull_elapsed` as a high-water mark, and when
    /// the Lull ends [`Surface::begin_push`] folds them into `lull_offset`,
    /// which never comes back down. Without that fold the reported tick would
    /// fall by a whole Lull -- 3 600 ticks at a three-minute one -- the instant
    /// the Push began, and [`RateLimiter::admit`] says in as many words that it
    /// treats a backwards tick as the host going wrong and refuses to refill a
    /// budget from it. A seat would get one tick's worth of calls for a whole
    /// Push. So: `runner.tick() + lull_offset + lull_elapsed`, in every phase,
    /// and the sum only ever rises.
    ///
    /// Two consequences, both deliberate. [`MatchTime::tick`] is **not** the
    /// sim's tick, so anything that has to be in sim-tick space reads
    /// `host.runner().tick()` instead -- [`Surface::begin_segment`] is the one
    /// place that does. And the derivation applies to the **Lull** and to no
    /// other phase: `rules.match.lull_ms` is a Lull's length and means nothing
    /// measured against a recap, which has no declared length at all and simply
    /// holds the tick still for as long as it lasts.
    /// `rules.match.lull_ms`: how long a Lull is declared to be.
    ///
    /// A tuning value, read from the rules table the surface was built with
    /// rather than written into this crate as a constant (AGENTS.md section
    /// 12). Zero when the table has no match settings, which is a table no
    /// shipped match runs on.
    fn lull_ms(&self) -> i32 {
        self.rules
            .message()
            .r#match
            .as_ref()
            .map_or(0, |settings| settings.lull_ms)
    }

    fn sync_time(&mut self) {
        let Some(host) = self.host.as_ref() else {
            return;
        };
        let runner = host.runner();
        let state = runner.world().match_state();
        let phase = match runner.phase() {
            MatchPhase::Lull => Phase::Lull,
            MatchPhase::Push => Phase::Push,
            MatchPhase::Recap => Phase::Recap,
            MatchPhase::Ended => Phase::Ended,
        };
        let in_push = runner.phase() == MatchPhase::Push;
        let in_lull = runner.phase() == MatchPhase::Lull;
        let reported = self.lull_remaining_ms;
        // PLACEHOLDER: what `_status.phase_remaining_ms` shows outside a Push
        // is the client's own report, and zero when there is none -- which
        // makes an UNTIMED Lull and a RECAP read identically to a Lull whose
        // timer has just run out. A recap has no declared length at all, and
        // `serve_report_host_clock` now refuses a countdown outside a Lull,
        // so the recap's figure is fixed at zero by this lane rather than
        // decided. Whether a recap and an untimed Lull should instead be a
        // distinguishable "no countdown" is **OWNER**, at **S1**, with the
        // recap screen.
        let remaining = if in_push {
            let elapsed = runner.tick().since(state.segment_started());
            Ms::from_ticks(state.segment_ticks().saturating_sub(elapsed))
        } else {
            reported.unwrap_or(Ms::ZERO)
        };
        let lull_ms = host
            .rules()
            .message()
            .r#match
            .as_ref()
            .map_or(0, |settings| settings.lull_ms);
        // In a Push the sim's own tick is the clock. Outside one, the host's
        // own report is taken first -- it is phase-neutral and is what a recap
        // and an ended match have -- and the Lull's countdown is the older
        // derivation kept for a client that reports only that (item 99's
        // arrangement, and what `set_phase_remaining_ms` still does).
        let elapsed_in_phase = if in_push {
            0
        } else if let Some(spent) = self.reported_elapsed_ms {
            spent.to_ticks_floor()
        } else {
            match (in_lull, reported) {
                (true, Some(left)) => {
                    Ms::new(lull_ms.saturating_sub(left.raw()).max(0)).to_ticks_floor()
                }
                _ => 0,
            }
        };
        let sim_tick = runner.tick().raw();
        let carried = self.lull_offset;
        self.phase_elapsed = self.phase_elapsed.max(elapsed_in_phase);
        self.time = MatchTime {
            // The sim's tick, plus every tick of host time no sim tick covered.
            // See the method's own doc for why that is not a clock read, and
            // why it is a running total rather than this Lull's own figure.
            tick: Tick::new(
                sim_tick
                    .saturating_add(carried)
                    .saturating_add(self.phase_elapsed),
            ),
            phase,
            phase_remaining_ms: remaining,
            segment_length_ms: runner.frozen().coming_segment_ms(),
            round: runner.round(),
        };
    }

    /// A phase that consumes no sim tick is over: carry what it spent, and
    /// forget its clock.
    ///
    /// Folding `phase_elapsed` into `lull_offset` is what keeps
    /// [`MatchTime::tick`] monotonic (see [`Surface::sync_time`]); clearing
    /// the two reported figures is what stops the next phase reporting the
    /// last one's leftover as its own, and what lets the monotone check in
    /// [`Surface::set_host_clock`] start again from zero.
    fn close_phase(&mut self) {
        self.lull_offset = self.lull_offset.saturating_add(self.phase_elapsed);
        self.phase_elapsed = 0;
        self.lull_remaining_ms = None;
        self.reported_elapsed_ms = None;
    }

    /// Drain the sim's event bus onto the segment feed.
    ///
    /// The translation is the fog filter's input, and [`Surface::audience_of`]
    /// decides it kind by kind.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when the sim emits a kind this crate's
    /// [`Kind`] rule refuses, which would mean the two spellings of item 97's
    /// rule had drifted apart; and when a kind that is one seat's private
    /// business arrives naming no seat, because there is then nobody it may
    /// be shown to and showing it to everybody is the wrong default.
    fn absorb_events(&mut self) -> Result<(), Error> {
        let Some(host) = self.host.as_mut() else {
            return Ok(());
        };
        let drained = host.drain_events();
        let anchor = self.feed_anchor;
        for event in drained {
            let kind = Kind::new(event.kind.name()).map_err(|error| {
                Error::internal(format!(
                    "the sim emitted `{}`, which this feed will not carry: {}",
                    event.kind.name(),
                    error.message
                ))
            })?;
            let audience = Self::audience_of(&event)?;
            let at_ms = Ms::from_ticks(event.tick.since(anchor));
            self.feed.publish(Event {
                at_ms,
                kind,
                text: crate::strings::event_text(&event),
                audience,
            })?;
        }
        Ok(())
    }

    /// Who one sim event is for.
    ///
    /// One arm per kind, so a kind added to the sim's bus fails to compile
    /// here rather than being shown to whoever the default happened to be --
    /// the same reason [`crate::strings::event_text`] matches exhaustively.
    ///
    /// * A match-wide line is [`Audience::Public`].
    /// * Something that happened to an asset in the world is
    ///   [`Audience::World`], owned by the seat it happened to and placed
    ///   where it happened, and fog decides. One that carries no place is
    ///   placed at the origin.
    /// * What the playbook interpreter reports -- the seal, a step, a rule, the
    ///   reflex, a visit and its rows, the fallback -- is
    ///   [`Audience::Private`] to the seat whose commander it is. Those lines
    ///   carry step and rule indices, which are the shape of a sealed
    ///   playbook, and a playbook never leaves the gateway for another seat
    ///   (AGENTS.md section 7): not through fog, not under a no-fog policy and
    ///   not to a spectator. `beacon_placed` is the exception among them,
    ///   because a beacon standing in the world is a thing an opponent with
    ///   eyes on the place can see.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when a private kind names no seat.
    fn audience_of(event: &pharmakos_sim::events::Event) -> Result<Audience, Error> {
        use pharmakos_sim::events::EventKind;
        match event.kind {
            EventKind::MatchStarted
            | EventKind::LullOpened
            | EventKind::PushStarted
            | EventKind::SegmentEnded
            | EventKind::RecapOpened
            | EventKind::MatchEnded
            | EventKind::SeatEliminated => Ok(Audience::Public),
            EventKind::BeaconDestroyed
            | EventKind::StructureRuined
            | EventKind::CommanderDied
            | EventKind::CommanderRespawned
            | EventKind::UnitSealedIn
            | EventKind::BeaconPlaced
            // T14's one world line. A finished Generator is a building
            // standing on a vent: an opponent with eyes on the place sees it,
            // exactly as it sees a placed beacon. Its *queuing* is not here -
            // that is a spend, and a spend is the seat's own business.
            | EventKind::StructureCompleted => Ok(Audience::World {
                owner: event.seat,
                at: event
                    .at
                    .map(crate::view::voxel_of_position)
                    .unwrap_or_default(),
            }),
            EventKind::PlanSealed
            | EventKind::StepStarted
            | EventKind::StepCompleted
            | EventKind::StepSkipped
            | EventKind::StepFailed
            | EventKind::RuleFired
            | EventKind::RuleEnded
            | EventKind::ReflexFired
            | EventKind::ReflexCleared
            | EventKind::VisitStarted
            | EventKind::RowCommitted
            | EventKind::VisitEnded
            | EventKind::FallbackEngaged
            // T14's economy lines, every one of them private, by item 103(10)'s
            // reasoning applied row by row: anything that reveals a seat's
            // orders, treasury, spend or power state is the seat's own.
            //
            // * `beacon_browned_out` and `beacon_revived` are **power state** -
            //   how close a seat is to its supply ceiling, which is what an
            //   attacker most wants to know and what the seat has no way to
            //   hide. The dome going dark is visible in the vista; the *line*
            //   naming the beacon is not.
            // * `unit_fabricated` and `structure_queued` are **spends**: both
            //   say what a seat decided to buy and therefore what its mandate
            //   is doing. The thing they buy becomes visible on its own terms -
            //   a unit through fog, a structure through `structure_completed`.
            // * `ore_delivered`, `salvage_delivered`, `beacon_recycled` and
            //   `settled` all carry a **treasury** figure in `value`.
            //   `beacon_recycled` is the one that had a case for being a world
            //   line, because a beacon leaving the world is visible - but its
            //   `value` is the refund, and the `beacon_destroyed` line that
            //   follows it on the same tick already tells an onlooker the
            //   beacon is gone.
            // * `kill_credited` names who was paid for a destruction, which is
            //   a seat's own share and nobody else's business until the recap.
            | EventKind::BeaconBrownedOut
            | EventKind::BeaconRevived
            | EventKind::UnitFabricated
            | EventKind::StructureQueued
            | EventKind::OreDelivered
            | EventKind::SalvageDelivered
            | EventKind::BeaconRecycled
            | EventKind::Settled
            | EventKind::KillCredited => event.seat.map(Audience::Private).ok_or_else(|| {
                Error::internal(format!(
                    "the sim emitted `{}` for no seat, and a seat's orders have no other audience",
                    event.kind.name()
                ))
            }),
        }
    }

    /// File the safe playbook for every seat that sealed nothing.
    ///
    /// Spec section 14 and decisions-log item 5: the safe playbook is filed
    /// **only** if nothing verified was ever submitted. A seat that sealed
    /// something keeps it, and one that sealed something in an earlier round
    /// does not: a seal is a round's, so a seat that planned in round 1 and
    /// submitted nothing in round 2 plays round 2 on the safe playbook — in the
    /// world as well as in this store, because [`Surface::begin_push`] seals
    /// whatever is here.
    ///
    /// # Whose safe playbook
    ///
    /// **The seat's own**, when the built-in operator advised it this round
    /// and its safe playbook qualified (decisions-log item 111, decision C5):
    /// that one was verified FULL and compiled when it was filed
    /// ([`Surface::file_advice`]), against the frozen snapshot this Lull has
    /// planned against all along, so it is filed here as it stands. Otherwise
    /// the gateway's fallback, [`crate::host::SAFE_PLAYBOOK`] — for a seat no
    /// operator advises, and for one whose advice did not qualify, which the
    /// audit log already says.
    ///
    /// The fallback is **compiled here**, and a failure is
    /// [`crate::error::Code::Internal`] and loud: if it will not compile then
    /// the one playbook that is supposed to be safe in every situation is not
    /// runnable in any, and filing it silently would hand the seat a Push in
    /// which its commander stands still.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when the fallback will not compile.
    fn file_safe_playbooks(&mut self, round: u32) -> Result<(), Error> {
        let Some(host) = self.host.as_ref() else {
            return Ok(());
        };
        let stale = |seat: &SeatState| seat.sealed.as_ref().is_none_or(|held| held.round != round);
        let own = |seat: &SeatState| {
            seat.advice
                .as_ref()
                .filter(|advised| advised.round == round)
                .and_then(|advised| advised.safe.clone())
        };
        let needs_fallback = self
            .seats
            .iter()
            .any(|seat| stale(seat) && own(seat).is_none());
        let fallback = if needs_fallback {
            let safe = host.safe_playbook().to_owned();
            let plan = crate::surface::planning::compile_playbook(&safe, host.rules()).map_err(
                |error| {
                    Error::internal(format!(
                        "the safe playbook this gateway files will not compile: {}",
                        error.message
                    ))
                },
            )?;
            Some(Sealed {
                playbook_jsonc: safe,
                // No report: this playbook did not come from a seat and was
                // not pre-checked by one. The Push's own verification is what
                // accepts it, and an invented hash here would be a hash
                // nothing produced.
                report_hash: Vec::new(),
                round,
                filed_by_the_gateway: true,
                plan,
            })
        } else {
            None
        };
        for seat in &mut self.seats {
            if !stale(seat) {
                continue;
            }
            let filed = own(seat).or_else(|| fallback.clone());
            if let Some(mut filed) = filed {
                filed.round = round;
                seat.sealed = Some(filed);
            }
        }
        Ok(())
    }

    /// File what the built-in operator advised one seat for this round.
    ///
    /// **Host-side.** [`crate::serve`] calls it once per Lull for each seat the
    /// operator does not play, with what that seat's [`crate::serve::Advisor`]
    /// returned; no method handler reaches it, and `tests/confinement.rs`
    /// bans the name from every handler module (decisions-log item 111,
    /// decision C5). It writes the seat's private store directly, as
    /// [`Surface::begin_push`]'s filing does, because the subject asking is
    /// the host rather than a token.
    ///
    /// The safe playbook is **verified FULL and compiled here**, against the
    /// frozen snapshot, which does not change for the rest of the Lull — so
    /// verifying it now and verifying it when the Lull ends are the same check,
    /// and `get_safe_plan` can answer exactly what would be filed. One that
    /// does not qualify, or qualifies and will not compile, is not stored:
    /// the seat is shown and filed the gateway's fallback, and the audit log
    /// gets a `file advice` line naming the code, so a bad advice is never
    /// replaced silently. A good one is logged `ok`.
    ///
    /// Each wizard suggestion is **instantiated once here** over the template
    /// it names; one that would not instantiate is dropped with a
    /// `file advice suggestion` line naming the code, so the human's
    /// `instantiate_template{suggested}` shows the template's own values
    /// rather than failing on the operator's mistake.
    ///
    /// # Errors
    ///
    /// As [`Surface::host`]; [`crate::error::Code::PhaseClosed`] outside a
    /// Lull, because advice is for a round that has not been sealed yet; and
    /// [`crate::error::Code::NotFound`] for a seat the match has not got.
    pub fn file_advice(&mut self, seat: SeatId, advice: Advice) -> Result<(), Error> {
        if self.host()?.runner().phase() != MatchPhase::Lull {
            return Err(Error::phase_closed(
                "advice is filed during a Lull, for the round about to be sealed",
            ));
        }
        if !self.seats.iter().any(|slot| slot.seat == seat) {
            return Err(Error::not_found(format!(
                "this match has no seat {}",
                seat.raw()
            )));
        }
        let round = self.host()?.runner().round();
        let tick = self.time.tick;
        let subject = Subject::Seat(seat);
        let checked = self.qualify_safe_playbook(seat, &advice.safe_playbook_jsonc, round);
        let safe = match checked {
            Ok(sealed) => {
                self.audit
                    .record(tick, Some(subject), None, "file advice", Outcome::Ok);
                Some(sealed)
            }
            Err(error) => {
                self.audit
                    .refused(tick, Some(subject), None, "file advice", &error);
                None
            }
        };
        let Advice {
            safe_playbook_jsonc,
            suggestions,
        } = advice;
        let mut kept = Vec::with_capacity(suggestions.len());
        for suggestion in suggestions {
            match self.check_suggestion(&suggestion) {
                Ok(()) => kept.push(suggestion),
                Err(error) => {
                    self.audit.refused(
                        tick,
                        Some(subject),
                        None,
                        // The id is the operator's text, so it stays out of
                        // the log's action column.
                        "file advice suggestion",
                        &error,
                    );
                }
            }
        }
        if let Some(slot) = self.seats.iter_mut().find(|slot| slot.seat == seat) {
            slot.advice = Some(Advised {
                round,
                advice: Advice {
                    safe_playbook_jsonc,
                    suggestions: kept,
                },
                safe,
            });
        }
        Ok(())
    }

    /// Whether one wizard suggestion instantiates, on its own, over the
    /// template it names.
    ///
    /// A suggestion that would not -- a template the library has not got, a
    /// value that is not JSON, a pointer that does not resolve -- is the
    /// operator's fault, and filing it would make the human's
    /// `instantiate_template{suggested}` fail for that template, blamed on a
    /// parameter the human never wrote, until the next Lull. So
    /// [`Surface::file_advice`] drops it here and audits why, and the wizard
    /// shows the template's own values. With no library there is nothing to
    /// instantiate and nothing to check: `instantiate_template` answers
    /// `NOT_FOUND` whatever the advice says.
    fn check_suggestion(&self, suggestion: &crate::advice::Suggestion) -> Result<(), Error> {
        let Some(folder) = self.host()?.library() else {
            return Ok(());
        };
        let template_id = suggestion.template_id.as_str();
        // The refusal names the template and never the path, as
        // `instantiate_template`'s does.
        let text = pharmakos_plan_core::library::read(folder, template_id).map_err(|_| {
            Error::not_found(format!(
                "no template `{template_id}` in this gateway's library"
            ))
        })?;
        let offered: Vec<pharmakos_plan_core::library::Parameter> = suggestion
            .parameters
            .iter()
            .map(|value| pharmakos_plan_core::library::Parameter {
                name: value.pointer.clone(),
                value: value.value.clone(),
            })
            .collect();
        pharmakos_plan_core::instantiate(&text, &[], &offered)
            .map(|_| ())
            .map_err(|error| Error::invalid(error.message))
    }

    /// An advised safe playbook, verified FULL and compiled, or the reason it
    /// is not filed.
    ///
    /// `NO_QUALIFYING_PLAN` for one the verifier refuses — the code spec
    /// section 12 names for "nothing that qualifies" — and the compile door's
    /// own refusal for one this build cannot execute
    /// ([`crate::surface::planning::compile_playbook`]).
    fn qualify_safe_playbook(
        &self,
        seat: SeatId,
        playbook_jsonc: &str,
        round: u32,
    ) -> Result<Sealed, Error> {
        if playbook_jsonc.chars().count() > crate::surface::planning::MAX_PLAYBOOK_CHARS {
            return Err(Error::invalid(format!(
                "an advised safe playbook is at most {} characters, like any other",
                crate::surface::planning::MAX_PLAYBOOK_CHARS
            )));
        }
        let report = self.verify_for(seat, playbook_jsonc, Depth::Full)?;
        if !report.qualifies {
            return Err(Error::new(
                crate::error::Code::NoQualifyingPlan,
                format!(
                    "seat {}'s advised safe playbook does not qualify ({} diagnostics); the \
                     gateway's fallback is filed instead",
                    seat.raw(),
                    report.diagnostics.len()
                ),
            ));
        }
        let plan =
            crate::surface::planning::compile_playbook(playbook_jsonc, self.host()?.rules())?;
        Ok(Sealed {
            playbook_jsonc: playbook_jsonc.to_owned(),
            report_hash: report.report_hash,
            round,
            // Filed on the seat's behalf, not written by it: draft continuity
            // does not offer it back as the seat's own work.
            filed_by_the_gateway: true,
            plan,
        })
    }

    /// What the operator advised a seat for the round the match is in, if it
    /// did.
    pub(crate) fn current_advice(&self, seat: SeatId) -> Option<&Advised> {
        let round = self.host.as_ref()?.runner().round();
        self.seats
            .iter()
            .find(|slot| slot.seat == seat)?
            .advice
            .as_ref()
            .filter(|advised| advised.round == round)
    }

    /// Pre-load last round's playbook as an editable draft and re-verify it.
    ///
    /// Nothing is carried for a seat that sealed nothing, and nothing is
    /// carried for a playbook the **gateway** filed: the safe playbook is not
    /// the player's work, and offering it back as "your draft" would be the
    /// gateway putting words in a seat's mouth. `get_safe_plan` is how a seat
    /// asks for that one.
    fn carry_draft_forward(&mut self, seat: SeatId, round: u32) -> Result<(), Error> {
        let carried = self
            .seat_state(Subject::Seat(seat), seat)?
            .sealed
            .as_ref()
            .filter(|sealed| !sealed.filed_by_the_gateway)
            .map(|sealed| sealed.playbook_jsonc.clone());
        let Some(playbook) = carried else {
            return Ok(());
        };
        let report = self.verify_for(seat, &playbook, Depth::Full)?;
        let continuity = Continuity {
            draft_id: String::from(CARRIED_DRAFT_ID),
            qualifies: report.qualifies,
            diagnostics: u32::try_from(report.diagnostics.len()).unwrap_or(u32::MAX),
            report_hash: report.report_hash.clone(),
        };
        let state = self.seat_state_mut(Subject::Seat(seat), seat)?;
        state
            .drafts
            .retain(|draft| draft.draft_id != CARRIED_DRAFT_ID);
        state.drafts.push(Draft {
            draft_id: String::from(CARRIED_DRAFT_ID),
            label: format!("carried from round {}", round.saturating_sub(1)),
            round,
            playbook_jsonc: playbook,
        });
        state.continuity = Some(continuity);
        Ok(())
    }

    /// Verify one seat's playbook against the frozen snapshot.
    ///
    /// The one path from a JSONC file to a report: `verify_plan`, `submit_plan`
    /// and draft continuity all come through here, which is what makes spec
    /// section 11's promise -- "a pre-check and the check at submit are
    /// byte-identical" -- a property of the code rather than of somebody's
    /// care. Nothing here steps or forks anything (AGENTS.md section 3 rule 2).
    ///
    /// # Errors
    ///
    /// As [`Surface::host`], plus [`crate::error::Code::Internal`] when the
    /// rules table does not carry a block the checks read. **Never** for a
    /// playbook that is wrong: that is a report with `qualifies: false`.
    pub(crate) fn verify_for(
        &self,
        seat: SeatId,
        playbook_jsonc: &str,
        depth: Depth,
    ) -> Result<VerifyReport, Error> {
        let snapshot = self.snapshot_bytes()?;
        let scope = self.verifier_scope(seat)?;
        pharmakos_plan_core::verify::verify_jsonc(
            playbook_jsonc,
            &snapshot,
            &scope,
            self.host()?.rules(),
            depth,
        )
        .map_err(|error| {
            Error::internal(format!(
                "this build cannot verify a playbook: {}",
                error.message
            ))
        })
    }

    /// The seat's view of the frozen snapshot, as the verifier reads it.
    ///
    /// **Its own beacons and nothing else.** A seat's knowledge of another
    /// seat's beacons is the knowledge store's, which is S3's, and a verifier
    /// scope that carried beacons a seat has not seen would let a playbook
    /// reference one it cannot know about -- the same fog leak
    /// [`crate::fog`] exists to prevent, arriving through the back door.
    ///
    /// PLACEHOLDER: a seat's **sighted** enemy beacons join this scope with the
    /// knowledge store at **S3**; `is_core` is decided here by "the seat's
    /// lowest-numbered beacon", which is true of every map the generator makes
    /// because it pre-places the core first, and becomes a column the day a
    /// beacon has a kind (owner, at T14 with the Build mandate).
    ///
    /// # Errors
    ///
    /// As [`Surface::host`].
    pub(crate) fn verifier_scope(&self, seat: SeatId) -> Result<VerifierScope, Error> {
        let world = self.host()?.world();
        let seats = world.seats();
        let row = seats
            .seats()
            .iter()
            .position(|held| *held == seat.raw())
            .ok_or_else(|| Error::not_found(format!("this match has no seat {}", seat.raw())))?;
        let economy = pharmakos_sim::knowledge::SeatEconomy {
            treasury: seats.treasuries().get(row).copied().unwrap_or_default(),
            supply: seats.supplies().get(row).copied().unwrap_or_default(),
            draw: seats.draws().get(row).copied().unwrap_or_default(),
        };
        let mut scope = VerifierScope::new(seat, economy);

        let beacons = world.beacons();
        let mut own: Vec<usize> = (0..beacons.ids().len())
            .filter(|index| beacons.seats().get(*index).copied() == Some(seat.raw()))
            .collect();
        own.sort_by_key(|index| beacons.ids().get(*index).copied().unwrap_or(u32::MAX));
        let core = crate::surface::knowledge::core_beacon_of(world, seat);
        for index in own {
            let id = beacons.ids().get(index).copied().unwrap_or_default();
            scope.push_beacon(pharmakos_verifier::KnownBeacon {
                beacon_id: crate::view::beacon_id(pharmakos_sim::tables::BeaconId::new(id)),
                owner: seat,
                side: pharmakos_verifier::Ownership::Own,
                mandate: crate::view::mandate_of(crate::view::mandate_from_id(
                    beacons.mandates().get(index).copied().unwrap_or_default(),
                )),
                // No tag column exists in the sim, and inventing one here would
                // be a proto change wearing a disguise (AGENTS.md section 5).
                tags: Vec::new(),
                at: beacons
                    .positions()
                    .get(index)
                    .copied()
                    .map(crate::view::voxel_of)
                    .unwrap_or_default(),
                is_core: core == Some(id),
            });
        }
        Ok(scope)
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

    /// The match's fog policy, to read. The handlers build a
    /// [`crate::fog::FogFilter`] from it and the host's vision; none of them
    /// may change it.
    #[must_use]
    pub(crate) const fn fog_policy(&self) -> &FogPolicy {
        &self.fog
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

    /// Store a draft for a seat without going through `save_draft`.
    ///
    /// T13 built `save_draft`, so this is no longer the only way in: what it is
    /// now is the **host's and a test's** way to seed a seat's private store
    /// directly, which is how the secrecy tests get something to fail to read.
    /// It takes the subject like every other path to the private store, so it
    /// is not a way around the secrecy gate; what it is around is the method's
    /// bounds on a label, an id and a count, which is why nothing a **client**
    /// sends reaches it.
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
        let action = call_action(&request.method);
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
            let limits = self
                .in_process_of(handle)
                .map_or(self.limits, |treatment| treatment.limits);
            self.limiters
                .push((handle, RateLimiter::with_limits(limits)));
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
        handle: Handle,
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

        let scratch = method == Method::GetView
            && self
                .in_process_of(handle)
                .is_some_and(|treatment| treatment.scratch_view);
        let mut result = if scratch {
            // See [`InProcess::scratch_view`]. The view feed is derived,
            // unhashed state, so a copy answers exactly what the real one
            // would, and putting the real one back leaves every viewer's state
            // and the feed's stamp as they were.
            let kept = self.views.clone();
            let answered = self.dispatch(method, subject, held, request, vision);
            self.views = kept;
            answered?
        } else {
            self.dispatch(method, subject, held, request, vision)?
        };
        if let Json::Object(entries) = &mut result {
            entries.push((String::from("_status"), self.time.footer()));
        }
        Ok(result)
    }

    /// Step 6: the method slice.
    ///
    /// One arm per method of the schema, so a method T1 adds fails to compile
    /// here rather than falling into the catch-all and answering `INTERNAL` to
    /// a client that had every right to call it.
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
            Method::WaitFor => self.wait_for(subject, held, request, vision),
            Method::GetSegmentFeed => self.segment_feed(subject, held, request, vision),
            Method::SaveNotes => self.save_notes(subject, request),
            Method::SetReady => self.set_ready(subject, request),

            // Knowledge.
            Method::GetBriefing => self.get_briefing(subject, request),
            Method::GetRecap => self.get_recap(request),
            Method::ListBeacons => self.list_beacons(subject, held, request, vision),
            Method::GetBeacon => self.get_beacon(subject, held, request, vision),
            Method::GetMapSummary => self.get_map_summary(request),
            Method::GetEconomyForecast => self.get_economy_forecast(subject, request),
            Method::EstimateRoute => self.estimate_route(subject, request),

            // Docs and planning.
            Method::GetSchema => Surface::get_schema(request),
            Method::ListTemplates => self.list_templates(request),
            Method::InstantiateTemplate => self.instantiate_template(subject, request),
            Method::VerifyPlan => self.verify_plan(subject, request),
            Method::RenderPlan => self.render_plan(subject, request),
            Method::PatchPlan => Surface::patch_plan(request),
            Method::SaveDraft => self.save_draft(subject, request),
            Method::ListDrafts => self.list_drafts(subject),
            Method::GetSafePlan => self.get_safe_plan(subject),
            Method::SubmitPlan => self.submit_plan(subject, request),

            // The view.
            Method::GetView => self.get_view(subject, held, request, vision),

            // Match control. Four `admin` methods, and the only handlers of
            // this crate that may drive the match -- `surface/control.rs` says
            // why, and `tests/confinement.rs` holds the rule.
            Method::EndLull => self.serve_end_lull(),
            Method::AdvancePush => self.serve_advance_push(request),
            Method::EndRecap => self.serve_end_recap(),
            Method::ReportHostClock => self.serve_report_host_clock(request),

            // The four `gateway.proto` gives no request/response pair at all,
            // plus `METHOD_UNSPECIFIED`, which `method_from_wire` already
            // refuses before a handler is reached.
            other => Err(Error::internal(format!(
                "`{}` is in the schema and this build does not serve it: `gateway.proto` gives it \
                 no request/response pair, because no skeleton client calls it and the stage that \
                 introduces what it reports on is the stage that will know what it takes",
                scopes::method_wire_name(other)
            ))),
        }
    }

    /// `wait_for`: has the trigger fired since the caller's mark?
    ///
    /// **It polls; it does not block, and it must not.** A long poll waits on a
    /// clock, and the gateway reads none (AGENTS.md section 4.5, decisions-log
    /// item 99's closing note). Blocking on a condition instead would be worse
    /// rather than better: the surface answers one call at a time and the host
    /// steps the match between calls, so a handler that waited for the phase to
    /// change would be waiting for something only its own caller's return can
    /// cause. So `timeout_ms` is validated and capped -- the shape v1.1
    /// publishes -- and the answer comes back at once, with `fired: false`
    /// where a blocking implementation would have waited. `gateway.proto`
    /// already says a timeout is a normal result and not an error. The cap is
    /// [`MAX_WAIT_MS`], and a timeout over it is refused rather than clamped:
    /// see that constant.
    ///
    /// The two triggers use two marks, and the difference is not cosmetic:
    ///
    /// * `phase_change` carries a **phase mark** ([`crate::time::PhaseMark`]),
    ///   which is tied to the match rather than to the segment. A
    ///   snapshot-tied cursor would go stale at the exact moment the phase
    ///   changed, which is the one moment this trigger exists for.
    /// * `feed_digest` carries the feed's own opaque cursor, and fires when
    ///   this viewer has an event it has not been shown.
    fn wait_for<V: Vision>(
        &self,
        subject: Subject,
        held: scopes::ScopeSet,
        request: &Request,
        vision: &V,
    ) -> Result<Json, Error> {
        let trigger = request
            .string_param("trigger")?
            .ok_or_else(|| Error::invalid("`trigger` is `phase_change` or `feed_digest`"))?;
        if let Some(timeout) = request.integer_param("timeout_ms")? {
            if timeout < 0 {
                return Err(Error::invalid(
                    "`timeout_ms` is game milliseconds and is never negative",
                ));
            }
            if timeout > i64::from(MAX_WAIT_MS) {
                return Err(Error::invalid(format!(
                    "`timeout_ms` is at most {MAX_WAIT_MS} game milliseconds and this \
                     is {timeout}"
                )));
            }
        }
        let cursor = request.string_param("cursor")?.unwrap_or_default();
        let (fired, next) = match trigger {
            "phase_change" => {
                let mark = crate::time::PhaseMark::of(self.match_seed, self.time);
                let fired = !cursor.is_empty()
                    && crate::time::PhaseMark::parse(cursor, self.match_seed)? != mark;
                (fired, mark.render())
            }
            "feed_digest" => {
                let viewer = self.viewer_of(subject, held);
                let filter = FogFilter::new(&self.fog, vision);
                let page = self.feed.page(
                    viewer,
                    &filter,
                    Some(cursor).filter(|text| !text.is_empty()),
                    crate::feed::MAX_PAGE_EVENTS,
                )?;
                (!page.events.is_empty(), page.next_cursor.render())
            }
            other => {
                return Err(Error::invalid(format!(
                    "`trigger` is `phase_change` or `feed_digest`, and this is `{other}`"
                )));
            }
        };
        Ok(Json::Object(vec![
            (String::from("fired"), Json::Bool(fired)),
            (String::from("status"), self.time.footer()),
            (String::from("cursor"), Json::String(next)),
        ]))
    }

    /// Who a subject is, to the fog filter.
    #[allow(
        clippy::unused_self,
        reason = "a method, not a free function: who a subject is to the fog filter is the \
                  surface's answer, and the day a match carries a spectator policy of its own \
                  this reads it"
    )]
    fn viewer_of(&self, subject: Subject, held: scopes::ScopeSet) -> Viewer {
        match subject {
            Subject::Seat(seat) => Viewer::Seat(seat),
            Subject::Spectator => Viewer::Spectator {
                nofog: held.holds(Scope::SpectateNofog),
            },
            Subject::Admin => Viewer::Admin,
        }
    }

    /// The seat a subject is, or a refusal.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::ForbiddenScope`] for a spectator or the lobby:
    /// neither has a briefing, a draft or a plan, and `admin` "can never read
    /// another seat's playbooks, drafts or knowledge" (spec section 12).
    pub(crate) fn seat_of(subject: Subject, what: &str) -> Result<SeatId, Error> {
        subject
            .seat()
            .ok_or_else(|| Error::forbidden(format!("only a seat has {what}")))
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
        let viewer = self.viewer_of(subject, held);
        let filter = FogFilter::new(&self.fog, vision);
        let cursor = request.string_param("cursor")?;
        // The detail budget is the ceiling and `limit` asks for no more than it:
        // a client may always ask for less than its budget, and never for more
        // (spec section 12, "Budgets"; [`crate::detail`] for the salience rule
        // that makes the events the part a budget cuts).
        let budget = crate::detail::events(crate::detail::of(request)?);
        let limit = request
            .integer_param("limit")?
            .and_then(|value| usize::try_from(value).ok())
            .map_or(budget, |asked| asked.min(budget));
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

/// What the audit log records for a call, resolved against the schema.
///
/// **The method string a client sent never reaches the log.** The log is
/// tab-separated and one record to a line, so a method name holding a tab or a
/// newline would be a forged record -- a seat writing a line that says another
/// seat submitted a plan -- and a very long one would be a way to fill the
/// private match cache from a single authenticated call. Neither is a formatting
/// problem; both are the log failing at the one thing it is for. So an unknown
/// method is logged as an unknown method, which is what the reader needs to
/// know, and the name it asked for is in the refusal the caller gets back.
pub(crate) fn call_action(method: &str) -> String {
    scopes::method_from_wire(method).map_or_else(
        || String::from("call <unknown>"),
        |resolved| format!("call {}", scopes::method_wire_name(resolved)),
    )
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
    use super::{Draft, Event, Surface};
    use crate::detail::Detail;
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

    /// Spec section 12's `detail` budget, applied to the one read method T9
    /// serves. The parameter's shape is what v1.1 publishes, so it is frozen
    /// here rather than added once clients exist ([`crate::detail`]).
    #[test]
    fn a_read_method_takes_a_detail_budget_and_the_digest_ignores_it() {
        let mut surface = surface();
        let token = seat_token(&mut surface, 0);
        for index in 0..40_i32 {
            surface
                .publish(Event {
                    at_ms: Ms::new(index.saturating_mul(100)),
                    kind: crate::feed::Kind::new("commander_moved").expect("a kind"),
                    text: String::from("."),
                    audience: crate::fog::Audience::Public,
                })
                .expect("published");
        }

        let events_of = |response: &Json| match result(response).get("events") {
            Some(Json::Array(events)) => events.len(),
            _ => panic!("a feed"),
        };
        let digest_of = |response: &Json| match result(response).get("digests") {
            Some(Json::Array(digests)) => digests.clone(),
            _ => panic!("digests"),
        };

        let brief = surface.call(
            Some(&token),
            &request("get_segment_feed", r#"{"detail":"brief"}"#),
            &Blind,
        );
        let full = surface.call(
            Some(&token),
            &request("get_segment_feed", r#"{"detail":"full"}"#),
            &Blind,
        );
        assert_eq!(events_of(&brief), crate::detail::events(Detail::Brief));
        assert_eq!(events_of(&full), 40, "everything there is");
        assert_eq!(
            digest_of(&brief),
            digest_of(&full),
            "the digest is the segment's: item 97's counts do not move with a budget"
        );

        // `limit` asks for less than the budget, never for more.
        let asked = surface.call(
            Some(&token),
            &request("get_segment_feed", r#"{"detail":"brief","limit":4}"#),
            &Blind,
        );
        assert_eq!(events_of(&asked), 4);
        let over = surface.call(
            Some(&token),
            &request("get_segment_feed", r#"{"detail":"brief","limit":1000}"#),
            &Blind,
        );
        assert_eq!(events_of(&over), crate::detail::events(Detail::Brief));

        // And a rung that does not exist is a refusal, not a silent default: a
        // budget the gateway ignored would be read as a complete answer.
        let wrong = surface.call(
            Some(&token),
            &request("get_segment_feed", r#"{"detail":"verbose"}"#),
            &Blind,
        );
        assert_eq!(code(&wrong), "INVALID_ARGUMENT");
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
                    playbook_jsonc: String::from("{}"),
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

    fn sim_event(
        kind: pharmakos_sim::events::EventKind,
        seat: Option<u8>,
    ) -> pharmakos_sim::events::Event {
        pharmakos_sim::events::Event {
            tick: Tick::new(7),
            seq: 0,
            kind,
            seat: seat.map(SeatId::new),
            subject: None,
            at: None,
            value: 3,
        }
    }

    #[test]
    fn a_seats_orders_are_private_to_it_whatever_the_fog_policy() {
        use crate::fog::{Audience, FogFilter, Viewer};
        use pharmakos_sim::events::EventKind;
        let mut private = 0;
        for kind in EventKind::ALL {
            let audience = Surface::audience_of(&sim_event(kind, Some(1))).expect("an audience");
            let Audience::Private(owner) = audience else {
                continue;
            };
            private += 1;
            assert_eq!(owner, SeatId::new(1), "{kind:?}");
            for policy in [FogPolicy::fogged(), FogPolicy::casual()] {
                let filter = FogFilter::new(&policy, &Blind);
                assert!(filter.visible(Viewer::Seat(SeatId::new(1)), &audience));
                for viewer in [
                    Viewer::Seat(SeatId::new(0)),
                    Viewer::Spectator { nofog: true },
                    Viewer::Admin,
                ] {
                    assert!(
                        !filter.visible(viewer, &audience),
                        "{kind:?} reached {viewer:?}"
                    );
                }
            }
        }
        // The seal, four step lines, two rule lines, two reflex lines, three
        // visit lines and the fallback: thirteen of T11's fourteen kinds.
        // `beacon_placed` is a thing in the world and fog decides it.
        //
        // Plus T14's nine economy lines: the two brownout lines, the two spend
        // lines, and the five that carry a treasury or a credit figure.
        // `structure_completed` is the tenth kind T14 adds and is the one it
        // puts on the world's side, for the reason `beacon_placed` is there.
        assert_eq!(private, 13 + 9);
        assert!(matches!(
            Surface::audience_of(&sim_event(EventKind::BeaconPlaced, Some(1))),
            Ok(Audience::World { .. })
        ));
    }

    #[test]
    fn a_private_kind_that_names_no_seat_is_refused_rather_than_shown() {
        use pharmakos_sim::events::EventKind;
        let error = Surface::audience_of(&sim_event(EventKind::StepStarted, None))
            .expect_err("nobody to show it to");
        assert_eq!(error.code, Code::Internal);
    }
}
