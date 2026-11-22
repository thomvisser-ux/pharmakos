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
//! [`StreamingDigest`] is xxh3-64 at [`crate::encoding::STATE_HASH_SEED`], fed
//! in pieces, which answers what [`crate::encoding::digest`] answers for the
//! pieces joined, and there is no second hash function in the project
//! (AGENTS.md section 5).
//!
//! It is determinism code (AGENTS.md section 5): a fingerprint is stamped into
//! every report and every save, so moving this function's value would refuse
//! every save already written. It moved here from `pharmakos-verifier`'s
//! `hash` module in S1's `build` lane **with its value unmoved**, and
//! `the_plan_fingerprint_is_unmoved` (this crate's `tests/vectors.rs`) holds it
//! to the value the committed verifier goldens carry. The verifier and the
//! gateway call this module; the verifier's `hash::plan_fingerprint` is a thin
//! reader of it.
//!
//! [`plan_fingerprint_of`] computes the value for a playbook decoded from JSON
//! without encoding it again (S3's `vfyq`, decisions-log item 158), and
//! without holding its text (S3's `fpstream`): the canonical bytes are written
//! straight from the wire bytes the playbook was decoded from, with
//! `meta.fingerprint` left out, which is [`canonical_input`]'s clearing, and
//! each piece of them is hashed as it is written
//! ([`json::write_wire_into`]), so the fingerprint takes a fixed amount of
//! memory whatever the playbook's size. [`plan_fingerprint`] is the same value
//! for a caller that holds only a message: it writes the canonical bytes
//! whole, because the message must be encoded to reach them, and hashes them
//! through the same digest. `crates/proto`'s differential tests hold the text
//! written in pieces to the text written whole on generated playbooks;
//! `crates/verifier/tests/fingerprint.rs` and
//! `crates/plan-core/tests/fingerprint.rs` hold both functions to a one-shot
//! digest of [`canonical_input`]'s bytes on every committed playbook and on
//! the shapes P1 is measured at.

use crate::encoding::StreamingDigest;
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

/// The fingerprint's digest, begun with [`PLAN_FINGERPRINT_DOMAIN`]: a sink for
/// the canonical text, each piece hashed as it is handed over.
#[derive(Debug)]
struct Fingerprint {
    digest: StreamingDigest,
}

impl Fingerprint {
    fn new() -> Fingerprint {
        let mut digest = StreamingDigest::new();
        digest.update(PLAN_FINGERPRINT_DOMAIN);
        Fingerprint { digest }
    }

    fn finish(&self) -> u64 {
        self.digest.finish()
    }
}

impl<'t> Extend<&'t str> for Fingerprint {
    fn extend<I: IntoIterator<Item = &'t str>>(&mut self, pieces: I) {
        for piece in pieces {
            self.digest.update(piece.as_bytes());
        }
    }
}

/// The plan fingerprint of a playbook message (decisions-log item 77).
///
/// `xxh3-64( PLAN_FINGERPRINT_DOMAIN || canonical_json_bytes )` under the
/// project's one seed. Clearing `meta.fingerprint` before hashing is
/// [`canonical_input`]'s, and it is what makes the fingerprint checkable:
/// recomputing over the file as it stands would hash the old fingerprint into
/// the new one.
///
/// A caller that decoded the playbook from JSON holds its wire bytes and asks
/// [`plan_fingerprint_of`] instead, which neither clones nor encodes it nor
/// holds its text; this function does all three, because a message alone has
/// no other way to its canonical bytes.
///
/// # Errors
///
/// [`FingerprintError::Canonical`] when the playbook cannot be written as
/// canonical JSON.
pub fn plan_fingerprint(playbook: &Playbook) -> Result<u64, FingerprintError> {
    let canonical = canonical_input(playbook).map_err(FingerprintError::Canonical)?;
    let mut fingerprint = Fingerprint::new();
    fingerprint.digest.update(&canonical);
    Ok(fingerprint.finish())
}

/// The plan fingerprint of a playbook decoded from JSON, from the wire bytes it
/// was decoded from: the value [`plan_fingerprint`] gives the playbook, without
/// encoding it again and without holding its text.
///
/// The canonical bytes are [`json::write_wire_into`]'s over those bytes with
/// `meta.fingerprint` ([`FINGERPRINT_FIELD`]) left out, which is the text
/// [`canonical_input`] writes for the playbook: the codec's bytes hold every
/// singular field once and nothing at a default prost would drop
/// ([`json::Encoded`]). They reach the digest in the codec's pieces, each
/// hashed as it comes, so what this function holds is the codec's one
/// gathering buffer and the digest's fixed state, never the text, which at
/// the size budget is near a megabyte. `crates/proto`'s differential tests
/// hold the text written in pieces to the text of the playbook encoded again
/// for every generated playbook the codec accepts.
///
/// # Errors
///
/// [`FingerprintError::Canonical`] when the bytes cannot be written as
/// canonical JSON, as [`plan_fingerprint`].
pub fn plan_fingerprint_of(decoded: &Decoded<Playbook>) -> Result<u64, FingerprintError> {
    let mut fingerprint = Fingerprint::new();
    json::write_wire_into(
        "gp.v1.Playbook",
        decoded.encoded().bytes(),
        FINGERPRINT_FIELD,
        &mut fingerprint,
    )
    .map_err(FingerprintError::Canonical)?;
    Ok(fingerprint.finish())
}
