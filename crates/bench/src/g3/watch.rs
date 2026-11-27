// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the gate's load costs a watcher, a seat's knowledge and the breach
//! search, read from outside the sim (S3's plan, E15 and E17; task `cert3`).
//!
//! One segment at the gate's load with raids ([`GateLoad::RAIDS`]), stepped
//! as a host steps it, and between its ticks three readings that ask the
//! sim and the gateway nothing they do not already answer:
//!
//! * **The view feed** (E15; S2-12's delta mode and S1-34's recompute key):
//!   each seat polls `get_view` through `Surface::call` with its own token,
//!   as a client does, a listing every [`POLL_TICKS`] ticks and at most
//!   [`PAGES_PER_TICK`] pages a tick, the first a keyframe and every later
//!   one a delta from the cursor the last listing handed back. For each
//!   listing: the whole entity list's bytes (each entity as the wire spells
//!   it, summed over the listing's pages), the entities, the answer's bytes,
//!   the pages and the wall-clock time of the calls. Whether the seat's
//!   sensors moved since its last listing is read from the sim's own sight
//!   (`World::sight`), the input the gateway's recompute key digests
//!   (`crates/gateway/src/surface/watch.rs`, `sight_key`), so the listings
//!   that recomputed every modified chunk and the ones that did not are told
//!   apart, and the recompute notice compares the two groups' mean times,
//!   page count by page count: a difference of means, not a cost per move.
//! * **Sightings** (E17): each seat's rows of the sim's sighting table after
//!   every tick, `SightingTable::len_of`, and the peak against the table's
//!   room per seat, `SightingTable::per_seat`; and what the sim itself
//!   counted over the segment, the memories each seat forgot for room
//!   (`World::sightings_forgotten_by`) and the sightings the table refused
//!   (`World::sightings_refused`). A seat that forgot nothing held its true
//!   peak; one that forgot any held a peak the room cut short.
//! * **The breach search** (`attack.rs`'s `breach_search`, ~133 and ~140), as
//!   a probe of its cost on the gate's world: once a minute, from every live
//!   Attack beacon's column to every enemy core's own column (a goal no walker
//!   stands on, so the oracle's `SealedWay::of` calls such a way sealed), the
//!   search itself, run read-only on the hosted world with a scratch of the
//!   bench's own and timed, fastest of [`BREACH_RUNS`], with its outcome;
//!   beside them the sim's own launched Attack rows, those holding a breach,
//!   and its counts of the searches it refused (`World::attack_counts`). The
//!   probe's ways are the bench's, not the ones the sim's raids ask (a lead's
//!   column to its aim, which the sim reads from state the bench does not
//!   restate). The expansions a search spends are the sim's own count,
//!   `BreachScratch::expansions`, read after each run: the most one search
//!   spent and their sum, against the search's bound. The count is a pure
//!   function of the world and the way, so a search whose runs disagree on
//!   it is a drift, and the probe fails.
//!
//! No gate reads these figures: an alarm's lines are published and gated
//! nowhere, and a certifying run's are recorded as evidence for E15 and E17
//! ([`crate::provenance::Role`]). The polls and the probes run between
//! ticks, so no tick here is timed: this segment is not G3′'s.

use std::time::Instant;

use pharmakos_gateway::fog::Blind;
use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::token::Token;
use pharmakos_proto::json::{self, Json};
use pharmakos_sim::attack::{
    BREACH_EXPANSIONS, BreachRefused, BreachScratch, SealedWay, breach_search,
};
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::sight::SensorKind;
use pharmakos_sim::tables::SeatId;
use pharmakos_sim::world::World;

use super::layout::{GATE_SEATS, Quarters};
use super::segment::{GateLoad, open_gate};
use crate::error::Error;
use crate::hosted;
use crate::stats::Summary;

/// A seat begins a listing this many ticks after it began its last: five
/// listings a second, a client's frame rate well inside the gateway's rate
/// limit (`limit::CALLS_PER_WINDOW` over `WINDOW_TICKS`).
pub const POLL_TICKS: u64 = 4;

/// The most pages a seat asks for in one tick; a listing longer than this
/// (the keyframe's) runs on over the next ticks, as a client's does.
pub const PAGES_PER_TICK: u32 = 2;

/// How many times each breach search is timed, the fastest kept.
pub const BREACH_RUNS: usize = 3;

/// The gate's ticks a minute.
const TICKS_PER_MINUTE: u64 = 1_200;

/// One listing a seat completed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Listing {
    /// The first listing, from no cursor: the whole map.
    pub keyframe: bool,
    /// Whether the seat's sensors had moved since its last listing began.
    pub moved: bool,
    /// The pages it took.
    pub pages: u32,
    /// The entities its pages held, summed over every page of the listing.
    pub entities: u64,
    /// The entity list's bytes, each entity as the wire spells it.
    pub entity_bytes: u64,
    /// Every page's answer, as the wire spells it.
    pub answer_bytes: u64,
    /// The calls' wall-clock time, in nanoseconds.
    pub ns: u128,
}

/// A listing a seat has begun and not completed.
#[derive(Clone, Debug)]
struct Open {
    listing: Listing,
}

/// A seat's poller.
#[derive(Debug)]
struct Poller {
    seat: SeatId,
    token: Token,
    cursor: String,
    open: Option<Open>,
    since: u64,
    sensors: Option<Vec<(u8, [i32; 3], i32)>>,
    done: Vec<Listing>,
}

/// The seat's sensors as the recompute key reads them: each one's kind,
/// centre and radius, in the sim's walk order.
fn sensors_of(world: &World, seat: SeatId) -> Vec<(u8, [i32; 3], i32)> {
    world
        .sight()
        .sensors()
        .iter()
        .filter(|sensor| sensor.seat == seat)
        .map(|sensor| {
            let tag = match sensor.kind {
                SensorKind::Sphere => 0,
                SensorKind::Unit(_) => 1,
                SensorKind::Structure(_) => 2,
            };
            (
                tag,
                [
                    sensor.centre[0].raw(),
                    sensor.centre[1].raw(),
                    sensor.centre[2].raw(),
                ],
                sensor.radius.raw(),
            )
        })
        .collect()
}

/// A count as a `u64`.
fn count(value: usize, what: &'static str) -> Result<u64, Error> {
    u64::try_from(value).map_err(|_| Error::Overflow(what))
}

impl Poller {
    /// Begin a listing when one is due, then ask for this tick's pages.
    fn tick(&mut self, surface: &mut Surface) -> Result<(), Error> {
        self.since = self
            .since
            .checked_add(1)
            .ok_or(Error::Overflow("a poller's ticks"))?;
        if self.open.is_none() {
            if self.since < POLL_TICKS && self.sensors.is_some() {
                return Ok(());
            }
            let now = sensors_of(hosted::world(surface)?, self.seat);
            let keyframe = self.sensors.is_none();
            let moved = self.sensors.as_ref().is_some_and(|was| *was != now);
            self.sensors = Some(now);
            self.since = 0;
            self.open = Some(Open {
                listing: Listing {
                    keyframe,
                    moved,
                    pages: 0,
                    entities: 0,
                    entity_bytes: 0,
                    answer_bytes: 0,
                    ns: 0,
                },
            });
        }
        for _ in 0..PAGES_PER_TICK {
            if self.page(surface)? {
                break;
            }
        }
        Ok(())
    }

    /// Ask for one page of the open listing; answer whether it completed it.
    fn page(&mut self, surface: &mut Surface) -> Result<bool, Error> {
        let params = json::write(&Json::Object(vec![(
            "cursor".to_owned(),
            Json::String(self.cursor.clone()),
        )]));
        let request = pharmakos_gateway::rpc::parse(&format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"get_view","params":{params}}}"#
        ))
        .map_err(|error| Error::Codec {
            what: "the bench's get_view request".to_owned(),
            reason: error.title().to_string(),
        })?;
        let started = Instant::now();
        let answer = surface.call(Some(&self.token), &request, &Blind);
        let ns = started.elapsed().as_nanos();
        let result = answer.get("result").ok_or_else(|| Error::Gateway {
            during: "polling the view",
            message: format!(
                "get_view for seat {}: {}",
                self.seat.raw(),
                json::write(&answer)
            ),
        })?;
        let Page {
            entities,
            complete,
            cursor,
        } = read_page(result)?;
        let open = self
            .open
            .as_mut()
            .ok_or_else(|| Error::Drift("a page asked for with no listing open".to_owned()))?;
        let listing = &mut open.listing;
        let mut bytes: u64 = 0;
        for entity in entities {
            bytes = bytes
                .checked_add(count(json::write(entity).len(), "an entity's bytes")?)
                .ok_or(Error::Overflow("an entity list's bytes"))?;
        }
        listing.pages = listing
            .pages
            .checked_add(1)
            .ok_or(Error::Overflow("a listing's pages"))?;
        listing.entities = listing
            .entities
            .checked_add(count(entities.len(), "the entities")?)
            .ok_or(Error::Overflow("a listing's entities"))?;
        listing.entity_bytes = listing
            .entity_bytes
            .checked_add(bytes)
            .ok_or(Error::Overflow("a listing's entity bytes"))?;
        listing.answer_bytes = listing
            .answer_bytes
            .checked_add(count(json::write(&answer).len(), "an answer's bytes")?)
            .ok_or(Error::Overflow("a listing's answer bytes"))?;
        listing.ns = listing
            .ns
            .checked_add(ns)
            .ok_or(Error::Overflow("a listing's time"))?;
        self.cursor = cursor;
        if complete {
            let done = *listing;
            self.done.push(done);
            self.open = None;
        }
        Ok(complete)
    }
}

/// One `get_view` page as the bench reads it.
#[derive(Debug)]
struct Page<'a> {
    entities: &'a [Json],
    complete: bool,
    cursor: String,
}

/// A `get_view` answer's page: its entities, whether it completes its
/// listing, and the cursor it hands back. Every field is required as the
/// gateway's answer writes it; a field missing or of another shape is
/// [`Error::Drift`] naming it, so a listing that never completes, or a page
/// whose entities went missing, cannot pass unseen.
fn read_page(result: &Json) -> Result<Page<'_>, Error> {
    let drift = |field: &str, found: Option<&Json>| {
        Error::Drift(format!(
            "get_view's answer has no `{field}` of its shape: {}",
            match found {
                Some(value) => json::write(value),
                None => "it is missing".to_owned(),
            }
        ))
    };
    let entities = match result.get("entities") {
        Some(Json::Array(items)) => items.as_slice(),
        other => return Err(drift("entities", other)),
    };
    let complete = match result.get("complete") {
        Some(Json::Bool(complete)) => *complete,
        other => return Err(drift("complete", other)),
    };
    let cursor = match result.get("next_cursor") {
        Some(Json::String(cursor)) => cursor.clone(),
        other => return Err(drift("next_cursor", other)),
    };
    Ok(Page {
        entities,
        complete,
        cursor,
    })
}

/// One minute's breach probe.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BreachProbe {
    /// The minute, from one.
    pub minute: u64,
    /// Ways asked of the oracle: a live Attack beacon to an enemy core.
    pub ways: u64,
    /// Of them, the ways the oracle called sealed.
    pub sealed: u64,
    /// Searches that found a breach.
    pub found: u64,
    /// Searches that spent their expansions.
    pub exhausted: u64,
    /// Searches that found no way across a wall.
    pub no_way: u64,
    /// The slowest search, fastest of its runs, in nanoseconds.
    pub slowest_ns: u128,
    /// The searches' times summed, fastest of each one's runs.
    pub total_ns: u128,
    /// The most states one search expanded (`BreachScratch::expansions`).
    pub most_expansions: u32,
    /// The states the searches expanded, summed.
    pub total_expansions: u64,
    /// The sim's own Attack rows launched at the probe.
    pub launched: u64,
    /// Of them, the rows holding a breach the sim's own search found.
    pub holding_breach: u64,
}

/// One breach search, run several times.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct TimedSearch {
    /// The fastest run, in nanoseconds.
    ns: u128,
    /// What the search answered.
    outcome: Result<(), BreachRefused>,
    /// The states it expanded (`BreachScratch::expansions`), the same on every run.
    expanded: u32,
}

/// One run of a search: its time, what it answered and the states it expanded.
type SearchRun = (u128, Result<(), BreachRefused>, u32);

/// Run one search `runs` times, keeping the fastest run's time. The search is
/// a pure function of the world and the way, so its runs answer alike and
/// expand the same states.
///
/// # Errors
///
/// [`Error::Empty`] for no run at all, and [`Error::Drift`] when two runs
/// disagree on the answer or on the states expanded.
fn fastest_of(runs: usize, mut run: impl FnMut() -> SearchRun) -> Result<TimedSearch, Error> {
    let mut kept: Option<TimedSearch> = None;
    for _ in 0..runs {
        let (ns, outcome, expanded) = run();
        kept = Some(match kept {
            Some(before) if before.expanded != expanded || before.outcome != outcome => {
                return Err(Error::Drift(format!(
                    "a breach search answered {outcome:?} over {expanded} states on one run and \
                     {:?} over {} on another",
                    before.outcome, before.expanded
                )));
            }
            Some(before) => TimedSearch {
                ns: before.ns.min(ns),
                ..before
            },
            None => TimedSearch {
                ns,
                outcome,
                expanded,
            },
        });
    }
    kept.ok_or_else(|| Error::Empty("a breach search ran no time".to_owned()))
}

impl BreachProbe {
    /// Book one timed search: its outcome, its time and its expansions.
    ///
    /// # Errors
    ///
    /// [`Error::Drift`] when the count is not the one its answer means: the
    /// heap's room is never the limit (`BREACH_HEAP`'s rule), so a search is
    /// exhausted exactly when it read one past [`BREACH_EXPANSIONS`], and no
    /// search reads further. [`Error::Overflow`] for a sum past its type.
    fn book(&mut self, search: &TimedSearch) -> Result<(), Error> {
        let spent_the_bound = search.expanded > BREACH_EXPANSIONS;
        let exhausted = search.outcome == Err(BreachRefused::Exhausted);
        let past_bound = BREACH_EXPANSIONS
            .checked_add(1)
            .ok_or(Error::Overflow("the breach search's bound"))?;
        if spent_the_bound != exhausted || search.expanded > past_bound {
            return Err(Error::Drift(format!(
                "a breach search answered {:?} over {} states, against a bound of \
                 {BREACH_EXPANSIONS}: a search is exhausted exactly when it reads one past its \
                 bound",
                search.outcome, search.expanded
            )));
        }
        let tally = match search.outcome {
            Ok(()) => &mut self.found,
            Err(BreachRefused::Exhausted) => &mut self.exhausted,
            Err(BreachRefused::NoWay) => &mut self.no_way,
        };
        *tally = tally
            .checked_add(1)
            .ok_or(Error::Overflow("the breach outcomes"))?;
        self.slowest_ns = self.slowest_ns.max(search.ns);
        self.total_ns = self
            .total_ns
            .checked_add(search.ns)
            .ok_or(Error::Overflow("the breach searches' time"))?;
        self.most_expansions = self.most_expansions.max(search.expanded);
        self.total_expansions = self
            .total_expansions
            .checked_add(u64::from(search.expanded))
            .ok_or(Error::Overflow("the breach searches' expansions"))?;
        Ok(())
    }
}

/// Probe the breach search on `world`, read only.
fn probe_breach(world: &World, minute: u64) -> Result<BreachProbe, Error> {
    let mut probe = BreachProbe {
        minute,
        ..BreachProbe::default()
    };
    let beacons = world.beacons();
    let attack = MandateKind::Attack.id();
    let mut cores: Vec<(u8, [i32; 2])> = Vec::new();
    let mut rows: Vec<(u32, u8, [i32; 2], u8, bool)> = beacons
        .ids()
        .iter()
        .zip(beacons.seats())
        .zip(beacons.positions())
        .zip(beacons.mandates())
        .zip(beacons.hit_points())
        .map(|((((id, seat), at), mandate), hp)| {
            (
                *id,
                *seat,
                [at[0].floor_voxels(), at[1].floor_voxels()],
                *mandate,
                hp.is_alive(),
            )
        })
        .collect();
    rows.sort_unstable_by_key(|row| row.0);
    for (_, seat, at, _, alive) in &rows {
        if *alive && !cores.iter().any(|(owner, _)| owner == seat) {
            cores.push((*seat, *at));
        }
    }
    for row in world.attack().rows() {
        if row.phase == pharmakos_sim::attack::Phase::Launched {
            probe.launched = probe
                .launched
                .checked_add(1)
                .ok_or(Error::Overflow("the launched rows"))?;
            if row.breach.is_some() {
                probe.holding_breach = probe
                    .holding_breach
                    .checked_add(1)
                    .ok_or(Error::Overflow("the rows holding a breach"))?;
            }
        }
    }
    let surface = world.surface();
    let mut scratch = BreachScratch::new(surface.node_count());
    for (_, seat, at, mandate, alive) in &rows {
        if !*alive || *mandate != attack {
            continue;
        }
        let Some(from) = surface.node_of(at[0], at[1]) else {
            return Err(Error::Drift(format!(
                "an Attack beacon stands off the map at {at:?}"
            )));
        };
        for (owner, core) in &cores {
            if owner == seat {
                continue;
            }
            let to = surface
                .node_of(core[0], core[1])
                .ok_or_else(|| Error::Drift(format!("a core stands off the map at {core:?}")))?;
            probe.ways = probe.ways.checked_add(1).ok_or(Error::Overflow("ways"))?;
            let Some(way) = SealedWay::of(world, from, to) else {
                continue;
            };
            probe.sealed = probe
                .sealed
                .checked_add(1)
                .ok_or(Error::Overflow("sealed ways"))?;
            let search = fastest_of(BREACH_RUNS, || {
                let started = Instant::now();
                let found = breach_search(world, &mut scratch, SeatId::new(*seat), way);
                let ns = started.elapsed().as_nanos();
                (ns, found.map(|_| ()), scratch.expansions())
            })
            .and_then(|search| probe.book(&search))
            .map_err(|error| match error {
                Error::Drift(why) => Error::Drift(format!("from {at:?} to {core:?}: {why}")),
                other => other,
            });
            search?;
        }
    }
    Ok(probe)
}

/// What the segment found.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WatchReport {
    /// The map seed.
    pub seed: u64,
    /// The bench variant's `rules_hash`.
    pub rules_hash: u64,
    /// The layout's digest.
    pub layout_digest: u64,
    /// The load.
    pub load: GateLoad,
    /// Ticks played.
    pub ticks: u64,
    /// Each seat's completed listings, in seat order.
    pub listings: Vec<Vec<Listing>>,
    /// Each seat's most sighting rows after a tick, and the tick.
    pub sightings: Vec<(u32, u64)>,
    /// The sighting table's room per seat.
    pub sighting_room: u32,
    /// The memories each seat forgot for room over the segment, in seat order
    /// (the sim's own count, `World::sightings_forgotten_by`).
    pub forgotten: Vec<u64>,
    /// The sightings the table refused outright over the segment, which its
    /// sizing rules out (`World::sightings_refused`).
    pub sightings_refused: u64,
    /// Each minute's breach probe.
    pub breach: Vec<BreachProbe>,
    /// The sim's own refused-search counts at the end: spent its expansions,
    /// and no way across a wall.
    pub sim_refused: (u64, u64),
    /// The played segment's hash chain, digested as `g3::segment` digests its
    /// own (`pharmakos_sim::digest` over each tick's hash, little-endian), so
    /// it reads beside `g3_raids_segments`' RAIDS line.
    pub chain_digest: u64,
}

/// The sim's own counts of the sightings it dropped, read at one moment.
struct Dropped {
    /// The memories each seat forgot for room, in the order the seats were
    /// asked for.
    forgotten: Vec<u64>,
    /// The sightings the table refused outright.
    refused: u64,
}

impl Dropped {
    /// The world's counts for `seats`. A seat the world does not seat is
    /// drift, never a count of nothing.
    fn read(world: &World, seats: &[SeatId]) -> Result<Dropped, Error> {
        Dropped::of(
            seats,
            |seat| world.sightings_forgotten_by(seat),
            world.sightings_refused(),
        )
    }

    /// The counts for `seats` as `forgotten_by` gives each, with `refused`
    /// beside them. A seat with no count is drift, never zero. Pure.
    fn of(
        seats: &[SeatId],
        forgotten_by: impl Fn(SeatId) -> Option<u64>,
        refused: u64,
    ) -> Result<Dropped, Error> {
        let mut forgotten = Vec::with_capacity(seats.len());
        for seat in seats {
            forgotten.push(forgotten_by(*seat).ok_or_else(|| {
                Error::Drift(format!(
                    "the world keeps no forgotten-sightings count for seat {}, which the gate \
                     seats",
                    seat.raw()
                ))
            })?);
        }
        Ok(Dropped { forgotten, refused })
    }

    /// What was dropped since `before`, count by count. A count that ran
    /// backwards, or a second read that names other seats, is drift.
    fn since(&self, before: &Dropped) -> Result<Dropped, Error> {
        let backwards = || {
            Error::Drift("a count of dropped sightings ran backwards over the segment".to_owned())
        };
        if self.forgotten.len() != before.forgotten.len() {
            return Err(Error::Drift(
                "the two reads of the forgotten sightings name different seats".to_owned(),
            ));
        }
        let mut forgotten = Vec::with_capacity(self.forgotten.len());
        for (now, then) in self.forgotten.iter().zip(&before.forgotten) {
            forgotten.push(now.checked_sub(*then).ok_or_else(backwards)?);
        }
        Ok(Dropped {
            forgotten,
            refused: self
                .refused
                .checked_sub(before.refused)
                .ok_or_else(backwards)?,
        })
    }
}

/// Play the gate's segment with raids on `seed`, polling, counting and
/// probing between its ticks.
///
/// # Errors
///
/// As [`open_gate`]; [`Error::Gateway`] for a refused poll or tick; and
/// [`Error::Drift`] for an answer of a shape the wire does not have.
pub fn measure(root: &std::path::Path, seed: u64, segment_ms: i32) -> Result<WatchReport, Error> {
    let gate = open_gate(
        root,
        seed,
        segment_ms,
        super::stream::CRATERS_PER_SECOND,
        Quarters::FULL,
        GateLoad::RAIDS,
    )?;
    let mut surface = gate.surface;
    let mut stream = gate.stream;
    let mut pollers: Vec<Poller> = Vec::new();
    for seat in hosted::seat_ids(GATE_SEATS)? {
        pollers.push(Poller {
            seat,
            token: hosted::mint(&mut surface, seat)?,
            cursor: String::new(),
            open: None,
            since: 0,
            sensors: None,
            done: Vec::new(),
        });
    }
    let per_seat = hosted::world(&surface)?.sightings().per_seat();
    let seats: Vec<SeatId> = pollers.iter().map(|poller| poller.seat).collect();
    let dropped_before = Dropped::read(hosted::world(&surface)?, &seats)?;
    let mut peaks: Vec<(u32, u64)> = vec![(0, 0); pollers.len()];
    let mut probes: Vec<BreachProbe> = Vec::new();
    let mut played: u64 = 0;
    let mut chain: Vec<u8> = Vec::new();
    loop {
        {
            let host = surface
                .host_mut()
                .map_err(|error| Error::gateway("filing the crater stream", &error))?;
            stream.file(host, played)?;
        }
        let report = surface
            .step()
            .map_err(|error| Error::gateway("stepping a tick", &error))?;
        let Some(report) = report else {
            break;
        };
        played = played
            .checked_add(1)
            .ok_or(Error::Overflow("the ticks played"))?;
        chain.extend_from_slice(&report.hash.to_le_bytes());
        {
            let world = hosted::world(&surface)?;
            for (poller, peak) in pollers.iter().zip(peaks.iter_mut()) {
                let rows = world.sightings().len_of(poller.seat);
                if rows > peak.0 {
                    *peak = (rows, played);
                }
            }
            if played.rem_euclid(TICKS_PER_MINUTE) == 0 || report.segment_ended {
                probes.push(probe_breach(world, played.div_ceil(TICKS_PER_MINUTE))?);
            }
        }
        if report.segment_ended {
            break;
        }
        for poller in &mut pollers {
            poller.tick(&mut surface)?;
        }
    }
    let counts = hosted::world(&surface)?.attack_counts();
    let dropped = Dropped::read(hosted::world(&surface)?, &seats)?.since(&dropped_before)?;
    Ok(WatchReport {
        seed,
        rules_hash: gate.provenance.bench_rules_hash,
        layout_digest: gate.provenance.layout_digest,
        load: gate.provenance.load,
        ticks: played,
        listings: pollers.into_iter().map(|poller| poller.done).collect(),
        sightings: peaks,
        sighting_room: per_seat,
        forgotten: dropped.forgotten,
        sightings_refused: dropped.refused,
        breach: probes,
        sim_refused: (counts.breach_exhausted, counts.breach_no_way),
        chain_digest: pharmakos_sim::digest(&chain),
    })
}

/// A summary of `values`, or the words for none.
fn summary_words(values: Vec<u128>, unit: impl Fn(u128) -> String) -> String {
    match Summary::of(values) {
        Ok(summary) => format!(
            "p50 {} / p99 {} / max {} over {} listings",
            unit(summary.p50_ns),
            unit(summary.p99_ns),
            unit(summary.max_ns),
            summary.samples
        ),
        Err(Error::Empty(_)) => "none (no listing)".to_owned(),
        Err(other) => format!("not summarised ({other})"),
    }
}

/// The provenance notice. Pure.
#[must_use]
pub fn provenance_notice(report: &WatchReport, os: &str) -> String {
    format!(
        "::notice title=G3' watch provenance::on {os}: seed 0x{:016x}, bench rules_hash {}, \
         layout {}, load: Attack {}, every seat sealing {}, {} ticks, the played chain's digest \
         {}; every seat polls get_view a listing every {POLL_TICKS} ticks, at most \
         {PAGES_PER_TICK} pages a tick, between ticks (no tick here is timed, so this segment \
         is not G3''s)",
        report.seed,
        pharmakos_sim::hex(report.rules_hash),
        pharmakos_sim::hex(report.layout_digest),
        if report.load.raids {
            "launching"
        } else {
            "never launching"
        },
        report.load.sealed.name(),
        report.ticks,
        pharmakos_sim::hex(report.chain_digest),
    )
}

/// E15's view notice: each seat's entity list per delta listing, and its
/// keyframe. Pure.
#[must_use]
pub fn view_notice(report: &WatchReport, os: &str) -> String {
    let seats: Vec<String> = report
        .listings
        .iter()
        .enumerate()
        .map(|(seat, listings)| {
            let deltas: Vec<&Listing> = listings.iter().filter(|l| !l.keyframe).collect();
            let keyframe = match listings.iter().find(|l| l.keyframe) {
                Some(key) => format!(
                    "keyframe {} pages, {} answer bytes, {} entities in {} bytes",
                    key.pages, key.answer_bytes, key.entities, key.entity_bytes
                ),
                None => "no keyframe completed".to_owned(),
            };
            format!(
                "seat {seat}: {} delta listings, the entity list's bytes {}, its entities {}, \
                 the answer's bytes {}; {keyframe}",
                deltas.len(),
                summary_words(
                    deltas.iter().map(|l| u128::from(l.entity_bytes)).collect(),
                    |n| n.to_string()
                ),
                summary_words(
                    deltas.iter().map(|l| u128::from(l.entities)).collect(),
                    |n| n.to_string()
                ),
                summary_words(
                    deltas.iter().map(|l| u128::from(l.answer_bytes)).collect(),
                    |n| n.to_string()
                ),
            )
        })
        .collect();
    format!(
        "::notice title=G3' view feed bytes::on {os}, the whole entity list per get_view poll \
         at the gate's load (E15, S2-12's delta mode), each entity as the wire spells it: {}",
        seats.join("; ")
    )
}

/// The mean of `values`, or the words for none.
fn mean_words(values: &[u128], unit: &str) -> String {
    match crate::stats::mean(values) {
        Some(mean) => format!("{mean} {unit}"),
        None => "none".to_owned(),
    }
}

/// E15's recompute notice: the delta listings after the seat's sensors moved
/// against the ones after none moved, compared page count by page count (a
/// listing of more pages is a longer listing whatever the recompute did),
/// with each group's entity count beside its time. What is left is a
/// difference of means, not a cost per move: a listing can follow several
/// moves, or span ticks, and its entity list is not the other group's. Pure.
#[must_use]
pub fn recompute_notice(report: &WatchReport, os: &str) -> String {
    let deltas: Vec<&Listing> = report
        .listings
        .iter()
        .flatten()
        .filter(|listing| !listing.keyframe)
        .collect();
    let mut pages: Vec<u32> = deltas.iter().map(|listing| listing.pages).collect();
    pages.sort_unstable();
    pages.dedup();
    let groups: Vec<String> = pages
        .iter()
        .map(|count| {
            let of = |moved: bool| -> Vec<&Listing> {
                deltas
                    .iter()
                    .filter(|listing| listing.pages == *count && listing.moved == moved)
                    .copied()
                    .collect()
            };
            let (moved, still) = (of(true), of(false));
            let ns = |group: &[&Listing]| group.iter().map(|l| l.ns).collect::<Vec<u128>>();
            let entities = |group: &[&Listing]| {
                group
                    .iter()
                    .map(|l| u128::from(l.entities))
                    .collect::<Vec<u128>>()
            };
            let difference = match (
                crate::stats::mean(&ns(&moved)),
                crate::stats::mean(&ns(&still)),
            ) {
                (Some(after_move), Some(after_none)) => {
                    match (i128::try_from(after_move), i128::try_from(after_none)) {
                        (Ok(a), Ok(b)) => match a.checked_sub(b) {
                            Some(ns) => format!("a difference of means of {ns} ns"),
                            None => "a difference past an i128".to_owned(),
                        },
                        _ => "a mean past an i128".to_owned(),
                    }
                }
                _ => "no difference: one side holds no listing".to_owned(),
            };
            format!(
                "{count} page(s): after a move {} listings, mean {} with mean {} entities; after \
                 none {} listings, mean {} with mean {} entities; {difference}",
                moved.len(),
                mean_words(&ns(&moved), "ns"),
                mean_words(&entities(&moved), "").trim_end(),
                still.len(),
                mean_words(&ns(&still), "ns"),
                mean_words(&entities(&still), "").trim_end(),
            )
        })
        .collect();
    format!(
        "::notice title=G3' view recompute::on {os}, S1-34's recompute key at the gate's load, \
         delta listings after the seat's sensors moved since its last listing against those \
         after none moved, page count by page count: {}. A difference of means, not a cost per \
         move: a listing can follow several moves or span ticks, and the two groups' entity \
         lists differ",
        groups.join("; "),
    )
}

/// E17's sightings notice: each seat's own peak beside the memories the sim
/// counted it forgetting for room. A seat that forgot none held its true
/// peak, provided the table refused no sighting over the segment (refusals
/// are counted for all the seats together, so one refusal leaves every
/// seat's peak unproven); a seat that forgot any held a peak the room cut
/// short, and the notice says so in capitals; a seat with no count is said to
/// have none, never read as zero. Pure.
#[must_use]
pub fn sightings_notice(report: &WatchReport, os: &str) -> String {
    let seats: Vec<String> = report
        .sightings
        .iter()
        .enumerate()
        .map(|(seat, (rows, tick))| match report.forgotten.get(seat) {
            Some(0) if report.sightings_refused == 0 => {
                format!("seat {seat} {rows} at tick {tick}, none forgotten for room: its true peak")
            }
            Some(0) => format!(
                "seat {seat} {rows} at tick {tick}, none forgotten for room, but SIGHTINGS WERE \
                 REFUSED: its true peak is not known"
            ),
            Some(forgotten) => format!(
                "seat {seat} {rows} at tick {tick}, {forgotten} FORGOTTEN FOR ROOM: the room cut \
                 its peak short"
            ),
            None => format!("seat {seat} {rows} at tick {tick}, NO COUNT of what it forgot"),
        })
        .collect();
    format!(
        "::notice title=G3' sightings::on {os}, the most sighting rows each seat held after a \
         tick over the gate's segment (E17), and the memories the sim counted it forgetting for \
         room: {}, against the table's {} a seat; {} sightings refused outright",
        seats.join(", "),
        report.sighting_room,
        report.sightings_refused,
    )
}

/// The breach notice. Pure.
#[must_use]
pub fn breach_notice(report: &WatchReport, os: &str) -> String {
    let minutes: Vec<String> = report
        .breach
        .iter()
        .map(|probe| {
            format!(
                "minute {}: {} ways, {} sealed, {} found, {} exhausted, {} no way, slowest {} ns, \
                 {} ns in all, the most one search expanded {} states, {} in all; the sim's own \
                 {} launched attacks, {} holding a breach",
                probe.minute,
                probe.ways,
                probe.sealed,
                probe.found,
                probe.exhausted,
                probe.no_way,
                probe.slowest_ns,
                probe.total_ns,
                probe.most_expansions,
                probe.total_expansions,
                probe.launched,
                probe.holding_breach,
            )
        })
        .collect();
    format!(
        "::notice title=G3' breach search::on {os}, a probe of the search's cost on the gate's \
         world, not the sim's own searches: once a minute, from every live Attack beacon's \
         column to every enemy core's own column (a goal no walker stands on, so the oracle \
         calls such a way sealed and the search runs to a breach, to its expansions' bound or \
         to no way), the search run read-only and timed, fastest of {BREACH_RUNS}, its \
         expansions the sim's own count against a bound of {BREACH_EXPANSIONS} (a search that \
         spends the bound reads one past it): {}; the sim's own refused searches over the \
         segment: {} spent their expansions, {} found no way",
        minutes.join("; "),
        report.sim_refused.0,
        report.sim_refused.1,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(fields: &[(&str, Json)]) -> Json {
        Json::Object(
            fields
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect(),
        )
    }

    /// C4: a page missing its entities, its `complete` or its cursor is a
    /// typed drift naming the field, never zero bytes or "not complete".
    #[test]
    fn a_page_missing_a_field_is_drift_naming_it() {
        let whole = [
            ("entities", Json::Array(vec![Json::Null])),
            ("complete", Json::Bool(true)),
            ("next_cursor", Json::String("c".to_owned())),
        ];
        let read = page(&whole);
        let ok = read_page(&read).expect("a whole page");
        assert_eq!(
            (ok.entities.len(), ok.complete, ok.cursor.as_str()),
            (1, true, "c")
        );
        for missing in ["entities", "complete", "next_cursor"] {
            let cut: Vec<(&str, Json)> = whole
                .iter()
                .filter(|(key, _)| *key != missing)
                .cloned()
                .collect();
            match read_page(&page(&cut)) {
                Err(Error::Drift(words)) => {
                    assert!(words.contains(&format!("`{missing}`")), "{words}");
                }
                other => panic!("{missing}: {other:?}"),
            }
        }
    }

    fn listing(pages: u32, moved: bool, ns: u128, entities: u64) -> Listing {
        Listing {
            keyframe: false,
            moved,
            pages,
            entities,
            entity_bytes: 0,
            answer_bytes: 0,
            ns,
        }
    }

    fn report(listings: Vec<Vec<Listing>>, sightings: Vec<(u32, u64)>) -> WatchReport {
        let forgotten = vec![0; sightings.len()];
        WatchReport {
            seed: 1,
            rules_hash: 2,
            layout_digest: 3,
            load: GateLoad::RAIDS,
            ticks: 10,
            listings,
            sightings,
            sighting_room: 64,
            forgotten,
            sightings_refused: 0,
            breach: Vec::new(),
            sim_refused: (0, 0),
            chain_digest: 4,
        }
    }

    /// The fastest run's time is kept, with the answer and the count every
    /// run gave; no run at all is `Empty`, and runs that disagree on the
    /// count or on the answer are `Drift`.
    #[test]
    fn a_timed_search_keeps_its_fastest_run_and_refuses_runs_that_disagree() {
        let mut runs = [(9, Ok(()), 40), (4, Ok(()), 40), (7, Ok(()), 40)].into_iter();
        let search = fastest_of(3, || runs.next().expect("three runs")).expect("runs that agree");
        assert_eq!(
            search,
            TimedSearch {
                ns: 4,
                outcome: Ok(()),
                expanded: 40
            }
        );
        assert!(matches!(
            fastest_of(0, || (1, Ok(()), 1)),
            Err(Error::Empty(_))
        ));
        let mut counts = [(1, Ok(()), 40), (1, Ok(()), 41)].into_iter();
        assert!(matches!(
            fastest_of(2, || counts.next().expect("two runs")),
            Err(Error::Drift(_))
        ));
        let mut answers = [(1, Ok(()), 40), (1, Err(BreachRefused::NoWay), 40)].into_iter();
        assert!(matches!(
            fastest_of(2, || answers.next().expect("two runs")),
            Err(Error::Drift(_))
        ));
    }

    /// A probe books each search's answer, its time and its count: the most
    /// one search expanded and their sum, the slowest and the summed time.
    #[test]
    fn a_probe_books_the_most_and_the_sum_of_its_searches() {
        let past_bound = BREACH_EXPANSIONS.checked_add(1).expect("the bound fits");
        let mut probe = BreachProbe::default();
        for (ns, outcome, expanded) in [
            (5, Ok(()), 30),
            (9, Err(BreachRefused::NoWay), 70),
            (2, Err(BreachRefused::Exhausted), past_bound),
            (3, Ok(()), 10),
        ] {
            probe
                .book(&TimedSearch {
                    ns,
                    outcome,
                    expanded,
                })
                .expect("a search whose count is its answer's");
        }
        assert_eq!((probe.found, probe.no_way, probe.exhausted), (2, 1, 1));
        assert_eq!((probe.slowest_ns, probe.total_ns), (9, 19));
        assert_eq!(probe.most_expansions, past_bound);
        assert_eq!(probe.total_expansions, u64::from(past_bound) + 110);
    }

    /// A count that is not its answer's is refused, not booked: exhausted
    /// under the bound, an answer past it, and any count further than one
    /// past the bound.
    #[test]
    fn a_probe_refuses_a_count_that_is_not_its_answers() {
        let past_bound = BREACH_EXPANSIONS.checked_add(1).expect("the bound fits");
        for (outcome, expanded) in [
            (Err(BreachRefused::Exhausted), BREACH_EXPANSIONS),
            (Err(BreachRefused::Exhausted), 0),
            (Ok(()), past_bound),
            (Err(BreachRefused::NoWay), past_bound),
            (
                Err(BreachRefused::Exhausted),
                past_bound.checked_add(1).expect("fits"),
            ),
        ] {
            let mut probe = BreachProbe::default();
            let booked = probe.book(&TimedSearch {
                ns: 1,
                outcome,
                expanded,
            });
            assert!(
                matches!(booked, Err(Error::Drift(_))),
                "{outcome:?} over {expanded}"
            );
            assert_eq!(probe, BreachProbe::default(), "nothing is booked");
        }
    }

    /// The breach notice names each minute's expansions, the most one search
    /// spent and their sum, and the bound they are read against.
    #[test]
    fn the_breach_notice_names_the_expansions_and_their_bound() {
        let mut watched = report(Vec::new(), Vec::new());
        watched.breach = vec![BreachProbe {
            minute: 1,
            ways: 2,
            sealed: 2,
            exhausted: 1,
            no_way: 1,
            slowest_ns: 9,
            total_ns: 12,
            most_expansions: 16_385,
            total_expansions: 16_390,
            ..BreachProbe::default()
        }];
        let line = breach_notice(&watched, "linux");
        assert!(
            line.contains(
                "minute 1: 2 ways, 2 sealed, 0 found, 1 exhausted, 1 no way, slowest 9 ns, 12 ns \
                 in all, the most one search expanded 16385 states, 16390 in all; the sim's own 0 \
                 launched attacks, 0 holding a breach"
            ),
            "{line}"
        );
        assert!(line.contains("against a bound of 16384"), "{line}");
        assert!(!line.contains("not read"), "{line}");
    }

    /// C5: listings compared page count by page count, the entity count beside
    /// each mean, and the figure named a difference of means.
    #[test]
    fn the_recompute_notice_is_word_for_word() {
        let line = recompute_notice(
            &report(
                vec![vec![
                    listing(1, true, 3_000, 50),
                    listing(1, false, 1_000, 40),
                    listing(2, true, 9_000, 120),
                ]],
                Vec::new(),
            ),
            "linux",
        );
        assert_eq!(
            line,
            "::notice title=G3' view recompute::on linux, S1-34's recompute key at the gate's \
             load, delta listings after the seat's sensors moved since its last listing against \
             those after none moved, page count by page count: 1 page(s): after a move 1 \
             listings, mean 3000 ns with mean 50 entities; after none 1 listings, mean 1000 ns \
             with mean 40 entities; a difference of means of 2000 ns; 2 page(s): after a move 1 \
             listings, mean 9000 ns with mean 120 entities; after none 0 listings, mean none \
             with mean none entities; no difference: one side holds no listing. A difference of \
             means, not a cost per move: a listing can follow several moves or span ticks, and \
             the two groups' entity lists differ"
        );
    }

    /// C6: each seat's own peak is printed beside its own forgotten count
    /// (O9's mutant read seat 0's alone): a seat that forgot none held its
    /// true peak whatever the room, a seat that forgot any is said to have
    /// been cut short, and the refused count closes the line.
    #[test]
    fn each_seats_peak_and_forgotten_count_are_its_own() {
        let mut held = report(Vec::new(), vec![(64, 1), (10, 5), (63, 7)]);
        held.forgotten = vec![0, 3, 0];
        assert_eq!(
            sightings_notice(&held, "linux"),
            "::notice title=G3' sightings::on linux, the most sighting rows each seat held after \
             a tick over the gate's segment (E17), and the memories the sim counted it forgetting \
             for room: seat 0 64 at tick 1, none forgotten for room: its true peak, seat 1 10 at \
             tick 5, 3 FORGOTTEN FOR ROOM: the room cut its peak short, seat 2 63 at tick 7, none \
             forgotten for room: its true peak, against the table's 64 a seat; 0 sightings \
             refused outright"
        );
        // A refusal is counted for all the seats together, so with one no
        // seat's peak is called its true peak, whatever it forgot.
        held.sightings_refused = 2;
        assert_eq!(
            sightings_notice(&held, "linux"),
            "::notice title=G3' sightings::on linux, the most sighting rows each seat held after \
             a tick over the gate's segment (E17), and the memories the sim counted it forgetting \
             for room: seat 0 64 at tick 1, none forgotten for room, but SIGHTINGS WERE REFUSED: \
             its true peak is not known, seat 1 10 at tick 5, 3 FORGOTTEN FOR ROOM: the room cut \
             its peak short, seat 2 63 at tick 7, none forgotten for room, but SIGHTINGS WERE \
             REFUSED: its true peak is not known, against the table's 64 a seat; 2 sightings \
             refused outright"
        );
    }

    /// A seat with a peak and no forgotten count is never read as having
    /// forgotten nothing.
    #[test]
    fn a_seat_with_no_forgotten_count_is_not_read_as_zero() {
        let mut held = report(Vec::new(), vec![(5, 1), (6, 2)]);
        held.forgotten = vec![0];
        let line = sightings_notice(&held, "linux");
        assert!(
            line.contains("seat 1 6 at tick 2, NO COUNT of what it forgot"),
            "{line}"
        );
    }

    /// A seat the world keeps no count for is drift naming the seat, never a
    /// count of nothing (review 2's mutant B3b).
    #[test]
    fn a_seat_with_no_count_in_the_world_is_drift_not_zero() {
        let counts = |seat: SeatId| (seat.raw() < 2).then_some(u64::from(seat.raw()) + 3);
        let read = Dropped::of(&[SeatId::new(0), SeatId::new(1)], counts, 4)
            .expect("both seats have a count");
        assert_eq!((read.forgotten, read.refused), (vec![3, 4], 4));
        match Dropped::of(&[SeatId::new(0), SeatId::new(5)], counts, 4) {
            Err(Error::Drift(words)) => assert!(words.contains("seat 5"), "{words}"),
            other => panic!(
                "a seat with no count: {:?}",
                other.map(|read| read.forgotten)
            ),
        }
    }

    /// The drops over a segment are the end's counts less the start's, seat
    /// by seat, and a count that ran backwards or a seat list that changed is
    /// drift, never a wrapped or clamped figure.
    #[test]
    fn the_drops_over_a_segment_are_a_checked_difference() {
        let before = Dropped {
            forgotten: vec![1, 0, 7],
            refused: 2,
        };
        let after = Dropped {
            forgotten: vec![4, 0, 7],
            refused: 5,
        };
        let over = after.since(&before).expect("counts that only grew");
        assert_eq!((over.forgotten, over.refused), (vec![3, 0, 0], 3));
        assert!(matches!(before.since(&after), Err(Error::Drift(_))));
        let fewer = Dropped {
            forgotten: vec![1, 0],
            refused: 2,
        };
        assert!(matches!(after.since(&fewer), Err(Error::Drift(_))));
        let refused_back = Dropped {
            forgotten: vec![4, 0, 7],
            refused: 1,
        };
        assert!(matches!(refused_back.since(&before), Err(Error::Drift(_))));
    }
}
