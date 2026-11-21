// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Player text reaches a person through the gateway only quoted (S3's
//! `vfyq2`, fix pass 3, item M2): every refusal a planning method makes that
//! echoes something a seat sent (an id, a label, a depth, a draft id, a
//! template id) shows it escaped and capped by the grammar's one rule, and a
//! pointer the refusal carries as data comes back whole.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use pharmakos_proto::json::grammar::HIDDEN;
use pharmakos_proto::json::{self, Json};

use support::{FIRST_LULL_MS, SEGMENT_MS, call_in_lull, hosted_with, library_folder, quote};

/// Text a player sent that a terminal or a message must never show raw:
/// ESC, an OSC title sequence, BEL, a right-to-left override and a tag
/// character.
const HOSTILE: &str = "\u{1b}]0;owned\u{7}\u{202e}zz\u{e0041}";

/// The most a refusal's message may be: a refusal that echoes a 200 KB id
/// whole is a way to fill a log with one call.
const MOST_MESSAGE_CHARS: usize = 2048;

/// The refusal's message, or a panic naming what came back instead.
fn refusal_message(response: &Json, what: &str) -> String {
    let message = response
        .get("error")
        .and_then(|error| error.get("data"))
        .and_then(|data| data.get("message"))
        .or_else(|| response.get("error").and_then(|error| error.get("message")));
    match message {
        Some(Json::String(text)) => text.clone(),
        _ => panic!("{what}: not a refusal with a message: {response:?}"),
    }
}

/// Every character of `message` that the quoting rule escapes, found raw.
fn raw_in(message: &str) -> Vec<String> {
    message
        .chars()
        .filter(|character| {
            HIDDEN
                .iter()
                .any(|(low, high)| *low <= *character && *character <= *high)
        })
        .map(|character| format!("U+{:04X}", u32::from(character)))
        .collect()
}

fn assert_clean(message: &str, what: &str) {
    let raw = raw_in(message);
    assert!(raw.is_empty(), "{what}: raw {raw:?} in {message:?}");
    assert!(
        message.chars().count() <= MOST_MESSAGE_CHARS,
        "{what}: a {}-character message",
        message.chars().count()
    );
}

/// A playbook whose route walks to a beacon named `beacon_id`.
fn walking_to(beacon_id: &str) -> String {
    format!(
        r#"{{"schema_version":{{"major":1}},"meta":{{"title":"case","author_kind":"HUMAN"}},"kind":"PLAYBOOK","declarative":{{"route":[{{"label":"s1","move":{{"to":{{"beacon_anchor":{{"beacon_id":{}}}}}}}}}]}},"on_death":{{"on_respawn":"CONTINUE","max_deaths_before_fallback":1}},"fallback":{{"hold":{{"at":{{"beacon_anchor":{{"safest":{{}}}}}}}}}}}}"#,
        json::write(&Json::String(beacon_id.to_owned()))
    )
}

#[test]
fn every_planning_refusal_quotes_what_a_seat_sent() {
    let mut surface = hosted_with(
        "m-vfyq2-quoting",
        pharmakos_gateway::fog::FogPolicy::fogged(),
        2,
        SEGMENT_MS,
        3,
        Some(library_folder()),
    );
    let token = support::seat_token(&mut surface, 0);
    let mut left = FIRST_LULL_MS;
    let long = format!("{HOSTILE}{}", "x".repeat(200_000));
    let mut call = |method: &str, params: String| {
        call_in_lull(&mut surface, &token, &mut left, method, &params)
    };

    // resolve_refs compiles without verifying, so the compile's refusal names
    // the beacon id the seat wrote.
    for id in [HOSTILE, long.as_str()] {
        let answer = call(
            "resolve_refs",
            format!(r#"{{"playbook_jsonc":{}}}"#, quote(&walking_to(id))),
        );
        assert_clean(&refusal_message(&answer, "resolve_refs"), "resolve_refs");
    }
    // A draft id, a template id and a depth the gateway refuses, each echoed.
    for id in [HOSTILE, long.as_str()] {
        let quoted_id = json::write(&Json::String(id.to_owned()));
        for (method, params) in [
            (
                "save_draft",
                format!(
                    r#"{{"playbook_jsonc":{},"label":"l","draft_id":{quoted_id}}}"#,
                    quote(&walking_to("b_00"))
                ),
            ),
            ("get_draft", format!(r#"{{"draft_id":{quoted_id}}}"#)),
            (
                "instantiate_template",
                format!(r#"{{"template_id":{quoted_id}}}"#),
            ),
            (
                "verify_plan",
                format!(
                    r#"{{"playbook_jsonc":{},"depth":{quoted_id}}}"#,
                    quote(&walking_to("b_00"))
                ),
            ),
        ] {
            let answer = call(method, params);
            assert_clean(&refusal_message(&answer, method), method);
        }
    }
    // A template id the id rule lets through, with a right-to-left override
    // in it, which names no template.
    let answer = call(
        "instantiate_template",
        format!(
            r#"{{"template_id":{}}}"#,
            json::write(&Json::String("hold\u{202e}build".to_owned()))
        ),
    );
    assert_clean(
        &refusal_message(&answer, "instantiate_template"),
        "instantiate_template, U+202E",
    );
}

/// A pointer a refusal carries as data comes back whole, however long, while
/// the message quotes (fix pass 2b's `quoted_pointer`, through the gateway).
#[test]
fn a_patch_refusal_carries_its_pointer_whole() {
    let mut surface = hosted_with(
        "m-vfyq2-pointer",
        pharmakos_gateway::fog::FogPolicy::fogged(),
        2,
        SEGMENT_MS,
        3,
        Some(library_folder()),
    );
    let token = support::seat_token(&mut surface, 0);
    let mut left = FIRST_LULL_MS;
    // A member the anchor does not have: the refusal is at this very path.
    let path = "/declarative/route/0/move/to/beacon_anchor/a_member_with_a_rather_long_name_indeed";
    assert!(path.chars().count() > 64);
    let json_patch = format!(r#"[{{"op":"remove","path":"{path}"}}]"#);
    let answer = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "patch_plan",
        &format!(
            r#"{{"playbook_jsonc":{},"json_patch":{}}}"#,
            quote(&walking_to("b_00")),
            quote(&json_patch)
        ),
    );
    let message = refusal_message(&answer, "patch_plan");
    assert_clean(&message, "patch_plan");
    assert!(message.contains(path), "the pointer whole: {message:?}");
}
