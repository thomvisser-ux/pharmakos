// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The G3′ segment: the gate's layout under the bench rules, one segment of
//! real play with the crater stream, every tick timed phase by phase through
//! the gateway's phase stepper (S2's plan, section 5 and E30).
//!
//! # One tick, as the harness times it
//!
//! 1. Between ticks: the crater stream files the craters due
//!    ([`super::stream`]), untimed, as a shell's landing would have.
//! 2. **begin**: `Surface::phased_step`, which opens the tick.
//! 3. **each phase**: `SurfaceTick::run_next_phase`, one call per phase of
//!    `PHASE_ORDER` but the hash, each timed alone.
//! 4. The open tick is put down with only its hash left (the stepper's
//!    type-state allows it, and the host refuses everything a half tick must
//!    not see), and the harness reads the world's counters for the tick:
//!    events, the repath serve, the support rule's passes and visits, the live
//!    and moving units. Untimed.
//! 5. **finish**: `Surface::phased_step` again, which resumes the open tick,
//!    and `SurfaceTick::finish`: the hash phase, and the gateway's per-tick
//!    bookkeeping (the event feed, the fog, the view stamp) that every hosted
//!    tick pays.
//!
//! The tick's time is begin, the phases and finish together: what one tick of
//! a hosted match costs the host. Pathing is the sim's own (item 64: no
//! charged substitute, and the broadphase's CSR rebuild is its own phase,
//! counted once); the tick-minus-pathing mean is the tick's mean less the
//! Pathing phase's (item 116 (6) (b)).

use std::path::Path;
use std::time::Instant;

use pharmakos_gateway::surface::Surface;

use pharmakos_sim::EVENT_BUS_CAPACITY;
use pharmakos_sim::events::EventKind;
use pharmakos_sim::math::quantity::MS_PER_TICK;
use pharmakos_sim::world::{PHASE_ORDER, Phase, World};

use super::layout::{self, GATE_SEATS, LAID_OUT_BEACONS, MOST_BEACONS_PER_SEAT, UNITS_PER_SEAT};
use super::sight::{self, SightProbe};
use super::stream::{CraterStream, StreamCounts};
use super::variant::{self, Override, SeatLoad};
use crate::decision::hosted_budget_playbook;
use crate::error::Error;
use crate::heap::HeapCounter;
use crate::hosted::{self, Opened, Opening};
use crate::stats::{self, Summary};

/// Ticks in one minute of game time.
pub const TICKS_PER_MINUTE: u64 = 1_200;

/// How the segment is played.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SegmentPlan {
    /// The map seed.
    pub seed: u64,
    /// The segment's length, in game milliseconds.
    pub segment_ms: i32,
    /// The crater stream's rate.
    pub craters_per_second: u32,
    /// Whether `seen_by` is probed at each minute's end.
    pub probe_sight: bool,
}

/// What the layout and the rules were, so the run can be reproduced.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Provenance {
    /// The map seed.
    pub seed: u64,
    /// The committed rules table's hash.
    pub shipped_rules_hash: u64,
    /// The bench variant's hash.
    pub bench_rules_hash: u64,
    /// Every row the variant moved.
    pub overrides: Vec<Override>,
    /// The layout's fingerprint ([`layout::digest`]).
    pub layout_digest: u64,
    /// Beacons per mandate, as [`layout::proportions`] counts them.
    pub proportions: Vec<(&'static str, usize)>,
    /// The size units each seat sealed.
    pub units_per_seat: u32,
}

/// One phase's sample over the segment.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PhaseFigure {
    /// The phase.
    pub phase: Phase,
    /// Its durations.
    pub summary: Summary,
}

/// One minute of the segment, for the drift check (item 68; spike step 6).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Minute {
    /// The minute, from 1.
    pub number: u64,
    /// The minute's tick p99, in nanoseconds.
    pub tick_p99_ns: u128,
    /// The minute's broadphase mean, in nanoseconds.
    pub broadphase_mean_ns: u128,
    /// Live units at the minute's end.
    pub live_units: u64,
    /// Live heap bytes at the minute's end, when a counter was given.
    pub live_heap_bytes: Option<u64>,
    /// `seen_by` at the minute's end, when it was probed.
    pub sight: Option<SightProbe>,
}

/// The repath serve against its cap (item 69).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Repaths {
    /// `locomotion.repath_cap_per_tick`.
    pub cap: u32,
    /// Repaths served over the segment.
    pub served: u64,
    /// Ticks that spent the whole cap: a burst the cap held back.
    pub ticks_at_cap: u64,
    /// The longest run of consecutive ticks at the cap.
    pub longest_run_at_cap: u64,
    /// The largest backlog left after a tick's serve.
    pub max_backlog: u32,
}

/// Allocations per tick, split where the sim's phases end.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Allocations {
    /// Mean allocations in begin and the phases (the sim's tick but its hash).
    pub phases_mean_milli: u64,
    /// The most in one tick's begin and phases.
    pub phases_max: u64,
    /// Mean allocations in finish (the hash, and the gateway's bookkeeping),
    /// in thousandths.
    pub finish_mean_milli: u64,
    /// The most in one tick's finish.
    pub finish_max: u64,
}

/// What the segment measured.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SegmentReport {
    /// How to reproduce it.
    pub provenance: Provenance,
    /// Ticks played.
    pub ticks: u64,
    /// The whole tick: begin, phases and finish.
    pub tick: Summary,
    /// Each phase but the hash, in `PHASE_ORDER`.
    pub phases: Vec<PhaseFigure>,
    /// Begin, alone.
    pub begin: Summary,
    /// Finish: the hash phase and the gateway's per-tick bookkeeping.
    pub finish: Summary,
    /// The tick's mean less the Pathing phase's, in nanoseconds.
    pub tick_minus_pathing_mean_ns: u128,
    /// The repath serve.
    pub repaths: Repaths,
    /// Per minute.
    pub minutes: Vec<Minute>,
    /// The most events one tick left on the bus.
    pub peak_events: usize,
    /// Deaths over the segment: units, structures, beacons and commanders.
    pub deaths: u64,
    /// Kill credits booked over the segment.
    pub kill_credits: u64,
    /// The support rule: regions that fell, the most passes and visits one
    /// tick took.
    pub regions_fallen: u64,
    /// The most passes one tick's fall took.
    pub max_fall_passes: u64,
    /// The most voxels one tick's searches visited.
    pub max_support_visits: usize,
    /// The mean of the units walking (destination not their position) per
    /// tick, in thousandths.
    pub movers_mean_milli: u64,
    /// The bytes the damage and edit queues hold room for (E5).
    pub queue_bytes: usize,
    /// The crater stream.
    pub craters: StreamCounts,
    /// Allocations per tick, when a counter was given.
    pub allocations: Option<Allocations>,
    /// Live heap bytes when the Push opened, when a counter was given.
    pub heap_at_start: Option<u64>,
    /// The hash chain's digest, so two runs can be compared.
    pub chain_digest: u64,
}

/// The gate's per-seat load, as the variant powers it.
#[must_use]
pub const fn gate_load() -> SeatLoad {
    SeatLoad {
        laid_out_units: UNITS_PER_SEAT,
        laid_out_beacons: MOST_BEACONS_PER_SEAT,
    }
}

/// What one tick left on the world, read between its last phase and its hash.
#[derive(Clone, Copy, Debug, Default)]
struct Counters {
    events: usize,
    deaths: u64,
    credits: u64,
    served: u32,
    backlog: u32,
    passes: u64,
    visits: usize,
    live_units: u64,
    movers: u64,
}

fn counters(world: &World) -> Counters {
    let events = world.events();
    let count = |kinds: &[EventKind]| -> u64 {
        events
            .iter()
            .filter(|event| kinds.contains(&event.kind))
            .fold(0_u64, |count, _| count.saturating_add(1))
    };
    let serve = world.serve_report();
    let collapse = world.collapse_counts();
    let units = world.units();
    let mut live_units: u64 = 0;
    let mut movers: u64 = 0;
    for ((hp, at), to) in units
        .hit_points()
        .iter()
        .zip(units.positions())
        .zip(units.destinations())
    {
        if hp.is_alive() {
            live_units = live_units.saturating_add(1);
            if at != to {
                movers = movers.saturating_add(1);
            }
        }
    }
    Counters {
        events: events.len(),
        deaths: count(&[
            EventKind::UnitDestroyed,
            EventKind::StructureDestroyed,
            EventKind::BeaconDestroyed,
            EventKind::CommanderDied,
        ]),
        credits: count(&[EventKind::KillCredited]),
        served: serve.served,
        backlog: serve.backlog,
        passes: collapse.last_passes,
        visits: collapse.last_visits,
        live_units,
        movers,
    }
}

/// One tick as [`timed_tick`] took it.
#[derive(Debug)]
pub(super) struct Timed {
    /// Opening the tick.
    pub(super) begin_ns: u128,
    /// Each phase but the hash: its index in `PHASE_ORDER`, and its time.
    pub(super) phase_ns: Vec<(usize, u128)>,
    /// The hash, and the gateway's per-tick bookkeeping.
    pub(super) finish_ns: u128,
    counters: Counters,
    pub(super) report: pharmakos_sim::runner::TickReport,
    /// Allocations in begin and the phases, then in finish.
    allocations: Option<(u64, u64)>,
}

impl Timed {
    /// The whole tick: begin, the phases and finish.
    pub(super) fn total_ns(&self) -> Result<u128, Error> {
        let phases = self
            .phase_ns
            .iter()
            .try_fold(0_u128, |sum, (_, ns)| sum.checked_add(*ns))
            .ok_or(Error::Overflow("a tick's phases"))?;
        self.begin_ns
            .checked_add(phases)
            .and_then(|sum| sum.checked_add(self.finish_ns))
            .ok_or(Error::Overflow("a tick's time"))
    }
}

/// The allocations between two readings of the counter.
fn between(before: u64, after: u64) -> Result<u64, Error> {
    after
        .checked_sub(before)
        .ok_or_else(|| Error::Drift("the allocation counter went backwards".to_owned()))
}

/// Open, run, read and finish one tick, or `None` once the Push is over.
pub(super) fn timed_tick(
    surface: &mut pharmakos_gateway::surface::Surface,
    heap: Option<&dyn HeapCounter>,
) -> Result<Option<Timed>, Error> {
    let allocations_before = heap.map(HeapCounter::allocations);
    let begun = Instant::now();
    let mut phase_ns: Vec<(usize, u128)> = Vec::with_capacity(PHASE_ORDER.len());
    let begin_ns;
    {
        let Some(mut tick) = surface
            .phased_step()
            .map_err(|error| Error::gateway("opening a tick", &error))?
        else {
            return Ok(None);
        };
        begin_ns = begun.elapsed().as_nanos();
        loop {
            let started = Instant::now();
            let ran = tick
                .run_next_phase()
                .map_err(|error| Error::gateway("running a phase", &error))?;
            let ns = started.elapsed().as_nanos();
            let Some(phase) = ran else {
                break;
            };
            let index = PHASE_ORDER
                .iter()
                .position(|named| *named == phase)
                .ok_or_else(|| {
                    Error::Drift(format!("the stepper ran {phase:?}, outside PHASE_ORDER"))
                })?;
            phase_ns.push((index, ns));
        }
    }
    let allocations_mid = heap.map(HeapCounter::allocations);
    let counters = counters(hosted::world(surface)?);
    let finished = Instant::now();
    let report = {
        let tick = surface
            .phased_step()
            .map_err(|error| Error::gateway("resuming the open tick", &error))?
            .ok_or_else(|| Error::Drift("the open tick could not be resumed".to_owned()))?;
        tick.finish()
            .map_err(|error| Error::gateway("finishing a tick", &error))?
            .ok_or_else(|| Error::Drift("a finished tick gave no report".to_owned()))?
    };
    let finish_ns = finished.elapsed().as_nanos();
    let allocations_after = heap.map(HeapCounter::allocations);
    let allocations = match (allocations_before, allocations_mid, allocations_after) {
        (Some(before), Some(mid), Some(after)) => {
            Some((between(before, mid)?, between(mid, after)?))
        }
        _ => None,
    };
    Ok(Some(Timed {
        begin_ns,
        phase_ns,
        finish_ns,
        counters,
        report,
        allocations,
    }))
}

/// Everything recorded tick by tick.
#[derive(Debug, Default)]
struct Record {
    tick: Vec<u128>,
    begin: Vec<u128>,
    finish: Vec<u128>,
    phases: Vec<Vec<u128>>,
    chain: Vec<u8>,
    repaths: Repaths,
    run_at_cap: u64,
    peak_events: usize,
    deaths: u64,
    credits: u64,
    max_passes: u64,
    max_visits: usize,
    movers: u64,
    phase_allocations: Vec<u64>,
    finish_allocations: Vec<u64>,
    minutes: Vec<Minute>,
}

impl Record {
    /// Record one tick, and answer its whole time and its broadphase's.
    fn push(&mut self, timed: &Timed, cap: u32) -> Result<(u128, u128), Error> {
        let tick_ns = timed.total_ns()?;
        for (index, ns) in &timed.phase_ns {
            self.phases
                .get_mut(*index)
                .ok_or(Error::Overflow("a phase index"))?
                .push(*ns);
        }
        let broadphase = timed
            .phase_ns
            .iter()
            .find(|(index, _)| PHASE_ORDER.get(*index) == Some(&Phase::Broadphase))
            .map(|(_, ns)| *ns)
            .ok_or_else(|| Error::Drift("a tick ran no broadphase".to_owned()))?;
        self.tick.push(tick_ns);
        self.begin.push(timed.begin_ns);
        self.finish.push(timed.finish_ns);
        self.chain
            .extend_from_slice(&timed.report.hash.to_le_bytes());
        self.note(timed.counters, cap);
        if let Some((phases, finish)) = timed.allocations {
            self.phase_allocations.push(phases);
            self.finish_allocations.push(finish);
        }
        Ok((tick_ns, broadphase))
    }

    fn note(&mut self, counters: Counters, cap: u32) {
        self.peak_events = self.peak_events.max(counters.events);
        self.deaths = self.deaths.saturating_add(counters.deaths);
        self.credits = self.credits.saturating_add(counters.credits);
        self.max_passes = self.max_passes.max(counters.passes);
        self.max_visits = self.max_visits.max(counters.visits);
        self.movers = self.movers.saturating_add(counters.movers);
        let repaths = &mut self.repaths;
        repaths.served = repaths.served.saturating_add(u64::from(counters.served));
        repaths.max_backlog = repaths.max_backlog.max(counters.backlog);
        if counters.served >= cap {
            repaths.ticks_at_cap = repaths.ticks_at_cap.saturating_add(1);
            self.run_at_cap = self.run_at_cap.saturating_add(1);
            repaths.longest_run_at_cap = repaths.longest_run_at_cap.max(self.run_at_cap);
        } else {
            self.run_at_cap = 0;
        }
    }
}

/// The gate's match, in its Push: the bench variant, the layout placed in the
/// lobby, every seat sealed, and the crater stream ready to file.
#[derive(Debug)]
pub struct Gate {
    /// The match.
    pub surface: Surface,
    /// The crater stream over the layout.
    pub stream: CraterStream,
    /// How to reproduce it.
    pub provenance: Provenance,
    /// `locomotion.repath_cap_per_tick`, as the variant carries it.
    pub repath_cap: u32,
}

/// Open the gate's match on `seed` for one segment of `segment_ms`, with a
/// crater stream of `craters_per_second`.
///
/// # Errors
///
/// When the variant, the layout or the match cannot be built, or a seat's
/// playbook is refused; [`Error::Drift`] when the hosted world does not hold
/// the gate's 40 beacons after the layout.
pub fn open_gate(
    root: &Path,
    seed: u64,
    segment_ms: i32,
    craters_per_second: u32,
) -> Result<Gate, Error> {
    let bench = variant::bench_rules(root, GATE_SEATS, gate_load())?;
    let playbook = hosted_budget_playbook(root)?;
    let opening = Opening {
        seed,
        seats: GATE_SEATS,
        segment_ms,
        rules: &bench.rules,
        playbook: &playbook,
    };
    let host = hosted::open_host(&opening)?;
    let layout = layout::gate_layout(host.world())?;
    let stream = CraterStream::new(seed, craters_per_second, &layout, host.world())?;
    let Opened {
        surface,
        units_per_seat,
    } = hosted::attach(host, Some(&layout), &opening)?;
    let beacon_count = hosted::world(&surface)?.beacons().len();
    let expected = LAID_OUT_BEACONS
        .checked_add(GATE_SEATS)
        .ok_or(Error::Overflow("the beacon total"))?;
    if beacon_count != expected {
        return Err(Error::Drift(format!(
            "the hosted world holds {beacon_count} beacons after the layout, not the gate's \
             {expected}"
        )));
    }
    Ok(Gate {
        surface,
        stream,
        provenance: Provenance {
            seed,
            shipped_rules_hash: bench.shipped_hash,
            bench_rules_hash: bench.bench_hash,
            overrides: bench.overrides,
            layout_digest: layout::digest(&layout),
            proportions: layout::proportions(&layout),
            units_per_seat,
        },
        repath_cap: bench.rules.repath_cap_per_tick(),
    })
}

/// Play the segment once and measure it.
///
/// # Errors
///
/// When the variant, the layout or the match cannot be built, a seat's
/// playbook is refused, the stream cannot file, or a tick refuses to run.
pub fn measure(
    root: &Path,
    plan: SegmentPlan,
    heap: Option<&dyn HeapCounter>,
) -> Result<SegmentReport, Error> {
    let Gate {
        mut surface,
        mut stream,
        provenance,
        repath_cap: cap,
    } = open_gate(root, plan.seed, plan.segment_ms, plan.craters_per_second)?;
    let mut record = Record {
        phases: vec![Vec::new(); PHASE_ORDER.len()],
        repaths: Repaths {
            cap,
            ..Repaths::default()
        },
        ..Record::default()
    };
    let heap_at_start = heap.map(HeapCounter::live_bytes);
    let queue_bytes = hosted::world(&surface)?.queue_bytes();
    let regions_before = hosted::world(&surface)?.collapse_counts().regions;
    let mut minute_ticks: Vec<u128> = Vec::new();
    let mut minute_broadphase: Vec<u128> = Vec::new();
    let mut played: u64 = 0;
    loop {
        {
            let host = surface
                .host_mut()
                .map_err(|error| Error::gateway("filing the crater stream", &error))?;
            stream.file(host, played)?;
        }
        let Some(timed) = timed_tick(&mut surface, heap)? else {
            break;
        };
        let segment_ended = timed.report.segment_ended;
        let (tick_ns, broadphase) = record.push(&timed, cap)?;
        minute_ticks.push(tick_ns);
        minute_broadphase.push(broadphase);
        played = played.saturating_add(1);
        if played.rem_euclid(TICKS_PER_MINUTE) == 0 || segment_ended {
            let ticks = std::mem::take(&mut minute_ticks);
            let broadphases = std::mem::take(&mut minute_broadphase);
            let probe = if plan.probe_sight {
                Some(sight::probe(hosted::world(&surface)?)?)
            } else {
                None
            };
            record.minutes.push(Minute {
                number: played.div_ceil(TICKS_PER_MINUTE),
                tick_p99_ns: Summary::of(ticks)?.p99_ns,
                broadphase_mean_ns: stats::mean(&broadphases)
                    .ok_or_else(|| Error::Empty("a minute with no broadphase".to_owned()))?,
                live_units: timed.counters.live_units,
                live_heap_bytes: heap.map(HeapCounter::live_bytes),
                sight: probe,
            });
        }
        if segment_ended {
            break;
        }
    }
    let regions_fallen = hosted::world(&surface)?
        .collapse_counts()
        .regions
        .checked_sub(regions_before)
        .ok_or_else(|| Error::Drift("the support rule's region count went down".to_owned()))?;
    finish(
        record,
        Tally {
            provenance,
            queue_bytes,
            craters: stream.counts(),
            heap_at_start,
            ticks: played,
            regions_fallen,
        },
    )
}

/// What the segment knows besides its record.
#[derive(Debug)]
struct Tally {
    provenance: Provenance,
    queue_bytes: usize,
    craters: StreamCounts,
    heap_at_start: Option<u64>,
    ticks: u64,
    regions_fallen: u64,
}

/// Turn the record into the report.
fn finish(record: Record, tally: Tally) -> Result<SegmentReport, Error> {
    let Tally {
        provenance,
        queue_bytes,
        craters,
        heap_at_start,
        ticks,
        regions_fallen,
    } = tally;
    if ticks == 0 {
        return Err(Error::Empty("the segment played no tick".to_owned()));
    }
    let tick = Summary::of(record.tick)?;
    let mut phases: Vec<PhaseFigure> = Vec::new();
    for (phase, sample) in PHASE_ORDER.iter().zip(record.phases) {
        if *phase == Phase::Hash {
            continue;
        }
        phases.push(PhaseFigure {
            phase: *phase,
            summary: Summary::of(sample)?,
        });
    }
    let pathing = phases
        .iter()
        .find(|figure| figure.phase == Phase::Pathing)
        .map(|figure| figure.summary.mean_ns)
        .ok_or_else(|| Error::Drift("no Pathing phase was timed".to_owned()))?;
    let milli = |sum: u64| -> Result<u64, Error> {
        sum.checked_mul(1_000)
            .and_then(|scaled| scaled.checked_div(ticks))
            .ok_or(Error::Overflow("a per-tick mean"))
    };
    let allocations = if record.phase_allocations.is_empty() {
        None
    } else {
        let sum = |sample: &[u64]| {
            sample
                .iter()
                .fold(0_u64, |sum, value| sum.saturating_add(*value))
        };
        let most = |sample: &[u64]| sample.iter().copied().fold(0_u64, u64::max);
        Some(Allocations {
            phases_mean_milli: milli(sum(&record.phase_allocations))?,
            phases_max: most(&record.phase_allocations),
            finish_mean_milli: milli(sum(&record.finish_allocations))?,
            finish_max: most(&record.finish_allocations),
        })
    };
    Ok(SegmentReport {
        provenance,
        ticks,
        tick_minus_pathing_mean_ns: tick.mean_ns.saturating_sub(pathing),
        tick,
        phases,
        begin: Summary::of(record.begin)?,
        finish: Summary::of(record.finish)?,
        repaths: record.repaths,
        minutes: record.minutes,
        peak_events: record.peak_events,
        deaths: record.deaths,
        kill_credits: record.credits,
        regions_fallen,
        max_fall_passes: record.max_passes,
        max_support_visits: record.max_visits,
        movers_mean_milli: milli(record.movers)?,
        queue_bytes,
        craters,
        allocations,
        heap_at_start,
        chain_digest: pharmakos_sim::digest(&record.chain),
    })
}

/// Ticks a second the tick's mean allows, and the multiple of real time, both
/// in thousandths.
///
/// # Errors
///
/// [`Error::Empty`] for a mean of zero.
pub fn speed_milli(tick_mean_ns: u128) -> Result<(u128, u128), Error> {
    let tick_ns = u128::try_from(MS_PER_TICK)
        .map_err(|_| Error::Overflow("the tick's length"))?
        .checked_mul(1_000_000)
        .ok_or(Error::Overflow("the tick's length"))?;
    let empty = || Error::Empty("a tick mean of zero has no speed".to_owned());
    let per_second = 1_000_000_000_000_u128
        .checked_div(tick_mean_ns)
        .ok_or_else(empty)?;
    let real_time = tick_ns
        .checked_mul(1_000)
        .and_then(|scaled| scaled.checked_div(tick_mean_ns))
        .ok_or_else(empty)?;
    Ok((per_second, real_time))
}

/// The bus's capacity, for the notice.
#[must_use]
pub const fn bus_capacity() -> usize {
    EVENT_BUS_CAPACITY
}
