// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The verifier and the sim's compile agree, held by a table (S3's `vfyq2`,
//! fix pass 2, item C1).
//!
//! "What qualifies at FULL compiles" was held by `fuzz.rs` over one-node
//! edits of two bases. This holds it by construction of the table: every
//! `pharmakos_sim::interpreter::PlanError` variant is classed by `classify`,
//! a `match` with no wildcard, so a variant the sim adds does not compile here
//! until it is classed. A reachable variant names the codes the verifier
//! refuses that shape with; an unreachable one says why no decoded playbook
//! can raise it. Each reachable variant has a text the verifier refuses with
//! one of its codes AND that `Plan::compile`, the verifier bypassed, refuses
//! with that variant. A text the verifier accepted would be a defect in the
//! verifier, fixed there, never a row deleted.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml's allow-expect-in-tests only recognises #[test] functions, not an \
              integration test's helpers. A panic is this file's failure report."
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::gp::v1::Playbook;
use pharmakos_proto::json;
use pharmakos_sim::interpreter::{Plan, PlanError};
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::tables::SeatId;
use pharmakos_verifier::{Input, Scope, verify};

/// How a variant stands to the verifier.
enum Row {
    /// The verifier refuses the shape first, with one of these codes.
    Refused(&'static [&'static str]),
    /// No decoded playbook raises it, for this reason.
    Unreachable(&'static str),
}

/// Every variant, by name and row. No wildcard: a variant the sim adds does
/// not compile here until it is classed.
fn classify(error: &PlanError) -> (&'static str, Row) {
    match error {
        PlanError::MissingBlock(_) => (
            "MissingBlock",
            Row::Refused(&["E0101", "E0106", "E0111", "E0206", "E0209"]),
        ),
        PlanError::UnsetEnum(_) => ("UnsetEnum", Row::Refused(&["E0111", "E0201"])),
        PlanError::UnknownEnum { .. } => ("UnknownEnum", Row::Refused(&["E0018"])),
        PlanError::StepWithoutAction => ("StepWithoutAction", Row::Refused(&["E0103"])),
        PlanError::UnknownBlueprint(_) => ("UnknownBlueprint", Row::Refused(&["E0506"])),
        PlanError::BlueprintNeverStands(_) => ("BlueprintNeverStands", Row::Refused(&["E0514"])),
        PlanError::NoRadius(_) => ("NoRadius", Row::Refused(&["E0101"])),
        PlanError::WindowUnderVisibleNow { .. } => {
            ("WindowUnderVisibleNow", Row::Refused(&["E0210"]))
        }
        PlanError::NegativeDuration(_) => ("NegativeDuration", Row::Refused(&["E0109"])),
        PlanError::WaitWithoutTimeout => ("WaitWithoutTimeout", Row::Refused(&["E0302"])),
        PlanError::BackwardJump { .. } => ("BackwardJump", Row::Refused(&["E0301"])),
        PlanError::VoxelOutOfRange { .. } => (
            "VoxelOutOfRange",
            Row::Unreachable(
                "every map extent fits a voxel coordinate (an i16): a voxel outside the map is \
                 VoxelOutOfMap (E0402) first, and an area's midpoint lies between two voxels \
                 inside it",
            ),
        ),
        PlanError::NegativeRate(_) => (
            "NegativeRate",
            Row::Unreachable("a rate is a rules-table row, not something a playbook writes"),
        ),
        PlanError::RowTooLong => (
            "RowTooLong",
            Row::Unreachable(
                "a row's time is the rules table's rates times a count the size budget bounds \
                 (E0110): it stays far inside an i32 of milliseconds",
            ),
        ),
        PlanError::AttackTargetNotEnemy => {
            ("AttackTargetNotEnemy", Row::Refused(&["E0414", "E0415"]))
        }
        PlanError::CommanderSeatOutOfRange { .. } => {
            ("CommanderSeatOutOfRange", Row::Refused(&["E0419"]))
        }
        PlanError::ForceBelowOutOfRange { .. } => {
            ("ForceBelowOutOfRange", Row::Refused(&["E0510"]))
        }
        PlanError::EngagementOutOfRange { .. } => {
            ("EngagementOutOfRange", Row::Refused(&["E0510"]))
        }
        PlanError::ScoutsOutOfRange { .. } => ("ScoutsOutOfRange", Row::Refused(&["E0508"])),
        PlanError::RetreatOutOfRange { .. } => ("RetreatOutOfRange", Row::Refused(&["E0501"])),
        PlanError::RotationOutOfRange { .. } => ("RotationOutOfRange", Row::Refused(&["E0505"])),
        PlanError::WallBoxOffAWall { .. } => ("WallBoxOffAWall", Row::Refused(&["E0509"])),
        PlanError::AreaInsideOut => ("AreaInsideOut", Row::Refused(&["E0406"])),
        PlanError::RepairThresholdOutOfRange { .. } => {
            ("RepairThresholdOutOfRange", Row::Refused(&["E0501"]))
        }
        PlanError::WallWithoutBox => ("WallWithoutBox", Row::Refused(&["E0101"])),
        PlanError::WallBoxInsideOut => ("WallBoxInsideOut", Row::Refused(&["E0406"])),
        PlanError::WallTurned { .. } => ("WallTurned", Row::Refused(&["E0511"])),
        PlanError::WallAnchorOutsideBox => ("WallAnchorOutsideBox", Row::Refused(&["E0417"])),
        PlanError::VoxelOutOfMap { .. } => ("VoxelOutOfMap", Row::Refused(&["E0402"])),
        PlanError::UnknownLabel(_) => ("UnknownLabel", Row::Refused(&["E0208", "E0304", "E0305"])),
        PlanError::UnknownHandler(_) => ("UnknownHandler", Row::Refused(&["E0207"])),
        PlanError::UnknownBeacon(_) => ("UnknownBeacon", Row::Refused(&["E0401"])),
        PlanError::UnknownFeature(_) => ("UnknownFeature", Row::Refused(&["E0412"])),
        PlanError::MisplacedTarget(_) => (
            "MisplacedTarget",
            Row::Refused(&[
                "E0407", "E0408", "E0409", "E0410", "E0411", "E0413", "E0420",
            ]),
        ),
        PlanError::CooldownTooShort { .. } => ("CooldownTooShort", Row::Refused(&["E0108"])),
        PlanError::FiresOutOfRange { .. } => ("FiresOutOfRange", Row::Refused(&["E0107"])),
        PlanError::ConditionTooDeep { .. } => ("ConditionTooDeep", Row::Refused(&["E0204"])),
        PlanError::ConditionTooBig { .. } => ("ConditionTooBig", Row::Refused(&["E0205"])),
        PlanError::TooManySteps { .. } => ("TooManySteps", Row::Refused(&["E0110"])),
        PlanError::TooManyHandlers { .. } => ("TooManyHandlers", Row::Refused(&["E0113"])),
        PlanError::NotAtThisStage { .. } => (
            "NotAtThisStage",
            Row::Refused(&[
                "E0009", "E0010", "E0011", "E0012", "E0013", "E0014", "E0015", "E0016", "E0017",
            ]),
        ),
    }
}

/// How many variants `PlanError` has: `classify`'s arms. A variant added to
/// the sim fails `classify` to compile; this count then fails the test until
/// the variant has its text or its unreachable sample below.
const VARIANTS: usize = 41;

/// One instance of each unreachable variant, so its row is asserted.
fn unreachable_samples() -> Vec<PlanError> {
    vec![
        PlanError::VoxelOutOfRange {
            axis: "x",
            value: 40_000,
        },
        PlanError::NegativeRate("switch_mandate"),
        PlanError::RowTooLong,
    ]
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the workspace root")
        .to_path_buf()
}

fn case(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("cases")
        .join(format!("{name}.json"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// A playbook around one route and one fallback, otherwise clean.
fn playbook(route: &str) -> String {
    format!(
        r#"{{"schema_version":{{"major":1}},"meta":{{"title":"case","author_kind":"HUMAN"}},"kind":"PLAYBOOK","declarative":{{"route":{route}}},"on_death":{{"on_respawn":"CONTINUE","max_deaths_before_fallback":1}},"fallback":{{"hold":{{"at":{{"beacon_anchor":{{"safest":{{}}}}}}}}}}}}"#
    )
}

/// A route of one `place_beacon` whose initial mandate is `mandate`.
fn placing(mandate: &str) -> String {
    playbook(&format!(
        r#"[{{"label":"s1","place_beacon":{{"at":{{"voxel":{{"x":96,"y":11,"z":62}}}},"initial":{{"mandate":{mandate}}}}}}}]"#
    ))
}

/// A text for each reachable variant: the committed case that refuses it,
/// or one written here where no case reached it.
fn texts() -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = [
        "e0018_an_enum_number_this_build_does_not_name",
        "e0101_missing_block",
        "e0101_a_wall_without_its_box",
        "e0101_an_enemy_predicate_without_its_radius",
        "e0103_step_without_action",
        "e0107_max_fires_out_of_range",
        "e0108_cooldown_below_minimum",
        "e0109_negative_duration",
        "e0111_a_predicate_with_its_role_unset",
        "e0113_more_handlers_than_this_build_compiles",
        "e0204_condition_too_deep",
        "e0205_condition_too_many_nodes",
        "e0207_unknown_handler_id",
        "e0208_unknown_step_label",
        "e0210_a_window_under_visible_now",
        "e0301_jump_not_forward",
        "e0302_wait_without_timeout",
        "e0402_voxel_off_the_map",
        "e0406_area_inside_out",
        "e0407_covering_outside_a_place_step",
        "e0415_an_attack_target_ranking_own_beacons",
        "e0417_a_wall_anchor_outside_its_box",
        "e0501_setting_percentage_out_of_range",
        "e0505_rotation_out_of_range",
        "e0506_a_predicate_naming_no_blueprint",
        "e0508_more_scouts_than_a_beacon_holds",
        "e0509_a_box_on_a_target_that_is_not_a_wall",
        "e0510_an_engagement_radius_past_the_sphere",
        "e0511_a_turned_wall",
        "e0514_a_predicate_about_a_wall",
        "e0010_a_blueprint_other_than_the_generator",
    ]
    .into_iter()
    .map(|name| (name.to_owned(), case(name)))
    .collect();
    let steps: Vec<String> = (0..257)
        .map(|index| format!(r#"{{"label":"s{index}","hold":{{"ms":1}}}}"#))
        .collect();
    found.push((
        "257 route steps".to_owned(),
        playbook(&format!("[{}]", steps.join(","))),
    ));
    found.push((
        "an enemy commander past any seat".to_owned(),
        placing(
            r#"{"roe":"FREE_FIRE","attack":{"target":{"enemy_commander":{"seat":300}},"retarget":"NONE","staging":{"x":100,"y":20,"z":58},"launch":{"segment_ms":0},"after_action":"RETURN_HOME","waves":1}}"#,
        ),
    ));
    found.push((
        "a retreat below 1 per cent".to_owned(),
        placing(
            r#"{"roe":"FREE_FIRE","attack":{"target":{"beacon":{"beacon_id":"e_01"}},"retarget":"NONE","staging":{"x":100,"y":20,"z":58},"launch":{"segment_ms":0},"after_action":"RETURN_HOME","waves":1,"retreat":{"force_below_pct":0}}}"#,
        ),
    ));
    found.push((
        "a repair threshold past 100".to_owned(),
        placing(r#"{"roe":"RETURN_FIRE","build":{"repair_threshold_pct":150,"terraform":"NONE"}}"#),
    ));
    found.push((
        "a wall box upside down".to_owned(),
        placing(
            r#"{"roe":"RETURN_FIRE","build":{"targets":[{"blueprint_id":"wall","anchor":{"voxel":{"x":100,"y":11,"z":61}},"order":1,"wall_box":{"min":{"x":100,"y":11,"z":63},"max":{"x":102,"y":11,"z":60}}}],"repair_threshold_pct":50,"terraform":"NONE"}}"#,
        ),
    ));
    found.push((
        "a beacon id that names no beacon".to_owned(),
        playbook(r#"[{"label":"s1","move":{"to":{"beacon_anchor":{"beacon_id":"zz"}}}}]"#),
    ));
    found.push((
        "a feature id that names no feature".to_owned(),
        playbook(r#"[{"label":"s1","place_beacon":{"at":{"covering":{"feature_id":"zz"}}}}]"#),
    ));
    found
}

#[test]
fn every_plan_error_the_compile_raises_the_verifier_refuses_first() {
    let rules = RulesTable::load(&root().join("rules").join("rules.v1.json"))
        .expect("the committed rules table");
    let scope = Scope::new(SeatId::new(0), SeatEconomy::default());
    let mut covered: BTreeSet<&'static str> = BTreeSet::new();
    let mut failures = Vec::new();
    for (name, text) in texts() {
        let playbook: Playbook =
            json::decode(&text).unwrap_or_else(|error| panic!("{name}: decodes: {error}"));
        let Err(error) = Plan::compile(&playbook, &rules) else {
            failures.push(format!("{name}: the compile accepts it"));
            continue;
        };
        let (variant, row) = classify(&error);
        covered.insert(variant);
        let codes = match row {
            Row::Refused(codes) => codes,
            Row::Unreachable(why) => {
                failures.push(format!(
                    "{name}: raised {variant}, classed unreachable: {why}"
                ));
                continue;
            }
        };
        let input = Input::new(text.as_bytes(), &[], &scope, &rules)
            .unwrap_or_else(|error| panic!("{name}: the input: {error}"));
        let report = verify(&input, Depth::Full);
        if report.qualifies {
            failures.push(format!(
                "{name}: the verifier qualifies what the compile refuses ({error:?})"
            ));
        }
        if !report
            .diagnostics
            .iter()
            .any(|diagnostic| codes.contains(&diagnostic.code.as_str()))
        {
            failures.push(format!(
                "{name}: {variant} is refused by none of {codes:?}; the report raised {:?}",
                report
                    .diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.code.as_str())
                    .collect::<Vec<_>>()
            ));
        }
    }
    for sample in unreachable_samples() {
        let (variant, row) = classify(&sample);
        assert!(
            matches!(row, Row::Unreachable(_)),
            "{variant} is sampled as unreachable"
        );
        covered.insert(variant);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(
        covered.len(),
        VARIANTS,
        "every PlanError variant has a text or an unreachable sample: {covered:?}"
    );
}
