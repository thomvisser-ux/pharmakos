// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The harness measures what it says it measures.
//!
//! These run in every leg's `test` step, debug build, and time nothing worth
//! reading: they hold the inputs to what P1 names, so that the figures
//! `tests/perf_alarm.rs` publishes are figures about the committed cases and a
//! hosted match at the size budget, and not about something that drifted.

use std::fs;

use pharmakos_bench::decision::{self, BENCH_SEATS, BENCH_SEED, DecisionPlan};
use pharmakos_bench::quick::{self, BUDGET_CASE, Fixture, QuickPlan};
use pharmakos_bench::workspace_root;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::json;

/// The bench's restatement of the verifier goldens' fixture is that fixture:
/// every committed case, verified FULL under it, is its committed report byte
/// for byte. When this goes red after a change to
/// `crates/verifier/tests/verifier.rs`'s `fixture_scope`, `map_scope`,
/// `MAP_CASES` or `fixture_snapshot`, restate the change in `quick`'s
/// namesakes; the QUICK figure is only P1's if it is taken on
/// the goldens' inputs.
#[test]
fn every_case_under_the_bench_fixture_is_its_committed_report() {
    let root = workspace_root();
    let fixture = Fixture::committed(&root).expect("the committed fixture");
    let cases = quick::committed_cases(&root).expect("the committed cases");
    let goldens = root.join("tests").join("golden").join("verifier");
    let mut drifted: Vec<String> = Vec::new();
    for case in &cases {
        let report = fixture
            .report(case, Depth::Full)
            .expect("the rules table is complete");
        let mut text = json::encode(&report).expect("the report encodes");
        text.push('\n');
        let path = goldens.join(&case.name).join("expected.report.json");
        let expected = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
        if text != expected {
            drifted.push(case.name.clone());
        }
    }
    assert!(
        drifted.is_empty(),
        "the bench's fixture is no longer the verifier goldens' fixture for {drifted:?}"
    );
    assert!(cases.len() > 1, "only {} case(s)", cases.len());
}

#[test]
fn the_budget_case_sits_exactly_on_the_budget() {
    let root = workspace_root();
    let fixture = Fixture::committed(&root).expect("the committed fixture");
    let cases = quick::committed_cases(&root).expect("the committed cases");
    let budget = cases
        .iter()
        .find(|case| case.name == BUDGET_CASE)
        .expect("budget_128 is committed");
    let report = fixture
        .report(budget, Depth::Quick)
        .expect("the rules table is complete");
    assert!(report.qualifies, "{:?}", report.diagnostics);
    assert_eq!(report.size_units, report.size_budget);
}

#[test]
fn a_short_quick_measurement_covers_every_case() {
    let root = workspace_root();
    let fixture = Fixture::committed(&root).expect("the committed fixture");
    let cases = quick::committed_cases(&root).expect("the committed cases");
    let timing = quick::measure(
        &fixture,
        &cases,
        QuickPlan {
            warmup: 0,
            calls: 2,
        },
    )
    .expect("a measurement");
    assert_eq!(timing.cases, cases.len());
    assert_eq!(timing.pooled.samples, cases.len() * 2);
    assert_eq!(timing.budget.samples, 2);
    assert!(timing.pooled.p50_ns <= timing.pooled.p99_ns);
    assert!(cases.iter().any(|case| case.name == timing.slowest.0));
}

/// Every seat's budget playbook goes through `submit_plan` and is sealed at
/// the budget (a refusal or an off-budget seal is an error), the two runs play
/// the same chain, and decision ticks are found and paired. The figures
/// themselves are not asserted: this is a debug build.
#[test]
fn a_short_hosted_match_seals_every_seat_at_the_budget_and_pairs_its_decision_ticks() {
    let root = workspace_root();
    let rules = quick::committed_rules(&root).expect("the committed rules");
    let timing = decision::measure(
        &root,
        &rules,
        DecisionPlan {
            seed: BENCH_SEED,
            seats: BENCH_SEATS,
            // Forty ticks: eight decision ticks at the committed 250 ms cadence.
            segment_ms: 2_000,
            runs: 2,
        },
    )
    .expect("a measurement");
    assert_eq!(timing.seats, BENCH_SEATS);
    let budget = rules
        .message()
        .verifier
        .as_ref()
        .map(|block| block.size_budget_units)
        .expect("the rules table has a verifier block");
    assert_eq!(timing.units_per_seat, budget);
    assert_eq!(timing.ticks, 40);
    assert!(timing.windows >= 7, "{timing:?}");
}
