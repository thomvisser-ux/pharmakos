// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The verifier — seal inspection. Role from spec §11, sitting in the gateway layer of
//! spec §15 (Architecture). Every seat passes through it: the human's playbook and the
//! built-in operator's are checked by the same code on the same snapshot.
//!
//! # Pipeline
//!
//! `decode → structure → resolve → semantics` (together **QUICK**, ≤ 5 ms p99, run on
//! every edit) `→ estimate → lint` (**FULL**, ≤ 50 ms p99 at the size budget, always run
//! on submit).
//!
//! `verify_plan` takes `depth: quick | full` and defaults to `full`; submit always runs
//! FULL.
//!
//! # Determinism
//!
//! A report is a pure function of exactly five inputs — playbook bytes, snapshot, rules
//! hash, verifier version and depth — so a pre-check and the check at submit are
//! byte-identical, which is what `report_hash` proves. Two reports compare only within
//! one depth.
//!
//! # Diagnostics
//!
//! Modelled on rustc's JSON output: a code, a severity, a JSON Pointer path, related
//! paths, map references, a precise message, a plain-language beginner sentence, and JSON
//! Patch suggestions labelled with how safely they apply. Codes are language-neutral;
//! every user-facing string in v1 is English and lives in one string table. About 35 codes
//! in these families: decode and version (`E000x`), limits and required blocks (`E01xx`),
//! conditions (`E02xx`), control flow (`E03xx`), references and placement (`E04xx`),
//! settings and recycle (`E05xx`), economy, power and messaging (`E06xx`), schedule,
//! conflicts and staleness (`W07xx`), information (`I…`).
//!
//! An out-of-vocabulary construct is rejected with a code and a JSON Pointer to the
//! offending node — never silently stripped.
//!
//! # Rules this crate is held to
//!
//! * **No dry runs**, and therefore **no `research` feature**: only `crates/sim` defines
//!   it, this crate may never enable or transitively reach it, and `cargo xtask ci`
//!   enforces that. Allowed work is pathfinder travel estimates, interface-time
//!   arithmetic, $ and kW projection, placement legality, selector previews and mast
//!   coverage. Forbidden is stepping or forking the sim, running mandates, programs,
//!   combat or construction, modelling enemy behaviour, or evaluating rule conditions over
//!   a projected future.
//! * Integer maths, ordered collections, no wall-clock time, no float arithmetic: the
//!   verifier is deterministic like the sim.
//!
//! Nothing is implemented yet — placeholder until the walking skeleton.
