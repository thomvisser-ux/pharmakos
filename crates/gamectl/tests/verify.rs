// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `gamectl verify`, against the verifier's own committed goldens.
//!
//! The skeleton plan's T15 acceptance line: "`gamectl verify
//! examples/playbooks/expand_east.jsonc` reproduces the verifier golden's
//! `report_hash`". The hash is **read out of the golden**, never written here.
//! That is the whole design of this file: `report_hash` is a pure function of
//! five inputs, two of which — the snapshot and the seat's view of it — this
//! binary has to supply from somewhere because a shell command has no match
//! (`crates/gamectl/src/seat.rs`). A literal here would pass while the two
//! fixtures drifted; reading the golden means the day T6's fixture moves, or
//! `SNAPSHOT_VERSION` moves, or `VERIFIER_VERSION` moves, this test names
//! `seat.rs` and asks for it to follow.
//!
//! It also means this file needs no re-blessing of its own: whoever moves the
//! verifier goldens has already moved what this compares against.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use pharmakos_gamectl::exit::Exit;
use pharmakos_gamectl::run;
use pharmakos_proto::json::{self, Json};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| panic!("crates/gamectl sits two levels below the workspace root"))
        .to_path_buf()
}

/// The `report_hash` a committed verifier golden carries, as hex.
///
/// The golden is canonical `gp.api.v1.VerifyReport` JSON, so `report_hash` is
/// base64 of eight big-endian bytes; this renders it the way `gamectl verify`
/// prints it, which is the comparison the acceptance line asks for.
fn golden_report_hash(case: &str) -> String {
    let path = root()
        .join("tests/golden/verifier")
        .join(case)
        .join("expected.report.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
    let report: Json = json::read(&text.replace("\r\n", "\n"))
        .unwrap_or_else(|error| panic!("{}: {error:?}", path.display()));
    let Some(Json::String(encoded)) = report.get("report_hash") else {
        panic!("{}: a report carries a report_hash", path.display());
    };
    let bytes = json::base64::decode(encoded)
        .unwrap_or_else(|| panic!("{}: report_hash is base64", path.display()));
    let mut hex = String::new();
    for byte in &bytes {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

fn gamectl(args: &[&str]) -> pharmakos_gamectl::Outcome {
    run(args.iter().map(|arg| (*arg).to_owned()), &root())
}

#[test]
fn verify_reproduces_the_verifier_goldens_report_hash_for_the_worked_example() {
    let outcome = gamectl(&["verify", "examples/playbooks/expand_east.jsonc"]);
    assert_eq!(
        outcome.code,
        Exit::Ok,
        "the spec's own worked example must qualify:\n{}{}",
        outcome.out,
        outcome.err
    );
    let wanted = golden_report_hash("expand_east");
    assert!(
        outcome.out.contains(&wanted),
        "`gamectl verify` printed a different report_hash from the one committed at \
         tests/golden/verifier/expand_east/expected.report.json ({wanted}).\n\nThe reference \
         seat view in crates/gamectl/src/seat.rs and the fixture in \
         crates/verifier/tests/verifier.rs are one fixture written in two places, and they have \
         drifted. Make seat.rs follow the verifier's, and say in the pull request which of the \
         five hashed inputs moved.\n\n{}",
        outcome.out
    );
}

/// The example on disk is JSONC and the verifier's case is canonical JSON. The
/// acceptance line is only true if those are the same bytes once `plan-core`
/// has had them, so that is asserted rather than assumed.
#[test]
fn the_jsonc_example_canonicalises_to_the_verifiers_own_case_bytes() {
    let jsonc = std::fs::read_to_string(root().join("examples/playbooks/expand_east.jsonc"))
        .expect("the worked example is committed");
    let canonical = pharmakos_plan_core::canonicalise_text(&jsonc)
        .expect("the worked example has a canonical form");
    let case = std::fs::read_to_string(root().join("crates/verifier/tests/cases/expand_east.json"))
        .expect("T6 committed the case");
    assert_eq!(
        canonical.json,
        case.replace("\r\n", "\n"),
        "the verifier hashes what plan-core canonicalises, so a report about the .jsonc and a \
         report about the .json are the same report only while these agree"
    );
}

#[test]
fn a_playbook_that_does_not_qualify_is_the_answer_and_not_an_error() {
    // Exit 3, and the diagnostics on the page. A verifier that exited 1 for a
    // bad playbook would be telling a script the command line was wrong.
    let outcome = gamectl(&[
        "verify",
        "crates/verifier/tests/cases/e0102_empty_route.json",
    ]);
    assert_eq!(outcome.code, Exit::Failed, "{}", outcome.err);
    assert!(outcome.err.contains("E0102"), "{}", outcome.err);
    assert!(outcome.err.contains("qualifies    no"), "{}", outcome.err);
    assert!(
        outcome.err.contains("report_hash"),
        "a report that does not qualify still carries its hash: {}",
        outcome.err
    );
}

#[test]
fn a_file_that_is_not_json_is_a_diagnostic_and_not_a_crash() {
    // The difference between a verifier and a parser, from the command line.
    let outcome = gamectl(&["verify", "crates/verifier/tests/cases/e0001_not_json.json"]);
    assert_eq!(outcome.code, Exit::Failed);
    assert!(outcome.err.contains("E0001"), "{}", outcome.err);
    assert!(
        outcome.err.contains("/byte/"),
        "E0001's lexical form carries a byte offset, not a node pointer: {}",
        outcome.err
    );
}

#[test]
fn quick_and_full_are_different_reports_and_say_which_they_are() {
    let quick = gamectl(&[
        "verify",
        "--depth",
        "quick",
        "examples/playbooks/expand_east.jsonc",
    ]);
    let full = gamectl(&["verify", "examples/playbooks/expand_east.jsonc"]);
    assert_eq!(quick.code, Exit::Ok);
    assert_eq!(full.code, Exit::Ok);
    assert!(quick.out.contains("QUICK"), "{}", quick.out);
    assert!(full.out.contains("FULL"), "{}", full.out);
    assert_ne!(
        quick.out, full.out,
        "depth is one of the five hashed inputs, so two depths are two reports"
    );
}

#[test]
fn a_missing_file_is_an_input_error_and_names_the_path() {
    let outcome = gamectl(&["verify", "examples/playbooks/not_here.jsonc"]);
    assert_eq!(outcome.code, Exit::Input);
    assert!(outcome.err.contains("not_here.jsonc"), "{}", outcome.err);
}
