// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A JSON value, a strict reader for one, and the canonical writer.
//!
//! Hand-written, because `serde_json` is not on the approved dependency list
//! (AGENTS.md section 3 rule 5) and because two properties matter more here
//! than convenience:
//!
//! * **Numbers are never floats.** A lexeme is kept as text and converted to
//!   `i64` or `u64` only when a field asks for it. No `gp.v1` or `gp.api.v1`
//!   field is floating point, so a fractional literal is a diagnostic rather
//!   than a rounding (AGENTS.md section 4.2).
//! * **Object order is the insertion order**, so the writer can emit fields in
//!   field-number order and the reader can report the position of a duplicate
//!   key rather than silently keeping the last one.

use std::fmt;
use std::fmt::Write as _;

/// A JSON value.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Json {
    /// `null`. The proto3 JSON mapping reads it as "the default value", so a
    /// field spelled out as null is loadable and encodes to nothing.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// The lexeme exactly as it was written, or as the writer means to write
    /// it. Never parsed into a float.
    Number(String),
    /// A string, already unescaped.
    String(String),
    /// An array, in order.
    Array(Vec<Json>),
    /// Key/value pairs in order. Duplicate keys are rejected by the reader.
    Object(Vec<(String, Json)>),
}

impl Json {
    /// The value under a key, if this is an object.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Object(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// The name of this kind of value, for diagnostics.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "a boolean",
            Self::Number(_) => "a number",
            Self::String(_) => "a string",
            Self::Array(_) => "an array",
            Self::Object(_) => "an object",
        }
    }
}

/// What kind of thing a text or a document did not allow (S3's `vfy6`, the
/// review of PR #179): the typed half of an [`Error`], so a caller decides
/// by kind and never by reading the message.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrorKind {
    /// The text is not JSON as RFC 8259 writes it: a syntax error, an
    /// escape, a surrogate, a number's spelling, trailing content.
    Grammar,
    /// Nested past the nesting limit, in the text or in the schema.
    Depth,
    /// One key written twice in one object.
    DuplicateField,
    /// A member the message does not declare.
    UnknownField,
    /// One field written under both its `.proto` name and its JSON name.
    BothNames,
    /// Two members of one `oneof` set at once.
    OneofArms,
    /// An integer field's value: not whole, out of range, or misspelt.
    Integer,
    /// A `bytes` field's value is not base64.
    Base64,
    /// A value of the wrong JSON type for its field, an enum name the enum
    /// does not hold, a floating-point field, or a value too long for its
    /// length prefix.
    Value,
    /// Wire bytes that do not decode as the message they are read as.
    Wire,
    /// A name the schema does not hold: a message, a type or a path.
    Schema,
}

/// Something a JSON text, or the schema it is read against, did not allow.
///
/// `pointer` is an RFC 6901 JSON Pointer into the document — `""` for the
/// root, `/declarative/route/0/move` for a step's move. The verifier's
/// diagnostics carry the same shape, which is the point: an error found here
/// can be reported at the place in the file that caused it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Error {
    // Every field is private (S3's `vfyq2`, fix pass 4b, item L-b): only
    // this crate makes an error, from a [`super::grammar::Said`], and a
    // public field could be written afterwards, so a caller reads through
    // [`Error::kind`], [`Error::pointer`], [`Error::message`] and
    // [`Error::said`], and [`Error::said`] is the codec's own words.
    /// What kind of thing it is.
    kind: ErrorKind,
    /// Where in the document the problem is, as an RFC 6901 JSON Pointer. A
    /// syntax error, which has no place in the tree yet, reports `/byte/NNN`.
    pointer: String,
    /// English, precise, and safe to show a player beside the pointer.
    message: String,
}

impl Error {
    pub(crate) fn at(
        kind: ErrorKind,
        pointer: &str,
        message: impl Into<super::grammar::Said>,
    ) -> Self {
        Self {
            kind,
            pointer: pointer.to_owned(),
            message: message.into().into_string(),
        }
    }

    /// The reference reader's error (test-only: the old codec, kept to hold
    /// the fast one to it): its words are its own `String`s, and nothing
    /// shipped reads them.
    #[cfg(test)]
    pub(crate) fn from_reference(kind: ErrorKind, pointer: String, message: String) -> Self {
        Self {
            kind,
            pointer,
            message,
        }
    }

    /// The message, as text a player is shown (S3's `vfyq2`, fix pass 2,
    /// item B): every message is made by [`Error::at`] from a
    /// [`super::grammar::Said`], and no other crate can make an `Error`, so
    /// the message is already in the shown form.
    #[must_use]
    pub fn said(&self) -> super::grammar::Said {
        super::grammar::Said::made_here(self.message.clone())
    }

    /// What kind of thing it is: a caller branches on this, never on the
    /// message.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Where in the document the problem is, as an RFC 6901 JSON Pointer;
    /// `/byte/NNN` for a syntax error.
    #[must_use]
    pub fn pointer(&self) -> &str {
        &self.pointer
    }

    /// The message, as text: English, precise, and safe to show a player
    /// beside the pointer.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// A codec error's words are its message, which [`Error::at`] took as a
/// [`super::grammar::Said`].
impl super::grammar::EngineWords for Error {}
impl super::grammar::door::EngineSeal for Error {}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The pointer token by token (S3's `vfyq2`, fix pass 4, item L-a): a
        // token can be the player's key.
        if self.pointer.is_empty() {
            write!(formatter, "{}", self.message)
        } else {
            write!(
                formatter,
                "{}: {}",
                super::grammar::quoted_pointer(&self.pointer),
                self.message
            )
        }
    }
}

impl std::error::Error for Error {}

/// Appends one RFC 6901 token to a pointer, escaping `~` and `/` as the
/// standard requires. The codec now carries a lazy `Path`; this is the
/// reference's, and the tests'.
#[cfg(test)]
pub(crate) fn push_pointer(base: &str, token: &str) -> String {
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

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// Reads a JSON text. Strict: no comments, no trailing commas, no duplicate
/// keys, no trailing content.
///
/// The JSONC layer — comments, and the byte-exact round-trip of formatting —
/// is `plan-core`'s, not this crate's (decisions-log item 74). A `.jsonc`
/// playbook reaches this reader with its comments already stripped.
///
/// # Errors
///
/// Returns the position of the first byte that does not belong, as a pointer
/// of the form `/byte/NNN`, because a syntax error has no place in the
/// document tree yet.
pub fn read(text: &str) -> Result<Json, Error> {
    let mut trivia = super::grammar::Whitespace;
    super::grammar::parse(text, &mut Tree, &mut trivia).map(|document| document.value)
}

/// What [`read`] builds: a plain tree, trivia discarded.
pub(crate) struct Tree;

impl<'t> super::grammar::Build<'t> for Tree {
    type Value = Json;
    type Members = Vec<(String, Json)>;
    type Items = Vec<Json>;

    fn members(&mut self) -> Self::Members {
        Vec::new()
    }

    fn items(&mut self) -> Self::Items {
        Vec::new()
    }

    fn null(&mut self) -> Json {
        Json::Null
    }

    fn boolean(&mut self, value: bool) -> Json {
        Json::Bool(value)
    }

    fn number(&mut self, lexeme: &'t str) -> Json {
        Json::Number(lexeme.to_owned())
    }

    fn string(&mut self, _raw: &'t str, text: String) -> Json {
        Json::String(text)
    }

    fn member(&mut self, members: &mut Self::Members, member: super::grammar::Member<'t, Json>) {
        members.push((member.key, member.value));
    }

    fn item(&mut self, items: &mut Self::Items, _before: &'t str, value: Json, _after: &'t str) {
        items.push(value);
    }

    fn object(&mut self, members: Self::Members, _inner: &'t str) -> Json {
        Json::Object(members)
    }

    fn array(&mut self, items: Self::Items, _inner: &'t str) -> Json {
        Json::Array(items)
    }
}

/// Whether `lexeme` is an integer by RFC 8259's grammar: `-`? then `0`, or a
/// non-zero digit and digits — no `+`, no leading zero, nothing else (S3's
/// `vfy6`).
///
/// A bare number the reader accepted already has that shape when it is
/// whole; the proto3 mapping also accepts an integer quoted, and a quoted one
/// is held to the same shape here, so `"+0"` and `"00"` are refused as `+0`
/// and `00` are. Every reader of an integer asks this before it parses one.
pub(crate) fn is_integer_lexeme(lexeme: &str) -> bool {
    let digits = lexeme.strip_prefix('-').unwrap_or(lexeme);
    match digits.as_bytes() {
        [b'0'] => true,
        [b'1'..=b'9', rest @ ..] => rest.iter().all(u8::is_ascii_digit),
        _ => false,
    }
}

/// Whether `lexeme` is digits that RFC 8259 does not write as an integer: a
/// `+`, or a leading zero. A refusal says so, rather than that the value is
/// not whole.
pub(crate) fn is_misspelt_integer(lexeme: &str) -> bool {
    let digits = lexeme.strip_prefix(['+', '-']).unwrap_or(lexeme);
    !digits.is_empty()
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && !is_integer_lexeme(lexeme)
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// The canonical text for a value: two-space indent, one entry per line, `\n`
/// line endings, a trailing newline.
///
/// One entry per line is deliberate. Golden files have to be human-diffable
/// (AGENTS.md section 9 item 7), and a one-line array turns a one-element
/// change into a whole-line diff nobody can read.
#[must_use]
pub fn write(value: &Json) -> String {
    let mut out = String::new();
    write_value(value, 0, &mut out);
    out.push('\n');
    out
}

/// A value as **one line** of JSON, with no line ending: the form for a
/// stream that frames a message as a line (the external seat's bridge,
/// `gamectl match serve`; decisions-log item 231).
///
/// The same lexemes as [`write`], so [`read`] reads it back to the same
/// value; the layout differs, and so does one thing about strings: beside the
/// characters [`write`] escapes (the C0 controls, a line feed among them),
/// this writes escaped every other character a line reader may split on:
/// DEL, the C1 controls (U+0085 among them), U+2028 and U+2029. The text
/// therefore holds no line break by any reader's rule, whatever the value
/// holds.
#[must_use]
pub fn write_line(value: &Json) -> String {
    let mut out = String::new();
    write_value_on_a_line(value, &mut out);
    out
}

fn write_value_on_a_line(value: &Json, out: &mut String) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(true) => out.push_str("true"),
        Json::Bool(false) => out.push_str("false"),
        Json::Number(lexeme) => out.push_str(lexeme),
        Json::String(text) => write_string_escaping(text, needs_escape_on_a_line, out),
        Json::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_value_on_a_line(item, out);
            }
            out.push(']');
        }
        Json::Object(entries) => {
            out.push('{');
            for (index, (key, item)) in entries.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_string_escaping(key, needs_escape_on_a_line, out);
                out.push(':');
                write_value_on_a_line(item, out);
            }
            out.push('}');
        }
    }
}

fn indent(depth: usize, out: &mut String) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn write_value(value: &Json, depth: usize, out: &mut String) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(true) => out.push_str("true"),
        Json::Bool(false) => out.push_str("false"),
        Json::Number(lexeme) => out.push_str(lexeme),
        Json::String(text) => write_string(text, out),
        Json::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push_str("[\n");
            let inner = depth.saturating_add(1);
            for (index, item) in items.iter().enumerate() {
                indent(inner, out);
                write_value(item, inner, out);
                if index.saturating_add(1) < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            indent(depth, out);
            out.push(']');
        }
        Json::Object(entries) => {
            if entries.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push_str("{\n");
            let inner = depth.saturating_add(1);
            for (index, (key, item)) in entries.iter().enumerate() {
                indent(inner, out);
                write_string(key, out);
                out.push_str(": ");
                write_value(item, inner, out);
                if index.saturating_add(1) < entries.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            indent(depth, out);
            out.push('}');
        }
    }
}

/// A string as canonical JSON: quoted, with `"`, `\` and the control
/// characters escaped and everything else as it is.
pub(crate) fn write_string(text: &str, out: &mut String) {
    write_string_escaping(text, needs_escape, out);
}

/// [`write_string`], escaping what `needs` says: the canonical writer's set,
/// or the one-line writer's wider one. A character with no short escape is
/// written `\uXXXX`; every character either set names is below U+10000, so
/// four digits hold it.
fn write_string_escaping(text: &str, needs: fn(char) -> bool, out: &mut String) {
    out.push('"');
    // The text between two characters that need an escape is copied in one
    // piece; each piece ends with the character that ends it, if any.
    for piece in text.split_inclusive(needs) {
        let mut characters = piece.chars();
        match characters.next_back() {
            Some(last) if needs(last) => {
                out.push_str(characters.as_str());
                match last {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\u{8}' => out.push_str("\\b"),
                    '\u{c}' => out.push_str("\\f"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    other => {
                        let _ignored = write!(out, "\\u{:04x}", u32::from(other));
                    }
                }
            }
            _ => out.push_str(piece),
        }
    }
    out.push('"');
}

/// Whether a character is written escaped: `"`, `\` and the C0 controls.
const fn needs_escape(character: char) -> bool {
    matches!(character, '"' | '\\' | '\u{0}'..='\u{1f}')
}

/// Whether the one-line writer writes a character escaped: the canonical
/// set, and every other character some line reader splits a text on. Those
/// are DEL and the C1 controls (U+0085, next line, among them) and the two
/// Unicode separators, U+2028 and U+2029: JSON allows each raw inside a
/// string, and a reader that splits on any of them (Python's
/// `str.splitlines` splits on all three named) would cut one answer into
/// pieces that are not JSON.
const fn needs_escape_on_a_line(character: char) -> bool {
    needs_escape(character) || matches!(character, '\u{7f}'..='\u{9f}' | '\u{2028}' | '\u{2029}')
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::super::grammar::{LINEAR_KEYS, parse_counting};
    use super::{Error, Tree, read};

    /// The one-line writer writes what the canonical writer writes, on one
    /// line: it reads back to the same value, and no control character
    /// reaches the text, whatever a string holds.
    #[test]
    fn the_one_line_form_reads_back_and_holds_no_line_break() {
        let text = "{\"a\":[1,-2.5e3,true,null,{}],\"b\":\"one\\ntwo\\r\\tthree\\u0000 \\\"q\\\" \\\\\",\
                    \"c\":{\"d\":[]},\"e\":\"\\u2028\"}";
        let value = read(text).expect("a value");
        let line = super::write_line(&value);
        assert!(
            !line.chars().any(char::is_control),
            "a control character on the line: {line:?}"
        );
        assert_eq!(read(&line).expect("the line reads back"), value);
        assert_eq!(
            read(&super::write(&value)).expect("the canonical form reads back"),
            value
        );
        assert_eq!(super::write_line(&read("[ ]").expect("empty")), "[]");
        assert_eq!(super::write_line(&read("{ }").expect("empty")), "{}");
        assert_eq!(
            super::write_line(&read("{\"k\": [1, \"v\"]}").expect("a value")),
            "{\"k\":[1,\"v\"]}"
        );
    }

    /// No character a line reader splits on reaches the one-line form raw, in
    /// a string or in a key: DEL, the C1 controls (U+0085, next line, among
    /// them) and the two Unicode separators are written escaped, and the line
    /// still reads back to the same value. The canonical writer is unchanged:
    /// it writes them raw, as it always has.
    #[test]
    fn the_one_line_form_escapes_every_character_a_line_reader_splits_on() {
        let splitters = [
            '\u{7f}', '\u{80}', '\u{85}', '\u{9f}', '\u{2028}', '\u{2029}',
        ];
        for splitter in splitters {
            let text = format!("a{splitter}b");
            let value = super::Json::Object(vec![(
                text.clone(),
                super::Json::Array(vec![super::Json::String(text.clone())]),
            )]);
            let line = super::write_line(&value);
            assert!(
                !line.contains(splitter),
                "U+{:04X} is raw on the line: {line:?}",
                u32::from(splitter)
            );
            let escaped = format!("a\\u{:04x}b", u32::from(splitter));
            assert_eq!(line, format!("{{\"{escaped}\":[\"{escaped}\"]}}"));
            assert_eq!(read(&line).expect("the line reads back"), value);
            assert!(
                super::write(&value).contains(splitter),
                "the canonical writer's escapes moved"
            );
        }
        // The neighbours on either side of each range are written as they are.
        let kept = "~\u{a0}\u{2027}\u{202a}";
        let line = super::write_line(&super::Json::String(kept.to_owned()));
        assert_eq!(line, format!("\"{kept}\""));
    }

    /// `read`, with how many key comparisons its duplicate-key check made.
    fn read_counting(text: &str) -> (Result<super::Json, Error>, u64) {
        let (value, comparisons, _steps) = parse_counting(text, &mut Tree);
        (value, comparisons)
    }

    /// One object of `n` distinct keys of one length, and optionally the key
    /// at `repeat` written again at the end.
    fn many_keys(n: usize, repeat: Option<usize>) -> String {
        let mut text = String::from("{");
        for at in 0..n {
            if at > 0 {
                text.push(',');
            }
            let _infallible = write!(text, "\"k{at:06}\":0");
        }
        if let Some(at) = repeat {
            let _infallible = write!(text, ",\"k{at:06}\":1");
        }
        text.push('}');
        text
    }

    /// The duplicate-key check scans a small object and keeps a sorted set
    /// for a large one, so its linear comparisons are bounded by a constant
    /// however many keys there are: 32,000 keys took 1.95 s when every key
    /// was compared with every key before it (PR #177's robustness review).
    #[test]
    fn many_distinct_keys_are_checked_in_less_than_quadratic_work() {
        let bound = u64::try_from(LINEAR_KEYS * LINEAR_KEYS).unwrap();
        for n in [LINEAR_KEYS, 1_000, 32_000] {
            let (value, comparisons) = read_counting(&many_keys(n, None));
            assert!(value.is_ok(), "{n} distinct keys are one object");
            assert!(
                comparisons <= bound,
                "{n} keys took {comparisons} linear comparisons, past {bound}"
            );
        }
    }

    /// A key written twice is refused at the same byte, with the same words,
    /// whether the object is small enough to scan or large enough to keep a
    /// set, and as the reference reader refuses it.
    #[test]
    fn a_duplicate_key_is_refused_alike_below_and_past_the_set() {
        for (n, repeat) in [
            (3, 0),
            (LINEAR_KEYS - 1, 2),
            (LINEAR_KEYS, LINEAR_KEYS - 1),
            (LINEAR_KEYS + 1, 0),
            (2_000, 1_999),
            (2_000, 7),
        ] {
            let text = many_keys(n, Some(repeat));
            let ours = read(&text);
            assert!(ours.is_err(), "{n} keys with key {repeat} written twice");
            assert_eq!(ours, super::super::reference::read(&text), "{n}, {repeat}");
        }
        // A duplicate among the first keys of an object that goes on to grow
        // past the set is still the first duplicate reported.
        let mut text = many_keys(40, None);
        text.insert_str(1, "\"k000003\":2,");
        assert_eq!(read(&text), super::super::reference::read(&text));
    }
}
