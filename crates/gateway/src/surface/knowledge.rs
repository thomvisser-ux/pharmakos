// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The knowledge reads: what a seat is allowed to know about the match it is
//! in.
//!
//! Spec section 12's Knowledge group, minus the four methods `gateway.proto`
//! gives no request/response pair. Every one of them reads the **frozen
//! planning snapshot or the live world through the fog filter**, and not one of
//! them steps anything: `estimate_route` estimates its legs through
//! [`crate::routes::RouteAdapter`], which owns its own search graph and has no
//! `World` in its hand at all; a `covering` waypoint and `get_map_summary`'s
//! features read the hosted world through the sim's own resolver, borrowed
//! read-only ([`crate::targeting`]), and rank by estimate rather than by
//! running anything; and `get_economy_forecast` reads the calling seat's own
//! row of the hosted world -- the frozen planning world in a Lull or a recap,
//! the live one in a Push -- and projects nothing (AGENTS.md section 3
//! rule 2).
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

use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::json::Json;
use pharmakos_sim::math::quantity::Ms;
use pharmakos_sim::runner::MatchEndReason;
use pharmakos_sim::tables::SeatId;

use crate::detail::{self, Detail};
use crate::error::Error;
use crate::fog::{Audience, FogFilter, Viewer, Vision};
use crate::rpc::Request;
use crate::strings;
use crate::surface::Surface;
use crate::view;

/// How many beacons one page of `list_beacons` carries at each rung.
///
/// PLACEHOLDER: 8 / 24 / 40 are working numbers. 40 is item 63's world total
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
/// A route costs one abstract search per leg and there is no estimate cache
/// (decisions-log item 61), so the count is the caller's multiplier on the
/// gateway's whole single-threaded turn: measured on the shipped 384x384x64
/// map, 2 000 waypoints between two far corners is a 64 KiB request -- well
/// inside [`crate::frame::MAX_MESSAGE_BYTES`] -- that takes twelve seconds,
/// during which nothing else is answered and the match is not stepped. So the
/// count is refused before any search runs, the same shape
/// `get_economy_forecast` already refuses a surplus of what-ifs in.
///
/// PLACEHOLDER: 64 is a working number -- a route with more legs than a
/// playbook has steps at item 94's 128-unit budget is not a route an editor
/// draws. OWNER settles it at hardening with the rate limits, beside
/// [`crate::surface::MAX_WAIT_MS`] and the draft bounds.
pub const MAX_WAYPOINTS: usize = 64;

/// How many what-if answers one `get_economy_forecast` carries at each rung.
///
/// PLACEHOLDER: as [`beacon_budget`], and settled with it.
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
    /// world as the sim will read it. Like the notebook, it is the seat's own.
    pub(super) fn get_briefing(
        &self,
        subject: crate::token::Subject,
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
        .unwrap_or(0);
        let segment_ms = host.runner().frozen().coming_segment_ms();
        let briefing = strings::briefing(
            host.runner().phase(),
            host.runner().round(),
            world.match_state().round_limit(),
            segment_ms,
            beacons,
            units,
        );
        let standing = strings::standing(living);
        // The Lull's "this round" sentence, after the standing: what this
        // round's playbook does wherever its route reads the map.
        let this_round = self.this_round(seat)?;

        Ok(Json::Object(vec![
            (String::from("notes"), Json::String(notes)),
            (
                String::from("standing"),
                // Spec section 3: a seat's OWN score and rank only. Both are
                // the full audit score, which is the economy's, so this build
                // carries the one number it knows and says so in the prose
                // rather than showing a zero a player would read as a rank
                // (`crate::strings::standing`).
                Json::Object(vec![
                    (String::from("rank"), Json::Number(String::from("0"))),
                    (String::from("score"), Json::Number(String::from("0"))),
                    (
                        String::from("living_seats"),
                        Json::Number(living.to_string()),
                    ),
                ]),
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
        ]))
    }

    /// `get_recap`: what the round did.
    ///
    /// No seat-private content at all, so it takes no subject: the recap is a
    /// match-wide statement, and the settlement lines that will be a seat's own
    /// are `reserved 2 to 15` in `gp.api.v1.GetRecapResponse` until the economy
    /// produces them.
    pub(super) fn get_recap(&self, request: &Request) -> Result<Json, Error> {
        let _ = detail::of(request)?;
        let host = self.host()?;
        let state = host.world().match_state();
        let ticks = host.runner().tick().since(state.segment_started());
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
        let prose = strings::recap(
            host.runner().round(),
            ticks,
            outcome
                .as_ref()
                .map(|(reason, winners)| (*reason, winners.as_slice())),
        );
        Ok(Json::Object(vec![(
            String::from("prose"),
            Json::String(prose),
        )]))
    }

    /// `list_beacons`: the beacons this seat may see, its own first.
    ///
    /// `&mut self` because naming another seat's beacon mints this viewer's
    /// `e_NN` for it the first time it is shown
    /// ([`crate::viewfeed::ViewFeed::beacon_name`]); nothing else is written.
    pub(super) fn list_beacons<V: Vision>(
        &mut self,
        subject: crate::token::Subject,
        held: crate::scopes::ScopeSet,
        request: &Request,
        vision: &V,
    ) -> Result<Json, Error> {
        let viewer = self.viewer_of(subject, held);
        let budget = beacon_budget(detail::of(request)?);
        let limit = request
            .integer_param("limit")?
            .and_then(|value| usize::try_from(value).ok())
            .map_or(budget, |asked| asked.min(budget));
        let visible = self.visible_beacons(viewer, vision);

        // The cursor is an index into THIS viewer's own visible listing, for
        // the same reason `crate::feed`'s is: an index over the unfiltered
        // table would be a number a fogged seat could subtract from its page
        // length to learn how many beacons it was not told about.
        let snapshot = self.feed().snapshot();
        let from = match request.string_param("cursor")? {
            Some(text) if !text.is_empty() => usize::try_from(
                crate::feed::Cursor::parse(text, snapshot, crate::feed::Listing::Beacons)?.index(),
            )
            .unwrap_or(usize::MAX),
            _ => 0,
        };
        // `limit` is taken as asked, zero included: the doc above says a client
        // may always ask for less than its budget, and zero is less. A page of
        // none with the cursor where it was is the honest answer to it, and a
        // silent `.max(1)` would be the gateway deciding what the client meant.
        let page: Vec<Json> = visible
            .iter()
            .skip(from)
            .take(limit)
            .map(|(name, row, at)| self.beacon_summary(viewer, name, *row, *at))
            .collect();
        let index = from.min(visible.len()).saturating_add(page.len());
        let next = if index >= visible.len() {
            String::new()
        } else {
            crate::feed::Cursor::at(
                snapshot,
                crate::feed::Listing::Beacons,
                u32::try_from(index).unwrap_or(u32::MAX),
            )
            .render()
        };
        Ok(Json::Object(vec![
            (String::from("beacons"), Json::Array(page)),
            (String::from("next_cursor"), Json::String(next)),
        ]))
    }

    /// `get_beacon`: one beacon, if this seat may see it.
    ///
    /// A `b_NN` names one of the caller's own beacons by its per-seat
    /// ordinal, and an `e_NN` the beacon this caller was shown under that
    /// handle (decisions-log item 127 (13); `docs/design/targeting.md`,
    /// "Names"). `&mut self` for the reason `list_beacons` is.
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
        if pharmakos_sim::tables::parse_beacon_name(wanted).is_none() {
            return Err(Error::not_found(format!("`{wanted}` is not a beacon id")));
        }
        let viewer = self.viewer_of(subject, held);

        let host = self
            .host
            .as_ref()
            .ok_or_else(|| Error::internal("this gateway is not hosting a match"))?;
        let world = host.world();
        let beacons = world.beacons();
        let row = self
            .views
            .beacon_row(viewer, world, wanted)
            // NOT_FOUND, deliberately, and the same refusal a beacon the seat
            // may not see gets below: "a named beacon does not exist, or is not
            // yours" is one answer in the closed set, and two answers would let
            // a seat map the enemy's beacons by asking for every id.
            .ok_or_else(|| Error::not_found(format!("no beacon `{wanted}` here")))?;
        let owner = beacons.seats().get(row).copied().unwrap_or_default();
        let at = beacons
            .positions()
            .get(row)
            .copied()
            .map(view::voxel_of)
            .unwrap_or_default();
        let filter = FogFilter::new(&self.fog, vision);
        let audience = Audience::World {
            owner: Some(SeatId::new(owner)),
            at,
        };
        if !filter.visible(viewer, &audience) {
            return Err(Error::not_found(format!("no beacon `{wanted}` here")));
        }
        let name = self.views.beacon_name(viewer, world, row);

        let hit_points = beacons
            .hit_points()
            .get(row)
            .copied()
            .unwrap_or_default()
            .raw();
        let mandate = view::mandate_from_id(beacons.mandates().get(row).copied().unwrap_or(0));
        let own = viewer == Viewer::Seat(SeatId::new(owner));
        Ok(Json::Object(vec![
            (
                String::from("summary"),
                self.beacon_summary(viewer, &name, row, at),
            ),
            (
                String::from("prose"),
                Json::String(strings::beacon(
                    &name,
                    view::mandate_name(mandate),
                    hit_points,
                    own,
                )),
            ),
        ]))
    }

    /// `get_map_summary`: the extent, the seed, and every vent and seam.
    ///
    /// The extent and the seed are "part of the match, not a secret"
    /// (`gateway.proto`), so neither is fog-filtered. Neither is the feature
    /// list in S1: decisions-log item 130 (3) (a) kept `live` on it and read
    /// `live`, `covered`, `travel_ms` and `reachable` off the world as it
    /// stands, unfogged, as targeting's own ranking is in S1, and the list is
    /// fog-filtered from S3 with the knowledge store (the register's S1-48).
    /// The terrain summary stays `reserved` until then (item 130 (3) (b)).
    ///
    /// Each feature's id, kind, grade and anchor are a pure function of the
    /// match's inputs; `covered`, `travel_ms` and `reachable` are the asking
    /// **seat's** own (its own spheres, its own commander), so a caller that
    /// is no seat -- `admin`, a spectator -- is told the first six fields and
    /// none of the three, which would be about nobody
    /// ([`Surface::map_features`]).
    pub(super) fn get_map_summary(
        &self,
        subject: crate::token::Subject,
        request: &Request,
    ) -> Result<Json, Error> {
        let _ = detail::of(request)?;
        let host = self.host()?;
        let size = host.world().voxels().size();
        let seed = view::seed_text(host.world().match_seed());
        let seats = host.world().seats().len();
        let axis = |index: usize| {
            Json::Number(i64::from(size.get(index).copied().unwrap_or(0)).to_string())
        };
        let features = self.map_features(subject.seat())?;
        Ok(Json::Object(vec![
            (
                String::from("size"),
                Json::Object(vec![
                    (String::from("x"), axis(0)),
                    (String::from("y"), axis(1)),
                    (String::from("z"), axis(2)),
                ]),
            ),
            (
                String::from("prose"),
                Json::String(strings::map_summary(size, &seed, seats)),
            ),
            (String::from("match_seed"), Json::String(seed)),
            (String::from("features"), Json::Array(features)),
        ]))
    }

    /// `get_map_summary.features`: every vent and seam, in feature id order,
    /// each a `gp.api.v1.MapFeature`.
    ///
    /// For a seat, `covered` is whether one of its own living beacons' spheres
    /// holds the feature (the opposite of a pick's `UNCOVERED`), and
    /// `travel_ms` and `reachable` are the estimate "nearest" ranks with, from
    /// where its commander stands, ranked by the sim's own
    /// [`pharmakos_sim::targeting::Ranker`] ([`crate::targeting`]). A commander
    /// that is not standing anywhere (dead, mid-Push) reaches nothing, and the
    /// answer says so. Nothing is stepped.
    fn map_features(&self, seat: Option<SeatId>) -> Result<Vec<Json>, Error> {
        let host = self.host()?;
        let world = host.world();
        let reads = crate::targeting::feature_reads(world, seat);
        let commander = seat.and_then(|seat| crate::targeting::commander_point(world, seat));
        let travel: Vec<(usize, i64)> = match commander {
            Some(commander) => {
                let tally = pharmakos_sim::seams::UnitTally::new();
                let ground = crate::targeting::lend(world, &tally);
                let mut scratch = crate::targeting::scratch_for(world)?;
                crate::targeting::ranked(
                    &ground,
                    &mut scratch,
                    pharmakos_sim::targeting::column_of(commander),
                    |_| true,
                )
            }
            None => Vec::new(),
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
                if seat.is_some() {
                    let reached = match travel.iter().find(|(index, _)| *index == read.index) {
                        Some((_, cost)) => {
                            Some(crate::targeting::travel_ms(world, *cost).ok_or_else(|| {
                                Error::internal(
                                    "an estimate's cost does not convert to game milliseconds",
                                )
                            })?)
                        }
                        None => None,
                    };
                    entries.push((
                        String::from("covered"),
                        Json::Bool(read.covered_by.is_some()),
                    ));
                    entries.push((
                        String::from("travel_ms"),
                        Json::Number(reached.map_or(0, Ms::raw).to_string()),
                    ));
                    entries.push((String::from("reachable"), Json::Bool(reached.is_some())));
                }
                Ok(Json::Object(entries))
            })
            .collect()
    }

    /// `get_economy_forecast`: the seat's own `$` and kW as they stand, with
    /// what-ifs.
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
    /// then. S1's projection, income and what-ifs land in new fields and never
    /// redefine these (`gateway.proto`). A subject that is not a seat has no
    /// economy and is refused, as it is refused a briefing; another seat's
    /// economy is on no wire at all.
    ///
    /// Whole `$` and whole kW, `int32` on the wire like the `Treasury` and
    /// `KwHeadroom` predicates a playbook compares them against. A figure past
    /// `i32` **saturates**: that is a treasury of two billion dollars, which
    /// no match reaches, and a saturated figure still compares the way a
    /// playbook's `Treasury` predicate would read it.
    ///
    /// PLACEHOLDER: this method answers the **live** world during a Push,
    /// although its name says "forecast"; spec section 3 allows a seat its own
    /// score live and does not name `$` or kW. **OWNER**, at **S1**.
    ///
    /// # What-ifs
    ///
    /// `gp.api.v1.WhatIf` still reserves 1 to 15, so a what-if that names
    /// anything is refused, and the count is bounded by the detail budget.
    /// PLACEHOLDER: the what-if vocabulary and its answers are **S1**'s.
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
                return Err(Error::invalid(format!(
                    "this detail budget answers {budget} what-ifs and {} were asked",
                    what_ifs.len()
                )));
            }
            for (index, what_if) in what_ifs.iter().enumerate() {
                match what_if {
                    Json::Object(entries) if entries.is_empty() => {}
                    _ => {
                        return Err(Error::invalid(format!(
                            "what-if {index} names something: the what-if vocabulary is \
                             `gp.api.v1.WhatIf`, which has no fields in this build"
                        )));
                    }
                }
            }
        } else if request.param("what_ifs").is_some() {
            return Err(Error::invalid("`what_ifs` is an array"));
        }

        let seats = host.world().seats();
        let row = seats
            .seats()
            .iter()
            .position(|held| *held == seat.raw())
            .ok_or_else(|| Error::not_found(format!("this match has no seat {}", seat.raw())))?;
        let treasury = seats
            .treasuries()
            .get(row)
            .copied()
            .unwrap_or_default()
            .raw();
        let supply = seats.supplies().get(row).copied().unwrap_or_default().raw();
        let draw = seats.draws().get(row).copied().unwrap_or_default().raw();
        let treasury =
            i32::try_from(treasury).unwrap_or(if treasury < 0 { i32::MIN } else { i32::MAX });
        let headroom = supply.saturating_sub(draw);
        let number = |value: i32| Json::Number(value.to_string());
        Ok(Json::Object(vec![
            (String::from("treasury_now"), number(treasury)),
            (String::from("supply_kw_now"), number(supply)),
            (String::from("draw_kw_now"), number(draw)),
            (String::from("headroom_kw_now"), number(headroom)),
        ]))
    }

    /// `estimate_route`: item 61's travel estimate, exposed.
    ///
    /// Abstract search alone -- it never refines, never steps, never forks --
    /// through the adapter item 100 (1) puts in this crate. Never optimistic,
    /// and a fogged leg comes back flagged so the editor draws it dashed and
    /// labels it a bound rather than an ETA (item 57).
    pub(super) fn estimate_route(
        &self,
        subject: crate::token::Subject,
        request: &Request,
    ) -> Result<Json, Error> {
        let seat = Surface::seat_of(subject, "a route to estimate")?;
        let Some(Json::Array(waypoints)) = request.param("waypoints") else {
            return Err(Error::invalid(
                "`waypoints` is an array of two or more `gp.v1.Location`s",
            ));
        };
        if waypoints.len() < 2 {
            return Err(Error::invalid(format!(
                "a route has two or more waypoints and this has {}",
                waypoints.len()
            )));
        }
        // Before a single search runs: see [`MAX_WAYPOINTS`].
        if waypoints.len() > MAX_WAYPOINTS {
            return Err(Error::invalid(format!(
                "a route names at most {MAX_WAYPOINTS} waypoints and this names {}",
                waypoints.len()
            )));
        }
        let mut places: Vec<Voxel> = Vec::with_capacity(waypoints.len());
        for (index, waypoint) in waypoints.iter().enumerate() {
            let from = places.last().copied();
            places.push(self.place_of(seat, waypoint, index, from)?);
        }

        let route = pharmakos_plan_core::travel::Route::estimate(&places, self.host()?.routes());
        let legs: Vec<Json> = route
            .legs
            .iter()
            .map(|leg| {
                let ms = leg.estimate.map_or(0, |found| found.ms().raw());
                Json::Object(vec![
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
                    (String::from("ms"), Json::Number(ms.to_string())),
                    (String::from("fogged"), Json::Bool(leg.fogged)),
                ])
            })
            .collect();
        let total = route.total().unwrap_or(Ms::ZERO);
        Ok(Json::Object(vec![
            (
                String::from("reachable"),
                Json::Bool(!route.is_unreachable()),
            ),
            (String::from("ms"), Json::Number(total.raw().to_string())),
            (String::from("legs"), Json::Array(legs)),
        ]))
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
    /// to, so it is refused as any other non-place is.
    fn place_of(
        &self,
        seat: SeatId,
        waypoint: &Json,
        index: usize,
        from: Option<Voxel>,
    ) -> Result<Voxel, Error> {
        use pharmakos_sim::interpreter::{BeaconSpec, beacon_spec_of, resolve_beacon_in};
        let at = format!("waypoint {index}");
        if let Some(voxel) = waypoint.get("voxel") {
            return read_voxel(voxel, &at);
        }
        if let Some(reference) = waypoint.get("covering") {
            return self.covering_site(seat, reference, &at, from);
        }
        let host = self.host()?;
        let spec = if let Some(reference) = waypoint.get("beacon_anchor") {
            let reference: pharmakos_proto::gp::v1::BeaconRef =
                pharmakos_proto::json::decode_json(reference).map_err(|error| {
                    Error::invalid(format!(
                        "{at}: `beacon_anchor` is a `gp.v1.BeaconRef`: {error}"
                    ))
                })?;
            beacon_spec_of(&reference, host.rules())
                .map_err(|error| Error::invalid(format!("{at}: {error}")))?
        } else if waypoint.get("safest").is_some() {
            BeaconSpec::Safest
        } else {
            return Err(Error::invalid(format!(
                "{at} names no place: a waypoint is a `voxel`, a `beacon_anchor`, `safest` or \
                 `covering`"
            )));
        };
        let snapshot = host.runner().frozen().snapshot();
        let origin = match from {
            Some(voxel) => Some(fx_point(voxel, &at)?),
            None => None,
        };
        let Some(beacon) = resolve_beacon_in(snapshot, host.rules(), seat, spec, origin) else {
            return Err(Error::not_found(match spec {
                BeaconSpec::Own(ordinal) => format!(
                    "{at} names `{}`, which this seat has not",
                    pharmakos_sim::tables::own_beacon_name(ordinal)
                ),
                BeaconSpec::Foreign(handle) => format!(
                    "{at} names `{}`, which is not this seat's to walk to by name",
                    pharmakos_sim::tables::foreign_beacon_name(handle)
                ),
                _ => format!("{at} is a beacon of this seat's, and none answers its selector"),
            }));
        };
        let row = snapshot
            .beacon_id
            .iter()
            .position(|held| *held == beacon.raw())
            .ok_or_else(|| Error::internal(format!("{at}: the resolved beacon has no row")))?;
        let axis = |offset: usize| {
            row.checked_mul(3)
                .and_then(|base| base.checked_add(offset))
                .and_then(|slot| snapshot.beacon_pos.get(slot).copied())
                .map(pharmakos_sim::math::fixed::Fx::from_raw)
        };
        match (axis(0), axis(1), axis(2)) {
            (Some(x), Some(y), Some(z)) => Ok(view::voxel_of([x, y, z])),
            _ => Err(Error::internal(format!(
                "{at}: the resolved beacon has no anchor"
            ))),
        }
    }

    /// A `covering` waypoint's site: where the beacon would stand, as the sim
    /// chooses it ([`Surface::place_of`] says why this is here).
    fn covering_site(
        &self,
        seat: SeatId,
        reference: &Json,
        at: &str,
        from: Option<Voxel>,
    ) -> Result<Voxel, Error> {
        use crate::targeting::Site;
        let reference: pharmakos_proto::gp::v1::FeatureRef =
            pharmakos_proto::json::decode_json(reference).map_err(|error| {
                Error::invalid(format!("{at}: `covering` is a `gp.v1.FeatureRef`: {error}"))
            })?;
        let spec = crate::targeting::spec_of(&reference, Site::Covering)
            .map_err(|why| Error::invalid(format!("{at}: {why}")))?;
        let world = self.host()?.world();
        let origin = match from {
            Some(voxel) => fx_point(voxel, at)?,
            None => crate::targeting::commander_point(world, seat).ok_or_else(|| {
                Error::not_found(format!(
                    "{at} covers from where the commander stands, and it is standing nowhere"
                ))
            })?,
        };
        let tally = pharmakos_sim::seams::UnitTally::new();
        let ground = crate::targeting::lend(world, &tally);
        let mut scratch = crate::targeting::scratch_for(world)?;
        let found = pharmakos_sim::targeting::cover(&ground, &mut scratch, seat, origin, spec)
            .map_err(|failure| {
                Error::not_found(format!(
                    "{at} covers nothing: its step would fail `{}`",
                    failure.name()
                ))
            })?;
        let [x, y] = found.site;
        let point = ground
            .standing(x, y)
            .ok_or_else(|| Error::internal(format!("{at}: the sim chose a site off the map")))?;
        Ok(view::voxel_of(point))
    }

    /// One `gp.api.v1.BeaconSummary`, as `viewer` may be told it.
    ///
    /// `beacon_id`, `at` and `owner` for every beacon a viewer may see. For a
    /// seat's **own** beacon, also `core`, `priority` and `powered`
    /// (decisions-log item 111, decision C7): what a client needs to tell
    /// which of its beacons would brown out first. **Another seat's beacon
    /// carries its owner and nothing more**, under every fog policy and to
    /// every viewer that is not its owner — a spectator and the lobby
    /// included — because its power state and priority are that seat's
    /// business (spec section 3 names only a seat's own), and a field left
    /// unset is left out rather than written as a zero a reader would take for
    /// an answer. The rest of spec section 12's beacon detail — mandate, HP,
    /// units by role, tags, a sighting's age — stays `reserved 7 to 15`.
    fn beacon_summary(&self, viewer: Viewer, name: &str, row: usize, at: Voxel) -> Json {
        let mut entries = vec![
            (String::from("beacon_id"), Json::String(name.to_owned())),
            (
                String::from("at"),
                Json::Object(vec![
                    (String::from("x"), Json::Number(at.x.to_string())),
                    (String::from("y"), Json::Number(at.y.to_string())),
                    (String::from("z"), Json::Number(at.z.to_string())),
                ]),
            ),
        ];
        let Ok(host) = self.host() else {
            return Json::Object(entries);
        };
        let beacons = host.world().beacons();
        if row >= beacons.ids().len() {
            return Json::Object(entries);
        }
        let id = beacons.ids().get(row).copied().unwrap_or(u32::MAX);
        let owner = beacons.seats().get(row).copied().unwrap_or_default();
        entries.push((
            String::from("owner"),
            Json::String(crate::token::Subject::Seat(SeatId::new(owner)).render()),
        ));
        if viewer != Viewer::Seat(SeatId::new(owner)) {
            return Json::Object(entries);
        }
        let core = core_beacon_of(host.world(), SeatId::new(owner)) == Some(id);
        let priority = beacons.priorities().get(row).copied().unwrap_or_default();
        let powered = !beacons.dormant().get(row).copied().unwrap_or(false);
        entries.push((String::from("core"), Json::Bool(core)));
        entries.push((
            String::from("priority"),
            Json::String(priority_wire_name(priority)),
        ));
        entries.push((String::from("powered"), Json::Bool(powered)));
        Json::Object(entries)
    }

    /// The beacons a viewer may see, with this viewer's name for each, own
    /// first by ordinal (`b_00` first), then everybody else's by this viewer's
    /// handle (`e_01` first).
    ///
    /// Another seat's beacons are named in table order the first time they
    /// are shown together, so the handles a listing mints count up in the
    /// order a seat's own listing reads; a beacon seen later takes the next
    /// handle and sorts last, which keeps an earlier page's cursor honest.
    fn visible_beacons<V: Vision>(
        &mut self,
        viewer: Viewer,
        vision: &V,
    ) -> Vec<(String, usize, Voxel)> {
        let Some(host) = self.host.as_ref() else {
            return Vec::new();
        };
        let world = host.world();
        let beacons = world.beacons();
        let filter = FogFilter::new(&self.fog, vision);
        let mut found: Vec<(bool, usize, Voxel)> = Vec::new();
        for row in 0..beacons.ids().len() {
            let owner = beacons.seats().get(row).copied().unwrap_or_default();
            let at = beacons
                .positions()
                .get(row)
                .copied()
                .map(view::voxel_of)
                .unwrap_or_default();
            let audience = Audience::World {
                owner: Some(SeatId::new(owner)),
                at,
            };
            if !filter.visible(viewer, &audience) {
                continue;
            }
            let own = viewer == Viewer::Seat(SeatId::new(owner));
            found.push((!own, row, at));
        }
        // Named in table order, which is the order a first listing mints the
        // `e_NN` handles in.
        found.sort_unstable_by_key(|(foreign, row, _)| (*foreign, *row));
        let mut named: Vec<(bool, u32, String, usize, Voxel)> = found
            .into_iter()
            .map(|(foreign, row, at)| {
                let name = self.views.beacon_name(viewer, world, row);
                let number = match pharmakos_sim::tables::parse_beacon_name(&name) {
                    Some(
                        pharmakos_sim::tables::BeaconName::Own(number)
                        | pharmakos_sim::tables::BeaconName::Foreign(number),
                    ) => number,
                    None => u32::MAX,
                };
                (foreign, number, name, row, at)
            })
            .collect();
        // Own beacons first, then by the number in the name: every sort key
        // ends in a unique value (item 62's convention, which this crate keeps
        // because its outputs are byte-compared across three operating
        // systems), and within a side a name's number is unique.
        named.sort_unstable_by_key(|(foreign, number, _, row, _)| (*foreign, *number, *row));
        named
            .into_iter()
            .map(|(_, _, name, row, at)| (name, row, at))
            .collect()
    }
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
/// the enum does not name is the sim and the schema disagreeing; it is spelt
/// `unspecified` rather than guessed at, which a client reads as "not known".
fn priority_wire_name(value: u8) -> String {
    use pharmakos_proto::gp::v1::interface_row::QuartermasterPriority;
    let named = QuartermasterPriority::try_from(i32::from(value))
        .unwrap_or(QuartermasterPriority::Unspecified);
    pharmakos_proto::scope::wire_name(
        "gp.v1.InterfaceRow.QuartermasterPriority",
        named.as_str_name(),
    )
}

/// A whole voxel as the fixed-point point at its corner, which is the voxel
/// a selector's ranking floors it back to.
fn fx_point(voxel: Voxel, at: &str) -> Result<[pharmakos_sim::math::fixed::Fx; 3], Error> {
    let axis = |value: i32| {
        i16::try_from(value)
            .map(pharmakos_sim::math::fixed::Fx::from_voxels)
            .map_err(|_| Error::invalid(format!("{at}: the waypoint before it is off the map")))
    };
    Ok([axis(voxel.x)?, axis(voxel.y)?, axis(voxel.z)?])
}

/// One `gp.v1.Voxel` out of a request.
fn read_voxel(value: &Json, at: &str) -> Result<Voxel, Error> {
    let axis = |name: &str| -> Result<i32, Error> {
        match value.get(name) {
            None => Ok(0),
            Some(Json::Number(lexeme)) => lexeme
                .parse::<i32>()
                .map_err(|_| Error::invalid(format!("{at}: `{name}` is a whole number of voxels"))),
            Some(other) => Err(Error::invalid(format!(
                "{at}: `{name}` is a whole number of voxels, and this is {}",
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
