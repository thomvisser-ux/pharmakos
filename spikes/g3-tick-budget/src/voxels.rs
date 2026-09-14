// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The 32^3 copy-on-write chunk store, and the destruction edits applied to it.
//!
//! Plan §2's `voxels` phase row: *"apply the destruction edits of this tick to
//! the copy-on-write chunk store — the sim half of G1's load"*. The mesher and
//! everything downstream of it are G1's subject and are deliberately absent.
//!
//! **Copy-on-write, concretely.** A chunk is an `Arc<[u8; 32768]>`; a clone of
//! the whole store clones 288 `Arc`s and nothing else, and the first write to a
//! chunk after a clone goes through `Arc::make_mut`, which copies that one
//! chunk and leaves its 287 neighbours shared. That is the structure plan §5's
//! "persistent/copy-on-write structures" row is about, and it is why this spike
//! counts allocations: a store that copied eagerly would put the whole 9.4 MB
//! on the allocator every time anything touched it, and the Windows heap and
//! glibc's malloc would then hand back two different budgets.
//!
//! **Why the state hash sees digests and not voxels.** The canonical encoding
//! of this table is the sequence of per-chunk digests, refreshed for the chunks
//! a crater actually touched. Hashing all 9.4 MB of voxel bytes every tick is
//! about 0.76 ms on the developer machine — some 45x the cost of the whole
//! eight-table hash as built here, and about three times everything the tick
//! does outside `pathing`. [`VoxelStore::whole_store_digest`] exists so that
//! comparison is a measurement the harness publishes rather than a claim in a
//! comment.

use std::sync::Arc;

use crate::hash::{Enc, digest};

/// Chunk edge, in voxels.
pub const CHUNK: usize = 32;
/// Voxels in one chunk.
pub const CHUNK_VOX: usize = CHUNK * CHUNK * CHUNK;
/// Chunks along x and along y: 384 / 32.
pub const CHUNKS_X: usize = 12;
pub const CHUNKS_Y: usize = 12;
/// Chunks along z: 64 / 32.
pub const CHUNKS_Z: usize = 2;
/// Chunks in the store.
pub const CHUNK_COUNT: usize = CHUNKS_X * CHUNKS_Y * CHUNKS_Z;

/// Map extent in voxels, as `usize`, for indexing.
pub const W: usize = CHUNKS_X * CHUNK;
pub const D: usize = CHUNKS_Y * CHUNK;
pub const H: usize = CHUNKS_Z * CHUNK;

/// Air.
pub const AIR: u8 = 0;
/// Rock.
pub const ROCK: u8 = 1;

/// The chunk store.
#[derive(Clone, Debug)]
pub struct VoxelStore {
    chunks: Vec<Arc<[u8; CHUNK_VOX]>>,
    digest: Vec<u64>,
    dirty: Vec<bool>,
    dirty_list: Vec<u32>,
    /// Craters applied since construction. Hashed.
    pub edits: u32,
    /// Voxels turned to air since construction. Hashed.
    pub removed: u32,
    /// Chunk-copy count: how many times `Arc::make_mut` had to copy because the
    /// chunk was shared. Not hashed; reported by the harness.
    pub copies: u64,
}

impl VoxelStore {
    /// A fresh map: rock below a height that varies with position, air above.
    ///
    /// The height field is an integer hash of `(x, y)`, so terrain generation
    /// is a pure function of the constants and adds nothing platform-specific.
    #[must_use]
    pub fn new() -> VoxelStore {
        let mut chunks: Vec<Arc<[u8; CHUNK_VOX]>> = Vec::with_capacity(CHUNK_COUNT);
        for c in 0..CHUNK_COUNT {
            let (bx, by, bz) = chunk_origin(c);
            let mut data = Box::new([AIR; CHUNK_VOX]);
            for lz in 0..CHUNK {
                let z = bz + lz;
                for ly in 0..CHUNK {
                    let y = by + ly;
                    for lx in 0..CHUNK {
                        let x = bx + lx;
                        if z < ground_height(x, y) {
                            data[(lz * CHUNK + ly) * CHUNK + lx] = ROCK;
                        }
                    }
                }
            }
            chunks.push(Arc::from(data));
        }
        let mut s = VoxelStore {
            chunks,
            digest: vec![0; CHUNK_COUNT],
            dirty: vec![true; CHUNK_COUNT],
            dirty_list: (0..CHUNK_COUNT)
                .map(|i| u32::try_from(i).expect("chunk index fits u32"))
                .collect(),
            edits: 0,
            removed: 0,
            copies: 0,
        };
        s.refresh_digests();
        s
    }

    #[must_use]
    pub fn chunks(&self) -> &[Arc<[u8; CHUNK_VOX]>] {
        &self.chunks
    }

    #[must_use]
    pub fn digests(&self) -> &[u64] {
        &self.digest
    }

    #[must_use]
    pub fn get(&self, x: usize, y: usize, z: usize) -> u8 {
        let (c, i) = index_of(x, y, z);
        self.chunks[c][i]
    }

    /// Carve an integer sphere of `radius` voxels centred on `(cx, cy, cz)`.
    ///
    /// Returns the number of voxels turned to air. The same shape G1 and G2 use
    /// (`dx^2 + dy^2 + dz^2 <= r^2`), so the three spikes' destruction streams
    /// are comparable.
    pub fn crater(&mut self, cx: i32, cy: i32, cz: i32, radius: i32) -> u32 {
        let r2 = radius * radius;
        let mut removed: u32 = 0;
        let w = i32::try_from(W).expect("map width fits i32");
        let d = i32::try_from(D).expect("map depth fits i32");
        let h = i32::try_from(H).expect("map height fits i32");
        let mut dz = -radius;
        while dz <= radius {
            let z = cz + dz;
            if z < 0 || z >= h {
                dz += 1;
                continue;
            }
            let mut dy = -radius;
            while dy <= radius {
                let y = cy + dy;
                if y < 0 || y >= d {
                    dy += 1;
                    continue;
                }
                let mut dx = -radius;
                while dx <= radius {
                    let x = cx + dx;
                    if x < 0 || x >= w || dx * dx + dy * dy + dz * dz > r2 {
                        dx += 1;
                        continue;
                    }
                    let (c, i) = index_of(
                        usize::try_from(x).expect("x in range"),
                        usize::try_from(y).expect("y in range"),
                        usize::try_from(z).expect("z in range"),
                    );
                    if self.chunks[c][i] != AIR {
                        let before = Arc::as_ptr(&self.chunks[c]);
                        let data = Arc::make_mut(&mut self.chunks[c]);
                        data[i] = AIR;
                        if !core::ptr::eq(before, Arc::as_ptr(&self.chunks[c])) {
                            self.copies += 1;
                        }
                        removed += 1;
                        if !self.dirty[c] {
                            self.dirty[c] = true;
                            self.dirty_list
                                .push(u32::try_from(c).expect("chunk index fits u32"));
                        }
                    }
                    dx += 1;
                }
                dy += 1;
            }
            dz += 1;
        }
        self.edits = self.edits.saturating_add(1);
        self.removed = self.removed.saturating_add(removed);
        removed
    }

    /// Recompute the digest of every chunk a write touched. Returns how many.
    pub fn refresh_digests(&mut self) -> u32 {
        let mut n: u32 = 0;
        while let Some(c) = self.dirty_list.pop() {
            let i = usize::try_from(c).expect("chunk index");
            self.digest[i] = digest(self.chunks[i].as_slice());
            self.dirty[i] = false;
            n += 1;
        }
        n
    }

    /// The canonical encoding: the version scalars and the per-chunk digests in
    /// chunk-index order.
    pub fn encode(&self, enc: &mut Enc) {
        enc.u32(self.edits);
        enc.u32(self.removed);
        enc.len(self.digest.len());
        for d in &self.digest {
            enc.u64(*d);
        }
    }

    /// Hash every voxel byte in the store. **Not part of the tick**: this is the
    /// measurement that justifies hashing digests instead, taken once by the
    /// harness and reported next to the two hash modes.
    #[must_use]
    pub fn whole_store_digest(&self) -> u64 {
        let mut acc: u64 = 0;
        for c in &self.chunks {
            acc ^= digest(c.as_slice());
        }
        acc
    }

    /// Bytes the store holds, ignoring sharing.
    #[must_use]
    pub fn logical_bytes(&self) -> u64 {
        u64::try_from(CHUNK_COUNT * CHUNK_VOX).expect("store size fits u64")
    }
}

impl Default for VoxelStore {
    fn default() -> VoxelStore {
        VoxelStore::new()
    }
}

/// Chunk index -> its origin voxel.
#[must_use]
pub fn chunk_origin(c: usize) -> (usize, usize, usize) {
    let cx = c % CHUNKS_X;
    let cy = (c / CHUNKS_X) % CHUNKS_Y;
    let cz = c / (CHUNKS_X * CHUNKS_Y);
    (cx * CHUNK, cy * CHUNK, cz * CHUNK)
}

/// Voxel coordinate -> `(chunk index, index within the chunk)`.
#[must_use]
pub fn index_of(x: usize, y: usize, z: usize) -> (usize, usize) {
    debug_assert!(x < W && y < D && z < H);
    let c = (z / CHUNK) * (CHUNKS_X * CHUNKS_Y) + (y / CHUNK) * CHUNKS_X + (x / CHUNK);
    let i = ((z % CHUNK) * CHUNK + (y % CHUNK)) * CHUNK + (x % CHUNK);
    (c, i)
}

/// Rock height at `(x, y)`: a small integer hash, so the map is a pure function
/// of the constants and no seed or table is needed.
#[must_use]
pub fn ground_height(x: usize, y: usize) -> usize {
    let a = u64::try_from(x).expect("x fits u64");
    let b = u64::try_from(y).expect("y fits u64");
    let h = (a.wrapping_mul(0x9E37_79B9)) ^ (b.wrapping_mul(0x85EB_CA6B));
    let h = (h ^ (h >> 13)).wrapping_mul(0xC2B2_AE35);
    // 12 .. 27 voxels of rock: thick enough that a radius-3 crater never punches
    // through, shallow enough that most chunks are mostly air.
    12 + usize::try_from((h >> 17) % 16).expect("height fits usize")
}
