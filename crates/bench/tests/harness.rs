// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The harness measures what it says it measures.
//!
//! These run in every leg's `test` step, debug build, and time nothing worth
//! reading: they hold the inputs to what P1 and G3′ name, so that the figures
//! `tests/perf_alarm.rs` and `tests/g3_real.rs` publish are figures about the
//! committed cases, a hosted match at the size budget and the gate's layout,
//! and not about something that drifted.
//!
//! # What the bench restates, and what holds each restatement
//!
//! The bench cannot import an integration test's helpers or a crate's private
//! items, so it restates a few. Each is listed here with the check that turns
//! its drift red (decisions-log item 133 (5)):
//!
//! | Restated in the bench | From | Held by |
//! |---|---|---|
//! | `quick::fixture_scope`, `map_scope`, `short_scope`, `sight_scope` | `crates/verifier/tests/verifier.rs`'s namesakes | `every_case_under_the_bench_fixture_is_its_committed_report` |
//! | `quick::fixture_snapshot`, `segment_snapshot` | the same file's namesakes | the same test |
//! | `quick::MAP_CASES`, `SHORT_CASES`, `SEGMENT_CASES`, `SIGHT_CASES` | the same file's namesakes | the same test |
//! | `quick::Fixture` (its scope and snapshot fields, `committed`, `scope_for`, `snapshot_for`, `report`) | the same file's case-to-fixture routing | the same test |
//! | `decision::hosted_budget_playbook` | `crates/verifier/tests/cases/budget_128.json` with its anchors named by role | `a_short_hosted_match_seals_every_seat_at_the_budget_and_pairs_its_decision_ticks`, and the unit test `the_hosted_playbook_is_the_budget_case_named_by_role` |
//! | `g3::sight`'s `seen_by` (the sensor walk under `RadiusOnly`) | `crates/sim/src/world/combat.rs`'s private `World::seen_by` | none can reach a private item; the probe names it in its notice as a restatement, and `g3r` reviews it against the sim at the certifying run |
//! | `g3::sight`'s core (a seat's lowest-id beacon row) | `crates/sim/src/power.rs`'s crate-private `core_of` | the same |
//! | `g3::variant`'s draw per seat (`kw_per_unit` a unit, none for the commander, `beacon_base_draw_kw` a beacon) | `crates/sim/src/power.rs`'s draw rule | `the_gate_match_is_reproducible_from_its_seed_and_rules_hash`: an underpowered layout would brown out, and the unit test `the_variant_moves_only_its_named_rows` pins the sum |

use std::fs;

use pharmakos_bench::decision::{self, BENCH_SEATS, BENCH_SEED, DecisionPlan};
use pharmakos_bench::quick::{self, BUDGET_CASE, Fixture, QuickPlan};
use pharmakos_bench::workspace_root;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::json;

/// The bench's restatement of the verifier goldens' fixture is that fixture:
/// every committed case, verified FULL under it, is its committed report byte
/// for byte. When this goes red after a change to
/// `crates/verifier/tests/verifier.rs`'s fixture (any row of the module docs'
/// table that names that file), restate the change in `quick`'s namesakes;
/// the QUICK figure is only P1's if it is taken on the goldens' inputs.
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

/// G3′'s gate match opens on the committed seed: the bench variant, the
/// layout's 37 beacons and 300 units placed through the host's layout seam,
/// every seat sealed at the size budget; and the run is reproducible from the
/// report's seed and `rules_hash`: two runs of the same plan play the same
/// layout, the same rules and the same hash chain. A debug build, so nothing
/// is timed worth reading.
#[test]
fn the_gate_match_is_reproducible_from_its_seed_and_rules_hash() {
    use pharmakos_bench::g3::segment::{self, SegmentPlan};
    let root = workspace_root();
    let plan = SegmentPlan {
        seed: BENCH_SEED,
        // Twenty ticks: a second's worth of the crater stream.
        segment_ms: 1_000,
        craters_per_second: 20,
        probe_sight: true,
        share: pharmakos_bench::g3::layout::Quarters::FULL,
    };
    let first = segment::measure(&root, plan, None).expect("a measurement");
    // The probe reads the world and moves nothing, so the second run skips it.
    let second = segment::measure(
        &root,
        SegmentPlan {
            probe_sight: false,
            ..plan
        },
        None,
    )
    .expect("a second measurement");
    assert_eq!(first.provenance, second.provenance);
    assert_eq!(
        first.chain_digest, second.chain_digest,
        "the same run plays the same chain"
    );
    assert_eq!(first.ticks, 20);
    assert_eq!(first.craters.filed, 20, "{:?}", first.craters);
    assert_eq!(first.craters.queue_full, 0);
    assert_ne!(
        first.provenance.bench_rules_hash, first.provenance.shipped_rules_hash,
        "the variant is not the shipped table"
    );
    assert!(
        first
            .minutes
            .first()
            .is_some_and(|minute| minute.sight.is_some()),
        "the probe ran"
    );
}

/// The support search's adversarial tick is what the module documents: on
/// the gate's match, the construct stands once built, and on the tick its
/// stems are cut A (a region of exactly `collapse.max_region_voxels`), X and
/// Y all fall, X more than once (the interlock), with the charges' seeds
/// searched beside them. A debug build, so nothing is timed worth reading.
#[test]
fn the_adversarial_support_tick_drops_the_construct_as_built() {
    let root = workspace_root();
    let report = pharmakos_bench::g3::support::measure(&root, BENCH_SEED).expect("a measurement");
    assert!(report.regions >= 3, "{report:?}");
    assert!(
        report.voxels >= u64::from(report.max_region_voxels),
        "{report:?}"
    );
    assert!(report.passes >= 3, "the interlock takes passes: {report:?}");
    assert_eq!(
        report.bound_skips, 0,
        "A is exactly the bound, not past it: {report:?}"
    );
}
