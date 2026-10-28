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
    Aim, AttackSettings, BreachScratch, ChargeState, Phase, TargetSpec, breach_search,
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
    let mut runner = Runner::new(world);
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
    let mut runner = Runner::new(world());
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
    let mut runner = Runner::new(fixture);
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
    let mut runner = Runner::new(fixture);
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
    let bytes = Snapshot::capture(runner.world()).to_bytes().unwrap();
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
    let mut scratch = BreachScratch::new(world.surface().node_count());
    let node = world
        .surface()
        .node_of(column(here)[0], column(here)[1])
        .unwrap();
    assert!(
        breach_search(&world, &mut scratch, seat(0), node, node).is_err(),
        "no wall, no breach"
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
    let bytes = Snapshot::capture(&beacon_raid).to_bytes().unwrap();
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

/// Two ground points near `centre`, at least 6 columns apart, whose squared
/// distances from `centre` are equal: heights differ, so the pair is searched
/// for among the columns 6 to 12 out rather than taken from a mirror.
fn tied_pair(world: &World, centre: [Fx; 3]) -> ([Fx; 3], [Fx; 3]) {
    let mut seen: Vec<(Sq, [Fx; 3])> = Vec::new();
    for dy in -12..=12 {
        for dx in -12..=12 {
            if !(36..=144).contains(&(dx * dx + dy * dy)) {
                continue;
            }
            let here = ground(world, centre, dx, dy);
            let d2 = Sq::between(here, centre);
            let [x, y] = column(here);
            if let Some((_, there)) = seen.iter().find(|(other, there)| {
                let [ox, oy] = column(*there);
                *other == d2 && (x - ox).abs().max((y - oy).abs()) >= 6
            }) {
                return (*there, here);
            }
            seen.push((d2, here));
        }
    }
    panic!("no two ground points tie round the field on this seed");
}

#[test]
fn a_tie_between_two_equally_near_known_beacons_goes_to_the_lowest_id() {
    let mut fixture = world();
    let post = nearest_known_raid(&mut fixture);
    // Staging is the beacon itself, so "nearest" is measured from there.
    let from = beacon_at(&fixture, post);
    let (first_at, second_at) = tied_pair(&fixture, from);
    let first = beacon(&mut fixture, seat(1), first_at, MandateKind::Build, 0);
    let second = beacon(&mut fixture, seat(1), second_at, MandateKind::Build, 0);
    assert!(first < second);
    assert_eq!(
        Sq::between(beacon_at(&fixture, first), from),
        Sq::between(beacon_at(&fixture, second), from),
        "the two stand equally near the staging point"
    );
    let mut runner = pushing(fixture);
    let mut log = Vec::new();
    let launched = play_until(&mut runner, 100, &mut log, |world, _| {
        world
            .attack()
            .row_of(post)
            .is_some_and(|row| row.phase == Phase::Launched)
    });
    assert!(launched, "both are sighted at once, so the raid launches");
    assert_eq!(
        row(runner.world(), post).target,
        AssetId::of_beacon(first).unwrap().raw(),
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
    let enemy_raw = AssetId::of_beacon(enemy).unwrap().raw();
    let sighted = |world: &World| {
        let table = world.sightings();
        table
            .seats()
            .iter()
            .zip(table.assets())
            .any(|(by, of)| *by == seat(0).raw() && *of == enemy_raw)
    };
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
    // No third: `waves` is read.
    hit(
        runner.world_mut(),
        DamageTarget::Beacon(far),
        1_000_000,
        seat(1),
    );
    play(&mut runner, 600, &mut log);
    assert_eq!(
        fired(&log, EventKind::AttackLaunched),
        2,
        "at most two waves"
    );
}
