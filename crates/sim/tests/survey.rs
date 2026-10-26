// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! S2's `surv`: full Survey (spec section 6's Survey row; S2's plan, task
//! `surv`, its Acceptance list under the names it gives them).
//!
//! The arrangements are put in the world through fixture seams
//! ([`World::place_beacon_directly`], [`World::field_unit`],
//! [`World::add_area`]), so each test isolates one rule; the committed
//! scenario `scenarios/s2/survey-finds-the-enemy` plays them from a sealed
//! playbook. Numbers are read from the rules table and the module's constants,
//! never pinned.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules, not an integration test's helpers. A panic is this file's failure report: these are panic lints, not determinism lints (AGENTS.md section 5)."
)]

use pharmakos_sim::events::{Event, EventKind};
use pharmakos_sim::knowledge::AssetId;
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::math::quantity::Damage;
use pharmakos_sim::runner::Runner;
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::survey::{REFRESH_INTERVAL, cell_of};
use pharmakos_sim::tables::{
    AreaKind, BeaconId, ColumnBox, PRIORITY_NORMAL, SeatId, UnitId, UnitKind,
};
use pharmakos_sim::world::{DamageOrder, DamageTarget, World, WorldConfig};
use pharmakos_sim::{MatchSettings, RulesTable, default_rules_path};
use std::path::PathBuf;

const SEED: u64 = 0x00F1_2E00_C0DE_0001;

fn rules() -> RulesTable {
    let path = default_rules_path().unwrap_or_else(|| PathBuf::from("rules/rules.v1.json"));
    RulesTable::load(&path).unwrap_or_else(|error| panic!("loading the rules table: {error}"))
}

fn world() -> World {
    World::new(&WorldConfig {
        match_seed: SEED,
        seats: 2,
        units_per_seat: 0,
        rules: rules(),
        match_settings: MatchSettings {
            segment_lengths_ms: vec![600_000],
            round_limit: 1,
        },
    })
    .unwrap_or_else(|error| panic!("generating the world: {error}"))
}

fn seat(raw: u8) -> SeatId {
    SeatId::new(raw)
}

fn core(world: &World, of: SeatId) -> BeaconId {
    world
        .beacons()
        .ids()
        .iter()
        .zip(world.beacons().seats())
        .filter(|(_, owner)| **owner == of.raw())
        .map(|(id, _)| *id)
        .min()
        .map(BeaconId::new)
        .expect("a core")
}

fn beacon_at(world: &World, beacon: BeaconId) -> [Fx; 3] {
    let row = world.beacons().row_of(beacon).expect("a live beacon");
    world.beacons().positions()[row.index()]
}

fn column(point: [Fx; 3]) -> [i32; 2] {
    [point[0].floor_voxels(), point[1].floor_voxels()]
}

fn ground(world: &World, from: [Fx; 3], dx: i32, dy: i32) -> [Fx; 3] {
    let [x, y] = column(from);
    world.standing_point([x + dx, y + dy, 0])
}

/// The midpoint of the two cores: far from both, where neither core's
/// autocannon reaches.
fn field(world: &World) -> [Fx; 3] {
    let [ax, ay] = column(beacon_at(world, core(world, seat(0))));
    let [bx, by] = column(beacon_at(world, core(world, seat(1))));
    world.standing_point([i32::midpoint(ax, bx), i32::midpoint(ay, by), 0])
}

/// A seat-0 Survey beacon at the field with one scout standing on it.
fn survey_post(world: &mut World) -> (BeaconId, UnitId) {
    let at = field(world);
    let post = world
        .place_beacon_directly(seat(0), at, MandateKind::Survey, PRIORITY_NORMAL)
        .expect("a beacon");
    world.set_scouts(post, 1);
    let scout = world
        .field_unit(seat(0), post, UnitKind::Scout, at)
        .expect("a scout");
    (post, scout)
}

/// An enemy raider homed to seat 1's core, put down at `at`: it walks home,
/// out of the post's sphere, and leaves a sighting behind.
fn passer_by(world: &mut World, at: [Fx; 3]) -> UnitId {
    let home = core(world, seat(1));
    world
        .field_unit(seat(1), home, UnitKind::Raider, at)
        .expect("a raider")
}

/// Take `unit` out of every seat-0 sensor at once: stand it beside seat 1's
/// core, so its sighting stops on this tick.
fn away(world: &mut World, unit: UnitId) {
    let home = beacon_at(world, core(world, seat(1)));
    let there = ground(world, home, 0, -6);
    assert!(world.place_unit_directly(unit, there), "a living unit");
}

fn pushing(world: World) -> Runner {
    let mut runner = Runner::new(world);
    assert_eq!(runner.begin_push(), Ok(true), "the Push begins");
    runner
}

fn play(runner: &mut Runner, ticks: u32, feed: &mut Vec<Event>) {
    for _ in 0..ticks {
        runner
            .step()
            .expect("the tick runs")
            .expect("still in the Push");
        feed.extend_from_slice(runner.events());
        runner.clear_events();
    }
}

fn going(world: &World, unit: UnitId) -> [Fx; 3] {
    let row = world.units().row_of(unit).expect("a live unit");
    world.units().destinations()[row.index()]
}

/// Where seat 0 remembers `asset`, and the tick it saw it on.
fn remembered(world: &World, asset: AssetId) -> ([Fx; 3], u32) {
    let table = world.sightings();
    let row = (0..table.seats().len())
        .find(|row| table.seats()[*row] == 0 && table.assets()[*row] == asset.raw())
        .expect("seat 0 remembers it");
    (table.places()[row], table.seen_at()[row])
}

/// Play until `tick` has been reached.
fn play_to(runner: &mut Runner, tick: u32, feed: &mut Vec<Event>) {
    let now = runner.world().tick().raw();
    play(runner, tick.saturating_sub(now), feed);
}

#[test]
fn scouts_scan_unexplored_ground_first() {
    let mut world = world();
    let (post, scout) = survey_post(&mut world);
    // A probe area three cells long to the east of the post, the scan's work.
    let at = beacon_at(&world, post);
    let [x, y] = column(at);
    let near = i16::try_from(x + 4).unwrap();
    let far = i16::try_from(x + 40).unwrap();
    let (low, high) = (i16::try_from(y - 4).unwrap(), i16::try_from(y + 4).unwrap());
    let area = ColumnBox::new([near, low], [far, high]).expect("a box the right way out");
    world
        .add_area(post, AreaKind::Probe, ground(&world, at, 20, 0), area)
        .expect("the probe area fits");
    // The area's cells, and the nearest one already explored, so the scan
    // must pass it over.
    let cells: Vec<u32> = (i32::from(near)..=i32::from(far))
        .flat_map(|cx| (i32::from(low)..=i32::from(high)).map(move |cy| [cx, cy]))
        .filter_map(cell_of)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    assert!(cells.len() >= 3, "the area spans several cells: {cells:?}");
    let first = cell_of([i32::from(near), y]).expect("a cell");
    assert!(world.mark_explored_directly(seat(0), first));

    let mut runner = pushing(world);
    let mut feed = Vec::new();
    play(&mut runner, 2, &mut feed);
    let target = cell_of(column(going(runner.world(), scout))).expect("a cell");
    assert!(
        cells.contains(&target) && target != first,
        "the scout walks to an unexplored cell of its probe area, not the explored one: {target}"
    );

    play(&mut runner, 1_200, &mut feed);
    let explored = runner.world().explored();
    assert!(
        cells.iter().all(|cell| explored.contains(seat(0), *cell)),
        "every cell of the area is explored"
    );
    let done = feed
        .iter()
        .filter(|event| event.kind == EventKind::AreaSurveyed && event.seat == Some(seat(0)))
        .count();
    assert_eq!(done, 1, "the scan reports its end once");
}

#[test]
fn the_stalest_sighting_is_refreshed_next() {
    let mut world = world();
    let (post, scout) = survey_post(&mut world);
    let at = beacon_at(&world, post);
    let east = ground(&world, at, 16, 0);
    let first = passer_by(&mut world, east);
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    play(&mut runner, 10, &mut feed);
    away(runner.world_mut(), first);
    play(&mut runner, 190, &mut feed);
    let second_at = ground(runner.world(), at, -16, 0);
    let second = passer_by(runner.world_mut(), second_at);
    play(&mut runner, 10, &mut feed);
    away(runner.world_mut(), second);
    play(&mut runner, 400, &mut feed);

    let first = AssetId::of_unit(first).unwrap();
    let second = AssetId::of_unit(second).unwrap();
    let (first_place, first_seen) = remembered(runner.world(), first);
    let (second_place, second_seen) = remembered(runner.world(), second);
    assert!(
        first_seen < second_seen,
        "the first walked out of sight earlier"
    );

    // Nothing is due yet: with no probe area there is nothing to scan, so
    // the scout keeps to its post.
    let due = REFRESH_INTERVAL.to_ticks_ceil();
    play_to(&mut runner, first_seen + due - 10, &mut feed);
    assert_ne!(going(runner.world(), scout), first_place);
    // The stalest is due first, and the scout goes to re-check it.
    play_to(&mut runner, first_seen + due + 6, &mut feed);
    assert_eq!(
        going(runner.world(), scout),
        first_place,
        "the stalest sighting is re-checked first"
    );
    // Once there, the first is in sight and not renewed: the next is next.
    play_to(&mut runner, second_seen + due + 6, &mut feed);
    assert_eq!(
        going(runner.world(), scout),
        second_place,
        "then the next stalest"
    );
    // The re-check found the first gone, and the seat forgot it.
    let table = runner.world().sightings();
    assert!(
        !(0..table.seats().len())
            .any(|row| table.seats()[row] == 0 && table.assets()[row] == first.raw()),
        "a re-check that finds nothing forgets the sighting"
    );
}

#[test]
fn enemy_beacons_come_first() {
    let mut world = world();
    let (post, scout) = survey_post(&mut world);
    let at = beacon_at(&world, post);
    // An enemy raider seen first, so its sighting is the older one...
    let west = ground(&world, at, -16, 0);
    let raider = passer_by(&mut world, west);
    // ...and an enemy beacon inside the post's sphere, seen until it is
    // destroyed a little later.
    let theirs = world
        .place_beacon_directly(
            seat(1),
            ground(&world, at, 16, 4),
            MandateKind::None,
            PRIORITY_NORMAL,
        )
        .expect("a beacon");
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    play(&mut runner, 10, &mut feed);
    away(runner.world_mut(), raider);
    play(&mut runner, 190, &mut feed);
    runner
        .world_mut()
        .request_damage(DamageOrder {
            target: DamageTarget::Beacon(theirs),
            amount: Damage::new(1_000_000).expect("a positive amount"),
            by: seat(0),
            source: None,
        })
        .expect("the order is filed");
    play(&mut runner, 10, &mut feed);

    let raider = AssetId::of_unit(raider).unwrap();
    let beacon = AssetId::of_beacon(theirs).unwrap();
    let (_, raider_seen) = remembered(runner.world(), raider);
    let (beacon_place, beacon_seen) = remembered(runner.world(), beacon);
    assert!(
        raider_seen < beacon_seen,
        "the raider's sighting is the older"
    );

    let due = REFRESH_INTERVAL.to_ticks_ceil();
    play_to(&mut runner, beacon_seen + due + 6, &mut feed);
    assert_eq!(
        going(runner.world(), scout),
        beacon_place,
        "an enemy beacon's sighting is re-checked before an older unit's"
    );
}

#[test]
fn a_survey_scout_reports_an_enemy_beacon_it_finds() {
    let mut world = world();
    let (post, _) = survey_post(&mut world);
    let at = beacon_at(&world, post);
    // An enemy beacon just past the post's sphere, inside a probe area: the
    // sphere does not see it, the scout scanning the area does.
    let reach = i32::from(rules().sphere_radius_voxels());
    let theirs_at = ground(&world, at, reach + 6, 0);
    let theirs = world
        .place_beacon_directly(seat(1), theirs_at, MandateKind::None, PRIORITY_NORMAL)
        .expect("a beacon");
    let [x, y] = column(theirs_at);
    let area = ColumnBox::new(
        [i16::try_from(x - 2).unwrap(), i16::try_from(y - 2).unwrap()],
        [i16::try_from(x + 2).unwrap(), i16::try_from(y + 2).unwrap()],
    )
    .expect("a box");
    world
        .add_area(post, AreaKind::Probe, theirs_at, area)
        .expect("the probe area fits");
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    play(&mut runner, 600, &mut feed);
    let beacon = AssetId::of_beacon(theirs).unwrap();
    let found: Vec<&Event> = feed
        .iter()
        .filter(|event| event.kind == EventKind::EnemyBeaconFound)
        .collect();
    assert_eq!(found.len(), 1, "found once: {found:?}");
    assert_eq!(found[0].seat, Some(seat(0)));
    assert_eq!(found[0].subject, Some(beacon));
}
