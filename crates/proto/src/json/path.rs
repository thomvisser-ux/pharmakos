// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where the codec is in a document, kept as a chain of borrowed tokens on the
//! stack and rendered as an RFC 6901 JSON Pointer only when an error needs one.
//!
//! The codec used to build the pointer string of every member it visited, so
//! that an error could carry it; S3's `vfyq` measured that as most of the
//! codec's cost (decisions-log item 158 (1)). A [`Path`] costs nothing until
//! [`Path::render`] is called, and renders the same string the eager pointer
//! held: the same tokens, escaped the same way.

/// A place in a document: the root, or one token below another place.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Path<'a> {
    /// The document itself, rendered `""`.
    Root,
    /// A member, by the name the pointer spells it with.
    Key(&'a Path<'a>, &'a str),
    /// An element of an array, by its index.
    Index(&'a Path<'a>, usize),
}

impl Path<'_> {
    /// The pointer, as `push_pointer` would have built it token by token.
    pub(crate) fn render(&self) -> String {
        let mut out = String::new();
        self.render_into(&mut out);
        out
    }

    fn render_into(&self, out: &mut String) {
        match *self {
            Path::Root => {}
            Path::Key(parent, token) => {
                parent.render_into(out);
                push_token(out, token);
            }
            Path::Index(parent, index) => {
                parent.render_into(out);
                push_token(out, &index.to_string());
            }
        }
    }
}

/// One RFC 6901 token, escaping `~` and `/` as the standard requires.
pub(crate) fn push_token(out: &mut String, token: &str) {
    out.push('/');
    for character in token.chars() {
        match character {
            '~' => out.push_str("~0"),
            '/' => out.push_str("~1"),
            other => out.push(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Path;
    use crate::json::value::push_pointer;

    #[test]
    fn a_path_renders_what_the_eager_pointer_held() {
        let root = Path::Root;
        assert_eq!(root.render(), "");
        let declarative = Path::Key(&root, "declarative");
        let route = Path::Key(&declarative, "route");
        let step = Path::Index(&route, 12);
        let odd = Path::Key(&step, "a/b~c");
        let eager = push_pointer(
            &push_pointer(
                &push_pointer(&push_pointer("", "declarative"), "route"),
                "12",
            ),
            "a/b~c",
        );
        assert_eq!(odd.render(), eager);
        assert_eq!(odd.render(), "/declarative/route/12/a~1b~0c");
    }
}
