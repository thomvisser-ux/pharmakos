// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The knowledge reads: what a seat is allowed to know about the match it is
//! in.
//!
//! Spec section 12's Knowledge group, minus the four methods `gateway.proto`
//! gives no request/response pair. Every one of them reads the **frozen
//! planning snapshot or the live world through the fog filter** -- with one
//! exception the design takes on purpose: `get_map_summary`'s features are
//! unfogged in S1, as targeting's ranking is (decisions-log item 130 (3) (a)),
//! and each seat-relative field in them is the asking seat's own -- and not
//! one of them steps anything: `estimate_route` prices its legs through
//! [`crate::routes::RouteAdapter`], which borrows the hosted world's surface
//! and clusters and has no `World` in its hand, and it answers only in a Lull
//! (the owner's "Refuse in a Push", 2026-11-21), its `covering` and `toward`
//! waypoints included; `get_map_summary`'s features read the hosted world
//! through the sim's own resolver, borrowed read-only ([`crate::targeting`]),
//! and rank by the cost of the walk the router serves, priced without running
//! anything; their travel times (`travel_ms`, `reachable`) are told only in a
//! Lull, over the frozen planning world, and withheld in every other phase, a
//! Push, a recap and an ended match (the owner's "Omit them in a Push",
//! 2026-11-21, built by S3's `mapw`; decisions-log item 170), so no walk is
//! priced there; and
//! `get_economy_forecast` reads the calling seat's own
//! row of the hosted world -- the frozen planning world in a Lull or a recap,
//! the live one in a Push -- and, in a Lull, prices the seat's own seal by the
//! verifier's `$` projection, which is arithmetic over the file and runs
//! nothing (AGENTS.md section 3 rule 2; spec section 11's allowed estimates).
//! `get_briefing`'s standing and `get_recap`'s settlement read the sim's own
//! audit and the Ledger's own `settled` events, and restate no rule of either.
//!
//! # Salience, per method
//!
//! [`crate::detail`] fixes the two rules T9 froze -- the `_status` footer is
//! never cut, and summaries outrank details. The per-method order is each
//! method's own, and here it is, once, so a reviewer can read it without
//! opening five functions:
//!
//! | Method | Cut first | Never cut |
//! |---|---|---|
//! | `get_briefing` | nothing: it is already one page | the notebook, the standing, the segment length, the prose |
//! | `get_recap` | nothing | the prose |
//! | `list_beacons` | beacons past the budget, **own beacons last** | the cursor that says where the cut fell |
//! | `get_beacon` | nothing: one beacon is one beacon | the summary and the prose |
//! | `get_map_summary` | nothing | the extent, the seed, the prose, every feature |
//! | `get_economy_forecast` | what-if answers past the budget | the treasury and the power figures |
//! | `estimate_route` | nothing: a route with legs missing is a wrong number | every leg |
//!
//! `list_beacons` is the only one with a real order to state, and it is worth
//! its sentence: **a seat's own beacons come last in the cut and first in the
//! answer**, because a seat that asked for a brief listing and got somebody
//! else's beacons instead of its own would have been told the least useful half
//! of what it can see.

use core::num::{NonZeroU32, NonZeroUsize};
use pharmakos_proto::json::grammar::Said;
use pharmakos_proto::said;

use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::json::Json;
use pharmakos_sim::math::quantity::Money;
use pharmakos_sim::runner::MatchEndReason;
use pharmakos_sim::tables::SeatId;

use crate::detail::{self, Detail};
use crate::error::Error;
use crate::fog::{Audience, FogFilter, Viewer, Vision};
use crate::rpc::Request;
use crate::strings;
use crate::surface::{BandError, RecapFacts, Surface};
use crate::view;

/// How many beacons one page of `list_beacons` carries at each rung.
///
/// PLACEHOLDER: the beacon ladder 8 / 24 / 40 — owner, at hardening, with the rest of the read-method budgets.
/// 8 / 24 / 40 are working numbers. 40 is item 63's world total
/// for beacons, so `full` is "every beacon there could be" by construction, and
/// the two rungs below it step roughly by three as spec section 12's token
/// budgets do. OWNER sets the real ladder at hardening, with the rest of the
/// read-method budgets and `crate::detail::events`.
#[must_use]
pub const fn beacon_budget(detail: Detail) -> usize {
    match detail {
        Detail::Brief => 8,
        Detail::Unspecified | Detail::Standard => 24,
        Detail::Full => 40,
    }
}

/// The most waypoints one `estimate_route` may name.
///
/// A route costs one estimate per leg (the walk the router would serve, its
/// partial routes chained, near pair or far) and there is no estimate cache
/// (decisions-log item 61), so the count is the caller's multiplier on the
/// gateway's whole single-threaded turn: measured on the shipped 384x384x64
/// map, 2 000 waypoints between two far corners is a 64 KiB request -- well
/// inside [`crate::frame::MAX_MESSAGE_BYTES`] -- that takes twelve seconds,
/// during which nothing else is answered and the match is not stepped. So the
/// count is refused before any search runs, the same shape
/// `get_economy_forecast` already refuses a surplus of what-ifs in.
///
/// PLACEHOLDER: 64 waypoints a route — owner, at hardening, with the rate limits.
/// 64 is a working number -- a route with more legs than a
/// playbook has steps at item 94's 128-unit budget is not a route an editor
/// draws. OWNER settles it at hardening with the rate limits, beside
/// [`crate::surface::MAX_WAIT_MS`] and the draft bounds.
pub const MAX_WAYPOINTS: usize = 64;

/// How many what-if answers one `get_economy_forecast` carries at each rung.
///
/// PLACEHOLDER: the what-if ladder, as [`beacon_budget`] — owner, at hardening, settled with it.
#[must_use]
pub const fn what_if_budget(detail: Detail) -> usize {
    match detail {
        Detail::Brief => 2,
        Detail::Unspecified | Detail::Standard => 8,
        Detail::Full => 24,
    }
}

impl Surface {
    /// `get_briefing`: the main read.
    ///
    /// The notebook is at the **top**, as spec section 12 asks, and it is the
    /// seat's own: [`Surface::seat_state`] is the only path to it and it takes
    /// the subject asking, so `admin` and another seat both get nothing.
    ///
    /// In a Lull the prose ends with the "this round" sentence
    /// ([`Surface::this_round`]): what the seat's sealed or carried playbook
    /// does this round wherever its route reads the map, read off the frozen
    /// world as the sim will read it. Like the notebook, it is the seat's own,
    /// and it goes only to a caller that holds `plan.submit` -- the seat's own
    /// client. The built-in advisor reads a human seat's briefing with a token
    /// that never holds that scope (`crate::serve`'s advisor scopes), and a
    /// sentence about the seat's playbook is exactly what decisions-log item
    /// 111 keeps from it: what it advises must not depend on what the seat
    /// wrote.
    ///
    /// From S3, `licences_held`: the licences the sim's licence table holds for
    /// the seat, ascending (decisions-log item 153 (3); S3's plan, E2), left
    /// out while it holds none. The seat's own, like the rest of the briefing:
    /// no seat reads another seat's set here.
    pub(super) fn get_briefing(
        &self,
        subject: crate::token::Subject,
        held: crate::scopes::ScopeSet,
        request: &Request,
    ) -> Result<Json, Error> {
        let seat = Surface::seat_of(subject, "a briefing")?;
        // Validated, then not spent: a briefing is already one page, so no rung
        // cuts it. The parameter is refused when it names a rung that does not
        // exist, because a budget the gateway ignored would be read as a
        // complete answer (`crate::detail`).
        let _ = detail::of(request)?;
        let notes = self.seat_state(subject, seat)?.notebook.clone();

        let host = self.host()?;
        let world = host.world();
        let beacons = rows_of(world.beacons().seats(), seat);
        let units = rows_of(world.units().seats(), seat);
        let living = u32::try_from(
            (0..world.seats().seats().len())
                .filter(|row| world.seats().is_alive(*row))
                .count(),
        )
        .map_err(|_| Error::internal("more living seats than a seat id can number"))?;
        let place = displayed_place(world, seat)?;
        let segment_ms = host.runner().frozen().coming_segment_ms();
        let briefing = strings::briefing(
            host.runner().phase(),
            host.runner().round(),
            world.match_state().round_limit(),
            segment_ms,
            beacons,
            units,
        );
        let standing = strings::standing(
            living,
            place.rank.map(|rank| (rank.get(), place.of)),
            place.score,
        );
        // The Lull's "this round" sentence, after the standing: what this
        // round's playbook does wherever its route reads the map. Never to an
        // advisor, which holds no `plan.submit`.
        let this_round = if held.holds(crate::scopes::Scope::PlanSubmit) {
            self.this_round(seat)?
        } else {
            None
        };

        // The licences this seat holds, ascending (decisions-log item 153 (3);
        // S3's plan, E2): its own, and left out while it holds none.
        let held: Vec<Json> = licence_rows(world)?
            .into_iter()
            .filter(|row| row.seat == seat)
            .map(|row| Json::String(licence_wire(row.licence)))
            .collect();

        let mut entries = vec![
            (String::from("notes"), Json::String(notes)),
            (
                String::from("standing"),
                // Spec section 3: a seat's OWN score and displayed rank only,
                // both on the full audit score ([`displayed_place`]). A seat
                // out of the match holds no place, so its `rank` is left out
                // rather than written as a 0 a reader would take for one, as
                // `beacon_summary` leaves an unknown field out; its score is
                // still its own held value.
                Json::Object(standing_entries(place, living)),
            ),
            (
                String::from("segment_length_ms"),
                Json::Number(segment_ms.raw().to_string()),
            ),
            (
                String::from("prose"),
                Json::String(match this_round {
                    Some(this_round) => format!("{briefing} {standing} {this_round}"),
                    None => format!("{briefing} {standing}"),
                }),
            ),
        ];
        if !held.is_empty() {
            entries.push((String::from("licences_held"), Json::Array(held)));
        }
        Ok(Json::Object(entries))
    }

    /// `get_recap`: what the last round did.
    ///
    /// The prose is match-wide and is told to every caller: which round last
    /// ended, how many ticks its segment ran, and how the match ended when it
    /// has. Both figures come from the segment's own end as the sim reported
    /// it ([`crate::surface::RecapFacts`]): read off the match state once the
    /// segment had closed, they said "Round N ran 0 ticks" for a round that
    /// ran in full (decisions-log item 130 (4), found by the `check` lane).
    ///
    /// A **seat** is told its own settlement and shortfall as well (spec
    /// section 7, "Settlement"; the register's X-16), as `settlement` and
    /// `shortfall` and as two sentences after the match-wide ones. Both are
    /// the seat's own, like every number on this surface: another seat's
    /// income is on no wire, so a caller that is no seat -- `admin`, a
    /// spectator -- is told the prose alone.
    ///
    /// * **The settlement**: `bmi_dollars` is what the Ledger credited the
    ///   seat, the `settled` event's own value (the BMI after its band's
    ///   adjustment), and `award_dollars` is 0, as `gateway.proto` says it is
    ///   until S4 brings the awards the fund is split by. `band_rank` (from 1,
    ///   the leader first) and `band_percent` are the sim's own reads,
    ///   `economy::ladder_place` and `economy::band_percent`, over the ladder
    ///   as the Ledger read it ([`crate::surface::RecapFacts::band`];
    ///   decisions-log item 134 (2) (c)). When the sim's event bus dropped
    ///   events at the segment, a `settled` among them, the ladder cannot be
    ///   rebuilt: `band_rank` and `band_percent` are left out (a `band_rank`
    ///   of 0, which counts from 1, says "not told") and the prose says why,
    ///   while the rest of the recap is still told.
    /// * **A feed loss**: when the bus dropped any event in the segment, the
    ///   prose says how many, since the failures below may then be short. A
    ///   segment end whose own `segment_ended` was dropped has no record, and
    ///   the recap refuses as `INTERNAL`, naming the loss, rather than tell
    ///   the round before it.
    /// * **The shortfall**, only when one of the seat's own beacons was dark
    ///   as the segment ended: `beacon_ids` names them, `b_NN`, ascending, and
    ///   `kw` is the grid's own shed read at that segment end
    ///   (`power::dark_load`'s `shed`; the same item).
    /// * **Why a step found nothing**: one sentence per distinct `step_failed`
    ///   the seat's playbook met in the segment, with how often, in the order
    ///   each first happened ([`strings::recap_step_failed`]): "3 matched,
    ///   none reachable" for a description that matched and reached nothing
    ///   (`docs/design/targeting.md`, "Surfaces").
    /// * **The tally** (S2's plan, E15): the seat's own kills, shared kills,
    ///   losses and enemy value destroyed for the match so far, as the sim's
    ///   hashed per-seat counters stood when the segment ended
    ///   ([`RecapFacts::tallies`]), never added up from the feed, whose bus
    ///   drops past its capacity. Like the settlement it is this recap's, so
    ///   it is the same in the recap, the Lull and the Push that follow.
    /// * **The licence line**, `licences_awarded` (spec section 8: "announced
    ///   in the recap"; decisions-log item 153 (3)), under the tally's fog
    ///   rule: between rounds, what the seat was awarded at this settlement
    ///   and no other seat's; once the match has ended, every seat's award of
    ///   the whole match that still stands. The seat's own awards at this
    ///   settlement are said in the prose too ([`licence_line`]).
    /// * **The Ledger's memos**, `ledger_memos`: those issued to the calling
    ///   seat over the match so far, in the order they were issued, in a
    ///   Probation match (S3's plan, E5; [`crate::probation`]).
    /// * **Every seat's tally**, `seat_tallies`, ascending by seat, **only once
    ///   the match is decided** -- from the final recap on, where the outcome
    ///   is decided and the fog lifts -- (the owner's ruling, decisions-log item
    ///   145 (7)): between rounds a seat learns its own counts and nobody
    ///   else's. Told to a seat only, like everything in this block: the lobby
    ///   and a spectator read no seat's knowledge (AGENTS.md section 7).
    pub(super) fn get_recap(
        &self,
        subject: crate::token::Subject,
        request: &Request,
    ) -> Result<Json, Error> {
        let _ = detail::of(request)?;
        let host = self.host()?;
        if let Some(lost) = self.recap_loss() {
            return Err(Error::internal(said!(
                "the sim's event bus dropped {} events at the last segment end, its \
                 `segment_ended` among them, so there is no record of that round to recap",
                lost.dropped
            )));
        }
        let facts = self.recap_facts();
        // Every winner: the last seat standing, or whoever the final audit
        // names, read from the world by the sim's one audit function rather
        // than stored, which is what lets a shared win name all its seats
        // while the outcome keeps its one winner byte (register X-08).
        let outcome = host.runner().outcome().map(|outcome| {
            let winners: Vec<u8> = match (outcome.reason, outcome.winner) {
                (MatchEndReason::LastSeatStanding, winner) => {
                    winner.map(SeatId::raw).into_iter().collect()
                }
                _ => pharmakos_sim::audit::final_audit(host.world())
                    .winners
                    .into_iter()
                    .map(SeatId::raw)
                    .collect(),
            };
            (outcome.reason, winners)
        });
        let outcome = outcome
            .as_ref()
            .map(|(reason, winners)| (*reason, winners.as_slice()));
        let mut prose = match facts {
            Some(facts) => strings::recap(facts.round, facts.ticks, outcome),
            None => strings::no_recap(
                host.runner().round() <= 1
                    && matches!(
                        host.runner().phase(),
                        pharmakos_sim::runner::MatchPhase::Lull
                            | pharmakos_sim::runner::MatchPhase::Push
                    ),
                outcome,
            ),
        };
        let mut entries = vec![];
        if let (Some(seat), Some(facts)) = (subject.seat(), facts) {
            if facts.dropped > 0 {
                prose.push(' ');
                prose.push_str(&strings::feed_lost(facts.dropped));
            }
            if let Some(settlement) = settlement_of(facts, host.world().rules(), seat, &mut prose)?
            {
                entries.push((String::from("settlement"), settlement));
            }
            let ended = outcome.is_some();
            entries.push((
                String::from("tally"),
                own_tally(facts, seat, ended, &mut prose)?,
            ));
            if let Some(dark) = facts.dark.iter().find(|dark| dark.seat == seat) {
                let names: Vec<String> = dark
                    .ordinals
                    .iter()
                    .copied()
                    .map(pharmakos_sim::tables::own_beacon_name)
                    .collect();
                let kw = dark.shed.raw();
                prose.push(' ');
                prose.push_str(&strings::shortfall(&names, kw));
                entries.push((
                    String::from("shortfall"),
                    Json::Object(vec![
                        (String::from("kw"), Json::Number(kw.to_string())),
                        (
                            String::from("beacon_ids"),
                            Json::Array(names.into_iter().map(Json::String).collect()),
                        ),
                    ]),
                ));
            }
            failure_lines(facts, seat, &mut prose)?;
            if let Some(all) = seat_tallies(host, facts)? {
                entries.push((String::from("seat_tallies"), all));
            }
            if let Some(awarded) = licence_line(host.world(), facts, seat, ended, &mut prose)? {
                entries.push((String::from("licences_awarded"), awarded));
            }
        }
        // The Ledger's memos issued to the calling seat over the match so far,
        // in the order they were issued (S3's plan, E5): a seat's own, like
        // everything in this block, and left out when there are none.
        if let Some(seat) = subject.seat() {
            let memos = &self.seat_state(subject, seat)?.memos;
            if !memos.is_empty() {
                entries.push((
                    String::from("ledger_memos"),
                    Json::Array(
                        memos
                            .iter()
                            .map(|memo| Ok(memo.json(&memo.text(&self.rules)?)))
                            .collect::<Result<Vec<Json>, Error>>()?,
                    ),
                ));
            }
        }
        entries.insert(0, (String::from("prose"), Json::String(prose)));
        Ok(Json::Object(entries))
    }

    /// `list_beacons`: the beacons this seat may see or remembers, its own
    /// first.
    ///
    /// `&mut self` because naming another seat's beacon mints this viewer's
    /// `e_NN` for it the first time it is shown
    /// ([`crate::viewfeed::ViewFeed::beacon_name`]); nothing else is written.
    ///
    /// # Errors
    ///
    /// As [`Surface::known_beacons`], and
    /// [`crate::error::Code::InvalidArgument`] for a cursor this gateway did
    /// not issue.
    pub(super) fn list_beacons<V: Vision>(
        &mut self,
        subject: crate::token::Subject,
        held: crate::scopes::ScopeSet,
        request: &Request,
        vision: &V,
    ) -> Result<Json, Error> {
        let viewer = self.viewer_of(subject, held);
        let budget = beacon_budget(detail::of(request)?);
        let limit = limit_param(request, budget)?;
        let visible = self.known_beacons(viewer, vision)?;

        // The cursor is an index into THIS viewer's own visible listing, for
        // the same reason `crate::feed`'s is: an index over the unfiltered
        // table would be a number a fogged seat could subtract from its page
        // length to learn how many beacons it was not told about.
        let snapshot = self.feed().snapshot();
        let from = match request.string_param("cursor")? {
            Some(text) if !text.is_empty() => {
                let index =
                    crate::feed::Cursor::parse(text, snapshot, crate::feed::Listing::Beacons)?
                        .index();
                usize::try_from(index).map_err(|_| {
                    Error::invalid(said!(
                        "this cursor points at beacon {}, past any listing",
                        index
                    ))
                })?
            }
            _ => 0,
        };
        // `limit` is taken as asked, zero included: the doc above says a client
        // may always ask for less than its budget, and zero is less. A page of
        // none with the cursor where it was is the honest answer to it, and a
        // silent `.max(1)` would be the gateway deciding what the client meant.
        let page = visible
            .iter()
            .skip(from)
            .take(limit)
            .map(|known| self.beacon_summary(known))
            .collect::<Result<Vec<Json>, Error>>()?;
        let index = from.min(visible.len()).saturating_add(page.len());
        let next = if index >= visible.len() {
            String::new()
        } else {
            let at = u32::try_from(index).map_err(|_| {
                Error::internal(said!("a beacon listing of {} does not fit a cursor", index))
            })?;
            crate::feed::Cursor::at(snapshot, crate::feed::Listing::Beacons, at).render()
        };
        Ok(Json::Object(vec![
            (String::from("beacons"), Json::Array(page)),
            (String::from("next_cursor"), Json::String(next)),
        ]))
    }

    /// `get_beacon`: one beacon, if this seat may see it or remembers it.
    ///
    /// A `b_NN` names one of the caller's own beacons by its per-seat
    /// ordinal, and an `e_NN` the beacon this caller was shown under that
    /// handle (decisions-log item 127 (13); `docs/design/targeting.md`,
    /// "Names"). Nothing is minted here: a handle the caller was never shown
    /// names nothing, so a guess answers exactly as an absent beacon does.
    ///
    /// **The prose says what the summary says and nothing more** (S2's plan,
    /// E9, with decision 5): another seat's beacon in sight is its owner and
    /// its hit points, one remembered is its owner, its place and how long ago
    /// it was seen, and neither is ever its mandate; a seat's own beacon is
    /// its mandate and its hit points, which are the seat's own business.
    ///
    /// **A seat's own beacon carries its detail** from S3 (the register's X-17,
    /// for S3-01's Visit & change rows; [`beacon_detail`]): its writ, each
    /// setting its writ holds with what may be written there, and a Build
    /// beacon's targets in the order the Quartermaster reads them. Another
    /// seat's beacon carries none of it, under every fog policy (decision 5).
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::NotFound`] for a name that names no beacon this
    /// caller may see or remembers, and as [`Surface::known_beacon`].
    pub(super) fn get_beacon<V: Vision>(
        &mut self,
        subject: crate::token::Subject,
        held: crate::scopes::ScopeSet,
        request: &Request,
        vision: &V,
    ) -> Result<Json, Error> {
        let _ = detail::of(request)?;
        let wanted = request
            .string_param("beacon_id")?
            .ok_or_else(|| Error::invalid("`beacon_id` names a beacon, as in `b_04`"))?;
        let Some(name) = pharmakos_sim::tables::parse_beacon_name(wanted) else {
            return Err(Error::not_found(said!(
                "`{}` is not a beacon id",
                pharmakos_proto::json::grammar::quoted(wanted)
            )));
        };
        let viewer = self.viewer_of(subject, held);
        // NOT_FOUND, deliberately, and the same refusal for every way a name
        // can name nothing: "a named beacon does not exist, or is not yours"
        // is one answer in the closed set, and two answers would let a seat
        // map the enemy's beacons by asking for every id.
        let absent = || {
            Error::not_found(said!(
                "no beacon `{}` here",
                pharmakos_proto::json::grammar::quoted(wanted)
            ))
        };
        let id = match name {
            pharmakos_sim::tables::BeaconName::Own(_) => {
                let world = self.host()?.world();
                let row = self
                    .views
                    .beacon_row(viewer, world, wanted)
                    .ok_or_else(absent)?;
                world.beacons().ids().get(row).copied().ok_or_else(|| {
                    Error::internal(said!("the beacon table has no id in row {}", row))
                })?
            }
            // By the handle's canonical spelling, so `e_7` and `e_07` name
            // the one beacon, as `parse_beacon_name` reads them.
            pharmakos_sim::tables::BeaconName::Foreign(number) => self
                .views
                .foreign_beacon_id(viewer, &pharmakos_sim::tables::foreign_beacon_name(number))
                .ok_or_else(absent)?,
        };
        let known = self.known_beacon(viewer, vision, id)?.ok_or_else(absent)?;
        let summary = self.beacon_summary(&known)?;
        let mut detail = Vec::new();
        let prose = match known.seen {
            Seen::Own { row } => {
                let world = self.host()?.world();
                let mandate = view::mandate_from_id(
                    world
                        .beacons()
                        .mandates()
                        .get(row)
                        .copied()
                        .ok_or_else(|| {
                            Error::internal(said!("the beacon table has no mandate in row {}", row))
                        })?,
                )?;
                detail = beacon_detail(world, row, mandate)?;
                let hp = self.beacon_hit_points(row, id)?;
                strings::own_beacon(&known.name, view::mandate_name(mandate), hp.0)
            }
            Seen::Live { row } => {
                let hp = self.beacon_hit_points(row, id)?;
                strings::foreign_beacon_in_sight(&known.name, known.owner, hp)
            }
            Seen::Remembered { age_ms } => {
                strings::foreign_beacon_remembered(&known.name, known.owner, known.at, age_ms)
            }
        };
        let mut entries = vec![
            (String::from("summary"), summary),
            (String::from("prose"), Json::String(prose)),
        ];
        entries.extend(detail);
        Ok(Json::Object(entries))
    }

    /// `get_map_summary`: the extent, the seed, and every vent and seam.
    ///
    /// The extent and the seed are "part of the match, not a secret"
    /// (`gateway.proto`), so neither is fog-filtered. **The seed is not told to
    /// a seat** from S3 (decisions-log item 127 (13); S3's plan, E18): a seat's
    /// token is answered with no `match_seed` and prose that names none, the
    /// built-in operator's in-process seat tokens included (AGENTS.md section 3
    /// rule 3: the operator is not privileged), while the lobby's and a
    /// spectator's keep it. A courtesy, not a secret kept: the generated map is
    /// still served whole (item 107 (1)), and a local agent can read the seed
    /// from the private match cache's `match.json`. Neither is the feature
    /// list yet: decisions-log item 130 (3) (a) kept `live` on it, and `live`
    /// and `covered` read the world as it stands (the frozen world in a Lull
    /// or a recap, the live one in a Push), unfogged, as targeting's own
    /// ranking is; `travel_ms` and `reachable` are told only in a Lull, over
    /// the frozen planning world, and withheld in every other phase (S3's
    /// `mapw`). The list is fog-filtered from S4, with real terrain fog
    /// (`gateway.proto`; S3's plan, decision 2, register S3-05), and the
    /// terrain summary stays `reserved` until then (item 130 (3) (b)).
    ///
    /// Each feature's id, kind, grade and anchor are a pure function of the
    /// match's inputs; `covered`, `travel_ms` and `reachable` are the asking
    /// **seat's** own (its own spheres, its own commander), so a caller that
    /// is no seat -- `admin`, a spectator -- is told the first six fields and
    /// none of the three, which would be about nobody
    /// ([`Surface::map_features`]). A seat is told `travel_ms` and
    /// `reachable` only in a Lull, and neither in any other phase (S3's
    /// `mapw`).
    pub(super) fn get_map_summary(
        &self,
        subject: crate::token::Subject,
        request: &Request,
    ) -> Result<Json, Error> {
        let _ = detail::of(request)?;
        let host = self.host()?;
        let size = host.world().voxels().size();
        // `None` for a seat's token (E18); the field is then left out, at
        // proto3's default, rather than written empty.
        let seed = subject
            .seat()
            .is_none()
            .then(|| view::seed_text(host.world().match_seed()));
        let seats = host.world().seats().len();
        let [x, y, z] = size;
        let axis = |length: u32| Json::Number(length.to_string());
        let features = self.map_features(subject.seat())?;
        let mut entries = vec![
            (
                String::from("size"),
                Json::Object(vec![
                    (String::from("x"), axis(x)),
                    (String::from("y"), axis(y)),
                    (String::from("z"), axis(z)),
                ]),
            ),
            (
                String::from("prose"),
                Json::String(strings::map_summary(size, seed.as_deref(), seats)),
            ),
        ];
        if let Some(seed) = seed {
            entries.push((String::from("match_seed"), Json::String(seed)));
        }
        entries.push((String::from("features"), Json::Array(features)));
        Ok(Json::Object(entries))
    }

    /// `get_map_summary.features`: every vent and seam, in feature id order,
    /// each a `gp.api.v1.MapFeature`.
    ///
    /// For a seat, `covered` is whether one of its own living beacons' spheres
    /// holds the feature (the opposite of a pick's `UNCOVERED`). **In a Lull
    /// only**, `travel_ms` and `reachable` are the walk the router serves its
    /// commander, over the frozen planning world, from where it stands to the
    /// column "nearest" measures to, priced by the sim's own
    /// [`pharmakos_sim::targeting::Ranker`]
    /// ([`pharmakos_sim::pathing::estimate::walk_cost`]; [`crate::targeting`]):
    /// the measure `resolve_refs` binds by and `estimate_route` answers in,
    /// both of which are Lull-only too (S3's `p3fix`). A commander that is not
    /// standing anywhere (dead) reaches nothing, and the answer says so. In
    /// every other phase both members are withheld and nothing is priced (see
    /// below). Nothing is stepped.
    ///
    /// **Travel is told only in a Lull** (the owner's ruling of 2026-11-21,
    /// "Omit them in a Push", taken by S3's `mapw` to every phase that is not
    /// a Lull, as [`Surface::estimate_route`]'s door is): in a Push the hosted
    /// world is the live one, unfogged, and a recap's is the Push's end
    /// state, so a time there would tell the seat about terrain it cannot
    /// see. Outside a Lull both members are left out of every feature
    /// (`gp.api.v1.MapFeature`'s third state, "withheld") and no walk is
    /// priced at all.
    fn map_features(&self, seat: Option<SeatId>) -> Result<Vec<Json>, Error> {
        self.map_features_priced(seat, &mut None)
    }

    /// [`Surface::map_features`], pricing its walks in `scratch`: the search
    /// scratch is built there on the first walk when the slot is empty, so a
    /// caller that keeps the slot reads the sim's own walk tally afterwards
    /// (`surface::tests::outside_a_lull_the_map_summary_prices_no_walk`).
    pub(super) fn map_features_priced(
        &self,
        seat: Option<SeatId>,
        scratch: &mut Option<pharmakos_sim::pathing::Scratch>,
    ) -> Result<Vec<Json>, Error> {
        let host = self.host()?;
        let world = host.world();
        let reads = crate::targeting::feature_reads(world, seat);
        // `None` for a caller that is no seat, which is told none of the
        // seat's own members.
        let travel: Option<Travel> = match seat {
            None => None,
            Some(_) if !self.time.planning_open() => Some(Travel::Withheld),
            Some(seat) => Some(Travel::Priced(
                match crate::targeting::commander_point(world, seat) {
                    Some(commander) => {
                        let tally = pharmakos_sim::seams::UnitTally::new();
                        let ground = crate::targeting::lend(world, &tally);
                        let scratch = match scratch {
                            Some(scratch) => scratch,
                            None => scratch.insert(crate::targeting::scratch_for(world)?),
                        };
                        crate::targeting::ranked(
                            &ground,
                            scratch,
                            pharmakos_sim::targeting::column_of(commander),
                            |_| true,
                        )
                    }
                    None => Vec::new(),
                },
            )),
        };
        reads
            .iter()
            .map(|read| {
                let mut entries = vec![
                    (String::from("feature_id"), Json::String(read.name.clone())),
                    (
                        String::from("kind"),
                        Json::String(feature_kind_wire(read.kind)),
                    ),
                    (String::from("grade"), Json::String(grade_wire(read.grade))),
                    (String::from("x"), Json::Number(read.x.to_string())),
                    (String::from("y"), Json::Number(read.y.to_string())),
                    (String::from("live"), Json::Bool(read.live)),
                ];
                let Some(travel) = &travel else {
                    return Ok(Json::Object(entries));
                };
                entries.push((
                    String::from("covered"),
                    Json::Bool(read.covered_by.is_some()),
                ));
                if let Travel::Priced(walks) = travel {
                    let reached = match walks.iter().find(|(index, _)| *index == read.index) {
                        // A commander with no speed walks nowhere, so the
                        // feature is not reached.
                        Some((_, walk)) => crate::targeting::travel_ms(world, *walk)?,
                        None => None,
                    };
                    // `travel_ms` only beside `reachable: true`, as
                    // `resolve_refs` writes it (decisions-log item 133 (3)
                    // (i)): a 0 beside an unreachable feature would read as
                    // a feature at the commander's feet.
                    if let Some(reached) = reached {
                        entries.push((
                            String::from("travel_ms"),
                            Json::Number(reached.raw().to_string()),
                        ));
                    }
                    entries.push((String::from("reachable"), Json::Bool(reached.is_some())));
                }
                Ok(Json::Object(entries))
            })
            .collect()
    }

    /// `get_economy_forecast`: the seat's own `$` and kW as they stand, what
    /// its sealed orders commit it to spend, and what-ifs.
    ///
    /// # The four present-state figures
    ///
    /// Decisions-log item 111, decision C7, a departure from item 105 (2)
    /// taken in the open: `treasury_now`, `supply_kw_now`, `draw_kw_now` and
    /// `headroom_kw_now` are what the sim already reads out
    /// (`SeatTable::treasuries`, `supplies` and `draws`), **not** a projection.
    /// They are the seat's **own**, as the world stands: the frozen planning
    /// world in a Lull or a recap, the live one in a Push — the world is not
    /// stepped in a Lull or a recap, so the hosted world *is* the frozen one
    /// then. Later answers land in new fields and never redefine these
    /// (`gateway.proto`). A subject that is not a seat has no economy and is
    /// refused, as it is refused a briefing; another seat's economy is on no
    /// wire at all.
    ///
    /// The live answer in a Push stays, as the owner ruled for S1
    /// (decisions-log item 128, S1's plan, decision 12; the register's
    /// S1-12): a seat's own economy is its own knowledge, which spec section 3
    /// allows it live.
    ///
    /// Whole `$` and whole kW, `int32` on the wire like the `Treasury` and
    /// `KwHeadroom` predicates a playbook compares them against. A figure past
    /// `int32` -- a treasury of two billion `$`, which no match reaches -- is
    /// refused as [`crate::error::Code::Internal`] rather than saturated: a
    /// saturated figure is a wrong answer told as a right one.
    ///
    /// # What decision 12 adds in S1 (the register's S1-46)
    ///
    /// * **`committed_dollars`**, in a Lull: the `$` this round's seal orders,
    ///   by the verifier's own projection
    ///   ([`pharmakos_verifier::projection::project`], spec section 11's "`$`
    ///   and kW projection (Quartermaster arithmetic)") -- the figure `W0602`
    ///   compares with the treasury, summed over everything the route may
    ///   order: the **upper bound** of the seal's unpaid orders, not an exact
    ///   figure, because a step that may order something is counted in full.
    ///   In a Lull none of it has been paid, so "not yet paid" holds of all
    ///   of it; 0 when the seat has sealed nothing this round, which commits
    ///   nothing. A seal always prices, because `submit_plan` refuses one
    ///   that does not ([`Surface::seal_commitment_of`]). **Outside a Lull it
    ///   is left out:** a Push pays its seal as it goes, and how much of it is
    ///   still unpaid is the interpreter's to know, which this method does not
    ///   read.
    /// * **`bmi_next_dollars`**, in every phase: the BMI at the band the seat
    ///   holds now (`gateway.proto`'s words) -- the sim's `economy::bmi_for`
    ///   at the sim's `economy::ladder_place` over every living seat's held
    ///   value as the world stands ([`bmi_next`]). It is a prediction, not
    ///   what the next settlement will pay: held value counts a structure
    ///   still going up at what it is worth so far, so spending in a Push can
    ///   move the seat's rank before the Ledger reads the ladder. Left
    ///   out for a seat that is out of the match, which is off the ladder and
    ///   is paid no BMI, and once the match has ended, when no settlement is
    ///   next.
    ///
    /// # What-ifs
    ///
    /// `gp.api.v1.WhatIf` still reserves 1 to 15, so a what-if that names
    /// anything is refused, and the count is bounded by the detail budget.
    ///
    /// What is left of S1-46 is the what-if vocabulary and its answers, the
    /// projected income and the projection (S1's plan, decision 12;
    /// decisions-log item 130 (3) (b); the register's S1-46).
    ///
    /// PLACEHOLDER: the what-if vocabulary and answers, projected income and projection (S1-46) — owner, S6, with the segment clock (S3's plan, register row S1-46)
    pub(super) fn get_economy_forecast(
        &self,
        subject: crate::token::Subject,
        request: &Request,
    ) -> Result<Json, Error> {
        let budget = what_if_budget(detail::of(request)?);
        let seat = Surface::seat_of(subject, "an economy")?;
        let host = self.host()?;
        if let Some(Json::Array(what_ifs)) = request.param("what_ifs") {
            if what_ifs.len() > budget {
                return Err(Error::invalid(said!(
                    "this detail budget answers {} what-ifs and {} were asked",
                    budget,
                    what_ifs.len()
                )));
            }
            for (index, what_if) in what_ifs.iter().enumerate() {
                match what_if {
                    Json::Object(entries) if entries.is_empty() => {}
                    _ => {
                        return Err(Error::invalid(said!(
                            "what-if {} names something: the what-if vocabulary is \
                             `gp.api.v1.WhatIf`, which has no fields in this build",
                            index
                        )));
                    }
                }
            }
        } else if request.param("what_ifs").is_some() {
            return Err(Error::invalid("`what_ifs` is an array"));
        }

        let economy = self.seat_economy(seat)?;
        let pharmakos_sim::knowledge::SeatEconomy {
            treasury,
            supply,
            draw,
        } = economy;
        let headroom = i64::from(supply.raw())
            .checked_sub(i64::from(draw.raw()))
            .ok_or_else(|| Error::internal("a seat's headroom does not fit a kW figure"))?;
        let mut entries = vec![
            (
                String::from("treasury_now"),
                wire_number(treasury.raw(), "treasury")?,
            ),
            (
                String::from("supply_kw_now"),
                wire_number(i64::from(supply.raw()), "supply")?,
            ),
            (
                String::from("draw_kw_now"),
                wire_number(i64::from(draw.raw()), "draw")?,
            ),
            (
                String::from("headroom_kw_now"),
                wire_number(headroom, "headroom")?,
            ),
        ];
        let next = if host.runner().outcome().is_some() {
            // An ended match settles nothing more.
            None
        } else {
            bmi_next(host.world(), seat)?
        };
        if let Some(bmi_next) = next {
            entries.push((
                String::from("bmi_next_dollars"),
                wire_number(bmi_next.raw(), "next BMI")?,
            ));
        }
        if host.runner().phase() == pharmakos_sim::runner::MatchPhase::Lull {
            let committed = self.committed_dollars(subject, seat)?;
            entries.push((
                String::from("committed_dollars"),
                Json::Number(committed.to_string()),
            ));
        }
        Ok(Json::Object(entries))
    }

    /// The `$` this round's seal orders, by the verifier's projection over it
    /// ([`Surface::get_economy_forecast`] says why), or 0 when the seat has
    /// sealed nothing this round.
    ///
    /// # Errors
    ///
    /// As [`Surface::seat_state`]. A seal always carries its price: every
    /// maker of one prices it ([`Surface::seal_commitment_of`]) and refuses one
    /// that does not.
    fn committed_dollars(
        &self,
        subject: crate::token::Subject,
        seat: SeatId,
    ) -> Result<i32, Error> {
        let round = self.host()?.runner().round();
        let Some(sealed) = self
            .seat_state(subject, seat)?
            .sealed
            .as_ref()
            .filter(|sealed| sealed.round() == round)
        else {
            return Ok(0);
        };
        // Priced once, by the seal's maker, from the decode it sealed (S3's
        // `fpstream`, part 2, fix pass 2): the spend is a function of the
        // text and the rules table alone (`projection::project`'s `spend`),
        // and this reads no text. A seal always carries it.
        Ok(sealed.committed_dollars())
    }

    /// The seat's own economy, as the hosted world has it: what the forecast
    /// reads, and what a seal is priced against.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::NotFound`] for a seat this match does not have,
    /// and [`crate::error::Code::Internal`] when the seat table has no
    /// treasury, supply or draw for it -- the table's columns are one length
    /// by construction.
    pub(super) fn seat_economy(
        &self,
        seat: SeatId,
    ) -> Result<pharmakos_sim::knowledge::SeatEconomy, Error> {
        let seats = self.host()?.world().seats();
        let row = seats
            .seats()
            .iter()
            .position(|held| *held == seat.raw())
            .ok_or_else(|| Error::not_found(said!("this match has no seat {}", seat.raw())))?;
        let column = |name: &str| {
            Error::internal(said!(
                "the seat table has no {} row",
                pharmakos_proto::json::grammar::quoted(name)
            ))
        };
        Ok(pharmakos_sim::knowledge::SeatEconomy {
            treasury: seats
                .treasuries()
                .get(row)
                .copied()
                .ok_or_else(|| column("treasury"))?,
            supply: seats
                .supplies()
                .get(row)
                .copied()
                .ok_or_else(|| column("supply"))?,
            draw: seats
                .draws()
                .get(row)
                .copied()
                .ok_or_else(|| column("draw"))?,
        })
    }

    /// What a seal of `playbook` by `seat` commits, in whole `$`, by the
    /// verifier's projection against the seat's economy as the hosted world
    /// has it: the figure the forecast's `committed_dollars` answers. It reads
    /// the canonical form a seal's maker decoded (S3's `vfyq2`, fix pass 1,
    /// item 3), and the maker carries the figure on the seal, so the forecast
    /// reads no text (S3's `fpstream`, part 2, fix pass 2).
    ///
    /// The one place a seal is priced, so the makers in `surface/sealed.rs`
    /// (`submit_plan`'s, an advised safe playbook's, the fallback's, a
    /// resume's and the harness's) cannot price it two ways. Two things make
    /// a playbook not price: its spend or draw leaving the sim's `$` and kW
    /// types ([`pharmakos_verifier::projection::ProjectionError::Overflow`]),
    /// and a spend that fits `$` but is past the wire's `int32`. Both are
    /// [`Unpriced::Playbook`], which the door refuses rather than seal a
    /// playbook whose own forecast could not answer. From S2 the verifier
    /// refuses both before a report qualifies (`E0603`, S2-14), so no
    /// qualifying playbook reaches the refusal; the door stays as defence in
    /// depth (decisions-log item 136 (5) (d)), and
    /// `the_seal_commitment_door_refuses_an_unpriced_seal` keeps it from
    /// rotting unseen.
    ///
    /// # Errors
    ///
    /// [`Unpriced::Playbook`] for a playbook the projection or the wire cannot
    /// carry, and [`Unpriced::Gateway`] for the rest: a rules table that does
    /// not price, a seat table missing a column.
    pub(super) fn seal_commitment_of(
        &self,
        seat: SeatId,
        playbook: &pharmakos_proto::gp::v1::Playbook,
    ) -> Result<i32, Unpriced> {
        let economy = self.seat_economy(seat).map_err(Unpriced::Gateway)?;
        let rules = self.host().map_err(Unpriced::Gateway)?.rules();
        let prices = pharmakos_verifier::projection::Prices::from_rules(rules).map_err(|gap| {
            Unpriced::Gateway(Error::internal(said!(
                "the rules table does not price: {}",
                pharmakos_proto::json::grammar::Said::engine_error(&gap)
            )))
        })?;
        let projection = pharmakos_verifier::projection::project(playbook, economy, &prices)
            .map_err(|why| Unpriced::Playbook(pharmakos_plan_core::Error::from(why).said()))?;
        let spend = projection.spend.raw();
        i32::try_from(spend).map_err(|_| {
            Unpriced::Playbook(said!(
                "it orders up to $ {}, past the `int32` the forecast's `committed_dollars` carries",
                spend
            ))
        })
    }

    /// `estimate_route`: item 61's travel estimate, exposed.
    ///
    /// Each leg is the walk the router serves, near or far, with the
    /// allowance per extra route (lane `p3fix`;
    /// [`pharmakos_sim::pathing::estimate::walk_ticks`]) -- it never steps,
    /// never forks -- through the adapter item 100 (1) puts in this crate,
    /// over the hosted world's own graph. Never optimistic from the tick the
    /// route is served (the owner's reading of item 57, 2026-11-21), and a
    /// fogged leg comes back flagged so the editor draws it dashed and labels
    /// it a bound rather than an ETA (item 57).
    ///
    /// **Only in a Lull** (the owner's answer of 2026-11-21: "Refuse in a
    /// Push"): every other phase -- a Push, a recap, the lobby, an ended
    /// match -- answers `PHASE_CLOSED` before anything is parsed, as the
    /// planning door closes the `plan`-scoped methods. The method stays
    /// `observe`-scoped; the gate is its own. In a Push the hosted world is
    /// the live one and the snapshot its beacon legs read is the frozen one,
    /// so an answer would mix two worlds; outside the Lull there is nothing
    /// to plan.
    ///
    /// From S3 each leg that reaches, and the whole route when every leg
    /// does, carries `travel_text`, the figure as the player reads it (the
    /// register's S3-18, items 57 and 61): plan-core's rendering, rounded up to
    /// a whole second and labelled "at most" when it is a bound
    /// ([`pharmakos_plan_core::travel`]), so the editor draws it and does no
    /// time maths.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] for a travel time the rendering
    /// refuses (one past what a game millisecond count holds), never drawn as
    /// a clamped figure; and the refusals of the waypoints' reads.
    pub(super) fn estimate_route(
        &self,
        subject: crate::token::Subject,
        request: &Request,
    ) -> Result<Json, Error> {
        if !self.time.planning_open() {
            return Err(Error::phase_closed(said!(
                "`estimate_route` prices a route over the frozen planning world, which is open \
                 only in a Lull: ask in a Lull"
            )));
        }
        let seat = Surface::seat_of(subject, "a route to estimate")?;
        let Some(Json::Array(waypoints)) = request.param("waypoints") else {
            return Err(Error::invalid(
                "`waypoints` is an array of two or more `gp.v1.Location`s",
            ));
        };
        if waypoints.len() < 2 {
            return Err(Error::invalid(said!(
                "a route has two or more waypoints and this has {}",
                waypoints.len()
            )));
        }
        // Before a single search runs: see [`MAX_WAYPOINTS`].
        if waypoints.len() > MAX_WAYPOINTS {
            return Err(Error::invalid(said!(
                "a route names at most {} waypoints and this names {}",
                MAX_WAYPOINTS,
                waypoints.len()
            )));
        }
        let mut places: Vec<Voxel> = Vec::with_capacity(waypoints.len());
        // One search scratch for every `covering` waypoint of the route, made
        // the first time one needs it.
        let mut scratch: Option<pharmakos_sim::pathing::Scratch> = None;
        for (index, waypoint) in waypoints.iter().enumerate() {
            let from = places.last().copied();
            places.push(self.place_of(seat, waypoint, index, from, &mut scratch)?);
        }

        let host = self.host()?;
        let routes = host.routes();
        let route = pharmakos_plan_core::travel::Route::estimate(&places, &routes);
        // A fault in a query is raised, never drawn as an unreachable leg
        // (`crate::routes`, "A fault is raised").
        routes.take_fault()?;
        let refused = |error: pharmakos_plan_core::travel::TravelError| {
            Error::internal(said!(
                "the route's travel has no rendering: {}",
                pharmakos_proto::json::grammar::Said::engine_error(&error)
            ))
        };
        let legs: Vec<Json> = route
            .legs
            .iter()
            .map(leg_json)
            .collect::<Result<Vec<Json>, Error>>()?;
        let mut entries = vec![(
            String::from("reachable"),
            Json::Bool(!route.is_unreachable()),
        )];
        // The total only for a route every leg of which reaches, as a leg's
        // own `ms` is written; and its text beside it, empty -- left out --
        // when the route does not reach.
        let total = route.total().map_err(refused)?;
        if let Some(total) = total {
            entries.push((String::from("ms"), Json::Number(total.raw().to_string())));
        }
        entries.push((String::from("legs"), Json::Array(legs)));
        if total.is_some() {
            entries.push((
                String::from("travel_text"),
                Json::String(route.render().map_err(refused)?),
            ));
        }
        Ok(Json::Object(entries))
    }

    /// One `gp.v1.Location` as the voxel it names.
    ///
    /// Three of the oneof's arms exist and all three are answered: a literal
    /// voxel; a `beacon_anchor`, fixed (`b_NN`) or a **selector** (`nearest`,
    /// `weakest`, `safest`); and the `safest` shorthand. A beacon is resolved
    /// over the **frozen planning snapshot** by the sim's own selector
    /// catalogue (`pharmakos_sim::interpreter::resolve_beacon_in`), which is
    /// the answer a decision taken on that world would give: the same own
    /// living beacons, the same ranking, the same tie to the lowest id. It
    /// used to send every ranked leg to the seat's first beacon, so a
    /// `nearest` leg was estimated to a beacon the commander would never walk
    /// to (S1's plan, the `fixs` lane). A selector the interpreter refuses at
    /// this stage (`most_threatened`, an enemy or tag filter) is refused here
    /// with the interpreter's own reason, and one that resolves to nothing is
    /// `NOT_FOUND`, as it would be a step failure at run time. Nothing is
    /// stepped: a selector ranks what the snapshot already holds.
    ///
    /// `from` is the waypoint before this one, already resolved: a `nearest`
    /// leg ranks from there, because at run time the selector resolves when
    /// its step starts, with the commander at the end of the leg before. The
    /// first waypoint has none and ranks from where the snapshot's commander
    /// stands.
    ///
    /// A fourth arm since S1's targeting: **`covering`**, which a
    /// `place_beacon` writes in its `at` (`docs/design/targeting.md`,
    /// "Surfaces": "`estimate_route` accepts `covering` and returns the
    /// site"). It answers the **site** the beacon would stand on -- the
    /// column the sim's own [`pharmakos_sim::targeting::cover`] chooses,
    /// ranked from `from` (or from the commander, for the first waypoint) as
    /// the step would rank from the commander when it starts -- standing on
    /// the ground, so the leg's `to` is that site and Easy keeps no second
    /// site algorithm (the register's S1-20). A reference that covers
    /// nothing is `NOT_FOUND` naming the step failure it would be
    /// (`no_target`, `illegal_site`): hidden, absent and someone else's are
    /// one answer. `on` is a Build target's anchor and never a place to walk
    /// to, so it is refused as any other non-place is. `scratch` is the
    /// route's one search scratch for its `covering` waypoints.
    fn place_of(
        &self,
        seat: SeatId,
        waypoint: &Json,
        index: usize,
        from: Option<Voxel>,
        scratch: &mut Option<pharmakos_sim::pathing::Scratch>,
    ) -> Result<Voxel, Error> {
        use pharmakos_sim::interpreter::{BeaconSpec, resolve_beacon_in};
        let at = format!("waypoint {index}");
        if let Some(voxel) = waypoint.get("voxel") {
            return read_voxel(voxel, &at);
        }
        if let Some(reference) = waypoint.get("covering") {
            return self.covering_site(seat, reference, &at, from, scratch);
        }
        if let Some(toward) = waypoint.get("toward") {
            return self.toward_site(seat, toward, &at, from, scratch);
        }
        let host = self.host()?;
        let spec = if let Some(reference) = waypoint.get("beacon_anchor") {
            let reference: pharmakos_proto::gp::v1::BeaconRef =
                pharmakos_proto::json::decode_json(reference).map_err(|error| {
                    Error::invalid(said!(
                        "{}: `beacon_anchor` is a `gp.v1.BeaconRef`: {}",
                        pharmakos_proto::json::grammar::quoted_pointer(&at),
                        error.said()
                    ))
                })?;
            crate::surface::plan_words::beacon_spec(&reference, host.rules()).map_err(
                |refusal| {
                    Error::invalid(said!(
                        "{}: {}",
                        pharmakos_proto::json::grammar::quoted_pointer(&at),
                        refusal
                    ))
                },
            )?
        } else if waypoint.get("safest").is_some() {
            BeaconSpec::Safest
        } else {
            return Err(Error::invalid(said!(
                "{} names no place: a waypoint is a `voxel`, a `beacon_anchor`, `safest` or \
                 `covering`",
                pharmakos_proto::json::grammar::quoted_pointer(&at)
            )));
        };
        let snapshot = host.runner().frozen().snapshot();
        let origin = match from {
            Some(voxel) => Some(fx_point(voxel, &at)?),
            None => None,
        };
        let Some(beacon) = resolve_beacon_in(snapshot, host.rules(), seat, spec, origin) else {
            let shown = pharmakos_proto::json::grammar::quoted_pointer(&at);
            return Err(Error::not_found(match spec {
                BeaconSpec::Own(ordinal) => said!(
                    "{} names `{}`, which this seat has not",
                    shown,
                    pharmakos_proto::json::grammar::quoted(
                        &pharmakos_sim::tables::own_beacon_name(ordinal)
                    )
                ),
                BeaconSpec::Foreign(handle) => said!(
                    "{} names `{}`, which is not this seat's to walk to by name",
                    shown,
                    pharmakos_proto::json::grammar::quoted(
                        &pharmakos_sim::tables::foreign_beacon_name(handle)
                    )
                ),
                _ => said!(
                    "{} is a beacon of this seat's, and none answers its selector",
                    shown
                ),
            }));
        };
        // The resolved beacon's row, read through its id (S2's plan, E1): the
        // snapshot's id column at that row must hold the same id, and the row
        // must not be free.
        let row = beacon
            .row()
            .ok()
            .map(pharmakos_sim::ids::Row::index)
            .filter(|row| {
                snapshot.beacon_id.get(*row) == Some(&beacon.raw())
                    && snapshot.beacon_free.get(*row) == Some(&0)
            })
            .ok_or_else(|| {
                Error::internal(said!(
                    "{}: the resolved beacon has no row",
                    pharmakos_proto::json::grammar::quoted_pointer(&at)
                ))
            })?;
        let axis = |offset: usize| {
            row.checked_mul(3)
                .and_then(|base| base.checked_add(offset))
                .and_then(|slot| snapshot.beacon_pos.get(slot).copied())
                .map(pharmakos_sim::math::fixed::Fx::from_raw)
        };
        match (axis(0), axis(1), axis(2)) {
            (Some(x), Some(y), Some(z)) => Ok(view::voxel_of([x, y, z])),
            _ => Err(Error::internal(said!(
                "{}: the resolved beacon has no anchor",
                pharmakos_proto::json::grammar::quoted_pointer(&at)
            ))),
        }
    }

    /// A `covering` waypoint's site: where the beacon would stand, as the sim
    /// chooses it ([`Surface::place_of`] says why this is here).
    ///
    /// The site is read off the hosted world, the beacons' spheres and the
    /// commander included, and the route's beacon and `safest` legs are read
    /// off the frozen snapshot; the two are the same world in a Lull, which
    /// is the only phase [`Surface::estimate_route`] answers in, so no
    /// answer mixes two worlds.
    fn covering_site(
        &self,
        seat: SeatId,
        reference: &Json,
        at: &str,
        from: Option<Voxel>,
        scratch: &mut Option<pharmakos_sim::pathing::Scratch>,
    ) -> Result<Voxel, Error> {
        use crate::targeting::Site;
        let reference: pharmakos_proto::gp::v1::FeatureRef =
            pharmakos_proto::json::decode_json(reference).map_err(|error| {
                Error::invalid(said!(
                    "{}: `covering` is a `gp.v1.FeatureRef`: {}",
                    pharmakos_proto::json::grammar::quoted_pointer(at),
                    error.said()
                ))
            })?;
        let spec = crate::targeting::spec_of(&reference, Site::Covering).map_err(|why| {
            Error::invalid(said!(
                "{}: {}",
                pharmakos_proto::json::grammar::quoted_pointer(at),
                why
            ))
        })?;
        let world = self.host()?.world();
        let origin = match from {
            Some(voxel) => fx_point(voxel, at)?,
            None => crate::targeting::commander_point(world, seat).ok_or_else(|| {
                Error::not_found(said!(
                    "{} covers from where the commander stands, and it is standing nowhere",
                    pharmakos_proto::json::grammar::quoted_pointer(at)
                ))
            })?,
        };
        let tally = pharmakos_sim::seams::UnitTally::new();
        let ground = crate::targeting::lend(world, &tally);
        let scratch = match scratch {
            Some(held) => held,
            None => scratch.insert(crate::targeting::scratch_for(world)?),
        };
        let found = pharmakos_sim::targeting::cover(&ground, scratch, seat, origin, spec).map_err(
            |failure| {
                Error::not_found(said!(
                    "{} covers nothing: its step would fail `{}`",
                    pharmakos_proto::json::grammar::quoted_pointer(at),
                    pharmakos_proto::json::grammar::quoted(failure.name())
                ))
            },
        )?;
        let [x, y] = found.site;
        let point = ground.standing(x, y).ok_or_else(|| {
            Error::internal(said!(
                "{}: the sim chose a site off the map",
                pharmakos_proto::json::grammar::quoted_pointer(at)
            ))
        })?;
        Ok(view::voxel_of(point))
    }

    /// A `toward` waypoint's site: where the stepping stone would stand, as the
    /// sim chooses it (S3's `pred`, E23), ranked from `from` (or from the
    /// commander, for the first waypoint) as `covering`'s is, in the Lull
    /// [`Surface::estimate_route`] answers in. A `toward` that finds no site
    /// is `NOT_FOUND` naming the step failure it would be.
    fn toward_site(
        &self,
        seat: SeatId,
        toward: &Json,
        at: &str,
        from: Option<Voxel>,
        scratch: &mut Option<pharmakos_sim::pathing::Scratch>,
    ) -> Result<Voxel, Error> {
        use crate::targeting::Site;
        let toward: pharmakos_proto::gp::v1::Toward = pharmakos_proto::json::decode_json(toward)
            .map_err(|error| {
                Error::invalid(said!(
                    "{}: `toward` is a `gp.v1.Toward`: {}",
                    pharmakos_proto::json::grammar::quoted_pointer(at),
                    error.said()
                ))
            })?;
        let reference = toward.feature.as_ref().ok_or_else(|| {
            Error::invalid(said!(
                "{}: `toward` names its `feature`",
                pharmakos_proto::json::grammar::quoted_pointer(at)
            ))
        })?;
        let spec = crate::targeting::spec_of(reference, Site::Toward).map_err(|why| {
            Error::invalid(said!(
                "{}: {}",
                pharmakos_proto::json::grammar::quoted_pointer(at),
                why
            ))
        })?;
        let world = self.host()?.world();
        let origin = match from {
            Some(voxel) => fx_point(voxel, at)?,
            None => crate::targeting::commander_point(world, seat).ok_or_else(|| {
                Error::not_found(said!(
                    "{} steps from where the commander stands, and it is standing nowhere",
                    pharmakos_proto::json::grammar::quoted_pointer(at)
                ))
            })?,
        };
        let tally = pharmakos_sim::seams::UnitTally::new();
        let ground = crate::targeting::lend(world, &tally);
        let scratch = match scratch {
            Some(held) => held,
            None => scratch.insert(crate::targeting::scratch_for(world)?),
        };
        let mut stones: Vec<pharmakos_sim::targeting::StoneCandidate> = Vec::new();
        let found = pharmakos_sim::targeting::toward_counted(
            &ground,
            scratch,
            &mut stones,
            seat,
            origin,
            spec,
        )
        .map_err(|failed| {
            Error::not_found(said!(
                "{} finds no stepping stone: its step would fail `{}`",
                pharmakos_proto::json::grammar::quoted_pointer(at),
                pharmakos_proto::json::grammar::quoted(failed.reason.name())
            ))
        })?;
        let [x, y] = found.site;
        let point = ground.standing(x, y).ok_or_else(|| {
            Error::internal(said!(
                "{}: the sim chose a site off the map",
                pharmakos_proto::json::grammar::quoted_pointer(at)
            ))
        })?;
        Ok(view::voxel_of(point))
    }

    /// Name, for every seat, every other seat's beacon it remembers or sees:
    /// **the one place a seat's `e_NN` handles are minted** (the main
    /// session's ruling R1 on the reviews of #128).
    ///
    /// Called where a Lull opens and where a save resumes, before anything is
    /// verified against the planning snapshot, and nowhere else: no
    /// `get_view`, `list_beacons` or `get_beacon` call mints a seat's handle,
    /// so the names -- which a verifier scope spells its sightings with
    /// ([`Surface::verifier_sightings`]) and every `report_hash` therefore
    /// reads -- depend on the sim's hashed sighting table and the frozen world
    /// alone, never on how often a client polled during the Push. New names
    /// are minted in the table's own `(seat, asset)` order, then for the
    /// beacons in live vision the table does not hold yet, by row.
    ///
    /// The sim's table keeps the tick a beacon was **last** seen at and not
    /// the first, so the names count up in that order rather than in
    /// first-sighting order; a resume rebuilds them from the table at its
    /// Lull, which reproduces the names a match had whenever every name it
    /// held was minted at that Lull. First-sighting order, which would make
    /// the names a pure function of hashed state across every Lull, is
    /// carried to the sim.
    ///
    /// # Errors
    ///
    /// As [`Surface::known_beacons`].
    pub(crate) fn name_sightings(&mut self) -> Result<(), Error> {
        let vision = crate::host::LiveVision::of(self.host()?.world());
        let seats: Vec<SeatId> = self.seats.iter().map(|slot| slot.seat).collect();
        for seat in seats {
            let viewer = Viewer::Seat(seat);
            // First what the seat remembers, in the sim's own sighting table
            // order, `(seat, asset)` ...
            let mut ids: Vec<u32> = remembered_beacons(hosted(self.host.as_ref())?.world(), seat)?
                .into_iter()
                .map(|(id, ..)| id)
                .collect();
            // ... then any other seat's beacon in its live vision of the
            // frozen world that the table does not hold yet (one placed since
            // the last decision tick), in table row order.
            let rows = self.host()?.world().beacons().ids().len();
            for row in 0..rows {
                if let Some((id, _, _, Seen::Live { .. })) =
                    self.live_beacon(viewer, &vision, row)?
                    && !ids.contains(&id)
                {
                    ids.push(id);
                }
            }
            for id in ids {
                let _ = self.views.foreign_beacon_name(viewer, id)?;
            }
        }
        Ok(())
    }

    /// `seat`'s sightings for its verifier scope: every other seat's beacon
    /// and commander it sees now, at an age of 0, and every one it remembers,
    /// at the age its sighting has (S2's plan, task `wgw`; the layout is
    /// `pharmakos_verifier::Sighting`'s, `vfy2`'s). Only what the seat itself
    /// saw: live vision is the sim's sensor interface through the fog filter,
    /// and memory is the sim's own sighting table. The verifier keeps the
    /// freshest of two sightings of one thing, so a beacon both in sight and
    /// remembered rests on its live sighting.
    ///
    /// Each sighting comes with the seat that owns what was seen, which the
    /// scope's [`pharmakos_verifier::KnownBeacon`] for a sighted enemy beacon
    /// names (the register's S3-04; [`Surface::verifier_scope`]).
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] for a sighted beacon this seat holds
    /// no handle for, which [`Surface::name_sightings`] gives it before any
    /// verification; and as [`sightings_of`] and [`Surface::live_beacon`].
    pub(crate) fn verifier_sightings(
        &self,
        seat: SeatId,
    ) -> Result<Vec<(pharmakos_verifier::Sighting, SeatId)>, Error> {
        let viewer = Viewer::Seat(seat);
        let world = self.host()?.world();
        let vision = crate::host::LiveVision::of(world);
        let handle = |id: u32| {
            self.views.foreign_beacon_handle(viewer, id).ok_or_else(|| {
                Error::internal(said!(
                    "seat {} sights beacon {} and holds no handle for it",
                    seat.raw(),
                    id
                ))
            })
        };
        let mut out: Vec<(pharmakos_verifier::Sighting, SeatId)> = Vec::new();
        let beacons = world.beacons();
        for row in 0..beacons.ids().len() {
            if let Some((id, at, owner, Seen::Live { .. })) =
                self.live_beacon(viewer, &vision, row)?
            {
                out.push((
                    pharmakos_verifier::Sighting {
                        what: pharmakos_verifier::Sighted::Beacon(handle(id)?),
                        at,
                        age_ms: 0,
                    },
                    owner,
                ));
            }
        }
        out.extend(self.commanders_in_sight(seat, &vision)?);
        for remembered in sightings_of(world, seat)? {
            let what = match remembered.kind {
                pharmakos_sim::knowledge::AssetKind::Beacon => {
                    pharmakos_verifier::Sighted::Beacon(handle(beacon_of(&remembered)?)?)
                }
                pharmakos_sim::knowledge::AssetKind::Commander => {
                    pharmakos_verifier::Sighted::Commander(remembered.owner)
                }
                // A unit, a structure or a wreck: no Attack target or
                // freshness rule reads one (spec section 10), and the
                // verifier's layout has no place for it. `Unknown` is a kind
                // the sim never records.
                pharmakos_sim::knowledge::AssetKind::Unit
                | pharmakos_sim::knowledge::AssetKind::Structure
                | pharmakos_sim::knowledge::AssetKind::Wreck
                | pharmakos_sim::knowledge::AssetKind::Unknown => continue,
            };
            out.push((
                pharmakos_verifier::Sighting {
                    what,
                    at: remembered.at,
                    age_ms: remembered.age_ms,
                },
                remembered.owner,
            ));
        }
        Ok(out)
    }

    /// The other seats' commanders `seat` sees now, at an age of 0, for
    /// [`Surface::verifier_sightings`], each with its owner.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when the unit table lacks a column.
    fn commanders_in_sight(
        &self,
        seat: SeatId,
        vision: &crate::host::LiveVision,
    ) -> Result<Vec<(pharmakos_verifier::Sighting, SeatId)>, Error> {
        let viewer = Viewer::Seat(seat);
        let world = self.host()?.world();
        let mut found = Vec::new();
        let filter = FogFilter::new(&self.fog, vision);
        let units = world.units();
        for row in 0..units.ids().len() {
            let missing = |name: &str| {
                Error::internal(said!(
                    "the unit table has no {} in row {}",
                    pharmakos_proto::json::grammar::quoted(name),
                    row
                ))
            };
            if units
                .frees()
                .get(row)
                .copied()
                .ok_or_else(|| missing("free flag"))?
            {
                continue;
            }
            let kind = units
                .kinds()
                .get(row)
                .copied()
                .ok_or_else(|| missing("kind"))?;
            let hp = units
                .hit_points()
                .get(row)
                .copied()
                .ok_or_else(|| missing("hit points"))?;
            if kind != pharmakos_sim::tables::UnitKind::Commander.id() || !hp.is_alive() {
                continue;
            }
            let owner = SeatId::new(
                units
                    .seats()
                    .get(row)
                    .copied()
                    .ok_or_else(|| missing("seat"))?,
            );
            if owner == seat {
                continue;
            }
            let at = view::voxel_of(
                units
                    .positions()
                    .get(row)
                    .copied()
                    .ok_or_else(|| missing("position"))?,
            );
            if filter.visible(
                viewer,
                &Audience::World {
                    owner: Some(owner),
                    at,
                },
            ) {
                found.push((
                    pharmakos_verifier::Sighting {
                        what: pharmakos_verifier::Sighted::Commander(owner),
                        at,
                        age_ms: 0,
                    },
                    owner,
                ));
            }
        }
        Ok(found)
    }

    /// One `gp.api.v1.BeaconSummary`, as `viewer` may be told it.
    ///
    /// `beacon_id`, `at` and `owner` for every beacon a viewer may see or
    /// remembers. For a seat's **own** beacon, also `core`, `priority` and
    /// `powered` (decisions-log item 111, decision C7): what a client needs to
    /// tell which of its beacons would brown out first, its `hp` with its
    /// `max_hp`, and from S3 its `sphere_radius_voxels`, the sim's one radius
    /// function's answer (S3's plan, E3), so the client draws a ring it never
    /// computes. **Another seat's beacon** carries what S2's plan's decision 5
    /// shows (decisions-log item 144 (11), the register's S2-11) and nothing
    /// more, under every fog policy and to every viewer that is not its owner:
    /// in live vision its `hp` and `max_hp` and a `sighting_age_ms` of 0, and
    /// from memory the place it was last seen and that sighting's age, with no
    /// hit points. Its mandate, power state, priority, units by role and build
    /// progress are never shown. A field left unset is left out rather than
    /// written as a zero a reader would take for an answer.
    ///
    /// **`max_hp` tells another seat's core from its other beacons**
    /// (`beacon.core_hp` against `structures.beacon.hp`), and that is accepted
    /// (the main session's ruling R3 on the reviews of #128): a seat's core is
    /// the beacon the map places for it at its spawn, public from the first
    /// tick, so the figure tells a seat nothing it could not see.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when the beacon table lacks a column
    /// for the row, or a figure does not fit the wire.
    fn beacon_summary(&self, known: &KnownBeacon) -> Result<Json, Error> {
        let at = known.at;
        let mut entries = vec![
            (String::from("beacon_id"), Json::String(known.name.clone())),
            (
                String::from("at"),
                Json::Object(vec![
                    (String::from("x"), Json::Number(at.x.to_string())),
                    (String::from("y"), Json::Number(at.y.to_string())),
                    (String::from("z"), Json::Number(at.z.to_string())),
                ]),
            ),
            (
                String::from("owner"),
                Json::String(crate::token::Subject::Seat(known.owner).render()),
            ),
        ];
        let hit_points = |entries: &mut Vec<(String, Json)>, (hp, max_hp): (u32, u32)| {
            entries.push((String::from("hp"), Json::Number(hp.to_string())));
            entries.push((String::from("max_hp"), Json::Number(max_hp.to_string())));
        };
        match known.seen {
            Seen::Own { row } => {
                let world = self.host()?.world();
                let beacons = world.beacons();
                let missing = |name: &str| {
                    Error::internal(said!(
                        "the beacon table has no {} in row {}",
                        pharmakos_proto::json::grammar::quoted(name),
                        row
                    ))
                };
                let core = core_beacon_of(world, known.owner) == Some(known.id);
                let priority = beacons
                    .priorities()
                    .get(row)
                    .copied()
                    .ok_or_else(|| missing("priority"))?;
                let dormant = beacons
                    .dormant()
                    .get(row)
                    .copied()
                    .ok_or_else(|| missing("dormancy"))?;
                entries.push((String::from("core"), Json::Bool(core)));
                entries.push((
                    String::from("priority"),
                    Json::String(priority_wire_name(priority)?),
                ));
                entries.push((String::from("powered"), Json::Bool(!dormant)));
                hit_points(&mut entries, self.beacon_hit_points(row, known.id)?);
                // The sphere's radius now, from the sim's one function (S3's
                // plan, E3): the base row, grown while a finished Resonance
                // Spire stands on the beacon. Own beacons only: another
                // seat's radius would tell whether its Spire stands.
                let radius =
                    world.sphere_radius_voxels_of(pharmakos_sim::tables::BeaconId::new(known.id));
                let radius = u32::try_from(radius).map_err(|_| {
                    Error::internal(said!(
                        "the sim sized {}'s sphere at {} voxels, which is no radius",
                        pharmakos_proto::json::grammar::quoted(&known.name),
                        radius
                    ))
                })?;
                entries.push((
                    String::from("sphere_radius_voxels"),
                    Json::Number(radius.to_string()),
                ));
            }
            Seen::Live { row } => {
                hit_points(&mut entries, self.beacon_hit_points(row, known.id)?);
                entries.push((
                    String::from("sighting_age_ms"),
                    Json::Number(String::from("0")),
                ));
            }
            Seen::Remembered { age_ms } => {
                let age = i32::try_from(age_ms).map_err(|_| {
                    Error::internal(said!(
                        "a sighting {} ms old does not fit the wire's int32 age",
                        age_ms
                    ))
                })?;
                entries.push((
                    String::from("sighting_age_ms"),
                    Json::Number(age.to_string()),
                ));
            }
        }
        Ok(Json::Object(entries))
    }

    /// One living beacon's hit points now and at full condition, as the
    /// wire's whole HP.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] for a row with no hit points, and as
    /// [`view::hp_wire`].
    fn beacon_hit_points(&self, row: usize, id: u32) -> Result<(u32, u32), Error> {
        let world = self.host()?.world();
        let hp = world
            .beacons()
            .hit_points()
            .get(row)
            .copied()
            .ok_or_else(|| {
                Error::internal(said!("the beacon table has no hit points in row {}", row))
            })?;
        let max = world.beacon_max_hp(pharmakos_sim::tables::BeaconId::new(id));
        Ok((view::hp_wire(hp)?, view::hp_wire(max)?))
    }

    /// Every beacon a viewer may see or remembers, with this viewer's name for
    /// each, own first by ordinal (`b_00` first), then everybody else's by this
    /// viewer's handle (`e_01` first).
    ///
    /// **Live vision first, then memory.** A beacon the fog filter lets the
    /// viewer see now is listed where it stands, with its hit points. A seat's
    /// own sightings of another seat's beacons that are not in sight now come
    /// from the sim's knowledge store (its hashed sighting table, which the one
    /// sensor interface fills on every decision tick; S2's plan, E10) at the
    /// place each was last seen, with that sighting's age: only what the seat
    /// itself saw, nothing inferred. A spectator and the lobby remember
    /// nothing, because memory is a seat's.
    ///
    /// Another seat's beacons are named in table order the first time they
    /// are shown together, so the handles a listing mints count up in the
    /// order a seat's own listing reads; a beacon seen later takes the next
    /// handle and sorts last, which keeps an earlier page's cursor honest.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when no match is hosted, a table row
    /// lacks a column, or a sighting cannot be read as a beacon's.
    fn known_beacons<V: Vision>(
        &mut self,
        viewer: Viewer,
        vision: &V,
    ) -> Result<Vec<KnownBeacon>, Error> {
        let mut found: Vec<(bool, usize, u32, Voxel, SeatId, Seen)> = Vec::new();
        {
            let world = self.host()?.world();
            let beacons = world.beacons();
            for row in 0..beacons.ids().len() {
                if let Some((id, at, owner, seen)) = self.live_beacon(viewer, vision, row)? {
                    found.push((seen != Seen::Own { row }, row, id, at, owner, seen));
                }
            }
            if let Viewer::Seat(seat) = viewer {
                for (id, at, owner, age_ms) in remembered_beacons(world, seat)? {
                    if found.iter().any(|(_, _, held, ..)| *held == id) {
                        continue;
                    }
                    found.push((
                        true,
                        row_key(id)?,
                        id,
                        at,
                        owner,
                        Seen::Remembered { age_ms },
                    ));
                }
            }
        }
        // Named in table order, which is the order a first listing mints the
        // `e_NN` handles in; the id breaks a tie between a remembered beacon
        // and a live one that took its row.
        found.sort_unstable_by_key(|(foreign, key, id, ..)| (*foreign, *key, *id));
        let mut named: Vec<(bool, u32, KnownBeacon)> = Vec::with_capacity(found.len());
        for (foreign, _, id, at, owner, seen) in found {
            let name = match seen {
                Seen::Own { row } => {
                    let world = hosted(self.host.as_ref())?.world();
                    self.views.beacon_name(viewer, world, row)?
                }
                Seen::Live { .. } | Seen::Remembered { .. } => match viewer {
                    // A seat's handles are minted only where a Lull opens
                    // ([`Surface::name_sightings`]): a beacon it first sees
                    // in a Push is listed once the next Lull names it.
                    Viewer::Seat(_) => match self.views.foreign_beacon_handle(viewer, id) {
                        Some(name) => name,
                        None => continue,
                    },
                    // A spectator's names reach no report, so it is shown a
                    // beacon under the handle minted the first time it sees it.
                    Viewer::Spectator { .. } | Viewer::Admin => {
                        self.views.foreign_beacon_name(viewer, id)?
                    }
                },
            };
            let Some(
                pharmakos_sim::tables::BeaconName::Own(number)
                | pharmakos_sim::tables::BeaconName::Foreign(number),
            ) = pharmakos_sim::tables::parse_beacon_name(&name)
            else {
                return Err(Error::internal(said!(
                    "the view feed minted `{}`, which is no beacon name",
                    pharmakos_proto::json::grammar::quoted(&name)
                )));
            };
            named.push((
                foreign,
                number,
                KnownBeacon {
                    name,
                    id,
                    at,
                    owner,
                    seen,
                },
            ));
        }
        // Own beacons first, then by the number in the name: every sort key
        // ends in a unique value (item 62's convention, which this crate keeps
        // because its outputs are byte-compared across three operating
        // systems), and within a side a name's number is unique.
        named.sort_unstable_by_key(|(foreign, number, known)| (*foreign, *number, known.id));
        Ok(named.into_iter().map(|(_, _, known)| known).collect())
    }

    /// The one beacon with id `id`, if `viewer` may see it now or remembers
    /// it, with the name this viewer already holds for it. Mints nothing:
    /// [`Surface::get_beacon`] reaches it only through a name the viewer was
    /// given.
    ///
    /// # Errors
    ///
    /// As [`Surface::known_beacons`].
    fn known_beacon<V: Vision>(
        &mut self,
        viewer: Viewer,
        vision: &V,
        id: u32,
    ) -> Result<Option<KnownBeacon>, Error> {
        let live = match self
            .host()?
            .world()
            .beacons()
            .row_of(pharmakos_sim::tables::BeaconId::new(id))
        {
            Ok(row) => self.live_beacon(viewer, vision, row.index())?,
            // An id whose row has been freed or reused names no beacon that
            // stands: the seat can only remember it.
            Err(_) => None,
        };
        let world = hosted(self.host.as_ref())?.world();
        if let Some((id, at, owner, seen)) = live {
            let name = match seen {
                Seen::Own { row } => self.views.beacon_name(viewer, world, row)?,
                Seen::Live { .. } | Seen::Remembered { .. } => {
                    match self.views.foreign_beacon_handle(viewer, id) {
                        Some(name) => name,
                        None => return Ok(None),
                    }
                }
            };
            return Ok(Some(KnownBeacon {
                name,
                id,
                at,
                owner,
                seen,
            }));
        }
        let Viewer::Seat(seat) = viewer else {
            return Ok(None);
        };
        let Some((id, at, owner, age_ms)) = remembered_beacons(world, seat)?
            .into_iter()
            .find(|(held, ..)| *held == id)
        else {
            return Ok(None);
        };
        let Some(name) = self.views.foreign_beacon_handle(viewer, id) else {
            return Ok(None);
        };
        Ok(Some(KnownBeacon {
            name,
            id,
            at,
            owner,
            seen: Seen::Remembered { age_ms },
        }))
    }

    /// The beacon in `row`, if it stands and `viewer` may see it now: its id,
    /// its place, its owner and how it is seen.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when no match is hosted or the row
    /// lacks a column.
    /// The row of `seat`'s own beacon `wanted` names, read as `get_beacon`
    /// reads a name (the view feed's [`crate::viewfeed`] `beacon_row`, then
    /// the standing beacon of that row, [`Surface::live_beacon`]), or `None`
    /// for every name that is not one of the seat's own **standing** beacons:
    /// another seat's, a foreign handle, a dead beacon whose row the table
    /// still holds (a core, or kill credit owed), a name that names nothing.
    /// One answer for all of them, as `get_beacon`'s `NOT_FOUND` is one (S3's
    /// `fixq`, review fix pass 1, item 5).
    ///
    /// # Errors
    ///
    /// As [`Surface::live_beacon`].
    pub(super) fn own_standing_beacon<V: Vision>(
        &self,
        seat: SeatId,
        vision: &V,
        wanted: &str,
    ) -> Result<Option<usize>, Error> {
        let viewer = Viewer::Seat(seat);
        let world = self.host()?.world();
        let Some(row) = self.views.beacon_row(viewer, world, wanted) else {
            return Ok(None);
        };
        Ok(match self.live_beacon(viewer, vision, row)? {
            Some((_, _, _, Seen::Own { row: own })) if own == row => Some(row),
            Some(_) | None => None,
        })
    }

    fn live_beacon<V: Vision>(
        &self,
        viewer: Viewer,
        vision: &V,
        row: usize,
    ) -> Result<Option<(u32, Voxel, SeatId, Seen)>, Error> {
        let beacons = self.host()?.world().beacons();
        // A freed row is no beacon at all (S2's plan, E1): listed, it would
        // be a phantom beacon of the no-seat value's.
        let missing = |name: &str| {
            Error::internal(said!(
                "the beacon table has no {} in row {}",
                pharmakos_proto::json::grammar::quoted(name),
                row
            ))
        };
        if beacons
            .frees()
            .get(row)
            .copied()
            .ok_or_else(|| missing("free flag"))?
        {
            return Ok(None);
        }
        let hp = beacons
            .hit_points()
            .get(row)
            .copied()
            .ok_or_else(|| missing("hit points"))?;
        if !hp.is_alive() {
            return Ok(None);
        }
        let owner = SeatId::new(
            beacons
                .seats()
                .get(row)
                .copied()
                .ok_or_else(|| missing("seat"))?,
        );
        let at = view::voxel_of(
            beacons
                .positions()
                .get(row)
                .copied()
                .ok_or_else(|| missing("position"))?,
        );
        let id = beacons
            .ids()
            .get(row)
            .copied()
            .ok_or_else(|| missing("id"))?;
        let filter = FogFilter::new(&self.fog, vision);
        let audience = Audience::World {
            owner: Some(owner),
            at,
        };
        if !filter.visible(viewer, &audience) {
            return Ok(None);
        }
        let seen = if viewer == Viewer::Seat(owner) {
            Seen::Own { row }
        } else {
            Seen::Live { row }
        };
        Ok(Some((id, at, owner, seen)))
    }
}

/// How a viewer knows one beacon.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Seen {
    /// One of the viewer's own, standing in `row`.
    Own {
        /// Its row of the beacon table.
        row: usize,
    },
    /// Another seat's, standing in `row`, in the viewer's live vision now.
    Live {
        /// Its row of the beacon table.
        row: usize,
    },
    /// Another seat's, from the viewer's memory, last seen `age_ms` ago on
    /// match game time.
    Remembered {
        /// Game milliseconds since the seat last saw it.
        age_ms: u32,
    },
}

/// One beacon as a viewer knows it, named as that viewer names it.
#[derive(Clone, PartialEq, Eq, Debug)]
struct KnownBeacon {
    /// This viewer's name for it: `b_NN` or `e_NN`.
    name: String,
    /// The beacon's id, generation and all.
    id: u32,
    /// Where it stands, or where it was last seen.
    at: Voxel,
    /// The seat that owns it, or owned it when it was seen.
    owner: SeatId,
    /// How the viewer knows it.
    seen: Seen,
}

/// A beacon id's row, the key a listing mints handles in: table order.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] for an id outside the sim's id layout,
/// which no sighting the sim records holds.
fn row_key(id: u32) -> Result<usize, Error> {
    pharmakos_sim::tables::BeaconId::new(id)
        .row()
        .map(pharmakos_sim::ids::Row::index)
        .map_err(|error| {
            Error::internal(said!(
                "a sighted beacon's id {} has no row: {}",
                id,
                crate::error::engine(&error)
            ))
        })
}

/// The hosted match, borrowed through the field alone, so the view feed can be
/// written while the world is read.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] when no match is hosted.
fn hosted(host: Option<&crate::host::Host>) -> Result<&crate::host::Host, Error> {
    host.ok_or_else(|| Error::internal("this gateway is not hosting a match"))
}

/// `seat`'s own sightings of other seats' beacons, from the sim's knowledge
/// store: each beacon's id, the place it was last seen, the seat that owned it
/// and the sighting's age, in the table's own order.
///
/// # Errors
///
/// As [`sightings_of`].
pub(crate) fn remembered_beacons(
    world: &pharmakos_sim::world::World,
    seat: SeatId,
) -> Result<Vec<(u32, Voxel, SeatId, u32)>, Error> {
    sightings_of(world, seat)?
        .into_iter()
        .filter(|sighting| sighting.kind == pharmakos_sim::knowledge::AssetKind::Beacon)
        .map(|sighting| {
            Ok((
                beacon_of(&sighting)?,
                sighting.at,
                sighting.owner,
                sighting.age_ms,
            ))
        })
        .collect()
}

/// The beacon id a sighting of a beacon names.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] for a sighting recorded as a beacon's
/// whose asset id is tagged as another kind's: the sim writes the two
/// together, so they disagreeing is the sim and the gateway disagreeing.
pub(crate) fn beacon_of(sighting: &Remembered) -> Result<u32, Error> {
    if sighting.asset.tag() != pharmakos_sim::knowledge::AssetId::TAG_BEACON {
        return Err(Error::internal(said!(
            "a sighting of a beacon names asset {}, which is tagged as another kind's",
            sighting.asset.raw()
        )));
    }
    Ok(sighting.asset.index())
}

/// One of a seat's own sightings, read off the sim's hashed sighting table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Remembered {
    /// What was seen.
    pub asset: pharmakos_sim::knowledge::AssetId,
    /// Of what kind.
    pub kind: pharmakos_sim::knowledge::AssetKind,
    /// The seat that owned it.
    pub owner: SeatId,
    /// Where it was last seen, as a whole voxel.
    pub at: Voxel,
    /// Game milliseconds since it was last seen, on match game time across
    /// Pushes: the table keeps the tick it was taken at, and the age is
    /// derived against the world's tick, so the Lull and the recap add
    /// nothing to it (spec section 6).
    pub age_ms: u32,
}

/// Every sighting `seat` holds, in the table's own `(seat, asset)` order: only
/// what the seat itself saw (S2's plan, decision 5, from memory).
///
/// # Errors
///
/// [`crate::error::Code::Internal`] when the table's columns disagree in
/// length, a row holds a kind this build does not name, a sighting was taken
/// after the world's own tick, or its age does not fit a `u32` of
/// milliseconds.
pub(crate) fn sightings_of(
    world: &pharmakos_sim::world::World,
    seat: SeatId,
) -> Result<Vec<Remembered>, Error> {
    let table = world.sightings();
    let now = world.tick().raw();
    let rows = table.seats().len();
    let short = |name: &str| {
        Error::internal(said!(
            "the sighting table holds {} seats and fewer {}",
            rows,
            pharmakos_proto::json::grammar::quoted(name)
        ))
    };
    let mut out: Vec<Remembered> = Vec::new();
    for row in 0..rows {
        if table.seats().get(row).copied() != Some(seat.raw()) {
            continue;
        }
        let asset = table
            .assets()
            .get(row)
            .copied()
            .ok_or_else(|| short("assets"))?;
        let raw_kind = table
            .kinds()
            .get(row)
            .copied()
            .ok_or_else(|| short("kinds"))?;
        let kind = pharmakos_sim::knowledge::AssetKind::from_id(raw_kind).ok_or_else(|| {
            Error::internal(said!(
                "a sighting holds kind {}, which this build does not name",
                raw_kind
            ))
        })?;
        let owner = table
            .owners()
            .get(row)
            .copied()
            .ok_or_else(|| short("owners"))?;
        let place = table
            .places()
            .get(row)
            .copied()
            .ok_or_else(|| short("places"))?;
        let seen_at = table
            .seen_at()
            .get(row)
            .copied()
            .ok_or_else(|| short("ticks"))?;
        out.push(Remembered {
            asset: pharmakos_sim::knowledge::AssetId::new(asset),
            kind,
            owner: SeatId::new(owner),
            at: view::voxel_of(place),
            age_ms: age_ms(now, seen_at)?,
        });
    }
    Ok(out)
}

/// The age of a sighting taken at tick `seen_at`, at tick `now`, in game
/// milliseconds.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] for a sighting taken after `now`, which
/// the sim cannot record, and for an age past a `u32` of milliseconds, which
/// no match of this project's lengths reaches; neither is clamped.
pub(crate) fn age_ms(now: u32, seen_at: u32) -> Result<u32, Error> {
    let ticks = now.checked_sub(seen_at).ok_or_else(|| {
        Error::internal(said!(
            "a sighting was taken at tick {}, after the world's tick {}",
            seen_at,
            now
        ))
    })?;
    let per_tick = u32::try_from(pharmakos_sim::math::quantity::MS_PER_TICK)
        .map_err(|_| Error::internal("the sim's tick is no whole number of milliseconds"))?;
    ticks.checked_mul(per_tick).ok_or_else(|| {
        Error::internal(said!(
            "a sighting {} ticks old does not fit a u32 of milliseconds",
            ticks
        ))
    })
}

/// A page's `limit`: as asked, never past `budget`, and the budget when it is
/// not asked.
///
/// # Errors
///
/// [`crate::error::Code::InvalidArgument`] for a negative limit, which counts
/// no page a client can mean; it is refused rather than read as the budget.
pub(crate) fn limit_param(request: &Request, budget: usize) -> Result<usize, Error> {
    match request.integer_param("limit")? {
        None => Ok(budget),
        Some(asked) => usize::try_from(asked)
            .map(|asked| asked.min(budget))
            .map_err(|_| Error::invalid(said!("`limit` is a count, and {} is not one", asked))),
    }
}
/// The recap's settlement for `seat`, the seat's own, and its sentence pushed
/// onto `prose` ([`Surface::get_recap`] says what it carries). `None` when the
/// Ledger credited the seat nothing at this segment end.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] when the credit does not fit the wire's
/// `int32`, or the band read finds the sim and this crate disagreeing
/// ([`BandError::Gateway`]). A feed loss ([`BandError::FeedLost`]) is no
/// error: the settlement is told without its band.
fn settlement_of(
    facts: &crate::surface::RecapFacts,
    rules: &pharmakos_sim::rules::RulesTable,
    seat: SeatId,
    prose: &mut String,
) -> Result<Option<Json>, Error> {
    let credited = facts.settled.iter().find(|(held, _)| *held == seat);
    let band = match facts.band(rules, seat) {
        Ok(band) => band.map(Some),
        // The rest of the recap is still told; only the band is not.
        Err(BandError::FeedLost { .. }) => Some(None),
        Err(BandError::Gateway(error)) => return Err(error),
    };
    if let (Some((_, credited)), Some(band)) = (credited, band) {
        let bmi = i32::try_from(*credited).map_err(|_| {
            Error::internal("the Ledger credited more than the wire's int32 carries")
        })?;
        prose.push(' ');
        let mut settlement = vec![(String::from("bmi_dollars"), Json::Number(bmi.to_string()))];
        match band {
            Some(band) => {
                prose.push_str(&strings::settlement(bmi, band.rank, band.percent));
                settlement.push((
                    String::from("band_rank"),
                    Json::Number(band.rank.to_string()),
                ));
                settlement.push((
                    String::from("band_percent"),
                    Json::Number(band.percent.to_string()),
                ));
            }
            None => prose.push_str(&strings::settlement_unbanded(bmi)),
        }
        settlement.push((
            String::from("award_dollars"),
            Json::Number(String::from("0")),
        ));
        return Ok(Some(Json::Object(settlement)));
    }
    Ok(None)
}

/// Why `seat`'s steps found nothing in the segment, pushed onto `prose`: each
/// distinct `step_failed` value once, with how often it happened, in the order
/// it first happened ([`strings::recap_step_failed`]).
///
/// # Errors
///
/// [`crate::error::Code::Internal`] for a count past `usize`, which no segment
/// reaches: refused rather than saturated.
fn failure_lines(
    facts: &crate::surface::RecapFacts,
    seat: SeatId,
    prose: &mut String,
) -> Result<(), Error> {
    // Why the seat's steps found nothing, each distinct failure once
    // with how often it happened, in the order it first happened.
    let mut failures: Vec<(i64, NonZeroUsize)> = Vec::new();
    for (_, value) in facts.failures.iter().filter(|(held, _)| *held == seat) {
        match failures.iter_mut().find(|(seen, _)| seen == value) {
            Some((_, times)) => {
                *times = times.checked_add(1).ok_or_else(|| {
                    Error::internal("a step failed more times than a count holds")
                })?;
            }
            None => failures.push((*value, NonZeroUsize::MIN)),
        }
    }
    for (value, times) in failures {
        prose.push(' ');
        prose.push_str(&strings::recap_step_failed(value, times));
    }
    Ok(())
}

/// What [`Surface::map_features`] tells a seat of its travel to each
/// feature: the walks priced over the frozen planning world in a Lull, or
/// nothing at all in any other phase.
enum Travel {
    /// Outside a Lull: neither `travel_ms` nor `reachable` is written.
    Withheld,
    /// In a Lull: every feature the commander's walk reaches, with its walk;
    /// a feature not listed is unreachable.
    Priced(Vec<(usize, pharmakos_sim::pathing::estimate::ServedWalk)>),
}

/// The calling seat's own tally for [`Surface::get_recap`], from the counters
/// the recap's facts hold for it, with its sentence pushed onto `prose`.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] for a seat the facts hold no tally for,
/// and as [`tally_json`].
fn own_tally(
    facts: &RecapFacts,
    seat: SeatId,
    ended: bool,
    prose: &mut String,
) -> Result<Json, Error> {
    let tally = facts
        .tallies
        .iter()
        .find(|(held, _)| *held == seat)
        .map(|(_, tally)| *tally)
        .ok_or_else(|| {
            Error::internal(said!("the recap holds no tally for seat {}", seat.raw()))
        })?;
    let (json, destroyed) = tally_json(tally)?;
    prose.push(' ');
    prose.push_str(&strings::recap_tally(
        tally.kills,
        tally.shared_kills,
        destroyed,
        tally.losses,
        ended,
    ));
    Ok(json)
}

/// The per-beacon detail `get_beacon` tells about one of the seat's own
/// beacons (`GetBeaconResponse` fields 3 to 5; the register's X-17), read off
/// the hosted world's own columns for the beacon in `row`, whose writ is
/// `mandate`.
///
/// * **`mandate`**, lower case (decisions-log item 80), left out on a beacon
///   with none.
/// * **`settings`**, one `gp.api.v1.BeaconSettingRow` per setting the writ
///   holds a value for, in the schema's field order: the common two first
///   (`roe`, `retreat_hp_pct`), then the writ's own. Each row's pointer is the
///   setting's inside `gp.v1.MandateSettings`, its title and shape are the
///   vocabulary's for that field ([`pharmakos_plan_core::vocabulary`]), so a
///   setting is drawn and edited as a picker's parameter is, and its value is
///   JSON text in the playbook's canonical spelling. Each row shows the
///   setting's **effective** value, the one the sim reads (S3's `gwr` review,
///   item 7): an unset `roe` as its writ's (`Roe::of_setting`), an unset
///   engagement radius, pursue, retarget, after-action or wave count as the
///   sim's own reading of it (`engagement_reading`, `pursue_reading`,
///   `retarget_reading`, `after_reading`, `waves_reading`), as the seam
///   choice and the terraform already were. An Attack's launch and retreat
///   triggers, which are off rather than defaulted when unwritten, have a row
///   only when written. A list (a protect list, the avoid areas, the probe
///   areas) and an Attack's target and staging point, which no one shape
///   describes or which the sim holds resolved, have no row.
/// * **`build_targets`**, a Build beacon's targets in the Quartermaster's
///   order, each a `gp.v1.BuildTarget`: its blueprint, its anchor as it was
///   written (a voxel, `on` a named feature, or `on` the nearest vent, its
///   rank in the wire's lower case), its rotation, its order and, for a wall,
///   its `wall_box`.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] when a column lacks the row, or holds a
/// value the schema does not name.
fn beacon_detail(
    world: &pharmakos_sim::world::World,
    row: usize,
    mandate: pharmakos_sim::seams::MandateKind,
) -> Result<Vec<(String, Json)>, Error> {
    use pharmakos_sim::seams::MandateKind;
    let beacons = world.beacons();
    let missing = |name: &str| {
        Error::internal(said!(
            "the beacon table has no {} in row {}",
            pharmakos_proto::json::grammar::quoted(name),
            row
        ))
    };
    let id = pharmakos_sim::tables::BeaconId::new(
        beacons
            .ids()
            .get(row)
            .copied()
            .ok_or_else(|| missing("id"))?,
    );
    let mut settings = Settings::new()?;
    let common = beacons
        .commons()
        .get(row)
        .copied()
        .ok_or_else(|| missing("common settings"))?;
    let roe = pharmakos_sim::world::Roe::of_setting(common.roe, mandate).ok_or_else(|| {
        Error::internal(said!(
            "the beacon table holds rules of engagement {}, which the sim does not name",
            common.roe
        ))
    })?;
    let roe = match roe {
        pharmakos_sim::world::Roe::Hold => "HOLD_FIRE",
        pharmakos_sim::world::Roe::Return => "RETURN_FIRE",
        pharmakos_sim::world::Roe::Free => "FREE_FIRE",
    };
    settings.word("", "/roe", "gp.v1.MandateSettings.Roe", roe)?;
    settings.number("", "/retreat_hp_pct", common.retreat_hp_pct)?;
    match mandate {
        MandateKind::None => {}
        MandateKind::Build => build_rows(beacons, row, &mut settings)?,
        MandateKind::Defend => defend_rows(world, id, &mut settings)?,
        MandateKind::Attack => attack_rows(world, id, &mut settings)?,
        MandateKind::Survey => {
            let scouts = beacons
                .scout_counts()
                .get(row)
                .copied()
                .ok_or_else(|| missing("scout count"))?;
            settings.number("survey", "/scout_count", scouts)?;
        }
        MandateKind::Mine => mine_rows(beacons, row, &mut settings)?,
    }
    let mut out = Vec::new();
    let wire = view::mandate_of(mandate);
    if wire != pharmakos_proto::gp::v1::beacon_filter::MandateKind::Unspecified {
        out.push((
            String::from("mandate"),
            Json::String(pharmakos_proto::scope::wire_name(
                "gp.v1.BeaconFilter.MandateKind",
                wire.as_str_name(),
            )),
        ));
    }
    if !settings.rows.is_empty() {
        out.push((String::from("settings"), Json::Array(settings.rows)));
    }
    if mandate == MandateKind::Build {
        let targets = build_targets(world, id)?;
        if !targets.is_empty() {
            // One sentence per target, in its order, from plan-core's words
            // for a Build target (S3's `fixq`, item 6), read off the target
            // as the wire carries it, so the sentence and the JSON beside it
            // cannot say two things.
            let lines = targets
                .iter()
                .map(|target| {
                    let canonical = canonical_enums("gp.v1.BuildTarget", target)?;
                    pharmakos_proto::json::decode_json::<pharmakos_proto::gp::v1::BuildTarget>(
                        &canonical,
                    )
                    .map(|target| Json::String(pharmakos_plan_core::render::target_line(&target)))
                    .map_err(|error| {
                        Error::internal(said!(
                            "a Build target this gateway wrote does not decode: {}",
                            pharmakos_proto::json::grammar::Said::engine_error(&error)
                        ))
                    })
                })
                .collect::<Result<Vec<Json>, Error>>()?;
            out.push((String::from("build_targets"), Json::Array(targets)));
            out.push((String::from("build_target_lines"), Json::Array(lines)));
        }
    }
    Ok(out)
}

/// `value`, read as the message `full_name`, with every enum value this
/// gateway spelt lower case on the wire (decisions-log item 80) spelt back as
/// the schema spells it, so a message this gateway wrote decodes (S3's
/// `fixq`: `get_beacon`'s target lines read the targets as written).
///
/// # Errors
///
/// [`crate::error::Code::Internal`] for an enum value the schema does not
/// name: this gateway writing a spelling it cannot read back.
fn canonical_enums(full_name: &str, value: &Json) -> Result<Json, Error> {
    use pharmakos_proto::descriptor::{self, ScalarKind};
    let (Some(message), Json::Object(entries)) = (descriptor::schema().message(full_name), value)
    else {
        return Ok(value.clone());
    };
    let one = |kind: ScalarKind, type_name: &str, item: &Json| match (kind, item) {
        (ScalarKind::Enum, Json::String(wire)) => {
            pharmakos_proto::scope::from_wire_name(type_name, wire)
                .map(Json::String)
                .ok_or_else(|| {
                    Error::internal(said!(
                        "this gateway wrote {} for {}, which the schema does not name",
                        pharmakos_proto::json::grammar::quoted(wire),
                        pharmakos_proto::json::grammar::quoted(type_name)
                    ))
                })
        }
        (ScalarKind::Message, Json::Object(_)) => canonical_enums(type_name, item),
        _ => Ok(item.clone()),
    };
    let mut out = Vec::with_capacity(entries.len());
    for (key, entry) in entries {
        let converted = match message.field_by_name(key) {
            Some(field) => match entry {
                Json::Array(items) if field.repeated => Json::Array(
                    items
                        .iter()
                        .map(|item| one(field.kind, &field.type_name, item))
                        .collect::<Result<Vec<Json>, Error>>()?,
                ),
                other => one(field.kind, &field.type_name, other)?,
            },
            None => entry.clone(),
        };
        out.push((key.clone(), converted));
    }
    Ok(Json::Object(out))
}

/// A Build beacon's settings rows.
///
/// # Errors
///
/// As [`beacon_detail`].
fn build_rows(
    beacons: &pharmakos_sim::tables::BeaconTable,
    row: usize,
    settings: &mut Settings,
) -> Result<(), Error> {
    let missing = |what: &str| {
        Error::internal(said!(
            "the beacon detail has no {} for its beacon",
            pharmakos_proto::json::grammar::quoted(what)
        ))
    };
    let site = beacons
        .sites()
        .get(row)
        .copied()
        .ok_or_else(|| missing("site"))?
        .settings;
    settings.number("build", "/repair_threshold_pct", site.repair_threshold_pct)?;
    settings.boolean("build", "/rebuild_destroyed", site.rebuild_destroyed)?;
    let terraform = match site.terraform {
        pharmakos_sim::site::Terraform::None => "NONE",
        pharmakos_sim::site::Terraform::Fill => "FILL",
        pharmakos_sim::site::Terraform::Dig => "DIG",
        pharmakos_sim::site::Terraform::Both => "BOTH",
    };
    settings.word(
        "build",
        "/terraform",
        "gp.v1.BuildSettings.Terraform",
        terraform,
    )?;
    Ok(())
}

/// A Defend beacon's settings rows.
///
/// # Errors
///
/// As [`beacon_detail`].
fn defend_rows(
    world: &pharmakos_sim::world::World,
    id: pharmakos_sim::tables::BeaconId,
    settings: &mut Settings,
) -> Result<(), Error> {
    let missing = |what: &str| {
        Error::internal(said!(
            "the beacon detail has no {} for its beacon",
            pharmakos_proto::json::grammar::quoted(what)
        ))
    };
    let defend = world
        .defend()
        .row_of(id)
        .ok_or_else(|| missing("Defend row"))?
        .settings;
    settings.number(
        "defend",
        "/engagement_radius_pct",
        defend.engagement_reading(),
    )?;
    settings.name(
        "defend",
        "/pursue",
        "gp.v1.DefendSettings.Pursue",
        i32::from(defend.pursue_reading().id()),
    )?;
    settings.number("defend", "/picket_scouts", defend.pickets)?;
    Ok(())
}

/// An Attack beacon's settings rows.
///
/// # Errors
///
/// As [`beacon_detail`].
fn attack_rows(
    world: &pharmakos_sim::world::World,
    id: pharmakos_sim::tables::BeaconId,
    settings: &mut Settings,
) -> Result<(), Error> {
    let missing = |what: &str| {
        Error::internal(said!(
            "the beacon detail has no {} for its beacon",
            pharmakos_proto::json::grammar::quoted(what)
        ))
    };
    let attack = world
        .attack()
        .row_of(id)
        .ok_or_else(|| missing("Attack row"))?
        .settings;
    let retarget = match attack.retarget_reading() {
        pharmakos_sim::attack::Retarget::None => "NONE",
        pharmakos_sim::attack::Retarget::NearestKnown => "NEAREST_KNOWN",
        pharmakos_sim::attack::Retarget::Unset => {
            return Err(Error::internal(
                "the sim's retarget reading is unset, which it never answers",
            ));
        }
    };
    settings.word(
        "attack",
        "/retarget",
        "gp.v1.AttackSettings.Retarget",
        retarget,
    )?;
    if let Some(force) = attack.force_size {
        settings.number("attack", "/launch/force_size", force)?;
    }
    if let Some(ms) = attack.launch_ms {
        settings.number("attack", "/launch/segment_ms", ms)?;
    }
    if let Some(pct) = attack.retreat_pct {
        settings.number("attack", "/retreat/force_below_pct", pct)?;
    }
    if let Some(ms) = attack.retreat_ms {
        settings.number("attack", "/retreat/after_launch_ms", ms)?;
    }
    let after = match attack.after_reading() {
        pharmakos_sim::attack::AfterAction::Regroup => "REGROUP_AT_STAGING",
        pharmakos_sim::attack::AfterAction::ReturnHome => "RETURN_HOME",
        pharmakos_sim::attack::AfterAction::Unset => {
            return Err(Error::internal(
                "the sim's after-action reading is unset, which it never answers",
            ));
        }
    };
    settings.word(
        "attack",
        "/after_action",
        "gp.v1.AttackSettings.AfterAction",
        after,
    )?;
    settings.boolean("attack", "/breach_allowed", attack.breach)?;
    settings.number("attack", "/waves", attack.waves_reading())?;
    Ok(())
}

/// A Mine beacon's settings rows.
///
/// # Errors
///
/// As [`beacon_detail`].
fn mine_rows(
    beacons: &pharmakos_sim::tables::BeaconTable,
    row: usize,
    settings: &mut Settings,
) -> Result<(), Error> {
    let missing = |what: &str| {
        Error::internal(said!(
            "the beacon detail has no {} for its beacon",
            pharmakos_proto::json::grammar::quoted(what)
        ))
    };
    let mine = beacons
        .mine_settings(row)
        .ok_or_else(|| missing("Mine settings"))?;
    settings.number("mine", "/dig_max_depth", mine.dig_max_depth)?;
    settings.boolean("mine", "/flee_on_threat", mine.flee_on_threat)?;
    let seam = match mine.seam_choice {
        pharmakos_sim::mining::SeamChoice::Richest => "RICHEST",
        pharmakos_sim::mining::SeamChoice::Nearest => "NEAREST",
        pharmakos_sim::mining::SeamChoice::Safest => "SAFEST",
    };
    settings.word(
        "mine",
        "/seam_choice",
        "gp.v1.MineSettings.SeamChoice",
        seam,
    )?;
    settings.number("mine", "/pillar_spacing", mine.pillar_spacing)?;
    Ok(())
}

/// The rows of [`beacon_detail`]'s `settings`, each one's title and shape
/// the vocabulary's for its field.
struct Settings {
    vocabulary: pharmakos_plan_core::vocabulary::Vocabulary,
    rows: Vec<Json>,
}

impl Settings {
    fn new() -> Result<Settings, Error> {
        Ok(Settings {
            vocabulary: pharmakos_plan_core::vocabulary::vocabulary(&[]).map_err(|error| {
                Error::internal(said!(
                    "the vocabulary: {}",
                    pharmakos_proto::json::grammar::Said::engine_error(&error)
                ))
            })?,
            rows: Vec::new(),
        })
    }

    /// One row: `arm` is the writ's field (`build`), empty for a common
    /// setting, and `field` the setting's pointer inside it.
    fn push(&mut self, arm: &str, field: &str, value_json: String) -> Result<(), Error> {
        let parameter = if arm.is_empty() {
            pharmakos_plan_core::vocabulary::common_parameters()
                .map_err(|error| {
                    Error::internal(said!(
                        "the vocabulary: {}",
                        pharmakos_proto::json::grammar::Said::engine_error(&error)
                    ))
                })?
                .into_iter()
                .find(|parameter| parameter.pointer == field)
        } else {
            self.vocabulary
                .mandates
                .iter()
                .find(|entry| entry.name == arm)
                .and_then(|entry| {
                    entry
                        .parameters
                        .iter()
                        .find(|parameter| parameter.pointer == field)
                        .cloned()
                })
        }
        .ok_or_else(|| {
            Error::internal(said!(
                "the vocabulary has no setting `{}` for the {} writ",
                pharmakos_proto::json::grammar::quoted(field),
                pharmakos_proto::json::grammar::quoted(arm)
            ))
        })?;
        let pointer = if arm.is_empty() {
            String::from(field)
        } else {
            format!("/{arm}{field}")
        };
        // An integer's value as it is shown and typed, in its display unit,
        // as a chip's `value_text` is (S3's `fixq`), and its words when it has
        // words of its own, as a chip's `value_title` is (S3's `zerotitle`);
        // both empty for any other.
        let (value_text, value_title) = match &parameter.shape {
            pharmakos_plan_core::shape::Shape::Integer { unit, .. } => {
                let value: i64 = value_json.parse().map_err(|_| {
                    Error::internal(said!(
                        "the setting `{}` holds a number this gateway cannot read",
                        pharmakos_proto::json::grammar::quoted(&pointer)
                    ))
                })?;
                (
                    pharmakos_plan_core::shape::shown_text(unit, value),
                    parameter.shape.value_title(value),
                )
            }
            pharmakos_plan_core::shape::Shape::Choice { .. }
            | pharmakos_plan_core::shape::Shape::Reference { .. } => (String::new(), None),
        };
        let mut row = vec![
            (String::from("pointer"), Json::String(pointer)),
            (
                String::from("title"),
                Json::String(parameter.title.to_owned()),
            ),
            (String::from("value_json"), Json::String(value_json)),
            (String::from("shape"), view::value_shape(&parameter.shape)),
            (String::from("value_text"), Json::String(value_text)),
        ];
        // Written only where the value has words of its own; proto3's empty
        // string is the absence (S3's `zerotitle`).
        if let Some(title) = value_title {
            row.push((String::from("value_title"), Json::String(title.to_owned())));
        }
        self.rows.push(Json::Object(row));
        Ok(())
    }

    fn number(&mut self, arm: &str, field: &str, value: impl ToString + Copy) -> Result<(), Error> {
        self.push(arm, field, value.to_string())
    }

    fn boolean(&mut self, arm: &str, field: &str, value: bool) -> Result<(), Error> {
        self.push(arm, field, value.to_string())
    }

    /// An enum setting by its schema value name, checked against the schema.
    fn word(&mut self, arm: &str, field: &str, full_name: &str, name: &str) -> Result<(), Error> {
        pharmakos_proto::descriptor::schema()
            .enumeration(full_name)
            .and_then(|declared| declared.value_by_name(name))
            .ok_or_else(|| {
                Error::internal(said!(
                    "the schema's `{}` names no `{}`",
                    pharmakos_proto::json::grammar::quoted(full_name),
                    pharmakos_proto::json::grammar::quoted(name)
                ))
            })?;
        self.push(arm, field, format!("\"{name}\""))
    }

    /// An enum setting by its wire number, which the schema must name.
    fn name(&mut self, arm: &str, field: &str, full_name: &str, number: i32) -> Result<(), Error> {
        let name = pharmakos_proto::descriptor::schema()
            .enumeration(full_name)
            .and_then(|declared| declared.value_by_number(number))
            .filter(|declared| declared.number != 0)
            .map(|declared| declared.name.clone())
            .ok_or_else(|| {
                Error::internal(said!(
                    "the schema's `{}` names no value {}",
                    pharmakos_proto::json::grammar::quoted(full_name),
                    number
                ))
            })?;
        self.push(arm, field, format!("\"{name}\""))
    }
}

/// A Build beacon's targets, in the Quartermaster's order (ascending `order`,
/// then the table's), each a `gp.v1.BuildTarget` with its anchor as it was
/// written.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] when a column lacks the row, a blueprint
/// the sim does not define, or a description the sim does not write.
fn build_targets(
    world: &pharmakos_sim::world::World,
    beacon: pharmakos_sim::tables::BeaconId,
) -> Result<Vec<Json>, Error> {
    let table = world.targets();
    let missing = |name: &str, row: usize| {
        Error::internal(said!(
            "the target table has no {} in row {}",
            pharmakos_proto::json::grammar::quoted(name),
            row
        ))
    };
    let features = crate::targeting::feature_reads(world, None);
    let mut found: Vec<(u32, usize, Json)> = Vec::new();
    for row in 0..table.row_count() {
        let owner = table
            .beacons()
            .get(row)
            .copied()
            .ok_or_else(|| missing("beacon", row))?;
        let kind = table
            .kinds()
            .get(row)
            .copied()
            .ok_or_else(|| missing("kind", row))?;
        if owner != beacon || kind != pharmakos_sim::tables::TargetKind::Build.id() {
            continue;
        }
        let blueprint_id = table
            .blueprints()
            .get(row)
            .copied()
            .ok_or_else(|| missing("blueprint", row))?;
        let blueprint = pharmakos_sim::tables::StructureKind::ALL
            .into_iter()
            .find(|kind| kind.id() == blueprint_id)
            .ok_or_else(|| {
                Error::internal(said!(
                    "the target table names blueprint {}, which the sim does not define",
                    blueprint_id
                ))
            })?;
        let order = table
            .orders()
            .get(row)
            .copied()
            .ok_or_else(|| missing("order", row))?;
        let rotation = table
            .rotations()
            .get(row)
            .copied()
            .ok_or_else(|| missing("rotation", row))?;
        let description = table
            .descriptions()
            .get(row)
            .copied()
            .ok_or_else(|| missing("description", row))?;
        let anchor = target_anchor(table, row, description, &features)?;
        let mut entries = vec![
            (
                String::from("blueprint_id"),
                Json::String(view::structure_subtype(blueprint).to_owned()),
            ),
            (String::from("anchor"), anchor),
        ];
        if rotation != 0 {
            entries.push((
                String::from("rotation_quarter_turns"),
                Json::Number(rotation.to_string()),
            ));
        }
        if order != 0 {
            entries.push((String::from("order"), Json::Number(order.to_string())));
        }
        if blueprint == pharmakos_sim::tables::StructureKind::Wall {
            entries.push((String::from("wall_box"), wall_box(table, row)?));
        }
        found.push((order, row, Json::Object(entries)));
    }
    found.sort_by_key(|(order, row, _)| (*order, *row));
    Ok(found.into_iter().map(|(_, _, target)| target).collect())
}

/// The lowest voxel of the map: every column's ground stands at or above it.
const MAP_FLOOR_Z: i32 = 0;

/// A wall target's `wall_box` (`gp.v1.Area`), from the target table's
/// columns: the box of columns ([`pharmakos_sim::tables::TargetTable::areas`])
/// and its top ([`pharmakos_sim::tables::TargetTable::wall_tops`]), `max.z`.
///
/// `min.z` "says only that the box reaches the ground: it lies at or below
/// the ground's top in every column" (`playbook.proto`'s `BuildTarget`): the
/// sim checks it when the row commits and keeps nothing of it, since a wall
/// fills from each column's ground up to its top whatever `min.z` was. So
/// the box's floor is answered as the map's floor, [`MAP_FLOOR_Z`], the one
/// value at or below every column's ground: the box the world holds, which
/// commits the same wall, not a value the seat wrote.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] when the row holds no box or no top, or
/// holds the no-wall top on a wall's row.
fn wall_box(table: &pharmakos_sim::tables::TargetTable, row: usize) -> Result<Json, Error> {
    let missing = |name: &str| {
        Error::internal(said!(
            "the target table has no wall {} in row {}",
            pharmakos_proto::json::grammar::quoted(name),
            row
        ))
    };
    let columns = table
        .areas()
        .get(row)
        .copied()
        .ok_or_else(|| missing("box"))?;
    let top = table
        .wall_tops()
        .get(row)
        .copied()
        .ok_or_else(|| missing("top"))?;
    if top == pharmakos_sim::tables::NO_WALL {
        return Err(missing("top (it holds none)"));
    }
    let corner = |[x, y]: [i16; 2], z: i32| {
        Json::Object(vec![
            (String::from("x"), Json::Number(x.to_string())),
            (String::from("y"), Json::Number(y.to_string())),
            (String::from("z"), Json::Number(z.to_string())),
        ])
    };
    Ok(Json::Object(vec![
        (
            String::from("min"),
            corner(columns.min_column(), MAP_FLOOR_Z),
        ),
        (
            String::from("max"),
            corner(columns.max_column(), i32::from(top)),
        ),
    ]))
}

/// A Build target's anchor as it was written: a voxel, `on` a named
/// feature, or `on` the nearest vent, by the target table's description column.
///
/// # Errors
///
/// As [`build_targets`].
fn target_anchor(
    table: &pharmakos_sim::tables::TargetTable,
    row: usize,
    description: u8,
    features: &[crate::targeting::FeatureRead],
) -> Result<Json, Error> {
    let missing = |name: &str, row: usize| {
        Error::internal(said!(
            "the target table has no {} in row {}",
            pharmakos_proto::json::grammar::quoted(name),
            row
        ))
    };
    Ok(match description {
        pharmakos_sim::targeting::DESCRIPTION_VOXEL => {
            let at = view::voxel_of(
                table
                    .anchors()
                    .get(row)
                    .copied()
                    .ok_or_else(|| missing("anchor", row))?,
            );
            Json::Object(vec![(
                String::from("voxel"),
                Json::Object(vec![
                    (String::from("x"), Json::Number(at.x.to_string())),
                    (String::from("y"), Json::Number(at.y.to_string())),
                    (String::from("z"), Json::Number(at.z.to_string())),
                ]),
            )])
        }
        pharmakos_sim::targeting::DESCRIPTION_NAME => {
            let index = table
                .features()
                .get(row)
                .copied()
                .ok_or_else(|| missing("feature", row))?;
            let name = usize::try_from(index)
                .ok()
                .and_then(|index| features.iter().find(|read| read.index == index))
                .map(|read| read.name.clone())
                .ok_or_else(|| {
                    Error::internal(said!(
                        "a target written by name is bound to feature {}, which the map \
                         does not hold",
                        index
                    ))
                })?;
            Json::Object(vec![(
                String::from("on"),
                Json::Object(vec![(String::from("feature_id"), Json::String(name))]),
            )])
        }
        pharmakos_sim::targeting::DESCRIPTION_NEAREST_VENT => Json::Object(vec![(
            String::from("on"),
            Json::Object(vec![(
                String::from("vent"),
                Json::Object(vec![(
                    String::from("rank"),
                    // Lower case, as every enum this surface answers with is
                    // (decisions-log item 80).
                    Json::String(pharmakos_proto::scope::wire_name(
                        "gp.v1.FeatureRef.Rank",
                        pharmakos_proto::gp::v1::feature_ref::Rank::Nearest.as_str_name(),
                    )),
                )]),
            )]),
        )]),
        other => {
            return Err(Error::internal(said!(
                "the target table holds description {}, which the sim does not write",
                other
            )));
        }
    })
}

/// One `gp.api.v1.Leg` of [`Surface::estimate_route`]'s answer.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] for a travel time the rendering refuses.
fn leg_json(leg: &pharmakos_plan_core::travel::Leg) -> Result<Json, Error> {
    let refused = |error: pharmakos_plan_core::travel::TravelError| {
        Error::internal(said!(
            "the route's travel has no rendering: {}",
            pharmakos_proto::json::grammar::Said::engine_error(&error)
        ))
    };
    let mut entries = vec![
        // `gp.api.v1.Leg.to` is a `gp.v1.Location`, so the voxel
        // goes under its oneof arm, `{"voxel": {x, y, z}}`, and
        // not bare (decisions-log item 112 (3); pinned by
        // `a_legs_end_is_written_as_the_location_the_schema_declares`,
        // because the walkthrough golden records summaries).
        (
            String::from("to"),
            Json::Object(vec![(
                String::from("voxel"),
                Json::Object(vec![
                    (String::from("x"), Json::Number(leg.to.x.to_string())),
                    (String::from("y"), Json::Number(leg.to.y.to_string())),
                    (String::from("z"), Json::Number(leg.to.z.to_string())),
                ]),
            )]),
        ),
    ];
    // A leg with no estimate writes no `ms` and no `travel_text`,
    // and the route says it is unreachable: a 0 would read as a
    // leg that costs nothing.
    let reached = match leg.estimate {
        Some(found) => Some((found.ms().map_err(refused)?, leg.render().map_err(refused)?)),
        None => None,
    };
    if let Some((ms, _)) = &reached {
        entries.push((String::from("ms"), Json::Number(ms.raw().to_string())));
    }
    entries.push((String::from("fogged"), Json::Bool(leg.fogged)));
    if let Some((_, text)) = reached {
        entries.push((String::from("travel_text"), Json::String(text)));
    }
    Ok(Json::Object(entries))
}

/// One row of the sim's licence table, read as the wire carries it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct LicenceHeld {
    /// The seat holding it.
    seat: SeatId,
    /// Which licence.
    licence: pharmakos_proto::gp::v1::Licence,
    /// The round whose settlement awarded it, from 1.
    round: u32,
}

/// Every row of the sim's licence table, in its order: ascending by seat, then
/// licence.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] for a row naming a licence this build does
/// not define, which the sim's table refuses to hold.
fn licence_rows(world: &pharmakos_sim::world::World) -> Result<Vec<LicenceHeld>, Error> {
    world
        .licences()
        .rows()
        .iter()
        .map(|row| {
            let licence = pharmakos_sim::licence::licence_of(row.licence).ok_or_else(|| {
                Error::internal(said!(
                    "the licence table holds licence {} for seat {}, which this build does not \
                     define",
                    row.licence,
                    row.seat
                ))
            })?;
            Ok(LicenceHeld {
                seat: SeatId::new(row.seat),
                licence,
                round: row.round,
            })
        })
        .collect()
}

/// A licence as the JSON-RPC wire spells `gp.v1.Licence`: `"resonance_spire"`
/// (decisions-log item 80).
fn licence_wire(licence: pharmakos_proto::gp::v1::Licence) -> String {
    pharmakos_proto::scope::wire_name("gp.v1.Licence", licence.as_str_name())
}

/// The recap's licence line for `seat` ([`Surface::get_recap`]), under the
/// tally's fog rule (the owner's ruling, decisions-log item 145 (7); S3's plan,
/// item 156): at a recap between rounds, what `seat` was awarded at this
/// settlement and no other seat's; once the match has ended (`ended`), every
/// seat's award of the whole match that still stands, an eliminated seat's
/// voided ones gone with it. Each row names its seat, as `seat.0`. `None` when
/// there is nothing to announce. The prose names `seat`'s own awards at this
/// settlement, "Licence awarded: Resonance Spire.", whichever rows the wire
/// carries, and, once the match has ended, every award that stands with its
/// seat and round ([`strings::licences_standing`]; S3's `probw`), so a client
/// draws no licence rows of its own.
///
/// # Errors
///
/// As [`licence_rows`], and [`crate::error::Code::Internal`] for a licence the
/// string table has no name for.
fn licence_line(
    world: &pharmakos_sim::world::World,
    facts: &RecapFacts,
    seat: SeatId,
    ended: bool,
    prose: &mut String,
) -> Result<Option<Json>, Error> {
    let rows = licence_rows(world)?;
    let this_settlement = |row: &LicenceHeld| row.seat == seat && row.round == facts.round;
    let title_of = |row: &LicenceHeld| {
        strings::licence_title(row.licence).ok_or_else(|| {
            Error::internal(said!(
                "the string table names no licence {}",
                row.licence.as_str_name()
            ))
        })
    };
    let titles = rows
        .iter()
        .filter(|row| this_settlement(row))
        .map(title_of)
        .collect::<Result<Vec<&str>, Error>>()?;
    if let Some(line) = strings::licences_awarded(&titles) {
        prose.push(' ');
        prose.push_str(&line);
    }
    // Once the match has ended, every award that still stands, in words too,
    // after the seat's own line (S3's `probw`, item 6): the rows the wire
    // carries then, each with its seat and round.
    if ended {
        let standing = rows
            .iter()
            .map(|row| Ok((row.seat.raw(), title_of(row)?, row.round)))
            .collect::<Result<Vec<(u8, &str, u32)>, Error>>()?;
        if let Some(line) = strings::licences_standing(&standing) {
            prose.push(' ');
            prose.push_str(&line);
        }
    }
    let told: Vec<Json> = rows
        .iter()
        .filter(|row| ended || this_settlement(row))
        .map(|row| {
            Json::Object(vec![
                (
                    String::from("seat"),
                    Json::String(crate::token::Subject::Seat(row.seat).render()),
                ),
                (
                    String::from("licence"),
                    Json::String(licence_wire(row.licence)),
                ),
                (
                    String::from("round_awarded"),
                    Json::Number(row.round.to_string()),
                ),
            ])
        })
        .collect();
    Ok((!told.is_empty()).then_some(Json::Array(told)))
}

/// Every seat's tally, ascending by seat, for [`Surface::get_recap`]: `None`
/// until the match is decided (the owner's ruling, decisions-log item 145
/// (7)), and from the final recap on, since the outcome is decided on the
/// final tick and the fog lifts there.
///
/// # Errors
///
/// As [`tally_json`].
fn seat_tallies(host: &crate::host::Host, facts: &RecapFacts) -> Result<Option<Json>, Error> {
    if host.runner().outcome().is_none() {
        return Ok(None);
    }
    let all = facts
        .tallies
        .iter()
        .map(|(seat, tally)| {
            Ok(Json::Object(vec![
                (
                    String::from("seat"),
                    Json::String(crate::token::Subject::Seat(*seat).render()),
                ),
                (String::from("tally"), tally_json(*tally)?.0),
            ]))
        })
        .collect::<Result<Vec<Json>, Error>>()?;
    Ok(Some(Json::Array(all)))
}

/// One `gp.api.v1.Tally`, every field written, the zeros included, and the
/// value destroyed as the wire's whole dollars.
///
/// # Errors
///
/// As [`view::dollars_wire`], for a value destroyed past `int32`.
fn tally_json(tally: pharmakos_sim::tables::Tally) -> Result<(Json, i32), Error> {
    let destroyed = view::dollars_wire(tally.destroyed, "the enemy value a seat destroyed")?;
    let json = Json::Object(vec![
        (String::from("kills"), Json::Number(tally.kills.to_string())),
        (
            String::from("shared_kills"),
            Json::Number(tally.shared_kills.to_string()),
        ),
        (
            String::from("losses"),
            Json::Number(tally.losses.to_string()),
        ),
        (
            String::from("value_destroyed_dollars"),
            Json::Number(destroyed.to_string()),
        ),
    ]);
    Ok((json, destroyed))
}

/// Why a seal does not price ([`Surface::seal_commitment_of`]).
pub(super) enum Unpriced {
    /// The playbook orders more than the projection or the wire can carry:
    /// something a seat sent, which the door refuses as `INVALID_ARGUMENT`.
    Playbook(Said),
    /// The gateway disagreeing with itself or with the sim: the error as it
    /// is, `INTERNAL` or `NOT_FOUND`.
    Gateway(Error),
}

/// The BMI at the band `seat` holds now: the sim's
/// [`pharmakos_sim::economy::bmi_for`] at the sim's
/// [`pharmakos_sim::economy::ladder_place`] over each living seat's held value
/// as the world stands ([`crate::surface::held_values`]), the ladder the Ledger
/// reads when it settles. A prediction: the ladder can move before the next
/// settlement ([`Surface::get_economy_forecast`] says how). `None` for a seat
/// out of the match, which is off the ladder and is paid nothing.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] when the seat table does not hold the
/// seat, or the sim's ladder read refuses a seat in the match.
fn bmi_next(world: &pharmakos_sim::world::World, seat: SeatId) -> Result<Option<Money>, Error> {
    let held = crate::surface::held_values(world);
    let index = held
        .iter()
        .position(|(held, _)| *held == seat)
        .ok_or_else(|| Error::internal(said!("the seat table has no seat {}", seat.raw())))?;
    if held.get(index).is_none_or(|(_, value)| value.is_none()) {
        return Ok(None);
    }
    let ladder: Vec<Option<Money>> = held.into_iter().map(|(_, value)| value).collect();
    let place = crate::surface::ladder_place(&ladder, index)?;
    pharmakos_sim::economy::bmi_for(world.rules(), place.rank, place.living)
        .map(Some)
        .map_err(|refused| {
            Error::internal(said!(
                "the sim's BMI read refused seat {}: {}",
                seat.raw(),
                crate::error::engine(&refused)
            ))
        })
}

/// A whole-`$` or whole-kW figure as the wire's `int32`.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] for a figure past `int32`, which no match
/// reaches: refused rather than saturated ([`Surface::get_economy_forecast`]).
fn wire_number(value: i64, what: &str) -> Result<Json, Error> {
    i32::try_from(value)
        .map(|value| Json::Number(value.to_string()))
        .map_err(|_| {
            Error::internal(said!(
                "this seat's {} does not fit the wire's int32",
                pharmakos_proto::json::grammar::quoted(what)
            ))
        })
}

/// How many rows of a seat column belong to one seat.
///
/// A fold rather than `filter().count()`: the columns are `[u8]`, so clippy
/// reads the obvious spelling as a byte count and asks for a `bytecount` crate
/// that is not on the approved list (AGENTS.md section 3 rule 5). This counts
/// seats, saturates rather than wrapping, and comes back in the `u32` the wire
/// wants.
fn rows_of(column: &[u8], seat: SeatId) -> u32 {
    column.iter().fold(0_u32, |sum, held| {
        if *held == seat.raw() {
            sum.saturating_add(1)
        } else {
            sum
        }
    })
}

/// A seat's place on the displayed standing (spec section 3): "live standings
/// show a seat its own score and displayed rank only -- the full audit score,
/// held value plus enemy value destroyed".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Place {
    /// 1-based, the leader first, among the seats the final audit decides
    /// between; `None` for a seat that is out of the match and so on no
    /// standing. Seats tied on every term share a rank.
    pub(crate) rank: Option<NonZeroU32>,
    /// How many seats the standing ranks.
    pub(crate) of: u32,
    /// The seat's audit score, whole `$`.
    pub(crate) score: i32,
}

/// `seat`'s place on the displayed standing, read off the world as it stands
/// by the sim's one audit function ([`pharmakos_sim::audit::final_audit`],
/// `fixs`'s X-08 audit, the register's X-03).
///
/// The score is the seat's own audit line. The rank is the audit's own
/// ([`pharmakos_sim::audit::FinalAudit::rank_of`]): the audited seats that
/// stand **strictly ahead** of it in spec section 3's tie-break order -- the
/// score, then enemy value destroyed, then fewer beacons lost -- plus one, so
/// seats tied on every term share a rank, which is what the final audit calls
/// a shared win. The sim's read retired this crate's copy of the tie-break
/// (decisions-log item 135 (2) (e)).
///
/// # Errors
///
/// [`crate::error::Code::Internal`] when the score does not fit the wire's
/// `int32`, a treasury past two billion `$` that no match reaches: refused
/// rather than saturated, because a saturated score would be a wrong answer
/// told as a right one; and for a rank past what a `u32` numbers.
pub(crate) fn displayed_place(
    world: &pharmakos_sim::world::World,
    seat: SeatId,
) -> Result<Place, Error> {
    let audit = pharmakos_sim::audit::final_audit(world);
    let line = pharmakos_sim::audit::line_of(world, seat);
    let score = i32::try_from(line.score.raw())
        .map_err(|_| Error::internal("a seat's audit score does not fit the wire's int32"))?;
    let of = u32::try_from(audit.lines.len())
        .map_err(|_| Error::internal("more audited seats than a seat id can number"))?;
    let rank = audit.rank_of(seat).map_err(|refused| {
        Error::internal(said!(
            "the audit's rank refused: {}",
            crate::error::engine(&refused)
        ))
    })?;
    Ok(Place { rank, of, score })
}

/// `gp.api.v1.Standing`'s fields for one seat, its `rank` left out when the
/// seat holds no place.
fn standing_entries(place: Place, living: u32) -> Vec<(String, Json)> {
    let mut entries = Vec::with_capacity(3);
    if let Some(rank) = place.rank {
        entries.push((String::from("rank"), Json::Number(rank.get().to_string())));
    }
    entries.push((String::from("score"), Json::Number(place.score.to_string())));
    entries.push((
        String::from("living_seats"),
        Json::Number(living.to_string()),
    ));
    entries
}

/// A seat's core: **its `b_00`**, the beacon the map generator pre-places
/// for it.
///
/// Since S1's targeting a beacon's name is its per-seat ordinal, minted in the
/// order the seat's beacons appear, and the generator places the core before
/// any seat can place anything, so the core is ordinal 0 on every map and a
/// template can name it as `b_00` anywhere (decisions-log item 127 (13);
/// `docs/design/targeting.md`, "Names"). The register's S1-13 asked for that
/// rule, or a column, the day a beacon could be told apart from the core; the
/// per-seat ordinal is the rule. [`Surface::verifier_scope`] marks `is_core`
/// by this same function, so the wire and the verifier cannot disagree about
/// which beacon is the core. A core that has fallen keeps its ordinal and
/// stays the core: nothing renumbers a seat's beacons.
pub(crate) fn core_beacon_of(world: &pharmakos_sim::world::World, seat: SeatId) -> Option<u32> {
    let beacons = world.beacons();
    (0..beacons.ids().len())
        .find(|row| {
            beacons.seats().get(*row).copied() == Some(seat.raw())
                && beacons.ordinals().get(*row).copied() == Some(0)
        })
        .and_then(|row| beacons.ids().get(row).copied())
}

/// A feature kind as the JSON-RPC wire spells `gp.api.v1.MapFeature.Kind`:
/// the value name, lower case (decisions-log item 80).
fn feature_kind_wire(kind: pharmakos_sim::features::FeatureKind) -> String {
    use pharmakos_proto::gp::api::v1::map_feature::Kind;
    let named = match kind {
        pharmakos_sim::features::FeatureKind::Vent => Kind::Vent,
        pharmakos_sim::features::FeatureKind::Seam => Kind::Seam,
    };
    pharmakos_proto::scope::wire_name("gp.api.v1.MapFeature.Kind", named.as_str_name())
}

/// A grade as the JSON-RPC wire spells `gp.v1.ByRichness.Richness`: `"rich"`.
fn grade_wire(grade: pharmakos_sim::voxels::Richness) -> String {
    pharmakos_proto::scope::wire_name(
        "gp.v1.ByRichness.Richness",
        grade_proto(grade).as_str_name(),
    )
}

/// The sim's grade as the schema's enum, which the wire and the verifier's
/// scope both carry.
pub(crate) const fn grade_proto(
    grade: pharmakos_sim::voxels::Richness,
) -> pharmakos_proto::gp::v1::by_richness::Richness {
    use pharmakos_proto::gp::v1::by_richness::Richness;
    match grade {
        pharmakos_sim::voxels::Richness::Lean => Richness::Lean,
        pharmakos_sim::voxels::Richness::Standard => Richness::Standard,
        pharmakos_sim::voxels::Richness::Rich => Richness::Rich,
    }
}

/// A Quartermaster priority column value as the JSON-RPC wire spells it:
/// `gp.v1.InterfaceRow.QuartermasterPriority`'s value name, lower case, as
/// every enum this surface answers with is (decisions-log item 80).
///
/// The column holds the enum's wire values (`SeatTable`'s doc), so a value
/// the enum does not name is the sim and the schema disagreeing, and it is
/// refused as a typed internal error rather than spelt `unspecified`, which a
/// client would read as an answer.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] for a value the schema does not name.
fn priority_wire_name(value: u8) -> Result<String, Error> {
    use pharmakos_proto::gp::v1::interface_row::QuartermasterPriority;
    let named = QuartermasterPriority::try_from(i32::from(value)).map_err(|_| {
        Error::internal(said!(
            "a beacon's Quartermaster priority column holds {}, which the schema does not \
             name",
            value
        ))
    })?;
    Ok(pharmakos_proto::scope::wire_name(
        "gp.v1.InterfaceRow.QuartermasterPriority",
        named.as_str_name(),
    ))
}

/// A whole voxel as the fixed-point point at its corner, which is the voxel
/// a selector's ranking floors it back to.
fn fx_point(voxel: Voxel, at: &str) -> Result<[pharmakos_sim::math::fixed::Fx; 3], Error> {
    let axis = |value: i32| {
        i16::try_from(value)
            .map(pharmakos_sim::math::fixed::Fx::from_voxels)
            .map_err(|_| {
                Error::invalid(said!(
                    "{}: the waypoint before it is off the map",
                    pharmakos_proto::json::grammar::quoted_pointer(at)
                ))
            })
    };
    Ok([axis(voxel.x)?, axis(voxel.y)?, axis(voxel.z)?])
}

/// One `gp.v1.Voxel` out of a request.
fn read_voxel(value: &Json, at: &str) -> Result<Voxel, Error> {
    let axis = |name: &str| -> Result<i32, Error> {
        match value.get(name) {
            None => Ok(0),
            Some(Json::Number(lexeme)) => lexeme.parse::<i32>().map_err(|_| {
                Error::invalid(said!(
                    "{}: `{}` is a whole number of voxels",
                    pharmakos_proto::json::grammar::quoted_pointer(at),
                    pharmakos_proto::json::grammar::quoted(name)
                ))
            }),
            Some(other) => Err(Error::invalid(said!(
                "{}: `{}` is a whole number of voxels, and this is {}",
                pharmakos_proto::json::grammar::quoted_pointer(at),
                pharmakos_proto::json::grammar::quoted(name),
                other.kind()
            ))),
        }
    };
    Ok(Voxel {
        x: axis("x")?,
        y: axis("y")?,
        z: axis("z")?,
    })
}

#[cfg(test)]
mod tests {
    use super::displayed_place;
    use core::num::NonZeroU32;
    use pharmakos_sim::audit::final_audit;
    use pharmakos_sim::math::quantity::Money;
    use pharmakos_sim::rules::RulesTable;
    use pharmakos_sim::runner::MatchSettings;
    use pharmakos_sim::tables::SeatId;
    use pharmakos_sim::world::{World, WorldConfig};
    use std::path::Path;

    /// The displayed standing over a world whose lines differ: three seats,
    /// each with a different treasury, ranked in the order the score says by
    /// the audit's own `rank_of`, and the seat ranked first is the audit's
    /// only winner. The tie-break term by term is the sim's test
    /// (`crates/sim/tests/audit.rs`).
    #[test]
    fn a_three_seat_world_ranks_as_its_final_audit_does() {
        let rules = RulesTable::load(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("rules")
                .join("rules.v1.json"),
        )
        .expect("the shipped rules table");
        let mut world = World::new(&WorldConfig {
            match_seed: 0x0000_0000_ca5c_aded,
            seats: 3,
            rules,
            match_settings: MatchSettings {
                segment_lengths_ms: vec![60_000],
                round_limit: 3,
            },
        })
        .expect("a world");
        world.set_treasury(SeatId::new(0), Money::new(1_000));
        world.set_treasury(SeatId::new(1), Money::new(3_000));
        world.set_treasury(SeatId::new(2), Money::new(2_000));
        let audit = final_audit(&world);
        let place = |raw: u8| displayed_place(&world, SeatId::new(raw)).expect("a place");
        let shown: Vec<Option<u32>> = (0..3)
            .map(|raw| place(raw).rank.map(NonZeroU32::get))
            .collect();
        assert_eq!(shown, vec![Some(3), Some(1), Some(2)]);
        assert_eq!(audit.winners, vec![SeatId::new(1)]);
        for raw in 0..3_u8 {
            let held = place(raw);
            assert_eq!(held.of, 3);
            let own = audit
                .lines
                .iter()
                .find(|line| line.seat == SeatId::new(raw))
                .expect("every seat is audited");
            assert_eq!(i64::from(held.score), own.score.raw());
            let ahead = audit
                .lines
                .iter()
                .filter(|other| other.score > own.score)
                .count();
            assert_eq!(
                held.rank.map(NonZeroU32::get),
                u32::try_from(ahead).ok().and_then(|n| n.checked_add(1)),
                "seat {raw}: with every score distinct, the rank is the score's order"
            );
        }
    }

    /// The recap's licence line under the tally's fog rule (S3's plan, item
    /// 156; decisions-log item 145 (7)): between rounds a seat is told its own
    /// award at this settlement and nobody else's, and an award of an earlier
    /// round is not announced again; once the match has ended every seat's
    /// standing award is told, ascending by seat. The prose names the seat's
    /// own award at this settlement only.
    #[test]
    fn the_recaps_licence_line_keeps_the_fog_rule() {
        use pharmakos_proto::gp::v1::Licence;
        use pharmakos_proto::json::Json;
        let rules = RulesTable::load(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("rules")
                .join("rules.v1.json"),
        )
        .expect("the shipped rules table");
        let mut world = World::new(&WorldConfig {
            match_seed: 0x0000_0000_ca5c_aded,
            seats: 2,
            rules,
            match_settings: MatchSettings {
                segment_lengths_ms: vec![60_000],
                round_limit: 3,
            },
        })
        .expect("a world");
        assert!(world.award_licence_directly(SeatId::new(0), Licence::ResonanceSpire, 2));
        assert!(world.award_licence_directly(SeatId::new(1), Licence::ResonanceSpire, 1));
        let facts = super::RecapFacts {
            round: 2,
            ticks: 20,
            settled: Vec::new(),
            dark: Vec::new(),
            held: Vec::new(),
            failures: Vec::new(),
            dropped: 0,
            tallies: Vec::new(),
        };
        let row = |seat: &str, round: u32| {
            Json::Object(vec![
                (String::from("seat"), Json::String(String::from(seat))),
                (
                    String::from("licence"),
                    Json::String(String::from("resonance_spire")),
                ),
                (
                    String::from("round_awarded"),
                    Json::Number(round.to_string()),
                ),
            ])
        };
        let line = |seat: u8, ended: bool| {
            let mut prose = String::new();
            let told = super::licence_line(&world, &facts, SeatId::new(seat), ended, &mut prose)
                .expect("a line");
            (told, prose)
        };

        assert_eq!(
            line(0, false),
            (
                Some(Json::Array(vec![row("seat.0", 2)])),
                String::from(" Licence awarded: Resonance Spire.")
            ),
            "seat 0 is told its own award at this settlement"
        );
        assert_eq!(
            line(1, false),
            (None, String::new()),
            "seat 1's award was an earlier round's, and seat 0's is not its to know"
        );
        assert_eq!(
            line(1, true),
            (
                Some(Json::Array(vec![row("seat.0", 2), row("seat.1", 1)])),
                String::from(
                    " The licences that stand at the match's end: seat 0's Resonance Spire, \
                     from round 2; seat 1's Resonance Spire, from round 1."
                )
            ),
            "at the match's end the fog lifts on every standing award, in words too (probw, \
             item 6)"
        );
        assert_eq!(
            line(0, true).1,
            " Licence awarded: Resonance Spire. The licences that stand at the match's end: \
             seat 0's Resonance Spire, from round 2; seat 1's Resonance Spire, from round 1.",
            "the seat's own line first, then every award that stands"
        );
    }
}
