// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The built-in unit programs: move, build, mine, scout — and salvage beside
//! them (spec section 6, "Programs").
//!
//! # The lower of the two layers
//!
//! The mandate says **what** ([`crate::mandate`]); a program decides **how**,
//! every tick, under the mandate's orders. v1 ships one built-in program per
//! kind, native Rust, and the player edits none of them; the
//! `program {builtin|script}` seam stays reserved and unread (AGENTS.md §2).
//!
//! # Dormancy is the first question every program asks
//!
//! A dormant beacon powers down everything homed to it, and its bound units
//! "park where they stand at 0 kW" (item 10). So every program below starts by
//! checking its home beacon, and a parked unit stops walking rather than
//! finishing the leg it was on — the power went out, it did not change its
//! mind. A unit with no home at all ([`crate::tables::UnitTable::homes`]) is
//! harness scaffolding and no program drives it.
//!
//! # Nothing here reads a clock, and nothing here allocates
//!
//! Timed work — a mining drone's `economy.mining_ms_per_voxel`, a reclaim
//! drone's lift — is a **tick** written into
//! [`crate::tables::UnitTable::busy_until`], which is hashed state. The ore
//! search reads the footprint of the seam the home beacon holds and sorts its
//! candidates in a fixed array ([`crate::mining`]), so nothing allocates.

use crate::events::{Emission, EventKind};
use crate::knowledge::{AssetId, Position};
use crate::math::fixed::{Fx, Sq};
use crate::math::quantity::{Hp, Money, Ms};
use crate::seams::ProgramId;
use crate::tables::{BeaconId, NO_WORK, SeatId, UnitId, UnitKind};
use crate::world::World;

/// The seam a built-in unit program plugs into.
///
/// One implementation per [`UnitKind`], dispatched by [`program_for`]. The
/// trait exists for the reason [`crate::mandate::Mandate`] does: it is the
/// shape the `program {builtin|script}` oneof lands on when the script arm
/// opens, and it keeps "the player edits none of them" a property of the type
/// rather than of a convention.
pub trait Program {
    /// The reserved `program_id` seam's value for this program.
    ///
    /// PLACEHOLDER: the program ids are this build's own numbering — owner, at v1.2.
    /// The ids are this build's own numbering and nothing reads
    /// them — [`crate::seams::ProgramId`] says why. The catalogue becomes a
    /// contract when the script arm opens, which is **v1.2** and not a stage of
    /// this plan (owner, at v1.2).
    fn id(&self) -> ProgramId;

    /// Run one tick for one unit.
    fn run(&self, world: &mut World, unit: u32);
}

/// Walk home and idle there: what a unit whose mandate has no job for it does
/// (spec section 6, the Common row).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MoveProgram;

/// Raise the beacon's Build targets.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BuildProgram;

/// Dig the seam the beacon holds and carry the ore home ([`crate::mining`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MineProgram;

/// Probe the mandate's areas, then refresh the stalest sighting.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ScoutProgram;

/// Lift wrecks inside the beacon's sphere and carry the salvage home.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SalvageProgram;

/// The built-in program a unit kind runs.
///
/// **A unit's kind is its job** (decisions log item 127 (6), the register's
/// S1-36): the program is chosen from the kind alone and never from the home
/// beacon's writ, and the mandate layer governs spending, not behaviour. So
/// the starting mining drone, which the spec's starting force (item 90) homes
/// to a core on **Build**, keeps mining and earning; spec section 6's Common
/// row ("a unit the mandate has no job for ... idles at its beacon") is to
/// be reworded to match (item 127 (6)); the spec still carries the old line.
///
/// The commander has **none**, and that has a consequence worth saying out
/// loud: because no program drives it, **a brownout never parks the
/// commander** (item 127 (5), the register's S1-35). It is the one exception to
/// dormancy parking a beacon's units: it walks through a blackout and **draws
/// nothing while its home is dark**, because draw follows the home beacon and
/// not the unit's motion ([`crate::power`] counts a unit only while its home is
/// lit), and while its home is lit it draws `power.kw_per_unit` like any other
/// unit. A total blackout's headroom is 0 kW for exactly that reason.
///
/// That is the spec's own logic. A dormant beacon keeps "the seal, the sphere,
/// interfacing and recycling" lit off the key-core (item 10) precisely so that
/// a brownout is a setback rather than a lockout, and the only way a seat acts
/// on a shortfall is to walk the commander to a beacon and interface on site:
/// raise its Quartermaster priority, which is what spec section 14's safe
/// playbook does, or change its mandate, or recycle it. A commander that
/// parked when the lights went out could file none of that, and a blackout
/// would be a lockout the seat could not walk out of until the settlement paid
/// it.
///
/// **A raise pays off at once** (item 127 (7), the register's S1-22): raising
/// a dark beacon's priority re-applies the brownout order, so the beacon
/// relights at the next settle when shedding lit beacons of lower priority
/// covers its load, with draw no higher than supply after the swap, and those
/// beacons go dark in its place (see [`crate::power`], "A priority raise
/// re-applies the order"). When no such shed covers it, the raise moves the
/// beacon later in the brownout order and earlier in the revival order, and
/// the beacon waits for the revival margin.
///
/// Where the commander goes is the playbook's to say and nothing else's
/// (AGENTS.md §11: "no manual control" cuts the other way too — no program may
/// steer it either). A raider runs the move program until S2 gives it a
/// target.
#[must_use]
pub fn program_for(kind: UnitKind) -> Option<&'static dyn Program> {
    match kind {
        UnitKind::Commander => None,
        UnitKind::BuildDrone => Some(&BuildProgram),
        UnitKind::MiningDrone => Some(&MineProgram),
        UnitKind::RepairDrone => Some(&SalvageProgram),
        UnitKind::Scout => Some(&ScoutProgram),
        UnitKind::Raider => Some(&MoveProgram),
    }
}

/// Run every unit's program, in ascending unit row.
///
/// The order is a total order over the writes (AGENTS.md §4.6): a program
/// writes its own unit's columns, its home beacon's structures and its seat's
/// treasury, and two units of the same seat that both deliver ore on the same
/// tick are added in row order. A freed row has no hit points and no home, so
/// no program drives it.
pub(crate) fn run_all(world: &mut World) {
    let count = world.units().len();
    let mut unit: u32 = 0;
    while unit < count {
        let row = crate::math::index::row(unit);
        let kind = world
            .units()
            .kinds()
            .get(row)
            .copied()
            .and_then(UnitKind::from_id);
        if let Some(program) = kind.and_then(program_for)
            && is_drivable(world, row)
        {
            program.run(world, unit);
        }
        unit = unit.saturating_add(1);
    }
    if world.is_decision_tick() {
        crate::survey::record_sightings(world);
    }
}

/// What a program reads of its own unit, read once: every column of the row
/// together, so no later read can miss (S2's plan, E7).
#[derive(Clone, Copy, Debug)]
struct Me {
    seat: SeatId,
    home: BeaconId,
    here: [Fx; 3],
    carried: Money,
    busy: u32,
}

/// The program's view of the unit in `row`, or `None` for a row the table
/// does not hold.
fn me(world: &World, row: usize) -> Option<Me> {
    let units = world.units();
    Some(Me {
        seat: SeatId::new(units.seats().get(row).copied()?),
        home: units.homes().get(row).copied()?,
        here: units.positions().get(row).copied()?,
        carried: units.carrying().get(row).copied()?,
        busy: units.busy_until().get(row).copied()?,
    })
}

/// Whether a program should drive this unit at all: it is alive, it has a home,
/// and that home is awake.
///
/// A unit whose home beacon is dormant is **parked** rather than skipped: the
/// route is cleared so it stops where it stands, which is what 0 kW looks like
/// from outside (item 10). A home since freed is a stale reference, counted,
/// and drives nothing (S2's plan, E1): a unit is re-homed when its beacon dies,
/// so only a unit whose seat has no beacon left keeps a dead home, and that
/// seat's units disband on the same tick.
fn is_drivable(world: &mut World, row: usize) -> bool {
    if !world
        .units()
        .hit_points()
        .get(row)
        .is_some_and(|hp| hp.is_alive())
    {
        return false;
    }
    let Some(home) = world.units().homes().get(row).copied() else {
        return false;
    };
    match world.beacons().row_of(home) {
        Ok(_) => {}
        Err(crate::ids::RefError::Stale) => {
            world.stale_refs().stale(crate::ids::StaleRef::UnitHome);
            return false;
        }
        Err(crate::ids::RefError::Id(_) | crate::ids::RefError::NoSuchRow) => return false,
    }
    if crate::power::beacon_is_live(world, home) {
        return true;
    }
    if let Ok(unit) = u32::try_from(row) {
        world.park_unit(unit);
    }
    false
}

impl Program for MoveProgram {
    fn id(&self) -> ProgramId {
        ProgramId::new(1)
    }

    fn run(&self, world: &mut World, unit: u32) {
        idle_at_home(world, unit);
    }
}

impl Program for BuildProgram {
    fn id(&self) -> ProgramId {
        ProgramId::new(2)
    }

    fn run(&self, world: &mut World, unit: u32) {
        let row = crate::math::index::row(unit);
        let Some(me) = me(world, row) else {
            return;
        };
        let Some(structure) = crate::mandate::next_unfinished_target(world, me.home) else {
            idle_at_home(world, unit);
            return;
        };
        let Ok(at) = world.structures().row_of(structure) else {
            idle_at_home(world, unit);
            return;
        };
        let at = at.index();
        let (Some(site), Some(hp), Some(kind)) = (
            world.structures().positions().get(at).copied(),
            world.structures().hit_points().get(at).copied(),
            world.structures().kinds().get(at).copied(),
        ) else {
            idle_at_home(world, unit);
            return;
        };
        if !within(me.here, site, world.arrive_radius()) {
            world.send_unit(unit, site);
            return;
        }
        // On site: raise it. `structures.build_hp_per_second` divides the tick
        // rate exactly -- a table whose row does not is refused at load -- so
        // there is no accumulator and no rounding rule
        // (`RulesTable::build_hp_per_tick`).
        //
        // A wall has no structure row, and no row is one
        // ([`World::structure_max_hp`]).
        let Some(full) = crate::tables::StructureKind::from_id(kind)
            .and_then(|kind| world.structure_max_hp(kind))
        else {
            return;
        };
        let per_tick = world.rules().build_hp_per_tick();
        let grown = hp.raw().saturating_add(per_tick.raw()).min(full.raw());
        if let Some(slot) = world.structures_mut().hit_points_mut().get_mut(at) {
            *slot = Hp::new(grown);
        }
        if grown >= full.raw() {
            world.finish_structure(structure);
        }
    }
}

impl Program for MineProgram {
    fn id(&self) -> ProgramId {
        ProgramId::new(3)
    }

    fn run(&self, world: &mut World, unit: u32) {
        let row = crate::math::index::row(unit);
        let tick = world.tick();
        let Some(me) = me(world, row) else {
            return;
        };
        // 1. Finish the voxel the drone is digging, if its clock has run out.
        if me.busy != NO_WORK {
            if tick.raw() < me.busy {
                return;
            }
            finish_dig(world, unit);
            return;
        }
        // 2. Keep walking to the stand it already has, while the voxel beside
        //    that stand is still ore — one lookup instead of a search over the
        //    whole sphere, and the drone searches again only when the voxel it
        //    was going for has gone. The answer is the same either way,
        //    because the search is a pure function of the chunk store and the
        //    store changes only where a drone digs. A drone in transit cannot
        //    have become full — only a dig fills its hands, and a dig happens
        //    standing still — so the load test below loses nothing by sitting
        //    after this.
        if let Some(stand) = current_dig(world, row)
            && !within(me.here, stand, world.arrive_radius())
        {
            return;
        }
        // 3. The next voxel of the seam the home beacon holds, and the stand
        //    to dig it from (`crate::mining`: the four Mine settings and the
        //    pit-safe stand) -- which is also what decides whether the hands
        //    are full, because the load is a number of **voxels** and a
        //    voxel's worth depends on the seam's grade.
        let Some(dig) = world.next_dig(me.home) else {
            // No work left in the seam, or no seam held: take home whatever is
            // in hand rather than standing over it.
            if me.carried.raw() > 0 {
                deliver(world, unit, EventKind::OreDelivered);
            } else {
                idle_at_home(world, unit);
            }
            return;
        };
        // 4. A full load goes home. "Delivered ore is credited to the seat
        //    treasury immediately" (spec section 6), so the credit lands on
        //    arrival and not at the seam.
        if me.carried.raw() >= full_load(world, dig.ore) {
            deliver(world, unit, EventKind::OreDelivered);
            return;
        }
        let stand = dig.stand;
        if within(me.here, stand, world.arrive_radius()) {
            let ms = world.rules().mining_ms_per_voxel();
            let due = tick
                .raw()
                .saturating_add(Ms::new(ms).to_ticks_ceil().max(1));
            if let Some(slot) = world.units_mut().busy_until_mut().get_mut(row) {
                *slot = due;
            }
        } else {
            world.send_unit(unit, stand);
        }
    }
}

impl Program for SalvageProgram {
    fn id(&self) -> ProgramId {
        ProgramId::new(4)
    }

    fn run(&self, world: &mut World, unit: u32) {
        let row = crate::math::index::row(unit);
        let Some(me) = me(world, row) else {
            return;
        };
        if me.carried.raw() > 0 {
            deliver(world, unit, EventKind::SalvageDelivered);
            return;
        }
        // "Any seat's reclaim drones may salvage any wreck, whoever owned it,
        // within their beacon's sphere" (spec section 7). Nothing produces a
        // wreck at the skeleton — destruction is S2 — so this is the path a
        // wreck takes the day one exists, exercised by a test that makes one.
        let Some(wreck) = world.wreck_in_sphere(me.home) else {
            idle_at_home(world, unit);
            return;
        };
        let at = wreck.index();
        let (Some(site), Some(value)) = (
            world.wrecks().positions().get(at).copied(),
            world.wrecks().salvages().get(at).copied(),
        ) else {
            idle_at_home(world, unit);
            return;
        };
        if !within(me.here, site, world.arrive_radius()) {
            world.send_unit(unit, site);
            return;
        }
        if let Some(slot) = world.wrecks_mut().salvages_mut().get_mut(at) {
            *slot = Money::ZERO;
        }
        if let Some(slot) = world.units_mut().carrying_mut().get_mut(row) {
            *slot = value;
        }
    }
}

impl Program for ScoutProgram {
    fn id(&self) -> ProgramId {
        ProgramId::new(5)
    }

    fn run(&self, world: &mut World, unit: u32) {
        let row = crate::math::index::row(unit);
        let Some(me) = me(world, row) else {
            return;
        };
        // "Scouts scan unexplored ground inside these first, then re-check the
        // sightings closest to leaving the reach window" (spec section 6).
        // Being *inside* a probe area is what "scanned" means at the skeleton:
        // the sighting half is done by `crate::survey`, which runs every
        // decision tick wherever the scout happens to be standing. Nothing
        // records the visit, so a mandate with two or more areas works only
        // the first one its scout reaches — see `World::probe_to_enter`'s
        // PLACEHOLDER for what settling that costs and who settles it.
        if let Some(area) = world.probe_to_enter(me.home, me.here) {
            world.send_unit(unit, area);
            return;
        }
        if let Some(stale) = world.stalest_sighting_place(me.seat) {
            world.send_unit(unit, stale);
            return;
        }
        idle_at_home(world, unit);
    }
}

/// What a full load of ore is worth, in `$`, for a drone working the seam that
/// `ore` belongs to.
///
/// `economy.mining_carry_voxels` voxels **at that seam's own yield** (the row
/// S1's first contract pull request added for the skeleton's
/// `MINING_LOAD_VOXELS`, register S1-25, and S1's `mine` lane reads), so the
/// load is a number of voxels rather than a number of `$`: a rich seam fills the drone's
/// hands in the same number of digs as a lean one and is worth four times as
/// much when it gets home. That is what "richness sets the total yield" means
/// (item 22) — the richness is in what a voxel is worth, not in how much a
/// drone can hold. Measured against the next voxel rather than against a
/// counter on the unit row, which would be a fourth hashed column for a figure
/// the seam already carries; the two answers differ only for a drone whose
/// trip crosses from one grade to another, and [`crate::mapgen`] stamps a seam
/// at one grade.
///
/// A voxel that is not ore — the seam went while the drone walked — falls back
/// to the standard yield, so the test stays a number rather than a panic.
fn full_load(world: &World, ore: [i32; 3]) -> i64 {
    let per_voxel = world.ore_yield_at(ore).map_or_else(
        || {
            world
                .rules()
                .message()
                .economy
                .as_ref()
                .and_then(|economy| economy.ore_yield_per_voxel_dollars)
                .map_or(0, |by| i64::from(by.standard))
        },
        Money::raw,
    );
    let carry = world.rules().mining_carry_voxels().get();
    per_voxel.saturating_mul(i64::from(carry))
}

/// The eight columns around one, in the order [`current_dig`] looks.
const AROUND: [(i32, i32); 8] = [
    (1, 0),
    (0, 1),
    (-1, 0),
    (0, -1),
    (1, 1),
    (-1, 1),
    (-1, -1),
    (1, -1),
];

/// Where the drone is already walking to, when that place is still a legal
/// stand beside ore.
///
/// One lookup instead of a search over the whole sphere. The answer is the same
/// either way, because the search is a pure function of the chunk store and the
/// store changes only where a drone digs — so the only thing that can invalidate
/// a target is a dig, and a dig re-derives it.
fn current_dig(world: &World, row: usize) -> Option<[Fx; 3]> {
    let dest = world.units().destinations().get(row).copied()?;
    let [x, y, z] = dest;
    let (x, y) = (x.floor_voxels(), y.floor_voxels());
    let z = z.floor_voxels().saturating_sub(1);
    // Ore in any of the eight columns around the one the drone is walking to.
    for (dx, dy) in AROUND {
        let nx = x.saturating_add(dx);
        let ny = y.saturating_add(dy);
        let Some(top) = world.voxels().top_solid_z(nx, ny) else {
            continue;
        };
        if top >= 0
            && top.saturating_sub(z).abs() <= 1
            && world
                .voxels()
                .get([nx, ny, top])
                .and_then(crate::voxels::Material::ore_richness)
                .is_some()
        {
            return Some(dest);
        }
    }
    None
}

/// Where the beacon a unit is homed to stands, or `None` for a unit homed to
/// nobody or to a beacon the table no longer holds.
fn home_site(world: &World, me: &Me) -> Option<[Fx; 3]> {
    let at = world.beacons().row_of(me.home).ok()?;
    world.beacons().positions().get(at.index()).copied()
}

/// Walk to the home beacon and stop there.
fn idle_at_home(world: &mut World, unit: u32) {
    let row = crate::math::index::row(unit);
    let Some(me) = me(world, row) else {
        return;
    };
    let Some(site) = home_site(world, &me) else {
        return;
    };
    if within(me.here, site, world.arrive_radius()) {
        world.park_unit(unit);
    } else {
        world.send_unit(unit, site);
    }
}

/// Take the ore voxel the drone has been digging and put its yield in the
/// drone's hands.
fn finish_dig(world: &mut World, unit: u32) {
    let row = crate::math::index::row(unit);
    if let Some(slot) = world.units_mut().busy_until_mut().get_mut(row) {
        *slot = NO_WORK;
    }
    let Some(me) = me(world, row) else {
        return;
    };
    // Another dig in the same seam's box landed in the queue first this tick:
    // judge this one next tick, against the world with that one taken out, so
    // two digs are never pit-safe only one at a time and never take one voxel
    // twice (`crate::mining::dig_waits`).
    if crate::mining::dig_waits(world, me.home) {
        let next = world.tick().raw().saturating_add(1);
        if let Some(slot) = world.units_mut().busy_until_mut().get_mut(row) {
            *slot = next;
        }
        return;
    }
    let Some(crate::mining::Dig { ore, stand }) = world.next_dig(me.home) else {
        return;
    };
    if !within(me.here, stand, world.arrive_radius()) {
        return;
    }
    let Some(yield_dollars) = world.ore_yield_at(ore) else {
        return;
    };
    // The voxel goes through the edit queue like every other write, so its
    // chunk is marked and its digest refreshed in the same tick (AGENTS.md
    // §4.8's fourth place). The dig carries the mining drone's seat, because
    // a dig is among the edits whose seat collapse credits (decisions-log item
    // 144 (2)).
    match world.file_voxel_edit(crate::voxels::VoxelEdit::Set {
        at: ore,
        material: crate::voxels::Material::AIR,
        by: Some(me.seat),
    }) {
        Ok(()) => {}
        // Mining's back-off: a full queue, which the queue's sizing makes
        // unreachable in a legal world (S2's plan, E5), leaves the voxel for
        // the drone's next tick rather than dropping the dig.
        Err(crate::world::FileRefused::QueueFull { .. }) => return,
        // The dig names a voxel the store addressed, inside a tick: the
        // other two refusals cannot reach the tick's own door, and if one
        // did the voxel would still be there for the next tick.
        Err(crate::world::FileRefused::OffTheMap | crate::world::FileRefused::MidTick) => {
            return;
        }
    }
    if let Some(slot) = world.units_mut().carrying_mut().get_mut(row) {
        *slot = Money::new(me.carried.raw().saturating_add(yield_dollars.raw()));
    }
}

/// Carry a load home and credit it, or keep walking.
fn deliver(world: &mut World, unit: u32, kind: EventKind) {
    let row = crate::math::index::row(unit);
    let Some(me) = me(world, row) else {
        return;
    };
    let Some(site) = home_site(world, &me) else {
        return;
    };
    if !within(me.here, site, world.arrive_radius()) {
        world.send_unit(unit, site);
        return;
    }
    if me.carried.raw() <= 0 {
        return;
    }
    if let Some(slot) = world.units_mut().carrying_mut().get_mut(row) {
        *slot = Money::ZERO;
    }
    world.credit_treasury(me.seat, me.carried);
    let tick = world.tick();
    let id = world.units().ids().get(row).copied().map(UnitId::new);
    let mut emission = Emission::of(kind)
        .seat(me.seat)
        .at(Position::from_array(site))
        .value(me.carried.raw());
    if let Some(id) = id {
        emission = emission.subject_of(AssetId::of_unit(id));
    }
    world.emit(tick, emission);
}

/// Whether `here` is within `radius` voxels of `there`. Squared distances, no
/// square root (AGENTS.md §4.2).
#[must_use]
pub(crate) fn within(here: [Fx; 3], there: [Fx; 3], radius: Fx) -> bool {
    Sq::between(here, there) <= Sq::of_radius(radius)
}
