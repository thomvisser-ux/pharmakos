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
//! search walks the home beacon's sphere over the chunk store and keeps
//! nothing; the one place it could have allocated, the candidate list, does not
//! exist because the search keeps a single best voxel by a total key.

use crate::economy::{BUILD_HP_PER_SECOND, MINING_LOAD_VOXELS};
use crate::events::{Emission, EventKind};
use crate::knowledge::{AssetId, Position};
use crate::math::fixed::{Fx, Sq};
use crate::math::quantity::{Hp, Money, Ms};
use crate::seams::ProgramId;
use crate::tables::{BeaconId, NO_WORK, SeatId, StructureId, UnitId, UnitKind};
use crate::world::World;

/// PLACEHOLDER: how far a unit sees, in whole voxels.
///
/// A named constant rather than a rules-table row (AGENTS.md §12): spec section
/// 6 says a seat's reach is "your spheres, the live vision of your own units
/// and structures", and revision 3 of the table carries
/// `beacon.sphere_radius_voxels` for the first of those and nothing at all for
/// the second. Nothing in T14's acceptance asserts the number — Survey-lite
/// needs a scout to see *something*, not to see exactly this far — so the row
/// is proposed by the stage that makes reach matter: **S2**, where Defend's
/// engagement radius and Attack's freshness test both read it (owner, at S2, as
/// `units.vision_radius_voxels`).
///
/// Eight, a third of `beacon.sphere_radius_voxels`, so a scout adds reach by
/// moving rather than by standing.
pub const UNIT_VISION_RADIUS_VOXELS: i16 = 8;

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
    /// PLACEHOLDER: the ids are this build's own numbering and nothing reads
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

/// Dig the seam in the beacon's sphere and carry the ore home.
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
/// The commander has **none**, and that has a second consequence worth saying
/// out loud: because no program drives it, **a brownout never parks the
/// commander**. It is homed to its seat's core and draws `power.kw_per_unit`
/// like any other unit, but dormancy cannot stop it walking.
///
/// That is deliberate and it is the spec's own logic. A dormant beacon keeps
/// "the seal, the sphere, interfacing and recycling" lit off the key-core (item
/// 10) precisely so that a brownout is a setback rather than a lockout — and
/// the only way a seat acts on a shortfall is to walk the commander to a beacon
/// and interface on site: raise its Quartermaster priority, which is what spec
/// section 14's safe playbook does, or change its mandate, or recycle it. A
/// commander that parked when the lights went out could file none of that, and
/// a brownout would be a lockout by design.
///
/// What a raise does on the grid as built is narrower than "fixing" anything:
/// it moves the beacon later in the brownout order and earlier in the revival
/// order, and nothing else. It does not relight a dark beacon and it sheds no
/// lit one in its place, because the power phase sheds only while draw outruns
/// supply and a revival waits for `power.revive_margin_kw` whatever the
/// priority (see [`crate::power`]).
///
/// PLACEHOLDER: whether a priority raise re-applies the brownout order, so that
/// a raised dark beacon relights and a lit lower-priority one sheds in its
/// place. The spec is silent; decisions log item 113 (4) logged it. Owner, at
/// S1, with the grid.
///
/// Where the commander goes is the playbook's to say and nothing else's
/// (AGENTS.md §11: "no manual control" cuts the other way too — no program may
/// steer it either). A raider runs the move program until S2 gives it a
/// target.
///
/// PLACEHOLDER: the exception above is **argued, not decided**. Item 10 says a
/// dormant beacon parks "its bound units ... at 0 kW" without excepting the
/// commander, and the asymmetry this build ships is the part to put in front
/// of the owner: the one unit dormancy cannot stop is also the one the grid
/// still counts as powered down. Draw follows the home beacon, not the unit's
/// motion ([`crate::power`] counts a unit only while its home is lit), so while
/// the core is dark the commander walks and draws nothing, like everything
/// else homed there; while the core is lit it draws `power.kw_per_unit` like
/// any other unit. The two options are (a) as built — the commander walks
/// through a brownout, drawing nothing while its home is dark, so a seat can
/// always walk to a beacon and interface on site, whatever that then changes
/// (a total blackout's headroom is 0 kW only because the commander drops out
/// of the draw with the rest); and (b) the commander parks like every other
/// unit, which reads the rule literally and makes a total blackout a lockout
/// the seat cannot walk out of until the settlement pays it. Owner, at S1,
/// with the rest of the grid's tuning.
///
/// PLACEHOLDER: a unit's program is chosen from its **kind** alone and never
/// from its home beacon's writ, so the mandate layer governs spending and not
/// behaviour. Spec section 6's Common row — "a unit the mandate has no job for
/// keeps the common settings and idles at its beacon" — reads as though the
/// writ should gate the program, and under that reading the starting mining
/// drone would idle, because the spec's own starting force (item 90) homes it
/// to a core on **Build**. That is the tension, and it is why this build does
/// not guess: gating on the writ would leave the skeleton with no income at
/// all, and the starting force is the spec's, not this task's. Owner, at S1,
/// with the mandate's settings: either the writ gates the program (and the
/// starting core's writ or its drone changes), or a unit's kind is its job and
/// the mandate only decides what is bought.
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

/// Run every unit's program, in ascending unit id.
///
/// The order is a total order over the writes (AGENTS.md §4.6): a program
/// writes its own unit's columns, its home beacon's structures and its seat's
/// treasury, and two units of the same seat that both deliver ore on the same
/// tick are added in id order.
pub(crate) fn run_all(world: &mut World) {
    let count = world.units().len();
    let mut unit: u32 = 0;
    while unit < count {
        let row = usize::try_from(unit).unwrap_or(usize::MAX);
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

/// Whether a program should drive this unit at all: it is alive, it has a home,
/// and that home is awake.
///
/// A unit whose home beacon is dormant is **parked** rather than skipped: the
/// route is cleared so it stops where it stands, which is what 0 kW looks like
/// from outside (item 10).
fn is_drivable(world: &mut World, row: usize) -> bool {
    if !world
        .units()
        .hit_points()
        .get(row)
        .is_some_and(|hp| hp.is_alive())
    {
        return false;
    }
    let home = BeaconId::new(
        world
            .units()
            .homes()
            .get(row)
            .copied()
            .unwrap_or(BeaconId::NONE.raw()),
    );
    if !home.is_some() {
        return false;
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
        let row = usize::try_from(unit).unwrap_or(usize::MAX);
        let home = home_of(world, row);
        let Some(structure) = crate::mandate::next_unfinished_target(world, home) else {
            idle_at_home(world, unit);
            return;
        };
        let at = usize::try_from(structure).unwrap_or(usize::MAX);
        let Some(site) = world.structures().positions().get(at).copied() else {
            idle_at_home(world, unit);
            return;
        };
        let here = world
            .units()
            .positions()
            .get(row)
            .copied()
            .unwrap_or([Fx::ZERO; 3]);
        if !within(here, site, world.arrive_radius()) {
            world.send_unit(unit, site);
            return;
        }
        // On site: raise it. `BUILD_HP_PER_SECOND` divides the tick rate
        // exactly, so there is no accumulator and no rounding rule — see the
        // constant's own note.
        let kind = world
            .structures()
            .kinds()
            .get(at)
            .copied()
            .and_then(crate::tables::StructureKind::from_id);
        let Some(kind) = kind else {
            return;
        };
        let full = world.structure_max_hp(kind);
        let per_tick = Hp::new(hp_per_tick());
        let grown = world
            .structures()
            .hit_points()
            .get(at)
            .copied()
            .unwrap_or(Hp::ZERO)
            .raw()
            .saturating_add(per_tick.raw())
            .min(full.raw());
        if let Some(slot) = world.structures_mut().hit_points_mut().get_mut(at) {
            *slot = Hp::new(grown);
        }
        if grown >= full.raw() && full.raw() > 0 {
            world.finish_structure(StructureId::new(structure));
        }
    }
}

impl Program for MineProgram {
    fn id(&self) -> ProgramId {
        ProgramId::new(3)
    }

    fn run(&self, world: &mut World, unit: u32) {
        let row = usize::try_from(unit).unwrap_or(usize::MAX);
        let tick = world.tick();
        let home = home_of(world, row);
        // 1. Finish the voxel the drone is digging, if its clock has run out.
        let busy = world
            .units()
            .busy_until()
            .get(row)
            .copied()
            .unwrap_or(NO_WORK);
        if busy != NO_WORK {
            if tick.raw() < busy {
                return;
            }
            finish_dig(world, unit);
            return;
        }
        let carried = world
            .units()
            .carrying()
            .get(row)
            .copied()
            .unwrap_or(Money::ZERO);
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
            && !within(
                world
                    .units()
                    .positions()
                    .get(row)
                    .copied()
                    .unwrap_or([Fx::ZERO; 3]),
                stand,
                world.arrive_radius(),
            )
        {
            return;
        }
        // 3. The next voxel of the seam — which is also what decides whether
        //    the hands are full, because the load is a number of **voxels**
        //    and a voxel's worth depends on the seam's grade.
        let Some(ore) = world.ore_in_sphere(home) else {
            // No ore left in the sphere: take home whatever is in hand rather
            // than standing over it.
            if carried.raw() > 0 {
                deliver(world, unit, EventKind::OreDelivered);
            } else {
                idle_at_home(world, unit);
            }
            return;
        };
        // 4. A full load goes home. "Delivered ore is credited to the seat
        //    treasury immediately" (spec section 6), so the credit lands on
        //    arrival and not at the seam.
        if carried.raw() >= full_load(world, ore) {
            deliver(world, unit, EventKind::OreDelivered);
            return;
        }
        let Some(stand) = world.dig_stand(ore) else {
            // Ore, but none a drone can stand beside.
            if carried.raw() > 0 {
                deliver(world, unit, EventKind::OreDelivered);
            } else {
                idle_at_home(world, unit);
            }
            return;
        };
        let here = world
            .units()
            .positions()
            .get(row)
            .copied()
            .unwrap_or([Fx::ZERO; 3]);
        if within(here, stand, world.arrive_radius()) {
            let ms = world
                .rules()
                .message()
                .economy
                .as_ref()
                .map_or(0, |economy| economy.mining_ms_per_voxel);
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
        let row = usize::try_from(unit).unwrap_or(usize::MAX);
        let home = home_of(world, row);
        let carried = world
            .units()
            .carrying()
            .get(row)
            .copied()
            .unwrap_or(Money::ZERO);
        if carried.raw() > 0 {
            deliver(world, unit, EventKind::SalvageDelivered);
            return;
        }
        // "Any seat's reclaim drones may salvage any wreck, whoever owned it,
        // within their beacon's sphere" (spec section 7). Nothing produces a
        // wreck at the skeleton — destruction is S2 — so this is the path a
        // wreck takes the day one exists, exercised by a test that makes one.
        let Some(wreck) = world.wreck_in_sphere(home) else {
            idle_at_home(world, unit);
            return;
        };
        let at = usize::try_from(wreck).unwrap_or(usize::MAX);
        let Some(site) = world.wrecks().positions().get(at).copied() else {
            idle_at_home(world, unit);
            return;
        };
        let here = world
            .units()
            .positions()
            .get(row)
            .copied()
            .unwrap_or([Fx::ZERO; 3]);
        if !within(here, site, world.arrive_radius()) {
            world.send_unit(unit, site);
            return;
        }
        let value = world
            .wrecks()
            .salvages()
            .get(at)
            .copied()
            .unwrap_or(Money::ZERO);
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
        let row = usize::try_from(unit).unwrap_or(usize::MAX);
        let home = home_of(world, row);
        let here = world
            .units()
            .positions()
            .get(row)
            .copied()
            .unwrap_or([Fx::ZERO; 3]);
        // "Scouts scan unexplored ground inside these first, then re-check the
        // sightings closest to leaving the reach window" (spec section 6).
        // Being *inside* a probe area is what "scanned" means at the skeleton:
        // the sighting half is done by `crate::survey`, which runs every
        // decision tick wherever the scout happens to be standing. Nothing
        // records the visit, so a mandate with two or more areas works only
        // the first one its scout reaches — see `World::probe_to_enter`'s
        // PLACEHOLDER for what settling that costs and who settles it.
        if let Some(area) = world.probe_to_enter(home, here) {
            world.send_unit(unit, area);
            return;
        }
        let seat = SeatId::new(world.units().seats().get(row).copied().unwrap_or(0));
        if let Some(stale) = world.stalest_sighting_place(seat) {
            world.send_unit(unit, stale);
            return;
        }
        idle_at_home(world, unit);
    }
}

/// Hit points one build drone adds per tick.
///
/// [`BUILD_HP_PER_SECOND`] is chosen a multiple of the tick rate precisely so
/// that this is exact and there is no accumulator to keep hashed.
fn hp_per_tick() -> i32 {
    BUILD_HP_PER_SECOND
        .checked_div(i32::try_from(crate::math::quantity::TICK_HZ).unwrap_or(20))
        .unwrap_or(0)
        .max(1)
}

/// What a full load of ore is worth, in `$`, for a drone working the seam that
/// `ore` belongs to.
///
/// [`MINING_LOAD_VOXELS`] voxels **at that seam's own yield**, so the load is a
/// number of voxels rather than a number of `$`: a rich seam fills the drone's
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
    per_voxel.saturating_mul(i64::from(MINING_LOAD_VOXELS))
}

/// Where the drone is already walking to, when that place is still a legal
/// stand beside ore.
///
/// One lookup instead of a search over the whole sphere. The answer is the same
/// either way, because the search is a pure function of the chunk store and the
/// store changes only where a drone digs — so the only thing that can invalidate
/// a target is a dig, and a dig re-derives it.
fn current_dig(world: &World, row: usize) -> Option<[Fx; 3]> {
    let dest = world.units().destinations().get(row).copied()?;
    let x = dest.first()?.floor_voxels();
    let y = dest.get(1)?.floor_voxels();
    let z = dest.get(2)?.floor_voxels().saturating_sub(1);
    // Ore in any of the eight columns around the one the drone is walking to.
    for step in [
        [1_i32, 0_i32],
        [0, 1],
        [-1, 0],
        [0, -1],
        [1, 1],
        [-1, 1],
        [-1, -1],
        [1, -1],
    ] {
        let nx = x.saturating_add(step.first().copied().unwrap_or(0));
        let ny = y.saturating_add(step.get(1).copied().unwrap_or(0));
        let top = world.voxels().top_solid_z(nx, ny).unwrap_or(-1);
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

/// The beacon a unit is homed to.
fn home_of(world: &World, row: usize) -> BeaconId {
    BeaconId::new(
        world
            .units()
            .homes()
            .get(row)
            .copied()
            .unwrap_or(BeaconId::NONE.raw()),
    )
}

/// Walk to the home beacon and stop there.
fn idle_at_home(world: &mut World, unit: u32) {
    let row = usize::try_from(unit).unwrap_or(usize::MAX);
    let home = home_of(world, row);
    let Ok(at) = usize::try_from(home.raw()) else {
        return;
    };
    let Some(site) = world.beacons().positions().get(at).copied() else {
        return;
    };
    let here = world
        .units()
        .positions()
        .get(row)
        .copied()
        .unwrap_or([Fx::ZERO; 3]);
    if within(here, site, world.arrive_radius()) {
        world.park_unit(unit);
    } else {
        world.send_unit(unit, site);
    }
}

/// Take the ore voxel the drone has been digging and put its yield in the
/// drone's hands.
fn finish_dig(world: &mut World, unit: u32) {
    let row = usize::try_from(unit).unwrap_or(usize::MAX);
    if let Some(slot) = world.units_mut().busy_until_mut().get_mut(row) {
        *slot = NO_WORK;
    }
    let home = home_of(world, row);
    let Some(ore) = world.ore_in_sphere(home) else {
        return;
    };
    let Some(stand) = world.dig_stand(ore) else {
        return;
    };
    let here = world
        .units()
        .positions()
        .get(row)
        .copied()
        .unwrap_or([Fx::ZERO; 3]);
    if !within(here, stand, world.arrive_radius()) {
        return;
    }
    let Some(yield_dollars) = world.ore_yield_at(ore) else {
        return;
    };
    // The voxel goes through the edit queue like every other write, so its
    // chunk is marked and its digest refreshed in the same tick (AGENTS.md
    // §4.8's fourth place).
    if !world.request_voxel_edit(crate::voxels::VoxelEdit::Set {
        at: ore,
        material: crate::voxels::Material::AIR,
    }) {
        return;
    }
    let carried = world
        .units()
        .carrying()
        .get(row)
        .copied()
        .unwrap_or(Money::ZERO);
    if let Some(slot) = world.units_mut().carrying_mut().get_mut(row) {
        *slot = Money::new(carried.raw().saturating_add(yield_dollars.raw()));
    }
}

/// Carry a load home and credit it, or keep walking.
fn deliver(world: &mut World, unit: u32, kind: EventKind) {
    let row = usize::try_from(unit).unwrap_or(usize::MAX);
    let home = home_of(world, row);
    let Ok(at) = usize::try_from(home.raw()) else {
        return;
    };
    let Some(site) = world.beacons().positions().get(at).copied() else {
        return;
    };
    let here = world
        .units()
        .positions()
        .get(row)
        .copied()
        .unwrap_or([Fx::ZERO; 3]);
    if !within(here, site, world.arrive_radius()) {
        world.send_unit(unit, site);
        return;
    }
    let carried = world
        .units()
        .carrying()
        .get(row)
        .copied()
        .unwrap_or(Money::ZERO);
    if carried.raw() <= 0 {
        return;
    }
    if let Some(slot) = world.units_mut().carrying_mut().get_mut(row) {
        *slot = Money::ZERO;
    }
    let seat = SeatId::new(world.units().seats().get(row).copied().unwrap_or(0));
    world.credit_treasury(seat, carried);
    let tick = world.tick();
    world.emit(
        tick,
        Emission::of(kind)
            .seat(seat)
            .subject(AssetId::of_unit(UnitId::new(unit)))
            .at(Position::from_array(site))
            .value(carried.raw()),
    );
}

/// Whether `here` is within `radius` voxels of `there`. Squared distances, no
/// square root (AGENTS.md §4.2).
#[must_use]
pub(crate) fn within(here: [Fx; 3], there: [Fx; 3], radius: Fx) -> bool {
    Sq::between(here, there) <= Sq::of_radius(radius)
}
