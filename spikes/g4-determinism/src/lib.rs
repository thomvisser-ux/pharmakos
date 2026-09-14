// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! G4 determinism spike — the toy sim.
//!
//! Small enough to read in one sitting, large enough that a platform difference
//! has somewhere to hide: ~200 units and 12 beacons over four seats, a 20 Hz
//! fixed tick, integer-only arithmetic, per-seat treasuries, kill-credit
//! counters apportioned by largest remainder with ties to the lowest seat id.
//!
//! Four seats, not three, and deliberately: an asset belongs to one seat and
//! takes damage only from the others (there is no friendly fire), so with three
//! seats a kill-credit list could never hold more than two entries and the
//! `MAX_CREDITS == 3` cap — the thing the plan singles out as "precisely the kind
//! of thing that goes non-deterministic quietly" — would never be exercised at
//! all. With four seats the three-entry apportionment happens in the trace.
//!
//! Discipline enforced by hand here (and by lints in harness part 1):
//!
//! * no `f32`/`f64` anywhere;
//! * no `as` casts — `From`/`TryFrom` with an explicit failure mode;
//! * no `HashMap`/`HashSet` — sorted `Vec`, `BTreeMap`, `imbl::OrdMap`;
//! * no wall-clock time inside `src/` (the benchmark harnesses in `src/bin/`
//!   may time themselves, and do);
//! * every sort key ends in a unique id, making every comparator total;
//! * overflow checks on in every profile, so a wrap is a panic, not a silent
//!   platform difference.

#![deny(clippy::as_conversions)]
#![deny(clippy::float_arithmetic)]
#![deny(clippy::disallowed_types)]

pub mod fixed;
pub mod hash;
pub mod rng;
pub mod snapshot;

// `fork` is behind the feature at the *module* level, not just the function, so
// a default build does not compile a line of it.
#[cfg(feature = "research")]
pub mod fork;

use fixed::{Angle, Fx, Sq};
use hash::Enc;
use imbl::OrdMap;
use rng::{Stream, StreamRng};

// ---------------------------------------------------------------------------
// Tuning. In the product these are rules-table data; in the toy they are const.
// ---------------------------------------------------------------------------

/// Seats in a match. Four, so that an asset can be damaged by three distinct
/// enemy seats and the `MAX_CREDITS` cap is reachable — see the module docs.
pub const SEATS: usize = 4;
/// Unit slots. Dead units keep their slot and can be rebuilt.
pub const UNIT_COUNT: usize = 200;
/// Beacons. Three per seat.
pub const BEACON_COUNT: usize = 12;
/// Beacon ids start here so that unit ids and beacon ids share one id space and
/// a `target: Option<u32>` is unambiguous.
pub const BEACON_ID_BASE: u32 = 1_000;

/// Ticks per second.
pub const TICK_HZ: u32 = 20;
/// A full toy match: 8 game-minutes at 20 Hz.
pub const MATCH_TICKS: u32 = 9_600;

/// The ten match seeds. Distinct, arbitrary, and pinned.
pub const MATCH_SEEDS: [u64; 10] = [
    0x0000_0000_0000_0001,
    0x1234_5678_9ABC_DEF0,
    0xDEAD_BEEF_CAFE_F00D,
    0x0F1E_2D3C_4B5A_6978,
    0xA5A5_A5A5_5A5A_5A5A,
    0x0000_0000_DEAD_0001,
    0xFFFF_FFFF_FFFF_FFFF,
    0x7FFF_FFFF_FFFF_FFFF,
    0x8000_0000_0000_0000,
    0x0102_0304_0506_0708,
];

const MAP_HALF_VOXELS: i16 = 1_024;
const SPAWN_RADIUS_VOXELS: i16 = 200;
const BEACON_RADIUS_VOXELS: i16 = 60;

const UNIT_HP: i32 = 150;
const BEACON_HP: i32 = 12_000;

/// Q16.16 voxels per tick: 0.5 voxels/tick = 10 voxels/s.
const UNIT_SPEED: Fx = Fx(32_768);
/// Angle units per tick: 512/65_536 of a turn = 2.8 deg/tick = 56 deg/s.
const TURN_RATE: u16 = 512;

const ATTACK_RANGE_VOXELS: i16 = 6;
const ATTACK_DAMAGE: i32 = 7;
const ATTACK_SPREAD: i32 = 3;
const ATTACK_COOLDOWN: u16 = 12;

const UNIT_BOUNTY: i64 = 250;
const BEACON_BOUNTY: i64 = 1_200;

const BEACON_INCOME: i64 = 40;
const BEACON_KW_DRAW: i32 = 40;
const UNIT_UPKEEP: i64 = 1;
const UNIT_KW: i32 = 1;

/// Rebuild sweep period, in ticks (every 10 game-seconds).
const REBUILD_PERIOD: u32 = 200;
/// Units rebuilt per seat per sweep.
const REBUILD_PER_SWEEP: usize = 8;
const REBUILD_COST: i64 = 200;

/// Target re-acquisition period, staggered by unit id so that the scan cost is
/// spread over the period rather than spiking on one tick.
const REACQUIRE_PERIOD: u32 = 20;

/// At most three kill-credit counters per asset, exactly as the spec describes.
///
/// With [`SEATS`] `== 4` and no friendly fire an asset has exactly three
/// possible damagers, so a list of three is reachable and [`apportion`] is
/// exercised with three entries. A *fourth* distinct damager still cannot occur
/// in the toy, so [`Credits::record`]'s eviction branch is unreachable from the
/// sim and is covered by a direct unit test instead (`tests/vectors.rs`).
pub const MAX_CREDITS: usize = 3;

// The eviction branch below is only meaningful while the cap can bind at all.
const _: () = assert!(MAX_CREDITS < SEATS, "MAX_CREDITS must be reachable");

// ---------------------------------------------------------------------------
// State. SoA tables, ordered, fixed-width.
// ---------------------------------------------------------------------------

/// Units, structure-of-arrays. Every vector has the same length and index `i`
/// is the same unit in all of them. `id[i] == i` holds for the toy, but nothing
/// depends on that: the hash walks the tables in id order explicitly.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Units {
    pub id: Vec<u32>,
    pub seat: Vec<u8>,
    pub pos: Vec<[Fx; 3]>,
    pub heading: Vec<u16>,
    pub hp: Vec<i32>,
    pub target: Vec<Option<u32>>,
    pub cooldown: Vec<u16>,
}

impl Units {
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
}

/// Beacons, structure-of-arrays.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Beacons {
    pub id: Vec<u32>,
    pub seat: Vec<u8>,
    pub pos: Vec<[Fx; 3]>,
    pub hp: Vec<i32>,
    pub treasury: Vec<i64>,
    pub kw_draw: Vec<i32>,
}

impl Beacons {
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
}

/// Per-seat treasury and counters.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Seat {
    pub seat: u8,
    pub cash: i64,
    pub kw: i32,
    pub kills: u32,
}

/// Up to three `(seat, damage)` counters for one asset, kept sorted by seat.
/// Seat is unique inside the list, so the sort key is total.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Credits {
    pub entries: Vec<(u8, u32)>,
}

impl Credits {
    /// Add `damage` to `seat`'s counter, creating it if there is room.
    ///
    /// **Ties to the lowest seat id**, in the same sense as [`apportion`]: the
    /// lowest seat id is *favoured*. When the list is full and two entries are
    /// equally small, the one that gets displaced is the one with the **highest**
    /// seat id, so the lowest seat survives the tie.
    ///
    /// `pub` so the eviction branch — unreachable from the toy sim, since four
    /// seats can only produce three distinct damagers — is directly testable.
    pub fn record(&mut self, seat: u8, damage: u32) {
        if let Some(slot) = self.entries.iter_mut().find(|e| e.0 == seat) {
            slot.1 = slot.1.saturating_add(damage);
            return;
        }
        if self.entries.len() < MAX_CREDITS {
            self.entries.push((seat, damage));
            // Keyed on the unique seat id: total order, stable or unstable.
            self.entries.sort_unstable_by_key(|e| e.0);
            return;
        }
        // Full: the smallest contributor is displaced only if it is smaller than
        // the newcomer. On a damage tie the highest seat id is displaced, which
        // is what "ties to the lowest seat id" means everywhere else in the toy —
        // the low seat keeps its slot. `Reverse` on the seat half of the key is
        // what flips it; without it the tuple comparison would evict the lowest.
        let mut worst = 0usize;
        let mut k = 1usize;
        while k < self.entries.len() {
            let (ws, wd) = self.entries[worst];
            let (cs, cd) = self.entries[k];
            if (cd, core::cmp::Reverse(cs)) < (wd, core::cmp::Reverse(ws)) {
                worst = k;
            }
            k += 1;
        }
        if damage > self.entries[worst].1 {
            self.entries[worst] = (seat, damage);
            self.entries.sort_unstable_by_key(|e| e.0);
        }
    }
}

/// The whole world. `Clone` is cheap for the credit table (`OrdMap` is a
/// copy-on-write B-tree) and a plain deep copy for the SoA vectors; `fork`
/// leans on exactly that.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct World {
    pub match_seed: u64,
    pub tick: u32,
    pub units: Units,
    pub beacons: Beacons,
    pub seats: Vec<Seat>,
    /// asset id -> kill-credit counters. `imbl::OrdMap`: ordered iteration and
    /// structural sharing on clone.
    pub credits: OrdMap<u32, Credits>,
    pub deaths: u32,
}

// ---------------------------------------------------------------------------
// Largest-remainder apportionment.
// ---------------------------------------------------------------------------

/// Apportion `bounty` over `entries` in proportion to damage, by largest
/// remainder, **ties to the lowest seat id**.
///
/// Exact integer arithmetic in `i128`; the returned shares sum to `bounty`.
#[must_use]
pub fn apportion(bounty: i64, entries: &[(u8, u32)]) -> Vec<(u8, i64)> {
    if bounty <= 0 || entries.is_empty() {
        return Vec::new();
    }
    let total: i128 = entries.iter().map(|e| i128::from(e.1)).sum();
    if total <= 0 {
        return Vec::new();
    }

    let b = i128::from(bounty);
    let mut shares: Vec<(u8, i64)> = Vec::with_capacity(entries.len());
    let mut remainders: Vec<(i128, u8, usize)> = Vec::with_capacity(entries.len());
    let mut assigned: i64 = 0;

    for (idx, &(seat, dmg)) in entries.iter().enumerate() {
        let num = b * i128::from(dmg);
        let q = num / total;
        let r = num - q * total;
        let qi = i64::try_from(q).expect("apportion quota out of range");
        assigned = assigned.checked_add(qi).expect("apportion sum overflow");
        shares.push((seat, qi));
        remainders.push((r, seat, idx));
    }

    // Largest remainder first; ties to the lowest seat id. The key ends in the
    // unique seat id, so the comparator is total and stable/unstable agree.
    remainders.sort_unstable_by(|a, b2| b2.0.cmp(&a.0).then(a.1.cmp(&b2.1)));

    let mut leftover = bounty - assigned;
    let mut p = 0usize;
    while leftover > 0 && p < remainders.len() {
        let idx = remainders[p].2;
        shares[idx].1 += 1;
        leftover -= 1;
        p += 1;
    }
    shares
}

// ---------------------------------------------------------------------------
// Construction.
// ---------------------------------------------------------------------------

fn seat_centre(seat: u8) -> [Fx; 3] {
    // Seats evenly around a circle. The division truncates when SEATS does not
    // divide 65_536; the truncation is deliberate and identical everywhere.
    let step: u32 = 65_536 / u32::try_from(SEATS).expect("seat count");
    let a = Angle(u16::try_from((u32::from(seat) * step) & 0xFFFF).expect("seat angle"));
    let r = Fx::from_voxels(SPAWN_RADIUS_VOXELS);
    [fixed::cos(a).mul_fx(r), fixed::sin(a).mul_fx(r), Fx::ZERO]
}

impl World {
    /// A fresh match at tick 0. Deterministic function of the seed alone.
    #[must_use]
    pub fn new(match_seed: u64) -> World {
        let mut units = Units::default();
        let centre = [Fx::ZERO, Fx::ZERO, Fx::ZERO];

        for i in 0..UNIT_COUNT {
            let id = u32::try_from(i).expect("unit id");
            let seat = u8::try_from(i % SEATS).expect("seat id");
            let base = seat_centre(seat);
            let mut r = StreamRng::new(match_seed, Stream::Spawn, 0, seat, id);
            let jx = r.range_i32(-(24 << 16), 24 << 16);
            let jy = r.range_i32(-(24 << 16), 24 << 16);
            let jz = r.range_i32(-(8 << 16), 8 << 16);
            let pos = [
                base[0].plus(Fx(jx)),
                base[1].plus(Fx(jy)),
                base[2].plus(Fx(jz)),
            ];
            let heading = Angle::from_delta(
                centre[0].minus(pos[0]),
                centre[1].minus(pos[1]),
            );
            units.id.push(id);
            units.seat.push(seat);
            units.pos.push(pos);
            units.heading.push(heading.0);
            units.hp.push(UNIT_HP);
            units.target.push(None);
            units.cooldown.push(0);
        }

        let mut beacons = Beacons::default();
        for i in 0..BEACON_COUNT {
            let id = BEACON_ID_BASE + u32::try_from(i).expect("beacon id");
            let seat = u8::try_from(i % SEATS).expect("seat id");
            let slot = u32::try_from(i / SEATS).expect("beacon slot");
            let base = seat_centre(seat);
            let a = Angle(u16::try_from((slot * 16_384) & 0xFFFF).expect("beacon angle"));
            let rr = Fx::from_voxels(BEACON_RADIUS_VOXELS);
            let mut r = StreamRng::new(match_seed, Stream::Map, 0, seat, id);
            let jx = r.range_i32(-(8 << 16), 8 << 16);
            let jy = r.range_i32(-(8 << 16), 8 << 16);
            let pos = [
                base[0].plus(fixed::cos(a).mul_fx(rr)).plus(Fx(jx)),
                base[1].plus(fixed::sin(a).mul_fx(rr)).plus(Fx(jy)),
                base[2],
            ];
            beacons.id.push(id);
            beacons.seat.push(seat);
            beacons.pos.push(pos);
            beacons.hp.push(BEACON_HP);
            beacons.treasury.push(0);
            beacons.kw_draw.push(BEACON_KW_DRAW);
        }

        let seats = (0..SEATS)
            .map(|s| Seat {
                seat: u8::try_from(s).expect("seat id"),
                cash: 1_000,
                kw: 0,
                kills: 0,
            })
            .collect();

        World {
            match_seed,
            tick: 0,
            units,
            beacons,
            seats,
            credits: OrdMap::new(),
            deaths: 0,
        }
    }

    // -----------------------------------------------------------------------
    // The tick.
    // -----------------------------------------------------------------------

    /// One 20 Hz tick. Phases run in a fixed order and every loop inside them
    /// runs in id order; nothing is parallel, nothing reads a clock.
    pub fn step(&mut self) {
        self.tick = self.tick.checked_add(1).expect("tick overflow");
        self.phase_acquire();
        self.phase_move();
        self.phase_attack();
        self.phase_economy();
        self.phase_deaths();
        self.phase_rebuild();
    }

    fn asset_pos(&self, id: u32) -> Option<[Fx; 3]> {
        if id >= BEACON_ID_BASE {
            let i = usize::try_from(id - BEACON_ID_BASE).expect("beacon index");
            if i < self.beacons.len() && self.beacons.alive(i) {
                return Some(self.beacons.pos[i]);
            }
            None
        } else {
            let i = usize::try_from(id).expect("unit index");
            if i < self.units.len() && self.units.alive(i) {
                return Some(self.units.pos[i]);
            }
            None
        }
    }

    fn phase_acquire(&mut self) {
        for i in 0..self.units.len() {
            if !self.units.alive(i) {
                self.units.target[i] = None;
                continue;
            }
            let id = self.units.id[i];
            let stale = (self.tick + id).is_multiple_of(REACQUIRE_PERIOD);
            let live = match self.units.target[i] {
                Some(t) => self.asset_pos(t).is_some(),
                None => false,
            };
            if live && !stale {
                continue;
            }

            let me = self.units.pos[i];
            let seat = self.units.seat[i];
            // Nearest enemy asset. Sort key is (squared distance, asset id) and
            // the id is unique, so the choice is total: no tie can be broken by
            // scan order.
            let mut best: Option<(Sq, u32)> = None;
            for j in 0..self.units.len() {
                if !self.units.alive(j) || self.units.seat[j] == seat {
                    continue;
                }
                let d = Sq::between(me, self.units.pos[j]);
                let cand = (d, self.units.id[j]);
                if best.is_none_or(|b| cand < b) {
                    best = Some(cand);
                }
            }
            for j in 0..self.beacons.len() {
                if !self.beacons.alive(j) || self.beacons.seat[j] == seat {
                    continue;
                }
                let d = Sq::between(me, self.beacons.pos[j]);
                let cand = (d, self.beacons.id[j]);
                if best.is_none_or(|b| cand < b) {
                    best = Some(cand);
                }
            }
            self.units.target[i] = best.map(|b| b.1);
        }
    }

    fn phase_move(&mut self) {
        let lo = Fx::from_voxels(-MAP_HALF_VOXELS);
        let hi = Fx::from_voxels(MAP_HALF_VOXELS);
        for i in 0..self.units.len() {
            if !self.units.alive(i) {
                continue;
            }
            let Some(t) = self.units.target[i] else { continue };
            let Some(tp) = self.asset_pos(t) else { continue };
            let me = self.units.pos[i];

            let desired = Angle::from_delta(tp[0].minus(me[0]), tp[1].minus(me[1]));
            let h = Angle(self.units.heading[i]).turn_toward(desired, TURN_RATE);
            self.units.heading[i] = h.0;

            // Hold station once inside weapons range.
            if Sq::between(me, tp) <= Sq::of_radius(Fx::from_voxels(ATTACK_RANGE_VOXELS)) {
                continue;
            }

            let dx = fixed::cos(h).mul_fx(UNIT_SPEED);
            let dy = fixed::sin(h).mul_fx(UNIT_SPEED);
            // Vertical step: move toward the target's altitude, clamped.
            let dzf = tp[2].minus(me[2]);
            let dz = if dzf.raw() > UNIT_SPEED.raw() {
                UNIT_SPEED
            } else if dzf.raw() < -UNIT_SPEED.raw() {
                UNIT_SPEED.negate()
            } else {
                dzf
            };
            self.units.pos[i] = [
                me[0].plus(dx).clamp_to(lo, hi),
                me[1].plus(dy).clamp_to(lo, hi),
                me[2].plus(dz).clamp_to(lo, hi),
            ];
        }
    }

    fn phase_attack(&mut self) {
        let range2 = Sq::of_radius(Fx::from_voxels(ATTACK_RANGE_VOXELS));
        for i in 0..self.units.len() {
            if !self.units.alive(i) {
                continue;
            }
            if self.units.cooldown[i] > 0 {
                self.units.cooldown[i] -= 1;
                continue;
            }
            let Some(t) = self.units.target[i] else { continue };
            let Some(tp) = self.asset_pos(t) else { continue };
            let me = self.units.pos[i];
            if Sq::between(me, tp) > range2 {
                continue;
            }
            let seat = self.units.seat[i];
            let id = self.units.id[i];
            let mut r = StreamRng::new(self.match_seed, Stream::Combat, self.tick, seat, id);
            let roll = r.range_i32(-ATTACK_SPREAD, ATTACK_SPREAD);
            let dmg = ATTACK_DAMAGE + roll;
            if dmg <= 0 {
                self.units.cooldown[i] = ATTACK_COOLDOWN;
                continue;
            }
            self.apply_damage(t, seat, dmg);
            self.units.cooldown[i] = ATTACK_COOLDOWN;
        }
    }

    fn apply_damage(&mut self, asset: u32, by_seat: u8, dmg: i32) {
        let d = u32::try_from(dmg).expect("damage must be positive here");
        if asset >= BEACON_ID_BASE {
            let i = usize::try_from(asset - BEACON_ID_BASE).expect("beacon index");
            if i >= self.beacons.len() || !self.beacons.alive(i) {
                return;
            }
            self.beacons.hp[i] -= dmg;
        } else {
            let i = usize::try_from(asset).expect("unit index");
            if i >= self.units.len() || !self.units.alive(i) {
                return;
            }
            self.units.hp[i] -= dmg;
        }
        // Copy-on-write update of the credit table.
        let mut c = self.credits.get(&asset).cloned().unwrap_or_default();
        c.record(by_seat, d);
        self.credits.insert(asset, c);
    }

    fn phase_economy(&mut self) {
        let mut income = [0i64; SEATS];
        let mut kw = [0i32; SEATS];
        for i in 0..self.beacons.len() {
            if !self.beacons.alive(i) {
                continue;
            }
            let s = usize::from(self.beacons.seat[i]);
            income[s] += BEACON_INCOME;
            kw[s] += self.beacons.kw_draw[i];
            self.beacons.treasury[i] += BEACON_INCOME;
        }
        let mut upkeep = [0i64; SEATS];
        let mut draw = [0i32; SEATS];
        for i in 0..self.units.len() {
            if !self.units.alive(i) {
                continue;
            }
            let s = usize::from(self.units.seat[i]);
            upkeep[s] += UNIT_UPKEEP;
            draw[s] += UNIT_KW;
        }
        for s in 0..SEATS {
            self.seats[s].cash += income[s] - upkeep[s];
            self.seats[s].kw = kw[s] - draw[s];
        }
    }

    fn phase_deaths(&mut self) {
        // Units first, then beacons, each in id order.
        for i in 0..self.units.len() {
            if self.units.hp[i] > 0 {
                continue;
            }
            let id = self.units.id[i];
            let Some(c) = self.credits.get(&id).cloned() else { continue };
            self.settle_death(&c, UNIT_BOUNTY);
            self.credits.remove(&id);
            self.units.hp[i] = 0;
            self.units.target[i] = None;
            self.units.cooldown[i] = 0;
            self.deaths += 1;
        }
        for i in 0..self.beacons.len() {
            if self.beacons.hp[i] > 0 {
                continue;
            }
            let id = self.beacons.id[i];
            let Some(c) = self.credits.get(&id).cloned() else { continue };
            self.settle_death(&c, BEACON_BOUNTY);
            self.credits.remove(&id);
            self.beacons.hp[i] = 0;
            self.beacons.kw_draw[i] = 0;
            self.deaths += 1;
        }
    }

    fn settle_death(&mut self, c: &Credits, bounty: i64) {
        for (seat, share) in apportion(bounty, &c.entries) {
            let s = usize::from(seat);
            self.seats[s].cash += share;
            self.seats[s].kills += 1;
        }
    }

    fn phase_rebuild(&mut self) {
        if !self.tick.is_multiple_of(REBUILD_PERIOD) {
            return;
        }
        let centre = [Fx::ZERO, Fx::ZERO, Fx::ZERO];
        for s in 0..SEATS {
            let seat = u8::try_from(s).expect("seat id");
            let mut built = 0usize;
            for i in 0..self.units.len() {
                if built >= REBUILD_PER_SWEEP {
                    break;
                }
                if self.units.seat[i] != seat || self.units.alive(i) {
                    continue;
                }
                if self.seats[s].cash < REBUILD_COST {
                    break;
                }
                self.seats[s].cash -= REBUILD_COST;
                let id = self.units.id[i];
                let base = seat_centre(seat);
                let mut r = StreamRng::new(self.match_seed, Stream::Spawn, self.tick, seat, id);
                let jx = r.range_i32(-(24 << 16), 24 << 16);
                let jy = r.range_i32(-(24 << 16), 24 << 16);
                let jz = r.range_i32(-(8 << 16), 8 << 16);
                let pos = [
                    base[0].plus(Fx(jx)),
                    base[1].plus(Fx(jy)),
                    base[2].plus(Fx(jz)),
                ];
                self.units.pos[i] = pos;
                self.units.hp[i] = UNIT_HP;
                self.units.cooldown[i] = 0;
                self.units.target[i] = None;
                self.units.heading[i] =
                    Angle::from_delta(centre[0].minus(pos[0]), centre[1].minus(pos[1])).0;
                built += 1;
            }
        }
    }

    // -----------------------------------------------------------------------
    // The canonical encoding and the state hash.
    // -----------------------------------------------------------------------

    /// Append the whole world to `enc` in canonical order. The *only* place
    /// that decides what is hashed; adding a field to state means adding it
    /// here, to the snapshot and to the goldens, in the same change.
    pub fn encode(&self, enc: &mut Enc) {
        enc.clear();
        enc.u64(self.match_seed);
        enc.u32(self.tick);
        enc.u32(self.deaths);

        enc.len(self.units.len());
        for i in 0..self.units.len() {
            enc.u32(self.units.id[i]);
            enc.u8(self.units.seat[i]);
            enc.i32(self.units.pos[i][0].raw());
            enc.i32(self.units.pos[i][1].raw());
            enc.i32(self.units.pos[i][2].raw());
            enc.u16(self.units.heading[i]);
            enc.i32(self.units.hp[i]);
            enc.opt_u32(self.units.target[i]);
            enc.u16(self.units.cooldown[i]);
        }

        enc.len(self.beacons.len());
        for i in 0..self.beacons.len() {
            enc.u32(self.beacons.id[i]);
            enc.u8(self.beacons.seat[i]);
            enc.i32(self.beacons.pos[i][0].raw());
            enc.i32(self.beacons.pos[i][1].raw());
            enc.i32(self.beacons.pos[i][2].raw());
            enc.i32(self.beacons.hp[i]);
            enc.i64(self.beacons.treasury[i]);
            enc.i32(self.beacons.kw_draw[i]);
        }

        enc.len(self.seats.len());
        for s in &self.seats {
            enc.u8(s.seat);
            enc.i64(s.cash);
            enc.i32(s.kw);
            enc.u32(s.kills);
        }

        // OrdMap iterates in key order by construction — no sort needed and no
        // process-dependent ordering possible.
        enc.len(self.credits.len());
        for (asset, c) in self.credits.iter() {
            enc.u32(*asset);
            enc.u8(u8::try_from(c.entries.len()).expect("credit count"));
            for &(seat, dmg) in &c.entries {
                enc.u8(seat);
                enc.u32(dmg);
            }
        }
    }

    /// Per-tick state hash. Allocates its own encoder; the hot loops in
    /// `src/bin/` reuse one via [`World::encode`] instead.
    #[must_use]
    pub fn state_hash(&self) -> u64 {
        let mut enc = Enc::with_capacity(16 * 1024);
        self.encode(&mut enc);
        enc.finish()
    }
}

/// Run one match and return its per-tick hash trace, `ticks` entries long,
/// starting with the hash of the initial state at tick 0.
#[must_use]
pub fn trace_match(match_seed: u64, ticks: u32) -> Vec<u64> {
    let mut w = World::new(match_seed);
    let mut enc = Enc::with_capacity(16 * 1024);
    let mut out = Vec::with_capacity(usize::try_from(ticks).expect("tick count"));
    w.encode(&mut enc);
    out.push(enc.finish());
    for _ in 1..ticks {
        w.step();
        w.encode(&mut enc);
        out.push(enc.finish());
    }
    out
}
