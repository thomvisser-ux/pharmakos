// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The deepest text the grammar accepts, and the shallowest it refuses, through
//! every planning method's real door on half the stack the gateway serves on
//! (S3's `vfyq2`, fix pass 2, item E).
//!
//! The door is [`Surface::call`] behind the JSON-RPC frame parser, which is
//! what the surface thread runs for a socket's frame and for an in-process
//! seat alike. The thread is half of
//! [`pharmakos_plan_core::stack::PLAYER_TEXT_STACK`], and the build is the
//! tests' (debug, where frames are largest), so a pass here is a factor of two
//! over the worst build rather than a pass at the size. A reader that needs
//! more overflows, which aborts this test binary: red.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::token::Token;
use pharmakos_plan_core::stack::PLAYER_TEXT_STACK;
use pharmakos_proto::json::Json;
use pharmakos_proto::json::grammar::MAX_DEPTH;

use support::{FIRST_LULL_MS, SEGMENT_MS, call_in_lull, hosted_with, library_folder, quote};

const MINIMAL: &str = concat!(
    "{\"schema_version\":{\"major\":1},",
    "\"meta\":{\"title\":\"t\",\"author_kind\":\"HUMAN\"},",
    "\"declarative\":{\"route\":[{\"label\":\"a\",\"hold\":{\"ms\":1000}}]},",
    "\"on_death\":{\"on_respawn\":\"CONTINUE\"},",
    "\"fallback\":{\"hold\":{\"at\":{\"beacon_anchor\":{\"safest\":{}}}}},",
    "\"kind\":\"PLAYBOOK\"}"
);

/// The `not`s that put a handler condition's numbers inside exactly
/// [`MAX_DEPTH`] containers (plan-core's `tests/stack.rs` counts them).
const DEEPEST_NOTS: usize = 27;

/// A handler whose condition is an `all` group around `nots` nested `not`s,
/// each level carrying a block comment.
fn nested(nots: usize) -> String {
    let mut when = r#"{"segment_elapsed":{"ms":{"op":"GE","value":1}}}"#.to_owned();
    when = format!(r#"{{"all":{{"items":[{when}]}}}}"#);
    for _ in 0..nots {
        when = format!(r#"{{ /* a level */ "not":{{ /* a level */ "item":{when}}}}}"#);
    }
    MINIMAL.replace(
        "\"declarative\":{",
        &format!(
            r#""declarative":{{"handlers":[{{"id":"h","when":{when},"body":[{{"label":"b","hold":{{"ms":1}}}}],"resume":"CONTINUE"}}],"#
        ),
    )
}

/// The pointer of the predicate's `value` under `nots` nested `not`s.
fn deepest_pointer(nots: usize) -> String {
    let mut pointer = "/declarative/handlers/0/when".to_owned();
    for _ in 0..nots {
        pointer.push_str("/not/item");
    }
    pointer.push_str("/all/items/0/segment_elapsed/ms/value");
    pointer
}

/// `levels` arrays around a number, as JSON text.
fn arrays(levels: usize) -> String {
    format!("{}1{}", "[".repeat(levels), "]".repeat(levels))
}

/// Whether the answer is a JSON-RPC answer at all: a result or a typed
/// refusal. What it says is not this file's business; that it came back is.
fn answered(response: &Json, what: &str) -> Json {
    if let Some(result) = response.get("result") {
        return result.clone();
    }
    let code = response
        .get("error")
        .and_then(|error| error.get("data"))
        .and_then(|data| data.get("code"));
    assert!(
        code.is_some(),
        "{what}: neither a result nor a coded refusal: {response:?}"
    );
    Json::Null
}

/// Every planning method, over the deepest text and the shallowest refused one.
fn every_planning_method() {
    let mut surface = hosted_with(
        "m-vfyq2-stack",
        pharmakos_gateway::fog::FogPolicy::fogged(),
        2,
        SEGMENT_MS,
        3,
        Some(library_folder()),
    );
    let token = support::seat_token(&mut surface, 0);
    let mut left = FIRST_LULL_MS;
    let deepest = nested(DEEPEST_NOTS);
    let past = nested(DEEPEST_NOTS + 1);
    for (name, text) in [("deepest", &deepest), ("past", &past)] {
        let playbook = quote(text);
        for depth in ["QUICK", "FULL"] {
            let report = answered(
                &call_in_lull(
                    &mut surface,
                    &token,
                    &mut left,
                    "verify_plan",
                    &format!(r#"{{"playbook_jsonc":{playbook},"depth":"{depth}"}}"#),
                ),
                &format!("verify_plan {depth}, {name}"),
            );
            let qualifies = report
                .get("report")
                .and_then(|report| report.get("qualifies"))
                .cloned();
            if name == "past" {
                assert_ne!(qualifies, Some(Json::Bool(true)), "{depth}: past the bound");
            }
        }
        for method in ["render_plan", "resolve_refs", "save_draft", "submit_plan"] {
            let params = if method == "save_draft" {
                format!(r#"{{"playbook_jsonc":{playbook},"label":"deep","draft_id":"deep"}}"#)
            } else {
                format!(r#"{{"playbook_jsonc":{playbook}}}"#)
            };
            let _ = answered(
                &call_in_lull(&mut surface, &token, &mut left, method, &params),
                &format!("{method}, {name}"),
            );
        }
        // A patch at the deepest value, and its inverse back.
        let patch = format!(
            r#"[{{"op":"replace","path":"{}","value":2}}]"#,
            deepest_pointer(DEEPEST_NOTS)
        );
        let patched = answered(
            &call_in_lull(
                &mut surface,
                &token,
                &mut left,
                "patch_plan",
                &format!(
                    r#"{{"playbook_jsonc":{playbook},"json_patch":{}}}"#,
                    quote(&patch)
                ),
            ),
            &format!("patch_plan, {name}"),
        );
        if let (Some(Json::String(text)), Some(Json::String(inverse))) = (
            patched.get("playbook_jsonc"),
            patched.get("inverse_json_patch"),
        ) {
            let _ = answered(
                &call_in_lull(
                    &mut surface,
                    &token,
                    &mut left,
                    "patch_plan",
                    &format!(
                        r#"{{"playbook_jsonc":{},"json_patch":{}}}"#,
                        quote(text),
                        quote(inverse)
                    ),
                ),
                &format!("patch_plan's inverse, {name}"),
            );
        } else {
            assert_eq!(name, "past", "the deepest text patches: {patched:?}");
        }
    }
    deep_values(&mut surface, &token, &mut left);
}

/// A patch value and a template parameter that nest the whole document to
/// the bound, and one past it.
fn deep_values(surface: &mut Surface, token: &Token, left: &mut i32) {
    let shallow = quote(MINIMAL);
    for levels in [MAX_DEPTH - 4, MAX_DEPTH - 3] {
        let patch = format!(
            r#"[{{"op":"replace","path":"/declarative/route/0/hold/ms","value":{}}}]"#,
            arrays(levels)
        );
        let _ = answered(
            &call_in_lull(
                surface,
                token,
                left,
                "patch_plan",
                &format!(
                    r#"{{"playbook_jsonc":{shallow},"json_patch":{}}}"#,
                    quote(&patch)
                ),
            ),
            &format!("patch_plan, a value of {levels} arrays"),
        );
        // A template parameter of the same depth: the hold's milliseconds sit
        // under five containers, so the parameter's arrays bring the file to
        // the bound and one past it.
        let _ = answered(
            &call_in_lull(
                surface,
                token,
                left,
                "instantiate_template",
                &format!(
                    r#"{{"template_id":"hold_and_build","parameters":[{{"name":"/declarative/route/1/hold/ms","value":{}}}]}}"#,
                    quote(&arrays(levels.saturating_sub(1)))
                ),
            ),
            &format!("instantiate_template, a value of {levels} arrays"),
        );
    }
}

#[test]
fn every_planning_method_reads_the_deepest_text_on_half_the_stated_stack() {
    std::thread::Builder::new()
        .name("half the stated stack".to_owned())
        .stack_size(PLAYER_TEXT_STACK >> 1)
        .spawn(every_planning_method)
        .expect("a thread")
        .join()
        .expect("every planning method answers on half the stated stack");
}
