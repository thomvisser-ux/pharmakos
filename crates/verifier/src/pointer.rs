// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! RFC 6901 JSON Pointers into the playbook.
//!
//! Every diagnostic carries one (spec section 11), and it is what the editor
//! jumps to. The spelling is the canonical JSON's, which is the `.proto`
//! spelling of the field: `/declarative/route/0/timeout_ms`, never a camel-case
//! one and never a Rust field name.
//!
//! The escaping is the standard's and is not optional: `~` becomes `~0` and `/`
//! becomes `~1`, in that order. A step label is author text and may contain
//! either, and a pointer that does not escape them addresses a different place.

/// The document root.
pub(crate) const ROOT: &str = "";

/// `base` with one more token appended, escaped as RFC 6901 requires.
pub(crate) fn child(base: &str, token: &str) -> String {
    let mut out = String::with_capacity(base.len().saturating_add(token.len()).saturating_add(3));
    out.push_str(base);
    out.push('/');
    for character in token.chars() {
        match character {
            '~' => out.push_str("~0"),
            '/' => out.push_str("~1"),
            other => out.push(other),
        }
    }
    out
}

/// `base` with one array index appended.
pub(crate) fn at(base: &str, index: usize) -> String {
    child(base, &index.to_string())
}

/// The last token of a pointer, unescaped, or `None` for the root.
///
/// Used to name the field a decode error landed on: the codec reports an
/// unknown field with a pointer to the key, and the key is what says whether it
/// is out of vocabulary (a reserved name) or simply not a field.
pub(crate) fn last_token(pointer: &str) -> Option<String> {
    let (_, token) = pointer.rsplit_once('/')?;
    let mut out = String::with_capacity(token.len());
    let mut chars = token.chars();
    while let Some(character) = chars.next() {
        if character == '~' {
            match chars.next() {
                Some('1') => out.push('/'),
                Some(other) if other != '0' => {
                    out.push('~');
                    out.push(other);
                }
                // `~0` is an escaped tilde; a trailing `~` is not an escape at
                // all, and both are written back as the tilde they came from.
                Some(_) | None => out.push('~'),
            }
        } else {
            out.push(character);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{at, child, last_token};

    #[test]
    fn a_token_is_escaped_as_rfc_6901_asks() {
        assert_eq!(child("", "meta"), "/meta");
        assert_eq!(child("/a", "b/c"), "/a/b~1c");
        assert_eq!(child("/a", "b~c"), "/a/b~0c");
        assert_eq!(at("/declarative/route", 2), "/declarative/route/2");
    }

    #[test]
    fn the_last_token_round_trips_through_the_escaping() {
        assert_eq!(last_token("/meta/title").as_deref(), Some("title"));
        assert_eq!(last_token("/a/b~1c").as_deref(), Some("b/c"));
        assert_eq!(last_token("/a/b~0c").as_deref(), Some("b~c"));
        assert_eq!(last_token(""), None);
    }
}
