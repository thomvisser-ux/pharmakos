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
//! The test is `#[ignore]`d, so `cargo test --workspace` does not pay for it.
//! `cargo xtask perf-alarms` runs it on every runner as
//!
//! ```sh
//! cargo test --release -p pharmakos-bench --test perf_alarm -- --ignored --nocapture
//! ```
//!
//! (with `--locked` in CI). **`--release` is not optional**, for the mesher
//! alarm's reason: a debug build's overflow, bounds and inlining make its
//! figure a different number, not a slow version of this one.
//!
//! The two measurements are **one test**, run one after the other, so they
//! never share the machine with each other whatever thread count libtest
//! picks. One that cannot measure prints a `::warning::` and the other still
//! runs; the test fails at the end if either did. The test prints an empty
//! line before its first notice: libtest run on one thread prints
//! `test <name> ... ` before the test starts, and a workflow command that does
//! not start its line is not read as one.
//!
//! Wall-clock time is legal here: the crate is walled (`WALLED_PACKAGES` in
//! `xtask/src/main.rs`), and nothing timed here reaches the sim.

use std::path::Path;

use pharmakos_bench::decision::{self, BENCH_SEATS, BENCH_SEED, DecisionPlan};
use pharmakos_bench::quick::{self, Fixture, QuickPlan};
use pharmakos_bench::workspace_root;

/// Untimed QUICK calls per case before its sample.
const QUICK_WARMUP: usize = 20;

/// Timed QUICK calls per case. Two hundred calls make a case's own p99 its
/// second-slowest call, and the pool's p99 is taken over every case's calls.
const QUICK_CALLS: usize = 200;

/// How many times the bench match is played, each tick keeping its fastest.
const RUNS: usize = 3;

#[test]
#[ignore = "a performance alarm, not a gate: cargo xtask perf-alarms runs it with --ignored"]
fn p1_quick_and_decision_tick_timings() {
    let root = workspace_root();
    let os = std::env::consts::OS;
    // A notice must start its line; see the module docs.
    println!();
    let mut failed = Vec::new();
    match quick_notices(&root, os) {
        Ok(lines) => lines.iter().for_each(|line| println!("{line}")),
        Err(error) => {
            println!("::warning title=QUICK p99::not measured on {os}: {error}");
            failed.push(error);
        }
    }
    match decision_notice(&root, os) {
        Ok(line) => println!("{line}"),
        Err(error) => {
            println!("::warning title=decision tick per size unit::not measured on {os}: {error}");
            failed.push(error);
        }
    }
    assert!(failed.is_empty(), "{failed:?}");
}

/// QUICK's p50 and p99 over the committed cases, as notices.
fn quick_notices(root: &Path, os: &str) -> Result<Vec<String>, String> {
    let fixture = Fixture::committed(root)?;
    let cases = quick::committed_cases(root)?;
    let timing = quick::measure(
        &fixture,
        &cases,
        QuickPlan {
            warmup: QUICK_WARMUP,
            calls: QUICK_CALLS,
        },
    )?;
    if timing.pooled.samples != cases.len() * QUICK_CALLS {
        return Err(format!(
            "{} samples pooled, expected {}",
            timing.pooled.samples,
            cases.len() * QUICK_CALLS
        ));
    }
    Ok(quick::notices(&timing, os))
}

/// The decision tick's ns per size unit, as a notice. The bench match's one
/// segment is the rules table's first, the shortest Push of the ladder.
fn decision_notice(root: &Path, os: &str) -> Result<String, String> {
    let rules = quick::committed_rules(root)?;
    let segment_ms = *rules
        .segment_lengths_ms()
        .first()
        .ok_or("the rules table has no match.segment_lengths_ms")?;
    let timing = decision::measure(
        root,
        &rules,
        DecisionPlan {
            seed: BENCH_SEED,
            seats: BENCH_SEATS,
            segment_ms,
            runs: RUNS,
        },
    )?;
    if timing.windows == 0 {
        return Err("no decision tick was measured".to_owned());
    }
    Ok(decision::notice(&timing, os))
}
