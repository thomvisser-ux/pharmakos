// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later
#![deny(clippy::float_arithmetic, clippy::as_conversions)]
//! Terrain edits during the Push, and the incremental rebuild they force.
//!
//! Plan §2: *"Terrain edits during the Push are the interesting part … The bench
//! replays a destruction stream (the same explosion generator shape as G1, at 20
//! explosions/s) that invalidates surface cells, and forces the affected
//! clusters to be repaired and the units crossing them to repath."*
//!
//! # The crater is G1's integer sphere, projected onto a heightmap
//!
//! [`Crater`] uses exactly the membership test from the G1 spike's
//! `src/explode.rs` — `dx² + dy² + dz² <= r²`, a squared comparison and never a
//! square root, which is the form `AGENTS.md` §4.2 prescribes. Copying between
//! spikes is what `spikes/README.md` rule 2 allows, and using the same shape is
//! what lets the two spikes' destruction streams be compared.
//!
//! Because [`crate::world::World`] is a heightmap (see its module docs), the
//! sphere is applied by column: at horizontal offset `(dx, dz)` inside the
//! radius the sphere's top is `cy + isqrt(r² - dx² - dz²)`, so the column's new
//! surface is one voxel below that — *if* the old surface was inside the sphere
//! at all. Two consequences the spike must state rather than hide:
//!
//! * a sphere entirely below the surface would hollow out a cave and leave a
//!   roof. A heightmap cannot represent that, so such an explosion is a no-op
//!   here, where the product's voxel world would produce an overhang;
//! * the resulting crater is a **bowl**, and whether it is passable falls out of
//!   the one-voxel step rule rather than being decided: at `r = 2` the rim steps
//!   are 1 voxel and the dimple is walkable; from `r = 3` the outermost step is
//!   2 voxels and the crater becomes a hole units must walk around. The bench
//!   defaults to 3 for exactly that reason — a crater that nobody has to path
//!   around would not invalidate anything.
//!
//! # What an edit dirties, and why the neighbours come too
//!
//! Plan §2's reason for a 32-cell cluster is that *"an explosion dirtied chunk
//! (cx, cy)"* maps straight onto *"repair cluster (cx, cy)"*. The dirty set is
//! the clusters containing an edited column — but repairing one cluster means
//! rebuilding its entrances, and an entrance is **shared**: rebuilding the
//! border between `A` and `B` changes `B`'s transition node set, so `B`'s
//! intra-cluster edges are stale even though no voxel inside `B` moved. Hence
//! [`repair`] rebuilds entrances on all four sides of each dirty cluster and
//! intra edges for the dirty clusters *and* their affected neighbours. Missing
//! that is the classic HPA\* repair bug: it shows up as an abstract edge to a
//! transition node that no longer exists, hours later, on one seed.

use crate::astar::Scratch;
use crate::clusters::Clusters;
use crate::ix;
use crate::world::{W, World, coord_of, node_of};

/// A spherical explosion, in world voxel coordinates. Same fields as G1's
/// `Explosion`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Crater {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub radius: i32,
}

/// What one crater did.
#[derive(Clone, Debug, Default)]
pub struct Blast {
    /// Columns whose surface moved, in node-id order.
    pub edited: Vec<u32>,
    /// Clusters that contain an edited column, in index order.
    pub dirty: Vec<u16>,
}

/// Carve one crater. Lowers the surface of every column the sphere's top
/// reaches, recomputes walkability for those columns, and reports the edit.
pub fn carve(world: &mut World, cl: &Clusters, c: &Crater, out: &mut Blast) {
    out.edited.clear();
    out.dirty.clear();
    let r = c.radius.max(0);
    let r2 = r * r;
    for dz in -r..=r {
        for dx in -r..=r {
            let d2 = dx * dx + dz * dz;
            if d2 > r2 {
                continue;
            }
            let Some(id) = node_of(c.x + dx, c.z + dz) else {
                continue;
            };
            // The sphere's topmost voxel in this column (G1's integer shape).
            let k = (r2 - d2).isqrt();
            let sphere_top = c.y + k;
            let sphere_bot = c.y - k;
            let top = world.top_at(id);
            if top > sphere_top || top < sphere_bot {
                // Either the sphere is entirely underground in this column (a
                // cave a heightmap cannot hold — see the module docs), or it is
                // entirely in the air above it. Either way, nothing moves.
                continue;
            }
            let new_top = sphere_bot - 1;
            if new_top >= top {
                continue;
            }
            world.top[ix(id)] = i16::try_from(new_top.max(-1)).expect("within world height");
            world.recompute_walkable(id);
            out.edited.push(id);
        }
    }
    out.edited.sort_unstable();
    out.dirty = out
        .edited
        .iter()
        .map(|id| cl.of_node[ix(*id)])
        .collect::<Vec<u16>>();
    out.dirty.sort_unstable();
    out.dirty.dedup();
}

/// The **conservative** superset of the clusters whose intra edges an edit
/// invalidates: the dirty ones and every cluster sharing a border with one of
/// them.
///
/// [`repair`] uses the precise set instead. A neighbour's intra edges only go
/// stale if the *shared border* actually changed, and on this terrain a crater
/// leaves most of its cluster's borders untouched, so the precise set is much
/// the smaller. This formulation is kept because it is the obvious one, and
/// because `repair_is_identical_to_the_conservative_set` checks that the two
/// reach the same graph — which is the only thing that makes the narrower set
/// safe.
#[must_use]
pub fn affected(cl: &Clusters, dirty: &[u16]) -> Vec<u16> {
    let mut v: Vec<u16> = Vec::with_capacity(dirty.len() * 5);
    for d in dirty {
        let (cx, cz) = cl.cxz(usize::from(*d));
        v.push(*d);
        for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            let (nx, nz) = (cx + dx, cz + dz);
            if nx < 0 || nz < 0 || nx >= cl.nx || nz >= cl.nz {
                continue;
            }
            v.push(u16::try_from(cl.cidx(nx, nz)).expect("at most 576 clusters"));
        }
    }
    v.sort_unstable();
    v.dedup();
    v
}

/// Repair one cluster's *local* state: components and the entrances on all four
/// of its sides. Split out so the bench can time a single cluster's repair,
/// which is what plan §3 step 4 asks to record.
///
/// Returns the neighbours whose shared border actually changed — the ones whose
/// own intra edges are now stale. A border that comes back identical costs its
/// neighbour nothing, and most do.
pub fn repair_cluster(world: &World, cl: &mut Clusters, c: u16) -> Vec<u16> {
    let ci = usize::from(c);
    cl.rebuild_local_comp(world, ci);
    let (cx, cz) = cl.cxz(ci);
    let mut changed: Vec<u16> = Vec::new();
    if cx + 1 < cl.nx {
        let bi = cl.vborder_index(cx, cz);
        let before = core::mem::take(&mut cl.vborder[bi]);
        cl.rebuild_vborder(world, cx, cz);
        if cl.vborder[bi] != before {
            changed.push(u16::try_from(cl.cidx(cx + 1, cz)).expect("at most 576 clusters"));
        }
    }
    if cx > 0 {
        let bi = cl.vborder_index(cx - 1, cz);
        let before = core::mem::take(&mut cl.vborder[bi]);
        cl.rebuild_vborder(world, cx - 1, cz);
        if cl.vborder[bi] != before {
            changed.push(u16::try_from(cl.cidx(cx - 1, cz)).expect("at most 576 clusters"));
        }
    }
    if cz + 1 < cl.nz {
        let bi = cl.hborder_index(cx, cz);
        let before = core::mem::take(&mut cl.hborder[bi]);
        cl.rebuild_hborder(world, cx, cz);
        if cl.hborder[bi] != before {
            changed.push(u16::try_from(cl.cidx(cx, cz + 1)).expect("at most 576 clusters"));
        }
    }
    if cz > 0 {
        let bi = cl.hborder_index(cx, cz - 1);
        let before = core::mem::take(&mut cl.hborder[bi]);
        cl.rebuild_hborder(world, cx, cz - 1);
        if cl.hborder[bi] != before {
            changed.push(u16::try_from(cl.cidx(cx, cz - 1)).expect("at most 576 clusters"));
        }
    }
    changed
}

/// The whole repair for one edit: local state for the dirty clusters, intra
/// edges for them and their affected neighbours, then one graph rebuild.
///
/// Returns the list of clusters whose intra edges were rebuilt.
pub fn repair(world: &World, cl: &mut Clusters, sc: &mut Scratch, dirty: &[u16]) -> Vec<u16> {
    let mut aff: Vec<u16> = dirty.to_vec();
    for c in dirty {
        aff.extend(repair_cluster(world, cl, *c));
    }
    aff.sort_unstable();
    aff.dedup();
    for c in &aff {
        cl.rebuild_trans(usize::from(*c));
    }
    for c in &aff {
        cl.rebuild_intra(world, sc, usize::from(*c));
    }
    cl.rebuild_graph();
    aff
}

/// The deterministic crater schedule, in the shape of G1's `Schedule`: a
/// splitmix-style mix of the seed and the ordinal, landing on the surface.
///
/// The bench steers craters onto walkers' routes instead of using this, because
/// a uniformly-placed crater almost never invalidates anything and the plan
/// wants ≥20 000 repaths — but this exists so the bench can fall back to an
/// unsteered stream, and so the two spikes' streams are visibly the same idea.
#[must_use]
pub fn scheduled(world: &World, seed: u64, n: u64, radius: i32) -> Crater {
    let mut s = seed.wrapping_add(n.wrapping_mul(0x9e37_79b9_7f4a_7c15));
    s ^= s >> 30;
    s = s.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    s ^= s >> 27;
    s = s.wrapping_mul(0x94d0_49bb_1331_11eb);
    s ^= s >> 31;
    let x = i32::try_from(s % 320).unwrap_or(0) + 32;
    let z = i32::try_from((s >> 20) % 320).unwrap_or(0) + 32;
    surface_crater(world, x, z, s >> 40, radius)
}

/// A crater centred on the surface of column `(x, z)`, sunk 0–2 voxels so the
/// bite is not always identical. Shared by the schedule and by the bench's
/// route-steered stream.
#[must_use]
pub fn surface_crater(world: &World, x: i32, z: i32, jitter: u64, radius: i32) -> Crater {
    let x = x.clamp(0, W - 1);
    let z = z.clamp(0, W - 1);
    let id = node_of(x, z).expect("clamped in bounds");
    let sink = i32::try_from(jitter % 3).unwrap_or(0);
    let y = (world.top_at(id) - sink).clamp(0, crate::world::WY - 2);
    Crater { x, y, z, radius }
}

/// Nodes an edit can change the *legality of a step through*: the edited
/// columns and their eight-neighbourhood. A diagonal step's legality depends on
/// the two orthogonal companions, so a column one cell off a path can still
/// invalidate it.
pub fn touched_ring(edited: &[u32], out: &mut Vec<u32>) {
    out.clear();
    for id in edited {
        let (x, z) = coord_of(*id);
        for dz in -1..=1 {
            for dx in -1..=1 {
                if let Some(n) = node_of(x + dx, z + dz) {
                    out.push(n);
                }
            }
        }
    }
    out.sort_unstable();
    out.dedup();
}

#[cfg(test)]
mod tests {
    use super::{Blast, Crater, carve, repair, surface_crater, touched_ring};
    use crate::astar::Scratch;
    use crate::clusters::Clusters;
    use crate::world::{World, node_of};

    #[test]
    fn a_crater_is_the_same_integer_sphere_as_g1() {
        // r = 3: the columns edited are exactly the lattice disc dx²+dz² <= 9,
        // which is 29 cells.
        let mut w = World::flat(30);
        let mut sc = Scratch::new();
        let cl = Clusters::build(&w, 32, &mut sc);
        let mut b = Blast::default();
        let c = surface_crater(&w, 100, 100, 0, 3);
        carve(&mut w, &cl, &c, &mut b);
        assert_eq!(b.edited.len(), 29);
        // Bowl shape: deepest in the middle, shallow at the rim.
        let centre = node_of(100, 100).expect("in bounds");
        let rim = node_of(103, 100).expect("in bounds");
        assert!(w.top_at(centre) < w.top_at(rim));
    }

    #[test]
    fn repair_leaves_the_graph_identical_to_a_from_scratch_build() {
        // The assertion plan §2's repair strategy lives or dies on.
        let mut w = World::generate(20_260_913);
        let mut sc = Scratch::new();
        let mut cl = Clusters::build(&w, 32, &mut sc);
        let mut b = Blast::default();
        let mut n = 0;
        for i in 0..24u64 {
            let x = 60 + i32::try_from(i * 13 % 260).expect("fits");
            let z = 60 + i32::try_from(i * 29 % 260).expect("fits");
            let c = surface_crater(&w, x, z, i, 3);
            carve(&mut w, &cl, &c, &mut b);
            if b.dirty.is_empty() {
                continue;
            }
            repair(&w, &mut cl, &mut sc, &b.dirty);
            let fresh = Clusters::build(&w, 32, &mut sc);
            assert_eq!(
                cl.fingerprint(),
                fresh.fingerprint(),
                "incremental repair diverged from a from-scratch build after crater {i}"
            );
            n += 1;
        }
        assert!(n > 15, "only {n} craters actually edited terrain");
    }

    #[test]
    fn repair_is_identical_to_the_conservative_set() {
        // The precise affected-neighbour set must reach the same graph as the
        // obvious "every neighbour of every dirty cluster" formulation.
        use super::{affected, repair_cluster};
        let mut w = World::generate(11);
        let mut sc = Scratch::new();
        let mut cl = Clusters::build(&w, 32, &mut sc);
        let mut wide = Clusters::build(&w, 32, &mut sc);
        let mut b = Blast::default();
        let mut n = 0;
        for i in 0..12u64 {
            let c = surface_crater(&w, 120 + i32::try_from(i * 19).expect("fits"), 210, i, 3);
            carve(&mut w, &cl, &c, &mut b);
            if b.dirty.is_empty() {
                continue;
            }
            repair(&w, &mut cl, &mut sc, &b.dirty);
            for d in &b.dirty {
                repair_cluster(&w, &mut wide, *d);
            }
            let conservative = affected(&wide, &b.dirty);
            for c in &conservative {
                wide.rebuild_trans(usize::from(*c));
            }
            for c in &conservative {
                wide.rebuild_intra(&w, &mut sc, usize::from(*c));
            }
            wide.rebuild_graph();
            assert_eq!(cl.fingerprint(), wide.fingerprint(), "crater {i}");
            n += 1;
        }
        assert!(n > 8, "only {n} craters edited terrain");
    }

    #[test]
    fn repair_is_identical_at_every_cluster_size() {
        for size in [16, 32, 64] {
            let mut w = World::generate(5);
            let mut sc = Scratch::new();
            let mut cl = Clusters::build(&w, size, &mut sc);
            let mut b = Blast::default();
            for i in 0..6u64 {
                let c = surface_crater(&w, 150 + i32::try_from(i * 17).expect("fits"), 170, i, 4);
                carve(&mut w, &cl, &c, &mut b);
                if b.dirty.is_empty() {
                    continue;
                }
                repair(&w, &mut cl, &mut sc, &b.dirty);
            }
            let fresh = Clusters::build(&w, size, &mut sc);
            assert_eq!(cl.fingerprint(), fresh.fingerprint(), "size {size}");
        }
    }

    #[test]
    fn the_touched_ring_covers_the_diagonal_corner_rule() {
        let a = node_of(10, 10).expect("in bounds");
        let mut out = Vec::new();
        touched_ring(&[a], &mut out);
        assert_eq!(out.len(), 9);
        assert!(out.contains(&node_of(9, 9).expect("in bounds")));
        assert!(out.contains(&node_of(11, 11).expect("in bounds")));
    }

    #[test]
    fn a_crater_below_the_surface_is_a_no_op_on_a_heightmap() {
        let mut w = World::flat(30);
        let mut sc = Scratch::new();
        let cl = Clusters::build(&w, 32, &mut sc);
        let mut b = Blast::default();
        carve(
            &mut w,
            &cl,
            &Crater {
                x: 100,
                y: 10,
                z: 100,
                radius: 3,
            },
            &mut b,
        );
        assert!(b.edited.is_empty());
    }
}
