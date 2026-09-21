// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the seat's **frozen planning snapshot** says, in the sim's own integer
//! newtypes.
//!
//! Everything the prose and the projection need about the world comes from
//! here, and here reads the snapshot and nothing else. That is the rule spec
//! section 13 states for the editor clock —
//!
//! > The clock and the pill are scaled to the coming segment's length, taken
//! > from the frozen snapshot rather than assumed
//!
//! — and the reason [`segment_length_ms`] returns an `Option` rather than a
//! number: a snapshot taken before a match opened carries none, and a constant
//! would be exactly the thing the rule forbids.

use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::math::quantity::{Kw, Money, Ms, Tick};
use pharmakos_sim::snapshot::Snapshot;

use crate::error::Error;

/// The coming segment's length, out of the frozen snapshot.
///
/// **The PLACEHOLDER that stood here is discharged.** It named T10, which has
/// landed: `Snapshot::coming_segment_ms` is the field, and the sim's own
/// acceptance test `the_coming_segments_length_comes_from_the_snapshot_not_a_constant`
/// restores a frozen snapshot into a world built under a *different* ladder to
/// prove the carried number is the one that decides.
///
/// So this reads that field and nothing else. It is never
/// `rules.match.segment_lengths_ms`: the ladder is the host's setting and the
/// snapshot is the fact, and a table lookup here would be an assumption about
/// which round it is — the constant spec section 13 forbids.
///
/// `None` for a snapshot that carries no positive length, which is a default
/// snapshot or one taken before a match opened. A caller that wants to say
/// something about the segment still says nothing rather than guessing: a
/// rendering with no segment claim is correct, and one with a claim of zero is
/// not.
#[must_use]
pub const fn segment_length_ms(snapshot: &Snapshot) -> Option<Ms> {
    if snapshot.coming_segment_ms > 0 {
        Some(Ms::new(snapshot.coming_segment_ms))
    } else {
        None
    }
}

/// The facts one planning session is against.
///
/// Built once per snapshot per seat. Everything is an integer newtype from
/// `pharmakos-sim`, so the `$`, `kW` and millisecond arithmetic downstream
/// cannot mix a treasury with a power figure (AGENTS.md section 4.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PlanContext {
    seat: u8,
    tick: Tick,
    treasury: Money,
    supply: Kw,
    draw: Kw,
    segment_ms: Option<Ms>,
}

impl PlanContext {
    /// Reads the snapshot for one seat.
    ///
    /// # Errors
    ///
    /// When the snapshot does not carry that seat, or carries it without one
    /// of its three economy columns — a snapshot whose seat rows are ragged is
    /// not a snapshot, and substituting a zero treasury would make every
    /// projection wrong in the safe-looking direction.
    pub fn from_snapshot(snapshot: &Snapshot, seat: u8) -> Result<Self, Error> {
        let row = snapshot
            .seat_id
            .iter()
            .position(|id| *id == seat)
            .ok_or_else(|| {
                Error::at(
                    "",
                    format!("the frozen snapshot does not carry seat {seat}"),
                )
            })?;
        let ragged = |what: &str| {
            Error::at(
                "",
                format!("the frozen snapshot has no {what} for seat {seat}"),
            )
        };
        Ok(Self {
            seat,
            tick: Tick::new(snapshot.tick),
            treasury: Money::new(
                *snapshot
                    .seat_treasury
                    .get(row)
                    .ok_or_else(|| ragged("treasury"))?,
            ),
            supply: Kw::new(
                *snapshot
                    .seat_supply
                    .get(row)
                    .ok_or_else(|| ragged("supply"))?,
            ),
            draw: Kw::new(*snapshot.seat_draw.get(row).ok_or_else(|| ragged("draw"))?),
            segment_ms: segment_length_ms(snapshot),
        })
    }

    /// Which seat this is.
    #[must_use]
    pub const fn seat(&self) -> u8 {
        self.seat
    }

    /// The tick the snapshot was frozen at.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// The seat's treasury, in `$`.
    #[must_use]
    pub const fn treasury(&self) -> Money {
        self.treasury
    }

    /// The seat's power supply, in `kW`.
    #[must_use]
    pub const fn supply(&self) -> Kw {
        self.supply
    }

    /// The seat's power draw, in `kW`.
    #[must_use]
    pub const fn draw(&self) -> Kw {
        self.draw
    }

    /// Supply minus draw. `None` on overflow, which the caller reports rather
    /// than wrapping (AGENTS.md section 4.1).
    #[must_use]
    pub fn headroom(&self) -> Option<Kw> {
        self.supply.checked_sub(self.draw)
    }

    /// The same three numbers as the sim's own seat-economy type, so the
    /// gateway can build a `pharmakos_verifier::Scope` from this context
    /// rather than reading the snapshot a second time and risking a different
    /// answer.
    #[must_use]
    pub const fn economy(&self) -> SeatEconomy {
        SeatEconomy {
            treasury: self.treasury,
            supply: self.supply,
            draw: self.draw,
        }
    }

    /// The coming segment's length, when the snapshot carries it.
    ///
    /// See [`segment_length_ms`]: read from the snapshot's own
    /// `coming_segment_ms`, never from the rules table's ladder.
    #[must_use]
    pub const fn segment_ms(&self) -> Option<Ms> {
        self.segment_ms
    }
}

#[cfg(test)]
mod tests {
    use super::{PlanContext, segment_length_ms};
    use pharmakos_sim::math::quantity::{Kw, Money};
    use pharmakos_sim::snapshot::Snapshot;

    fn snapshot() -> Snapshot {
        Snapshot {
            tick: 42,
            seat_id: vec![0, 1],
            seat_treasury: vec![200, 50],
            seat_supply: vec![10, 4],
            seat_draw: vec![6, 9],
            ..Snapshot::default()
        }
    }

    #[test]
    fn the_economy_comes_from_the_seats_own_row() {
        let context = PlanContext::from_snapshot(&snapshot(), 1).expect("seat 1 is there");
        assert_eq!(context.treasury(), Money::new(50));
        assert_eq!(context.supply(), Kw::new(4));
        assert_eq!(context.draw(), Kw::new(9));
        assert_eq!(context.headroom(), Some(Kw::new(-5)));
    }

    #[test]
    fn a_seat_the_snapshot_does_not_carry_is_an_error() {
        assert!(PlanContext::from_snapshot(&snapshot(), 7).is_err());
    }

    #[test]
    fn a_ragged_snapshot_is_an_error_rather_than_a_zero() {
        let ragged = Snapshot {
            seat_id: vec![0],
            seat_treasury: Vec::new(),
            ..Snapshot::default()
        };
        assert!(PlanContext::from_snapshot(&ragged, 0).is_err());
    }

    /// T10's field, read and not guessed: a snapshot carrying a length gives
    /// that length, and one carrying none gives nothing rather than the rules
    /// table's ladder.
    #[test]
    fn the_segment_length_comes_from_the_snapshot_and_is_never_guessed() {
        assert_eq!(segment_length_ms(&snapshot()), None, "a default snapshot");

        let carried = Snapshot {
            coming_segment_ms: 9_000,
            ..snapshot()
        };
        assert_eq!(
            segment_length_ms(&carried),
            Some(pharmakos_sim::math::quantity::Ms::new(9_000))
        );
        let context = PlanContext::from_snapshot(&carried, 0).expect("seat 0");
        assert_eq!(
            context.segment_ms(),
            Some(pharmakos_sim::math::quantity::Ms::new(9_000)),
            "9 s is no entry of the 3/5/8 ladder, so this number can only have \
             come from the snapshot"
        );

        let negative = Snapshot {
            coming_segment_ms: -1,
            ..snapshot()
        };
        assert_eq!(
            segment_length_ms(&negative),
            None,
            "never a claim of nonsense"
        );
    }
}
