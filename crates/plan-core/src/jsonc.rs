// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! JSONC: canonical proto JSON **plus comments**, read and written without
//! losing a byte.
//!
//! This is the layer decisions-log item 74 puts on top of `crates/proto`'s
//! canonical codec, and spec section 13's "Exact round-trip" is its whole
//! reason for existing:
//!
//! > The editor's model is the playbook JSON. Edits are JSON Patches with
//! > inverse-patch undo, and comments survive a load-and-save unchanged.
//!
//! So this is a **lossless, trivia-preserving** reader and writer, not "strip
//! the comments and reformat". A [`Document`] keeps every byte of whitespace
//! and every comment in the slot it was found in, and [`Document::to_text`]
//! puts them back exactly. `the_example_round_trips_byte_for_byte` in
//! `tests/jsonc.rs` is the assertion; a dropped comment is a bug even when the
//! JSON is identical (`tests/golden/plan-core/README.md`).
//!
//! # Trivia slots
//!
//! Every place a comment can legally sit is a named slot, and the writer emits
//! the slots in source order, so exactness is structural rather than lucky:
//!
//! ```text
//! {  <before> "key" <pre_colon> : <post_colon> VALUE <after> , <before> … }
//! [  <before> VALUE <after> , <before> VALUE <after> ]
//! {  <inner> }                      an empty container
//! <prologue> ROOT <epilogue>        the document
//! ```
//!
//! # What this layer is strict about
//!
//! Comments — `//` to end of line, and `/* … */` — are the **only** extension
//! over RFC 8259. A trailing comma is refused, because canonical proto JSON
//! never writes one and accepting it here would mean the round trip could not
//! promise byte equality to a file the editor itself would not produce.
//! Duplicate keys are refused for the reason the codec refuses them: a
//! playbook whose meaning depends on which of two keys wins is not a sealed
//! order.
//!
//! # What this layer does *not* know
//!
//! The schema. A [`Document`] is a JSON tree with trivia; whether it is a
//! playbook at all is [`crate::canonical`]'s question, and whether it is a
//! *valid* one is the verifier's.

use pharmakos_proto::json::Json;

use crate::error::{Error, push_pointer};

/// A JSONC document: a root value with everything around and inside it.
///
/// [`Document::parse`] and [`Document::to_text`] are inverses on every input
/// they accept, byte for byte.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Document {
    /// Whitespace and comments before the root value — where a file's SPDX
    /// header and its description comment live.
    prologue: String,
    root: Node,
    /// Whitespace and comments after the root value.
    epilogue: String,
}

impl Document {
    /// Reads a JSONC text.
    ///
    /// # Errors
    ///
    /// The first byte that does not belong, reported at `/byte/NNN` because a
    /// syntax error has no place in the document tree yet.
    pub fn parse(text: &str) -> Result<Self, Error> {
        let mut reader = Reader {
            bytes: text.as_bytes(),
            text,
            pos: 0,
            depth: 0,
        };
        let prologue = reader.trivia()?;
        let root = reader.value()?;
        let epilogue = reader.trivia()?;
        if reader.pos < reader.bytes.len() {
            return Err(reader.error("trailing content after the JSON value"));
        }
        Ok(Self {
            prologue,
            root,
            epilogue,
        })
    }

    /// Writes the document back out, byte for byte as it was read.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&self.prologue);
        self.root.write(&mut out);
        out.push_str(&self.epilogue);
        out
    }

    /// The document's root node.
    #[must_use]
    pub const fn root(&self) -> &Node {
        &self.root
    }

    /// The document's root node, for editing.
    pub fn root_mut(&mut self) -> &mut Node {
        &mut self.root
    }

    /// The document as a plain JSON value, trivia discarded.
    ///
    /// This is what the schema codec reads; the trivia comes back through
    /// [`crate::canonical`], which re-anchors each comment to the node it was
    /// written against.
    #[must_use]
    pub fn to_json(&self) -> Json {
        self.root.to_json()
    }

    /// A document around one value, with no comments and no formatting of its
    /// own beyond what the canonical writer will give it.
    #[must_use]
    pub fn from_json(value: &Json) -> Self {
        Self {
            prologue: String::new(),
            root: Node::from_json(value),
            epilogue: String::new(),
        }
    }

    /// Every comment in the document, in source order, each with the node it
    /// was written against.
    #[must_use]
    pub fn comments(&self) -> Vec<Comment> {
        let mut found = Vec::new();
        collect(&self.prologue, &Anchor::Prologue, &mut found);
        self.root.collect_comments("", &mut found);
        collect(&self.epilogue, &Anchor::Epilogue, &mut found);
        found
    }
}

/// One JSON value, with the trivia inside it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Node {
    value: Value,
}

/// What a node is.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Value {
    Null,
    Bool(bool),
    /// The lexeme exactly as written. Never parsed into a float
    /// (AGENTS.md section 4.2); the codec converts it when a field asks.
    Number(String),
    Str {
        /// The source text including its quotes and escapes.
        raw: String,
        /// The unescaped characters.
        text: String,
    },
    Array(Vec<Item>, String),
    Object(Vec<Member>, String),
}

/// A value with the trivia that surrounded it where it came from.
///
/// This is what makes undo byte-exact. A patch that removes a member and the
/// inverse patch that puts it back would otherwise lose the comment above it
/// and the spacing around it: RFC 6902's wire form carries a *value*, and a
/// value has no comments. So an inverse patch this crate produces carries a
/// fragment, and only the wire form — [`crate::patch::Patch::to_text`] —
/// flattens one back to a plain value. That loss is the wire format's, stated
/// where it happens, rather than a silent one.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Fragment {
    node: Node,
    trivia: Trivia,
}

/// The trivia a fragment was cut out with.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Trivia {
    /// Built from a plain JSON value: no trivia to restore.
    None,
    Member {
        before: String,
        /// The key exactly as it was spelled in the source, quotes and escapes
        /// included. A key may be written `"A"` where the canonical form
        /// would write `"A"`, and an undo has to give the file back as it was.
        key_raw: String,
        pre_colon: String,
        post_colon: String,
        after: String,
    },
    Item {
        before: String,
        after: String,
    },
}

impl Fragment {
    /// A fragment around a plain JSON value, with no trivia.
    #[must_use]
    pub fn from_json(value: &Json) -> Self {
        Self {
            node: Node::from_json(value),
            trivia: Trivia::None,
        }
    }

    /// A fragment around a node, with no trivia.
    #[must_use]
    pub const fn from_node(node: Node) -> Self {
        Self {
            node,
            trivia: Trivia::None,
        }
    }

    /// The value inside, trivia discarded.
    #[must_use]
    pub fn to_json(&self) -> Json {
        self.node.to_json()
    }

    /// The node inside.
    #[must_use]
    pub const fn node(&self) -> &Node {
        &self.node
    }

    /// The node inside, taken out.
    #[must_use]
    pub fn into_node(self) -> Node {
        self.node
    }
}

/// One element of an array, with the trivia around it.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Item {
    before: String,
    node: Node,
    after: String,
}

/// One member of an object, with the trivia around it.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Member {
    before: String,
    key_raw: String,
    key: String,
    pre_colon: String,
    post_colon: String,
    node: Node,
    after: String,
}

impl Node {
    /// A node from a plain JSON value, with no trivia.
    #[must_use]
    pub fn from_json(value: &Json) -> Self {
        let value = match value {
            Json::Null => Value::Null,
            Json::Bool(flag) => Value::Bool(*flag),
            Json::Number(lexeme) => Value::Number(lexeme.clone()),
            Json::String(text) => Value::Str {
                raw: escape(text),
                text: text.clone(),
            },
            Json::Array(items) => Value::Array(
                items
                    .iter()
                    .map(|item| Item {
                        before: String::new(),
                        node: Self::from_json(item),
                        after: String::new(),
                    })
                    .collect(),
                String::new(),
            ),
            Json::Object(entries) => Value::Object(
                entries
                    .iter()
                    .map(|(key, item)| Member {
                        before: String::new(),
                        key_raw: escape(key),
                        key: key.clone(),
                        pre_colon: String::new(),
                        post_colon: String::new(),
                        node: Self::from_json(item),
                        after: String::new(),
                    })
                    .collect(),
                String::new(),
            ),
        };
        Self { value }
    }

    /// The node as a plain JSON value, trivia discarded.
    #[must_use]
    pub fn to_json(&self) -> Json {
        match &self.value {
            Value::Null => Json::Null,
            Value::Bool(flag) => Json::Bool(*flag),
            Value::Number(lexeme) => Json::Number(lexeme.clone()),
            Value::Str { text, .. } => Json::String(text.clone()),
            Value::Array(items, _) => {
                Json::Array(items.iter().map(|item| item.node.to_json()).collect())
            }
            Value::Object(members, _) => Json::Object(
                members
                    .iter()
                    .map(|member| (member.key.clone(), member.node.to_json()))
                    .collect(),
            ),
        }
    }

    /// The child under one RFC 6901 token, if this node has one.
    #[must_use]
    pub fn child(&self, token: &str) -> Option<&Self> {
        match &self.value {
            Value::Object(members, _) => members
                .iter()
                .find(|member| member.key == token)
                .map(|member| &member.node),
            Value::Array(items, _) => items.get(index_of(token)?).map(|item| &item.node),
            _ => None,
        }
    }

    /// The child under one RFC 6901 token, for editing.
    pub fn child_mut(&mut self, token: &str) -> Option<&mut Self> {
        match &mut self.value {
            Value::Object(members, _) => members
                .iter_mut()
                .find(|member| member.key == token)
                .map(|member| &mut member.node),
            Value::Array(items, _) => {
                let index = index_of(token)?;
                items.get_mut(index).map(|item| &mut item.node)
            }
            _ => None,
        }
    }

    /// True when this node is an object or an array.
    #[must_use]
    pub const fn is_container(&self) -> bool {
        matches!(self.value, Value::Object(..) | Value::Array(..))
    }

    /// What kind of value this is, for a diagnostic.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self.value {
            Value::Null => "null",
            Value::Bool(_) => "a boolean",
            Value::Number(_) => "a number",
            Value::Str { .. } => "a string",
            Value::Array(..) => "an array",
            Value::Object(..) => "an object",
        }
    }

    /// True when this node is an empty container. `false` for a scalar,
    /// which has no members to be empty of.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == Some(0)
    }

    /// How many elements an array has, or how many members an object has.
    #[must_use]
    pub fn len(&self) -> Option<usize> {
        match &self.value {
            Value::Array(items, _) => Some(items.len()),
            Value::Object(members, _) => Some(members.len()),
            _ => None,
        }
    }

    /// The position of a key in an object, if it has one.
    #[must_use]
    pub fn position_of(&self, key: &str) -> Option<usize> {
        match &self.value {
            Value::Object(members, _) => members.iter().position(|member| member.key == key),
            _ => None,
        }
    }

    /// Takes a member out of an object, trivia and all, so that putting it
    /// back restores the file byte for byte.
    pub fn take_member(&mut self, key: &str) -> Option<Fragment> {
        let Value::Object(members, _) = &mut self.value else {
            return None;
        };
        let at = members.iter().position(|member| member.key == key)?;
        let member = members.remove(at);
        Some(Fragment {
            node: member.node,
            trivia: Trivia::Member {
                before: member.before,
                key_raw: member.key_raw,
                pre_colon: member.pre_colon,
                post_colon: member.post_colon,
                after: member.after,
            },
        })
    }

    /// Puts a member into an object at `at`, or at the end when `at` is past
    /// it.
    ///
    /// # Errors
    ///
    /// When this node is not an object, or the key is already there.
    pub fn put_member(&mut self, key: &str, fragment: Fragment, at: usize) -> Result<(), Error> {
        let Value::Object(members, _) = &mut self.value else {
            return Err(Error::at("", "not an object"));
        };
        if members.iter().any(|member| member.key == key) {
            return Err(Error::at("", format!("`{key}` is already there")));
        }
        let (before, spelled, pre_colon, post_colon, after) = match fragment.trivia {
            Trivia::Member {
                before,
                key_raw,
                pre_colon,
                post_colon,
                after,
            } => (before, Some(key_raw), pre_colon, post_colon, after),
            Trivia::Item { before, after } => (before, None, String::new(), String::new(), after),
            Trivia::None => (
                String::new(),
                None,
                String::new(),
                String::new(),
                String::new(),
            ),
        };
        // The key comes back spelled as it was written, but only when it is
        // still the same key: a fragment moved to a different name takes the
        // canonical spelling of the name it landed on.
        let key_raw = spelled
            .filter(|raw| unescaped(raw).as_deref() == Some(key))
            .unwrap_or_else(|| escape(key));
        let member = Member {
            before,
            key_raw,
            key: key.to_owned(),
            pre_colon,
            post_colon,
            node: fragment.node,
            after,
        };
        let at = at.min(members.len());
        members.insert(at, member);
        Ok(())
    }

    /// Takes an element out of an array, trivia and all.
    pub fn take_item(&mut self, index: usize) -> Option<Fragment> {
        let Value::Array(items, _) = &mut self.value else {
            return None;
        };
        if index >= items.len() {
            return None;
        }
        let item = items.remove(index);
        Some(Fragment {
            node: item.node,
            trivia: Trivia::Item {
                before: item.before,
                after: item.after,
            },
        })
    }

    /// Puts an element into an array at `index`.
    ///
    /// # Errors
    ///
    /// When this node is not an array, or the index is past its end.
    pub fn put_item(&mut self, index: usize, fragment: Fragment) -> Result<(), Error> {
        let Value::Array(items, _) = &mut self.value else {
            return Err(Error::at("", "not an array"));
        };
        if index > items.len() {
            return Err(Error::at("", "that index is past the end of the array"));
        }
        let (before, after) = match fragment.trivia {
            Trivia::Item { before, after }
            | Trivia::Member {
                before,
                key_raw: _,
                pre_colon: _,
                post_colon: _,
                after,
            } => (before, after),
            Trivia::None => (String::new(), String::new()),
        };
        items.insert(
            index,
            Item {
                before,
                node: fragment.node,
                after,
            },
        );
        Ok(())
    }

    /// Swaps this node's value for another, keeping the trivia around it.
    #[must_use]
    pub fn set(&mut self, node: Self) -> Self {
        std::mem::replace(self, node)
    }

    fn write(&self, out: &mut String) {
        match &self.value {
            Value::Null => out.push_str("null"),
            Value::Bool(true) => out.push_str("true"),
            Value::Bool(false) => out.push_str("false"),
            Value::Number(lexeme) => out.push_str(lexeme),
            Value::Str { raw, .. } => out.push_str(raw),
            Value::Array(items, inner) => {
                out.push('[');
                if items.is_empty() {
                    out.push_str(inner);
                }
                for (index, item) in items.iter().enumerate() {
                    out.push_str(&item.before);
                    item.node.write(out);
                    out.push_str(&item.after);
                    if index.saturating_add(1) < items.len() {
                        out.push(',');
                    }
                }
                out.push(']');
            }
            Value::Object(members, inner) => {
                out.push('{');
                if members.is_empty() {
                    out.push_str(inner);
                }
                for (index, member) in members.iter().enumerate() {
                    out.push_str(&member.before);
                    out.push_str(&member.key_raw);
                    out.push_str(&member.pre_colon);
                    out.push(':');
                    out.push_str(&member.post_colon);
                    member.node.write(out);
                    out.push_str(&member.after);
                    if index.saturating_add(1) < members.len() {
                        out.push(',');
                    }
                }
                out.push('}');
            }
        }
    }

    fn collect_comments(&self, pointer: &str, found: &mut Vec<Comment>) {
        match &self.value {
            Value::Array(items, inner) => {
                if items.is_empty() {
                    collect(inner, &Anchor::Inside(pointer.to_owned()), found);
                }
                let mut previous: Option<String> = None;
                for (index, item) in items.iter().enumerate() {
                    let child = push_pointer(pointer, &index.to_string());
                    let last = index.saturating_add(1) == items.len();
                    opening(&item.before, previous.as_deref(), &child, found);
                    item.node.collect_comments(&child, found);
                    closing(&item.after, &child, pointer, last, found);
                    previous = Some(child);
                }
            }
            Value::Object(members, inner) => {
                if members.is_empty() {
                    collect(inner, &Anchor::Inside(pointer.to_owned()), found);
                }
                let mut previous: Option<String> = None;
                for (index, member) in members.iter().enumerate() {
                    let child = push_pointer(pointer, &member.key);
                    let last = index.saturating_add(1) == members.len();
                    opening(&member.before, previous.as_deref(), &child, found);
                    collect(&member.pre_colon, &Anchor::Before(child.clone()), found);
                    collect(&member.post_colon, &Anchor::Before(child.clone()), found);
                    member.node.collect_comments(&child, found);
                    closing(&member.after, &child, pointer, last, found);
                    previous = Some(child);
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::Str { .. } => {}
        }
    }
}

/// The trivia between the opening bracket (or the preceding comma) and this
/// member.
///
/// The split at the first newline is what makes `"a": 1, // why` read the way
/// a human wrote it. The comma has already been consumed by the time this
/// trivia starts, so a comment on the *same line* as the comma belongs to the
/// member before it, and only a comment on a line of its own belongs to the
/// member after it.
fn opening(trivia: &str, previous: Option<&str>, child: &str, found: &mut Vec<Comment>) {
    match (trivia.find('\n'), previous) {
        (Some(at), Some(sibling)) => {
            let head = trivia.get(..at).unwrap_or_default();
            let tail = trivia.get(at..).unwrap_or_default();
            collect(head, &Anchor::After(sibling.to_owned()), found);
            collect(tail, &Anchor::Before(child.to_owned()), found);
        }
        (None, Some(sibling)) => collect(trivia, &Anchor::After(sibling.to_owned()), found),
        (_, None) => collect(trivia, &Anchor::Before(child.to_owned()), found),
    }
}

/// The trivia between this member's value and the comma or closing bracket
/// after it.
///
/// On the last member, a comment on a line of its own sits at the end of the
/// container rather than on the member's line, and [`Anchor::Inside`] is where
/// the canonical writer puts it back.
fn closing(trivia: &str, child: &str, container: &str, last: bool, found: &mut Vec<Comment>) {
    match trivia.find('\n') {
        Some(at) if last => {
            let head = trivia.get(..at).unwrap_or_default();
            let tail = trivia.get(at..).unwrap_or_default();
            collect(head, &Anchor::After(child.to_owned()), found);
            collect(tail, &Anchor::Inside(container.to_owned()), found);
        }
        _ => collect(trivia, &Anchor::After(child.to_owned()), found),
    }
}

/// An array index token: RFC 6901 spells one as decimal digits with no leading
/// zero (except `0` itself).
fn index_of(token: &str) -> Option<usize> {
    if token != "0" && token.starts_with('0') {
        return None;
    }
    token.parse::<usize>().ok()
}

// ---------------------------------------------------------------------------
// Comments
// ---------------------------------------------------------------------------

/// Where a comment was written, relative to the node it belongs to.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Anchor {
    /// Before the root value: a file header.
    Prologue,
    /// After the root value.
    Epilogue,
    /// Before the member or element at this pointer — the usual case, and the
    /// one the canonical writer puts back on its own line above the node.
    Before(String),
    /// After the member or element at this pointer, on the same line.
    After(String),
    /// At the end of the container at this pointer: inside an empty one, or on
    /// a line of its own after the container's last member.
    Inside(String),
}

impl Anchor {
    /// The pointer this anchor hangs off, if it hangs off a node.
    #[must_use]
    pub fn pointer(&self) -> Option<&str> {
        match self {
            Self::Prologue | Self::Epilogue => None,
            Self::Before(pointer) | Self::After(pointer) | Self::Inside(pointer) => Some(pointer),
        }
    }
}

/// How a comment was spelled. Both survive; the canonical writer puts a line
/// comment on its own line and a block comment where it was.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CommentKind {
    /// `// …` to the end of the line.
    Line,
    /// `/* … */`.
    Block,
}

/// One comment, with where it was written.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Comment {
    /// Where it sat.
    pub anchor: Anchor,
    /// How it was spelled.
    pub kind: CommentKind,
    /// The comment **including** its `//` or `/* */` delimiters, exactly as
    /// written. Keeping the delimiters means the writer never has to guess how
    /// to spell one back.
    pub text: String,
}

/// Pulls the comments out of one trivia slot, in order.
fn collect(trivia: &str, anchor: &Anchor, found: &mut Vec<Comment>) {
    let bytes = trivia.as_bytes();
    let mut pos = 0usize;
    while pos < bytes.len() {
        match (bytes.get(pos), bytes.get(pos.saturating_add(1))) {
            (Some(b'/'), Some(b'/')) => {
                let end = trivia
                    .get(pos..)
                    .and_then(|rest| rest.find('\n'))
                    .map_or(trivia.len(), |offset| pos.saturating_add(offset));
                if let Some(text) = trivia.get(pos..end) {
                    found.push(Comment {
                        anchor: anchor.clone(),
                        kind: CommentKind::Line,
                        text: text.trim_end().to_owned(),
                    });
                }
                pos = end;
            }
            (Some(b'/'), Some(b'*')) => {
                let end = trivia
                    .get(pos..)
                    .and_then(|rest| rest.find("*/"))
                    .map_or(trivia.len(), |offset| {
                        pos.saturating_add(offset).saturating_add(2)
                    });
                if let Some(text) = trivia.get(pos..end) {
                    found.push(Comment {
                        anchor: anchor.clone(),
                        kind: CommentKind::Block,
                        text: text.to_owned(),
                    });
                }
                pos = end;
            }
            _ => pos = pos.saturating_add(1),
        }
    }
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// The deepest nesting a document may have, matching the codec's own cap: low
/// enough that a hostile file cannot exhaust the stack, generous for a
/// playbook whose condition trees the vocabulary caps at depth 4.
const MAX_DEPTH: usize = 64;

struct Reader<'a> {
    bytes: &'a [u8],
    text: &'a str,
    pos: usize,
    depth: usize,
}

impl Reader<'_> {
    fn error(&self, message: impl Into<String>) -> Error {
        Error::at_byte(self.pos, message)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos.saturating_add(offset)).copied()
    }

    /// Whitespace and comments, returned verbatim.
    fn trivia(&mut self) -> Result<String, Error> {
        let start = self.pos;
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\r' | b'\n') => self.pos = self.pos.saturating_add(1),
                Some(b'/') => match self.peek_at(1) {
                    Some(b'/') => {
                        while !matches!(self.peek(), None | Some(b'\n')) {
                            self.pos = self.pos.saturating_add(1);
                        }
                    }
                    Some(b'*') => {
                        self.pos = self.pos.saturating_add(2);
                        loop {
                            match (self.peek(), self.peek_at(1)) {
                                (Some(b'*'), Some(b'/')) => {
                                    self.pos = self.pos.saturating_add(2);
                                    break;
                                }
                                (None, _) => {
                                    return Err(self.error("a block comment was never closed"));
                                }
                                _ => self.pos = self.pos.saturating_add(1),
                            }
                        }
                    }
                    _ => return Err(self.error("a lone `/` is neither a comment nor a value")),
                },
                _ => break,
            }
        }
        Ok(self.slice(start, self.pos)?.to_owned())
    }

    fn slice(&self, from: usize, to: usize) -> Result<&str, Error> {
        self.text
            .get(from..to)
            .ok_or_else(|| Error::at_byte(from, "a token ended inside a character"))
    }

    /// Consumes one expected byte, or reports where it was wanted.
    fn require(&mut self, byte: u8, what: &str) -> Result<(), Error> {
        if self.peek() == Some(byte) {
            self.pos = self.pos.saturating_add(1);
            Ok(())
        } else {
            Err(self.error(format!("expected {what}")))
        }
    }

    fn value(&mut self) -> Result<Node, Error> {
        if self.depth >= MAX_DEPTH {
            return Err(self.error(format!("nested deeper than {MAX_DEPTH} levels")));
        }
        let value = match self.peek() {
            Some(b'{') => self.object()?,
            Some(b'[') => self.array()?,
            Some(b'"') => {
                let (raw, text) = self.string()?;
                Value::Str { raw, text }
            }
            Some(b't') => {
                self.literal("true")?;
                Value::Bool(true)
            }
            Some(b'f') => {
                self.literal("false")?;
                Value::Bool(false)
            }
            Some(b'n') => {
                self.literal("null")?;
                Value::Null
            }
            Some(byte) if byte == b'-' || byte.is_ascii_digit() => Value::Number(self.number()?),
            Some(_) => return Err(self.error("expected a JSON value")),
            None => return Err(self.error("the document ended where a value was expected")),
        };
        Ok(Node { value })
    }

    fn literal(&mut self, word: &str) -> Result<(), Error> {
        let end = self.pos.saturating_add(word.len());
        if self.text.get(self.pos..end) == Some(word) {
            self.pos = end;
            Ok(())
        } else {
            Err(self.error(format!("expected `{word}`")))
        }
    }

    fn number(&mut self) -> Result<String, Error> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos = self.pos.saturating_add(1);
        }
        let digits_from = self.pos;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.pos = self.pos.saturating_add(1);
        }
        if self.pos == digits_from {
            return Err(self.error("a number needs at least one digit"));
        }
        if self.peek() == Some(b'.') {
            self.pos = self.pos.saturating_add(1);
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.pos = self.pos.saturating_add(1);
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos = self.pos.saturating_add(1);
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos = self.pos.saturating_add(1);
            }
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.pos = self.pos.saturating_add(1);
            }
        }
        Ok(self.slice(start, self.pos)?.to_owned())
    }

    /// A string: the raw source including quotes, and the unescaped text.
    fn string(&mut self) -> Result<(String, String), Error> {
        let start = self.pos;
        self.require(b'"', "a string")?;
        let mut text = String::new();
        loop {
            match self.peek() {
                None => return Err(self.error("a string was never closed")),
                Some(b'"') => {
                    self.pos = self.pos.saturating_add(1);
                    let raw = self.slice(start, self.pos)?.to_owned();
                    return Ok((raw, text));
                }
                Some(b'\\') => {
                    self.pos = self.pos.saturating_add(1);
                    let escape = self
                        .peek()
                        .ok_or_else(|| self.error("an escape ran off the end of the document"))?;
                    self.pos = self.pos.saturating_add(1);
                    match escape {
                        b'"' => text.push('"'),
                        b'\\' => text.push('\\'),
                        b'/' => text.push('/'),
                        b'b' => text.push('\u{8}'),
                        b'f' => text.push('\u{c}'),
                        b'n' => text.push('\n'),
                        b'r' => text.push('\r'),
                        b't' => text.push('\t'),
                        b'u' => text.push(self.unicode_escape()?),
                        _ => return Err(self.error("unknown escape sequence")),
                    }
                }
                Some(byte) if byte < 0x20 => {
                    return Err(self.error("a raw control character is not allowed in a string"));
                }
                Some(_) => {
                    let rest = self.text.get(self.pos..).unwrap_or_default();
                    let character = rest
                        .chars()
                        .next()
                        .ok_or_else(|| self.error("a string ended inside a character"))?;
                    text.push(character);
                    self.pos = self.pos.saturating_add(character.len_utf8());
                }
            }
        }
    }

    /// The four hex digits after `\u`, with the surrogate pair that may follow.
    fn unicode_escape(&mut self) -> Result<char, Error> {
        let first = self.hex4()?;
        if (0xD800..0xDC00).contains(&first) {
            if self.peek() != Some(b'\\') || self.peek_at(1) != Some(b'u') {
                return Err(self.error("a high surrogate needs a low surrogate after it"));
            }
            self.pos = self.pos.saturating_add(2);
            let second = self.hex4()?;
            if !(0xDC00..0xE000).contains(&second) {
                return Err(self.error("a high surrogate needs a low surrogate after it"));
            }
            let combined = 0x1_0000u32
                .saturating_add(first.saturating_sub(0xD800).saturating_mul(0x400))
                .saturating_add(second.saturating_sub(0xDC00));
            return char::from_u32(combined)
                .ok_or_else(|| self.error("that surrogate pair is not a character"));
        }
        char::from_u32(first).ok_or_else(|| self.error("that escape is not a character"))
    }

    fn hex4(&mut self) -> Result<u32, Error> {
        let end = self.pos.saturating_add(4);
        let digits = self
            .text
            .get(self.pos..end)
            .ok_or_else(|| self.error("`\\u` needs four hex digits"))?;
        let value = u32::from_str_radix(digits, 16)
            .map_err(|_ignored| self.error("`\\u` needs four hex digits"))?;
        self.pos = end;
        Ok(value)
    }

    fn array(&mut self) -> Result<Value, Error> {
        self.require(b'[', "`[`")?;
        self.depth = self.depth.saturating_add(1);
        let mut items: Vec<Item> = Vec::new();
        let mut inner = String::new();
        loop {
            let before = self.trivia()?;
            if self.peek() == Some(b']') {
                if items.is_empty() {
                    inner = before;
                } else {
                    return Err(self.error("a trailing comma is not canonical JSON"));
                }
                self.pos = self.pos.saturating_add(1);
                break;
            }
            let node = self.value()?;
            let after = self.trivia()?;
            items.push(Item {
                before,
                node,
                after,
            });
            match self.peek() {
                Some(b',') => self.pos = self.pos.saturating_add(1),
                Some(b']') => {
                    self.pos = self.pos.saturating_add(1);
                    break;
                }
                _ => return Err(self.error("expected `,` or `]`")),
            }
        }
        self.depth = self.depth.saturating_sub(1);
        Ok(Value::Array(items, inner))
    }

    fn object(&mut self) -> Result<Value, Error> {
        self.require(b'{', "`{`")?;
        self.depth = self.depth.saturating_add(1);
        let mut members: Vec<Member> = Vec::new();
        let mut inner = String::new();
        loop {
            let before = self.trivia()?;
            if self.peek() == Some(b'}') {
                if members.is_empty() {
                    inner = before;
                } else {
                    return Err(self.error("a trailing comma is not canonical JSON"));
                }
                self.pos = self.pos.saturating_add(1);
                break;
            }
            let key_at = self.pos;
            let (key_raw, key) = self.string()?;
            if members.iter().any(|member| member.key == key) {
                return Err(Error::at_byte(
                    key_at,
                    format!("`{key}` is written twice in the same object"),
                ));
            }
            let pre_colon = self.trivia()?;
            self.require(b':', "`:` after a key")?;
            let post_colon = self.trivia()?;
            let node = self.value()?;
            let after = self.trivia()?;
            members.push(Member {
                before,
                key_raw,
                key,
                pre_colon,
                post_colon,
                node,
                after,
            });
            match self.peek() {
                Some(b',') => self.pos = self.pos.saturating_add(1),
                Some(b'}') => {
                    self.pos = self.pos.saturating_add(1);
                    break;
                }
                _ => return Err(self.error("expected `,` or `}`")),
            }
        }
        self.depth = self.depth.saturating_sub(1);
        Ok(Value::Object(members, inner))
    }
}

/// Reads a JSON string lexeme back, so a key's source spelling can be checked
/// against the key it is being put back under.
fn unescaped(raw: &str) -> Option<String> {
    let mut reader = Reader {
        bytes: raw.as_bytes(),
        text: raw,
        pos: 0,
        depth: 0,
    };
    reader.string().ok().map(|(_raw, text)| text)
}

/// Escapes a string into its JSON source form, quotes included.
///
/// The same escape set the codec's writer uses, so a value this crate builds
/// and a value the codec builds are spelled identically.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len().saturating_add(2));
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if u32::from(other) < 0x20 => {
                out.push_str("\\u");
                for shift in [12u32, 8, 4, 0] {
                    let nibble = u32::from(other).wrapping_shr(shift) & 0xF;
                    out.push(char::from_digit(nibble, 16).unwrap_or('0'));
                }
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::{Anchor, CommentKind, Document};
    use pharmakos_proto::json::Json;

    fn round_trip(text: &str) {
        let document = Document::parse(text).expect("a document the reader accepts");
        assert_eq!(document.to_text(), text, "the round trip lost a byte");
    }

    #[test]
    fn whitespace_and_comments_survive_in_every_slot() {
        round_trip(
            "// header\n{ /* a */ \"a\" /* b */ : /* c */ 1 /* d */ , \"b\" : [ /* e */ ] }\n// tail\n",
        );
    }

    #[test]
    fn the_empty_containers_keep_their_insides() {
        round_trip("{\"a\":{ /* nothing yet */ },\"b\":[\n  // soon\n]}");
    }

    #[test]
    fn escapes_and_unicode_come_back_as_written() {
        round_trip(r#"{"a":"\u00e9\n\t\"\\","b":"\ud83d\ude00"}"#);
        let document = Document::parse(r#"{"a":"\ud83d\ude00"}"#).expect("a surrogate pair");
        assert_eq!(
            document.to_json().get("a"),
            Some(&Json::String("\u{1F600}".to_owned()))
        );
    }

    #[test]
    fn numbers_keep_their_lexeme() {
        round_trip("{\"a\":-0,\"b\":1e10,\"c\":1.50,\"d\":120000}");
    }

    #[test]
    fn comments_carry_the_pointer_they_were_written_against() {
        let document = Document::parse("{\n  // why\n  \"a\": 1, // trailing\n  \"b\": {}\n}")
            .expect("a document");
        let comments = document.comments();
        assert_eq!(comments.len(), 2);
        assert_eq!(
            comments.first().map(|c| c.anchor.clone()),
            Some(Anchor::Before("/a".to_owned()))
        );
        assert_eq!(comments.first().map(|c| c.kind), Some(CommentKind::Line));
        assert_eq!(
            comments.get(1).map(|c| c.anchor.clone()),
            Some(Anchor::After("/a".to_owned()))
        );
    }

    #[test]
    fn a_trailing_comma_is_refused() {
        let error = Document::parse("{\"a\":1,}").expect_err("a trailing comma");
        assert!(error.is_syntax());
        assert!(error.message.contains("trailing comma"));
    }

    #[test]
    fn a_duplicate_key_is_refused_at_its_own_byte() {
        let error = Document::parse("{\"a\":1,\"a\":2}").expect_err("a duplicate key");
        assert_eq!(error.pointer, "/byte/7");
    }

    #[test]
    fn an_unclosed_block_comment_is_refused() {
        assert!(Document::parse("{} /* forever").is_err());
    }

    #[test]
    fn a_keys_source_spelling_survives_a_take_and_a_put_back() {
        let mut document = Document::parse(r#"{"A":1,"b":2}"#).expect("a document");
        let fragment = document
            .root_mut()
            .take_member("A")
            .expect("the escaped key is `A`");
        document
            .root_mut()
            .put_member("A", fragment, 0)
            .expect("putting it back");
        assert_eq!(document.to_text(), r#"{"A":1,"b":2}"#);
    }

    #[test]
    fn a_fragment_put_back_under_a_different_name_takes_the_canonical_spelling() {
        let mut document = Document::parse(r#"{"A":1}"#).expect("a document");
        let fragment = document.root_mut().take_member("A").expect("the key `A`");
        document
            .root_mut()
            .put_member("B", fragment, 0)
            .expect("putting it back under another name");
        assert_eq!(document.to_text(), r#"{"B":1}"#);
    }

    #[test]
    fn a_node_can_be_built_from_a_json_value_and_read_back() {
        let value = Json::Object(vec![
            (
                "a".to_owned(),
                Json::Array(vec![Json::Number("1".to_owned())]),
            ),
            ("b\"".to_owned(), Json::Bool(true)),
        ]);
        let document = Document::from_json(&value);
        assert_eq!(document.to_json(), value);
        assert_eq!(document.to_text(), r#"{"a":[1],"b\"":true}"#);
    }
}
