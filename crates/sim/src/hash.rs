// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The plan fingerprint (decisions-log item 77; the register's S1-42).
//!
//! `gp.v1.Meta.fingerprint` is
//!
//! ```text
//!     xxh3-64( PLAN_FINGERPRINT_DOMAIN || canonical_json_bytes )
//! ```
//!
//! under the project's one seed, where the canonical bytes are the playbook's
//! own canonical JSON with `meta.fingerprint` cleared. `crates/proto`'s
//! `fingerprint` module owns the **rule** -- the domain separator, the byte
//! order and what is hashed ([`canonical_input`]) -- and this module owns the
//! **arithmetic**, beside the state hash it shares a function and a seed with:
//! [`digest`] is xxh3-64 at [`crate::encoding::STATE_HASH_SEED`], and there is
//! no second hash function in the project (AGENTS.md section 5).
//!
//! It is determinism code (AGENTS.md section 5): a fingerprint is stamped into
//! every report and every save, so moving this function's value would refuse
//! every save already written. It moved here from `pharmakos-verifier`'s
//! `hash` module in S1's `build` lane **with its value unmoved**, and
//! `the_plan_fingerprint_is_unmoved` (this crate's `tests/vectors.rs`) holds it
//! to the value the committed verifier goldens carry. The verifier and the
//! gateway call this function; the verifier's `hash::plan_fingerprint` is now a
//! thin reader of it.
//!
//! [`plan_fingerprint_of`] computes the same value for a playbook decoded from
//! JSON without encoding it again (S3's `vfyq`, decisions-log item 158): the
//! canonical bytes are written straight from the wire bytes the playbook was
//! decoded from, with `meta.fingerprint` left out, which is
//! [`canonical_input`]'s clearing. `crates/proto`'s differential tests hold the
//! two texts equal on generated playbooks, and
//! `crates/verifier/tests/fingerprint.rs` holds the two functions equal on
//! every committed playbook and on the shapes P1 is measured at.

use crate::encoding::digest;
use pharmakos_proto::fingerprint::{FINGERPRINT_FIELD, PLAN_FINGERPRINT_DOMAIN, canonical_input};
use pharmakos_proto::gp::v1::Playbook;
use pharmakos_proto::json::{self, Decoded};

/// Why a playbook has no fingerprint.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum FingerprintError {
    /// The playbook cannot be written as canonical JSON, so there are no bytes
    /// to take a fingerprint over. For a message just decoded from canonical
    /// JSON this means the codec has lost a field it can read but not write.
    Canonical(json::Error),
}

impl core::fmt::Display for FingerprintError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FingerprintError::Canonical(error) => write!(
                f,
                "the playbook cannot be written as canonical JSON, so it has no fingerprint: \
                 {error}"
            ),
        }
    }
}

impl std::error::Error for FingerprintError {}

/// The plan fingerprint of a decoded playbook (decisions-log item 77).
///
/// `xxh3-64( PLAN_FINGERPRINT_DOMAIN || canonical_json_bytes )` under the
/// project's one seed. Clearing `meta.fingerprint` before hashing is
/// [`canonical_input`]'s, and it is what makes the fingerprint checkable:
/// recomputing over the file as it stands would hash the old fingerprint into
/// the new one.
///
/// # Errors
///
/// [`FingerprintError::Canonical`] when the playbook cannot be written as
/// canonical JSON.
pub fn plan_fingerprint(playbook: &Playbook) -> Result<u64, FingerprintError> {
    let canonical = canonical_input(playbook).map_err(FingerprintError::Canonical)?;
    let mut bytes = PLAN_FINGERPRINT_DOMAIN.to_vec();
    bytes.extend_from_slice(&canonical);
    Ok(digest(&bytes))
}

/// [`PLAN_FINGERPRINT_DOMAIN`] as text. It is ASCII, and the compiler checks
/// that it is UTF-8, so the canonical text can be written after it in one
/// buffer.
const DOMAIN_TEXT: &str = match core::str::from_utf8(PLAN_FINGERPRINT_DOMAIN) {
    Ok(text) => text,
    Err(_) => panic!("PLAN_FINGERPRINT_DOMAIN is not UTF-8"),
};

/// How many bytes of canonical text a byte of a playbook's wire bytes makes,
/// about: a capacity hint, so the text is not copied as it grows. The text is
/// indented a level for every message it nests, and a condition nests deep:
/// the size budget's shapes measure 23 and 24.
const TEXT_PER_WIRE_BYTE: usize = 24;

/// The plan fingerprint of a playbook decoded from JSON, from the wire bytes it
/// was decoded from: the value [`plan_fingerprint`] gives the playbook, without
/// encoding it again.
///
/// The canonical bytes are [`json::write_wire_into`]'s over those bytes with
/// `meta.fingerprint` ([`FINGERPRINT_FIELD`]) left out, which is the text [`canonical_input`] writes
/// for the playbook when the bytes are plain ([`json::Encoded::is_plain`]):
/// every singular field once, nothing at a default it would drop. Bytes that
/// are not plain, from a file that spells a field twice, are answered by
/// [`plan_fingerprint`] itself.
///
/// # Errors
///
/// [`FingerprintError::Canonical`] when the bytes cannot be written as
/// canonical JSON, as [`plan_fingerprint`].
pub fn plan_fingerprint_of(decoded: &Decoded<Playbook>) -> Result<u64, FingerprintError> {
    let encoded = decoded.encoded();
    if !encoded.is_plain() {
        return plan_fingerprint(decoded.message());
    }
    let mut text = String::with_capacity(
        DOMAIN_TEXT
            .len()
            .saturating_add(encoded.bytes().len().saturating_mul(TEXT_PER_WIRE_BYTE)),
    );
    text.push_str(DOMAIN_TEXT);
    json::write_wire_into(
        "gp.v1.Playbook",
        encoded.bytes(),
        FINGERPRINT_FIELD,
        &mut text,
    )
    .map_err(FingerprintError::Canonical)?;
    Ok(digest(text.as_bytes()))
}
