// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The verifier's Fix for a `KNOWN_WITHIN` with no window, held to the window
//! plan-core's enemy skeletons write (S3's `vfyq2`).
//!
//! Two crates write the same starting window: the vocabulary's three enemy
//! predicates insert `"known_within_ms":30000` when a player picks one
//! (`vocabulary.rs`'s `enemy_knowledge_family`), and the verifier's `E0209`
//! offers the same number when a player wrote `KNOWN_WITHIN` and no window
//! (`crates/verifier/src/structure.rs`'s `freshness`). Nothing else ties the
//! two numbers together, so this test does: an edit to either alone fails it.
//! It then applies the Fix with plan-core's own patch and verifies the result,
//! once for each of the two spellings a missing window has, the member left
//! out and the member written as 0, where the Fix's `add` replaces.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml's allow-expect-in-tests only recognises #[test] functions, not an \
              integration test's helpers. A panic is this file's failure report."
)]

use std::path::Path;

use pharmakos_plan_core::patch::patch_text;
use pharmakos_plan_core::verify::verify_jsonc;
use pharmakos_plan_core::vocabulary::vocabulary;
use pharmakos_proto::gp::api::v1::VerifyReport;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::json::{self, Json};
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::tables::SeatId;
use pharmakos_verifier::Scope;

/// The enemy predicates whose skeletons write a window.
const ENEMY_PREDICATES: [&str; 3] = [
    "enemy_units_near",
    "enemy_beacon_known",
    "enemy_structure_near",
];

/// Where the rule's window stands in [`playbook`]'s file.
const WINDOW: &str = "/declarative/handlers/0/when/enemy_beacon_known/freshness/known_within_ms";

fn rules() -> RulesTable {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/plan-core sits two levels below the workspace root");
    RulesTable::load(&root.join("rules").join("rules.v1.json")).expect("the committed rules table")
}

/// A plain seat, which is all a QUICK check of [`playbook`] reads.
fn scope() -> Scope {
    Scope::new(SeatId::new(0), SeatEconomy::default())
}

/// A playbook with one rule watching `enemy_beacon_known` under `freshness`,
/// every step's `on_fail` written, so that nothing but the window is noted.
fn playbook(freshness: &str) -> String {
    format!(
        r#"{{
  "schema_version": {{"major": 1}},
  "meta": {{"title": "a window", "author_kind": "HUMAN"}},
  "kind": "PLAYBOOK",
  "declarative": {{
    "route": [{{"label": "s", "on_fail": {{"action": "SKIP"}}, "hold": {{"ms": 1000}}}}],
    "handlers": [{{
      "id": "h",
      // the window is the question
      "when": {{"enemy_beacon_known": {{"freshness": {freshness}}}}},
      "body": [{{"label": "b", "on_fail": {{"action": "SKIP"}}, "hold": {{"ms": 1000}}}}],
      "resume": "CONTINUE",
      "cooldown_ms": 5000,
      "max_fires": 1
    }}]
  }},
  "on_death": {{"on_respawn": "CONTINUE", "max_deaths_before_fallback": 1}},
  "fallback": {{"hold": {{"at": {{"beacon_anchor": {{"safest": {{}}}}}}}}}}
}}
"#
    )
}

fn check(text: &str) -> VerifyReport {
    verify_jsonc(text, &[], &scope(), &rules(), Depth::Quick).expect("the rules table is complete")
}

/// The windows the vocabulary's enemy skeletons write, one per predicate.
fn skeleton_windows() -> Vec<(&'static str, Json)> {
    let vocabulary = vocabulary(&[]).expect("the vocabulary builds");
    ENEMY_PREDICATES
        .iter()
        .map(|name| {
            let entry = vocabulary
                .families
                .iter()
                .flat_map(|family| family.predicates.iter())
                .find(|entry| entry.name == *name)
                .unwrap_or_else(|| panic!("the vocabulary holds `{name}`"));
            let skeleton = json::read(&entry.skeleton_json)
                .unwrap_or_else(|error| panic!("`{name}`'s skeleton is JSON: {error}"));
            let window = skeleton
                .get("freshness")
                .and_then(|freshness| freshness.get("known_within_ms"))
                .unwrap_or_else(|| panic!("`{name}`'s skeleton writes a window"))
                .clone();
            (*name, window)
        })
        .collect()
}

#[test]
fn the_fix_for_a_missing_window_writes_the_skeletons_window_and_qualifies() {
    let windows = skeleton_windows();
    for freshness in [
        r#"{"kind": "KNOWN_WITHIN"}"#,
        r#"{"kind": "KNOWN_WITHIN", "known_within_ms": 0}"#,
        // The JSON name, which proto3's mapping reads as the same field: the
        // Fix addresses the member the file writes (fix pass 1, item 9).
        r#"{"kind": "KNOWN_WITHIN", "knownWithinMs": 0}"#,
    ] {
        let text = playbook(freshness);
        let report = check(&text);
        let refused: Vec<_> = report
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "E0209")
            .collect();
        assert_eq!(refused.len(), 1, "{freshness}: one E0209: {report:?}");
        let diagnostic = refused.first().expect("one E0209");
        assert_eq!(diagnostic.path, WINDOW, "{freshness}");
        let fix = diagnostic
            .suggestions
            .first()
            .unwrap_or_else(|| panic!("{freshness}: E0209 offers a Fix"));

        // (1) Its value is every enemy skeleton's window.
        let patch = json::read(&fix.json_patch).expect("the Fix's patch is JSON");
        let Json::Array(operations) = &patch else {
            panic!("{freshness}: the Fix's patch is a list: {}", fix.json_patch);
        };
        let [operation] = operations.as_slice() else {
            panic!("{freshness}: the Fix is one operation: {}", fix.json_patch);
        };
        assert_eq!(
            operation.get("op"),
            Some(&Json::String("add".to_owned())),
            "{freshness}"
        );
        assert_eq!(
            operation.get("path"),
            Some(&Json::String(WINDOW.to_owned())),
            "{freshness}"
        );
        let value = operation.get("value").expect("an `add` carries a value");
        for (name, window) in &windows {
            assert_eq!(
                value, window,
                "{freshness}: the Fix writes the window `{name}`'s skeleton writes"
            );
        }

        // (2) Applied with plan-core's own patch, the file qualifies.
        let (patched, _inverse) =
            patch_text(&text, &fix.json_patch).expect("the Fix applies to the file");
        assert!(
            patched.contains("// the window is the question"),
            "{freshness}: the patch keeps the file's comment"
        );
        let report = check(&patched);
        assert!(
            report
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "E0209" && diagnostic.code != "E0210"),
            "{freshness}: the window is answered: {report:?}"
        );
        assert!(report.qualifies, "{freshness}: {report:?}");
    }
}
