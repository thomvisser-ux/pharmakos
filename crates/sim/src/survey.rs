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
//! * **The refresh pass.** The sightings a seat holds are re-checked before
//!   the next Lull, so that its playbook is written on fresh knowledge
//!   (decisions-log item 13; never cut, item 144 (11); the sentence binds the
//!   sightings a seat already holds, decisions-log item 173 (1), and the
//!   refresh of an enemy core wins over the scouts it costs, item 173 (2),
//!   which the main session reads as covering the sightings inside a core's
//!   gun range as it covers the core's own). A sighting is **due** once it is
//!   [`REFRESH_INTERVAL`] old, and stays due for as long as the seat holds
//!   it, past the reach window (`verifier.reach_memory_ms`) as much as inside
//!   it; it is **needed** when it would be out of the window at the Lull with
//!   no re-check before it. A seat's scouts are decided **together** on every
//!   decision tick ([`decide_seat`] states the construction, its order and
//!   its work):
//!   - every due or needed sighting, needed first, then enemy beacons, then
//!     oldest, goes into one scout's **list**, the ordered sightings it will
//!     visit;
//!   - a scout already walking to one keeps it at its head;
//!   - each other goes at the end of the list it lengthens least, or, where
//!     no end keeps every needed sighting fitting, into the list and
//!     position it lengthens least (cheapest insertion at the octile bound);
//!   - ties go to the **later** position, then the lowest unit row, so a
//!     sighting that costs the same either way is visited in rank order: the
//!     earlier position would break S2's ruled acceptance tests
//!     `the_stalest_sighting_is_refreshed_next` and `enemy_beacons_come_first`
//!     (`tests/survey.rs`);
//!   - a list never spans more than the reach window from its first needed
//!     refresh to its last (with the departure's slack and a re-ask a leg),
//!     or its first refresh would be stale again by the Lull;
//!   - sightings under one remembered gun's range are split across the
//!     seat's scouts before they are chained on one, so a scout killed
//!     refreshing a core does not take the next sighting under that gun with
//!     it. The seat takes **every enemy beacon for a core** and so for armed
//!     (`beacon.core_weapon`), though a forward beacon is unarmed
//!     (`crate::world::combat`): the gateway does not show a seat which of
//!     another seat's beacons is its core, so the seat cannot tell. The cost:
//!     sightings near a forward beacon are split across scouts when one
//!     scout could have taken them in turn, and a death near one counts as a
//!     death under a gun when a drop is named.
//!
//!   A scout leaves its scan only when it must: its head is due and it has
//!   nothing to scan or the Push is in its last [`REFRESH_INTERVAL`], or its
//!   list no longer leaves time to scan on ([`list_leaves_time`]: the legs in
//!   order up to the last needed sighting). On a [`pricing_decision`], one in
//!   [`REPRICE_DECISIONS`], that check prices the served legs; on any other
//!   it reads the octile bounds alone, and a scout whose bounds leave no time
//!   sets off at once, unpriced. A needed sighting no list takes is counted
//!   under the [`Drop`] reason that names its cause, once a Push, never
//!   silently stale. Every other scout scans.
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
//!   A scout walking to a cell's scan column when its seat sees a gun over
//!   that column has looked at the cell: the cell is marked on that tick as a
//!   scan marks it ([`seen_under_a_gun`]), so it is due again only once the
//!   mark is older than reach memory, and until then it draws no scout back.
//!   And a scout keeps the cell it set off for while that cell stays open
//!   ([`scan_walk`]). Both hold a scout's walk steady although which cells
//!   are open moves with what its seat sees on the tick.
//! * **Several scouts** share the work: a sighting is in one scout's list (the
//!   refresh pass above), and a cell a scout of the same beacon in
//!   a lower row is already walking to is not scanned again.
//! * **Arrival.** A scout that has reached the place it was sent to parks
//!   there, so the wandering a unit at the end of its route is given
//!   ([`World`]'s arrival draw) does not carry it off before it looks.
//! * **Reporting back** is three private feed lines: `area_surveyed` when a
//!   scout's mark takes its beacon's last scannable cell (a mark from the
//!   cell's scan column, or the mark of a cell seen to lie under a gun),
//!   `enemy_beacon_found`
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
use crate::pathing::estimate::{PastTicks, WalkError, ticks_of, walk_cost_in};
use crate::pathing::surface::Node;
use crate::pathing::{Speed, ticks_for_cost};
use crate::reserved::Reserved;
use crate::seams::MandateKind;
use crate::sight::{RadiusOnly, Sensor};
use crate::tables::{
    BeaconId, ColumnBox, NO_BLUEPRINT, SeatId, StructureId, TargetKind, UnitId, UnitKind,
};
use crate::targeting::column_of;
use crate::world::World;

/// How many decision intervals apart the served legs of a scout's list are
/// priced ([`pricing_decision`]): every eighth, two seconds of game time at
/// `match.decision_tick_ms` 250; between two, the departure reads the octile
/// bounds alone. The departure rule's slack grows with it
/// ([`list_leaves_time`]), so a longer cadence prices fewer walks and sets a
/// scout off up to that much sooner.
// PLACEHOLDER: Survey's re-pricing cadence, every 8th decision (2 s) — owner, at S3's demo (item 152 (1) 2), with `tune3`'s balance re-run
pub const REPRICE_DECISIONS: u32 = 8;

/// The explored table's coarse cell, in columns on a side.
///
/// A scout standing on a cell's centre sees all of it while its vision radius
/// is at least the cell's half-diagonal, about 11.3 columns here: true at
/// the scout's vision of 28 (24 from the rules table's revision 6, `tune2`,
/// to its revision 8, `tune3`); at the 8 it held before, a cell's corners
/// could be marked unseen.
// PLACEHOLDER: Survey's coarse cell, 16 columns square, which a scout of vision 28 sees whole from its centre — owner, at S3's demo (item 152 (1) 2), with `tune3`'s balance re-run
pub const CELL_VOXELS: i32 = 16;

/// Survey's refresh interval: a sighting this old is due a re-check, and
/// stays due for as long as the seat holds it (decisions-log item 13). Which
/// due sighting a scout sets off for, and when, is the seat's lists'
/// ([`decide_seat`]): a sighting that would leave the reach window before
/// the Lull with no re-check is needed first, and a scout with a longer walk
/// sets off sooner ([`list_leaves_time`]).
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
    if surveyed {
        report_surveyed(world, sensor.seat, home, sensor.centre);
    }
}

/// Report `area_surveyed` for `home` when a mark just made was the last: no
/// cell of its probe areas that a scout at `here` can reach is left
/// unexplored for `seat`.
fn report_surveyed(world: &mut World, seat: SeatId, home: BeaconId, here: [Fx; 3]) {
    if nearest_cell(world, seat, home, here, Pick::Unexplored, &|_| true).is_some() {
        return;
    }
    let now = world.tick();
    world.emit(
        now,
        Emission::of(EventKind::AreaSurveyed)
            .seat(seat)
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

/// The scan a scout of `home` standing at `here` is already walking to
/// (`to`), as its cell and the point it scans from, while it is still one
/// [`nearest_cell`] could pick for it, whatever is open: the scan column of a
/// cell of `home`'s probe areas that `seat` has not explored, or explored
/// longer ago than reach memory, which the scout can reach.
///
/// A scout keeps it while it is open, and picks the nearest cell again only
/// once it has none to keep ([`depart_or_scan`]). Which cells are open moves
/// with what the seat sees on the tick (a known threat is what a seat sees
/// this tick, not what it remembers: decisions-log item 149 (4) (g), and item
/// 173 (2) kept it so), and a scout that picked afresh on every decision
/// turned on the edge of its own sight for ever: toward a cell under a gun
/// while it did not see the gun, away to the next cell once it did, and back
/// as soon as the gun left its sight again. Rules revision 8 showed it, with
/// a scout that sights a core from outside the core's gun: on the golden seed
/// a scout walked the same two columns from the tick it found seat 1's core
/// to the end of the match. Kept, every walk a scout sets off on ends on a
/// cell it marks, from its scan column or as [`seen_under_a_gun`], unless
/// another scout claims the cell or its seat's list calls it away.
fn scan_walk(
    world: &World,
    seat: SeatId,
    home: BeaconId,
    here: [Fx; 3],
    to: [Fx; 3],
) -> Option<(u32, [Fx; 3])> {
    let cell = cell_of(column_of(to))?;
    if !in_probe_areas(world, home, cell) {
        return None;
    }
    let [x, y] = scan_column(world, home, cell)?;
    let point = world.standing_point([x, y, 0]);
    if point != to {
        return None;
    }
    let now = world.tick().raw();
    let wanted = world
        .explored()
        .explored_on(seat, cell)
        .is_none_or(|explored| cell_due(world, explored, now));
    let surface = world.surface();
    let [hx, hy] = column_of(here);
    let reachable = surface
        .node_of(hx, hy)
        .zip(surface.node_of(x, y))
        .is_some_and(|(start, node)| world.clusters().connected(surface, start, node));
    (wanted && reachable).then_some((cell, point))
}

/// Mark `cell` looked at for `seat` on this tick, by a scout of `home` at
/// `here` that was walking to the cell's scan column when its seat saw a gun
/// over that column; and report `area_surveyed` when the mark took the last
/// unexplored cell of `home`'s probe areas, as [`explore`] does.
///
/// The seat has learnt what the scan was for: an armed enemy asset stands
/// over the cell. Marked, the cell is due a look again once the mark is older
/// than reach memory (decision 7), and the scout that comes for it then looks
/// from the edge of its sight again. Unmarked, the cell stayed unexplored for
/// ever and was open again on every tick the seat did not see the gun, so it
/// drew a scout back as often as the scout had nothing nearer: with other
/// cells to scan, a walk to the edge of the gun after each of them; with
/// none, a turn for home and a turn back, on the spot, for the rest of the
/// match. No cell is opened or closed by this: a cell under a seen gun is no
/// destination (decision 12) before and after it.
fn seen_under_a_gun(world: &mut World, seat: SeatId, home: BeaconId, here: [Fx; 3], cell: u32) {
    let tick = world.tick().raw();
    if world.explored_mut().mark(seat, cell, tick) {
        report_surveyed(world, seat, home, here);
    }
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

/// The most due sightings one decision weighs for a seat: the sighting
/// table's per-seat share ([`crate::world::SIGHTINGS_PER_SEAT`]), so a seat's
/// lists fit arrays on the stack and the decision allocates nothing.
const LIST_ROOM: usize = crate::math::index::row(crate::world::SIGHTINGS_PER_SEAT);

/// A due sighting's rank: **needed** first (it would be out of the reach
/// window at the Lull with no re-check before it; fixed within a Push, since
/// a sighting's age and the ticks the Push has left move together), then not
/// an enemy beacon (beacons first), then the tick it was seen on (oldest
/// first), then its asset id. A total order: a seat holds one sighting per
/// asset.
type Rank = (bool, bool, u32, u32);

/// One due sighting of a seat's, open to a scout.
#[derive(Clone, Copy, Debug)]
struct Due {
    rank: Rank,
    place: [Fx; 3],
    /// Its asset id.
    asset: u32,
    /// Whether it would be out of the reach window at the Lull with no
    /// re-check before it: its age and the ticks the Push has left are more
    /// than the window.
    needed: bool,
    /// Whether it is due a re-check now (ripe): at least [`REFRESH_INTERVAL`] old.
    ripe: bool,
    /// The asset id of the first remembered armed enemy asset (in sighting
    /// table order) whose weapon range holds its place, if any
    /// ([`gun_over`]).
    armed: Option<u32>,
}

/// The weapon range of what the sighting in `slot` remembers, if it is armed
/// as far as its seat can know: an enemy beacon has the core's gun
/// (`beacon.core_weapon`; a seat does not know which beacon is a core, so it
/// takes every beacon for one), and an enemy structure its blueprint's weapon
/// row. Units move and are not counted here.
fn armed_reach(world: &World, slot: usize) -> Option<Sq> {
    let table = world.sightings();
    let kind = table.kinds().get(slot).copied()?;
    if kind == AssetKind::Beacon.id() {
        return Some(Sq::of_radius(world.rules().core_weapon().range()));
    }
    if kind == AssetKind::Structure.id() {
        let blueprint = table.blueprints().get(slot).copied()?;
        let row = crate::tables::StructureKind::from_id(blueprint)
            .and_then(|kind| world.rules().structure(kind))?;
        return row.weapon.map(|weapon| Sq::of_radius(weapon.range()));
    }
    None
}

/// The first armed sighting of `seat`'s, in table order, whose range holds
/// `place`: its asset id, its place and its range squared.
fn gun_over<M: crate::seams::Meter + ?Sized>(
    world: &World,
    seat: SeatId,
    place: [Fx; 3],
    meter: &M,
) -> Option<(u32, [Fx; 3], Sq)> {
    let table = world.sightings();
    (0..table.seats().len()).find_map(|slot| {
        meter.rows(1);
        if table.seats().get(slot).copied() != Some(seat.raw()) {
            return None;
        }
        let at = table.places().get(slot).copied()?;
        let reach = armed_reach(world, slot)?;
        (Sq::between(at, place) <= reach)
            .then(|| {
                table
                    .assets()
                    .get(slot)
                    .copied()
                    .map(|asset| (asset, at, reach))
            })
            .flatten()
    })
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
    // E7: rule: a sighting is taken on a tick already played, so it is never later than now
    let age = tick.saturating_sub(seen);
    let window = world.rules().attack().reach_memory.to_ticks_floor();
    // E7: rule: an age and a tick count of one match, so their sum is past any window long before `u32`
    let needed = age.saturating_add(ticks_left(world)) > window;
    let due = due_at(seen, tick);
    // A sighting the lists hold: one due a re-check now, or one that needs a
    // re-check before the Lull though it is not due yet, so a list's walk
    // counts it before it falls due rather than after, when the time to
    // walk it may be gone.
    if who != seat.raw()
        || !(due || needed)
        || threats
            .iter()
            .any(|(at, reach)| Sq::between(*at, place) <= *reach)
    {
        return None;
    }
    Some(Due {
        rank: (!needed, kind != AssetKind::Beacon.id(), seen, asset),
        place,
        asset,
        needed,
        ripe: due,
        // Read once the sighting is listed ([`decide_seat`]), not on every
        // rank walk.
        armed: None,
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

/// One due sighting in one decision's lists.
#[derive(Clone, Copy, Debug)]
struct Entry {
    due: Due,
    /// The unit row of the scout whose list holds it.
    owner: Option<usize>,
    /// The entry after it in its scout's list.
    next: Option<usize>,
    /// Whether it heads its scout's list because the scout is already
    /// walking to it (rule (a)).
    kept: bool,
    /// Whether it is the first entry of its scout's list.
    first: bool,
}

/// One decision's lists: every due sighting of the seat in rank order, each
/// in at most one scout's list, the lists threaded through `next`. Built on
/// the stack and re-derived on every decision, so no list is state a later
/// tick reads.
struct Lists {
    entries: [Option<Entry>; LIST_ROOM],
    n: usize,
}

impl Lists {
    fn get(&self, p: usize) -> Option<Entry> {
        self.entries.get(p).copied().flatten()
    }

    fn set(&mut self, p: usize, entry: Entry) {
        if let Some(slot) = self.entries.get_mut(p) {
            *slot = Some(entry);
        }
    }

    /// The first entry of the scout in unit `row`'s list.
    fn head<M: crate::seams::Meter + ?Sized>(&self, row: usize, meter: &M) -> Option<usize> {
        (0..self.n).find(|p| {
            meter.rows(1);
            self.get(*p)
                .is_some_and(|entry| entry.owner == Some(row) && entry.first)
        })
    }

    /// The scout in unit `row`'s list in its order, into `out`; how many.
    fn sequence<M: crate::seams::Meter + ?Sized>(
        &self,
        row: usize,
        out: &mut [usize; LIST_ROOM],
        meter: &M,
    ) -> usize {
        let mut len: usize = 0;
        let mut at = self.head(row, meter);
        while let Some(p) = at {
            meter.rows(1);
            let Some(slot) = out.get_mut(len) else {
                break;
            };
            *slot = p;
            len = len.saturating_add(1);
            at = self.get(p).and_then(|entry| entry.next);
        }
        len
    }

    /// Put entry `p` into the scout in unit `row`'s list after `after` (or at
    /// its front, for `None`).
    fn insert<M: crate::seams::Meter + ?Sized>(
        &mut self,
        p: usize,
        row: usize,
        after: Option<usize>,
        meter: &M,
    ) {
        let next = match after {
            Some(q) => self.get(q).and_then(|entry| entry.next),
            None => self.head(row, meter),
        };
        match after {
            Some(q) => {
                if let Some(mut prev) = self.get(q) {
                    prev.next = Some(p);
                    self.set(q, prev);
                }
            }
            // At the front: the old head is first no more.
            None => {
                if let Some(q) = next
                    && let Some(mut old) = self.get(q)
                {
                    old.first = false;
                    self.set(q, old);
                }
            }
        }
        if let Some(mut entry) = self.get(p) {
            entry.owner = Some(row);
            entry.next = next;
            entry.first = after.is_none();
            self.set(p, entry);
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

/// Why a needed sighting is in no list on this decision, named by its cause
/// and tested in this order ([`drop_threatened`], then [`drop_reason`]; each
/// counted once a Push for every seat, asset and reason,
/// [`Scratch::survey_drop`](crate::pathing::search::Scratch)).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Drop {
    /// A threat the seat sees now covers its place, so it is no destination
    /// (decision 12) for any scout, whatever happened before: the threat is
    /// what keeps every list from it on this decision.
    UnderThreat,
    /// It was listed, and fitted, at an earlier decision of this Push, and a
    /// scout of the seat has died since: the one whose list held it, or one
    /// where the sighting's remembered gun reaches or within a scout's vision
    /// of it, or any while the seat is short of the scouts its beacons field:
    /// the death took the time to reach it. Tested before reach, so a miss that follows a death is never
    /// put down to distance.
    ScoutLost,
    /// It was never listed and fitting this Push, and no scout of the seat,
    /// setting off at once for it alone, reaches it before the Lull, even at
    /// the octile bound.
    OutOfReach,
    /// Otherwise: a scout alone would reach it, or it was listed earlier with
    /// no death since, but no list takes it without a needed sighting already
    /// in it ceasing to fit: the construction's miss.
    CrowdedOut,
}

/// A [`Memory`] with no room left for what it was asked to keep.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Full;

/// The most Survey drops one Push logs: three seats, the sighting table's
/// share of 64 each, four reasons.
const DROP_ROOM: usize = 3 * 64 * 4;

/// The most listed sightings, scouts and scout deaths one Push remembers:
/// three seats, the sighting table's share of 64 each.
const SEAT_ROOM: usize = 3 * 64;

/// What naming a Survey drop's reason remembers of the running Push, and the
/// drop log: a **measurement**, kept in the search scratch
/// ([`Scratch::survey`](crate::pathing::search::Scratch)), never hashed,
/// never snapshotted and empty after a resume, so a drop's reason names its
/// cause only from the resume on. It is read only to name a drop's reason,
/// and the reasons only by tests: nothing a scout does reads it, so the
/// match plays the same with it full, empty or past its room.
///
/// It keeps, for the Push that began on its `push` tick:
///
/// * every sighting listed and fitting at a decision, with the last decision
///   tick it was ([`note_listed`]);
/// * every scout a decision decided, while it lives, and for each seat the
///   last decision tick one of them was found dead ([`note_scouts`]);
/// * the drops, once each for every seat, asset and reason, with the last
///   decision tick each was dropped on.
///
/// Every list is reserved when the scratch is made, so a decision allocates
/// nothing; past the room, the scratch counts the miss
/// (`Scratch::survey_drops_unlogged`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Memory {
    /// The first tick of the Push this memory is for.
    push: Option<u32>,
    /// (seat, asset, reason, the last decision tick it was dropped on).
    log: Reserved<(u8, u32, Drop, u32)>,
    /// (seat, asset, the last decision tick it was listed and fitting, the
    /// unit id of the scout whose list held it then).
    listed: Reserved<(u8, u32, u32, u32)>,
    /// (seat, unit id, where it stood at the last decision) of every living
    /// scout a decision decided.
    scouts: Reserved<(u8, u32, [Fx; 3])>,
    /// (seat, the decision tick a scout of it was found dead on, where it
    /// stood at the decision before, its unit id).
    deaths: Reserved<(u8, u32, [Fx; 3], u32)>,
}

impl Memory {
    /// An empty memory with its room reserved.
    pub(crate) fn new() -> Self {
        Self {
            push: None,
            log: Reserved::with_capacity(DROP_ROOM),
            listed: Reserved::with_capacity(SEAT_ROOM),
            scouts: Reserved::with_capacity(SEAT_ROOM),
            deaths: Reserved::with_capacity(SEAT_ROOM),
        }
    }

    /// Forget everything.
    pub(crate) fn clear(&mut self) {
        self.push = None;
        self.log.clear();
        self.listed.clear();
        self.scouts.clear();
        self.deaths.clear();
    }

    /// Keep the Push that began on tick `push`: a new Push forgets the last
    /// one's, so a Push with no drop reads an empty log.
    pub(crate) fn begin(&mut self, push: u32) {
        if self.push != Some(push) {
            self.clear();
            self.push = Some(push);
        }
    }

    /// The running Push's drops.
    pub(crate) fn log(&self) -> &[(u8, u32, Drop, u32)] {
        &self.log
    }

    /// Log a drop on decision tick `tick`: `Ok(true)` the first time this
    /// Push for its seat, asset and reason, `Ok(false)` after (its tick
    /// moves on).
    pub(crate) fn log_drop(
        &mut self,
        seat: u8,
        asset: u32,
        reason: Drop,
        tick: u32,
    ) -> Result<bool, Full> {
        if let Some(held) = self
            .log
            .iter_mut()
            .find(|(who, what, why, _)| (*who, *what, *why) == (seat, asset, reason))
        {
            held.3 = tick;
            return Ok(false);
        }
        if self.log.len() >= DROP_ROOM {
            return Err(Full);
        }
        self.log.push((seat, asset, reason, tick));
        Ok(true)
    }

    /// `seat`'s sighting of `asset` is listed and fitting on decision tick
    /// `tick`, in the list of the scout `owner` (its unit id).
    fn listed(&mut self, seat: u8, asset: u32, tick: u32, owner: u32) -> Result<(), Full> {
        if let Some(held) = self
            .listed
            .iter_mut()
            .find(|(who, what, _, _)| (*who, *what) == (seat, asset))
        {
            held.2 = tick;
            held.3 = owner;
            return Ok(());
        }
        if self.listed.len() >= SEAT_ROOM {
            return Err(Full);
        }
        self.listed.push((seat, asset, tick, owner));
        Ok(())
    }

    /// The last decision tick of this Push `seat`'s sighting of `asset` was
    /// listed and fitting on, and the unit id of the scout whose list held it.
    fn listed_at(&self, seat: u8, asset: u32) -> Option<(u32, u32)> {
        self.listed
            .iter()
            .find(|(who, what, _, _)| (*who, *what) == (seat, asset))
            .map(|(_, _, at, owner)| (*at, *owner))
    }

    /// Whether a scout of `seat` was found dead on a decision after tick
    /// `tick`, its place and unit id such that `which` holds.
    fn died_after(&self, seat: u8, tick: u32, which: impl Fn([Fx; 3], u32) -> bool) -> bool {
        self.deaths
            .iter()
            .any(|(who, when, place, unit)| *who == seat && *when > tick && which(*place, *unit))
    }

    /// Keep `unit`, a scout of `seat`'s a decision decides, standing at
    /// `place`.
    fn scout(&mut self, seat: u8, unit: u32, place: [Fx; 3]) -> Result<(), Full> {
        if let Some(held) = self
            .scouts
            .iter_mut()
            .find(|(who, what, _)| (*who, *what) == (seat, unit))
        {
            held.2 = place;
            return Ok(());
        }
        if self.scouts.len() >= SEAT_ROOM {
            return Err(Full);
        }
        self.scouts.push((seat, unit, place));
        Ok(())
    }

    /// `unit`, a scout of `seat`'s, was found dead on decision tick `tick`.
    fn died(&mut self, seat: u8, unit: u32, tick: u32) -> Result<(), Full> {
        let place = self
            .scouts
            .iter()
            .find(|(who, what, _)| (*who, *what) == (seat, unit))
            .map(|(_, _, place)| *place);
        self.scouts
            .retain(|(who, what, _)| (*who, *what) != (seat, unit));
        let Some(place) = place else {
            return Ok(());
        };
        if self.deaths.len() >= SEAT_ROOM {
            return Err(Full);
        }
        self.deaths.push((seat, tick, place, unit));
        Ok(())
    }
}

/// The reason a needed sighting of `seat`'s, `asset` at `place`, is dropped
/// on this decision, where no threat the seat sees covers it
/// ([`drop_threatened`] names those): a scout lost when it was listed and
/// fitting earlier this Push and a scout of the seat has died since: the
/// scout whose list held it then, or one that died where the sighting's
/// remembered gun reaches or within a scout's vision of the sighting, or any
/// scout while the seat is short of the scouts its beacons field; out of
/// reach when it was never listed this Push and no scout alone reaches it
/// (`alone_reaches` false); crowded out otherwise.
#[allow(
    clippy::too_many_arguments,
    reason = "the drop's seat, members and sighting and the reach its decision already holds, read at the one place a drop is named"
)]
fn drop_reason<M: crate::seams::Meter + ?Sized>(
    world: &mut World,
    seat: SeatId,
    members: Members,
    asset: u32,
    place: [Fx; 3],
    alone_reaches: bool,
    meter: &M,
) -> Drop {
    let gun = gun_over(world, seat, place, meter);
    let sight = Sq::of_radius(Fx::from_voxels(
        world.rules().unit(UnitKind::Scout).vision_radius_voxels,
    ));
    let short = short_of_scouts(world, seat, members, meter);
    let memory = &world.scratch_mut().survey;
    let listed = memory.listed_at(seat.raw(), asset);
    let lost = listed.is_some_and(|(at, owner)| {
        memory.died_after(seat.raw(), at, |died, unit| {
            unit == owner
                || short
                || Sq::between(place, died) <= sight
                || gun.is_some_and(|(_, gun, reach)| Sq::between(gun, died) <= reach)
        })
    });
    if lost {
        Drop::ScoutLost
    } else if listed.is_none() && !alone_reaches {
        Drop::OutOfReach
    } else {
        Drop::CrowdedOut
    }
}

/// Whether the seat has fewer living Survey scouts deciding than its Survey
/// beacons field: a scout lost, its replacement not yet out.
fn short_of_scouts<M: crate::seams::Meter + ?Sized>(
    world: &World,
    seat: SeatId,
    members: Members,
    meter: &M,
) -> bool {
    let beacons = world.beacons();
    let fielded: u32 = (0..beacons.ids().len())
        .filter(|row| {
            meter.rows(1);
            beacons.seats().get(*row).copied() == Some(seat.raw())
                && beacons.mandates().get(*row).copied() == Some(MandateKind::Survey.id())
                && beacons
                    .hit_points()
                    .get(*row)
                    .is_some_and(|hp| hp.is_alive())
        })
        .filter_map(|row| beacons.scout_counts().get(row).copied().map(u32::from))
        .fold(0, u32::saturating_add);
    let live = (0..crate::math::index::row(world.units().len()))
        .filter(|row| {
            meter.rows(1);
            member(world, seat, members, *row)
        })
        .count();
    u32::try_from(live).is_ok_and(|live| live < fielded)
}

/// Log a drop of `seat`'s sighting of `asset` for `reason` on this decision.
fn log_drop(world: &mut World, seat: SeatId, asset: u32, reason: Drop) {
    let tick = world.tick().raw();
    world
        .scratch_mut()
        .survey_drop(seat.raw(), asset, reason, tick);
}

/// The memory's part at the start of a decision of `seat`'s `members`
/// ([`Memory`]): begin the running Push, find dead every scout of the seat it
/// kept that no longer lives, and keep every member; whether any member
/// decides. The member walk is the decision's own (one unit a unit row, the
/// walk that finds whether any scout decides); the memory's bookkeeping is a
/// measurement and reports nothing to the meter.
fn note_scouts<M: crate::seams::Meter + ?Sized>(
    world: &mut World,
    seat: SeatId,
    members: Members,
    meter: &M,
) -> bool {
    let tick = world.tick().raw();
    let push = world.match_state().segment_started().raw();
    world.scratch_mut().survey.begin(push);
    // The seat's kept scouts, copied out of the scratch so the unit table
    // can be read beside them.
    let mut kept = [0_u32; SEAT_ROOM];
    let mut count: usize = 0;
    for (who, unit, _) in &world.scratch_mut().survey.scouts {
        if *who == seat.raw()
            && let Some(slot) = kept.get_mut(count)
        {
            *slot = *unit;
            count = count.saturating_add(1);
        }
    }
    let mut gone = [0_u32; SEAT_ROOM];
    let mut dead: usize = 0;
    for unit in kept.iter().take(count) {
        let units = world.units();
        let alive = units
            .row_of(UnitId::new(*unit))
            .ok()
            .and_then(|row| units.hit_points().get(row.index()).copied())
            .is_some_and(crate::math::quantity::Hp::is_alive);
        if !alive && let Some(slot) = gone.get_mut(dead) {
            *slot = *unit;
            dead = dead.saturating_add(1);
        }
    }
    let mut any = false;
    for unit in gone.iter().take(dead) {
        if world
            .scratch_mut()
            .survey
            .died(seat.raw(), *unit, tick)
            .is_err()
        {
            world.scratch_mut().survey_drops_unlogged.bump();
        }
    }
    for row in 0..crate::math::index::row(world.units().len()) {
        meter.rows(1);
        if !member(world, seat, members, row) {
            continue;
        }
        any = true;
        let (Some(unit), Some(place)) = (
            world.units().ids().get(row).copied(),
            world.units().positions().get(row).copied(),
        ) else {
            continue;
        };
        if world
            .scratch_mut()
            .survey
            .scout(seat.raw(), unit, place)
            .is_err()
        {
            world.scratch_mut().survey_drops_unlogged.bump();
        }
    }
    any
}

/// The memory's part at the end of a decision ([`Memory`]): every needed
/// sighting in a list, and fitting, is listed on this tick. An inserted
/// needed sighting fits by the insertion's own test; a kept head fits when its
/// bound walk and a re-ask finish within the Push. A measurement: it reports
/// nothing to the meter.
fn note_listed(world: &mut World, seat: SeatId, lists: &Lists) {
    let tick = world.tick().raw();
    let left = i64::from(ticks_left(world));
    let link = i64::from(world.link_ticks());
    for p in 0..lists.n {
        let Some(entry) = lists.get(p) else {
            continue;
        };
        let Some(owner) = entry.owner.filter(|_| entry.due.needed) else {
            continue;
        };
        let fits = !entry.kept
            || world.units().positions().get(owner).is_some_and(|at| {
                bound_ticks(world, *at, entry.due.place).saturating_add(link) <= left
            });
        let Some(unit) = world.units().ids().get(owner).copied() else {
            continue;
        };
        if fits
            && world
                .scratch_mut()
                .survey
                .listed(seat.raw(), entry.due.asset, tick, unit)
                .is_err()
        {
            world.scratch_mut().survey_drops_unlogged.bump();
        }
    }
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
/// 1. **Rank.** The seat's sightings open to a scout that are due a
///    re-check or needed (out of the window at the Lull with none before),
///    in [`Rank`] order: needed first, then enemy beacons, then oldest, ties
///    to the lowest asset id. A needed sighting not due yet is listed so the
///    list's walk counts it before it falls due, when the time to walk it may
///    be gone.
/// 2. **Rule (a), keep.** In ascending unit row, a scout already walking to a
///    due sighting no lower-row scout keeps has it at the head of its list.
/// 3. **Insertion.** Every other sighting, in rank order, goes at the end of
///    the list it lengthens least, so the scouts re-check in the spec's
///    order (the closest to leaving the window first, enemy beacons first, as
///    S2's `surv` pins); and where no list's end keeps every needed sighting
///    fitting, into the list and position where it lengthens that list
///    least (cheapest insertion), so another order can still fit it. The
///    lengthening is at the octile bound: for a position between `a` and
///    `b`, the leg `a` to it plus it to `b`, less `a` to `b` (from the
///    scout's own place for the front); ties to the later position, then the
///    lowest unit row. Never ahead of a
///    kept head; never where a needed sighting already listed, which fitted,
///    would stop fitting; and, for a needed sighting, only where it fits
///    itself (its bound finish plus a re-ask a leg within the Push). A list
///    already holding a sighting inside the same remembered armed range is
///    taken only when no other list takes it, so the sightings under one gun
///    are split across the seat's scouts rather than chained on one. Only a
///    scout the connectivity oracle connects to the sighting counts (none:
///    `Scratch::survey_unserved`). A needed sighting no list takes is
///    dropped for this decision under the [`Drop`] reason that names its
///    cause, in this order: under a threat the seat sees now (it was never
///    offered: decision 12); a scout lost, when it was listed and fitting
///    earlier this Push and a scout of the seat has died since: the one
///    whose list held it, one under its gun or in sight of it, or any while
///    the seat is short of scouts;
///    out of reach, when it was never listed this Push and no scout alone
///    reaches it before the Lull; crowded out otherwise.
/// 4. **Departure.** In ascending unit row, a scout with a list walks to its
///    head when it is already walking there; when its head is due and it has
///    nothing to scan or the Push is in its last [`REFRESH_INTERVAL`]; or
///    when its list no longer leaves time to scan on ([`list_leaves_time`],
///    which prices the served legs on a [`pricing_decision`] only and reads
///    the octile bounds alone on any other, setting the scout off unpriced
///    when they leave no time). Every other scout
///    scans: the nearest unexplored cell no lower-row scout of its beacon is
///    walking to; with none, a cell due a re-scan (decision 7); with none, it
///    idles at its beacon. "Scan first" stays the rule: only the scouts a list
///    needs leave the scan.
///
/// A scout that has arrived where it was sent parks, so the end of the route
/// does not carry it off before the next decision looks.
///
/// Every loop reports the counted work it does to `meter`
/// ([`crate::seams::Meter`]; decisions-log item 175 (1)): one unit a row it
/// reads at its innermost loops (a unit row, a sighting row, a list
/// position), the threat scan's rows through
/// [`crate::defend::for_each_known_threat_metered`], and the nodes each served
/// leg's search pops. A meter only hears counts: nothing this decides changes
/// with it. [`decide_scouts`] and [`run_scout`] pass `()`, because the scouts
/// are decided in the Programs phase, outside any seat's decision, as
/// Attack's breach search is (item 175 (4)).
///
/// Work per decision, at a seat's most due sightings `n` (64, the sighting
/// table's share), its scouts `m` and the seat's rows in the sighting table
/// `t`: the rank walk reads `n × t` rows and the armed ranges `n × t`; the
/// insertion walks each of `m` lists, of at most `n` entries, a fixed number
/// of times a sighting, with the prefix bounds, the fit margins and the span
/// read from arrays, so `n² × m` octile bounds in all; the departure prices
/// a scout's served legs up to its last needed sighting, at most `n`, on a
/// pricing decision only, and none on any other. No replacement's leg is
/// priced: the death margin that priced one was not kept
/// ([`list_leaves_time`]). Naming a drop reads the sighting table for the
/// sighting's gun and the unit and beacon tables for the seat's scouts,
/// once a drop; the [`Memory`] it consults is a measurement and reports
/// nothing.
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
    if !note_scouts(world, seat, members, meter) {
        world.put_threat_scratch(threats);
        return;
    }
    drop_threatened(world, seat, &threats, meter);
    let mut lists = Lists {
        entries: [None; LIST_ROOM],
        n: 0,
    };
    // The seat's due sightings, in rank order, but for those a scout this
    // decision does not decide is walking to.
    let mut after = None;
    while let Some(due) = next_due(world, seat, after, &threats, meter) {
        after = Some(due.rank);
        if walked_by_another(world, seat, members, due.place, meter) {
            continue;
        }
        let due = Due {
            armed: gun_over(world, seat, due.place, meter).map(|(asset, _, _)| asset),
            ..due
        };
        // E7: cannot miss: the sighting table holds at most `LIST_ROOM` sightings a seat
        let Some(slot) = lists.entries.get_mut(lists.n) else {
            break;
        };
        *slot = Some(Entry {
            due,
            owner: None,
            next: None,
            kept: false,
            first: false,
        });
        lists.n = lists.n.saturating_add(1);
    }
    keep(world, seat, members, &mut lists, meter);
    insert(world, seat, members, &mut lists, meter);
    note_listed(world, seat, &lists);
    depart_or_scan(world, seat, members, &lists, &threats, meter);
    world.put_threat_scratch(threats);
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
    let table = world.sightings();
    let mut best: Option<Due> = None;
    for slot in 0..table.seats().len() {
        // One unit for the row this walk reads; `due_in` counts its own.
        meter.rows(1);
        if table.seats().get(slot).copied() != Some(seat.raw()) {
            continue;
        }
        let Some(due) = due_in(world, seat, slot, threats, meter) else {
            continue;
        };
        if after.is_some_and(|after| due.rank <= after) {
            continue;
        }
        if best.is_none_or(|held| due.rank < held.rank) {
            best = Some(due);
        }
    }
    best
}

/// Step 2: in ascending unit row, every member walking to a due sighting no
/// lower-row member keeps has it at the head of its list.
fn keep<M: crate::seams::Meter + ?Sized>(
    world: &World,
    seat: SeatId,
    members: Members,
    lists: &mut Lists,
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
        let held = (0..lists.n).find(|p| {
            meter.rows(1);
            lists
                .get(*p)
                .is_some_and(|entry| entry.owner.is_none() && entry.due.place == to)
        });
        if let Some(p) = held
            && let Some(mut entry) = lists.get(p)
        {
            entry.owner = Some(row);
            entry.kept = true;
            entry.first = true;
            lists.set(p, entry);
        }
    }
}

/// Every needed sighting of `seat`'s whose place a threat the seat sees now
/// covers: it is no destination (decision 12), so no list holds it, and it is
/// dropped as [`Drop::UnderThreat`], whatever came before. One unit a
/// sighting row.
fn drop_threatened<M: crate::seams::Meter + ?Sized>(
    world: &mut World,
    seat: SeatId,
    threats: &[([Fx; 3], Sq)],
    meter: &M,
) {
    if threats.is_empty() {
        return;
    }
    let tick = world.tick().raw();
    let window = world.rules().attack().reach_memory.to_ticks_floor();
    let left = ticks_left(world);
    let mut covered = [0_u32; LIST_ROOM];
    let mut count: usize = 0;
    let table = world.sightings();
    for slot in 0..table.seats().len() {
        meter.rows(1);
        let (Some(who), Some(asset), Some(place), Some(seen)) = (
            table.seats().get(slot).copied(),
            table.assets().get(slot).copied(),
            table.places().get(slot).copied(),
            table.seen_at().get(slot).copied(),
        ) else {
            continue;
        };
        // E7: rule: as `due_in`'s: a sighting is never later than now, and an age and a tick count of one match sum inside `u32`
        let needed = tick.saturating_sub(seen).saturating_add(left) > window;
        if who == seat.raw()
            && needed
            && threats
                .iter()
                .any(|(at, reach)| Sq::between(*at, place) <= *reach)
            && let Some(held) = covered.get_mut(count)
        {
            *held = asset;
            count = count.saturating_add(1);
        }
    }
    for asset in covered.iter().take(count) {
        log_drop(world, seat, *asset, Drop::UnderThreat);
    }
}

/// One candidate place for a sighting in one list: the scout's unit row, the
/// entry it would follow (`None`, the front), and how much it lengthens the
/// list.
#[derive(Clone, Copy, Debug)]
struct Place {
    row: usize,
    after: Option<usize>,
    /// The insertion's position in the list, for the tie-break.
    at: usize,
    delta: i64,
}

/// Step 3: every due sighting in no list, in rank order, into the list and
/// position where it lengthens that list least.
#[allow(
    clippy::too_many_lines,
    reason = "one pass over a list and its positions per sighting: the prefix bounds, the fit margins and the span read the same arrays, and splitting them would pass the arrays and every bound through a second function"
)]
fn insert<M: crate::seams::Meter + ?Sized>(
    world: &mut World,
    seat: SeatId,
    members: Members,
    lists: &mut Lists,
    meter: &M,
) {
    let left = i64::from(ticks_left(world));
    let link = i64::from(world.link_ticks());
    let window = i64::from(world.rules().attack().reach_memory.to_ticks_floor());
    // The departure's slack beside the re-asks: twice the pricing cadence
    // (`list_leaves_time`). A `u32` interval times a small cadence, doubled:
    // inside an `i64`.
    let cadence = 2 * i64::from(world.decision_ticks()) * i64::from(REPRICE_DECISIONS);
    let mut order = [0_usize; LIST_ROOM];
    let mut prefix = [0_i64; LIST_ROOM];
    let mut margin = [i64::MAX; LIST_ROOM];
    for p in 0..lists.n {
        meter.rows(1);
        let Some(entry) = lists.get(p) else {
            continue;
        };
        if entry.owner.is_some() {
            continue;
        }
        let due = entry.due;
        let [gx, gy] = column_of(due.place);
        let goal = world.surface().node_of(gx, gy);
        // The best place among lists with no sighting under the same gun, and
        // among the rest.
        // The best place in each preference tier: at the end of a list (rank
        // order kept) before anywhere else, and in a list with no sighting
        // under the same gun before one that has.
        let mut split_end: Option<Place> = None;
        let mut split_any: Option<Place> = None;
        let mut chained_end: Option<Place> = None;
        let mut chained_any: Option<Place> = None;
        let mut connected = false;
        let mut alone_reaches = false;
        for row in 0..crate::math::index::row(world.units().len()) {
            meter.rows(1);
            if !member(world, seat, members, row) {
                continue;
            }
            let Some(start) = world.units().positions().get(row).copied() else {
                continue;
            };
            let [sx, sy] = column_of(start);
            let reachable =
                goal.zip(world.surface().node_of(sx, sy))
                    .is_some_and(|(goal, from)| {
                        world.clusters().connected(world.surface(), from, goal)
                    });
            if !reachable {
                continue;
            }
            connected = true;
            if bound_ticks(world, start, due.place).saturating_add(link) <= left {
                alone_reaches = true;
            }
            // The list in order, with each entry's bound finish and the
            // margin the needed entries from it on leave.
            let len = lists.sequence(row, &mut order, meter);
            let mut at = start;
            let mut finish: i64 = 0;
            let mut shares_gun = false;
            // The first and last needed entries: the refresh that will be
            // oldest at the Lull, and the one the list must reach in time.
            let mut first_needed: Option<usize> = None;
            let mut last_needed: Option<usize> = None;
            for (index, q) in order.iter().take(len).enumerate() {
                meter.rows(1);
                let Some(listed) = lists.get(*q) else {
                    continue;
                };
                finish = finish.saturating_add(bound_ticks(world, at, listed.due.place));
                at = listed.due.place;
                if let Some(slot) = prefix.get_mut(index) {
                    *slot = finish;
                }
                if listed.due.needed {
                    first_needed = first_needed.or(Some(index));
                    last_needed = Some(index);
                }
                if due.armed.is_some() && listed.due.armed == due.armed {
                    shares_gun = true;
                }
            }
            // From the back: the least slack any needed entry from `index` on
            // has, among those that fit now.
            let mut least = i64::MAX;
            for index in (0..len).rev() {
                meter.rows(1);
                let Some(listed) = order.get(index).and_then(|q| lists.get(*q)) else {
                    continue;
                };
                let Some(finish) = prefix.get(index).copied() else {
                    continue;
                };
                // E7: rule: at most 64 legs of a re-ask each, inside an `i64`
                let fitted = finish.saturating_add(link.saturating_mul(index_legs(index))) <= left;
                if listed.due.needed && fitted {
                    least =
                        least.min(left.saturating_sub(finish).saturating_sub(
                            link.saturating_mul(index_legs(index).saturating_add(1)),
                        ));
                }
                if let Some(slot) = margin.get_mut(index) {
                    *slot = least;
                }
            }
            // Every position: before the entry at `index` (or at the end).
            // A kept head stays first: positions start after it.
            let first = usize::from(
                len > 0
                    && order
                        .first()
                        .and_then(|q| lists.get(*q))
                        .is_some_and(|e| e.kept),
            );
            for index in first..=len {
                meter.rows(1);
                let before = match index.checked_sub(1) {
                    None => start,
                    Some(back) => match order.get(back).and_then(|q| lists.get(*q)) {
                        Some(e) => e.due.place,
                        None => continue,
                    },
                };
                let next = order
                    .get(index)
                    .filter(|_| index < len)
                    .and_then(|q| lists.get(*q));
                let to_it = bound_ticks(world, before, due.place);
                let delta = match next {
                    Some(next) => to_it
                        .saturating_add(bound_ticks(world, due.place, next.due.place))
                        .saturating_sub(bound_ticks(world, before, next.due.place)),
                    None => to_it,
                };
                // Needed entries after the insertion must still fit.
                let keeps = index == len || margin.get(index).is_none_or(|least| delta <= *least);
                // A needed sighting must fit where it goes.
                let reach = match index.checked_sub(1) {
                    None => 0,
                    Some(back) => match prefix.get(back).copied() {
                        Some(reach) => reach,
                        None => continue,
                    },
                };
                let fits_itself = !due.needed
                    || reach
                        .saturating_add(to_it)
                        .saturating_add(link.saturating_mul(index_legs(index).saturating_add(1)))
                        <= left;
                // The span from the list's first needed refresh to its last,
                // with the slack, must stay inside the reach window, or the
                // first would be stale at the Lull: before and after.
                let shifted = |index_of: usize| {
                    prefix.get(index_of).copied().map(|at| {
                        if index_of >= index {
                            at.saturating_add(delta)
                        } else {
                            at
                        }
                    })
                };
                let own = due.needed.then(|| reach.saturating_add(to_it));
                let span_of = |first: Option<i64>, last: Option<i64>, legs: i64| match (first, last)
                {
                    (Some(earliest), Some(latest)) => latest
                        .saturating_sub(earliest)
                        .saturating_add(cadence)
                        .saturating_add(link.saturating_mul(legs)),
                    _ => 0,
                };
                let earliest_now = first_needed.and_then(|i| prefix.get(i).copied());
                let latest_now = last_needed.and_then(|i| prefix.get(i).copied());
                // E7: rule: a list with no needed entry has no span, and no legs to count
                let legs_now = last_needed.map_or(0, index_legs);
                let old_span = span_of(earliest_now, latest_now, legs_now);
                let first_after = [first_needed.and_then(shifted), own]
                    .into_iter()
                    .flatten()
                    .min();
                let last_after = [last_needed.and_then(shifted), own]
                    .into_iter()
                    .flatten()
                    .max();
                let legs_after = last_needed
                    .map_or(0, |i| index_legs(i).saturating_add(1))
                    .max(if due.needed { index_legs(index) } else { 0 });
                let new_span = span_of(first_after, last_after, legs_after);
                let in_window = new_span <= window || new_span <= old_span;
                if !keeps || !fits_itself || !in_window {
                    continue;
                }
                let place = Place {
                    row,
                    after: index.checked_sub(1).and_then(|i| order.get(i).copied()),
                    at: index,
                    delta,
                };
                // Least lengthening; ties to the later position, then the
                // lowest unit row.
                let key = (delta, core::cmp::Reverse(index), row);
                let better = |held: &Option<Place>| {
                    held.is_none_or(|held| {
                        key < (held.delta, core::cmp::Reverse(held.at), held.row)
                    })
                };
                let (end, any) = if shares_gun {
                    (&mut chained_end, &mut chained_any)
                } else {
                    (&mut split_end, &mut split_any)
                };
                if index == len && better(end) {
                    *end = Some(place);
                }
                if better(any) {
                    *any = Some(place);
                }
            }
        }
        match split_end.or(split_any).or(chained_end).or(chained_any) {
            Some(place) => lists.insert(p, place.row, place.after, meter),
            None if !connected => world.scratch_mut().survey_unserved.bump(),
            None if due.needed => {
                let reason = drop_reason(
                    world,
                    seat,
                    members,
                    due.asset,
                    due.place,
                    alone_reaches,
                    meter,
                );
                log_drop(world, seat, due.asset, reason);
            }
            // An un-needed sighting no list takes waits: nothing in it would
            // be out of the window at the Lull.
            None => {}
        }
    }
}

/// The legs up to and including the entry at `index` of a list: `index + 1`.
fn index_legs(index: usize) -> i64 {
    match i64::try_from(index) {
        Ok(index) => index.saturating_add(1),
        // E7: cannot miss: an index into a list of at most 64 entries
        Err(_) => i64::MAX,
    }
}

/// Step 4: in ascending unit row, every member with a list walks to its head
/// when it must, and every other member scans, re-scans or idles; a member
/// that arrived parks.
///
/// A member walking to a scan column a known threat covers has looked at
/// that cell ([`seen_under_a_gun`]), whether it then leaves for its list or
/// scans on. A member that scans on keeps the cell it set off for while the
/// cell is open ([`scan_walk`]), and otherwise takes the nearest open cell:
/// an unexplored one first, then one due a re-scan.
fn depart_or_scan<M: crate::seams::Meter + ?Sized>(
    world: &mut World,
    seat: SeatId,
    members: Members,
    lists: &Lists,
    threats: &[([Fx; 3], Sq)],
    meter: &M,
) {
    let last_minute = refresh_pass(world);
    let mut order = [0_usize; LIST_ROOM];
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
        let len = lists.sequence(row, &mut order, meter);
        let first = order
            .first()
            .filter(|_| len > 0)
            .and_then(|q| lists.get(*q))
            .map(|entry| entry.due);
        let head = first.map(|due| due.place);
        // A scout with nothing to scan, or in the last minute, re-checks its
        // head when the head is due; and any scout sets off when its list
        // needs the time left.
        let leaves = first.is_some_and(|first| {
            first.place == to
                || (first.ripe && (last_minute || !has_scan(world, seat, row, threats)))
                || !list_leaves_time(world, lists, row, &order, len, meter)
        });
        let threatened = |point: [Fx; 3]| {
            threats
                .iter()
                .any(|(at, reach)| Sq::between(*at, point) <= *reach)
        };
        let walking_to = scan_walk(world, seat, home, here, to);
        let walking_to = match walking_to {
            Some((cell, point)) if threatened(point) => {
                seen_under_a_gun(world, seat, home, here, cell);
                None
            }
            other => other,
        };
        let target = if leaves {
            head
        } else {
            let open = |point: [Fx; 3]| !threatened(point) && !claimed(world, home, row, point);
            walking_to
                .map(|(_, point)| point)
                .filter(|point| open(*point))
                .or_else(|| {
                    nearest_cell(world, seat, home, here, Pick::Unexplored, &open)
                        .or_else(|| nearest_cell(world, seat, home, here, Pick::Due, &open))
                        .map(|(_, point)| point)
                })
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

/// Whether the member in unit `row` may scan on: its list `order[..len]`,
/// walked from where it stands up to its last needed sighting, leaves time
/// before the Lull.
///
/// A scout that dies on its list (decisions-log item 173 (2): the refresh
/// wins, and the losses it costs are accepted) is answered by the insertion,
/// not here: sightings under one remembered gun are split across the seat's
/// scouts before they are chained on one, so a scout killed refreshing a core
/// does not take the next sighting under that gun with it; and a needed
/// sighting no list takes while the seat is short of a scout is counted as
/// [`Drop::ScoutLost`]. A death margin in this check was built and measured
/// twice, and is not kept: fix pass 3's (a replacement's walk from the post
/// after every leg under a gun) and fix pass 4's narrow form (one margin per
/// needed sighting under a gun, the larger of a replacement's walk from the
/// post and another scout's list end, on that sighting's final departure
/// only). Each set the scouts off so soon that more of them died at the
/// guns, fewer cells were explored, and more sightings were stale at the
/// Lull on the review's thirty seeds; the figures are in PR #210.
///
/// What the code does. On a [`pricing_decision`] it prices the served legs in
/// order ([`ticks_of`] at the scout's speed). On any other decision it
/// reads the octile bounds only: if they leave time the scout scans on, and
/// if they do not it sets off at once, unpriced. A scout's arrival is never
/// later than [`ticks_of`] from the tick the router serves the route, plus
/// one re-ask a leg ([`World::link_ticks`]); a scout sets off up to
/// [`REPRICE_DECISIONS`] decision intervals after its list first needed the
/// rest, and meanwhile it walks on with its scan, which lengthens its first
/// leg by at most the ticks it walked (each step away is one step back): the
/// slack is twice the pricing cadence and a re-ask a leg. Sight is further
/// slack the rule does not count on: a scout renews a sighting once the place
/// is within its vision radius, before it arrives. A list with no needed
/// sighting always leaves time.
fn list_leaves_time<M: crate::seams::Meter + ?Sized>(
    world: &mut World,
    lists: &Lists,
    row: usize,
    order: &[usize; LIST_ROOM],
    len: usize,
    meter: &M,
) -> bool {
    let Some(start) = world.units().positions().get(row).copied() else {
        return true;
    };
    let mut places = [[Fx::ZERO; 3]; LIST_ROOM];
    let mut needed: usize = 0;
    for (index, q) in order.iter().take(len).enumerate() {
        meter.rows(1);
        let Some(entry) = lists.get(*q) else {
            continue;
        };
        if let Some(place) = places.get_mut(index) {
            *place = entry.due.place;
        }
        if entry.due.needed {
            needed = index.saturating_add(1);
        }
    }
    if needed == 0 {
        return true;
    }
    let left = i64::from(ticks_left(world));
    // A `u32` interval times a small cadence, doubled, plus a `u32` re-ask
    // a leg for at most 64 legs: inside an `i64`.
    let slack = 2 * i64::from(world.decision_ticks()) * i64::from(REPRICE_DECISIONS)
        + i64::from(world.link_ticks()) * index_legs(needed.saturating_sub(1));
    // The walk up to the last needed sighting, at the bound or served; `None`
    // when a leg has no walk.
    let walked = |world: &mut World, priced: bool| -> Option<i64> {
        let speed = Speed {
            link_ticks: world.link_ticks(),
            ..Speed::of_kind(world.rules(), UnitKind::Scout)
        };
        let mut total: i64 = 0;
        let mut from = start;
        for to in places.iter().take(needed) {
            meter.rows(1);
            let leg = if priced {
                let [ax, ay] = column_of(from);
                let [bx, by] = column_of(*to);
                let (Some(a), Some(b)) = (
                    world.surface().node_of(ax, ay),
                    world.surface().node_of(bx, by),
                ) else {
                    return None;
                };
                served_walk_ticks(world, a, b, speed, meter)?
            } else {
                bound_ticks(world, from, *to)
            };
            total = total.saturating_add(leg);
            from = *to;
        }
        Some(total)
    };
    let leaves = |total: Option<i64>| total.is_some_and(|total| total.saturating_add(slack) < left);
    if !leaves(walked(world, false)) {
        return false;
    }
    if !pricing_decision(world) {
        return true;
    }
    // A leg the router serves no walk for is counted where it was priced
    // (`served_walk_ticks`); the list cannot be trusted to leave time, so the
    // scout sets off for its head and the next decision builds the lists
    // again from where it then stands.
    leaves(walked(world, true))
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
/// `start` to `goal` at `speed` ([`ticks_of`]), or `None` when it serves
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
    let before = crate::targeting::popped(scratch);
    let walked = walk_cost_in(ground.surface, ground.clusters, scratch, (start, goal));
    meter.measured(crate::targeting::popped(scratch).and_then(|after| {
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
    match ticks_of(walk, speed) {
        Ok(ticks) => ticks.map(i64::from),
        Err(PastTicks::Cost { .. } | PastTicks::Ticks { .. }) => Some(i64::MAX),
    }
}

/// Whether this decision tick prices the served legs of the scouts' lists:
/// the Push's first decision, then every [`REPRICE_DECISIONS`]th. On any other
/// decision no leg is priced: [`list_leaves_time`] reads the octile bounds
/// alone, and a scout whose bounds leave no time sets off unpriced.
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
    use crate::math::fixed::Fx;
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

    /// A Push of ten minutes with seat 0's post at the map's centre, `scouts`
    /// scouts at it and a probe strip along the south edge, played to
    /// `left_s` seconds before its end with the scouts back at the post.
    fn late_in_the_push(scouts: usize, left_s: u32) -> (Runner, i32, i32) {
        let mut runner = arranged(scouts, 0);
        let end: u32 = 12_000;
        while runner.world().tick().raw() < end - left_s * 20 {
            step(&mut runner);
        }
        let map = runner
            .world()
            .rules()
            .message()
            .map
            .unwrap_or_else(|| panic!("a map block"));
        let (cx, cy) = (
            i32::try_from(map.size_x.div_euclid(2)).unwrap_or_else(|error| panic!("{error}")),
            i32::try_from(map.size_y.div_euclid(2)).unwrap_or_else(|error| panic!("{error}")),
        );
        let post = runner.world().standing_point([cx, cy, 0]);
        let scouts: Vec<u32> = (0..crate::math::index::row(runner.world().units().len()))
            .filter(|row| {
                runner.world().units().kinds().get(*row).copied() == Some(UnitKind::Scout.id())
                    && runner.world().units().seats().get(*row).copied() == Some(0)
            })
            .filter_map(|row| runner.world().units().ids().get(row).copied())
            .collect();
        for id in scouts {
            assert!(
                runner
                    .world_mut()
                    .place_unit_directly(crate::tables::UnitId::new(id), post),
                "a scout"
            );
        }
        (runner, cx, cy)
    }

    /// Seat 0 sights `id`, of `kind`, at `[x, y]`, `age_s` seconds ago.
    fn sighted(runner: &mut Runner, kind: AssetKind, id: AssetId, at: [i32; 2], age_s: u32) {
        let now = runner.world().tick().raw();
        let place = runner.world().standing_point([at[0], at[1], 0]);
        runner.world_mut().see(
            SeatId::new(0),
            id,
            SeatId::new(1),
            kind.id(),
            NO_BLUEPRINT,
            place,
            now - age_s * 20,
        );
    }

    /// How old seat 0's sighting of `id` is at the Lull, or `None` when a
    /// re-check found nothing there and forgot it (these sightings remember
    /// nothing that stands in the world).
    fn age_at_the_lull(runner: &mut Runner, id: AssetId) -> Option<u32> {
        loop {
            match runner.step() {
                Ok(Some(_)) => runner.clear_events(),
                Ok(None) => break,
                Err(error) => panic!("a tick: {error}"),
            }
        }
        let now = runner.world().tick().raw();
        let table = runner.world().sightings();
        (0..table.seats().len())
            .find(|row| {
                table.seats().get(*row).copied() == Some(0)
                    && table.assets().get(*row).copied() == Some(id.raw())
            })
            .and_then(|row| table.seen_at().get(row).copied())
            .map(|seen| now - seen)
    }

    fn window() -> u32 {
        let path = crate::default_rules_path().unwrap_or_else(|| panic!("the rules table"));
        RulesTable::load(&path)
            .unwrap_or_else(|error| panic!("the rules load: {error}"))
            .attack()
            .reach_memory
            .to_ticks_floor()
    }

    /// Review 3's R3A: one scout, 110 s left, an enemy beacon 65 s old 100
    /// columns west (fresh at the Lull whatever happens) and a unit sighting
    /// 100 s old 100 columns east (needed). The needed one ranks first and is
    /// re-checked; before, the beacon took the only list's head and the unit
    /// sighting ended 210 s old, counted out of reach.
    #[test]
    fn a_needed_sighting_ranks_before_a_fresher_beacon() {
        let (mut runner, cx, cy) = late_in_the_push(1, 110);
        let beacon = AssetId::of_beacon(BeaconId::new(901))
            .unwrap_or_else(|error| panic!("an id: {error:?}"));
        let unit = AssetId::of_unit(crate::tables::UnitId::new(951))
            .unwrap_or_else(|error| panic!("an id: {error:?}"));
        sighted(&mut runner, AssetKind::Beacon, beacon, [cx - 100, cy], 65);
        sighted(&mut runner, AssetKind::Unit, unit, [cx + 100, cy], 100);
        let age = age_at_the_lull(&mut runner, unit);
        assert!(
            age.is_none_or(|age| age <= window()),
            "the needed sighting is re-checked before the Lull: {age:?} ticks"
        );
    }

    /// Review 3's R3B: one scout, 85 s left, two needed enemy beacon
    /// sightings, the older 100 columns east and the newer off the axis near
    /// the post. Near then far fits; far then near, the rank order, does not.
    #[test]
    fn another_order_fits_what_the_rank_order_does_not() {
        let (mut runner, cx, cy) = late_in_the_push(1, 85);
        let far = AssetId::of_beacon(BeaconId::new(901))
            .unwrap_or_else(|error| panic!("an id: {error:?}"));
        let near = AssetId::of_beacon(BeaconId::new(902))
            .unwrap_or_else(|error| panic!("an id: {error:?}"));
        sighted(&mut runner, AssetKind::Beacon, far, [cx + 100, cy], 130);
        sighted(
            &mut runner,
            AssetKind::Beacon,
            near,
            [cx + 30, cy + 40],
            120,
        );
        let window = window();
        let (far_age, near_age) = {
            let far_age = age_at_the_lull(&mut runner, far);
            let now = runner.world().tick().raw();
            let table = runner.world().sightings();
            let near_age = (0..table.seats().len())
                .find(|row| table.assets().get(*row).copied() == Some(near.raw()))
                .and_then(|row| table.seen_at().get(row).copied())
                .map(|seen| now - seen);
            (far_age, near_age)
        };
        assert!(
            far_age.is_none_or(|age| age <= window) && near_age.is_none_or(|age| age <= window),
            "both re-checked before the Lull: far {far_age:?}, near {near_age:?} ticks"
        );
    }

    /// A meter that keeps its rows and its measured units apart.
    #[derive(Debug, Default)]
    struct Counted {
        rows: core::cell::Cell<u64>,
        measured: core::cell::Cell<u64>,
    }

    impl crate::seams::Meter for Counted {
        fn rows(&self, rows: usize) {
            let rows = u64::try_from(rows).unwrap_or_else(|error| panic!("{error}"));
            self.rows.set(self.rows.get() + rows);
        }

        fn measured(&self, units: Result<u64, crate::seams::Unmeasured>) {
            let units = units.unwrap_or_else(|reason| panic!("measured: {reason:?}"));
            self.measured.set(self.measured.get() + units);
        }
    }

    /// One decision of one scout with one due sighting, its counted work
    /// tallied by hand from the loops it runs (as `dunit`'s
    /// `decision_units_equal_the_primitive_steps_counted` tallies a resolve):
    /// the rows, and apart from them the pops of the served legs it prices.
    #[test]
    fn the_scouts_decision_counts_exactly_the_rows_it_reads_and_the_pops() {
        for pricing in [false, true] {
            let mut runner = arranged(1, 1);
            while !(runner.world().is_decision_tick()
                && super::pricing_decision(runner.world()) == pricing)
            {
                step(&mut runner);
            }
            let world = runner.world();
            let seat = SeatId::new(0);
            let units = u64::from(world.units().len());
            let table = world.sightings();
            let rows = u64::try_from(table.seats().len()).unwrap_or_else(|error| panic!("{error}"));
            let mine = table
                .seats()
                .iter()
                .fold(0_u64, |count, who| count + u64::from(*who == 0));
            // The sighting is a beacon's, so its own range covers it: the armed
            // walk reads the table up to it.
            let armed_at = (0..table.seats().len())
                .position(|slot| table.seats().get(slot).copied() == Some(0))
                .unwrap_or_else(|| panic!("the sighting"));
            let armed_at = u64::try_from(armed_at).unwrap_or_else(|error| panic!("{error}"));
            let threat = Counted::default();
            crate::defend::for_each_known_threat_metered(world, seat, &threat, |_| {});
            let n: u64 = 1;
            let expected = threat.rows.get()
                // The member walk (`note_scouts`): every unit row.
                + units
                // The rank walk: `n + 1` passes over the table, and a `due_in`
                // read for each of the seat's rows on each.
                + (n + 1) * (rows + mine)
                // `walked_by_another`, the whole unit table, for the sighting.
                + n * units
                // The armed walk, up to the sighting itself.
                + n * (armed_at + 1)
                // `keep`: every unit row, and the member's find over the list.
                + units + n
                // `insert`: the sighting; every unit row; the member's head
                // find over the list; its empty list's walks; its one
                // position; and the splice's head find, at the front.
                + n + units + n + 1 + n
                // `depart_or_scan`: every unit row; the head find and the
                // list's one entry; `list_leaves_time`'s entry and its bound
                // leg, and on a pricing decision its served leg too.
                + units + n + 1 + 1 + 1 + u64::from(pricing);
            let before = crate::targeting::popped(runner.world_mut().scratch_mut())
                .unwrap_or_else(|reason| panic!("{reason:?}"));
            let meter = Counted::default();
            decide_seat(runner.world_mut(), seat, Members::Survey, &meter);
            let after = crate::targeting::popped(runner.world_mut().scratch_mut())
                .unwrap_or_else(|reason| panic!("{reason:?}"));
            assert_eq!(
                meter.rows.get(),
                expected,
                "pricing {pricing}: the rows read"
            );
            assert_eq!(
                meter.measured.get(),
                after - before,
                "pricing {pricing}: the pops of the legs priced"
            );
            assert_eq!(
                meter.measured.get() > 0,
                pricing,
                "pops are counted on a pricing decision alone"
            );
        }
    }

    /// One scout, 110 s left, two needed enemy beacon sightings 100 columns
    /// either side of the post: each alone is a 50 s walk, both together are
    /// not inside the Push. The one left out is first dropped as crowded out
    /// (a scout alone would reach it), not as out of reach; only once no scout
    /// alone can reach it before the Lull is it out of reach.
    #[test]
    fn a_sighting_a_scout_alone_reaches_is_crowded_out_not_out_of_reach() {
        let (mut runner, cx, cy) = late_in_the_push(1, 110);
        let west = AssetId::of_beacon(BeaconId::new(901))
            .unwrap_or_else(|error| panic!("an id: {error:?}"));
        let east = AssetId::of_beacon(BeaconId::new(902))
            .unwrap_or_else(|error| panic!("an id: {error:?}"));
        sighted(&mut runner, AssetKind::Beacon, west, [cx - 100, cy], 120);
        sighted(&mut runner, AssetKind::Beacon, east, [cx + 100, cy], 110);
        // The first decision drops one: while a scout alone would reach it.
        let mut first = None;
        for _ in 0..20 {
            step(&mut runner);
            first = runner
                .world_mut()
                .scratch_mut()
                .survey_drops()
                .first()
                .copied();
            if first.is_some() {
                break;
            }
        }
        assert_eq!(
            first.map(|(_, _, why, _)| why),
            Some(super::Drop::CrowdedOut),
            "the one left out is crowded out, not out of reach: {first:?}"
        );
    }

    /// Step until a seat-0 scout is sent to one of `places`; that place's
    /// index, the ticks the Push then had left and the scout's unit row.
    fn first_sent(runner: &mut Runner, places: &[[Fx; 3]]) -> (usize, u32, usize) {
        let end = runner.world().match_state().segment_started().raw()
            + runner.world().match_state().segment_ticks();
        while runner.world().tick().raw() < end {
            step(runner);
            let world = runner.world();
            let units = world.units();
            for row in 0..crate::math::index::row(units.len()) {
                if units.kinds().get(row).copied() != Some(UnitKind::Scout.id())
                    || units.seats().get(row).copied() != Some(0)
                {
                    continue;
                }
                let Some(to) = units.destinations().get(row).copied() else {
                    continue;
                };
                if let Some(index) = places.iter().position(|place| *place == to) {
                    return (index, end - world.tick().raw(), row);
                }
            }
        }
        panic!("no scout was sent to a sighting");
    }

    /// The place `seat 0` remembers `id` at.
    fn place_of(runner: &Runner, id: AssetId) -> [Fx; 3] {
        let table = runner.world().sightings();
        (0..table.seats().len())
            .find(|row| table.assets().get(*row).copied() == Some(id.raw()))
            .and_then(|row| table.places().get(row).copied())
            .unwrap_or_else(|| panic!("held"))
    }

    fn beacon_id(raw: u32) -> AssetId {
        AssetId::of_beacon(BeaconId::new(raw)).unwrap_or_else(|error| panic!("an id: {error:?}"))
    }

    fn unit_id(raw: u32) -> AssetId {
        AssetId::of_unit(crate::tables::UnitId::new(raw))
            .unwrap_or_else(|error| panic!("an id: {error:?}"))
    }

    /// Needed first: one scout, 110 s left, an enemy beacon sighting 65 s old
    /// forty columns west (due, and fresh at the Lull whatever happens) and a
    /// needed unit sighting sixty columns east. Both fit either way; the
    /// needed one heads the list, so the scout sets off east.
    #[test]
    fn a_needed_sighting_is_visited_before_one_that_is_not() {
        let (mut runner, cx, cy) = late_in_the_push(1, 110);
        sighted(
            &mut runner,
            AssetKind::Beacon,
            beacon_id(901),
            [cx - 40, cy],
            65,
        );
        sighted(
            &mut runner,
            AssetKind::Unit,
            unit_id(951),
            [cx + 60, cy],
            100,
        );
        let places = [
            place_of(&runner, unit_id(951)),
            place_of(&runner, beacon_id(901)),
        ];
        let (first, _, _) = first_sent(&mut runner, &places);
        assert_eq!(first, 0, "the needed sighting first");
    }

    /// The span: one scout, 330 s left, two needed unit sightings on a
    /// diagonal, 150 columns out either way. Visiting both takes longer than
    /// the reach window from the first refresh to the second, so one list
    /// cannot keep both fresh; the one listed is refreshed late enough to be
    /// fresh at the Lull, not first and then left to go stale.
    #[test]
    fn a_list_never_spans_more_than_the_reach_window() {
        let (mut runner, cx, cy) = late_in_the_push(1, 330);
        sighted(
            &mut runner,
            AssetKind::Unit,
            unit_id(951),
            [cx + 150, cy + 150],
            30,
        );
        sighted(
            &mut runner,
            AssetKind::Unit,
            unit_id(952),
            [cx - 150, cy - 150],
            20,
        );
        let places = [
            place_of(&runner, unit_id(951)),
            place_of(&runner, unit_id(952)),
        ];
        let post = runner.world().standing_point([cx, cy, 0]);
        let (first, left, _) = first_sent(&mut runner, &places);
        let to_first = places.get(first).map_or_else(
            || panic!("a place"),
            |place| super::bound_ticks(runner.world(), post, *place),
        );
        let window = i64::from(window());
        let fresh_for = i64::from(left) - to_first;
        assert!(
            fresh_for <= window,
            "set off with {left} ticks left, {to_first} from its first refresh: that refresh would be {fresh_for} ticks old at the Lull (window {window})"
        );
    }

    /// Split under one gun: two scouts, 50 s left (the last minute), two
    /// needed sightings two columns apart inside one remembered gun's range.
    /// They go to different scouts rather than both to one.
    #[test]
    fn sightings_under_one_gun_are_split_across_scouts() {
        let (mut runner, cx, cy) = late_in_the_push(2, 50);
        sighted(
            &mut runner,
            AssetKind::Beacon,
            beacon_id(901),
            [cx + 40, cy],
            100,
        );
        sighted(
            &mut runner,
            AssetKind::Unit,
            unit_id(951),
            [cx + 42, cy],
            100,
        );
        let places = [
            place_of(&runner, beacon_id(901)),
            place_of(&runner, unit_id(951)),
        ];
        for _ in 0..10 {
            step(&mut runner);
        }
        let units = runner.world().units();
        let sent: Vec<usize> = (0..crate::math::index::row(units.len()))
            .filter(|row| {
                units.kinds().get(*row).copied() == Some(UnitKind::Scout.id())
                    && units.seats().get(*row).copied() == Some(0)
            })
            .filter_map(|row| {
                let to = units.destinations().get(row).copied()?;
                places.iter().position(|place| *place == to)
            })
            .collect();
        assert!(
            sent.contains(&0) && sent.contains(&1),
            "each scout sent to one of the two: {sent:?}"
        );
    }

    /// Seat 0's unit rows of its scouts, in row order.
    fn scout_rows(runner: &Runner) -> Vec<usize> {
        let units = runner.world().units();
        (0..crate::math::index::row(units.len()))
            .filter(|row| {
                units.kinds().get(*row).copied() == Some(UnitKind::Scout.id())
                    && units.seats().get(*row).copied() == Some(0)
                    && units.hit_points().get(*row).is_some_and(|hp| hp.is_alive())
            })
            .collect()
    }

    /// Destroy the unit in `row` at once, on seat 1's order.
    fn kill_row(runner: &mut Runner, row: usize) {
        let id = runner
            .world()
            .units()
            .ids()
            .get(row)
            .copied()
            .unwrap_or_else(|| panic!("a unit in row {row}"));
        runner
            .world_mut()
            .request_damage(crate::world::DamageOrder {
                target: crate::world::DamageTarget::Unit(crate::tables::UnitId::new(id)),
                amount: crate::math::quantity::Damage::new(1_000_000)
                    .unwrap_or_else(|error| panic!("a positive amount: {error:?}")),
                by: SeatId::new(1),
                source: None,
            })
            .unwrap_or_else(|error| panic!("the order is filed: {error:?}"));
    }

    /// Seat 0's drops of `id` so far this Push, by reason.
    fn drops_of(runner: &mut Runner, id: AssetId) -> Vec<super::Drop> {
        runner
            .world_mut()
            .scratch_mut()
            .survey_drops()
            .iter()
            .filter(|(seat, asset, _, _)| *seat == 0 && *asset == id.raw())
            .map(|(_, _, why, _)| *why)
            .collect()
    }

    /// Fix pass 4, item 1: a miss that follows the death of the scout carrying
    /// the sighting is a scout lost, not out of reach. Two scouts, 90 s left:
    /// one at the post, one sent 180 columns west; a needed sighting of a unit
    /// a hundred columns east, which the post's scout alone reaches (a 50 s
    /// walk) and the far one does not, and seat 1's forward beacon twenty
    /// columns short of it, sighted, whose remembered gun covers it (a seat
    /// takes every enemy beacon for a core). The post's
    /// scout lists the unit, sets off, and is killed inside that gun's range
    /// before it has the unit in sight; the far scout cannot reach the unit
    /// alone before the Lull, and the unit was listed and fitting before the
    /// death, so the drop is [`super::Drop::ScoutLost`], never out of reach.
    #[test]
    fn a_miss_after_the_carrying_scout_dies_is_a_scout_lost() {
        let (mut runner, cx, cy) = late_in_the_push(2, 90);
        let rows = scout_rows(&runner);
        let (Some(near), Some(far)) = (rows.first().copied(), rows.get(1).copied()) else {
            panic!("two scouts: {rows:?}");
        };
        let far_id = runner
            .world()
            .units()
            .ids()
            .get(far)
            .copied()
            .unwrap_or_else(|| panic!("the far scout's id"));
        let west = runner.world().standing_point([cx - 180, cy, 0]);
        assert!(
            runner
                .world_mut()
                .place_unit_directly(crate::tables::UnitId::new(far_id), west),
            "the far scout"
        );
        let target = unit_id(951);
        sighted(&mut runner, AssetKind::Unit, target, [cx + 100, cy], 100);
        let forward = runner.world().standing_point([cx + 80, cy, 0]);
        let forward = runner
            .world_mut()
            .place_beacon_directly(SeatId::new(1), forward, MandateKind::None, PRIORITY_NORMAL)
            .unwrap_or_else(|| panic!("seat 1's forward beacon"));
        let forward = AssetId::of_beacon(forward).unwrap_or_else(|error| panic!("{error:?}"));
        sighted(&mut runner, AssetKind::Beacon, forward, [cx + 80, cy], 10);
        let place = place_of(&runner, target);
        let gun_at = place_of(&runner, forward);
        let gun = crate::math::fixed::Sq::of_radius(runner.world().rules().core_weapon().range());
        let sight = crate::math::fixed::Sq::of_radius(crate::math::fixed::Fx::from_voxels(
            runner
                .world()
                .rules()
                .unit(UnitKind::Scout)
                .vision_radius_voxels,
        ));
        // Walk until the near scout, sent to the unit, is inside the beacon's
        // gun and has the unit not yet in sight.
        let mut closing = false;
        for _ in 0..1_800 {
            step(&mut runner);
            let units = runner.world().units();
            let (Some(at), Some(to)) = (
                units.positions().get(near).copied(),
                units.destinations().get(near).copied(),
            ) else {
                panic!("the near scout");
            };
            closing = to == place
                && crate::math::fixed::Sq::between(at, gun_at) <= gun
                && crate::math::fixed::Sq::between(at, place) > sight;
            if closing {
                break;
            }
        }
        assert!(
            closing,
            "the near scout set off for the unit and closed on it"
        );
        assert!(
            drops_of(&mut runner, target).is_empty(),
            "listed and fitting while its scout lives"
        );
        // A decision with the scout where it now stands, then the kill.
        while !runner.world().is_decision_tick() {
            step(&mut runner);
        }
        kill_row(&mut runner, near);
        let mut dropped = Vec::new();
        for _ in 0..40 {
            step(&mut runner);
            dropped = drops_of(&mut runner, target);
            if !dropped.is_empty() {
                break;
            }
        }
        assert_eq!(
            dropped,
            vec![super::Drop::ScoutLost],
            "dropped once its scout died, as a scout lost"
        );
    }

    /// Fix pass 4, item 2: the drop log is the running Push's. A Push of two
    /// rounds; a drop logged in the first stays readable through its Lull, and
    /// the second Push's first decision, which drops nothing, empties it.
    #[test]
    fn a_push_with_no_drop_reads_an_empty_log() {
        let path = crate::default_rules_path().unwrap_or_else(|| panic!("the rules table"));
        let rules =
            RulesTable::load(&path).unwrap_or_else(|error| panic!("the rules load: {error}"));
        let mut world = World::new(&WorldConfig {
            match_seed: SEED,
            seats: 2,
            rules,
            match_settings: MatchSettings {
                segment_lengths_ms: vec![60_000, 60_000],
                round_limit: 2,
            },
        })
        .unwrap_or_else(|error| panic!("the world: {error}"));
        let field = world.standing_point([200, 200, 0]);
        let post = world
            .place_beacon_directly(SeatId::new(0), field, MandateKind::Survey, PRIORITY_NORMAL)
            .unwrap_or_else(|| panic!("a post"));
        world.set_scouts(post, 0);
        world
            .field_unit(SeatId::new(0), post, UnitKind::Scout, field)
            .unwrap_or_else(|| panic!("a scout"));
        let mut runner = Runner::new(world).unwrap_or_else(|error| panic!("a runner: {error}"));
        assert_eq!(runner.begin_push(), Ok(true));
        for _ in 0..10 {
            step(&mut runner);
        }
        let tick = runner.world().tick().raw();
        runner
            .world_mut()
            .scratch_mut()
            .survey_drop(0, 999, super::Drop::OutOfReach, tick);
        loop {
            match runner.step() {
                Ok(Some(_)) => runner.clear_events(),
                Ok(None) => break,
                Err(error) => panic!("a tick: {error}"),
            }
        }
        assert_eq!(
            runner.world_mut().scratch_mut().survey_drops().len(),
            1,
            "the first Push's drop is read at its Lull"
        );
        if runner.phase() == crate::runner::MatchPhase::Recap {
            runner
                .end_recap()
                .unwrap_or_else(|error| panic!("the recap closes: {error}"));
        }
        assert_eq!(runner.begin_push(), Ok(true));
        for _ in 0..10 {
            step(&mut runner);
        }
        assert_eq!(
            runner.world_mut().scratch_mut().survey_drops(),
            &[],
            "the second Push dropped nothing, and its log is empty"
        );
    }

    /// Fix pass 4, item 5: a needed sighting inside a threat the seat sees is
    /// no destination (decision 12), and it is counted under
    /// [`super::Drop::UnderThreat`], never silently stale. One scout, 110 s
    /// left; a needed beacon sighting twenty columns from the post, where an
    /// armed seat-1 raider stands in the post's sight.
    #[test]
    fn a_needed_sighting_under_a_seen_threat_is_counted() {
        let (mut runner, cx, cy) = late_in_the_push(1, 110);
        let target = beacon_id(901);
        sighted(&mut runner, AssetKind::Beacon, target, [cx + 20, cy], 100);
        let place = place_of(&runner, target);
        let home = (0..runner.world().beacons().ids().len())
            .find(|row| runner.world().beacons().seats().get(*row).copied() == Some(1))
            .and_then(|row| runner.world().beacons().ids().get(row).copied())
            .map_or_else(|| panic!("seat 1's core"), BeaconId::new);
        runner
            .world_mut()
            .field_unit(SeatId::new(1), home, UnitKind::Raider, place)
            .unwrap_or_else(|| panic!("a raider"));
        let mut dropped = Vec::new();
        for _ in 0..10 {
            step(&mut runner);
            dropped = drops_of(&mut runner, target);
            if !dropped.is_empty() {
                break;
            }
        }
        assert_eq!(
            dropped,
            vec![super::Drop::UnderThreat],
            "counted under its threat"
        );
        assert_eq!(
            runner
                .world_mut()
                .scratch_mut()
                .survey_under_threat()
                .unwrap_or_else(|error| panic!("{error:?}")),
            1,
            "once"
        );
    }

    /// Two scouts, 90 s left: the near one at the post, the far one 180
    /// columns west; a needed unit sighting a hundred columns east with no
    /// remembered gun near it. Step until the near scout is sent to it; the
    /// runner, the unit rows of the near and far scouts, and the sighting.
    fn carried(cx_dx: i32) -> (Runner, usize, usize, AssetId) {
        let (mut runner, cx, cy) = late_in_the_push(2, 90);
        let rows = scout_rows(&runner);
        let (Some(near), Some(far)) = (rows.first().copied(), rows.get(1).copied()) else {
            panic!("two scouts: {rows:?}");
        };
        let far_id = runner
            .world()
            .units()
            .ids()
            .get(far)
            .copied()
            .unwrap_or_else(|| panic!("the far scout's id"));
        let west = runner.world().standing_point([cx - 180, cy, 0]);
        assert!(
            runner
                .world_mut()
                .place_unit_directly(crate::tables::UnitId::new(far_id), west),
            "the far scout"
        );
        let target = unit_id(951);
        sighted(&mut runner, AssetKind::Unit, target, [cx + cx_dx, cy], 100);
        let place = place_of(&runner, target);
        for _ in 0..1_800 {
            step(&mut runner);
            if runner.world().units().destinations().get(near).copied() == Some(place) {
                return (runner, near, far, target);
            }
        }
        panic!("the near scout was never sent to the sighting");
    }

    /// Step until seat 0 drops `target`; its drops then.
    fn until_dropped(runner: &mut Runner, target: AssetId) -> Vec<super::Drop> {
        for _ in 0..40 {
            step(runner);
            let dropped = drops_of(runner, target);
            if !dropped.is_empty() {
                return dropped;
            }
        }
        Vec::new()
    }

    /// The scout carrying a needed sighting is killed at the start of its
    /// walk, far from the sighting and from any remembered gun, and nobody
    /// else reaches it alone: a scout lost, because the dead scout's list
    /// held it.
    #[test]
    fn a_miss_after_its_scout_dies_anywhere_is_a_scout_lost() {
        let (mut runner, near, _, target) = carried(100);
        while !runner.world().is_decision_tick() {
            step(&mut runner);
        }
        kill_row(&mut runner, near);
        assert_eq!(
            until_dropped(&mut runner, target),
            vec![super::Drop::ScoutLost],
            "the scout carrying it died"
        );
    }

    /// The scout carrying a needed sighting lives, but is carried off where
    /// it can no longer reach it, while another scout dies far from it: the
    /// death took nothing from this sighting, so it is crowded out, not a
    /// scout lost.
    #[test]
    fn an_unrelated_death_is_not_a_scout_lost() {
        let (mut runner, near, far, target) = carried(100);
        while !runner.world().is_decision_tick() {
            step(&mut runner);
        }
        let near_id = runner
            .world()
            .units()
            .ids()
            .get(near)
            .copied()
            .unwrap_or_else(|| panic!("the near scout's id"));
        let at = runner
            .world()
            .units()
            .positions()
            .get(far)
            .copied()
            .unwrap_or_else(|| panic!("the far scout"));
        assert!(
            runner
                .world_mut()
                .place_unit_directly(crate::tables::UnitId::new(near_id), at),
            "the near scout carried off"
        );
        kill_row(&mut runner, far);
        assert_eq!(
            until_dropped(&mut runner, target),
            vec![super::Drop::CrowdedOut],
            "listed, its scout alive, and the death elsewhere"
        );
    }

    /// One scout, 30 s left, and a needed beacon sighting 150 columns east (a
    /// 75 s walk): it is never listed, and no scout alone reaches it, so it is
    /// out of reach, counted once.
    #[test]
    fn a_sighting_never_listed_and_beyond_reach_is_out_of_reach() {
        let (mut runner, cx, cy) = late_in_the_push(1, 30);
        let target = beacon_id(901);
        sighted(&mut runner, AssetKind::Beacon, target, [cx + 150, cy], 160);
        let dropped = until_dropped(&mut runner, target);
        for _ in 0..20 {
            step(&mut runner);
        }
        assert_eq!(dropped, vec![super::Drop::OutOfReach]);
        assert_eq!(
            runner
                .world_mut()
                .scratch_mut()
                .survey_out_of_reach()
                .unwrap_or_else(|error| panic!("{error:?}")),
            1,
            "counted once, over several decisions"
        );
    }
}
