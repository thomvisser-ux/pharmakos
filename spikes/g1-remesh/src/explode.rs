// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later
#![deny(clippy::float_arithmetic)]
//! Deterministic integer sphere edit. Returns the ordered dirty chunk list.
//!
//! **Sim side of the wall.** This is the voxel-edit *input* path: which voxels
//! an explosion removes is integer, deterministic, and identical on every
//! machine (G1 plan §2 "Determinism note"). The membership test is `dx² + dy² +
//! dz² <= r²` — a squared comparison, never a square root — which is exactly the
//! form AGENTS.md §4.2 prescribes. `#![deny(clippy::float_arithmetic)]` at the
//! top of the file is the wall, in the place the wall belongs.

use crate::chunk::AIR;
use crate::world::{LIGHT_REACH, World};

/// A sphere of voxels to remove, in world voxel coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Explosion {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub radius: i32,
}

/// What one explosion did.
#[derive(Clone, Debug, Default)]
pub struct Blast {
    /// Voxels actually turned to air (already-air voxels do not count).
    pub removed: u32,
    /// Chunks whose surface is now stale, in chunk-index order, deduplicated.
    pub dirty: Vec<u32>,
}

/// Apply one explosion. Carves the sphere, repairs the heightmap, re-runs the
/// light flood fill over the affected box, and marks the dirty chunks.
pub fn explode(world: &mut World, e: &Explosion) -> Blast {
    let r = e.radius.max(0);
    let r2 = r * r;
    let mut removed = 0u32;

    for dy in -r..=r {
        let y = e.y + dy;
        for dz in -r..=r {
            let z = e.z + dz;
            for dx in -r..=r {
                if dx * dx + dy * dy + dz * dz > r2 {
                    continue;
                }
                if world.set_voxel(e.x + dx, y, z, AIR) {
                    removed += 1;
                }
            }
        }
    }

    let lo = (e.x - r, e.y - r, e.z - r);
    let hi = (e.x + r, e.y + r, e.z + r);

    world.recompute_heights(lo.0, lo.2, hi.0, hi.2);
    world.relight_box(
        (lo.0 - LIGHT_REACH, lo.1 - LIGHT_REACH, lo.2 - LIGHT_REACH),
        (hi.0 + LIGHT_REACH, hi.1 + LIGHT_REACH, hi.2 + LIGHT_REACH),
    );
    let dirty = world.mark_dirty_box(lo, hi);

    Blast { removed, dirty }
}

/// Count the dirty chunks an explosion *would* produce without editing the
/// world. Used by `meshbench`'s fan-out step, which needs 1 000 placements per
/// radius and must not mutate the terrain between them.
pub fn dirty_fanout(e: &Explosion) -> usize {
    use crate::chunk::CHUNK_EDGE_I;
    use crate::world::{CHUNKS_X, CHUNKS_Y, CHUNKS_Z, DIRTY_PAD, WORLD_X, WORLD_Y, WORLD_Z};
    let r = e.radius.max(0) + DIRTY_PAD;
    let cx0 = ((e.x - r).max(0) / CHUNK_EDGE_I) as usize;
    let cy0 = ((e.y - r).max(0) / CHUNK_EDGE_I) as usize;
    let cz0 = ((e.z - r).max(0) / CHUNK_EDGE_I) as usize;
    let cx1 = ((e.x + r).min(WORLD_X - 1) / CHUNK_EDGE_I) as usize;
    let cy1 = ((e.y + r).min(WORLD_Y - 1) / CHUNK_EDGE_I) as usize;
    let cz1 = ((e.z + r).min(WORLD_Z - 1) / CHUNK_EDGE_I) as usize;
    if cx1 >= CHUNKS_X || cy1 >= CHUNKS_Y || cz1 >= CHUNKS_Z {
        return 0;
    }
    (cx1 + 1 - cx0) * (cy1 + 1 - cy0) * (cz1 + 1 - cz0)
}

/// The deterministic explosion schedule the in-engine run and the headless
/// pipeline proxy both use, so the two are comparing the same load.
///
/// `n` is the explosion ordinal (0, 1, 2, …). Positions walk the middle of the
/// map with a 64-bit LCG and land on the terrain surface, which is where a
/// crater actually costs something.
pub struct Schedule {
    seed: u64,
    pub radius: i32,
}

impl Schedule {
    pub const fn new(seed: u64, radius: i32) -> Self {
        Self { seed, radius }
    }

    pub fn nth(&self, world: &World, n: u64) -> Explosion {
        let mut s = self
            .seed
            .wrapping_add(n.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        s ^= s >> 30;
        s = s.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        s ^= s >> 27;
        s = s.wrapping_mul(0x94d0_49bb_1331_11eb);
        s ^= s >> 31;
        // The 96..288 band is the field the fixed vista camera looks at.
        let x = 96 + i32::try_from(s % 192).unwrap_or(0);
        let z = 96 + i32::try_from((s >> 20) % 192).unwrap_or(0);
        let h = i32::from(world.height_at(x, z));
        let y = (h - 1 - i32::try_from((s >> 40) % 3).unwrap_or(0)).clamp(1, 62);
        Explosion {
            x,
            y,
            z,
            radius: self.radius,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{DIRTY_PAD, World};

    #[test]
    fn sphere_is_integer_and_symmetric() {
        let mut w = World::new_empty(0);
        for dy in -8..=8 {
            for dz in -8..=8 {
                for dx in -8..=8 {
                    w.set_voxel(200 + dx, 40 + dy, 200 + dz, 1);
                }
            }
        }
        let b = explode(
            &mut w,
            &Explosion {
                x: 200,
                y: 40,
                z: 200,
                radius: 4,
            },
        );
        // r=4 integer ball: 257 lattice points with dx²+dy²+dz² <= 16.
        assert_eq!(b.removed, 257);
        assert_eq!(w.get(200, 40, 200), 0);
        assert_eq!(w.get(204, 40, 200), 0);
        assert_ne!(w.get(205, 40, 200), 0);
    }

    #[test]
    fn explosion_is_deterministic() {
        let e = Explosion {
            x: 101,
            y: 33,
            z: 77,
            radius: 6,
        };
        let mut a = World::generate(9);
        let mut b = World::generate(9);
        let ba = explode(&mut a, &e);
        let bb = explode(&mut b, &e);
        assert_eq!(ba.removed, bb.removed);
        assert_eq!(ba.dirty, bb.dirty);
    }

    #[test]
    fn dirty_list_is_sorted_and_unique() {
        let mut w = World::generate(2);
        let b = explode(
            &mut w,
            &Explosion {
                x: 128,
                y: 32,
                z: 128,
                radius: 8,
            },
        );
        assert!(b.dirty.windows(2).all(|p| p[0] < p[1]));
        assert!(!b.dirty.is_empty());
    }

    #[test]
    fn a_corner_hit_fans_out_to_eight_chunks() {
        // Chunk corner at (128, 32, 128): the 2x2x2 chunk neighbourhood.
        let e = Explosion {
            x: 128,
            y: 32,
            z: 128,
            radius: 4,
        };
        assert_eq!(dirty_fanout(&e), 8);
        // Well inside a chunk with radius 2 and a pad of DIRTY_PAD: one chunk.
        let e2 = Explosion {
            x: 144,
            y: 48,
            z: 144,
            radius: 2,
        };
        const { assert!(DIRTY_PAD < 8) };
        assert_eq!(dirty_fanout(&e2), 1);
    }
}
