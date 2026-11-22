// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Lane `srvy`: Survey's scouts refresh the sightings their seat holds before
//! the next Lull (spec section 6's Survey row, "They refresh those sightings
//! before the next Lull, so the playbook is written on fresh knowledge";
//! decisions-log item 13; the owner's reading of 2026-11-22, that the
//! sentence binds the refresh of sightings a seat already holds, not a first
//! discovery).
//!
//! Two causes kept a held sighting stale, and each has a test of its own:
//!
//! * the refresh pass was the Push's last minute for every scout, and a scout
//!   scanning further than a minute's walk from the sighting could not reach
//!   it in that minute ([`a_scout_sets_off_in_time_for_a_far_sighting`]);
//! * a sighting stopped being due once it had left the reach window, and a
//!   Push is longer than the window, so a sighting the scan had kept the
//!   scouts from was out of the window by the pass and never re-checked
//!   ([`the_refresh_pass_rechecks_a_sighting_past_the_reach_window`]).
//!
//! [`a_held_enemy_beacon_sighting_is_fresh_at_every_later_lull`] is the
//! property on the committed seeds, played by the sim alone: a seat-0 Survey
//! post beside its core with two scouts and the far half of the map as its
//! probe area (the arrangement Easy's Survey post makes), four rounds on the
//! rules table's own ladder.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules, not an integration test's helpers. A panic is this file's failure report: these are panic lints, not determinism lints (AGENTS.md section 5)."
)]

use pharmakos_sim::events::{Event, EventKind};
use pharmakos_sim::knowledge::{AssetId, AssetKind};
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::runner::{MatchPhase, Runner};
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::survey::REFRESH_INTERVAL;
use pharmakos_sim::tables::{
    AreaKind, BeaconId, ColumnBox, PRIORITY_NORMAL, SeatId, UnitId, UnitKind,
};
use pharmakos_sim::world::{World, WorldConfig};
use pharmakos_sim::{MatchSettings, RulesTable, default_rules_path};
use std::path::PathBuf;

/// The surv tests' seed (`tests/survey.rs`).
const SEED: u64 = 0x00F1_2E00_C0DE_0001;

/// The committed seeds and seat counts: Probation's seed 20 with two seats,
/// and the determinism and scenario seeds with two and three.
const COMMITTED: [(u64, u32); 5] = [
    (20, 2),
    (0x0102_0304_0506_0708, 2),
    (0x0102_0304_0506_0708, 3),
    (0x0000_0000_CA5C_ADED, 2),
    (0x0000_0000_CA5C_ADED, 3),
];

/// The rounds the property plays: the ladder's three lengths and one more.
const ROUNDS: u32 = 4;

fn rules() -> RulesTable {
    let path = default_rules_path().unwrap_or_else(|| PathBuf::from("rules/rules.v1.json"));
    RulesTable::load(&path).unwrap_or_else(|error| panic!("loading the rules table: {error}"))
}

/// Reach memory in whole ticks, as Survey's re-scan reads it.
fn window() -> u32 {
    rules().attack().reach_memory.to_ticks_floor()
}

fn seat(raw: u8) -> SeatId {
    SeatId::new(raw)
}

fn world(seed: u64, seats: u32, segment_lengths_ms: Vec<i32>, round_limit: u32) -> World {
    World::new(&WorldConfig {
        match_seed: seed,
        seats,
        rules: rules(),
        match_settings: MatchSettings {
            segment_lengths_ms,
            round_limit,
        },
    })
    .unwrap_or_else(|error| panic!("generating the world: {error}"))
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

fn boxed(x0: i32, y0: i32, x1: i32, y1: i32) -> ColumnBox {
    ColumnBox::new(
        [i16::try_from(x0).unwrap(), i16::try_from(y0).unwrap()],
        [i16::try_from(x1).unwrap(), i16::try_from(y1).unwrap()],
    )
    .expect("a box the right way out")
}

/// The map's last column on each axis.
fn last_column(world: &World) -> [i32; 2] {
    let map = world.rules().message().map.as_ref().expect("a map block");
    [
        i32::try_from(map.size_x).unwrap() - 1,
        i32::try_from(map.size_y).unwrap() - 1,
    ]
}

/// A seat-0 Survey beacon at `at`, with no scout count, so its fabricator
/// fields none: each test fields its own.
fn survey_post(world: &mut World, at: [Fx; 3]) -> BeaconId {
    let post = world
        .place_beacon_directly(seat(0), at, MandateKind::Survey, PRIORITY_NORMAL)
        .expect("a beacon");
    world.set_scouts(post, 0);
    post
}

fn scout_at(world: &mut World, post: BeaconId, at: [Fx; 3]) -> UnitId {
    world
        .field_unit(seat(0), post, UnitKind::Scout, at)
        .expect("a scout")
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

/// Play the running Push to its end.
fn play_out(runner: &mut Runner, feed: &mut Vec<Event>) {
    while runner.step().expect("the tick runs").is_some() {
        feed.extend_from_slice(runner.events());
        runner.clear_events();
    }
}

/// Where seat 0 remembers `asset`, and the tick it saw it on, or `None`.
fn remembered(world: &World, asset: AssetId) -> Option<([Fx; 3], u32)> {
    let table = world.sightings();
    (0..table.seats().len())
        .find(|row| table.seats()[*row] == 0 && table.assets()[*row] == asset.raw())
        .map(|row| (table.places()[row], table.seen_at()[row]))
}

fn going(world: &World, unit: UnitId) -> [Fx; 3] {
    let row = world.units().row_of(unit).expect("a live unit");
    world.units().destinations()[row.index()]
}

/// The age in ticks of the stalest enemy-beacon sighting `of` holds, or
/// `None` when it holds none.
fn stalest_beacon(world: &World, of: SeatId) -> Option<u32> {
    let table = world.sightings();
    let now = world.tick().raw();
    (0..table.seats().len())
        .filter(|row| {
            table.seats()[*row] == of.raw() && table.kinds()[*row] == AssetKind::Beacon.id()
        })
        .map(|row| now - table.seen_at()[row])
        .max()
}

/// The ticks a scout takes, at the latest, on the walk the router serves
/// from `from` to `to`.
fn served_ticks(world: &World, from: [Fx; 3], to: [Fx; 3]) -> u32 {
    use pharmakos_sim::pathing::Speed;
    use pharmakos_sim::pathing::estimate::{walk_cost, walk_ticks};
    let surface = world.surface();
    let clusters = world.clusters();
    let mut scratch = pharmakos_sim::pathing::search::Scratch::for_map(surface, clusters.size())
        .expect("a scratch");
    let [fx, fy] = column(from);
    let [tx, ty] = column(to);
    let start = surface.node_of(fx, fy).expect("on the map");
    let goal = surface.node_of(tx, ty).expect("on the map");
    let walk = walk_cost(surface, clusters, &mut scratch, (start, goal)).expect("a walk");
    let speed = Speed {
        link_ticks: world.link_ticks(),
        ..Speed::of_kind(world.rules(), UnitKind::Scout)
    };
    let ticks = walk_ticks(walk, speed)
        .expect("priced")
        .expect("a scout walks");
    u32::try_from(ticks).expect("a tick count")
}

/// How many Survey scouts of `of`'s are alive.
fn live_scouts(world: &World, of: SeatId) -> usize {
    let units = world.units();
    units
        .kinds()
        .iter()
        .zip(units.seats())
        .zip(units.hit_points())
        .filter(|((kind, owner), hp)| {
            **kind == UnitKind::Scout.id() && **owner == of.raw() && hp.is_alive()
        })
        .count()
}

/// One committed match played by the sim alone: seat 0's Survey post twelve
/// columns from its core toward seat 1's, two scouts, and the half of the map
/// on seat 1's side of the cores' midpoint (across the axis the cores lie
/// further apart on) as its probe area. At every Lull after the first, the
/// round, the stalest enemy-beacon sighting seat 0 holds (in ticks), its
/// living scouts and the scouts it lost in the Push before.
fn surveyed(seed: u64, seats: u32) -> Vec<(u32, Option<u32>, usize, usize)> {
    let mut world = world(seed, seats, vec![180_000, 300_000, 480_000], ROUNDS);
    let mine = beacon_at(&world, core(&world, seat(0)));
    let theirs = beacon_at(&world, core(&world, seat(1)));
    let [mx, my] = column(mine);
    let [tx, ty] = column(theirs);
    let (dx, dy) = (tx - mx, ty - my);
    let length = (dx * dx + dy * dy).isqrt();
    let post_at = world.standing_point([
        mx + (dx * 12).div_euclid(length),
        my + (dy * 12).div_euclid(length),
        0,
    ]);
    let post = survey_post(&mut world, post_at);
    // Two scouts, and a count of two, so its fabricator replaces a lost one,
    // as Easy's post asks.
    world.set_scouts(post, 2);
    let [last_x, last_y] = last_column(&world);
    let area = if dx.abs() >= dy.abs() {
        let middle = i32::midpoint(mx, tx);
        if dx > 0 {
            boxed(middle, 0, last_x, last_y)
        } else {
            boxed(0, 0, middle, last_y)
        }
    } else {
        let middle = i32::midpoint(my, ty);
        if dy > 0 {
            boxed(0, middle, last_x, last_y)
        } else {
            boxed(0, 0, last_x, middle)
        }
    };
    world
        .add_area(post, AreaKind::Probe, theirs, area)
        .expect("the probe area fits");
    scout_at(&mut world, post, post_at);
    scout_at(&mut world, post, post_at);

    let mut runner = Runner::new(world).unwrap_or_else(|error| panic!("{error}"));
    let mut feed: Vec<Event> = Vec::new();
    let mut lulls = Vec::new();
    for round in 1..=ROUNDS {
        if round > 1 {
            let lost = feed
                .iter()
                .filter(|event| {
                    event.kind == EventKind::UnitDestroyed
                        && event.seat == Some(seat(0))
                        && event.value == i64::from(UnitKind::Scout.id())
                })
                .count();
            feed.clear();
            lulls.push((
                round,
                stalest_beacon(runner.world(), seat(0)),
                live_scouts(runner.world(), seat(0)),
                lost,
            ));
        }
        assert_eq!(runner.phase(), MatchPhase::Lull, "round {round}'s Lull");
        assert_eq!(runner.begin_push(), Ok(true), "round {round}'s Push");
        play_out(&mut runner, &mut feed);
        if runner.phase() == MatchPhase::Recap {
            runner.end_recap().expect("the recap closes");
        }
        if runner.phase() != MatchPhase::Lull {
            break;
        }
    }
    lulls
}

/// The property: on the committed seeds, at every Lull after seat 0 first
/// holds an enemy beacon's sighting, while one of its Survey scouts is alive,
/// every enemy-beacon sighting it holds is inside reach memory.
#[test]
fn a_held_enemy_beacon_sighting_is_fresh_at_every_later_lull() {
    let window = window();
    let mut fresh: Vec<String> = Vec::new();
    let mut stale: Vec<String> = Vec::new();
    for (seed, seats) in COMMITTED {
        let mut held = false;
        for (round, stalest, scouts, lost) in surveyed(seed, seats) {
            let Some(age) = stalest else {
                assert!(
                    !held,
                    "seed {seed:#x}/{seats}: a held sighting is only ever forgotten by a re-check that finds the beacon gone, and no beacon of seat 1's is destroyed here"
                );
                continue;
            };
            held = true;
            let line = format!(
                "seed {seed:#x}/{seats}, round {round}'s Lull: stalest enemy beacon {} ms, {scouts} scouts alive, {lost} lost in the Push",
                age * 50
            );
            if scouts == 0 {
                continue;
            }
            if age <= window {
                fresh.push(line);
            } else {
                stale.push(line);
            }
        }
    }
    eprintln!("fresh: {fresh:#?}\nstale: {stale:#?}");
    assert!(stale.is_empty(), "a held sighting went stale: {stale:#?}");
    assert!(
        fresh.len() >= 5,
        "enough Lulls follow a first sighting to make the property mean something: {fresh:#?}"
    );
}

/// A scout scanning far from a held sighting sets off in time to refresh it
/// before the Lull, though the walk back is longer than the Push's last
/// [`REFRESH_INTERVAL`].
#[test]
fn a_scout_sets_off_in_time_for_a_far_sighting() {
    // One Push of five minutes.
    let mut world = world(SEED, 2, vec![300_000], 1);
    // The map's centre: open ground, far from either core.
    let [last_x, last_y] = last_column(&world);
    let field = world.standing_point([last_x.div_euclid(2), last_y.div_euclid(2), 0]);
    let post = survey_post(&mut world, field);
    // An unarmed enemy beacon well outside the post's sphere, where the scout
    // starts, so the scout's first look sights it.
    let sphere = i32::from(rules().sphere_radius_voxels());
    let lone_at = ground(&world, field, -(sphere + 16), 0);
    let lone = world
        .place_beacon_directly(seat(1), lone_at, MandateKind::None, PRIORITY_NORMAL)
        .expect("a beacon");
    // Far more ground to the east than a scout scans in a Push, starting
    // well past the scout's sight of the beacon.
    let [fx, _] = column(field);
    let [last_x, last_y] = last_column(&world);
    world
        .add_area(
            post,
            AreaKind::Probe,
            ground(&world, field, 40, 0),
            boxed(fx + 40, 0, last_x, last_y),
        )
        .expect("the probe area fits");
    let scout = scout_at(&mut world, post, lone_at);
    let mut runner = Runner::new(world).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(runner.begin_push(), Ok(true));
    let mut feed = Vec::new();
    play(&mut runner, 10, &mut feed);
    let lone = AssetId::of_beacon(lone).unwrap();
    let (place, first) = remembered(runner.world(), lone).expect("sighted");

    // Scanning, the scout leaves the beacon behind; before the Push's last
    // minute it turns back for it, from further than a minute's walk.
    let end = runner.world().match_state().segment_ticks();
    let pass = end - REFRESH_INTERVAL.to_ticks_ceil();
    let mut set_off: Option<(u32, [Fx; 3])> = None;
    while runner.world().tick().raw() < pass {
        play(&mut runner, 1, &mut feed);
        let world = runner.world();
        let row = world.units().row_of(scout).expect("alive");
        let at = world.units().positions()[row.index()];
        if going(world, scout) != place {
            set_off = None;
        } else if set_off.is_none() {
            set_off = Some((world.tick().raw(), at));
        }
    }
    let (tick, from) =
        set_off.expect("the scout set off for the beacon before the Push's last minute");
    let walk = served_ticks(runner.world(), from, place);
    assert!(
        walk > REFRESH_INTERVAL.to_ticks_ceil(),
        "it set off on tick {tick} on a walk of {walk} ticks, longer than the last minute's"
    );

    play_out(&mut runner, &mut feed);
    let (_, last) = remembered(runner.world(), lone).expect("still held");
    let age = runner.world().tick().raw() - last;
    assert!(
        last > first && age <= window(),
        "refreshed before the Lull: seen on tick {first}, then {last}, {age} ticks before the Push ended"
    );
    assert!(
        feed.iter()
            .any(|event| event.kind == EventKind::SightingRefreshed && event.subject == Some(lone)),
        "the refresh is reported"
    );
}

/// A held sighting older than reach memory is still due a re-check: the
/// refresh pass takes it.
#[test]
fn the_refresh_pass_rechecks_a_sighting_past_the_reach_window() {
    // One long Push, and a post with no probe area, so nothing is left to
    // scan and every decision is the refresh pass's.
    let mut world = world(SEED, 2, vec![600_000], 1);
    // The map's centre: open ground, far from either core.
    let [last_x, last_y] = last_column(&world);
    let field = world.standing_point([last_x.div_euclid(2), last_y.div_euclid(2), 0]);
    let post = survey_post(&mut world, field);
    let sphere = i32::from(rules().sphere_radius_voxels());
    let lone_at = ground(&world, field, -(sphere + 16), 0);
    let lone = world
        .place_beacon_directly(seat(1), lone_at, MandateKind::None, PRIORITY_NORMAL)
        .expect("a beacon");
    // A first scout sights it and is lost at once.
    let first = scout_at(&mut world, post, lone_at);
    let mut runner = Runner::new(world).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(runner.begin_push(), Ok(true));
    let mut feed = Vec::new();
    play(&mut runner, 10, &mut feed);
    let lone = AssetId::of_beacon(lone).unwrap();
    runner
        .world_mut()
        .request_damage(pharmakos_sim::world::DamageOrder {
            target: pharmakos_sim::world::DamageTarget::Unit(first),
            amount: pharmakos_sim::math::quantity::Damage::new(1_000_000).expect("positive"),
            by: seat(1),
            source: None,
        })
        .expect("the order is filed");
    play(&mut runner, 10, &mut feed);
    assert_eq!(
        live_scouts(runner.world(), seat(0)),
        0,
        "the first scout is lost"
    );
    let (place, seen) = remembered(runner.world(), lone).expect("sighted");

    // Past the reach window, a second scout: it goes to re-check it.
    let out_of_window = seen + window() + 20;
    let now = runner.world().tick().raw();
    play(&mut runner, out_of_window - now, &mut feed);
    assert_eq!(
        remembered(runner.world(), lone).map(|(_, tick)| tick),
        Some(seen),
        "held, and not renewed: the post's sphere does not reach it"
    );
    let post_at = beacon_at(runner.world(), post);
    let second = scout_at(runner.world_mut(), post, post_at);
    play(&mut runner, 6, &mut feed);
    assert_eq!(
        going(runner.world(), second),
        place,
        "a sighting out of the reach window is re-checked, not left to go stale for good"
    );
}
