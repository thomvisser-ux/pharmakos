// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `kW`: supply, draw, the brownout order and dormancy (spec sections 5 and 7;
//! decisions log items 10 and 90).
//!
//! # The beacon is the unit of power
//!
//! Item 10, in one paragraph. A dormant beacon powers down **everything homed
//! to it** — its fabricator, its structures, and its bound units, which park
//! where they stand at 0 kW. The seal, the sphere, interfacing and recycling
//! stay lit off the key-core, so a browned-out beacon can still be walked to
//! and changed; that is what keeps a brownout a setback rather than a lockout.
//! The core autocannon runs off the deep bore, **outside the grid**, so it
//! neither draws nor is ever shed.
//!
//! Supply is the core's deep-bore surplus plus one Generator per vent at the
//! vent's richness — the vent's heat is the limit, not the tap, so a second
//! Generator on a vent that is already tapped adds nothing ([`supply_of`]).
//! Draw is per fielded item: every unit `power.kw_per_unit`, every capability
//! structure its own row.
//!
//! # A beacon is net zero through its own key-core
//!
//! A live beacon draws its base (`power.beacon_base_draw_kw`), and its own
//! key-core supplies exactly that base (spec section 7's Power supply row;
//! decisions log section 2.3's key-core decision and item 90), so the two
//! cancel and **neither column carries either**: [`draw_of`] counts no
//! beacon's base, the core's included, and [`revive_cost`] does not start from
//! one. That is the net the tick-0 columns map generation writes already use
//! (`mapgen`'s `starting_draw`), so a seat's meter does not step at the first
//! settle, and placing a beacon costs the grid nothing: a beacon with nothing
//! homed to it is never shed by its own draw (decisions log item 113 (5)).
//!
//! PLACEHOLDER: the key-core is **netted out of draw** rather than shown as an
//! output in supply with the base shown in draw. Spec section 7 gives a
//! non-core key-core an output of "exactly its own beacon's base draw (net
//! zero)" and says nothing about which figures the meter shows; item 113 (5)
//! chose the net figures, as the tick-0 columns already are. The other reading
//! grows both columns by the base per live beacon and takes a dormant beacon
//! out of both. Owner, at S1, with the grid.
//!
//! # The order is fixed, and it is not the `$` order
//!
//! When draw outruns supply the Quartermaster **holds fabricator orders first**
//! ([`crate::economy::SingleTreasury`] refuses a request the headroom cannot
//! run), and only then does the grid brown out. Beacons go dormant in one
//! order and one order only:
//!
//! 1. **lowest priority** — the per-beacon low / normal / high knob, which is
//!    the one player lever and is `kW`-only;
//! 2. then **furthest from the core** (or from its former site, which is where
//!    the core's row still stands even when it is dead);
//! 3. then **the core last**, because a key-core always powers its own beacon
//!    and only the load homed to it can brown it out;
//! 4. ties by **ascending beacon id**, item 62's convention, so the key is
//!    total.
//!
//! A dormant beacon revives only once supply exceeds draw by
//! `power.revive_margin_kw` **with that beacon's own load added back**, so a
//! grid on the edge does not oscillate and domes do not flicker. Reviving walks
//! the same order backwards: the core first, then highest priority, nearest the
//! core, lowest id. A shed core brings its deep-bore surplus back with it, so
//! its revival is weighed with that surplus credited, and a core shed by the
//! load homed to it comes back once that load leaves the margin spare; the
//! surplus is lost for good only when the core is destroyed (spec section 5,
//! the Core row).
//!
//! A priority raise changes only these two orders: it makes a beacon shed
//! later and revive sooner. It does not relight a dark beacon and it sheds no
//! lit one in its place, because [`brown_out`] runs only while draw outruns
//! supply and a revival waits for the margin whatever the priority.
//!
//! Nothing in this module reads a clock and nothing in it divides: every figure
//! is a sum of integer `kW` rows.

use crate::events::{Emission, EventKind};
use crate::knowledge::{AssetId, Position};
use crate::math::fixed::Sq;
use crate::math::quantity::Kw;
use crate::rules::RulesTable;
use crate::tables::{BeaconId, SeatId, StructureKind};
use crate::voxels::Richness;
use crate::world::World;

/// The `kW` rows the power phase reads, taken once per tick.
///
/// A flat copy rather than a borrow of the rules table, because the phase
/// writes the tables the same table is reached through, and because reading
/// ten rows once a tick is cheaper than reading them once a beacon.
///
/// `power.beacon_base_draw_kw` is not among them: a live beacon's base is
/// netted out by its own key-core (see the module docs), so the phase never
/// reads it. The row stays in the table, where it is contract data.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PowerRules {
    /// `power.core_surplus_kw`.
    pub core_surplus: i32,
    /// `power.generator_output_kw` for a lean vent.
    pub generator_lean: i32,
    /// `power.generator_output_kw` for a standard vent.
    pub generator_standard: i32,
    /// `power.generator_output_kw` for a rich vent.
    pub generator_rich: i32,
    /// `power.kw_per_unit`.
    pub per_unit: i32,
    /// `power.revive_margin_kw`.
    pub revive_margin: i32,
    /// `structures.autocannon.draw_kw`, read only so the code can say out loud
    /// that it is never charged.
    pub autocannon_draw: i32,
    /// `structures.mortar.draw_kw`.
    pub mortar_draw: i32,
    /// `structures.survey_post.draw_kw`.
    pub survey_post_draw: i32,
    /// `structures.resonance_spire.draw_kw`.
    pub spire_draw: i32,
}

impl PowerRules {
    /// Read every `kW` row the phase needs.
    #[must_use]
    pub fn of(rules: &RulesTable) -> PowerRules {
        let message = rules.message();
        let power = message.power.as_ref();
        let by_richness = power.and_then(|block| block.generator_output_kw);
        let structures = message.structures.as_ref();
        PowerRules {
            core_surplus: power.map_or(0, |block| narrow(block.core_surplus_kw)),
            generator_lean: by_richness.map_or(0, |by| narrow(by.lean)),
            generator_standard: by_richness.map_or(0, |by| narrow(by.standard)),
            generator_rich: by_richness.map_or(0, |by| narrow(by.rich)),
            per_unit: power.map_or(0, |block| narrow(block.kw_per_unit)),
            revive_margin: power.map_or(0, |block| narrow(block.revive_margin_kw)),
            autocannon_draw: structures
                .and_then(|block| block.autocannon)
                .map_or(0, |row| narrow(row.draw_kw)),
            mortar_draw: structures
                .and_then(|block| block.mortar)
                .map_or(0, |row| narrow(row.draw_kw)),
            survey_post_draw: structures
                .and_then(|block| block.survey_post)
                .map_or(0, |row| narrow(row.draw_kw)),
            spire_draw: structures
                .and_then(|block| block.resonance_spire)
                .map_or(0, |row| narrow(row.draw_kw)),
        }
    }

    /// A Generator's output on a vent of this grade.
    #[must_use]
    pub const fn generator_output(&self, richness: Richness) -> i32 {
        match richness {
            Richness::Lean => self.generator_lean,
            Richness::Standard => self.generator_standard,
            Richness::Rich => self.generator_rich,
        }
    }

    /// One structure kind's draw.
    ///
    /// **A Generator draws nothing**: it is the thing that supplies. **An
    /// autocannon draws nothing either**, and that is the load-bearing line —
    /// it runs off the core's deep bore, outside the grid, which is the same
    /// sentence that makes it the one thing a brownout never sheds (spec
    /// section 5, the Core row). A wall has no draw because it has no row: its
    /// hit points are by material and it is priced per voxel.
    #[must_use]
    pub const fn structure_draw(&self, kind: StructureKind) -> i32 {
        match kind {
            StructureKind::Generator | StructureKind::Autocannon | StructureKind::Wall => 0,
            StructureKind::Mortar => self.mortar_draw,
            StructureKind::SurveyPost => self.survey_post_draw,
            StructureKind::ResonanceSpire => self.spire_draw,
        }
    }
}

/// A `uint32` row narrowed to the sim's signed `kW`, saturating rather than
/// wrapping: a table with a `kW` row above two billion is a table mistake and
/// saturation is the answer that stays deterministic (AGENTS.md §4.3).
fn narrow(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

/// Settle every seat's grid for this tick.
///
/// `order` is the caller's scratch buffer, so the phase allocates nothing
/// (G3′ §9.17): it holds the brownout order of one seat's beacons at a time.
pub(crate) fn settle(world: &mut World, order: &mut Vec<u32>) {
    let rules = PowerRules::of(world.rules());
    let seats = usize::try_from(world.seats().len()).unwrap_or(0);
    let mut index: usize = 0;
    while index < seats {
        let raw = world.seats().seats().get(index).copied().unwrap_or(0);
        let seat = SeatId::new(raw);
        if world.seats().is_alive(index) {
            brown_out(world, seat, rules, order);
            revive(world, seat, rules, order);
        }
        let supply = supply_of(world, seat, rules);
        let draw = draw_of(world, seat, rules);
        {
            let columns = world.seats_mut().power_columns();
            if let Some(slot) = columns.supply.get_mut(index) {
                *slot = Kw::new(supply);
            }
            if let Some(slot) = columns.draw.get_mut(index) {
                *slot = Kw::new(draw);
            }
        }
        index = index.saturating_add(1);
    }
}

/// How far apart two surface columns can stand and still be voxels of one heat
/// vent.
///
/// A vent is stamped as a square patch around one column
/// ([`crate::mapgen::VENT_PATCH_RADIUS`]), so two voxels of one patch differ by
/// at most its full span on each axis.
const VENT_PATCH_SPAN: i32 = 2 * crate::mapgen::VENT_PATCH_RADIUS;

/// Whether two standing points are taps on **one** heat vent.
///
/// Same grade and inside one patch's span on both horizontal axes. The grade
/// is part of the test because two patches of different richness are two
/// vents however close the generator laid them.
///
/// PLACEHOLDER: the span is the honest test only while a vent is the square
/// patch [`crate::mapgen`] stamps and two patches of the same grade are never
/// laid within it — which is true of every map the committed table generates,
/// and is what `STARTING_FEATURE_CLEARANCE` keeps true for contested features.
/// A vent identity carried on the world (a vent table, hashed) would make it
/// true by construction and would also let Survey report vents; that is the
/// map's own work. Owner, at S1, with the vent and seam tuning.
fn one_vent(world: &World, a: [crate::math::fixed::Fx; 3], b: [crate::math::fixed::Fx; 3]) -> bool {
    let (Some(grade_a), Some(grade_b)) = (vent_under(world, a), vent_under(world, b)) else {
        return false;
    };
    if grade_a != grade_b {
        return false;
    }
    let (Some(ax), Some(ay)) = (a.first(), a.get(1)) else {
        return false;
    };
    let (Some(bx), Some(by)) = (b.first(), b.get(1)) else {
        return false;
    };
    let dx = ax.floor_voxels().saturating_sub(bx.floor_voxels());
    let dy = ay.floor_voxels().saturating_sub(by.floor_voxels());
    dx.saturating_abs() <= VENT_PATCH_SPAN && dy.saturating_abs() <= VENT_PATCH_SPAN
}

/// Whether an earlier live Generator of `seat` already taps the vent under
/// `at`.
///
/// "Earlier" is the lower structure id, which is a total order (item 62), so
/// which tap counts does not depend on the order the table is walked in.
fn vent_already_tapped(
    world: &World,
    seat: SeatId,
    before: usize,
    at: [crate::math::fixed::Fx; 3],
) -> bool {
    let structures = world.structures();
    let mut row: usize = 0;
    while row < before {
        if structure_is_live(world, seat, row)
            && structures.kinds().get(row).copied() == Some(StructureKind::Generator.id())
            && let Some(other) = structures.positions().get(row).copied()
            && one_vent(world, at, other)
        {
            return true;
        }
        row = row.saturating_add(1);
    }
    false
}

/// The seat's supply: the core's deep-bore surplus plus every live Generator,
/// **one to a vent**.
///
/// A Generator that is still going up supplies nothing — "construction time is
/// hit points and spectacle" (item 23) — and one homed to a dormant beacon
/// supplies nothing either, because dormancy powers down everything homed to a
/// beacon and a tap that is powered down is not a tap.
///
/// A second Generator standing on a vent that is already tapped supplies
/// nothing either, and that is the whole of "one Generator per vent, output
/// set by the vent's richness, **because the vent's heat is the limit, not the
/// tap**" (spec section 5). The rule is enforced here, where the heat is
/// counted, rather than by refusing the second building: a seat is free to
/// stand two Generators on one vent and free to waste the `$`, and the grid is
/// not one kilowatt richer for it. The interface row that names a Build target
/// already refuses an anchor another target has claimed
/// ([`World::anchor_is_claimed`]), so the sealed-playbook route to stacking
/// them is shut as well.
#[must_use]
pub(crate) fn supply_of(world: &World, seat: SeatId, rules: PowerRules) -> i32 {
    let mut total: i32 = 0;
    let core = core_of(world, seat);
    if let Some(core) = core
        && beacon_is_live(world, core)
    {
        total = total.saturating_add(rules.core_surplus);
    }
    let structures = world.structures();
    let count = usize::try_from(structures.len()).unwrap_or(0);
    let mut row: usize = 0;
    while row < count {
        if structure_is_live(world, seat, row)
            && structures.kinds().get(row).copied() == Some(StructureKind::Generator.id())
        {
            let at = structures.positions().get(row).copied();
            let richness = at.and_then(|point| vent_under(world, point));
            if let (Some(grade), Some(point)) = (richness, at)
                && !vent_already_tapped(world, seat, row, point)
            {
                total = total.saturating_add(rules.generator_output(grade));
            }
        }
        row = row.saturating_add(1);
    }
    total
}

/// The seat's draw: every unit and every capability structure homed to a live
/// beacon.
///
/// **No beacon's base is in it**, the core's included: a live beacon's own
/// key-core supplies exactly its base, so the beacon is net zero and the
/// column shows the net (see the module docs and their PLACEHOLDER). A beacon
/// with nothing homed to it adds nothing here.
#[must_use]
pub(crate) fn draw_of(world: &World, seat: SeatId, rules: PowerRules) -> i32 {
    let mut total: i32 = unit_draw_of(world, seat, rules, None);
    let structures = world.structures();
    let count = usize::try_from(structures.len()).unwrap_or(0);
    let mut row: usize = 0;
    while row < count {
        if structure_is_live(world, seat, row) {
            let kind = structures
                .kinds()
                .get(row)
                .copied()
                .and_then(StructureKind::from_id);
            if let Some(kind) = kind {
                total = total.saturating_add(rules.structure_draw(kind));
            }
        }
        row = row.saturating_add(1);
    }
    total
}

/// What the seat's units draw, optionally only those homed to `only`.
fn unit_draw_of(world: &World, seat: SeatId, rules: PowerRules, only: Option<BeaconId>) -> i32 {
    let mut total: i32 = 0;
    let units = world.units();
    let count = usize::try_from(units.len()).unwrap_or(0);
    let mut row: usize = 0;
    while row < count {
        let mine = units.seats().get(row).copied() == Some(seat.raw());
        let alive = units.hit_points().get(row).is_some_and(|hp| hp.is_alive());
        let home = units
            .homes()
            .get(row)
            .copied()
            .unwrap_or(BeaconId::NONE.raw());
        let wanted = only.is_none_or(|id| home == id.raw());
        if mine && alive && wanted && beacon_is_live(world, BeaconId::new(home)) {
            total = total.saturating_add(rules.per_unit);
        }
        row = row.saturating_add(1);
    }
    total
}

/// The seat's core: its lowest-id beacon row, alive or not.
///
/// "Brownout distance is measured from the core **or its former site**" (spec
/// section 5), so a dead core still anchors the order — which is exactly what
/// keeping its row gives for free.
#[must_use]
pub(crate) fn core_of(world: &World, seat: SeatId) -> Option<BeaconId> {
    let beacons = world.beacons();
    let count = usize::try_from(beacons.len()).unwrap_or(0);
    let mut best: Option<u32> = None;
    let mut row: usize = 0;
    while row < count {
        if beacons.seats().get(row).copied() == Some(seat.raw()) {
            let id = beacons.ids().get(row).copied().unwrap_or(0);
            if best.is_none_or(|held| id < held) {
                best = Some(id);
            }
        }
        row = row.saturating_add(1);
    }
    best.map(BeaconId::new)
}

/// Whether a beacon is alive and awake.
#[must_use]
pub(crate) fn beacon_is_live(world: &World, beacon: BeaconId) -> bool {
    if !beacon.is_some() {
        return false;
    }
    let Ok(row) = usize::try_from(beacon.raw()) else {
        return false;
    };
    beacon_row_is_live(world, row)
}

/// Whether the beacon at `row` is alive and awake.
fn beacon_row_is_live(world: &World, row: usize) -> bool {
    let beacons = world.beacons();
    beacons
        .hit_points()
        .get(row)
        .is_some_and(|hp| hp.is_alive())
        && beacons.dormant().get(row).copied() == Some(false)
}

/// Whether the structure at `row` is `seat`'s, standing, finished, and homed to
/// a beacon that is alive and awake.
fn structure_is_live(world: &World, seat: SeatId, row: usize) -> bool {
    let structures = world.structures();
    if structures.seats().get(row).copied() != Some(seat.raw()) {
        return false;
    }
    if !structures
        .hit_points()
        .get(row)
        .is_some_and(|hp| hp.is_alive())
    {
        return false;
    }
    if structures.building().get(row).copied() != Some(false) {
        return false;
    }
    let home = structures
        .homes()
        .get(row)
        .copied()
        .unwrap_or(BeaconId::NONE.raw());
    beacon_is_live(world, BeaconId::new(home))
}

/// The grade of the heat vent a Generator stands on, or `None` when it stands
/// on no vent at all.
///
/// A structure's position is the standing point — one voxel above the column's
/// top solid voxel, the same convention a walker uses — so the vent is the
/// voxel underneath it. The voxel *at* the position is checked too, so that a
/// Generator placed by a test directly on the vent reads the same grade as one
/// the Build mandate raised.
#[must_use]
pub(crate) fn vent_under(world: &World, at: [crate::math::fixed::Fx; 3]) -> Option<Richness> {
    let x = at.first()?.floor_voxels();
    let y = at.get(1)?.floor_voxels();
    let z = at.get(2)?.floor_voxels();
    let below = world.voxels().get([x, y, z.saturating_sub(1)]);
    below
        .and_then(crate::voxels::Material::vent_richness)
        .or_else(|| {
            world
                .voxels()
                .get([x, y, z])
                .and_then(crate::voxels::Material::vent_richness)
        })
}

/// Shed beacons, in the fixed order, until draw no longer outruns supply.
///
/// The order is walked **without asking what a shed relieves**. With a
/// beacon's base netted out by its key-core, shedding a non-core beacon with
/// nothing homed to it relieves 0 kW, and it is still shed, so a deficit
/// darkens every such beacon before the core (decisions log item 113 (5)).
///
/// PLACEHOLDER: whether the order skips a beacon whose shed relieves nothing.
/// Spec section 5's Dormant row states the order with no such exception, and
/// item 113 (5) kept the order as written; the cost is that idle expansions go
/// dark in any deficit. Owner, at S1, with the grid.
///
/// The candidate list is built **once** and walked in order rather than
/// re-picked after every shed, which is what bounds the loop: a shed that made
/// the deficit worse — a beacon hosting a Generator goes dark and takes its
/// supply with it — cannot send the phase round again, and the order a reader
/// sees is the order the spec writes down.
fn brown_out(world: &mut World, seat: SeatId, rules: PowerRules, order: &mut Vec<u32>) {
    if supply_of(world, seat, rules) >= draw_of(world, seat, rules) {
        return;
    }
    shed_order(world, seat, order);
    let tick = world.tick();
    let mut at: usize = 0;
    while at < order.len() {
        if supply_of(world, seat, rules) >= draw_of(world, seat, rules) {
            break;
        }
        let Some(beacon) = order.get(at).copied() else {
            break;
        };
        at = at.saturating_add(1);
        let row = usize::try_from(beacon).unwrap_or(usize::MAX);
        if let Some(slot) = world.beacons_mut().dormant_mut().get_mut(row) {
            if *slot {
                continue;
            }
            *slot = true;
        } else {
            continue;
        }
        let place = world.beacons().positions().get(row).copied();
        let mut emission = Emission::of(EventKind::BeaconBrownedOut)
            .seat(seat)
            .subject(AssetId::of_beacon(BeaconId::new(beacon)));
        if let Some(point) = place {
            emission = emission.at(Position::from_array(point));
        }
        world.emit(tick, emission);
    }
    order.clear();
}

/// The brownout order: lowest priority, then furthest from the core, then the
/// core last, ties by ascending id.
///
/// Written into `order` as beacon ids. Only this seat's living, awake beacons
/// are candidates: a dead one is already off the grid and a dormant one is
/// already shed.
fn shed_order(world: &World, seat: SeatId, order: &mut Vec<u32>) {
    order.clear();
    let core = core_of(world, seat);
    let core_at = core
        .and_then(|id| usize::try_from(id.raw()).ok())
        .and_then(|row| world.beacons().positions().get(row).copied());
    let beacons = world.beacons();
    let count = usize::try_from(beacons.len()).unwrap_or(0);
    let mut row: usize = 0;
    while row < count {
        if beacons.seats().get(row).copied() == Some(seat.raw()) && beacon_row_is_live(world, row) {
            order.push(beacons.ids().get(row).copied().unwrap_or(0));
        }
        row = row.saturating_add(1);
    }
    // item 62: the key ends in the beacon id, which is unique, so the order is
    // total. `is_core` sorts last as a plain `bool`, which is the core's own
    // rule; the distance is negated by `Reverse` so the furthest sheds first.
    order.sort_unstable_by_key(|id| {
        let row = usize::try_from(*id).unwrap_or(usize::MAX);
        let priority = beacons.priorities().get(row).copied().unwrap_or(0);
        let at = beacons.positions().get(row).copied();
        let distance = match (at, core_at) {
            (Some(here), Some(there)) => Sq::between(here, there),
            _ => Sq::ZERO,
        };
        let is_core = core.is_some_and(|core| core.raw() == *id);
        (is_core, priority, core::cmp::Reverse(distance), *id)
    });
}

/// Bring dormant beacons back, best first, while the margin allows it.
///
/// The margin is spec section 5's: a beacon revives "only once supply exceeds
/// draw by a margin", and the margin is measured **with that beacon's own load
/// added back**, so a beacon cannot revive into a deficit it immediately causes
/// and then be shed again next tick. That is the whole anti-flicker rule, and
/// it holds only because [`revive_cost`] measures that load with the same
/// [`supply_of`] and [`draw_of`] the next settle sheds by.
fn revive(world: &mut World, seat: SeatId, rules: PowerRules, order: &mut Vec<u32>) {
    let mut guard: u32 = 0;
    let limit = world.beacons().len();
    while guard <= limit {
        guard = guard.saturating_add(1);
        revive_order(world, seat, order);
        let mut woke = false;
        let mut at: usize = 0;
        while at < order.len() {
            let Some(beacon) = order.get(at).copied() else {
                break;
            };
            at = at.saturating_add(1);
            let row = usize::try_from(beacon).unwrap_or(usize::MAX);
            let headroom =
                supply_of(world, seat, rules).saturating_sub(draw_of(world, seat, rules));
            let cost = revive_cost(world, seat, rules, BeaconId::new(beacon));
            if headroom.saturating_sub(cost) < rules.revive_margin {
                continue;
            }
            if let Some(slot) = world.beacons_mut().dormant_mut().get_mut(row) {
                *slot = false;
            }
            let tick = world.tick();
            let place = world.beacons().positions().get(row).copied();
            let mut emission = Emission::of(EventKind::BeaconRevived)
                .seat(seat)
                .subject(AssetId::of_beacon(BeaconId::new(beacon)));
            if let Some(point) = place {
                emission = emission.at(Position::from_array(point));
            }
            world.emit(tick, emission);
            woke = true;
            break;
        }
        if !woke {
            break;
        }
    }
    order.clear();
}

/// The order beacons come back in: the brownout order, backwards.
fn revive_order(world: &World, seat: SeatId, order: &mut Vec<u32>) {
    order.clear();
    let core = core_of(world, seat);
    let core_at = core
        .and_then(|id| usize::try_from(id.raw()).ok())
        .and_then(|row| world.beacons().positions().get(row).copied());
    let beacons = world.beacons();
    let count = usize::try_from(beacons.len()).unwrap_or(0);
    let mut row: usize = 0;
    while row < count {
        let mine = beacons.seats().get(row).copied() == Some(seat.raw());
        let alive = beacons
            .hit_points()
            .get(row)
            .is_some_and(|hp| hp.is_alive());
        let dormant = beacons.dormant().get(row).copied() == Some(true);
        if mine && alive && dormant {
            order.push(beacons.ids().get(row).copied().unwrap_or(0));
        }
        row = row.saturating_add(1);
    }
    // The core first, then the highest priority, then the nearest: every term
    // of the shed key, reversed.
    // item 62: the key still ends in the unique beacon id, ascending, so it is
    // total.
    order.sort_unstable_by_key(|id| {
        let row = usize::try_from(*id).unwrap_or(usize::MAX);
        let priority = beacons.priorities().get(row).copied().unwrap_or(0);
        let at = beacons.positions().get(row).copied();
        let distance = match (at, core_at) {
            (Some(here), Some(there)) => Sq::between(here, there),
            _ => Sq::ZERO,
        };
        let is_core = core.is_some_and(|core| core.raw() == *id);
        (!is_core, core::cmp::Reverse(priority), distance, *id)
    });
}

/// What reviving `beacon` would add to the seat's net draw, **measured rather
/// than modelled**: the seat's headroom now, less its headroom with `beacon`
/// awake.
///
/// The beacon is woken, [`supply_of`] and [`draw_of`] are read, and its flag
/// is put back before anything else runs, so every rule those two apply is
/// weighed once and in one place: the units and structures homed to it, the
/// Generators it would bring back **only where their vent is not already
/// tapped** (a Generator on a vent an earlier live one taps adds nothing, and
/// waking the earlier one moves the tap rather than adding one), and the
/// core's deep-bore surplus when `beacon` is the seat's core. A cost summed by
/// hand from the homed rows credited a Generator on a tapped vent with output
/// it would not add, so a beacon revived into a deficit and was shed again at
/// the next settle, every tick.
///
/// The beacon's own base is not in it: its key-core nets it out, as in
/// [`draw_of`]. The core's surplus counts for the same reason the Generators
/// do: [`supply_of`] counts it only while the core is live, so reviving the
/// core brings it back. Without that a shed core could never revive, since a
/// total blackout's headroom is 0 and the margin is positive, and spec section
/// 5 loses the surplus for good only when the core is **destroyed** (decisions
/// log item 113 (5)).
///
/// Nothing observes the world between the flip and the flip back: no event is
/// emitted, no hash is taken and no other seat's grid is read, so the
/// measurement leaves the world exactly as it found it.
fn revive_cost(world: &mut World, seat: SeatId, rules: PowerRules, beacon: BeaconId) -> i32 {
    let now = supply_of(world, seat, rules).saturating_sub(draw_of(world, seat, rules));
    let Ok(row) = usize::try_from(beacon.raw()) else {
        return 0;
    };
    let Some(was) = world.beacons().dormant().get(row).copied() else {
        return 0;
    };
    if let Some(slot) = world.beacons_mut().dormant_mut().get_mut(row) {
        *slot = false;
    }
    let awake = supply_of(world, seat, rules).saturating_sub(draw_of(world, seat, rules));
    if let Some(slot) = world.beacons_mut().dormant_mut().get_mut(row) {
        *slot = was;
    }
    now.saturating_sub(awake)
}
