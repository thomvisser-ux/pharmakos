// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `fork` — research build only.
//!
//! The whole module is behind `#[cfg(feature = "research")]` at its `mod`
//! declaration in `lib.rs`, not merely on the functions, so a default build does
//! not compile a line of this file and no `fork` symbol reaches the binary.
//!
//! A fork is a full logical copy of the world that can be stepped without
//! touching the parent. The SoA tables are deep-copied; the kill-credit table is
//! an `imbl::OrdMap`, so it is structurally shared until one side writes.

use crate::World;

/// Logical size of a world's heap, in bytes. Not a measurement of the
/// allocator — a count of what a fork must be able to diverge on, which is the
/// number the research build cares about.
#[must_use]
pub fn heap_bytes(w: &World) -> usize {
    let u = w.units.len();
    let b = w.beacons.len();
    let per_unit = 4 + 1 + 12 + 2 + 4 + 8 + 2; // id, seat, pos, heading, hp, Option<u32>, cooldown
    let per_beacon = 4 + 1 + 12 + 4 + 8 + 4;
    let per_seat = 1 + 8 + 4 + 4;
    let mut credit_bytes = 0usize;
    for (_, c) in w.credits.iter() {
        credit_bytes += 4 + 1 + c.entries.len() * 5;
    }
    u * per_unit + b * per_beacon + w.seats.len() * per_seat + credit_bytes
}

/// Fork the world. The child is an independent timeline from this tick on.
#[must_use]
pub fn fork(parent: &World) -> World {
    parent.clone()
}

/// Fork, step the child `n` ticks, and return the child's hash trace for those
/// ticks. The parent is taken by shared reference and cannot be perturbed.
#[must_use]
pub fn fork_and_step(parent: &World, n: u32) -> (World, Vec<u64>) {
    let mut child = fork(parent);
    let mut enc = crate::hash::Enc::with_capacity(16 * 1024);
    let mut trace = Vec::with_capacity(usize::try_from(n).expect("child tick count"));
    for _ in 0..n {
        child.step();
        child.encode(&mut enc);
        trace.push(enc.finish());
    }
    (child, trace)
}
