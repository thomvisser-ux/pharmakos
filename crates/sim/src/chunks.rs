// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The chunk store's seam into the state hash (decisions log item 66).
//!
//! The chunk store itself is T5's. What lives here is the *encoder* it plugs
//! into, defined now so that T5 adds voxels without touching the hash contract.
//!
//! **The chunk store enters the hash as per-chunk digests, never as bytes.**
//! G3′ measured the alternative: hashing the store's 9.4 MB would cost 0.83 ms,
//! 43× the whole eight-table hash and three times the entire non-pathing tick.
//! A digest is refreshed only when an edit touches its chunk; every other chunk
//! contributes the eight bytes it contributed last tick. That is not a
//! dirty-flag scheme — it is the encoding itself, which is why item 66 keeps it
//! while rejecting incremental hashing everywhere else.
//!
//! # How T5 plugs in
//!
//! 1. Build [`ChunkDigests`] with the chunk count the map generator produced.
//! 2. After every voxel edit settles, call [`ChunkDigests::refresh`] with
//!    `digest(chunk_bytes)` from [`crate::encoding::digest`].
//! 3. Nothing else changes: the hash phase already walks the digests in chunk
//!    index order.

use crate::encoding::Enc;

/// Chunks in the skeleton's provisional map.
///
/// `384 × 384 × 64` voxels in 32³ chunks is `12 × 12 × 2 = 288`, which is also
/// the configuration item 66's measurements were taken at.
///
/// PLACEHOLDER: map dimensions are Tuning and belong to the owner at T5
/// (skeleton plan decision 14). This constant is the shape the seam was sized
/// against, not a promise about the map.
pub const SKELETON_CHUNK_COUNT: u32 = 288;

/// One xxh3-64 digest per chunk, in chunk-index order.
///
/// Not a cache: it is the canonical form of the chunk store as far as the state
/// hash is concerned, so it is hashed state and it round-trips through the
/// snapshot like every other table.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ChunkDigests {
    digests: Vec<u64>,
}

impl ChunkDigests {
    /// A store of `count` chunks, every digest at the empty-chunk value zero.
    ///
    /// Zero is a legitimate digest for "nothing has ever been written here";
    /// T5 sets the real value for every chunk the generator fills, at
    /// construction, before the first hash is taken.
    #[must_use]
    pub fn new(count: u32) -> ChunkDigests {
        ChunkDigests {
            digests: vec![0; usize::try_from(count).unwrap_or(0)],
        }
    }

    /// How many chunks the store covers.
    #[must_use]
    pub fn len(&self) -> u32 {
        u32::try_from(self.digests.len()).unwrap_or(u32::MAX)
    }

    /// Whether the store is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.digests.is_empty()
    }

    /// The digest of one chunk, or `None` when the index is off the map.
    #[must_use]
    pub fn get(&self, chunk_index: u32) -> Option<u64> {
        self.digests
            .get(usize::try_from(chunk_index).ok()?)
            .copied()
    }

    /// Record a chunk's digest after an edit.
    ///
    /// Returns `false` when the index is off the map, which is a caller bug and
    /// never a game state.
    pub fn refresh(&mut self, chunk_index: u32, digest: u64) -> bool {
        let Ok(index) = usize::try_from(chunk_index) else {
            return false;
        };
        match self.digests.get_mut(index) {
            Some(slot) => {
                *slot = digest;
                true
            }
            None => false,
        }
    }

    /// Every digest, in chunk-index order — the snapshot's view.
    #[must_use]
    pub fn as_slice(&self) -> &[u64] {
        &self.digests
    }

    /// Replace every digest from a restored snapshot.
    pub fn restore(&mut self, digests: Vec<u64>) {
        self.digests = digests;
    }

    /// Append the store to the canonical encoding: a `u32` count then one
    /// little-endian `u64` per chunk, in chunk-index order.
    pub fn encode(&self, enc: &mut Enc) {
        enc.len(self.len());
        for digest in &self.digests {
            enc.u64(*digest);
        }
    }
}
