// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Survey-lite: what a seat sees, and how stale the memory of it is (spec
//! section 6, "Vision and sightings").
//!
//! # What "reach" is at the skeleton, said plainly
//!
//! The spec's reach is "your spheres, the live vision of your own units and
//! structures, and Sensor Spire reveals". Two of those three exist here:
//!
//! * every **live beacon**'s sphere (`beacon.sphere_radius_voxels`), which is
//!   the passive vision every seat has from the first tick; and
//! * every living **scout**'s own vision
//!   ([`crate::programs::UNIT_VISION_RADIUS_VOXELS`]), which is what a Survey
//!   mandate buys by fielding scouts and sending them to its probe areas.
//!
//! Deliberately **not** here: vision from every unit and every structure. That
//! is the other half of the same sentence, and it belongs to the stage that
//! makes reach matter — Defend's engagement radius and Attack's freshness test
//! are both S2's, and adding it now would be a per-tick cost with nothing
//! reading it. Sensor Spire reveals are S3's. Both are additive: the viewer
//! list below grows, and nothing else changes.
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
//! could never be acted on any sooner, and the scan is quadratic in the number
//! of assets. The cadence is game time, identical on every machine, and it is
//! the same cadence the interpreter's own decisions run on.

use crate::knowledge::{AssetId, AssetKind};
use crate::math::fixed::{Fx, Sq};
use crate::tables::SeatId;
use crate::world::World;

/// Record what every seat can see, right now.
///
/// Walked seat by seat in ascending id, and within a seat viewer by viewer in
/// ascending row, so the order sightings are written in is a total order
/// (AGENTS.md §4.6) — which matters because a full table forgets its stalest
/// row to make space, and *which* row that is has to be the same everywhere.
pub(crate) fn record_sightings(world: &mut World) {
    let tick = world.tick().raw();
    let sphere = world.sphere_radius();
    let scout_vision = Fx::from_voxels(crate::programs::UNIT_VISION_RADIUS_VOXELS);
    let seats = usize::try_from(world.seats().len()).unwrap_or(0);
    let mut index: usize = 0;
    while index < seats {
        if !world.seats().is_alive(index) {
            index = index.saturating_add(1);
            continue;
        }
        let raw = world.seats().seats().get(index).copied().unwrap_or(0);
        let seat = SeatId::new(raw);
        look_from_beacons(world, seat, sphere, tick);
        look_from_scouts(world, seat, scout_vision, tick);
        index = index.saturating_add(1);
    }
}

/// Every live beacon's sphere is a pair of eyes.
fn look_from_beacons(world: &mut World, seat: SeatId, radius: Fx, tick: u32) {
    let count = world.beacons().len();
    let mut row: u32 = 0;
    while row < count {
        let at = usize::try_from(row).unwrap_or(usize::MAX);
        let mine = world.beacons().seats().get(at).copied() == Some(seat.raw());
        let live = crate::power::beacon_is_live(world, crate::tables::BeaconId::new(row));
        if mine
            && live
            && let Some(centre) = world.beacons().positions().get(at).copied()
        {
            look_around(world, seat, centre, radius, tick);
        }
        row = row.saturating_add(1);
    }
}

/// Every living scout's own vision.
fn look_from_scouts(world: &mut World, seat: SeatId, radius: Fx, tick: u32) {
    let count = world.units().len();
    let mut row: u32 = 0;
    while row < count {
        let at = usize::try_from(row).unwrap_or(usize::MAX);
        let mine = world.units().seats().get(at).copied() == Some(seat.raw());
        let scout =
            world.units().kinds().get(at).copied() == Some(crate::tables::UnitKind::Scout.id());
        let alive = world
            .units()
            .hit_points()
            .get(at)
            .is_some_and(|hp| hp.is_alive());
        if mine
            && scout
            && alive
            && let Some(centre) = world.units().positions().get(at).copied()
        {
            look_around(world, seat, centre, radius, tick);
        }
        row = row.saturating_add(1);
    }
}

/// Everything of another seat's within `radius` of `centre` becomes a sighting.
///
/// Only **another** seat's: a seat does not remember seeing its own things,
/// because it knows where they are. That is not an optimisation — it is what
/// keeps a sighting a memory of somebody else rather than a duplicate of the
/// unit table.
fn look_around(world: &mut World, seat: SeatId, centre: [Fx; 3], radius: Fx, tick: u32) {
    let limit = Sq::of_radius(radius);
    let units = world.units().len();
    let mut row: u32 = 0;
    while row < units {
        let at = usize::try_from(row).unwrap_or(usize::MAX);
        let owner = world.units().seats().get(at).copied().unwrap_or(0);
        let alive = world
            .units()
            .hit_points()
            .get(at)
            .is_some_and(|hp| hp.is_alive());
        if owner != seat.raw()
            && SeatId::new(owner).is_some()
            && alive
            && let Some(place) = world.units().positions().get(at).copied()
            && Sq::between(place, centre) <= limit
        {
            let kind = if world.units().kinds().get(at).copied()
                == Some(crate::tables::UnitKind::Commander.id())
            {
                AssetKind::Commander
            } else {
                AssetKind::Unit
            };
            let id = AssetId::of_unit(crate::tables::UnitId::new(row));
            world.see(seat, id, SeatId::new(owner), kind.id(), place, tick);
        }
        row = row.saturating_add(1);
    }

    let beacons = world.beacons().len();
    let mut row: u32 = 0;
    while row < beacons {
        let at = usize::try_from(row).unwrap_or(usize::MAX);
        let owner = world.beacons().seats().get(at).copied().unwrap_or(0);
        let alive = world
            .beacons()
            .hit_points()
            .get(at)
            .is_some_and(|hp| hp.is_alive());
        if owner != seat.raw()
            && SeatId::new(owner).is_some()
            && alive
            && let Some(place) = world.beacons().positions().get(at).copied()
            && Sq::between(place, centre) <= limit
        {
            let id = AssetId::of_beacon(crate::tables::BeaconId::new(row));
            world.see(
                seat,
                id,
                SeatId::new(owner),
                AssetKind::Beacon.id(),
                place,
                tick,
            );
        }
        row = row.saturating_add(1);
    }

    let structures = world.structures().len();
    let mut row: u32 = 0;
    while row < structures {
        let at = usize::try_from(row).unwrap_or(usize::MAX);
        let owner = world.structures().seats().get(at).copied().unwrap_or(0);
        let alive = world
            .structures()
            .hit_points()
            .get(at)
            .is_some_and(|hp| hp.is_alive());
        if owner != seat.raw()
            && SeatId::new(owner).is_some()
            && alive
            && let Some(place) = world.structures().positions().get(at).copied()
            && Sq::between(place, centre) <= limit
        {
            let id = AssetId::of_structure(crate::tables::StructureId::new(row));
            world.see(
                seat,
                id,
                SeatId::new(owner),
                AssetKind::Structure.id(),
                place,
                tick,
            );
        }
        row = row.saturating_add(1);
    }
}
