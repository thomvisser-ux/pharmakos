// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Path A and path B, the switch between them, and the guard that is the price of B.
//!
//! Decisions log section 2.7 item 53: **path B** — a `RenderingServer` mesh RID per chunk,
//! patched in place where it can be — with **path A** — an `ArrayMesh` on a
//! `MeshInstance3D` — kept as a build-time switch, "so that a retreat to A is a flag, not
//! a rewrite". B is 1.48x cheaper per upload and creates no resources against A's 5 798
//! `ArrayMesh` objects a minute; it also carries four bug classes, and this module is
//! where three of them are answered. (The fourth, the dangling material RID, is answered
//! by ownership on the engine side — see `bridge`.)
//!
//! # Decide once, execute twice
//!
//! The decision of *what to do with a chunk* is here, in [`Uploader::plan`], and it
//! produces an [`UploadOp`]. Two things execute that op:
//!
//! * the engine, through `RenderingServer` or `ArrayMesh` calls (`bridge`);
//! * [`ResidentSurface`], a plain-data model of what the engine would then be holding.
//!
//! One decision path, two executors, and no copy of the logic. That is what makes T12's
//! acceptance — "paths A and B produce identical geometry digests on the same chunk set"
//! — a test that runs on every CI leg with no GPU, rather than a claim about a screenshot.
//!
//! # The guard that matters
//!
//! **An equal vertex count does not imply an equal index array.** Each quad emits
//! `v0, v0+1, v0+2, v0, v0+2, v0+3` or the reverse depending on its face sign, so two
//! remeshes with the same totals but a different split of quads across the six directions
//! need different indices. Path B's in-place write touches the vertex and attribute
//! regions only — the index buffer is left as it was — so patching on a count match alone
//! leaves the chunk drawing the new vertices with the old winding: back-face-culled holes
//! that no timing number would show. G1 measured **29 of 2 570** in-place updates in its
//! headline run with a changed index array. [`Uploader::plan`] therefore compares the
//! index array itself, and `an_index_array_change_under_an_equal_vertex_count_forces_a_rebuild`
//! is the test that keeps it doing so.

use crate::error::BridgeError;
use crate::surface::{ATTRIBUTE_STRIDE, ColourQuantisation, Surface, SurfaceProbe, VERTEX_STRIDE};

/// Which upload path this build uses.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UploadPath {
    /// Path A: a fresh `ArrayMesh` on a `MeshInstance3D` per remesh. Idiomatic,
    /// inspectable in the remote scene tree, and it churns resources.
    ArrayMesh,
    /// Path B: one `RenderingServer` mesh RID per chunk, patched in place where the
    /// index array and the probed layout allow it. The measured default.
    RenderingServerRids,
}

impl UploadPath {
    /// A short name for a diagnostic line.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ArrayMesh => "A (ArrayMesh)",
            Self::RenderingServerRids => "B (RenderingServer RIDs)",
        }
    }
}

/// The path this build defaults to.
///
/// **This is item 53's build-time switch.** Both paths are compiled, linted and tested in
/// every configuration — a feature that compiled only one of them would leave the retreat
/// untested, which is the opposite of what keeping A alive is for. What the
/// `upload-path-a` feature changes is this constant, so a build that wants A is
/// `--features pharmakos-client-gdext/upload-path-a` and nothing else: a flag, not a
/// rewrite.
pub const DEFAULT_UPLOAD_PATH: UploadPath = if cfg!(feature = "upload-path-a") {
    UploadPath::ArrayMesh
} else {
    UploadPath::RenderingServerRids
};

/// What the engine is being asked to do with one chunk this frame.
///
/// Carries no engine types, so the same value drives a `RenderingServer` call and the
/// plain-data model the tests compare.
#[derive(Clone, PartialEq, Debug)]
pub enum UploadOp {
    /// Nothing is resident yet: create a surface from the arrays.
    Create {
        /// The surface to build.
        surface: Surface,
    },
    /// A surface is resident but cannot be patched: clear it and build it again.
    Rebuild {
        /// The surface to build.
        surface: Surface,
        /// Why the fast path was refused, for the counters and the log.
        because: RebuildReason,
    },
    /// The surface is resident, the index array is unchanged and the layout is the one
    /// the region write assumes: overwrite the two buffers in place.
    Patch {
        /// The vertex-buffer bytes, three little-endian `f32` per vertex.
        vertex_bytes: Vec<u8>,
        /// The attribute-buffer bytes, quantised as the probe found the engine does.
        attribute_bytes: Vec<u8>,
    },
    /// The chunk meshed to nothing. Hide whatever is resident; do not free it, because
    /// the next edit will very likely fill it again.
    Hide,
}

/// Why an upload took the rebuild branch rather than the in-place one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RebuildReason {
    /// This build uses path A, which has no in-place branch at all.
    PathA,
    /// The vertex count changed, so the buffers are a different size.
    VertexCountChanged,
    /// The vertex count matched and the **index array did not**. The 29-of-2 570 case.
    IndexArrayChanged,
    /// The probed layout or colour conversion is not one the region write can produce.
    LayoutNotPatchable,
}

impl RebuildReason {
    /// A short name for a diagnostic line.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::PathA => "path A has no in-place branch",
            Self::VertexCountChanged => "the vertex count changed",
            Self::IndexArrayChanged => "the index array changed",
            Self::LayoutNotPatchable => "the probed layout is not patchable",
        }
    }
}

/// What the bridge remembers about a chunk between frames.
#[derive(Clone, PartialEq, Debug)]
struct Resident {
    vertex_count: usize,
    /// The index array as last uploaded. Kept whole, because comparing the counts is
    /// exactly the mistake the module header describes.
    indices: Vec<u16>,
}

/// Counters the self-check and the CI leg read.
///
/// Every one of them is a count of something that happened, not a rate: a rate computed
/// here would be arithmetic the bridge has no business doing, and a caller that wants one
/// can divide two of these.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct UploadCounters {
    /// Surfaces created for a chunk that had none.
    pub creates: u64,
    /// Surfaces patched in place.
    pub patches: u64,
    /// Surfaces cleared and built again.
    pub rebuilds: u64,
    /// Rebuilds forced because the vertex count changed.
    pub rebuilds_vertex_count: u64,
    /// Rebuilds forced because the **index array** changed although the vertex count did
    /// not — the cases a count-only guard would have got wrong.
    pub rebuilds_index_changed: u64,
    /// Rebuilds forced because the probed layout could not be patched.
    pub rebuilds_layout: u64,
    /// Chunks that meshed to nothing and were hidden.
    pub hides: u64,
    /// Bytes the chosen branch actually wrote across the seam. A create or a rebuild
    /// writes the whole surface; a patch writes the two regions and no indices, which is
    /// why charging the full figure for both would make the byte budget mean two
    /// different things on the two paths.
    pub bytes: u64,
}

/// The per-chunk upload state machine, without any engine in it.
///
/// One per client. `plan` is called for each chunk the drain queue releases; the caller
/// executes the returned [`UploadOp`] against the engine, and the uploader's counters and
/// per-chunk memory are updated as part of planning, because the engine call cannot fail
/// in a way the plan would want to take back.
#[derive(Clone, Debug)]
pub struct Uploader {
    path: UploadPath,
    probe: SurfaceProbe,
    resident: Vec<Option<Resident>>,
    counters: UploadCounters,
}

impl Uploader {
    /// An uploader for `chunks` chunks on `path`, with nothing resident and nothing
    /// probed.
    #[must_use]
    pub fn new(path: UploadPath, chunks: usize) -> Self {
        Self {
            path,
            probe: SurfaceProbe::default(),
            resident: vec![None; chunks],
            counters: UploadCounters::default(),
        }
    }

    /// Which path this uploader is on.
    #[must_use]
    pub const fn path(&self) -> UploadPath {
        self.path
    }

    /// The counters so far.
    #[must_use]
    pub const fn counters(&self) -> UploadCounters {
        self.counters
    }

    /// What the engine last said about the surface layout.
    #[must_use]
    pub const fn probe(&self) -> SurfaceProbe {
        self.probe
    }

    /// Records what the engine reported for the first surface it built.
    ///
    /// Only the first: the layout is a property of the format, not of the chunk, and
    /// re-probing every frame would cost a read-back per upload for an answer that cannot
    /// change. Path A never calls this, because it has no in-place branch to gate.
    pub fn record_probe(&mut self, probe: SurfaceProbe) {
        if self.probe == SurfaceProbe::default() {
            self.probe = probe;
        }
    }

    /// Decides what to do with `chunk` given its freshly meshed `surface`.
    ///
    /// # Errors
    ///
    /// [`BridgeError::SurfaceSize`] when `chunk` is outside the chunk count this uploader
    /// was built for.
    pub fn plan(&mut self, chunk: usize, surface: &Surface) -> Result<UploadOp, BridgeError> {
        let chunks = self.resident.len();
        if chunk >= chunks {
            return Err(BridgeError::SurfaceSize {
                what: "the chunk index",
                got: chunk,
                cap: chunks,
            });
        }

        if surface.is_empty() {
            self.counters.hides = self.counters.hides.saturating_add(1);
            return Ok(UploadOp::Hide);
        }

        let full_bytes = surface_bytes(surface);

        // The whole decision, taken while only a shared borrow is out: `None` means
        // nothing is resident, `Some(None)` means the resident surface can be patched,
        // and `Some(Some(reason))` says why it cannot. Copying the resident index array
        // to end the borrow would be an allocation of up to nine kilobytes per chunk,
        // every frame, for a comparison that never needed to own anything.
        let decision: Option<Option<RebuildReason>> = self
            .resident
            .get(chunk)
            .and_then(Option::as_ref)
            .map(|previous| self.rebuild_reason(previous, surface));

        let Some(reason) = decision else {
            self.counters.creates = self.counters.creates.saturating_add(1);
            self.counters.bytes = self.counters.bytes.saturating_add(full_bytes);
            self.remember(chunk, surface);
            return Ok(UploadOp::Create {
                surface: surface.clone(),
            });
        };

        if let Some(because) = reason {
            self.count_rebuild(because);
            self.counters.bytes = self.counters.bytes.saturating_add(full_bytes);
            self.remember(chunk, surface);
            return Ok(UploadOp::Rebuild {
                surface: surface.clone(),
                because,
            });
        }

        // The only branch that reaches the engine's buffers directly, and the only one
        // the three guards above exist to protect.
        let Some(quantisation) = self.probe.patchable() else {
            // Unreachable: `rebuild_reason` returns `LayoutNotPatchable` for exactly this
            // case. Handled rather than unwrapped, because `unwrap_used` is denied and a
            // panic here would be caught silently at the `#[func]` boundary.
            self.count_rebuild(RebuildReason::LayoutNotPatchable);
            self.counters.bytes = self.counters.bytes.saturating_add(full_bytes);
            self.remember(chunk, surface);
            return Ok(UploadOp::Rebuild {
                surface: surface.clone(),
                because: RebuildReason::LayoutNotPatchable,
            });
        };

        // Two allocations per patched chunk, at K = 4 surfaces a frame. The op owns its
        // bytes because the engine call consumes them, so a scratch buffer here would be
        // copied out again and would buy nothing.
        let mut vertex_bytes = Vec::new();
        let mut attribute_bytes = Vec::new();
        surface.vertex_bytes(&mut vertex_bytes);
        surface.attribute_bytes(quantisation, &mut attribute_bytes);
        self.counters.patches = self.counters.patches.saturating_add(1);
        self.counters.bytes = self
            .counters
            .bytes
            .saturating_add(patch_bytes(surface.vertex_count()));
        self.remember(chunk, surface);
        Ok(UploadOp::Patch {
            vertex_bytes,
            attribute_bytes,
        })
    }

    /// Why this surface cannot be patched over the resident one, or `None` when it can.
    fn rebuild_reason(&self, previous: &Resident, surface: &Surface) -> Option<RebuildReason> {
        if self.path == UploadPath::ArrayMesh {
            return Some(RebuildReason::PathA);
        }
        if previous.vertex_count != surface.vertex_count() {
            return Some(RebuildReason::VertexCountChanged);
        }
        // The guard. Counts are equal here; the arrays still may not be.
        if previous.indices != surface.indices() {
            return Some(RebuildReason::IndexArrayChanged);
        }
        if self.probe.patchable().is_none() {
            return Some(RebuildReason::LayoutNotPatchable);
        }
        None
    }

    fn count_rebuild(&mut self, because: RebuildReason) {
        self.counters.rebuilds = self.counters.rebuilds.saturating_add(1);
        match because {
            RebuildReason::VertexCountChanged => {
                self.counters.rebuilds_vertex_count =
                    self.counters.rebuilds_vertex_count.saturating_add(1);
            }
            RebuildReason::IndexArrayChanged => {
                self.counters.rebuilds_index_changed =
                    self.counters.rebuilds_index_changed.saturating_add(1);
            }
            RebuildReason::LayoutNotPatchable => {
                self.counters.rebuilds_layout = self.counters.rebuilds_layout.saturating_add(1);
            }
            RebuildReason::PathA => {}
        }
    }

    fn remember(&mut self, chunk: usize, surface: &Surface) {
        if let Some(slot) = self.resident.get_mut(chunk) {
            *slot = Some(Resident {
                vertex_count: surface.vertex_count(),
                indices: surface.indices().to_vec(),
            });
        }
    }
}

/// The GPU-side cost of a whole surface, in the mesher's own accounting.
fn surface_bytes(surface: &Surface) -> u64 {
    // The mesher owns this figure so that the byte budget and the mesher agree; the
    // bridge does not compute a second one. `MeshBuffers` is the mesher's type, so the
    // arithmetic is re-derived here from the same per-vertex and per-index widths that
    // `gpu_surface_bytes` documents, and the test below pins the two together.
    let vertices = u64::try_from(surface.vertex_count()).unwrap_or(u64::MAX);
    let indices = u64::try_from(surface.indices().len()).unwrap_or(u64::MAX);
    vertices
        .saturating_mul(16)
        .saturating_add(indices.saturating_mul(2))
}

/// The bytes an in-place patch writes: the two regions, and no indices.
fn patch_bytes(vertices: usize) -> u64 {
    let stride = u64::try_from(VERTEX_STRIDE.saturating_add(ATTRIBUTE_STRIDE)).unwrap_or(u64::MAX);
    u64::try_from(vertices)
        .unwrap_or(u64::MAX)
        .saturating_mul(stride)
}

/// What the engine holds for one chunk once an [`UploadOp`] has been executed.
///
/// A plain-data stand-in for a `RenderingServer` surface or an `ArrayMesh`, so that the
/// two paths can be compared byte for byte with no engine in the loop. The colours go
/// through the same quantisation the engine applies, which is the point: a create sends
/// floats and lets the engine quantise, a patch sends bytes, and the model applies
/// whichever of the two the branch used.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ResidentSurface {
    /// Vertex positions as the engine holds them.
    pub positions: Vec<[f32; 3]>,
    /// Vertex colours as the engine holds them, after quantisation.
    pub colours: Vec<[u8; 4]>,
    /// The index buffer as the engine holds it. A patch does **not** touch it.
    pub indices: Vec<u16>,
    /// Whether the chunk's instance is visible.
    pub visible: bool,
}

impl ResidentSurface {
    /// Applies one op, as the engine would.
    ///
    /// `quantisation` is what the engine does to a `Color` on the create and rebuild
    /// branches; the patch branch carries its bytes already quantised.
    ///
    /// # Errors
    ///
    /// [`BridgeError::Length`] when a patch's buffers are not the size of the resident
    /// surface — which, if it ever happened against the real engine, would be a write
    /// past the end of an allocated region rather than a test failure.
    pub fn apply(
        &mut self,
        op: &UploadOp,
        quantisation: ColourQuantisation,
    ) -> Result<(), BridgeError> {
        match op {
            UploadOp::Create { surface } | UploadOp::Rebuild { surface, .. } => {
                self.positions = surface.positions().to_vec();
                self.colours = surface
                    .colour_floats()
                    .iter()
                    .map(|colour| {
                        [
                            quantisation.apply(colour[0]),
                            quantisation.apply(colour[1]),
                            quantisation.apply(colour[2]),
                            quantisation.apply(colour[3]),
                        ]
                    })
                    .collect();
                self.indices = surface.indices().to_vec();
                self.visible = true;
                Ok(())
            }
            UploadOp::Patch {
                vertex_bytes,
                attribute_bytes,
            } => {
                let expected_vertex = self.positions.len().saturating_mul(VERTEX_STRIDE);
                if vertex_bytes.len() != expected_vertex {
                    return Err(BridgeError::Length {
                        what: "an in-place vertex region",
                        expected: expected_vertex,
                        got: vertex_bytes.len(),
                    });
                }
                let expected_attribute = self.colours.len().saturating_mul(ATTRIBUTE_STRIDE);
                if attribute_bytes.len() != expected_attribute {
                    return Err(BridgeError::Length {
                        what: "an in-place attribute region",
                        expected: expected_attribute,
                        got: attribute_bytes.len(),
                    });
                }
                self.positions = read_positions(vertex_bytes);
                self.colours = read_colours(attribute_bytes);
                // The index buffer is untouched, exactly as the region write leaves it.
                self.visible = true;
                Ok(())
            }
            UploadOp::Hide => {
                self.visible = false;
                Ok(())
            }
        }
    }
}

fn read_positions(bytes: &[u8]) -> Vec<[f32; 3]> {
    bytes
        .chunks_exact(VERTEX_STRIDE)
        .map(|vertex| {
            [
                read_f32(vertex, 0),
                read_f32(vertex, 4),
                read_f32(vertex, 8),
            ]
        })
        .collect()
}

fn read_f32(bytes: &[u8], offset: usize) -> f32 {
    let end = offset.saturating_add(4);
    let mut word = [0_u8; 4];
    if let Some(slice) = bytes.get(offset..end) {
        word.copy_from_slice(slice);
    }
    f32::from_le_bytes(word)
}

fn read_colours(bytes: &[u8]) -> Vec<[u8; 4]> {
    bytes
        .chunks_exact(ATTRIBUTE_STRIDE)
        .map(|colour| {
            let mut out = [0_u8; 4];
            out.copy_from_slice(colour);
            out
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pharmakos_mesher::{ChunkInput, LightParams, MeshBuffers, gpu_surface_bytes, mesh_chunk};

    fn probed() -> SurfaceProbe {
        SurfaceProbe {
            vertex_stride: Some(VERTEX_STRIDE),
            attribute_stride: Some(ATTRIBUTE_STRIDE),
            quantisation: Some(ColourQuantisation::RoundHalfUp),
        }
    }

    fn surface(vertices: usize, indices: Vec<u16>, tint: u8) -> Surface {
        let positions: Vec<[f32; 3]> = (0..vertices)
            .map(|index| {
                let value = f32::from(u8::try_from(index % 32).unwrap_or(0));
                [value, value, value]
            })
            .collect();
        let colours = vec![[tint, 20, 30, 255]; vertices];
        Surface::new(positions, colours, indices).expect("matching lengths")
    }

    #[test]
    fn the_default_path_is_bs_unless_the_retreat_flag_is_set() {
        // Item 53's switch. In the configuration CI builds, B is the default.
        assert_eq!(
            DEFAULT_UPLOAD_PATH,
            if cfg!(feature = "upload-path-a") {
                UploadPath::ArrayMesh
            } else {
                UploadPath::RenderingServerRids
            }
        );
    }

    #[test]
    fn the_first_upload_of_a_chunk_creates_and_the_second_identical_one_patches() {
        let mut uploader = Uploader::new(UploadPath::RenderingServerRids, 4);
        uploader.record_probe(probed());
        let first = surface(4, vec![0, 1, 2, 0, 2, 3], 10);

        assert!(matches!(
            uploader.plan(1, &first).expect("in range"),
            UploadOp::Create { .. }
        ));
        let second = surface(4, vec![0, 1, 2, 0, 2, 3], 11);
        assert!(matches!(
            uploader.plan(1, &second).expect("in range"),
            UploadOp::Patch { .. }
        ));
        assert_eq!(uploader.counters().creates, 1);
        assert_eq!(uploader.counters().patches, 1);
        assert_eq!(uploader.counters().rebuilds, 0);
    }

    /// G1's 29-of-2 570. The counts match, the arrays do not, and a count-only guard
    /// would have left the chunk drawing with stale winding.
    #[test]
    fn an_index_array_change_under_an_equal_vertex_count_forces_a_rebuild() {
        let mut uploader = Uploader::new(UploadPath::RenderingServerRids, 4);
        uploader.record_probe(probed());
        let first = surface(4, vec![0, 1, 2, 0, 2, 3], 10);
        let flipped = surface(4, vec![0, 2, 1, 0, 3, 2], 10);
        assert_eq!(
            first.vertex_count(),
            flipped.vertex_count(),
            "the whole point is that the counts agree"
        );

        uploader.plan(0, &first).expect("in range");
        let op = uploader.plan(0, &flipped).expect("in range");
        assert!(
            matches!(
                op,
                UploadOp::Rebuild {
                    because: RebuildReason::IndexArrayChanged,
                    ..
                }
            ),
            "{op:?}"
        );
        assert_eq!(uploader.counters().rebuilds_index_changed, 1);
    }

    #[test]
    fn an_unprobed_uploader_never_patches() {
        let mut uploader = Uploader::new(UploadPath::RenderingServerRids, 2);
        let first = surface(4, vec![0, 1, 2, 0, 2, 3], 10);
        uploader.plan(0, &first).expect("in range");
        let op = uploader.plan(0, &first).expect("in range");
        assert!(
            matches!(
                op,
                UploadOp::Rebuild {
                    because: RebuildReason::LayoutNotPatchable,
                    ..
                }
            ),
            "{op:?}"
        );
        assert_eq!(uploader.counters().rebuilds_layout, 1);
    }

    #[test]
    fn path_a_never_patches_and_says_so() {
        let mut uploader = Uploader::new(UploadPath::ArrayMesh, 2);
        uploader.record_probe(probed());
        let first = surface(4, vec![0, 1, 2, 0, 2, 3], 10);
        uploader.plan(0, &first).expect("in range");
        let op = uploader.plan(0, &first).expect("in range");
        assert!(
            matches!(
                op,
                UploadOp::Rebuild {
                    because: RebuildReason::PathA,
                    ..
                }
            ),
            "{op:?}"
        );
        assert_eq!(uploader.counters().patches, 0);
    }

    #[test]
    fn an_empty_chunk_hides_rather_than_uploading_nothing() {
        let mut uploader = Uploader::new(UploadPath::RenderingServerRids, 2);
        let empty = Surface::default();
        assert_eq!(uploader.plan(0, &empty).expect("in range"), UploadOp::Hide);
        assert_eq!(uploader.counters().hides, 1);
        assert_eq!(uploader.counters().bytes, 0);
    }

    #[test]
    fn a_chunk_index_past_the_end_is_an_error_rather_than_a_panic() {
        let mut uploader = Uploader::new(UploadPath::RenderingServerRids, 2);
        let error = uploader
            .plan(9, &surface(4, vec![0, 1, 2], 1))
            .expect_err("nine is not a chunk of two");
        assert!(matches!(error, BridgeError::SurfaceSize { .. }), "{error}");
    }

    #[test]
    fn a_patch_leaves_the_index_buffer_exactly_as_it_was() {
        let mut uploader = Uploader::new(UploadPath::RenderingServerRids, 1);
        uploader.record_probe(probed());
        let first = surface(4, vec![0, 1, 2, 0, 2, 3], 10);
        let mut resident = ResidentSurface::default();
        resident
            .apply(
                &uploader.plan(0, &first).expect("in range"),
                ColourQuantisation::RoundHalfUp,
            )
            .expect("a create");
        let before = resident.indices.clone();

        let second = surface(4, vec![0, 1, 2, 0, 2, 3], 99);
        resident
            .apply(
                &uploader.plan(0, &second).expect("in range"),
                ColourQuantisation::RoundHalfUp,
            )
            .expect("a patch");
        assert_eq!(
            resident.indices, before,
            "a region write must not move indices"
        );
        assert_eq!(resident.colours.first(), Some(&[99_u8, 20, 30, 255]));
    }

    /// `surface_bytes` re-derives the mesher's per-vertex and per-index widths, so it is
    /// pinned to the mesher's own function on a surface that actually has geometry —
    /// an empty chunk would make the assertion `0 == 0` and pin nothing.
    #[test]
    fn the_byte_figures_agree_with_the_meshers_own_accounting() {
        let mut materials = vec![0_u8; pharmakos_mesher::CHUNK_VOLUME];
        for (index, slot) in materials.iter_mut().enumerate() {
            // A solid lower half, so the sweep emits a real surface. `checked_div`
            // because `integer_division` stays denied inside a walled crate.
            let half = pharmakos_mesher::CHUNK_VOLUME.checked_div(2).unwrap_or(0);
            if index < half {
                *slot = 1;
            }
        }
        let light = vec![15_u8; pharmakos_mesher::CHUNK_VOLUME];
        let input = ChunkInput::new(materials, light, ChunkInput::no_borders())
            .expect("a full chunk and no borders");
        let buffers: MeshBuffers =
            mesh_chunk(&input, LightParams::new(15, 1).expect("legal")).expect("a half chunk");
        let surface = Surface::from_mesh(&buffers).expect("under the cap");
        assert!(surface.vertex_count() > 0, "the case must have geometry");
        assert_eq!(
            surface_bytes(&surface),
            u64::try_from(gpu_surface_bytes(&buffers)).unwrap_or(u64::MAX),
            "the bridge must charge the byte budget in the mesher's own units"
        );
    }

    #[test]
    fn a_patch_whose_regions_do_not_fit_is_refused_rather_than_written() {
        let mut resident = ResidentSurface::default();
        let op = UploadOp::Patch {
            vertex_bytes: vec![0_u8; VERTEX_STRIDE],
            attribute_bytes: vec![0_u8; ATTRIBUTE_STRIDE],
        };
        let error = resident
            .apply(&op, ColourQuantisation::RoundHalfUp)
            .expect_err("nothing is resident, so nothing can be patched");
        assert!(matches!(error, BridgeError::Length { .. }), "{error}");
    }
}
