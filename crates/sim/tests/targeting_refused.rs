// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Targeting's two sites are refused until their behaviour lands.
//!
//! S1's targeting proto (S1's plan, task `con2`) adds `Location.on` and
//! `Location.covering`, each a `gp.v1.FeatureRef`, to the vocabulary. Their
//! effect -- the resolver, "nearest", `covering`'s spiral and the `on` rules --
//! is the targeting behaviour pull request's (task `tgt`). Until then the
//! interpreter refuses both at compile with `PlanError::NotAtThisStage`, naming
//! the construct, exactly as it refuses today's other held constructs
//! (`tests/interpreter.rs`,
//! `a_construct_this_build_cannot_execute_is_refused_rather_than_skipped`), and
//! never reads either as a voxel, a beacon or nothing (AGENTS.md section 12).
//! `tgt` replaces these assertions with the behaviour.
//!
//! The playbooks are written as canonical `gp.v1` JSON, as a player would write
//! them, so the test also pins that each new arm decodes.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report."
)]

use std::path::PathBuf;

use pharmakos_proto::gp;
use pharmakos_proto::json;
use pharmakos_sim::RulesTable;
use pharmakos_sim::interpreter::{Plan, PlanError};

fn repo_root() -> PathBuf {
    let here = PathBuf::from(".");
    if here.join("rules").join("rules.v1.json").is_file() {
        return here;
    }
    PathBuf::from("..").join("..")
}

fn rules() -> RulesTable {
    RulesTable::load(&repo_root().join("rules").join("rules.v1.json"))
        .expect("the committed rules table loads")
}

/// A whole playbook around one route step, as canonical `gp.v1` JSON.
fn playbook_with(step: &str) -> gp::v1::Playbook {
    let text = format!(
        concat!(
            "{{\"schema_version\":{{\"major\":1}},",
            "\"meta\":{{\"title\":\"targeting\",\"author_kind\":\"HUMAN\"}},",
            "\"declarative\":{{\"route\":[{step}]}},",
            "\"on_death\":{{\"on_respawn\":\"CONTINUE\",\"max_deaths_before_fallback\":2}},",
            "\"fallback\":{{\"hold\":{{\"at\":{{\"safest\":{{}}}}}}}},",
            "\"kind\":\"PLAYBOOK\"}}"
        ),
        step = step
    );
    json::decode(&text).unwrap_or_else(|error| panic!("the case decodes: {error}\n{text}"))
}

/// Compiles the playbook and returns the construct the refusal names.
fn refused_construct(step: &str) -> &'static str {
    match Plan::compile(&playbook_with(step), &rules()) {
        Ok(_) => panic!("a targeting site compiled before its behaviour landed:\n{step}"),
        Err(PlanError::NotAtThisStage { construct, stage }) => {
            assert!(
                stage.contains("targeting"),
                "the refusal names targeting as the stage that fills it, not `{stage}`"
            );
            construct
        }
        Err(other) => panic!("refused, but not as a held construct: {other}\n{step}"),
    }
}

const NEAREST_UNCOVERED_VENT: &str = r#"{"vent":{"rank":"NEAREST","coverage":"UNCOVERED"}}"#;

#[test]
fn a_place_beacon_covering_a_vent_is_refused_naming_covering() {
    let step = format!(
        r#"{{"label":"cover","place_beacon":{{"at":{{"covering":{NEAREST_UNCOVERED_VENT}}}}}}}"#
    );
    assert_eq!(refused_construct(&step), "site `covering`");
}

#[test]
fn a_place_beacon_covering_a_named_seam_is_refused_naming_covering() {
    let step =
        r#"{"label":"cover","place_beacon":{"at":{"covering":{"feature_id":"seam_120_88"}}}}"#;
    assert_eq!(refused_construct(step), "site `covering`");
}

#[test]
fn a_build_target_on_a_vent_is_refused_naming_on() {
    let step = concat!(
        r#"{"label":"build","interface":{"beacon":{"beacon_id":"b_00"},"rows":[{"add_build_target":"#,
        r#"{"target":{"blueprint_id":"generator","anchor":{"on":{"vent":{"rank":"NEAREST"}}}}}}]}}"#
    );
    assert_eq!(refused_construct(step), "site `on`");
}

#[test]
fn removing_a_target_by_its_feature_is_refused_naming_on() {
    let step = concat!(
        r#"{"label":"unbuild","interface":{"beacon":{"beacon_id":"b_00"},"rows":[{"remove_build_target":"#,
        r#"{"anchor":{"on":{"feature_id":"vent_120_88"}}}}]}}"#
    );
    assert_eq!(refused_construct(step), "site `on`");
}

#[test]
fn a_generator_on_the_covered_vent_in_a_new_beacons_initial_is_refused() {
    // The adopted spelling of "place a beacon covering the nearest vent you can
    // cover, and build a Generator on it" (targeting.md, "Sites"). Both arms
    // are refused; whichever the compile meets first is named, and it is one
    // of the two, never a silent skip.
    let step = format!(
        concat!(
            r#"{{"label":"cover","place_beacon":{{"at":{{"covering":{vent}}},"#,
            r#""initial":{{"mandate":{{"build":{{"targets":[{{"blueprint_id":"generator","#,
            r#""anchor":{{"on":{{"covered":{{}}}}}}}}]}}}}}}}}}}"#
        ),
        vent = NEAREST_UNCOVERED_VENT
    );
    let construct = refused_construct(&step);
    assert!(
        construct == "site `covering`" || construct == "site `on`",
        "{construct}"
    );
}

#[test]
fn a_covered_vent_under_a_fixed_site_is_refused_naming_on() {
    // `covered {}` is legal only under `on`, so the `on` refusal is what
    // reaches it when the site itself is a fixed voxel.
    let step = concat!(
        r#"{"label":"place","place_beacon":{"at":{"voxel":{"x":100,"y":100,"z":40}},"#,
        r#""initial":{"mandate":{"build":{"targets":[{"blueprint_id":"generator","#,
        r#""anchor":{"on":{"covered":{}}}}]}}}}}"#
    );
    assert_eq!(refused_construct(step), "site `on`");
}

#[test]
fn a_site_written_where_it_is_not_legal_is_still_refused_not_read_as_a_place() {
    // A move to `covering` is illegal (the verifier's to say, from S1's
    // targeting verifier lane), but the interpreter must not read it as a place
    // in the meantime.
    let step =
        format!(r#"{{"label":"walk","move":{{"to":{{"covering":{NEAREST_UNCOVERED_VENT}}}}}}}"#);
    assert_eq!(refused_construct(&step), "site `covering`");
}

#[test]
fn the_refusal_reads_as_a_held_construct() {
    let step = format!(
        r#"{{"label":"cover","place_beacon":{{"at":{{"covering":{NEAREST_UNCOVERED_VENT}}}}}}}"#
    );
    let Err(error) = Plan::compile(&playbook_with(&step), &rules()) else {
        panic!("refused");
    };
    let message = error.to_string();
    assert!(
        message.contains("site `covering`") && message.contains("refused"),
        "{message}"
    );
}

#[test]
fn the_same_playbook_at_a_fixed_voxel_compiles() {
    // The control: the refusal is about the new arm, not about the step.
    let step = r#"{"label":"place","place_beacon":{"at":{"voxel":{"x":100,"y":100,"z":40}}}}"#;
    Plan::compile(&playbook_with(step), &rules())
        .unwrap_or_else(|error| panic!("a fixed site compiles: {error}"));
}
