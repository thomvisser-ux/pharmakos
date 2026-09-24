// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// gdext registers a GDExtension by implementing an `unsafe` trait — `unsafe impl
// ExtensionLibrary for Pharmakos` in `entry` below — and its `#[godot_api]` macro expands
// to code that needs the same permission. `unsafe_code` is denied for the whole workspace
// in Cargo.toml's `[workspace.lints.rust]`, so one of the two has to give.
//
// Decisions log section 2.7 item 83 chose THIS shape over a per-crate `[lints]` override:
// a crate-local allow, in `crates/client-gdext` only, with a comment naming gdext as the
// reason, raised in T12's pull request rather than edited into the contract lint table.
// The crate is walled and `cargo xtask wall-guard` proves no deterministic crate depends
// on it.
//
// **The precedent is this and nothing wider.** AGENTS.md section 4.9 warns about exactly
// this shape for DETERMINISM lints — float arithmetic, `as` casts, hash maps, clocks —
// where an inner allow would be invisible to CI and would open a determinism hole.
// `unsafe_code` is not one of those: it has no bearing on the hash chain, the wall lifts
// none of the determinism lints in this file, and `cargo xtask clippy` pass 2 still lints
// this crate with `-D warnings`, `indexing_slicing`, `unwrap_used` and `integer_division`.
// Nobody should read this line as licence to allow anything else here.
#![allow(
    unsafe_code,
    reason = "gdext's ExtensionLibrary impl and #[godot_api] expansion; decisions log \
              section 2.7 item 83"
)]

//! The thin gdext bridge between the Rust core and the Godot 4.7 client.
//!
//! Role from spec section 15 (Architecture), clients layer: "Godot 4.7 client — GDScript
//! views and playbook editor; thin gdext crate". **Thin** is the whole specification:
//! this crate marshals, and it decides nothing (AGENTS.md section 3 rule 4). No game
//! rule, no verification, no `$`, no `kW`, no time arithmetic lives on either side of
//! this seam — the editor runs none of its own and asks the gateway, and the bridge does
//! not do for it what it is forbidden to do for itself. `tests/no_arithmetic.rs` reads
//! this crate's own source and says so.
//!
//! # What crosses, and in which direction
//!
//! | in | out | module |
//! |---|---|---|
//! | sim-order chunk bytes | mesher-order chunk bytes | [`chunks`] |
//! | a `gp.v1.RulesTable` | the mesher's `DrainBudget` and `LightParams` | [`rules`] |
//! | `MeshBuffers` | an [`UploadOp`](upload::UploadOp) the engine executes | [`surface`], [`upload`] |
//! | a `gp.api.v1` result as JSON | canonical JSON, schema-checked | [`api`] |
//! | a `get_view` result | chunks on the drain queue, the entity list | [`view`] |
//! | a gateway answer | the next JSON-RPC frame for each connection | [`rig`] |
//! | a click on the map, a Load, a Fix | the editor's next planning call, and its rows | [`editor`] |
//! | the wall clock | game milliseconds asked for, the host clock reported | [`pacer`] |
//!
//! Each of those is a copy or a lookup. The places where something is *decided* are all
//! presentation or connection plumbing that the sim must never see: which frame a chunk is
//! uploaded on (the mesher's own `DrainQueue`, item 54); whether a surface can be patched
//! in place rather than rebuilt (item 53's guards, [`upload`]); the order the editor's
//! planning calls go out in ([`editor`]); how many calls the seat connection spends
//! between two clock reports, and Ready waiting behind a submission still in flight
//! ([`rig`]); and when FULL is owed after 600 ms idle ([`pacer::IdleTimer`]). None of them
//! is a rule of the game, a verdict, a price or a travel time; those are the gateway's.
//!
//! # Why this crate is behind the wall
//!
//! Vertex positions are `f32` and `godot-core` pulls `glam`, so floats, `as` casts, hash
//! maps and clocks are legal here and nowhere in the deterministic crates. That allowance
//! is a crate boundary, not an attribute: `cargo xtask clippy` pass 2 passes it on the
//! command line, and `cargo xtask wall-guard` fails the build if `sim`, `plan-core`,
//! `verifier`, `operator` or `gateway` ever depends on this crate (AGENTS.md section 4.9).
//! Nothing in this source asks for one of those allowances, and the one `#![allow]` above
//! is `unsafe_code`, which is not among them.
//!
//! # Two things the bridge counts, because neither is loud
//!
//! * **Caught panics.** gdext's catch at the `#[func]` boundary turns a panic into a
//!   default return value and a healthy-looking frame (G1 section 10.12). [`panics`]
//!   counts its own catches, the self-check reports the count, and CI asserts it is zero.
//! * **Why each upload took the branch it took.** Path B's in-place write is only legal
//!   when the index array is unchanged and the probed layout is the one being written; the
//!   counters say how often each guard fired, so "the fast path is working" is a number.
//!
//! # The watch rig (T16)
//!
//! The client meets the real gateway here, as the plan's T16 amendment describes
//! (`docs/design/skeleton-plan-t16a-notes.md` section A): the lobby spawns `gamectl host`
//! and holds two WebSocket connections and their tokens in GDScript; [`rig`] decides what
//! each connection sends and reads what comes back; [`view`] turns `get_view` results into
//! chunks on the drain queue; and [`pacer`] is the one wall clock — the pacer that asks for
//! game milliseconds, the host clock reported outside a Push, and the keep-alive. The pacer
//! is **presentation pacing on the walled side, like the Lull timer, and not game-rule time
//! arithmetic**: it never sees a step of the sim or a hash, and `tests/no_arithmetic.rs`
//! still holds every other module of this crate to "no time maths".
//!
//! # The editor (T19, pull request 1)
//!
//! The playbook editor is one more client of the gateway on the seat connection the vista
//! already holds: [`editor`] turns a click on the map into a JSON Patch and each answer into
//! validation rows, a priced route and a placement ghost, and [`rig`] fits its calls into the
//! seat token's rate budget beside the vista's polls. Every verdict, travel time and patch
//! is the gateway's; the editor's one timer, FULL after 600 ms idle, is the pacer's
//! ([`pacer::IdleTimer`]).
//!
//! # What is not here yet
//!
//! T19's pull request 2: the template wizard with `instantiate_template`'s suggestions,
//! the prose rule list, the `$`/`kW` meter and the lobby's Resume. The `.vox` models:
//! entities are drawn as placeholder primitives by GDScript until S6, and `dot_vox` arrives
//! with the models rather than here (decisions log item 101).

pub mod api;
pub mod chunks;
pub mod editor;
pub mod enums;
pub mod error;
pub mod pacer;
pub mod panics;
pub mod rig;
pub mod rules;
pub mod surface;
pub mod upload;
pub mod view;

mod bridge;
mod editor_view;
mod engine;
mod entry;
mod variant;

pub use crate::bridge::{PharmakosBridge, SelfCheck, self_check};
pub use crate::error::BridgeError;

/// The class name the `.gdextension` and the scenes refer to.
///
/// Kept beside the type rather than only in the `#[class]` attribute so that
/// `tests/godot_project.rs` can check `godot/` and this crate still agree about it — a
/// renamed class is otherwise a run-time failure inside Godot, and a silent one on a
/// fresh checkout where the classes instantiate as placeholders anyway.
pub const BRIDGE_CLASS_NAME: &str = "PharmakosBridge";
