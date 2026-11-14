// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! P1's wall-clock figures, published as notices and never gated, the one
//! report line `cargo xtask p1-budget` reads, and the certifying run.
//!
//! S1's plan section 5 and task `p1`: QUICK's p50 and p99 over the committed
//! verifier cases and `budget_128`, and a decision tick's cost in ns per size
//! unit in a hosted match whose playbooks sit at the size budget. S3's plan
//! section 5.2 and task `bench3` add P1 FULL: FULL over the same cases, both
//! depths over the generated shapes at each size of E24's curve with the
//! end-to-end `verify_plan` beside them, QUICK's cost at the transport cap,
//! and the decision tick's cost at each size, in nanoseconds and in counted
//! evaluation units. Each figure is a `::notice::` per runner with **no
//! threshold**, never compared across operating systems (decisions-log item
//! 116 (6) (b)).
//!
//! The binary holds five `#[ignore]`d tests, so `cargo test --workspace`
//! pays for none of them, and each is run **alone, by its exact name**, so no
//! two share the machine:
//!
//! * `p1_quick_and_full_timings`: `cargo xtask perf-alarms p1`, ten notices
//!   ([`P1_NOTICES`]): QUICK's two, FULL's over the cases, the shapes'
//!   provenance and one per shape;
//! * `p1_transport_cap_timings`: `cargo xtask perf-alarms p1-cap`, the
//!   `max_text` shape in ASCII and in four-byte characters;
//! * `p1_decision_tick_timings`: `cargo xtask perf-alarms p1-decision`, the
//!   decision tick at the size budget on two seeds, the curve and the
//!   selector variant at the budget;
//! * `p1_budget`: `cargo xtask p1-budget` (E12), the shapes at the committed
//!   budget and `budget_128`, and the report line that alarm reads;
//! * `p1_certify`: run by hand by `cert3` on the owner's machine, every P1
//!   figure at [`full::CERT_PLAN`]'s 2 000 calls (item 158 (2)); never in CI.
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
//! the end if any did, or if an alarm printed other than the notices it
//! declares, each declared at most ten (GitHub annotates only a step's first
//! ten, item 149 (4)). Each test prints an empty line before its first
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

/// The alarm's sample, per case or per shape, size and depth: 20 untimed
/// calls, then 200 timed. With 200 calls an input's own nearest-rank p99 is
/// its 198th call, the third-slowest; the pool's p99 is taken over every
/// case's calls. CI's counts are kept cheap, and every notice says them;
/// certification takes [`full::CERT_PLAN`].
const ALARM_PLAN: QuickPlan = QuickPlan {
    warmup: 20,
    calls: 200,
};

/// How many times the bench match is played, each tick keeping its fastest.
const RUNS: usize = 3;

/// The decision-tick curve's segment at each size: 30 seconds, 120 decision
/// ticks at the committed 250 ms cadence, the fourth seat's line's length.
const CURVE_SEGMENT_MS: i32 = 30_000;

/// The second seed the decision tick is measured on at the budget, so E12's
/// seed rule (`BENCH_SEED` stays unless another seed reads more than 20 %
/// above it) has a figure to compare.
const SECOND_SEED: u64 = pharmakos_sim::DETERMINISM_MATCH_SEED;

/// GitHub annotates the first ten notices of a step.
const NOTICE_LIMIT: usize = 10;

/// `p1`'s notices: QUICK's two, FULL's over the cases, the provenance, and
/// one a shape.
const P1_NOTICES: usize = 4 + Shape::ALL.len();

/// `p1-cap`'s notices: one a padding.
const P1_CAP_NOTICES: usize = 2;

/// `p1-decision`'s notices: the budget on two seeds, the curve, and the
/// selector variant at the budget.
const P1_DECISION_NOTICES: usize = 4;

const _: () = assert!(P1_NOTICES <= NOTICE_LIMIT);
const _: () = assert!(P1_CAP_NOTICES <= NOTICE_LIMIT);
const _: () = assert!(P1_DECISION_NOTICES <= NOTICE_LIMIT);

/// Print each line of a measurement, or its warning, and keep the failure.
/// Answers how many notices it printed.
fn report(
    os: &str,
    title: &str,
    lines: Result<Vec<String>, Error>,
    failed: &mut Vec<Error>,
) -> usize {
    match lines {
        Ok(lines) => {
            for line in &lines {
                println!("{line}");
            }
            lines
                .iter()
                .filter(|line| line.starts_with("::notice "))
                .count()
        }
        Err(error) => {
            println!("::warning title={title}::not measured on {os}: {error}");
            failed.push(error);
            0
        }
    }
}

/// The test's verdict: no measurement failed, and an alarm that measured
/// everything printed exactly the notices it declares.
fn verdict(failed: &[Error], printed: usize, declared: usize) {
    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(
        printed, declared,
        "the alarm printed {printed} notices and declares {declared}"
    );
}

#[test]
#[ignore = "a performance alarm, not a gate: cargo xtask perf-alarms p1 runs it with --exact"]
fn p1_quick_and_full_timings() {
    let root = workspace_root();
    let os = std::env::consts::OS;
    // A notice must start its line; see the module docs.
    println!();
    let mut failed = Vec::new();
    let mut printed = report(os, "QUICK p99", quick_notices(&root, os), &mut failed);
    printed += report(
        os,
        "FULL p99",
        full_cases_notice(&root, ALARM_PLAN, os),
        &mut failed,
    );
    printed += report(
        os,
        "P1 shapes",
        shape_notices(&root, ALARM_PLAN, os),
        &mut failed,
    );
    verdict(&failed, printed, P1_NOTICES);
}

#[test]
#[ignore = "a performance alarm, not a gate: cargo xtask perf-alarms p1-cap runs it with --exact"]
fn p1_transport_cap_timings() {
    let root = workspace_root();
    let os = std::env::consts::OS;
    println!();
    let mut failed = Vec::new();
    let printed = report(
        os,
        "P1 at the transport cap",
        cap_notices(&root, ALARM_PLAN, os),
        &mut failed,
    );
    verdict(&failed, printed, P1_CAP_NOTICES);
}

#[test]
#[ignore = "a performance alarm, not a gate: cargo xtask perf-alarms p1-decision runs it with --exact"]
fn p1_decision_tick_timings() {
    let root = workspace_root();
    let os = std::env::consts::OS;
    println!();
    let mut failed = Vec::new();
    let mut printed = 0;
    for seed in [BENCH_SEED, SECOND_SEED] {
        printed += report(
            os,
            "decision tick per size unit",
            decision_notice(&root, seed, os).map(|line| vec![line]),
            &mut failed,
        );
    }
    printed += report(
        os,
        "decision tick by size",
        curve_notice(&root, Shape::FullEval, &SIZES, os).map(|line| vec![line]),
        &mut failed,
    );
    printed += report(
        os,
        "decision tick with selectors",
        budget_size(&root).and_then(|units| {
            curve_notice(&root, Shape::FullEvalSelectors, &[units], os).map(|line| vec![line])
        }),
        &mut failed,
    );
    verdict(&failed, printed, P1_DECISION_NOTICES);
}

#[test]
#[ignore = "the P1 CI alarm: cargo xtask p1-budget runs it with --ignored --exact"]
fn p1_budget() {
    let root = workspace_root();
    let os = std::env::consts::OS;
    println!();
    match full::measure_budget(&root, full::BUDGET_PLAN) {
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

#[test]
#[ignore = "the certifying run, by hand on the owner's machine (--ignored --exact)"]
fn p1_certify() {
    let root = workspace_root();
    let os = std::env::consts::OS;
    println!();
    let mut failed = Vec::new();
    report(
        os,
        "FULL p99",
        full_cases_notice(&root, full::CERT_PLAN, os),
        &mut failed,
    );
    report(
        os,
        "P1 shapes",
        shape_notices(&root, full::CERT_PLAN, os),
        &mut failed,
    );
    report(
        os,
        "P1 at the transport cap",
        cap_notices(&root, full::CERT_PLAN, os),
        &mut failed,
    );
    report(
        os,
        "P1 budget",
        full::measure_budget(&root, full::CERT_PLAN).and_then(|figures| {
            Ok(vec![
                full::budget_notice(&figures, os)?,
                full::budget_report_line(&figures),
            ])
        }),
        &mut failed,
    );
    assert!(failed.is_empty(), "{failed:?}");
}

/// QUICK's p50 and p99 over the committed cases, as notices.
fn quick_notices(root: &Path, os: &str) -> Result<Vec<String>, Error> {
    let fixture = Fixture::committed(root)?;
    let cases = quick::committed_cases(root)?;
    let timing = quick::measure(&fixture, &cases, ALARM_PLAN)?;
    if timing.pooled.samples != cases.len() * ALARM_PLAN.calls {
        return Err(Error::Drift(format!(
            "{} samples pooled, expected {}",
            timing.pooled.samples,
            cases.len() * ALARM_PLAN.calls
        )));
    }
    Ok(quick::notices(&timing, os))
}

/// FULL's p50 and p99 over the committed cases, as a notice.
fn full_cases_notice(root: &Path, plan: QuickPlan, os: &str) -> Result<Vec<String>, Error> {
    let fixture = Fixture::committed(root)?;
    let cases = quick::committed_cases(root)?;
    let timing = quick::measure_at(&fixture, &cases, plan, Depth::Full)?;
    Ok(vec![full::cases_notice(&timing, os)])
}

/// Every shape at every size of the curve: the provenance, then a notice a
/// shape.
fn shape_notices(root: &Path, plan: QuickPlan, os: &str) -> Result<Vec<String>, Error> {
    let curve = full::measure_curve(root, &SIZES, plan)?;
    let mut lines = vec![full::provenance_notice(&curve, os)];
    lines.extend(
        Shape::ALL
            .into_iter()
            .map(|shape| full::shape_notice(&curve, shape, os)),
    );
    Ok(lines)
}

/// The `max_text` shape at the transport cap, a notice a padding.
fn cap_notices(root: &Path, plan: QuickPlan, os: &str) -> Result<Vec<String>, Error> {
    Ok(full::measure_cap(root, plan)?
        .iter()
        .map(|figure| full::cap_notice(figure, plan, os))
        .collect())
}

/// The committed size budget.
fn budget_size(root: &Path) -> Result<u32, Error> {
    full::committed_budget(&quick::committed_rules(root)?)
}

/// The decision tick's ns per size unit on `seed`, as a notice. The bench
/// match's one segment is the rules table's first, the shortest Push of the
/// ladder.
fn decision_notice(root: &Path, seed: u64, os: &str) -> Result<String, Error> {
    let rules = quick::committed_rules(root)?;
    let segment_ms = *rules
        .segment_lengths_ms()
        .first()
        .ok_or_else(|| Error::Rules("no match.segment_lengths_ms".to_owned()))?;
    let timing = decision::measure(
        root,
        &rules,
        DecisionPlan {
            seed,
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

/// The decision tick with every seat sealing `shape` at each of `sizes`, as a
/// notice.
fn curve_notice(root: &Path, shape: Shape, sizes: &[u32], os: &str) -> Result<String, Error> {
    let rules = quick::committed_rules(root)?;
    let curve = decision::measure_curve(
        &rules,
        shape,
        sizes,
        DecisionPlan {
            seed: BENCH_SEED,
            seats: BENCH_SEATS,
            segment_ms: CURVE_SEGMENT_MS,
            runs: RUNS,
        },
    )?;
    Ok(decision::curve_notice(
        &curve,
        shape,
        BENCH_SEED,
        BENCH_SEATS,
        os,
    ))
}
