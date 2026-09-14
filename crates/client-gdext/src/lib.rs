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
//! Nothing is implemented yet — placeholder until the G1 spike (mesher and remesh in
//! Godot 4.7). The `.gdextension` file and the Godot project itself live outside this
//! crate and are not written yet.
