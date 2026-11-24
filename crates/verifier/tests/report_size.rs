// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A report's size, against what the client can take (S3's `vfy6`, the
//! review of PR #179, item 3; S3's `vcap`, decisions-log item 183 (4)).
//!
//! The editor reads a report in one WebSocket message through a peer whose
//! inbound buffer is `INBOUND_BUFFER_BYTES` (`crates/client-gdext/src/transport.rs`,
//! which `godot/scripts/host_link.gd` sets on every peer since S3's `fixq-c`),
//! and a message past it is never handed over. A report lists at most
//! `MAX_PER_CODE` diagnostics of one code and closes with one `I0007` per code
//! it left out of.
//!
//! Three holds:
//!
//! * **the derivation**: the worst report the model in `MAX_PER_CODE`'s doc
//!   allows, every code a stage raises at the cap, fits three quarters of the
//!   buffer, and at one more per code it would not: the cap is the top of the
//!   range;
//! * **the model holds bytes**: a report built to the model's every bound,
//!   encoded, is no longer than the model says;
//! * **real files**: the reports for the longest files the text cap admits
//!   fit the 128 KiB allowance, an eighth of the buffer or less.

#![allow(
    clippy::expect_used,
    reason = "a panic is this file's failure report, and the helpers are not #[test] functions"
)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use pharmakos_proto::gp::api::v1::diagnostic::Severity;
use pharmakos_proto::gp::api::v1::patch_suggestion::Applicability;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::gp::api::v1::{Diagnostic, PatchSuggestion, VerifyReport};
use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::json;
use pharmakos_proto::json::grammar::MAX_ENGINE_CHARS;
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::math::quantity::{Kw, Money};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::tables::SeatId;
use pharmakos_verifier::catalogue::{CATALOGUE, Emitter};
use pharmakos_verifier::strings::VERIFIER_STRINGS;
use pharmakos_verifier::{
    ARGUMENT_BYTES_MAX, DIAGNOSTIC_ROOM_BYTES, Input, MAX_PER_CODE, MAX_PLAYBOOK_CHARS,
    MAX_POINTER_BYTES, ONCE_PER_REPORT, Scope, is_too_long, verify, verify_counted,
};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The most a report for the longest real file may be: an eighth of the 1 MiB inbound buffer
/// the client had when this test was written, and an eighth or less of any buffer since
/// (`the_allowance_is_within_the_clients_buffer`).
const REPORT_ALLOWANCE_BYTES: usize = 131_072;
// The ruled 128 KiB, held at compile time: raising it is a ruling, not an edit (fix pass 3).
const _: () = assert!(REPORT_ALLOWANCE_BYTES == 128 * 1024);

/// What the report around its diagnostics takes at most as JSON: its eight
/// other fields (three eight-byte hashes as base64, the version, the depth,
/// two numbers) and the array's framing. The committed reports with no
/// diagnostic take about 230 bytes.
const REPORT_FRAMING_BYTES: usize = 512;

/// What one closing `I0007` takes at most beyond its two strings: its framing,
/// the root pointer, and its three arguments, a code and two numbers of at
/// most twenty digits each (no pointer, no related path, no Fix, no map).
const MARKER_ROOM_BYTES: usize = 256 + 5 + 20 + 20;

/// The bytes one argument of `E0001` adds at most: the codec's engine words,
/// at most `MAX_ENGINE_CHARS` characters, already quoted, so each is at most
/// four bytes of UTF-8 or a backslash escape the report doubles.
const ENGINE_ARGUMENT_BYTES_MAX: usize = MAX_ENGINE_CHARS * 4;

/// The client's inbound buffer, read from the client's own source rather
/// than written here a second time (the verifier may not depend on the
/// walled client crate, so the line is read as text).
fn inbound_bytes() -> usize {
    const DECLARED: &str = "pub const INBOUND_BUFFER_BYTES: usize = ";
    let source = std::fs::read_to_string(root().join("crates/client-gdext/src/transport.rs"))
        .expect("the client's transport sizes");
    let line = source
        .lines()
        .find(|line| line.starts_with(DECLARED))
        .expect("transport.rs declares INBOUND_BUFFER_BYTES");
    line.trim_start_matches(DECLARED)
        .trim_end_matches(';')
        .replace('_', "")
        .parse()
        .expect("INBOUND_BUFFER_BYTES is a whole number")
}

/// Three quarters of the client's buffer: what the worst report may take.
fn room() -> usize {
    inbound_bytes()
        .checked_div(4)
        .and_then(|quarter| quarter.checked_mul(3))
        .expect("a buffer of some size")
}

fn cap() -> usize {
    usize::try_from(MAX_PER_CODE).expect("fits")
}

/// How many `{name}` arguments a template names.
fn arguments_in(template: &str) -> usize {
    template.matches('{').count()
}

/// A code's two strings, from the table.
fn strings_of(code: &str) -> (&'static str, &'static str) {
    let text = VERIFIER_STRINGS
        .iter()
        .find(|row| row.code == code)
        .expect("every catalogue code has its strings");
    (text.message, text.beginner)
}

/// The most bytes one diagnostic of `code` takes, by the model.
fn diagnostic_bound(code: &str) -> usize {
    let (message, beginner) = strings_of(code);
    let per_argument = if code == "E0001" {
        ENGINE_ARGUMENT_BYTES_MAX
    } else {
        ARGUMENT_BYTES_MAX
    };
    message
        .len()
        .saturating_add(beginner.len())
        .saturating_add(DIAGNOSTIC_ROOM_BYTES)
        .saturating_add(
            arguments_in(message)
                .saturating_add(arguments_in(beginner))
                .saturating_mul(per_argument),
        )
}

/// The most bytes one closing `I0007` takes, by the model.
fn marker_bound() -> usize {
    let (message, beginner) = strings_of("I0007");
    message
        .len()
        .saturating_add(beginner.len())
        .saturating_add(MARKER_ROOM_BYTES)
}

/// The worst report at a cap of `per_code`, by the model: every code a stage
/// raises, at the cap unless it is raised once per report, and one `I0007`
/// for each code that can reach the cap.
fn worst_report(per_code: usize) -> usize {
    let mut total = REPORT_FRAMING_BYTES;
    for row in CATALOGUE {
        if !matches!(row.emitter, Emitter::Stage(_)) || row.code == "I0007" {
            continue;
        }
        let once = ONCE_PER_REPORT.contains(&row.code);
        let count = if once { 1 } else { per_code };
        total = total.saturating_add(count.saturating_mul(diagnostic_bound(row.code)));
        if !once {
            total = total.saturating_add(marker_bound());
        }
    }
    total
}

/// `MAX_PER_CODE` is derived: the worst report the model allows fits three
/// quarters of the client's inbound buffer at the cap, and would not at one
/// more, so the cap is the top of the range (decisions-log item 183 (4); the
/// rulings' item 2). Below two, Easy's probe could not ask in parts (its call
/// budget is the owner's, `IN_PROCESS_LIMITS`): the compile-time check below.
const _: () = assert!(
    MAX_PER_CODE >= 2,
    "a cap under two leaves Easy's probe no part"
);

#[test]
fn the_cap_is_the_largest_whose_worst_report_fits_the_clients_buffer() {
    let room = room();
    let at_cap = worst_report(cap());
    let past = worst_report(cap().checked_add(1).expect("fits"));
    let codes = CATALOGUE
        .iter()
        .filter(|row| matches!(row.emitter, Emitter::Stage(_)) && row.code != "I0007")
        .count();
    println!(
        "{codes} codes a stage raises ({} once a report); at {MAX_PER_CODE} a code the worst \
         report is {at_cap} bytes, at one more {past}; room {room} of a {}-byte buffer",
        ONCE_PER_REPORT.len(),
        inbound_bytes(),
    );
    assert!(
        at_cap <= room,
        "the worst report at {MAX_PER_CODE} a code is {at_cap} bytes, past {room}"
    );
    assert!(
        past > room,
        "the worst report at {} a code is {past} bytes, within {room}: the cap is not the top \
         of its range",
        cap().saturating_add(1)
    );
}

/// The room the model gives a diagnostic covers its parts: a pointer and a
/// related pointer at the pointer bound, a Fix whose title is one argument
/// and whose patch repeats a pointer (escaped once more inside the patch's
/// string) with a value, two map references, and the framing.
#[test]
fn the_room_a_diagnostic_is_given_covers_its_parts() {
    let parts = 256 // framing, code, severity, applicability
        + MAX_POINTER_BYTES // path
        + MAX_POINTER_BYTES // one related path
        + ARGUMENT_BYTES_MAX // a Fix's title
        + 2 * MAX_POINTER_BYTES + 256 + 64 // the patch: its pointer twice-escaped, a value, the op
        + 2 * 64; // two map references
    assert!(
        parts <= DIAGNOSTIC_ROOM_BYTES,
        "a diagnostic's parts take {parts} bytes, past DIAGNOSTIC_ROOM_BYTES"
    );
}

/// A diagnostic built to the model's every bound for `code`: its strings at
/// their templates' length with every argument at its bound, a pointer and a
/// related pointer at the pointer bound, a Fix, two map references.
fn diagnostic_at_bound(code: &str, severity: i32) -> Diagnostic {
    let (message, beginner) = strings_of(code);
    let argument = "x".repeat(ARGUMENT_BYTES_MAX);
    let filled = |template: &str| {
        let mut out = String::new();
        let mut rest = template;
        while let Some((head, tail)) = rest.split_once('{') {
            out.push_str(head);
            out.push_str(&argument);
            rest = tail.split_once('}').map_or("", |(_, after)| after);
        }
        out.push_str(rest);
        out
    };
    // A pointer of the bound's length whose characters each take two bytes
    // of JSON once inside the patch's string.
    let pointer = format!("/{}", "a".repeat(MAX_POINTER_BYTES.saturating_sub(1)));
    let patch = format!(
        r#"[{{"op":"replace","path":"{pointer}","value":"{}"}}]"#,
        "v".repeat(200)
    );
    Diagnostic {
        code: code.to_owned(),
        severity,
        path: pointer.clone(),
        related_paths: vec![pointer],
        message: filled(message),
        beginner: filled(beginner),
        suggestions: vec![PatchSuggestion {
            title: argument.clone(),
            json_patch: patch,
            applicability: i32::from(Applicability::MaybeIncorrect),
        }],
        map_refs: vec![
            Voxel {
                x: i32::MIN,
                y: i32::MIN,
                z: i32::MIN,
            };
            2
        ],
    }
}

/// The model holds bytes: a report built to it (every code at the cap but
/// the once-a-report codes, every diagnostic at its bound, `E0001`'s engine
/// words left at the plain bound and its own term checked separately, one
/// `I0007` per code) encodes to no more than the model's sum.
#[test]
fn a_report_built_to_the_models_bounds_is_within_its_sum() {
    let mut diagnostics = Vec::new();
    let mut codes = 0_usize;
    for row in CATALOGUE {
        if !matches!(row.emitter, Emitter::Stage(_)) || row.code == "I0007" {
            continue;
        }
        let once = ONCE_PER_REPORT.contains(&row.code);
        let count = if once { 1 } else { cap() };
        for _ in 0..count {
            diagnostics.push(diagnostic_at_bound(row.code, i32::from(row.severity)));
        }
        if !once {
            codes = codes.saturating_add(1);
        }
    }
    let (message, beginner) = strings_of("I0007");
    for _ in 0..codes {
        diagnostics.push(Diagnostic {
            code: "I0007".to_owned(),
            severity: i32::from(Severity::Info),
            path: String::new(),
            message: message.replace("{count}", &u64::MAX.to_string()),
            beginner: beginner.replace("{count}", &u64::MAX.to_string()),
            ..Diagnostic::default()
        });
    }
    let report = VerifyReport {
        qualifies: false,
        depth: 2,
        diagnostics,
        report_hash: vec![0xff; 8],
        rules_hash: vec![0xff; 8],
        plan_fingerprint: vec![0xff; 8],
        verifier_version: pharmakos_verifier::VERIFIER_VERSION.to_owned(),
        size_units: u32::MAX,
        size_budget: u32::MAX,
    };
    let bytes = json::encode(&report).expect("a report encodes").len();
    let model = worst_report(cap());
    println!("built to the model: {bytes} bytes; the model's sum {model}");
    assert!(
        bytes <= model,
        "{bytes} bytes built, past the model's {model}"
    );
}

fn report_for(steps: &str) -> VerifyReport {
    let text = format!(
        r#"{{"schema_version":{{"major":1}},"meta":{{"title":"long","author_kind":"HUMAN"}},"kind":"PLAYBOOK","declarative":{{"route":[{steps}]}},"on_death":{{"on_respawn":"CONTINUE","max_deaths_before_fallback":1}},"fallback":{{"hold":{{"at":{{"safest":{{}}}}}}}}}}"#
    );
    report_of_text(&text)
}

fn report_of_text(text: &str) -> VerifyReport {
    let rules =
        RulesTable::load(&root().join("rules").join("rules.v1.json")).expect("the rules table");
    let scope = Scope::new(
        SeatId::new(0),
        SeatEconomy {
            treasury: Money::ZERO,
            supply: Kw::ZERO,
            draw: Kw::ZERO,
        },
    );
    let input = if is_too_long(text) {
        Input::too_long(&[], &scope, &rules).expect("the input")
    } else {
        Input::new(text.as_bytes(), &[], &scope, &rules).expect("the input")
    };
    verify(&input, Depth::Full)
}

fn encoded(report: &VerifyReport) -> String {
    json::encode(report).expect("a report encodes")
}

/// How many diagnostics of each code `report` lists.
fn listed(report: &VerifyReport) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for found in &report.diagnostics {
        let count: &mut usize = counts.entry(found.code.clone()).or_default();
        *count = count.saturating_add(1);
    }
    counts
}

/// As many copies of `step(index)` as the cap on the player's text allows.
fn longest_route(step: impl Fn(u32) -> String) -> String {
    let mut steps = String::new();
    let room = MAX_PLAYBOOK_CHARS.saturating_sub(512);
    for index in 0_u32.. {
        let one = format!("{},", step(index));
        if steps.len().saturating_add(one.len()) > room {
            break;
        }
        steps.push_str(&one);
    }
    steps.pop();
    steps
}

/// Whether a report is within the allowance: the test's own constant, never the buffer.
fn within_allowance(report: &str) -> bool {
    report.len() <= REPORT_ALLOWANCE_BYTES
}

/// The longest files the size cap admits, one per way a long file repeats a
/// diagnostic: named holds (an `I0003` note per step), unlabelled holds with
/// no duration (an `E0104`, an `E0303` and an `I0003` per step: about 3.9
/// MiB and 16.8 MiB of report before the per-code cap, `fixq-c`'s review 3),
/// named holds that jump to a label nobody has (an `E0305` per step), and
/// holds paced to avoid threats nobody has seen (an `I0005` per step). Each
/// fits the allowance, lists no code past the cap, and has one `I0007` per
/// code it left out of.
#[test]
fn the_reports_for_the_longest_files_fit_the_allowance() {
    for (name, steps) in [
        (
            "named holds",
            longest_route(|index| format!(r#"{{"label":"s{index}","hold":{{"ms":1}}}}"#)),
        ),
        (
            "unlabelled holds",
            longest_route(|_| r#"{"hold":{}}"#.to_owned()),
        ),
        (
            "unlabelled timed holds",
            longest_route(|_| r#"{"timeout_ms":1,"hold":{"ms":1}}"#.to_owned()),
        ),
        (
            "jumps to nowhere",
            longest_route(|index| {
                format!(
                    r#"{{"label":"s{index}","hold":{{"ms":1}},"on_fail":{{"jump_to_label":"nowhere"}}}}"#
                )
            }),
        ),
        (
            "moves avoiding unseen threats",
            longest_route(|index| {
                format!(
                    r#"{{"label":"m{index}","move":{{"to":{{"safest":{{}}}},"pace":"AVOID_KNOWN_THREATS"}}}}"#
                )
            }),
        ),
    ] {
        assert!(!is_too_long(&steps), "{name}: within the cap on the text");
        let report = report_for(&steps);
        let text = encoded(&report);
        println!("{name}: {} bytes, {:?}", text.len(), listed(&report));
        assert!(
            within_allowance(&text),
            "{name}: a report of {} bytes, past the {REPORT_ALLOWANCE_BYTES}-byte allowance",
            text.len()
        );
        let counts = listed(&report);
        for (code, count) in &counts {
            if code != "I0007" {
                assert!(*count <= cap(), "{name}: {count} {code} listed");
            }
        }
        let capped = counts
            .iter()
            .filter(|(code, count)| code.as_str() != "I0007" && **count == cap())
            .count();
        let markers = counts.get("I0007").copied().unwrap_or(0);
        assert!(markers >= 1, "{name}: a long file left nothing out");
        assert!(
            markers <= capped.saturating_add(1),
            "{name}: {markers} I0007 for {capped} codes at the cap"
        );
    }
}

/// A member name of the player's own as long as the text cap allows, and a
/// reserved name under a long one, are reported at a pointer within the
/// bound: the report names the object holding the member.
#[test]
fn a_long_member_name_is_reported_within_the_pointer_bound() {
    const HEAD: &str = r#""schema_version":{"major":1},"meta":{"title":"long","author_kind":"HUMAN"},"kind":"PLAYBOOK""#;
    let long = "k".repeat(MAX_PLAYBOOK_CHARS.saturating_sub(1_024));
    // Each `\u0001` is six characters of the text: as many as the cap allows.
    let control = "\\u0001".repeat(85_000);
    let reserved: Vec<&str> = (0..100).map(|_| r#"{"set_flag":1}"#).collect();
    let mid = "k".repeat(40_000);
    for (name, text) in [
        ("an unknown long name", format!("{{{HEAD},\"{long}\":1}}")),
        (
            "an unknown name of controls",
            format!("{{{HEAD},\"{control}\":1}}"),
        ),
        (
            "reserved names under a long name",
            format!("{{{HEAD},\"{mid}\":[{}]}}", reserved.join(",")),
        ),
    ] {
        let report = report_of_text(&text);
        assert!(!report.qualifies, "{name}: qualifies");
        assert!(!report.diagnostics.is_empty(), "{name}: nothing said");
        for found in &report.diagnostics {
            for pointer in std::iter::once(&found.path).chain(&found.related_paths) {
                let bytes = json_string_bytes(pointer);
                assert!(
                    bytes <= MAX_POINTER_BYTES,
                    "{name}: {} points {bytes} bytes deep",
                    found.code
                );
            }
        }
        let text = encoded(&report);
        assert!(
            within_allowance(&text),
            "{name}: {} bytes of report",
            text.len()
        );
    }
}

/// How many bytes `text` takes inside a JSON string, as the report's
/// encoder writes it: two for `"` and `\`, six for any other control
/// character, its UTF-8 length otherwise.
fn json_string_bytes(text: &str) -> usize {
    text.chars()
        .map(|character| match character {
            '"' | '\\' => 2,
            other if u32::from(other) < 0x20 => 6,
            other => other.len_utf8(),
        })
        .sum()
}

/// **The allowance is the test's own constant, not the buffer** (fix pass 4 of `fixq-c`,
/// review 3's M7): a text just past 128 KiB is past the allowance, where an eighth of the
/// 4 MiB buffer would take it.
#[test]
fn a_report_just_past_the_allowance_is_past_it() {
    let just_past = "x".repeat(REPORT_ALLOWANCE_BYTES.saturating_add(1));
    assert!(!within_allowance(&just_past));
    assert!(within_allowance(&"x".repeat(REPORT_ALLOWANCE_BYTES)));
}

/// The allowance is no more than an eighth of the client's inbound buffer, so the buffer can
/// never shrink under it unseen.
#[test]
fn the_allowance_is_within_the_clients_buffer() {
    let eighth = inbound_bytes()
        .checked_div(8)
        .expect("a buffer of some size");
    assert!(
        REPORT_ALLOWANCE_BYTES <= eighth,
        "the {REPORT_ALLOWANCE_BYTES}-byte allowance is past an eighth of the client's {} \
         byte buffer",
        inbound_bytes()
    );
}

/// For every count of one code from none to three times the cap, the report
/// lists `min(count, cap)`, and its `I0007` and its typed [`LeftOut`] say
/// `count - cap` past the cap and nothing at or under it.
///
/// [`LeftOut`]: pharmakos_verifier::LeftOut
#[test]
fn a_code_lists_to_the_cap_and_counts_the_rest() {
    let rules =
        RulesTable::load(&root().join("rules").join("rules.v1.json")).expect("the rules table");
    let scope = Scope::new(
        SeatId::new(0),
        SeatEconomy {
            treasury: Money::ZERO,
            supply: Kw::ZERO,
            draw: Kw::ZERO,
        },
    );
    for count in 0..=cap().saturating_mul(3) {
        // `count` jumps to a label nobody has: one `E0305` each.
        let steps: Vec<String> = (0..count)
            .map(|index| {
                format!(
                    r#"{{"label":"s{index}","hold":{{"ms":1}},"on_fail":{{"jump_to_label":"nowhere"}}}}"#
                )
            })
            .collect();
        let text = format!(
            r#"{{"schema_version":{{"major":1}},"meta":{{"title":"t","author_kind":"HUMAN"}},"kind":"PLAYBOOK","declarative":{{"route":[{}]}},"on_death":{{"on_respawn":"CONTINUE","max_deaths_before_fallback":1}},"fallback":{{"hold":{{"at":{{"safest":{{}}}}}}}}}}"#,
            steps.join(",")
        );
        let input = Input::new(text.as_bytes(), &[], &scope, &rules).expect("the input");
        let counted = verify_counted(&input, Depth::Quick);
        let report = &counted.report;
        let listed = report
            .diagnostics
            .iter()
            .filter(|found| found.code == "E0305")
            .count();
        assert_eq!(listed, count.min(cap()), "{count} jumps to nowhere");
        let gap = counted.left_out.iter().find(|gap| gap.code == "E0305");
        let markers: Vec<&Diagnostic> = report
            .diagnostics
            .iter()
            .filter(|found| found.code == "I0007" && found.message.contains("`E0305`"))
            .collect();
        if count > cap() {
            let more = u64::try_from(count.saturating_sub(cap())).expect("fits");
            let gap = gap.expect("a count past the cap is left out");
            assert_eq!((gap.count, gap.max), (more, MAX_PER_CODE));
            assert_eq!(markers.len(), 1, "{count}: one I0007 for E0305");
            assert!(
                markers
                    .first()
                    .is_some_and(|marker| marker.message.starts_with(&format!("{more} more"))),
                "{count}: {markers:?}"
            );
            assert!(
                !report.qualifies,
                "{count}: an error left out still refuses"
            );
        } else {
            assert!(gap.is_none(), "{count}: {gap:?}");
            assert!(markers.is_empty(), "{count}: {markers:?}");
        }
        if count > 0 {
            assert!(!report.qualifies, "{count}: the verdict");
        }
    }
}

#[test]
fn the_notes_stop_at_the_cap_and_one_more_says_how_many_were_left_out() {
    // Within the size budget, with more omitted settings than the cap.
    let steps: Vec<String> = (0..cap().checked_add(20).expect("fits"))
        .map(|index| format!(r#"{{"label":"s{index}","hold":{{"ms":1}}}}"#))
        .collect();
    let report = encoded(&report_for(&steps.join(",")));
    assert_eq!(report.matches("\"I0003\"").count(), cap(), "{report}");
    assert_eq!(report.matches("\"I0007\"").count(), 1, "{report}");
}

/// Past the size budget no `I0003` note is written at all; they are counted.
#[test]
fn past_the_size_budget_no_note_is_listed() {
    let steps = longest_route(|index| format!(r#"{{"label":"s{index}","hold":{{"ms":1}}}}"#));
    let report = report_for(&steps);
    let counts = listed(&report);
    assert!(
        counts.contains_key("E0110"),
        "the route is past the size budget"
    );
    assert_eq!(counts.get("I0003"), None, "{counts:?}");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|found| found.code == "I0007" && found.message.contains("`I0003`")),
        "the notes left out are counted"
    );
}
