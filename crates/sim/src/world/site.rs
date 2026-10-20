// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Build's S2 work: the base kit's walls, terraform, repair and salvage
//! (S2's plan, task `site`; spec sections 4, 6, 7, 8 and 9; decisions-log
//! items 128 (d) and 144 (5); the plan's decisions 10, 15, 26, 38, 40, 42 and
//! 43).
//!
//! # What a Build beacon does in S2
//!
//! * **The base kit.** `autocannon` and `mortar` are structures a Build target
//!   names and the Quartermaster buys whole, as the Generator is. A `wall` is
//!   a box of voxels: every column of its box is filled from the ground's top
//!   up to the box's top, one voxel at a time, so no wall voxel has air under
//!   it (decision 39), each voxel at `structures.wall_cost_per_voxel` and each
//!   edit carrying the building seat ([`crate::voxels::VoxelEdit`]). Where it
//!   stands is checked against the ground when its row commits
//!   ([`check_wall`]): the verifier sees no terrain, so the ground-dependent
//!   halves of the box's checks are the sim's (decisions-log item 146 (4)).
//! * **Terraform** (item 144 (5)). Before a structure target is raised its
//!   drone levels the target's footprint, its anchor column, to the anchor's
//!   height: `FILL` raises it, `DIG` lowers it, `BOTH` both. `FILL` and `BOTH`
//!   also refill craters and trenches inside the beacon's sphere back to the
//!   generated surface ([`crate::terrain`]). A refill fills air only, so a
//!   cracked voxel, which is solid, stays cracked (decision 10). Every voxel
//!   costs the wall's price, and goes through the voxel queue like every other
//!   write (AGENTS.md section 4.8).
//! * **Repair** (decisions 38, 42 and 43). A repair drone repairs an own
//!   structure inside its beacon's sphere once it is below the beacon's
//!   `repair_threshold_pct` (0: never), every own unit there and the commander
//!   below full, and restores a cracked own wall voxel to whole; then it
//!   salvages any seat's wrecks and ruins inside the sphere for `$`.
//! * **Rebuild destroyed** (item 128 (d)): a destroyed ordinary target is
//!   bought again when `rebuild_destroyed` is set; a capability structure
//!   never is (spec section 8).
//!
//! # How work is paid for: the Quartermaster, in advance
//!
//! Spec section 7 puts repair second on the Quartermaster's ladder, and
//! decision 43 files it "as a spend request under the Repair band, the drone
//! waiting while it is unfilled". A request is filled on a decision tick, one
//! per beacon, while repair and voxel work happen tick by tick. So a beacon
//! buys its drones' work **in advance**: when the work its drones stand at
//! would outrun what is already paid for before the next decision tick, its
//! Build mandate asks for the price of five seconds of that work ([`TOP_UP_TICKS`]), in the Repair
//! band for repair and in the Build band for voxel work, and the Quartermaster
//! moves it from the treasury onto the beacon ([`BeaconSite`]'s two funds).
//! The drones draw on it as each step or voxel is done, and a drone whose step
//! the fund cannot cover waits. Nothing is spent outside the Quartermaster,
//! and nothing is paid that the ladder did not approve.
//!
//! What is paid stays the seat's: it counts in held value
//! ([`World::held_value`]) as paid work, "paid means yours" (item 23), and it
//! goes with the beacon when the beacon dies, as a structure going up does. A
//! mandate switch and a recycle give it back to the treasury whole
//! ([`World::return_site_funds`]), since the drones it paid for no longer do
//! that work. Every writ asks for its repair drones' top-ups, since a drone
//! keeps its job when it is re-homed; only Build fabricates them.
//!
//! **The price of repair** is decision 43's, as the verification review
//! corrected it: the cost to full at `h` hit points is `ceil((max - h) * cost
//! / max)`, and a step from `h` to `h'` charges the cost to full before it
//! less the cost to full after it ([`repair_charge`]), so one repair's charges
//! telescope and a repair from one hit point to full costs at most the build.
//! The commander has no build cost and is repaired free.
//!
//! # Which drone does which voxel
//!
//! A voxel job is one drone's: two drones on one voxel would pay for it twice.
//! The jobs are a pure function of the world, recomputed every tick, so a
//! drone keeps no job of its own. Each voxel job belongs to one **class**, its
//! column's, and a drone works only the jobs of its class: the drone's rank
//! among its beacon's drones of its kind, by row, modulo their count
//! ([`class_of`]). A wall's columns are spread over the drones that way, and a
//! column's voxels all belong to one drone, which fills it from the bottom. A
//! structure being raised is every drone's.
//!
//! # Nothing here allocates, and nothing reads a clock
//!
//! Timed work is a tick written into the unit's `busy_until`, as a mining
//! drone's is ([`crate::programs`]), and every search walks the tables in
//! place.

use crate::events::{Emission, EventKind};
use crate::ids::Row;
use crate::knowledge::{AssetId, Position};
use crate::math::fixed::{Fx, Sq};
use crate::math::quantity::{Hp, Money, Ms, TICK_HZ};
use crate::rules::RulesError;
use crate::tables::{
    BeaconId, ColumnBox, NO_WALL, NO_WORK, SeatId, StructureId, StructureKind, TargetKind, UnitId,
    UnitKind,
};
use crate::voxels::{Material, VoxelEdit};
use crate::world::World;
use pharmakos_proto::gp;

/// The rows the site's work reads, typed at load (S2's plan, decision 17's
/// proposals in `rules/rules.v1.json`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SiteRows {
    /// `combat.repair_hp_per_second` over the tick rate: the hit points one
    /// working repair drone restores a tick. Refused at load unless a positive
    /// multiple of the tick rate, as `structures.build_hp_per_second` is, so
    /// repair needs no accumulator and no rounding rule.
    pub repair_hp_per_tick: Hp,
    /// `combat.terraform_ms_per_voxel` in ticks, rounded up and at least one:
    /// how long a drone spends levelling, refilling, filling a wall voxel or
    /// restoring a cracked one.
    pub ticks_per_voxel: u32,
    /// `materials.wall_min_height_voxels`: the lowest a wall may stand over
    /// the ground in any column. Refused at load when zero.
    pub wall_min_height_voxels: i16,
}

impl SiteRows {
    /// Read the rows, refusing what the tick could not use.
    ///
    /// # Errors
    ///
    /// [`RulesError::MissingBlock`] for an absent `combat` or `materials`
    /// block, and [`RulesError::OutOfRange`] for a repair rate that is not a
    /// positive multiple of the tick rate, a voxel time that is not positive,
    /// and a wall height of zero or past a voxel length.
    pub fn read(message: &gp::v1::RulesTable) -> Result<SiteRows, RulesError> {
        let combat = message
            .combat
            .as_ref()
            .ok_or(RulesError::MissingBlock("combat"))?;
        let materials = message
            .materials
            .as_ref()
            .ok_or(RulesError::MissingBlock("materials"))?;
        let refused = |field: &str, value: String| RulesError::OutOfRange {
            field: field.to_owned(),
            value,
        };
        let per_second = combat.repair_hp_per_second;
        if per_second == 0 || per_second.checked_rem(TICK_HZ) != Some(0) {
            return Err(refused(
                "combat.repair_hp_per_second",
                format!(
                    "{per_second}; repair must be whole hit points a tick at the tick rate, and \
                     a drone that restores nothing never finishes a repair"
                ),
            ));
        }
        let repair_hp_per_tick = per_second
            .checked_div(TICK_HZ)
            .and_then(|per_tick| i32::try_from(per_tick).ok())
            .map(Hp::new)
            .ok_or_else(|| {
                refused(
                    "combat.repair_hp_per_second",
                    format!("{per_second}; past a hit-point count"),
                )
            })?;
        if combat.terraform_ms_per_voxel <= 0 {
            return Err(refused(
                "combat.terraform_ms_per_voxel",
                format!(
                    "{}; a voxel that takes no time is a wall raised in a tick",
                    combat.terraform_ms_per_voxel
                ),
            ));
        }
        let ticks_per_voxel = Ms::new(combat.terraform_ms_per_voxel)
            .to_ticks_ceil()
            .max(1);
        let height = materials.wall_min_height_voxels;
        let wall_min_height_voxels = i16::try_from(height)
            .ok()
            .filter(|voxels| *voxels > 0)
            .ok_or_else(|| {
                refused(
                    "materials.wall_min_height_voxels",
                    format!(
                        "{height}; a wall stands at least one voxel, and a height is a voxel length"
                    ),
                )
            })?;
        Ok(SiteRows {
            repair_hp_per_tick,
            ticks_per_voxel,
            wall_min_height_voxels,
        })
    }
}

/// `BuildSettings.terraform` as a beacon holds it (item 144 (5)). An omitted
/// one reads as [`Terraform::None`], one of the playbook's named defaults.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum Terraform {
    /// Ground is left as it is.
    #[default]
    None,
    /// A footprint is raised to its anchor's height, and craters inside the
    /// sphere are refilled.
    Fill,
    /// A footprint is lowered to its anchor's height.
    Dig,
    /// Both.
    Both,
}

impl Terraform {
    /// Every value, in id order.
    pub const ALL: [Terraform; 4] = [
        Terraform::None,
        Terraform::Fill,
        Terraform::Dig,
        Terraform::Both,
    ];

    /// The wire id, `BuildSettings.Terraform`'s own number. Additive only.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            Terraform::None => 1,
            Terraform::Fill => 2,
            Terraform::Dig => 3,
            Terraform::Both => 4,
        }
    }

    /// The value an id names, or `None` for one this build does not define
    /// (`TERRAFORM_UNSPECIFIED`, 0, is no value a beacon holds).
    #[must_use]
    pub const fn from_id(id: u8) -> Option<Terraform> {
        match id {
            1 => Some(Terraform::None),
            2 => Some(Terraform::Fill),
            3 => Some(Terraform::Dig),
            4 => Some(Terraform::Both),
            _ => None,
        }
    }

    /// Whether it raises ground.
    #[must_use]
    pub const fn fills(self) -> bool {
        matches!(self, Terraform::Fill | Terraform::Both)
    }

    /// Whether it lowers ground.
    #[must_use]
    pub const fn digs(self) -> bool {
        matches!(self, Terraform::Dig | Terraform::Both)
    }
}

/// A Build beacon's S2 settings: `repair_threshold_pct`, `rebuild_destroyed`
/// and `terraform` (`playbook.proto`'s `BuildSettings` 2 to 4). The defaults
/// are a beacon's at placement and after a mandate switch: never repair a
/// structure, never rebuild, leave the ground.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BuildSettings {
    /// 0 to 100; 0 never repairs a structure (decision 38).
    pub repair_threshold_pct: u8,
    /// Rebuild a destroyed ordinary target.
    pub rebuild_destroyed: bool,
    /// What the beacon may do to the ground.
    pub terraform: Terraform,
}

impl BuildSettings {
    /// The defaults.
    pub const DEFAULT: BuildSettings = BuildSettings {
        repair_threshold_pct: 0,
        rebuild_destroyed: false,
        terraform: Terraform::None,
    };
}

/// The Build settings a settings row writes, each only when the row names it
/// (a proto3 scalar is named when it is not its default), as
/// [`crate::mining::MineEdit`] is for Mine.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BuildEdit {
    /// `repair_threshold_pct`, when the row names it.
    pub repair_threshold_pct: Option<u8>,
    /// `rebuild_destroyed`, when the row names it.
    pub rebuild_destroyed: Option<bool>,
    /// `terraform`, when the row names it.
    pub terraform: Option<Terraform>,
}

impl BuildEdit {
    /// Whether the edit writes anything.
    #[must_use]
    pub const fn writes(self) -> bool {
        self.repair_threshold_pct.is_some()
            || self.rebuild_destroyed.is_some()
            || self.terraform.is_some()
    }

    /// `settings` with this edit written over it.
    #[must_use]
    pub fn over(self, settings: BuildSettings) -> BuildSettings {
        BuildSettings {
            repair_threshold_pct: self
                .repair_threshold_pct
                .unwrap_or(settings.repair_threshold_pct),
            rebuild_destroyed: self.rebuild_destroyed.unwrap_or(settings.rebuild_destroyed),
            terraform: self.terraform.unwrap_or(settings.terraform),
        }
    }
}

/// One beacon's Build settings and the work the Quartermaster has paid its
/// drones for and they have not yet done. **Hashed, snapshotted and in the
/// goldens** ([`World::encode`]'s site block).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BeaconSite {
    /// The beacon's Build settings.
    pub settings: BuildSettings,
    /// Paid repair work, in `$`: the Repair band's fills, drawn on by the
    /// beacon's repair drones as they repair.
    pub repair_fund: Money,
    /// Paid voxel work, in `$`: the Build band's fills for walls, levelling and
    /// refills, drawn on by the beacon's build drones voxel by voxel.
    pub build_fund: Money,
}

impl BeaconSite {
    /// A beacon's at placement: default settings and nothing paid.
    pub const DEFAULT: BeaconSite = BeaconSite {
        settings: BuildSettings::DEFAULT,
        repair_fund: Money::ZERO,
        build_fund: Money::ZERO,
    };

    /// Whether the row holds what a placed beacon holds, so the hash's site
    /// block may leave it out ([`World::encode`]).
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == BeaconSite::DEFAULT
    }

    /// What is paid and not yet done, both funds together.
    #[must_use]
    pub const fn paid(&self) -> Money {
        Money::new(self.repair_fund.raw().saturating_add(self.build_fund.raw()))
    }
}

/// A `wall` target's box as the playbook wrote it: its columns, the floor it
/// says reaches the ground (`min.z`) and its top (`max.z`), inclusive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WallBox {
    /// The box's columns.
    pub columns: ColumnBox,
    /// `min.z`: at or below the ground's top in every column.
    pub floor: i16,
    /// `max.z`: the highest voxel the wall fills.
    pub top: i16,
}

/// Why a wall's box cannot stand where it was written (decisions-log item 146
/// (4)): the halves of the verifier's box checks that need the ground.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WallRefused {
    /// `min.z` lies above the ground's top in some column: the box does not
    /// reach the ground, so the wall it fills would hang in the air.
    OffTheGround,
    /// In some column the box's top stands less than
    /// `materials.wall_min_height_voxels` over the ground.
    TooLow,
    /// A voxel the wall would fill lies outside the sphere of the beacon whose
    /// mandate holds it, building's only range (decisions-log item 12).
    OutsideSphere,
}

/// Check a wall's box against the ground and the beacon's sphere, as its row
/// commits.
///
/// Every column of the box: its ground's top `g` (the column's top solid
/// voxel now) must be at or above `min.z`, the box's top must stand at least
/// `materials.wall_min_height_voxels` over it (`max.z - g`), and the lowest
/// and highest voxel the wall fills there (`g + 1` and `max.z`) must both lie
/// inside the beacon's sphere, which, being convex, then holds the voxels
/// between. A column off the map, or one with no ground, is off the ground.
///
/// # Errors
///
/// The first [`WallRefused`] a column meets, in `(y, x)` order.
pub fn check_wall(world: &World, beacon: BeaconId, wall: WallBox) -> Result<(), WallRefused> {
    let rows = world.rules().site();
    let [lx, ly] = wall.columns.min_column();
    let [hx, hy] = wall.columns.max_column();
    let top = i32::from(wall.top);
    for y in i32::from(ly)..=i32::from(hy) {
        for x in i32::from(lx)..=i32::from(hx) {
            let ground = ground_under_walls(world, x, y).ok_or(WallRefused::OffTheGround)?;
            if ground < i32::from(wall.floor) {
                return Err(WallRefused::OffTheGround);
            }
            if top.saturating_sub(ground) < i32::from(rows.wall_min_height_voxels) {
                return Err(WallRefused::TooLow);
            }
            let low = voxel_point([x, y, ground.saturating_add(1)]);
            let high = voxel_point([x, y, top]);
            let inside = |point: Option<[Fx; 3]>| {
                point.is_some_and(|at| crate::mandate::inside_sphere(world, beacon, at))
            };
            if !inside(low) || !inside(high) {
                return Err(WallRefused::OutsideSphere);
            }
        }
    }
    Ok(())
}

/// The cost to full of an asset at `hp` of `max` whose build cost is `cost`:
/// `ceil((max - hp) * cost / max)`, zero at or above full, for a free asset,
/// or for one with no hit points to have (decision 43, as built).
#[must_use]
pub fn cost_to_full(cost: Money, hp: i32, max: i32) -> Money {
    if max <= 0 || hp >= max || cost.raw() <= 0 {
        return Money::ZERO;
    }
    let missing = i64::from(max).saturating_sub(i64::from(hp.max(0)));
    let scaled = missing.saturating_mul(cost.raw());
    let whole = i64::from(max);
    // A ceiling division of a non-negative numerator by a positive divisor.
    let ceiling = scaled
        .checked_add(whole.saturating_sub(1))
        .and_then(|sum| sum.checked_div(whole));
    // E7: cannot miss: `max` is positive, and the numerator is a product of a hit-point count and a price
    Money::new(ceiling.unwrap_or(cost.raw()))
}

/// What a repair from `from` to `to` hit points charges: the cost to full
/// before it less the cost to full after it, so a repair's steps telescope to
/// `ceil(restored * cost / max)` and a repair from one hit point to full costs
/// no more than the build (decision 43, as the verification review corrected
/// it).
#[must_use]
pub fn repair_charge(cost: Money, from: i32, to: i32, max: i32) -> Money {
    let before = cost_to_full(cost, from, max);
    let after = cost_to_full(cost, to, max);
    Money::new(before.raw().saturating_sub(after.raw()).max(0))
}

/// What a voxel job does to its voxel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VoxelWork {
    /// Level a structure target's footprint: fill below its anchor.
    LevelFill,
    /// Level a structure target's footprint: dig at or above its anchor.
    LevelDig,
    /// Fill a wall's box.
    Wall,
    /// Refill a crater or a trench back to the generated surface.
    Refill,
    /// Restore a cracked own wall voxel to whole (a repair drone's).
    Restore,
}

/// One voxel of work: where, what it becomes, and why.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VoxelJob {
    /// The voxel, in whole voxels.
    pub at: [i32; 3],
    /// What it becomes.
    pub material: Material,
    /// Why.
    pub work: VoxelWork,
}

/// What a build drone does next.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuildJob {
    /// Raise a paid structure that is still going up, standing at `at`.
    Raise {
        /// The structure.
        structure: StructureId,
        /// Where it stands.
        at: [Fx; 3],
    },
    /// One voxel of its class.
    Voxel(VoxelJob),
}

/// What a repair drone repairs: a unit or a structure, by row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Patient {
    /// A unit (the commander included).
    Unit(Row),
    /// A structure.
    Structure(Row),
}

/// What a repair drone does next.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RepairJob {
    /// Repair a unit or a structure standing at `at`.
    Heal {
        /// Who.
        patient: Patient,
        /// Where it stands.
        at: [Fx; 3],
    },
    /// Restore a cracked own wall voxel.
    Restore(VoxelJob),
    /// Lift a wreck.
    Wreck(Row),
    /// Lift a ruin.
    Ruin(Row),
}

/// The point at the bottom corner of a voxel, as a structure or a unit stands
/// on one, or `None` off a coordinate.
#[must_use]
pub fn voxel_point(at: [i32; 3]) -> Option<[Fx; 3]> {
    let [x, y, z] = at;
    Some([
        Fx::from_voxels(i16::try_from(x).ok()?),
        Fx::from_voxels(i16::try_from(y).ok()?),
        Fx::from_voxels(i16::try_from(z).ok()?),
    ])
}

/// The column `(x, y)`'s top solid voxel now, read off the pathing surface
/// (one array read; it is refreshed in the phase that changes the bytes), or
/// `None` off the map or for a column of air.
fn ground_top(world: &World, x: i32, y: i32) -> Option<i32> {
    let node = world.surface().node_of(x, y)?;
    let top = world.surface().top(node);
    (top >= 0).then_some(top)
}

/// The column `(x, y)`'s ground under any wall standing on it: its top solid
/// voxel that is no wall's, found by stepping down through wall voxels from the
/// top, so a wall already filled reads the ground it was filled on and a row
/// that names it again checks the same box against the same ground.
fn ground_under_walls(world: &World, x: i32, y: i32) -> Option<i32> {
    let mut z = ground_top(world, x, y)?;
    while world
        .voxels()
        .get([x, y, z])
        .is_some_and(|material| material.wall_of().is_some() || material.is_air())
    {
        z = z.checked_sub(1).filter(|below| *below >= 0)?;
    }
    Some(z)
}

/// A drone's class among its beacon's drones of its kind: its rank by row and
/// how many there are. `None` for a unit that is no live drone of `kind`
/// homed to `home`.
fn rank_of(world: &World, unit: usize, kind: UnitKind, home: BeaconId) -> Option<(u32, u32)> {
    let units = world.units();
    let mut rank: u32 = 0;
    let mut count: u32 = 0;
    let mut found = false;
    let rows = units
        .kinds()
        .iter()
        .zip(units.homes())
        .zip(units.hit_points())
        .enumerate();
    for (row, ((of, homed), hp)) in rows {
        if *of != kind.id() || *homed != home || !hp.is_alive() {
            continue;
        }
        if row == unit {
            found = true;
            rank = count;
        }
        count = count.saturating_add(1);
    }
    found.then_some((rank, count))
}

/// The class a column belongs to among `count` drones: a spread of the
/// columns, so neighbouring columns of a wall go to different drones.
#[must_use]
pub fn class_of(x: i32, y: i32, count: u32) -> u32 {
    let key = i64::from(x).saturating_add(i64::from(y).saturating_mul(31));
    let count = i64::from(count.max(1));
    // E7: cannot miss: `rem_euclid` of a positive modulus lies in `0..count`, which a `u32` holds
    u32::try_from(key.rem_euclid(count)).unwrap_or(0)
}

/// Set every live unit standing inside solid ground back on its column's
/// surface: what a refill or a levelling fill does to a walker of the filling
/// seat that stood, or stepped, where the fill went in. Run at the start of the
/// Programs phase, so a unit the last tick's fill closed over stands on the new
/// ground before anything reads where it is. The pathing surface is refreshed
/// in the phase that changed the bytes, so its standing height is the new top.
/// Nothing else in this build fills a voxel a unit can stand in.
pub(crate) fn reseat_buried(world: &mut World) {
    let mut row: usize = 0;
    while row < world.units().positions().len() {
        let alive = world
            .units()
            .hit_points()
            .get(row)
            .is_some_and(|hp| hp.is_alive());
        let at = world.units().positions().get(row).copied();
        row = row.saturating_add(1);
        let Some(point) = at.filter(|_| alive) else {
            continue;
        };
        let voxel = point.map(Fx::floor_voxels);
        if !world.voxels().get(voxel).is_some_and(Material::is_solid) {
            continue;
        }
        let [x, y, _] = voxel;
        let Some(standing) = world
            .surface()
            .node_of(x, y)
            .map(|node| world.surface().standing_z(node))
            .and_then(|z| i16::try_from(z).ok())
        else {
            continue;
        };
        if let Some(slot) = world
            .units_mut()
            .positions_mut()
            .get_mut(row.saturating_sub(1))
        {
            let [_, _, pz] = slot;
            *pz = Fx::from_voxels(standing);
        }
    }
}

/// Whether a live unit stands in the voxel `at`.
fn occupied(world: &World, at: [i32; 3]) -> bool {
    let units = world.units();
    units
        .positions()
        .iter()
        .zip(units.hit_points())
        .any(|(point, hp)| hp.is_alive() && point.map(Fx::floor_voxels) == at)
}

/// Whether a live beacon of any seat stands on the column `(x, y)`.
fn beacon_on(world: &World, x: i32, y: i32) -> bool {
    let beacons = world.beacons();
    beacons
        .positions()
        .iter()
        .zip(beacons.hit_points())
        .any(|(at, hp)| hp.is_alive() && crate::targeting::column_of(*at) == [x, y])
}

/// Whether a fill may go into the voxel `at`: it is air, and no structure or
/// beacon stands on its column, whose voxels are theirs. A walker of the
/// filling seat standing in it is set on the new ground ([`reseat_buried`]);
/// another seat's is waited for ([`work_voxel`]).
fn fillable(world: &World, at: [i32; 3]) -> bool {
    let [x, y, _] = at;
    world.voxels().get(at).is_some_and(Material::is_air)
        && !world.ground().structure_on(x, y)
        && !beacon_on(world, x, y)
}

/// Whether the voxel `at` may be dug by a footprint's levelling: terrain, not
/// bedrock and not the core's bore.
fn diggable(world: &World, at: [i32; 3]) -> bool {
    world
        .voxels()
        .get(at)
        .is_some_and(|material| material.terrain().is_some() && !material.is_bedrock())
        && !world.bore().contains(at)
}

/// The settings of `beacon`, or the defaults for a row the table no longer
/// holds, which does no work.
fn settings_of(world: &World, beacon: BeaconId) -> Option<BeaconSite> {
    let row = world.beacons().row_of(beacon).ok()?;
    world.beacons().sites().get(row.index()).copied()
}

/// The seat that owns `beacon`, while it is a live beacon of a seat.
fn seat_of(world: &World, beacon: BeaconId) -> Option<SeatId> {
    let row = world.beacons().row_of(beacon).ok()?;
    let alive = world
        .beacons()
        .hit_points()
        .get(row.index())
        .is_some_and(|hp| hp.is_alive());
    let seat = SeatId::new(*world.beacons().seats().get(row.index())?);
    (alive && seat.is_some()).then_some(seat)
}

/// The levelling voxel a structure target at `anchor` needs next under
/// `terraform`, or `None` when its footprint is level (or cannot be levelled
/// further: a voxel that cannot be dug stays).
fn levelling(world: &World, terraform: Terraform, anchor: [Fx; 3]) -> Option<VoxelJob> {
    let [x, y] = crate::targeting::column_of(anchor);
    let height = anchor.get(2)?.floor_voxels();
    let top = ground_top(world, x, y)?;
    if terraform.fills() && top < height.saturating_sub(1) {
        let at = [x, y, top.saturating_add(1)];
        return Some(VoxelJob {
            at,
            material: Material::DIRT,
            work: VoxelWork::LevelFill,
        });
    }
    if terraform.digs() && top >= height {
        let at = [x, y, top];
        if diggable(world, at) {
            return Some(VoxelJob {
                at,
                material: Material::AIR,
                work: VoxelWork::LevelDig,
            });
        }
    }
    None
}

/// The next voxel of a wall target's box in the class `class` of `count`, or
/// any class when `count` is zero: the lowest air voxel of each column, from
/// the ground up to the box's top, columns in `(y, x)` order.
fn wall_voxel(
    world: &World,
    beacon: BeaconId,
    seat: SeatId,
    columns: ColumnBox,
    top: i16,
    class: Option<Drone>,
) -> Option<VoxelJob> {
    let material = Material::wall(seat, false)?;
    let [lx, ly] = columns.min_column();
    let [hx, hy] = columns.max_column();
    for y in i32::from(ly)..=i32::from(hy) {
        for x in i32::from(lx)..=i32::from(hx) {
            if class.is_some_and(|drone| class_of(x, y, drone.count) != drone.rank) {
                continue;
            }
            let Some(ground) = ground_top(world, x, y) else {
                continue;
            };
            let z = ground.saturating_add(1);
            if z > i32::from(top) {
                continue;
            }
            let at = [x, y, z];
            let Some(point) = voxel_point(at) else {
                continue;
            };
            if crate::mandate::protected(world, beacon, point)
                || !crate::mandate::inside_sphere(world, beacon, point)
                || !fillable(world, at)
                || occupied(world, at)
            {
                continue;
            }
            if class.is_some_and(|drone| !drone.reaches(world, x, y)) {
                continue;
            }
            return Some(VoxelJob {
                at,
                material,
                work: VoxelWork::Wall,
            });
        }
    }
    None
}

/// The next refill voxel inside `beacon`'s sphere in the class `class` (any
/// class when `None`): a column below the generated surface gets its lowest
/// air voxel filled, columns in `(y, x)` order over the sphere's square.
///
/// A refill leaves alone what is not a crater or a trench of open ground: a
/// column under one of the beacon's protected areas ("ground the mandate will
/// not dig, fill or build on"), a feature's footprint (a vent or a seam, where
/// filling would bury what a Generator stands on or a Mine beacon digs), and
/// a column a structure or a beacon stands on.
fn refill_voxel(world: &World, beacon: BeaconId, class: Option<Drone>) -> Option<VoxelJob> {
    let row = world.beacons().row_of(beacon).ok()?;
    let centre = world.beacons().positions().get(row.index()).copied()?;
    let [cx, cy] = crate::targeting::column_of(centre);
    let radius = i32::from(world.rules().sphere_radius_voxels());
    for dy in radius.saturating_neg()..=radius {
        for dx in radius.saturating_neg()..=radius {
            let x = cx.saturating_add(dx);
            let y = cy.saturating_add(dy);
            let Some(made) = world.generated_surface().top(x, y) else {
                continue;
            };
            let Some(now) = ground_top(world, x, y) else {
                continue;
            };
            if now >= made {
                continue;
            }
            if class.is_some_and(|drone| class_of(x, y, drone.count) != drone.rank) {
                continue;
            }
            let at = [x, y, now.saturating_add(1)];
            let Some(point) = voxel_point(at) else {
                continue;
            };
            if !crate::mandate::inside_sphere(world, beacon, point)
                || crate::mandate::protected(world, beacon, point)
                || world.features().at_column(x, y).is_some()
                || !fillable(world, at)
            {
                continue;
            }
            if class.is_some_and(|drone| !drone.reaches(world, x, y)) {
                continue;
            }
            return Some(VoxelJob {
                at,
                material: Material::DIRT,
                work: VoxelWork::Refill,
            });
        }
    }
    None
}

/// The build drone in `unit`'s next job, or `None` when it has none.
///
/// Its beacon's Build targets in table order, as the skeleton's drones raised
/// them: a protected target is passed over; a wall target's box gives the
/// drone the next voxel of its class; a structure still going up is levelled
/// first, under the beacon's `terraform`, by the drone of its column's class
/// (a Generator is never levelled: it stands on its vent as the vent lies),
/// and is then raised by any drone. Then, under `FILL` or `BOTH`, the refill.
#[must_use]
pub fn build_job(world: &World, unit: usize) -> Option<BuildJob> {
    let home = world.units().homes().get(unit).copied()?;
    let seat = seat_of(world, home)?;
    let site = settings_of(world, home)?;
    let class = rank_of(world, unit, UnitKind::BuildDrone, home)?;
    let here = world.units().positions().get(unit).copied()?;
    let drone = Drone {
        rank: class.0,
        count: class.1,
        here,
    };
    let terraform = site.settings.terraform;
    let targets = world.targets();
    let rows = targets
        .beacons()
        .iter()
        .zip(targets.kinds())
        .zip(targets.anchors().iter().zip(targets.built()))
        .zip(targets.areas().iter().zip(targets.wall_tops()));
    for (((owner, kind), (anchor, built)), (columns, top)) in rows {
        if *owner != home
            || *kind != TargetKind::Build.id()
            || crate::mandate::protected(world, home, *anchor)
        {
            continue;
        }
        if *top != NO_WALL {
            if let Some(job) = wall_voxel(world, home, seat, *columns, *top, Some(drone)) {
                return Some(BuildJob::Voxel(job));
            }
            continue;
        }
        let Some(index) = crate::mandate::built_row(world, *built) else {
            continue;
        };
        let structures = world.structures();
        let going_up = structures.building().get(index).copied() == Some(true)
            && structures
                .hit_points()
                .get(index)
                .is_some_and(|hp| hp.is_alive());
        if !going_up {
            continue;
        }
        let generator =
            structures.kinds().get(index).copied() == Some(StructureKind::Generator.id());
        if !generator && let Some(job) = levelling(world, terraform, *anchor) {
            let [x, y, _] = job.at;
            if class_of(x, y, class.1) == class.0 {
                return Some(BuildJob::Voxel(job));
            }
            continue;
        }
        let at = structures.positions().get(index).copied()?;
        return Some(BuildJob::Raise {
            structure: *built,
            at,
        });
    }
    if terraform.fills() {
        return refill_voxel(world, home, Some(drone)).map(BuildJob::Voxel);
    }
    None
}

/// How much voxel work `beacon` has for build drones, as fabrication counts
/// it (item 22): each wall target with a voxel left to fill, and the refill as
/// one item when there is any. A structure's levelling is counted with its
/// structure, which the mandate already counts while it goes up.
#[must_use]
pub fn voxel_work(world: &World, beacon: BeaconId) -> u32 {
    let (Some(seat), Some(site)) = (seat_of(world, beacon), settings_of(world, beacon)) else {
        return 0;
    };
    let targets = world.targets();
    let rows = targets
        .beacons()
        .iter()
        .zip(targets.kinds())
        .zip(targets.anchors())
        .zip(targets.areas().iter().zip(targets.wall_tops()));
    let mut work: u32 = 0;
    for (((owner, kind), anchor), (columns, top)) in rows {
        if *owner == beacon
            && *kind == TargetKind::Build.id()
            && *top != NO_WALL
            && !crate::mandate::protected(world, beacon, *anchor)
            && wall_voxel(world, beacon, seat, *columns, *top, None).is_some()
        {
            work = work.saturating_add(1);
        }
    }
    if site.settings.terraform.fills() && refill_voxel(world, beacon, None).is_some() {
        work = work.saturating_add(1);
    }
    work
}

/// The repair drone in `unit`'s next job, or `None` when it has none.
///
/// First the nearest patient inside its beacon's sphere, ties to the lowest
/// row, units before structures: an own structure below the beacon's
/// `repair_threshold_pct` that is not still going up (0 never repairs one;
/// decision 38), and every own unit below full, the commander included and
/// the drone itself not (spec section 4). Then a cracked own wall voxel of its
/// class inside the sphere (decision 42). Then the nearer of the wreck and the
/// ruin nearest the beacon, any seat's (spec section 7).
#[must_use]
pub fn repair_job(world: &World, unit: usize) -> Option<RepairJob> {
    let home = world.units().homes().get(unit).copied()?;
    let seat = seat_of(world, home)?;
    let site = settings_of(world, home)?;
    let here = world.units().positions().get(unit).copied()?;
    let healing = world.units().busy_until().get(unit).copied() == Some(HEALING);
    if let Some((patient, at)) = nearest_patient(
        world,
        home,
        seat,
        site.settings,
        (here, healing),
        Some(unit),
    ) {
        return Some(RepairJob::Heal { patient, at });
    }
    let class = rank_of(world, unit, UnitKind::RepairDrone, home)?;
    let drone = Drone {
        rank: class.0,
        count: class.1,
        here,
    };
    if let Some(job) = cracked_wall(world, home, seat, Some(drone)) {
        return Some(RepairJob::Restore(job));
    }
    salvage_job(world, home)
}

/// The patient nearest `here` inside `beacon`'s sphere, or `None`.
fn nearest_patient(
    world: &World,
    beacon: BeaconId,
    seat: SeatId,
    settings: BuildSettings,
    (here, healing): ([Fx; 3], bool),
    skip: Option<usize>,
) -> Option<(Patient, [Fx; 3])> {
    let mut best: Option<(Sq, u8, usize, Patient, [Fx; 3])> = None;
    let mut consider = |tag: u8, row: usize, patient: Patient, at: [Fx; 3]| {
        let key = (Sq::between(here, at), tag, row);
        if best.is_none_or(|(d, t, r, _, _)| key < (d, t, r)) {
            best = Some((key.0, tag, row, patient, at));
        }
    };
    let units = world.units();
    let rows = units
        .seats()
        .iter()
        .zip(units.kinds())
        .zip(units.hit_points().iter().zip(units.positions()))
        .enumerate();
    for (row, ((owner, kind), (hp, at))) in rows {
        if *owner != seat.raw() || !hp.is_alive() || skip == Some(row) {
            continue;
        }
        let Some(kind) = UnitKind::from_id(*kind) else {
            continue;
        };
        if hp.raw() >= world.unit_max_hp(kind).raw()
            || !crate::mandate::inside_sphere(world, beacon, *at)
        {
            continue;
        }
        if let Some(index) = Row::of_index(row) {
            consider(0, row, Patient::Unit(index), *at);
        }
    }
    if settings.repair_threshold_pct > 0 {
        let structures = world.structures();
        let rows = structures
            .seats()
            .iter()
            .zip(structures.kinds())
            .zip(structures.hit_points().iter().zip(structures.positions()))
            .zip(structures.building())
            .enumerate();
        for (row, (((owner, kind), (hp, at)), building)) in rows {
            if *owner != seat.raw() || !hp.is_alive() || *building {
                continue;
            }
            let Some(max) =
                StructureKind::from_id(*kind).and_then(|kind| world.structure_max_hp(kind))
            else {
                continue;
            };
            // A repair once begun runs to full: a structure stays a patient of
            // the drone that is repairing it ([`HEALING`]) until it is whole,
            // though it rose past the threshold on the way (decision 38's
            // "once it falls below"). A drone merely standing beside a
            // structure above the threshold begins nothing.
            let begun = healing && hp.raw() < max.raw() && at_work(world, here, *at);
            if !(below_threshold(hp.raw(), max.raw(), settings.repair_threshold_pct) || begun)
                || !crate::mandate::inside_sphere(world, beacon, *at)
            {
                continue;
            }
            if let Some(index) = Row::of_index(row) {
                consider(1, row, Patient::Structure(index), *at);
            }
        }
    }
    best.map(|(_, _, _, patient, at)| (patient, at))
}

/// Whether `hp` of `max` is below `pct` percent of it: `hp * 100 < pct * max`.
#[must_use]
pub fn below_threshold(hp: i32, max: i32, pct: u8) -> bool {
    i64::from(hp).saturating_mul(100) < i64::from(pct).saturating_mul(i64::from(max))
}

/// The next cracked own wall voxel inside `beacon`'s sphere in the class
/// `class` (any class when `None`), or `None`.
///
/// Read over the boxes of `seat`'s own wall targets, every beacon's of the
/// seat, since a cracked voxel is a wall's and the boxes are where walls
/// stand; each column from the box's top down through the wall to the
/// ground, columns in `(y, x)` order and boxes in table order. A wall voxel
/// is its seat's while it lies inside one of the seat's live spheres (E14,
/// decision 42), which this beacon's is.
fn cracked_wall(
    world: &World,
    beacon: BeaconId,
    seat: SeatId,
    class: Option<Drone>,
) -> Option<VoxelJob> {
    let whole = Material::wall(seat, false)?;
    let row = world.beacons().row_of(beacon).ok()?;
    let [cx, cy] = world
        .beacons()
        .positions()
        .get(row.index())
        .copied()
        .map(crate::targeting::column_of)?;
    let radius = i32::from(world.rules().sphere_radius_voxels());
    let targets = world.targets();
    let rows = targets
        .beacons()
        .iter()
        .zip(targets.kinds())
        .zip(targets.areas().iter().zip(targets.wall_tops()));
    for ((owner, kind), (columns, top)) in rows {
        if *kind != TargetKind::Build.id()
            || *top == NO_WALL
            || seat_of(world, *owner) != Some(seat)
        {
            continue;
        }
        let [lx, ly] = columns.min_column();
        let [hx, hy] = columns.max_column();
        // A box wholly outside the sphere's square holds nothing this beacon
        // repairs: passed over before a column of it is read.
        let misses = i32::from(hx) < cx.saturating_sub(radius)
            || i32::from(lx) > cx.saturating_add(radius)
            || i32::from(hy) < cy.saturating_sub(radius)
            || i32::from(ly) > cy.saturating_add(radius);
        if misses {
            continue;
        }
        for y in i32::from(ly)..=i32::from(hy) {
            for x in i32::from(lx)..=i32::from(hx) {
                if class.is_some_and(|drone| class_of(x, y, drone.count) != drone.rank)
                    || class.is_some_and(|drone| !drone.reaches(world, x, y))
                {
                    continue;
                }
                let mut z = i32::from(*top);
                while z >= 0 {
                    let at = [x, y, z];
                    let Some(material) = world.voxels().get(at) else {
                        break;
                    };
                    match material.wall_of() {
                        Some((builder, true)) if builder == seat => {
                            let inside = voxel_point(at).is_some_and(|point| {
                                crate::mandate::inside_sphere(world, beacon, point)
                            });
                            if inside {
                                return Some(VoxelJob {
                                    at,
                                    material: whole,
                                    work: VoxelWork::Restore,
                                });
                            }
                        }
                        Some(_) => {}
                        None if material.is_air() => {}
                        None => break,
                    }
                    z = z.saturating_sub(1);
                }
            }
        }
    }
    None
}

/// The nearer of the wreck and the ruin nearest `beacon`'s anchor inside its
/// sphere with salvage left, ties to the wreck; `None` when there is neither.
fn salvage_job(world: &World, beacon: BeaconId) -> Option<RepairJob> {
    let row = world.beacons().row_of(beacon).ok()?;
    let centre = world.beacons().positions().get(row.index()).copied()?;
    let wreck = world.wreck_in_sphere(beacon).and_then(|at| {
        let place = world.wrecks().positions().get(at.index()).copied()?;
        Some((Sq::between(centre, place), RepairJob::Wreck(at)))
    });
    let ruin = ruin_in_sphere(world, beacon).and_then(|at| {
        let place = world.structures().positions().get(at.index()).copied()?;
        Some((Sq::between(centre, place), RepairJob::Ruin(at)))
    });
    match (wreck, ruin) {
        (Some((dw, w)), Some((dr, r))) => Some(if dr < dw { r } else { w }),
        (Some((_, w)), None) => Some(w),
        (None, Some((_, r))) => Some(r),
        (None, None) => None,
    }
}

/// The ruin nearest `beacon`'s anchor inside its sphere, ties to the lowest
/// row, or `None`. A ruin is a structure row nobody owns that is not free
/// (S2's plan, E1), salvageable by any seat's reclaim drones (spec section
/// 5).
#[must_use]
pub fn ruin_in_sphere(world: &World, beacon: BeaconId) -> Option<Row> {
    let row = world.beacons().row_of(beacon).ok()?;
    let centre = world.beacons().positions().get(row.index()).copied()?;
    let structures = world.structures();
    let mut best: Option<(Sq, usize)> = None;
    let rows = structures
        .seats()
        .iter()
        .zip(structures.frees())
        .zip(structures.positions().iter().zip(structures.ruined_at()))
        .enumerate();
    for (index, ((owner, free), (at, ruined))) in rows {
        if *free
            || SeatId::new(*owner).is_some()
            || *ruined == crate::tables::NOT_RUINED
            || !crate::mandate::inside_sphere(world, beacon, *at)
        {
            continue;
        }
        let key = (Sq::between(centre, *at), index);
        if best.is_none_or(|held| key < held) {
            best = Some(key);
        }
    }
    best.and_then(|(_, index)| Row::of_index(index))
}

/// How much repair work `beacon` has in hand, as fabrication counts it (item
/// 22; spec section 9: repair drones are "fabricated by Build beacons on work
/// in hand"): each patient, a cracked wall as one item, and each wreck and
/// ruin inside the sphere.
#[must_use]
pub fn repair_work(world: &World, beacon: BeaconId) -> u32 {
    let (Some(seat), Some(site)) = (seat_of(world, beacon), settings_of(world, beacon)) else {
        return 0;
    };
    let mut work: u32 = 0;
    let units = world.units();
    let rows = units
        .seats()
        .iter()
        .zip(units.kinds())
        .zip(units.hit_points().iter().zip(units.positions()));
    for ((owner, kind), (hp, at)) in rows {
        let wounded =
            UnitKind::from_id(*kind).is_some_and(|kind| hp.raw() < world.unit_max_hp(kind).raw());
        if *owner == seat.raw()
            && hp.is_alive()
            && wounded
            && crate::mandate::inside_sphere(world, beacon, *at)
        {
            work = work.saturating_add(1);
        }
    }
    if site.settings.repair_threshold_pct > 0 {
        let structures = world.structures();
        let rows = structures
            .seats()
            .iter()
            .zip(structures.kinds())
            .zip(structures.hit_points().iter().zip(structures.positions()))
            .zip(structures.building());
        for (((owner, kind), (hp, at)), building) in rows {
            let damaged = StructureKind::from_id(*kind)
                .and_then(|kind| world.structure_max_hp(kind))
                .is_some_and(|max| {
                    below_threshold(hp.raw(), max.raw(), site.settings.repair_threshold_pct)
                });
            if *owner == seat.raw()
                && hp.is_alive()
                && !*building
                && damaged
                && crate::mandate::inside_sphere(world, beacon, *at)
            {
                work = work.saturating_add(1);
            }
        }
    }
    if cracked_wall(world, beacon, seat, None).is_some() {
        work = work.saturating_add(1);
    }
    let centre = world
        .beacons()
        .row_of(beacon)
        .ok()
        .and_then(|row| world.beacons().positions().get(row.index()).copied());
    if let Some(centre) = centre {
        let limit = Sq::of_radius(world.sphere_radius());
        let wrecks = world.wrecks();
        for ((free, value), at) in wrecks
            .frees()
            .iter()
            .zip(wrecks.salvages())
            .zip(wrecks.positions())
        {
            if !*free && value.raw() > 0 && Sq::between(centre, *at) <= limit {
                work = work.saturating_add(1);
            }
        }
        let structures = world.structures();
        let rows = structures
            .seats()
            .iter()
            .zip(structures.frees())
            .zip(structures.positions().iter().zip(structures.ruined_at()));
        for ((owner, free), (at, ruined)) in rows {
            if !*free
                && !SeatId::new(*owner).is_some()
                && *ruined != crate::tables::NOT_RUINED
                && Sq::between(centre, *at) <= limit
            {
                work = work.saturating_add(1);
            }
        }
    }
    work
}

/// One window of work's price: what the drones of `kind` homed to `beacon`
/// that stand at a paid job would draw over the next decision interval and
/// over the next second.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Need {
    interval: i64,
    second: i64,
}

/// What a fund of `held` must be topped up by for `need`: `None` while it
/// covers the coming interval, otherwise the price of [`TOP_UP_TICKS`] of the work
/// less what it holds.
fn top_up(held: Money, need: Need) -> Option<Money> {
    if need.interval <= 0 || held.raw() >= need.interval {
        return None;
    }
    let amount = need.second.max(need.interval).saturating_sub(held.raw());
    (amount > 0).then_some(Money::new(amount))
}

/// How many voxels one drone finishes in `ticks`, at least one.
fn voxels_in(world: &World, ticks: u32) -> i64 {
    let per = world.rules().site().ticks_per_voxel.max(1);
    let whole = ticks.saturating_add(per.saturating_sub(1)).checked_div(per);
    // E7: cannot miss: `per` is at least one
    i64::from(whole.unwrap_or(1).max(1))
}

/// The Repair band's top-up `beacon` asks for, or `None` (decision 43).
#[must_use]
pub fn repair_shortfall(world: &World, beacon: BeaconId) -> Option<Money> {
    let site = settings_of(world, beacon)?;
    let interval = world.decision_ticks();
    let per_tick = world.rules().site().repair_hp_per_tick.raw();
    let price = world.rules().wall_cost_per_voxel().raw();
    let mut need = Need::default();
    for unit in drones_of(world, beacon, UnitKind::RepairDrone) {
        let Some(here) = world.units().positions().get(unit).copied() else {
            continue;
        };
        match repair_job(world, unit) {
            Some(RepairJob::Heal { patient, at }) if at_work(world, here, at) => {
                let Some((hp, max, cost)) = patient_state(world, patient) else {
                    continue;
                };
                let charge = |ticks: u32| {
                    let steps = i64::from(ticks).saturating_mul(i64::from(per_tick));
                    let to = i64::from(hp).saturating_add(steps).min(i64::from(max));
                    // E7: cannot miss: `to` lies between `hp` and `max`, both `i32`
                    let to = i32::try_from(to).unwrap_or(max);
                    repair_charge(cost, hp, to, max).raw()
                };
                need.interval = need.interval.saturating_add(charge(interval));
                need.second = need.second.saturating_add(charge(TOP_UP_TICKS));
            }
            Some(RepairJob::Restore(job)) if at_voxel(world, here, job.at) => {
                need.interval = need
                    .interval
                    .saturating_add(price.saturating_mul(voxels_in(world, interval)));
                need.second = need
                    .second
                    .saturating_add(price.saturating_mul(voxels_in(world, TOP_UP_TICKS)));
            }
            _ => {}
        }
    }
    top_up(site.repair_fund, need)
}

/// The Build band's voxel top-up `beacon` asks for, or `None`.
#[must_use]
pub fn build_shortfall(world: &World, beacon: BeaconId) -> Option<Money> {
    let site = settings_of(world, beacon)?;
    let interval = world.decision_ticks();
    let price = world.rules().wall_cost_per_voxel().raw();
    let mut need = Need::default();
    for unit in drones_of(world, beacon, UnitKind::BuildDrone) {
        let Some(here) = world.units().positions().get(unit).copied() else {
            continue;
        };
        if let Some(BuildJob::Voxel(job)) = build_job(world, unit)
            && at_voxel(world, here, job.at)
        {
            need.interval = need
                .interval
                .saturating_add(price.saturating_mul(voxels_in(world, interval)));
            need.second = need
                .second
                .saturating_add(price.saturating_mul(voxels_in(world, TOP_UP_TICKS)));
        }
    }
    top_up(site.build_fund, need)
}

/// The rows of the live drones of `kind` homed to `beacon` whose home is lit,
/// in row order.
fn drones_of(world: &World, beacon: BeaconId, kind: UnitKind) -> impl Iterator<Item = usize> + '_ {
    let lit = crate::power::beacon_is_live(world, beacon);
    let units = world.units();
    units
        .kinds()
        .iter()
        .zip(units.homes())
        .zip(units.hit_points())
        .enumerate()
        .filter(move |(_, ((of, home), hp))| {
            lit && **of == kind.id() && **home == beacon && hp.is_alive()
        })
        .map(|(row, _)| row)
}

/// A patient's hit points, its maximum and its build cost (the commander's is
/// nothing), or `None` for a row that is no longer a live patient.
fn patient_state(world: &World, patient: Patient) -> Option<(i32, i32, Money)> {
    match patient {
        Patient::Unit(row) => {
            let units = world.units();
            let kind = UnitKind::from_id(*units.kinds().get(row.index())?)?;
            let hp = units.hit_points().get(row.index())?.raw();
            let cost = if kind == UnitKind::Commander {
                Money::ZERO
            } else {
                world.unit_cost(kind)
            };
            (hp > 0).then_some((hp, world.unit_max_hp(kind).raw(), cost))
        }
        Patient::Structure(row) => {
            let structures = world.structures();
            let kind = StructureKind::from_id(*structures.kinds().get(row.index())?)?;
            let hp = structures.hit_points().get(row.index())?.raw();
            let max = world.structure_max_hp(kind)?.raw();
            (hp > 0).then_some((hp, max, world.structure_cost(kind)))
        }
    }
}

/// The eight columns around one, in the order a drone looks for a stand.
const AROUND: [[i32; 2]; 8] = [
    [1, 0],
    [0, 1],
    [-1, 0],
    [0, -1],
    [1, 1],
    [-1, 1],
    [-1, -1],
    [1, -1],
];

/// Whether a walker on `here`'s column can walk to the column `(x, y)`: both
/// are walkable and the abstract graph joins them (the same two array reads
/// that refuse a route, item 60).
fn reachable(world: &World, here: [Fx; 3], x: i32, y: i32) -> bool {
    let [hx, hy] = crate::targeting::column_of(here);
    match (
        world.surface().node_of(hx, hy),
        world.surface().node_of(x, y),
    ) {
        (Some(from), Some(to)) => world.clusters().connected(world.surface(), from, to),
        _ => false,
    }
}

/// Whether `here` stands beside the column `(x, y)`: within the arrival
/// radius of it across the ground, whatever the height (on it included). A
/// drone works a column from beside it, so a footprint raised above its
/// neighbours, a wall's next voxel and a crater's floor are all in reach of a
/// drone standing on the ground next to them.
fn beside(world: &World, here: [Fx; 3], x: i32, y: i32) -> bool {
    let [hx, hy] = crate::targeting::column_of(here);
    let reach = i64::from(world.rules().arrive_radius_voxels());
    let dx = i64::from(hx.saturating_sub(x));
    let dy = i64::from(hy.saturating_sub(y));
    dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy)) <= reach.saturating_mul(reach)
}

/// A place to stand beside the column `(x, y)` that a walker at `here` can
/// reach: the first of its eight neighbours, in [`AROUND`]'s order, that is
/// walkable and joined to `here`'s column. `None` when none is.
fn stand_beside(world: &World, here: [Fx; 3], x: i32, y: i32) -> Option<[Fx; 3]> {
    AROUND.iter().find_map(|[dx, dy]| {
        let (sx, sy) = (x.saturating_add(*dx), y.saturating_add(*dy));
        reachable(world, here, sx, sy).then(|| world.standing_point([sx, sy, 0]))
    })
}

/// Whether a drone at `here` is at work on something standing at `there`: as
/// the skeleton's drones were, within the arrival radius of it; or, when its
/// column cannot be walked to (a footprint levelled above its neighbours, a
/// wreck in a crater), beside it.
fn at_work(world: &World, here: [Fx; 3], there: [Fx; 3]) -> bool {
    if crate::programs::within(here, there, world.arrive_radius()) {
        return true;
    }
    let [x, y] = crate::targeting::column_of(there);
    !reachable(world, here, x, y) && beside(world, here, x, y)
}

/// Walk a drone at `here` toward work standing at `there`: to it while its
/// column can be walked to, as the skeleton's drones did, and otherwise to a
/// stand beside it. A drone that can reach neither waits where it is.
fn go_to_work(world: &mut World, unit: u32, here: [Fx; 3], there: [Fx; 3]) {
    let [x, y] = crate::targeting::column_of(there);
    if reachable(world, here, x, y) {
        world.send_unit(unit, there);
    } else if let Some(stand) = stand_beside(world, here, x, y) {
        world.send_unit(unit, stand);
    } else {
        world.park_unit(unit);
    }
}

/// Whether a drone at `here` is at work on the voxel `at`: beside its column,
/// or on it, a fill setting it on the new ground ([`reseat_buried`]).
fn at_voxel(world: &World, here: [Fx; 3], at: [i32; 3]) -> bool {
    let [x, y, _] = at;
    beside(world, here, x, y)
}

/// Walk a drone at `here` to a stand beside the voxel `at`'s column.
fn go_to_voxel(world: &mut World, unit: u32, here: [Fx; 3], at: [i32; 3]) {
    let [x, y, _] = at;
    match stand_beside(world, here, x, y) {
        Some(stand) => world.send_unit(unit, stand),
        None => world.park_unit(unit),
    }
}

/// Which fund a voxel job draws on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fund {
    Repair,
    Build,
}

impl World {
    /// Write `beacon`'s Build settings over what it holds (a committed
    /// settings row), or back to the defaults (a mandate switch).
    pub(crate) fn set_build_settings(&mut self, row: usize, settings: BuildSettings) {
        if let Some(slot) = self.beacons.sites_mut().get_mut(row) {
            slot.settings = settings;
        }
    }

    /// The Quartermaster's fill of a site fund: `amount` moves onto `beacon`'s
    /// repair or build fund. `false` for a beacon the table no longer holds,
    /// so nothing is charged.
    pub(crate) fn pay_site_fund(&mut self, beacon: BeaconId, repair: bool, amount: Money) -> bool {
        let Ok(row) = self.beacons.row_of(beacon) else {
            return false;
        };
        let Some(slot) = self.beacons.sites_mut().get_mut(row.index()) else {
            return false;
        };
        let fund = if repair {
            &mut slot.repair_fund
        } else {
            &mut slot.build_fund
        };
        let Some(sum) = fund.checked_add(amount) else {
            return false;
        };
        *fund = sum;
        true
    }

    /// Draw `amount` from `beacon`'s fund: `false`, drawing nothing, when the
    /// fund does not cover it.
    fn draw_fund(&mut self, beacon: BeaconId, fund: Fund, amount: Money) -> bool {
        let Ok(row) = self.beacons.row_of(beacon) else {
            return false;
        };
        let Some(slot) = self.beacons.sites_mut().get_mut(row.index()) else {
            return false;
        };
        let held = match fund {
            Fund::Repair => &mut slot.repair_fund,
            Fund::Build => &mut slot.build_fund,
        };
        match held.checked_sub(amount) {
            Some(left) if left.raw() >= 0 => {
                *held = left;
                true
            }
            _ => false,
        }
    }

    /// Recycle `structure`, a structure of `seat`'s standing for a Build target
    /// a settings edit dropped (S2-10, decision 26): refund
    /// `economy.recycle_refund_percent` of its remaining value, as recycling
    /// a beacon does, and take it out of the world. Its hit points go to zero
    /// and the ordinary settlement runs, so whatever damage anybody had done
    /// to it is credited through the damage split ([`crate::credit`]) exactly
    /// as a beacon's recycle is; it leaves no wreck, because it was not
    /// destroyed. `false` for a structure that is not `seat`'s or not alive.
    pub(crate) fn recycle_structure(&mut self, seat: SeatId, structure: StructureId) -> bool {
        let Ok(row) = self.structures.row_of(structure) else {
            return false;
        };
        let index = row.index();
        let (Some(owner), Some(hp), Some(kind)) = (
            self.structures.seats().get(index).copied(),
            self.structures.hit_points().get(index).copied(),
            self.structures
                .kinds()
                .get(index)
                .copied()
                .and_then(StructureKind::from_id),
        ) else {
            return false;
        };
        if owner != seat.raw() || !hp.is_alive() {
            return false;
        }
        let Some(max) = self.structure_max_hp(kind) else {
            return false;
        };
        let remaining = crate::economy::value_of(self.structure_cost(kind), hp.raw(), max.raw());
        let refund = crate::economy::percent_of(remaining, self.rules.recycle_refund_percent());
        if let Some(slot) = self.structures.hit_points_mut().get_mut(index) {
            *slot = Hp::ZERO;
        }
        self.credit_treasury(seat, refund);
        let at = self.structures.positions().get(index).copied();
        let mut emission = Emission::of(EventKind::StructureRecycled)
            .seat(seat)
            .subject_of(AssetId::of_structure(structure))
            .value(refund.raw());
        if let Some(point) = at {
            emission = emission.at(Position::from_array(point));
        }
        let tick = self.tick;
        self.emit(tick, emission);
        true
    }

    /// Lift the ruin in `row`: its salvage, `economy.salvage_percent` of its
    /// kind's build cost, and its row freed, as the ash's burial frees one.
    /// `None` for a row that holds no ruin.
    fn lift_ruin(&mut self, row: Row) -> Option<Money> {
        let index = row.index();
        let owner = *self.structures.seats().get(index)?;
        let free = *self.structures.frees().get(index)?;
        let ruined = *self.structures.ruined_at().get(index)?;
        if free || SeatId::new(owner).is_some() || ruined == crate::tables::NOT_RUINED {
            return None;
        }
        let kind = StructureKind::from_id(*self.structures.kinds().get(index)?)?;
        let value = crate::economy::percent_of(
            self.structure_cost(kind),
            self.rules.combat().salvage_percent,
        );
        if self.structures.release(row) {
            self.refs.retired();
        }
        Some(value)
    }
}

/// Run one tick of a build drone (spec section 6's Build row): its job, or
/// idle at home. Called by [`crate::programs::BuildProgram`].
pub(crate) fn run_build_drone(world: &mut World, unit: u32) {
    let row = crate::math::index::row(unit);
    let Some(here) = world.units().positions().get(row).copied() else {
        return;
    };
    let Some(home) = world.units().homes().get(row).copied() else {
        return;
    };
    match build_job(world, row) {
        None => {
            clear_busy(world, row);
            crate::programs::idle_at_home(world, unit);
        }
        Some(BuildJob::Raise { structure, at }) => {
            clear_busy(world, row);
            raise(world, unit, here, structure, at);
        }
        Some(BuildJob::Voxel(job)) => {
            let seat = seat_of(world, home);
            work_voxel(world, unit, here, home, seat, job, Fund::Build);
        }
    }
}

/// Raise a structure going up: walk to it, then add a tick's hit points
/// (`structures.build_hp_per_second`), finishing it at full. The skeleton's
/// build program, unchanged.
fn raise(world: &mut World, unit: u32, here: [Fx; 3], structure: StructureId, site: [Fx; 3]) {
    if !at_work(world, here, site) {
        go_to_work(world, unit, here, site);
        return;
    }
    let Ok(at) = world.structures().row_of(structure) else {
        return;
    };
    let at = at.index();
    let (Some(hp), Some(kind)) = (
        world.structures().hit_points().get(at).copied(),
        world.structures().kinds().get(at).copied(),
    ) else {
        return;
    };
    // `structures.build_hp_per_second` divides the tick rate exactly -- a
    // table whose row does not is refused at load -- so there is no
    // accumulator and no rounding rule (`RulesTable::build_hp_per_tick`). A
    // wall has no structure row, and no row is one.
    let Some(full) = StructureKind::from_id(kind).and_then(|kind| world.structure_max_hp(kind))
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

/// Clear a drone's timed work: it is not standing at a voxel.
fn clear_busy(world: &mut World, row: usize) {
    if let Some(slot) = world.units_mut().busy_until_mut().get_mut(row)
        && *slot != NO_WORK
    {
        *slot = NO_WORK;
    }
}

/// Work one voxel: walk to it, spend `site.ticks_per_voxel` beside it, then,
/// when the fund covers the voxel's price, file the edit and draw the price.
/// A drone whose voxel the fund cannot cover waits beside it. The time spent is
/// the drone's and not the voxel's, since a drone keeps no job of its own: one
/// whose job moved to another voxel within its reach while it worked (its rank
/// shifted, the voxel was taken) finishes the voxel it has now, and one whose
/// new job is out of reach walks to it and starts again.
fn work_voxel(
    world: &mut World,
    unit: u32,
    here: [Fx; 3],
    home: BeaconId,
    seat: Option<SeatId>,
    job: VoxelJob,
    fund: Fund,
) {
    let row = crate::math::index::row(unit);
    let Some(point) = voxel_point(job.at) else {
        return;
    };
    if !at_voxel(world, here, job.at) {
        clear_busy(world, row);
        go_to_voxel(world, unit, here, job.at);
        return;
    }
    let tick = world.tick().raw();
    let held = world.units().busy_until().get(row).copied();
    // E7: rule: a row the table does not hold is doing no timed work
    let busy = held.unwrap_or(NO_WORK);
    if busy == NO_WORK {
        world.park_unit(unit);
        let due = tick.saturating_add(world.rules().site().ticks_per_voxel);
        if let Some(slot) = world.units_mut().busy_until_mut().get_mut(row) {
            *slot = due;
        }
        return;
    }
    if tick < busy {
        return;
    }
    // A fill never goes in under another seat's walker: it would lift a raider
    // standing in a breach onto the wall that closes it. The drone waits for
    // the voxel to clear; its own seat's walkers are set on the new ground ([`reseat_buried`]).
    if job.material.is_solid() && foreigner_in(world, job.at, seat) {
        return;
    }
    // Two beacons of one seat whose spheres overlap may set their drones on
    // one voxel in one tick: the second finds the first's edit queued, pays
    // nothing and goes on to its next job.
    let queued = world
        .edits
        .iter()
        .any(|edit| matches!(edit, VoxelEdit::Set { at, .. } if *at == job.at));
    if queued {
        world.site_drops.already_queued = world.site_drops.already_queued.saturating_add(1);
        clear_busy(world, row);
        return;
    }
    let price = world.rules().wall_cost_per_voxel();
    if !world.draw_fund(home, fund, price) {
        // Waiting on the Quartermaster: the voxel is done the tick its price
        // is paid (decision 43's "the drone waiting while it is unfilled").
        return;
    }
    let filed = world.file_voxel_edit(VoxelEdit::Set {
        at: job.at,
        material: job.material,
        by: seat,
    });
    // The queue's sizing makes a full queue unreachable in a legal world (S2's
    // plan, E5); the voxel's price goes back to the fund and the drone tries
    // again next tick, as a mining drone's dig does, each drop counted.
    if filed.is_err() {
        world.site_drops.queue_refused = world.site_drops.queue_refused.saturating_add(1);
        if !world.pay_site_fund(home, fund == Fund::Repair, price) {
            world.site_drops.refund_lost = world.site_drops.refund_lost.saturating_add(1);
        }
        return;
    }
    clear_busy(world, row);
    if job.work == VoxelWork::Refill
        && world
            .generated_surface()
            .top(job.at[0], job.at[1])
            .is_some_and(|made| made == job.at[2])
    {
        let tick = world.tick();
        let mut emission = Emission::of(EventKind::GroundRefilled).at(Position::from_array(point));
        if let Some(seat) = seat {
            emission = emission.seat(seat);
        }
        world.emit(tick, emission);
    }
}

/// Run one tick of a repair drone (spec section 9): deliver what it carries,
/// then its job, or idle at home. Called by [`crate::programs::RepairProgram`].
pub(crate) fn run_repair_drone(world: &mut World, unit: u32) {
    let row = crate::math::index::row(unit);
    let (Some(here), Some(home), Some(carried)) = (
        world.units().positions().get(row).copied(),
        world.units().homes().get(row).copied(),
        world.units().carrying().get(row).copied(),
    ) else {
        return;
    };
    if carried.raw() > 0 {
        clear_busy(world, row);
        crate::programs::deliver(world, unit, EventKind::SalvageDelivered);
        return;
    }
    match repair_job(world, row) {
        None => {
            clear_busy(world, row);
            crate::programs::idle_at_home(world, unit);
        }
        Some(RepairJob::Heal { patient, at }) => {
            heal(world, unit, here, home, patient, at);
        }
        Some(RepairJob::Restore(job)) => {
            if world.units().busy_until().get(row).copied() == Some(HEALING) {
                clear_busy(world, row);
            }
            let seat = seat_of(world, home);
            work_voxel(world, unit, here, home, seat, job, Fund::Repair);
        }
        Some(RepairJob::Wreck(at)) => {
            clear_busy(world, row);
            let Some(site) = world.wrecks().positions().get(at.index()).copied() else {
                return;
            };
            if !at_work(world, here, site) {
                go_to_work(world, unit, here, site);
                return;
            }
            let Some(value) = world.wrecks().salvages().get(at.index()).copied() else {
                return;
            };
            if let Some(slot) = world.wrecks_mut().salvages_mut().get_mut(at.index()) {
                *slot = Money::ZERO;
            }
            if let Some(slot) = world.units_mut().carrying_mut().get_mut(row) {
                *slot = value;
            }
        }
        Some(RepairJob::Ruin(at)) => {
            clear_busy(world, row);
            let Some(site) = world.structures().positions().get(at.index()).copied() else {
                return;
            };
            if !at_work(world, here, site) {
                go_to_work(world, unit, here, site);
                return;
            }
            if let Some(value) = world.lift_ruin(at)
                && let Some(slot) = world.units_mut().carrying_mut().get_mut(row)
            {
                *slot = value;
            }
        }
    }
}

/// Repair a patient by a tick's hit points, drawing the step's price
/// ([`repair_charge`]) from the beacon's repair fund; a step the fund cannot
/// cover waits. Reaching full announces `asset_repaired`.
fn heal(
    world: &mut World,
    unit: u32,
    here: [Fx; 3],
    home: BeaconId,
    patient: Patient,
    at: [Fx; 3],
) {
    let drone = crate::math::index::row(unit);
    if !at_work(world, here, at) {
        clear_busy(world, drone);
        go_to_work(world, unit, here, at);
        return;
    }
    let Some((hp, max, cost)) = patient_state(world, patient) else {
        clear_busy(world, drone);
        return;
    };
    world.park_unit(unit);
    // The mark that lets a structure's repair run on to full ([`HEALING`]):
    // set while the drone works a structure short of full, cleared otherwise.
    let mark =
        matches!(patient, Patient::Structure(_)) && hp.saturating_add(per_tick_of(world)) < max;
    if let Some(slot) = world.units_mut().busy_until_mut().get_mut(drone) {
        *slot = if mark { HEALING } else { NO_WORK };
    }
    let per_tick = world.rules().site().repair_hp_per_tick.raw();
    let to = hp.saturating_add(per_tick).min(max);
    let charge = repair_charge(cost, hp, to, max);
    if !world.draw_fund(home, Fund::Repair, charge) {
        return;
    }
    let (slot, subject, seat) = match patient {
        Patient::Unit(row) => {
            let id = world
                .units()
                .ids()
                .get(row.index())
                .copied()
                .map(UnitId::new);
            let seat = world.units().seats().get(row.index()).copied();
            (
                world.units_mut().hit_points_mut().get_mut(row.index()),
                id.map(AssetId::of_unit),
                seat,
            )
        }
        Patient::Structure(row) => {
            let id = world
                .structures()
                .ids()
                .get(row.index())
                .copied()
                .map(StructureId::new);
            let seat = world.structures().seats().get(row.index()).copied();
            (
                world.structures_mut().hit_points_mut().get_mut(row.index()),
                id.map(AssetId::of_structure),
                seat,
            )
        }
    };
    if let Some(slot) = slot {
        *slot = Hp::new(to);
    }
    if to >= max {
        let tick = world.tick();
        let mut emission = Emission::of(EventKind::AssetRepaired)
            .at(Position::from_array(at))
            .value(i64::from(max));
        if let Some(seat) = seat {
            emission = emission.seat(SeatId::new(seat));
        }
        if let Some(subject) = subject {
            emission = emission.subject_of(subject);
        }
        world.emit(tick, emission);
    }
}

impl From<WallRefused> for crate::interpreter::state::StepFailure {
    fn from(refused: WallRefused) -> Self {
        match refused {
            WallRefused::OffTheGround => crate::interpreter::state::StepFailure::WallOffTheGround,
            WallRefused::TooLow => crate::interpreter::state::StepFailure::WallTooLow,
            WallRefused::OutsideSphere => crate::interpreter::state::StepFailure::WallOutsideSphere,
        }
    }
}

/// The site block's tag in the state hash ([`World::encode`]): unlike the
/// shell block's ([`crate::tables::ShellTable::HASH_TAG`]), so the two
/// optional blocks at the end of the encoding never read alike.
pub const SITE_HASH_TAG: u8 = 2;

/// A repair drone's `busy_until` while it is repairing a structure that has
/// not reached full: the mark that lets the repair run on past the threshold
/// (decision 38's "once it falls below"). Hashed and carried as the column is;
/// no tick reaches it, since a match is far shorter than `u32::MAX - 1` ticks,
/// and it is cleared the moment the drone does anything else.
pub const HEALING: u32 = NO_WORK - 1;

/// The hit points one working repair drone restores a tick.
fn per_tick_of(world: &World) -> i32 {
    world.rules().site().repair_hp_per_tick.raw()
}

/// Whether a live unit of a seat other than `seat` stands in the voxel `at`.
fn foreigner_in(world: &World, at: [i32; 3], seat: Option<SeatId>) -> bool {
    let [x, y, z] = at;
    let units = world.units();
    units
        .positions()
        .iter()
        .zip(units.hit_points())
        .zip(units.seats())
        .any(|((point, hp), owner)| {
            let [px, py, pz] = *point;
            hp.is_alive()
                && Some(SeatId::new(*owner)) != seat
                && px.floor_voxels() == x
                && py.floor_voxels() == y
                && pz.floor_voxels() == z
        })
}

/// How much work a top-up pays for: five seconds of it, at the drones' rates.
///
/// PLACEHOLDER: a site top-up's horizon, five seconds of drone work — owner, at S2's demo, with `tune2`'s report.
/// A beacon files one request a decision tick, so a short horizon would
/// spend most of its requests on top-ups and leave its fabricator and its
/// targets waiting; a long one moves more `$` onto the beacon ahead of the
/// work. Five seconds keeps a top-up to about one request in twenty.
const TOP_UP_TICKS: u32 = 5 * TICK_HZ;

impl World {
    /// Give the work `row`'s beacon was paid for in advance and has not done
    /// back to its seat's treasury, whole: what a mandate switch and a recycle
    /// do with it (S2's `site`). It is the seat's `$` still -- it counts in
    /// held value as paid work -- and the drones it was paid for no longer do
    /// that work. A beacon destroyed loses it, as a structure going up is lost.
    pub(crate) fn return_site_funds(&mut self, row: usize) {
        let Some(site) = self.beacons.sites().get(row).copied() else {
            return;
        };
        let Some(seat) = self.beacons.seats().get(row).copied().map(SeatId::new) else {
            return;
        };
        let paid = site.paid();
        if paid.raw() <= 0 {
            return;
        }
        if let Some(slot) = self.beacons.sites_mut().get_mut(row) {
            slot.repair_fund = Money::ZERO;
            slot.build_fund = Money::ZERO;
        }
        self.credit_treasury(seat, paid);
    }
}

/// A drone looking for voxel work: its class among its beacon's drones of its
/// kind ([`class_of`]) and where it stands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Drone {
    rank: u32,
    count: u32,
    here: [Fx; 3],
}

impl Drone {
    /// Whether the drone can work the column `(x, y)`: it stands beside it, or
    /// it can walk to a stand beside it. A column no stand of which it can
    /// reach is passed over rather than held, so one unreachable column never
    /// stalls the rest of its class's work.
    fn reaches(self, world: &World, x: i32, y: i32) -> bool {
        beside(world, self.here, x, y) || stand_beside(world, self.here, x, y).is_some()
    }
}

/// Why a drone's finished voxel was not filed, each counted (the no-silent-
/// drop rule). Diagnostic; never hashed, as [`crate::world::DamageDrops`] is
/// not.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SiteDrops {
    /// The voxel queue refused the edit (full, which its sizing makes
    /// unreachable in a legal world, S2's plan, E5); its price went back to
    /// the fund and the drone tries again next tick.
    pub queue_refused: u64,
    /// The price could not go back to the fund either (a beacon the table no
    /// longer holds, or a fund past a `$` count): the `$` is gone, and counted.
    pub refund_lost: u64,
    /// Another drone's edit for the same voxel was already queued this tick
    /// (two beacons of one seat whose spheres overlap): nothing was charged.
    pub already_queued: u64,
}

impl World {
    /// What S2's `site` dropped, by reason. Diagnostic; never hashed.
    #[must_use]
    pub const fn site_drops(&self) -> SiteDrops {
        self.site_drops
    }
}

#[cfg(test)]
mod tests {
    use super::{BuildSettings, Terraform};
    use crate::encoding::Enc;
    use crate::math::fixed::Fx;
    use crate::math::quantity::Money;
    use crate::rules::RulesTable;
    use crate::runner::MatchSettings;
    use crate::tables::{SeatId, StructureKind, UnitKind};
    use crate::voxels::VoxelEdit;
    use crate::world::{Phase, World, WorldConfig};

    fn world() -> World {
        let rules = RulesTable::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("rules")
                .join("rules.v1.json"),
        )
        .unwrap_or_else(|error| panic!("the committed rules table loads: {error}"));
        World::new(&WorldConfig {
            match_seed: crate::DETERMINISM_MATCH_SEED,
            seats: 2,
            units_per_seat: 0,
            rules,
            match_settings: MatchSettings::default(),
        })
        .unwrap_or_else(|error| panic!("the committed table describes a world: {error}"))
    }

    /// Every levelling edit carries the seat of the beacon whose drone made
    /// it, as every other edit does, so a dig that detaches a region is
    /// credited as any other edit is (decision 37).
    #[test]
    fn a_levelling_edit_carries_its_seat() {
        let mut world = world();
        let seat = SeatId::new(0);
        let core =
            crate::power::core_of(&world, seat).unwrap_or_else(|| panic!("seat 0 has a core"));
        world.set_treasury(seat, Money::new(100_000));
        world.set_build_settings_directly(
            core,
            BuildSettings {
                repair_threshold_pct: 0,
                rebuild_destroyed: false,
                terraform: Terraform::Both,
            },
        );
        let row = world
            .beacons()
            .row_of(core)
            .unwrap_or_else(|_| panic!("the core has a row"));
        let [cx, cy, _] = world
            .beacons()
            .positions()
            .get(row.index())
            .copied()
            .unwrap_or_else(|| panic!("the core stands somewhere"))
            .map(Fx::floor_voxels);
        let (x, y) = (cx.saturating_add(5), cy.saturating_add(3));
        let ground = world
            .voxels()
            .top_solid_z(x, y)
            .unwrap_or_else(|| panic!("ground"));
        let anchor = [x, y, ground.saturating_add(3)]
            .map(|axis| Fx::from_voxels(i16::try_from(axis).unwrap_or_else(|_| panic!("axis"))));
        assert!(
            world
                .add_target(core, StructureKind::Autocannon.id(), anchor)
                .is_ok()
        );
        let stand = world.standing_point([cx.saturating_add(1), cy.saturating_add(1), 0]);
        assert!(
            world
                .field_unit(seat, core, UnitKind::BuildDrone, stand)
                .is_some()
        );
        let mut runner = crate::runner::Runner::new(world);
        assert_eq!(runner.begin_push(), Ok(true));
        let mut found: Vec<Option<SeatId>> = Vec::new();
        for _ in 0..2_000 {
            let world = runner.world_mut();
            assert!(world.begin_tick().is_ok());
            assert!(world.run_phase(Phase::Broadphase).is_ok());
            assert!(world.run_phase(Phase::Programs).is_ok());
            found.extend(world.edits.iter().map(VoxelEdit::by));
            let mut next = world.next_phase();
            while let Some(phase) = next {
                if phase == Phase::Hash {
                    break;
                }
                assert!(world.run_phase(phase).is_ok());
                next = world.next_phase();
            }
            let mut enc = Enc::with_capacity(64 * 1024);
            assert!(world.end_tick(&mut enc).is_ok());
            if found.len() >= 2 {
                break;
            }
        }
        assert_eq!(
            found,
            vec![Some(seat), Some(seat)],
            "both levelling voxels are seat 0's"
        );
    }
}
