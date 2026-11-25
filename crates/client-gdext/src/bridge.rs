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
    GString, PackedByteArray, PackedInt32Array, VarArray, VarDictionary, Variant, Vector3, Vector3i,
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
use crate::editor::{RuleEditRefusal, RuleNamed};
use crate::editor_view::{
    instance_dictionary, meter_dictionary, name_into, recap_dictionary, rows_array,
    rows_of_report_text, rule_list_dictionary, state_dictionary, target_of, vocabulary_dictionary,
};
use crate::engine::ChunkRenderer;
use crate::error::BridgeError;
use crate::pacer::WallClock;
use crate::panics::PanicCounter;
use crate::playtest::{self, Another, CameraMove, ExportFailure, Recorder, Understood};
use crate::rig::{Answer, EventRow, LullLengths, MatchPreset, Phase, PresetCheck, Rig, Speaker};
use crate::rules::{self, MesherRules};
use crate::surface::{ColourQuantisation, Surface, SurfaceProbe};
use crate::upload::{DEFAULT_UPLOAD_PATH, ResidentSurface, UploadCounters, UploadPath, Uploader};
use crate::variant::{int_variant, json_to_variant, text_or_empty};
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
    /// The two Lull lengths, `match.first_lull_ms` and `match.lull_ms`, as `configure` read
    /// them; a `None` in either is a Lull with no countdown.
    lulls: LullLengths,
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
    /// Playtest mode's recorder (`crate::playtest`): off at every launch, kept across matches
    /// while on, and fed only by the lines below that call it.
    playtest: Recorder,
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
            lulls: LullLengths::default(),
            buffers: MeshBuffers::empty(),
            transposed: Vec::new(),
            view: None,
            rig: None,
            wall: WallClock::start(),
            playtest: Recorder::new(),
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
    /// How many panics this bridge has caught at its own boundary, as an `int`. CI asserts
    /// zero. A count past what an `int` holds is the typed refusal [`int_variant`] sends.
    #[func]
    fn caught_panics(&self) -> Variant {
        int_variant(self.panics.caught())
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
        let Ok(count) = usize::try_from(chunks) else {
            let reason = format!("{chunks} is not a chunk count");
            godot_error!("[pharmakos] configure: {reason}");
            return not_configured(&reason);
        };
        let text = rules_json.to_string();
        let parsed = self.panics.guard("configure", || {
            rules::table_from_json(&text).and_then(|table| {
                Ok((
                    rules::mesher_rules(&table)?,
                    LullLengths {
                        first_ms: Some(u64::from(rules::first_lull_ms(&table)?)),
                        later_ms: Some(u64::from(rules::lull_ms(&table)?)),
                    },
                    rules::map_extent(&table)?,
                ))
            })
        });
        let (parsed, lulls, extent) = match parsed {
            Some(Ok(parsed)) => parsed,
            Some(Err(error)) => {
                godot_error!("[pharmakos] configure: {error}");
                return not_configured(&error.to_string());
            }
            None => return not_configured("a panic was caught"),
        };

        self.mesher = Some(Mesher::new(parsed.light));
        self.budget = Some(parsed.budget);
        let model = match ViewModel::new(parsed.light, parsed.budget, extent) {
            Ok(model) => model,
            Err(error) => {
                godot_error!("[pharmakos] configure: {error}");
                return not_configured(&error.to_string());
            }
        };
        self.view = Some(model);
        self.lulls = lulls;

        let attached = self.attach_renderer(count);

        let mut report = VarDictionary::new();
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
            .guard_or("upload_budget", VarDictionary::new(), || {
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
            .guard_or("upload_counters", VarDictionary::new(), || {
                counters_of(renderer)
            })
    }

    /// Rewrites one chunk's bytes from the sim's voxel order into the mesher's.
    ///
    /// The one piece of marshalling item 92 names by name. An array of the wrong length
    /// comes back empty and logs why, rather than being padded into something the mesher
    /// would accept.
    ///
    /// The refusal is the answer, and the caller reads it from the empty array, so its
    /// reason is printed as a note rather than as an `ERROR`: the client check's negative
    /// probe asks for exactly this refusal, and its log line read as an error in a player's
    /// log, which the game and the checks share (F6, decisions-log item 126 (3)). Only the
    /// client check calls this today; a caller in play must report the refusal itself (for
    /// example with `push_error`), or a real failure goes by as a plain note.
    #[func]
    fn transpose_chunk(&mut self, sim_order: PackedByteArray) -> PackedByteArray {
        let mut out = std::mem::take(&mut self.transposed);
        let result = self.panics.guard("transpose_chunk", || {
            chunks::sim_to_mesher_chunk("chunk materials", sim_order.as_slice(), &mut out)
        });
        let bytes = match result {
            Some(Ok(())) => PackedByteArray::from(out.as_slice()),
            Some(Err(error)) => {
                godot_print!("[pharmakos] transpose_chunk refused: {error}");
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
    /// Pointer in the log: an unknown field is refused here, never stripped. The `null` is
    /// the refusal, so the reason is printed as a note, not an `ERROR`, for the reason
    /// `transpose_chunk` gives. Only the client check calls this today; a caller in play
    /// must report the refusal itself (for example with `push_error`), or schema drift
    /// between the client and the gateway goes by as a plain note.
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
                godot_print!("[pharmakos] decode_result({wire}) refused: {error}");
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
        let edge = view::CHUNK_EDGE_F32;
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
                    renderer.upload(
                        view::chunk_slot(chunk),
                        corner.map(|value| value as f32),
                        &surface,
                    )?;
                }
                uploaded = uploaded.checked_add(1).ok_or(BridgeError::CountOverflow {
                    what: "chunks uploaded",
                })?;
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
        let Some(grid) = self.view.as_ref().and_then(ViewModel::grid) else {
            return Vector3i::ZERO;
        };
        self.panics.guard_or("view_extent", Vector3i::ZERO, || {
            let [x, y, z] = grid.voxels();
            Vector3i::new(x, y, z)
        })
    }

    /// Starts a fresh watch rig, with both connections closed, timing round 1's Lull by the
    /// `match.first_lull_ms` row and every later one by `match.lull_ms`, as `configure` read
    /// them (a Lull whose row is missing is untimed).
    #[func]
    fn watch_begin(&mut self) {
        // The rig this replaces is the one teardown of a match in the client: Playtest mode keeps
        // what was open in it as left, timed to the last frame drawn in it.
        let _ = self
            .panics
            .guard("watch_begin", || self.playtest.match_begun());
        let mut rig = Rig::new();
        rig.set_lull_length(self.lulls);
        self.rig = Some(rig);
    }

    /// Connection `link` (0 admin, 1 seat) is open.
    #[func]
    fn watch_opened(&mut self, link: i64) {
        if let (Some(rig), Ok(link)) = (self.rig.as_mut(), usize::try_from(link)) {
            let _ = self.panics.guard("watch_opened", || rig.opened(link));
        }
    }

    /// What `host_link.gd` does next with connection `link` (0 admin, 1 seat): `wait`,
    /// `connect` or `give_up`, as the rig's reconnect policy says (fix pass 5). The script asks
    /// this every frame a link is closed, and decides nothing itself.
    #[func]
    fn watch_link_action(&self, link: i64) -> GString {
        let action = match (self.rig.as_ref(), usize::try_from(link)) {
            (Some(rig), Ok(link)) => rig.link_action(link),
            _ => None,
        };
        GString::from(match action {
            Some(crate::rig::LinkAction::Wait) => "wait",
            Some(crate::rig::LinkAction::Connect) => "connect",
            Some(crate::rig::LinkAction::GiveUp) => "give_up",
            // No rig, or no such link: there is nothing to open again.
            None => "no_link",
        })
    }

    /// Why connection `link` was given up: `attempts` (in a row, none of which stayed up) and
    /// `code` (the last close's code, -1 for a connection lost with none), and, when the host
    /// process exited, `host_exited` (true) with `host_words`, its last line ("" for none), which
    /// the lobby leads with (S3's `outcap`); empty when it was not given up.
    #[func]
    fn watch_give_up(&self, link: i64) -> VarDictionary {
        let mut words = VarDictionary::new();
        if let (Some(rig), Ok(link)) = (self.rig.as_ref(), usize::try_from(link))
            && let Some(why) = rig.give_up(link)
        {
            words.set(
                &"attempts".to_variant(),
                &i64::from(why.attempts).to_variant(),
            );
            let code = match why.last {
                Some(crate::rig::Close::ThisMessage(code)) => i64::from(code),
                Some(crate::rig::Close::Connection) | None => -1,
            };
            words.set(&"code".to_variant(), &code.to_variant());
            if let Some(host) = why.host_exited {
                words.set(&"host_exited".to_variant(), &true.to_variant());
                words.set(&"host_words".to_variant(), &host.to_variant());
            }
        }
        words
    }

    /// The host process has exited, and `words` is the last line it wrote on its standard error
    /// ("" for none), as `host_link.gd` read it (S3's `outcap`): the rig gives every closed link up
    /// at once and hands the words back through `watch_give_up`. Passed through: the rig decides.
    #[func]
    fn watch_host_exited(&mut self, words: GString) {
        if let Some(rig) = self.rig.as_mut() {
            let words = words.to_string();
            let _ = self
                .panics
                .guard("watch_host_exited", || rig.host_exited(&words));
        }
    }

    /// Connection `link`'s state as its `WebSocketPeer` reports it (`get_ready_state`), every
    /// frame (fix pass 7). Answers whether `host_link.gd` aborts the peer now: an open link that
    /// owes a close (a call past its deadline), a connect past its deadline, or a close past its
    /// grace (`Rig::transport`). A state Godot does not define is said and aborted: a connection
    /// in a state the rig cannot read is not left to hang.
    #[func]
    fn watch_transport(&mut self, link: i64, state: i64) -> bool {
        match (
            self.rig.as_mut(),
            usize::try_from(link),
            crate::rig::Transport::of_ready_state(state),
        ) {
            (Some(rig), Ok(link), Some(transport)) => {
                self.panics
                    .guard_or("watch_transport", true, || rig.transport(link, transport))
            }
            (_, _, None) => {
                godot_error!(
                    "[pharmakos] watch_transport: connection {link} is in state {state}, which this client cannot read; it is aborted"
                );
                true
            }
            // No rig, or no such link: there is no connection to abort.
            _ => false,
        }
    }

    /// Closes on connection `link` since it was last healthy (the rig's reconnect policy).
    #[func]
    fn watch_link_attempts(&self, link: i64) -> i64 {
        match (
            self.rig.as_ref(),
            usize::try_from(link)
                .ok()
                .and_then(|link| self.rig.as_ref()?.link_attempts(link)),
        ) {
            (Some(_), Some(attempts)) => i64::from(attempts),
            // No rig, or no such link: said as no count.
            _ => -1,
        }
    }

    /// The transport refused to send the frame `link` (0 admin, 1 seat) has in flight:
    /// `WebSocketPeer.send_text` answered the error `code` (fix pass 2). The rig settles that call
    /// as refused at once and never asks it again.
    #[func]
    fn watch_send_failed(&mut self, link: i64, code: i64) {
        if let (Some(rig), Ok(link)) = (self.rig.as_mut(), usize::try_from(link)) {
            let _ = self
                .panics
                .guard("watch_send_failed", || rig.send_failed(link, code));
        }
    }

    /// What each WebSocket peer must carry, as `host_link.gd` sets it on every peer it makes:
    /// `outbound_bytes` and `inbound_bytes` (`crate::transport`), and `message_bytes`, the
    /// gateway's message cap the rig sends nothing past, for the words that name it. Passed
    /// through, never computed in GDScript.
    #[func]
    fn watch_transport_limits(&self) -> VarDictionary {
        let mut limits = VarDictionary::new();
        for (key, value) in [
            ("outbound_bytes", crate::transport::OUTBOUND_BUFFER_BYTES),
            ("inbound_bytes", crate::transport::INBOUND_BUFFER_BYTES),
            (
                "message_bytes",
                pharmakos_proto::transport::MAX_MESSAGE_BYTES,
            ),
        ] {
            limits.set(&key.to_variant(), &int_variant(value));
        }
        limits
    }

    /// Connection `link` (0 admin, 1 seat) dropped, with the WebSocket close `code` the peer
    /// read (`WebSocketPeer.get_close_code()`, -1 for none): a code about the message itself
    /// (1009 and its kin) settles the call in flight as refused and asks it never again
    /// ([`crate::rig::Close`]).
    #[func]
    fn watch_dropped(&mut self, link: i64, code: i64) {
        if let (Some(rig), Ok(link)) = (self.rig.as_mut(), usize::try_from(link)) {
            let close = crate::rig::Close::of_code(code);
            let _ = self
                .panics
                .guard("watch_dropped", || rig.closed(link, close));
        }
    }

    /// Reads one text frame from connection `link`.
    ///
    /// Returns `phase`, `phase_changed`, `events` (new event-list rows, each a dictionary
    /// `{clock, kind, text, ledger, line}`: `event_row_dictionary`),
    /// `error` (the gateway's refusal, empty when there was none) and, for a `get_view`
    /// answer, `view`: what `view_apply` returns for the same result. An empty dictionary
    /// means the frame could not be read, and the reason is in the log. A `link` no connection
    /// has is refused before anything is touched: `{refused: "no_such_link"}`, the rig kept.
    #[func]
    fn watch_receive(&mut self, link: i64, text: GString) -> VarDictionary {
        let Some(link) = usize::try_from(link)
            .ok()
            .filter(|link| *link == crate::rig::ADMIN || *link == crate::rig::SEAT)
        else {
            godot_error!("[pharmakos] watch_receive: no connection is link {link}");
            let mut refused = VarDictionary::new();
            refused.set(&"refused".to_variant(), &"no_such_link".to_variant());
            return refused;
        };
        let Some(mut rig) = self.rig.take() else {
            return VarDictionary::new();
        };
        let text = text.to_string();
        let read = self
            .panics
            .guard("watch_receive", || rig.receive(link, &text));
        if let Some(Ok(answer)) = read.as_ref() {
            // Playtest mode keeps a phase change and a refusal's code; off, it keeps nothing.
            let now = self.wall.now_us();
            let _ = self.panics.guard("watch_receive", || {
                self.playtest.answered(now, &rig, answer)
            });
        }
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
            view_starts,
            events,
            error,
            phase_changed,
        } = answer;
        if view.is_some()
            && view_starts
            && let Some(model) = self.view.as_mut()
        {
            model.begin_listing();
        }
        let mut report = VarDictionary::new();
        report.set(&"phase".to_variant(), &phase.to_variant());
        report.set(&"phase_changed".to_variant(), &phase_changed.to_variant());
        let mut rows = VarArray::new();
        for row in &events {
            rows.push(&event_row_dictionary(row).to_variant());
        }
        report.set(&"events".to_variant(), &rows.to_variant());
        report.set(
            &"error".to_variant(),
            &text_or_empty(error.as_deref()).to_variant(),
        );
        if let Some(result) = view {
            // The rig has already moved its cursor past this page; a page the bridge
            // refuses is lost unless the rig is told, so it asks for a keyframe again.
            let applied = self.apply_view(&result);
            if applied.is_none()
                && let Some(rig) = self.rig.as_mut()
            {
                let _ = self.panics.guard("watch_receive", || rig.view_refused());
            }
            report.set(
                &"view".to_variant(),
                &applied.unwrap_or_default().to_variant(),
            );
        }
        report
    }

    /// The lobby wrote the preset `name` (`custom`, `probation`) on a new match's config line:
    /// the rig holds the gateway's `get_status` answer to it, and `watch_state`'s
    /// `preset_check` says whether the two agree. Returns `false`, holding nothing, for a
    /// name no preset has, or before `watch_begin`.
    #[func]
    fn watch_expect_preset(&mut self, name: GString) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        let Some(preset) = MatchPreset::of_name(&name.to_string()) else {
            godot_error!("[pharmakos] watch_expect_preset: `{name}` names no preset");
            return false;
        };
        self.panics.guard_or("watch_expect_preset", false, || {
            rig.expect_preset(preset);
            true
        })
    }

    /// A `get_recap` answer's text (a whole JSON-RPC answer) read with no gateway behind it,
    /// in the shape `watch_recap` returns: what the hostless lobby shot draws. An empty
    /// dictionary means the reason is in the log.
    #[func]
    fn watch_recap_of_response(&mut self, text: GString) -> VarDictionary {
        let text = text.to_string();
        let read = self.panics.guard("watch_recap_of_response", || {
            crate::rig::recap_of_text(&text)
        });
        match read {
            Some(Ok(recap)) => recap_dictionary(&recap),
            Some(Err(error)) => {
                godot_error!("[pharmakos] watch_recap_of_response: {error}");
                VarDictionary::new()
            }
            None => VarDictionary::new(),
        }
    }

    /// A `get_status` answer's text (a whole JSON-RPC answer) read with no gateway behind it, as
    /// the rig reads one: `preset`, the served preset's name (`none` for no match hosted). An
    /// empty dictionary means the reason is in the log.
    #[func]
    fn watch_status_of_response(&mut self, text: GString) -> VarDictionary {
        let text = text.to_string();
        let read = self.panics.guard("watch_status_of_response", || {
            let frame = pharmakos_proto::json::read(&text)?;
            let result = frame
                .get("result")
                .cloned()
                .ok_or_else(|| BridgeError::Rpc("a committed answer with no result".to_owned()))?;
            crate::rig::read_status_facts(&result)
        });
        let mut out = VarDictionary::new();
        match read {
            Some(Ok((preset, _))) => out.set(&"preset".to_variant(), &preset.name().to_variant()),
            Some(Err(error)) => godot_error!("[pharmakos] watch_status_of_response: {error}"),
            None => {}
        }
        out
    }

    /// A `get_segment_feed` answer's text (a whole JSON-RPC answer) read with no gateway behind
    /// it: its event-list rows, each as `watch_receive`'s `events` carries one. An empty array
    /// means the feed was empty or the reason is in the log.
    #[func]
    fn watch_rows_of_response(&mut self, text: GString) -> VarArray {
        let text = text.to_string();
        let read = self.panics.guard("watch_rows_of_response", || {
            crate::rig::feed_rows_of_text(&text)
        });
        let mut rows = VarArray::new();
        match read {
            Some(Ok(events)) => {
                for row in &events {
                    rows.push(&event_row_dictionary(row).to_variant());
                }
            }
            Some(Err(error)) => godot_error!("[pharmakos] watch_rows_of_response: {error}"),
            None => {}
        }
        rows
    }

    /// The speeds the watch rig offers, `pacer::SPEEDS`, in order: the lobby builds its
    /// speed buttons from this, so the set has one home.
    #[func]
    fn watch_speeds(&self) -> PackedInt32Array {
        let mut speeds = PackedInt32Array::new();
        for speed in crate::pacer::SPEEDS {
            // Lossless: `all_fit_i32` proves every speed fits an `i32` at compile time, so
            // reading its bits as signed keeps its value.
            speeds.push(speed.cast_signed());
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
            .guard_or("watch_outbox", Vec::new(), || rig.poll(now));
        // Playtest mode takes each frame's reading, so a frame gap is told apart from play, and
        // counts the verifier's error rows the editor shows, by code.
        let _ = self.panics.guard("watch_outbox", || {
            self.playtest.seen(now);
            self.playtest.shown(rig.editor())
        });
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
        let now = self.wall.now_us();
        let playtest = &mut self.playtest;
        // Each command the rig takes is told to Playtest mode, which counts it and decides
        // nothing; off, it keeps nothing.
        self.panics
            .guard_or("watch_command", false, || match command.as_str() {
                "speed" => u32::try_from(value).is_ok_and(|speed| {
                    let from = rig.speed();
                    let taken = rig.set_speed(speed);
                    if taken {
                        playtest.speed(now, from, speed);
                    }
                    taken
                }),
                "skip" => {
                    rig.skip();
                    playtest.skip(now);
                    true
                }
                "ready" => {
                    rig.ready();
                    playtest.ready(now);
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
    }

    /// What the lobby shows: `phase`, `round`, `timer` (empty while the footer is
    /// `untimed`, which is also given), `speed`, `skipping`, `all_ready`,
    /// `drops_admin`, `drops_seat`, `calls_admin`, `calls_seat`, `view_settled`,
    /// `seat_settled`, `view_refusals`, `pending` and `last_error`.
    #[func]
    fn watch_state(&mut self) -> VarDictionary {
        let Some(rig) = self.rig.as_ref() else {
            return VarDictionary::new();
        };
        let pending = self.view.as_ref().map_or(0, ViewModel::pending);
        self.panics
            .guard_or("watch_state", VarDictionary::new(), || {
                rig_state(rig, pending)
            })
    }

    // --- Playtest mode (S3's `prob`, part 2) -------------------------------------------------
    //
    // Off at every launch (`crate::playtest`). The lobby's check button switches it;
    // `playtest.gd` forwards the window's focus and the camera rig's moves, sends the survey and
    // asks for the export. The recorder is fed only here and by the three lines above that tell
    // it a rig answer, a command and the editor's rows; it reads the clock through `self.wall`,
    // and nothing here is sent to the gateway.

    /// Switches Playtest mode on (`true`) or off, the window having focus now or not
    /// (`focused`). Off drops everything it kept. Answers whether it is on.
    #[func]
    fn playtest_set(&mut self, on: bool, focused: bool) -> bool {
        let now = self.wall.now_us();
        let (phase, round) = self
            .rig
            .as_ref()
            .map_or((Phase::Unknown, 0), |rig| (rig.phase(), rig.round()));
        let _ = self.panics.guard("playtest_set", || {
            self.playtest.switch(on, now, focused, phase, round)
        });
        self.playtest.is_on()
    }

    /// The window gained (`true`) or lost focus (Godot's application focus notifications).
    #[func]
    fn playtest_focus(&mut self, focused: bool) {
        let now = self.wall.now_us();
        let _ = self
            .panics
            .guard("playtest_focus", || self.playtest.focus(now, focused));
    }

    /// The tester moved the camera: `kind` is `pan`, `zoom`, `turn` or `follow`. Returns `false`,
    /// counting nothing, for any other word.
    #[func]
    fn playtest_camera(&mut self, kind: GString) -> bool {
        let Some(kind) = CameraMove::of_name(&kind.to_string()) else {
            godot_error!("[pharmakos] playtest_camera: `{kind}` is no camera move");
            return false;
        };
        let _ = self
            .panics
            .guard("playtest_camera", || self.playtest.camera(kind));
        true
    }

    /// Export playtest bundle, with the survey's answers as the screen shows them at this moment:
    /// `another` (`yes`, `no`, `unanswered`), `understood` (`yes`, `partly`, `no`,
    /// `unanswered`) and `text`, the tester's own words, sent as written. The bundle is written
    /// whole to `path` (an absolute path the tester picked in a file dialog), and nowhere else.
    /// Answers as `file_write` does (`written`, and on a failure `failure`, `path`, `step`,
    /// `error`, `leftover`), with `bytes` on a write, and `refusal` when no bundle was built:
    /// `off`, `too_large`, `survey_too_long` (with `limit`, the most characters the text takes;
    /// no file is written), `unknown_answer`, or `failed` (a caught panic, counted and named).
    #[func]
    fn playtest_export(
        &mut self,
        path: GString,
        another: GString,
        understood: GString,
        text: GString,
    ) -> VarDictionary {
        let refused = |name: &str| {
            let mut answer = VarDictionary::new();
            answer.set(&"written".to_variant(), &false.to_variant());
            answer.set(&"refusal".to_variant(), &name.to_variant());
            answer.set(
                &"limit".to_variant(),
                &count_of(playtest::MAX_SURVEY_TEXT_CHARS),
            );
            answer
        };
        let (Some(another), Some(understood)) = (
            Another::of_name(&another.to_string()),
            Understood::of_name(&understood.to_string()),
        ) else {
            return refused("unknown_answer");
        };
        let path = std::path::PathBuf::from(path.to_string());
        let text = text.to_string();
        let now = self.wall.now_us();
        match self.panics.guard("playtest_export", || {
            self.playtest.export(now, &path, another, understood, &text)
        }) {
            Some(Ok(exported)) => {
                let mut answer = written_dictionary(Ok(exported
                    .stale_removed
                    .map(|stale| stale.display().to_string())));
                answer.set(&"bytes".to_variant(), &count_of(exported.bytes));
                answer
            }
            Some(Err(ExportFailure::Write(failure))) => written_dictionary(Err(failure)),
            Some(Err(ExportFailure::Refused(refusal))) => refused(refusal.name()),
            None => refused("failed"),
        }
    }

    /// What the lobby shows of Playtest mode: `on`, the rig's `phase` (empty before a match),
    /// and while on `pushes`, `skipped_pushes`, `lulls`, `lines`, `dropped` (whether anything
    /// was), with `survey_limit`, the most characters the survey's text takes, and `entries`, the
    /// bundle's file names.
    #[func]
    fn playtest_state(&self) -> VarDictionary {
        let mut state = VarDictionary::new();
        let mut put = |key: &str, value: Variant| state.set(&key.to_variant(), &value);
        put("on", self.playtest.is_on().to_variant());
        put(
            "phase",
            self.rig
                .as_ref()
                .map_or("", |rig| rig.phase().name())
                .to_variant(),
        );
        put("survey_limit", count_of(playtest::MAX_SURVEY_TEXT_CHARS));
        let entries: godot::builtin::PackedStringArray = playtest::BUNDLE_ENTRIES
            .iter()
            .map(|name| GString::from(*name))
            .collect();
        put("entries", entries.to_variant());
        if let Some(summary) = self.playtest.summary() {
            put("pushes", counter(summary.pushes));
            put("skipped_pushes", counter(summary.skipped_pushes));
            put("lulls", count_of(summary.lulls));
            put("lines", count_of(summary.lines));
            put("dropped", summary.dropped.to_variant());
        }
        state
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
        self.panics.guard_or("editor_load", false, || {
            rig.editor_mut().load(bytes.as_slice())
        })
    }

    /// Load a playbook file's bytes, read from `path`: as `editor_load`, and once the editor
    /// takes the text, `path` is the file it came from, which Save writes and Save as offers
    /// (the editor state's `file`).
    #[func]
    fn editor_load_file(&mut self, bytes: PackedByteArray, path: GString) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        let path = path.to_string();
        self.panics.guard_or("editor_load_file", false, || {
            rig.editor_mut().open(bytes.as_slice(), Some(&path))
        })
    }

    /// New: put `bytes`, the empty playbook the client ships, on screen as one edit Undo takes
    /// back; its checks follow as for any edit. Returns `false` with the door shut or for
    /// bytes that are not text.
    #[func]
    fn editor_new(&mut self, bytes: PackedByteArray) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        self.panics.guard_or("editor_new", false, || {
            rig.editor_mut().start_new(bytes.as_slice())
        })
    }

    /// The playbook on screen was written to `path`: from now on it is the text's file.
    #[func]
    fn editor_saved_to(&mut self, path: GString) {
        let path = path.to_string();
        if let Some(rig) = self.rig.as_mut() {
            let _ = self
                .panics
                .guard("editor_saved_to", || rig.editor_mut().saved_to(&path));
        }
    }

    /// `bytes` written to the file `path` (an absolute path) whole, by an atomic replace
    /// (`crate::write`, S3's `fixq-c`, fix pass 5): Save and Save as..., and the lobby's remembered
    /// line. Answers `written`, and on a failure `failure` (the string table's `write_<failure>`
    /// row), `path`, `step`, `error` (the operating system's words) and `leftover`; on a write,
    /// `stale` (a temporary file of ours from a stopped write, removed first) when there was one.
    #[func]
    fn file_write(&self, path: GString, bytes: PackedByteArray) -> VarDictionary {
        let path = std::path::PathBuf::from(path.to_string());
        written_dictionary(
            crate::write::write_whole(&path, bytes.as_slice()).map(|written| {
                written
                    .stale_removed
                    .map(|stale| stale.display().to_string())
            }),
        )
    }

    /// The names of every write failure (`crate::write::WriteFailure::NAMES`), for the check that
    /// the string table has a `write_<name>` row for each.
    #[func]
    fn file_failure_names(&self) -> godot::builtin::PackedStringArray {
        crate::write::WriteFailure::NAMES
            .iter()
            .map(|name| GString::from(*name))
            .collect()
    }

    /// The file `path` (an absolute path) removed: the lobby's forgetting of an ended match.
    /// Answers as [`Self::file_write`] does (`written` meaning it is gone, or was never there).
    #[func]
    fn file_remove(&self, path: GString) -> VarDictionary {
        let path = std::path::PathBuf::from(path.to_string());
        written_dictionary(crate::write::remove_whole(&path).map(|_| None))
    }

    /// The playbook the editor holds, as the bytes Save writes; empty before a Load.
    #[func]
    fn editor_bytes(&mut self) -> PackedByteArray {
        let Some(rig) = self.rig.as_ref() else {
            return PackedByteArray::new();
        };
        self.panics
            .guard_or("editor_bytes", PackedByteArray::new(), || {
                PackedByteArray::from(rig.editor().bytes().as_slice())
            })
    }

    /// The bytes the last accepted `submit_plan` sealed; empty before one.
    #[func]
    fn editor_sealed_bytes(&mut self) -> PackedByteArray {
        let Some(rig) = self.rig.as_ref() else {
            return PackedByteArray::new();
        };
        self.panics
            .guard_or("editor_sealed_bytes", PackedByteArray::new(), || {
                PackedByteArray::from(rig.editor().sealed_bytes().as_slice())
            })
    }

    /// A map action: `go`, `visit_low`, `visit_normal`, `visit_high`, `recycle`, `place` or
    /// `place_covering`, at a target dictionary naming a `beacon`, a `selector`, a `voxel`
    /// (sim axes), a `feature` (its name) or `nearest_uncovered` (a feature kind).
    /// Returns whether the action was taken.
    #[func]
    fn editor_action(&mut self, action: GString, target: VarDictionary) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        let action = action.to_string();
        self.panics.guard_or("editor_action", false, || {
            match (Action::from_name(&action), target_of(&target)) {
                (Some(action), Some(target)) => rig.editor_mut().act(action, &target),
                _ => false,
            }
        })
    }

    /// The placement ghost at `at` (sim axes): the QUICK verdict of the draft with a beacon
    /// placed there, asked for once per click.
    #[func]
    fn editor_preview_place(&mut self, at: Vector3i) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        self.panics.guard_or("editor_preview_place", false, || {
            rig.editor_mut().preview_place([at.x, at.y, at.z])
        })
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
            .guard_or("editor_fix", false, || rig.editor_mut().fix(row, fix))
    }

    /// Undo the last edit with the inverse patch the gateway handed back for it.
    #[func]
    fn editor_undo(&mut self) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        self.panics
            .guard_or("editor_undo", false, || rig.editor_mut().undo())
    }

    /// Submit the playbook on screen.
    #[func]
    fn editor_submit(&mut self) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        self.panics
            .guard_or("editor_submit", false, || rig.editor_mut().submit())
    }

    /// Tick (`on`) or clear the `allow_dormant_beacons` checkbox E0601 shows: the verifier's
    /// own patch, or the option's removal, through `patch_plan`. Returns whether it was taken.
    #[func]
    fn editor_set_allow_dormant(&mut self, on: bool) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        self.panics.guard_or("editor_set_allow_dormant", false, || {
            rig.editor_mut().set_allow_dormant(on).is_ok()
        })
    }

    // --- The rule list (S3's `ed3b`) ---------------------------------------------------------
    //
    // Each edit names the rendering it was opened on (`revision`, the editor state's
    // `rules_revision`) and the chip's `pointer`, so it never lands on a chip of another
    // rendering. An index or a revision no Rust type holds (a negative int) is refused
    // through the editor like any other refusal, by its reason on the status line.

    /// Rule-list chip `chip`'s value field sent `text`, exactly as typed: the editor reads it
    /// as a whole number and sends one `replace` through `patch_plan`. Returns whether it was
    /// taken; a refusal is on the status line, on the chip's rule and in the state's `field`.
    #[func]
    fn editor_set_number_text(
        &mut self,
        chip: i64,
        revision: i64,
        pointer: GString,
        text: GString,
    ) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        let (pointer, text) = (pointer.to_string(), text.to_string());
        self.panics.guard_or("editor_set_number_text", false, || {
            let editor = rig.editor_mut();
            match (usize::try_from(chip), u64::try_from(revision)) {
                (Ok(chip), Ok(revision)) => editor
                    .set_number_text(chip, revision, &pointer, &text)
                    .is_ok(),
                (Err(_), _) => refused(editor, RuleEditRefusal::NoSuchChip),
                (_, Err(_)) => refused(editor, RuleEditRefusal::Changed),
            }
        })
    }

    /// A value field opened: what the last one heard is no longer its to say.
    #[func]
    fn editor_field_opened(&mut self) {
        if let Some(rig) = self.rig.as_mut() {
            let _ = self
                .panics
                .guard("editor_field_opened", || rig.editor_mut().field_opened());
        }
    }

    /// Rule-list chip `chip`'s enum took its `which`th value as sent: one `replace`.
    #[func]
    fn editor_choose(&mut self, chip: i64, revision: i64, pointer: GString, which: i64) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        let pointer = pointer.to_string();
        self.panics.guard_or("editor_choose", false, || {
            let editor = rig.editor_mut();
            match (
                usize::try_from(chip),
                u64::try_from(revision),
                usize::try_from(which),
            ) {
                (Ok(chip), Ok(revision), Ok(which)) => {
                    editor.choose(chip, revision, &pointer, which).is_ok()
                }
                (Err(_), _, _) => refused(editor, RuleEditRefusal::NoSuchChip),
                (_, Err(_), _) => refused(editor, RuleEditRefusal::Changed),
                (_, _, Err(_)) => refused(editor, RuleEditRefusal::NoSuchValue),
            }
        })
    }

    /// The next map click is rule-list chip `chip`'s pick.
    #[func]
    fn editor_begin_pick(&mut self, chip: i64, revision: i64, pointer: GString) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        let pointer = pointer.to_string();
        self.panics.guard_or("editor_begin_pick", false, || {
            let editor = rig.editor_mut();
            match (usize::try_from(chip), u64::try_from(revision)) {
                (Ok(chip), Ok(revision)) => editor.begin_pick(chip, revision, &pointer).is_ok(),
                (Err(_), _) => refused(editor, RuleEditRefusal::NoSuchChip),
                (_, Err(_)) => refused(editor, RuleEditRefusal::Changed),
            }
        })
    }

    /// Calls the pick off.
    #[func]
    fn editor_cancel_pick(&mut self) {
        if let Some(rig) = self.rig.as_mut() {
            let _ = self
                .panics
                .guard("editor_cancel_pick", || rig.editor_mut().cancel_pick());
        }
    }

    /// A map click while a pick is on, at a target dictionary as `editor_action` takes it (a
    /// `beacon`, a `voxel` or a `feature`): one `replace` of the picked chip. Returns whether
    /// it was taken; a refusal is on the status line.
    #[func]
    fn editor_pick(&mut self, target: VarDictionary) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        self.panics.guard_or("editor_pick", false, || {
            let editor = rig.editor_mut();
            match target_of(&target) {
                Some(target) => editor.pick(&target).is_ok(),
                // A click on nothing: refused, and the pick stays on for the next click.
                None => editor.pick_missed().is_ok(),
            }
        })
    }

    /// A drag of rule `from` onto rule `to`, each named by its index and its pointer, in the
    /// rendering `rendering` the drag began on: one `move`.
    #[func]
    fn editor_move_rule(
        &mut self,
        from: i64,
        from_pointer: GString,
        to: i64,
        to_pointer: GString,
        rendering: i64,
    ) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        let (from_pointer, to_pointer) = (from_pointer.to_string(), to_pointer.to_string());
        self.panics.guard_or("editor_move_rule", false, || {
            let editor = rig.editor_mut();
            match (
                usize::try_from(from),
                usize::try_from(to),
                u64::try_from(rendering),
            ) {
                (Ok(from), Ok(to), Ok(rendering)) => editor
                    .move_rule(
                        &RuleNamed {
                            index: from,
                            rendering,
                            pointer: from_pointer,
                        },
                        &RuleNamed {
                            index: to,
                            rendering,
                            pointer: to_pointer,
                        },
                    )
                    .is_ok(),
                (Err(_), _, _) | (_, Err(_), _) => refused(editor, RuleEditRefusal::NoSuchRule),
                (_, _, Err(_)) => refused(editor, RuleEditRefusal::Changed),
            }
        })
    }

    /// The condition picker took predicate `entry` of family `family` (indices into the editor
    /// state's `vocabulary`) for condition slot `slot` (its index and `pointer` in the rendering
    /// `rendering` the picker opened on): one patch with the op the slot names. Returns whether
    /// it was taken; a refusal is on the status line.
    #[func]
    fn editor_choose_predicate(
        &mut self,
        slot: i64,
        pointer: GString,
        rendering: i64,
        family: i64,
        entry: i64,
    ) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        let pointer = pointer.to_string();
        self.panics.guard_or("editor_choose_predicate", false, || {
            let editor = rig.editor_mut();
            match (
                usize::try_from(slot),
                u64::try_from(rendering),
                usize::try_from(family),
                usize::try_from(entry),
            ) {
                (Ok(slot), Ok(rendering), Ok(family), Ok(entry)) => editor
                    .choose_predicate(slot, rendering, &pointer, family, entry)
                    .is_ok(),
                (Err(_), _, _, _) => refused(editor, RuleEditRefusal::NoSuchSlot),
                (_, Err(_), _, _) => refused(editor, RuleEditRefusal::Changed),
                (_, _, Err(_), _) | (_, _, _, Err(_)) => {
                    refused(editor, RuleEditRefusal::NoSuchEntry)
                }
            }
        })
    }

    /// The condition picker took group `which` (an index into the editor state's
    /// `vocabulary.groups`: `all`, `any`, `not`) for condition slot `slot`, named as for
    /// [`Self::editor_choose_predicate`]: one patch with the op the slot names (S3's `fixq`).
    #[func]
    fn editor_choose_group(
        &mut self,
        slot: i64,
        pointer: GString,
        rendering: i64,
        which: i64,
    ) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        let pointer = pointer.to_string();
        self.panics.guard_or("editor_choose_group", false, || {
            let editor = rig.editor_mut();
            match (
                usize::try_from(slot),
                u64::try_from(rendering),
                usize::try_from(which),
            ) {
                (Ok(slot), Ok(rendering), Ok(which)) => editor
                    .choose_group(slot, rendering, &pointer, which)
                    .is_ok(),
                (Err(_), _, _) => refused(editor, RuleEditRefusal::NoSuchSlot),
                (_, Err(_), _) => refused(editor, RuleEditRefusal::Changed),
                (_, _, Err(_)) => refused(editor, RuleEditRefusal::NoSuchEntry),
            }
        })
    }

    /// Add a rule: the gateway's `new_rule` against the text on screen (S3's `fixq`). Returns
    /// whether it was taken; a refusal is on the status line.
    #[func]
    fn editor_new_rule(&mut self) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        self.panics.guard_or("editor_new_rule", false, || {
            rig.editor_mut().new_rule().is_ok()
        })
    }

    /// Remove rule `rule` (its index and `pointer` in the rendering `rendering` it was drawn in):
    /// one `remove` at its pointer. Returns whether it was taken.
    #[func]
    fn editor_remove_rule(&mut self, rule: i64, pointer: GString, rendering: i64) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        let pointer = pointer.to_string();
        self.panics.guard_or("editor_remove_rule", false, || {
            let editor = rig.editor_mut();
            match (usize::try_from(rule), u64::try_from(rendering)) {
                (Ok(index), Ok(rendering)) => editor
                    .remove_rule(&RuleNamed {
                        index,
                        rendering,
                        pointer,
                    })
                    .is_ok(),
                (Err(_), _) => refused(editor, RuleEditRefusal::NoSuchRule),
                (_, Err(_)) => refused(editor, RuleEditRefusal::Changed),
            }
        })
    }

    /// Rule-list name chip `chip` took its `which`th choice from the editor state's
    /// `step_labels` or `rule_ids`, by its kind: one `replace`.
    #[func]
    fn editor_choose_name(
        &mut self,
        chip: i64,
        revision: i64,
        pointer: GString,
        which: i64,
    ) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        let pointer = pointer.to_string();
        self.panics.guard_or("editor_choose_name", false, || {
            let editor = rig.editor_mut();
            match (
                usize::try_from(chip),
                u64::try_from(revision),
                usize::try_from(which),
            ) {
                (Ok(chip), Ok(revision), Ok(which)) => {
                    editor.choose_name(chip, revision, &pointer, which).is_ok()
                }
                (Err(_), _, _) => refused(editor, RuleEditRefusal::NoSuchChip),
                (_, Err(_), _) => refused(editor, RuleEditRefusal::Changed),
                (_, _, Err(_)) => refused(editor, RuleEditRefusal::NoSuchValue),
            }
        })
    }

    /// Visit & change: beacon `beacon`'s setting `setting` (its index and pointer in the editor
    /// state's `beacon_detail`) took `text` from its value field, exactly as typed: read as a
    /// whole number, one route step, one row. A refusal is on the status line and in `field`.
    #[func]
    fn editor_change_setting_number_text(
        &mut self,
        beacon: GString,
        setting: i64,
        pointer: GString,
        text: GString,
    ) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        let (beacon, pointer, text) = (beacon.to_string(), pointer.to_string(), text.to_string());
        self.panics
            .guard_or("editor_change_setting_number_text", false, || {
                let editor = rig.editor_mut();
                match usize::try_from(setting) {
                    Ok(setting) => editor
                        .change_setting_number_text(&beacon, setting, &pointer, &text)
                        .is_ok(),
                    Err(_) => refused(editor, RuleEditRefusal::NoSuchEntry),
                }
            })
    }

    /// Visit & change: beacon `beacon`'s setting `setting` took its `which`th value as sent.
    #[func]
    fn editor_change_setting_choice(
        &mut self,
        beacon: GString,
        setting: i64,
        pointer: GString,
        which: i64,
    ) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        let (beacon, pointer) = (beacon.to_string(), pointer.to_string());
        self.panics
            .guard_or("editor_change_setting_choice", false, || {
                let editor = rig.editor_mut();
                match (usize::try_from(setting), usize::try_from(which)) {
                    (Ok(setting), Ok(which)) => editor
                        .change_setting_choice(&beacon, setting, &pointer, which)
                        .is_ok(),
                    (Err(_), _) => refused(editor, RuleEditRefusal::NoSuchEntry),
                    (_, Err(_)) => refused(editor, RuleEditRefusal::NoSuchValue),
                }
            })
    }

    /// Visit & change: one row on beacon `beacon` naming entry `which` of what `row` names:
    /// `mandate` (switch to the vocabulary's mandate `which`), `add_target` (the next ground
    /// click anchors the vocabulary's blueprint `which`), `remove_target` (the detail's target
    /// `which`) or `queue` (queue the vocabulary's blueprint `which`).
    #[func]
    fn editor_visit_row(&mut self, beacon: GString, row: GString, which: i64) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        let (beacon, row) = (beacon.to_string(), row.to_string());
        self.panics.guard_or("editor_visit_row", false, || {
            let editor = rig.editor_mut();
            match (usize::try_from(which), row.as_str()) {
                (Err(_), _) => refused(editor, RuleEditRefusal::NoSuchEntry),
                (Ok(which), "mandate") => editor.switch_mandate(&beacon, which).is_ok(),
                (Ok(which), "add_target") => editor.begin_build_target(&beacon, which).is_ok(),
                (Ok(which), "remove_target") => editor.remove_build_target(&beacon, which).is_ok(),
                (Ok(which), "queue") => editor.queue_structure(&beacon, which).is_ok(),
                (Ok(_), _) => refused(editor, RuleEditRefusal::NotThisEditor),
            }
        })
    }

    /// Ask for the vocabulary once more, as the picker's "Ask again" does after a refused or
    /// unreadable read. Returns whether it was asked: not with the door shut or a read already
    /// on its way.
    #[func]
    fn editor_vocabulary_again(&mut self) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        self.panics.guard_or("editor_vocabulary_again", false, || {
            rig.editor_mut().ask_vocabulary_again()
        })
    }

    /// The vocabulary from one `get_vocabulary` answer's text, with no gateway behind it: what
    /// the hostless picker scene draws, in the shape of the editor state's `vocabulary`. An
    /// empty dictionary means the reason is in the log.
    #[func]
    fn editor_vocabulary_of_response(&mut self, text: GString) -> VarDictionary {
        let text = text.to_string();
        match self.panics.guard("editor_vocabulary_of_response", || {
            crate::vocabulary::vocabulary_of_text(&text)
        }) {
            Some(Ok(words)) => vocabulary_dictionary(Some(&words), None),
            Some(Err(error)) => {
                godot_error!("[pharmakos] editor_vocabulary_of_response: {error}");
                VarDictionary::new()
            }
            None => VarDictionary::new(),
        }
    }

    /// The features the map lists this Lull (`get_map_summary`), for the editor's vent
    /// click: `id`, `kind`, `x` and `y` as the name spells them, `covered`, `live`, and
    /// `at`, a `Vector3i` in the sim's axes standing on the ground the view holds at the
    /// feature's anchor column. A feature whose column the view does not hold is left out:
    /// there is nowhere to draw it.
    #[func]
    fn editor_features(&mut self) -> VarArray {
        let (Some(rig), Some(model)) = (self.rig.as_ref(), self.view.as_ref()) else {
            return VarArray::new();
        };
        self.panics
            .guard_or("editor_features", VarArray::new(), || {
                let mut out = VarArray::new();
                for feature in rig.editor().features() {
                    let Some(ground) = model.ground_at(feature.x, feature.y) else {
                        continue;
                    };
                    let mut one = VarDictionary::new();
                    one.set(&"id".to_variant(), &feature.id.to_variant());
                    name_into(
                        &mut one,
                        crate::targeting::FeatureName::parse(&feature.id).as_ref(),
                    );
                    one.set(&"kind".to_variant(), &feature.kind.name().to_variant());
                    one.set(&"covered".to_variant(), &feature.covered.to_variant());
                    one.set(&"live".to_variant(), &feature.live.to_variant());
                    one.set(
                        &"at".to_variant(),
                        &Vector3i::new(feature.x, feature.y, ground).to_variant(),
                    );
                    out.push(&one.to_variant());
                }
                out
            })
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
        self.panics.guard_or("editor_save_draft", false, || {
            rig.editor_mut().save_draft(&label)
        })
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
            .guard_or("editor_state", VarDictionary::new(), || {
                state_dictionary(rig.editor())
            })
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
        self.panics.guard_or("editor_wizard_set", false, || {
            rig.editor_mut().wizard_set(&pointer, &text)
        })
    }

    /// Put the wizard's playbook into the editor, as one edit Undo takes back.
    #[func]
    fn editor_wizard_use(&mut self) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        self.panics
            .guard_or("editor_wizard_use", false, || rig.editor_mut().wizard_use())
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
    /// with no gateway behind it: `pages`, `why` and `playbook_jsonc`, as the hostless
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

    /// The template list from one `list_templates` answer's text (a whole JSON-RPC answer, or
    /// its result) with no gateway behind it, in the shape of the editor state's `templates`:
    /// what the hostless template-list shot draws. An empty array means the list was empty or
    /// the reason is in the log.
    #[func]
    fn editor_templates_of_response(&mut self, text: GString) -> VarArray {
        let text = text.to_string();
        match self.panics.guard("editor_templates_of_response", || {
            crate::wizard::templates_of_text(&text)
        }) {
            Some(Ok(rows)) => crate::editor_view::templates_array(&rows),
            Some(Err(error)) => {
                godot_error!("[pharmakos] editor_templates_of_response: {error}");
                VarArray::new()
            }
            None => VarArray::new(),
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
            .guard_or("watch_meter", VarDictionary::new(), || {
                meter_dictionary(rig.meter())
            })
    }

    /// The recap: `answers`, `round` and `prose`, as the gateway last wrote it, and
    /// `refusal`, its refusal of the read as it came (empty when none). Empty before
    /// `watch_begin`.
    #[func]
    fn watch_recap(&mut self) -> VarDictionary {
        let Some(rig) = self.rig.as_ref() else {
            return VarDictionary::new();
        };
        self.panics
            .guard_or("watch_recap", VarDictionary::new(), || {
                recap_dictionary(rig.recap())
            })
    }

    /// The rule list from one `render_plan` answer's text (a whole JSON-RPC answer, or its
    /// result), with no gateway behind it: what the hostless rule-list scene draws, in the
    /// shape of the editor state's `rule_list`. An empty dictionary means the reason is in
    /// the log.
    #[func]
    fn editor_rule_list_of_response(&mut self, text: GString) -> VarDictionary {
        let text = text.to_string();
        match self.panics.guard("editor_rule_list_of_response", || {
            crate::rule_list::rule_list_of_text(&text)
        }) {
            Some(Ok(list)) => rule_list_dictionary(&list),
            Some(Err(error)) => {
                godot_error!("[pharmakos] editor_rule_list_of_response: {error}");
                VarDictionary::new()
            }
            None => VarDictionary::new(),
        }
    }

    /// Validation rows from a `gp.api.v1.VerifyReport` JSON text, with no gateway behind
    /// them: what the hostless rows scene draws. An empty array means the reason is in
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
        // A page that changed the grid built it (`ViewModel::apply`), so the grid is there.
        if applied.grid_changed
            && let Some(grid) = model.grid()
        {
            self.attach_renderer(view::chunk_slot(grid.chunk_count()));
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
    state.set(&"untimed".to_variant(), &rig.untimed().to_variant());
    state.set(
        &"untimed_by_rule".to_variant(),
        &rig.untimed_by_rule().to_variant(),
    );
    state.set(
        &"clock_answers".to_variant(),
        &int_variant(rig.clock_answers()),
    );
    state.set(
        &"countdowns_sent".to_variant(),
        &int_variant(rig.countdowns_sent()),
    );
    // A given-up zero report, with its cause for the string table's `lobby_zero_<cause>` row.
    if let Some((cause, said)) = rig.zero_refusal() {
        state.set(&"zero_refusal".to_variant(), &said.to_variant());
        state.set(&"zero_cause".to_variant(), &cause.to_variant());
    }
    state.set(&"view_held".to_variant(), &rig.view_held().to_variant());
    state.set(&"speed".to_variant(), &i64::from(rig.speed()).to_variant());
    state.set(&"skipping".to_variant(), &rig.skipping().to_variant());
    state.set(&"all_ready".to_variant(), &rig.all_ready().to_variant());
    let [drops_admin, drops_seat] = rig.drops();
    let [[closed_admin, closed_seat], [unsent_admin, unsent_seat]] = rig.message_refusals();
    for (key, count) in [
        ("message_drops_admin", closed_admin),
        ("message_drops_seat", closed_seat),
        ("unsent_admin", unsent_admin),
        ("unsent_seat", unsent_seat),
        ("send_failures_admin", rig.send_failures()[0]),
        ("send_failures_seat", rig.send_failures()[1]),
    ] {
        state.set(&key.to_variant(), &i64::from(count).to_variant());
    }
    let [calls_admin, calls_seat] = rig.calls();
    state.set(
        &"drops_admin".to_variant(),
        &i64::from(drops_admin).to_variant(),
    );
    state.set(
        &"drops_seat".to_variant(),
        &i64::from(drops_seat).to_variant(),
    );
    state.set(&"calls_admin".to_variant(), &counter(calls_admin));
    state.set(&"calls_seat".to_variant(), &counter(calls_seat));
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
        &text_or_empty(rig.last_error()).to_variant(),
    );
    closed_on_into(&mut state, rig);
    status_facts_into(&mut state, rig);
    state
}

/// A write's or a removal's answer for GDScript (`file_write`, `file_remove`).
fn written_dictionary(result: Result<Option<String>, crate::write::WriteFailure>) -> VarDictionary {
    let mut answer = VarDictionary::new();
    let mut put = |key: &str, value: Variant| answer.set(&key.to_variant(), &value);
    match result {
        Ok(stale) => {
            put("written", true.to_variant());
            if let Some(stale) = stale {
                put("stale", stale.to_variant());
            }
        }
        Err(failure) => {
            // The new file is in place when only the folder's sync failed: written, with words.
            put(
                "written",
                matches!(failure, crate::write::WriteFailure::SyncFolder { .. }).to_variant(),
            );
            put("failure", failure.name().to_variant());
            put("path", failure.path().display().to_string().to_variant());
            if let Some(step) = failure.step() {
                put("step", step.to_variant());
            }
            if let Some(error) = failure.error() {
                put("error", error.to_string().to_variant());
            }
            if let Some(leftover) = failure.leftover() {
                put("leftover", leftover.display().to_string().to_variant());
            }
        }
    }
    answer
}

/// The last close on a message on the watch state (fix pass 3), as `closed_on`, present only
/// when there was one: `connection` (`admin` or `seat`), `code`, `call` (the string table's
/// `call_word_<call>`, present only when a call was in flight) and `open` (whether that
/// connection is open now). The lobby says them; it decides nothing from them.
fn closed_on_into(state: &mut VarDictionary, rig: &Rig) {
    let Some(closed) = rig.closed_on_message() else {
        return;
    };
    let mut entry = VarDictionary::new();
    entry.set(&"connection".to_variant(), &closed.connection.to_variant());
    entry.set(&"code".to_variant(), &i64::from(closed.code).to_variant());
    if let Some(call) = closed.call {
        entry.set(&"call".to_variant(), &call.to_variant());
    }
    entry.set(
        &"open".to_variant(),
        &rig.link_open(closed.link).to_variant(),
    );
    state.set(&"closed_on".to_variant(), &entry.to_variant());
}

/// `get_status`'s facts on the watch state: `preset` (the served preset's name, `none` when
/// the gateway hosts no match, `""` before one was read), `preset_check` (`not_read`,
/// `no_match`, `unasked`, `agrees` or `disagrees`, with
/// `preset_asked` naming what the lobby wrote when it disagrees), `sealed_this_round` (the
/// seat connection's word, present only while the rig is in the round and phase the answer
/// was served in: `Rig::sealed_this_round`), and
/// `status_failed`/`status_detail` (`refused` or `unreadable` and why, for the newest read).
fn status_facts_into(state: &mut VarDictionary, rig: &Rig) {
    let facts = rig.status_facts();
    // `""` is "not read yet", a state of its own beside every name a read can give.
    let served = match facts.preset {
        None => "",
        Some(preset) => preset.name(),
    };
    state.set(&"preset".to_variant(), &served.to_variant());
    let check = rig.preset_check();
    state.set(&"preset_check".to_variant(), &check.name().to_variant());
    let asked = match check {
        PresetCheck::Disagrees { asked, .. } => asked.name(),
        PresetCheck::NotRead
        | PresetCheck::NoMatch
        | PresetCheck::Unasked(_)
        | PresetCheck::Agrees(_) => "",
    };
    state.set(&"preset_asked".to_variant(), &asked.to_variant());
    // Only where the gateway said it: an absent key is "not told yet", never a false.
    if let Some(sealed) = rig.sealed_this_round() {
        state.set(&"sealed_this_round".to_variant(), &sealed.to_variant());
    }
    // As `failed_into` says a read's failure: `""` is "the newest read came".
    let (failed, detail) = match facts.failed.as_ref() {
        None => ("", ""),
        Some(failed) => (failed.name(), failed.detail()),
    };
    state.set(&"status_failed".to_variant(), &failed.to_variant());
    state.set(
        &"seal_not_told".to_variant(),
        &rig.seal_not_told().to_variant(),
    );
    // Why this seat's Ready did not go through (fix pass 4): the string table's
    // `ready_failed_<name>` row and its words, present only then.
    if let Some((name, said)) = rig.ready_failed() {
        state.set(&"ready_failed".to_variant(), &name.to_variant());
        state.set(&"ready_failed_detail".to_variant(), &said.to_variant());
    }
    state.set(&"status_detail".to_variant(), &detail.to_variant());
}

/// One event-list row as a dictionary: `clock`, `kind`, `text` as the feed sent them,
/// `ledger` (whether it is one of the Ledger's memos, which the lobby draws under the
/// Ledger's speaker word) and `line`, the row as an ordinary event's is drawn.
fn event_row_dictionary(row: &EventRow) -> VarDictionary {
    let mut one = VarDictionary::new();
    one.set(&"clock".to_variant(), &row.clock.to_variant());
    one.set(&"kind".to_variant(), &row.kind.to_variant());
    one.set(&"text".to_variant(), &row.text.to_variant());
    one.set(
        &"ledger".to_variant(),
        &(row.speaker == Speaker::Ledger).to_variant(),
    );
    one.set(&"line".to_variant(), &row.line().to_variant());
    one
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
    // Hit points only where the view carried them: an absent key is "draw no bar", never a
    // bar at zero (decision 5's fields, as the gateway's fog filter filled them).
    if let Some(health) = entity.hp {
        dictionary.set(&"hp".to_variant(), &i64::from(health.hp).to_variant());
        dictionary.set(
            &"max_hp".to_variant(),
            &i64::from(health.max_hp.get()).to_variant(),
        );
    }
    if let Some(heading) = entity.heading {
        dictionary.set(&"yaw".to_variant(), &yaw_of(heading).to_variant());
    }
    // A beacon's sphere, whole voxels, only where the view carried it: an absent key is "draw
    // no ring" (another seat's beacon), never a ring of a guessed size.
    if let Some(radius) = entity.ring {
        dictionary.set(&"ring".to_variant(), &i64::from(radius.get()).to_variant());
    }
    // The look only where the view carried one (a structure, a wall, a ruin): `shape` by name
    // and its size in the SIM's axes as sent; a shape this build cannot name refused the page
    // (`view.rs`). The title and the going-up flag as sent (S3's `fixq-c`).
    if let Some(look) = entity.look {
        let mut drawn = VarDictionary::new();
        drawn.set(&"shape".to_variant(), &look.shape.name().to_variant());
        let [x, y, z] = look.size.map(|edge| i64::from(edge.get()));
        for (key, edge) in [("x", x), ("y", y), ("z", z)] {
            drawn.set(&key.to_variant(), &edge.to_variant());
        }
        dictionary.set(&"look".to_variant(), &drawn.to_variant());
    }
    dictionary.set(&"title".to_variant(), &entity.title.to_variant());
    if entity.building {
        dictionary.set(&"building".to_variant(), &true.to_variant());
    }
    dictionary
}

/// A sim heading as the yaw a marker turns to, in radians about the world's up axis.
///
/// The sim's heading is 65 536 to the full turn, 0 east (+x) and 16 384 north (+y, the
/// world's +z); a marker's yaw is `atan2(dx, dz)` of the way it faces, as `vista.gd` turns
/// a marker along its last move. So a heading `h` faces `(cos t, sin t)` in the world's x
/// and z for `t = h * TAU / 65 536`, which is the yaw `PI / 2 - t`. A unit conversion for
/// drawing, which decides nothing.
fn yaw_of(heading: u16) -> f32 {
    std::f32::consts::FRAC_PI_2 - f32::from(heading) * std::f32::consts::TAU / 65_536.0
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
    int_variant(value)
}

/// One size as the `Variant` a dictionary holds.
fn count_of(value: usize) -> Variant {
    int_variant(value)
}

/// What `configure` answers when it configured nothing: `configured` and `attached` are
/// false, and `reason` says why.
fn not_configured(reason: &str) -> VarDictionary {
    let mut report = VarDictionary::new();
    report.set(&"configured".to_variant(), &false.to_variant());
    report.set(&"attached".to_variant(), &false.to_variant());
    report.set(&"reason".to_variant(), &reason.to_variant());
    report
}

/// Whether every speed in `speeds` fits Godot's `PackedInt32Array`, for the compile-time
/// proof below.
const fn all_fit_i32(speeds: &[u32]) -> bool {
    let mut rest = speeds;
    while let [first, tail @ ..] = rest {
        if *first > i32::MAX as u32 {
            return false;
        }
        rest = tail;
    }
    true
}

/// `watch_speeds` hands each speed over as an `i32`, losslessly.
const _: () = assert!(all_fit_i32(&crate::pacer::SPEEDS));

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
        &counter(parsed.budget.bytes_per_frame),
    );
    dictionary.set(
        &"age_frames".to_variant(),
        &counter(parsed.budget.age_frames),
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
        dictionary.set(&"vertices".to_variant(), &counter(self.vertices));
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
                if let (Some(more_vertices), Some(more_indices)) = (
                    vertices.checked_add(surface.vertex_count() as u64),
                    indices.checked_add(surface.indices().len() as u64),
                ) {
                    vertices = more_vertices;
                    indices = more_indices;
                } else {
                    failures.push(format!(
                        "chunk {case}: {}",
                        BridgeError::CountOverflow {
                            what: "self-check vertices and indices"
                        }
                    ));
                    geometry_identical = false;
                }
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
        chunks: CASES_U32,
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
const CASES_U32: u32 = 4;

/// [`CASES_U32`] as a count of slots: a `u32` widens to a `usize` on every target the
/// client builds for (`view::chunk_slot`'s proof).
const CASES: usize = view::chunk_slot(CASES_U32);

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
            ..Default::default()
        }),
        ..GetStatusResponse::default()
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
                // A shift rather than `/`: `integer_division` stays denied inside a
                // walled crate too (AGENTS.md section 4.9), because being explicit about
                // rounding is not a determinism concern that the wall lifts. `x >> 3` is
                // `x / 8` rounded down, and cannot fail.
                let step = x >> 3;
                let height = 8 + (case + step) % 6;
                let bitten = pass == 1
                    && (12..20).contains(&x)
                    && (12..20).contains(&y)
                    && z.saturating_add(2) >= height;
                let solid = z < height && !bitten;
                if solid && let Some(slot) = sim_order.get_mut(chunks::sim_voxel_index(x, y, z)) {
                    // `case % 3` is below 3, so the material id is 1 to 3 and the cast
                    // cannot truncate.
                    *slot = 1 + (case % 3) as u8;
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
        light
            .chunk_light(0)
            .ok_or_else(|| BridgeError::Schema {
                pointer: String::new(),
                message: "the self-check map has no chunk 0".to_owned(),
            })?
            .to_vec(),
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

/// A rule-list ask the bridge could not name in Rust's types: refused through the editor, by
/// its reason on the status line, and answered `false` to GDScript.
fn refused(editor: &mut crate::editor::Editor, refusal: RuleEditRefusal) -> bool {
    let _ = editor.refuse_rule_edit(refusal);
    false
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
