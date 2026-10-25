// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! S2's `dfnd`: the Defend mandate, the threat model and the bodyguard (S2's
//! plan, task `dfnd`; its Acceptance list, under the names it gives them).
//!
//! The arrangements are put in the world through fixture seams
//! ([`World::place_beacon_directly`], [`World::set_defend`],
//! [`World::field_unit`]), so each test isolates one rule; the committed
//! scenario `scenarios/s2/guards-escort-the-commander` plays the same rules
//! from a sealed playbook. Numbers are read from the rules table, never pinned.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules, not an integration test's helpers. A panic is this file's failure report: these are panic lints, not determinism lints (AGENTS.md section 5)."
)]

use pharmakos_sim::interpreter::{
    BeaconSpec, Filter, beacon_under_attack, resolve_beacon_in, resolve_beacon_now,
};
use pharmakos_sim::knowledge::AssetId;
use pharmakos_sim::math::fixed::{Fx, Sq};
use pharmakos_sim::math::quantity::Damage;
use pharmakos_sim::runner::Runner;
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::snapshot::Snapshot;
use pharmakos_sim::tables::{
    BeaconId, Common, DefendSettings, NO_QUARRY, PRIORITY_NORMAL, Pursue, SeatId, UnitId, UnitKind,
};
use pharmakos_sim::world::{DamageOrder, DamageTarget, Roe, World, WorldConfig};
use pharmakos_sim::{MatchSettings, RulesTable, default_rules_path};
use std::path::PathBuf;

const SEED: u64 = 0x00F1_2E00_C0DE_0001;

/// `MandateSettings.Roe`'s wire values.
const HOLD_FIRE: u8 = 1;
const RETURN_FIRE: u8 = 2;

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

fn sphere() -> i32 {
    i32::from(rules().sphere_radius_voxels())
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

fn defend(world: &mut World, of: SeatId, at: [Fx; 3], settings: DefendSettings) -> BeaconId {
    let id = beacon(world, of, at, MandateKind::Defend, 0);
    assert!(world.set_defend(id, settings, &[]), "a Defend row");
    id
}

fn unit(world: &mut World, of: SeatId, home: BeaconId, kind: UnitKind, at: [Fx; 3]) -> UnitId {
    world.field_unit(of, home, kind, at).expect("a unit")
}

fn unit_row(world: &World, id: UnitId) -> u32 {
    world.units().row_of(id).expect("a live unit").raw()
}

fn unit_at(world: &World, id: UnitId) -> [Fx; 3] {
    world.units().positions()[world.units().row_of(id).expect("a live unit").index()]
}

fn distance2(a: [Fx; 3], b: [Fx; 3]) -> i64 {
    Sq::between(a, b).raw()
}

fn within(a: [Fx; 3], b: [Fx; 3], voxels: i16) -> bool {
    Sq::between(a, b) <= Sq::of_radius(Fx::from_voxels(voxels))
}

fn pushing(world: World) -> Runner {
    let mut runner = Runner::new(world);
    assert_eq!(runner.begin_push(), Ok(true), "the Push begins");
    runner
}

fn play(runner: &mut Runner, ticks: u32) {
    for _ in 0..ticks {
        runner
            .step()
            .expect("the tick runs")
            .expect("still in the Push");
        runner.clear_events();
    }
}

fn quarry(world: &World, beacon: BeaconId) -> u32 {
    world.defend().row_of(beacon).expect("a Defend row").quarry
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

/// Every Defend setting written, for a beacon that holds the whole sphere.
fn holding(pursue: Pursue, escort: bool) -> DefendSettings {
    DefendSettings {
        engagement_pct: 100,
        pursue,
        pickets: 0,
        escort,
    }
}

#[test]
fn a_defend_beacons_unset_roe_is_free_fire() {
    assert_eq!(Roe::of_setting(0, MandateKind::Defend), Some(Roe::Free));
    assert_eq!(Roe::of_setting(0, MandateKind::Build), Some(Roe::Return));
    assert_eq!(
        Roe::of_setting(RETURN_FIRE, MandateKind::Defend),
        Some(Roe::Return)
    );

    // An explicit RETURN_FIRE starts no fight: an enemy raider in the sphere
    // is no quarry until it hits one of the seat's own, and an unset roe
    // takes it at once (item 148 (2)).
    let mut fixture = world();
    let here = field(&fixture);
    let quiet = defend(&mut fixture, seat(0), here, holding(Pursue::Never, false));
    let spot_1 = ground(&fixture, here, 60, 0);
    let enemy_home = beacon(&mut fixture, seat(1), spot_1, MandateKind::Build, HOLD_FIRE);
    let spot_2 = ground(&fixture, here, 4, 0);
    let raider = unit(&mut fixture, seat(1), enemy_home, UnitKind::Raider, spot_2);
    let mut free = fixture.clone();
    fixture.set_common(
        quiet,
        Common {
            roe: RETURN_FIRE,
            retreat_hp_pct: 0,
        },
    );
    let mut runner = pushing(fixture);
    play(&mut runner, 1);
    assert_eq!(
        quarry(runner.world(), quiet),
        NO_QUARRY,
        "RETURN_FIRE starts no fight"
    );
    let mut runner_free = {
        free.set_common(
            quiet,
            Common {
                roe: 0,
                retreat_hp_pct: 0,
            },
        );
        pushing(free)
    };
    play(&mut runner_free, 1);
    assert_eq!(
        quarry(runner_free.world(), quiet),
        AssetId::of_unit(raider).unwrap().raw(),
        "an unset roe on a Defend beacon is FREE_FIRE"
    );
    // The raider hits the beacon: now RETURN_FIRE answers it.
    hit(runner.world_mut(), DamageTarget::Beacon(quiet), 1, seat(1));
    play(&mut runner, 2);
    // The order above names no source, so the beacon remembers no attacker:
    // still no quarry. With the raider as the source it answers.
    assert_eq!(quarry(runner.world(), quiet), NO_QUARRY);
    runner
        .world_mut()
        .request_damage(DamageOrder {
            target: DamageTarget::Beacon(quiet),
            amount: Damage::new(1).unwrap(),
            by: seat(1),
            source: Some(AssetId::of_unit(raider).unwrap()),
        })
        .unwrap();
    play(&mut runner, 2);
    assert_eq!(
        quarry(runner.world(), quiet),
        AssetId::of_unit(raider).unwrap().raw(),
        "RETURN_FIRE answers the asset that hit its seat"
    );
}

#[test]
fn the_most_threatened_beacon_holds_the_most_threat() {
    let mut world = world();
    let here = field(&world);
    let r = sphere();
    let quiet = beacon(&mut world, seat(0), here, MandateKind::Build, 0);
    let east = ground(&world, here, 3 * r, 0);
    let threatened = beacon(&mut world, seat(0), east, MandateKind::Build, 0);
    let spot_3 = ground(&world, here, 6 * r, 0);
    let enemy_home = beacon(&mut world, seat(1), spot_3, MandateKind::Build, HOLD_FIRE);
    let spec = BeaconSpec::MostThreatened(Filter::default());
    // No threat anywhere: every sum is zero, and the tie goes to the lowest id.
    let lowest = resolve_beacon_now(&world, seat(0), spec);
    assert_eq!(lowest, Some(core(&world, seat(0))));
    let spot_4 = ground(&world, east, 2, 0);
    unit(&mut world, seat(1), enemy_home, UnitKind::Raider, spot_4);
    assert_eq!(resolve_beacon_now(&world, seat(0), spec), Some(threatened));
    // Two raiders by the quiet beacon outweigh one by the other.
    let spot_5 = ground(&world, here, 2, 0);
    unit(&mut world, seat(1), enemy_home, UnitKind::Raider, spot_5);
    let spot_6 = ground(&world, here, -2, 0);
    unit(&mut world, seat(1), enemy_home, UnitKind::Raider, spot_6);
    assert_eq!(resolve_beacon_now(&world, seat(0), spec), Some(quiet));
    // The frozen snapshot answers as the live world does.
    let snapshot = Snapshot::capture(&world);
    assert_eq!(
        resolve_beacon_in(&snapshot, world.rules(), seat(0), spec, None),
        Some(quiet)
    );
    // And `safest` ranks the threatened beacons behind every quiet one.
    let safest = resolve_beacon_now(&world, seat(0), BeaconSpec::Safest);
    assert_ne!(safest, Some(quiet));
    assert_ne!(safest, Some(threatened));
    assert_eq!(
        resolve_beacon_in(&snapshot, world.rules(), seat(0), BeaconSpec::Safest, None),
        safest
    );
}

#[test]
fn under_attack_reads_its_window() {
    let mut fixture = world();
    let here = field(&fixture);
    let target = beacon(&mut fixture, seat(0), here, MandateKind::Build, HOLD_FIRE);
    assert!(!beacon_under_attack(&fixture, target, 5_000), "never hit");
    let spot = ground(&fixture, here, 60, 0);
    let enemy_home = beacon(&mut fixture, seat(1), spot, MandateKind::Build, HOLD_FIRE);
    let raider = unit(&mut fixture, seat(1), enemy_home, UnitKind::Raider, spot);
    let mut runner = pushing(fixture);
    // A sourceless order is no attack: the combat memory records the last
    // enemy that hit, which is what the predicate reads.
    hit(runner.world_mut(), DamageTarget::Beacon(target), 1, seat(1));
    play(&mut runner, 1);
    assert!(!beacon_under_attack(runner.world(), target, 5_000));
    runner
        .world_mut()
        .request_damage(DamageOrder {
            target: DamageTarget::Beacon(target),
            amount: Damage::new(1).unwrap(),
            by: seat(1),
            source: Some(AssetId::of_unit(raider).unwrap()),
        })
        .unwrap();
    play(&mut runner, 1);
    assert!(beacon_under_attack(runner.world(), target, 5_000));
    assert!(
        beacon_under_attack(runner.world(), target, 0),
        "hit this tick"
    );
    // 2 s of game time later: inside a 5 s window, outside a 1 s one.
    play(&mut runner, 40);
    assert!(beacon_under_attack(runner.world(), target, 5_000));
    assert!(!beacon_under_attack(runner.world(), target, 1_000));
}

#[test]
fn guards_escort_the_commander_inside_the_sphere_only() {
    let mut fixture = world();
    let here = field(&fixture);
    let post = defend(&mut fixture, seat(0), here, holding(Pursue::Never, true));
    let guard = unit(&mut fixture, seat(0), post, UnitKind::Raider, here);
    let commander = fixture.commander_of(seat(0)).expect("a commander");
    let inside = ground(&fixture, here, sphere() >> 1, 0);
    let outside = ground(&fixture, here, sphere() * 3, 0);
    let row = unit_row(&fixture, commander);
    // The commander stands inside the sphere: the guard escorts it.
    assert!(fixture.place_unit_directly(commander, inside));
    let mut runner = pushing(fixture);
    runner.world_mut().send_unit_directly(row, inside);
    play(&mut runner, 400);
    let world = runner.world();
    assert!(
        within(unit_at(world, guard), unit_at(world, commander), 3),
        "the guard escorts the commander inside the sphere"
    );
    // Out of the sphere: the guard goes home and holds it.
    assert!(runner.world_mut().place_unit_directly(commander, outside));
    play(&mut runner, 900);
    let world = runner.world();
    assert!(
        within(unit_at(world, commander), outside, 3),
        "the commander left"
    );
    assert!(
        within(unit_at(world, guard), here, 3),
        "the guard does not leave its sphere to escort"
    );
}

#[test]
fn a_damaged_guard_walks_to_a_repair_drone() {
    let mut fixture = world();
    let here = field(&fixture);
    let post = defend(&mut fixture, seat(0), here, holding(Pursue::Never, false));
    let guard = unit(&mut fixture, seat(0), post, UnitKind::Raider, here);
    let mend_at = ground(&fixture, here, sphere() * 3, 0);
    let mender_home = beacon(&mut fixture, seat(0), mend_at, MandateKind::Build, 0);
    let drone = unit(
        &mut fixture,
        seat(0),
        mender_home,
        UnitKind::RepairDrone,
        mend_at,
    );
    let mut runner = pushing(fixture);
    hit(runner.world_mut(), DamageTarget::Unit(guard), 5, seat(1));
    let mut closest = i64::MAX;
    for _ in 0..1_200 {
        play(&mut runner, 1);
        let world = runner.world();
        closest = closest.min(distance2(unit_at(world, guard), unit_at(world, drone)));
    }
    assert!(
        closest <= Sq::of_radius(Fx::from_voxels(4)).raw(),
        "the damaged guard walked to the repair drone"
    );
}

#[test]
fn pursue_never_stops_at_the_engagement_radius() {
    // An unarmed enemy drone starts inside a half-sphere engagement radius
    // and walks home to a beacon inside the sphere and beyond that radius
    // (a Build beacon holds no seam, so its mining drone idles at home).
    let run = |pursue: Pursue| -> (u32, u32, UnitId) {
        let mut fixture = world();
        let here = field(&fixture);
        let post = defend(
            &mut fixture,
            seat(0),
            here,
            DefendSettings {
                engagement_pct: 50,
                pursue,
                pickets: 0,
                escort: false,
            },
        );
        let away = ground(&fixture, here, (sphere() * 3) >> 2, 0);
        let enemy_home = beacon(&mut fixture, seat(1), away, MandateKind::Build, HOLD_FIRE);
        let spot_7 = ground(&fixture, here, 2, 0);
        let scout = unit(
            &mut fixture,
            seat(1),
            enemy_home,
            UnitKind::MiningDrone,
            spot_7,
        );
        let mut runner = pushing(fixture);
        play(&mut runner, 1);
        let first = quarry(runner.world(), post);
        play(&mut runner, 600);
        assert!(
            within(unit_at(runner.world(), scout), away, 3),
            "the drone went home"
        );
        (first, quarry(runner.world(), post), scout)
    };
    let (first, last, scout) = run(Pursue::Never);
    let scout = AssetId::of_unit(scout).unwrap().raw();
    assert_eq!(
        first, scout,
        "a threat inside the engagement radius is the quarry"
    );
    assert_eq!(last, NO_QUARRY, "NEVER lets it go at the engagement radius");
    let (first, last, scout) = run(Pursue::SphereEdge);
    let scout = AssetId::of_unit(scout).unwrap().raw();
    assert_eq!(first, scout);
    assert_eq!(last, scout, "SPHERE_EDGE follows it inside the sphere");
}

#[test]
fn a_picket_sees_past_the_sphere() {
    let mut fixture = world();
    let here = field(&fixture);
    let post = defend(
        &mut fixture,
        seat(0),
        here,
        DefendSettings {
            engagement_pct: 100,
            pursue: Pursue::Never,
            pickets: 1,
            escort: false,
        },
    );
    let ring = sphere() + i32::from(rules().picket_radius_voxels());
    // An enemy scout parked at its own beacon just past the ring's first point,
    // beyond the Defend beacon's sphere.
    let lurker_at = ground(&fixture, here, ring + 2, 0);
    let lurker_home = beacon(
        &mut fixture,
        seat(1),
        lurker_at,
        MandateKind::Build,
        HOLD_FIRE,
    );
    unit(
        &mut fixture,
        seat(1),
        lurker_home,
        UnitKind::Scout,
        lurker_at,
    );
    let voxel = [
        lurker_at[0].floor_voxels(),
        lurker_at[1].floor_voxels(),
        lurker_at[2].floor_voxels(),
    ];
    assert!(
        !fixture.sight().sees(seat(0), voxel),
        "the sphere does not reach it"
    );
    unit(&mut fixture, seat(0), post, UnitKind::Scout, here);
    let mut runner = pushing(fixture);
    let mut seen = false;
    for _ in 0..800 {
        play(&mut runner, 1);
        seen |= runner.world().sight().sees(seat(0), voxel);
    }
    assert!(seen, "the picket saw past the sphere");
}

#[test]
fn a_wounded_commander_is_walked_to_a_repair_drone() {
    let mut world = world();
    let here = field(&world);
    let mended = beacon(&mut world, seat(0), here, MandateKind::Build, 0);
    unit(&mut world, seat(0), mended, UnitKind::RepairDrone, here);
    // No money, so the core fabricates no repair drone of its own for the
    // wounded commander standing in its sphere.
    world.set_treasury(seat(0), pharmakos_sim::math::quantity::Money::ZERO);
    // Unhurt, `safest` ranks by hit points and threat only: a tie at full
    // health goes to the lowest id, the core.
    assert_eq!(
        resolve_beacon_now(&world, seat(0), BeaconSpec::Safest),
        Some(core(&world, seat(0)))
    );
    let commander = world.commander_of(seat(0)).unwrap();
    let mut runner = pushing(world);
    hit(
        runner.world_mut(),
        DamageTarget::Unit(commander),
        10,
        seat(1),
    );
    play(&mut runner, 1);
    // Wounded, the beacon whose sphere holds a repair drone comes first: the
    // reflex and the safe playbook walk the commander there (decision 6).
    assert_eq!(
        resolve_beacon_now(runner.world(), seat(0), BeaconSpec::Safest),
        Some(mended)
    );
    let snapshot = Snapshot::capture(runner.world());
    assert_eq!(
        resolve_beacon_in(
            &snapshot,
            runner.world().rules(),
            seat(0),
            BeaconSpec::Safest,
            None
        ),
        Some(mended)
    );
}

#[test]
fn an_empty_new_table_hashes_nothing() {
    let mut world = world();
    let before = world.state_hash();
    let here = field(&world);
    let post = world
        .place_beacon_directly(seat(0), here, MandateKind::Build, PRIORITY_NORMAL)
        .unwrap();
    let placed = world.state_hash();
    assert_ne!(placed, before);
    // A Defend beacon holds a row, which hashes; the same beacon switched back
    // holds none, and the world hashes as the Build beacon's did.
    let mut defended = world.clone();
    let row = defended.beacons().row_of(post).unwrap().index();
    defended.switch_mandate_directly(row, MandateKind::Defend);
    assert!(defended.defend().row_of(post).is_some());
    assert_ne!(defended.state_hash(), placed);
    defended.switch_mandate_directly(row, MandateKind::Build);
    assert!(defended.defend().rows().is_empty());
    assert_eq!(
        defended.state_hash(),
        placed,
        "an empty Defend table hashes nothing"
    );
    // And the explored table: one mark hashes, none hashes nothing.
    let mut explored = world.clone();
    assert!(explored.mark_explored_directly(seat(1), 7));
    assert_ne!(explored.state_hash(), placed);
    assert_eq!(world.state_hash(), placed);
}

#[test]
fn the_explored_table_survives_save_and_restore() {
    let mut world = world();
    let here = field(&world);
    assert!(world.mark_explored_directly(seat(1), 9));
    assert!(world.mark_explored_directly(seat(0), 40));
    assert!(
        !world.mark_explored_directly(seat(0), 40),
        "a mark is made once"
    );
    let post = defend(
        &mut world,
        seat(0),
        here,
        holding(Pursue::WhileInReach, true),
    );
    let spot_8 = ground(&world, here, 5, 0);
    let other = beacon(&mut world, seat(0), spot_8, MandateKind::Build, 0);
    assert!(world.set_defend(post, holding(Pursue::WhileInReach, true), &[other]));
    let hash = world.state_hash();
    let bytes = Snapshot::capture(&world).to_bytes().unwrap();
    let mut restored = self::world();
    Snapshot::from_bytes(&bytes)
        .unwrap()
        .restore_into(&mut restored)
        .unwrap();
    assert_eq!(restored.state_hash(), hash);
    assert_eq!(restored.explored().rows(), &[(0, 40), (1, 9)]);
    assert_eq!(restored.defend(), world.defend());
}
