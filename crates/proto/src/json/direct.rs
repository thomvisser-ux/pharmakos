// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! JSON text straight to wire bytes, in one pass, with no `Json` tree in
//! between (S3's `vfyq`, decisions-log item 158).
//!
//! [`super::read`] then [`super::json_to_encoded`] builds an owned tree — a
//! `String` for every key, number and string, a `Vec` for every object and
//! array — and walks it once to write the wire. For a playbook at the size
//! budget that tree is most of the decode's cost. This reader walks the text
//! once, against the schema, and writes the wire as it goes.
//!
//! # It answers only when it can answer exactly
//!
//! [`text_to_encoded`] returns exactly what `read` then `json_to_encoded`
//! return for a document both accept — the same bytes, in the same order, and
//! the same [`Encoded::is_plain`] — and `None` for every document either of
//! them refuses. It never says *why*: a refusal is reported by the exact path,
//! with its pointer and its message, which is the path the caller takes on
//! `None`. So the codec has one set of errors, and this reader cannot drift
//! from it; the differential tests feed both the same generated documents.
//!
//! What it does to match the exact path:
//!
//! * the grammar is `read`'s, byte for byte: the same whitespace, literals,
//!   number lexemes, escapes and surrogates, the same nesting limit, and a
//!   duplicate key refused;
//! * a member is resolved, skipped as `null` or as a written-out default, or
//!   refused as an unknown field or a second arm of a `oneof`, as
//!   `json_to_message` does;
//! * each object's members are written in document order and then, if their
//!   field numbers are not already ascending, moved into a stable ascending
//!   order — the order `json_to_message` writes in;
//! * a submessage's length is reserved as one byte and patched when its
//!   payload is done, widened in place past 127 bytes, so the bytes are what
//!   writing the payload first would have made.

use super::base64;
use super::codec::{Encoded, Lexeme, is_proto3_default_lexeme};
use super::index::{Index, Indexed, IndexedField, index};
use crate::descriptor::ScalarKind;
use crate::wire::{i64_bits_to_u64, zigzag_encode};

/// `read`'s nesting limit, and the codec's.
const MAX_DEPTH: usize = 64;

/// The wire types this reader writes, as a tag's low three bits.
const VARINT: u64 = 0;
const FIXED64: u64 = 1;
const DELIMITED: u64 = 2;
const FIXED32: u64 = 5;

/// The wire bytes for `text` as a message of type `full_name`, or `None` when
/// [`super::read`] or [`super::json_to_encoded`] would refuse it.
///
/// See the module's documentation for why `None` carries no reason.
pub(crate) fn text_to_encoded(full_name: &str, text: &str) -> Option<Encoded> {
    let index = index();
    let at = index.find(full_name)?;
    let mut reader = Direct {
        index,
        text,
        bytes: text.as_bytes(),
        pos: 0,
        out: Vec::with_capacity(text.len()),
        members: Vec::new(),
        keys: Vec::new(),
        key_text: String::new(),
        oneofs: Vec::new(),
        unescaped: String::new(),
        moved: Vec::new(),
        plain: true,
    };
    reader.skip_whitespace();
    reader.message(at, 0, 0)?;
    reader.skip_whitespace();
    if reader.pos < reader.bytes.len() {
        return None;
    }
    Some(Encoded::new(reader.out, reader.plain))
}

/// Whether the schema holds a message of this type, which is the one reason
/// [`text_to_encoded`] declines that is not about the text.
pub(crate) fn knows(full_name: &str) -> bool {
    index().find(full_name).is_some()
}

/// One member an object wrote: its field, and where its bytes sit in `out`.
#[derive(Clone, Copy)]
struct Member {
    number: i32,
    repeated: bool,
    start: usize,
    end: usize,
}

/// A string token's text: a slice of the document when it held no escape,
/// or the reader's `unescaped` buffer when it did.
#[derive(Clone, Copy)]
enum Text {
    Slice(usize, usize),
    Unescaped,
}

/// A scalar token, as a field reads it.
#[derive(Clone, Copy)]
enum Scalar<'t> {
    Bool(bool),
    /// A number's lexeme.
    Number(&'t str),
    String(Text),
}

struct Direct<'t> {
    index: &'static Index,
    text: &'t str,
    bytes: &'t [u8],
    pos: usize,
    out: Vec<u8>,
    /// The members of every object on the way down, as a stack.
    members: Vec<Member>,
    /// The keys of every object on the way down, unescaped, as a stack of
    /// spans into `key_text`, for the duplicate-key check.
    keys: Vec<(usize, usize)>,
    /// The text of those keys.
    key_text: String,
    /// The `oneof`s every object on the way down has set, as a stack.
    oneofs: Vec<i32>,
    /// A string with an escape in it, unescaped.
    unescaped: String,
    /// Scratch for moving an object's members into field-number order.
    moved: Vec<u8>,
    plain: bool,
}

impl<'t> Direct<'t> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn bump(&mut self) {
        self.pos = self.pos.saturating_add(1);
    }

    fn skip_whitespace(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.peek() {
            self.bump();
        }
    }

    fn expect(&mut self, byte: u8) -> Option<()> {
        (self.peek() == Some(byte)).then(|| self.bump())
    }

    fn literal(&mut self, word: &[u8]) -> Option<()> {
        let end = self.pos.checked_add(word.len())?;
        (self.bytes.get(self.pos..end)? == word).then(|| self.pos = end)
    }

    /// One message: an object whose members are this message's fields.
    fn message(&mut self, at: usize, depth: usize, message_depth: usize) -> Option<()> {
        if depth > MAX_DEPTH || message_depth > MAX_DEPTH {
            return None;
        }
        let message: &'static Indexed = self.index.get(at)?;
        self.expect(b'{')?;
        let member_base = self.members.len();
        let key_base = self.keys.len();
        let key_text_base = self.key_text.len();
        let oneof_base = self.oneofs.len();
        let region = self.out.len();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.bump();
            return Some(());
        }
        loop {
            self.skip_whitespace();
            let field = self.key(message, key_base)?;
            self.skip_whitespace();
            self.expect(b':')?;
            self.skip_whitespace();
            self.member(field, oneof_base, depth.saturating_add(1), message_depth)?;
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.bump(),
                Some(b'}') => {
                    self.bump();
                    break;
                }
                _ => return None,
            }
        }
        self.order(member_base, region)?;
        self.members.truncate(member_base);
        self.keys.truncate(key_base);
        self.key_text.truncate(key_text_base);
        self.oneofs.truncate(oneof_base);
        Some(())
    }

    /// A member's key: read, refused if this object has already had it, and
    /// resolved to the field it names.
    fn key(&mut self, message: &'static Indexed, key_base: usize) -> Option<&'static IndexedField> {
        let text = self.string()?;
        let start = self.key_text.len();
        match text {
            Text::Slice(from, to) => self.key_text.push_str(self.text.get(from..to)?),
            Text::Unescaped => self.key_text.push_str(&self.unescaped),
        }
        let end = self.key_text.len();
        let key = self.key_text.get(start..end)?;
        let seen = self.keys.get(key_base..)?;
        if seen
            .iter()
            .any(|&(from, to)| self.key_text.get(from..to) == Some(key))
        {
            return None;
        }
        let field = message.field_by_name(key)?;
        self.keys.push((start, end));
        Some(field)
    }

    /// One member's value, written for its field.
    fn member(
        &mut self,
        field: &'static IndexedField,
        oneof_base: usize,
        depth: usize,
        message_depth: usize,
    ) -> Option<()> {
        if depth > MAX_DEPTH {
            return None;
        }
        if self.peek() == Some(b'n') {
            // `null` is the default, and is skipped before anything else is
            // asked of the member, as `json_to_message` skips it.
            return self.literal(b"null");
        }
        if let Some(oneof) = field.field.oneof_index {
            if self.oneofs.get(oneof_base..)?.contains(&oneof) {
                return None;
            }
            self.oneofs.push(oneof);
        }
        let start = self.out.len();
        if field.field.repeated {
            self.expect(b'[')?;
            self.skip_whitespace();
            if self.peek() == Some(b']') {
                self.bump();
            } else {
                loop {
                    self.skip_whitespace();
                    self.element(field, depth.saturating_add(1), message_depth, false)?;
                    self.skip_whitespace();
                    match self.peek() {
                        Some(b',') => self.bump(),
                        Some(b']') => {
                            self.bump();
                            break;
                        }
                        _ => return None,
                    }
                }
            }
        } else {
            let droppable = field.field.oneof_index.is_none();
            self.element(field, depth, message_depth, droppable)?;
        }
        let end = self.out.len();
        if end > start {
            self.members.push(Member {
                number: field.field.number,
                repeated: field.field.repeated,
                start,
                end,
            });
        }
        Some(())
    }

    /// One value of a field: a submessage, or a scalar written unless it is
    /// `droppable` and spells the field's proto3 default.
    fn element(
        &mut self,
        field: &'static IndexedField,
        depth: usize,
        message_depth: usize,
        droppable: bool,
    ) -> Option<()> {
        if depth > MAX_DEPTH {
            return None;
        }
        let number = field.field.number;
        if field.field.kind == ScalarKind::Message {
            let nested = field.nested?;
            self.tag(number, DELIMITED);
            self.out.push(0);
            let payload = self.out.len();
            self.message(nested, depth, message_depth.saturating_add(1))?;
            return self.patch_length(payload);
        }
        let scalar = self.scalar()?;
        if droppable && self.is_default(field, scalar) {
            return Some(());
        }
        let zero = self.write_scalar(field, scalar)?;
        if zero && !field.presence {
            self.plain = false;
        }
        Some(())
    }

    /// A scalar token: `true`, `false`, a string or a number. An object, an
    /// array or `null` where a scalar belongs is the exact path's to refuse.
    fn scalar(&mut self) -> Option<Scalar<'t>> {
        match self.peek()? {
            b'"' => self.string().map(Scalar::String),
            b't' => self.literal(b"true").map(|()| Scalar::Bool(true)),
            b'f' => self.literal(b"false").map(|()| Scalar::Bool(false)),
            b'{' | b'[' | b'n' => None,
            _ => self.number().map(Scalar::Number),
        }
    }

    /// `read`'s number grammar: an optional `-`, then digits and any of
    /// `.eE+-`, with at least one digit.
    fn number(&mut self) -> Option<&'t str> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.bump();
        }
        let mut digits = false;
        while let Some(byte) = self.peek() {
            if byte.is_ascii_digit() {
                digits = true;
            } else if !matches!(byte, b'.' | b'e' | b'E' | b'+' | b'-') {
                break;
            }
            self.bump();
        }
        if !digits {
            return None;
        }
        self.text.get(start..self.pos)
    }

    /// `read`'s string grammar, unescaping into `unescaped` only when the
    /// string holds an escape.
    fn string(&mut self) -> Option<Text> {
        self.expect(b'"')?;
        let start = self.pos;
        loop {
            match self.peek()? {
                b'"' => {
                    let end = self.pos;
                    self.bump();
                    return Some(Text::Slice(start, end));
                }
                b'\\' => break,
                0x00..=0x1f => return None,
                _ => self.bump(),
            }
        }
        // An escape: copy what came before it, then go on unescaping.
        self.unescaped.clear();
        let before = self.text.get(start..self.pos)?;
        self.unescaped.push_str(before);
        loop {
            let run = self.pos;
            while let Some(byte) = self.peek() {
                if byte == b'"' || byte == b'\\' || byte < 0x20 {
                    break;
                }
                self.bump();
            }
            let plain = self.text.get(run..self.pos)?;
            self.unescaped.push_str(plain);
            match self.peek()? {
                b'"' => {
                    self.bump();
                    return Some(Text::Unescaped);
                }
                b'\\' => {
                    self.bump();
                    self.escape()?;
                }
                _ => return None,
            }
        }
    }

    fn escape(&mut self) -> Option<()> {
        let byte = self.peek()?;
        self.bump();
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
                let first = self.hex4()?;
                let code = if (0xd800..0xdc00).contains(&first) {
                    self.expect(b'\\')?;
                    self.expect(b'u')?;
                    let second = self.hex4()?;
                    if !(0xdc00..0xe000).contains(&second) {
                        return None;
                    }
                    let high = first.checked_sub(0xd800)? << 10;
                    let low = second.checked_sub(0xdc00)?;
                    0x1_0000_u32.checked_add(high | low)?
                } else {
                    first
                };
                char::from_u32(code)?
            }
            _ => return None,
        };
        self.unescaped.push(character);
        Some(())
    }

    fn hex4(&mut self) -> Option<u32> {
        let mut value: u32 = 0;
        for _ in 0..4 {
            let digit = char::from(self.peek()?).to_digit(16)?;
            value = value.checked_mul(16)?.checked_add(digit)?;
            self.bump();
        }
        Some(value)
    }

    fn text_of(&self, text: Text) -> Option<&str> {
        match text {
            Text::Slice(start, end) => self.text.get(start..end),
            Text::Unescaped => Some(self.unescaped.as_str()),
        }
    }

    /// `is_proto3_default`, read off a token rather than a tree.
    fn is_default(&self, field: &IndexedField, scalar: Scalar<'_>) -> bool {
        match scalar {
            Scalar::Bool(flag) => is_proto3_default_lexeme(field, Lexeme::Bool(flag)),
            Scalar::Number(lexeme) => is_proto3_default_lexeme(field, Lexeme::Number(lexeme)),
            Scalar::String(text) => self
                .text_of(text)
                .is_some_and(|text| is_proto3_default_lexeme(field, Lexeme::String(text))),
        }
    }

    /// Writes one scalar for its field, as `write_scalar` does, and says
    /// whether the value written is the field's default. `None` where
    /// `write_scalar` refuses.
    #[allow(
        clippy::too_many_lines,
        reason = "one arm per Protobuf scalar kind, as the codec's `write_scalar`, which it \
                  must answer as"
    )]
    fn write_scalar(&mut self, field: &IndexedField, scalar: Scalar<'_>) -> Option<bool> {
        let number = field.field.number;
        match field.field.kind {
            ScalarKind::Double | ScalarKind::Float | ScalarKind::Message => None,
            ScalarKind::Bool => {
                let Scalar::Bool(flag) = scalar else {
                    return None;
                };
                self.varint_field(number, u64::from(flag));
                Some(!flag)
            }
            ScalarKind::Int32 => {
                let narrow = i32::try_from(self.integer(scalar)?).ok()?;
                self.varint_field(number, i64_bits_to_u64(i64::from(narrow)));
                Some(narrow == 0)
            }
            ScalarKind::Sint32 => {
                let narrow = i32::try_from(self.integer(scalar)?).ok()?;
                self.varint_field(number, zigzag_encode(i64::from(narrow)));
                Some(narrow == 0)
            }
            ScalarKind::Uint32 => {
                let narrow = u32::try_from(self.unsigned(scalar)?).ok()?;
                self.varint_field(number, u64::from(narrow));
                Some(narrow == 0)
            }
            ScalarKind::Fixed32 => {
                let narrow = u32::try_from(self.unsigned(scalar)?).ok()?;
                self.tag(number, FIXED32);
                self.out.extend_from_slice(&narrow.to_le_bytes());
                Some(narrow == 0)
            }
            ScalarKind::Sfixed32 => {
                let narrow = i32::try_from(self.integer(scalar)?).ok()?;
                self.tag(number, FIXED32);
                self.out.extend_from_slice(&narrow.to_le_bytes());
                Some(narrow == 0)
            }
            ScalarKind::Int64 => {
                let wide = self.integer(scalar)?;
                self.varint_field(number, i64_bits_to_u64(wide));
                Some(wide == 0)
            }
            ScalarKind::Sint64 => {
                let wide = self.integer(scalar)?;
                self.varint_field(number, zigzag_encode(wide));
                Some(wide == 0)
            }
            ScalarKind::Uint64 => {
                let wide = self.unsigned(scalar)?;
                self.varint_field(number, wide);
                Some(wide == 0)
            }
            ScalarKind::Fixed64 => {
                let wide = self.unsigned(scalar)?;
                self.tag(number, FIXED64);
                self.out.extend_from_slice(&wide.to_le_bytes());
                Some(wide == 0)
            }
            ScalarKind::Sfixed64 => {
                let wide = self.integer(scalar)?;
                self.tag(number, FIXED64);
                self.out.extend_from_slice(&wide.to_le_bytes());
                Some(wide == 0)
            }
            ScalarKind::String => {
                let Scalar::String(text) = scalar else {
                    return None;
                };
                let (start, end) = match text {
                    Text::Slice(start, end) => (start, end),
                    Text::Unescaped => {
                        let length = u64::try_from(self.unescaped.len()).ok()?;
                        self.tag(number, DELIMITED);
                        self.varint(length);
                        self.out.extend_from_slice(self.unescaped.as_bytes());
                        return Some(self.unescaped.is_empty());
                    }
                };
                let slice = self.bytes.get(start..end)?;
                let length = u64::try_from(slice.len()).ok()?;
                self.tag(number, DELIMITED);
                self.varint(length);
                self.out.extend_from_slice(slice);
                Some(slice.is_empty())
            }
            ScalarKind::Bytes => {
                let Scalar::String(text) = scalar else {
                    return None;
                };
                let decoded = base64::decode(self.text_of(text)?)?;
                let length = u64::try_from(decoded.len()).ok()?;
                self.tag(number, DELIMITED);
                self.varint(length);
                self.out.extend_from_slice(&decoded);
                Some(decoded.is_empty())
            }
            ScalarKind::Enum => {
                let declared = field.enumeration?;
                let found = match scalar {
                    Scalar::String(text) => declared.value_by_name(self.text_of(text)?)?.number,
                    Scalar::Number(_) => i32::try_from(self.integer(scalar)?).ok()?,
                    Scalar::Bool(_) => return None,
                };
                self.varint_field(number, i64_bits_to_u64(i64::from(found)));
                Some(found == 0)
            }
        }
    }

    /// `integer`: a bare or quoted lexeme, as an `i64`.
    fn integer(&self, scalar: Scalar<'_>) -> Option<i64> {
        match scalar {
            Scalar::Number(lexeme) => lexeme.parse::<i64>().ok(),
            Scalar::String(text) => self.text_of(text)?.parse::<i64>().ok(),
            Scalar::Bool(_) => None,
        }
    }

    /// `unsigned`: a bare or quoted lexeme, as a `u64`.
    fn unsigned(&self, scalar: Scalar<'_>) -> Option<u64> {
        match scalar {
            Scalar::Number(lexeme) => lexeme.parse::<u64>().ok(),
            Scalar::String(text) => self.text_of(text)?.parse::<u64>().ok(),
            Scalar::Bool(_) => None,
        }
    }

    /// Moves the members an object wrote since `region` into ascending field
    /// number, stably, if they are not already; and marks the bytes not plain
    /// if a singular field was written twice.
    fn order(&mut self, member_base: usize, region: usize) -> Option<()> {
        let members = self.members.get_mut(member_base..)?;
        if !members.is_sorted_by_key(|member| member.number) {
            members.sort_by_key(|member| member.number);
            self.moved.clear();
            self.moved.extend_from_slice(self.out.get(region..)?);
            self.out.truncate(region);
            for member in members.iter() {
                let start = member.start.checked_sub(region)?;
                let end = member.end.checked_sub(region)?;
                self.out.extend_from_slice(self.moved.get(start..end)?);
            }
        }
        let twice = members.windows(2).any(|pair| {
            matches!(pair, [first, second]
                if first.number == second.number && !first.repeated)
        });
        if twice {
            self.plain = false;
        }
        Some(())
    }

    fn varint(&mut self, mut value: u64) {
        loop {
            // The low seven bits, read off the lowest byte: no conversion, so
            // nothing can fail.
            let [lowest, ..] = value.to_le_bytes();
            let low = lowest & 0x7f;
            value >>= 7;
            if value == 0 {
                self.out.push(low);
                return;
            }
            self.out.push(low | 0x80);
        }
    }

    fn tag(&mut self, number: i32, wire: u64) {
        let number_bits = u64::from(number.unsigned_abs());
        self.varint((number_bits << 3) | wire);
    }

    fn varint_field(&mut self, number: i32, value: u64) {
        self.tag(number, VARINT);
        self.varint(value);
    }

    /// Patches the one-byte length slot in front of `payload`, widening it in
    /// place when the payload is 128 bytes or longer.
    fn patch_length(&mut self, payload: usize) -> Option<()> {
        let length = self.out.len().checked_sub(payload)?;
        let slot = payload.checked_sub(1)?;
        let mut prefix = [0_u8; 10];
        let mut used = 0_usize;
        let mut rest = u64::try_from(length).ok()?;
        loop {
            let [lowest, ..] = rest.to_le_bytes();
            rest >>= 7;
            let byte = if rest == 0 {
                lowest & 0x7f
            } else {
                lowest | 0x80
            };
            *prefix.get_mut(used)? = byte;
            used = used.checked_add(1)?;
            if rest == 0 {
                break;
            }
        }
        let prefix = prefix.get(..used)?;
        if let [only] = prefix {
            *self.out.get_mut(slot)? = *only;
        } else {
            self.out.splice(slot..payload, prefix.iter().copied());
        }
        Some(())
    }
}
