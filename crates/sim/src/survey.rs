// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Survey-lite: what a seat sees, and how stale the memory of it is (spec
//! section 6, "Vision and sightings").
//!
//! # What "reach" is, said plainly
//!
//! The spec's reach is "your spheres, the live vision of your own units and
//! structures, and Sensor Spire reveals". Since S2's `ids` lane the first two
//! come through the one sensor interface ([`crate::sight::Sensor`]; S2's plan,
//! E10), the knowledge store's output of it:
//!
//! * every **live beacon**'s sphere (`beacon.sphere_radius_voxels`), the
//!   passive vision every seat has from the first tick;
//! * every living **unit**'s own vision, its kind's `vision_radius_voxels`
//!   row (the commander's `commander.vision_radius_voxels`), which is what a
//!   Survey mandate buys by fielding scouts and sending them to its probe
//!   areas; and
//! * every finished **structure** a seat owns, its kind's
//!   `vision_radius_voxels` row.
//!
//! Each sees everything within its radius, with no line of sight (item 144
//! (4)); the line-of-sight model is the interface's seam
//! ([`crate::sight::LineOfSight`]). Sensor Spire reveals are S4's, and they
//! join as one more sensor.
//!
//! A unit is found through the broadphase grid, padded by the most a unit
//! moves in a tick and tested at its current position ([`World::units_within`];
//! E11), so a unit that walked into range since the grid was built is seen.
//! The beacon and structure tables are small and walked whole.
//!
//! # The age accrues on match game time
//!
//! A sighting stores the **tick it was taken at**, and its age is derived
//! against the current tick. That is what makes item 61's sentence true for
//! free: the age "accrues on match game time and runs across Pushes; the Lull
//! and the recap add nothing to it", because neither consumes a tick
//! ([`crate::runner`]). A stored age would have had to be advanced by somebody,
//! and whoever forgot to would have made the Lull free.
//!
//! # Why this runs on a decision tick
//!
//! Recording runs once per `match.decision_tick_ms` rather than every tick: a
//! sighting is read by a decision, so a sighting taken between two decisions
//! could never be acted on any sooner. The cadence is game time, identical on
//! every machine, and it is the same cadence the interpreter's own decisions run
//! on.
//!
//! # A sighting names the asset it saw
//!
//! A sighting's asset is an [`AssetId`] with its row's generation (S2's plan,
//! E1), so a memory of a unit whose row has since been reused stays a memory of
//! that unit, and the unit that took the row is a new sighting of its own.

use crate::ids::Row;
use crate::knowledge::{AssetId, AssetKind};
use crate::math::fixed::Sq;
use crate::sight::{RadiusOnly, Sensor};
use crate::tables::{BeaconId, SeatId, StructureId, UnitId, UnitKind};
use crate::world::World;

/// Record what every seat can see, right now.
///
/// Walked seat by seat in ascending id, and within a seat sensor by sensor in
/// the interface's order ([`World::sensor_at`]), so the order sightings are
/// written in is a total order (AGENTS.md §4.6) — which matters because a full
/// table forgets its stalest row to make space, and *which* row that is has to
/// be the same everywhere.
pub(crate) fn record_sightings(world: &mut World) {
    let tick = world.tick().raw();
    let mut found = core::mem::take(world.sight_rows_mut());
    let seats = crate::math::index::row(world.seats().len());
    let mut index: usize = 0;
    while index < seats {
        let seat = world.seats().seats().get(index).copied().map(SeatId::new);
        let alive = world.seats().is_alive(index);
        index = index.saturating_add(1);
        let Some(seat) = seat.filter(|_| alive) else {
            continue;
        };
        // One walk over every sensor, whose order is seat-blind, keeping this
        // seat's.
        let mut at: usize = 0;
        while let Some((sensor, next)) = world.sensor_at(at) {
            at = next;
            if sensor.seat == seat {
                look_around(world, &sensor, tick, &mut found);
            }
        }
    }
    found.clear();
    *world.sight_rows_mut() = found;
}

/// Everything of another seat's that `sensor` sees becomes a sighting.
///
/// Only **another** seat's: a seat does not remember seeing its own things,
/// because it knows where they are. That is not an optimisation — it is what
/// keeps a sighting a memory of somebody else rather than a duplicate of the
/// unit table.
fn look_around(world: &mut World, sensor: &Sensor, tick: u32, found: &mut Vec<Row>) {
    let seat = sensor.seat;
    let limit = Sq::of_radius(sensor.radius);
    world.units_within(sensor.centre, sensor.radius, found);
    for row in found.iter() {
        let units = world.units();
        let at = row.index();
        let (Some(owner), Some(hp), Some(kind), Some(place), Some(id)) = (
            units.seats().get(at).copied(),
            units.hit_points().get(at).copied(),
            units.kinds().get(at).copied(),
            units.positions().get(at).copied(),
            units.ids().get(at).copied(),
        ) else {
            continue;
        };
        if owner == seat.raw()
            || !SeatId::new(owner).is_some()
            || !hp.is_alive()
            || !sensor.sees(&RadiusOnly, place)
        {
            continue;
        }
        let kind = if kind == UnitKind::Commander.id() {
            AssetKind::Commander
        } else {
            AssetKind::Unit
        };
        if let Ok(id) = AssetId::of_unit(UnitId::new(id)) {
            world.see(seat, id, SeatId::new(owner), kind.id(), place, tick);
        }
    }

    let beacons = world.beacons().len();
    let mut row: u32 = 0;
    while row < beacons {
        let at = crate::math::index::row(row);
        row = row.saturating_add(1);
        let b = world.beacons();
        let (Some(owner), Some(hp), Some(place), Some(id)) = (
            b.seats().get(at).copied(),
            b.hit_points().get(at).copied(),
            b.positions().get(at).copied(),
            b.ids().get(at).copied(),
        ) else {
            continue;
        };
        if owner != seat.raw()
            && SeatId::new(owner).is_some()
            && hp.is_alive()
            && Sq::between(place, sensor.centre) <= limit
            && sensor.sees(&RadiusOnly, place)
            && let Ok(id) = AssetId::of_beacon(BeaconId::new(id))
        {
            world.see(
                seat,
                id,
                SeatId::new(owner),
                AssetKind::Beacon.id(),
                place,
                tick,
            );
        }
    }

    let structures = world.structures().len();
    let mut row: u32 = 0;
    while row < structures {
        let at = crate::math::index::row(row);
        row = row.saturating_add(1);
        let s = world.structures();
        let (Some(owner), Some(hp), Some(place), Some(id)) = (
            s.seats().get(at).copied(),
            s.hit_points().get(at).copied(),
            s.positions().get(at).copied(),
            s.ids().get(at).copied(),
        ) else {
            continue;
        };
        if owner != seat.raw()
            && SeatId::new(owner).is_some()
            && hp.is_alive()
            && Sq::between(place, sensor.centre) <= limit
            && sensor.sees(&RadiusOnly, place)
            && let Ok(id) = AssetId::of_structure(StructureId::new(id))
        {
            world.see(
                seat,
                id,
                SeatId::new(owner),
                AssetKind::Structure.id(),
                place,
                tick,
            );
        }
    }
}
