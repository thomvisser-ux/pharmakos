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
//! 6. From S3's `gwu`, `expected.slots.txt` for each rendered case and for
//!    `condition_slots`: every place a condition stands or could stand, and a
//!    check that the walk lists every place the schema puts one.
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
use pharmakos_plan_core::render::{PickOp, Rendering, render, render_plan};
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
        // S3's `fixq`: values at their defaults, chipped, some outside.
        ("defaults", case("defaults")),
    ]
}

/// A shape as one line of the golden.
fn shape_text(shape: &Shape) -> String {
    let bound = |value: Option<i32>| value.map_or_else(String::new, |value| value.to_string());
    match shape {
        Shape::Integer {
            min,
            max,
            unit,
            zero,
        } => {
            let shown = pharmakos_plan_core::shape::display_unit(unit);
            let unit = if unit.is_empty() {
                String::new()
            } else if shown == *unit {
                format!(" {unit}")
            } else {
                format!(" {unit}, shown in {shown}")
            };
            // S3's `zerotitle`: what a 0 does, where the shape has words for it.
            let zero = zero.map_or_else(String::new, |zero| format!(", 0 is \"{zero}\""));
            format!("integer {}..{}{unit}{zero}", bound(*min), bound(*max))
        }
        Shape::Choice { options } => format!(
            "choice {}",
            options
                .iter()
                .map(|option| format!("{} ({})", option.json, option.title))
                .collect::<Vec<_>>()
                .join(" | ")
        ),
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
                "  {}:{}-{} \"{marked}\"\n    {}\n    {}\n    = {}{}{}{}, {}\n",
                chip.line,
                chip.start,
                chip.end,
                chip.pointer,
                shape_text(&chip.shape),
                chip.value_json,
                // S3's `fixq`: the value as typed, whether it is outside its
                // shape, and the op an edit sends.
                if chip.value_text.is_empty() {
                    String::new()
                } else {
                    format!(" (typed {})", chip.value_text)
                },
                // S3's `zerotitle`: the value's own words, where it has them.
                chip.value_title
                    .map_or_else(String::new, |title| format!(" \"{title}\"")),
                if chip.outside { ", outside" } else { "" },
                chip.pick.proto_name().to_lowercase()
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
            let value = pharmakos_proto::json::read(&chip.value_json)
                .unwrap_or_else(|error| panic!("{case}: {} is not JSON: {error:?}", chip.pointer));
            // A chip the file writes is the file's value, and its edit
            // replaces it; a chip at its default (S3's `fixq`, item 11) adds,
            // and the file holds that default or nothing there.
            match (at_pointer(&document, &chip.pointer), chip.pick) {
                (Some(held), _) => assert_eq!(
                    &value, held,
                    "{case}: {}'s value is not the file's",
                    chip.pointer
                ),
                (None, PickOp::Add) => {}
                (None, PickOp::Replace) => {
                    panic!("{case}: the file holds nothing at {}", chip.pointer)
                }
            }
            // `outside` is exactly a choice's value not listed, or an
            // integer past a bound (S3's `fixq`, item 14).
            match &chip.shape {
                Shape::Choice { options } => assert_eq!(
                    chip.outside,
                    !options.iter().any(|option| option.json == chip.value_json),
                    "{case}: {} holds {}, and says outside {}",
                    chip.pointer,
                    chip.value_json,
                    chip.outside
                ),
                Shape::Integer { min, max, .. } => {
                    let number: i64 = chip.value_json.parse().unwrap_or_else(|error| {
                        panic!("{case}: {} is no number: {error:?}", chip.pointer)
                    });
                    let past = min.is_some_and(|min| number < i64::from(min))
                        || max.is_some_and(|max| number > i64::from(max));
                    assert_eq!(chip.outside, past, "{case}: {}", chip.pointer);
                }
                Shape::Reference { .. } => assert!(!chip.outside, "{case}: {}", chip.pointer),
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

// ---------------------------------------------------------------------------
// 6. The condition slots (S3's task `gwu`)
// ---------------------------------------------------------------------------

/// The case that holds every kind of condition slot: filled, empty, a group
/// and its items, a body step's guard, a rule with no `when`.
fn condition_slots_case() -> String {
    read(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("cases")
            .join("condition_slots.jsonc"),
    )
}

/// The rendering's condition slots, one line a slot: its rule's or step's
/// pointer, the slot's pointer, its state, its predicate (`-` for none), the
/// op a pick there sends (`-` for a group), its line and its title.
fn slots_text(rendering: &Rendering) -> String {
    let mut out = String::new();
    for rule in &rendering.rules {
        for slot in &rule.slots {
            let predicate = if slot.predicate.is_empty() {
                "-"
            } else {
                slot.predicate
            };
            let _ = writeln!(
                out,
                "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                rule.pointer,
                slot.pointer,
                slot.state.proto_name(),
                predicate,
                slot.pick.map_or("-", PickOp::proto_name),
                slot.line,
                slot.title
            );
        }
    }
    out
}

/// The cases the slots golden covers: every rendered case, and the one built
/// to hold every kind of slot.
fn slot_cases() -> Vec<(&'static str, String)> {
    let mut cases = rendered_cases();
    cases.push(("condition_slots", condition_slots_case()));
    cases
}

/// Each case's slots, against `expected.slots.txt`, and the promises a slot
/// keeps whatever the case: it sits on its own rule's line; its pointer is
/// under its rule's; a filled slot's pointer names a member the canonical
/// file has, holding the arm the slot names, and an empty one names a member
/// the file leaves out or writes with no arm; every route step has its
/// `skip_if`; a group's items follow it.
#[test]
fn every_case_has_its_slots_golden_and_each_slot_names_its_place() {
    use pharmakos_plan_core::render::SlotState;
    for (case, text) in slot_cases() {
        let canonical = canonicalise_text(&text)
            .unwrap_or_else(|error| panic!("{case} canonicalises: {error:?}"));
        let rendering = render(&canonical.playbook, &context(), &rules())
            .unwrap_or_else(|error| panic!("the committed rules table is complete: {error:?}"));
        let document = pharmakos_proto::json::read(&canonical.json)
            .unwrap_or_else(|error| panic!("canonical JSON reads: {error:?}"));
        for rule in &rendering.rules {
            if rule.pointer.starts_with("/declarative/route/") {
                assert!(
                    rule.slots
                        .first()
                        .is_some_and(|slot| slot.pointer == format!("{}/skip_if", rule.pointer)),
                    "{case}: {} lists its skip_if first",
                    rule.pointer
                );
            }
            for (index, slot) in rule.slots.iter().enumerate() {
                assert_eq!(slot.line, rule.line, "{case}: {}", slot.pointer);
                assert!(
                    slot.pointer.starts_with(&format!("{}/", rule.pointer)),
                    "{case}: {} under {}",
                    slot.pointer,
                    rule.pointer
                );
                let held = at_pointer(&document, &slot.pointer);
                let arm = match held {
                    Some(Json::Object(members)) => members.first().map(|(key, _)| key.as_str()),
                    _ => None,
                };
                match slot.state {
                    SlotState::Empty => {
                        assert!(arm.is_none(), "{case}: {} is empty", slot.pointer);
                        assert!(slot.predicate.is_empty(), "{case}: {}", slot.pointer);
                    }
                    SlotState::Leaf | SlotState::Group => {
                        assert_eq!(arm, Some(slot.predicate), "{case}: {}", slot.pointer);
                    }
                }
                let pick = match (slot.state, held) {
                    (SlotState::Group, _) => None,
                    (_, Some(_)) => Some(PickOp::Replace),
                    (_, None) => Some(PickOp::Add),
                };
                assert_eq!(slot.pick, pick, "{case}: {} names its op", slot.pointer);
                if slot.state == SlotState::Group {
                    let next = rule.slots.get(index.saturating_add(1));
                    assert!(
                        next.is_some_and(|item| item
                            .pointer
                            .starts_with(&format!("{}/{}/", slot.pointer, slot.predicate))),
                        "{case}: {}'s items follow it",
                        slot.pointer
                    );
                }
            }
        }
        assert_golden(case, "slots.txt", &slots_text(&rendering));
    }
}

/// Every place `playbook.proto` puts a `gp.v1.Condition` is one the slots
/// walk lists: the schema's own fields of that type, read from the
/// descriptor set, against a table of where each one's slot points, and the
/// `condition_slots` case holds a slot at each. A new place a condition can
/// stand fails here until the walk lists it.
#[test]
fn the_slots_walk_lists_every_place_the_schema_puts_a_condition() {
    // (message.field, the tail of a slot pointer that stands there)
    let table: [(&str, &str); 6] = [
        ("gp.v1.Handler.when", "/when"),
        ("gp.v1.Step.skip_if", "/skip_if"),
        ("gp.v1.WaitUntilStep.condition", "/wait_until/condition"),
        ("gp.v1.All.items", "/all/items/"),
        ("gp.v1.Any.items", "/any/items/"),
        ("gp.v1.Not.item", "/not/item"),
    ];
    let mut in_schema: Vec<String> = pharmakos_proto::descriptor::schema()
        .messages()
        .flat_map(|message| {
            message
                .fields
                .iter()
                .filter(|field| field.type_name == "gp.v1.Condition")
                .map(|field| format!("{}.{}", message.full_name, field.name))
        })
        .collect();
    in_schema.sort();
    let mut listed: Vec<String> = table.iter().map(|(name, _)| (*name).to_owned()).collect();
    listed.sort();
    assert_eq!(
        in_schema, listed,
        "the schema puts a condition somewhere the slots walk does not list"
    );

    let canonical = canonicalise_text(&condition_slots_case())
        .unwrap_or_else(|error| panic!("the case canonicalises: {error:?}"));
    let rendering = render(&canonical.playbook, &context(), &rules())
        .unwrap_or_else(|error| panic!("the committed rules table is complete: {error:?}"));
    let pointers: Vec<&str> = rendering
        .rules
        .iter()
        .flat_map(|rule| rule.slots.iter().map(|slot| slot.pointer.as_str()))
        .collect();
    for (name, tail) in table {
        assert!(
            pointers.iter().any(|pointer| {
                pointer.ends_with(tail)
                    || (tail.ends_with('/')
                        && pointer
                            .rsplit_once('/')
                            .is_some_and(|(head, _)| format!("{head}/").ends_with(tail)))
            }),
            "no slot of the case stands at {name}: {pointers:?}"
        );
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
fn replaced(text: &str, pointer: &str, op: PickOp, value: i64) -> String {
    // The chip's own op (S3's `fixq`): `add` for one at its default.
    let op = op.proto_name().to_lowercase();
    let patch = format!(r#"[{{"op":"{op}","path":"{pointer}","value":{value}}}]"#);
    patch_text(text, &patch)
        .unwrap_or_else(|error| panic!("{pointer} takes an {op}: {error}"))
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
                diagnosed_paths(&replaced(&canonical.text, &chip.pointer, chip.pick, value))
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

/// Every committed playbook a slot can be read from: the slot cases, the
/// worked example, and every scenario playbook, by path.
#[allow(
    clippy::disallowed_methods,
    reason = "each folder's listing is collected and sorted before it is used, as the lint asks"
)]
fn committed_playbooks() -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = slot_cases()
        .into_iter()
        .map(|(case, text)| (case.to_owned(), text))
        .collect();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let scenarios = root.join("scenarios");
    let mut folders = vec![scenarios];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .collect();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                folders.push(path);
            } else if path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(".playbook.jsonc"))
            {
                found.push((path.display().to_string(), read(&path)));
            }
        }
    }
    found
}

/// The verifier at QUICK over `text`, as `(code, path)` pairs.
fn diagnosed(text: &str) -> Vec<(String, String)> {
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
        .map(|diagnostic| (diagnostic.code, diagnostic.path))
        .collect()
}

/// The verifier's codes for a condition missing where one is required: a
/// rule with no `when` (`E0106`) and a `not` with no item (`E0203`). A pick
/// that fills the slot answers them, at the place above it they are raised.
const MISSING_CONDITION: [&str; 2] = ["E0106", "E0203"];

/// S3's `gwu`, fix pass 1 (the review's C1, K1): for every pickable slot of
/// every committed playbook, the op the slot names, sent with a vocabulary
/// entry's skeleton, fills or replaces that slot and nothing else. The
/// re-rendered rule has as many slots as before, every one where it was and
/// as it was but the picked one, now the new leaf; and the verifier draws
/// nothing new but at the slot, where the new leaf may draw its own, and
/// drops nothing but at the slot or above it, where a complaint about the
/// missing condition (a rule with no `when`, a `not` with no item) is the
/// pick's to answer.
/// An `add` at an item of a group would insert and fail here.
#[test]
fn a_pick_with_the_op_its_slot_names_changes_that_slot_alone() {
    use pharmakos_plan_core::render::SlotState;
    let words = pharmakos_plan_core::vocabulary::vocabulary(&[])
        .unwrap_or_else(|error| panic!("the vocabulary: {error:?}"));
    let found = words
        .families
        .iter()
        .flat_map(|family| &family.predicates)
        .find(|entry| entry.name == "treasury");
    let Some(skeleton) = found.map(|entry| entry.skeleton_json.clone()) else {
        panic!("the vocabulary has treasury");
    };
    let mut picked = 0_usize;
    for (case, text) in committed_playbooks() {
        let Ok(canonical) = canonicalise_text(&text) else {
            continue;
        };
        let rendering = render(&canonical.playbook, &context(), &rules())
            .unwrap_or_else(|error| panic!("{case}: a rendering: {error:?}"));
        let before = diagnosed(&text);
        for (rule_index, rule) in rendering.rules.iter().enumerate() {
            for (slot_index, slot) in rule.slots.iter().enumerate() {
                let Some(op) = slot.pick else {
                    assert_eq!(slot.state, SlotState::Group, "{case}: {}", slot.pointer);
                    continue;
                };
                let op = match op {
                    PickOp::Add => "add",
                    PickOp::Replace => "replace",
                };
                let patch = format!(
                    r#"[{{"op":"{op}","path":"{}","value":{{"treasury":{skeleton}}}}}]"#,
                    slot.pointer
                );
                let (patched, _) = patch_text(&text, &patch)
                    .unwrap_or_else(|error| panic!("{case}: {op} at {}: {error:?}", slot.pointer));
                let again = canonicalise_text(&patched)
                    .unwrap_or_else(|error| panic!("{case}: {}: {error:?}", slot.pointer));
                let after = render(&again.playbook, &context(), &rules())
                    .unwrap_or_else(|error| panic!("{case}: {}: {error:?}", slot.pointer));
                let rule_after = after
                    .rules
                    .get(rule_index)
                    .unwrap_or_else(|| panic!("{case}: {} lost its rule", slot.pointer));
                assert_eq!(
                    rule_after.slots.len(),
                    rule.slots.len(),
                    "{case}: {}",
                    slot.pointer
                );
                for (index, (was, now)) in rule.slots.iter().zip(&rule_after.slots).enumerate() {
                    if index == slot_index {
                        assert_eq!(
                            (now.pointer.as_str(), now.state, now.predicate),
                            (slot.pointer.as_str(), SlotState::Leaf, "treasury"),
                            "{case}: the picked slot"
                        );
                    } else {
                        assert_eq!(
                            (&now.pointer, now.state, now.predicate),
                            (&was.pointer, was.state, was.predicate),
                            "{case}: picking {} moved another slot",
                            slot.pointer
                        );
                    }
                }
                let now = diagnosed(&patched);
                let gained: Vec<&(String, String)> =
                    now.iter().filter(|found| !before.contains(found)).collect();
                let lost: Vec<&(String, String)> =
                    before.iter().filter(|found| !now.contains(found)).collect();
                // By path segment: `/items/1` holds `/items/1/..`, never
                // `/items/10`.
                let under = |path: &str, base: &str| {
                    path == base
                        || path
                            .strip_prefix(base)
                            .is_some_and(|rest| rest.starts_with('/'))
                };
                assert!(
                    gained.iter().all(|(_, path)| under(path, &slot.pointer)),
                    "{case}: picking {} drew {gained:?} elsewhere",
                    slot.pointer
                );
                // Lost: what the new leaf answers at or under the slot, and,
                // above it, only the codes that say this slot's own
                // condition was missing: a rule with no `when` (`E0106`), a
                // `not` with no item (`E0203`).
                assert!(
                    lost.iter().all(|(code, path)| under(path, &slot.pointer)
                        || (MISSING_CONDITION.contains(&code.as_str())
                            && under(&slot.pointer, path))),
                    "{case}: picking {} answered {lost:?} elsewhere",
                    slot.pointer
                );
                picked = picked.saturating_add(1);
            }
        }
    }
    assert!(picked > 20, "the property ran over {picked} picks");
}
