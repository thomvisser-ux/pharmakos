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
//!   apart, and the recompute's cost per sensor move is the difference of
//!   their mean times.
//! * **Sightings** (E17): each seat's rows of the sim's sighting table after
//!   every tick, `SightingTable::len_of`, and the peak against the table's
//!   room per seat, `SightingTable::per_seat`.
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
//!   restate). The expansions a search spends are not read: `breach_search`
//!   keeps them in a local and returns none, so the figure needs a sim API
//!   this lane may not add.
//!
//! Published, never gated. The polls and the probes run between ticks, so
//! no tick here is timed: this segment is not G3′'s.

use std::time::Instant;

use pharmakos_gateway::fog::Blind;
use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::token::Token;
use pharmakos_proto::json::{self, Json};
use pharmakos_sim::attack::{BreachScratch, SealedWay, breach_search};
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
    /// The entities its completing page's whole list held.
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
        let entities: &[Json] = match result.get("entities") {
            Some(Json::Array(items)) => items,
            None => &[],
            Some(other) => {
                return Err(Error::Drift(format!(
                    "get_view's `entities` is not a list: {}",
                    json::write(other)
                )));
            }
        };
        let complete = match result.get("complete") {
            Some(Json::Bool(complete)) => *complete,
            None => false,
            Some(other) => {
                return Err(Error::Drift(format!(
                    "get_view's `complete` is not a bool: {}",
                    json::write(other)
                )));
            }
        };
        let cursor = match result.get("next_cursor") {
            Some(Json::String(cursor)) => cursor.clone(),
            other => {
                return Err(Error::Drift(format!(
                    "get_view answered no `next_cursor`: {other:?}"
                )));
            }
        };
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
    /// The sim's own Attack rows launched at the probe.
    pub launched: u64,
    /// Of them, the rows holding a breach the sim's own search found.
    pub holding_breach: u64,
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
            let mut best: Option<u128> = None;
            let mut outcome = None;
            for _ in 0..BREACH_RUNS {
                let started = Instant::now();
                let found = breach_search(world, &mut scratch, SeatId::new(*seat), way);
                let ns = started.elapsed().as_nanos();
                best = Some(match best {
                    Some(kept) => kept.min(ns),
                    None => ns,
                });
                outcome = Some(found.map(|_| ()));
            }
            let ns = best.ok_or_else(|| Error::Empty("a breach search run no time".to_owned()))?;
            let tally = match outcome {
                Some(Ok(())) => &mut probe.found,
                Some(Err(pharmakos_sim::attack::BreachRefused::Exhausted)) => &mut probe.exhausted,
                Some(Err(pharmakos_sim::attack::BreachRefused::NoWay)) => &mut probe.no_way,
                None => return Err(Error::Empty("a breach search ran no time".to_owned())),
            };
            *tally = tally
                .checked_add(1)
                .ok_or(Error::Overflow("the breach outcomes"))?;
            probe.slowest_ns = probe.slowest_ns.max(ns);
            probe.total_ns = probe
                .total_ns
                .checked_add(ns)
                .ok_or(Error::Overflow("the breach searches' time"))?;
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
    /// Each minute's breach probe.
    pub breach: Vec<BreachProbe>,
    /// The sim's own refused-search counts at the end: spent its expansions,
    /// and no way across a wall.
    pub sim_refused: (u64, u64),
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
    let mut peaks: Vec<(u32, u64)> = vec![(0, 0); pollers.len()];
    let mut probes: Vec<BreachProbe> = Vec::new();
    let mut played: u64 = 0;
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
    Ok(WatchReport {
        seed,
        rules_hash: gate.provenance.bench_rules_hash,
        layout_digest: gate.provenance.layout_digest,
        load: gate.provenance.load,
        ticks: played,
        listings: pollers.into_iter().map(|poller| poller.done).collect(),
        sightings: peaks,
        sighting_room: per_seat,
        breach: probes,
        sim_refused: (counts.breach_exhausted, counts.breach_no_way),
    })
}

/// A summary of `values`, or the words for none.
fn summary_words(values: Vec<u128>, unit: impl Fn(u128) -> String) -> String {
    match Summary::of(values) {
        Ok(summary) => format!(
            "p50 {} / p99 {} / max {} over {}",
            unit(summary.p50_ns),
            unit(summary.p99_ns),
            unit(summary.max_ns),
            summary.samples
        ),
        Err(_) => "none".to_owned(),
    }
}

/// The provenance notice. Pure.
#[must_use]
pub fn provenance_notice(report: &WatchReport, os: &str) -> String {
    format!(
        "::notice title=G3' watch provenance::on {os}: seed 0x{:016x}, bench rules_hash {}, \
         layout {}, load: Attack {}, every seat sealing {}, {} ticks; every seat polls get_view a listing every {POLL_TICKS} \
         ticks, at most {PAGES_PER_TICK} pages a tick, between ticks (no tick here is timed, so \
         this segment is not G3''s)",
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

/// E15's recompute notice: the delta listings after a sensor moved against
/// the ones after none did, and the difference of their means. Pure.
#[must_use]
pub fn recompute_notice(report: &WatchReport, os: &str) -> String {
    let deltas: Vec<&Listing> = report
        .listings
        .iter()
        .flatten()
        .filter(|listing| !listing.keyframe)
        .collect();
    let moved: Vec<u128> = deltas.iter().filter(|l| l.moved).map(|l| l.ns).collect();
    let still: Vec<u128> = deltas.iter().filter(|l| !l.moved).map(|l| l.ns).collect();
    let cost = match (crate::stats::mean(&moved), crate::stats::mean(&still)) {
        (Some(moved_mean), Some(still_mean)) => {
            let difference = i128::try_from(moved_mean)
                .ok()
                .zip(i128::try_from(still_mean).ok())
                .and_then(|(a, b)| a.checked_sub(b));
            match difference {
                Some(ns) => format!("the recompute's cost per sensor move is {ns} ns"),
                None => "the difference does not fit an i128".to_owned(),
            }
        }
        (None, _) => "no listing followed a sensor move".to_owned(),
        (_, None) => {
            "no listing found every sensor where it was, so the recompute's cost cannot be told \
             from a listing's"
                .to_owned()
        }
    };
    format!(
        "::notice title=G3' view recompute::on {os}, S1-34's recompute key at the gate's load: \
         a delta listing after the seat's sensors moved {} ns, after none moved {} ns; {cost}",
        summary_words(moved, |n| n.to_string()),
        summary_words(still, |n| n.to_string()),
    )
}

/// E17's sightings notice. Pure.
#[must_use]
pub fn sightings_notice(report: &WatchReport, os: &str) -> String {
    let seats: Vec<String> = report
        .sightings
        .iter()
        .enumerate()
        .map(|(seat, (rows, tick))| format!("seat {seat} {rows} at tick {tick}"))
        .collect();
    format!(
        "::notice title=G3' sightings::on {os}, the most sighting rows a seat held after a tick \
         over the gate's segment (E17): {}, against the table's {} a seat",
        seats.join(", "),
        report.sighting_room,
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
                 {} ns in all; the sim's own {} launched attacks, {} holding a breach",
                probe.minute,
                probe.ways,
                probe.sealed,
                probe.found,
                probe.exhausted,
                probe.no_way,
                probe.slowest_ns,
                probe.total_ns,
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
         to no way), the search run read-only and timed, fastest of {BREACH_RUNS}: {}; the \
         sim's own refused searches over the segment: {} spent their expansions, {} found no \
         way. Expansions per search are not read: the search keeps them in a local and returns \
         none",
        minutes.join("; "),
        report.sim_refused.0,
        report.sim_refused.1,
    )
}
