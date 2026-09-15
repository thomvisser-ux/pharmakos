// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The six-direction greedy sweep: [`ChunkView`] in, [`MeshBuffers`] out.
//!
//! For each of the six face directions, sweep the 32 slices of the chunk; build a 32-by-32
//! mask of *visible* faces keyed by `(material, light)`; merge each maximal rectangle of
//! equal key into one quad. Neighbour-aware: the chunk is copied into a 34-cubed buffer
//! with a one-voxel border of its six neighbours first, so a face that is interior across a
//! chunk boundary is never emitted.
//!
//! **T-junctions are accepted, not prevented.** The padded copy stops interior faces being
//! emitted; it has nothing to do with T-junctions, which greedy meshing produces by
//! construction wherever one merged quad abuts several smaller coplanar ones — inside a
//! chunk as much as across a seam. Spike G1 observed no crack in the fixed vista at
//! 1920 x 1080 with MSAA off, because every vertex coordinate is a small exact integer in
//! `f32` and there is nothing to interpolate. That is a measurement on that configuration,
//! not a guarantee.
//!
//! # Scratch, and why it lives on the struct
//!
//! The Windows allocator is markedly slower than glibc's for the many short-lived vectors a
//! naive mesher produces (G1 plan section 6), so the padded copy and the mask live on
//! [`Mesher`] and the output arrays keep their capacity across meshings. The steady state
//! is **zero allocations per meshing**, asserted by `tests/allocations.rs`.

use std::fmt;

use crate::{
    AIR, CHUNK_EDGE, ChunkInput, ChunkView, FACE_AREA, Face, LightParams, MeshBuffers, Surface,
};

/// Edge of the padded copy: the chunk plus one voxel of each neighbour.
const PAD_EDGE: usize = CHUNK_EDGE + 2;

/// Voxels in the padded copy, 39 304.
const PAD_VOLUME: usize = PAD_EDGE * PAD_EDGE * PAD_EDGE;

/// Step in the padded buffer for one unit of x, y and z — the same precedence as
/// [`crate::voxel_index`]: x fastest, then z, then y.
const PAD_STRIDE: [usize; 3] = [1, PAD_EDGE * PAD_EDGE, PAD_EDGE];

/// Vertices a chunk may emit before `u16` indices stop being enough.
///
/// Spike G1 measured 6 660 on a cratered 32-cubed chunk, about ten times under this, so a
/// chunk never needs a split surface or a 32-bit index buffer. Godot also stores the
/// surface's indices as 16-bit below this cap, which is why an in-place patch of the index
/// region cannot hand it 32-bit ones. Exceeding it is [`MeshError::TooManyVertices`],
/// never a truncation.
pub const MAX_VERTICES: usize = 65_536;

/// Godot 4.7 treats **clockwise** winding as front-facing under `CULL_BACK` for triangle
/// primitives — which is what Godot's own `Mesh` documentation says, and what spike G1's
/// vista screenshot confirmed: the first run came back showing the inside of the terrain
/// (sky through the ground, box lids shaded as undersides) with counter-clockwise quads,
/// and emitting them clockwise fixed it (G1 section 10.7). The name, the value and the
/// behaviour say the same thing, and no timing number would ever catch a mistake here.
const CLOCKWISE_FRONT: bool = true;

/// The material palette, one RGB triple per material id. Id 0 is air and is never emitted.
///
/// PLACEHOLDER: these are spike G1's colours, carried over so that the vista renders as
/// something rather than as grey. They are art, not a rules-table row — nothing in
/// `rules/rules.v1.json` names a palette — so they cannot be a parameter the caller fills.
/// Owner replaces them at S6's art polish, and the geometry golden moves when they do.
pub const PALETTE: [[u8; 3]; 7] = [
    [0, 0, 0],       // 0 air, never emitted
    [133, 133, 143], // 1 stone
    [107, 79, 51],   // 2 dirt
    [79, 143, 64],   // 3 grass
    [204, 163, 66],  // 4 ore A
    [71, 168, 189],  // 5 ore B
    [189, 184, 173], // 6 concrete
];

/// The per-face shading factor, in 256ths, indexed by [`Face::index`].
///
/// The classic six-sided shade: the top face unshaded, the underside darkest. It is folded
/// into the vertex colour rather than lit by a light, which is the look the game wants and
/// what keeps path B's buffers unambiguous (G1 section 10.2).
///
/// PLACEHOLDER: spike G1's factors, rounded to 256ths. Art, like [`PALETTE`]; owner at S6.
pub const FACE_SHADE_256: [u32; 6] = [
    184, // -x  0.72
    210, // +x  0.82
    115, // -y  0.45
    256, // +y  1.00
    159, // -z  0.62
    235, // +z  0.92
];

/// The ambient floor, in 256ths: an unlit face is dark, not black.
const AMBIENT_256: u32 = 64;

/// What the light value is worth above the ambient floor, in 256ths.
/// `AMBIENT_256 + LIGHT_SPAN_256` is exactly 256, so a fully lit face is unattenuated.
const LIGHT_SPAN_256: u32 = 192;

/// One channel of a quad's colour, quantised.
///
/// **The formula, in integers, stated once so Windows and Linux cannot disagree.** Nothing
/// here is a float: a float multiply-add would round per platform, and the vertex colours
/// are in the geometry digest that the CI matrix byte-compares.
///
/// ```text
///   light_256 = AMBIENT_256 + (LIGHT_SPAN_256 * light) / light_max     (0 -> 64, max -> 256)
///   numerator = base * shade_256 * light_256                           (<= 255 * 256 * 256)
///   channel   = (numerator + 32_768) >> 16                             (round half up)
/// ```
///
/// The shift is exact division by 65 536 — the product of the two 256th scales — and the
/// addend is half of it, so the rounding is half-up and the maximum, `255 * 256 * 256`,
/// comes back as exactly 255.
///
/// `light_max` of zero is treated as "no light scale", giving the ambient floor: the
/// alternative is a division by zero in the one place a caller can reach with a bad
/// parameter, and [`crate::LightParams`] refuses that value at its own boundary anyway.
#[must_use]
pub fn quantise_channel(base: u8, shade_256: u32, light: u8, light_max: u8) -> u8 {
    let scaled = LIGHT_SPAN_256
        .saturating_mul(u32::from(light.min(light_max)))
        .checked_div(u32::from(light_max))
        .unwrap_or(0);
    let light_256 = AMBIENT_256.saturating_add(scaled);
    let numerator = u32::from(base)
        .saturating_mul(shade_256)
        .saturating_mul(light_256);
    let channel = numerator.saturating_add(32_768) >> 16_u32;
    u8::try_from(channel.min(255)).unwrap_or(255)
}

/// Why a chunk could not be meshed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MeshError {
    /// The chunk would need more than [`MAX_VERTICES`] vertices, so `u16` indices would
    /// not reach them. The output buffers are left empty rather than truncated.
    TooManyVertices {
        /// How many vertices had been emitted when the cap was reached. The real figure is
        /// larger; the sweep stops rather than finishing work it cannot use.
        emitted: usize,
    },
}

impl fmt::Display for MeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::TooManyVertices { emitted } => write!(
                f,
                "a chunk needs more than {MAX_VERTICES} vertices for 16-bit indices \
                 ({emitted} emitted before the sweep stopped). Spike G1 measured 6 660 at \
                 worst, so this is a correctness failure, not a budget one."
            ),
        }
    }
}

impl std::error::Error for MeshError {}

/// The greedy mesher, with its scratch.
///
/// Build one and keep it: the padded copy and the mask are allocated once, and
/// [`Mesher::mesh_chunk_into`] reuses the caller's [`MeshBuffers`] capacity, so a warm
/// mesher allocates nothing.
///
/// It holds [`LightParams`] because the colour ramp needs `light_max` — the same value the
/// bake seeds the sky with. One struct carries both uses deliberately: a bake and a shade
/// that disagreed about the maximum would give a world correctly lit and wrongly coloured,
/// and the maximum is a rules-table row, so the mesher must be told it rather than assume
/// one.
#[derive(Debug)]
pub struct Mesher {
    /// The sky's light value, and the value a fully lit face is shaded at.
    params: LightParams,
    /// The 34-cubed padded material copy.
    pad_materials: Vec<u8>,
    /// The 34-cubed padded light copy.
    pad_light: Vec<u8>,
    /// One slice's face mask, `(material << 8) | light`, zero for "no face".
    mask: Vec<u32>,
    /// Quads emitted since this mesher was built — a counter for the perf alarm, not state.
    quads: u64,
    /// Chunks meshed since this mesher was built.
    chunks: u64,
}

/// The eight numbers one quad needs, bundled so the emit function keeps a readable
/// signature.
#[derive(Clone, Copy, Debug)]
struct Quad {
    /// Which axis the face is perpendicular to: 0 for x, 1 for y, 2 for z.
    axis: usize,
    /// The free axis the mask's first index runs along.
    axis_u: usize,
    /// The free axis the mask's second index runs along.
    axis_v: usize,
    /// The plane the quad sits in, along `axis`, in chunk-local voxels.
    plane: usize,
    /// The quad's origin along `axis_u`.
    origin_u: usize,
    /// The quad's origin along `axis_v`.
    origin_v: usize,
    /// The quad's extent along `axis_u`.
    width: usize,
    /// The quad's extent along `axis_v`.
    height: usize,
    /// True when the face points along the positive axis direction.
    positive: bool,
    /// The mask key, `(material << 8) | light`.
    key: u32,
}

impl Mesher {
    /// A mesher with its scratch allocated. Reuse it.
    #[must_use]
    pub fn new(params: LightParams) -> Self {
        Self {
            params,
            pad_materials: vec![AIR; PAD_VOLUME],
            pad_light: vec![0_u8; PAD_VOLUME],
            mask: vec![0_u32; FACE_AREA],
            quads: 0,
            chunks: 0,
        }
    }

    /// The light parameters this mesher shades with.
    #[must_use]
    pub const fn params(&self) -> LightParams {
        self.params
    }

    /// Quads emitted since this mesher was built.
    #[must_use]
    pub const fn quads_emitted(&self) -> u64 {
        self.quads
    }

    /// Chunks meshed since this mesher was built.
    #[must_use]
    pub const fn chunks_meshed(&self) -> u64 {
        self.chunks
    }

    /// Meshes one chunk into `out`, which is cleared first and keeps its capacity.
    ///
    /// # Winding
    ///
    /// The quad's four corners are walked `A -> B -> C -> D`, `+u` then `+v`:
    ///
    /// ```text
    ///        v
    ///        ^
    ///   D----------C          A = (origin_u,         origin_v)
    ///   |          |          B = (origin_u + width, origin_v)
    ///   |   face   |          C = (origin_u + width, origin_v + height)
    ///   |          |          D = (origin_u,         origin_v + height)
    ///   A----------B---> u
    /// ```
    ///
    /// `e_u x e_v = e_axis` for every one of the three axis choices below, so that walk is
    /// **counter-clockwise seen from the positive-axis side** and clockwise seen from the
    /// negative one. Godot's front face under `CULL_BACK` is **clockwise**
    /// (`CLOCKWISE_FRONT`, the crate-private constant this module opens with), so:
    ///
    /// * a face pointing along `+axis` is seen from `+axis` and its walk reads
    ///   counter-clockwise, so it is emitted **reversed**: `A C B`, `A D C`;
    /// * a face pointing along `-axis` is seen from `-axis` and its walk already reads
    ///   clockwise, so it is emitted as it stands: `A B C`, `A C D`.
    ///
    /// This is why a tidy-up of the quad walk moves the geometry golden, and why inverted
    /// winding is invisible in a digest and obvious in the vista screenshot.
    ///
    /// # Errors
    ///
    /// [`MeshError::TooManyVertices`] when the chunk would need more than [`MAX_VERTICES`]
    /// vertices. `out` is left empty in that case: half a chunk rendered is worse than
    /// none, and a caller that ignored the error would upload stale winding.
    pub fn mesh_chunk_into(
        &mut self,
        view: &ChunkView<'_>,
        out: &mut MeshBuffers,
    ) -> Result<(), MeshError> {
        out.clear();
        self.fill_padded(view);
        self.chunks = self.chunks.saturating_add(1);

        for axis in 0..3_usize {
            let axis_u = (axis + 1) % 3;
            let axis_v = (axis + 2) % 3;
            for positive in [true, false] {
                for slice in 0..CHUNK_EDGE {
                    if !self.build_mask(axis, axis_u, axis_v, slice, positive) {
                        continue;
                    }
                    self.merge_slice(axis, axis_u, axis_v, slice, positive, out)?;
                }
            }
        }

        if !out.indices.is_empty() {
            out.surfaces.push(Surface {
                first_vertex: 0,
                vertex_count: u32::try_from(out.positions.len()).unwrap_or(u32::MAX),
                first_index: 0,
                index_count: u32::try_from(out.indices.len()).unwrap_or(u32::MAX),
            });
        }
        Ok(())
    }

    /// Copies the chunk and its six borders into the 34-cubed padded buffers.
    ///
    /// A border the caller did not hand in stays air at light 0 — [`ChunkView`]'s "open
    /// sky" rule — because the buffers are reset to exactly that first.
    fn fill_padded(&mut self, view: &ChunkView<'_>) {
        self.pad_materials.fill(AIR);
        self.pad_light.fill(0);

        // The interior: one contiguous 32-byte row per (y, z), which is the whole reason
        // the voxel order puts x fastest.
        for y in 0..CHUNK_EDGE {
            for z in 0..CHUNK_EDGE {
                let from = (y * CHUNK_EDGE + z) * CHUNK_EDGE;
                let to = (y + 1) * PAD_STRIDE[1] + (z + 1) * PAD_STRIDE[2] + 1;
                copy_row(&mut self.pad_materials, to, view.materials(), from);
                copy_row(&mut self.pad_light, to, view.light(), from);
            }
        }

        for face in Face::ALL {
            let Some(border) = view.border(face) else {
                continue;
            };
            let outer = if face.is_positive() { PAD_EDGE - 1 } else { 0 };
            for first in 0..CHUNK_EDGE {
                for second in 0..CHUNK_EDGE {
                    // The border's own indexing: the two free axes, x fastest, then z,
                    // then y. `first` is the faster of the two.
                    let source = second * CHUNK_EDGE + first;
                    let target = match face.axis() {
                        // +/-x: free axes are z (faster) and y.
                        0 => {
                            second.saturating_add(1) * PAD_STRIDE[1]
                                + first.saturating_add(1) * PAD_STRIDE[2]
                                + outer
                        }
                        // +/-y: free axes are x (faster) and z.
                        1 => {
                            outer * PAD_STRIDE[1]
                                + second.saturating_add(1) * PAD_STRIDE[2]
                                + first.saturating_add(1)
                        }
                        // +/-z: free axes are x (faster) and y.
                        _ => {
                            second.saturating_add(1) * PAD_STRIDE[1]
                                + outer * PAD_STRIDE[2]
                                + first.saturating_add(1)
                        }
                    };
                    set_byte(
                        &mut self.pad_materials,
                        target,
                        byte_at(border.materials(), source),
                    );
                    set_byte(&mut self.pad_light, target, byte_at(border.light(), source));
                }
            }
        }
    }

    /// Fills [`Mesher::mask`] for one slice. Returns `false` when the slice is blank.
    ///
    /// The padded index is a linear function of the three coordinates, so the sweep is a
    /// pointer walk: one add per cell along u, one per row along v, and no three-dimensional
    /// index arithmetic inside the loop. That matters more than anything else here — this
    /// loop runs 6 x 32 x 1024 times per chunk whatever the geometry looks like, so it, not
    /// the quad count, is what sets the p50.
    fn build_mask(
        &mut self,
        axis: usize,
        axis_u: usize,
        axis_v: usize,
        slice: usize,
        positive: bool,
    ) -> bool {
        let stride_axis = stride(axis);
        let stride_u = stride(axis_u);
        let stride_v = stride(axis_v);
        // Padded coordinates are chunk-local + 1, so (0, 0) sits one step along each free
        // axis from the slice's origin.
        let base = (slice + 1) * stride_axis + stride_u + stride_v;
        let mut any = false;

        for step_v in 0..CHUNK_EDGE {
            let mut at = base + step_v * stride_v;
            let row = step_v * CHUNK_EDGE;
            for step_u in 0..CHUNK_EDGE {
                let here = byte_at(&self.pad_materials, at);
                let across = if positive {
                    at.saturating_add(stride_axis)
                } else {
                    at.saturating_sub(stride_axis)
                };
                let key = if here == AIR || byte_at(&self.pad_materials, across) != AIR {
                    0
                } else {
                    any = true;
                    (u32::from(here) << 8_u32) | u32::from(byte_at(&self.pad_light, across))
                };
                set_mask(&mut self.mask, row + step_u, key);
                at = at.saturating_add(stride_u);
            }
        }
        any
    }

    /// Merges the mask's maximal rectangles and emits one quad each.
    fn merge_slice(
        &mut self,
        axis: usize,
        axis_u: usize,
        axis_v: usize,
        slice: usize,
        positive: bool,
        out: &mut MeshBuffers,
    ) -> Result<(), MeshError> {
        let plane = if positive { slice + 1 } else { slice };

        for origin_v in 0..CHUNK_EDGE {
            let mut origin_u = 0_usize;
            while origin_u < CHUNK_EDGE {
                let key = mask_at(&self.mask, origin_v * CHUNK_EDGE + origin_u);
                if key == 0 {
                    origin_u += 1;
                    continue;
                }
                let width = self.widen(origin_u, origin_v, key);
                let height = self.grow(origin_u, origin_v, width, key);
                for down in 0..height {
                    for across in 0..width {
                        set_mask(
                            &mut self.mask,
                            (origin_v + down) * CHUNK_EDGE + origin_u + across,
                            0,
                        );
                    }
                }
                self.emit_quad(
                    Quad {
                        axis,
                        axis_u,
                        axis_v,
                        plane,
                        origin_u,
                        origin_v,
                        width,
                        height,
                        positive,
                        key,
                    },
                    out,
                )?;
                origin_u += width;
            }
        }
        Ok(())
    }

    /// How far the run of equal keys extends along u from `(origin_u, origin_v)`.
    fn widen(&self, origin_u: usize, origin_v: usize, key: u32) -> usize {
        let mut width = 1_usize;
        while origin_u + width < CHUNK_EDGE
            && mask_at(&self.mask, origin_v * CHUNK_EDGE + origin_u + width) == key
        {
            width += 1;
        }
        width
    }

    /// How many whole rows of `width` equal keys sit below `(origin_u, origin_v)`.
    fn grow(&self, origin_u: usize, origin_v: usize, width: usize, key: u32) -> usize {
        let mut height = 1_usize;
        'rows: while origin_v + height < CHUNK_EDGE {
            for across in 0..width {
                if mask_at(
                    &self.mask,
                    (origin_v + height) * CHUNK_EDGE + origin_u + across,
                ) != key
                {
                    break 'rows;
                }
            }
            height += 1;
        }
        height
    }

    /// Emits one quad: four vertices, six indices, one flat colour.
    ///
    /// The winding is [`Mesher::mesh_chunk_into`]'s diagram, and the colour is
    /// [`quantise_channel`]'s formula per channel.
    fn emit_quad(&mut self, quad: Quad, out: &mut MeshBuffers) -> Result<(), MeshError> {
        let first = out.positions.len();
        if first.saturating_add(4) > MAX_VERTICES {
            out.clear();
            return Err(MeshError::TooManyVertices { emitted: first });
        }

        let material = usize::try_from(quad.key >> 8_u32).unwrap_or(0);
        let light = u8::try_from(quad.key & 0xFF).unwrap_or(0);
        let base = PALETTE
            .get(material)
            .copied()
            .unwrap_or([255_u8, 0_u8, 255_u8]);
        let face = face_of(quad.axis, quad.positive);
        let shade = FACE_SHADE_256.get(face.index()).copied().unwrap_or(256);
        // The light in the key is the *neighbour* cell's — the air cell the face looks out
        // into — and the ramp runs from 0 to the caller's `light_max`, which is the value
        // the bake seeds the sky with. That is why the mesher is told the parameter rather
        // than assuming 15: a rules table with a different maximum would otherwise render
        // the whole world at the wrong brightness with nothing to point at.
        let light_max = self.params.light_max();
        let colour = [
            quantise_channel(byte_at3(base, 0), shade, light, light_max),
            quantise_channel(byte_at3(base, 1), shade, light, light_max),
            quantise_channel(byte_at3(base, 2), shade, light, light_max),
            255,
        ];

        let corner = |along_u: usize, along_v: usize| -> [f32; 3] {
            let mut point = [0.0_f32; 3];
            set_coord(&mut point, quad.axis, quad.plane);
            set_coord(&mut point, quad.axis_u, quad.origin_u + along_u);
            set_coord(&mut point, quad.axis_v, quad.origin_v + along_v);
            point
        };
        out.positions.push(corner(0, 0));
        out.positions.push(corner(quad.width, 0));
        out.positions.push(corner(quad.width, quad.height));
        out.positions.push(corner(0, quad.height));

        let mut normal = [0.0_f32; 3];
        if let Some(slot) = normal.get_mut(quad.axis) {
            *slot = if quad.positive { 1.0 } else { -1.0 };
        }
        for _ in 0..4 {
            out.normals.push(normal);
            out.colours.push(colour);
        }

        let base_index = u16::try_from(first).unwrap_or(u16::MAX);
        let pattern: [u16; 6] = if quad.positive == CLOCKWISE_FRONT {
            // Seen from the visible side the walk reads counter-clockwise: reverse it.
            [0, 2, 1, 0, 3, 2]
        } else {
            [0, 1, 2, 0, 2, 3]
        };
        for offset in pattern {
            out.indices.push(base_index.saturating_add(offset));
        }
        self.quads = self.quads.saturating_add(1);
        Ok(())
    }
}

/// The face for one axis and sign.
fn face_of(axis: usize, positive: bool) -> Face {
    match (axis, positive) {
        (0, true) => Face::PosX,
        (0, false) => Face::NegX,
        (1, true) => Face::PosY,
        (1, false) => Face::NegY,
        (_, true) => Face::PosZ,
        (_, false) => Face::NegZ,
    }
}

/// The padded buffer's stride for one axis.
fn stride(axis: usize) -> usize {
    PAD_STRIDE.get(axis).copied().unwrap_or(1)
}

/// One byte of a slice, or air past its end. `clippy::indexing_slicing` is denied
/// workspace-wide and the wall does not lift it; every read in the sweep goes through here.
fn byte_at(slice: &[u8], at: usize) -> u8 {
    slice.get(at).copied().unwrap_or(AIR)
}

/// One byte of an RGB triple.
fn byte_at3(triple: [u8; 3], at: usize) -> u8 {
    triple.get(at).copied().unwrap_or(0)
}

/// Writes one byte, ignoring a write past the end.
fn set_byte(slice: &mut [u8], at: usize, value: u8) {
    if let Some(slot) = slice.get_mut(at) {
        *slot = value;
    }
}

/// One mask entry, or "no face" past its end.
fn mask_at(mask: &[u32], at: usize) -> u32 {
    mask.get(at).copied().unwrap_or(0)
}

/// Writes one mask entry, ignoring a write past the end.
fn set_mask(mask: &mut [u32], at: usize, value: u32) {
    if let Some(slot) = mask.get_mut(at) {
        *slot = value;
    }
}

/// Copies one 32-byte row out of a chunk array into the padded buffer.
fn copy_row(into: &mut [u8], to: usize, from_slice: &[u8], from: usize) {
    let Some(source) = from_slice.get(from..from + CHUNK_EDGE) else {
        return;
    };
    if let Some(target) = into.get_mut(to..to + CHUNK_EDGE) {
        target.copy_from_slice(source);
    }
}

/// Writes one coordinate of a vertex position.
///
/// The value is a chunk-local voxel coordinate, 0 to 32 inclusive, so it is exact in `f32`
/// and reaches the float side through `f32::from(u8)` rather than through a cast — there is
/// no rounding to disagree about across platforms.
fn set_coord(point: &mut [f32; 3], axis: usize, value: usize) {
    if let Some(slot) = point.get_mut(axis) {
        *slot = f32::from(u8::try_from(value).unwrap_or(u8::MAX));
    }
}

/// Meshes one owned chunk, allocating as it goes.
///
/// The convenience form, for tests, goldens and any caller meshing one chunk in isolation.
/// A caller meshing chunks every frame builds a [`Mesher`] and calls
/// [`Mesher::mesh_chunk_into`] instead, which allocates nothing once warm.
///
/// # Errors
///
/// [`MeshError::TooManyVertices`], as [`Mesher::mesh_chunk_into`].
pub fn mesh_chunk(input: &ChunkInput, params: LightParams) -> Result<MeshBuffers, MeshError> {
    let mut mesher = Mesher::new(params);
    let mut out = MeshBuffers::empty();
    mesher.mesh_chunk_into(&input.as_view(), &mut out)?;
    Ok(out)
}

/// How many bytes a chunk's surface occupies on the GPU, at the format spike G1 measured:
/// 12 bytes of position plus 4 of RGBA colour per vertex, and a 16-bit index each.
///
/// This is the figure [`DrainQueue`](crate::DrainQueue)'s byte budget is spent in. It is
/// the GPU-side cost, not the FFI cost: G1 reports both because they differ by more than a
/// factor of two, and a budget set against the wrong one is a budget set wrong.
#[must_use]
pub fn gpu_surface_bytes(buffers: &MeshBuffers) -> usize {
    buffers
        .positions()
        .len()
        .saturating_mul(16)
        .saturating_add(buffers.indices().len().saturating_mul(2))
}

#[cfg(test)]
mod tests {
    use super::{MAX_VERTICES, MeshError, Mesher, gpu_surface_bytes, mesh_chunk, quantise_channel};
    use crate::{
        AIR, CHUNK_EDGE, CHUNK_VOLUME, ChunkInput, FACE_AREA, Face, LightParams, MeshBuffers,
        NeighbourBorder, voxel_index,
    };

    /// The committed rules table's pair, as T12 will fill it.
    fn params() -> LightParams {
        LightParams::new(15, 1).expect("light_max 15 and light_atten 1 are the committed rows")
    }

    /// A chunk of all air with the given voxels set to material 1, everything at full light.
    fn chunk_with(solid: &[(usize, usize, usize)]) -> ChunkInput {
        let mut materials = vec![AIR; CHUNK_VOLUME];
        let light = vec![15_u8; CHUNK_VOLUME];
        for &(x, y, z) in solid {
            if let Some(slot) = materials.get_mut(voxel_index(x, y, z)) {
                *slot = 1;
            }
        }
        ChunkInput::new(materials, light, ChunkInput::no_borders())
            .expect("both arrays are CHUNK_VOLUME long")
    }

    #[test]
    fn one_floating_voxel_is_six_quads() {
        let buffers = mesh_chunk(&chunk_with(&[(16, 16, 16)]), params()).expect("under the cap");
        assert_eq!(buffers.positions().len(), 24, "6 quads x 4 vertices");
        assert_eq!(buffers.indices().len(), 36);
        assert_eq!(buffers.surface_count(), 1);
        assert_eq!(buffers.normals().len(), 24);
        assert_eq!(buffers.colours().len(), 24);
        let surface = buffers.surfaces().first().copied().unwrap_or_default();
        assert_eq!(surface.vertex_count, 24);
        assert_eq!(surface.index_count, 36);
    }

    #[test]
    fn a_flat_layer_merges_into_one_quad_per_face() {
        let mut solid = Vec::new();
        for x in 0..CHUNK_EDGE {
            for z in 0..CHUNK_EDGE {
                solid.push((x, 10, z));
            }
        }
        let buffers = mesh_chunk(&chunk_with(&solid), params()).expect("under the cap");
        // Top and bottom are one quad each; the four sides are one 32 x 1 strip each.
        assert_eq!(
            buffers.quad_count(),
            6,
            "greedy merge failed: {} quads",
            buffers.quad_count()
        );
    }

    #[test]
    fn a_border_hides_the_face_across_it() {
        let solid: Vec<(usize, usize, usize)> = (0..CHUNK_EDGE).map(|x| (x, 5, 5)).collect();
        let mut borders = ChunkInput::no_borders();
        // The +x neighbour is solid exactly where the row meets it: (z, y) = (5, 5), and a
        // plus-or-minus-x border is indexed z + 32 * y.
        let mut materials = vec![AIR; FACE_AREA];
        if let Some(slot) = materials.get_mut(5 + CHUNK_EDGE * 5) {
            *slot = 1;
        }
        if let Some(slot) = borders.get_mut(Face::PosX.index()) {
            *slot =
                Some(NeighbourBorder::new(materials, vec![15; FACE_AREA]).expect("FACE_AREA long"));
        }
        let base = chunk_with(&solid);
        let with_border =
            ChunkInput::new(base.materials().to_vec(), base.light().to_vec(), borders)
                .expect("both arrays are CHUNK_VOLUME long");

        let open = mesh_chunk(&base, params()).expect("under the cap");
        let closed = mesh_chunk(&with_border, params()).expect("under the cap");
        assert_eq!(open.quad_count(), 6, "-x, +x and four strips");
        assert_eq!(
            closed.quad_count(),
            5,
            "the +x cap is interior across the border and must not be emitted"
        );
    }

    #[test]
    fn the_light_value_splits_a_quad() {
        let mut materials = vec![AIR; CHUNK_VOLUME];
        for x in 0..CHUNK_EDGE {
            if let Some(slot) = materials.get_mut(voxel_index(x, 0, 0)) {
                *slot = 1;
            }
        }
        // Uniform light: the top face is one merged quad.
        let uniform = ChunkInput::new(
            materials.clone(),
            vec![15_u8; CHUNK_VOLUME],
            ChunkInput::no_borders(),
        )
        .expect("both arrays are CHUNK_VOLUME long");
        // Split light: the air cell above x >= 16 is darker, so the key changes there.
        let mut light = vec![15_u8; CHUNK_VOLUME];
        for x in 16..CHUNK_EDGE {
            if let Some(slot) = light.get_mut(voxel_index(x, 1, 0)) {
                *slot = 7;
            }
        }
        let split = ChunkInput::new(materials, light, ChunkInput::no_borders())
            .expect("both arrays are CHUNK_VOLUME long");

        let uniform_quads = mesh_chunk(&uniform, params())
            .expect("under the cap")
            .quad_count();
        let split_quads = mesh_chunk(&split, params())
            .expect("under the cap")
            .quad_count();
        assert_eq!(
            split_quads,
            uniform_quads + 1,
            "the (material, light) key must split the +y quad in two"
        );
    }

    #[test]
    fn winding_is_reversed_for_positive_faces() {
        let buffers = mesh_chunk(&chunk_with(&[(0, 0, 0)]), params()).expect("under the cap");
        // Six quads, in sweep order: +x, -x, +y, -y, +z, -z. The positive ones are emitted
        // reversed (A C B / A D C) and the negative ones as they stand (A B C / A C D).
        let indices = buffers.indices();
        for quad in 0..6_usize {
            let first = quad * 6;
            let base = u16::try_from(quad * 4).expect("24 vertices fit a u16");
            let pattern: Vec<u16> = indices
                .get(first..first + 6)
                .unwrap_or_default()
                .iter()
                .map(|index| index.saturating_sub(base))
                .collect();
            let expected: Vec<u16> = if quad % 2 == 0 {
                vec![0, 2, 1, 0, 3, 2]
            } else {
                vec![0, 1, 2, 0, 2, 3]
            };
            assert_eq!(pattern, expected, "quad {quad}");
        }
    }

    #[test]
    fn the_quantiser_is_exact_at_both_ends() {
        // Full light, unshaded face: the base colour survives unchanged.
        assert_eq!(quantise_channel(255, 256, 15, 15), 255);
        assert_eq!(quantise_channel(133, 256, 15, 15), 133);
        // No light: the ambient floor is a quarter.
        assert_eq!(quantise_channel(255, 256, 0, 15), 64);
        // Light above the maximum clamps rather than overshooting.
        assert_eq!(quantise_channel(255, 256, 200, 15), 255);
        // A zero light scale cannot divide by zero.
        assert_eq!(quantise_channel(255, 256, 9, 0), 64);
    }

    #[test]
    fn gpu_bytes_follow_the_measured_format() {
        let buffers = mesh_chunk(&chunk_with(&[(1, 1, 1)]), params()).expect("under the cap");
        assert_eq!(gpu_surface_bytes(&buffers), 24 * 16 + 36 * 2);
        assert_eq!(gpu_surface_bytes(&MeshBuffers::empty()), 0);
    }

    #[test]
    fn meshing_the_same_chunk_twice_gives_the_same_bytes() {
        let chunk = chunk_with(&[(1, 1, 1), (2, 1, 1), (1, 2, 1)]);
        let mut mesher = Mesher::new(params());
        let mut first = MeshBuffers::empty();
        let mut second = MeshBuffers::empty();
        mesher
            .mesh_chunk_into(&chunk.as_view(), &mut first)
            .expect("under the cap");
        mesher
            .mesh_chunk_into(&chunk.as_view(), &mut second)
            .expect("under the cap");
        assert_eq!(first.positions(), second.positions());
        assert_eq!(first.indices(), second.indices());
        assert_eq!(first.colours(), second.colours());
        assert_eq!(mesher.chunks_meshed(), 2);
        assert!(mesher.quads_emitted() > 0);
        assert_eq!(mesher.params(), params());
    }

    #[test]
    fn the_sixteen_bit_cap_is_an_error_and_never_a_truncation() {
        // A full-chunk checkerboard of isolated voxels is 16 384 voxels x 6 quads x 4
        // vertices = 393 216, so it cannot fit 16-bit indices. The mesher must say so
        // rather than wrap an index and render a chunk of garbage.
        let mut materials = vec![AIR; CHUNK_VOLUME];
        for y in 0..CHUNK_EDGE {
            for z in 0..CHUNK_EDGE {
                for x in 0..CHUNK_EDGE {
                    if (x + y + z) % 2 == 0 {
                        if let Some(slot) = materials.get_mut(voxel_index(x, y, z)) {
                            *slot = 1;
                        }
                    }
                }
            }
        }
        let chunk = ChunkInput::new(materials, vec![15; CHUNK_VOLUME], ChunkInput::no_borders())
            .expect("both arrays are CHUNK_VOLUME long");
        match mesh_chunk(&chunk, params()) {
            Err(MeshError::TooManyVertices { emitted }) => {
                assert!(emitted <= MAX_VERTICES, "emitted {emitted}");
            }
            Ok(buffers) => panic!(
                "the checkerboard meshed to {} vertices without hitting the cap",
                buffers.positions().len()
            ),
        }
    }
}
