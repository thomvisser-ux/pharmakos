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
//! 5. From S3's `gwr`, `expected.chips.txt` for each rendered case: the rules,
//!    steps and chips `render` places beside the prose (S3's plan, E8), with
//!    the promises they keep and the verifier's agreement on their bounds.
//!
//! The producing tests write `actual.*` under `<target>/golden/plan-core/`,
//! where `cargo xtask ci`'s `golden` step byte-compares them with what is
//! committed. `cargo xtask golden --bless` accepts a move, and a blessed
//! golden has to be explained in the pull request that moved it (AGENTS.md
//! section 5).

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use pharmakos_plan_core::canonical::canonicalise_text;
use pharmakos_plan_core::context::PlanContext;
use pharmakos_plan_core::error::split_pointer;
use pharmakos_plan_core::jsonc::Document;
use pharmakos_plan_core::patch::patch_text;
use pharmakos_plan_core::render::{Rendering, render, render_plan};
use pharmakos_plan_core::shape::Shape;
use pharmakos_plan_core::verify::verify_jsonc;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::json::Json;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::snapshot::Snapshot;
use pharmakos_verifier::Scope;

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

fn cover_the_nearest_vent() -> String {
    read(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("cases")
            .join("cover_the_nearest_vent.jsonc"),
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

// ---------------------------------------------------------------------------
// Targeting's sites (S1, task `tgtv`)
// ---------------------------------------------------------------------------

#[test]
fn targetings_sites_round_trip_canonicalise_and_render_to_their_goldens() {
    // Every new arm in the places an author writes it: `covering` with a pick
    // and with a name, `on` with a pick, with `covered {}` and with a name.
    let text = cover_the_nearest_vent();
    let document = Document::parse(&text).expect("the targeting fixture parses");
    assert_eq!(document.to_text(), text, "load-and-save lost a byte");
    let canonical = canonicalise_text(&text).expect("the targeting fixture canonicalises");
    assert!(
        canonical.relocated.is_empty(),
        "nothing in the fixture is written at a default: {:?}",
        canonical.relocated
    );
    assert_golden("cover_the_nearest_vent", "jsonc", &canonical.text);
    let again = canonicalise_text(&canonical.text).expect("the canonical text canonicalises");
    assert_eq!(
        again.text, canonical.text,
        "the canonical form is a fixed point"
    );
    let prose = render_plan(&canonical.playbook, &context(), &rules())
        .expect("the committed rules table is complete");
    assert_golden("cover_the_nearest_vent", "prose.txt", &prose);
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

// ---------------------------------------------------------------------------
// S2's constructs (task `vfy2`)
// ---------------------------------------------------------------------------

fn defend_and_attack() -> String {
    read(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("cases")
            .join("defend_and_attack.jsonc"),
    )
}

#[test]
fn s3s_constructs_round_trip_canonicalise_and_render_to_their_goldens() {
    // The eight predicates and `toward`, on the wire from S3's `con6`.
    let text = read(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("cases")
            .join("predicates_and_toward.jsonc"),
    );
    let document = Document::parse(&text).expect("the S3 fixture parses");
    assert_eq!(document.to_text(), text, "load-and-save lost a byte");
    let canonical = canonicalise_text(&text).expect("the S3 fixture canonicalises");
    assert!(
        canonical.relocated.is_empty(),
        "nothing in the fixture is written at a default: {:?}",
        canonical.relocated
    );
    assert_golden("predicates_and_toward", "jsonc", &canonical.text);
    let again = canonicalise_text(&canonical.text).expect("the canonical text canonicalises");
    assert_eq!(
        again.text, canonical.text,
        "the canonical form is a fixed point"
    );
    let prose = render_plan(&canonical.playbook, &context(), &rules())
        .expect("the committed rules table is complete");
    for said in [
        "a site toward the nearest heat vent you do not cover yet",
        "a site toward Heat vent (120, 88)",
        "has at or above 2 scouts",
        "carries Build",
        "has a finished generator",
        "the seat knows of at or above 3 enemy units within 24 voxels of",
        "in sight now",
        "the seat knows of an enemy beacon, seen in the last 30 s",
        "the seat knows of an enemy mortar within 16 voxels of",
        "the seat knows of an enemy structure within 16 voxels of",
        "the grid's supply is at or above 40 kW",
        "the grid's draw is below 30 kW",
    ] {
        assert!(prose.contains(said), "the prose says `{said}`:\n{prose}");
    }
    assert_golden("predicates_and_toward", "prose.txt", &prose);
}

#[test]
fn s2s_constructs_round_trip_canonicalise_and_render_to_their_goldens() {
    // A Defend arm and an Attack arm with every field, an edit writing a
    // present `false`, an empty switch, and a wall target with its box.
    let text = defend_and_attack();
    let document = Document::parse(&text).expect("the S2 fixture parses");
    assert_eq!(document.to_text(), text, "load-and-save lost a byte");
    let canonical = canonicalise_text(&text).expect("the S2 fixture canonicalises");
    assert!(
        canonical.relocated.is_empty(),
        "nothing in the fixture is written at a default: {:?}",
        canonical.relocated
    );
    assert!(
        canonical.text.contains("\"breach_allowed\": false"),
        "a present `optional` false survives the canonical form"
    );
    assert_golden("defend_and_attack", "jsonc", &canonical.text);
    let again = canonicalise_text(&canonical.text).expect("the canonical text canonicalises");
    assert_eq!(
        again.text, canonical.text,
        "the canonical form is a fixed point"
    );
    let prose = render_plan(&canonical.playbook, &context(), &rules())
        .expect("the committed rules table is complete");
    for said in [
        "protecting beacon \"b_00\" and your commander",
        "engaging within 60% of the sphere",
        "with a picket of 2 scouts",
        "attacking beacon \"e_01\"",
        "launching once 6 raiders stand there or 1:30 of the segment have passed",
        "never breaching walls",
        "attacking the enemy commander",
    ] {
        assert!(prose.contains(said), "the prose says `{said}`:\n{prose}");
    }
    assert_golden("defend_and_attack", "prose.txt", &prose);
}

// ---------------------------------------------------------------------------
// 5. The chips (S3's plan, E8; task `gwr`)
// ---------------------------------------------------------------------------

/// Every case with a prose golden, by name, with its text.
fn rendered_cases() -> Vec<(&'static str, String)> {
    let case = |name: &str| {
        read(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("cases")
                .join(format!("{name}.jsonc")),
        )
    };
    vec![
        ("expand_east", example()),
        ("cover_the_nearest_vent", cover_the_nearest_vent()),
        ("defend_and_attack", defend_and_attack()),
        ("predicates_and_toward", case("predicates_and_toward")),
    ]
}

/// A shape as one line of the golden.
fn shape_text(shape: &Shape) -> String {
    let bound = |value: Option<i32>| value.map_or_else(String::new, |value| value.to_string());
    match shape {
        Shape::Integer { min, max, unit } => {
            let unit = if unit.is_empty() {
                String::new()
            } else {
                format!(" {unit}")
            };
            format!("integer {}..{}{unit}", bound(*min), bound(*max))
        }
        Shape::Choice { values } => format!("choice {}", values.join(" | ")),
        Shape::Reference { kind, arms } => {
            format!("reference {} [{}]", kind.proto_name(), arms.join(", "))
        }
    }
}

/// The rendering's rules and chips, one line a chip, each with the words it
/// marks, so a diff reads as which value moved where.
fn chips_text(rendering: &Rendering) -> String {
    let lines: Vec<&str> = rendering.prose.lines().collect();
    let mut out = String::new();
    for rule in &rendering.rules {
        let _ = writeln!(
            out,
            "{}  lines {}+{}",
            rule.pointer, rule.line, rule.line_count
        );
        for chip in &rule.chips {
            let line = lines
                .get(
                    usize::try_from(chip.line)
                        .unwrap_or_else(|error| panic!("a line index: {error:?}")),
                )
                .copied()
                .unwrap_or_else(|| panic!("chip line {} is not in the prose", chip.line));
            let marked: String = line
                .chars()
                .skip(
                    usize::try_from(chip.start).unwrap_or_else(|error| panic!("a span: {error:?}")),
                )
                .take(
                    usize::try_from(chip.end.saturating_sub(chip.start))
                        .unwrap_or_else(|error| panic!("a span: {error:?}")),
                )
                .collect();
            let _ = write!(
                out,
                "  {}:{}-{} \"{marked}\"\n    {}\n    {}\n    = {}\n",
                chip.line,
                chip.start,
                chip.end,
                chip.pointer,
                shape_text(&chip.shape),
                chip.value_json
            );
        }
    }
    out
}

/// The JSON value at an RFC 6901 pointer.
fn at_pointer<'a>(document: &'a Json, pointer: &str) -> Option<&'a Json> {
    let tokens = split_pointer(pointer).ok()?;
    let mut here = document;
    for token in tokens {
        here = match here {
            Json::Array(items) => items.get(token.parse::<usize>().ok()?)?,
            other => other.get(&token)?,
        };
    }
    Some(here)
}

/// The rendering's own promises, which hold for every playbook: each rule's
/// lines are inside the prose and follow one another, each chip stands on one
/// of its rule's lines with a non-empty span inside that line and after the
/// chip before it, and each chip's value is the value the file holds at its
/// pointer, so a `replace` there has a member to replace.
fn assert_chips_hold(case: &str, rendering: &Rendering, canonical_json: &str) {
    let document = pharmakos_proto::json::read(canonical_json)
        .unwrap_or_else(|error| panic!("canonical JSON reads: {error:?}"));
    let lines: Vec<&str> = rendering.prose.lines().collect();
    let mut previous_end = 0;
    for rule in &rendering.rules {
        assert!(
            rule.line_count >= 1,
            "{case}: {} takes no line",
            rule.pointer
        );
        assert!(
            rule.line >= previous_end,
            "{case}: {} starts inside the rule before it",
            rule.pointer
        );
        previous_end = rule.line.saturating_add(rule.line_count);
        assert!(
            usize::try_from(previous_end).unwrap_or_else(|error| panic!("a line: {error:?}"))
                <= lines.len(),
            "{case}: {} runs past the prose",
            rule.pointer
        );
        let mut last = (0, 0);
        for chip in &rule.chips {
            assert!(
                chip.line >= rule.line && chip.line < previous_end,
                "{case}: {} stands outside {}",
                chip.pointer,
                rule.pointer
            );
            assert!(
                chip.pointer.starts_with(&rule.pointer),
                "{case}: {} is not inside {}",
                chip.pointer,
                rule.pointer
            );
            let width = lines
                .get(usize::try_from(chip.line).unwrap_or_else(|error| panic!("a line: {error:?}")))
                .map_or(0, |line| line.chars().count());
            assert!(
                chip.start < chip.end
                    && usize::try_from(chip.end)
                        .unwrap_or_else(|error| panic!("a span: {error:?}"))
                        <= width,
                "{case}: {} spans {}..{} of a {width}-character line",
                chip.pointer,
                chip.start,
                chip.end
            );
            assert!(
                (chip.line, chip.start) >= last,
                "{case}: {} overlaps the chip before it",
                chip.pointer
            );
            last = (chip.line, chip.end);
            let held = at_pointer(&document, &chip.pointer)
                .unwrap_or_else(|| panic!("{case}: the file holds nothing at {}", chip.pointer));
            let value = pharmakos_proto::json::read(&chip.value_json)
                .unwrap_or_else(|error| panic!("{case}: {} is not JSON: {error:?}", chip.pointer));
            assert_eq!(
                &value, held,
                "{case}: {}'s value is not the file's",
                chip.pointer
            );
            if let Shape::Choice { values } = &chip.shape {
                assert!(
                    values.contains(&chip.value_json),
                    "{case}: {} holds {}, which its choices do not list",
                    chip.pointer,
                    chip.value_json
                );
            }
        }
    }
}

#[test]
fn every_rendered_case_has_its_chips_golden_and_keeps_their_promises() {
    for (case, text) in rendered_cases() {
        let canonical = canonicalise_text(&text)
            .unwrap_or_else(|error| panic!("the case canonicalises: {error:?}"));
        let rendering = render(&canonical.playbook, &context(), &rules())
            .unwrap_or_else(|error| panic!("the committed rules table is complete: {error:?}"));
        let prose = render_plan(&canonical.playbook, &context(), &rules())
            .unwrap_or_else(|error| panic!("a rendering: {error:?}"));
        assert_eq!(rendering.prose, prose, "{case}: the chips moved the prose");
        assert!(
            rendering.rules.iter().any(|rule| !rule.chips.is_empty()),
            "{case}: no chip at all"
        );
        assert_chips_hold(case, &rendering, &canonical.json);
        assert_golden(case, "chips.txt", &chips_text(&rendering));
    }
}

/// The verifier at QUICK over `text`, as the paths of its diagnostics.
fn diagnosed_paths(text: &str) -> Vec<String> {
    let scope = Scope::new(
        pharmakos_sim::tables::SeatId::new(0),
        pharmakos_sim::knowledge::SeatEconomy {
            treasury: pharmakos_sim::math::quantity::Money::new(200),
            supply: pharmakos_sim::math::quantity::Kw::new(10),
            draw: pharmakos_sim::math::quantity::Kw::new(2),
        },
    );
    verify_jsonc(text, &[], &scope, &rules(), Depth::Quick)
        .unwrap_or_else(|error| panic!("the committed rules table is complete: {error:?}"))
        .diagnostics
        .into_iter()
        .map(|diagnostic| diagnostic.path)
        .collect()
}

/// `text` with the value at `pointer` replaced by `value`.
fn replaced(text: &str, pointer: &str, value: i64) -> String {
    let patch = format!(r#"[{{"op":"replace","path":"{pointer}","value":{value}}}]"#);
    patch_text(text, &patch)
        .unwrap_or_else(|error| panic!("{pointer} takes a replace: {error}"))
        .0
}

/// S3's plan, E8: an integer chip's bounds are the ones the verifier
/// enforces at its pointer. One step past a bound the chip states draws a
/// diagnostic there, and the bound itself draws none there; and a side the
/// chip leaves open draws none even far out, so a check the verifier gains
/// later shows here as a bound the chip does not state.
#[test]
fn an_integer_chips_bounds_are_the_verifiers() {
    for (case, text) in rendered_cases() {
        let canonical = canonicalise_text(&text)
            .unwrap_or_else(|error| panic!("the case canonicalises: {error:?}"));
        let rendering = render(&canonical.playbook, &context(), &rules())
            .unwrap_or_else(|error| panic!("a rendering: {error:?}"));
        for chip in rendering.rules.iter().flat_map(|rule| &rule.chips) {
            let Shape::Integer { min, max, .. } = chip.shape else {
                continue;
            };
            let flagged = |value: i64| {
                diagnosed_paths(&replaced(&canonical.text, &chip.pointer, value))
                    .contains(&chip.pointer)
            };
            match min {
                Some(least) => {
                    assert!(
                        flagged(i64::from(least) - 1),
                        "{case}: {} below its least, {least}, draws nothing there",
                        chip.pointer
                    );
                    assert!(
                        !flagged(i64::from(least)),
                        "{case}: {} at its least, {least}, is refused",
                        chip.pointer
                    );
                }
                None => assert!(
                    !flagged(-1_000_000),
                    "{case}: {} is refused far below, and its chip states no least",
                    chip.pointer
                ),
            }
            match max {
                Some(most) => {
                    assert!(
                        flagged(i64::from(most) + 1),
                        "{case}: {} above its most, {most}, draws nothing there",
                        chip.pointer
                    );
                    assert!(
                        !flagged(i64::from(most)),
                        "{case}: {} at its most, {most}, is refused",
                        chip.pointer
                    );
                }
                None => assert!(
                    !flagged(1_000_000),
                    "{case}: {} is refused far above, and its chip states no most",
                    chip.pointer
                ),
            }
        }
    }
}
