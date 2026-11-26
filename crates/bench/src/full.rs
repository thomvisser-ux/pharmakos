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
//! editor waits on: `verify_plan` in a hosted first Lull
//! (`hosted::PlanningLull`, three seats, the sized table), sent the shape's
//! **compact** text ([`crate::shapes::compact`]), timed from the request text
//! a client writes to the answer's text (`PlanningLull::time_text`): the
//! JSON-RPC parse, the door, the canonicalisation, the verifier and the
//! answer's encode are inside the clock; the socket is not. The shape's
//! interface steps visit the hosted seat's own core, `b_00`, and the answer
//! says whether it qualifies there. A request that one message cannot carry
//! (`frame::travels` on its bytes, as `patch_plan` decides it, decisions-log
//! item 167 (4)) is marked as over the transport cap and not sent.
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
//! The notices are published per runner and gated nowhere
//! (`tests/perf_alarm.rs`, `cargo xtask perf-alarms p1`), and say so; a
//! certifying run's say what gate they are evidence for
//! ([`crate::provenance::Role`]). `cargo xtask
//! p1-budget` (E12) reads the one [`budget_report_line`] the bench's
//! `p1_budget` test prints, the worst p99 of each depth over the shapes at the
//! committed budget and `budget_128`, and the worst end-to-end p99 over the
//! shapes, and fails at the bars doubled.
//!
//! # The verdicts
//!
//! The certifying run (`p1_certify`) prints a verdict per shape at the
//! committed budget ([`budget_verdicts`]) and one for the committed cases
//! ([`cases_verdict`]): QUICK and FULL on the verifier alone against spec
//! section 16's bars, and the end-to-end wait against the editor's own,
//! [`END_TO_END_BAR_NS`] (item 162 (1)). A shape over the end-to-end bar
//! prints where its wait goes: each depth's end-to-end p50 and p99 beside the
//! verifier's own on the same playbook, and the rest, which is the request's
//! parse, the door, the canonicalisation and the answer's encode.

use std::path::Path;

use pharmakos_gateway::frame::MAX_MESSAGE_BYTES;
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

/// The budget line's end-to-end inputs: every shape of [`Shape::ALL`] at the
/// committed budget, sent compact through the hosted gateway. The line prints
/// it as `planned_e2e_inputs`, so a shape the transport refused is a short
/// run and no verdict.
pub const BUDGET_END_TO_END_INPUTS: usize = Shape::ALL.len();

/// Certification's sample: at least 2 000 timed calls a figure (item 158 (2):
/// the owner's machine is noisy at p99).
pub const CERT_PLAN: QuickPlan = QuickPlan {
    warmup: 100,
    calls: 2_000,
};

/// The prefix and format version of the line `cargo xtask p1-budget` reads
/// (`xtask/src/budget.rs`, whose unit test holds the two spellings together).
pub const BUDGET_REPORT_PREFIX: &str = "p1-budget-report v2";

/// QUICK's bar at the size budget, spec section 16's P1 row: 5 ms p99.
pub const QUICK_BAR_NS: u128 = 5_000_000;

/// FULL's bar at the size budget, spec section 16's P1 row: 50 ms p99.
pub const FULL_BAR_NS: u128 = 50_000_000;

/// The editor's end-to-end bar at the size budget: `verify_plan` through the
/// hosted gateway, 50 ms p99, on every shape P1 certifies (decisions-log item
/// 162 (1); S3's plan, section 5.2's amendment). It is held at both depths: a
/// check the editor asks for is one or the other, and the player waits on
/// either.
pub const END_TO_END_BAR_NS: u128 = 50_000_000;

/// The bars a P1 verdict holds a figure to, each in nanoseconds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bars {
    /// QUICK's p99 on the verifier alone.
    pub quick: u128,
    /// FULL's p99 on the verifier alone.
    pub full: u128,
    /// The end-to-end p99 at either depth.
    pub end_to_end: u128,
}

impl Bars {
    /// P1's bars: spec section 16's two and item 162 (1)'s end-to-end one.
    pub const P1: Bars = Bars {
        quick: QUICK_BAR_NS,
        full: FULL_BAR_NS,
        end_to_end: END_TO_END_BAR_NS,
    };
}

/// Where a figure stands against its bar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    /// At or below the bar.
    Within,
    /// Above it.
    Over,
}

impl Side {
    /// `figure` against `bar`: at the bar is within it.
    #[must_use]
    pub const fn of(figure: u128, bar: u128) -> Side {
        if figure <= bar {
            Side::Within
        } else {
            Side::Over
        }
    }

    /// The verdict's word.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Side::Within => "within",
            Side::Over => "OVER",
        }
    }
}

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
    /// The request carrying the compact text is past what one message
    /// carries (`frame::travels`), so the socket would close on it before
    /// anything is verified: not sent.
    OverCap {
        /// The request's bytes, as a client writes it.
        bytes: usize,
    },
}

impl EndToEnd {
    /// The higher of the two depths' p99, or `None` for a text that was not
    /// sent, which has no end-to-end figure.
    #[must_use]
    pub fn worst_p99_ns(&self) -> Option<u128> {
        match self {
            EndToEnd::Measured { quick, full, .. } => Some(quick.p99_ns.max(full.p99_ns)),
            EndToEnd::OverCap { .. } => None,
        }
    }
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
/// map's extent ([`crate::hosted::seat_core`]).
fn hosted_core(lull: &PlanningLull, rules: &RulesTable) -> Result<Core, Error> {
    crate::hosted::seat_core(crate::hosted::world(lull.surface())?, SeatId::new(0), rules)
}

/// `verify_plan`'s parameters for `compact` at `depth`.
fn verify_params(compact: &str, depth: &str) -> Json {
    Json::Object(vec![
        (
            "playbook_jsonc".to_owned(),
            Json::String(compact.to_owned()),
        ),
        ("depth".to_owned(), Json::String(depth.to_owned())),
    ])
}

/// Whether `compact`'s `verify_plan` request is one message carries: its
/// bytes as a client writes it, and the frame's answer
/// ([`crate::hosted::travels`]). The deeper depth's name is the longer, so
/// FULL's request decides.
#[must_use]
pub fn request_travels(compact: &str) -> (usize, bool) {
    crate::hosted::travels("verify_plan", verify_params(compact, "full"))
}

/// The end-to-end figure on [`BENCH_SEED`]'s map ([`end_to_end_on`]).
///
/// # Errors
///
/// As [`end_to_end_on`].
pub fn end_to_end(
    rules: &RulesTable,
    plan: QuickPlan,
    text: impl Fn(&Core) -> Result<String, Error>,
) -> Result<EndToEnd, Error> {
    end_to_end_on(rules, plan, BENCH_SEED, text)
}

/// The end-to-end figure: a hosted first Lull under `rules` on `seed`'s map,
/// the canonical text `text` makes for the hosted seat's core turned compact,
/// and `verify_plan` timed at both depths from request text to answer text
/// (`PlanningLull::time_text`), `plan.warmup` untimed calls before each
/// sample. A request one message cannot carry is not sent.
///
/// # Errors
///
/// As [`PlanningLull::open`] and [`PlanningLull::time_text`];
/// [`Error::Empty`] for zero calls; [`Error::Drift`] for an answer with no
/// `qualifies`.
pub fn end_to_end_on(
    rules: &RulesTable,
    plan: QuickPlan,
    seed: u64,
    text: impl Fn(&Core) -> Result<String, Error>,
) -> Result<EndToEnd, Error> {
    if plan.calls == 0 {
        return Err(Error::Empty(
            "an end-to-end sample of zero calls measures nothing".to_owned(),
        ));
    }
    let mut lull = PlanningLull::open(rules, seed, BENCH_SEATS)?;
    let core = hosted_core(&lull, rules)?;
    let compact = shapes::compact(&text(&core)?);
    let (bytes, travels) = request_travels(&compact);
    if !travels {
        return Ok(EndToEnd::OverCap { bytes });
    }
    let mut call = |depth: &str| -> Result<(u128, Json), Error> {
        lull.time_text("verify_plan", verify_params(&compact, depth))
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
        EndToEnd::OverCap { bytes } => format!(
            "OVER THE TRANSPORT CAP: a {bytes}-byte request, past the {MAX_MESSAGE_BYTES} one \
             message carries, so not sent end to end"
        ),
    }
}

/// QUICK's cost at the transport cap, one [`Pad`]: [`shapes::max_text`] at
/// the committed budget, the gate figure on its canonical bytes and the
/// end-to-end figure on its compact text, exactly `MAX_PLAYBOOK_CHARS`
/// characters, whose request is past the 256 KiB one message carries, so the
/// end-to-end figure reads it over the transport cap and sends nothing (the
/// socket's limit against the text cap is settled at hardening, item 162
/// (1)). Published, never a budget failure: whether 5 ms must hold at the cap
/// is the owner's (S3's `bench3` fix pass, item 3).
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
         the canonical bytes; {}. No gate reads this figure: whether 5 ms must hold at the cap \
         is the owner's",
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
pub fn cases_notice(full: &QuickTiming, role: crate::provenance::Role, os: &str) -> String {
    format!(
        "::notice title=FULL p99::{} p99, {} p50 on {os}, over {} committed verifier cases x {} \
         calls each, timed singly (Input::new then verify at FULL); {BUDGET_CASE}: {} p99, {} \
         p50; slowest case by p99: {} at {}. Spec section 16's P1 bar is 50 ms p99 at the size \
         budget; {}",
        micros(full.pooled.p99_ns),
        micros(full.pooled.p50_ns),
        full.cases,
        full.calls_per_case,
        micros(full.budget.p99_ns),
        micros(full.budget.p50_ns),
        full.slowest.0,
        micros(full.slowest.1),
        role.words("P1's FULL bar"),
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
pub fn shape_notice(
    curve: &Curve,
    shape: Shape,
    role: crate::provenance::Role,
    os: &str,
) -> String {
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
         budget is {} units; spec section 16's P1 bars there are QUICK {} and FULL {} p99; \
         {}",
        shape.name(),
        curve.plan.calls,
        curve.plan.warmup,
        sizes.join("; "),
        curve.budget_units,
        millis(QUICK_BAR_NS),
        millis(FULL_BAR_NS),
        role.words("P1's QUICK and FULL bars"),
    )
}

/// A signed nanosecond difference as milliseconds with three places.
fn signed_millis(ns: i128) -> String {
    let sign = if ns < 0 { "-" } else { "" };
    format!("{sign}{}", millis(ns.unsigned_abs()))
}

/// `end` less `part`, signed: a profile's remainder, which a noisy p99 can
/// put below zero.
fn less(end: u128, part: u128) -> Result<i128, Error> {
    let signed = |ns: u128| i128::try_from(ns).map_err(|_| Error::Overflow("a p99 in ns"));
    signed(end)?
        .checked_sub(signed(part)?)
        .ok_or(Error::Overflow("a profile's remainder"))
}

/// Where one depth's end-to-end wait goes: the end-to-end p50 and p99, the
/// verifier's own (the gate figure, the same playbook's canonical bytes), and
/// the rest at p99 (the canonicalisation, the request's parse and the
/// answer's framing), each a p99 less a p99, so a profile and not a sum.
fn depth_profile(depth: &str, end: &Summary, verifier: &Summary) -> Result<String, Error> {
    Ok(format!(
        "{depth} end to end {} / {}, of which the verifier alone {} / {}, the rest {} at p99",
        millis(end.p50_ns),
        millis(end.p99_ns),
        millis(verifier.p50_ns),
        millis(verifier.p99_ns),
        signed_millis(less(end.p99_ns, verifier.p99_ns)?),
    ))
}

/// One shape's P1 verdict at its size, word for word: QUICK and FULL on the
/// verifier alone against [`Bars::quick`] and [`Bars::full`], and the
/// end-to-end wait at both depths against [`Bars::end_to_end`]. A shape
/// over the end-to-end bar carries its profile, where the wait goes; a
/// shape whose text the transport refuses is over it, since the editor cannot
/// have it checked at all. Pure.
///
/// # Errors
///
/// [`Error::Overflow`] for a figure past an `i128`.
pub fn shape_verdict(figure: &ShapeFigure, bars: Bars, os: &str) -> Result<String, Error> {
    let quick = Side::of(figure.quick.p99_ns, bars.quick);
    let full = Side::of(figure.full.p99_ns, bars.full);
    let end_to_end = match &figure.end_to_end {
        EndToEnd::Measured {
            quick: end_quick,
            full: end_full,
            qualifies,
        } => {
            let worst = end_quick.p99_ns.max(end_full.p99_ns);
            let side = Side::of(worst, bars.end_to_end);
            let view = if *qualifies {
                ""
            } else {
                " (the hosted seat's view does not qualify it)"
            };
            let profile = if side == Side::Over {
                format!(
                    "; where the wait goes: {}; {}",
                    depth_profile("QUICK", end_quick, &figure.quick)?,
                    depth_profile("FULL", end_full, &figure.full)?,
                )
            } else {
                String::new()
            };
            (
                side,
                format!(
                    "end to end QUICK {} and FULL {} p99, the worse against {}: {}{view}{profile}",
                    millis(end_quick.p99_ns),
                    millis(end_full.p99_ns),
                    millis(bars.end_to_end),
                    side.word(),
                ),
            )
        }
        EndToEnd::OverCap { bytes } => (
            Side::Over,
            format!(
                "end to end OVER: a {bytes}-byte request, past the {MAX_MESSAGE_BYTES} one \
                 message carries, so the editor cannot have it checked"
            ),
        ),
    };
    let verdict = if [quick, full, end_to_end.0].contains(&Side::Over) {
        "OVER A BAR"
    } else {
        "within every bar"
    };
    Ok(format!(
        "::notice title=P1 verdict ({})::on {os}, at {} units, {verdict}: QUICK {} p99 against \
         {}: {}; FULL {} p99 against {}: {}; {}",
        figure.shape.name(),
        figure.units,
        millis(figure.quick.p99_ns),
        millis(bars.quick),
        quick.word(),
        millis(figure.full.p99_ns),
        millis(bars.full),
        full.word(),
        end_to_end.1,
    ))
}

/// Every shape's verdict at the committed budget, in [`Shape::ALL`]'s order.
///
/// # Errors
///
/// [`Error::Empty`] when the curve holds no point at the committed budget, or
/// a shape is missing from it: no verdict is ever a pass; and as
/// [`shape_verdict`].
pub fn budget_verdicts(curve: &Curve, bars: Bars, os: &str) -> Result<Vec<String>, Error> {
    let point = curve
        .points
        .iter()
        .find(|point| point.units == curve.budget_units)
        .ok_or_else(|| {
            Error::Empty(format!(
                "the curve holds no point at the committed budget of {} units, so it gives no \
                 verdict",
                curve.budget_units
            ))
        })?;
    Shape::ALL
        .into_iter()
        .map(|shape| {
            let figure = point
                .shapes
                .iter()
                .find(|figure| figure.shape == shape)
                .ok_or_else(|| {
                    Error::Empty(format!(
                        "the curve's point at {} units holds no {}",
                        point.units,
                        shape.name()
                    ))
                })?;
            shape_verdict(figure, bars, os)
        })
        .collect()
}

/// The committed cases' verdict at both depths: the pool and the at-budget
/// case, on the verifier alone, against QUICK's and FULL's bars. Pure.
#[must_use]
pub fn cases_verdict(quick: &QuickTiming, full: &QuickTiming, bars: Bars, os: &str) -> String {
    let side = |figure: u128, bar: u128| Side::of(figure, bar).word();
    format!(
        "::notice title=P1 verdict (committed cases)::on {os}, {} cases x {} calls: QUICK pooled \
         {} p99 against {}: {}, {BUDGET_CASE} {}: {}; FULL pooled {} p99 against {}: {}, \
         {BUDGET_CASE} {}: {}",
        quick.cases,
        quick.calls_per_case,
        millis(quick.pooled.p99_ns),
        millis(bars.quick),
        side(quick.pooled.p99_ns, bars.quick),
        millis(quick.budget.p99_ns),
        side(quick.budget.p99_ns, bars.quick),
        millis(full.pooled.p99_ns),
        millis(bars.full),
        side(full.pooled.p99_ns, bars.full),
        millis(full.budget.p99_ns),
        side(full.budget.p99_ns, bars.full),
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
    /// Each shape's end-to-end figure at the committed budget, in
    /// [`Shape::ALL`]'s order: the editor's wait (item 162 (1)), of the
    /// [`BUDGET_END_TO_END_INPUTS`] the line plans.
    pub end_to_end: Vec<(String, EndToEnd)>,
}

impl BudgetFigures {
    /// The shapes whose text was sent end to end, and the worst p99 over them
    /// at either depth, with its shape; `None` for the worst when none was.
    #[must_use]
    pub fn worst_end_to_end(&self) -> (usize, Option<(&str, u128)>) {
        let measured: Vec<(&str, u128)> = self
            .end_to_end
            .iter()
            .filter_map(|(name, figure)| figure.worst_p99_ns().map(|p99| (name.as_str(), p99)))
            .collect();
        (
            measured.len(),
            measured.into_iter().max_by_key(|(_, p99)| *p99),
        )
    }

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
    let mut end_to_end_figures: Vec<(String, EndToEnd)> = Vec::with_capacity(Shape::ALL.len());
    for shape in Shape::ALL {
        end_to_end_figures.push((
            shape.name().to_owned(),
            end_to_end(fixture.rules(), plan, |core| {
                shapes::canonical(&shape.playbook_at(budget_units, core)?, shape.name())
            })?,
        ));
    }
    Ok(BudgetFigures {
        budget_units,
        rules_hash: fixture.rules().rules_hash(),
        calls: plan.calls,
        planned,
        inputs,
        end_to_end: end_to_end_figures,
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
    let end_to_end = match figures.worst_end_to_end() {
        (sent, Some((name, p99))) => format!(
            "the worst end-to-end verify_plan p99 over the {sent} shapes sent is {} ({name})",
            millis(p99)
        ),
        (_, None) => "no shape was sent end to end".to_owned(),
    };
    Ok(format!(
        "::notice title=P1 budget::on {os}, at the committed budget of {} units (rules_hash {}), \
         over {} inputs (every generated shape and {BUDGET_CASE}) x {} calls each: the worst \
         QUICK p99 is {} ({quick_name}) and the worst FULL p99 {} ({full_name}); {end_to_end}; \
         the bars are QUICK {}, FULL {} and end to end {}, and the alarm fails at twice them",
        figures.budget_units,
        hex(figures.rules_hash),
        figures.inputs.len(),
        figures.calls,
        millis(quick),
        millis(full),
        millis(QUICK_BAR_NS),
        millis(FULL_BAR_NS),
        millis(END_TO_END_BAR_NS),
    ))
}

/// The line `cargo xtask p1-budget` reads, after [`BUDGET_REPORT_PREFIX`]:
/// the inputs measured against [`BUDGET_INPUTS`], the shapes sent end to end
/// against [`BUDGET_END_TO_END_INPUTS`] and the calls against
/// [`BUDGET_CALLS`] (so a cut run, or a shape the transport refused, is
/// refused as no verdict), and the worst p99 of each depth and of the
/// end-to-end wait in whole nanoseconds. A run over no input has no worst,
/// and prints `none` for it, which the reader refuses as not a whole number:
/// no verdict, never a pass.
#[must_use]
pub fn budget_report_line(figures: &BudgetFigures) -> String {
    let worst = |depth: Depth| match figures.worst(depth) {
        Some((_, p99)) => p99.to_string(),
        None => "none".to_owned(),
    };
    let (sent, end_to_end) = figures.worst_end_to_end();
    let end_to_end = match end_to_end {
        Some((_, p99)) => p99.to_string(),
        None => "none".to_owned(),
    };
    format!(
        "{BUDGET_REPORT_PREFIX} budget_units={} inputs={} planned_inputs={BUDGET_INPUTS} \
         e2e_inputs={sent} planned_e2e_inputs={BUDGET_END_TO_END_INPUTS} calls={} \
         planned_calls={BUDGET_CALLS} quick_p99_ns={} full_p99_ns={} e2e_p99_ns={end_to_end} \
         quick_bar_ns={QUICK_BAR_NS} full_bar_ns={FULL_BAR_NS} e2e_bar_ns={END_TO_END_BAR_NS}",
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

    /// B5: exactly at the bar is within it; a nanosecond past it is over.
    #[test]
    fn at_the_bar_is_within_and_a_nanosecond_past_is_over() {
        assert_eq!(Side::of(5_000_000, 5_000_000), Side::Within);
        assert_eq!(Side::of(5_000_001, 5_000_000), Side::Over);
        assert_eq!(Side::of(4_999_999, 5_000_000), Side::Within);
        assert_eq!((Side::Within.word(), Side::Over.word()), ("within", "OVER"));
    }

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
            end_to_end: vec![
                (
                    "a".to_owned(),
                    EndToEnd::Measured {
                        quick: summary(1_000, 8_000_000),
                        full: summary(1_000, 31_000_000),
                        qualifies: true,
                    },
                ),
                ("b".to_owned(), EndToEnd::OverCap { bytes: 600_000 }),
            ],
        };
        assert_eq!(
            budget_report_line(&figures),
            "p1-budget-report v2 budget_units=128 inputs=2 planned_inputs=7 e2e_inputs=1 \
             planned_e2e_inputs=6 calls=500 planned_calls=500 quick_p99_ns=4000000 \
             full_p99_ns=12000000 e2e_p99_ns=31000000 quick_bar_ns=5000000 \
             full_bar_ns=50000000 e2e_bar_ns=50000000"
        );
        assert_eq!(figures.worst(Depth::Quick), Some(("a", 4_000_000)));
        assert_eq!(figures.worst(Depth::Full), Some(("b", 12_000_000)));
        assert_eq!(figures.worst_end_to_end(), (1, Some(("a", 31_000_000))));
        let none = BudgetFigures {
            inputs: Vec::new(),
            end_to_end: Vec::new(),
            ..figures
        };
        assert!(budget_report_line(&none).ends_with(
            "e2e_inputs=0 planned_e2e_inputs=6 calls=500 planned_calls=500 quick_p99_ns=none \
             full_p99_ns=none e2e_p99_ns=none quick_bar_ns=5000000 full_bar_ns=50000000 \
             e2e_bar_ns=50000000"
        ));
        assert!(budget_notice(&none, "linux").is_err());
    }

    fn figure(end_to_end: EndToEnd) -> ShapeFigure {
        ShapeFigure {
            shape: Shape::RouteGuards24,
            units: 128,
            bytes: 9_000,
            compact_bytes: 3_000,
            compact_chars: 3_000,
            handlers: 0,
            quick: summary(1_500_000, 2_000_000),
            full: summary(3_000_000, 6_000_000),
            end_to_end,
        }
    }

    #[test]
    fn the_verdict_is_word_for_word() {
        let within = figure(EndToEnd::Measured {
            quick: summary(9_000_000, 12_000_000),
            full: summary(20_000_000, 30_000_000),
            qualifies: true,
        });
        assert_eq!(
            shape_verdict(&within, Bars::P1, "linux").expect("a verdict"),
            "::notice title=P1 verdict (route_guards_24_node)::on linux, at 128 units, within \
             every bar: QUICK 2.000 ms p99 against 5.000 ms: within; FULL 6.000 ms p99 against \
             50.000 ms: within; end to end QUICK 12.000 ms and FULL 30.000 ms p99, the worse \
             against 50.000 ms: within"
        );
    }

    /// The must-trip: the end-to-end bar halved turns a 30 ms wait from
    /// within to over, and the verdict then carries where the wait goes.
    #[test]
    fn a_halved_end_to_end_bar_turns_the_verdict_and_prints_the_profile() {
        let at = figure(EndToEnd::Measured {
            quick: summary(9_000_000, 12_000_000),
            full: summary(20_000_000, 30_000_000),
            qualifies: true,
        });
        let halved = Bars {
            end_to_end: 25_000_000,
            ..Bars::P1
        };
        let verdict = shape_verdict(&at, halved, "linux").expect("a verdict");
        assert_eq!(
            verdict,
            "::notice title=P1 verdict (route_guards_24_node)::on linux, at 128 units, OVER A \
             BAR: QUICK 2.000 ms p99 against 5.000 ms: within; FULL 6.000 ms p99 against 50.000 \
             ms: within; end to end QUICK 12.000 ms and FULL 30.000 ms p99, the worse against \
             25.000 ms: OVER; where the wait goes: QUICK end to end 9.000 ms / 12.000 ms, of \
             which the verifier alone 1.500 ms / 2.000 ms, the rest 10.000 ms at p99; FULL end \
             to end 20.000 ms / 30.000 ms, of which the verifier alone 3.000 ms / 6.000 ms, the \
             rest 24.000 ms at p99"
        );
        // Each of the other two bars halved below its figure turns it too.
        let quick = Bars {
            quick: 1_000_000,
            ..Bars::P1
        };
        assert!(
            shape_verdict(&at, quick, "linux")
                .expect("a verdict")
                .contains("OVER A BAR: QUICK 2.000 ms p99 against 1.000 ms: OVER;")
        );
        let full = Bars {
            full: 3_000_000,
            ..Bars::P1
        };
        assert!(
            shape_verdict(&at, full, "linux")
                .expect("a verdict")
                .contains("FULL 6.000 ms p99 against 3.000 ms: OVER;")
        );
        // A text the transport refuses is over the bar: the editor cannot
        // have it checked.
        let refused = shape_verdict(
            &figure(EndToEnd::OverCap { bytes: 600_000 }),
            Bars::P1,
            "linux",
        )
        .expect("a verdict");
        assert!(refused.contains("OVER A BAR"), "{refused}");
        assert!(
            refused.ends_with(
                "end to end OVER: a 600000-byte request, past the 262144 one message carries, so \
                 the editor cannot have it checked"
            ),
            "{refused}"
        );
    }

    /// One verdict a shape, every shape of `Shape::ALL`, in its order: a
    /// dropped shape (the worst one included) is red (P1c's mutant).
    #[test]
    fn every_shape_has_its_verdict() {
        let shapes: Vec<ShapeFigure> = Shape::ALL
            .into_iter()
            .map(|shape| ShapeFigure {
                shape,
                ..figure(EndToEnd::OverCap { bytes: 1 })
            })
            .collect();
        let curve = Curve {
            committed_hash: 1,
            budget_units: 128,
            plan: QuickPlan {
                warmup: 0,
                calls: 1,
            },
            points: vec![SizePoint {
                units: 128,
                rules_hash: 2,
                shapes,
            }],
        };
        let lines = budget_verdicts(&curve, Bars::P1, "linux").expect("verdicts");
        assert_eq!(lines.len(), Shape::ALL.len());
        for (line, shape) in lines.iter().zip(Shape::ALL) {
            assert!(
                line.starts_with(&format!("::notice title=P1 verdict ({})::", shape.name())),
                "{line}"
            );
        }
    }

    /// No point at the committed budget, or a shape missing from it, gives no
    /// verdict: never a pass.
    #[test]
    fn a_curve_without_the_budget_gives_no_verdict() {
        let curve = Curve {
            committed_hash: 1,
            budget_units: 128,
            plan: QuickPlan {
                warmup: 0,
                calls: 1,
            },
            points: vec![SizePoint {
                units: 64,
                rules_hash: 2,
                shapes: Vec::new(),
            }],
        };
        assert!(matches!(
            budget_verdicts(&curve, Bars::P1, "linux"),
            Err(Error::Empty(_))
        ));
        let at_budget = Curve {
            points: vec![SizePoint {
                units: 128,
                rules_hash: 2,
                shapes: vec![figure(EndToEnd::OverCap { bytes: 1 })],
            }],
            ..curve
        };
        assert!(matches!(
            budget_verdicts(&at_budget, Bars::P1, "linux"),
            Err(Error::Empty(_))
        ));
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
                    end_to_end: EndToEnd::OverCap { bytes: 600_000 },
                }],
            }],
        };
        assert_eq!(
            shape_notice(
                &curve,
                Shape::LongestRoute,
                crate::provenance::Role::Published,
                "macos"
            ),
            "::notice title=P1 shape longest_route::p50 / p99 on macos by size, 200 calls a \
             figure after 20 untimed, QUICK and FULL on the canonical bytes (the gate figure): 64 \
             units (0 handlers; 9000 canonical bytes, 3000 compact) QUICK 1.500 ms / 2.250 ms, \
             FULL 3.000 ms / 4.000 ms; OVER THE TRANSPORT CAP: a 600000-byte request, past the \
             262144 one message carries, so not sent end to end. The committed budget is 128 \
             units; spec section 16's P1 bars there are QUICK 5.000 ms and FULL 50.000 ms p99; \
             published here and gated nowhere (the gate, P1's QUICK and FULL bars, is certified \
             on the owner's machine)"
        );
    }
}
