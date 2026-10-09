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
#[must_use]
pub fn report_hash(
    playbook: &[u8],
    snapshot: &[u8],
    scope: &Scope,
    rules_hash: u64,
    verifier_version: &str,
    depth: Depth,
) -> u64 {
    let mut enc = Enc::with_capacity(
        playbook
            .len()
            .saturating_add(snapshot.len())
            .saturating_add(128),
    );
    append_bytes(&mut enc, REPORT_HASH_DOMAIN);
    // 1. the playbook bytes
    append_bytes(&mut enc, playbook);
    // 2. the snapshot bytes, then the seat's view of them
    append_bytes(&mut enc, snapshot);
    scope.encode(&mut enc);
    // 3. the rules table
    enc.u64(rules_hash);
    // 4. this build of the verifier
    append_bytes(&mut enc, verifier_version.as_bytes());
    // 5. the depth
    enc.i32(i32::from(depth));
    enc.finish()
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

/// One length-prefixed byte string.
fn append_bytes(enc: &mut Enc, bytes: &[u8]) {
    enc.len(u32::try_from(bytes.len()).unwrap_or(u32::MAX));
    enc.bytes(bytes);
}
