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
mod value;

pub use codec::{json_to_wire, wire_to_json};
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
    Ok(write(&encode_json(message)?))
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
    decode_json(&read(text)?)
}

/// A message read from an already-parsed canonical JSON value.
///
/// # Errors
///
/// As [`decode`].
pub fn decode_json<M: Message + Name + Default>(value: &Json) -> Result<M, Error> {
    let full_name = M::full_name();
    let bytes = json_to_wire(&full_name, value)?;
    M::decode(bytes.as_slice()).map_err(|error| {
        Error::at(
            "",
            format!("`{full_name}` did not decode from its own encoding: {error}"),
        )
    })
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
    Ok(write(&wire_to_json(full_name, &bytes)?))
}
