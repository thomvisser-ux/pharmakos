// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later
//! Six-direction greedy mesher with vertex colours and baked flood-fill light.
//!
//! # THIS FILE IS THE PRESENTATION SIDE OF THE WALL
//!
//! Everything above it — `chunk.rs`, `world.rs`, `explode.rs`, `budget.rs` — is
//! integer and deterministic and carries `#![deny(clippy::float_arithmetic)]`.
//! The float boundary starts *here*, at vertex generation, and goes no further:
//! nothing in this module writes voxel state, and nothing it produces is read
//! back by the sim (G1 plan §2 "Determinism note", AGENTS.md §4.9). This is the
//! exact shape of the carve-out the float lint has to be written around.
//!
//! # Algorithm
//!
//! For each of the 6 face directions, sweep the 32 slices of the chunk; build a
//! 32×32 mask of *visible* faces keyed by `(material, light)`; merge each
//! maximal rectangle of equal key into one quad. Neighbour-aware: the chunk is
//! copied with a one-voxel border of its six neighbours first
//! (`World::fill_padded`), so a face that is interior across a chunk boundary is
//! never emitted.
//!
//! **T-junctions are accepted, not prevented.** The padded copy stops interior
//! faces being emitted; it has nothing to do with T-junctions, which greedy
//! meshing produces by construction wherever one merged quad abuts several
//! smaller coplanar ones — inside a chunk as much as across a seam. No crack was
//! observed in the fixed vista at 1920×1080 with MSAA off, because every vertex
//! coordinate is a small exact integer in `f32` and there is no interpolation to
//! disagree about. That is a *measurement on this configuration*, not a
//! guarantee: a surface format with interpolated attributes, a non-integer chunk
//! transform, or a larger world where coordinates stop being exact would all
//! need it re-checked.
//!
//! # Surface format, and why it is this one
//!
//! `ARRAY_VERTEX` (`Vector3`) + `ARRAY_COLOR` + `ARRAY_INDEX`. No normals, no
//! tangents, no UVs. The classic per-face shading factor and the flood-fill
//! light are folded into the vertex colour and the material is unshaded, which
//! (a) is the look this game wants anyway, and (b) leaves path B's buffers
//! unambiguous: the vertex buffer is exactly 12 bytes/vertex of position and
//! the attribute buffer exactly 4 bytes/vertex of RGBA8, which is what makes
//! `mesh_surface_update_vertex_region` safe to use in place.

use crate::chunk::{AIR, CHUNK_EDGE};
use crate::world::{MAT_COUNT, PAD_EDGE, PAD_VOX, World};

/// FFI payload cost of one vertex: `Vector3` position + `Color` (4 × f32).
/// This is what `mesh_add_surface_from_arrays` actually carries across, on the
/// first build and on every rebuild. The GPU-side cost is smaller (12 + 4), and
/// path B's *in-place* update carries smaller still (12 + 4 and no indices) —
/// see `upload_rs::RsPath::upload`, which reports what each branch really wrote
/// rather than this figure.
pub const BYTES_PER_VERTEX: usize = 12 + 16;
/// FFI index cost: the mesher hands Godot a `PackedInt32Array`, 4 bytes each.
pub const BYTES_PER_INDEX: usize = 4;
/// GPU-side bytes: position 12 + RGBA8 colour 4.
pub const GPU_BYTES_PER_VERTEX: usize = 12 + 4;
/// GPU-side index width. Godot stores 16-bit indices for any surface under
/// 65 536 vertices, which step 1 asserts is always the case here (max ≈ 6 940).
/// `gpu_surface_bytes` checks that assumption rather than trusting it.
pub const GPU_BYTES_PER_INDEX: usize = 2;
/// Vertex count above which Godot switches the index buffer to 32-bit.
pub const INDEX16_VERTEX_CAP: usize = 65_536;

#[inline]
pub const fn surface_bytes(verts: usize, indices: usize) -> usize {
    verts * BYTES_PER_VERTEX + indices * BYTES_PER_INDEX
}

/// What the surface occupies on the GPU. Uses the 16-bit index width Godot
/// picks below `INDEX16_VERTEX_CAP`, and falls back to 32-bit above it, so the
/// figure cannot quietly disagree with the "16-bit indices suffice" claim it
/// sits next to.
#[inline]
pub const fn gpu_surface_bytes(verts: usize, indices: usize) -> usize {
    let iw = if verts < INDEX16_VERTEX_CAP {
        GPU_BYTES_PER_INDEX
    } else {
        BYTES_PER_INDEX
    };
    verts * GPU_BYTES_PER_VERTEX + indices * iw
}

/// Godot 4.7 treats **clockwise** winding as front-facing under `CULL_BACK` for
/// triangle primitives — which is what Godot's own `Mesh` documentation says,
/// and what the vista screenshot confirmed: the first run came back showing the
/// inside of the terrain (sky through the ground, box lids shaded as
/// undersides) with counter-clockwise quads, and emitting them clockwise fixed
/// it. `Mesher::flip_winding` is set straight from this constant, so the name,
/// the value and the behaviour say the same thing — this is the sentence that
/// gets copied into `docs/design/decisions-log.md`, so it has to. Kept as a
/// run-time override (`--flip_winding=1`) as well, so a winding mistake costs a
/// re-run rather than a rebuild: the screenshot in plan §3 step 9 is exactly the
/// check that catches this, and no timing number would have.
pub const CLOCKWISE_FRONT: bool = true;

const PALETTE: [[f32; 3]; MAT_COUNT] = [
    [0.00, 0.00, 0.00], // 0 air, never emitted
    [0.52, 0.52, 0.56], // 1 stone
    [0.42, 0.31, 0.20], // 2 dirt
    [0.31, 0.56, 0.25], // 3 grass
    [0.80, 0.64, 0.26], // 4 ore A
    [0.28, 0.66, 0.74], // 5 ore B
    [0.74, 0.72, 0.68], // 6 concrete
];

/// Face shading by direction index `axis * 2 + (sign < 0)`.
const FACE_SHADE: [f32; 6] = [0.82, 0.72, 1.00, 0.45, 0.92, 0.62];

/// Step in the padded 34^3 buffer for one unit of world axis x / y / z.
/// `pidx(x, y, z) == y * 1156 + z * 34 + x`.
const PAD_STRIDE: [usize; 3] = [1, PAD_EDGE * PAD_EDGE, PAD_EDGE];

/// One chunk surface. Reused across meshings; `clear()` keeps the capacity.
#[derive(Default)]
pub struct MeshBuf {
    pub pos: Vec<[f32; 3]>,
    pub col: Vec<[f32; 4]>,
    pub idx: Vec<i32>,
}

impl MeshBuf {
    pub fn clear(&mut self) {
        self.pos.clear();
        self.col.clear();
        self.idx.clear();
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.idx.is_empty()
    }
    #[inline]
    pub fn vertex_count(&self) -> usize {
        self.pos.len()
    }
    #[inline]
    pub fn index_count(&self) -> usize {
        self.idx.len()
    }
    #[inline]
    pub fn bytes(&self) -> usize {
        surface_bytes(self.pos.len(), self.idx.len())
    }
}

/// Scratch buffers live on the mesher, not on the stack of `mesh()`: the
/// Windows allocator is markedly slower than glibc's for the many short-lived
/// vectors a naive mesher produces (G1 plan §6, "Allocator behaviour"), so the
/// steady state here is *zero* allocations per meshing once the vectors have
/// grown.
pub struct Mesher {
    mat: Vec<u8>,
    light: Vec<u8>,
    mask: Vec<u16>,
    pub flip_winding: bool,
    pub quads_emitted: u64,
    pub chunks_meshed: u64,
}

impl Default for Mesher {
    fn default() -> Self {
        Self::new()
    }
}

impl Mesher {
    pub fn new() -> Self {
        Self {
            mat: vec![0u8; PAD_VOX],
            light: vec![0u8; PAD_VOX],
            mask: vec![0u16; CHUNK_EDGE * CHUNK_EDGE],
            // Godot wants clockwise front faces, so the counter-clockwise
            // A→B→C→D order `emit_quad` derives has to be flipped.
            flip_winding: CLOCKWISE_FRONT,
            quads_emitted: 0,
            chunks_meshed: 0,
        }
    }

    /// Just the padded neighbour-aware copy, with no sweeping. Exposed so
    /// `meshbench` can report how much of the per-chunk cost is the extraction
    /// and how much is the sweep — they optimise differently.
    pub fn prepare_only(&mut self, world: &World, ci: usize) {
        world.fill_padded(ci, &mut self.mat, &mut self.light);
    }

    /// Mesh chunk `ci` of `world` into `out`. `out` is cleared first.
    pub fn mesh(&mut self, world: &World, ci: usize, out: &mut MeshBuf) {
        out.clear();
        world.fill_padded(ci, &mut self.mat, &mut self.light);
        self.chunks_meshed += 1;

        for axis in 0..3usize {
            let u = (axis + 1) % 3;
            let v = (axis + 2) % 3;
            for (si, sign) in [1i32, -1].into_iter().enumerate() {
                let dir = axis * 2 + si;
                for s in 0..CHUNK_EDGE {
                    if !self.build_mask(axis, u, v, s, sign) {
                        continue;
                    }
                    self.merge_slice(axis, u, v, s, sign, dir, out);
                }
            }
        }
    }

    /// Fill `self.mask` for one slice. Returns `false` if the slice is blank.
    ///
    /// The padded index is a linear function of the three coordinates, so the
    /// whole sweep is a pointer walk: one add per cell along u, one per row
    /// along v, and no 3-D index arithmetic inside the loop. That matters more
    /// than anything else in the mesher — this loop runs 6 x 32 x 1024 times per
    /// chunk whatever the geometry looks like, so it, not the quad count, is
    /// what sets the p50.
    fn build_mask(&mut self, axis: usize, u: usize, v: usize, s: usize, sign: i32) -> bool {
        let sa = PAD_STRIDE[axis];
        let su = PAD_STRIDE[u];
        let sv = PAD_STRIDE[v];
        // Padded coordinates are chunk-local + 1, so (uu, vv) = (0, 0) sits at
        // +su +sv from the slice's origin.
        let base = (s + 1) * sa + su + sv;
        let mat = &self.mat[..];
        let light = &self.light[..];
        let mask = &mut self.mask[..];
        let mut any = false;

        if sign > 0 {
            for vv in 0..CHUNK_EDGE {
                let mut i = base + vv * sv;
                let row = vv * CHUNK_EDGE;
                for uu in 0..CHUNK_EDGE {
                    let m = mat[i];
                    let ni = i + sa;
                    mask[row + uu] = if m == AIR || mat[ni] != AIR {
                        0
                    } else {
                        any = true;
                        (u16::from(m) << 4) | u16::from(light[ni])
                    };
                    i += su;
                }
            }
        } else {
            for vv in 0..CHUNK_EDGE {
                let mut i = base + vv * sv;
                let row = vv * CHUNK_EDGE;
                for uu in 0..CHUNK_EDGE {
                    let m = mat[i];
                    let ni = i - sa;
                    mask[row + uu] = if m == AIR || mat[ni] != AIR {
                        0
                    } else {
                        any = true;
                        (u16::from(m) << 4) | u16::from(light[ni])
                    };
                    i += su;
                }
            }
        }
        any
    }

    #[allow(clippy::too_many_arguments)]
    fn merge_slice(
        &mut self,
        axis: usize,
        u: usize,
        v: usize,
        s: usize,
        sign: i32,
        dir: usize,
        out: &mut MeshBuf,
    ) {
        let plane = if sign > 0 { s + 1 } else { s };
        let shade = FACE_SHADE[dir];

        for vv in 0..CHUNK_EDGE {
            let mut uu = 0usize;
            while uu < CHUNK_EDGE {
                let key = self.mask[vv * CHUNK_EDGE + uu];
                if key == 0 {
                    uu += 1;
                    continue;
                }
                // Widen along u.
                let mut w = 1usize;
                while uu + w < CHUNK_EDGE && self.mask[vv * CHUNK_EDGE + uu + w] == key {
                    w += 1;
                }
                // Then grow along v while the whole run matches.
                let mut h = 1usize;
                'grow: while vv + h < CHUNK_EDGE {
                    for t in 0..w {
                        if self.mask[(vv + h) * CHUNK_EDGE + uu + t] != key {
                            break 'grow;
                        }
                    }
                    h += 1;
                }
                for dy in 0..h {
                    for dx in 0..w {
                        self.mask[(vv + dy) * CHUNK_EDGE + uu + dx] = 0;
                    }
                }

                self.emit_quad(axis, u, v, plane, uu, vv, w, h, key, shade, sign, out);
                uu += w;
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_quad(
        &mut self,
        axis: usize,
        u: usize,
        v: usize,
        plane: usize,
        uu: usize,
        vv: usize,
        w: usize,
        h: usize,
        key: u16,
        shade: f32,
        sign: i32,
        out: &mut MeshBuf,
    ) {
        let material = usize::from(key >> 4);
        let light = f32::from(key & 0xF);
        // 0.25 ambient floor so unlit faces are dark but not black.
        let lf = 0.25 + 0.75 * (light / 15.0);
        let base = PALETTE[material.min(MAT_COUNT - 1)];
        let f = shade * lf;
        let colour = [base[0] * f, base[1] * f, base[2] * f, 1.0];

        let corner = |du: usize, dv: usize| -> [f32; 3] {
            let mut c = [0f32; 3];
            c[axis] = plane as f32;
            c[u] = (uu + du) as f32;
            c[v] = (vv + dv) as f32;
            c
        };
        let a = corner(0, 0);
        let b = corner(w, 0);
        let c = corner(w, h);
        let d = corner(0, h);

        let v0 = i32::try_from(out.pos.len()).unwrap_or(0);
        out.pos.extend_from_slice(&[a, b, c, d]);
        out.col.extend_from_slice(&[colour; 4]);

        // A→B→C→D walks +u then +v, so by the right-hand rule (e_u × e_v =
        // e_axis) that order is counter-clockwise seen from the +axis side.
        // `positive_is_ccw` is therefore true for sign > 0 and false for sign < 0;
        // `flip_winding` then converts to whichever handedness Godot wants.
        let positive_is_ccw = sign > 0;
        let ccw = positive_is_ccw != self.flip_winding;
        if ccw {
            out.idx
                .extend_from_slice(&[v0, v0 + 1, v0 + 2, v0, v0 + 2, v0 + 3]);
        } else {
            out.idx
                .extend_from_slice(&[v0, v0 + 2, v0 + 1, v0, v0 + 3, v0 + 2]);
        }
        self.quads_emitted += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{World, chunk_index, chunk_origin};

    #[test]
    fn one_floating_voxel_is_six_quads() {
        let mut w = World::new_empty(0);
        // Middle of chunk (5, 1, 5): high up, surrounded by air.
        let (ox, oy, oz) = chunk_origin(chunk_index(5, 1, 5));
        w.set_voxel(ox + 16, oy + 16, oz + 16, 1);
        let mut m = Mesher::new();
        let mut out = MeshBuf::default();
        m.mesh(&w, chunk_index(5, 1, 5), &mut out);
        assert_eq!(out.vertex_count(), 24, "6 quads x 4 verts");
        assert_eq!(out.index_count(), 36);
    }

    #[test]
    fn a_flat_slab_merges_into_one_quad_per_face() {
        let mut w = World::new_empty(0);
        let ci = chunk_index(3, 1, 3);
        let (ox, oy, oz) = chunk_origin(ci);
        // Full 32x32 layer inside the chunk.
        for z in 0..32 {
            for x in 0..32 {
                w.set_voxel(ox + x, oy + 10, oz + z, 1);
            }
        }
        let mut m = Mesher::new();
        let mut out = MeshBuf::default();
        m.mesh(&w, ci, &mut out);
        // Top and bottom merge to one quad each; the four sides are one quad
        // each (32 x 1 strips). 6 quads total.
        assert_eq!(
            out.vertex_count(),
            24,
            "greedy merge failed: {} verts",
            out.vertex_count()
        );
    }

    #[test]
    fn interior_faces_across_a_chunk_border_are_not_emitted() {
        let mut w = World::new_empty(0);
        let ci = chunk_index(4, 1, 4);
        let (ox, oy, oz) = chunk_origin(ci);
        // Solid column filling the chunk's whole x span and spilling one voxel
        // into the +x neighbour, at a single (y, z).
        for x in 0..=32 {
            w.set_voxel(ox + x, oy + 5, oz + 5, 1);
        }
        let mut m = Mesher::new();
        let mut out = MeshBuf::default();
        m.mesh(&w, ci, &mut out);
        // The +x face at the chunk boundary is covered by the neighbour voxel,
        // so only the -x cap plus 4 side strips survive: 5 quads.
        assert_eq!(
            out.index_count() / 6,
            5,
            "quads = {}",
            out.index_count() / 6
        );
    }

    #[test]
    fn meshing_is_deterministic_for_a_given_world() {
        let w = World::generate(3);
        let mut m = Mesher::new();
        let mut a = MeshBuf::default();
        let mut b = MeshBuf::default();
        for ci in [200usize, 201, 250] {
            m.mesh(&w, ci, &mut a);
            m.mesh(&w, ci, &mut b);
            assert_eq!(a.pos, b.pos);
            assert_eq!(a.idx, b.idx);
        }
    }
}
