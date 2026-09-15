// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What seal inspection promises, asserted.
//!
//! Two of these tests write golden `actual.*` files into
//! `<target>/golden/verifier/`, where `cargo xtask ci`'s `golden` step compares
//! them byte for byte with `tests/golden/verifier/` at the workspace root.
//! `cargo xtask golden --bless` accepts a move, and a blessed golden has to be
//! explained in the pull request that moved it.
//!
//! # The fixture
//!
//! One scope serves every case, and it is written out in [`fixture_scope`]: seat
//! 0, a core `b_01` on BUILD, a mining beacon `b_02` tagged `east`, and one
//! enemy beacon `e_01` the seat has seen. One snapshot serves every case too —
//! a real `pharmakos_sim::snapshot::Snapshot`, encoded, because the bytes are a
//! hashed input and a made-up byte string would not be one.
//!
//! Sharing the fixture is deliberate. A case's job is to isolate **one
//! diagnostic**, and a per-case scope would make each report a function of two
//! things that changed instead of one.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use pharmakos_proto::gp::api::v1::VerifyReport;
use pharmakos_proto::gp::api::v1::diagnostic::Severity;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::gp::v1::beacon_filter::MandateKind;
use pharmakos_proto::json;
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::math::quantity::{Kw, Money};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::snapshot::{SNAPSHOT_VERSION, Snapshot};
use pharmakos_sim::tables::SeatId;
use pharmakos_verifier::catalogue::{CATALOGUE, Emitter, catalogue_json};
use pharmakos_verifier::{Input, KnownBeacon, Ownership, Scope, VERIFIER_VERSION, hash, verify};

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// The workspace root: this crate is `<root>/crates/verifier`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| panic!("crates/verifier sits two levels below the workspace root"))
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

fn cases_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("cases")
}

fn golden_dir() -> PathBuf {
    workspace_root()
        .join("tests")
        .join("golden")
        .join("verifier")
}

/// Every case, by name, in path order.
///
/// `std::fs::read_dir` is on `clippy.toml`'s disallowed list because directory
/// order differs between ext4, NTFS and APFS. The sanctioned use is exactly this
/// one, and `clippy.toml`'s own reason string says so: collect, sort, then use.
#[allow(
    clippy::disallowed_methods,
    reason = "clippy.toml sanctions read_dir when the listing is collected and sorted before use, which is what happens here"
)]
fn cases() -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(cases_dir())
        .unwrap_or_else(|error| panic!("reading {}: {error}", cases_dir().display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .filter_map(|path| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .collect();
    names.sort();
    assert!(
        names.len() > 40,
        "the case walk found only {} files; it is looking in the wrong place and would pass \
         vacuously",
        names.len()
    );
    names
}

fn write_actual(case: &str, contents: &str) {
    let directory = target_dir().join("golden").join("verifier").join(case);
    fs::create_dir_all(&directory)
        .unwrap_or_else(|error| panic!("creating {}: {error}", directory.display()));
    fs::write(directory.join("actual.report.json"), contents)
        .unwrap_or_else(|error| panic!("writing {case}'s fresh report: {error}"));
}

fn read_expected(case: &str) -> Option<String> {
    fs::read_to_string(golden_dir().join(case).join("expected.report.json")).ok()
}

/// A readable first difference, so a failure names a byte rather than dumping
/// two files at the reader.
fn first_difference(expected: &str, actual: &str) -> String {
    let mut index = 0_usize;
    let left = expected.as_bytes();
    let right = actual.as_bytes();
    while left.get(index) == right.get(index) && left.get(index).is_some() {
        index = index.saturating_add(1);
    }
    let line = expected
        .get(..index)
        .map_or(1, |head| head.lines().count().max(1));
    format!(
        "byte {index} (line {line}):\n  expected: {:?}\n    actual: {:?}",
        expected.get(index..index.saturating_add(70)),
        actual.get(index..index.saturating_add(70)),
    )
}

// ---------------------------------------------------------------------------
// The fixture
// ---------------------------------------------------------------------------

fn rules() -> RulesTable {
    RulesTable::load(&workspace_root().join("rules").join("rules.v1.json"))
        .unwrap_or_else(|error| panic!("reading the rules table: {error}"))
}

/// The seat's frozen planning snapshot, as real snapshot bytes.
fn fixture_snapshot() -> Vec<u8> {
    Snapshot {
        version: SNAPSHOT_VERSION,
        match_seed: 0x0102_0304_0506_0708,
        tick: 3_600,
        ..Snapshot::default()
    }
    .to_bytes()
    .unwrap_or_else(|error| panic!("encoding the fixture snapshot: {error}"))
}

fn beacon(
    id: &str,
    side: Ownership,
    mandate: MandateKind,
    at: (i32, i32, i32),
    is_core: bool,
    tags: &[&str],
) -> KnownBeacon {
    KnownBeacon {
        beacon_id: id.to_owned(),
        owner: if side == Ownership::EnemyKnown {
            SeatId::new(1)
        } else {
            SeatId::new(0)
        },
        side,
        mandate,
        tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
        at: Voxel {
            x: at.0,
            y: at.1,
            z: at.2,
        },
        is_core,
    }
}

/// The seat's view every case is checked against.
fn fixture_scope() -> Scope {
    Scope::new(
        SeatId::new(0),
        SeatEconomy {
            treasury: Money::new(200),
            supply: Kw::new(10),
            draw: Kw::new(4),
        },
    )
    .with_beacon(beacon(
        "b_01",
        Ownership::Own,
        MandateKind::Build,
        (80, 11, 55),
        true,
        &[],
    ))
    .with_beacon(beacon(
        "b_02",
        Ownership::Own,
        MandateKind::Mine,
        (100, 20, 58),
        false,
        &["east"],
    ))
    .with_beacon(beacon(
        "e_01",
        Ownership::EnemyKnown,
        MandateKind::Unspecified,
        (300, 300, 40),
        false,
        &[],
    ))
}

fn case_bytes(case: &str) -> Vec<u8> {
    let path = cases_dir().join(format!("{case}.json"));
    fs::read(&path).unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

fn report_for(case: &str, depth: Depth) -> VerifyReport {
    let rules = rules();
    let scope = fixture_scope();
    let snapshot = fixture_snapshot();
    let playbook = case_bytes(case);
    let input = Input::new(&playbook, &snapshot, &scope, &rules)
        .unwrap_or_else(|error| panic!("assembling the input: {error}"));
    verify(&input, depth)
}

/// One report as the canonical JSON the gateway would hand back.
fn report_text(case: &str) -> String {
    let report = report_for(case, Depth::Full);
    let mut text = json::encode(&report)
        .unwrap_or_else(|error| panic!("{case}: encoding the report: {error}"));
    text.push('\n');
    text
}

// ---------------------------------------------------------------------------
// The goldens
// ---------------------------------------------------------------------------

#[test]
fn every_case_has_its_committed_report() {
    let mut failures: Vec<String> = Vec::new();
    for case in cases() {
        let actual = report_text(&case);
        write_actual(&case, &actual);
        match read_expected(&case) {
            Some(expected) if expected == actual => {}
            Some(expected) => failures.push(format!(
                "{case} differs from its golden at {}",
                first_difference(&expected, &actual)
            )),
            None => failures.push(format!(
                "{case}: nothing committed at tests/golden/verifier/{case}/expected.report.json"
            )),
        }
    }
    assert!(
        failures.is_empty(),
        "verifier reports moved. A report that moved is a behaviour change; say which in the \
         pull request, and see tests/golden/verifier/README.md.\n{}",
        failures.join("\n")
    );
}

#[test]
fn the_catalogue_is_the_committed_one() {
    let mut actual = json::write(&catalogue_json());
    actual.push('\n');
    let directory = target_dir().join("golden").join("verifier");
    fs::create_dir_all(&directory)
        .unwrap_or_else(|error| panic!("creating {}: {error}", directory.display()));
    fs::write(directory.join("actual.catalogue.json"), &actual)
        .unwrap_or_else(|error| panic!("writing the fresh catalogue: {error}"));

    let path = golden_dir().join("expected.catalogue.json");
    let expected = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
    assert!(
        expected == actual,
        "the diagnostic catalogue moved. Code numbers are append-only from the day they ship: a \
         renamed or renumbered code is a breaking change to every saved report.\n{}",
        first_difference(&expected, &actual)
    );
}

// ---------------------------------------------------------------------------
// Coverage
// ---------------------------------------------------------------------------

#[test]
fn every_emitted_code_has_a_case() {
    let mut seen: Vec<String> = Vec::new();
    for case in cases() {
        for diagnostic in report_for(&case, Depth::Full).diagnostics {
            if !seen.contains(&diagnostic.code) {
                seen.push(diagnostic.code);
            }
        }
    }
    let mut missing: Vec<&str> = Vec::new();
    for row in CATALOGUE {
        let emitted = matches!(row.emitter, Emitter::Stage(_));
        let covered = seen.iter().any(|code| code == row.code);
        if emitted && !covered {
            missing.push(row.code);
        }
        assert!(
            emitted || !covered,
            "{} is in the catalogue as unemitted, but a case produced it",
            row.code
        );
    }
    assert!(
        missing.is_empty(),
        "these codes claim an emitter and no case produces one: {missing:?}"
    );
}

#[test]
fn nothing_outside_the_catalogue_is_ever_emitted() {
    for case in cases() {
        for diagnostic in report_for(&case, Depth::Full).diagnostics {
            assert!(
                CATALOGUE.iter().any(|row| row.code == diagnostic.code),
                "{case} produced `{}`, which is not in the catalogue",
                diagnostic.code
            );
            assert!(!diagnostic.message.is_empty(), "{case}: an empty message");
            assert!(!diagnostic.beginner.is_empty(), "{case}: no beginner line");
            assert!(
                !diagnostic.message.contains('{'),
                "{case}: `{}` left a template placeholder unfilled: {}",
                diagnostic.code,
                diagnostic.message
            );
            assert!(
                diagnostic.path.is_empty() || diagnostic.path.starts_with('/'),
                "{case}: `{}` has a path that is not a JSON Pointer: {}",
                diagnostic.code,
                diagnostic.path
            );
            for suggestion in &diagnostic.suggestions {
                assert!(
                    json::read(&suggestion.json_patch).is_ok(),
                    "{case}: `{}` carries a suggestion that is not JSON",
                    diagnostic.code
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Depth, and the two empty FULL stages
// ---------------------------------------------------------------------------

#[test]
fn full_finds_what_quick_finds() {
    // Item 82: FULL's estimate and lint stages are present and empty, so today a
    // FULL report carries exactly QUICK's diagnostics. When S1 and S3 fill them
    // this test is the one that says so, and the report goldens move with it.
    for case in cases() {
        let quick = report_for(&case, Depth::Quick);
        let full = report_for(&case, Depth::Full);
        assert_eq!(
            quick.diagnostics, full.diagnostics,
            "{case}: FULL found something QUICK did not, but both of FULL's extra stages are \
             empty (decisions-log item 82)"
        );
        assert_eq!(quick.qualifies, full.qualifies, "{case}");
        assert_ne!(
            quick.report_hash, full.report_hash,
            "{case}: the depth is one of the five hashed inputs, so two depths never share a hash"
        );
    }
}

#[test]
fn an_unspecified_depth_is_read_as_full() {
    let unspecified = report_for("expand_east", Depth::Unspecified);
    let full = report_for("expand_east", Depth::Full);
    assert_eq!(unspecified.depth, i32::from(Depth::Full));
    assert_eq!(unspecified.report_hash, full.report_hash);
}

// ---------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------

#[test]
fn a_report_is_the_same_bytes_twice() {
    for case in cases() {
        assert_eq!(
            report_text(&case),
            report_text(&case),
            "{case}: two runs over the same five inputs produced different bytes"
        );
    }
}

#[test]
fn report_hash_moves_with_the_playbook_the_snapshot_and_the_scope() {
    let rules = rules();
    let scope = fixture_scope();
    let snapshot = fixture_snapshot();
    let playbook = case_bytes("expand_east");

    let baseline = hash::report_hash(
        &playbook,
        &snapshot,
        &scope,
        rules.rules_hash(),
        VERIFIER_VERSION,
        Depth::Full,
    );

    // Nothing moved.
    assert_eq!(
        baseline,
        hash::report_hash(
            &playbook,
            &snapshot,
            &scope,
            rules.rules_hash(),
            VERIFIER_VERSION,
            Depth::Full,
        )
    );

    // 1. the playbook bytes.
    let mut other_playbook = playbook.clone();
    other_playbook.push(b' ');
    assert_ne!(
        baseline,
        hash::report_hash(
            &other_playbook,
            &snapshot,
            &scope,
            rules.rules_hash(),
            VERIFIER_VERSION,
            Depth::Full,
        )
    );

    // 2a. the snapshot bytes.
    let other_snapshot = Snapshot {
        version: SNAPSHOT_VERSION,
        match_seed: 0x0102_0304_0506_0708,
        tick: 3_601,
        ..Snapshot::default()
    }
    .to_bytes()
    .expect("encoding");
    assert_ne!(
        baseline,
        hash::report_hash(
            &playbook,
            &other_snapshot,
            &scope,
            rules.rules_hash(),
            VERIFIER_VERSION,
            Depth::Full,
        )
    );

    // 2b. the seat's view of that snapshot.
    let other_scope = fixture_scope().with_beacon(beacon(
        "b_03",
        Ownership::Own,
        MandateKind::Defend,
        (90, 15, 56),
        false,
        &[],
    ));
    assert_ne!(
        baseline,
        hash::report_hash(
            &playbook,
            &snapshot,
            &other_scope,
            rules.rules_hash(),
            VERIFIER_VERSION,
            Depth::Full,
        )
    );
}

#[test]
fn report_hash_moves_when_the_rules_table_moves() {
    // Input 3. Item 89: `rules_hash` covers the **whole** canonical table,
    // `note` included, so a tuning pull request cannot change what the verifier
    // compares against while leaving every cached report where it was.
    let rules = rules();
    let scope = fixture_scope();
    let snapshot = fixture_snapshot();
    let playbook = case_bytes("expand_east");
    let baseline = hash::report_hash(
        &playbook,
        &snapshot,
        &scope,
        rules.rules_hash(),
        VERIFIER_VERSION,
        Depth::Full,
    );

    let mut message = rules.message().clone();
    message.note.push_str(" (moved for a test)");
    let other_rules = RulesTable::from_message(&message).expect("a rules table");
    assert_ne!(rules.rules_hash(), other_rules.rules_hash());
    assert_ne!(
        baseline,
        hash::report_hash(
            &playbook,
            &snapshot,
            &scope,
            other_rules.rules_hash(),
            VERIFIER_VERSION,
            Depth::Full,
        )
    );
}

#[test]
fn report_hash_moves_with_the_verifier_version_and_the_depth() {
    // Inputs 4 and 5. "Two reports compare only within one depth", and a report
    // is only comparable with another report from the same build.
    let rules = rules();
    let scope = fixture_scope();
    let snapshot = fixture_snapshot();
    let playbook = case_bytes("expand_east");
    let baseline = hash::report_hash(
        &playbook,
        &snapshot,
        &scope,
        rules.rules_hash(),
        VERIFIER_VERSION,
        Depth::Full,
    );

    assert_ne!(
        baseline,
        hash::report_hash(
            &playbook,
            &snapshot,
            &scope,
            rules.rules_hash(),
            "0.0.0-not-this-build",
            Depth::Full,
        )
    );
    assert_ne!(
        baseline,
        hash::report_hash(
            &playbook,
            &snapshot,
            &scope,
            rules.rules_hash(),
            VERIFIER_VERSION,
            Depth::Quick,
        )
    );
}

#[test]
fn the_report_records_the_hashes_it_was_produced_under() {
    let rules = rules();
    let report = report_for("expand_east", Depth::Full);
    assert_eq!(report.report_hash.len(), 8, "eight bytes, big-endian");
    assert_eq!(report.rules_hash, rules.rules_hash().to_be_bytes());
    assert_eq!(report.plan_fingerprint.len(), 8);
    assert_eq!(report.verifier_version, VERIFIER_VERSION);
}

// ---------------------------------------------------------------------------
// The worked example, and the size meter
// ---------------------------------------------------------------------------

#[test]
fn the_worked_example_qualifies_and_is_six_units() {
    let report = report_for("expand_east", Depth::Full);
    assert!(
        report.qualifies,
        "spec section 10's own worked example must verify clean; it found {:?}",
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.clone(), diagnostic.message.clone()))
            .collect::<Vec<_>>()
    );
    assert!(report.diagnostics.is_empty());
    // Item 94's own worked example: three route steps, one handler, two steps in
    // its body.
    assert_eq!(report.size_units, 6);
    assert_eq!(report.size_budget, 128);
}

#[test]
fn the_budget_fixture_sits_exactly_on_the_budget() {
    let report = report_for("budget_128", Depth::Full);
    assert!(
        report.qualifies,
        "the bench fixture must be a playbook that qualifies: {:?}",
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(report.size_units, report.size_budget);
    assert_eq!(report.size_units, 128);
}

#[test]
fn the_fingerprint_is_the_rule_crates_proto_writes_down() {
    let playbook: pharmakos_proto::gp::v1::Playbook =
        json::decode(&String::from_utf8(case_bytes("expand_east")).expect("UTF-8"))
            .expect("the worked example decodes");
    let digest = hash::plan_fingerprint(&playbook).expect("a fingerprint");
    let canonical = pharmakos_proto::fingerprint::canonical_input(&playbook).expect("canonical");
    let mut bytes = pharmakos_proto::fingerprint::PLAN_FINGERPRINT_DOMAIN.to_vec();
    bytes.extend_from_slice(&canonical);
    assert_eq!(digest, pharmakos_sim::encoding::digest(&bytes));
    assert_eq!(
        pharmakos_proto::fingerprint::from_field(&pharmakos_proto::fingerprint::to_field(digest)),
        Some(digest)
    );
}

// ---------------------------------------------------------------------------
// Load never strips
// ---------------------------------------------------------------------------

#[test]
fn an_out_of_vocabulary_construct_is_rejected_with_a_code_and_a_pointer() {
    let report = report_for("e0003_reserved_construct", Depth::Full);
    assert!(!report.qualifies);
    let first = report.diagnostics.first().expect("one diagnostic");
    assert_eq!(first.code, "E0003");
    assert_eq!(first.path, "/declarative/route/0/set_flag");
    assert_eq!(first.severity, i32::from(Severity::Error));
    assert!(
        first.message.contains("set_flag"),
        "the message names the word that was refused: {}",
        first.message
    );
    // Never stripped: the fix is offered to the author, not applied.
    let suggestion = first.suggestions.first().expect("a suggestion");
    assert!(suggestion.json_patch.contains("\"remove\""));
    assert!(
        suggestion
            .json_patch
            .contains("/declarative/route/0/set_flag")
    );
}

#[test]
fn every_reserved_name_the_schema_holds_is_refused_by_name() {
    // One document, every held-back word. The scan is by name over the whole
    // file, so each one is named where it stands rather than reported as an
    // anonymous unknown field.
    let mut text = String::from("{\"meta\":{");
    let held = [
        "team_id",
        "set_flag",
        "clear_flag",
        "branch",
        "repeat",
        "dispatch",
        "flag_set",
        "flag_clear",
        "link_up",
        "jammed",
        "satellite_pass",
        "grid_island",
        "reassign",
        "logistics",
        "capture",
        "reflex_hp_pct",
        "report_kinds",
        "accept_broadcasts",
        "repair_drones",
        "reclaim_drones",
        "probe_directions",
    ];
    for (index, name) in held.iter().enumerate() {
        if index > 0 {
            text.push(',');
        }
        // Writing into a `String` cannot fail; the result is dropped rather
        // than unwrapped.
        let _ = write!(text, "\"{name}\":1");
    }
    text.push_str("}}");

    let rules = rules();
    let scope = fixture_scope();
    let snapshot = fixture_snapshot();
    let input = Input::new(text.as_bytes(), &snapshot, &scope, &rules).expect("input");
    let report = verify(&input, Depth::Full);
    assert!(!report.qualifies);
    assert_eq!(report.diagnostics.len(), held.len());
    for (diagnostic, name) in report.diagnostics.iter().zip(held) {
        assert_eq!(diagnostic.code, "E0003");
        assert_eq!(diagnostic.path, format!("/meta/{name}"));
    }
}

#[test]
fn the_codec_still_says_what_this_crate_reads() {
    // `crates/verifier`'s decode stage tells `E0002` from `E0001` by the codec's
    // own wording. The coupling is deliberate and this is where it breaks
    // loudly: if `crates/proto` rewords the message, this test goes red in the
    // pull request that reworded it, rather than the code quietly changing.
    let error = json::decode::<pharmakos_proto::gp::v1::Playbook>("{\"nonsense\":1}")
        .expect_err("an unknown field is rejected");
    assert!(
        error.message.contains("is not a field"),
        "the marker `crates/verifier/src/decode.rs` reads is gone: {}",
        error.message
    );
    assert_eq!(error.pointer, "/nonsense");

    let report = report_for("e0002_unknown_field", Depth::Full);
    let first = report.diagnostics.first().expect("one diagnostic");
    assert_eq!(first.code, "E0002");
    assert_eq!(first.path, "/nonsense");
}

#[test]
fn author_text_cannot_talk_the_decoder_into_the_wrong_code() {
    // The codec quotes author text back: an enum value it does not know is
    // reported as "`{value}` is not a value of `{enum}`". A file whose value
    // *is* the unknown-field marker must still come back as `E0001` — anything
    // else names `author_kind`, a real and declared field, as unknown, and
    // offers a patch that deletes it.
    let text = "{\"schema_version\":{\"major\":1},\"kind\":\"PLAYBOOK\",                \"meta\":{\"title\":\"case\",\"author_kind\":\"is not a field\"}}";
    let rules = rules();
    let scope = fixture_scope();
    let snapshot = fixture_snapshot();
    let input = Input::new(text.as_bytes(), &snapshot, &scope, &rules).expect("input");
    let report = verify(&input, Depth::Full);
    let first = report.diagnostics.first().expect("one diagnostic");
    assert_eq!(
        first.code, "E0001",
        "a bad enum value is a value error, not an unknown field: {}",
        first.message
    );
    assert!(
        report.diagnostics.iter().all(|found| found.code != "E0002"),
        "nothing here is an unknown field"
    );
    assert!(
        report.diagnostics.iter().all(|found| found
            .suggestions
            .iter()
            .all(|fix| !fix.json_patch.contains("author_kind"))),
        "no suggestion offers to delete a field the schema declares"
    );
}

#[test]
fn the_worked_example_case_is_the_canonical_bytes_crates_proto_pins() {
    // `tests/golden/verifier/README.md` says this case is "spec section 10's
    // worked example, canonicalised", and every claim it makes about the case
    // rests on those being the same bytes `crates/proto`'s own golden holds. An
    // assertion is cheaper than a promise.
    let ours = fs::read(cases_dir().join("expand_east.json")).expect("the case");
    let theirs = fs::read(
        workspace_root()
            .join("tests")
            .join("golden")
            .join("proto")
            .join("expected.expand_east.json"),
    )
    .expect("crates/proto's canonical golden");
    assert!(
        ours == theirs,
        "crates/verifier/tests/cases/expand_east.json has drifted from          tests/golden/proto/expected.expand_east.json; the verifier's clean case must be exactly          the canonical form the proto lane pins"
    );
}

// ---------------------------------------------------------------------------
// The inputs themselves
// ---------------------------------------------------------------------------

#[test]
fn the_snapshot_fixture_is_a_snapshot() {
    let rules = rules();
    let scope = fixture_scope();
    let snapshot = fixture_snapshot();
    let playbook = case_bytes("expand_east");
    let input = Input::new(&playbook, &snapshot, &scope, &rules).expect("input");
    let decoded = input.decode_snapshot().expect("the fixture is a snapshot");
    assert_eq!(decoded.version, SNAPSHOT_VERSION);
    assert_eq!(decoded.tick, 3_600);
}

#[test]
fn a_missing_rules_block_is_refused_rather_than_defaulted() {
    let rules = rules();
    let mut message = rules.message().clone();
    message.verifier = None;
    let gapped = RulesTable::from_message(&message).expect("the sim's own view still builds");
    let scope = fixture_scope();
    let snapshot = fixture_snapshot();
    let playbook = case_bytes("expand_east");
    let error = Input::new(&playbook, &snapshot, &scope, &gapped)
        .expect_err("a verifier with no size budget rejects every playbook ever written");
    assert!(
        error.to_string().contains("verifier"),
        "the gap names the block: {error}"
    );
}

#[test]
fn the_limits_are_the_rules_table_and_not_constants() {
    let rules = rules();
    let scope = fixture_scope();
    let snapshot = fixture_snapshot();
    let playbook = case_bytes("expand_east");
    let input = Input::new(&playbook, &snapshot, &scope, &rules).expect("input");
    let limits = input.limits();
    let message = rules.message();
    let verifier = message.verifier.as_ref().expect("the verifier block");
    assert_eq!(limits.size_budget_units(), verifier.size_budget_units);
    assert_eq!(
        limits.handler_cooldown_min_ms(),
        verifier.handler_cooldown_min_ms
    );
    assert_eq!(limits.max_fires_max(), verifier.max_fires_max);
    assert_eq!(limits.notebook_max_chars(), verifier.notebook_max_chars);
    assert_eq!(limits.reach_memory_ms(), verifier.reach_memory_ms);
    let map = message.map.as_ref().expect("the map block");
    assert_eq!(
        limits.map_size(),
        [
            i32::try_from(map.size_x).expect("in range"),
            i32::try_from(map.size_y).expect("in range"),
            i32::try_from(map.size_z).expect("in range"),
        ]
    );
    let beacon_block = message.beacon.as_ref().expect("the beacon block");
    assert_eq!(
        limits.beacon_sphere_radius_voxels(),
        i32::try_from(beacon_block.sphere_radius_voxels).expect("in range")
    );
    let matched = message.r#match.as_ref().expect("the match block");
    assert_eq!(limits.decision_tick_ms(), matched.decision_tick_ms);
}
