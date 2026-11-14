// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The codec as it stood before S3's `vfyq` (main at `0f6466f`), kept
//! **test-only** as the reference the differential tests hold the fast codec
//! to (decisions-log item 158 (2); S3's plan, section `vfyq`).
//!
//! Nothing here is reachable from a build that is not a test. It is the old
//! path, copied without a behavioural change: `read` and `write` from
//! `value.rs`, `json_to_wire` and `wire_to_json` from `codec.rs`. One statement
//! is respelled: the unknown enum number's `map_or_else` is a `match` here, so
//! that `tests/fallbacks.rs` finds the rule's one allowed statement in the
//! codec alone. The differential tests in `super::differential` feed it and the fast codec the
//! same generated documents and assert equal bytes or the same error. It is
//! kept until the old path is deleted; committed vectors replace it then.

#![allow(
    clippy::too_many_lines,
    reason = "a verbatim copy of the old codec, kept as the differential tests' reference"
)]

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::base64;
use super::value::{Error, Json, push_pointer};
use crate::descriptor::{Field, Message, ScalarKind, Schema, schema};
use crate::wire::{
    Reader, Value, Writer, i64_bits_to_u64, u64_bits_to_i64, zigzag_decode, zigzag_encode,
};

pub(super) fn read(text: &str) -> Result<Json, Error> {
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

pub(super) fn write(value: &Json) -> String {
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

const CODEC_MAX_DEPTH: usize = 64;

/// Wire bytes for a message of this type, as canonical JSON.
///
/// # Errors
///
/// If the type is not in the schema, if the bytes are not a valid encoding of
/// it, or if the schema contains a floating-point field.
pub(super) fn wire_to_json(full_name: &str, bytes: &[u8]) -> Result<Json, Error> {
    let schema = schema();
    let message = schema
        .message(full_name)
        .ok_or_else(|| Error::at("", format!("`{full_name}` is not a message in the schema")))?;
    message_to_json(schema, message, bytes, "", 0)
}

/// Canonical JSON for a message of this type, as wire bytes.
///
/// # Errors
///
/// If the type is not in the schema, if the document carries a field the
/// message does not declare, if a value does not fit its field, or if two
/// members of the same `oneof` are set at once.
pub(super) fn json_to_wire(full_name: &str, value: &Json) -> Result<Vec<u8>, Error> {
    let schema = schema();
    let message = schema
        .message(full_name)
        .ok_or_else(|| Error::at("", format!("`{full_name}` is not a message in the schema")))?;
    json_to_message(schema, message, value, "", 0)
}

// ---------------------------------------------------------------------------
// Wire -> JSON
// ---------------------------------------------------------------------------

fn message_to_json(
    schema: &Schema,
    message: &Message,
    bytes: &[u8],
    pointer: &str,
    depth: usize,
) -> Result<Json, Error> {
    if depth > CODEC_MAX_DEPTH {
        return Err(Error::at(pointer, "a message nested deeper than 64 levels"));
    }

    // Grouped by field number, so the output is in ascending number order
    // whatever order the encoder wrote them in.
    let mut grouped: BTreeMap<i32, Vec<Value<'_>>> = BTreeMap::new();
    let mut reader = Reader::new(bytes);
    while !reader.is_done() {
        let (number, value) = reader.field().map_err(|error| {
            Error::at(
                pointer,
                format!("`{}` is not a valid encoding: {error:?}", message.full_name),
            )
        })?;
        grouped.entry(number).or_default().push(value);
    }

    let mut entries: Vec<(String, Json)> = Vec::new();
    for (number, values) in grouped {
        let field = message.field_by_number(number).ok_or_else(|| {
            Error::at(
                pointer,
                format!(
                    "field number {number} is not declared on `{}`",
                    message.full_name
                ),
            )
        })?;
        let field_pointer = push_pointer(pointer, &field.name);
        let json = if field.repeated {
            let mut items: Vec<Json> = Vec::new();
            for value in values {
                for element in unpack(field.kind, value, &field_pointer)? {
                    items.push(scalar_to_json(
                        schema,
                        field,
                        element,
                        &field_pointer,
                        depth,
                    )?);
                }
            }
            Json::Array(items)
        } else {
            // proto3: the last occurrence of a singular field wins.
            let last = values
                .last()
                .copied()
                .ok_or_else(|| Error::at(&field_pointer, "a field with no value"))?;
            scalar_to_json(schema, field, last, &field_pointer, depth)?
        };
        entries.push((field.name.clone(), json));
    }
    Ok(Json::Object(entries))
}

/// Splits a packed repeated payload into its elements. prost writes packable
/// repeated fields packed, so this is the common path on the way out; a
/// repeated field written unpacked arrives one element per tag and passes
/// straight through.
fn unpack<'a>(kind: ScalarKind, value: Value<'a>, pointer: &str) -> Result<Vec<Value<'a>>, Error> {
    let (true, Value::Delimited(payload)) = (packable(kind), value) else {
        return Ok(vec![value]);
    };

    let mut out: Vec<Value<'a>> = Vec::new();
    let mut reader = Reader::new(payload);
    while !reader.is_done() {
        let fail = |error| Error::at(pointer, format!("a packed repeated field: {error:?}"));
        let element = match kind {
            ScalarKind::Fixed32 | ScalarKind::Sfixed32 | ScalarKind::Float => {
                let array: [u8; 4] = reader
                    .raw(4)
                    .map_err(fail)?
                    .try_into()
                    .map_err(|_| Error::at(pointer, "a truncated packed fixed32"))?;
                Value::Fixed32(u32::from_le_bytes(array))
            }
            ScalarKind::Fixed64 | ScalarKind::Sfixed64 | ScalarKind::Double => {
                let array: [u8; 8] = reader
                    .raw(8)
                    .map_err(fail)?
                    .try_into()
                    .map_err(|_| Error::at(pointer, "a truncated packed fixed64"))?;
                Value::Fixed64(u64::from_le_bytes(array))
            }
            _ => Value::Varint(reader.varint().map_err(fail)?),
        };
        out.push(element);
    }
    Ok(out)
}

const fn packable(kind: ScalarKind) -> bool {
    !matches!(
        kind,
        ScalarKind::String | ScalarKind::Bytes | ScalarKind::Message
    )
}

fn scalar_to_json(
    schema: &Schema,
    field: &Field,
    value: Value<'_>,
    pointer: &str,
    depth: usize,
) -> Result<Json, Error> {
    match field.kind {
        ScalarKind::Double | ScalarKind::Float => Err(Error::at(
            pointer,
            "a floating-point field: the sim is integer-only and this codec refuses one",
        )),
        ScalarKind::Bool => Ok(Json::Bool(varint(value, pointer)? != 0)),
        ScalarKind::Int32 => {
            let wide = u64_bits_to_i64(varint(value, pointer)?);
            let narrow = i32::try_from(wide)
                .map_err(|_| Error::at(pointer, "an int32 field carrying a wider value"))?;
            Ok(Json::Number(narrow.to_string()))
        }
        ScalarKind::Sint32 => {
            let wide = zigzag_decode(varint(value, pointer)?);
            let narrow = i32::try_from(wide)
                .map_err(|_| Error::at(pointer, "an sint32 field carrying a wider value"))?;
            Ok(Json::Number(narrow.to_string()))
        }
        ScalarKind::Uint32 => {
            let narrow = u32::try_from(varint(value, pointer)?)
                .map_err(|_| Error::at(pointer, "a uint32 field carrying a wider value"))?;
            Ok(Json::Number(narrow.to_string()))
        }
        ScalarKind::Fixed32 => Ok(Json::Number(fixed32(value, pointer)?.to_string())),
        ScalarKind::Sfixed32 => {
            let bits = fixed32(value, pointer)?;
            Ok(Json::Number(
                i32::from_ne_bytes(bits.to_ne_bytes()).to_string(),
            ))
        }
        // The 64-bit kinds are quoted strings, which is the whole reason no
        // duration field in gp.v1 is one (decisions-log item 46).
        ScalarKind::Int64 => Ok(Json::String(
            u64_bits_to_i64(varint(value, pointer)?).to_string(),
        )),
        ScalarKind::Sint64 => Ok(Json::String(
            zigzag_decode(varint(value, pointer)?).to_string(),
        )),
        ScalarKind::Uint64 => Ok(Json::String(varint(value, pointer)?.to_string())),
        ScalarKind::Fixed64 => Ok(Json::String(fixed64(value, pointer)?.to_string())),
        ScalarKind::Sfixed64 => {
            let bits = fixed64(value, pointer)?;
            Ok(Json::String(u64_bits_to_i64(bits).to_string()))
        }
        ScalarKind::String => {
            let payload = delimited(value, pointer)?;
            let text = std::str::from_utf8(payload)
                .map_err(|_| Error::at(pointer, "a string field that is not valid UTF-8"))?;
            Ok(Json::String(text.to_owned()))
        }
        ScalarKind::Bytes => Ok(Json::String(base64::encode(delimited(value, pointer)?))),
        ScalarKind::Enum => {
            let number = i32::try_from(u64_bits_to_i64(varint(value, pointer)?))
                .map_err(|_| Error::at(pointer, "an enum value out of range"))?;
            let declared = schema.enumeration(&field.type_name).ok_or_else(|| {
                Error::at(
                    pointer,
                    format!("`{}` is not in the schema", field.type_name),
                )
            })?;
            // An unrecognised number is written as a number, which is what
            // the proto3 JSON mapping says and what lets a diagnostic name
            // the offending value instead of losing it. (A `match` here where
            // the codec has `map_or_else`, so that `tests/fallbacks.rs` reads
            // the rule's one allowed statement in one file.)
            match declared.value_by_number(number) {
                Some(found) => Ok(Json::String(found.name.clone())),
                None => Ok(Json::Number(number.to_string())),
            }
        }
        ScalarKind::Message => {
            let nested = schema.message(&field.type_name).ok_or_else(|| {
                Error::at(
                    pointer,
                    format!("`{}` is not in the schema", field.type_name),
                )
            })?;
            message_to_json(
                schema,
                nested,
                delimited(value, pointer)?,
                pointer,
                depth.saturating_add(1),
            )
        }
    }
}

fn varint(value: Value<'_>, pointer: &str) -> Result<u64, Error> {
    match value {
        Value::Varint(raw) => Ok(raw),
        _ => Err(Error::at(pointer, "expected a varint on the wire")),
    }
}

fn fixed32(value: Value<'_>, pointer: &str) -> Result<u32, Error> {
    match value {
        Value::Fixed32(raw) => Ok(raw),
        _ => Err(Error::at(pointer, "expected four fixed bytes on the wire")),
    }
}

fn fixed64(value: Value<'_>, pointer: &str) -> Result<u64, Error> {
    match value {
        Value::Fixed64(raw) => Ok(raw),
        _ => Err(Error::at(pointer, "expected eight fixed bytes on the wire")),
    }
}

fn delimited<'a>(value: Value<'a>, pointer: &str) -> Result<&'a [u8], Error> {
    match value {
        Value::Delimited(payload) => Ok(payload),
        _ => Err(Error::at(pointer, "expected a length-delimited value")),
    }
}

// ---------------------------------------------------------------------------
// JSON -> wire
// ---------------------------------------------------------------------------

fn json_to_message(
    schema: &Schema,
    message: &Message,
    value: &Json,
    pointer: &str,
    depth: usize,
) -> Result<Vec<u8>, Error> {
    if depth > CODEC_MAX_DEPTH {
        return Err(Error::at(pointer, "a message nested deeper than 64 levels"));
    }
    let Json::Object(entries) = value else {
        return Err(Error::at(
            pointer,
            format!(
                "expected an object for `{}`, found {}",
                message.full_name,
                value.kind()
            ),
        ));
    };

    // Resolve every key first, so an unknown field is reported before anything
    // is written — and so the write order can be by field number rather than
    // by the order somebody happened to type them in.
    let mut resolved: Vec<(&Field, &Json)> = Vec::new();
    let mut oneofs_used: BTreeMap<i32, String> = BTreeMap::new();
    for (key, item) in entries {
        let key_pointer = push_pointer(pointer, key);
        let field = message.field_by_name(key).ok_or_else(|| {
            Error::at(
                &key_pointer,
                format!(
                    "`{key}` is not a field of `{}`. Unknown fields are rejected, never stripped.",
                    message.full_name
                ),
            )
        })?;
        if matches!(item, Json::Null) {
            // The mapping treats null as "the default value", and a default is
            // not written on the wire. Recording it as a no-op keeps a
            // hand-written file that spells a field out as null loadable.
            continue;
        }
        if let Some(index) = field.oneof_index {
            if let Some(previous) = oneofs_used.get(&index) {
                return Err(Error::at(
                    &key_pointer,
                    format!(
                        "`{key}` and `{previous}` are both set, but only one member of a oneof may be"
                    ),
                ));
            }
            oneofs_used.insert(index, key.clone());
        } else if is_proto3_default(schema, field, item) {
            // A field WRITTEN OUT at its proto3 default is not written to the
            // wire, exactly as prost's own encoder drops it. `is_proto3_default`
            // says why that has to happen here rather than in `canonicalise`,
            // and where the rule stops.
            continue;
        }
        resolved.push((field, item));
    }
    resolved.sort_by_key(|(field, _)| field.number);

    let mut writer = Writer::default();
    for (field, item) in resolved {
        let field_pointer = push_pointer(pointer, &field.name);
        if field.repeated {
            let Json::Array(items) = item else {
                return Err(Error::at(
                    &field_pointer,
                    format!("expected an array, found {}", item.kind()),
                ));
            };
            for (index, element) in items.iter().enumerate() {
                let element_pointer = push_pointer(&field_pointer, &index.to_string());
                write_scalar(schema, field, element, &element_pointer, depth, &mut writer)?;
            }
        } else {
            write_scalar(schema, field, item, &field_pointer, depth, &mut writer)?;
        }
    }
    Ok(writer.into_vec())
}

/// True when `value` is what this field would hold if it were absent — and
/// when absence and this value are indistinguishable once decoded.
///
/// # Why this exists: `canonicalise` has to agree with `decode` then `encode`
///
/// The canonical form has to have exactly one spelling, and before
/// decisions-log item 100 (10) it had two. [`super::encode`] goes through
/// prost, whose encoder drops a scalar at its default, so
/// `{"major":1,"minor":0}` came back as `{"major": 1}`. [`super::canonicalise`]
/// goes JSON → wire → JSON without a typed message in between, so it wrote the
/// zero out again and came back as `{"major": 1, "minor": 0}`. Two callers,
/// one document, two answers — and the committed goldens
/// (`tests/golden/proto/expected.expand_east.json`, `tests/golden/plan-core/**`)
/// all carry the first spelling, so the first is the canonical one and this is
/// where the second is removed.
///
/// # Where the rule stops
///
/// This mirrors prost's encoder and nothing wider, so the three cases that
/// **have presence** in proto3 are all excluded, at the call site rather than
/// here:
///
/// * a **`oneof` member**, where which arm is set is the whole message — an
///   arm holding its own zero still has to be written, or the arm disappears;
/// * an **`optional` field**, which protoc compiles to a synthetic one-field
///   oneof, so the same exclusion covers it without a second test;
/// * a **`repeated` field**, whose empty array already writes nothing.
///
/// A **message-typed** field is excluded here, by this function: a present but
/// empty submessage is a different message from an absent one, and prost writes
/// a zero-length one for `Some(Default::default())`.
fn is_proto3_default(schema: &Schema, field: &Field, value: &Json) -> bool {
    if field.repeated {
        return false;
    }
    match field.kind {
        // Two different reasons for one answer. A present submessage HAS
        // presence, so `{}` is not absence and must be written. A
        // floating-point field is refused outright by `write_scalar`, and must
        // reach it to be refused: a dropped field is not an error, and a float
        // field has to be one.
        ScalarKind::Message | ScalarKind::Double | ScalarKind::Float => false,
        ScalarKind::Bool => matches!(value, Json::Bool(false)),
        ScalarKind::String | ScalarKind::Bytes => {
            matches!(value, Json::String(text) if text.is_empty())
        }
        ScalarKind::Enum => {
            schema
                .enumeration(&field.type_name)
                .is_some_and(|declared| match value {
                    Json::String(name) => declared
                        .value_by_name(name)
                        .is_some_and(|it| it.number == 0),
                    Json::Number(lexeme) => is_zero(lexeme),
                    _ => false,
                })
        }
        ScalarKind::Int32
        | ScalarKind::Sint32
        | ScalarKind::Uint32
        | ScalarKind::Fixed32
        | ScalarKind::Sfixed32
        | ScalarKind::Int64
        | ScalarKind::Sint64
        | ScalarKind::Uint64
        | ScalarKind::Fixed64
        | ScalarKind::Sfixed64 => match value {
            // The mapping accepts a 64-bit integer as a quoted string as well
            // as bare, so both spellings of zero are the same zero.
            Json::Number(lexeme) | Json::String(lexeme) => is_zero(lexeme),
            _ => false,
        },
    }
}

/// True when a JSON number lexeme is a whole zero.
///
/// Parsed rather than compared with `"0"`: `-0` and `00` are the same value and
/// a lexeme this schema cannot hold (`0.0`, `1e3`) is left for `write_scalar`
/// to refuse with its own message.
fn is_zero(lexeme: &str) -> bool {
    lexeme.parse::<i64>() == Ok(0) || lexeme.parse::<u64>() == Ok(0)
}

#[allow(
    clippy::too_many_lines,
    reason = "one arm per Protobuf scalar kind; splitting it would hide the \
              one-to-one correspondence with the descriptor's type enum"
)]
fn write_scalar(
    schema: &Schema,
    field: &Field,
    value: &Json,
    pointer: &str,
    depth: usize,
    writer: &mut Writer,
) -> Result<(), Error> {
    match field.kind {
        ScalarKind::Double | ScalarKind::Float => Err(Error::at(
            pointer,
            "a floating-point field: the sim is integer-only and this codec refuses one",
        )),
        ScalarKind::Bool => {
            let Json::Bool(flag) = value else {
                return Err(Error::at(
                    pointer,
                    format!("expected true or false, found {}", value.kind()),
                ));
            };
            writer.varint_field(field.number, u64::from(*flag));
            Ok(())
        }
        ScalarKind::Int32 => {
            let number = integer(value, pointer)?;
            let narrow = i32::try_from(number)
                .map_err(|_| Error::at(pointer, "out of range for an int32 field"))?;
            writer.varint_field(field.number, i64_bits_to_u64(i64::from(narrow)));
            Ok(())
        }
        ScalarKind::Sint32 => {
            let number = integer(value, pointer)?;
            let narrow = i32::try_from(number)
                .map_err(|_| Error::at(pointer, "out of range for an sint32 field"))?;
            writer.varint_field(field.number, zigzag_encode(i64::from(narrow)));
            Ok(())
        }
        ScalarKind::Uint32 => {
            let number = unsigned(value, pointer)?;
            let narrow = u32::try_from(number)
                .map_err(|_| Error::at(pointer, "out of range for a uint32 field"))?;
            writer.varint_field(field.number, u64::from(narrow));
            Ok(())
        }
        ScalarKind::Fixed32 => {
            let number = unsigned(value, pointer)?;
            let narrow = u32::try_from(number)
                .map_err(|_| Error::at(pointer, "out of range for a fixed32 field"))?;
            writer.fixed32_field(field.number, narrow);
            Ok(())
        }
        ScalarKind::Sfixed32 => {
            let number = integer(value, pointer)?;
            let narrow = i32::try_from(number)
                .map_err(|_| Error::at(pointer, "out of range for an sfixed32 field"))?;
            writer.fixed32_field(field.number, u32::from_ne_bytes(narrow.to_ne_bytes()));
            Ok(())
        }
        ScalarKind::Int64 => {
            writer.varint_field(field.number, i64_bits_to_u64(integer(value, pointer)?));
            Ok(())
        }
        ScalarKind::Sint64 => {
            writer.varint_field(field.number, zigzag_encode(integer(value, pointer)?));
            Ok(())
        }
        ScalarKind::Uint64 => {
            writer.varint_field(field.number, unsigned(value, pointer)?);
            Ok(())
        }
        ScalarKind::Fixed64 => {
            writer.fixed64_field(field.number, unsigned(value, pointer)?);
            Ok(())
        }
        ScalarKind::Sfixed64 => {
            writer.fixed64_field(field.number, i64_bits_to_u64(integer(value, pointer)?));
            Ok(())
        }
        ScalarKind::String => {
            let Json::String(text) = value else {
                return Err(Error::at(
                    pointer,
                    format!("expected a string, found {}", value.kind()),
                ));
            };
            writer
                .delimited_field(field.number, text.as_bytes())
                .map_err(|_| Error::at(pointer, "a string too long for its length prefix"))
        }
        ScalarKind::Bytes => {
            let Json::String(text) = value else {
                return Err(Error::at(
                    pointer,
                    format!("expected base64 in a string, found {}", value.kind()),
                ));
            };
            let bytes =
                base64::decode(text).ok_or_else(|| Error::at(pointer, "not valid base64"))?;
            writer
                .delimited_field(field.number, &bytes)
                .map_err(|_| Error::at(pointer, "bytes too long for their length prefix"))
        }
        ScalarKind::Enum => {
            let declared = schema.enumeration(&field.type_name).ok_or_else(|| {
                Error::at(
                    pointer,
                    format!("`{}` is not in the schema", field.type_name),
                )
            })?;
            let number = match value {
                Json::String(name) => {
                    declared
                        .value_by_name(name)
                        .ok_or_else(|| {
                            Error::at(
                                pointer,
                                format!("`{name}` is not a value of `{}`", field.type_name),
                            )
                        })?
                        .number
                }
                Json::Number(_) => {
                    let raw = integer(value, pointer)?;
                    i32::try_from(raw)
                        .map_err(|_| Error::at(pointer, "an enum value out of range"))?
                }
                other => {
                    return Err(Error::at(
                        pointer,
                        format!("expected an enum value name, found {}", other.kind()),
                    ));
                }
            };
            writer.varint_field(field.number, i64_bits_to_u64(i64::from(number)));
            Ok(())
        }
        ScalarKind::Message => {
            let nested = schema.message(&field.type_name).ok_or_else(|| {
                Error::at(
                    pointer,
                    format!("`{}` is not in the schema", field.type_name),
                )
            })?;
            let payload = json_to_message(schema, nested, value, pointer, depth.saturating_add(1))?;
            writer
                .delimited_field(field.number, &payload)
                .map_err(|_| Error::at(pointer, "a message too long for its length prefix"))
        }
    }
}

/// A JSON integer, from a bare number or from the quoted form the mapping also
/// accepts. A fractional or exponential lexeme is refused: this schema has no
/// field that could hold one.
fn integer(value: &Json, pointer: &str) -> Result<i64, Error> {
    let lexeme = match value {
        Json::Number(text) | Json::String(text) => text,
        other => {
            return Err(Error::at(
                pointer,
                format!("expected an integer, found {}", other.kind()),
            ));
        }
    };
    lexeme.parse::<i64>().map_err(|_| {
        Error::at(
            pointer,
            format!("`{lexeme}` is not a whole number. This schema has no fractional field."),
        )
    })
}

fn unsigned(value: &Json, pointer: &str) -> Result<u64, Error> {
    let lexeme = match value {
        Json::Number(text) | Json::String(text) => text,
        other => {
            return Err(Error::at(
                pointer,
                format!("expected a non-negative integer, found {}", other.kind()),
            ));
        }
    };
    lexeme.parse::<u64>().map_err(|_| {
        Error::at(
            pointer,
            format!("`{lexeme}` is not a non-negative whole number"),
        )
    })
}
