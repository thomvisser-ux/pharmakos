// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Conditions and selectors: the read-only half of a decision.
//!
//! Spec section 10, "Conditions": `all` / `any` / `not` trees, **integer
//! comparisons only**, over the seat's own knowledge. Nothing here writes, and
//! nothing here reads a clock: `segment_elapsed` is game milliseconds counted
//! from the segment's own start tick.
//!
//! # Selectors resolve once, at step start
//!
//! [`resolve_beacon`] is the whole selector catalogue. Every arm of it — the
//! three ranked selectors **and a fixed `b_NN` id** — answers over the seat's
//! **living own** beacons and nothing else, because spec section 10's
//! conditions "read only the seat's knowledge store" and there is no knowledge
//! store to read another seat's beacon through. A ranked selector ties to the
//! lowest beacon id (item 62's convention, and AGENTS.md §4.6's "ties to the
//! lowest id"), and resolving to nothing is a step failure rather than a silent
//! skip. The executor calls it once, when a step starts, and pins the answer
//! for that step.
//!
//! What each selector ranks by:
//!
//! * `safest` — highest hit points as a whole percentage of the beacon's
//!   maximum. **PLACEHOLDER**: the word "safest" wants a threat term, and
//!   incoming threat does not exist until S2 brings combat; hit-point fraction
//!   is the part of "safe" this build can answer, and it is the same ranking
//!   the fixed reflex walks to. Owner, at S2, with the threat model.
//! * `weakest` — lowest hit points as a whole percentage, the beacon's own and
//!   not its structures'.
//! * `nearest` — least travel from the commander, measured as the **octile
//!   ground distance** at `locomotion.step_cost_cardinal` and
//!   `step_cost_diagonal`. That is the metric item 90's spawn-distance rule
//!   uses and it is a lower bound on any walked path. The abstract estimator
//!   ([`crate::pathing::estimate`]) would be exact, and it is deliberately not
//!   used here: it costs a graph search per selector inside a tick phase and
//!   takes a mutable borrow of the search scratch. A selector picks a beacon,
//!   and the ordering of a handful of own beacons by a lower bound is the same
//!   ordering in every case the skeleton can produce.

use crate::interpreter::{BeaconSpec, Cond, Filter};
use crate::math::fixed::Fx;
use crate::math::quantity::Hp;
use crate::math::quantity::{Ms, Tick};
use crate::tables::{BeaconId, SeatId};
use crate::world::World;

use super::state::{NO_TICK, PlanState};

/// One seat's read-only view of the world, for one decision.
#[derive(Clone, Copy, Debug)]
pub(crate) struct View<'a> {
    /// The world, to read and never to write.
    pub(crate) world: &'a World,
    /// Whose decision this is.
    pub(crate) seat: SeatId,
    /// That seat's row in the seat table.
    pub(crate) seat_index: usize,
    /// That seat's execution state.
    pub(crate) state: &'a PlanState,
}

impl View<'_> {
    /// The tick the decision is being taken on.
    pub(crate) fn tick(&self) -> Tick {
        self.world.tick()
    }

    /// The seat's commander, when it has a living one.
    pub(crate) fn commander(&self) -> Option<usize> {
        let commander = self.world.commander_of(self.seat);
        if !commander.is_some() {
            return None;
        }
        usize::try_from(commander.raw()).ok()
    }

    /// The commander's position, when it has one.
    pub(crate) fn commander_at(&self) -> Option<[Fx; 3]> {
        let index = self.commander()?;
        self.world.units().positions().get(index).copied()
    }

    /// The commander's hit points, or zero when it has none.
    pub(crate) fn commander_hp(&self) -> i32 {
        self.commander()
            .and_then(|index| self.world.units().hit_points().get(index).copied())
            .map_or(0, Hp::raw)
    }

    /// The commander's maximum hit points, from `commander.hp`.
    pub(crate) fn commander_max_hp(&self) -> i32 {
        self.world
            .rules()
            .message()
            .commander
            .as_ref()
            .and_then(|block| i32::try_from(block.hp).ok())
            .unwrap_or(0)
    }

    /// The commander's hit points as a whole percentage of its maximum.
    ///
    /// `100` when the table has no maximum to divide by, which is the reading
    /// that cannot make the reflex fire on a rules-table mistake.
    pub(crate) fn commander_hp_pct(&self) -> i64 {
        percent(
            i64::from(self.commander_hp()),
            i64::from(self.commander_max_hp()),
        )
    }

    /// How far into the segment the Push has run, in game milliseconds.
    pub(crate) fn segment_elapsed_ms(&self) -> i64 {
        let ran = self
            .world
            .tick()
            .since(self.world.match_state().segment_started());
        i64::from(Ms::from_ticks(ran).raw())
    }

    /// A beacon's row, when the id names one.
    pub(crate) fn beacon_row(&self, beacon: BeaconId) -> Option<usize> {
        let beacons = self.world.beacons();
        let count = usize::try_from(beacons.len()).unwrap_or(0);
        let index = usize::try_from(beacon.raw()).ok()?;
        // Rows are dense and in id order, so the id is the row — checked rather
        // than assumed, because a snapshot could in principle carry otherwise.
        if beacons.ids().get(index).copied() == Some(beacon.raw()) {
            return Some(index);
        }
        (0..count).find(|row| beacons.ids().get(*row).copied() == Some(beacon.raw()))
    }

    /// A beacon's maximum hit points.
    ///
    /// The seat's **lowest-id** beacon is its core (spec section 3: a seat's
    /// first beacon is its core), which is `beacon.core_hp`; every other is a
    /// placed beacon at `structures.beacon.hp`. The same rule the respawn point
    /// uses to find a core.
    pub(crate) fn beacon_max_hp(&self, row: usize) -> i64 {
        let rules = self.world.rules().message();
        let seat = self.world.beacons().seats().get(row).copied().unwrap_or(0);
        let is_core = self.core_row(seat) == Some(row);
        if is_core {
            return rules
                .beacon
                .as_ref()
                .map_or(0, |block| i64::from(block.core_hp));
        }
        rules
            .structures
            .as_ref()
            .and_then(|block| block.beacon)
            .map_or(0, |row| i64::from(row.hp))
    }

    /// The row of a seat's core: its lowest-id beacon.
    pub(crate) fn core_row(&self, seat: u8) -> Option<usize> {
        let beacons = self.world.beacons();
        let count = usize::try_from(beacons.len()).unwrap_or(0);
        let mut best: Option<(u32, usize)> = None;
        let mut row: usize = 0;
        while row < count {
            if beacons.seats().get(row).copied() == Some(seat) {
                let id = beacons.ids().get(row).copied().unwrap_or(u32::MAX);
                if best.is_none_or(|(found, _)| id < found) {
                    best = Some((id, row));
                }
            }
            row = row.saturating_add(1);
        }
        best.map(|(_, row)| row)
    }

    /// A beacon's hit points as a whole percentage of its maximum.
    pub(crate) fn beacon_hp_pct(&self, row: usize) -> i64 {
        let hp = self
            .world
            .beacons()
            .hit_points()
            .get(row)
            .map_or(0, |hp| i64::from(hp.raw()));
        percent(hp, self.beacon_max_hp(row))
    }

    /// Whether a beacon row belongs to this seat.
    pub(crate) fn owns(&self, row: usize) -> bool {
        self.world.beacons().seats().get(row).copied() == Some(self.seat.raw())
    }

    /// Whether a beacon is alive.
    pub(crate) fn beacon_alive(&self, row: usize) -> bool {
        self.world
            .beacons()
            .hit_points()
            .get(row)
            .is_some_and(|hp| hp.is_alive())
    }

    /// The seat's treasury in whole `$`.
    pub(crate) fn treasury(&self) -> i64 {
        self.world
            .seats()
            .treasuries()
            .get(self.seat_index)
            .map_or(0, |money| money.raw())
    }

    /// Supply minus draw, in whole `kW`.
    pub(crate) fn headroom(&self) -> i64 {
        let supply = self
            .world
            .seats()
            .supplies()
            .get(self.seat_index)
            .map_or(0, |kw| i64::from(kw.raw()));
        let draw = self
            .world
            .seats()
            .draws()
            .get(self.seat_index)
            .map_or(0, |kw| i64::from(kw.raw()));
        supply.saturating_sub(draw)
    }
}

/// `value * 100 / total`, saturating, with an empty total reading as full.
#[allow(
    clippy::integer_division,
    reason = "a whole percentage is what the predicate is defined over (spec section 10: integer comparisons only); the zero divisor is tested for first"
)]
fn percent(value: i64, total: i64) -> i64 {
    if total <= 0 {
        return 100;
    }
    value.saturating_mul(100) / total
}

/// Evaluate one condition tree.
pub(crate) fn evaluate(view: &View<'_>, condition: &Cond) -> bool {
    match condition {
        Cond::All(items) => items.iter().all(|item| evaluate(view, item)),
        Cond::Any(items) => items.iter().any(|item| evaluate(view, item)),
        Cond::Not(item) => !evaluate(view, item),
        Cond::CmdrHpPct(cmp, right) => cmp.holds(view.commander_hp_pct(), *right),
        Cond::CmdrTookDamageWithin(ms) => took_damage_within(view, *ms),
        Cond::CmdrDeaths(cmp, right) => cmp.holds(i64::from(view.state.deaths_match), *right),
        Cond::SegmentElapsed(cmp, right) => cmp.holds(view.segment_elapsed_ms(), *right),
        Cond::BeaconHpPct(spec, cmp, right) => resolve_beacon(view, *spec)
            .and_then(|beacon| view.beacon_row(beacon))
            .is_some_and(|row| cmp.holds(view.beacon_hp_pct(row), *right)),
        Cond::BeaconPowered(spec, powered) => resolve_beacon(view, *spec)
            .and_then(|beacon| view.beacon_row(beacon))
            .is_some_and(|row| {
                let dormant = view
                    .world
                    .beacons()
                    .dormant()
                    .get(row)
                    .copied()
                    .unwrap_or(false);
                dormant != *powered
            }),
        Cond::Treasury(cmp, right) => cmp.holds(view.treasury(), *right),
        Cond::KwHeadroom(cmp, right) => cmp.holds(view.headroom(), *right),
        Cond::StepReached(index) => {
            view.state.reached != super::state::NO_INDEX && *index <= view.state.reached
        }
        Cond::RuleFired(index) => {
            let at = usize::try_from(*index).unwrap_or(usize::MAX);
            view.state.fires.get(at).copied().unwrap_or(0) > 0
        }
    }
}

/// Whether the commander lost hit points within the last `ms` of game time.
fn took_damage_within(view: &View<'_>, ms: i32) -> bool {
    if view.state.last_damage == NO_TICK {
        return false;
    }
    let since = view.tick().raw().saturating_sub(view.state.last_damage);
    i64::from(Ms::from_ticks(since).raw()) <= i64::from(ms)
}

/// Resolve a beacon reference against the seat's own living beacons.
///
/// `None` is a **step failure** at the call site, never a silent skip (spec
/// section 10, "Late-bound selectors").
pub(crate) fn resolve_beacon(view: &View<'_>, spec: BeaconSpec) -> Option<BeaconId> {
    match spec {
        BeaconSpec::Id(id) => {
            let row = view.beacon_row(id)?;
            // **Own beacons only, fixed ids included.** Spec section 10 says
            // conditions read only the seat's knowledge store and lists "own
            // beacon" as the family; the verifier resolves a fixed `beacon_id`
            // against "a beacon the seat's snapshot holds", so that a saved
            // playbook carried into another match is told so "rather than
            // quietly pointed at somebody else's beacon" (`resolve.rs`). A
            // `b_NN` that names another seat's beacon would otherwise answer
            // `beacon_hp_pct` and `beacon_powered` from that seat's live state —
            // a knowledge store nobody built, read through the one selector
            // that skips ranking.
            if view.owns(row) && view.beacon_alive(row) {
                Some(id)
            } else {
                None
            }
        }
        BeaconSpec::Safest => rank(view, Filter::default(), Rank::Safest),
        BeaconSpec::Weakest(filter) => rank(view, filter, Rank::Weakest),
        BeaconSpec::Nearest(filter) => rank(view, filter, Rank::Nearest),
    }
}

/// How a selector orders the beacons that pass its filter.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Rank {
    /// Highest hit-point percentage first.
    Safest,
    /// Lowest hit-point percentage first.
    Weakest,
    /// Least octile travel from the commander first.
    Nearest,
}

/// The best own living beacon under `rank`, ties to the lowest beacon id.
///
/// One scan, no allocation and no sort: a tick allocates nothing (G3′ §9.17),
/// and a running minimum over `(key, id)` is a total order because the id is
/// unique (item 62).
fn rank(view: &View<'_>, filter: Filter, rank: Rank) -> Option<BeaconId> {
    let beacons = view.world.beacons();
    let count = usize::try_from(beacons.len()).unwrap_or(0);
    let from = view.commander_at();
    let mut best: Option<(i64, u32)> = None;
    let mut row: usize = 0;
    while row < count {
        let id = beacons.ids().get(row).copied().unwrap_or(u32::MAX);
        if beacons.seats().get(row).copied() != Some(view.seat.raw()) || !view.beacon_alive(row) {
            row = row.saturating_add(1);
            continue;
        }
        if let Some(wanted) = filter.mandate {
            let carried = beacons.mandates().get(row).copied().unwrap_or(0);
            if carried != wanted.id() {
                row = row.saturating_add(1);
                continue;
            }
        }
        let key = match rank {
            // A running *minimum* over the key, so "safest" negates the
            // percentage rather than needing a second comparison.
            Rank::Safest => view.beacon_hp_pct(row).saturating_neg(),
            Rank::Weakest => view.beacon_hp_pct(row),
            Rank::Nearest => match (from, beacons.positions().get(row).copied()) {
                (Some(a), Some(b)) => octile(view.world, a, b),
                _ => i64::MAX,
            },
        };
        if best.is_none_or(|(found, found_id)| (key, id) < (found, found_id)) {
            best = Some((key, id));
        }
        row = row.saturating_add(1);
    }
    best.map(|(_, id)| BeaconId::new(id))
}

/// The octile ground distance between two places, in path cost units.
///
/// `cardinal * (long - short) + diagonal * short` at the rules table's own step
/// costs (item 59's 10 / 14). A lower bound on any walked path, which is what
/// makes it an admissible stand-in for "least travel" — see the module docs.
pub(crate) fn octile(world: &World, a: [Fx; 3], b: [Fx; 3]) -> i64 {
    let axis = |point: &[Fx; 3], at: usize| -> i64 {
        point
            .get(at)
            .map_or(0, |value| i64::from(value.floor_voxels()))
    };
    let dx = axis(&a, 0).saturating_sub(axis(&b, 0)).saturating_abs();
    let dy = axis(&a, 1).saturating_sub(axis(&b, 1)).saturating_abs();
    let short = dx.min(dy);
    let long = dx.max(dy);
    let cardinal = i64::from(world.rules().step_cardinal());
    let diagonal = i64::from(world.rules().step_diagonal());
    cardinal
        .saturating_mul(long.saturating_sub(short))
        .saturating_add(diagonal.saturating_mul(short))
}

/// Whether two places are within `voxels` of each other, by squared distance.
///
/// Range checks use r², never a square root (AGENTS.md §4.2).
pub(crate) fn within(a: [Fx; 3], b: [Fx; 3], voxels: i32) -> bool {
    let radius = Fx::from_voxels(i16::try_from(voxels.max(0)).unwrap_or(i16::MAX));
    crate::math::fixed::Sq::between(a, b) <= crate::math::fixed::Sq::of_radius(radius)
}
