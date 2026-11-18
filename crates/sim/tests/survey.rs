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
        rules: rules(),
        match_settings: MatchSettings {
            segment_lengths_ms: vec![600_000],
            round_limit: 1,
        },
    })
    .unwrap_or_else(|error| panic!("generating the world: {error}"))
}

/// The world with the scout's vision put back to 8: the two refresh-order
/// tests below stage two sightings 16 voxels either side of the post, and a
/// scout of the committed vision 24 (decision 17, rules revision 6) sees both
/// from the post at once, so it could not show which it re-checks first.
fn short_sighted_world() -> World {
    let mut message = rules().message().clone();
    message
        .units
        .as_mut()
        .and_then(|units| units.scout.as_mut())
        .expect("a scout row")
        .vision_radius_voxels = 8;
    World::new(&WorldConfig {
        match_seed: SEED,
        seats: 2,
        rules: RulesTable::from_message(&message)
            .unwrap_or_else(|error| panic!("the edited table: {error}")),
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

/// A world whose one Push lasts `push_ms`.
fn world_with(push_ms: i32) -> World {
    World::new(&WorldConfig {
        match_seed: SEED,
        seats: 2,
        rules: rules(),
        match_settings: MatchSettings {
            segment_lengths_ms: vec![push_ms],
            round_limit: 1,
        },
    })
    .unwrap_or_else(|error| panic!("generating the world: {error}"))
}

/// A seat-0 Survey beacon at the field, with no scout and a scout count of
/// nought, so its fabricator fields none.
fn survey_post(world: &mut World) -> BeaconId {
    let at = field(world);
    let post = world
        .place_beacon_directly(seat(0), at, MandateKind::Survey, PRIORITY_NORMAL)
        .expect("a beacon");
    // No count: each test fields its own scouts, when it wants them.
    world.set_scouts(post, 0);
    post
}

/// A Survey scout of `post`, standing on it.
fn scout_of(world: &mut World, post: BeaconId) -> UnitId {
    let at = beacon_at(world, post);
    world
        .field_unit(seat(0), post, UnitKind::Scout, at)
        .expect("a scout")
}

/// An enemy raider homed to seat 1's core, put down at `at`.
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

/// Destroy `beacon` at once, on seat 0's order.
fn destroy(world: &mut World, beacon: BeaconId) {
    world
        .request_damage(DamageOrder {
            target: DamageTarget::Beacon(beacon),
            amount: Damage::new(1_000_000).expect("a positive amount"),
            by: seat(0),
            source: None,
        })
        .expect("the order is filed");
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

/// Play until `tick` has been reached.
fn play_to(runner: &mut Runner, tick: u32, feed: &mut Vec<Event>) {
    let now = runner.world().tick().raw();
    play(runner, tick.saturating_sub(now), feed);
}

fn going(world: &World, unit: UnitId) -> [Fx; 3] {
    let row = world.units().row_of(unit).expect("a live unit");
    world.units().destinations()[row.index()]
}

/// Where seat 0 remembers `asset`, and the tick it saw it on, or `None`.
fn remembered(world: &World, asset: AssetId) -> Option<([Fx; 3], u32)> {
    let table = world.sightings();
    (0..table.seats().len())
        .find(|row| table.seats()[*row] == 0 && table.assets()[*row] == asset.raw())
        .map(|row| (table.places()[row], table.seen_at()[row]))
}

fn due() -> u32 {
    REFRESH_INTERVAL.to_ticks_ceil()
}

/// A probe area of `post`'s inside one row of cells, from `west` to `east`
/// columns east of the post, and the cells it overlaps west to east.
fn strip(world: &mut World, post: BeaconId, west: i32, east: i32) -> Vec<u32> {
    let at = beacon_at(world, post);
    let [x, y] = column(at);
    // One row of cells: the post's, clear of its edges.
    let row = y.div_euclid(16) * 16;
    let (low, high) = (row + 6, row + 10);
    let area = ColumnBox::new(
        [
            i16::try_from(x + west).unwrap(),
            i16::try_from(low).unwrap(),
        ],
        [
            i16::try_from(x + east).unwrap(),
            i16::try_from(high).unwrap(),
        ],
    )
    .expect("a box the right way out");
    world
        .add_area(post, AreaKind::Probe, ground(world, at, west, 0), area)
        .expect("the probe area fits");
    let cells: std::collections::BTreeSet<u32> = (x + west..=x + east)
        .filter_map(|cx| cell_of([cx, low]))
        .collect();
    cells.into_iter().collect()
}

#[test]
fn scouts_scan_unexplored_ground_first() {
    let mut world = world();
    let post = survey_post(&mut world);
    let cells = strip(&mut world, post, 4, 52);
    assert!(cells.len() >= 4, "the area spans several cells: {cells:?}");
    // The nearest cell is explored already, so the scan passes it over.
    assert!(world.mark_explored_directly(seat(0), cells[0]));
    let first = scout_of(&mut world, post);
    let second = scout_of(&mut world, post);

    let mut runner = pushing(world);
    let mut feed = Vec::new();
    play(&mut runner, 2, &mut feed);
    let target = |runner: &Runner, scout| cell_of(column(going(runner.world(), scout)));
    assert_eq!(
        target(&runner, first),
        Some(cells[1]),
        "the scout walks to the nearest unexplored cell of its probe area"
    );
    assert_eq!(
        target(&runner, second),
        Some(cells[2]),
        "and a second scout of the beacon takes the next one"
    );

    play(&mut runner, 1_600, &mut feed);
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

/// S3's `pred`, decision 7: an explored cell is due again once it is older
/// than reach memory, and a scout takes a due cell only when no unexplored
/// cell is left, by the same selection and order as an unexplored one: the
/// nearest scan point, ties to the lowest cell id (the main session's ruling).
/// Every cell of the area is explored at tick 0, and the nearest again a
/// little later; while none is due the scout has nothing to scan,
/// and once the earlier ones are due it walks to the nearest of them, passing
/// over the nearest cell, which is not due yet.
#[test]
fn a_cell_older_than_reach_memory_is_scanned_again_nearest_first() {
    let mut world = world();
    let post = survey_post(&mut world);
    let cells = strip(&mut world, post, 20, 68);
    assert!(cells.len() >= 3, "the area spans several cells: {cells:?}");
    for cell in &cells {
        assert!(world.mark_explored_directly(seat(0), *cell));
    }
    let scout = scout_of(&mut world, post);
    let window = world.rules().attack().reach_memory.to_ticks_floor();

    let mut runner = pushing(world);
    let mut feed = Vec::new();
    play(&mut runner, 200, &mut feed);
    // The nearest cell is explored again 200 ticks after the rest, so it
    // falls due 200 ticks later: a mark renews its tick.
    assert!(
        !runner.world_mut().mark_explored_directly(seat(0), cells[0]),
        "the nearest cell was explored already"
    );
    let target = |runner: &Runner| cell_of(column(going(runner.world(), scout)));
    play_to(&mut runner, window, &mut feed);
    assert!(
        !cells.contains(&target(&runner).expect("a cell")),
        "no cell is due yet, so the scout scans none of them: {:?}",
        target(&runner)
    );
    // Past reach memory the cells explored at tick 0 are due; the nearest
    // cell, explored at tick 200, is not.
    play_to(&mut runner, window + 10, &mut feed);
    assert_eq!(
        target(&runner),
        Some(cells[1]),
        "the scout walks to the nearest due cell, passing over the one not due yet"
    );
    // The scout reaches the due cell and explores it again: its explore tick
    // is renewed past the window (fix pass 1, C2: a renewal the scout made,
    // not one the fixture wrote).
    let mut renewed = None;
    while runner.world().tick().raw() < window + 1_200 {
        play(&mut runner, 1, &mut feed);
        if runner
            .world()
            .explored()
            .explored_on(seat(0), cells[1])
            .is_some_and(|tick| tick > window)
        {
            renewed = Some(runner.world().tick().raw());
            break;
        }
    }
    let renewed = renewed.expect("the scout scans the due cell again");
    // The first renewal's tick and the chain's hash on it, pinned: the one
    // committed chain of this lane that holds a re-scan (fix pass 1, C2). A
    // change that moves either names why.
    assert_eq!(
        (renewed, runner.world().state_hash()),
        (RENEWAL_TICK, RENEWAL_HASH),
        "the first renewal, and the chain on its tick"
    );
    // And it moves on to another cell that is due now, never back to the one
    // it has just renewed.
    play(&mut runner, 10, &mut feed);
    let now = runner.world().tick().raw();
    let next = target(&runner).expect("a cell");
    assert!(
        cells.contains(&next) && next != cells[1],
        "the scout moves on to another cell of the area: {next}"
    );
    let explored = runner
        .world()
        .explored()
        .explored_on(seat(0), next)
        .expect("an explored cell");
    assert!(
        now.saturating_sub(explored) > window,
        "and that cell is due: explored on {explored}, now {now}"
    );
}

/// The tick a Survey scout first renews a due cell in
/// `a_cell_older_than_reach_memory_is_scanned_again_nearest_first`.
const RENEWAL_TICK: u32 = 3_961;

/// The chain's hash on [`RENEWAL_TICK`].
const RENEWAL_HASH: u64 = 0xf795_cf68_97a4_d21c;

/// Fix pass 1, C1 (AGENTS.md section 4.8, the second place): a save and a
/// restore with both of snapshot 16's columns filled. A Survey scout has
/// marked cells of its area on different ticks (no renewal: a cell falls due
/// only after reach memory's 3600 ticks, which the re-scan test covers), and
/// seat 0 remembers seat 1's Generator; the
/// snapshot goes through its bytes and back, the restored world hashes as the
/// saved one did, both run on alike, and every explore tick and every sighted
/// blueprint is the saved one.
#[test]
fn a_save_keeps_every_explore_tick_and_every_sighted_blueprint() {
    let mut world = world();
    let post = survey_post(&mut world);
    let cells = strip(&mut world, post, 20, 68);
    let at = beacon_at(&world, post);
    let theirs = core(&world, seat(1));
    let near = ground(&world, at, 0, 6);
    world
        .raise_structure(
            seat(1),
            theirs,
            pharmakos_sim::tables::StructureKind::Generator,
            near,
        )
        .expect("room for a structure");
    let _scout = scout_of(&mut world, post);
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    play(&mut runner, 400, &mut feed);
    let explored = runner.world().explored();
    let ticks: std::collections::BTreeSet<u32> = cells
        .iter()
        .filter_map(|cell| explored.explored_on(seat(0), *cell))
        .collect();
    assert!(
        ticks.len() >= 2,
        "the scout marked cells of its area on different ticks: {ticks:?}"
    );
    let sightings = runner.world().sightings();
    assert!(
        sightings
            .blueprints()
            .iter()
            .any(|blueprint| *blueprint == pharmakos_sim::tables::StructureKind::Generator.id()),
        "seat 0 remembers seat 1's Generator"
    );

    let bytes = runner
        .capture()
        .expect("between ticks")
        .to_bytes()
        .expect("the snapshot encodes");
    let snapshot = pharmakos_sim::snapshot::Snapshot::from_bytes(&bytes).expect("the bytes decode");
    let before = runner.world().state_hash();
    let mut resumed = Runner::new(world_with(600_000));
    resumed.restore(&snapshot).expect("the snapshot restores");
    assert_eq!(
        resumed.world().state_hash(),
        before,
        "a restore is hash-identical"
    );
    assert_eq!(
        resumed.world().explored(),
        runner.world().explored(),
        "every explore tick is the saved one"
    );
    assert_eq!(
        resumed.world().sightings().blueprints(),
        runner.world().sightings().blueprints(),
        "every sighted blueprint is the saved one"
    );
    for _ in 0..200 {
        let one = runner.step().expect("a tick").map(|report| report.hash);
        let two = resumed.step().expect("a tick").map(|report| report.hash);
        assert_eq!(one, two, "the resumed run diverged from the saved one");
        runner.clear_events();
        resumed.clear_events();
    }
}

#[test]
fn the_stalest_sighting_is_refreshed_next() {
    let mut world = short_sighted_world();
    let post = survey_post(&mut world);
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

    let first = AssetId::of_unit(first).unwrap();
    let second = AssetId::of_unit(second).unwrap();
    let (first_place, first_seen) = remembered(runner.world(), first).expect("seen");
    let (second_place, second_seen) = remembered(runner.world(), second).expect("seen");
    assert!(first_seen < second_seen, "the first is the older");

    // Both due, and only then a scout: its first choice is between the two.
    play_to(&mut runner, second_seen + due() + 1, &mut feed);
    let scout = scout_of(runner.world_mut(), post);
    play(&mut runner, 6, &mut feed);
    assert_eq!(
        going(runner.world(), scout),
        first_place,
        "the stalest due sighting is re-checked first"
    );
    // There, the first is in sight and not renewed: it is forgotten, and the
    // next stalest is next.
    until_forgotten(&mut runner, first, &mut feed);
    play(&mut runner, 6, &mut feed);
    assert_eq!(
        remembered(runner.world(), first),
        None,
        "a re-check that finds nothing forgets the sighting"
    );
    assert_eq!(going(runner.world(), scout), second_place, "then the next");
}

#[test]
fn enemy_beacons_come_first() {
    let mut world = short_sighted_world();
    let post = survey_post(&mut world);
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
    destroy(runner.world_mut(), theirs);
    play(&mut runner, 10, &mut feed);

    let raider = AssetId::of_unit(raider).unwrap();
    let beacon = AssetId::of_beacon(theirs).unwrap();
    let (raider_place, raider_seen) = remembered(runner.world(), raider).expect("seen");
    let (beacon_place, beacon_seen) = remembered(runner.world(), beacon).expect("seen");
    assert!(
        raider_seen < beacon_seen,
        "the raider's sighting is the older"
    );

    // Both due, and only then a scout.
    play_to(&mut runner, beacon_seen + due() + 1, &mut feed);
    let scout = scout_of(runner.world_mut(), post);
    play(&mut runner, 6, &mut feed);
    assert_eq!(
        going(runner.world(), scout),
        beacon_place,
        "an enemy beacon's sighting is re-checked before an older unit's"
    );
    assert!(
        remembered(runner.world(), raider).is_some(),
        "the older unit's sighting was still there to choose"
    );
    // The beacon is gone; the raider is next.
    until_forgotten(&mut runner, beacon, &mut feed);
    play(&mut runner, 6, &mut feed);
    assert_eq!(going(runner.world(), scout), raider_place);
}

#[test]
fn the_refresh_pass_takes_over_before_the_lull() {
    // A Push of two minutes: its last minute is the refresh pass.
    let mut world = world_with(120_000);
    let post = survey_post(&mut world);
    let at = beacon_at(&world, post);
    // Far more ground than a scout scans in a Push.
    let cells = strip(&mut world, post, 40, 300);
    let east = ground(&world, at, 16, 0);
    let raider = passer_by(&mut world, east);
    let scout = scout_of(&mut world, post);
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    play(&mut runner, 10, &mut feed);
    away(runner.world_mut(), raider);
    let raider = AssetId::of_unit(raider).unwrap();
    let (place, seen) = remembered(runner.world(), raider).expect("seen");

    play_to(&mut runner, seen + due() - 5, &mut feed);
    assert_ne!(going(runner.world(), scout), place, "still scanning");
    play_to(&mut runner, seen + due() + 6, &mut feed);
    assert!(
        !cells
            .iter()
            .all(|cell| runner.world().explored().contains(seat(0), *cell)),
        "the scan is not done"
    );
    assert_eq!(
        going(runner.world(), scout),
        place,
        "in the Push's last refresh interval the due sighting comes before the scan"
    );
}

#[test]
fn a_survey_scout_reports_an_enemy_beacon_it_finds_and_its_refresh() {
    let mut world = world();
    let post = survey_post(&mut world);
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
    scout_of(&mut world, post);
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

    // The scan done, the scout goes home; once the sighting is due it goes
    // back, and the beacon, still standing, is refreshed.
    let (_, seen) = remembered(runner.world(), beacon).expect("seen");
    play_to(&mut runner, seen + due() + 600, &mut feed);
    let refreshed: Vec<&Event> = feed
        .iter()
        .filter(|event| event.kind == EventKind::SightingRefreshed)
        .collect();
    assert_eq!(refreshed.len(), 1, "refreshed once: {refreshed:?}");
    assert_eq!(refreshed[0].subject, Some(beacon));
    assert!(
        refreshed[0].value >= i64::from(REFRESH_INTERVAL.raw()),
        "its value is the age it replaced: {}",
        refreshed[0].value
    );
}

/// Play until seat 0 no longer remembers `asset`, for at most a minute.
fn until_forgotten(runner: &mut Runner, asset: AssetId, feed: &mut Vec<Event>) {
    for _ in 0..1_200 {
        if remembered(runner.world(), asset).is_none() {
            return;
        }
        play(runner, 1, feed);
    }
    panic!("the re-check never forgot it");
}
