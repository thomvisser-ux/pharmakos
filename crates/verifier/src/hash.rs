// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `report_hash`, and the verifier's reader of the plan fingerprint.
//!
//! # One hash function, and it is the sim's
//!
//! AGENTS.md section 5 asks that the project never add a second hash function,
//! and decisions-log item 70 keeps the canonical encoding and the state hash
//! together in `pharmakos-sim`. So nothing here computes anything: it builds
//! **bytes**, in a canonical order, and hands them to the sim's encoding:
//! the small parts are written by [`Enc`](pharmakos_sim::encoding::Enc) and
//! every part, the playbook and the snapshot as they stand among them, goes
//! to [`StreamingDigest`](pharmakos_sim::encoding::StreamingDigest), which is
//! xxh3-64 under the project's one seed and answers what `Enc` answers for
//! the parts joined. This crate takes no hashing dependency of its own, and the
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
use pharmakos_sim::encoding::{Enc, StreamingDigest};

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
/// golden. `/3` from S2's second verifier lane (task `vfy2`): the view gained
/// the seat's sightings with their ages, for an Attack target's freshness.
/// `/4` from S3's `vfy5`: the view gained the seat's licences, its grown
/// sphere radii and the beacons a Resonance Spire stands at.
pub const REPORT_HASH_DOMAIN: &[u8] = b"gp.api.v1/report/4";

/// The domain separator of the one report that is about no playbook bytes:
/// `E0029`'s, for a player's text past
/// [`MAX_PLAYBOOK_CHARS`](crate::MAX_PLAYBOOK_CHARS) (S3's `vfy6`,
/// [`Input::too_long`](crate::Input::too_long)).
///
/// Its bytes are this separator, then the cap itself as a `u64`, then the
/// inputs [`report_hash`] takes with an empty playbook, because the text is
/// never read. The separator keeps the hash from being any playbook's (an
/// empty file's report, under [`REPORT_HASH_DOMAIN`], is a different report
/// over the same other inputs), and the cap in the bytes keeps a change to
/// the cap from giving two different verdicts one hash. It moves no existing
/// report hash. [`too_long_report_hash`] reproduces it.
pub const TOO_LONG_REPORT_DOMAIN: &[u8] = b"gp.api.v1/report-too-long/1";

/// What a report is about: a playbook's bytes, or a player's text past the
/// cap, which has none ([`TOO_LONG_REPORT_DOMAIN`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Subject<'a> {
    /// The bytes handed to [`Input::new`](crate::Input::new).
    Playbook(&'a [u8]),
    /// `E0029`'s report ([`Input::too_long`](crate::Input::too_long)).
    TooLong,
}

/// `E0029`'s report hash (S3's `vfy6`): [`report_hash`]'s bytes under
/// [`TOO_LONG_REPORT_DOMAIN`] with the cap folded in, for a caller that wants
/// to check one without running the pipeline.
///
/// # Errors
///
/// [`HashError::TooLong`] when an input has no encoding, as [`report_hash`].
pub fn too_long_report_hash(
    snapshot: &[u8],
    scope: &Scope,
    rules_hash: u64,
    verifier_version: &str,
    depth: Depth,
) -> Result<u64, HashError> {
    let inputs = inputs(
        Subject::TooLong,
        snapshot,
        scope,
        rules_hash,
        verifier_version,
    )?;
    Ok(at_depth(&inputs, depth))
}

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
    let inputs = inputs(
        Subject::Playbook(playbook),
        snapshot,
        scope,
        rules_hash,
        verifier_version,
    )?;
    Ok(at_depth(&inputs, depth))
}

/// The report hash at each of the two depths a report can be produced at,
/// computed once when the [`Input`](crate::Input) is assembled.
///
/// Computing both there is what makes [`verify`](crate::verify) infallible: an
/// input that has no encoding is refused when it is assembled, with a typed
/// error, rather than discovered half-way through a report that has nowhere to
/// put it. Each is built by the same function [`report_hash`] builds its bytes
/// with, so the bytes each hash is over are exactly [`report_hash`]'s. The
/// first four inputs are digested once and the digest's state copied for the
/// second depth, so the playbook and the snapshot, which at the size budget
/// are near a megabyte, are read once and never copied.
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
        subject: Subject<'_>,
        snapshot: &[u8],
        scope: &Scope,
        rules_hash: u64,
        verifier_version: &str,
    ) -> Result<ReportHashes, HashError> {
        let inputs = inputs(subject, snapshot, scope, rules_hash, verifier_version)?;
        Ok(ReportHashes {
            quick: at_depth(&inputs, Depth::Quick),
            full: at_depth(&inputs, Depth::Full),
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

/// The fifth input, the depth, after the first four: the report hash.
fn at_depth(inputs: &StreamingDigest, depth: Depth) -> u64 {
    let mut digest = inputs.clone();
    // The encoding's version byte and the depth's four.
    let mut part = Enc::with_capacity(5);
    // 5. the depth
    part.i32(i32::from(depth));
    digest.update(part.body());
    digest.finish()
}

/// The domain separator and the first four inputs, in order, digested.
///
/// The bytes are the ones one [`Enc`] would build whole (the test
/// `the_streamed_inputs_are_the_whole_buffers_bytes` holds them to it): the
/// parts between the two large byte strings are written by the one encoder
/// and handed to the digest, [`Enc::as_bytes`] for the first, which carries
/// the encoding's version byte, and [`Enc::body`] for the later ones; the
/// playbook and the snapshot are handed over as they stand. So nothing here
/// holds a copy of either, and each is read once.
fn inputs(
    subject: Subject<'_>,
    snapshot: &[u8],
    scope: &Scope,
    rules_hash: u64,
    verifier_version: &str,
) -> Result<StreamingDigest, HashError> {
    let playbook = match subject {
        Subject::Playbook(bytes) => bytes,
        Subject::TooLong => &[],
    };
    let mut digest = StreamingDigest::new();
    let mut enc = Enc::with_capacity(128);
    match subject {
        Subject::Playbook(_) => {
            append_bytes(&mut enc, "the domain separator", REPORT_HASH_DOMAIN)?;
        }
        Subject::TooLong => {
            append_bytes(&mut enc, "the domain separator", TOO_LONG_REPORT_DOMAIN)?;
            let cap = u64::try_from(crate::MAX_PLAYBOOK_CHARS).map_err(|_| HashError::TooLong {
                what: "the playbook cap",
                len: crate::MAX_PLAYBOOK_CHARS,
            })?;
            enc.u64(cap);
        }
    }
    // 1. the playbook bytes
    enc.len(length("the playbook", playbook.len())?);
    digest.update(enc.as_bytes());
    digest.update(playbook);
    // 2. the snapshot bytes, then the seat's view of them
    enc.clear();
    enc.len(length("the snapshot", snapshot.len())?);
    digest.update(enc.body());
    digest.update(snapshot);
    enc.clear();
    scope.encode(&mut enc)?;
    // 3. the rules table
    enc.u64(rules_hash);
    // 4. this build of the verifier
    append_bytes(
        &mut enc,
        "the verifier version",
        verifier_version.as_bytes(),
    )?;
    digest.update(enc.body());
    Ok(digest)
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

/// A hash refusal names what it hashed and a length: engine words (S3's
/// `vfyq2`, fix pass 3, item M2 (d)).
impl pharmakos_proto::json::grammar::EngineWords for HashError {}
impl pharmakos_proto::json::grammar::door::EngineSeal for HashError {}

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
    use super::{
        HashError, REPORT_HASH_DOMAIN, ReportHashes, Subject, TOO_LONG_REPORT_DOMAIN, length,
        report_hash, too_long_report_hash,
    };
    use crate::scope::Scope;
    use pharmakos_proto::gp::api::v1::verify_plan::Depth;
    use pharmakos_sim::encoding::Enc;
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
        let both = ReportHashes::new(Subject::Playbook(b"{}"), b"snapshot", &scope, 7, "v")
            .expect("hashable");
        for depth in [Depth::Quick, Depth::Full] {
            assert_eq!(
                Ok(both.at(depth)),
                report_hash(b"{}", b"snapshot", &scope, 7, "v", depth)
            );
        }
        assert_eq!(both.at(Depth::Unspecified), both.at(Depth::Full));
        assert_ne!(both.at(Depth::Quick), both.at(Depth::Full));
    }

    /// The report hash's bytes built whole by one encoder, as the hash was
    /// computed until the inputs were streamed: the reference the streamed
    /// digest is held to.
    fn whole_buffer(
        subject: Subject<'_>,
        snapshot: &[u8],
        scope: &Scope,
        rules_hash: u64,
        verifier_version: &str,
        depth: Depth,
    ) -> u64 {
        let mut enc = Enc::with_capacity(0);
        let put = |enc: &mut Enc, bytes: &[u8]| {
            enc.len(u32::try_from(bytes.len()).expect("a test's bytes fit the prefix"));
            enc.bytes(bytes);
        };
        match subject {
            Subject::Playbook(playbook) => {
                put(&mut enc, REPORT_HASH_DOMAIN);
                put(&mut enc, playbook);
            }
            Subject::TooLong => {
                put(&mut enc, TOO_LONG_REPORT_DOMAIN);
                enc.u64(u64::try_from(crate::MAX_PLAYBOOK_CHARS).expect("the cap fits a u64"));
                put(&mut enc, &[]);
            }
        }
        put(&mut enc, snapshot);
        scope
            .encode(&mut enc)
            .expect("a small view has an encoding");
        enc.u64(rules_hash);
        put(&mut enc, verifier_version.as_bytes());
        enc.i32(i32::from(depth));
        enc.finish()
    }

    /// Streaming the inputs moved no hash: for empty, small and megabyte
    /// inputs, at each depth and for `E0029`'s subject, the streamed digest
    /// is the digest of the bytes one encoder builds whole.
    #[test]
    fn the_streamed_inputs_are_the_whole_buffers_bytes() {
        let scope = Scope::new(SeatId::new(2), SeatEconomy::default());
        let large: Vec<u8> = (0..1_000_003_u32).map(|n| n.to_le_bytes()[0]).collect();
        let playbooks: [&[u8]; 4] = [b"", b"{}", b"{\"meta\":{}}", &large];
        let snapshots: [&[u8]; 3] = [b"", b"snapshot", &large];
        for depth in [Depth::Quick, Depth::Full] {
            for snapshot in snapshots {
                for playbook in playbooks {
                    assert_eq!(
                        report_hash(playbook, snapshot, &scope, 7, "0.1.0-test", depth),
                        Ok(whole_buffer(
                            Subject::Playbook(playbook),
                            snapshot,
                            &scope,
                            7,
                            "0.1.0-test",
                            depth
                        )),
                        "a playbook of {} bytes and a snapshot of {}",
                        playbook.len(),
                        snapshot.len()
                    );
                }
                assert_eq!(
                    too_long_report_hash(snapshot, &scope, 9, "0.1.0-test", depth),
                    Ok(whole_buffer(
                        Subject::TooLong,
                        snapshot,
                        &scope,
                        9,
                        "0.1.0-test",
                        depth
                    ))
                );
            }
        }
    }
}
