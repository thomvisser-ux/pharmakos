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
use pharmakos_sim::economy::Purchase;
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::math::quantity::{Damage, Money};
use pharmakos_sim::runner::Runner;
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::site::{self, BuildSettings, Terraform};
use pharmakos_sim::tables::{
    BeaconId, Common, DefendSettings, PRIORITY_NORMAL, Pursue, SeatId, StructureKind, UnitKind,
};
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
    for _ in 0..ticks {
        let report = runner
            .step()
            .unwrap_or_else(|error| panic!("the tick runs: {error}"));
        assert!(report.is_some(), "still in the Push");
        runner.clear_events();
    }
    runner
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

/// "structures never repaired": a Build beacon whose `repair_threshold_pct`
/// is 0 counts no damaged structure as repair work; at 100 it counts it.
fn repair_is_off_at_zero() {
    for (threshold, counted) in [(100_u8, true), (0, false)] {
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
        let at = by_core(&world, 4, -5);
        let cannon = world
            .raise_structure(seat(0), beacon, StructureKind::Autocannon, at)
            .expect("room for a structure");
        let row = world.structures().row_of(cannon).expect("standing");
        let hp = i64::from(world.structures().hit_points()[row.index()].raw());
        world
            .request_damage(damage(
                DamageTarget::Structure(cannon),
                (hp * 9).checked_div(10).expect("a tenth"),
            ))
            .expect("the hit is filed");
        let runner = stepped(world, 1);
        assert_eq!(
            site::repair_work(runner.world(), beacon) > 0,
            counted,
            "repair_threshold_pct {threshold}"
        );
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

/// The titles the vocabulary carries are exactly the three driven above,
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
