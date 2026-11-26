// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A bench match, hosted exactly as `gamectl scenario run` hosts one: the
//! gateway's [`Host`] behind a [`Surface`], every seat's playbook arriving
//! through `submit_plan` with that seat's own token and sealed by
//! [`Surface::begin_push`].
//!
//! P1's decision tick ([`crate::decision`]) and G3′-real ([`crate::g3`]) both
//! open their matches here, so the two figures are taken on the same door.
//! Nothing here seals around the verifier: a seat whose submission is refused,
//! or that is not exactly on the size budget, stops the measurement with the
//! reason.
//!
//! The one host-side seam it adds to that door is the lobby layout
//! ([`Host::seed_layout`], S2's plan, E30, and from S3 its mandate settings,
//! [`Host::seed_layout_with_settings`], E13), placed before the Lull opens, so
//! the layout is an input of the match like its seed. [`open_lull`] stops at
//! the Lull, with nothing submitted, for a measurement of the planning door.

use pharmakos_gateway::fog::{Blind, FogPolicy};
use pharmakos_gateway::host::Host;
use pharmakos_gateway::scopes::{Scope, ScopeSet};
use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::token::{Subject, Token};
use pharmakos_proto::json::{self, Json};
use pharmakos_sim::math::quantity::{Ms, Tick};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::runner::{Layout, LayoutSettings, MatchSettings};
use pharmakos_sim::tables::SeatId;
use pharmakos_sim::world::{World, WorldConfig};

use crate::error::Error;
use crate::shapes::Core;

/// The match id every bench match is hosted under. Lower-case letters and
/// hyphens only, as `MatchCache::valid_match_id` asks; it reaches nothing
/// hashed.
const MATCH_ID: &str = "m-bench";

/// How a bench match is opened.
#[derive(Clone, Copy, Debug)]
pub struct Opening<'a> {
    /// The map seed.
    pub seed: u64,
    /// The seats, each sealing `playbook`.
    pub seats: u32,
    /// The one segment's length, in game milliseconds.
    pub segment_ms: i32,
    /// The rules table the match is played under.
    pub rules: &'a RulesTable,
    /// The playbook every seat submits, as canonical JSON.
    pub playbook: &'a str,
}

/// A bench match in its Push, and the size units each seat sealed.
#[derive(Debug)]
pub struct Opened {
    /// The match, its Push open.
    pub surface: Surface,
    /// The size units each seat's sealed playbook holds: the size budget.
    pub units_per_seat: u32,
}

/// The seats `0..seats`, as ids.
///
/// # Errors
///
/// [`Error::Overflow`] for a count past a seat id's range.
pub fn seat_ids(seats: u32) -> Result<Vec<SeatId>, Error> {
    (0..seats)
        .map(|seat| u8::try_from(seat).map(SeatId::new))
        .collect::<Result<_, _>>()
        .map_err(|_| Error::Overflow("the seat count"))
}

/// Open the host alone, in its lobby, so a caller can read the generated world
/// and lay it out before [`attach`] hands it to a surface.
///
/// # Errors
///
/// [`Error::Gateway`] when the host refuses the match.
pub fn open_host(opening: &Opening<'_>) -> Result<Host, Error> {
    Host::open(
        &WorldConfig {
            match_seed: opening.seed,
            seats: opening.seats,
            rules: opening.rules.clone(),
            match_settings: MatchSettings {
                segment_lengths_ms: vec![opening.segment_ms],
                round_limit: 1,
            },
        },
        None,
    )
    .map_err(|error| Error::gateway("opening the bench match", &error))
}

/// Lay `host` out (when a layout is given), put it behind a surface, open the
/// Lull, submit `opening.playbook` for every seat and open the Push.
///
/// # Errors
///
/// [`Error::Gateway`] for a refused step of the match's opening, and
/// [`Error::Submission`] for a refused or off-budget playbook.
pub fn attach(host: Host, layout: Option<&Layout>, opening: &Opening<'_>) -> Result<Opened, Error> {
    attach_with(host, layout, &[], opening)
}

/// [`attach`], with mandate settings for the layout's beacons (S3's plan,
/// E13), placed through the host's lobby seam `Host::seed_layout_with_settings`
/// as the layout is.
///
/// # Errors
///
/// As [`attach`].
pub fn attach_with(
    host: Host,
    layout: Option<&Layout>,
    settings: &[LayoutSettings],
    opening: &Opening<'_>,
) -> Result<Opened, Error> {
    let seats = usize::try_from(opening.seats).map_err(|_| Error::Overflow("the seat count"))?;
    let playbooks = vec![opening.playbook.to_owned(); seats];
    attach_each(host, layout, settings, opening, &playbooks)
}

/// [`attach_with`], with a playbook of its own for each seat, in seat order:
/// a playbook that names places round a seat's own core
/// ([`seat_core`]) is a different text for each seat. `opening.playbook` is
/// not read.
///
/// # Errors
///
/// As [`attach`], and [`Error::Submission`] when the playbooks are not one a
/// seat.
pub fn attach_each(
    mut host: Host,
    layout: Option<&Layout>,
    settings: &[LayoutSettings],
    opening: &Opening<'_>,
    playbooks: &[String],
) -> Result<Opened, Error> {
    if let Some(layout) = layout {
        host.seed_layout_with_settings(layout, settings)
            .map_err(|error| Error::gateway("placing the gate's layout", &error))?;
    } else if !settings.is_empty() {
        return Err(Error::Layout(format!(
            "{} mandate settings rows and no layout for them to name",
            settings.len()
        )));
    }
    let mut surface = open_lull(host, opening, Some(Ms::new(lull_ms(opening.rules)?)))?;
    let seats = seat_ids(opening.seats)?;
    if playbooks.len() != seats.len() {
        return Err(Error::Submission(format!(
            "{} playbooks for {} seats",
            playbooks.len(),
            seats.len()
        )));
    }
    let mut units_per_seat = None;
    for (seat, playbook) in seats.iter().zip(playbooks) {
        let token = mint(&mut surface, *seat)?;
        let units = submit(&mut surface, &token, *seat, playbook)?;
        if units_per_seat.is_some_and(|kept| kept != units) {
            return Err(Error::Submission(format!(
                "seat {} sealed {units} size units and an earlier seat a different count, from \
                 the same playbook",
                seat.raw()
            )));
        }
        units_per_seat = Some(units);
    }
    let units_per_seat = units_per_seat
        .ok_or_else(|| Error::Empty("a bench match of no seats seals nothing".to_owned()))?;
    if !surface
        .begin_push()
        .map_err(|error| Error::gateway("opening the Push", &error))?
    {
        return Err(Error::Gateway {
            during: "opening the Push",
            message: "the match was not in its Lull".to_owned(),
        });
    }
    Ok(Opened {
        surface,
        units_per_seat,
    })
}

/// `rules.match.lull_ms`: the countdown a bench match's Lull is opened with.
///
/// # Errors
///
/// [`Error::Rules`] when the table has no `match` block.
pub fn lull_ms(rules: &RulesTable) -> Result<i32, Error> {
    rules
        .message()
        .r#match
        .as_ref()
        .map(|block| block.lull_ms)
        .ok_or_else(|| {
            Error::Rules("no `match` block, so nothing says how long a Lull is".to_owned())
        })
}

/// Put `host` behind a surface and open its first Lull with nothing
/// submitted: the planning door every seat's editor calls through (P3's
/// estimate is taken here). `countdown`, when given, is set on the Lull
/// before it opens, as a host that spends no time in it sets it
/// ([`attach`] gives [`lull_ms`]); without one the Lull opens untimed, for a
/// caller that reports its own clock through `Surface::set_host_clock`.
///
/// # Errors
///
/// [`Error::Gateway`] for a refused step of the match's opening.
pub fn open_lull(
    host: Host,
    opening: &Opening<'_>,
    countdown: Option<Ms>,
) -> Result<Surface, Error> {
    let seats = seat_ids(opening.seats)?;
    let mut surface = Surface::new(
        MATCH_ID,
        opening.seed,
        opening.rules.clone(),
        FogPolicy::fogged(),
        &seats,
    )
    .map_err(|error| Error::gateway("building the surface", &error))?;
    surface
        .attach(host)
        .map_err(|error| Error::gateway("attaching the host", &error))?;
    if let Some(countdown) = countdown {
        surface
            .set_phase_remaining_ms(countdown)
            .map_err(|error| Error::gateway("setting the Lull's countdown", &error))?;
    }
    surface
        .open_lull()
        .map_err(|error| Error::gateway("opening the Lull", &error))?;
    Ok(surface)
}

/// `seat`'s core on `world`, as the seat's own view names it: `b_00`, the
/// seat's lowest-id beacon row (the gateway's per-seat beacon names,
/// `crates/gateway/src/surface.rs`, "`b_00`, its core, first"), at its row's
/// position, with the map's extent from `rules`.
///
/// # Errors
///
/// [`Error::Layout`] when the seat holds no beacon, [`Error::Rules`] for a
/// table with no `map` block, and [`Error::Overflow`] for an extent past an
/// `i32`.
pub fn seat_core(world: &World, seat: SeatId, rules: &RulesTable) -> Result<Core, Error> {
    let beacons = world.beacons();
    let at = beacons
        .ids()
        .iter()
        .zip(beacons.seats())
        .zip(beacons.positions())
        .filter(|((_, owner), _)| **owner == seat.raw())
        .min_by_key(|((id, _), _)| **id)
        .map(|(_, at)| at)
        .ok_or_else(|| Error::Layout(format!("seat {} has no core on the map", seat.raw())))?;
    let map = rules
        .message()
        .map
        .as_ref()
        .ok_or_else(|| Error::Rules("no `map` block".to_owned()))?;
    let extent = |size: u32| i32::try_from(size).map_err(|_| Error::Overflow("the map's extent"));
    let column = [
        at[0].floor_voxels(),
        at[1].floor_voxels(),
        at[2].floor_voxels(),
    ];
    Ok(Core {
        id: "b_00".to_owned(),
        at: column,
        extent: [extent(map.size_x)?, extent(map.size_y)?],
        far: far_point(world, column)?,
    })
}

/// How far, along each axis, [`far_point`] looks from a core: what a
/// commander walks well inside a kind's segment before its first
/// placement resolves.
pub const FAR_REACH: i32 = 45;

/// The column within [`FAR_REACH`] of `core` along each axis, standable and
/// connected to the core's own column, whose nearest vent (by squared
/// column distance to its anchor) is farthest; ties go to the first in
/// `(y, x)` order. A pure function of the map, so the far covering kind
/// walks to the same column on every run (`cert3` fix pass 3, N2 (c)).
/// Columns no one can stand on, or that the core's component does not
/// reach, are no candidates: a walk there could not be served.
///
/// # Errors
///
/// [`Error::Layout`] when the core stands on no node, the map holds no
/// vent, or no column qualifies.
pub fn far_point(world: &World, core: [i32; 3]) -> Result<[i32; 2], Error> {
    let surface = world.surface();
    let clusters = world.clusters();
    let home = surface
        .node_of(core[0], core[1])
        .ok_or_else(|| Error::Layout(format!("a core stands on no node at {core:?}")))?;
    let vents: Vec<[i32; 2]> = world
        .features()
        .features()
        .iter()
        .filter(|feature| feature.kind == pharmakos_sim::features::FeatureKind::Vent)
        .map(|feature| feature.anchor)
        .collect();
    let span = |at: i32| -> Result<(i32, i32), Error> {
        Ok((
            at.checked_sub(FAR_REACH)
                .ok_or(Error::Overflow("the far point's search span"))?,
            at.checked_add(FAR_REACH)
                .ok_or(Error::Overflow("the far point's search span"))?,
        ))
    };
    // A column's squared distance to a vent's anchor, in `i64`: each axis's
    // difference is widened before it is taken, and every step is checked.
    let squared = |[vx, vy]: [i32; 2], x: i32, y: i32| -> Result<i64, Error> {
        let overflow = Error::Overflow("a vent's squared distance");
        let dx = i64::from(vx)
            .checked_sub(i64::from(x))
            .ok_or(overflow.clone())?;
        let dy = i64::from(vy)
            .checked_sub(i64::from(y))
            .ok_or(overflow.clone())?;
        dx.checked_mul(dx)
            .and_then(|xx| dy.checked_mul(dy).and_then(|yy| xx.checked_add(yy)))
            .ok_or(overflow)
    };
    let ((x0, x1), (y0, y1)) = (span(core[0])?, span(core[1])?);
    let mut best: Option<(i64, [i32; 2])> = None;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let reached = surface
                .node_of(x, y)
                .is_some_and(|node| clusters.connected(surface, home, node));
            if !reached {
                continue;
            }
            let mut nearest: Option<i64> = None;
            for vent in &vents {
                let distance = squared(*vent, x, y)?;
                if nearest.is_none_or(|kept| distance < kept) {
                    nearest = Some(distance);
                }
            }
            let nearest =
                nearest.ok_or_else(|| Error::Layout("the map holds no vent".to_owned()))?;
            if best.is_none_or(|(kept, _)| nearest > kept) {
                best = Some((nearest, [x, y]));
            }
        }
    }
    best.map(|(_, column)| column).ok_or_else(|| {
        Error::Layout(format!(
            "no column within {FAR_REACH} of the core at {core:?} is reached"
        ))
    })
}

/// The hosted world, read only.
///
/// # Errors
///
/// [`Error::Gateway`] when the surface hosts no match.
pub fn world(surface: &Surface) -> Result<&World, Error> {
    surface
        .host()
        .map(Host::world)
        .map_err(|error| Error::gateway("reading the hosted world", &error))
}

/// A seat token with the scopes a player's client holds: `observe`, `plan`
/// and `plan.submit`.
///
/// # Errors
///
/// [`Error::Gateway`] when the surface refuses to mint it.
pub fn mint(surface: &mut Surface, seat: SeatId) -> Result<Token, Error> {
    let scopes = ScopeSet::of(&[Scope::Observe, Scope::Plan, Scope::PlanSubmit]);
    surface
        .tokens()
        .mint(Subject::Seat(seat), MATCH_ID, scopes, Tick::ZERO)
        .map(|(token, _)| token)
        .map_err(|error| Error::gateway("minting a seat token", &error))
}

/// Submit through the door every player's playbook takes, and answer the
/// size units the report says were sealed. Refused, or off the budget, is an
/// error naming why.
fn submit(
    surface: &mut Surface,
    token: &Token,
    seat: SeatId,
    playbook: &str,
) -> Result<u32, Error> {
    let params = json::write(&Json::Object(vec![(
        String::from("playbook_jsonc"),
        Json::String(playbook.to_owned()),
    )]));
    let request = pharmakos_gateway::rpc::parse(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"submit_plan","params":{params}}}"#
    ))
    .map_err(|error| Error::Codec {
        what: "the bench's submit_plan request".to_owned(),
        reason: error.title().to_string(),
    })?;
    let answer = surface.call(Some(token), &request, &Blind);
    let result = answer.get("result");
    if result.and_then(|result| result.get("accepted")) != Some(&Json::Bool(true)) {
        return Err(Error::Submission(format!(
            "seat {} could not submit the budget playbook: {}",
            seat.raw(),
            json::write(&answer)
        )));
    }
    let report = result.and_then(|result| result.get("report"));
    let units = number(report.and_then(|report| report.get("size_units")));
    let budget = number(report.and_then(|report| report.get("size_budget")));
    match (units, budget) {
        (Some(units), Some(budget)) if units == budget => Ok(units),
        _ => Err(Error::Submission(format!(
            "seat {}'s sealed playbook is not exactly on the size budget ({units:?} of \
             {budget:?} units), so a cost per unit at the budget cannot be read from it",
            seat.raw()
        ))),
    }
}

/// A JSON number as a `u32`, or `None` for anything else: the caller names
/// what it expected.
fn number(value: Option<&Json>) -> Option<u32> {
    match value {
        Some(Json::Number(lexeme)) => lexeme.parse().ok(),
        _ => None,
    }
}

/// `method`'s request with `params` as a client writes it, with the widest
/// numeric id a client counts to (a `u64`'s twenty digits), as the gateway
/// measures a request it hands back (`crates/gateway/src/surface/planning.rs`,
/// `undo_travels`).
#[must_use]
pub fn request_text(method: &str, params: Json) -> String {
    pharmakos_gateway::rpc::request_text(&Json::Number(u64::MAX.to_string()), method, params)
}

/// Whether `method`'s request with `params` is a text one message carries:
/// the frame's own rule on its bytes (`pharmakos_gateway::frame::travels`),
/// as `patch_plan` decides it (decisions-log item 167 (4)). Answers the
/// request's bytes and the rule's answer.
#[must_use]
pub fn travels(method: &str, params: Json) -> (usize, bool) {
    let bytes = request_text(method, params).len();
    (bytes, pharmakos_gateway::frame::travels(bytes))
}

/// A hosted first Lull a planning call is timed in, with nothing submitted:
/// the door a seat's editor calls through. P3 times `estimate_route` here
/// and P1 FULL's end-to-end figure times `verify_plan`.
///
/// The Lull opens untimed, and before each call the host reports one more
/// tick of its clock (`Surface::set_host_clock`, untimed), so the gateway's
/// limiter, which counts in ticks, sees a client calling once a tick; the
/// countdown is round 1's Lull, `rules.match.first_lull_ms`, from its whole.
#[derive(Debug)]
pub struct PlanningLull {
    surface: Surface,
    token: Token,
    elapsed_ms: i32,
    length_ms: i32,
}

impl PlanningLull {
    /// Open the match on `seed` at `seats` seats under `rules` and stop in its
    /// first Lull, with seat 0's token.
    ///
    /// # Errors
    ///
    /// As [`open_host`] and [`open_lull`]; [`Error::Rules`] for a table with
    /// no `match` block.
    pub fn open(rules: &RulesTable, seed: u64, seats: u32) -> Result<PlanningLull, Error> {
        let opening = Opening {
            seed,
            seats,
            // The Lull is all the measurement plays; the segment never opens.
            segment_ms: pharmakos_sim::math::quantity::MS_PER_TICK,
            rules,
            playbook: "",
        };
        let host = open_host(&opening)?;
        let mut surface = open_lull(host, &opening, None)?;
        let token = mint(&mut surface, SeatId::new(0))?;
        let length_ms = rules
            .message()
            .r#match
            .as_ref()
            .map(|block| block.first_lull_ms)
            .ok_or_else(|| Error::Rules("no `match` block, so no first Lull".to_owned()))?;
        Ok(PlanningLull {
            surface,
            token,
            elapsed_ms: 0,
            length_ms,
        })
    }

    /// The hosted surface, read only.
    #[must_use]
    pub const fn surface(&self) -> &Surface {
        &self.surface
    }

    /// Report one more tick of host time in the Lull, untimed.
    fn tick(&mut self) -> Result<(), Error> {
        self.elapsed_ms = self
            .elapsed_ms
            .checked_add(pharmakos_sim::math::quantity::MS_PER_TICK)
            .ok_or(Error::Overflow("the Lull's clock"))?;
        let remaining = self
            .length_ms
            .checked_sub(self.elapsed_ms)
            .filter(|remaining| *remaining > 0)
            .ok_or_else(|| {
                Error::Empty(format!(
                    "the calls would outlast the {} ms Lull they are timed in",
                    self.length_ms
                ))
            })?;
        self.surface
            .set_host_clock(Ms::new(self.elapsed_ms), Ms::new(remaining))
            .map_err(|error| Error::gateway("reporting the host clock", &error))
    }

    /// One call of `method` with `params` as the editor waits on it, as seat
    /// 0: the request as the text a client writes (`rpc::request_text`), and
    /// the clock around everything the gateway does with it in process: the
    /// JSON-RPC parse, the door, the handler and the answer's encode to text.
    /// The socket stays outside. Answers the time, the request's bytes and
    /// the call's `result`; an answer with no `result` is the gateway's
    /// refusal, an error.
    ///
    /// # Errors
    ///
    /// [`Error::Codec`] for a request the gateway's parser refuses,
    /// [`Error::Gateway`] for a refusal, and as the clock report.
    pub fn time_text(&mut self, method: &str, params: Json) -> Result<(u128, Json), Error> {
        self.tick()?;
        let text = request_text(method, params);
        let started = std::time::Instant::now();
        let parsed = pharmakos_gateway::rpc::parse(&text);
        let answer = parsed
            .as_ref()
            .map(|request| self.surface.call(Some(&self.token), request, &Blind));
        let written = answer.as_ref().map(json::write);
        let ns = started.elapsed().as_nanos();
        std::hint::black_box(&written);
        let answer = answer.map_err(|error| Error::Codec {
            what: format!("the bench's {method} request"),
            reason: error.title().to_string(),
        })?;
        match answer.get("result") {
            Some(result) => Ok((ns, result.clone())),
            None => Err(Error::Gateway {
                during: "a planning call in the Lull",
                message: format!(
                    "{method} at {} ms of host time in the Lull: {}",
                    self.elapsed_ms,
                    json::write(&answer)
                ),
            }),
        }
    }

    /// One timed call of `method` with `params`, as seat 0: the request is
    /// parsed before the clock starts, and the timed region is
    /// `Surface::call` alone. Answers the time and the call's `result`; an
    /// answer with no `result` is the gateway's refusal, an error.
    ///
    /// # Errors
    ///
    /// [`Error::Gateway`] for a refusal, naming the host time it came at, and
    /// as the clock report.
    pub fn time(&mut self, method: &str, params: &Json) -> Result<(u128, Json), Error> {
        self.tick()?;
        let params = json::write(params);
        let request = pharmakos_gateway::rpc::parse(&format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"{method}","params":{params}}}"#
        ))
        .map_err(|error| Error::Codec {
            what: format!("the bench's {method} request"),
            reason: error.title().to_string(),
        })?;
        let started = std::time::Instant::now();
        let answer = self.surface.call(Some(&self.token), &request, &Blind);
        let ns = started.elapsed().as_nanos();
        match answer.get("result") {
            Some(result) => Ok((ns, result.clone())),
            None => Err(Error::Gateway {
                during: "a planning call in the Lull",
                message: format!(
                    "{method} at {} ms of host time in the Lull: {}",
                    self.elapsed_ms,
                    json::write(&answer)
                ),
            }),
        }
    }
}
