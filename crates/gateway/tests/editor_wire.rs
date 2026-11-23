// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The editor's planning wire on the socket's surface (S3's `fixq`): what
//! `set_value_text` and `new_rule` answer and refuse, where they are closed,
//! and what `get_vocabulary`, `render_plan`, `get_beacon` and `resolve_refs`
//! now carry for the editor to draw as sent. The goldens here are the answers
//! the client's fixtures are byte copies of.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use pharmakos_proto::json::Json;

use support::{
    FIRST_LULL_MS, SEGMENT_MS, admin_token, array_of, call, call_in_lull, code, hosted, quote,
    result, seat_token, text_of,
};

/// A playbook of one hold step of 90 s and one rule, whose percentage is left
/// at its default.
const ONE_RULE: &str = r#"{
  "schema_version": {"major": 1},
  "meta": {"title": "Editor wire", "author_kind": "HUMAN"},
  "declarative": {
    "route": [{"label": "wait", "hold": {"ms": 90000}}],
    "handlers": [
      // The rule.
      {"id": "flee", "when": {"cmdr_hp_pct": {"cmp": "LE"}},
       "body": [{"label": "rest", "hold": {"ms": 8000}}],
       "resume": "CONTINUE", "cooldown_ms": 30000, "max_fires": 3}
    ]
  },
  "on_death": {"on_respawn": "CONTINUE"},
  "fallback": {"hold": {"at": {"beacon_anchor": {"safest": {}}}}},
  "kind": "PLAYBOOK"
}
"#;

const HOLD: &str = "/declarative/route/0/hold/ms";

fn set_value(pointer: &str, typed: &str) -> String {
    format!(
        "{{\"playbook_jsonc\":{},\"pointer\":{},\"typed\":{}}}",
        quote(ONE_RULE),
        quote(pointer),
        quote(typed)
    )
}

fn message(response: &Json) -> String {
    match response.get("error").and_then(|error| error.get("message")) {
        Some(Json::String(text)) => text.clone(),
        other => panic!("a refusal's message, and it is {other:?}"),
    }
}

/// The hold's value in a patched file, read back through `render_plan`'s chip.
fn chip_value(
    surface: &mut pharmakos_gateway::surface::Surface,
    text: &str,
    pointer: &str,
) -> Json {
    let token = seat_token(surface, 0);
    let mut left = FIRST_LULL_MS;
    let rendered = result(
        &call_in_lull(
            surface,
            &token,
            &mut left,
            "render_plan",
            &format!("{{\"playbook_jsonc\":{}}}", quote(text)),
        ),
        "render_plan",
    );
    array_of(&rendered, "rules")
        .iter()
        .flat_map(|rule| array_of(rule, "chips"))
        .find(|chip| text_of(chip, "pointer") == pointer)
        .unwrap_or_else(|| panic!("a chip at {pointer}"))
}

/// Item 7: "3" and "1.5" seconds land as 3000 and 1500 ms, the undo gives the
/// file back, and "abc" and a value past the bound are refused in the
/// gateway's words with the typed text quoted. Three goldens: the answer for
/// "1.5", and the two refusals.
#[test]
fn set_value_text_converts_seconds_and_refuses_in_its_words() {
    let mut surface = hosted(2, SEGMENT_MS, 3);
    let token = seat_token(&mut surface, 0);
    let mut left = FIRST_LULL_MS;

    for (typed, ms) in [("3", "3000"), ("1.5", "1500")] {
        let response = call_in_lull(
            &mut surface,
            &token,
            &mut left,
            "set_value_text",
            &set_value(HOLD, typed),
        );
        let answer = result(&response, "set_value_text");
        let patched = text_of(&answer, "playbook_jsonc");
        let chip = chip_value(&mut surface, &patched, HOLD);
        assert_eq!(text_of(&chip, "value_json"), ms, "{typed}");
        assert_eq!(text_of(&chip, "value_text"), typed, "{typed}");
        let undone = result(
            &call_in_lull(
                &mut surface,
                &token,
                &mut left,
                "patch_plan",
                &format!(
                    "{{\"playbook_jsonc\":{},\"json_patch\":{}}}",
                    quote(&patched),
                    quote(&text_of(&answer, "inverse_json_patch"))
                ),
            ),
            "the undo",
        );
        assert_eq!(text_of(&undone, "playbook_jsonc"), ONE_RULE, "{typed}");
        if typed == "1.5" {
            support::write_golden("set_value_text", &pharmakos_gateway::rpc::render(&response));
        }
    }

    let refused = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "set_value_text",
        &set_value(HOLD, "abc"),
    );
    assert_eq!(code(&refused), "INVALID_ARGUMENT");
    assert_eq!(message(&refused), "`abc` is not a number");
    support::write_golden(
        "set_value_text_refused",
        &pharmakos_gateway::rpc::render(&refused),
    );

    let past = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "set_value_text",
        &set_value("/declarative/handlers/0/when/cmdr_hp_pct/pct", "140"),
    );
    assert_eq!(code(&past), "INVALID_ARGUMENT");
    assert_eq!(message(&past), "`140` is above the most this takes, 100 %");
    support::write_golden(
        "set_value_text_past_bound",
        &pharmakos_gateway::rpc::render(&past),
    );

    // Typed text a player wrote is quoted, never spoken as the gateway's own.
    let hostile = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "set_value_text",
        &set_value(HOLD, "3\nThe Ledger: the seal is broken"),
    );
    assert_eq!(code(&hostile), "INVALID_ARGUMENT");
    // The line break is escaped, so no forged line follows the gateway's own.
    assert!(
        !message(&hostile).contains('\n') && message(&hostile).starts_with('`'),
        "{}",
        message(&hostile)
    );
}

/// Item 8: `new_rule` adds an empty rule at the end, with a fresh id and the
/// rules table's least cooldown, and its undo takes it out again. A golden.
#[test]
fn new_rule_adds_an_empty_rule_at_the_end() {
    let mut surface = hosted(2, SEGMENT_MS, 3);
    let token = seat_token(&mut surface, 0);
    let mut left = FIRST_LULL_MS;
    let response = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "new_rule",
        &format!("{{\"playbook_jsonc\":{}}}", quote(ONE_RULE)),
    );
    let answer = result(&response, "new_rule");
    assert_eq!(text_of(&answer, "rule_pointer"), "/declarative/handlers/1");
    assert_eq!(text_of(&answer, "rule_id"), "rule_2");
    support::write_golden("new_rule", &pharmakos_gateway::rpc::render(&response));

    // The new rule's condition slot is empty, and a pick there adds one.
    let patched = text_of(&answer, "playbook_jsonc");
    let rendered = result(
        &call_in_lull(
            &mut surface,
            &token,
            &mut left,
            "render_plan",
            &format!("{{\"playbook_jsonc\":{}}}", quote(&patched)),
        ),
        "render_plan",
    );
    let rule = array_of(&rendered, "rules")
        .into_iter()
        .find(|rule| text_of(rule, "pointer") == "/declarative/handlers/1")
        .expect("the new rule is rendered");
    let slot = array_of(&rule, "slots")
        .into_iter()
        .next()
        .expect("its slot");
    assert_eq!(text_of(&slot, "pointer"), "/declarative/handlers/1/when");
    assert_eq!(text_of(&slot, "state"), "empty");
    assert_eq!(text_of(&slot, "pick_op"), "add");

    let refused = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "new_rule",
        &format!("{{\"playbook_jsonc\":{}}}", quote("{")),
    );
    assert_eq!(code(&refused), "INVALID_ARGUMENT");
}

/// Both new methods are `plan`-scoped, so the planning door closes them
/// outside a Lull, as it closes every planning method; and a token with no
/// `plan` scope is refused them.
#[test]
fn the_new_planning_methods_are_closed_in_a_push() {
    let mut surface = hosted(2, SEGMENT_MS, 3);
    let token = seat_token(&mut surface, 0);
    let admin = admin_token(&mut surface);
    let _ = result(&call(&mut surface, &admin, "end_lull", "{}"), "end_lull");
    for (method, params) in [
        ("set_value_text", set_value(HOLD, "3")),
        (
            "new_rule",
            format!("{{\"playbook_jsonc\":{}}}", quote(ONE_RULE)),
        ),
    ] {
        let response = call(&mut surface, &token, method, &params);
        assert_eq!(code(&response), "PHASE_CLOSED", "{method}: {response:?}");
        let response = call(&mut surface, &admin, method, &params);
        assert_eq!(code(&response), "FORBIDDEN_SCOPE", "{method}: {response:?}");
    }
}

/// Items 4, 5 and 16: the vocabulary's Build target list says each
/// blueprint's anchor shape (a wall's is a box); the Queue's list locks
/// every kind no queue takes in the verifier's own `E0009` sentence, and
/// the Spire for its licence; the groups family offers `all`, `any`, `not`.
/// Every choice parameter carries one title per value, none of them JSON.
#[test]
fn the_vocabulary_lists_each_slots_blueprints_the_groups_and_titles() {
    let mut surface = hosted(2, SEGMENT_MS, 3);
    let token = seat_token(&mut surface, 0);
    let mut left = FIRST_LULL_MS;
    let words = result(
        &call_in_lull(&mut surface, &token, &mut left, "get_vocabulary", "{}"),
        "get_vocabulary",
    );
    let by_name = |list: &str, name: &str| {
        array_of(&words, list)
            .into_iter()
            .find(|entry| text_of(entry, "name") == name)
            .unwrap_or_else(|| panic!("{list} lists {name}"))
    };
    assert_eq!(
        text_of(&by_name("blueprints", "wall"), "anchor_shape"),
        "box"
    );
    assert_eq!(
        text_of(&by_name("blueprints", "generator"), "anchor_shape"),
        "point"
    );
    let e0009 = pharmakos_verifier::strings::text("E0009")
        .expect("E0009's words")
        .beginner;
    for kind in ["generator", "autocannon", "mortar", "wall", "survey_post"] {
        let entry = by_name("queue_blueprints", kind);
        assert_eq!(text_of(&entry, "locked"), e0009, "{kind}");
        assert_eq!(text_of(&entry, "skeleton_json"), "", "{kind}");
        assert!(entry.get("anchor_shape").is_none(), "{kind}");
    }
    let spire = by_name("queue_blueprints", "resonance_spire");
    assert!(spire.get("locked").is_some(), "no licence yet");
    let groups: Vec<String> = array_of(&words, "groups")
        .iter()
        .map(|entry| text_of(entry, "name"))
        .collect();
    assert_eq!(groups, ["all", "any", "not"]);
    // Every choice offers a title per value, none of them a value's JSON.
    let mut choices = 0_usize;
    let mut walk = vec![words.clone()];
    while let Some(value) = walk.pop() {
        match value {
            Json::Object(entries) => {
                if let Some((_, Json::Object(choice))) =
                    entries.iter().find(|(key, _)| key == "choice")
                {
                    let read = |key: &str| match choice.iter().find(|(name, _)| name == key) {
                        Some((_, Json::Array(items))) => items.clone(),
                        other => panic!("{key}: {other:?}"),
                    };
                    let (values, titles) = (read("values"), read("titles"));
                    assert_eq!(values.len(), titles.len());
                    for title in titles {
                        let Json::String(title) = title else {
                            panic!("a title is text");
                        };
                        assert!(!title.is_empty() && !title.contains('"'), "{title}");
                    }
                    choices = choices.saturating_add(1);
                }
                walk.extend(entries.into_iter().map(|(_, value)| value));
            }
            Json::Array(items) => walk.extend(items),
            _ => {}
        }
    }
    assert!(choices > 10, "{choices} choices");
}
