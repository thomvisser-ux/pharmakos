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
use crate::seams::BeaconMandate;

/// A unit's identity. Dense, assigned at construction, stable for the match.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct UnitId(u32);

impl UnitId {
    /// "No unit" — the sentinel a per-seat index carries for a seat that has
    /// none. Same reasoning as [`BeaconId::NONE`].
    pub const NONE: UnitId = UnitId(u32::MAX);

    /// Build from a raw id.
    #[must_use]
    pub const fn new(raw: u32) -> UnitId {
        UnitId(raw)
    }

    /// Whether this is a real unit rather than [`UnitId::NONE`].
    #[must_use]
    pub const fn is_some(self) -> bool {
        self.0 != UnitId::NONE.0
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
    /// Nobody's — the owner a neutral ruin carries.
    ///
    /// A sentinel rather than an `Option` for the reason [`BeaconId::NONE`] is
    /// one: the seat column is snapshotted and hashed, and the snapshot's rule
    /// is that every field is fixed-width with no tag byte whose layout could
    /// differ between targets ([`crate::snapshot`]). `255` is far above v1's
    /// three seats and above the determinism harness's four.
    pub const NEUTRAL: SeatId = SeatId(u8::MAX);

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

    /// Whether this is a real seat rather than [`SeatId::NEUTRAL`].
    #[must_use]
    pub const fn is_some(self) -> bool {
        self.0 != SeatId::NEUTRAL.0
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
    kind: Vec<u8>,
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
            kind: Vec::with_capacity(n),
            pos: Vec::with_capacity(n),
            dest: Vec::with_capacity(n),
            heading: Vec::with_capacity(n),
            hp: Vec::with_capacity(n),
        }
    }

    /// Append one unit. Construction only — never called inside a tick.
    pub fn push(
        &mut self,
        id: UnitId,
        seat: SeatId,
        kind: UnitKind,
        pos: [Fx; 3],
        dest: [Fx; 3],
        hp: Hp,
    ) {
        self.id.push(id.raw());
        self.seat.push(seat.raw());
        self.kind.push(kind.id());
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

    /// The kind column, as [`UnitKind::id`] wire values.
    #[must_use]
    pub fn kinds(&self) -> &[u8] {
        &self.kind
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

    /// The hit-point column, to write into.
    ///
    /// Three callers, all in [`crate::world`]'s tick: the combat phase's damage
    /// drain, elimination disbanding a seat's units on the spot, and a
    /// commander coming back from a respawn. A unit's hit points are the one
    /// thing about it that a phase other than movement changes.
    pub fn hit_points_mut(&mut self) -> &mut [Hp] {
        &mut self.hp
    }

    /// The position column, to write into.
    ///
    /// Movement writes it through [`UnitTable::movement_columns`]; this is for
    /// the one write that is not a walk — a commander reappearing at its core
    /// after a respawn (item 21).
    pub fn positions_mut(&mut self) -> &mut [[Fx; 3]] {
        &mut self.pos
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
            kinds: &self.kind,
            hit_points: &self.hp,
            positions: &mut self.pos,
            destinations: &mut self.dest,
            headings: &mut self.heading,
        }
    }

    /// The destination column, to write into.
    ///
    /// The one column a phase outside movement changes: the tick draws a unit a
    /// new place to be when it arrives, and T11's interpreter will do the same
    /// from a playbook.
    pub fn destinations_mut(&mut self) -> &mut [[Fx; 3]] {
        &mut self.dest
    }

    /// Replace the whole table from the columns of a restored snapshot.
    ///
    /// Returns `false` and changes nothing when the columns disagree in
    /// length, which is what a corrupt or truncated snapshot looks like.
    pub fn restore(&mut self, columns: UnitColumns) -> bool {
        let n = columns.id.len();
        if columns.seat.len() != n
            || columns.kind.len() != n
            || columns.pos.len() != n
            || columns.dest.len() != n
            || columns.heading.len() != n
            || columns.hp.len() != n
        {
            return false;
        }
        let Ok(count) = u32::try_from(n) else {
            return false;
        };
        self.count = count;
        self.id = columns.id;
        self.seat = columns.seat;
        self.kind = columns.kind;
        self.pos = columns.pos;
        self.dest = columns.dest;
        self.heading = columns.heading;
        self.hp = columns.hp;
        true
    }
}

/// Every column of a restored [`UnitTable`], handed over in one piece.
///
/// A struct rather than a parameter list because a table with seven columns has
/// seven chances to pass two of them the wrong way round, and a named field
/// cannot be swapped by accident.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct UnitColumns {
    /// Unit ids.
    pub id: Vec<u32>,
    /// The seat each unit belongs to.
    pub seat: Vec<u8>,
    /// Each unit's [`UnitKind::id`].
    pub kind: Vec<u8>,
    /// Positions.
    pub pos: Vec<[Fx; 3]>,
    /// Destinations.
    pub dest: Vec<[Fx; 3]>,
    /// Headings.
    pub heading: Vec<Angle>,
    /// Hit points.
    pub hp: Vec<Hp>,
}

/// The columns [`UnitTable::movement_columns`] hands out, borrowed together.
#[derive(Debug)]
pub struct MovementColumns<'a> {
    /// The id column, read-only.
    pub ids: &'a [u32],
    /// The seat column, read-only.
    pub seats: &'a [u8],
    /// The kind column, read-only: movement reads it for the walking speed
    /// item 90 gives each kind.
    pub kinds: &'a [u8],
    /// The hit-point column, read-only: movement reads it to skip the dead.
    pub hit_points: &'a [Hp],
    /// The position column.
    pub positions: &'a mut [[Fx; 3]],
    /// The destination column.
    pub destinations: &'a mut [[Fx; 3]],
    /// The heading column.
    pub headings: &'a mut [Angle],
}

/// "This seat is still in the match" — the value [`SeatTable::eliminated_at`]
/// carries for a seat that has not been eliminated.
///
/// A sentinel rather than an `Option`, for the reason [`BeaconId::NONE`] is
/// one. It is not a tick any match reaches: `u32::MAX` ticks is over six years
/// of game time ([`crate::math::quantity::Tick::next`]).
pub const NOT_ELIMINATED: u32 = u32::MAX;

/// "No respawn is pending" — the value [`SeatTable::respawn_due`] carries for a
/// seat whose commander is alive.
pub const NO_RESPAWN: u32 = u32::MAX;

/// Per-seat treasury, power and match standing, structure-of-arrays.
///
/// The economy proper is T14's; what is here is the shape the hash and the
/// snapshot need from day one, so that adding the real columns is an ordinary
/// extension rather than a new table.
///
/// The last three columns are T10's, and two of them are **per-Push state**
/// (item 21): the commander's death counter and its pending respawn are reset
/// when a Push opens, so that a seat's fifth death in round 1 does not price
/// its first death in round 2. `eliminated_at` is not — elimination is for the
/// match.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SeatTable {
    count: u32,
    seat: Vec<u8>,
    treasury: Vec<Money>,
    supply: Vec<Kw>,
    draw: Vec<Kw>,
    eliminated_at: Vec<u32>,
    commander_deaths: Vec<u32>,
    respawn_due: Vec<u32>,
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
            eliminated_at: Vec::with_capacity(n),
            commander_deaths: Vec::with_capacity(n),
            respawn_due: Vec::with_capacity(n),
        }
    }

    /// Append one seat, alive and with no deaths behind it. Construction only.
    pub fn push(&mut self, seat: SeatId, treasury: Money, supply: Kw, draw: Kw) {
        self.seat.push(seat.raw());
        self.treasury.push(treasury);
        self.supply.push(supply);
        self.draw.push(draw);
        self.eliminated_at.push(NOT_ELIMINATED);
        self.commander_deaths.push(0);
        self.respawn_due.push(NO_RESPAWN);
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

    /// The tick each seat was eliminated at, or [`NOT_ELIMINATED`].
    #[must_use]
    pub fn eliminated_at(&self) -> &[u32] {
        &self.eliminated_at
    }

    /// Whether a seat is still in the match. An index the table does not cover
    /// answers `false`, which is the safe direction: a seat that is not there
    /// is not one of the two the one-tick rule counts.
    #[must_use]
    pub fn is_alive(&self, index: usize) -> bool {
        self.eliminated_at
            .get(index)
            .is_some_and(|at| *at == NOT_ELIMINATED)
    }

    /// How many times each seat's commander has died **in this Push**
    /// (item 21).
    #[must_use]
    pub fn commander_deaths(&self) -> &[u32] {
        &self.commander_deaths
    }

    /// The tick each seat's commander is due back at, or [`NO_RESPAWN`].
    #[must_use]
    pub fn respawn_due(&self) -> &[u32] {
        &self.respawn_due
    }

    /// The three match columns, to write into. [`crate::world`]'s match phase
    /// is the only caller.
    pub fn match_columns(&mut self) -> MatchColumns<'_> {
        MatchColumns {
            eliminated_at: &mut self.eliminated_at,
            commander_deaths: &mut self.commander_deaths,
            respawn_due: &mut self.respawn_due,
        }
    }

    /// Replace the whole table from a restored snapshot. Same contract as
    /// [`UnitTable::restore`].
    pub fn restore(&mut self, columns: SeatColumns) -> bool {
        let n = columns.seat.len();
        if columns.treasury.len() != n
            || columns.supply.len() != n
            || columns.draw.len() != n
            || columns.eliminated_at.len() != n
            || columns.commander_deaths.len() != n
            || columns.respawn_due.len() != n
        {
            return false;
        }
        let Ok(count) = u32::try_from(n) else {
            return false;
        };
        self.count = count;
        self.seat = columns.seat;
        self.treasury = columns.treasury;
        self.supply = columns.supply;
        self.draw = columns.draw;
        self.eliminated_at = columns.eliminated_at;
        self.commander_deaths = columns.commander_deaths;
        self.respawn_due = columns.respawn_due;
        true
    }
}

/// Every column of a restored [`SeatTable`]. Same reasoning as
/// [`UnitColumns`].
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct SeatColumns {
    /// Seat ids.
    pub seat: Vec<u8>,
    /// Treasuries.
    pub treasury: Vec<Money>,
    /// Power supplies.
    pub supply: Vec<Kw>,
    /// Power draws.
    pub draw: Vec<Kw>,
    /// Elimination ticks, or [`NOT_ELIMINATED`].
    pub eliminated_at: Vec<u32>,
    /// Commander deaths this Push.
    pub commander_deaths: Vec<u32>,
    /// Pending respawn ticks, or [`NO_RESPAWN`].
    pub respawn_due: Vec<u32>,
}

/// The three match columns, borrowed together.
///
/// One borrow rather than three accessors because the match phase writes all
/// three in the same loop — a seat that is eliminated stops counting deaths and
/// drops its pending respawn in one step.
#[derive(Debug)]
pub struct MatchColumns<'a> {
    /// Elimination ticks, or [`NOT_ELIMINATED`].
    pub eliminated_at: &'a mut [u32],
    /// Commander deaths this Push.
    pub commander_deaths: &'a mut [u32],
    /// Pending respawn ticks, or [`NO_RESPAWN`].
    pub respawn_due: &'a mut [u32],
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

// ---------------------------------------------------------------------------
// The world's tables beyond units and seats (T5)
// ---------------------------------------------------------------------------

/// What a unit is (spec sections 4, 7 and 9; item 90's per-kind rows).
///
/// The discriminants are written out in [`UnitKind::id`] rather than taken from
/// the enum's order, for the same reason [`crate::math::random::Stream::id`]'s
/// are: the id reaches the canonical encoding, so a reordering must not be able
/// to change a wire value by accident. Ids are additive only.
///
/// `RepairDrone` is item 90's "repair-reclaim drone" under the short name the
/// rules table's row already uses.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum UnitKind {
    /// The commander: one per occupied seat, the only thing that can change a
    /// sealed order.
    Commander,
    /// A build drone.
    BuildDrone,
    /// A mining drone.
    MiningDrone,
    /// A repair-reclaim drone (S2).
    RepairDrone,
    /// A raider (S2).
    Raider,
    /// A scout (S2).
    Scout,
}

impl UnitKind {
    /// Every kind, in ascending id order.
    pub const ALL: [UnitKind; 6] = [
        UnitKind::Commander,
        UnitKind::BuildDrone,
        UnitKind::MiningDrone,
        UnitKind::RepairDrone,
        UnitKind::Raider,
        UnitKind::Scout,
    ];

    /// The wire id. Additive only: never reuse, never renumber.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            UnitKind::Commander => 1,
            UnitKind::BuildDrone => 2,
            UnitKind::MiningDrone => 3,
            UnitKind::RepairDrone => 4,
            UnitKind::Raider => 5,
            UnitKind::Scout => 6,
        }
    }

    /// The kind an id names, or `None` for an id this build does not define.
    #[must_use]
    pub const fn from_id(id: u8) -> Option<UnitKind> {
        match id {
            1 => Some(UnitKind::Commander),
            2 => Some(UnitKind::BuildDrone),
            3 => Some(UnitKind::MiningDrone),
            4 => Some(UnitKind::RepairDrone),
            5 => Some(UnitKind::Raider),
            6 => Some(UnitKind::Scout),
            _ => None,
        }
    }
}

/// What a structure is (spec sections 7 and 9; item 90's `structures` rows).
///
/// A beacon is **not** here: beacons carry mandates and are their own table.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum StructureKind {
    /// A Generator, standing on a heat vent.
    Generator,
    /// An autocannon (S2).
    Autocannon,
    /// A mortar (S2).
    Mortar,
    /// A Survey post.
    SurveyPost,
    /// A Resonance Spire (S3).
    ResonanceSpire,
    /// A wall segment (S2), priced per voxel rather than per building.
    Wall,
}

impl StructureKind {
    /// Every kind, in ascending id order.
    pub const ALL: [StructureKind; 6] = [
        StructureKind::Generator,
        StructureKind::Autocannon,
        StructureKind::Mortar,
        StructureKind::SurveyPost,
        StructureKind::ResonanceSpire,
        StructureKind::Wall,
    ];

    /// The wire id. Additive only: never reuse, never renumber.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            StructureKind::Generator => 1,
            StructureKind::Autocannon => 2,
            StructureKind::Mortar => 3,
            StructureKind::SurveyPost => 4,
            StructureKind::ResonanceSpire => 5,
            StructureKind::Wall => 6,
        }
    }

    /// The kind an id names, or `None` for an id this build does not define.
    #[must_use]
    pub const fn from_id(id: u8) -> Option<StructureKind> {
        match id {
            1 => Some(StructureKind::Generator),
            2 => Some(StructureKind::Autocannon),
            3 => Some(StructureKind::Mortar),
            4 => Some(StructureKind::SurveyPost),
            5 => Some(StructureKind::ResonanceSpire),
            6 => Some(StructureKind::Wall),
            _ => None,
        }
    }
}

/// A beacon's identity. Dense, assigned at construction, stable for the match.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct BeaconId(u32);

impl BeaconId {
    /// "No beacon" — the sentinel a structure with no home carries.
    ///
    /// A sentinel rather than an `Option` because the column is snapshotted and
    /// hashed, and the snapshot's rule is that every field is fixed-width with
    /// no tag byte whose layout could differ between targets
    /// ([`crate::snapshot`]).
    pub const NONE: BeaconId = BeaconId(u32::MAX);

    /// Build from a raw id.
    #[must_use]
    pub const fn new(raw: u32) -> BeaconId {
        BeaconId(raw)
    }

    /// The raw id, for the canonical encoder and for sort keys.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// Whether this is a real beacon rather than [`BeaconId::NONE`].
    #[must_use]
    pub const fn is_some(self) -> bool {
        self.0 != BeaconId::NONE.0
    }

    /// The string a playbook's `beacon_id` names this beacon by: `b_` followed
    /// by the id in decimal, zero-padded to two digits — `b_00`, `b_01`,
    /// `b_42`, and `b_100` once a match ever holds more than a hundred beacons.
    ///
    /// The verifier resolves a playbook's beacon references against a list of
    /// these strings, and the gateway builds that list from this table, so the
    /// spelling is a contract between three crates and belongs next to the id
    /// rather than inside any one of them. Two digits because the world total
    /// is 40 beacons (item 63) and a fixed width sorts and reads well; the
    /// padding is a minimum, never a truncation.
    #[must_use]
    pub fn playbook_id(self) -> String {
        format!("b_{:02}", self.0)
    }
}

/// A structure's identity.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct StructureId(u32);

impl StructureId {
    /// Build from a raw id.
    #[must_use]
    pub const fn new(raw: u32) -> StructureId {
        StructureId(raw)
    }

    /// The raw id.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// A wreck's identity.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct WreckId(u32);

impl WreckId {
    /// Build from a raw id.
    #[must_use]
    pub const fn new(raw: u32) -> WreckId {
        WreckId(raw)
    }

    /// The raw id.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// Every column of a restored [`BeaconTable`]. Same reasoning as
/// [`UnitColumns`].
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct BeaconColumns {
    /// Beacon ids.
    pub id: Vec<u32>,
    /// The seat each beacon belongs to.
    pub seat: Vec<u8>,
    /// Positions.
    pub pos: Vec<[Fx; 3]>,
    /// Each beacon's [`crate::seams::MandateKind::id`].
    pub mandate: Vec<u8>,
    /// Each beacon's `program_id` seam.
    pub program: Vec<u32>,
    /// Hit points.
    pub hp: Vec<Hp>,
    /// Dormancy.
    pub dormant: Vec<bool>,
}

/// Every column of a restored [`StructureTable`]. Same reasoning as
/// [`UnitColumns`].
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct StructureColumns {
    /// Structure ids.
    pub id: Vec<u32>,
    /// The seat each structure belongs to.
    pub seat: Vec<u8>,
    /// Each structure's [`StructureKind::id`].
    pub kind: Vec<u8>,
    /// Positions.
    pub pos: Vec<[Fx; 3]>,
    /// Hit points.
    pub hp: Vec<Hp>,
    /// Home beacons, or [`BeaconId::NONE`].
    pub home: Vec<u32>,
}

/// Beacons, structure-of-arrays.
///
/// The unit of power (AGENTS.md section 1): every beacon carries a mandate and
/// a sphere of authority, and the pre-placed core is an ordinary row of this
/// table with `beacon.core_hp` hit points. `dormant` is the power grid's flag
/// (T14's brownout order); it is hashed from the day the column exists, because
/// a field that affects behaviour and is not hashed is a latent desync
/// (AGENTS.md section 4.8).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BeaconTable {
    count: u32,
    id: Vec<u32>,
    seat: Vec<u8>,
    pos: Vec<[Fx; 3]>,
    mandate: Vec<u8>,
    program: Vec<u32>,
    hp: Vec<Hp>,
    dormant: Vec<bool>,
}

impl BeaconTable {
    /// An empty table sized for `count` beacons. Nothing allocates after this.
    #[must_use]
    pub fn with_capacity(count: u32) -> BeaconTable {
        let n = usize::try_from(count).unwrap_or(0);
        BeaconTable {
            count: 0,
            id: Vec::with_capacity(n),
            seat: Vec::with_capacity(n),
            pos: Vec::with_capacity(n),
            mandate: Vec::with_capacity(n),
            program: Vec::with_capacity(n),
            hp: Vec::with_capacity(n),
            dormant: Vec::with_capacity(n),
        }
    }

    /// Append one beacon. Construction only — never called inside a tick.
    pub fn push(
        &mut self,
        id: BeaconId,
        seat: SeatId,
        pos: [Fx; 3],
        mandate: BeaconMandate,
        hp: Hp,
        dormant: bool,
    ) {
        self.id.push(id.raw());
        self.seat.push(seat.raw());
        self.pos.push(pos);
        self.mandate.push(mandate.kind.id());
        self.program.push(mandate.program_id.raw());
        self.hp.push(hp);
        self.dormant.push(dormant);
        self.count = self.count.saturating_add(1);
    }

    /// How many beacons the table holds.
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

    /// The mandate column, as [`crate::seams::MandateKind::id`] wire values.
    #[must_use]
    pub fn mandates(&self) -> &[u8] {
        &self.mandate
    }

    /// The `program_id` column — the reserved seam, read by nothing
    /// ([`crate::seams::ProgramId`]).
    #[must_use]
    pub fn programs(&self) -> &[u32] {
        &self.program
    }

    /// The hit-point column.
    #[must_use]
    pub fn hit_points(&self) -> &[Hp] {
        &self.hp
    }

    /// The dormancy column.
    #[must_use]
    pub fn dormant(&self) -> &[bool] {
        &self.dormant
    }

    /// The hit-point column, to write into.
    ///
    /// The combat phase's damage drain is the only caller. A beacon at zero
    /// hit points is dead, and item 20's local elimination follows in the same
    /// tick.
    pub fn hit_points_mut(&mut self) -> &mut [Hp] {
        &mut self.hp
    }

    /// Replace the whole table from a restored snapshot. Same contract as
    /// [`UnitTable::restore`].
    pub fn restore(&mut self, columns: BeaconColumns) -> bool {
        let n = columns.id.len();
        if columns.seat.len() != n
            || columns.pos.len() != n
            || columns.mandate.len() != n
            || columns.program.len() != n
            || columns.hp.len() != n
            || columns.dormant.len() != n
        {
            return false;
        }
        let Ok(count) = u32::try_from(n) else {
            return false;
        };
        self.count = count;
        self.id = columns.id;
        self.seat = columns.seat;
        self.pos = columns.pos;
        self.mandate = columns.mandate;
        self.program = columns.program;
        self.hp = columns.hp;
        self.dormant = columns.dormant;
        true
    }
}

/// Structures, structure-of-arrays.
///
/// Empty at the skeleton: the generator pre-places a core **beacon** per
/// occupied zone and nothing else, and the first Generator is built during a
/// Push (T14). The table exists now because adding a hashed table later is a
/// bigger change than filling one.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StructureTable {
    count: u32,
    id: Vec<u32>,
    seat: Vec<u8>,
    kind: Vec<u8>,
    pos: Vec<[Fx; 3]>,
    hp: Vec<Hp>,
    home: Vec<u32>,
}

impl StructureTable {
    /// An empty table sized for `count` structures.
    #[must_use]
    pub fn with_capacity(count: u32) -> StructureTable {
        let n = usize::try_from(count).unwrap_or(0);
        StructureTable {
            count: 0,
            id: Vec::with_capacity(n),
            seat: Vec::with_capacity(n),
            kind: Vec::with_capacity(n),
            pos: Vec::with_capacity(n),
            hp: Vec::with_capacity(n),
            home: Vec::with_capacity(n),
        }
    }

    /// Append one structure. Construction only.
    pub fn push(
        &mut self,
        id: StructureId,
        seat: SeatId,
        kind: StructureKind,
        pos: [Fx; 3],
        hp: Hp,
        home: BeaconId,
    ) {
        self.id.push(id.raw());
        self.seat.push(seat.raw());
        self.kind.push(kind.id());
        self.pos.push(pos);
        self.hp.push(hp);
        self.home.push(home.raw());
        self.count = self.count.saturating_add(1);
    }

    /// How many structures the table holds.
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

    /// The kind column, as [`StructureKind::id`] wire values.
    #[must_use]
    pub fn kinds(&self) -> &[u8] {
        &self.kind
    }

    /// The position column.
    #[must_use]
    pub fn positions(&self) -> &[[Fx; 3]] {
        &self.pos
    }

    /// The hit-point column.
    #[must_use]
    pub fn hit_points(&self) -> &[Hp] {
        &self.hp
    }

    /// The home-beacon column, as raw [`BeaconId`]s. [`BeaconId::NONE`] means
    /// the structure has no home beacon.
    #[must_use]
    pub fn homes(&self) -> &[u32] {
        &self.home
    }

    /// The hit-point column, to write into. The combat phase's damage drain.
    pub fn hit_points_mut(&mut self) -> &mut [Hp] {
        &mut self.hp
    }

    /// The owner column, to write into.
    ///
    /// One caller, one value: item 20's ruination writes [`SeatId::NEUTRAL`]
    /// when the structure's home beacon dies or its seat is eliminated. A ruin
    /// is inert — no supply, no draw, no capability, unrepairable, zero audit
    /// value — and having no owner is what makes it all of those at once,
    /// rather than five flags that could disagree.
    pub fn seats_mut(&mut self) -> &mut [u8] {
        &mut self.seat
    }

    /// Replace the whole table from a restored snapshot. Same contract as
    /// [`UnitTable::restore`].
    pub fn restore(&mut self, columns: StructureColumns) -> bool {
        let n = columns.id.len();
        if columns.seat.len() != n
            || columns.kind.len() != n
            || columns.pos.len() != n
            || columns.hp.len() != n
            || columns.home.len() != n
        {
            return false;
        }
        let Ok(count) = u32::try_from(n) else {
            return false;
        };
        self.count = count;
        self.id = columns.id;
        self.seat = columns.seat;
        self.kind = columns.kind;
        self.pos = columns.pos;
        self.hp = columns.hp;
        self.home = columns.home;
        true
    }
}

/// Wrecks, structure-of-arrays.
///
/// What a destroyed unit or structure leaves behind: a place and a salvage
/// value in `$`, which a reclaim drone converts at `economy.salvage_percent` of
/// build cost (S2). Empty at the skeleton, hashed from today.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WreckTable {
    count: u32,
    id: Vec<u32>,
    pos: Vec<[Fx; 3]>,
    salvage: Vec<Money>,
}

impl WreckTable {
    /// An empty table sized for `count` wrecks.
    #[must_use]
    pub fn with_capacity(count: u32) -> WreckTable {
        let n = usize::try_from(count).unwrap_or(0);
        WreckTable {
            count: 0,
            id: Vec::with_capacity(n),
            pos: Vec::with_capacity(n),
            salvage: Vec::with_capacity(n),
        }
    }

    /// Append one wreck. Construction only.
    pub fn push(&mut self, id: WreckId, pos: [Fx; 3], salvage: Money) {
        self.id.push(id.raw());
        self.pos.push(pos);
        self.salvage.push(salvage);
        self.count = self.count.saturating_add(1);
    }

    /// How many wrecks the table holds.
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

    /// The position column.
    #[must_use]
    pub fn positions(&self) -> &[[Fx; 3]] {
        &self.pos
    }

    /// The salvage-value column, in `$`.
    #[must_use]
    pub fn salvages(&self) -> &[Money] {
        &self.salvage
    }

    /// Replace the whole table from a restored snapshot. Same contract as
    /// [`UnitTable::restore`].
    pub fn restore(&mut self, id: Vec<u32>, pos: Vec<[Fx; 3]>, salvage: Vec<Money>) -> bool {
        let n = id.len();
        if pos.len() != n || salvage.len() != n {
            return false;
        }
        let Ok(count) = u32::try_from(n) else {
            return false;
        };
        self.count = count;
        self.id = id;
        self.pos = pos;
        self.salvage = salvage;
        true
    }
}
