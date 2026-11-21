// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! S2's `atk`: the Attack mandate, hunting, the breach and the demolition
//! charge (S2's plan, task `atk`; its Acceptance list, under the names it
//! gives them).
//!
//! The arrangements are put in the world through fixture seams
//! ([`World::place_beacon_directly`], [`World::set_attack`],
//! [`World::field_unit`], [`World::request_voxel_edit`]), so each test
//! isolates one rule; the committed scenarios under `scenarios/s2/` play the
//! same rules from sealed playbooks. Numbers are read from the rules table,
//! never pinned.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules, not an integration test's helpers. A panic is this file's failure report: these are panic lints, not determinism lints (AGENTS.md section 5)."
)]

use pharmakos_proto::gp;
use pharmakos_sim::attack::{
    Aim, AttackSettings, BreachScratch, ChargeState, Phase, SealedWay, TargetSpec, breach_search,
};
use pharmakos_sim::encoding::Enc;
use pharmakos_sim::events::{Event, EventKind};
use pharmakos_sim::interpreter::{Action, Plan, Row};
use pharmakos_sim::knowledge::AssetId;
use pharmakos_sim::math::fixed::{Fx, Sq};
use pharmakos_sim::math::quantity::Damage;
use pharmakos_sim::pathing::route::{Surcharge, Zone, route, route_with};
use pharmakos_sim::pathing::search::Scratch;
use pharmakos_sim::runner::{MatchPhase, Runner, SealRefused};
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::snapshot::Snapshot;
use pharmakos_sim::tables::{BeaconId, Common, PRIORITY_NORMAL, SeatId, UnitId, UnitKind};
use pharmakos_sim::voxels::{Material, VoxelEdit};
use pharmakos_sim::world::{DamageOrder, DamageTarget, Roe, World, WorldConfig};
use pharmakos_sim::{MatchSettings, RulesTable, default_rules_path};
use std::path::PathBuf;

const SEED: u64 = 0x00F1_2E00_C0DE_0001;

/// `MandateSettings.Roe`'s wire values.
const RETURN_FIRE: u8 = 2;
const FREE_FIRE: u8 = 3;

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
            round_limit: 3,
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

fn beacon(world: &mut World, of: SeatId, at: [Fx; 3], mandate: MandateKind, roe: u8) -> BeaconId {
    let id = world
        .place_beacon_directly(of, at, mandate, PRIORITY_NORMAL)
        .expect("a beacon");
    world.set_common(
        id,
        Common {
            roe,
            retreat_hp_pct: 0,
        },
    );
    id
}

fn voxel(at: [Fx; 3]) -> [i16; 3] {
    [
        i16::try_from(at[0].floor_voxels()).unwrap(),
        i16::try_from(at[1].floor_voxels()).unwrap(),
        i16::try_from(at[2].floor_voxels()).unwrap(),
    ]
}

/// Every Attack setting a raid on `aim` needs, launching on `force` raiders
/// at the beacon itself.
fn raid(aim: Aim, force: u32) -> AttackSettings {
    AttackSettings {
        aim,
        force_size: Some(force),
        waves: 1,
        ..AttackSettings::default()
    }
}

fn attack(
    world: &mut World,
    of: SeatId,
    at: [Fx; 3],
    roe: u8,
    settings: AttackSettings,
) -> BeaconId {
    let id = beacon(world, of, at, MandateKind::Attack, roe);
    assert!(world.set_attack(id, settings, &[]), "an Attack row");
    id
}

fn unit(world: &mut World, of: SeatId, home: BeaconId, kind: UnitKind, at: [Fx; 3]) -> UnitId {
    world.field_unit(of, home, kind, at).expect("a unit")
}

fn unit_at(world: &World, id: UnitId) -> [Fx; 3] {
    world.units().positions()[world.units().row_of(id).expect("a live unit").index()]
}

fn within(a: [Fx; 3], b: [Fx; 3], voxels: i16) -> bool {
    Sq::between(a, b) <= Sq::of_radius(Fx::from_voxels(voxels))
}

fn pushing(world: World) -> Runner {
    let mut runner = Runner::new(world).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(runner.begin_push(), Ok(true), "the Push begins");
    runner
}

/// Play `ticks` ticks and keep every event they emitted.
fn play(runner: &mut Runner, ticks: u32, log: &mut Vec<Event>) {
    for _ in 0..ticks {
        if runner.phase() != MatchPhase::Push {
            return;
        }
        runner
            .step()
            .expect("the tick runs")
            .expect("still in the Push");
        log.extend_from_slice(runner.events());
        runner.clear_events();
    }
}

/// Play until `done` holds, at most `limit` ticks; `true` when it held.
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
        play(runner, 1, log);
    }
    done(runner.world(), log)
}

fn fired(log: &[Event], kind: EventKind) -> usize {
    log.iter().filter(|event| event.kind == kind).count()
}

fn row(world: &World, beacon: BeaconId) -> pharmakos_sim::attack::AttackRow {
    *world.attack().row_of(beacon).expect("an Attack row")
}

fn hit(world: &mut World, target: DamageTarget, amount: i64, by: SeatId) {
    world
        .request_damage(DamageOrder {
            target,
            amount: Damage::new(amount).expect("a positive amount"),
            by,
            source: None,
        })
        .expect("the order is filed");
}

/// A square ring of `seat`'s wall voxels round `centre`, `half` columns out
/// and `height` high, filled down to the ground as a wall is (`site`).
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

#[test]
fn an_attack_beacons_unset_roe_is_free_fire() {
    assert_eq!(Roe::of_setting(0, MandateKind::Attack), Some(Roe::Free));
    assert_eq!(Roe::of_setting(0, MandateKind::Build), Some(Roe::Return));
    assert_eq!(
        Roe::of_setting(RETURN_FIRE, MandateKind::Attack),
        Some(Roe::Return)
    );

    // An explicit RETURN_FIRE starts no fight: its force gathers and never
    // launches (item 148 (2)); an unset roe launches.
    let mut fixture = world();
    let here = field(&fixture);
    let aim = Aim::Voxel(voxel(ground(&fixture, here, 30, 0)));
    let quiet = attack(&mut fixture, seat(0), here, RETURN_FIRE, raid(aim, 1));
    let spot = ground(&fixture, here, 0, 40);
    let loud = attack(&mut fixture, seat(0), spot, 0, raid(aim, 1));
    unit(&mut fixture, seat(0), quiet, UnitKind::Raider, here);
    unit(&mut fixture, seat(0), loud, UnitKind::Raider, spot);
    let mut runner = pushing(fixture);
    let mut log = Vec::new();
    play(&mut runner, 20, &mut log);
    assert_eq!(row(runner.world(), quiet).phase, Phase::Gathering);
    assert_eq!(row(runner.world(), loud).phase, Phase::Launched);
    assert_eq!(fired(&log, EventKind::AttackLaunched), 1);
}

#[test]
fn an_attack_waits_for_its_force_size() {
    let mut fixture = world();
    let here = field(&fixture);
    let aim = Aim::Voxel(voxel(ground(&fixture, here, 30, 0)));
    // The staging point is a walk from the beacon, where a fielded raider
    // appears, so the third arrives well after the first two.
    let staging = ground(&fixture, here, 0, 12);
    let post = attack(
        &mut fixture,
        seat(0),
        here,
        FREE_FIRE,
        AttackSettings {
            staging: Some(voxel(staging)),
            ..raid(aim, 3)
        },
    );
    unit(&mut fixture, seat(0), post, UnitKind::Raider, staging);
    unit(&mut fixture, seat(0), post, UnitKind::Raider, staging);
    let mut runner = pushing(fixture);
    let mut log = Vec::new();
    play(&mut runner, 40, &mut log);
    assert_eq!(
        row(runner.world(), post).phase,
        Phase::Gathering,
        "two of three"
    );
    assert_eq!(fired(&log, EventKind::AttackLaunched), 0);
    // The fabricator fields the third on the work in hand, and the force
    // launches once it stands at the staging point.
    let launched = play_until(&mut runner, 2_000, &mut log, |world, _| {
        world
            .attack()
            .row_of(post)
            .is_some_and(|row| row.phase == Phase::Launched)
    });
    assert!(launched, "the force launches once it is three");
    let launch = log
        .iter()
        .find(|event| event.kind == EventKind::AttackLaunched)
        .expect("announced");
    assert_eq!(launch.value, 3, "with the three it waited for");
    assert!(
        fired(&log, EventKind::UnitFabricated) >= 1,
        "the third was fielded"
    );
}

/// A playbook whose one step deploys an Attack beacon at `x, y` aimed at
/// the enemy beacon `e_01`.
fn deploy_against(handle: &str) -> gp::v1::Playbook {
    let text = format!(
        r#"{{"schema_version":{{"major":1}},"meta":{{"title":"t","author_kind":"HUMAN"}},
        "kind":"PLAYBOOK","declarative":{{"route":[{{"label":"s","place_beacon":{{
        "at":{{"voxel":{{"x":96,"y":11,"z":62}}}},"initial":{{"mandate":{{"attack":{{
        "target":{{"beacon":{{"beacon_id":"{handle}"}}}},"retarget":"NONE",
        "staging":{{"x":96,"y":11,"z":62}},"launch":{{"segment_ms":0}},
        "after_action":"RETURN_HOME","waves":1}}}}}}}}}}]}},
        "on_death":{{"on_respawn":"CONTINUE","max_deaths_before_fallback":1}},
        "fallback":{{"hold":{{"at":{{"beacon_anchor":{{"safest":{{}}}}}}}}}}}}"#
    );
    pharmakos_proto::json::decode(&text).expect("a playbook")
}

/// The Attack target the compiled plan's deploy carries.
fn deployed_target(plan: &Plan) -> Option<TargetSpec> {
    let step = plan.route().first()?;
    let Action::PlaceBeacon { rows, .. } = &step.action else {
        return None;
    };
    rows.iter().find_map(|row| match row {
        Row::Settings {
            attack: Some(edit), ..
        } => edit.target,
        _ => None,
    })
}

#[test]
fn the_target_is_pinned_at_the_seal() {
    let rules = rules();
    let mut plan = Plan::compile(&deploy_against("e_01"), &rules).expect("it compiles");
    assert_eq!(
        deployed_target(&plan),
        Some(TargetSpec::Foreign {
            handle: 1,
            pinned: None
        }),
        "a handle compiles unpinned: the names are the gateway's"
    );
    let named = BeaconId::new(7);
    plan.pin_foreign(|handle| (handle == 1).then_some(named));
    assert_eq!(
        deployed_target(&plan),
        Some(TargetSpec::Foreign {
            handle: 1,
            pinned: Some(named)
        }),
        "the seal pins it to the beacon the handle names"
    );
    // A handle naming nothing at the seal stays unpinned, and fails its row
    // `no_target` when it commits (exec's `attack_aim`).
    let mut unknown = Plan::compile(&deploy_against("e_09"), &rules).expect("it compiles");
    unknown.pin_foreign(|_| None);
    assert!(matches!(
        deployed_target(&unknown),
        Some(TargetSpec::Foreign { pinned: None, .. })
    ));
    // An own beacon is no Attack target.
    assert!(Plan::compile(&deploy_against("b_01"), &rules).is_err());

    // And launch never re-tests it: a pinned beacon that is dead by the
    // launch is still where the force goes, and the arrival decides.
    let mut fixture = world();
    let here = field(&fixture);
    let spot = ground(&fixture, here, 20, 0);
    let target = beacon(&mut fixture, seat(1), spot, MandateKind::Build, 0);
    let post = attack(
        &mut fixture,
        seat(0),
        here,
        FREE_FIRE,
        raid(Aim::Beacon(target), 1),
    );
    unit(&mut fixture, seat(0), post, UnitKind::Raider, here);
    hit(
        &mut fixture,
        DamageTarget::Beacon(target),
        1_000_000,
        seat(1),
    );
    let mut runner = pushing(fixture);
    let mut log = Vec::new();
    play(&mut runner, 3, &mut log);
    assert_eq!(
        fired(&log, EventKind::AttackLaunched),
        1,
        "launched at the dead target"
    );
    let ended = play_until(&mut runner, 1_200, &mut log, |_, log| {
        fired(log, EventKind::AttackEnded) > 0
    });
    assert!(ended, "decided on arrival");
    let end = log
        .iter()
        .find(|event| event.kind == EventKind::AttackEnded)
        .unwrap();
    assert_eq!(
        end.value, 1,
        "the target is gone and NONE does not retarget"
    );
}

#[test]
fn a_seal_refuses_a_hunt_for_the_own_or_an_absent_commander() {
    let rules = rules();
    let hunting = |seat: u32| {
        let text = format!(
            r#"{{"schema_version":{{"major":1}},"meta":{{"title":"t","author_kind":"HUMAN"}},
            "kind":"PLAYBOOK","declarative":{{"route":[{{"label":"s","interface":{{
            "beacon":{{"beacon_id":"b_01"}},"rows":[{{"set_mandate_settings":{{"attack":{{
            "target":{{"enemy_commander":{{"seat":{seat}}}}}}}}}}}]}}}}]}},
            "on_death":{{"on_respawn":"CONTINUE","max_deaths_before_fallback":1}},
            "fallback":{{"hold":{{"at":{{"beacon_anchor":{{"safest":{{}}}}}}}}}}}}"#
        );
        let playbook: gp::v1::Playbook = pharmakos_proto::json::decode(&text).unwrap();
        Plan::compile(&playbook, &rules).expect("it compiles")
    };
    let mut runner = Runner::new(world()).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        runner.seal_playbook(seat(0), hunting(0)),
        Err(SealRefused::CommanderSeat {
            seat: seat(0),
            named: 0
        })
    );
    assert_eq!(
        runner.seal_playbook(seat(0), hunting(2)),
        Err(SealRefused::CommanderSeat {
            seat: seat(0),
            named: 2
        })
    );
    assert_eq!(runner.seal_playbook(seat(0), hunting(1)), Ok(()));
}

#[test]
fn a_hunted_commander_is_retargeted_while_seen_and_lost_when_not() {
    let mut fixture = world();
    let here = field(&fixture);
    let post = attack(
        &mut fixture,
        seat(0),
        here,
        FREE_FIRE,
        raid(Aim::Commander(Some(1)), 1),
    );
    let raider = unit(&mut fixture, seat(0), post, UnitKind::Raider, here);
    // Seat 1's commander, inside the Attack beacon's sphere, so the seat
    // sights it at its first decision tick.
    let commander = fixture.commander_of(seat(1)).expect("a commander");
    let near = ground(&fixture, here, 10, 0);
    assert!(fixture.place_unit_directly(commander, near));
    let mut runner = pushing(fixture);
    let mut log = Vec::new();
    let launched = play_until(&mut runner, 100, &mut log, |world, _| {
        world
            .attack()
            .row_of(post)
            .is_some_and(|row| row.phase == Phase::Launched)
    });
    assert!(launched, "a fresh sighting launches the hunt");
    let hunted = AssetId::of_unit(commander).unwrap().raw();
    assert_eq!(row(runner.world(), post).target, hunted);
    // While a sensor sees it, the hunt follows it live.
    let moved = ground(runner.world(), here, 14, 6);
    assert!(runner.world_mut().place_unit_directly(commander, moved));
    play(&mut runner, 2, &mut log);
    assert!(
        within(row(runner.world(), post).aim_at, moved, 1),
        "retargeted live"
    );
    // Gone where nothing sees it: the hunt walks to the last sighting, sees
    // nothing there, and the commander is lost.
    let far = ground(runner.world(), here, -120, -120);
    let last = row(runner.world(), post).aim_at;
    assert!(runner.world_mut().place_unit_directly(commander, far));
    let ended = play_until(&mut runner, 2_000, &mut log, |_, log| {
        fired(log, EventKind::AttackEnded) > 0
    });
    assert!(ended, "the hunt ends");
    let end = log
        .iter()
        .find(|event| event.kind == EventKind::AttackEnded)
        .unwrap();
    assert_eq!(
        end.value, 1,
        "the commander is lost, and NONE does not retarget"
    );
    assert!(
        !within(row(runner.world(), post).aim_at, far, 8),
        "it never learned where the commander went"
    );
    assert_eq!(row(runner.world(), post).phase, Phase::Gathering);
    let _ = last;
    let _ = unit_at(runner.world(), raider);
}

#[test]
fn avoid_known_threats_routes_around_range() {
    let world = world();
    let here = field(&world);
    let surface = world.surface();
    let clusters = world.clusters();
    let node = |at: [Fx; 3]| {
        let [x, y] = column(at);
        surface.node_of(x, y).expect("a column")
    };
    let start = node(ground(&world, here, 0, -30));
    let goal = node(ground(&world, here, 0, 30));
    let threat = ground(&world, here, 0, 0);
    let zone = [Zone::Disc {
        centre: [threat[0], threat[1], Fx::from_raw(0)],
        limit: Sq::of_radius(Fx::from_voxels(10)),
    }];
    let mut scratch = Scratch::for_map(surface, 32).expect("a scratch");
    let mut plain = Vec::new();
    route(surface, clusters, &mut scratch, start, goal, &mut plain).expect("a route");
    let mut avoiding = Vec::new();
    route_with(
        surface,
        clusters,
        &mut scratch,
        (start, goal),
        Surcharge::new(&zone, rules().attack().step_multiplier),
        &mut avoiding,
    )
    .expect("a route");
    let inside = |nodes: &[u32]| nodes.iter().filter(|n| zone[0].holds(surface, **n)).count();
    assert!(
        inside(&plain) > 0,
        "the direct route crosses the threat's range"
    );
    assert!(
        inside(&avoiding) < inside(&plain),
        "the surcharged route goes round it: {} < {}",
        inside(&avoiding),
        inside(&plain)
    );
    // And no surcharge is the route as it always was.
    let mut same = Vec::new();
    route_with(
        surface,
        clusters,
        &mut scratch,
        (start, goal),
        Surcharge::NONE,
        &mut same,
    )
    .unwrap();
    assert_eq!(same, plain);
}

/// An Attack beacon of seat 0 aimed at a voxel inside a ring of neutral wall,
/// with `breach_allowed`, and one raider; the treasury holds a charge.
fn walled(height: i32) -> (Runner, BeaconId, UnitId, [Fx; 3]) {
    let mut fixture = world();
    let here = field(&fixture);
    let inside = ground(&fixture, here, 24, 0);
    wall_ring(&mut fixture, inside, 4, height, seat(1));
    let post = attack(
        &mut fixture,
        seat(0),
        here,
        FREE_FIRE,
        AttackSettings {
            breach: true,
            ..raid(Aim::Voxel(voxel(inside)), 1)
        },
    );
    let raider = unit(&mut fixture, seat(0), post, UnitKind::Raider, here);
    let mut runner = Runner::new(fixture).unwrap_or_else(|error| panic!("{error}"));
    // The walls go in at a tick of their own, before the Push.
    runner
        .world_mut()
        .step(&mut Enc::default())
        .expect("the walls settle");
    assert_eq!(runner.begin_push(), Ok(true));
    (runner, post, raider, inside)
}

#[test]
fn a_breach_sends_a_charge_to_the_wall_and_resumes() {
    let (mut runner, post, raider, inside) = walled(3);
    let mut log = Vec::new();
    let breaching = play_until(&mut runner, 200, &mut log, |world, _| {
        world
            .attack()
            .row_of(post)
            .is_some_and(|row| row.breach.is_some())
    });
    assert!(breaching, "the sealed way finds a breach");
    let detonated = play_until(&mut runner, 4_000, &mut log, |_, log| {
        fired(log, EventKind::ChargeDetonated) > 0
    });
    assert!(
        detonated,
        "the charge is bought, carried, planted and fires"
    );
    assert_eq!(fired(&log, EventKind::ChargePlanted), 1);
    let planted = log
        .iter()
        .find(|event| event.kind == EventKind::ChargePlanted)
        .unwrap();
    assert_eq!(planted.seat, Some(seat(0)));
    // Through the gap, the raider reaches the voxel inside the ring.
    let through = play_until(&mut runner, 3_000, &mut log, |world, _| {
        within(unit_at(world, raider), inside, 2)
    });
    assert!(through, "the raider walks the gap");
    assert_eq!(
        fired(&log, EventKind::RegionCollapsed),
        0,
        "a breach cut drops nothing"
    );
    let counts = runner.world().attack_counts();
    assert_eq!(
        (counts.breach_exhausted, counts.breach_no_way),
        (0, 0),
        "no breach search is refused"
    );
}

#[test]
fn a_charge_cuts_flat_to_the_footing_and_the_gap_is_walkable() {
    let mut world = world();
    let here = field(&world);
    let [x, y] = column(here);
    let footing = world.standing_point([x, y, 0])[2].floor_voxels() - 1;
    // A wall of three, and whole stone heaped beside it, both inside the cut.
    let wall = Material::wall(seat(1), false).unwrap();
    for z in footing + 1..footing + 4 {
        world
            .request_voxel_edit(VoxelEdit::Set {
                at: [x, y, z],
                material: wall,
                by: None,
            })
            .unwrap();
        world
            .request_voxel_edit(VoxelEdit::Set {
                at: [x + 2, y, z],
                material: Material::STONE,
                by: None,
            })
            .unwrap();
    }
    world.step(&mut Enc::default()).unwrap();
    let radius = rules().attack().charge_radius;
    world
        .request_voxel_edit(VoxelEdit::Cut {
            centre: [x, y, footing],
            radius,
            by: Some(seat(0)),
        })
        .unwrap();
    world.step(&mut Enc::default()).unwrap();
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy > radius * radius {
                continue;
            }
            for z in footing + 1..footing + 8 {
                let material = world.voxels().get([x + dx, y + dy, z]).unwrap();
                assert!(material.is_air(), "({dx}, {dy}, {z}) is cut: {material:?}");
            }
        }
    }
    assert!(
        world.voxels().get([x, y, footing]).unwrap().is_solid(),
        "the footing stays"
    );
    // Flat: every step across the gap is a step a walker takes.
    let surface = world.surface();
    for dx in -radius..radius {
        let from = surface.node_of(x + dx, y).unwrap();
        let to = surface.node_of(x + dx + 1, y).unwrap();
        assert!(
            surface.step_cost_between(from, to).is_some(),
            "{dx}: the gap is walkable"
        );
    }
}

#[test]
fn a_charge_cut_spares_a_footing() {
    let mut world = world();
    let here = field(&world);
    let post = beacon(&mut world, seat(1), here, MandateKind::Build, 0);
    let [x, y] = column(beacon_at(&world, post));
    let footing = world.standing_point([x, y, 0])[2].floor_voxels() - 1;
    let before = world.voxels().get([x, y, footing]).unwrap();
    world
        .request_voxel_edit(VoxelEdit::Cut {
            centre: [x, y, footing - 3],
            radius: 3,
            by: Some(seat(0)),
        })
        .unwrap();
    world.step(&mut Enc::default()).unwrap();
    assert_eq!(
        world.voxels().get([x, y, footing]).unwrap(),
        before,
        "a live beacon's footing ignores the cut"
    );
    assert!(
        world.voxels().get([x + 3, y, footing]).unwrap().is_air(),
        "beside the footing the cut cuts"
    );
}

#[test]
fn only_a_breach_shells_an_enemy_wall() {
    // A FREE_FIRE mortar of seat 0 beside an enemy wall it can see, and no
    // enemy asset: it fires at nothing.
    let mut fixture = world();
    let here = field(&fixture);
    let home = beacon(&mut fixture, seat(0), here, MandateKind::Build, FREE_FIRE);
    fixture
        .raise_structure(
            seat(0),
            home,
            pharmakos_sim::tables::StructureKind::Mortar,
            here,
        )
        .expect("a mortar");
    let wall_at = ground(&fixture, here, 16, 0);
    wall_ring(&mut fixture, wall_at, 3, 2, seat(1));
    let mut runner = Runner::new(fixture).unwrap_or_else(|error| panic!("{error}"));
    runner.world_mut().step(&mut Enc::default()).unwrap();
    assert_eq!(runner.begin_push(), Ok(true));
    let mut log = Vec::new();
    play(&mut runner, 200, &mut log);
    assert_eq!(
        runner.world().combat_counts().shells_fired(),
        0,
        "a wall is no target"
    );

    // The same mortar, asked by a breach: it shells the wall.
    let (mut runner, post, _, _) = walled(3);
    let spot = ground(runner.world(), field(runner.world()), 10, -10);
    let home = beacon(
        runner.world_mut(),
        seat(0),
        spot,
        MandateKind::Build,
        FREE_FIRE,
    );
    let at = beacon_at(runner.world(), home);
    runner
        .world_mut()
        .raise_structure(
            seat(0),
            home,
            pharmakos_sim::tables::StructureKind::Mortar,
            at,
        )
        .expect("a mortar");
    let mut log = Vec::new();
    let shelled = play_until(&mut runner, 1_500, &mut log, |_, log| {
        fired(log, EventKind::ShellLanded) > 0
    });
    assert!(shelled, "the breach asks the mortar to shell its wall");
    let _ = post;
}

#[test]
fn a_carrier_killed_before_it_plants_fires_nothing() {
    let (mut runner, post, raider, _) = walled(3);
    let mut log = Vec::new();
    let loaded = play_until(&mut runner, 400, &mut log, |world, _| {
        world
            .attack()
            .row_of(post)
            .and_then(|row| row.breach)
            .is_some_and(|breach| breach.charge == ChargeState::Loaded)
    });
    assert!(loaded, "a charge is bought and loaded");
    hit(
        runner.world_mut(),
        DamageTarget::Unit(raider),
        1_000_000,
        seat(1),
    );
    let fuse = rules().attack().charge_fuse.to_ticks_ceil();
    play(&mut runner, fuse * 3, &mut log);
    assert_eq!(
        fired(&log, EventKind::ChargeLost),
        1,
        "the charge is lost with its carrier"
    );
    assert_eq!(fired(&log, EventKind::ChargePlanted), 0);
    assert_eq!(fired(&log, EventKind::ChargeDetonated), 0, "it never fires");
    assert!(runner.world().attack().charges().is_empty());
}

/// Decisions-log item 151 (2) (c) (3), from S3's `lic`: the charge's blast
/// takes `combat.charge_damage_hp` from every unit, structure and beacon in
/// its ball, any seat's, as it cuts the ground; and nothing outside it.
#[test]
fn a_charge_blast_damages_what_stands_in_its_ball() {
    let (mut runner, _, _, _) = walled(3);
    let mut log = Vec::new();
    let planted = play_until(&mut runner, 4_000, &mut log, |_, log| {
        fired(log, EventKind::ChargePlanted) > 0
    });
    assert!(planted, "a charge is planted at the breach");
    let [x, y, z] = runner.world().attack().charges()[0].centre;
    // A column of open ground beside the breach, not a wall's, so nothing
    // falls under what stands on it when the cut takes the wall.
    let open = |world: &World, dx: i32, dy: i32| {
        let at = world.standing_point([x + dx, y + dy, z]);
        let top = [x + dx, y + dy, at[2].floor_voxels() - 1];
        let wall = world
            .voxels()
            .get(top)
            .is_some_and(|material| material.wall_of().is_some());
        (!wall && at[2].floor_voxels() <= z + 1).then_some(at)
    };
    let inside = [
        (2, 0),
        (-2, 0),
        (0, 2),
        (0, -2),
        (2, 2),
        (-2, -2),
        (2, -2),
        (-2, 2),
    ]
    .into_iter()
    .find_map(|(dx, dy)| open(runner.world(), dx, dy))
    .expect("open ground beside the breach");
    let outside = runner.world().standing_point([x + 16, y + 16, z]);
    let structure = |world: &mut World, at| {
        let home = beacon(world, seat(1), at, MandateKind::Build, 0);
        world
            .raise_structure(
                seat(1),
                home,
                pharmakos_sim::tables::StructureKind::Generator,
                at,
            )
            .expect("room for a structure")
    };
    let near = structure(runner.world_mut(), inside);
    let far = structure(runner.world_mut(), outside);
    let hp = |world: &World, id: pharmakos_sim::tables::StructureId| {
        let row = world.structures().row_of(id).expect("standing").index();
        world.structures().hit_points()[row].raw()
    };
    let (near_before, far_before) = (hp(runner.world(), near), hp(runner.world(), far));
    let detonated = play_until(&mut runner, 400, &mut log, |_, log| {
        fired(log, EventKind::ChargeDetonated) > 0
    });
    assert!(detonated);
    let damage = rules().attack().charge_damage.raw();
    assert_eq!(
        near_before - hp(runner.world(), near),
        damage,
        "the blast takes its damage from a structure in the ball, on the tick it fires"
    );
    assert_eq!(
        hp(runner.world(), far),
        far_before,
        "and nothing outside it"
    );
}

#[test]
fn a_charge_crater_carries_its_seat() {
    let edit = VoxelEdit::Cut {
        centre: [1, 1, 1],
        radius: 2,
        by: Some(seat(1)),
    };
    assert_eq!(edit.by(), Some(seat(1)));
    let (mut runner, _, _, _) = walled(3);
    let mut log = Vec::new();
    let detonated = play_until(&mut runner, 4_000, &mut log, |_, log| {
        fired(log, EventKind::ChargeDetonated) > 0
    });
    assert!(detonated);
    let blast = log
        .iter()
        .find(|event| event.kind == EventKind::ChargeDetonated)
        .unwrap();
    assert_eq!(blast.seat, Some(seat(0)), "the cut is the carrier's seat's");
}

#[test]
fn the_last_beacon_lost_ends_the_match_that_tick() {
    let mut fixture = world();
    let target = core(&fixture, seat(1));
    let at = beacon_at(&fixture, target);
    let here = ground(&fixture, at, 3, 0);
    let post = attack(
        &mut fixture,
        seat(0),
        here,
        FREE_FIRE,
        raid(Aim::Beacon(target), 1),
    );
    let raider = unit(&mut fixture, seat(0), post, UnitKind::Raider, here);
    // The core is at its last hit points, so one raider's shot ends it.
    let hp = fixture.beacons().hit_points()[fixture.beacons().row_of(target).unwrap().index()];
    hit(
        &mut fixture,
        DamageTarget::Beacon(target),
        i64::from(hp.raw()) - 1,
        seat(0),
    );
    let mut runner = pushing(fixture);
    let mut log = Vec::new();
    let over = play_until(&mut runner, 200, &mut log, |_, log| {
        fired(log, EventKind::MatchEnded) > 0
    });
    assert!(over, "the raid takes the last beacon");
    let lost = log
        .iter()
        .find(|event| event.kind == EventKind::BeaconDestroyed)
        .expect("the core is destroyed");
    let ended = log
        .iter()
        .find(|event| event.kind == EventKind::MatchEnded)
        .unwrap();
    assert_eq!(lost.tick, ended.tick, "the match ends on that tick");
    assert_eq!(ended.seat, Some(seat(0)), "the last seat standing wins");
    let _ = raider;
}

#[test]
fn an_attack_in_a_breach_survives_save_and_restore() {
    let (mut runner, post, _, _) = walled(3);
    let mut log = Vec::new();
    let planted = play_until(&mut runner, 4_000, &mut log, |world, _| {
        !world.attack().charges().is_empty()
    });
    assert!(planted, "state worth saving: a planted charge");
    assert!(row(runner.world(), post).breach.is_some());
    let bytes = Snapshot::capture(runner.world())
        .expect("a world between ticks")
        .to_bytes()
        .unwrap();
    let (mut copy, _, _, _) = walled(3);
    Snapshot::from_bytes(&bytes)
        .unwrap()
        .restore_into(copy.world_mut())
        .unwrap();
    assert_eq!(copy.world().state_hash(), runner.world().state_hash());
    assert_eq!(copy.world().attack(), runner.world().attack());
    for _ in 0..200 {
        let a = runner.step().unwrap().expect("in the Push");
        let b = copy.step().unwrap().expect("in the Push");
        assert_eq!(a, b, "the restored run's chain is the unbroken one's");
        runner.clear_events();
        copy.clear_events();
    }
    assert_eq!(
        (
            runner.world().attack_counts().breach_no_way,
            copy.world().attack_counts().breach_no_way
        ),
        (0, 0),
        "no breach search is refused, before the save or after it"
    );
}

#[test]
fn an_empty_attack_table_hashes_nothing() {
    let mut world = world();
    let here = field(&world);
    let before = world.state_hash();
    let id = world
        .place_beacon_directly(seat(0), here, MandateKind::Build, PRIORITY_NORMAL)
        .unwrap();
    let build = world.state_hash();
    assert_ne!(before, build);
    let row = world.beacons().row_of(id).unwrap().index();
    world.switch_mandate_directly(row, MandateKind::Attack);
    assert_ne!(world.state_hash(), build, "an Attack row is hashed");
    world.switch_mandate_directly(row, MandateKind::Build);
    assert_eq!(
        world.state_hash(),
        build,
        "and the empty table hashes nothing"
    );
    let node = world
        .surface()
        .node_of(column(here)[0], column(here)[1])
        .unwrap();
    assert_eq!(
        SealedWay::of(&world, node, node),
        None,
        "a way to where it starts is open: no breach is asked of it"
    );
}

/// A playbook whose one step deploys an Attack beacon whose target is the
/// beacon reference `reference`, spelt as canonical JSON.
fn deploy_against_ref(reference: &str) -> gp::v1::Playbook {
    let text = format!(
        r#"{{"schema_version":{{"major":1}},"meta":{{"title":"t","author_kind":"HUMAN"}},
        "kind":"PLAYBOOK","declarative":{{"route":[{{"label":"s","place_beacon":{{
        "at":{{"voxel":{{"x":96,"y":11,"z":62}}}},"initial":{{"mandate":{{"attack":{{
        "target":{{"beacon":{reference}}},"retarget":"NONE",
        "staging":{{"x":96,"y":11,"z":62}},"launch":{{"segment_ms":0}},
        "after_action":"RETURN_HOME","waves":1}}}}}}}}}}]}},
        "on_death":{{"on_respawn":"CONTINUE","max_deaths_before_fallback":1}},
        "fallback":{{"hold":{{"at":{{"beacon_anchor":{{"safest":{{}}}}}}}}}}}}"#
    );
    pharmakos_proto::json::decode(&text).expect("a playbook")
}

#[test]
fn only_the_nearest_known_enemy_beacon_compiles_as_a_selector_target() {
    let rules = rules();
    let plan = Plan::compile(
        &deploy_against_ref(r#"{"nearest":{"filter":{"side":"ENEMY_KNOWN"}}}"#),
        &rules,
    )
    .expect("the one lifted selector compiles");
    assert_eq!(deployed_target(&plan), Some(TargetSpec::NearestKnownBeacon));
    // Every other selector stays refused: one ranking the seat's own
    // beacons is no enemy, and every other enemy selector, or this one with
    // a further filter field, is the filter's refusal until S3.
    assert_eq!(
        Plan::compile(&deploy_against_ref(r#"{"nearest":{}}"#), &rules).err(),
        Some(pharmakos_sim::interpreter::PlanError::AttackTargetNotEnemy)
    );
    let enemy_known = pharmakos_sim::interpreter::REFUSE_ENEMY_KNOWN.error();
    for refused in [
        r#"{"weakest":{"filter":{"side":"ENEMY_KNOWN"}}}"#,
        r#"{"most_threatened":{"filter":{"side":"ENEMY_KNOWN"}}}"#,
        r#"{"nearest":{"filter":{"side":"ENEMY_KNOWN","mandate":"DEFEND"}}}"#,
        r#"{"nearest":{"filter":{"side":"ENEMY_KNOWN","tags":["x"]}}}"#,
    ] {
        assert_eq!(
            Plan::compile(&deploy_against_ref(refused), &rules).err(),
            Some(enemy_known.clone()),
            "{refused}"
        );
    }
}

/// A raid on the nearest known enemy beacon from a beacon in the field, its
/// force of one standing at it.
fn nearest_known_raid(fixture: &mut World) -> BeaconId {
    let here = field(fixture);
    let post = attack(
        fixture,
        seat(0),
        here,
        FREE_FIRE,
        raid(Aim::NearestKnownBeacon, 1),
    );
    unit(fixture, seat(0), post, UnitKind::Raider, here);
    post
}

#[test]
fn a_raid_on_the_nearest_known_beacon_launches_at_the_nearest_sighting() {
    let mut fixture = world();
    let post = nearest_known_raid(&mut fixture);
    let here = field(&fixture);
    // Two of seat 1's beacons inside the Attack beacon's sphere, so the seat
    // sights both at its first decision tick: the nearer is the target.
    let (near_at, far_at) = (
        ground(&fixture, here, 10, 0),
        ground(&fixture, here, -16, 0),
    );
    // The farther is placed first, so it holds the lower id: distance, not
    // the tie rule, chooses.
    let far = beacon(&mut fixture, seat(1), far_at, MandateKind::Build, 0);
    let near = beacon(&mut fixture, seat(1), near_at, MandateKind::Build, 0);
    assert!(far < near);
    let mut runner = pushing(fixture);
    let mut log = Vec::new();
    let launched = play_until(&mut runner, 100, &mut log, |world, _| {
        world
            .attack()
            .row_of(post)
            .is_some_and(|row| row.phase == Phase::Launched)
    });
    assert!(launched, "a fresh sighting launches the raid");
    let row = row(runner.world(), post);
    assert_eq!(row.target, AssetId::of_beacon(near).unwrap().raw());
    assert_ne!(row.target, AssetId::of_beacon(far).unwrap().raw());
    assert_eq!(fired(&log, EventKind::AttackLaunched), 1);
    // A decision before the first sighting may have held, under its own
    // reason; the commander hunt's never moves.
    let counts = runner.world().attack_counts();
    assert!(counts.launch_without_known_beacon <= 1);
    assert_eq!(counts.launch_without_target, 0);
}

#[test]
fn a_raid_on_the_nearest_known_beacon_retargets_when_its_choice_falls() {
    let mut fixture = world();
    let here = field(&fixture);
    let post = attack(
        &mut fixture,
        seat(0),
        here,
        FREE_FIRE,
        AttackSettings {
            retarget: pharmakos_sim::attack::Retarget::NearestKnown,
            ..raid(Aim::NearestKnownBeacon, 1)
        },
    );
    unit(&mut fixture, seat(0), post, UnitKind::Raider, here);
    let (near_at, far_at) = (
        ground(&fixture, here, 10, 0),
        ground(&fixture, here, -16, 0),
    );
    let far = beacon(&mut fixture, seat(1), far_at, MandateKind::Build, 0);
    let near = beacon(&mut fixture, seat(1), near_at, MandateKind::Build, 0);
    let mut runner = pushing(fixture);
    let mut log = Vec::new();
    let launched = play_until(&mut runner, 100, &mut log, |world, _| {
        world
            .attack()
            .row_of(post)
            .is_some_and(|row| row.phase == Phase::Launched)
    });
    assert!(launched, "launched at the nearer");
    assert_eq!(
        row(runner.world(), post).target,
        AssetId::of_beacon(near).unwrap().raw()
    );
    // The chosen beacon falls where the seat sees it: `NEAREST_KNOWN` takes
    // the next one the seat knows of, never the fallen one again.
    hit(
        runner.world_mut(),
        DamageTarget::Beacon(near),
        1_000_000,
        seat(1),
    );
    let far_raw = AssetId::of_beacon(far).unwrap().raw();
    let moved_on = play_until(&mut runner, 200, &mut log, |world, _| {
        world
            .attack()
            .row_of(post)
            .is_some_and(|row| row.target == far_raw)
    });
    assert!(moved_on, "retargeted to the other known beacon");
}

#[test]
fn a_settings_row_compiles_the_nearest_known_enemy_beacon_too() {
    let text = r#"{"schema_version":{"major":1},"meta":{"title":"t","author_kind":"HUMAN"},
        "kind":"PLAYBOOK","declarative":{"route":[{"label":"s","interface":{
        "beacon":{"beacon_id":"b_01"},"rows":[{"set_mandate_settings":{"attack":{
        "target":{"beacon":{"nearest":{"filter":{"side":"ENEMY_KNOWN"}}}}}}}]}}]},
        "on_death":{"on_respawn":"CONTINUE","max_deaths_before_fallback":1},
        "fallback":{"hold":{"at":{"beacon_anchor":{"safest":{}}}}}}"#;
    let playbook: gp::v1::Playbook = pharmakos_proto::json::decode(text).unwrap();
    let plan = Plan::compile(&playbook, &rules()).expect("it compiles");
    let Action::Interface { rows, .. } = &plan.route()[0].action else {
        panic!("an interface step");
    };
    assert!(rows.iter().any(|row| matches!(
        row,
        Row::Settings {
            attack: Some(edit),
            ..
        } if edit.target == Some(TargetSpec::NearestKnownBeacon)
    )));
}

#[test]
fn a_restored_nearest_known_aim_carries_no_value() {
    let mut world = world();
    let here = field(&world);
    let post = attack(
        &mut world,
        seat(0),
        here,
        FREE_FIRE,
        raid(Aim::NearestKnownBeacon, 1),
    );
    let columns = pharmakos_sim::attack::AttackColumns::of(world.attack());
    let beacons = [post.raw()];
    assert!(columns.restore(|_| true, &beacons, 2).is_ok());
    let mut valued = columns.clone();
    valued.aim_value[0] = 1;
    assert_eq!(
        valued.restore(|_| true, &beacons, 2).err(),
        Some("attack_aim")
    );
    let mut placed = columns;
    placed.aim_voxel[0] = 1;
    assert_eq!(
        placed.restore(|_| true, &beacons, 2).err(),
        Some("attack_aim")
    );
}

#[test]
fn a_raid_on_the_nearest_known_beacon_holds_and_counts_with_none_known() {
    let mut fixture = world();
    let post = nearest_known_raid(&mut fixture);
    let mut runner = pushing(fixture);
    let mut log = Vec::new();
    play(&mut runner, 40, &mut log);
    let row = row(runner.world(), post);
    assert_eq!(row.phase, Phase::Gathering, "held, never sent to a default");
    assert_eq!(row.target, pharmakos_sim::attack::NO_TARGET);
    assert_eq!(fired(&log, EventKind::AttackLaunched), 0);
    let counts = runner.world().attack_counts();
    assert!(
        counts.launch_without_known_beacon > 0,
        "each held decision is counted"
    );
    assert_eq!(
        counts.launch_without_target, 0,
        "under its own reason, not the commander hunt's"
    );
}

#[test]
fn the_nearest_known_beacon_aim_is_hashed_and_survives_save_and_restore() {
    let settle = |aim: Aim| {
        let mut world = world();
        let here = field(&world);
        attack(&mut world, seat(0), here, FREE_FIRE, raid(aim, 1));
        world
    };
    let beacon_raid = settle(Aim::NearestKnownBeacon);
    let hunt = settle(Aim::Commander(None));
    assert_ne!(
        beacon_raid.state_hash(),
        hunt.state_hash(),
        "the aim's tag is hashed apart from the seatless hunt's"
    );
    let bytes = Snapshot::capture(&beacon_raid)
        .expect("a world between ticks")
        .to_bytes()
        .unwrap();
    let mut copy = settle(Aim::Commander(None));
    Snapshot::from_bytes(&bytes)
        .unwrap()
        .restore_into(&mut copy)
        .unwrap();
    assert_eq!(copy.attack(), beacon_raid.attack());
    assert_eq!(copy.state_hash(), beacon_raid.state_hash());
}

// The nearest known enemy beacon's edge cases (S2's `rush`, the gaps `atk2`'s
// review listed): a tie, a stale sighting and a second wave.

/// Two ground points `min` to `max` columns out from `centre`, at least
/// `apart` columns from each other, whose squared distances from `centre`
/// are equal: heights differ, so the pair is searched for rather than taken
/// from a mirror.
fn tied_pair(world: &World, centre: [Fx; 3], min: i32, max: i32, apart: i32) -> ([Fx; 3], [Fx; 3]) {
    let mut seen: Vec<(Sq, [Fx; 3])> = Vec::new();
    for dy in -max..=max {
        for dx in -max..=max {
            if !(min * min..=max * max).contains(&(dx * dx + dy * dy)) {
                continue;
            }
            let here = ground(world, centre, dx, dy);
            let d2 = Sq::between(here, centre);
            let [x, y] = column(here);
            if let Some((_, there)) = seen.iter().find(|(other, there)| {
                let [ox, oy] = column(*there);
                *other == d2 && (x - ox).abs().max((y - oy).abs()) >= apart
            }) {
                return (*there, here);
            }
            seen.push((d2, here));
        }
    }
    panic!("no two ground points tie round the field on this seed");
}

/// Whether seat 0 holds a sighting of `beacon`.
fn sighted_by_seat_0(world: &World, beacon: BeaconId) -> bool {
    let raw = AssetId::of_beacon(beacon).unwrap().raw();
    let table = world.sightings();
    table
        .seats()
        .iter()
        .zip(table.assets())
        .any(|(by, of)| *by == seat(0).raw() && *of == raw)
}

#[test]
fn a_tie_between_two_equally_near_known_beacons_goes_to_the_lowest_id() {
    let mut fixture = world();
    let here = field(&fixture);
    // Held under `RETURN_FIRE` while the two sightings are taken, so the
    // choice is made only once both are known.
    let post = attack(
        &mut fixture,
        seat(0),
        here,
        RETURN_FIRE,
        raid(Aim::NearestKnownBeacon, 1),
    );
    unit(&mut fixture, seat(0), post, UnitKind::Raider, here);
    // Staging is the beacon itself, so "nearest" is measured from there. The
    // two stand beyond the post's sphere (24) and the raider's sight (12),
    // and far enough apart that a scout beside one does not see the other.
    let from = beacon_at(&fixture, post);
    let (low_at, high_at) = tied_pair(&fixture, from, 36, 44, 60);
    let low = beacon(&mut fixture, seat(1), low_at, MandateKind::Build, 0);
    let high = beacon(&mut fixture, seat(1), high_at, MandateKind::Build, 0);
    assert!(low < high);
    assert_eq!(
        Sq::between(beacon_at(&fixture, low), from),
        Sq::between(beacon_at(&fixture, high), from),
        "the two stand equally near the staging point"
    );
    let home = core(&fixture, seat(0));
    unit(&mut fixture, seat(0), home, UnitKind::Scout, high_at);
    let mut runner = pushing(fixture);
    let mut log = Vec::new();
    // The higher id is seen first, so the sighting table holds it first and
    // an order-only choice would take it: only the id breaks the tie.
    assert!(play_until(&mut runner, 100, &mut log, |world, _| {
        sighted_by_seat_0(world, high)
    }));
    assert!(!sighted_by_seat_0(runner.world(), low), "not yet seen");
    unit(runner.world_mut(), seat(0), home, UnitKind::Scout, low_at);
    assert!(play_until(&mut runner, 100, &mut log, |world, _| {
        sighted_by_seat_0(world, low)
    }));
    runner.world_mut().set_common(
        post,
        Common {
            roe: FREE_FIRE,
            retreat_hp_pct: 0,
        },
    );
    let launched = play_until(&mut runner, 100, &mut log, |world, _| {
        world
            .attack()
            .row_of(post)
            .is_some_and(|row| row.phase == Phase::Launched)
    });
    assert!(launched, "both are known, so the raid launches");
    assert_eq!(
        row(runner.world(), post).target,
        AssetId::of_beacon(low).unwrap().raw(),
        "ties to the lowest id"
    );
    assert_eq!(fired(&log, EventKind::AttackLaunched), 1);
}

#[test]
fn a_stale_sighting_of_an_enemy_beacon_holds_the_force() {
    let mut fixture = world();
    let here = field(&fixture);
    // `RETURN_FIRE` gathers and never launches, so the force stands ready
    // while the sighting ages; `FREE_FIRE` is set once it is stale.
    let post = attack(
        &mut fixture,
        seat(0),
        here,
        RETURN_FIRE,
        raid(Aim::NearestKnownBeacon, 1),
    );
    unit(&mut fixture, seat(0), post, UnitKind::Raider, here);
    // A scout of seat 0's beside seat 1's core, on the field's side of it:
    // the only sensor of seat 0's that ever sees an enemy beacon.
    let enemy = core(&fixture, seat(1));
    let enemy_at = beacon_at(&fixture, enemy);
    let [ex, ey] = column(enemy_at);
    let [hx, hy] = column(here);
    let scout_at = ground(
        &fixture,
        enemy_at,
        (hx - ex).signum() * 10,
        (hy - ey).signum() * 10,
    );
    let home = core(&fixture, seat(0));
    let scout = unit(&mut fixture, seat(0), home, UnitKind::Scout, scout_at);
    let sighted = |world: &World| sighted_by_seat_0(world, enemy);
    let mut runner = pushing(fixture);
    let mut log = Vec::new();
    assert!(
        play_until(&mut runner, 100, &mut log, |world, _| sighted(world)),
        "the scout sights the enemy core"
    );
    hit(
        runner.world_mut(),
        DamageTarget::Unit(scout),
        1_000_000,
        seat(1),
    );
    // Past the reach window, with nothing of seat 0's in sight of the core.
    let window = runner.world().rules().attack().reach_memory.to_ticks_ceil();
    play(&mut runner, window + 40, &mut log);
    assert!(
        runner.world().units().row_of(scout).is_err(),
        "the scout is dead"
    );
    assert!(
        sighted(runner.world()),
        "the sighting is still held, only stale"
    );
    assert_eq!(row(runner.world(), post).phase, Phase::Gathering);
    assert_eq!(runner.phase(), MatchPhase::Push, "still in the Push");
    let held_before = runner.world().attack_counts().launch_without_known_beacon;
    runner.world_mut().set_common(
        post,
        Common {
            roe: FREE_FIRE,
            retreat_hp_pct: 0,
        },
    );
    play(&mut runner, 100, &mut log);
    let row = row(runner.world(), post);
    assert_eq!(
        row.phase,
        Phase::Gathering,
        "held, never sent to a stale sighting"
    );
    assert_eq!(row.target, pharmakos_sim::attack::NO_TARGET);
    assert_eq!(fired(&log, EventKind::AttackLaunched), 0);
    assert!(
        runner.world().attack_counts().launch_without_known_beacon > held_before,
        "each held decision is counted"
    );
}

#[test]
fn a_second_wave_launches_at_the_nearest_known_beacon_left() {
    let mut fixture = world();
    let here = field(&fixture);
    let post = attack(
        &mut fixture,
        seat(0),
        here,
        FREE_FIRE,
        AttackSettings {
            waves: 2,
            ..raid(Aim::NearestKnownBeacon, 1)
        },
    );
    unit(&mut fixture, seat(0), post, UnitKind::Raider, here);
    let (near_at, far_at) = (
        ground(&fixture, here, 10, 0),
        ground(&fixture, here, -16, 0),
    );
    let far = beacon(&mut fixture, seat(1), far_at, MandateKind::Build, 0);
    let near = beacon(&mut fixture, seat(1), near_at, MandateKind::Build, 0);
    // A third known beacon, the farthest, alive to the end: only `waves`
    // can stop a third launch at it.
    let spare_at = ground(&fixture, here, 0, 21);
    let spare = beacon(&mut fixture, seat(1), spare_at, MandateKind::Build, 0);
    assert!(Sq::between(spare_at, here) > Sq::between(far_at, here));
    let mut runner = pushing(fixture);
    let mut log = Vec::new();
    let first = play_until(&mut runner, 100, &mut log, |world, _| {
        world
            .attack()
            .row_of(post)
            .is_some_and(|row| row.phase == Phase::Launched)
    });
    assert!(first, "the first wave launches");
    assert_eq!(
        row(runner.world(), post).target,
        AssetId::of_beacon(near).unwrap().raw()
    );
    // The first wave's target falls where the seat sees it: with no
    // retarget written the attack ends, the force regroups at staging, and
    // the second wave chooses again at its own launch, never the fallen one.
    hit(
        runner.world_mut(),
        DamageTarget::Beacon(near),
        1_000_000,
        seat(1),
    );
    let second = play_until(&mut runner, 1_000, &mut log, |world, _| {
        world
            .attack()
            .row_of(post)
            .is_some_and(|row| row.waves_used == 2 && row.phase == Phase::Launched)
    });
    assert!(second, "the second wave launches");
    assert_eq!(
        row(runner.world(), post).target,
        AssetId::of_beacon(far).unwrap().raw()
    );
    assert_eq!(fired(&log, EventKind::AttackLaunched), 2);
    assert!(
        fired(&log, EventKind::AttackEnded) >= 1,
        "the first wave ended before the second"
    );
    // No third, though the spare is known and fresh: `waves` is read, and it
    // stops the force before the target is looked for, so nothing is held
    // and counted either.
    hit(
        runner.world_mut(),
        DamageTarget::Beacon(far),
        1_000_000,
        seat(1),
    );
    let held_before = runner.world().attack_counts().launch_without_known_beacon;
    play(&mut runner, 600, &mut log);
    assert_eq!(runner.phase(), MatchPhase::Push, "still in the Push");
    assert!(
        sighted_by_seat_0(runner.world(), spare),
        "the spare is known"
    );
    assert!(runner.world().beacons().row_of(spare).is_ok(), "and alive");
    assert_eq!(
        runner.world().attack_counts().launch_without_known_beacon,
        held_before
    );
    assert_eq!(
        fired(&log, EventKind::AttackLaunched),
        2,
        "at most two waves"
    );
}

// ---------------------------------------------------------------------------
// The breach search reads the terrain by the surface's own step rule (lane
// `settle`, fix pass 2): a wall joined only corner to corner seals the way for
// walkers, so it seals it for the breach search's wall-free moves too, and a
// sealed way always crosses a wall.
// ---------------------------------------------------------------------------

/// `seat`'s wall voxels, `height` high from the ground, on each column of
/// `columns`, given as offsets from `centre`.
fn wall_columns(
    world: &mut World,
    centre: [Fx; 3],
    columns: &[(i32, i32)],
    height: i32,
    of: SeatId,
) {
    let [cx, cy] = column(centre);
    let wall = Material::wall(of, false).expect("a wall id");
    for &(dx, dy) in columns {
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

/// A diamond, `|dx| + |dy| == half`: one voxel thick, every column joined to
/// the next only corner to corner, as any wall drawn along a diagonal is.
fn diamond(half: i32) -> Vec<(i32, i32)> {
    let mut columns = Vec::new();
    for dy in -half..=half {
        for dx in -half..=half {
            if dx.abs() + dy.abs() == half {
                columns.push((dx, dy));
            }
        }
    }
    columns
}

/// Four straight wall boxes round a square, their ends meeting only corner to
/// corner: the square's four corner columns are left open.
fn corner_to_corner(half: i32) -> Vec<(i32, i32)> {
    let mut columns = Vec::new();
    for along in (1 - half)..half {
        columns.extend([(along, half), (along, -half), (half, along), (-half, along)]);
    }
    columns
}

/// The square ring of [`wall_ring`], as columns.
fn ring(half: i32) -> Vec<(i32, i32)> {
    let mut columns = Vec::new();
    for dy in -half..=half {
        for dx in -half..=half {
            if dx.abs() == half || dy.abs() == half {
                columns.push((dx, dy));
            }
        }
    }
    columns
}

/// A world with seat 1's wall on `columns` round a point 24 columns east of
/// the field, settled: the field, and that point.
fn enclosed(columns: &[(i32, i32)], height: i32) -> (World, [Fx; 3], [Fx; 3]) {
    let mut fixture = world();
    let here = field(&fixture);
    let inside = ground(&fixture, here, 24, 0);
    wall_columns(&mut fixture, inside, columns, height, seat(1));
    fixture.step(&mut Enc::default()).expect("the walls settle");
    (fixture, here, inside)
}

/// The breach search from `from` to `to` for seat 0, after the oracle has
/// called the way sealed.
fn sealed_breach(
    world: &World,
    from: [Fx; 3],
    to: [Fx; 3],
) -> Result<pharmakos_sim::attack::Breach, pharmakos_sim::attack::BreachRefused> {
    let surface = world.surface();
    let node = |at: [Fx; 3]| {
        let [x, y] = column(at);
        surface.node_of(x, y).expect("a column")
    };
    let (start, goal) = (node(from), node(to));
    assert!(
        !world.clusters().connected(surface, start, goal),
        "the oracle calls the way sealed"
    );
    let mut scratch = BreachScratch::new(surface.node_count());
    breach_search(
        world,
        &mut scratch,
        seat(0),
        SealedWay::of(world, start, goal).expect("a sealed way"),
    )
}

/// Whether the breach's wall is one of `columns` round `inside`.
fn on_the_wall(breach: [i32; 3], inside: [Fx; 3], columns: &[(i32, i32)]) -> bool {
    let [cx, cy] = column(inside);
    columns.contains(&(breach[0] - cx, breach[1] - cy))
}

/// A raid with `breach` allowed from the field on the point inside `columns`:
/// the raider plants a charge, it fires, and the raider gets in; no breach
/// search is refused.
fn raid_gets_through(columns: &[(i32, i32)], height: i32, crater: Option<(i32, i32)>) {
    let mut fixture = world();
    let here = field(&fixture);
    let inside = ground(&fixture, here, 24, 0);
    wall_columns(&mut fixture, inside, columns, height, seat(1));
    if let Some((dx, dy)) = crater {
        let [cx, cy] = column(inside);
        let z = fixture.standing_point([cx + dx, cy + dy, 0])[2].floor_voxels();
        fixture.step(&mut Enc::default()).expect("the walls settle");
        fixture
            .request_voxel_edit(VoxelEdit::Crater {
                centre: [cx + dx, cy + dy, z + 2],
                radius: 1,
                by: None,
            })
            .expect("the crater is filed");
    }
    let post = attack(
        &mut fixture,
        seat(0),
        here,
        FREE_FIRE,
        AttackSettings {
            breach: true,
            ..raid(Aim::Voxel(voxel(inside)), 1)
        },
    );
    let raider = unit(&mut fixture, seat(0), post, UnitKind::Raider, here);
    let mut runner = Runner::new(fixture).expect("a world from `World::new`");
    runner
        .world_mut()
        .step(&mut Enc::default())
        .expect("the terrain settles");
    assert_eq!(runner.begin_push(), Ok(true));
    let mut log = Vec::new();
    let through = play_until(&mut runner, 6_000, &mut log, |world, _| {
        within(unit_at(world, raider), inside, 2)
    });
    let counts = runner.world().attack_counts();
    assert_eq!(
        (counts.breach_exhausted, counts.breach_no_way),
        (0, 0),
        "no breach search is refused"
    );
    assert!(
        through,
        "the raider gets in (planted {}, detonated {}, ended {})",
        fired(&log, EventKind::ChargePlanted),
        fired(&log, EventKind::ChargeDetonated),
        fired(&log, EventKind::AttackEnded),
    );
}

#[test]
fn the_breach_search_breaches_a_diagonal_wall() {
    let columns = diamond(5);
    let (world, here, inside) = enclosed(&columns, 3);
    let breach = sealed_breach(&world, here, inside).expect("a breach of the diamond");
    assert!(
        on_the_wall(breach.wall, inside, &columns),
        "the breach {:?} is on the diamond",
        breach.wall
    );
}

#[test]
fn the_breach_search_breaches_boxes_joined_corner_to_corner() {
    let columns = corner_to_corner(5);
    let (world, here, inside) = enclosed(&columns, 3);
    let breach = sealed_breach(&world, here, inside).expect("a breach of the boxes");
    assert!(
        on_the_wall(breach.wall, inside, &columns),
        "the breach {:?} is on a box",
        breach.wall
    );
}

#[test]
fn a_raid_breaches_a_diagonal_wall_and_gets_in() {
    raid_gets_through(&diamond(5), 3, None);
}

#[test]
fn a_raid_breaches_boxes_joined_corner_to_corner_and_gets_in() {
    raid_gets_through(&corner_to_corner(5), 3, None);
}

#[test]
fn a_raid_breaches_a_cratered_ring_and_gets_in() {
    raid_gets_through(&ring(4), 3, Some((4, 4)));
}

// ---------------------------------------------------------------------------
// The breach search is the walkers' moves, then one crossing, then the
// breach's moves (lane `settle`, fix pass 3): a lead on an enemy wall moves
// as a walker does, the plant is a column a walker reaches, and a lead with no
// walker way off its perch has no breach.
// ---------------------------------------------------------------------------

/// The breach search from column `from` to column `to`, for seat 0, after the
/// oracle has called the way sealed.
fn search_between(
    world: &World,
    from: [i32; 2],
    to: [i32; 2],
) -> Result<pharmakos_sim::attack::Breach, pharmakos_sim::attack::BreachRefused> {
    let surface = world.surface();
    let start = surface.node_of(from[0], from[1]).expect("a column");
    let goal = surface.node_of(to[0], to[1]).expect("a column");
    assert!(
        !world.clusters().connected(surface, start, goal),
        "the oracle calls the way sealed"
    );
    let mut scratch = BreachScratch::new(surface.node_count());
    breach_search(
        world,
        &mut scratch,
        seat(0),
        SealedWay::of(world, start, goal).expect("a sealed way"),
    )
}

/// Whether the column is a wall of another seat or of nobody, read at its
/// top: one the breach search may demolish.
fn breachable(world: &World, x: i32, y: i32) -> bool {
    let surface = world.surface();
    let Some(node) = surface.node_of(x, y) else {
        return false;
    };
    if !surface.walkable(node) {
        return false;
    }
    let top = surface.top(node);
    world
        .voxels()
        .get([x, y, top])
        .and_then(Material::wall_of)
        .is_some()
        && world.wall_owner([x, y, top]) != Some(seat(0))
}

/// The walkers' component of `start`, by breadth-first search over
/// `Surface::neighbour`.
fn walker_reach(world: &World, start: u32) -> Vec<bool> {
    let surface = world.surface();
    let n = usize::try_from(surface.node_count()).unwrap();
    let mut seen = vec![false; n];
    let mut queue = std::collections::VecDeque::new();
    if surface.walkable(start) {
        seen[usize::try_from(start).unwrap()] = true;
        queue.push_back(start);
    }
    while let Some(node) = queue.pop_front() {
        for k in 0..8 {
            if let Some((next, _)) = surface.neighbour(node, k) {
                let at = usize::try_from(next).unwrap();
                if !seen[at] {
                    seen[at] = true;
                    queue.push_back(next);
                }
            }
        }
    }
    seen
}

/// A breach's plant and wall, held to what a breach is: the plant a column a
/// walker from `start` reaches, standing where a walker stands, no breachable
/// wall, and beside the wall, which is breachable.
fn assert_a_breach(world: &World, start: [i32; 2], breach: &pharmakos_sim::attack::Breach) {
    let surface = world.surface();
    let walker = walker_reach(world, surface.node_of(start[0], start[1]).unwrap());
    let [wx, wy, _] = breach.wall;
    let [px, py, pz] = breach.plant;
    let plant = surface.node_of(px, py).unwrap();
    assert!(
        breachable(world, wx, wy),
        "the wall {:?} is breachable",
        breach.wall
    );
    assert!(
        !breachable(world, px, py),
        "the plant {:?} is on a breachable wall",
        breach.plant
    );
    assert!(
        walker[usize::try_from(plant).unwrap()],
        "the plant {:?} is a column a walker from the start reaches",
        breach.plant
    );
    assert_eq!(
        surface.standing(plant),
        Some(pz),
        "the plant stands where a walker stands"
    );
    assert!(
        (wx - px).abs() <= 1 && (wy - py).abs() <= 1 && (wx, wy) != (px, py),
        "the plant {:?} is beside the wall {:?}",
        breach.plant,
        breach.wall
    );
}

/// A ring round a point 24 columns east of the field, with `perch`: seat 1's
/// wall columns, each `(offset, height)`, the last the lead's.
fn perched(perch: &[((i32, i32), i32)]) -> (World, [i32; 2], [Fx; 3]) {
    let mut fixture = world();
    let here = field(&fixture);
    let inside = ground(&fixture, here, 24, 0);
    wall_columns(&mut fixture, inside, &ring(4), 3, seat(1));
    for &(at, height) in perch {
        wall_columns(&mut fixture, inside, &[at], height, seat(1));
    }
    let [cx, cy] = column(inside);
    let ((px, py), _) = *perch.last().expect("a perch");
    (fixture, [cx + px, cy + py], inside)
}

/// A raid with `breach` allowed from the lead's perch on the point inside the
/// ring: the runner, the beacon, the raider.
fn raid_from_the_perch(
    mut fixture: World,
    perch: [i32; 2],
    inside: [Fx; 3],
) -> (Runner, BeaconId, UnitId) {
    fixture.step(&mut Enc::default()).expect("the walls settle");
    let at = fixture.standing_point([perch[0], perch[1], 0]);
    let post = attack(
        &mut fixture,
        seat(0),
        at,
        FREE_FIRE,
        AttackSettings {
            breach: true,
            ..raid(Aim::Voxel(voxel(inside)), 1)
        },
    );
    let raider = unit(&mut fixture, seat(0), post, UnitKind::Raider, at);
    (pushing(fixture), post, raider)
}

/// N5: a lead on a two-high enemy wall it climbed by a one-high stub (a
/// staircase a walker uses both ways) walks down and breaches the ring from
/// the ground.
#[test]
fn a_lead_on_a_staircase_wall_breaches_from_where_a_walker_reaches() {
    let (mut fixture, perch, inside) = perched(&[((-8, 0), 1), ((-7, 0), 2)]);
    fixture.step(&mut Enc::default()).expect("the walls settle");
    let breach = search_between(&fixture, perch, column(inside)).expect("a breach");
    assert_a_breach(&fixture, perch, &breach);
    assert!(
        on_the_wall(breach.wall, inside, &ring(4)),
        "the breach {:?} is on the ring",
        breach.wall
    );
    let (fixture, perch, inside) = perched(&[((-8, 0), 1), ((-7, 0), 2)]);
    let (mut runner, _, raider) = raid_from_the_perch(fixture, perch, inside);
    let mut log = Vec::new();
    let through = play_until(&mut runner, 6_000, &mut log, |world, _| {
        within(unit_at(world, raider), inside, 2)
    });
    assert!(
        through,
        "the raider gets in (planted {}, detonated {}, ended {})",
        fired(&log, EventKind::ChargePlanted),
        fired(&log, EventKind::ChargeDetonated),
        fired(&log, EventKind::AttackEnded),
    );
}

/// A lead on a pillar of enemy wall it cannot step off (two or more above the
/// ground on every side) has no breach: N3's stubs two and three high one
/// column off the ring, and a three-high pillar fourteen columns out.
#[test]
fn a_lead_on_a_pillar_it_cannot_step_off_has_no_breach() {
    for (offset, height) in [((-6, 0), 2), ((-6, 0), 3), ((-14, 0), 3)] {
        let (mut fixture, perch, inside) = perched(&[(offset, height)]);
        fixture.step(&mut Enc::default()).expect("the walls settle");
        assert_eq!(
            search_between(&fixture, perch, column(inside)).map(|breach| breach.wall),
            Err(pharmakos_sim::attack::BreachRefused::NoWay),
            "a pillar {height} high at {offset:?}"
        );
    }
}

/// The played half: the raid from the three-high pillar fourteen columns out
/// ends `Sealed` with its refusal counted, within a bounded count of ticks.
/// On `main` the search read the lead at the pillar's footing and named a
/// breach of the ring whose plant the carrier could never reach: the force
/// stayed `Launched` on the pillar for 3 000 ticks with nothing planted and
/// no `AttackEnded`.
#[test]
fn a_raid_from_a_pillar_it_cannot_step_off_ends_sealed() {
    let (fixture, perch, inside) = perched(&[((-14, 0), 3)]);
    let (mut runner, post, _) = raid_from_the_perch(fixture, perch, inside);
    let mut log = Vec::new();
    let ended = play_until(&mut runner, 400, &mut log, |_, log| {
        fired(log, EventKind::AttackEnded) > 0
    });
    assert!(
        ended,
        "the attack ends (phase {:?}, breach {:?})",
        row(runner.world(), post).phase,
        row(runner.world(), post).breach.map(|breach| breach.wall)
    );
    let ended = log
        .iter()
        .find(|event| event.kind == EventKind::AttackEnded)
        .map(|event| event.value);
    assert_eq!(ended, Some(4), "it ends `Sealed`");
    assert_eq!(
        runner.world().attack_counts().breach_no_way,
        1,
        "the refusal is counted"
    );
}

/// The stub one column off the ring is a step a walker takes both ways: the
/// lead on it plants where a walker stands, never on the stub.
#[test]
fn a_lead_on_a_stub_beside_the_ring_plants_where_a_walker_stands() {
    let (mut fixture, perch, inside) = perched(&[((-5, 0), 1)]);
    fixture.step(&mut Enc::default()).expect("the walls settle");
    let breach = search_between(&fixture, perch, column(inside)).expect("a breach");
    assert_a_breach(&fixture, perch, &breach);
    assert!(
        on_the_wall(breach.wall, inside, &ring(4)),
        "the breach {:?} is on the ring",
        breach.wall
    );
}

/// A double ring, and a ring two thick: the wall named is the first one the
/// way crosses, and the plant is outside every wall.
#[test]
fn the_breach_is_the_first_wall_crossed() {
    for (outer, inner) in [(ring(6), ring(4)), (ring(5), ring(4))] {
        let mut fixture = world();
        let here = field(&fixture);
        let inside = ground(&fixture, here, 24, 0);
        wall_columns(&mut fixture, inside, &outer, 3, seat(1));
        wall_columns(&mut fixture, inside, &inner, 3, seat(1));
        fixture.step(&mut Enc::default()).expect("the walls settle");
        let breach = search_between(&fixture, column(here), column(inside)).expect("a breach");
        assert_a_breach(&fixture, column(here), &breach);
        assert!(
            on_the_wall(breach.wall, inside, &outer),
            "the breach {:?} is on the outer wall",
            breach.wall
        );
    }
}

// ---------------------------------------------------------------------------
// A seeded property test over generated wall layouts.
// ---------------------------------------------------------------------------

/// xorshift64*, seeded: the layouts are a function of the seed.
struct LayoutRng(u64);

impl LayoutRng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: i32) -> i32 {
        i32::try_from(self.next() % u64::try_from(n).unwrap()).unwrap()
    }
}

/// A column's height as the independent reading below takes it: a breachable
/// wall at its footing (wall voxels stripped down to a solid non-wall voxel),
/// every other walkable column at its top.
fn footing_or_top(world: &World, node: u32) -> Option<i32> {
    let surface = world.surface();
    if !surface.walkable(node) {
        return None;
    }
    let top = surface.top(node);
    let (x, y) = surface.coord_of(node);
    if !breachable(world, x, y) {
        return Some(top);
    }
    let mut z = top;
    while z >= 0
        && world
            .voxels()
            .get([x, y, z])
            .is_some_and(|material| material.wall_of().is_some())
    {
        z -= 1;
    }
    world
        .voxels()
        .get([x, y, z])
        .filter(|material| material.is_solid())
        .map(|_| z)
}

/// The four cardinal neighbours of `node` on the map.
fn cardinal(world: &World, node: u32) -> impl Iterator<Item = u32> + '_ {
    let surface = world.surface();
    let (x, y) = surface.coord_of(node);
    [(1, 0), (-1, 0), (0, 1), (0, -1)]
        .into_iter()
        .filter_map(move |(dx, dy)| surface.node_of(x + dx, y + dy))
}

/// An independent reading of "a way through walls", from the columns marked
/// in `plants` (each a column a walker stands on that is no breachable wall):
/// a CARDINAL step onto a breachable column at its footing, then cardinal
/// steps over every column at its footing or top. Reaching the goal this way
/// means a breach exists that the search must find. It shares nothing with
/// the breach search's step rule.
fn a_cardinal_way_through_walls(world: &World, plants: &[bool], goal: u32) -> bool {
    let surface = world.surface();
    let n = usize::try_from(surface.node_count()).unwrap();
    let mut seen = vec![false; n];
    let mut queue = std::collections::VecDeque::new();
    for (at, plant) in plants.iter().enumerate() {
        let node = u32::try_from(at).unwrap();
        let (x, y) = surface.coord_of(node);
        if !plant || breachable(world, x, y) {
            continue;
        }
        let Some(stand) = surface.standing(node) else {
            continue;
        };
        for next in cardinal(world, node) {
            let (nx, ny) = surface.coord_of(next);
            if !breachable(world, nx, ny) {
                continue;
            }
            let Some(to) = footing_or_top(world, next) else {
                continue;
            };
            let slot = usize::try_from(next).unwrap();
            if (to - stand).abs() <= 1 && !seen[slot] {
                seen[slot] = true;
                queue.push_back(next);
            }
        }
    }
    while let Some(node) = queue.pop_front() {
        if node == goal {
            return true;
        }
        let Some(from) = footing_or_top(world, node) else {
            continue;
        };
        for next in cardinal(world, node) {
            let slot = usize::try_from(next).unwrap();
            if seen[slot] {
                continue;
            }
            let Some(to) = footing_or_top(world, next) else {
                continue;
            };
            if (to - from).abs() <= 1 {
                seen[slot] = true;
                queue.push_back(next);
            }
        }
    }
    false
}

/// The columns a walker from `start` reaches by CARDINAL steps of at most one
/// voxel over the tops it stands on: the review's reading (P1) of where the
/// start may walk, kept independent of `Surface::neighbour` and of the breach
/// search.
fn cardinal_walk(world: &World, start: u32) -> Vec<bool> {
    let surface = world.surface();
    let n = usize::try_from(surface.node_count()).unwrap();
    let mut seen = vec![false; n];
    let mut queue = std::collections::VecDeque::new();
    if surface.standing(start).is_some() {
        seen[usize::try_from(start).unwrap()] = true;
        queue.push_back(start);
    }
    while let Some(node) = queue.pop_front() {
        let Some(from) = surface.standing(node) else {
            continue;
        };
        for next in cardinal(world, node) {
            let slot = usize::try_from(next).unwrap();
            if seen[slot] {
                continue;
            }
            if surface
                .standing(next)
                .is_some_and(|to| (to - from).abs() <= 1)
            {
                seen[slot] = true;
                queue.push_back(next);
            }
        }
    }
    seen
}

/// The eight neighbours of `node` on the map.
fn around(world: &World, node: u32) -> impl Iterator<Item = u32> + '_ {
    let surface = world.surface();
    let (x, y) = surface.coord_of(node);
    [
        (1, 0),
        (-1, 0),
        (0, 1),
        (0, -1),
        (1, 1),
        (1, -1),
        (-1, 1),
        (-1, -1),
    ]
    .into_iter()
    .filter_map(move |(dx, dy)| surface.node_of(x + dx, y + dy))
}

/// The independent reading's step between adjacent columns `a` and `b` read
/// at heights `ha` and `hb`: at most one voxel up or down, and a diagonal's
/// two sides walkable and within one voxel of both ends where a walker stands
/// on them; its price from the map's step costs. Written here, not taken from
/// the breach search's step rule.
fn reference_step(world: &World, a: u32, ha: i32, b: u32, hb: i32) -> Option<i32> {
    let surface = world.surface();
    let costs = surface.costs();
    let ((ax, ay), (bx, by)) = (surface.coord_of(a), surface.coord_of(b));
    let (dx, dy) = (bx - ax, by - ay);
    if (hb - ha).abs() > 1 {
        return None;
    }
    let climb = if ha == hb { 0 } else { costs.climb() };
    if dx != 0 && dy != 0 {
        for (sx, sy) in [(ax + dx, ay), (ax, ay + dy)] {
            let side = surface.standing(surface.node_of(sx, sy)?)?;
            if (side - ha).abs() > 1 || (side - hb).abs() > 1 {
                return None;
            }
        }
        return Some(costs.diagonal() + climb);
    }
    Some(costs.cardinal() + climb)
}

/// Dijkstra over `steps` from `from`: every column's least cost, `i32::MAX`
/// where none.
fn least_costs(world: &World, from: u32, steps: impl Fn(u32) -> Vec<(u32, i32)>) -> Vec<i32> {
    use std::cmp::Reverse;
    let n = usize::try_from(world.surface().node_count()).unwrap();
    let mut best = vec![i32::MAX; n];
    let mut heap = std::collections::BinaryHeap::new();
    best[usize::try_from(from).unwrap()] = 0;
    heap.push(Reverse((0, from)));
    while let Some(Reverse((cost, node))) = heap.pop() {
        if cost > best[usize::try_from(node).unwrap()] {
            continue;
        }
        for (next, step) in steps(node) {
            let at = usize::try_from(next).unwrap();
            if cost + step < best[at] {
                best[at] = cost + step;
                heap.push(Reverse((cost + step, next)));
            }
        }
    }
    best
}

/// The least cost of a breach way from `start` to `goal`, by the independent
/// reading: a walker's way to a column that is no breachable wall, the step
/// onto a breachable wall at its footing ([`BREACH_COLUMN_COST`] more), then
/// steps over footings and tops (each breachable column entered
/// [`BREACH_COLUMN_COST`] more). With it, the cost of the way through the one
/// crossing from `plant` onto `wall`. Both `None` where there is no such way.
fn least_breach_costs(
    world: &World,
    start: u32,
    goal: u32,
    plant_and_wall: Option<(u32, u32)>,
) -> (Option<i32>, Option<i32>) {
    use pharmakos_sim::attack::BREACH_COLUMN_COST;
    let surface = world.surface();
    let is_wall = |node: u32| {
        let (x, y) = surface.coord_of(node);
        breachable(world, x, y)
    };
    let walk = least_costs(world, start, |node| {
        (0..8).filter_map(|k| surface.neighbour(node, k)).collect()
    });
    // Back from the goal over the crossed layer: the cost of the step from a
    // column onto the next, the next's demolition included.
    let back = least_costs(world, goal, |to| {
        let Some(hto) = footing_or_top(world, to) else {
            return Vec::new();
        };
        let demolish = if is_wall(to) { BREACH_COLUMN_COST } else { 0 };
        around(world, to)
            .filter_map(|from| {
                let hfrom = footing_or_top(world, from)?;
                Some((
                    from,
                    reference_step(world, from, hfrom, to, hto)? + demolish,
                ))
            })
            .collect()
    });
    let through = |plant: u32, wall: u32| -> Option<i32> {
        let before = walk[usize::try_from(plant).unwrap()];
        let after = back[usize::try_from(wall).unwrap()];
        if before == i32::MAX || after == i32::MAX || is_wall(plant) || !is_wall(wall) {
            return None;
        }
        let step = reference_step(
            world,
            plant,
            surface.standing(plant)?,
            wall,
            footing_or_top(world, wall)?,
        )?;
        Some(before + step + BREACH_COLUMN_COST + after)
    };
    let n = surface.node_count();
    let least = (0..n)
        .flat_map(|plant| around(world, plant).map(move |wall| (plant, wall)))
        .filter_map(|(plant, wall)| through(plant, wall))
        .min();
    (
        least,
        plant_and_wall.and_then(|(plant, wall)| through(plant, wall)),
    )
}

/// One generated layout round `centre`, settled.
fn generated_layout(base: &World, rng: &mut LayoutRng, centre: [i32; 2]) -> World {
    let mut world = base.clone();
    let [cx, cy] = centre;
    let half = 3 + rng.below(4);
    let mut columns: Vec<(i32, i32)> = match rng.below(5) {
        0 => ring(half),
        1 => diamond(half),
        2 => corner_to_corner(half),
        3 => {
            let mut scattered = ring(half);
            for _ in 0..(20 + rng.below(80)) {
                scattered.push((rng.below(17) - 8, rng.below(17) - 8));
            }
            scattered
        }
        _ => {
            let mut half_and_half: Vec<(i32, i32)> =
                ring(half).into_iter().filter(|(dx, _)| *dx <= 0).collect();
            half_and_half.extend(diamond(half).into_iter().filter(|(dx, _)| *dx > 0));
            half_and_half
        }
    };
    for _ in 0..rng.below(2) {
        if !columns.is_empty() {
            let at = usize::try_from(rng.below(i32::try_from(columns.len()).unwrap())).unwrap();
            columns.remove(at);
        }
    }
    for _ in 0..rng.below(6) {
        let (x, y) = (cx + rng.below(19) - 9, cy + rng.below(19) - 9);
        let top = world.standing_point([x, y, 0])[2].floor_voxels();
        for z in top..top + 1 + rng.below(4) {
            let _ = world.request_voxel_edit(VoxelEdit::Set {
                at: [x, y, z],
                material: Material::STONE,
                by: None,
            });
        }
    }
    world
        .step(&mut Enc::default())
        .expect("the terrain settles");
    let mut stubs = Vec::new();
    for _ in 0..rng.below(6) {
        stubs.push((rng.below(21) - 10, rng.below(21) - 10, 1 + rng.below(3)));
    }
    for &(dx, dy) in &columns {
        let (x, y) = (cx + dx, cy + dy);
        let top = world.standing_point([x, y, 0])[2].floor_voxels();
        let height = if rng.below(60) == 0 {
            1
        } else {
            2 + rng.below(2)
        };
        let owner = match rng.below(10) {
            0 => seat(0),
            1 => seat(2),
            _ => seat(1),
        };
        for z in top..top + height {
            let of = if rng.below(8) == 0 { seat(1) } else { owner };
            let _ = world.request_voxel_edit(VoxelEdit::Set {
                at: [x, y, z],
                material: Material::wall(of, rng.below(4) == 0).expect("a wall id"),
                by: None,
            });
        }
    }
    for &(dx, dy, height) in &stubs {
        let (x, y) = (cx + dx, cy + dy);
        let top = world.standing_point([x, y, 0])[2].floor_voxels();
        for z in top..top + height {
            let _ = world.request_voxel_edit(VoxelEdit::Set {
                at: [x, y, z],
                material: Material::wall(seat(1), false).expect("a wall id"),
                by: None,
            });
        }
    }
    world.step(&mut Enc::default()).expect("the walls settle");
    if rng.below(3) == 0 && !columns.is_empty() {
        let at = usize::try_from(rng.below(i32::try_from(columns.len()).unwrap())).unwrap();
        let (dx, dy) = columns[at];
        let z = world.standing_point([cx + dx, cy + dy, 0])[2].floor_voxels();
        let _ = world.request_voxel_edit(VoxelEdit::Crater {
            centre: [cx + dx, cy + dy, z],
            radius: 1 + rng.below(2),
            by: None,
        });
        world.step(&mut Enc::default()).expect("the crater settles");
    }
    world
}

/// Where a generated layout stands: round `inside`, or, against a map edge,
/// two to four columns in from it.
fn layout_centre(rng: &mut LayoutRng, edge: bool, inside: [i32; 2], size: [i32; 2]) -> [i32; 2] {
    let [ix, iy] = inside;
    if !edge {
        return inside;
    }
    match rng.below(4) {
        0 => [2 + rng.below(3), iy],
        1 => [size[0] - 3 - rng.below(3), iy],
        2 => [ix, 2 + rng.below(3)],
        _ => [ix, size[1] - 3 - rng.below(3)],
    }
}

/// Where a case starts: a column near the layout, a breachable column near
/// it when one is found, or the field (for a layout against an edge, twelve
/// columns in from the edge).
fn layout_start(
    world: &World,
    rng: &mut LayoutRng,
    centre: [i32; 2],
    edge: Option<[i32; 2]>,
    field_at: [i32; 2],
) -> Option<u32> {
    let surface = world.surface();
    match rng.below(4) {
        0 => surface.node_of(
            centre[0] + rng.below(21) - 10,
            centre[1] + rng.below(21) - 10,
        ),
        1 => {
            for _ in 0..40 {
                let (x, y) = (
                    centre[0] + rng.below(21) - 10,
                    centre[1] + rng.below(21) - 10,
                );
                if breachable(world, x, y) {
                    return surface.node_of(x, y);
                }
            }
            None
        }
        _ => {
            let away = |at: i32, extent: i32| {
                if at < 10 {
                    12
                } else if at > extent - 10 {
                    -12
                } else {
                    0
                }
            };
            match edge {
                Some(size) => surface.node_of(
                    centre[0] + away(centre[0], size[0]),
                    centre[1] + away(centre[1], size[1]),
                ),
                None => surface.node_of(field_at[0], field_at[1]),
            }
        }
    }
}

/// One sealed case of [`the_breach_search_holds_over_generated_walls`] held to
/// its invariants 2, 4, 5 and 6: the search's answer handed back, its breach
/// dropped.
fn held_to_the_readings(
    world: &World,
    case: u32,
    start: u32,
    goal: u32,
    walker: &[bool],
    found: Result<pharmakos_sim::attack::Breach, pharmakos_sim::attack::BreachRefused>,
) -> Result<(), pharmakos_sim::attack::BreachRefused> {
    let surface = world.surface();
    let (sx, sy) = surface.coord_of(start);
    match found {
        Ok(breach) => {
            assert_a_breach(world, [sx, sy], &breach);
            let plant = surface.node_of(breach.plant[0], breach.plant[1]).unwrap();
            let wall = surface.node_of(breach.wall[0], breach.wall[1]).unwrap();
            let (least, chosen) = least_breach_costs(world, start, goal, Some((plant, wall)));
            assert!(
                least.is_some() && chosen == least,
                "case {case}: the breach's way costs {chosen:?}, the least is {least:?}"
            );
            Ok(())
        }
        Err(pharmakos_sim::attack::BreachRefused::NoWay) => {
            assert_eq!(
                least_breach_costs(world, start, goal, None).0,
                None,
                "case {case}: `NoWay` where the independent reading prices a way"
            );
            assert!(
                !a_cardinal_way_through_walls(world, &cardinal_walk(world, start), goal),
                "case {case}: `NoWay` where a cardinal way from the start through walls reaches the goal"
            );
            assert!(
                !a_cardinal_way_through_walls(world, walker, goal),
                "case {case}: `NoWay` where a walker reaches a wall whose far side reaches the goal"
            );
            Err(pharmakos_sim::attack::BreachRefused::NoWay)
        }
        Err(refused) => {
            assert_eq!(
                refused,
                pharmakos_sim::attack::BreachRefused::Exhausted,
                "case {case}"
            );
            Err(refused)
        }
    }
}

/// The property test's seed and its count of generated layouts.
const LAYOUT_SEED: u64 = 0x5E77_1E3E_0000_0003;
const LAYOUT_CASES: u32 = 240;

/// Over generated wall layouts (rings, diamonds, boxes joined corner to
/// corner, scatters, half ring and half diamond; heights 1 to 3; the seat's
/// own walls and a third seat's, mixed stacks, cracked voxels, stone steps,
/// one- to three-high stubs and craters; a quarter against a map edge;
/// starts on the field, near the layout, or on a breachable column), seeded,
/// with a counted number of cases and no clock:
///
/// 1. the oracle and a breadth-first search over the walkers' own rule agree
///    on what is sealed;
/// 2. every breach names a breachable wall, and a plant a walker from the
///    start reaches, standing where a walker stands, on no breachable wall,
///    beside the wall;
/// 3. the search is a function of the world: run again on a used scratch, it
///    answers the same;
/// 4. it never answers `NoWay` where an independent cardinal reading (P1's:
///    cardinal steps from the start over the tops it stands on, then a step
///    from a column that is no breachable wall onto one at its footing, then
///    cardinal steps over footings and tops) reaches the goal;
/// 5. where a walker (`Surface::neighbour`) reaches a column, no breachable
///    wall, beside a breachable wall whose far side reaches the goal (the same
///    independent reading), the answer is never `NoWay`.
///
/// 6. a breach lies on a least-cost way, priced by an independent Dijkstra
///    (the walkers' moves, the one crossing, then footings and tops, its own
///    step rule), and `NoWay` is answered only where that reading prices none.
///
/// A plant is never a breachable wall, so a lead whose every reachable column
/// is one has no breach: every reading holds to that rule.
#[test]
fn the_breach_search_holds_over_generated_walls() {
    let mut base = world();
    let here = field(&base);
    let inside = ground(&base, here, 24, 0);
    let own_at = ground(&base, inside, 0, 9);
    let _own = beacon(&mut base, seat(0), own_at, MandateKind::Defend, FREE_FIRE);
    base.step(&mut Enc::default()).expect("the beacon settles");
    let [sx, sy, _] = base.surface().size();
    let size = [sx, sy];
    let mut rng = LayoutRng(LAYOUT_SEED);
    let (mut cases, mut sealed_cases, mut breaches, mut no_ways) = (0, 0, 0, 0);
    for case in 0..LAYOUT_CASES {
        let edge = rng.below(4) == 0;
        let centre = layout_centre(&mut rng, edge, column(inside), size);
        let world = generated_layout(&base, &mut rng, centre);
        let surface = world.surface();
        let goal = surface.node_of(centre[0], centre[1]).expect("a column");
        let start = layout_start(&world, &mut rng, centre, edge.then_some(size), column(here));
        let Some(start) = start else {
            continue;
        };
        if start == goal || !surface.walkable(start) {
            continue;
        }
        cases += 1;
        let walker = walker_reach(&world, start);
        let sealed = !world.clusters().connected(surface, start, goal);
        assert_eq!(
            sealed,
            !walker[usize::try_from(goal).unwrap()],
            "case {case}: the oracle and the walkers disagree on the seal"
        );
        if !sealed {
            continue;
        }
        sealed_cases += 1;
        let mut scratch = BreachScratch::new(surface.node_count());
        let found = breach_search(
            &world,
            &mut scratch,
            seat(0),
            SealedWay::of(&world, start, goal).expect("a sealed way"),
        );
        let again = breach_search(
            &world,
            &mut scratch,
            seat(0),
            SealedWay::of(&world, start, goal).expect("a sealed way"),
        );
        assert_eq!(
            found, again,
            "case {case}: the search is a function of the world"
        );
        match held_to_the_readings(&world, case, start, goal, &walker, found) {
            Ok(()) => breaches += 1,
            Err(pharmakos_sim::attack::BreachRefused::NoWay) => no_ways += 1,
            Err(_) => {}
        }
    }
    eprintln!(
        "generated layouts: seed {LAYOUT_SEED:#x}, {cases} cases, {sealed_cases} sealed, {breaches} breaches, {no_ways} without a way, on a map of {} columns",
        base.surface().node_count()
    );
    assert!(
        cases * 2 >= LAYOUT_CASES && sealed_cases >= 40 && breaches >= 20 && no_ways >= 5,
        "the generator covers its shapes: {cases} cases, {sealed_cases} sealed, {breaches} breaches, {no_ways} without a way"
    );
}

/// A breach whose plant the lead no longer reaches is planned again: here the
/// plant is boxed in by stone after the breach is chosen and before the
/// charge is planted, so the carrier could never stand within reach of it.
/// The force names another breach, plants there and gets in. Without the
/// re-plan the carrier walks at the box for ever and the force never ends.
#[test]
fn a_breach_whose_plant_is_walled_off_is_planned_again() {
    let (mut runner, post, raider, inside) = walled(3);
    let mut log = Vec::new();
    assert!(
        play_until(&mut runner, 2_000, &mut log, |world, _| {
            charge_of(world, post) == Some(ChargeState::Loaded)
        }),
        "the breach's charge is bought and carried"
    );
    let first = row(runner.world(), post).breach.expect("a breach");
    let paid = treasury(runner.world());
    assert_eq!(
        fired(&log, EventKind::ChargePlanted),
        0,
        "nothing is planted yet"
    );
    // A box of stone four high, two and three columns out from the plant, so
    // no column a walker reaches is within reach of it.
    let [px, py, _] = first.plant;
    for dy in -3..=3_i32 {
        for dx in -3..=3_i32 {
            if dx.abs().max(dy.abs()) < 2 {
                continue;
            }
            let (x, y) = (px + dx, py + dy);
            let Some(top) = runner.world().voxels().top_solid_z(x, y) else {
                continue;
            };
            for z in top + 1..=top + 4 {
                runner
                    .world_mut()
                    .request_voxel_edit(VoxelEdit::Set {
                        at: [x, y, z],
                        material: Material::STONE,
                        by: None,
                    })
                    .expect("a stone voxel is filed");
            }
        }
    }
    play(&mut runner, 3, &mut log);
    let second = row(runner.world(), post)
        .breach
        .expect("a breach planned again");
    assert_ne!(
        second.plant, first.plant,
        "the new plant is not the walled-off one"
    );
    assert_eq!(
        (second.charge, second.carrier),
        (ChargeState::Loaded, first.carrier),
        "the carried charge and its carrier are kept for the new breach"
    );
    assert_eq!(treasury(runner.world()), paid, "no second charge is bought");
    assert_eq!(fired(&log, EventKind::ChargeLost), 0, "no charge is lost");
    let through = play_until(&mut runner, 6_000, &mut log, |world, _| {
        within(unit_at(world, raider), inside, 2)
    });
    let planted = log
        .iter()
        .find(|event| event.kind == EventKind::ChargePlanted)
        .map(|event| event.at);
    assert!(
        through,
        "the raider gets in (planted at {planted:?}, ended {}, breach {:?})",
        fired(&log, EventKind::AttackEnded),
        row(runner.world(), post)
            .breach
            .map(|breach| (breach.wall, breach.plant))
    );
    assert!(
        planted.is_some(),
        "a charge is planted, at a plant other than the walled-off one"
    );
}

// ---------------------------------------------------------------------------
// A charge in hand (lane `settle`, fix pass 4): kept on every re-plan, and
// never gone unannounced.
// ---------------------------------------------------------------------------

/// Seat 0's money.
fn treasury(world: &World) -> i64 {
    world.seats().treasuries()[0].raw()
}

/// The charge of the breach `post` holds, if it holds one.
fn charge_of(world: &World, post: BeaconId) -> Option<ChargeState> {
    world
        .attack()
        .row_of(post)
        .and_then(|row| row.breach)
        .map(|breach| breach.charge)
}

/// A raid with `breach` allowed from the field on a point inside a ring of
/// seat 1's wall four columns out, three high, with `raiders` raiders and
/// `settings` over the raid's own: played until its charge is carried.
fn carrying(
    raiders: u32,
    settings: impl FnOnce(AttackSettings, &mut World, [Fx; 3]) -> AttackSettings,
) -> (Runner, BeaconId, Vec<UnitId>, [Fx; 3], Vec<Event>) {
    let mut fixture = world();
    let here = field(&fixture);
    let inside = ground(&fixture, here, 24, 0);
    wall_ring(&mut fixture, inside, 4, 3, seat(1));
    let base = AttackSettings {
        breach: true,
        ..raid(Aim::Voxel(voxel(inside)), raiders)
    };
    let settings = settings(base, &mut fixture, inside);
    let post = attack(&mut fixture, seat(0), here, FREE_FIRE, settings);
    let force = (0..raiders)
        .map(|_| unit(&mut fixture, seat(0), post, UnitKind::Raider, here))
        .collect();
    let mut runner = Runner::new(fixture).expect("a world from `World::new`");
    runner
        .world_mut()
        .step(&mut Enc::default())
        .expect("the walls settle");
    assert_eq!(runner.begin_push(), Ok(true));
    let mut log = Vec::new();
    assert!(
        play_until(&mut runner, 2_000, &mut log, |world, _| {
            charge_of(world, post) == Some(ChargeState::Loaded)
        }),
        "the breach's charge is bought and carried"
    );
    (runner, post, force, inside, log)
}

/// The value of the first `attack_ended` in `log`, if one fired.
fn ended_with(log: &[Event]) -> Option<i64> {
    log.iter()
        .find(|event| event.kind == EventKind::AttackEnded)
        .map(|event| event.value)
}

/// R4-LOADED-WALL: the breach wall's top voxel knocked off while the charge is
/// carried, the ring still sealing at two high. The charge is kept for the
/// breach planned again: no second charge is bought, none is lost, and it is
/// planted and fires.
#[test]
fn a_carried_charge_survives_the_breach_walls_fall() {
    let (mut runner, post, _, inside, mut log) = carrying(1, |settings, _, _| settings);
    let paid = treasury(runner.world());
    let first = row(runner.world(), post).breach.expect("a breach");
    runner
        .world_mut()
        .request_voxel_edit(VoxelEdit::Set {
            at: first.wall,
            material: Material::AIR,
            by: None,
        })
        .expect("the voxel is filed");
    play(&mut runner, 3, &mut log);
    let second = row(runner.world(), post)
        .breach
        .expect("a breach planned again");
    assert_eq!(
        (second.charge, second.carrier),
        (ChargeState::Loaded, first.carrier),
        "the carried charge and its carrier are kept"
    );
    let _ = play_until(&mut runner, 6_000, &mut log, |_, log| {
        fired(log, EventKind::ChargeDetonated) > 0
    });
    assert_eq!(treasury(runner.world()), paid, "no second charge is bought");
    assert_eq!(fired(&log, EventKind::ChargeLost), 0, "no charge is lost");
    assert_eq!(
        fired(&log, EventKind::ChargePlanted),
        1,
        "the kept charge is planted"
    );
    assert!(
        fired(&log, EventKind::ChargeDetonated) > 0,
        "and fires (inside {:?})",
        column(inside)
    );
}

/// The beacon's `roe` changed while the charge is carried: the attack ends
/// `StoodDown`, and the charge is announced lost, once.
#[test]
fn a_charge_carried_when_the_attack_stands_down_is_announced_lost() {
    let (mut runner, post, _, _, mut log) = carrying(1, |settings, _, _| settings);
    runner.world_mut().set_common(
        post,
        Common {
            roe: 1,
            retreat_hp_pct: 0,
        },
    );
    let ended = play_until(&mut runner, 200, &mut log, |_, log| {
        fired(log, EventKind::AttackEnded) > 0
    });
    assert!(ended, "the attack ends");
    assert_eq!(ended_with(&log), Some(6), "it ends `StoodDown`");
    assert_eq!(
        fired(&log, EventKind::ChargeLost),
        1,
        "the charge is announced lost, once"
    );
    assert_eq!(
        charge_of(runner.world(), post),
        None,
        "and the breach is gone"
    );
}

/// The ring taken down while the charge is carried: the way is open, the
/// breach goes, and the charge with it, announced lost, once.
#[test]
fn a_charge_carried_when_the_way_opens_is_announced_lost() {
    let (mut runner, post, _, inside, mut log) = carrying(1, |settings, _, _| settings);
    let [cx, cy] = column(inside);
    for dy in -4..=4_i32 {
        for dx in -4..=4_i32 {
            if dx.abs() != 4 && dy.abs() != 4 {
                continue;
            }
            let (x, y) = (cx + dx, cy + dy);
            let Some(top) = runner.world().voxels().top_solid_z(x, y) else {
                continue;
            };
            for z in top - 2..=top {
                runner
                    .world_mut()
                    .request_voxel_edit(VoxelEdit::Set {
                        at: [x, y, z],
                        material: Material::AIR,
                        by: None,
                    })
                    .expect("the voxel is filed");
            }
        }
    }
    play(&mut runner, 3, &mut log);
    assert_eq!(charge_of(runner.world(), post), None, "the breach is gone");
    assert_eq!(
        fired(&log, EventKind::ChargeLost),
        1,
        "the charge is announced lost, once"
    );
}

/// The lead walled in by stone while the charge is carried: no breach is left
/// to plan, the attack ends `Sealed` with the refusal counted, and the
/// charge is announced lost, once.
#[test]
fn a_charge_carried_when_a_replan_finds_no_breach_is_announced_lost() {
    let (mut runner, _, force, _, mut log) = carrying(1, |settings, _, _| settings);
    let [x, y] = column(unit_at(runner.world(), force[0]));
    for dy in -1..=1_i32 {
        for dx in -1..=1_i32 {
            if (dx, dy) == (0, 0) {
                continue;
            }
            let Some(top) = runner.world().voxels().top_solid_z(x + dx, y + dy) else {
                continue;
            };
            for z in top + 1..=top + 5 {
                runner
                    .world_mut()
                    .request_voxel_edit(VoxelEdit::Set {
                        at: [x + dx, y + dy, z],
                        material: Material::STONE,
                        by: None,
                    })
                    .expect("the voxel is filed");
            }
        }
    }
    let ended = play_until(&mut runner, 200, &mut log, |_, log| {
        fired(log, EventKind::AttackEnded) > 0
    });
    assert!(ended, "the attack ends");
    assert_eq!(ended_with(&log), Some(4), "it ends `Sealed`");
    assert_eq!(
        runner.world().attack_counts().breach_no_way,
        1,
        "the refusal is counted"
    );
    assert_eq!(
        fired(&log, EventKind::ChargeLost),
        1,
        "the charge is announced lost, once"
    );
}

/// Two raiders, a retreat below 60 % of the force: the raider that is not the
/// carrier killed while the charge is carried. The attack ends `Retreat`, and
/// the charge is announced lost, once.
#[test]
fn a_charge_carried_when_the_force_retreats_is_announced_lost() {
    let (mut runner, post, force, _, mut log) = carrying(2, |settings, _, _| AttackSettings {
        retreat_pct: Some(60),
        ..settings
    });
    let carrier = row(runner.world(), post).breach.expect("a breach").carrier;
    let other = force
        .iter()
        .copied()
        .find(|raider| raider.raw() != carrier)
        .expect("a raider that is not the carrier");
    hit(
        runner.world_mut(),
        DamageTarget::Unit(other),
        1_000_000,
        seat(1),
    );
    let ended = play_until(&mut runner, 20, &mut log, |_, log| {
        fired(log, EventKind::AttackEnded) > 0
    });
    assert!(ended, "the attack ends");
    assert_eq!(ended_with(&log), Some(2), "it ends `Retreat`");
    assert_eq!(
        fired(&log, EventKind::ChargeLost),
        1,
        "the charge is announced lost, once"
    );
}

/// A raid on a beacon inside the ring, the beacon destroyed while the charge
/// is carried and no retarget allowed: the attack ends `TargetGone`, and the
/// charge is announced lost, once.
#[test]
fn a_charge_carried_when_the_target_is_gone_is_announced_lost() {
    let mut target = None;
    let (mut runner, post, _, _, mut log) = carrying(1, |settings, world, inside| {
        let id = beacon(world, seat(1), inside, MandateKind::Build, FREE_FIRE);
        target = Some(id);
        AttackSettings {
            aim: Aim::Beacon(id),
            ..settings
        }
    });
    let target = target.expect("the target beacon");
    hit(
        runner.world_mut(),
        DamageTarget::Beacon(target),
        1_000_000,
        seat(0),
    );
    let ended = play_until(&mut runner, 20, &mut log, |_, log| {
        fired(log, EventKind::AttackEnded) > 0
    });
    assert!(ended, "the attack ends");
    assert_eq!(ended_with(&log), Some(1), "it ends `TargetGone`");
    assert_eq!(
        fired(&log, EventKind::ChargeLost),
        1,
        "the charge is announced lost, once"
    );
    assert_eq!(
        charge_of(runner.world(), post),
        None,
        "and the breach is gone"
    );
}
