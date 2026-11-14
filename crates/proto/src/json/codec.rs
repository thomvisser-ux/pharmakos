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
//!
//! And one place where it follows the mapping, which is worth writing down
//! because it looks like a deviation when you diff a golden file:
//!
//! * **A field at its proto3 default is not written**, as the mapping says.
//!   The consequence is that a default somebody *wrote out* does not come
//!   back: `examples/playbooks/expand_east.jsonc` opens with
//!   `"schema_version": {"major":1,"minor":0}` and
//!   `tests/golden/proto/expected.expand_east.json` reads `{"major": 1}`.
//!   Nothing is lost semantically — proto3 has no presence on a scalar, so
//!   `minor: 0` and an absent `minor` are the same message, and the canonical
//!   form is canonical precisely because it picks one spelling. But "the
//!   player's file round-trips byte for byte" (spec section 13) is a claim
//!   about the **JSONC layer**, not about this one: that layer preserves the
//!   author's text — comments, key order, spacing and a written-out default
//!   alike — and uses this codec for the canonical form it compares and
//!   verifies against. This codec has no `emit_defaults` mode and does not
//!   need one; adding one would give the canonical form two spellings, which
//!   is the one thing it may not have.
//!
//!   The **JSON → wire** direction holds up its half of that (decisions-log
//!   item 100 (10)): a field a document spells out at its default is dropped
//!   on the way to the wire, exactly as prost's encoder drops it, so
//!   [`super::canonicalise`] — which never sees a typed message — answers
//!   what `decode` then `encode` answers. [`is_proto3_default`] is the rule
//!   and says where it stops: `oneof` members, `optional` fields, repeated
//!   fields and message-typed fields all have presence in proto3 and are
//!   never dropped.

//!
//! # Cost (S3's `vfyq`, decisions-log item 158)
//!
//! QUICK's budget is spent here more than anywhere, so the codec is written to
//! be linear in the document with a small constant:
//!
//! * **A JSON Pointer is built only for an error.** The codec carries a
//!   [`Path`] of borrowed tokens and renders it when, and only when, it has
//!   something to report — the same string the eager pointer held.
//! * **The schema is indexed once** (`super::index`): a field is found by a
//!   binary search of its message's names and a nested type by an index, not
//!   by a string comparison down the descriptor's maps at every node.
//! * **Canonical text is written straight from the wire** ([`write_wire`]),
//!   without building a [`Json`] tree first, and it is byte for byte what
//!   [`super::write`] makes of [`wire_to_json`]'s answer.
//! * **Text is read straight to the wire** when it decodes
//!   (`super::direct`, behind [`super::decode_direct`]), with no tree at all;
//!   this module's [`json_to_wire`] is the exact path for a tree, and for a
//!   text with something to report.
//!
//! `super::reference` keeps the codec as it was before, test-only, and the
//! differential tests hold this one to it: equal bytes or the same error.

use std::fmt::Write as _;

use super::base64;
use super::index::{Index, Indexed, IndexedField, index};
use super::path::Path;
use super::value::{Error, Json, write_string};
use crate::descriptor::ScalarKind;
use crate::wire::{
    Reader, Value, Writer, i64_bits_to_u64, u64_bits_to_i64, zigzag_decode, zigzag_encode,
};

/// Deeper than any message in either package, and shallow enough that a
/// hostile document cannot exhaust the stack.
const MAX_DEPTH: usize = 64;

fn unknown_message(full_name: &str) -> Error {
    Error::at("", format!("`{full_name}` is not a message in the schema"))
}

/// Wire bytes for a message of this type, as canonical JSON.
///
/// # Errors
///
/// If the type is not in the schema, if the bytes are not a valid encoding of
/// it, or if the schema contains a floating-point field.
pub fn wire_to_json(full_name: &str, bytes: &[u8]) -> Result<Json, Error> {
    let index = index();
    let at = index
        .find(full_name)
        .ok_or_else(|| unknown_message(full_name))?;
    let mut decoder = Decoder {
        index,
        entries: Vec::new(),
    };
    decoder.message_to_json(at, bytes, &Path::Root, 0)
}

/// Canonical JSON for a message of this type, as wire bytes.
///
/// # Errors
///
/// If the type is not in the schema, if the document carries a field the
/// message does not declare, if a value does not fit its field, or if two
/// members of the same `oneof` are set at once.
pub fn json_to_wire(full_name: &str, value: &Json) -> Result<Vec<u8>, Error> {
    Ok(json_to_encoded(full_name, value)?.bytes)
}

/// The wire bytes [`json_to_wire`] writes for a document, with what the
/// codec learned about them on the way.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Encoded {
    bytes: Vec<u8>,
    plain: bool,
}

impl Encoded {
    pub(crate) const fn new(bytes: Vec<u8>, plain: bool) -> Encoded {
        Encoded { bytes, plain }
    }

    /// The wire bytes, exactly as [`json_to_wire`] answers.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Whether the bytes hold what prost's encoder writes for the message
    /// they decode to, up to the order of fields: every singular field at most
    /// once in each message, and no field without presence at its default.
    ///
    /// A document falls short of that only by spelling one field twice — once
    /// by its `.proto` name and once by its JSON name — or by spelling a
    /// default in a way `is_proto3_default` does not recognise, such as
    /// `"===="` for empty bytes. Decoding such bytes merges or drops what the
    /// wire says, so canonical text written from them is not the decoded
    /// message's; [`write_wire`] over plain bytes is.
    #[must_use]
    pub const fn is_plain(&self) -> bool {
        self.plain
    }
}

/// [`json_to_wire`], with what [`Encoded`] says about the bytes.
///
/// # Errors
///
/// As [`json_to_wire`].
pub fn json_to_encoded(full_name: &str, value: &Json) -> Result<Encoded, Error> {
    let index = index();
    let at = index
        .find(full_name)
        .ok_or_else(|| unknown_message(full_name))?;
    let mut encoder = Encoder {
        index,
        fields: Vec::new(),
        oneofs: Vec::new(),
        plain: true,
    };
    let bytes = encoder.json_to_message(at, value, &Path::Root, 0)?;
    Ok(Encoded {
        bytes,
        plain: encoder.plain,
    })
}

/// Canonical JSON text straight from wire bytes: exactly what
/// [`super::write`] makes of [`wire_to_json`]'s answer, without the tree in
/// between.
///
/// # Errors
///
/// As [`wire_to_json`].
pub fn write_wire(full_name: &str, bytes: &[u8]) -> Result<String, Error> {
    let mut out = String::with_capacity(bytes.len().saturating_mul(4));
    write_wire_into(full_name, bytes, &[], &mut out)?;
    Ok(out)
}

/// [`write_wire`], appending to `out`, with one field left out.
///
/// `omit` names the field by its path of `.proto` field names from the
/// message, [`crate::fingerprint::FINGERPRINT_FIELD`] for a playbook's
/// fingerprint, and the text is what [`write_wire`] writes for the same
/// message with that field cleared. Every name on the path must be a singular
/// field of the message before it, by its `.proto` name, and every name but
/// the last a message-typed one; a field the bytes do not carry leaves nothing
/// out, and an empty path leaves nothing out.
///
/// # Errors
///
/// As [`wire_to_json`], and a path that names no singular field, which is
/// refused before anything is written. Nothing is promised about what `out`
/// holds after an error.
pub fn write_wire_into(
    full_name: &str,
    bytes: &[u8],
    omit: &[&str],
    out: &mut String,
) -> Result<(), Error> {
    let index = index();
    let at = index
        .find(full_name)
        .ok_or_else(|| unknown_message(full_name))?;
    check_omit(index, at, full_name, omit)?;
    let mut decoder = Decoder {
        index,
        entries: Vec::new(),
    };
    let place = Place {
        path: &Path::Root,
        depth: 0,
        indent: 0,
    };
    decoder.write_message(at, bytes, place, omit, out)?;
    out.push('\n');
    Ok(())
}

/// Whether `omit` is a path [`write_wire_into`] can leave out of the message
/// at `at`: each name a singular field of the message before it, by its
/// `.proto` name (the name [`Decoder::write_message`] matches), and each name
/// but the last a field of a message type. A misspelt path would otherwise
/// leave nothing out without a word.
fn check_omit(index: &Index, at: usize, full_name: &str, omit: &[&str]) -> Result<(), Error> {
    let refused = || {
        Error::at(
            "",
            format!(
                "`{}` is not a path of singular fields of `{full_name}` by their `.proto` names, \
                 so there is no field to leave out",
                omit.join(".")
            ),
        )
    };
    let mut message = at;
    let mut names = omit.iter().peekable();
    while let Some(name) = names.next() {
        let indexed = index.get(message).ok_or_else(refused)?;
        let field = indexed
            .fields
            .iter()
            .find(|candidate| candidate.field.name == *name && !candidate.field.repeated)
            .ok_or_else(refused)?;
        if names.peek().is_some() {
            message = field.nested.ok_or_else(refused)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Wire -> JSON
// ---------------------------------------------------------------------------

struct Decoder<'b> {
    index: &'static Index,
    /// Every field of every message on the way down, as a stack: a message
    /// reads its fields onto the top and pops them when it is done.
    entries: Vec<(i32, Value<'b>)>,
}

/// Where a value being written stands: its pointer, its message depth (for
/// [`MAX_DEPTH`]) and its indentation in the text.
#[derive(Clone, Copy)]
struct Place<'p> {
    path: &'p Path<'p>,
    depth: usize,
    indent: usize,
}

impl<'b> Decoder<'b> {
    fn indexed(&self, at: usize, path: &Path<'_>) -> Result<&'static Indexed, Error> {
        self.index
            .get(at)
            .ok_or_else(|| Error::at(&path.render(), "a message index outside the schema"))
    }

    /// Reads one message's fields onto the stack, in ascending field number
    /// and, within a number, in wire order — the grouping `wire_to_json` used
    /// a `BTreeMap` of lists for. Returns where they start.
    fn read_fields(
        &mut self,
        message: &Indexed,
        bytes: &'b [u8],
        path: &Path<'_>,
    ) -> Result<usize, Error> {
        let base = self.entries.len();
        let mut reader = Reader::new(bytes);
        while !reader.is_done() {
            let field = reader.field().map_err(|error| {
                Error::at(
                    &path.render(),
                    format!(
                        "`{}` is not a valid encoding: {error:?}",
                        message.message.full_name
                    ),
                )
            })?;
            self.entries.push(field);
        }
        if let Some(read) = self.entries.get_mut(base..)
            && !read.is_sorted_by_key(|(number, _)| *number)
        {
            // Stable, so a repeated field's elements keep their wire order.
            read.sort_by_key(|(number, _)| *number);
        }
        Ok(base)
    }

    /// The end of the run of entries that share `start`'s field number.
    fn run_end(&self, start: usize, number: i32) -> usize {
        let mut end = start;
        while self
            .entries
            .get(end)
            .is_some_and(|(found, _)| *found == number)
        {
            end = end.saturating_add(1);
        }
        end
    }

    fn entry(&self, at: usize) -> Option<(i32, Value<'b>)> {
        self.entries.get(at).copied()
    }

    /// The last value of a run: proto3's last occurrence of a singular field,
    /// which wins.
    fn last(&self, end: usize, path: &Path<'_>) -> Result<Value<'b>, Error> {
        end.checked_sub(1)
            .and_then(|slot| self.entry(slot))
            .map(|(_, value)| value)
            .ok_or_else(|| Error::at(&path.render(), "a field with no value"))
    }

    fn message_to_json(
        &mut self,
        at: usize,
        bytes: &'b [u8],
        path: &Path<'_>,
        depth: usize,
    ) -> Result<Json, Error> {
        if depth > MAX_DEPTH {
            return Err(Error::at(
                &path.render(),
                "a message nested deeper than 64 levels",
            ));
        }
        let message = self.indexed(at, path)?;
        let base = self.read_fields(message, bytes, path)?;
        let mut entries: Vec<(String, Json)> = Vec::new();
        let mut start = base;
        while let Some((number, _)) = self.entry(start) {
            let end = self.run_end(start, number);
            let field = declared(message, number, path)?;
            let field_path = Path::Key(path, &field.field.name);
            let json = if field.field.repeated {
                let mut items: Vec<Json> = Vec::new();
                let mut elements: Vec<Value<'b>> = Vec::new();
                for slot in start..end {
                    let Some((_, value)) = self.entry(slot) else {
                        break;
                    };
                    elements.clear();
                    unpack(field.field.kind, value, &field_path, &mut elements)?;
                    for element in elements.drain(..) {
                        items.push(self.scalar_to_json(field, element, &field_path, depth)?);
                    }
                }
                Json::Array(items)
            } else {
                let last = self.last(end, &field_path)?;
                self.scalar_to_json(field, last, &field_path, depth)?
            };
            entries.push((field.field.name.clone(), json));
            start = end;
        }
        self.entries.truncate(base);
        Ok(Json::Object(entries))
    }

    fn scalar_to_json(
        &mut self,
        field: &IndexedField,
        value: Value<'b>,
        path: &Path<'_>,
        depth: usize,
    ) -> Result<Json, Error> {
        Ok(match scalar_of(field, value, path)? {
            Scalar::Bool(flag) => Json::Bool(flag),
            Scalar::Signed(number) => Json::Number(number.to_string()),
            Scalar::Unsigned(number) => Json::Number(number.to_string()),
            Scalar::QuotedSigned(number) => Json::String(number.to_string()),
            Scalar::QuotedUnsigned(number) => Json::String(number.to_string()),
            Scalar::Bytes(bytes) => Json::String(base64::encode(bytes)),
            Scalar::Text(text) => Json::String(text.to_owned()),
            Scalar::Name(name) => Json::String(name.to_owned()),
            Scalar::Message(nested, payload) => {
                self.message_to_json(nested, payload, path, depth.saturating_add(1))?
            }
        })
    }

    /// One message as canonical text, at `place`'s indentation, leaving out
    /// the field `omit` names.
    fn write_message(
        &mut self,
        at: usize,
        bytes: &'b [u8],
        place: Place<'_>,
        omit: &[&str],
        out: &mut String,
    ) -> Result<(), Error> {
        let path = place.path;
        if place.depth > MAX_DEPTH {
            return Err(Error::at(
                &path.render(),
                "a message nested deeper than 64 levels",
            ));
        }
        let message = self.indexed(at, path)?;
        let base = self.read_fields(message, bytes, path)?;
        let inner = place.indent.saturating_add(1);
        let mut first = true;
        let mut start = base;
        while let Some((number, _)) = self.entry(start) {
            let end = self.run_end(start, number);
            let field = declared(message, number, path)?;
            let (leave_out, omit_below): (bool, &[&str]) = match omit.split_first() {
                Some((name, rest)) if *name == field.field.name && !field.field.repeated => {
                    (rest.is_empty(), rest)
                }
                _ => (false, &[]),
            };
            if leave_out {
                start = end;
                continue;
            }
            out.push_str(if first { "{\n" } else { ",\n" });
            first = false;
            push_indent(inner, out);
            out.push_str(&field.key);
            let field_path = Path::Key(path, &field.field.name);
            let here = Place {
                path: &field_path,
                depth: place.depth,
                indent: inner,
            };
            if field.field.repeated {
                let item = Place {
                    indent: inner.saturating_add(1),
                    ..here
                };
                // Each wire value is unpacked and its elements written before
                // the next value is read, the order `wire_to_json` reports a
                // bad element or a bad packed payload in.
                let mut none = true;
                let mut elements: Vec<Value<'b>> = Vec::new();
                for slot in start..end {
                    let Some((_, value)) = self.entry(slot) else {
                        break;
                    };
                    elements.clear();
                    unpack(field.field.kind, value, &field_path, &mut elements)?;
                    for element in elements.drain(..) {
                        out.push_str(if none { "[\n" } else { ",\n" });
                        none = false;
                        push_indent(item.indent, out);
                        self.write_scalar(field, element, item, &[], out)?;
                    }
                }
                if none {
                    out.push_str("[]");
                } else {
                    out.push('\n');
                    push_indent(inner, out);
                    out.push(']');
                }
            } else {
                let last = self.last(end, &field_path)?;
                self.write_scalar(field, last, here, omit_below, out)?;
            }
            start = end;
        }
        self.entries.truncate(base);
        if first {
            out.push_str("{}");
        } else {
            out.push('\n');
            push_indent(place.indent, out);
            out.push('}');
        }
        Ok(())
    }

    fn write_scalar(
        &mut self,
        field: &IndexedField,
        value: Value<'b>,
        place: Place<'_>,
        omit: &[&str],
        out: &mut String,
    ) -> Result<(), Error> {
        match scalar_of(field, value, place.path)? {
            Scalar::Bool(true) => out.push_str("true"),
            Scalar::Bool(false) => out.push_str("false"),
            // Writing to a `String` cannot fail. A quoted number and base64
            // hold nothing `write_string` would escape, so they are quoted
            // here directly.
            Scalar::Signed(number) => {
                let _infallible = write!(out, "{number}");
            }
            Scalar::Unsigned(number) => {
                let _infallible = write!(out, "{number}");
            }
            Scalar::QuotedSigned(number) => {
                let _infallible = write!(out, "\"{number}\"");
            }
            Scalar::QuotedUnsigned(number) => {
                let _infallible = write!(out, "\"{number}\"");
            }
            Scalar::Bytes(bytes) => {
                out.push('"');
                out.push_str(&base64::encode(bytes));
                out.push('"');
            }
            Scalar::Text(text) | Scalar::Name(text) => write_string(text, out),
            Scalar::Message(nested, payload) => {
                let below = Place {
                    depth: place.depth.saturating_add(1),
                    ..place
                };
                self.write_message(nested, payload, below, omit, out)?;
            }
        }
        Ok(())
    }
}

/// Two spaces for each of the first 64 levels, which every playbook's text
/// stays within.
const SPACES: &str = concat!(
    "                                                                ",
    "                                                                ",
);

/// Two spaces a level: copied from [`SPACES`] in one piece, or pushed one by
/// one past its 64 levels. Either way the same spaces.
fn push_indent(depth: usize, out: &mut String) {
    let width = depth.saturating_mul(2);
    match SPACES.get(..width) {
        Some(spaces) => out.push_str(spaces),
        None => out.extend(core::iter::repeat_n(' ', width)),
    }
}

fn declared<'m>(
    message: &'m Indexed,
    number: i32,
    path: &Path<'_>,
) -> Result<&'m IndexedField, Error> {
    message.field_by_number(number).ok_or_else(|| {
        Error::at(
            &path.render(),
            format!(
                "field number {number} is not declared on `{}`",
                message.message.full_name
            ),
        )
    })
}

/// One wire value of a field, as the JSON it reads as. Numbers are carried
/// as numbers and strings and enum names are borrowed, so writing them as text
/// formats or copies them once, straight into the text.
enum Scalar<'b> {
    Bool(bool),
    /// A bare JSON number: a 32-bit integer or an unknown enum number.
    Signed(i64),
    /// A bare JSON number: an unsigned 32-bit integer.
    Unsigned(u64),
    /// A signed 64-bit integer, which the mapping quotes.
    QuotedSigned(i64),
    /// An unsigned 64-bit integer, which the mapping quotes.
    QuotedUnsigned(u64),
    /// A bytes field's payload, written as base64.
    Bytes(&'b [u8]),
    /// A string field's text.
    Text(&'b str),
    /// An enum value's name.
    Name(&'static str),
    /// A message: its index and its payload.
    Message(usize, &'b [u8]),
}

/// Appends the elements a repeated field's wire value holds: the elements of
/// a packed payload, or the value itself. prost writes packable repeated
/// fields packed, so this is the common path on the way out; a repeated field
/// written unpacked arrives one element per tag and passes straight through.
fn unpack<'b>(
    kind: ScalarKind,
    value: Value<'b>,
    path: &Path<'_>,
    out: &mut Vec<Value<'b>>,
) -> Result<(), Error> {
    let (true, Value::Delimited(payload)) = (packable(kind), value) else {
        out.push(value);
        return Ok(());
    };
    let mut reader = Reader::new(payload);
    while !reader.is_done() {
        let fail = |error| {
            Error::at(
                &path.render(),
                format!("a packed repeated field: {error:?}"),
            )
        };
        let element = match kind {
            ScalarKind::Fixed32 | ScalarKind::Sfixed32 | ScalarKind::Float => {
                let array: [u8; 4] = reader
                    .raw(4)
                    .map_err(fail)?
                    .try_into()
                    .map_err(|_| Error::at(&path.render(), "a truncated packed fixed32"))?;
                Value::Fixed32(u32::from_le_bytes(array))
            }
            ScalarKind::Fixed64 | ScalarKind::Sfixed64 | ScalarKind::Double => {
                let array: [u8; 8] = reader
                    .raw(8)
                    .map_err(fail)?
                    .try_into()
                    .map_err(|_| Error::at(&path.render(), "a truncated packed fixed64"))?;
                Value::Fixed64(u64::from_le_bytes(array))
            }
            _ => Value::Varint(reader.varint().map_err(fail)?),
        };
        out.push(element);
    }
    Ok(())
}

const fn packable(kind: ScalarKind) -> bool {
    !matches!(
        kind,
        ScalarKind::String | ScalarKind::Bytes | ScalarKind::Message
    )
}

fn scalar_of<'b>(
    field: &IndexedField,
    value: Value<'b>,
    path: &Path<'_>,
) -> Result<Scalar<'b>, Error> {
    let at = |message: &str| Error::at(&path.render(), message);
    match field.field.kind {
        ScalarKind::Double | ScalarKind::Float => Err(at(
            "a floating-point field: the sim is integer-only and this codec refuses one",
        )),
        ScalarKind::Bool => Ok(Scalar::Bool(varint(value, path)? != 0)),
        ScalarKind::Int32 => {
            let wide = u64_bits_to_i64(varint(value, path)?);
            let narrow =
                i32::try_from(wide).map_err(|_| at("an int32 field carrying a wider value"))?;
            Ok(Scalar::Signed(i64::from(narrow)))
        }
        ScalarKind::Sint32 => {
            let wide = zigzag_decode(varint(value, path)?);
            let narrow =
                i32::try_from(wide).map_err(|_| at("an sint32 field carrying a wider value"))?;
            Ok(Scalar::Signed(i64::from(narrow)))
        }
        ScalarKind::Uint32 => {
            let narrow = u32::try_from(varint(value, path)?)
                .map_err(|_| at("a uint32 field carrying a wider value"))?;
            Ok(Scalar::Signed(i64::from(narrow)))
        }
        ScalarKind::Fixed32 => Ok(Scalar::Unsigned(u64::from(fixed32(value, path)?))),
        ScalarKind::Sfixed32 => {
            let bits = fixed32(value, path)?;
            Ok(Scalar::Signed(i64::from(i32::from_ne_bytes(
                bits.to_ne_bytes(),
            ))))
        }
        // The 64-bit kinds are quoted strings, which is the whole reason no
        // duration field in gp.v1 is one (decisions-log item 46).
        ScalarKind::Int64 => Ok(Scalar::QuotedSigned(u64_bits_to_i64(varint(value, path)?))),
        ScalarKind::Sint64 => Ok(Scalar::QuotedSigned(zigzag_decode(varint(value, path)?))),
        ScalarKind::Uint64 => Ok(Scalar::QuotedUnsigned(varint(value, path)?)),
        ScalarKind::Fixed64 => Ok(Scalar::QuotedUnsigned(fixed64(value, path)?)),
        ScalarKind::Sfixed64 => {
            let bits = fixed64(value, path)?;
            Ok(Scalar::QuotedSigned(u64_bits_to_i64(bits)))
        }
        ScalarKind::String => {
            let payload = delimited(value, path)?;
            let text = std::str::from_utf8(payload)
                .map_err(|_| at("a string field that is not valid UTF-8"))?;
            Ok(Scalar::Text(text))
        }
        ScalarKind::Bytes => Ok(Scalar::Bytes(delimited(value, path)?)),
        ScalarKind::Enum => {
            let number = i32::try_from(u64_bits_to_i64(varint(value, path)?))
                .map_err(|_| at("an enum value out of range"))?;
            let declared = field
                .enumeration
                .ok_or_else(|| at(&format!("`{}` is not in the schema", field.field.type_name)))?;
            Ok(declared.value_by_number(number).map_or_else(
                // An unrecognised number is written as a number, which is what
                // the proto3 JSON mapping says and what lets a diagnostic name
                // the offending value instead of losing it.
                || Scalar::Signed(i64::from(number)),
                |found| Scalar::Name(found.name.as_str()),
            ))
        }
        ScalarKind::Message => {
            let nested = field
                .nested
                .ok_or_else(|| at(&format!("`{}` is not in the schema", field.field.type_name)))?;
            Ok(Scalar::Message(nested, delimited(value, path)?))
        }
    }
}

fn varint(value: Value<'_>, path: &Path<'_>) -> Result<u64, Error> {
    match value {
        Value::Varint(raw) => Ok(raw),
        _ => Err(Error::at(&path.render(), "expected a varint on the wire")),
    }
}

fn fixed32(value: Value<'_>, path: &Path<'_>) -> Result<u32, Error> {
    match value {
        Value::Fixed32(raw) => Ok(raw),
        _ => Err(Error::at(
            &path.render(),
            "expected four fixed bytes on the wire",
        )),
    }
}

fn fixed64(value: Value<'_>, path: &Path<'_>) -> Result<u64, Error> {
    match value {
        Value::Fixed64(raw) => Ok(raw),
        _ => Err(Error::at(
            &path.render(),
            "expected eight fixed bytes on the wire",
        )),
    }
}

fn delimited<'b>(value: Value<'b>, path: &Path<'_>) -> Result<&'b [u8], Error> {
    match value {
        Value::Delimited(payload) => Ok(payload),
        _ => Err(Error::at(
            &path.render(),
            "expected a length-delimited value",
        )),
    }
}

// ---------------------------------------------------------------------------
// JSON -> wire
// ---------------------------------------------------------------------------

struct Encoder<'j> {
    index: &'static Index,
    /// The resolved members of every object on the way down, as a stack.
    fields: Vec<(&'static IndexedField, &'j Json)>,
    /// The `oneof`s each object on the way down has set, with the member name
    /// that set it, as a stack.
    oneofs: Vec<(i32, &'j str)>,
    /// [`Encoded::is_plain`].
    plain: bool,
}

impl<'j> Encoder<'j> {
    fn json_to_message(
        &mut self,
        at: usize,
        value: &'j Json,
        path: &Path<'_>,
        depth: usize,
    ) -> Result<Vec<u8>, Error> {
        if depth > MAX_DEPTH {
            return Err(Error::at(
                &path.render(),
                "a message nested deeper than 64 levels",
            ));
        }
        let message: &'static Indexed = self
            .index
            .get(at)
            .ok_or_else(|| Error::at(&path.render(), "a message index outside the schema"))?;
        let Json::Object(entries) = value else {
            return Err(Error::at(
                &path.render(),
                format!(
                    "expected an object for `{}`, found {}",
                    message.message.full_name,
                    value.kind()
                ),
            ));
        };

        // Resolve every key first, so an unknown field is reported before
        // anything is written — and so the write order can be by field number
        // rather than by the order somebody happened to type them in.
        let base = self.fields.len();
        let oneof_base = self.oneofs.len();
        for (key, item) in entries {
            let field = message.field_by_name(key).ok_or_else(|| {
                Error::at(
                    &Path::Key(path, key).render(),
                    format!(
                        "`{key}` is not a field of `{}`. Unknown fields are rejected, never \
                         stripped.",
                        message.message.full_name
                    ),
                )
            })?;
            if matches!(item, Json::Null) {
                // The mapping treats null as "the default value", and a
                // default is not written on the wire. Recording it as a no-op
                // keeps a hand-written file that spells a field out as null
                // loadable.
                continue;
            }
            if let Some(index) = field.field.oneof_index {
                let previous = self
                    .oneofs
                    .get(oneof_base..)
                    .and_then(|set| set.iter().find(|(used, _)| *used == index));
                if let Some((_, previous)) = previous {
                    return Err(Error::at(
                        &Path::Key(path, key).render(),
                        format!(
                            "`{key}` and `{previous}` are both set, but only one member of a \
                             oneof may be"
                        ),
                    ));
                }
                self.oneofs.push((index, key.as_str()));
            } else if is_proto3_default(field, item) {
                // A field WRITTEN OUT at its proto3 default is not written to
                // the wire, exactly as prost's own encoder drops it.
                // `is_proto3_default` says why that has to happen here rather
                // than in `canonicalise`, and where the rule stops.
                continue;
            }
            self.fields.push((field, item));
        }
        self.oneofs.truncate(oneof_base);
        let end = self.fields.len();
        if let Some(resolved) = self.fields.get_mut(base..end) {
            if !resolved.is_sorted_by_key(|(field, _)| field.field.number) {
                // Stable, so a field spelled twice keeps its document order.
                resolved.sort_by_key(|(field, _)| field.field.number);
            }
            if spelled_twice(resolved) {
                self.plain = false;
            }
        }

        let mut writer = Writer::default();
        for slot in base..end {
            let Some(&(field, item)) = self.fields.get(slot) else {
                break;
            };
            let field_path = Path::Key(path, &field.field.name);
            if field.field.repeated {
                let Json::Array(items) = item else {
                    return Err(Error::at(
                        &field_path.render(),
                        format!("expected an array, found {}", item.kind()),
                    ));
                };
                for (index, element) in items.iter().enumerate() {
                    let element_path = Path::Index(&field_path, index);
                    self.write_scalar(field, element, &element_path, depth, &mut writer)?;
                }
            } else {
                self.write_scalar(field, item, &field_path, depth, &mut writer)?;
            }
        }
        self.fields.truncate(base);
        Ok(writer.into_vec())
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one arm per Protobuf scalar kind; splitting it would hide the \
                  one-to-one correspondence with the descriptor's type enum"
    )]
    fn write_scalar(
        &mut self,
        field: &'static IndexedField,
        value: &'j Json,
        path: &Path<'_>,
        depth: usize,
        writer: &mut Writer,
    ) -> Result<(), Error> {
        let at = |message: &str| Error::at(&path.render(), message);
        let number = field.field.number;
        // Whether the value written is the field's default, for
        // `Encoded::is_plain`: a field without presence is never at its
        // default in plain bytes.
        let zero = match field.field.kind {
            ScalarKind::Double | ScalarKind::Float => {
                return Err(at(
                    "a floating-point field: the sim is integer-only and this codec refuses one",
                ));
            }
            ScalarKind::Bool => {
                let Json::Bool(flag) = value else {
                    return Err(at(&format!(
                        "expected true or false, found {}",
                        value.kind()
                    )));
                };
                writer.varint_field(number, u64::from(*flag));
                !*flag
            }
            ScalarKind::Int32 => {
                let narrow = i32::try_from(integer(value, path)?)
                    .map_err(|_| at("out of range for an int32 field"))?;
                writer.varint_field(number, i64_bits_to_u64(i64::from(narrow)));
                narrow == 0
            }
            ScalarKind::Sint32 => {
                let narrow = i32::try_from(integer(value, path)?)
                    .map_err(|_| at("out of range for an sint32 field"))?;
                writer.varint_field(number, zigzag_encode(i64::from(narrow)));
                narrow == 0
            }
            ScalarKind::Uint32 => {
                let narrow = u32::try_from(unsigned(value, path)?)
                    .map_err(|_| at("out of range for a uint32 field"))?;
                writer.varint_field(number, u64::from(narrow));
                narrow == 0
            }
            ScalarKind::Fixed32 => {
                let narrow = u32::try_from(unsigned(value, path)?)
                    .map_err(|_| at("out of range for a fixed32 field"))?;
                writer.fixed32_field(number, narrow);
                narrow == 0
            }
            ScalarKind::Sfixed32 => {
                let narrow = i32::try_from(integer(value, path)?)
                    .map_err(|_| at("out of range for an sfixed32 field"))?;
                writer.fixed32_field(number, u32::from_ne_bytes(narrow.to_ne_bytes()));
                narrow == 0
            }
            ScalarKind::Int64 => {
                let wide = integer(value, path)?;
                writer.varint_field(number, i64_bits_to_u64(wide));
                wide == 0
            }
            ScalarKind::Sint64 => {
                let wide = integer(value, path)?;
                writer.varint_field(number, zigzag_encode(wide));
                wide == 0
            }
            ScalarKind::Uint64 => {
                let wide = unsigned(value, path)?;
                writer.varint_field(number, wide);
                wide == 0
            }
            ScalarKind::Fixed64 => {
                let wide = unsigned(value, path)?;
                writer.fixed64_field(number, wide);
                wide == 0
            }
            ScalarKind::Sfixed64 => {
                let wide = integer(value, path)?;
                writer.fixed64_field(number, i64_bits_to_u64(wide));
                wide == 0
            }
            ScalarKind::String => {
                let Json::String(text) = value else {
                    return Err(at(&format!("expected a string, found {}", value.kind())));
                };
                writer
                    .delimited_field(number, text.as_bytes())
                    .map_err(|_| at("a string too long for its length prefix"))?;
                text.is_empty()
            }
            ScalarKind::Bytes => {
                let Json::String(text) = value else {
                    return Err(at(&format!(
                        "expected base64 in a string, found {}",
                        value.kind()
                    )));
                };
                let bytes = base64::decode(text).ok_or_else(|| at("not valid base64"))?;
                writer
                    .delimited_field(number, &bytes)
                    .map_err(|_| at("bytes too long for their length prefix"))?;
                bytes.is_empty()
            }
            ScalarKind::Enum => {
                let declared = field.enumeration.ok_or_else(|| {
                    at(&format!("`{}` is not in the schema", field.field.type_name))
                })?;
                let found = match value {
                    Json::String(name) => {
                        declared
                            .value_by_name(name)
                            .ok_or_else(|| {
                                at(&format!(
                                    "`{name}` is not a value of `{}`",
                                    field.field.type_name
                                ))
                            })?
                            .number
                    }
                    Json::Number(_) => i32::try_from(integer(value, path)?)
                        .map_err(|_| at("an enum value out of range"))?,
                    other => {
                        return Err(at(&format!(
                            "expected an enum value name, found {}",
                            other.kind()
                        )));
                    }
                };
                writer.varint_field(number, i64_bits_to_u64(i64::from(found)));
                found == 0
            }
            ScalarKind::Message => {
                let nested = field.nested.ok_or_else(|| {
                    at(&format!("`{}` is not in the schema", field.field.type_name))
                })?;
                let payload = self.json_to_message(nested, value, path, depth.saturating_add(1))?;
                writer
                    .delimited_field(number, &payload)
                    .map_err(|_| at("a message too long for its length prefix"))?;
                false
            }
        };
        if zero && !field.presence {
            self.plain = false;
        }
        Ok(())
    }
}

/// Whether a message's resolved members, in field-number order, write one
/// singular field twice: the first half of [`Encoded::is_plain`].
pub(crate) fn spelled_twice(resolved: &[(&IndexedField, &Json)]) -> bool {
    resolved.windows(2).any(|pair| {
        matches!(pair, [(first, _), (second, _)]
            if first.field.number == second.field.number && !first.field.repeated)
    })
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
fn is_proto3_default(field: &IndexedField, value: &Json) -> bool {
    match value {
        Json::Bool(flag) => is_proto3_default_lexeme(field, Lexeme::Bool(*flag)),
        Json::Number(lexeme) => is_proto3_default_lexeme(field, Lexeme::Number(lexeme)),
        Json::String(text) => is_proto3_default_lexeme(field, Lexeme::String(text)),
        // No field's default is spelled as null (which is skipped before this
        // is asked), an array or an object.
        Json::Null | Json::Array(_) | Json::Object(_) => false,
    }
}

/// A scalar token, as [`is_proto3_default_lexeme`] reads it: from a tree, or
/// straight off the text (`super::direct`).
#[derive(Clone, Copy, Debug)]
pub(crate) enum Lexeme<'a> {
    Bool(bool),
    Number(&'a str),
    String(&'a str),
}

/// [`is_proto3_default`] for one scalar token: the rule, in one place for the
/// tree and for the direct reader.
pub(crate) fn is_proto3_default_lexeme(field: &IndexedField, value: Lexeme<'_>) -> bool {
    if field.field.repeated {
        return false;
    }
    match field.field.kind {
        // Two different reasons for one answer. A present submessage HAS
        // presence, so `{}` is not absence and must be written. A
        // floating-point field is refused outright by `write_scalar`, and must
        // reach it to be refused: a dropped field is not an error, and a float
        // field has to be one.
        ScalarKind::Message | ScalarKind::Double | ScalarKind::Float => false,
        ScalarKind::Bool => matches!(value, Lexeme::Bool(false)),
        ScalarKind::String | ScalarKind::Bytes => {
            matches!(value, Lexeme::String(text) if text.is_empty())
        }
        ScalarKind::Enum => field.enumeration.is_some_and(|declared| match value {
            Lexeme::String(name) => declared
                .value_by_name(name)
                .is_some_and(|it| it.number == 0),
            Lexeme::Number(lexeme) => is_zero(lexeme),
            Lexeme::Bool(_) => false,
        }),
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
            Lexeme::Number(lexeme) | Lexeme::String(lexeme) => is_zero(lexeme),
            Lexeme::Bool(_) => false,
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

/// A JSON integer, from a bare number or from the quoted form the mapping also
/// accepts. A fractional or exponential lexeme is refused: this schema has no
/// field that could hold one.
fn integer(value: &Json, path: &Path<'_>) -> Result<i64, Error> {
    let lexeme = match value {
        Json::Number(text) | Json::String(text) => text,
        other => {
            return Err(Error::at(
                &path.render(),
                format!("expected an integer, found {}", other.kind()),
            ));
        }
    };
    lexeme.parse::<i64>().map_err(|_| {
        Error::at(
            &path.render(),
            format!("`{lexeme}` is not a whole number. This schema has no fractional field."),
        )
    })
}

fn unsigned(value: &Json, path: &Path<'_>) -> Result<u64, Error> {
    let lexeme = match value {
        Json::Number(text) | Json::String(text) => text,
        other => {
            return Err(Error::at(
                &path.render(),
                format!("expected a non-negative integer, found {}", other.kind()),
            ));
        }
    };
    lexeme.parse::<u64>().map_err(|_| {
        Error::at(
            &path.render(),
            format!("`{lexeme}` is not a non-negative whole number"),
        )
    })
}
