// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The decision tick — plan §2's `decision` row: *"the playbook/mandate
//! decision tick, **one per 250 ms of game time** — every 5th tick — for 4
//! seats. Section 16's P1 row ties the per-rule cost to this cadence."*
//!
//! Nothing here is a playbook interpreter: v1's interpreter is `crates/sim`'s
//! job and this is a throwaway spike. What it is, is *representative work at
//! the right cadence and the right size* — for every seat, on every fifth tick:
//!
//! * per beacon: five mandates scored over six integer terms each, with the
//!   winner written back to the table (ties to the lowest mandate id, so the
//!   choice is total);
//! * per unit: four handler guards evaluated against the unit's own state and
//!   the broadphase's cached nearest-enemy distance, with the winning program
//!   written back;
//! * per seat: one selector — the eight units nearest an enemy, chosen by
//!   `(squared distance, unit id)` into a fixed-size array, so the selector
//!   allocates nothing and its order is total.
//!
//! The cost this produces is what `costmodel` divides by the seat count to get
//! "ns per decision tick per seat", which is the number section 16's P1 row
//! needs. Because the phase does nothing at all on four ticks out of five,
//! `tickbench` records the `decision` distribution over the ticks it *ran* —
//! folding four zeroes in for every sample would put the p50 at zero and say
//! nothing.

use crate::tick::{Work, World};

/// Units a selector returns. Fixed size: a selector that allocated would put
/// its cost on the allocator rather than on the rules.
const SELECT_N: usize = 8;

/// Mandates a beacon chooses between: Build, Defend, Attack, Mine, Survey.
const MANDATES: u8 = 5;

pub fn phase_decision(w: &mut World) -> Work {
    let mut work = Work::default();
    if !w.is_decision_tick() {
        return work;
    }
    let seats = w.cfg.seats;

    for s in 0..seats {
        let seat = u8::try_from(s).expect("seat id fits u8");
        let cash = w.seats.rows[s].cash;
        let margin = i64::from(w.seats.rows[s].kw_supply) - i64::from(w.seats.rows[s].kw_draw);
        let alive = i64::from(w.seats.rows[s].units_alive);

        // ---- beacons: score five mandates over six terms each -------------
        for i in 0..w.beacons.len() {
            if w.beacons.seat[i] != seat || !w.beacons.alive(i) {
                continue;
            }
            let hp = i64::from(w.beacons.hp[i]);
            let roster = i64::from(w.beacons.roster[i]);
            let treasury = w.beacons.treasury[i];
            let mut best: (i64, u8) = (i64::MIN, 0);
            let mut m: u8 = 0;
            while m < MANDATES {
                let k = i64::from(m);
                // Six terms, all integer, all reading real table state: the
                // shape of a utility score, without pretending to be one.
                let score = (hp / 16)
                    + (roster * (3 - k).abs())
                    + (cash / 128) * i64::from(u8::from(m == 0))
                    + margin * i64::from(u8::from(m == 1))
                    + (treasury / 64) * i64::from(u8::from(m == 3))
                    + (alive * (k + 1)) / 4;
                // Strictly greater, so a tie goes to the lowest mandate id.
                if score > best.0 {
                    best = (score, m);
                }
                work.rules += 6;
                m += 1;
            }
            w.beacons.mandate[i] = best.1;
        }

        // ---- units: four handler guards, plus the selector ---------------
        let mut sel: [(i64, u32); SELECT_N] = [(i64::MAX, u32::MAX); SELECT_N];
        for i in 0..w.units.len() {
            if w.units.seat[i] != seat || !w.units.alive(i) {
                continue;
            }
            let hp = i64::from(w.units.hp[i]);
            let d2 = w.units.nearest_d2[i];
            let engaged = w.units.nearest[i].is_some();
            let cool = i64::from(w.units.cooldown[i]);
            work.rules += 4;
            let program: u8 = if hp < i64::from(crate::UNIT_HP) / 4 {
                3
            } else if engaged && d2 <= 1 << 40 {
                0
            } else if engaged && cool == 0 {
                1
            } else {
                2
            };
            w.units.program[i] = program;

            if engaged {
                // Insertion into a fixed-size top-N. The key ends in the unique
                // unit id, so two machines select the same eight units.
                let cand = (d2, w.units.id[i]);
                let mut k = SELECT_N;
                while k > 0 && cand < sel[k - 1] {
                    k -= 1;
                }
                if k < SELECT_N {
                    let mut j = SELECT_N - 1;
                    while j > k {
                        sel[j] = sel[j - 1];
                        j -= 1;
                    }
                    sel[k] = cand;
                }
            }
        }
        for &(_, id) in &sel {
            if id == u32::MAX {
                continue;
            }
            let i = usize::try_from(id).expect("unit index");
            if i < w.units.len() && w.units.alive(i) {
                w.units.stance[i] = 4;
                work.rules += 1;
            }
        }
    }

    w.hasher.mark(crate::Table::Units);
    w.hasher.mark(crate::Table::Beacons);
    work
}
