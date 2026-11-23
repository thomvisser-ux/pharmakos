// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Survey: what a seat sees, how stale the memory of it is (spec section 6,
//! "Vision and sightings"), and the Survey mandate's scouts (spec section 6's
//! Survey row; S2's `surv`). The scouts' part is under "Full Survey" below.
//!
//! # What "reach" is, said plainly
//!
//! The spec's reach is "your spheres, the live vision of your own units and
//! structures, and Sensor Spire reveals". Since S2's `ids` lane the first two
//! come through the one sensor interface ([`crate::sight::Sensor`]; S2's plan,
//! E10), the knowledge store's output of it:
//!
//! * every **live beacon**'s sphere, the passive vision every seat has from
//!   the first tick: `beacon.sphere_radius_voxels`, grown by a finished
//!   Resonance Spire (S3's `lic`; [`crate::spire::radius_voxels`]);
//! * every living **unit**'s own vision, its kind's `vision_radius_voxels`
//!   row (the commander's `commander.vision_radius_voxels`), which is what a
//!   Survey mandate buys by fielding scouts and sending them to its probe
//!   areas; and
//! * every finished **structure** a seat owns, its kind's
//!   `vision_radius_voxels` row.
//!
//! Each sees everything within its radius, with no line of sight (item 144
//! (4)); the line-of-sight model is the interface's seam
//! ([`crate::sight::LineOfSight`]). Sensor Spire reveals are S4's, and they
//! join as one more sensor.
//!
//! A unit is found through the broadphase grid, padded by the most a unit
//! moves in a tick and tested at its current position ([`World::units_within`];
//! E11), so a unit that walked into range since the grid was built is seen.
//! The beacon and structure tables are small and walked whole.
//!
//! # The age accrues on match game time
//!
//! A sighting stores the **tick it was taken at**, and its age is derived
//! against the current tick. That is what makes item 61's sentence true for
//! free: the age "accrues on match game time and runs across Pushes; the Lull
//! and the recap add nothing to it", because neither consumes a tick
//! ([`crate::runner`]). A stored age would have had to be advanced by somebody,
//! and whoever forgot to would have made the Lull free.
//!
//! # Why this runs on a decision tick
//!
//! Recording runs once per `match.decision_tick_ms` rather than every tick: a
//! sighting is read by a decision, so a sighting taken between two decisions
//! could never be acted on any sooner. The cadence is game time, identical on
//! every machine, and it is the same cadence the interpreter's own decisions run
//! on.
//!
//! # A sighting names the asset it saw
//!
//! A sighting's asset is an [`AssetId`] with its row's generation (S2's plan,
//! E1), so a memory of a unit whose row has since been reused stays a memory of
//! that unit, and the unit that took the row is a new sighting of its own.
//!
//! # Full Survey (S2's `surv`)
//!
//! Spec section 6's Survey row: scouts "scan unexplored ground first, then
//! re-check the sightings closest to leaving the reach window, enemy beacons
//! first", "refresh those sightings before the next Lull", "avoid known
//! threats and report back". Each clause is one rule here, which every scout
//! runs on every decision tick ([`run_scout`]):
//!
//! * **Explored ground** is the per-seat explored table over coarse cells
//!   [`CELL_VOXELS`] columns square ([`cell_of`]). A Survey scout marks a
//!   cell for its seat on a decision tick its sensor is on when it stands on
//!   the cell's scan column ([`record_sightings`]): the cell's centre column,
//!   brought inside the first of its beacon's probe areas the cell overlaps,
//!   the place it looks round the cell from. Only a Survey beacon's scouts
//!   mark: a Defend beacon's pickets warn, they do not survey
//!   ([`World::survey_scout_sensor`]). A cell once explored stays explored,
//!   and each mark writes the tick it was made on, so a scout standing on the
//!   scan column again renews it.
//! * **The re-scan** (S3's plan, decision 7, ruled by item 154 (4); S3's
//!   `pred`): an explored cell is **due** again once it is older than reach
//!   memory (`verifier.reach_memory_ms`), the one clock a seat's Attack
//!   targets are held to, so a seat's map stays as fresh as its targets must
//!   be. A scout takes a due cell only when no unexplored cell is left and no
//!   sighting is due a re-check, chosen by the same selection and order as an
//!   unexplored one ([`nearest_cell`]).
//! * **The scan.** The scout walks to the nearest unexplored cell of its
//!   beacon's probe areas, ties to the lowest cell: to the cell's scan column,
//!   when that column is ground the scout can reach (the router's own
//!   connectivity oracle), so a scan never sends a scout to a column it cannot
//!   stand on.
//! * **The refresh pass.** The due sightings are re-checked before the next
//!   Lull, so that its playbook is written on fresh knowledge (decisions-log
//!   item 13; never cut, item 144 (11); the sentence binds the sightings a
//!   seat already holds, decisions-log item 173 (1), and the refresh of an
//!   enemy core wins over the scouts it costs, item 173 (2)). A sighting is
//!   **due** once it is [`REFRESH_INTERVAL`]
//!   old, and stays due for as long as the seat holds it, past the reach
//!   window (`verifier.reach_memory_ms`) as much as inside it: a Push can be
//!   longer than the window, so a sighting the scan kept the scouts from has
//!   left the window by the time they turn to it, and the re-check is what
//!   brings it back. The due sightings rank **enemy beacons first**, then
//!   oldest first -- the one closest to leaving the window, or longest out of
//!   it -- ties to the lowest asset id. A seat's scouts are decided
//!   **together** on every decision tick ([`decide_seat`] states the
//!   construction and its order): every due sighting goes into one scout's
//!   **list**, the ordered sightings it will visit -- a scout already walking
//!   to one keeps it at its head, and each other is appended to the list
//!   that finishes soonest with it -- and a scout leaves its scan only when
//!   it must: it has nothing to scan, or the Push is in its last
//!   [`REFRESH_INTERVAL`], or its list up to its last sighting that would be
//!   out of the window at the Lull, the served legs in order at the scout's
//!   speed, at the latest, no longer leaves time to scan on
//!   ([`list_leaves_time`]), priced once every [`REPRICE_DECISIONS`]
//!   decisions while the legs' octile bounds leave time and every decision
//!   once they do not. A sighting no list reaches before the Lull is counted,
//!   never silently stale. Every other scout scans.
//! * **A re-check that finds nothing forgets.** A due sighting whose place one
//!   of the seat's Survey scouts has in sight, and which no sensor renewed on
//!   that decision tick, remembers an asset that is no longer there, and the
//!   seat forgets it ([`World::forget_sighting`]; the sighting table is hashed
//!   and snapshotted whole, so this is hashed state like any sighting). A
//!   sighting younger than [`REFRESH_INTERVAL`] is not due a re-check and is
//!   kept, so an asset that has just walked out of a scout's sight is still
//!   remembered where it was last seen. Only a Survey scout's re-check
//!   forgets: a sphere or a picket that sees the place passively changes no
//!   memory.
//! * **Known threats** (decision 12, under item 148's seat sight): a cell or
//!   a sighting inside the weapon range of an armed enemy asset the seat sees
//!   now is no destination. The walk itself is the router's: a route that
//!   prices a known threat's range is `AVOID_KNOWN_THREATS`, S2's `atk`'s.
//! * **Several scouts** share the work: a sighting is in one scout's list (the
//!   refresh pass above), and a cell a scout of the same beacon in
//!   a lower row is already walking to is not scanned again.
//! * **Arrival.** A scout that has reached the place it was sent to parks
//!   there, so the wandering a unit at the end of its route is given
//!   ([`World`]'s arrival draw) does not carry it off before it looks.
//! * **Reporting back** is three private feed lines: `area_surveyed` when a
//!   scout's mark takes its beacon's last scannable cell, `enemy_beacon_found`
//!   for an enemy beacon its seat holds no sighting of, and
//!   `sighting_refreshed` for a sighting older than [`REFRESH_INTERVAL`] a
//!   Survey scout renewed.
//!
//! With nothing to scan and nothing due, the scout idles at its beacon.

use crate::events::{Emission, EventKind};
use crate::ids::Row;
use crate::knowledge::{AssetId, AssetKind, Position};
use crate::math::fixed::{Fx, Sq};
use crate::math::quantity::Ms;
use crate::pathing::estimate::{EstimateError, WalkError, walk_cost, walk_ticks};
use crate::pathing::surface::Node;
use crate::pathing::{Speed, ticks_for_cost};
use crate::seams::MandateKind;
use crate::sight::{RadiusOnly, Sensor};
use crate::tables::{
    BeaconId, ColumnBox, NO_BLUEPRINT, SeatId, StructureId, TargetKind, UnitId, UnitKind,
};
use crate::targeting::column_of;
use crate::world::World;

/// How many decision intervals apart the served legs of a scout's list are
/// priced ([`pricing_decision`]): every eighth, two
/// seconds of game time at `match.decision_tick_ms` 250. The departure rule's
/// slack grows with it ([`list_leaves_time`]), so a longer cadence prices
/// fewer walks and sets a scout off up to that much sooner.
// PLACEHOLDER: Survey's re-pricing cadence, every 8th decision (2 s) — owner, at S3's demo (item 152 (1) 2), with `tune3`'s balance re-run
pub const REPRICE_DECISIONS: u32 = 8;

/// The explored table's coarse cell, in columns on a side.
///
/// A scout standing on a cell's centre sees all of it while its vision radius
/// is at least the cell's half-diagonal, about 11.3 columns here: true at
/// decision 17's scout vision of 24, which the rules table holds since its
/// revision 6 (`tune2`); at the 8 it held before, a cell's corners could be
/// marked unseen.
// PLACEHOLDER: Survey's coarse cell, 16 columns square, which a scout of vision 24 sees whole from its centre — owner, at S3's demo (item 152 (1) 2), with `tune3`'s balance re-run
pub const CELL_VOXELS: i32 = 16;

/// Survey's refresh interval: a sighting this old is due a re-check, and a
/// Push's last this-long is the refresh pass for every scout, and for a scout
/// with a longer walk to make it starts sooner (decisions-log item 13).
// PLACEHOLDER: Survey's refresh interval, 60 s of game time, a third of reach memory's 180 s — owner, at S3's demo (item 152 (1) 2), with `tune3`'s balance re-run
pub const REFRESH_INTERVAL: Ms = Ms::new(60_000);

/// The cell id's stride between two rows of cells: the id packs the cell row
/// above the cell column, so the explored table's ascending order is
/// row-major.
const CELL_ROW: u32 = 1 << 16;

/// The explored-table cell holding column `[x, y]`, or `None` for a column
/// west or south of the map's origin, which is on no map.
#[must_use]
pub fn cell_of(column: [i32; 2]) -> Option<u32> {
    let [x, y] = column;
    if x < 0 || y < 0 {
        return None;
    }
    pack(x.checked_div(CELL_VOXELS)?, y.checked_div(CELL_VOXELS)?)
}

/// A cell id from its cell column and cell row, or `None` past `u16` cells.
fn pack(cx: i32, cy: i32) -> Option<u32> {
    let cx = u32::from(u16::try_from(cx).ok()?);
    let cy = u32::from(u16::try_from(cy).ok()?);
    cy.checked_mul(CELL_ROW)?.checked_add(cx)
}

/// Record what every seat can see, right now; and what its Survey scouts
/// explored and found.
///
/// The Survey scouts look first, in ascending unit row, so a report reads the
/// memory as it stood before this tick, and each marks the cells it surveyed.
/// Then every seat in ascending id, and within a seat sensor by sensor in the
/// interface's order ([`World::sensor_at`]), so the order sightings are written
/// in is a total order (AGENTS.md §4.6) — which matters because a full table
/// forgets its stalest row to make space, and *which* row that is has to be the
/// same everywhere. A scout's second look writes what its first did: the same
/// asset at the same place on the same tick.
pub(crate) fn record_sightings(world: &mut World) {
    let tick = world.tick().raw();
    let mut found = core::mem::take(world.sight_rows_mut());
    for row in 0..crate::math::index::row(world.units().len()) {
        let Some((sensor, home)) = world.survey_scout_sensor(row) else {
            continue;
        };
        if seat_alive(world, sensor.seat) {
            look_around(world, &sensor, tick, &mut found, true);
            explore(world, &sensor, home);
        }
    }
    for index in 0..crate::math::index::row(world.seats().len()) {
        let seat = world.seats().seats().get(index).copied().map(SeatId::new);
        let alive = world.seats().is_alive(index);
        let Some(seat) = seat.filter(|_| alive) else {
            continue;
        };
        // One walk over every sensor, whose order is seat-blind, keeping this
        // seat's.
        let mut at: usize = 0;
        while let Some((sensor, next)) = world.sensor_at(at) {
            at = next;
            if sensor.seat == seat {
                look_around(world, &sensor, tick, &mut found, false);
            }
        }
    }
    found.clear();
    *world.sight_rows_mut() = found;
    recheck(world, tick);
}

/// A Survey scout's re-check that found nothing: a due sighting of its
/// seat's (at least [`REFRESH_INTERVAL`] old) whose place the scout has in
/// sight, and which no sensor renewed this tick,
/// is a memory of an asset no longer there, and the seat forgets it. Scouts in
/// ascending unit row, sightings in table order, so the forgets are a total
/// order.
fn recheck(world: &mut World, tick: u32) {
    for row in 0..crate::math::index::row(world.units().len()) {
        let Some((sensor, _)) = world.survey_scout_sensor(row) else {
            continue;
        };
        if !seat_alive(world, sensor.seat) {
            continue;
        }
        let mut from: usize = 0;
        while let Some(slot) = gone_from(world, &sensor, tick, from) {
            // The next row moves into `slot`, so the walk goes on from it.
            world.forget_sighting(slot);
            from = slot;
        }
    }
}

/// The first sighting row at or after `from` that `sensor` disproves: its
/// seat's, due a re-check (at least [`REFRESH_INTERVAL`] old, so not renewed
/// on `tick`), and with its place in the sensor's sight.
fn gone_from(world: &World, sensor: &Sensor, tick: u32, from: usize) -> Option<usize> {
    let table = world.sightings();
    table
        .seats()
        .iter()
        .zip(table.places())
        .zip(table.seen_at())
        .enumerate()
        .skip(from)
        .find(|(_, ((who, place), seen))| {
            **who == sensor.seat.raw() && due_at(**seen, tick) && sensor.sees(&RadiusOnly, **place)
        })
        .map(|(slot, _)| slot)
}

/// Whether `seat` is a seat of the match still in it.
fn seat_alive(world: &World, seat: SeatId) -> bool {
    world
        .seats()
        .seats()
        .iter()
        .position(|held| *held == seat.raw())
        .is_some_and(|index| world.seats().is_alive(index))
}

/// Everything of another seat's that `sensor` sees becomes a sighting; and,
/// for a Survey scout's sensor (`report`), what it brought in is reported.
///
/// Only **another** seat's: a seat does not remember seeing its own things,
/// because it knows where they are. That is not an optimisation — it is what
/// keeps a sighting a memory of somebody else rather than a duplicate of the
/// unit table.
fn look_around(world: &mut World, sensor: &Sensor, tick: u32, found: &mut Vec<Row>, report: bool) {
    let seat = sensor.seat;
    let limit = Sq::of_radius(sensor.radius);
    world.units_within(sensor.centre, sensor.radius, found);
    for row in found.iter() {
        let units = world.units();
        let at = row.index();
        let (Some(owner), Some(hp), Some(kind), Some(place), Some(id)) = (
            units.seats().get(at).copied(),
            units.hit_points().get(at).copied(),
            units.kinds().get(at).copied(),
            units.positions().get(at).copied(),
            units.ids().get(at).copied(),
        ) else {
            continue;
        };
        if owner == seat.raw()
            || !SeatId::new(owner).is_some()
            || !hp.is_alive()
            || !sensor.sees(&RadiusOnly, place)
        {
            continue;
        }
        let kind = if kind == UnitKind::Commander.id() {
            AssetKind::Commander
        } else {
            AssetKind::Unit
        };
        if let Ok(id) = AssetId::of_unit(UnitId::new(id)) {
            let seen = Seen {
                seat,
                id,
                owner: SeatId::new(owner),
                kind,
                blueprint: NO_BLUEPRINT,
                place,
                tick,
            };
            note(world, report, seen);
        }
    }

    for at in 0..crate::math::index::row(world.beacons().len()) {
        let b = world.beacons();
        let (Some(owner), Some(hp), Some(place), Some(id)) = (
            b.seats().get(at).copied(),
            b.hit_points().get(at).copied(),
            b.positions().get(at).copied(),
            b.ids().get(at).copied(),
        ) else {
            continue;
        };
        if owner != seat.raw()
            && SeatId::new(owner).is_some()
            && hp.is_alive()
            && Sq::between(place, sensor.centre) <= limit
            && sensor.sees(&RadiusOnly, place)
            && let Ok(id) = AssetId::of_beacon(BeaconId::new(id))
        {
            let seen = Seen {
                seat,
                id,
                owner: SeatId::new(owner),
                kind: AssetKind::Beacon,
                blueprint: NO_BLUEPRINT,
                place,
                tick,
            };
            note(world, report, seen);
        }
    }

    for at in 0..crate::math::index::row(world.structures().len()) {
        let s = world.structures();
        let (Some(owner), Some(hp), Some(place), Some(id), Some(blueprint)) = (
            s.seats().get(at).copied(),
            s.hit_points().get(at).copied(),
            s.positions().get(at).copied(),
            s.ids().get(at).copied(),
            s.kinds().get(at).copied(),
        ) else {
            continue;
        };
        if owner != seat.raw()
            && SeatId::new(owner).is_some()
            && hp.is_alive()
            && Sq::between(place, sensor.centre) <= limit
            && sensor.sees(&RadiusOnly, place)
            && let Ok(id) = AssetId::of_structure(StructureId::new(id))
        {
            let seen = Seen {
                seat,
                id,
                owner: SeatId::new(owner),
                kind: AssetKind::Structure,
                blueprint,
                place,
                tick,
            };
            note(world, report, seen);
        }
    }
}

/// One sighting about to be written: the columns of [`World::see`].
#[derive(Clone, Copy, Debug)]
struct Seen {
    seat: SeatId,
    id: AssetId,
    owner: SeatId,
    kind: AssetKind,
    /// The structure's [`crate::tables::StructureKind::id`], or
    /// [`NO_BLUEPRINT`]: a seat that sees a structure sees what it is.
    blueprint: u8,
    place: [Fx; 3],
    tick: u32,
}

/// Record one sighting, and when a Survey scout took it (`report`), report
/// what it brought in: an enemy beacon the seat held no sighting of, or a sighting
/// that was due a re-check ([`REFRESH_INTERVAL`]).
fn note(world: &mut World, report: bool, seen: Seen) {
    let before = report.then(|| seen_at(world, seen.seat, seen.id));
    world.see(
        seen.seat,
        seen.id,
        seen.owner,
        seen.kind.id(),
        seen.blueprint,
        seen.place,
        seen.tick,
    );
    let line = match before {
        Some(None) if seen.kind == AssetKind::Beacon => Emission::of(EventKind::EnemyBeaconFound),
        Some(Some(then)) => {
            // E7: rule: a sighting is taken on a tick already played, so it is never later than now
            let age = seen.tick.saturating_sub(then);
            if !due_at(then, seen.tick) {
                return;
            }
            // E7: rule: `Ms::from_ticks` saturates at `i32::MAX` ms, past any reach window a rules table holds
            Emission::of(EventKind::SightingRefreshed).value(i64::from(Ms::from_ticks(age).raw()))
        }
        _ => return,
    };
    let now = world.tick();
    world.emit(
        now,
        line.seat(seen.seat)
            .subject(seen.id)
            .at(Position::from_array(seen.place)),
    );
}

/// The tick `seat` last saw `asset` on, or `None` when it remembers nothing of
/// it.
fn seen_at(world: &World, seat: SeatId, asset: AssetId) -> Option<u32> {
    let table = world.sightings();
    table
        .seats()
        .iter()
        .zip(table.assets())
        .zip(table.seen_at())
        .find(|((who, what), _)| **who == seat.raw() && **what == asset.raw())
        .map(|(_, seen)| *seen)
}

/// Mark the cell a Survey scout stands in as explored for its seat once it
/// stands on the cell's scan column ([`scan_column`], within the arrive
/// radius on the map, whatever the heights), the place it looks round the cell
/// from; and report `area_surveyed` when that mark took the last scannable
/// cell of its beacon's probe areas.
fn explore(world: &mut World, sensor: &Sensor, home: BeaconId) {
    let tick = world.tick().raw();
    let [x, y] = column_of(sensor.centre);
    let reach = world.arrive_radius().floor_voxels();
    let mut surveyed = false;
    // The cells within the arrive radius of where the scout stands: a scan
    // column on a cell's edge can be reached from the next cell over.
    for dy in [-reach, 0, reach] {
        for dx in [-reach, 0, reach] {
            let (Some(cx), Some(cy)) = (x.checked_add(dx), y.checked_add(dy)) else {
                continue;
            };
            let Some(cell) = cell_of([cx, cy]) else {
                continue;
            };
            if on_scan_column(world, sensor.centre, home, cell)
                && world.explored_mut().mark(sensor.seat, cell, tick)
                && in_probe_areas(world, home, cell)
            {
                surveyed = true;
            }
        }
    }
    if !surveyed
        || nearest_cell(
            world,
            sensor.seat,
            home,
            sensor.centre,
            Pick::Unexplored,
            &|_| true,
        )
        .is_some()
    {
        return;
    }
    let now = world.tick();
    world.emit(
        now,
        Emission::of(EventKind::AreaSurveyed)
            .seat(sensor.seat)
            .subject_of(AssetId::of_beacon(home)),
    );
}

/// Whether `here` stands on `cell`'s scan column: within the arrive radius
/// of it on the map, whatever the heights.
fn on_scan_column(world: &World, here: [Fx; 3], home: BeaconId, cell: u32) -> bool {
    let Some([x, y]) = scan_column(world, home, cell) else {
        return false;
    };
    // E7: cannot miss: a scan column is a column of the map, which `RulesTable::map_last_voxel` keeps inside `i16`
    let (Ok(x), Ok(y)) = (i16::try_from(x), i16::try_from(y)) else {
        return false;
    };
    let [_, _, z] = here;
    Sq::between(here, [Fx::from_voxels(x), Fx::from_voxels(y), z])
        <= Sq::of_radius(world.arrive_radius())
}

/// The column a scout of `home` scans `cell` from, and which marks it once it
/// stands there: the cell's centre column brought inside the first of
/// `home`'s probe areas it overlaps, in target-table order, or its own centre
/// column when it overlaps none. One column per cell, so the scan's
/// destination and the mark agree.
fn scan_column(world: &World, home: BeaconId, cell: u32) -> Option<[i32; 2]> {
    let (cx, cy) = unpack(cell)?;
    match probe_areas(world, home).find(|area| overlaps(*area, cx, cy)) {
        Some(area) => {
            let ([x0, y0], [x1, y1]) = bounds(area);
            Some([centre(cx, x0, x1)?, centre(cy, y0, y1)?])
        }
        None => Some([
            centre(cx, i32::MIN, i32::MAX)?,
            centre(cy, i32::MIN, i32::MAX)?,
        ]),
    }
}

/// A cell id's cell column and cell row.
fn unpack(cell: u32) -> Option<(i32, i32)> {
    let cx = i32::try_from(cell.checked_rem(CELL_ROW)?).ok()?;
    let cy = i32::try_from(cell.checked_div(CELL_ROW)?).ok()?;
    Some((cx, cy))
}

/// An area's lowest and highest columns, the lowest clipped at the map's
/// origin.
fn bounds(area: ColumnBox) -> ([i32; 2], [i32; 2]) {
    // E7: rule: a column west or south of the origin is on no map, so the area's part there holds no cell
    let low = area.min_column().map(|v| i32::from(v).max(0));
    (low, area.max_column().map(i32::from))
}

/// Whether `area` overlaps the cell at cell column `cx`, cell row `cy`.
fn overlaps(area: ColumnBox, cx: i32, cy: i32) -> bool {
    let ([x0, y0], [x1, y1]) = bounds(area);
    let side = |c: i32, lo: i32, hi: i32| {
        let first = c.checked_mul(CELL_VOXELS);
        let last = first.and_then(|first| first.checked_add(CELL_VOXELS.checked_sub(1)?));
        matches!((first, last), (Some(first), Some(last)) if first <= hi && last >= lo)
    };
    side(cx, x0, x1) && side(cy, y0, y1)
}

/// Whether `cell` overlaps one of `home`'s probe areas.
fn in_probe_areas(world: &World, home: BeaconId, cell: u32) -> bool {
    unpack(cell).is_some_and(|(cx, cy)| probe_areas(world, home).any(|area| overlaps(area, cx, cy)))
}

/// `home`'s probe areas, in target-table order.
fn probe_areas(world: &World, home: BeaconId) -> impl Iterator<Item = ColumnBox> + '_ {
    let targets = world.targets();
    targets
        .beacons()
        .iter()
        .zip(targets.kinds())
        .zip(targets.areas())
        .filter(move |((owner, kind), _)| **owner == home && **kind == TargetKind::Probe.id())
        .map(|(_, area)| *area)
}

/// Every cell `area` overlaps, in ascending id, each with its cell column and
/// row and the column a scout scans it from for this area: the cell's centre
/// column, brought inside the area. An area wholly west or south of the
/// origin overlaps no cell.
fn cells_of(area: ColumnBox) -> impl Iterator<Item = (u32, [i32; 2], [i32; 2])> {
    let ([x0, y0], [x1, y1]) = bounds(area);
    // `None` for an axis on which the area holds no column on the map's side
    // of the origin.
    let span = |lo: i32, hi: i32| {
        (hi >= 0)
            .then(|| Some(lo.checked_div(CELL_VOXELS)?..=hi.checked_div(CELL_VOXELS)?))
            .flatten()
    };
    let (rows, columns) = (span(y0, y1), span(x0, x1));
    rows.into_iter().flatten().flat_map(move |cy| {
        columns.clone().into_iter().flatten().filter_map(move |cx| {
            let cell = pack(cx, cy)?;
            Some((cell, [cx, cy], [centre(cx, x0, x1)?, centre(cy, y0, y1)?]))
        })
    })
}

/// The centre column of cell column (or row) `c`, brought inside `lo..=hi`,
/// or `None` past `i32`.
fn centre(c: i32, lo: i32, hi: i32) -> Option<i32> {
    let mid = c
        .checked_mul(CELL_VOXELS)?
        .checked_add(CELL_VOXELS.checked_div(2)?)?;
    // E7: rule: the scan column of a cell an area overlaps lies in the area, which is where its playbook said to look
    Some(mid.clamp(lo, hi))
}

/// Which cells a scan looks for (decision 7).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pick {
    /// Cells `seat` has never explored.
    Unexplored,
    /// Cells `seat` explored longer ago than reach memory
    /// (`verifier.reach_memory_ms`): due a re-scan ([`cell_due`]).
    Due,
}

/// Whether a cell explored on tick `explored` is due a re-scan on tick `now`
/// (S3's plan, decision 7): **older than reach memory**, the one clock a
/// seat's Attack targets are held to as well. The age is in whole ticks
/// against reach memory floored to ticks, so "older than N ms" is exact: an
/// age of `t` ticks is `t * 50` ms, which exceeds N exactly when `t` exceeds
/// N's whole ticks.
fn cell_due(world: &World, explored: u32, now: u32) -> bool {
    let window = world.rules().attack().reach_memory.to_ticks_floor();
    // E7: rule: a cell is explored on a tick already played, so never later than now
    now.saturating_sub(explored) > window
}

/// The nearest cell of `home`'s probe areas that `pick` admits for `seat` and a
/// scout at `here` can reach, with the point it walks to, ties to the lowest
/// cell; `open` refuses a point the caller will not send it to.
///
/// One selection and one order for both picks (S3's `pred`, the main
/// session's ruling on decision 7's tie-break): a due cell is chosen exactly
/// as an unexplored one is, by squared distance from the scout to the point
/// it would scan from, ties to the lowest cell id. Decision 7 adds no rule of
/// its own beyond which cells are candidates.
fn nearest_cell(
    world: &World,
    seat: SeatId,
    home: BeaconId,
    here: [Fx; 3],
    pick: Pick,
    open: &dyn Fn([Fx; 3]) -> bool,
) -> Option<(u32, [Fx; 3])> {
    let now = world.tick().raw();
    let surface = world.surface();
    let [hx, hy] = column_of(here);
    let start = surface.node_of(hx, hy)?;
    let mut best: Option<(Sq, u32, [Fx; 3])> = None;
    for (index, area) in probe_areas(world, home).enumerate() {
        for (cell, [cx, cy], [x, y]) in cells_of(area) {
            // A cell two areas overlap is scanned from the first's column
            // ([`scan_column`]), so it is a candidate of the first only.
            let candidate = match (pick, world.explored().explored_on(seat, cell)) {
                (Pick::Unexplored, explored) => explored.is_none(),
                (Pick::Due, Some(explored)) => cell_due(world, explored, now),
                (Pick::Due, None) => false,
            };
            if !candidate
                || probe_areas(world, home)
                    .take(index)
                    .any(|earlier| overlaps(earlier, cx, cy))
            {
                continue;
            }
            let Some(node) = surface.node_of(x, y) else {
                continue;
            };
            if !world.clusters().connected(surface, start, node) {
                continue;
            }
            let point = world.standing_point([x, y, 0]);
            let distance = Sq::between(here, point);
            if best.is_none_or(|(bd, bc, _)| (distance, cell) < (bd, bc)) && open(point) {
                best = Some((distance, cell, point));
            }
        }
    }
    best.map(|(_, cell, point)| (cell, point))
}

/// Every Survey scout's decision on a decision tick, seat by seat in
/// ascending seat id: [`crate::programs::run_all`] calls it before the units'
/// programs run, so a seat's scouts are decided together ([`decide_seat`]).
/// A Survey scout's own program then does nothing ([`is_survey_scout`]).
pub(crate) fn decide_scouts(world: &mut World) {
    if !world.is_decision_tick() {
        return;
    }
    for index in 0..crate::math::index::row(world.seats().len()) {
        let seat = world.seats().seats().get(index).copied().map(SeatId::new);
        let alive = world.seats().is_alive(index);
        if let Some(seat) = seat.filter(|_| alive) {
            // The Programs phase, outside any seat's decision: the scouts'
            // work is no decision's, so it is counted nowhere, as Attack's
            // breach search is not (decisions-log item 175 (4)).
            decide_seat(world, seat, Members::Survey, &());
        }
    }
}

/// Run one decision of a scout that is neither standing a picket nor a
/// Survey beacon's (a scout whose home is some other beacon): it is decided
/// alone, by the same rules ([`decide_seat`]). Called by
/// [`crate::programs::ScoutProgram`] on every tick; it decides on a decision
/// tick only, and between two the scout walks where it was sent.
pub(crate) fn run_scout(world: &mut World, unit: u32) {
    if !world.is_decision_tick() {
        return;
    }
    let row = crate::math::index::row(unit);
    let Some(seat) = world.units().seats().get(row).copied().map(SeatId::new) else {
        return;
    };
    decide_seat(world, seat, Members::One(row), &());
}

/// Whether the unit in `row` is a living scout whose home is a living
/// Survey beacon: one [`decide_scouts`] decides.
pub(crate) fn is_survey_scout(world: &World, row: usize) -> bool {
    let units = world.units();
    let (Some(kind), Some(hp), Some(home)) = (
        units.kinds().get(row).copied(),
        units.hit_points().get(row).copied(),
        units.homes().get(row).copied(),
    ) else {
        return false;
    };
    if kind != UnitKind::Scout.id() || !hp.is_alive() {
        return false;
    }
    let Ok(beacon) = world.beacons().row_of(home) else {
        return false;
    };
    let beacon = beacon.index();
    world.beacons().mandates().get(beacon).copied() == Some(MandateKind::Survey.id())
        && world
            .beacons()
            .hit_points()
            .get(beacon)
            .is_some_and(|hp| hp.is_alive())
}

/// The scouts one [`decide_seat`] decides.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Members {
    /// The seat's Survey scouts whose programs run this tick: home lit and
    /// not retreating. A scout whose retreat ends on this tick is decided on
    /// the next decision tick, one interval later, because the retreat is
    /// read here before the programs end it.
    Survey,
    /// One scout, in this row.
    One(usize),
}

/// Whether the unit in `row` is one of `members` of `seat`.
fn member(world: &World, seat: SeatId, members: Members, row: usize) -> bool {
    match members {
        Members::One(one) => one == row,
        Members::Survey => {
            let units = world.units();
            units.seats().get(row).copied() == Some(seat.raw())
                && is_survey_scout(world, row)
                && units.retreats().get(row).is_some_and(|to| !to.is_some())
                && units
                    .homes()
                    .get(row)
                    .copied()
                    .is_some_and(|home| crate::power::beacon_is_live(world, home))
        }
    }
}

/// A due sighting's rank: not an enemy beacon (beacons first), then the tick
/// it was seen on (oldest first), then its asset id. A total order: a seat
/// holds one sighting per asset.
type Rank = (bool, u32, u32);

/// One due sighting of a seat's, open to a scout.
#[derive(Clone, Copy, Debug)]
struct Due {
    /// Its row in the sighting table.
    slot: usize,
    rank: Rank,
    place: [Fx; 3],
    /// Whether it would be out of the reach window at the Lull with no
    /// re-check before it: its age and the ticks the Push has left are more
    /// than the window.
    needed: bool,
}

/// `seat`'s next due sighting after `after` in rank order, whose place lies
/// inside no known threat's range (decision 12).
fn next_due<M: crate::seams::Meter + ?Sized>(
    world: &World,
    seat: SeatId,
    after: Option<Rank>,
    threats: &[([Fx; 3], Sq)],
    meter: &M,
) -> Option<Due> {
    let tick = world.tick().raw();
    let left = ticks_left(world);
    let window = world.rules().attack().reach_memory.to_ticks_floor();
    let table = world.sightings();
    let rows = table
        .seats()
        .iter()
        .zip(table.assets())
        .zip(table.kinds())
        .zip(table.places())
        .zip(table.seen_at())
        .enumerate();
    let mut best: Option<Due> = None;
    for (slot, ((((who, asset), kind), place), seen)) in rows {
        meter.rows(1);
        let rank = (*kind != AssetKind::Beacon.id(), *seen, *asset);
        if *who != seat.raw()
            || !due_at(*seen, tick)
            || after.is_some_and(|after| rank <= after)
            || threats
                .iter()
                .any(|(at, reach)| Sq::between(*at, *place) <= *reach)
        {
            continue;
        }
        if best.is_none_or(|held| rank < held.rank) {
            // E7: rule: a sighting is taken on a tick already played, so it is never later than now
            let age = tick.saturating_sub(*seen);
            best = Some(Due {
                slot,
                rank,
                place: *place,
                // E7: rule: an age and a tick count of one match, so their sum is past any window long before `u32`
                needed: age.saturating_add(left) > window,
            });
        }
    }
    best
}

/// The due sighting in the sighting table's `slot`, if it is still one of
/// `seat`'s due sightings open to a scout.
fn due_in<M: crate::seams::Meter + ?Sized>(
    world: &World,
    seat: SeatId,
    slot: usize,
    threats: &[([Fx; 3], Sq)],
    meter: &M,
) -> Option<Due> {
    meter.rows(1);
    let table = world.sightings();
    let (Some(who), Some(asset), Some(kind), Some(place), Some(seen)) = (
        table.seats().get(slot).copied(),
        table.assets().get(slot).copied(),
        table.kinds().get(slot).copied(),
        table.places().get(slot).copied(),
        table.seen_at().get(slot).copied(),
    ) else {
        return None;
    };
    let tick = world.tick().raw();
    if who != seat.raw()
        || !due_at(seen, tick)
        || threats
            .iter()
            .any(|(at, reach)| Sq::between(*at, place) <= *reach)
    {
        return None;
    }
    // E7: rule: a sighting is taken on a tick already played, so it is never later than now
    let age = tick.saturating_sub(seen);
    let window = world.rules().attack().reach_memory.to_ticks_floor();
    Some(Due {
        slot,
        rank: (kind != AssetKind::Beacon.id(), seen, asset),
        place,
        // E7: rule: an age and a tick count of one match, so their sum is past any window long before `u32`
        needed: age.saturating_add(ticks_left(world)) > window,
    })
}

/// Whether a scout of `seat` that this decision does not decide is walking
/// to `place`: a sighting it already answers for.
fn walked_by_another<M: crate::seams::Meter + ?Sized>(
    world: &World,
    seat: SeatId,
    members: Members,
    place: [Fx; 3],
    meter: &M,
) -> bool {
    let units = world.units();
    units
        .kinds()
        .iter()
        .zip(units.seats())
        .zip(units.hit_points())
        .zip(units.destinations())
        .enumerate()
        .inspect(|_| meter.rows(1))
        .any(|(row, (((kind, owner), hp), to))| {
            *kind == UnitKind::Scout.id()
                && *owner == seat.raw()
                && hp.is_alive()
                && *to == place
                && !member(world, seat, members, row)
        })
}

/// The lists one decision builds, kept in the world's sight-row buffer for
/// the decision's length and re-derived on every decision, so no list is
/// state a later tick reads. For `n` due sightings in rank order, the buffer
/// holds `3n` rows: at `p` the sighting table row of the `p`th sighting; at
/// `n + p` the unit row of the scout whose list holds it; at `2n + p` row 1
/// when it is in a list and row 0 when not. A seat holds at most the sighting
/// table's per-seat share (64), so the buffer, reserved at the unit ceiling,
/// never grows.
struct Lists<'a> {
    rows: &'a mut Vec<crate::ids::Row>,
    n: usize,
}

impl Lists<'_> {
    /// The `p`th sighting's row in the sighting table.
    fn slot(&self, p: usize) -> Option<usize> {
        self.rows.get(p).map(|row| row.index())
    }

    /// The unit row of the scout whose list holds the `p`th sighting.
    fn owner(&self, p: usize) -> Option<usize> {
        let held = self.rows.get(p.checked_add(self.n.checked_mul(2)?)?)?;
        (held.index() == 1)
            .then(|| self.rows.get(p.checked_add(self.n)?).map(|row| row.index()))
            .flatten()
    }

    /// Put the `p`th sighting in the list of the scout in unit row `row`.
    fn give(&mut self, p: usize, row: usize) {
        let (Some(owner), Some(held)) = (crate::ids::Row::of_index(row), crate::ids::Row::new(1))
        else {
            return;
        };
        let n = self.n;
        if let Some(slot) = p.checked_add(n).and_then(|at| self.rows.get_mut(at)) {
            *slot = owner;
        }
        if let Some(slot) = n
            .checked_mul(2)
            .and_then(|twice| p.checked_add(twice))
            .and_then(|at| self.rows.get_mut(at))
        {
            *slot = held;
        }
    }
}

/// The scout in unit `row`'s list, in the order it walks it: the sighting it
/// is already walking to first (rule (a)), then the rest in rank order. Calls
/// `each` with every entry's position and due sighting, in that order.
fn walk_list<M: crate::seams::Meter + ?Sized>(
    world: &World,
    seat: SeatId,
    lists: &Lists<'_>,
    row: usize,
    threats: &[([Fx; 3], Sq)],
    meter: &M,
    mut each: impl FnMut(usize, Due),
) {
    let going = world.units().destinations().get(row).copied();
    let mut head = None;
    for p in 0..lists.n {
        meter.rows(1);
        if lists.owner(p) != Some(row) {
            continue;
        }
        let Some(due) = lists
            .slot(p)
            .and_then(|slot| due_in(world, seat, slot, threats, meter))
        else {
            continue;
        };
        if Some(due.place) == going {
            head = Some(p);
            each(p, due);
            break;
        }
    }
    for p in 0..lists.n {
        meter.rows(1);
        if lists.owner(p) != Some(row) || head == Some(p) {
            continue;
        }
        if let Some(due) = lists
            .slot(p)
            .and_then(|slot| due_in(world, seat, slot, threats, meter))
        {
            each(p, due);
        }
    }
}

/// The octile bound, in ticks at a scout's speed, of the walk from `from` to
/// `to`: no served walk is quicker.
fn bound_ticks(world: &World, from: [Fx; 3], to: [Fx; 3]) -> i64 {
    let [fx, fy] = column_of(from);
    let [tx, ty] = column_of(to);
    // Two columns of the map, inside `i16` (`RulesTable::map_last_voxel`):
    // their difference holds.
    let cost = world.surface().costs().octile(tx - fx, ty - fy);
    ticks_for_cost(cost, world.rules().cost_per_second(UnitKind::Scout)).map_or(i64::MAX, i64::from)
}

/// The scout in unit `row`'s list as built so far: the bound on the tick it
/// finishes it, counted from now, and the place it ends at (where it stands,
/// for an empty list).
fn list_end<M: crate::seams::Meter + ?Sized>(
    world: &World,
    seat: SeatId,
    lists: &Lists<'_>,
    row: usize,
    threats: &[([Fx; 3], Sq)],
    meter: &M,
) -> Option<(i64, [Fx; 3], u32)> {
    let mut at = world.units().positions().get(row).copied()?;
    let mut finish: i64 = 0;
    let mut legs: u32 = 0;
    walk_list(world, seat, lists, row, threats, meter, |_, due| {
        finish = finish.saturating_add(bound_ticks(world, at, due.place));
        at = due.place;
        legs = legs.saturating_add(1);
    });
    Some((finish, at, legs))
}

/// One seat's Survey scouts, decided together (spec section 6's Survey row:
/// "scan unexplored ground first, then re-check the sightings closest to
/// leaving the reach window, enemy beacons first ... refresh those sightings
/// before the next Lull"). Every scout is given an ordered **list** of the due
/// sightings it will visit, and leaves its scan when its list, started now,
/// needs the time the Push has left. The construction and its order, which is
/// total and read from hashed state only (re-derived on every decision, so no
/// list is state):
///
/// 1. **Rule (a), keep.** In ascending unit row, a scout already walking to a
///    due sighting no lower-row scout keeps has it at the head of its list.
/// 2. **Greedy insertion.** Every other due sighting of the seat, in rank
///    order ([`Rank`]: enemy beacons first, then oldest first, ties to the
///    lowest asset id), is appended to the list of the scout that would
///    finish its list soonest with it: the octile bound of its list's legs
///    from where it stands, then of the leg to the sighting; ties to the
///    lowest unit row. Only a scout the connectivity oracle connects to the
///    sighting counts; with none, the drop is counted
///    ([`Scratch::survey_unserved`](crate::pathing::search::Scratch)). A
///    sighting that would be out of the reach window at the Lull (it is
///    *needed*) and that no list reaches before the Lull even at the bound
///    is left out and counted
///    ([`Scratch::survey_out_of_reach`](crate::pathing::search::Scratch)),
///    never silently stale. This greedily balances the lists' finishing
///    times; it is not the optimum tour.
/// 3. **Departure.** In ascending unit row, a scout with a list walks to its
///    head when it is already walking there, when it has nothing to scan,
///    when the Push is in its last [`REFRESH_INTERVAL`], or when its list up
///    to its last needed sighting, the served legs in order plus the slack
///    ([`list_leaves_time`]), no longer leaves time to scan on. The served
///    legs are priced on a [`pricing_decision`], or on any decision once the
///    legs' octile bounds no longer leave time. Every other scout scans: the
///    nearest unexplored cell no lower-row scout of its beacon is walking to;
///    with none, a cell due a re-scan (decision 7); with none, it idles at
///    its beacon. "Scan first" stays the rule: only the scouts a list needs
///    leave the scan.
///
/// A scout that has arrived where it was sent parks, so the end of the route
/// does not carry it off before the next decision looks.
///
/// Every loop reports the counted work it does to `meter`
/// ([`crate::seams::Meter`]; decisions-log item 175 (1)): one unit a row it
/// reads at its innermost loops (a unit row, a sighting row, a list
/// position), the threat scan's rows through
/// [`crate::defend::for_each_known_threat_metered`], and the nodes each served
/// leg's search pops. A meter only hears counts: nothing this decides
/// changes with it. [`decide_scouts`] and [`run_scout`] pass `()`, because the
/// scouts are decided in the Programs phase, outside any seat's decision, as
/// Attack's breach search is (item 175 (4)).
///
/// Work per decision, at a seat's most due sightings `n` (64, the sighting
/// table's share) and its scouts `m`: the insertion evaluates `n × m` list
/// ends of at most `n` legs each, octile bounds only, `n² × m` in all; the
/// departure prices at most `n` served legs in all (each sighting is a leg of
/// one list), on a pricing decision or once a list's bound no longer leaves
/// time, and none otherwise.
fn decide_seat<M: crate::seams::Meter + ?Sized>(
    world: &mut World,
    seat: SeatId,
    members: Members,
    meter: &M,
) {
    let mut threats = world.take_threat_scratch();
    crate::defend::for_each_known_threat_metered(world, seat, meter, |known| {
        threats.push((known.at, Sq::of_radius(known.weapon.range())));
    });
    // A seat with no scout this decision decides holds no list: its
    // sightings wait for a scout, and none of them is a drop.
    if !(0..crate::math::index::row(world.units().len()))
        .inspect(|_| meter.rows(1))
        .any(|row| member(world, seat, members, row))
    {
        world.put_threat_scratch(threats);
        return;
    }
    let mut rows = core::mem::take(world.sight_rows_mut());
    rows.clear();
    // The seat's due sightings, in rank order, but for those a scout this
    // decision does not decide is walking to.
    let mut after = None;
    while let Some(due) = next_due(world, seat, after, &threats, meter) {
        after = Some(due.rank);
        if !walked_by_another(world, seat, members, due.place, meter)
            && let Some(slot) = crate::ids::Row::of_index(due.slot)
        {
            rows.push(slot);
        }
    }
    let n = rows.len();
    if let Some(len) = n.checked_mul(3) {
        rows.resize(len, crate::ids::Row::FIRST);
    }
    let mut lists = Lists { rows: &mut rows, n };
    keep(world, seat, members, &mut lists, &threats, meter);
    insert(world, seat, members, &mut lists, &threats, meter);
    depart_or_scan(world, seat, members, &lists, &threats, meter);
    rows.clear();
    *world.sight_rows_mut() = rows;
    world.put_threat_scratch(threats);
}

/// Step 1: in ascending unit row, every member walking to a due sighting no
/// lower-row member keeps has it at the head of its list.
fn keep<M: crate::seams::Meter + ?Sized>(
    world: &World,
    seat: SeatId,
    members: Members,
    lists: &mut Lists<'_>,
    threats: &[([Fx; 3], Sq)],
    meter: &M,
) {
    for row in 0..crate::math::index::row(world.units().len()) {
        meter.rows(1);
        if !member(world, seat, members, row) {
            continue;
        }
        let Some(to) = world.units().destinations().get(row).copied() else {
            continue;
        };
        let held = (0..lists.n).inspect(|_| meter.rows(1)).find(|p| {
            lists.owner(*p).is_none()
                && lists
                    .slot(*p)
                    .and_then(|slot| due_in(world, seat, slot, threats, meter))
                    .is_some_and(|due| due.place == to)
        });
        if let Some(p) = held {
            lists.give(p, row);
        }
    }
}

/// Step 2: every due sighting in no list, in rank order, appended to the list
/// of the member that finishes soonest with it.
fn insert<M: crate::seams::Meter + ?Sized>(
    world: &mut World,
    seat: SeatId,
    members: Members,
    lists: &mut Lists<'_>,
    threats: &[([Fx; 3], Sq)],
    meter: &M,
) {
    let left = i64::from(ticks_left(world));
    let link = i64::from(world.link_ticks());
    for p in 0..lists.n {
        meter.rows(1);
        if lists.owner(p).is_some() {
            continue;
        }
        let Some(due) = lists
            .slot(p)
            .and_then(|slot| due_in(world, seat, slot, threats, meter))
        else {
            continue;
        };
        let [gx, gy] = column_of(due.place);
        let goal = world.surface().node_of(gx, gy);
        let mut best: Option<(i64, usize, u32)> = None;
        for row in 0..crate::math::index::row(world.units().len()) {
            meter.rows(1);
            if !member(world, seat, members, row) {
                continue;
            }
            let Some((finish, at, legs)) = list_end(world, seat, lists, row, threats, meter) else {
                continue;
            };
            let [ax, ay] = column_of(at);
            let reachable =
                goal.zip(world.surface().node_of(ax, ay))
                    .is_some_and(|(goal, from)| {
                        world.clusters().connected(world.surface(), from, goal)
                    });
            if !reachable {
                continue;
            }
            let finish = finish.saturating_add(bound_ticks(world, at, due.place));
            if best.is_none_or(|(held, held_row, _)| (finish, row) < (held, held_row)) {
                best = Some((finish, row, legs.saturating_add(1)));
            }
        }
        let Some((finish, row, legs)) = best else {
            world.scratch_mut().survey_unserved.bump();
            continue;
        };
        // A needed sighting no list reaches before the Lull, even at the
        // bound and with a re-ask a leg, is out of reach this Push.
        if due.needed && finish.saturating_add(link.saturating_mul(i64::from(legs))) > left {
            world.scratch_mut().survey_out_of_reach.bump();
            continue;
        }
        lists.give(p, row);
    }
}

/// Step 3: in ascending unit row, every member with a list walks to its head
/// when it must, and every other member scans, re-scans or idles; a member
/// that arrived parks.
fn depart_or_scan<M: crate::seams::Meter + ?Sized>(
    world: &mut World,
    seat: SeatId,
    members: Members,
    lists: &Lists<'_>,
    threats: &[([Fx; 3], Sq)],
    meter: &M,
) {
    let last_minute = refresh_pass(world);
    for row in 0..crate::math::index::row(world.units().len()) {
        meter.rows(1);
        if !member(world, seat, members, row) {
            continue;
        }
        let units = world.units();
        let (Some(home), Some(here), Some(to), Ok(unit)) = (
            units.homes().get(row).copied(),
            units.positions().get(row).copied(),
            units.destinations().get(row).copied(),
            u32::try_from(row),
        ) else {
            continue;
        };
        let mut head: Option<[Fx; 3]> = None;
        walk_list(world, seat, lists, row, threats, meter, |_, due| {
            if head.is_none() {
                head = Some(due.place);
            }
        });
        let leaves = head.is_some_and(|head| {
            head == to
                || last_minute
                || !has_scan(world, seat, row, threats)
                || !list_leaves_time(world, seat, lists, row, threats, meter)
        });
        let target = if leaves {
            head
        } else {
            let open = |point: [Fx; 3]| {
                !threats
                    .iter()
                    .any(|(at, reach)| Sq::between(*at, point) <= *reach)
                    && !claimed(world, home, row, point)
            };
            nearest_cell(world, seat, home, here, Pick::Unexplored, &open)
                .or_else(|| nearest_cell(world, seat, home, here, Pick::Due, &open))
                .map(|(_, point)| point)
        };
        match target {
            // Arrived (on the map, whatever the heights): park, so the end of
            // the route's wandering does not carry the scout off before the
            // next decision looks.
            Some([tx, ty, _])
                if crate::programs::within(here, [tx, ty, here[2]], world.arrive_radius()) =>
            {
                world.park_unit(unit);
            }
            Some(to) => world.send_unit(unit, to),
            None => crate::programs::idle_at_home(world, unit),
        }
    }
}

/// Whether the member in unit `row` may scan on: its list, walked from where
/// it stands up to its last needed sighting, leaves time before the Lull.
/// The walk is the served legs in order ([`walk_ticks`] at the scout's
/// speed), plus the slack.
///
/// What holds, exactly. [`walk_ticks`] is never optimistic from the tick the
/// router serves each leg's route: the scout arrives no later than that,
/// plus one re-ask a leg for the wait before the router serves it
/// ([`World::link_ticks`]). The legs are priced on pricing decisions only,
/// every [`REPRICE_DECISIONS`] decision intervals, unless their octile bounds
/// already need the time; so a scout sets off up to that many intervals
/// after its list first needed the rest, and meanwhile it walks on with its
/// scan, which lengthens its first leg by at most the ticks it walked (each
/// step away is one step back), as many intervals again. The slack is twice
/// the pricing cadence and a re-ask a leg. Sight is further slack the rule
/// does not count on: a scout renews a sighting once the place is within its
/// vision radius, before it arrives. A list with no needed sighting always
/// leaves time: nothing in it would be out of the window at the Lull.
fn list_leaves_time<M: crate::seams::Meter + ?Sized>(
    world: &mut World,
    seat: SeatId,
    lists: &Lists<'_>,
    row: usize,
    threats: &[([Fx; 3], Sq)],
    meter: &M,
) -> bool {
    let Some(start) = world.units().positions().get(row).copied() else {
        return true;
    };
    // The legs up to the last needed sighting: its count, and their bound.
    let mut legs: u32 = 0;
    let mut needed: u32 = 0;
    let mut bound: i64 = 0;
    let mut bound_needed: i64 = 0;
    let mut at = start;
    walk_list(world, seat, lists, row, threats, meter, |_, due| {
        bound = bound.saturating_add(bound_ticks(world, at, due.place));
        at = due.place;
        legs = legs.saturating_add(1);
        if due.needed {
            needed = legs;
            bound_needed = bound;
        }
    });
    if needed == 0 {
        return true;
    }
    let left = i64::from(ticks_left(world));
    // A `u32` interval times a small cadence, doubled, plus a `u32` re-ask
    // times at most 64 legs: inside an `i64`.
    let slack = 2 * i64::from(world.decision_ticks()) * i64::from(REPRICE_DECISIONS)
        + i64::from(world.link_ticks()) * i64::from(needed);
    if bound_needed.saturating_add(slack) >= left {
        return false;
    }
    if !pricing_decision(world) {
        return true;
    }
    // Price the served legs, up to the last needed sighting.
    let speed = Speed {
        link_ticks: world.link_ticks(),
        ..Speed::of_kind(world.rules(), UnitKind::Scout)
    };
    let mut walked: i64 = 0;
    let mut from = start;
    let mut leg: u32 = 0;
    let mut broken = false;
    // The legs are walked in the list's order; each is priced from the
    // previous place. The list is read again for each leg (at most 64), so
    // the pricing needs no buffer.
    while leg < needed {
        let mut index: u32 = 0;
        let mut next: Option<[Fx; 3]> = None;
        walk_list(world, seat, lists, row, threats, meter, |_, due| {
            if index == leg {
                next = Some(due.place);
            }
            index = index.saturating_add(1);
        });
        let Some(to) = next else {
            break;
        };
        let [fx, fy] = column_of(from);
        let [tx, ty] = column_of(to);
        let (Some(a), Some(b)) = (
            world.surface().node_of(fx, fy),
            world.surface().node_of(tx, ty),
        ) else {
            broken = true;
            break;
        };
        let Some(ticks) = served_walk_ticks(world, a, b, speed, meter) else {
            broken = true;
            break;
        };
        walked = walked.saturating_add(ticks);
        from = to;
        leg = leg.saturating_add(1);
    }
    // A leg the router serves no walk for is counted where it was priced
    // (`served_walk_ticks`); the list cannot be trusted to leave time, so
    // the scout sets off for its head and the next decision builds the lists
    // again from where it then stands.
    !broken && walked.saturating_add(slack) < left
}

/// Whether the member in `row` has an unexplored cell to scan.
fn has_scan(world: &World, seat: SeatId, row: usize, threats: &[([Fx; 3], Sq)]) -> bool {
    let units = world.units();
    let (Some(home), Some(here)) = (
        units.homes().get(row).copied(),
        units.positions().get(row).copied(),
    ) else {
        return false;
    };
    let open = |point: [Fx; 3]| {
        !threats
            .iter()
            .any(|(at, reach)| Sq::between(*at, point) <= *reach)
            && !claimed(world, home, row, point)
    };
    nearest_cell(world, seat, home, here, Pick::Unexplored, &open).is_some()
}

/// The nodes `scratch`'s searches have popped since its counters were last
/// reset: the grid and endpoint pops (`expansions`, of which the endpoint
/// sweeps' are a part) and the abstract pops, as `crate::targeting` reads
/// them.
fn popped(scratch: &crate::pathing::search::Scratch) -> Result<u64, crate::seams::Unmeasured> {
    let unreadable = |_| crate::seams::Unmeasured::SourceUnreadable;
    scratch
        .expansions()
        .map_err(unreadable)?
        .checked_add(scratch.abstract_expansions().map_err(unreadable)?)
        .ok_or(crate::seams::Unmeasured::SourceUnreadable)
}

/// The ticks a scout takes, at the latest, on the walk the router serves from
/// `start` to `goal` at `speed` ([`walk_ticks`]), or `None` when it serves
/// none: `NoPath` (counted, [`Scratch::survey_unserved`](crate::pathing::search::Scratch)),
/// or a chain whose queries stop falling (counted,
/// [`Scratch::stalled`](crate::pathing::search::Scratch)), or a walker with no
/// speed. A walk too long for a tick count is [`i64::MAX`] ticks: longer than
/// any Push.
fn served_walk_ticks<M: crate::seams::Meter + ?Sized>(
    world: &mut World,
    start: Node,
    goal: Node,
    speed: Speed,
    meter: &M,
) -> Option<i64> {
    let (ground, scratch) = world.ground_and_scratch();
    // A served walk is charged the nodes its searches pop, the change in the
    // scratch's own counters over the call (decisions-log item 175 (1)).
    let before = popped(scratch);
    let walked = walk_cost(ground.surface, ground.clusters, scratch, (start, goal));
    meter.measured(popped(scratch).and_then(|after| {
        after
            .checked_sub(before?)
            .ok_or(crate::seams::Unmeasured::SourceUnreadable)
    }));
    let walk = match walked {
        Ok(walk) => walk,
        Err(WalkError::NoPath(_)) => {
            scratch.survey_unserved.bump();
            return None;
        }
        Err(WalkError::Stalled { .. }) => {
            scratch.stalled.bump();
            return None;
        }
    };
    match walk_ticks(walk, speed) {
        Ok(ticks) => ticks.map(i64::from),
        Err(EstimateError::PastCost { .. } | EstimateError::PastTicks { .. }) => Some(i64::MAX),
        // `walk_ticks` prices a served walk and never meets a chain; were it
        // to, the chain is one whose queries stopped falling, as above.
        Err(EstimateError::Stalled { .. }) => {
            scratch.stalled.bump();
            None
        }
    }
}

/// Whether this decision tick prices the served legs of the scouts' lists:
/// the Push's first decision, then every [`REPRICE_DECISIONS`]th. Between two,
/// a list is priced only when its legs' octile bounds no longer leave time, so
/// a scout far from its deadline is priced once a cadence rather than once a
/// decision, and one near it every decision.
fn pricing_decision(world: &World) -> bool {
    let elapsed = world.tick().since(world.match_state().segment_started());
    // E7: rule: a decision tick is past the Push's first tick, so `elapsed` is at least one
    let decision = elapsed.saturating_sub(1);
    // E7: cannot miss: `World::decision_ticks` is at least one and the cadence a positive constant
    decision
        .checked_div(world.decision_ticks())
        .and_then(|decisions| decisions.checked_rem(REPRICE_DECISIONS))
        .is_some_and(|phase| phase == 0)
}

/// Whether a scout of `home` in a row below `row` is already walking to
/// `point`.
fn claimed(world: &World, home: BeaconId, row: usize, point: [Fx; 3]) -> bool {
    let units = world.units();
    units
        .kinds()
        .iter()
        .zip(units.homes())
        .zip(units.hit_points())
        .zip(units.destinations())
        .take(row)
        .any(|(((kind, owner), hp), going)| {
            *kind == UnitKind::Scout.id() && *owner == home && hp.is_alive() && *going == point
        })
}

/// Whether the running Push is in its last [`REFRESH_INTERVAL`], the refresh
/// pass for every free scout ([`decide_seat`] sets one off sooner when its walk
/// needs it).
fn refresh_pass(world: &World) -> bool {
    ticks_left(world) <= REFRESH_INTERVAL.to_ticks_ceil()
}

/// The ticks the running Push has left to play.
fn ticks_left(world: &World) -> u32 {
    let state = world.match_state();
    // E7: rule: a segment's last tick is a tick the match plays, which `u32` holds by far (`Tick`)
    let end = state
        .segment_started()
        .raw()
        .saturating_add(state.segment_ticks());
    // E7: rule: zero once the segment's last tick has been played
    end.saturating_sub(world.tick().raw())
}

/// Whether a sighting taken on tick `seen` is due a re-check on tick `now`:
/// at least [`REFRESH_INTERVAL`] old.
fn due_at(seen: u32, now: u32) -> bool {
    // E7: rule: a sighting is taken on a tick already played, so it is never later than now
    now.saturating_sub(seen) >= REFRESH_INTERVAL.to_ticks_ceil()
}

#[cfg(test)]
mod tests {
    //! The scouts' decision reports its counted work to a [`Meter`]
    //! (decisions-log item 175 (1)): one unit a row it reads at its innermost
    //! loops, and the pops of each served leg it prices; and a measurement
    //! never reaches the state hash.
    //!
    //! [`Meter`]: crate::seams::Meter

    use super::{Members, decide_seat};
    use crate::knowledge::{AssetId, AssetKind};
    use crate::seams::{MandateKind, UnitTally};
    use crate::tables::{
        AreaKind, BeaconId, ColumnBox, NO_BLUEPRINT, PRIORITY_NORMAL, SeatId, UnitKind,
    };
    use crate::{MatchSettings, RulesTable, Runner, World, WorldConfig};

    const SEED: u64 = 0x00F1_2E00_C0DE_0001;

    /// A Push past its first minute: seat 0's Survey post at the map's
    /// centre with `scouts` scouts and a probe strip along the south edge, and
    /// `sightings` enemy beacon sightings seat 0 took on tick 1, each a
    /// hundred columns from the centre in turn, so every one is due.
    fn arranged(scouts: usize, sightings: usize) -> Runner {
        let path = crate::default_rules_path().unwrap_or_else(|| panic!("the rules table"));
        let rules =
            RulesTable::load(&path).unwrap_or_else(|error| panic!("the rules load: {error}"));
        let mut world = World::new(&WorldConfig {
            match_seed: SEED,
            seats: 2,
            rules,
            match_settings: MatchSettings {
                segment_lengths_ms: vec![600_000],
                round_limit: 1,
            },
        })
        .unwrap_or_else(|error| panic!("the world: {error}"));
        let map = world
            .rules()
            .message()
            .map
            .unwrap_or_else(|| panic!("a map block"));
        let (cx, cy) = (
            i32::try_from(map.size_x.div_euclid(2)).unwrap_or_else(|error| panic!("{error}")),
            i32::try_from(map.size_y.div_euclid(2)).unwrap_or_else(|error| panic!("{error}")),
        );
        let field = world.standing_point([cx, cy, 0]);
        let post = world
            .place_beacon_directly(SeatId::new(0), field, MandateKind::Survey, PRIORITY_NORMAL)
            .unwrap_or_else(|| panic!("a post"));
        world.set_scouts(post, 0);
        let last_x = i16::try_from(map.size_x - 1).unwrap_or_else(|error| panic!("{error}"));
        world
            .add_area(
                post,
                AreaKind::Probe,
                world.standing_point([cx, 8, 0]),
                ColumnBox::new([0, 0], [last_x, 40]).unwrap_or_else(|| panic!("a box")),
            )
            .unwrap_or_else(|error| panic!("the area fits: {error:?}"));
        for _ in 0..scouts {
            world
                .field_unit(SeatId::new(0), post, UnitKind::Scout, field)
                .unwrap_or_else(|| panic!("a scout"));
        }
        let mut runner = Runner::new(world).unwrap_or_else(|error| panic!("a runner: {error}"));
        assert_eq!(runner.begin_push(), Ok(true));
        for _ in 0..1_300 {
            step(&mut runner);
            runner.clear_events();
        }
        let offsets = [
            (-100, 0),
            (100, 0),
            (0, 100),
            (0, -100),
            (-100, 100),
            (100, -100),
        ];
        for (index, (dx, dy)) in offsets.iter().take(sightings).enumerate() {
            let at = runner.world().standing_point([cx + dx, cy + dy, 0]);
            let index = u32::try_from(index).unwrap_or_else(|error| panic!("{error}"));
            let id = AssetId::of_beacon(BeaconId::new(index + 900))
                .unwrap_or_else(|error| panic!("an id: {error:?}"));
            runner.world_mut().see(
                SeatId::new(0),
                id,
                SeatId::new(1),
                AssetKind::Beacon.id(),
                NO_BLUEPRINT,
                at,
                1,
            );
        }
        runner
    }

    /// Play one tick of the Push.
    fn step(runner: &mut Runner) {
        match runner.step() {
            Ok(Some(_)) => runner.clear_events(),
            Ok(None) => panic!("the Push ended"),
            Err(error) => panic!("a tick: {error}"),
        }
    }

    /// The units one decision of seat 0's scouts counts.
    fn units(scouts: usize, sightings: usize) -> u64 {
        let mut runner = arranged(scouts, sightings);
        let tally = UnitTally::new();
        decide_seat(runner.world_mut(), SeatId::new(0), Members::Survey, &tally);
        tally
            .read()
            .unwrap_or_else(|reason| panic!("measured: {reason:?}"))
    }

    #[test]
    fn the_scouts_decision_counts_more_for_more_sightings_and_scouts() {
        let base = units(1, 1);
        let more_sightings = units(1, 4);
        let more_scouts = units(3, 4);
        assert!(
            base > 0,
            "a decision with a scout and a sighting counts work"
        );
        assert!(
            more_sightings > base,
            "more sightings, more work: {base} then {more_sightings}"
        );
        assert!(
            more_scouts > more_sightings,
            "more scouts, more work: {more_sightings} then {more_scouts}"
        );
    }

    /// The same decision with a tally, with a faulted tally and with `()`
    /// leaves the world hashing the same, then and for the ticks after.
    #[test]
    fn the_scouts_measurement_never_reaches_the_state_hash() {
        let mut plain = arranged(2, 4);
        let mut counted = arranged(2, 4);
        let mut faulted = arranged(2, 4);
        let tally = UnitTally::new();
        let broken = UnitTally::new();
        broken.fault_next_charge();
        decide_seat(plain.world_mut(), SeatId::new(0), Members::Survey, &());
        decide_seat(counted.world_mut(), SeatId::new(0), Members::Survey, &tally);
        decide_seat(
            faulted.world_mut(),
            SeatId::new(0),
            Members::Survey,
            &broken,
        );
        assert!(
            tally.read().is_ok_and(|units| units > 0),
            "the tally counted"
        );
        assert!(broken.read().is_err(), "the faulted tally is unmeasured");
        for _ in 0..400 {
            let hash = plain.world().state_hash();
            assert_eq!(counted.world().state_hash(), hash);
            assert_eq!(faulted.world().state_hash(), hash);
            for runner in [&mut plain, &mut counted, &mut faulted] {
                step(runner);
                runner.clear_events();
            }
        }
    }
}
