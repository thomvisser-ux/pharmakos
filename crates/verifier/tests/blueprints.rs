// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `BLUEPRINT_SLOTS` against the verifier's own refusals (S3's `vfy6`,
//! follow-up E).
//!
//! The table is the verifier's per-slot blueprint legality, published so a
//! client reads it rather than probing the verifier with a playbook. These
//! tests put every row to the checks it claims to describe — `E0009` for a
//! `queue_structure` row, `E0010` and `E0011` for a Build target, `E0417` for
//! a wall's anchor — and hold its `queued` column to the sim's own
//! `pharmakos_sim::spire::queued_kind`, so the table cannot say one thing
//! while the checks do another.

#![allow(
    clippy::expect_used,
    reason = "a panic is this file's failure report (clippy.toml's own wording), and the \
              helpers are not #[test] functions"
)]

use std::path::PathBuf;

use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::math::quantity::{Kw, Money};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::tables::{SeatId, StructureKind};
use pharmakos_verifier::{Anchor, BLUEPRINT_SLOTS, Input, Scope, slots_of};

fn rules() -> RulesTable {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    RulesTable::load(&root.join("rules").join("rules.v1.json")).expect("the rules table")
}

/// Where the interface step's rows stand.
const ROWS: &str = "/declarative/route/0/interface/rows";

/// The `(code, path)` pairs QUICK raises for a playbook whose one interface
/// step holds `rows`, every one listed: the per-code cap is off (S3's
/// `vcap`), because these tests hold each row's code, and a table row past
/// the cap is still raised (`the_cap_never_moves_a_verdict_or_a_total`).
fn codes(rows: &[String]) -> Vec<(String, String)> {
    let text = format!(
        r#"{{"schema_version":{{"major":1}},"meta":{{"title":"slots","author_kind":"HUMAN"}},"kind":"PLAYBOOK","declarative":{{"route":[{{"label":"probe","interface":{{"beacon":{{"beacon_id":"b_00"}},"rows":[{}]}}}}]}},"on_death":{{"on_respawn":"CONTINUE"}},"fallback":{{"hold":{{"at":{{"safest":{{}}}}}}}}}}"#,
        rows.join(",")
    );
    let rules = rules();
    let scope = Scope::new(
        SeatId::new(0),
        SeatEconomy {
            treasury: Money::ZERO,
            supply: Kw::ZERO,
            draw: Kw::ZERO,
        },
    );
    let input = Input::new(text.as_bytes(), &[], &scope, &rules).expect("the input");
    pharmakos_verifier::verify_counted_at_cap(&input, Depth::Quick, u32::MAX)
        .report
        .diagnostics
        .into_iter()
        .map(|diagnostic| (diagnostic.code, diagnostic.path))
        .collect()
}

fn has(found: &[(String, String)], code: &str, path: &str) -> bool {
    found
        .iter()
        .any(|(at_code, at)| at_code == code && at == path)
}

/// A Build target row naming `id`, anchored by `anchor` (JSON), with a box
/// when `boxed`.
fn target(id: &str, anchor: &str, boxed: bool) -> String {
    let wall_box = if boxed {
        r#","wall_box":{"min":{"x":0,"y":0,"z":0},"max":{"x":3,"y":0,"z":3}}"#
    } else {
        ""
    };
    format!(
        r#"{{"add_build_target":{{"target":{{"blueprint_id":"{id}","anchor":{anchor}{wall_box}}}}}}}"#
    )
}

const VOXEL: &str = r#"{"voxel":{"x":0,"y":0,"z":0}}"#;
const OUTSIDE: &str = r#"{"voxel":{"x":9,"y":9,"z":9}}"#;
const SELECTOR: &str = r#"{"beacon_anchor":{"beacon_id":"b_00"}}"#;

#[test]
fn the_table_names_every_kind_the_sim_defines_once_in_its_order() {
    let kinds: Vec<StructureKind> = BLUEPRINT_SLOTS.iter().map(|row| row.kind).collect();
    assert_eq!(kinds, StructureKind::ALL);
    for row in BLUEPRINT_SLOTS {
        assert_eq!(slots_of(row.id), Some(&row));
        // The queue's column is the sim's compile's answer.
        assert_eq!(
            pharmakos_sim::spire::queued_kind(row.id) == Some(row.kind),
            row.queued,
            "{}",
            row.id
        );
    }
    assert_eq!(slots_of("death_ray"), None);
}

#[test]
fn a_queue_row_is_refused_exactly_where_the_table_says() {
    let mut rows: Vec<String> = BLUEPRINT_SLOTS
        .iter()
        .map(|row| format!(r#"{{"queue_structure":{{"blueprint_id":"{}"}}}}"#, row.id))
        .collect();
    rows.push(r#"{"queue_structure":{"blueprint_id":"death_ray"}}"#.to_owned());
    let found = codes(&rows);
    for (index, row) in BLUEPRINT_SLOTS.iter().enumerate() {
        let at = format!("{ROWS}/{index}/queue_structure");
        assert_eq!(
            has(&found, "E0009", &at),
            !row.queued,
            "{}: {found:?}",
            row.id
        );
    }
    let unknown = format!("{ROWS}/{}/queue_structure", BLUEPRINT_SLOTS.len());
    assert!(has(&found, "E0009", &unknown), "{found:?}");
}

#[test]
fn a_build_target_is_refused_exactly_where_the_table_says() {
    let mut rows: Vec<String> = BLUEPRINT_SLOTS
        .iter()
        .map(|row| target(row.id, VOXEL, row.anchor == Anchor::VoxelInsideBox))
        .collect();
    rows.push(target("death_ray", VOXEL, false));
    let found = codes(&rows);
    for (index, row) in BLUEPRINT_SLOTS.iter().enumerate() {
        let at = format!("{ROWS}/{index}/add_build_target/target/blueprint_id");
        assert_eq!(
            has(&found, "E0010", &at),
            !row.build_target,
            "{}: {found:?}",
            row.id
        );
        // A voxel inside the box is a legal anchor for every kind.
        let anchor = format!("{ROWS}/{index}/add_build_target/target/anchor");
        assert!(
            !found
                .iter()
                .any(|(code, path)| (code == "E0011" || code == "E0417")
                    && path.starts_with(&anchor)),
            "{}: {found:?}",
            row.id
        );
    }
    let unknown = format!(
        "{ROWS}/{}/add_build_target/target/blueprint_id",
        BLUEPRINT_SLOTS.len()
    );
    assert!(has(&found, "E0010", &unknown), "{found:?}");
}

#[test]
fn a_build_targets_anchor_is_refused_exactly_where_the_table_says() {
    for row in BLUEPRINT_SLOTS.iter().filter(|row| row.build_target) {
        let boxed = row.anchor == Anchor::VoxelInsideBox;
        let anchor = format!("{ROWS}/0/add_build_target/target/anchor");
        // A beacon selector: `E0011` for every kind.
        let found = codes(&[target(row.id, SELECTOR, boxed)]);
        assert!(
            has(&found, "E0011", &format!("{anchor}/beacon_anchor")),
            "{}: {found:?}",
            row.id
        );
        // A voxel outside the box: `E0417` for a kind that fills one, and no
        // anchor refusal for any other.
        let found = codes(&[target(row.id, OUTSIDE, boxed)]);
        let refused = found
            .iter()
            .any(|(code, path)| code == "E0417" && path.starts_with(&anchor));
        assert_eq!(refused, boxed, "{}: {found:?}", row.id);
        // `on` a feature: legal unless the kind fills a box.
        let on = r#"{"on":{"feature_id":"vent_0_0"}}"#;
        let found = codes(&[target(row.id, on, boxed)]);
        let refused = found
            .iter()
            .any(|(code, path)| code == "E0417" && path.starts_with(&anchor));
        assert_eq!(refused, boxed, "{}: {found:?}", row.id);
        assert!(
            !has(&found, "E0011", &format!("{anchor}/on")),
            "{}: {found:?}",
            row.id
        );
    }
}

/// The sim's half (the review of PR #179, item 10): the compile accepts a
/// minimal `add_build_target` naming each kind exactly when the table says a
/// Build target takes it, so the table cannot let a playbook qualify that the
/// seal then refuses, nor refuse one the seal takes.
#[test]
fn the_sims_compile_takes_a_build_target_exactly_where_the_table_does() {
    let rules = rules();
    for row in BLUEPRINT_SLOTS {
        let boxed = row.anchor == Anchor::VoxelInsideBox;
        let text = format!(
            r#"{{"schema_version":{{"major":1}},"meta":{{"title":"slots","author_kind":"HUMAN"}},"kind":"PLAYBOOK","declarative":{{"route":[{{"label":"probe","interface":{{"beacon":{{"beacon_id":"b_00"}},"rows":[{}]}}}}]}},"on_death":{{"on_respawn":"CONTINUE"}},"fallback":{{"hold":{{"at":{{"safest":{{}}}}}}}}}}"#,
            target(row.id, VOXEL, boxed)
        );
        let playbook: pharmakos_proto::gp::v1::Playbook =
            pharmakos_proto::json::decode(&text).expect("the probe decodes");
        let compiled = pharmakos_sim::interpreter::Plan::compile(&playbook, &rules);
        assert_eq!(
            compiled.is_ok(),
            row.build_target,
            "{}: {:?}",
            row.id,
            compiled.err()
        );
    }
}
