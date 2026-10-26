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

/// A unit's identity: its row in the unit table and the row's generation
/// ([`crate::ids`], S2's plan, E1). A row is reused once its unit has died and
/// its death has settled, under the next generation, so an id names one unit
/// for the match and never the unit that later takes its row.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct UnitId(u32);

crate::ids::generational_id!(UnitId, "unit");

impl UnitId {
    /// "No unit" — the sentinel a per-seat index carries for a seat that has
    /// none. Same reasoning as [`BeaconId::NONE`]. Its [`UnitId::row`] is
    /// [`crate::ids::IdError::Sentinel`]: it stays in storage and never names
    /// a row.
    pub const NONE: UnitId = UnitId(u32::MAX);

    /// Build from a raw id, as the canonical encoder and a snapshot carry it.
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
/// them. The columns never grow inside a tick: the room is reserved at
/// construction and at a restore.
///
/// **Rows are reused** (S2's plan, E1). A dead unit keeps its row until its
/// death has settled; then [`UnitTable::release`] frees it, and the next unit
/// placed takes the lowest free row under the next [`crate::ids::Generation`]
/// ([`UnitTable::vacancy`], [`UnitTable::place`]). A freed row belongs to nobody:
/// its seat is [`SeatId::NEUTRAL`], it has no hit points and no home, and every
/// column is written afresh when it is reused. Every column added here must go
/// into *three* places in the same pull request — [`crate::world::World::encode`],
/// the snapshot round-trip and the golden files (AGENTS.md §4.8) — and into
/// [`UnitTable::place`] and [`UnitTable::release`].
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
    home: Vec<BeaconId>,
    carrying: Vec<Money>,
    busy_until: Vec<u32>,
    arms: Vec<Arms>,
    retreat: Vec<BeaconId>,
    free: Vec<bool>,
}

/// Why a row could not be placed in a table: it holds all the rows it may,
/// and none of them is free to reuse.
///
/// A typed answer, so the caller holds the order (a fabrication, a structure
/// paid for, a deploy) rather than growing a column inside a tick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TableFull {
    /// How many rows the table may hold.
    pub room: u32,
}

impl core::fmt::Display for TableFull {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "the table holds all {} of its rows and none is free",
            self.room
        )
    }
}

impl std::error::Error for TableFull {}

/// Where the next row of a table goes ([`UnitTable::vacancy`] and its
/// siblings): a free row reused under its next generation, or a fresh row on
/// the end at generation 0.
///
/// Its fields are read-only outside this module: a vacancy is only ever made
/// by the table it names a row of, so a `place` can never be handed a row past
/// the table's end.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Vacancy {
    row: crate::ids::Row,
    generation: crate::ids::Generation,
    reused: bool,
}

impl Vacancy {
    /// The row.
    #[must_use]
    pub const fn row(self) -> crate::ids::Row {
        self.row
    }

    /// The generation the asset placed in it will carry.
    #[must_use]
    pub const fn generation(self) -> crate::ids::Generation {
        self.generation
    }

    /// Whether the row is reused rather than appended.
    #[must_use]
    pub const fn reused(self) -> bool {
        self.reused
    }
}

/// An asset just placed in a table: its id, and whether it took a freed row
/// (which every derived per-row index must reset) or a fresh one on the end
/// (which every derived per-row index must grow by).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Placed<Id> {
    /// The new asset's id.
    pub id: Id,
    /// Whether the row was reused.
    pub reused: bool,
}

/// The lowest free row of a table whose generation can still go up, or the
/// next row on the end while the table holds fewer than `room` rows
/// (S2's plan, E1: "the lowest free row is reused first and its generation
/// goes up by one").
///
/// A free row at the last generation is never handed out again; it was
/// counted as retired when it was freed ([`crate::ids::IdDrop::Retired`]).
///
/// # Errors
///
/// [`TableFull`] when no row is free and the table is at its room.
fn vacancy_in(ids: &[u32], free: &[bool], room: u32) -> Result<Vacancy, TableFull> {
    for ((id, free), row) in ids.iter().zip(free).zip(crate::ids::Row::all()) {
        if !*free {
            continue;
        }
        let Ok((_, generation)) = crate::ids::unpack(*id) else {
            continue;
        };
        if let Some(next) = generation.next() {
            return Ok(Vacancy {
                row,
                generation: next,
                reused: true,
            });
        }
    }
    let full = TableFull { room };
    let len = u32::try_from(ids.len()).map_err(|_| full)?;
    if len >= room {
        return Err(full);
    }
    let row = crate::ids::Row::new(len).ok_or(full)?;
    Ok(Vacancy {
        row,
        generation: crate::ids::Generation::ZERO,
        reused: false,
    })
}

/// The row an id names in a table whose id and free columns are `ids` and
/// `free`, checked by generation: the reference is answered only while the
/// row still holds the asset it named.
///
/// # Errors
///
/// [`crate::ids::RefError`]: a sentinel, a row the table does not hold, or a
/// row since freed or reused ([`crate::ids::RefError::Stale`]).
fn row_of_in(
    ids: &[u32],
    free: &[bool],
    raw: u32,
) -> Result<crate::ids::Row, crate::ids::RefError> {
    let (row, _) = crate::ids::unpack(raw)?;
    let (Some(held), Some(freed)) = (ids.get(row.index()), free.get(row.index())) else {
        return Err(crate::ids::RefError::NoSuchRow);
    };
    if *held != raw || *freed {
        return Err(crate::ids::RefError::Stale);
    }
    Ok(row)
}

/// Whether every row's id names that row, as a restore checks: a file whose
/// ids disagree with their places describes no table this sim builds.
#[must_use]
pub fn ids_name_their_rows(ids: &[u32]) -> bool {
    ids.iter()
        .enumerate()
        .all(|(index, id)| crate::ids::unpack(*id).is_ok_and(|(row, _)| row.index() == index))
}

/// Write `value` at `row` of `column`, or append it when `row` is the
/// column's length: the one shape every table's `place` takes, so a reused row
/// and an appended one are written by the same line. A [`Vacancy`] names a
/// row of the column or the one after its end, never further.
fn put<T>(column: &mut Vec<T>, row: crate::ids::Row, value: T) {
    match column.get_mut(row.index()) {
        Some(slot) => *slot = value,
        None => column.push(value),
    }
}

impl UnitTable {
    /// An empty table sized for `count` units. Nothing allocates after this.
    #[must_use]
    pub fn with_capacity(count: u32) -> UnitTable {
        let n = crate::math::index::row(count);
        UnitTable {
            count: 0,
            id: Vec::with_capacity(n),
            seat: Vec::with_capacity(n),
            kind: Vec::with_capacity(n),
            pos: Vec::with_capacity(n),
            dest: Vec::with_capacity(n),
            heading: Vec::with_capacity(n),
            hp: Vec::with_capacity(n),
            home: Vec::with_capacity(n),
            carrying: Vec::with_capacity(n),
            busy_until: Vec::with_capacity(n),
            arms: Vec::with_capacity(n),
            retreat: Vec::with_capacity(n),
            free: Vec::with_capacity(n),
        }
    }

    /// Make room for `extra` more units without growing later.
    ///
    /// The unit table is the second table a **Push** adds rows to: a beacon's
    /// fabricator produces a drone while its mandate has outstanding work
    /// (item 22). The room is reserved at construction and at a restore for the
    /// reason [`BeaconTable::reserve`] gives — a `push` that grew a column would
    /// be an allocation inside a tick — and a fabrication order that finds none
    /// is held rather than filled. Construction only.
    pub fn reserve(&mut self, extra: u32) {
        let n = crate::math::index::row(extra);
        self.id.reserve(n);
        self.seat.reserve(n);
        self.kind.reserve(n);
        self.pos.reserve(n);
        self.dest.reserve(n);
        self.heading.reserve(n);
        self.hp.reserve(n);
        self.home.reserve(n);
        self.carrying.reserve(n);
        self.busy_until.reserve(n);
        self.arms.reserve(n);
        self.retreat.reserve(n);
        self.free.reserve(n);
    }

    /// Where the next unit goes ([`vacancy_in`]), within `room` rows.
    ///
    /// # Errors
    ///
    /// [`TableFull`] when no row is free and the table holds `room`.
    pub fn vacancy(&self, room: u32) -> Result<Vacancy, TableFull> {
        vacancy_in(&self.id, &self.free, room)
    }

    /// Place one unit in the next [`Vacancy`] within `room` rows, carrying
    /// nothing and doing nothing, with **every column written** -- a reused row
    /// keeps nothing of the unit that held it (S2's plan, E1).
    ///
    /// `home` is the beacon whose fabricator produced it — the one whose
    /// mandate it follows, even outside the sphere, and the one whose dormancy
    /// powers it down (spec section 5). [`BeaconId::NONE`] is harness
    /// scaffolding only: see [`UnitTable::homes`].
    #[allow(
        clippy::too_many_arguments,
        reason = "one argument per column of the SoA table, named at every call site; a struct here would be `UnitColumns` with a length of one, which is the shape `restore` already has for the bulk case"
    )]
    ///
    /// # Errors
    ///
    /// [`TableFull`] when no row is free and the table holds `room`.
    pub fn place(
        &mut self,
        room: u32,
        seat: SeatId,
        kind: UnitKind,
        pos: [Fx; 3],
        dest: [Fx; 3],
        hp: Hp,
        home: BeaconId,
        ready_at: u32,
    ) -> Result<Placed<UnitId>, TableFull> {
        let at = self.vacancy(room)?;
        let id = UnitId::of(at.row, at.generation);
        let row = at.row;
        put(&mut self.id, row, id.raw());
        put(&mut self.seat, row, seat.raw());
        put(&mut self.kind, row, kind.id());
        put(&mut self.pos, row, pos);
        put(&mut self.dest, row, dest);
        put(&mut self.heading, row, Angle::ZERO);
        put(&mut self.hp, row, hp);
        put(&mut self.home, row, home);
        put(&mut self.carrying, row, Money::ZERO);
        put(&mut self.busy_until, row, NO_WORK);
        put(&mut self.arms, row, Arms::ready_at(ready_at));
        put(&mut self.retreat, row, BeaconId::NONE);
        put(&mut self.free, row, false);
        if !at.reused {
            // An appended row is the table's last, at most `Row::MAX`, so the
            // count is its row plus one and cannot overflow.
            self.count = row.raw().saturating_add(1);
        }
        Ok(Placed {
            id,
            reused: at.reused,
        })
    }

    /// Append one unit at generation 0: what construction does, before any
    /// row can have been freed.
    ///
    /// # Errors
    ///
    /// [`TableFull`] past [`crate::ids::ROOM_LIMIT`] rows.
    pub fn push(
        &mut self,
        seat: SeatId,
        kind: UnitKind,
        pos: [Fx; 3],
        dest: [Fx; 3],
        hp: Hp,
        home: BeaconId,
    ) -> Result<UnitId, TableFull> {
        self.place(crate::ids::ROOM_LIMIT, seat, kind, pos, dest, hp, home, 0)
            .map(|placed| placed.id)
    }

    /// Free `row`: its unit has died and its death has settled. The row
    /// belongs to nobody until it is reused, so nothing that walks the table
    /// by seat, hit points or home reads it as anybody's. `true` when the row's
    /// generation is the last, so it will never be reused
    /// ([`crate::ids::IdDrop::Retired`]).
    pub fn release(&mut self, row: crate::ids::Row) -> bool {
        let index = row.index();
        let Some(free) = self.free.get_mut(index) else {
            return false;
        };
        *free = true;
        if let Some(slot) = self.seat.get_mut(index) {
            *slot = SeatId::NEUTRAL.raw();
        }
        if let Some(slot) = self.hp.get_mut(index) {
            *slot = Hp::ZERO;
        }
        if let Some(slot) = self.home.get_mut(index) {
            *slot = BeaconId::NONE;
        }
        if let Some(slot) = self.carrying.get_mut(index) {
            *slot = Money::ZERO;
        }
        if let Some(slot) = self.busy_until.get_mut(index) {
            *slot = NO_WORK;
        }
        if let Some(slot) = self.arms.get_mut(index) {
            *slot = Arms::NONE;
        }
        if let Some(slot) = self.retreat.get_mut(index) {
            *slot = BeaconId::NONE;
        }
        if let (Some(here), Some(slot)) = (self.pos.get(index).copied(), self.dest.get_mut(index)) {
            *slot = here;
        }
        self.id
            .get(index)
            .and_then(|id| crate::ids::unpack(*id).ok())
            .is_some_and(|(_, generation)| generation.next().is_none())
    }

    /// The row `unit` lives in, checked by generation.
    ///
    /// # Errors
    ///
    /// [`crate::ids::RefError`]: a sentinel, no such row, or a row since freed
    /// or reused.
    pub fn row_of(&self, unit: UnitId) -> Result<crate::ids::Row, crate::ids::RefError> {
        row_of_in(&self.id, &self.free, unit.raw())
    }

    /// The id of the unit in `row`, or `None` for a row the table does not
    /// hold or one that is free.
    #[must_use]
    pub fn id_of(&self, row: crate::ids::Row) -> Option<UnitId> {
        if self.free.get(row.index()).copied() != Some(false) {
            return None;
        }
        self.id.get(row.index()).copied().map(UnitId::new)
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

    /// The id column, as raw [`UnitId`]s: each row's own id, its generation
    /// included.
    #[must_use]
    pub fn ids(&self) -> &[u32] {
        &self.id
    }

    /// The free column: `true` on a row whose unit has died, settled and been
    /// freed, waiting to be reused (S2's plan, E1).
    #[must_use]
    pub fn frees(&self) -> &[bool] {
        &self.free
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

    /// The home-beacon column.
    ///
    /// Every unit a fabricator produces is bound to the beacon that produced it
    /// and follows that beacon's mandate, even outside the sphere (spec section
    /// 5, "Units"). [`BeaconId::NONE`] means the unit is on no grid at all: it
    /// draws no `kW`, no mandate drives it and no dormancy parks it. Nothing in
    /// a real match produces one — the value exists for the harness walkers
    /// [`crate::world::WorldConfig::units_per_seat`] fields, which are
    /// scaffolding rather than a seat's force, and for a freed row. A home is a
    /// reference into the beacon table, read through
    /// [`BeaconTable::row_of`], which refuses the sentinel and a beacon since
    /// freed.
    #[must_use]
    pub fn homes(&self) -> &[BeaconId] {
        &self.home
    }

    /// The home of the unit in `row`, or `None` for a unit homed to nobody
    /// (or a row the table does not hold).
    #[must_use]
    pub fn home(&self, row: crate::ids::Row) -> Option<BeaconId> {
        self.home
            .get(row.index())
            .copied()
            .filter(|home| home.is_some())
    }

    /// The home-beacon column, to write into.
    ///
    /// Two callers, both in the match phase: item 20's re-homing when a bound
    /// beacon dies or is recycled, and the fabricator giving a fresh unit its
    /// home.
    pub fn homes_mut(&mut self) -> &mut [BeaconId] {
        &mut self.home
    }

    /// The carried-`$` column: ore a mining drone has dug and not yet
    /// delivered, and salvage a reclaim drone is carrying.
    ///
    /// Hashed state, because "delivered ore is credited to the seat treasury
    /// immediately" (spec section 6, Mine) makes *where* the `$` is a fact about
    /// the world: a drone killed on the way home loses its load.
    #[must_use]
    pub fn carrying(&self) -> &[Money] {
        &self.carrying
    }

    /// The carried-`$` column, to write into. The mine and salvage programs.
    pub fn carrying_mut(&mut self) -> &mut [Money] {
        &mut self.carrying
    }

    /// The tick each unit's timed work finishes at, or [`NO_WORK`] when it is
    /// doing none.
    ///
    /// One column for every program's clock — a mining drone's
    /// `economy.mining_ms_per_voxel`, a reclaim drone's salvage — because a
    /// unit does one timed thing at a time and a second column would be a second
    /// chance for the two to disagree.
    #[must_use]
    pub fn busy_until(&self) -> &[u32] {
        &self.busy_until
    }

    /// The busy-until column, to write into. The programs phase.
    pub fn busy_until_mut(&mut self) -> &mut [u32] {
        &mut self.busy_until
    }

    /// The combat memory column ([`Arms`]; S2's plan, E12).
    #[must_use]
    pub fn arms(&self) -> &[Arms] {
        &self.arms
    }

    /// The combat memory column, to write into: the combat phase.
    pub fn arms_mut(&mut self) -> &mut [Arms] {
        &mut self.arms
    }

    /// The retreat column (decision 4; S2's plan, E12): the beacon a unit is
    /// retreating to, or [`BeaconId::NONE`] when it is not retreating.
    #[must_use]
    pub fn retreats(&self) -> &[BeaconId] {
        &self.retreat
    }

    /// The retreat column, to write into: the combat phase starts a retreat
    /// and the programs phase ends one.
    pub fn retreats_mut(&mut self) -> &mut [BeaconId] {
        &mut self.retreat
    }

    /// Replace the whole table from the columns of a restored snapshot.
    ///
    /// Returns `false` and changes nothing when the columns disagree in
    /// length, or when a row's id does not name that row, which is what a
    /// corrupt or truncated snapshot looks like.
    pub fn restore(&mut self, columns: UnitColumns) -> bool {
        let n = columns.id.len();
        if columns.seat.len() != n
            || columns.kind.len() != n
            || columns.pos.len() != n
            || columns.dest.len() != n
            || columns.heading.len() != n
            || columns.hp.len() != n
            || columns.home.len() != n
            || columns.carrying.len() != n
            || columns.busy_until.len() != n
            || columns.arms.len() != n
            || columns.retreat.len() != n
            || columns.free.len() != n
            || !ids_name_their_rows(&columns.id)
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
        self.home = columns.home;
        self.carrying = columns.carrying;
        self.busy_until = columns.busy_until;
        self.arms = columns.arms;
        self.retreat = columns.retreat;
        self.free = columns.free;
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
    /// Home beacons, or [`BeaconId::NONE`].
    pub home: Vec<BeaconId>,
    /// Carried but undelivered `$`.
    pub carrying: Vec<Money>,
    /// The tick each unit's timed work finishes at, or [`NO_WORK`].
    pub busy_until: Vec<u32>,
    /// Each unit's combat memory ([`Arms`]).
    pub arms: Vec<Arms>,
    /// The beacon each unit is retreating to, or [`BeaconId::NONE`].
    pub retreat: Vec<BeaconId>,
    /// Whether each row is free (S2's plan, E1).
    pub free: Vec<bool>,
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

/// "This unit is doing no timed work" — the value [`UnitTable::busy_until`]
/// carries for a unit that is walking, idle or parked.
///
/// A sentinel rather than an `Option`, for the reason [`BeaconId::NONE`] is
/// one, and not a tick any match reaches.
pub const NO_WORK: u32 = u32::MAX;

/// "No attacker remembered" -- the value [`Arms::attacker`] carries for an
/// asset no enemy has damaged. `u32::MAX` is tag 15 of an
/// [`crate::knowledge::AssetId`], which names no kind, so no asset holds it.
pub const NO_ATTACKER: u32 = u32::MAX;

/// "Never attacked" -- the value [`Arms::attacked_at`] carries beside
/// [`NO_ATTACKER`]. Not a tick any match reaches.
pub const NEVER_ATTACKED: u32 = u32::MAX;

/// One asset's combat memory (S2's plan, E12): when its weapon may fire next,
/// and the last enemy that damaged it, with the tick. **Hashed, snapshotted
/// and in the goldens**, a column of the unit, structure and beacon tables,
/// reset when a row is reused (E1).
///
/// * `ready_at` is the first tick the asset's weapon may fire on: a shot sets
///   it to the shot's tick plus the weapon's cooldown in ticks, so a cooldown
///   is game milliseconds turned into ticks (AGENTS.md section 4.5). A
///   structure or a unit is placed with it at its placement tick plus that
///   cooldown, so no row fires twice inside one cooldown, a reused row
///   included (the shell room's bound, E13). An unarmed asset's is never read.
/// * `attacker` is the last enemy asset that damaged it, as an
///   [`crate::knowledge::AssetId`] raw value with its generation, or
///   [`NO_ATTACKER`]; for a shell it is the shell's firer. `RETURN_FIRE` and
///   retreat read it (decision 4).
/// * `attacked_at` is the tick of that damage, or [`NEVER_ATTACKED`].
/// * `engaged` is the target its last `RETURN_FIRE` shot answered, or
///   [`NO_ATTACKER`]: a shot at a different target opens a new engagement,
///   which is announced (`returned_fire`, decision 4), and a shot at the same
///   one is not, so return fire is announced once per engagement and never
///   once per shot (E15).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Arms {
    /// The first tick the weapon may fire on.
    pub ready_at: u32,
    /// The last enemy that damaged it, or [`NO_ATTACKER`].
    pub attacker: u32,
    /// When, or [`NEVER_ATTACKED`].
    pub attacked_at: u32,
    /// The target its last return-fire shot answered, or [`NO_ATTACKER`].
    pub engaged: u32,
}

impl Arms {
    /// A fresh row's memory: ready at `ready_at`, attacked by nobody.
    #[must_use]
    pub const fn ready_at(ready_at: u32) -> Arms {
        Arms {
            ready_at,
            attacker: NO_ATTACKER,
            attacked_at: NEVER_ATTACKED,
            engaged: NO_ATTACKER,
        }
    }

    /// A freed row's memory: nothing.
    pub const NONE: Arms = Arms::ready_at(0);
}

/// A beacon's Common settings (spec section 6's Common row): the rules of
/// engagement and the retreat threshold every armed asset and unit homed to it
/// follows (decision 4). **Hashed, snapshotted and in the goldens**, written by
/// a committed settings row that names them and reset by a mandate switch
/// (spec section 5: switching clears the old settings).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Common {
    /// `MandateSettings.roe`'s wire value: 0 unset, which reads as
    /// `RETURN_FIRE` (decision 4), 1 `HOLD_FIRE`, 2 `RETURN_FIRE`, 3
    /// `FREE_FIRE`.
    pub roe: u8,
    /// `MandateSettings.retreat_hp_pct`, 0 to 100: a unit below this
    /// percentage of its hit points retreats. Zero never retreats.
    pub retreat_hp_pct: u8,
}

impl Common {
    /// The defaults a beacon is placed with and a switch leaves: an unset
    /// `roe` and no retreat.
    pub const DEFAULT: Common = Common {
        roe: 0,
        retreat_hp_pct: 0,
    };
}

/// One seat's tally for the match (S2's plan, E15): the enemy value it
/// destroyed, its kills, the kills it shared and its losses. **Hashed,
/// snapshotted and in the goldens**, accumulated as deaths settle and never
/// read off the event bus, which drops past its capacity.
///
/// * `destroyed` is the seat's settled share of the build cost of every enemy
///   asset destroyed (largest remainder, spec section 3), the final audit's
///   enemy-value term (decision 7).
/// * `kills` counts enemy assets destroyed with a share of their credit going
///   to the seat; `shared_kills` those of them whose credit was split with
///   another seat (spec section 3's "shared kill").
/// * `losses` counts the seat's own assets destroyed, whatever destroyed them:
///   a unit or structure killed, a beacon destroyed or recycled, and a
///   structure left as a ruin (decisions-log item 145 (5)). The commander is
///   not an asset and is counted in none of these (`gateway.proto`'s `Tally`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Tally {
    /// Enemy value destroyed, in `$`.
    pub destroyed: Money,
    /// Enemy assets destroyed with a share of their credit.
    pub kills: u32,
    /// Of `kills`, those whose credit was split with another seat.
    pub shared_kills: u32,
    /// Own assets destroyed.
    pub losses: u32,
}

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
    qm_cursor: Vec<u32>,
    placed: Vec<u32>,
    beacons_lost: Vec<u32>,
    tally: Vec<Tally>,
    attack: Vec<Arms>,
}

impl SeatTable {
    /// An empty table sized for `count` seats.
    #[must_use]
    pub fn with_capacity(count: u32) -> SeatTable {
        let n = crate::math::index::row(count);
        SeatTable {
            count: 0,
            seat: Vec::with_capacity(n),
            treasury: Vec::with_capacity(n),
            supply: Vec::with_capacity(n),
            draw: Vec::with_capacity(n),
            eliminated_at: Vec::with_capacity(n),
            commander_deaths: Vec::with_capacity(n),
            respawn_due: Vec::with_capacity(n),
            qm_cursor: Vec::with_capacity(n),
            placed: Vec::with_capacity(n),
            beacons_lost: Vec::with_capacity(n),
            tally: Vec::with_capacity(n),
            attack: Vec::with_capacity(n),
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
        self.qm_cursor.push(BeaconId::NONE.raw());
        self.placed.push(0);
        self.beacons_lost.push(0);
        self.tally.push(Tally::default());
        self.attack.push(Arms::NONE);
        self.count = self.count.saturating_add(1);
    }

    /// Each seat's **placement counter**: how many beacons it has placed this
    /// match, its core included, which is the ordinal its next beacon takes
    /// (S2's plan, E1). Hashed and snapshotted: since rows are reused, a
    /// beacon's `b_NN` is the counter's value when it was placed, never a
    /// count of the rows the seat holds, so a name is never handed out twice
    /// and a dead `b_03` stays gone rather than naming the next beacon.
    #[must_use]
    pub fn placed(&self) -> &[u32] {
        &self.placed
    }

    /// Take the next ordinal of the seat at `index` and count the placement.
    /// `None` for a seat the table does not hold, or a counter at its last
    /// value, which no match reaches at the room the beacon table has.
    pub fn take_ordinal(&mut self, index: usize) -> Option<u32> {
        let slot = self.placed.get_mut(index)?;
        let ordinal = *slot;
        *slot = ordinal.checked_add(1)?;
        Some(ordinal)
    }

    /// Set the seat at `index`'s placement counter: construction only, from
    /// the beacons the generator placed.
    pub fn set_placed(&mut self, index: usize, placed: u32) {
        if let Some(slot) = self.placed.get_mut(index) {
            *slot = placed;
        }
    }

    /// How many of each seat's beacons have died this match (X-08's "beacons
    /// lost", the final audit's third term). A per-seat hashed counter since
    /// S2's `ids` lane: a dead beacon's row is freed and reused, so the count
    /// can no longer be read off the beacon table.
    #[must_use]
    pub fn beacons_lost(&self) -> &[u32] {
        &self.beacons_lost
    }

    /// Count one beacon of the seat at `index` lost. Saturates by rule: a
    /// match holds at most the beacon room's worth of placements, far below
    /// `u32::MAX`, so the cap is never met.
    pub fn count_beacon_lost(&mut self, index: usize) {
        if let Some(slot) = self.beacons_lost.get_mut(index) {
            *slot = slot.saturating_add(1);
        }
    }

    /// Each seat's tally for the match ([`Tally`]; S2's plan, E15).
    #[must_use]
    pub fn tallies(&self) -> &[Tally] {
        &self.tally
    }

    /// The tally column, to write into: the settle of a tick's deaths.
    pub fn tallies_mut(&mut self) -> &mut [Tally] {
        &mut self.tally
    }

    /// Each seat's memory of the last enemy attack on any of its assets
    /// ([`Arms::attacker`] and [`Arms::attacked_at`]; the rest unused): what
    /// `RETURN_FIRE` answers when the asset attacked died (decision 4).
    #[must_use]
    pub fn attacks(&self) -> &[Arms] {
        &self.attack
    }

    /// The attack column, to write into: the combat phase.
    pub fn attacks_mut(&mut self) -> &mut [Arms] {
        &mut self.attack
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

    /// The treasury column, to write into.
    ///
    /// The single treasury (spec section 7): the Quartermaster's spend, the
    /// recycle refund, delivered ore and salvage, and the Ledger's settlement
    /// at each recap. There are no per-beacon pools and no sweeps, so there is
    /// one column and one writer.
    pub fn treasuries_mut(&mut self) -> &mut [Money] {
        &mut self.treasury
    }

    /// The supply and draw columns, borrowed together.
    ///
    /// One borrow rather than two accessors because the power phase settles
    /// both in the same pass: a seat's headroom is the difference, and a phase
    /// that could write one without the other is a phase that can leave them
    /// describing two different ticks.
    pub fn power_columns(&mut self) -> PowerColumns<'_> {
        PowerColumns {
            supply: &mut self.supply,
            draw: &mut self.draw,
        }
    }

    /// Where each seat's Quartermaster left off in its round-robin, as a
    /// [`BeaconId`] raw value, or [`BeaconId::NONE`] before it has served
    /// anybody.
    ///
    /// Hashed state, and not a cache: it is what decides which beacon of a band
    /// is served first on the next tick, so two machines that disagree about it
    /// spend a seat's `$` on different beacons (spec section 7: "round-robin
    /// within a band, so no single beacon can hog the treasury").
    #[must_use]
    pub fn qm_cursors(&self) -> &[u32] {
        &self.qm_cursor
    }

    /// The Quartermaster's cursor column, to write into.
    pub fn qm_cursors_mut(&mut self) -> &mut [u32] {
        &mut self.qm_cursor
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
            || columns.qm_cursor.len() != n
            || columns.placed.len() != n
            || columns.beacons_lost.len() != n
            || columns.tally.len() != n
            || columns.attack.len() != n
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
        self.qm_cursor = columns.qm_cursor;
        self.placed = columns.placed;
        self.beacons_lost = columns.beacons_lost;
        self.tally = columns.tally;
        self.attack = columns.attack;
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
    /// The Quartermaster's round-robin cursor, per seat.
    pub qm_cursor: Vec<u32>,
    /// The placement counter, per seat ([`SeatTable::placed`]).
    pub placed: Vec<u32>,
    /// Beacons lost, per seat ([`SeatTable::beacons_lost`]).
    pub beacons_lost: Vec<u32>,
    /// The tally, per seat ([`SeatTable::tallies`]).
    pub tally: Vec<Tally>,
    /// The last attack, per seat ([`SeatTable::attacks`]).
    pub attack: Vec<Arms>,
}

/// The supply and draw columns, borrowed together.
#[derive(Debug)]
pub struct PowerColumns<'a> {
    /// Power supply, per seat.
    pub supply: &'a mut [Kw],
    /// Power draw, per seat.
    pub draw: &'a mut [Kw],
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

/// A target row's wall top when the row is no wall ([`TargetTable::wall_tops`]):
/// below every voxel, so no comparison with a height can read it as one.
pub const NO_WALL: i16 = i16::MIN;

/// What one row of a [`TargetTable`] is.
///
/// The ids are written out and additive only, for the reason every other wire
/// id in this crate is: the byte reaches the canonical encoding.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum TargetKind {
    /// One entry of a Build mandate's target list: a blueprint at an anchor
    /// (spec section 6, the Build row).
    Build,
    /// Ground a Build mandate will not dig, fill or build on.
    Protected,
    /// Where a Survey mandate's scouts look first.
    Probe,
}

impl TargetKind {
    /// Every kind, in ascending id order.
    pub const ALL: [TargetKind; 3] = [TargetKind::Build, TargetKind::Protected, TargetKind::Probe];

    /// The wire id. Additive only: never reuse, never renumber.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            TargetKind::Build => 1,
            TargetKind::Protected => 2,
            TargetKind::Probe => 3,
        }
    }

    /// The kind an id names, or `None` for one this build does not define.
    #[must_use]
    pub const fn from_id(id: u8) -> Option<TargetKind> {
        match id {
            1 => Some(TargetKind::Build),
            2 => Some(TargetKind::Protected),
            3 => Some(TargetKind::Probe),
            _ => None,
        }
    }
}

/// Quarter turns about the vertical axis, 0 to 3: a Build target's
/// `rotation_quarter_turns` and the facing of the structure built for it.
///
/// An integer because the sim is integer-only: there are no free rotations and
/// no angles in a playbook (AGENTS.md section 4.2). The type cannot hold a
/// fourth turn, so a value past 3 is refused where it is read
/// ([`QuarterTurns::new`]) rather than wrapped.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct QuarterTurns(u8);

impl QuarterTurns {
    /// No turn: the facing a structure has when nothing chose one.
    pub const NONE: QuarterTurns = QuarterTurns(0);

    /// `turns` quarter turns, or `None` past three.
    #[must_use]
    pub const fn new(turns: u32) -> Option<QuarterTurns> {
        match turns {
            0 => Some(QuarterTurns(0)),
            1 => Some(QuarterTurns(1)),
            2 => Some(QuarterTurns(2)),
            3 => Some(QuarterTurns(3)),
            _ => None,
        }
    }

    /// The count, 0 to 3, as the canonical encoding writes it.
    #[must_use]
    pub const fn raw(self) -> u8 {
        self.0
    }
}

/// Where a Build target sits in its beacon's build order, and how the
/// structure built for it faces (spec section 6's Build row: "blueprint,
/// anchor, rotation, order"; S1's plan, decision 5).
///
/// The mandate builds the highest-order affordable target first: the lowest
/// `order`, ties by list position (`playbook.proto`'s `BuildTarget.order`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct BuildOrder {
    /// `BuildTarget.order`: lower builds first.
    pub order: u32,
    /// `BuildTarget.rotation_quarter_turns`.
    pub rotation: QuarterTurns,
}

impl BuildOrder {
    /// Order 0, no turn: what a protected area, a probe area and a fixture's
    /// target carry.
    pub const NONE: BuildOrder = BuildOrder {
        order: 0,
        rotation: QuarterTurns::NONE,
    };
}

/// An area's footprint on the ground: every column from `min` to `max` in x
/// and y, **both ends inclusive** -- the box a playbook's `Area` writes
/// (`playbook.proto`: "a box is what the editor's drag-select produces and
/// what an integer sim can test in three comparisons"), less its height.
///
/// A protected area and a probe area are tested on the **column**, whatever
/// the height: a Build target stands on its column and a scout walks on the
/// ground, so the box's `z` range decides nothing here (it only sets the
/// height of the centre a scout walks to). The test is the box itself, four
/// integer comparisons, so a column one step outside the box the author drew
/// is outside the area (S1's `build` lane, from its review: a covering disc
/// had protected ground the author never drew).
///
/// The type cannot hold an inside-out box: [`ColumnBox::new`] refuses one, as
/// the verifier's E0406 does at plan time.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct ColumnBox {
    min: [i16; 2],
    max: [i16; 2],
}

impl ColumnBox {
    /// The columns from `min` to `max`, or `None` when `min` exceeds `max` on
    /// either axis.
    #[must_use]
    pub const fn new(min: [i16; 2], max: [i16; 2]) -> Option<ColumnBox> {
        let [lx, ly] = min;
        let [hx, hy] = max;
        if lx > hx || ly > hy {
            return None;
        }
        Some(ColumnBox { min, max })
    }

    /// The lowest column, `[x, y]`.
    #[must_use]
    pub const fn min_column(self) -> [i16; 2] {
        self.min
    }

    /// The highest column, `[x, y]`.
    #[must_use]
    pub const fn max_column(self) -> [i16; 2] {
        self.max
    }

    /// Whether the column `(x, y)` lies inside the box, edges included.
    #[must_use]
    pub fn contains(self, x: i32, y: i32) -> bool {
        let [lx, ly] = self.min;
        let [hx, hy] = self.max;
        i32::from(lx) <= x && x <= i32::from(hx) && i32::from(ly) <= y && y <= i32::from(hy)
    }
}

/// Which list an area row belongs to: the two [`TargetKind`]s that are areas
/// rather than points.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum AreaKind {
    /// [`TargetKind::Protected`].
    Protected,
    /// [`TargetKind::Probe`].
    Probe,
}

impl AreaKind {
    /// The table kind the row is written under.
    #[must_use]
    pub const fn kind(self) -> TargetKind {
        match self {
            AreaKind::Protected => TargetKind::Protected,
            AreaKind::Probe => TargetKind::Probe,
        }
    }
}

/// Why a row could not go into the [`TargetTable`]: it already holds as many
/// rows as it was built for, and it never grows a column inside a tick.
///
/// A typed answer rather than a `false` (decisions-log item 136 (5)), so a
/// settings row that meets it fails its step with
/// [`crate::interpreter::StepFailure::TargetsUnwritten`] rather than dropping
/// the entry it could not write.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TargetsFull {
    /// How many rows the table holds, which is all it was built for.
    pub capacity: u32,
}

impl core::fmt::Display for TargetsFull {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "the list-settings table is full at {} rows",
            self.capacity
        )
    }
}

impl std::error::Error for TargetsFull {}

/// A beacon's mandate settings that are **lists**: Build targets, protected
/// areas and Survey probe areas.
///
/// One table rather than three, sorted by `(beacon, kind)`, because all three
/// are the same shape — a place, a footprint and a blueprint — and because a
/// mandate switch clears all of one beacon's rows at once (item 20).
///
/// Hashed state: a target decides what a fabricator pays for and where a build
/// drone walks.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TargetTable {
    count: u32,
    beacon: Vec<BeaconId>,
    kind: Vec<u8>,
    blueprint: Vec<u8>,
    at: Vec<[Fx; 3]>,
    area: Vec<ColumnBox>,
    built: Vec<StructureId>,
    feature: Vec<u32>,
    desc: Vec<u8>,
    order: Vec<u32>,
    rotation: Vec<u8>,
    wall_top: Vec<i16>,
    wall_done: Vec<bool>,
    capacity: u32,
}

impl TargetTable {
    /// How many rows the table holds, as a column index: the length of its
    /// columns, which every writer keeps equal, so a walk over `0..row_count()`
    /// needs no conversion and no fallback.
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.beacon.len()
    }

    /// A table holding `capacity` rows across the whole match, never growing.
    #[must_use]
    pub fn with_capacity(capacity: u32) -> TargetTable {
        let n = crate::math::index::row(capacity);
        TargetTable {
            count: 0,
            beacon: Vec::with_capacity(n),
            kind: Vec::with_capacity(n),
            blueprint: Vec::with_capacity(n),
            at: Vec::with_capacity(n),
            area: Vec::with_capacity(n),
            built: Vec::with_capacity(n),
            feature: Vec::with_capacity(n),
            desc: Vec::with_capacity(n),
            order: Vec::with_capacity(n),
            rotation: Vec::with_capacity(n),
            wall_top: Vec::with_capacity(n),
            wall_done: Vec::with_capacity(n),
            capacity,
        }
    }

    /// How many rows the table holds.
    #[must_use]
    pub const fn len(&self) -> u32 {
        self.count
    }

    /// Whether no beacon carries a list setting.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// How many rows it will ever hold.
    #[must_use]
    pub const fn capacity(&self) -> u32 {
        self.capacity
    }

    /// The beacon column: the beacon each row belongs to, a reference into the
    /// beacon table. A beacon's rows are cleared when it dies (item 20), so a
    /// row never outlives its beacon's row.
    #[must_use]
    pub fn beacons(&self) -> &[BeaconId] {
        &self.beacon
    }

    /// The kind column, as [`TargetKind::id`] wire values.
    #[must_use]
    pub fn kinds(&self) -> &[u8] {
        &self.kind
    }

    /// The blueprint column, as [`StructureKind::id`] wire values; zero on a
    /// row that names no blueprint.
    #[must_use]
    pub fn blueprints(&self) -> &[u8] {
        &self.blueprint
    }

    /// The anchor column: a Build target's place, or an area's centre (the
    /// point a scout walks to).
    #[must_use]
    pub fn anchors(&self) -> &[[Fx; 3]] {
        &self.at
    }

    /// The footprint column: an area row's [`ColumnBox`], the one test of
    /// whether a column is inside it; [`ColumnBox::default`] on a Build row,
    /// which is a point and has none.
    #[must_use]
    pub fn areas(&self) -> &[ColumnBox] {
        &self.area
    }

    /// The structure realising each Build target, or [`StructureId::NONE`]
    /// when nothing has been paid for yet.
    ///
    /// A reference into the structure table, read through
    /// [`StructureTable::row_of`], which checks the generation: a link to a
    /// structure since freed and its row reused is stale, and the target is
    /// read as not going up (S2's plan, E1).
    #[must_use]
    pub fn built(&self) -> &[StructureId] {
        &self.built
    }

    /// The realising-structure column, to write into: the Quartermaster sets
    /// it when it pays for the target, and item 20's ruination clears it.
    pub fn built_mut(&mut self) -> &mut [StructureId] {
        &mut self.built
    }

    /// The anchor column, to write into: the one writer is a held
    /// description's re-read, which moves an unbuilt target onto the vent it
    /// now names (`docs/design/targeting.md`, "Three reading rules", 2).
    pub fn anchors_mut(&mut self) -> &mut [[Fx; 3]] {
        &mut self.at
    }

    /// The feature each Build target is bound to, as an index into the map's
    /// feature table ([`crate::features::FeatureTable`]), or
    /// [`crate::targeting::NO_FEATURE`] for a target written as a fixed voxel
    /// and for every non-Build row.
    ///
    /// Hashed: a bound target idles or reads again when its feature is lost,
    /// and **Build targets made through `on` are keyed by it** -- a removal
    /// names the feature, and a settings edit carries a target over by it
    /// (targeting.md, "Sites").
    #[must_use]
    pub fn features(&self) -> &[u32] {
        &self.feature
    }

    /// The bound-feature column, to write into. A held description's re-read
    /// is the one writer.
    pub fn features_mut(&mut self) -> &mut [u32] {
        &mut self.feature
    }

    /// How each Build target was written: [`crate::targeting::DESCRIPTION_VOXEL`],
    /// [`crate::targeting::DESCRIPTION_NAME`] or
    /// [`crate::targeting::DESCRIPTION_NEAREST_VENT`]. Hashed, because it
    /// decides whether a lost target idles or reads again.
    #[must_use]
    pub fn descriptions(&self) -> &[u8] {
        &self.desc
    }

    /// Each Build target's place in its beacon's build order
    /// ([`BuildOrder::order`]); zero on every other row. Hashed: it decides
    /// which target the mandate pays for first.
    #[must_use]
    pub fn orders(&self) -> &[u32] {
        &self.order
    }

    /// Each Build target's rotation, as [`QuarterTurns::raw`]; zero on every
    /// other row. Hashed: the structure built for the target takes it.
    #[must_use]
    pub fn rotations(&self) -> &[u8] {
        &self.rotation
    }

    /// Each wall target's top, the highest voxel its box fills (S2's
    /// `site`), or [`NO_WALL`] on every other row. A wall row's box of
    /// columns is its [`TargetTable::areas`] entry. Hashed: it decides how far
    /// a build drone fills.
    #[must_use]
    pub fn wall_tops(&self) -> &[i16] {
        &self.wall_top
    }

    /// Write row `slot` as a wall: its box of columns and its top. The row was
    /// added by [`TargetTable::add_bound`] a moment before, so it is a Build
    /// row; `false` for a slot the table does not hold.
    pub fn make_wall(&mut self, slot: usize, columns: ColumnBox, top: i16) -> bool {
        let (Some(area), Some(wall)) = (self.area.get_mut(slot), self.wall_top.get_mut(slot))
        else {
            return false;
        };
        *area = columns;
        *wall = top;
        if let Some(done) = self.wall_done.get_mut(slot) {
            *done = false;
        }
        true
    }

    /// Whether each wall target's box has been filled whole once (S2's
    /// `site`; the owner's ruling of 2026-10-20): from then on a voxel blown
    /// out of it is a destroyed part of the wall, rebuilt only under
    /// `rebuild_destroyed`. `false` on every other row. Hashed.
    #[must_use]
    pub fn walls_done(&self) -> &[bool] {
        &self.wall_done
    }

    /// Mark row `slot`'s wall filled whole once.
    pub fn mark_wall_done(&mut self, slot: usize) {
        if let Some(done) = self.wall_done.get_mut(slot) {
            *done = true;
        }
    }

    /// Append one Build target written as a fixed voxel, keeping the table in
    /// `(beacon, kind)` order, at order 0 with no turn ([`BuildOrder::NONE`]).
    ///
    /// # Errors
    ///
    /// [`TargetsFull`] when the table is full: the table never grows a column
    /// inside a tick.
    pub fn add(&mut self, beacon: BeaconId, blueprint: u8, at: [Fx; 3]) -> Result<(), TargetsFull> {
        self.add_bound(
            beacon,
            TargetKind::Build,
            blueprint,
            at,
            ColumnBox::default(),
            (u32::MAX, crate::targeting::DESCRIPTION_VOXEL),
            BuildOrder::NONE,
        )
    }

    /// Append one area row -- a protected area or a probe area -- with its
    /// `centre` and its footprint, keeping the table in `(beacon, kind)` order.
    ///
    /// # Errors
    ///
    /// [`TargetsFull`], for the reason [`TargetTable::add`] gives.
    pub fn add_area(
        &mut self,
        beacon: BeaconId,
        kind: AreaKind,
        centre: [Fx; 3],
        area: ColumnBox,
    ) -> Result<(), TargetsFull> {
        self.add_bound(
            beacon,
            kind.kind(),
            0,
            centre,
            area,
            (u32::MAX, crate::targeting::DESCRIPTION_VOXEL),
            BuildOrder::NONE,
        )
    }

    /// The one writer of a row: [`TargetTable::add`] and
    /// [`TargetTable::add_area`] go through it, and so does a Build target
    /// with its place in the build order and its rotation, and -- for one
    /// written through `on` -- `bound`, the feature it is bound to and how it
    /// was written. `area` is the row's footprint, [`ColumnBox::default`] on a
    /// Build row.
    ///
    /// A row goes after every row of its `(beacon, kind)`, so a beacon's
    /// targets keep their list position, which breaks a tie of `order`.
    ///
    /// # Errors
    ///
    /// [`TargetsFull`], for the reason [`TargetTable::add`] gives.
    #[allow(
        clippy::too_many_arguments,
        reason = "one argument per column a row writes, as `StructureTable::push`; a parameter struct would be built at its callers only to be taken apart here"
    )]
    pub fn add_bound(
        &mut self,
        beacon: BeaconId,
        kind: TargetKind,
        blueprint: u8,
        at: [Fx; 3],
        area: ColumnBox,
        bound: (u32, u8),
        build: BuildOrder,
    ) -> Result<(), TargetsFull> {
        if self.count >= self.capacity {
            return Err(TargetsFull {
                capacity: self.capacity,
            });
        }
        let key = (beacon.raw(), kind.id());
        let slot = self
            .beacon
            .iter()
            .zip(&self.kind)
            .position(|(here, of)| (here.raw(), *of) > key)
            .unwrap_or(self.beacon.len());
        self.beacon.insert(slot, beacon);
        self.kind.insert(slot, kind.id());
        self.blueprint.insert(slot, blueprint);
        self.at.insert(slot, at);
        self.area.insert(slot, area);
        self.built.insert(slot, StructureId::NONE);
        self.feature.insert(slot, bound.0);
        self.desc.insert(slot, bound.1);
        self.order.insert(slot, build.order);
        self.rotation.insert(slot, build.rotation.raw());
        self.wall_top.insert(slot, NO_WALL);
        self.wall_done.insert(slot, false);
        self.count = self.count.saturating_add(1);
        Ok(())
    }

    /// Remove row `slot`.
    pub fn remove(&mut self, slot: usize) {
        if slot >= self.beacon.len() {
            return;
        }
        self.beacon.remove(slot);
        self.kind.remove(slot);
        self.blueprint.remove(slot);
        self.at.remove(slot);
        self.area.remove(slot);
        self.built.remove(slot);
        self.feature.remove(slot);
        self.desc.remove(slot);
        self.order.remove(slot);
        self.rotation.remove(slot);
        self.wall_top.remove(slot);
        self.wall_done.remove(slot);
        self.count = self.count.saturating_sub(1);
    }

    /// Remove every row of `beacon`.
    ///
    /// What a mandate switch does (item 20: "switching a beacon's mandate
    /// clears the old mandate's settings and targets"), and what a recycled or
    /// destroyed beacon leaves behind — nothing.
    pub fn clear_beacon(&mut self, beacon: BeaconId) {
        let mut slot: usize = 0;
        while slot < self.beacon.len() {
            if self.beacon.get(slot).copied() == Some(beacon) {
                self.remove(slot);
            } else {
                slot = slot.saturating_add(1);
            }
        }
    }

    /// Replace the whole table from a restored snapshot. Same contract as
    /// [`UnitTable::restore`], plus: every kind byte must name a kind this
    /// build defines, for the reason the match phase's byte must.
    pub fn restore(&mut self, capacity: u32, columns: TargetColumns) -> bool {
        let n = columns.beacon.len();
        if columns.kind.len() != n
            || columns.blueprint.len() != n
            || columns.at.len() != n
            || columns.area.len() != n
            || columns.built.len() != n
            || columns.feature.len() != n
            || columns.desc.len() != n
            || columns.order.len() != n
            || columns.rotation.len() != n
            || columns.wall_top.len() != n
            || columns.wall_done.len() != n
        {
            return false;
        }
        if !columns
            .rotation
            .iter()
            .all(|turns| QuarterTurns::new(u32::from(*turns)).is_some())
        {
            return false;
        }
        if !columns
            .kind
            .iter()
            .all(|id| TargetKind::from_id(*id).is_some())
        {
            return false;
        }
        if !columns
            .desc
            .iter()
            .all(|id| *id <= crate::targeting::DESCRIPTION_NEAREST_VENT)
        {
            return false;
        }
        // A wall top stands on a wall's Build row and on no other, and a wall
        // is bought by the voxel, so no structure ever stands for it (S2's
        // `site`): a file pairing them otherwise describes no table this sim
        // builds.
        let walls_agree = columns
            .kind
            .iter()
            .zip(&columns.blueprint)
            .zip(columns.wall_top.iter().zip(&columns.built))
            .all(|((kind, blueprint), (top, built))| {
                let wall =
                    *kind == TargetKind::Build.id() && *blueprint == StructureKind::Wall.id();
                (*top != NO_WALL) == wall && !(wall && built.is_some())
            });
        let done_on_walls = columns
            .wall_done
            .iter()
            .zip(&columns.wall_top)
            .all(|(done, top)| !*done || *top != NO_WALL);
        if !done_on_walls {
            return false;
        }
        if !walls_agree {
            return false;
        }
        let Ok(count) = u32::try_from(n) else {
            return false;
        };
        if count > capacity {
            return false;
        }
        self.count = count;
        self.capacity = capacity;
        self.beacon = columns.beacon;
        self.kind = columns.kind;
        self.blueprint = columns.blueprint;
        self.at = columns.at;
        self.area = columns.area;
        self.built = columns.built;
        self.feature = columns.feature;
        self.desc = columns.desc;
        self.order = columns.order;
        self.rotation = columns.rotation;
        self.wall_top = columns.wall_top;
        self.wall_done = columns.wall_done;
        true
    }
}

/// Every column of a restored [`TargetTable`]. Same reasoning as
/// [`UnitColumns`].
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct TargetColumns {
    /// The beacon each row belongs to.
    pub beacon: Vec<BeaconId>,
    /// [`TargetKind::id`] per row.
    pub kind: Vec<u8>,
    /// [`StructureKind::id`] per row, or zero.
    pub blueprint: Vec<u8>,
    /// Anchors.
    pub at: Vec<[Fx; 3]>,
    /// Each area row's footprint; [`ColumnBox::default`] on a Build row.
    pub area: Vec<ColumnBox>,
    /// The structure realising each Build target.
    pub built: Vec<StructureId>,
    /// The feature each Build target is bound to.
    pub feature: Vec<u32>,
    /// How each Build target was written.
    pub desc: Vec<u8>,
    /// Each Build target's place in the build order.
    pub order: Vec<u32>,
    /// Each Build target's rotation, 0 to 3.
    pub rotation: Vec<u8>,
    /// Each wall target's top, or [`NO_WALL`].
    pub wall_top: Vec<i16>,
    /// Whether each wall target's box has been filled whole once.
    pub wall_done: Vec<bool>,
}

/// What one seat remembers seeing, with the tick it was seen at (spec section
/// 6, "Vision and sightings").
///
/// Hashed state, because a Survey mandate re-checks "the sightings closest to
/// leaving the reach window" and that decides where a scout walks. The **age**
/// a playbook compares against is derived from `seen_at` and the current tick,
/// so the age accrues on match game time and the Lull and the recap add nothing
/// to it — there is no tick in either.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SightingTable {
    count: u32,
    seat: Vec<u8>,
    asset: Vec<u32>,
    owner: Vec<u8>,
    kind: Vec<u8>,
    at: Vec<[Fx; 3]>,
    seen_at: Vec<u32>,
    capacity: u32,
    per_seat: u32,
}

impl SightingTable {
    /// A table holding `per_seat` sightings for each of `seats` seats, never
    /// growing.
    ///
    /// The per-seat number is the one the eviction rule reads, and the total
    /// is its product with the seat count: one seat filling its own memory
    /// therefore costs another seat nothing, which is what "per seat" has to
    /// mean if the constant is to be read the way it is named. The rows share
    /// one set of columns only so that the table is one allocation.
    #[must_use]
    pub fn with_room(seats: u32, per_seat: u32) -> SightingTable {
        let capacity = seats.saturating_mul(per_seat);
        let n = crate::math::index::row(capacity);
        SightingTable {
            count: 0,
            seat: Vec::with_capacity(n),
            asset: Vec::with_capacity(n),
            owner: Vec::with_capacity(n),
            kind: Vec::with_capacity(n),
            at: Vec::with_capacity(n),
            seen_at: Vec::with_capacity(n),
            capacity,
            per_seat,
        }
    }

    /// How many sightings **one seat** may hold at once.
    #[must_use]
    pub const fn per_seat(&self) -> u32 {
        self.per_seat
    }

    /// How many sightings `seat` is holding.
    #[must_use]
    pub fn len_of(&self, seat: SeatId) -> u32 {
        let mut total: u32 = 0;
        for who in &self.seat {
            if *who == seat.raw() {
                total = total.saturating_add(1);
            }
        }
        total
    }

    /// How many sightings are remembered.
    #[must_use]
    pub const fn len(&self) -> u32 {
        self.count
    }

    /// Whether nobody remembers anything.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// How many sightings it will ever hold.
    #[must_use]
    pub const fn capacity(&self) -> u32 {
        self.capacity
    }

    /// The seat column.
    #[must_use]
    pub fn seats(&self) -> &[u8] {
        &self.seat
    }

    /// The asset column.
    #[must_use]
    pub fn assets(&self) -> &[u32] {
        &self.asset
    }

    /// The owning-seat column.
    #[must_use]
    pub fn owners(&self) -> &[u8] {
        &self.owner
    }

    /// The kind column, as [`crate::knowledge::AssetKind::id`] wire values.
    #[must_use]
    pub fn kinds(&self) -> &[u8] {
        &self.kind
    }

    /// The remembered-place column.
    #[must_use]
    pub fn places(&self) -> &[[Fx; 3]] {
        &self.at
    }

    /// The tick each sighting was taken at.
    #[must_use]
    pub fn seen_at(&self) -> &[u32] {
        &self.seen_at
    }

    /// Where `(seat, asset)` sits, or where it would be inserted. The list is
    /// sorted by that pair, which is unique by construction (item 62).
    #[allow(
        clippy::integer_division,
        reason = "a binary search's midpoint; both operands are non-negative and the halving is exact by construction"
    )]
    fn seek(&self, seat: u8, asset: u32) -> Result<usize, usize> {
        let mut lo: usize = 0;
        let mut hi = self.seat.len();
        while lo < hi {
            let mid = lo.saturating_add(hi.saturating_sub(lo) / 2);
            // `mid` is below the seat column's length, and every writer keeps
            // the asset column the same length, so both reads find their row.
            let (Some(held), Some(of)) = (self.seat.get(mid), self.asset.get(mid)) else {
                return Err(lo);
            };
            match (*held, *of).cmp(&(seat, asset)) {
                core::cmp::Ordering::Less => lo = mid.saturating_add(1),
                core::cmp::Ordering::Greater => hi = mid,
                core::cmp::Ordering::Equal => return Ok(mid),
            }
        }
        Err(lo)
    }

    /// Record that `seat` saw `asset` at `at` on `tick`.
    ///
    /// A second sighting of the same asset **replaces** the memory of it rather
    /// than joining it, which is what keeps the `(seat, asset)` key unique. A
    /// seat that is holding its [`SightingTable::per_seat`] allowance forgets
    /// its **own** stalest sighting first, ties to the lowest asset id — the
    /// same order the Survey mandate refreshes in, so what falls out is what
    /// was about to leave the window anyway. The allowance is per seat rather
    /// than per table, so a seat that sees a great deal cannot starve a seat
    /// that has seen nothing; the shared ceiling is the sum of the allowances
    /// and cannot be reached before one of them is. `false` when nothing was
    /// recorded.
    #[allow(
        clippy::too_many_arguments,
        reason = "one argument per column of the row being written, as the SoA `push`es above"
    )]
    pub fn see(
        &mut self,
        seat: SeatId,
        asset: u32,
        owner: SeatId,
        kind: u8,
        at: [Fx; 3],
        tick: u32,
    ) -> bool {
        match self.seek(seat.raw(), asset) {
            Ok(slot) => {
                if let Some(place) = self.at.get_mut(slot) {
                    *place = at;
                }
                if let Some(when) = self.seen_at.get_mut(slot) {
                    *when = tick;
                }
                if let Some(who) = self.owner.get_mut(slot) {
                    *who = owner.raw();
                }
                if let Some(what) = self.kind.get_mut(slot) {
                    *what = kind;
                }
                true
            }
            Err(slot) => {
                if self.len_of(seat) >= self.per_seat {
                    let Some(stalest) = self.stalest_of(seat) else {
                        return false;
                    };
                    self.forget(stalest);
                    return self.see(seat, asset, owner, kind, at, tick);
                }
                if self.count >= self.capacity {
                    return false;
                }
                self.seat.insert(slot, seat.raw());
                self.asset.insert(slot, asset);
                self.owner.insert(slot, owner.raw());
                self.kind.insert(slot, kind);
                self.at.insert(slot, at);
                self.seen_at.insert(slot, tick);
                self.count = self.count.saturating_add(1);
                true
            }
        }
    }

    /// The row holding `seat`'s stalest sighting — the one closest to leaving
    /// the reach window — or `None` when it remembers nothing.
    ///
    /// The key is `(seen_at, asset)`: earliest first, ties to the lowest asset
    /// id, which is item 62's convention and what makes "closest to leaving the
    /// window first" a total order rather than a preference.
    #[must_use]
    pub fn stalest_of(&self, seat: SeatId) -> Option<usize> {
        let mut best: Option<(u32, u32, usize)> = None;
        let rows = self.seat.iter().zip(&self.seen_at).zip(&self.asset);
        for (slot, ((who, when), asset)) in rows.enumerate() {
            if *who != seat.raw() {
                continue;
            }
            if best.is_none_or(|(bw, ba, _)| (*when, *asset) < (bw, ba)) {
                best = Some((*when, *asset, slot));
            }
        }
        best.map(|(_, _, slot)| slot)
    }

    /// Forget row `slot`.
    pub fn forget(&mut self, slot: usize) {
        if slot >= self.seat.len() {
            return;
        }
        self.seat.remove(slot);
        self.asset.remove(slot);
        self.owner.remove(slot);
        self.kind.remove(slot);
        self.at.remove(slot);
        self.seen_at.remove(slot);
        self.count = self.count.saturating_sub(1);
    }

    /// Replace the whole table from a restored snapshot. Same contract as
    /// [`UnitTable::restore`], plus: the `(seat, asset)` key must be strictly
    /// ascending, which is what the table's own search assumes.
    pub fn restore(&mut self, capacity: u32, columns: SightingColumns) -> bool {
        let n = columns.seat.len();
        if columns.asset.len() != n
            || columns.owner.len() != n
            || columns.kind.len() != n
            || columns.at.len() != n
            || columns.seen_at.len() != n
        {
            return false;
        }
        let mut keys = columns.seat.iter().zip(&columns.asset);
        let mut before = keys.next();
        for here in keys {
            if before.is_some_and(|previous| previous >= here) {
                return false;
            }
            before = Some(here);
        }
        let Ok(count) = u32::try_from(n) else {
            return false;
        };
        if count > capacity {
            return false;
        }
        self.count = count;
        self.capacity = capacity;
        self.seat = columns.seat;
        self.asset = columns.asset;
        self.owner = columns.owner;
        self.kind = columns.kind;
        self.at = columns.at;
        self.seen_at = columns.seen_at;
        true
    }
}

/// Every column of a restored [`SightingTable`]. Same reasoning as
/// [`UnitColumns`].
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct SightingColumns {
    /// Whose memory each row is.
    pub seat: Vec<u8>,
    /// What was seen.
    pub asset: Vec<u32>,
    /// Who owns the thing seen.
    pub owner: Vec<u8>,
    /// [`crate::knowledge::AssetKind::id`] per row.
    pub kind: Vec<u8>,
    /// Where it was when it was seen.
    pub at: Vec<[Fx; 3]>,
    /// The tick it was seen at.
    pub seen_at: Vec<u32>,
}

/// The Quartermaster priority a beacon carries when nobody has set one:
/// `gp.v1.InterfaceRow.QuartermasterPriority.NORMAL`.
///
/// The three constants are the schema's wire values rather than an enum of our
/// own, because the byte reaches the canonical encoding and a second numbering
/// beside the proto's is a second thing to keep in step.
pub const PRIORITY_NORMAL: u8 = 2;

/// `gp.v1.InterfaceRow.QuartermasterPriority.LOW` - browned out first.
pub const PRIORITY_LOW: u8 = 1;

/// `gp.v1.InterfaceRow.QuartermasterPriority.HIGH` - browned out last.
pub const PRIORITY_HIGH: u8 = 3;

/// How many seats may hold kill credit against one asset at a time (item 17).
///
/// Three, which is exactly enough and not a cap anybody can meet: only damage
/// dealt by **other** seats counts, and v1 seats three, so an asset can carry
/// credit for at most two. The determinism harness seats four, which is the
/// reason the slot count is three rather than two - it is the widest the table
/// can be asked to be. A fourth distinct damager is dropped by
/// [`CreditTable::credit`], deterministically and with the reason written
/// there, rather than growing a row inside a tick.
pub const CREDIT_SLOTS: usize = 3;

/// Per-seat kill-credit counters, at most [`CREDIT_SLOTS`] per asset
/// (item 17).
///
/// Sparse and sorted by [`crate::knowledge::AssetId`]: an asset nobody has
/// damaged has no row, so the table is empty until combat exists (S2) and costs
/// the hash one length prefix. It is **hashed from the day it exists** anyway,
/// because a field that affects behaviour and is not hashed is a latent desync
/// and the apportionment it feeds decides who is paid for a kill
/// (AGENTS.md section 4.8).
///
/// The rules it carries, all item 17's: only damage dealt by other seats
/// counts, clamped to the hit points actually removed; reaching full hit points
/// clears the counters; an asset lost with no enemy damage on the clock credits
/// nobody; an eliminated seat's accrued share is dropped rather than
/// redistributed; and the split is apportioned as integers by largest remainder
/// with ties to the lowest seat id.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CreditTable {
    count: u32,
    asset: Vec<u32>,
    seat: Vec<[u8; CREDIT_SLOTS]>,
    damage: Vec<[i32; CREDIT_SLOTS]>,
    capacity: u32,
}

impl CreditTable {
    /// A table that will hold credit against `capacity` assets at once and
    /// never grow.
    #[must_use]
    pub fn with_capacity(capacity: u32) -> CreditTable {
        let n = crate::math::index::row(capacity);
        CreditTable {
            count: 0,
            asset: Vec::with_capacity(n),
            seat: Vec::with_capacity(n),
            damage: Vec::with_capacity(n),
            capacity,
        }
    }

    /// How many assets carry credit right now.
    #[must_use]
    pub const fn len(&self) -> u32 {
        self.count
    }

    /// Whether nobody holds credit against anything.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// How many assets it will ever hold credit against.
    #[must_use]
    pub const fn capacity(&self) -> u32 {
        self.capacity
    }

    /// The asset column, ascending. The sort key, and unique: one row per
    /// asset (item 62).
    #[must_use]
    pub fn assets(&self) -> &[u32] {
        &self.asset
    }

    /// The per-slot seat column.
    #[must_use]
    pub fn seats(&self) -> &[[u8; CREDIT_SLOTS]] {
        &self.seat
    }

    /// The per-slot damage column, in hit points actually removed.
    #[must_use]
    pub fn damages(&self) -> &[[i32; CREDIT_SLOTS]] {
        &self.damage
    }

    /// Where `asset` sits, or where it would be inserted.
    fn seek(&self, asset: u32) -> Result<usize, usize> {
        self.asset.binary_search(&asset)
    }

    /// Book `amount` hit points of damage dealt to `asset` by `by`.
    ///
    /// `false` when nothing was booked: `amount` is not positive, `by` is not a
    /// seat, the table is full, or the asset already carries [`CREDIT_SLOTS`]
    /// other damagers. A drop is deterministic and visible to the caller, and
    /// it cannot happen at any seat count v1 allows.
    pub fn credit(&mut self, asset: u32, by: SeatId, amount: i32) -> bool {
        if amount <= 0 || !by.is_some() {
            return false;
        }
        let at = match self.seek(asset) {
            Ok(at) => at,
            Err(at) => {
                if self.count >= self.capacity {
                    return false;
                }
                self.asset.insert(at, asset);
                self.seat.insert(at, [SeatId::NEUTRAL.raw(); CREDIT_SLOTS]);
                self.damage.insert(at, [0; CREDIT_SLOTS]);
                self.count = self.count.saturating_add(1);
                at
            }
        };
        let (Some(seats), Some(damages)) = (self.seat.get_mut(at), self.damage.get_mut(at)) else {
            return false;
        };
        for (owner, total) in seats.iter_mut().zip(damages.iter_mut()) {
            if *owner == by.raw() {
                *total = total.saturating_add(amount);
                return true;
            }
            if *owner == SeatId::NEUTRAL.raw() {
                *owner = by.raw();
                *total = amount;
                return true;
            }
        }
        false
    }

    /// What each seat has dealt to `asset`, ascending by seat id, into `out`.
    ///
    /// `out` is the caller's buffer and is cleared first, so a tick phase that
    /// keeps one around allocates nothing.
    pub fn shares_of(&self, asset: u32, out: &mut Vec<(u8, i32)>) {
        out.clear();
        let Ok(at) = self.seek(asset) else {
            return;
        };
        let (Some(seats), Some(damages)) = (self.seat.get(at), self.damage.get(at)) else {
            return;
        };
        for (seat, dealt) in seats.iter().zip(damages) {
            if SeatId::new(*seat).is_some() && *dealt > 0 {
                out.push((*seat, *dealt));
            }
        }
        // item 62: the key is the seat id, unique within a row by `credit`.
        out.sort_unstable();
    }

    /// Forget every counter against `asset`.
    ///
    /// Item 17: reaching full hit points clears the counters, and so does
    /// settling a death. A cleared row leaves the table rather than sitting
    /// there at zero, so the table stays the sparse list its sort key assumes.
    pub fn clear_asset(&mut self, asset: u32) {
        let Ok(at) = self.seek(asset) else {
            return;
        };
        self.asset.remove(at);
        self.seat.remove(at);
        self.damage.remove(at);
        self.count = self.count.saturating_sub(1);
    }

    /// Drop every counter an eliminated seat holds, everywhere.
    ///
    /// Item 17: an eliminated seat's accrued share is **dropped, not
    /// redistributed**, so the remaining damagers keep their own numbers and
    /// the apportionment simply has less to divide.
    pub fn drop_seat(&mut self, seat: SeatId) {
        for (seats, damages) in self.seat.iter_mut().zip(self.damage.iter_mut()) {
            for (owner, total) in seats.iter_mut().zip(damages.iter_mut()) {
                if *owner == seat.raw() {
                    *owner = SeatId::NEUTRAL.raw();
                    *total = 0;
                }
            }
        }
    }

    /// Whether any seat holds credit against `asset`: a death that has not
    /// settled yet, so the asset's row may not be freed (S2's plan, E1).
    #[must_use]
    pub fn holds(&self, asset: u32) -> bool {
        self.seek(asset).is_ok()
    }

    /// Replace the whole table from a restored snapshot.
    ///
    /// Returns `false` and changes nothing when the columns disagree in
    /// length, when the asset column is not strictly ascending — the sort key
    /// the table's own search assumes — or when the file holds more rows than
    /// the receiving world has room for.
    pub fn restore(
        &mut self,
        capacity: u32,
        asset: Vec<u32>,
        seat: Vec<[u8; CREDIT_SLOTS]>,
        damage: Vec<[i32; CREDIT_SLOTS]>,
    ) -> bool {
        let n = asset.len();
        if seat.len() != n || damage.len() != n {
            return false;
        }
        if asset.windows(2).any(|w| w.first() >= w.get(1)) {
            return false;
        }
        let Ok(count) = u32::try_from(n) else {
            return false;
        };
        if count > capacity {
            return false;
        }
        self.count = count;
        self.capacity = capacity;
        self.asset = asset;
        self.seat = seat;
        self.damage = damage;
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
    /// One entry per item, holding the item's row in the table the grid was
    /// built from: typed, so it is never read as an id (S2's plan, E1).
    items: Vec<crate::ids::Row>,
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
            items: vec![crate::ids::Row::FIRST; capacity_usize],
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
        let [px, py, _] = pos;
        let (x, y) = (px.floor_voxels(), py.floor_voxels());
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
        // Both operands are non-negative, so the quotient is too and its
        // magnitude is itself.
        let cell = (offset / self.cell_size_voxels).unsigned_abs();
        cell.min(cells.saturating_sub(1))
    }

    /// Rebuild the index from the positions of the items in rows
    /// `0..positions.len()`, by counting sort. Allocation-free and infallible
    /// (S2's plan, E6).
    ///
    /// **The grid is built for the table's room.** Its owner sizes it, at
    /// construction, for the most rows the table can ever hold, and the table
    /// refuses to grow past that ([`crate::world::World::fabricate`], and a
    /// restore's `OverCeiling`), so `positions` is never longer than the grid's
    /// capacity. It is read up to that capacity, which is all of it.
    pub fn rebuild(&mut self, positions: &[[Fx; 3]]) {
        self.cursor.fill(0);
        self.starts.fill(0);
        let capacity = self.items.len();

        // Pass 1: count per cell.
        for pos in positions.iter().take(capacity) {
            let cell = crate::math::index::row(self.cell_of(*pos));
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
        for (pos, row) in positions.iter().take(capacity).zip(crate::ids::Row::all()) {
            let cell = crate::math::index::row(self.cell_of(*pos));
            let Some(head) = self.cursor.get_mut(cell) else {
                continue;
            };
            let at = crate::math::index::row(*head);
            *head = head.saturating_add(1);
            if let Some(slot) = self.items.get_mut(at) {
                *slot = row;
            }
        }
    }

    /// Collect every item within `radius` **voxels** of `centre`, sorted
    /// ascending by row, into `out`.
    ///
    /// `positions` is the table's position column **as it is now**, which may
    /// be later than the [rebuild](Csr::rebuild): the grid stores rows, and
    /// each candidate's exact distance is taken from `positions` at its row. A
    /// row the slice no longer covers is dropped rather than guessed at.
    ///
    /// **The grid is stale by however far an item has moved since it was
    /// built** (S2's plan, E11): it is rebuilt in the Broadphase, before
    /// Movement, so a unit that walked into range this tick sits in a cell the
    /// radius alone would not touch. `pad` widens the cells asked for by the
    /// most any item can have moved since the rebuild; the exact test is still
    /// against `radius`, so the answer is exact over positions that moved.
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
        pad: Fx,
        out: &mut Vec<crate::ids::Row>,
    ) {
        out.clear();
        if radius < Fx::ZERO || pad < Fx::ZERO {
            return;
        }
        let limit = Sq::of_radius(radius);
        let radius_cells = self.radius_in_cells(radius.saturating_add(pad));
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
                let c = crate::math::index::row(y.saturating_mul(self.cells_x).saturating_add(x));
                // `x` and `y` are clamped into the grid above, so `c` and
                // `c + 1` are both entries of `starts`, which holds one more
                // entry than there are cells.
                if let (Some(from), Some(to)) = (
                    self.starts.get(c).copied(),
                    self.starts.get(c.saturating_add(1)).copied(),
                ) {
                    let range = crate::math::index::row(from)..crate::math::index::row(to);
                    if let Some(slice) = self.items.get(range) {
                        for row in slice {
                            let Some(pos) = positions.get(row.index()) else {
                                continue;
                            };
                            if Sq::between(*pos, centre) <= limit {
                                out.push(*row);
                            }
                        }
                    }
                }
                x = x.saturating_add(1);
            }
            y = y.saturating_add(1);
        }
        // Rows are unique, so an unstable sort is a total order (item 62's
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
        // E7: cannot miss: `voxels` is at least one for a radius that is not negative, and the cell edge was refused at construction unless positive (`Csr::new`)
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

    /// Whether it is a **capability** structure, earned by a licence and never
    /// rebuilt automatically (spec section 8): of this build's kinds, the
    /// Resonance Spire, one of spec section 8's three licensed structures. The
    /// Generator, the base kit and the Survey post are ordinary.
    ///
    /// PLACEHOLDER: which structure kinds are capability structures, the Resonance Spire alone — owner, at S4, with the capability catalogue.
    /// The Survey post is spec section 6's in-world name for a Survey
    /// beacon's post and earns no licence, so it is read as ordinary; no Build
    /// target can name either before S4's catalogue, so the reading moves
    /// nothing yet.
    #[must_use]
    pub const fn is_capability(self) -> bool {
        matches!(self, StructureKind::ResonanceSpire)
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

/// A beacon's identity: its row and the row's generation ([`crate::ids`]).
/// A seat's core is generation 0 and its row is never freed.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct BeaconId(u32);

crate::ids::generational_id!(BeaconId, "beacon");

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
}

/// The prefix of a seat's name for one of its **own** beacons: `b_NN`.
pub const OWN_BEACON_PREFIX: &str = "b_";

/// The prefix of a viewer's name for **another seat's** beacon: `e_NN`.
pub const FOREIGN_BEACON_PREFIX: &str = "e_";

/// How many digits a beacon name pads its number to.
///
/// Two, so a seat's core is `b_00` and the spec's own `get_beacon{b_04}` reads
/// as written. The padding is a minimum, never a truncation: a number past 99
/// writes three digits, and [`parse_beacon_name`] reads the number rather than
/// the padding, so `b_4`, `b_04` and `b_004` are one name.
pub const BEACON_NAME_DIGITS: usize = 2;

/// The name a seat calls one of its **own** beacons by: `b_` and the beacon's
/// per-seat ordinal ([`BeaconTable::ordinals`]).
///
/// Per seat since S1 (decisions-log item 127 (13); `docs/design/targeting.md`,
/// "Names"): a seat's core is always `b_00`, its first placed beacon `b_01`,
/// and the number counts the seat's own beacons and nothing else, so a new
/// beacon's name tells a seat nothing about how many the others placed. Before
/// S1 the number was the beacon's row in the world's table, a count of every
/// seat's beacons. The verifier resolves a playbook's references against a
/// list of these names, the gateway builds that list, and the interpreter
/// parses them back, so the spelling lives here, beside the table, rather than
/// inside any one of the three.
#[must_use]
pub fn own_beacon_name(ordinal: u32) -> String {
    format!("{OWN_BEACON_PREFIX}{ordinal:0BEACON_NAME_DIGITS$}")
}

/// The name a viewer calls **another seat's** beacon by: `e_` and the number
/// that viewer minted for it on first sighting, from one
/// (`docs/design/targeting.md`, "Names"). The minting is the gateway's, per
/// viewer; the spelling is here so the two halves of a name agree.
#[must_use]
pub fn foreign_beacon_name(handle: u32) -> String {
    format!("{FOREIGN_BEACON_PREFIX}{handle:0BEACON_NAME_DIGITS$}")
}

/// What a beacon name names: one of the reader's own beacons, by its per-seat
/// ordinal, or another seat's, by the reader's own handle for it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum BeaconName {
    /// `b_NN`: the reader's own beacon with this ordinal.
    Own(u32),
    /// `e_NN`: another seat's beacon, by the reader's handle for it.
    Foreign(u32),
}

/// The name `text` spells, or `None` when it is not a beacon name.
///
/// Decided on **characters**, never on a parse that would also take `b_+4`,
/// `b_ 4` or `b_4\n`: the digits must be ASCII digits and nothing else, the
/// same rule `plan-core`'s template ids are decided by, for the same
/// cross-platform reason (decisions-log item 100's closing note).
#[must_use]
pub fn parse_beacon_name(text: &str) -> Option<BeaconName> {
    let (digits, own) = if let Some(rest) = text.strip_prefix(OWN_BEACON_PREFIX) {
        (rest, true)
    } else {
        (text.strip_prefix(FOREIGN_BEACON_PREFIX)?, false)
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let number = digits.parse::<u32>().ok()?;
    Some(if own {
        BeaconName::Own(number)
    } else {
        BeaconName::Foreign(number)
    })
}

/// A structure's identity: its row and the row's generation ([`crate::ids`]).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct StructureId(u32);

crate::ids::generational_id!(StructureId, "structure");

impl StructureId {
    /// "No structure" — the sentinel a Build target's `built` link carries
    /// before anything is paid for. The same raw value as [`BeaconId::NONE`],
    /// which is what the link held before it was typed, so no byte moves.
    pub const NONE: StructureId = StructureId(u32::MAX);

    /// Whether this is a real structure rather than [`StructureId::NONE`].
    #[must_use]
    pub const fn is_some(self) -> bool {
        self.0 != StructureId::NONE.0
    }

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

/// A wreck's identity: its row and the row's generation ([`crate::ids`]).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct WreckId(u32);

crate::ids::generational_id!(WreckId, "wreck");

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
    /// The Quartermaster priority knob.
    pub priority: Vec<u8>,
    /// The Survey mandate's scout count.
    pub scouts: Vec<u8>,
    /// Each beacon's per-seat ordinal ([`BeaconTable::ordinals`]).
    pub ordinal: Vec<u32>,
    /// The Mine mandate's `dig_max_depth`, per beacon.
    pub dig_depth: Vec<u32>,
    /// The Mine mandate's `pillar_spacing`, per beacon.
    pub pillars: Vec<u32>,
    /// The Mine mandate's `seam_choice`, as [`crate::mining::SeamChoice::id`].
    pub seam_choice: Vec<u8>,
    /// The Mine mandate's `flee_on_threat`, per beacon.
    pub flee: Vec<bool>,
    /// The seam each beacon holds ([`BeaconTable::seams`]).
    pub seam: Vec<u32>,
    /// Each beacon's combat memory ([`Arms`]): only a core fires.
    pub arms: Vec<Arms>,
    /// Each beacon's Common settings ([`Common`]).
    pub common: Vec<Common>,
    /// Each beacon's Build settings and paid work ([`crate::site::BeaconSite`]).
    pub site: Vec<crate::site::BeaconSite>,
    /// Whether each row is free (S2's plan, E1).
    pub free: Vec<bool>,
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
    pub home: Vec<BeaconId>,
    /// Whether each structure is still going up.
    pub building: Vec<bool>,
    /// Each structure's rotation, 0 to 3 ([`QuarterTurns::raw`]).
    pub rotation: Vec<u8>,
    /// The tick each ruin was ruined at, or [`NOT_RUINED`].
    pub ruined_at: Vec<u32>,
    /// Each structure's combat memory ([`Arms`]).
    pub arms: Vec<Arms>,
    /// Whether each row is free (S2's plan, E1).
    pub free: Vec<bool>,
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
    priority: Vec<u8>,
    scouts: Vec<u8>,
    ordinal: Vec<u32>,
    dig_depth: Vec<u32>,
    pillars: Vec<u32>,
    seam_choice: Vec<u8>,
    flee: Vec<bool>,
    seam: Vec<u32>,
    arms: Vec<Arms>,
    common: Vec<Common>,
    site: Vec<crate::site::BeaconSite>,
    free: Vec<bool>,
}

impl BeaconTable {
    /// An empty table sized for `count` beacons. Nothing allocates after this.
    #[must_use]
    pub fn with_capacity(count: u32) -> BeaconTable {
        let n = crate::math::index::row(count);
        BeaconTable {
            count: 0,
            id: Vec::with_capacity(n),
            seat: Vec::with_capacity(n),
            pos: Vec::with_capacity(n),
            mandate: Vec::with_capacity(n),
            program: Vec::with_capacity(n),
            hp: Vec::with_capacity(n),
            dormant: Vec::with_capacity(n),
            priority: Vec::with_capacity(n),
            scouts: Vec::with_capacity(n),
            ordinal: Vec::with_capacity(n),
            dig_depth: Vec::with_capacity(n),
            pillars: Vec::with_capacity(n),
            seam_choice: Vec::with_capacity(n),
            flee: Vec::with_capacity(n),
            seam: Vec::with_capacity(n),
            arms: Vec::with_capacity(n),
            common: Vec::with_capacity(n),
            site: Vec::with_capacity(n),
            free: Vec::with_capacity(n),
        }
    }

    /// Append one beacon at generation 0, as the generator places a core.
    ///
    /// Its ordinal is the number of rows `seat` already holds, so a seat's
    /// first beacon -- its core, which the generator places before anything
    /// else -- is ordinal 0. A beacon placed during a match takes its ordinal
    /// from the seat's placement counter instead ([`BeaconTable::place`],
    /// [`SeatTable::placed`]), because rows are reused and a count of the rows a
    /// seat holds would hand a dead beacon's name to the next one.
    ///
    /// # Errors
    ///
    /// [`TableFull`] past [`crate::ids::ROOM_LIMIT`] rows.
    pub fn push(
        &mut self,
        seat: SeatId,
        pos: [Fx; 3],
        mandate: BeaconMandate,
        hp: Hp,
        dormant: bool,
    ) -> Result<BeaconId, TableFull> {
        let ordinal = self.count_of(seat);
        self.place(
            crate::ids::ROOM_LIMIT,
            seat,
            pos,
            mandate,
            BeaconStart {
                hp,
                dormant,
                ordinal,
                ready_at: 0,
            },
        )
        .map(|placed| placed.id)
    }

    /// Where the next beacon goes ([`vacancy_in`]), within `room` rows.
    ///
    /// # Errors
    ///
    /// [`TableFull`] when no row is free and the table holds `room`.
    pub fn vacancy(&self, room: u32) -> Result<Vacancy, TableFull> {
        vacancy_in(&self.id, &self.free, room)
    }

    /// Place one beacon in the next [`Vacancy`] within `room` rows, with
    /// **every column written**: a reused row keeps nothing of the beacon that
    /// held it (S2's plan, E1). Never grows a column inside a tick: the room a
    /// Push deploys into is reserved first ([`BeaconTable::reserve`]).
    ///
    /// # Errors
    ///
    /// [`TableFull`] when no row is free and the table holds `room`.
    pub fn place(
        &mut self,
        room: u32,
        seat: SeatId,
        pos: [Fx; 3],
        mandate: BeaconMandate,
        start: BeaconStart,
    ) -> Result<Placed<BeaconId>, TableFull> {
        let at = self.vacancy(room)?;
        let id = BeaconId::of(at.row, at.generation);
        let row = at.row;
        let mine = crate::mining::MineSettings::default();
        put(&mut self.id, row, id.raw());
        put(&mut self.seat, row, seat.raw());
        put(&mut self.pos, row, pos);
        put(&mut self.mandate, row, mandate.kind.id());
        put(&mut self.program, row, mandate.program_id.raw());
        put(&mut self.hp, row, start.hp);
        put(&mut self.dormant, row, start.dormant);
        put(&mut self.priority, row, PRIORITY_NORMAL);
        put(&mut self.scouts, row, 0);
        put(&mut self.ordinal, row, start.ordinal);
        put(&mut self.dig_depth, row, mine.dig_max_depth);
        put(&mut self.pillars, row, mine.pillar_spacing);
        put(&mut self.seam_choice, row, mine.seam_choice.id());
        put(&mut self.flee, row, mine.flee_on_threat);
        put(&mut self.seam, row, crate::targeting::NO_FEATURE);
        put(&mut self.arms, row, Arms::ready_at(start.ready_at));
        put(&mut self.common, row, Common::DEFAULT);
        put(&mut self.site, row, crate::site::BeaconSite::DEFAULT);
        put(&mut self.free, row, false);
        if !at.reused {
            // An appended row is the table's last, at most `Row::MAX`.
            self.count = row.raw().saturating_add(1);
        }
        Ok(Placed {
            id,
            reused: at.reused,
        })
    }

    /// Free `row`: its beacon has died, its death has settled, and it is not a
    /// core (a core's row is never freed: "brownout distance is measured from
    /// the core or its former site", spec section 5). Every column goes back to
    /// nobody's: no seat, no hit points, no writ and no settings. `true` when
    /// the row's generation is the last, so it will never be reused.
    pub fn release(&mut self, row: crate::ids::Row) -> bool {
        let index = row.index();
        let Some(free) = self.free.get_mut(index) else {
            return false;
        };
        *free = true;
        let mine = crate::mining::MineSettings::default();
        if let Some(slot) = self.seat.get_mut(index) {
            *slot = SeatId::NEUTRAL.raw();
        }
        if let Some(slot) = self.hp.get_mut(index) {
            *slot = Hp::ZERO;
        }
        if let Some(slot) = self.mandate.get_mut(index) {
            *slot = crate::seams::MandateKind::None.id();
        }
        if let Some(slot) = self.dormant.get_mut(index) {
            *slot = false;
        }
        if let Some(slot) = self.priority.get_mut(index) {
            *slot = PRIORITY_NORMAL;
        }
        if let Some(slot) = self.scouts.get_mut(index) {
            *slot = 0;
        }
        if let Some(slot) = self.ordinal.get_mut(index) {
            *slot = 0;
        }
        self.set_mine_settings(index, mine);
        if let Some(slot) = self.seam.get_mut(index) {
            *slot = crate::targeting::NO_FEATURE;
        }
        if let Some(slot) = self.arms.get_mut(index) {
            *slot = Arms::NONE;
        }
        if let Some(slot) = self.common.get_mut(index) {
            *slot = Common::DEFAULT;
        }
        if let Some(slot) = self.site.get_mut(index) {
            *slot = crate::site::BeaconSite::DEFAULT;
        }
        self.id
            .get(index)
            .and_then(|id| crate::ids::unpack(*id).ok())
            .is_some_and(|(_, generation)| generation.next().is_none())
    }

    /// The row `beacon` lives in, checked by generation.
    ///
    /// # Errors
    ///
    /// [`crate::ids::RefError`]: a sentinel, no such row, or a row since freed
    /// or reused.
    pub fn row_of(&self, beacon: BeaconId) -> Result<crate::ids::Row, crate::ids::RefError> {
        row_of_in(&self.id, &self.free, beacon.raw())
    }

    /// The id of the beacon in `row`, or `None` for a row the table does not
    /// hold or one that is free.
    #[must_use]
    pub fn id_of(&self, row: crate::ids::Row) -> Option<BeaconId> {
        if self.free.get(row.index()).copied() != Some(false) {
            return None;
        }
        self.id.get(row.index()).copied().map(BeaconId::new)
    }

    /// The free column ([`UnitTable::frees`]).
    #[must_use]
    pub fn frees(&self) -> &[bool] {
        &self.free
    }

    /// How many beacon rows `seat` holds, alive or dead: every row it placed
    /// that has not been freed. A seat's core is always one of them.
    #[must_use]
    pub fn count_of(&self, seat: SeatId) -> u32 {
        self.seat.iter().fold(0_u32, |sum, held| {
            if *held == seat.raw() {
                sum.saturating_add(1)
            } else {
                sum
            }
        })
    }

    /// The row of `seat`'s beacon with this ordinal, alive or dead, or `None`
    /// when the seat holds no such beacon -- because it was never placed, or
    /// because it died and its row has been freed ([`BeaconTable::ordinal`]
    /// tells the two apart).
    ///
    /// What a `b_NN` resolves through: a name counts the reader's own beacons
    /// and nothing else (`docs/design/targeting.md`, "Names").
    #[must_use]
    pub fn row_of_ordinal(&self, seat: SeatId, ordinal: u32) -> Option<usize> {
        self.seat
            .iter()
            .zip(&self.ordinal)
            .position(|(held, number)| *held == seat.raw() && *number == ordinal)
    }

    /// What `seat`'s `b_NN` with this ordinal names, given the seat's
    /// placement counter `placed` ([`SeatTable::placed`]): a row the seat
    /// still holds, a beacon it placed that has since gone, or a number it has
    /// never reached (S2's plan, E1).
    #[must_use]
    pub fn ordinal(&self, seat: SeatId, ordinal: u32, placed: u32) -> Ordinal {
        match self.row_of_ordinal(seat, ordinal) {
            Some(row) => Ordinal::Held(row),
            None if ordinal < placed => Ordinal::Gone,
            None => Ordinal::NeverPlaced,
        }
    }

    /// Make room for `extra` more beacons without growing later.
    ///
    /// The beacon table is the one table a **Push** adds rows to: a
    /// `place_beacon` step deploys one (T11). Nothing in a tick allocates
    /// (G3′ §9.17), so the room is reserved at construction and at a restore,
    /// and a deploy that finds none fails the step rather than growing a
    /// column. Construction only — never called inside a tick.
    pub fn reserve(&mut self, extra: u32) {
        let n = crate::math::index::row(extra);
        self.id.reserve(n);
        self.seat.reserve(n);
        self.pos.reserve(n);
        self.mandate.reserve(n);
        self.program.reserve(n);
        self.hp.reserve(n);
        self.dormant.reserve(n);
        self.priority.reserve(n);
        self.scouts.reserve(n);
        self.ordinal.reserve(n);
        self.dig_depth.reserve(n);
        self.pillars.reserve(n);
        self.seam_choice.reserve(n);
        self.flee.reserve(n);
        self.seam.reserve(n);
        self.arms.reserve(n);
        self.common.reserve(n);
        self.site.reserve(n);
        self.free.reserve(n);
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

    /// The position column, to write into: the support rule moves an asset
    /// that stands on a falling region down with it (S2's plan, `fall`).
    pub fn positions_mut(&mut self) -> &mut [[Fx; 3]] {
        &mut self.pos
    }

    /// The mandate column, to write into.
    ///
    /// One caller: the interpreter's committed `set_mandate` row, which is the
    /// only thing in the sim that changes a beacon's writ — *touch to change*
    /// (AGENTS.md §1).
    pub fn mandates_mut(&mut self) -> &mut [u8] {
        &mut self.mandate
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

    /// The dormancy column, to write into.
    ///
    /// One caller: the power phase's brownout and revive
    /// ([`crate::power`]). Dormancy is a consequence of supply and draw, never
    /// an order a playbook can give.
    pub fn dormant_mut(&mut self) -> &mut [bool] {
        &mut self.dormant
    }

    /// The Quartermaster-priority column, as
    /// `gp.v1.InterfaceRow.QuartermasterPriority` wire values
    /// ([`PRIORITY_LOW`], [`PRIORITY_NORMAL`], [`PRIORITY_HIGH`]).
    ///
    /// The one player lever over power (spec section 7): it orders brownouts
    /// and nothing else, while `$` follows the urgency ladder. It belongs to the
    /// **beacon**, not to the mandate, so a mandate switch does not clear it
    /// (item 20).
    #[must_use]
    pub fn priorities(&self) -> &[u8] {
        &self.priority
    }

    /// The priority column, to write into. The interpreter's committed
    /// `set_priority` row is the only caller — *touch to change*.
    pub fn priorities_mut(&mut self) -> &mut [u8] {
        &mut self.priority
    }

    /// The Survey mandate's scout count, per beacon; zero on every other writ.
    ///
    /// The one mandate setting that is a plain number rather than a list, so it
    /// lives beside the writ rather than in [`TargetTable`].
    #[must_use]
    pub fn scout_counts(&self) -> &[u8] {
        &self.scouts
    }

    /// The scout-count column, to write into. A committed settings row.
    pub fn scout_counts_mut(&mut self) -> &mut [u8] {
        &mut self.scouts
    }

    /// Each beacon's **per-seat ordinal**: its place among its own seat's
    /// beacons, counting from 0, in the order they were placed. A seat's core
    /// is 0.
    ///
    /// What a seat's `b_NN` names ([`own_beacon_name`]). Hashed and
    /// snapshotted although it is a function of the seat column, because it
    /// is what a playbook's name resolves through, and a value that decides
    /// what a tick does is in the hash in its own right (AGENTS.md section
    /// 4.8). [`BeaconTable::restore`] refuses a column that disagrees with the
    /// seat column it travels beside.
    #[must_use]
    pub fn ordinals(&self) -> &[u32] {
        &self.ordinal
    }

    /// The Mine mandate's settings of the beacon at `row`, or `None` when
    /// there is no such row.
    ///
    /// Four plain columns beside the writ, for the reason the scout count is
    /// one: settings that are numbers rather than lists. Every beacon carries
    /// them whatever its writ, at [`crate::mining::MineSettings::default`]
    /// until a settings row writes them, because a mining drone works under
    /// its home beacon's settings whatever that beacon's writ (item 127 (6):
    /// a unit's kind is its job). A mandate switch puts them back to the
    /// defaults (item 20). Hashed and snapshotted like every order.
    #[must_use]
    pub fn mine_settings(&self, row: usize) -> Option<crate::mining::MineSettings> {
        Some(crate::mining::MineSettings {
            dig_max_depth: self.dig_depth.get(row).copied()?,
            pillar_spacing: self.pillars.get(row).copied()?,
            seam_choice: crate::mining::SeamChoice::from_id(self.seam_choice.get(row).copied()?)?,
            flee_on_threat: self.flee.get(row).copied()?,
        })
    }

    /// Write the Mine mandate's settings of the beacon at `row`. A committed
    /// settings row, a mandate switch's reset, or a fixture.
    pub fn set_mine_settings(&mut self, row: usize, settings: crate::mining::MineSettings) {
        if let Some(slot) = self.dig_depth.get_mut(row) {
            *slot = settings.dig_max_depth;
        }
        if let Some(slot) = self.pillars.get_mut(row) {
            *slot = settings.pillar_spacing;
        }
        if let Some(slot) = self.seam_choice.get_mut(row) {
            *slot = settings.seam_choice.id();
        }
        if let Some(slot) = self.flee.get_mut(row) {
            *slot = settings.flee_on_threat;
        }
    }

    /// The `dig_max_depth` column.
    #[must_use]
    pub fn dig_depths(&self) -> &[u32] {
        &self.dig_depth
    }

    /// The `pillar_spacing` column.
    #[must_use]
    pub fn pillar_spacings(&self) -> &[u32] {
        &self.pillars
    }

    /// The `seam_choice` column, as [`crate::mining::SeamChoice::id`] values.
    #[must_use]
    pub fn seam_choices(&self) -> &[u8] {
        &self.seam_choice
    }

    /// The `flee_on_threat` column.
    #[must_use]
    pub fn flees(&self) -> &[bool] {
        &self.flee
    }

    /// The seam each beacon **holds**: its index in the map's feature table,
    /// or [`crate::targeting::NO_FEATURE`] for none.
    ///
    /// The Mine program's `seam_choice`, held until the seam is spent
    /// (`docs/design/targeting.md`, "Three reading rules": the named exception
    /// for a mandate's own `seam_choice`). It decides where every drone homed
    /// to the beacon digs, so it is hashed, snapshotted and in the goldens
    /// (AGENTS.md section 4.8). Chosen on the decision tick
    /// ([`crate::mining`]); released by a mandate switch and by a written
    /// `seam_choice`.
    #[must_use]
    pub fn seams(&self) -> &[u32] {
        &self.seam
    }

    /// The held-seam column, to write into. Crate-internal: the seam choice
    /// and the rows that release it are its only writers.
    pub(crate) fn seams_mut(&mut self) -> &mut [u32] {
        &mut self.seam
    }

    /// The hit-point column, to write into.
    ///
    /// The combat phase's damage drain is the only caller. A beacon at zero
    /// hit points is dead, and item 20's local elimination follows in the same
    /// tick.
    pub fn hit_points_mut(&mut self) -> &mut [Hp] {
        &mut self.hp
    }

    /// The combat memory column ([`Arms`]): a core's cooldown, and every
    /// beacon's attacker.
    #[must_use]
    pub fn arms(&self) -> &[Arms] {
        &self.arms
    }

    /// The combat memory column, to write into: the combat phase.
    pub fn arms_mut(&mut self) -> &mut [Arms] {
        &mut self.arms
    }

    /// The Common settings column ([`Common`]).
    #[must_use]
    pub fn commons(&self) -> &[Common] {
        &self.common
    }

    /// The Common settings column, to write into: a committed settings row
    /// and a mandate switch.
    pub fn commons_mut(&mut self) -> &mut [Common] {
        &mut self.common
    }

    /// The Build settings and paid-work column ([`crate::site::BeaconSite`]).
    #[must_use]
    pub fn sites(&self) -> &[crate::site::BeaconSite] {
        &self.site
    }

    /// The Build settings and paid-work column, to write into: a committed
    /// settings row, a mandate switch, the Quartermaster and the drones.
    pub fn sites_mut(&mut self) -> &mut [crate::site::BeaconSite] {
        &mut self.site
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
            || columns.priority.len() != n
            || columns.scouts.len() != n
            || columns.ordinal.len() != n
            || columns.dig_depth.len() != n
            || columns.pillars.len() != n
            || columns.seam_choice.len() != n
            || columns.flee.len() != n
            || columns.seam.len() != n
            || columns.arms.len() != n
            || columns.common.len() != n
            || columns.site.len() != n
            || columns.free.len() != n
            || !ids_name_their_rows(&columns.id)
        {
            return false;
        }
        // A rules of engagement is one of the four wire values this build
        // names, and a retreat threshold a percent.
        if columns
            .common
            .iter()
            .any(|common| common.roe > 3 || common.retreat_hp_pct > 100)
        {
            return false;
        }
        // A seam choice is one of the three the schema names: an unset or
        // unknown byte describes no beacon this sim builds.
        if columns
            .seam_choice
            .iter()
            .any(|id| crate::mining::SeamChoice::from_id(*id).is_none())
        {
            return false;
        }
        // A seat's ordinals are unique among the rows it holds, so a name
        // resolves to one beacon or none: a file that names two describes no
        // table this sim builds. That each is below its seat's placement
        // counter is the snapshot's check, which holds the counters.
        let mut held: std::collections::BTreeSet<(u8, u32)> = std::collections::BTreeSet::new();
        for (seat, ordinal) in columns.seat.iter().zip(columns.ordinal.iter()) {
            if SeatId::new(*seat).is_some() && !held.insert((*seat, *ordinal)) {
                return false;
            }
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
        self.priority = columns.priority;
        self.scouts = columns.scouts;
        self.ordinal = columns.ordinal;
        self.dig_depth = columns.dig_depth;
        self.pillars = columns.pillars;
        self.seam_choice = columns.seam_choice;
        self.flee = columns.flee;
        self.seam = columns.seam;
        self.arms = columns.arms;
        self.common = columns.common;
        self.site = columns.site;
        self.free = columns.free;
        true
    }
}

/// What a placed beacon starts with beyond its seat, place and writ: its hit
/// points, its dormancy and its per-seat ordinal ([`BeaconTable::place`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BeaconStart {
    /// Hit points at placement.
    pub hp: Hp,
    /// Whether it starts dormant.
    pub dormant: bool,
    /// Its per-seat ordinal: the seat's placement counter when it is placed.
    pub ordinal: u32,
    /// The first tick its weapon may fire on ([`Arms::ready_at`]): only a
    /// core is armed, and the generator places a core ready at tick zero.
    pub ready_at: u32,
}

/// What a seat's `b_NN` names ([`BeaconTable::ordinal`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ordinal {
    /// The row of the beacon the seat holds under the name, alive or dead.
    Held(usize),
    /// The seat placed a beacon under the name, and it is gone: its row was
    /// freed and may hold another asset now.
    Gone,
    /// The seat has placed fewer beacons than the number names.
    NeverPlaced,
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
    home: Vec<BeaconId>,
    building: Vec<bool>,
    rotation: Vec<u8>,
    ruined_at: Vec<u32>,
    arms: Vec<Arms>,
    free: Vec<bool>,
}

/// "This structure is not a ruin" — the value [`StructureTable::ruined_at`]
/// carries for a structure a seat still owns. A sentinel rather than an
/// `Option`, for the reason [`BeaconId::NONE`] is one.
pub const NOT_RUINED: u32 = u32::MAX;

impl StructureTable {
    /// An empty table sized for `count` structures.
    #[must_use]
    pub fn with_capacity(count: u32) -> StructureTable {
        let n = crate::math::index::row(count);
        StructureTable {
            count: 0,
            id: Vec::with_capacity(n),
            seat: Vec::with_capacity(n),
            kind: Vec::with_capacity(n),
            pos: Vec::with_capacity(n),
            hp: Vec::with_capacity(n),
            home: Vec::with_capacity(n),
            building: Vec::with_capacity(n),
            rotation: Vec::with_capacity(n),
            ruined_at: Vec::with_capacity(n),
            arms: Vec::with_capacity(n),
            free: Vec::with_capacity(n),
        }
    }

    /// Make room for `extra` more structures without growing later. Same
    /// contract as [`BeaconTable::reserve`].
    pub fn reserve(&mut self, extra: u32) {
        let n = crate::math::index::row(extra);
        self.id.reserve(n);
        self.seat.reserve(n);
        self.kind.reserve(n);
        self.pos.reserve(n);
        self.hp.reserve(n);
        self.home.reserve(n);
        self.building.reserve(n);
        self.rotation.reserve(n);
        self.ruined_at.reserve(n);
        self.arms.reserve(n);
        self.free.reserve(n);
    }

    /// Where the next structure goes ([`vacancy_in`]), within `room` rows.
    ///
    /// # Errors
    ///
    /// [`TableFull`] when no row is free and the table holds `room`.
    pub fn vacancy(&self, room: u32) -> Result<Vacancy, TableFull> {
        vacancy_in(&self.id, &self.free, room)
    }

    /// Place one structure in the next [`Vacancy`] within `room` rows, with
    /// every column written.
    ///
    /// `building` is spec section 7's "construction time is HP and spectacle,
    /// not accounting": a structure the Quartermaster has paid for goes into
    /// the ground at one hit point with the flag set, and a build drone raises
    /// it. It supplies nothing, draws nothing and has no capability until the
    /// flag clears at full hit points.
    ///
    /// `rotation` is the facing the Build target that paid for it chose
    /// (spec section 6's Build row); [`QuarterTurns::NONE`] for one that was
    /// not built for a target.
    #[allow(
        clippy::too_many_arguments,
        reason = "one argument per column, as `UnitTable::push` above; the same reasoning applies"
    )]
    ///
    /// # Errors
    ///
    /// [`TableFull`] when no row is free and the table holds `room`.
    pub fn place(
        &mut self,
        room: u32,
        seat: SeatId,
        kind: StructureKind,
        pos: [Fx; 3],
        hp: Hp,
        home: BeaconId,
        building: bool,
        rotation: QuarterTurns,
        ready_at: u32,
    ) -> Result<Placed<StructureId>, TableFull> {
        let at = self.vacancy(room)?;
        let id = StructureId::of(at.row, at.generation);
        let row = at.row;
        put(&mut self.id, row, id.raw());
        put(&mut self.seat, row, seat.raw());
        put(&mut self.kind, row, kind.id());
        put(&mut self.pos, row, pos);
        put(&mut self.hp, row, hp);
        put(&mut self.home, row, home);
        put(&mut self.building, row, building);
        put(&mut self.rotation, row, rotation.raw());
        put(&mut self.ruined_at, row, NOT_RUINED);
        put(&mut self.arms, row, Arms::ready_at(ready_at));
        put(&mut self.free, row, false);
        if !at.reused {
            // An appended row is the table's last, at most `Row::MAX`.
            self.count = row.raw().saturating_add(1);
        }
        Ok(Placed {
            id,
            reused: at.reused,
        })
    }

    /// Free `row`: its structure was destroyed and its death has settled, or a
    /// ruin was salvaged or buried. `true` when the row's generation is the
    /// last, so it will never be reused.
    pub fn release(&mut self, row: crate::ids::Row) -> bool {
        let index = row.index();
        let Some(free) = self.free.get_mut(index) else {
            return false;
        };
        *free = true;
        if let Some(slot) = self.seat.get_mut(index) {
            *slot = SeatId::NEUTRAL.raw();
        }
        if let Some(slot) = self.hp.get_mut(index) {
            *slot = Hp::ZERO;
        }
        if let Some(slot) = self.home.get_mut(index) {
            *slot = BeaconId::NONE;
        }
        if let Some(slot) = self.building.get_mut(index) {
            *slot = false;
        }
        if let Some(slot) = self.ruined_at.get_mut(index) {
            *slot = NOT_RUINED;
        }
        if let Some(slot) = self.arms.get_mut(index) {
            *slot = Arms::NONE;
        }
        self.id
            .get(index)
            .and_then(|id| crate::ids::unpack(*id).ok())
            .is_some_and(|(_, generation)| generation.next().is_none())
    }

    /// The row `structure` lives in, checked by generation.
    ///
    /// # Errors
    ///
    /// [`crate::ids::RefError`]: a sentinel, no such row, or a row since freed
    /// or reused.
    pub fn row_of(&self, structure: StructureId) -> Result<crate::ids::Row, crate::ids::RefError> {
        row_of_in(&self.id, &self.free, structure.raw())
    }

    /// The id of the structure in `row`, or `None` for a row the table does
    /// not hold or one that is free.
    #[must_use]
    pub fn id_of(&self, row: crate::ids::Row) -> Option<StructureId> {
        if self.free.get(row.index()).copied() != Some(false) {
            return None;
        }
        self.id.get(row.index()).copied().map(StructureId::new)
    }

    /// The free column ([`UnitTable::frees`]).
    #[must_use]
    pub fn frees(&self) -> &[bool] {
        &self.free
    }

    /// The tick each ruin was ruined at, or [`NOT_RUINED`]: what "the oldest
    /// ruin" means when the ruin reserve is full (S2's plan, E1). Hashed and
    /// snapshotted.
    #[must_use]
    pub fn ruined_at(&self) -> &[u32] {
        &self.ruined_at
    }

    /// The ruined-at column, to write into: item 20's ruination is the one
    /// writer.
    pub fn ruined_at_mut(&mut self) -> &mut [u32] {
        &mut self.ruined_at
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

    /// The position column, to write into: the support rule moves an asset
    /// that stands on a falling region down with it (S2's plan, `fall`).
    pub fn positions_mut(&mut self) -> &mut [[Fx; 3]] {
        &mut self.pos
    }

    /// The hit-point column.
    #[must_use]
    pub fn hit_points(&self) -> &[Hp] {
        &self.hp
    }

    /// The home-beacon column. [`BeaconId::NONE`] means the structure has no
    /// home beacon. A home is a reference into the beacon table, read through
    /// [`BeaconTable::row_of`], which checks the generation: a ruin's home has
    /// died and its row may hold another beacon now.
    #[must_use]
    pub fn homes(&self) -> &[BeaconId] {
        &self.home
    }

    /// The hit-point column, to write into. The combat phase's damage drain
    /// and the build program's construction.
    pub fn hit_points_mut(&mut self) -> &mut [Hp] {
        &mut self.hp
    }

    /// The under-construction column.
    ///
    /// `true` while a structure is going up: paid for in full (item 23), worth
    /// build cost times its current hit points (item 18), and inert until it is
    /// finished.
    #[must_use]
    pub fn building(&self) -> &[bool] {
        &self.building
    }

    /// The rotation column, as [`QuarterTurns::raw`]: the facing the Build
    /// target that paid for each structure chose. Hashed and snapshotted.
    #[must_use]
    pub fn rotations(&self) -> &[u8] {
        &self.rotation
    }

    /// The under-construction column, to write into. The build program clears
    /// it at full hit points.
    pub fn building_mut(&mut self) -> &mut [bool] {
        &mut self.building
    }

    /// The home-beacon column, to write into. Item 20's re-homing.
    pub fn homes_mut(&mut self) -> &mut [BeaconId] {
        &mut self.home
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

    /// The combat memory column ([`Arms`]): an armed structure's cooldown,
    /// and every structure's attacker.
    #[must_use]
    pub fn arms(&self) -> &[Arms] {
        &self.arms
    }

    /// The combat memory column, to write into: the combat phase.
    pub fn arms_mut(&mut self) -> &mut [Arms] {
        &mut self.arms
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
            || columns.building.len() != n
            || columns.rotation.len() != n
            || columns.ruined_at.len() != n
            || columns.arms.len() != n
            || columns.free.len() != n
            || !ids_name_their_rows(&columns.id)
        {
            return false;
        }
        if !columns
            .rotation
            .iter()
            .all(|turns| QuarterTurns::new(u32::from(*turns)).is_some())
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
        self.building = columns.building;
        self.rotation = columns.rotation;
        self.ruined_at = columns.ruined_at;
        self.arms = columns.arms;
        self.free = columns.free;
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
    left_at: Vec<u32>,
    free: Vec<bool>,
}

impl WreckTable {
    /// An empty table sized for `count` wrecks.
    #[must_use]
    pub fn with_capacity(count: u32) -> WreckTable {
        let n = crate::math::index::row(count);
        WreckTable {
            count: 0,
            id: Vec::with_capacity(n),
            pos: Vec::with_capacity(n),
            salvage: Vec::with_capacity(n),
            left_at: Vec::with_capacity(n),
            free: Vec::with_capacity(n),
        }
    }

    /// Make room for `extra` more wrecks without growing later. Same contract
    /// as [`BeaconTable::reserve`].
    pub fn reserve(&mut self, extra: u32) {
        let n = crate::math::index::row(extra);
        self.id.reserve(n);
        self.pos.reserve(n);
        self.salvage.reserve(n);
        self.left_at.reserve(n);
        self.free.reserve(n);
    }

    /// Place one wreck in the lowest free row within `room` rows, or on the
    /// end ([`vacancy_in`]).
    ///
    /// # Errors
    ///
    /// [`TableFull`] when no row is free and the table holds `room`.
    /// `left_at` is the tick it was left on, which decides the oldest wreck a
    /// full table buries (S2's plan, decision 25).
    pub fn place(
        &mut self,
        room: u32,
        pos: [Fx; 3],
        salvage: Money,
        left_at: u32,
    ) -> Result<Placed<WreckId>, TableFull> {
        let at = vacancy_in(&self.id, &self.free, room)?;
        let id = WreckId::of(at.row, at.generation);
        let row = at.row;
        put(&mut self.id, row, id.raw());
        put(&mut self.pos, row, pos);
        put(&mut self.salvage, row, salvage);
        put(&mut self.left_at, row, left_at);
        put(&mut self.free, row, false);
        if !at.reused {
            // An appended row is the table's last, at most `Row::MAX`.
            self.count = row.raw().saturating_add(1);
        }
        Ok(Placed {
            id,
            reused: at.reused,
        })
    }

    /// Free `row`: its wreck has been picked clean. `true` when the row's
    /// generation is the last, so it will never be reused.
    pub fn release(&mut self, row: crate::ids::Row) -> bool {
        let index = row.index();
        let Some(free) = self.free.get_mut(index) else {
            return false;
        };
        *free = true;
        if let Some(slot) = self.salvage.get_mut(index) {
            *slot = Money::ZERO;
        }
        if let Some(slot) = self.left_at.get_mut(index) {
            *slot = 0;
        }
        self.id
            .get(index)
            .and_then(|id| crate::ids::unpack(*id).ok())
            .is_some_and(|(_, generation)| generation.next().is_none())
    }

    /// The free column ([`UnitTable::frees`]).
    #[must_use]
    pub fn frees(&self) -> &[bool] {
        &self.free
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

    /// The position column, to write into: the support rule moves an asset
    /// that stands on a falling region down with it (S2's plan, `fall`).
    pub fn positions_mut(&mut self) -> &mut [[Fx; 3]] {
        &mut self.pos
    }

    /// The salvage-value column, in `$`.
    #[must_use]
    pub fn salvages(&self) -> &[Money] {
        &self.salvage
    }

    /// The salvage-value column, to write into. A reclaim drone lifting a
    /// wreck drains it; a wreck at zero is picked clean.
    pub fn salvages_mut(&mut self) -> &mut [Money] {
        &mut self.salvage
    }

    /// The tick each wreck was left on (decision 25: a full table buries the
    /// oldest unsalvaged wreck); zero on a free row.
    #[must_use]
    pub fn left_at(&self) -> &[u32] {
        &self.left_at
    }

    /// Replace the whole table from a restored snapshot. Same contract as
    /// [`UnitTable::restore`].
    pub fn restore(
        &mut self,
        id: Vec<u32>,
        pos: Vec<[Fx; 3]>,
        salvage: Vec<Money>,
        left_at: Vec<u32>,
        free: Vec<bool>,
    ) -> bool {
        let n = id.len();
        if pos.len() != n
            || salvage.len() != n
            || left_at.len() != n
            || free.len() != n
            || !ids_name_their_rows(&id)
        {
            return false;
        }
        let Ok(count) = u32::try_from(n) else {
            return false;
        };
        self.count = count;
        self.id = id;
        self.pos = pos;
        self.salvage = salvage;
        self.left_at = left_at;
        self.free = free;
        true
    }
}

/// Which weapon fired a shell ([`ShellTable`]): the core's own autocannon, or
/// the weapon of a unit or structure kind's rules row. A wire byte, additive
/// only: the core is `1`, a unit kind `0x10` plus its [`UnitKind::id`] and a
/// structure kind `0x20` plus its [`StructureKind::id`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Armament {
    /// A seat's core, `beacon.core_weapon`.
    Core,
    /// A unit kind's `weapon` row.
    Unit(UnitKind),
    /// A structure kind's `weapon` row.
    Structure(StructureKind),
}

impl Armament {
    /// The wire byte.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            Armament::Core => 1,
            Armament::Unit(kind) => 0x10 | kind.id(),
            Armament::Structure(kind) => 0x20 | kind.id(),
        }
    }

    /// The armament a byte names, or `None` for one this build does not.
    #[must_use]
    pub const fn from_id(id: u8) -> Option<Armament> {
        match id {
            1 => Some(Armament::Core),
            0x11..=0x1F => match UnitKind::from_id(id & 0x0F) {
                Some(kind) => Some(Armament::Unit(kind)),
                None => None,
            },
            0x21..=0x2F => match StructureKind::from_id(id & 0x0F) {
                Some(kind) => Some(Armament::Structure(kind)),
                None => None,
            },
            _ => None,
        }
    }
}

/// A shell could not be filed: the table holds its whole room
/// ([`ShellTable::push`]). The room is sized so that no legal world fills it
/// (S2's plan, E13), so this is a typed error, propagated, never a drop.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ShellsFull {
    /// How many shells the table holds.
    pub room: u32,
}

impl core::fmt::Display for ShellsFull {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "the shell table holds all {} of its shells", self.room)
    }
}

impl std::error::Error for ShellsFull {}

/// The shells in flight, structure-of-arrays (S2's plan, E13): **hashed
/// entities**, in the hash, the snapshot and the goldens.
///
/// Each row is a shell fired and not yet landed: the point it was aimed at,
/// fixed when it was fired (no lead: a unit that keeps walking is missed,
/// decision 3), the tick it lands on, its firer's seat, its firer with the
/// id's generation, and its weapon. Rows are kept in filing order, which is
/// the firing order (seat, kind, id; E12), and a landing removes its rows in
/// place, so the order is a total one and nothing allocates.
///
/// The room is fixed at construction from firings, not from mortars, because a
/// dead mortar's shells stay in flight while its row is reused (E1 and E13);
/// the world sizes it ([`crate::world::World::shells`]).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ShellTable {
    room: u32,
    aim: Vec<[Fx; 3]>,
    lands_at: Vec<u32>,
    launch: Vec<[Fx; 3]>,
    fired_at: Vec<u32>,
    seat: Vec<u8>,
    firer: Vec<u32>,
    weapon: Vec<u8>,
}

impl ShellTable {
    /// The table's tag in the state hash (S2's plan, E29): a non-empty table
    /// writes it before its count, so two new tables of one row shape never
    /// hash alike.
    pub const HASH_TAG: u8 = 1;

    /// An empty table with room for `room` shells. Nothing allocates after
    /// this.
    #[must_use]
    pub fn with_room(room: u32) -> ShellTable {
        let n = crate::math::index::row(room);
        ShellTable {
            room,
            aim: Vec::with_capacity(n),
            lands_at: Vec::with_capacity(n),
            launch: Vec::with_capacity(n),
            fired_at: Vec::with_capacity(n),
            seat: Vec::with_capacity(n),
            firer: Vec::with_capacity(n),
            weapon: Vec::with_capacity(n),
        }
    }

    /// How many shells the table may hold.
    #[must_use]
    pub const fn room(&self) -> u32 {
        self.room
    }

    /// How many shells are in flight.
    #[must_use]
    pub fn len(&self) -> usize {
        self.aim.len()
    }

    /// How many shells are in flight, as the `u32` the room is: never more
    /// than [`ShellTable::room`], which [`ShellTable::push`] and
    /// [`ShellTable::restore`] hold it to.
    #[must_use]
    pub fn count(&self) -> u32 {
        // E7: cannot miss: the table never holds more than its `u32` room
        u32::try_from(self.aim.len()).unwrap_or(self.room)
    }

    /// Whether no shell is in flight.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.aim.is_empty()
    }

    /// File one shell: where it lands and when, and where it was fired from
    /// and when (`launch`, the firer's point and the fire tick; S2's `site`, so
    /// a shell can be drawn in flight, [`ShellTable::position_at`]).
    ///
    /// # Errors
    ///
    /// [`ShellsFull`] when the table holds its room.
    pub fn push(
        &mut self,
        aim: [Fx; 3],
        lands_at: u32,
        launch: ([Fx; 3], u32),
        seat: SeatId,
        firer: u32,
        weapon: Armament,
    ) -> Result<(), ShellsFull> {
        let full = ShellsFull { room: self.room };
        let held = u32::try_from(self.aim.len()).map_err(|_| full)?;
        if held >= self.room {
            return Err(full);
        }
        self.aim.push(aim);
        self.lands_at.push(lands_at);
        self.launch.push(launch.0);
        self.fired_at.push(launch.1);
        self.seat.push(seat.raw());
        self.firer.push(firer);
        self.weapon.push(weapon.id());
        Ok(())
    }

    /// Remove every shell that lands on or before `tick`, keeping the rest in
    /// order.
    /// Compacted in place, column by column, so nothing allocates.
    pub fn remove_landed(&mut self, tick: u32) {
        let mut kept: usize = 0;
        let mut at: usize = 0;
        while let Some(lands) = self.lands_at.get(at).copied() {
            if lands > tick {
                if kept != at {
                    self.aim.swap(kept, at);
                    self.lands_at.swap(kept, at);
                    self.launch.swap(kept, at);
                    self.fired_at.swap(kept, at);
                    self.seat.swap(kept, at);
                    self.firer.swap(kept, at);
                    self.weapon.swap(kept, at);
                }
                kept = kept.saturating_add(1);
            }
            at = at.saturating_add(1);
        }
        self.aim.truncate(kept);
        self.lands_at.truncate(kept);
        self.launch.truncate(kept);
        self.fired_at.truncate(kept);
        self.seat.truncate(kept);
        self.firer.truncate(kept);
        self.weapon.truncate(kept);
    }

    /// The aim-point column.
    #[must_use]
    pub fn aims(&self) -> &[[Fx; 3]] {
        &self.aim
    }

    /// The landing-tick column.
    #[must_use]
    pub fn lands_at(&self) -> &[u32] {
        &self.lands_at
    }

    /// The launch-point column: where each shell was fired from (S2's `site`).
    #[must_use]
    pub fn launches(&self) -> &[[Fx; 3]] {
        &self.launch
    }

    /// The fire-tick column: the tick each shell was fired on.
    #[must_use]
    pub fn fired_at(&self) -> &[u32] {
        &self.fired_at
    }

    /// Where the shell in `row` is at `tick`: the straight-line point between
    /// its launch point and its aim point, in proportion to the ticks flown,
    /// its launch point before it is fired and its aim point from the tick it
    /// lands. A pure read of the row, in integer `Fx`; `None` for a row the
    /// table does not hold. "Slow and visible in flight" (S2's plan, step 4):
    /// what a client draws, which the gateway's view reads.
    #[must_use]
    pub fn position_at(&self, row: usize, tick: u32) -> Option<[Fx; 3]> {
        let aim = self.aim.get(row).copied()?;
        let lands = self.lands_at.get(row).copied()?;
        let launch = self.launch.get(row).copied()?;
        let fired = self.fired_at.get(row).copied()?;
        if tick <= fired {
            return Some(launch);
        }
        if tick >= lands {
            return Some(aim);
        }
        let flown = i64::from(tick.checked_sub(fired)?);
        let whole = i64::from(lands.checked_sub(fired)?);
        let mut at = launch;
        for (axis, (from, to)) in at.iter_mut().zip(launch.iter().zip(aim)) {
            let span = i64::from(to.raw()).checked_sub(i64::from(from.raw()))?;
            let step = span.checked_mul(flown)?.checked_div(whole)?;
            let raw = i64::from(from.raw()).checked_add(step)?;
            *axis = Fx::from_raw(i32::try_from(raw).ok()?);
        }
        Some(at)
    }

    /// The firer's-seat column.
    #[must_use]
    pub fn seats(&self) -> &[u8] {
        &self.seat
    }

    /// The firer column, [`crate::knowledge::AssetId`] raw values.
    #[must_use]
    pub fn firers(&self) -> &[u32] {
        &self.firer
    }

    /// The weapon column, [`Armament::id`] bytes.
    #[must_use]
    pub fn weapons(&self) -> &[u8] {
        &self.weapon
    }

    /// Replace the whole table from a restored snapshot. `false`, and nothing
    /// changed, when the columns disagree in length or hold more than the
    /// room.
    pub fn restore(
        &mut self,
        aim: Vec<[Fx; 3]>,
        lands_at: Vec<u32>,
        launch: (Vec<[Fx; 3]>, Vec<u32>),
        seat: Vec<u8>,
        firer: Vec<u32>,
        weapon: Vec<u8>,
    ) -> bool {
        let n = aim.len();
        let (launch, fired_at) = launch;
        if lands_at.len() != n
            || launch.len() != n
            || fired_at.len() != n
            || fired_at
                .iter()
                .zip(&lands_at)
                .any(|(fired, lands)| fired > lands)
            || seat.len() != n
            || firer.len() != n
            || weapon.len() != n
            || !u32::try_from(n).is_ok_and(|n| n <= self.room)
        {
            return false;
        }
        self.aim.clear();
        self.aim.extend(aim);
        self.lands_at.clear();
        self.lands_at.extend(lands_at);
        self.launch.clear();
        self.launch.extend(launch);
        self.fired_at.clear();
        self.fired_at.extend(fired_at);
        self.seat.clear();
        self.seat.extend(seat);
        self.firer.clear();
        self.firer.extend(firer);
        self.weapon.clear();
        self.weapon.extend(weapon);
        true
    }
}

/// How far a Defend beacon's guards follow a threat (`DefendSettings.Pursue`,
/// spec section 6's Defend row).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum Pursue {
    /// Not written: a Defend beacon placed or switched with an empty arm,
    /// which reads as [`Pursue::Never`] ([`DefendSettings::pursue_reading`]).
    #[default]
    Unset,
    /// A guard never leaves the engagement radius.
    Never,
    /// A guard follows a threat as far as the sphere edge.
    SphereEdge,
    /// A guard follows a threat while its seat sees it.
    WhileInReach,
}

impl Pursue {
    /// The wire value (`DefendSettings.Pursue`), which is also the hashed byte.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            Pursue::Unset => 0,
            Pursue::Never => 1,
            Pursue::SphereEdge => 2,
            Pursue::WhileInReach => 3,
        }
    }

    /// The setting a wire value names, or `None` for one this build does not
    /// name.
    #[must_use]
    pub const fn from_id(id: u8) -> Option<Pursue> {
        match id {
            0 => Some(Pursue::Unset),
            1 => Some(Pursue::Never),
            2 => Some(Pursue::SphereEdge),
            3 => Some(Pursue::WhileInReach),
            _ => None,
        }
    }
}

/// A Defend beacon's settings as the sim holds them (spec section 6's Defend
/// row; S2's `dfnd`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DefendSettings {
    /// The engagement radius as a whole percentage of the sphere radius,
    /// 1 to 100, or 0 when no row has written it.
    pub engagement_pct: u8,
    /// How far a guard follows a threat.
    pub pursue: Pursue,
    /// How many scouts picket just outside the sphere edge: Defend's own
    /// column, a byte (the verifier's `picket_scouts` ceiling, register
    /// S2-18).
    pub pickets: u8,
    /// Whether the protect list holds `my_commander`.
    pub escort: bool,
}

impl DefendSettings {
    /// The engagement radius the beacon's guards read, as a percentage: the
    /// one written, or the whole sphere when none is (an empty Defend arm,
    /// legal at a deploy and a switch, decisions-log item 145 (5)).
    ///
    /// PLACEHOLDER: an empty Defend arm reads its engagement radius as the whole sphere and its pursue as NEVER — owner, S2 demo.
    #[must_use]
    pub const fn engagement_reading(self) -> u8 {
        if self.engagement_pct == 0 {
            100
        } else {
            self.engagement_pct
        }
    }

    /// The pursue setting the beacon's guards read: the one written, or
    /// [`Pursue::Never`] when none is (see
    /// [`DefendSettings::engagement_reading`]).
    #[must_use]
    pub const fn pursue_reading(self) -> Pursue {
        match self.pursue {
            Pursue::Unset => Pursue::Never,
            other => other,
        }
    }
}

/// No quarry: a Defend beacon whose guards have no threat to go after.
pub const NO_QUARRY: u32 = u32::MAX;

/// One row of the Defend table: a Defend beacon, its settings and its state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DefendRow {
    /// The beacon, with its generation.
    pub beacon: BeaconId,
    /// Its settings.
    pub settings: DefendSettings,
    /// The guard assignment: the enemy unit every guard of the beacon goes
    /// after, an [`crate::knowledge::AssetId`] raw value, or [`NO_QUARRY`].
    pub quarry: u32,
    /// The picket's leg: which of the ring's points the first picket walks
    /// to, the others spaced evenly after it.
    pub leg: u8,
}

/// Every Defend beacon's settings and state (S2's `dfnd`): one row per live
/// Defend beacon, in ascending beacon id, and the protect list's beacons as
/// `(defend beacon, protected beacon)` pairs, grouped by the defending beacon
/// in ascending id and in list order within a group.
///
/// **Hashed as one block that writes nothing while the table holds no row**
/// (S2's plan, E29), so a match with no Defend beacon hashes as it did before
/// the table existed. A row is opened when a beacon takes the Defend writ and
/// closed when it leaves it or dies (the world's `set_beacon_mandate`,
/// `place_beacon` and `end_mandate`), so a row always names a live Defend
/// beacon.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct DefendTable {
    rows: Vec<DefendRow>,
    protected: Vec<(BeaconId, BeaconId)>,
}

impl DefendTable {
    /// The table's tag in the state hash (E29), after the shell table's 1 and
    /// the site block's 2.
    pub const HASH_TAG: u8 = 3;

    /// An empty table with room for `beacons` rows, so a row opened inside a
    /// tick allocates nothing.
    #[must_use]
    pub fn with_room(beacons: u32) -> DefendTable {
        DefendTable {
            rows: Vec::with_capacity(crate::math::index::row(beacons)),
            protected: Vec::new(),
        }
    }

    /// The rows, in ascending beacon id.
    #[must_use]
    pub fn rows(&self) -> &[DefendRow] {
        &self.rows
    }

    /// The protect list's beacons, `(defend beacon, protected beacon)`.
    #[must_use]
    pub fn protected(&self) -> &[(BeaconId, BeaconId)] {
        &self.protected
    }

    /// Whether the table holds no row (and so no protected beacon, which
    /// [`DefendTable::restore`] and [`DefendTable::close`] keep true).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty() && self.protected.is_empty()
    }

    fn index_of(&self, beacon: BeaconId) -> Result<usize, usize> {
        self.rows
            .binary_search_by_key(&beacon.raw(), |row| row.beacon.raw())
    }

    /// The row of `beacon`, if it is a Defend beacon.
    #[must_use]
    pub fn row_of(&self, beacon: BeaconId) -> Option<&DefendRow> {
        self.index_of(beacon).ok().and_then(|at| self.rows.get(at))
    }

    /// The row of `beacon`, to write.
    pub fn row_mut(&mut self, beacon: BeaconId) -> Option<&mut DefendRow> {
        let at = self.index_of(beacon).ok()?;
        self.rows.get_mut(at)
    }

    /// Give `beacon` a row at the defaults, or leave the one it has: a beacon
    /// that takes the Defend writ.
    pub fn open(&mut self, beacon: BeaconId) {
        if let Err(at) = self.index_of(beacon) {
            self.rows.insert(
                at,
                DefendRow {
                    beacon,
                    settings: DefendSettings::default(),
                    quarry: NO_QUARRY,
                    leg: 0,
                },
            );
        }
    }

    /// Remove `beacon`'s row and its protect list: a beacon that leaves the
    /// Defend writ or dies. A protect entry naming it stays: a protected
    /// beacon that changes its writ is still protected.
    pub fn close(&mut self, beacon: BeaconId) {
        if let Ok(at) = self.index_of(beacon) {
            self.rows.remove(at);
        }
        self.protected.retain(|(by, _)| *by != beacon);
    }

    /// Remove every protect entry naming `beacon`: a beacon that died.
    pub fn forget(&mut self, beacon: BeaconId) {
        self.protected.retain(|(_, of)| *of != beacon);
    }

    /// Replace `beacon`'s protect list's beacons with `list`, in order. Does
    /// nothing for a beacon with no row.
    pub fn set_protected(&mut self, beacon: BeaconId, list: &[BeaconId]) {
        if self.index_of(beacon).is_err() {
            return;
        }
        self.protected.retain(|(by, _)| *by != beacon);
        let at = self
            .protected
            .partition_point(|(by, _)| by.raw() < beacon.raw());
        let mut index = at;
        for protected in list {
            self.protected.insert(index, (beacon, *protected));
            index = index.saturating_add(1);
        }
    }

    /// The beacons `beacon` protects, in list order.
    pub fn protected_by(&self, beacon: BeaconId) -> impl Iterator<Item = BeaconId> + '_ {
        self.protected
            .iter()
            .filter(move |(by, _)| *by == beacon)
            .map(|(_, of)| *of)
    }

    /// Replace the table from a restored snapshot. `false`, changing nothing,
    /// when the rows are not in strictly ascending beacon id, or the protect
    /// list is not grouped in ascending order or names a defending beacon
    /// with no row.
    pub fn restore(&mut self, rows: Vec<DefendRow>, protected: Vec<(BeaconId, BeaconId)>) -> bool {
        let ascending = rows
            .windows(2)
            .all(|pair| matches!(pair, [a, b] if a.beacon.raw() < b.beacon.raw()));
        let grouped = protected
            .windows(2)
            .all(|pair| matches!(pair, [a, b] if a.0.raw() <= b.0.raw()));
        let owned = protected.iter().all(|(by, _)| {
            rows.binary_search_by_key(&by.raw(), |row| row.beacon.raw())
                .is_ok()
        });
        if !ascending || !grouped || !owned {
            return false;
        }
        self.rows.clear();
        self.rows.extend(rows);
        self.protected = protected;
        true
    }
}

/// Every seat's explored coarse cells (S2's `surv`, laid out by `dfnd`): one
/// row per `(seat, cell)` a seat has explored, ascending. The cell's meaning
/// -- its size, and how a scout marks one -- is `surv`'s; this table holds
/// the marks, and nothing writes one yet.
///
/// **Hashed as one block that writes nothing while the table holds no row**
/// (S2's plan, E29).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ExploredTable {
    rows: Vec<(u8, u32)>,
}

impl ExploredTable {
    /// The table's tag in the state hash (E29).
    pub const HASH_TAG: u8 = 4;

    /// The rows, `(seat, cell)`, ascending.
    #[must_use]
    pub fn rows(&self) -> &[(u8, u32)] {
        &self.rows
    }

    /// Whether no seat has explored a cell.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Whether `seat` has explored `cell`.
    #[must_use]
    pub fn contains(&self, seat: SeatId, cell: u32) -> bool {
        self.rows.binary_search(&(seat.raw(), cell)).is_ok()
    }

    /// Mark `cell` explored for `seat`: `true` when the mark is new.
    pub fn mark(&mut self, seat: SeatId, cell: u32) -> bool {
        match self.rows.binary_search(&(seat.raw(), cell)) {
            Ok(_) => false,
            Err(at) => {
                self.rows.insert(at, (seat.raw(), cell));
                true
            }
        }
    }

    /// Replace the table from a restored snapshot. `false`, changing nothing,
    /// when the rows are not strictly ascending.
    pub fn restore(&mut self, rows: Vec<(u8, u32)>) -> bool {
        if !rows.windows(2).all(|pair| matches!(pair, [a, b] if a < b)) {
            return false;
        }
        self.rows = rows;
        true
    }
}
