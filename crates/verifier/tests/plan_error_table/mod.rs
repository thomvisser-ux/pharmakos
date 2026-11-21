// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `PlanError` table, shared (S3's `vfyq2`, fix pass 3, item M5):
//! `plan_errors.rs` holds every variant's text to the verifier, and
//! `fuzz.rs` seeds its two-node probe with the same texts and counts, per
//! variant, the generated cases that reach it with the verifier bypassed.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml's allow-expect-in-tests only recognises #[test] functions, not an integration test's helpers. A panic is this module's failure report."
)]

use std::path::{Path, PathBuf};

use pharmakos_sim::interpreter::PlanError;

/// How a variant stands to the verifier.
pub(crate) enum Row {
    /// The verifier refuses the shape first, with one of these codes.
    Refused(&'static [&'static str]),
    /// No decoded playbook raises it, for this reason.
    Unreachable(&'static str),
}

/// Every variant, by name and row. No wildcard: a variant the sim adds does
/// not compile here until it is classed.
pub(crate) fn classify(error: &PlanError) -> (&'static str, Row) {
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
                "the verifier's `Input::new` refuses a rules table under which a row a \
                 playbook within the size budget can write passes the sim's i32 \
                 (RulesGap::RowPastTheSim, fix pass 4): under every table it takes, every row \
                 fits, so a qualifying playbook never raises it (fuzz.rs's \
                 a_table_whose_rows_could_pass_the_sims_i32_is_refused and the extremes \
                 probe show it)",
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
        // The sim's own `MAX_ROUTE_STEPS`, read by the verifier (fix pass 3,
        // item M4): the size budget (E0110) is a rules value, and a table with
        // a budget past the ceiling let a 257-step route qualify.
        PlanError::TooManySteps { .. } => ("TooManySteps", Row::Refused(&["E0114"])),
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
pub(crate) const VARIANTS: usize = 41;

/// One instance of each unreachable variant, so its row is asserted.
pub(crate) fn unreachable_samples() -> Vec<PlanError> {
    vec![
        PlanError::VoxelOutOfRange {
            axis: "x",
            value: 40_000,
        },
        PlanError::NegativeRate("switch_mandate"),
        PlanError::RowTooLong,
    ]
}

pub(crate) fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the workspace root")
        .to_path_buf()
}

pub(crate) fn case(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("cases")
        .join(format!("{name}.json"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// A playbook around one route and one fallback, otherwise clean.
pub(crate) fn playbook(route: &str) -> String {
    format!(
        r#"{{"schema_version":{{"major":1}},"meta":{{"title":"case","author_kind":"HUMAN"}},"kind":"PLAYBOOK","declarative":{{"route":{route}}},"on_death":{{"on_respawn":"CONTINUE","max_deaths_before_fallback":1}},"fallback":{{"hold":{{"at":{{"beacon_anchor":{{"safest":{{}}}}}}}}}}}}"#
    )
}

/// A route of one `place_beacon` whose initial mandate is `mandate`.
pub(crate) fn placing(mandate: &str) -> String {
    playbook(&format!(
        r#"[{{"label":"s1","place_beacon":{{"at":{{"voxel":{{"x":96,"y":11,"z":62}}}},"initial":{{"mandate":{mandate}}}}}}}]"#
    ))
}

/// A text for each reachable variant: the committed case that refuses it,
/// or one written here where no case reached it.
pub(crate) fn texts() -> Vec<(String, String)> {
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

/// One instance of every `PlanError` variant, its fields filled with any
/// value: what a count by variant walks. `classify` names each, and the
/// test that uses this asserts it holds [`VARIANTS`] distinct names.
pub(crate) fn every_variant() -> Vec<PlanError> {
    let mut all = vec![
        PlanError::MissingBlock("x"),
        PlanError::UnsetEnum("x"),
        PlanError::UnknownEnum {
            field: "x",
            value: 0,
        },
        PlanError::StepWithoutAction,
        PlanError::UnknownBlueprint(String::new()),
        PlanError::BlueprintNeverStands(String::new()),
        PlanError::NoRadius("x"),
        PlanError::WindowUnderVisibleNow { found: 0 },
        PlanError::NegativeDuration("x"),
        PlanError::WaitWithoutTimeout,
        PlanError::BackwardJump { from: 0, to: 0 },
        PlanError::AttackTargetNotEnemy,
        PlanError::CommanderSeatOutOfRange { found: 0 },
        PlanError::ForceBelowOutOfRange { found: 0 },
        PlanError::EngagementOutOfRange { found: 0 },
        PlanError::ScoutsOutOfRange { found: 0, most: 0 },
        PlanError::RetreatOutOfRange { found: 0 },
        PlanError::RotationOutOfRange { found: 0 },
        PlanError::WallBoxOffAWall {
            blueprint: String::new(),
        },
        PlanError::AreaInsideOut,
        PlanError::RepairThresholdOutOfRange { found: 0 },
        PlanError::WallWithoutBox,
        PlanError::WallBoxInsideOut,
        PlanError::WallTurned { found: 0 },
        PlanError::WallAnchorOutsideBox,
        PlanError::VoxelOutOfMap {
            axis: "x",
            value: 0,
            limit: 0,
        },
        PlanError::UnknownLabel(String::new()),
        PlanError::UnknownHandler(String::new()),
        PlanError::UnknownBeacon(String::new()),
        PlanError::UnknownFeature(String::new()),
        PlanError::MisplacedTarget("x"),
        PlanError::CooldownTooShort { found: 0, least: 0 },
        PlanError::FiresOutOfRange { found: 0, most: 0 },
        PlanError::ConditionTooDeep { limit: 0 },
        PlanError::ConditionTooBig { limit: 0 },
        PlanError::TooManySteps { found: 0, limit: 0 },
        PlanError::TooManyHandlers { found: 0, limit: 0 },
        PlanError::NotAtThisStage {
            construct: "x",
            stage: "x",
        },
    ];
    all.extend(unreachable_samples());
    all
}

/// Where a variant's limit comes from, for the verifier's refusal and the
/// compile's alike (S3's `vfyq2`, fix pass 3, item M4): the C1 table's column
/// on the rules table.
pub(crate) enum Rules {
    /// Neither the verifier's refusal nor the compile's reads the rules table:
    /// the playbook's shape, a spec number or one of the sim's own ceilings,
    /// which the verifier reads from the sim.
    Independent,
    /// Both read this one rules-table value, so they agree under any table.
    SameValue(&'static str),
}

/// Every variant's rules column. No wildcard: a variant the sim adds does not
/// compile here until it is classed.
pub(crate) fn rules_of(error: &PlanError) -> Rules {
    match error {
        // Two of its arms read the rules table, `rules.interface_times` and
        // `rules.verifier` (fix pass 4, item 1): the verifier reads the same
        // two blocks and refuses a table missing either (RulesGap::MissingBlock)
        // before it reads a playbook. The rest of its arms are the playbook's.
        PlanError::MissingBlock(_) => {
            Rules::SameValue("the rules table's interface_times and verifier blocks")
        }
        PlanError::UnsetEnum(_)
        | PlanError::UnknownEnum { .. }
        | PlanError::StepWithoutAction
        | PlanError::UnknownBlueprint(_)
        | PlanError::BlueprintNeverStands(_)
        | PlanError::NoRadius(_)
        | PlanError::WindowUnderVisibleNow { .. }
        | PlanError::NegativeDuration(_)
        | PlanError::WaitWithoutTimeout
        | PlanError::BackwardJump { .. }
        | PlanError::VoxelOutOfRange { .. }
        | PlanError::AttackTargetNotEnemy
        | PlanError::CommanderSeatOutOfRange { .. }
        | PlanError::ForceBelowOutOfRange { .. }
        | PlanError::EngagementOutOfRange { .. }
        | PlanError::ScoutsOutOfRange { .. }
        | PlanError::RetreatOutOfRange { .. }
        | PlanError::RotationOutOfRange { .. }
        | PlanError::WallBoxOffAWall { .. }
        | PlanError::AreaInsideOut
        | PlanError::RepairThresholdOutOfRange { .. }
        | PlanError::WallWithoutBox
        | PlanError::WallBoxInsideOut
        | PlanError::WallTurned { .. }
        | PlanError::WallAnchorOutsideBox
        | PlanError::UnknownLabel(_)
        | PlanError::UnknownHandler(_)
        | PlanError::UnknownBeacon(_)
        | PlanError::UnknownFeature(_)
        | PlanError::MisplacedTarget(_)
        | PlanError::ConditionTooDeep { .. }
        | PlanError::ConditionTooBig { .. }
        | PlanError::TooManySteps { .. }
        | PlanError::TooManyHandlers { .. }
        | PlanError::NotAtThisStage { .. } => Rules::Independent,
        PlanError::VoxelOutOfMap { .. } => Rules::SameValue("map.size_x, size_y, size_z"),
        PlanError::NegativeRate(_) => Rules::SameValue("the interface rates"),
        PlanError::RowTooLong => Rules::SameValue(
            "the interface rates against the size budget (RulesGap::RowPastTheSim)",
        ),
        PlanError::CooldownTooShort { .. } => Rules::SameValue("verifier.handler_cooldown_min_ms"),
        PlanError::FiresOutOfRange { .. } => Rules::SameValue("verifier.max_fires_max"),
    }
}
