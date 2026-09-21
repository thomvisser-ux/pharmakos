// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The knowledge reads: what a seat is allowed to know about the match it is
//! in.
//!
//! Spec section 12's Knowledge group, minus the four methods `gateway.proto`
//! gives no request/response pair. Every one of them reads the **frozen
//! planning snapshot or the live world through the fog filter**, and not one of
//! them steps anything: `estimate_route` goes through
//! [`crate::routes::RouteAdapter`], which owns its own search graph and has no
//! `World` in its hand at all, and `get_economy_forecast` is arithmetic over
//! the snapshot (AGENTS.md section 3 rule 2).
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
//! | `get_map_summary` | nothing | the extent, the seed, the prose |
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
use pharmakos_sim::tables::{BeaconId, SeatId};

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
                Json::String(format!(
                    "{} {}",
                    strings::briefing(
                        host.runner().phase(),
                        host.runner().round(),
                        world.match_state().round_limit(),
                        segment_ms,
                        beacons,
                        units,
                    ),
                    strings::standing(living),
                )),
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
        let outcome = host
            .runner()
            .outcome()
            .map(|outcome| (outcome.reason, outcome.winner.map(SeatId::raw)));
        Ok(Json::Object(vec![(
            String::from("prose"),
            Json::String(strings::recap(host.runner().round(), ticks, outcome)),
        )]))
    }

    /// `list_beacons`: the beacons this seat may see, its own first.
    pub(super) fn list_beacons<V: Vision>(
        &self,
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
            Some(text) if !text.is_empty() => {
                usize::try_from(crate::feed::Cursor::parse(text, snapshot)?.index())
                    .unwrap_or(usize::MAX)
            }
            _ => 0,
        };
        let page: Vec<Json> = visible
            .iter()
            .skip(from)
            .take(limit.max(1))
            .map(|(id, at)| beacon_summary(*id, *at))
            .collect();
        let index = from.min(visible.len()).saturating_add(page.len());
        let next = if index >= visible.len() {
            String::new()
        } else {
            crate::feed::Cursor::at(snapshot, u32::try_from(index).unwrap_or(u32::MAX)).render()
        };
        Ok(Json::Object(vec![
            (String::from("beacons"), Json::Array(page)),
            (String::from("next_cursor"), Json::String(next)),
        ]))
    }

    /// `get_beacon`: one beacon, if this seat may see it.
    pub(super) fn get_beacon<V: Vision>(
        &self,
        subject: crate::token::Subject,
        held: crate::scopes::ScopeSet,
        request: &Request,
        vision: &V,
    ) -> Result<Json, Error> {
        let _ = detail::of(request)?;
        let wanted = request
            .string_param("beacon_id")?
            .ok_or_else(|| Error::invalid("`beacon_id` names a beacon, as in `b_04`"))?;
        let id = view::beacon_id_from(wanted)
            .ok_or_else(|| Error::not_found(format!("`{wanted}` is not a beacon id")))?;
        let viewer = self.viewer_of(subject, held);

        let host = self.host()?;
        let beacons = host.world().beacons();
        let row = beacons
            .ids()
            .iter()
            .position(|held| *held == id.raw())
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
        let filter = FogFilter::new(self.fog_policy(), vision);
        let audience = Audience::World {
            owner: Some(SeatId::new(owner)),
            at,
        };
        if !filter.visible(viewer, &audience) {
            return Err(Error::not_found(format!("no beacon `{wanted}` here")));
        }

        let hit_points = beacons
            .hit_points()
            .get(row)
            .copied()
            .unwrap_or_default()
            .raw();
        let mandate = view::mandate_from_id(beacons.mandates().get(row).copied().unwrap_or(0));
        let own = viewer == Viewer::Seat(SeatId::new(owner));
        Ok(Json::Object(vec![
            (String::from("summary"), beacon_summary(id, at)),
            (
                String::from("prose"),
                Json::String(strings::beacon(
                    &view::beacon_id(id),
                    view::mandate_name(mandate),
                    hit_points,
                    own,
                )),
            ),
        ]))
    }

    /// `get_map_summary`: the extent and the seed.
    ///
    /// Neither is a secret -- "part of the match, not a secret"
    /// (`gateway.proto`) -- so this one is not fog-filtered. What *is* fogged
    /// about a map is its vents, seams and terrain, and those are the
    /// `reserved 5 to 15` the message holds for S1.
    pub(super) fn get_map_summary(&self, request: &Request) -> Result<Json, Error> {
        let _ = detail::of(request)?;
        let host = self.host()?;
        let size = host.world().voxels().size();
        let seed = view::seed_text(host.world().match_seed());
        let seats = host.world().seats().len();
        let axis = |index: usize| {
            Json::Number(i64::from(size.get(index).copied().unwrap_or(0)).to_string())
        };
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
        ]))
    }

    /// `get_economy_forecast`: `$` and kW, with what-ifs.
    ///
    /// **This build answers the envelope and nothing in it, and that is not a
    /// stub that was forgotten.** `gp.api.v1.GetEconomyForecastResponse`
    /// reserves 1 to 15 and `gp.api.v1.WhatIf` reserves 1 to 15: the treasury,
    /// the projected income, the kW supply, draw and headroom and the what-if
    /// vocabulary are all numbers the **economy** produces, and the economy is
    /// T14. Inventing field numbers for them here would pick T14's numbering
    /// for it, which is a contract change wearing a disguise (AGENTS.md
    /// section 5).
    ///
    /// What the method does do is real and is what a client needs from it
    /// today: it exists, it is scoped and phase-checked like every other read,
    /// it refuses a what-if that names anything -- because `WhatIf` has no
    /// field a caller could name -- and it carries the `_status` footer.
    ///
    /// PLACEHOLDER: the payload, filled with **T14**'s economy, in the proto
    /// change that gives `WhatIf` and the response their fields.
    pub(super) fn get_economy_forecast(&self, request: &Request) -> Result<Json, Error> {
        let budget = what_if_budget(detail::of(request)?);
        // Read for its side effect: a match must be hosted for a forecast to
        // mean anything, and a client that asked one of the lobby should be
        // told rather than handed an empty object that looks like an answer.
        let _ = self.host()?;
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
        Ok(Json::Object(Vec::new()))
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
        let mut places: Vec<Voxel> = Vec::with_capacity(waypoints.len());
        for (index, waypoint) in waypoints.iter().enumerate() {
            places.push(self.place_of(seat, waypoint, index)?);
        }

        let route = pharmakos_plan_core::travel::Route::estimate(&places, self.host()?.routes());
        let legs: Vec<Json> = route
            .legs
            .iter()
            .map(|leg| {
                let ms = leg.estimate.map_or(0, |found| found.ms().raw());
                Json::Object(vec![
                    (
                        String::from("to"),
                        Json::Object(vec![
                            (String::from("x"), Json::Number(leg.to.x.to_string())),
                            (String::from("y"), Json::Number(leg.to.y.to_string())),
                            (String::from("z"), Json::Number(leg.to.z.to_string())),
                        ]),
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
    /// Three of the oneof's arms exist, and only the two that need no knowledge
    /// store are answered here: a literal voxel, and a beacon the seat's own
    /// scope carries (`beacon_anchor{beacon_id}` and the `safest` shorthand,
    /// which is own beacons only). A **selector** -- nearest, weakest, most
    /// threatened -- resolves at step start over the beacons that pass its
    /// filter, which is the interpreter's job at run time and not a question
    /// an estimate can answer at plan time.
    fn place_of(&self, seat: SeatId, waypoint: &Json, index: usize) -> Result<Voxel, Error> {
        let at = format!("waypoint {index}");
        if let Some(voxel) = waypoint.get("voxel") {
            return read_voxel(voxel, &at);
        }
        let beacon_ref = waypoint
            .get("beacon_anchor")
            .or_else(|| waypoint.get("safest"));
        let Some(beacon_ref) = beacon_ref else {
            return Err(Error::invalid(format!(
                "{at} names no place: a waypoint is a `voxel` or a `beacon_anchor`"
            )));
        };
        let scope = self.verifier_scope(seat)?;
        // `safest` and `beacon_anchor{safest}` are the same place and are own
        // beacons only, so "the seat's first own beacon" is the whole of what
        // this build can answer without the threat model S2 brings.
        let named = beacon_ref.get("beacon_id").and_then(|value| match value {
            Json::String(text) => Some(text.clone()),
            _ => None,
        });
        match named {
            Some(beacon_id) => scope
                .beacon(&beacon_id)
                .map(|known| known.at)
                .ok_or_else(|| {
                    Error::not_found(format!("{at} names `{beacon_id}`, which this seat has not"))
                }),
            None => scope
                .own_beacons()
                .next()
                .map(|known| known.at)
                .ok_or_else(|| {
                    Error::not_found(format!("{at} is a beacon of this seat's, and it has none"))
                }),
        }
    }

    /// The beacons a viewer may see, own first, then by id.
    fn visible_beacons<V: Vision>(&self, viewer: Viewer, vision: &V) -> Vec<(BeaconId, Voxel)> {
        let Ok(host) = self.host() else {
            return Vec::new();
        };
        let beacons = host.world().beacons();
        let filter = FogFilter::new(self.fog_policy(), vision);
        let mut found: Vec<(bool, u32, Voxel)> = Vec::new();
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
            found.push((
                !own,
                beacons.ids().get(row).copied().unwrap_or(u32::MAX),
                at,
            ));
        }
        // Own beacons first, then by id: every sort key ends in a unique id
        // (item 62's convention, which this crate keeps because its outputs are
        // byte-compared across three operating systems).
        found.sort_unstable_by_key(|(foreign, id, _)| (*foreign, *id));
        found
            .into_iter()
            .map(|(_, id, at)| (BeaconId::new(id), at))
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

/// One `gp.api.v1.BeaconSummary`.
///
/// Two fields, which is what the message has: everything else about a beacon --
/// its mandate, HP, power state, units by role, tags and the sighting age of
/// one that is not the seat's own -- is `reserved 3 to 15`, waiting for the
/// systems that produce them.
fn beacon_summary(id: BeaconId, at: Voxel) -> Json {
    Json::Object(vec![
        (String::from("beacon_id"), Json::String(view::beacon_id(id))),
        (
            String::from("at"),
            Json::Object(vec![
                (String::from("x"), Json::Number(at.x.to_string())),
                (String::from("y"), Json::Number(at.y.to_string())),
                (String::from("z"), Json::Number(at.z.to_string())),
            ]),
        ),
    ])
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
