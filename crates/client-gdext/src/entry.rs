// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `GDExtension` entry point.
//!
//! One `unsafe impl`, and the symbol Godot looks for. `godot/pharmakos.gdextension` names
//! it — `entry_symbol = "gdext_rust_init"` (decisions log item 73) — and
//! `cargo xtask stage-client` byte-scans the staged library for that string before it
//! declares success, because a staged library built without gdext reports "Can't resolve
//! symbol `gdext_rust_init`, error 127", which reads like an ABI problem and is not one
//! (G1 section 10.12, "Two builds, two target directories").

use godot::prelude::{ExtensionLibrary, gdextension};

/// The extension itself. Registering it is what makes [`crate::PharmakosBridge`]
/// instantiable from a scene.
struct Pharmakos;

// SAFETY: the contract gdext states for this trait is that the library is loaded by
// Godot through the generated entry point and by nothing else, which is what
// `godot/pharmakos.gdextension` arranges. See the crate-level comment in lib.rs for why
// the workspace's `unsafe_code = "deny"` is lifted for this crate alone.
#[gdextension]
unsafe impl ExtensionLibrary for Pharmakos {}
