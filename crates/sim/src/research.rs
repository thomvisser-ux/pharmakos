// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `fork` — **research builds only**.
//!
//! The whole module sits behind `#[cfg(feature = "research")]` at its `mod`
//! declaration in `lib.rs`, not merely on its functions, so a default build
//! does not compile a line of this file and no `fork` symbol reaches the
//! binary.
//!
//! `pharmakos-sim` is the **sole definer** of the `research` feature
//! (AGENTS.md §3 rule 1). Release builds never enable it; CI builds both
//! configurations so neither rots; and `plan-core`, `verifier`, `operator` and
//! `gateway` may never reach it — not in `[dependencies]`, not in
//! `[dev-dependencies]`, not through a default feature, not transitively.
//! `cargo xtask ci`'s `research-guard` step walks `cargo tree -e features` and
//! fails the build if it does.
//!
//! **"No dry runs" is a hard guarantee in shipped builds** (AGENTS.md §3
//! rule 2). Snapshot and restore are in the core and are what saves and replays
//! use; this is not that.

use crate::encoding::Enc;
use crate::world::World;

/// Fork the world. The child is an independent timeline from this tick on.
#[must_use]
pub fn fork(parent: &World) -> World {
    parent.clone()
}

/// Fork, step the child `ticks` ticks, and return the child's hash trace.
///
/// The parent is taken by shared reference and cannot be perturbed — which is
/// the property G4 asserted 20 times out of 20, with 10 of 10 parents
/// unperturbed, and which `tests/fork.rs` re-asserts against the real world.
#[must_use]
pub fn fork_and_step(parent: &World, ticks: u32) -> (World, Vec<u64>) {
    let mut child = fork(parent);
    let mut enc = Enc::with_capacity(16 * 1024);
    let mut trace = Vec::with_capacity(usize::try_from(ticks).unwrap_or(0));
    let mut n: u32 = 0;
    while n < ticks {
        trace.push(child.step(&mut enc));
        n = n.saturating_add(1);
    }
    (child, trace)
}
