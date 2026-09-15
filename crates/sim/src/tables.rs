// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Structure-of-arrays tables and the CSR broadphase (decisions log item 67).
//!
//! Two shapes, both chosen by measurement rather than taste:
//!
//! * **Vec-backed `SoA` tables with the counts fixed at construction.** The G4
//!   spike ran 300 and 1 200 units from the same code with zero allocations per
//!   tick. Nothing here grows a vector after [`UnitTable::with_capacity`] and
//!   [`SeatTable::with_capacity`] return.
//! * **A CSR uniform grid rebuilt every tick by counting sort.** Cell edge
//!   equal to the largest query radius, so a radius query touches at most
//!   3 × 3 cells; the rebuild is `O(items + cells)` and allocation-free after
//!   construction.
//!
//! The caveat item 67 asked to be written down: the broadphase is
//! **super-linear in unit count** (+34 % quadratic share at 600 units in G3′)
//! and its cell size is a density-tied tuning value. It is 28 % of the quiet
//! tick at the gate load and 47 % at twice it — the phase to watch before the
//! unit ceiling doubles.
//!
//! # The cell size is a performance knob, not game state
//!
//! [`Csr::collect_in_radius`] takes its radius **in voxels**, widens it to
//! whole cells itself, and filters the superset the grid hands back by the
//! exact test `Sq::between(pos, centre) <= Sq::of_radius(radius)` before
//! sorting the survivors by id. So the *membership* of the answer is a function
//! of the world and the radius, and its *order* is a function of the ids: the
//! cell size can reach neither. That is what lets `tests/determinism.rs` assert
//! that changing the cell size leaves both the answer and the hash chain
//! byte-identical (G3′ §9.17: keep the calibration constant outside hashed
//! state, and *test* that it is). Nothing in this module is encoded into the
//! state hash.
//!
//! Sorting alone would not have bought that, and the earlier
//! radius-in-**cells** signature is the reason this paragraph exists: one cell
//! of radius is a 24-voxel box at a cell edge of 8 and a 48-voxel one at 16, so
//! the first tick phase to query the grid would have turned a knob the rules
//! table declares non-hashed into behaviour, and into the hash chain with it.
//! The exact filter is what makes the declaration true rather than vacuous.

use crate::math::fixed::{Angle, Fx, Sq};
use crate::math::quantity::{Hp, Kw, Money};

/// A unit's identity. Dense, assigned at construction, stable for the match.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct UnitId(u32);

impl UnitId {
    /// Build from a raw id.
    #[must_use]
    pub const fn new(raw: u32) -> UnitId {
        UnitId(raw)
    }

    /// The raw id, for the canonical encoder and for sort keys.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// A seat's identity: 0, 1 or 2 in v1. Ties break to the **lowest** seat id
/// everywhere in the sim (AGENTS.md §4.6).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct SeatId(u8);

impl SeatId {
    /// Build from a raw id.
    #[must_use]
    pub const fn new(raw: u8) -> SeatId {
        SeatId(raw)
    }

    /// The raw id.
    #[must_use]
    pub const fn raw(self) -> u8 {
        self.0
    }
}

/// Units, structure-of-arrays.
///
/// Every column has the same length and index `i` is the same unit in all of
/// them. The count is fixed at construction: a dead unit keeps its slot.
///
/// **This is the skeleton's minimum, not the final table.** T5 adds the rest of
/// the world's tables (beacons, structures, wrecks) and T11 adds what the
/// interpreter needs. Every column added here must go into *three* places in
/// the same pull request — [`crate::world::World::encode`], the snapshot round-trip
/// and the golden files (AGENTS.md §4.8).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UnitTable {
    count: u32,
    id: Vec<u32>,
    seat: Vec<u8>,
    pos: Vec<[Fx; 3]>,
    dest: Vec<[Fx; 3]>,
    heading: Vec<Angle>,
    hp: Vec<Hp>,
}

impl UnitTable {
    /// An empty table sized for `count` units. Nothing allocates after this.
    #[must_use]
    pub fn with_capacity(count: u32) -> UnitTable {
        let n = usize::try_from(count).unwrap_or(0);
        UnitTable {
            count: 0,
            id: Vec::with_capacity(n),
            seat: Vec::with_capacity(n),
            pos: Vec::with_capacity(n),
            dest: Vec::with_capacity(n),
            heading: Vec::with_capacity(n),
            hp: Vec::with_capacity(n),
        }
    }

    /// Append one unit. Construction only — never called inside a tick.
    pub fn push(&mut self, id: UnitId, seat: SeatId, pos: [Fx; 3], dest: [Fx; 3], hp: Hp) {
        self.id.push(id.raw());
        self.seat.push(seat.raw());
        self.pos.push(pos);
        self.dest.push(dest);
        self.heading.push(Angle::ZERO);
        self.hp.push(hp);
        self.count = self.count.saturating_add(1);
    }

    /// How many units the table holds.
    #[must_use]
    pub const fn len(&self) -> u32 {
        self.count
    }

    /// Whether the table is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The id column.
    #[must_use]
    pub fn ids(&self) -> &[u32] {
        &self.id
    }

    /// The seat column.
    #[must_use]
    pub fn seats(&self) -> &[u8] {
        &self.seat
    }

    /// The position column.
    #[must_use]
    pub fn positions(&self) -> &[[Fx; 3]] {
        &self.pos
    }

    /// The destination column.
    #[must_use]
    pub fn destinations(&self) -> &[[Fx; 3]] {
        &self.dest
    }

    /// The heading column.
    #[must_use]
    pub fn headings(&self) -> &[Angle] {
        &self.heading
    }

    /// The hit-point column.
    #[must_use]
    pub fn hit_points(&self) -> &[Hp] {
        &self.hp
    }

    /// Every column the movement phase touches, borrowed at once.
    ///
    /// One method rather than six accessors because the phase needs the
    /// read-only identity columns *and* the mutable position columns in the
    /// same loop, and copying the identity columns out to satisfy the borrow
    /// checker would be an allocation per tick — which is exactly what the
    /// zero-allocation assertion exists to catch.
    pub fn movement_columns(&mut self) -> MovementColumns<'_> {
        MovementColumns {
            ids: &self.id,
            seats: &self.seat,
            hit_points: &self.hp,
            positions: &mut self.pos,
            destinations: &mut self.dest,
            headings: &mut self.heading,
        }
    }

    /// Replace the whole table from the columns of a restored snapshot.
    ///
    /// Returns `false` and changes nothing when the columns disagree in
    /// length, which is what a corrupt or truncated snapshot looks like.
    pub fn restore(
        &mut self,
        id: Vec<u32>,
        seat: Vec<u8>,
        pos: Vec<[Fx; 3]>,
        dest: Vec<[Fx; 3]>,
        heading: Vec<Angle>,
        hp: Vec<Hp>,
    ) -> bool {
        let n = id.len();
        if seat.len() != n
            || pos.len() != n
            || dest.len() != n
            || heading.len() != n
            || hp.len() != n
        {
            return false;
        }
        let Ok(count) = u32::try_from(n) else {
            return false;
        };
        self.count = count;
        self.id = id;
        self.seat = seat;
        self.pos = pos;
        self.dest = dest;
        self.heading = heading;
        self.hp = hp;
        true
    }
}

/// The columns [`UnitTable::movement_columns`] hands out, borrowed together.
#[derive(Debug)]
pub struct MovementColumns<'a> {
    /// The id column, read-only.
    pub ids: &'a [u32],
    /// The seat column, read-only.
    pub seats: &'a [u8],
    /// The hit-point column, read-only: movement reads it to skip the dead.
    pub hit_points: &'a [Hp],
    /// The position column.
    pub positions: &'a mut [[Fx; 3]],
    /// The destination column.
    pub destinations: &'a mut [[Fx; 3]],
    /// The heading column.
    pub headings: &'a mut [Angle],
}

/// Per-seat treasury and power, structure-of-arrays.
///
/// The economy proper is T14's; what is here is the shape the hash and the
/// snapshot need from day one, so that adding the real columns is an ordinary
/// extension rather than a new table.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SeatTable {
    count: u32,
    seat: Vec<u8>,
    treasury: Vec<Money>,
    supply: Vec<Kw>,
    draw: Vec<Kw>,
}

impl SeatTable {
    /// An empty table sized for `count` seats.
    #[must_use]
    pub fn with_capacity(count: u32) -> SeatTable {
        let n = usize::try_from(count).unwrap_or(0);
        SeatTable {
            count: 0,
            seat: Vec::with_capacity(n),
            treasury: Vec::with_capacity(n),
            supply: Vec::with_capacity(n),
            draw: Vec::with_capacity(n),
        }
    }

    /// Append one seat. Construction only.
    pub fn push(&mut self, seat: SeatId, treasury: Money, supply: Kw, draw: Kw) {
        self.seat.push(seat.raw());
        self.treasury.push(treasury);
        self.supply.push(supply);
        self.draw.push(draw);
        self.count = self.count.saturating_add(1);
    }

    /// How many seats the table holds.
    #[must_use]
    pub const fn len(&self) -> u32 {
        self.count
    }

    /// Whether the table is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The seat-id column.
    #[must_use]
    pub fn seats(&self) -> &[u8] {
        &self.seat
    }

    /// The treasury column.
    #[must_use]
    pub fn treasuries(&self) -> &[Money] {
        &self.treasury
    }

    /// The power-supply column.
    #[must_use]
    pub fn supplies(&self) -> &[Kw] {
        &self.supply
    }

    /// The power-draw column.
    #[must_use]
    pub fn draws(&self) -> &[Kw] {
        &self.draw
    }

    /// Replace the whole table from a restored snapshot. Same contract as
    /// [`UnitTable::restore`].
    pub fn restore(
        &mut self,
        seat: Vec<u8>,
        treasury: Vec<Money>,
        supply: Vec<Kw>,
        draw: Vec<Kw>,
    ) -> bool {
        let n = seat.len();
        if treasury.len() != n || supply.len() != n || draw.len() != n {
            return false;
        }
        let Ok(count) = u32::try_from(n) else {
            return false;
        };
        self.count = count;
        self.seat = seat;
        self.treasury = treasury;
        self.supply = supply;
        self.draw = draw;
        true
    }
}

/// A CSR uniform grid over the map footprint, rebuilt every tick by counting
/// sort.
///
/// Not hashed, not snapshotted: it is a derived index, rebuilt from the
/// positions at the top of every tick by the `broadphase` phase.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Csr {
    cell_size_voxels: i32,
    cells_x: u32,
    cells_y: u32,
    origin_x: i32,
    origin_y: i32,
    /// `cells_x * cells_y + 1` entries; `starts[c]..starts[c + 1]` indexes
    /// [`Csr::items`].
    starts: Vec<u32>,
    /// One entry per item, holding the item's id.
    items: Vec<u32>,
    /// Scratch for the counting sort, `cells_x * cells_y` entries.
    cursor: Vec<u32>,
}

impl Csr {
    /// Build a grid covering `[origin, origin + extent)` voxels in x and y,
    /// with square cells of `cell_size_voxels`, sized for `capacity` items.
    ///
    /// Returns `None` when the cell size is not positive or the grid would not
    /// fit in `u32` — both of which are rules-table mistakes, caught once at
    /// construction rather than every tick.
    #[must_use]
    pub fn new(
        origin: [i32; 2],
        extent_voxels: [i32; 2],
        cell_size_voxels: i32,
        capacity: u32,
    ) -> Option<Csr> {
        if cell_size_voxels <= 0 {
            return None;
        }
        let cells_x = cells_across(*extent_voxels.first()?, cell_size_voxels)?;
        let cells_y = cells_across(*extent_voxels.get(1)?, cell_size_voxels)?;
        let cells = cells_x.checked_mul(cells_y)?;
        let cells_usize = usize::try_from(cells).ok()?;
        let capacity_usize = usize::try_from(capacity).ok()?;
        Some(Csr {
            cell_size_voxels,
            cells_x,
            cells_y,
            origin_x: *origin.first()?,
            origin_y: *origin.get(1)?,
            starts: vec![0; cells_usize.checked_add(1)?],
            items: vec![0; capacity_usize],
            cursor: vec![0; cells_usize],
        })
    }

    /// The cell edge, in whole voxels. A tuning value, never hashed.
    #[must_use]
    pub const fn cell_size_voxels(&self) -> i32 {
        self.cell_size_voxels
    }

    /// Cells along x and y.
    #[must_use]
    pub const fn cell_counts(&self) -> [u32; 2] {
        [self.cells_x, self.cells_y]
    }

    /// The cell a position falls in, clamped to the grid.
    #[must_use]
    pub fn cell_of(&self, pos: [Fx; 3]) -> u32 {
        let x = pos.first().map_or(0, |v| v.floor_voxels());
        let y = pos.get(1).map_or(0, |v| v.floor_voxels());
        let cx = self.axis_cell(x, self.origin_x, self.cells_x);
        let cy = self.axis_cell(y, self.origin_y, self.cells_y);
        cy.saturating_mul(self.cells_x).saturating_add(cx)
    }

    #[allow(
        clippy::integer_division,
        reason = "floor division into a grid cell; the operands are made non-negative first, so the rounding is unambiguous"
    )]
    fn axis_cell(&self, v: i32, origin: i32, cells: u32) -> u32 {
        let offset = v.saturating_sub(origin).max(0);
        let cell = offset / self.cell_size_voxels;
        let cell = u32::try_from(cell).unwrap_or(0);
        cell.min(cells.saturating_sub(1))
    }

    /// Rebuild the index from the positions of `count` items whose ids are
    /// `0..count`, by counting sort. Allocation-free.
    ///
    /// Returns `false` and leaves the grid empty when the table is larger than
    /// the capacity the grid was built for, which is a construction bug rather
    /// than a game state.
    pub fn rebuild(&mut self, positions: &[[Fx; 3]]) -> bool {
        self.cursor.fill(0);
        self.starts.fill(0);
        if positions.len() > self.items.len() {
            return false;
        }

        // Pass 1: count per cell.
        for pos in positions {
            let cell = usize::try_from(self.cell_of(*pos)).unwrap_or(0);
            if let Some(slot) = self.cursor.get_mut(cell) {
                *slot = slot.saturating_add(1);
            }
        }
        // Pass 2: prefix sums into `starts`.
        let mut running: u32 = 0;
        for (cell, count) in self.cursor.iter().enumerate() {
            if let Some(slot) = self.starts.get_mut(cell) {
                *slot = running;
            }
            running = running.saturating_add(*count);
        }
        if let Some(last) = self.starts.last_mut() {
            *last = running;
        }
        // Pass 3: scatter, reusing `cursor` as the write head per cell.
        let cells = self.cursor.len();
        if let Some(heads) = self.starts.get(..cells) {
            self.cursor.copy_from_slice(heads);
        }
        for (index, pos) in positions.iter().enumerate() {
            let cell = usize::try_from(self.cell_of(*pos)).unwrap_or(0);
            let Some(head) = self.cursor.get_mut(cell) else {
                continue;
            };
            let at = usize::try_from(*head).unwrap_or(0);
            *head = head.saturating_add(1);
            let Ok(id) = u32::try_from(index) else {
                continue;
            };
            if let Some(slot) = self.items.get_mut(at) {
                *slot = id;
            }
        }
        true
    }

    /// Collect every item within `radius` **voxels** of `centre`, sorted
    /// ascending by id, into `out`.
    ///
    /// `positions` must be the slice the grid was last [rebuilt](Csr::rebuild)
    /// from: the grid stores ids, and an id is that slice's index. An id the
    /// slice no longer covers is dropped rather than guessed at.
    ///
    /// The radius is in voxels and not in cells **on purpose** (see the module
    /// docs). The grid is only asked for a superset — every cell within
    /// [`Csr::radius_in_cells`] of the centre's — and the exact
    /// `Sq::between(pos, centre) <= Sq::of_radius(radius)` test decides
    /// membership, so the cell edge changes how much work this does and not
    /// what it answers.
    ///
    /// `out` is cleared first and is the caller's buffer, so a tick phase that
    /// keeps one around allocates nothing: the answer can never be longer than
    /// the item count the grid was built for.
    #[allow(
        clippy::integer_division,
        reason = "recovering the grid row from a cell index; both operands are non-negative and the divisor is the row stride, so the rounding is exact"
    )]
    pub fn collect_in_radius(
        &self,
        positions: &[[Fx; 3]],
        centre: [Fx; 3],
        radius: Fx,
        out: &mut Vec<u32>,
    ) {
        out.clear();
        if radius < Fx::ZERO {
            return;
        }
        let limit = Sq::of_radius(radius);
        let radius_cells = self.radius_in_cells(radius);
        let cell = self.cell_of(centre);
        let cx = cell % self.cells_x.max(1);
        let cy = cell / self.cells_x.max(1);
        let lo_x = cx.saturating_sub(radius_cells);
        let hi_x = cx
            .saturating_add(radius_cells)
            .min(self.cells_x.saturating_sub(1));
        let lo_y = cy.saturating_sub(radius_cells);
        let hi_y = cy
            .saturating_add(radius_cells)
            .min(self.cells_y.saturating_sub(1));
        let mut y = lo_y;
        while y <= hi_y {
            let mut x = lo_x;
            while x <= hi_x {
                let c =
                    usize::try_from(y.saturating_mul(self.cells_x).saturating_add(x)).unwrap_or(0);
                let from = self.starts.get(c).copied().unwrap_or(0);
                let to = self.starts.get(c.saturating_add(1)).copied().unwrap_or(0);
                let range = usize::try_from(from).unwrap_or(0)..usize::try_from(to).unwrap_or(0);
                if let Some(slice) = self.items.get(range) {
                    for id in slice {
                        let Some(pos) = positions.get(usize::try_from(*id).unwrap_or(usize::MAX))
                        else {
                            continue;
                        };
                        if Sq::between(*pos, centre) <= limit {
                            out.push(*id);
                        }
                    }
                }
                x = x.saturating_add(1);
            }
            y = y.saturating_add(1);
        }
        // Ids are unique, so an unstable sort is a total order (item 62's
        // convention: every sort key ends in a unique id).
        out.sort_unstable();
    }

    /// How many cells of grid a `radius`-voxel query has to touch for the
    /// superset to be complete.
    ///
    /// Two roundings, both upward, both deliberate. The radius rounds up to a
    /// whole voxel and gains one more, because [`Csr::cell_of`] floors a
    /// position to a voxel first and `floor(a) - floor(b) <= floor(a - b) + 1`;
    /// then the voxels round up into cells, because
    /// `floor(a / c) - floor(b / c) <= ceil((a - b) / c)` for non-negative
    /// operands. Over-collecting costs candidates the exact test then throws
    /// away; under-collecting would silently lose a unit.
    #[must_use]
    pub fn radius_in_cells(&self, radius: Fx) -> u32 {
        if radius < Fx::ZERO {
            return 0;
        }
        let almost_one = Fx::from_raw(Fx::ONE.raw().saturating_sub(1));
        let voxels = radius
            .saturating_add(almost_one)
            .floor_voxels()
            .saturating_add(1);
        cells_across(voxels, self.cell_size_voxels).unwrap_or(0)
    }
}

#[allow(
    clippy::integer_division,
    reason = "cells across an extent, rounded up; both operands are positive by the test above, so the rounding is exactly ceiling"
)]
fn cells_across(extent_voxels: i32, cell_size_voxels: i32) -> Option<u32> {
    if extent_voxels <= 0 || cell_size_voxels <= 0 {
        return None;
    }
    // `i32::div_ceil` is still unstable, and the hand-written form is the one
    // the rounding comment above is about anyway.
    let cells = extent_voxels.checked_add(cell_size_voxels.checked_sub(1)?)? / cell_size_voxels;
    u32::try_from(cells).ok()
}
