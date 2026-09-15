// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The property the editor's undo stack rests on: **patch then inverse-patch
//! is the identity**, on every fixture, at every pointer, for every operation.
//!
//! Spec section 13 asks for two things at once — "Edits are JSON Patches with
//! inverse-patch undo" and "comments survive a load-and-save unchanged" — and
//! it is the *combination* that is easy to get wrong. A patch layer that works
//! on values alone passes an undo test on the JSON and still loses the comment
//! above the member it removed. So the identity asserted here is over the
//! **bytes of the file**, not over its decoded value.
//!
//! The generated patch set is exhaustive rather than random: every pointer in
//! each fixture, crossed with every operation that is legal at it. There is no
//! seed and no sampling, so a failure is reproducible by its name.

use std::fs;
use std::path::{Path, PathBuf};

use pharmakos_plan_core::error::push_pointer;
use pharmakos_plan_core::jsonc::{Document, Fragment};
use pharmakos_plan_core::patch::{Op, Operation, Patch, apply};
use pharmakos_proto::json::Json;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| panic!("crates/plan-core sits two levels below the workspace root"))
        .to_path_buf()
}

fn fixtures() -> Vec<(&'static str, String)> {
    let example = workspace_root()
        .join("examples")
        .join("playbooks")
        .join("expand_east.jsonc");
    let awkward = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("cases")
        .join("awkward.jsonc");
    vec![("expand_east", read(&example)), ("awkward", read(&awkward))]
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

/// Every pointer in a value, the root included, in document order.
fn pointers(value: &Json, at: &str, found: &mut Vec<String>) {
    found.push(at.to_owned());
    match value {
        Json::Object(entries) => {
            for (key, item) in entries {
                pointers(item, &push_pointer(at, key), found);
            }
        }
        Json::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                pointers(item, &push_pointer(at, &index.to_string()), found);
            }
        }
        _ => {}
    }
}

fn all_pointers(document: &Document) -> Vec<String> {
    let mut found = Vec::new();
    pointers(&document.to_json(), "", &mut found);
    found
}

/// A value that is never equal to anything already in a fixture, so a
/// `replace` always changes something.
fn sentinel() -> Fragment {
    Fragment::from_json(&Json::String("\u{2620} replaced".to_owned()))
}

/// Applies one patch and its inverse, and asserts the file came back.
///
/// Returns whether the patch actually applied. Every property test below
/// counts those and asserts a floor, because a `return` on a refusal would
/// otherwise let the whole file pass green if `apply` regressed to refusing
/// everything: the property would be asserted zero times and say `ok`.
#[must_use]
fn round_trip(name: &str, text: &str, what: &str, patch: &Patch) -> bool {
    let document = Document::parse(text).unwrap_or_else(|error| panic!("{name}: {error}"));
    let Ok((patched, inverse)) = apply(&document, patch) else {
        // An operation the document's shape does not allow is not a failure of
        // the property: it is the patch layer refusing, which is what it is
        // for. What must never happen is applying and then failing to undo.
        return false;
    };
    let (back, _again) = apply(&patched, &inverse)
        .unwrap_or_else(|error| panic!("{name}: {what}: the inverse did not apply: {error}"));
    assert_eq!(
        back.to_text(),
        text,
        "{name}: {what}: undo was not byte-exact"
    );
    true
}

/// The floor a property test asserts. Each fixture has more than twenty
/// pointers, so most of the tests below clear twenty comfortably; the
/// two-operation test filters its pairs hard and carries its own smaller
/// number. The point of the floor is not its size but that it is above zero:
/// a run that applied nothing would otherwise report `ok`.
fn assert_applied(name: &str, what: &str, applied: usize, floor: usize) {
    assert!(
        applied >= floor,
        "{name}: only {applied} {what} round trips applied; the property is being asserted \
         vacuously"
    );
}

#[test]
fn every_fixture_parses_and_has_pointers_to_patch() {
    for (name, text) in fixtures() {
        let document = Document::parse(&text).unwrap_or_else(|error| panic!("{name}: {error}"));
        let found = all_pointers(&document);
        assert!(
            found.len() > 20,
            "{name}: only {} pointers; the walk is looking in the wrong place and the property \
             tests below would pass vacuously",
            found.len()
        );
    }
}

#[test]
fn replace_then_undo_is_the_identity_at_every_pointer() {
    for (name, text) in fixtures() {
        let document = Document::parse(&text).unwrap_or_else(|error| panic!("{name}: {error}"));
        let mut applied = 0;
        for pointer in all_pointers(&document) {
            if pointer.is_empty() {
                continue;
            }
            let patch = Patch::new(vec![Operation::with_value(
                Op::Replace,
                pointer.clone(),
                sentinel(),
            )]);
            applied += usize::from(round_trip(
                name,
                &text,
                &format!("replace {pointer}"),
                &patch,
            ));
        }
        assert_applied(name, "replace", applied, 20);
    }
}

#[test]
fn remove_then_undo_is_the_identity_at_every_pointer() {
    for (name, text) in fixtures() {
        let document = Document::parse(&text).unwrap_or_else(|error| panic!("{name}: {error}"));
        let mut applied = 0;
        for pointer in all_pointers(&document) {
            if pointer.is_empty() {
                continue;
            }
            let patch = Patch::new(vec![Operation::plain(Op::Remove, pointer.clone())]);
            applied += usize::from(round_trip(
                name,
                &text,
                &format!("remove {pointer}"),
                &patch,
            ));
        }
        assert_applied(name, "remove", applied, 20);
    }
}

#[test]
fn add_then_undo_is_the_identity_under_every_container() {
    for (name, text) in fixtures() {
        let document = Document::parse(&text).unwrap_or_else(|error| panic!("{name}: {error}"));
        let mut applied = 0;
        for pointer in all_pointers(&document) {
            for token in ["added", "0", "-"] {
                let patch = Patch::new(vec![Operation::with_value(
                    Op::Add,
                    push_pointer(&pointer, token),
                    sentinel(),
                )]);
                applied += usize::from(round_trip(
                    name,
                    &text,
                    &format!("add {pointer}/{token}"),
                    &patch,
                ));
            }
            // RFC 6902 section 4.1: `add` at a pointer that already names
            // something is a **replace**, and it is the branch the three
            // tokens above can never reach, because none of them names an
            // existing member. It is also the branch an ordinary editor field
            // edit takes, and the one where taking the member out and putting
            // a wire fragment back would drop the comment above it.
            if !pointer.is_empty() {
                let patch = Patch::new(vec![Operation::with_value(
                    Op::Add,
                    pointer.clone(),
                    sentinel(),
                )]);
                applied += usize::from(round_trip(
                    name,
                    &text,
                    &format!("add over the existing {pointer}"),
                    &patch,
                ));
            }
        }
        assert_applied(name, "add", applied, 20);
    }
}

#[test]
fn move_and_copy_then_undo_are_the_identity() {
    for (name, text) in fixtures() {
        let document = Document::parse(&text).unwrap_or_else(|error| panic!("{name}: {error}"));
        let mut applied = 0;
        for pointer in all_pointers(&document) {
            if pointer.is_empty() {
                continue;
            }
            // `/moved` is a slot that does not exist, `/kind` is a top-level
            // member both fixtures have. The second target is the one that
            // goes through `add`'s replace branch, so a `move` onto a member
            // that is already there has to give the file back as it was too.
            for target in ["/moved", "/kind"] {
                if pointer == target || pointer.starts_with(&format!("{target}/")) {
                    continue;
                }
                for op in [Op::Move, Op::Copy] {
                    let patch = Patch::new(vec![Operation {
                        op,
                        path: target.to_owned(),
                        from: Some(pointer.clone()),
                        value: None,
                        position: None,
                    }]);
                    applied += usize::from(round_trip(
                        name,
                        &text,
                        &format!("{} {pointer} to {target}", op.name()),
                        &patch,
                    ));
                }
            }
        }
        assert_applied(name, "move and copy", applied, 20);
    }
}

#[test]
fn a_multi_operation_patch_undoes_in_reverse_order() {
    for (name, text) in fixtures() {
        let document = Document::parse(&text).unwrap_or_else(|error| panic!("{name}: {error}"));
        let pointers = all_pointers(&document);
        let mut applied = 0;
        // Two edits at once, the second at a pointer the first did not touch.
        for pair in pointers.windows(2) {
            let (Some(first), Some(second)) = (pair.first(), pair.get(1)) else {
                continue;
            };
            if first.is_empty() || second.is_empty() || second.starts_with(first.as_str()) {
                continue;
            }
            let patch = Patch::new(vec![
                Operation::with_value(Op::Replace, first.clone(), sentinel()),
                Operation::plain(Op::Remove, second.clone()),
            ]);
            applied += usize::from(round_trip(
                name,
                &text,
                &format!("replace {first}, remove {second}"),
                &patch,
            ));
        }
        assert_applied(name, "two-operation", applied, 8);
    }
}

#[test]
fn a_test_operation_changes_nothing_and_undoes_to_nothing() {
    for (name, text) in fixtures() {
        let document = Document::parse(&text).unwrap_or_else(|error| panic!("{name}: {error}"));
        let value = document.to_json();
        let patch = Patch::new(vec![Operation::with_value(
            Op::Test,
            "",
            Fragment::from_json(&value),
        )]);
        let (patched, inverse) = apply(&document, &patch)
            .unwrap_or_else(|error| panic!("{name}: a test against the whole document: {error}"));
        assert_eq!(patched.to_text(), text);
        assert!(inverse.is_empty(), "{name}: a test has nothing to undo");
    }
}

#[test]
fn the_wire_form_survives_a_round_trip_through_its_own_text() {
    for (name, text) in fixtures() {
        let document = Document::parse(&text).unwrap_or_else(|error| panic!("{name}: {error}"));
        for pointer in all_pointers(&document).into_iter().take(12) {
            if pointer.is_empty() {
                continue;
            }
            let patch = Patch::new(vec![Operation::with_value(
                Op::Replace,
                pointer.clone(),
                sentinel(),
            )]);
            let again = Patch::from_text(&patch.to_text())
                .unwrap_or_else(|error| panic!("{name}: {pointer}: {error}"));
            assert_eq!(patch, again, "{name}: {pointer}");
        }
    }
}

/// The editor's ordinary field edit, spelled the way RFC 6902 lets a client
/// spell it. `add` at a member that is already there is a replace (RFC 6902
/// section 4.1), and `gp.api.v1.PatchPlanResponse` takes arbitrary RFC 6902,
/// so this is the spelling the crate has to survive as well as `replace`.
#[test]
fn add_over_an_existing_member_keeps_its_comment_and_its_spacing() {
    let text = "{\n  // why a is 1\n  \"a\": 1,\n  \"b\": 2\n}\n";
    let document = Document::parse(text).expect("a document");
    for wire in [
        r#"[{"op":"add","path":"/a","value":9}]"#,
        r#"[{"op":"move","from":"/b","path":"/a"}]"#,
        r#"[{"op":"copy","from":"/b","path":"/a"}]"#,
    ] {
        let patch = Patch::from_text(wire).unwrap_or_else(|error| panic!("{wire}: {error}"));
        let (patched, inverse) =
            apply(&document, &patch).unwrap_or_else(|error| panic!("{wire}: {error}"));
        assert!(
            patched.to_text().contains("// why a is 1"),
            "{wire}: the comment above the member went with the edit: {}",
            patched.to_text()
        );
        let (back, _again) =
            apply(&patched, &inverse).unwrap_or_else(|error| panic!("{wire}: undo: {error}"));
        assert_eq!(back.to_text(), text, "{wire}: undo was not byte-exact");
    }
}

/// The one place the wire form is lossy, asserted rather than left to be
/// discovered: an inverse patch serialised to RFC 6902 text carries a value,
/// and a value has no comments.
#[test]
fn the_wire_form_of_an_inverse_loses_the_trivia_it_carried() {
    let text = "{\n  // why\n  \"a\": 1,\n  \"b\": 2\n}";
    let document = Document::parse(text).expect("a document");
    let patch = Patch::from_text(r#"[{"op":"remove","path":"/a"}]"#).expect("a patch");
    let (patched, inverse) = apply(&document, &patch).expect("it applies");

    // In memory: byte-exact.
    let (back, _again) = apply(&patched, &inverse).expect("the inverse applies");
    assert_eq!(back.to_text(), text);

    // Through the wire: the value comes back, the comment does not.
    let on_the_wire = Patch::from_text(&inverse.to_text()).expect("its own text");
    let (flattened, _again) = apply(&patched, &on_the_wire).expect("the wire inverse applies");
    assert_ne!(flattened.to_text(), text);
    assert!(!flattened.to_text().contains("// why"));
    // The *value* is intact; only the trivia and the member's slot are gone,
    // and a member's slot is not semantic JSON — which is exactly why RFC 6902
    // does not carry one and why an in-memory inverse does.
    let mut restored = match flattened.to_json() {
        Json::Object(entries) => entries,
        other => panic!("the patched document is still an object, not {other:?}"),
    };
    restored.sort_by(|left, right| left.0.cmp(&right.0));
    assert_eq!(
        restored,
        vec![
            ("a".to_owned(), Json::Number("1".to_owned())),
            ("b".to_owned(), Json::Number("2".to_owned())),
        ]
    );
}
