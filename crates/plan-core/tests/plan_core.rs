// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The acceptance tests for T8, and the goldens under
//! `tests/golden/plan-core/`.
//!
//! Four properties, in the order the skeleton plan's T8 row names them:
//!
//! 1. `examples/playbooks/expand_east.jsonc` **reproduced byte for byte**
//!    through load → canonicalise → save, comments in awkward places intact.
//! 2. A canonical-form golden pinning the writer.
//! 3. `expected.prose.txt` for `render_plan`.
//! 4. The writer proved to emit **bare-number durations** exactly as the
//!    example writes them (decisions-log item 46).
//!
//! The producing tests write `actual.*` under `<target>/golden/plan-core/`,
//! where `cargo xtask ci`'s `golden` step byte-compares them with what is
//! committed. `cargo xtask golden --bless` accepts a move, and a blessed
//! golden has to be explained in the pull request that moved it (AGENTS.md
//! section 5).

use std::fs;
use std::path::{Path, PathBuf};

use pharmakos_plan_core::canonical::canonicalise_text;
use pharmakos_plan_core::context::PlanContext;
use pharmakos_plan_core::jsonc::Document;
use pharmakos_plan_core::render::render_plan;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::snapshot::Snapshot;

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// The workspace root: this crate is `<root>/crates/plan-core`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| panic!("crates/plan-core sits two levels below the workspace root"))
        .to_path_buf()
}

/// The cargo target directory. `CARGO_TARGET_TMPDIR` is `<target>/tmp`, and it
/// is the only way a test can find the target directory that also honours
/// `CARGO_TARGET_DIR` — which every agent worktree sets to its own lane.
fn target_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .parent()
        .unwrap_or_else(|| panic!("CARGO_TARGET_TMPDIR always has a parent"))
        .to_path_buf()
}

fn golden_dir() -> PathBuf {
    workspace_root()
        .join("tests")
        .join("golden")
        .join("plan-core")
}

fn example() -> String {
    read(
        &workspace_root()
            .join("examples")
            .join("playbooks")
            .join("expand_east.jsonc"),
    )
}

fn awkward() -> String {
    read(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("cases")
            .join("awkward.jsonc"),
    )
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

fn rules() -> RulesTable {
    RulesTable::load(&workspace_root().join("rules").join("rules.v1.json"))
        .unwrap_or_else(|error| panic!("reading the committed rules table: {error}"))
}

/// A frozen snapshot with one seat in it. Nothing in the goldens reads the
/// world, so the seat's own row is all the context needs.
fn context() -> PlanContext {
    let snapshot = Snapshot {
        seat_id: vec![0],
        seat_treasury: vec![200],
        seat_supply: vec![10],
        seat_draw: vec![2],
        ..Snapshot::default()
    };
    PlanContext::from_snapshot(&snapshot, 0)
        .unwrap_or_else(|error| panic!("seat 0 is in the snapshot: {error}"))
}

// ---------------------------------------------------------------------------
// The golden comparison
// ---------------------------------------------------------------------------

fn write_actual(case: &str, name: &str, contents: &str) {
    let directory = target_dir().join("golden").join("plan-core").join(case);
    fs::create_dir_all(&directory)
        .unwrap_or_else(|error| panic!("creating {}: {error}", directory.display()));
    fs::write(directory.join(name), contents)
        .unwrap_or_else(|error| panic!("writing {case}/{name}: {error}"));
}

/// A readable first difference, so a failure names a byte rather than dumping
/// two files at the reader.
fn first_difference(expected: &str, actual: &str) -> String {
    let left = expected.as_bytes();
    let right = actual.as_bytes();
    let index = (0..left.len().max(right.len()))
        .find(|at| left.get(*at) != right.get(*at))
        .unwrap_or(0);
    let line = expected
        .get(..index)
        .unwrap_or_default()
        .matches('\n')
        .count()
        .saturating_add(1);
    format!(
        "byte {index} (line {line}):\n  expected: {:?}\n    actual: {:?}",
        expected.get(index..index.saturating_add(70)),
        actual.get(index..index.saturating_add(70)),
    )
}

/// Writes the fresh output as `actual.<suffix>` and compares it with the
/// committed `expected.<suffix>` beside it — the convention `xtask`'s `golden`
/// step re-runs over the whole tree (`tests/golden/README.md`).
fn assert_golden(case: &str, suffix: &str, actual: &str) {
    write_actual(case, &format!("actual.{suffix}"), actual);
    let path = golden_dir().join(case).join(format!("expected.{suffix}"));
    let Ok(expected) = fs::read_to_string(&path) else {
        panic!("{case}: nothing committed at {}", path.display());
    };
    assert!(
        expected == actual,
        "{case}/expected.{suffix} differs from the fresh output at {}\n\nA golden that moves is a behaviour \
         change: say which in the pull request, and see tests/golden/plan-core/README.md.",
        first_difference(&expected, actual)
    );
}

// ---------------------------------------------------------------------------
// 1. The exact round trip
// ---------------------------------------------------------------------------

#[test]
fn the_example_round_trips_byte_for_byte() {
    let text = example();
    let document = Document::parse(&text).expect("the worked example parses");
    assert_eq!(
        document.to_text(),
        text,
        "load-and-save lost a byte of the spec's own worked example"
    );
}

#[test]
fn the_awkward_fixture_round_trips_byte_for_byte() {
    let text = awkward();
    let document = Document::parse(&text).expect("the awkward fixture parses");
    assert_eq!(document.to_text(), text, "load-and-save lost a byte");
}

#[test]
fn every_comment_in_the_awkward_fixture_is_found() {
    let document = Document::parse(&awkward()).expect("the awkward fixture parses");
    let comments = document.comments();
    // Fifteen header lines, then the eleven slot comments the fixture's own
    // header enumerates.
    assert_eq!(comments.len(), 26, "{comments:#?}");
    let text = awkward();
    for comment in &comments {
        assert!(
            text.contains(&comment.text),
            "a comment was rewritten rather than kept verbatim: {:?}",
            comment.text
        );
    }
}

// ---------------------------------------------------------------------------
// 2 and 3. The goldens
// ---------------------------------------------------------------------------

#[test]
fn the_example_canonicalises_to_its_golden() {
    let canonical = canonicalise_text(&example()).expect("the worked example canonicalises");
    assert!(
        canonical.relocated.is_empty(),
        "nothing in the worked example should need relocating: {:?}",
        canonical.relocated
    );
    assert_golden("expand_east", "jsonc", &canonical.text);
}

#[test]
fn the_examples_canonical_json_is_the_one_the_proto_lane_pins() {
    let canonical = canonicalise_text(&example()).expect("the worked example canonicalises");
    let pinned = read(
        &workspace_root()
            .join("tests")
            .join("golden")
            .join("proto")
            .join("expected.expand_east.json"),
    );
    assert_eq!(
        canonical.json, pinned,
        "plan-core's canonical form and crates/proto's are the same form or one of them is \
         wrong; tests/golden/plan-core/README.md says a canonical-form change moves every \
         verifier golden with it"
    );
}

#[test]
fn the_awkward_fixture_canonicalises_to_its_goldens() {
    let canonical = canonicalise_text(&awkward()).expect("the awkward fixture canonicalises");
    assert_golden("awkward", "jsonc", &canonical.text);
    assert_golden("awkward", "canonical.json", &canonical.json);
}

#[test]
fn a_comment_on_a_written_out_default_is_relocated_and_reported() {
    let canonical = canonicalise_text(&awkward()).expect("the awkward fixture canonicalises");
    let moved: Vec<(String, String)> = canonical
        .relocated
        .iter()
        .map(|moved| (moved.from.clone(), moved.to.clone()))
        .collect();
    assert_eq!(
        moved,
        vec![
            (
                "/schema_version/minor".to_owned(),
                "/schema_version".to_owned()
            ),
            (
                "/schema_version/minor".to_owned(),
                "/schema_version".to_owned()
            ),
        ],
        "the two lines of the comment on `minor` are relocated, not dropped"
    );
    assert!(
        canonical
            .text
            .contains("// a default the author wrote out: the canonical form drops the member, so"),
        "a relocated comment is still in the file:\n{}",
        canonical.text
    );
}

#[test]
fn the_example_renders_to_its_prose_golden() {
    let canonical = canonicalise_text(&example()).expect("the worked example canonicalises");
    let prose = render_plan(&canonical.playbook, &context(), &rules())
        .expect("the committed rules table is complete");
    assert_golden("expand_east", "prose.txt", &prose);
}

#[test]
fn the_rendering_is_the_same_on_every_run() {
    let canonical = canonicalise_text(&example()).expect("the worked example canonicalises");
    let once = render_plan(&canonical.playbook, &context(), &rules()).expect("a rendering");
    let twice = render_plan(&canonical.playbook, &context(), &rules()).expect("a rendering");
    assert_eq!(once, twice);
}

// ---------------------------------------------------------------------------
// 4. Bare-number durations
// ---------------------------------------------------------------------------

#[test]
fn the_writer_emits_bare_number_durations_as_the_example_writes_them() {
    let canonical = canonicalise_text(&example()).expect("the worked example canonicalises");
    for (field, value) in [
        ("timeout_ms", "120000"),
        ("cooldown_ms", "30000"),
        ("ms", "3000"),
    ] {
        let bare = format!("\"{field}\": {value}");
        let as_string = format!("\"{field}\": \"{value}\"");
        assert!(
            canonical.json.contains(&bare),
            "decisions-log item 46: `{field}` is int32, so the JSON mapping emits a bare number \
             and the spec's example loads as written\n{}",
            canonical.json
        );
        assert!(
            !canonical.json.contains(&as_string),
            "`{field}` came back as a quoted string, which is the int64 mapping"
        );
    }
    // And the same in the JSONC form, which is what a player actually reads.
    assert!(canonical.text.contains("\"timeout_ms\": 120000"));
}

// ---------------------------------------------------------------------------
// The whole pipeline
// ---------------------------------------------------------------------------

#[test]
fn canonicalising_is_idempotent() {
    let once = canonicalise_text(&example()).expect("a canonical form");
    let twice = canonicalise_text(&once.text).expect("canonicalising it again");
    assert_eq!(once.text, twice.text);
    assert_eq!(once.json, twice.json);
    assert!(twice.relocated.is_empty());
}

#[test]
fn a_canonical_file_still_round_trips_byte_for_byte() {
    let canonical = canonicalise_text(&example()).expect("a canonical form");
    let document = Document::parse(&canonical.text).expect("the canonical form parses");
    assert_eq!(document.to_text(), canonical.text);
}
