// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! P1 FULL, S3's exit (S3's plan, section 5.2 and task `bench3`; E12 and E24).
//!
//! Spec section 16's P1 row: "QUICK ≤5 ms p99; FULL ≤50 ms p99 at the size
//! budget". S1 published QUICK over the committed verifier cases
//! ([`crate::quick`]); this module adds FULL over the same cases, and both
//! depths over the generated worst-shaped playbooks of [`crate::shapes`] at
//! each size of E24's curve, [`crate::shapes::SIZES`], so the size budget is
//! read from measurements rather than guessed.
//!
//! # What one timed call is
//!
//! The same call as QUICK's ([`Fixture::time`]): `Input::new` then `verify`
//! at the depth, against the verifier goldens' fixture
//! (`quick::fixture_scope` and `quick::fixture_snapshot`). `Input::new` is
//! inside the timed region, as `plan-core`'s `verify_jsonc` builds a fresh
//! `Input` for every call (it decodes the bytes, computes the fingerprint and
//! `rules_hash` and reads the limits); the canonicalisation in front of it
//! and the JSON-RPC framing around it are not. Each call is timed once,
//! never best-of-N, after untimed warm-up calls of the same input.
//!
//! # The sized tables
//!
//! A shape at 192 units is over the committed budget, and a playbook over the
//! budget is a refusal (E0110), not the playbook a budget of 192 would
//! accept. So each size is measured under [`sized_rules`]: the committed
//! table with `verifier.size_budget_units` set to that size and nothing else,
//! its own `rules_hash` printed beside its figures. At the committed budget
//! the sized table is the committed table, byte for byte and hash for hash.
//! Before anything is timed, every shape is verified at both depths and must
//! qualify, sitting exactly on its table's budget ([`Error::Drift`]
//! otherwise), so a figure is always a figure of a playbook the verifier
//! accepts.
//!
//! # The alarm and the budget line
//!
//! The notices are published per runner and never gated
//! (`tests/perf_alarm.rs`, `cargo xtask perf-alarms p1`). `cargo xtask
//! p1-budget` (E12) reads the one [`budget_report_line`] the bench's
//! `p1_budget` test prints, the worst p99 of each depth over the shapes at the
//! committed budget and `budget_128`, and fails at the bars doubled.

use std::path::Path;

use pharmakos_proto::gp::api::v1::VerifyReport;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_sim::hex;
use pharmakos_sim::rules::RulesTable;

use crate::error::Error;
use crate::quick::{BUDGET_CASE, Case, Fixture, QuickPlan, QuickTiming};
use crate::shapes::Shape;
use crate::stats::{Summary, micros, millis};

/// The prefix and format version of the line `cargo xtask p1-budget` reads
/// (`xtask/src/budget.rs`, whose unit test holds the two spellings together).
pub const BUDGET_REPORT_PREFIX: &str = "p1-budget-report v1";

/// QUICK's bar at the size budget, spec section 16's P1 row: 5 ms p99.
pub const QUICK_BAR_NS: u128 = 5_000_000;

/// FULL's bar at the size budget, spec section 16's P1 row: 50 ms p99.
pub const FULL_BAR_NS: u128 = 50_000_000;

/// The committed table with `verifier.size_budget_units` set to a size.
#[derive(Clone, Debug)]
pub struct SizedRules {
    /// The size, in size units, the table's budget is set to.
    pub units: u32,
    /// The table.
    pub rules: RulesTable,
    /// Its `rules_hash`.
    pub hash: u64,
}

/// The committed `verifier.size_budget_units`.
///
/// # Errors
///
/// [`Error::Rules`] when the table has no `verifier` block.
pub fn committed_budget(rules: &RulesTable) -> Result<u32, Error> {
    rules
        .message()
        .verifier
        .as_ref()
        .map(|block| block.size_budget_units)
        .ok_or_else(|| Error::Rules("no `verifier` block, so no size budget".to_owned()))
}

/// `committed` with its size budget set to `units`, and nothing else moved.
///
/// # Errors
///
/// [`Error::Rules`] when the table has no `verifier` block, or the sized
/// table is refused at load.
pub fn sized_rules(committed: &RulesTable, units: u32) -> Result<SizedRules, Error> {
    let mut message = committed.message().clone();
    message
        .verifier
        .as_mut()
        .ok_or_else(|| Error::Rules("no `verifier` block, so no size budget".to_owned()))?
        .size_budget_units = units;
    let rules = RulesTable::from_message(&message).map_err(|error| {
        Error::Rules(format!(
            "the table sized at {units} units is refused at load: {error}"
        ))
    })?;
    Ok(SizedRules {
        units,
        hash: rules.rules_hash(),
        rules,
    })
}

/// One shape at one size, both depths.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ShapeFigure {
    /// The shape.
    pub shape: Shape,
    /// Its size, in size units: its table's budget.
    pub units: u32,
    /// Its canonical bytes' length.
    pub bytes: usize,
    /// QUICK's calls.
    pub quick: Summary,
    /// FULL's calls.
    pub full: Summary,
}

/// One size of the curve: its table and every shape at it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SizePoint {
    /// The size, in size units.
    pub units: u32,
    /// The sized table's `rules_hash`.
    pub rules_hash: u64,
    /// Every shape, in [`Shape::ALL`]'s order.
    pub shapes: Vec<ShapeFigure>,
}

/// The whole curve.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Curve {
    /// The committed table's `rules_hash`.
    pub committed_hash: u64,
    /// The committed size budget.
    pub budget_units: u32,
    /// How each input was sampled.
    pub plan: QuickPlan,
    /// Each size, in the order asked.
    pub points: Vec<SizePoint>,
}

/// The case a generated shape is checked as: its name, `<shape>_<units>`,
/// is in none of `quick`'s routing lists, so it is checked against
/// `quick::fixture_scope` and `quick::fixture_snapshot`.
///
/// # Errors
///
/// As [`Shape::bytes`].
pub fn shape_case(shape: Shape, units: u32) -> Result<Case, Error> {
    Ok(Case {
        name: format!("{}_{units}", shape.name()),
        bytes: shape.bytes(units)?,
    })
}

/// The diagnostics' codes, for an error that says why a report is not one a
/// figure may be taken on.
fn codes(report: &VerifyReport) -> String {
    let codes: Vec<&str> = report
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect();
    codes.join(", ")
}

/// Verify `case` at both depths under `fixture` and refuse it unless it
/// qualifies and sits exactly on the table's budget of `units`.
///
/// # Errors
///
/// [`Error::Drift`] naming the depth and the codes, and as
/// [`Fixture::report`].
pub fn check_at_budget(fixture: &Fixture, case: &Case, units: u32) -> Result<(), Error> {
    for depth in [Depth::Quick, Depth::Full] {
        let report = fixture.report(case, depth)?;
        if !report.qualifies || report.size_units != units || report.size_budget != units {
            return Err(Error::Drift(format!(
                "{} at {depth:?} does not qualify on the budget: it qualifies {}, at {} of {} \
                 units, with {}; a figure is taken only on a playbook the verifier accepts at \
                 its size",
                case.name,
                report.qualifies,
                report.size_units,
                report.size_budget,
                codes(&report),
            )));
        }
    }
    Ok(())
}

/// Time `case`'s calls at `depth`: `plan.warmup` untimed, then `plan.calls`
/// timed singly.
///
/// # Errors
///
/// [`Error::Empty`] for zero calls, and as [`Fixture::time`].
pub fn sample(
    fixture: &Fixture,
    case: &Case,
    plan: QuickPlan,
    depth: Depth,
) -> Result<Summary, Error> {
    if plan.calls == 0 {
        return Err(Error::Empty(format!(
            "a {depth:?} sample of zero calls of {} measures nothing",
            case.name
        )));
    }
    for _ in 0..plan.warmup {
        fixture.time(case, depth)?;
    }
    let mut durations: Vec<u128> = Vec::with_capacity(plan.calls);
    for _ in 0..plan.calls {
        durations.push(fixture.time(case, depth)?);
    }
    Summary::of(durations)
}

/// Every shape at every size of `sizes`, each checked, then sampled at QUICK
/// and at FULL.
///
/// # Errors
///
/// [`Error::Empty`] for no size or zero calls; as [`sized_rules`],
/// [`shape_case`] and [`check_at_budget`].
pub fn measure_curve(root: &Path, sizes: &[u32], plan: QuickPlan) -> Result<Curve, Error> {
    if sizes.is_empty() {
        return Err(Error::Empty(
            "a curve over no size measures nothing".to_owned(),
        ));
    }
    let mut fixture = Fixture::committed(root)?;
    let committed = fixture.rules().clone();
    let committed_hash = committed.rules_hash();
    let budget_units = committed_budget(&committed)?;
    let mut points: Vec<SizePoint> = Vec::with_capacity(sizes.len());
    for units in sizes {
        let table = sized_rules(&committed, *units)?;
        fixture = fixture.with_rules(table.rules);
        let mut shapes: Vec<ShapeFigure> = Vec::with_capacity(Shape::ALL.len());
        for shape in Shape::ALL {
            let case = shape_case(shape, *units)?;
            check_at_budget(&fixture, &case, *units)?;
            shapes.push(ShapeFigure {
                shape,
                units: *units,
                bytes: case.bytes.len(),
                quick: sample(&fixture, &case, plan, Depth::Quick)?,
                full: sample(&fixture, &case, plan, Depth::Full)?,
            });
        }
        points.push(SizePoint {
            units: *units,
            rules_hash: table.hash,
            shapes,
        });
    }
    Ok(Curve {
        committed_hash,
        budget_units,
        plan,
        points,
    })
}

/// FULL over the committed cases, as notices word for word. Pure.
#[must_use]
pub fn cases_notice(full: &QuickTiming, os: &str) -> String {
    format!(
        "::notice title=FULL p99::{} p99, {} p50 on {os}, over {} committed verifier cases x {} \
         calls each, timed singly (Input::new then verify at FULL); {BUDGET_CASE}: {} p99, {} \
         p50; slowest case by p99: {} at {}. Spec section 16's P1 bar is 50 ms p99 at the size \
         budget, published here and never gated",
        micros(full.pooled.p99_ns),
        micros(full.pooled.p50_ns),
        full.cases,
        full.calls_per_case,
        micros(full.budget.p99_ns),
        micros(full.budget.p50_ns),
        full.slowest.0,
        micros(full.slowest.1),
    )
}

/// What reproduces the curve, as a notice. Pure.
#[must_use]
pub fn provenance_notice(curve: &Curve, os: &str) -> String {
    let tables: Vec<String> = curve
        .points
        .iter()
        .map(|point| format!("{} units rules_hash {}", point.units, hex(point.rules_hash)))
        .collect();
    format!(
        "::notice title=P1 shapes provenance::on {os}: the generated shapes of crates/bench/src/\
         shapes.rs, checked against the verifier goldens' fixture (quick::fixture_scope and \
         fixture_snapshot), each call timed singly from Input::new through verify, {} calls \
         after {} untimed per shape, size and depth; every shape qualifies on its size's budget \
         before it is timed. The committed table (rules_hash {}, verifier.size_budget_units {}), \
         and at each size that table with the budget row set to the size: {}",
        curve.plan.calls,
        curve.plan.warmup,
        hex(curve.committed_hash),
        curve.budget_units,
        tables.join(", "),
    )
}

/// One shape's curve, as a notice. Pure.
#[must_use]
pub fn shape_notice(curve: &Curve, shape: Shape, os: &str) -> String {
    let sizes: Vec<String> = curve
        .points
        .iter()
        .filter_map(|point| point.shapes.iter().find(|figure| figure.shape == shape))
        .map(|figure| {
            format!(
                "{} units ({} bytes) QUICK {} / {}, FULL {} / {}",
                figure.units,
                figure.bytes,
                millis(figure.quick.p50_ns),
                millis(figure.quick.p99_ns),
                millis(figure.full.p50_ns),
                millis(figure.full.p99_ns),
            )
        })
        .collect();
    format!(
        "::notice title=P1 shape {}::p50 / p99 on {os} by size: {}. The committed budget is {} \
         units; spec section 16's P1 bars there are QUICK {} and FULL {} p99, published here \
         and never gated",
        shape.name(),
        sizes.join("; "),
        curve.budget_units,
        millis(QUICK_BAR_NS),
        millis(FULL_BAR_NS),
    )
}

/// One input of the budget line: a shape at the committed budget, or
/// `budget_128`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BudgetInput {
    /// The case's name.
    pub name: String,
    /// QUICK's calls.
    pub quick: Summary,
    /// FULL's calls.
    pub full: Summary,
}

/// What `cargo xtask p1-budget` reads: every shape at the committed budget,
/// and the committed at-budget case.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BudgetFigures {
    /// The committed size budget.
    pub budget_units: u32,
    /// The committed table's `rules_hash`.
    pub rules_hash: u64,
    /// Calls per input and depth.
    pub calls: usize,
    /// How many inputs the run planned: every shape and `budget_128`.
    pub planned: usize,
    /// The inputs measured, in order.
    pub inputs: Vec<BudgetInput>,
}

impl BudgetFigures {
    /// The input whose p99 at `depth` is the highest, and that p99; `None`
    /// for no input.
    #[must_use]
    pub fn worst(&self, depth: Depth) -> Option<(&str, u128)> {
        self.inputs
            .iter()
            .map(|input| {
                let summary = if depth == Depth::Full {
                    input.full
                } else {
                    input.quick
                };
                (input.name.as_str(), summary.p99_ns)
            })
            .max_by_key(|(_, p99)| *p99)
    }
}

/// Measure every shape at the committed budget and `budget_128`, under the
/// committed table.
///
/// # Errors
///
/// As [`check_at_budget`] and [`sample`], and [`Error::Empty`] when the
/// committed cases hold no `budget_128`.
pub fn measure_budget(root: &Path, plan: QuickPlan) -> Result<BudgetFigures, Error> {
    let fixture = Fixture::committed(root)?;
    let budget_units = committed_budget(fixture.rules())?;
    let mut cases: Vec<Case> = Shape::ALL
        .into_iter()
        .map(|shape| shape_case(shape, budget_units))
        .collect::<Result<_, _>>()?;
    for case in &cases {
        check_at_budget(&fixture, case, budget_units)?;
    }
    let committed = crate::quick::committed_cases(root)?
        .into_iter()
        .find(|case| case.name == BUDGET_CASE)
        .ok_or_else(|| Error::Empty(format!("the committed cases hold no {BUDGET_CASE}")))?;
    cases.push(committed);
    let planned = cases.len();
    let mut inputs: Vec<BudgetInput> = Vec::with_capacity(planned);
    for case in &cases {
        inputs.push(BudgetInput {
            name: case.name.clone(),
            quick: sample(&fixture, case, plan, Depth::Quick)?,
            full: sample(&fixture, case, plan, Depth::Full)?,
        });
    }
    Ok(BudgetFigures {
        budget_units,
        rules_hash: fixture.rules().rules_hash(),
        calls: plan.calls,
        planned,
        inputs,
    })
}

/// The notice beside the budget line, naming the worst inputs. Pure.
///
/// # Errors
///
/// [`Error::Empty`] for figures over no input.
pub fn budget_notice(figures: &BudgetFigures, os: &str) -> Result<String, Error> {
    let empty = || Error::Empty("budget figures over no input".to_owned());
    let (quick_name, quick) = figures.worst(Depth::Quick).ok_or_else(empty)?;
    let (full_name, full) = figures.worst(Depth::Full).ok_or_else(empty)?;
    Ok(format!(
        "::notice title=P1 budget::on {os}, at the committed budget of {} units (rules_hash {}), \
         over {} inputs (every generated shape and {BUDGET_CASE}) x {} calls each: the worst \
         QUICK p99 is {} ({quick_name}) and the worst FULL p99 {} ({full_name}); the bars are \
         QUICK {} and FULL {}, and the alarm fails at twice them",
        figures.budget_units,
        hex(figures.rules_hash),
        figures.inputs.len(),
        figures.calls,
        millis(quick),
        millis(full),
        millis(QUICK_BAR_NS),
        millis(FULL_BAR_NS),
    ))
}

/// The line `cargo xtask p1-budget` reads, after [`BUDGET_REPORT_PREFIX`]:
/// the inputs measured and planned (so a short run is refused), the calls,
/// and the worst p99 of each depth in whole nanoseconds. A run over no input
/// has no worst, and prints `none` for it, which the reader refuses as not a
/// whole number: no verdict, never a pass.
#[must_use]
pub fn budget_report_line(figures: &BudgetFigures) -> String {
    let worst = |depth: Depth| match figures.worst(depth) {
        Some((_, p99)) => p99.to_string(),
        None => "none".to_owned(),
    };
    format!(
        "{BUDGET_REPORT_PREFIX} budget_units={} inputs={} planned_inputs={} calls={} \
         quick_p99_ns={} full_p99_ns={}",
        figures.budget_units,
        figures.inputs.len(),
        figures.planned,
        figures.calls,
        worst(Depth::Quick),
        worst(Depth::Full),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(p50_ns: u128, p99_ns: u128) -> Summary {
        Summary {
            p50_ns,
            p99_ns,
            mean_ns: p50_ns,
            max_ns: p99_ns,
            samples: 200,
        }
    }

    #[test]
    fn a_sized_table_moves_only_its_budget_row() {
        let root = crate::workspace_root();
        let committed = crate::quick::committed_rules(&root).expect("the committed rules");
        let budget = committed_budget(&committed).expect("a budget");
        let same = sized_rules(&committed, budget).expect("the committed size");
        assert_eq!(
            same.hash,
            committed.rules_hash(),
            "at its own budget it is the table"
        );
        let larger = sized_rules(&committed, 256).expect("256");
        assert_ne!(larger.hash, committed.rules_hash());
        let mut expected = committed.message().clone();
        expected
            .verifier
            .as_mut()
            .expect("a verifier block")
            .size_budget_units = 256;
        assert_eq!(larger.rules.message(), &expected, "nothing else moved");
    }

    #[test]
    fn the_budget_line_is_word_for_word() {
        let figures = BudgetFigures {
            budget_units: 128,
            rules_hash: 0x0123,
            calls: 500,
            planned: 2,
            inputs: vec![
                BudgetInput {
                    name: "a".to_owned(),
                    quick: summary(1_000, 4_000_000),
                    full: summary(1_000, 9_000_000),
                },
                BudgetInput {
                    name: "b".to_owned(),
                    quick: summary(1_000, 3_000_000),
                    full: summary(1_000, 12_000_000),
                },
            ],
        };
        assert_eq!(
            budget_report_line(&figures),
            "p1-budget-report v1 budget_units=128 inputs=2 planned_inputs=2 calls=500 \
             quick_p99_ns=4000000 full_p99_ns=12000000"
        );
        assert_eq!(figures.worst(Depth::Quick), Some(("a", 4_000_000)));
        assert_eq!(figures.worst(Depth::Full), Some(("b", 12_000_000)));
        let none = BudgetFigures {
            inputs: Vec::new(),
            ..figures
        };
        assert!(budget_report_line(&none).ends_with("quick_p99_ns=none full_p99_ns=none"));
        assert!(budget_notice(&none, "linux").is_err());
    }

    #[test]
    fn the_shape_notice_is_word_for_word() {
        let curve = Curve {
            committed_hash: 1,
            budget_units: 128,
            plan: QuickPlan {
                warmup: 20,
                calls: 200,
            },
            points: vec![SizePoint {
                units: 64,
                rules_hash: 2,
                shapes: vec![ShapeFigure {
                    shape: Shape::LongestRoute,
                    units: 64,
                    bytes: 9_000,
                    quick: summary(1_500_000, 2_250_000),
                    full: summary(3_000_000, 4_000_000),
                }],
            }],
        };
        assert_eq!(
            shape_notice(&curve, Shape::LongestRoute, "macos"),
            "::notice title=P1 shape longest_route::p50 / p99 on macos by size: 64 units (9000 \
             bytes) QUICK 1.500 ms / 2.250 ms, FULL 3.000 ms / 4.000 ms. The committed budget \
             is 128 units; spec section 16's P1 bars there are QUICK 5.000 ms and FULL 50.000 \
             ms p99, published here and never gated"
        );
    }
}
