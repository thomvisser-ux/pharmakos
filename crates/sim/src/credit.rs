// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Kill credit: who is paid for a destruction, and in what proportion
//! (decisions log item 17; spec section 3).
//!
//! # The rules, all of them
//!
//! * Only damage dealt by **other** seats counts, clamped to the hit points
//!   actually removed — a shot that overkills an asset by a thousand books what
//!   was there to take and no more.
//! * Reaching **full hit points clears** the counters: a thing that was repaired
//!   back to new owes nobody anything.
//! * An asset lost with **no enemy damage** on the clock — self-inflicted,
//!   terrain, disbanded on elimination, or already a ruin — credits nobody.
//! * An **eliminated seat's** accrued share is dropped, not redistributed
//!   ([`crate::tables::CreditTable::drop_seat`]).
//! * The split is apportioned as **integers by largest remainder**, ties to the
//!   lowest seat id.
//! * **Recycling books destruction credit exactly as destruction does** (item
//!   18), through this same split — which is why recycling sets a beacon's hit
//!   points to zero and lets the ordinary death path run, rather than having a
//!   removal path of its own.
//!
//! # What is apportioned, and what is not stored
//!
//! The quantity split is the asset's **full build cost** in `$`: item 23 says
//! an asset counts at its full build cost for "kill credit if destroyed
//! mid-build", so the basis is the price rather than the condition — unlike
//! held value, which item 18 scales ([`crate::economy::value_of`] says why the
//! two are not in tension).
//!
//! # The counters name an asset, not a row
//!
//! A counter is keyed by the asset's [`AssetId`], which carries its row's
//! generation (S2's plan, E1): a dead asset's row is freed only once its death
//! has settled here, so nobody's credit outlives it, and a counter that meets a
//! reused row anyway is stale and credits nobody ([`asset_state`]).
//!
//! The split is **emitted and accumulated** (S2's plan, E15): each seat's
//! share rides the event bus as [`crate::events::EventKind::KillCredited`],
//! and is added to the seat's hashed tally ([`crate::tables::Tally`]): its
//! enemy value destroyed, the final audit's second term (decision 7), and its
//! kill and shared-kill counters, which the recap reads instead of the bus,
//! because the bus drops past its capacity and a tally may not.
//!
//! PLACEHOLDER: the awards' per-seat totals as hashed state — owner, at S4, with the licences.
//! The counters that *do* read the split are the awards — "awards
//! computed deterministically from sim counters", spec section 7 — and the
//! award catalogue lands with **S4**'s licences. The day it does, the per-seat
//! totals become hashed state and this module gains a table; the split itself
//! does not change (owner, at S4).

use crate::events::{Emission, EventKind};
use crate::knowledge::{AssetId, Position};
use crate::math::fixed::Fx;
use crate::math::quantity::Money;
use crate::tables::SeatId;
use crate::world::World;

/// Apportion `total` between `shares` by **largest remainder**, ties to the
/// lowest seat id, writing `(seat, share)` into `out`.
///
/// `shares` must already be sorted ascending by seat id, which
/// [`crate::tables::CreditTable::shares_of`] guarantees; the tie rule is then
/// simply "the earlier entry wins", because that entry is the lower seat.
///
/// `shares` must also hold at most [`crate::tables::CREDIT_SLOTS`] entries,
/// which is the counter table's own ceiling ("at most 3 per asset", item 17)
/// and what [`crate::tables::CreditTable::shares_of`] emits. The largest-
/// remainder pass below marks a topped-up entry in a fixed array of that
/// width, so a longer list would leave its tail permanently unmarked and the
/// leftover units undealt — hence the `debug_assert` rather than a silent
/// short sum.
///
/// The shares then sum to exactly `total` whenever `total` is not negative and
/// the damage sums to more than zero. `out` is the caller's buffer and is
/// cleared first, so nothing here allocates inside a tick.
pub fn apportion(total: i64, shares: &[(u8, i32)], out: &mut Vec<(u8, i64)>) {
    debug_assert!(
        shares.len() <= crate::tables::CREDIT_SLOTS,
        "apportion takes at most CREDIT_SLOTS shares; got {}",
        shares.len()
    );
    out.clear();
    if total <= 0 || shares.is_empty() {
        return;
    }
    let mut dealt: i64 = 0;
    for (_, amount) in shares {
        dealt = dealt.saturating_add(i64::from(*amount));
    }
    if dealt <= 0 {
        return;
    }
    let mut handed: i64 = 0;
    for (seat, amount) in shares {
        let exact = total.saturating_mul(i64::from(*amount));
        let floor = exact.checked_div(dealt).unwrap_or(0);
        handed = handed.saturating_add(floor);
        out.push((*seat, floor));
    }
    // Each floor loses strictly less than one unit, so the leftover is smaller
    // than the list and **no seat is topped up twice**. That is what makes the
    // pass below a single scan per leftover unit rather than a priority queue,
    // and it is why `topped` is a flag rather than a count.
    let mut left = total.saturating_sub(handed);
    let mut topped: [bool; crate::tables::CREDIT_SLOTS] = [false; crate::tables::CREDIT_SLOTS];
    while left > 0 {
        let mut best: Option<(i64, u8, usize)> = None;
        // The flags walk with the shares, one per slot, so no read of them can
        // miss (S2's plan, E7).
        for ((slot, (seat, amount)), done) in shares.iter().enumerate().zip(topped) {
            if done {
                continue;
            }
            let exact = total.saturating_mul(i64::from(*amount));
            let floor = exact.checked_div(dealt).unwrap_or(0);
            let remainder = exact.saturating_sub(floor.saturating_mul(dealt));
            // Largest remainder wins; a tie goes to the **lowest seat id**,
            // which is the sim's tie rule everywhere (AGENTS.md §4.6). The seat
            // column is ascending, so an equal remainder never displaces the
            // entry already held.
            if best.is_none_or(|(held, _, _)| remainder > held) {
                best = Some((remainder, *seat, slot));
            }
        }
        let Some((_, _, slot)) = best else {
            break;
        };
        if let Some(flag) = topped.get_mut(slot) {
            *flag = true;
        }
        if let Some((_, given)) = out.get_mut(slot) {
            *given = given.saturating_add(1);
        }
        left = left.saturating_sub(1);
    }
}

/// Apportion `total` between any number of `shares` by **largest
/// remainder**, ties to the lowest seat id, writing `(seat, share)` into `out`
/// in the order of `shares`, which must be ascending by seat.
///
/// [`apportion`]'s rule over a list of any length: the combat phase's split
/// of one tick's damage between every seat that dealt it, nobody's bucket
/// among them (S2's plan, E32). `topped` is the caller's scratch, one flag a
/// share, so nothing allocates once it has grown to the seat count. The
/// shares sum to `total` whenever `total` is not negative and the amounts sum
/// to more than zero.
pub(crate) fn apportion_wide(
    total: i64,
    shares: &[(u8, i64)],
    out: &mut Vec<(u8, i64)>,
    topped: &mut Vec<bool>,
) {
    out.clear();
    topped.clear();
    if total <= 0 || shares.is_empty() {
        return;
    }
    let dealt = shares
        .iter()
        .fold(0_i64, |sum, (_, amount)| sum.saturating_add(*amount));
    if dealt <= 0 {
        return;
    }
    let exact = |amount: i64| i128::from(total).saturating_mul(i128::from(amount));
    let floor = |amount: i64| {
        exact(amount)
            .checked_div(i128::from(dealt))
            .and_then(|share| i64::try_from(share).ok())
    };
    let mut handed: i64 = 0;
    for (seat, amount) in shares {
        let Some(share) = floor(*amount) else {
            out.clear();
            return;
        };
        handed = handed.saturating_add(share);
        out.push((*seat, share));
        topped.push(false);
    }
    // Each floor loses less than one unit, so the leftover is smaller than the
    // list and no seat is topped up twice ([`apportion`]'s argument).
    let mut left = total.saturating_sub(handed);
    while left > 0 {
        let mut best: Option<(i128, usize)> = None;
        for (slot, ((_, amount), done)) in shares.iter().zip(topped.iter()).enumerate() {
            if *done {
                continue;
            }
            // E7: cannot miss: `dealt` is positive, so the remainder exists
            let remainder = exact(*amount).checked_rem(i128::from(dealt)).unwrap_or(0);
            // Largest remainder wins; the seats are ascending, so an equal
            // remainder never displaces the lower seat already held.
            if best.is_none_or(|(held, _)| remainder > held) {
                best = Some((remainder, slot));
            }
        }
        let Some((_, slot)) = best else {
            break;
        };
        if let Some(flag) = topped.get_mut(slot) {
            *flag = true;
        }
        if let Some((_, given)) = out.get_mut(slot) {
            *given = given.saturating_add(1);
        }
        left = left.saturating_sub(1);
    }
}

/// Settle every asset that died this tick and carries credit, and clear the
/// counters of everything that is back to full hit points.
///
/// Walked in ascending [`AssetId`], which is the order the table is kept in, so
/// the events come out in a total order (AGENTS.md §4.6).
pub(crate) fn settle(world: &mut World, shares: &mut Vec<(u8, i32)>, split: &mut Vec<(u8, i64)>) {
    if world.credit().is_empty() {
        return;
    }
    let tick = world.tick();
    let mut at: usize = 0;
    while at < crate::math::index::row(world.credit().len()) {
        let Some(asset) = world.credit().assets().get(at).copied() else {
            break;
        };
        let id = AssetId::new(asset);
        let Some(state) = asset_state(world, id) else {
            // The asset is not one this build can find: drop the row rather
            // than carrying a counter against nothing.
            world.credit_mut().clear_asset(asset);
            continue;
        };
        if state.hp >= state.max_hp && state.max_hp > 0 {
            // Item 17: reaching full hit points clears the counters.
            world.credit_mut().clear_asset(asset);
            continue;
        }
        if state.hp > 0 {
            at = at.saturating_add(1);
            continue;
        }
        world.credit().shares_of(asset, shares);
        apportion(state.cost.raw(), shares, split);
        // The recap's kills read the credit, not the money (decisions-log
        // item 145 (5)): a seat with hit points of credit against an asset it
        // helped destroy has a kill, and a kill whose credit was split with
        // another seat is a shared kill for each of them (spec section 3).
        // The commander is no asset (spec section 4), so it is nobody's kill.
        if state.asset {
            let dealers = shares.iter().filter(|(_, dealt)| *dealt > 0).count();
            for (seat, dealt) in shares.iter() {
                if *dealt <= 0 {
                    continue;
                }
                if let Some(tally) = world.tally_mut(SeatId::new(*seat)) {
                    tally.kills = tally.kills.saturating_add(1);
                    if dealers > 1 {
                        tally.shared_kills = tally.shared_kills.saturating_add(1);
                    }
                }
            }
        }
        for (seat, share) in split.iter() {
            if *share <= 0 {
                continue;
            }
            // E7: rule: a match's destroyed value is bounded by the build cost of every asset it fields, far inside an `i64` of `$`
            if let Some(tally) = world.tally_mut(SeatId::new(*seat)) {
                tally.destroyed = Money::new(tally.destroyed.raw().saturating_add(*share));
            }
            world.emit(
                tick,
                Emission::of(EventKind::KillCredited)
                    .seat(SeatId::new(*seat))
                    .subject(id)
                    .at(Position::from_array(state.at))
                    .value(*share),
            );
        }
        world.credit_mut().clear_asset(asset);
    }
}

/// What the kill-credit phase needs to know about one asset.
struct AssetState {
    hp: i32,
    max_hp: i32,
    cost: Money,
    at: [Fx; 3],
    /// Whether it is an asset the recap counts: everything but a commander.
    asset: bool,
}

/// Look up an asset's condition, price and place, or `None` when this build
/// cannot find it.
///
/// The asset's row is read through its id and **checked by generation** (S2's
/// plan, E1): a counter against an asset whose row has since been freed or
/// reused is stale, counted under [`crate::ids::StaleRef::CreditSlot`], and
/// answers nothing, so its row is dropped and credits nobody -- never booked
/// against whatever took the row.
fn asset_state(world: &World, id: AssetId) -> Option<AssetState> {
    let found = match id.tag() {
        AssetId::TAG_UNIT => world.units().row_of(crate::tables::UnitId::new(id.index())),
        AssetId::TAG_BEACON => world
            .beacons()
            .row_of(crate::tables::BeaconId::new(id.index())),
        AssetId::TAG_STRUCTURE => world
            .structures()
            .row_of(crate::tables::StructureId::new(id.index())),
        _ => return None,
    };
    let row = match found {
        Ok(row) => row.index(),
        Err(crate::ids::RefError::Stale) => {
            world.stale_refs().stale(crate::ids::StaleRef::CreditSlot);
            return None;
        }
        Err(crate::ids::RefError::Id(_) | crate::ids::RefError::NoSuchRow) => return None,
    };
    match id.tag() {
        AssetId::TAG_UNIT => {
            let kind = world
                .units()
                .kinds()
                .get(row)
                .copied()
                .and_then(crate::tables::UnitKind::from_id)?;
            Some(AssetState {
                hp: world.units().hit_points().get(row)?.raw(),
                max_hp: world.unit_max_hp(kind).raw(),
                cost: world.unit_cost(kind),
                at: world.units().positions().get(row).copied()?,
                asset: kind != crate::tables::UnitKind::Commander,
            })
        }
        AssetId::TAG_BEACON => Some(AssetState {
            hp: world.beacons().hit_points().get(row)?.raw(),
            max_hp: world
                .beacon_max_hp(crate::tables::BeaconId::new(id.index()))
                .raw(),
            cost: world.beacon_cost(),
            at: world.beacons().positions().get(row).copied()?,
            asset: true,
        }),
        AssetId::TAG_STRUCTURE => {
            let kind = world
                .structures()
                .kinds()
                .get(row)
                .copied()
                .and_then(crate::tables::StructureKind::from_id)?;
            Some(AssetState {
                hp: world.structures().hit_points().get(row)?.raw(),
                // A wall has no structure row, and no row is one.
                max_hp: world.structure_max_hp(kind)?.raw(),
                cost: world.structure_cost(kind),
                at: world.structures().positions().get(row).copied()?,
                asset: true,
            })
        }
        _ => None,
    }
}
