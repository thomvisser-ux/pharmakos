// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Conditions and selectors: the read-only half of a decision.
//!
//! Spec section 10, "Conditions": `all` / `any` / `not` trees, **integer
//! comparisons only**, over the seat's own knowledge. Nothing here writes, and
//! nothing here reads a clock: `segment_elapsed` is game milliseconds counted
//! from the segment's own start tick.
//!
//! # When a selector resolves
//!
//! [`resolve_beacon`] is the whole selector catalogue. Every arm of it (the
//! three ranked selectors **and a fixed `b_NN` name**) answers over the seat's
//! **living own** beacons and nothing else, because spec section 10's
//! conditions "read only the seat's knowledge store" and there is no knowledge
//! store to read another seat's beacon through. A `b_NN` is the seat's own
//! beacon with that per-seat ordinal, and an `e_NN` -- another seat's beacon,
//! by the gateway's per-viewer handle -- resolves to nothing at all, so a
//! guess at another seat's beacon fails `no_target` exactly as an absent one
//! does (`docs/design/targeting.md`, "Failure"). A ranked selector ties to the
//! lowest beacon id (item 62's convention, and AGENTS.md section 4.6's "ties to
//! the lowest id"), and resolving to nothing is a step failure rather than a
//! silent skip.
//!
//! **A step resolves its selector once, when it starts, and pins the answer for
//! that step. A condition resolves its selector at every evaluation**:
//! `beacon_hp_pct` and `beacon_powered` name a beacon by a selector, and a
//! handler's condition, a `skip_if` guard or a `wait_until` re-reads it on
//! every decision it is evaluated on, so the beacon a condition is about can
//! change from one decision to the next. (This module's doc used to say every
//! selector resolves once, at step start, which was true of steps only; S1's
//! plan, the `fixs` lane, corrected it.)
//!
//! The same catalogue answers over a frozen planning snapshot through
//! [`resolve_beacon_in`], which is how the gateway's `estimate_route` sends a
//! selector leg where a decision would, without stepping anything.
//!
//! What each selector ranks by:
//!
//! * `safest` — S2's decisions 6 and 12 (`dfnd`): **while the seat's
//!   commander is below full hit points**, a beacon whose sphere holds one of
//!   the seat's live repair drones ranks ahead of one without (the rescue
//!   rule, so the fixed reflex and the safe playbook's shadowing walk a
//!   wounded commander to a beacon where a drone heals it); then the
//!   beacon's hit points as a whole percentage of its maximum **less the
//!   summed threat** of the enemy assets the seat sees inside its sphere,
//!   read lexicographically: the least threat first, then the highest
//!   percentage, so a beacon under any armed threat ranks behind every quiet
//!   one and two threatened beacons rank by their threat (an engineering
//!   reading of decision 12's "subtracts", which gives no scale). It is the
//!   same ranking the fixed reflex walks to.
//! * `most_threatened` — the largest summed threat of the enemy assets the
//!   seat sees inside the beacon's sphere (decision 12).
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
use crate::math::fixed::{Fx, Sq};
use crate::math::quantity::Hp;
use crate::math::quantity::{Ms, Tick};
use crate::rules::RulesTable;
use crate::snapshot::Snapshot;
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

/// The row a beacon the interpreter holds an id of lives in, checked by
/// generation: a beacon since freed is a stale reference, counted under
/// [`crate::ids::StaleRef::PinnedBeacon`], and answers no row (S2's plan, E1).
pub(crate) fn pinned_row(world: &crate::world::World, beacon: BeaconId) -> Option<usize> {
    match world.beacons().row_of(beacon) {
        Ok(row) => Some(row.index()),
        Err(crate::ids::RefError::Stale) => {
            world.stale_refs().stale(crate::ids::StaleRef::PinnedBeacon);
            None
        }
        Err(crate::ids::RefError::Id(_) | crate::ids::RefError::NoSuchRow) => None,
    }
}

impl View<'_> {
    /// The tick the decision is being taken on.
    pub(crate) fn tick(&self) -> Tick {
        self.world.tick()
    }

    /// The seat's commander, when it has a living one.
    pub(crate) fn commander(&self) -> Option<usize> {
        let commander = self.world.commander_of(self.seat)?;
        self.world
            .units()
            .row_of(commander)
            .ok()
            .map(crate::ids::Row::index)
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
            // E7: rule: a seat with no commander row has no hit points
            .map_or(0, Hp::raw)
    }

    /// The commander's maximum hit points, from `commander.hp`.
    ///
    /// Read typed at load, so a table missing it was refused there.
    pub(crate) fn commander_max_hp(&self) -> i32 {
        self.world
            .rules()
            .unit_hp(crate::tables::UnitKind::Commander)
            .raw()
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
    ///
    /// Checked by generation (S2's plan, E1): a pinned beacon whose row has
    /// since been freed or reused is a stale reference, counted, and names no
    /// row -- the beacon the step pinned is gone.
    pub(crate) fn beacon_row(&self, beacon: BeaconId) -> Option<usize> {
        pinned_row(self.world, beacon)
    }

    /// A beacon's hit points as a whole percentage of its maximum: the
    /// selector catalogue's own [`Board::row_hp_pct`], so a condition and a
    /// selector read one rule for a beacon's maximum (the core is the seat's
    /// `b_00`).
    pub(crate) fn beacon_hp_pct(&self, row: usize) -> i64 {
        self.row_hp_pct(row)
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
            // E7: cannot miss: `seat_index` is a row of the seat table the view was built for
            .map_or(0, |money| money.raw())
    }

    /// Supply minus draw, in whole `kW`.
    pub(crate) fn headroom(&self) -> i64 {
        let supply = self
            .world
            .seats()
            .supplies()
            .get(self.seat_index)
            // E7: cannot miss: as above
            .map_or(0, |kw| i64::from(kw.raw()));
        let draw = self
            .world
            .seats()
            .draws()
            .get(self.seat_index)
            // E7: cannot miss: as above
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
///
/// Every node evaluated is one of P1's evaluation units (S1's plan, section
/// 5): counted on the world's unhashed decision counter as it is reached, so a
/// short-circuited `all` or `any` counts only what it read.
pub(crate) fn evaluate(view: &View<'_>, condition: &Cond) -> bool {
    view.world.charge_decision(1);
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
        Cond::BeaconUnderAttack(spec, within_ms) => resolve_beacon(view, *spec)
            .and_then(|beacon| view.beacon_row(beacon))
            .is_some_and(|row| under_attack(view.world, row, *within_ms)),
        Cond::BeaconPowered(spec, powered) => resolve_beacon(view, *spec)
            .and_then(|beacon| view.beacon_row(beacon))
            .is_some_and(|row| {
                let dormant = view
                    .world
                    .beacons()
                    .dormant()
                    .get(row)
                    .copied()
                    // E7: cannot miss: `row` is a row `beacon_row` found in this table
                    .unwrap_or(false);
                dormant != *powered
            }),
        Cond::Treasury(cmp, right) => cmp.holds(view.treasury(), *right),
        Cond::KwHeadroom(cmp, right) => cmp.holds(view.headroom(), *right),
        Cond::StepReached(index) => {
            view.state.reached != super::state::NO_INDEX && *index <= view.state.reached
        }
        Cond::RuleFired(index) => {
            let at = crate::math::index::row(*index);
            // E7: rule: a handler index the plan does not hold has never fired, and the compile refuses one
            view.state.fires.get(at).copied().unwrap_or(0) > 0
        }
    }
}

/// Whether the beacon in `row` took damage from an enemy asset at any point
/// inside the last `within_ms` game milliseconds (`beacon_under_attack`, the
/// predicate's own window; decision 12): the tick its combat memory last
/// recorded an enemy's hit ([`crate::tables::Arms::attacked_at`]). The
/// beacon's own hit points only, as the predicate's message says; damage with
/// no enemy source (a host's order) records no attack.
pub(crate) fn under_attack(world: &World, row: usize, within_ms: i32) -> bool {
    let Some(arms) = world.beacons().arms().get(row) else {
        return false;
    };
    if arms.attacked_at == crate::tables::NEVER_ATTACKED {
        return false;
    }
    let since = world.tick().raw().saturating_sub(arms.attacked_at);
    i64::from(Ms::from_ticks(since).raw()) <= i64::from(within_ms)
}

/// Whether the commander lost hit points within the last `ms` of game time.
fn took_damage_within(view: &View<'_>, ms: i32) -> bool {
    if view.state.last_damage == NO_TICK {
        return false;
    }
    let since = view.tick().raw().saturating_sub(view.state.last_damage);
    i64::from(Ms::from_ticks(since).raw()) <= i64::from(ms)
}

/// What a selector reads: one seat's own beacons and its commander.
///
/// Two boards implement it, so the selector catalogue is written once: the
/// live world a decision reads ([`View`]), and the **frozen planning snapshot**
/// a gateway estimate or `resolve_refs` reads ([`SnapshotBoard`], behind
/// [`resolve_beacon_in`]). A plan-time answer and the run-time answer are then
/// the same function of the same kind of data, and cannot drift apart the way
/// `estimate_route`'s own "the seat's first beacon" did.
pub(crate) trait Board {
    /// Whose selector it is.
    fn seat(&self) -> SeatId;
    /// How many beacon rows there are.
    fn beacon_rows(&self) -> usize;
    /// One row's beacon id.
    fn row_id(&self, row: usize) -> Option<u32>;
    /// One row's seat.
    fn row_seat(&self, row: usize) -> Option<u8>;
    /// One row's per-seat ordinal, what its owner's `b_NN` names.
    fn row_ordinal(&self, row: usize) -> Option<u32>;
    /// One row's writ byte.
    fn row_mandate(&self, row: usize) -> u8;
    /// One row's anchor.
    fn row_at(&self, row: usize) -> Option<[Fx; 3]>;
    /// One row's hit points.
    fn row_hp(&self, row: usize) -> i64;
    /// The rules table the maxima and the step costs come from.
    fn rules(&self) -> &RulesTable;
    /// Where the seat's commander stands, when it has one.
    fn commander_at(&self) -> Option<[Fx; 3]>;
    /// The summed threat inside the sphere of the beacon in `row`, in
    /// thousandths of a hit point per second (decision 12).
    fn row_threat(&self, row: usize) -> u64;
    /// Whether one of the seat's live repair drones stands inside the sphere
    /// of the beacon in `row` (decision 6).
    fn row_repairs(&self, row: usize) -> bool;
    /// Whether the seat's commander is alive and below its full hit points
    /// (decision 6).
    fn commander_wounded(&self) -> bool;

    /// Count `units` of evaluation work: the live world counts a decision's
    /// selector candidates (P1's counted half); a frozen snapshot, which no
    /// decision reads, counts nothing.
    fn charge(&self, units: u32) {
        let _ = units;
    }

    /// Whether a row's beacon is alive.
    fn row_alive(&self, row: usize) -> bool {
        self.row_hp(row) > 0
    }

    /// The row of `seat`'s beacon with this ordinal, alive or dead.
    fn row_of_ordinal(&self, seat: SeatId, ordinal: u32) -> Option<usize> {
        (0..self.beacon_rows()).find(|row| {
            self.row_seat(*row) == Some(seat.raw()) && self.row_ordinal(*row) == Some(ordinal)
        })
    }

    /// A row's maximum hit points: `beacon.core_hp` for a seat's core, its
    /// `b_00` (spec section 3: the generator places it first), and
    /// `structures.beacon.hp` for any other.
    ///
    /// Both rows are read typed at load, so neither can be missing.
    fn row_max_hp(&self, row: usize) -> i64 {
        if self.row_ordinal(row) == Some(0) {
            return i64::from(self.rules().core_hp().raw());
        }
        i64::from(self.rules().beacon_row().hp.raw())
    }

    /// A row's hit points as a whole percentage of its maximum.
    fn row_hp_pct(&self, row: usize) -> i64 {
        percent(self.row_hp(row), self.row_max_hp(row))
    }
}

impl Board for View<'_> {
    fn seat(&self) -> SeatId {
        self.seat
    }
    fn beacon_rows(&self) -> usize {
        crate::math::index::row(self.world.beacons().len())
    }
    fn row_id(&self, row: usize) -> Option<u32> {
        self.world.beacons().ids().get(row).copied()
    }
    fn row_seat(&self, row: usize) -> Option<u8> {
        self.world.beacons().seats().get(row).copied()
    }
    fn row_ordinal(&self, row: usize) -> Option<u32> {
        self.world.beacons().ordinals().get(row).copied()
    }
    fn row_mandate(&self, row: usize) -> u8 {
        self.world
            .beacons()
            .mandates()
            .get(row)
            .copied()
            // E7: cannot miss: every caller's `row` is a row of the beacon table it read
            .unwrap_or(0)
    }
    fn row_at(&self, row: usize) -> Option<[Fx; 3]> {
        self.world.beacons().positions().get(row).copied()
    }
    fn row_hp(&self, row: usize) -> i64 {
        self.world
            .beacons()
            .hit_points()
            .get(row)
            // E7: cannot miss: every caller's `row` is a row of the beacon table it read
            .map_or(0, |hp| i64::from(hp.raw()))
    }
    fn rules(&self) -> &RulesTable {
        self.world.rules()
    }
    fn commander_at(&self) -> Option<[Fx; 3]> {
        View::commander_at(self)
    }
    fn row_threat(&self, row: usize) -> u64 {
        live_row_threat(self.world, self.seat, row)
    }
    fn row_repairs(&self, row: usize) -> bool {
        live_row_repairs(self.world, self.seat, row)
    }
    fn commander_wounded(&self) -> bool {
        live_commander_wounded(self.world, self.seat)
    }
    fn charge(&self, units: u32) {
        self.world.charge_decision(units);
    }
}

/// One seat's view of a frozen planning snapshot, for a selector.
struct SnapshotBoard<'a> {
    snapshot: &'a Snapshot,
    rules: &'a RulesTable,
    seat: SeatId,
    /// Where a `nearest` ranks from, when not from the commander.
    from: Option<[Fx; 3]>,
}

impl Board for SnapshotBoard<'_> {
    fn seat(&self) -> SeatId {
        self.seat
    }
    fn beacon_rows(&self) -> usize {
        self.snapshot.beacon_id.len()
    }
    fn row_id(&self, row: usize) -> Option<u32> {
        self.snapshot.beacon_id.get(row).copied()
    }
    fn row_seat(&self, row: usize) -> Option<u8> {
        self.snapshot.beacon_seat.get(row).copied()
    }
    fn row_ordinal(&self, row: usize) -> Option<u32> {
        self.snapshot.beacon_ordinal.get(row).copied()
    }
    fn row_mandate(&self, row: usize) -> u8 {
        // E7: cannot miss: every caller's `row` is a row of the snapshot's beacon columns
        self.snapshot.beacon_mandate.get(row).copied().unwrap_or(0)
    }
    fn row_at(&self, row: usize) -> Option<[Fx; 3]> {
        point_at(&self.snapshot.beacon_pos, row)
    }
    fn row_hp(&self, row: usize) -> i64 {
        self.snapshot
            .beacon_hp
            .get(row)
            // E7: cannot miss: every caller's `row` is a row of the snapshot's beacon columns
            .map_or(0, |hp| i64::from(*hp))
    }
    fn rules(&self) -> &RulesTable {
        self.rules
    }
    /// The seat's commander: its first commander row in unit-id order, the
    /// same rule the world's own index keeps (one per occupied seat, spec
    /// section 4). `None` when it is dead, as the live view answers. A caller
    /// that names the point a later step starts from
    /// ([`resolve_beacon_in`]'s `from`) is answered with that point instead:
    /// it is where the commander will stand when that step resolves.
    fn commander_at(&self) -> Option<[Fx; 3]> {
        if self.from.is_some() {
            return self.from;
        }
        let commander = crate::tables::UnitKind::Commander.id();
        let row = self
            .snapshot
            .unit_kind
            .iter()
            .zip(&self.snapshot.unit_seat)
            .position(|(kind, seat)| *kind == commander && *seat == self.seat.raw())?;
        if self.snapshot.unit_hp.get(row).is_none_or(|hp| *hp <= 0) {
            return None;
        }
        point_at(&self.snapshot.unit_pos, row)
    }
    /// Decision 12's sphere threat over the frozen snapshot: every armed
    /// enemy unit, finished structure and core inside the beacon's sphere.
    /// Inside an own live beacon's sphere is inside one of the seat's sensors
    /// (the sphere is one, [`crate::sight::SensorKind::Sphere`]), so under the
    /// radius-only model this is what the live world's seen-by test answers;
    /// `tests/defend.rs`' `the_frozen_snapshot_ranks_as_the_live_world_does`
    /// holds the two to each other. A core is the seat's beacon of ordinal 0
    /// (`b_00`, decisions-log item 127 (13)), the live world's
    /// `power::core_of`.
    fn row_threat(&self, row: usize) -> u64 {
        let Some(centre) = self.row_at(row) else {
            return 0;
        };
        let snapshot = self.snapshot;
        let rules = self.rules;
        let seat = self.seat.raw();
        let limit = Sq::of_radius(Fx::from_voxels(rules.sphere_radius_voxels()));
        let enemy = |owner: u8| owner != seat && SeatId::new(owner).is_some();
        let inside = |at: Option<[Fx; 3]>| at.is_some_and(|at| Sq::between(at, centre) <= limit);
        let mut sum: u64 = 0;
        let mut add = |weapon: Option<crate::arms::WeaponRow>| {
            sum = sum.saturating_add(crate::arms::Threat::of(weapon).milli_hp_per_second());
        };
        for (index, ((owner, kind), hp)) in snapshot
            .unit_seat
            .iter()
            .zip(&snapshot.unit_kind)
            .zip(&snapshot.unit_hp)
            .enumerate()
        {
            if *hp > 0 && enemy(*owner) && inside(point_at(&snapshot.unit_pos, index)) {
                add(crate::tables::UnitKind::from_id(*kind)
                    .and_then(|kind| rules.unit(kind).weapon));
            }
        }
        for (index, (((owner, kind), hp), building)) in snapshot
            .structure_seat
            .iter()
            .zip(&snapshot.structure_kind)
            .zip(&snapshot.structure_hp)
            .zip(&snapshot.structure_building)
            .enumerate()
        {
            if *hp > 0
                && *building == 0
                && enemy(*owner)
                && inside(point_at(&snapshot.structure_pos, index))
            {
                add(crate::tables::StructureKind::from_id(*kind)
                    .and_then(|kind| rules.structure(kind))
                    .and_then(|row| row.weapon));
            }
        }
        for (index, ((owner, hp), ordinal)) in snapshot
            .beacon_seat
            .iter()
            .zip(&snapshot.beacon_hp)
            .zip(&snapshot.beacon_ordinal)
            .enumerate()
        {
            if *hp > 0
                && *ordinal == 0
                && enemy(*owner)
                && inside(point_at(&snapshot.beacon_pos, index))
            {
                add(Some(rules.core_weapon()));
            }
        }
        sum
    }
    fn row_repairs(&self, row: usize) -> bool {
        let Some(centre) = self.row_at(row) else {
            return false;
        };
        let limit = Sq::of_radius(Fx::from_voxels(self.rules.sphere_radius_voxels()));
        let drone = crate::tables::UnitKind::RepairDrone.id();
        let snapshot = self.snapshot;
        snapshot
            .unit_kind
            .iter()
            .zip(&snapshot.unit_seat)
            .zip(&snapshot.unit_hp)
            .enumerate()
            .any(|(index, ((kind, owner), hp))| {
                *kind == drone
                    && *owner == self.seat.raw()
                    && *hp > 0
                    && point_at(&snapshot.unit_pos, index)
                        .is_some_and(|at| Sq::between(at, centre) <= limit)
            })
    }
    fn commander_wounded(&self) -> bool {
        let commander = crate::tables::UnitKind::Commander.id();
        let snapshot = self.snapshot;
        let full = self.rules.unit_hp(crate::tables::UnitKind::Commander).raw();
        snapshot
            .unit_kind
            .iter()
            .zip(&snapshot.unit_seat)
            .position(|(kind, seat)| *kind == commander && *seat == self.seat.raw())
            .and_then(|row| snapshot.unit_hp.get(row))
            .is_some_and(|hp| *hp > 0 && *hp < full)
    }
}

/// Point `row` of a snapshot's flattened `[x, y, z]` column.
fn point_at(axes: &[i32], row: usize) -> Option<[Fx; 3]> {
    let base = row.checked_mul(3)?;
    Some([
        Fx::from_raw(*axes.get(base)?),
        Fx::from_raw(*axes.get(base.checked_add(1)?)?),
        Fx::from_raw(*axes.get(base.checked_add(2)?)?),
    ])
}

/// Resolve a beacon reference against the seat's own living beacons, in the
/// live world a decision reads.
///
/// `None` is a **step failure** at the call site, never a silent skip (spec
/// section 10, "Late-bound selectors").
pub(crate) fn resolve_beacon(view: &View<'_>, spec: BeaconSpec) -> Option<BeaconId> {
    resolve_on(view, spec)
}

/// Resolve a beacon reference for `seat` over a **frozen planning snapshot**:
/// the answer [`resolve_beacon`] would give a decision taken on that world.
///
/// The one snapshot-level resolver the sim exports. The gateway's
/// `estimate_route` sends a selector leg where the run would (S1's plan, the
/// `fixs` lane: it used to send every ranked leg to the seat's first beacon),
/// and the targeting lane's `resolve_refs` reuses it. It reads, it never steps
/// (AGENTS.md section 3 rule 2): a selector is a ranking over what the
/// snapshot already holds.
///
/// `from` is where a `nearest` selector ranks from. At run time a selector
/// resolves when its step starts, from wherever the commander stands then,
/// which for any leg after the first is the end of the leg before it; so a
/// caller estimating a later leg passes that point, and `None` ranks from
/// where the snapshot's commander stands, as a route's first step does.
#[must_use]
pub fn resolve_beacon_in(
    snapshot: &Snapshot,
    rules: &RulesTable,
    seat: SeatId,
    spec: BeaconSpec,
    from: Option<[Fx; 3]>,
) -> Option<BeaconId> {
    resolve_on(
        &SnapshotBoard {
            snapshot,
            rules,
            seat,
            from,
        },
        spec,
    )
}

/// The selector catalogue, over any [`Board`].
fn resolve_on<B: Board>(board: &B, spec: BeaconSpec) -> Option<BeaconId> {
    match spec {
        BeaconSpec::Own(ordinal) => {
            // **Own beacons only, by construction.** A `b_NN` counts the
            // seat's own beacons (decisions-log item 127 (13)), so it cannot
            // name another seat's: the row it finds is the seat's, alive or
            // not, and a dead one resolves to nothing.
            let row = board.row_of_ordinal(board.seat(), ordinal)?;
            if board.row_alive(row) {
                board.row_id(row).map(BeaconId::new)
            } else {
                None
            }
        }
        // Another seat's beacon: never a target a seat's own orders resolve
        // to (see [`BeaconSpec::Foreign`]).
        BeaconSpec::Foreign(_) => None,
        BeaconSpec::Safest => rank(board, Filter::default(), Rank::Safest),
        BeaconSpec::Weakest(filter) => rank(board, filter, Rank::Weakest),
        BeaconSpec::Nearest(filter) => rank(board, filter, Rank::Nearest),
        BeaconSpec::MostThreatened(filter) => rank(board, filter, Rank::MostThreatened),
    }
}

/// Resolve a beacon reference for `seat` against the live world, outside a
/// decision: a Defend protect list's beacon, pinned when its row commits
/// (S2's `dfnd`). The same catalogue [`resolve_beacon`] answers with.
pub(crate) fn resolve_beacon_for(
    world: &World,
    seat: SeatId,
    spec: BeaconSpec,
) -> Option<BeaconId> {
    resolve_on(&SeatBoard { world, seat }, spec)
}

/// Resolve a beacon reference for `seat` against the live world, as a decision
/// taken now would (S2's `dfnd`): the read the Defend protect list pins at a
/// commit, offered to a host and to tests. It reads, it never steps.
#[must_use]
pub fn resolve_beacon_now(world: &World, seat: SeatId, spec: BeaconSpec) -> Option<BeaconId> {
    resolve_beacon_for(world, seat, spec)
}

/// Whether `beacon` took damage within the last `within_ms` game milliseconds:
/// the `beacon_under_attack` predicate as a decision reads it (S2's `dfnd`),
/// `false` for a beacon the world does not hold. It reads, it never steps.
#[must_use]
pub fn beacon_under_attack(world: &World, beacon: BeaconId, within_ms: i32) -> bool {
    world
        .beacons()
        .row_of(beacon)
        .is_ok_and(|row| under_attack(world, row.index(), within_ms))
}

/// One seat's view of the live world, for a selector read outside a decision.
struct SeatBoard<'a> {
    world: &'a World,
    seat: SeatId,
}

impl Board for SeatBoard<'_> {
    fn seat(&self) -> SeatId {
        self.seat
    }
    fn beacon_rows(&self) -> usize {
        crate::math::index::row(self.world.beacons().len())
    }
    fn row_id(&self, row: usize) -> Option<u32> {
        self.world.beacons().ids().get(row).copied()
    }
    fn row_seat(&self, row: usize) -> Option<u8> {
        self.world.beacons().seats().get(row).copied()
    }
    fn row_ordinal(&self, row: usize) -> Option<u32> {
        self.world.beacons().ordinals().get(row).copied()
    }
    fn row_mandate(&self, row: usize) -> u8 {
        live_row_mandate(self.world, row)
    }
    fn row_at(&self, row: usize) -> Option<[Fx; 3]> {
        self.world.beacons().positions().get(row).copied()
    }
    fn row_hp(&self, row: usize) -> i64 {
        live_row_hp(self.world, row)
    }
    fn rules(&self) -> &RulesTable {
        self.world.rules()
    }
    fn commander_at(&self) -> Option<[Fx; 3]> {
        live_commander(self.world, self.seat).map(|(at, _)| at)
    }
    fn row_threat(&self, row: usize) -> u64 {
        live_row_threat(self.world, self.seat, row)
    }
    fn row_repairs(&self, row: usize) -> bool {
        live_row_repairs(self.world, self.seat, row)
    }
    fn commander_wounded(&self) -> bool {
        live_commander_wounded(self.world, self.seat)
    }
}

/// A live beacon row's writ byte.
fn live_row_mandate(world: &World, row: usize) -> u8 {
    world
        .beacons()
        .mandates()
        .get(row)
        .copied()
        // E7: cannot miss: every caller's `row` is a row of the beacon table it read
        .unwrap_or(0)
}

/// A live beacon row's hit points.
fn live_row_hp(world: &World, row: usize) -> i64 {
    world
        .beacons()
        .hit_points()
        .get(row)
        // E7: cannot miss: every caller's `row` is a row of the beacon table it read
        .map_or(0, |hp| i64::from(hp.raw()))
}

/// Where `seat`'s living commander stands, and its hit points.
fn live_commander(world: &World, seat: SeatId) -> Option<([Fx; 3], Hp)> {
    let id = world.commander_of(seat)?;
    let index = world.units().row_of(id).ok()?.index();
    let hp = world.units().hit_points().get(index).copied()?;
    if !hp.is_alive() {
        return None;
    }
    Some((world.units().positions().get(index).copied()?, hp))
}

/// Decision 6's "below full HP", over the live world.
fn live_commander_wounded(world: &World, seat: SeatId) -> bool {
    live_commander(world, seat)
        .is_some_and(|(_, hp)| hp < world.rules().unit_hp(crate::tables::UnitKind::Commander))
}

/// Decision 12's sphere threat for a live beacon row.
fn live_row_threat(world: &World, seat: SeatId, row: usize) -> u64 {
    world
        .beacons()
        .positions()
        .get(row)
        .copied()
        // E7: cannot miss: every caller's `row` is a row of the beacon table it read
        .map_or(0, |centre| {
            crate::defend::sphere_threat(world, seat, centre)
        })
}

/// Decision 6's repair term for a live beacon row.
fn live_row_repairs(world: &World, seat: SeatId, row: usize) -> bool {
    world
        .beacons()
        .positions()
        .get(row)
        .copied()
        .is_some_and(|centre| crate::defend::repairs_in_sphere(world, seat, centre))
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
    /// Largest summed threat inside the sphere first (decision 12).
    MostThreatened,
}

/// The best own living beacon under `rank`, ties to the lowest beacon id.
///
/// One scan, no allocation and no sort: a tick allocates nothing (G3′ §9.17),
/// and a running minimum over `(key, id)` is a total order because the id is
/// unique (item 62).
fn rank<B: Board>(board: &B, filter: Filter, rank: Rank) -> Option<BeaconId> {
    let count = board.beacon_rows();
    let from = board.commander_at();
    let cardinal = i64::from(board.rules().step_cardinal());
    let diagonal = i64::from(board.rules().step_diagonal());
    // Decision 6: read once per ranking, not once per beacon.
    let rescue = rank == Rank::Safest && board.commander_wounded();
    let mut best: Option<((i64, i64, i64), u32)> = None;
    let mut row: usize = 0;
    while row < count {
        board.charge(1);
        let id = board.row_id(row).unwrap_or(u32::MAX);
        if board.row_seat(row) != Some(board.seat().raw()) || !board.row_alive(row) {
            row = row.saturating_add(1);
            continue;
        }
        if let Some(wanted) = filter.mandate
            && board.row_mandate(row) != wanted.id()
        {
            row = row.saturating_add(1);
            continue;
        }
        // E7: rule: a summed threat past `i64` is a threat no beacon outranks
        let threat = |row: usize| i64::try_from(board.row_threat(row)).unwrap_or(i64::MAX);
        let key = match rank {
            // A running *minimum* over the key, so "safest" ranks a beacon
            // with a repair drone first while the commander is wounded, then
            // the least threat, then the highest percentage (decisions 6 and
            // 12): the threat subtracted from the hit-point rank, ordered so
            // that any threat outweighs any percentage, whatever its size.
            Rank::Safest => (
                i64::from(rescue && !board.row_repairs(row)),
                threat(row),
                board.row_hp_pct(row).saturating_neg(),
            ),
            Rank::Weakest => (0, 0, board.row_hp_pct(row)),
            Rank::Nearest => (
                0,
                0,
                match (from, board.row_at(row)) {
                    (Some(a), Some(b)) => octile(cardinal, diagonal, a, b),
                    _ => i64::MAX,
                },
            ),
            Rank::MostThreatened => (0, threat(row).saturating_neg(), 0),
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
/// makes it an admissible stand-in for "least travel" (see the module docs).
fn octile(cardinal: i64, diagonal: i64, a: [Fx; 3], b: [Fx; 3]) -> i64 {
    let axis = |point: &[Fx; 3], at: usize| -> i64 {
        let [east, north, up] = *point;
        let value = match at {
            0 => east,
            1 => north,
            _ => up,
        };
        i64::from(value.floor_voxels())
    };
    let dx = axis(&a, 0).saturating_sub(axis(&b, 0)).saturating_abs();
    let dy = axis(&a, 1).saturating_sub(axis(&b, 1)).saturating_abs();
    let short = dx.min(dy);
    let long = dx.max(dy);
    cardinal
        .saturating_mul(long.saturating_sub(short))
        .saturating_add(diagonal.saturating_mul(short))
}
