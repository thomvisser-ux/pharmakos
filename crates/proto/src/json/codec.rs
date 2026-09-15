// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Canonical proto3 JSON, both ways, driven by the descriptor set.
//!
//! The pivot is the binary wire format, not the generated Rust types:
//!
//! ```text
//!   typed message --prost--> wire bytes --descriptor--> canonical JSON
//!   canonical JSON --descriptor--> wire bytes --prost--> typed message
//! ```
//!
//! One codec then covers every message in `gp.v1` and `gp.api.v1`, and a
//! message added to the schema is handled the day it is generated, with no
//! second place to update. That is the property decisions-log item 74 bought
//! by keeping the codec in-house.
//!
//! Where this deviates from the proto3 JSON mapping, and why:
//!
//! * **Field names are the `.proto` spelling**, not lowerCamelCase. Playbooks
//!   are hand-edited text and spec section 10's worked example writes
//!   `"schema_version"` and `"timeout_ms"`. Both spellings are *accepted* on
//!   input, as the mapping requires; only the original is written.
//! * **Fields are written in field-number order.** The mapping says nothing
//!   about order; a golden file needs one, and ascending number is the order
//!   the schema itself is reviewed in.
//! * **Floating-point fields are refused outright.** There are none in either
//!   package, the sim is integer-only (AGENTS.md section 4.2), and a codec
//!   that cannot express a float cannot be the way one arrives.
//! * **An unknown field name is an error**, never ignored. Spec section 10:
//!   "Load never strips".

use std::collections::BTreeMap;

use super::base64;
use super::value::{Error, Json, push_pointer};
use crate::descriptor::{Field, Message, ScalarKind, Schema, schema};
use crate::wire::{
    Reader, Value, Writer, i64_bits_to_u64, u64_bits_to_i64, zigzag_decode, zigzag_encode,
};

/// Deeper than any message in either package, and shallow enough that a
/// hostile document cannot exhaust the stack.
const MAX_DEPTH: usize = 64;

/// Wire bytes for a message of this type, as canonical JSON.
///
/// # Errors
///
/// If the type is not in the schema, if the bytes are not a valid encoding of
/// it, or if the schema contains a floating-point field.
pub fn wire_to_json(full_name: &str, bytes: &[u8]) -> Result<Json, Error> {
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
pub fn json_to_wire(full_name: &str, value: &Json) -> Result<Vec<u8>, Error> {
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
    if depth > MAX_DEPTH {
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
            declared.value_by_number(number).map_or_else(
                // An unrecognised number is written as a number, which is what
                // the proto3 JSON mapping says and what lets a diagnostic name
                // the offending value instead of losing it.
                || Ok(Json::Number(number.to_string())),
                |found| Ok(Json::String(found.name.clone())),
            )
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
    if depth > MAX_DEPTH {
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
            writer.delimited_field(field.number, text.as_bytes());
            Ok(())
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
            writer.delimited_field(field.number, &bytes);
            Ok(())
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
            writer.delimited_field(field.number, &payload);
            Ok(())
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
