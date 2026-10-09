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

use crate::encoding::digest;
use pharmakos_proto::fingerprint::{PLAN_FINGERPRINT_DOMAIN, canonical_input};
use pharmakos_proto::gp::v1::Playbook;
use pharmakos_proto::json;

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
