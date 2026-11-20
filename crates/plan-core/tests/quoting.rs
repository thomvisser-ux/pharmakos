// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The planning methods' refusals show a player's text through one rule
//! (S3's `vfyq2`, fix pass 2, item B).
//!
//! `render_plan`, `patch_plan`, `instantiate_template` and `verify_plan` read
//! the player's text through plan-core, and a refusal they answer goes to the
//! editor's status line and to `gamectl`'s terminal. Each refusal here carries
//! hostile text (ESC, BEL, U+202E RIGHT-TO-LEFT OVERRIDE and 200,000 more
//! characters) where the player wrote it, and must show it escaped and
//! capped: no raw control or reordering character, and a sentence's length.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml's allow-expect-in-tests only recognises #[test] functions, not an \
              integration test's helpers. A panic is this file's failure report."
)]

use pharmakos_plan_core::canonical::canonicalise_text;
use pharmakos_plan_core::jsonc::Document;
use pharmakos_plan_core::library::{Parameter, instantiate};
use pharmakos_plan_core::patch::patch_text;
use pharmakos_proto::json::{Json, write};

const MINIMAL: &str = concat!(
    "{\"schema_version\":{\"major\":1},",
    "\"meta\":{\"title\":\"t\",\"author_kind\":\"HUMAN\"},",
    "\"declarative\":{\"route\":[{\"label\":\"a\",\"hold\":{\"ms\":1000}}]},",
    "\"on_death\":{\"on_respawn\":\"CONTINUE\"},",
    "\"fallback\":{\"hold\":{\"at\":{\"beacon_anchor\":{\"safest\":{}}}}},",
    "\"kind\":\"PLAYBOOK\"}"
);

fn hostile() -> String {
    format!("\u{1b}]0;x\u{7}\u{202e}{}", "y".repeat(200_000))
}

/// The hostile text as a JSON string, quotes included.
fn as_json(text: &str) -> String {
    write(&Json::String(text.to_owned())).trim_end().to_owned()
}

/// A refusal's message, held to the rule.
fn holds(what: &str, message: &str) {
    let raw = message.chars().find(|character| {
        character.is_control()
            || ('\u{202a}'..='\u{202e}').contains(character)
            || ('\u{2066}'..='\u{2069}').contains(character)
    });
    assert!(raw.is_none(), "{what}: shows {raw:?} raw");
    assert!(
        message.chars().count() <= 600,
        "{what}: {} characters",
        message.chars().count()
    );
    assert!(
        message.contains("\\u001b") && message.contains("\\u202e"),
        "{what}: does not show the hostile text quoted: {message}"
    );
}

#[test]
fn every_planning_refusal_quotes_the_player_text_it_carries() {
    let hostile_json = as_json(&hostile());
    for (what, patch) in [
        (
            "an operation's name",
            format!(r#"[{{"op":{hostile_json},"path":"/a"}}]"#),
        ),
        (
            "a member of an operation",
            format!(r#"[{{"op":"remove","path":"/kind",{hostile_json}:1}}]"#),
        ),
    ] {
        let refused = patch_text(MINIMAL, &patch).expect_err(what);
        holds(what, &refused.message);
    }
    let duplicated = MINIMAL.replace(
        "\"kind\":\"PLAYBOOK\"}",
        &format!("\"kind\":\"PLAYBOOK\",{hostile_json}:1,{hostile_json}:2}}"),
    );
    let refused = Document::parse(&duplicated).expect_err("a key written twice");
    holds("a key written twice", &refused.message);
    let unknown = MINIMAL.replace(
        "\"kind\":\"PLAYBOOK\"}",
        &format!("\"kind\":\"PLAYBOOK\",{hostile_json}:1}}"),
    );
    let refused = canonicalise_text(&unknown).expect_err("an unknown field");
    holds("an unknown field", &refused.message);
    let enum_value = MINIMAL.replace("\"HUMAN\"", &hostile_json);
    let refused = canonicalise_text(&enum_value).expect_err("an enum value");
    holds("an enum value", &refused.message);
    let refused = instantiate(
        MINIMAL,
        &[Parameter {
            name: "/meta/title".to_owned(),
            value: format!("{}x", hostile()),
        }],
        &[],
    )
    .expect_err("a parameter that is not JSON");
    holds("a parameter's value", &refused.message);
}
