// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! P1's wall-clock figures, published as notices and never gated.
//!
//! S1's plan section 5 and task `p1`: QUICK's p50 and p99 over the committed
//! verifier cases and `budget_128`, and a decision tick's cost in ns per size
//! unit in a hosted match whose playbooks sit at the size budget, each a
//! `::notice::` per runner with **no threshold**, never compared across
//! operating systems (decisions-log item 116 (6) (b)), never a CI gate in S1
//! (item 128 (3) (b)). The certifying run is the demo's, on the owner's
//! machine, and records these same lines.
//!
//! Both tests are `#[ignore]`d, so `cargo test --workspace` does not pay for
//! them. `cargo xtask perf-alarms` runs them on every runner as
//!
//! ```sh
//! cargo test --release -p pharmakos-bench --test perf_alarm -- --ignored --nocapture --test-threads=1
//! ```
//!
//! (with `--locked` in CI). **`--release` is not optional**, for the mesher
//! alarm's reason: a debug build's overflow, bounds and inlining make its
//! figure a different number, not a slow version of this one. And
//! **`--test-threads=1`**, so the two measurements never share the machine
//! with each other.
//!
//! Wall-clock time is legal here: the crate is walled (`WALLED_PACKAGES` in
//! `xtask/src/main.rs`), and nothing timed here reaches the sim.

use pharmakos_bench::decision::{self, BENCH_SEATS, BENCH_SEED, DecisionPlan};
use pharmakos_bench::quick::{self, Fixture, QuickPlan};
use pharmakos_bench::workspace_root;

/// Untimed QUICK calls per case before its sample.
const QUICK_WARMUP: usize = 20;

/// Timed QUICK calls per case. Two hundred calls make a case's own p99 its
/// second-slowest call, and the pool's p99 is taken over every case's calls.
const QUICK_CALLS: usize = 200;

/// The bench match's one segment: the spec's shortest Push, three minutes
/// (3 600 ticks, 720 decision ticks).
const SEGMENT_MS: i32 = 180_000;

/// How many times the bench match is played, each tick keeping its fastest.
const RUNS: usize = 3;

#[test]
#[ignore = "a performance alarm, not a gate: cargo xtask perf-alarms runs it with --ignored"]
fn quick_p50_and_p99_over_the_committed_cases() {
    let root = workspace_root();
    let fixture = Fixture::committed(&root).unwrap_or_else(|error| panic!("{error}"));
    let cases = quick::committed_cases(&root).unwrap_or_else(|error| panic!("{error}"));
    let timing = quick::measure(
        &fixture,
        &cases,
        QuickPlan {
            warmup: QUICK_WARMUP,
            calls: QUICK_CALLS,
        },
    )
    .unwrap_or_else(|error| panic!("{error}"));
    for line in quick::notices(&timing, std::env::consts::OS) {
        println!("{line}");
    }
    assert_eq!(timing.pooled.samples, cases.len() * QUICK_CALLS);
}

#[test]
#[ignore = "a performance alarm, not a gate: cargo xtask perf-alarms runs it with --ignored"]
fn decision_tick_ns_per_size_unit_at_the_budget() {
    let root = workspace_root();
    let rules = quick::committed_rules(&root).unwrap_or_else(|error| panic!("{error}"));
    let timing = decision::measure(
        &root,
        &rules,
        DecisionPlan {
            seed: BENCH_SEED,
            seats: BENCH_SEATS,
            segment_ms: SEGMENT_MS,
            runs: RUNS,
        },
    )
    .unwrap_or_else(|error| panic!("{error}"));
    println!("{}", decision::notice(&timing, std::env::consts::OS));
    assert!(timing.windows > 0);
}
