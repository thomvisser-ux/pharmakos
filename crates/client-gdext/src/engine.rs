// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The engine side of the upload: [`UploadOp`] in, `RenderingServer` or `ArrayMesh` calls
//! out.
//!
//! Nothing is decided here. [`crate::upload::Uploader`] decided it; this file is the
//! executor, and the only reason it is a file of its own is that everything in it names a
//! Godot type, so keeping it apart is what lets the decisions be tested with no engine in
//! the loop.
//!
//! # The fourth guard: the material must be owned, not borrowed by RID
//!
//! Item 53's fourth bug class, and the first real one path B produced in G1: a `Resource`
//! whose last Rust handle drops is freed, and an `Rid` copied out of it before that then
//! dangles. Godot reports it as a stream of "Parameter material is null" errors and draws
//! nothing. The fix is ownership, expressed in the type: [`Material`] holds the
//! `Gd<StandardMaterial3D>` for the renderer's whole life and only ever hands out the RID
//! through `&self`, so a RID cannot be obtained without a live handle to obtain it from.
//! A `get_rid()` on a temporary cannot be written through this type at all.
//!
//! # RIDs are not reference counted
//!
//! Nothing frees a mesh or an instance but [`ChunkRenderer::free_all`], which the node
//! calls from `_exit_tree`. That is the price of path B and it is paid here rather than
//! spread across the bridge.

use godot::builtin::{
    Aabb, Color, PackedByteArray, PackedColorArray, PackedInt32Array, PackedVector3Array,
    Transform3D, VarArray, Variant, Vector3,
};
use godot::classes::base_material_3d::{CullMode, Flags, ShadingMode};
use godot::classes::mesh::{ArrayType, PrimitiveType as MeshPrimitiveType};
use godot::classes::rendering_server::PrimitiveType;
use godot::classes::{ArrayMesh, MeshInstance3D, Node3D, RenderingServer, StandardMaterial3D};
use godot::meta::ToGodot;
use godot::obj::{EngineEnum, Gd, NewAlloc, NewGd, Singleton};
use godot::prelude::Rid;

use crate::error::BridgeError;
use crate::surface::{Surface, SurfaceProbe};
use crate::upload::{UploadOp, UploadPath, Uploader};

/// The unshaded, vertex-coloured material every chunk surface uses, owned for as long as
/// any RID taken from it is in use.
///
/// The mesher already folds the per-face shade and the baked light into the vertex
/// colour, so the material's whole job is to say "draw the vertex colour, cull the back
/// face, do not light it again".
#[derive(Debug)]
pub(crate) struct Material {
    handle: Gd<StandardMaterial3D>,
}

impl Material {
    /// Creates the material.
    #[must_use]
    pub(crate) fn new() -> Self {
        let mut handle = StandardMaterial3D::new_gd();
        handle.set_shading_mode(ShadingMode::UNSHADED);
        handle.set_flag(Flags::ALBEDO_FROM_VERTEX_COLOR, true);
        handle.set_cull_mode(CullMode::BACK);
        Self { handle }
    }

    /// The material's RID, borrowed from the live handle.
    ///
    /// The borrow is the guard: the returned `Rid` is `Copy`, but it cannot be obtained
    /// from a handle that is not alive, and this type never drops its handle while a
    /// [`ChunkRenderer`] holds it.
    #[must_use]
    pub(crate) fn rid(&self) -> Rid {
        self.handle.get_rid()
    }

    /// The material itself, for path A's `surface_set_material`.
    #[must_use]
    pub(crate) fn handle(&self) -> &Gd<StandardMaterial3D> {
        &self.handle
    }
}

impl Default for Material {
    fn default() -> Self {
        Self::new()
    }
}

/// What path B holds for one chunk.
#[derive(Debug)]
struct ChunkRids {
    mesh: Rid,
    instance: Rid,
}

/// Executes upload ops against the engine, on whichever path this build selected.
#[derive(Debug)]
pub(crate) struct ChunkRenderer {
    uploader: Uploader,
    material: Material,
    /// Path B: the scenario the instances are placed in.
    scenario: Rid,
    /// Path B: one mesh and one instance RID per chunk.
    rids: Vec<Option<ChunkRids>>,
    /// Path A: one `MeshInstance3D` per chunk, parented to the holder.
    instances: Vec<Option<Gd<MeshInstance3D>>>,
    /// Path A: the node the instances hang from.
    holder: Option<Gd<Node3D>>,
    /// Path A's resource churn, the figure item 53 weighed B against.
    resources_created: u64,
}

impl ChunkRenderer {
    /// A renderer for `chunks` chunks.
    ///
    /// `scenario` is path B's world scenario and `holder` is path A's parent node; each
    /// path ignores the other's. A caller that has neither — a headless run with no 3D
    /// world — passes `Rid::Invalid` and `None`, and the engine calls become the dummy
    /// renderer's no-ops rather than a panic.
    #[must_use]
    pub(crate) fn new(
        path: UploadPath,
        chunks: usize,
        scenario: Rid,
        holder: Option<Gd<Node3D>>,
    ) -> Self {
        Self {
            uploader: Uploader::new(path, chunks),
            material: Material::new(),
            scenario,
            rids: (0..chunks).map(|_| None).collect(),
            instances: (0..chunks).map(|_| None).collect(),
            holder,
            resources_created: 0,
        }
    }

    /// The uploader's decisions so far.
    #[must_use]
    pub(crate) const fn uploader(&self) -> &Uploader {
        &self.uploader
    }

    /// How many `ArrayMesh` resources path A has created. Zero on path B, which is the
    /// difference item 53 measured.
    #[must_use]
    pub(crate) const fn resources_created(&self) -> u64 {
        self.resources_created
    }

    /// Meshes-to-engine for one chunk: plan, then execute.
    ///
    /// `origin` is the chunk's corner in voxels, which is where the instance is placed.
    ///
    /// # Errors
    ///
    /// Whatever [`Uploader::plan`] refused.
    pub(crate) fn upload(
        &mut self,
        chunk: usize,
        origin: [f32; 3],
        surface: &Surface,
    ) -> Result<(), BridgeError> {
        let op = self.uploader.plan(chunk, surface)?;
        match self.uploader.path() {
            UploadPath::ArrayMesh => self.execute_path_a(chunk, origin, &op),
            UploadPath::RenderingServerRids => self.execute_path_b(chunk, origin, &op),
        }
        Ok(())
    }

    /// Path A: a fresh `ArrayMesh` on a `MeshInstance3D`.
    fn execute_path_a(&mut self, chunk: usize, origin: [f32; 3], op: &UploadOp) {
        let surface = match op {
            UploadOp::Create { surface } | UploadOp::Rebuild { surface, .. } => surface,
            // Path A never patches; the uploader guarantees it, and a patch reaching here
            // would be a bug in `rebuild_reason` rather than something to execute.
            UploadOp::Patch { .. } => return,
            UploadOp::Hide => {
                if let Some(Some(instance)) = self.instances.get_mut(chunk) {
                    instance.set_visible(false);
                }
                return;
            }
        };

        let mut mesh = ArrayMesh::new_gd();
        mesh.add_surface_from_arrays(MeshPrimitiveType::TRIANGLES, &surface_arrays(surface));
        mesh.surface_set_material(0, self.material.handle());
        mesh.set_custom_aabb(chunk_aabb());
        self.resources_created = self.resources_created.saturating_add(1);

        match self.instances.get_mut(chunk) {
            Some(Some(instance)) => {
                instance.set_mesh(&mesh);
                instance.set_visible(true);
            }
            Some(slot) => {
                let mut instance = MeshInstance3D::new_alloc();
                instance.set_name(&format!("chunk_{chunk}"));
                instance.set_position(Vector3::new(origin[0], origin[1], origin[2]));
                instance.set_mesh(&mesh);
                if let Some(holder) = self.holder.as_mut() {
                    holder.add_child(&instance);
                }
                *slot = Some(instance);
            }
            None => {}
        }
    }

    /// Path B: one mesh RID per chunk, patched in place where the guards allow.
    fn execute_path_b(&mut self, chunk: usize, origin: [f32; 3], op: &UploadOp) {
        let mut server = RenderingServer::singleton();
        let existing = self
            .rids
            .get(chunk)
            .and_then(Option::as_ref)
            .map(|rids| (rids.mesh, rids.instance));

        match (op, existing) {
            (UploadOp::Hide, Some((_, instance))) => {
                server.instance_set_visible(instance, false);
            }
            (UploadOp::Create { surface }, _) => {
                let mesh = server.mesh_create();
                server.mesh_add_surface_from_arrays(
                    mesh,
                    PrimitiveType::TRIANGLES,
                    &surface_arrays(surface),
                );
                server.mesh_surface_set_material(mesh, 0, self.material.rid());
                server.mesh_set_custom_aabb(mesh, chunk_aabb());

                let instance = server.instance_create2(mesh, self.scenario);
                server.instance_set_transform(
                    instance,
                    Transform3D::IDENTITY.translated(Vector3::new(origin[0], origin[1], origin[2])),
                );
                if let Some(slot) = self.rids.get_mut(chunk) {
                    *slot = Some(ChunkRids { mesh, instance });
                }
                // The layout is a property of the format, so it is read back once, from
                // the first surface the engine actually built.
                let probe = read_back_probe(&mut server, mesh, surface);
                self.uploader.record_probe(probe);
            }
            (UploadOp::Rebuild { surface, .. }, Some((mesh, instance))) => {
                server.mesh_clear(mesh);
                server.mesh_add_surface_from_arrays(
                    mesh,
                    PrimitiveType::TRIANGLES,
                    &surface_arrays(surface),
                );
                server.mesh_surface_set_material(mesh, 0, self.material.rid());
                server.mesh_set_custom_aabb(mesh, chunk_aabb());
                server.instance_set_visible(instance, true);
            }
            (
                UploadOp::Patch {
                    vertex_bytes,
                    attribute_bytes,
                },
                Some((mesh, instance)),
            ) => {
                // The only write that touches the engine's buffers directly. It is
                // reached only when the index array is unchanged and the probed strides
                // and colour conversion are the ones these bytes were written for.
                server.mesh_surface_update_vertex_region(
                    mesh,
                    0,
                    0,
                    &PackedByteArray::from(vertex_bytes.as_slice()),
                );
                server.mesh_surface_update_attribute_region(
                    mesh,
                    0,
                    0,
                    &PackedByteArray::from(attribute_bytes.as_slice()),
                );
                server.instance_set_visible(instance, true);
            }
            // Nothing to do. `Hide` with nothing resident is the ordinary case of a
            // chunk that has always been empty. A rebuild or a patch with nothing
            // resident cannot happen at all — the uploader returns `Create` for a chunk
            // it has not seen — and is ignored rather than unwrapped, because a panic
            // here would be caught silently at the `#[func]` boundary above.
            (UploadOp::Hide | UploadOp::Rebuild { .. } | UploadOp::Patch { .. }, None) => {}
        }
    }

    /// Frees every mesh and instance this renderer created, and drops path A's nodes.
    ///
    /// RIDs are not reference counted; nothing else will do this. Called from the node's
    /// `_exit_tree`.
    pub(crate) fn free_all(&mut self) {
        let mut server = RenderingServer::singleton();
        for slot in &mut self.rids {
            if let Some(rids) = slot.take() {
                server.free_rid(rids.instance);
                server.free_rid(rids.mesh);
            }
        }
        for slot in &mut self.instances {
            if let Some(mut instance) = slot.take() {
                instance.queue_free();
            }
        }
    }
}

/// The surface arrays both paths hand `add_surface_from_arrays`.
///
/// Position and colour only — the format `crate::surface` documents. The colours go
/// across as `Color`, which is what makes the quantisation the engine's and so makes the
/// probe necessary.
fn surface_arrays(surface: &Surface) -> VarArray {
    let vertices = surface.vertex_count();

    let mut positions = PackedVector3Array::new();
    positions.resize(vertices);
    {
        let slots = positions.as_mut_slice();
        for (slot, position) in slots.iter_mut().zip(surface.positions()) {
            *slot = Vector3::new(position[0], position[1], position[2]);
        }
    }

    let mut colours = PackedColorArray::new();
    colours.resize(vertices);
    {
        let slots = colours.as_mut_slice();
        for (slot, colour) in slots.iter_mut().zip(surface.colour_floats()) {
            *slot = Color::from_rgba(colour[0], colour[1], colour[2], colour[3]);
        }
    }

    let mut indices = PackedInt32Array::new();
    indices.resize(surface.indices().len());
    {
        let slots = indices.as_mut_slice();
        for (slot, index) in slots.iter_mut().zip(surface.indices()) {
            *slot = i32::from(*index);
        }
    }

    let mut arrays = VarArray::new();
    arrays.resize(
        ArrayType::MAX.ord().try_into().unwrap_or(0),
        &Variant::nil(),
    );
    let vertex_slot: usize = ArrayType::VERTEX.ord().try_into().unwrap_or(0);
    let colour_slot: usize = ArrayType::COLOR.ord().try_into().unwrap_or(0);
    let index_slot: usize = ArrayType::INDEX.ord().try_into().unwrap_or(0);
    arrays.set(vertex_slot, &positions.to_variant());
    arrays.set(colour_slot, &colours.to_variant());
    arrays.set(index_slot, &indices.to_variant());
    arrays
}

/// One chunk's bounding box, so the renderer does not compute one from the vertices.
fn chunk_aabb() -> Aabb {
    let edge = f32::from(u16::try_from(pharmakos_mesher::CHUNK_EDGE).unwrap_or(32));
    Aabb::new(Vector3::ZERO, Vector3::splat(edge))
}

/// Asks the engine what it built, and turns the answer into a [`SurfaceProbe`].
///
/// Everything here is a read-back. A surface the engine did not build — the dummy
/// renderer under `--headless` returns nothing — produces a probe that says "not
/// patchable", which is the correct answer and not a failure.
fn read_back_probe(server: &mut Gd<RenderingServer>, mesh: Rid, surface: &Surface) -> SurfaceProbe {
    let described = server.mesh_get_surface(mesh, 0);
    let vertex_data = described
        .get("vertex_data")
        .and_then(|value| value.try_to::<PackedByteArray>().ok());
    let attribute_data = described
        .get("attribute_data")
        .and_then(|value| value.try_to::<PackedByteArray>().ok());
    let vertex_count = described
        .get("vertex_count")
        .and_then(|value| value.try_to::<i64>().ok())
        .and_then(|count| usize::try_from(count).ok())
        .unwrap_or(0);

    match (vertex_data, attribute_data) {
        (Some(vertex_data), Some(attribute_data)) => SurfaceProbe::read_back(
            surface,
            vertex_data.as_slice(),
            attribute_data.as_slice(),
            vertex_count,
        ),
        _ => SurfaceProbe::default(),
    }
}
