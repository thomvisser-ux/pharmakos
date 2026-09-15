// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! RFC 6902 JSON Patch over a playbook file, with the inverse patch that
//! undoes it.
//!
//! Spec section 13: "Edits are JSON Patches with inverse-patch undo, and
//! comments survive a load-and-save unchanged." So this module has two jobs at
//! once — apply the edit, and hand back something the editor's undo stack can
//! apply to get the *file* back, not merely the value.
//!
//! # Why an inverse patch carries a fragment and the wire form does not
//!
//! RFC 6902's `add` and `replace` carry a **value**, and a value has no
//! comments and no spacing. An inverse patch built out of plain values would
//! therefore undo a `remove` by putting the member back without the comment
//! that sat above it — a silently dropped comment, which
//! `tests/golden/plan-core/README.md` calls a bug even when the JSON is
//! identical.
//!
//! So an [`Operation`] carries a [`Fragment`]: the node **and** the trivia it
//! was cut out with. [`apply`] returns an inverse patch of fragments, and
//! `undo_is_byte_exact_on_every_fixture` in `tests/patch.rs` asserts that
//! applying it gives the original bytes back. [`Patch::to_text`] flattens the
//! fragments to plain values because that is what RFC 6902 can say, and
//! `gp.api.v1.PatchPlanResponse.inverse_json_patch` is that text — the loss is
//! the wire format's, and it is stated here rather than discovered later.
//!
//! # What is supported
//!
//! All six operations: `add`, `remove`, `replace`, `move`, `copy` and `test`.
//! `move` and `copy` are expanded into the pair they are defined as, which is
//! also how their inverse is expressed.
//!
//! The patch is applied to the **document**, not to the schema. A patch that
//! produces something that is no longer a playbook is caught by the verifier,
//! with a code and a pointer, exactly as a hand-edit would be — the editor
//! runs no validation of its own (spec section 12).

use pharmakos_proto::json::{Json, read, write};

use crate::error::{Error, push_pointer, split_pointer};
use crate::jsonc::{Document, Fragment, Node};

/// The six RFC 6902 operations.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    /// Insert a member or element, or replace a member that is already there.
    Add,
    /// Take a member or element out.
    Remove,
    /// Swap the value at a path that must already exist.
    Replace,
    /// Remove from `from` and add at `path`.
    Move,
    /// Add a copy of `from` at `path`.
    Copy,
    /// Assert a value, changing nothing.
    Test,
}

impl Op {
    /// The RFC 6902 spelling.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Remove => "remove",
            Self::Replace => "replace",
            Self::Move => "move",
            Self::Copy => "copy",
            Self::Test => "test",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "add" => Some(Self::Add),
            "remove" => Some(Self::Remove),
            "replace" => Some(Self::Replace),
            "move" => Some(Self::Move),
            "copy" => Some(Self::Copy),
            "test" => Some(Self::Test),
            _ => None,
        }
    }
}

/// One patch operation.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Operation {
    /// Which operation.
    pub op: Op,
    /// The RFC 6901 pointer it acts on.
    pub path: String,
    /// The source pointer, for `move` and `copy`.
    pub from: Option<String>,
    /// The value, for `add`, `replace` and `test`. A fragment rather than a
    /// plain value so that an inverse patch can restore comments.
    pub value: Option<Fragment>,
    /// Which slot of an object an `add` should go into.
    ///
    /// RFC 6902 has no such field, and [`Patch::to_json`] does not write one:
    /// an object's member order is not semantic JSON. It is semantic *text*,
    /// though, and an undo has to give the file back exactly, so an inverse
    /// patch this crate produces remembers which slot the member came out of.
    /// A patch read off the wire never carries one and appends, as RFC 6902
    /// says.
    pub position: Option<usize>,
}

impl Operation {
    /// An operation with a value.
    #[must_use]
    pub fn with_value(op: Op, path: impl Into<String>, value: Fragment) -> Self {
        Self {
            op,
            path: path.into(),
            from: None,
            value: Some(value),
            position: None,
        }
    }

    /// The same operation, remembering which slot of an object it belongs in.
    #[must_use]
    pub const fn at(mut self, position: usize) -> Self {
        self.position = Some(position);
        self
    }

    /// An operation with no value: `remove`.
    #[must_use]
    pub fn plain(op: Op, path: impl Into<String>) -> Self {
        Self {
            op,
            path: path.into(),
            from: None,
            value: None,
            position: None,
        }
    }
}

/// A sequence of operations, applied in order.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Patch {
    operations: Vec<Operation>,
}

impl Patch {
    /// A patch from its operations.
    #[must_use]
    pub const fn new(operations: Vec<Operation>) -> Self {
        Self { operations }
    }

    /// The operations, in order.
    #[must_use]
    pub fn operations(&self) -> &[Operation] {
        &self.operations
    }

    /// True when there is nothing to do.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    /// Reads a patch from its RFC 6902 JSON text.
    ///
    /// # Errors
    ///
    /// When the text is not a JSON array of operation objects, when an `op` is
    /// not one of the six, or when a required member is missing.
    pub fn from_text(text: &str) -> Result<Self, Error> {
        Self::from_json(&read(text)?)
    }

    /// Reads a patch from an already-parsed value.
    ///
    /// # Errors
    ///
    /// As [`Patch::from_text`].
    pub fn from_json(value: &Json) -> Result<Self, Error> {
        let Json::Array(items) = value else {
            return Err(Error::at("", "a JSON Patch is an array of operations"));
        };
        let mut operations = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            let at = format!("/{index}");
            let name = match item.get("op") {
                Some(Json::String(name)) => name.clone(),
                _ => return Err(Error::at(at, "an operation needs a string `op`")),
            };
            let op = Op::from_name(&name)
                .ok_or_else(|| Error::at(&at, format!("`{name}` is not an RFC 6902 operation")))?;
            let path = match item.get("path") {
                Some(Json::String(path)) => path.clone(),
                _ => return Err(Error::at(at, "an operation needs a string `path`")),
            };
            let from = match item.get("from") {
                Some(Json::String(from)) => Some(from.clone()),
                Some(_) => return Err(Error::at(at, "`from` is a string")),
                None => None,
            };
            if matches!(op, Op::Move | Op::Copy) && from.is_none() {
                return Err(Error::at(at, "`move` and `copy` need a `from`"));
            }
            let value = item.get("value").map(Fragment::from_json);
            if matches!(op, Op::Add | Op::Replace | Op::Test) && value.is_none() {
                return Err(Error::at(at, "`add`, `replace` and `test` need a `value`"));
            }
            operations.push(Operation {
                op,
                path,
                from,
                value,
                position: None,
            });
        }
        Ok(Self { operations })
    }

    /// The patch as a JSON value: plain RFC 6902, so any fragment's trivia is
    /// flattened away.
    #[must_use]
    pub fn to_json(&self) -> Json {
        Json::Array(
            self.operations
                .iter()
                .map(|operation| {
                    let mut entries = vec![
                        (
                            "op".to_owned(),
                            Json::String(operation.op.name().to_owned()),
                        ),
                        ("path".to_owned(), Json::String(operation.path.clone())),
                    ];
                    if let Some(from) = operation.from.as_ref() {
                        entries.push(("from".to_owned(), Json::String(from.clone())));
                    }
                    if let Some(value) = operation.value.as_ref() {
                        entries.push(("value".to_owned(), value.to_json()));
                    }
                    Json::Object(entries)
                })
                .collect(),
        )
    }

    /// The patch as RFC 6902 text, which is what the gateway exchanges.
    ///
    /// Comments in a fragment do not survive this: see the module docs.
    #[must_use]
    pub fn to_text(&self) -> String {
        write(&self.to_json())
    }
}

/// Applies a patch to a document and returns the patch that undoes it.
///
/// The inverse is the reverse-ordered inverse of each operation, so applying
/// it puts the file back exactly — comments, spacing and member order
/// included.
///
/// # Errors
///
/// When a path does not resolve, when `test` does not hold, or when an
/// operation asks for something the document's shape cannot do.
pub fn apply(document: &Document, patch: &Patch) -> Result<(Document, Patch), Error> {
    let mut working = document.clone();
    let mut inverse: Vec<Operation> = Vec::new();
    for operation in patch.operations() {
        // The last operation is undone first, so each operation's undo goes in
        // front of everything accumulated so far.
        let mut next = one(&mut working, operation)?;
        next.append(&mut inverse);
        inverse = next;
    }
    Ok((working, Patch::new(inverse)))
}

/// Applies a patch given as RFC 6902 text and returns the patched file and the
/// inverse patch's text.
///
/// This is `patch_plan` (spec section 12): the file comes back with its
/// comments and formatting, and the inverse comes back as text for the undo
/// stack.
///
/// # Errors
///
/// As [`apply`], plus a syntax error in either text.
pub fn patch_text(playbook_jsonc: &str, json_patch: &str) -> Result<(String, String), Error> {
    let document = Document::parse(playbook_jsonc)?;
    let patch = Patch::from_text(json_patch)?;
    let (patched, inverse) = apply(&document, &patch)?;
    Ok((patched.to_text(), inverse.to_text()))
}

/// One operation. Returns the operations that undo it, **in the order they
/// must be applied**.
fn one(document: &mut Document, operation: &Operation) -> Result<Vec<Operation>, Error> {
    match operation.op {
        Op::Test => {
            let wanted = value_of(operation)?;
            let found = resolve(document, &operation.path)?;
            if found.to_json() == wanted.to_json() {
                Ok(Vec::new())
            } else {
                Err(Error::at(
                    &operation.path,
                    "the value here is not the one the patch tested for",
                ))
            }
        }
        Op::Remove => {
            let (taken, at) = take(document, &operation.path)?;
            Ok(vec![
                Operation::with_value(Op::Add, operation.path.clone(), taken).at(at),
            ])
        }
        Op::Add => add(
            document,
            &operation.path,
            value_of(operation)?,
            operation.position,
        ),
        Op::Replace => {
            let fragment = value_of(operation)?;
            let target = resolve_mut(document, &operation.path)?;
            let old = target.set(fragment.into_node());
            Ok(vec![Operation::with_value(
                Op::Replace,
                operation.path.clone(),
                Fragment::from_node(old),
            )])
        }
        Op::Move => {
            let from = from_of(operation)?.to_owned();
            let (taken, at) = take(document, &from)?;
            // The trivia goes with the member, so the undo has to carry the
            // same fragment rather than a fresh value: otherwise moving a
            // commented member and undoing it would lose the comment.
            let restore = taken.clone();
            let mut inverse = add(document, &operation.path, taken, operation.position)?;
            inverse.push(Operation::with_value(Op::Add, from, restore).at(at));
            Ok(inverse)
        }
        Op::Copy => {
            let from = from_of(operation)?;
            let copied = resolve(document, from)?.clone();
            add(
                document,
                &operation.path,
                Fragment::from_node(copied),
                operation.position,
            )
        }
    }
}

fn value_of(operation: &Operation) -> Result<Fragment, Error> {
    operation
        .value
        .clone()
        .ok_or_else(|| Error::at(&operation.path, "this operation needs a `value`"))
}

fn from_of(operation: &Operation) -> Result<&str, Error> {
    operation
        .from
        .as_deref()
        .ok_or_else(|| Error::at(&operation.path, "this operation needs a `from`"))
}

/// Splits a pointer into the parent's pointer and the last token.
fn parent_of(pointer: &str) -> Result<(String, String), Error> {
    let mut tokens = split_pointer(pointer)?;
    let last = tokens
        .pop()
        .ok_or_else(|| Error::at(pointer, "the document root cannot be added or removed"))?;
    let parent = tokens
        .iter()
        .fold(String::new(), |acc, token| push_pointer(&acc, token));
    Ok((parent, last))
}

fn resolve<'a>(document: &'a Document, pointer: &str) -> Result<&'a Node, Error> {
    let mut here = document.root();
    for token in split_pointer(pointer)? {
        here = here
            .child(&token)
            .ok_or_else(|| Error::at(pointer, "there is nothing at that path"))?;
    }
    Ok(here)
}

fn resolve_mut<'a>(document: &'a mut Document, pointer: &str) -> Result<&'a mut Node, Error> {
    let mut here = document.root_mut();
    for token in split_pointer(pointer)? {
        here = here
            .child_mut(&token)
            .ok_or_else(|| Error::at(pointer, "there is nothing at that path"))?;
    }
    Ok(here)
}

/// How long the container at `pointer` is, and whether it is an array.
fn shape(document: &Document, pointer: &str) -> Result<(bool, usize), Error> {
    let container = resolve(document, pointer)?;
    let len = container.len().ok_or_else(|| {
        Error::at(
            pointer,
            format!("{} has no members to add to or remove", container.kind()),
        )
    })?;
    Ok((container.kind() == "an array", len))
}

/// Takes the member or element at `pointer` out, trivia and all, and says
/// which slot it came out of so that an undo can put it back there.
fn take(document: &mut Document, pointer: &str) -> Result<(Fragment, usize), Error> {
    let (parent, last) = parent_of(pointer)?;
    let (is_array, len) = shape(document, &parent)?;
    let nothing = || Error::at(pointer, "there is nothing at that path");
    let container = resolve_mut(document, &parent)?;
    if is_array {
        let index =
            array_index(&last, len, false).map_err(|message| Error::at(pointer, message))?;
        Ok((container.take_item(index).ok_or_else(nothing)?, index))
    } else {
        let at = container.position_of(&last).ok_or_else(nothing)?;
        Ok((container.take_member(&last).ok_or_else(nothing)?, at))
    }
}

/// Adds at `pointer`.
///
/// RFC 6902: on an object member that already exists this **replaces** it, so
/// the inverse is a `replace` carrying the old fragment rather than a
/// `remove`. On an array it inserts, and `-` means "after the last element".
fn add(
    document: &mut Document,
    pointer: &str,
    fragment: Fragment,
    position: Option<usize>,
) -> Result<Vec<Operation>, Error> {
    let (parent, last) = parent_of(pointer)?;
    let (is_array, len) = shape(document, &parent)?;
    if is_array {
        let index = array_index(&last, len, true).map_err(|message| Error::at(pointer, message))?;
        let container = resolve_mut(document, &parent)?;
        container.put_item(index, fragment)?;
        let settled = push_pointer(&parent, &index.to_string());
        return Ok(vec![Operation::plain(Op::Remove, settled)]);
    }
    if resolve(document, &parent)?.position_of(&last).is_some() {
        // RFC 6902 section 4.1 makes `add` over an existing member a replace,
        // so take the replace route rather than take-then-put: taking the
        // member out carries its `before` comment, its spacing and its key
        // spelling away with it, and the incoming fragment — which came off
        // the wire with no trivia at all — would be written in their place.
        // The file's bytes would change around an edit that only changed a
        // value, and the inverse would not put them back. `set` swaps the
        // value inside the member and leaves the member alone.
        let target = resolve_mut(document, pointer)?;
        let old = target.set(fragment.into_node());
        return Ok(vec![Operation::with_value(
            Op::Replace,
            pointer.to_owned(),
            Fragment::from_node(old),
        )]);
    }
    let at = position.unwrap_or(len).min(len);
    let container = resolve_mut(document, &parent)?;
    container.put_member(&last, fragment, at)?;
    Ok(vec![Operation::plain(Op::Remove, pointer.to_owned())])
}

/// An RFC 6902 array index. `-` means "one past the end", and only `add`
/// accepts it.
fn array_index(token: &str, len: usize, appending: bool) -> Result<usize, &'static str> {
    if token == "-" {
        return if appending {
            Ok(len)
        } else {
            Err("`-` names the end of an array and only `add` may use it")
        };
    }
    if token != "0" && token.starts_with('0') {
        return Err("an array index has no leading zero");
    }
    let index: usize = token
        .parse()
        .map_err(|_ignored| "that is neither a member name nor an array index")?;
    let limit = if appending {
        len
    } else {
        len.saturating_sub(1)
    };
    if index > limit {
        return Err("that index is past the end of the array");
    }
    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::{Patch, apply, patch_text};
    use crate::jsonc::Document;

    const FILE: &str = "{\n  // why\n  \"a\": 1,\n  \"b\": [1, 2],\n  \"c\": {\"d\": true}\n}";

    fn round(patch: &str) -> String {
        let document = Document::parse(FILE).expect("a document");
        let forward = Patch::from_text(patch).expect("a patch");
        let (patched, inverse) = apply(&document, &forward).expect("the patch applies");
        let (back, _again) = apply(&patched, &inverse).expect("the inverse applies");
        assert_eq!(back.to_text(), FILE, "undo was not byte-exact");
        patched.to_text()
    }

    #[test]
    fn replacing_a_scalar_keeps_the_comment_above_it() {
        let after = round(r#"[{"op":"replace","path":"/a","value":7}]"#);
        assert!(after.contains("// why"), "{after}");
        assert!(after.contains("\"a\": 7"), "{after}");
    }

    #[test]
    fn removing_a_commented_member_and_undoing_it_restores_the_comment() {
        let after = round(r#"[{"op":"remove","path":"/a"}]"#);
        assert!(!after.contains("\"a\""), "{after}");
    }

    #[test]
    fn adding_a_member_appends_it() {
        let after = round(r#"[{"op":"add","path":"/e","value":"x"}]"#);
        assert!(after.contains("\"e\":\"x\""), "{after}");
    }

    #[test]
    fn adding_to_an_array_by_index_and_by_dash() {
        let after =
            round(r#"[{"op":"add","path":"/b/0","value":0},{"op":"add","path":"/b/-","value":3}]"#);
        assert!(after.contains("[0,1, 2,3]"), "{after}");
    }

    #[test]
    fn removing_an_array_element() {
        let after = round(r#"[{"op":"remove","path":"/b/0"}]"#);
        assert!(after.contains("\"b\": [ 2]"), "{after}");
    }

    #[test]
    fn a_test_that_holds_changes_nothing() {
        let after = round(r#"[{"op":"test","path":"/a","value":1}]"#);
        assert_eq!(after, FILE);
    }

    #[test]
    fn a_test_that_fails_stops_the_patch() {
        let document = Document::parse(FILE).expect("a document");
        let patch = Patch::from_text(r#"[{"op":"test","path":"/a","value":2}]"#).expect("a patch");
        let error = apply(&document, &patch).expect_err("the test does not hold");
        assert_eq!(error.pointer, "/a");
    }

    #[test]
    fn move_and_copy_undo_cleanly() {
        round(r#"[{"op":"move","from":"/a","path":"/c/a"}]"#);
        round(r#"[{"op":"copy","from":"/a","path":"/c/a"}]"#);
    }

    #[test]
    fn the_wire_form_round_trips_the_operations() {
        let patch = Patch::from_text(
            r#"[{"op":"replace","path":"/a","value":7},{"op":"remove","path":"/b/0"}]"#,
        )
        .expect("a patch");
        let again = Patch::from_text(&patch.to_text()).expect("its own text");
        assert_eq!(patch, again);
    }

    #[test]
    fn patch_text_gives_back_the_file_and_the_inverse() {
        let (patched, inverse) =
            patch_text(FILE, r#"[{"op":"replace","path":"/a","value":7}]"#).expect("it applies");
        let (back, _again) = patch_text(&patched, &inverse).expect("the inverse applies");
        assert_eq!(back, FILE);
    }

    #[test]
    fn an_unknown_operation_is_refused() {
        assert!(Patch::from_text(r#"[{"op":"frobnicate","path":"/a"}]"#).is_err());
    }

    #[test]
    fn a_path_that_does_not_resolve_is_refused() {
        let document = Document::parse(FILE).expect("a document");
        let patch = Patch::from_text(r#"[{"op":"remove","path":"/zzz"}]"#).expect("a patch");
        assert!(apply(&document, &patch).is_err());
    }
}
