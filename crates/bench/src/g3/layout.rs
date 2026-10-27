// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The gate's layout (S2's plan, section 5, E30 and decision 2): 40 beacons
//! as a world total (three cores and 37 laid out) and 100 laid-out units a
//! seat, at three seats, placed once through `Host::seed_layout` in the lobby.
//!
//! The layout is a pure function of the generated world, which is a pure
//! function of the seed and the rules, so the report's seed and `rules_hash`
//! reproduce it; [`digest`] fingerprints it with the sim's own digest so two
//! reports can say they ran the same one.
//!
//! # Where everything goes, per seat
//!
//! | Mandate | Beacons | Units each | Where |
//! |---|---|---|---|
//! | Attack | 2 | 18 raiders | inside each enemy core's autocannon range, [`ATTACK_FROM_ENEMY_CORE`] voxels from it on the line between the two cores |
//! | Defend | 2 | 11 raiders | toward each enemy, [`DEFEND_FROM_CORE`] voxels from the own core, so an enemy Attack beacon stands 3 voxels from it |
//! | Mine | 3 | 6 mining drones | on the three seams nearest the own core, a column apart per seat so two seats never share one |
//! | Build | 3 (4 at seat 0) | 3 build drones and 2 repair drones (the fourth none) | the cardinal points [`BUILD_RING`] voxels from the core |
//! | Survey | 2 | 5 and 4 scouts | two diagonals [`SURVEY_RING`] voxels from the core |
//!
//! The proportions and distances are the harness's own inputs, not rules:
//! they set where the load stands, and the report prints them beside every
//! figure. `g3r` kept `g3h`'s on the finished sim, which the owner can veto.
//!
//! Seat 0 takes the 13th beacon because 37 does not divide by three; the
//! world's share rule (13 a seat at three seats) allows it. The units are
//! 36 + 22 + 18 + 15 + 9 = 100 a seat.
//!
//! **What the mandates make of it is the sim's.** On the finished S2 sim
//! (`dfnd`, `atk`), each Defend beacon's guards go after the enemy Attack
//! raiders standing three voxels from it (an unset `roe` reads `FREE_FIRE`,
//! item 148 (2)), and each core's autocannon fires at the Attack raiders
//! inside its range. The layout seam carries no mandate settings
//! (`runner::LayoutBeacon`), so an Attack beacon has no target and its
//! raiders gather at it and never launch, and a Build beacon has no blueprint.
//!
//! [`layout_at`] lays out the same beacons with a share of the units, for the
//! power sweep ([`super::sweep`]).

use pharmakos_sim::features::FeatureKind;
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::runner::{Layout, LayoutBeacon};
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::tables::{SeatId, UnitKind};
use pharmakos_sim::world::World;

use crate::error::Error;

/// The seats the gate is measured at (decision 2).
pub const GATE_SEATS: u32 = 3;

/// Units laid out per seat (decision 2: 300 as a world total, item 63).
pub const UNITS_PER_SEAT: u32 = 100;

/// Beacons laid out as a world total: the gate's 40 less the three cores.
pub const LAID_OUT_BEACONS: u32 = 37;

/// The most beacons one seat is laid out with: seat 0's 13.
pub const MOST_BEACONS_PER_SEAT: u32 = 13;

/// How far from an enemy core an Attack beacon stands, in voxels: inside the
/// core's 24-voxel autocannon range and outside a raider's 4.
pub const ATTACK_FROM_ENEMY_CORE: i64 = 14;

/// How far from its own core a Defend beacon stands, toward an enemy.
pub const DEFEND_FROM_CORE: i64 = 11;

/// The Build beacons' distance from the core, on the cardinal points.
pub const BUILD_RING: i32 = 8;

/// The Survey beacons' distance from the core along each axis, on two
/// diagonals.
pub const SURVEY_RING: i32 = 11;

/// One seat's core column, as the generator placed it.
fn cores(world: &World) -> Result<Vec<[i32; 2]>, Error> {
    let beacons = world.beacons();
    let mut cores: Vec<Option<[i32; 2]>> =
        vec![None; usize::try_from(GATE_SEATS).map_err(|_| Error::Overflow("the seat count"))?];
    for (seat, at) in beacons.seats().iter().zip(beacons.positions()) {
        let slot = cores.get_mut(usize::from(*seat)).ok_or_else(|| {
            Error::Layout(format!(
                "a generated beacon belongs to seat {seat}, past the gate's seats"
            ))
        })?;
        if slot.is_some() {
            return Err(Error::Layout(format!(
                "seat {seat} holds two beacons before the layout; the gate lays out a world whose \
                 only beacons are the cores"
            )));
        }
        *slot = Some([at[0].floor_voxels(), at[1].floor_voxels()]);
    }
    cores
        .into_iter()
        .enumerate()
        .map(|(seat, core)| {
            core.ok_or_else(|| Error::Layout(format!("seat {seat} has no core on this map")))
        })
        .collect()
}

/// `from` moved `by` voxels toward `to`, by integer arithmetic.
fn toward(from: [i32; 2], to: [i32; 2], by: i64) -> Result<[i32; 2], Error> {
    let dx = i64::from(to[0]) - i64::from(from[0]);
    let dy = i64::from(to[1]) - i64::from(from[1]);
    let length = (dx * dx + dy * dy).isqrt();
    if length <= by {
        return Err(Error::Layout(format!(
            "two cores {length} voxels apart leave no room for a beacon {by} voxels out"
        )));
    }
    let step = |origin: i32, delta: i64| -> Result<i32, Error> {
        let moved = i64::from(origin) + (delta * by).div_euclid(length);
        i32::try_from(moved).map_err(|_| Error::Overflow("a laid-out column"))
    };
    Ok([step(from[0], dx)?, step(from[1], dy)?])
}

/// `core` offset by `[dx, dy]`.
fn offset(core: [i32; 2], dx: i32, dy: i32) -> Result<[i32; 2], Error> {
    Ok([
        core[0]
            .checked_add(dx)
            .ok_or(Error::Overflow("a laid-out column"))?,
        core[1]
            .checked_add(dy)
            .ok_or(Error::Overflow("a laid-out column"))?,
    ])
}

fn beacon(
    seat: u8,
    column: [i32; 2],
    mandate: MandateKind,
    units: Vec<(UnitKind, u32)>,
) -> LayoutBeacon {
    LayoutBeacon {
        seat: SeatId::new(seat),
        column,
        mandate,
        units,
    }
}

/// The three seams nearest `core`, nearest first, ties by anchor.
fn nearest_seams(world: &World, core: [i32; 2]) -> Result<Vec<[i32; 2]>, Error> {
    let mut seams: Vec<(i64, [i32; 2])> = world
        .features()
        .features()
        .iter()
        .filter(|feature| feature.kind == FeatureKind::Seam)
        .map(|feature| {
            let dx = i64::from(feature.anchor[0]) - i64::from(core[0]);
            let dy = i64::from(feature.anchor[1]) - i64::from(core[1]);
            (dx * dx + dy * dy, feature.anchor)
        })
        .collect();
    seams.sort_unstable();
    let nearest: Vec<[i32; 2]> = seams
        .into_iter()
        .take(3)
        .map(|(_, anchor)| anchor)
        .collect();
    if nearest.len() < 3 {
        return Err(Error::Layout(format!(
            "the map holds {} seams, and the gate lays three Mine beacons a seat on seams",
            nearest.len()
        )));
    }
    Ok(nearest)
}

/// A share of the gate's units, in quarters: what the power sweep lays out
/// to measure a unit's cost (S2's plan, section 5 item 3; item 65's
/// arithmetic). Every beacon is laid out at every share; only its units scale,
/// each group's count times the quarters over four, rounded down.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Quarters(u8);

impl Quarters {
    /// The gate's own load: every unit the module's table names.
    pub const FULL: Quarters = Quarters(4);

    /// `quarters` of the gate's units, or `None` past four.
    #[must_use]
    pub const fn new(quarters: u8) -> Option<Quarters> {
        if quarters <= 4 {
            Some(Quarters(quarters))
        } else {
            None
        }
    }

    /// The quarters, 0 to 4.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }

    /// `count` at this share, rounded down.
    fn of(self, count: u32) -> Result<u32, Error> {
        count
            .checked_mul(u32::from(self.0))
            .map(|scaled| scaled.div_euclid(4))
            .ok_or(Error::Overflow("a scaled unit count"))
    }
}

/// One seat's unit groups, as the module's table lays them out: Attack's
/// two, Defend's two, Mine's three, Build's three crews (the fourth Build
/// beacon has none) and Survey's two.
const SEAT_GROUPS: [(UnitKind, u32); 15] = [
    (UnitKind::Raider, 18),
    (UnitKind::Raider, 18),
    (UnitKind::Raider, 11),
    (UnitKind::Raider, 11),
    (UnitKind::MiningDrone, 6),
    (UnitKind::MiningDrone, 6),
    (UnitKind::MiningDrone, 6),
    (UnitKind::BuildDrone, 3),
    (UnitKind::RepairDrone, 2),
    (UnitKind::BuildDrone, 3),
    (UnitKind::RepairDrone, 2),
    (UnitKind::BuildDrone, 3),
    (UnitKind::RepairDrone, 2),
    (UnitKind::Scout, 5),
    (UnitKind::Scout, 4),
];

/// The units one seat is laid out with at `share`: 100 at [`Quarters::FULL`].
///
/// # Errors
///
/// [`Error::Overflow`], which the table's counts cannot reach.
pub fn units_per_seat(share: Quarters) -> Result<u32, Error> {
    SEAT_GROUPS.iter().try_fold(0_u32, |sum, (_, count)| {
        sum.checked_add(share.of(*count)?)
            .ok_or(Error::Overflow("a seat's laid-out units"))
    })
}

/// The gate's layout on `world`, a freshly generated three-seat map.
///
/// # Errors
///
/// [`Error::Layout`] for a map the layout does not fit: not three cores, too
/// few seams, or cores too close together.
pub fn gate_layout(world: &World) -> Result<Layout, Error> {
    layout_at(world, Quarters::FULL)
}

/// The gate's layout with `share` of its units (the power sweep's).
///
/// # Errors
///
/// As [`gate_layout`].
pub fn layout_at(world: &World, share: Quarters) -> Result<Layout, Error> {
    let scaled = |groups: Vec<(UnitKind, u32)>| -> Result<Vec<(UnitKind, u32)>, Error> {
        let mut kept = Vec::with_capacity(groups.len());
        for (kind, count) in groups {
            let count = share.of(count)?;
            if count > 0 {
                kept.push((kind, count));
            }
        }
        Ok(kept)
    };
    let cores = cores(world)?;
    let mut beacons: Vec<LayoutBeacon> = Vec::new();
    for (index, core) in cores.iter().enumerate() {
        let seat = u8::try_from(index).map_err(|_| Error::Overflow("a seat id"))?;
        let enemies = [1_usize, 2].map(|step| (index + step) % cores.len());
        for enemy in enemies {
            let theirs = *cores.get(enemy).ok_or(Error::Overflow("an enemy index"))?;
            beacons.push(beacon(
                seat,
                toward(theirs, *core, ATTACK_FROM_ENEMY_CORE)?,
                MandateKind::Attack,
                scaled(vec![(UnitKind::Raider, 18)])?,
            ));
            beacons.push(beacon(
                seat,
                toward(*core, theirs, DEFEND_FROM_CORE)?,
                MandateKind::Defend,
                scaled(vec![(UnitKind::Raider, 11)])?,
            ));
        }
        let shift = i32::from(seat);
        for seam in nearest_seams(world, *core)? {
            beacons.push(beacon(
                seat,
                offset(seam, shift, 0)?,
                MandateKind::Mine,
                scaled(vec![(UnitKind::MiningDrone, 6)])?,
            ));
        }
        let ring = [
            (BUILD_RING, 0),
            (0, BUILD_RING),
            (-BUILD_RING, 0),
            (0, -BUILD_RING),
        ];
        let builds = if seat == 0 { 4 } else { 3 };
        for (slot, (dx, dy)) in ring.into_iter().take(builds).enumerate() {
            let units = if slot < 3 {
                scaled(vec![(UnitKind::BuildDrone, 3), (UnitKind::RepairDrone, 2)])?
            } else {
                Vec::new()
            };
            beacons.push(beacon(
                seat,
                offset(*core, dx, dy)?,
                MandateKind::Build,
                units,
            ));
        }
        for (dx, dy, scouts) in [
            (SURVEY_RING, SURVEY_RING, 5),
            (-SURVEY_RING, -SURVEY_RING, 4),
        ] {
            beacons.push(beacon(
                seat,
                offset(*core, dx, dy)?,
                MandateKind::Survey,
                scaled(vec![(UnitKind::Scout, scouts)])?,
            ));
        }
    }
    let layout = Layout { beacons };
    check(&layout, units_per_seat(share)?)?;
    Ok(layout)
}

/// The layout holds what the module documents: 37 beacons, and `units` a seat
/// (100 at the full share).
fn check(layout: &Layout, expected: u32) -> Result<(), Error> {
    let placed =
        u32::try_from(layout.beacons.len()).map_err(|_| Error::Overflow("the beacon count"))?;
    if placed != LAID_OUT_BEACONS {
        return Err(Error::Drift(format!(
            "the gate's layout holds {placed} beacons, not the documented {LAID_OUT_BEACONS}"
        )));
    }
    for seat in 0..GATE_SEATS {
        let units: u32 = layout
            .beacons
            .iter()
            .filter(|beacon| u32::from(beacon.seat.raw()) == seat)
            .flat_map(|beacon| beacon.units.iter().map(|(_, count)| *count))
            .sum();
        if units != expected {
            return Err(Error::Drift(format!(
                "seat {seat} is laid out with {units} units, not the documented {expected}"
            )));
        }
    }
    Ok(())
}

/// How many beacons of each mandate the layout holds, in the order the report
/// prints them.
#[must_use]
pub fn proportions(layout: &Layout) -> Vec<(&'static str, usize)> {
    [
        (MandateKind::Build, "Build"),
        (MandateKind::Defend, "Defend"),
        (MandateKind::Attack, "Attack"),
        (MandateKind::Mine, "Mine"),
        (MandateKind::Survey, "Survey"),
    ]
    .into_iter()
    .map(|(kind, name)| {
        (
            name,
            layout
                .beacons
                .iter()
                .filter(|beacon| beacon.mandate == kind)
                .count(),
        )
    })
    .collect()
}

/// A fingerprint of the layout: the sim's own digest over each beacon's seat,
/// column, mandate and units, in order.
#[must_use]
pub fn digest(layout: &Layout) -> u64 {
    let mut bytes: Vec<u8> = Vec::new();
    for beacon in &layout.beacons {
        bytes.push(beacon.seat.raw());
        bytes.extend_from_slice(&beacon.column[0].to_le_bytes());
        bytes.extend_from_slice(&beacon.column[1].to_le_bytes());
        bytes.push(beacon.mandate.id());
        for (kind, count) in &beacon.units {
            bytes.push(kind.id());
            bytes.extend_from_slice(&count.to_le_bytes());
        }
        bytes.push(0xff);
    }
    pharmakos_sim::digest(&bytes)
}

/// A column as the sim's Q16.16 position, for a caller that compares one with
/// a unit's.
///
/// # Errors
///
/// [`Error::Overflow`] for a column past the map's coordinate range.
pub fn column_fx(column: [i32; 2]) -> Result<[Fx; 2], Error> {
    let fx = |v: i32| {
        i16::try_from(v)
            .map(Fx::from_voxels)
            .map_err(|_| Error::Overflow("a column"))
    };
    Ok([fx(column[0])?, fx(column[1])?])
}
