// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `PharmakosBridge`, the one class GDScript talks to, and the self-check CI runs.
//!
//! Every `#[func]` below does the same three things and nothing else: take Godot's types
//! apart, call a module that knows how to marshal, put Godot's types back together. Each
//! one goes through [`PanicCounter::guard`], because gdext's own catch at this boundary is
//! silent and a default return value that nobody flagged is the failure mode the count
//! exists to make visible (G1 section 10.12).
//!
//! # The self-check
//!
//! [`self_check`] is T12's acceptance in one function, and it runs with no GPU, no world
//! and no gateway:
//!
//! 1. a rules table is encoded to canonical JSON and read back, and the mesher's five
//!    parameters come out of it;
//! 2. a chunk is built in **sim** order, transposed into **mesher** order, lit, and meshed
//!    by the real mesher;
//! 3. the same chunk set is driven through **path A** and **path B**, and the two
//!    resident surfaces are compared — item 53's "geometry is byte-identical", as a check
//!    rather than as a claim;
//! 4. a `gp.api.v1` result is encoded and decoded through the schema;
//! 5. the caught-panic count is reported.
//!
//! `godot/scripts/client_check.gd` calls it, prints the report and quits non-zero if
//! anything in it failed — which is the line `.github/workflows/ci.yml`'s client leg runs
//! after `cargo xtask stage-client`.

// `#[godot_api]` synthesises a visibility-forwarding helper that carries no `Debug`, and
// the lint reports it against `PharmakosBridge` — which does have a hand-written `Debug`
// impl, a few lines below. The allow is module-scoped because the item it fires on is
// inside a macro expansion and an attribute on the struct or the impl block does not
// reach it. `missing_debug_implementations` is a documentation lint, not one of the
// determinism bans AGENTS.md section 4.9 protects, and the wall lifts none of those here.
#![allow(
    missing_debug_implementations,
    reason = "a Debug-less helper inside gdext's #[godot_api] expansion; the public type \
              has a hand-written impl"
)]

use std::fmt;

use godot::builtin::{GString, PackedByteArray, VarDictionary, Variant, Vector3};
use godot::classes::{INode3D, Node3D};
use godot::meta::ToGodot;
use godot::obj::{Base, WithBaseField};
use godot::prelude::{GodotClass, godot_api, godot_error, godot_print};

use pharmakos_mesher::{
    AIR, CHUNK_EDGE, CHUNK_VOLUME, ChunkGrid, ChunkInput, ChunkView, DrainBudget, LightField,
    MeshBuffers, Mesher,
};
use pharmakos_proto::gp::api::v1::{GetStatusResponse, Status, status};
use pharmakos_proto::gp::v1::{RulesTable, rules_table};
use pharmakos_proto::json;

use crate::api;
use crate::chunks;
use crate::engine::ChunkRenderer;
use crate::error::BridgeError;
use crate::panics::PanicCounter;
use crate::rules::{self, MesherRules};
use crate::surface::{ColourQuantisation, Surface, SurfaceProbe};
use crate::upload::{DEFAULT_UPLOAD_PATH, ResidentSurface, UploadCounters, UploadPath, Uploader};
use crate::variant::json_to_variant;

/// The bridge node. One per client; the scenes hold it and GDScript calls it.
#[derive(GodotClass)]
#[class(base = Node3D)]
pub struct PharmakosBridge {
    base: Base<Node3D>,
    panics: PanicCounter,
    /// Built by `configure` once there is a world to render into. `None` until then,
    /// which is the headless case and the editor case alike.
    renderer: Option<ChunkRenderer>,
    /// Built by `configure` from the rules table's light rows. `None` until then.
    mesher: Option<Mesher>,
    /// Item 54's K, B and ageing term, as `configure` read them. Held so a caller can ask
    /// what this client is budgeted at without re-reading the table.
    budget: Option<DrainBudget>,
    /// Reused across the frame's chunks, so a steady-state remesh allocates nothing.
    buffers: MeshBuffers,
    /// Reused across the frame's chunks, so the per-chunk transposition allocates once.
    transposed: Vec<u8>,
}

impl fmt::Debug for PharmakosBridge {
    /// Written by hand because `Base<Node3D>` is not `Debug`, and
    /// `missing_debug_implementations` is a workspace lint. What a reader wants from it
    /// is the two counts, not the node.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PharmakosBridge")
            .field("caught_panics", &self.panics.caught())
            .field("renderer", &self.renderer.is_some())
            .field("configured", &self.mesher.is_some())
            // Non-exhaustive on purpose: `Base<Node3D>` is not `Debug`, and the node is
            // not what a reader wants out of this anyway.
            .finish_non_exhaustive()
    }
}

#[godot_api]
impl INode3D for PharmakosBridge {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            base,
            panics: PanicCounter::new(),
            renderer: None,
            mesher: None,
            budget: None,
            buffers: MeshBuffers::empty(),
            transposed: Vec::new(),
        }
    }

    /// RIDs are not reference counted, so the renderer's are freed here and nowhere else.
    fn exit_tree(&mut self) {
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.free_all();
        }
    }
}

// Two pedantic lints have to give here, and both are gdext's calling convention
// rather than anything this file chooses:
//
//   * `needless_pass_by_value` — a `#[func]`'s exported signature is built from its
//     parameter types, and Godot's own value types (`GString`, `PackedByteArray`) are
//     what cross the boundary. Taking them by reference changes the signature the
//     engine sees;
//   * `unused_self` — a `#[func]` is a method on the class whether or not its body
//     needs the receiver. `upload_path` reports a build-time constant and is still a
//     method, because GDScript calls it on the node.
//
// Neither is a determinism lint (AGENTS.md section 4.9): the wall lifts floats, casts,
// hash maps and clocks, and these two are neither lifted nor weakened here.
#[allow(clippy::needless_pass_by_value, clippy::unused_self)]
#[godot_api]
impl PharmakosBridge {
    /// How many panics this bridge has caught at its own boundary. CI asserts zero.
    #[func]
    fn caught_panics(&self) -> i64 {
        i64::try_from(self.panics.caught()).unwrap_or(i64::MAX)
    }

    /// The caught-panic count and the last message, as one line for the log.
    #[func]
    fn panic_report(&self) -> GString {
        GString::from(self.panics.describe().as_str())
    }

    /// Which upload path this build selected — item 53's build-time switch, as a string
    /// the log and the self-check report can print.
    #[func]
    fn upload_path(&self) -> GString {
        GString::from(DEFAULT_UPLOAD_PATH.name())
    }

    /// Reads the rules table, builds the mesher, and attaches the renderer to this
    /// node's world.
    ///
    /// Everything the client needs before it can upload anything, in one call, so that a
    /// half-configured bridge is not a state anybody can reach. The returned dictionary
    /// says what happened: `configured` is whether the mesher exists, `attached` is
    /// whether there is a world to render into — false in a headless run, which is a
    /// state to report rather than an error to raise — and `reason` says why not.
    #[func]
    fn configure(&mut self, chunks: i64, rules_json: GString) -> VarDictionary {
        let text = rules_json.to_string();
        let parsed = self.panics.guard("configure", || {
            rules::table_from_json(&text).and_then(|table| rules::mesher_rules(&table))
        });
        let mut report = VarDictionary::new();
        let parsed = match parsed {
            Some(Ok(parsed)) => parsed,
            Some(Err(error)) => {
                godot_error!("[pharmakos] configure: {error}");
                report.set(&"configured".to_variant(), &false.to_variant());
                report.set(&"attached".to_variant(), &false.to_variant());
                report.set(&"reason".to_variant(), &error.to_string().to_variant());
                return report;
            }
            None => {
                report.set(&"configured".to_variant(), &false.to_variant());
                report.set(&"attached".to_variant(), &false.to_variant());
                report.set(&"reason".to_variant(), &"a panic was caught".to_variant());
                return report;
            }
        };

        self.mesher = Some(Mesher::new(parsed.light));
        self.budget = Some(parsed.budget);

        let count = usize::try_from(chunks).unwrap_or(0);
        let scenario = self
            .base()
            .get_world_3d()
            .map(|world| world.get_scenario())
            .filter(godot::prelude::Rid::is_valid);
        let attached = match scenario {
            Some(scenario) => {
                let holder = match DEFAULT_UPLOAD_PATH {
                    UploadPath::ArrayMesh => Some(self.to_gd().upcast::<Node3D>()),
                    UploadPath::RenderingServerRids => None,
                };
                self.renderer = Some(ChunkRenderer::new(
                    DEFAULT_UPLOAD_PATH,
                    count,
                    scenario,
                    holder,
                ));
                true
            }
            None => false,
        };

        report.set(&"configured".to_variant(), &true.to_variant());
        report.set(&"attached".to_variant(), &attached.to_variant());
        report.set(
            &"reason".to_variant(),
            &if attached {
                String::new()
            } else {
                "this node has no 3D world; nothing to render into".to_owned()
            }
            .to_variant(),
        );
        report.set(
            &"rules".to_variant(),
            &mesher_rules_dictionary(parsed).to_variant(),
        );
        report
    }

    /// Meshes one chunk and hands the surface to the engine on whichever path this build
    /// selected.
    ///
    /// `materials` and `light` are in **mesher** order — a caller holding sim-order bytes
    /// runs them through `transpose_chunk` first. The returned dictionary reports what the
    /// surface cost and how the uploads have gone; an empty dictionary means the reason is
    /// in the log.
    #[func]
    fn upload_chunk(
        &mut self,
        chunk: i64,
        origin: Vector3,
        materials: PackedByteArray,
        light: PackedByteArray,
    ) -> VarDictionary {
        let Ok(index) = usize::try_from(chunk) else {
            godot_error!("[pharmakos] upload_chunk: {chunk} is not a chunk index");
            return VarDictionary::new();
        };
        let Some(mut mesher) = self.mesher.take() else {
            godot_error!("[pharmakos] upload_chunk: call configure() first");
            return VarDictionary::new();
        };
        let mut buffers = std::mem::replace(&mut self.buffers, MeshBuffers::empty());
        let mut renderer = self.renderer.take();

        let outcome = self.panics.guard("upload_chunk", || {
            let view = ChunkView::new(
                materials.as_slice(),
                light.as_slice(),
                ChunkView::no_borders(),
            )?;
            mesher.mesh_chunk_into(&view, &mut buffers)?;
            let surface = Surface::from_mesh(&buffers)?;
            let vertices = surface.vertex_count();
            let indices = surface.indices().len();
            if let Some(renderer) = renderer.as_mut() {
                renderer.upload(index, [origin.x, origin.y, origin.z], &surface)?;
            }
            Ok::<(usize, usize), BridgeError>((vertices, indices))
        });

        self.mesher = Some(mesher);
        self.buffers = buffers;
        self.renderer = renderer;

        match outcome {
            Some(Ok((vertices, indices))) => {
                let mut report = VarDictionary::new();
                report.set(&"vertices".to_variant(), &count_of(vertices));
                report.set(&"indices".to_variant(), &count_of(indices));
                report.set(
                    &"counters".to_variant(),
                    &self.upload_counters().to_variant(),
                );
                report
            }
            Some(Err(error)) => {
                godot_error!("[pharmakos] upload_chunk({chunk}): {error}");
                VarDictionary::new()
            }
            None => VarDictionary::new(),
        }
    }

    /// Item 54's per-frame upload budget, as `configure` read it out of the rules
    /// table: K surfaces, B bytes, and the ageing term.
    ///
    /// The three numbers the mesher's own `DrainQueue` is built from. They are reported
    /// rather than acted on here: which chunks are queued is the sim's, and which frame
    /// each one drains on is the queue's — the bridge only carries the rows across the
    /// wall, because the mesher cannot read the rules table itself (item 92).
    #[func]
    fn upload_budget(&self) -> VarDictionary {
        let mut report = VarDictionary::new();
        let Some(budget) = self.budget else {
            return report;
        };
        report.set(
            &"surfaces_per_frame".to_variant(),
            &i64::from(budget.surfaces_per_frame).to_variant(),
        );
        report.set(
            &"bytes_per_frame".to_variant(),
            &counter(budget.bytes_per_frame),
        );
        report.set(&"age_frames".to_variant(), &counter(budget.age_frames));
        report
    }

    /// The upload counters: how often each of item 53's guards fired.
    ///
    /// Counts, never rates. A caller that wants "the in-place ratio" divides two of these
    /// itself, because the division would be arithmetic the bridge has no business doing.
    #[func]
    fn upload_counters(&self) -> VarDictionary {
        let mut report = VarDictionary::new();
        let Some(renderer) = self.renderer.as_ref() else {
            return report;
        };
        let counters = renderer.uploader().counters();
        report.set(&"creates".to_variant(), &counter(counters.creates));
        report.set(&"patches".to_variant(), &counter(counters.patches));
        report.set(&"rebuilds".to_variant(), &counter(counters.rebuilds));
        report.set(
            &"rebuilds_index_changed".to_variant(),
            &counter(counters.rebuilds_index_changed),
        );
        report.set(
            &"rebuilds_vertex_count".to_variant(),
            &counter(counters.rebuilds_vertex_count),
        );
        report.set(
            &"rebuilds_layout".to_variant(),
            &counter(counters.rebuilds_layout),
        );
        report.set(&"hides".to_variant(), &counter(counters.hides));
        report.set(&"bytes".to_variant(), &counter(counters.bytes));
        report.set(
            &"resources_created".to_variant(),
            &counter(renderer.resources_created()),
        );
        report.set(
            &"surface_probe".to_variant(),
            &renderer.uploader().probe().describe().to_variant(),
        );
        report
    }

    /// Rewrites one chunk's bytes from the sim's voxel order into the mesher's.
    ///
    /// The one piece of marshalling item 92 names by name. An array of the wrong length
    /// comes back empty and logs why, rather than being padded into something the mesher
    /// would accept.
    #[func]
    fn transpose_chunk(&mut self, sim_order: PackedByteArray) -> PackedByteArray {
        let mut out = std::mem::take(&mut self.transposed);
        let result = self.panics.guard("transpose_chunk", || {
            chunks::sim_to_mesher_chunk("chunk materials", sim_order.as_slice(), &mut out)
        });
        let bytes = match result {
            Some(Ok(())) => PackedByteArray::from(out.as_slice()),
            Some(Err(error)) => {
                godot_error!("[pharmakos] transpose_chunk: {error}");
                PackedByteArray::new()
            }
            None => PackedByteArray::new(),
        };
        self.transposed = out;
        bytes
    }

    /// The mesher's five rules-table parameters, read out of a canonical `gp.v1`
    /// `RulesTable` JSON text.
    ///
    /// The bridge holds no constant of its own: a table without a `mesher` row comes back
    /// as an empty dictionary and a logged reason (AGENTS.md section 12).
    #[func]
    fn mesher_rules(&mut self, rules_json: GString) -> VarDictionary {
        let text = rules_json.to_string();
        let result = self.panics.guard("mesher_rules", || {
            rules::table_from_json(&text).and_then(|table| rules::mesher_rules(&table))
        });
        match result {
            Some(Ok(parsed)) => mesher_rules_dictionary(parsed),
            Some(Err(error)) => {
                godot_error!("[pharmakos] mesher_rules: {error}");
                VarDictionary::new()
            }
            None => VarDictionary::new(),
        }
    }

    /// One gateway result, decoded against the schema and handed on as Godot types.
    ///
    /// `method` is the gateway's own wire spelling — `"get_status"`, `"verify_plan"`.
    /// Anything the schema rejects comes back as `null` with the reason and its JSON
    /// Pointer in the log: an unknown field is refused here, never stripped.
    #[func]
    fn decode_result(&mut self, method: GString, result_json: GString) -> Variant {
        let wire = method.to_string();
        let text = result_json.to_string();
        let decoded = self
            .panics
            .guard("decode_result", || api::decode_result(&wire, &text));
        match decoded {
            Some(Ok(value)) => json_to_variant(&value),
            Some(Err(error)) => {
                godot_error!("[pharmakos] decode_result({wire}): {error}");
                Variant::nil()
            }
            None => Variant::nil(),
        }
    }

    /// Runs T12's acceptance and returns it as a dictionary.
    ///
    /// The one call `godot/scripts/client_check.gd` makes, and through it the one the CI
    /// client leg asserts on.
    #[func]
    fn run_self_check(&mut self) -> VarDictionary {
        let report = self.panics.guard("run_self_check", self_check);
        let passed = report.as_ref().is_some_and(SelfCheck::ok);
        let mut dictionary = match report.as_ref() {
            Some(report) => {
                for line in report.lines() {
                    godot_print!("[pharmakos] {line}");
                }
                report.to_dictionary()
            }
            None => VarDictionary::new(),
        };
        // Reported after the run, so a panic inside the check is part of the verdict
        // rather than something the verdict was computed before.
        dictionary.set(
            &"caught_panics".to_variant(),
            &self.caught_panics().to_variant(),
        );
        dictionary.set(
            &"ok".to_variant(),
            &(passed && self.panics.caught() == 0).to_variant(),
        );
        dictionary
    }
}

/// One counter as the `Variant` a dictionary holds.
fn counter(value: u64) -> Variant {
    i64::try_from(value).unwrap_or(i64::MAX).to_variant()
}

/// One size as the `Variant` a dictionary holds.
fn count_of(value: usize) -> Variant {
    i64::try_from(value).unwrap_or(i64::MAX).to_variant()
}

/// The mesher's five parameters, as a dictionary GDScript can read.
fn mesher_rules_dictionary(parsed: MesherRules) -> VarDictionary {
    let mut dictionary = VarDictionary::new();
    dictionary.set(
        &"revision".to_variant(),
        &i64::from(parsed.revision).to_variant(),
    );
    dictionary.set(
        &"surfaces_per_frame".to_variant(),
        &i64::from(parsed.budget.surfaces_per_frame).to_variant(),
    );
    dictionary.set(
        &"bytes_per_frame".to_variant(),
        &i64::try_from(parsed.budget.bytes_per_frame)
            .unwrap_or(i64::MAX)
            .to_variant(),
    );
    dictionary.set(
        &"age_frames".to_variant(),
        &i64::try_from(parsed.budget.age_frames)
            .unwrap_or(i64::MAX)
            .to_variant(),
    );
    dictionary.set(
        &"light_max".to_variant(),
        &i64::from(parsed.light.light_max()).to_variant(),
    );
    dictionary.set(
        &"light_atten".to_variant(),
        &i64::from(parsed.light.light_atten()).to_variant(),
    );
    dictionary
}

/// What [`self_check`] found.
///
/// Plain data with no Godot types in it, so the same check runs under `cargo test` and
/// under a headless Godot, and the two cannot drift.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SelfCheck {
    /// The rules-table revision the five parameters came from.
    pub rules_revision: u32,
    /// Chunks meshed and uploaded through both paths.
    pub chunks: u32,
    /// Vertices across the whole chunk set.
    pub vertices: u64,
    /// Indices across the whole chunk set.
    pub indices: u64,
    /// Whether every chunk's resident surface came out identical on the two paths.
    pub geometry_identical: bool,
    /// Path A's counters.
    pub counters_a: UploadCounters,
    /// Path B's counters.
    pub counters_b: UploadCounters,
    /// Whether the `gp.api.v1` result round-tripped through the schema.
    pub api_round_trip: bool,
    /// Anything that went wrong, in the order it went wrong.
    pub failures: Vec<String>,
}

impl SelfCheck {
    /// Whether everything the check looked at held.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.failures.is_empty() && self.geometry_identical && self.api_round_trip
    }

    /// The report, one fact per line.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!("upload path: {}", DEFAULT_UPLOAD_PATH.name()),
            format!("rules revision: {}", self.rules_revision),
            format!(
                "chunks: {} ({} vertices, {} indices)",
                self.chunks, self.vertices, self.indices
            ),
            format!(
                "geometry identical across paths A and B: {}",
                self.geometry_identical
            ),
            format!(
                "path A: {} creates, {} rebuilds, {} patches",
                self.counters_a.creates, self.counters_a.rebuilds, self.counters_a.patches
            ),
            format!(
                "path B: {} creates, {} rebuilds ({} index-array, {} vertex-count), {} patches",
                self.counters_b.creates,
                self.counters_b.rebuilds,
                self.counters_b.rebuilds_index_changed,
                self.counters_b.rebuilds_vertex_count,
                self.counters_b.patches
            ),
            format!("gp.api.v1 round trip: {}", self.api_round_trip),
        ];
        for failure in &self.failures {
            lines.push(format!("FAILED: {failure}"));
        }
        lines
    }

    /// The report as a dictionary GDScript can read.
    fn to_dictionary(&self) -> VarDictionary {
        let mut dictionary = VarDictionary::new();
        dictionary.set(
            &"upload_path".to_variant(),
            &DEFAULT_UPLOAD_PATH.name().to_variant(),
        );
        dictionary.set(
            &"rules_revision".to_variant(),
            &i64::from(self.rules_revision).to_variant(),
        );
        dictionary.set(&"chunks".to_variant(), &i64::from(self.chunks).to_variant());
        dictionary.set(
            &"vertices".to_variant(),
            &i64::try_from(self.vertices)
                .unwrap_or(i64::MAX)
                .to_variant(),
        );
        dictionary.set(
            &"geometry_identical".to_variant(),
            &self.geometry_identical.to_variant(),
        );
        dictionary.set(
            &"api_round_trip".to_variant(),
            &self.api_round_trip.to_variant(),
        );
        dictionary.set(
            &"failures".to_variant(),
            &self.failures.join("; ").to_variant(),
        );
        dictionary
    }
}

/// T12's acceptance, with no engine, no GPU and no gateway in it.
///
/// See the module header for what it checks and why each step is there.
#[must_use]
pub fn self_check() -> SelfCheck {
    let mut failures: Vec<String> = Vec::new();

    let parsed = match round_trip_rules() {
        Ok(parsed) => parsed,
        Err(error) => {
            failures.push(format!("the rules table did not round-trip: {error}"));
            return SelfCheck {
                rules_revision: 0,
                chunks: 0,
                vertices: 0,
                indices: 0,
                geometry_identical: false,
                counters_a: UploadCounters::default(),
                counters_b: UploadCounters::default(),
                api_round_trip: false,
                failures,
            };
        }
    };

    let mut mesher = Mesher::new(parsed.light);
    let mut buffers = MeshBuffers::empty();
    let mut path_a = Uploader::new(UploadPath::ArrayMesh, CASES);
    let mut path_b = Uploader::new(UploadPath::RenderingServerRids, CASES);
    // The two paths are compared under a probe that says "patchable", because a probe
    // that said otherwise would turn every path B upload into a rebuild and the
    // comparison would no longer exercise the branch it exists to check.
    path_b.record_probe(patchable_probe());

    let mut resident_a = vec![ResidentSurface::default(); CASES];
    let mut resident_b = vec![ResidentSurface::default(); CASES];
    let mut vertices = 0_u64;
    let mut indices = 0_u64;
    let mut geometry_identical = true;

    // Three passes over the same chunks, and each one is there for a branch:
    //
    //   0  nothing is resident, so both paths CREATE;
    //   1  the chunk has been edited, so the vertex count moves and both paths REBUILD;
    //   2  the chunk is unchanged, so path B PATCHES in place while path A rebuilds.
    //
    // Pass 2 is the one that makes the comparison worth making. A patch writes the
    // vertex and attribute bytes itself; a rebuild hands the engine floats and lets it
    // quantise. If those two disagreed by a least-significant bit, a chunk's shade would
    // depend on which branch its last upload took — and it is here, comparing the two
    // residents, that the disagreement shows.
    for pass in 0..3_u32 {
        for case in 0..CASES {
            // Pass 2 re-meshes pass 1's geometry, unchanged. That is what leaves the
            // index array equal and so lets path B take its in-place branch.
            let shape = pass.min(1);
            let surface = match mesh_case(&mut mesher, &mut buffers, case, shape) {
                Ok(surface) => surface,
                Err(error) => {
                    failures.push(format!("chunk {case} on pass {pass}: {error}"));
                    geometry_identical = false;
                    continue;
                }
            };
            if pass == 2 {
                vertices = vertices.saturating_add(surface.vertex_count() as u64);
                indices = indices.saturating_add(surface.indices().len() as u64);
            }

            if let Err(error) = apply(&mut path_a, resident_a.get_mut(case), case, &surface) {
                failures.push(format!("path A, chunk {case}: {error}"));
                geometry_identical = false;
            }
            if let Err(error) = apply(&mut path_b, resident_b.get_mut(case), case, &surface) {
                failures.push(format!("path B, chunk {case}: {error}"));
                geometry_identical = false;
            }
        }
    }

    for case in 0..CASES {
        if resident_a.get(case) != resident_b.get(case) {
            geometry_identical = false;
            failures.push(format!(
                "chunk {case} is not identical across paths A and B"
            ));
        }
    }

    let api_round_trip = match round_trip_status() {
        Ok(()) => true,
        Err(error) => {
            failures.push(format!("the gp.api.v1 round trip failed: {error}"));
            false
        }
    };

    SelfCheck {
        rules_revision: parsed.revision,
        chunks: u32::try_from(CASES).unwrap_or(u32::MAX),
        vertices,
        indices,
        geometry_identical,
        counters_a: path_a.counters(),
        counters_b: path_b.counters(),
        api_round_trip,
        failures,
    }
}

/// How many chunks the self-check meshes. Small on purpose: it runs inside a CI job that
/// is about whether the extension loads, not about throughput.
const CASES: usize = 4;

/// A probe that says the layout is the one an in-place write assumes.
fn patchable_probe() -> SurfaceProbe {
    SurfaceProbe {
        vertex_stride: Some(crate::surface::VERTEX_STRIDE),
        attribute_stride: Some(crate::surface::ATTRIBUTE_STRIDE),
        quantisation: Some(ColourQuantisation::RoundHalfUp),
    }
}

/// A rules table, encoded to canonical JSON and read back through the schema.
fn round_trip_rules() -> Result<MesherRules, BridgeError> {
    let table = RulesTable {
        revision: 1,
        mesher: Some(rules_table::Mesher {
            surfaces_per_frame: 4,
            bytes_per_frame: 524_288,
            age_frames: 2,
            light_max: 15,
            light_atten: 1,
        }),
        ..RulesTable::default()
    };
    let text = json::encode(&table)?;
    let read_back = rules::table_from_json(&text)?;
    rules::mesher_rules(&read_back)
}

/// A `gp.api.v1` result, encoded and decoded the way a gateway answer would be.
fn round_trip_status() -> Result<(), BridgeError> {
    let response = GetStatusResponse {
        status: Some(Status {
            phase: status::Phase::Lull.into(),
            phase_remaining_ms: 90_000,
            segment_length_ms: 180_000,
            round: 1,
        }),
    };
    let text = json::encode(&response)?;
    let decoded = api::decode_result("get_status", &text)?;
    let again = json::write(&decoded);
    if again == text {
        Ok(())
    } else {
        Err(BridgeError::Schema {
            pointer: String::new(),
            message: format!("canonical form is not stable: {text} became {again}"),
        })
    }
}

/// Builds one case's chunk in **sim** order, transposes it, lights it and meshes it.
///
/// `pass` changes the chunk, so the second pass is a genuine remesh rather than the same
/// bytes twice.
fn mesh_case(
    mesher: &mut Mesher,
    buffers: &mut MeshBuffers,
    case: usize,
    pass: u32,
) -> Result<Surface, BridgeError> {
    let mut sim_order = vec![AIR; CHUNK_VOLUME];
    for z in 0..CHUNK_EDGE {
        for y in 0..CHUNK_EDGE {
            for x in 0..CHUNK_EDGE {
                // A stepped floor whose height depends on the case, with a bite taken out
                // of it on the second pass — enough to move quads between face directions
                // and so to change the index array.
                // `checked_div` rather than `/`: `integer_division` stays denied inside
                // a walled crate too (AGENTS.md section 4.9), because being explicit
                // about rounding is not a determinism concern that the wall lifts.
                let step = x.checked_div(8).unwrap_or(0);
                let height = 8 + (case + step) % 6;
                let bitten = pass == 1
                    && (12..20).contains(&x)
                    && (12..20).contains(&y)
                    && z.saturating_add(2) >= height;
                let solid = z < height && !bitten;
                if solid {
                    if let Some(slot) = sim_order.get_mut(chunks::sim_voxel_index(x, y, z)) {
                        *slot = u8::try_from(1 + (case % 3)).unwrap_or(1);
                    }
                }
            }
        }
    }

    let mut mesher_order = Vec::new();
    chunks::sim_to_mesher_chunk("chunk materials", &sim_order, &mut mesher_order)?;

    // One chunk on its own: the light bake needs a grid, and a one-chunk grid is the
    // honest shape for a chunk with no neighbours.
    let grid = ChunkGrid::new(1, 1, 1).map_err(|error| BridgeError::Schema {
        pointer: String::new(),
        message: format!("a one-chunk grid was refused: {error:?}"),
    })?;
    let mut light = LightField::new(grid, mesher.params());
    light
        .bake_all(&mesher_order)
        .map_err(|error| BridgeError::Schema {
            pointer: String::new(),
            message: format!("the light bake was refused: {error}"),
        })?;

    let input = ChunkInput::new(
        mesher_order,
        light.chunk_light(0).to_vec(),
        ChunkInput::no_borders(),
    )?;
    mesher.mesh_chunk_into(&input.as_view(), buffers)?;
    Surface::from_mesh(buffers)
}

/// Plans one upload and applies it to the matching resident model.
fn apply(
    uploader: &mut Uploader,
    resident: Option<&mut ResidentSurface>,
    case: usize,
    surface: &Surface,
) -> Result<(), BridgeError> {
    let op = uploader.plan(case, surface)?;
    match resident {
        Some(resident) => resident.apply(&op, ColourQuantisation::RoundHalfUp),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same function the headless Godot run calls, run under `cargo test`. If this
    /// passes and the CI client leg does not, the difference is the engine — which is the
    /// only thing the client leg is there to tell us.
    #[test]
    fn the_self_check_passes_with_no_engine_in_the_loop() {
        let report = self_check();
        assert!(report.ok(), "{:#?}", report.lines());
        assert!(report.geometry_identical);
        assert!(report.api_round_trip);
        assert!(report.vertices > 0, "the cases must have geometry");
    }

    /// The comparison is only worth anything if path B actually took each of its
    /// branches. A run in which B never patched would be comparing two rebuild paths and
    /// would prove nothing about the one write that goes to the engine buffers directly,
    /// so the counters are asserted rather than printed.
    #[test]
    fn every_branch_of_path_b_is_exercised_and_still_matches_path_a() {
        let report = self_check();
        let cases = u64::try_from(CASES).expect("four");
        assert_eq!(report.counters_a.creates, cases);
        assert_eq!(report.counters_b.creates, cases);
        assert_eq!(
            report.counters_a.patches, 0,
            "path A has no in-place branch"
        );
        assert_eq!(
            report.counters_b.rebuilds_vertex_count,
            cases,
            "pass 1 must move the vertex count: {:#?}",
            report.lines()
        );
        assert_eq!(
            report.counters_b.patches,
            cases,
            "pass 2 must patch in place; without it the two paths are trivially equal: {:#?}",
            report.lines()
        );
        assert!(report.geometry_identical, "{:#?}", report.lines());
    }
}
