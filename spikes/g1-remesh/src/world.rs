// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later
#![deny(clippy::float_arithmetic)]
//! 12 × 12 × 2 chunk grid = 384 × 384 × 64 voxels (G1 plan §2, spec §9 and §15),
//! plus the **ordered** dirty-chunk set and the flood-fill light field.
//!
//! **Sim side of the wall.** Terrain generation, the voxel edit path, the
//! heightmap, the light flood fill and the dirty set are all integer; floats are
//! denied at the top of this file. The float boundary starts in `greedy.rs`.
//!
//! The dirty set is a `BTreeSet<u32>` of chunk indices, not a `HashSet`:
//! AGENTS.md §4.4/§4.6 — iteration order is part of the observable behaviour,
//! and here it is also what makes the upload queue's tie-break reproducible.

use std::collections::{BTreeSet, VecDeque};

use crate::chunk::{AIR, CHUNK_EDGE, CHUNK_EDGE_I, CHUNK_VOX, Chunk, lindex};

pub const CHUNKS_X: usize = 12;
pub const CHUNKS_Y: usize = 2;
pub const CHUNKS_Z: usize = 12;
pub const CHUNK_COUNT: usize = CHUNKS_X * CHUNKS_Y * CHUNKS_Z; // 288

pub const WORLD_X: i32 = (CHUNKS_X * CHUNK_EDGE) as i32; // 384
pub const WORLD_Y: i32 = (CHUNKS_Y * CHUNK_EDGE) as i32; // 64
pub const WORLD_Z: i32 = (CHUNKS_Z * CHUNK_EDGE) as i32; // 384

/// Material ids. 0 is air (see `chunk::AIR`).
pub const MAT_STONE: u8 = 1;
pub const MAT_DIRT: u8 = 2;
pub const MAT_GRASS: u8 = 3;
pub const MAT_ORE_A: u8 = 4;
pub const MAT_ORE_B: u8 = 5;
pub const MAT_CONCRETE: u8 = 6;
pub const MAT_COUNT: usize = 7;

/// Outside the world in x/z, and below y = 0, the world is closed solid. That
/// stops the mesher emitting four 384 × 64 walls of quads that nothing can see
/// and that would swamp the per-chunk numbers for the 44 border chunks.
const EDGE_MATERIAL: u8 = MAT_STONE;

pub const LIGHT_MAX: u8 = 15;
/// Attenuation per propagation step. 3 gives levels 15/12/9/6/3, and `flood`
/// refuses to propagate out of a cell whose level is `<= LIGHT_ATTEN`, so 3 is
/// terminal and the light travels exactly `LIGHT_REACH` voxels from an opening.
pub const LIGHT_ATTEN: u8 = 3;

/// How far light actually propagates from a fully-lit cell, in voxels.
///
/// **Derived, not asserted:** levels run 15 → 12 → 9 → 6 → 3, and 3 is terminal
/// (`flood` skips any cell with `l <= LIGHT_ATTEN`), so a level-15 cell changes
/// cells up to `LIGHT_MAX / LIGHT_ATTEN - 1 = 4` voxels away and no further.
/// Writing it as arithmetic on the two constants means that changing either one
/// moves the dirty pad with it instead of leaving it silently one short.
pub const LIGHT_REACH: i32 = (LIGHT_MAX / LIGHT_ATTEN) as i32 - 1; // 4

/// How far past the edited box a chunk still counts as dirty.
///
/// The two reasons **compose**, they do not compete: the light around a crater
/// changes out to `LIGHT_REACH` voxels, and the mesher reads the *neighbour*
/// cell one voxel outside its own chunk when it decides whether a boundary face
/// is visible and what shade it gets (`greedy.rs`, `light[ni]`). So a light
/// change at distance `LIGHT_REACH` from the edit still alters the baked colour
/// of a face one voxel further out again: the pad is
/// `LIGHT_REACH + 1`, which also subsumes the +1 the border-neighbour rule
/// needs on its own (G1 plan §2).
pub const DIRTY_PAD: i32 = LIGHT_REACH + 1; // 5

pub const PAD_EDGE: usize = CHUNK_EDGE + 2; // 34
pub const PAD_VOX: usize = PAD_EDGE * PAD_EDGE * PAD_EDGE; // 39_304

#[inline(always)]
pub const fn pidx(x: usize, y: usize, z: usize) -> usize {
    (y * PAD_EDGE + z) * PAD_EDGE + x
}

#[inline(always)]
pub const fn chunk_index(cx: usize, cy: usize, cz: usize) -> usize {
    (cy * CHUNKS_Z + cz) * CHUNKS_X + cx
}

/// Chunk origin in voxels.
#[inline]
pub const fn chunk_origin(ci: usize) -> (i32, i32, i32) {
    let cx = ci % CHUNKS_X;
    let cz = (ci / CHUNKS_X) % CHUNKS_Z;
    let cy = ci / (CHUNKS_X * CHUNKS_Z);
    (
        (cx * CHUNK_EDGE) as i32,
        (cy * CHUNK_EDGE) as i32,
        (cz * CHUNK_EDGE) as i32,
    )
}

// ---------------------------------------------------------------------------
// Integer value noise. No floats: terrain is sim input.
// ---------------------------------------------------------------------------

#[inline]
fn mix(mut h: u64) -> u64 {
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^= h >> 33;
    h
}

#[inline]
fn hash2(seed: u64, x: i32, z: i32) -> u64 {
    let a = u64::from(x as u32);
    let b = u64::from(z as u32);
    mix(seed ^ a.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ b.wrapping_mul(0xc2b2_ae3d_27d4_eb4f))
}

#[inline]
fn hash3(seed: u64, x: i32, y: i32, z: i32) -> u64 {
    let a = u64::from(x as u32);
    let b = u64::from(y as u32);
    let c = u64::from(z as u32);
    mix(seed
        ^ a.wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ b.wrapping_mul(0x85eb_ca6b_1234_5678)
        ^ c.wrapping_mul(0xc2b2_ae3d_27d4_eb4f))
}

const NOISE_UNIT: i64 = 1024;

#[inline]
fn corner(seed: u64, x: i32, z: i32) -> i64 {
    (hash2(seed, x, z) % (NOISE_UNIT as u64)) as i64
}

/// Smoothed integer value noise on a `period`-voxel lattice. Result 0..=1023.
fn noise(seed: u64, x: i32, z: i32, period: i32) -> i64 {
    let x0 = x.div_euclid(period);
    let z0 = z.div_euclid(period);
    let fx = i64::from(x.rem_euclid(period));
    let fz = i64::from(z.rem_euclid(period));
    let p = i64::from(period);

    let tx = fx * NOISE_UNIT / p;
    let tz = fz * NOISE_UNIT / p;
    // Integer smoothstep: 3t² − 2t³ on the 0..1024 scale.
    let sx = (3 * tx * tx * NOISE_UNIT - 2 * tx * tx * tx) / (NOISE_UNIT * NOISE_UNIT);
    let sz = (3 * tz * tz * NOISE_UNIT - 2 * tz * tz * tz) / (NOISE_UNIT * NOISE_UNIT);

    let v00 = corner(seed, x0, z0);
    let v10 = corner(seed, x0 + 1, z0);
    let v01 = corner(seed, x0, z0 + 1);
    let v11 = corner(seed, x0 + 1, z0 + 1);

    let a = v00 + (v10 - v00) * sx / NOISE_UNIT;
    let b = v01 + (v11 - v01) * sx / NOISE_UNIT;
    a + (b - a) * sz / NOISE_UNIT
}

// ---------------------------------------------------------------------------

pub struct World {
    chunks: Vec<Chunk>,
    /// Flood-fill light, 0..=15, one byte per voxel, same layout as the chunk.
    light: Vec<Vec<u8>>,
    /// Topmost solid voxel + 1 per column; 0 means the column is empty.
    height: Vec<u8>,
    dirty: BTreeSet<u32>,
    seed: u64,
}

impl World {
    pub fn new_empty(seed: u64) -> Self {
        Self {
            chunks: vec![Chunk::new_air(); CHUNK_COUNT],
            light: vec![vec![0u8; CHUNK_VOX]; CHUNK_COUNT],
            height: vec![0u8; (WORLD_X * WORLD_Z) as usize],
            dirty: BTreeSet::new(),
            seed,
        }
    }

    /// Terrain: a 20–49 voxel heightfield with grass/dirt/stone banding, two ore
    /// seams and 24 deterministic concrete bunkers on the surface. Chosen so a
    /// realistic fraction of the 288 chunks is non-empty and the top chunk layer
    /// carries most of the visible geometry.
    pub fn generate(seed: u64) -> Self {
        let mut w = Self::new_empty(seed);
        for z in 0..WORLD_Z {
            for x in 0..WORLD_X {
                let n = noise(seed, x, z, 256) * 18
                    + noise(seed ^ 0xa1, x, z, 64) * 8
                    + noise(seed ^ 0xb2, x, z, 16) * 4;
                // n is 0..=30_690 (three octaves weighted 18/8/4); /1024 maps it
                // onto a 0..29 rise above a floor of 20, i.e. heights 20..=49.
                let h = (20 + i32::try_from(n / 1024).unwrap_or(0)).clamp(20, 49);
                for y in 0..h {
                    let m = if y == h - 1 {
                        MAT_GRASS
                    } else if y >= h - 4 {
                        MAT_DIRT
                    } else if hash3(seed ^ 0xc3, x >> 2, y >> 2, z >> 2).is_multiple_of(23) {
                        MAT_ORE_A
                    } else if hash3(seed ^ 0xd4, x >> 2, y >> 2, z >> 2) % 37 == 1 {
                        MAT_ORE_B
                    } else {
                        MAT_STONE
                    };
                    w.set_voxel(x, y, z, m);
                }
            }
        }

        // A few structures: hollow concrete bunkers, so the vista has vertical
        // faces and interior surfaces rather than only a hillside.
        for s in 0..24i32 {
            let hx = hash2(seed ^ 0xe5, s, 0);
            let hz = hash2(seed ^ 0xf6, s, 1);
            let bx = 40 + i32::try_from(hx % 300).unwrap_or(0);
            let bz = 40 + i32::try_from(hz % 300).unwrap_or(0);
            let bw = 6 + i32::try_from(hx % 7).unwrap_or(0);
            let bd = 6 + i32::try_from(hz % 7).unwrap_or(0);
            let bh = 7 + i32::try_from((hx >> 8) % 6).unwrap_or(0);
            let mut base = 0i32;
            for z in bz..bz + bd {
                for x in bx..bx + bw {
                    base = base.max(i32::from(w.height_at(x, z)));
                }
            }
            for y in base..(base + bh).min(WORLD_Y) {
                for z in bz..bz + bd {
                    for x in bx..bx + bw {
                        let shell = x == bx
                            || x == bx + bw - 1
                            || z == bz
                            || z == bz + bd - 1
                            || y == base + bh - 1;
                        if shell {
                            w.set_voxel(x, y, z, MAT_CONCRETE);
                        }
                    }
                }
            }
        }

        w.relight_all();
        w.dirty.clear();
        w
    }

    #[inline]
    pub fn seed(&self) -> u64 {
        self.seed
    }

    #[inline]
    pub fn chunk(&self, ci: usize) -> &Chunk {
        &self.chunks[ci]
    }

    #[inline]
    fn col(x: i32, z: i32) -> usize {
        (z * WORLD_X + x) as usize
    }

    #[inline]
    pub fn height_at(&self, x: i32, z: i32) -> u8 {
        if !(0..WORLD_X).contains(&x) || !(0..WORLD_Z).contains(&z) {
            return 0;
        }
        self.height[Self::col(x, z)]
    }

    #[inline]
    pub fn in_bounds(x: i32, y: i32, z: i32) -> bool {
        (0..WORLD_X).contains(&x) && (0..WORLD_Y).contains(&y) && (0..WORLD_Z).contains(&z)
    }

    /// Material at a world voxel. Outside the world: closed solid in x/z and
    /// below, open air above.
    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> u8 {
        if y >= WORLD_Y {
            return AIR;
        }
        if y < 0 || !(0..WORLD_X).contains(&x) || !(0..WORLD_Z).contains(&z) {
            return EDGE_MATERIAL;
        }
        let ci = chunk_index(
            (x / CHUNK_EDGE_I) as usize,
            (y / CHUNK_EDGE_I) as usize,
            (z / CHUNK_EDGE_I) as usize,
        );
        self.chunks[ci].get(
            (x % CHUNK_EDGE_I) as usize,
            (y % CHUNK_EDGE_I) as usize,
            (z % CHUNK_EDGE_I) as usize,
        )
    }

    #[inline]
    pub fn light_at(&self, x: i32, y: i32, z: i32) -> u8 {
        if y >= WORLD_Y {
            return LIGHT_MAX;
        }
        if !Self::in_bounds(x, y, z) {
            return 0;
        }
        let ci = chunk_index(
            (x / CHUNK_EDGE_I) as usize,
            (y / CHUNK_EDGE_I) as usize,
            (z / CHUNK_EDGE_I) as usize,
        );
        self.light[ci][lindex(
            (x % CHUNK_EDGE_I) as usize,
            (y % CHUNK_EDGE_I) as usize,
            (z % CHUNK_EDGE_I) as usize,
        )]
    }

    #[inline]
    fn set_light(&mut self, x: i32, y: i32, z: i32, l: u8) {
        let ci = chunk_index(
            (x / CHUNK_EDGE_I) as usize,
            (y / CHUNK_EDGE_I) as usize,
            (z / CHUNK_EDGE_I) as usize,
        );
        self.light[ci][lindex(
            (x % CHUNK_EDGE_I) as usize,
            (y % CHUNK_EDGE_I) as usize,
            (z % CHUNK_EDGE_I) as usize,
        )] = l;
    }

    /// Write one voxel. Returns `true` if it changed. Keeps the heightmap in
    /// sync but does **not** mark dirty or re-light: `explode.rs` batches both.
    pub fn set_voxel(&mut self, x: i32, y: i32, z: i32, m: u8) -> bool {
        if !Self::in_bounds(x, y, z) {
            return false;
        }
        let ci = chunk_index(
            (x / CHUNK_EDGE_I) as usize,
            (y / CHUNK_EDGE_I) as usize,
            (z / CHUNK_EDGE_I) as usize,
        );
        let changed = self.chunks[ci].set(
            (x % CHUNK_EDGE_I) as usize,
            (y % CHUNK_EDGE_I) as usize,
            (z % CHUNK_EDGE_I) as usize,
            m,
        );
        if changed && m != AIR {
            let c = Self::col(x, z);
            let top = u8::try_from(y + 1).unwrap_or(u8::MAX);
            if top > self.height[c] {
                self.height[c] = top;
            }
        }
        changed
    }

    /// Re-derive the heightmap for every column in an inclusive x/z box.
    pub fn recompute_heights(&mut self, x0: i32, z0: i32, x1: i32, z1: i32) {
        let x0 = x0.max(0);
        let z0 = z0.max(0);
        let x1 = x1.min(WORLD_X - 1);
        let z1 = z1.min(WORLD_Z - 1);
        for z in z0..=z1 {
            for x in x0..=x1 {
                let mut top = 0u8;
                for y in (0..WORLD_Y).rev() {
                    if self.get(x, y, z) != AIR {
                        top = u8::try_from(y + 1).unwrap_or(u8::MAX);
                        break;
                    }
                }
                let c = Self::col(x, z);
                self.height[c] = top;
            }
        }
    }

    // -- light ------------------------------------------------------------

    /// Tallest of the four horizontal neighbours of column `(x, z)`. Outside
    /// the world the column is closed solid, so it contributes nothing.
    #[inline]
    fn neighbour_height(&self, x: i32, z: i32) -> i32 {
        let mut m = 0i32;
        for (dx, dz) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
            m = m.max(i32::from(self.height_at(x + dx, z + dz)));
        }
        m
    }

    fn relight_all(&mut self) {
        for l in &mut self.light {
            l.fill(0);
        }
        let mut q: VecDeque<(i32, i32, i32)> = VecDeque::with_capacity(1 << 16);
        for z in 0..WORLD_Z {
            for x in 0..WORLD_X {
                let h = i32::from(self.height[Self::col(x, z)]);
                for y in h..WORLD_Y {
                    self.set_light(x, y, z, LIGHT_MAX);
                }
                // EVERY sky cell is a seed, not just the lowest one. Seeding
                // only `(x, h, z)` leaves any air cell whose sole lit neighbour
                // is a sky cell *above the adjacent column's floor* at light 0 —
                // a tunnel mouth in a cliff face, a breached bunker, anything
                // under an overhang renders pitch black.
                //
                // Seeding all of them is 6.5 M queue entries on this world, so
                // seed exactly the ones that can do any work instead. A sky cell
                // at `(x, y, z)` can only lift a neighbour that is *not* itself
                // sky. Its vertical neighbours never qualify (y+1 is sky; y-1 is
                // below sky only when y == h, and that cell is the topmost solid
                // voxel by definition of `height`), so the condition reduces to
                // "some horizontal neighbour column is taller than y" — i.e.
                // `y < neighbour_height`. That is exactly equivalent to seeding
                // every sky cell, and costs a handful of entries per column.
                let hn = self.neighbour_height(x, z).min(WORLD_Y);
                for y in h..hn {
                    q.push_back((x, y, z));
                }
            }
        }
        self.flood(&mut q, (0, 0, 0), (WORLD_X - 1, WORLD_Y - 1, WORLD_Z - 1));
    }

    /// Re-run the flood fill inside an inclusive box, seeded from the sky and
    /// from the box's outer shell so the result joins up with the light outside.
    pub fn relight_box(&mut self, lo: (i32, i32, i32), hi: (i32, i32, i32)) {
        let lo = (lo.0.max(0), lo.1.max(0), lo.2.max(0));
        let hi = (
            hi.0.min(WORLD_X - 1),
            hi.1.min(WORLD_Y - 1),
            hi.2.min(WORLD_Z - 1),
        );
        if lo.0 > hi.0 || lo.1 > hi.1 || lo.2 > hi.2 {
            return;
        }
        for z in lo.2..=hi.2 {
            for x in lo.0..=hi.0 {
                for y in lo.1..=hi.1 {
                    self.set_light(x, y, z, 0);
                }
            }
        }
        let mut q: VecDeque<(i32, i32, i32)> = VecDeque::with_capacity(4096);
        // Sky seeds inside the box. Every sky cell is a seed, for the reason
        // spelled out in `relight_all`: seeding only the lowest one leaves a
        // tunnel mouth or a breached bunker unlit. The box is small (an r=4
        // crater plus the light pad is 19³), so this one does not need
        // `relight_all`'s neighbour-height trick to stay cheap.
        for z in lo.2..=hi.2 {
            for x in lo.0..=hi.0 {
                let h = i32::from(self.height[Self::col(x, z)]);
                for y in h.max(lo.1)..=hi.1 {
                    self.set_light(x, y, z, LIGHT_MAX);
                    q.push_back((x, y, z));
                }
            }
        }
        // Shell seeds: cells one voxel outside the box keep their light and
        // push inwards.
        for z in lo.2 - 1..=hi.2 + 1 {
            for y in lo.1 - 1..=hi.1 + 1 {
                for x in lo.0 - 1..=hi.0 + 1 {
                    let inside =
                        x >= lo.0 && x <= hi.0 && y >= lo.1 && y <= hi.1 && z >= lo.2 && z <= hi.2;
                    if inside || !Self::in_bounds(x, y, z) {
                        continue;
                    }
                    if self.get(x, y, z) == AIR && self.light_at(x, y, z) > LIGHT_ATTEN {
                        q.push_back((x, y, z));
                    }
                }
            }
        }
        self.flood(&mut q, lo, hi);
    }

    /// BFS light propagation. Only writes inside the inclusive `lo..=hi` box.
    fn flood(
        &mut self,
        q: &mut VecDeque<(i32, i32, i32)>,
        lo: (i32, i32, i32),
        hi: (i32, i32, i32),
    ) {
        const N6: [(i32, i32, i32); 6] = [
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
        ];
        while let Some((x, y, z)) = q.pop_front() {
            let l = self.light_at(x, y, z);
            if l <= LIGHT_ATTEN {
                continue;
            }
            let nl = l - LIGHT_ATTEN;
            for (dx, dy, dz) in N6 {
                let (nx, ny, nz) = (x + dx, y + dy, z + dz);
                if nx < lo.0 || nx > hi.0 || ny < lo.1 || ny > hi.1 || nz < lo.2 || nz > hi.2 {
                    continue;
                }
                if !Self::in_bounds(nx, ny, nz) || self.get(nx, ny, nz) != AIR {
                    continue;
                }
                if self.light_at(nx, ny, nz) >= nl {
                    continue;
                }
                self.set_light(nx, ny, nz, nl);
                q.push_back((nx, ny, nz));
            }
        }
    }

    // -- dirty set --------------------------------------------------------

    /// Mark every chunk overlapping the inclusive box padded by `DIRTY_PAD`.
    /// Returns the newly-marked-or-already-marked set in chunk-index order.
    pub fn mark_dirty_box(&mut self, lo: (i32, i32, i32), hi: (i32, i32, i32)) -> Vec<u32> {
        let lo = (lo.0 - DIRTY_PAD, lo.1 - DIRTY_PAD, lo.2 - DIRTY_PAD);
        let hi = (hi.0 + DIRTY_PAD, hi.1 + DIRTY_PAD, hi.2 + DIRTY_PAD);
        let cx0 = (lo.0.max(0) / CHUNK_EDGE_I) as usize;
        let cy0 = (lo.1.max(0) / CHUNK_EDGE_I) as usize;
        let cz0 = (lo.2.max(0) / CHUNK_EDGE_I) as usize;
        let cx1 = (hi.0.min(WORLD_X - 1) / CHUNK_EDGE_I) as usize;
        let cy1 = (hi.1.min(WORLD_Y - 1) / CHUNK_EDGE_I) as usize;
        let cz1 = (hi.2.min(WORLD_Z - 1) / CHUNK_EDGE_I) as usize;

        let mut out = Vec::new();
        for cy in cy0..=cy1 {
            for cz in cz0..=cz1 {
                for cx in cx0..=cx1 {
                    let ci = u32::try_from(chunk_index(cx, cy, cz)).unwrap_or(0);
                    self.dirty.insert(ci);
                    out.push(ci);
                }
            }
        }
        out.sort_unstable();
        out
    }

    #[inline]
    pub fn dirty_len(&self) -> usize {
        self.dirty.len()
    }

    /// Drain the ordered dirty set.
    pub fn take_dirty(&mut self) -> Vec<u32> {
        let v: Vec<u32> = self.dirty.iter().copied().collect();
        self.dirty.clear();
        v
    }

    /// Chunk indices that can produce geometry, in index order.
    pub fn nonempty_chunks(&self) -> Vec<u32> {
        (0..CHUNK_COUNT)
            .filter(|&ci| !self.chunks[ci].is_empty())
            .map(|ci| u32::try_from(ci).unwrap_or(0))
            .collect()
    }

    pub fn solid_voxels(&self) -> u64 {
        self.chunks.iter().map(|c| u64::from(c.solid_count())).sum()
    }

    // -- padded extraction for the mesher ---------------------------------

    /// Copy chunk `ci` plus a one-voxel border of its six neighbours into the
    /// mesher's 34³ scratch buffers. Interior rows are 32-byte `memcpy`s (the
    /// chunk layout is x-contiguous); only the 6 536 border cells are fetched
    /// one at a time.
    pub fn fill_padded(&self, ci: usize, mat: &mut [u8], light: &mut [u8]) {
        debug_assert_eq!(mat.len(), PAD_VOX);
        debug_assert_eq!(light.len(), PAD_VOX);
        let (ox, oy, oz) = chunk_origin(ci);
        let cx = (ox / CHUNK_EDGE_I) as usize;

        for py in 0..PAD_EDGE {
            let wy = oy + i32::try_from(py).unwrap_or(0) - 1;
            for pz in 0..PAD_EDGE {
                let wz = oz + i32::try_from(pz).unwrap_or(0) - 1;
                let base = pidx(0, py, pz);

                // x = -1 and x = 32 come from the ±x neighbour chunks.
                mat[base] = self.get(ox - 1, wy, wz);
                light[base] = self.light_at(ox - 1, wy, wz);
                mat[base + PAD_EDGE - 1] = self.get(ox + CHUNK_EDGE_I, wy, wz);
                light[base + PAD_EDGE - 1] = self.light_at(ox + CHUNK_EDGE_I, wy, wz);

                let inner = base + 1..base + 1 + CHUNK_EDGE;
                if !(0..WORLD_Y).contains(&wy) || !(0..WORLD_Z).contains(&wz) {
                    let (m, l) = if wy >= WORLD_Y {
                        (AIR, LIGHT_MAX)
                    } else {
                        (EDGE_MATERIAL, 0)
                    };
                    mat[inner.clone()].fill(m);
                    light[inner].fill(l);
                    continue;
                }
                let cy = (wy / CHUNK_EDGE_I) as usize;
                let cz = (wz / CHUNK_EDGE_I) as usize;
                let nci = chunk_index(cx, cy, cz);
                let ly = (wy % CHUNK_EDGE_I) as usize;
                let lz = (wz % CHUNK_EDGE_I) as usize;
                match self.chunks[nci].row(ly, lz) {
                    Some(r) => mat[inner.clone()].copy_from_slice(r),
                    None => mat[inner.clone()].fill(AIR),
                }
                let li = lindex(0, ly, lz);
                light[inner].copy_from_slice(&self.light[nci][li..li + CHUNK_EDGE]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_is_288_chunks_and_indices_round_trip() {
        assert_eq!(CHUNK_COUNT, 288);
        assert_eq!((WORLD_X, WORLD_Y, WORLD_Z), (384, 64, 384));
        for ci in 0..CHUNK_COUNT {
            let (ox, oy, oz) = chunk_origin(ci);
            let back = chunk_index((ox / 32) as usize, (oy / 32) as usize, (oz / 32) as usize);
            assert_eq!(back, ci);
        }
    }

    #[test]
    fn padded_copy_matches_pointwise_reads() {
        let w = World::generate(7);
        for &ci in &[0usize, 145, 287] {
            let mut m = vec![0u8; PAD_VOX];
            let mut l = vec![0u8; PAD_VOX];
            w.fill_padded(ci, &mut m, &mut l);
            let (ox, oy, oz) = chunk_origin(ci);
            for py in 0..PAD_EDGE {
                for pz in 0..PAD_EDGE {
                    for px in 0..PAD_EDGE {
                        let (x, y, z) =
                            (ox + px as i32 - 1, oy + py as i32 - 1, oz + pz as i32 - 1);
                        assert_eq!(m[pidx(px, py, pz)], w.get(x, y, z), "mat {x},{y},{z}");
                        assert_eq!(
                            l[pidx(px, py, pz)],
                            w.light_at(x, y, z),
                            "light {x},{y},{z}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn terrain_leaves_a_realistic_fraction_non_empty() {
        let w = World::generate(1);
        let n = w.nonempty_chunks().len();
        assert!(n > 150 && n <= CHUNK_COUNT, "non-empty chunks = {n}");
        for z in (0..WORLD_Z).step_by(37) {
            for x in (0..WORLD_X).step_by(41) {
                let h = i32::from(w.height_at(x, z));
                assert!((20..=WORLD_Y).contains(&h), "height {h} at {x},{z}");
            }
        }
    }

    /// The flood fill has to seed *every* sky cell, not just the lowest one of
    /// each column. A cliff with a tunnel bored into its face is the cheapest
    /// counter-example: the tunnel's only lit neighbour is a sky cell partway up
    /// the shorter column, which a lowest-cell-only seeding never pops.
    #[test]
    fn a_tunnel_under_an_overhang_is_lit_from_the_sky_beside_it() {
        let mut w = World::new_empty(0);
        for z in 190..210 {
            for x in 0..WORLD_X {
                let h = if x < 200 { 10 } else { 40 };
                for y in 0..h {
                    w.set_voxel(x, y, z, MAT_STONE);
                }
            }
        }
        // Bore a 1-voxel tunnel at y = 25 into the tall half.
        for x in 200..213 {
            w.set_voxel(x, 25, 200, AIR);
        }
        w.recompute_heights(0, 190, WORLD_X - 1, 209);
        w.relight_all();

        // The sky cell beside the cliff is the origin; the tunnel is lit from it.
        assert_eq!(w.light_at(199, 25, 200), LIGHT_MAX, "sky beside the cliff");
        for step in 1..=LIGHT_REACH {
            let x = 199 + step;
            let want = LIGHT_MAX - LIGHT_ATTEN * u8::try_from(step).unwrap();
            assert_eq!(
                w.light_at(x, 25, 200),
                want,
                "tunnel cell x={x} ({step} voxels in) should be {want}"
            );
        }
        // LIGHT_REACH is the last voxel that changes; one further is dark.
        assert_eq!(w.light_at(199 + LIGHT_REACH + 1, 25, 200), 0);
    }

    /// `DIRTY_PAD` is `LIGHT_REACH + 1` because the mesher reads the neighbour
    /// cell one voxel *outside* the chunk. An edit whose light change lands
    /// exactly on the last voxel of a chunk must still dirty the chunk on the
    /// far side of that boundary.
    #[test]
    fn an_edit_whose_light_reaches_a_chunk_boundary_dirties_the_far_side() {
        let mut w = World::new_empty(0);
        // x = 27 changes light out to x = 27 + LIGHT_REACH = 31, the last voxel
        // of chunk column 0; chunk column 1's -x boundary faces read it.
        let x = CHUNK_EDGE_I - 1 - LIGHT_REACH;
        let marked = w.mark_dirty_box((x, 40, 40), (x, 40, 40));
        let far = u32::try_from(chunk_index(1, 1, 1)).unwrap();
        assert!(
            marked.contains(&far),
            "pad {DIRTY_PAD} did not reach chunk column 1 from x={x}: {marked:?}"
        );
    }

    #[test]
    fn generate_is_deterministic() {
        let a = World::generate(42);
        let b = World::generate(42);
        for z in (0..WORLD_Z).step_by(13) {
            for x in (0..WORLD_X).step_by(11) {
                assert_eq!(a.height_at(x, z), b.height_at(x, z));
                for y in (0..WORLD_Y).step_by(7) {
                    assert_eq!(a.get(x, y, z), b.get(x, y, z));
                    assert_eq!(a.light_at(x, y, z), b.light_at(x, y, z));
                }
            }
        }
    }
}
