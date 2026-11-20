// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A patch cannot grow a document without bound (S3's `vfyq2`, fix pass 1,
//! item 2).
//!
//! `patch_plan` takes a JSON Patch from a socket. RFC 6902's `copy` copies a
//! subtree, the root included, so each `copy` of the root into a member
//! doubles the document: the security review of PR #198 measured 66 MB from
//! 18 operations (675 bytes of patch) and an aborted process from about 40.
//! The budget is an invariant of `apply`: at most
//! [`MAX_PATCH_OPERATIONS`] operations; the document, after each operation,
//! within the playbook's cap in characters and the grammar's nesting limit;
//! a `copy` refused before it copies what would pass the cap; and the inverse
//! patch within the same cap. Each refusal is typed by its [`Bound`].

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::integer_division,
    reason = "a test: a panic is its failure report, and its arithmetic is on small counts it \
              made itself"
)]

use pharmakos_plan_core::error::Bound;

use pharmakos_plan_core::patch::{MAX_PATCH_OPERATIONS, patch_text};
use pharmakos_proto::json::grammar::MAX_DEPTH;
use pharmakos_verifier::MAX_PLAYBOOK_CHARS;

const MINIMAL: &str = concat!(
    "{\"schema_version\":{\"major\":1},",
    "\"meta\":{\"title\":\"t\",\"author_kind\":\"HUMAN\"},",
    "\"declarative\":{\"route\":[{\"label\":\"a\",\"hold\":{\"ms\":1000}}]},",
    "\"on_death\":{\"on_respawn\":\"CONTINUE\"},",
    "\"fallback\":{\"hold\":{\"at\":{\"beacon_anchor\":{\"safest\":{}}}}},",
    "\"kind\":\"PLAYBOOK\"}"
);

fn bound_of(text: &str, patch: &str) -> Option<Bound> {
    match patch_text(text, patch) {
        Ok(_) => None,
        Err(error) => error.bound,
    }
}

#[test]
fn copying_the_root_into_itself_is_refused_before_it_passes_the_cap() {
    let operations: Vec<String> = (0..18)
        .map(|index| format!(r#"{{"op":"copy","from":"","path":"/x{index}"}}"#))
        .collect();
    let patch = format!("[{}]", operations.join(","));
    assert_eq!(bound_of(MINIMAL, &patch), Some(Bound::Size));
}

#[test]
fn more_operations_than_the_cap_are_refused_before_any_is_applied() {
    let operations: Vec<String> = (0..=MAX_PATCH_OPERATIONS)
        .map(|_| r#"{"op":"test","path":"/kind","value":"PLAYBOOK"}"#.to_owned())
        .collect();
    let patch = format!("[{}]", operations.join(","));
    assert_eq!(bound_of(MINIMAL, &patch), Some(Bound::Operations));
    let at_the_cap = format!("[{}]", operations[1..].join(","));
    assert_eq!(bound_of(MINIMAL, &at_the_cap), None);
}

#[test]
fn a_value_nested_past_the_grammars_limit_is_refused() {
    // `/meta/title` stands inside one container, so a value there whose own
    // numbers stand inside `MAX_DEPTH` arrays puts them inside one more.
    let deep = format!("{}1{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
    let patch = format!(
        r#"[{{"op":"add","path":"/meta/title","value":{}}}]"#,
        &deep[1..deep.len() - 1]
    );
    assert_eq!(
        bound_of(MINIMAL, &patch),
        None,
        "inside {MAX_DEPTH} containers"
    );
    let patch = format!(r#"[{{"op":"add","path":"/meta/title","value":{deep}}}]"#);
    assert_eq!(bound_of(MINIMAL, &patch), Some(Bound::Depth));
}

#[test]
fn an_inverse_past_the_cap_is_refused() {
    let big = "x".repeat(MAX_PLAYBOOK_CHARS / 2);
    let text = MINIMAL.replace("\"title\":\"t\"", &format!("\"title\":\"{big}\""));
    let swap = format!(
        r#"[{{"op":"replace","path":"/meta/title","value":"s"}},{{"op":"replace","path":"/meta/title","value":"{big}"}},{{"op":"replace","path":"/meta/title","value":"s"}}]"#
    );
    assert_eq!(bound_of(&text, &swap), Some(Bound::Inverse));
}
