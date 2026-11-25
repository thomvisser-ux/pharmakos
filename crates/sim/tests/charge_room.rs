// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The planted charges' stated room (lane `simroom`, P4 of its review): a
//! carrier whose plant finds the charge table at its room keeps the charge in
//! hand, in the world, and plants it once a charge has fired and freed a
//! place. The arrangement is `tests/attack.rs`'s walled raid, whose helpers
//! are restated here.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules, not an integration test's helpers. A panic is this file's failure report: these are panic lints, not determinism lints (AGENTS.md section 5)."
)]

use pharmakos_sim::attack::{Aim, AttackSettings, Charge, ChargeState, PlantRefused};
use pharmakos_sim::encoding::Enc;
use pharmakos_sim::events::{Event, EventKind};
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::runner::{MatchPhase, Runner};
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::tables::{BeaconId, Common, PRIORITY_NORMAL, SeatId, UnitKind};
use pharmakos_sim::voxels::{Material, VoxelEdit};
use pharmakos_sim::world::{World, WorldConfig};
use pharmakos_sim::{MatchSettings, RulesTable, default_rules_path};
use std::path::PathBuf;

const SEED: u64 = 0x00F1_2E00_C0DE_0001;

/// `MandateSettings.Roe`'s `FREE_FIRE`.
const FREE_FIRE: u8 = 3;

/// The charge table's stated room in a two-seat match: one a beacon row
/// (`beacon_limit_of(2)`, 42).
const CHARGE_ROOM: usize = 42;

fn world() -> World {
    let path = default_rules_path().unwrap_or_else(|| PathBuf::from("rules/rules.v1.json"));
    let rules =
        RulesTable::load(&path).unwrap_or_else(|error| panic!("loading the rules table: {error}"));
    World::new(&WorldConfig {
        match_seed: SEED,
        seats: 2,
        rules,
        match_settings: MatchSettings {
            segment_lengths_ms: vec![600_000],
            round_limit: 3,
        },
    })
    .unwrap_or_else(|error| panic!("generating the world: {error}"))
}

fn column(point: [Fx; 3]) -> [i32; 2] {
    [point[0].floor_voxels(), point[1].floor_voxels()]
}

fn core_at(world: &World, seat: u8) -> [Fx; 3] {
    let row = world
        .beacons()
        .seats()
        .iter()
        .position(|owner| *owner == seat)
        .expect("a core");
    world.beacons().positions()[row]
}

/// The midpoint of the two cores, where neither core's autocannon reaches.
fn field(world: &World) -> [Fx; 3] {
    let [ax, ay] = column(core_at(world, 0));
    let [bx, by] = column(core_at(world, 1));
    world.standing_point([i32::midpoint(ax, bx), i32::midpoint(ay, by), 0])
}

fn voxel(at: [Fx; 3]) -> [i16; 3] {
    [
        i16::try_from(at[0].floor_voxels()).unwrap(),
        i16::try_from(at[1].floor_voxels()).unwrap(),
        i16::try_from(at[2].floor_voxels()).unwrap(),
    ]
}

/// A square ring of `of`'s wall voxels round `centre`, `half` columns out and
/// `height` high, filled down to the ground as a wall is.
fn wall_ring(world: &mut World, centre: [Fx; 3], half: i32, height: i32, of: SeatId) {
    let [cx, cy] = column(centre);
    let wall = Material::wall(of, false).expect("a wall id");
    for dy in -half..=half {
        for dx in -half..=half {
            if dx.abs() != half && dy.abs() != half {
                continue;
            }
            let (x, y) = (cx + dx, cy + dy);
            let top = world.standing_point([x, y, 0])[2].floor_voxels();
            for z in top..top + height {
                world
                    .request_voxel_edit(VoxelEdit::Set {
                        at: [x, y, z],
                        material: wall,
                        by: None,
                    })
                    .expect("a wall voxel is filed");
            }
        }
    }
}

/// `tests/attack.rs`'s walled raid: an Attack beacon of seat 0 aimed at a
/// voxel inside a ring of seat 1's wall, with `breach_allowed`, and one
/// raider; the treasury holds a charge.
fn walled() -> (Runner, BeaconId) {
    let mut fixture = world();
    let here = field(&fixture);
    let [x, y] = column(here);
    let inside = fixture.standing_point([x + 24, y, 0]);
    wall_ring(&mut fixture, inside, 4, 3, SeatId::new(1));
    let post = fixture
        .place_beacon_directly(SeatId::new(0), here, MandateKind::Attack, PRIORITY_NORMAL)
        .expect("a beacon");
    fixture.set_common(
        post,
        Common {
            roe: FREE_FIRE,
            retreat_hp_pct: 0,
        },
    );
    assert!(fixture.set_attack(
        post,
        AttackSettings {
            aim: Aim::Voxel(voxel(inside)),
            force_size: Some(1),
            waves: 1,
            breach: true,
            ..AttackSettings::default()
        },
        &[],
    ));
    fixture
        .field_unit(SeatId::new(0), post, UnitKind::Raider, here)
        .expect("a raider");
    let mut runner = Runner::new(fixture).unwrap_or_else(|error| panic!("{error}"));
    runner
        .world_mut()
        .step(&mut Enc::default())
        .expect("the walls settle");
    assert_eq!(runner.begin_push(), Ok(true));
    (runner, post)
}

/// Play until `done` holds, at most `limit` ticks, keeping every event.
fn play_until(
    runner: &mut Runner,
    limit: u32,
    log: &mut Vec<Event>,
    done: impl Fn(&World, &[Event]) -> bool,
) -> bool {
    for _ in 0..limit {
        if done(runner.world(), log) {
            return true;
        }
        if runner.phase() != MatchPhase::Push {
            break;
        }
        runner
            .step()
            .expect("the tick runs")
            .expect("still in the Push");
        log.extend_from_slice(runner.events());
        runner.clear_events();
    }
    done(runner.world(), log)
}

fn charge_of(world: &World, post: BeaconId) -> Option<ChargeState> {
    world
        .attack()
        .row_of(post)
        .and_then(|row| row.breach)
        .map(|breach| breach.charge)
}

fn first_tick(log: &[Event], kind: EventKind) -> Option<u32> {
    log.iter()
        .find(|event| event.kind == kind)
        .map(|event| event.tick.raw())
}

/// **A plant the charge table has no room for holds the charge in the
/// world**: once the carrier is loaded, the table is filled to its stated room
/// with charges planted elsewhere, one of them due soon. The carrier reaches
/// the wall and is refused: its charge stays `Loaded` in its breach,
/// `plant_held` counts the refusals, nothing is planted; the due charge fires
/// and frees a place, and the carrier plants on that same tick (the
/// charges fire before the carriers' programs run in a tick).
///
/// What bounds the hold: every charge in play is filed by a plant, which
/// fires it a fuse later (`attack.charge_fuse_ms`), so a full table frees a
/// place within one fuse of its oldest charge; a carrier holds at most that
/// long. (The far-future charges here are a fixture's, which no play files.)
#[test]
fn a_plant_past_the_charge_room_holds_the_charge_and_plants_once_one_fires() {
    let (mut runner, post) = walled();
    let mut log = Vec::new();
    let loaded = play_until(&mut runner, 4_000, &mut log, |world, _| {
        charge_of(world, post) == Some(ChargeState::Loaded)
    });
    assert!(loaded, "the breach's charge is bought and carried");
    assert_eq!(first_tick(&log, EventKind::ChargePlanted), None);

    let now = runner.world().tick().raw();
    let far = [
        2,
        2,
        runner.world().standing_point([2, 2, 0])[2].floor_voxels(),
    ];
    let held = runner.world().attack().charges().len();
    for index in held..CHARGE_ROOM {
        let fires_at = if index == held { now + 1_000 } else { u32::MAX };
        runner
            .world_mut()
            .plant_charge_directly(Charge {
                beacon: post,
                seat: SeatId::new(0),
                centre: far,
                fires_at,
            })
            .expect("a charge inside the room");
    }
    assert_eq!(
        runner.world_mut().plant_charge_directly(Charge {
            beacon: post,
            seat: SeatId::new(0),
            centre: far,
            fires_at: u32::MAX,
        }),
        Err(PlantRefused::Full),
        "the table is at its stated room"
    );

    let refused = play_until(&mut runner, 2_000, &mut log, |world, _| {
        world.attack_counts().plant_held > 0
    });
    assert!(refused, "the carrier reached the wall and found no room");
    assert_eq!(
        charge_of(runner.world(), post),
        Some(ChargeState::Loaded),
        "the refused carrier keeps its charge in hand"
    );
    assert_eq!(first_tick(&log, EventKind::ChargePlanted), None);

    let planted = play_until(&mut runner, 1_500, &mut log, |_, log| {
        first_tick(log, EventKind::ChargePlanted).is_some()
    });
    assert!(planted, "the carrier plants once a place is free");
    let fired = first_tick(&log, EventKind::ChargeDetonated).expect("the due charge fired");
    let planted_at = first_tick(&log, EventKind::ChargePlanted).expect("planted");
    assert_eq!(
        planted_at, fired,
        "the carrier plants on the tick the due charge fires: the charges fire before the \
         carriers' programs run"
    );
    assert!(runner.world().attack_counts().plant_held >= 1);
    assert_eq!(runner.world().attack().charges().len(), CHARGE_ROOM);
}

/// **A save at every tick of a Push with plants and firings restores**
/// (lane `simroom`, fix pass 2): the walled raid's charge is bought,
/// carried, planted and fires; a save is taken between every two ticks from
/// five ticks before the plant to past the firing. Every save restores, through
/// its bytes, and every restored chain equals the straight run's from that
/// tick. Every charge a save holds fires after the save's tick and no later
/// than a fuse after it: what the restore's charge check admits.
#[test]
fn a_save_at_every_tick_around_a_plant_and_a_firing_restores() {
    use pharmakos_sim::snapshot::Snapshot;
    const AFTER: u32 = 4;
    // A first run finds the plant's tick; the second saves from a few ticks
    // before it to a few after the firing, at every tick.
    let (mut probe, _) = walled();
    let mut log = Vec::new();
    let planted = play_until(&mut probe, 4_000, &mut log, |_, log| {
        first_tick(log, EventKind::ChargePlanted).is_some()
    });
    assert!(
        planted,
        "the breach's charge is bought, carried and planted"
    );
    let plant = first_tick(&log, EventKind::ChargePlanted).expect("planted");
    let (mut straight, _) = walled();
    let mut log = Vec::new();
    let near = play_until(&mut straight, 4_000, &mut log, |world, _| {
        world.tick().raw() + 5 >= plant
    });
    assert!(near);
    let fuse = straight
        .world()
        .rules()
        .attack()
        .charge_fuse
        .to_ticks_ceil();
    let mut saves: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut chain: Vec<(u32, u64)> = Vec::new();
    let mut held = 0_usize;
    let mut fired_at: Option<u32> = None;
    loop {
        let snapshot = straight.capture().expect("a save between ticks");
        for fires in &snapshot.attack.charge_fires {
            assert!(
                *fires > snapshot.tick && *fires <= snapshot.tick + fuse,
                "a save at tick {} holds a charge firing at {fires}, outside (tick, tick + {fuse}]",
                snapshot.tick
            );
            held += 1;
        }
        saves.push((
            snapshot.tick,
            snapshot.to_bytes().expect("the save encodes"),
        ));
        let report = straight
            .step()
            .expect("the tick runs")
            .expect("still in the Push");
        let tick = straight.world().tick().raw();
        chain.push((tick, report.hash));
        log.extend_from_slice(straight.events());
        straight.clear_events();
        if fired_at.is_none() {
            fired_at = first_tick(&log, EventKind::ChargeDetonated);
        }
        if fired_at.is_some_and(|fired| tick > fired + AFTER + 2) {
            break;
        }
        assert!(saves.len() < 4_000, "the charge fires");
    }
    assert!(held > 0, "the saves held the planted charge");
    // One runner takes every save in turn: a restore replaces the world whole.
    let (mut restored, _) = walled();
    for (index, (_, bytes)) in saves.iter().enumerate() {
        let save = Snapshot::from_bytes(bytes).expect("the save decodes");
        restored.restore(&save).expect("every save restores");
        for step in 0..AFTER {
            let Some(expected) = chain.get(index + usize::try_from(step).unwrap()) else {
                break;
            };
            let report = restored
                .step()
                .expect("the tick runs")
                .expect("still in the Push");
            restored.clear_events();
            assert_eq!(
                (restored.world().tick().raw(), report.hash),
                *expected,
                "restored from save {index}, the chain parts at step {step}"
            );
        }
    }
}
