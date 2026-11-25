// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Which settings say what a 0 does (S3's `zerotitle`, decisions-log item
//! 184 (9)): the sweep, held to one table.
//!
//! Every integer a vocabulary entry offers whose bounds admit a 0 is listed
//! below, with what the sim does at that 0 and the title it gets, or why it
//! gets none. A title goes only where the sim reads a 0 as turning what the
//! setting governs off; a 0 that is just a number (a comparison's value, a
//! count, a time at the start of the segment) has none. The sim's half of
//! each title is held by `crates/gateway/tests/zero_titles.rs`, which drives
//! the sim at that 0 and names this table's titled pointers, so a title added
//! here without a check there fails there.
//!
//! The prose says each titled 0 in the same words, from the same string-table
//! entry, as `one_entry_two_readers` below holds.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "clippy.toml sets allow-expect-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules, not an integration test's helpers. A panic is this file's failure report: these are panic lints, not determinism lints (AGENTS.md section 5)."
)]

use pharmakos_plan_core::shape::Shape;
use pharmakos_plan_core::strings as s;
use pharmakos_plan_core::vocabulary::{common_parameters, vocabulary};

/// The settings sweep: each `(entry, pointer)` of an integer parameter whose
/// bounds admit a 0, and its title, `None` where a 0 is just a number. An
/// entry of `""` is a common setting, a parameter of `MandateSettings`
/// itself. A predicate's parameter is named by its entry's name.
///
/// | setting | what the sim does at 0 | title |
/// |---|---|---|
/// | `retreat_hp_pct` | no unit ever falls below 0 %: retreat off (`below_retreat`) | "retreat off" |
/// | `build.repair_threshold_pct` | no structure, beacon or cracked wall is repair work; units and the commander still are (`repair_work`) | "never repairing structures, beacons or walls" |
/// | `defend.picket_scouts` | the Defend mandate asks for no scout (`mandate.rs`) | "no picket" |
/// | `attack.launch.force_size` | a force of none is one, launched at once without gathering (`wanted_force`, `by_size`) | "launching at once with its first raider, ungathered" |
/// | `attack.launch.segment_ms` | launches once the segment has run 0 s, at once | none: a time like any other |
/// | `attack.retreat.after_launch_ms` | breaks off 0 s after the launch | none: a time like any other |
/// | `survey.scout_count` | a held 0 fields no scout (the Survey mandate asks for none) | "no scouts" |
/// | `mine.dig_max_depth` | a held 0 takes no voxel below the seam's top | "digging the exposed rim only" |
/// | `mine.pillar_spacing` | a held 0 leaves no pillar (`pillar`) | "leaving no pillars" |
/// | a predicate's percentage, count, `$`, `kW` or window | a comparison against 0 | none: a number like any other |
const SWEEP: &[(&str, &str, Option<&str>)] = &[
    ("", "/retreat_hp_pct", Some(s::RETREAT_OFF)),
    ("build", "/repair_threshold_pct", Some(s::REPAIR_NEVER)),
    ("defend", "/picket_scouts", Some(s::PICKET_NONE)),
    ("attack", "/launch/force_size", Some(s::LAUNCH_FIRST_RAIDER)),
    ("attack", "/launch/segment_ms", None),
    ("attack", "/retreat/after_launch_ms", None),
    ("survey", "/scout_count", Some(s::SCOUTS_NONE)),
    ("mine", "/dig_max_depth", Some(s::DIG_RIM_ONLY)),
    ("mine", "/pillar_spacing", Some(s::PILLARS_NONE)),
];

/// Whether an integer shape admits a 0.
fn admits_zero(shape: &Shape) -> bool {
    match shape {
        Shape::Integer { min, max, .. } => {
            min.is_none_or(|least| least <= 0) && max.is_none_or(|most| most >= 0)
        }
        Shape::Choice { .. } | Shape::Reference { .. } => false,
    }
}

/// Every mandate setting and common setting the vocabulary offers whose
/// bounds admit a 0 is in [`SWEEP`] with the title the table gives it, and
/// nothing in [`SWEEP`] is missing from the vocabulary; and no predicate's
/// parameter carries a title.
#[test]
fn every_setting_that_admits_a_zero_is_swept() {
    let vocabulary = vocabulary(&[]).expect("the vocabulary");
    let mut found: Vec<(&str, &str, Option<&str>)> = Vec::new();
    for parameter in common_parameters().expect("the common settings") {
        if admits_zero(&parameter.shape) {
            found.push(("", parameter.pointer, parameter.shape.zero_title()));
        }
    }
    for entry in &vocabulary.mandates {
        for parameter in &entry.parameters {
            if admits_zero(&parameter.shape) {
                found.push((entry.name, parameter.pointer, parameter.shape.zero_title()));
            }
        }
    }
    let mut swept = SWEEP.to_vec();
    swept.sort_unstable();
    found.sort_unstable();
    assert_eq!(
        found, swept,
        "the vocabulary's zero-admitting settings and their titles"
    );
    for family in &vocabulary.families {
        for entry in &family.predicates {
            for parameter in &entry.parameters {
                assert_eq!(
                    parameter.shape.zero_title(),
                    None,
                    "{}{}: a comparison's 0 is a number",
                    entry.name,
                    parameter.pointer
                );
            }
        }
    }
    for entry in vocabulary.steps.iter().chain(&vocabulary.groups) {
        for parameter in &entry.parameters {
            assert_eq!(
                parameter.shape.zero_title(),
                None,
                "{}{}",
                entry.name,
                parameter.pointer
            );
        }
    }
}

/// The titles the table names, by pointer, for the gateway's check of the
/// sim's half: what `crates/gateway/tests/zero_titles.rs` holds itself to.
#[test]
fn the_titled_settings_are_the_seven_the_sim_reads_apart() {
    let titled: Vec<&str> = SWEEP
        .iter()
        .filter(|(_, _, title)| title.is_some())
        .map(|(_, pointer, _)| *pointer)
        .collect();
    assert_eq!(
        titled,
        [
            "/retreat_hp_pct",
            "/repair_threshold_pct",
            "/picket_scouts",
            "/launch/force_size",
            "/scout_count",
            "/dig_max_depth",
            "/pillar_spacing"
        ]
    );
}

/// The prose says a titled 0 in the words its shape's zero title uses: one
/// string-table entry, two readers (the settings row and `render_plan`).
#[test]
fn one_entry_two_readers() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let rules = pharmakos_sim::rules::RulesTable::load(&root.join("rules").join("rules.v1.json"))
        .expect("the committed rules table loads");
    let snapshot = pharmakos_sim::snapshot::Snapshot {
        seat_id: vec![0],
        seat_treasury: vec![200],
        seat_supply: vec![10],
        seat_draw: vec![2],
        ..pharmakos_sim::snapshot::Snapshot::default()
    };
    let context = pharmakos_plan_core::context::PlanContext::from_snapshot(&snapshot, 0)
        .expect("seat 0 is in the snapshot");
    let text = r#"{"schema_version":{"major":1},"kind":"PLAYBOOK",
        "meta":{"title":"zero","author_kind":"HUMAN"},
        "declarative":{"route":[
          {"label":"a","place_beacon":{"at":{"voxel":{"x":1,"y":1,"z":0}},
            "initial":{"mandate":{"retreat_hp_pct":0,"defend":{"picket_scouts":0}}}}},
          {"label":"b","place_beacon":{"at":{"voxel":{"x":2,"y":2,"z":0}},
            "initial":{"mandate":{"retreat_hp_pct":30,"defend":{"picket_scouts":2}}}}}
        ]},
        "on_death":{"on_respawn":"CONTINUE","max_deaths_before_fallback":3},
        "fallback":{"hold":{"at":{"safest":{}}}}}"#;
    let canonical =
        pharmakos_plan_core::canonical::canonicalise_text(text).expect("the playbook decodes");
    let rendering = pharmakos_plan_core::render::render(&canonical.playbook, &context, &rules)
        .expect("the playbook renders");
    let first = rendering
        .prose
        .lines()
        .find(|line| line.contains("voxel 1, 1"))
        .expect("the first beacon's line");
    // Each titled 0 is said as its shape's words and then its chip, in one
    // span: ": retreat off (0%)", ", no picket (0)".
    for (shape, after) in [
        (Shape::retreat_threshold(), " (0%)"),
        (Shape::picket_scouts(), " (0)"),
    ] {
        let title = shape.zero_title().expect("a title");
        let said = format!(" {title}{after}");
        assert!(first.contains(&said), "{first:?} says {said:?}");
    }
    let second = rendering
        .prose
        .lines()
        .find(|line| line.contains("voxel 2, 2"))
        .expect("the second beacon's line");
    for title in [s::RETREAT_OFF, s::PICKET_NONE] {
        assert!(
            !second.contains(title),
            "{second:?}: a non-zero is said as a number"
        );
    }
    let chips: Vec<_> = rendering
        .rules
        .iter()
        .flat_map(|rule| &rule.chips)
        .filter(|chip| chip.value_title.is_some())
        .map(|chip| (chip.pointer.as_str(), chip.value_title))
        .collect();
    assert_eq!(
        chips,
        [
            (
                "/declarative/route/0/place_beacon/initial/mandate/retreat_hp_pct",
                Some(s::RETREAT_OFF)
            ),
            (
                "/declarative/route/0/place_beacon/initial/mandate/defend/picket_scouts",
                Some(s::PICKET_NONE)
            ),
        ],
        "only the zeros carry their words"
    );
}

/// An omitted `roe` in a new beacon's settings is said as the value the
/// verifier's `I0003` note says it reads as, writ by writ, as what the beacon
/// is doing, and as the writ's default: the prose and the
/// note do not disagree (S3's `zerotitle`, carried from `retreat`).
#[test]
fn an_omitted_roe_is_said_as_the_note_reads_it() {
    use pharmakos_proto::gp::api::v1::verify_plan::Depth;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let rules = pharmakos_sim::rules::RulesTable::load(&root.join("rules").join("rules.v1.json"))
        .expect("the committed rules table loads");
    let snapshot = pharmakos_sim::snapshot::Snapshot {
        seat_id: vec![0],
        seat_treasury: vec![200],
        seat_supply: vec![10],
        seat_draw: vec![2],
        ..pharmakos_sim::snapshot::Snapshot::default()
    };
    let context = pharmakos_plan_core::context::PlanContext::from_snapshot(&snapshot, 0)
        .expect("seat 0 is in the snapshot");
    let scope = pharmakos_verifier::Scope::new(
        pharmakos_sim::tables::SeatId::new(0),
        pharmakos_sim::knowledge::SeatEconomy::default(),
    );
    for (writ, arm) in [
        ("build", "{}"),
        ("defend", "{}"),
        ("attack", "{}"),
        ("survey", "{}"),
        ("mine", "{}"),
    ] {
        let text = format!(
            r#"{{"schema_version":{{"major":1}},"kind":"PLAYBOOK",
            "meta":{{"title":"roe","author_kind":"HUMAN"}},
            "declarative":{{"route":[
              {{"label":"a","on_fail":{{"action":"SKIP"}},"place_beacon":{{"at":{{"voxel":{{"x":1,"y":1,"z":0}}}},
                "initial":{{"mandate":{{"{writ}":{arm}}}}}}}}}
            ]}},
            "on_death":{{"on_respawn":"CONTINUE","max_deaths_before_fallback":3}},
            "fallback":{{"hold":{{"at":{{"safest":{{}}}}}}}}}}"#
        );
        let report =
            pharmakos_plan_core::verify::verify_jsonc(&text, &[], &scope, &rules, Depth::Full)
                .expect("the rules table is complete");
        let note = report
            .diagnostics
            .iter()
            .find(|diag| diag.code == "I0003" && diag.path.ends_with("/mandate/roe"))
            .unwrap_or_else(|| panic!("{writ}: the verifier notes the omitted roe"));
        let value = note
            .message
            .trim_end_matches('.')
            .rsplit(' ')
            .next()
            .expect("the note names the value");
        // The prose says the value as what the beacon is doing, a participle
        // (S3's `edlists`); the menu's title for it is the order.
        let words = match value {
            "HOLD_FIRE" => s::ROE_HOLD_FIRE,
            "RETURN_FIRE" => s::ROE_RETURN_FIRE,
            "FREE_FIRE" => s::ROE_FREE_FIRE,
            other => panic!("{writ}: the note names {other}, which has no words"),
        };
        let canonical =
            pharmakos_plan_core::canonical::canonicalise_text(&text).expect("the playbook decodes");
        let prose = pharmakos_plan_core::render::render(&canonical.playbook, &context, &rules)
            .expect("the playbook renders")
            .prose;
        let said = format!("{words} {}", s::ROE_BY_DEFAULT);
        assert!(prose.contains(&said), "{writ}: {prose:?} says {said:?}");
    }
}
