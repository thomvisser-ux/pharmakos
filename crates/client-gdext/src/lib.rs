// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The gdext bridge to the Godot 4.7 client. Role from spec §15 (Architecture), clients
//! layer: "Godot 4.7 client — GDScript views and playbook editor; thin gdext crate".
//!
//! # What lives here
//!
//! The narrow seam between Rust and Godot, and nothing else:
//!
//! * Mesh uploads — our own greedy mesher in Rust feeds Godot `RenderingServer` RIDs under
//!   a per-frame upload budget. The same mesher handles terrain and `.vox` models loaded
//!   with `dot_vox`, so no Godot importer addons are needed and Voxel Tools stays optional.
//!   **The mesher itself now lives in `pharmakos-mesher`** (decisions log §2.7 item 56), a
//!   walled crate that links without gdext; this crate only marshals its buffers — it
//!   applies the per-frame upload budget and hands the vertex, colour and index arrays to
//!   the `RenderingServer`, and it decides nothing about the geometry. This crate is the
//!   mesher's only dependant today; what `cargo xtask wall-guard` checks on every run is
//!   the half that matters — that no deterministic crate, the sim included, ever becomes
//!   one.
//! * Gateway and plan-core calls the editor needs — the plan model, the verifier and
//!   travel-time queries are reached through here.
//! * The watch rig's data: own-fog camera, event list, speed and skip, and the full map on
//!   elimination or match end.
//!
//! # What does not live here
//!
//! Decisions. GDScript is views and editor only; this crate marshals between them and the
//! Rust core. No game rule, no verification, no time maths is implemented on either side
//! of this seam — the editor runs none of its own, and neither does the bridge.
//!
//! Like every crate outside `crates/sim`, it must never enable or transitively reach the
//! `research` feature that gates `fork`.
//!
//! Nothing is implemented yet. Spike G1 (mesher and remesh in Godot 4.7) is closed — its
//! results are in `docs/spikes/G1-destruction-remesh.md` §10 and its decisions are items
//! 52–56 — and the bridge is written against the real types at the walking skeleton's
//! vista task. The `.gdextension` file and the Godot project itself live outside this
//! crate and are not written yet.
//!
//! One G1 lesson lands here rather than in the mesher: **gdext's panic catch at the
//! `#[func]` boundary is silent**, turning a panic into a default return value and a
//! healthy-looking frame. Count caught panics at the boundary and report them, rather than
//! trusting the boundary to be loud (G1 §10.12).
