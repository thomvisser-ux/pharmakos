// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Resonance Spire (spec section 8; S3's plan, E3, E4 and decision 6;
//! S3's `lic`).
//!
//! # The sphere, derived (E3)
//!
//! A beacon's sphere radius is **derived, never stored**: the base row,
//! `beacon.sphere_radius_voxels`, plus `structures.
//! resonance_spire_sphere_bonus_voxels` while a finished Resonance Spire of the
//! beacon's own seat stands homed to it. The structure rows are already hashed
//! and snapshotted, so the larger sphere adds no column and a restore cannot
//! disagree with it. [`radius_voxels`] is the one function every reader
//! decision 6 says grows calls (through [`crate::world::World::sphere_radius_of`]
//! and its siblings); the readers it says do not grow keep the base row, by
//! name, and `tests/spire.rs`' `every_sphere_reader_is_named` fails on a reader
//! in neither list.
//!
//! **What grows** (decision 6): the sphere's own uses, spec section 5's "the
//! only range for physical work": the mandates' areas (Build, Mine, Defend's
//! guard escort, engagement radius and picket ring, Survey's area), the drones'
//! work reach (build, dig, repair, salvage), the sphere's coverage of vents and
//! seams for `covering`, the anchor for placing the next beacon, and the
//! sphere's place in the sensor seam's sphere output. **What does not grow:**
//! unit and structure vision (their own `vision_radius_voxels` rows, which
//! never read the sphere), mapgen's twice-the-sphere reach rule (vents are
//! placed against the base at generation), the commander's placement range
//! (`commander.interface_range_voxels`), and the sphere of a site where no
//! beacon stands yet, which no Spire can have grown.
//!
//! **On the Spire's destruction** the radius returns to its base on that tick,
//! since nothing is stored: what then lies outside the sphere stands and stays
//! powered, but no physical work reaches it until a Spire is rebuilt
//! (decision 6). A ruined Spire (its beacon gone, or its seat eliminated) is
//! nobody's, so it grows nothing.
//!
//! # The queue (E4)
//!
//! `queue_structure` is lifted for the Spire alone. A seat that holds the
//! licence queues it on site at any of its beacons, whatever the mandate: the
//! commander stays for `interface_times.queue_structure_ms`, the `$` leaves the
//! treasury the moment the row commits, the kW draw starts with it, and that
//! beacon's built-in fabricator builds it (spec section 8). One Spire per
//! beacon ("two spires interfere with each other"): a second is refused at run
//! time with the step failure `spire_present`, since the verifier's plan-time
//! answer can go stale. The Spire stands beside its beacon, on the nearest
//! free column of its sphere, so it is an asset of its own to aim at.

use crate::math::fixed::Fx;
use crate::rules::RulesTable;
use crate::tables::{BeaconId, BeaconTable, StructureKind, StructureTable};

/// The one blueprint id `queue_structure` accepts in S3
/// (`QueueStructureRow.blueprint_id`; S3's plan, E4).
pub const SPIRE_BLUEPRINT: &str = "resonance_spire";

/// The capability structure a `queue_structure` blueprint id names, or `None`
/// for an id S3 does not accept: every id but [`SPIRE_BLUEPRINT`] waits for
/// S4's capability catalogue.
#[must_use]
pub fn queued_kind(blueprint_id: &str) -> Option<StructureKind> {
    (blueprint_id == SPIRE_BLUEPRINT).then_some(StructureKind::ResonanceSpire)
}

/// One structure row as the radius reads it, so the live tables and a frozen
/// snapshot's columns are read by one rule.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StructureFacts {
    /// Its owner's seat byte.
    pub seat: u8,
    /// Its [`StructureKind::id`].
    pub kind: u8,
    /// Its home beacon's raw id.
    pub home: u32,
    /// Whether it is still going up.
    pub building: bool,
    /// Whether it has hit points left.
    pub alive: bool,
    /// Whether its row is free.
    pub free: bool,
}

impl StructureFacts {
    /// Whether this row is a finished, living Resonance Spire of `seat`
    /// homed to the beacon whose raw id is `beacon`.
    #[must_use]
    pub fn grows(&self, seat: u8, beacon: u32) -> bool {
        !self.free
            && self.alive
            && !self.building
            && self.seat == seat
            && self.home == beacon
            && self.kind == StructureKind::ResonanceSpire.id()
    }

    /// Whether this row is a living Resonance Spire of `seat` homed to the
    /// beacon whose raw id is `beacon`, finished **or still going up**: the
    /// one-Spire-per-beacon rule's test.
    #[must_use]
    pub fn stands(&self, seat: u8, beacon: u32) -> bool {
        !self.free
            && self.alive
            && self.seat == seat
            && self.home == beacon
            && self.kind == StructureKind::ResonanceSpire.id()
    }
}

/// Every row of a live structure table, as [`StructureFacts`].
pub fn facts_of(structures: &StructureTable) -> impl Iterator<Item = StructureFacts> + '_ {
    structures
        .seats()
        .iter()
        .zip(structures.kinds())
        .zip(structures.homes())
        .zip(structures.building())
        .zip(structures.hit_points())
        .zip(structures.frees())
        .map(
            |(((((seat, kind), home), building), hp), free)| StructureFacts {
                seat: *seat,
                kind: *kind,
                home: home.raw(),
                building: *building,
                alive: hp.is_alive(),
                free: *free,
            },
        )
}

/// **The one function** (E3): the sphere radius, in whole voxels, of the
/// beacon of `seat` whose raw id is `beacon`, given every structure row.
///
/// The base row, or the grown radius the rules table checked at load
/// ([`RulesTable::grown_sphere_radius_voxels`]) while a finished Spire of the
/// beacon's own seat stands homed to it. No arithmetic happens here, so no
/// sum can overflow inside a tick.
#[must_use]
pub fn radius_voxels(
    rules: &RulesTable,
    seat: u8,
    beacon: u32,
    mut structures: impl Iterator<Item = StructureFacts>,
) -> i16 {
    if structures.any(|facts| facts.grows(seat, beacon)) {
        rules.grown_sphere_radius_voxels()
    } else {
        rules.sphere_radius_voxels()
    }
}

/// [`radius_voxels`] for the beacon in `row` of the live tables; the base
/// radius for a row the table does not hold, which no Spire stands on.
#[must_use]
pub fn radius_at_row(
    rules: &RulesTable,
    beacons: &BeaconTable,
    structures: &StructureTable,
    row: usize,
) -> i16 {
    match (beacons.seats().get(row), beacons.ids().get(row)) {
        (Some(seat), Some(id)) => radius_voxels(rules, *seat, *id, facts_of(structures)),
        _ => rules.sphere_radius_voxels(),
    }
}

/// [`radius_voxels`] for `beacon` in the live tables, read through its id's
/// row and checked by generation; the base radius for an id that names no
/// row the table still holds.
#[must_use]
pub fn radius_of(
    rules: &RulesTable,
    beacons: &BeaconTable,
    structures: &StructureTable,
    beacon: BeaconId,
) -> i16 {
    match beacons.row_of(beacon) {
        Ok(row) => radius_at_row(rules, beacons, structures, row.index()),
        Err(_) => rules.sphere_radius_voxels(),
    }
}

/// The fabricators' work on the capability structures going up, one tick of
/// it (spec section 8: "that beacon's built-in fabricator builds it whatever
/// the mandate"; S3's plan, E4), in the programs phase, in structure row
/// order.
///
/// A living capability structure of a seat, going up, homed to a beacon that
/// is alive and awake gains a tick's hit points and is finished at full, as a
/// build drone raises a Build target's structure. A dark beacon's fabricator is
/// powered down with everything else homed to it (item 10), so its structure
/// waits; a structure whose beacon died is a ruin and nobody's, and is never
/// finished.
///
/// PLACEHOLDER: the fabricator's build rate, one build drone's `structures.build_hp_per_second` — owner, at S3's demo, with `tune3`'s Spire numbers.
/// Spec section 8 puts the lattice's growth at "about 60 s" as spectacle; at
/// the committed rows (500 hit points at 60 a second) the Spire finishes in
/// about eight seconds, and no row prices a fabricator's rate of its own.
pub(crate) fn fabricate(world: &mut crate::world::World) {
    let per_tick = world.rules().build_hp_per_tick();
    let count = crate::math::index::row(world.structures().len());
    let mut row: usize = 0;
    while row < count {
        if let Some((id, grown, full)) = fabricated(world, row, per_tick) {
            if let Some(slot) = world.structures_mut().hit_points_mut().get_mut(row) {
                *slot = grown;
            }
            if grown >= full {
                world.finish_structure(id);
            }
        }
        row = row.saturating_add(1);
    }
}

/// The structure in `row` if a fabricator works on it this tick: its id, its
/// hit points after the tick's work, and its full hit points.
fn fabricated(
    world: &crate::world::World,
    row: usize,
    per_tick: crate::math::quantity::Hp,
) -> Option<(
    crate::tables::StructureId,
    crate::math::quantity::Hp,
    crate::math::quantity::Hp,
)> {
    let structures = world.structures();
    if structures.frees().get(row).copied() != Some(false)
        || structures.building().get(row).copied() != Some(true)
    {
        return None;
    }
    let kind = StructureKind::from_id(structures.kinds().get(row).copied()?)?;
    if !kind.is_capability() {
        return None;
    }
    let seat = crate::tables::SeatId::new(structures.seats().get(row).copied()?);
    let hp = structures.hit_points().get(row).copied()?;
    if !seat.is_some() || !hp.is_alive() {
        return None;
    }
    let home = structures.homes().get(row).copied()?;
    let home_row = world.beacons().row_of(home).ok()?;
    if world.beacons().seats().get(home_row.index()).copied() != Some(seat.raw())
        || !crate::power::beacon_is_live(world, home)
    {
        return None;
    }
    let full = world.structure_max_hp(kind)?;
    let id = structures.id_of(crate::ids::Row::of_index(row)?)?;
    // A structure going up holds fewer hit points than its full count, and a
    // tick's work past the full count stops at it: the build rule, as a drone
    // raises a Build target's structure.
    let grown =
        crate::math::quantity::Hp::new(hp.raw().saturating_add(per_tick.raw()).min(full.raw()));
    Some((id, grown, full))
}

/// Every structure row of a frozen snapshot, as [`StructureFacts`], so a
/// condition read over the frozen snapshot sizes a sphere by the rule the live
/// world does (`tests/defend.rs`' `the_frozen_snapshot_ranks_as_the_live_world_does`).
pub fn facts_of_snapshot(
    snapshot: &crate::snapshot::Snapshot,
) -> impl Iterator<Item = StructureFacts> + '_ {
    snapshot
        .structure_seat
        .iter()
        .zip(&snapshot.structure_kind)
        .zip(&snapshot.structure_home)
        .zip(&snapshot.structure_building)
        .zip(&snapshot.structure_hp)
        .zip(&snapshot.structure_free)
        .map(
            |(((((seat, kind), home), building), hp), free)| StructureFacts {
                seat: *seat,
                kind: *kind,
                home: *home,
                building: *building != 0,
                alive: *hp > 0,
                free: *free != 0,
            },
        )
}

/// [`radius_voxels`] for the beacon in `row` of a frozen snapshot; the base
/// radius for a row the snapshot does not hold.
#[must_use]
pub fn radius_in_snapshot(
    rules: &RulesTable,
    snapshot: &crate::snapshot::Snapshot,
    row: usize,
) -> i16 {
    match (snapshot.beacon_seat.get(row), snapshot.beacon_id.get(row)) {
        (Some(seat), Some(id)) => radius_voxels(rules, *seat, *id, facts_of_snapshot(snapshot)),
        _ => rules.sphere_radius_voxels(),
    }
}

/// Whether `point` lies inside the sphere of the beacon in `row` of the live
/// tables, centred on `centre`: [`radius_at_row`]'s radius, asked so that the
/// structure table is walked only for a point between the base and the grown
/// radius, where the answer depends on a Spire. A sphere test over many
/// points of many beacons (a `covering` site's spiral) stays as cheap as the
/// base row's test was.
#[must_use]
pub fn inside_at_row(
    rules: &RulesTable,
    beacons: &BeaconTable,
    structures: &StructureTable,
    row: usize,
    centre: [Fx; 3],
    point: [Fx; 3],
) -> bool {
    let distance = crate::math::fixed::Sq::between(centre, point);
    if distance <= crate::math::fixed::Sq::of_radius(Fx::from_voxels(rules.sphere_radius_voxels()))
    {
        return true;
    }
    if distance
        > crate::math::fixed::Sq::of_radius(Fx::from_voxels(rules.grown_sphere_radius_voxels()))
    {
        return false;
    }
    radius_at_row(rules, beacons, structures, row) == rules.grown_sphere_radius_voxels()
}
