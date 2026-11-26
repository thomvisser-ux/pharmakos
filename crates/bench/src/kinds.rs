// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A decision tick's nanoseconds per counted unit, kind by kind (S3's plan,
//! section 5.2 item 4; decisions-log items 175 (8) and 196).
//!
//! `dunit` part 2 made every loop a decision runs charge one unit a visit,
//! so ns per counted unit should be **uniform in kind** (item 196 (1)). This
//! module is the check: for each kind of decision work it plays a hosted
//! match whose seats' playbooks (and, for the seam upkeep, whose layout) run
//! that kind, times each decision tick's `Decision` phase alone through the
//! phase stepper (`g3::segment::timed_tick`), reads the counted units the
//! sim published for it (`World::decision_work`, summed over the seats), and
//! answers the kind's nanoseconds per counted unit; then it says whether the
//! kinds agree within [`TOLERANCE_PERCENT`] of their median and which kind
//! is the outlier.
//!
//! # The kinds
//!
//! | [`Kind`] | The match | The decision ticks it is read on |
//! |---|---|---|
//! | [`Kind::Condition`] | every seat seals [`Shape::ConditionHolds`] at the committed budget | every one |
//! | [`Kind::Ranker`] | [`Shape::RankerHolds`]: `nearest` leaves, ranked by the served walk | every one |
//! | [`Kind::Threat`] | [`Shape::ThreatHolds`]: `safest` leaves, through the threat scan | every one |
//! | [`Kind::SeamUpkeep`] | [`Shape::Idle`], and the gate layout's Mine beacons and their drones placed in the lobby, under the G3′ variant | every one |
//! | [`Kind::Commit`] | [`Shape::BuildTargets`] at the committed budget: one interface step at the core, a Generator target a row | those that committed a row and started no step |
//! | [`Kind::Toward`] | [`Shape::Toward`] at [`TOWARD_STEPS`]: stepping stones | those that started a step and placed no beacon |
//! | [`Kind::Deploy`] | the same match | those that placed a beacon and started no step |
//!
//! # The figure
//!
//! Each kind's match is played beside a **baseline**, the same seed and
//! rules with every seat sealing [`Shape::Idle`] (and no layout), so what a
//! decision costs whatever it decides (the seat loop, the tally's reset and
//! read, the interpreter's dispatch) cancels: on each decision tick the kind
//! read on, the kind's `Decision` phase less the baseline's on the same tick,
//! and its counted units less the baseline's. The kind's figure is the sum of
//! the first over the sum of the second, in thousandths of a nanosecond. Both
//! matches are played [`RUNS`] times and each tick keeps its fastest run, the
//! runs having agreed on the chain and the counted units (a run that does
//! not stops the measurement). The two matches' decision ticks fall on the
//! same ticks, since the cadence is the rules'; a pair that does not is
//! [`Error::Drift`].
//!
//! Said honestly: the kinds' worlds part from the baseline's (a commander that
//! walks, a beacon placed), so a difference also carries whatever of that the
//! decision reads, which is counted, and the noise of two runs, which is not;
//! the tolerance is the reader's allowance for both.
//!
//! # The seeds
//!
//! Every kind is measured on [`BENCH_SEED`] and on
//! [`pharmakos_sim::DETERMINISM_MATCH_SEED`], and the second's figure is held
//! to E12's rule: a figure more than [`SEED_GAP_PERCENT`] above the first's
//! means P1 reports both seeds (S3's plan, E12; register S1-49).
//!
//! # Allocations
//!
//! Run under a counting allocator (`tests/g3_real.rs`'s), each kind also
//! reports the allocations of its ticks' opening and phases, the counting
//! allocator's view of a `toward` step (B4 of `cert3`).

use std::path::Path;

use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::runner::Layout;
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::world::{PHASE_ORDER, Phase};

use crate::decision::{BENCH_SEATS, BENCH_SEED};
use crate::error::Error;
use crate::g3::layout::{self, Quarters};
use crate::g3::segment::{self, TickMarks};
use crate::heap::HeapCounter;
use crate::hosted::{self, Opened, Opening};
use crate::shapes::Shape;

/// How long each kind's match is played: two minutes, 480 decision ticks at
/// the committed 250 ms cadence, long enough for a commander to walk to its
/// first stepping stone and deploy it.
pub const KIND_SEGMENT_MS: i32 = 120_000;

/// How many times each match is played; each tick keeps its fastest run.
pub const RUNS: usize = 3;

/// How the kinds' matches are played.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KindPlan {
    /// Each match's one segment, in game milliseconds.
    pub segment_ms: i32,
    /// How many times each match is played; each tick keeps its fastest.
    pub runs: usize,
}

impl KindPlan {
    /// The alarm's and the certifying run's plan: [`KIND_SEGMENT_MS`] and
    /// [`RUNS`].
    pub const MEASURED: KindPlan = KindPlan {
        segment_ms: KIND_SEGMENT_MS,
        runs: RUNS,
    };
}

/// The stepping stones [`Kind::Toward`]'s seats each seal.
pub const TOWARD_STEPS: u32 = 4;

/// How far a kind's figure may sit from the kinds' median, in per cent of the
/// median, and the kinds still read as uniform. A judgement, stated: a unit
/// is a visit, and a node popped from a heap is a longer visit than a
/// condition node read, so the kinds can differ by a small factor and still
/// be a count of work; a kind past half again or half the median is where a
/// loop is charged at the wrong level.
pub const TOLERANCE_PERCENT: u128 = 50;

/// E12's seed rule: the second seed's figure more than this many per cent
/// above the first's means P1 reports both (S3's plan, E12).
pub const SEED_GAP_PERCENT: u128 = 20;

/// One kind of decision work (see the module docs' table).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Kind {
    /// Handlers whose conditions are evaluated in full.
    Condition,
    /// A served-walk ranker: `nearest` selector leaves.
    Ranker,
    /// A threat-ranking selector: `safest` selector leaves.
    Threat,
    /// The Mine beacons' seam upkeep.
    SeamUpkeep,
    /// A row committed at a beacon.
    Commit,
    /// A `toward` site resolved as its step starts.
    Toward,
    /// A beacon deployed.
    Deploy,
}

impl Kind {
    /// Every kind, in the order the notices print them.
    pub const ALL: [Kind; 7] = [
        Kind::Condition,
        Kind::Ranker,
        Kind::Threat,
        Kind::SeamUpkeep,
        Kind::Commit,
        Kind::Toward,
        Kind::Deploy,
    ];

    /// The kind's name, as the notices print it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Kind::Condition => "condition",
            Kind::Ranker => "served-walk ranker",
            Kind::Threat => "threat-ranking selector",
            Kind::SeamUpkeep => "seam upkeep",
            Kind::Commit => "commit",
            Kind::Toward => "toward",
            Kind::Deploy => "deploy",
        }
    }

    /// Whether a decision tick of the kind's match is one the kind is read on.
    #[must_use]
    pub const fn reads(self, marks: TickMarks) -> bool {
        match self {
            Kind::Condition | Kind::Ranker | Kind::Threat | Kind::SeamUpkeep => true,
            Kind::Commit => marks.committed > 0 && marks.started == 0,
            Kind::Toward => marks.started > 0 && marks.placed == 0,
            Kind::Deploy => marks.placed > 0 && marks.started == 0,
        }
    }
}

/// Which table a match is played under.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Table {
    /// The committed table.
    Committed,
    /// The G3′ bench variant, whose power carries the layout.
    Variant,
}

/// What one match is: who seals what, at which size, under which table, with
/// or without the Mine layout.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Spec {
    shape: Shape,
    /// The size, or `None` for the table's committed budget.
    units: Option<u32>,
    table: Table,
    mine_layout: bool,
}

impl Spec {
    /// The baseline every match under `table` is compared with.
    const fn baseline(table: Table) -> Spec {
        Spec {
            shape: Shape::Idle,
            units: Some(1),
            table,
            mine_layout: false,
        }
    }

    /// The match a kind is read from.
    fn of(kind: Kind) -> Spec {
        let at_budget = |shape: Shape| Spec {
            shape,
            units: None,
            table: Table::Committed,
            mine_layout: false,
        };
        match kind {
            Kind::Condition => at_budget(Shape::ConditionHolds),
            Kind::Ranker => at_budget(Shape::RankerHolds),
            Kind::Threat => at_budget(Shape::ThreatHolds),
            Kind::Commit => at_budget(Shape::BuildTargets),
            Kind::SeamUpkeep => Spec {
                shape: Shape::Idle,
                units: Some(1),
                table: Table::Variant,
                mine_layout: true,
            },
            Kind::Toward | Kind::Deploy => Spec {
                shape: Shape::Toward,
                units: Some(TOWARD_STEPS),
                table: Table::Committed,
                mine_layout: false,
            },
        }
    }
}

/// One decision tick as the fastest of its runs saw it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DecisionSample {
    /// The `Decision` phase's time, in nanoseconds.
    pub ns: u128,
    /// The counted units the seats' decisions spent, summed.
    pub units: u64,
    /// The decision events the tick emitted.
    pub marks: TickMarks,
    /// Allocations in the tick's opening and phases, when counted.
    pub allocations: Option<u64>,
}

/// One match, played [`RUNS`] times.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Played {
    /// The table's `rules_hash`.
    pub rules_hash: u64,
    /// The size every seat sealed.
    pub units_per_seat: u32,
    /// The Mine layout's digest, when the match placed one.
    pub layout: Option<u64>,
    /// Every decision tick but the segment's last, in order.
    pub samples: Vec<DecisionSample>,
}

/// The tables a kind's match and its baseline are played under.
#[derive(Clone, Debug)]
pub struct Tables {
    /// The committed table.
    pub committed: RulesTable,
    /// The G3′ bench variant.
    pub variant: RulesTable,
}

impl Tables {
    /// The committed table and the G3′ variant, from the workspace at `root`.
    ///
    /// # Errors
    ///
    /// As [`crate::quick::committed_rules`] and
    /// [`crate::g3::variant::bench_rules`].
    pub fn load(root: &Path) -> Result<Tables, Error> {
        Ok(Tables {
            committed: crate::quick::committed_rules(root)?,
            variant: crate::g3::variant::bench_rules(
                root,
                BENCH_SEATS,
                segment::gate_load(Quarters::FULL)?,
            )?
            .rules,
        })
    }

    const fn of(&self, table: Table) -> &RulesTable {
        match table {
            Table::Committed => &self.committed,
            Table::Variant => &self.variant,
        }
    }
}

/// The gate's layout cut to its Mine beacons and their drones.
fn mine_layout(world: &pharmakos_sim::world::World) -> Result<Layout, Error> {
    let full = layout::layout_at(world, Quarters::FULL)?;
    let beacons: Vec<_> = full
        .beacons
        .into_iter()
        .filter(|beacon| beacon.mandate == MandateKind::Mine)
        .collect();
    if beacons.is_empty() {
        return Err(Error::Layout(
            "the gate's layout holds no Mine beacon".to_owned(),
        ));
    }
    Ok(Layout { beacons })
}

/// The `Decision` phase's time in one timed tick.
fn decision_ns(phases: &[(usize, u128)]) -> Result<u128, Error> {
    phases
        .iter()
        .find(|(index, _)| PHASE_ORDER.get(*index) == Some(&Phase::Decision))
        .map(|(_, ns)| *ns)
        .ok_or_else(|| Error::Drift("a tick ran no Decision phase".to_owned()))
}

/// The seats' counted units on the decision just taken, summed; an
/// eliminated seat is refused, since a kind's match eliminates nobody in its
/// two minutes and a seat out of it would not be the kind's work.
fn seat_units(world: &pharmakos_sim::world::World, seats: u32) -> Result<u64, Error> {
    let mut sum: u64 = 0;
    for seat in 0..usize::try_from(seats).map_err(|_| Error::Overflow("the seat count"))? {
        let figure = world.decision_work(seat).ok_or_else(|| {
            Error::Drift(format!("the world keeps no counted work for seat {seat}"))
        })?;
        let units = crate::decision::read_figure(seat, figure)?.ok_or_else(|| {
            Error::Drift(format!(
                "seat {seat} was eliminated inside a kind's two-minute match"
            ))
        })?;
        sum = sum
            .checked_add(units)
            .ok_or(Error::Overflow("the seats' counted units"))?;
    }
    Ok(sum)
}

/// One run of a match.
#[derive(Clone, Debug)]
struct Run {
    /// Every decision tick but the segment's last.
    samples: Vec<DecisionSample>,
    /// Every tick's state hash.
    chain: Vec<u64>,
    /// The size every seat sealed.
    units_per_seat: u32,
    /// The Mine layout's digest, when one was placed.
    layout: Option<u64>,
}

/// One run of `spec`'s match on `seed`: every decision tick's `Decision`
/// phase, units and marks, and the chain.
fn run_once(
    spec: Spec,
    seed: u64,
    segment_ms: i32,
    rules: &RulesTable,
    heap: Option<&dyn HeapCounter>,
) -> Result<Run, Error> {
    let units = match spec.units {
        Some(units) => units,
        None => crate::full::committed_budget(rules)?,
    };
    let playbook = crate::decision::shape_text(spec.shape, units)?;
    let opening = Opening {
        seed,
        seats: BENCH_SEATS,
        segment_ms,
        rules,
        playbook: &playbook,
    };
    let host = hosted::open_host(&opening)?;
    // A shape whose interface step visits the core and whose anchors stand
    // round it is laid at each seat's own core: the same name, `b_00`, in
    // each seat's view, and a different place on the map.
    let mut playbooks: Vec<String> = Vec::new();
    for seat in hosted::seat_ids(BENCH_SEATS)? {
        playbooks.push(if spec.shape == Shape::BuildTargets {
            let core = hosted::seat_core(host.world(), seat, rules)?;
            crate::shapes::compact(&crate::shapes::canonical(
                &spec.shape.playbook_at(units, &core)?,
                spec.shape.name(),
            )?)
        } else {
            playbook.clone()
        });
    }
    let placed = if spec.mine_layout {
        Some(mine_layout(host.world())?)
    } else {
        None
    };
    let digest = placed.as_ref().map(layout::digest);
    let Opened {
        mut surface,
        units_per_seat,
    } = hosted::attach_each(host, placed.as_ref(), &[], &opening, &playbooks)?;
    let mut samples: Vec<DecisionSample> = Vec::new();
    let mut chain: Vec<u64> = Vec::new();
    while let Some(timed) = segment::timed_tick(&mut surface, heap)? {
        chain.push(timed.report.hash);
        if timed.report.segment_ended {
            break;
        }
        let world = hosted::world(&surface)?;
        if world.is_decision_tick() {
            samples.push(DecisionSample {
                ns: decision_ns(&timed.phase_ns)?,
                units: seat_units(world, BENCH_SEATS)?,
                marks: timed.marks(),
                allocations: timed.allocations.map(|(phases, _)| phases),
            });
        }
    }
    Ok(Run {
        samples,
        chain,
        units_per_seat,
        layout: digest,
    })
}

/// `spec`'s match on `seed`, played [`RUNS`] times, each tick keeping its
/// fastest run.
fn play(
    spec: Spec,
    seed: u64,
    plan: KindPlan,
    tables: &Tables,
    heap: Option<&dyn HeapCounter>,
) -> Result<Played, Error> {
    if plan.runs == 0 {
        return Err(Error::Empty(
            "a kind's match played zero times measures nothing".to_owned(),
        ));
    }
    let base = tables.of(spec.table);
    let units = match spec.units {
        Some(units) => units,
        None => crate::full::committed_budget(base)?,
    };
    let sized = crate::full::sized_rules(base, units)?;
    let mut kept: Option<Run> = None;
    for run in 0..plan.runs {
        let now = run_once(spec, seed, plan.segment_ms, &sized.rules, heap)?;
        let Some(best) = kept.as_mut() else {
            kept = Some(now);
            continue;
        };
        if best.chain != now.chain
            || best.samples.len() != now.samples.len()
            || best
                .samples
                .iter()
                .zip(&now.samples)
                .any(|(was, this)| was.units != this.units || was.marks != this.marks)
        {
            return Err(Error::Drift(format!(
                "run {run} of the {} match played a different chain, counted work or events",
                spec.shape.name()
            )));
        }
        for (was, this) in best.samples.iter_mut().zip(&now.samples) {
            was.ns = was.ns.min(this.ns);
            was.allocations = match (was.allocations, this.allocations) {
                (Some(a), Some(b)) => Some(a.max(b)),
                _ => None,
            };
        }
    }
    let kept =
        kept.ok_or_else(|| Error::Empty("no run of a kind's match was played".to_owned()))?;
    Ok(Played {
        rules_hash: sized.hash,
        units_per_seat: kept.units_per_seat,
        layout: kept.layout,
        samples: kept.samples,
    })
}

/// One kind's figure on one seed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct KindFigure {
    /// The kind.
    pub kind: Kind,
    /// Its match's sized table's `rules_hash`.
    pub rules_hash: u64,
    /// The size its seats sealed.
    pub units_per_seat: u32,
    /// The Mine layout's digest, for the seam upkeep.
    pub layout: Option<u64>,
    /// The decision ticks it was read on.
    pub ticks: usize,
    /// The counted units over the baseline's, summed over those ticks.
    pub units: i128,
    /// The `Decision` phase's time over the baseline's, summed, in ns.
    pub ns: i128,
    /// `ns` over `units`, in thousandths of a nanosecond.
    pub milli_ns_per_unit: i128,
    /// The most allocations one of those ticks made in its opening and
    /// phases, when counted.
    pub most_allocations: Option<u64>,
}

/// `kind`'s figure from its match and the baseline's: pure.
///
/// # Errors
///
/// [`Error::Drift`] when the two hold different decision ticks;
/// [`Error::Empty`] when the kind was read on no tick, or its ticks spent no
/// more counted units than the baseline's, which has no figure;
/// [`Error::Overflow`] for a sum past an `i128`.
pub fn figure(kind: Kind, played: &Played, baseline: &Played) -> Result<KindFigure, Error> {
    if played.samples.len() != baseline.samples.len() {
        return Err(Error::Drift(format!(
            "the {} match took {} decision ticks and its baseline {}",
            kind.name(),
            played.samples.len(),
            baseline.samples.len()
        )));
    }
    let signed = |value: u128| i128::try_from(value).map_err(|_| Error::Overflow("a tick's ns"));
    let mut ticks: usize = 0;
    let mut units: i128 = 0;
    let mut ns: i128 = 0;
    let mut most: Option<u64> = Some(0);
    for (sample, base) in played.samples.iter().zip(&baseline.samples) {
        if !kind.reads(sample.marks) {
            continue;
        }
        ticks = ticks.checked_add(1).ok_or(Error::Overflow("the ticks"))?;
        units = units
            .checked_add(i128::from(sample.units))
            .and_then(|sum| sum.checked_sub(i128::from(base.units)))
            .ok_or(Error::Overflow("the kind's units"))?;
        let (now, then) = (signed(sample.ns)?, signed(base.ns)?);
        ns = ns
            .checked_add(now)
            .and_then(|sum| sum.checked_sub(then))
            .ok_or(Error::Overflow("the kind's ns"))?;
        most = match (most, sample.allocations) {
            (Some(kept), Some(now)) => Some(kept.max(now)),
            _ => None,
        };
    }
    if ticks == 0 {
        return Err(Error::Empty(format!(
            "the {} match held no decision tick the kind is read on",
            kind.name()
        )));
    }
    if units <= 0 {
        return Err(Error::Empty(format!(
            "the {} kind's {ticks} decision ticks spent {units} counted units over the \
             baseline's, so it has no cost per unit",
            kind.name()
        )));
    }
    let milli_ns_per_unit = ns
        .checked_mul(1_000)
        .and_then(|scaled| scaled.checked_div(units))
        .ok_or(Error::Overflow("the kind's ns per unit"))?;
    Ok(KindFigure {
        kind,
        rules_hash: played.rules_hash,
        units_per_seat: played.units_per_seat,
        layout: played.layout,
        ticks,
        units,
        ns,
        milli_ns_per_unit,
        most_allocations: most,
    })
}

/// One kind's figure on `seed`, its match and its baseline played for it
/// alone.
///
/// # Errors
///
/// As [`play`] and [`figure`].
pub fn measure_kind(
    kind: Kind,
    seed: u64,
    plan: KindPlan,
    tables: &Tables,
) -> Result<KindFigure, Error> {
    let spec = Spec::of(kind);
    let baseline = play(Spec::baseline(spec.table), seed, plan, tables, None)?;
    figure(kind, &play(spec, seed, plan, tables, None)?, &baseline)
}

/// Every kind's figure on `seed`, each against its table's baseline.
///
/// # Errors
///
/// As [`play`] and [`figure`].
pub fn measure(
    seed: u64,
    plan: KindPlan,
    tables: &Tables,
    heap: Option<&dyn HeapCounter>,
) -> Result<Vec<KindFigure>, Error> {
    let committed = play(Spec::baseline(Table::Committed), seed, plan, tables, heap)?;
    let variant = play(Spec::baseline(Table::Variant), seed, plan, tables, heap)?;
    let mut figures: Vec<KindFigure> = Vec::with_capacity(Kind::ALL.len());
    let mut stones: Option<Played> = None;
    for kind in Kind::ALL {
        let spec = Spec::of(kind);
        let baseline = match spec.table {
            Table::Committed => &committed,
            Table::Variant => &variant,
        };
        // The toward and deploy kinds read one match.
        let played = if spec.shape != Shape::Toward {
            play(spec, seed, plan, tables, heap)?
        } else if let Some(played) = &stones {
            played.clone()
        } else {
            let played = play(spec, seed, plan, tables, heap)?;
            stones = Some(played.clone());
            played
        };
        figures.push(figure(kind, &played, baseline)?);
    }
    Ok(figures)
}

/// A thousandths figure, signed, with three places.
fn thousandths(milli: i128) -> String {
    let sign = if milli < 0 { "-" } else { "" };
    let magnitude = milli.unsigned_abs();
    format!(
        "{sign}{}.{:03}",
        magnitude.div_euclid(1_000),
        magnitude.rem_euclid(1_000)
    )
}

/// Whether the kinds agree: the median of their figures, and each kind's
/// distance from it in per cent of it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Agreement {
    /// The median figure, in thousandths of a nanosecond a unit.
    pub median_milli: i128,
    /// Each kind and its distance from the median, in per cent of it.
    pub distances: Vec<(Kind, u128)>,
    /// The kind farthest from the median, and its distance.
    pub outlier: (Kind, u128),
}

impl Agreement {
    /// Whether every kind sits within `tolerance` per cent of the median.
    #[must_use]
    pub fn within(&self, tolerance: u128) -> bool {
        self.outlier.1 <= tolerance
    }
}

/// The kinds' agreement over `figures`: pure.
///
/// # Errors
///
/// [`Error::Empty`] for no figure, or a median at or below zero, which no
/// distance can be measured against.
pub fn agreement(figures: &[KindFigure]) -> Result<Agreement, Error> {
    let mut sorted: Vec<i128> = figures
        .iter()
        .map(|figure| figure.milli_ns_per_unit)
        .collect();
    sorted.sort_unstable();
    let median_milli = sorted
        .get(sorted.len().div_euclid(2))
        .copied()
        .ok_or_else(|| Error::Empty("no kind's figure to agree".to_owned()))?;
    let median = match u128::try_from(median_milli) {
        Ok(median) if median > 0 => median,
        _ => {
            return Err(Error::Empty(format!(
                "the kinds' median is {} ns a unit, which no distance is measured against",
                thousandths(median_milli)
            )));
        }
    };
    let mut distances: Vec<(Kind, u128)> = Vec::with_capacity(figures.len());
    for figure in figures {
        let gap = figure
            .milli_ns_per_unit
            .checked_sub(median_milli)
            .ok_or(Error::Overflow("a kind's distance"))?
            .unsigned_abs();
        let percent = gap
            .checked_mul(100)
            .and_then(|scaled| scaled.checked_div(median))
            .ok_or(Error::Overflow("a kind's distance"))?;
        distances.push((figure.kind, percent));
    }
    let outlier = distances
        .iter()
        .copied()
        .max_by_key(|(_, percent)| *percent)
        .ok_or_else(|| Error::Empty("no kind's figure to agree".to_owned()))?;
    Ok(Agreement {
        median_milli,
        distances,
        outlier,
    })
}

/// One seed's notice: every kind's figure. Pure.
#[must_use]
pub fn kinds_notice(figures: &[KindFigure], seed: u64, plan: KindPlan, os: &str) -> String {
    let kinds: Vec<String> = figures
        .iter()
        .map(|figure| {
            let layout = match figure.layout {
                Some(digest) => format!(", Mine layout {}", pharmakos_sim::hex(digest)),
                None => String::new(),
            };
            let allocations = match figure.most_allocations {
                Some(most) => format!(", at most {most} allocations a tick"),
                None => String::new(),
            };
            format!(
                "{} {} ns a unit ({} units over {} ns on {} decision ticks; {} units a seat, \
                 rules_hash {}{layout}{allocations})",
                figure.kind.name(),
                thousandths(figure.milli_ns_per_unit),
                figure.units,
                figure.ns,
                figure.ticks,
                figure.units_per_seat,
                pharmakos_sim::hex(figure.rules_hash),
            )
        })
        .collect();
    format!(
        "::notice title=decision tick by kind (0x{seed:016x})::on {os}, {BENCH_SEATS} seats, \
         each kind's Decision phase and counted units less its baseline's (every seat sealing \
         the idle shape) on the same ticks, fastest of {} runs a tick over a {} s segment: \
         {}",
        plan.runs,
        plan.segment_ms.div_euclid(1_000),
        kinds.join("; "),
    )
}

/// One seed's agreement verdict. Pure.
#[must_use]
pub fn agreement_notice(agreement: &Agreement, seed: u64, os: &str) -> String {
    let distances: Vec<String> = agreement
        .distances
        .iter()
        .map(|(kind, percent)| format!("{} {percent}%", kind.name()))
        .collect();
    let verdict = if agreement.within(TOLERANCE_PERCENT) {
        "the kinds AGREE"
    } else {
        "the kinds DO NOT AGREE"
    };
    format!(
        "::notice title=decision tick kinds agree (0x{seed:016x})::on {os}, {verdict} within \
         {TOLERANCE_PERCENT}% of their median of {} ns a unit; the outlier is {} at {}%; each \
         kind's distance: {}",
        thousandths(agreement.median_milli),
        agreement.outlier.0.name(),
        agreement.outlier.1,
        distances.join(", "),
    )
}

/// The second seed against the first, kind by kind and pooled, against E12's
/// rule. Pure.
///
/// # Errors
///
/// [`Error::Drift`] when the two seeds' figures are not the same kinds in
/// the same order; [`Error::Empty`] for a first figure at or below zero;
/// [`Error::Overflow`] for a sum past an `i128`.
pub fn seed_gap_notice(
    first: &[KindFigure],
    second: &[KindFigure],
    os: &str,
) -> Result<String, Error> {
    if first.len() != second.len()
        || first
            .iter()
            .zip(second)
            .any(|(one, two)| one.kind != two.kind)
    {
        return Err(Error::Drift(
            "the two seeds measured different kinds".to_owned(),
        ));
    }
    let gap = |one: i128, two: i128| -> Result<i128, Error> {
        if one <= 0 {
            return Err(Error::Empty(format!(
                "a first seed's figure of {} ns a unit has no gap",
                thousandths(one)
            )));
        }
        two.checked_sub(one)
            .and_then(|difference| difference.checked_mul(100))
            .and_then(|scaled| scaled.checked_div(one))
            .ok_or(Error::Overflow("a seed gap"))
    };
    let pooled = |figures: &[KindFigure]| -> Result<i128, Error> {
        let (ns, units) = figures
            .iter()
            .try_fold((0_i128, 0_i128), |(ns, units), f| {
                Some((ns.checked_add(f.ns)?, units.checked_add(f.units)?))
            })
            .ok_or(Error::Overflow("the pooled figure"))?;
        ns.checked_mul(1_000)
            .and_then(|scaled| scaled.checked_div(units))
            .ok_or_else(|| Error::Empty("the kinds pooled no unit".to_owned()))
    };
    let threshold = i128::try_from(SEED_GAP_PERCENT).map_err(|_| Error::Overflow("the rule"))?;
    let mut kinds: Vec<String> = Vec::with_capacity(first.len());
    for (one, two) in first.iter().zip(second) {
        // A kind whose first figure is at or below zero spent less time than
        // its baseline on its ticks: its work is inside the clock's noise
        // there, and a per cent of it is no figure. Said in words; the
        // pooled figure, which is the verdict, still needs a positive one.
        kinds.push(if one.milli_ns_per_unit <= 0 {
            format!(
                "{} no gap (the first seed's {} ns a unit is at or below zero: inside the \
                 clock's noise)",
                one.kind.name(),
                thousandths(one.milli_ns_per_unit)
            )
        } else {
            format!(
                "{} {:+}%",
                one.kind.name(),
                gap(one.milli_ns_per_unit, two.milli_ns_per_unit)?
            )
        });
    }
    let (one, two) = (pooled(first)?, pooled(second)?);
    let pooled_gap = gap(one, two)?;
    let verdict = if pooled_gap > threshold {
        "PAST E12's rule: P1 reports both seeds"
    } else {
        "within E12's rule"
    };
    Ok(format!(
        "::notice title=decision tick seed gap::on {os}, 0x{:016x} against 0x{BENCH_SEED:016x}: \
         pooled over the kinds {} against {} ns a unit, {pooled_gap:+}%, {verdict} (more than \
         {SEED_GAP_PERCENT}% above the first seed); by kind {}",
        pharmakos_sim::DETERMINISM_MATCH_SEED,
        thousandths(two),
        thousandths(one),
        kinds.join(", "),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn figure_of(kind: Kind, milli: i128) -> KindFigure {
        KindFigure {
            kind,
            rules_hash: 1,
            units_per_seat: 128,
            layout: None,
            ticks: 10,
            units: 1_000,
            ns: milli,
            milli_ns_per_unit: milli,
            most_allocations: Some(0),
        }
    }

    fn sample(ns: u128, units: u64, marks: TickMarks) -> DecisionSample {
        DecisionSample {
            ns,
            units,
            marks,
            allocations: Some(0),
        }
    }

    #[test]
    fn a_kind_is_its_ticks_less_the_baselines() {
        let quiet = TickMarks::default();
        let deployed = TickMarks {
            placed: 1,
            ..TickMarks::default()
        };
        let played = Played {
            rules_hash: 7,
            units_per_seat: 4,
            layout: None,
            samples: vec![
                sample(5_000, 10, quiet),
                sample(105_000, 1_010, deployed),
                sample(6_000, 10, quiet),
            ],
        };
        let baseline = Played {
            samples: vec![
                sample(4_000, 10, quiet),
                sample(5_000, 10, quiet),
                sample(4_000, 10, quiet),
            ],
            ..played.clone()
        };
        // Deploy reads the one tick that placed: 100 000 ns over 1 000 units.
        let deploy = figure(Kind::Deploy, &played, &baseline).expect("a figure");
        assert_eq!((deploy.ticks, deploy.units, deploy.ns), (1, 1_000, 100_000));
        assert_eq!(deploy.milli_ns_per_unit, 100_000);
        // Toward reads none of them, which is no figure.
        assert!(matches!(
            figure(Kind::Toward, &played, &baseline),
            Err(Error::Empty(_))
        ));
        // A baseline of other ticks is refused.
        let short = Played {
            samples: vec![sample(1, 1, quiet)],
            ..played.clone()
        };
        assert!(matches!(
            figure(Kind::Deploy, &played, &short),
            Err(Error::Drift(_))
        ));
    }

    #[test]
    fn the_kinds_agree_only_within_the_tolerance() {
        let near: Vec<KindFigure> = Kind::ALL
            .iter()
            .zip([100_000, 110_000, 90_000, 100_000, 120_000, 95_000, 105_000])
            .map(|(kind, milli)| figure_of(*kind, milli))
            .collect();
        let agreed = agreement(&near).expect("an agreement");
        assert_eq!(agreed.median_milli, 100_000);
        assert_eq!(agreed.outlier, (Kind::Commit, 20));
        assert!(agreed.within(TOLERANCE_PERCENT));
        // The must-trip: a tolerance below the outlier's distance turns it.
        assert!(!agreed.within(19));
        let mut far = near;
        if let Some(toward) = far.iter_mut().find(|figure| figure.kind == Kind::Toward) {
            toward.milli_ns_per_unit = 400_000;
        }
        let split = agreement(&far).expect("an agreement");
        assert_eq!(split.outlier, (Kind::Toward, 280));
        assert!(!split.within(TOLERANCE_PERCENT));
        assert_eq!(
            agreement_notice(&split, 1, "linux"),
            "::notice title=decision tick kinds agree (0x0000000000000001)::on linux, the kinds \
             DO NOT AGREE within 50% of their median of 105.000 ns a unit; the outlier is toward \
             at 280%; each kind's distance: condition 4%, served-walk ranker 4%, \
             threat-ranking selector 14%, seam upkeep 4%, commit 14%, toward 280%, deploy 0%"
        );
        assert!(agreement(&[]).is_err());
    }

    #[test]
    fn the_seed_gap_is_held_to_e12s_rule() {
        let first: Vec<KindFigure> = Kind::ALL
            .iter()
            .map(|kind| figure_of(*kind, 100_000))
            .collect();
        let near: Vec<KindFigure> = Kind::ALL
            .iter()
            .map(|kind| figure_of(*kind, 120_000))
            .collect();
        let line = seed_gap_notice(&first, &near, "linux").expect("a notice");
        assert!(line.contains("+20%, within E12's rule"), "{line}");
        let far: Vec<KindFigure> = Kind::ALL
            .iter()
            .map(|kind| figure_of(*kind, 123_000))
            .collect();
        let line = seed_gap_notice(&first, &far, "linux").expect("a notice");
        assert!(
            line.contains("+23%, PAST E12's rule: P1 reports both seeds"),
            "{line}"
        );
        let cut: Vec<KindFigure> = far.into_iter().take(2).collect();
        assert!(seed_gap_notice(&first, &cut, "linux").is_err());
        // A kind inside the clock's noise on the first seed says so, and the
        // pooled verdict still stands.
        let mut noisy = first.clone();
        if let Some(deploy) = noisy.iter_mut().find(|figure| figure.kind == Kind::Deploy) {
            deploy.milli_ns_per_unit = -3_656;
        }
        let line = seed_gap_notice(&noisy, &near, "linux").expect("a notice");
        assert!(
            line.contains("deploy no gap (the first seed's -3.656 ns a unit is at or below zero"),
            "{line}"
        );
        assert!(line.contains("within E12's rule"), "{line}");
    }

    #[test]
    fn each_kind_reads_its_own_ticks() {
        let committed = TickMarks {
            committed: 1,
            ..TickMarks::default()
        };
        let started = TickMarks {
            started: 1,
            ..TickMarks::default()
        };
        let both = TickMarks {
            started: 1,
            placed: 1,
            committed: 0,
        };
        assert!(Kind::Commit.reads(committed));
        assert!(!Kind::Commit.reads(started));
        assert!(Kind::Toward.reads(started));
        assert!(
            !Kind::Toward.reads(both),
            "a tick of two kinds is neither's"
        );
        assert!(!Kind::Deploy.reads(both));
        assert!(Kind::Condition.reads(TickMarks::default()));
    }
}
