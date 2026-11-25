// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! One error type for everything this crate can refuse, carrying the same
//! `(pointer, message)` shape the schema codec and the verifier's diagnostics
//! use.
//!
//! The pointer is what makes an error *placeable*: the editor underlines the
//! node it names, and `gamectl verify` prints it beside the line. A syntax
//! error has no place in the document tree yet, so it reports `/byte/NNN`,
//! exactly as `crates/proto`'s reader does and as decisions-log item 96 (2)
//! settles for the verifier's `E0001`.

use std::fmt;

use pharmakos_proto::json::grammar::Said;

/// Something a playbook file, a patch, or the schema it is read against did not
/// allow.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Error {
    /// Where the problem is, as an RFC 6901 JSON Pointer — `""` for the root,
    /// `/declarative/route/0/move` for a step's move, `/byte/412` for a syntax
    /// error that has no place in the tree. Every field is private (S3's
    /// `vfyq2`, fix pass 4b, item L-b): a public one could be written after
    /// the constructor, so a caller reads through the getters.
    pub(crate) pointer: String,

    /// The bound that refused it, when one did: a caller tells a refusal by
    /// its bound, never by reading the message.
    pub(crate) bound: Option<Bound>,
    /// What was malformed in a patch or a pointer, when that is why: a caller
    /// tells it by this, never by reading the message.
    pub(crate) malformed: Option<Malformed>,
    /// The message as it was said: only this crate's constructors, which
    /// take a [`Said`], make an `Error` (the field is the crate's, so no other
    /// crate can write one), so [`Error::said`] is never a caller's raw text
    /// (S3's `vfyq2`, fix pass 3, item M2 (d)).
    pub(crate) said: Said,
}

/// Something a patch or a pointer wrote that its grammar does not have
/// (S3's `vfyq2`, fix pass 2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Malformed {
    /// A `~` in a JSON Pointer not followed by `0` or `1` (RFC 6901 section
    /// 3 has no other escape).
    PointerEscape,
    /// A member of a patch operation that its operation does not have, or a
    /// member of an undo it does not have.
    UnknownMember,
    /// A patch that mixes forward operations and undo operations: an undo is
    /// the whole inverse of one patch, applied as it was handed back.
    MixedUndo,
    /// An undo operation whose written member, item or value is not exactly
    /// one of them, as the JSONC reader reads it.
    UndoText,
    /// An undo whose slot is past the end of what it puts into, or, in an
    /// array, is not the index its path ends in (S3's `vfyq2`, fix pass 3,
    /// items L2 and L5).
    UndoSlot,
    /// An undo that names what the document does not hold, or puts back a
    /// member already there (S3's `vfyq2`, fix pass 3, item L5).
    UndoTarget,
    /// An array index not spelled as RFC 6901 spells one, `0|[1-9][0-9]*`:
    /// a sign, a leading zero, anything but digits (S3's `vfyq2`, fix pass 4,
    /// item L-d).
    PointerIndex,
    /// `-`, the end of an array, where only an `add` (or a `move` or `copy`'s
    /// destination) may name it (S3's `vfyq2`, fix pass 6, item 4).
    EndOfArray,
}

/// A bound on the work one call may do (S3's `vfyq2`, fix pass 1): what a
/// refusal that is not about the playbook's meaning was refused by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bound {
    /// More operations in one patch than [`crate::patch::MAX_PATCH_OPERATIONS`].
    Operations,
    /// A patch's text longer than [`crate::patch::MAX_PATCH_TEXT_CHARS`],
    /// refused before it is read (S3's `vfyq2`, fix pass 3, item L4).
    PatchText,
    /// A JSON Pointer with more tokens than any node of a playbook can have:
    /// `pharmakos_proto::json::grammar::MAX_DEPTH` containers, and one token
    /// past the last of them ([`MAX_POINTER_TOKENS`]).
    PointerTokens,
    /// A patched document longer than the playbook's cap,
    /// `pharmakos_verifier::MAX_PLAYBOOK_CHARS` characters.
    Size,
    /// A patched document nested past the grammar's
    /// `pharmakos_proto::json::grammar::MAX_DEPTH`.
    Depth,
    /// An inverse patch longer than the playbook's cap.
    Inverse,
    /// More undo operations in one patch than
    /// [`crate::patch::MAX_UNDO_OPERATIONS`] (S3's `vfyq2`, fix pass 5).
    UndoOperations,
    /// A library file past the playbook's text cap, refused when it is
    /// read and never read whole ([`crate::library::MAX_FILE_BYTES`]; S3's
    /// `outcap`).
    LibraryFile,
}

impl Error {
    /// An error at a pointer.
    #[must_use]
    pub fn at(pointer: impl Into<String>, message: impl Into<Said>) -> Self {
        let said = message.into();
        Self {
            pointer: pointer.into(),
            bound: None,
            malformed: None,
            said,
        }
    }

    /// A refusal of something malformed in a patch or a pointer.
    #[must_use]
    pub fn malformed(
        malformed: Malformed,
        pointer: impl Into<String>,
        message: impl Into<Said>,
    ) -> Self {
        let said = message.into();
        Self {
            pointer: pointer.into(),
            bound: None,
            malformed: Some(malformed),
            said,
        }
    }

    /// A refusal by a bound on the work one call may do.
    #[must_use]
    pub fn bounded(bound: Bound, pointer: impl Into<String>, message: impl Into<Said>) -> Self {
        let said = message.into();
        Self {
            pointer: pointer.into(),
            bound: Some(bound),
            malformed: None,
            said,
        }
    }

    /// An error at a byte offset, for a document that did not parse.
    #[must_use]
    pub fn at_byte(offset: usize, message: impl Into<Said>) -> Self {
        Self::at(format!("/byte/{offset}"), message)
    }

    /// The message, as text: English, precise, and safe to show a player
    /// beside the pointer. The field is private (S3's `vfyq2`, fix pass 4,
    /// item L-b): it is what was said.
    #[must_use]
    pub fn message(&self) -> &str {
        self.said.as_str()
    }

    /// Where the problem is, as an RFC 6901 JSON Pointer: `""` for the root,
    /// `/byte/NNN` for a syntax error.
    #[must_use]
    pub fn pointer(&self) -> &str {
        &self.pointer
    }

    /// The bound that refused it, when one did: a caller tells a refusal by
    /// this, never by reading the message.
    #[must_use]
    pub const fn bound(&self) -> Option<Bound> {
        self.bound
    }

    /// What was malformed in a patch or a pointer, when that is why.
    #[must_use]
    pub const fn malformation(&self) -> Option<Malformed> {
        self.malformed
    }

    /// The message, as text a person is shown.
    #[must_use]
    pub fn said(&self) -> Said {
        self.said.clone()
    }

    /// The refusal as a person reads it: the pointer, shown token by token
    /// ([`pharmakos_proto::json::grammar::quoted_pointer`], so one of the
    /// schema's names comes back whole), then the message; the message alone
    /// when the error is at the root.
    #[must_use]
    pub fn shown(&self) -> Said {
        if self.pointer.is_empty() {
            self.said()
        } else {
            pharmakos_proto::said!(
                "at {}: {}",
                pharmakos_proto::json::grammar::quoted_pointer(&self.pointer),
                self.said()
            )
        }
    }

    /// True when this error points at a byte offset rather than at a node.
    ///
    /// The editor branches on the `/byte/` prefix (decisions-log item 96 (2)):
    /// there is nothing to underline, so it highlights the offset instead.
    #[must_use]
    pub fn is_syntax(&self) -> bool {
        self.pointer.starts_with("/byte/")
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.pointer.is_empty() {
            write!(formatter, "{}", self.said)
        } else {
            // The pointer token by token (S3's `vfyq2`, fix pass 5, item L9):
            // a token can be the player's key.
            write!(
                formatter,
                "{}: {}",
                pharmakos_proto::json::grammar::quoted_pointer(&self.pointer),
                self.said
            )
        }
    }
}

impl std::error::Error for Error {}

impl From<pharmakos_verifier::RulesGap> for Error {
    /// Each gap says what it is: a missing block is an incomplete table, and
    /// a value out of range is a table the verifier cannot read, which is a
    /// different fault to fix (decisions-log item 145 (5): the wrapping used
    /// to call every gap "incomplete").
    fn from(gap: pharmakos_verifier::RulesGap) -> Self {
        let what = match gap {
            pharmakos_verifier::RulesGap::MissingBlock(_) => "the rules table is incomplete",
            pharmakos_verifier::RulesGap::OutOfRange { .. }
            | pharmakos_verifier::RulesGap::Negative { .. }
            | pharmakos_verifier::RulesGap::NotPositive { .. } => {
                "the rules table holds a value the verifier cannot use"
            }
        };
        Self::at(
            "",
            pharmakos_proto::said!(
                "{}: {}",
                what,
                pharmakos_proto::json::grammar::Said::engine_error(&gap)
            ),
        )
    }
}

impl From<pharmakos_verifier::InputError> for Error {
    /// Why a verification could not start, said truthfully per cause: the
    /// rules table's own gap, or an input with no `report_hash` encoding,
    /// which is not a fault of the rules table at all.
    fn from(error: pharmakos_verifier::InputError) -> Self {
        match error {
            pharmakos_verifier::InputError::Rules(gap) => Self::from(gap),
            pharmakos_verifier::InputError::Unhashable(cause) => Self::at(
                "",
                pharmakos_proto::said!(
                    "the playbook or the seat's view has no report hash: {}",
                    pharmakos_proto::json::grammar::Said::engine_error(&cause)
                ),
            ),
            pharmakos_verifier::InputError::View(cause) => Self::at(
                "",
                pharmakos_proto::said!(
                    "the seat's view does not hold against the rules table: {}",
                    pharmakos_proto::json::grammar::Said::engine_error(&cause)
                ),
            ),
            // S3's `vfy6`: the verifier's backstop. `verify_jsonc` refuses a
            // player's text past the cap first, so this answers a canonical
            // text no capped file can make.
            cause @ pharmakos_verifier::InputError::Oversize { .. } => Self::at(
                "",
                pharmakos_proto::said!(
                    "the playbook is longer than the verifier takes: {}",
                    pharmakos_proto::json::grammar::Said::engine_error(&cause)
                ),
            ),
        }
    }
}

impl From<pharmakos_verifier::ProjectionError> for Error {
    /// The projection's refusal, said here from its fields (S3's `vfyq2`,
    /// fix pass 3, item M2 (d)): `UnknownBlueprint` carries the blueprint id
    /// the playbook wrote, so it is quoted, and the type does not declare
    /// its `Display` engine words.
    fn from(error: pharmakos_verifier::ProjectionError) -> Self {
        use pharmakos_verifier::ProjectionError;
        let said = match &error {
            ProjectionError::UnknownBlueprint(id) => pharmakos_proto::said!(
                "`{}` is not a blueprint the rules table prices",
                pharmakos_proto::json::grammar::quoted(id)
            ),
            ProjectionError::Overflow => {
                "the projection does not fit in a `$` or `kW` figure".into()
            }
            ProjectionError::UnpricedWall => "a `wall` target with no whole box (none, a corner \
                                              missing, or inside out) has no voxels to price"
                .into(),
        };
        Self::at("", said)
    }
}

impl From<pharmakos_proto::json::Error> for Error {
    fn from(error: pharmakos_proto::json::Error) -> Self {
        // A nesting refusal keeps its kind as a bound (S3's `vfyq2`, fix pass
        // 4, item L-e), so a caller tells it by the bound.
        if error.kind() == pharmakos_proto::json::ErrorKind::Depth {
            return Self::bounded(Bound::Depth, error.pointer().to_owned(), error.said());
        }
        Self::at(error.pointer().to_owned(), error.said())
    }
}

/// Appends one RFC 6901 token to a pointer, escaping `~` and `/` as the
/// standard requires.
///
/// The codec has the same three lines and keeps them private; a pointer this
/// crate builds has to agree with one the codec built, so the rule is written
/// twice and `a_pointer_agrees_with_the_codecs` pins the two together.
#[must_use]
pub fn push_pointer(base: &str, token: &str) -> String {
    let mut pointer =
        String::with_capacity(base.len().saturating_add(token.len()).saturating_add(1));
    pointer.push_str(base);
    pointer.push('/');
    for character in token.chars() {
        match character {
            '~' => pointer.push_str("~0"),
            '/' => pointer.push_str("~1"),
            other => pointer.push(other),
        }
    }
    pointer
}

/// The most tokens a JSON Pointer into a playbook can have: a value inside
/// `pharmakos_proto::json::grammar::MAX_DEPTH` containers is named by one
/// token per container, and an `add` names one slot past the last of them.
pub const MAX_POINTER_TOKENS: usize = pharmakos_proto::json::grammar::MAX_DEPTH + 1;

/// Splits an RFC 6901 pointer into its unescaped tokens, in one pass over its
/// text, read through the grammar's [`pharmakos_proto::json::grammar::Scan`]
/// so the work is on the meter (S3's `vfyq2`, fix pass 2, item A2).
///
/// # Errors
///
/// When the pointer is neither empty nor starts with `/`, which RFC 6901
/// requires of every non-root pointer; when a `~` is followed by anything
/// but `0` or `1` ([`Malformed::PointerEscape`]); and when it has more than
/// [`MAX_POINTER_TOKENS`] tokens ([`Bound::PointerTokens`]), which no node of
/// a playbook has.
pub fn split_pointer(pointer: &str) -> Result<Vec<String>, Error> {
    let scan = pharmakos_proto::json::grammar::Scan::new(pointer);
    match scan.get(0) {
        None => return Ok(Vec::new()),
        Some(b'/') => {}
        Some(_) => {
            return Err(Error::at(
                pointer,
                "a JSON Pointer is empty or starts with `/` (RFC 6901)",
            ));
        }
    }
    let mut tokens: Vec<String> = Vec::new();
    let mut token = String::new();
    // Bytes from `run` up to the next `/` or `~` are copied as one piece;
    // both are ASCII, so a run never ends inside a character.
    let mut pos: usize = 1;
    let mut run: usize = 1;
    loop {
        match scan.get(pos) {
            None | Some(b'/') => {
                token.push_str(scan.slice(run, pos).ok_or_else(|| {
                    Error::at(pointer, "a JSON Pointer token ended inside a character")
                })?);
                if tokens.len() == MAX_POINTER_TOKENS {
                    return Err(Error::bounded(
                        Bound::PointerTokens,
                        "",
                        pharmakos_proto::said!(
                            "a JSON Pointer here names at most {} tokens, one per container a \
                             playbook may nest and one past them",
                            MAX_POINTER_TOKENS
                        ),
                    ));
                }
                tokens.push(std::mem::take(&mut token));
                if scan.get(pos).is_none() {
                    return Ok(tokens);
                }
                pos = pos.saturating_add(1);
                run = pos;
            }
            Some(b'~') => {
                token.push_str(scan.slice(run, pos).ok_or_else(|| {
                    Error::at(pointer, "a JSON Pointer token ended inside a character")
                })?);
                let escaped = match scan.get(pos.saturating_add(1)) {
                    Some(b'0') => '~',
                    Some(b'1') => '/',
                    _ => {
                        return Err(Error::malformed(
                            Malformed::PointerEscape,
                            "",
                            pharmakos_proto::said!(
                                "a JSON Pointer's `~` is followed by `0` or `1` (RFC 6901), and \
                                 the one at byte {} is not",
                                pos
                            ),
                        ));
                    }
                };
                token.push(escaped);
                pos = pos.saturating_add(2);
                run = pos;
            }
            Some(_) => pos = pos.saturating_add(1),
        }
    }
}

/// The parent's pointer and the last token's text, unescaped: the pointer
/// cut at its last `/`, with no tokens rebuilt (S3's `vfyq2`, fix pass 2,
/// item A2).
///
/// # Errors
///
/// As [`split_pointer`] over the last token, and the root, which has no
/// parent.
pub fn parent_of(pointer: &str) -> Result<(&str, String), Error> {
    let cut = pointer
        .rfind('/')
        .ok_or_else(|| Error::at(pointer, "the document root cannot be added or removed"))?;
    let (parent, last) = pointer.split_at(cut);
    let mut tokens = split_pointer(last)?;
    let token = tokens
        .pop()
        .ok_or_else(|| Error::at(pointer, "the document root cannot be added or removed"))?;
    Ok((parent, token))
}

#[cfg(test)]
mod tests {
    use super::{Error, push_pointer, split_pointer};

    #[test]
    fn a_byte_error_is_a_syntax_error() {
        let error = Error::at_byte(412, "a comma is missing");
        assert_eq!(error.pointer, "/byte/412");
        assert!(error.is_syntax());
        assert!(!Error::at("/meta/title", "too long").is_syntax());
    }

    #[test]
    fn pointers_escape_and_unescape_the_two_reserved_characters() {
        let pointer = push_pointer("", "a/b~c");
        assert_eq!(pointer, "/a~1b~0c");
        assert_eq!(
            split_pointer(&pointer).expect("a pointer that starts with `/`"),
            vec!["a/b~c".to_owned()]
        );
    }

    /// The guard the doc on [`push_pointer`] promises. This crate's escaping
    /// and the codec's are two copies of the same three lines, and a pointer
    /// this crate builds is compared against one the codec built — a verifier
    /// diagnostic's pointer against a patch's path, say — so the two have to
    /// agree character for character. The codec keeps its copy private, so the
    /// only way to see one is to make it report an error on a key that needs
    /// both escapes.
    #[test]
    fn a_pointer_agrees_with_the_codecs() {
        let error =
            pharmakos_proto::json::decode::<pharmakos_proto::gp::v1::Playbook>("{\"a/b~c\":1}")
                .map(|_decoded| ())
                .expect_err("`a/b~c` is not a field of gp.v1.Playbook");
        assert_eq!(error.pointer(), push_pointer("", "a/b~c"));
        assert_eq!(error.pointer(), "/a~1b~0c");
    }

    /// Decisions-log item 145 (5): every `Input::new` error used to read "the
    /// rules table is incomplete", an unhashable input included.
    #[test]
    fn each_input_error_says_what_it_is() {
        use pharmakos_verifier::{HashError, InputError, RulesGap};
        let missing: Error = InputError::Rules(RulesGap::MissingBlock("verifier")).into();
        assert!(
            missing
                .message()
                .starts_with("the rules table is incomplete: ")
        );
        let range: Error = InputError::Rules(RulesGap::OutOfRange {
            field: "map.size_x",
            value: u32::MAX,
        })
        .into();
        assert!(
            range
                .message()
                .starts_with("the rules table holds a value the verifier cannot use: "),
            "{}",
            range.message()
        );
        let unhashable: Error = InputError::Unhashable(HashError::TooLong {
            what: "the playbook",
            len: 5,
        })
        .into();
        assert!(
            unhashable
                .message()
                .starts_with("the playbook or the seat's view has no report hash"),
            "{}",
            unhashable.message()
        );
        assert!(!unhashable.message().contains("rules table"));
        let view: Error = InputError::View(pharmakos_verifier::ScopeError::NotOwnBeacon(
            "b_02".to_owned(),
        ))
        .into();
        assert!(
            view.message()
                .starts_with("the seat's view does not hold against the rules table: "),
            "{}",
            view.message()
        );
    }

    #[test]
    fn the_root_pointer_has_no_tokens() {
        assert!(
            split_pointer("")
                .expect("the root pointer is legal")
                .is_empty()
        );
    }

    #[test]
    fn a_pointer_that_does_not_start_with_a_slash_is_refused() {
        assert!(split_pointer("meta/title").is_err());
    }
}
