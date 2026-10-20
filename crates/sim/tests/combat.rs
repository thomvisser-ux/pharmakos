// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! S2's `fire`: weapons, shells, materials, wrecks and the kill-credit split
//! (S2's plan, task `fire`; its Acceptance list, test by test, under the names
//! the plan gives them).
//!
//! Every armed structure and unit here is put in the world through a fixture
//! seam ([`World::raise_structure`], [`World::field_unit`],
//! [`World::order_shell`], [`World::request_damage`]): Build's placement of
//! the base kit is `site`'s, and raiders are fielded by Defend and Attack
//! (`dfnd`, `atk`), so no playbook reaches these arrangements yet. The two
//! staged integration tests at the end stand in for the committed scenarios of
//! the same names, which `atk`, the first lane whose playbooks reach them,
//! adds.
//!
//! Each test asserts the **rule** wherever the rule is what was decided: the
//! numbers are rules-table proposals (decision 17), the owner's at S2's demo,
//! so the tests read them from the table rather than pinning them.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules, not an integration test's helpers. A panic is this file's failure report, an out-of-bounds index in an assertion among them: these are panic lints, not determinism lints (AGENTS.md section 5)."
)]

use pharmakos_sim::arms::Threat;
use pharmakos_sim::events::{Event, EventKind};
use pharmakos_sim::knowledge::AssetId;
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::math::quantity::{Damage, Hp};
use pharmakos_sim::runner::Runner;
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::snapshot::Snapshot;
use pharmakos_sim::tables::{
    BeaconId, Common, PRIORITY_NORMAL, SeatId, StructureId, StructureKind, UnitId, UnitKind,
};
use pharmakos_sim::voxels::{BlastLevels, Material, VoxelEdit};
use pharmakos_sim::world::{CombatDrop, DamageOrder, DamageTarget, World, WorldConfig};
use pharmakos_sim::{MatchSettings, RulesTable, default_rules_path};
use std::path::PathBuf;

const SEED: u64 = 0x00F1_2E00_C0DE_0001;

/// `MandateSettings.Roe`'s wire values.
const HOLD_FIRE: u8 = 1;
const RETURN_FIRE: u8 = 2;
const FREE_FIRE: u8 = 3;

fn rules() -> RulesTable {
    let path = default_rules_path().unwrap_or_else(|| PathBuf::from("rules/rules.v1.json"));
    RulesTable::load(&path).unwrap_or_else(|error| panic!("loading the rules table: {error}"))
}

/// The committed table, edited.
fn edited(edit: impl FnOnce(&mut pharmakos_proto::gp::v1::RulesTable)) -> RulesTable {
    let mut message = rules().message().clone();
    edit(&mut message);
    RulesTable::from_message(&message).unwrap_or_else(|error| panic!("the edited table: {error}"))
}

/// The committed table edited, or the refusal.
fn try_edited(
    edit: impl FnOnce(&mut pharmakos_proto::gp::v1::RulesTable),
) -> Result<RulesTable, pharmakos_sim::RulesError> {
    let mut message = rules().message().clone();
    edit(&mut message);
    RulesTable::from_message(&message)
}

fn world_of(rules: RulesTable, seats: u32) -> World {
    World::new(&WorldConfig {
        match_seed: SEED,
        seats,
        units_per_seat: 0,
        rules,
        match_settings: MatchSettings {
            segment_lengths_ms: vec![600_000],
            round_limit: 3,
        },
    })
    .unwrap_or_else(|error| panic!("generating the world: {error}"))
}

fn world(seats: u32) -> World {
    world_of(rules(), seats)
}

fn seat(raw: u8) -> SeatId {
    SeatId::new(raw)
}

/// A seat's core: its lowest beacon id.
fn core(world: &World, of: SeatId) -> BeaconId {
    world
        .beacons()
        .ids()
        .iter()
        .zip(world.beacons().seats())
        .filter(|(_, owner)| **owner == of.raw())
        .map(|(id, _)| *id)
        .min()
        .map_or_else(|| panic!("seat {} has a core", of.raw()), BeaconId::new)
}

fn beacon_at(world: &World, beacon: BeaconId) -> [Fx; 3] {
    let row = world.beacons().row_of(beacon).expect("a live beacon");
    world.beacons().positions()[row.index()]
}

fn column(point: [Fx; 3]) -> [i32; 2] {
    [point[0].floor_voxels(), point[1].floor_voxels()]
}

/// The standing point `dx`, `dy` columns from `from`.
fn ground(world: &World, from: [Fx; 3], dx: i32, dy: i32) -> [Fx; 3] {
    let [x, y] = column(from);
    world.standing_point([x + dx, y + dy, 0])
}

/// A place far from every core: the midpoint of the two first seats' cores,
/// where nothing of either seat stands and neither core's autocannon reaches.
fn field(world: &World) -> [Fx; 3] {
    let a = beacon_at(world, core(world, seat(0)));
    let b = beacon_at(world, core(world, seat(1)));
    let [ax, ay] = column(a);
    let [bx, by] = column(b);
    world.standing_point([i32::midpoint(ax, bx), i32::midpoint(ay, by), 0])
}

/// Place a beacon of `of` at `at`, its Common settings given.
fn beacon(world: &mut World, of: SeatId, at: [Fx; 3], roe: u8) -> BeaconId {
    let id = world
        .place_beacon_directly(of, at, MandateKind::Build, PRIORITY_NORMAL)
        .unwrap_or_else(|| panic!("a beacon for seat {}", of.raw()));
    world.set_common(
        id,
        Common {
            roe,
            retreat_hp_pct: 0,
        },
    );
    id
}

fn raise(
    world: &mut World,
    of: SeatId,
    home: BeaconId,
    kind: StructureKind,
    at: [Fx; 3],
) -> StructureId {
    world
        .raise_structure(of, home, kind, at)
        .unwrap_or_else(|| panic!("a {kind:?} for seat {}", of.raw()))
}

fn unit(world: &mut World, of: SeatId, home: BeaconId, kind: UnitKind, at: [Fx; 3]) -> UnitId {
    world
        .field_unit(of, home, kind, at)
        .unwrap_or_else(|| panic!("a {kind:?} for seat {}", of.raw()))
}

fn unit_hp(world: &World, id: UnitId) -> i32 {
    match world.units().row_of(id) {
        Ok(row) => world.units().hit_points()[row.index()].raw(),
        Err(_) => 0,
    }
}

fn structure_hp(world: &World, id: StructureId) -> i32 {
    match world.structures().row_of(id) {
        Ok(row) => world.structures().hit_points()[row.index()].raw(),
        Err(_) => 0,
    }
}

fn hit(target: DamageTarget, amount: i64, by: SeatId, source: Option<AssetId>) -> DamageOrder {
    DamageOrder {
        target,
        amount: Damage::new(amount).expect("a positive amount"),
        by,
        source,
    }
}

/// A runner in its first Push.
fn pushing(world: World) -> Runner {
    let mut runner = Runner::new(world);
    assert_eq!(runner.begin_push(), Ok(true), "the Push begins");
    runner
}

/// Step `ticks` ticks, keeping every event.
fn play(runner: &mut Runner, ticks: u32, feed: &mut Vec<Event>) {
    for _ in 0..ticks {
        let report = runner
            .step()
            .unwrap_or_else(|error| panic!("the tick runs: {error}"));
        assert!(report.is_some(), "still in the Push");
        feed.extend_from_slice(runner.events());
        runner.clear_events();
    }
}

fn count(feed: &[Event], kind: EventKind) -> usize {
    feed.iter().filter(|event| event.kind == kind).count()
}

fn no_drop(world: &World) {
    assert_eq!(world.event_bus().dropped(), 0, "no event was dropped");
    assert_eq!(
        world.combat_counts().dropped_total(),
        0,
        "combat filed everything it chose"
    );
}

// ---------------------------------------------------------------------------
// Direct fire and the rules of engagement
// ---------------------------------------------------------------------------

#[test]
fn an_autocannon_kills_a_raider_in_range_and_not_out_of_it() {
    let mut world = world(2);
    let here = field(&world);
    let range = rules()
        .structure(StructureKind::Autocannon)
        .and_then(|row| row.weapon)
        .expect("the autocannon is armed")
        .range_voxels;
    let range = i32::from(range);
    let spot_1 = ground(&world, here, -6, 0);
    let home = beacon(&mut world, seat(0), spot_1, FREE_FIRE);
    let gun = raise(&mut world, seat(0), home, StructureKind::Autocannon, here);
    // Seat 1's two raiders stand at their homes: one inside the range, one
    // beyond it, both out of the raider's own short reach.
    let near_at = ground(&world, here, range >> 1, 0);
    let far_at = ground(&world, here, range + 8, 0);
    let near_home = beacon(&mut world, seat(1), near_at, HOLD_FIRE);
    let far_home = beacon(&mut world, seat(1), far_at, HOLD_FIRE);
    let near = unit(&mut world, seat(1), near_home, UnitKind::Raider, near_at);
    let far = unit(&mut world, seat(1), far_home, UnitKind::Raider, far_at);
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    play(&mut runner, 400, &mut feed);
    let world = runner.world();
    assert_eq!(unit_hp(world, near), 0, "the raider in range died");
    assert_eq!(
        unit_hp(world, far),
        world.unit_max_hp(UnitKind::Raider).raw(),
        "the raider out of range was never touched"
    );
    assert!(structure_hp(world, gun) > 0);
    assert!(count(&feed, EventKind::UnitDestroyed) >= 1);
    no_drop(world);
}

#[test]
fn return_fire_waits_for_damage() {
    let mut world = world(2);
    let here = field(&world);
    // The autocannon's home leaves `roe` unset, which reads as RETURN_FIRE.
    let spot_2 = ground(&world, here, -6, 0);
    let home = beacon(&mut world, seat(0), spot_2, 0);
    let gun = raise(&mut world, seat(0), home, StructureKind::Autocannon, here);
    let raider_at = ground(&world, here, 10, 0);
    let raider_home = beacon(&mut world, seat(1), raider_at, HOLD_FIRE);
    let raider = unit(
        &mut world,
        seat(1),
        raider_home,
        UnitKind::Raider,
        raider_at,
    );
    let raider_id = AssetId::of_unit(raider).expect("an id");
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    play(&mut runner, 100, &mut feed);
    let full = runner.world().unit_max_hp(UnitKind::Raider).raw();
    assert_eq!(unit_hp(runner.world(), raider), full, "no damage, no fire");
    assert_eq!(count(&feed, EventKind::ReturnedFire), 0);
    // The raider hits the autocannon (a host's order, carrying the raider as
    // its source): the autocannon answers it, and says so.
    assert!(
        runner
            .world_mut()
            .request_damage(hit(
                DamageTarget::Structure(gun),
                5,
                seat(1),
                Some(raider_id)
            ))
            .is_ok()
    );
    play(&mut runner, 2, &mut feed);
    let arms = {
        let world = runner.world();
        let row = world.structures().row_of(gun).expect("the gun stands");
        world.structures().arms()[row.index()]
    };
    assert_eq!(
        arms.attacker,
        raider_id.raw(),
        "the attacker memory names the raider"
    );
    play(&mut runner, 20, &mut feed);
    assert!(unit_hp(runner.world(), raider) < full, "and is answered");
    assert_eq!(
        count(&feed, EventKind::ReturnedFire),
        1,
        "once, not once per shot"
    );
    no_drop(runner.world());
}

#[test]
fn return_fire_answers_a_shell_by_its_firer() {
    let mut world = world(2);
    let here = field(&world);
    let spot_3 = ground(&world, here, -6, 0);
    let home = beacon(&mut world, seat(0), spot_3, RETURN_FIRE);
    let gun = raise(&mut world, seat(0), home, StructureKind::Autocannon, here);
    // Seat 1's mortar holds its own fire; the fixture shells the autocannon.
    let mortar_at = ground(&world, here, 15, 0);
    let spot_4 = ground(&world, here, 22, 0);
    let mortar_home = beacon(&mut world, seat(1), spot_4, HOLD_FIRE);
    let mortar = raise(
        &mut world,
        seat(1),
        mortar_home,
        StructureKind::Mortar,
        mortar_at,
    );
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    play(&mut runner, 100, &mut feed);
    let full = runner
        .world()
        .structure_max_hp(StructureKind::Mortar)
        .expect("a mortar row")
        .raw();
    assert_eq!(structure_hp(runner.world(), mortar), full);
    assert!(runner.world_mut().order_shell(mortar, here).is_ok());
    play(&mut runner, 200, &mut feed);
    assert_eq!(count(&feed, EventKind::ShellLanded), 1);
    let world = runner.world();
    let row = world.structures().row_of(gun).expect("the gun stands");
    assert_eq!(
        world.structures().arms()[row.index()].attacker,
        AssetId::of_structure(mortar).expect("an id").raw(),
        "a shell's damage remembers its firer"
    );
    assert!(
        structure_hp(world, mortar) < full,
        "and the firer is answered"
    );
    no_drop(world);
}

#[test]
fn hold_fire_never_fires() {
    let mut world = world(2);
    let here = field(&world);
    let spot_5 = ground(&world, here, -6, 0);
    let home = beacon(&mut world, seat(0), spot_5, HOLD_FIRE);
    let gun = raise(&mut world, seat(0), home, StructureKind::Autocannon, here);
    // A raider on FREE_FIRE stands beside it and shoots it.
    let raider_at = ground(&world, here, 2, 0);
    let raider_home = beacon(&mut world, seat(1), raider_at, FREE_FIRE);
    let raider = unit(
        &mut world,
        seat(1),
        raider_home,
        UnitKind::Raider,
        raider_at,
    );
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    play(&mut runner, 200, &mut feed);
    let world = runner.world();
    assert_eq!(
        unit_hp(world, raider),
        world.unit_max_hp(UnitKind::Raider).raw(),
        "HOLD_FIRE never fires, not even back"
    );
    let full = world
        .structure_max_hp(StructureKind::Autocannon)
        .expect("a row")
        .raw();
    assert!(structure_hp(world, gun) < full, "while it is shot at");
    let row = world.structures().row_of(gun).expect("the gun stands");
    assert_eq!(
        world.structures().arms()[row.index()].attacker,
        AssetId::of_unit(raider).expect("an id").raw(),
        "the combat phase's own shot carries its firer as its source"
    );
    no_drop(world);
}

#[test]
fn fire_is_simultaneous() {
    let mut world = world(2);
    let here = field(&world);
    let spot_6 = ground(&world, here, -8, 0);
    let home = beacon(&mut world, seat(0), spot_6, FREE_FIRE);
    let raider_at = ground(&world, here, 2, 0);
    let raider_home = beacon(&mut world, seat(1), raider_at, FREE_FIRE);
    let raider = unit(
        &mut world,
        seat(1),
        raider_home,
        UnitKind::Raider,
        raider_at,
    );
    let raider_cooldown = rules()
        .unit(UnitKind::Raider)
        .weapon
        .expect("the raider is armed")
        .cooldown_ticks();
    let gun_cooldown = rules()
        .structure(StructureKind::Autocannon)
        .and_then(|row| row.weapon)
        .expect("armed")
        .cooldown_ticks();
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    // The autocannon goes up so that its first ready tick is the raider's.
    let wait = raider_cooldown.saturating_sub(gun_cooldown);
    play(&mut runner, wait, &mut feed);
    let gun = raise(
        runner.world_mut(),
        seat(0),
        home,
        StructureKind::Autocannon,
        here,
    );
    // Each is one shot from death.
    let gun_hp = structure_hp(runner.world(), gun);
    let raider_hp = unit_hp(runner.world(), raider);
    assert!(
        runner
            .world_mut()
            .request_damage(hit(
                DamageTarget::Structure(gun),
                i64::from(gun_hp - 1),
                SeatId::NEUTRAL,
                None
            ))
            .is_ok()
    );
    assert!(
        runner
            .world_mut()
            .request_damage(hit(
                DamageTarget::Unit(raider),
                i64::from(raider_hp - 1),
                SeatId::NEUTRAL,
                None
            ))
            .is_ok()
    );
    play(&mut runner, gun_cooldown.saturating_add(2), &mut feed);
    let world = runner.world();
    assert_eq!(unit_hp(world, raider), 0, "the raider died");
    assert_eq!(structure_hp(world, gun), 0, "and so did the autocannon");
    let died = |kind: EventKind| {
        feed.iter()
            .find(|event| event.kind == kind)
            .map(|event| event.tick)
    };
    assert_eq!(
        died(EventKind::UnitDestroyed),
        died(EventKind::StructureDestroyed),
        "on the same tick: no seat's shot lands before another's choice"
    );
    no_drop(world);
}

// ---------------------------------------------------------------------------
// Kill credit, the tally and wrecks
// ---------------------------------------------------------------------------

#[test]
fn a_volley_shares_its_kill_credit() {
    let mut world = world(3);
    let here = field(&world);
    let home = beacon(&mut world, seat(2), here, HOLD_FIRE);
    let raider = unit(&mut world, seat(2), home, UnitKind::Raider, here);
    let hp = i64::from(unit_hp(&world, raider));
    // Two seats fire on one tick, the first filed overkilling on its own.
    assert!(
        world
            .request_damage(hit(DamageTarget::Unit(raider), hp, seat(0), None))
            .is_ok()
    );
    assert!(
        world
            .request_damage(hit(DamageTarget::Unit(raider), hp, seat(1), None))
            .is_ok()
    );
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    play(&mut runner, 1, &mut feed);
    let shares: Vec<(Option<SeatId>, i64)> = feed
        .iter()
        .filter(|event| event.kind == EventKind::KillCredited)
        .map(|event| (event.seat, event.value))
        .collect();
    let cost = runner.world().unit_cost(UnitKind::Raider).raw();
    let half = cost >> 1;
    let rest = cost - half;
    assert_eq!(
        shares,
        vec![(Some(seat(0)), rest), (Some(seat(1)), half)],
        "split in proportion to the orders, ties to the lowest seat, never all to the first filed"
    );
    for (who, share) in [(seat(0), rest), (seat(1), half)] {
        let tally = runner.world().tally(who).expect("a seat");
        assert_eq!(tally.kills, 1);
        assert_eq!(tally.shared_kills, 1, "a split kill is a shared kill");
        assert_eq!(tally.destroyed.raw(), share);
    }
    assert_eq!(runner.world().tally(seat(2)).expect("a seat").losses, 1);
}

#[test]
fn destroyed_value_is_the_settled_share_of_build_cost() {
    let mut world = world(2);
    let here = field(&world);
    let home = beacon(&mut world, seat(1), here, HOLD_FIRE);
    let spot_7 = ground(&world, here, 2, 0);
    let mortar = raise(&mut world, seat(1), home, StructureKind::Mortar, spot_7);
    let hp = i64::from(structure_hp(&world, mortar));
    let cost = world.structure_cost(StructureKind::Mortar).raw();
    // Seat 0 deals three quarters, seat 1's own quarter credits nobody.
    let quarter = hp >> 2;
    assert!(
        world
            .request_damage(hit(DamageTarget::Structure(mortar), quarter, seat(1), None))
            .is_ok()
    );
    assert!(
        world
            .request_damage(hit(
                DamageTarget::Structure(mortar),
                hp - quarter,
                seat(0),
                None
            ))
            .is_ok()
    );
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    play(&mut runner, 1, &mut feed);
    let world = runner.world();
    let destroyed = pharmakos_sim::audit::destroyed_value(world, seat(0));
    assert_eq!(
        destroyed.raw(),
        cost,
        "the only enemy dealer settles the whole cost"
    );
    assert_eq!(
        pharmakos_sim::audit::line_of(world, seat(0)).destroyed,
        destroyed,
        "the audit's term reads the tally"
    );
    assert_eq!(
        pharmakos_sim::audit::destroyed_value(world, seat(1)).raw(),
        0
    );
    assert_eq!(world.tally(seat(0)).expect("a seat").shared_kills, 0);
}

#[test]
fn a_death_leaves_a_wreck_worth_the_salvage_percent() {
    let mut world = world(2);
    let here = field(&world);
    let home = beacon(&mut world, seat(1), here, HOLD_FIRE);
    let raider = unit(&mut world, seat(1), home, UnitKind::Raider, here);
    assert!(
        world
            .request_damage(hit(DamageTarget::Unit(raider), 1_000, seat(0), None))
            .is_ok()
    );
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    play(&mut runner, 2, &mut feed);
    let world = runner.world();
    let percent = rules().combat().salvage_percent;
    let expected = pharmakos_sim::economy::percent_of(world.unit_cost(UnitKind::Raider), percent);
    let wrecks: Vec<i64> = world
        .wrecks()
        .salvages()
        .iter()
        .zip(world.wrecks().frees())
        .filter(|(_, free)| !**free)
        .map(|(value, _)| value.raw())
        .collect();
    assert_eq!(
        wrecks,
        vec![expected.raw()],
        "one wreck, at the salvage percent of the build cost"
    );
    assert_eq!(count(&feed, EventKind::WreckLeft), 1);
    assert_eq!(count(&feed, EventKind::UnitDestroyed), 1);
    assert_eq!(world.tally(seat(1)).expect("a seat").losses, 1);
    assert_eq!(world.tally(seat(0)).expect("a seat").kills, 1);
}

#[test]
fn a_full_wreck_table_buries_the_oldest_wreck() {
    let mut world = world(2);
    let here = field(&world);
    let home = beacon(&mut world, seat(1), here, HOLD_FIRE);
    let limit = world.wreck_limit();
    assert!(limit > 0);
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    // Kill raiders one a tick until the table has turned over once.
    let mut killed: u32 = 0;
    while killed <= limit {
        let Some(raider) = runner
            .world_mut()
            .field_unit(seat(1), home, UnitKind::Raider, here)
        else {
            play(&mut runner, 1, &mut feed);
            continue;
        };
        assert!(
            runner
                .world_mut()
                .request_damage(hit(DamageTarget::Unit(raider), 1_000, seat(0), None))
                .is_ok()
        );
        play(&mut runner, 1, &mut feed);
        killed = killed.saturating_add(1);
    }
    assert!(
        count(&feed, EventKind::WreckBuried) >= 1,
        "the ash buried the oldest"
    );
    assert!(
        runner
            .world()
            .wrecks()
            .frees()
            .iter()
            .filter(|free| !**free)
            .count()
            <= usize::try_from(limit).unwrap()
    );
    assert_eq!(
        runner
            .world()
            .combat_counts()
            .dropped(CombatDrop::NoWreckRoom),
        0
    );
}

#[test]
fn kill_credit_settles_at_a_realistic_death_rate() {
    // G3' section 9.12: a hundred raiders of seat 1 die over a few ticks to two
    // seats' fire, and every death's credit settles in the tick it died.
    let mut world = world(3);
    let here = field(&world);
    let home = beacon(&mut world, seat(1), here, HOLD_FIRE);
    let mut raiders: Vec<UnitId> = Vec::new();
    while let Some(raider) = world.field_unit(seat(1), home, UnitKind::Raider, here) {
        raiders.push(raider);
        if raiders.len() == 90 {
            break;
        }
    }
    assert!(raiders.len() >= 50, "a realistic crowd: {}", raiders.len());
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    for chunk in raiders.chunks(30) {
        for raider in chunk {
            for (by, amount) in [(seat(0), 70), (seat(2), 70)] {
                assert!(
                    runner
                        .world_mut()
                        .request_damage(hit(DamageTarget::Unit(*raider), amount, by, None))
                        .is_ok()
                );
            }
        }
        play(&mut runner, 1, &mut feed);
    }
    let dead = u32::try_from(raiders.len()).expect("a count");
    let world = runner.world();
    assert_eq!(world.tally(seat(1)).expect("a seat").losses, dead);
    for by in [seat(0), seat(2)] {
        let tally = world.tally(by).expect("a seat");
        assert_eq!(tally.kills, dead);
        assert_eq!(tally.shared_kills, dead);
    }
    let cost = world.unit_cost(UnitKind::Raider).raw();
    let total = world.tally(seat(0)).expect("a seat").destroyed.raw()
        + world.tally(seat(2)).expect("a seat").destroyed.raw();
    assert_eq!(
        total,
        cost * i64::from(dead),
        "every death's cost settled in full"
    );
    assert!(
        world.credit().is_empty(),
        "nothing is owed once every death settled"
    );
    no_drop(world);
}

#[test]
fn a_damage_order_carries_its_source() {
    let mut world = world(2);
    let here = field(&world);
    let home = beacon(&mut world, seat(0), here, HOLD_FIRE);
    let victim = unit(&mut world, seat(0), home, UnitKind::Raider, here);
    let spot_8 = ground(&world, here, 30, 0);
    let enemy_home = beacon(&mut world, seat(1), spot_8, HOLD_FIRE);
    let spot_9 = ground(&world, here, 30, 0);
    let shooter = unit(&mut world, seat(1), enemy_home, UnitKind::Raider, spot_9);
    let source = AssetId::of_unit(shooter).expect("an id");
    assert!(
        world
            .request_damage(hit(DamageTarget::Unit(victim), 5, seat(1), Some(source)))
            .is_ok()
    );
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    play(&mut runner, 1, &mut feed);
    let world = runner.world();
    let row = world.units().row_of(victim).expect("alive");
    let arms = world.units().arms()[row.index()];
    assert_eq!(arms.attacker, source.raw());
    assert_eq!(arms.attacked_at, world.tick().raw());
    // An own or seat-less order remembers nobody.
    let enemy_row = world.units().row_of(shooter).expect("alive");
    assert_eq!(
        world.units().arms()[enemy_row.index()].attacker,
        pharmakos_sim::tables::NO_ATTACKER
    );
}

#[test]
fn a_hurt_unit_retreats_and_says_so() {
    let mut world = world(2);
    let here = field(&world);
    let home = beacon(&mut world, seat(0), here, HOLD_FIRE);
    world.set_common(
        home,
        Common {
            roe: HOLD_FIRE,
            retreat_hp_pct: 50,
        },
    );
    let spot_10 = ground(&world, here, 6, 0);
    let raider = unit(&mut world, seat(0), home, UnitKind::Raider, spot_10);
    let hp = i64::from(unit_hp(&world, raider));
    assert!(
        world
            .request_damage(hit(
                DamageTarget::Unit(raider),
                (hp * 2).checked_div(3).unwrap(),
                seat(1),
                None
            ))
            .is_ok()
    );
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    play(&mut runner, 1, &mut feed);
    assert_eq!(count(&feed, EventKind::RetreatAnnounced), 1);
    let row = runner.world().units().row_of(raider).expect("alive");
    assert_eq!(
        runner.world().units().retreats()[row.index()],
        home,
        "home: no repair drone stands"
    );
    play(&mut runner, 20, &mut feed);
    assert_eq!(
        count(&feed, EventKind::RetreatAnnounced),
        1,
        "announced once"
    );
}

// ---------------------------------------------------------------------------
// Shells and splash
// ---------------------------------------------------------------------------

#[test]
fn a_shell_misses_a_unit_that_moved() {
    let mut world = world(2);
    let here = field(&world);
    let spot_11 = ground(&world, here, -6, 0);
    let home = beacon(&mut world, seat(0), spot_11, HOLD_FIRE);
    let mortar = raise(&mut world, seat(0), home, StructureKind::Mortar, here);
    // Seat 1's raider stands 20 voxels out and walks home, further out still.
    let raider_at = ground(&world, here, 20, 0);
    let spot_12 = ground(&world, here, 70, 0);
    let raider_home = beacon(&mut world, seat(1), spot_12, HOLD_FIRE);
    let raider = unit(
        &mut world,
        seat(1),
        raider_home,
        UnitKind::Raider,
        raider_at,
    );
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    let cooldown = rules()
        .structure(StructureKind::Mortar)
        .and_then(|row| row.weapon)
        .expect("armed")
        .cooldown_ticks();
    play(&mut runner, cooldown, &mut feed);
    let aim = {
        let world = runner.world();
        let row = world.units().row_of(raider).expect("alive");
        world.units().positions()[row.index()]
    };
    assert!(runner.world_mut().order_shell(mortar, aim).is_ok());
    play(&mut runner, 200, &mut feed);
    assert_eq!(count(&feed, EventKind::ShellLanded), 1);
    assert_eq!(
        unit_hp(runner.world(), raider),
        runner.world().unit_max_hp(UnitKind::Raider).raw(),
        "the shell landed where the raider stood, and it had walked on"
    );
}

#[test]
fn splash_hurts_the_firers_own_unit() {
    let mut world = world(2);
    let here = field(&world);
    let spot_13 = ground(&world, here, -6, 0);
    let home = beacon(&mut world, seat(0), spot_13, HOLD_FIRE);
    let mortar = raise(&mut world, seat(0), home, StructureKind::Mortar, here);
    let own_at = ground(&world, here, 20, 0);
    let own_home = beacon(&mut world, seat(0), own_at, HOLD_FIRE);
    let own = unit(&mut world, seat(0), own_home, UnitKind::Raider, own_at);
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    play(&mut runner, 100, &mut feed);
    assert!(runner.world_mut().order_shell(mortar, own_at).is_ok());
    play(&mut runner, 200, &mut feed);
    assert!(
        unit_hp(runner.world(), own) < runner.world().unit_max_hp(UnitKind::Raider).raw(),
        "friendly fire: the shell hurts what stands under it, whoever's"
    );
}

#[test]
fn a_mortar_never_aims_where_its_splash_holds_an_own_asset() {
    let mut world = world(2);
    let here = field(&world);
    let spot_14 = ground(&world, here, -6, 0);
    let home = beacon(&mut world, seat(0), spot_14, FREE_FIRE);
    let _mortar = raise(&mut world, seat(0), home, StructureKind::Mortar, here);
    // A clump of seat 1's beside a unit of seat 0's, and one raider alone.
    let clump_at = ground(&world, here, 25, 0);
    let own_at = ground(&world, here, 25, 2);
    let lone_at = ground(&world, here, 25, 15);
    let own_home = beacon(&mut world, seat(0), own_at, HOLD_FIRE);
    let _own = unit(&mut world, seat(0), own_home, UnitKind::Raider, own_at);
    let clump_home = beacon(&mut world, seat(1), clump_at, HOLD_FIRE);
    for _ in 0..3 {
        let _ = unit(&mut world, seat(1), clump_home, UnitKind::Raider, clump_at);
    }
    let lone_home = beacon(&mut world, seat(1), lone_at, HOLD_FIRE);
    let lone = unit(&mut world, seat(1), lone_home, UnitKind::Raider, lone_at);
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    for _ in 0..200 {
        play(&mut runner, 1, &mut feed);
        if !runner.world().shells().is_empty() {
            break;
        }
    }
    let world = runner.world();
    let aims = world.shells().aims();
    assert_eq!(aims.len(), 1, "the mortar fired");
    let row = world.units().row_of(lone).expect("alive");
    assert_eq!(
        aims[0],
        world.units().positions()[row.index()],
        "the lone raider, not the bigger clump over its own unit"
    );
}

#[test]
fn the_shell_table_survives_save_and_restore() {
    let mut world = world(2);
    let here = field(&world);
    let spot_15 = ground(&world, here, -6, 0);
    let home = beacon(&mut world, seat(0), spot_15, HOLD_FIRE);
    let mortar = raise(&mut world, seat(0), home, StructureKind::Mortar, here);
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    play(&mut runner, 100, &mut feed);
    let spot_16 = ground(runner.world(), here, 30, 0);
    assert!(runner.world_mut().order_shell(mortar, spot_16).is_ok());
    play(&mut runner, 3, &mut feed);
    assert_eq!(runner.world().shells().len(), 1, "in flight");
    let saved = Snapshot::capture(runner.world());
    let bytes = saved.to_bytes().expect("encodes");
    let mut restored = world_of(rules(), 2);
    Snapshot::from_bytes(&bytes)
        .expect("decodes")
        .restore_into(&mut restored)
        .expect("restores");
    assert_eq!(restored.shells(), runner.world().shells());
    assert_eq!(restored.state_hash(), runner.world().state_hash());
    let mut enc = pharmakos_sim::Enc::with_capacity(64 * 1024);
    for _ in 0..200 {
        let a = runner.step().expect("runs").map(|report| report.hash);
        let b = restored.step(&mut enc).expect("runs");
        assert_eq!(
            a,
            Some(b),
            "the resumed run lands the shell as the unbroken one does"
        );
    }
}

#[test]
fn the_shell_table_cannot_fill() {
    // The room is derived from firings (E13): at most ceil(F / C) + 1 shells a
    // row can have in flight, every structure row a mortar.
    let rules = rules();
    let mortar = rules
        .structure(StructureKind::Mortar)
        .and_then(|row| row.weapon)
        .expect("armed");
    let flight = mortar.longest_flight_ticks().expect("a shell");
    let per_row = flight.div_ceil(mortar.cooldown_ticks()) + 1;
    let world = world_of(rules, 2);
    assert_eq!(
        world.shells().room(),
        per_row * (240),
        "every structure row's worst case, and only the mortar is indirect"
    );
    // A full table refuses with a typed error, never a silent drop.
    let mut table = pharmakos_sim::tables::ShellTable::with_room(1);
    let aim = [Fx::ZERO; 3];
    let weapon = pharmakos_sim::tables::Armament::Structure(StructureKind::Mortar);
    assert!(table.push(aim, 5, seat(0), 0, weapon).is_ok());
    assert_eq!(
        table.push(aim, 5, seat(0), 0, weapon),
        Err(pharmakos_sim::tables::ShellsFull { room: 1 })
    );
}

#[test]
fn the_attacker_memory_survives_save_and_restore() {
    let mut world = world(2);
    let here = field(&world);
    let home = beacon(&mut world, seat(0), here, HOLD_FIRE);
    world.set_common(
        home,
        Common {
            roe: RETURN_FIRE,
            retreat_hp_pct: 90,
        },
    );
    let spot = ground(&world, here, 6, 0);
    let victim = unit(&mut world, seat(0), home, UnitKind::Raider, spot);
    let source = AssetId::of_beacon(core(&world, seat(1))).expect("an id");
    assert!(
        world
            .request_damage(hit(DamageTarget::Unit(victim), 30, seat(1), Some(source)))
            .is_ok()
    );
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    play(&mut runner, 2, &mut feed);
    assert!(
        runner.world().units().retreats().contains(&home),
        "a retreat to save"
    );
    assert!(
        runner
            .world()
            .beacons()
            .commons()
            .iter()
            .any(|common| common.retreat_hp_pct == 90),
        "and a Common setting"
    );
    let saved = Snapshot::capture(runner.world());
    let mut restored = world_of(rules(), 2);
    saved.restore_into(&mut restored).expect("restores");
    assert_eq!(restored.units().arms(), runner.world().units().arms());
    assert_eq!(
        restored.units().retreats(),
        runner.world().units().retreats()
    );
    assert_eq!(
        restored.beacons().commons(),
        runner.world().beacons().commons()
    );
    assert_eq!(restored.seats().tallies(), runner.world().seats().tallies());
    assert_eq!(restored.seats().attacks(), runner.world().seats().attacks());
    assert_eq!(restored.state_hash(), runner.world().state_hash());
}

#[test]
fn a_raider_on_the_default_rules_still_raids() {
    // Decision 21: a raider's job is the nearest enemy structure or beacon,
    // and an unset `roe` (RETURN_FIRE, decision 4) says what it does about
    // units, not whether it raids.
    let mut world = world(2);
    let here = field(&world);
    let spot = ground(&world, here, -8, 0);
    let home = beacon(&mut world, seat(0), spot, HOLD_FIRE);
    let target = raise(&mut world, seat(0), home, StructureKind::Generator, here);
    let raider_at = ground(&world, here, 2, 0);
    let raider_home = beacon(&mut world, seat(1), raider_at, 0);
    let _raider = unit(
        &mut world,
        seat(1),
        raider_home,
        UnitKind::Raider,
        raider_at,
    );
    let full = structure_hp(&world, target);
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    play(&mut runner, 100, &mut feed);
    assert!(
        structure_hp(runner.world(), target) < full,
        "the raider raided"
    );
}

#[test]
fn return_fire_answers_the_killer_of_an_asset() {
    // Decision 4: "damaged one of the seat's own assets", a dead one among
    // them. The victim dies outright; the autocannon still answers its killer.
    let mut world = world(2);
    let here = field(&world);
    let spot = ground(&world, here, -6, 0);
    let home = beacon(&mut world, seat(0), spot, RETURN_FIRE);
    let _gun = raise(&mut world, seat(0), home, StructureKind::Autocannon, here);
    let victim_at = ground(&world, here, 3, 3);
    let victim = unit(&mut world, seat(0), home, UnitKind::BuildDrone, victim_at);
    let raider_at = ground(&world, here, 10, 0);
    let raider_home = beacon(&mut world, seat(1), raider_at, HOLD_FIRE);
    let raider = unit(
        &mut world,
        seat(1),
        raider_home,
        UnitKind::Raider,
        raider_at,
    );
    let source = AssetId::of_unit(raider).expect("an id");
    assert!(
        world
            .request_damage(hit(
                DamageTarget::Unit(victim),
                10_000,
                seat(1),
                Some(source)
            ))
            .is_ok()
    );
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    play(&mut runner, 30, &mut feed);
    assert_eq!(
        unit_hp(runner.world(), victim),
        0,
        "the victim died at once"
    );
    assert!(
        unit_hp(runner.world(), raider) < runner.world().unit_max_hp(UnitKind::Raider).raw(),
        "and its killer was answered"
    );
}

#[test]
fn a_ruin_and_a_disbanded_unit_credit_nobody() {
    // Item 17: an asset already a ruin, or disbanded on elimination, credits
    // nobody, whoever had damaged it.
    let mut world = world(2);
    let here = field(&world);
    let home = beacon(&mut world, seat(1), here, HOLD_FIRE);
    let spot = ground(&world, here, 2, 0);
    let structure = raise(&mut world, seat(1), home, StructureKind::Generator, spot);
    let drone_spot = ground(&world, here, 4, 0);
    let drone = unit(&mut world, seat(1), home, UnitKind::BuildDrone, drone_spot);
    for target in [
        DamageTarget::Structure(structure),
        DamageTarget::Unit(drone),
    ] {
        assert!(world.request_damage(hit(target, 10, seat(0), None)).is_ok());
    }
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    play(&mut runner, 1, &mut feed);
    // Seat 1's forward beacon dies: the structure is ruined.
    assert!(
        runner
            .world_mut()
            .request_damage(hit(DamageTarget::Beacon(home), 100_000, seat(0), None))
            .is_ok()
    );
    play(&mut runner, 1, &mut feed);
    assert_eq!(
        runner.world().tally(seat(1)).expect("a seat").losses,
        2,
        "the beacon and its ruin"
    );
    // The ruin is destroyed; then seat 1's core falls and it is eliminated.
    let core1 = core(runner.world(), seat(1));
    for order in [
        hit(DamageTarget::Structure(structure), 100_000, seat(0), None),
        hit(DamageTarget::Beacon(core1), 100_000, seat(0), None),
    ] {
        assert!(runner.world_mut().request_damage(order).is_ok());
        let report = runner.step().expect("runs");
        feed.extend_from_slice(runner.events());
        runner.clear_events();
        if report.is_none() {
            break;
        }
    }
    let structure_id = AssetId::of_structure(structure).expect("an id");
    let drone_id = AssetId::of_unit(drone).expect("an id");
    for event in &feed {
        if event.kind == EventKind::KillCredited {
            assert_ne!(event.subject, Some(structure_id), "a ruin credits nobody");
            assert_ne!(
                event.subject,
                Some(drone_id),
                "a disbanded unit credits nobody"
            );
        }
    }
    assert_eq!(
        runner.world().tally(seat(0)).expect("a seat").kills,
        2,
        "the two beacons only"
    );
}

#[test]
fn a_retreat_home_with_nothing_to_repair_ends_on_arrival() {
    let mut world = world(2);
    let here = field(&world);
    let home = beacon(&mut world, seat(0), here, HOLD_FIRE);
    world.set_common(
        home,
        Common {
            roe: HOLD_FIRE,
            retreat_hp_pct: 50,
        },
    );
    let spot = ground(&world, here, 8, 0);
    let raider = unit(&mut world, seat(0), home, UnitKind::Raider, spot);
    assert!(
        world
            .request_damage(hit(DamageTarget::Unit(raider), 100, seat(1), None))
            .is_ok()
    );
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    play(&mut runner, 400, &mut feed);
    assert_eq!(count(&feed, EventKind::RetreatAnnounced), 1);
    let row = runner.world().units().row_of(raider).expect("alive");
    assert!(
        !runner.world().units().retreats()[row.index()].is_some(),
        "home, nothing repairs there: the retreat was the walk"
    );
}

#[test]
fn the_shell_table_holds_its_worst_case() {
    // E13: every structure row a mortar, each firing near its full range on
    // every cooldown, and the table never refuses a shell. The mortars draw
    // nothing here, so two hundred of them do not brown the core out.
    let quiet = edited(|message| {
        if let Some(mortar) = message.structures.as_mut().and_then(|s| s.mortar.as_mut()) {
            mortar.draw_kw = 0;
        }
    });
    let mut world = world_of(quiet, 2);
    let here = beacon_at(&world, core(&world, seat(0)));
    let home = core(&world, seat(0));
    let mut mortars = Vec::new();
    while let Some(mortar) = world.raise_structure(seat(0), home, StructureKind::Mortar, here) {
        mortars.push(mortar);
    }
    assert!(mortars.len() >= 200, "{} mortars", mortars.len());
    let range = rules()
        .structure(StructureKind::Mortar)
        .and_then(|row| row.weapon)
        .expect("armed")
        .range_voxels;
    let aim = ground(&world, here, i32::from(range) - 6, 0);
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    let mut most = 0;
    for _ in 0..400 {
        for mortar in &mortars {
            let _ = runner.world_mut().order_shell(*mortar, aim);
        }
        most = most.max(runner.world().shells().len());
        play(&mut runner, 1, &mut feed);
        feed.clear();
    }
    assert!(
        most > mortars.len(),
        "shells from several firings were in flight at once: {most}"
    );
    assert!(most <= usize::try_from(runner.world().shells().room()).unwrap());
    assert_eq!(
        runner
            .world()
            .combat_counts()
            .dropped(CombatDrop::ShellsFull),
        0
    );
}

// ---------------------------------------------------------------------------
// Materials, bedrock and the bore
// ---------------------------------------------------------------------------

/// Blast the voxel `at` once, between ticks, and run the voxel phase.
fn blast_once(world: &mut World, at: [i32; 3], radius: i32) {
    assert!(
        world
            .request_voxel_edit(VoxelEdit::Crater {
                centre: at,
                radius,
                by: None,
            })
            .is_ok()
    );
    let mut enc = pharmakos_sim::Enc::with_capacity(64 * 1024);
    assert!(world.step(&mut enc).is_ok());
}

/// The top solid voxel of the column under `point`.
fn top(world: &World, point: [Fx; 3]) -> [i32; 3] {
    let [x, y] = column(point);
    [x, y, world.voxels().top_solid_z(x, y).expect("a floor")]
}

#[test]
fn a_wall_voxel_breaks_on_its_last_level() {
    let mut world = world(2);
    let here = field(&world);
    let at = {
        let [x, y, z] = top(&world, here);
        [x, y, z + 1]
    };
    let wall = Material::wall(seat(0), false).expect("a wall id");
    assert!(
        world
            .request_voxel_edit(VoxelEdit::Set {
                at,
                material: wall,
                by: None
            })
            .is_ok()
    );
    let mut enc = pharmakos_sim::Enc::with_capacity(64 * 1024);
    assert!(world.step(&mut enc).is_ok());
    assert_eq!(world.voxels().get(at), Some(wall));
    blast_once(&mut world, at, 0);
    assert_eq!(
        world.voxels().get(at),
        Material::wall(seat(0), true),
        "it cracks first"
    );
    blast_once(&mut world, at, 0);
    assert_eq!(
        world.voxels().get(at),
        Some(Material::AIR),
        "and breaks on its second blast"
    );
}

#[test]
fn stone_takes_two_blasts() {
    let mut world = world(2);
    let here = field(&world);
    let [x, y, z] = top(&world, here);
    let at = [x, y, z - 4];
    assert_eq!(
        world.voxels().get(at),
        Some(Material::STONE),
        "below the dirt skin"
    );
    blast_once(&mut world, at, 0);
    assert_eq!(world.voxels().get(at), Some(Material::STONE_CRACKED_ONCE));
    blast_once(&mut world, at, 0);
    assert_eq!(world.voxels().get(at), Some(Material::AIR));
}

#[test]
fn dirt_goes_in_one_blast() {
    let mut world = world(2);
    let here = field(&world);
    let at = top(&world, here);
    assert_eq!(world.voxels().get(at), Some(Material::DIRT));
    blast_once(&mut world, at, 0);
    assert_eq!(world.voxels().get(at), Some(Material::AIR));
}

#[test]
fn a_cracked_seam_mines_and_a_cracked_vent_feeds_as_itself() {
    let rules = edited(|message| {
        let levels = message
            .materials
            .as_mut()
            .and_then(|materials| materials.blast_levels.as_mut())
            .expect("a blast-levels block");
        levels.ore = 2;
        levels.vent = 2;
    });
    let mut world = world_of(rules, 2);
    let features: Vec<_> = world.features().features().to_vec();
    let seam = features
        .iter()
        .find(|feature| feature.kind == pharmakos_sim::features::FeatureKind::Seam)
        .expect("a seam");
    let column = seam.anchor_column().expect("an anchor column");
    let ore_at = [column.x, column.y, column.top];
    let grade = world
        .voxels()
        .get(ore_at)
        .and_then(Material::ore_richness)
        .expect("ore");
    let yield_whole = world.ore_yield_at(ore_at).expect("a yield");
    blast_once(&mut world, ore_at, 0);
    let cracked = world.voxels().get(ore_at).expect("on the map");
    assert_ne!(cracked, grade.ore(), "it cracked");
    assert_eq!(cracked.ore_richness(), Some(grade), "and keeps its grade");
    assert_eq!(
        world.ore_yield_at(ore_at),
        Some(yield_whole),
        "and mines as itself"
    );

    let index = features
        .iter()
        .position(|feature| feature.kind == pharmakos_sim::features::FeatureKind::Vent)
        .expect("a vent");
    let vent = features.get(index).expect("the vent");
    for column in &vent.footprint {
        blast_once(&mut world, [column.x, column.y, column.top], 0);
    }
    assert!(world.feature_is_live(index), "a cracked vent still feeds");
    let anchor = vent.anchor_column().expect("an anchor");
    assert_eq!(
        world
            .voxels()
            .get([anchor.x, anchor.y, anchor.top])
            .and_then(Material::vent_richness),
        Some(vent.grade)
    );
}

#[test]
fn a_crater_that_takes_a_vent_kills_its_feature() {
    let mut world = world(2);
    let features: Vec<_> = world.features().features().to_vec();
    let index = features
        .iter()
        .position(|feature| feature.kind == pharmakos_sim::features::FeatureKind::Vent)
        .expect("a vent");
    assert!(world.feature_is_live(index));
    let vent = features.get(index).expect("the vent");
    let anchor = vent.anchor_column().expect("an anchor");
    blast_once(&mut world, [anchor.x, anchor.y, anchor.top], 3);
    assert!(
        !world.feature_is_live(index),
        "its last exposed voxels are gone"
    );
}

#[test]
fn a_blast_level_outside_the_catalogue_is_refused_at_load() {
    for level in [0, u32::from(BlastLevels::MAX) + 1] {
        let refused = try_edited(|message| {
            let levels = message
                .materials
                .as_mut()
                .and_then(|materials| materials.blast_levels.as_mut())
                .expect("a blast-levels block");
            levels.stone = level;
        });
        assert!(
            matches!(&refused, Err(pharmakos_sim::RulesError::OutOfRange { field, .. }) if field == "materials.blast_levels.stone"),
            "{level}: {refused:?}"
        );
    }
}

#[test]
fn the_combat_rows_a_tick_could_not_use_are_refused_at_load() {
    let refused = |edit: fn(&mut pharmakos_proto::gp::v1::RulesTable), field: &str| {
        let answer = try_edited(edit);
        assert!(
            matches!(&answer, Err(pharmakos_sim::RulesError::OutOfRange { field: f, .. }) if f == field),
            "{field}: {answer:?}"
        );
    };
    refused(
        |m| {
            if let Some(w) = m
                .structures
                .as_mut()
                .and_then(|s| s.autocannon.as_mut())
                .and_then(|a| a.weapon.as_mut())
            {
                w.cooldown_ms = 0;
            }
        },
        "structures.autocannon.cooldown_ms",
    );
    refused(
        |m| {
            if let Some(w) = m
                .structures
                .as_mut()
                .and_then(|s| s.autocannon.as_mut())
                .and_then(|a| a.weapon.as_mut())
            {
                w.range_voxels = 0;
            }
        },
        "structures.autocannon.range_voxels",
    );
    refused(
        |m| {
            if let Some(p) = m
                .structures
                .as_mut()
                .and_then(|s| s.mortar.as_mut())
                .and_then(|a| a.weapon.as_mut())
                .and_then(|w| w.projectile.as_mut())
            {
                p.speed_voxels_per_second = 0;
            }
        },
        "structures.mortar.projectile.speed_voxels_per_second",
    );
    refused(
        |m| {
            if let Some(materials) = m.materials.as_mut() {
                materials.bedrock_layers_voxels = 0;
            }
        },
        "materials.bedrock_layers_voxels",
    );
    refused(
        |m| {
            if let Some(w) = m.beacon.as_mut().and_then(|b| b.core_weapon.as_mut()) {
                w.damage_hp = 0;
            }
        },
        "beacon.core_weapon.damage_hp",
    );
    refused(
        |m| {
            if let Some(combat) = m.combat.as_mut() {
                combat.under_attack_window_ms = 0;
            }
        },
        "combat.under_attack_window_ms",
    );
    refused(
        |m| {
            if let Some(economy) = m.economy.as_mut() {
                economy.salvage_percent = 101;
            }
        },
        "economy.salvage_percent",
    );
}

#[test]
fn generation_writes_bedrock_in_the_bottom_layers() {
    let world = world(3);
    let layers = i32::from(rules().bedrock_layers_voxels());
    let [width, depth, _] = world.voxels().size();
    let mut y: i32 = 0;
    while y < i32::try_from(depth).expect("a size") {
        let mut x: i32 = 0;
        while x < i32::try_from(width).expect("a size") {
            for z in 0..layers {
                assert_eq!(
                    world.voxels().get([x, y, z]),
                    Some(Material::BEDROCK),
                    "column ({x}, {y}) at z = {z}"
                );
            }
            assert_ne!(world.voxels().get([x, y, layers]), Some(Material::BEDROCK));
            x += 7;
        }
        y += 7;
    }
}

#[test]
fn bedrock_and_the_bore_survive_any_blast() {
    let mut world = world(2);
    let column = *world.bore().columns().first().expect("a bore");
    let before: Vec<Option<Material>> = (0..=column.top)
        .map(|z| world.voxels().get([column.x, column.y, z]))
        .collect();
    for z in [0, column.top >> 1, column.top] {
        for _ in 0..3 {
            blast_once(&mut world, [column.x, column.y, z], 4);
        }
    }
    let after: Vec<Option<Material>> = (0..=column.top)
        .map(|z| world.voxels().get([column.x, column.y, z]))
        .collect();
    assert_eq!(before, after, "the bore keeps its generated material");
    for dx in -3..=3 {
        assert_eq!(
            world.voxels().get([column.x + dx, column.y, 0]),
            Some(Material::BEDROCK),
            "bedrock never cracks"
        );
    }
}

#[test]
fn a_wall_turns_neutral_when_its_last_sphere_dies() {
    let mut world = world(2);
    let here = field(&world);
    let home = beacon(&mut world, seat(0), here, HOLD_FIRE);
    let at = {
        let [x, y, z] = top(&world, ground(&world, here, 3, 0));
        [x, y, z + 1]
    };
    assert!(
        world
            .request_voxel_edit(VoxelEdit::Set {
                at,
                material: Material::wall(seat(0), false).expect("a wall id"),
                by: None,
            })
            .is_ok()
    );
    assert!(
        world
            .request_damage(hit(DamageTarget::Beacon(home), 1, seat(1), None))
            .is_ok()
    );
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    play(&mut runner, 1, &mut feed);
    assert_eq!(
        runner.world().wall_owner(at),
        Some(seat(0)),
        "inside its seat's live sphere"
    );
    assert!(
        runner
            .world_mut()
            .request_damage(hit(DamageTarget::Beacon(home), 100_000, seat(1), None))
            .is_ok()
    );
    play(&mut runner, 1, &mut feed);
    assert_eq!(
        runner.world().wall_owner(at),
        None,
        "neutral once the sphere died"
    );
    assert_eq!(
        runner.world().voxels().get(at),
        Material::wall(seat(0), false),
        "and no voxel was rewritten"
    );
}

// ---------------------------------------------------------------------------
// The threat value and the combat stream
// ---------------------------------------------------------------------------

#[test]
fn the_threat_value_is_the_weapons_damage_per_second() {
    let mut world = world(2);
    let here = field(&world);
    let home = beacon(&mut world, seat(0), here, HOLD_FIRE);
    let gun = raise(&mut world, seat(0), home, StructureKind::Autocannon, here);
    let drone = unit(&mut world, seat(0), home, UnitKind::BuildDrone, here);
    let row = rules()
        .structure(StructureKind::Autocannon)
        .and_then(|row| row.weapon)
        .expect("armed");
    let expected = (u64::from(row.damage.raw().unsigned_abs()) * 1_000_000)
        .checked_div(u64::from(row.cooldown_ms.raw().unsigned_abs()))
        .unwrap();
    assert_eq!(
        world
            .threat_of(AssetId::of_structure(gun).expect("an id"))
            .milli_hp_per_second(),
        expected
    );
    assert_eq!(
        world.threat_of(AssetId::of_unit(drone).expect("an id")),
        Threat::NONE
    );
    let core_threat = world.threat_of(AssetId::of_beacon(core(&world, seat(0))).expect("an id"));
    assert!(core_threat > Threat::NONE, "the core is armed");
    assert_eq!(
        world.threat_of(AssetId::of_beacon(home).expect("an id")),
        Threat::NONE
    );
}

#[test]
fn the_combat_stream_is_drawn_from_nowhere_else() {
    // AGENTS.md section 4.7 and S2's plan, E12: `Stream::Combat` is combat's,
    // drawn in `src/world/combat.rs` and declared in `src/math/random.rs`.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut stack = vec![root];
    let mut found: Vec<String> = Vec::new();
    while let Some(dir) = stack.pop() {
        #[allow(
            clippy::disallowed_methods,
            reason = "the listing is collected and sorted before it is read, as clippy.toml's reason string asks"
        )]
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
            .expect("the source tree")
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = path.to_string_lossy().replace('\\', "/");
            if name.ends_with("world/combat.rs") || name.ends_with("math/random.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("a source file");
            for (number, line) in text.lines().enumerate() {
                let code = line.split("//").next().unwrap_or_default();
                if code.contains("Stream::Combat") {
                    found.push(format!("{name}:{}", number + 1));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "Stream::Combat drawn outside combat: {found:?}"
    );
}

// ---------------------------------------------------------------------------
// The two staged integration tests, in place of scenarios
// ---------------------------------------------------------------------------

/// Run `build` twice and once through a save and restore at tick `at`, and
/// return the first run's hashes at the named ticks, asserting all three runs
/// agree on every one of them.
/// One staged run's start: the runner in its Push, and what to do before
/// each tick.
type Staged = (Runner, Box<dyn Fn(&mut Runner)>);

fn three_ways(
    build: impl Fn() -> Staged,
    ticks: u32,
    named: &[u32],
    save_at: u32,
) -> Vec<(u32, u64)> {
    let run = |restore: bool| -> (Vec<(u32, u64)>, Vec<Event>) {
        let (mut runner, act) = build();
        let mut hashes = Vec::new();
        let mut feed = Vec::new();
        for tick in 1..=ticks {
            act(&mut runner);
            let report = runner.step().expect("runs").expect("in the Push");
            feed.extend_from_slice(runner.events());
            runner.clear_events();
            if named.contains(&tick) {
                hashes.push((tick, report.hash));
            }
            if restore && tick == save_at {
                let saved = Snapshot::capture(runner.world());
                let mut fresh = world_of(rules(), 2);
                saved.restore_into(&mut fresh).expect("restores");
                assert_eq!(fresh.state_hash(), runner.world().state_hash());
                *runner.world_mut() = fresh;
            }
        }
        no_drop(runner.world());
        (hashes, feed)
    };
    let (first, feed) = run(false);
    let (second, again) = run(false);
    let (resumed, _) = run(true);
    assert_eq!(
        first, second,
        "the same seed and layout give the same chain"
    );
    assert_eq!(feed, again, "and the same events");
    assert_eq!(first, resumed, "and a save and restore changes nothing");
    first
}

#[test]
fn a_mortar_breaks_a_wall() {
    // A mortar, directed at a wall through the fixture seam, shells it to a
    // gap: the wall voxels crack, then break; the crater in the stone below
    // cracks before it opens.
    let ticks = 1_200;
    let build = || -> Staged {
        let mut world = world(2);
        let here = field(&world);
        let spot_17 = ground(&world, here, -6, 0);
        let home = beacon(&mut world, seat(0), spot_17, HOLD_FIRE);
        let mortar = raise(&mut world, seat(0), home, StructureKind::Mortar, here);
        let wall_column = ground(&world, here, 20, 0);
        let [x, y, z] = top(&world, wall_column);
        for dz in 1..=2 {
            assert!(
                world
                    .request_voxel_edit(VoxelEdit::Set {
                        at: [x, y, z + dz],
                        material: Material::wall(seat(1), false).expect("a wall id"),
                        by: Some(seat(1)),
                    })
                    .is_ok()
            );
        }
        let runner = pushing(world);
        // Each shell is aimed at the column's surface as it now stands, so
        // the shelling digs on: the wall, the dirt, then the stone.
        let act: Box<dyn Fn(&mut Runner)> = Box::new(move |runner: &mut Runner| {
            let Some(top) = runner.world().voxels().top_solid_z(x, y) else {
                return;
            };
            let aim = [
                Fx::from_voxels(i16::try_from(x).expect("an x")),
                Fx::from_voxels(i16::try_from(y).expect("a y")),
                Fx::from_voxels(i16::try_from(top + 1).expect("a z")),
            ];
            let _ = runner.world_mut().order_shell(mortar, aim);
        });
        (runner, act)
    };
    let hashes = three_ways(build, ticks, &[1, 100, 300, 600, 1_200], 250);
    assert_eq!(hashes.len(), 5);
    // And the wall is gone.
    let (mut runner, act) = build();
    let mut feed = Vec::new();
    let here = field(runner.world());
    let [x, y, z] = top(runner.world(), ground(runner.world(), here, 20, 0));
    let cracked = Material::wall(seat(1), true);
    let mut saw_cracked = false;
    let mut saw_stone_cracked = false;
    for _ in 0..ticks {
        act(&mut runner);
        play(&mut runner, 1, &mut feed);
        let voxels = runner.world().voxels();
        saw_cracked |= (1..=2).any(|dz| voxels.get([x, y, z + dz]) == cracked);
        saw_stone_cracked |=
            (z - 4..=z).any(|zz| voxels.get([x, y, zz]) == Some(Material::STONE_CRACKED_ONCE));
    }
    assert!(saw_cracked, "a wall voxel cracks before it breaks");
    assert!(
        saw_stone_cracked,
        "the stone under it cracks before it opens"
    );
    let world = runner.world();
    assert!(count(&feed, EventKind::ShellLanded) >= 2);
    for dz in 1..=2 {
        assert_eq!(
            world.voxels().get([x, y, z + dz]),
            Some(Material::AIR),
            "a gap at {dz}"
        );
    }
}

#[test]
fn a_raid_on_a_mortar() {
    // Raiders dive a mortar inside the core's reach; the autocannon beside it
    // and the core kill them, and each leaves a wreck.
    let ticks = 400;
    let build = || -> Staged {
        let mut world = world(2);
        let core_at = beacon_at(&world, core(&world, seat(0)));
        world.set_common(
            core(&world, seat(0)),
            Common {
                roe: FREE_FIRE,
                retreat_hp_pct: 0,
            },
        );
        let home = core(&world, seat(0));
        let mortar_at = ground(&world, core_at, 10, 0);
        let _mortar = raise(&mut world, seat(0), home, StructureKind::Mortar, mortar_at);
        let spot_18 = ground(&world, core_at, 6, 0);
        let _gun = raise(
            &mut world,
            seat(0),
            home,
            StructureKind::Autocannon,
            spot_18,
        );
        let raid_at = ground(&world, core_at, 12, 0);
        let raid_home = beacon(&mut world, seat(1), raid_at, FREE_FIRE);
        for _ in 0..3 {
            let _ = unit(&mut world, seat(1), raid_home, UnitKind::Raider, raid_at);
        }
        let runner = pushing(world);
        let act: Box<dyn Fn(&mut Runner)> = Box::new(|_: &mut Runner| {});
        (runner, act)
    };
    let hashes = three_ways(build, ticks, &[1, 50, 200, 400], 120);
    assert_eq!(hashes.len(), 4);
    let (mut runner, _) = build();
    let mut feed = Vec::new();
    play(&mut runner, ticks, &mut feed);
    assert_eq!(
        count(&feed, EventKind::UnitDestroyed),
        3,
        "every raider died"
    );
    assert_eq!(count(&feed, EventKind::WreckLeft), 3, "and left a wreck");
    let world = runner.world();
    assert_eq!(world.tally(seat(1)).expect("a seat").losses, 3);
    assert_eq!(world.tally(seat(0)).expect("a seat").kills, 3);
    assert!(world.combat_counts().shots() > 0);
    let _ = Hp::ZERO;
}
