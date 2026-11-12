// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Ledger's licences (spec section 8; S3's plan, E2 and decision 6; S3's
//! `lic`).
//!
//! "Capabilities are Ledger licences, earned by meeting its conditions; the
//! conditions are checked at the settlement and announced in the recap." S3
//! builds the general mechanism with one entry, the Resonance Spire
//! (decisions-log item 153 (3)); S4 adds the others as new values of
//! `gp.v1.Licence` and new triggers beside [`settle`]'s one.
//!
//! # The table
//!
//! A sparse table of `(seat, licence, round_awarded)` rows, ascending by seat
//! then licence ([`LicenceTable`]). A licence is permanent once awarded (spec
//! section 8), so a row is only ever added at a settlement and removed when its
//! seat is eliminated ("An eliminated seat's licences are voided"). The row
//! stores the licence's `gp.v1.Licence` wire value, the one enum the sim, the
//! verifier and the gateway's wire share, so no layer spells a licence as a
//! string.
//!
//! **Hashed state** (AGENTS.md section 4.8): a block after the Attack block
//! that writes nothing while the table holds no row (item 136's block rule),
//! so a chain whose seats earn nothing does not move for the table; carried in
//! the snapshot from version 15 ([`crate::snapshot::SNAPSHOT_VERSION`]).
//!
//! # The trigger
//!
//! Decision 6's reading of "Running Generators on 2 vents at a settlement":
//! the licence is earned when, at the settlement, `licences.resonance_vents`
//! of the seat's finished, powered Generators stand on as many **different**
//! vents. "Finished and powered" is exactly what the power phase counts as a
//! tap ([`crate::power::tapped_vents`]), so a Generator still going up, one
//! homed to a dark beacon and a second Generator on a vent already tapped each
//! count for nothing here, as they supply nothing there.
//!
//! The check runs once per settlement, after the Ledger's income, in seat
//! order ([`crate::world::World`]'s segment close), for every seat still in
//! the match, and awards each licence a seat does not yet hold.

use pharmakos_proto::gp::v1::Licence;

use crate::tables::SeatId;

/// Every licence this build can award, in ascending wire value.
///
/// S3's one entry (decisions-log item 153 (3)); S4 adds the others at the end.
pub const AWARDED: [Licence; 1] = [Licence::ResonanceSpire];

/// The wire value of `licence` as the table stores it, or `None` for the
/// unspecified zero and any value past a byte.
#[must_use]
pub fn id_of(licence: Licence) -> Option<u8> {
    if licence == Licence::Unspecified {
        return None;
    }
    u8::try_from(i32::from(licence)).ok()
}

/// The licence a stored value names, or `None` for zero or a value this build
/// does not define.
#[must_use]
pub fn licence_of(id: u8) -> Option<Licence> {
    Licence::try_from(i32::from(id))
        .ok()
        .filter(|licence| *licence != Licence::Unspecified)
}

/// One row of the [`LicenceTable`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct LicenceRow {
    /// The seat holding it.
    pub seat: u8,
    /// The licence's `gp.v1.Licence` wire value, never zero.
    pub licence: u8,
    /// The round whose settlement awarded it, from 1.
    pub round: u32,
}

/// Why a restored licence table was refused ([`LicenceTable::restore`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LicenceRowError {
    /// The rows are not strictly ascending by seat then licence: a seat holds
    /// a licence twice, or the order is not the one every writer keeps.
    Order,
    /// A row names a licence this build does not define, or the zero.
    UnknownLicence,
    /// A row names round zero: a licence is awarded at a round's settlement.
    RoundZero,
    /// The rows would not fit the room the world reserved for them.
    Room,
}

/// Every seat's licences, ascending by seat then licence (S3's plan, E2).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct LicenceTable {
    rows: Vec<LicenceRow>,
}

impl LicenceTable {
    /// The table's tag in the state hash, after the Attack block's 5 (E2,
    /// item 136's block rule).
    pub const HASH_TAG: u8 = 6;

    /// An empty table with room for every licence of `seats` seats, so an
    /// award at a settlement allocates nothing inside the tick
    /// (`tests/allocations.rs`).
    #[must_use]
    pub fn with_room(seats: u32) -> LicenceTable {
        LicenceTable {
            rows: Vec::with_capacity(room_for(seats)),
        }
    }

    /// The rows, ascending by seat then licence.
    #[must_use]
    pub fn rows(&self) -> &[LicenceRow] {
        &self.rows
    }

    /// Whether no seat holds a licence.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Whether `seat` holds `licence`.
    #[must_use]
    pub fn holds(&self, seat: SeatId, licence: Licence) -> bool {
        id_of(licence).is_some_and(|id| self.position(seat.raw(), id).is_ok())
    }

    /// Every licence `seat` holds, ascending.
    pub fn held_by(&self, seat: SeatId) -> impl Iterator<Item = Licence> + '_ {
        self.rows
            .iter()
            .filter(move |row| row.seat == seat.raw())
            .filter_map(|row| licence_of(row.licence))
    }

    /// Award `licence` to `seat` at `round`: `true` when the row is new,
    /// `false` when the seat already held it (a licence is permanent and is
    /// never re-awarded) or `licence` is the unspecified zero.
    pub fn award(&mut self, seat: SeatId, licence: Licence, round: u32) -> bool {
        let Some(id) = id_of(licence) else {
            return false;
        };
        match self.position(seat.raw(), id) {
            Ok(_) => false,
            Err(at) => {
                self.rows.insert(
                    at,
                    LicenceRow {
                        seat: seat.raw(),
                        licence: id,
                        round,
                    },
                );
                true
            }
        }
    }

    /// Void every licence `seat` holds (spec section 8: "An eliminated seat's
    /// licences are voided"), writing each voided licence's wire value into
    /// `out` in ascending order. `out` is the caller's, cleared first, so the
    /// tick allocates nothing.
    pub fn void_seat(&mut self, seat: SeatId, out: &mut Vec<u8>) {
        out.clear();
        for row in &self.rows {
            if row.seat == seat.raw() {
                out.push(row.licence);
            }
        }
        self.rows.retain(|row| row.seat != seat.raw());
    }

    /// Make room for every licence of `seats` seats.
    pub fn reserve_for(&mut self, seats: u32) {
        let room = room_for(seats);
        self.rows.reserve(room.saturating_sub(self.rows.len()));
    }

    /// Replace the table from a restored snapshot, checked before anything is
    /// written: strictly ascending by seat then licence, every licence one
    /// this build defines, every round from 1, and no more rows than `seats`
    /// seats can hold. The caller checks each seat byte against its own seat
    /// table.
    ///
    /// # Errors
    ///
    /// [`LicenceRowError`] naming the first check that fails; the table is
    /// left as it was.
    pub fn restore(&mut self, rows: Vec<LicenceRow>, seats: u32) -> Result<(), LicenceRowError> {
        if rows.iter().any(|row| licence_of(row.licence).is_none()) {
            return Err(LicenceRowError::UnknownLicence);
        }
        if rows.iter().any(|row| row.round == 0) {
            return Err(LicenceRowError::RoundZero);
        }
        if !rows
            .windows(2)
            .all(|pair| matches!(pair, [a, b] if (a.seat, a.licence) < (b.seat, b.licence)))
        {
            return Err(LicenceRowError::Order);
        }
        if rows.len() > room_for(seats) {
            return Err(LicenceRowError::Room);
        }
        let mut rows = rows;
        rows.reserve(room_for(seats).saturating_sub(rows.len()));
        self.rows = rows;
        Ok(())
    }

    fn position(&self, seat: u8, licence: u8) -> Result<usize, usize> {
        // item 62: the key is `(seat, licence)`, which a seat holds once, so it
        // is unique and the order total.
        self.rows
            .binary_search_by(|row| (row.seat, row.licence).cmp(&(seat, licence)))
    }
}

/// The Ledger's licence check at a settlement (S3's plan, E2): for every seat
/// still in the match, in seat order, award each licence of [`AWARDED`] whose
/// trigger holds and which the seat does not yet hold, with one
/// `licence_awarded` event per award (`seat` the seat, `value` the licence's
/// wire value).
///
/// Called inside the tick that opens the recap, after the Ledger's income
/// ([`crate::world::World`]'s settlement), so the award is in that tick's
/// hash and in the frozen snapshot the next Lull plans against, exactly as the
/// income is.
pub(crate) fn settle(world: &mut crate::world::World) {
    let tick = world.tick();
    let round = world.match_state().round();
    let vents = world.rules().resonance_vents();
    let seats = crate::math::index::row(world.seats().len());
    let mut index: usize = 0;
    while index < seats {
        if world.is_in_match(index)
            && let Some(raw) = world.seats().seats().get(index).copied()
        {
            let seat = SeatId::new(raw);
            if crate::power::tapped_vents(world, seat) >= vents.get()
                && world
                    .licences_mut()
                    .award(seat, Licence::ResonanceSpire, round)
                && let Some(id) = id_of(Licence::ResonanceSpire)
            {
                world.emit(
                    tick,
                    crate::events::Emission::of(crate::events::EventKind::LicenceAwarded)
                        .seat(seat)
                        .value(i64::from(id)),
                );
            }
        }
        index = index.saturating_add(1);
    }
}

/// The most rows `seats` seats can hold: one per seat per licence this build
/// awards.
fn room_for(seats: u32) -> usize {
    crate::math::index::row(seats).saturating_mul(AWARDED.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_award_is_permanent_and_never_repeated() {
        let mut table = LicenceTable::with_room(3);
        assert!(table.is_empty());
        assert!(table.award(SeatId::new(1), Licence::ResonanceSpire, 2));
        assert!(!table.award(SeatId::new(1), Licence::ResonanceSpire, 3));
        assert!(!table.award(SeatId::new(0), Licence::Unspecified, 3));
        assert!(table.award(SeatId::new(0), Licence::ResonanceSpire, 3));
        assert_eq!(
            table.rows(),
            &[
                LicenceRow {
                    seat: 0,
                    licence: 1,
                    round: 3
                },
                LicenceRow {
                    seat: 1,
                    licence: 1,
                    round: 2
                },
            ],
            "ascending by seat, the first award's round kept"
        );
        assert!(table.holds(SeatId::new(1), Licence::ResonanceSpire));
        assert!(!table.holds(SeatId::new(2), Licence::ResonanceSpire));
    }

    #[test]
    fn voiding_a_seat_removes_only_its_rows() {
        let mut table = LicenceTable::with_room(3);
        table.award(SeatId::new(0), Licence::ResonanceSpire, 2);
        table.award(SeatId::new(2), Licence::ResonanceSpire, 2);
        let mut voided = Vec::with_capacity(1);
        table.void_seat(SeatId::new(2), &mut voided);
        assert_eq!(voided, vec![1]);
        assert!(!table.holds(SeatId::new(2), Licence::ResonanceSpire));
        assert!(table.holds(SeatId::new(0), Licence::ResonanceSpire));
        table.void_seat(SeatId::new(1), &mut voided);
        assert!(voided.is_empty(), "a seat with no licence voids nothing");
    }

    #[test]
    fn a_restore_refuses_rows_no_writer_makes() {
        let row = |seat, licence, round| LicenceRow {
            seat,
            licence,
            round,
        };
        let mut table = LicenceTable::default();
        assert_eq!(
            table.restore(vec![row(1, 1, 2), row(0, 1, 2)], 2),
            Err(LicenceRowError::Order)
        );
        assert_eq!(
            table.restore(vec![row(0, 1, 2), row(0, 1, 3)], 2),
            Err(LicenceRowError::Order),
            "a seat holds a licence once"
        );
        assert_eq!(
            table.restore(vec![row(0, 0, 2)], 2),
            Err(LicenceRowError::UnknownLicence)
        );
        assert_eq!(
            table.restore(vec![row(0, 200, 2)], 2),
            Err(LicenceRowError::UnknownLicence)
        );
        assert_eq!(
            table.restore(vec![row(0, 1, 0)], 2),
            Err(LicenceRowError::RoundZero)
        );
        assert_eq!(
            table.restore(vec![row(0, 1, 1), row(1, 1, 1)], 1),
            Err(LicenceRowError::Room)
        );
        assert!(table.is_empty(), "a refused restore writes nothing");
        assert_eq!(table.restore(vec![row(0, 1, 1), row(1, 1, 4)], 2), Ok(()));
        assert_eq!(table.rows().len(), 2);
    }

    #[test]
    fn every_awarded_licence_has_a_byte() {
        for licence in AWARDED {
            assert_eq!(id_of(licence).and_then(licence_of), Some(licence));
        }
        assert_eq!(id_of(Licence::Unspecified), None);
    }
}
