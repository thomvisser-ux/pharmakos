// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later
//! **Path B — direct `RenderingServer` RIDs.**
//!
//! Presentation side of the wall. `mesh_create()` once per chunk to get a RID,
//! `mesh_add_surface_from_arrays` on first build, then
//! `mesh_surface_update_vertex_region` / `mesh_surface_update_attribute_region`
//! **in place** whenever the surface's *index array is unchanged*, with
//! `instance_create2` + `instance_set_transform` + `instance_set_scenario` for
//! placement. No scene-tree node per chunk and no resource churn.
//!
//! Four hazards from the plan's §6 risk table are handled here explicitly:
//!
//! * **Surface-update format strictness.** Rather than *assume* Godot's buffer
//!   layout, the first surface built is read back with `mesh_get_surface` and
//!   the real strides are derived from `vertex_data.size() / vertex_count` and
//!   `attribute_data.size() / vertex_count`. In-place updates are only ever
//!   attempted when those come back as the expected 12 (a `Vector3` position)
//!   and 4 (RGBA8 colour); otherwise every remesh takes the rebuild path and
//!   says so in the counters. That is why the mesher's surface format is
//!   deliberately minimal (see `greedy.rs`).
//! * **The index buffer is not patchable, so it must not need patching.**
//!   `write_regions` rewrites the vertex and attribute regions only — there is
//!   no safe `mesh_surface_update_index_region` call to make here, because the
//!   surface's index width is 16-bit at these vertex counts while the mesher
//!   hands Godot a `PackedInt32Array`, and writing that region raw would
//!   corrupt it. An equal vertex *count* does **not** imply an equal index
//!   array: each quad emits `v0,v0+1,v0+2,v0,v0+2,v0+3` or the reversed pattern
//!   depending on its face sign, so two remeshes with the same totals but a
//!   different split of quads across the six directions have different indices.
//!   The guard therefore compares the emitted index array itself and falls
//!   through to a full rebuild on any difference. The first version of this file
//!   compared only the counts, and 7 of 584 in-place updates in the headline run
//!   left a chunk rendering with stale winding — back-face-culled holes that no
//!   timing number would ever have shown.
//! * **Colour quantisation.** `mesh_add_surface_from_arrays` lets Godot convert
//!   `Color` → RGBA8 itself; `write_regions` has to do the same conversion by
//!   hand, and getting it off by one least-significant bit makes a chunk's shade
//!   depend on whether its last update happened to take the in-place path. So
//!   the conversion is *probed*, not assumed: the first surface's
//!   `attribute_data` is read back and matched against both a truncating and a
//!   round-half-up conversion of the colours we handed in. Whichever reproduces
//!   Godot's bytes exactly is used; if neither does, in-place updates are
//!   disabled for the run and the counter says so.
//! * **Render-thread affinity.** Every call here is made from the main thread,
//!   and `project.godot` pins `rendering/driver/threads/thread_model = 1`
//!   (Single-Safe) so the main thread *is* the thread that owns the
//!   `RenderingServer`. Recorded, because it constrains the skeleton's design.
//! * **Manual lifetime.** RIDs are not reference counted. `free_all` releases
//!   every instance and mesh; it is called from `_exit_tree`.

use godot::classes::rendering_server::PrimitiveType;
use godot::classes::{Material, RenderingServer};
use godot::prelude::*;

use crate::greedy::MeshBuf;
use crate::upload_arraymesh::build_arrays;
use crate::world::{CHUNK_COUNT, chunk_origin};

const EXPECTED_VERTEX_STRIDE: usize = 12; // Vector3 position
const EXPECTED_ATTRIB_STRIDE: usize = 4; // RGBA8 colour

/// How Godot turns a `Color` channel into the byte it stores. Probed, never
/// assumed — see the module header.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColourConv {
    /// `uint8_t(clamp(c * 255, 0, 255))` — a truncating cast.
    Truncate,
    /// `round(c * 255)`.
    RoundHalfUp,
}

impl ColourConv {
    #[inline]
    fn apply(self, c: f32) -> u8 {
        let v = c.clamp(0.0, 1.0) * 255.0;
        let v = match self {
            Self::Truncate => v,
            Self::RoundHalfUp => v + 0.5,
        };
        // `f32 as u8` saturates in Rust; the clamp above already bounds it.
        v as u8
    }
}

struct ChunkRids {
    mesh: Rid,
    instance: Rid,
    verts: usize,
    /// The index array as emitted last time. The in-place fast path is only
    /// legal when the new one is byte-for-byte the same (module header).
    indices: Vec<i32>,
}

pub struct RsPath {
    scenario: Rid,
    /// The material is held as a `Gd`, not just its RID: a `Resource` whose last
    /// Rust handle is dropped is freed, and the RID that was copied out of it
    /// then dangles. Godot reports this as a stream of "Parameter material is
    /// null" errors from the renderer and draws nothing — the first real bug
    /// path B produced, and worth recording because the skeleton will meet it
    /// again the moment it holds RIDs by hand.
    material: Gd<Material>,
    rids: Vec<Option<ChunkRids>>,
    /// Observed strides, learned from the first `mesh_get_surface` read-back.
    strides: Option<(usize, usize)>,
    /// Observed colour quantisation, learned from the same read-back. `None`
    /// means "not probed yet or nothing matched" and disables in-place updates.
    colour_conv: Option<ColourConv>,
    pub first_builds: u64,
    pub inplace_updates: u64,
    pub rebuilds: u64,
    /// Rebuilds forced because the layout was not the one we can patch.
    pub rebuilds_format: u64,
    /// Rebuilds forced because the index array changed even though the vertex
    /// count did not — the cases a count-only guard would have got wrong.
    pub rebuilds_index_changed: u64,
    scratch_v: Vec<u8>,
    scratch_a: Vec<u8>,
}

impl RsPath {
    pub fn new(scenario: Rid, material: Gd<Material>) -> Self {
        Self {
            scenario,
            material,
            rids: (0..CHUNK_COUNT).map(|_| None).collect(),
            strides: None,
            colour_conv: None,
            first_builds: 0,
            inplace_updates: 0,
            rebuilds: 0,
            rebuilds_format: 0,
            rebuilds_index_changed: 0,
            scratch_v: Vec::new(),
            scratch_a: Vec::new(),
        }
    }

    pub fn live_instances(&self) -> usize {
        self.rids.iter().filter(|r| r.is_some()).count()
    }

    pub fn colour_conv(&self) -> Option<ColourConv> {
        self.colour_conv
    }

    /// Ratio of in-place updates to all updates of an existing surface.
    pub fn inplace_ratio_ppm(&self) -> u64 {
        let total = self.inplace_updates + self.rebuilds;
        if total == 0 {
            return 0;
        }
        self.inplace_updates.saturating_mul(1_000_000) / total
    }

    /// Upload one chunk surface. Returns the number of bytes this branch
    /// **actually wrote across the FFI** — 28 B/vertex + 4 B/index for a build
    /// or rebuild (`PackedVector3Array` + `PackedColorArray` +
    /// `PackedInt32Array`), but only 12 + 4 B/vertex and no indices for an
    /// in-place region write. Charging `buf.bytes()` for both would inflate
    /// path B's throughput and make the byte budget *B* mean two different
    /// things on the two paths.
    pub fn upload(&mut self, ci: usize, buf: &MeshBuf) -> usize {
        let mut rs = RenderingServer::singleton();

        if buf.is_empty() {
            if let Some(r) = self.rids[ci].as_ref() {
                rs.instance_set_visible(r.instance, false);
            }
            return 0;
        }

        let full_bytes = buf.bytes();
        let inplace_bytes = buf.vertex_count() * (EXPECTED_VERTEX_STRIDE + EXPECTED_ATTRIB_STRIDE);

        // Decide against the stored surface first, then let the borrow go: the
        // branches below need `&mut self` for the counters and the scratch
        // buffers.
        let existing: Option<(Rid, Rid, bool, bool)> = self.rids[ci].as_ref().map(|r| {
            let same_verts = r.verts == buf.vertex_count();
            (
                r.mesh,
                r.instance,
                same_verts,
                same_verts && r.indices == buf.idx,
            )
        });

        match existing {
            None => {
                let mesh = rs.mesh_create();
                rs.mesh_add_surface_from_arrays(mesh, PrimitiveType::TRIANGLES, &build_arrays(buf));
                rs.mesh_surface_set_material(mesh, 0, self.material.get_rid());
                rs.mesh_set_custom_aabb(mesh, Aabb::new(Vector3::ZERO, Vector3::splat(32.0)));

                let instance = rs.instance_create2(mesh, self.scenario);
                let (ox, oy, oz) = chunk_origin(ci);
                rs.instance_set_transform(
                    instance,
                    Transform3D::IDENTITY.translated(Vector3::new(ox as f32, oy as f32, oz as f32)),
                );
                self.rids[ci] = Some(ChunkRids {
                    mesh,
                    instance,
                    verts: buf.vertex_count(),
                    indices: buf.idx.clone(),
                });
                self.first_builds += 1;
                if self.strides.is_none() {
                    self.probe_surface(&mut rs, mesh, buf);
                }
                full_bytes
            }
            Some((mesh, instance, same_verts, same_indices)) => {
                // Equal vertex count is necessary but NOT sufficient: the index
                // array has to be identical too, or the untouched index buffer
                // renders the new vertices with the old winding.
                let patchable = self.strides
                    == Some((EXPECTED_VERTEX_STRIDE, EXPECTED_ATTRIB_STRIDE))
                    && self.colour_conv.is_some();

                if same_indices && patchable {
                    self.write_regions(&mut rs, mesh, buf);
                    self.inplace_updates += 1;
                    rs.instance_set_visible(instance, true);
                    inplace_bytes
                } else {
                    rs.mesh_clear(mesh);
                    rs.mesh_add_surface_from_arrays(
                        mesh,
                        PrimitiveType::TRIANGLES,
                        &build_arrays(buf),
                    );
                    rs.mesh_surface_set_material(mesh, 0, self.material.get_rid());
                    rs.mesh_set_custom_aabb(mesh, Aabb::new(Vector3::ZERO, Vector3::splat(32.0)));
                    self.rids[ci] = Some(ChunkRids {
                        mesh,
                        instance,
                        verts: buf.vertex_count(),
                        indices: buf.idx.clone(),
                    });
                    self.rebuilds += 1;
                    if same_indices && !patchable {
                        self.rebuilds_format += 1;
                    }
                    if same_verts && !same_indices {
                        self.rebuilds_index_changed += 1;
                    }
                    rs.instance_set_visible(instance, true);
                    full_bytes
                }
            }
        }
    }

    /// Ask Godot what it actually built, instead of assuming: the buffer strides
    /// *and* the colour quantisation.
    fn probe_surface(&mut self, rs: &mut RenderingServer, mesh: Rid, buf: &MeshBuf) {
        if buf.vertex_count() == 0 {
            return;
        }
        let d = rs.mesh_get_surface(mesh, 0);
        let (Some(vd), Some(ad)) = (
            d.get("vertex_data")
                .and_then(|v| v.try_to::<PackedByteArray>().ok()),
            d.get("attribute_data")
                .and_then(|v| v.try_to::<PackedByteArray>().ok()),
        ) else {
            godot_error!("[g1] path B: mesh_get_surface returned no buffers; in-place disabled");
            return;
        };
        let vc = d
            .get("vertex_count")
            .and_then(|v| v.try_to::<i64>().ok())
            .map(|v| v.max(1) as usize)
            .unwrap_or(buf.vertex_count());
        let strides = (vd.len() / vc, ad.len() / vc);
        self.strides = Some(strides);

        // Which conversion reproduces Godot's bytes exactly?
        self.colour_conv = if strides.1 == EXPECTED_ATTRIB_STRIDE {
            let bytes = ad.as_slice();
            [ColourConv::Truncate, ColourConv::RoundHalfUp]
                .into_iter()
                .find(|&conv| {
                    buf.col.iter().take(vc).enumerate().all(|(i, c)| {
                        (0..4).all(|k| {
                            bytes
                                .get(i * EXPECTED_ATTRIB_STRIDE + k)
                                .copied()
                                .is_some_and(|b| b == conv.apply(c[k]))
                        })
                    })
                })
        } else {
            None
        };

        godot_print!(
            "[g1] path B surface probe: vertex_data={} attribute_data={} vertex_count={} \
             -> strides {}/{}  colour conversion {:?}",
            vd.len(),
            ad.len(),
            vc,
            strides.0,
            strides.1,
            self.colour_conv
        );
        if self.colour_conv.is_none() {
            godot_error!(
                "[g1] path B: no colour conversion reproduced Godot's attribute bytes; \
                 in-place updates disabled (every remesh will rebuild)"
            );
        }
    }

    /// In-place region writes. Only reached when the index array is unchanged
    /// *and* the probed strides and colour conversion match, so both regions are
    /// exactly the size of the buffers already allocated for the surface and the
    /// bytes are the ones Godot would have written itself.
    fn write_regions(&mut self, rs: &mut RenderingServer, mesh: Rid, buf: &MeshBuf) {
        let conv = match self.colour_conv {
            Some(c) => c,
            None => return,
        };
        let n = buf.pos.len();
        self.scratch_v.clear();
        self.scratch_v.reserve(n * EXPECTED_VERTEX_STRIDE);
        for p in &buf.pos {
            self.scratch_v.extend_from_slice(&p[0].to_le_bytes());
            self.scratch_v.extend_from_slice(&p[1].to_le_bytes());
            self.scratch_v.extend_from_slice(&p[2].to_le_bytes());
        }
        self.scratch_a.clear();
        self.scratch_a.reserve(n * EXPECTED_ATTRIB_STRIDE);
        for c in &buf.col {
            for ch in c {
                self.scratch_a.push(conv.apply(*ch));
            }
        }
        let vb = PackedByteArray::from(self.scratch_v.as_slice());
        let ab = PackedByteArray::from(self.scratch_a.as_slice());
        rs.mesh_surface_update_vertex_region(mesh, 0, 0, &vb);
        rs.mesh_surface_update_attribute_region(mesh, 0, 0, &ab);
    }

    /// RIDs are not reference counted; nothing frees these but us.
    pub fn free_all(&mut self) {
        let mut rs = RenderingServer::singleton();
        for slot in &mut self.rids {
            if let Some(r) = slot.take() {
                rs.free_rid(r.instance);
                rs.free_rid(r.mesh);
            }
        }
    }
}
