// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The walled timing harness for P1, the depth task of S1 (S1's plan, section 5
//! and task `p1`; decisions-log items 33 (c), 116 (6) (b) and 128 (3) (b)), and
//! for G3′-real, the depth task of S2 (S2's plan, section 5 and task `g3h`).
//!
//! Spec section 16's P1 and G3′ rows ask wall-clock questions no other crate
//! may answer. The verifier, the sim and `gamectl` read no clock (AGENTS.md
//! section 4.5); `client-gdext` may not reach the verifier and the mesher may
//! not reach the sim (`CLIENT_WALL` in `xtask/src/main.rs`); and `xtask` has no
//! dependencies. So this crate exists, walled, to drive them from outside:
//!
//! | Module | Question |
//! |---|---|
//! | [`quick`] | QUICK's p50 and p99 over the committed verifier cases and `budget_128` |
//! | [`decision`] | A decision tick's wall-clock cost per size unit, in a hosted match at the size budget |
//! | [`g3`] | G3′-real: the tick at the gate's load, phase by phase, over an 8-minute segment, and the figures the power budget needs |
//! | [`hosted`] | A bench match, hosted as `gamectl scenario run` hosts one, the gate's layout placed in its lobby |
//! | [`heap`] | What a counting global allocator tells the G3′ segment |
//! | [`stats`] | Nearest-rank percentiles, means, and how a duration is written |
//! | [`error`] | The one error type: every way a measurement can fail to measure |
//!
//! # The wall, and what it allows here
//!
//! `bench` is in `WALLED_PACKAGES`, so `cargo xtask clippy` lints it in pass 2
//! with the clock (and float, cast and hash-map) allowance passed on the
//! command line, and `cargo xtask wall-guard` fails the build if any
//! deterministic crate depends on it. It is also in `GUARDED_PACKAGES`, so the
//! research guard fails the build if it ever reaches the sim's `research`
//! feature. It takes no `CLIENT_WALL` line: that list names what a walled crate
//! may never reach, and this one exists to reach the sim, the verifier and the
//! gateway, as section 4.5's walled harness does.
//!
//! Two planks of AGENTS.md section 4.9 hold inside it: it writes no sim state
//! itself (it reads the world, and drives a match through the gateway's `Host` and
//! `Surface`, the way `gamectl scenario run` does), and nothing it measures
//! crosses back into the sim. The match's inputs it gives are a host's: the
//! playbooks through `submit_plan`, the gate's layout through the lobby seam
//! `Host::seed_layout`, and the crater stream and the support construct
//! through the filing seam `Host::file_voxel_edit` (S2's plan, E5 and E30),
//! each between ticks; it times a tick from outside through the phase stepper
//! (`Surface::phased_step`), which hands control back between phases. One
//! line drives the sim's own `Runner` rather than the gateway, because the
//! gateway refuses the fourth seat it measures ([`g3::fourth`]; S2's plan,
//! E19 and decision 2). Nothing here uses a float either; the allowance it
//! needs is the clock.
//!
//! # Where the figures go
//!
//! `cargo xtask perf-alarms` runs `tests/perf_alarm.rs` and `tests/g3_real.rs`
//! with `--release`, `--ignored` and `--nocapture`, which print each figure as
//! a `::notice::`
//! per runner, with no threshold and never compared across operating systems
//! (decisions-log item 116 (6) (b)): S1 publishes P1 and never gates on it
//! (item 128 (3) (b)). The certifying run is the demo's, on the owner's
//! machine, after the lanes that change verifier and decision cost have
//! merged (S1's plan, task `demo`).
//!
//! Never shipped: `publish = false`, and `cargo xtask package` builds the
//! client library and `gamectl` alone.

pub mod decision;
pub mod error;
pub mod g3;
pub mod heap;
pub mod hosted;
pub mod quick;
pub mod stats;

pub use error::Error;

use std::path::PathBuf;

/// The workspace root: this crate is `<root>/crates/bench`.
#[must_use]
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}
