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
//! The split is **emitted and not accumulated**. Each seat's share rides the
//! event bus as [`crate::events::EventKind::KillCredited`] and no per-seat
//! total is kept, for exactly the reason [`crate::seams::WorkCounter`] is not
//! hashed: nothing in a tick reads such a total, and a field that affects
//! nothing must not move a golden file.
//!
//! PLACEHOLDER: the counters that *do* read the split are the awards — "awards
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
        for (slot, (seat, amount)) in shares.iter().enumerate() {
            if topped.get(slot).copied().unwrap_or(true) {
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
    while at < usize::try_from(world.credit().len()).unwrap_or(0) {
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
        for (seat, share) in split.iter() {
            if *share <= 0 {
                continue;
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
}

/// Look up an asset's condition, price and place, or `None` when this build
/// cannot find it.
fn asset_state(world: &World, id: AssetId) -> Option<AssetState> {
    let row = usize::try_from(id.index()).ok()?;
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
            })
        }
        AssetId::TAG_BEACON => Some(AssetState {
            hp: world.beacons().hit_points().get(row)?.raw(),
            max_hp: world
                .beacon_max_hp(crate::tables::BeaconId::new(id.index()))
                .raw(),
            cost: world.beacon_cost(),
            at: world.beacons().positions().get(row).copied()?,
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
                max_hp: world.structure_max_hp(kind).raw(),
                cost: world.structure_cost(kind),
                at: world.structures().positions().get(row).copied()?,
            })
        }
        _ => None,
    }
}
