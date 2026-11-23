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
//!   item 13; never cut, item 144 (11); decisions-log, the item recording
//!   2026-11-22's Survey ruling: the sentence binds the sightings a seat
//!   already holds). A sighting is **due** once it is [`REFRESH_INTERVAL`]
//!   old, and stays due for as long as the seat holds it, past the reach
//!   window (`verifier.reach_memory_ms`) as much as inside it: a Push can be
//!   longer than the window, so a sighting the scan kept the scouts from has
//!   left the window by the time they turn to it, and the re-check is what
//!   brings it back. The due sightings rank **enemy beacons first**, then
//!   oldest first -- the one closest to leaving the window, or longest out of
//!   it -- ties to the lowest asset id. A seat's scouts are decided
//!   **together** on every decision tick ([`decide_seat`] states the
//!   algorithm and its order): a scout already walking to a due sighting
//!   keeps it; each other sighting goes to the free scout whose served walk
//!   reaches it soonest; and that scout leaves its scan only when it must --
//!   it has nothing to scan, or the Push is in its last
//!   [`REFRESH_INTERVAL`], or the sighting would be out of the window at the
//!   Lull and the walk the router serves to it, at the scout's speed, at the
//!   latest, no longer leaves time to scan on ([`walk_leaves_time`]), priced
//!   once every [`REPRICE_DECISIONS`] decisions while its octile bound leaves
//!   time and every decision once it does not. Every other scout scans.
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
//! * **Several scouts** share the work: a sighting goes to one scout of the
//!   seat (the refresh pass above), and a cell a scout of the same beacon in
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

/// How many decision intervals apart a free scout's walk to a sighting it may
/// have to set off for is priced ([`pricing_decision`]): every eighth, two
/// seconds of game time at `match.decision_tick_ms` 250. The departure rule's
/// slack grows with it ([`walk_leaves_time`]), so a longer cadence prices
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
            decide_seat(world, seat, Members::Survey);
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
    decide_seat(world, seat, Members::One(row));
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

/// One due sighting of a seat's, open to a scout: its rank, place and age.
#[derive(Clone, Copy, Debug)]
struct Due {
    rank: Rank,
    place: [Fx; 3],
    age: u32,
}

/// `seat`'s next due sighting of tier `beacons` after `after` in rank order,
/// whose place lies inside no known threat's range (decision 12).
fn next_due(
    world: &World,
    seat: SeatId,
    beacons: bool,
    after: Option<Rank>,
    threats: &[([Fx; 3], Sq)],
) -> Option<Due> {
    let tick = world.tick().raw();
    let table = world.sightings();
    let rows = table
        .seats()
        .iter()
        .zip(table.assets())
        .zip(table.kinds())
        .zip(table.places())
        .zip(table.seen_at());
    let mut best: Option<Due> = None;
    for ((((who, asset), kind), place), seen) in rows {
        let rank = (*kind != AssetKind::Beacon.id(), *seen, *asset);
        if *who != seat.raw()
            || rank.0 == beacons
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
                rank,
                place: *place,
                age,
            });
        }
    }
    best
}

/// Whether a scout of `seat` already goes to `place` on this decision: one
/// in `going` (kept or sent), or one this decision does not decide.
fn taken(
    world: &World,
    seat: SeatId,
    members: Members,
    going: &[crate::ids::Row],
    place: [Fx; 3],
) -> bool {
    let units = world.units();
    units
        .kinds()
        .iter()
        .zip(units.seats())
        .zip(units.hit_points())
        .zip(units.destinations())
        .enumerate()
        .any(|(row, (((kind, owner), hp), to))| {
            *kind == UnitKind::Scout.id()
                && *owner == seat.raw()
                && hp.is_alive()
                && *to == place
                && (going.iter().any(|held| held.index() == row)
                    || !member(world, seat, members, row))
        })
}

/// One seat's Survey scouts, decided together (spec section 6's Survey row:
/// "scan unexplored ground first, then re-check the sightings closest to
/// leaving the reach window, enemy beacons first ... refresh those sightings
/// before the next Lull"). The algorithm, and its order, which is total:
///
/// 1. **The due sightings are assigned to the scouts**, enemy beacons first
///    (the whole beacon tier, then the rest), each tier in rank order
///    ([`Rank`]: oldest first, ties to the lowest asset id). In each tier:
///    * **a scout already walking to a due sighting keeps it** (the scouts in
///      ascending unit row; two walking to one, the lower row keeps it);
///    * then each sighting no scout keeps, in rank order, goes to **the free
///      scout whose served walk reaches it soonest** (ties to the lowest unit
///      row; [`soonest`]), and that scout sets off for it **only if it must**:
///      it has nothing to scan, or the Push is in its last
///      [`REFRESH_INTERVAL`], or the sighting would be out of the reach
///      window at the Lull and no free scout's walk to it leaves time to scan
///      on ([`walk_leaves_time`]). A sighting no scout must set off for takes
///      none, and the tier stops after as many sightings as it had free
///      scouts, so the scouts answer for the most due first. A scout walking
///      to a lower tier's sighting is free for an enemy beacon.
/// 2. **Every other scout scans**, in ascending unit row: the nearest
///    unexplored cell no lower-row scout of its beacon is walking to; with
///    none, a cell due a re-scan (decision 7); with none, it idles at its
///    beacon. "Scan first" stays the rule: only the scouts a refresh needs
///    leave the scan.
///
/// A scout that has arrived where it was sent parks, so the end of the
/// route does not carry it off before the next decision looks.
fn decide_seat(world: &mut World, seat: SeatId, members: Members) {
    let mut threats = world.take_threat_scratch();
    crate::defend::for_each_known_threat(world, seat, |known| {
        threats.push((known.at, Sq::of_radius(known.weapon.range())));
    });
    let mut going = core::mem::take(world.sight_rows_mut());
    going.clear();
    for beacons in [true, false] {
        keep(world, seat, members, beacons, &threats, &mut going);
        assign(world, seat, members, beacons, &threats, &mut going);
    }
    scan_or_idle(world, seat, members, &threats, &going);
    going.clear();
    *world.sight_rows_mut() = going;
    world.put_threat_scratch(threats);
}

/// Step 1's first half for one tier: every member walking to a due sighting
/// of the tier that no lower-row scout keeps, keeps it.
fn keep(
    world: &World,
    seat: SeatId,
    members: Members,
    beacons: bool,
    threats: &[([Fx; 3], Sq)],
    going: &mut Vec<crate::ids::Row>,
) {
    for row in 0..crate::math::index::row(world.units().len()) {
        if !member(world, seat, members, row) || going.iter().any(|held| held.index() == row) {
            continue;
        }
        let Some(to) = world.units().destinations().get(row).copied() else {
            continue;
        };
        let mut after = None;
        while let Some(due) = next_due(world, seat, beacons, after, threats) {
            after = Some(due.rank);
            if due.place == to {
                if !taken(world, seat, members, going, to)
                    && let Some(held) = crate::ids::Row::of_index(row)
                {
                    going.push(held);
                }
                break;
            }
        }
    }
}

/// Step 1's second half for one tier: the sightings no scout keeps, in rank
/// order, each to the free member that reaches it soonest when one must set
/// off now; at most as many sightings as the tier had free members.
fn assign(
    world: &mut World,
    seat: SeatId,
    members: Members,
    beacons: bool,
    threats: &[([Fx; 3], Sq)],
    going: &mut Vec<crate::ids::Row>,
) {
    let free = (0..crate::math::index::row(world.units().len()))
        .filter(|row| {
            member(world, seat, members, *row) && !going.iter().any(|held| held.index() == *row)
        })
        .count();
    let left = ticks_left(world);
    let window = world.rules().attack().reach_memory.to_ticks_floor();
    let last_minute = refresh_pass(world);
    let mut considered: usize = 0;
    let mut after = None;
    while considered < free {
        let Some(due) = next_due(world, seat, beacons, after, threats) else {
            return;
        };
        after = Some(due.rank);
        if taken(world, seat, members, going, due.place) {
            continue;
        }
        considered = considered.saturating_add(1);
        // A member with nothing to scan, the nearest of them, sets off: it
        // has nothing else to do.
        let idle = soonest(world, seat, members, going, due.place, Want::Idle, threats);
        let chosen = match idle {
            Soonest::Scout(row) => Some(row),
            Soonest::Time | Soonest::None if last_minute => {
                match soonest(world, seat, members, going, due.place, Want::Any, threats) {
                    Soonest::Scout(row) => Some(row),
                    Soonest::Time | Soonest::None => None,
                }
            }
            // E7: rule: an age and a tick count of one match, so their sum is past any window long before `u32`
            Soonest::Time | Soonest::None if due.age.saturating_add(left) > window => {
                match soonest(
                    world,
                    seat,
                    members,
                    going,
                    due.place,
                    Want::Pressed {
                        left,
                        pricing: pricing_decision(world),
                    },
                    threats,
                ) {
                    Soonest::Scout(row) => Some(row),
                    Soonest::Time | Soonest::None => None,
                }
            }
            // Fresh at the Lull without a re-check now: the last minute's
            // pass re-checks it, and nobody leaves the scan for it yet.
            Soonest::Time | Soonest::None => None,
        };
        if let Some(row) = chosen
            && let (Some(held), Ok(unit)) = (crate::ids::Row::of_index(row), u32::try_from(row))
        {
            world.send_unit(unit, due.place);
            going.push(held);
        }
    }
}

/// Which free members [`soonest`] weighs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Want {
    /// Only those with nothing to scan.
    Idle,
    /// Every free member.
    Any,
    /// Every free member, and the one reaching the place soonest only when
    /// none's walk leaves time to scan on with `left` ticks of the Push to
    /// play ([`walk_leaves_time`]). Off a [`pricing_decision`], a member whose
    /// octile bound leaves time is taken to have time without its walk being
    /// priced: the next pricing decision prices it.
    Pressed {
        /// The ticks the Push has left.
        left: u32,
        /// Whether this decision prices the walks ([`pricing_decision`]).
        pricing: bool,
    },
}

/// What [`soonest`] found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Soonest {
    /// The member, by unit row, whose served walk reaches the place soonest.
    Scout(usize),
    /// Under [`Want::Pressed`], a member whose walk leaves time: nobody must
    /// set off yet.
    Time,
    /// No member it weighs can walk to the place.
    None,
}

/// The free member of `seat`'s (not in `going`) whose served walk reaches
/// `place` soonest, ties to the lowest unit row ([`Want`] says which members
/// count). The walks are priced in the order of their octile bounds, which no
/// served walk undercuts, and the search stops once the next bound is no
/// sooner than the best walk priced, so a member a bound already rules out
/// is never priced.
fn soonest(
    world: &mut World,
    seat: SeatId,
    members: Members,
    going: &[crate::ids::Row],
    place: [Fx; 3],
    want: Want,
    threats: &[([Fx; 3], Sq)],
) -> Soonest {
    let speed = Speed {
        link_ticks: world.link_ticks(),
        ..Speed::of_kind(world.rules(), UnitKind::Scout)
    };
    let [tx, ty] = column_of(place);
    let Some(goal) = world.surface().node_of(tx, ty) else {
        // A place off the surface graph: the router serves no walk there.
        world.scratch_mut().survey_unserved.bump();
        return Soonest::None;
    };
    let mut best: Option<(i64, usize)> = None;
    let mut cursor: Option<(i64, usize)> = None;
    loop {
        // The next candidate in (bound, row) order.
        let mut next: Option<(i64, usize, Node)> = None;
        for row in 0..crate::math::index::row(world.units().len()) {
            if !member(world, seat, members, row) || going.iter().any(|held| held.index() == row) {
                continue;
            }
            let Some(here) = world.units().positions().get(row).copied() else {
                continue;
            };
            let [fx, fy] = column_of(here);
            let Some(start) = world.surface().node_of(fx, fy) else {
                continue;
            };
            // Two columns of the map, inside `i16` (`RulesTable::map_last_voxel`):
            // their difference holds.
            let bound = world.surface().costs().octile(tx - fx, ty - fy);
            let Some(bound) = ticks_for_cost(bound, speed.cost_per_second).map(i64::from) else {
                continue;
            };
            let key = (bound, row);
            if cursor.is_some_and(|cursor| key <= cursor)
                || next.is_some_and(|(b, r, _)| key >= (b, r))
            {
                continue;
            }
            if want == Want::Idle && has_scan(world, seat, row, threats) {
                continue;
            }
            next = Some((bound, row, start));
        }
        let Some((bound, row, start)) = next else {
            break;
        };
        cursor = Some((bound, row));
        if best.is_some_and(|(walk, _)| bound > walk) {
            break;
        }
        if let Want::Pressed {
            left,
            pricing: false,
        } = want
            && walk_leaves_time(world, bound, left)
        {
            return Soonest::Time;
        }
        let Some(walk) = served_walk_ticks(world, start, goal, speed) else {
            continue;
        };
        if let Want::Pressed { left, .. } = want
            && walk_leaves_time(world, walk, left)
        {
            return Soonest::Time;
        }
        if best.is_none_or(|held| (walk, row) < held) {
            best = Some((walk, row));
        }
    }
    best.map_or(Soonest::None, |(_, row)| Soonest::Scout(row))
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

/// The ticks a scout takes, at the latest, on the walk the router serves from
/// `start` to `goal` at `speed` ([`walk_ticks`]), or `None` when it serves
/// none: `NoPath` (counted, [`Scratch::survey_unserved`](crate::pathing::search::Scratch)),
/// or a chain whose queries stop falling (counted,
/// [`Scratch::stalled`](crate::pathing::search::Scratch)), or a walker with no
/// speed. A walk too long for a tick count is [`i64::MAX`] ticks: longer than
/// any Push.
fn served_walk_ticks(world: &mut World, start: Node, goal: Node, speed: Speed) -> Option<i64> {
    let (ground, scratch) = world.ground_and_scratch();
    let walk = match walk_cost(ground.surface, ground.clusters, scratch, (start, goal)) {
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

/// Whether a walk of `walk` ticks, set off at the next [`pricing_decision`],
/// still arrives before the Lull with `left` ticks of the Push to play: the
/// walk plus the slack is less than `left`.
///
/// What holds, exactly. [`walk_ticks`] is never optimistic from the tick the
/// router serves the route: the scout arrives no later than that, plus the
/// wait for its first route, one re-ask ([`World::link_ticks`]). A free
/// scout's walk is priced on pricing decisions only, every
/// [`REPRICE_DECISIONS`] decision intervals, so it sets off up to that many
/// intervals after the walk first needed the rest; and meanwhile it walks on
/// with its scan, which lengthens its walk by at most the ticks it walked
/// (each step away is one step back), as many intervals again. The slack is
/// twice the pricing cadence and the re-ask. Sight is further slack the rule
/// does not count on: the scout renews the sighting once the place is within
/// its vision radius, before it arrives.
fn walk_leaves_time(world: &World, walk: i64, left: u32) -> bool {
    // A `u32` interval times a small cadence, doubled, plus a `u32`: inside an
    // `i64`.
    let slack = 2 * i64::from(world.decision_ticks()) * i64::from(REPRICE_DECISIONS)
        + i64::from(world.link_ticks());
    walk.saturating_add(slack) < i64::from(left)
}

/// Whether this decision tick prices free scouts' walks to the sightings
/// they may have to set off for: the Push's first decision, then every
/// [`REPRICE_DECISIONS`]th. Between two, a walk is priced only when its octile
/// bound no longer leaves time, so a scout far from its deadline is priced
/// once a cadence rather than once a decision, and one near it every
/// decision.
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

/// Step 2: every member that does not go to a sighting scans, re-scans or
/// idles, in ascending unit row; and every member that arrived parks.
fn scan_or_idle(
    world: &mut World,
    seat: SeatId,
    members: Members,
    threats: &[([Fx; 3], Sq)],
    going: &[crate::ids::Row],
) {
    for row in 0..crate::math::index::row(world.units().len()) {
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
        let target = if going.iter().any(|held| held.index() == row) {
            Some(to)
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
