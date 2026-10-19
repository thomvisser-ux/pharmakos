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

/// Something a playbook file, a patch, or the schema it is read against did not
/// allow.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Error {
    /// Where the problem is, as an RFC 6901 JSON Pointer — `""` for the root,
    /// `/declarative/route/0/move` for a step's move, `/byte/412` for a syntax
    /// error that has no place in the tree.
    pub pointer: String,
    /// English, precise, and safe to show a player beside the pointer.
    pub message: String,
}

impl Error {
    /// An error at a pointer.
    #[must_use]
    pub fn at(pointer: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            pointer: pointer.into(),
            message: message.into(),
        }
    }

    /// An error at a byte offset, for a document that did not parse.
    #[must_use]
    pub fn at_byte(offset: usize, message: impl Into<String>) -> Self {
        Self::at(format!("/byte/{offset}"), message)
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
            write!(formatter, "{}", self.message)
        } else {
            write!(formatter, "{}: {}", self.pointer, self.message)
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
        Self::at("", format!("{what}: {gap}"))
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
                format!("the playbook or the seat's view has no report hash: {cause}"),
            ),
        }
    }
}

impl From<pharmakos_verifier::ProjectionError> for Error {
    fn from(error: pharmakos_verifier::ProjectionError) -> Self {
        Self::at("", error.to_string())
    }
}

impl From<pharmakos_proto::json::Error> for Error {
    fn from(error: pharmakos_proto::json::Error) -> Self {
        Self {
            pointer: error.pointer,
            message: error.message,
        }
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

/// Splits an RFC 6901 pointer into its unescaped tokens.
///
/// # Errors
///
/// When the pointer is neither empty nor starts with `/`, which RFC 6901
/// requires of every non-root pointer.
pub fn split_pointer(pointer: &str) -> Result<Vec<String>, Error> {
    if pointer.is_empty() {
        return Ok(Vec::new());
    }
    let Some(body) = pointer.strip_prefix('/') else {
        return Err(Error::at(
            pointer,
            "a JSON Pointer is empty or starts with `/` (RFC 6901)",
        ));
    };
    Ok(body
        .split('/')
        .map(|token| token.replace("~1", "/").replace("~0", "~"))
        .collect())
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
        assert_eq!(error.pointer, push_pointer("", "a/b~c"));
        assert_eq!(error.pointer, "/a~1b~0c");
    }

    /// Decisions-log item 145 (5): every `Input::new` error used to read "the
    /// rules table is incomplete", an unhashable input included.
    #[test]
    fn each_input_error_says_what_it_is() {
        use pharmakos_verifier::{HashError, InputError, RulesGap};
        let missing: Error = InputError::Rules(RulesGap::MissingBlock("verifier")).into();
        assert!(
            missing
                .message
                .starts_with("the rules table is incomplete: ")
        );
        let range: Error = InputError::Rules(RulesGap::OutOfRange {
            field: "map.size_x",
            value: u32::MAX,
        })
        .into();
        assert!(
            range
                .message
                .starts_with("the rules table holds a value the verifier cannot use: "),
            "{}",
            range.message
        );
        let unhashable: Error = InputError::Unhashable(HashError::TooLong {
            what: "the playbook",
            len: 5,
        })
        .into();
        assert!(
            unhashable
                .message
                .starts_with("the playbook or the seat's view has no report hash"),
            "{}",
            unhashable.message
        );
        assert!(!unhashable.message.contains("rules table"));
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
