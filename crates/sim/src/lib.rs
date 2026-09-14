// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The deterministic simulation: the authoritative world and the only thing that advances
//! time. Role from spec §15 (Architecture), simulation layer.
//!
//! # What lives here
//!
//! * **Deterministic sim** — fixed 20 Hz tick, integer maths, structure-of-arrays tables.
//! * **World** — 32³ copy-on-write chunks with a 64-layer height cap, plus pathing and ETA
//!   (our own HPA*, not `hierarchical_pathfinding`).
//! * **Runner** — the Lull / Push / recap cycle, the playbook interpreter, the built-in
//!   mandates and programs, the Quartermaster and the power grid.
//! * **Replay** — the private deterministic replay (seed + playbooks + log) and the
//!   shareable recording (keyframes plus state deltas and events), with a per-tick xxh3
//!   state hash. Snapshot and restore are in the core.
//!
//! # Rules this crate is held to
//!
//! * Integer-first maths: position `Q16.16`, squared distance `Q32.32` (range checks
//!   compare r², never a square root), `u16` angles with a 4096-entry lookup table,
//!   integer HP and $. Floats appear only in a walled presentation/solve module.
//! * Determinism: split seeded RNG streams, ordered collections, no `HashMap`/`HashSet`,
//!   no wall-clock time, overflow checks on in every profile. Cross-OS hash equality is a
//!   CI gate. Per-seat integer kill-credit counters and their largest-remainder
//!   apportionment (ties to the lowest seat id) are hashed state like everything else.
//! * **`fork` is `research`-only.** This crate is the sole definer of the `research`
//!   feature. Release builds never enable it; CI builds both configurations; and the
//!   plan-core, verifier, operator and gateway crates may never depend on it, so the
//!   "no dry runs" guarantee holds in every shipped build.
//!
//! Nothing is implemented yet — the crate is a placeholder until the G4 (determinism,
//! save/restore, fork equivalence) and G1 (mesher) spikes land.

#[cfg(feature = "research")]
pub mod research {
    //! Speculative stepping (`fork`) for the G4 spike and research builds.
    //!
    //! Compiled only under the `research` feature, which release builds never enable.
    //! Nothing outside this module — and no other crate in the workspace — may expose it.
}
