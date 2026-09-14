// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The built-in operator. Role from spec §14, listed in spec §15 (Architecture) among the
//! gateway's clients: it runs in process and uses the same traits as everything else.
//!
//! Every commander is run by this operator. It does three jobs:
//!
//! 1. **Generate** a playbook with the rudimentary tool — templates plus utility scoring.
//! 2. **File the safe playbook** when a seat submits nothing verified before the Lull ends.
//! 3. **Execute** whatever playbook the seat sealed — the human's edited playbook and its
//!    own alike.
//!
//! A computer opponent is this operator with no human editing it, plus the built-in
//! mandates on its beacons and the built-in programs in its units and buildings.
//!
//! # It is an ordinary client
//!
//! Same snapshot, same verifier, same submit path, no privileged reads (spec §12, §14).
//! It reads the briefing, beacons, known enemies, economy and capabilities; computes
//! integer situation features (threat, power headroom, vent sites, expansion sites,
//! capability and attack opportunities, commander risk); fills template parameters with
//! the top-k options; composes visit goals by greedy insertion by utility per second until
//! the route fills its target share of the segment; scores defence + economy + expansion +
//! capability + attack − risk; adds the standard rules; then runs verify-and-repair up to
//! four times.
//!
//! Determinism: its seed comes from the match, seat and round, and its budget is measured
//! in evaluation units rather than wall time.
//!
//! v1 ships the Balanced weighting only, at Easy / Normal / Hard breadth. **No lookahead
//! at any level** — Hard differs only in breadth and rules. It ignores the seat notebook,
//! and on a built-in seat it emits one-way engine chatter; it never reads chatter.
//!
//! # Rules this crate is held to
//!
//! * **No dry runs**, so **no `research` feature**: only `crates/sim` defines it, this
//!   crate may never enable or transitively reach it, and `cargo xtask ci` enforces that.
//! * Deterministic like the sim: integer maths, seeded RNG streams, ordered collections,
//!   no wall-clock time, no float arithmetic.
//!
//! Nothing is implemented yet — placeholder. Easy arrives with the walking skeleton;
//! Normal and Hard are built in stage S5.
