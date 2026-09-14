// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later
//! Spike G1 — destruction remesh. Throwaway (`spikes/README.md`).
//!
//! # Where the wall is
//!
//! This crate is deliberately split down the middle of AGENTS.md §4.9:
//!
//! | module | side | floats |
//! |---|---|---|
//! | `chunk`, `world`, `explode`, `budget`, `stats` | sim | **denied**, per-file `#![deny(clippy::float_arithmetic)]` |
//! | `greedy`, `upload_arraymesh`, `upload_rs`, the `G1World` node | presentation | allowed |
//!
//! The float boundary starts at vertex generation and nowhere earlier: which
//! voxels an explosion removes, which chunks that dirties, and the order the
//! queue drains them in are all integer. In the product this split is a *crate*
//! boundary, not a module boundary — which is the point G1 has to report back.
//!
//! # Building
//!
//! ```text
//! cargo build --release                                     # cdylib for Godot
//! cargo build --release --no-default-features --bin meshbench   # no gdext at all
//! ```

pub mod budget;
pub mod chunk;
pub mod explode;
pub mod greedy;
pub mod stats;
pub mod world;

#[cfg(feature = "gdext")]
mod upload_arraymesh;
#[cfg(feature = "gdext")]
mod upload_rs;

#[cfg(feature = "gdext")]
mod gdext_node {
    use std::time::Instant;

    use godot::classes::base_material_3d::{CullMode, Flags, ShadingMode};
    use godot::classes::{INode3D, Node3D, StandardMaterial3D};
    use godot::prelude::*;

    use crate::budget::{Budget, UploadQueue, drain};
    use crate::explode::{Explosion, Schedule, explode};
    use crate::greedy::{MeshBuf, Mesher};
    use crate::stats::summarise;
    use crate::upload_arraymesh::ArrayMeshPath;
    use crate::upload_rs::RsPath;
    use crate::world::World;

    struct G1Extension;

    #[gdextension]
    unsafe impl ExtensionLibrary for G1Extension {}

    /// The GDExtension class the scene drives. `main.gd` calls `configure`,
    /// `build`, then `step` once per frame.
    ///
    /// Panics: gdext installs a catch at every `#[func]` boundary, so a panic in
    /// the mesher surfaces as a Godot error and a returned default instead of
    /// unwinding into C++ (plan §6, "Panics across FFI"). That catch is silent
    /// as far as the CSV is concerned, so `upload_chunk` carries its own
    /// `catch_unwind` and counts what it caught; `panics` is emitted in the
    /// summary and `analyse.py` asserts it is 0. A panic under load is a
    /// finding, not something to shrug at, and an instrument that is only
    /// claimed in a doc comment is not an instrument.
    #[derive(GodotClass)]
    #[class(base = Node3D)]
    pub struct G1World {
        base: Base<Node3D>,

        world: Option<World>,
        mesher: Mesher,
        buf: MeshBuf,
        queue: UploadQueue,
        budget: Budget,
        schedule: Schedule,

        /// 0 = path A (ArrayMesh), 1 = path B (RenderingServer RIDs).
        path: u8,
        a: Option<ArrayMeshPath>,
        b: Option<RsPath>,

        frame: u64,
        eps: u64,
        target_fps: u64,
        seed: u64,
        cam: (i32, i32, i32),

        explosions_issued: u64,
        chunks_uploaded: u64,
        bytes_uploaded: u64,
        voxels_removed: u64,
        latencies: Vec<u32>,
        mesh_us: Vec<u64>,
        edit_us_total: u64,
        frame_mesh_ns: u64,
        frame_upload_ns: u64,
        last_step: Option<Instant>,
        /// Panics caught inside `upload_chunk`. Must be 0.
        panics: u64,
    }

    #[godot_api]
    impl INode3D for G1World {
        fn init(base: Base<Node3D>) -> Self {
            Self {
                base,
                world: None,
                mesher: Mesher::new(),
                buf: MeshBuf::default(),
                queue: UploadQueue::new(),
                budget: Budget::UNBOUNDED,
                schedule: Schedule::new(0xA1B2_C3D4, 4),
                path: 0,
                a: None,
                b: None,
                frame: 0,
                eps: 20,
                target_fps: 60,
                seed: 1,
                cam: (192, 96, 32),
                explosions_issued: 0,
                chunks_uploaded: 0,
                bytes_uploaded: 0,
                voxels_removed: 0,
                latencies: Vec::new(),
                mesh_us: Vec::new(),
                edit_us_total: 0,
                frame_mesh_ns: 0,
                frame_upload_ns: 0,
                last_step: None,
                panics: 0,
            }
        }

        fn exit_tree(&mut self) {
            if let Some(b) = self.b.as_mut() {
                b.free_all();
            }
        }
    }

    #[godot_api]
    impl G1World {
        /// `k`/`b` of 0 mean unbounded.
        #[allow(clippy::too_many_arguments)]
        #[func]
        fn configure(
            &mut self,
            path: i32,
            k: i32,
            b: i32,
            explosions_per_s: i32,
            radius: i32,
            seed: i64,
            flip_winding_override: bool,
        ) {
            self.path = u8::from(path != 0);
            self.budget = Budget {
                k: if k <= 0 { usize::MAX } else { k as usize },
                b: if b <= 0 { usize::MAX } else { b as usize },
            };
            self.eps = u64::try_from(explosions_per_s.max(0)).unwrap_or(20);
            self.seed = u64::try_from(seed).unwrap_or(1);
            self.schedule = Schedule::new(self.seed ^ 0xA1B2_C3D4, radius.max(1));
            // `flip_winding_override` XORs the *measured* default rather than
            // replacing it, so `--flip_winding=0` means "use what the screenshot
            // said is right" and `=1` means "try the other handedness". An
            // earlier version assigned it directly, which silently undid the
            // winding fix on every run that did not pass the flag.
            self.mesher.flip_winding = crate::greedy::CLOCKWISE_FRONT != flip_winding_override;
        }

        #[func]
        fn set_camera_voxel(&mut self, p: Vector3) {
            self.cam = (p.x as i32, p.y as i32, p.z as i32);
        }

        /// Generate the world, mesh every non-empty chunk and upload the lot
        /// (unbudgeted — this is scene load, not the measured steady state).
        #[func]
        fn build(&mut self) -> VarDictionary {
            let t0 = Instant::now();
            let world = World::generate(self.seed);
            let gen_ms = t0.elapsed().as_millis() as i64;

            let nonempty = world.nonempty_chunks();
            self.world = Some(world);

            let mut mat = StandardMaterial3D::new_gd();
            // Unshaded + albedo-from-vertex-colour: the face shade and the
            // flood-fill light are already baked into the vertex colours, which
            // is what lets the surface format stay position + colour only.
            mat.set_shading_mode(ShadingMode::UNSHADED);
            mat.set_flag(Flags::ALBEDO_FROM_VERTEX_COLOR, true);
            mat.set_cull_mode(CullMode::BACK);
            let material = mat.upcast::<godot::classes::Material>();

            if self.path == 0 {
                let holder = Node3D::new_alloc();
                self.base_mut().add_child(&holder);
                self.a = Some(ArrayMeshPath::new(holder, material));
            } else {
                let scenario = self
                    .base()
                    .get_world_3d()
                    .map(|w| w.get_scenario())
                    .unwrap_or(Rid::Invalid);
                self.b = Some(RsPath::new(scenario, material));
            }

            let t1 = Instant::now();
            let mut bytes = 0u64;
            let mut verts = 0u64;
            for &ci in &nonempty {
                bytes += self.upload_chunk(ci) as u64;
                verts += self.buf.vertex_count() as u64;
            }
            let upload_ms = t1.elapsed().as_millis() as i64;

            // Scene load is not part of the measured steady state.
            self.mesh_us.clear();
            self.chunks_uploaded = 0;
            self.bytes_uploaded = 0;

            let mut d = VarDictionary::new();
            d.set("gen_ms", gen_ms);
            d.set("initial_mesh_upload_ms", upload_ms);
            d.set("nonempty_chunks", nonempty.len() as i64);
            d.set("initial_bytes", bytes as i64);
            d.set("initial_vertices", verts as i64);
            d.set(
                "solid_voxels",
                self.world.as_ref().map_or(0, |w| w.solid_voxels()) as i64,
            );
            d.set("path", i64::from(self.path));
            d.set(
                "budget_k",
                if self.budget.k == usize::MAX {
                    0
                } else {
                    self.budget.k as i64
                },
            );
            d.set(
                "budget_b",
                if self.budget.b == usize::MAX {
                    0
                } else {
                    self.budget.b as i64
                },
            );
            d
        }

        /// One frame of the measured run: issue the explosions that are due,
        /// then drain the upload queue under the budget.
        #[func]
        fn step(&mut self) -> VarDictionary {
            let now = Instant::now();
            let ext_dt_us = self
                .last_step
                .map(|t| now.duration_since(t).as_micros() as i64)
                .unwrap_or(0);
            self.last_step = Some(now);

            self.frame += 1;
            self.frame_mesh_ns = 0;
            self.frame_upload_ns = 0;

            // Explosion schedule is in *frames*, not wall time: one explosion
            // every 3 frames at 20/s and 60 fps, exactly as the gate words it.
            let due = self.frame.saturating_mul(self.eps) / self.target_fps;
            let mut issued_now = 0i64;
            let t_edit = Instant::now();
            while self.explosions_issued < due {
                let n = self.explosions_issued;
                let e: Explosion = {
                    let w = match self.world.as_ref() {
                        Some(w) => w,
                        None => break,
                    };
                    self.schedule.nth(w, n)
                };
                if let Some(w) = self.world.as_mut() {
                    let blast = explode(w, &e);
                    self.voxels_removed += u64::from(blast.removed);
                    for c in blast.dirty {
                        self.queue.push(c, self.frame);
                    }
                }
                self.explosions_issued += 1;
                issued_now += 1;
            }
            self.edit_us_total += t_edit.elapsed().as_micros() as u64;

            let queued_before = self.queue.len() as i64;

            let mut q = std::mem::take(&mut self.queue);
            let cam = self.cam;
            let budget = self.budget;
            let frame = self.frame;
            let r = drain(&mut q, cam, budget, frame, |ci, _| self.upload_chunk(ci));
            self.queue = q;

            self.chunks_uploaded += r.uploaded as u64;
            self.bytes_uploaded += r.bytes as u64;
            let mut lat_max = 0u32;
            for l in &r.latencies {
                lat_max = lat_max.max(*l);
                self.latencies.push(*l);
            }

            let mut d = VarDictionary::new();
            d.set("frame", self.frame as i64);
            d.set("ext_dt_us", ext_dt_us);
            d.set("explosions", issued_now);
            d.set("queued_before", queued_before);
            d.set("queued_after", self.queue.len() as i64);
            d.set("uploaded_chunks", r.uploaded as i64);
            d.set("uploaded_bytes", r.bytes as i64);
            d.set("deferred_for_bytes", r.deferred_for_bytes as i64);
            d.set("remesh_us", (self.frame_mesh_ns / 1_000) as i64);
            d.set("upload_us", (self.frame_upload_ns / 1_000) as i64);
            d.set("lat_max_this_frame", i64::from(lat_max));
            d
        }

        #[func]
        fn summary(&mut self) -> VarDictionary {
            let mut lat: Vec<u64> = self.latencies.iter().map(|&l| u64::from(l)).collect();
            let ls = summarise(&mut lat);
            let ms = summarise(&mut self.mesh_us.clone());

            // Latency is only sampled when a chunk is *uploaded*, so a
            // configuration whose queue runs away hides its worst chunks: at
            // K = 1 the 65 oldest never got uploaded at all and contributed no
            // sample. The inclusive series counts them at their current age, so
            // the sweep table cannot flatter a failing rung.
            let leftover = self.queue.pending_ages(self.frame);
            let mut lat_incl = lat.clone();
            lat_incl.extend_from_slice(&leftover);
            let li = summarise(&mut lat_incl);

            let mut d = VarDictionary::new();
            d.set("lat_pending_n", leftover.len() as i64);
            d.set("lat_incl_pending_p99", li.p99 as i64);
            d.set("lat_incl_pending_max", li.max as i64);
            d.set("panics", self.panics as i64);
            d.set("path", i64::from(self.path));
            d.set("frames", self.frame as i64);
            d.set("explosions", self.explosions_issued as i64);
            d.set("voxels_removed", self.voxels_removed as i64);
            d.set("chunks_uploaded", self.chunks_uploaded as i64);
            d.set("bytes_uploaded", self.bytes_uploaded as i64);
            d.set("lat_n", ls.n as i64);
            d.set("lat_p50", ls.p50 as i64);
            d.set("lat_p90", ls.p90 as i64);
            d.set("lat_p99", ls.p99 as i64);
            d.set("lat_max", ls.max as i64);
            d.set("mesh_us_p50", ms.p50 as i64);
            d.set("mesh_us_p90", ms.p90 as i64);
            d.set("mesh_us_p99", ms.p99 as i64);
            d.set("mesh_us_max", ms.max as i64);
            d.set("edit_us_total", self.edit_us_total as i64);
            d.set("max_queue_depth", self.queue.max_depth as i64);
            d.set("coalesced", self.queue.coalesced as i64);
            d.set("enqueued", self.queue.enqueued as i64);
            if let Some(a) = self.a.as_ref() {
                d.set("resources_created", a.resources_created as i64);
                d.set("instances", a.live_instances() as i64);
            }
            if let Some(b) = self.b.as_ref() {
                d.set("first_builds", b.first_builds as i64);
                d.set("inplace_updates", b.inplace_updates as i64);
                d.set("rebuilds", b.rebuilds as i64);
                d.set("rebuilds_format", b.rebuilds_format as i64);
                d.set("rebuilds_index_changed", b.rebuilds_index_changed as i64);
                d.set("inplace_ratio_ppm", b.inplace_ratio_ppm() as i64);
                d.set("colour_conv", format!("{:?}", b.colour_conv()));
                d.set("instances", b.live_instances() as i64);
            }
            d
        }

        /// Mesh chunk `ci` and hand the surface to the selected upload path.
        /// Returns the bytes the chosen upload branch actually wrote.
        ///
        /// The `catch_unwind` is the instrument plan §6's "Panics across FFI"
        /// row asks for: gdext's own catch at the `#[func]` boundary would
        /// swallow a mesher panic into a default return value and the run would
        /// carry on looking healthy, so the count is kept here and reported.
        fn upload_chunk(&mut self, ci: u32) -> usize {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.upload_chunk_inner(ci)
            }));
            match r {
                Ok(bytes) => bytes,
                Err(_) => {
                    self.panics += 1;
                    godot_error!("[g1] panic caught while meshing/uploading chunk {ci}");
                    0
                }
            }
        }

        fn upload_chunk_inner(&mut self, ci: u32) -> usize {
            let t0 = Instant::now();
            if let Some(w) = self.world.as_ref() {
                self.mesher.mesh(w, ci as usize, &mut self.buf);
            }
            let mesh_ns = t0.elapsed().as_nanos() as u64;
            self.frame_mesh_ns += mesh_ns;
            self.mesh_us.push(mesh_ns / 1_000);

            let t1 = Instant::now();
            let bytes = if self.path == 0 {
                match self.a.as_mut() {
                    Some(a) => a.upload(ci as usize, &self.buf),
                    None => 0,
                }
            } else {
                match self.b.as_mut() {
                    Some(b) => b.upload(ci as usize, &self.buf),
                    None => 0,
                }
            };
            self.frame_upload_ns += t1.elapsed().as_nanos() as u64;
            bytes
        }
    }
}

#[cfg(feature = "gdext")]
pub use gdext_node::G1World;
