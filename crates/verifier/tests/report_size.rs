// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A report's size, against what the client can take (S3's `vfy6`, the
//! review of PR #179, item 3).
//!
//! The editor reads a report through a socket whose inbound buffer is
//! `INBOUND_BUFFER_BYTES` (`crates/client-gdext/src/transport.rs`, which
//! `godot/scripts/host_link.gd` sets on every peer since S3's `fixq-c`). Decision 8's `I0003` notes
//! come one per omitted setting, so a long file of bare steps once made a
//! report of megabytes where it had been one `E0110`. The notes are capped
//! (`MAX_DEFAULT_NOTES`, then one `I0007` with the count), and none is written
//! once the size budget is blown.

#![allow(
    clippy::expect_used,
    reason = "a panic is this file's failure report, and the helpers are not #[test] functions"
)]

use std::path::PathBuf;

use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::json;
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::math::quantity::{Kw, Money};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::tables::SeatId;
use pharmakos_verifier::{
    Input, MAX_DEFAULT_NOTES, MAX_PLAYBOOK_CHARS, Scope, is_too_long, verify,
};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The most a report for the longest file may be: an eighth of the 1 MiB inbound buffer
/// the client had when this test was written. The report's real cap is lane `vcap`'s to
/// set (decisions-log item 183 (4)); until then the allowance stays where it was, and
/// the client's buffer may only grow under it (`the_allowance_is_within_the_clients_buffer`).
const REPORT_ALLOWANCE_BYTES: usize = 131_072;
// The ruled 128 KiB, held at compile time: raising it is a ruling, not an edit (fix pass 3).
const _: () = assert!(REPORT_ALLOWANCE_BYTES == 128 * 1024);

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

fn report_text(steps: &str) -> String {
    let text = format!(
        r#"{{"schema_version":{{"major":1}},"meta":{{"title":"long","author_kind":"HUMAN"}},"kind":"PLAYBOOK","declarative":{{"route":[{steps}]}},"on_death":{{"on_respawn":"CONTINUE","max_deaths_before_fallback":1}},"fallback":{{"hold":{{"at":{{"safest":{{}}}}}}}}}}"#
    );
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
    let input = Input::new(text.as_bytes(), &[], &scope, &rules).expect("the input");
    json::encode(&verify(&input, Depth::Full)).expect("a report encodes")
}

/// Named bare holds, as many as the cap on the player's text allows: the
/// review's case, a file every step of which is well formed and omits its
/// `on_fail`. (A step with no label is `E0104` each, a refusal per step that
/// this cap does not touch; the PR reports it.)
fn longest_route() -> String {
    let mut steps = String::new();
    let room = MAX_PLAYBOOK_CHARS.saturating_sub(512);
    for index in 0_u32.. {
        let step = format!(r#"{{"label":"s{index}","hold":{{"ms":1}}}},"#);
        if steps.len().saturating_add(step.len()) > room {
            break;
        }
        steps.push_str(&step);
    }
    steps.pop();
    steps
}

#[test]
fn the_report_for_the_longest_file_fits_the_clients_buffer_with_room() {
    let steps = longest_route();
    assert!(!is_too_long(&steps));
    let report = report_text(&steps);
    assert!(
        within_allowance(&report),
        "a report of {} bytes for the longest route, past the {REPORT_ALLOWANCE_BYTES}-byte \
         allowance",
        report.len(),
    );
    // Past the size budget, no per-step note is written at all.
    assert!(
        report.contains("E0110"),
        "the route is past the size budget"
    );
    let notes = report.matches("\"I0003\"").count();
    assert!(notes <= 1, "{notes} I0003 notes past the size budget");
}

/// Whether a report is within the allowance: the test's own constant, never the buffer.
fn within_allowance(report: &str) -> bool {
    report.len() <= REPORT_ALLOWANCE_BYTES
}

/// **The allowance is the test's own constant, not the buffer** (fix pass 4, review 3's M7): a
/// real report just past 128 KiB, made of the per-step `E0104` (a step with no label) that no
/// cap touches, is past the allowance, where an eighth of the 4 MiB buffer would take it.
#[test]
fn a_report_just_past_the_allowance_is_past_it() {
    let mut count = 200_usize;
    let report = loop {
        let steps: Vec<&str> = (0..count).map(|_| r#"{"hold":{"ms":1}}"#).collect();
        let report = report_text(&steps.join(","));
        if report.len() > REPORT_ALLOWANCE_BYTES {
            break report;
        }
        count = count.checked_add(20).expect("fits");
        assert!(
            count < 5_000,
            "no route of bare holds reached the allowance"
        );
    };
    assert!(
        report.len() < REPORT_ALLOWANCE_BYTES.checked_mul(2).expect("fits"),
        "the report is not just past the allowance: {} bytes",
        report.len()
    );
    assert!(
        !within_allowance(&report),
        "a {}-byte report is within the allowance",
        report.len()
    );
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

#[test]
fn the_notes_stop_at_the_cap_and_one_more_says_how_many_were_left_out() {
    // Within the size budget, with more omitted settings than the cap.
    let cap = usize::try_from(MAX_DEFAULT_NOTES).expect("fits");
    let steps: Vec<String> = (0..cap.checked_add(20).expect("fits"))
        .map(|index| format!(r#"{{"label":"s{index}","hold":{{"ms":1}}}}"#))
        .collect();
    let report = report_text(&steps.join(","));
    assert_eq!(report.matches("\"I0003\"").count(), cap, "{report}");
    assert_eq!(report.matches("\"I0007\"").count(), 1, "{report}");
}
