// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The harness measures what it says it measures.
//!
//! These run in every leg's `test` step, debug build, and time nothing worth
//! reading: they hold the inputs to what P1, G3′ and P3 name, so that the
//! figures `tests/perf_alarm.rs`, `tests/g3_real.rs` and `tests/p3_alarm.rs`
//! publish are figures about the committed cases, the generated shapes at
//! their sizes, a hosted match at the size budget, the gate's layout and the
//! sim's own graph, and not about something that drifted.
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
//! | `shapes::Shape::Handlers24` at 127 units (63 handlers of `any(any(any(21)))` and `all(all(all(21)))` over distinct `segment_elapsed EQ` leaves, and a route step) | `crates/verifier/src/lint.rs`'s `the_worst_case_is_bounded_by_the_memo` (`vfy5`; items 157 (7) and 158 (1)) | the unit test `the_handler_shape_at_127_is_item_157s_worst_case`, and `every_shape_at_every_size_qualifies_on_its_budget` |
//! | `shapes`' anchors inside the fixture core's sphere (`b_01` at (80, 11, 55), 24 voxels) | `quick::fixture_scope` and the rules' `beacon.sphere_radius_voxels` | `every_shape_at_every_size_qualifies_on_its_budget`: an anchor off the map or out of the sphere is an error code, and a shape that does not qualify is refused before it is timed |
//! | `full::sized_rules` (the committed table, `verifier.size_budget_units` set to the size) | the row the verifier reads at `crates/verifier/src/limits.rs`'s `size_budget_units` (E24) | `every_shape_at_every_size_qualifies_on_its_budget` (each report's `size_budget` is the size), and the unit test `a_sized_table_moves_only_its_budget_row` |
//! | `p3::Graph::generate` (`mapgen::generate`, the store settled, `Surface::new`, `pathing::graph_for`) | `World::new`'s graph (`crates/sim/src/world.rs`) | `a_short_p3_run_measures_every_figure_on_the_worlds_own_graph`: the measurement refuses a hosted world whose graph's facts differ from the bench's |
//! | `p3::mark_chunk_clusters` | `crates/sim/src/world.rs`'s private `mark_chunk_clusters` | `the_crater_streams_repair_is_a_rebuild`: after the stream's craters the repaired graph's transitions and edges are a from-scratch rebuild's, which a missed cluster would break |
//! | `full`'s hosted core (seat 0's core is `b_00`, at its beacon row's position) | the gateway's per-seat beacon names (`crates/gateway/src/surface.rs`, "`b_00`, its core, first") | `the_end_to_end_figure_verifies_the_compact_text_through_the_gateway`: the target shape at that core qualifies through `verify_plan` |
//! | `shapes::FIXTURE_EXTENT` | the committed rules' `map.size_x` and `map.size_y` | `the_fixture_extent_is_the_committed_map` |
//! | `p3::check_hosted_graph`'s route-adapter half (its columns and cluster edge) | `crates/gateway/src/routes.rs`'s `RouteAdapter`, whose transitions and edges have no getter | the check itself refuses a mismatch; the adapter's transitions and edges are compared only through the world's graph it is built from, until a gateway lane adds a getter |
//! | `g3::layout::raid_settings`' aim (an Attack beacon's nearest enemy core) | `g3::layout::layout_at`'s placing, `ATTACK_FROM_ENEMY_CORE` voxels from the enemy core | `the_gate_load_with_raids_launches`: the hosted segment's `attack_launched` count is above zero |

use std::fs;

use pharmakos_bench::decision::{self, BENCH_SEATS, BENCH_SEED, DecisionPlan};
use pharmakos_bench::full;
use pharmakos_bench::provenance::{self, Certify};
use pharmakos_bench::quick::{self, BUDGET_CASE, Fixture, QuickPlan};
use pharmakos_bench::shapes::{SIZES, Shape};
use pharmakos_bench::workspace_root;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::json;

/// The bench's restatement of the verifier goldens' fixture is that fixture:
/// every committed case, verified FULL under it, is its committed report byte
/// for byte. When this goes red after a change to
/// `crates/verifier/tests/verifier.rs`'s fixture (any row of the module docs'
/// table that names that file), restate the change in `quick`'s namesakes;
/// the QUICK figure is only P1's if it is taken on the goldens' inputs.
/// `cert3` fix pass 2, M2 and B7: every line a certifying command assembles
/// for a cut run, asked to certify or not, carries the banner and fails the
/// one predicate, and none speaks in the certifying role.
fn no_line_certifies(lines: &[String], what: &str) {
    use pharmakos_bench::provenance::{self, Role};
    assert!(!lines.is_empty(), "{what}: no line");
    for line in lines {
        assert!(!provenance::is_certifying(line), "{what}: {line}");
        assert!(line.contains(provenance::CUT_BANNER), "{what}: {line}");
        for foreign in Role::Published.foreign_phrases() {
            assert!(!line.contains(foreign), "{what}: {line}");
        }
        assert!(!line.contains("never gated"), "{what}: {line}");
    }
}

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
        load: segment::GateLoad::S2,
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

/// Every generated shape at every size of the curve qualifies, sitting
/// exactly on the budget of its sized table, at both depths: the curve's
/// figures are figures of playbooks the verifier accepts. A debug build, so
/// nothing is timed.
#[test]
fn every_shape_at_every_size_qualifies_on_its_budget() {
    let root = workspace_root();
    let mut fixture = Fixture::committed(&root).expect("the committed fixture");
    let committed = fixture.rules().clone();
    for units in SIZES {
        let sized = full::sized_rules(&committed, units).expect("a sized table");
        fixture = fixture.with_rules(sized.rules);
        for shape in Shape::ALL {
            let case = full::shape_case(shape, units).expect("the shape");
            if let Err(error) = full::check_at_budget(&fixture, &case, units) {
                panic!("{error}");
            }
        }
    }
}

/// The budget line's measurement covers every shape at the committed budget
/// and `budget_128`, and prints the line `cargo xtask p1-budget` reads.
#[test]
fn the_p1_budget_line_covers_every_input() {
    let root = workspace_root();
    let figures = full::measure_budget(
        &root,
        QuickPlan {
            warmup: 0,
            calls: 1,
        },
    )
    .expect("a measurement");
    assert_eq!(figures.planned, full::BUDGET_INPUTS);
    assert_eq!(figures.inputs.len(), figures.planned);
    assert!(figures.inputs.iter().any(|input| input.name == BUDGET_CASE));
    // Every shape at the budget is sent end to end: none is past the cap.
    assert_eq!(figures.end_to_end.len(), full::BUDGET_END_TO_END_INPUTS);
    assert_eq!(
        figures.worst_end_to_end().0,
        full::BUDGET_END_TO_END_INPUTS,
        "{:?}",
        figures.end_to_end
    );
    let line = full::budget_report_line(&figures);
    assert!(line.starts_with(full::BUDGET_REPORT_PREFIX), "{line}");
    // A cut run says so: one call of the 500 planned, which `cargo xtask
    // p1-budget` refuses as a short run, no verdict (xtask/src/budget.rs's
    // `a_cut_run_is_no_verdict`).
    assert!(
        line.contains(
            " inputs=7 planned_inputs=7 e2e_inputs=6 planned_e2e_inputs=6 calls=1 \
             planned_calls=500 "
        ),
        "{line}"
    );
}

/// The decision tick at a size of the curve: every seat seals the handler
/// shape at that size under its sized table, and the sim counts the work. A
/// debug build, so the nanoseconds are not asserted.
#[test]
fn the_decision_curve_seals_each_size_and_counts_its_work() {
    let root = workspace_root();
    let rules = quick::committed_rules(&root).expect("the committed rules");
    let curve = decision::measure_curve(
        &rules,
        Shape::FullEval,
        &[64],
        DecisionPlan {
            seed: BENCH_SEED,
            seats: BENCH_SEATS,
            // Twenty ticks: four decision ticks at the committed 250 ms cadence.
            segment_ms: 1_000,
            runs: 2,
        },
    )
    .expect("a measurement");
    let point = curve.first().expect("one size");
    assert_eq!(point.units, 64);
    assert_eq!(point.timing.units_per_seat, 64);
    assert_eq!(point.handlers, 31);
    assert!(point.timing.decision_ticks >= 4, "{point:?}");
    assert!(
        point.timing.work_max > 0,
        "the conditions are counted: {point:?}"
    );
}

/// The gate's load with raids (S3's plan, E13): the layout's settings rows
/// aim every Attack beacon at an enemy core, and on the hosted segment the
/// raids launch. `sim3a`'s row test steps `Runner::step` with no seal, so
/// this is the check on the hosted door (item 156 (3)).
#[test]
fn the_gate_load_with_raids_launches() {
    use pharmakos_bench::g3::layout::Quarters;
    use pharmakos_bench::g3::segment::{self, GateLoad, SegmentPlan};
    let root = workspace_root();
    let report = segment::measure(
        &root,
        SegmentPlan {
            seed: BENCH_SEED,
            // Forty ticks: eight decision ticks, and a force that stands
            // gathered launches on one of the first.
            segment_ms: 2_000,
            craters_per_second: 20,
            probe_sight: false,
            share: Quarters::FULL,
            load: GateLoad::RAIDS,
        },
        None,
    )
    .expect("a measurement");
    // Six Attack beacons: two a seat.
    assert_eq!(
        report.provenance.settings_rows, 6,
        "{:?}",
        report.provenance
    );
    assert!(
        report.attack_launches > 0,
        "the raids launch on the hosted segment: {} launches",
        report.attack_launches
    );
    // G3's verdict line names the load and turns with its bars: a debug
    // build's tick is over a bar of nothing and within a bar of everything.
    let over = pharmakos_bench::g3::tick_verdict(&report, 0, 0, "linux");
    assert!(
        over.contains(": OVER; mean ") && over.ends_with(": OVER"),
        "{over}"
    );
    assert!(over.contains(", the raids launching: tick p99 "), "{over}");
    let within = pharmakos_bench::g3::tick_verdict(&report, u128::MAX, u128::MAX, "linux");
    assert!(
        within.contains(": within; mean ") && within.ends_with(": within"),
        "{within}"
    );
    // B5: a figure exactly at its bar is within it, a nanosecond past over.
    let (p99, mean) = (report.tick.p99_ns, report.tick.mean_ns);
    let at = pharmakos_bench::g3::tick_verdict(&report, p99, mean, "linux");
    assert!(
        at.contains(": within; mean ") && at.ends_with(": within"),
        "{at}"
    );
    let past = pharmakos_bench::g3::tick_verdict(&report, p99 - 1, mean - 1, "linux");
    assert!(
        past.contains(": OVER; mean ") && past.ends_with(": OVER"),
        "{past}"
    );
    for certify in [Certify::On, Certify::Off] {
        let lines = pharmakos_bench::certify::raids_lines(
            &root,
            std::slice::from_ref(&report),
            certify,
            "linux",
        )
        .expect("the raids' lines");
        assert_eq!(lines.len(), 7, "the commit and one segment's six");
        no_line_certifies(&lines, "the raids");
    }
    assert!(pharmakos_bench::certify::raids_lines(&root, &[], Certify::On, "linux").is_err());
}

/// E24's test (c)'s load: every seat seals the handler shape at the size
/// budget on the gate's match with raids.
#[test]
fn the_gate_load_at_the_size_budget_seals_every_seat_at_b() {
    use pharmakos_bench::g3::layout::Quarters;
    use pharmakos_bench::g3::segment::{self, GateLoad, SegmentPlan};
    let root = workspace_root();
    let rules = quick::committed_rules(&root).expect("the committed rules");
    let budget = full::committed_budget(&rules).expect("a budget");
    let report = segment::measure(
        &root,
        SegmentPlan {
            seed: BENCH_SEED,
            segment_ms: 500,
            craters_per_second: 20,
            probe_sight: false,
            share: Quarters::FULL,
            load: GateLoad::RAIDS_AT_B,
        },
        None,
    )
    .expect("a measurement");
    assert_eq!(report.provenance.units_per_seat, budget);
    assert_eq!(report.provenance.load, GateLoad::RAIDS_AT_B);
}

/// A short P3 run takes every figure, on a hosted world whose graph is the
/// bench's (the measurement refuses one that is not). A debug build, so
/// nothing is timed worth reading.
#[test]
fn a_short_p3_run_measures_every_figure_on_the_worlds_own_graph() {
    use pharmakos_bench::p3::{self, P3Plan};
    let root = workspace_root();
    let report = p3::measure(
        &root,
        &P3Plan {
            seeds: vec![p3::P3_SEEDS[0]],
            pairs: 8,
        },
    )
    .expect("a measurement");
    let seed = report.seeds.first().expect("one seed");
    assert_eq!(seed.cold.samples, 8);
    assert_eq!(seed.stream.samples, 8);
    assert_eq!(seed.repair.samples, 8);
    assert_eq!(seed.one_leg.samples, 8);
    assert_eq!(seed.five_leg.samples, 8);
    assert_eq!(seed.estimator.samples, 8);
    assert_eq!(
        seed.craters,
        u64::try_from(8 + p3::WARMUP).expect("fits"),
        "the warm-up ticks land craters too"
    );
    assert_eq!(seed.graph.cluster_voxels, 32);
    // A short run is not the gate's, and its line says so to the reader:
    // one seed of three and 8 pairs of 2 000, which `cargo xtask p3-budget`
    // refuses as no verdict.
    let line = p3::budget_report_line(&report);
    assert!(
        line.contains(" seeds=1 planned_seeds=3 pairs=8 planned_pairs=2000 "),
        "{line}"
    );
    for certify in [Certify::On, Certify::Off] {
        let lines = pharmakos_bench::certify::p3_lines(&root, &report, certify, "linux")
            .expect("P3's lines");
        assert_eq!(lines.len(), 3, "the commit, one seed and the summary");
        no_line_certifies(&lines, "P3");
        assert!(
            lines
                .iter()
                .all(|line| line.contains(" 1 seeds of the certifying 3")
                    && line.contains(" 8 pairs a seed of the certifying 2000")),
            "{lines:?}"
        );
    }
}

/// The bench's restatement of the sim's cluster marking: after the crater
/// stream, the repaired graph is the graph a rebuild makes.
#[test]
fn the_crater_streams_repair_is_a_rebuild() {
    use pharmakos_bench::p3;
    let root = workspace_root();
    let rules = quick::committed_rules(&root).expect("the committed rules");
    let (repaired, rebuilt) =
        p3::stream_repair_check(&rules, p3::P3_SEEDS[0], 40).expect("a stream");
    assert_eq!(repaired, rebuilt);
}

/// Every generated shape's compact text, at every size of the curve up to
/// the committed budget, travels: its `verify_plan` request, as a client
/// writes it, is one message the frame carries (`frame::travels`), so a
/// player at the budget could send any of them. Past the budget the curve's
/// end-to-end figure marks a request that does not travel as over the
/// transport cap (`route_guards_24_node` at 192 units is one), which the
/// unit test of the curve's words pins. The rule is the gateway's own, read,
/// never restated.
#[test]
fn every_shape_fits_the_transport_cap() {
    let rules = quick::committed_rules(&workspace_root()).expect("the committed rules");
    let budget = full::committed_budget(&rules).expect("a budget");
    for units in SIZES.into_iter().filter(|units| *units <= budget) {
        for shape in Shape::ALL
            .into_iter()
            .chain([Shape::FullEval, Shape::FullEvalSelectors])
        {
            let canonical =
                String::from_utf8(shape.bytes(units).expect("the shape")).expect("utf-8");
            let (bytes, travels) =
                full::request_travels(&pharmakos_bench::shapes::compact(&canonical));
            assert!(
                travels,
                "{} at {units} units is a {bytes}-byte request, past what one message carries",
                shape.name()
            );
        }
    }
}

/// The fixture's map extent is the committed rules' `map` block, which the
/// target and area shapes keep their anchors inside.
#[test]
fn the_fixture_extent_is_the_committed_map() {
    let root = workspace_root();
    let rules = quick::committed_rules(&root).expect("the committed rules");
    let map = rules.message().map.as_ref().expect("a map block");
    assert_eq!(
        pharmakos_bench::shapes::FIXTURE_EXTENT,
        [
            i32::try_from(map.size_x).expect("fits"),
            i32::try_from(map.size_y).expect("fits")
        ]
    );
}

/// The end-to-end figure takes `verify_plan` through the hosted gateway on a
/// shape's compact text, at the hosted seat's own core: the shape that names
/// the core qualifies there too.
#[test]
fn the_end_to_end_figure_verifies_the_compact_text_through_the_gateway() {
    let root = workspace_root();
    let rules = quick::committed_rules(&root).expect("the committed rules");
    let budget = full::committed_budget(&rules).expect("a budget");
    let figure = full::end_to_end(
        &rules,
        QuickPlan {
            warmup: 0,
            calls: 1,
        },
        |core| {
            pharmakos_bench::shapes::canonical(
                &Shape::BuildTargets.playbook_at(budget, core)?,
                "build_targets",
            )
        },
    )
    .expect("a figure");
    match figure {
        full::EndToEnd::Measured {
            quick, qualifies, ..
        } => {
            assert_eq!(quick.samples, 1);
            assert!(qualifies, "the target shape at the hosted core qualifies");
        }
        full::EndToEnd::OverCap { bytes } => panic!("a {bytes}-byte request, past the cap"),
    }
}

/// The `max_text` shape sits exactly on the text cap, `MAX_PLAYBOOK_CHARS`, in
/// both paddings and qualifies on the budget; its request is past the 256 KiB
/// one message carries, so the end-to-end figure reads it OVER the transport
/// cap and sends nothing: a request over 256 KiB and under 512 K characters is
/// one the editor cannot have checked (decisions-log item 167 (4)).
#[test]
fn the_max_text_shape_qualifies_at_the_cap() {
    let root = workspace_root();
    let figures = full::measure_cap(
        &root,
        QuickPlan {
            warmup: 0,
            calls: 1,
        },
    )
    .expect("a measurement");
    assert_eq!(figures.len(), 2);
    for figure in &figures {
        assert_eq!(
            figure.compact_chars,
            pharmakos_gateway::surface::planning::MAX_PLAYBOOK_CHARS
        );
        match figure.end_to_end {
            full::EndToEnd::OverCap { bytes } => assert!(
                bytes > pharmakos_gateway::frame::MAX_MESSAGE_BYTES,
                "{figure:?}"
            ),
            full::EndToEnd::Measured { .. } => {
                panic!("a request past one message was sent: {figure:?}")
            }
        }
    }
    let bytes: Vec<usize> = figures.iter().map(|figure| figure.compact_bytes).collect();
    assert!(
        matches!(bytes.as_slice(), [ascii, four] if four > ascii),
        "four bytes a character: {bytes:?}"
    );
}

/// The decision tick's kinds measure what they name (`cert3`'s B3 and B4):
/// the condition kind on every decision tick, and the `toward` kind on the
/// ticks a stepping stone's step starts, each spending counted units over its
/// baseline's; and the notice prints them. A debug build and a short segment,
/// so the nanoseconds are not asserted.
#[test]
fn the_decision_kinds_read_their_own_ticks() {
    use pharmakos_bench::kinds::{self, Kind, KindPlan, Tables};
    let root = workspace_root();
    let tables = Tables::load(&root).expect("the tables");
    let plan = KindPlan {
        // Two hundred ticks: forty decision ticks.
        segment_ms: 10_000,
        runs: 1,
    };
    let condition =
        kinds::measure_kind(Kind::Condition, BENCH_SEED, plan, &tables).expect("the condition");
    assert_eq!(condition.ticks, 40, "{condition:?}");
    assert!(condition.units > 0, "{condition:?}");
    let toward = kinds::measure_kind(Kind::Toward, BENCH_SEED, plan, &tables).expect("toward");
    assert!(toward.ticks >= 1, "{toward:?}");
    assert!(
        toward.units > 0,
        "a stepping stone's resolve is counted: {toward:?}"
    );
    // What ran, read back: one run of two hundred ticks.
    assert_eq!((condition.runs, condition.match_ticks), (1, 200));
    let figures = vec![condition, toward];
    let line = kinds::kinds_notice(&figures, 100, BENCH_SEED, plan, "linux");
    assert!(
        line.starts_with("::notice title=decision tick by kind (0x00000000ca5caded)::on linux, "),
        "{line}"
    );
    assert!(line.contains("; toward "), "{line}");
    let seeds = [
        (BENCH_SEED, (figures.clone(), 100)),
        (pharmakos_sim::DETERMINISM_MATCH_SEED, (figures, 100)),
    ];
    for certify in [Certify::On, Certify::Off] {
        let lines = pharmakos_bench::certify::kinds_lines(&root, &seeds, plan, certify, "linux")
            .expect("the kinds' lines");
        assert_eq!(lines.len(), 6, "{lines:?}");
        no_line_certifies(&lines, "the kinds");
        assert!(
            lines
                .iter()
                .all(|line| line.contains(" 1 runs a match of the certifying 3")),
            "{lines:?}"
        );
    }
}

/// The watch segment reads what it names (`cert3`'s B5): every seat's
/// keyframe completes through `get_view` and its deltas follow, the
/// sightings are counted against the table's room, and the breach probe runs
/// at the segment's end. A debug build, so nothing is timed worth reading.
#[test]
fn the_watch_segment_polls_counts_and_probes() {
    use pharmakos_bench::g3::watch;
    let root = workspace_root();
    // Sixty ticks: room for the keyframe's pages and a few deltas.
    let report = watch::measure(&root, BENCH_SEED, 3_000).expect("a measurement");
    assert_eq!(report.ticks, 60);
    assert_eq!(report.listings.len(), 3);
    for listings in &report.listings {
        assert!(
            listings.first().is_some_and(|first| first.keyframe),
            "{listings:?}"
        );
        assert!(
            listings.iter().filter(|listing| !listing.keyframe).count() >= 1,
            "a delta follows the keyframe: {listings:?}"
        );
        assert!(
            listings.iter().all(|listing| listing.entities > 0),
            "a seat always sees its own core: {listings:?}"
        );
    }
    assert!(report.sighting_room > 0);
    assert_eq!(report.forgotten.len(), 3, "a forgotten count a seat");
    assert_eq!(report.breach.len(), 1, "the segment's end probes once");
    assert!(
        report.breach.iter().all(|probe| probe.ways > 0),
        "{:?}",
        report.breach
    );
    // The expansions are the sim's own count: a search that spent its bound
    // reads one past it, and no search reads further.
    let bound = pharmakos_sim::attack::BREACH_EXPANSIONS;
    let past_bound = bound
        .checked_add(1)
        .expect("the bound is far below u32::MAX");
    for probe in &report.breach {
        assert!(probe.most_expansions <= past_bound, "{probe:?}");
        assert!(
            probe.total_expansions >= u64::from(probe.most_expansions),
            "{probe:?}"
        );
        assert_eq!(
            probe.exhausted > 0,
            probe.most_expansions > bound,
            "a search is exhausted exactly when it read past its bound: {probe:?}"
        );
        let searched = [probe.found, probe.exhausted, probe.no_way]
            .iter()
            .try_fold(0_u64, |sum, count| sum.checked_add(*count))
            .expect("the outcomes of a minute's searches fit a u64");
        assert_eq!(
            searched, probe.sealed,
            "every sealed way is searched: {probe:?}"
        );
    }
    let line = watch::view_notice(&report, "linux");
    assert!(
        line.starts_with("::notice title=G3' view feed bytes::on linux, "),
        "{line}"
    );
    for certify in [Certify::On, Certify::Off] {
        let lines = pharmakos_bench::certify::watch_lines(&root, &report, certify, "linux")
            .expect("the watch's lines");
        assert_eq!(lines.len(), 6, "{lines:?}");
        no_line_certifies(&lines, "the watch");
    }
}

/// A run that is not a certifying one says so on every line (`cert3` fix
/// passes 1 and 2, items A1 and M2): P1's whole certifying output at one call
/// a figure, asked to certify and not asked, every line run through the one
/// predicate, and none passes as certifying; and the notices speak in the
/// published role, never claiming to be evidence for a gate. A debug build,
/// so nothing is timed worth reading.
#[test]
fn no_line_of_a_cut_or_unasked_p1_run_passes_as_certifying() {
    let root = workspace_root();
    let plan = QuickPlan {
        warmup: 0,
        calls: 1,
    };
    for certify in [Certify::On, Certify::Off] {
        let run =
            pharmakos_bench::certify::p1_lines(&root, plan, certify, "linux").expect("a cut run");
        assert!(run.failure.is_none(), "{:?}", run.failure);
        let lines = run.lines;
        assert!(lines.len() > 10, "{lines:?}");
        for line in &lines {
            assert!(!provenance::is_certifying(line), "{certify:?}: {line}");
            assert!(line.contains(provenance::CUT_BANNER), "{certify:?}: {line}");
            assert!(
                line.contains(" 1 timed calls a figure of the certifying 2000"),
                "{certify:?}: {line}"
            );
            for foreign in provenance::Role::Published.foreign_phrases() {
                assert!(!line.contains(foreign), "{certify:?}: {line}");
            }
            assert!(!line.contains("never gated"), "{certify:?}: {line}");
        }
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("::notice title=P1 E12 seed rule::")),
            "{lines:?}"
        );
    }
}

/// A watched segment plays the bare segment's chain (`cert3` fix pass 1,
/// C6): the polls, the sightings' reads and the breach probes read the world
/// between ticks and change nothing in it, so the watch's chain digest is the
/// RAIDS segment's on the same seed. A debug build, so nothing is timed.
#[test]
fn a_watched_segment_ends_on_the_bare_segments_chain() {
    use pharmakos_bench::g3::layout::Quarters;
    use pharmakos_bench::g3::segment::{self, GateLoad, SegmentPlan};
    use pharmakos_bench::g3::watch;
    let root = workspace_root();
    let watched = watch::measure(&root, BENCH_SEED, 3_000).expect("a watched segment");
    let bare = segment::measure(
        &root,
        SegmentPlan {
            seed: BENCH_SEED,
            segment_ms: 3_000,
            craters_per_second: 20,
            probe_sight: false,
            share: Quarters::FULL,
            load: GateLoad::RAIDS,
        },
        None,
    )
    .expect("a bare segment");
    assert_eq!(watched.ticks, bare.ticks);
    assert_eq!(watched.chain_digest, bare.chain_digest);
}

/// The covering kinds' resolve serves walks, and the far kind's serves
/// longer ones (`cert3` fix pass 3, N1 and N2 (c)). On each bench seed, from
/// seat 0's core column and from its far point (`hosted::far_point`), a
/// `covering` resolve of the nearest uncovered vent: the arm the kinds'
/// steps take (`FeatureSpec::Nearest`, `Ranker::next_in`, `Ground::travel`,
/// `walk_cost_in`). The walks' own work is read from the scratch's public
/// counts (`expansions`, `abstract_expansions` and `visits`, the sum the
/// sim's `targeting::popped` charges), and the far resolve's is strictly
/// more. A second identical resolve in the same scratch counts exactly what
/// the first counted: the walks are served again, never read back from an
/// earlier one, so a resolver that reused a served route, or ranked by a row
/// scan with no walk, would fail here.
#[test]
fn a_covering_resolve_serves_longer_walks_from_the_far_point() {
    use pharmakos_bench::hosted::{self, Opening};
    use pharmakos_sim::features::FeatureKind;
    use pharmakos_sim::math::fixed::Fx;
    use pharmakos_sim::pathing::Scratch;
    use pharmakos_sim::tables::SeatId;
    use pharmakos_sim::targeting::{FeatureSpec, cover_counted};
    let root = workspace_root();
    let rules = quick::committed_rules(&root).expect("the committed rules");
    for seed in [BENCH_SEED, pharmakos_sim::DETERMINISM_MATCH_SEED] {
        let host = hosted::open_host(&Opening {
            seed,
            seats: BENCH_SEATS,
            segment_ms: 1_000,
            rules: &rules,
            playbook: "",
        })
        .expect("a host");
        let world = host.world();
        let seat = SeatId::new(0);
        let core = hosted::seat_core(world, seat, &rules).expect("seat 0's core");
        let surface = world.surface();
        let at = |[x, y]: [i32; 2]| {
            let node = surface.node_of(x, y).expect("a standable column");
            let voxels = |value: i32| Fx::from_voxels(i16::try_from(value).expect("on the map"));
            [voxels(x), voxels(y), voxels(surface.top(node))]
        };
        let walked = |scratch: &Scratch| {
            scratch.expansions().expect("a count")
                + scratch.abstract_expansions().expect("a count")
                + scratch.visits().expect("a count")
        };
        let resolve = |scratch: &mut Scratch, from: [i32; 2]| {
            let before = walked(scratch);
            let resolved = cover_counted(
                &world.ground(),
                scratch,
                seat,
                at(from),
                FeatureSpec::Nearest {
                    kind: FeatureKind::Vent,
                    uncovered: true,
                },
            );
            assert!(
                resolved.is_ok(),
                "seed {seed:#x} from {from:?}: {resolved:?}"
            );
            walked(scratch) - before
        };
        let fresh =
            || Scratch::for_map(surface, world.clusters().size()).expect("a scratch for the map");
        let home = [core.at[0], core.at[1]];
        let near = resolve(&mut fresh(), home);
        let mut scratch = fresh();
        let far = resolve(&mut scratch, core.far);
        let again = resolve(&mut scratch, core.far);
        assert!(
            far > near,
            "seed {seed:#x}: the far point {:?} serves {far} pops and visits, the core {home:?} {near}",
            core.far
        );
        assert!(near > 0, "seed {seed:#x}: the near resolve served a walk");
        assert_eq!(
            again, far,
            "seed {seed:#x}: a second resolve serves its walks again"
        );
    }
}

/// The selector variant at the size budget is submittable through the hosted
/// door: its pretty canonical text is past the transport cap, so a seat sends
/// it compact, as `decision::shape_text` writes it.
#[test]
fn the_selector_shape_at_the_budget_seals_through_the_gateway() {
    let root = workspace_root();
    let rules = quick::committed_rules(&root).expect("the committed rules");
    let budget = full::committed_budget(&rules).expect("a budget");
    let curve = decision::measure_curve(
        &rules,
        Shape::FullEvalSelectors,
        &[budget],
        DecisionPlan {
            seed: BENCH_SEED,
            seats: BENCH_SEATS,
            segment_ms: 500,
            runs: 1,
        },
    )
    .expect("a measurement");
    let point = curve.first().expect("one size");
    assert_eq!(point.timing.units_per_seat, budget);
    assert!(point.timing.work_max > 0, "{point:?}");
}
