// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The final audit: who wins a match the last-seat-standing rule did not
//! decide (spec section 3; decisions-log items 4, 16 and 18; register X-08).
//!
//! Spec section 3: "Round-limit winner, the final audit: score = value held
//! after the final settlement (`$` plus the build cost of your beacons,
//! structures and units, each counted at build cost × current HP) + the build
//! cost of enemy assets you destroyed", and the tie-break order "unsmoothed
//! net worth at the final audit, then enemy value destroyed, then fewer
//! beacons lost, then a shared win". Item 16: "If no seat survives that tick,
//! the final audit at that tick decides", and "there is no draw state".
//!
//! # One function over hashed state, and no new state
//!
//! [`final_audit`] reads the treasury, the tables' hit points and the beacon
//! table, all of which are already hashed and snapshotted, and it writes
//! nothing. The runner records the one winner it names in
//! [`crate::runner::MatchOutcome::winner`] — `None` on a shared win, which is
//! the byte the match state already hashed — and the recap and the gateway
//! call the same function again for the terms and the tied set. So the tied set
//! is never stored, and cannot disagree with the world it was read from.
//!
//! # Who is audited
//!
//! The seats still in the match: a seat that has been eliminated is out of it,
//! and the audit decides between the seats it is choosing a winner from. When
//! no seat is still in (item 16's no-survivor end), the seats that fell on the
//! latest elimination tick are the ones the audit decides between — the seats
//! that were still in until the tick that ended the match. Neither sentence is
//! spelled out in the spec; both are the reading that keeps an eliminated seat
//! from winning a match it was already out of, and the pull request that
//! introduced this module names them for the owner.
//!
//! # The terms, in order
//!
//! 1. **Net worth at the final audit**: the audit score, held value
//!    ([`World::held_value`], the same number the Ledger ranks on) plus enemy
//!    value destroyed. "Unsmoothed" is the raw figure, not a displayed
//!    standing.
//! 2. **Enemy value destroyed.** See [`destroyed_value`]: zero until S2.
//! 3. **Fewer beacons lost**: [`beacons_lost`], read from the beacon table,
//!    which keeps a dead beacon's row for the whole match at zero hit points —
//!    a recycled one included, because recycling books a death exactly as
//!    destruction does (item 18).
//! 4. **A shared win**: every seat tied on all three.

use crate::math::quantity::Money;
use crate::tables::SeatId;
use crate::world::World;

/// One seat's line of the final audit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditLine {
    /// Whose line it is.
    pub seat: SeatId,
    /// Value held: `$` plus own assets at build cost × current HP.
    pub held: Money,
    /// The build cost of enemy assets this seat destroyed.
    pub destroyed: Money,
    /// The audit score, `held + destroyed`: the first term of the tie-break.
    pub score: Money,
    /// How many of this seat's beacons have died this match.
    pub beacons_lost: u32,
}

impl AuditLine {
    /// The line's rank key: higher is better, and two lines with the same key
    /// are tied on every term.
    fn key(&self) -> (i64, i64, i64) {
        (
            self.score.raw(),
            self.destroyed.raw(),
            i64::from(self.beacons_lost).saturating_neg(),
        )
    }
}

/// The final audit of a world as it stands.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct FinalAudit {
    /// Every audited seat's line, in seat order.
    pub lines: Vec<AuditLine>,
    /// The seats that top the audit on every term, in seat order: one seat is
    /// a win, more than one is a shared win.
    pub winners: Vec<SeatId>,
}

impl FinalAudit {
    /// The single winner, or `None` when the win is shared (or nobody was
    /// audited).
    #[must_use]
    pub fn winner(&self) -> Option<SeatId> {
        match self.winners.as_slice() {
            [only] => Some(*only),
            _ => None,
        }
    }

    /// Whether more than one seat tied on every term.
    #[must_use]
    pub fn is_shared(&self) -> bool {
        self.winners.len() > 1
    }
}

/// Audit every seat the audit decides between, and name the winners.
///
/// Allocates; the tick calls [`audit_winner`], which does not.
#[must_use]
pub fn final_audit(world: &World) -> FinalAudit {
    let mut lines = Vec::new();
    let count = seat_count(world);
    let latest = latest_fall(world);
    let mut index: usize = 0;
    while index < count {
        if let Some(seat) = audited(world, index, latest) {
            lines.push(line_of(world, seat));
        }
        index = index.saturating_add(1);
    }
    let best = lines.iter().map(AuditLine::key).max();
    let winners = lines
        .iter()
        .filter(|line| Some(line.key()) == best)
        .map(|line| line.seat)
        .collect();
    FinalAudit { lines, winners }
}

/// The audit's single winner, or `None` on a shared win: [`final_audit`]'s
/// answer without its allocation, for the tick that ends a match (a tick
/// allocates nothing, G3′ §9.17).
#[must_use]
pub fn audit_winner(world: &World) -> Option<SeatId> {
    let count = seat_count(world);
    let latest = latest_fall(world);
    let mut best: Option<((i64, i64, i64), SeatId)> = None;
    let mut tied = false;
    let mut index: usize = 0;
    while index < count {
        if let Some(seat) = audited(world, index, latest) {
            let key = line_of(world, seat).key();
            match best {
                Some((found, _)) if key < found => {}
                Some((found, _)) if key == found => tied = true,
                _ => {
                    best = Some((key, seat));
                    tied = false;
                }
            }
        }
        index = index.saturating_add(1);
    }
    if tied {
        None
    } else {
        best.map(|(_, seat)| seat)
    }
}

/// One seat's audit line.
#[must_use]
pub fn line_of(world: &World, seat: SeatId) -> AuditLine {
    let held = world.held_value(seat);
    let destroyed = destroyed_value(world, seat);
    AuditLine {
        seat,
        held,
        destroyed,
        score: Money::new(held.raw().saturating_add(destroyed.raw())),
        beacons_lost: beacons_lost(world, seat),
    }
}

/// The build cost of the enemy assets `seat` destroyed.
///
/// PLACEHOLDER: always zero. Destruction credit is apportioned by the
/// kill-credit split (spec section 3, item 18), whose counters exist
/// ([`crate::credit`]) but which nothing books a destroyed asset's value
/// against until S2 brings combat; the value a seat destroyed is tracked
/// nowhere yet. The owner settles it at S2, with the kill-credit split.
#[must_use]
pub const fn destroyed_value(_world: &World, _seat: SeatId) -> Money {
    Money::ZERO
}

/// How many of `seat`'s beacons have died this match: its rows in the beacon
/// table at zero hit points.
///
/// A beacon's row is never removed — a destroyed or recycled beacon keeps it
/// for the whole match at zero hit points (item 18: recycling books a death
/// exactly as destruction does) — so this is a count over hashed, snapshotted
/// state and needs no column of its own.
#[must_use]
pub fn beacons_lost(world: &World, seat: SeatId) -> u32 {
    let beacons = world.beacons();
    let count = usize::try_from(beacons.len()).unwrap_or(0);
    let mut lost: u32 = 0;
    let mut row: usize = 0;
    while row < count {
        if beacons.seats().get(row).copied() == Some(seat.raw())
            && !beacons
                .hit_points()
                .get(row)
                .is_some_and(|hp| hp.is_alive())
        {
            lost = lost.saturating_add(1);
        }
        row = row.saturating_add(1);
    }
    lost
}

fn seat_count(world: &World) -> usize {
    usize::try_from(world.seats().len()).unwrap_or(0)
}

/// The tick of the most recent elimination, when no seat is still in the
/// match; `None` while one is.
fn latest_fall(world: &World) -> Option<u32> {
    let seats = world.seats();
    let count = seat_count(world);
    if (0..count).any(|index| seats.is_alive(index)) {
        return None;
    }
    seats.eliminated_at().iter().copied().max()
}

/// The seat at `index`, when the audit decides between it and the others.
fn audited(world: &World, index: usize, latest: Option<u32>) -> Option<SeatId> {
    let seats = world.seats();
    let seat = SeatId::new(seats.seats().get(index).copied()?);
    let counted = match latest {
        None => seats.is_alive(index),
        Some(tick) => seats.eliminated_at().get(index).copied() == Some(tick),
    };
    counted.then_some(seat)
}

/// Put the audit's lines on the event bus: one `final_audit` per audited seat,
/// in seat order, each carrying the seat and its score. Allocation-free, so the
/// tick that ends a match can call it.
pub(crate) fn emit_lines(world: &mut World, tick: crate::math::quantity::Tick) {
    let count = seat_count(world);
    let latest = latest_fall(world);
    let mut index: usize = 0;
    while index < count {
        if let Some(seat) = audited(world, index, latest) {
            let score = line_of(world, seat).score;
            world.emit(
                tick,
                crate::events::Emission::of(crate::events::EventKind::FinalAudit)
                    .seat(seat)
                    .value(score.raw()),
            );
        }
        index = index.saturating_add(1);
    }
}
