// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `report_hash`, and the verifier's reader of the plan fingerprint.
//!
//! # One hash function, and it is the sim's
//!
//! AGENTS.md section 5 asks that the project never add a second hash function,
//! and decisions-log item 70 keeps the canonical encoding and the state hash
//! together in `pharmakos-sim`. So nothing here computes anything: it builds
//! **bytes**, in a canonical order, and hands them to
//! [`Enc`](pharmakos_sim::encoding::Enc), which is xxh3-64 under the project's
//! one seed. This crate takes no hashing dependency of its own, and the
//! confinement test asserts that it never will.
//!
//! # The five inputs, in this order
//!
//! Spec section 11: a report is a pure function of five inputs, so a pre-check
//! and the check at submit are byte-identical.
//!
//! | # | Input | Why it moves the hash |
//! |---|---|---|
//! | 1 | the playbook bytes | a different file is a different report |
//! | 2 | the snapshot bytes, then the [`Scope`]'s canonical encoding | see below |
//! | 3 | `rules_hash` | a tuning change changes what the checks compare against (item 89) |
//! | 4 | [`VERIFIER_VERSION`](crate::VERIFIER_VERSION) | a different build may find different things |
//! | 5 | the depth id | "two reports compare only within one depth" |
//!
//! Each of the four byte-string inputs carries a `u32` length prefix and the two
//! numbers are fixed width, so the encoding is unambiguous: no concatenation of
//! one input can be mistaken for a different concatenation of another.
//!
//! **Why the scope rides with the snapshot.** The verifier reads the seat's
//! *view* of the snapshot, which the gateway builds and hands over as a
//! [`Scope`]; of the bytes themselves it reads one number, the coming
//! segment's length, in FULL's estimate stage (spec section 3 puts it in the
//! snapshot). Input 2 is therefore the pair — the bytes and
//! the view of them — because hashing the bytes alone would give two different
//! reports, built from two different views of one snapshot, the same hash. The
//! scope is the seat's view of that snapshot and is hashed as part of it. From
//! S1 that view carries the map's vents and seams and the commander's position
//! as well as the beacons and the economy (docs/design/targeting.md,
//! "Surfaces": "`report_hash` stays a pure function of `Scope`"), so whatever
//! the checks read of the map is inside the hash too.
//!
//! **The domain separator.** The bytes start with `REPORT_HASH_DOMAIN`, which is
//! a tag rather than a sixth input. It is there for the reason
//! `crates/proto`'s `fingerprint` module gives for the plan fingerprint's own
//! separator: a report hash must not be able to collide with a state hash or a
//! plan fingerprint that happened to digest the same bytes. It carries a version
//! of its own, so a change to this encoding that should move every report hash
//! moves it visibly, in one constant.
//!
//! # The bytes the playbook input is over
//!
//! Exactly the bytes handed in — the ones the caller wants a report about. The
//! verifier decodes what it hashes and hashes what it decodes. Comment
//! stripping, canonicalisation and the byte-exact JSONC round trip are
//! `plan-core`'s (T8, decisions-log item 74), and they happen **before** this
//! crate is called.

use pharmakos_proto::fingerprint::canonical_input;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::gp::v1::Playbook;
use pharmakos_proto::json;
use pharmakos_sim::encoding::Enc;

use crate::scope::Scope;

/// The domain separator the report hash's bytes begin with.
///
/// Changing it moves every `report_hash` in the project, so it is a determinism
/// change and is reviewed as one (AGENTS.md section 5).
///
/// `/2` from S1's targeting verifier (S1's plan, task `tgtv`): the [`Scope`]'s
/// encoding widened to carry the map's features and the commander's position,
/// so input 2's bytes are laid out differently from `/1`'s, and the version
/// says so in one place rather than leaving a reader to infer it from a moved
/// golden.
pub const REPORT_HASH_DOMAIN: &[u8] = b"gp.api.v1/report/2";

/// The report hash over the five inputs.
///
/// Public because it is the contract rather than an implementation detail: a
/// caller that wants to know whether a cached report is still current asks this
/// rather than re-running the pipeline, and a test can move one input at a time
/// and watch the hash move with it.
///
/// # Errors
///
/// [`HashError::TooLong`] when an input has no encoding: see the type.
pub fn report_hash(
    playbook: &[u8],
    snapshot: &[u8],
    scope: &Scope,
    rules_hash: u64,
    verifier_version: &str,
    depth: Depth,
) -> Result<u64, HashError> {
    let mut enc = inputs(playbook, snapshot, scope, rules_hash, verifier_version)?;
    // 5. the depth
    enc.i32(i32::from(depth));
    Ok(enc.finish())
}

/// The report hash at each of the two depths a report can be produced at,
/// computed once when the [`Input`](crate::Input) is assembled.
///
/// Computing both there is what makes [`verify`](crate::verify) infallible: an
/// input that has no encoding is refused when it is assembled, with a typed
/// error, rather than discovered half-way through a report that has nowhere to
/// put it. Each is built by the same function [`report_hash`] builds its bytes
/// with, so the bytes each hash is over are exactly [`report_hash`]'s; the
/// cost is a second encoding of the inputs, which are a playbook and a
/// snapshot, per assembled input.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct ReportHashes {
    quick: u64,
    full: u64,
}

impl ReportHashes {
    /// Both hashes over the same first four inputs.
    ///
    /// # Errors
    ///
    /// [`HashError::TooLong`], as [`report_hash`].
    pub(crate) fn new(
        playbook: &[u8],
        snapshot: &[u8],
        scope: &Scope,
        rules_hash: u64,
        verifier_version: &str,
    ) -> Result<ReportHashes, HashError> {
        let mut full = inputs(playbook, snapshot, scope, rules_hash, verifier_version)?;
        let mut quick = inputs(playbook, snapshot, scope, rules_hash, verifier_version)?;
        full.i32(i32::from(Depth::Full));
        quick.i32(i32::from(Depth::Quick));
        Ok(ReportHashes {
            quick: quick.finish(),
            full: full.finish(),
        })
    }

    /// The hash at the depth that ran: [`verify`](crate::verify) reads an
    /// unspecified depth as FULL before it asks, and so does this.
    pub(crate) const fn at(&self, depth: Depth) -> u64 {
        match depth {
            Depth::Quick => self.quick,
            Depth::Unspecified | Depth::Full => self.full,
        }
    }
}

/// The domain separator and the first four inputs, in order.
fn inputs(
    playbook: &[u8],
    snapshot: &[u8],
    scope: &Scope,
    rules_hash: u64,
    verifier_version: &str,
) -> Result<Enc, HashError> {
    // The capacity is a hint and nothing reads it back, so its sum saturating
    // could cost a reallocation and never a byte of the encoding.
    let mut enc = Enc::with_capacity(
        playbook
            .len()
            .saturating_add(snapshot.len())
            .saturating_add(128),
    );
    append_bytes(&mut enc, "the domain separator", REPORT_HASH_DOMAIN)?;
    // 1. the playbook bytes
    append_bytes(&mut enc, "the playbook", playbook)?;
    // 2. the snapshot bytes, then the seat's view of them
    append_bytes(&mut enc, "the snapshot", snapshot)?;
    scope.encode(&mut enc)?;
    // 3. the rules table
    enc.u64(rules_hash);
    // 4. this build of the verifier
    append_bytes(
        &mut enc,
        "the verifier version",
        verifier_version.as_bytes(),
    )?;
    Ok(enc)
}

/// The plan fingerprint for a decoded playbook (decisions-log item 77).
///
/// The arithmetic is the sim's: [`pharmakos_sim::hash::plan_fingerprint`],
/// `xxh3-64( PLAN_FINGERPRINT_DOMAIN || canonical_json_bytes )` under the
/// project's one seed, over the playbook's canonical JSON with
/// `meta.fingerprint` cleared. It lived here until S1's `build` lane folded it
/// into the sim's `hash` module beside the state hash (the register's S1-42),
/// with its value unmoved: the sim's `the_plan_fingerprint_is_unmoved` holds
/// it to the fingerprints this crate's report goldens carry. This reader keeps
/// the verifier's own question, "is there a fingerprint", for its two callers.
///
/// Returns `None` when the playbook cannot be written as canonical JSON, which
/// for a message that has just been decoded from canonical JSON means the codec
/// has lost a field it can read but not write
/// ([`pharmakos_sim::hash::FingerprintError`]); `E0008` and the report then
/// carry no fingerprint rather than a made-up one.
#[must_use]
pub fn plan_fingerprint(playbook: &Playbook) -> Option<u64> {
    pharmakos_sim::hash::plan_fingerprint(playbook).ok()
}

/// The canonical JSON of a playbook, for a caller that wants the bytes the
/// fingerprint is over.
///
/// # Errors
///
/// As [`json::encode`].
pub fn canonical_bytes(playbook: &Playbook) -> Result<Vec<u8>, json::Error> {
    canonical_input(playbook)
}

/// Why the five inputs could not be written as the report hash's bytes.
///
/// Every byte string and every list in the encoding carries a `u32` length
/// prefix, so a string or a list longer than `u32::MAX` has no encoding. No
/// playbook, snapshot or view this game produces comes near it (the size
/// budget and the gateway's frame limit are many orders of magnitude below),
/// but the length is converted, not assumed: until S2 the prefix saturated at
/// `u32::MAX`, which would have given two different over-long inputs one
/// length (S2's plan, task `vfy1`; decisions-log item 136 (5)). No real input
/// reached the saturation, so the change moves no encoding.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HashError {
    /// One input, or one list or string inside the seat's view, is longer
    /// than a `u32` length prefix can say.
    TooLong {
        /// Which input, or which part of the view.
        what: &'static str,
        /// Its length, in bytes or in entries.
        len: usize,
    },
}

impl core::fmt::Display for HashError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            HashError::TooLong { what, len } => write!(
                formatter,
                "{what} is {len} long, past the `u32` length prefix the report hash's encoding \
                 gives it"
            ),
        }
    }
}

impl std::error::Error for HashError {}

/// A length as the encoding's `u32` prefix.
///
/// # Errors
///
/// [`HashError::TooLong`] past `u32::MAX`.
pub(crate) fn length(what: &'static str, len: usize) -> Result<u32, HashError> {
    u32::try_from(len).map_err(|_| HashError::TooLong { what, len })
}

/// One length-prefixed byte string.
fn append_bytes(enc: &mut Enc, what: &'static str, bytes: &[u8]) -> Result<(), HashError> {
    enc.len(length(what, bytes.len())?);
    enc.bytes(bytes);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{HashError, ReportHashes, length, report_hash};
    use crate::scope::Scope;
    use pharmakos_proto::gp::api::v1::verify_plan::Depth;
    use pharmakos_sim::knowledge::SeatEconomy;
    use pharmakos_sim::tables::SeatId;

    #[test]
    fn a_length_past_the_prefix_is_refused_and_not_saturated() {
        assert_eq!(length("the playbook", 7), Ok(7));
        let most = usize::try_from(u32::MAX).expect("a u32 fits a usize here");
        assert_eq!(length("the playbook", most), Ok(u32::MAX));
        // One past the prefix: until S2 this read as `u32::MAX`, the length of
        // a different input.
        if let Some(past) = most.checked_add(1) {
            assert_eq!(
                length("the playbook", past),
                Err(HashError::TooLong {
                    what: "the playbook",
                    len: past
                })
            );
        }
    }

    #[test]
    fn the_two_precomputed_hashes_are_report_hash_at_each_depth() {
        let scope = Scope::new(SeatId::new(0), SeatEconomy::default());
        let both = ReportHashes::new(b"{}", b"snapshot", &scope, 7, "v").expect("hashable");
        for depth in [Depth::Quick, Depth::Full] {
            assert_eq!(
                Ok(both.at(depth)),
                report_hash(b"{}", b"snapshot", &scope, 7, "v", depth)
            );
        }
        assert_eq!(both.at(Depth::Unspecified), both.at(Depth::Full));
        assert_ne!(both.at(Depth::Quick), both.at(Depth::Full));
    }
}
