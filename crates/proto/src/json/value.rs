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

/// Something a JSON text, or the schema it is read against, did not allow.
///
/// `pointer` is an RFC 6901 JSON Pointer into the document — `""` for the
/// root, `/declarative/route/0/move` for a step's move. The verifier's
/// diagnostics carry the same shape, which is the point: an error found here
/// can be reported at the place in the file that caused it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Error {
    /// Where in the document the problem is, as an RFC 6901 JSON Pointer. A
    /// syntax error, which has no place in the tree yet, reports `/byte/NNN`.
    pub pointer: String,
    /// English, precise, and safe to show a player beside the pointer.
    pub message: String,
}

impl Error {
    pub(crate) fn at(pointer: &str, message: impl Into<String>) -> Self {
        Self {
            pointer: pointer.to_owned(),
            message: message.into(),
        }
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

/// Appends one RFC 6901 token to a pointer, escaping `~` and `/` as the
/// standard requires.
pub(crate) fn push_pointer(base: &str, token: &str) -> String {
    let mut pointer = String::with_capacity(base.len().saturating_add(token.len()) + 1);
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
    let bytes = text.as_bytes();
    let mut reader = JsonReader { bytes, pos: 0 };
    reader.skip_whitespace();
    let value = reader.value(0)?;
    reader.skip_whitespace();
    if reader.pos < bytes.len() {
        return Err(reader.error("trailing content after the JSON value"));
    }
    Ok(value)
}

/// The deepest nesting a document may have. Generous for a playbook, whose
/// condition trees are capped at depth 4 by the vocabulary, and low enough
/// that a hostile file cannot exhaust the stack — the verifier's fuzz target
/// throws 10,000 generated playbooks at this path.
const MAX_DEPTH: usize = 64;

struct JsonReader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl JsonReader<'_> {
    fn error(&self, message: impl Into<String>) -> Error {
        Error {
            pointer: format!("/byte/{}", self.pos),
            message: message.into(),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn bump(&mut self) {
        self.pos = self.pos.saturating_add(1);
    }

    fn skip_whitespace(&mut self) {
        while let Some(byte) = self.peek() {
            if matches!(byte, b' ' | b'\t' | b'\n' | b'\r') {
                self.bump();
            } else {
                return;
            }
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), Error> {
        if self.peek() == Some(byte) {
            self.bump();
            Ok(())
        } else {
            Err(self.error(format!("expected `{}`", char::from(byte))))
        }
    }

    fn literal(&mut self, word: &str) -> Result<(), Error> {
        for expected in word.bytes() {
            if self.peek() != Some(expected) {
                return Err(self.error(format!("expected `{word}`")));
            }
            self.bump();
        }
        Ok(())
    }

    fn value(&mut self, depth: usize) -> Result<Json, Error> {
        if depth > MAX_DEPTH {
            return Err(self.error("JSON nested deeper than 64 levels"));
        }
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => self.string().map(Json::String),
            Some(b't') => {
                self.literal("true")?;
                Ok(Json::Bool(true))
            }
            Some(b'f') => {
                self.literal("false")?;
                Ok(Json::Bool(false))
            }
            Some(b'n') => {
                self.literal("null")?;
                Ok(Json::Null)
            }
            Some(_) => self.number(),
            None => Err(self.error("the document ended where a value was expected")),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, Error> {
        self.expect(b'{')?;
        let mut entries: Vec<(String, Json)> = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.bump();
            return Ok(Json::Object(entries));
        }
        loop {
            self.skip_whitespace();
            let key = self.string()?;
            if entries.iter().any(|(name, _)| *name == key) {
                return Err(self.error(format!("duplicate key `{key}`")));
            }
            self.skip_whitespace();
            self.expect(b':')?;
            self.skip_whitespace();
            let value = self.value(depth.saturating_add(1))?;
            entries.push((key, value));
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.bump(),
                Some(b'}') => {
                    self.bump();
                    return Ok(Json::Object(entries));
                }
                _ => return Err(self.error("expected `,` or `}`")),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, Error> {
        self.expect(b'[')?;
        let mut items: Vec<Json> = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.bump();
            return Ok(Json::Array(items));
        }
        loop {
            self.skip_whitespace();
            items.push(self.value(depth.saturating_add(1))?);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.bump(),
                Some(b']') => {
                    self.bump();
                    return Ok(Json::Array(items));
                }
                _ => return Err(self.error("expected `,` or `]`")),
            }
        }
    }

    fn string(&mut self) -> Result<String, Error> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let byte = self
                .peek()
                .ok_or_else(|| self.error("unterminated string"))?;
            match byte {
                b'"' => {
                    self.bump();
                    return Ok(out);
                }
                b'\\' => {
                    self.bump();
                    self.escape(&mut out)?;
                }
                0x00..=0x1f => {
                    return Err(self.error("a raw control character in a string"));
                }
                _ => {
                    // Copy one whole UTF-8 sequence, which `str` guarantees is
                    // well formed because the input was a `&str`.
                    let start = self.pos;
                    self.bump();
                    while let Some(next) = self.peek() {
                        if next & 0xc0 == 0x80 {
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    let slice = self
                        .bytes
                        .get(start..self.pos)
                        .ok_or_else(|| self.error("truncated UTF-8"))?;
                    out.push_str(
                        std::str::from_utf8(slice)
                            .map_err(|_| self.error("invalid UTF-8 in a string"))?,
                    );
                }
            }
        }
    }

    fn escape(&mut self, out: &mut String) -> Result<(), Error> {
        let byte = self
            .peek()
            .ok_or_else(|| self.error("the document ended inside an escape"))?;
        self.bump();
        match byte {
            b'"' => out.push('"'),
            b'\\' => out.push('\\'),
            b'/' => out.push('/'),
            b'b' => out.push('\u{8}'),
            b'f' => out.push('\u{c}'),
            b'n' => out.push('\n'),
            b'r' => out.push('\r'),
            b't' => out.push('\t'),
            b'u' => {
                let first = self.hex4()?;
                let code = if (0xd800..0xdc00).contains(&first) {
                    self.expect(b'\\')?;
                    self.expect(b'u')?;
                    let second = self.hex4()?;
                    if !(0xdc00..0xe000).contains(&second) {
                        return Err(self.error("a high surrogate without a low surrogate"));
                    }
                    let high = first.saturating_sub(0xd800) << 10;
                    let low = second.saturating_sub(0xdc00);
                    0x1_0000u32
                        .checked_add(high | low)
                        .ok_or_else(|| self.error("a surrogate pair out of range"))?
                } else {
                    first
                };
                let character =
                    char::from_u32(code).ok_or_else(|| self.error("not a Unicode scalar value"))?;
                out.push(character);
            }
            other => {
                return Err(self.error(format!("unknown escape `\\{}`", char::from(other))));
            }
        }
        Ok(())
    }

    fn hex4(&mut self) -> Result<u32, Error> {
        let mut value: u32 = 0;
        for _ in 0..4 {
            let byte = self
                .peek()
                .ok_or_else(|| self.error("a short \\u escape"))?;
            let digit = char::from(byte)
                .to_digit(16)
                .ok_or_else(|| self.error("a non-hexadecimal digit in a \\u escape"))?;
            value = value
                .checked_mul(16)
                .and_then(|shifted| shifted.checked_add(digit))
                .ok_or_else(|| self.error("a \\u escape out of range"))?;
            self.bump();
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<Json, Error> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.bump();
        }
        let mut digits = 0_usize;
        while let Some(byte) = self.peek() {
            if byte.is_ascii_digit() {
                digits = digits.saturating_add(1);
                self.bump();
            } else if matches!(byte, b'.' | b'e' | b'E' | b'+' | b'-') {
                // Accepted by the grammar, kept in the lexeme, and rejected by
                // whichever field asks for it: there is no floating-point
                // field in this schema to accept it.
                self.bump();
            } else {
                break;
            }
        }
        if digits == 0 {
            return Err(self.error("expected a value"));
        }
        let slice = self
            .bytes
            .get(start..self.pos)
            .ok_or_else(|| self.error("a truncated number"))?;
        let lexeme = std::str::from_utf8(slice)
            .map_err(|_| self.error("invalid UTF-8 in a number"))?
            .to_owned();
        Ok(Json::Number(lexeme))
    }
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

fn write_string(text: &str, out: &mut String) {
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
                let _ignored = write!(out, "\\u{:04x}", u32::from(other));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}
