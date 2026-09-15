// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The schema, read back out of the checked-in descriptor set.
//!
//! `crates/proto/src/generated/descriptor.binpb` is written by
//! `buf build proto --exclude-source-info --as-file-descriptor-set` (see
//! `proto/buf.gen.yaml`) and embedded with `include_bytes!`. Everything in
//! this crate that has to agree with the `.proto` files — the canonical JSON
//! codec, the reserved-number table, the method/scope table — reads it rather
//! than repeating it, so none of the three can drift from the schema.
//!
//! It is parsed with [`crate::wire`] rather than with `prost-types` for one
//! reason: prost drops unknown fields, and the scope annotation is a custom
//! option, which is an unknown field to every generated `EnumValueOptions`.
//!
//! The parse is done once, behind a [`OnceLock`]. Lookups are `BTreeMap`, not
//! `HashMap`: this crate is on the deterministic side of the wall and an
//! unordered iteration is a desync waiting to happen (AGENTS.md section 4.4).

use std::collections::BTreeMap;
use std::sync::OnceLock;

use crate::wire::{Reader, Value, WireError};

/// A field's declared type, as `FieldDescriptorProto.type` spells it.
///
/// **No field in `gp.v1` or `gp.api.v1` is `Double` or `Float`** — the sim is
/// integer-only (AGENTS.md section 4.2), a float in a hand-edited playbook
/// would be a determinism hole with a text editor attached, and
/// `tests/schema.rs` asserts it. The two arms exist so the codec can refuse
/// them by name rather than by falling through.
///
/// The 64-bit arms matter for a different reason: canonical proto JSON writes
/// them as **quoted strings**, which is why no duration field in `gp.v1` is
/// one (decisions-log item 46), and `tests/schema.rs` asserts that too.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScalarKind {
    /// `double`. Not present in either package.
    Double,
    /// `float`. Not present in either package.
    Float,
    /// `int64`. Quoted in JSON.
    Int64,
    /// `uint64`. Quoted in JSON.
    Uint64,
    /// `int32`. A bare JSON number.
    Int32,
    /// `fixed64`. Quoted in JSON.
    Fixed64,
    /// `fixed32`. A bare JSON number.
    Fixed32,
    /// `bool`.
    Bool,
    /// `string`.
    String,
    /// An embedded message, named by `type_name`.
    Message,
    /// `bytes`. Base64 in JSON.
    Bytes,
    /// `uint32`. A bare JSON number.
    Uint32,
    /// An enum, named by `type_name`. Its value name in JSON.
    Enum,
    /// `sfixed32`. A bare JSON number.
    Sfixed32,
    /// `sfixed64`. Quoted in JSON.
    Sfixed64,
    /// `sint32`, `ZigZag` on the wire. A bare JSON number.
    Sint32,
    /// `sint64`, `ZigZag` on the wire. Quoted in JSON.
    Sint64,
}

impl ScalarKind {
    const fn from_number(value: u64) -> Option<Self> {
        match value {
            1 => Some(Self::Double),
            2 => Some(Self::Float),
            3 => Some(Self::Int64),
            4 => Some(Self::Uint64),
            5 => Some(Self::Int32),
            6 => Some(Self::Fixed64),
            7 => Some(Self::Fixed32),
            8 => Some(Self::Bool),
            9 => Some(Self::String),
            11 => Some(Self::Message),
            12 => Some(Self::Bytes),
            13 => Some(Self::Uint32),
            14 => Some(Self::Enum),
            15 => Some(Self::Sfixed32),
            16 => Some(Self::Sfixed64),
            17 => Some(Self::Sint32),
            18 => Some(Self::Sint64),
            // 10 is `group`, which proto3 does not have.
            _ => None,
        }
    }
}

/// One field of a message.
#[derive(Clone, Debug)]
pub struct Field {
    /// The name as written in the `.proto`. **This is the name the canonical
    /// JSON uses**, not the lowerCamelCase `json_name`: playbooks are
    /// hand-edited and spec section 10's worked example writes
    /// `"schema_version"`, not `"schemaVersion"`.
    pub name: String,
    /// The lowerCamelCase spelling. Accepted on input, never written on
    /// output, because the proto3 JSON mapping requires parsers to take both.
    pub json_name: String,
    /// The field number, which is what the canonical JSON orders by.
    pub number: i32,
    /// `true` for a `repeated` field, which is an array in JSON.
    pub repeated: bool,
    /// The declared type.
    pub kind: ScalarKind,
    /// Fully-qualified, without the leading dot, for `Message` and `Enum`.
    pub type_name: String,
    /// Which `oneof` this field belongs to, by index into the message's
    /// `oneofs`.
    pub oneof_index: Option<i32>,
}

/// One message.
#[derive(Clone, Debug, Default)]
pub struct Message {
    /// Fully qualified, without a leading dot: `gp.v1.Playbook`.
    pub full_name: String,
    /// Sorted by field number, which is the order the canonical JSON writes
    /// them in.
    pub fields: Vec<Field>,
    /// The `oneof` declarations, in order. A field's `oneof_index` indexes
    /// into this.
    pub oneofs: Vec<String>,
    /// Inclusive at both ends. The descriptor stores message reserved ranges
    /// with an exclusive end; they are normalised here so the reserved-number
    /// golden reads the same for messages and enums.
    pub reserved_ranges: Vec<(i32, i32)>,
    /// Names that may never be used again, sorted.
    pub reserved_names: Vec<String>,
}

impl Message {
    /// The field with this number.
    #[must_use]
    pub fn field_by_number(&self, number: i32) -> Option<&Field> {
        self.fields.iter().find(|field| field.number == number)
    }

    /// The field with this name, accepting either spelling.
    #[must_use]
    pub fn field_by_name(&self, name: &str) -> Option<&Field> {
        self.fields
            .iter()
            .find(|field| field.name == name || field.json_name == name)
    }
}

/// One enum value, with the raw bytes of its options so a custom option can be
/// read back out (prost would have dropped them).
#[derive(Clone, Debug)]
pub struct EnumValue {
    /// The value name as the `.proto` writes it, which is also how canonical
    /// JSON spells it.
    pub name: String,
    /// The value's number.
    pub number: i32,
    options: Vec<u8>,
}

impl EnumValue {
    /// The varint carried by extension field `number` on this value's options,
    /// if it is set. This is how the method/scope table is read back.
    #[must_use]
    pub fn option_varint(&self, number: i32) -> Option<u64> {
        let mut reader = Reader::new(&self.options);
        while !reader.is_done() {
            let Ok((found, value)) = reader.field() else {
                return None;
            };
            if found == number {
                if let Value::Varint(raw) = value {
                    return Some(raw);
                }
            }
        }
        None
    }
}

/// One enum.
#[derive(Clone, Debug, Default)]
pub struct Enum {
    /// Fully qualified, without a leading dot: `gp.v1.Meta.AuthorKind`.
    pub full_name: String,
    /// The values, in declaration order.
    pub values: Vec<EnumValue>,
    /// Inclusive at both ends, as the descriptor stores them for enums.
    pub reserved_ranges: Vec<(i32, i32)>,
    /// Names that may never be used again, sorted.
    pub reserved_names: Vec<String>,
}

impl Enum {
    /// The value with this name.
    #[must_use]
    pub fn value_by_name(&self, name: &str) -> Option<&EnumValue> {
        self.values.iter().find(|value| value.name == name)
    }

    /// The value with this number.
    #[must_use]
    pub fn value_by_number(&self, number: i32) -> Option<&EnumValue> {
        self.values.iter().find(|value| value.number == number)
    }
}

/// Every message and enum in `gp.v1` and `gp.api.v1`, by fully-qualified name.
#[derive(Clone, Debug, Default)]
pub struct Schema {
    messages: BTreeMap<String, Message>,
    enums: BTreeMap<String, Enum>,
    extensions: BTreeMap<String, i32>,
}

impl Schema {
    /// The message with this fully-qualified name.
    #[must_use]
    pub fn message(&self, full_name: &str) -> Option<&Message> {
        self.messages.get(full_name)
    }

    /// The enum with this fully-qualified name.
    #[must_use]
    pub fn enumeration(&self, full_name: &str) -> Option<&Enum> {
        self.enums.get(full_name)
    }

    /// The field number of a declared extension, by fully-qualified name —
    /// `gp.api.v1.required_scope`, for instance.
    #[must_use]
    pub fn extension_number(&self, full_name: &str) -> Option<i32> {
        self.extensions.get(full_name).copied()
    }

    /// Every message, in name order.
    pub fn messages(&self) -> impl Iterator<Item = &Message> {
        self.messages.values()
    }

    /// Every enum, in name order.
    pub fn enums(&self) -> impl Iterator<Item = &Enum> {
        self.enums.values()
    }
}

/// The parsed schema. Parsed once; the descriptor set is a compile-time
/// constant, so a failure here is a build error we would rather see as a
/// panic at first use than as a silently empty schema.
///
/// # Panics
///
/// If the embedded descriptor set does not parse. That can only happen if the
/// checked-in `descriptor.binpb` is corrupt, which the regeneration test in
/// `tests/` is there to catch.
#[must_use]
pub fn schema() -> &'static Schema {
    static SCHEMA: OnceLock<Schema> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        parse(crate::DESCRIPTOR_SET).unwrap_or_else(|error| {
            panic!("the checked-in descriptor set does not parse: {error:?}")
        })
    })
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

fn parse(bytes: &[u8]) -> Result<Schema, WireError> {
    let mut schema = Schema::default();
    let mut reader = Reader::new(bytes);
    while !reader.is_done() {
        let (number, value) = reader.field()?;
        // FileDescriptorSet.file = 1.
        if number == 1 {
            if let Value::Delimited(payload) = value {
                parse_file(payload, &mut schema)?;
            }
        }
    }
    Ok(schema)
}

fn parse_file(bytes: &[u8], schema: &mut Schema) -> Result<(), WireError> {
    let mut package = String::new();
    let mut messages: Vec<&[u8]> = Vec::new();
    let mut enums: Vec<&[u8]> = Vec::new();
    let mut extensions: Vec<&[u8]> = Vec::new();

    let mut reader = Reader::new(bytes);
    while !reader.is_done() {
        let (number, value) = reader.field()?;
        match (number, value) {
            // FileDescriptorProto.package = 2.
            (2, Value::Delimited(payload)) => package = utf8(payload),
            // .message_type = 4, .enum_type = 5, .extension = 7.
            (4, Value::Delimited(payload)) => messages.push(payload),
            (5, Value::Delimited(payload)) => enums.push(payload),
            (7, Value::Delimited(payload)) => extensions.push(payload),
            _ => {}
        }
    }

    for payload in messages {
        parse_message(payload, &package, schema)?;
    }
    for payload in enums {
        let parsed = parse_enum(payload, &package)?;
        schema.enums.insert(parsed.full_name.clone(), parsed);
    }
    for payload in extensions {
        let field = parse_field(payload)?;
        schema
            .extensions
            .insert(join(&package, &field.name), field.number);
    }
    Ok(())
}

fn parse_message(bytes: &[u8], scope: &str, schema: &mut Schema) -> Result<(), WireError> {
    let mut name = String::new();
    let mut field_payloads: Vec<&[u8]> = Vec::new();
    let mut nested: Vec<&[u8]> = Vec::new();
    let mut enums: Vec<&[u8]> = Vec::new();
    let mut message = Message::default();

    let mut reader = Reader::new(bytes);
    while !reader.is_done() {
        let (number, value) = reader.field()?;
        match (number, value) {
            // DescriptorProto: name = 1, field = 2, nested_type = 3,
            // enum_type = 4, oneof_decl = 8, reserved_range = 9,
            // reserved_name = 10.
            (1, Value::Delimited(payload)) => name = utf8(payload),
            (2, Value::Delimited(payload)) => field_payloads.push(payload),
            (3, Value::Delimited(payload)) => nested.push(payload),
            (4, Value::Delimited(payload)) => enums.push(payload),
            (8, Value::Delimited(payload)) => message.oneofs.push(oneof_name(payload)?),
            (9, Value::Delimited(payload)) => {
                // DescriptorProto.ReservedRange's end is EXCLUSIVE.
                let (start, end) = range(payload)?;
                message.reserved_ranges.push((start, end.saturating_sub(1)));
            }
            (10, Value::Delimited(payload)) => message.reserved_names.push(utf8(payload)),
            _ => {}
        }
    }

    let full_name = join(scope, &name);
    for payload in field_payloads {
        message.fields.push(parse_field(payload)?);
    }
    message.fields.sort_by_key(|field| field.number);
    message.reserved_ranges.sort_unstable();
    message.reserved_names.sort();
    message.full_name.clone_from(&full_name);
    schema.messages.insert(full_name.clone(), message);

    for payload in nested {
        parse_message(payload, &full_name, schema)?;
    }
    for payload in enums {
        let parsed = parse_enum(payload, &full_name)?;
        schema.enums.insert(parsed.full_name.clone(), parsed);
    }
    Ok(())
}

fn parse_field(bytes: &[u8]) -> Result<Field, WireError> {
    let mut name = String::new();
    let mut json_name = String::new();
    let mut number: i32 = 0;
    let mut label: u64 = 0;
    let mut kind = ScalarKind::String;
    let mut type_name = String::new();
    let mut oneof_index: Option<i32> = None;

    let mut reader = Reader::new(bytes);
    while !reader.is_done() {
        let (field_number, value) = reader.field()?;
        match (field_number, value) {
            // FieldDescriptorProto: name = 1, number = 3, label = 4,
            // type = 5, type_name = 6, oneof_index = 9, json_name = 10.
            (1, Value::Delimited(payload)) => name = utf8(payload),
            (3, Value::Varint(raw)) => number = signed32(raw),
            (4, Value::Varint(raw)) => label = raw,
            (5, Value::Varint(raw)) => {
                kind = ScalarKind::from_number(raw).ok_or(WireError::BadWireType)?;
            }
            (6, Value::Delimited(payload)) => {
                utf8(payload)
                    .trim_start_matches('.')
                    .clone_into(&mut type_name);
            }
            (9, Value::Varint(raw)) => oneof_index = Some(signed32(raw)),
            (10, Value::Delimited(payload)) => json_name = utf8(payload),
            _ => {}
        }
    }

    Ok(Field {
        name,
        json_name,
        number,
        // LABEL_REPEATED = 3.
        repeated: label == 3,
        kind,
        type_name,
        oneof_index,
    })
}

fn parse_enum(bytes: &[u8], scope: &str) -> Result<Enum, WireError> {
    let mut name = String::new();
    let mut parsed = Enum::default();
    let mut value_payloads: Vec<&[u8]> = Vec::new();

    let mut reader = Reader::new(bytes);
    while !reader.is_done() {
        let (number, value) = reader.field()?;
        match (number, value) {
            // EnumDescriptorProto: name = 1, value = 2, reserved_range = 4,
            // reserved_name = 5.
            (1, Value::Delimited(payload)) => name = utf8(payload),
            (2, Value::Delimited(payload)) => value_payloads.push(payload),
            (4, Value::Delimited(payload)) => {
                // EnumReservedRange's end is INCLUSIVE, unlike a message's.
                parsed.reserved_ranges.push(range(payload)?);
            }
            (5, Value::Delimited(payload)) => parsed.reserved_names.push(utf8(payload)),
            _ => {}
        }
    }

    for payload in value_payloads {
        parsed.values.push(parse_enum_value(payload)?);
    }
    parsed.reserved_ranges.sort_unstable();
    parsed.reserved_names.sort();
    parsed.full_name = join(scope, &name);
    Ok(parsed)
}

fn parse_enum_value(bytes: &[u8]) -> Result<EnumValue, WireError> {
    let mut name = String::new();
    let mut number: i32 = 0;
    let mut options: Vec<u8> = Vec::new();

    let mut reader = Reader::new(bytes);
    while !reader.is_done() {
        let (field_number, value) = reader.field()?;
        match (field_number, value) {
            // EnumValueDescriptorProto: name = 1, number = 2, options = 3.
            (1, Value::Delimited(payload)) => name = utf8(payload),
            (2, Value::Varint(raw)) => number = signed32(raw),
            (3, Value::Delimited(payload)) => options = payload.to_vec(),
            _ => {}
        }
    }
    Ok(EnumValue {
        name,
        number,
        options,
    })
}

fn oneof_name(bytes: &[u8]) -> Result<String, WireError> {
    let mut reader = Reader::new(bytes);
    while !reader.is_done() {
        let (number, value) = reader.field()?;
        if let (1, Value::Delimited(payload)) = (number, value) {
            return Ok(utf8(payload));
        }
    }
    Ok(String::new())
}

fn range(bytes: &[u8]) -> Result<(i32, i32), WireError> {
    let mut start: i32 = 0;
    let mut end: i32 = 0;
    let mut reader = Reader::new(bytes);
    while !reader.is_done() {
        let (number, value) = reader.field()?;
        match (number, value) {
            (1, Value::Varint(raw)) => start = signed32(raw),
            (2, Value::Varint(raw)) => end = signed32(raw),
            _ => {}
        }
    }
    Ok((start, end))
}

/// A descriptor `int32` arrives as a varint that was sign-extended to 64 bits.
fn signed32(raw: u64) -> i32 {
    let wide = crate::wire::u64_bits_to_i64(raw);
    i32::try_from(wide).unwrap_or(0)
}

fn utf8(payload: &[u8]) -> String {
    String::from_utf8_lossy(payload).into_owned()
}

fn join(scope: &str, name: &str) -> String {
    if scope.is_empty() {
        name.to_owned()
    } else {
        let mut joined = String::with_capacity(scope.len().saturating_add(name.len()) + 1);
        joined.push_str(scope);
        joined.push('.');
        joined.push_str(name);
        joined
    }
}
