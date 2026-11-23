// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Lane `srvy`: Survey's scouts refresh the sightings their seat holds before
//! the next Lull (spec section 6's Survey row, "They refresh those sightings
//! before the next Lull, so the playbook is written on fresh knowledge";
//! decisions-log item 13; item 173 (1): the sentence binds the refresh of
//! sightings a seat already holds, not a first discovery).
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

use pharmakos_sim::encoding::Enc;
use pharmakos_sim::events::{Event, EventKind};
use pharmakos_sim::knowledge::{AssetId, AssetKind};
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::math::quantity::Ms;
use pharmakos_sim::runner::{MatchPhase, Runner};
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::survey::{Drop, REFRESH_INTERVAL, REPRICE_DECISIONS};
use pharmakos_sim::tables::{
    AreaKind, BeaconId, ColumnBox, PRIORITY_NORMAL, SeatId, UnitId, UnitKind,
};
use pharmakos_sim::voxels::{Material, VoxelEdit};
use pharmakos_sim::world::{World, WorldConfig};
use pharmakos_sim::{MatchSettings, RulesTable, default_rules_path};
use std::fmt::Write as _;
use std::path::PathBuf;

/// The surv tests' seed (`tests/survey.rs`).
const SEED: u64 = 0x00F1_2E00_C0DE_0001;

/// The committed seeds and seat counts: Probation's seed 20 with two seats,
/// and the determinism and scenario seeds with two and three; and seed 2 with
/// three seats, where PR #210's first review found a scout displaced from a
/// sighting it had walked toward for minutes (fix pass 1, item 1); and seed
/// 29 with two seats, where review 3 found the commander's sighting two
/// columns behind seat 1's core left 725 s old after the scout that renewed
/// the core was killed (fix pass 3, item 2).
const COMMITTED: [(u64, u32); 7] = [
    (20, 2),
    (0x0102_0304_0506_0708, 2),
    (0x0102_0304_0506_0708, 3),
    (0x0000_0000_CA5C_ADED, 2),
    (0x0000_0000_CA5C_ADED, 3),
    (2, 3),
    (29, 2),
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

/// A world whose scout sees `vision` voxels, the committed table otherwise.
fn sighted_world(vision: u32, segment_lengths_ms: Vec<i32>) -> World {
    let mut message = rules().message().clone();
    message
        .units
        .as_mut()
        .and_then(|units| units.scout.as_mut())
        .expect("a scout row")
        .vision_radius_voxels = vision;
    World::new(&WorldConfig {
        match_seed: SEED,
        seats: 2,
        rules: RulesTable::from_message(&message)
            .unwrap_or_else(|error| panic!("the edited table: {error}")),
        match_settings: MatchSettings {
            segment_lengths_ms,
            round_limit: 1,
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

/// The age in ticks of the stalest sighting `of` holds, enemy beacons only
/// when `beacons`, or `None` when it holds none.
fn stalest(world: &World, of: SeatId, beacons: bool) -> Option<u32> {
    let table = world.sightings();
    let now = world.tick().raw();
    (0..table.seats().len())
        .filter(|row| {
            table.seats()[*row] == of.raw()
                && (!beacons || table.kinds()[*row] == AssetKind::Beacon.id())
        })
        .map(|row| now - table.seen_at()[row])
        .max()
}

/// One Lull of [`surveyed`]'s match.
struct Lull {
    round: u32,
    /// The stalest enemy-beacon sighting seat 0 holds, in ticks.
    beacon: Option<u32>,
    /// The stalest sighting of any kind seat 0 holds, in ticks.
    any: Option<u32>,
    /// Seat 0's living Survey scouts.
    scouts: usize,
    /// The scouts it lost in the Push before.
    lost: usize,
    /// Every sighting of any kind seat 0 holds past reach memory: its kind,
    /// asset id, age in ticks, and whether it is excused ([`excused`]).
    stale: Vec<(u8, u32, u32, Result<String, String>)>,
    /// Seat 0's Survey drops of the Push before, once each for every asset
    /// and reason (`Scratch::survey_drops`): the asset id, the reason and the
    /// last decision tick it was dropped on.
    drops: Vec<(u32, Drop, u32)>,
    /// The ticks of the Push before on which a scout of seat 0's died.
    deaths: Vec<u32>,
    /// Every scout seat 0 fielded during the Push before: the first decision
    /// tick it was decided on (the one after it was first met), its unit id,
    /// and whether that decision sent it off (its destination is not where it
    /// stood).
    fielded: Vec<(u32, u32, bool)>,
}

/// One decision tick of a Push, with where seat 0's living scouts stood and
/// were sent.
type Decision = (u32, Vec<([Fx; 3], [Fx; 3])>);

/// What [`excused`] reads of a Push: the ticks a scout of seat 0's died on,
/// every [`Decision`], and the tick the Push ended on.
struct Watched<'a> {
    drops: &'a [(u32, Drop, u32)],
    deaths: &'a [u32],
    sent: &'a [Decision],
    end: u32,
}

/// What [`play_push`] watches of one Push of seat 0's scouts.
#[derive(Default)]
struct Track {
    /// The ticks a scout of seat 0's died on.
    deaths: Vec<u32>,
    /// Every decision tick.
    sent: Vec<Decision>,
    /// Every scout fielded in the Push: its first decision tick, unit id,
    /// and whether that decision sent it off.
    fielded: Vec<(u32, u32, bool)>,
    /// The scouts met at a decision of the Push so far.
    decided: Vec<u32>,
    /// The scouts first met after the Push's first decision, waiting for the
    /// decision after.
    waiting: Vec<u32>,
}

/// Play the running Push to its end, extending `feed` with its events and
/// keeping `track` of seat 0's scouts.
fn play_push(runner: &mut Runner, feed: &mut Vec<Event>, track: &mut Track) {
    loop {
        let before = scouts_sent(runner.world());
        let stepped = runner.step().expect("the tick runs");
        feed.extend_from_slice(runner.events());
        runner.clear_events();
        if stepped.is_none() {
            return;
        }
        let world = runner.world();
        let now = world.tick().raw();
        let after = scouts_sent(world);
        if before
            .iter()
            .any(|(id, _, _)| !after.iter().any(|(held, _, _)| held == id))
        {
            track.deaths.push(now);
        }
        if world.is_decision_tick() {
            // A scout first met after the Push's first decision was fielded
            // in the Push: its first decision is the next one.
            for (id, at, to) in &after {
                if let Some(index) = track.waiting.iter().position(|held| held == id) {
                    track.waiting.remove(index);
                    track.fielded.push((now, *id, at != to));
                }
                if !track.decided.contains(id) {
                    if !track.sent.is_empty() {
                        track.waiting.push(*id);
                    }
                    track.decided.push(*id);
                }
            }
            track
                .sent
                .push((now, after.iter().map(|(_, at, to)| (*at, *to)).collect()));
        }
    }
}

/// Where seat 0's living scouts stand and are sent, by unit id.
fn scouts_sent(world: &World) -> Vec<(u32, [Fx; 3], [Fx; 3])> {
    let units = world.units();
    (0..units.ids().len())
        .filter(|row| {
            units.kinds()[*row] == UnitKind::Scout.id()
                && units.seats()[*row] == 0
                && units.hit_points()[*row].is_alive()
        })
        .map(|row| {
            (
                units.ids()[row],
                units.positions()[row],
                units.destinations()[row],
            )
        })
        .collect()
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

/// [`surveyed`]'s world: seat 0's Survey post twelve columns from its core
/// toward seat 1's, two scouts, and the far half of the map its probe area.
fn surveyed_world(seed: u64, seats: u32) -> World {
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
    world
}

/// One committed match played by the sim alone: seat 0's Survey post twelve
/// columns from its core toward seat 1's, two scouts, and the half of the map
/// on seat 1's side of the cores' midpoint (across the axis the cores lie
/// further apart on) as its probe area. At every Lull after the first, the
/// round, the stalest sightings seat 0 holds, its living scouts, the scouts
/// it lost and the sightings counted out of reach in the Push before.
fn surveyed(seed: u64, seats: u32) -> Vec<Lull> {
    let mut runner =
        Runner::new(surveyed_world(seed, seats)).unwrap_or_else(|error| panic!("{error}"));
    let mut feed: Vec<Event> = Vec::new();
    let mut lulls = Vec::new();
    let mut track = Track::default();
    let window = window();
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
            let drops: Vec<(u32, Drop, u32)> = runner
                .world_mut()
                .scratch_mut()
                .survey_drops()
                .iter()
                .filter(|(who, _, _, _)| *who == 0)
                .map(|(_, asset, why, at)| (*asset, *why, *at))
                .collect();
            let world = runner.world();
            let table = world.sightings();
            let now = world.tick().raw();
            let watched = Watched {
                drops: &drops,
                deaths: &track.deaths,
                sent: &track.sent,
                end: now,
            };
            let stale = (0..table.seats().len())
                .filter(|row| table.seats()[*row] == 0 && now - table.seen_at()[*row] > window)
                .map(|row| {
                    (
                        table.kinds()[row],
                        table.assets()[row],
                        now - table.seen_at()[row],
                        excused(world, &watched, table.assets()[row], table.places()[row]),
                    )
                })
                .collect();
            let track = core::mem::take(&mut track);
            lulls.push(Lull {
                round,
                beacon: stalest(world, seat(0), true),
                any: stalest(world, seat(0), false),
                scouts: live_scouts(world, seat(0)),
                lost,
                stale,
                drops,
                deaths: track.deaths,
                fielded: track.fielded,
            });
        }
        assert_eq!(runner.phase(), MatchPhase::Lull, "round {round}'s Lull");
        assert_eq!(runner.begin_push(), Ok(true), "round {round}'s Push");
        play_push(&mut runner, &mut feed, &mut track);
        if runner.phase() == MatchPhase::Recap {
            runner.end_recap().expect("the recap closes");
        }
        if runner.phase() != MatchPhase::Lull {
            break;
        }
    }
    lulls
}

/// Whether a stale sighting of `asset`, remembered at `place`, is excused,
/// and how (`Ok`), or why not (`Err`), from what its Push `watched`. Excused
/// only:
///
/// * when its Push dropped it as out of reach and for no other reason
///   (`Drop::OutOfReach` alone: never listed, and no scout alone could reach
///   it before the Lull); or
/// * when its last drop was a scout lost (`Drop::ScoutLost`), and on the
///   first decision after the last death before that drop either a scout of
///   the seat set off for it (the departure tick is the proof) or no living
///   scout of the seat, setting off then, could have reached it before the
///   Lull on the walk the router serves (each scout's walk and the ticks
///   left are the proof): the death came too late for any replacement,
///   which is the loss item 173 (2) accepts.
///
/// Every other combination, and a sighting no drop names, is not.
fn excused(
    world: &World,
    watched: &Watched<'_>,
    asset: u32,
    place: [Fx; 3],
) -> Result<String, String> {
    let mine: Vec<(Drop, u32)> = watched
        .drops
        .iter()
        .filter(|(dropped, _, _)| *dropped == asset)
        .map(|(_, why, at)| (*why, *at))
        .collect();
    if !mine.is_empty() && mine.iter().all(|(why, _)| *why == Drop::OutOfReach) {
        return Ok("out of reach alone".to_owned());
    }
    let Some((Drop::ScoutLost, at)) = mine.iter().max_by_key(|(_, at)| *at).copied() else {
        return Err(format!("dropped as {mine:?}"));
    };
    let Some(death) = watched
        .deaths
        .iter()
        .copied()
        .filter(|died| *died <= at)
        .max()
    else {
        return Err(format!(
            "a scout lost on {at} with no death before it: {mine:?}"
        ));
    };
    let Some((tick, scouts)) = watched.sent.iter().find(|(tick, _)| *tick > death) else {
        return Err(format!(
            "a scout lost on {death}, and no decision after it: {mine:?}"
        ));
    };
    if scouts.iter().any(|(_, to)| *to == place) {
        return Ok(format!(
            "a scout lost on {death}, the replacement set off on {tick}"
        ));
    }
    let left = watched.end - tick;
    let walks: Vec<u32> = scouts
        .iter()
        .map(|(from, _)| served_ticks(world, *from, place))
        .collect();
    if walks.iter().all(|walk| *walk > left) {
        return Ok(format!(
            "a scout lost on {death}; on {tick}, {left} ticks before the Lull, no scout could reach it (walks {walks:?})"
        ));
    }
    Err(format!(
        "a scout lost on {death}, and on {tick}, {left} ticks before the Lull, no scout set off though one could (walks {walks:?}): {mine:?}"
    ))
}

/// The property: on the committed seeds, at every Lull after seat 0 first
/// holds an enemy beacon's sighting, while one of its Survey scouts is alive,
/// every sighting of any kind it holds is inside reach memory, but for one
/// [`excused`] names: dropped as out of reach alone, or a scout lost with its
/// replacement's departure shown or the death too late for any replacement.
/// Any other stale sighting fails.
#[test]
fn a_held_enemy_beacon_sighting_is_fresh_at_every_later_lull() {
    let mut fresh: Vec<String> = Vec::new();
    let mut stale: Vec<String> = Vec::new();
    for (seed, seats) in COMMITTED {
        let mut held = false;
        for lull in surveyed(seed, seats) {
            let Some(age) = lull.beacon else {
                assert!(
                    !held,
                    "seed {seed:#x}/{seats}: a held sighting is only ever forgotten by a re-check that finds the beacon gone, and no beacon of seat 1's is destroyed here"
                );
                continue;
            };
            held = true;
            let any = lull.any.unwrap_or(age);
            let mut line = format!(
                "seed {seed:#x}/{seats}, round {}'s Lull: stalest enemy beacon {} ms, stalest of any kind {} ms, {} scouts alive, {} lost",
                lull.round,
                age * 50,
                any * 50,
                lull.scouts,
                lull.lost,
            );
            if lull.scouts == 0 {
                continue;
            }
            let mut unexcused: Vec<String> = Vec::new();
            for (kind, asset, age, verdict) in &lull.stale {
                match verdict {
                    Ok(how) => write!(line, "; asset {asset} stale, {how}").expect("a String"),
                    Err(why) => {
                        unexcused.push(format!("kind {kind} asset {asset} {} ms: {why}", age * 50));
                    }
                }
            }
            if unexcused.is_empty() {
                fresh.push(line);
            } else {
                stale.push(format!("{line}: {unexcused:?}"));
            }
        }
    }
    eprintln!(
        "fresh: {fresh:#?}
stale: {stale:#?}"
    );
    assert!(stale.is_empty(), "a held sighting went stale: {stale:#?}");
    assert!(
        fresh.len() >= 5,
        "enough Lulls follow a first sighting to make the property mean something: {fresh:#?}"
    );
}

/// Review 3's and 4's seed 29 with two seats, scripted (fix pass 4, item
/// 3): seat 1's commander, sighted at column (359, 22) behind seat 1's core,
/// in round 2's Push of five minutes. Its outcome on this head, exactly: the
/// three scouts seat 0 loses at the core (54 s, 271 s and 286 s into the
/// Push), a replacement after each, sent off on the first decision it is
/// decided on, and the commander's sighting 245 s old at round 3's Lull, last
/// dropped as a scout lost: on the decision after the third death, 13 s
/// before the Lull, neither living scout could reach it on the walk the
/// router serves (159 s and 174 s). Rounds 2 and 4 hold it fresh.
#[test]
fn seed_29_loses_the_commander_behind_the_core_to_its_third_scout() {
    let lulls = surveyed(29, 2);
    let at = |round: u32| {
        lulls
            .iter()
            .find(|lull| lull.round == round)
            .unwrap_or_else(|| panic!("round {round}'s Lull"))
    };
    for lull in &lulls {
        eprintln!(
            "round {}: stale {:?}, deaths {:?}, fielded {:?}, drops {:?}",
            lull.round, lull.stale, lull.deaths, lull.fielded, lull.drops
        );
    }
    let third = at(3);
    assert_eq!(
        third.deaths,
        vec![4699, 9037, 9337],
        "the three deaths at the core"
    );
    assert_eq!(
        third.fielded,
        vec![(4706, 4104, true), (9046, 8200, true), (9346, 4105, true)],
        "a replacement after each death, sent off on its first decision"
    );
    assert_eq!(
        third.stale.len(),
        1,
        "one stale sighting: {:?}",
        third.stale
    );
    let (kind, asset, age, verdict) = &third.stale[0];
    assert_eq!(
        (*kind, *asset, *age),
        (AssetKind::Commander.id(), 4, 4904),
        "the commander, 245 s old"
    );
    let how = verdict
        .as_ref()
        .unwrap_or_else(|why| panic!("not excused: {why}"));
    assert!(
        how == "a scout lost on 9337; on 9341, 259 ticks before the Lull, no scout could reach it (walks [3182, 3474])",
        "{how}"
    );
    assert!(
        at(2).stale.is_empty() && at(4).stale.is_empty(),
        "rounds 2 and 4 fresh"
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

/// An enemy raider homed to seat 1's core, put down at `at`.
fn passer_by(world: &mut World, at: [Fx; 3]) -> UnitId {
    let home = core(world, seat(1));
    world
        .field_unit(seat(1), home, UnitKind::Raider, at)
        .expect("a raider")
}

/// Take `unit` out of every seat-0 sensor: stand it beside seat 1's core.
fn away(world: &mut World, unit: UnitId) {
    let home = beacon_at(world, core(world, seat(1)));
    let there = ground(world, home, 0, -6);
    assert!(world.place_unit_directly(unit, there), "a living unit");
}

/// Destroy `unit` at once, on seat 1's order.
fn kill(runner: &mut Runner, unit: UnitId) {
    runner
        .world_mut()
        .request_damage(pharmakos_sim::world::DamageOrder {
            target: pharmakos_sim::world::DamageTarget::Unit(unit),
            amount: pharmakos_sim::math::quantity::Damage::new(1_000_000).expect("positive"),
            by: seat(1),
            source: None,
        })
        .expect("the order is filed");
}

/// A scout already walking to a due sighting keeps it when another scout of
/// its seat, in a lower unit row, becomes free: the newcomer takes the other
/// sighting (fix pass 1, item 1; the old rule gave the sighting to the lower
/// row and sent the walker elsewhere).
#[test]
fn a_scout_walking_to_a_sighting_keeps_it() {
    // Vision 8, so a scout at the post sees neither sighting's place.
    let mut world = sighted_world(8, vec![600_000]);
    let [last_x, last_y] = last_column(&world);
    let field = world.standing_point([last_x.div_euclid(2), last_y.div_euclid(2), 0]);
    let post = survey_post(&mut world, field);
    let east = ground(&world, field, 40, 0);
    let west = ground(&world, field, -40, 0);
    let first = passer_by(&mut world, east);
    let second = passer_by(&mut world, west);
    // A scout at each raider sights it; the one in the lower row is lost, so
    // its row is free for the newcomer.
    let lost = scout_at(&mut world, post, east);
    let walker = scout_at(&mut world, post, west);
    let mut runner = Runner::new(world).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(runner.begin_push(), Ok(true));
    let mut feed = Vec::new();
    play(&mut runner, 10, &mut feed);
    away(runner.world_mut(), first);
    away(runner.world_mut(), second);
    kill(&mut runner, lost);
    play(&mut runner, 10, &mut feed);
    let first = AssetId::of_unit(first).unwrap();
    let second = AssetId::of_unit(second).unwrap();
    let (first_place, first_seen) = remembered(runner.world(), first).expect("sighted");
    let (second_place, _) = remembered(runner.world(), second).expect("sighted");

    // Both due: the walker, alone, sets off for the first.
    let due = REFRESH_INTERVAL.to_ticks_ceil();
    let now = runner.world().tick().raw();
    play(&mut runner, first_seen + due + 10 - now, &mut feed);
    assert_eq!(
        going(runner.world(), walker),
        first_place,
        "the walker re-checks the first"
    );
    // A newcomer, in the freed lower row.
    let post_at = beacon_at(runner.world(), post);
    let newcomer = scout_at(runner.world_mut(), post, post_at);
    let row = |world: &World, unit: UnitId| world.units().row_of(unit).expect("alive").index();
    assert!(
        row(runner.world(), newcomer) < row(runner.world(), walker),
        "the arrangement needs the newcomer below the walker"
    );
    play(&mut runner, 6, &mut feed);
    assert_eq!(
        going(runner.world(), walker),
        first_place,
        "the walker keeps the sighting it is walking to"
    );
    assert_eq!(
        going(runner.world(), newcomer),
        second_place,
        "the newcomer takes the other"
    );
}

/// Where a scout stood when it decided, and the ticks the Push had left.
type Decided = ([Fx; 3], u32);

/// The departure rule, pinned at its tick: a scout that sees two voxels, so a
/// sighting is renewed only on arrival, sets off on a decision on which its
/// served walk to the sighting, plus the slack (twice the re-pricing cadence
/// in decision intervals, and one re-ask), is at least what the Push has
/// left; not while the last pricing decision before it found time; and
/// arrives before the Lull.
#[test]
fn a_scout_sets_off_on_the_decision_its_walk_needs() {
    let mut world = sighted_world(2, vec![300_000]);
    let [last_x, last_y] = last_column(&world);
    let field = world.standing_point([last_x.div_euclid(2), last_y.div_euclid(2), 0]);
    let post = survey_post(&mut world, field);
    let sphere = i32::from(rules().sphere_radius_voxels());
    let lone_at = ground(&world, field, -(sphere + 16), 0);
    let lone = world
        .place_beacon_directly(seat(1), lone_at, MandateKind::None, PRIORITY_NORMAL)
        .expect("a beacon");
    let [fx, _] = column(field);
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

    let interval = Ms::new(rules().decision_tick_ms()).to_ticks_ceil().max(1);
    let slack = 2 * interval * REPRICE_DECISIONS + runner.world().link_ticks();
    let started = runner.world().match_state().segment_started().raw();
    let end = started + runner.world().match_state().segment_ticks();
    // Per decision: where the scout stood and what the Push had left when it
    // decided; the departure is the decision that first sent it to the place.
    let mut before: Option<Decided> = None;
    let mut departure: Option<(u32, [Fx; 3], u32, Option<Decided>)> = None;
    while departure.is_none() && runner.world().tick().raw() < end {
        let world = runner.world();
        let row = world.units().row_of(scout).expect("alive").index();
        let at = world.units().positions()[row];
        let was = going(world, scout);
        play(&mut runner, 1, &mut feed);
        let world = runner.world();
        let tick = world.tick().raw();
        // The tick just played was a decision tick: the Push's first tick,
        // then every interval.
        let elapsed = tick - started;
        if elapsed == 0 || !(elapsed - 1).is_multiple_of(interval) {
            continue;
        }
        let left = end - tick;
        if was != place && going(world, scout) == place {
            departure = Some((tick, at, left, before));
        }
        // A pricing decision: the Push's first decision, then every
        // `REPRICE_DECISIONS`th.
        if (elapsed - 1)
            .div_euclid(interval)
            .is_multiple_of(REPRICE_DECISIONS)
        {
            before = Some((at, left));
        }
    }
    let (tick, at, left, before) = departure.expect("the scout sets off for the beacon");
    let walk = served_ticks(runner.world(), at, place);
    assert!(
        walk + slack >= left,
        "set off on tick {tick} with {left} ticks left, on a walk of {walk} ticks and a slack of {slack}"
    );
    let (then_at, then_left) = before.expect("a decision before the departure");
    let then_walk = served_ticks(runner.world(), then_at, place);
    assert!(
        then_walk + slack < then_left,
        "not a cadence late: at the pricing decision before, {then_left} ticks left on a walk of {then_walk}"
    );
    assert!(
        walk > REFRESH_INTERVAL.to_ticks_ceil(),
        "the walk is longer than the last minute's, so the fixed pass would not have done: {walk}"
    );
    play_out(&mut runner, &mut feed);
    let (_, last) = remembered(runner.world(), lone).expect("still held");
    assert!(last > first, "renewed on arrival, before the Lull");
}

/// File `edit`, stepping the world once when the edit queue is full.
fn file(world: &mut World, enc: &mut Enc, edit: VoxelEdit) {
    if world.request_voxel_edit(edit).is_err() {
        world.step(enc).expect("a tick");
        world.request_voxel_edit(edit).expect("room after a tick");
    }
}

/// A ring of stone three voxels high, `radius` columns round `centre`: a
/// wall no walker climbs, with no gap.
fn ring(world: &mut World, centre: [i32; 2], radius: i32) {
    let mut enc = Enc::with_capacity(64 * 1024);
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let d2 = dx * dx + dy * dy;
            if d2 < (radius - 1) * (radius - 1) || d2 > radius * radius {
                continue;
            }
            let (x, y) = (centre[0] + dx, centre[1] + dy);
            let Some(top) = world.voxels().top_solid_z(x, y) else {
                continue;
            };
            for z in top + 1..=top + 3 {
                file(
                    world,
                    &mut enc,
                    VoxelEdit::Set {
                        at: [x, y, z],
                        material: Material::STONE,
                        by: None,
                    },
                );
            }
        }
    }
    // Step until the graph has settled every edit filed.
    for _ in 0..3 {
        world.step(&mut enc).expect("a tick");
    }
}

/// A due sighting no walk reaches is no destination, and the drop is counted
/// (`Scratch::survey_unserved`), never silent (fix pass 1, item 5).
#[test]
fn a_sighting_no_walk_reaches_is_counted() {
    let mut world = sighted_world(8, vec![600_000]);
    let [last_x, last_y] = last_column(&world);
    let field = world.standing_point([last_x.div_euclid(2), last_y.div_euclid(2), 0]);
    let post = survey_post(&mut world, field);
    let inside = ground(&world, field, 40, 0);
    ring(&mut world, column(inside), 4);
    let inside = world.standing_point([column(inside)[0], column(inside)[1], 0]);
    let raider = passer_by(&mut world, inside);
    // A scout inside the ring sights the raider and is lost; the raider
    // leaves.
    let lost = scout_at(&mut world, post, inside);
    let mut runner = Runner::new(world).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(runner.begin_push(), Ok(true));
    let mut feed = Vec::new();
    play(&mut runner, 10, &mut feed);
    away(runner.world_mut(), raider);
    kill(&mut runner, lost);
    play(&mut runner, 10, &mut feed);
    let raider = AssetId::of_unit(raider).unwrap();
    let (place, seen) = remembered(runner.world(), raider).expect("sighted");

    // Due, and a scout at the post with nothing to scan: no walk reaches the
    // place, so the scout does not go, and the drop is counted.
    let due = REFRESH_INTERVAL.to_ticks_ceil();
    let now = runner.world().tick().raw();
    play(&mut runner, seen + due + 1 - now, &mut feed);
    let post_at = beacon_at(runner.world(), post);
    let scout = scout_at(runner.world_mut(), post, post_at);
    let before = runner.world_mut().scratch_mut().survey_unserved().unwrap();
    play(&mut runner, 6, &mut feed);
    let after = runner.world_mut().scratch_mut().survey_unserved().unwrap();
    assert_ne!(going(runner.world(), scout), place, "no walk reaches it");
    assert!(after > before, "the drop is counted: {before} then {after}");
}

/// `scouts` Survey scouts at the map's centre with a probe strip along the
/// south edge (always something to scan, far from the sightings), and one
/// unarmed enemy beacon at each of `offsets` from the centre, each sighted on
/// the Push's first decision by a scout that is then lost. One Push of
/// `push_ms`. Each beacon sighting's age at the Lull, in ticks, in the order
/// of `offsets`, and the Push's [`Drops`].
fn serial(world: World, scouts: usize, offsets: &[(i32, i32)]) -> (Vec<u32>, Drops) {
    let mut world = world;
    let [last_x, last_y] = last_column(&world);
    let (cx, cy) = (last_x.div_euclid(2), last_y.div_euclid(2));
    let field = world.standing_point([cx, cy, 0]);
    let post = survey_post(&mut world, field);
    let strip_at = world.standing_point([cx, 8, 0]);
    world
        .add_area(post, AreaKind::Probe, strip_at, boxed(0, 0, last_x, 40))
        .expect("the probe area fits");
    let mut beacons = Vec::new();
    let mut lost = Vec::new();
    for (dx, dy) in offsets {
        let at = world.standing_point([cx + dx, cy + dy, 0]);
        let owner = seat(1);
        beacons.push(
            world
                .place_beacon_directly(owner, at, MandateKind::None, PRIORITY_NORMAL)
                .expect("a beacon"),
        );
        lost.push(scout_at(&mut world, post, at));
    }
    for _ in 0..scouts {
        scout_at(&mut world, post, field);
    }
    let mut runner = Runner::new(world).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(runner.begin_push(), Ok(true));
    let mut feed = Vec::new();
    play(&mut runner, 10, &mut feed);
    for unit in lost {
        kill(&mut runner, unit);
    }
    play_out(&mut runner, &mut feed);
    let now = runner.world().tick().raw();
    let ages = beacons
        .iter()
        .map(|beacon| {
            let (_, seen) =
                remembered(runner.world(), AssetId::of_beacon(*beacon).unwrap()).expect("held");
            now - seen
        })
        .collect();
    let scratch = runner.world_mut().scratch_mut();
    let counted = [
        scratch.survey_under_threat().unwrap(),
        scratch.survey_scout_lost().unwrap(),
        scratch.survey_out_of_reach().unwrap(),
        scratch.survey_crowded_out().unwrap(),
        scratch.survey_drops_unlogged().unwrap(),
    ]
    .iter()
    .sum();
    let logged = scratch
        .survey_drops()
        .iter()
        .filter(|(who, _, _, _)| *who == 0)
        .map(|(_, asset, why, _)| (*asset, *why))
        .collect();
    let assets = beacons
        .iter()
        .map(|beacon| AssetId::of_beacon(*beacon).unwrap().raw())
        .collect();
    (
        ages,
        Drops {
            assets,
            logged,
            counted,
        },
    )
}

/// One Push's Survey drops of seat 0's: the beacons' asset ids in the order
/// of the offsets, the drop log, (asset id, reason) once each, and the sum
/// of the reason tallies.
struct Drops {
    assets: Vec<u32>,
    logged: Vec<(u32, Drop)>,
    counted: u64,
}

/// One scout and two beacons a hundred columns either side of its post, in
/// one Push of ten minutes: visiting one then the other fits easily, so both
/// are fresh at the Lull (PR #210's second review: the scout answered for
/// the first, left for it late, and had seven seconds left for the second).
#[test]
fn one_scout_refreshes_two_sightings_in_turn() {
    let (ages, drops) = serial(sighted_world(24, vec![600_000]), 1, &[(-100, 0), (100, 0)]);
    assert!(
        ages.iter().all(|age| *age <= window()),
        "every sighting fresh at the Lull: ages {ages:?} ticks"
    );
    assert_eq!(
        drops.counted, 0,
        "every sighting fits a list: {:?}",
        drops.logged
    );
}

/// Two scouts and three beacons: the same, with the work shared.
#[test]
fn two_scouts_refresh_three_sightings() {
    let (ages, drops) = serial(
        sighted_world(24, vec![600_000]),
        2,
        &[(-100, 0), (100, 0), (0, 120)],
    );
    assert!(
        ages.iter().all(|age| *age <= window()),
        "every sighting fresh at the Lull: ages {ages:?} ticks"
    );
    assert_eq!(
        drops.counted, 0,
        "every sighting fits a list: {:?}",
        drops.logged
    );
}

/// Three seats' worth of sightings and two scouts: the two enemy cores and a
/// forward beacon of seat 1's, each sighted on the Push's first decision by a
/// scout then lost, in one Push of eight minutes. The cores' autocannons
/// reach as far as a scout sees, so a refresh costs the scout, and the post
/// fields another (decisions-log item 173 (2)); every sighting is fresh at
/// the Lull.
#[test]
fn two_scouts_refresh_both_enemy_cores_and_a_forward_beacon() {
    let mut world = World::new(&WorldConfig {
        match_seed: SEED,
        seats: 3,
        rules: rules(),
        match_settings: MatchSettings {
            segment_lengths_ms: vec![480_000],
            round_limit: 1,
        },
    })
    .unwrap_or_else(|error| panic!("generating the world: {error}"));
    let [last_x, last_y] = last_column(&world);
    let (cx, cy) = (last_x.div_euclid(2), last_y.div_euclid(2));
    let field = world.standing_point([cx, cy, 0]);
    let post = survey_post(&mut world, field);
    world.set_scouts(post, 2);
    let strip_at = world.standing_point([cx, 8, 0]);
    world
        .add_area(post, AreaKind::Probe, strip_at, boxed(0, 0, last_x, 40))
        .expect("the probe area fits");
    let one = beacon_at(&world, core(&world, seat(1)));
    let two = beacon_at(&world, core(&world, seat(2)));
    let forward_at = world.standing_point([
        i32::midpoint(cx, column(one)[0]),
        i32::midpoint(cy, column(one)[1]),
        0,
    ]);
    let forward = world
        .place_beacon_directly(seat(1), forward_at, MandateKind::None, PRIORITY_NORMAL)
        .expect("a beacon");
    let mut lost = Vec::new();
    for at in [one, two, forward_at] {
        lost.push(scout_at(&mut world, post, at));
    }
    scout_at(&mut world, post, field);
    scout_at(&mut world, post, field);
    let assets = [
        AssetId::of_beacon(core(&world, seat(1))).unwrap(),
        AssetId::of_beacon(core(&world, seat(2))).unwrap(),
        AssetId::of_beacon(forward).unwrap(),
    ];
    let mut runner = Runner::new(world).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(runner.begin_push(), Ok(true));
    let mut feed = Vec::new();
    play(&mut runner, 10, &mut feed);
    for unit in lost {
        kill(&mut runner, unit);
    }
    play_out(&mut runner, &mut feed);
    let now = runner.world().tick().raw();
    let ages: Vec<u32> = assets
        .iter()
        .map(|asset| now - remembered(runner.world(), *asset).expect("held").1)
        .collect();
    assert!(
        ages.iter().all(|age| *age <= window()),
        "every sighting fresh at the Lull: ages {ages:?} ticks"
    );
}

/// A sighting no list reaches before the Lull is counted under a reason
/// (`survey::Drop`), never silently stale: one scout, three beacons spread
/// far, in a Push of four minutes. Each stale sighting is in the drop log,
/// and the tallies count each logged (asset, reason) once a Push, not once a
/// decision.
#[test]
fn a_sighting_no_list_reaches_is_counted() {
    let world = sighted_world(24, vec![240_000]);
    let (ages, drops) = serial(world, 1, &[(-150, 0), (150, 0), (0, -150)]);
    let window = window();
    let stale: Vec<usize> = (0..ages.len())
        .filter(|index| ages[*index] > window)
        .collect();
    assert!(
        !stale.is_empty(),
        "the arrangement leaves one stale: ages {ages:?}"
    );
    for index in &stale {
        let asset = drops.assets[*index];
        assert!(
            drops.logged.iter().any(|(dropped, _)| *dropped == asset),
            "stale sighting {index} is counted under a reason: ages {ages:?}, drops {:?}",
            drops.logged
        );
    }
    assert_eq!(
        drops.counted,
        u64::try_from(drops.logged.len()).unwrap(),
        "each (sighting, reason) is counted once a Push, not once a decision: {:?}",
        drops.logged
    );
    eprintln!("ages {ages:?}, drops {:?}", drops.logged);
}

/// Review 3's `star4-60`: one scout, four beacons sixty columns out on the
/// four compass points. The rank order's tour (192 s) does not fit the
/// window; a circular one does, and the insertion finds it.
#[test]
fn a_star_of_four_is_toured_round() {
    let (ages, _) = serial(
        sighted_world(24, vec![600_000]),
        1,
        &[(-60, 0), (60, 0), (0, 60), (0, -60)],
    );
    assert!(
        ages.iter().all(|age| *age <= window()),
        "every sighting fresh at the Lull: ages {ages:?} ticks"
    );
}
