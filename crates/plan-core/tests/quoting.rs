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
        holds(what, refused.message());
    }
    let duplicated = MINIMAL.replace(
        "\"kind\":\"PLAYBOOK\"}",
        &format!("\"kind\":\"PLAYBOOK\",{hostile_json}:1,{hostile_json}:2}}"),
    );
    let refused = Document::parse(&duplicated).expect_err("a key written twice");
    holds("a key written twice", refused.message());
    let unknown = MINIMAL.replace(
        "\"kind\":\"PLAYBOOK\"}",
        &format!("\"kind\":\"PLAYBOOK\",{hostile_json}:1}}"),
    );
    let refused = canonicalise_text(&unknown).expect_err("an unknown field");
    holds("an unknown field", refused.message());
    let enum_value = MINIMAL.replace("\"HUMAN\"", &hostile_json);
    let refused = canonicalise_text(&enum_value).expect_err("an enum value");
    holds("an enum value", refused.message());
    let refused = instantiate(
        MINIMAL,
        &[Parameter {
            name: "/meta/title".to_owned(),
            value: format!("{}x", hostile()),
        }],
        &[],
    )
    .expect_err("a parameter that is not JSON");
    holds("a parameter's value", refused.message());
}

/// The prose shows a player's text through the grammar's one rule (S3's
/// `vfyq2`, fix pass 3, item M2 (c)): a title, a label and a tag holding a
/// right-to-left override, a soft hyphen, a tag character or a line separator
/// come back escaped, not raw, and are not cut, because prose is the player's
/// own text read back in full.
#[test]
fn the_prose_escapes_every_hidden_character_and_cuts_nothing() {
    use pharmakos_plan_core::context::PlanContext;
    use pharmakos_plan_core::render::render_plan;
    use pharmakos_proto::json::grammar::HIDDEN;
    let hidden = "a\u{202e}b\u{ad}c\u{e0041}d\u{2028}e";
    let long = "z".repeat(300);
    let text = MINIMAL
        .replace(
            "\"title\":\"t\"",
            &format!("\"title\":{}", as_json(&format!("{hidden}{long}"))),
        )
        .replace("\"label\":\"a\"", &format!("\"label\":{}", as_json(hidden)));
    let canonical = canonicalise_text(&text).expect("a canonical form");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the workspace root")
        .to_path_buf();
    let rules = pharmakos_sim::rules::RulesTable::load(&root.join("rules").join("rules.v1.json"))
        .expect("the committed rules table");
    let snapshot = pharmakos_sim::snapshot::Snapshot {
        seat_id: vec![0],
        seat_treasury: vec![200],
        seat_supply: vec![10],
        seat_draw: vec![2],
        ..pharmakos_sim::snapshot::Snapshot::default()
    };
    let context = PlanContext::from_snapshot(&snapshot, 0).expect("seat 0 is in the snapshot");
    let prose = render_plan(&canonical.playbook, &context, &rules).expect("prose");
    let raw: Vec<String> = prose
        .chars()
        .filter(|character| *character != '\n')
        .filter(|character| {
            HIDDEN
                .iter()
                .any(|(low, high)| *low <= *character && *character <= *high)
        })
        .map(|character| format!("U+{:04X}", u32::from(character)))
        .collect();
    assert!(raw.is_empty(), "raw {raw:?} in the prose:\n{prose}");
    assert!(prose.contains(&long), "the title is not cut:\n{prose}");
    assert!(prose.contains("\\u202e"), "{prose}");
}

/// Quoting never rewrites the file's bytes (fix pass 4, item 2): a label
/// holding a zero-width joiner, a zero-width non-joiner, a soft hyphen and a
/// variation selector survives a patch, the canonical form (what a save
/// writes) and the file the editor holds byte for byte; only refusals and
/// prose escape it.
#[test]
fn quoting_never_rewrites_the_files_bytes() {
    let label = "a\u{200d}b\u{200c}c\u{ad}d\u{fe0f}e";
    let text = MINIMAL.replace("\"label\":\"a\"", &format!("\"label\":{}", as_json(label)));
    let (patched, inverse) = patch_text(
        &text,
        r#"[{"op":"replace","path":"/meta/title","value":"edited"}]"#,
    )
    .expect("the patch applies");
    assert!(patched.contains(label), "the patch keeps the label's bytes");
    let (back, _) = patch_text(&patched, &inverse).expect("the undo applies");
    assert_eq!(back, text, "the undo gives the file back byte for byte");
    let canonical = canonicalise_text(&text).expect("a canonical form");
    assert!(
        canonical.text.contains(label),
        "the canonical form keeps the label's bytes"
    );
    let decoded = pharmakos_plan_core::decode_jsonc(&text).expect("it decodes");
    let route = decoded
        .decoded()
        .message()
        .declarative
        .as_ref()
        .and_then(|body| body.route.first())
        .map(|step| step.label.clone());
    assert_eq!(
        route.as_deref(),
        Some(label),
        "the decoded label is the label"
    );
}
