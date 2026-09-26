// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `PharmakosBridge`, the one class GDScript talks to, and the self-check CI runs.
//!
//! Every `#[func]` below does the same three things and nothing else: take Godot's types
//! apart, call a module that knows how to marshal, put Godot's types back together. Every
//! one that reaches a marshalling module or the renderer goes through
//! [`PanicCounter::guard`], because gdext's own catch at this boundary is silent and a
//! default return value that nobody flagged is the failure mode the count exists to make
//! visible (G1 section 10.12).
//!
//! Three do not, and the list is short on purpose: [`PharmakosBridge::caught_panics`],
//! [`PharmakosBridge::panic_report`] and [`PharmakosBridge::upload_path`]. Each reads a
//! value this struct already holds — a `u64`, a `String` the counter owns, a build-time
//! constant — and calls nothing. Guarding them would be guarding the instrument with
//! itself. Everything that does real work, the two counter dictionaries included, is
//! guarded: `upload_counters` formats the surface probe's description and
//! `upload_budget` reads rows `configure` parsed, and a panic in either used to come back
//! as an empty dictionary that looked like "not configured yet".
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

use godot::builtin::{
    GString, PackedByteArray, PackedInt32Array, PackedStringArray, VarArray, VarDictionary,
    Variant, Vector3, Vector3i,
};
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
use crate::editor::Action;
use crate::editor_view::{
    instance_dictionary, meter_dictionary, rows_array, rows_of_report_text, state_dictionary,
    target_of,
};
use crate::engine::ChunkRenderer;
use crate::error::BridgeError;
use crate::pacer::WallClock;
use crate::panics::PanicCounter;
use crate::rig::{Answer, Rig};
use crate::rules::{self, MesherRules};
use crate::surface::{ColourQuantisation, Surface, SurfaceProbe};
use crate::upload::{DEFAULT_UPLOAD_PATH, ResidentSurface, UploadCounters, UploadPath, Uploader};
use crate::variant::json_to_variant;
use crate::view::{self, Applied, Entity, ViewModel};

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
    /// The Lull's length, `match.lull_ms`, as `configure` read it; `None` is untimed.
    lull_ms: Option<u32>,
    /// Reused across the frame's chunks, so a steady-state remesh allocates nothing.
    buffers: MeshBuffers,
    /// Reused across the frame's chunks, so the per-chunk transposition allocates once.
    transposed: Vec<u8>,
    /// The client's copy of the view: built by `configure`, filled by `view_apply`.
    view: Option<ViewModel>,
    /// The watch rig: built by `watch_begin`.
    rig: Option<Rig>,
    /// The one wall clock the client reads (`crate::pacer`).
    wall: WallClock,
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
            lull_ms: None,
            buffers: MeshBuffers::empty(),
            transposed: Vec::new(),
            view: None,
            rig: None,
            wall: WallClock::start(),
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
            rules::table_from_json(&text).and_then(|table| {
                Ok((
                    rules::mesher_rules(&table)?,
                    rules::lull_ms(&table),
                    rules::map_extent(&table)?,
                ))
            })
        });
        let mut report = VarDictionary::new();
        let (parsed, lull_ms, extent) = match parsed {
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
        self.view = Some(ViewModel::new(parsed.light, parsed.budget, extent));
        self.lull_ms = lull_ms;

        let attached = self.attach_renderer(usize::try_from(chunks).unwrap_or(0));

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
    ///
    /// # This call meshes one chunk as if surrounded by air
    ///
    /// It passes [`ChunkView::no_borders`], so the full set of chunk-boundary faces is
    /// emitted: right for the single-chunk self-check and the engine-side upload check,
    /// wrong for a world. The world's chunks go through `view_drain` instead, which meshes
    /// each chunk of the resident map with its six real neighbours and its baked light
    /// (`crate::view`, T16).
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
    /// The three numbers the mesher's own `DrainQueue` is built from.
    ///
    /// **They are applied by `view_drain`** (T16): `configure` builds the view model's
    /// drain queue from exactly these three numbers, and each call of `view_drain` uploads
    /// what item 54's order and budget allow that frame. `upload_chunk` stays a per-chunk
    /// call with no per-frame accounting, because its callers are checks, not a world.
    ///
    /// Carrying them is the bridge's job either way, because the mesher cannot read the
    /// rules table itself (item 92).
    #[func]
    fn upload_budget(&mut self) -> VarDictionary {
        let Some(budget) = self.budget else {
            return VarDictionary::new();
        };
        self.panics
            .guard("upload_budget", || {
                let mut report = VarDictionary::new();
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
            })
            .unwrap_or_default()
    }

    /// The upload counters: how often each of item 53's guards fired.
    ///
    /// Counts, never rates. A caller that wants "the in-place ratio" divides two of these
    /// itself, because the division would be arithmetic the bridge has no business doing.
    #[func]
    fn upload_counters(&mut self) -> VarDictionary {
        let Some(renderer) = self.renderer.as_ref() else {
            return VarDictionary::new();
        };
        self.panics
            .guard("upload_counters", || counters_of(renderer))
            .unwrap_or_default()
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

    /// **The view feed's one entry**: a `get_view` result text in, its chunks on the
    /// drain queue out.
    ///
    /// `result_text` is the object a JSON-RPC answer's `result` member holds, `_status`
    /// footer and all; one line of `godot/fixtures/view_keyframe.jsonl` is exactly one.
    /// The chunks are decoded, stored by origin, lit and queued ([`crate::view`]); nothing
    /// is drawn until `view_drain`. The watch rig hands every `get_view` answer it reads to
    /// the same function this calls, so the hostless vista and the live one decode through
    /// one path.
    ///
    /// The dictionary says `complete`, `next_cursor`, `at_ms`, `chunks`, `queued`,
    /// `pending` and `held`, and on a completing page `entities`: an array of dictionaries
    /// with `id`, `kind`, `subtype`, `owner` and `at` (a `Vector3i` in the SIM's axes: x
    /// east, y north, z up). An empty dictionary means the reason is in the log.
    #[func]
    fn view_apply(&mut self, result_text: GString) -> VarDictionary {
        let text = result_text.to_string();
        let parsed = self.panics.guard("view_apply", || {
            json::read(&text).map_err(BridgeError::from)
        });
        match parsed {
            Some(Ok(result)) => self.apply_view(&result).unwrap_or_default(),
            Some(Err(error)) => {
                godot_error!("[pharmakos] view_apply: {error}");
                VarDictionary::new()
            }
            None => VarDictionary::new(),
        }
    }

    /// One frame's uploads: item 54's K surfaces and B bytes, nearest the camera first.
    ///
    /// `camera` is the camera's position in the world, which is the MESHER's axes (y up).
    /// Returns `uploaded` and `pending`; an empty dictionary means the reason is in the
    /// log.
    #[func]
    fn view_drain(&mut self, camera: Vector3) -> VarDictionary {
        let (Some(mut model), Some(mut mesher)) = (self.view.take(), self.mesher.take()) else {
            return VarDictionary::new();
        };
        let mut buffers = std::mem::replace(&mut self.buffers, MeshBuffers::empty());
        let mut renderer = self.renderer.take();
        let edge = f32::from(u16::try_from(pharmakos_mesher::CHUNK_EDGE).unwrap_or(32));
        let camera_chunk = [
            (camera.x / edge).floor() as i32,
            (camera.y / edge).floor() as i32,
            (camera.z / edge).floor() as i32,
        ];
        let outcome = self.panics.guard("view_drain", || {
            let mut uploaded = 0_usize;
            for chunk in model.next_uploads(camera_chunk) {
                let corner = model.mesh(chunk, &mut mesher, &mut buffers)?;
                let surface = Surface::from_mesh(&buffers)?;
                if let Some(renderer) = renderer.as_mut() {
                    let index = usize::try_from(chunk).unwrap_or(usize::MAX);
                    renderer.upload(index, corner.map(|value| value as f32), &surface)?;
                }
                uploaded = uploaded.saturating_add(1);
            }
            Ok::<usize, BridgeError>(uploaded)
        });
        let pending = model.pending();
        self.view = Some(model);
        self.mesher = Some(mesher);
        self.buffers = buffers;
        self.renderer = renderer;
        let mut report = VarDictionary::new();
        match outcome {
            Some(Ok(uploaded)) => {
                report.set(&"uploaded".to_variant(), &count_of(uploaded));
                report.set(&"pending".to_variant(), &count_of(pending));
            }
            Some(Err(error)) => godot_error!("[pharmakos] view_drain: {error}"),
            None => {}
        }
        report
    }

    /// The map's extent in voxels, in the MESHER's axes (y up); zero before a keyframe.
    #[func]
    fn view_extent(&mut self) -> Vector3i {
        let grid = self.view.as_ref().and_then(ViewModel::grid);
        self.panics
            .guard("view_extent", || {
                grid.map_or(Vector3i::ZERO, |grid| {
                    let [x, y, z] = grid.voxels();
                    Vector3i::new(x, y, z)
                })
            })
            .unwrap_or(Vector3i::ZERO)
    }

    /// Starts a fresh watch rig, with both connections closed, timing its Lulls by the
    /// `match.lull_ms` row `configure` read (untimed when there was none).
    #[func]
    fn watch_begin(&mut self) {
        let mut rig = Rig::new();
        rig.set_lull_length(self.lull_ms.map_or(0, u64::from));
        self.rig = Some(rig);
    }

    /// Connection `link` (0 admin, 1 seat) is open.
    #[func]
    fn watch_opened(&mut self, link: i64) {
        if let (Some(rig), Ok(link)) = (self.rig.as_mut(), usize::try_from(link)) {
            let _ = self.panics.guard("watch_opened", || rig.opened(link));
        }
    }

    /// Connection `link` (0 admin, 1 seat) dropped.
    #[func]
    fn watch_dropped(&mut self, link: i64) {
        if let (Some(rig), Ok(link)) = (self.rig.as_mut(), usize::try_from(link)) {
            let _ = self.panics.guard("watch_dropped", || rig.dropped(link));
        }
    }

    /// Reads one text frame from connection `link`.
    ///
    /// Returns `phase`, `phase_changed`, `events` (new event-list rows, as strings),
    /// `error` (the gateway's refusal, empty when there was none) and, for a `get_view`
    /// answer, `view`: what `view_apply` returns for the same result. An empty dictionary
    /// means the frame could not be read, and the reason is in the log.
    #[func]
    fn watch_receive(&mut self, link: i64, text: GString) -> VarDictionary {
        let (Some(mut rig), Ok(link)) = (self.rig.take(), usize::try_from(link)) else {
            return VarDictionary::new();
        };
        let text = text.to_string();
        let read = self
            .panics
            .guard("watch_receive", || rig.receive(link, &text));
        let phase = rig.phase().name();
        self.rig = Some(rig);
        let answer = match read {
            Some(Ok(answer)) => answer,
            Some(Err(error)) => {
                godot_error!("[pharmakos] watch_receive: {error}");
                return VarDictionary::new();
            }
            None => return VarDictionary::new(),
        };
        let Answer {
            view,
            events,
            error,
            phase_changed,
        } = answer;
        let mut report = VarDictionary::new();
        report.set(&"phase".to_variant(), &phase.to_variant());
        report.set(&"phase_changed".to_variant(), &phase_changed.to_variant());
        let mut rows = PackedStringArray::new();
        for row in &events {
            rows.push(row.as_str());
        }
        report.set(&"events".to_variant(), &rows.to_variant());
        report.set(
            &"error".to_variant(),
            &error.unwrap_or_default().to_variant(),
        );
        if let Some(result) = view {
            // The rig has already moved its cursor past this page; a page the bridge
            // refuses is lost unless the rig is told, so it asks for a keyframe again.
            let applied = self.apply_view(&result);
            if applied.is_none() {
                if let Some(rig) = self.rig.as_mut() {
                    let _ = self.panics.guard("watch_receive", || rig.view_refused());
                }
            }
            report.set(
                &"view".to_variant(),
                &applied.unwrap_or_default().to_variant(),
            );
        }
        report
    }

    /// The speeds the watch rig offers, `pacer::SPEEDS`, in order: the lobby builds its
    /// speed buttons from this, so the set has one home.
    #[func]
    fn watch_speeds(&self) -> PackedInt32Array {
        let mut speeds = PackedInt32Array::new();
        for speed in crate::pacer::SPEEDS {
            speeds.push(i32::try_from(speed).unwrap_or(i32::MAX));
        }
        speeds
    }

    /// The frames to send now, as an array of `[link, text]` pairs.
    #[func]
    fn watch_outbox(&mut self) -> VarArray {
        let Some(rig) = self.rig.as_mut() else {
            return VarArray::new();
        };
        let now = self.wall.now_us();
        let frames = self
            .panics
            .guard("watch_outbox", || rig.poll(now))
            .unwrap_or_default();
        let mut out = VarArray::new();
        for frame in frames {
            let mut pair = VarArray::new();
            pair.push(&count_of(frame.link));
            pair.push(&frame.text.to_variant());
            out.push(&pair.to_variant());
        }
        out
    }

    /// A human command to the rig: `speed` (with the speed as `value`), `skip`, `ready`,
    /// `end_lull` or `end_recap`. Returns whether it was taken.
    #[func]
    fn watch_command(&mut self, command: GString, value: i64) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        let command = command.to_string();
        self.panics
            .guard("watch_command", || match command.as_str() {
                "speed" => u32::try_from(value).is_ok_and(|speed| rig.set_speed(speed)),
                "skip" => {
                    rig.skip();
                    true
                }
                "ready" => {
                    rig.ready();
                    true
                }
                "end_lull" => {
                    rig.end_lull();
                    true
                }
                "end_recap" => {
                    rig.end_recap();
                    true
                }
                _ => false,
            })
            .unwrap_or(false)
    }

    /// What the lobby shows: `phase`, `round`, `timer`, `speed`, `skipping`, `all_ready`,
    /// `drops_admin`, `drops_seat`, `calls_admin`, `calls_seat`, `view_settled`,
    /// `seat_settled`, `view_refusals`, `pending` and `last_error`.
    #[func]
    fn watch_state(&mut self) -> VarDictionary {
        let Some(rig) = self.rig.as_ref() else {
            return VarDictionary::new();
        };
        let pending = self.view.as_ref().map_or(0, ViewModel::pending);
        self.panics
            .guard("watch_state", || rig_state(rig, pending))
            .unwrap_or_default()
    }

    // --- The editor (T19, pull requests 1 and 2) ---------------------------------------
    //
    // The editor lives in the watch rig, because it shares the seat connection and its rate
    // budget with the vista's polls; so every call below needs `watch_begin` first, and
    // answers `false` or an empty value before it.

    /// The seat this client plays, spelt as the gateway spells a seat (`seat.0`): the
    /// editor starts every route from this seat's commander.
    #[func]
    fn editor_seat(&mut self, seat: GString) {
        let seat = seat.to_string();
        if let Some(rig) = self.rig.as_mut() {
            let _ = self
                .panics
                .guard("editor_seat", || rig.editor_mut().set_seat(&seat));
        }
    }

    /// Load a playbook file's bytes. The file is QUICK-checked first and refused, with the
    /// verifier's code and pointer, when it carries anything the v1 vocabulary does not
    /// (spec section 13). Returns `false` for bytes that are not text at all.
    #[func]
    fn editor_load(&mut self, bytes: PackedByteArray) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        self.panics
            .guard("editor_load", || rig.editor_mut().load(bytes.as_slice()))
            .unwrap_or(false)
    }

    /// The playbook the editor holds, as the bytes Save writes; empty before a Load.
    #[func]
    fn editor_bytes(&mut self) -> PackedByteArray {
        let Some(rig) = self.rig.as_ref() else {
            return PackedByteArray::new();
        };
        self.panics
            .guard("editor_bytes", || {
                PackedByteArray::from(rig.editor().bytes().as_slice())
            })
            .unwrap_or_default()
    }

    /// The bytes the last accepted `submit_plan` sealed; empty before one.
    #[func]
    fn editor_sealed_bytes(&mut self) -> PackedByteArray {
        let Some(rig) = self.rig.as_ref() else {
            return PackedByteArray::new();
        };
        self.panics
            .guard("editor_sealed_bytes", || {
                PackedByteArray::from(rig.editor().sealed_bytes().as_slice())
            })
            .unwrap_or_default()
    }

    /// A map action: `go`, `visit_low`, `visit_normal`, `visit_high`, `recycle` or `place`,
    /// at a target dictionary naming a `beacon`, a `selector` or a `voxel` (sim axes).
    /// Returns whether the action was taken.
    #[func]
    fn editor_action(&mut self, action: GString, target: VarDictionary) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        let action = action.to_string();
        self.panics
            .guard("editor_action", || {
                match (Action::from_name(&action), target_of(&target)) {
                    (Some(action), Some(target)) => rig.editor_mut().act(action, &target),
                    _ => false,
                }
            })
            .unwrap_or(false)
    }

    /// The placement ghost at `at` (sim axes): the QUICK verdict of the draft with a beacon
    /// placed there, asked for once per click.
    #[func]
    fn editor_preview_place(&mut self, at: Vector3i) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        self.panics
            .guard("editor_preview_place", || {
                rig.editor_mut().preview_place([at.x, at.y, at.z])
            })
            .unwrap_or(false)
    }

    /// Apply the `fix`th Fix button of validation row `row`: the verifier's own patch,
    /// through `patch_plan`.
    #[func]
    fn editor_fix(&mut self, row: i64, fix: i64) -> bool {
        let (Some(rig), Ok(row), Ok(fix)) = (
            self.rig.as_mut(),
            usize::try_from(row),
            usize::try_from(fix),
        ) else {
            return false;
        };
        self.panics
            .guard("editor_fix", || rig.editor_mut().fix(row, fix))
            .unwrap_or(false)
    }

    /// Undo the last edit with the inverse patch the gateway handed back for it.
    #[func]
    fn editor_undo(&mut self) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        self.panics
            .guard("editor_undo", || rig.editor_mut().undo())
            .unwrap_or(false)
    }

    /// Submit the playbook on screen.
    #[func]
    fn editor_submit(&mut self) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        self.panics
            .guard("editor_submit", || rig.editor_mut().submit())
            .unwrap_or(false)
    }

    /// Save the notes box to the seat notebook.
    #[func]
    fn editor_save_notes(&mut self, notes: GString) {
        let notes = notes.to_string();
        if let Some(rig) = self.rig.as_mut() {
            let _ = self
                .panics
                .guard("editor_save_notes", || rig.editor_mut().save_notes(&notes));
        }
    }

    /// Keep the playbook on screen as this seat's editor draft.
    #[func]
    fn editor_save_draft(&mut self, label: GString) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        let label = label.to_string();
        self.panics
            .guard("editor_save_draft", || rig.editor_mut().save_draft(&label))
            .unwrap_or(false)
    }

    /// Ask the gateway about the beacon `id` (`b_NN`), for the map menu's heading.
    #[func]
    fn editor_describe_beacon(&mut self, id: GString) {
        let id = id.to_string();
        if let Some(rig) = self.rig.as_mut() {
            let _ = self.panics.guard("editor_describe_beacon", || {
                rig.editor_mut().describe_beacon(&id);
            });
        }
    }

    /// Everything the editor's panel draws: the rows, the route, the ghost, the notes, the
    /// drafts, the last submission and the status line, with a `changes` counter that moves
    /// whenever any of it did. Empty before `watch_begin`.
    #[func]
    fn editor_state(&mut self) -> VarDictionary {
        let Some(rig) = self.rig.as_ref() else {
            return VarDictionary::new();
        };
        self.panics
            .guard("editor_state", || state_dictionary(rig.editor()))
            .unwrap_or_default()
    }

    // --- The wizard, the rule list and the meter (T19, pull request 2) ----------------------

    /// Ask for the template list again.
    #[func]
    fn editor_list_templates(&mut self) {
        if let Some(rig) = self.rig.as_mut() {
            let _ = self.panics.guard("editor_list_templates", || {
                rig.editor_mut().list_templates();
            });
        }
    }

    /// Open the wizard on the template `template_id`, as `list_templates` named it.
    #[func]
    fn editor_wizard_open(&mut self, template_id: GString) {
        let template_id = template_id.to_string();
        if let Some(rig) = self.rig.as_mut() {
            let _ = self.panics.guard("editor_wizard_open", || {
                rig.editor_mut().wizard_open(&template_id);
            });
        }
    }

    /// The player typed `text` on the wizard page whose pointer is `pointer`: it goes to
    /// the gateway exactly as typed. Returns whether that page is on screen.
    #[func]
    fn editor_wizard_set(&mut self, pointer: GString, text: GString) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        let (pointer, text) = (pointer.to_string(), text.to_string());
        self.panics
            .guard("editor_wizard_set", || {
                rig.editor_mut().wizard_set(&pointer, &text)
            })
            .unwrap_or(false)
    }

    /// Put the wizard's playbook into the editor, as one edit Undo takes back.
    #[func]
    fn editor_wizard_use(&mut self) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        self.panics
            .guard("editor_wizard_use", || rig.editor_mut().wizard_use())
            .unwrap_or(false)
    }

    /// Close the wizard.
    #[func]
    fn editor_wizard_close(&mut self) {
        if let Some(rig) = self.rig.as_mut() {
            let _ = self
                .panics
                .guard("editor_wizard_close", || rig.editor_mut().wizard_close());
        }
    }

    /// One `instantiate_template` answer's text (a whole JSON-RPC answer, or its result),
    /// with no gateway behind it: `pages`, `why` and `playbook_jsonc`, as the render-only
    /// wizard scene and the watch check's pin read them. An empty dictionary means the
    /// reason is in the log.
    #[func]
    fn editor_wizard_of_response(&mut self, text: GString) -> VarDictionary {
        let text = text.to_string();
        match self.panics.guard("editor_wizard_of_response", || {
            crate::wizard::instance_of_text(&text)
        }) {
            Some(Ok(instance)) => instance_dictionary(&instance),
            Some(Err(error)) => {
                godot_error!("[pharmakos] editor_wizard_of_response: {error}");
                VarDictionary::new()
            }
            None => VarDictionary::new(),
        }
    }

    /// The own `$`/`kW` meter: `answers`, `phase`, and `treasury_now`, `supply_kw_now`,
    /// `draw_kw_now` and `headroom_kw_now` exactly as the gateway last answered them. Empty
    /// before `watch_begin`.
    #[func]
    fn watch_meter(&mut self) -> VarDictionary {
        let Some(rig) = self.rig.as_ref() else {
            return VarDictionary::new();
        };
        self.panics
            .guard("watch_meter", || meter_dictionary(rig.meter()))
            .unwrap_or_default()
    }

    /// Validation rows from a `gp.api.v1.VerifyReport` JSON text, with no gateway behind
    /// them: what the render-only rows scene draws. An empty array means the reason is in
    /// the log.
    #[func]
    fn editor_rows_of_report(&mut self, report_json: GString) -> VarArray {
        let text = report_json.to_string();
        match self
            .panics
            .guard("editor_rows_of_report", || rows_of_report_text(&text))
        {
            Some(Ok(rows)) => rows_array(&rows),
            Some(Err(error)) => {
                godot_error!("[pharmakos] editor_rows_of_report: {error}");
                VarArray::new()
            }
            None => VarArray::new(),
        }
    }

    /// The voxel of ground under a ray from the camera (`origin` and `direction` in the
    /// world's axes): `{hit, at}`, with `at` in the sim's axes, or `{hit: false}`.
    #[func]
    fn view_pick(&mut self, origin: Vector3, direction: Vector3) -> VarDictionary {
        let mut report = VarDictionary::new();
        let Some(model) = self.view.as_ref() else {
            report.set(&"hit".to_variant(), &false.to_variant());
            return report;
        };
        let found = self
            .panics
            .guard("view_pick", || {
                model.pick(
                    [origin.x, origin.y, origin.z],
                    [direction.x, direction.y, direction.z],
                )
            })
            .flatten();
        report.set(&"hit".to_variant(), &found.is_some().to_variant());
        if let Some([x, y, z]) = found {
            report.set(&"at".to_variant(), &Vector3i::new(x, y, z).to_variant());
        }
        report
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

impl PharmakosBridge {
    /// Builds a renderer for `count` chunks in this node's world, freeing any previous one.
    /// Returns whether there is a world to render into.
    fn attach_renderer(&mut self, count: usize) -> bool {
        if let Some(mut previous) = self.renderer.take() {
            previous.free_all();
        }
        let scenario = self
            .base()
            .get_world_3d()
            .map(|world| world.get_scenario())
            .filter(godot::prelude::Rid::is_valid);
        let Some(scenario) = scenario else {
            return false;
        };
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

    /// Decodes one `get_view` result into the view model: the body of `view_apply`, and
    /// what the watch rig does with every `get_view` answer. `None` when the page was
    /// refused, with the reason in the log.
    fn apply_view(&mut self, result: &pharmakos_proto::json::Json) -> Option<VarDictionary> {
        let Some(mut model) = self.view.take() else {
            godot_error!("[pharmakos] view_apply: call configure() first");
            return None;
        };
        let outcome = self.panics.guard("view_apply", || {
            let page = view::decode_page(result)?;
            model.apply(page)
        });
        let applied: Applied = match outcome {
            Some(Ok(applied)) => applied,
            Some(Err(error)) => {
                godot_error!("[pharmakos] view_apply: {error}");
                self.view = Some(model);
                return None;
            }
            None => {
                self.view = Some(model);
                return None;
            }
        };
        if applied.grid_changed {
            let count = model
                .grid()
                .and_then(|grid| usize::try_from(grid.chunk_count()).ok())
                .unwrap_or(0);
            self.attach_renderer(count);
        }
        let mut report = VarDictionary::new();
        report.set(&"complete".to_variant(), &applied.complete.to_variant());
        report.set(
            &"next_cursor".to_variant(),
            &applied.next_cursor.to_variant(),
        );
        report.set(
            &"at_ms".to_variant(),
            &i64::from(applied.at_ms).to_variant(),
        );
        report.set(&"chunks".to_variant(), &count_of(applied.chunks));
        report.set(&"queued".to_variant(), &count_of(applied.queued));
        report.set(&"pending".to_variant(), &count_of(model.pending()));
        report.set(&"held".to_variant(), &count_of(model.held()));
        if applied.complete {
            let mut entities = VarArray::new();
            for entity in model.entities() {
                entities.push(&entity_dictionary(entity).to_variant());
            }
            report.set(&"entities".to_variant(), &entities.to_variant());
            // Every route the editor draws starts where the seat's commander stands.
            if let Some(rig) = self.rig.as_mut() {
                let seen = model.entities();
                let _ = self
                    .panics
                    .guard("view_apply", || rig.editor_mut().set_entities(seen));
            }
        }
        self.view = Some(model);
        Some(report)
    }
}

/// The rig's state as the dictionary `watch_state` returns.
fn rig_state(rig: &Rig, pending: usize) -> VarDictionary {
    let mut state = VarDictionary::new();
    state.set(&"phase".to_variant(), &rig.phase().name().to_variant());
    state.set(&"round".to_variant(), &i64::from(rig.round()).to_variant());
    state.set(&"timer".to_variant(), &rig.timer_text().to_variant());
    state.set(&"speed".to_variant(), &i64::from(rig.speed()).to_variant());
    state.set(&"skipping".to_variant(), &rig.skipping().to_variant());
    state.set(&"all_ready".to_variant(), &rig.all_ready().to_variant());
    state.set(
        &"drops_admin".to_variant(),
        &i64::from(rig.drops(crate::rig::ADMIN)).to_variant(),
    );
    state.set(
        &"drops_seat".to_variant(),
        &i64::from(rig.drops(crate::rig::SEAT)).to_variant(),
    );
    state.set(
        &"calls_admin".to_variant(),
        &counter(rig.calls(crate::rig::ADMIN)),
    );
    state.set(
        &"calls_seat".to_variant(),
        &counter(rig.calls(crate::rig::SEAT)),
    );
    state.set(
        &"view_settled".to_variant(),
        &rig.view_settled().to_variant(),
    );
    state.set(
        &"seat_settled".to_variant(),
        &rig.seat_settled().to_variant(),
    );
    state.set(
        &"view_refusals".to_variant(),
        &i64::from(rig.view_refusals()).to_variant(),
    );
    state.set(&"pending".to_variant(), &count_of(pending));
    state.set(
        &"last_error".to_variant(),
        &rig.last_error().unwrap_or_default().to_variant(),
    );
    state
}

/// One entity as a dictionary GDScript can read.
fn entity_dictionary(entity: &Entity) -> VarDictionary {
    let mut dictionary = VarDictionary::new();
    dictionary.set(&"id".to_variant(), &entity.id.to_variant());
    dictionary.set(&"kind".to_variant(), &entity.kind.name().to_variant());
    dictionary.set(&"subtype".to_variant(), &entity.subtype.to_variant());
    dictionary.set(&"owner".to_variant(), &entity.owner.to_variant());
    let [x, y, z] = entity.at;
    dictionary.set(&"at".to_variant(), &Vector3i::new(x, y, z).to_variant());
    dictionary
}

/// One renderer's counters as a dictionary GDScript can read.
///
/// A free function rather than a method, so that `upload_counters` can run it inside
/// [`PanicCounter::guard`] while the guard holds `self.panics`.
fn counters_of(renderer: &ChunkRenderer) -> VarDictionary {
    let counters = renderer.uploader().counters();
    let mut report = VarDictionary::new();
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
            // Labelled, because it is the check's OWN fixture's revision and not the
            // committed table's — the two differ whenever a lane adds a row elsewhere in
            // rules/rules.v1.json, and a bare "rules revision: 1" reads like a claim
            // about the shipped table. The five values that matter are pinned to the
            // committed file by `the_self_checks_table_is_the_committed_one`.
            format!(
                "rules revision (this check's fixture): {}",
                self.rules_revision
            ),
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
///
/// The five values are item 54's, written out because the self-check has to run inside a
/// Godot whose `res://` cannot reach `rules/rules.v1.json` at the repository root. That
/// makes them a COPY of a tuning row, which is the shape AGENTS.md section 12 warns
/// about — so `the_self_checks_table_is_the_committed_one` below reads the committed file
/// and fails if the two ever disagree. The copy is loud rather than silent.
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

    /// The self-check's inline rules table is the committed one, value for value.
    ///
    /// [`round_trip_rules`] writes item 54's five numbers out, because a `res://` path
    /// inside Godot cannot reach `rules/rules.v1.json` at the repository root. A copied
    /// tuning row that nothing compares is the failure AGENTS.md section 12 describes:
    /// the row moves, both copies keep the old values, every check stays green and the
    /// self-check reports "rules revision: 1" over a table that no longer exists. This
    /// reads the committed file and fails when they part.
    #[test]
    fn the_self_checks_table_is_the_committed_one() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("rules")
            .join("rules.v1.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
        let committed = rules::mesher_rules(
            &rules::table_from_json(&text).expect("the committed table is canonical gp.v1 JSON"),
        )
        .expect("the committed table has a mesher row");
        let inline = round_trip_rules().expect("the inline table round-trips");

        // The MESHER ROW, not the whole table. `revision` is the committed table's own
        // version and it moves whenever any lane adds a row anywhere in it — it was 1
        // when this fixture was written and is 3 today, with the five values below
        // unchanged. Pinning the revision as well would turn every unrelated row into a
        // failure in this crate, which is how a check gets a `#[allow]` on it.
        assert_eq!(
            inline.budget, committed.budget,
            "the self-check's inline drain budget has drifted from rules/rules.v1.json. \
             Update `round_trip_rules` and `godot/scripts/mesher_rules.gd`'s RULES_JSON \
             together, and say in the pull request which row moved."
        );
        assert_eq!(
            inline.light, committed.light,
            "the self-check's inline light parameters have drifted from \
             rules/rules.v1.json. Update `round_trip_rules` and \
             `godot/scripts/mesher_rules.gd`'s RULES_JSON together, and say in the pull \
             request which row moved."
        );
    }

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
