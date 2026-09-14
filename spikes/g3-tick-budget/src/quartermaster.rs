// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integer `$` settlement, one treasury per seat — plan §2's `quartermaster`
//! row.
//!
//! It also carries the **rebuild sweep**, which is here rather than in a phase
//! of its own for a measurement reason worth writing down: without it the toy's
//! population dies out over an 8-minute segment, the last minute measures an
//! almost empty world, and the falling tick cost would read as the opposite of
//! the leak plan §3 step 6 is hunting for. The sweep keeps the world at roughly
//! its configured size for the whole run, which is what makes the per-minute
//! p99 curve mean something.
//!
//! Every value is an integer. There is no rounding anywhere, so the treasury is
//! the same number on every platform for the same reason the hash is.

use crate::tick::{Work, World};
use crate::{
    BEACON_INCOME, REBUILD_COST, REBUILD_PER_SWEEP, REBUILD_PERIOD, STRUCT_UPKEEP, UNIT_HP,
    UNIT_UPKEEP,
};

pub fn phase_quartermaster(w: &mut World) -> Work {
    let mut work = Work::default();
    let seats = w.cfg.seats;

    let mut alive_before: [u32; 8] = [0; 8];
    for (s, before) in alive_before.iter_mut().enumerate().take(seats) {
        *before = w.seats.rows[s].units_alive;
        w.seats.rows[s].units_alive = 0;
    }

    let mut income: [i64; 8] = [0; 8];
    let mut upkeep: [i64; 8] = [0; 8];
    // Dirty tracking for the incremental hash: marked on a write, not on entry.
    // See `tick::World::phase_kill_credit`.
    let mut dirty_beacons = false;
    let mut dirty_units = false;

    for i in 0..w.beacons.len() {
        if !w.beacons.alive(i) || w.beacons.powered[i] == 0 {
            continue;
        }
        let s = usize::from(w.beacons.seat[i]);
        if s < seats {
            income[s] += BEACON_INCOME;
        }
        let before = w.beacons.treasury[i];
        w.beacons.treasury[i] = before.saturating_add(BEACON_INCOME);
        dirty_beacons |= w.beacons.treasury[i] != before;
    }
    for i in 0..w.structures.len() {
        if !w.structures.alive(i) {
            continue;
        }
        let s = usize::from(w.structures.seat[i]);
        if s < seats {
            upkeep[s] += STRUCT_UPKEEP;
        }
    }
    for i in 0..w.units.len() {
        if !w.units.alive(i) {
            continue;
        }
        let s = usize::from(w.units.seat[i]);
        if s < seats {
            upkeep[s] += UNIT_UPKEEP;
            w.seats.rows[s].units_alive += 1;
        }
    }
    let mut dirty_seats = false;
    for s in 0..seats {
        let before = w.seats.rows[s].cash;
        w.seats.rows[s].cash = before.saturating_add(income[s] - upkeep[s]);
        dirty_seats |= w.seats.rows[s].cash != before;
        dirty_seats |= w.seats.rows[s].units_alive != alive_before[s];
    }

    // The rebuild sweep, in seat then unit-id order.
    if w.tick.is_multiple_of(REBUILD_PERIOD) {
        for s in 0..seats {
            let seat = u8::try_from(s).expect("seat id fits u8");
            let centre = crate::tick::seat_centre(seat, seats);
            let mut built = 0usize;
            for i in 0..w.units.len() {
                if built >= REBUILD_PER_SWEEP {
                    break;
                }
                if w.units.seat[i] != seat || w.units.alive(i) {
                    continue;
                }
                if w.seats.rows[s].cash < REBUILD_COST {
                    break;
                }
                w.seats.rows[s].cash -= REBUILD_COST;
                w.seats.rows[s].spent += REBUILD_COST;
                dirty_seats = true;
                dirty_units = true;
                let id = w.units.id[i];
                let mut r = crate::rng::StreamRng::new(
                    w.cfg.match_seed,
                    crate::rng::Stream::Spawn,
                    w.tick,
                    seat,
                    id,
                );
                let jx = r.range_i32(-(30 << 16), 30 << 16);
                let jy = r.range_i32(-(30 << 16), 30 << 16);
                w.units.pos[i] = [
                    crate::tick::clamp_xy(centre[0].plus(crate::fixed::Fx(jx))),
                    crate::tick::clamp_xy(centre[1].plus(crate::fixed::Fx(jy))),
                    centre[2],
                ];
                w.units.hp[i] = UNIT_HP;
                w.units.cooldown[i] = 0;
                w.units.target[i] = None;
                w.units.nearest[i] = None;
                w.units.nearest_d2[i] = 0;
                w.units.route_len[i] = 0;
                w.units.route_idx[i] = 0;
                built += 1;
                work.programs_run += 1;
            }
        }
    }

    if dirty_units {
        w.hasher.mark(crate::Table::Units);
    }
    if dirty_beacons {
        w.hasher.mark(crate::Table::Beacons);
    }
    if dirty_seats {
        w.hasher.mark(crate::Table::Seats);
    }
    work
}
