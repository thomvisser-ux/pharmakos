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

// ---------------------------------------------------------------------------
// The size cap (S3's `vfy6`, follow-up D)
// ---------------------------------------------------------------------------

/// A file under this test's scratch folder, written fresh.
fn scratch_file(name: &str, bytes: &[u8]) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&path, bytes).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    path
}

/// `gamectl verify` over a file outside the workspace, by its absolute path.
fn verify_file(path: &Path) -> pharmakos_gamectl::Outcome {
    let shown = path.to_str().expect("a UTF-8 scratch path");
    gamectl(&["verify", shown])
}

#[test]
fn a_huge_file_is_e0029_and_is_never_read_whole() {
    // Not UTF-8, and longer than the cap could be in any bytes: had the whole
    // file been read as text it would be an unreadable file (`Exit::Input`),
    // so `E0029` says the read stopped at the bound.
    let length = pharmakos_verifier::MAX_PLAYBOOK_READ
        .checked_mul(3)
        .expect("fits");
    let path = scratch_file("vfy6_huge.json", &vec![0xff_u8; length]);
    let outcome = verify_file(&path);
    assert_eq!(outcome.code, Exit::Failed, "{}{}", outcome.out, outcome.err);
    assert!(outcome.err.contains("E0029"), "{}", outcome.err);
    assert!(outcome.err.contains("report_hash"), "{}", outcome.err);
}

#[test]
fn a_text_past_the_cap_in_characters_is_e0029() {
    let title = "t".repeat(pharmakos_verifier::MAX_PLAYBOOK_CHARS);
    let text = format!(r#"{{"kind":"PLAYBOOK","meta":{{"title":"{title}"}}}}"#);
    assert!(!pharmakos_verifier::is_too_long_in_bytes(text.len()));
    let path = scratch_file("vfy6_long.json", text.as_bytes());
    let outcome = verify_file(&path);
    assert_eq!(outcome.code, Exit::Failed, "{}{}", outcome.out, outcome.err);
    assert!(outcome.err.contains("E0029"), "{}", outcome.err);
    // Nothing past the cap is decoded, so nothing else is said about it.
    assert!(!outcome.err.contains("E0001"), "{}", outcome.err);
}

/// The cap is on the text the player wrote, never on the canonical text the
/// verifier is handed, which is indented a level for every message it nests
/// (bench3's finding in PR #178: a 151 KB file is 855 KB of canonical text).
#[test]
fn a_file_under_the_cap_whose_canonical_text_is_over_it_still_verifies() {
    let leaf =
        |value: usize| format!(r#"{{"segment_elapsed":{{"ms":{{"op":"EQ","value":{value}}}}}}}"#);
    let mut next = 0_usize;
    let steps: Vec<String> = (0..127_usize)
        .map(|id| {
            let leaves: Vec<String> = (0..21)
                .map(|_| {
                    next = next.saturating_add(1);
                    leaf(next)
                })
                .collect();
            let group = if id.is_multiple_of(2) { "any" } else { "all" };
            let wrap = |inner: String| format!(r#"{{"{group}":{{"items":[{inner}]}}}}"#);
            let condition = wrap(wrap(wrap(leaves.join(","))));
            format!(r#"{{"label":"r{id:03}","skip_if":{condition},"hold":{{"ms":1000}}}}"#)
        })
        .collect();
    let text = format!(
        r#"{{"schema_version":{{"major":1}},"meta":{{"title":"cap","author_kind":"HUMAN"}},"kind":"PLAYBOOK","declarative":{{"route":[{}]}},"on_death":{{"on_respawn":"CONTINUE","max_deaths_before_fallback":1}},"fallback":{{"hold":{{"at":{{"beacon_anchor":{{"safest":{{}}}}}}}}}}}}"#,
        steps.join(",")
    );
    let canonical = pharmakos_plan_core::canonicalise_text(&text)
        .expect("the shape has a canonical form")
        .json;
    assert!(
        !pharmakos_verifier::is_too_long(&text),
        "{} chars",
        text.len()
    );
    assert!(
        pharmakos_verifier::is_too_long(&canonical),
        "the canonical text is {} chars, within the cap, so this test proves nothing",
        canonical.chars().count()
    );
    let path = scratch_file("vfy6_under_the_cap.json", text.as_bytes());
    let outcome = verify_file(&path);
    let said = format!("{}{}", outcome.out, outcome.err);
    assert!(!said.contains("E0029"), "{said}");
    assert!(said.contains("report_hash"), "{said}");
}

/// A diagnostic's pointer is printed whole (S3's `vfyq2`, fix pass 2b): the
/// headless watch check, and anyone who reads the line, takes the pointer from
/// it, so a pointer the verifier builds from the schema's own names is printed
/// exactly, however long. Fix pass 2 printed it through the 64-character
/// quote, and this pointer, 70 characters, lost its tail to `…`.
#[test]
fn a_long_pointer_is_printed_whole() {
    let text = r#"{
  "schema_version": {"major": 1},
  "meta": {"title": "t", "author_kind": "HUMAN"},
  "kind": "PLAYBOOK",
  "declarative": {"route": [
    {"label": "guard", "place_beacon": {"at": {"covering": {"feature_id": "seam_1_1"}},
      "initial": {"priority": "NORMAL", "mandate": {"roe": "FREE_FIRE", "defend": {
        "protect": [{"beacon": {"beacon_id": "b_77"}}]}}}}}
  ]},
  "on_death": {"on_respawn": "CONTINUE"},
  "fallback": {"hold": {"at": {"beacon_anchor": {"safest": {}}}}}
}"#;
    let path = scratch_file("a_long_pointer.jsonc", text.as_bytes());
    let outcome = verify_file(&path);
    let pointer =
        "/declarative/route/0/place_beacon/initial/mandate/defend/protect/0/beacon/beacon_id";
    assert!(
        pointer.chars().count() > 64,
        "the case needs a pointer past the quote's cap"
    );
    let said = format!("{}{}", outcome.out, outcome.err);
    let printed: Vec<&str> = said
        .lines()
        .filter_map(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            (words.first() == Some(&"error"))
                .then(|| words.get(2).copied())
                .flatten()
        })
        .collect();
    assert!(
        printed.contains(&pointer),
        "the pointer is printed whole: {printed:?}\n{}{}",
        outcome.out,
        outcome.err
    );
}

// ---------------------------------------------------------------------------
// The view a report names (S3's plan, E19) and the lobby's (`--preset`)
// ---------------------------------------------------------------------------

/// The reference view's heading line, word for word.
const REFERENCE_VIEW: &str =
    "  view         the reference view (not a match's snapshot; `gamectl seat doctor` prints it)";

/// The Probation preset's heading line, word for word.
const PROBATION_VIEW: &str = "  view         Probation, seed 20, seat 0's opening Lull snapshot";

/// The sentence a reference-view report with `E0403` carries after its
/// diagnostics.
const E0403_NOTE: &str = "  E0403 here is against the reference view. In a match the gateway \
                          checks against the seat's own frozen snapshot, where this beacon may \
                          be legal.";

/// A case the reference view refuses with `E0403`.
const E0403_CASE: &str = "crates/verifier/tests/cases/e0403_site_outside_every_sphere.json";

/// A playbook written for a live seat, which names its core `b_00`: the
/// reference view's core is `b_01`, so it is `E0401` there.
const NAMES_B_00: &str = "library/defend_the_core.jsonc";

fn said(outcome: &pharmakos_gamectl::Outcome) -> String {
    format!("{}{}", outcome.out, outcome.err)
}

/// The report's lines, wherever the exit code sent them: standard output when
/// it qualifies, standard error when it does not.
fn report_lines(outcome: &pharmakos_gamectl::Outcome) -> Vec<String> {
    let text = if outcome.code == Exit::Ok {
        &outcome.out
    } else {
        &outcome.err
    };
    text.lines().map(str::to_owned).collect()
}

/// E19: the report says at its head which view it was verified against, at
/// either depth.
#[test]
fn the_heading_names_the_reference_view_at_either_depth() {
    for (depth, word) in [("full", "FULL"), ("quick", "QUICK")] {
        let outcome = gamectl(&[
            "verify",
            "--depth",
            depth,
            "examples/playbooks/expand_east.jsonc",
        ]);
        assert_eq!(outcome.code, Exit::Ok, "{}", said(&outcome));
        let head: Vec<String> = report_lines(&outcome).into_iter().take(3).collect();
        assert_eq!(
            head,
            vec![
                String::from("examples/playbooks/expand_east.jsonc"),
                format!("  depth        {word}"),
                String::from(REFERENCE_VIEW),
            ],
            "{}",
            outcome.out
        );
    }
}

/// E19: an `E0403` against the reference view is followed, directly after
/// the diagnostics and before the footer, by the sentence that says a match
/// may accept the beacon; a report without `E0403` has no such sentence.
#[test]
fn an_e0403_against_the_reference_view_says_a_match_may_accept_it() {
    let refused = gamectl(&["verify", E0403_CASE]);
    assert_eq!(refused.code, Exit::Failed, "{}", said(&refused));
    let lines = report_lines(&refused);
    let note = lines
        .iter()
        .position(|line| line == E0403_NOTE)
        .unwrap_or_else(|| panic!("no E0403 note:\n{}", refused.err));
    assert!(
        lines
            .get(note.saturating_sub(2))
            .is_some_and(|line| line.trim_start().starts_with("error   E0403")),
        "the note follows the last diagnostic, E0403 and its message:\n{}",
        refused.err
    );
    assert!(
        lines
            .get(note.saturating_add(1))
            .is_some_and(|line| line.starts_with("  Verified against the reference seat view")),
        "the footer follows the note:\n{}",
        refused.err
    );
    for args in [
        &["verify", "examples/playbooks/expand_east.jsonc"][..],
        &[
            "verify",
            "--depth",
            "quick",
            "examples/playbooks/expand_east.jsonc",
        ],
        &[
            "verify",
            "crates/verifier/tests/cases/e0102_empty_route.json",
        ],
    ] {
        let outcome = gamectl(args);
        assert!(
            !said(&outcome).contains("E0403 here"),
            "{args:?}: a report without E0403 carries no note:\n{}",
            said(&outcome)
        );
    }
}

/// `--preset probation`: the file is verified by the gateway against seat 0's
/// frozen snapshot in the opening Lull of the lobby's Probation match. A
/// playbook naming the live core `b_00`, `E0401` against the reference view,
/// qualifies there, and the heading names that view.
#[test]
fn the_probation_preset_verifies_against_the_lobby_s_match() {
    let reference = gamectl(&["verify", NAMES_B_00]);
    assert_eq!(reference.code, Exit::Failed, "{}", said(&reference));
    assert!(reference.err.contains("E0401"), "{}", reference.err);

    for (depth, word) in [("full", "FULL"), ("quick", "QUICK")] {
        let preset = gamectl(&[
            "verify",
            "--preset",
            "probation",
            "--depth",
            depth,
            NAMES_B_00,
        ]);
        assert_eq!(preset.code, Exit::Ok, "{}", said(&preset));
        let lines = report_lines(&preset);
        let head: Vec<String> = lines.iter().take(3).cloned().collect();
        assert_eq!(
            head,
            vec![
                String::from(NAMES_B_00),
                format!("  depth        {word}"),
                String::from(PROBATION_VIEW),
            ],
            "{}",
            preset.out
        );
        assert!(
            lines.last().is_some_and(|line| line.starts_with(
                "  Verified by the gateway against seat 0's frozen snapshot in the opening Lull \
                 of the Probation match the lobby opens (seed 20, 2 seats)"
            )),
            "{}",
            preset.out
        );
    }

    // The match's own report carries no reference-view sentence, even where
    // it says E0403 itself.
    let e0403 = gamectl(&["verify", "--preset", "probation", E0403_CASE]);
    assert_eq!(e0403.code, Exit::Failed, "{}", said(&e0403));
    assert!(e0403.err.contains("E0403"), "{}", e0403.err);
    assert!(!e0403.err.contains("E0403 here"), "{}", e0403.err);
}

/// The preset's report is the gateway's: the same file verified twice
/// answers the same `report_hash`, and it is not the reference view's.
#[test]
fn the_preset_s_report_is_reproducible_and_its_own() {
    let hash_of = |outcome: &pharmakos_gamectl::Outcome| -> String {
        report_lines(outcome)
            .iter()
            .find_map(|line| {
                line.trim_start()
                    .strip_prefix("report_hash  ")
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| panic!("a report_hash:\n{}", said(outcome)))
    };
    let example = "examples/playbooks/expand_east.jsonc";
    let first = gamectl(&["verify", "--preset", "probation", example]);
    let second = gamectl(&["verify", "--preset", "probation", example]);
    let reference = gamectl(&["verify", example]);
    assert_eq!(hash_of(&first), hash_of(&second));
    assert_ne!(hash_of(&first), hash_of(&reference));
}

/// A file past the size cap in bytes, under the preset, is refused by this
/// command itself, read no further than the bound: the answer is no (exit 3),
/// with the file's real size and the cap, and no made-up figure (fix pass 1
/// of #219, item 5; the gateway was once handed a text one character past
/// the cap and its refusal quoted that length).
#[test]
fn a_huge_file_is_refused_by_gamectl_under_the_preset() {
    let length = pharmakos_verifier::MAX_PLAYBOOK_READ
        .checked_mul(2)
        .expect("fits");
    let path = scratch_file("nite3_huge_preset.json", &vec![0xff_u8; length]);
    let shown = path.to_str().expect("a UTF-8 scratch path");
    let outcome = gamectl(&["verify", "--preset", "probation", shown]);
    assert_eq!(outcome.code, Exit::Failed, "{}", said(&outcome));
    for wanted in [
        format!("is {length} bytes"),
        format!("past {} bytes", pharmakos_verifier::MAX_PLAYBOOK_READ),
        format!(
            "the {}-character cap",
            pharmakos_verifier::MAX_PLAYBOOK_CHARS
        ),
        String::from("the gateway was not asked"),
    ] {
        assert!(outcome.err.contains(&wanted), "{wanted}: {}", outcome.err);
    }
    let one_past = pharmakos_verifier::MAX_PLAYBOOK_CHARS.saturating_add(1);
    assert!(
        !outcome.err.contains(&one_past.to_string()),
        "no figure one past the cap: {}",
        outcome.err
    );
}

/// The lobby's line, which `gamectl` carries to open the preset's match, is
/// the lobby's own: each field is held to `godot/scripts/host_link.gd`'s
/// constant, and the seed to the sim's pin of the Probation seed
/// (`crates/sim/tests/probation_seed.rs`).
#[test]
fn the_preset_s_match_is_the_one_the_lobby_opens() {
    let read = |relative: &str| {
        std::fs::read_to_string(root().join(relative))
            .unwrap_or_else(|error| panic!("{relative}: {error}"))
    };
    let constant = |text: &str, prefix: &str| -> String {
        text.lines()
            .find_map(|line| line.strip_prefix(prefix))
            .map_or_else(
                || panic!("no line starts `{prefix}`"),
                |value| value.trim().trim_matches('"').to_owned(),
            )
    };
    let lobby = read("godot/scripts/host_link.gd");
    let fields: Vec<&str> = pharmakos_gamectl::lobby::PROBATION_LINE
        .split('\t')
        .collect();
    assert_eq!(fields.len(), 7, "the seven fields of a lobby's line");
    for (index, prefix) in [
        (1, "const MATCH_SEED := "),
        (2, "const MATCH_SEATS := "),
        (3, "const HUMAN_SEAT := "),
        (4, "const LADDER := "),
        (5, "const ROUND_LIMIT := "),
        (6, "const PRESET := "),
    ] {
        assert_eq!(
            fields.get(index).copied(),
            Some(constant(&lobby, prefix).as_str()),
            "field {index} of the line gamectl carries is not host_link.gd's `{prefix}`"
        );
    }
    let pin = constant(
        &read("crates/sim/tests/probation_seed.rs"),
        "const PROBATION_SEED: u64 = ",
    );
    let config = pharmakos_gamectl::lobby::probation().expect("the line reads");
    assert_eq!(
        Some(config.seed.to_string().as_str()),
        pin.strip_suffix(';'),
        "the Probation seed the sim pins"
    );
}

// ---------------------------------------------------------------------------
// `--format json`: the report as the verifier's own JSON
// ---------------------------------------------------------------------------

/// A committed verifier golden's document: the encoder's text, which ends in
/// one line feed. The verifier's producer writes a second one after it
/// (`report_text` in `crates/verifier/tests/verifier.rs`); a command's output
/// ends in exactly one, as every message of this binary does.
fn verifier_golden(case: &str) -> String {
    let path = root()
        .join("tests/golden/verifier")
        .join(case)
        .join("expected.report.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
        .replace("\r\n", "\n");
    text.strip_suffix('\n')
        .unwrap_or_else(|| panic!("{} ends in its producer's line feed", path.display()))
        .to_owned()
}

/// The JSON report is the verifier's own `gp.api.v1.VerifyReport`: for the
/// spec's worked example, whose `.jsonc` canonicalises to the verifier's own
/// case bytes (the test above), it is byte for byte the golden the verifier
/// commits, one object on standard output and nothing on standard error.
#[test]
fn the_json_report_is_the_verifier_s_own_golden_byte_for_byte() {
    let outcome = gamectl(&[
        "verify",
        "--format",
        "json",
        "examples/playbooks/expand_east.jsonc",
    ]);
    assert_eq!(outcome.code, Exit::Ok, "{}", said(&outcome));
    assert!(outcome.err.is_empty(), "{}", outcome.err);
    assert_eq!(outcome.out, verifier_golden("expand_east"));
}

/// A playbook that does not qualify is still the answer: exit 3, the report
/// as one JSON object on standard output (proto3 JSON leaves `qualifies:
/// false` out), and nothing on standard error.
#[test]
fn a_refused_playbook_s_json_report_is_on_standard_output() {
    let outcome = gamectl(&[
        "verify",
        "--format",
        "json",
        "crates/verifier/tests/cases/e0102_empty_route.json",
    ]);
    assert_eq!(outcome.code, Exit::Failed, "{}", said(&outcome));
    assert!(outcome.err.is_empty(), "{}", outcome.err);
    let parsed = json::read(&outcome.out).expect("one JSON document");
    assert!(matches!(parsed, Json::Object(_)), "one object");
    assert_eq!(parsed.get("qualifies"), None, "{}", outcome.out);
    assert!(
        outcome.out.contains("\"code\": \"E0102\""),
        "{}",
        outcome.out
    );
    assert!(outcome.out.ends_with("}\n"), "{:?}", outcome.out);
}

/// The JSON names what the human report names: the same `report_hash`,
/// under the reference view and under the preset.
#[test]
fn the_json_report_carries_the_text_report_s_hash() {
    for view in [&[][..], &["--preset", "probation"]] {
        let mut text_args = vec!["verify"];
        text_args.extend_from_slice(view);
        text_args.push(NAMES_B_00);
        let mut json_args = text_args.clone();
        json_args.splice(1..1, ["--format", "json"]);
        let text = gamectl(&text_args);
        let json_outcome = gamectl(&json_args);
        assert_eq!(text.code, json_outcome.code, "{view:?}");
        let shown = report_lines(&text)
            .iter()
            .find_map(|line| {
                line.trim_start()
                    .strip_prefix("report_hash  ")
                    .map(str::to_owned)
            })
            .expect("a report_hash line");
        let parsed = json::read(&json_outcome.out).expect("one JSON document");
        let Some(Json::String(encoded)) = parsed.get("report_hash") else {
            panic!("{view:?}: a report_hash: {}", json_outcome.out);
        };
        let bytes = json::base64::decode(encoded).expect("base64");
        let mut hex = String::new();
        for byte in &bytes {
            let _ = write!(hex, "{byte:02x}");
        }
        assert_eq!(hex, shown, "{view:?}");
    }
}

/// Every string in the JSON goes through the human report's escaping: a key
/// the player wrote with U+202E and U+200B in it, which the verifier's
/// pointer carries as it is, is written `\u202e` and `\u200b`, never raw.
#[test]
fn the_json_report_escapes_what_the_text_report_escapes() {
    let text = "{\"kind\":\"PLAYBOOK\",\"a\u{202E}b\u{200B}c\":1}";
    let path = scratch_file("nite3_hostile_key.json", text.as_bytes());
    let shown = path.to_str().expect("a UTF-8 scratch path");
    let outcome = gamectl(&["verify", "--format", "json", shown]);
    assert_eq!(outcome.code, Exit::Failed, "{}", said(&outcome));
    assert!(
        !outcome.out.contains('\u{202E}') && !outcome.out.contains('\u{200B}'),
        "printed raw: {:?}",
        outcome.out
    );
    assert!(
        outcome.out.contains("\\u202e") && outcome.out.contains("\\u200b"),
        "{}",
        outcome.out
    );
    let parsed = json::read(&outcome.out).expect("still one JSON document");
    let Some(Json::Array(diagnostics)) = parsed.get("diagnostics") else {
        panic!("diagnostics: {}", outcome.out);
    };
    assert!(
        diagnostics.iter().any(|diagnostic| matches!(
            diagnostic.get("path"),
            Some(Json::String(path)) if path.contains('\u{202E}')
        )),
        "the escape reads back as the player's own character: {}",
        outcome.out
    );
}

#[test]
fn the_hidden_escape_writes_utf16_units() {
    assert_eq!(
        pharmakos_gamectl::verify::escape_hidden("\"a\u{202E}b\u{E0001}c\""),
        "\"a\\u202eb\\udb40\\udc01c\""
    );
    assert_eq!(
        pharmakos_gamectl::verify::escape_hidden("plain \u{e9}"),
        "plain \u{e9}"
    );
}
