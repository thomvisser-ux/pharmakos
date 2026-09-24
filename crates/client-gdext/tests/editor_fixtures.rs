// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The editor's fixtures under `godot/fixtures/` are what they say they are.
//!
//! The headless watch check (`godot/scripts/watch_check.gd`) opens, edits and submits
//! these files against a real `gamectl host`, and the render-only rows scene draws one of
//! them. Each is a copy of, or is made from, something another crate owns, and a copy that
//! nothing compares drifts in silence — so each is compared here:
//!
//! * `editor_check.jsonc` is in plan-core's canonical form: with its comment lines taken
//!   out, it is byte for byte what `pharmakos-proto`'s canonical writer makes of it, which
//!   is the form plan-core's canonical text lays out (plan-core pins the two together in
//!   `the_examples_canonical_json_is_the_one_the_proto_lane_pins`);
//! * `editor_check.expected.jsonc` is that playbook with exactly the step the check's map
//!   action appends — `Go here` to the nearest own beacon, built by the editor's own
//!   [`step_value`] — and nothing else changed. Its byte layout is plan-core's `patch_plan`
//!   output, which the live check compares byte for byte on the Windows and Linux legs;
//! * `out_of_vocabulary.json` is the verifier's own E0003 case, byte for byte;
//! * `needs_a_fix.jsonc` is the verifier's own E0108 case, comments aside;
//! * `rows_report.json` carries the verifier's own committed diagnostics for four cases.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules — \
              not an integration test's helper functions. A panic is this file's failure \
              report (clippy.toml's own wording)."
)]

use std::path::PathBuf;

use pharmakos_client_gdext::editor::{Action, Selector, Target, step_value, strip_comments};
use pharmakos_proto::json::{self, Json};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn read(relative: &str) -> String {
    let path = root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The text with every line that is only a comment taken out.
fn without_comment_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
    {
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn document(text: &str) -> Json {
    json::read(&strip_comments(text)).expect("JSON once the comments are gone")
}

fn route(document: &Json) -> Vec<Json> {
    match document
        .get("declarative")
        .and_then(|declarative| declarative.get("route"))
    {
        Some(Json::Array(steps)) => steps.clone(),
        other => panic!("a route, not {other:?}"),
    }
}

/// `document` with its route taken out, for comparing the rest.
fn without_route(document: &Json) -> Json {
    let Json::Object(entries) = document else {
        panic!("a playbook is an object");
    };
    Json::Object(
        entries
            .iter()
            .map(|(key, value)| {
                if key == "declarative" {
                    (key.clone(), Json::Null)
                } else {
                    (key.clone(), value.clone())
                }
            })
            .collect(),
    )
}

#[test]
fn the_checks_playbook_is_in_canonical_form() {
    let text = read("godot/fixtures/editor_check.jsonc");
    assert!(!text.contains('\r'), "LF endings");
    let body = without_comment_lines(&text);
    let canonical =
        json::canonicalise("gp.v1.Playbook", &body).expect("a gp.v1 playbook once uncommented");
    assert_eq!(
        body.trim_end(),
        canonical.trim_end(),
        "godot/fixtures/editor_check.jsonc is no longer in canonical form, so opening and saving \
         it no longer shows the byte-exact round trip on a canonical file. Regenerate it with \
         plan-core's `canonicalise_text` and regenerate editor_check.expected.jsonc with it."
    );
}

#[test]
fn the_expected_submission_is_the_checks_playbook_plus_its_one_map_action() {
    let check = document(&read("godot/fixtures/editor_check.jsonc"));
    let expected_text = read("godot/fixtures/editor_check.expected.jsonc");
    assert!(!expected_text.contains('\r'), "LF endings");
    let expected = document(&expected_text);
    let mut steps = route(&check);
    steps.push(
        step_value(Action::Go, &Target::Selector(Selector::Nearest), "go_1")
            .expect("the check's map action"),
    );
    let got = route(&expected);
    assert_eq!(
        got.len(),
        steps.len(),
        "the expected submission appends exactly one step"
    );
    for (got, want) in got.iter().zip(&steps) {
        assert_eq!(json::write(got), json::write(want));
    }
    assert_eq!(
        json::write(&without_route(&expected)),
        json::write(&without_route(&check)),
        "nothing but the route changed"
    );
    assert!(
        expected_text.starts_with(
            read("godot/fixtures/editor_check.jsonc")
                .lines()
                .next()
                .expect("a first line")
        ),
        "the header comment survives the patch"
    );
}

#[test]
fn the_out_of_vocabulary_file_is_the_verifiers_own_case() {
    assert_eq!(
        read("godot/fixtures/out_of_vocabulary.json"),
        read("crates/verifier/tests/cases/e0003_reserved_construct.json"),
        "godot/fixtures/out_of_vocabulary.json is a byte-identical copy of the verifier's \
         E0003 case, so the check's refusal is the verifier's"
    );
}

#[test]
fn the_file_with_a_fix_is_the_verifiers_own_case() {
    assert_eq!(
        json::write(&document(&read("godot/fixtures/needs_a_fix.jsonc"))),
        json::write(&document(&read(
            "crates/verifier/tests/cases/e0108_cooldown_below_minimum.json"
        ))),
    );
}

#[test]
fn the_rows_report_carries_the_verifiers_own_diagnostics() {
    let report = document(&read("godot/fixtures/rows_report.json"));
    let Some(Json::Array(diagnostics)) = report.get("diagnostics") else {
        panic!("a report with diagnostics");
    };
    let cases = [
        "e0003_reserved_construct",
        "e0108_cooldown_below_minimum",
        "e0008_stale_fingerprint",
        "e0403_site_outside_every_sphere",
    ];
    let mut want: Vec<Json> = Vec::new();
    for case in cases {
        let golden = document(&read(&format!(
            "tests/golden/verifier/{case}/expected.report.json"
        )));
        match golden.get("diagnostics") {
            Some(Json::Array(found)) => want.extend(found.iter().cloned()),
            other => panic!("{case}: {other:?}"),
        }
    }
    assert_eq!(
        diagnostics.iter().map(json::write).collect::<Vec<_>>(),
        want.iter().map(json::write).collect::<Vec<_>>(),
        "godot/fixtures/rows_report.json carries the committed goldens' diagnostics, in order"
    );
}
