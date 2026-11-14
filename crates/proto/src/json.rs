// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Canonical proto3 JSON for `gp.v1` and `gp.api.v1` — the disk format the
//! whole project rests on.
//!
//! A playbook on disk is a **JSONC** file: this canonical JSON plus comments,
//! which the editor round-trips byte for byte. The comment-and-formatting
//! layer is `plan-core`'s, not this crate's (decisions-log item 74); what is
//! here is the part that has to agree with the schema, and it is driven by the
//! checked-in descriptor set so it cannot drift from the `.proto` files.
//!
//! ```no_run
//! use pharmakos_proto::gp::v1::Playbook;
//! use pharmakos_proto::json;
//!
//! let text = std::fs::read_to_string("examples/playbooks/expand_east.jsonc")?;
//! // (a real caller strips the comments with plan-core first)
//! let playbook: Playbook = json::decode(&text)?;
//! let canonical: String = json::encode(&playbook)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod base64;
mod codec;
#[cfg(test)]
mod differential;
mod direct;
mod index;
mod path;
#[cfg(test)]
mod reference;
mod value;

pub use codec::{
    Encoded, json_to_encoded, json_to_wire, wire_to_json, write_wire, write_wire_into,
};
pub use value::{Error, Json, read, write};

use prost::{Message, Name};

/// A message as canonical JSON text.
///
/// # Errors
///
/// If the message's type is not in the checked-in descriptor set, or if the
/// schema declares something the canonical form cannot express — a
/// floating-point field, for instance, which this codec refuses on principle.
pub fn encode<M: Message + Name>(message: &M) -> Result<String, Error> {
    // Straight from the wire to the text: what `write(&encode_json(message)?)`
    // answers, without the tree in between (S3's `vfyq`).
    write_wire(&M::full_name(), &message.encode_to_vec())
}

/// A message as a canonical JSON value, without rendering it to text.
///
/// # Errors
///
/// As [`encode`].
pub fn encode_json<M: Message + Name>(message: &M) -> Result<Json, Error> {
    wire_to_json(&M::full_name(), &message.encode_to_vec())
}

/// A message read from canonical JSON text.
///
/// Strict, deliberately: an unknown field is rejected with a JSON Pointer to
/// where it sits, never ignored and never stripped (spec section 10, "Load
/// never strips"). Comments are not accepted here — they belong to the JSONC
/// layer above.
///
/// # Errors
///
/// If the text is not JSON, if it carries a field the message does not
/// declare, if a value does not fit its field, or if two members of one
/// `oneof` are set at once.
pub fn decode<M: Message + Name + Default>(text: &str) -> Result<M, Error> {
    decode_encoded(text).map(Decoded::into_message)
}

/// [`decode`], keeping the wire bytes the message was decoded from.
///
/// The answer is exactly [`decode_json_encoded`]'s over [`read`]'s: the
/// direct reader ([`decode_direct`]) answers when it can, and the exact path
/// answers when it cannot, which is when there is an error to report.
///
/// # Errors
///
/// As [`decode`].
pub fn decode_encoded<M: Message + Name + Default>(text: &str) -> Result<Decoded<M>, Error> {
    match decode_direct(text) {
        Some(decoded) => Ok(decoded),
        None => decode_json_encoded(&read(text)?),
    }
}

/// A message read from JSON text in one pass, without a [`Json`] tree, or
/// `None` when the text is not one [`read`] then [`decode_json_encoded`]
/// accept.
///
/// When it answers, the answer is theirs: the same message, the same wire
/// bytes, the same [`Encoded::is_plain`]. When it does not, they are the
/// answer, and it is an error with a pointer and a message; this function
/// never gives a reason of its own, so the codec has one set of errors (S3's
/// `vfyq`, decisions-log item 158). A caller that wants the error asks them,
/// as [`decode_encoded`] does; a caller that also wants the tree for its own
/// diagnostics, such as the verifier's decode stage, reads it then.
#[must_use]
pub fn decode_direct<M: Message + Name + Default>(text: &str) -> Option<Decoded<M>> {
    let encoded = direct::text_to_encoded(&M::full_name(), text)?;
    // A prost refusal is the exact path's to report, with its own sentence.
    let message = M::decode(encoded.bytes()).ok()?;
    Some(Decoded { message, encoded })
}

/// A message read from an already-parsed canonical JSON value.
///
/// # Errors
///
/// As [`decode`].
pub fn decode_json<M: Message + Name + Default>(value: &Json) -> Result<M, Error> {
    decode_json_encoded(value).map(Decoded::into_message)
}

/// A message decoded from canonical JSON, with the wire bytes it was decoded
/// from.
///
/// The two travel together, and only [`decode_json_encoded`] makes one, so the
/// bytes are always the ones this message was decoded from: a caller that
/// wants canonical text for the message can write it from them
/// ([`write_wire`]) without encoding the message again, when
/// [`Encoded::is_plain`] says the text would be the same.
#[derive(Clone, PartialEq, Debug)]
pub struct Decoded<M> {
    message: M,
    encoded: Encoded,
}

impl<M> Decoded<M> {
    /// The message.
    pub const fn message(&self) -> &M {
        &self.message
    }

    /// The message, without the bytes.
    pub fn into_message(self) -> M {
        self.message
    }

    /// The wire bytes the message was decoded from.
    pub const fn encoded(&self) -> &Encoded {
        &self.encoded
    }
}

/// [`decode_json`], keeping the wire bytes the message was decoded from.
///
/// # Errors
///
/// As [`decode`].
pub fn decode_json_encoded<M: Message + Name + Default>(value: &Json) -> Result<Decoded<M>, Error> {
    let full_name = M::full_name();
    let encoded = json_to_encoded(&full_name, value)?;
    let message = M::decode(encoded.bytes()).map_err(|error| {
        Error::at(
            "",
            format!("`{full_name}` did not decode from its own encoding: {error}"),
        )
    })?;
    Ok(Decoded { message, encoded })
}

/// Canonicalises a JSON text for a named message type: read it, check it
/// against the schema, and write it back in canonical form.
///
/// This is the function the golden files are made of, and the one a caller
/// wants when it has a type name rather than a Rust type — `gamectl verify`,
/// the rules table, the schema drift check.
///
/// **It answers exactly what [`decode`] then [`encode`] answers**, which is
/// the property decisions-log item 100 (10) asked for and
/// `canonicalise_agrees_with_decode_then_encode` asserts: a field the input
/// spells out at its proto3 default is dropped, because the canonical form has
/// one spelling and the committed goldens carry that one. The rule and its
/// three exclusions live at `codec::is_proto3_default`.
///
/// # Errors
///
/// As [`decode`], plus an unknown type name.
pub fn canonicalise(full_name: &str, text: &str) -> Result<String, Error> {
    let value = read(text)?;
    let bytes = json_to_wire(full_name, &value)?;
    write_wire(full_name, &bytes)
}
