// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later
#![deny(clippy::float_arithmetic)]
//! 32³ copy-on-write chunk with one `u8` material id per voxel.
//!
//! **Sim side of the wall.** Voxel storage is the *input* to the mesher and
//! belongs to the deterministic half (AGENTS.md §4.9, G1 plan §2 "Determinism
//! note"): integers only, denied at the top of this file.
//!
//! Copy-on-write: the voxel block lives behind an `Arc`, so cloning a `Chunk`
//! (for a snapshot, a fork, or a rollback) is one atomic increment and the
//! 32 KiB block is only duplicated when somebody writes to a shared copy.
//! An all-air chunk stores no block at all.

use std::sync::Arc;

pub const CHUNK_EDGE: usize = 32;
pub const CHUNK_EDGE_I: i32 = 32;
pub const CHUNK_VOX: usize = CHUNK_EDGE * CHUNK_EDGE * CHUNK_EDGE; // 32_768

/// Material id 0 is air. Everything else is solid.
pub const AIR: u8 = 0;

/// Linear index inside a chunk. `x` is contiguous, then `z`, then `y`: a whole
/// 32-voxel row along x is one `memcpy`, which the mesher's padded-copy step
/// depends on.
#[inline(always)]
pub const fn lindex(x: usize, y: usize, z: usize) -> usize {
    (y << 10) | (z << 5) | x
}

#[derive(Clone)]
pub struct VoxelBlock(pub [u8; CHUNK_VOX]);

impl VoxelBlock {
    #[inline]
    fn air() -> Self {
        Self([AIR; CHUNK_VOX])
    }
}

/// One 32³ chunk. `None` data means "entirely air" and costs nothing.
#[derive(Clone, Default)]
pub struct Chunk {
    data: Option<Arc<VoxelBlock>>,
    solid: u32,
}

impl Chunk {
    #[inline]
    pub fn new_air() -> Self {
        Self {
            data: None,
            solid: 0,
        }
    }

    /// Voxels that are not air. 0 means the chunk can be skipped entirely.
    #[inline]
    pub fn solid_count(&self) -> u32 {
        self.solid
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.solid == 0
    }

    /// `true` when every voxel is solid — such a chunk has no interior faces,
    /// only whatever its neighbours expose.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.solid as usize == CHUNK_VOX
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize, z: usize) -> u8 {
        match &self.data {
            Some(b) => b.0[lindex(x, y, z)],
            None => AIR,
        }
    }

    /// Raw row of 32 voxels starting at `(0, y, z)`. `None` for an air chunk.
    #[inline]
    pub fn row(&self, y: usize, z: usize) -> Option<&[u8]> {
        self.data.as_ref().map(|b| {
            let i = lindex(0, y, z);
            &b.0[i..i + CHUNK_EDGE]
        })
    }

    /// The copy-on-write write barrier: materialises the block if the chunk was
    /// air, and deep-copies it if any other handle still shares it.
    #[inline]
    fn block_mut(&mut self) -> &mut VoxelBlock {
        let arc = self.data.get_or_insert_with(|| Arc::new(VoxelBlock::air()));
        Arc::make_mut(arc)
    }

    /// Returns `true` if the voxel actually changed.
    #[inline]
    pub fn set(&mut self, x: usize, y: usize, z: usize, m: u8) -> bool {
        let i = lindex(x, y, z);
        let old = match &self.data {
            Some(b) => b.0[i],
            None => AIR,
        };
        if old == m {
            return false;
        }
        if old == AIR {
            self.solid += 1;
        } else if m == AIR {
            self.solid -= 1;
        }
        self.block_mut().0[i] = m;
        if self.solid == 0 {
            self.data = None;
        }
        true
    }

    /// How many handles share this chunk's block (1 = unshared, 0 = air).
    /// Only used by the copy-on-write assertion in the tests.
    #[cfg(test)]
    pub fn share_count(&self) -> usize {
        self.data.as_ref().map_or(0, Arc::strong_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn air_chunk_costs_nothing_and_cow_defers_the_copy() {
        let mut a = Chunk::new_air();
        assert!(a.is_empty());
        assert_eq!(a.share_count(), 0);

        assert!(a.set(1, 2, 3, 7));
        assert_eq!(a.get(1, 2, 3), 7);
        assert_eq!(a.solid_count(), 1);
        assert_eq!(a.share_count(), 1);

        // Snapshot: no copy yet.
        let snap = a.clone();
        assert_eq!(a.share_count(), 2);

        // Writing to `a` un-shares it; the snapshot keeps the old voxel.
        assert!(a.set(1, 2, 3, 9));
        assert_eq!(a.share_count(), 1);
        assert_eq!(snap.get(1, 2, 3), 7);
        assert_eq!(a.get(1, 2, 3), 9);
    }

    #[test]
    fn clearing_the_last_voxel_releases_the_block() {
        let mut a = Chunk::new_air();
        a.set(0, 0, 0, 4);
        a.set(0, 0, 0, AIR);
        assert!(a.is_empty());
        assert_eq!(a.share_count(), 0);
    }

    #[test]
    fn rows_are_contiguous_in_x() {
        let mut a = Chunk::new_air();
        for x in 0..CHUNK_EDGE {
            a.set(x, 5, 6, u8::try_from(x + 1).unwrap());
        }
        let r = a.row(5, 6).unwrap();
        assert_eq!(r.len(), CHUNK_EDGE);
        assert_eq!(r[0], 1);
        assert_eq!(r[31], 32);
    }
}
