// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! P3's wall-clock figures (S3's plan, section 5.1 and task `bench3`),
//! published as notices and never gated, and the one report line `cargo
//! xtask p3-budget` reads (E12).
//!
//! The repath on the sim's own graph, cold and under the crater stream with
//! the repair beside it, and the travel estimate through the gateway's
//! `estimate_route` in a hosted Lull and through the sim's estimator alone,
//! on three seeds, the golden seed first ([`pharmakos_bench::p3`] says what
//! each timed region is). Each figure is a `::notice::` per runner with **no
//! threshold**, never compared across operating systems (decisions-log item
//! 116 (6) (b)); the gate is certified on the owner's machine (S3's `cert3`).
//!
//! The binary holds two `#[ignore]`d tests, each run **alone, by its exact
//! name**:
//!
//! * `p3_pathing_timings`: `cargo xtask perf-alarms p3`, a notice a seed and
//!   the summary, four in all;
//! * `p3_budget`: `cargo xtask p3-budget`, the summary and the report line.
//!
//! ```sh
//! cargo test --release -p pharmakos-bench --test p3_alarm -- --ignored --nocapture \
//!     --exact p3_pathing_timings
//! ```
//!
//! (with `--locked` in CI). **`--release` is not optional**: a debug build's
//! figure is a different number, not a slow version of this one. Each test
//! prints an empty line before its first notice, for `perf_alarm.rs`'s
//! reason.

use pharmakos_bench::p3::{self, P3Plan};
use pharmakos_bench::workspace_root;

#[test]
#[ignore = "a performance alarm, not a gate: cargo xtask perf-alarms p3 runs it with --exact"]
fn p3_pathing_timings() {
    let root = workspace_root();
    let os = std::env::consts::OS;
    // A notice must start its line; see the module docs.
    println!();
    match p3::measure(&root, &P3Plan::gate()) {
        Ok(report) => {
            for seed in &report.seeds {
                println!("{}", p3::seed_notice(&report, seed, os));
            }
            match p3::summary_notice(&report, os) {
                Ok(line) => println!("{line}"),
                Err(error) => panic!("the P3 summary: {error}"),
            }
        }
        Err(error) => {
            println!("::warning title=P3::not measured on {os}: {error}");
            panic!("P3 was not measured on {os}: {error}");
        }
    }
}

#[test]
#[ignore = "the P3 CI alarm: cargo xtask p3-budget runs it with --ignored --exact"]
fn p3_budget() {
    let root = workspace_root();
    let os = std::env::consts::OS;
    println!();
    match p3::measure(&root, &P3Plan::gate()) {
        Ok(report) => {
            match p3::summary_notice(&report, os) {
                Ok(line) => println!("{line}"),
                Err(error) => panic!("the P3 summary: {error}"),
            }
            println!("{}", p3::budget_report_line(&report));
        }
        // No report line: `cargo xtask p3-budget` fails on its absence too.
        Err(error) => panic!("P3 was not measured on {os}: {error}"),
    }
}
