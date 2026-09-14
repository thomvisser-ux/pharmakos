// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The synthetic tick, split into the eleven named phases the harness times.
//!
//! Plan §2's table names ten phases. This crate runs **eleven**: `pathing` is
//! added between `programs` and `movement` to stand in for G2, which the plan's
//! "not in the toy" paragraph says the spike substitutes rather than builds.
//! The declared order is [`Phase::ALL`] and is a contract in the same sense the
//! table order in `hash.rs` is: moving a phase moves every hash.
//!
//! | # | Phase | Work |
//! |---|---|---|
//! | 1 | `broadphase` | rebuild the uniform grid; one exact nearest-enemy query per unit, in squared distance |
//! | 2 | `programs` | every unit, building and beacon runs its built-in program |
//! | 3 | `pathing` | integer path-follow along a stored route, plus G2's measured repath and repair costs, charged |
//! | 4 | `movement` | integrate Q16.16 positions, u16 headings through the 4_096-entry angle table |
//! | 5 | `combat` | cooldowns, damage rolls, projectiles, friendly-fire checks, integer HP |
//! | 6 | `kill_credit` | ordered settlement of the tick's damage, <=3 counters per asset, largest-remainder bounties |
//! | 7 | `power` | per-seat kW supply/draw and the brownout order |
//! | 8 | `quartermaster` | integer `$` settlement, upkeep, the rebuild sweep |
//! | 9 | `decision` | the playbook/mandate decision tick, every 5th tick, for every seat |
//! | 10 | `voxels` | this tick's craters into the copy-on-write chunk store |
//! | 11 | `hash` | the per-tick xxh3, full or incremental |
//!
//! **The tick reads no clock and returns no elapsed time.** It returns [`Work`]
//! counters only; `src/bin/tickbench.rs` times each phase from outside by
//! calling [`World::phase`] eleven times in [`Phase::ALL`] order. Plan §5's row
//! on this is unambiguous, and `tests/wall.rs` checks it against the source
//! text rather than trusting the comment.

use crate::broadphase::{Grid, NO_CELL, Population, cell_of, nearest_enemy};
use crate::fixed::{Angle, Fx, Sq};
use crate::hash::{Enc, HashMode, StateHasher, Table};
use crate::rng::{Stream, StreamRng};
use crate::tables::{Beacons, CreditTable, DamageEvent, Projectiles, Seats, Structures, Units};
use crate::voxels::VoxelStore;
use crate::{
    ARRIVE_RADIUS_VOXELS, ATTACK_COOLDOWN, ATTACK_DAMAGE, ATTACK_RANGE_VOXELS, ATTACK_SPREAD,
    BEACON_BOUNTY, BEACON_HP, BEACON_ID_BASE, BEACON_KW_DRAW, BEACON_KW_SUPPLY, DECISION_PERIOD,
    MAP_H_VOXELS, MAP_W_VOXELS, PROJ_ID_BASE, PROJECTILE_TTL, ROUTE_MAX, STRUCT_BOUNTY, STRUCT_HP,
    STRUCT_ID_BASE, STRUCT_KW_DRAW, STRUCTURES_PER_BEACON, TICK_HZ, TURN_RATE, UNIT_BOUNTY,
    UNIT_HP, UNIT_KW, UNIT_SPEED_RAW, apportion, g2, synthetic_work,
};

/// The eleven phases, in declared order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum Phase {
    Broadphase,
    Programs,
    Pathing,
    Movement,
    Combat,
    KillCredit,
    Power,
    Quartermaster,
    Decision,
    Voxels,
    Hash,
}

/// How many phases the tick has.
pub const PHASE_COUNT: usize = 11;

impl Phase {
    /// The declared order. Reordering this moves every hash.
    pub const ALL: [Phase; PHASE_COUNT] = [
        Phase::Broadphase,
        Phase::Programs,
        Phase::Pathing,
        Phase::Movement,
        Phase::Combat,
        Phase::KillCredit,
        Phase::Power,
        Phase::Quartermaster,
        Phase::Decision,
        Phase::Voxels,
        Phase::Hash,
    ];

    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Phase::Broadphase => 0,
            Phase::Programs => 1,
            Phase::Pathing => 2,
            Phase::Movement => 3,
            Phase::Combat => 4,
            Phase::KillCredit => 5,
            Phase::Power => 6,
            Phase::Quartermaster => 7,
            Phase::Decision => 8,
            Phase::Voxels => 9,
            Phase::Hash => 10,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Phase::Broadphase => "broadphase",
            Phase::Programs => "programs",
            Phase::Pathing => "pathing",
            Phase::Movement => "movement",
            Phase::Combat => "combat",
            Phase::KillCredit => "kill_credit",
            Phase::Power => "power",
            Phase::Quartermaster => "quartermaster",
            Phase::Decision => "decision",
            Phase::Voxels => "voxels",
            Phase::Hash => "hash",
        }
    }
}

/// What a phase did, in counters. The tick's only return value: no elapsed
/// time, no clock, nothing the harness could mistake for a measurement it did
/// not take itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Work {
    /// Per-phase visits to a live unit, **not** a unit count: `broadphase` and
    /// `movement` each visit every live unit, so at the gate configuration this
    /// is about two per live unit per tick. Named for what it counts rather
    /// than renamed to hide the doubling, because a reader dividing it by the
    /// tick count should get 600 at 300 units and know why.
    pub units_seen: u32,
    pub programs_run: u32,
    pub queries: u32,
    pub candidates: u32,
    /// Path-follow steps taken in the `pathing` phase. Counted there and
    /// nowhere else, so the total is a count of steps rather than of phases
    /// that happened to touch a unit.
    pub steps: u32,
    pub attacks: u32,
    pub damage_events: u32,
    pub deaths: u32,
    pub repaths_served: u32,
    /// The repath backlog **at the end of this tick**. Merged across ticks it
    /// becomes a sum, which is why the harness divides by the tick count and
    /// publishes a mean backlog rather than a number called "at the end".
    pub repath_backlog: u32,
    pub craters: u32,
    pub voxels_removed: u32,
    pub chunks_rehashed: u32,
    pub rules: u32,
    pub brownouts: u32,
    pub tables_rehashed: u32,
    pub hash_bytes: u64,
    pub state_hash: u64,
    /// Iterations of [`synthetic_work`] the `pathing` phase charged this tick.
    /// The harness multiplies it by its measured picoseconds-per-iteration to
    /// split the phase into its **charged** half (G2's substituted cost) and its
    /// **real** half (the integer path-follow over the unit table). Without that
    /// split the phase's 98% share of the tick reads as measured work.
    pub path_iters: u64,
    /// The synthetic pathing workload's result. Escapes so nothing is elided.
    pub checksum: u64,
}

impl Work {
    pub fn merge(&mut self, o: Work) {
        self.units_seen += o.units_seen;
        self.programs_run += o.programs_run;
        self.queries += o.queries;
        self.candidates += o.candidates;
        self.steps += o.steps;
        self.attacks += o.attacks;
        self.damage_events += o.damage_events;
        self.deaths += o.deaths;
        self.repaths_served += o.repaths_served;
        self.repath_backlog += o.repath_backlog;
        self.craters += o.craters;
        self.voxels_removed += o.voxels_removed;
        self.chunks_rehashed += o.chunks_rehashed;
        self.rules += o.rules;
        self.brownouts += o.brownouts;
        self.tables_rehashed += o.tables_rehashed;
        self.hash_bytes += o.hash_bytes;
        self.path_iters += o.path_iters;
        if o.state_hash != 0 {
            self.state_hash = o.state_hash;
        }
        self.checksum ^= o.checksum;
    }
}

/// The calibrated iteration counts that turn G2's measured milliseconds into
/// work this tick actually does.
///
/// All zero by default, so the library's own tests and any run that does not
/// calibrate measure the sim without the pathing substitute. `tickbench`
/// calibrates against the clock and fills these in; see
/// [`CostConstants::from_calibration`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CostConstants {
    pub iters_per_repath: u64,
    pub iters_per_repath_tail: u64,
    pub iters_per_crater_repair: u64,
    pub iters_csr_per_tick: u64,
    /// Picoseconds per iteration of [`synthetic_work`], as the harness measured
    /// it. Carried here only so the JSON can publish what the numbers above
    /// were derived from.
    pub ps_per_iter: u64,
}

impl CostConstants {
    /// Convert G2's measured nanosecond costs into iteration counts, given a
    /// measured cost per iteration in picoseconds.
    #[must_use]
    pub fn from_calibration(ps_per_iter: u64) -> CostConstants {
        let iters = |ns: u64| -> u64 {
            ns.saturating_mul(1_000)
                .checked_div(ps_per_iter)
                .unwrap_or(0)
        };
        CostConstants {
            iters_per_repath: iters(g2::REPATH_NS_P50),
            // The tail charge is the *extra* over the median, once in every
            // TAIL_ONE_IN served repaths.
            iters_per_repath_tail: iters(g2::REPATH_NS_P99.saturating_sub(g2::REPATH_NS_P50)),
            iters_per_crater_repair: iters(g2::REPAIR_NS_PER_CRATER),
            iters_csr_per_tick: iters(g2::CSR_REBUILD_NS_PER_TICK),
            ps_per_iter,
        }
    }
}

/// The default repath-burst frequency: one tick in 64.
///
/// **PLACEHOLDER: repath burst frequency, owner/G2 at S2 exit.** See
/// [`Config::burst_one_in`] for why this number is a free parameter rather than
/// a measurement, and what it decides.
pub const DEFAULT_BURST_ONE_IN: u32 = 64;

/// The burst multiplier. This one *is* G2's: 8x the ordinary per-tick demand at
/// 300 units puts the peak near G2's observed `repaths_per_tick_peak` of 74.
pub const BURST_FACTOR: u64 = 8;

/// Run-time configuration. Counts are constructor arguments, not `const`s —
/// that is the difference from the G4 spike this crate was copied from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Config {
    pub match_seed: u64,
    pub seats: usize,
    pub units: usize,
    pub beacons: usize,
    pub edits_per_s: u32,
    /// Repaths the `pathing` phase may serve in one tick. [`u32::MAX`] is
    /// "unbounded". The cap is a tuning value (decisions-log item 60: round
    /// robin by seat, beacon, unit); this spike sweeps it.
    pub repath_cap: u32,
    /// One tick in this many carries a burst of 8x the ordinary repath demand.
    /// `0` disables the burst entirely.
    ///
    /// **PLACEHOLDER: repath burst frequency, owner/G2 at S2 exit.** G2 measured
    /// the burst's *magnitude* (`repaths_per_tick_peak: 74`, a once-per-run
    /// maximum) and says nothing about how often such a tick occurs. The 8x
    /// factor is therefore G2's; the 1-in-64 rate is not, and the tick p99 —
    /// and so the G3'-a verdict at an unbounded cap — is a direct readout of
    /// it: at 9_600 ticks a 1-in-64 rate puts ~150 burst ticks above the 97
    /// samples that sit above the p99 index, so the p99 *is* a burst tick,
    /// while at 1-in-128 it is not and the gate flips. `run-measurements.sh`
    /// sweeps {32, 64, 128} and the plan's §9 publishes all three next to the
    /// verdict, because no single value of this constant is a measurement.
    ///
    /// Whatever the rate, the **long-run mean is pinned to G2's measured 9.45
    /// repaths per crater**: the base demand is scaled by `p / (p + 7)` so that
    /// `base * (1 + 7/p)` comes back to 9.45. Changing the rate therefore moves
    /// the tail of the distribution and not its mean, which is what makes the
    /// sweep a sweep of one parameter rather than of two.
    pub burst_one_in: u32,
    pub hash_mode: HashMode,
    pub cost: CostConstants,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            match_seed: crate::DEFAULT_MATCH_SEED,
            seats: 4,
            units: 300,
            beacons: 40,
            edits_per_s: 20,
            repath_cap: u32::MAX,
            burst_one_in: DEFAULT_BURST_ONE_IN,
            hash_mode: HashMode::Full,
            cost: CostConstants::default(),
        }
    }
}

impl Config {
    #[must_use]
    pub fn structures(&self) -> usize {
        self.beacons * STRUCTURES_PER_BEACON
    }
}

/// The repath cost model's own state. **Not hashed, on purpose.**
///
/// Everything in here depends on a calibration taken against the host clock and
/// on the `--repath-cap` flag. If any of it reached the state hash, two machines
/// that calibrate differently would produce different hash streams and the
/// cross-OS comparison in `ci/spike-g3.yml` would fail for a reason that has
/// nothing to do with the sim. `tests/` pins that independence.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PathCost {
    /// Outstanding repath demand, in thousandths of a repath.
    pub queue_milli: u64,
    pub demand_total: u64,
    pub served_total: u64,
    pub tail_counter: u64,
    /// Craters applied on the previous tick; their cluster repair is charged on
    /// this one, which is when a repath would meet the repaired graph.
    pub craters_prev: u32,
    pub checksum: u64,
}

/// The whole world.
#[derive(Clone, Debug)]
pub struct World {
    pub cfg: Config,
    pub tick: u32,

    pub units: Units,
    pub beacons: Beacons,
    pub structures: Structures,
    pub projectiles: Projectiles,
    pub credits: CreditTable,
    pub seats: Seats,
    pub voxels: VoxelStore,

    // Broadphase: two grids, rebuilt every tick.
    pub movers: Grid,
    pub statics: Grid,
    /// Beacons then structures, flattened once at construction. Positions are
    /// static, so only the cell array is recomputed per tick.
    pub static_pos: Vec<[Fx; 3]>,
    pub static_seat: Vec<u8>,
    pub static_id: Vec<u32>,
    mover_cells: Vec<u16>,
    static_cells: Vec<u16>,

    // Scratch, all allocated once.
    pub damage: Vec<DamageEvent>,
    shares: Vec<(u8, i64)>,
    pub kw_supply: Vec<i32>,
    pub kw_draw: Vec<i32>,
    /// Beacon indices in brownout shed order: `(priority, seat, id)`, a total
    /// order because the id is unique.
    ///
    /// Re-sorted by the `power` phase on every tick, because `programs`
    /// rewrites `beacons.priority` from current HP on every tick. Sorting it
    /// once at construction — which is what this spike did first — left the
    /// live priority field write-only and the shed order fixed at the
    /// construction-time priorities, so the code and the comments described an
    /// order the sim did not use. The re-sort is ~1 us at 40 beacons and shows
    /// up in the `power` phase's number, which is where it belongs.
    pub shed_order: Vec<u32>,
    proj_cursor: u32,
    edit_accum: u32,

    // Hashing.
    pub hasher: StateHasher,
    pub enc: Enc,
    pub last_hash: u64,

    // Cost model.
    pub path: PathCost,
}

/// Seat spawn centre: seats evenly around a circle inside the footprint.
#[must_use]
pub(crate) fn seat_centre(seat: u8, seats: usize) -> [Fx; 3] {
    let step: u32 = 65_536 / u32::try_from(seats.max(1)).expect("seat count fits u32");
    let a = Angle(u16::try_from((u32::from(seat) * step) & 0xFFFF).expect("seat angle"));
    let r = Fx::from_voxels(120);
    let mid = Fx::from_voxels(i16::try_from(MAP_W_VOXELS / 2).expect("map centre fits i16"));
    [
        mid.plus(crate::fixed::cos(a).mul_fx(r)),
        mid.plus(crate::fixed::sin(a).mul_fx(r)),
        Fx::from_voxels(28),
    ]
}

impl World {
    /// A fresh match at tick 0. A deterministic function of [`Config`] alone —
    /// except for [`CostConstants`], which by construction touches nothing
    /// hashed.
    #[must_use]
    pub fn new(cfg: Config) -> World {
        assert!(cfg.seats >= 1 && cfg.seats <= 8, "seats out of range");
        assert!(cfg.units >= 1, "units must be positive");
        assert!(cfg.beacons >= 1, "beacons must be positive");

        let structures_n = cfg.structures();
        let mut units = Units::with_count(cfg.units);
        let mut beacons = Beacons::with_count(cfg.beacons);
        let mut structures = Structures::with_count(structures_n);

        for i in 0..cfg.units {
            let id = u32::try_from(i).expect("unit id");
            let seat = u8::try_from(i % cfg.seats).expect("seat id");
            let base = seat_centre(seat, cfg.seats);
            let mut r = StreamRng::new(cfg.match_seed, Stream::Spawn, 0, seat, id);
            let jx = r.range_i32(-(30 << 16), 30 << 16);
            let jy = r.range_i32(-(30 << 16), 30 << 16);
            units.seat[i] = seat;
            units.hp[i] = UNIT_HP;
            units.pos[i] = [
                clamp_xy(base[0].plus(Fx(jx))),
                clamp_xy(base[1].plus(Fx(jy))),
                base[2],
            ];
            units.heading[i] = 0;
            units.program[i] = u8::try_from(i % 4).expect("program id");
            units.kw[i] = UNIT_KW;
        }

        for i in 0..cfg.beacons {
            let seat = u8::try_from(i % cfg.seats).expect("seat id");
            let slot = u32::try_from(i / cfg.seats).expect("beacon slot");
            let per_seat =
                u32::try_from(cfg.beacons.div_ceil(cfg.seats)).expect("per-seat beacons");
            let base = seat_centre(seat, cfg.seats);
            let a = Angle(
                u16::try_from((slot.wrapping_mul(65_536 / per_seat.max(1))) & 0xFFFF)
                    .expect("beacon angle"),
            );
            let rr = Fx::from_voxels(55);
            beacons.seat[i] = seat;
            beacons.hp[i] = BEACON_HP;
            beacons.pos[i] = [
                clamp_xy(base[0].plus(crate::fixed::cos(a).mul_fx(rr))),
                clamp_xy(base[1].plus(crate::fixed::sin(a).mul_fx(rr))),
                base[2],
            ];
            beacons.mandate[i] = u8::try_from(i % 5).expect("mandate id");
            beacons.kw_supply[i] = BEACON_KW_SUPPLY;
            beacons.kw_draw[i] = BEACON_KW_DRAW;
            beacons.priority[i] = u8::try_from(i % 3).expect("priority");
        }

        for i in 0..structures_n {
            let b = i / STRUCTURES_PER_BEACON;
            let off = i % STRUCTURES_PER_BEACON;
            let seat = beacons.seat[b];
            let d = Fx::from_voxels(i16::try_from(8 + off * 6).expect("structure offset"));
            structures.seat[i] = seat;
            structures.hp[i] = STRUCT_HP;
            structures.pos[i] = [
                clamp_xy(beacons.pos[b][0].plus(d)),
                clamp_xy(beacons.pos[b][1].minus(d)),
                beacons.pos[b][2],
            ];
            structures.kind[i] = u8::try_from(off).expect("structure kind");
            structures.program[i] = u8::try_from(i % 3).expect("structure program");
            structures.kw_draw[i] = STRUCT_KW_DRAW;
        }

        let statics_n = cfg.beacons + structures_n;
        let mut static_pos: Vec<[Fx; 3]> = Vec::with_capacity(statics_n);
        let mut static_seat: Vec<u8> = Vec::with_capacity(statics_n);
        let mut static_id: Vec<u32> = Vec::with_capacity(statics_n);
        for i in 0..cfg.beacons {
            static_pos.push(beacons.pos[i]);
            static_seat.push(beacons.seat[i]);
            static_id.push(beacons.id[i]);
        }
        for i in 0..structures_n {
            static_pos.push(structures.pos[i]);
            static_seat.push(structures.seat[i]);
            static_id.push(structures.id[i]);
        }

        // Brownout shed order: a total order, so two machines shed the same
        // beacon. The key ends in the unique beacon id (AGENTS.md §4.6). The
        // `power` phase re-sorts it every tick from the live priorities.
        let mut shed_order: Vec<u32> = (0..cfg.beacons)
            .map(|i| u32::try_from(i).expect("beacon index fits u32"))
            .collect();
        shed_order.sort_unstable_by_key(|&i| {
            let k = usize::try_from(i).expect("beacon index");
            (beacons.priority[k], beacons.seat[k], beacons.id[k])
        });

        let projectiles_n = cfg.units;
        let mut w = World {
            cfg,
            tick: 0,
            credits: CreditTable::with_counts(cfg.units, cfg.beacons, structures_n),
            units,
            beacons,
            structures,
            projectiles: Projectiles::with_count(projectiles_n),
            seats: Seats::with_count(cfg.seats),
            voxels: VoxelStore::new(),
            movers: Grid::with_capacity(cfg.units),
            statics: Grid::with_capacity(statics_n),
            static_pos,
            static_seat,
            static_id,
            mover_cells: vec![NO_CELL; cfg.units],
            static_cells: vec![NO_CELL; statics_n],
            damage: Vec::with_capacity(cfg.units * 2),
            shares: Vec::with_capacity(crate::tables::MAX_CREDITS),
            kw_supply: vec![0; cfg.seats],
            kw_draw: vec![0; cfg.seats],
            shed_order,
            proj_cursor: 0,
            edit_accum: 0,
            hasher: StateHasher::new(),
            // Sized for the largest table's encoding at the per-seat reading
            // (1_200 units), so the hot loop never reallocates.
            enc: Enc::with_capacity(256 * 1024),
            last_hash: 0,
            path: PathCost::default(),
        };
        w.hasher.mark_all();
        w.rehash(true);
        w
    }

    // -----------------------------------------------------------------------
    // Phase dispatch
    // -----------------------------------------------------------------------

    /// Run one phase. The harness calls this eleven times per tick, timing each
    /// call from outside.
    pub fn phase(&mut self, p: Phase) -> Work {
        match p {
            Phase::Broadphase => self.phase_broadphase(),
            Phase::Programs => self.phase_programs(),
            Phase::Pathing => self.phase_pathing(),
            Phase::Movement => self.phase_movement(),
            Phase::Combat => self.phase_combat(),
            Phase::KillCredit => self.phase_kill_credit(),
            Phase::Power => crate::power::phase_power(self),
            Phase::Quartermaster => crate::quartermaster::phase_quartermaster(self),
            Phase::Decision => crate::decision::phase_decision(self),
            Phase::Voxels => self.phase_voxels(),
            Phase::Hash => self.phase_hash(),
        }
    }

    /// One whole 20 Hz tick, in declared phase order.
    pub fn step(&mut self) -> Work {
        let mut w = Work::default();
        for p in Phase::ALL {
            w.merge(self.phase(p));
        }
        w
    }

    #[must_use]
    pub fn is_decision_tick(&self) -> bool {
        self.tick.is_multiple_of(DECISION_PERIOD)
    }

    // -----------------------------------------------------------------------
    // 1. broadphase
    // -----------------------------------------------------------------------

    fn phase_broadphase(&mut self) -> Work {
        // The tick counter advances at the head of the first phase, so a phase
        // never sees two different tick numbers.
        self.tick = self.tick.checked_add(1).expect("tick overflow");

        let mut work = Work::default();
        for i in 0..self.units.len() {
            self.mover_cells[i] = if self.units.alive(i) {
                cell_of(self.units.pos[i])
            } else {
                NO_CELL
            };
        }
        self.movers.rebuild(&self.mover_cells);

        let nb = self.beacons.len();
        for i in 0..self.static_pos.len() {
            let live = if i < nb {
                self.beacons.alive(i)
            } else {
                self.structures.alive(i - nb)
            };
            self.static_cells[i] = if live {
                cell_of(self.static_pos[i])
            } else {
                NO_CELL
            };
        }
        self.statics.rebuild(&self.static_cells);

        let movers = Population {
            grid: &self.movers,
            pos: &self.units.pos,
            seat: &self.units.seat,
            id: &self.units.id,
        };
        let statics = Population {
            grid: &self.statics,
            pos: &self.static_pos,
            seat: &self.static_seat,
            id: &self.static_id,
        };

        let mut examined: u32 = 0;
        for i in 0..self.units.len() {
            if !self.units.alive(i) {
                self.units.nearest[i] = None;
                self.units.nearest_d2[i] = 0;
                continue;
            }
            work.units_seen += 1;
            work.queries += 2;
            let from = self.units.pos[i];
            let seat = self.units.seat[i];
            let a = nearest_enemy(movers, from, seat, &mut examined);
            let b = nearest_enemy(statics, from, seat, &mut examined);
            let best = match (a, b) {
                (Some(x), Some(y)) => Some(if x <= y { x } else { y }),
                (Some(x), None) => Some(x),
                (None, Some(y)) => Some(y),
                (None, None) => None,
            };
            match best {
                Some((d2, id)) => {
                    self.units.nearest[i] = Some(id);
                    self.units.nearest_d2[i] = d2;
                }
                None => {
                    self.units.nearest[i] = None;
                    self.units.nearest_d2[i] = 0;
                }
            }
        }
        work.candidates = examined;
        self.hasher.mark(Table::Units);
        self.hasher.mark(Table::Header);
        work
    }

    // -----------------------------------------------------------------------
    // 2. programs
    // -----------------------------------------------------------------------

    fn phase_programs(&mut self) -> Work {
        let mut work = Work::default();
        let guard_r2 = Sq::of_radius(Fx::from_voxels(48)).0;

        for i in 0..self.units.len() {
            if !self.units.alive(i) {
                self.units.target[i] = None;
                continue;
            }
            work.programs_run += 1;
            let near = self.units.nearest[i];
            let d2 = self.units.nearest_d2[i];
            match self.units.program[i] {
                // Assault: always engage the nearest enemy.
                0 => {
                    self.units.target[i] = near;
                    self.units.stance[i] = u8::from(d2 > 0 && d2 <= guard_r2);
                }
                // Guard: engage only inside the guard radius.
                1 => {
                    self.units.target[i] = if near.is_some() && d2 <= guard_r2 {
                        near
                    } else {
                        None
                    };
                    self.units.stance[i] = 2;
                }
                // Mine: works unless something gets close.
                2 => {
                    let threatened = near.is_some() && d2 <= guard_r2 / 4;
                    self.units.target[i] = if threatened { near } else { None };
                    self.units.stance[i] = u8::from(threatened);
                }
                // Support: no target, regenerates.
                _ => {
                    self.units.target[i] = None;
                    self.units.stance[i] = 3;
                    if self.units.hp[i] < UNIT_HP {
                        self.units.hp[i] += 1;
                    }
                }
            }
        }

        let mut dirty_structures = false;
        for i in 0..self.structures.len() {
            if !self.structures.alive(i) {
                continue;
            }
            work.programs_run += 1;
            if self.structures.powered[i] == 1 {
                dirty_structures = true;
                let rate = 3 + i32::from(self.structures.program[i]);
                self.structures.progress[i] = self.structures.progress[i].saturating_add(rate);
                if self.structures.progress[i] >= 1_000 {
                    self.structures.progress[i] -= 1_000;
                    let b = i / STRUCTURES_PER_BEACON;
                    if b < self.beacons.len() {
                        self.beacons.roster[b] = self.beacons.roster[b].saturating_add(1);
                    }
                }
            }
        }

        for i in 0..self.beacons.len() {
            if !self.beacons.alive(i) {
                continue;
            }
            work.programs_run += 1;
            // The mandate's per-tick effect: the beacon accrues, and its shed
            // priority tracks how hurt it is.
            let m = i64::from(self.beacons.mandate[i]);
            self.beacons.treasury[i] = self.beacons.treasury[i].saturating_add(2 + m);
            let hp_frac = self.beacons.hp[i] / (BEACON_HP / 4).max(1);
            self.beacons.priority[i] = u8::try_from(hp_frac.clamp(0, 3)).expect("priority fits u8");
        }

        // Units and Beacons are genuinely written on every tick here — every
        // alive unit's `target` and `stance`, and every alive beacon's
        // `treasury` and `priority` — so the marks are unconditional because
        // the writes are, not because the phase ran. Structures only progress
        // when they are powered, which a brownout can stop.
        self.hasher.mark(Table::Units);
        self.hasher.mark(Table::Beacons);
        if dirty_structures {
            self.hasher.mark(Table::Structures);
        }
        work
    }

    // -----------------------------------------------------------------------
    // 3. pathing — the G2 substitute
    // -----------------------------------------------------------------------

    /// Path-follow along the stored route, plus G2's measured repath and repair
    /// costs charged as a calibrated synthetic workload.
    ///
    /// The **real** half is hashed: every alive unit advances along its route,
    /// turning toward the current waypoint through the angle table and
    /// retiring waypoints it has arrived at; a unit that runs out of route lays
    /// a new one from its current position toward its target. That is the
    /// "fixed per-unit path-follow cost" plan §2 asks for, and it is real
    /// integer work over real table memory rather than a constant.
    ///
    /// The **charged** half is not hashed and cannot be: it is
    /// [`synthetic_work`] sized from G2's measurements —
    ///
    /// * demand: `9.45` repaths per crater at 300 units, scaled linearly with
    ///   the unit count, with a 1-in-[`Config::burst_one_in`] burst of
    ///   [`BURST_FACTOR`]x so that the per-tick peak lands near G2's observed
    ///   74. The base is scaled by `p / (p + 7)` so the **long-run mean stays at
    ///   G2's measured 9.45** whatever the burst rate is;
    /// * service: at most `--repath-cap` repaths per tick, the rest carried to
    ///   the next tick. **Which units those are is out of scope here**:
    ///   decisions-log item 60's round robin (seat, then beacon, then unit)
    ///   selects a subject, and the charged half of this substitute has no
    ///   subject — it charges a cost without dispatching work to a row. The
    ///   spike therefore measures what a cap costs, not what a cap picks, and
    ///   harness part 1 must not read item 60's rule as exercised here;
    /// * cost: `0.77 ms` each, with the `1.84 ms` p99 charged once in every
    ///   hundred, plus `2.5 ms` of cluster repair per crater of the previous
    ///   tick and `0.5 ms` of CSR rebuild every tick.
    fn phase_pathing(&mut self) -> Work {
        let mut work = Work::default();
        let arrive2 = Sq::of_radius(Fx::from_voxels(
            i16::try_from(ARRIVE_RADIUS_VOXELS).expect("arrive radius fits i16"),
        ))
        .0;

        for i in 0..self.units.len() {
            if !self.units.alive(i) {
                self.units.route_len[i] = 0;
                self.units.route_idx[i] = 0;
                continue;
            }
            work.steps += 1;
            if usize::from(self.units.route_idx[i]) >= usize::from(self.units.route_len[i]) {
                self.lay_route(i);
            }
            let idx = usize::from(self.units.route_idx[i]);
            if idx >= usize::from(self.units.route_len[i]) {
                continue;
            }
            let wp = self.units.route[i][idx];
            let me = self.units.pos[i];
            let dx = wp[0].minus(me[0]);
            let dy = wp[1].minus(me[1]);
            let flat_d2 = i64::from(dx.raw()) * i64::from(dx.raw())
                + i64::from(dy.raw()) * i64::from(dy.raw());
            if flat_d2 <= arrive2 {
                self.units.route_idx[i] += 1;
                continue;
            }
            let desired = Angle::from_delta(dx, dy);
            let h = Angle(self.units.heading[i]).turn_toward(desired, TURN_RATE);
            self.units.heading[i] = h.0;
        }

        // ---- the charged half -------------------------------------------
        let craters = u64::from(self.path.craters_prev);
        if craters > 0 {
            // The base rate, scaled so that `base * (1 + (BURST_FACTOR-1)/p)`
            // comes back to G2's measured 9.45 per crater. Without this the
            // burst would inflate the mean by 10.9% at p = 64, and the mean is
            // the number G3'-b is judged on.
            let p = u64::from(self.cfg.burst_one_in);
            let base_milli = if p == 0 {
                g2::REPATHS_PER_CRATER_MILLI
            } else {
                g2::REPATHS_PER_CRATER_MILLI * p / (p + BURST_FACTOR - 1)
            };
            let per_crater = base_milli
                * u64::try_from(self.units.len()).expect("unit count fits u64")
                / g2::REPATHS_MEASURED_AT_UNITS;
            let mut demand = craters * per_crater;
            // The burst. A stream of its own, so this draw cannot perturb any
            // draw that reaches hashed state.
            if p > 1 {
                let mut r = StreamRng::new(self.cfg.match_seed, Stream::PathCost, self.tick, 0, 0);
                let hi = i32::try_from(p - 1).unwrap_or(i32::MAX);
                if r.range_i32(0, hi) == 0 {
                    demand = demand.saturating_mul(BURST_FACTOR);
                }
            } else if p == 1 {
                demand = demand.saturating_mul(BURST_FACTOR);
            }
            self.path.queue_milli = self.path.queue_milli.saturating_add(demand);
            self.path.demand_total = self.path.demand_total.saturating_add(demand);
        }
        let want = self.path.queue_milli / 1_000;
        let cap = u64::from(self.cfg.repath_cap);
        let served = want.min(cap);
        self.path.queue_milli -= served * 1_000;
        self.path.served_total = self.path.served_total.saturating_add(served);
        work.repaths_served = u32::try_from(served).unwrap_or(u32::MAX);
        work.repath_backlog = u32::try_from(self.path.queue_milli / 1_000).unwrap_or(u32::MAX);

        let mut tails: u64 = 0;
        if served > 0 {
            let before = self.path.tail_counter;
            self.path.tail_counter += served;
            tails = self.path.tail_counter / g2::TAIL_ONE_IN - before / g2::TAIL_ONE_IN;
        }
        let iters = served * self.cfg.cost.iters_per_repath
            + tails * self.cfg.cost.iters_per_repath_tail
            + craters * self.cfg.cost.iters_per_crater_repair
            + self.cfg.cost.iters_csr_per_tick;
        work.path_iters = iters;
        if iters > 0 {
            let c = synthetic_work(iters, self.path.checksum ^ u64::from(self.tick));
            self.path.checksum = c;
            work.checksum = c;
        }

        self.hasher.mark(Table::Units);
        work
    }

    /// Lay a fresh route from the unit's position toward its target.
    fn lay_route(&mut self, i: usize) {
        let me = self.units.pos[i];
        let id = self.units.id[i];
        let seat = self.units.seat[i];
        let dest = match self.units.target[i].or(self.units.nearest[i]) {
            Some(t) => self.asset_pos(t).unwrap_or(me),
            None => {
                let mut r = StreamRng::new(self.cfg.match_seed, Stream::Map, self.tick, seat, id);
                let x = r.range_i32(0, (MAP_W_VOXELS - 1) << 16);
                let y = r.range_i32(0, (MAP_W_VOXELS - 1) << 16);
                [Fx(x), Fx(y), me[2]]
            }
        };
        let n = 4 + usize::try_from(id % 5).expect("route length");
        let n = n.min(ROUTE_MAX);
        let mut r = StreamRng::new(
            self.cfg.match_seed,
            Stream::Map,
            self.tick,
            seat,
            id ^ 0x5A5A,
        );
        for k in 0..n {
            let num = i32::try_from(k + 1).expect("waypoint index");
            let den = i32::try_from(n).expect("route length");
            let fx = me[0].plus(dest[0].minus(me[0]).scale(num).div_fx(Fx::from_voxels(
                i16::try_from(den).expect("route length fits i16"),
            )));
            let fy = me[1].plus(dest[1].minus(me[1]).scale(num).div_fx(Fx::from_voxels(
                i16::try_from(den).expect("route length fits i16"),
            )));
            let jx = r.range_i32(-(6 << 16), 6 << 16);
            let jy = r.range_i32(-(6 << 16), 6 << 16);
            self.units.route[i][k] = [clamp_xy(fx.plus(Fx(jx))), clamp_xy(fy.plus(Fx(jy)))];
        }
        for k in n..ROUTE_MAX {
            self.units.route[i][k] = [Fx::ZERO, Fx::ZERO];
        }
        self.units.route_len[i] = u8::try_from(n).expect("route length fits u8");
        self.units.route_idx[i] = 0;
    }

    // -----------------------------------------------------------------------
    // 4. movement
    // -----------------------------------------------------------------------

    fn phase_movement(&mut self) -> Work {
        let mut work = Work::default();
        let speed = Fx(UNIT_SPEED_RAW);
        let lo = Fx::ZERO;
        let hi = Fx::from_voxels(i16::try_from(MAP_W_VOXELS).expect("map width fits i16"));
        let zhi = Fx::from_voxels(i16::try_from(MAP_H_VOXELS).expect("map height fits i16"));
        for i in 0..self.units.len() {
            if !self.units.alive(i) {
                self.units.step[i] = [Fx::ZERO; 3];
                continue;
            }
            work.units_seen += 1;
            let h = Angle(self.units.heading[i]);
            // The 4_096-entry angle table, twice; no trigonometry at run time.
            let dx = crate::fixed::cos(h).mul_fx(speed);
            let dy = crate::fixed::sin(h).mul_fx(speed);
            let dz = Fx::ZERO;
            self.units.step[i] = [dx, dy, dz];
            self.units.pos[i] = [
                self.units.pos[i][0].plus(dx).clamp_to(lo, hi),
                self.units.pos[i][1].plus(dy).clamp_to(lo, hi),
                self.units.pos[i][2].plus(dz).clamp_to(lo, zhi),
            ];
        }
        self.hasher.mark(Table::Units);
        work
    }

    // -----------------------------------------------------------------------
    // 5. combat
    // -----------------------------------------------------------------------

    fn phase_combat(&mut self) -> Work {
        let mut work = Work::default();
        let range2 = Sq::of_radius(Fx::from_voxels(
            i16::try_from(ATTACK_RANGE_VOXELS).expect("range fits i16"),
        ))
        .0;
        self.damage.clear();
        // See `phase_kill_credit` for why these are tracked rather than marked
        // on entry.
        let mut dirty_units = false;
        let mut dirty_projectiles = false;

        for i in 0..self.units.len() {
            if !self.units.alive(i) {
                continue;
            }
            if self.units.cooldown[i] > 0 {
                self.units.cooldown[i] -= 1;
                dirty_units = true;
                continue;
            }
            let Some(t) = self.units.target[i] else {
                continue;
            };
            let d2 = if self.units.nearest[i] == Some(t) {
                self.units.nearest_d2[i]
            } else {
                match self.asset_pos(t) {
                    Some(p) => Sq::between(self.units.pos[i], p).0,
                    None => continue,
                }
            };
            if d2 > range2 {
                continue;
            }
            let seat = self.units.seat[i];
            let id = self.units.id[i];
            let mut r = StreamRng::new(self.cfg.match_seed, Stream::Combat, self.tick, seat, id);
            let dmg = ATTACK_DAMAGE + r.range_i32(-ATTACK_SPREAD, ATTACK_SPREAD);
            self.units.cooldown[i] = ATTACK_COOLDOWN;
            dirty_units = true;
            if dmg <= 0 {
                continue;
            }
            work.attacks += 1;
            if id.is_multiple_of(3) {
                // Ranged: a projectile, so the projectile table is live state
                // rather than an empty table that costs nothing to hash.
                dirty_projectiles |= self.spawn_projectile(i, t, dmg);
            } else {
                self.damage.push(DamageEvent {
                    asset: t,
                    seat,
                    source: id,
                    dmg,
                });
            }
        }

        // Advance projectiles in id order.
        for p in 0..self.projectiles.len() {
            if self.projectiles.active[p] == 0 {
                continue;
            }
            dirty_projectiles = true;
            self.projectiles.pos[p] = [
                self.projectiles.pos[p][0].plus(self.projectiles.vel[p][0]),
                self.projectiles.pos[p][1].plus(self.projectiles.vel[p][1]),
                self.projectiles.pos[p][2].plus(self.projectiles.vel[p][2]),
            ];
            self.projectiles.ttl[p] -= 1;
            let hit = match self.projectiles.target[p] {
                Some(t) => match self.asset_pos(t) {
                    Some(tp) => Sq::between(self.projectiles.pos[p], tp).0 <= range2,
                    None => false,
                },
                None => false,
            };
            if hit {
                if let Some(t) = self.projectiles.target[p] {
                    self.damage.push(DamageEvent {
                        asset: t,
                        seat: self.projectiles.seat[p],
                        source: self.projectiles.id[p],
                        dmg: self.projectiles.dmg[p],
                    });
                }
                self.projectiles.active[p] = 0;
            } else if self.projectiles.ttl[p] == 0 {
                self.projectiles.active[p] = 0;
            }
        }

        work.damage_events = u32::try_from(self.damage.len()).expect("damage count fits u32");
        if dirty_units {
            self.hasher.mark(Table::Units);
        }
        if dirty_projectiles {
            self.hasher.mark(Table::Projectiles);
        }
        work
    }

    /// Returns whether a projectile slot was actually written.
    fn spawn_projectile(&mut self, unit: usize, target: u32, dmg: i32) -> bool {
        let n = self.projectiles.len();
        if n == 0 {
            return false;
        }
        // Slots are searched from a rolling cursor in id order, so which slot a
        // projectile lands in never depends on scan order or on a container's
        // iteration order.
        let start = usize::try_from(self.proj_cursor).expect("cursor") % n;
        let mut k = 0usize;
        while k < n {
            let p = (start + k) % n;
            if self.projectiles.active[p] == 0 {
                let from = self.units.pos[unit];
                let to = self.asset_pos(target).unwrap_or(from);
                let h = Angle::from_delta(to[0].minus(from[0]), to[1].minus(from[1]));
                let speed = Fx(UNIT_SPEED_RAW.saturating_mul(3));
                self.projectiles.active[p] = 1;
                self.projectiles.seat[p] = self.units.seat[unit];
                self.projectiles.pos[p] = from;
                self.projectiles.vel[p] = [
                    crate::fixed::cos(h).mul_fx(speed),
                    crate::fixed::sin(h).mul_fx(speed),
                    Fx::ZERO,
                ];
                self.projectiles.dmg[p] = dmg;
                self.projectiles.ttl[p] = PROJECTILE_TTL;
                self.projectiles.target[p] = Some(target);
                self.proj_cursor = u32::try_from((p + 1) % n).expect("cursor fits u32");
                return true;
            }
            k += 1;
        }
        false
    }

    // -----------------------------------------------------------------------
    // 6. kill_credit
    // -----------------------------------------------------------------------

    fn phase_kill_credit(&mut self) -> Work {
        let mut work = Work::default();
        // A total order established before anything mutates the world
        // (AGENTS.md §4.6). The key ends in the unique source id.
        self.damage.sort_unstable();

        // Dirty tracking, for the incremental hash mode. A phase marks a table
        // when it **wrote bytes**, not when it ran: marking on entry is what
        // made the first version of this spike's incremental mode degenerate to
        // full (8 of 8 tables re-encoded every tick) and turned the plan's
        // §8 item (3) comparison into a comparison of full against full.
        let mut dirty_units = false;
        let mut dirty_beacons = false;
        let mut dirty_structures = false;
        let mut dirty_credits = false;
        let mut dirty_seats = false;

        let mut events = core::mem::take(&mut self.damage);
        for e in &events {
            let Some(row) = self.credits.slot(e.asset) else {
                continue;
            };
            if !self.asset_alive(e.asset) {
                continue;
            }
            self.apply_damage(e.asset, e.dmg);
            if e.asset >= STRUCT_ID_BASE {
                dirty_structures = true;
            } else if e.asset >= BEACON_ID_BASE {
                dirty_beacons = true;
            } else {
                dirty_units = true;
            }
            let d = u32::try_from(e.dmg).expect("damage is positive here");
            self.credits.record(row, e.seat, d);
            dirty_credits = true;
        }
        events.clear();
        self.damage = events;

        // Deaths, in id order: units, then beacons, then structures.
        for i in 0..self.units.len() {
            if self.units.hp[i] > 0 {
                continue;
            }
            if self.credits.n[i] == 0 {
                continue;
            }
            self.settle(i, UNIT_BOUNTY);
            self.units.hp[i] = 0;
            self.units.target[i] = None;
            self.units.nearest[i] = None;
            self.units.cooldown[i] = 0;
            work.deaths += 1;
            dirty_units = true;
            dirty_credits = true;
            dirty_seats = true;
        }
        let nu = self.units.len();
        for i in 0..self.beacons.len() {
            if self.beacons.hp[i] > 0 || self.credits.n[nu + i] == 0 {
                continue;
            }
            self.settle(nu + i, BEACON_BOUNTY);
            self.beacons.hp[i] = 0;
            self.beacons.kw_draw[i] = 0;
            self.beacons.kw_supply[i] = 0;
            self.beacons.powered[i] = 0;
            work.deaths += 1;
            dirty_beacons = true;
            dirty_credits = true;
            dirty_seats = true;
        }
        let nb = self.beacons.len();
        for i in 0..self.structures.len() {
            if self.structures.hp[i] > 0 || self.credits.n[nu + nb + i] == 0 {
                continue;
            }
            self.settle(nu + nb + i, STRUCT_BOUNTY);
            self.structures.hp[i] = 0;
            self.structures.kw_draw[i] = 0;
            self.structures.powered[i] = 0;
            work.deaths += 1;
            dirty_structures = true;
            dirty_credits = true;
            dirty_seats = true;
        }

        if dirty_units {
            self.hasher.mark(Table::Units);
        }
        if dirty_beacons {
            self.hasher.mark(Table::Beacons);
        }
        if dirty_structures {
            self.hasher.mark(Table::Structures);
        }
        if dirty_credits {
            self.hasher.mark(Table::Credits);
        }
        if dirty_seats {
            self.hasher.mark(Table::Seats);
        }
        work
    }

    fn settle(&mut self, row: usize, bounty: i64) {
        let (entries, n) = self.credits.entries(row);
        let mut shares = core::mem::take(&mut self.shares);
        apportion(bounty, &entries[..n], &mut shares);
        for &(seat, share) in &shares {
            let s = usize::from(seat);
            if s < self.seats.len() {
                self.seats.rows[s].cash += share;
                self.seats.rows[s].kills += 1;
            }
        }
        self.shares = shares;
        self.credits.clear_row(row);
    }

    fn apply_damage(&mut self, asset: u32, dmg: i32) {
        if asset >= PROJ_ID_BASE {
        } else if asset >= STRUCT_ID_BASE {
            let i = usize::try_from(asset - STRUCT_ID_BASE).expect("structure index");
            if i < self.structures.len() {
                self.structures.hp[i] -= dmg;
            }
        } else if asset >= BEACON_ID_BASE {
            let i = usize::try_from(asset - BEACON_ID_BASE).expect("beacon index");
            if i < self.beacons.len() {
                self.beacons.hp[i] -= dmg;
            }
        } else {
            let i = usize::try_from(asset).expect("unit index");
            if i < self.units.len() {
                self.units.hp[i] -= dmg;
            }
        }
    }

    #[must_use]
    pub fn asset_alive(&self, asset: u32) -> bool {
        if asset >= PROJ_ID_BASE {
            false
        } else if asset >= STRUCT_ID_BASE {
            let i = usize::try_from(asset - STRUCT_ID_BASE).expect("structure index");
            i < self.structures.len() && self.structures.alive(i)
        } else if asset >= BEACON_ID_BASE {
            let i = usize::try_from(asset - BEACON_ID_BASE).expect("beacon index");
            i < self.beacons.len() && self.beacons.alive(i)
        } else {
            let i = usize::try_from(asset).expect("unit index");
            i < self.units.len() && self.units.alive(i)
        }
    }

    #[must_use]
    pub fn asset_pos(&self, asset: u32) -> Option<[Fx; 3]> {
        if asset >= PROJ_ID_BASE {
            None
        } else if asset >= STRUCT_ID_BASE {
            let i = usize::try_from(asset - STRUCT_ID_BASE).expect("structure index");
            if i < self.structures.len() && self.structures.alive(i) {
                Some(self.structures.pos[i])
            } else {
                None
            }
        } else if asset >= BEACON_ID_BASE {
            let i = usize::try_from(asset - BEACON_ID_BASE).expect("beacon index");
            if i < self.beacons.len() && self.beacons.alive(i) {
                Some(self.beacons.pos[i])
            } else {
                None
            }
        } else {
            let i = usize::try_from(asset).expect("unit index");
            if i < self.units.len() && self.units.alive(i) {
                Some(self.units.pos[i])
            } else {
                None
            }
        }
    }

    // -----------------------------------------------------------------------
    // 10. voxels
    // -----------------------------------------------------------------------

    fn phase_voxels(&mut self) -> Work {
        let mut work = Work::default();
        self.path.craters_prev = 0;
        if self.cfg.edits_per_s == 0 {
            // Even with no destruction the store still refreshes nothing and
            // the phase costs a branch; that is the honest zero point of the
            // edits sweep.
            return work;
        }
        // Integer accumulator: `edits_per_s` craters spread over TICK_HZ ticks
        // with no remainder drift.
        self.edit_accum += self.cfg.edits_per_s;
        let mut n: u32 = 0;
        while self.edit_accum >= TICK_HZ {
            self.edit_accum -= TICK_HZ;
            n += 1;
        }
        for k in 0..n {
            let mut r = StreamRng::new(self.cfg.match_seed, Stream::Voxel, self.tick, 0, k);
            let cx = r.range_i32(0, MAP_W_VOXELS - 1);
            let cy = r.range_i32(0, MAP_W_VOXELS - 1);
            let cz = r.range_i32(8, 30);
            // Radius 3, the same crater G1 and G2 use.
            work.voxels_removed += self.voxels.crater(cx, cy, cz, 3);
            work.craters += 1;
        }
        work.chunks_rehashed = self.voxels.refresh_digests();
        self.path.craters_prev = n;
        if n > 0 {
            self.hasher.mark(Table::Voxels);
        }
        work
    }

    // -----------------------------------------------------------------------
    // 11. hash
    // -----------------------------------------------------------------------

    fn phase_hash(&mut self) -> Work {
        let mut work = Work::default();
        let full = self.cfg.hash_mode == HashMode::Full;
        let (rehashed, bytes) = self.rehash(full);
        work.tables_rehashed = rehashed;
        work.hash_bytes = bytes;
        work.state_hash = self.last_hash;
        work
    }

    /// Recompute sub-hashes — all of them when `full`, the dirty ones
    /// otherwise — and fold. Returns `(tables rehashed, bytes encoded)`.
    fn rehash(&mut self, full: bool) -> (u32, u64) {
        let mut rehashed: u32 = 0;
        let mut bytes: u64 = 0;
        for t in Table::ALL {
            if !full && !self.hasher.is_dirty(t) {
                continue;
            }
            self.enc.clear();
            self.enc.u8(t.tag());
            match t {
                Table::Header => {
                    self.enc.u64(self.cfg.match_seed);
                    self.enc.u32(self.tick);
                    self.enc.len(self.cfg.seats);
                    self.enc.u32(self.cfg.edits_per_s);
                    // Both of these are path-dependent scalars that decide
                    // hashed state: `proj_cursor` picks which slot a shot
                    // occupies (and so which row of the Projectiles table
                    // carries it), `edit_accum` decides how many craters land
                    // on a given tick. Neither depends on the tick number
                    // alone, so a snapshot taken mid-match and restored without
                    // them would resume with a different future — the exact
                    // AGENTS.md §4.8 pattern ("a field that affects behaviour
                    // but is not hashed is a latent desync"). `PathCost` is the
                    // one deliberate exclusion: it depends on the host clock
                    // calibration, and `tests/spike.rs` pins that exclusion.
                    self.enc.u32(self.proj_cursor);
                    self.enc.u32(self.edit_accum);
                }
                Table::Units => self.units.encode(&mut self.enc),
                Table::Beacons => self.beacons.encode(&mut self.enc),
                Table::Structures => self.structures.encode(&mut self.enc),
                Table::Projectiles => self.projectiles.encode(&mut self.enc),
                Table::Credits => self.credits.encode(&mut self.enc),
                Table::Seats => self.seats.encode(&mut self.enc),
                Table::Voxels => self.voxels.encode(&mut self.enc),
            }
            bytes += u64::try_from(self.enc.byte_len()).expect("encoded length fits u64");
            let h = self.enc.finish();
            self.hasher.set_sub(t, h);
            rehashed += 1;
        }
        self.last_hash = self.hasher.fold();
        (rehashed, bytes)
    }

    /// The per-tick hash as a full re-hash of every table, without disturbing
    /// the cached sub-hashes' dirty flags in a way the caller can observe.
    /// Used by the tests to check the incremental value against a from-scratch
    /// fold every tick.
    pub fn full_hash_now(&mut self) -> u64 {
        let saved = self.hasher.clone();
        let (_, _) = self.rehash(true);
        let h = self.last_hash;
        self.hasher = saved;
        self.last_hash = self.hasher.fold();
        h
    }
}

/// Clamp a horizontal coordinate into the footprint.
#[must_use]
pub(crate) fn clamp_xy(v: Fx) -> Fx {
    v.clamp_to(
        Fx::ZERO,
        Fx::from_voxels(i16::try_from(MAP_W_VOXELS).expect("map width fits i16")),
    )
}
