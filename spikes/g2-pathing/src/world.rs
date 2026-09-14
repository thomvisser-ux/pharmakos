// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later
#![deny(clippy::float_arithmetic, clippy::as_conversions)]
//! The 384×384×64 world, its deterministic generator, and the 2.5D surface
//! walkability derivation the search graph is built on.
//!
//! # The search graph is a surface, not a volume
//!
//! Plan §2: *"the walkable set is: a voxel column's surface cell is walkable if
//! it is solid, the cell above is empty with enough headroom, and the step up or
//! down to a neighbour is at most 1 voxel"*. One node per column, 384×384 =
//! 147 456 of them, rather than 9.4 M voxels. The node id **is** the column
//! index, `z * 384 + x`, as a `u32` (plan §5: never `usize`, so target pointer
//! width cannot leak into anything hashed).
//!
//! # Overhangs and bridges: not possible here, and what would change
//!
//! [`World`] stores one `top` height per column. That is a *heightmap*, so by
//! construction there is exactly one surface cell per column and neither
//! overhangs nor bridges nor tunnels can exist — [`OVERHANGS_POSSIBLE`] is
//! `false` and the plan's §2 question is answered in the negative. A generator
//! that produced them (an arch, a cave mouth, a second storey) would need more
//! than one node per column, which changes exactly three things and nothing
//! else: the id↔coordinate pair below stops being a bijection with the column
//! index and needs a per-column surface list plus an offset table; the
//! neighbour function has to pick *which* surface of the neighbouring column it
//! is stepping to (there may be two within ±1); and the node count stops being
//! a compile-time constant, so the dense search arrays in [`crate::astar`] have
//! to be sized at load. The cost model, the heuristic, the cluster
//! decomposition and the repair strategy are all unaffected — they only ever
//! see node ids. The spike deliberately does not pay for that, because the
//! numbers it is measuring (branching factor 8, one surface per column) would
//! not move much: a map with overhangs has perhaps 1.05 nodes per column, not
//! 2. Recorded here rather than discovered at S3.
//!
//! # Three-fold symmetry, exactly, in integers
//!
//! Plan §3 step 1 wants *"3-way rotational symmetry, ridges, a few chokepoints,
//! ore seams, one big central basin"* and explicitly **not** noise. A 120°
//! Euclidean rotation needs `cos 120° = -1/2` and `sin 120° = √3/2`, which in
//! fixed point does not satisfy `R³ = I` exactly — the field would be *nearly*
//! symmetric, and "nearly" is the kind of thing that turns into a desync story
//! later. So the symmetry used here is the exact integer one: writing
//! `u = x - 192`, `v = z - 192` as axial coordinates of a triangular lattice,
//!
//! ```text
//! S(u, v) = (v, -u - v)
//! ```
//!
//! is an integer linear map with `S³ = I` exactly (it is the 120° rotation of
//! the hexagonal lattice, in cube coordinates `(u, v, -u-v) → (v, -u-v, u)`).
//! Every feature below is made invariant under `S` either by taking the maximum
//! over the orbit `{p, Sp, S²p}` (the ridges and the gates), or by depending
//! only on quantities that are exactly `S`-invariant: the hex distance
//! `(|u| + |v| + |u+v|) / 2`, and any symmetric function of the multiset
//! `{|u|, |v|, |u+v|}` (the undulation). The result is a height field that is
//! **bit-exactly** invariant under a three-fold symmetry, with no rounding
//! caveat, and three start regions with identical terrain.
//!
//! Geometrically `S` is a 120° rotation composed with the shear that turns the
//! square lattice into a triangular one, so the map looks sheared rather than
//! round. For a pathing spike that is a feature, not a defect: sheared
//! chokepoints and asymmetric-looking detours are harder for an abstract
//! estimator than round ones.

use crate::cost::{CLIMB_SURCHARGE, STEP_CARDINAL, STEP_DIAGONAL};
use crate::{id32, ix};

/// Footprint edge, in cells. Plan §2.
pub const W: i32 = 384;
/// World height, in voxels. Plan §2.
pub const WY: i32 = 64;
/// One node per column.
pub const NODES: usize = 147_456;
/// Voxels of clear air a unit needs above its feet.
pub const HEADROOM: i32 = 2;
/// See the module docs: this generator is a heightmap.
pub const OVERHANGS_POSSIBLE: bool = false;

/// The eight neighbour offsets, in a fixed, documented order (row-major from
/// the top-left). Expansion order is part of the determinism contract; the
/// tiebreak test permutes it and demands an identical path.
pub const NEIGHBOURS: [(i32, i32); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// The default expansion order (identity over [`NEIGHBOURS`]).
pub const DEFAULT_ORDER: [u8; 8] = [0, 1, 2, 3, 4, 5, 6, 7];

/// Coordinate → node id. One half of the audited pair (plan §5); round-tripped
/// in the tests below.
#[inline]
#[must_use]
pub fn node_of(x: i32, z: i32) -> Option<u32> {
    if x < 0 || z < 0 || x >= W || z >= W {
        return None;
    }
    let lin = z * W + x;
    u32::try_from(lin).ok()
}

/// Node id → coordinate. The other half.
#[inline]
#[must_use]
pub fn coord_of(id: u32) -> (i32, i32) {
    let lin = i32::try_from(id).expect("node ids are below 147 456");
    (lin % W, lin / W)
}

/// An integer triangle wave: period `period`, amplitude `amp`, value 0 at
/// `t = 0`. Used for the gentle undulation that gives the surface its 1-voxel
/// steps — the thing the climb surcharge exists to price.
#[must_use]
fn tri(t: i32, period: i32, amp: i32) -> i32 {
    let q = period / 4;
    let p = t.rem_euclid(period);
    if p < q {
        p * amp / q
    } else if p < 3 * q {
        amp - (p - q) * amp / q
    } else {
        -amp + (p - 3 * q) * amp / q
    }
}

/// `S(u, v) = (v, -u - v)`: the exact order-3 lattice rotation. See module docs.
#[inline]
#[must_use]
const fn s_rot(u: i32, v: i32) -> (i32, i32) {
    (v, -u - v)
}

/// Hex distance from the centre; exactly `S`-invariant.
#[inline]
#[must_use]
const fn hex_dist(u: i32, v: i32) -> i32 {
    (u.abs() + v.abs() + (u + v).abs()) / 2
}

/// One ridge: a thick triangular wall along a segment, with one gap in it.
///
/// `gap_at` and `gap_half` are in per-mille of the segment length, so a gap is
/// specified as a fraction of the ridge rather than in cells.
#[derive(Clone, Copy, Debug)]
struct Ridge {
    ax: i32,
    az: i32,
    bx: i32,
    bz: i32,
    half: i32,
    height: i32,
    gap_at: i32,
    gap_half: i32,
}

/// Height a ridge contributes at `(pu, pv)`; 0 outside its footprint and inside
/// its gap. Integer throughout: the closest point on the segment is found by
/// an integer projection, and the distance by `i32::isqrt`.
fn ridge_at(r: &Ridge, pu: i32, pv: i32) -> i32 {
    let vx = r.bx - r.ax;
    let vz = r.bz - r.az;
    let vv = vx * vx + vz * vz;
    if vv == 0 {
        return 0;
    }
    let wx = pu - r.ax;
    let wz = pv - r.az;
    let t = (wx * vx + wz * vz).clamp(0, vv);
    let cx = r.ax + (vx * t) / vv;
    let cz = r.az + (vz * t) / vv;
    let dx = pu - cx;
    let dz = pv - cz;
    let d2 = dx * dx + dz * dz;
    if d2 >= r.half * r.half {
        return 0;
    }
    let permille = (t * 1000) / vv;
    if (permille - r.gap_at).abs() <= r.gap_half {
        return 0;
    }
    let d = d2.isqrt();
    r.height * (r.half - d) / r.half
}

/// The two base ridges. Each is turned into three by the `S` orbit, so the map
/// carries six walls: three long "spokes" running out from the basin that
/// separate the three lobes, and three shorter cross-walls that force a detour
/// inside a lobe. One chokepoint gap each.
const RIDGES: [Ridge; 2] = [
    Ridge {
        ax: 34,
        az: -18,
        bx: 172,
        bz: -86,
        half: 7,
        height: 17,
        gap_at: 540,
        gap_half: 42,
    },
    Ridge {
        ax: -104,
        az: -14,
        bx: -24,
        bz: -118,
        half: 6,
        height: 15,
        gap_at: 460,
        gap_half: 52,
    },
];

/// Ring walls, expressed on the exactly-invariant hex distance, with gates cut
/// by the `S` orbit of a single box. Two rings: an inner one around the basin
/// and an outer one, so a cross-map route has to find four gates or go round.
const RINGS: [(i32, i32, i32); 2] = [
    // (hex radius, half thickness, height)
    (78, 5, 14),
    (146, 6, 16),
];

/// Gate centres, one per ring; each becomes three gates through the orbit.
///
/// A gate centre must sit *on* its ring (`hex_dist == radius`) and its box must
/// be wide enough to span the whole ring thickness in hex distance, or the gate
/// is a notch rather than a hole and the ring still seals. That is not a
/// hypothetical: the first version of this table put the ring-0 gate at hex
/// distance 70 against a ring at 78, and the map came out as three lobes with no
/// route between them at all.
const GATES: [(i32, i32, i32, i32); 2] = [
    // (ring index, u, v, half-extent)
    (0, 78, -39, 11),
    (1, 146, -73, 13),
];

/// Height of the terrain at one column, before clamping: the designed features
/// evaluated over the `S` orbit.
fn raw_height(u: i32, v: i32) -> i32 {
    let hd = hex_dist(u, v);

    // Base plateau, the central basin, and the fall-off past the outer ring.
    let mut h = 30;
    if hd < 60 {
        h -= (60 - hd) * 12 / 60;
    }
    if hd > 200 {
        h -= (hd - 200) * 10 / 80;
    }

    // Ridges: max over the orbit, so the three copies are exact.
    let mut orbit = [(u, v); 3];
    orbit[1] = s_rot(orbit[0].0, orbit[0].1);
    orbit[2] = s_rot(orbit[1].0, orbit[1].1);
    let mut wall = 0;
    for (ou, ov) in orbit {
        for r in &RIDGES {
            let c = ridge_at(r, ou, ov);
            if c > wall {
                wall = c;
            }
        }
    }

    // Rings: the radius is invariant already; the gates are cut by the orbit.
    for (ri, &(radius, thick, height)) in RINGS.iter().enumerate() {
        let d = (hd - radius).abs();
        if d >= thick {
            continue;
        }
        let mut gated = false;
        for &(gr, gu, gv, ext) in &GATES {
            if gr != i32::try_from(ri).expect("two rings") {
                continue;
            }
            for (ou, ov) in orbit {
                if (ou - gu).abs() <= ext && (ov - gv).abs() <= ext {
                    gated = true;
                }
            }
        }
        if gated {
            continue;
        }
        let c = height * (thick - d) / thick;
        if c > wall {
            wall = c;
        }
    }
    h += wall;

    // Gentle undulation. A symmetric function of the multiset {|u|,|v|,|u+v|},
    // which S permutes, so this is exactly invariant too. Amplitude is small
    // enough that most neighbour steps are 0 or 1 voxels — which is the point:
    // the climb surcharge has to have something to price.
    h += tri(u.abs(), 96, 3) + tri(v.abs(), 96, 3) + tri((u + v).abs(), 96, 3);

    h
}

/// The world: one surface height per column, plus the derived walkable mask and
/// an ore-seam mask that exists only so the generator matches plan §3 step 1
/// (ore does not affect movement).
#[derive(Clone, Debug)]
pub struct World {
    pub seed: u64,
    /// Topmost solid voxel of each column, or `-1` for an empty column.
    pub top: Vec<i16>,
    pub walk: Vec<bool>,
    pub ore: Vec<bool>,
}

impl World {
    /// Build the map. Deterministic in `seed` alone; `seed` shifts the feature
    /// phases without breaking the exact `S` symmetry (it only enters through
    /// invariant quantities).
    #[must_use]
    pub fn generate(seed: u64) -> Self {
        let phase = i32::try_from(seed % 64).expect("below 64");
        let mut top = vec![0i16; NODES];
        let mut ore = vec![false; NODES];
        for z in 0..W {
            for x in 0..W {
                let u = x - W / 2;
                let v = z - W / 2;
                let hd = hex_dist(u, v);
                let mut h = raw_height(u, v);
                // The seed's only job is to move the undulation's phase; it goes
                // through an invariant quantity so symmetry survives it.
                h += tri(hd + phase, 128, 2);
                let h = h.clamp(1, WY - 6);
                let i = ix(node_of(x, z).expect("in bounds"));
                top[i] = i16::try_from(h - 1).expect("clamped to 1..=58");
                // Ore seams: bands at two hex radii, thinned by an invariant
                // wave. Cosmetic for pathing; recorded because plan §3 step 1
                // asks the generator for them.
                let band = (hd - 92).abs() < 3 || (hd - 168).abs() < 2;
                ore[i] = band && (hd + phase).rem_euclid(7) < 3;
            }
        }
        let mut w = Self {
            seed,
            top,
            walk: vec![false; NODES],
            ore,
        };
        w.recompute_walkable_all();
        w
    }

    /// A flat, fully-connected map. Used by the worst-case no-recursion test
    /// (plan §5 "stack size": one region, 147 456 cells).
    #[must_use]
    pub fn flat(height: i16) -> Self {
        let mut w = Self {
            seed: 0,
            top: vec![height; NODES],
            walk: vec![false; NODES],
            ore: vec![false; NODES],
        };
        w.recompute_walkable_all();
        w
    }

    #[inline]
    #[must_use]
    pub fn top_at(&self, id: u32) -> i32 {
        i32::from(self.top[ix(id)])
    }

    #[inline]
    #[must_use]
    pub fn walkable(&self, id: u32) -> bool {
        self.walk[ix(id)]
    }

    /// A column is walkable when it has a solid surface and [`HEADROOM`] voxels
    /// of air above it, inside the world. Plan §2.
    ///
    /// **On this map the test is a tautology, and the write-up says so.**
    /// [`World::generate`] clamps every height to `1..=WY - 6`, so `top` lands
    /// in `0..=57` and `t + HEADROOM < WY` (`t + 2 < 64`) holds for every column
    /// of every generated map: `walkable_count()` is 147 456 of 147 456 by
    /// construction. Craters can push a column to `top = -1` and fail the first
    /// half of the test, but only 49 of 40 085 edited columns did so in the
    /// recorded run. The headroom half is therefore **untested** by the
    /// measurement, and `assert_cost_headroom(walkable_count())` re-derives
    /// `MAX_PATH_COST` rather than checking anything about the map.
    ///
    /// What actually blocks movement here is the one-voxel step rule in
    /// [`World::neighbour`] — which is why the scale number worth reading is
    /// [`World::directed_step_count`] (and the component sizes), not the
    /// walkable-column count. Raising the clamp ceiling so ridges reach the
    /// world ceiling would give the headroom test something to do; it would also
    /// change every number in the run, so it is recorded as a known gap rather
    /// than changed after the measurement.
    #[inline]
    #[must_use]
    pub fn column_walkable(&self, id: u32) -> bool {
        let t = i32::from(self.top[ix(id)]);
        t >= 0 && t + HEADROOM < WY
    }

    pub fn recompute_walkable_all(&mut self) {
        for i in 0..NODES {
            self.walk[i] = self.column_walkable(id32(i));
        }
    }

    #[inline]
    pub fn recompute_walkable(&mut self, id: u32) {
        self.walk[ix(id)] = self.column_walkable(id);
    }

    /// The `k`-th neighbour of `id` under `NEIGHBOURS`, with its step cost, or
    /// `None` when the step is illegal.
    ///
    /// Legality, in full (plan §2 plus the corner rule):
    ///
    /// * both columns walkable;
    /// * `|Δtop| <= 1` — one voxel of climb, section 9's locomotion rule;
    /// * for a diagonal, both orthogonal companions walkable and within one
    ///   voxel of **both** endpoints. Checking against both endpoints rather
    ///   than just the origin is what keeps the edge relation — and therefore
    ///   the cost — symmetric, which A\*/Dijkstra agreement and the reversed-path
    ///   test both rely on.
    #[inline]
    #[must_use]
    pub fn neighbour(&self, id: u32, k: usize) -> Option<(u32, i32)> {
        let (x, z) = coord_of(id);
        let (dx, dz) = NEIGHBOURS[k];
        let nid = node_of(x + dx, z + dz)?;
        if !self.walk[ix(nid)] || !self.walk[ix(id)] {
            return None;
        }
        let ta = i32::from(self.top[ix(id)]);
        let tb = i32::from(self.top[ix(nid)]);
        let dh = tb - ta;
        if dh.abs() > 1 {
            return None;
        }
        let climb = if dh == 0 { 0 } else { CLIMB_SURCHARGE };
        if dx != 0 && dz != 0 {
            let o1 = node_of(x + dx, z)?;
            let o2 = node_of(x, z + dz)?;
            if !self.walk[ix(o1)] || !self.walk[ix(o2)] {
                return None;
            }
            let t1 = i32::from(self.top[ix(o1)]);
            let t2 = i32::from(self.top[ix(o2)]);
            if (t1 - ta).abs() > 1
                || (t2 - ta).abs() > 1
                || (t1 - tb).abs() > 1
                || (t2 - tb).abs() > 1
            {
                return None;
            }
            return Some((nid, STEP_DIAGONAL + climb));
        }
        Some((nid, STEP_CARDINAL + climb))
    }

    /// Cost of the step between two adjacent nodes, or `None` if illegal. Used
    /// by the smoother and by the movement integrator.
    #[must_use]
    pub fn step_cost_between(&self, a: u32, b: u32) -> Option<i32> {
        let (ax, az) = coord_of(a);
        let (bx, bz) = coord_of(b);
        let (dx, dz) = (bx - ax, bz - az);
        if dx.abs() > 1 || dz.abs() > 1 || (dx == 0 && dz == 0) {
            return None;
        }
        let k = NEIGHBOURS.iter().position(|&o| o == (dx, dz))?;
        self.neighbour(a, k).map(|(_, c)| c)
    }

    #[must_use]
    pub fn walkable_count(&self) -> usize {
        self.walk.iter().filter(|w| **w).count()
    }

    /// The number of **legal directed steps** on the current terrain: summed
    /// over every node, how many of its eight neighbours it can actually step
    /// to.
    ///
    /// This is the search graph's real size, and the number plan §3 step 1's
    /// "walkable-cell count" is reaching for. On a heightmap the column count is
    /// 100% by construction (see [`World::column_walkable`]); the edge count is
    /// what the one-voxel step rule actually thins, so it is what moves when
    /// craters cut a corridor. Diagnostic, O(nodes × 8), never called from a
    /// timed path.
    #[must_use]
    pub fn directed_step_count(&self) -> usize {
        let mut n = 0usize;
        for i in 0..NODES {
            let id = id32(i);
            if !self.walk[i] {
                continue;
            }
            for k in 0..NEIGHBOURS.len() {
                if self.neighbour(id, k).is_some() {
                    n += 1;
                }
            }
        }
        n
    }

    #[must_use]
    pub fn ore_count(&self) -> usize {
        self.ore.iter().filter(|o| **o).count()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_ORDER, NEIGHBOURS, NODES, OVERHANGS_POSSIBLE, W, World, coord_of, hex_dist,
        node_of, s_rot, tri,
    };
    use crate::cost::MAX_STEP_COST;
    use crate::{id32, ix};

    #[test]
    fn coord_and_id_round_trip_everywhere() {
        for z in 0..W {
            for x in 0..W {
                let id = node_of(x, z).expect("in bounds");
                assert_eq!(coord_of(id), (x, z));
            }
        }
        assert!(node_of(-1, 0).is_none());
        assert!(node_of(0, -1).is_none());
        assert!(node_of(W, 0).is_none());
        assert!(node_of(0, W).is_none());
        assert_eq!(ix(id32(NODES - 1)), NODES - 1);
    }

    #[test]
    fn s_rot_has_order_three_exactly() {
        for u in -200..200 {
            for v in [-300, -77, -1, 0, 1, 77, 300] {
                let a = s_rot(u, v);
                let b = s_rot(a.0, a.1);
                let c = s_rot(b.0, b.1);
                assert_eq!(c, (u, v));
                assert_eq!(hex_dist(u, v), hex_dist(a.0, a.1));
            }
        }
    }

    #[test]
    fn the_height_field_is_exactly_three_fold_symmetric() {
        let w = World::generate(20_260_913);
        let mut checked = 0u32;
        for z in (0..W).step_by(3) {
            for x in (0..W).step_by(3) {
                let (u, v) = (x - W / 2, z - W / 2);
                let (su, sv) = s_rot(u, v);
                let (sx, sz) = (su + W / 2, sv + W / 2);
                if let Some(other) = node_of(sx, sz) {
                    let here = node_of(x, z).expect("in bounds");
                    assert_eq!(
                        w.top_at(here),
                        w.top_at(other),
                        "symmetry broken at ({x},{z}) -> ({sx},{sz})"
                    );
                    checked += 1;
                }
            }
        }
        assert!(
            checked > 5_000,
            "only {checked} cells had their image in range"
        );
    }

    #[test]
    fn triangle_wave_is_bounded_and_periodic() {
        for t in -500..500 {
            let a = tri(t, 96, 3);
            assert!((-3..=3).contains(&a));
            assert_eq!(a, tri(t + 96, 96, 3));
        }
    }

    #[test]
    fn steps_are_symmetric_and_bounded() {
        let w = World::generate(7);
        for id in (0..NODES).step_by(997) {
            let id = id32(id);
            for k in DEFAULT_ORDER {
                let k = usize::from(k);
                if let Some((n, c)) = w.neighbour(id, k) {
                    assert!(c <= MAX_STEP_COST);
                    let back = w.step_cost_between(n, id).expect("edges are symmetric");
                    assert_eq!(back, c);
                    let (dx, dz) = NEIGHBOURS[k];
                    let (ax, az) = coord_of(id);
                    assert_eq!(coord_of(n), (ax + dx, az + dz));
                }
            }
        }
    }

    #[test]
    fn the_directed_step_count_is_the_scale_number_the_column_count_is_not() {
        let w = World::generate(20_260_913);
        // The tautology, asserted so the write-up's claim is checked rather than
        // merely stated: every column of this map passes `column_walkable`.
        assert_eq!(
            w.walkable_count(),
            NODES,
            "on a heightmap clamped to 1..=WY-6 every column is walkable"
        );
        let e = w.directed_step_count();
        assert!(
            e < 8 * NODES,
            "the one-voxel step rule must thin the graph: got {e} of {}",
            8 * NODES
        );
        assert!(e > NODES, "…but not to nothing: got {e}");
    }

    #[test]
    fn the_generator_makes_no_overhangs() {
        // The claim is structural, not statistical: `top` is one number per
        // column, so there is nowhere for a second surface to live.
        const { assert!(!OVERHANGS_POSSIBLE) };
        let w = World::generate(1);
        assert_eq!(w.top.len(), NODES);
    }
}
