// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The layout's mandate settings (S3's plan, E13; decisions-log item 152 (1)
//! 10): the gate load's Attack seam. A layout with an Attack row seeds and
//! launches, in the shape the walled harness builds its load; a layout with no
//! row places the world it always did; a row the world cannot take is refused
//! with its typed reason and places nothing.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules — not an integration test's helper functions. A panic is this file's failure report."
)]

use pharmakos_sim::attack::{Aim, AttackSettings, Phase};
use pharmakos_sim::events::EventKind;
use pharmakos_sim::runner::{
    Layout, LayoutBeacon, LayoutMandateSettings, LayoutRefused, LayoutSetting, LayoutSettings,
};
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::tables::{BeaconId, SeatId, UnitKind};
use pharmakos_sim::{
    MatchPhase, MatchSettings, RulesTable, Runner, World, WorldConfig, default_rules_path,
};
use std::path::PathBuf;

/// The golden seed, as the gate's load plays it (S2's plan, decision 2).
const SEED: u64 = 0x0000_0000_CA5C_ADED;

/// The gate's seats.
const SEATS: u32 = 3;

/// The raiders of one of the gate load's Attack beacons.
const RAIDERS: u32 = 18;

/// How far from the enemy core the gate load stands its Attack beacons
/// (`crates/bench/src/g3/layout.rs`' `ATTACK_FROM_ENEMY_CORE`, a harness input
/// restated so this test keeps the load's shape).
const FROM_ENEMY_CORE: i64 = 14;

/// A launch is a decision tick or two away for a force that stands gathered;
/// this bounds the wait so a force that never launches fails rather than runs.
const LAUNCH_WITHIN_TICKS: u32 = 400;

fn rules() -> RulesTable {
    let path = default_rules_path().unwrap_or_else(|| PathBuf::from("rules/rules.v1.json"));
    RulesTable::load(&path).unwrap_or_else(|error| panic!("loading the rules table: {error}"))
}

fn runner() -> Runner {
    Runner::new(
        World::new(&WorldConfig {
            match_seed: SEED,
            seats: SEATS,
            rules: rules(),
            match_settings: MatchSettings {
                segment_lengths_ms: vec![600_000],
                round_limit: 1,
            },
        })
        .expect("the golden seed makes a world"),
    )
}

/// `seat`'s core: its one beacon before anything is placed.
fn core(world: &World, seat: u8) -> (BeaconId, [i64; 2]) {
    let beacons = world.beacons();
    let row = beacons
        .seats()
        .iter()
        .position(|owner| *owner == seat)
        .expect("a core");
    let at = beacons.positions()[row];
    (
        BeaconId::new(beacons.ids()[row]),
        [
            i64::from(at[0].floor_voxels()),
            i64::from(at[1].floor_voxels()),
        ],
    )
}

/// The column `distance` voxels from `to` on the line towards `from`, as the
/// gate's layout stands an Attack beacon.
fn towards(from: [i64; 2], to: [i64; 2], distance: i64) -> [i32; 2] {
    let (dx, dy) = (from[0] - to[0], from[1] - to[1]);
    let length = (dx * dx + dy * dy).isqrt();
    [(to[0], dx), (to[1], dy)].map(|(axis, delta)| {
        let step = (delta * distance)
            .checked_div(length)
            .expect("two cores apart");
        i32::try_from(axis + step).expect("on the map")
    })
}

/// The gate load's shape, one seat's worth: seat 0 stands two Attack beacons
/// inside seat 1's core's range, the first with a row and the second without.
fn gate_shape(world: &World) -> (Layout, BeaconId) {
    let (_, own) = core(world, 0);
    let (target, enemy) = core(world, 1);
    let beacon = |distance: i64| LayoutBeacon {
        seat: SeatId::new(0),
        column: towards(own, enemy, distance),
        mandate: MandateKind::Attack,
        units: vec![(UnitKind::Raider, RAIDERS)],
    };
    (
        Layout {
            beacons: vec![beacon(FROM_ENEMY_CORE), beacon(FROM_ENEMY_CORE + 6)],
        },
        target,
    )
}

fn raid_on(target: BeaconId) -> AttackSettings {
    AttackSettings {
        aim: Aim::Beacon(target),
        force_size: Some(RAIDERS),
        waves: 1,
        ..AttackSettings::default()
    }
}

fn row(beacon: usize, settings: AttackSettings) -> LayoutSettings {
    LayoutSettings {
        beacon,
        settings: LayoutMandateSettings::Attack(settings),
    }
}

/// The Attack beacons a layout placed, in its order: seat 0's non-core rows.
fn placed(world: &World) -> Vec<BeaconId> {
    let beacons = world.beacons();
    beacons
        .ids()
        .iter()
        .zip(beacons.seats())
        .filter(|(_, owner)| **owner == 0)
        .skip(1)
        .map(|(id, _)| BeaconId::new(*id))
        .collect()
}

#[test]
fn a_layout_with_an_attack_row_seeds_and_launches() {
    let mut runner = runner();
    let (layout, target) = gate_shape(runner.world());
    runner
        .seed_layout_with_settings(&layout, &[row(0, raid_on(target))])
        .expect("the lobby takes the layout");
    let beacons = placed(runner.world());
    let (raiding, control) = (beacons[0], beacons[1]);
    assert_eq!(
        runner
            .world()
            .attack()
            .row_of(raiding)
            .expect("an Attack row")
            .settings,
        raid_on(target),
        "the row's settings are the beacon's"
    );
    assert_eq!(runner.begin_push(), Ok(true), "the Push begins");
    let mut launches = 0_usize;
    for _ in 0..LAUNCH_WITHIN_TICKS {
        assert_eq!(runner.phase(), MatchPhase::Push);
        runner.step().expect("the tick runs").expect("in the Push");
        launches += runner
            .events()
            .iter()
            .filter(|event| event.kind == EventKind::AttackLaunched)
            .count();
        runner.clear_events();
        if launches > 0 {
            break;
        }
    }
    assert_eq!(launches, 1, "the raid launches, and it alone");
    let attack = runner.world().attack();
    assert_eq!(
        attack.row_of(raiding).expect("its row").phase,
        Phase::Launched
    );
    assert_eq!(
        attack.row_of(control).expect("its row").phase,
        Phase::Gathering,
        "the beacon with no row gathers, as every laid-out Attack beacon did before the seam"
    );
}

#[test]
fn no_row_is_the_world_the_layout_always_placed() {
    let mut plain = runner();
    let (layout, target) = gate_shape(plain.world());
    plain.seed_layout(&layout).expect("the lobby takes it");
    let mut empty = runner();
    empty
        .seed_layout_with_settings(&layout, &[])
        .expect("the lobby takes it");
    assert_eq!(
        plain.world().state_hash(),
        empty.world().state_hash(),
        "no row moves no hash, which is why no chain moves for the seam"
    );
    let mut set = runner();
    set.seed_layout_with_settings(&layout, &[row(0, raid_on(target))])
        .expect("the lobby takes it");
    assert_ne!(
        plain.world().state_hash(),
        set.world().state_hash(),
        "a row is state, in the Attack table the hash already covers"
    );
}

/// The row is state like any other Attack row's: a save of the laid-out match
/// restores it, hash for hash.
#[test]
fn a_rows_settings_survive_a_save() {
    let mut first = runner();
    let (layout, target) = gate_shape(first.world());
    first
        .seed_layout_with_settings(&layout, &[row(0, raid_on(target))])
        .expect("the lobby takes it");
    let saved = first.capture().expect("between ticks");
    let mut resumed = runner();
    resumed.restore(&saved).expect("the save restores");
    assert_eq!(first.world().state_hash(), resumed.world().state_hash());
    let beacon = placed(resumed.world())[0];
    assert_eq!(
        resumed
            .world()
            .attack()
            .row_of(beacon)
            .expect("its row")
            .settings,
        raid_on(target)
    );
    assert_eq!(
        resumed.seed_layout_with_settings(&layout, &[]),
        Err(LayoutRefused::NotInTheLobby),
        "and it is still one layout"
    );
}

/// Every refusal of a row, each with its typed reason, and each leaving the
/// world as it was.
#[test]
fn a_row_the_world_cannot_take_is_refused_and_places_nothing() {
    let mut lobby = runner();
    let (layout, target) = gate_shape(lobby.world());
    let own = core(lobby.world(), 0).0;
    let raid = raid_on(target);
    let bad = |setting: LayoutSetting| LayoutRefused::BadSetting { index: 0, setting };
    let edited = |edit: &dyn Fn(&mut AttackSettings)| {
        let mut settings = raid;
        edit(&mut settings);
        vec![row(0, settings)]
    };
    let cases: Vec<(Vec<LayoutSettings>, LayoutRefused)> = vec![
        (
            vec![row(2, raid)],
            LayoutRefused::NoSuchLayoutBeacon { index: 2 },
        ),
        (
            vec![row(0, raid), row(0, raid)],
            LayoutRefused::SettingsTwice { index: 0 },
        ),
        (edited(&|s| s.aim = Aim::Unset), bad(LayoutSetting::Aim)),
        (
            edited(&|s| s.aim = Aim::Beacon(own)),
            bad(LayoutSetting::Aim),
        ),
        (
            edited(&|s| s.aim = Aim::Beacon(BeaconId::NONE)),
            bad(LayoutSetting::Aim),
        ),
        (
            edited(&|s| s.aim = Aim::Commander(Some(0))),
            bad(LayoutSetting::Aim),
        ),
        (
            edited(&|s| s.aim = Aim::Commander(Some(3))),
            bad(LayoutSetting::Aim),
        ),
        (
            edited(&|s| s.aim = Aim::Voxel([i16::MAX, 0, 0])),
            bad(LayoutSetting::Aim),
        ),
        (
            edited(&|s| {
                s.force_size = None;
                s.launch_ms = None;
            }),
            bad(LayoutSetting::Launch),
        ),
        (
            edited(&|s| s.staging = Some([-1, 0, 0])),
            bad(LayoutSetting::Staging),
        ),
        (
            edited(&|s| s.launch_ms = Some(-1)),
            bad(LayoutSetting::LaunchMs),
        ),
        (
            edited(&|s| s.retreat_pct = Some(0)),
            bad(LayoutSetting::RetreatPct),
        ),
        (
            edited(&|s| s.retreat_pct = Some(101)),
            bad(LayoutSetting::RetreatPct),
        ),
        (
            edited(&|s| s.retreat_ms = Some(-1)),
            bad(LayoutSetting::RetreatMs),
        ),
    ];
    let before = lobby.world().state_hash();
    for (rows, refused) in cases {
        assert_eq!(
            lobby.seed_layout_with_settings(&layout, &rows),
            Err(refused)
        );
        assert_eq!(lobby.world().state_hash(), before, "{refused}");
        assert!(!lobby.world().is_laid_out(), "{refused}");
    }
    let mut build = layout.clone();
    build.beacons[0].mandate = MandateKind::Build;
    assert_eq!(
        lobby.seed_layout_with_settings(&build, &[row(0, raid)]),
        Err(LayoutRefused::WrongWrit { index: 0 }),
        "an Attack row on a Build beacon is refused, never dropped"
    );
    assert!(!lobby.world().is_laid_out());
    assert_eq!(
        lobby.seed_layout_with_settings(&layout, &[row(0, raid)]),
        Ok(()),
        "and the lobby it left still takes a good one"
    );
    // A refused row's words name what was refused.
    assert_eq!(
        bad(LayoutSetting::RetreatMs).to_string(),
        "beacon 0's retreat time cannot be taken"
    );
}

/// A segment time alone is a launch condition, as a force alone is.
#[test]
fn a_segment_time_alone_is_taken() {
    let mut lobby = runner();
    let (layout, target) = gate_shape(lobby.world());
    let settings = AttackSettings {
        force_size: None,
        launch_ms: Some(0),
        ..raid_on(target)
    };
    assert_eq!(
        lobby.seed_layout_with_settings(&layout, &[row(0, settings)]),
        Ok(())
    );
}

/// A row reaches every target arm a load may name, and a beacon of another
/// seat placed before the layout is one of them.
#[test]
fn every_legal_aim_is_taken() {
    let probe = runner();
    let (layout, target) = gate_shape(probe.world());
    for aim in [
        Aim::Beacon(target),
        Aim::Voxel([10, 10, 10]),
        Aim::Commander(None),
        Aim::Commander(Some(1)),
        Aim::NearestKnownBeacon,
    ] {
        let mut lobby = runner();
        let settings = AttackSettings {
            aim,
            ..raid_on(target)
        };
        assert_eq!(
            lobby.seed_layout_with_settings(&layout, &[row(1, settings)]),
            Ok(()),
            "{aim:?}"
        );
        let beacon = placed(lobby.world())[1];
        assert_eq!(
            lobby
                .world()
                .attack()
                .row_of(beacon)
                .expect("its row")
                .settings
                .aim,
            aim
        );
    }
}
