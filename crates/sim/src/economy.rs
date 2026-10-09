// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `$`: the treasury, the Quartermaster, value and the Ledger's settlement
//! (spec section 7; decisions log items 18, 19, 22 and 23).
//!
//! # The two sentences this module is built on, and how they compose
//!
//! They look as if they disagree, and they do not:
//!
//! * **"Paid means yours"** (item 23). `$` leaves the treasury the moment an
//!   order commits — a deploy starts, a structure is queued, a unit is ordered
//!   — and from that instant the asset counts at its **full build cost** for
//!   every purpose. There is no partial value anywhere: construction time is
//!   hit points and spectacle, not accounting, and there is no refund if the
//!   thing dies mid-build.
//! * **"Value follows condition"** (item 18). An asset is worth *build cost ×
//!   current hit points* wherever value is read: held value at the final audit
//!   and in live standings, and the recycle refund
//!   (`economy.recycle_refund_percent` of remaining value).
//!
//! The first fixes the **basis** — full build cost from the instant of commit,
//! never a fraction of it because the thing is half built — and the second
//! scales that basis by **condition**. So [`value_of`] is one function with one
//! rule, `cost * hp / max_hp`, and a structure the Quartermaster paid for one
//! tick ago is worth almost nothing because it has almost no hit points, not
//! because it was only partly paid for. That is exactly what the acceptance
//! test `value_follows_condition_at_the_audit_and_at_the_recycle_refund` reads,
//! and it is why `paid_means_yours_survives_a_mid_build_death` can assert that
//! the `$` is gone and nothing comes back.
//!
//! # The ladder and the round robin
//!
//! Spec section 7, the Quartermaster row: it "fills the mandates' spend
//! requests by urgency: defend under attack, then repair, units, build, mine —
//! round-robin within a band, so no single beacon can hog the treasury". Those
//! five bands are [`Urgency`], in that order, and the round robin's position is
//! **hashed state** on the seat table
//! ([`crate::tables::SeatTable::qm_cursors`]): it decides which beacon of a
//! band is served first next tick, so a machine that disagreed about it would
//! spend a seat's `$` on a different beacon.
//!
//! The one player lever over power — the per-beacon low / normal / high knob —
//! does **not** appear here. Priority is kW-only: it orders brownouts
//! ([`crate::power`]) and nothing else, while `$` follows the ladder.

use crate::math::quantity::{Kw, Money};
use crate::rules::RulesTable;
use crate::tables::{BeaconId, SeatId};

/// The five urgency bands the Quartermaster fills in order (spec section 7).
///
/// The ids are written out and additive only, for the reason every other wire
/// id in this crate is: the band reaches an event's `value` and a scenario file
/// can assert on it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Urgency {
    /// A Defend mandate whose beacon is under attack. **S2**: nothing produces
    /// one at the skeleton, because nothing attacks. The band exists now
    /// because the ladder's *order* is the rule, and a band added in the middle
    /// later would move every settlement golden.
    DefendUnderAttack,
    /// Repairing a damaged structure of your own. **S2**, for the same reason.
    Repair,
    /// A fabricator producing a unit its mandate has outstanding work for
    /// (item 22).
    Units,
    /// A Build mandate paying for an unbuilt target.
    Build,
    /// A Mine mandate's spend: the mining drone it fabricates while the seam
    /// it holds has work left. Last on the ladder (spec section 7), and
    /// filed here rather than under [`Urgency::Units`] since S1's `mine`
    /// lane, so a Build mandate's drones and targets are paid for before a
    /// Mine mandate's drone.
    Mine,
}

impl Urgency {
    /// Every band, in the order the Quartermaster fills them.
    pub const ALL: [Urgency; 5] = [
        Urgency::DefendUnderAttack,
        Urgency::Repair,
        Urgency::Units,
        Urgency::Build,
        Urgency::Mine,
    ];

    /// The wire id. Additive only: never reuse, never renumber. Ascending in
    /// ladder order, so a sort by id is a sort by urgency.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            Urgency::DefendUnderAttack => 1,
            Urgency::Repair => 2,
            Urgency::Units => 3,
            Urgency::Build => 4,
            Urgency::Mine => 5,
        }
    }

    /// The band an id names, or `None` for one this build does not define.
    #[must_use]
    pub const fn from_id(id: u8) -> Option<Urgency> {
        match id {
            1 => Some(Urgency::DefendUnderAttack),
            2 => Some(Urgency::Repair),
            3 => Some(Urgency::Units),
            4 => Some(Urgency::Build),
            5 => Some(Urgency::Mine),
            _ => None,
        }
    }

    /// The name a transcript and a scenario file read.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Urgency::DefendUnderAttack => "defend_under_attack",
            Urgency::Repair => "repair",
            Urgency::Units => "units",
            Urgency::Build => "build",
            Urgency::Mine => "mine",
        }
    }
}

/// What one beacon's mandate is asking to be paid for, this tick.
///
/// A mandate offers **at most one** request per tick: the round robin is what
/// stops a beacon hogging the treasury, and a beacon allowed to file five
/// requests would walk straight around it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct SpendRequest {
    /// Whose treasury it draws on.
    pub seat: SeatId,
    /// Which beacon's mandate asked. The round robin's key.
    pub beacon: BeaconId,
    /// Which band it sits in.
    pub urgency: Urgency,
    /// What it costs, in `$`.
    pub cost: Money,
    /// What it will add to the seat's draw once it is fielded, in `kW`. Zero
    /// for anything that draws nothing — a Generator supplies rather than
    /// draws.
    pub draw: Kw,
    /// What it buys.
    pub buys: Purchase,
}

/// What a [`SpendRequest`] buys, once it is approved.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Purchase {
    /// One unit of this kind, homed to the asking beacon.
    Unit(crate::tables::UnitKind),
    /// The Build target at this row of [`crate::tables::TargetTable`]: the
    /// structure goes into the ground at one hit point and a build drone
    /// raises it.
    Structure(u32),
}

/// The spend-control seam spec section 15 keeps ("a Quartermaster trait").
///
/// It answers one question — may this request be filled right now? — and it
/// does **not** decide the order requests arrive in. The ladder and the round
/// robin are the sim's, in [`crate::world::World`]'s Quartermaster phase,
/// because both are rules rather than policy; what a later custom Quartermaster
/// replaces (spec section 7's roadmap row, after v1.2) is the judgement below.
///
/// Two rules bind every implementation, the same two that bind
/// [`crate::seams::Operator`]:
///
/// * **No clock.** Everything it reads is game state.
/// * **No privileged reads.** It sees the seat's own treasury and headroom and
///   the request, and nothing else.
pub trait Quartermaster {
    /// A name for a report. Not a user-facing string: those live in one English
    /// string table (AGENTS.md §12).
    fn name(&self) -> &'static str;

    /// Whether `request` may be filled from `treasury` with `headroom` `kW` to
    /// spare.
    fn approve(&self, request: &SpendRequest, treasury: Money, headroom: Kw) -> bool;
}

/// The skeleton's Quartermaster: one treasury, no per-beacon pools, no sweeps
/// (spec section 7, "Treasury").
///
/// It says yes when the treasury covers the cost **and**, for anything that
/// will draw power, when the seat has the headroom to run it. The second half
/// is spec section 7's "balances power: when draw exceeds supply, it holds
/// fabricator orders, then applies the brownout order" — **holding comes
/// first**, so a seat at the edge of its supply stops buying before anything of
/// its goes dark. A Generator asks for no headroom because it is the thing that
/// supplies.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SingleTreasury;

impl Quartermaster for SingleTreasury {
    fn name(&self) -> &'static str {
        "single_treasury"
    }

    fn approve(&self, request: &SpendRequest, treasury: Money, headroom: Kw) -> bool {
        if request.cost.raw() < 0 || treasury.raw() < request.cost.raw() {
            return false;
        }
        request.draw.raw() <= headroom.raw()
    }
}

/// An asset's value: **build cost scaled by condition** (item 18).
///
/// `cost * hp / max_hp`, in integer `$`, truncated toward zero — one rounding,
/// stated once, so the audit and the recycle refund cannot disagree about it. A
/// structure with no hit points left is worth nothing; a ruin is worth nothing
/// because it has no owner and is never valued at all (item 20).
///
/// `max_hp` of zero answers zero rather than dividing: a kind whose rules-table
/// row is missing is a table mistake, and answering "worth nothing" is the
/// answer that cannot be mistaken for a real number.
#[must_use]
pub fn value_of(cost: Money, hp: i32, max_hp: i32) -> Money {
    if hp <= 0 || max_hp <= 0 || cost.raw() <= 0 {
        return Money::ZERO;
    }
    let held = i64::from(hp.min(max_hp));
    let full = i64::from(max_hp);
    cost.raw()
        .checked_mul(held)
        .and_then(|scaled| scaled.checked_div(full))
        .map_or(Money::ZERO, Money::new)
}

/// A percentage of a `$` amount, truncated toward zero.
///
/// The recycle refund and the salvage share are both "this many percent of
/// that", and both round the same way for the reason [`value_of`] does.
#[must_use]
pub fn percent_of(amount: Money, percent: u32) -> Money {
    let percent = i64::from(percent);
    amount
        .raw()
        .checked_mul(percent)
        .and_then(|scaled| scaled.checked_div(100))
        .map_or(Money::ZERO, Money::new)
}

/// The Base Maintenance Indemnity one seat is paid at a settlement, after the
/// standing adjustment (spec section 7, "BMI scaling").
///
/// `rank` is the seat's place on held value among the **living** seats, from
/// zero for the leader; `living` is how many seats are on that ladder.
/// Eliminated seats have already left it, which is what makes the ladder
/// shrink as a match goes on.
///
/// The adjustment is linear between the leader's malus and the last place's
/// bonus:
///
/// ```text
/// percent = (bonus * rank - malus * (living - 1 - rank)) / (living - 1)
/// ```
///
/// truncated toward zero ([`band_percent`], the one copy of the formula). The
/// **adjustment** is zero when `living` is one, so the seat is paid the
/// unadjusted `economy.bmi_dollars`: a seat that is both the leader and the
/// last place has nobody to catch up to. The indemnity is `bmi_dollars` plus
/// that percent of it, truncated toward zero, the rounding [`percent_of`]
/// states.
///
/// Every read is typed (S1's `build` lane; decisions-log item 134): the
/// economy block, the place on the ladder and the arithmetic each refuse
/// rather than answer 0 or clamp a rank onto the ladder. Under a table that
/// loaded, a place on the ladder is always paid: `RulesTable::from_message`
/// refuses a band percent no signed 32-bit percent holds, so for a `rank`
/// below `living` none of the errors below can arise from the table.
///
/// # Errors
///
/// [`SettlementReadError::MissingEconomy`] when the table has no `economy`
/// block, [`SettlementReadError::RankOffLadder`] when `rank` is not below
/// `living`, and [`SettlementReadError::PercentOutOfRange`] when the band's
/// arithmetic leaves its type.
pub fn bmi_for(rules: &RulesTable, rank: u32, living: u32) -> Result<Money, SettlementReadError> {
    let economy = rules
        .message()
        .economy
        .as_ref()
        .ok_or(SettlementReadError::MissingEconomy)?;
    let percent = band_percent(rules, rank, living)?;
    let base = i64::from(economy.bmi_dollars);
    base.checked_mul(i64::from(percent))
        .and_then(|scaled| scaled.checked_div(100))
        .and_then(|adjustment| base.checked_add(adjustment))
        .map(Money::new)
        .ok_or(SettlementReadError::PercentOutOfRange)
}

/// A seat's place on the settlement's ladder ([`ladder_place`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LadderPlace {
    /// How many living seats are placed ahead of it: 0 for the leader. The
    /// wire's `Settlement.band_rank` counts from 1, so it is this plus one.
    pub rank: u32,
    /// How many seats are on the ladder: the living ones.
    pub living: u32,
}

/// Why a settlement read refused ([`ladder_place`], [`band_percent`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettlementReadError {
    /// The rules table carries no `economy` block, so there is no band to
    /// read. A table loaded through [`RulesTable::from_message`] always has
    /// one (it refuses a table without), so this answers a hand-built message
    /// only.
    MissingEconomy,
    /// The seat at this index is not on the ladder: past its end, or
    /// eliminated.
    SeatOffLadder {
        /// The seat's index on the ladder.
        index: usize,
    },
    /// `rank` is no place on a ladder of `living` seats: `living` is zero, or
    /// `rank` is not below it.
    RankOffLadder {
        /// The place asked about.
        rank: u32,
        /// The seats on the ladder.
        living: u32,
    },
    /// The ladder holds more seats than a `u32` counts.
    LadderTooLong,
    /// The band's arithmetic overflowed, or its percent does not fit the
    /// wire's whole-percent field: a rules table no settlement could pay by.
    PercentOutOfRange,
}

impl core::fmt::Display for SettlementReadError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SettlementReadError::MissingEconomy => {
                write!(f, "the rules table has no `economy` block")
            }
            SettlementReadError::SeatOffLadder { index } => {
                write!(f, "seat index {index} is not on the settlement's ladder")
            }
            SettlementReadError::RankOffLadder { rank, living } => {
                write!(f, "rank {rank} is no place on a ladder of {living} seats")
            }
            SettlementReadError::LadderTooLong => {
                write!(f, "the ladder holds more seats than a rank counts")
            }
            SettlementReadError::PercentOutOfRange => {
                write!(f, "the band's percent does not fit a whole percent")
            }
        }
    }
}

impl std::error::Error for SettlementReadError {}

/// Where the seat at `index` stands on the settlement's ladder: the rank the
/// Ledger's settlement computes inline (`World::settle_ledger`), as a pure
/// read.
///
/// `ladder` holds each seat's **held value** as the Ledger read it, before
/// anybody was paid, by seat index, and `None` for a seat that has left the
/// ladder (eliminated). A seat is placed ahead of another when it holds
/// strictly more, or the same with the **lower** seat index (spec section 7,
/// "tie-break the lower seat index"). A reader after the settlement recovers
/// the ladder as each living seat's held value less the `settled` credit it
/// was paid, since a recap consumes no tick and nothing else moves a treasury
/// at that tick's end.
///
/// # Errors
///
/// [`SettlementReadError::SeatOffLadder`] when `index` is past the ladder or
/// names a seat that left it; [`SettlementReadError::LadderTooLong`] when the
/// ladder counts past a `u32`.
pub fn ladder_place(
    ladder: &[Option<Money>],
    index: usize,
) -> Result<LadderPlace, SettlementReadError> {
    let mine = ladder
        .get(index)
        .copied()
        .flatten()
        .ok_or(SettlementReadError::SeatOffLadder { index })?;
    let ahead = ladder
        .iter()
        .enumerate()
        .filter(|(other, theirs)| {
            *other != index
                && theirs.is_some_and(|held| held > mine || (held == mine && *other < index))
        })
        .count();
    let living = ladder.iter().filter(|held| held.is_some()).count();
    Ok(LadderPlace {
        rank: u32::try_from(ahead).map_err(|_| SettlementReadError::LadderTooLong)?,
        living: u32::try_from(living).map_err(|_| SettlementReadError::LadderTooLong)?,
    })
}

/// The band's adjustment to the BMI in whole percent, truncated toward zero,
/// for the seat at `rank` (from 0, the leader) on a ladder of `living` seats:
/// the percent [`bmi_for`] scales `economy.bmi_dollars` by, as a pure read
/// (spec section 7, "BMI scaling"). Linear from the leader's malus (negative)
/// to the last place's bonus, and 0 for a seat alone on the ladder.
///
/// [`bmi_for`] pays by it; like it, this read refuses a rank past the
/// ladder's end rather than clamping it onto the ladder.
///
/// # Errors
///
/// [`SettlementReadError::MissingEconomy`] when the table has no `economy`
/// block; [`SettlementReadError::RankOffLadder`] when `rank` is not below
/// `living`; [`SettlementReadError::PercentOutOfRange`] when the arithmetic
/// overflows or the percent does not fit an `i32`.
pub fn band_percent(
    rules: &RulesTable,
    rank: u32,
    living: u32,
) -> Result<i32, SettlementReadError> {
    let economy = rules
        .message()
        .economy
        .as_ref()
        .ok_or(SettlementReadError::MissingEconomy)?;
    if rank >= living {
        return Err(SettlementReadError::RankOffLadder { rank, living });
    }
    let last = i64::from(living) - 1;
    if last == 0 {
        return Ok(0);
    }
    let rank = i64::from(rank);
    let bonus = i64::from(economy.scaling_last_place_bonus_percent);
    let malus = i64::from(economy.scaling_leader_malus_percent);
    let up = bonus.checked_mul(rank);
    let down = malus.checked_mul(last - rank);
    let percent = up
        .zip(down)
        .and_then(|(up, down)| up.checked_sub(down))
        .and_then(|numerator| numerator.checked_div(last))
        .ok_or(SettlementReadError::PercentOutOfRange)?;
    i32::try_from(percent).map_err(|_| SettlementReadError::PercentOutOfRange)
}

/// The award fund one settlement releases, in `$`
/// (`economy.award_fund_percent_of_bmi` of one unadjusted BMI).
///
/// PLACEHOLDER: the fund is computed and **nobody is paid out of it**, because
/// it is "split among that round's winners in proportion to awards won" and no
/// award exists to win — the award catalogue is spec section 7's Tuning row and
/// lands with **S4**'s licences and awards. Releasing it to nobody is the
/// honest reading of a fund with no winners; crediting it evenly would invent a
/// rule the spec does not have (owner, at S4).
#[must_use]
pub fn award_fund(rules: &RulesTable) -> Money {
    let Some(economy) = rules.message().economy.as_ref() else {
        return Money::ZERO;
    };
    percent_of(
        Money::new(i64::from(economy.bmi_dollars)),
        economy.award_fund_percent_of_bmi,
    )
}

/// The `$` a seat starts with: `economy.starting_bmi_multiplier` indemnities.
///
/// A multiple rather than a second number, so the spec's rule — "starting money
/// is two indemnities" — stays a rule that cannot drift away from the BMI it is
/// a multiple of.
#[must_use]
pub fn starting_treasury(rules: &RulesTable) -> Money {
    let Some(economy) = rules.message().economy.as_ref() else {
        return Money::ZERO;
    };
    let base = i64::from(economy.bmi_dollars);
    let multiple = i64::from(economy.starting_bmi_multiplier);
    Money::new(base.saturating_mul(multiple))
}

#[cfg(test)]
mod tests {
    use super::{Urgency, percent_of, value_of};
    use crate::math::quantity::Money;

    #[test]
    fn value_is_build_cost_scaled_by_condition() {
        // Item 18, the whole rule in three lines: full hit points is full
        // value, half is half, and none is none.
        assert_eq!(value_of(Money::new(80), 600, 600), Money::new(80));
        assert_eq!(value_of(Money::new(80), 300, 600), Money::new(40));
        assert_eq!(value_of(Money::new(80), 0, 600), Money::ZERO);
        // A structure one tick into its construction is worth almost nothing,
        // and is still paid for in full (item 23). The two are not in tension:
        // the basis is the full cost, the scale is the condition.
        assert_eq!(value_of(Money::new(80), 3, 600), Money::ZERO);
    }

    #[test]
    fn a_refund_is_a_percentage_of_remaining_value() {
        let remaining = value_of(Money::new(60), 400, 800);
        assert_eq!(remaining, Money::new(30));
        assert_eq!(percent_of(remaining, 50), Money::new(15));
    }

    fn rules() -> crate::rules::RulesTable {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join(crate::rules::RULES_PATH);
        crate::rules::RulesTable::load(&path)
            .unwrap_or_else(|error| panic!("the committed rules table loads: {error}"))
    }

    #[test]
    fn the_band_percent_is_the_percent_bmi_for_pays_by() {
        use super::{band_percent, bmi_for};
        let rules = rules();
        let base = i64::from(rules.message().economy.as_ref().map_or_else(
            || panic!("the committed table has an economy block"),
            |economy| economy.bmi_dollars,
        ));
        for living in 1..=3_u32 {
            for rank in 0..living {
                let percent = band_percent(&rules, rank, living)
                    .unwrap_or_else(|error| panic!("rank {rank} of {living}: {error}"));
                let adjustment = (base * i64::from(percent))
                    .checked_div(100)
                    .unwrap_or_else(|| panic!("a hundred divides"));
                assert_eq!(
                    Ok(Money::new(base + adjustment)),
                    bmi_for(&rules, rank, living),
                    "rank {rank} of {living} at {percent} %"
                );
            }
        }
        assert_eq!(band_percent(&rules, 0, 1), Ok(0), "alone on the ladder");
        let leader = band_percent(&rules, 0, 3).unwrap_or_else(|error| panic!("{error}"));
        let last = band_percent(&rules, 2, 3).unwrap_or_else(|error| panic!("{error}"));
        assert!(leader < 0 && last > 0, "malus {leader}, bonus {last}");
    }

    #[test]
    fn a_band_off_the_ladder_is_refused_rather_than_clamped() {
        use super::{SettlementReadError, band_percent};
        let rules = rules();
        assert_eq!(
            band_percent(&rules, 3, 3),
            Err(SettlementReadError::RankOffLadder { rank: 3, living: 3 })
        );
        assert_eq!(
            band_percent(&rules, 0, 0),
            Err(SettlementReadError::RankOffLadder { rank: 0, living: 0 })
        );
        // And so is `bmi_for`, which pays by it: a rank past the ladder is
        // refused rather than clamped onto its last place.
        assert_eq!(
            super::bmi_for(&rules, 3, 3),
            Err(SettlementReadError::RankOffLadder { rank: 3, living: 3 })
        );
        // A band percent no signed 32-bit percent holds is refused when the
        // table loads, so a table a world can hold always pays its ladder.
        let mut message = rules.message().clone();
        if let Some(economy) = message.economy.as_mut() {
            economy.scaling_last_place_bonus_percent = u32::MAX;
        }
        assert!(
            matches!(
                crate::rules::RulesTable::from_message(&message),
                Err(crate::rules::RulesError::OutOfRange { ref field, .. })
                    if field == "economy.scaling_last_place_bonus_percent"
            ),
            "a bonus past a signed percent does not load"
        );
    }

    #[test]
    fn the_ladder_places_a_tie_by_the_lower_seat_index_and_skips_the_eliminated() {
        use super::{LadderPlace, SettlementReadError, ladder_place};
        let held = |value: i64| Some(Money::new(value));
        // Seats 0 and 2 tie; seat 1 has left the ladder; seat 3 leads.
        let ladder = [held(50), None, held(50), held(80)];
        let place = |index: usize| ladder_place(&ladder, index);
        assert_eq!(place(3), Ok(LadderPlace { rank: 0, living: 3 }));
        assert_eq!(place(0), Ok(LadderPlace { rank: 1, living: 3 }));
        assert_eq!(place(2), Ok(LadderPlace { rank: 2, living: 3 }));
        assert_eq!(
            place(1),
            Err(SettlementReadError::SeatOffLadder { index: 1 })
        );
        assert_eq!(
            place(4),
            Err(SettlementReadError::SeatOffLadder { index: 4 })
        );
    }

    #[test]
    fn the_ladder_is_ascending_in_urgency_order() {
        // A sort by id has to be a sort by urgency, because that is what the
        // Quartermaster phase relies on.
        for pair in Urgency::ALL.windows(2) {
            let (Some(first), Some(second)) = (pair.first(), pair.get(1)) else {
                continue;
            };
            assert!(first.id() < second.id(), "{first:?} before {second:?}");
        }
    }
}
