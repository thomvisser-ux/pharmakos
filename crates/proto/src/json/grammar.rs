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
use std::cell::Cell;
use std::collections::BTreeSet;
use std::fmt::{self, Write as _};

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

/// A text as the grammar's scanners read it (S3's `vfyq2`, fix pass 1, item
/// 1): a byte at a time ([`Scan::get`]), or a piece already read
/// ([`Scan::slice`]), each byte counted, and no other way in. The count goes
/// to the work meter ([`super::work`]) when the scan is dropped, so every byte
/// a scanner looks at, a second look included, is on the meter: a scanner
/// that rereads the text fails the linearity tests rather than only running
/// slower, which a meter fed by the scanners' own reports could not promise.
#[derive(Debug)]
pub struct Scan<'t> {
    text: &'t str,
    read: Cell<usize>,
}

impl<'t> Scan<'t> {
    /// A scan of `text`, nothing read yet.
    #[must_use]
    pub const fn new(text: &'t str) -> Scan<'t> {
        Scan {
            text,
            read: Cell::new(0),
        }
    }

    /// The byte at `at`, counted.
    #[must_use]
    pub fn get(&self, at: usize) -> Option<u8> {
        self.count(1);
        self.text.as_bytes().get(at).copied()
    }

    /// The text from `from` to `to`, every byte of it counted; `None` past
    /// the end or off a character boundary.
    #[must_use]
    pub fn slice(&self, from: usize, to: usize) -> Option<&'t str> {
        self.count(to.saturating_sub(from));
        self.text.get(from..to)
    }

    /// How many bytes the text has.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.text.len()
    }

    /// Whether the text is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    fn count(&self, bytes: usize) {
        self.read.set(self.read.get().saturating_add(bytes));
    }
}

impl Drop for Scan<'_> {
    fn drop(&mut self) {
        super::work::add(self.read.get());
    }
}

/// The byte after the run of whitespace that starts at `at`.
#[must_use]
pub fn whitespace_end(scan: &Scan<'_>, at: usize) -> usize {
    let mut pos = at;
    while scan.get(pos).is_some_and(is_whitespace) {
        pos = pos.saturating_add(1);
    }
    pos
}

/// The refusal of a comma with no member or item after it.
pub const TRAILING_COMMA: &str = "a trailing comma: JSON has no member or item after it";

// The grammar's other refusal words (S3's `vfyq2`, fix pass 1, item 13): one
// table beside `TRAILING_COMMA`, so every reader built on this grammar says one
// thing for one refusal and a test names a refusal by its constant. The words
// that carry a value (`expected `true``, an unknown escape, a duplicate key, the
// depth) are written where the value is known, through `quoted`.
/// Where a value must start and none does.
pub const EXPECTED_VALUE: &str = "expected a value";
/// The text ended where a value must start.
pub const ENDED_BEFORE_VALUE: &str = "the document ended where a value was expected";
/// Anything but trivia after the document's one value.
pub const TRAILING_CONTENT: &str = "trailing content after the JSON value";
/// A member's key not followed by `:`.
pub const EXPECTED_COLON: &str = "expected `:`";
/// An object member not followed by `,` or `}`.
pub const EXPECTED_COMMA_OR_BRACE: &str = "expected `,` or `}`";
/// An array item not followed by `,` or `]`.
pub const EXPECTED_COMMA_OR_BRACKET: &str = "expected `,` or `]`";
/// A `-` with no digit after it.
pub const NUMBER_WITHOUT_DIGIT: &str = "a number needs at least one digit";
/// `01`, which RFC 8259 does not allow.
pub const LEADING_ZERO: &str = "a number with a leading zero";
/// `1.` or `1.e3`.
pub const FRACTION_WITHOUT_DIGIT: &str = "a number's fraction without a digit";
/// `1e` or `1e+`.
pub const EXPONENT_WITHOUT_DIGIT: &str = "a number's exponent without a digit";
/// Where a string (a key) must start and none does.
pub const EXPECTED_QUOTE: &str = "expected `\"`";
/// A string the text ends inside.
pub const UNTERMINATED_STRING: &str = "unterminated string";
/// A control character written raw inside a string.
pub const RAW_CONTROL: &str = "a raw control character in a string";
/// Bytes inside a string that are not UTF-8.
pub const INVALID_UTF8: &str = "invalid UTF-8 in a string";
/// A `\` the text ends after.
pub const ENDED_IN_ESCAPE: &str = "the document ended inside an escape";
/// A high surrogate's escape not followed by another escape.
pub const EXPECTED_BACKSLASH: &str = "expected `\\`";
/// A high surrogate's escape followed by an escape that is not `\u`.
pub const EXPECTED_U: &str = "expected `u`";
/// A high surrogate followed by a `\u` that is not a low one.
pub const LONE_HIGH_SURROGATE: &str = "a high surrogate without a low surrogate";
/// A surrogate pair past U+10FFFF.
pub const SURROGATE_OUT_OF_RANGE: &str = "a surrogate pair out of range";
/// A `\u` escape naming a lone surrogate or no character.
pub const NOT_A_SCALAR: &str = "not a Unicode scalar value";
/// A `\u` with fewer than four digits before the text ends.
pub const SHORT_U_ESCAPE: &str = "a short \\u escape";
/// A `\u` escape with a digit that is not hexadecimal.
pub const NON_HEX_DIGIT: &str = "a non-hexadecimal digit in a \\u escape";
/// A `\u` escape's four digits not making a UTF-16 unit.
pub const U_ESCAPE_OUT_OF_RANGE: &str = "a \\u escape out of range";
/// A token whose end falls inside a character.
pub const TOKEN_SPLITS_CHARACTER: &str = "a token ended inside a character";

/// What the grammar refused, and where.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Refusal {
    /// What kind of thing it is: [`ErrorKind::Grammar`], [`ErrorKind::Depth`]
    /// or [`ErrorKind::DuplicateField`].
    pub kind: ErrorKind,
    /// The byte the refusal is at.
    pub at: usize,
    /// English, and safe to show a player beside the offset: [`Said`], so
    /// only words and quoted text make one (S3's `vfyq2`, fix pass 3, item
    /// M2 (d)).
    pub message: Said,
}

impl Refusal {
    fn grammar(at: usize, message: impl Into<Said>) -> Refusal {
        Refusal {
            kind: ErrorKind::Grammar,
            at,
            message: message.into(),
        }
    }
}

impl From<Refusal> for Error {
    fn from(refusal: Refusal) -> Error {
        Error::at(
            refusal.kind,
            &format!("/byte/{}", refusal.at),
            refusal.message,
        )
    }
}

/// The byte after `word`, which must start at `at`; refused at the first byte
/// that differs.
///
/// # Errors
///
/// A [`Refusal`] at the first byte that is not `word`'s.
pub fn literal(scan: &Scan<'_>, at: usize, word: &'static str) -> Result<usize, Refusal> {
    let mut pos = at;
    for expected in word.bytes() {
        if scan.get(pos) != Some(expected) {
            return Err(Refusal::grammar(pos, crate::said!("expected `{}`", word)));
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
pub fn number(scan: &Scan<'_>, at: usize) -> Result<usize, Refusal> {
    let mut pos = at;
    let digit = |pos: usize| scan.get(pos).is_some_and(|byte| byte.is_ascii_digit());
    let digits = |mut pos: usize| {
        while digit(pos) {
            pos = pos.saturating_add(1);
        }
        pos
    };
    let signed = scan.get(pos) == Some(b'-');
    if signed {
        pos = pos.saturating_add(1);
    }
    match scan.get(pos) {
        Some(b'0') => {
            pos = pos.saturating_add(1);
            if digit(pos) {
                return Err(Refusal::grammar(pos, LEADING_ZERO));
            }
        }
        Some(b'1'..=b'9') => pos = digits(pos),
        // A `-` makes it a number: what it lacks is the digit (fix pass 2,
        // item D5).
        _ if signed => return Err(Refusal::grammar(pos, NUMBER_WITHOUT_DIGIT)),
        _ => return Err(Refusal::grammar(pos, EXPECTED_VALUE)),
    }
    if scan.get(pos) == Some(b'.') {
        pos = pos.saturating_add(1);
        if !digit(pos) {
            return Err(Refusal::grammar(pos, FRACTION_WITHOUT_DIGIT));
        }
        pos = digits(pos);
    }
    if matches!(scan.get(pos), Some(b'e' | b'E')) {
        pos = pos.saturating_add(1);
        if matches!(scan.get(pos), Some(b'+' | b'-')) {
            pos = pos.saturating_add(1);
        }
        if !digit(pos) {
            return Err(Refusal::grammar(pos, EXPONENT_WITHOUT_DIGIT));
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
    scan: &Scan<'_>,
    at: usize,
    mut unescaped: Option<&mut String>,
) -> Result<(usize, Body), Refusal> {
    if scan.get(at) != Some(b'"') {
        return Err(Refusal::grammar(at, EXPECTED_QUOTE));
    }
    let start = at.saturating_add(1);
    let mut pos = start;
    let mut body = Body::Plain;
    loop {
        // The run of bytes up to the next quote, backslash or control
        // character, in one piece. Each of those is ASCII, so a run never ends
        // inside a UTF-8 sequence.
        let run = pos;
        while let Some(byte) = scan.get(pos) {
            if byte == b'"' || byte == b'\\' || byte < 0x20 {
                break;
            }
            pos = pos.saturating_add(1);
        }
        if body == Body::Escaped
            && let Some(out) = unescaped.as_deref_mut()
        {
            let piece = scan
                .slice(run, pos)
                .ok_or_else(|| Refusal::grammar(run, INVALID_UTF8))?;
            out.push_str(piece);
        }
        match scan.get(pos) {
            None => return Err(Refusal::grammar(pos, UNTERMINATED_STRING)),
            Some(b'"') => return Ok((pos.saturating_add(1), body)),
            Some(b'\\') => {
                if body == Body::Plain {
                    body = Body::Escaped;
                    if let Some(out) = unescaped.as_deref_mut() {
                        out.clear();
                        let before = scan
                            .slice(start, pos)
                            .ok_or_else(|| Refusal::grammar(start, INVALID_UTF8))?;
                        out.push_str(before);
                    }
                }
                pos = escape(scan, pos, unescaped.as_deref_mut())?;
            }
            Some(_) => return Err(Refusal::grammar(pos, RAW_CONTROL)),
        }
    }
}

/// One escape, whose backslash is at `backslash`: the byte after it, its
/// character written to `out` when given. A refusal of the escape points at
/// the first byte of what is wrong (fix pass 2, item D5): the backslash of an
/// escape that is wrong as a whole, the backslash of the second escape of a
/// surrogate pair that is not one.
fn escape(scan: &Scan<'_>, backslash: usize, out: Option<&mut String>) -> Result<usize, Refusal> {
    let at = backslash.saturating_add(1);
    let Some(byte) = scan.get(at) else {
        return Err(Refusal::grammar(at, ENDED_IN_ESCAPE));
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
            (first, pos) = hex4(scan, pos)?;
            let code = if (0xd800..0xdc00).contains(&first) {
                let second_backslash = pos;
                if scan.get(pos) != Some(b'\\') {
                    return Err(Refusal::grammar(pos, EXPECTED_BACKSLASH));
                }
                pos = pos.saturating_add(1);
                if scan.get(pos) != Some(b'u') {
                    return Err(Refusal::grammar(pos, EXPECTED_U));
                }
                pos = pos.saturating_add(1);
                let second;
                (second, pos) = hex4(scan, pos)?;
                if !(0xdc00..0xe000).contains(&second) {
                    return Err(Refusal::grammar(second_backslash, LONE_HIGH_SURROGATE));
                }
                let high = first.saturating_sub(0xd800) << 10;
                let low = second.saturating_sub(0xdc00);
                0x1_0000_u32
                    .checked_add(high | low)
                    .ok_or_else(|| Refusal::grammar(backslash, SURROGATE_OUT_OF_RANGE))?
            } else {
                first
            };
            char::from_u32(code).ok_or_else(|| Refusal::grammar(backslash, NOT_A_SCALAR))?
        }
        _ => {
            // The escaped character as a character, not its first byte: a
            // multi-byte one is quoted whole; the refusal is at the backslash
            // the escape begins with.
            let Some(other) = utf8_width(byte)
                .and_then(|width| scan.slice(at, at.saturating_add(width)))
                .and_then(|piece| piece.chars().next())
            else {
                return Err(Refusal::grammar(pos, INVALID_UTF8));
            };
            return Err(Refusal::grammar(
                backslash,
                crate::said!(
                    "unknown escape `\\{}`",
                    quoted(other.encode_utf8(&mut [0; 4]))
                ),
            ));
        }
    };
    if let Some(out) = out {
        out.push(character);
    }
    Ok(pos)
}

/// Four hex digits at `at`: their value, and the byte after them.
fn hex4(scan: &Scan<'_>, at: usize) -> Result<(u32, usize), Refusal> {
    let mut value: u32 = 0;
    let mut pos = at;
    for _ in 0..4 {
        let Some(byte) = scan.get(pos) else {
            return Err(Refusal::grammar(pos, SHORT_U_ESCAPE));
        };
        let digit = char::from(byte)
            .to_digit(16)
            .ok_or_else(|| Refusal::grammar(pos, NON_HEX_DIGIT))?;
        value = value
            .checked_mul(16)
            .and_then(|shifted| shifted.checked_add(digit))
            .ok_or_else(|| Refusal::grammar(pos, U_ESCAPE_OUT_OF_RANGE))?;
        pos = pos.saturating_add(1);
    }
    Ok((value, pos))
}

/// What may stand between two tokens.
pub trait Trivia {
    /// The byte after the run of trivia that starts at `at`. It never
    /// refuses: it stops at the first byte that is not trivia, and the
    /// grammar refuses that byte if it does not belong.
    fn skip(&mut self, scan: &Scan<'_>, at: usize) -> usize;
}

/// The codec's trivia: whitespace, and nothing else.
#[derive(Clone, Copy, Debug, Default)]
pub struct Whitespace;

impl Trivia for Whitespace {
    fn skip(&mut self, scan: &Scan<'_>, at: usize) -> usize {
        whitespace_end(scan, at)
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
    parse_refusing(text, build, trivia).map_err(Error::from)
}

/// [`parse`], answering the [`Refusal`] itself: its byte as a number, for a
/// caller that must know where the text stopped being JSON (`plan-core`'s
/// hand-over, which blanks comments up to that byte and no further).
///
/// # Errors
///
/// As [`parse`].
pub fn parse_refusing<'t, B: Build<'t>, T: Trivia>(
    text: &'t str,
    build: &mut B,
    trivia: &mut T,
) -> Result<Document<'t, B::Value>, Refusal> {
    let mut reader = Reader {
        text,
        scan: Scan::new(text),
        pos: 0,
        build,
        trivia,
        #[cfg(test)]
        comparisons: 0,
        #[cfg(test)]
        steps: 0,
    };
    reader.document()
}

struct Reader<'t, 'b, B, T> {
    /// The text, for slicing out what the scan has read.
    text: &'t str,
    scan: Scan<'t>,
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
            .ok_or_else(|| Refusal::grammar(from, TOKEN_SPLITS_CHARACTER))
    }

    fn skip(&mut self) -> Result<&'t str, Refusal> {
        #[cfg(test)]
        {
            self.steps = self.steps.saturating_add(1);
        }
        let start = self.pos;
        self.pos = self.trivia.skip(&self.scan, start);
        self.slice(start, self.pos)
    }

    fn peek(&self) -> Option<u8> {
        self.scan.get(self.pos)
    }

    fn bump(&mut self) {
        self.pos = self.pos.saturating_add(1);
    }

    fn document(&mut self) -> Result<Document<'t, B::Value>, Refusal> {
        let prologue = self.skip()?;
        let value = self.value(0)?;
        let epilogue = self.skip()?;
        if self.pos < self.scan.len() {
            return Err(Refusal::grammar(self.pos, TRAILING_CONTENT));
        }
        Ok(Document {
            prologue,
            value,
            epilogue,
        })
    }

    /// One value. This, [`Reader::object`] and [`Reader::array`] are the
    /// recursion, one frame of each per container, so they hold only what a
    /// level needs: a scalar, a key and the depth refusal are read in frames
    /// of their own that are gone before the next level starts (S3's `vfyq2`,
    /// fix pass 2, item E).
    fn value(&mut self, depth: usize) -> Result<B::Value, Refusal> {
        #[cfg(test)]
        {
            self.steps = self.steps.saturating_add(1);
        }
        if !may_nest(depth) {
            return Err(self.too_deep());
        }
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            _ => self.scalar(),
        }
    }

    /// The refusal of a value past [`MAX_DEPTH`] containers.
    #[cold]
    #[inline(never)]
    fn too_deep(&self) -> Refusal {
        Refusal {
            kind: ErrorKind::Depth,
            at: self.pos,
            message: crate::said!("JSON nested deeper than {} levels", MAX_DEPTH),
        }
    }

    /// A value that is not a container.
    #[inline(never)]
    fn scalar(&mut self) -> Result<B::Value, Refusal> {
        match self.peek() {
            Some(b'"') => {
                let start = self.pos;
                let mut unescaped = String::new();
                let (end, body) = string(&self.scan, start, Some(&mut unescaped))?;
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
                self.pos = literal(&self.scan, self.pos, "true")?;
                Ok(self.build.boolean(true))
            }
            Some(b'f') => {
                self.pos = literal(&self.scan, self.pos, "false")?;
                Ok(self.build.boolean(false))
            }
            Some(b'n') => {
                self.pos = literal(&self.scan, self.pos, "null")?;
                Ok(self.build.null())
            }
            Some(_) => {
                let start = self.pos;
                self.pos = number(&self.scan, start)?;
                let lexeme = self.slice(start, self.pos)?;
                Ok(self.build.number(lexeme))
            }
            None => Err(Refusal::grammar(self.pos, ENDED_BEFORE_VALUE)),
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
            let (key_raw, key) = self.key(&mut keys, &mut sorted)?;
            let pre_colon = self.skip()?;
            if self.peek() != Some(b':') {
                return Err(Refusal::grammar(self.pos, EXPECTED_COLON));
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
                    return Err(Refusal::grammar(self.pos, EXPECTED_COMMA_OR_BRACE));
                }
            }
        }
    }

    /// One member's key, refused when the object already holds it.
    #[inline(never)]
    fn key(
        &mut self,
        keys: &mut Vec<String>,
        sorted: &mut Option<BTreeSet<String>>,
    ) -> Result<(&'t str, String), Refusal> {
        let key_at = self.pos;
        let mut unescaped = String::new();
        let (end, body) = string(&self.scan, key_at, Some(&mut unescaped))?;
        self.pos = end;
        let key_raw = self.slice(key_at, end)?;
        let key = match body {
            Body::Plain => self
                .slice(key_at.saturating_add(1), end.saturating_sub(1))?
                .to_owned(),
            Body::Escaped => unescaped,
        };
        let (seen, compared) = seen_before(keys, sorted, &key);
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
                message: crate::said!("duplicate key `{}`", quoted(&key)),
            });
        }
        Ok((key_raw, key))
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
                    return Err(Refusal::grammar(self.pos, EXPECTED_COMMA_OR_BRACKET));
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
        scan: Scan::new(text),
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

/// How many bytes the UTF-8 sequence led by `lead` takes; `None` for a byte
/// that leads none.
const fn utf8_width(lead: u8) -> Option<usize> {
    match lead {
        0x00..=0x7f => Some(1),
        0xc0..=0xdf => Some(2),
        0xe0..=0xef => Some(3),
        0xf0..=0xf7 => Some(4),
        _ => None,
    }
}

/// The most characters of a player's text a refusal quotes back (S3's
/// `vfyq2`, fix pass 1, item 5): enough to recognise a key, a value or a
/// lexeme, and never the whole of a 200 KB one.
pub const MAX_QUOTED_CHARS: usize = 64;

/// The most characters an engine error is shown with, the rest shown as `…`
/// (S3's `vfyq2`, fix pass 5, item L4: [`Said::engine_error`] is capped like
/// [`quoted`]). Room for the longest words a declared type writes: a codec
/// refusal's pointer, at most [`MAX_DEPTH`] + 1 tokens, each [`quoted`]
/// (at most [`MAX_QUOTED_CHARS`] characters, each escaped in at most twelve,
/// and a `…` and a `/`), and a sentence of up to 1,024 characters around it
/// (a host path, a field, a value).
pub const MAX_ENGINE_CHARS: usize = (MAX_DEPTH + 1) * (MAX_QUOTED_CHARS * 12 + 2) + 1024;

/// The Unicode version [`HIDDEN`]'s general categories were taken from.
pub const HIDDEN_UNICODE_VERSION: &str = "16.0.0";

/// The Unicode version [`HIDDEN`]'s `Default_Ignorable_Code_Point` ranges were
/// taken from.
pub const HIDDEN_DEFAULT_IGNORABLE_VERSION: &str = "15.0.0";

/// Every character a player's text is never shown raw in (S3's `vfyq2`, fix
/// pass 3, item M3, and fix pass 4, item 2), as data: the union of
///
/// * General Category Cc (the C0 and C1 control characters, DEL among them),
///   Cf (format characters: the direction marks, embeddings, overrides and
///   isolates that reorder text, the zero-width characters that hide it, the
///   tag characters that can spell a message nobody sees), Zl and Zp (the line
///   and paragraph separators, which break a one-line message), in Unicode
///   [`HIDDEN_UNICODE_VERSION`], from Python 3.14.4's `unicodedata`
///   (`unidata_version` 16.0.0) under WSL2 on the machine that made this;
/// * the noncharacters (U+FDD0 to U+FDEF and the last two code points of
///   every plane), by the standard's definition;
/// * `Default_Ignorable_Code_Point` (the variation selectors, the Hangul
///   fillers, U+034F, the Mongolian free variation selectors and the
///   unassigned code points the standard reserves as ignorable, among them),
///   in Unicode [`HIDDEN_DEFAULT_IGNORABLE_VERSION`], from Perl 5.40.1's
///   `unicore/lib/DI/Y.pl` under WSL2 on the same machine, read offline.
///   The property's reserved ranges are stable by the standard's policy (a
///   code point reserved as default-ignorable stays so); no data on this
///   machine carries the property for 16.0, so a change 16.0 made, if any, is
///   not shown here.
///
/// 44 ranges, ascending, disjoint and merged, 4339 code points; each row's
/// comment names what puts it in (`DI` for the property). Only the ranges are
/// committed, which are facts, not either data file's text.
/// `tests/grammar.rs` holds [`quoted`] to this table over every `char`.
pub const HIDDEN: &[(char, char)] = &[
    ('\u{0000}', '\u{001F}'),     // Cc
    ('\u{007F}', '\u{009F}'),     // Cc
    ('\u{00AD}', '\u{00AD}'),     // Cf, DI
    ('\u{034F}', '\u{034F}'),     // DI
    ('\u{0600}', '\u{0605}'),     // Cf
    ('\u{061C}', '\u{061C}'),     // Cf, DI
    ('\u{06DD}', '\u{06DD}'),     // Cf
    ('\u{070F}', '\u{070F}'),     // Cf
    ('\u{0890}', '\u{0891}'),     // Cf
    ('\u{08E2}', '\u{08E2}'),     // Cf
    ('\u{115F}', '\u{1160}'),     // DI
    ('\u{17B4}', '\u{17B5}'),     // DI
    ('\u{180B}', '\u{180F}'),     // Cf, DI
    ('\u{200B}', '\u{200F}'),     // Cf, DI
    ('\u{2028}', '\u{202E}'),     // Cf, DI, Zl, Zp
    ('\u{2060}', '\u{206F}'),     // Cf, DI
    ('\u{3164}', '\u{3164}'),     // DI
    ('\u{FDD0}', '\u{FDEF}'),     // noncharacter
    ('\u{FE00}', '\u{FE0F}'),     // DI
    ('\u{FEFF}', '\u{FEFF}'),     // Cf, DI
    ('\u{FFA0}', '\u{FFA0}'),     // DI
    ('\u{FFF0}', '\u{FFFB}'),     // Cf, DI
    ('\u{FFFE}', '\u{FFFF}'),     // noncharacter
    ('\u{110BD}', '\u{110BD}'),   // Cf
    ('\u{110CD}', '\u{110CD}'),   // Cf
    ('\u{13430}', '\u{1343F}'),   // Cf
    ('\u{1BCA0}', '\u{1BCA3}'),   // Cf, DI
    ('\u{1D173}', '\u{1D17A}'),   // Cf, DI
    ('\u{1FFFE}', '\u{1FFFF}'),   // noncharacter
    ('\u{2FFFE}', '\u{2FFFF}'),   // noncharacter
    ('\u{3FFFE}', '\u{3FFFF}'),   // noncharacter
    ('\u{4FFFE}', '\u{4FFFF}'),   // noncharacter
    ('\u{5FFFE}', '\u{5FFFF}'),   // noncharacter
    ('\u{6FFFE}', '\u{6FFFF}'),   // noncharacter
    ('\u{7FFFE}', '\u{7FFFF}'),   // noncharacter
    ('\u{8FFFE}', '\u{8FFFF}'),   // noncharacter
    ('\u{9FFFE}', '\u{9FFFF}'),   // noncharacter
    ('\u{AFFFE}', '\u{AFFFF}'),   // noncharacter
    ('\u{BFFFE}', '\u{BFFFF}'),   // noncharacter
    ('\u{CFFFE}', '\u{CFFFF}'),   // noncharacter
    ('\u{DFFFE}', '\u{E0FFF}'),   // Cf, DI, noncharacter
    ('\u{EFFFE}', '\u{EFFFF}'),   // noncharacter
    ('\u{FFFFE}', '\u{FFFFF}'),   // noncharacter
    ('\u{10FFFE}', '\u{10FFFF}'), // noncharacter
];

/// Whether `character` is shown escaped rather than as itself: in
/// [`HIDDEN`], by binary search over its ascending ranges.
fn shown_escaped(character: char) -> bool {
    HIDDEN
        .binary_search_by(|(low, high)| {
            if *high < character {
                std::cmp::Ordering::Less
            } else if *low > character {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// A player's text as a refusal or a diagnostic shows it, made only by
/// [`quoted`] (S3's `vfyq2`, fix pass 2, item B): what a message builder
/// takes where player text goes, so a raw string cannot get there.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Quoted(String);

impl Quoted {
    /// The text, as shown.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Quoted {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A player's text, as a refusal or a diagnostic quotes it back: control
/// characters (and DEL) and the characters of [`HIDDEN`] written as
/// `\uXXXX`, so a key holding ESC or BEL cannot drive the terminal or the
/// client that shows the message and one holding U+202E cannot reorder it,
/// and at most [`MAX_QUOTED_CHARS`] characters, the rest shown as `…`. Every
/// quote of player text in the codec's refusals, the verifier's diagnostics
/// and plan-core's refusals goes through here, so there is one rule.
#[must_use]
pub fn quoted(text: &str) -> Quoted {
    shown(text, Some(MAX_QUOTED_CHARS))
}

/// A player's text as prose reads it back (S3's `vfyq2`, fix pass 3, item M2
/// (c)): [`quoted`]'s escaping, with no cap. The prose is the player's own
/// playbook read back to them, a title, a label, a note, so it is shown
/// whole; every character of [`HIDDEN`] is still escaped, so nothing in it
/// can break a line, reorder the text around it or hide.
#[must_use]
pub fn escaped(text: &str) -> Quoted {
    shown(text, None)
}

/// [`quoted`] and [`escaped`]: every character of [`HIDDEN`] as `\uXXXX`,
/// and at most `cap` characters, the rest shown as `…`.
fn shown(text: &str, cap: Option<usize>) -> Quoted {
    let budget = cap.map_or(text.len(), |cap| text.len().min(cap.saturating_mul(6)));
    let mut out = String::with_capacity(budget);
    for (index, character) in text.chars().enumerate() {
        if Some(index) == cap {
            out.push('…');
            break;
        }
        if shown_escaped(character) {
            for unit in character.encode_utf16(&mut [0; 2]) {
                let _infallible = write!(out, "\\u{unit:04x}");
            }
        } else {
            out.push(character);
        }
    }
    Quoted(out)
}

/// A JSON Pointer as a line shows it (S3's `vfyq2`, fix pass 2b): each
/// reference token [`quoted`] on its own, joined by `/`.
///
/// A pointer is data a reader takes back off the line (the headless watch
/// check, a person searching the file for the field), and every token the
/// verifier builds from the schema's names and array indices is far shorter
/// than [`MAX_QUOTED_CHARS`] and holds no character [`quoted`] escapes, so
/// such a pointer is shown exactly, however deep it is. Only a token that is
/// the player's own text (an unknown key) can hold a control character, a
/// character of [`HIDDEN`] or 200 KB, and that token alone is escaped and
/// capped. A pointer has at most `MAX_DEPTH` + 1 tokens, so the line is
/// bounded too. Quoting the pointer whole, as fix pass 2 did, cut every
/// pointer past 64 characters, schema names and all.
#[must_use]
pub fn quoted_pointer(pointer: &str) -> Quoted {
    let mut out = String::with_capacity(pointer.len().min(MAX_QUOTED_CHARS.saturating_mul(6)));
    for (index, token) in pointer.split('/').enumerate() {
        if index > 0 {
            out.push('/');
        }
        out.push_str(quoted(token).as_str());
    }
    Quoted(out)
}

/// One piece of text a player is shown: static words, a number, or player
/// text [`quoted`] (S3's `vfyq2`, fix pass 2, item B). The verifier's
/// diagnostic and Fix-title builders and plan-core's refusals take this, and
/// there is no way to make one from a `String`, so player text that skipped
/// the quoting rule does not compile. Several pieces are composed with
/// [`crate::said`], whose every argument is one.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Said(Cow<'static, str>);

impl Said {
    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The text, owned.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0.into_owned()
    }

    /// An engine error's own words, for a type that has declared them
    /// [`EngineWords`]: its `Display` composes static words, numbers and text
    /// this workspace made, never text read from a player's file.
    #[must_use]
    pub fn engine_error(error: &impl EngineWords) -> Said {
        // Escaped by the one rule, uncapped (S3's `vfyq2`, fix pass 4, item
        // L-a): a declared type's words are the engine's, but some carry a
        // `String` (a rules file's path, a codec's message), and no character
        // of `HIDDEN` reaches a person through any of them.
        Said(Cow::Owned(
            shown(&error.to_string(), Some(MAX_ENGINE_CHARS)).0,
        ))
    }

    /// [`crate::said`]'s substitution: `template`'s `{}` in turn, each
    /// replaced by the next of `pieces` (`{{` and `}}` are a brace). Every
    /// input is a `&'static str` or a [`Said`], so the result is one too
    /// (S3's `vfyq2`, fix pass 3, item M2 (d): this replaced a constructor
    /// over any `String`). `said!` checks at compile time that the template
    /// holds exactly one `{}` per piece; a template written by hand that
    /// holds fewer leaves the rest of the pieces out, and one that holds more
    /// writes the extra `{}` as it stands.
    // Private (fix pass 5, item L4): only `compose_checked`, which `said!`
    // calls, reaches it.
    #[must_use]
    fn compose(template: &'static str, pieces: &[Said]) -> Said {
        let extra: usize = pieces.iter().map(|piece| piece.0.len()).sum();
        let mut out = String::with_capacity(template.len().saturating_add(extra));
        let mut next = pieces.iter();
        let mut rest = template;
        while let Some(at) = rest.find(['{', '}']) {
            let (before, from) = rest.split_at(at);
            out.push_str(before);
            if let Some(after) = from.strip_prefix("{{") {
                out.push('{');
                rest = after;
            } else if let Some(after) = from.strip_prefix("}}") {
                out.push('}');
                rest = after;
            } else if let Some(after) = from.strip_prefix("{}") {
                match next.next() {
                    Some(piece) => out.push_str(&piece.0),
                    None => out.push_str("{}"),
                }
                rest = after;
            } else {
                let (brace, after) = from.split_at(1);
                out.push_str(brace);
                rest = after;
            }
        }
        out.push_str(rest);
        Said(Cow::Owned(out))
    }

    /// [`crate::said`]'s door: [`Said::compose`], after the compiler has
    /// checked the template against the pieces (`checked`, which writes
    /// nothing and is dropped).
    #[doc(hidden)]
    #[must_use]
    pub fn compose_checked(
        checked: fmt::Arguments<'_>,
        template: &'static str,
        pieces: &[Said],
    ) -> Said {
        let _ = checked;
        Said::compose(template, pieces)
    }

    /// Text this crate composed from words and quoted pieces itself: a
    /// codec message ([`super::Error::said`]), never a caller's `String`.
    pub(crate) const fn made_here(text: String) -> Said {
        Said(Cow::Owned(text))
    }
}

/// An error type whose `Display` is the engine's own words: static text,
/// numbers, and text this workspace made, never text read from a player's
/// file (S3's `vfyq2`, fix pass 3, item M2 (d)). Implementing it is a
/// declaration that the type's words carry no player text, reviewed where
/// it is written; [`Said::engine_error`] takes only such a type. A type
/// whose words do carry player text (the sim's `PlanError`, which writes
/// labels and ids raw) does not implement it, and is said through a
/// sentence composed from its fields.
///
/// It is sealed by [`door::EngineSeal`] (S3's `vfyq2`, fix pass 5, item L4). The
/// seal has to be nameable by the other crates behind the door (plan-core,
/// the verifier, the gateway), so the type system cannot keep a downstream
/// crate from naming it too: `crates/plan-core/tests/door.rs` holds every
/// implementation of either trait to a crate behind the door's `src`.
pub trait EngineWords: fmt::Display + door::EngineSeal {}

/// [`EngineWords`]'s seal (S3's `vfyq2`, fix pass 5, item L4).
#[doc(hidden)]
pub mod door {
    /// Implemented beside each [`super::EngineWords`] declaration, behind the
    /// door alone.
    pub trait EngineSeal {}
}

/// An I/O error's words are the operating system's: a file the host could
/// not read, never the file's contents.
impl EngineWords for std::io::Error {}
impl door::EngineSeal for std::io::Error {}

/// prost's decode error says which wire fault it met, in prost's words; the
/// bytes it read are this workspace's own encoding.
impl EngineWords for prost::DecodeError {}
impl door::EngineSeal for prost::DecodeError {}

/// [`crate::said`]'s compile-time check of a template's `{}` count, one per
/// piece; it writes nothing and is never shown.
#[doc(hidden)]
#[derive(Debug)]
pub struct Placeholder;

impl fmt::Display for Placeholder {
    fn fmt(&self, _formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(())
    }
}

impl fmt::Display for Said {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl From<&'static str> for Said {
    fn from(text: &'static str) -> Said {
        Said(Cow::Borrowed(text))
    }
}

impl From<Quoted> for Said {
    fn from(text: Quoted) -> Said {
        Said(Cow::Owned(text.0))
    }
}

impl From<&Quoted> for Said {
    fn from(text: &Quoted) -> Said {
        Said(Cow::Owned(text.0.clone()))
    }
}

impl From<&Said> for Said {
    fn from(text: &Said) -> Said {
        text.clone()
    }
}

impl From<char> for Said {
    fn from(character: char) -> Said {
        Said(Cow::Owned(quoted(character.encode_utf8(&mut [0; 4])).0))
    }
}

macro_rules! said_from_numbers {
    ($($number:ty),*) => {
        $(
            impl From<$number> for Said {
                fn from(number: $number) -> Said {
                    Said(Cow::Owned(number.to_string()))
                }
            }

            impl From<&$number> for Said {
                fn from(number: &$number) -> Said {
                    Said(Cow::Owned(number.to_string()))
                }
            }
        )*
    };
}

said_from_numbers!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize
);

/// Text a player is shown, composed of [`json::grammar::Said`] pieces: static
/// words, numbers and [`json::grammar::quoted`] player text. The format
/// string goes through `concat!`, so it cannot capture a variable by name,
/// and every argument converts into a `Said`, so a raw `String` does not
/// compile.
///
/// [`json::grammar::Said`]: crate::json::grammar::Said
/// [`json::grammar::quoted`]: crate::json::grammar::quoted
#[macro_export]
macro_rules! said {
    ($format:literal $(, $piece:expr)* $(,)?) => {
        $crate::json::grammar::Said::compose_checked(
            // The template holds exactly one `{}` per piece, and no other
            // placeholder: checked by the compiler, with nothing evaluated.
            ::std::format_args!(
                ::std::concat!($format)
                $(, { let _ = ::std::stringify!($piece); $crate::json::grammar::Placeholder })*
            ),
            ::std::concat!($format),
            &[$($crate::json::grammar::Said::from($piece)),*],
        )
    };
}

#[cfg(test)]
mod engine_cap {
    use super::{EngineWords, MAX_QUOTED_CHARS, Said};

    /// A declared type whose words run long: the review's probe, from inside
    /// the door, where a declaration is allowed.
    struct Long(usize);

    impl core::fmt::Display for Long {
        fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            formatter.write_str(&"x".repeat(self.0))
        }
    }

    impl EngineWords for Long {}
    impl super::door::EngineSeal for Long {}

    /// `Said::engine_error` is capped like `quoted` (S3's `vfyq2`, fix pass
    /// 5, item L4): a declared type's words are shown at most to the cap,
    /// the rest as `…`.
    #[test]
    fn an_engine_error_is_capped_like_a_quote() {
        let said = Said::engine_error(&Long(100_000));
        let shown = said.as_str().chars().count();
        assert!(
            shown < 100_000 && shown > MAX_QUOTED_CHARS,
            "shown {shown} characters"
        );
        assert!(said.as_str().ends_with('…'));
    }
}
