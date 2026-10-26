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
//! * every **live beacon**'s sphere (`beacon.sphere_radius_voxels`), the
//!   passive vision every seat has from the first tick;
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
//!   brought inside its beacon's probe area when the cell is in one, the
//!   place it looks round the cell from.
//!   Only a Survey beacon's scouts mark: a Defend beacon's pickets warn,
//!   they do not survey ([`World::survey_scout_sensor`]).
//! * **The scan.** The scout walks to the nearest unexplored cell of its
//!   beacon's probe areas, ties to the lowest cell: to the cell's centre
//!   column brought inside the area, when that column is ground the scout can
//!   reach (the router's own connectivity oracle), so a scan never parks a
//!   scout in front of a column it cannot stand on.
//! * **The refresh pass.** Once nothing is left to scan, and in any case for
//!   the last [`REFRESH_INTERVAL`] of every Push, so that the playbook of the
//!   next Lull is written on fresh knowledge (decisions-log item 13; never cut,
//!   item 144 (11)), the scout walks to the sighting most due a re-check. A
//!   sighting is **due** from the moment it is [`REFRESH_INTERVAL`] old until
//!   it leaves the reach window (`verifier.reach_memory_ms`), and the due
//!   sightings rank **enemy beacons first**, then oldest first -- the one
//!   closest to leaving the window -- ties to the lowest asset id. A scout
//!   already walking to a due sighting keeps it, so a scout does not turn
//!   back and forth between two.
//! * **A re-check that finds nothing forgets.** A sighting whose place one of
//!   the seat's Survey scouts has in sight, and which no sensor renewed on
//!   that decision tick, remembers an asset that is no longer there, and the
//!   seat forgets it ([`World::forget_sighting`]; the sighting table is hashed
//!   and snapshotted whole, so this is hashed state like any sighting). Only
//!   a Survey scout's re-check forgets: a sphere or a picket that sees the
//!   place passively changes no memory.
//! * **Known threats** (decision 12, under item 148's seat sight): a cell or
//!   a sighting inside the weapon range of an armed enemy asset the seat sees
//!   now is no destination.
//! * **Several scouts** of one beacon share the work: a destination a scout of
//!   the same beacon in a lower row is already walking to is not taken again.
//! * **Reporting back** is three private feed lines: `area_surveyed` when a
//!   beacon's last scannable cell is marked, `enemy_beacon_found` for an enemy
//!   beacon its seat had never seen, and `sighting_refreshed` for a due
//!   sighting a Survey scout renewed.
//!
//! With nothing to scan and nothing due, the scout idles at its beacon.

use crate::events::{Emission, EventKind};
use crate::ids::Row;
use crate::knowledge::{AssetId, AssetKind, Position};
use crate::math::fixed::{Fx, Sq};
use crate::math::quantity::Ms;
use crate::sight::{RadiusOnly, Sensor};
use crate::tables::{BeaconId, ColumnBox, SeatId, StructureId, TargetKind, UnitId, UnitKind};
use crate::targeting::column_of;
use crate::world::World;

/// The explored table's coarse cell, in columns on a side.
// PLACEHOLDER: Survey's coarse cell, 16 columns square — tune2, at S2's demo
pub const CELL_VOXELS: i32 = 16;

/// Survey's refresh interval: a sighting this old is due a re-check, and a
/// Push's last this-long is the refresh pass (decisions-log item 13).
// PLACEHOLDER: Survey's refresh interval, 60 s of game time — tune2, at S2's demo
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

/// A Survey scout's re-check that found nothing: a sighting of its seat's
/// whose place the scout has in sight, and which no sensor renewed this tick,
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
/// seat's, not renewed on `tick`, and with its place in the sensor's sight.
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
            **who == sensor.seat.raw() && **seen < tick && sensor.sees(&RadiusOnly, **place)
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
                place,
                tick,
            };
            note(world, report, seen);
        }
    }

    for at in 0..crate::math::index::row(world.structures().len()) {
        let s = world.structures();
        let (Some(owner), Some(hp), Some(place), Some(id)) = (
            s.seats().get(at).copied(),
            s.hit_points().get(at).copied(),
            s.positions().get(at).copied(),
            s.ids().get(at).copied(),
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
    place: [Fx; 3],
    tick: u32,
}

/// Record one sighting, and when a Survey scout took it (`report`), report
/// what it brought in: an enemy beacon the seat had never seen, or a sighting
/// that was due a re-check ([`REFRESH_INTERVAL`]).
fn note(world: &mut World, report: bool, seen: Seen) {
    let before = report.then(|| seen_at(world, seen.seat, seen.id));
    world.see(
        seen.seat,
        seen.id,
        seen.owner,
        seen.kind.id(),
        seen.place,
        seen.tick,
    );
    let line = match before {
        Some(None) if seen.kind == AssetKind::Beacon => Emission::of(EventKind::EnemyBeaconFound),
        Some(Some(then)) => {
            // E7: rule: a sighting is taken on a tick already played, so it is never later than now
            let age = seen.tick.saturating_sub(then);
            if age < REFRESH_INTERVAL.to_ticks_ceil() {
                return;
            }
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
    let Some(cell) = cell_of(column_of(sensor.centre)) else {
        return;
    };
    let Some([x, y]) = scan_column(world, home, cell) else {
        return;
    };
    let (Ok(x), Ok(y)) = (i16::try_from(x), i16::try_from(y)) else {
        return;
    };
    let [_, _, z] = sensor.centre;
    let on = [Fx::from_voxels(x), Fx::from_voxels(y), z];
    if Sq::between(sensor.centre, on) > Sq::of_radius(world.arrive_radius())
        || !world.explored_mut().mark(sensor.seat, cell)
        || !in_probe_areas(world, home, cell)
        || nearest_cell(world, sensor.seat, home, sensor.centre, &|_| true).is_some()
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

/// The column a scout of `home` scans `cell` from, and which marks it once in
/// sight: the cell's centre column brought inside the first of `home`'s probe
/// areas it overlaps, in target-table order, or its own centre column when it
/// overlaps none. One column per cell, so the scan's destination and the
/// mark agree.
fn scan_column(world: &World, home: BeaconId, cell: u32) -> Option<[i32; 2]> {
    if let Some(column) = probe_areas(world, home)
        .find_map(|area| cells_of(area).find(|(held, _)| *held == cell))
        .map(|(_, column)| column)
    {
        return Some(column);
    }
    let cx = i32::try_from(cell.checked_rem(CELL_ROW)?).ok()?;
    let cy = i32::try_from(cell.checked_div(CELL_ROW)?).ok()?;
    Some([
        centre(cx, i32::MIN, i32::MAX)?,
        centre(cy, i32::MIN, i32::MAX)?,
    ])
}

/// Whether `cell` overlaps one of `home`'s probe areas.
fn in_probe_areas(world: &World, home: BeaconId, cell: u32) -> bool {
    probe_areas(world, home).any(|area| cells_of(area).any(|(held, _)| held == cell))
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

/// Every cell `area` overlaps, in ascending id, each with the column a scout
/// scans it from: the cell's centre column, brought inside the area. An area
/// wholly west or south of the origin overlaps no cell.
fn cells_of(area: ColumnBox) -> impl Iterator<Item = (u32, [i32; 2])> {
    let [x0, y0] = area.min_column().map(|v| i32::from(v).max(0));
    let [x1, y1] = area.max_column().map(i32::from);
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
            Some((cell, [centre(cx, x0, x1)?, centre(cy, y0, y1)?]))
        })
    })
}

/// The centre column of cell column (or row) `c`, brought inside `lo..=hi`,
/// or `None` past `i32`.
fn centre(c: i32, lo: i32, hi: i32) -> Option<i32> {
    let mid = c
        .checked_mul(CELL_VOXELS)?
        .checked_add(CELL_VOXELS.checked_div(2)?)?;
    Some(mid.clamp(lo, hi))
}

/// The nearest cell of `home`'s probe areas that `seat` has not explored and a
/// scout at `here` can reach, with the point it walks to, ties to the lowest
/// cell; `open` refuses a point the caller will not send it to.
fn nearest_cell(
    world: &World,
    seat: SeatId,
    home: BeaconId,
    here: [Fx; 3],
    open: &dyn Fn([Fx; 3]) -> bool,
) -> Option<(u32, [Fx; 3])> {
    let surface = world.surface();
    let [hx, hy] = column_of(here);
    let start = surface.node_of(hx, hy)?;
    let mut best: Option<(Sq, u32, [Fx; 3])> = None;
    for area in probe_areas(world, home) {
        for (cell, [x, y]) in cells_of(area) {
            // A cell two areas overlap is scanned from the first's column.
            if world.explored().contains(seat, cell)
                || scan_column(world, home, cell) != Some([x, y])
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

/// Run one decision of a scout that is not standing a picket (spec section
/// 6's Survey row): scan, or refresh, or idle at its beacon. Called by
/// [`crate::programs::ScoutProgram`] on every tick; it decides on a decision
/// tick only, and between two the scout walks where it was sent.
pub(crate) fn run_scout(world: &mut World, unit: u32) {
    if !world.is_decision_tick() {
        return;
    }
    let row = crate::math::index::row(unit);
    let units = world.units();
    let (Some(seat), Some(home), Some(here), Some(going)) = (
        units.seats().get(row).copied().map(SeatId::new),
        units.homes().get(row).copied(),
        units.positions().get(row).copied(),
        units.destinations().get(row).copied(),
    ) else {
        return;
    };
    let mut threats = world.take_threat_scratch();
    crate::defend::for_each_known_threat(world, seat, |known| {
        threats.push((known.at, Sq::of_radius(known.weapon.range())));
    });
    let target = {
        let open = |point: [Fx; 3]| {
            !threats
                .iter()
                .any(|(at, reach)| Sq::between(*at, point) <= *reach)
                && !claimed(world, home, row, point)
        };
        let scan = nearest_cell(world, seat, home, here, &open).map(|(_, point)| point);
        if scan.is_some() && !refresh_pass(world) {
            scan
        } else {
            most_due(world, seat, going, &open).or(scan)
        }
    };
    world.put_threat_scratch(threats);
    match target {
        Some(to) => world.send_unit(unit, to),
        None => crate::programs::idle_at_home(world, unit),
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

/// Whether the running Push is in its refresh pass: its last
/// [`REFRESH_INTERVAL`].
fn refresh_pass(world: &World) -> bool {
    let state = world.match_state();
    // E7: rule: a segment's last tick is a tick the match plays, which `u32` holds by far (`Tick`)
    let end = state
        .segment_started()
        .raw()
        .saturating_add(state.segment_ticks());
    // E7: rule: zero once the segment's last tick has been played
    end.saturating_sub(world.tick().raw()) <= REFRESH_INTERVAL.to_ticks_ceil()
}

/// Where a scout in the refresh pass walks: to `seat`'s sighting most due a
/// re-check, keeping `going` while it is still one of them, or `None` when
/// none is due.
fn most_due(
    world: &World,
    seat: SeatId,
    going: [Fx; 3],
    open: &dyn Fn([Fx; 3]) -> bool,
) -> Option<[Fx; 3]> {
    // E7: rule: the interpreter refuses a playbook whose rules carry no `verifier` block (`PlanError::MissingBlock`), so no Survey beacon stands without one; with none, no sighting is inside a window
    let window = world
        .rules()
        .message()
        .verifier
        .as_ref()
        .map(|block| Ms::new(block.reach_memory_ms).to_ticks_floor())?;
    let due = REFRESH_INTERVAL.to_ticks_ceil();
    let tick = world.tick().raw();
    let table = world.sightings();
    let rows = table
        .seats()
        .iter()
        .zip(table.assets())
        .zip(table.kinds())
        .zip(table.places())
        .zip(table.seen_at());
    let mut best: Option<((bool, u32, u32), [Fx; 3])> = None;
    for ((((who, asset), kind), place), seen) in rows {
        // E7: rule: a sighting is taken on a tick already played, so it is never later than now
        let age = tick.saturating_sub(*seen);
        if *who != seat.raw() || age < due || age > window || !open(*place) {
            continue;
        }
        if *place == going {
            return Some(going);
        }
        let key = (*kind != AssetKind::Beacon.id(), *seen, *asset);
        if best.is_none_or(|(held, _)| key < held) {
            best = Some((key, *place));
        }
    }
    best.map(|(_, place)| place)
}
