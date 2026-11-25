// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A report's size, against what the client can take (S3's `vfy6`, the
//! review of PR #179, item 3; S3's `vcap`, decisions-log item 183 (4)).
//!
//! The editor reads a report in one WebSocket message. The gateway sends no
//! answer past `pharmakos_proto::transport::MAX_ANSWER_BYTES` (S3's `outcap`),
//! and the client's inbound buffer is that cap and a frame's header
//! (`INBOUND_BUFFER_BYTES`, `crates/client-gdext/src/transport.rs`, which
//! `godot/scripts/host_link.gd` sets on every peer). A report lists at most
//! `MAX_PER_CODE` diagnostics of one code and closes with one `I0007` per code
//! it left out of.
//!
//! The derivation of the cap is `report.rs`'s own: its unit tests build the
//! worst report through the builder and the serialiser at the cap and at one
//! more (`the_cap_is_the_top_of_its_range`). This file holds the reports for
//! the longest real files the text cap admits to the 128 KiB allowance, an
//! eighth of the buffer or less, and a hostile member name to the bounds.

#![allow(
    clippy::expect_used,
    reason = "a panic is this file's failure report, and the helpers are not #[test] functions"
)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::gp::api::v1::{Diagnostic, VerifyReport};
use pharmakos_proto::json;
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::math::quantity::{Kw, Money};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::tables::SeatId;
use pharmakos_verifier::{
    Cuts, Input, MAX_PER_CODE, MAX_PLAYBOOK_CHARS, MAX_POINTER_BYTES, Scope, diagnostic_bytes,
    diagnostic_bytes_max, is_too_long, json_bytes, verify, verify_counted,
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

/// The largest answer the gateway sends, from its one source (S3's `outcap`):
/// the client's inbound buffer is this and a frame's header, so a report the
/// gateway sends is one the client takes whole.
fn inbound_bytes() -> usize {
    pharmakos_proto::transport::MAX_ANSWER_BYTES
}

fn cap() -> usize {
    usize::try_from(MAX_PER_CODE).expect("fits")
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
                let bytes = json_bytes(pointer);
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

/// The review's probe (S3's `vcap`, fix pass 1): a member name of a hundred
/// tag characters, each of which the quoting rule writes as two `\uXXXX`
/// escapes, fourteen bytes once the report's JSON doubles each backslash.
/// The `E0002` that names it is within its code's bound, and nothing had to
/// be cut for it to be: the argument bound is the quoting rule's own worst.
#[test]
fn a_member_name_of_tag_characters_is_within_its_bounds() {
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
    let tags: String = std::iter::repeat_n('\u{e0041}', 100).collect();
    let text = format!(r#"{{"schema_version":{{"major":1}},"{tags}":1}}"#);
    let input = Input::new(text.as_bytes(), &[], &scope, &rules).expect("the input");
    let counted = verify_counted(&input, Depth::Full);
    assert!(counted.faults.is_empty(), "{:?}", counted.faults);
    assert_eq!(counted.cuts, Cuts::default());
    let named = counted
        .report
        .diagnostics
        .iter()
        .find(|found| found.code == "E0002")
        .expect("the member is refused by name");
    assert!(
        diagnostic_bytes(named) <= diagnostic_bytes_max("E0002").expect("a bound"),
        "{} bytes",
        diagnostic_bytes(named)
    );
}

/// **The decode's pointer is cut at the report's door, and counted** (S3's
/// `vcap`, fix pass 2): a member name of 500 KiB is refused as `E0002` at the
/// object that holds it, with its Fix dropped, and the report counts one
/// pointer cut and one Fix dropped.
#[test]
fn a_long_member_name_s_pointer_is_cut_and_counted_at_the_door() {
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
    let long = "k".repeat(500 * 1024);
    let text = format!(r#"{{"schema_version":{{"major":1}},"{long}":1}}"#);
    assert!(!is_too_long(&text));
    let input = Input::new(text.as_bytes(), &[], &scope, &rules).expect("the input");
    let counted = verify_counted(&input, Depth::Full);
    assert!(counted.faults.is_empty(), "{:?}", counted.faults);
    assert_eq!(counted.cuts.pointers, 1, "{:?}", counted.cuts);
    assert_eq!(counted.cuts.fixes, 1, "{:?}", counted.cuts);
    let named = counted
        .report
        .diagnostics
        .iter()
        .find(|found| found.code == "E0002")
        .expect("the member is refused by name");
    assert_eq!(named.path, "", "the object that holds it: the document");
    assert!(named.suggestions.is_empty());
}
