// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The one sight rule a host draws with: a seat sees inside the spheres of its
//! own living beacons.
//!
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
//! Nothing here is hashed state and nothing here is read by a tick: it is a
//! question asked of the world, never an input to it. It does **not** include a
//! scout's own vision ([`crate::programs::UNIT_VISION_RADIUS_VOXELS`]) or
//! Survey-lite's recorded sightings ([`crate::survey`]). Item 108 (1) keeps
//! both out of the live view; whether scouts join it is the owner's question
//! (the wave-6 notes, section D, question 4). PLACEHOLDER: **OWNER**, now,
//! with S3's knowledge store for the sightings.

use crate::interpreter::cond::within;
use crate::math::fixed::Fx;
use crate::math::quantity::Hp;
use crate::tables::SeatId;
use crate::world::World;

/// Every living beacon's sphere, as an owned value a host can hold while the
/// world moves on.
///
/// Built by [`World::spheres`]. Rebuild it after anything that can place, lose
/// or move a beacon: a stale one draws last tick's spheres.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Spheres {
    /// `beacon.sphere_radius_voxels`, in whole voxels.
    radius_voxels: i32,
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
            .any(|(_, centre)| within(point, *centre, self.radius_voxels))
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
    pub const fn radius_voxels(&self) -> i32 {
        self.radius_voxels
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
    /// Every living beacon's sphere, owned, for a host that asks many times.
    ///
    /// Rows are read in table order, which is beacon id order, and a beacon at
    /// zero hit points is not a sphere (T14: a dead beacon's structures are
    /// ruins and it powers nothing).
    #[must_use]
    pub fn spheres(&self) -> Spheres {
        let radius_voxels = self
            .rules()
            .message()
            .beacon
            .as_ref()
            .and_then(|block| i32::try_from(block.sphere_radius_voxels).ok())
            .unwrap_or(0);
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
