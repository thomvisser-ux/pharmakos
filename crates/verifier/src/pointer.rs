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

use std::fmt::Write as _;

/// The document root.
pub(crate) const ROOT: &str = "";

/// `base` with one more token appended, escaped as RFC 6901 requires.
pub(crate) fn child(base: &str, token: &str) -> String {
    let mut out = String::with_capacity(base.len().saturating_add(token.len()).saturating_add(3));
    out.push_str(base);
    push_token(&mut out, token);
    out
}

/// Appends `/` and one token, escaped as RFC 6901 requires.
fn push_token(out: &mut String, token: &str) {
    out.push('/');
    for character in token.chars() {
        match character {
            '~' => out.push_str("~0"),
            '/' => out.push_str("~1"),
            other => out.push(other),
        }
    }
}

/// A JSON Pointer built in place as a walk descends: one buffer, with a token
/// pushed on the way down and popped on the way back, so a walk allocates
/// nothing per node and a diagnostic copies the pointer only when it is raised
/// (S3's `vfyq`, decisions-log item 158). [`Here::as_str`] is the same string
/// [`child`] and [`at`] would have built.
#[derive(Debug)]
pub(crate) struct Here {
    buf: String,
}

/// Where a [`Here`] stood before a push, to pop back to.
///
/// A push that drops its mark leaves the pointer a level too deep for the
/// walk's next sibling, so a mark must be used (PR #177's review).
#[derive(Clone, Copy, Debug)]
#[must_use = "pop back to the mark, or every later pointer is one level too deep"]
pub(crate) struct Mark(usize);

impl Here {
    /// A pointer starting at `at`.
    pub(crate) fn new(at: &str) -> Here {
        let mut buf = String::with_capacity(at.len().saturating_add(160));
        buf.push_str(at);
        Here { buf }
    }

    /// The pointer to where the walk is.
    pub(crate) fn as_str(&self) -> &str {
        &self.buf
    }

    /// Descends one member.
    pub(crate) fn push(&mut self, token: &str) -> Mark {
        let mark = Mark(self.buf.len());
        push_token(&mut self.buf, token);
        mark
    }

    /// Descends several members at once, climbed back from as one: one push,
    /// one mark, one pop.
    pub(crate) fn push_path(&mut self, tokens: &[&str]) -> Mark {
        let mark = Mark(self.buf.len());
        for token in tokens {
            push_token(&mut self.buf, token);
        }
        mark
    }

    /// Descends one array element.
    pub(crate) fn push_index(&mut self, index: usize) -> Mark {
        let mark = Mark(self.buf.len());
        self.buf.push('/');
        // Writing to a `String` cannot fail.
        let _infallible = write!(self.buf, "{index}");
        mark
    }

    /// Climbs back to where `mark` was taken.
    pub(crate) fn pop(&mut self, mark: Mark) {
        self.buf.truncate(mark.0);
    }
}

/// `base` with several tokens appended, each escaped: the pointer a check
/// builds only when it has something to report.
pub(crate) fn join(base: &str, tokens: &[&str]) -> String {
    let mut out = String::with_capacity(base.len().saturating_add(32));
    out.push_str(base);
    for token in tokens {
        push_token(&mut out, token);
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
    use super::{Here, at, child, last_token};

    #[test]
    fn a_pointer_built_in_place_is_the_one_built_by_parts() {
        let mut here = Here::new("/declarative/route");
        let step = here.push_index(12);
        let odd = here.push("a/b~c");
        assert_eq!(here.as_str(), child(&at("/declarative/route", 12), "a/b~c"));
        here.pop(odd);
        assert_eq!(here.as_str(), "/declarative/route/12");
        here.pop(step);
        assert_eq!(here.as_str(), "/declarative/route");
    }

    #[test]
    fn a_path_pushed_at_once_is_the_one_pushed_token_by_token() {
        let mut here = Here::new("/fallback");
        let at = here.push_path(&["patrol", "way/points"]);
        assert_eq!(
            here.as_str(),
            child(&child("/fallback", "patrol"), "way/points")
        );
        here.pop(at);
        assert_eq!(here.as_str(), "/fallback");
    }

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
