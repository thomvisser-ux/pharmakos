// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A report's size, against what the client can take (S3's `vfy6`, the
//! review of PR #179, item 3).
//!
//! The editor reads a report through a socket whose inbound buffer is
//! `INBOUND_BYTES` (`godot/scripts/host_link.gd`). Decision 8's `I0003` notes
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

/// The client's inbound buffer, read from the client's own script rather
/// than written here a second time.
fn inbound_bytes() -> usize {
    let script = std::fs::read_to_string(root().join("godot/scripts/host_link.gd"))
        .expect("the client's host link");
    let line = script
        .lines()
        .find(|line| line.starts_with("const INBOUND_BYTES := "))
        .expect("host_link.gd declares INBOUND_BYTES");
    line.trim_start_matches("const INBOUND_BYTES := ")
        .trim()
        .parse()
        .expect("INBOUND_BYTES is a whole number")
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
    let budget = inbound_bytes()
        .checked_div(8)
        .expect("a buffer of some size");
    assert!(
        report.len() <= budget,
        "a report of {} bytes for the longest route, past an eighth of the client's {} \
         byte buffer",
        report.len(),
        inbound_bytes()
    );
    // Past the size budget, no per-step note is written at all.
    assert!(
        report.contains("E0110"),
        "the route is past the size budget"
    );
    let notes = report.matches("\"I0003\"").count();
    assert!(notes <= 1, "{notes} I0003 notes past the size budget");
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
