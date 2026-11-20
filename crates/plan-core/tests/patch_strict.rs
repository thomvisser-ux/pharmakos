// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The patch reader's refusals, each with its typed answer (S3's `vfyq2`, fix
//! pass 2, items A1 and A2): a member an operation does not have, forward and
//! undo operations in one patch, a pointer of more tokens than any document
//! can hold, more template parameters than one patch may carry, and a `move`
//! onto itself, which changes nothing.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml's allow-expect-in-tests only recognises #[test] functions, not an \
              integration test's helpers. A panic is this file's failure report."
)]

use pharmakos_plan_core::error::{Bound, MAX_POINTER_TOKENS, Malformed};
use pharmakos_plan_core::jsonc::Document;
use pharmakos_plan_core::library::{Parameter, instantiate};
use pharmakos_plan_core::patch::{MAX_PATCH_OPERATIONS, Patch, apply};

/// RFC 6902 would ignore a member it does not know; this reader refuses it,
/// so a misspelt `from` or `value` is never silently a different operation.
#[test]
fn a_member_an_operation_does_not_have_is_refused() {
    for patch in [
        r#"[{"op":"remove","path":"/a","value":1}]"#,
        r#"[{"op":"add","path":"/a","value":1,"form":"/b"}]"#,
        r#"[{"op":"remove","path":"/a","undo":{"at":0}}]"#,
    ] {
        let refused = Patch::from_text(patch).expect_err(patch);
        assert_eq!(refused.malformed, Some(Malformed::UnknownMember), "{patch}");
    }
}

/// An undo is the inverse of one patch, whole: a patch holding forward
/// operations and undo operations together is neither.
#[test]
fn forward_and_undo_operations_in_one_patch_are_refused() {
    let patch = r#"[{"op":"remove","path":"/a","undo":{}},{"op":"remove","path":"/b"}]"#;
    let refused = Patch::from_text(patch).expect_err("mixed");
    assert_eq!(refused.malformed, Some(Malformed::MixedUndo));
}

/// No document holds a value under more than `MAX_DEPTH` containers, so a
/// pointer of more tokens than that names nothing, and is refused before it
/// is followed.
#[test]
fn a_pointer_of_more_tokens_than_any_document_holds_is_refused() {
    let longest = "/a".repeat(MAX_POINTER_TOKENS);
    let patch = format!(r#"[{{"op":"remove","path":"{longest}"}}]"#);
    let read = Patch::from_text(&patch);
    assert!(
        read.as_ref()
            .err()
            .is_none_or(|error| error.bound != Some(Bound::PointerTokens)),
        "{MAX_POINTER_TOKENS} tokens are a pointer"
    );
    let past = format!(r#"[{{"op":"remove","path":"{longest}/a"}}]"#);
    let refused = Patch::from_text(&past).expect_err("one token past");
    assert_eq!(refused.bound, Some(Bound::PointerTokens));
}

/// Each template parameter becomes at most one operation of one patch, so a
/// list longer than a patch may be is refused before any name is read.
#[test]
fn more_parameters_than_a_patch_may_carry_are_refused() {
    let template = r#"{"kind":"TEMPLATE","meta":{"title":"t"}}"#;
    let parameters: Vec<Parameter> = (0..=MAX_PATCH_OPERATIONS)
        .map(|index| Parameter {
            name: format!("/meta/p{index}"),
            value: String::from("1"),
        })
        .collect();
    let refused = instantiate(template, &parameters, &[]).expect_err("too many");
    assert_eq!(refused.bound, Some(Bound::Operations));
    let refused = instantiate(template, &[], &parameters).expect_err("too many suggested");
    assert_eq!(refused.bound, Some(Bound::Operations));
}

/// RFC 6902 section 4.4: a `move` whose `from` is its `path` changes nothing.
/// It used to take the member out and put it back at the end of its object.
#[test]
fn a_move_onto_itself_changes_nothing() {
    let text = "{\n  // first\n  \"a\": 1,\n  \"b\": 2\n}";
    let document = Document::parse(text).expect("a document");
    let patch = Patch::from_text(r#"[{"op":"move","from":"/a","path":"/a"}]"#).expect("a patch");
    let (moved, inverse) = apply(&document, &patch).expect("it applies");
    assert_eq!(moved.to_text(), text);
    let (back, _) = apply(&moved, &inverse).expect("its inverse applies");
    assert_eq!(back.to_text(), text);
}
