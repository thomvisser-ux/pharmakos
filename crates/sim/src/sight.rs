// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sight: the one sensor interface (S2's plan, E10), and the sphere rule a host
//! draws with today.
//!
//! # One sensor interface, two outputs
//!
//! Every source of sight is a [`Sensor`]: a beacon's sphere
//! (`beacon.sphere_radius_voxels`), a unit's vision radius from its kind's
//! rules row (`units.<kind>.vision_radius_voxels`, the commander's
//! `commander.vision_radius_voxels`), and a finished structure's from its row
//! (`structures.<kind>.vision_radius_voxels`); Defend's pickets and S4's Sensor
//! Spire join as more sensors, and radar would be one more (item 138 (4) (c)).
//! The interface has two outputs: the **live view** the fog filter reads
//! ([`World::sight`], an owned [`Sight`] a host asks many times), and the
//! **knowledge store's sightings and reach** ([`crate::survey`], which walks
//! the same sensors through [`World::sensor_at`]).
//!
//! A sensor sees everything within its radius (item 144 (4): unit vision is a
//! radius, with no line of sight). The line-of-sight test is a seam,
//! [`LineOfSight`], whose one model today, [`RadiusOnly`], answers "visible" for
//! every point in the radius; a later model replaces the model, not its
//! callers.
//!
//! # The sphere rule the live view still draws with
//!
//! The gateway draws the live view from the spheres alone until S2's `wgw`
//! moves it onto [`World::sight`]:
//! Spec section 6 states it -- **spheres give passive vision** -- and the
//! rules table numbers it, `beacon.sphere_radius_voxels`. The owner's answer of
//! 2026-09-21 (decisions-log item 108 (1)) is that this is the whole of the
//! live view at the skeleton: enemy units, beacons, structures and voxel edits
//! "appear only inside own beacon spheres until the match-end unlock". So this
//! module is exactly that rule, as decisions-log items 107 (6) and 110 (5) name
//! it: [`World::in_own_sphere`].
//!
//! # One definition of "inside a sphere"
//!
//! The arithmetic is the interpreter's own [`crate::interpreter::cond`]
//! `within`: integer squared distance in Q32.32 against the squared radius, no
//! square root (AGENTS.md section 4.2). Before T17 the gateway held a second
//! copy of it (`host::SphereVision`, a labelled stopgap); it now holds a
//! [`Spheres`] value and asks it, so the sim's rule and the view's cannot
//! drift apart.
//!
//! # Why a value as well as a query
//!
//! A host answers "may this seat see that voxel" thousands of times a call,
//! from code that holds the surface mutably while it does -- so it cannot hold
//! a borrow of the world. [`World::spheres`] is the owned snapshot of what the
//! rule reads (each living beacon's owner and centre, and the radius), and
//! [`World::in_own_sphere`] is that snapshot asked once. Both reach the same
//! [`Spheres::contains`], which is the only place the rule is written.
//!
//! # What it does not model
//!
//! Nothing here is hashed state: a sensor is read off the hashed tables, and the
//! sightings a tick records from them are hashed where they are kept
//! ([`crate::tables::SightingTable`]). [`Spheres`] does **not** include a unit's
//! or a structure's own vision, which [`Sight`] does; item 108 (1) kept the
//! live view to the spheres, ruled at the demo (item 126 (2) (b), register
//! D-25), and S2's `wgw` moves the gateway onto [`World::sight`] (S1-34's
//! gateway half).

use crate::ids::Row;
use crate::math::fixed::Fx;
use crate::math::quantity::Hp;
use crate::programs::within;
use crate::tables::{BeaconId, SeatId, StructureKind, UnitKind};
use crate::world::World;

/// What a sensor is: the source its radius comes from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SensorKind {
    /// A living beacon's sphere, dormant or not, `beacon.sphere_radius_voxels`.
    Sphere,
    /// A living unit's own vision, its kind's `vision_radius_voxels`.
    Unit(UnitKind),
    /// A finished structure's own vision, its kind's `vision_radius_voxels`.
    Structure(StructureKind),
}

/// One source of sight (S2's plan, E10): whose it is, where it stands, how far
/// it sees, and what it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sensor {
    /// The seat that sees through it.
    pub seat: SeatId,
    /// Where it stands.
    pub centre: [Fx; 3],
    /// How far it sees, in Q16.16 voxels.
    pub radius: Fx,
    /// What it is.
    pub kind: SensorKind,
}

/// The line-of-sight seam (item 144 (4)): whether a sensor that reaches a point
/// sees it.
///
/// A later model -- terrain masking, a 3D voxel traversal (the radar memo's
/// section 10) -- replaces the model, not the sensors' callers.
pub trait LineOfSight {
    /// Whether `sensor` sees `point`, which lies within its radius.
    fn visible(&self, sensor: &Sensor, point: [Fx; 3]) -> bool;
}

/// The one model today: everything in the radius is visible (item 144 (4):
/// "unit vision is a radius, with no line of sight").
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RadiusOnly;

impl LineOfSight for RadiusOnly {
    fn visible(&self, _sensor: &Sensor, _point: [Fx; 3]) -> bool {
        true
    }
}

impl Sensor {
    /// Whether this sensor sees `point`: inside its radius, and visible under
    /// the line-of-sight model.
    #[must_use]
    pub fn sees<L: LineOfSight>(&self, model: &L, point: [Fx; 3]) -> bool {
        within(point, self.centre, self.radius) && model.visible(self, point)
    }
}

/// The live view's output of the sensor interface: every seat's sensors, owned,
/// for a host that asks many times while the world moves on ([`World::sight`]).
///
/// Rebuild it after anything that can move, place or lose a sensor: a stale
/// one draws last tick's sight.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Sight {
    sensors: Vec<Sensor>,
}

impl Sight {
    /// True when `voxel` is seen by one of `seat`'s sensors. A coordinate
    /// outside the fixed-point range is off every map and is seen by nobody.
    #[must_use]
    pub fn sees(&self, seat: SeatId, voxel: [i32; 3]) -> bool {
        let Some(point) = point_of(voxel) else {
            return false;
        };
        self.sensors
            .iter()
            .filter(|sensor| sensor.seat == seat)
            .any(|sensor| sensor.sees(&RadiusOnly, point))
    }

    /// Every sensor, in the order [`World::sensor_at`] walks them.
    #[must_use]
    pub fn sensors(&self) -> &[Sensor] {
        &self.sensors
    }
}

/// Every living beacon's sphere, as an owned value a host can hold while the
/// world moves on.
///
/// Built by [`World::spheres`]. Rebuild it after anything that can place, lose
/// or move a beacon: a stale one draws last tick's spheres.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Spheres {
    /// `beacon.sphere_radius_voxels`, in whole voxels: a voxel length, which
    /// the rules table refuses at load to be anything else.
    radius_voxels: i16,
    /// One entry per living beacon: the seat that owns it and its centre.
    centres: Vec<(u8, [Fx; 3])>,
}

impl Spheres {
    /// True when `voxel` lies inside the sphere of one of `seat`'s own living
    /// beacons.
    ///
    /// The voxel is measured from its own lowest corner -- the point a
    /// position floors onto -- so "the voxel a beacon stands in" and "the
    /// voxel a sphere reaches" are measured from the same place. A coordinate
    /// outside the fixed-point range is off every map this project makes and
    /// is seen by nobody.
    #[must_use]
    pub fn contains(&self, seat: SeatId, voxel: [i32; 3]) -> bool {
        let Some(point) = point_of(voxel) else {
            return false;
        };
        self.centres
            .iter()
            .filter(|(owner, _)| *owner == seat.raw())
            .any(|(_, centre)| within(point, *centre, Fx::from_voxels(self.radius_voxels)))
    }

    /// How many living beacons the snapshot holds, every seat's together.
    #[must_use]
    pub fn len(&self) -> usize {
        self.centres.len()
    }

    /// True when no beacon is alive.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.centres.is_empty()
    }

    /// The radius every sphere has, in whole voxels.
    #[must_use]
    pub fn radius_voxels(&self) -> i32 {
        i32::from(self.radius_voxels)
    }
}

/// One whole voxel as the fixed-point point a squared distance is taken to.
fn point_of(voxel: [i32; 3]) -> Option<[Fx; 3]> {
    let [x, y, z] = voxel;
    Some([
        Fx::from_voxels(i16::try_from(x).ok()?),
        Fx::from_voxels(i16::try_from(y).ok()?),
        Fx::from_voxels(i16::try_from(z).ok()?),
    ])
}

impl World {
    /// The sensor at position `at` of the walk over every sensor, and the
    /// position after it, or `None` once the walk is done (S2's plan, E10).
    ///
    /// The walk is a total order: every living beacon by row, then every
    /// living unit by row, then every finished structure a seat owns by row.
    /// Positions that hold no sensor are skipped, so a caller walks with
    /// `while let Some((sensor, next)) = world.sensor_at(at)` and allocates
    /// nothing.
    #[must_use]
    pub fn sensor_at(&self, at: usize) -> Option<(Sensor, usize)> {
        let beacons = crate::math::index::row(self.beacons().len());
        let units = crate::math::index::row(self.units().len());
        let structures = crate::math::index::row(self.structures().len());
        let mut at = at;
        loop {
            let sensor = if at < beacons {
                self.beacon_sensor(at)
            } else if at < beacons.saturating_add(units) {
                self.unit_sensor(at.saturating_sub(beacons))
            } else if at < beacons.saturating_add(units).saturating_add(structures) {
                self.structure_sensor(at.saturating_sub(beacons.saturating_add(units)))
            } else {
                return None;
            };
            at = at.saturating_add(1);
            if let Some(sensor) = sensor {
                return Some((sensor, at));
            }
        }
    }

    /// The beacon in `row` as a sensor: its sphere, while it is alive,
    /// dormant or not. A dormant beacon keeps its seal and its sphere powered
    /// off the key-core (item 10), so its sphere still sees, as
    /// [`World::spheres`] has it (decisions-log item 146).
    fn beacon_sensor(&self, row: usize) -> Option<Sensor> {
        self.beacons().id_of(Row::of_index(row)?)?;
        if !self.beacons().hit_points().get(row)?.is_alive() {
            return None;
        }
        let seat = SeatId::new(self.beacons().seats().get(row).copied()?);
        Some(Sensor {
            seat: seat.is_some().then_some(seat)?,
            centre: self.beacons().positions().get(row).copied()?,
            radius: self.sphere_radius(),
            kind: SensorKind::Sphere,
        })
    }

    /// The unit in `row` as a sensor: its kind's vision radius, while it is
    /// alive and its home beacon is not dormant. A dormant beacon's units are
    /// powered down and see nothing (decisions-log item 146); the commander is
    /// homed to no grid's dormancy and keeps its own sight.
    fn unit_sensor(&self, row: usize) -> Option<Sensor> {
        let units = self.units();
        if !units.hit_points().get(row)?.is_alive() {
            return None;
        }
        let seat = SeatId::new(units.seats().get(row).copied()?);
        let kind = UnitKind::from_id(units.kinds().get(row).copied()?)?;
        if kind != UnitKind::Commander && self.home_is_dark(*units.homes().get(row)?) {
            return None;
        }
        Some(Sensor {
            seat: seat.is_some().then_some(seat)?,
            centre: units.positions().get(row).copied()?,
            radius: Fx::from_voxels(self.rules().unit(kind).vision_radius_voxels),
            kind: SensorKind::Unit(kind),
        })
    }

    /// The structure in `row` as a sensor: its kind's vision radius, while a
    /// seat owns it, it stands and it is finished. A ruin is nobody's, and a
    /// structure going up has no capability until it is finished (spec
    /// section 7).
    ///
    /// A structure homed to a dormant beacon is powered down and sees nothing
    /// (decisions-log item 146).
    fn structure_sensor(&self, row: usize) -> Option<Sensor> {
        let structures = self.structures();
        if !structures.hit_points().get(row)?.is_alive()
            || *structures.building().get(row)?
            || self.home_is_dark(*structures.homes().get(row)?)
        {
            return None;
        }
        let seat = SeatId::new(structures.seats().get(row).copied()?);
        let kind = StructureKind::from_id(structures.kinds().get(row).copied()?)?;
        Some(Sensor {
            seat: seat.is_some().then_some(seat)?,
            centre: structures.positions().get(row).copied()?,
            radius: Fx::from_voxels(self.rules().structure(kind)?.vision_radius_voxels),
            kind: SensorKind::Structure(kind),
        })
    }

    /// Whether `home` is a beacon this world still holds and it is dormant: the
    /// one thing that powers a unit's or a structure's sight down. No home, or
    /// a home since freed, is no grid and no dormancy.
    fn home_is_dark(&self, home: BeaconId) -> bool {
        self.beacons()
            .row_of(home)
            .ok()
            .and_then(|row| self.beacons().dormant().get(row.index()).copied())
            == Some(true)
    }

    /// The unit in `row` as a **Survey scout**'s sensor, with the Survey
    /// beacon it is homed to (S2's `surv`): a living scout whose sensor is on
    /// ([`World::sensor_at`]'s rule for a unit) and whose home is a living
    /// beacon carrying the Survey writ. A Defend beacon's pickets are scouts
    /// too, and are not this: they warn, they do not survey.
    ///
    /// The Survey half of this module: [`crate::survey`] marks the explored
    /// cells and files its reports through this sensor, and only through it.
    pub(crate) fn survey_scout_sensor(&self, row: usize) -> Option<(Sensor, BeaconId)> {
        let sensor = self.unit_sensor(row)?;
        if sensor.kind != SensorKind::Unit(UnitKind::Scout) {
            return None;
        }
        let home = *self.units().homes().get(row)?;
        let beacon = self.beacons().row_of(home).ok()?.index();
        let survey = self.beacons().mandates().get(beacon).copied()?
            == crate::seams::MandateKind::Survey.id();
        let alive = self.beacons().hit_points().get(beacon)?.is_alive();
        (survey && alive).then_some((sensor, home))
    }

    /// The live view's output of the sensor interface: every sensor, owned
    /// ([`Sight`]). Allocates; a host's question, never asked inside a tick.
    #[must_use]
    pub fn sight(&self) -> Sight {
        let mut sensors = Vec::new();
        let mut at: usize = 0;
        while let Some((sensor, next)) = self.sensor_at(at) {
            sensors.push(sensor);
            at = next;
        }
        Sight { sensors }
    }

    /// Every living beacon's sphere, owned, for a host that asks many times.
    ///
    /// Rows are read in table order, which is beacon id order, and a beacon at
    /// zero hit points is not a sphere (T14: a dead beacon's structures are
    /// ruins and it powers nothing).
    #[must_use]
    pub fn spheres(&self) -> Spheres {
        let radius_voxels = self.rules().sphere_radius_voxels();
        let beacons = self.beacons();
        let mut centres: Vec<(u8, [Fx; 3])> = Vec::with_capacity(beacons.ids().len());
        for row in 0..beacons.ids().len() {
            let alive = beacons
                .hit_points()
                .get(row)
                .copied()
                .is_some_and(Hp::is_alive);
            if !alive {
                continue;
            }
            let (Some(seat), Some(centre)) = (
                beacons.seats().get(row).copied(),
                beacons.positions().get(row).copied(),
            ) else {
                continue;
            };
            centres.push((seat, centre));
        }
        Spheres {
            radius_voxels,
            centres,
        }
    }

    /// True when `voxel` lies inside the sphere of one of `seat`'s own living
    /// beacons: spec section 6's "spheres give passive vision", the one sight
    /// rule the live view draws with (decisions-log items 107 (6), 108 (1) and
    /// 110 (5)).
    ///
    /// Reuses the interpreter's `within`, so a sphere here and a sphere in a
    /// playbook's `Build` placement check are one rule. Moves no hashed state.
    #[must_use]
    pub fn in_own_sphere(&self, seat: SeatId, voxel: [i32; 3]) -> bool {
        self.spheres().contains(seat, voxel)
    }
}

#[cfg(test)]
mod tests {
    use super::Spheres;
    use crate::math::fixed::Fx;
    use crate::tables::SeatId;

    fn at(x: i16, y: i16, z: i16) -> [Fx; 3] {
        [Fx::from_voxels(x), Fx::from_voxels(y), Fx::from_voxels(z)]
    }

    /// The boundary, written out: a radius of 3 reaches (3, 0, 0) and not
    /// (4, 0, 0), and it reaches (2, 2, 1) -- 4 + 4 + 1 = 9 -- but not
    /// (2, 2, 2) -- 12.
    #[test]
    fn the_boundary_voxel_is_inside_and_the_next_is_not() {
        let spheres = Spheres {
            radius_voxels: 3,
            centres: vec![(1, at(10, 10, 10))],
        };
        let seat = SeatId::new(1);
        assert!(spheres.contains(seat, [13, 10, 10]));
        assert!(!spheres.contains(seat, [14, 10, 10]));
        assert!(spheres.contains(seat, [12, 12, 11]));
        assert!(!spheres.contains(seat, [12, 12, 12]));
        assert!(spheres.contains(seat, [7, 10, 10]), "and on the far side");
        assert!(!spheres.contains(seat, [6, 10, 10]));
    }

    #[test]
    fn a_sphere_is_its_owners_and_nobody_elses() {
        let spheres = Spheres {
            radius_voxels: 3,
            centres: vec![(1, at(10, 10, 10))],
        };
        assert!(spheres.contains(SeatId::new(1), [10, 10, 10]));
        assert!(!spheres.contains(SeatId::new(0), [10, 10, 10]));
        assert!(
            !spheres.contains(SeatId::new(1), [40_000, 10, 10]),
            "off the fixed-point range is seen by nobody"
        );
        assert_eq!(spheres.len(), 1);
        assert!(!spheres.is_empty());
        assert!(Spheres::default().is_empty());
    }
}
