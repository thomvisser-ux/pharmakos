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
//! **The gate figure** is the same call as QUICK's ([`Fixture::time`]):
//! `Input::new` then `verify` at the depth, on the shape's canonical bytes,
//! against the verifier goldens' fixture (`quick::fixture_scope` and
//! `quick::fixture_snapshot`). `Input::new` is inside the timed region, as
//! `plan-core`'s `verify_jsonc` builds a fresh `Input` for every call (it
//! decodes the bytes, computes the fingerprint and `rules_hash` and reads the
//! limits); the canonicalisation in front of it and the JSON-RPC framing
//! around it are not.
//!
//! **The end-to-end figure** beside it ([`EndToEnd`]) is what a player's
//! editor waits on: `verify_plan` through `Surface::call` in a hosted first
//! Lull (`hosted::PlanningLull`, the golden seed, three seats, the sized
//! table), sent the shape's **compact** text ([`crate::shapes::compact`]), so
//! the canonicalisation, the request's parse and the answer's framing are in
//! it. The shape's interface steps visit the hosted seat's own core, `b_00`,
//! and the answer says whether it qualifies there; a compact text past the
//! gateway's `MAX_PLAYBOOK_CHARS` is marked as over the transport cap and not
//! sent.
//!
//! Each call is timed once, never best-of-N, after untimed warm-up calls of
//! the same input; every notice says how many calls it took. CI's alarm takes
//! 200 a figure; certification takes [`CERT_PLAN`]'s 2 000 (item 158 (2)),
//! through the bench's `p1_certify` test, which `cert3` runs by hand.
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

use pharmakos_gateway::surface::planning::MAX_PLAYBOOK_CHARS;
use pharmakos_proto::gp::api::v1::VerifyReport;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::json::Json;
use pharmakos_sim::hex;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::tables::SeatId;

use crate::decision::{BENCH_SEATS, BENCH_SEED};
use crate::error::Error;
use crate::hosted::PlanningLull;
use crate::quick::{BUDGET_CASE, Case, Fixture, QuickPlan, QuickTiming};
use crate::shapes::{self, Core, Pad, Shape};
use crate::stats::{Summary, micros, millis};

/// Calls a figure of the budget line takes; the line prints it as
/// `planned_calls`, so a run cut short of it is no verdict.
pub const BUDGET_CALLS: usize = 500;

/// The budget line's sample.
pub const BUDGET_PLAN: QuickPlan = QuickPlan {
    warmup: 50,
    calls: BUDGET_CALLS,
};

/// The budget line's inputs: every shape of [`Shape::ALL`] at the committed
/// budget, and `budget_128`. The line prints it as `planned_inputs`.
pub const BUDGET_INPUTS: usize = Shape::ALL.len() + 1;

/// Certification's sample: at least 2 000 timed calls a figure (item 158 (2):
/// the owner's machine is noisy at p99).
pub const CERT_PLAN: QuickPlan = QuickPlan {
    warmup: 100,
    calls: 2_000,
};

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

/// The end-to-end figure: `verify_plan` through the hosted gateway, sent the
/// compact text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum EndToEnd {
    /// Both depths were timed.
    Measured {
        /// `verify_plan` at `quick`.
        quick: Summary,
        /// `verify_plan` at `full`.
        full: Summary,
        /// Whether the FULL report qualified on the hosted seat's view.
        qualifies: bool,
    },
    /// The compact text is past the gateway's `MAX_PLAYBOOK_CHARS`, which
    /// refuses it before anything is verified: not sent.
    OverCap {
        /// Its characters.
        chars: usize,
    },
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
    /// Its compact text's length in bytes.
    pub compact_bytes: usize,
    /// Its compact text's length in characters, what the transport caps.
    pub compact_chars: usize,
    /// Its handlers.
    pub handlers: usize,
    /// QUICK's calls, on the canonical bytes: the gate figure.
    pub quick: Summary,
    /// FULL's calls, on the canonical bytes.
    pub full: Summary,
    /// `verify_plan` through the hosted gateway, on the compact text.
    pub end_to_end: EndToEnd,
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
            let compact = compact_text(&case)?;
            shapes.push(ShapeFigure {
                shape,
                units: *units,
                bytes: case.bytes.len(),
                compact_bytes: compact.len(),
                compact_chars: compact.chars().count(),
                handlers: shape.handlers_at(*units)?,
                quick: sample(&fixture, &case, plan, Depth::Quick)?,
                full: sample(&fixture, &case, plan, Depth::Full)?,
                end_to_end: end_to_end(fixture.rules(), plan, |core| {
                    shapes::canonical(&shape.playbook_at(*units, core)?, shape.name())
                })?,
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

/// A case's canonical bytes as compact text.
fn compact_text(case: &Case) -> Result<String, Error> {
    let text = std::str::from_utf8(&case.bytes).map_err(|error| Error::Codec {
        what: format!("{} as text", case.name),
        reason: error.to_string(),
    })?;
    Ok(shapes::compact(text))
}

/// Seat 0's core in the hosted match: `b_00`, its column and height, and the
/// map's extent.
fn hosted_core(lull: &PlanningLull, rules: &RulesTable) -> Result<Core, Error> {
    let world = crate::hosted::world(lull.surface())?;
    let beacons = world.beacons();
    let at = beacons
        .seats()
        .iter()
        .zip(beacons.positions())
        .find(|(seat, _)| **seat == SeatId::new(0).raw())
        .map(|(_, at)| at)
        .ok_or_else(|| Error::Layout("seat 0 has no core on the hosted map".to_owned()))?;
    let map = rules
        .message()
        .map
        .as_ref()
        .ok_or_else(|| Error::Rules("no `map` block".to_owned()))?;
    let extent = |size: u32| i32::try_from(size).map_err(|_| Error::Overflow("the map's extent"));
    Ok(Core {
        id: "b_00".to_owned(),
        at: [
            at[0].floor_voxels(),
            at[1].floor_voxels(),
            at[2].floor_voxels(),
        ],
        extent: [extent(map.size_x)?, extent(map.size_y)?],
    })
}

/// The end-to-end figure: a hosted first Lull under `rules`, the canonical
/// text `text` makes for the hosted seat's core turned compact, and
/// `verify_plan` timed at both depths, `plan.warmup` untimed calls before
/// each sample.
///
/// # Errors
///
/// As [`PlanningLull::open`] and [`PlanningLull::time`]; [`Error::Empty`] for
/// zero calls; [`Error::Drift`] for an answer with no `qualifies`.
pub fn end_to_end(
    rules: &RulesTable,
    plan: QuickPlan,
    text: impl Fn(&Core) -> Result<String, Error>,
) -> Result<EndToEnd, Error> {
    if plan.calls == 0 {
        return Err(Error::Empty(
            "an end-to-end sample of zero calls measures nothing".to_owned(),
        ));
    }
    let mut lull = PlanningLull::open(rules, BENCH_SEED, BENCH_SEATS)?;
    let core = hosted_core(&lull, rules)?;
    let compact = shapes::compact(&text(&core)?);
    let chars = compact.chars().count();
    if chars > MAX_PLAYBOOK_CHARS {
        return Ok(EndToEnd::OverCap { chars });
    }
    let mut call = |depth: &str| -> Result<(u128, Json), Error> {
        lull.time(
            "verify_plan",
            &Json::Object(vec![
                ("playbook_jsonc".to_owned(), Json::String(compact.clone())),
                ("depth".to_owned(), Json::String(depth.to_owned())),
            ]),
        )
    };
    let mut summaries: Vec<Summary> = Vec::with_capacity(2);
    let mut qualifies = None;
    for depth in ["quick", "full"] {
        for _ in 0..plan.warmup {
            call(depth)?;
        }
        let mut durations: Vec<u128> = Vec::with_capacity(plan.calls);
        for _ in 0..plan.calls {
            let (ns, result) = call(depth)?;
            durations.push(ns);
            qualifies = match result
                .get("report")
                .and_then(|report| report.get("qualifies"))
            {
                Some(Json::Bool(answer)) => Some(*answer),
                _ => {
                    return Err(Error::Drift(format!(
                        "a verify_plan answer with no `qualifies`: {}",
                        pharmakos_proto::json::write(&result)
                    )));
                }
            };
        }
        summaries.push(Summary::of(durations)?);
    }
    match (summaries.as_slice(), qualifies) {
        ([quick, full], Some(qualifies)) => Ok(EndToEnd::Measured {
            quick: *quick,
            full: *full,
            qualifies,
        }),
        _ => Err(Error::Drift(
            "an end-to-end figure missing a depth".to_owned(),
        )),
    }
}

/// The end-to-end figure's words in a notice.
fn end_to_end_words(figure: &EndToEnd) -> String {
    match figure {
        EndToEnd::Measured {
            quick,
            full,
            qualifies,
        } => format!(
            "verify_plan end to end QUICK {} / {}, FULL {} / {}{}",
            millis(quick.p50_ns),
            millis(quick.p99_ns),
            millis(full.p50_ns),
            millis(full.p99_ns),
            if *qualifies {
                ""
            } else {
                " (does not qualify on the hosted seat's view)"
            }
        ),
        EndToEnd::OverCap { chars } => format!(
            "OVER THE TRANSPORT CAP: {chars} compact characters past MAX_PLAYBOOK_CHARS \
             {MAX_PLAYBOOK_CHARS}, so not sent end to end"
        ),
    }
}

/// QUICK's cost at the transport cap, one [`Pad`]: [`shapes::max_text`] at
/// the committed budget, the gate figure on its canonical bytes and the
/// end-to-end figure on its compact text, exactly `MAX_PLAYBOOK_CHARS`
/// characters. Published, never a budget failure: whether 5 ms must hold at
/// the cap is the owner's (S3's `bench3` fix pass, item 3).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CapFigure {
    /// The padding.
    pub pad: Pad,
    /// The size, in size units: the committed budget.
    pub units: u32,
    /// The canonical bytes' length.
    pub bytes: usize,
    /// The compact text's length in bytes.
    pub compact_bytes: usize,
    /// The compact text's length in characters: `MAX_PLAYBOOK_CHARS`.
    pub compact_chars: usize,
    /// QUICK's calls, on the canonical bytes.
    pub quick: Summary,
    /// FULL's calls, on the canonical bytes.
    pub full: Summary,
    /// `verify_plan` through the hosted gateway, on the compact text.
    pub end_to_end: EndToEnd,
}

/// [`CapFigure`] for each [`Pad`], under the committed table.
///
/// # Errors
///
/// As [`shapes::max_text`], [`check_at_budget`], [`sample`] and
/// [`end_to_end`].
pub fn measure_cap(root: &Path, plan: QuickPlan) -> Result<Vec<CapFigure>, Error> {
    let fixture = Fixture::committed(root)?;
    let units = committed_budget(fixture.rules())?;
    let mut figures: Vec<CapFigure> = Vec::with_capacity(2);
    for pad in [Pad::Ascii, Pad::FourByte] {
        let playbook = shapes::max_text(units, pad, MAX_PLAYBOOK_CHARS)?;
        let text = shapes::canonical(&playbook, "the max_text shape")?;
        let case = Case {
            name: format!("max_text_{}_{units}", pad.name()),
            bytes: text.clone().into_bytes(),
        };
        check_at_budget(&fixture, &case, units)?;
        let compact = shapes::compact(&text);
        figures.push(CapFigure {
            pad,
            units,
            bytes: case.bytes.len(),
            compact_bytes: compact.len(),
            compact_chars: compact.chars().count(),
            quick: sample(&fixture, &case, plan, Depth::Quick)?,
            full: sample(&fixture, &case, plan, Depth::Full)?,
            end_to_end: end_to_end(fixture.rules(), plan, |_| Ok(text.clone()))?,
        });
    }
    Ok(figures)
}

/// One [`CapFigure`]'s notice, word for word. Pure.
#[must_use]
pub fn cap_notice(figure: &CapFigure, plan: QuickPlan, os: &str) -> String {
    format!(
        "::notice title=P1 at the transport cap ({})::on {os}, {}'s structure at {} units, its \
         labels and title padded with {} characters to {} compact characters ({} bytes; {} \
         canonical bytes), {} calls after {} untimed: QUICK {} / {}, FULL {} / {} p50 / p99 on \
         the canonical bytes; {}. Published, never a budget failure",
        figure.pad.name(),
        Shape::RouteGuards24.name(),
        figure.units,
        figure.pad.name(),
        figure.compact_chars,
        figure.compact_bytes,
        figure.bytes,
        plan.calls,
        plan.warmup,
        millis(figure.quick.p50_ns),
        millis(figure.quick.p99_ns),
        millis(figure.full.p50_ns),
        millis(figure.full.p99_ns),
        end_to_end_words(&figure.end_to_end),
    )
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
                "{} units ({} handlers; {} canonical bytes, {} compact) QUICK {} / {}, FULL {} / \
                 {}; {}",
                figure.units,
                figure.handlers,
                figure.bytes,
                figure.compact_bytes,
                millis(figure.quick.p50_ns),
                millis(figure.quick.p99_ns),
                millis(figure.full.p50_ns),
                millis(figure.full.p99_ns),
                end_to_end_words(&figure.end_to_end),
            )
        })
        .collect();
    format!(
        "::notice title=P1 shape {}::p50 / p99 on {os} by size, {} calls a figure after {} \
         untimed, QUICK and FULL on the canonical bytes (the gate figure): {}. The committed \
         budget is {} units; spec section 16's P1 bars there are QUICK {} and FULL {} p99, \
         published here and never gated",
        shape.name(),
        curve.plan.calls,
        curve.plan.warmup,
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
    /// How many inputs the run measured over, of the [`BUDGET_INPUTS`] the
    /// line plans.
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
/// the inputs measured against [`BUDGET_INPUTS`] and the calls against
/// [`BUDGET_CALLS`] (so a cut run is refused as no verdict),
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
        "{BUDGET_REPORT_PREFIX} budget_units={} inputs={} planned_inputs={BUDGET_INPUTS} calls={} \
         planned_calls={BUDGET_CALLS} quick_p99_ns={} full_p99_ns={}",
        figures.budget_units,
        figures.inputs.len(),
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
            "p1-budget-report v1 budget_units=128 inputs=2 planned_inputs=7 calls=500 \
             planned_calls=500 quick_p99_ns=4000000 full_p99_ns=12000000"
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
                    compact_bytes: 3_000,
                    compact_chars: 3_000,
                    handlers: 0,
                    quick: summary(1_500_000, 2_250_000),
                    full: summary(3_000_000, 4_000_000),
                    end_to_end: EndToEnd::OverCap { chars: 600_000 },
                }],
            }],
        };
        assert_eq!(
            shape_notice(&curve, Shape::LongestRoute, "macos"),
            "::notice title=P1 shape longest_route::p50 / p99 on macos by size, 200 calls a \
             figure after 20 untimed, QUICK and FULL on the canonical bytes (the gate figure): 64 \
             units (0 handlers; 9000 canonical bytes, 3000 compact) QUICK 1.500 ms / 2.250 ms, \
             FULL 3.000 ms / 4.000 ms; OVER THE TRANSPORT CAP: 600000 compact characters past \
             MAX_PLAYBOOK_CHARS 524288, so not sent end to end. The committed budget is 128 \
             units; spec section 16's P1 bars there are QUICK 5.000 ms and FULL 50.000 ms p99, \
             published here and never gated"
        );
    }
}
