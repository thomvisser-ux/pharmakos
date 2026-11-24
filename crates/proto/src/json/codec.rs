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
//!   [`super::write`] makes of [`wire_to_json`]'s answer; and
//!   [`write_wire_into`] hands it on in pieces of one fixed buffer, so a
//!   sink that hashes them never holds the text (S3's `fpstream`).
//! * **Text is read straight to the wire** when it decodes
//!   (`super::direct`, behind [`super::decode_direct`]), with no tree at all;
//!   this module's [`json_to_wire`] is the exact path for a tree, and for a
//!   text with something to report.
//!
//! `super::reference` keeps the codec as it was before, test-only, and the
//! differential tests hold this one to it: equal bytes or the same error.

use super::base64;
use super::index::{Index, Indexed, IndexedField, index};
use super::path::Path;
use super::value::{Error, ErrorKind, Json, is_integer_lexeme, is_misspelt_integer, write_string};
use crate::descriptor::ScalarKind;
use crate::wire::{
    Reader, Value, Writer, i64_bits_to_u64, u64_bits_to_i64, zigzag_decode, zigzag_encode,
};

// Deeper than any message in either package, and shallow enough that a
// hostile document cannot exhaust the stack: the grammar's one limit
// ([`super::grammar::MAX_DEPTH`]), a message nesting counted as a value does.
use super::grammar::{Said, may_nest, quoted};

fn unknown_message(full_name: &str) -> Error {
    Error::at(
        ErrorKind::Schema,
        "",
        crate::said!("`{}` is not a message in the schema", quoted(full_name)),
    )
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

/// The wire bytes [`json_to_wire`] writes for a document.
///
/// They hold what prost's encoder writes for the message they decode to, up
/// to the order of fields: every singular field at most once in each message,
/// and no field without presence at its default. So canonical text written
/// from them ([`write_wire`]) is the decoded message's, without encoding it
/// again.
///
/// The codec makes that so by what it refuses (S3's `vfy6`, follow-up C): a
/// field written under both its `.proto` name and its JSON name, as a key
/// written twice is; and a default the codec does not drop, which since the
/// number grammar (RFC 8259's, follow-up B) and base64's padding rule leaves
/// none. Until then a document could fall short of it, and
/// `Encoded::is_plain` said so; nothing can now, and the differential tests
/// hold every accepted generated playbook's canonical text from its bytes to
/// the text of the playbook encoded again.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Encoded {
    bytes: Vec<u8>,
}

impl Encoded {
    pub(crate) const fn new(bytes: Vec<u8>) -> Encoded {
        Encoded { bytes }
    }

    /// The wire bytes, exactly as [`json_to_wire`] answers.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// [`json_to_wire`], as an [`Encoded`].
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
        named: Vec::new(),
    };
    let bytes = encoder.json_to_message(at, value, &Path::Root, 0)?;
    Ok(Encoded { bytes })
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
    write_into(full_name, bytes, &[], &mut out)?;
    super::work::add(out.len());
    Ok(out)
}

/// [`write_wire`], with one field left out, handed to `sink` in pieces as it
/// is written.
///
/// `omit` names the field by its path of `.proto` field names from the
/// message, [`crate::fingerprint::FINGERPRINT_FIELD`] for a playbook's
/// fingerprint, and the text is what [`write_wire`] writes for the same
/// message with that field cleared. Every name on the path must be a singular
/// field of the message before it, by its `.proto` name, and every name but
/// the last a message-typed one; a field the bytes do not carry leaves nothing
/// out, and an empty path leaves nothing out.
///
/// The text reaches `sink` in order, through [`Extend::extend`], in pieces of
/// at most [`CHUNK`] bytes gathered in one buffer of that size (S3's
/// `fpstream`): the pieces joined are the text, byte for byte, and the text is
/// never held whole here, so a sink that hashes each piece as it comes (the
/// plan fingerprint's) takes a fingerprint in a fixed amount of memory. A
/// `String` is a sink, and is appended to. A cut may fall anywhere, inside a
/// string's escapes included, never inside a character.
///
/// # Errors
///
/// As [`wire_to_json`], and a path that names no singular field, which is
/// refused before anything is written. Nothing is promised about what the
/// sink holds after an error.
pub fn write_wire_into<S>(
    full_name: &str,
    bytes: &[u8],
    omit: &[&str],
    sink: &mut S,
) -> Result<(), Error>
where
    S: for<'t> Extend<&'t str> + ?Sized,
{
    let mut chunks = Chunks::new(sink, CHUNK);
    write_into(full_name, bytes, omit, &mut chunks)?;
    chunks.flush();
    Ok(())
}

/// The most text [`write_wire_into`] gathers before it hands it on: 8 KiB.
///
/// What it buys is fewer, longer pieces for the sink, which for xxh3's
/// streaming state means fewer calls per byte (each call pays a fixed cost
/// before it reaches the block loop); what it costs is one buffer of this
/// size per text. Any size writes the same pieces joined, so it is a cost
/// and never a different text. Untimed: this machine was not idle when the
/// size was chosen, so it is the order of a first-level data cache, well past
/// xxh3's 1 KiB block, rather than a measured optimum.
const CHUNK: Limit = match Limit::new(8 << 10) {
    Some(limit) => limit,
    None => panic!("the chunk is too small to hold one escaped character"),
};

/// The text of the message at `full_name` with `omit` left out, written to
/// `out`, newline included.
fn write_into<O: Out>(
    full_name: &str,
    bytes: &[u8],
    omit: &[&str],
    out: &mut O,
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

/// Where the canonical writer puts its text: a `String`, appended to in
/// place, or [`Chunks`], which hands it on in pieces. The writer is the same
/// code for both, so the two cannot write different text.
trait Out: core::fmt::Write {
    /// Append `text` as it is.
    fn push_str(&mut self, text: &str);

    /// Append one character.
    fn push(&mut self, character: char);

    /// Append `text` as a JSON string, quoted and escaped as
    /// [`write_string`] writes it.
    ///
    /// # Errors
    ///
    /// Only [`Chunks`] can fail, and only if [`write_string`] stops putting a
    /// string between two quotes (see [`Chunks::json_string`]).
    fn json_string(&mut self, text: &str, path: &Path<'_>) -> Result<(), Error>;
}

impl Out for String {
    fn push_str(&mut self, text: &str) {
        String::push_str(self, text);
    }

    fn push(&mut self, character: char) {
        String::push(self, character);
    }

    fn json_string(&mut self, text: &str, _path: &Path<'_>) -> Result<(), Error> {
        write_string(text, self);
        Ok(())
    }
}

/// How much text [`Chunks`] gathers, and how long a piece of a string it
/// escapes at once: every character of a piece escaped at its longest, with
/// two quotes, still fits. Only [`Limit::new`] makes one, so a buffer too small
/// for one escaped character cannot be asked for.
#[derive(Clone, Copy, Debug)]
struct Limit {
    /// The most bytes the buffer holds.
    bytes: usize,
    /// The most bytes of a string escaped straight into the buffer, and the
    /// most a long string's pieces take: `(bytes - 2) / 6`, at least one.
    piece: usize,
}

/// The longest a character's escape is: `\u` and four hex digits.
const LONGEST_ESCAPE: usize = 6;

impl Limit {
    /// A limit of `bytes`, or `None` when the buffer could not hold one
    /// character escaped at its longest between two quotes.
    const fn new(bytes: usize) -> Option<Limit> {
        let Some(room) = bytes.checked_sub(2) else {
            return None;
        };
        match room.checked_div(LONGEST_ESCAPE) {
            Some(piece) if piece > 0 => Some(Limit { bytes, piece }),
            _ => None,
        }
    }
}

/// Text gathered in one buffer of at most [`Limit::bytes`] and handed to a
/// sink whenever the next piece would not fit, so the text is never held
/// whole.
///
/// A piece longer than the buffer is handed on as it is, from where it
/// already lies, without being copied; a string longer than
/// [`Limit::piece`] is escaped in pieces of at most that many bytes, each of
/// whose escapes fits the buffer, so no string is held whole either. Every
/// byte handed on is counted by [`super::work`], as [`write_wire`] counts the
/// same text.
struct Chunks<'s, S: ?Sized> {
    sink: &'s mut S,
    buffer: String,
    limit: Limit,
}

impl<'s, S> Chunks<'s, S>
where
    S: for<'t> Extend<&'t str> + ?Sized,
{
    fn new(sink: &'s mut S, limit: Limit) -> Chunks<'s, S> {
        Chunks {
            sink,
            buffer: String::with_capacity(limit.bytes),
            limit,
        }
    }

    /// Hand `text` to the sink and count it.
    fn feed(sink: &mut S, text: &str) {
        if !text.is_empty() {
            super::work::add(text.len());
            sink.extend(core::iter::once(text));
        }
    }

    /// Hand on what the buffer holds, and empty it.
    fn flush(&mut self) {
        Self::feed(self.sink, &self.buffer);
        self.buffer.clear();
    }

    /// Whether `extra` more bytes fit the buffer without growing it past its
    /// limit.
    fn fits(&self, extra: usize) -> bool {
        self.limit
            .bytes
            .checked_sub(self.buffer.len())
            .is_some_and(|room| extra <= room)
    }

    /// `text` cut into the next piece of a long string and the rest: the
    /// piece at most [`Limit::piece`] bytes, ended at a character boundary,
    /// and at least the first character, so a piece is never empty while text
    /// remains. `None` for an empty `text`, and otherwise only if `str`'s
    /// own boundary search answered a place that is not one.
    fn cut<'t>(&self, text: &'t str) -> Option<(&'t str, &'t str)> {
        let at = match text.floor_char_boundary(self.limit.piece) {
            0 => text.ceil_char_boundary(1),
            at => at,
        };
        text.split_at_checked(at)
            .filter(|(piece, _)| !piece.is_empty())
    }
}

impl<S> core::fmt::Write for Chunks<'_, S>
where
    S: for<'t> Extend<&'t str> + ?Sized,
{
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        Out::push_str(self, text);
        Ok(())
    }
}

impl<S> Out for Chunks<'_, S>
where
    S: for<'t> Extend<&'t str> + ?Sized,
{
    fn push_str(&mut self, text: &str) {
        if !self.fits(text.len()) {
            self.flush();
        }
        if self.fits(text.len()) {
            self.buffer.push_str(text);
        } else {
            Self::feed(self.sink, text);
        }
    }

    fn push(&mut self, character: char) {
        let mut bytes = [0_u8; 4];
        Out::push_str(self, character.encode_utf8(&mut bytes));
    }

    /// A string of at most [`Limit::piece`] bytes is escaped straight into
    /// the buffer, which is emptied first if its escapes might not fit. A
    /// longer one is written as its two quotes around the escapes of its
    /// pieces, each piece escaped by [`write_string`] alone into the emptied
    /// buffer and handed on without the two quotes that call puts around it.
    /// Escaping is one character at a time, so the escapes of the pieces
    /// joined are the escapes of the string, and the text is the one
    /// [`write_string`] writes for the string whole.
    fn json_string(&mut self, text: &str, path: &Path<'_>) -> Result<(), Error> {
        if text.len() <= self.limit.piece {
            // At most `piece` bytes, so at most `bytes` escaped and quoted:
            // `Limit::new`'s arithmetic, which cannot overflow here.
            let longest = text
                .len()
                .checked_mul(LONGEST_ESCAPE)
                .and_then(|escaped| escaped.checked_add(2));
            if !longest.is_some_and(|longest| self.fits(longest)) {
                self.flush();
            }
            write_string(text, &mut self.buffer);
            return Ok(());
        }
        let lost = || {
            Error::at(
                ErrorKind::Value,
                &path.render(),
                "a long string's escapes came back without the two quotes around them",
            )
        };
        Out::push(self, '"');
        self.flush();
        let mut rest = text;
        while !rest.is_empty() {
            let (piece, after) = self.cut(rest).ok_or_else(|| {
                Error::at(
                    ErrorKind::Value,
                    &path.render(),
                    "a long string could not be cut at a character boundary",
                )
            })?;
            write_string(piece, &mut self.buffer);
            let escaped = self
                .buffer
                .strip_prefix('"')
                .and_then(|inner| inner.strip_suffix('"'))
                .ok_or_else(lost)?;
            Self::feed(self.sink, escaped);
            self.buffer.clear();
            rest = after;
        }
        Out::push(self, '"');
        Ok(())
    }
}

/// Whether `omit` is a path [`write_wire_into`] can leave out of the message
/// at `at`: each name a singular field of the message before it, by its
/// `.proto` name (the name [`Decoder::write_message`] matches), and each name
/// but the last a field of a message type. A misspelt path would otherwise
/// leave nothing out without a word.
fn check_omit(index: &Index, at: usize, full_name: &str, omit: &[&str]) -> Result<(), Error> {
    let refused = || {
        Error::at(
            ErrorKind::Schema,
            "",
            crate::said!(
                "`{}` is not a path of singular fields of `{}` by their `.proto` names, so there \
                 is no field to leave out",
                quoted(&omit.join(".")),
                quoted(full_name)
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
/// [`super::grammar::MAX_DEPTH`]) and its indentation in the text.
#[derive(Clone, Copy)]
struct Place<'p> {
    path: &'p Path<'p>,
    depth: usize,
    indent: usize,
}

impl<'b> Decoder<'b> {
    fn indexed(&self, at: usize, path: &Path<'_>) -> Result<&'static Indexed, Error> {
        self.index.get(at).ok_or_else(|| {
            Error::at(
                ErrorKind::Schema,
                &path.render(),
                "a message index outside the schema",
            )
        })
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
                    ErrorKind::Wire,
                    &path.render(),
                    crate::said!(
                        "`{}` is not a valid encoding: {}",
                        message.message.full_name.as_str(),
                        error.name()
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
            .ok_or_else(|| Error::at(ErrorKind::Wire, &path.render(), "a field with no value"))
    }

    fn message_to_json(
        &mut self,
        at: usize,
        bytes: &'b [u8],
        path: &Path<'_>,
        depth: usize,
    ) -> Result<Json, Error> {
        if !may_nest(depth) {
            return Err(Error::at(
                ErrorKind::Depth,
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
    fn write_message<O: Out>(
        &mut self,
        at: usize,
        bytes: &'b [u8],
        place: Place<'_>,
        omit: &[&str],
        out: &mut O,
    ) -> Result<(), Error> {
        let path = place.path;
        if !may_nest(place.depth) {
            return Err(Error::at(
                ErrorKind::Depth,
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

    fn write_scalar<O: Out>(
        &mut self,
        field: &IndexedField,
        value: Value<'b>,
        place: Place<'_>,
        omit: &[&str],
        out: &mut O,
    ) -> Result<(), Error> {
        match scalar_of(field, value, place.path)? {
            Scalar::Bool(true) => out.push_str("true"),
            Scalar::Bool(false) => out.push_str("false"),
            // Neither sink can fail: a `String` and `Chunks` both answer
            // every write with `Ok`. A quoted number and base64
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
            Scalar::Text(text) | Scalar::Name(text) => out.json_string(text, place.path)?,
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
fn push_indent<O: Out>(depth: usize, out: &mut O) {
    let width = depth.saturating_mul(2);
    match SPACES.get(..width) {
        Some(spaces) => out.push_str(spaces),
        None => (0..width).for_each(|_| out.push(' ')),
    }
}

fn declared<'m>(
    message: &'m Indexed,
    number: i32,
    path: &Path<'_>,
) -> Result<&'m IndexedField, Error> {
    message.field_by_number(number).ok_or_else(|| {
        Error::at(
            ErrorKind::Wire,
            &path.render(),
            crate::said!(
                "field number {} is not declared on `{}`",
                number,
                message.message.full_name.as_str()
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
        let fail = |error: crate::wire::WireError| {
            Error::at(
                ErrorKind::Wire,
                &path.render(),
                crate::said!("a packed repeated field: {}", error.name()),
            )
        };
        let element = match kind {
            ScalarKind::Fixed32 | ScalarKind::Sfixed32 | ScalarKind::Float => {
                let array: [u8; 4] = reader.raw(4).map_err(fail)?.try_into().map_err(|_| {
                    Error::at(
                        ErrorKind::Wire,
                        &path.render(),
                        "a truncated packed fixed32",
                    )
                })?;
                Value::Fixed32(u32::from_le_bytes(array))
            }
            ScalarKind::Fixed64 | ScalarKind::Sfixed64 | ScalarKind::Double => {
                let array: [u8; 8] = reader.raw(8).map_err(fail)?.try_into().map_err(|_| {
                    Error::at(
                        ErrorKind::Wire,
                        &path.render(),
                        "a truncated packed fixed64",
                    )
                })?;
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
    let at = |kind: ErrorKind, message: Said| Error::at(kind, &path.render(), message);
    match field.field.kind {
        ScalarKind::Double | ScalarKind::Float => Err(at(
            ErrorKind::Value,
            "a floating-point field: the sim is integer-only and this codec refuses one".into(),
        )),
        ScalarKind::Bool => Ok(Scalar::Bool(varint(value, path)? != 0)),
        ScalarKind::Int32 => {
            let wide = u64_bits_to_i64(varint(value, path)?);
            let narrow = i32::try_from(wide).map_err(|_| {
                at(
                    ErrorKind::Wire,
                    "an int32 field carrying a wider value".into(),
                )
            })?;
            Ok(Scalar::Signed(i64::from(narrow)))
        }
        ScalarKind::Sint32 => {
            let wide = zigzag_decode(varint(value, path)?);
            let narrow = i32::try_from(wide).map_err(|_| {
                at(
                    ErrorKind::Wire,
                    "an sint32 field carrying a wider value".into(),
                )
            })?;
            Ok(Scalar::Signed(i64::from(narrow)))
        }
        ScalarKind::Uint32 => {
            let narrow = u32::try_from(varint(value, path)?).map_err(|_| {
                at(
                    ErrorKind::Wire,
                    "a uint32 field carrying a wider value".into(),
                )
            })?;
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
            let text = std::str::from_utf8(payload).map_err(|_| {
                at(
                    ErrorKind::Wire,
                    "a string field that is not valid UTF-8".into(),
                )
            })?;
            Ok(Scalar::Text(text))
        }
        ScalarKind::Bytes => Ok(Scalar::Bytes(delimited(value, path)?)),
        ScalarKind::Enum => {
            let number = i32::try_from(u64_bits_to_i64(varint(value, path)?))
                .map_err(|_| at(ErrorKind::Wire, "an enum value out of range".into()))?;
            let declared = field.enumeration.ok_or_else(|| {
                at(
                    ErrorKind::Schema,
                    crate::said!("`{}` is not in the schema", field.field.type_name.as_str()),
                )
            })?;
            Ok(declared.value_by_number(number).map_or_else(
                // An unrecognised number is written as a number, which is what
                // the proto3 JSON mapping says and what lets a diagnostic name
                // the offending value instead of losing it.
                || Scalar::Signed(i64::from(number)),
                |found| Scalar::Name(found.name.as_str()),
            ))
        }
        ScalarKind::Message => {
            let nested = field.nested.ok_or_else(|| {
                at(
                    ErrorKind::Schema,
                    crate::said!("`{}` is not in the schema", field.field.type_name.as_str()),
                )
            })?;
            Ok(Scalar::Message(nested, delimited(value, path)?))
        }
    }
}

fn varint(value: Value<'_>, path: &Path<'_>) -> Result<u64, Error> {
    match value {
        Value::Varint(raw) => Ok(raw),
        _ => Err(Error::at(
            ErrorKind::Wire,
            &path.render(),
            "expected a varint on the wire",
        )),
    }
}

fn fixed32(value: Value<'_>, path: &Path<'_>) -> Result<u32, Error> {
    match value {
        Value::Fixed32(raw) => Ok(raw),
        _ => Err(Error::at(
            ErrorKind::Wire,
            &path.render(),
            "expected four fixed bytes on the wire",
        )),
    }
}

fn fixed64(value: Value<'_>, path: &Path<'_>) -> Result<u64, Error> {
    match value {
        Value::Fixed64(raw) => Ok(raw),
        _ => Err(Error::at(
            ErrorKind::Wire,
            &path.render(),
            "expected eight fixed bytes on the wire",
        )),
    }
}

fn delimited<'b>(value: Value<'b>, path: &Path<'_>) -> Result<&'b [u8], Error> {
    match value {
        Value::Delimited(payload) => Ok(payload),
        _ => Err(Error::at(
            ErrorKind::Wire,
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
    /// The fields each object on the way down has named, by number, with the
    /// member name that named it, as a stack: a field is named once, under
    /// either of its names.
    named: Vec<(i32, &'j str)>,
}

impl<'j> Encoder<'j> {
    /// Records that this object names `field`, by `key`; or refuses a field
    /// written under both its names, its `.proto` name and its JSON name, as
    /// a key written twice is (S3's `vfy6`), whatever either member holds,
    /// `null` included.
    fn name_once(
        &mut self,
        named_base: usize,
        field: &IndexedField,
        key: &'j str,
        path: &Path<'_>,
    ) -> Result<(), Error> {
        let first = self.named.get(named_base..).and_then(|named| {
            named
                .iter()
                .find(|(number, _)| *number == field.field.number)
        });
        if let Some((_, first)) = first {
            return Err(Error::at(
                ErrorKind::BothNames,
                &Path::Key(path, key).render(),
                crate::said!(
                    "`{}` and `{}` both name the field `{}`. A field is written once, \
                     under one of its names.",
                    quoted(first),
                    quoted(key),
                    field.field.name.as_str()
                ),
            ));
        }
        self.named.push((field.field.number, key));
        Ok(())
    }

    fn json_to_message(
        &mut self,
        at: usize,
        value: &'j Json,
        path: &Path<'_>,
        depth: usize,
    ) -> Result<Vec<u8>, Error> {
        if !may_nest(depth) {
            return Err(Error::at(
                ErrorKind::Depth,
                &path.render(),
                "a message nested deeper than 64 levels",
            ));
        }
        let message: &'static Indexed = self.index.get(at).ok_or_else(|| {
            Error::at(
                ErrorKind::Schema,
                &path.render(),
                "a message index outside the schema",
            )
        })?;
        let Json::Object(entries) = value else {
            return Err(Error::at(
                ErrorKind::Value,
                &path.render(),
                crate::said!(
                    "expected an object for `{}`, found {}",
                    message.message.full_name.as_str(),
                    value.kind()
                ),
            ));
        };

        // Resolve every key first, so an unknown field is reported before
        // anything is written — and so the write order can be by field number
        // rather than by the order somebody happened to type them in.
        let base = self.fields.len();
        let oneof_base = self.oneofs.len();
        let named_base = self.named.len();
        for (key, item) in entries {
            let field = message.field_by_name(key).ok_or_else(|| {
                Error::at(
                    ErrorKind::UnknownField,
                    &Path::Key(path, key).render(),
                    crate::said!(
                        "`{}` is not a field of `{}`. Unknown fields are rejected, never \
                         stripped.",
                        quoted(key),
                        message.message.full_name.as_str()
                    ),
                )
            })?;
            self.name_once(named_base, field, key, path)?;
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
                        ErrorKind::OneofArms,
                        &Path::Key(path, key).render(),
                        crate::said!(
                            "`{}` and `{}` are both set, but only one member of a \
                             oneof may be",
                            quoted(key),
                            quoted(previous)
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
        self.named.truncate(named_base);
        let end = self.fields.len();
        if let Some(resolved) = self.fields.get_mut(base..end) {
            // No field is resolved twice, so the order is total.
            if !resolved.is_sorted_by_key(|(field, _)| field.field.number) {
                resolved.sort_unstable_by_key(|(field, _)| field.field.number);
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
                        ErrorKind::Value,
                        &field_path.render(),
                        crate::said!("expected an array, found {}", item.kind()),
                    ));
                };
                for (index, element) in items.iter().enumerate() {
                    let element_path = Path::Index(&field_path, index);
                    self.write_field(field, element, &element_path, depth, &mut writer)?;
                }
            } else {
                self.write_field(field, item, &field_path, depth, &mut writer)?;
            }
        }
        self.fields.truncate(base);
        Ok(writer.into_vec())
    }

    /// One field's value: a message through [`Encoder::write_nested`], which
    /// is the recursion, and anything else through [`Encoder::write_scalar`],
    /// which is not. The recursion is one frame of this, of `write_nested` and
    /// of [`Encoder::json_to_message`] per message level, so the scalar arms'
    /// temporaries live in a frame of their own that is gone before the next
    /// level starts (S3's `vfyq2`, fix pass 2, item E: a debug build spent
    /// about 10 KiB a level here).
    fn write_field(
        &mut self,
        field: &'static IndexedField,
        value: &'j Json,
        path: &Path<'_>,
        depth: usize,
        writer: &mut Writer,
    ) -> Result<(), Error> {
        if matches!(field.field.kind, ScalarKind::Message) {
            self.write_nested(field, value, path, depth, writer)
        } else {
            self.write_scalar(field, value, path, depth, writer)
        }
    }

    /// A message-typed field's value: encoded, then written length-delimited.
    fn write_nested(
        &mut self,
        field: &'static IndexedField,
        value: &'j Json,
        path: &Path<'_>,
        depth: usize,
        writer: &mut Writer,
    ) -> Result<(), Error> {
        let Some(nested) = field.nested else {
            return Err(Error::at(
                ErrorKind::Schema,
                &path.render(),
                crate::said!("`{}` is not in the schema", field.field.type_name.as_str()),
            ));
        };
        let payload = self.json_to_message(nested, value, path, depth.saturating_add(1))?;
        writer
            .delimited_field(field.field.number, &payload)
            .map_err(|_| {
                Error::at(
                    ErrorKind::Value,
                    &path.render(),
                    "a message too long for its length prefix",
                )
            })
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one arm per Protobuf scalar kind; splitting it would hide the \
                  one-to-one correspondence with the descriptor's type enum"
    )]
    #[inline(never)]
    fn write_scalar(
        &mut self,
        field: &'static IndexedField,
        value: &'j Json,
        path: &Path<'_>,
        depth: usize,
        writer: &mut Writer,
    ) -> Result<(), Error> {
        let at = |kind: ErrorKind, message: Said| Error::at(kind, &path.render(), message);
        let number = field.field.number;
        match field.field.kind {
            ScalarKind::Double | ScalarKind::Float => {
                return Err(at(
                    ErrorKind::Value,
                    "a floating-point field: the sim is integer-only and this codec refuses one"
                        .into(),
                ));
            }
            ScalarKind::Bool => {
                let Json::Bool(flag) = value else {
                    return Err(at(
                        ErrorKind::Value,
                        crate::said!("expected true or false, found {}", value.kind()),
                    ));
                };
                writer.varint_field(number, u64::from(*flag));
            }
            ScalarKind::Int32 => {
                let narrow = i32::try_from(integer(value, path)?).map_err(|_| {
                    at(ErrorKind::Integer, "out of range for an int32 field".into())
                })?;
                writer.varint_field(number, i64_bits_to_u64(i64::from(narrow)));
            }
            ScalarKind::Sint32 => {
                let narrow = i32::try_from(integer(value, path)?).map_err(|_| {
                    at(
                        ErrorKind::Integer,
                        "out of range for an sint32 field".into(),
                    )
                })?;
                writer.varint_field(number, zigzag_encode(i64::from(narrow)));
            }
            ScalarKind::Uint32 => {
                let narrow = u32::try_from(unsigned(value, path)?).map_err(|_| {
                    at(ErrorKind::Integer, "out of range for a uint32 field".into())
                })?;
                writer.varint_field(number, u64::from(narrow));
            }
            ScalarKind::Fixed32 => {
                let narrow = u32::try_from(unsigned(value, path)?).map_err(|_| {
                    at(
                        ErrorKind::Integer,
                        "out of range for a fixed32 field".into(),
                    )
                })?;
                writer.fixed32_field(number, narrow);
            }
            ScalarKind::Sfixed32 => {
                let narrow = i32::try_from(integer(value, path)?).map_err(|_| {
                    at(
                        ErrorKind::Integer,
                        "out of range for an sfixed32 field".into(),
                    )
                })?;
                writer.fixed32_field(number, u32::from_ne_bytes(narrow.to_ne_bytes()));
            }
            ScalarKind::Int64 => {
                let wide = integer(value, path)?;
                writer.varint_field(number, i64_bits_to_u64(wide));
            }
            ScalarKind::Sint64 => {
                let wide = integer(value, path)?;
                writer.varint_field(number, zigzag_encode(wide));
            }
            ScalarKind::Uint64 => {
                let wide = unsigned(value, path)?;
                writer.varint_field(number, wide);
            }
            ScalarKind::Fixed64 => {
                let wide = unsigned(value, path)?;
                writer.fixed64_field(number, wide);
            }
            ScalarKind::Sfixed64 => {
                let wide = integer(value, path)?;
                writer.fixed64_field(number, i64_bits_to_u64(wide));
            }
            ScalarKind::String => {
                let Json::String(text) = value else {
                    return Err(at(
                        ErrorKind::Value,
                        crate::said!("expected a string, found {}", value.kind()),
                    ));
                };
                writer
                    .delimited_field(number, text.as_bytes())
                    .map_err(|_| {
                        at(
                            ErrorKind::Value,
                            "a string too long for its length prefix".into(),
                        )
                    })?;
            }
            ScalarKind::Bytes => {
                let Json::String(text) = value else {
                    return Err(at(
                        ErrorKind::Value,
                        crate::said!("expected base64 in a string, found {}", value.kind()),
                    ));
                };
                let bytes = base64::decode(text)
                    .ok_or_else(|| at(ErrorKind::Base64, "not valid base64".into()))?;
                writer.delimited_field(number, &bytes).map_err(|_| {
                    at(
                        ErrorKind::Value,
                        "bytes too long for their length prefix".into(),
                    )
                })?;
            }
            ScalarKind::Enum => {
                let declared = field.enumeration.ok_or_else(|| {
                    at(
                        ErrorKind::Schema,
                        crate::said!("`{}` is not in the schema", field.field.type_name.as_str()),
                    )
                })?;
                let found = match value {
                    Json::String(name) => {
                        declared
                            .value_by_name(name)
                            .ok_or_else(|| {
                                at(
                                    ErrorKind::Value,
                                    crate::said!(
                                        "`{}` is not a value of `{}`",
                                        quoted(name),
                                        field.field.type_name.as_str()
                                    ),
                                )
                            })?
                            .number
                    }
                    Json::Number(_) => i32::try_from(integer(value, path)?)
                        .map_err(|_| at(ErrorKind::Integer, "an enum value out of range".into()))?,
                    other => {
                        return Err(at(
                            ErrorKind::Value,
                            crate::said!("expected an enum value name, found {}", other.kind()),
                        ));
                    }
                };
                writer.varint_field(number, i64_bits_to_u64(i64::from(found)));
            }
            // `write_field` sends a message to `write_nested`; one that comes
            // here is written the same way.
            ScalarKind::Message => {
                self.write_nested(field, value, path, depth, writer)?;
            }
        }
        Ok(())
    }
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

/// True when a JSON number lexeme is a whole zero: `0` or `-0`.
///
/// A lexeme off RFC 8259's integer grammar is never a zero here, however it
/// parses (`+0`, `00`), so it reaches `write_scalar` and is refused there with
/// its own message rather than dropped as a default (S3's `vfy6`); so is a
/// lexeme this schema cannot hold (`0.0`, `1e3`).
fn is_zero(lexeme: &str) -> bool {
    is_integer_lexeme(lexeme) && (lexeme.parse::<i64>() == Ok(0) || lexeme.parse::<u64>() == Ok(0))
}

/// Why `lexeme` is not the integer a field asked for: digits RFC 8259 does
/// not write as an integer, or `otherwise`.
fn integer_refusal(lexeme: &str, otherwise: &'static str, signed: bool) -> Said {
    if is_misspelt_integer(lexeme) {
        crate::said!(
            "`{}` is not an integer as JSON writes one: RFC 8259 has no `+` and no \
             leading zero",
            quoted(lexeme)
        )
    } else if is_integer_lexeme(lexeme) && (signed || !lexeme.starts_with('-')) {
        // A whole number too large for the field's 64 bits (the review of
        // PR #179): out of range, not a fraction.
        crate::said!("`{}` is out of range for a 64-bit integer", quoted(lexeme))
    } else {
        crate::said!("`{}` {}", quoted(lexeme), otherwise)
    }
}

/// A JSON integer, from a bare number or from the quoted form the mapping also
/// accepts. A fractional or exponential lexeme is refused: this schema has no
/// field that could hold one.
fn integer(value: &Json, path: &Path<'_>) -> Result<i64, Error> {
    let lexeme = match value {
        Json::Number(text) | Json::String(text) => text,
        other => {
            return Err(Error::at(
                ErrorKind::Integer,
                &path.render(),
                crate::said!("expected an integer, found {}", other.kind()),
            ));
        }
    };
    lexeme
        .parse::<i64>()
        .ok()
        .filter(|_| is_integer_lexeme(lexeme))
        .ok_or_else(|| {
            Error::at(
                ErrorKind::Integer,
                &path.render(),
                integer_refusal(
                    lexeme,
                    "is not a whole number. This schema has no fractional field.",
                    true,
                ),
            )
        })
}

fn unsigned(value: &Json, path: &Path<'_>) -> Result<u64, Error> {
    let lexeme = match value {
        Json::Number(text) | Json::String(text) => text,
        other => {
            return Err(Error::at(
                ErrorKind::Integer,
                &path.render(),
                crate::said!("expected a non-negative integer, found {}", other.kind()),
            ));
        }
    };
    // `-0` is RFC 8259's other spelling of a whole zero, which `is_zero`
    // drops as a plain field's default; with presence it is the written 0
    // (the review of PR #230), never a negative number.
    let digits = if lexeme.as_str() == "-0" {
        "0"
    } else {
        lexeme.as_str()
    };
    digits
        .parse::<u64>()
        .ok()
        .filter(|_| is_integer_lexeme(lexeme))
        .ok_or_else(|| {
            Error::at(
                ErrorKind::Integer,
                &path.render(),
                integer_refusal(lexeme, "is not a non-negative whole number", false),
            )
        })
}

#[cfg(test)]
mod tests {
    //! [`Chunks`] against the `String` the codec writes in place (S3's
    //! `fpstream`): the pieces joined are the text, whatever the limit, and
    //! the gathering buffer never grows.

    use super::{CHUNK, Chunks, Limit, Out, write_into, write_wire, write_wire_into};
    use crate::fingerprint::FINGERPRINT_FIELD;
    use crate::json::{read, work};

    /// The pieces a sink was handed, kept apart so a test can see the cuts.
    #[derive(Default)]
    struct Pieces(Vec<String>);

    impl<'t> Extend<&'t str> for Pieces {
        fn extend<I: IntoIterator<Item = &'t str>>(&mut self, pieces: I) {
            self.0.extend(pieces.into_iter().map(str::to_owned));
        }
    }

    /// splitmix64: seeded, the same on every platform.
    fn mix(seed: u64) -> u64 {
        let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A string of `length` characters drawn from ones that escape at every
    /// width (a quote, a backslash, a newline, two controls) and ones that are
    /// one, two, three and four bytes of UTF-8 and escape not at all.
    fn string(seed: u64, length: usize) -> String {
        const ALPHABET: [char; 10] = [
            'a',
            '"',
            '\\',
            '\n',
            '\u{1}',
            '\u{1f}',
            '\u{e9}',
            '\u{20ac}',
            '\u{1f600}',
            ' ',
        ];
        let mut state = seed;
        (0..length)
            .map(|_| {
                state = mix(state);
                let at = usize::try_from(state % 10).expect("below ten");
                ALPHABET.get(at).copied().expect("below ten")
            })
            .collect()
    }

    /// A JSON string literal for `text`, written with the codec's own
    /// escaper, for a document to carry.
    fn literal(text: &str) -> String {
        let mut out = String::new();
        super::write_string(text, &mut out);
        out
    }

    /// Playbook documents: strings at lengths either side of every piece
    /// size the limits below give, a long string, a nested condition, and a
    /// stamped fingerprint for the omitting path.
    fn documents() -> Vec<String> {
        let mut out = Vec::new();
        let lengths = [
            0, 1, 2, 3, 5, 9, 10, 11, 12, 13, 40, 200, 1_364, 1_365, 1_366, 9_000,
        ];
        for (seed, length) in (0_u64..).zip(lengths) {
            out.push(format!(
                r#"{{"meta":{{"title":{},"note":{},"fingerprint":"AAAAAAAAAAE="}},"kind":"PLAYBOOK"}}"#,
                literal(&string(seed, length)),
                literal(&string(seed.wrapping_add(100), length.wrapping_mul(3) % 50)),
            ));
        }
        // Wide characters alone, past one piece and past the buffer, shifted
        // by 0 to 3 ASCII bytes so one straddles each place a cut could fall
        // (fix pass 1).
        for wide in ['\u{e9}', '\u{20ac}', '\u{1f600}'] {
            for prefix in 0..4_usize {
                for bytes in [1_366_usize, 8_193] {
                    let count = bytes.div_ceil(wide.len_utf8());
                    let title: String = "a"
                        .repeat(prefix)
                        .chars()
                        .chain(core::iter::repeat_n(wide, count))
                        .collect();
                    out.push(format!(r#"{{"meta":{{"title":{}}}}}"#, literal(&title)));
                }
            }
        }
        let leaf = |value: usize| {
            format!(r#"{{"segment_elapsed":{{"ms":{{"op":"EQ","value":{value}}}}}}}"#)
        };
        let leaves: Vec<String> = (0..21).map(leaf).collect();
        let mut condition = leaves.join(",");
        for _ in 0..12 {
            condition = format!(r#"{{"any":{{"items":[{condition}]}}}}"#);
        }
        out.push(format!(
            r#"{{"schema_version":{{"major":1}},"meta":{{"title":"p1","author_kind":"HUMAN"}},"kind":"PLAYBOOK","declarative":{{"route":[{{"label":"r0","skip_if":{condition},"hold":{{"ms":1000}}}}]}}}}"#
        ));
        out
    }

    fn wire(text: &str) -> Vec<u8> {
        let value = read(text).expect("a JSON document");
        super::json_to_encoded("gp.v1.Playbook", &value)
            .expect("a playbook")
            .bytes()
            .to_vec()
    }

    /// The text a limit writes, the pieces it was handed on in, and whether
    /// the gathering buffer kept the capacity it was made with.
    fn chunked(bytes: &[u8], omit: &[&str], limit: Limit) -> (String, Vec<String>, bool) {
        let mut pieces = Pieces::default();
        let mut chunks = Chunks::new(&mut pieces, limit);
        let capacity = chunks.buffer.capacity();
        write_into("gp.v1.Playbook", bytes, omit, &mut chunks).expect("the bytes write");
        chunks.flush();
        let kept = chunks.buffer.capacity() == capacity;
        (pieces.0.concat(), pieces.0, kept)
    }

    #[test]
    fn a_limit_too_small_for_one_escaped_character_is_refused() {
        for bytes in 0..8 {
            assert!(Limit::new(bytes).is_none(), "{bytes}");
        }
        let smallest = Limit::new(8).expect("eight bytes hold a quoted six-byte escape");
        assert_eq!(smallest.piece, 1);
        assert_eq!(CHUNK.bytes, 8 << 10);
        assert_eq!(CHUNK.piece, 1_365);
    }

    #[test]
    fn the_pieces_joined_are_the_text_at_every_limit() {
        for document in documents() {
            let bytes = wire(&document);
            let whole = write_wire("gp.v1.Playbook", &bytes).expect("the bytes write");
            let mut cleared = String::new();
            write_into("gp.v1.Playbook", &bytes, FINGERPRINT_FIELD, &mut cleared)
                .expect("the bytes write");
            assert!(!cleared.contains("fingerprint"), "{cleared}");
            for bytes_limit in (8..=80).chain([4_096, CHUNK.bytes]) {
                let limit = Limit::new(bytes_limit).expect("a limit of eight or more");
                for (omit, expected) in [(&[][..], &whole), (FINGERPRINT_FIELD, &cleared)] {
                    let (text, pieces, kept) = chunked(&bytes, omit, limit);
                    assert_eq!(&text, expected, "limit {bytes_limit}");
                    assert!(kept, "the buffer grew at limit {bytes_limit}");
                    assert!(
                        pieces.iter().all(|piece| !piece.is_empty()),
                        "an empty piece at limit {bytes_limit}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_long_string_is_never_held_whole() {
        // A string of 9,000 characters, most of them escaped or wide, is
        // handed on in pieces no longer than the buffer.
        let title = string(7, 9_000);
        let document = format!(r#"{{"meta":{{"title":{}}}}}"#, literal(&title));
        let bytes = wire(&document);
        let limit = Limit::new(64).expect("a limit");
        let (text, pieces, kept) = chunked(&bytes, &[], limit);
        assert_eq!(Ok(text), write_wire("gp.v1.Playbook", &bytes));
        assert!(kept);
        let longest = pieces.iter().map(String::len).max().expect("pieces");
        assert!(longest <= 64, "a piece of {longest} bytes");
    }

    #[test]
    fn the_work_counted_is_the_text_both_ways() {
        for document in documents() {
            let bytes = wire(&document);
            let _ = work::take();
            let whole = write_wire("gp.v1.Playbook", &bytes).expect("the bytes write");
            let counted_whole = work::take();
            let mut joined = String::new();
            write_wire_into("gp.v1.Playbook", &bytes, &[], &mut joined).expect("the bytes write");
            let counted_joined = work::take();
            assert_eq!(joined, whole);
            assert_eq!(counted_whole, counted_joined);
            assert_eq!(
                counted_whole,
                u128::try_from(whole.len()).expect("a length")
            );
        }
    }

    #[test]
    fn a_string_sink_is_appended_to_in_place() {
        let mut text = String::from("a");
        Out::push_str(&mut text, "bc");
        Out::push(&mut text, 'd');
        write_wire_into("gp.v1.Playbook", &[], &[], &mut text).expect("an empty playbook");
        assert_eq!(text, "abcd{}\n");
    }
}
