// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The per-seat power grid: supply, draw, and the brownout order.
//!
//! Plan §2's `power` row: *"per-seat grid: supply, draw, brownout order with
//! the beacon as the unit of power — S1's subject, but it ticks in every
//! segment"*. It is in the budget because it runs on every one of the 9_600
//! ticks of an 8-minute segment, not because S1 needs it here.
//!
//! **The beacon is the unit of power.** A seat's supply is the sum of its live
//! beacons' generators; its draw is the sum of its live beacons, the buildings
//! attached to them, and its units. When draw exceeds supply the grid browns
//! out, and beacons are shed — with the buildings attached to them — until it
//! does not.
//!
//! **The shed order is total.** It is `(priority, seat, beacon id)`, and the
//! beacon id is unique, so no tie can be broken by scan order and two machines
//! always shed the same beacon (AGENTS.md §4.6). `tests/` checks the totality
//! directly rather than inferring it from a passing hash comparison.
//!
//! **It is re-sorted here, every tick.** `programs` rewrites each beacon's
//! `priority` from its current HP on every tick, so an order sorted once at
//! construction would be the order of the *construction-time* priorities for
//! the whole match while the live field went unread. The re-sort costs about a
//! microsecond at 40 beacons and is inside this phase's measured number.

use crate::tables::Structures;
use crate::tick::{Work, World};
use crate::{STRUCTURES_PER_BEACON, UNIT_KW};

/// The shed key of one beacon. Public so the totality test can assert on the
/// same expression the order is built from, rather than on a copy of it.
#[must_use]
pub fn shed_key(priority: u8, seat: u8, id: u32) -> (u8, u8, u32) {
    (priority, seat, id)
}

/// Un-power the buildings attached to a shed beacon, and return the draw that
/// removes together with whether any `powered` byte actually moved.
fn shed_attached(structures: &mut Structures, beacon: usize) -> (i32, bool) {
    let mut freed: i32 = 0;
    let mut changed = false;
    for off in 0..STRUCTURES_PER_BEACON {
        let i = beacon * STRUCTURES_PER_BEACON + off;
        if i >= structures.len() || !structures.alive(i) || structures.powered[i] == 0 {
            continue;
        }
        structures.powered[i] = 0;
        changed = true;
        freed = freed.saturating_add(structures.kw_draw[i]);
    }
    (freed, changed)
}

pub fn phase_power(w: &mut World) -> Work {
    let mut work = Work::default();
    for s in 0..w.cfg.seats {
        w.kw_supply[s] = 0;
        w.kw_draw[s] = 0;
    }
    // Dirty tracking for the incremental hash: a table is marked when a byte of
    // it changed, never because the phase ran. See `tick::World::phase_kill_credit`.
    let mut dirty_beacons = false;
    let mut dirty_structures = false;

    for i in 0..w.beacons.len() {
        if !w.beacons.alive(i) {
            dirty_beacons |= w.beacons.powered[i] != 0;
            w.beacons.powered[i] = 0;
            continue;
        }
        dirty_beacons |= w.beacons.powered[i] != 1;
        w.beacons.powered[i] = 1;
        let s = usize::from(w.beacons.seat[i]);
        if s < w.cfg.seats {
            w.kw_supply[s] = w.kw_supply[s].saturating_add(w.beacons.kw_supply[i]);
            w.kw_draw[s] = w.kw_draw[s].saturating_add(w.beacons.kw_draw[i]);
        }
    }
    for i in 0..w.structures.len() {
        if !w.structures.alive(i) {
            dirty_structures |= w.structures.powered[i] != 0;
            w.structures.powered[i] = 0;
            continue;
        }
        dirty_structures |= w.structures.powered[i] != 1;
        w.structures.powered[i] = 1;
        let s = usize::from(w.structures.seat[i]);
        if s < w.cfg.seats {
            w.kw_draw[s] = w.kw_draw[s].saturating_add(w.structures.kw_draw[i]);
        }
    }
    for i in 0..w.units.len() {
        if !w.units.alive(i) {
            continue;
        }
        let s = usize::from(w.units.seat[i]);
        if s < w.cfg.seats {
            w.kw_draw[s] = w.kw_draw[s].saturating_add(UNIT_KW);
        }
    }

    // Brownout, in the total order, re-sorted from the live priorities. The key
    // ends in the unique beacon id, so the comparator is total (AGENTS.md §4.6).
    let mut order = core::mem::take(&mut w.shed_order);
    {
        let beacons = &w.beacons;
        order.sort_unstable_by_key(|&i| {
            let k = usize::try_from(i).expect("beacon index");
            (beacons.priority[k], beacons.seat[k], beacons.id[k])
        });
    }
    for &bi in &order {
        let i = usize::try_from(bi).expect("beacon index");
        if !w.beacons.alive(i) || w.beacons.powered[i] == 0 {
            continue;
        }
        let s = usize::from(w.beacons.seat[i]);
        if s >= w.cfg.seats || w.kw_draw[s] <= w.kw_supply[s] {
            continue;
        }
        w.beacons.powered[i] = 0;
        dirty_beacons = true;
        let (attached, attached_changed) = shed_attached(&mut w.structures, i);
        let freed = w.beacons.kw_draw[i].saturating_add(attached);
        dirty_structures |= attached_changed;
        w.kw_draw[s] = w.kw_draw[s].saturating_sub(freed);
        work.brownouts += 1;
    }
    w.shed_order = order;

    let mut dirty_seats = false;
    for s in 0..w.cfg.seats {
        dirty_seats |= w.seats.rows[s].kw_supply != w.kw_supply[s];
        dirty_seats |= w.seats.rows[s].kw_draw != w.kw_draw[s];
        w.seats.rows[s].kw_supply = w.kw_supply[s];
        w.seats.rows[s].kw_draw = w.kw_draw[s];
        if work.brownouts > 0 {
            w.seats.rows[s].brownouts = w.seats.rows[s].brownouts.saturating_add(1);
            dirty_seats = true;
        }
    }

    if dirty_beacons {
        w.hasher.mark(crate::Table::Beacons);
    }
    if dirty_structures {
        w.hasher.mark(crate::Table::Structures);
    }
    if dirty_seats {
        w.hasher.mark(crate::Table::Seats);
    }
    work
}
