// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Three number settings carry presence and four list settings can be
//! cleared (decisions-log item 184 (10), the owner's "Fix all seven").
//!
//! Before, a `set_mandate_settings` row could not SET `scout_count`,
//! `dig_max_depth` or `pillar_spacing` to 0, because proto3 read a written 0
//! as a field left out, and could not EMPTY `protected_areas`, `protect`,
//! `avoid_areas` or `probe_areas`, because it read an empty list the same way.
//! The numbers are `optional` now, so a written 0 is one field of the edit and
//! takes effect; each list has a `clear_*` flag beside it, which empties the
//! beacon's list in an edit and is refused beside a non-empty list or in
//! settings that start from nothing.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules, not an \
              integration test's helpers. A panic is this file's failure report."
)]

use std::path::PathBuf;

use pharmakos_proto::gp;
use pharmakos_proto::json;
use pharmakos_sim::events::EventKind;
use pharmakos_sim::interpreter::{Action, ClearRefusal, Plan, PlanError, Row};
use pharmakos_sim::math::quantity::Money;
use pharmakos_sim::mining::MineSettings;
use pharmakos_sim::runner::{MatchPhase, MatchSettings, Runner};
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::tables::{BeaconId, SeatId, TargetKind, UnitKind};
use pharmakos_sim::voxels::Material;
use pharmakos_sim::{RulesTable, World, WorldConfig};

/// The seed `tests/build.rs`, `tests/economy.rs` and `tests/mining.rs` play on.
const SEED: u64 = 0x00A1_B2C3_D4E5_F607;

const SEAT: SeatId = SeatId::new(0);

fn rules() -> RulesTable {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    RulesTable::load(&root.join("rules").join("rules.v1.json"))
        .expect("the committed rules table loads")
}

fn world(segments: &[i32]) -> World {
    World::new(&WorldConfig {
        match_seed: SEED,
        seats: 3,
        rules: rules(),
        match_settings: MatchSettings {
            segment_lengths_ms: segments.to_vec(),
            round_limit: 1,
        },
    })
    .expect("the rules table describes a map")
}

/// Seat 0's core, its `b_00`.
fn core(world: &World) -> BeaconId {
    let row = world
        .beacons()
        .row_of_ordinal(SEAT, 0)
        .expect("seat 0 has a core");
    BeaconId::new(u32::try_from(row).expect("a beacon row"))
}

fn row_of(beacon: BeaconId) -> usize {
    usize::try_from(beacon.raw()).expect("a row")
}

fn core_voxel(world: &World) -> [i32; 3] {
    let [x, y, z] = world
        .beacons()
        .positions()
        .get(row_of(core(world)))
        .copied()
        .expect("the core stands somewhere");
    [x.floor_voxels(), y.floor_voxels(), z.floor_voxels()]
}

/// A whole playbook around one route step, as canonical `gp.v1` JSON.
fn playbook(route: &str) -> gp::v1::Playbook {
    let text = format!(
        concat!(
            "{{\"schema_version\":{{\"major\":1}},",
            "\"meta\":{{\"title\":\"presence\",\"author_kind\":\"HUMAN\"}},",
            "\"declarative\":{{\"route\":[{route}]}},",
            "\"on_death\":{{\"on_respawn\":\"CONTINUE\",\"max_deaths_before_fallback\":3}},",
            "\"fallback\":{{\"hold\":{{\"at\":{{\"safest\":{{}}}}}}}},",
            "\"kind\":\"PLAYBOOK\"}}"
        ),
        route = route
    );
    json::decode(&text).unwrap_or_else(|error| panic!("the case decodes: {error}\n{text}"))
}

/// A visit to `b_00` with the given rows.
fn visit_with(rows: &[&str]) -> String {
    format!(
        r#"{{"label":"visit","interface":{{"beacon":{{"beacon_id":"b_00"}},"rows":[{}]}},"timeout_ms":120000}}"#,
        rows.join(",")
    )
}

/// `fixture` with `route` sealed for seat 0 and played until its visit ends:
/// the runner, still in the Push.
fn until_the_visit_ends(fixture: World, route: &str) -> Runner {
    let plan = Plan::compile(&playbook(route), &rules()).expect("compiles");
    let mut runner = Runner::new(fixture).unwrap_or_else(|error| panic!("{error}"));
    runner
        .seal_playbook(SEAT, plan)
        .expect("a Lull takes a seal");
    assert!(runner.begin_push().unwrap());
    let mut ended = false;
    let mut ticks: u32 = 0;
    while !ended && ticks < 3_000 {
        runner.step().unwrap();
        ended = runner
            .events()
            .iter()
            .any(|event| event.kind == EventKind::VisitEnded && event.seat == Some(SEAT));
        runner.clear_events();
        ticks += 1;
    }
    assert!(ended, "the visit ends");
    runner
}

/// The world once `rows` have been written at seat 0's core.
fn visit(rows: &[&str]) -> World {
    until_the_visit_ends(world(&[180_000]), &visit_with(rows)).into_world()
}

/// Play the rest of the Push.
fn play_out(runner: &mut Runner) {
    while runner.phase() == MatchPhase::Push {
        let Some(_) = runner.step().unwrap() else {
            break;
        };
        runner.clear_events();
    }
}

/// The rows of the first route step, an interface step.
fn rows_of(compiled: &Plan) -> &[Row] {
    match compiled.route().first().map(|step| &step.action) {
        Some(Action::Interface { rows, .. }) => rows,
        other => panic!("an interface step: {other:?}"),
    }
}

/// What `rows` written at the core cost, row by row.
fn priced(rows: &[&str]) -> Vec<Result<i32, PlanError>> {
    let compiled = Plan::compile(&playbook(&visit_with(rows)), &rules()).expect("compiles");
    let card = compiled.interface_times();
    rows_of(&compiled)
        .iter()
        .map(|row| row.duration_ms(card))
        .collect()
}

fn compile(route: &str) -> Result<(), PlanError> {
    Plan::compile(&playbook(route), &rules()).map(|_| ())
}

fn times() -> gp::v1::rules_table::InterfaceTimes {
    rules().message().interface_times.expect("interface times")
}

fn scouts_of(world: &World) -> u8 {
    world
        .beacons()
        .scout_counts()
        .get(row_of(core(world)))
        .copied()
        .expect("a row")
}

fn mine_of(world: &World) -> MineSettings {
    world
        .beacons()
        .mine_settings(row_of(core(world)))
        .expect("a row")
}

/// How many list rows of `kind` the core holds.
fn listed(world: &World, kind: TargetKind) -> usize {
    let targets = world.targets();
    let beacon = core(world);
    (0..targets.row_count())
        .filter(|row| {
            targets.beacons().get(*row) == Some(&beacon)
                && targets.kinds().get(*row) == Some(&kind.id())
        })
        .count()
}

/// The live scouts homed at the core.
fn scouts_fielded(world: &World) -> usize {
    let units = world.units();
    let beacon = core(world);
    (0..units.homes().len())
        .filter(|row| {
            units.homes().get(*row) == Some(&beacon)
                && units.kinds().get(*row) == Some(&UnitKind::Scout.id())
                && units.hit_points().get(*row).is_some_and(|hp| hp.is_alive())
        })
        .count()
}

/// A one-column area beside the core, as JSON.
fn area(world: &World, dx: i32) -> String {
    let [x, y, z] = core_voxel(world);
    format!(
        r#"{{"min":{{"x":{},"y":{y},"z":{z}}},"max":{{"x":{},"y":{y},"z":{z}}}}}"#,
        x + dx,
        x + dx
    )
}

// --- scout_count -----------------------------------------------------------

#[test]
fn a_written_zero_scout_count_stands_the_scouts_down() {
    let fixture = world(&[180_000]);
    let rich = |mut world: World| {
        world.set_treasury(SEAT, Money::new(100_000));
        world
    };
    // The core is switched to Survey with three scouts, then an edit writes 0:
    // the beacon holds 0 and fields no scout for the rest of the Push.
    let mut runner = until_the_visit_ends(
        rich(fixture.clone()),
        &visit_with(&[
            r#"{"set_mandate":{"survey":{"scout_count":3}}}"#,
            r#"{"set_mandate_settings":{"survey":{"scout_count":0}}}"#,
        ]),
    );
    assert_eq!(scouts_of(runner.world()), 0, "the written 0 is the count");
    let before = scouts_fielded(runner.world());
    play_out(&mut runner);
    assert_eq!(
        scouts_fielded(runner.world()),
        before,
        "no scout is fielded after the count is 0"
    );

    // The same edit leaving the count out leaves 3, and the beacon fields them.
    let mut runner = until_the_visit_ends(
        rich(fixture),
        &visit_with(&[
            r#"{"set_mandate":{"survey":{"scout_count":3}}}"#,
            r#"{"set_mandate_settings":{"roe":"HOLD_FIRE","survey":{}}}"#,
        ]),
    );
    assert_eq!(
        scouts_of(runner.world()),
        3,
        "an omitted count is left alone"
    );
    play_out(&mut runner);
    assert!(
        scouts_fielded(runner.world()) > 0,
        "with the count left at 3 the beacon fields scouts"
    );
}

#[test]
fn a_written_zero_scout_count_in_a_switch_is_one_field_and_sets_zero() {
    let after = visit(&[
        r#"{"set_mandate":{"survey":{"scout_count":3}}}"#,
        r#"{"set_mandate":{"survey":{"scout_count":0}}}"#,
    ]);
    assert_eq!(scouts_of(&after), 0);
    let times = times();
    assert_eq!(
        priced(&[
            r#"{"set_mandate":{"survey":{"scout_count":0}}}"#,
            r#"{"set_mandate_settings":{"survey":{"scout_count":0}}}"#,
            r#"{"set_mandate_settings":{"survey":{}}}"#,
        ]),
        vec![
            Ok(times.switch_mandate_ms + times.edit_settings_base_ms),
            Ok(times.edit_settings_base_ms),
            Ok(0),
        ],
        "a written 0 is one field, an omitted count none"
    );
}

// --- dig_max_depth and pillar_spacing ---------------------------------------

/// Seat 0's core on Mine with `start`, then `edit` written at it: the runner
/// once the visit has ended, and the world as it stood then.
fn mine_edit(start: MineSettings, edit: &str) -> (Runner, World) {
    let mut fixture = world(&[180_000, 180_000]);
    let beacon = core(&fixture);
    fixture.set_writ(beacon, MandateKind::Mine);
    fixture.set_mine_settings(beacon, start);
    let runner = until_the_visit_ends(
        fixture,
        &visit_with(&[&format!(r#"{{"set_mandate_settings":{{"mine":{edit}}}}}"#)]),
    );
    let at_edit = runner.world().clone();
    (runner, at_edit)
}

/// The seam the core holds, as its footprint's columns and its anchor.
fn held_seam(world: &World) -> (Vec<[i32; 3]>, [i32; 2]) {
    let seam = world
        .beacons()
        .seams()
        .get(row_of(core(world)))
        .copied()
        .expect("a row");
    let feature = world
        .features()
        .get(usize::try_from(seam).expect("the core holds a seam"))
        .expect("the seam");
    (
        feature
            .footprint
            .iter()
            .map(|column| [column.x, column.y, column.top])
            .collect(),
        feature.anchor,
    )
}

/// Whether the voxel at `at` was ore in `before` and is not in `after`.
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

/// How many voxels below the seam's original top were dug between `before`
/// and `after`.
fn dug_below_the_rim(before: &World, after: &World) -> usize {
    let (columns, _) = held_seam(before);
    columns
        .iter()
        .flat_map(|[x, y, top]| (1..6).map(move |depth| [*x, *y, top - depth]))
        .filter(|at| dug(before, after, *at))
        .count()
}

#[test]
fn a_written_zero_dig_depth_keeps_the_rim_only() {
    let deep = MineSettings {
        dig_max_depth: 3,
        ..MineSettings::default()
    };
    let (mut runner, at_edit) = mine_edit(deep, r#"{"dig_max_depth":0}"#);
    assert_eq!(
        mine_of(&at_edit).dig_max_depth,
        0,
        "the written 0 is the depth"
    );
    play_out(&mut runner);
    assert_eq!(
        dug_below_the_rim(&at_edit, runner.world()),
        0,
        "after the edit nothing below the rim is dug"
    );

    // Left out, the depth stays 3 and the beacon digs below the rim.
    let (mut runner, at_edit) = mine_edit(deep, r#"{"seam_choice":"NEAREST"}"#);
    assert_eq!(
        mine_of(&at_edit).dig_max_depth,
        3,
        "an omitted depth is left alone"
    );
    play_out(&mut runner);
    assert!(
        dug_below_the_rim(&at_edit, runner.world()) > 0,
        "at depth 3 the beacon digs below the rim"
    );
}

/// How many pillar columns' tops at `spacing` were dug between `before` and
/// `after`.
fn pillars_dug(before: &World, after: &World, spacing: i32) -> usize {
    let (columns, [ax, ay]) = held_seam(before);
    columns
        .iter()
        .filter(|[x, y, _]| (x - ax).rem_euclid(spacing) == 0 && (y - ay).rem_euclid(spacing) == 0)
        .filter(|at| dug(before, after, **at))
        .count()
}

#[test]
fn a_written_zero_pillar_spacing_digs_the_pillars_too() {
    let pillared = MineSettings {
        pillar_spacing: 2,
        ..MineSettings::default()
    };
    let (mut runner, at_edit) = mine_edit(pillared, r#"{"pillar_spacing":0}"#);
    assert_eq!(
        mine_of(&at_edit).pillar_spacing,
        0,
        "the written 0 is the spacing"
    );
    play_out(&mut runner);
    assert!(
        pillars_dug(&at_edit, runner.world(), 2) > 0,
        "with no pillars, a column the old spacing kept is dug"
    );

    // Left out, the spacing stays 2 and no pillar is dug.
    let (mut runner, at_edit) = mine_edit(pillared, r#"{"seam_choice":"NEAREST"}"#);
    assert_eq!(
        mine_of(&at_edit).pillar_spacing,
        2,
        "an omitted spacing is left alone"
    );
    play_out(&mut runner);
    assert_eq!(pillars_dug(&at_edit, runner.world(), 2), 0);
}

#[test]
fn a_written_zero_mine_setting_is_one_field() {
    let times = times();
    assert_eq!(
        priced(&[
            r#"{"set_mandate_settings":{"mine":{"dig_max_depth":0}}}"#,
            r#"{"set_mandate_settings":{"mine":{"dig_max_depth":0,"pillar_spacing":0}}}"#,
            r#"{"set_mandate":{"mine":{"pillar_spacing":0}}}"#,
        ]),
        vec![
            Ok(times.edit_settings_base_ms),
            Ok(times.edit_settings_base_ms + times.edit_settings_per_field_ms),
            Ok(times.switch_mandate_ms + times.edit_settings_base_ms),
        ]
    );
    // A switch starts from the defaults, so its written 0s set 0.
    let after = visit(&[
        r#"{"set_mandate":{"mine":{"dig_max_depth":4,"pillar_spacing":3}}}"#,
        r#"{"set_mandate":{"mine":{"dig_max_depth":0,"pillar_spacing":0}}}"#,
    ]);
    assert_eq!(mine_of(&after), MineSettings::default());
}

// --- the four lists ---------------------------------------------------------

#[test]
fn a_clear_empties_the_protected_areas() {
    let fixture = world(&[180_000]);
    let one = area(&fixture, 4);
    let written =
        format!(r#"{{"set_mandate_settings":{{"build":{{"protected_areas":[{one}]}}}}}}"#);
    let after = visit(&[
        &written,
        r#"{"set_mandate_settings":{"build":{"clear_protected_areas":true}}}"#,
    ]);
    assert_eq!(
        listed(&after, TargetKind::Protected),
        0,
        "the list is empty"
    );
    let after = visit(&[
        &written,
        r#"{"set_mandate_settings":{"build":{"terraform":"NONE"}}}"#,
    ]);
    assert_eq!(
        listed(&after, TargetKind::Protected),
        1,
        "a row silent about the list leaves it"
    );
}

#[test]
fn a_clear_empties_the_probe_areas() {
    let fixture = world(&[180_000]);
    let one = area(&fixture, 4);
    let two = area(&fixture, -4);
    let written = format!(r#"{{"set_mandate":{{"survey":{{"probe_areas":[{one},{two}]}}}}}}"#);
    let after = visit(&[
        &written,
        r#"{"set_mandate_settings":{"survey":{"clear_probe_areas":true}}}"#,
    ]);
    assert_eq!(listed(&after, TargetKind::Probe), 0, "the list is empty");
    let after = visit(&[
        &written,
        r#"{"set_mandate_settings":{"survey":{"scout_count":1}}}"#,
    ]);
    assert_eq!(listed(&after, TargetKind::Probe), 2);
}

#[test]
fn a_clear_empties_the_protect_list_and_ends_the_escort() {
    let defend = r#"{"set_mandate":{"defend":{"protect":[{"my_commander":{}},{"beacon":{"beacon_id":"b_00"}}],"engagement_radius_pct":80,"pursue":"NEVER"}}}"#;
    let after = visit(&[defend]);
    let beacon = core(&after);
    let row = after.defend().row_of(beacon).expect("a Defend row");
    assert!(row.settings.escort, "the list escorts the commander");
    assert_eq!(
        after
            .defend()
            .protected()
            .iter()
            .filter(|(by, _)| *by == beacon)
            .count(),
        1
    );

    let after = visit(&[
        defend,
        r#"{"set_mandate_settings":{"defend":{"clear_protect":true}}}"#,
    ]);
    let beacon = core(&after);
    let row = after.defend().row_of(beacon).expect("a Defend row");
    assert!(!row.settings.escort, "the commander is no longer escorted");
    assert!(
        after
            .defend()
            .protected()
            .iter()
            .all(|(by, _)| *by != beacon),
        "no beacon is protected"
    );
    assert_eq!(row.settings.engagement_pct, 80, "the rest is left alone");
}

#[test]
fn a_clear_empties_the_avoid_areas() {
    let fixture = world(&[180_000]);
    let [x, y, z] = core_voxel(&fixture);
    let one = area(&fixture, 4);
    let attack = format!(
        concat!(
            r#"{{"set_mandate":{{"attack":{{"target":{{"voxel":{{"x":{x},"y":{y},"z":{z}}}}},"#,
            r#""retarget":"NONE","staging":{{"x":{x},"y":{y},"z":{z}}},"#,
            r#""launch":{{"segment_ms":600000}},"after_action":"RETURN_HOME","waves":1,"#,
            r#""avoid_areas":[{one}]}}}}}}"#
        ),
        x = x,
        y = y,
        z = z,
        one = one
    );
    let after = visit(&[&attack]);
    assert_eq!(after.attack().avoid_of(core(&after)).count(), 1);
    let after = visit(&[
        &attack,
        r#"{"set_mandate_settings":{"attack":{"clear_avoid_areas":true}}}"#,
    ]);
    assert_eq!(
        after.attack().avoid_of(core(&after)).count(),
        0,
        "the force avoids nothing"
    );
    let after = visit(&[
        &attack,
        r#"{"set_mandate_settings":{"attack":{"waves":2}}}"#,
    ]);
    assert_eq!(after.attack().avoid_of(core(&after)).count(), 1);
}

#[test]
fn a_clear_is_one_field_of_its_edit() {
    let times = times();
    let base = times.edit_settings_base_ms;
    let per = times.edit_settings_per_field_ms;
    assert_eq!(
        priced(&[
            r#"{"set_mandate_settings":{"build":{"clear_protected_areas":true}}}"#,
            r#"{"set_mandate_settings":{"survey":{"clear_probe_areas":true,"scout_count":0}}}"#,
            r#"{"set_mandate_settings":{"defend":{"clear_protect":true}}}"#,
            r#"{"set_mandate_settings":{"attack":{"clear_avoid_areas":true,"waves":1}}}"#,
            r#"{"set_mandate_settings":{"build":{"clear_protected_areas":false}}}"#,
        ]),
        vec![Ok(base), Ok(base + per), Ok(base), Ok(base + per), Ok(0)],
        "a clear is one field; false is no field"
    );
}

#[test]
fn a_clear_beside_a_list_is_refused() {
    let fixture = world(&[180_000]);
    let one = area(&fixture, 4);
    for (row, list) in [
        (
            format!(r#"{{"build":{{"clear_protected_areas":true,"protected_areas":[{one}]}}}}"#),
            "protected_areas",
        ),
        (
            format!(r#"{{"survey":{{"clear_probe_areas":true,"probe_areas":[{one}]}}}}"#),
            "probe_areas",
        ),
        (
            r#"{"defend":{"clear_protect":true,"protect":[{"my_commander":{}}]}}"#.to_owned(),
            "protect",
        ),
        (
            format!(r#"{{"attack":{{"clear_avoid_areas":true,"avoid_areas":[{one}]}}}}"#),
            "avoid_areas",
        ),
    ] {
        assert_eq!(
            compile(&visit_with(&[&format!(
                r#"{{"set_mandate_settings":{row}}}"#
            )])),
            Err(PlanError::ClearRefused {
                list,
                why: ClearRefusal::BesideAList,
            }),
            "{row}"
        );
    }
}

#[test]
fn a_clear_in_settings_that_start_from_nothing_is_refused() {
    let nothing = |list| {
        Err(PlanError::ClearRefused {
            list,
            why: ClearRefusal::NothingToClear,
        })
    };
    for (arm, list) in [
        (
            r#"{"build":{"clear_protected_areas":true}}"#,
            "protected_areas",
        ),
        (r#"{"survey":{"clear_probe_areas":true}}"#, "probe_areas"),
        (r#"{"defend":{"clear_protect":true}}"#, "protect"),
        (r#"{"attack":{"clear_avoid_areas":true}}"#, "avoid_areas"),
    ] {
        assert_eq!(
            compile(&visit_with(&[&format!(r#"{{"set_mandate":{arm}}}"#)])),
            nothing(list),
            "a switch: {arm}"
        );
        let deploy = format!(
            r#"{{"label":"deploy","place_beacon":{{"at":{{"voxel":{{"x":27,"y":356,"z":29}}}},"initial":{{"mandate":{arm}}}}},"timeout_ms":30000,"on_fail":{{"action":"SKIP"}}}}"#
        );
        assert_eq!(compile(&deploy), nothing(list), "a deploy: {arm}");
    }
}
