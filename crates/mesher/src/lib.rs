// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The greedy mesher: integer chunk data in, vertex buffers out. Role from spec section 15
//! (Architecture), rendering row — "our own greedy mesher in Rust feeds Godot
//! `RenderingServer` RIDs under a per-frame upload budget", and the same mesher meshes the
//! `.vox` models loaded with `dot_vox`, so no Godot importer addon is needed and Voxel
//! Tools stays optional.
//!
//! # Where the wall is
//!
//! This crate is the presentation side of the wall, and the line is the one spike G1
//! found and decisions log section 2.7 item 56 fixed:
//!
//! * everything up to and including the voxel edit, the dirty set and the drain order is
//!   **integer** and lives in `pharmakos-sim`;
//! * **floats begin at the first vertex coordinate**, which is here;
//! * nothing a float touches is ever read back by the sim.
//!
//! The mechanism, not the intention, is what enforces that:
//!
//! * `pharmakos-mesher` is listed in `WALLED_PACKAGES` in `xtask/src/main.rs` (and in the
//!   `clippy.toml` header), so `cargo xtask clippy` lints it in pass 2 with the float,
//!   cast, hash-map and clock allowances on the command line. Nothing in this source asks
//!   for an allowance, and no `#![allow]` on a determinism lint belongs in it;
//! * `cargo xtask wall-guard` fails the build if `sim`, `plan-core`, `verifier`,
//!   `operator` or `gateway` depends on this crate, transitively included — that list is
//!   `WALL_GUARDED_PACKAGES` in `xtask/src/main.rs`, and the sim is on it because the sim
//!   is what owns hashed state. `pharmakos-client-gdext` is the only dependant today;
//!   that it stays the only one is convention, but that no deterministic crate becomes
//!   one is checked on every run;
//! * it **links without gdext**, which is why the headless CPU proxy and the CI geometry
//!   check can reuse the real mesher instead of a copy of it. Keep it that way: a `godot`
//!   dependency here would put the engine between CI and the geometry it checks.
//!
//! Anything crossing back towards the sim crosses as an integer newtype at a named
//! boundary function (AGENTS.md section 4.9). Today nothing crosses back at all.
//!
//! # What the real mesher must honour (spike G1, section 10)
//!
//! These are measured findings, not preferences. The implementation that fills
//! [`mesh_chunk`] in is written against the real types with the spike open beside it, and
//! it has to come out on the right side of every one of them:
//!
//! * **Winding: Godot's front face for triangle primitives is CLOCKWISE under
//!   `CULL_BACK`**, which is what Godot's own `Mesh` documentation says. The quad walk is
//!   emitted reversed to achieve it. G1 section 10.7 records this as a correction: the
//!   spike's code had the geometry right and the stated fact inverted, so the constant
//!   name is worth reading twice, and the vista screenshot — not a timing number — is what
//!   catches a mistake here.
//! * **An in-place surface update must compare the index array, not the vertex and index
//!   counts.** Each quad emits `v0, v0+1, v0+2, v0, v0+2, v0+3` or the reversed pattern
//!   depending on its face sign, so two remeshes with identical counts can still need
//!   different indices when quads move between the six face directions. In G1's run, 29
//!   of the updates a count-only guard would have patched in place had a changed index
//!   array — 29 chunk surfaces that would have rendered with stale winding, that is,
//!   back-face-culled holes, until their next rebuild. No timing number would ever have
//!   shown it.
//! * **16-bit indices are sufficient, with about 10x headroom.** G1 measured a maximum of
//!   6 660 vertices on a cratered 32-cubed chunk against the 65 536 cap, so a chunk never
//!   needs a split surface or a 32-bit index buffer. [`MeshBuffers`] therefore carries
//!   `u16` indices. The corollary G1 also found: Godot stores the surface's indices as
//!   16-bit below that cap, which is why an in-place patch of the index region cannot
//!   hand it 32-bit ones.
//! * **Vertex colours carry the material and a flood-fill light value.** The face mask is
//!   keyed by `(material, light)`, so light-driven fragmentation is part of the quad
//!   merge, and the per-face shading factor plus the baked light fold into the colour
//!   (spec section 15's art-pipeline row). The flood fill that produces that light must
//!   seed **every** sky cell, not the lowest one per column, or anything under an overhang
//!   renders black and the mask key degenerates (G1 section 10.12).
//! * **Chunk borders and the light pad.** A chunk is meshed from a copy padded with one
//!   voxel of each of its six neighbours, so a face that is interior across a chunk
//!   boundary is never emitted — which is why one explosion straddling a border dirties
//!   more chunks than it visually touches. The dirty pad is `light reach + 1`, derived
//!   from the light maximum and attenuation rather than asserted, because the mesher reads
//!   the neighbour cell one voxel outside the chunk and a light change at the edge of its
//!   reach still alters a face one voxel further out. The padded copy does **not**
//!   eliminate T-junctions; greedy meshing produces those by construction, and G1 saw none
//!   only because every vertex coordinate is a small exact integer in `f32` with MSAA off.
//!
//! # What lives here
//!
//! Today: the boundary types only — [`ChunkInput`] in, [`MeshBuffers`] out, and
//! [`mesh_chunk`] between them. The algorithm itself lands at the walking skeleton's vista
//! task; see the `PLACEHOLDER` on [`mesh_chunk`].

use std::fmt;

/// Voxels along one edge of a chunk. The sim's chunks are 32-cubed (spec section 15), and
/// spike G1 measured the mesher at that size; it is not a tuning knob.
pub const CHUNK_EDGE: usize = 32;

/// Voxels in a chunk: `CHUNK_EDGE` cubed, 32 768.
pub const CHUNK_VOLUME: usize = CHUNK_EDGE * CHUNK_EDGE * CHUNK_EDGE;

/// Voxels in one face of a chunk: `CHUNK_EDGE` squared, 1 024. The length of one
/// neighbour's boundary slice.
pub const FACE_AREA: usize = CHUNK_EDGE * CHUNK_EDGE;

/// Material id 0 is air: it emits no faces and hides none.
pub const AIR: u8 = 0;

/// The linear index of a chunk-local voxel, in the crate's fixed order.
///
/// **x is fastest, then z, then y** — `index = x + 32 * z + 1024 * y` — so a whole
/// 32-voxel row along x is one contiguous copy, which is what the padded-neighbour copy
/// depends on (spike G1's chunk store used exactly this order and charged 12 microseconds
/// of a 330 microsecond mesh to that copy). y is the vertical axis, as in Godot.
///
/// The result is meaningful only for coordinates below [`CHUNK_EDGE`]; the function does
/// not range-check, because it sits in the inner loop of the mask sweep.
///
/// PLACEHOLDER: the sim's chunk store must adopt this same order, so that the boundary
/// hands over a slice rather than a transposition — owner decides when the contract layer
/// writes the chunk store at the walking skeleton.
#[must_use]
pub const fn voxel_index(x: usize, y: usize, z: usize) -> usize {
    (y * CHUNK_EDGE + z) * CHUNK_EDGE + x
}

/// One of the six axis-aligned face directions a chunk is swept along.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Face {
    /// Towards decreasing x.
    NegX,
    /// Towards increasing x.
    PosX,
    /// Towards decreasing y — downwards.
    NegY,
    /// Towards increasing y — upwards.
    PosY,
    /// Towards decreasing z.
    NegZ,
    /// Towards increasing z.
    PosZ,
}

impl Face {
    /// The six faces in the order the borders of a [`ChunkInput`] are given in.
    pub const ALL: [Self; 6] = [
        Self::NegX,
        Self::PosX,
        Self::NegY,
        Self::PosY,
        Self::NegZ,
        Self::PosZ,
    ];

    /// This face's position in [`Face::ALL`], and so its slot in the border array.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::NegX => 0,
            Self::PosX => 1,
            Self::NegY => 2,
            Self::PosY => 3,
            Self::NegZ => 4,
            Self::PosZ => 5,
        }
    }

    /// A short name for diagnostics.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::NegX => "-x",
            Self::PosX => "+x",
            Self::NegY => "-y",
            Self::PosY => "+y",
            Self::NegZ => "-z",
            Self::PosZ => "+z",
        }
    }
}

impl fmt::Display for Face {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Why a [`ChunkInput`] or a [`NeighbourBorder`] was refused.
///
/// Length is the only thing checked: material ids and light values are bytes, and every
/// byte is a legal one at this boundary.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChunkInputError {
    /// The material array was not [`CHUNK_VOLUME`] long.
    Materials {
        /// The length that was offered.
        len: usize,
    },
    /// The light array was not [`CHUNK_VOLUME`] long.
    Light {
        /// The length that was offered.
        len: usize,
    },
    /// A neighbour's border material slice was not [`FACE_AREA`] long.
    BorderMaterials {
        /// The length that was offered.
        len: usize,
    },
    /// A neighbour's border light slice was not [`FACE_AREA`] long.
    BorderLight {
        /// The length that was offered.
        len: usize,
    },
}

impl fmt::Display for ChunkInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Materials { len } => {
                write!(f, "materials must be {CHUNK_VOLUME} bytes, got {len}")
            }
            Self::Light { len } => write!(f, "light must be {CHUNK_VOLUME} bytes, got {len}"),
            Self::BorderMaterials { len } => {
                write!(f, "border materials must be {FACE_AREA} bytes, got {len}")
            }
            Self::BorderLight { len } => {
                write!(f, "border light must be {FACE_AREA} bytes, got {len}")
            }
        }
    }
}

impl std::error::Error for ChunkInputError {}

/// One neighbour's boundary slice: the 32-by-32 layer of voxels immediately outside a
/// chunk face, which is what keeps the mesher from emitting a face that is interior across
/// the boundary.
///
/// Both slices are [`FACE_AREA`] long and are indexed by the two axes the face direction
/// leaves free, taken in the volume order's own precedence — **x fastest, then z, then
/// y**. So a plus-or-minus-x border is indexed `z + 32 * y`, a plus-or-minus-y border
/// `x + 32 * z`, and a plus-or-minus-z border `x + 32 * y`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NeighbourBorder {
    materials: Box<[u8]>,
    light: Box<[u8]>,
}

impl NeighbourBorder {
    /// Takes one neighbour's boundary slice, checking both lengths.
    ///
    /// # Errors
    ///
    /// [`ChunkInputError::BorderMaterials`] or [`ChunkInputError::BorderLight`] when the
    /// slice offered is not [`FACE_AREA`] bytes long.
    pub fn new(materials: Vec<u8>, light: Vec<u8>) -> Result<Self, ChunkInputError> {
        if materials.len() != FACE_AREA {
            return Err(ChunkInputError::BorderMaterials {
                len: materials.len(),
            });
        }
        if light.len() != FACE_AREA {
            return Err(ChunkInputError::BorderLight { len: light.len() });
        }
        Ok(Self {
            materials: materials.into_boxed_slice(),
            light: light.into_boxed_slice(),
        })
    }

    /// The neighbour's material ids across the shared face.
    #[must_use]
    pub fn materials(&self) -> &[u8] {
        &self.materials
    }

    /// The neighbour's baked light values across the shared face.
    #[must_use]
    pub fn light(&self) -> &[u8] {
        &self.light
    }
}

/// One chunk, as the mesher takes it: integer material ids, integer light, and whatever is
/// known of the six neighbours.
///
/// Both arrays are [`CHUNK_VOLUME`] bytes in the order [`voxel_index`] defines. A border
/// that is [`None`] means "no neighbour data" — the chunk sits at the edge of the world,
/// or the caller has not loaded it — and the mesher treats that face as open sky rather
/// than guessing.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ChunkInput {
    materials: Box<[u8]>,
    light: Box<[u8]>,
    borders: [Option<NeighbourBorder>; 6],
}

impl ChunkInput {
    /// Six empty border slots, for a caller that has no neighbour data to hand.
    #[must_use]
    pub const fn no_borders() -> [Option<NeighbourBorder>; 6] {
        [None, None, None, None, None, None]
    }

    /// Takes one chunk's integer data, checking every length before anything is kept.
    ///
    /// `borders` is indexed by [`Face::index`].
    ///
    /// # Errors
    ///
    /// [`ChunkInputError::Materials`] or [`ChunkInputError::Light`] when an array is not
    /// [`CHUNK_VOLUME`] bytes long. A border can only fail its own lengths, and
    /// [`NeighbourBorder::new`] has already checked those.
    pub fn new(
        materials: Vec<u8>,
        light: Vec<u8>,
        borders: [Option<NeighbourBorder>; 6],
    ) -> Result<Self, ChunkInputError> {
        if materials.len() != CHUNK_VOLUME {
            return Err(ChunkInputError::Materials {
                len: materials.len(),
            });
        }
        if light.len() != CHUNK_VOLUME {
            return Err(ChunkInputError::Light { len: light.len() });
        }
        Ok(Self {
            materials: materials.into_boxed_slice(),
            light: light.into_boxed_slice(),
            borders,
        })
    }

    /// An all-air, unlit chunk with no neighbour data. Meshes to nothing.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            materials: vec![AIR; CHUNK_VOLUME].into_boxed_slice(),
            light: vec![0_u8; CHUNK_VOLUME].into_boxed_slice(),
            borders: Self::no_borders(),
        }
    }

    /// The chunk's material ids, in [`voxel_index`] order.
    #[must_use]
    pub fn materials(&self) -> &[u8] {
        &self.materials
    }

    /// The chunk's baked flood-fill light, in [`voxel_index`] order.
    #[must_use]
    pub fn light(&self) -> &[u8] {
        &self.light
    }

    /// The neighbour data for one face, when the caller had any.
    #[must_use]
    pub fn border(&self, face: Face) -> Option<&NeighbourBorder> {
        self.borders.get(face.index()).and_then(Option::as_ref)
    }
}

/// One drawable surface inside a [`MeshBuffers`]: a half-open run of vertices and a
/// half-open run of indices.
///
/// A 32-cubed chunk is one surface with a palette material (spike G1), so this is a Vec of
/// one for terrain today; `.vox` models are the reason the shape is a list rather than a
/// pair of counts.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Surface {
    /// Index of this surface's first vertex in the position, normal and colour arrays.
    pub first_vertex: u32,
    /// How many vertices this surface owns.
    pub vertex_count: u32,
    /// Offset of this surface's first entry in the index array.
    pub first_index: u32,
    /// How many indices this surface owns — always a multiple of three.
    pub index_count: u32,
}

/// What the mesher produces: parallel vertex arrays, a 16-bit index array, and the
/// surfaces that carve them up.
///
/// This is the float side of the wall and the only float-bearing type this crate exposes.
/// The arrays are laid out to be handed to `client-gdext` and on to Godot's
/// `RenderingServer` with no repacking: positions and colours are what spike G1 measured
/// (12 bytes of position and 4 of colour per vertex on the wire), and the indices are
/// `u16` because G1 measured 6 660 vertices at worst against the 65 536 cap.
///
/// PLACEHOLDER: `normals` is carried here because the boundary was specified with it, but
/// G1's measured path-B surface format was position plus colour plus index only — the
/// per-face shading factor is folded into the vertex colour and the material is unshaded,
/// which is both the look the game wants and what keeps path B's buffers unambiguous.
/// Whether normals are uploaded at all is the vista task's decision; owner signs it off
/// when the mesher is written.
#[derive(Clone, Debug, Default)]
pub struct MeshBuffers {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colours: Vec<[u8; 4]>,
    indices: Vec<u16>,
    surfaces: Vec<Surface>,
}

impl MeshBuffers {
    /// Buffers with nothing in them: what a fully buried or fully empty chunk produces.
    ///
    /// A chunk that meshes to nothing has no surface at all — it is not a surface with
    /// zero vertices, and G1 records that averaging those into a byte percentile is how an
    /// upload budget gets set a third too low.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            positions: Vec::new(),
            normals: Vec::new(),
            colours: Vec::new(),
            indices: Vec::new(),
            surfaces: Vec::new(),
        }
    }

    /// Vertex positions in chunk-local voxels.
    #[must_use]
    pub fn positions(&self) -> &[[f32; 3]] {
        &self.positions
    }

    /// Per-vertex normals, parallel to [`MeshBuffers::positions`].
    #[must_use]
    pub fn normals(&self) -> &[[f32; 3]] {
        &self.normals
    }

    /// Per-vertex RGBA colours carrying the material and the baked flood-fill light.
    #[must_use]
    pub fn colours(&self) -> &[[u8; 4]] {
        &self.colours
    }

    /// Triangle indices. Compare this array, never the counts, before patching a surface
    /// in place.
    #[must_use]
    pub fn indices(&self) -> &[u16] {
        &self.indices
    }

    /// The surfaces these arrays carry.
    #[must_use]
    pub fn surfaces(&self) -> &[Surface] {
        &self.surfaces
    }

    /// How many surfaces there are. Zero means "upload nothing".
    #[must_use]
    pub fn surface_count(&self) -> usize {
        self.surfaces.len()
    }

    /// True when there is nothing to upload.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.surfaces.is_empty()
    }
}

/// Meshes one chunk.
///
/// PLACEHOLDER: the greedy mesher is written against these types at the skeleton's vista
/// task (owner schedules it); `spikes/g1-remesh/src/greedy.rs` at tag `spike-end` is the
/// reference, never copied. Until then this returns empty buffers, so a caller wired up
/// early uploads nothing rather than uploading wrong geometry.
#[must_use]
pub fn mesh_chunk(input: &ChunkInput) -> MeshBuffers {
    let _ = input;
    MeshBuffers::empty()
}

#[cfg(test)]
mod tests {
    use super::{
        AIR, CHUNK_VOLUME, ChunkInput, ChunkInputError, FACE_AREA, Face, MeshBuffers,
        NeighbourBorder, mesh_chunk, voxel_index,
    };

    #[test]
    fn materials_of_the_wrong_length_are_refused() {
        let err = ChunkInput::new(
            vec![AIR; CHUNK_VOLUME - 1],
            vec![0; CHUNK_VOLUME],
            ChunkInput::no_borders(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            ChunkInputError::Materials {
                len: CHUNK_VOLUME - 1
            }
        );
    }

    #[test]
    fn light_of_the_wrong_length_is_refused() {
        let err = ChunkInput::new(
            vec![AIR; CHUNK_VOLUME],
            vec![0; CHUNK_VOLUME + 1],
            ChunkInput::no_borders(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            ChunkInputError::Light {
                len: CHUNK_VOLUME + 1
            }
        );
    }

    #[test]
    fn a_border_of_the_wrong_length_is_refused() {
        assert_eq!(
            NeighbourBorder::new(vec![AIR; FACE_AREA - 1], vec![0; FACE_AREA]).unwrap_err(),
            ChunkInputError::BorderMaterials { len: FACE_AREA - 1 }
        );
        assert_eq!(
            NeighbourBorder::new(vec![AIR; FACE_AREA], vec![0; 0]).unwrap_err(),
            ChunkInputError::BorderLight { len: 0 }
        );
    }

    #[test]
    fn the_right_lengths_are_accepted_and_kept() {
        let mut borders = ChunkInput::no_borders();
        let slot = borders
            .get_mut(Face::PosY.index())
            .expect("Face::index is inside the six-slot array");
        *slot = Some(
            NeighbourBorder::new(vec![AIR; FACE_AREA], vec![15; FACE_AREA])
                .expect("both slices are FACE_AREA long"),
        );

        let chunk = ChunkInput::new(vec![AIR; CHUNK_VOLUME], vec![0; CHUNK_VOLUME], borders)
            .expect("both arrays are CHUNK_VOLUME long");

        assert_eq!(chunk.materials().len(), CHUNK_VOLUME);
        assert_eq!(chunk.light().len(), CHUNK_VOLUME);
        assert!(chunk.border(Face::PosY).is_some());
        assert!(chunk.border(Face::NegY).is_none());
    }

    #[test]
    fn an_empty_chunk_meshes_to_zero_surfaces() {
        let buffers = mesh_chunk(&ChunkInput::empty());

        assert_eq!(buffers.surface_count(), 0);
        assert!(buffers.is_empty());
        assert!(buffers.positions().is_empty());
        assert!(buffers.normals().is_empty());
        assert!(buffers.colours().is_empty());
        assert!(buffers.indices().is_empty());
        assert!(MeshBuffers::empty().is_empty());
    }

    #[test]
    fn the_voxel_order_is_x_fastest_then_z_then_y() {
        assert_eq!(voxel_index(0, 0, 0), 0);
        assert_eq!(voxel_index(1, 0, 0), 1);
        assert_eq!(voxel_index(0, 0, 1), 32);
        assert_eq!(voxel_index(0, 1, 0), 1024);
        assert_eq!(voxel_index(31, 31, 31), CHUNK_VOLUME - 1);
    }

    #[test]
    fn every_face_has_its_own_slot() {
        for (slot, face) in Face::ALL.iter().enumerate() {
            assert_eq!(face.index(), slot);
        }
    }
}
