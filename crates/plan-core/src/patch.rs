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
//! # An undo is exact, by its type and on the wire
//!
//! A forward patch is RFC 6902: its pointers name a playbook field by either
//! of its names and address the member the file writes, under the file's
//! spelling (`as_the_file_spells`; S3's `vfyq2`). Its inverse is not: it is a
//! patch of [`Undo`] operations, a type of its own, each naming the member it
//! acts on exactly as the file spells it now, and putting back exactly what
//! was there — the value with its inner comments, the member's key spelling,
//! the trivia around it and its slot in its object or array (S3's `vfyq2`,
//! fix pass 2). An undo is never translated by spelling and never refused as
//! writing a field twice: it restores what the file held.
//!
//! The editor holds the inverse as text ([`Patch::to_text`]) and sends it back
//! to undo, so the exactness is spelled in the text by one documented member
//! of each operation, `undo`:
//!
//! * `{"op":"add","path":P,"undo":{"at":N,"jsonc":T}}` puts back at slot `N`
//!   the member or element `T` writes, as it was written (a member: its
//!   leading trivia, its key, the colon, its value and its trailing trivia);
//! * `{"op":"remove","path":P,"undo":{}}` takes out what the forward patch
//!   put in;
//! * `{"op":"replace","path":P,"undo":{"jsonc":T}}` puts back the value `T`
//!   writes, its inner comments included.
//!
//! [`Patch::from_json`] reads every operation strictly: a member its operation
//! does not have is refused, in an undo too ([`Malformed::UnknownMember`]); an
//! undo's `jsonc` is read by the JSONC reader and must be exactly one member,
//! element or value ([`Malformed::UndoText`]); and a patch is all forward or
//! all undo ([`Malformed::MixedUndo`]). That is stricter than RFC 6902's
//! "ignore what you do not know", on purpose: a member the editor did not mean
//! is a mistake to report, not to drop.
//!
//! # What is supported
//!
//! All six operations: `add`, `remove`, `replace`, `move`, `copy` and `test`.
//! `move` and `copy` are expanded into the pair they are defined as, which is
//! also how their inverse is expressed; a `move` onto itself changes nothing
//! (RFC 6902 section 4.4).
//!
//! The patch is applied to the **document**, not to the schema. A patch that
//! produces something that is no longer a playbook is caught by the verifier,
//! with a code and a pointer, exactly as a hand-edit would be — the editor
//! runs no validation of its own (spec section 12).
//! One exception, because it would leave a file that no longer decodes: a
//! forward patch that writes a field under both its names in one object is
//! refused ([`apply`]; S3's `gwu`).

use pharmakos_proto::json::{Json, read, write};

use pharmakos_proto::json::grammar::{MAX_DEPTH, may_nest};
use pharmakos_verifier::MAX_PLAYBOOK_CHARS;

use crate::error::{Bound, Error, Malformed, parent_of, push_pointer, split_pointer};
use crate::jsonc::{Document, Fragment, Index, Missing, Node, rfc6901_index};

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

    /// The members an operation of this kind writes, `op` and `path` included.
    const fn members(self) -> &'static [&'static str] {
        match self {
            Self::Add | Self::Replace | Self::Test => &["op", "path", "value"],
            Self::Remove => &["op", "path"],
            Self::Move | Self::Copy => &["op", "path", "from"],
        }
    }
}

/// One forward patch operation, RFC 6902's.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Operation {
    /// Which operation.
    pub op: Op,
    /// The RFC 6901 pointer it acts on.
    pub path: String,
    /// The source pointer, for `move` and `copy`.
    pub from: Option<String>,
    /// The value, for `add`, `replace` and `test`.
    pub value: Option<Fragment>,
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
        }
    }

    /// An operation with no value: `remove`.
    #[must_use]
    pub fn plain(op: Op, path: impl Into<String>) -> Self {
        Self {
            op,
            path: path.into(),
            from: None,
            value: None,
        }
    }
}

/// One operation of an undo: what it does, at a pointer that names a member
/// exactly as the file spells it (S3's `vfyq2`, fix pass 2).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Undo {
    /// The member or element acted on, exactly.
    pub path: String,
    /// What is done there.
    pub step: UndoStep,
}

/// What an [`Undo`] does.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum UndoStep {
    /// Put back, at slot `at` of the object or array, the member or element
    /// `written` writes, exactly as it was written.
    Put {
        /// The member or element, as written ([`Fragment::written`]).
        written: String,
        /// Its slot.
        at: usize,
    },
    /// Take out what the forward operation put in.
    Take,
    /// Put back the value `written` writes, its inner trivia included
    /// ([`Node::written`]).
    Swap {
        /// The value, as written.
        written: String,
    },
}

/// A sequence of operations, applied in order: all forward, or all undo.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Patch {
    steps: Steps,
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum Steps {
    Forward(Vec<Operation>),
    Undo(Vec<Undo>),
}

impl Default for Patch {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl Patch {
    /// A forward patch from its operations.
    #[must_use]
    pub const fn new(operations: Vec<Operation>) -> Self {
        Self {
            steps: Steps::Forward(operations),
        }
    }

    /// An undo patch from its operations.
    #[must_use]
    pub const fn undo(steps: Vec<Undo>) -> Self {
        Self {
            steps: Steps::Undo(steps),
        }
    }

    /// The forward operations, in order; none for an undo.
    #[must_use]
    pub fn operations(&self) -> &[Operation] {
        match &self.steps {
            Steps::Forward(operations) => operations,
            Steps::Undo(_) => &[],
        }
    }

    /// The undo operations, in order; none for a forward patch.
    #[must_use]
    pub fn undos(&self) -> &[Undo] {
        match &self.steps {
            Steps::Forward(_) => &[],
            Steps::Undo(undos) => undos,
        }
    }

    /// How many operations there are.
    #[must_use]
    pub fn len(&self) -> usize {
        match &self.steps {
            Steps::Forward(operations) => operations.len(),
            Steps::Undo(undos) => undos.len(),
        }
    }

    /// True when there is nothing to do.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Reads a patch from its JSON text: RFC 6902, or an undo this module
    /// wrote (the module docs).
    ///
    /// # Errors
    ///
    /// When the text is not a JSON array of operation objects, when an `op` is
    /// not one of the six, when a required member is missing, when an
    /// operation carries a member it does not have, when an undo is
    /// malformed, when forward and undo operations are mixed, and when a
    /// pointer is not one ([`split_pointer`]).
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
        // Counted before anything is read: an undo patch (its first
        // operation carries `undo`) against its own bound, a forward one
        // against the forward cap (fix pass 5, MEDIUM 1).
        let undoing = items.first().is_some_and(|item| item.get("undo").is_some());
        refuse_count(items.len(), undoing)?;
        let mut forward: Vec<Operation> = Vec::new();
        let mut undo: Vec<Undo> = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let at = format!("/{index}");
            let Json::Object(members) = item else {
                return Err(Error::at(at, "an operation is an object"));
            };
            let name = match item.get("op") {
                Some(Json::String(name)) => name.clone(),
                _ => return Err(Error::at(at, "an operation needs a string `op`")),
            };
            let op = Op::from_name(&name).ok_or_else(|| {
                Error::at(
                    &at,
                    pharmakos_proto::said!(
                        "`{}` is not an RFC 6902 operation",
                        pharmakos_proto::json::grammar::quoted(&name)
                    ),
                )
            })?;
            let path = match item.get("path") {
                Some(Json::String(path)) => path.clone(),
                _ => return Err(Error::at(at, "an operation needs a string `path`")),
            };
            split_pointer(&path).map_err(|error| at_operation(error, &at))?;
            if let Some(written) = item.get("undo") {
                only_members(members, &["op", "path", "undo"], &at)?;
                undo.push(Undo {
                    path,
                    step: read_undo(op, written, &at)?,
                });
                continue;
            }
            only_members(members, op.members(), &at)?;
            let from = match item.get("from") {
                Some(Json::String(from)) => {
                    split_pointer(from).map_err(|error| at_operation(error, &at))?;
                    Some(from.clone())
                }
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
            forward.push(Operation {
                op,
                path,
                from,
                value,
            });
        }
        match (forward.is_empty(), undo.is_empty()) {
            (_, true) => {
                refuse_count(forward.len(), false)?;
                Ok(Self::new(forward))
            }
            (true, false) => Ok(Self::undo(undo)),
            (false, false) => Err(Error::malformed(
                Malformed::MixedUndo,
                "",
                "a patch is forward operations or the undo of one patch, never both",
            )),
        }
    }

    /// The patch as a JSON value: RFC 6902 for a forward patch, and the undo
    /// form of the module docs for an undo.
    #[must_use]
    pub fn to_json(&self) -> Json {
        match &self.steps {
            Steps::Forward(operations) => Json::Array(
                operations
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
            ),
            Steps::Undo(undos) => Json::Array(undos.iter().map(undo_json).collect()),
        }
    }

    /// The patch as text, which is what the gateway exchanges.
    #[must_use]
    pub fn to_text(&self) -> String {
        write(&self.to_json())
    }
}

/// One undo, as the wire writes it.
fn undo_json(undo: &Undo) -> Json {
    let (op, written) = match &undo.step {
        UndoStep::Put { written, at } => (
            Op::Add,
            vec![
                ("at".to_owned(), Json::Number(at.to_string())),
                ("jsonc".to_owned(), Json::String(written.clone())),
            ],
        ),
        UndoStep::Take => (Op::Remove, Vec::new()),
        UndoStep::Swap { written } => (
            Op::Replace,
            vec![("jsonc".to_owned(), Json::String(written.clone()))],
        ),
    };
    Json::Object(vec![
        ("op".to_owned(), Json::String(op.name().to_owned())),
        ("path".to_owned(), Json::String(undo.path.clone())),
        ("undo".to_owned(), Json::Object(written)),
    ])
}

/// An undo's step, read strictly from its `undo` member.
fn read_undo(op: Op, written: &Json, at: &str) -> Result<UndoStep, Error> {
    let Json::Object(members) = written else {
        return Err(Error::malformed(
            Malformed::UndoText,
            at,
            "an operation's `undo` is an object",
        ));
    };
    let text = || match written.get("jsonc") {
        Some(Json::String(text)) => Ok(text.clone()),
        _ => Err(Error::malformed(
            Malformed::UndoText,
            at,
            "an undo that puts something back carries it as `jsonc` text",
        )),
    };
    match op {
        Op::Add => {
            only_members(members, &["at", "jsonc"], at)?;
            let bad_slot = || {
                Error::malformed(
                    Malformed::UndoText,
                    at,
                    "an undo's `at` is the slot it puts back into, a whole number",
                )
            };
            let slot = match written.get("at") {
                Some(Json::Number(lexeme)) => lexeme.parse::<usize>().map_err(|_| bad_slot()),
                _ => Err(bad_slot()),
            }?;
            Ok(UndoStep::Put {
                written: text()?,
                at: slot,
            })
        }
        Op::Remove => {
            only_members(members, &[], at)?;
            Ok(UndoStep::Take)
        }
        Op::Replace => {
            only_members(members, &["jsonc"], at)?;
            Ok(UndoStep::Swap { written: text()? })
        }
        Op::Move | Op::Copy | Op::Test => Err(Error::malformed(
            Malformed::UndoText,
            at,
            "an undo puts back (`add`), takes out (`remove`) or swaps back (`replace`)",
        )),
    }
}

/// Refuses any member of an operation, or of its `undo`, not in `allowed`.
fn only_members(members: &[(String, Json)], allowed: &[&str], at: &str) -> Result<(), Error> {
    match members
        .iter()
        .find(|(key, _)| !allowed.contains(&key.as_str()))
    {
        None => Ok(()),
        Some((key, _)) if allowed.is_empty() => Err(Error::malformed(
            Malformed::UnknownMember,
            at,
            pharmakos_proto::said!(
                "`{}` is not a member of this undo; an undo that takes out has no members",
                pharmakos_proto::json::grammar::quoted(key)
            ),
        )),
        Some((key, _)) => Err(Error::malformed(
            Malformed::UnknownMember,
            at,
            pharmakos_proto::said!(
                "`{}` is not a member of this operation; it has {}",
                pharmakos_proto::json::grammar::quoted(key),
                pharmakos_proto::json::grammar::quoted(&allowed.join(", "))
            ),
        )),
    }
}

/// A pointer's refusal, placed at the operation that carried it.
fn at_operation(error: Error, at: &str) -> Error {
    Error {
        pointer: at.to_owned(),
        ..error
    }
}

/// The most operations one patch may carry (S3's `vfyq2`, fix pass 1).
///
/// The largest patch anything in the game composes is `instantiate`'s: one
/// operation per parameter a template declares (four, in `library/` today)
/// and per parameter the caller names, and two more. The editor composes one
/// or two per click, and every verifier Fix is one. Sixty-four leaves room
/// for templates many times richer; past it a patch is refused before any
/// operation is applied ([`crate::error::Bound::Operations`]).
pub const MAX_PATCH_OPERATIONS: usize = 64;

/// The most undos one forward operation yields: a `move` yields two (the
/// put at its `path` and the put back at its `from`), every other operation
/// one or none. It is [`Undos`]'s widest variant, so the type that carries
/// one operation's undos holds the bound (S3's `vfyq2`, fix pass 5, MEDIUM 1).
pub const MAX_UNDOS_PER_OPERATION: usize = Undos::MOST;

/// The most undo operations one undo patch may carry: the most a forward
/// patch the door accepts can yield, so the inverse of every accepted
/// forward patch is itself accepted (fix pass 5, MEDIUM 1;
/// [`crate::error::Bound::UndoOperations`]). The inverse of an undo patch has
/// one undo per undo, so the bound closes.
pub const MAX_UNDO_OPERATIONS: usize = MAX_UNDOS_PER_OPERATION * MAX_PATCH_OPERATIONS;

/// The most characters the JSON writer spends on one character of a string:
/// a C0 control written `\u00XX`. Every other character is one or two.
const WRITER_WORST_ESCAPE: usize = 6;

/// The most characters one undo adds to an inverse's text around its path
/// and its text: the writer's indented `{"op": "replace", "path": "",
/// "undo": {"at": , "jsonc": ""}}`, the comma between two, and an `at` of
/// twenty digits, a `usize`'s widest (114 measured, by the test below).
const UNDO_SYNTAX_CHARS: usize = 128;

// The inverse of every accepted patch fits the patch text's cap: an inverse
// holds at most `MAX_PLAYBOOK_CHARS` characters of paths and text (the
// `Bound::Inverse` refusal), each written in at most `WRITER_WORST_ESCAPE`,
// around at most `MAX_UNDO_OPERATIONS` undos' syntax.
const _: () = assert!(
    MAX_PLAYBOOK_CHARS * WRITER_WORST_ESCAPE + MAX_UNDO_OPERATIONS * UNDO_SYNTAX_CHARS
        <= MAX_PATCH_TEXT_CHARS
);

/// The longest patch text `patch_text` reads, in characters, refused before
/// it is parsed: **a bound on the work one call does** (S3's `vfyq2`, fix
/// pass 3, item L4, and fix pass 4, item 3).
///
/// It is not a bound on what a legal patch can be: a patch may repeat a
/// value (sixty-four `test`s of one long member, or `add`s and `remove`s of
/// it), and those are refused by this bound or by [`MAX_PATCH_OPERATIONS`],
/// each its own `Bound`, so a refusal says which limit it met. It is sized so
/// that one operation whose value and path each fit the playbook's
/// [`MAX_PLAYBOOK_CHARS`] characters is never refused for its escapes, at
/// the worst escape there is: an astral character written as a surrogate
/// pair, `\uD83D\uDE00`, twelve characters for one. So twelve characters a
/// character for the value, twelve for the path, and the syntax of every
/// operation a patch may carry.
pub const MAX_PATCH_TEXT_CHARS: usize =
    MAX_PLAYBOOK_CHARS * WORST_ESCAPE * 2 + MAX_PATCH_OPERATIONS * PATCH_SYNTAX_CHARS;

/// The most characters JSON text writes for one character: a surrogate
/// pair's two `\uXXXX` escapes.
const WORST_ESCAPE: usize = 12;

/// The syntax one operation writes around its path and value:
/// `{"op":"replace","path":"","undo":{"at":,"jsonc":""}},` and an index.
const PATCH_SYNTAX_CHARS: usize = 64;

/// Refuses a patch of more operations than its kind may carry: an undo
/// patch [`MAX_UNDO_OPERATIONS`], a forward one [`MAX_PATCH_OPERATIONS`].
fn refuse_count(found: usize, undoing: bool) -> Result<(), Error> {
    if undoing {
        if found > MAX_UNDO_OPERATIONS {
            return Err(Error::bounded(
                Bound::UndoOperations,
                "",
                pharmakos_proto::said!(
                    "this undo carries {} operations, and an undo carries at most {}, the most \
                     a patch of {} operations yields",
                    found,
                    MAX_UNDO_OPERATIONS,
                    MAX_PATCH_OPERATIONS
                ),
            ));
        }
    } else if found > MAX_PATCH_OPERATIONS {
        return Err(too_many(found));
    }
    Ok(())
}

fn too_many(found: usize) -> Error {
    Error::bounded(
        Bound::Operations,
        "",
        pharmakos_proto::said!(
            "this patch carries {} operations, and a patch carries at most {}",
            found,
            MAX_PATCH_OPERATIONS
        ),
    )
}

/// Applies a patch to a document and returns the patch that undoes it.
///
/// The inverse of a forward patch is an undo ([`Undo`]), the reverse-ordered
/// undo of each operation, so applying it puts the file back exactly —
/// comments, spacing, key spelling and member order included. The inverse of
/// an undo is again an undo.
///
/// # Errors
///
/// When a path does not resolve, when `test` does not hold, or when an
/// operation asks for something the document's shape cannot do; at the
/// bounds ([`Bound`]); and when a forward patch would write one playbook
/// field under both its names, its `.proto` name and its camelCase JSON name,
/// in one object where the file did not ([`WrittenTwice`]; S3's `gwu`, fix
/// pass 1): the decoder refuses such a file, so a patch that made one would
/// hand back a file that no longer decodes.
pub fn apply(document: &Document, patch: &Patch) -> Result<(Document, Patch), Error> {
    let (patched, inverse) = apply_once(document, patch)?;
    // The inverse is handed back only when the door takes it (fix pass 5,
    // MEDIUM 1): applied here, under every bound the door holds it to, it
    // must give the document back exactly. Otherwise this patch is refused
    // up front, rather than applied with an undo that would not work. The
    // inverse of the inverse is this patch's effect again, whose bounds this
    // call has already met, so the closure holds both ways.
    let undone = apply_once(&patched, &inverse).map_err(|error| {
        Error::bounded(
            Bound::Inverse,
            "",
            pharmakos_proto::said!(
                "this patch is not applied: the patch that would undo it could not be issued \
                 ({})",
                error.said()
            ),
        )
    })?;
    // The same tree writes the same bytes, so the text is compared only
    // when the trees differ (trivia can sit on either side of a node and
    // write the same bytes).
    if undone.0 != *document && undone.0.to_text() != document.to_text() {
        return Err(Error::bounded(
            Bound::Inverse,
            "",
            "this patch is not applied: the patch that would undo it does not give the file \
             back as it was",
        ));
    }
    Ok((patched, inverse))
}

/// [`apply`]'s one pass: the patch applied and its inverse made, under every
/// bound.
fn apply_once(document: &Document, patch: &Patch) -> Result<(Document, Patch), Error> {
    refuse_count(patch.len(), matches!(patch.steps, Steps::Undo(_)))?;
    let mut working = document.clone();
    let mut inverse: Vec<Undo> = Vec::new();
    let mut inverse_chars: usize = 0;
    let mut budget = |working: &Document, path: &str, next: &[Undo]| -> Result<(), Error> {
        let size = working.measure();
        if size.chars > MAX_PLAYBOOK_CHARS {
            return Err(too_long(path));
        }
        if !may_nest(size.depth) {
            return Err(Error::bounded(
                Bound::Depth,
                path,
                pharmakos_proto::said!(
                    "this patch would nest a value inside {} containers, past the {} a \
                     playbook may hold",
                    size.depth,
                    MAX_DEPTH
                ),
            ));
        }
        // Checked, and a sum past `usize` is past the cap: the same refusal.
        let mut held = Some(inverse_chars);
        for undo in next {
            held = held.and_then(|sum| sum.checked_add(undo.path.chars().count()));
            if let UndoStep::Put { written, .. } | UndoStep::Swap { written } = &undo.step {
                held = held.and_then(|sum| sum.checked_add(written.chars().count()));
            }
        }
        let within = held.filter(|sum| *sum <= MAX_PLAYBOOK_CHARS);
        if let Some(sum) = within {
            inverse_chars = sum;
        } else {
            return Err(Error::bounded(
                Bound::Inverse,
                path,
                pharmakos_proto::said!(
                    "the patch that undoes this one would run past the playbook's {} \
                     characters",
                    MAX_PLAYBOOK_CHARS
                ),
            ));
        }
        Ok(())
    };
    match &patch.steps {
        Steps::Undo(undos) => {
            // Every undo refusal points at its operation in the patch,
            // `/<index>`, the space `Patch::from_json` reads it in, and
            // carries a `Malformed` code (fix pass 3, item L5); a bound's
            // refusal keeps its bound.
            for (index, undo) in undos.iter().enumerate() {
                let operation = format!("/{index}");
                let mut next = vec![exact(&mut working, undo).map_err(|error| {
                    let coded = if error.malformation().is_none() && error.bound().is_none() {
                        Error::malformed(Malformed::UndoTarget, "", error.said())
                    } else {
                        error
                    };
                    at_operation(coded, &operation)
                })?];
                budget(&working, &undo.path, &next)
                    .map_err(|error| at_operation(error, &operation))?;
                next.append(&mut inverse);
                inverse = next;
            }
            Ok((working, Patch::undo(inverse)))
        }
        Steps::Forward(operations) => {
            for operation in operations {
                // A `copy` doubles what it copies: refused before it copies
                // anything that would take the document past the cap.
                if operation.op == Op::Copy {
                    let from = as_the_file_spells(&working, from_of(operation)?)?;
                    let copied = resolve(&working, &from)?.measure().chars;
                    let now = working.measure().chars;
                    if now
                        .checked_add(copied)
                        .is_none_or(|sum| sum > MAX_PLAYBOOK_CHARS)
                    {
                        return Err(too_long(&operation.path));
                    }
                }
                // The last operation is undone first, so each operation's undo
                // goes in front of everything accumulated so far.
                let mut next = one(&mut working, operation)?.into_vec();
                budget(&working, &operation.path, &next)?;
                next.append(&mut inverse);
                inverse = next;
            }
            refuse_written_twice(document, &working)?;
            Ok((working, Patch::undo(inverse)))
        }
    }
}

/// Refuses a forward patch whose result writes a field under both its names
/// in more objects than the original did.
fn refuse_written_twice(original: &Document, working: &Document) -> Result<(), Error> {
    // One walk for almost every patch: the original is read only when the
    // patched file names a field twice somewhere (S3's `gwu`, fix pass 2: two
    // walks cost every `patch_plan` about 60% more at the text cap).
    let after = written_twice(&working.to_json());
    if after.is_empty() {
        return Ok(());
    }
    let original = original.to_json();
    let before = written_twice(&original);
    // What is new is told by the field and its two spellings, and by how many
    // objects write them so, never by an object's pointer, which a patch that
    // adds or removes an array item before it moves.
    let same = |one: &WrittenTwice, other: &WrittenTwice| {
        one.field == other.field
            && ((one.written == other.written && one.added == other.added)
                || (one.written == other.added && one.added == other.written))
    };
    let new = after.iter().find(|found| {
        let now = after.iter().filter(|other| same(found, other)).count();
        let then = before.iter().filter(|other| same(found, other)).count();
        now > then
    });
    let Some(twice) = new else {
        return Ok(());
    };
    // The file already wrote the first spelling where the patch put the
    // second; otherwise the patch brought both.
    let already = matches!(
        json_at(&original, &twice.pointer),
        Some(Json::Object(members))
            if members.iter().any(|(key, _)| *key == twice.written)
                && !members.iter().any(|(key, _)| *key == twice.added)
    );
    let quoted = pharmakos_proto::json::grammar::quoted;
    let advice = if already {
        pharmakos_proto::said!(
            "edit the member the file already writes, `{}`",
            quoted(&twice.written)
        )
    } else {
        pharmakos_proto::said!("the patch itself writes both; write the field once")
    };
    Err(Error::at(
        twice.pointer.clone(),
        pharmakos_proto::said!(
            "this patch would write the field `{}` twice in one object, as `{}` and as `{}`, \
             and a playbook writes each field once, under one of its names; {}",
            quoted(&twice.field),
            quoted(&twice.written),
            quoted(&twice.added),
            advice
        ),
    ))
}

/// The refusal of a patch that would take the document past the playbook's
/// cap, at the operation that would.
fn too_long(path: &str) -> Error {
    Error::bounded(
        Bound::Size,
        path,
        pharmakos_proto::said!(
            "this patch would make the playbook longer than its {} characters",
            MAX_PLAYBOOK_CHARS
        ),
    )
}

/// The value at an RFC 6901 pointer in plain JSON: `None` where nothing is
/// (a pointer the walk wrote, so it always splits; an index that is not one
/// names nothing).
fn json_at<'a>(value: &'a Json, pointer: &str) -> Option<&'a Json> {
    let Ok(tokens) = split_pointer(pointer) else {
        return None;
    };
    let mut here = value;
    for token in tokens {
        here = match here {
            Json::Object(members) => members
                .iter()
                .find(|(key, _)| *key == token)
                .map(|(_, inner)| inner)?,
            Json::Array(items) => match token.parse::<usize>() {
                Ok(index) => items.get(index)?,
                Err(_) => return None,
            },
            _ => return None,
        };
    }
    Some(here)
}

/// One playbook field written under both its names in one object.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WrittenTwice {
    /// The object's JSON Pointer.
    pub pointer: String,
    /// The field's `.proto` name.
    pub field: String,
    /// The spelling met first in the object.
    pub written: String,
    /// The other spelling.
    pub added: String,
}

/// Every object of a playbook's JSON that names one field under both its
/// names, read against the schema from `gp.v1.Playbook` down: each member is
/// matched to its field by either name, and each message-typed member is
/// walked in turn. A member the schema does not declare is not walked (the
/// decoder refuses it on its own terms).
fn written_twice(value: &Json) -> Vec<WrittenTwice> {
    let mut found = Vec::new();
    walk_twice(value, "gp.v1.Playbook", "", &mut found);
    found
}

fn walk_twice(value: &Json, message: &str, pointer: &str, found: &mut Vec<WrittenTwice>) {
    let Json::Object(members) = value else {
        return;
    };
    let Some(declared) = pharmakos_proto::descriptor::schema().message(message) else {
        return;
    };
    let mut named: Vec<(i32, &str)> = Vec::new();
    for (key, inner) in members {
        let Some(field) = declared
            .fields
            .iter()
            .find(|field| field.name == *key || field.json_name == *key)
        else {
            continue;
        };
        if let Some((_, first)) = named.iter().find(|(number, _)| *number == field.number) {
            if *first != key.as_str() {
                found.push(WrittenTwice {
                    pointer: pointer.to_owned(),
                    field: field.name.clone(),
                    written: (*first).to_owned(),
                    added: key.clone(),
                });
            }
        } else {
            named.push((field.number, key.as_str()));
        }
        if field.kind != pharmakos_proto::descriptor::ScalarKind::Message {
            continue;
        }
        let here = push_pointer(pointer, key);
        match inner {
            Json::Array(items) if field.repeated => {
                for (index, item) in items.iter().enumerate() {
                    walk_twice(
                        item,
                        &field.type_name,
                        &push_pointer(&here, &index.to_string()),
                        found,
                    );
                }
            }
            _ => walk_twice(inner, &field.type_name, &here, found),
        }
    }
}

/// Applies a patch given as text and returns the patched file and the
/// inverse patch's text.
///
/// This is `patch_plan` (spec section 12): the file comes back with its
/// comments and formatting, and the inverse comes back as text for the undo
/// stack, which gives the file back byte for byte (the module docs).
///
/// # Errors
///
/// As [`apply`], plus a syntax error in either text.
pub fn patch_text(playbook_jsonc: &str, json_patch: &str) -> Result<(String, String), Error> {
    // The patch's length first, before a byte of it is parsed (fix pass 3,
    // item L4): its bytes bound its characters, and only a text whose bytes
    // pass the cap is counted.
    if json_patch.len() > MAX_PATCH_TEXT_CHARS && json_patch.chars().count() > MAX_PATCH_TEXT_CHARS
    {
        return Err(Error::bounded(
            Bound::PatchText,
            "",
            pharmakos_proto::said!(
                "a patch is at most {} characters: a bound on the work one call does, with room \
                 for one operation whose value and path each fit a playbook's {} characters at \
                 the worst escape",
                MAX_PATCH_TEXT_CHARS,
                MAX_PLAYBOOK_CHARS
            ),
        ));
    }
    let document = Document::parse(playbook_jsonc)?;
    let patch = Patch::from_text(json_patch)?;
    let (patched, inverse) = apply(&document, &patch)?;
    Ok((patched.to_text(), inverse.to_text()))
}

/// `pointer`, each token that names a playbook field spelled as the file
/// already writes that field (S3's `vfyq2`, fix pass 1, item 9).
///
/// A playbook field has two names, its `.proto` name and its camelCase JSON
/// name, and a file may write either. A pointer from the verifier, a Fix or
/// the editor names the field by its `.proto` name; in a file that writes the
/// member under its JSON name, that pointer addresses the member the file
/// already writes, under the file's own spelling, so `add` replaces it and
/// `remove` takes it, and no edit writes a field twice. The walk reads the
/// document beside the schema from `gp.v1.Playbook` down, token by token,
/// and stops translating where the document ends (a new member keeps the
/// pointer's spelling). A token the object already holds under that exact
/// spelling is left alone, so a file holding both spellings is addressed as
/// written. One pointer buffer, one pass (fix pass 2, item A2).
///
/// # Errors
///
/// As [`split_pointer`]; and this build's schema not declaring
/// `gp.v1.Playbook`, which every playbook is read against.
fn as_the_file_spells(document: &Document, pointer: &str) -> Result<String, Error> {
    /// What the walk is reading: an object of this message, an array of
    /// them, or something the schema says nothing more of.
    enum Reading<'s> {
        Message(&'s pharmakos_proto::descriptor::Message),
        Repeated(&'s pharmakos_proto::descriptor::Message),
        Other,
    }
    let schema = pharmakos_proto::descriptor::schema();
    let playbook = schema
        .message(crate::canonical::PLAYBOOK_MESSAGE)
        .ok_or_else(|| {
            Error::at(
                "",
                pharmakos_proto::said!(
                    "this build's schema does not declare `{}`",
                    crate::canonical::PLAYBOOK_MESSAGE
                ),
            )
        })?;
    let mut reading = Reading::Message(playbook);
    let mut here = Some(document.root());
    let mut spelled = String::with_capacity(pointer.len());
    for token in split_pointer(pointer)? {
        let mut token = token;
        let mut next = Reading::Other;
        match reading {
            Reading::Message(message) => {
                if let Some(field) = message
                    .fields
                    .iter()
                    .find(|field| field.name == token || field.json_name == token)
                {
                    if let Some(node) = here
                        && node.position_of(&token).is_none()
                    {
                        let other = if field.name == token {
                            &field.json_name
                        } else {
                            &field.name
                        };
                        if node.position_of(other).is_some() {
                            token.clone_from(other);
                        }
                    }
                    if field.kind == pharmakos_proto::descriptor::ScalarKind::Message
                        && let Some(nested) = schema.message(&field.type_name)
                    {
                        next = if field.repeated {
                            Reading::Repeated(nested)
                        } else {
                            Reading::Message(nested)
                        };
                    }
                }
            }
            Reading::Repeated(message) => next = Reading::Message(message),
            Reading::Other => {}
        }
        // Past where the document ends, nothing is translated: a new member
        // keeps the pointer's spelling, and whatever the pointer's token
        // meets there is the operation's own refusal (`resolve`), in its
        // typed form (fix pass 5, item L1).
        here = match here.map(|node| node.lookup(&token)) {
            Some(Ok(child)) => Some(child),
            Some(Err(
                Missing::NoMember
                | Missing::NotAnIndex
                | Missing::PastTheEnd { .. }
                | Missing::NotAContainer
                | Missing::End,
            ))
            | None => None,
        };
        append_token(&mut spelled, &token);
        reading = next;
    }
    Ok(spelled)
}

/// Appends one escaped RFC 6901 token to a pointer, in place.
fn append_token(pointer: &mut String, token: &str) {
    pointer.push('/');
    for character in token.chars() {
        match character {
            '~' => pointer.push_str("~0"),
            '/' => pointer.push_str("~1"),
            other => pointer.push(other),
        }
    }
}

/// One forward operation. Returns the undos that undo it, **in the order they
/// must be applied**.
fn one(document: &mut Document, operation: &Operation) -> Result<Undos, Error> {
    // Each pointer addresses the member the file writes, under either of a
    // field's names (`as_the_file_spells`); the undos name what was acted on
    // exactly.
    let path = as_the_file_spells(document, &operation.path)?;
    let from = match operation.from.as_deref() {
        Some(from) => Some(as_the_file_spells(document, from)?),
        None => None,
    };
    match operation.op {
        Op::Test => {
            let wanted = value_of(operation)?;
            let found = resolve(document, &path)?;
            if found.to_json() == wanted.to_json() {
                Ok(Undos::None)
            } else {
                Err(Error::at(
                    &path,
                    "the value here is not the one the patch tested for",
                ))
            }
        }
        Op::Remove => {
            let (taken, at) = take(document, &path)?;
            Ok(Undos::One(put_back(path, &taken, at)))
        }
        Op::Add => Ok(Undos::One(add(document, &path, value_of(operation)?)?)),
        Op::Replace => {
            let fragment = value_of(operation)?;
            let target = resolve_mut(document, &path)?;
            let old = target.set(fragment.into_node());
            Ok(Undos::One(Undo {
                path,
                step: UndoStep::Swap {
                    written: old.written(),
                },
            }))
        }
        Op::Move => {
            let from = from.ok_or_else(|| Error::at(&operation.path, "a `move` needs a `from`"))?;
            // RFC 6902 section 4.4: a move to where it already is changes
            // nothing, and is not a take and a put at the end.
            if from == path {
                resolve(document, &path)?;
                return Ok(Undos::None);
            }
            let (taken, at) = take(document, &from)?;
            // The trivia goes with the member, so the undo carries the same
            // fragment, as written: otherwise moving a commented member and
            // undoing it would lose the comment.
            let restore = put_back(from, &taken, at);
            let put = add(document, &path, taken)?;
            // Undone last first: the put taken out, then the member put back.
            Ok(Undos::Two(put, restore))
        }
        Op::Copy => {
            let from = from.ok_or_else(|| Error::at(&operation.path, "a `copy` needs a `from`"))?;
            let copied = resolve(document, &from)?.clone();
            Ok(Undos::One(add(
                document,
                &path,
                Fragment::from_node(copied),
            )?))
        }
    }
}

/// One forward operation's undos, in the order they are applied: none, one,
/// or two (a `move`'s). Its widest variant is [`MAX_UNDOS_PER_OPERATION`].
enum Undos {
    None,
    One(Undo),
    Two(Undo, Undo),
}

impl Undos {
    /// The most undos one variant holds.
    const MOST: usize = 2;

    fn into_vec(self) -> Vec<Undo> {
        match self {
            Undos::None => Vec::new(),
            Undos::One(undo) => vec![undo],
            Undos::Two(first, second) => vec![first, second],
        }
    }
}

/// The undo that puts `taken` back at slot `at` of what `path` names.
fn put_back(path: String, taken: &Fragment, at: usize) -> Undo {
    Undo {
        path,
        step: UndoStep::Put {
            written: taken.written(),
            at,
        },
    }
}

/// One undo, applied exactly: its pointer names the member as the file
/// spells it, and nothing is translated. Returns the undo that undoes it.
fn exact(document: &mut Document, undo: &Undo) -> Result<Undo, Error> {
    let path = undo.path.clone();
    match &undo.step {
        UndoStep::Take => {
            let (taken, at) = take(document, &path)?;
            Ok(put_back(path, &taken, at))
        }
        UndoStep::Swap { written } => {
            let node = Node::from_written(written).map_err(|error| at_operation(error, &path))?;
            let target = resolve_mut(document, &path)?;
            let old = target.set(node);
            Ok(Undo {
                path,
                step: UndoStep::Swap {
                    written: old.written(),
                },
            })
        }
        UndoStep::Put { written, at } => {
            let (parent, last) = parent_of(&path)?;
            let (is_array, len) = shape(document, parent)?;
            if *at > len {
                return Err(Error::malformed(
                    Malformed::UndoSlot,
                    &path,
                    pharmakos_proto::said!(
                        "this undo puts back at slot {}, past the {} there are",
                        *at,
                        len
                    ),
                ));
            }
            // An array undo's path ends in the index it puts back at (fix
            // pass 3, item L2): a path and a slot that disagree are not an
            // inverse anything wrote.
            let slot = if is_array {
                match rfc6901_index(&last) {
                    Err(_) => return Err(not_an_index(&path)),
                    Ok(Index::End) => return Err(end_of_array(&path)),
                    Ok(index) => Some(index),
                }
            } else {
                None
            };
            if slot.is_some_and(|index| index != Index::At(*at)) {
                return Err(Error::malformed(
                    Malformed::UndoSlot,
                    &path,
                    pharmakos_proto::said!(
                        "this undo puts an element back at slot {}, and its path ends in `{}`",
                        *at,
                        pharmakos_proto::json::grammar::quoted(&last)
                    ),
                ));
            }
            let whole = container_undo(document, parent)?;
            let container = resolve_mut(document, parent)?;
            if is_array {
                let fragment = Fragment::item_from_written(written)
                    .map_err(|error| at_operation(error, &path))?;
                container
                    .put_item(*at, fragment)
                    .map_err(|error| at_operation(error, &path))?;
                Ok(put_undo(whole, push_pointer(parent, &at.to_string())))
            } else {
                let fragment = Fragment::member_from_written(written, &last)
                    .map_err(|error| at_operation(error, &path))?;
                container
                    .put_member(&last, fragment, *at)
                    .map_err(|error| at_operation(error, &path))?;
                Ok(put_undo(whole, path))
            }
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

fn resolve<'a>(document: &'a Document, pointer: &str) -> Result<&'a Node, Error> {
    let mut here = document.root();
    for token in split_pointer(pointer)? {
        here = here
            .lookup(&token)
            .map_err(|missing| missing_at(pointer, &token, missing))?;
    }
    Ok(here)
}

fn resolve_mut<'a>(document: &'a mut Document, pointer: &str) -> Result<&'a mut Node, Error> {
    let mut here = document.root_mut();
    for token in split_pointer(pointer)? {
        here = here
            .lookup_mut(&token)
            .map_err(|missing| missing_at(pointer, &token, missing))?;
    }
    Ok(here)
}

/// Why `pointer` names nothing, at the token that met it (S3's `vfyq2`, fix
/// pass 5, item L1): a misspelt index is `Malformed::PointerIndex`, a
/// well-formed one past the end says so, with the token.
fn missing_at(pointer: &str, token: &str, missing: Missing) -> Error {
    let quoted = pharmakos_proto::json::grammar::quoted;
    match missing {
        Missing::NotAnIndex => not_an_index(pointer),
        Missing::End => end_of_array(pointer),
        Missing::PastTheEnd { len } => past_the_end(pointer, token, len),
        Missing::NoMember => Error::at(
            pointer,
            pharmakos_proto::said!(
                "there is nothing at that path: no member `{}`",
                quoted(token)
            ),
        ),
        Missing::NotAContainer => Error::at(
            pointer,
            pharmakos_proto::said!(
                "there is nothing at that path: `{}` is under a value that is neither an \
                 object nor an array",
                quoted(token)
            ),
        ),
    }
}

/// A token RFC 6901 does not spell as an array index.
fn not_an_index(pointer: &str) -> Error {
    Error::malformed(
        Malformed::PointerIndex,
        pointer,
        "an array index is `0` or digits with no leading zero and no sign (RFC 6901)",
    )
}

/// `-`, the end of an array, where the operation may not name it: the one
/// refusal for it (S3's `vfyq2`, fix pass 6, item 4).
fn end_of_array(pointer: &str) -> Error {
    Error::malformed(
        Malformed::EndOfArray,
        pointer,
        "`-` names the end of an array, which only an `add`, or a `move` or `copy`'s \
         destination, may name",
    )
}

/// A well-formed index at or past the end of an array of `len`.
fn past_the_end(pointer: &str, token: &str, len: usize) -> Error {
    Error::at(
        pointer,
        pharmakos_proto::said!(
            "index `{}` is past the end of the array, which holds {}",
            pharmakos_proto::json::grammar::quoted(token),
            len
        ),
    )
}

/// How long the container at `pointer` is, and whether it is an array.
fn shape(document: &Document, pointer: &str) -> Result<(bool, usize), Error> {
    let container = resolve(document, pointer)?;
    let len = container.len().ok_or_else(|| {
        Error::at(
            pointer,
            pharmakos_proto::said!("{} has no members to add to or remove", container.kind()),
        )
    })?;
    Ok((container.kind() == "an array", len))
}

/// Takes the member or element at `pointer` out, trivia and all, and says
/// which slot it came out of so that an undo can put it back there.
fn take(document: &mut Document, pointer: &str) -> Result<(Fragment, usize), Error> {
    let (parent, last) = parent_of(pointer)?;
    let (is_array, len) = shape(document, parent)?;
    let nothing = || Error::at(pointer, "there is nothing at that path");
    let container = resolve_mut(document, parent)?;
    if is_array {
        let index = array_index(&last, len, false, pointer)?;
        Ok((container.take_item(index).ok_or_else(nothing)?, index))
    } else {
        let at = container.position_of(&last).ok_or_else(nothing)?;
        Ok((container.take_member(&last).ok_or_else(nothing)?, at))
    }
}

/// Adds at `pointer`.
///
/// RFC 6902: on an object member that already exists this **replaces** it, so
/// the undo swaps the old value back rather than taking the member out. On an
/// array it inserts, and `-` means "after the last element". A new member
/// goes at the end of its object.
fn add(document: &mut Document, pointer: &str, fragment: Fragment) -> Result<Undo, Error> {
    let (parent, last) = parent_of(pointer)?;
    let (is_array, len) = shape(document, parent)?;
    // The first put into an empty container with trivia inside carries that
    // trivia into what it puts (`Node::put_member`), so taking it out again
    // would not give the bytes back: the undo swaps the whole container back,
    // as it was written (S3's `vfyq2`, fix pass 3, item M1).
    let whole = container_undo(document, parent)?;
    if is_array {
        let index = array_index(&last, len, true, pointer)?;
        let container = resolve_mut(document, parent)?;
        container.put_item(index, fragment)?;
        return Ok(put_undo(whole, push_pointer(parent, &index.to_string())));
    }
    if resolve(document, parent)?.position_of(&last).is_some() {
        // RFC 6902 section 4.1 makes `add` over an existing member a replace,
        // so take the replace route rather than take-then-put: taking the
        // member out carries its `before` comment, its spacing and its key
        // spelling away with it, and the incoming fragment — which came off
        // the wire with no trivia at all — would be written in their place.
        // `set` swaps the value inside the member and leaves the member alone.
        let target = resolve_mut(document, pointer)?;
        let old = target.set(fragment.into_node());
        return Ok(Undo {
            path: pointer.to_owned(),
            step: UndoStep::Swap {
                written: old.written(),
            },
        });
    }
    let container = resolve_mut(document, parent)?;
    container.put_member(&last, fragment, len)?;
    Ok(put_undo(whole, pointer.to_owned()))
}

/// The undo of one put: the container swapped back whole when
/// [`container_undo`] found one, else the put member or element at `path`
/// taken out again.
fn put_undo(whole: Option<Undo>, path: String) -> Undo {
    match whole {
        Some(undo) => undo,
        None => Undo {
            path,
            step: UndoStep::Take,
        },
    }
}

/// The undo of a put into the container at `parent`, when the put changes
/// more than the slot it fills: an empty container with trivia inside, whose
/// trivia the put carries along, is swapped back whole, as it was written.
/// `None` when taking the put member or element out again is exact.
fn container_undo(document: &Document, parent: &str) -> Result<Option<Undo>, Error> {
    let container = resolve(document, parent)?;
    Ok(container.holds_inside_trivia().then(|| Undo {
        path: parent.to_owned(),
        step: UndoStep::Swap {
            written: container.written(),
        },
    }))
}

/// An RFC 6902 array index. `-` means "one past the end", and only `add`
/// accepts it.
fn array_index(token: &str, len: usize, appending: bool, pointer: &str) -> Result<usize, Error> {
    // RFC 6901's spelling and nothing else, by the one reader (fix pass 4,
    // item L-d, fix pass 5, item L1, and fix pass 6, item 4).
    let index = match rfc6901_index(token).map_err(|_| not_an_index(pointer))? {
        Index::At(index) => index,
        Index::Beyond => return Err(past_the_end(pointer, token, len)),
        Index::End if appending => return Ok(len),
        Index::End => return Err(end_of_array(pointer)),
    };
    let past = if appending { index > len } else { index >= len };
    if past {
        return Err(past_the_end(pointer, token, len));
    }
    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::{Patch, apply, patch_text};
    use crate::jsonc::Document;

    /// The two constants the inverse's text bound is built from hold for
    /// the writer and the undo form as they are (fix pass 5, MEDIUM 1).
    #[test]
    fn the_inverse_text_bound_s_constants_hold() {
        use super::{UNDO_SYNTAX_CHARS, Undo, UndoStep, WRITER_WORST_ESCAPE, undo_json};
        use pharmakos_proto::json::{Json, write};
        let mut widest = 0_usize;
        for character in (0..=0x0010_FFFF_u32).filter_map(char::from_u32) {
            let written = write(&Json::String(character.to_string())).chars().count();
            // Less its quotes and the writer's closing newline.
            widest = widest.max(written.saturating_sub(3));
        }
        assert_eq!(widest, WRITER_WORST_ESCAPE);
        for step in [
            UndoStep::Put {
                written: String::new(),
                at: usize::MAX,
            },
            UndoStep::Swap {
                written: String::new(),
            },
            UndoStep::Take,
        ] {
            let undo = Json::Array(vec![undo_json(&Undo {
                path: String::new(),
                step,
            })]);
            // What one more undo adds to an inverse's text, indentation and
            // the comma between two included.
            let one = write(&undo).chars().count();
            let Json::Array(mut items) = undo else {
                unreachable!("built as an array above")
            };
            items.extend(items.clone());
            let two = write(&Json::Array(items)).chars().count();
            let syntax = two.saturating_sub(one);
            assert!(syntax <= UNDO_SYNTAX_CHARS, "{syntax}");
        }
    }

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
        assert_eq!(error.pointer(), "/a");
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
