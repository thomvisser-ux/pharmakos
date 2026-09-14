// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Plan core: everything done *to* a playbook that is not simulating it. Role from spec
//! §15 (Architecture), gateway layer — it runs in process with the Seat Gateway.
//!
//! # What lives here
//!
//! * **verify** — drives the verifier pipeline and returns its report.
//! * **estimate** — travel estimates over known terrain, interface-time arithmetic, and
//!   the $ and kW projection, so the editor's segment clock and the gateway agree.
//! * **render** — deterministic template prose: the plain-language reading of a playbook
//!   that the editor shows and `gamectl` prints.
//! * **patch** — JSON Patch application with inverse-patch undo; comments survive a
//!   load-and-save unchanged.
//! * **canonicalise** — the authoritative canonical form of a playbook. Golden tests
//!   byte-compare the output, so the editor's round-trip cannot drift.
//!
//! Playbooks are JSONC on disk: canonical proto JSON plus comments. Protobuf is the
//! single schema source (`gp.v1`, `gp.api.v1`).
//!
//! # Rules this crate is held to
//!
//! * **No dry runs.** Estimates may use the pathfinder, interface-time arithmetic,
//!   Quartermaster projection and placement legality. They may never step or fork the
//!   simulation, run mandates, programs, combat or construction, model an opponent, or
//!   evaluate rule conditions over a projected future.
//! * Consequently this crate **may never depend on the `research` feature** of
//!   `pharmakos-sim`, which is what gates `fork`. Only `crates/sim` defines that feature;
//!   depend on the sim with `default-features = false` and never name it. `cargo xtask ci`
//!   enforces this.
//! * Determinism: integer maths, ordered collections, no wall-clock time, no
//!   `HashMap`/`HashSet`, no float arithmetic.
//!
//! Nothing is implemented yet — placeholder until the walking skeleton.
