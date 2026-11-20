// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The grammar of JSON text, written once (S3's `vfyq2`).
//!
//! Three readers read a player's playbook: the codec's tree reader
//! ([`super::read`]), the codec's one-pass reader (`super::direct`), and
//! `plan-core`'s JSONC reader, which keeps every comment and every byte of
//! formatting. Each used to carry its own copy of RFC 8259, and the copies
//! parted: the JSONC reader refused a value nested inside exactly
//! [`MAX_DEPTH`] containers, which the codec took, and took a number with a
//! leading zero, which the codec refused. A file could then verify and fail to
//! render, or render and fail to verify. So the grammar lives here, and every
//! reader calls it:
//!
//! * [`parse`], the whole document's structure (objects, arrays, members,
//!   items, the nesting rule, duplicate keys, trailing content), with the
//!   space between tokens read by a [`Trivia`] the caller gives: the codec
//!   reads [`Whitespace`], and the JSONC reader whitespace and its comments.
//!   What it builds is the caller's ([`Build`]).
//! * The tokens, which `super::direct` calls one by one as it walks the
//!   schema: [`whitespace_end`], [`literal`], [`number`] and [`string`].
//!
//! Every refusal is made here, with one message and one byte offset, so two
//! readers that refuse a text refuse it at the same byte in the same words.
//!
//! # Bounds
//!
//! This is a parser of untrusted input (a playbook arrives over a socket).
//!
//! * **Nesting**: a value may stand inside at most [`MAX_DEPTH`] containers
//!   ([`may_nest`]); the recursion is bounded by it.
//! * **Work**: each byte is read a bounded number of times. A token is
//!   scanned once; a key is checked against the keys before it by a scan of at
//!   most [`LINEAR_KEYS`] and then by a sorted set, never by a scan of every
//!   key before it.
//! * **Memory**: what is built is the caller's and at most a constant times
//!   the text, a token or a key held once.
//! * **Length**: the text's own length is capped where it enters, by the
//!   boundary that holds it: a player's playbook at the verifier's
//!   `MAX_PLAYBOOK_CHARS`, a socket's message at the gateway's frame limit.
//!   This module reads texts that are not playbooks as well (the rules table,
//!   a save, a scenario), whose sizes those boundaries own.

use std::borrow::Cow;
use std::collections::BTreeSet;

use super::value::{Error, ErrorKind};

/// The most containers a value may stand inside: the codec's limit, and now
/// every reader's. Generous for a playbook, whose condition trees the
/// vocabulary caps at depth 4, and low enough that a hostile file cannot
/// exhaust the stack.
pub const MAX_DEPTH: usize = 64;

/// Whether a value standing inside `containers` containers may be read: the
/// one nesting rule. A document's root stands inside none.
#[must_use]
pub const fn may_nest(containers: usize) -> bool {
    containers <= MAX_DEPTH
}

/// How many members an object holds before its duplicate-key check keeps a
/// sorted set rather than scanning the keys one by one.
pub const LINEAR_KEYS: usize = 16;

/// JSON's whitespace: space, tab, line feed and carriage return, and nothing
/// else (no form feed, no vertical tab, no Unicode space).
#[must_use]
pub const fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

/// The byte after the run of whitespace that starts at `at`.
#[must_use]
pub fn whitespace_end(bytes: &[u8], at: usize) -> usize {
    let mut pos = at;
    let mut examined: usize = 1;
    while bytes.get(pos).copied().is_some_and(is_whitespace) {
        pos = pos.saturating_add(1);
        examined = examined.saturating_add(1);
    }
    super::work::add(examined);
    pos
}

/// The refusal of a comma with no member or item after it.
pub const TRAILING_COMMA: &str = "a trailing comma: JSON has no member or item after it";

/// What the grammar refused, and where.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Refusal {
    /// What kind of thing it is: [`ErrorKind::Grammar`], [`ErrorKind::Depth`]
    /// or [`ErrorKind::DuplicateField`].
    pub kind: ErrorKind,
    /// The byte the refusal is at.
    pub at: usize,
    /// English, and safe to show a player beside the offset.
    pub message: Cow<'static, str>,
}

impl Refusal {
    fn grammar(at: usize, message: impl Into<Cow<'static, str>>) -> Refusal {
        Refusal {
            kind: ErrorKind::Grammar,
            at,
            message: message.into(),
        }
    }
}

impl From<Refusal> for Error {
    fn from(refusal: Refusal) -> Error {
        Error {
            kind: refusal.kind,
            pointer: format!("/byte/{}", refusal.at),
            message: refusal.message.into_owned(),
        }
    }
}

/// The byte after `word`, which must start at `at`; refused at the first byte
/// that differs.
///
/// # Errors
///
/// A [`Refusal`] at the first byte that is not `word`'s.
pub fn literal(bytes: &[u8], at: usize, word: &'static str) -> Result<usize, Refusal> {
    metered(at, literal_unmetered(bytes, at, word))
}

fn literal_unmetered(bytes: &[u8], at: usize, word: &'static str) -> Result<usize, Refusal> {
    let mut pos = at;
    for expected in word.bytes() {
        if bytes.get(pos) != Some(&expected) {
            return Err(Refusal::grammar(pos, format!("expected `{word}`")));
        }
        pos = pos.saturating_add(1);
    }
    Ok(pos)
}

/// The byte after the number that starts at `at`, by RFC 8259's grammar: `-`?
/// then `0` or a non-zero digit and digits, then `.` and digits, then `e` or
/// `E`, a sign and digits, each part after the first optional. A fraction or
/// an exponent is a number here and refused by whichever field asks for an
/// integer: no field of this schema holds anything else.
///
/// # Errors
///
/// A [`Refusal`] at the byte that breaks the grammar.
pub fn number(bytes: &[u8], at: usize) -> Result<usize, Refusal> {
    metered(at, number_unmetered(bytes, at))
}

fn number_unmetered(bytes: &[u8], at: usize) -> Result<usize, Refusal> {
    let mut pos = at;
    let digit = |pos: usize| bytes.get(pos).is_some_and(u8::is_ascii_digit);
    let digits = |mut pos: usize| {
        while digit(pos) {
            pos = pos.saturating_add(1);
        }
        pos
    };
    if bytes.get(pos) == Some(&b'-') {
        pos = pos.saturating_add(1);
    }
    match bytes.get(pos) {
        Some(b'0') => {
            pos = pos.saturating_add(1);
            if digit(pos) {
                return Err(Refusal::grammar(pos, "a number with a leading zero"));
            }
        }
        Some(b'1'..=b'9') => pos = digits(pos),
        _ => return Err(Refusal::grammar(pos, "expected a value")),
    }
    if bytes.get(pos) == Some(&b'.') {
        pos = pos.saturating_add(1);
        if !digit(pos) {
            return Err(Refusal::grammar(pos, "a number's fraction without a digit"));
        }
        pos = digits(pos);
    }
    if matches!(bytes.get(pos), Some(b'e' | b'E')) {
        pos = pos.saturating_add(1);
        if matches!(bytes.get(pos), Some(b'+' | b'-')) {
            pos = pos.saturating_add(1);
        }
        if !digit(pos) {
            return Err(Refusal::grammar(pos, "a number's exponent without a digit"));
        }
        pos = digits(pos);
    }
    Ok(pos)
}

/// What a string token held, as [`string`] read it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Body {
    /// No escape: the text is the bytes between the quotes, as written.
    Plain,
    /// An escape: the text is what [`string`] unescaped into its buffer.
    Escaped,
}

/// The byte after the string token whose opening quote is at `at`, and what
/// it held.
///
/// When the string holds an escape and `unescaped` is given, its text is
/// written there (the buffer is cleared first); a string with no escape is
/// left to the caller to slice, so the common case allocates nothing. Give
/// `None` to check a string without keeping it.
///
/// # Errors
///
/// A [`Refusal`] at the byte that breaks the grammar: a missing quote, a raw
/// control character, an escape it does not know, a `\u` that is not four
/// hex digits, a lone surrogate, or the text ending inside the string.
pub fn string(
    text: &str,
    at: usize,
    unescaped: Option<&mut String>,
) -> Result<(usize, Body), Refusal> {
    let answer = string_unmetered(text, at, unescaped);
    let end = match &answer {
        Ok((end, _)) => *end,
        Err(refusal) => refusal.at,
    };
    super::work::add(end.saturating_sub(at).saturating_add(1));
    answer
}

fn string_unmetered(
    text: &str,
    at: usize,
    mut unescaped: Option<&mut String>,
) -> Result<(usize, Body), Refusal> {
    let bytes = text.as_bytes();
    if bytes.get(at) != Some(&b'"') {
        return Err(Refusal::grammar(at, "expected `\"`"));
    }
    let start = at.saturating_add(1);
    let mut pos = start;
    let mut body = Body::Plain;
    loop {
        // The run of bytes up to the next quote, backslash or control
        // character, in one piece. Each of those is ASCII, so a run never ends
        // inside a UTF-8 sequence.
        let run = pos;
        while let Some(&byte) = bytes.get(pos) {
            if byte == b'"' || byte == b'\\' || byte < 0x20 {
                break;
            }
            pos = pos.saturating_add(1);
        }
        if body == Body::Escaped
            && let Some(out) = unescaped.as_deref_mut()
        {
            let piece = text
                .get(run..pos)
                .ok_or_else(|| Refusal::grammar(run, "invalid UTF-8 in a string"))?;
            out.push_str(piece);
        }
        match bytes.get(pos) {
            None => return Err(Refusal::grammar(pos, "unterminated string")),
            Some(b'"') => return Ok((pos.saturating_add(1), body)),
            Some(b'\\') => {
                if body == Body::Plain {
                    body = Body::Escaped;
                    if let Some(out) = unescaped.as_deref_mut() {
                        out.clear();
                        let before = text
                            .get(start..pos)
                            .ok_or_else(|| Refusal::grammar(start, "invalid UTF-8 in a string"))?;
                        out.push_str(before);
                    }
                }
                pos = escape(bytes, pos.saturating_add(1), unescaped.as_deref_mut())?;
            }
            Some(_) => return Err(Refusal::grammar(pos, "a raw control character in a string")),
        }
    }
}

/// One escape, its backslash read: the byte after it, its character written
/// to `out` when given.
fn escape(bytes: &[u8], at: usize, out: Option<&mut String>) -> Result<usize, Refusal> {
    let Some(&byte) = bytes.get(at) else {
        return Err(Refusal::grammar(at, "the document ended inside an escape"));
    };
    let mut pos = at.saturating_add(1);
    let character = match byte {
        b'"' => '"',
        b'\\' => '\\',
        b'/' => '/',
        b'b' => '\u{8}',
        b'f' => '\u{c}',
        b'n' => '\n',
        b'r' => '\r',
        b't' => '\t',
        b'u' => {
            let first;
            (first, pos) = hex4(bytes, pos)?;
            let code = if (0xd800..0xdc00).contains(&first) {
                if bytes.get(pos) != Some(&b'\\') {
                    return Err(Refusal::grammar(pos, "expected `\\`"));
                }
                pos = pos.saturating_add(1);
                if bytes.get(pos) != Some(&b'u') {
                    return Err(Refusal::grammar(pos, "expected `u`"));
                }
                pos = pos.saturating_add(1);
                let second;
                (second, pos) = hex4(bytes, pos)?;
                if !(0xdc00..0xe000).contains(&second) {
                    return Err(Refusal::grammar(
                        pos,
                        "a high surrogate without a low surrogate",
                    ));
                }
                let high = first.saturating_sub(0xd800) << 10;
                let low = second.saturating_sub(0xdc00);
                0x1_0000_u32
                    .checked_add(high | low)
                    .ok_or_else(|| Refusal::grammar(pos, "a surrogate pair out of range"))?
            } else {
                first
            };
            char::from_u32(code)
                .ok_or_else(|| Refusal::grammar(pos, "not a Unicode scalar value"))?
        }
        other => {
            return Err(Refusal::grammar(
                pos,
                format!("unknown escape `\\{}`", char::from(other)),
            ));
        }
    };
    if let Some(out) = out {
        out.push(character);
    }
    Ok(pos)
}

/// Four hex digits at `at`: their value, and the byte after them.
fn hex4(bytes: &[u8], at: usize) -> Result<(u32, usize), Refusal> {
    let mut value: u32 = 0;
    let mut pos = at;
    for _ in 0..4 {
        let Some(&byte) = bytes.get(pos) else {
            return Err(Refusal::grammar(pos, "a short \\u escape"));
        };
        let digit = char::from(byte)
            .to_digit(16)
            .ok_or_else(|| Refusal::grammar(pos, "a non-hexadecimal digit in a \\u escape"))?;
        value = value
            .checked_mul(16)
            .and_then(|shifted| shifted.checked_add(digit))
            .ok_or_else(|| Refusal::grammar(pos, "a \\u escape out of range"))?;
        pos = pos.saturating_add(1);
    }
    Ok((value, pos))
}

/// What may stand between two tokens.
pub trait Trivia {
    /// The byte after the run of trivia that starts at `at`. It never
    /// refuses: it stops at the first byte that is not trivia, and the
    /// grammar refuses that byte if it does not belong.
    fn skip(&mut self, bytes: &[u8], at: usize) -> usize;
}

/// The codec's trivia: whitespace, and nothing else.
#[derive(Clone, Copy, Debug, Default)]
pub struct Whitespace;

impl Trivia for Whitespace {
    fn skip(&mut self, bytes: &[u8], at: usize) -> usize {
        whitespace_end(bytes, at)
    }
}

/// One member of an object, as [`parse`] read it: each piece of trivia
/// around it as written, the key as written and unescaped, and the value.
#[derive(Debug)]
pub struct Member<'t, V> {
    /// The trivia before the key.
    pub before: &'t str,
    /// The key as written, quotes and escapes included.
    pub key_raw: &'t str,
    /// The key, unescaped.
    pub key: String,
    /// The trivia between the key and the colon.
    pub pre_colon: &'t str,
    /// The trivia between the colon and the value.
    pub post_colon: &'t str,
    /// The value.
    pub value: V,
    /// The trivia after the value, before the comma or the brace.
    pub after: &'t str,
}

/// What a document is built into: the caller's, so that the codec builds a
/// plain tree and the JSONC reader a tree that keeps its trivia, from one
/// reading.
pub trait Build<'t> {
    /// A value.
    type Value;
    /// An object's members, as they are read.
    type Members;
    /// An array's items, as they are read.
    type Items;
    /// No members yet.
    fn members(&mut self) -> Self::Members;
    /// No items yet.
    fn items(&mut self) -> Self::Items;
    /// `null`.
    fn null(&mut self) -> Self::Value;
    /// `true` or `false`.
    fn boolean(&mut self, value: bool) -> Self::Value;
    /// A number, as its lexeme.
    fn number(&mut self, lexeme: &'t str) -> Self::Value;
    /// A string, as written (quotes and escapes included) and unescaped.
    fn string(&mut self, raw: &'t str, text: String) -> Self::Value;
    /// One member, read.
    fn member(&mut self, members: &mut Self::Members, member: Member<'t, Self::Value>);
    /// One item, read, with the trivia before and after it.
    fn item(
        &mut self,
        items: &mut Self::Items,
        before: &'t str,
        value: Self::Value,
        after: &'t str,
    );
    /// An object, its members read; `inner` is the trivia inside an empty one.
    fn object(&mut self, members: Self::Members, inner: &'t str) -> Self::Value;
    /// An array, its items read; `inner` is the trivia inside an empty one.
    fn array(&mut self, items: Self::Items, inner: &'t str) -> Self::Value;
}

/// A document: the trivia before its value, the value, and the trivia after.
#[derive(Debug)]
pub struct Document<'t, V> {
    /// The trivia before the value.
    pub prologue: &'t str,
    /// The value.
    pub value: V,
    /// The trivia after the value.
    pub epilogue: &'t str,
}

/// Reads one JSON document: one value with only trivia around it.
///
/// # Errors
///
/// The first byte that does not belong, as an [`Error`] at `/byte/NNN`:
/// a token the grammar refuses, a value nested past [`MAX_DEPTH`], a key
/// written twice in one object, or content after the value.
pub fn parse<'t, B: Build<'t>, T: Trivia>(
    text: &'t str,
    build: &mut B,
    trivia: &mut T,
) -> Result<Document<'t, B::Value>, Error> {
    let mut reader = Reader {
        text,
        bytes: text.as_bytes(),
        pos: 0,
        build,
        trivia,
        #[cfg(test)]
        comparisons: 0,
        #[cfg(test)]
        steps: 0,
    };
    reader.document().map_err(Error::from)
}

struct Reader<'t, 'b, B, T> {
    text: &'t str,
    bytes: &'t [u8],
    pos: usize,
    build: &'b mut B,
    trivia: &'b mut T,
    /// How many keys the duplicate-key check has compared one by one, for
    /// the test that holds it below quadratic.
    #[cfg(test)]
    comparisons: u64,
    /// How many times the reader has read a value or a run of trivia, for the
    /// test that holds its work to a constant per byte.
    #[cfg(test)]
    steps: u64,
}

impl<'t, B: Build<'t>, T: Trivia> Reader<'t, '_, B, T> {
    fn slice(&self, from: usize, to: usize) -> Result<&'t str, Refusal> {
        self.text
            .get(from..to)
            .ok_or_else(|| Refusal::grammar(from, "a token ended inside a character"))
    }

    fn skip(&mut self) -> Result<&'t str, Refusal> {
        #[cfg(test)]
        {
            self.steps = self.steps.saturating_add(1);
        }
        let start = self.pos;
        self.pos = self.trivia.skip(self.bytes, start);
        self.slice(start, self.pos)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn bump(&mut self) {
        self.pos = self.pos.saturating_add(1);
    }

    fn document(&mut self) -> Result<Document<'t, B::Value>, Refusal> {
        let prologue = self.skip()?;
        let value = self.value(0)?;
        let epilogue = self.skip()?;
        if self.pos < self.bytes.len() {
            return Err(Refusal::grammar(
                self.pos,
                "trailing content after the JSON value",
            ));
        }
        Ok(Document {
            prologue,
            value,
            epilogue,
        })
    }

    fn value(&mut self, depth: usize) -> Result<B::Value, Refusal> {
        #[cfg(test)]
        {
            self.steps = self.steps.saturating_add(1);
        }
        if !may_nest(depth) {
            return Err(Refusal {
                kind: ErrorKind::Depth,
                at: self.pos,
                message: Cow::Owned(format!("JSON nested deeper than {MAX_DEPTH} levels")),
            });
        }
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => {
                let start = self.pos;
                let mut unescaped = String::new();
                let (end, body) = string(self.text, start, Some(&mut unescaped))?;
                self.pos = end;
                let raw = self.slice(start, end)?;
                let text = match body {
                    Body::Plain => self
                        .slice(start.saturating_add(1), end.saturating_sub(1))?
                        .to_owned(),
                    Body::Escaped => unescaped,
                };
                Ok(self.build.string(raw, text))
            }
            Some(b't') => {
                self.pos = literal(self.bytes, self.pos, "true")?;
                Ok(self.build.boolean(true))
            }
            Some(b'f') => {
                self.pos = literal(self.bytes, self.pos, "false")?;
                Ok(self.build.boolean(false))
            }
            Some(b'n') => {
                self.pos = literal(self.bytes, self.pos, "null")?;
                Ok(self.build.null())
            }
            Some(_) => {
                let start = self.pos;
                self.pos = number(self.bytes, start)?;
                let lexeme = self.slice(start, self.pos)?;
                Ok(self.build.number(lexeme))
            }
            None => Err(Refusal::grammar(
                self.pos,
                "the document ended where a value was expected",
            )),
        }
    }

    fn object(&mut self, depth: usize) -> Result<B::Value, Refusal> {
        self.bump();
        let mut members = self.build.members();
        let mut keys: Vec<String> = Vec::new();
        // Past `LINEAR_KEYS` members, the keys so far as a sorted set: an
        // object of n members then costs O(n log n) comparisons, not the
        // O(n²) of comparing every key with every key before it.
        let mut sorted: Option<BTreeSet<String>> = None;
        let mut before = self.skip()?;
        if self.peek() == Some(b'}') {
            self.bump();
            return Ok(self.build.object(members, before));
        }
        loop {
            let key_at = self.pos;
            let mut unescaped = String::new();
            let (end, body) = string(self.text, key_at, Some(&mut unescaped))?;
            self.pos = end;
            let key_raw = self.slice(key_at, end)?;
            let key = match body {
                Body::Plain => self
                    .slice(key_at.saturating_add(1), end.saturating_sub(1))?
                    .to_owned(),
                Body::Escaped => unescaped,
            };
            let (seen, compared) = seen_before(&mut keys, &mut sorted, &key);
            // Counted for the test that holds the check below quadratic, and
            // nothing else.
            #[cfg(test)]
            {
                self.comparisons = self.comparisons.saturating_add(compared);
            }
            #[cfg(not(test))]
            let _: u64 = compared;
            if seen {
                // At the key's own opening quote: the byte a reader would look at.
                return Err(Refusal {
                    kind: ErrorKind::DuplicateField,
                    at: key_at,
                    message: Cow::Owned(format!("duplicate key `{key}`")),
                });
            }
            let pre_colon = self.skip()?;
            if self.peek() != Some(b':') {
                return Err(Refusal::grammar(self.pos, "expected `:`"));
            }
            self.bump();
            let post_colon = self.skip()?;
            let value = self.value(depth.saturating_add(1))?;
            let after = self.skip()?;
            self.build.member(
                &mut members,
                Member {
                    before,
                    key_raw,
                    key,
                    pre_colon,
                    post_colon,
                    value,
                    after,
                },
            );
            match self.peek() {
                Some(b',') => {
                    self.bump();
                    before = self.skip()?;
                    if self.peek() == Some(b'}') {
                        return Err(Refusal::grammar(self.pos, TRAILING_COMMA));
                    }
                }
                Some(b'}') => {
                    self.bump();
                    return Ok(self.build.object(members, ""));
                }
                _ => {
                    return Err(Refusal::grammar(self.pos, "expected `,` or `}`"));
                }
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<B::Value, Refusal> {
        self.bump();
        let mut items = self.build.items();
        let mut before = self.skip()?;
        if self.peek() == Some(b']') {
            self.bump();
            return Ok(self.build.array(items, before));
        }
        loop {
            let value = self.value(depth.saturating_add(1))?;
            let after = self.skip()?;
            self.build.item(&mut items, before, value, after);
            match self.peek() {
                Some(b',') => {
                    self.bump();
                    before = self.skip()?;
                    if self.peek() == Some(b']') {
                        return Err(Refusal::grammar(self.pos, TRAILING_COMMA));
                    }
                }
                Some(b']') => {
                    self.bump();
                    return Ok(self.build.array(items, ""));
                }
                _ => {
                    return Err(Refusal::grammar(self.pos, "expected `,` or `]`"));
                }
            }
        }
    }
}

/// Whether an object already holds `key`, its keys so far being `keys`, and
/// how many keys were compared one by one to say so; `key` joins them.
///
/// They are scanned one by one while there are fewer than [`LINEAR_KEYS`], as
/// in almost every object a playbook has, and looked up in `sorted` once there
/// are more, which this builds the first time and keeps up to date. Either way
/// the answer is the same.
fn seen_before(
    keys: &mut Vec<String>,
    sorted: &mut Option<BTreeSet<String>>,
    key: &str,
) -> (bool, u64) {
    if sorted.is_none() && keys.len() < LINEAR_KEYS {
        let mut compared: u64 = 0;
        let seen = keys.iter().any(|name| {
            compared = compared.saturating_add(1);
            name == key
        });
        if !seen {
            keys.push(key.to_owned());
        }
        return (seen, compared);
    }
    let set = sorted.get_or_insert_with(|| keys.drain(..).collect());
    (!set.insert(key.to_owned()), 0)
}

/// [`parse`] over plain whitespace, with how many key comparisons its
/// duplicate-key check made and how many values and runs of trivia it read.
#[cfg(test)]
pub(crate) fn parse_counting<'t, B: Build<'t>>(
    text: &'t str,
    build: &mut B,
) -> (Result<B::Value, Error>, u64, u64) {
    let mut trivia = Whitespace;
    let mut reader = Reader {
        text,
        bytes: text.as_bytes(),
        pos: 0,
        build,
        trivia: &mut trivia,
        comparisons: 0,
        steps: 0,
    };
    let value = reader
        .document()
        .map(|document| document.value)
        .map_err(Error::from);
    (value, reader.comparisons, reader.steps)
}

/// A token's answer, its examined bytes counted on the work meter
/// ([`super::work`]): from `at` to the byte after it, or to the byte it was
/// refused at.
fn metered(at: usize, answer: Result<usize, Refusal>) -> Result<usize, Refusal> {
    let end = match &answer {
        Ok(end) => *end,
        Err(refusal) => refusal.at,
    };
    super::work::add(end.saturating_sub(at).saturating_add(1));
    answer
}
