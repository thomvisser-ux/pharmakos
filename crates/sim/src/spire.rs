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

/// Whether a kind column holds a Resonance Spire's row, of any state: the
/// radius functions' fast exit. A freed row keeps its kind byte, so this can
/// answer yes for a table with no live Spire; it never answers no for one
/// that has one.
#[must_use]
pub fn holds_a_spire(kinds: &[u8]) -> bool {
    kinds.contains(&StructureKind::ResonanceSpire.id())
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
    // The fast exit: with no Spire row in the table, nothing grows, and the
    // kind column is one byte a row, so the test costs a byte search rather
    // than the six-column walk below (the tick asks this from per-unit and
    // per-beacon paths).
    if !holds_a_spire(structures.kinds()) {
        return rules.sphere_radius_voxels();
    }
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

/// How long a Resonance Spire takes to go up, from its queue to its last hit
/// point, while its beacon's fabricator works on it.
///
/// PLACEHOLDER: the Spire's build time, spec section 8's "about 60 s" — owner, at S3's `tune3`, which proposes its row.
/// Spec section 8: "A crystal lattice grows over about 60 s". No row prices
/// a fabricator's rate, so the time is this constant until `tune3` proposes
/// one (AGENTS.md section 12: a tuning value whose row does not exist yet).
pub const SPIRE_BUILD: crate::math::quantity::Ms = crate::math::quantity::Ms::new(60_000);

/// The fabricators' work on the capability structures going up, one tick of
/// it (spec section 8: "that beacon's built-in fabricator builds it whatever
/// the mandate"; S3's plan, E4), in the programs phase, in structure row
/// order.
///
/// A living capability structure of a seat, going up, homed to a beacon that
/// is alive and awake gains its share of the tick's hit points and is finished
/// at full. **The work goes through the Quartermaster's hold** (S3's plan,
/// E4; [`crate::power`]'s shortfall order): while the seat's grid is short,
/// draw above supply as the last power phase settled it, the fabricator's
/// construction is held exactly as its unit orders are, and resumes when the
/// headroom does. The structure's draw is unchanged by the hold: it draws from
/// the tick it was queued. A dark beacon's fabricator is powered down with
/// everything else homed to it (item 10), so its structure waits too; a
/// structure whose beacon died is a ruin and nobody's, and is never finished.
///
/// The hit points are spread over [`SPIRE_BUILD`] by the tick itself, not by
/// a counter: on tick `t` the structure gains `⌊(t + 1)·F / T⌋ − ⌊t·F / T⌋`
/// of its `F` full hit points, `T` being the build time in ticks, so any run of
/// `T` working ticks adds exactly `F`, with nothing new to hash or snapshot.
pub(crate) fn fabricate(world: &mut crate::world::World) {
    let tick = u64::from(world.tick().raw());
    let count = crate::math::index::row(world.structures().len());
    let mut row: usize = 0;
    while row < count {
        if let Some((id, grown, full)) = fabricated(world, row, tick) {
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

/// What tick `tick` adds to a structure of `full` hit points built over
/// `ticks` ticks: `⌊(tick + 1)·full / ticks⌋ − ⌊tick·full / ticks⌋`, exact in
/// `u64`. `None` for a build time of no ticks, which no constant here is.
fn share_of_tick(tick: u64, full: u64, ticks: u64) -> Option<u64> {
    let after = tick.checked_add(1)?.checked_mul(full)?.checked_div(ticks)?;
    let before = tick.checked_mul(full)?.checked_div(ticks)?;
    after.checked_sub(before)
}

/// The structure in `row` if a fabricator works on it this tick: its id, its
/// hit points after the tick's work, and its full hit points.
fn fabricated(
    world: &crate::world::World,
    row: usize,
    tick: u64,
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
    // The Quartermaster's hold: no construction while the seat's grid is short.
    let index = world.seat_row(seat)?;
    let supply = world.seats().supplies().get(index).copied()?;
    let draw = world.seats().draws().get(index).copied()?;
    if draw.raw() > supply.raw() {
        return None;
    }
    let full = world.structure_max_hp(kind)?;
    let id = structures.id_of(crate::ids::Row::of_index(row)?)?;
    let ticks = u64::from(SPIRE_BUILD.to_ticks_ceil());
    let share = share_of_tick(tick, u64::try_from(full.raw()).ok()?, ticks)?;
    // A structure going up holds fewer hit points than its full count, and a
    // tick's work stops at the full count: the build rule, as a drone raises a
    // Build target's structure. The sum fits: both terms are at most `full`.
    let grown = i64::from(hp.raw())
        .checked_add(i64::try_from(share).ok()?)?
        .min(i64::from(full.raw()));
    Some((
        id,
        crate::math::quantity::Hp::new(i32::try_from(grown).ok()?),
        full,
    ))
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
    if !holds_a_spire(&snapshot.structure_kind) {
        return rules.sphere_radius_voxels();
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::quantity::Kw;
    use crate::tables::SeatId;

    /// A licensed seat 0 with a Spire queued at its core, going up.
    fn queued() -> (crate::world::World, crate::tables::StructureId) {
        let path = crate::default_rules_path().unwrap_or_else(|| panic!("the rules table"));
        let rules = RulesTable::load(&path).unwrap_or_else(|error| panic!("it loads: {error:?}"));
        let mut world = crate::world::World::new(&crate::world::WorldConfig {
            match_seed: 0x0000_0000_ca5c_aded,
            seats: 2,
            rules,
            match_settings: crate::runner::MatchSettings {
                segment_lengths_ms: vec![180_000],
                round_limit: 1,
            },
        })
        .unwrap_or_else(|error| panic!("a world: {error:?}"));
        let seat = SeatId::new(0);
        world.award_licence_directly(seat, pharmakos_proto::gp::v1::Licence::ResonanceSpire, 1);
        world.set_treasury(seat, crate::math::quantity::Money::new(10_000));
        let row = world
            .beacons()
            .seats()
            .iter()
            .position(|owner| *owner == 0)
            .unwrap_or_else(|| panic!("seat 0's core"));
        let core = BeaconId::new(
            world
                .beacons()
                .ids()
                .get(row)
                .copied()
                .unwrap_or_else(|| panic!("an id")),
        );
        let spire = world
            .queue_capability(seat, core, StructureKind::ResonanceSpire)
            .unwrap_or_else(|failure| panic!("queued: {failure:?}"));
        (world, spire)
    }

    fn hp_of(world: &crate::world::World, spire: crate::tables::StructureId) -> i32 {
        let row = world
            .structures()
            .row_of(spire)
            .unwrap_or_else(|error| panic!("a row: {error:?}"))
            .index();
        world
            .structures()
            .hit_points()
            .get(row)
            .map_or(0, |hp| hp.raw())
    }

    /// E4: the Spire's construction goes through the Quartermaster's hold, so
    /// a seat whose grid is short (draw above supply as the power phase last
    /// settled it) raises nothing until its headroom is back; its draw rule is
    /// unchanged, since the draw is the power phase's and this reads it.
    #[test]
    fn a_shortfall_holds_the_spires_construction() {
        let (mut world, spire) = queued();
        let row = world
            .structures()
            .row_of(spire)
            .unwrap_or_else(|error| panic!("a row: {error:?}"))
            .index();
        let worked = |world: &crate::world::World| {
            (0_u64..40)
                .filter(|tick| fabricated(world, row, *tick).is_some())
                .count()
        };
        {
            let columns = world.seats_mut().power_columns();
            if let (Some(supply), Some(draw)) =
                (columns.supply.first_mut(), columns.draw.first_mut())
            {
                *supply = Kw::new(5);
                *draw = Kw::new(6);
            }
        }
        assert_eq!(worked(&world), 0, "held while the grid is short");
        {
            let columns = world.seats_mut().power_columns();
            if let Some(draw) = columns.draw.first_mut() {
                *draw = Kw::new(5);
            }
        }
        assert_eq!(
            worked(&world),
            40,
            "worked on every tick once the headroom is back"
        );
        let grown = (0_u64..1_200)
            .filter_map(|tick| fabricated(&world, row, tick))
            .map(|(_, grown, _)| grown.raw())
            .max();
        assert!(grown > Some(hp_of(&world, spire)), "and it grows");
    }

    /// Spec section 8's "about 60 s": the shares of any run of the build
    /// time's ticks add up to the full hit points, whatever tick it starts on.
    #[test]
    fn the_spire_takes_its_build_time() {
        let ticks = u64::from(SPIRE_BUILD.to_ticks_ceil());
        for start in [0_u64, 1, 7, 1_199, 123_457] {
            let total: u64 = (start..start + ticks)
                .map(|tick| share_of_tick(tick, 500, ticks).unwrap_or(0))
                .sum();
            assert_eq!(total, 500, "from tick {start}");
        }
        assert_eq!(ticks, 1_200, "sixty seconds at 20 Hz");
    }
}
