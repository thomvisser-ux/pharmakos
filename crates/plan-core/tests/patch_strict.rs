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
        assert_eq!(
            refused.malformation(),
            Some(Malformed::UnknownMember),
            "{patch}"
        );
    }
}

/// An undo is the inverse of one patch, whole: a patch holding forward
/// operations and undo operations together is neither.
#[test]
fn forward_and_undo_operations_in_one_patch_are_refused() {
    let patch = r#"[{"op":"remove","path":"/a","undo":{}},{"op":"remove","path":"/b"}]"#;
    let refused = Patch::from_text(patch).expect_err("mixed");
    assert_eq!(refused.malformation(), Some(Malformed::MixedUndo));
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
            .is_none_or(|error| error.bound() != Some(Bound::PointerTokens)),
        "{MAX_POINTER_TOKENS} tokens are a pointer"
    );
    let past = format!(r#"[{{"op":"remove","path":"{longest}/a"}}]"#);
    let refused = Patch::from_text(&past).expect_err("one token past");
    assert_eq!(refused.bound(), Some(Bound::PointerTokens));
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
    assert_eq!(refused.bound(), Some(Bound::Operations));
    let refused = instantiate(template, &[], &parameters).expect_err("too many suggested");
    assert_eq!(refused.bound(), Some(Bound::Operations));
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

/// The one document the forged undos below are applied to.
const FORGE_BASE: &str = "{\"meta\":{\"author_kind\":\"HUMAN\"},\"b\":[1,2]}";

/// A forged undo's refusal: its `Malformed` code, its pointer and its words.
fn forged(undo: &str) -> pharmakos_plan_core::Error {
    pharmakos_plan_core::patch::patch_text(FORGE_BASE, undo).expect_err(undo)
}

/// An undo's `jsonc` is exactly one member or element: a line comment that
/// would swallow the wrapper's closing bracket is refused, not read as an
/// epilogue (fix pass 3, item L1).
#[test]
fn an_undo_whose_text_runs_past_its_member_is_refused() {
    for undo in [
        r#"[{"op":"add","path":"/meta/x","undo":{"at":0,"jsonc":"\"x\":1} // tail"}}]"#,
        r#"[{"op":"add","path":"/meta/x","undo":{"at":0,"jsonc":"\"x\":1} /* tail */"}}]"#,
        r#"[{"op":"add","path":"/b/0","undo":{"at":0,"jsonc":"9] // tail"}}]"#,
    ] {
        assert_eq!(
            forged(undo).malformation(),
            Some(Malformed::UndoText),
            "{undo}"
        );
    }
}

/// An array undo puts its element back at the slot its path names: a path
/// and an `at` that disagree are refused (fix pass 3, item L2).
#[test]
fn an_array_undo_whose_path_and_slot_disagree_is_refused() {
    let undo = r#"[{"op":"add","path":"/b/0","undo":{"at":1,"jsonc":"9"}}]"#;
    assert_eq!(forged(undo).malformation(), Some(Malformed::UndoSlot));
}

/// Every undo refusal carries a `Malformed` code and points at its operation
/// in the patch, `/<index>`, the space the forward patch's own refusals use;
/// no sentence ends with an empty list (fix pass 3, item L5).
#[test]
fn every_undo_refusal_is_coded_and_points_at_its_operation() {
    for (undo, code) in [
        (
            r#"[{"op":"remove","path":"/meta","undo":{"x":1}}]"#,
            Malformed::UnknownMember,
        ),
        (
            r#"[{"op":"remove","path":"/meta/nothing","undo":{}}]"#,
            Malformed::UndoTarget,
        ),
        (
            r#"[{"op":"replace","path":"/nowhere/x","undo":{"jsonc":"1"}}]"#,
            Malformed::UndoTarget,
        ),
        (
            r#"[{"op":"add","path":"/meta/author_kind","undo":{"at":0,"jsonc":"\"author_kind\":\"AI\""}}]"#,
            Malformed::UndoTarget,
        ),
        (
            r#"[{"op":"add","path":"/meta/x","undo":{"at":7,"jsonc":"\"x\":1"}}]"#,
            Malformed::UndoSlot,
        ),
    ] {
        let refused = forged(undo);
        assert_eq!(
            refused.malformation(),
            Some(code),
            "{undo}: {}",
            refused.message()
        );
        assert_eq!(refused.pointer(), "/0", "{undo}: {}", refused.message());
        assert!(
            !refused.message().contains("it has ``")
                && !refused.message().trim_end().ends_with("it has"),
            "{undo}: {}",
            refused.message()
        );
    }
}

/// An undo that would take the document past the cap is refused, as a
/// forward patch is (fix pass 3, item L11: the review's `undo_unbudgeted`).
#[test]
fn an_undo_past_the_cap_is_refused() {
    let huge = format!(
        "\"x\":\"{}\"",
        "y".repeat(pharmakos_verifier::MAX_PLAYBOOK_CHARS)
    );
    let undo = format!(
        r#"[{{"op":"add","path":"/meta/x","undo":{{"at":0,"jsonc":{}}}}}]"#,
        pharmakos_proto::json::write(&pharmakos_proto::json::Json::String(huge))
    );
    assert_eq!(forged(&undo).bound(), Some(Bound::Size));
}

/// A member put at a slot past the end of its object is refused, never put at
/// the end (fix pass 3, item L10: it was clamped).
#[test]
fn a_member_put_past_the_end_is_refused() {
    use pharmakos_plan_core::jsonc::{Document, Fragment};
    let mut document = Document::parse("{\"a\":1}").expect("a document");
    let fragment = Fragment::member_from_written("\"b\":2", "b").expect("a member");
    assert!(document.root_mut().put_member("b", fragment, 2).is_err());
    assert_eq!(document.to_text(), "{\"a\":1}");
}

/// An array index is spelled as RFC 6901 spells one, `0|[1-9][0-9]*`, and
/// nothing else is read as one (fix pass 4, item L-d): `+0`, `+1` and `01`
/// are refused, typed, in a forward patch and in an undo's path.
#[test]
fn an_array_index_is_spelled_as_rfc_6901_spells_one() {
    for token in ["+0", "+1", "01", "1_0", " 1"] {
        let patch = format!(r#"[{{"op":"remove","path":"/b/{token}"}}]"#);
        let refused = forged(&patch);
        assert_eq!(
            refused.malformation(),
            Some(Malformed::PointerIndex),
            "{token}: {}",
            refused.message()
        );
    }
    let undo = r#"[{"op":"add","path":"/b/+1","undo":{"at":1,"jsonc":"9"}}]"#;
    assert_eq!(forged(undo).malformation(), Some(Malformed::PointerIndex));
}

/// One typed index reader on every path (fix pass 5, item L1): `replace`,
/// `test`, a `move`'s `from`, `add`, `remove` and every undo answer
/// `Malformed::PointerIndex` for a token RFC 6901 does not spell as an index;
/// a well-formed index past the end, or past any `usize`, answers "past the
/// end", with a sentence true of that token.
#[test]
fn every_path_reads_an_array_index_one_way() {
    let shapes = [
        r#"[{"op":"replace","path":"/b/TOKEN","value":9}]"#,
        r#"[{"op":"test","path":"/b/TOKEN","value":1}]"#,
        r#"[{"op":"move","from":"/b/TOKEN","path":"/meta/x"}]"#,
        r#"[{"op":"copy","from":"/b/TOKEN","path":"/meta/x"}]"#,
        r#"[{"op":"add","path":"/b/TOKEN","value":9}]"#,
        r#"[{"op":"remove","path":"/b/TOKEN"}]"#,
        r#"[{"op":"remove","path":"/b/TOKEN","undo":{}}]"#,
        r#"[{"op":"replace","path":"/b/TOKEN","undo":{"jsonc":"9"}}]"#,
        r#"[{"op":"add","path":"/b/TOKEN","undo":{"at":1,"jsonc":"9"}}]"#,
    ];
    for shape in shapes {
        for token in ["+1", "01", "-1", "", "1e0", " 1"] {
            let patch = shape.replace("TOKEN", token);
            let refused = forged(&patch);
            assert_eq!(
                refused.malformation(),
                Some(Malformed::PointerIndex),
                "{patch}: {}",
                refused.message()
            );
        }
        // Past the end: the array holds two; and past any `usize`. A put
        // undo whose path and slot disagree is `UndoSlot`'s, not this.
        if shape.contains(r#""at":1"#) {
            continue;
        }
        for token in ["3", "123456789012345678901234567890"] {
            let patch = shape.replace("TOKEN", token);
            let refused = forged(&patch);
            assert_ne!(
                refused.malformation(),
                Some(Malformed::PointerIndex),
                "{patch}"
            );
            assert!(
                refused.message().contains("past the end"),
                "{patch}: {}",
                refused.message()
            );
            assert!(
                refused.message().contains(token),
                "the sentence names the token: {patch}: {}",
                refused.message()
            );
        }
    }
}

/// An undo's text past the nesting limit is refused as a depth refusal, not
/// as an undo text that is not one value (fix pass 4, item L-e).
#[test]
fn an_undo_too_deep_is_a_depth_refusal() {
    let deep = format!("{}1{}", "[".repeat(80), "]".repeat(80));
    let undo = format!(
        r#"[{{"op":"replace","path":"/meta/author_kind","undo":{{"jsonc":{}}}}}]"#,
        pharmakos_proto::json::write(&pharmakos_proto::json::Json::String(deep))
    );
    let refused = forged(&undo);
    assert_eq!(refused.bound(), Some(Bound::Depth), "{}", refused.message());
}
