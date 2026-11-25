// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Each zero title the gateway sends is held to the sim (S3's `zerotitle`,
//! decisions-log item 184 (9)): a title says what a 0 does, so a test drives
//! the sim at that 0, beside the same setting at a non-zero, and reads the
//! rule the title names. A title nobody can check is not written:
//! `every_title_is_checked_here` fails on a title the vocabulary carries
//! that this file does not drive.
//!
//! The gateway is the crate that hosts and steps the sim, so the check lives
//! here rather than in plan-core, which may never step it (AGENTS.md section
//! 3 rule 2); plan-core's `tests/zero_titles.rs` holds which settings carry a
//! title.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules, not an integration test's helpers. A panic is this file's failure report: these are panic lints, not determinism lints (AGENTS.md section 5)."
)]

use pharmakos_plan_core::strings as s;
use pharmakos_plan_core::vocabulary::{common_parameters, vocabulary};
use pharmakos_sim::attack::{Aim, AttackSettings, Phase};
use pharmakos_sim::economy::Purchase;
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::math::quantity::{Damage, Money};
use pharmakos_sim::mining::MineSettings;
use pharmakos_sim::runner::{MatchPhase, Runner};
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::site::{BuildSettings, Terraform};
use pharmakos_sim::tables::{
    BeaconId, Common, DefendSettings, PRIORITY_NORMAL, Pursue, SeatId, StructureKind, UnitKind,
};
use pharmakos_sim::voxels::Material;
use pharmakos_sim::world::{DamageOrder, DamageTarget};
use pharmakos_sim::{MatchSettings, RulesTable, World, WorldConfig, mandate_for};

const SEED: u64 = 0x00F1_2E00_C0DE_0001;

fn rules() -> RulesTable {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("rules")
        .join("rules.v1.json");
    RulesTable::load(&path).unwrap_or_else(|error| panic!("loading the rules table: {error}"))
}

fn world() -> World {
    let mut world = World::new(&WorldConfig {
        match_seed: SEED,
        seats: 2,
        rules: rules(),
        match_settings: MatchSettings {
            segment_lengths_ms: vec![600_000],
            round_limit: 3,
        },
    })
    .unwrap_or_else(|error| panic!("generating the world: {error}"));
    world.set_treasury(seat(0), Money::new(100_000));
    world
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

fn column_of(world: &World, beacon: BeaconId) -> [i32; 2] {
    let row = world.beacons().row_of(beacon).expect("a live beacon");
    let [x, y, _] = world.beacons().positions()[row.index()];
    [x.floor_voxels(), y.floor_voxels()]
}

/// The standing point on the ground between the two cores.
fn field(world: &World) -> [Fx; 3] {
    let [ax, ay] = column_of(world, core(world, seat(0)));
    let [bx, by] = column_of(world, core(world, seat(1)));
    world.standing_point([i32::midpoint(ax, bx), i32::midpoint(ay, by), 0])
}

/// A standing point `dx`, `dy` columns from seat 0's core.
fn by_core(world: &World, dx: i32, dy: i32) -> [Fx; 3] {
    let [x, y] = column_of(world, core(world, seat(0)));
    world.standing_point([x + dx, y + dy, 0])
}

fn damage(target: DamageTarget, amount: i64) -> DamageOrder {
    DamageOrder {
        target,
        amount: Damage::new(amount).expect("a positive amount"),
        by: seat(1),
        source: None,
    }
}

/// A runner in its first Push, stepped `ticks` ticks.
fn stepped(world: World, ticks: u32) -> Runner {
    let mut runner = Runner::new(world).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(runner.begin_push(), Ok(true), "the Push begins");
    step(&mut runner, ticks);
    runner
}

/// Step `ticks` ticks of the Push, stopping early when the Push ends.
fn step(runner: &mut Runner, ticks: u32) {
    for _ in 0..ticks {
        if runner.phase() != MatchPhase::Push {
            return;
        }
        let report = runner
            .step()
            .unwrap_or_else(|error| panic!("the tick runs: {error}"));
        assert!(report.is_some(), "still in the Push");
        runner.clear_events();
    }
}

/// A voxel position as an Attack setting holds one.
fn voxel(at: [Fx; 3]) -> [i16; 3] {
    at.map(|axis| i16::try_from(axis.floor_voxels()).expect("on the map"))
}

/// An Attack beacon of seat 0 on the field, aimed at a voxel 30 columns off,
/// gathering at a staging point 12 columns off, with one raider fielded at
/// the beacon (not at the staging point), stepped `ticks` ticks: its phase
/// then, and how often it launched.
fn attack_phase(settings: AttackSettings, ticks: u32) -> Phase {
    let mut world = world();
    let here = field(&world);
    let [x, y] = [here[0].floor_voxels(), here[1].floor_voxels()];
    let target = world.standing_point([x + 30, y, 0]);
    let staging = world.standing_point([x, y + 12, 0]);
    let post = world
        .place_beacon_directly(seat(0), here, MandateKind::Attack, PRIORITY_NORMAL)
        .expect("a beacon");
    assert!(
        world.set_attack(
            post,
            AttackSettings {
                aim: Aim::Voxel(voxel(target)),
                staging: Some(voxel(staging)),
                waves: 1,
                ..settings
            },
            &[],
        ),
        "an Attack row"
    );
    world
        .field_unit(seat(0), post, UnitKind::Raider, here)
        .expect("a raider");
    let runner = stepped(world, ticks);
    runner
        .world()
        .attack()
        .row_of(post)
        .expect("the Attack row")
        .phase
}

/// "retreat off": a raider whose home's `retreat_hp_pct` is 0 does not
/// retreat, hurt to a third of its hit points; at 50 it does.
fn retreat_is_off_at_zero() {
    for (threshold, retreats) in [(50_u8, true), (0, false)] {
        let mut world = world();
        let home = core(&world, seat(0));
        world.set_common(
            home,
            Common {
                roe: 1,
                retreat_hp_pct: threshold,
            },
        );
        let at = by_core(&world, 6, 0);
        let raider = world
            .field_unit(seat(0), home, UnitKind::Raider, at)
            .expect("a raider");
        let row = world.units().row_of(raider).expect("alive");
        let hp = i64::from(world.units().hit_points()[row.index()].raw());
        world
            .request_damage(damage(
                DamageTarget::Unit(raider),
                (hp * 2).checked_div(3).expect("a third"),
            ))
            .expect("the hit is filed");
        let runner = stepped(world, 1);
        let row = runner.world().units().row_of(raider).expect("alive");
        assert_eq!(
            runner.world().units().retreats()[row.index()] != BeaconId::NONE,
            retreats,
            "retreat_hp_pct {threshold}"
        );
    }
}

/// "never repairing structures, beacons or walls": with a repair drone beside
/// it, a Build beacon whose `repair_threshold_pct` is 0 never raises a
/// damaged structure's hit points; at 100 it does (review item 1: the hit
/// points themselves, not a proxy).
fn repair_is_off_at_zero() {
    for (threshold, repaired) in [(100_u8, true), (0, false)] {
        let mut world = world();
        let beacon = core(&world, seat(0));
        world.set_build_settings_directly(
            beacon,
            BuildSettings {
                repair_threshold_pct: threshold,
                rebuild_destroyed: false,
                terraform: Terraform::None,
            },
        );
        let drone_at = by_core(&world, 1, 1);
        world
            .field_unit(seat(0), beacon, UnitKind::RepairDrone, drone_at)
            .expect("room for a repair drone");
        let at = by_core(&world, 4, -5);
        let cannon = world
            .raise_structure(seat(0), beacon, StructureKind::Autocannon, at)
            .expect("room for a structure");
        let hp_of = |world: &World| {
            let row = world.structures().row_of(cannon).expect("standing");
            i64::from(world.structures().hit_points()[row.index()].raw())
        };
        let full = hp_of(&world);
        world
            .request_damage(damage(
                DamageTarget::Structure(cannon),
                (full * 9).checked_div(10).expect("a tenth"),
            ))
            .expect("the hit is filed");
        let mut runner = stepped(world, 1);
        let hurt = hp_of(runner.world());
        assert!(hurt < full, "the hit landed");
        step(&mut runner, 1_200);
        let after = hp_of(runner.world());
        assert_eq!(
            after > hurt,
            repaired,
            "repair_threshold_pct {threshold}: {hurt} -> {after}"
        );
    }
}

/// "launching at once with its first raider, ungathered": a `force_size` of
/// 0 launches the one raider standing at the beacon with nothing staged; a
/// force of 1 waits for it to reach the staging point (review item C).
fn a_force_of_zero_launches_ungathered() {
    for (force, launched) in [(0_u32, true), (1, false)] {
        let phase = attack_phase(
            AttackSettings {
                force_size: Some(force),
                ..AttackSettings::default()
            },
            2,
        );
        assert_eq!(
            phase == Phase::Launched,
            launched,
            "force_size {force}: {phase:?}"
        );
    }
}

/// "no scouts": a Survey beacon whose scout count is 0 asks for no scout; at
/// 2 it asks for one.
fn no_scouts_at_zero() {
    for (scouts, asks) in [(2_u8, true), (0, false)] {
        let mut world = world();
        let here = field(&world);
        let post = world
            .place_beacon_directly(seat(0), here, MandateKind::Survey, PRIORITY_NORMAL)
            .expect("a beacon");
        world.set_scouts(post, scouts);
        let survey = mandate_for(MandateKind::Survey).expect("Survey runs");
        assert_eq!(
            survey
                .request(&world, post)
                .is_some_and(|request| request.buys == Purchase::Unit(UnitKind::Scout)),
            asks,
            "scout_count {scouts}"
        );
    }
}

/// The match the Mine checks play: two segments of three minutes, on the
/// seed and seat count the sim's own mining tests use, so seat 0's core has
/// a seam. Answers the world before, the world after, and the seam it held.
fn mining(settings: MineSettings) -> (World, World, usize) {
    let mut world = World::new(&WorldConfig {
        match_seed: 0x00A1_B2C3_D4E5_F607,
        seats: 3,
        rules: rules(),
        match_settings: MatchSettings {
            segment_lengths_ms: vec![180_000, 180_000],
            round_limit: 2,
        },
    })
    .unwrap_or_else(|error| panic!("generating the world: {error}"));
    let core = core(&world, seat(0));
    world.set_writ(core, MandateKind::Mine);
    world.set_mine_settings(core, settings);
    let before = world.clone();
    let mut runner = Runner::new(world).unwrap_or_else(|error| panic!("{error}"));
    let mut seam = None;
    loop {
        assert_eq!(runner.begin_push(), Ok(true), "the Push begins");
        step(&mut runner, 1);
        if seam.is_none() {
            let row = runner.world().beacons().row_of(core).expect("the core");
            seam = runner.world().beacons().seams().get(row.index()).copied();
        }
        step(&mut runner, u32::MAX);
        if !runner.end_recap().unwrap_or_else(|error| panic!("{error}"))
            || runner.phase() != MatchPhase::Lull
        {
            break;
        }
    }
    let seam = usize::try_from(seam.expect("the core chose a seam")).expect("an index");
    (before, runner.world().clone(), seam)
}

/// Whether the ore voxel at `at` in `before` is gone in `after`.
fn dug(before: &World, after: &World, at: [i32; 3]) -> bool {
    before
        .voxels()
        .get(at)
        .and_then(Material::ore_richness)
        .is_some()
        && after
            .voxels()
            .get(at)
            .and_then(Material::ore_richness)
            .is_none()
}

/// "digging the exposed rim only": at a `dig_max_depth` of 0 nothing below
/// the seam's original top is dug; at 1 something one voxel down is.
fn rim_only_at_zero() {
    for (depth, below) in [(1_u32, true), (0, false)] {
        let (before, after, seam) = mining(MineSettings {
            dig_max_depth: depth,
            ..MineSettings::default()
        });
        let feature = before.features().get(seam).expect("the seam");
        let deeper = feature
            .footprint
            .iter()
            .any(|column| dug(&before, &after, [column.x, column.y, column.top - 1]));
        assert_eq!(deeper, below, "dig_max_depth {depth}");
    }
}

/// "leaving no pillars": at a `pillar_spacing` of 0 a column on the anchor's
/// grid of 2 is dug; at 2 none of them is.
fn no_pillars_at_zero() {
    for (spacing, kept) in [(2_u32, true), (0, false)] {
        let (before, after, seam) = mining(MineSettings {
            pillar_spacing: spacing,
            ..MineSettings::default()
        });
        let feature = before.features().get(seam).expect("the seam");
        let [ax, ay] = feature.anchor;
        let on_grid_dug = feature
            .footprint
            .iter()
            .filter(|column| {
                (column.x - ax).rem_euclid(2) == 0 && (column.y - ay).rem_euclid(2) == 0
            })
            .any(|column| dug(&before, &after, [column.x, column.y, column.top]));
        assert_eq!(!on_grid_dug, kept, "pillar_spacing {spacing}");
    }
}

/// "no picket": a quiet Defend beacon with `picket_scouts` 0 asks for no
/// scout; at 2 it asks for one.
fn picket_is_off_at_zero() {
    for (pickets, asks) in [(2_u8, true), (0, false)] {
        let mut world = world();
        let here = field(&world);
        let post = world
            .place_beacon_directly(seat(0), here, MandateKind::Defend, PRIORITY_NORMAL)
            .expect("a beacon");
        assert!(
            world.set_defend(
                post,
                DefendSettings {
                    engagement_pct: 100,
                    pursue: Pursue::Never,
                    pickets,
                    escort: false,
                },
                &[],
            ),
            "a Defend row"
        );
        let defend = mandate_for(MandateKind::Defend).expect("Defend runs");
        let request = defend.request(&world, post);
        assert_eq!(
            request.is_some_and(|request| request.buys == Purchase::Unit(UnitKind::Scout)),
            asks,
            "picket_scouts {pickets}"
        );
    }
}

#[test]
fn a_zero_retreat_threshold_turns_retreat_off() {
    retreat_is_off_at_zero();
}

#[test]
fn a_zero_repair_threshold_never_repairs_a_structure() {
    repair_is_off_at_zero();
}

#[test]
fn a_zero_picket_stands_the_picket_down() {
    picket_is_off_at_zero();
}

#[test]
fn a_zero_force_launches_its_first_raider_ungathered() {
    a_force_of_zero_launches_ungathered();
}

#[test]
fn a_zero_scout_count_fields_no_scout() {
    no_scouts_at_zero();
}

#[test]
fn a_zero_dig_depth_digs_the_rim_only() {
    rim_only_at_zero();
}

#[test]
fn a_zero_pillar_spacing_leaves_no_pillar() {
    no_pillars_at_zero();
}

/// The untitled zeros of plan-core's sweep, driven (review item 2): a 0
/// that is just a number behaves as its neighbour does. A `segment_ms` of 0
/// launches the force as one of 50 ms (a tick) does, and an
/// `after_launch_ms` of 0 breaks the attack off as one of 50 ms does, each
/// against the same launch and the same ticks; and neither is the phase a
/// far value gives, so the check reads the setting at all.
#[test]
fn an_untitled_zero_behaves_as_its_neighbour() {
    let launch = |ms: i32| {
        attack_phase(
            AttackSettings {
                launch_ms: Some(ms),
                ..AttackSettings::default()
            },
            2,
        )
    };
    assert_eq!(launch(0), launch(50), "segment_ms 0 and 50");
    assert_ne!(launch(0), launch(600_000), "segment_ms is read");
    let break_off = |ms: i32| {
        attack_phase(
            AttackSettings {
                force_size: Some(0),
                retreat_ms: Some(ms),
                ..AttackSettings::default()
            },
            6,
        )
    };
    assert_eq!(break_off(0), break_off(50), "after_launch_ms 0 and 50");
    assert_ne!(break_off(0), break_off(600_000), "after_launch_ms is read");
}

/// The titles the vocabulary carries are exactly the seven driven above,
/// each by the words the check names; a title added without a check here
/// fails.
#[test]
fn every_title_is_checked_here() {
    let checked: [(&str, &str, &str); 7] = [
        ("", "/retreat_hp_pct", s::RETREAT_OFF),
        ("build", "/repair_threshold_pct", s::REPAIR_NEVER),
        ("defend", "/picket_scouts", s::PICKET_NONE),
        ("attack", "/launch/force_size", s::LAUNCH_FIRST_RAIDER),
        ("survey", "/scout_count", s::SCOUTS_NONE),
        ("mine", "/dig_max_depth", s::DIG_RIM_ONLY),
        ("mine", "/pillar_spacing", s::PILLARS_NONE),
    ];
    let mut titled: Vec<(&str, &str, &str)> = Vec::new();
    for parameter in common_parameters().expect("the common settings") {
        if let Some(title) = parameter.shape.zero_title() {
            titled.push(("", parameter.pointer, title));
        }
    }
    let vocabulary = vocabulary(&[]).expect("the vocabulary");
    for entry in vocabulary
        .mandates
        .iter()
        .chain(&vocabulary.steps)
        .chain(&vocabulary.groups)
        .chain(
            vocabulary
                .families
                .iter()
                .flat_map(|family| &family.predicates),
        )
    {
        for parameter in &entry.parameters {
            if let Some(title) = parameter.shape.zero_title() {
                titled.push((entry.name, parameter.pointer, title));
            }
        }
    }
    assert_eq!(titled, checked);
}
