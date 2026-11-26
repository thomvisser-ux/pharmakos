// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A decision tick's nanoseconds per counted unit, kind by kind (S3's plan,
//! section 5.2 item 4; decisions-log items 175 (8) and 196).
//!
//! `dunit` part 2 made every loop a decision runs charge one unit a visit,
//! so ns per counted unit should be **uniform in kind** (item 196 (1)). This
//! module is the check: for each kind of decision work it plays a hosted
//! match whose seats' playbooks (and, where the table says, whose layout) run
//! that kind, times each decision tick's `Decision` phase alone through the
//! phase stepper (`g3::segment::timed_tick`), reads the counted units the
//! sim published for it (`World::decision_work`, summed over the seats), and
//! answers the kind's nanoseconds per counted unit; then it says whether the
//! kinds agree within this lane's own tolerance, [`TOLERANCE_PERCENT`] of
//! their median, and which kind is the outlier.
//!
//! # The kinds
//!
//! | [`Kind`] | The match | The decision ticks it is read on |
//! |---|---|---|
//! | [`Kind::Condition`] | every seat seals [`Shape::ConditionHolds`] at the committed budget | every one |
//! | [`Kind::OctileRanker`] | [`Shape::RankerHolds`]: `nearest` leaves, each ranking the seat's beacon rows by octile distance from the commander in one scan (`cond.rs`'s `rank`): no walk is served | every one |
//! | [`Kind::Covering`] | [`Shape::Covering`] at [`COVER_STEPS`]: `place_beacon` steps `covering` the nearest uncovered vent, each resolve ranking the vents by the walk the router serves from the commander beside its core | those that started or failed a step and placed no beacon |
//! | [`Kind::CoveringFar`] | [`Shape::CoveringFar`]: the same after the commander walks out toward the map's centre, so its resolves rank the vents by walks from farther out (a vent nearer the centre can still be near) | the same |
//! | [`Kind::Threat`] | [`Shape::ThreatHolds`]: `safest` leaves, through the threat scan | every one |
//! | [`Kind::SeamUpkeep`] | [`Shape::Idle`], and the gate layout's Mine beacons and their drones placed in the lobby, under the G3′ variant | every one |
//! | [`Kind::Commit`] | [`Shape::BuildTargets`] at the committed budget at each seat's own core, on the spread layout (below) | those that committed a row and started no step |
//! | [`Kind::Toward`] | [`Shape::Toward`] at [`TOWARD_STEPS`]: stepping stones, on the spread layout | those that started a step and placed no beacon |
//! | [`Kind::Deploy`] | the same match | those that placed a beacon and started no step |
//!
//! The **spread layout** is the gate's layout's beacons under the G3′
//! variant, with none of their units (so nothing on it moves, fights or
//! digs, and a kind's world parts from its baseline's only by what the
//! kind's playbook does) and each seat's beacons cut to leave room in its
//! share for [`TOWARD_STEPS`] stones. A commit and a deploy scan the tables they write against, so on a
//! world of three cores they spend about a microsecond a tick, at a clock
//! that reads to 100 ns on Windows; on the spread layout they scan ten times
//! the rows, and so clear the measurement floor (below).
//!
//! # The figure
//!
//! Each kind's match is played beside a **baseline**, the same seed, rules
//! and layout with every seat sealing [`Shape::Idle`], so what a decision
//! costs whatever it decides (the seat loop, the tally's reset and read, the
//! interpreter's dispatch, the seam upkeep of the layout's own beacons)
//! cancels (the seam upkeep's own baseline stands on bare ground, since its
//! work is its layout's Mine beacons): on each decision tick the kind is read on, the kind's `Decision`
//! phase less the baseline's on the same tick, and its counted units less
//! the baseline's. The kind's figure is the sum of the first over the sum of
//! the second, in thousandths of a nanosecond. Both matches are played
//! [`RUNS`] times and each tick keeps its fastest run, the runs having agreed
//! on the chain and the counted units (a run that does not stops the
//! measurement). The two matches' decision ticks fall on the same ticks,
//! since the cadence is the rules'; a pair that does not is [`Error::Drift`].
//!
//! Said honestly: the kinds' worlds part from the baseline's (a commander that
//! walks, a beacon placed), so a difference also carries whatever of that the
//! decision reads, which is counted, and the noise of two runs, which is not.
//!
//! # The measurement floor
//!
//! A kind whose summed difference is inside what the clock can tell apart
//! decides nothing. The floor is derived, not picked: the clock's resolution,
//! read at run time ([`clock_resolution`], the smallest non-zero difference
//! two reads of it return over a calibration loop), times the timer reads
//! behind the figure (two a tick: the kind's and the baseline's phase, each a
//! difference of two reads, so each off by under one resolution), times
//! [`FLOOR_MULTIPLE`], so a kind at its floor carries a quantisation error of
//! at most a tenth. A kind under its floor is printed as such with both
//! numbers, and left out of the median and the agreement verdict, which name
//! it.
//!
//! # The seeds
//!
//! Every kind is measured on [`BENCH_SEED`] and on
//! [`pharmakos_sim::DETERMINISM_MATCH_SEED`], and the second's figure against
//! the first's is published as this lane's own figure, with no verdict: E12's
//! seed rule reads P1 FULL's hosted figure (`crate::certify`).
//!
//! # Allocations
//!
//! Run under a counting allocator (`tests/g3_real.rs`'s), each kind also
//! reports the allocations of its ticks' opening and phases, the counting
//! allocator's view of a `toward` step (B4 of `cert3`).

use std::path::Path;
use std::time::Instant;

use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::runner::Layout;
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::tables::UnitKind;
use pharmakos_sim::world::{PHASE_ORDER, Phase, World};

use crate::decision::{BENCH_SEATS, BENCH_SEED};
use crate::error::Error;
use crate::g3::layout::{self, MOST_BEACONS_PER_SEAT, Quarters};
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

/// How a kinds run was sampled, from what it played: the fewest runs any
/// kind's match or baseline played against [`RUNS`], and the fewest ticks a
/// run played against [`KIND_SEGMENT_MS`]'s. The ticks a kind reads have no
/// plan (they follow from the match), so each kind's figure prints its own.
///
/// # Errors
///
/// [`Error::Empty`] for no figure; [`Error::Overflow`] for a segment's ticks
/// past a `usize`.
pub fn sampling(
    figures: &[KindFigure],
    certify: crate::provenance::Certify,
) -> Result<crate::provenance::Sampling, Error> {
    let fewest = |of: fn(&KindFigure) -> usize| {
        figures
            .iter()
            .map(of)
            .min()
            .ok_or_else(|| Error::Empty("a kinds run of no figure".to_owned()))
    };
    let planned_ticks = KIND_SEGMENT_MS
        .checked_div(pharmakos_sim::math::quantity::MS_PER_TICK)
        .and_then(|ticks| usize::try_from(ticks).ok())
        .ok_or(Error::Overflow("a kind's segment"))?;
    Ok(crate::provenance::Sampling {
        certify,
        counts: vec![
            crate::provenance::Count {
                ran: fewest(|figure| figure.runs)?,
                planned: RUNS,
                unit: "runs a match",
            },
            crate::provenance::Count {
                ran: fewest(|figure| figure.match_ticks)?,
                planned: planned_ticks,
                unit: "ticks a match",
            },
        ],
    })
}

/// The `covering` steps [`Kind::Covering`]'s seats each seal.
pub const COVER_STEPS: u32 = 4;

/// The stepping stones [`Kind::Toward`]'s seats each seal.
pub const TOWARD_STEPS: u32 = 4;

/// This lane's own tolerance: how far a kind's figure may sit from the kinds'
/// median, in per cent of the median, and the kinds still read as uniform. A
/// judgement, not a gate and no plan figure: a unit is a visit, and a node
/// popped from a heap carries its sift, which is not counted
/// (`pathing::search::Heap`: bounded by what is already counted, every
/// counted push and pop sifting at most the base-two logarithm of the heap's
/// length), so it is a longer visit than a condition node read; the kinds can
/// differ by a small factor and still be a count of work; a kind past half
/// again or half the median is where a loop is charged at the wrong level,
/// which is what the check is for.
pub const TOLERANCE_PERCENT: u128 = 50;

/// The measurement floor's multiple of the clock's quantisation: at its
/// floor a kind's summed difference carries at most a tenth of quantisation
/// error (see the module docs).
pub const FLOOR_MULTIPLE: u128 = 10;

/// The timer reads behind one tick of a kind's figure: the kind's phase and
/// the baseline's, each the difference of two reads of the clock.
pub const READS_PER_TICK: u128 = 2;

/// Calibration reads of the clock, for [`clock_resolution`].
const CALIBRATION_READS: usize = 200_000;

/// One kind of decision work (see the module docs' table).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Kind {
    /// Handlers whose conditions are evaluated in full.
    Condition,
    /// A ranker by octile distance over the beacon rows: `nearest` selector
    /// leaves. It serves no walk.
    OctileRanker,
    /// A `covering` resolve, its walks served from beside the core.
    Covering,
    /// A `covering` resolve, its walks served from far out.
    CoveringFar,
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
    pub const ALL: [Kind; 9] = [
        Kind::Condition,
        Kind::OctileRanker,
        Kind::Covering,
        Kind::CoveringFar,
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
            Kind::OctileRanker => "octile ranker",
            Kind::Covering => "covering, near walks",
            Kind::CoveringFar => "covering, far walks",
            Kind::Threat => "threat-ranking selector",
            Kind::SeamUpkeep => "seam upkeep",
            Kind::Commit => "commit",
            Kind::Toward => "toward",
            Kind::Deploy => "deploy",
        }
    }

    /// Whether the kind's decisions serve walks through the router: the
    /// covering resolves (each candidate ranked by the walk the router serves)
    /// and the stepping stone's (its grid optimum's pops).
    #[must_use]
    pub const fn serves_walks(self) -> bool {
        matches!(self, Kind::Covering | Kind::CoveringFar | Kind::Toward)
    }

    /// Whether a decision tick of the kind's match is one the kind is read on.
    #[must_use]
    pub const fn reads(self, marks: TickMarks) -> bool {
        match self {
            Kind::Condition | Kind::OctileRanker | Kind::Threat | Kind::SeamUpkeep => true,
            Kind::Covering | Kind::CoveringFar => {
                (marks.started > 0 || marks.failed > 0) && marks.placed == 0
            }
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
    /// The G3′ bench variant, whose power carries a layout.
    Variant,
}

/// What stands on the map besides the cores, placed in the lobby.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ground {
    /// Nothing.
    Bare,
    /// The gate layout's Mine beacons and their drones.
    Mine,
    /// The gate layout's beacons with none of their units, each seat's cut to
    /// leave room for its stepping stones.
    Spread,
}

/// What one match is: who seals what, at which size, under which table, on
/// which ground.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Spec {
    shape: Shape,
    /// The size, or `None` for the table's committed budget.
    units: Option<u32>,
    table: Table,
    ground: Ground,
}

impl Spec {
    /// The baseline a match under `table` on `ground` is compared with.
    const fn baseline(table: Table, ground: Ground) -> Spec {
        Spec {
            shape: Shape::Idle,
            units: Some(1),
            table,
            ground,
        }
    }

    /// The ground this match's baseline stands on: the same as its own, so
    /// what the ground costs a decision cancels, except for the seam upkeep,
    /// whose work is its ground's Mine beacons, so its baseline is bare.
    const fn baseline_ground(self) -> Ground {
        match self.ground {
            Ground::Mine => Ground::Bare,
            other => other,
        }
    }

    /// The match a kind is read from.
    fn of(kind: Kind) -> Spec {
        let at_budget = |shape: Shape| Spec {
            shape,
            units: None,
            table: Table::Committed,
            ground: Ground::Bare,
        };
        match kind {
            Kind::Condition => at_budget(Shape::ConditionHolds),
            Kind::OctileRanker => at_budget(Shape::RankerHolds),
            Kind::Covering => Spec {
                shape: Shape::Covering,
                units: Some(COVER_STEPS),
                table: Table::Committed,
                ground: Ground::Bare,
            },
            Kind::CoveringFar => Spec {
                shape: Shape::CoveringFar,
                units: Some(COVER_STEPS + 1),
                table: Table::Committed,
                ground: Ground::Bare,
            },
            Kind::Threat => at_budget(Shape::ThreatHolds),
            Kind::Commit => Spec {
                shape: Shape::BuildTargets,
                units: None,
                table: Table::Variant,
                ground: Ground::Spread,
            },
            Kind::SeamUpkeep => Spec {
                shape: Shape::Idle,
                units: Some(1),
                table: Table::Variant,
                ground: Ground::Mine,
            },
            Kind::Toward | Kind::Deploy => Spec {
                shape: Shape::Toward,
                units: Some(TOWARD_STEPS),
                table: Table::Variant,
                ground: Ground::Spread,
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
    /// The farthest any seat's commander stood from its own core, in whole
    /// voxels, as the crow flies: where it stood, not what a walk cost.
    pub offset: u32,
}

/// One match, played [`RUNS`] times.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Played {
    /// The table's `rules_hash`.
    pub rules_hash: u64,
    /// The size every seat sealed.
    pub units_per_seat: u32,
    /// The layout's digest, when the match placed one.
    pub layout: Option<u64>,
    /// Every decision tick but the segment's last, in order.
    pub samples: Vec<DecisionSample>,
    /// The runs of the match that were played, each agreeing with the first.
    pub runs: usize,
    /// The ticks one run played, every tick its chain holds.
    pub match_ticks: usize,
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

/// The ground's layout on `world`, or `None` for bare ground.
fn layout_of(world: &World, ground: Ground) -> Result<Option<Layout>, Error> {
    let full = match ground {
        Ground::Bare => return Ok(None),
        Ground::Mine | Ground::Spread => layout::layout_at(world, Quarters::FULL)?,
    };
    let beacons: Vec<_> = match ground {
        Ground::Mine => full
            .beacons
            .into_iter()
            .filter(|beacon| beacon.mandate == MandateKind::Mine)
            .collect(),
        Ground::Spread | Ground::Bare => {
            // A seat's share holds its core and its laid-out beacons; the
            // stones need room of their own.
            let keep = MOST_BEACONS_PER_SEAT
                .checked_sub(TOWARD_STEPS)
                .and_then(|left| usize::try_from(left).ok())
                .ok_or(Error::Overflow("the spread layout's beacons a seat"))?;
            let mut kept: Vec<_> = Vec::new();
            for beacon in full.beacons {
                let held = kept
                    .iter()
                    .filter(|other: &&pharmakos_sim::runner::LayoutBeacon| {
                        other.seat == beacon.seat
                    })
                    .count();
                if held < keep {
                    // Bare of units: rows to scan, and nothing that moves,
                    // fights or digs, so the kind's world and its baseline's
                    // part only by what the kind's playbook does.
                    kept.push(pharmakos_sim::runner::LayoutBeacon {
                        units: Vec::new(),
                        ..beacon
                    });
                }
            }
            kept
        }
    };
    if beacons.is_empty() {
        return Err(Error::Layout(format!(
            "the gate's layout cut for {ground:?} holds no beacon"
        )));
    }
    Ok(Some(Layout { beacons }))
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
fn seat_units(world: &World, seats: u32) -> Result<u64, Error> {
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

/// The farthest any seat's live commander stands from its own core, in whole
/// voxels as the crow flies: where the commander is, not what any walk cost.
///
/// # Errors
///
/// [`Error::Drift`] for a seat with no live commander, which a kind's match
/// never holds (as [`seat_units`] refuses an eliminated seat).
fn commander_offset(world: &World, seats: u32, rules: &RulesTable) -> Result<u32, Error> {
    let units = world.units();
    let commander = UnitKind::Commander.id();
    let mut farthest: u32 = 0;
    for seat in hosted::seat_ids(seats)? {
        let core = hosted::seat_core(world, seat, rules)?.at;
        let at = units
            .seats()
            .iter()
            .zip(units.kinds())
            .zip(units.positions())
            .zip(units.hit_points())
            .find(|(((owner, kind), _), hp)| {
                **owner == seat.raw() && **kind == commander && hp.is_alive()
            })
            .map(|(((_, _), at), _)| [at[0].floor_voxels(), at[1].floor_voxels()]);
        let [x, y] = at.ok_or_else(|| {
            Error::Drift(format!(
                "seat {} has no live commander inside a kind's two-minute match",
                seat.raw()
            ))
        })?;
        let dx = i64::from(x) - i64::from(core[0]);
        let dy = i64::from(y) - i64::from(core[1]);
        let squared = u64::try_from(dx * dx + dy * dy)
            .map_err(|_| Error::Overflow("a commander's distance"))?;
        let distance = u32::try_from(squared.isqrt())
            .map_err(|_| Error::Overflow("a commander's distance"))?;
        farthest = farthest.max(distance);
    }
    Ok(farthest)
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
    /// The layout's digest, when one was placed.
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
        playbooks.push(
            if matches!(spec.shape, Shape::BuildTargets | Shape::CoveringFar) {
                let core = hosted::seat_core(host.world(), seat, rules)?;
                crate::shapes::compact(&crate::shapes::canonical(
                    &spec.shape.playbook_at(units, &core)?,
                    spec.shape.name(),
                )?)
            } else {
                playbook.clone()
            },
        );
    }
    let placed = layout_of(host.world(), spec.ground)?;
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
                offset: commander_offset(world, BENCH_SEATS, rules)?,
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
    let mut runs: usize = 0;
    for run in 0..plan.runs {
        runs = runs.checked_add(1).ok_or(Error::Overflow("the runs"))?;
        let now = run_once(spec, seed, plan.segment_ms, &sized.rules, heap)?;
        let Some(best) = kept.as_mut() else {
            kept = Some(now);
            continue;
        };
        if best.chain != now.chain
            || best.samples.len() != now.samples.len()
            || best.samples.iter().zip(&now.samples).any(|(was, this)| {
                was.units != this.units || was.marks != this.marks || was.offset != this.offset
            })
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
        match_ticks: kept.chain.len(),
        samples: kept.samples,
        runs,
    })
}

/// The clock's resolution: the smallest non-zero difference two reads of
/// [`Instant::now`] return over [`CALIBRATION_READS`] reads, in nanoseconds.
///
/// # Errors
///
/// [`Error::Empty`] when no two reads differed, which leaves no resolution
/// to derive a floor from.
pub fn clock_resolution() -> Result<u128, Error> {
    let mut smallest: Option<u128> = None;
    let mut last = Instant::now();
    for _ in 0..CALIBRATION_READS {
        let now = Instant::now();
        let step = now.duration_since(last).as_nanos();
        if step > 0 && smallest.is_none_or(|kept| step < kept) {
            smallest = Some(step);
        }
        last = now;
    }
    smallest.ok_or_else(|| {
        Error::Empty(format!(
            "{CALIBRATION_READS} reads of the clock never differed, so it has no resolution"
        ))
    })
}

/// The measurement floor for a figure over `ticks` decision ticks at a clock
/// of `resolution_ns`: the reads behind it times the resolution times
/// [`FLOOR_MULTIPLE`].
///
/// # Errors
///
/// [`Error::Overflow`] for a floor past an `i128`.
pub fn floor_ns(resolution_ns: u128, ticks: usize) -> Result<i128, Error> {
    let ticks = u128::try_from(ticks).map_err(|_| Error::Overflow("the ticks"))?;
    resolution_ns
        .checked_mul(READS_PER_TICK)
        .and_then(|ns| ns.checked_mul(ticks))
        .and_then(|ns| ns.checked_mul(FLOOR_MULTIPLE))
        .and_then(|ns| i128::try_from(ns).ok())
        .ok_or(Error::Overflow("the measurement floor"))
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
    /// The layout's digest, when the match placed one.
    pub layout: Option<u64>,
    /// The decision ticks it was read on.
    pub ticks: usize,
    /// The counted units over the baseline's, summed over those ticks.
    pub units: i128,
    /// The `Decision` phase's time over the baseline's, summed, in ns.
    pub ns: i128,
    /// `ns` over `units`, in thousandths of a nanosecond.
    pub milli_ns_per_unit: i128,
    /// The measurement floor `ns` is held to.
    pub floor_ns: i128,
    /// The most allocations one of those ticks made in its opening and
    /// phases, when counted.
    pub most_allocations: Option<u64>,
    /// The nearest and farthest a commander stood from its core on those
    /// ticks, in whole voxels as the crow flies.
    pub offset: (u32, u32),
    /// The fewest and most counted units one of those ticks spent over the
    /// baseline's: for a kind that serves walks, the walks' work as the sim
    /// counted it, a tick at a time.
    pub per_tick: (i128, i128),
    /// The fewer runs the kind's match and its baseline played.
    pub runs: usize,
    /// The fewer ticks a run of the kind's match and of its baseline played.
    pub match_ticks: usize,
}

impl KindFigure {
    /// Whether the kind's summed difference is at or past its floor, so the
    /// figure decides anything.
    #[must_use]
    pub const fn clears_floor(&self) -> bool {
        self.ns >= self.floor_ns
    }
}

/// `kind`'s figure from its match and the baseline's at a clock of
/// `resolution_ns`: pure.
///
/// # Errors
///
/// [`Error::Drift`] when the two hold different decision ticks;
/// [`Error::Empty`] when the kind was read on no tick, or its ticks spent no
/// more counted units than the baseline's, which has no figure;
/// [`Error::Overflow`] for a sum past an `i128`.
pub fn figure(
    kind: Kind,
    played: &Played,
    baseline: &Played,
    resolution_ns: u128,
) -> Result<KindFigure, Error> {
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
    let mut offset: Option<(u32, u32)> = None;
    let mut per_tick: Option<(i128, i128)> = None;
    for (sample, base) in played.samples.iter().zip(&baseline.samples) {
        if !kind.reads(sample.marks) {
            continue;
        }
        ticks = ticks.checked_add(1).ok_or(Error::Overflow("the ticks"))?;
        let tick_units = i128::from(sample.units)
            .checked_sub(i128::from(base.units))
            .ok_or(Error::Overflow("a tick's units"))?;
        units = units
            .checked_add(tick_units)
            .ok_or(Error::Overflow("the kind's units"))?;
        per_tick = Some(match per_tick {
            Some((fewest, most)) => (fewest.min(tick_units), most.max(tick_units)),
            None => (tick_units, tick_units),
        });
        let (now, then) = (signed(sample.ns)?, signed(base.ns)?);
        ns = ns
            .checked_add(now)
            .and_then(|sum| sum.checked_sub(then))
            .ok_or(Error::Overflow("the kind's ns"))?;
        most = match (most, sample.allocations) {
            (Some(kept), Some(now)) => Some(kept.max(now)),
            _ => None,
        };
        offset = Some(match offset {
            Some((near, far)) => (near.min(sample.offset), far.max(sample.offset)),
            None => (sample.offset, sample.offset),
        });
    }
    let (Some(offset), Some(per_tick)) = (offset, per_tick) else {
        return Err(Error::Empty(format!(
            "the {} match held no decision tick the kind is read on",
            kind.name()
        )));
    };
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
        floor_ns: floor_ns(resolution_ns, ticks)?,
        most_allocations: most,
        offset,
        per_tick,
        runs: played.runs.min(baseline.runs),
        match_ticks: played.match_ticks.min(baseline.match_ticks),
    })
}

/// One kind's figure on `seed`, its match and its baseline played for it
/// alone.
///
/// # Errors
///
/// As [`play`], [`clock_resolution`] and [`figure`].
pub fn measure_kind(
    kind: Kind,
    seed: u64,
    plan: KindPlan,
    tables: &Tables,
) -> Result<KindFigure, Error> {
    let spec = Spec::of(kind);
    let baseline = play(
        Spec::baseline(spec.table, spec.baseline_ground()),
        seed,
        plan,
        tables,
        None,
    )?;
    figure(
        kind,
        &play(spec, seed, plan, tables, None)?,
        &baseline,
        clock_resolution()?,
    )
}

/// Every kind's figure on `seed`, each against its baseline, and the clock's
/// resolution they were held to.
///
/// # Errors
///
/// As [`play`], [`clock_resolution`] and [`figure`].
pub fn measure(
    seed: u64,
    plan: KindPlan,
    tables: &Tables,
    heap: Option<&dyn HeapCounter>,
) -> Result<(Vec<KindFigure>, u128), Error> {
    let resolution = clock_resolution()?;
    let mut baselines: Vec<((Table, Ground), Played)> = Vec::new();
    let mut figures: Vec<KindFigure> = Vec::with_capacity(Kind::ALL.len());
    let mut stones: Option<Played> = None;
    for kind in Kind::ALL {
        let spec = Spec::of(kind);
        let key = (spec.table, spec.baseline_ground());
        if !baselines.iter().any(|(held, _)| *held == key) {
            let played = play(
                Spec::baseline(spec.table, spec.baseline_ground()),
                seed,
                plan,
                tables,
                heap,
            )?;
            baselines.push((key, played));
        }
        let baseline = baselines
            .iter()
            .find(|(held, _)| *held == key)
            .map(|(_, played)| played)
            .ok_or_else(|| Error::Drift("a baseline played and not kept".to_owned()))?;
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
        figures.push(figure(kind, &played, baseline, resolution)?);
    }
    Ok((figures, resolution))
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

/// A per-mille figure with one place: `509` is `50.9`.
fn tenths(per_mille: u128) -> String {
    format!("{}.{}", per_mille.div_euclid(10), per_mille.rem_euclid(10))
}

/// Whether the kinds agree: the median of the figures that clear their
/// floors, and each such kind's distance from it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Agreement {
    /// The median figure, in thousandths of a nanosecond a unit.
    pub median_milli: i128,
    /// Each kind and its distance from the median, in thousandths of a
    /// nanosecond a unit.
    pub distances: Vec<(Kind, u128)>,
    /// The kind farthest from the median, and its distance.
    pub outlier: (Kind, u128),
    /// The kinds under their floors, left out of the median and the verdict.
    pub left_out: Vec<Kind>,
}

impl Agreement {
    /// Whether every kind sits within `tolerance` per cent of the median,
    /// compared exactly in integers: `distance * 100 <= tolerance * median`.
    ///
    /// # Errors
    ///
    /// [`Error::Overflow`] when either product passes a `u128`, which has no
    /// exact answer.
    pub fn within(&self, tolerance: u128) -> Result<bool, Error> {
        let median = self.median_milli.unsigned_abs();
        let distance = self
            .outlier
            .1
            .checked_mul(100)
            .ok_or(Error::Overflow("the outlier's distance in per cent"))?;
        let room = tolerance
            .checked_mul(median)
            .ok_or(Error::Overflow("the tolerance's room"))?;
        Ok(distance <= room)
    }

    /// A distance in per mille of the median, for printing to one place.
    fn per_mille(&self, distance: u128) -> Option<u128> {
        distance
            .checked_mul(1_000)
            .and_then(|scaled| scaled.checked_div(self.median_milli.unsigned_abs()))
    }
}

/// The kinds' agreement over `figures`, the kinds under their floors left
/// out: pure.
///
/// # Errors
///
/// [`Error::Empty`] for no figure that clears its floor, or a median at or
/// below zero, which no distance can be measured against.
pub fn agreement(figures: &[KindFigure]) -> Result<Agreement, Error> {
    let cleared: Vec<&KindFigure> = figures
        .iter()
        .filter(|figure| figure.clears_floor())
        .collect();
    let left_out: Vec<Kind> = figures
        .iter()
        .filter(|figure| !figure.clears_floor())
        .map(|figure| figure.kind)
        .collect();
    let mut sorted: Vec<i128> = cleared
        .iter()
        .map(|figure| figure.milli_ns_per_unit)
        .collect();
    sorted.sort_unstable();
    let median_milli = sorted
        .get(sorted.len().div_euclid(2))
        .copied()
        .ok_or_else(|| Error::Empty("no kind's figure clears its floor".to_owned()))?;
    if median_milli <= 0 {
        return Err(Error::Empty(format!(
            "the kinds' median is {} ns a unit, which no distance is measured against",
            thousandths(median_milli)
        )));
    }
    let mut distances: Vec<(Kind, u128)> = Vec::with_capacity(cleared.len());
    for figure in &cleared {
        let gap = figure
            .milli_ns_per_unit
            .checked_sub(median_milli)
            .ok_or(Error::Overflow("a kind's distance"))?
            .unsigned_abs();
        distances.push((figure.kind, gap));
    }
    let outlier = distances
        .iter()
        .copied()
        .max_by_key(|(_, gap)| *gap)
        .ok_or_else(|| Error::Empty("no kind's figure clears its floor".to_owned()))?;
    Ok(Agreement {
        median_milli,
        distances,
        outlier,
        left_out,
    })
}

/// One seed's notice: every kind's figure, each under its floor said so.
/// Pure.
#[must_use]
pub fn kinds_notice(
    figures: &[KindFigure],
    resolution_ns: u128,
    seed: u64,
    plan: KindPlan,
    os: &str,
) -> String {
    let kinds: Vec<String> = figures
        .iter()
        .map(|figure| {
            let layout = match figure.layout {
                Some(digest) => format!(", layout {}", pharmakos_sim::hex(digest)),
                None => String::new(),
            };
            let allocations = match figure.most_allocations {
                Some(most) => format!(", at most {most} allocations a tick"),
                None => String::new(),
            };
            let floor = if figure.clears_floor() {
                format!("{} ns of floor", figure.floor_ns)
            } else {
                format!(
                    "BELOW THE MEASUREMENT FLOOR, {} ns against {} ns: decides nothing",
                    figure.ns, figure.floor_ns
                )
            };
            let walks = if figure.kind.serves_walks() {
                format!(
                    "; the walks' work as the sim counted it, {} to {} units a tick",
                    figure.per_tick.0, figure.per_tick.1
                )
            } else {
                String::new()
            };
            format!(
                "{} {} ns a unit ({} units over {} ns on {} decision ticks, {floor}; {} units a \
                 seat, rules_hash {}{layout}{allocations}; a commander stood {} to {} voxels \
                 from its core, as the crow flies{walks})",
                figure.kind.name(),
                thousandths(figure.milli_ns_per_unit),
                figure.units,
                figure.ns,
                figure.ticks,
                figure.units_per_seat,
                pharmakos_sim::hex(figure.rules_hash),
                figure.offset.0,
                figure.offset.1,
            )
        })
        .collect();
    format!(
        "::notice title=decision tick by kind (0x{seed:016x})::on {os}, {BENCH_SEATS} seats, \
         each kind's Decision phase and counted units less its baseline's (the same ground, \
         every seat sealing the idle shape) on the same ticks, fastest of {} runs a tick over \
         a {} s segment; the clock reads to {resolution_ns} ns, and a kind's floor is that \
         times {READS_PER_TICK} reads a tick times its ticks times {FLOOR_MULTIPLE}: {}{}",
        plan.runs,
        plan.segment_ms.div_euclid(1_000),
        kinds.join("; "),
        if figures.iter().any(|figure| figure.kind.serves_walks()) {
            ". The covering and toward kinds serve walks, and a served walk is charged every \
             loop visit it makes, a pop and every visit that pops nothing; a heap's sift is not \
             counted, bounded by the counted pushes and pops"
        } else {
            ""
        },
    )
}

/// One seed's agreement verdict. Pure.
///
/// # Errors
///
/// As [`Agreement::within`].
pub fn agreement_notice(agreement: &Agreement, seed: u64, os: &str) -> Result<String, Error> {
    let distances: Vec<String> = agreement
        .distances
        .iter()
        .map(|(kind, gap)| match agreement.per_mille(*gap) {
            Some(per_mille) => format!("{} {}%", kind.name(), tenths(per_mille)),
            None => format!("{} past a u128's per cent", kind.name()),
        })
        .collect();
    let verdict = if agreement.within(TOLERANCE_PERCENT)? {
        "the kinds AGREE"
    } else {
        "the kinds DO NOT AGREE"
    };
    let outlier = match agreement.per_mille(agreement.outlier.1) {
        Some(per_mille) => format!("{}%", tenths(per_mille)),
        None => "past a u128's per cent".to_owned(),
    };
    let left_out = if agreement.left_out.is_empty() {
        "no kind left out".to_owned()
    } else {
        let names: Vec<&str> = agreement.left_out.iter().map(|kind| kind.name()).collect();
        format!(
            "left out, under their measurement floors: {}",
            names.join(", ")
        )
    };
    Ok(format!(
        "::notice title=decision tick kinds agree (0x{seed:016x})::on {os}, {verdict} within \
         this lane's own tolerance, {TOLERANCE_PERCENT}% of their median (a judgement, not a \
         gate), of {} ns a unit; the outlier is {} at {outlier}; {left_out}; each kind's \
         distance: {}",
        thousandths(agreement.median_milli),
        agreement.outlier.0.name(),
        distances.join(", "),
    ))
}

/// The second seed against the first, kind by kind and pooled over the kinds
/// that clear their floors: this lane's own published figure, with no
/// verdict (E12's rule reads the hosted FULL, `crate::certify`). Pure.
///
/// # Errors
///
/// [`Error::Drift`] when the two seeds' figures are not the same kinds in
/// the same order; [`Error::Empty`] when the kinds pool no unit or a pooled
/// first figure is at or below zero; [`Error::Overflow`] for a sum past an
/// `i128`.
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
    // The gap in per mille of the first, signed: `(two - one) * 1000 / one`.
    let gap = |one: i128, two: i128| -> Result<i128, Error> {
        two.checked_sub(one)
            .and_then(|difference| difference.checked_mul(1_000))
            .and_then(|scaled| scaled.checked_div(one))
            .ok_or(Error::Overflow("a seed gap"))
    };
    let signed_tenths = |per_mille: i128| {
        let sign = if per_mille < 0 { "-" } else { "+" };
        format!("{sign}{}", tenths(per_mille.unsigned_abs()))
    };
    let both = |one: &KindFigure, two: &KindFigure| one.clears_floor() && two.clears_floor();
    let pooled = |figures: &[&KindFigure]| -> Result<i128, Error> {
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
    let mut kinds: Vec<String> = Vec::with_capacity(first.len());
    let mut pool_first: Vec<&KindFigure> = Vec::new();
    let mut pool_second: Vec<&KindFigure> = Vec::new();
    for (one, two) in first.iter().zip(second) {
        if both(one, two) && one.milli_ns_per_unit > 0 {
            kinds.push(format!(
                "{} {}%",
                one.kind.name(),
                signed_tenths(gap(one.milli_ns_per_unit, two.milli_ns_per_unit)?)
            ));
            pool_first.push(one);
            pool_second.push(two);
        } else {
            kinds.push(format!(
                "{} left out (under its measurement floor on a seed)",
                one.kind.name()
            ));
        }
    }
    let (one, two) = (pooled(&pool_first)?, pooled(&pool_second)?);
    if one <= 0 {
        return Err(Error::Empty(format!(
            "a pooled first figure of {} ns a unit has no gap",
            thousandths(one)
        )));
    }
    Ok(format!(
        "::notice title=decision tick seed gap (this lane's own figure)::on {os}, 0x{:016x} \
         against 0x{BENCH_SEED:016x}: pooled over the kinds that clear their floors on both \
         seeds {} against {} ns a unit, {}%; by kind {}. Published, no verdict: E12's seed \
         rule reads P1 FULL's hosted figure",
        pharmakos_sim::DETERMINISM_MATCH_SEED,
        thousandths(two),
        thousandths(one),
        signed_tenths(gap(one, two)?),
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
            floor_ns: 0,
            most_allocations: Some(0),
            offset: (0, 0),
            per_tick: (0, 0),
            runs: RUNS,
            match_ticks: 2_400,
        }
    }

    fn sample(ns: u128, units: u64, marks: TickMarks) -> DecisionSample {
        DecisionSample {
            ns,
            units,
            marks,
            allocations: Some(0),
            offset: 3,
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
            runs: 3,
            match_ticks: 15,
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
        let deploy = figure(Kind::Deploy, &played, &baseline, 100).expect("a figure");
        assert_eq!((deploy.ticks, deploy.units, deploy.ns), (1, 1_000, 100_000));
        assert_eq!(deploy.milli_ns_per_unit, 100_000);
        // One tick at a 100 ns clock: 2 reads x 1 tick x 100 ns x 10.
        assert_eq!(deploy.floor_ns, 2_000);
        assert!(deploy.clears_floor());
        // Toward reads none of them, which is no figure.
        assert!(matches!(
            figure(Kind::Toward, &played, &baseline, 100),
            Err(Error::Empty(_))
        ));
        // A baseline of other ticks is refused.
        let short = Played {
            samples: vec![sample(1, 1, quiet)],
            ..played.clone()
        };
        assert!(matches!(
            figure(Kind::Deploy, &played, &short, 100),
            Err(Error::Drift(_))
        ));
    }

    /// M2: the kinds certify only when asked and at their plan; a cut run or
    /// an unasked one stamps the banner on every line.
    #[test]
    fn a_kinds_run_certifies_only_when_asked_and_whole() {
        use crate::provenance::{Certify, is_certifying, stamped};
        let line = "::notice title=x::a figure";
        let figures: Vec<KindFigure> = Kind::ALL
            .iter()
            .map(|kind| figure_of(*kind, 100_000))
            .collect();
        let whole = sampling(&figures, Certify::On).expect("a sampling");
        assert!(is_certifying(&stamped(line, &whole)));
        let unasked = sampling(&figures, Certify::Off).expect("a sampling");
        assert!(!is_certifying(&stamped(line, &unasked)));
        // One kind's match that played fewer runs or ticks than planned cuts
        // the whole run: the counts are read from what played.
        for cut in [
            KindFigure {
                runs: 1,
                ..figure_of(Kind::Deploy, 100_000)
            },
            KindFigure {
                match_ticks: 200,
                ..figure_of(Kind::Toward, 100_000)
            },
        ] {
            let mut figures = figures.clone();
            figures.push(cut);
            let cut = sampling(&figures, Certify::On).expect("a sampling");
            assert!(!is_certifying(&stamped(line, &cut)), "{cut:?}");
        }
        assert!(sampling(&[], Certify::On).is_err());
    }

    #[test]
    fn the_floor_is_the_clocks_reads_times_the_multiple() {
        assert_eq!(floor_ns(100, 8), Ok(16_000));
        assert_eq!(floor_ns(1, 0), Ok(0));
        assert!(clock_resolution().is_ok_and(|ns| ns > 0));
    }

    /// C1: a kind under its floor is left out and named, and the verdict over
    /// the rest is the same whatever that kind's figure is; a figure exactly at
    /// zero is left out like any under its floor (K4's mutant).
    #[test]
    fn a_kind_under_its_floor_is_left_out_and_named() {
        let mut figures: Vec<KindFigure> = Kind::ALL
            .iter()
            .map(|kind| figure_of(*kind, 100_000))
            .collect();
        let base = agreement(&figures).expect("an agreement");
        assert!(base.left_out.is_empty());
        for noisy in [-3_656_i128, 0, 900_000] {
            if let Some(deploy) = figures.iter_mut().find(|f| f.kind == Kind::Deploy) {
                deploy.milli_ns_per_unit = noisy;
                deploy.ns = noisy;
                deploy.floor_ns = 1_000_000;
            }
            let agreed = agreement(&figures).expect("an agreement");
            assert_eq!(agreed.left_out, vec![Kind::Deploy], "{noisy}");
            assert_eq!(agreed.median_milli, base.median_milli, "{noisy}");
            assert_eq!(
                agreed.within(TOLERANCE_PERCENT),
                base.within(TOLERANCE_PERCENT)
            );
            let line = agreement_notice(&agreed, 1, "linux").expect("a notice");
            assert!(
                line.contains("left out, under their measurement floors: deploy;"),
                "{line}"
            );
        }
        // At exactly its floor a kind counts.
        let mut at = figure_of(Kind::Commit, 5);
        at.floor_ns = 5;
        assert!(at.clears_floor());
        at.ns = 4;
        assert!(!at.clears_floor());
    }

    #[test]
    fn the_kinds_agree_only_within_the_tolerance() {
        let near: Vec<KindFigure> = Kind::ALL
            .iter()
            .zip([
                100_000, 110_000, 100_000, 100_000, 90_000, 100_000, 120_000, 95_000, 105_000,
            ])
            .map(|(kind, milli)| figure_of(*kind, milli))
            .collect();
        let agreed = agreement(&near).expect("an agreement");
        assert_eq!(agreed.median_milli, 100_000);
        assert_eq!(agreed.outlier, (Kind::Commit, 20_000));
        assert_eq!(agreed.within(TOLERANCE_PERCENT), Ok(true));
        // The must-trip: a tolerance below the outlier's distance turns it.
        assert_eq!(agreed.within(19), Ok(false));
        // F4: a room past a `u128` has no exact answer, and says so.
        assert!(matches!(agreed.within(u128::MAX), Err(Error::Overflow(_))));
        // C7: compared exactly, 50.9 % is not within 50 %.
        let mut edge = near.clone();
        if let Some(toward) = edge.iter_mut().find(|figure| figure.kind == Kind::Toward) {
            toward.milli_ns_per_unit = 150_900;
        }
        let split = agreement(&edge).expect("an agreement");
        assert_eq!(split.median_milli, 100_000);
        assert_eq!(split.outlier, (Kind::Toward, 50_900));
        assert_eq!(split.within(TOLERANCE_PERCENT), Ok(false));
        let expected = [
            "::notice title=decision tick kinds agree (0x0000000000000001)::on linux, the kinds ",
            "DO NOT AGREE within this lane's own tolerance, 50% of their median (a judgement, ",
            "not a gate), of 100.000 ns a unit; the outlier is toward at 50.9%; no kind left ",
            "out; each kind's distance: condition 0.0%, octile ranker 10.0%, covering, near ",
            "walks 0.0%, covering, far walks 0.0%, threat-ranking selector 10.0%, seam upkeep ",
            "0.0%, commit 20.0%, toward 50.9%, deploy 5.0%",
        ]
        .concat();
        assert_eq!(
            agreement_notice(&split, 1, "linux").expect("a notice"),
            expected
        );
        if let Some(toward) = edge.iter_mut().find(|figure| figure.kind == Kind::Toward) {
            toward.milli_ns_per_unit = 150_000;
        }
        assert_eq!(
            agreement(&edge)
                .expect("an agreement")
                .within(TOLERANCE_PERCENT),
            Ok(true)
        );
        assert!(agreement(&[]).is_err());
    }

    #[test]
    fn the_seed_gap_is_this_lanes_figure_with_no_verdict() {
        let first: Vec<KindFigure> = Kind::ALL
            .iter()
            .map(|kind| figure_of(*kind, 100_000))
            .collect();
        let far: Vec<KindFigure> = Kind::ALL
            .iter()
            .map(|kind| figure_of(*kind, 123_400))
            .collect();
        let line = seed_gap_notice(&first, &far, "linux").expect("a notice");
        assert!(
            line.contains("100.000 ns a unit, +23.4%; by kind"),
            "{line}"
        );
        assert!(!line.contains("PAST"), "{line}");
        assert!(!line.contains("within E12"), "{line}");
        let cut: Vec<KindFigure> = far.iter().take(2).cloned().collect();
        assert!(seed_gap_notice(&first, &cut, "linux").is_err());
        // A kind under its floor on either seed is left out by name, and
        // the pool is taken over the rest.
        let mut noisy = first.clone();
        if let Some(deploy) = noisy.iter_mut().find(|figure| figure.kind == Kind::Deploy) {
            deploy.ns = -3_656;
            deploy.milli_ns_per_unit = -3_656;
            deploy.floor_ns = 1_000;
        }
        let line = seed_gap_notice(&noisy, &far, "linux").expect("a notice");
        assert!(
            line.contains("deploy left out (under its measurement floor on a seed)"),
            "{line}"
        );
        assert!(line.contains("+23.4%; by kind"), "{line}");
    }

    /// F1: a kinds line names E12 only to say it gives no verdict, since
    /// E12's seed rule reads P1 FULL's hosted figure and not these.
    #[test]
    fn no_kinds_line_names_e12_but_to_give_no_verdict() {
        let first: Vec<KindFigure> = Kind::ALL
            .iter()
            .map(|kind| figure_of(*kind, 100_000))
            .collect();
        let far: Vec<KindFigure> = Kind::ALL
            .iter()
            .map(|kind| figure_of(*kind, 190_000))
            .collect();
        let lines = [
            kinds_notice(&first, 100, 1, KindPlan::MEASURED, "linux"),
            agreement_notice(&agreement(&far).expect("an agreement"), 1, "linux")
                .expect("a notice"),
            seed_gap_notice(&first, &far, "linux").expect("a notice"),
        ];
        let form = "no verdict: E12's seed rule reads P1 FULL's hosted figure";
        for line in &lines {
            let named = line.matches("E12").count();
            let allowed = line.matches(form).count();
            assert_eq!(named, allowed, "{line}");
        }
        assert_eq!(
            lines
                .iter()
                .map(|line| line.matches(form).count())
                .sum::<usize>(),
            1,
            "the seed gap's one sentence"
        );
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
            failed: 0,
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
        assert!(Kind::OctileRanker.reads(TickMarks::default()));
        // A covering resolve that finds no site fails its step as it starts.
        let failed = TickMarks {
            failed: 1,
            ..TickMarks::default()
        };
        assert!(Kind::Covering.reads(failed));
        assert!(Kind::CoveringFar.reads(started));
        assert!(!Kind::Covering.reads(TickMarks::default()));
    }
}
