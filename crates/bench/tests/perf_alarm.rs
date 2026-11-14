// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! P1's wall-clock figures, published as notices and never gated, and the one
//! report line `cargo xtask p1-budget` reads.
//!
//! S1's plan section 5 and task `p1`: QUICK's p50 and p99 over the committed
//! verifier cases and `budget_128`, and a decision tick's cost in ns per size
//! unit in a hosted match whose playbooks sit at the size budget. S3's plan
//! section 5.2 and task `bench3` add P1 FULL: FULL over the same cases, both
//! depths over the generated shapes at each size of E24's curve, and the
//! decision tick's cost at each size, in nanoseconds and in counted
//! evaluation units. Each figure is a `::notice::` per runner with **no
//! threshold**, never compared across operating systems (decisions-log item
//! 116 (6) (b)). The certifying run is on the owner's machine, and records
//! these same lines (S3's `cert3`).
//!
//! The binary holds three `#[ignore]`d tests, so `cargo test --workspace`
//! pays for none of them, and each is run **alone, by its exact name**, so no
//! two share the machine:
//!
//! * `p1_quick_and_full_timings`: `cargo xtask perf-alarms p1`, at most ten
//!   notices (GitHub annotates only a step's first ten, item 149 (4)): QUICK's
//!   two, FULL's over the cases, the shapes' provenance and one per shape;
//! * `p1_decision_tick_timings`: `cargo xtask perf-alarms p1-decision`, the
//!   decision tick at the size budget and at each size of the curve;
//! * `p1_budget`: `cargo xtask p1-budget` (E12), the shapes at the committed
//!   budget and `budget_128`, and the report line that alarm reads.
//!
//! ```sh
//! cargo test --release -p pharmakos-bench --test perf_alarm -- --ignored --nocapture \
//!     --exact p1_quick_and_full_timings
//! ```
//!
//! (with `--locked` in CI). **`--release` is not optional**, for the mesher
//! alarm's reason: a debug build's overflow, bounds and inlining make its
//! figure a different number, not a slow version of this one.
//!
//! The measurements of one test run one after the other. One that cannot
//! measure prints a `::warning::` and the others still run; the test fails at
//! the end if any did. Each test prints an empty line before its first
//! notice: libtest run on one thread prints `test <name> ... ` before the test
//! starts, and a workflow command that does not start its line is not read as
//! one.
//!
//! Wall-clock time is legal here: the crate is walled (`WALLED_PACKAGES` in
//! `xtask/src/main.rs`), and nothing timed here reaches the sim.

use std::path::Path;

use pharmakos_bench::decision::{self, BENCH_SEATS, BENCH_SEED, DecisionPlan};
use pharmakos_bench::full;
use pharmakos_bench::quick::{self, Fixture, QuickPlan};
use pharmakos_bench::shapes::{SIZES, Shape};
use pharmakos_bench::{Error, workspace_root};
use pharmakos_proto::gp::api::v1::verify_plan::Depth;

/// Untimed calls per case, or per shape, size and depth, before its sample.
const QUICK_WARMUP: usize = 20;

/// Timed calls per case, or per shape, size and depth. Two hundred calls make
/// an input's own p99 its second-slowest call; the pool's p99 is taken over
/// every case's calls.
const QUICK_CALLS: usize = 200;

/// The budget line's sample: more calls than the alarm's, since one figure
/// per input decides it.
const BUDGET_PLAN: QuickPlan = QuickPlan {
    warmup: 50,
    calls: 500,
};

/// How many times the bench match is played, each tick keeping its fastest.
const RUNS: usize = 3;

/// The decision-tick curve's segment at each size: 30 seconds, 120 decision
/// ticks at the committed 250 ms cadence, the fourth seat's line's length.
const CURVE_SEGMENT_MS: i32 = 30_000;

/// Print each line of a measurement, or its warning, and keep the failure.
fn report(os: &str, title: &str, lines: Result<Vec<String>, Error>, failed: &mut Vec<Error>) {
    match lines {
        Ok(lines) => lines.iter().for_each(|line| println!("{line}")),
        Err(error) => {
            println!("::warning title={title}::not measured on {os}: {error}");
            failed.push(error);
        }
    }
}

#[test]
#[ignore = "a performance alarm, not a gate: cargo xtask perf-alarms p1 runs it with --exact"]
fn p1_quick_and_full_timings() {
    let root = workspace_root();
    let os = std::env::consts::OS;
    // A notice must start its line; see the module docs.
    println!();
    let mut failed = Vec::new();
    report(os, "QUICK p99", quick_notices(&root, os), &mut failed);
    report(os, "FULL p99", full_cases_notice(&root, os), &mut failed);
    report(os, "P1 shapes", shape_notices(&root, os), &mut failed);
    assert!(failed.is_empty(), "{failed:?}");
}

#[test]
#[ignore = "a performance alarm, not a gate: cargo xtask perf-alarms p1-decision runs it with --exact"]
fn p1_decision_tick_timings() {
    let root = workspace_root();
    let os = std::env::consts::OS;
    println!();
    let mut failed = Vec::new();
    report(
        os,
        "decision tick per size unit",
        decision_notice(&root, os).map(|line| vec![line]),
        &mut failed,
    );
    report(
        os,
        "decision tick by size",
        curve_notice(&root, os).map(|line| vec![line]),
        &mut failed,
    );
    assert!(failed.is_empty(), "{failed:?}");
}

#[test]
#[ignore = "the P1 CI alarm: cargo xtask p1-budget runs it with --ignored --exact"]
fn p1_budget() {
    let root = workspace_root();
    let os = std::env::consts::OS;
    println!();
    match full::measure_budget(&root, BUDGET_PLAN) {
        Ok(figures) => {
            match full::budget_notice(&figures, os) {
                Ok(line) => println!("{line}"),
                Err(error) => panic!("the budget notice: {error}"),
            }
            println!("{}", full::budget_report_line(&figures));
        }
        // No report line: `cargo xtask p1-budget` fails on its absence too.
        Err(error) => panic!("P1 at the size budget was not measured on {os}: {error}"),
    }
}

/// QUICK's p50 and p99 over the committed cases, as notices.
fn quick_notices(root: &Path, os: &str) -> Result<Vec<String>, Error> {
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
        return Err(Error::Drift(format!(
            "{} samples pooled, expected {}",
            timing.pooled.samples,
            cases.len() * QUICK_CALLS
        )));
    }
    Ok(quick::notices(&timing, os))
}

/// FULL's p50 and p99 over the committed cases, as a notice.
fn full_cases_notice(root: &Path, os: &str) -> Result<Vec<String>, Error> {
    let fixture = Fixture::committed(root)?;
    let cases = quick::committed_cases(root)?;
    let timing = quick::measure_at(
        &fixture,
        &cases,
        QuickPlan {
            warmup: QUICK_WARMUP,
            calls: QUICK_CALLS,
        },
        Depth::Full,
    )?;
    Ok(vec![full::cases_notice(&timing, os)])
}

/// Every shape at every size of the curve: the provenance, then a notice a
/// shape.
fn shape_notices(root: &Path, os: &str) -> Result<Vec<String>, Error> {
    let curve = full::measure_curve(
        root,
        &SIZES,
        QuickPlan {
            warmup: QUICK_WARMUP,
            calls: QUICK_CALLS,
        },
    )?;
    let mut lines = vec![full::provenance_notice(&curve, os)];
    lines.extend(
        Shape::ALL
            .into_iter()
            .map(|shape| full::shape_notice(&curve, shape, os)),
    );
    Ok(lines)
}

/// The decision tick's ns per size unit, as a notice. The bench match's one
/// segment is the rules table's first, the shortest Push of the ladder.
fn decision_notice(root: &Path, os: &str) -> Result<String, Error> {
    let rules = quick::committed_rules(root)?;
    let segment_ms = *rules
        .segment_lengths_ms()
        .first()
        .ok_or_else(|| Error::Rules("no match.segment_lengths_ms".to_owned()))?;
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
        return Err(Error::Empty("no decision tick was measured".to_owned()));
    }
    Ok(decision::notice(&timing, os))
}

/// The decision tick at each size of the curve, as a notice.
fn curve_notice(root: &Path, os: &str) -> Result<String, Error> {
    let rules = quick::committed_rules(root)?;
    let curve = decision::measure_curve(
        &rules,
        &SIZES,
        DecisionPlan {
            seed: BENCH_SEED,
            seats: BENCH_SEATS,
            segment_ms: CURVE_SEGMENT_MS,
            runs: RUNS,
        },
    )?;
    Ok(decision::curve_notice(&curve, BENCH_SEED, BENCH_SEATS, os))
}
