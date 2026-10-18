// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Seeded property tests over the two decoders of untrusted bytes in this
//! crate: `chunk_rle::decode`, which a client runs on the gateway's view feed,
//! and `base64::decode`, which the JSON codec runs on every `bytes` field and
//! the gateway's handshake runs on a client's key.
//!
//! The property is the one the global verification rule asks of a decoder of
//! untrusted input: only the typed error escapes. `chunk_rle::decode` answers
//! `Ok` with exactly `CHUNK_VOXELS` voxels or a `ChunkError`, and
//! `base64::decode` answers `Some` or `None`; neither panics, whatever the
//! bytes. Overflow checks are on in every profile (AGENTS.md section 4.1), so
//! an arithmetic overflow inside either decoder would fail these tests as a
//! panic rather than pass as a wrapped number.
//!
//! The generator is a std-only xorshift64*, seeded per case, so a failure is
//! reproduced by its seed and no dependency is added (AGENTS.md section 3).

use std::num::NonZeroU16;

use pharmakos_proto::chunk_rle::{self, CHUNK_VOXELS, ChunkError, RUN_BYTES};
use pharmakos_proto::json::base64;

/// Cases per property. Each is a few kilobytes at most, so the whole run
/// stays well under a second.
const CASES: u64 = 4_000;

/// xorshift64*: small, std-only and good enough to spread a test's inputs.
struct Rng(u64);

impl Rng {
    /// A generator for one case. The seed is mixed so that case 0 is not the
    /// all-zero state xorshift cannot leave.
    const fn new(case: u64) -> Self {
        Self(case.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03)
    }

    const fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A value in `0..bound`: the bound is non-zero by type.
    fn below(&mut self, bound: NonZeroU16) -> u16 {
        let [low, high, ..] = self.next().to_le_bytes();
        u16::from_le_bytes([low, high]) % bound
    }

    fn byte(&mut self) -> u8 {
        let [low, ..] = self.next().to_le_bytes();
        low
    }

    fn length(&mut self, bound: NonZeroU16) -> usize {
        usize::from(self.below(bound))
    }
}

/// A non-zero bound, spelt as a constant.
const fn bound(value: u16) -> NonZeroU16 {
    match NonZeroU16::new(value) {
        Some(bound) => bound,
        None => panic!("a bound of zero"),
    }
}

/// `CHUNK_VOXELS` as a run length; `the_chunk_fits_one_run` holds them equal.
const CHUNK_RUN: u16 = 32_768;

#[test]
fn the_chunk_fits_one_run() {
    assert_eq!(usize::from(CHUNK_RUN), CHUNK_VOXELS);
}

/// One run's length, biased toward the lengths that matter: 0 (refused), 1,
/// the largest (a whole chunk), and lengths that land the total just short of,
/// on, or just past `CHUNK_VOXELS`.
fn run_length(rng: &mut Rng, total: usize) -> u16 {
    // What is left of the chunk; nothing once the total has passed it.
    let left = u16::try_from(total).map_or(0, |total| CHUNK_RUN.saturating_sub(total));
    match rng.below(bound(8)) {
        0 => 0,
        1 => 1,
        2 => CHUNK_RUN,
        3 => left,
        4 => left.saturating_sub(1),
        5 => left.saturating_add(1),
        6 => u16::MAX,
        _ => rng.below(bound(4_096)),
    }
}

#[test]
fn chunk_decode_answers_a_whole_chunk_or_its_typed_error() {
    let mut accepted = 0_u64;
    for case in 0..CASES {
        let mut rng = Rng::new(case);
        let mut bytes: Vec<u8> = Vec::new();
        let mut total: usize = 0;
        let runs = rng.length(bound(12)).saturating_add(1);
        for _ in 0..runs {
            let length = run_length(&mut rng, total);
            total = total.saturating_add(usize::from(length));
            bytes.push(rng.byte());
            bytes.extend_from_slice(&length.to_le_bytes());
        }
        // Now and then a truncated or padded tail: not a whole run.
        match rng.below(bound(6)) {
            0 => {
                bytes.pop();
            }
            1 => bytes.push(rng.byte()),
            _ => {}
        }

        match chunk_rle::decode(&bytes) {
            Ok(voxels) => {
                assert_eq!(
                    voxels.len(),
                    CHUNK_VOXELS,
                    "case {case}: an accepted chunk is exactly one chunk"
                );
                assert_eq!(
                    chunk_rle::decode(&chunk_rle::encode(&voxels)).as_deref(),
                    Ok(voxels.as_slice()),
                    "case {case}: an accepted chunk re-encodes to itself"
                );
                accepted = accepted.saturating_add(1);
            }
            Err(
                ChunkError::PartialRun { .. }
                | ChunkError::EmptyRun { .. }
                | ChunkError::TooLong { .. }
                | ChunkError::TooShort { .. },
            ) => {
                if bytes.len().checked_rem(RUN_BYTES) != Some(0) {
                    assert!(
                        matches!(
                            chunk_rle::decode(&bytes),
                            Err(ChunkError::PartialRun { .. })
                        ),
                        "case {case}: a partial run is refused as one"
                    );
                }
            }
        }
    }
    assert!(
        accepted > 0,
        "the generator never built a whole chunk, so the Ok half went unchecked"
    );
}

/// Base64's symbols, both alphabets, padding, and a few that are neither.
const SYMBOLS: &[u8] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/-_====  .!";

#[test]
fn base64_decode_answers_bytes_or_none() {
    let mut accepted = 0_u64;
    for case in 0..CASES {
        let mut rng = Rng::new(case.wrapping_add(1 << 40));
        let length = rng.length(bound(40));
        let text: String = (0..length)
            .map(|_| {
                let index = rng.length(bound(u16::try_from(SYMBOLS.len()).expect("a short table")));
                char::from(
                    *SYMBOLS
                        .get(index)
                        .expect("an index below the table's length"),
                )
            })
            .collect();
        if let Some(bytes) = base64::decode(&text) {
            // What it read is what a writer writes for those bytes, up to the
            // spellings the mapping lets a reader accept.
            assert_eq!(
                base64::decode(&base64::encode(&bytes)).as_deref(),
                Some(bytes.as_slice()),
                "case {case}: {text:?}"
            );
            accepted = accepted.saturating_add(1);
        }
    }
    assert!(
        accepted > 0,
        "the generator never built valid base64, so the Some half went unchecked"
    );
}

#[test]
fn base64_round_trips_random_bytes() {
    for case in 0..CASES {
        let mut rng = Rng::new(case.wrapping_add(1 << 41));
        let bytes: Vec<u8> = (0..rng.length(bound(64))).map(|_| rng.byte()).collect();
        assert_eq!(
            base64::decode(&base64::encode(&bytes)),
            Some(bytes),
            "case {case}"
        );
    }
}
