// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Structure-of-arrays state tables, and their canonical encodings.
//!
//! **What is different from the G4 spike.** G4 sized its tables with `const`
//! counts (200 units, 12 beacons). G3' has to sweep units over
//! `{50..600}` and report a 1_200-unit / 160-beacon per-seat reading, so the
//! counts are constructor arguments and every table is a set of `Vec`s whose
//! length is fixed at construction and never changes afterwards. Dead units
//! keep their slot and can be rebuilt, exactly as in G4 — an id is a slot
//! index, which is what lets the kill-credit table be dense.
//!
//! Every field is fixed-width and every field is hashed. That is the rule
//! AGENTS.md §4.8 calls the one that catches most desyncs, and in a benchmark
//! it has a second edge: a field that is not hashed is also a field the
//! optimiser is free to stop computing, so an unhashed field would quietly
//! make the measurement a lie.
//!
//! Id space, so that a `target: Option<u32>` is unambiguous across tables:
//!
//! | Table | Ids |
//! |---|---|
//! | units | `0 .. units` |
//! | beacons | `BEACON_ID_BASE + 0 .. beacons` |
//! | structures | `STRUCT_ID_BASE + 0 .. structures` |
//! | projectiles | `PROJ_ID_BASE + 0 .. projectiles` |

use crate::fixed::Fx;
use crate::hash::Enc;
use crate::{BEACON_ID_BASE, PROJ_ID_BASE, ROUTE_MAX, STRUCT_ID_BASE};

/// Units: the movers. One row per slot, alive or dead.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Units {
    pub id: Vec<u32>,
    pub seat: Vec<u8>,
    pub hp: Vec<i32>,
    pub pos: Vec<[Fx; 3]>,
    /// This tick's movement delta, computed and applied by `movement` in the
    /// same pass. Hashed because it is table state, not because it crosses a
    /// phase boundary — it does not. (`pathing` writes `heading`, `route` and
    /// `route_idx`; it never touches `step`.)
    pub step: Vec<[Fx; 3]>,
    pub heading: Vec<u16>,
    pub cooldown: Vec<u16>,
    pub target: Vec<Option<u32>>,
    /// Nearest enemy asset and its squared distance, written by `broadphase`
    /// and read by `programs`, `combat` and `decision`.
    pub nearest: Vec<Option<u32>>,
    pub nearest_d2: Vec<i64>,
    /// Which built-in program the unit runs (section 6's "every unit runs its
    /// built-in program"). A small integer; the program itself is a match arm.
    pub program: Vec<u8>,
    pub stance: Vec<u8>,
    pub kw: Vec<i32>,
    /// The stored route the `pathing` phase follows: up to [`ROUTE_MAX`]
    /// waypoints in the horizontal plane.
    pub route: Vec<[[Fx; 2]; ROUTE_MAX]>,
    pub route_len: Vec<u8>,
    pub route_idx: Vec<u8>,
}

impl Units {
    #[must_use]
    pub fn with_count(n: usize) -> Units {
        Units {
            id: (0..n)
                .map(|i| u32::try_from(i).expect("unit id fits u32"))
                .collect(),
            seat: vec![0; n],
            hp: vec![0; n],
            pos: vec![[Fx::ZERO; 3]; n],
            step: vec![[Fx::ZERO; 3]; n],
            heading: vec![0; n],
            cooldown: vec![0; n],
            target: vec![None; n],
            nearest: vec![None; n],
            nearest_d2: vec![0; n],
            program: vec![0; n],
            stance: vec![0; n],
            kw: vec![0; n],
            route: vec![[[Fx::ZERO; 2]; ROUTE_MAX]; n],
            route_len: vec![0; n],
            route_idx: vec![0; n],
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.id.is_empty()
    }

    #[must_use]
    pub fn alive(&self, i: usize) -> bool {
        self.hp[i] > 0
    }

    pub fn encode(&self, enc: &mut Enc) {
        enc.len(self.len());
        for i in 0..self.len() {
            enc.u32(self.id[i]);
            enc.u8(self.seat[i]);
            enc.i32(self.hp[i]);
            enc.i32(self.pos[i][0].raw());
            enc.i32(self.pos[i][1].raw());
            enc.i32(self.pos[i][2].raw());
            enc.i32(self.step[i][0].raw());
            enc.i32(self.step[i][1].raw());
            enc.i32(self.step[i][2].raw());
            enc.u16(self.heading[i]);
            enc.u16(self.cooldown[i]);
            enc.opt_u32(self.target[i]);
            enc.opt_u32(self.nearest[i]);
            enc.i64(self.nearest_d2[i]);
            enc.u8(self.program[i]);
            enc.u8(self.stance[i]);
            enc.i32(self.kw[i]);
            for w in &self.route[i] {
                enc.i32(w[0].raw());
                enc.i32(w[1].raw());
            }
            enc.u8(self.route_len[i]);
            enc.u8(self.route_idx[i]);
        }
    }
}

/// Beacons: the unit of power, and the carrier of a mandate.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Beacons {
    pub id: Vec<u32>,
    pub seat: Vec<u8>,
    pub hp: Vec<i32>,
    pub pos: Vec<[Fx; 3]>,
    /// Build / Defend / Attack / Mine / Survey, as a small integer.
    pub mandate: Vec<u8>,
    pub kw_supply: Vec<i32>,
    pub kw_draw: Vec<i32>,
    /// Set by the `power` phase's brownout pass.
    pub powered: Vec<u8>,
    /// Shed order within a seat. Ties are broken by beacon id, which is unique.
    pub priority: Vec<u8>,
    pub treasury: Vec<i64>,
    pub roster: Vec<u16>,
}

impl Beacons {
    #[must_use]
    pub fn with_count(n: usize) -> Beacons {
        Beacons {
            id: (0..n)
                .map(|i| BEACON_ID_BASE + u32::try_from(i).expect("beacon id fits u32"))
                .collect(),
            seat: vec![0; n],
            hp: vec![0; n],
            pos: vec![[Fx::ZERO; 3]; n],
            mandate: vec![0; n],
            kw_supply: vec![0; n],
            kw_draw: vec![0; n],
            powered: vec![1; n],
            priority: vec![0; n],
            treasury: vec![0; n],
            roster: vec![0; n],
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.id.is_empty()
    }

    #[must_use]
    pub fn alive(&self, i: usize) -> bool {
        self.hp[i] > 0
    }

    pub fn encode(&self, enc: &mut Enc) {
        enc.len(self.len());
        for i in 0..self.len() {
            enc.u32(self.id[i]);
            enc.u8(self.seat[i]);
            enc.i32(self.hp[i]);
            enc.i32(self.pos[i][0].raw());
            enc.i32(self.pos[i][1].raw());
            enc.i32(self.pos[i][2].raw());
            enc.u8(self.mandate[i]);
            enc.i32(self.kw_supply[i]);
            enc.i32(self.kw_draw[i]);
            enc.u8(self.powered[i]);
            enc.u8(self.priority[i]);
            enc.i64(self.treasury[i]);
            enc.u16(self.roster[i]);
        }
    }
}

/// Buildings. Two per beacon in the toy; they run programs and draw power.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Structures {
    pub id: Vec<u32>,
    pub seat: Vec<u8>,
    pub hp: Vec<i32>,
    pub pos: Vec<[Fx; 3]>,
    pub kind: Vec<u8>,
    pub program: Vec<u8>,
    pub kw_draw: Vec<i32>,
    pub progress: Vec<i32>,
    pub powered: Vec<u8>,
}

impl Structures {
    #[must_use]
    pub fn with_count(n: usize) -> Structures {
        Structures {
            id: (0..n)
                .map(|i| STRUCT_ID_BASE + u32::try_from(i).expect("structure id fits u32"))
                .collect(),
            seat: vec![0; n],
            hp: vec![0; n],
            pos: vec![[Fx::ZERO; 3]; n],
            kind: vec![0; n],
            program: vec![0; n],
            kw_draw: vec![0; n],
            progress: vec![0; n],
            powered: vec![1; n],
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.id.is_empty()
    }

    #[must_use]
    pub fn alive(&self, i: usize) -> bool {
        self.hp[i] > 0
    }

    pub fn encode(&self, enc: &mut Enc) {
        enc.len(self.len());
        for i in 0..self.len() {
            enc.u32(self.id[i]);
            enc.u8(self.seat[i]);
            enc.i32(self.hp[i]);
            enc.i32(self.pos[i][0].raw());
            enc.i32(self.pos[i][1].raw());
            enc.i32(self.pos[i][2].raw());
            enc.u8(self.kind[i]);
            enc.u8(self.program[i]);
            enc.i32(self.kw_draw[i]);
            enc.i32(self.progress[i]);
            enc.u8(self.powered[i]);
        }
    }
}

/// Projectiles in flight. A dense table with an `active` flag; slots are
/// recycled in id order, so allocation never depends on scan order.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Projectiles {
    pub id: Vec<u32>,
    pub seat: Vec<u8>,
    pub active: Vec<u8>,
    pub pos: Vec<[Fx; 3]>,
    pub vel: Vec<[Fx; 3]>,
    pub dmg: Vec<i32>,
    pub ttl: Vec<u16>,
    pub target: Vec<Option<u32>>,
}

impl Projectiles {
    #[must_use]
    pub fn with_count(n: usize) -> Projectiles {
        Projectiles {
            id: (0..n)
                .map(|i| PROJ_ID_BASE + u32::try_from(i).expect("projectile id fits u32"))
                .collect(),
            seat: vec![0; n],
            active: vec![0; n],
            pos: vec![[Fx::ZERO; 3]; n],
            vel: vec![[Fx::ZERO; 3]; n],
            dmg: vec![0; n],
            ttl: vec![0; n],
            target: vec![None; n],
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.id.is_empty()
    }

    pub fn encode(&self, enc: &mut Enc) {
        enc.len(self.len());
        for i in 0..self.len() {
            enc.u32(self.id[i]);
            enc.u8(self.seat[i]);
            enc.u8(self.active[i]);
            enc.i32(self.pos[i][0].raw());
            enc.i32(self.pos[i][1].raw());
            enc.i32(self.pos[i][2].raw());
            enc.i32(self.vel[i][0].raw());
            enc.i32(self.vel[i][1].raw());
            enc.i32(self.vel[i][2].raw());
            enc.i32(self.dmg[i]);
            enc.u16(self.ttl[i]);
            enc.opt_u32(self.target[i]);
        }
    }
}

/// At most three kill-credit counters per asset, exactly as the spec describes,
/// and hashed state like everything else (spec section 15 names them).
///
/// Dense rather than a map: one row per damageable asset slot, in the order
/// units, then beacons, then structures. [`CreditTable::slot`] is the only
/// place that mapping is written down.
pub const MAX_CREDITS: usize = 3;

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct CreditTable {
    pub seat: Vec<[u8; MAX_CREDITS]>,
    pub dmg: Vec<[u32; MAX_CREDITS]>,
    pub n: Vec<u8>,
    units: usize,
    beacons: usize,
    structures: usize,
}

impl CreditTable {
    #[must_use]
    pub fn with_counts(units: usize, beacons: usize, structures: usize) -> CreditTable {
        let n = units + beacons + structures;
        CreditTable {
            seat: vec![[0; MAX_CREDITS]; n],
            dmg: vec![[0; MAX_CREDITS]; n],
            n: vec![0; n],
            units,
            beacons,
            structures,
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.n.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.n.is_empty()
    }

    /// Asset id -> row. `None` for a projectile or an out-of-range id.
    #[must_use]
    pub fn slot(&self, asset: u32) -> Option<usize> {
        if asset >= PROJ_ID_BASE {
            None
        } else if asset >= STRUCT_ID_BASE {
            let i = usize::try_from(asset - STRUCT_ID_BASE).expect("structure index");
            if i < self.structures {
                Some(self.units + self.beacons + i)
            } else {
                None
            }
        } else if asset >= BEACON_ID_BASE {
            let i = usize::try_from(asset - BEACON_ID_BASE).expect("beacon index");
            if i < self.beacons {
                Some(self.units + i)
            } else {
                None
            }
        } else {
            let i = usize::try_from(asset).expect("unit index");
            if i < self.units { Some(i) } else { None }
        }
    }

    /// Add `damage` to `seat`'s counter for `row`, creating it if there is room.
    ///
    /// **Ties to the lowest seat id**, in the same sense as [`crate::apportion`]:
    /// when the list is full and two entries are equally small, the one that is
    /// displaced is the one with the **highest** seat id, so the lowest seat
    /// survives the tie. Copied from the G4 spike's `Credits::record` and kept
    /// behaviourally identical on purpose.
    pub fn record(&mut self, row: usize, seat: u8, damage: u32) {
        let n = usize::from(self.n[row]);
        for k in 0..n {
            if self.seat[row][k] == seat {
                self.dmg[row][k] = self.dmg[row][k].saturating_add(damage);
                return;
            }
        }
        if n < MAX_CREDITS {
            self.seat[row][n] = seat;
            self.dmg[row][n] = damage;
            self.n[row] = u8::try_from(n + 1).expect("credit count fits u8");
            self.sort_row(row);
            return;
        }
        // Full: displace the smallest contributor, and on a damage tie the
        // highest seat id, so the low seat keeps its slot.
        let mut worst = 0usize;
        for k in 1..MAX_CREDITS {
            let better = (self.dmg[row][k], core::cmp::Reverse(self.seat[row][k]))
                < (
                    self.dmg[row][worst],
                    core::cmp::Reverse(self.seat[row][worst]),
                );
            if better {
                worst = k;
            }
        }
        if damage > self.dmg[row][worst] {
            self.seat[row][worst] = seat;
            self.dmg[row][worst] = damage;
            self.sort_row(row);
        }
    }

    /// Keep a row sorted by seat id. The key is the unique seat id, so the
    /// order is total and a stable and an unstable sort agree.
    fn sort_row(&mut self, row: usize) {
        let n = usize::from(self.n[row]);
        for a in 1..n {
            let mut b = a;
            while b > 0 && self.seat[row][b - 1] > self.seat[row][b] {
                self.seat[row].swap(b - 1, b);
                self.dmg[row].swap(b - 1, b);
                b -= 1;
            }
        }
    }

    /// The `(seat, damage)` pairs of one row, for apportionment, as a
    /// fixed-size array and a length.
    ///
    /// **Not a `Vec`.** Returning one here was the whole of this spike's
    /// measured allocation traffic: 37 deaths over a 9_600-tick segment is
    /// 0.0039 allocations a tick, and `out/tick.json`'s
    /// `allocs_per_tick_milli: 3` was this call and nothing else. The array is
    /// [`MAX_CREDITS`] long — three `(u8, u32)` pairs — so it is cheaper to copy
    /// than to heap-allocate, and the settlement of a death now really does
    /// allocate nothing.
    #[must_use]
    pub fn entries(&self, row: usize) -> ([(u8, u32); MAX_CREDITS], usize) {
        let n = usize::from(self.n[row]);
        let mut out = [(0u8, 0u32); MAX_CREDITS];
        for (k, slot) in out.iter_mut().enumerate().take(n) {
            *slot = (self.seat[row][k], self.dmg[row][k]);
        }
        (out, n)
    }

    pub fn clear_row(&mut self, row: usize) {
        self.seat[row] = [0; MAX_CREDITS];
        self.dmg[row] = [0; MAX_CREDITS];
        self.n[row] = 0;
    }

    pub fn encode(&self, enc: &mut Enc) {
        enc.len(self.len());
        for row in 0..self.len() {
            enc.u8(self.n[row]);
            for k in 0..MAX_CREDITS {
                enc.u8(self.seat[row][k]);
                enc.u32(self.dmg[row][k]);
            }
        }
    }
}

/// Per-seat scalars: the treasury, the grid totals, the counters.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Seat {
    pub seat: u8,
    pub cash: i64,
    pub kw_supply: i32,
    pub kw_draw: i32,
    pub kills: u32,
    pub brownouts: u32,
    pub units_alive: u32,
    pub spent: i64,
}

/// The per-seat table.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Seats {
    pub rows: Vec<Seat>,
}

impl Seats {
    #[must_use]
    pub fn with_count(n: usize) -> Seats {
        Seats {
            rows: (0..n)
                .map(|s| Seat {
                    seat: u8::try_from(s).expect("seat id fits u8"),
                    cash: 5_000,
                    ..Seat::default()
                })
                .collect(),
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn encode(&self, enc: &mut Enc) {
        enc.len(self.len());
        for r in &self.rows {
            enc.u8(r.seat);
            enc.i64(r.cash);
            enc.i32(r.kw_supply);
            enc.i32(r.kw_draw);
            enc.u32(r.kills);
            enc.u32(r.brownouts);
            enc.u32(r.units_alive);
            enc.i64(r.spent);
        }
    }
}

/// One damage event, gathered by `combat` and settled by `kill_credit`.
///
/// Not hashed: the buffer is cleared at the end of every `kill_credit` phase,
/// so it holds no state across a tick boundary. It is sorted before anything
/// mutates the world (AGENTS.md §4.6), on a key that ends in the unique source
/// id.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct DamageEvent {
    pub asset: u32,
    pub seat: u8,
    pub dmg: i32,
    pub source: u32,
}
