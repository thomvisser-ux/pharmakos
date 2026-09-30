// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The plan fingerprint: what it is over, and who computes it.
//!
//! Decisions-log item 77 settles the rule. `gp.v1.Meta.fingerprint` is
//!
//! ```text
//!     xxh3-64( PLAN_FINGERPRINT_DOMAIN || canonical_json_bytes )
//! ```
//!
//! seeded with item 48's `STATE_HASH_SEED = 0x5048_4152_4D4B_4F53`, written
//! into the field as **eight big-endian bytes** so that the base64 the proto
//! JSON mapping emits for a `bytes` field reads and sorts in digest order.
//!
//! `canonical_json_bytes` is exactly what [`crate::json::encode`] writes for
//! the playbook **with `fingerprint` itself cleared** — [`canonical_input`]
//! below does that clearing, so no caller has to remember to.
//!
//! # Why the implementation is not here
//!
//! AGENTS.md section 5 asks that the project never add a second hash function,
//! and decisions-log item 70 keeps the canonical encoding, the RNG and the
//! state hash together as modules inside `pharmakos-sim`, which is what makes
//! "the determinism code" a nameable contract path. `STATE_HASH_SEED` lives
//! there. If this crate took `xxhash-rust` as well, the seed constant would
//! have two homes and the determinism contract two authors.
//!
//! So `crates/proto` owns the **rule** — the domain separator, the byte order,
//! and what is hashed — and the arithmetic lives beside the sim's hash: the
//! verifier's `hash::plan_fingerprint` computes it today, xxh3-64 under the
//! sim's `STATE_HASH_SEED` over [`PLAN_FINGERPRINT_DOMAIN`] followed by
//! [`canonical_input`]'s bytes, and stores [`to_field`]'s bytes in
//! `Meta.fingerprint`; the gateway calls it for what a save carries beside
//! each seal.
//!
//! PLACEHOLDER: the arithmetic's home, the sim's `hash` module — owner, S1, the
//! next time `crates/sim` is open, with the value unmoved (the register's
//! S1-42, decisions-log item 77). Until then the verifier's copy is the one to
//! call, and this crate's rule is what both copies are held to.

use crate::gp::v1::Playbook;
use crate::json;

/// The domain separator. Prefixed to the canonical bytes before hashing, so a
/// plan fingerprint can never collide with a state hash or a report hash that
/// happened to digest the same bytes.
///
/// It carries the schema package and a version of its own: if the
/// canonicalisation ever changes in a way that should move every fingerprint,
/// the separator changes with it and the move is visible in one constant.
pub const PLAN_FINGERPRINT_DOMAIN: &[u8] = b"gp.v1/fingerprint/1";

/// The bytes a fingerprint is taken over: the playbook's canonical JSON with
/// `Meta.fingerprint` cleared.
///
/// Clearing it is what makes the fingerprint checkable. A file arrives with a
/// fingerprint already in it; recomputing over the file as it stands would
/// hash the old fingerprint into the new one.
///
/// # Errors
///
/// If the playbook cannot be written as canonical JSON.
pub fn canonical_input(playbook: &Playbook) -> Result<Vec<u8>, json::Error> {
    let mut cleared = playbook.clone();
    if let Some(meta) = cleared.meta.as_mut() {
        meta.fingerprint.clear();
    }
    Ok(json::encode(&cleared)?.into_bytes())
}

/// A 64-bit digest as the eight big-endian bytes the field holds.
#[must_use]
pub fn to_field(digest: u64) -> Vec<u8> {
    digest.to_be_bytes().to_vec()
}

/// The digest a field holds, or `None` if the field is empty or the wrong
/// length. A malformed fingerprint is a diagnostic for the verifier to raise,
/// never something to repair quietly.
#[must_use]
pub fn from_field(bytes: &[u8]) -> Option<u64> {
    let array: [u8; 8] = bytes.try_into().ok()?;
    Some(u64::from_be_bytes(array))
}
