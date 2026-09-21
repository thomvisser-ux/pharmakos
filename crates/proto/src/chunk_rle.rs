// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The voxel run-length codec that `gp.api.v1.ViewChunk.voxels_rle` carries.
//!
//! One chunk of terrain is 32 768 material bytes. Sent raw, one keyframe of
//! the skeleton's 288-chunk map is 9.4 MB and fifty round trips; run-length
//! encoded it is a few hundred kilobytes, because generated terrain is a
//! heightmap and a row of it is a handful of runs.
//!
//! # The format, in three sentences
//!
//! Three bytes a run: the material byte, then the run length as a
//! **little-endian `u16`**, 1 to 32 768. Runs cross rows and layers freely —
//! the voxel order inside a chunk is `x + 32*y + 1024*z` (decisions-log item
//! 92) and a run is simply a stretch of that order. The lengths must sum to
//! **exactly** [`CHUNK_VOXELS`], or [`decode`] refuses: a chunk delivered
//! short would be drawn short, and a client cannot tell a truncated payload
//! from a map with a hole in it.
//!
//! # Why it lives here
//!
//! One codec, in the crate both sides already share, beside the base64 they
//! already share. The producer is the gateway's view feed and the consumer is
//! `client-gdext`; two implementations of one byte format with only a fixture
//! between them is how a format drifts, and the base64 module's own history
//! (decisions-log item 100 (10) — the gateway carried a copy until the
//! original was made reachable) already answered this question once.
//!
//! A byte format inside a `bytes` field is a contract `buf breaking` cannot
//! see, which is why the whole of it is written out in `ViewChunk`'s own
//! comment as well as here.

/// Voxels in one chunk: `32 * 32 * 32`.
///
/// The chunk edge is `pharmakos_sim::voxels::CHUNK_EDGE` and this crate
/// deliberately does not depend on the sim to learn it — `crates/proto` has
/// one dependency and it is `prost` (AGENTS.md section 3). The two are pinned
/// to each other by a gateway test rather than by an import, for the same
/// reason the material table is.
pub const CHUNK_VOXELS: usize = 32 * 32 * 32;

/// Bytes in one encoded run: the material, then two of length.
pub const RUN_BYTES: usize = 3;

/// The longest run one triple can carry, which is a whole chunk.
pub const MAX_RUN: usize = CHUNK_VOXELS;

/// Why a payload is not a chunk.
///
/// Four ways, and each one is a different bug in whoever produced the bytes,
/// so they are four variants rather than one `None`: a client that logs the
/// reason can tell a truncated transfer from a producer that lost count.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ChunkError {
    /// The payload is not a whole number of 3-byte runs.
    PartialRun {
        /// How many bytes were left over.
        trailing: usize,
    },
    /// A run of zero voxels, which advances nothing and would let a payload
    /// of any length claim to be a chunk.
    EmptyRun {
        /// Which run, counted from zero.
        run: usize,
    },
    /// The run lengths add up to more than a chunk.
    TooLong {
        /// How many voxels the runs covered before the overrun was found.
        voxels: usize,
    },
    /// The run lengths add up to fewer than a chunk.
    TooShort {
        /// How many voxels the runs covered in total.
        voxels: usize,
    },
}

impl core::fmt::Display for ChunkError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            ChunkError::PartialRun { trailing } => write!(
                formatter,
                "a chunk is a whole number of {RUN_BYTES}-byte runs and this has {trailing} \
                 bytes left over"
            ),
            ChunkError::EmptyRun { run } => write!(
                formatter,
                "run {run} is zero voxels long, and a run covers at least one"
            ),
            ChunkError::TooLong { voxels } => write!(
                formatter,
                "the runs cover more than the {CHUNK_VOXELS} voxels of a chunk ({voxels} so far)"
            ),
            ChunkError::TooShort { voxels } => write!(
                formatter,
                "the runs cover {voxels} voxels and a chunk is {CHUNK_VOXELS}"
            ),
        }
    }
}

impl core::error::Error for ChunkError {}

/// Run-length encode one chunk's materials.
///
/// `voxels` is read in the sim's own order and is expected to be
/// [`CHUNK_VOXELS`] long; a shorter or longer slice encodes what it holds,
/// which [`decode`] then refuses. Encoding never fails, so it returns bytes
/// rather than a result: the failure this format has is on the reading side,
/// where a payload arrived from somewhere else.
#[must_use]
pub fn encode(voxels: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut index: usize = 0;
    while index < voxels.len() {
        let Some(material) = voxels.get(index).copied() else {
            break;
        };
        let mut run: usize = 1;
        while run < MAX_RUN {
            let next = index.saturating_add(run);
            if voxels.get(next).copied() != Some(material) {
                break;
            }
            run = run.saturating_add(1);
        }
        out.push(material);
        // `run` is at most MAX_RUN, which is 32 768 and fits a u16; the
        // fallback is unreachable and is a zero rather than a panic because a
        // codec that panics on its own arithmetic is worse than one that
        // writes a run `decode` will refuse.
        let length = u16::try_from(run).unwrap_or(0);
        out.extend_from_slice(&length.to_le_bytes());
        index = index.saturating_add(run);
    }
    out
}

/// Read one chunk's materials back.
///
/// # Errors
///
/// A [`ChunkError`] for a payload that is not a whole number of runs, holds a
/// run of zero, or whose lengths do not sum to exactly [`CHUNK_VOXELS`].
pub fn decode(bytes: &[u8]) -> Result<Vec<u8>, ChunkError> {
    let trailing = bytes.len().checked_rem(RUN_BYTES).unwrap_or(0);
    if trailing != 0 {
        return Err(ChunkError::PartialRun { trailing });
    }
    let mut out: Vec<u8> = Vec::with_capacity(CHUNK_VOXELS);
    for (run, triple) in bytes.chunks(RUN_BYTES).enumerate() {
        let material = triple.first().copied().unwrap_or(0);
        let low = triple.get(1).copied().unwrap_or(0);
        let high = triple.get(2).copied().unwrap_or(0);
        let length = usize::from(u16::from_le_bytes([low, high]));
        if length == 0 {
            return Err(ChunkError::EmptyRun { run });
        }
        if out.len().saturating_add(length) > CHUNK_VOXELS {
            return Err(ChunkError::TooLong {
                voxels: out.len().saturating_add(length),
            });
        }
        out.resize(out.len().saturating_add(length), material);
    }
    if out.len() != CHUNK_VOXELS {
        return Err(ChunkError::TooShort { voxels: out.len() });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{CHUNK_VOXELS, ChunkError, MAX_RUN, RUN_BYTES, decode, encode};

    /// A chunk that is all one material: one run, three bytes.
    #[test]
    fn a_uniform_chunk_is_one_run() {
        let voxels = vec![2_u8; CHUNK_VOXELS];
        let encoded = encode(&voxels);
        assert_eq!(encoded.len(), RUN_BYTES);
        assert_eq!(encoded.first().copied(), Some(2));
        assert_eq!(
            encoded.get(1..3),
            Some([0x00, 0x80].as_slice()),
            "32 768, little-endian"
        );
        assert_eq!(decode(&encoded).expect("a chunk"), voxels);
    }

    /// The worst case: no two neighbours alike, so every voxel is its own run
    /// and the payload is three times the raw chunk. The page cut is by BYTES
    /// for exactly this reason, and the first chunk of a page always goes
    /// through so that such a chunk can never be a hole nothing fills.
    #[test]
    fn an_alternating_chunk_is_three_times_raw_and_still_round_trips() {
        let voxels: Vec<u8> = (0..CHUNK_VOXELS)
            .map(|index| u8::try_from(index.checked_rem(2).unwrap_or(0)).unwrap_or(0))
            .collect();
        let encoded = encode(&voxels);
        assert_eq!(encoded.len(), CHUNK_VOXELS.saturating_mul(RUN_BYTES));
        assert_eq!(decode(&encoded).expect("a chunk"), voxels);
    }

    /// A heightmap chunk, which is what the generator actually produces: solid
    /// below a per-column height and air above it.
    #[test]
    fn a_heightmap_chunk_round_trips_byte_for_byte() {
        let mut voxels = vec![0_u8; CHUNK_VOXELS];
        for z in 0..32_usize {
            for y in 0..32_usize {
                for x in 0..32_usize {
                    let height = 8_usize.saturating_add(x.checked_rem(5).unwrap_or(0));
                    let index = x
                        .saturating_add(y.saturating_mul(32))
                        .saturating_add(z.saturating_mul(1024));
                    if let Some(slot) = voxels.get_mut(index) {
                        *slot = if z < height { 2 } else { 0 };
                    }
                }
            }
        }
        let encoded = encode(&voxels);
        assert!(encoded.len() < CHUNK_VOXELS, "{}", encoded.len());
        assert_eq!(decode(&encoded).expect("a chunk"), voxels);
    }

    /// A run may cross a row and a layer: the order is one stretch of 32 768
    /// voxels and nothing in the format knows where a row ends.
    #[test]
    fn a_run_crosses_rows_and_layers() {
        let mut voxels = vec![1_u8; CHUNK_VOXELS];
        if let Some(slot) = voxels.get_mut(CHUNK_VOXELS.saturating_sub(1)) {
            *slot = 0;
        }
        let encoded = encode(&voxels);
        assert_eq!(encoded.len(), RUN_BYTES.saturating_mul(2));
        assert_eq!(decode(&encoded).expect("a chunk"), voxels);
    }

    #[test]
    fn a_short_or_long_run_list_is_refused_rather_than_drawn() {
        // One run short of a chunk.
        let short = encode(&vec![1_u8; CHUNK_VOXELS.saturating_sub(10)]);
        assert_eq!(
            decode(&short).expect_err("refused"),
            ChunkError::TooShort {
                voxels: CHUNK_VOXELS.saturating_sub(10)
            }
        );

        // Two whole-chunk runs.
        let mut long = encode(&vec![1_u8; CHUNK_VOXELS]);
        long.extend_from_slice(&encode(&[2_u8]));
        assert!(matches!(
            decode(&long).expect_err("refused"),
            ChunkError::TooLong { .. }
        ));

        // Bytes left over.
        assert_eq!(
            decode(&[1, 0, 0, 1]).expect_err("refused"),
            ChunkError::PartialRun { trailing: 1 }
        );

        // A run of nothing, which would otherwise let any length of payload
        // claim to be a chunk.
        assert_eq!(
            decode(&[1, 0, 0]).expect_err("refused"),
            ChunkError::EmptyRun { run: 0 }
        );

        // And an empty payload is a chunk of nothing, which is not a chunk.
        assert_eq!(
            decode(&[]).expect_err("refused"),
            ChunkError::TooShort { voxels: 0 }
        );
    }

    #[test]
    fn every_refusal_says_what_it_found() {
        for error in [
            ChunkError::PartialRun { trailing: 2 },
            ChunkError::EmptyRun { run: 7 },
            ChunkError::TooLong { voxels: 40_000 },
            ChunkError::TooShort { voxels: 12 },
        ] {
            assert!(!error.to_string().is_empty(), "{error:?}");
        }
        assert_eq!(MAX_RUN, CHUNK_VOXELS);
    }
}
