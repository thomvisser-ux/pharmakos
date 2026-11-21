// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A report quotes a player's text through one rule (S3's `vfyq2`, fix pass
//! 1, item 5).
//!
//! The security review of PR #198 found a key holding ESC and BEL reaching
//! `gamectl verify`'s terminal and the report raw, an escaped `é` reported as
//! a Latin-1 byte, and a 200 KB key echoed whole. Every quote of player text
//! in the codec's refusals and the verifier's diagnostics now goes through
//! `pharmakos_proto::json::grammar::quoted`: control characters as `\u00XX`
//! and at most `MAX_QUOTED_CHARS` characters, the rest `…`.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml's allow-expect-in-tests only recognises #[test] functions, not an \
              integration test's helpers. A panic is this file's failure report."
)]

use pharmakos_proto::gp::api::v1::VerifyReport;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::snapshot::{SNAPSHOT_VERSION, Snapshot};
use pharmakos_sim::tables::SeatId;
use pharmakos_verifier::{Input, Scope, verify};

const MINIMAL: &str = concat!(
    "{\"schema_version\":{\"major\":1},",
    "\"meta\":{\"title\":\"t\",\"author_kind\":\"HUMAN\"},",
    "\"declarative\":{\"route\":[{\"label\":\"a\",\"hold\":{\"ms\":1000}}]},",
    "\"on_death\":{\"on_respawn\":\"CONTINUE\"},",
    "\"fallback\":{\"hold\":{\"at\":{\"beacon_anchor\":{\"safest\":{}}}}},",
    "\"kind\":\"PLAYBOOK\"}"
);

fn report(text: &str) -> VerifyReport {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the workspace root");
    let rules = RulesTable::load(&root.join("rules").join("rules.v1.json"))
        .expect("the committed rules table");
    let snapshot = Snapshot {
        version: SNAPSHOT_VERSION,
        ..Snapshot::default()
    }
    .to_bytes()
    .expect("a snapshot");
    let scope = Scope::new(SeatId::new(0), SeatEconomy::default());
    let input = Input::new(text.as_bytes(), &snapshot, &scope, &rules).expect("an input");
    verify(&input, Depth::Quick)
}

/// The longest message a diagnostic may carry: a sentence around quotes of
/// at most `MAX_QUOTED_CHARS` (64) characters each. The longest sentence in
/// `strings.rs` is under 200 characters.
const LONGEST_MESSAGE: usize = 400;

/// Every message of every diagnostic: no control character, and nothing
/// longer than a sentence around capped quotes.
fn messages_are_safe(what: &str, report: &VerifyReport) {
    assert!(!report.diagnostics.is_empty(), "{what}: a diagnostic");
    for diagnostic in &report.diagnostics {
        assert!(
            !diagnostic.message.chars().any(char::is_control),
            "{what}: {} carries a control character: {:?}",
            diagnostic.code,
            diagnostic.message
        );
        assert!(
            diagnostic.message.chars().count() <= LONGEST_MESSAGE,
            "{what}: {} is {} characters long",
            diagnostic.code,
            diagnostic.message.chars().count()
        );
    }
}

#[test]
fn a_key_holding_terminal_controls_is_quoted_escaped() {
    for (what, text) in [
        (
            "an unknown key",
            MINIMAL.replace(
                "\"kind\":\"PLAYBOOK\"}",
                "\"kind\":\"PLAYBOOK\",\"\\u001b]0;pwned\\u0007\":1}",
            ),
        ),
        (
            "a key written twice",
            MINIMAL.replace(
                "\"kind\":\"PLAYBOOK\"}",
                "\"kind\":\"PLAYBOOK\",\"\\u001b[31mX\":1,\"\\u001b[31mX\":2}",
            ),
        ),
        (
            "an enum value",
            MINIMAL.replace("\"HUMAN\"", "\"\\u001b[2J\""),
        ),
        (
            "an unknown escape of a control",
            MINIMAL.replace("\"title\":\"t\"", "\"title\":\"\\\u{1b}\""),
        ),
    ] {
        let report = report(&text);
        messages_are_safe(what, &report);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("\\u001b")),
            "{what}: the ESC is shown as an escape: {:?}",
            report.diagnostics
        );
    }
}

#[test]
fn an_escaped_multibyte_character_is_quoted_whole() {
    let report = report(&MINIMAL.replace("\"title\":\"t\"", "\"title\":\"\\\u{e9}\""));
    messages_are_safe("an unknown escape of é", &report);
    let detail = &report.diagnostics.first().expect("one diagnostic").message;
    assert!(detail.contains("\\\u{e9}"), "{detail}");
}

#[test]
fn a_long_key_is_quoted_short() {
    let big = "k".repeat(200_000);
    for (what, text) in [
        (
            "an unknown key",
            MINIMAL.replace("\"kind\":\"PLAYBOOK\"}", &format!("\"kind\":\"PLAYBOOK\",\"{big}\":1}}")),
        ),
        (
            "a key written twice",
            MINIMAL.replace(
                "\"kind\":\"PLAYBOOK\"}",
                &format!("\"kind\":\"PLAYBOOK\",\"{big}\":1,\"{big}\":2}}"),
            ),
        ),
        ("a long label never resolved", MINIMAL.replace(
            "\"hold\":{\"ms\":1000}}",
            &format!("\"hold\":{{\"ms\":1000}},\"on_fail\":{{\"action\":\"JUMP\",\"jump_to_label\":\"{big}\"}}}}"),
        )),
    ] {
        messages_are_safe(what, &report(&text));
    }
}
