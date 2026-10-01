// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Targeting's two sites are refused with `E0003` until their rules land.
//!
//! S1's targeting proto (S1's plan, task `con2`) adds `Location.on` and
//! `Location.covering`, each a `gp.v1.FeatureRef`, to the vocabulary. Which arm
//! is legal where, the ranks and the coverage filters are S1's targeting
//! verifier lane's (task `tgtv`); until then seal inspection refuses each arm
//! wherever it is written, with `E0003` pointed at the arm, as it refuses a
//! held-back word. A report that qualified a file the sim then refused at
//! compile would be a verifier that lies, and the sim refuses both
//! (`crates/sim/tests/targeting_refused.rs`). `tgtv` replaces these assertions.
//!
//! Inline asserts and no golden, as the plan asks: the report goldens are the
//! committed cases', and none of them uses a targeting arm.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report."
)]

use std::path::{Path, PathBuf};

use pharmakos_proto::gp::api::v1::VerifyReport;
use pharmakos_proto::gp::api::v1::diagnostic::Severity;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::gp::v1::beacon_filter::MandateKind;
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::math::quantity::{Kw, Money};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::snapshot::{SNAPSHOT_VERSION, Snapshot};
use pharmakos_sim::tables::SeatId;
use pharmakos_verifier::{Input, KnownBeacon, Ownership, Scope, verify};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| panic!("crates/verifier sits two levels below the workspace root"))
        .to_path_buf()
}

fn rules() -> RulesTable {
    RulesTable::load(&workspace_root().join("rules").join("rules.v1.json"))
        .unwrap_or_else(|error| panic!("reading the rules table: {error}"))
}

fn snapshot() -> Vec<u8> {
    Snapshot {
        version: SNAPSHOT_VERSION,
        match_seed: 0x0102_0304_0506_0708,
        tick: 3_600,
        ..Snapshot::default()
    }
    .to_bytes()
    .unwrap_or_else(|error| panic!("encoding the snapshot: {error}"))
}

/// Seat 0 with its core, `b_01`, on BUILD: enough for an interface step to
/// name a beacon the seat holds, and for a site to lie inside its sphere.
fn scope() -> Scope {
    Scope::new(
        SeatId::new(0),
        SeatEconomy {
            treasury: Money::new(200),
            supply: Kw::new(10),
            draw: Kw::new(4),
        },
    )
    .with_beacon(KnownBeacon {
        beacon_id: "b_01".to_owned(),
        owner: SeatId::new(0),
        side: Ownership::Own,
        mandate: MandateKind::Build,
        tags: Vec::new(),
        at: Voxel {
            x: 80,
            y: 11,
            z: 55,
        },
        is_core: true,
    })
}

/// A whole playbook around one route step, as canonical `gp.v1` JSON.
fn playbook_with(step: &str) -> String {
    format!(
        concat!(
            "{{\"schema_version\":{{\"major\":1}},",
            "\"meta\":{{\"title\":\"targeting\",\"author_kind\":\"HUMAN\"}},",
            "\"declarative\":{{\"route\":[{step}]}},",
            "\"on_death\":{{\"on_respawn\":\"CONTINUE\",\"max_deaths_before_fallback\":2}},",
            "\"fallback\":{{\"hold\":{{\"at\":{{\"safest\":{{}}}}}}}},",
            "\"kind\":\"PLAYBOOK\"}}"
        ),
        step = step
    )
}

fn report(step: &str, depth: Depth) -> VerifyReport {
    let bytes = playbook_with(step);
    let snapshot = snapshot();
    let scope = scope();
    let rules = rules();
    let input = Input::new(bytes.as_bytes(), &snapshot, &scope, &rules)
        .unwrap_or_else(|error| panic!("the rules table carries what the checks read: {error}"));
    verify(&input, depth)
}

/// The `E0003` paths a report carries, in report order.
fn held_paths(report: &VerifyReport) -> Vec<String> {
    report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "E0003")
        .map(|diagnostic| diagnostic.path.clone())
        .collect()
}

/// Both depths refuse it, at exactly these pointers, and the file does not
/// qualify.
fn assert_refused_at(step: &str, expected: &[&str]) {
    for depth in [Depth::Quick, Depth::Full] {
        let report = report(step, depth);
        assert!(
            !report.qualifies,
            "{depth:?}: a targeting site qualified before its rules landed:\n{report:?}"
        );
        assert_eq!(held_paths(&report), expected, "{depth:?}: {report:?}");
        for diagnostic in &report.diagnostics {
            if diagnostic.code == "E0003" {
                assert_eq!(diagnostic.severity(), Severity::Error);
                assert!(
                    diagnostic.message.contains("`on`")
                        || diagnostic.message.contains("`covering`"),
                    "the message names the arm: {}",
                    diagnostic.message
                );
            }
        }
    }
}

const NEAREST_UNCOVERED_VENT: &str = r#"{"vent":{"rank":"NEAREST","coverage":"UNCOVERED"}}"#;

#[test]
fn a_place_beacon_covering_a_vent_verifies_with_e0003() {
    let step = format!(
        r#"{{"label":"cover","place_beacon":{{"at":{{"covering":{NEAREST_UNCOVERED_VENT}}}}}}}"#
    );
    assert_refused_at(&step, &["/declarative/route/0/place_beacon/at/covering"]);
}

#[test]
fn a_build_target_on_a_vent_verifies_with_e0003() {
    // A Build target's anchor is `on`'s one legal place, and the structure
    // stage's own place checks never see it, which is why the refusal walks
    // the whole file.
    let step = concat!(
        r#"{"label":"build","interface":{"beacon":{"beacon_id":"b_01"},"rows":[{"add_build_target":"#,
        r#"{"target":{"blueprint_id":"generator","anchor":{"on":{"vent":{"rank":"NEAREST"}}}}}}]}}"#
    );
    assert_refused_at(
        step,
        &["/declarative/route/0/interface/rows/0/add_build_target/target/anchor/on"],
    );
}

#[test]
fn removing_a_target_by_its_feature_verifies_with_e0003() {
    let step = concat!(
        r#"{"label":"unbuild","interface":{"beacon":{"beacon_id":"b_01"},"rows":[{"remove_build_target":"#,
        r#"{"anchor":{"on":{"feature_id":"vent_120_88"}}}}]}}"#
    );
    assert_refused_at(
        step,
        &["/declarative/route/0/interface/rows/0/remove_build_target/anchor/on"],
    );
}

#[test]
fn a_generator_on_the_covered_vent_verifies_with_e0003_at_both_arms() {
    // The adopted spelling of "place a beacon covering the nearest vent you can
    // cover, and build a Generator on it" (targeting.md, "Sites"), refused at
    // both of its arms in document order.
    let step = format!(
        concat!(
            r#"{{"label":"cover","place_beacon":{{"at":{{"covering":{vent}}},"#,
            r#""initial":{{"mandate":{{"build":{{"targets":[{{"blueprint_id":"generator","#,
            r#""anchor":{{"on":{{"covered":{{}}}}}}}}]}}}}}}}}}}"#
        ),
        vent = NEAREST_UNCOVERED_VENT
    );
    assert_refused_at(
        &step,
        &[
            "/declarative/route/0/place_beacon/at/covering",
            "/declarative/route/0/place_beacon/initial/mandate/build/targets/0/anchor/on",
        ],
    );
}

#[test]
fn a_site_in_the_fallback_is_refused_too() {
    // Not a legal place for either arm, which is `tgtv`'s to say; until then it
    // is refused rather than passed.
    let bytes = playbook_with(r#"{"label":"wait","hold":{"ms":1000}}"#).replace(
        r#""fallback":{"hold":{"at":{"safest":{}}}}"#,
        &format!(r#""fallback":{{"hold":{{"at":{{"covering":{NEAREST_UNCOVERED_VENT}}}}}}}"#),
    );
    let snapshot = snapshot();
    let scope = scope();
    let rules = rules();
    let input = Input::new(bytes.as_bytes(), &snapshot, &scope, &rules)
        .unwrap_or_else(|error| panic!("{error}"));
    let report = verify(&input, Depth::Quick);
    assert!(!report.qualifies, "{report:?}");
    assert_eq!(held_paths(&report), ["/fallback/hold/at/covering"]);
}

#[test]
fn the_same_step_at_a_fixed_voxel_carries_no_e0003() {
    // The control: the refusal is about the new arm, not about the step.
    let step = r#"{"label":"place","place_beacon":{"at":{"voxel":{"x":90,"y":20,"z":55}}}}"#;
    for depth in [Depth::Quick, Depth::Full] {
        let report = report(step, depth);
        assert!(held_paths(&report).is_empty(), "{depth:?}: {report:?}");
    }
}
