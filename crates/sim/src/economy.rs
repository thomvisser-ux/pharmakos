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

/// PLACEHOLDER: hit points a working build drone adds to a structure per
/// second of game time.
///
/// **A named constant rather than a rules-table row, deliberately**
/// (AGENTS.md §12, decisions-log item 105(1)). Spec section 5's interface table
/// prices queueing a structure at "5 s, **plus** fabricator construction time",
/// so construction time is explicitly not one of the interface times, and no
/// row in `gp.v1.RulesTable` carries it: revision 3 fills in every `$` and `kW`
/// number the walking skeleton names and this is not one of them. Nothing in
/// T14's acceptance asserts its *value* — only that a structure spends time
/// under construction, which any positive rate gives — so the row is proposed
/// by **S1's economy work**, which is the stage that has mining and building
/// rates to be wrong about together (owner, at S1, as
/// `structures.build_hp_per_second`).
///
/// Sixty is chosen so the number divides the tick rate exactly: 60 hit points a
/// second is 3 a tick at 20 Hz, so construction needs no accumulator and no
/// rounding rule, and the `structures.generator.hp` = 600 Generator of the
/// plan's §1.1 demo goes up in ten seconds on one drone and five on two.
pub const BUILD_HP_PER_SECOND: i32 = 60;

/// PLACEHOLDER: ore voxels a mining drone carries before it walks its load
/// home.
///
/// A named constant for the reason [`BUILD_HP_PER_SECOND`] is one: spec
/// section 6's Mine row says "drones carry ore to their home beacon" and
/// section 7 says delivered ore is credited immediately, but no row says how
/// much a drone carries, and `economy.seam_voxels` and `mining_ms_per_voxel`
/// are the only two numbers the seam has. Nothing in T14's acceptance asserts
/// its value. Proposed as `economy.mining_load_voxels` by **S1**, with the
/// finite-seam work that gives a load a meaning (owner, at S1).
///
/// Sixteen, so that a `economy.seam_voxels` = 150 seam is about nine round
/// trips rather than one or a hundred and fifty.
pub const MINING_LOAD_VOXELS: i32 = 16;

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
    /// A Mine mandate's spend.
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
/// truncated toward zero, and **zero when `living` is one**: a seat that is
/// both the leader and the last place has nobody to catch up to, and the
/// formula's denominator says the same thing by being empty rather than by
/// dividing by nothing.
#[must_use]
pub fn bmi_for(rules: &RulesTable, rank: u32, living: u32) -> Money {
    let Some(economy) = rules.message().economy.as_ref() else {
        return Money::ZERO;
    };
    let base = i64::from(economy.bmi_dollars);
    if living <= 1 {
        return Money::new(base);
    }
    let bonus = i64::from(economy.scaling_last_place_bonus_percent);
    let malus = i64::from(economy.scaling_leader_malus_percent);
    let rank = i64::from(rank.min(living.saturating_sub(1)));
    let last = i64::from(living.saturating_sub(1));
    let numerator = bonus
        .checked_mul(rank)
        .and_then(|up| {
            malus
                .checked_mul(last.saturating_sub(rank))
                .map(|down| up - down)
        })
        .unwrap_or(0);
    let percent = numerator.checked_div(last).unwrap_or(0);
    let adjustment = base
        .checked_mul(percent)
        .and_then(|scaled| scaled.checked_div(100))
        .unwrap_or(0);
    Money::new(base.saturating_add(adjustment))
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
