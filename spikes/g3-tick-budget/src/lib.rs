// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! G3' segment-budget spike — the synthetic tick.
//!
//! What this crate is for, in one sentence: find out how much of a 50 ms tick a
//! 20 Hz Pharmakos match at 4 seats / 300 units / 40 beacons actually spends, so
//! that the map generator's per-map power budget is founded on a measurement
//! rather than a hope. The plan is `docs/spikes/G3prime-segment-budget.md`; its
//! §2 is the file list this crate implements and its §3 is what the two
//! binaries in `src/bin/` do.
//!
//! **It starts from a copy of the G4 spike's toy sim** (`spikes/README.md`
//! rule 2 permits copying between spikes): the same Q16.16 / Q32.32 / `Angle`
//! fixed-point types, the same canonical encoding and xxh3 state hash at the
//! same seed and encoding version, the same counter-based split RNG streams,
//! and the same kill-credit largest-remainder rule with ties to the lowest seat
//! id. What is new here is everything the budget needs: run-time table counts,
//! eleven named phases the harness can time individually, a uniform-grid
//! broadphase, a copy-on-write chunk store with a destruction stream, a
//! power/brownout pass, a decision tick on a 250 ms cadence, and the per-tick
//! state hash in both a full and an incremental form.
//!
//! # The discipline, and where the wall is
//!
//! Everything under `src/` **except `src/bin/`** is sim code in the sense of
//! AGENTS.md §4, and is held to it:
//!
//! * no floating point of either width, and no `as` casts — denied at this
//!   crate root, below, so the denial covers every module through the module
//!   tree;
//! * no hash-keyed containers and no reading of the host clock — checked by
//!   `tests/wall.rs`, which reads this directory's source text, because clippy's
//!   `disallowed-types` is crate-wide and cannot be scoped to a path (see
//!   `clippy.toml`'s header for the whole mechanism);
//! * every sort key ends in a unique id, so every comparator is total;
//! * overflow checks stay on in the measured profile, because that is the
//!   shipping configuration and a budget measured without them is a budget the
//!   game cannot honour.
//!
//! The two binaries are separate crate roots. They may read the clock, they may
//! use floating point for percentiles and least-squares fits, and they are the
//! only things here that do. That split is the same wall the product will need
//! for its own benches, and the spike is prototyping the arrangement.
//!
//! # What the tick deliberately does not contain
//!
//! No mesher and no rendering (G1's), no real pathfinder (G2's — see
//! [`tick::World::phase_pathing`] for the substitute and what calibrates it),
//! no verifier and no gateway. Padding the tick with work the sim does not do
//! would produce a number nobody can act on.

#![deny(clippy::as_conversions)]
#![deny(clippy::float_arithmetic)]
#![deny(clippy::disallowed_types)]

pub mod broadphase;
pub mod decision;
pub mod fixed;
pub mod hash;
pub mod power;
pub mod quartermaster;
pub mod rng;
pub mod tables;
pub mod tick;
pub mod voxels;

pub use hash::{HashMode, Table};
pub use tick::{Phase, Work, World};

// ---------------------------------------------------------------------------
// Geometry and id space
// ---------------------------------------------------------------------------

/// Map footprint along x and y, in whole voxels.
pub const MAP_W_VOXELS: i32 = 384;
/// Map height, in whole voxels.
pub const MAP_H_VOXELS: i32 = 64;
/// The largest query radius any phase asks the broadphase for. The grid cell is
/// this size, so a radius query touches at most 3x3 cells.
pub const QUERY_RADIUS_VOXELS: i32 = 24;

/// Waypoints in a stored route. Fixed-width, so the route is hashable without a
/// length-prefixed variable field.
pub const ROUTE_MAX: usize = 8;

pub const BEACON_ID_BASE: u32 = 1_000_000;
pub const STRUCT_ID_BASE: u32 = 2_000_000;
pub const PROJ_ID_BASE: u32 = 3_000_000;

// ---------------------------------------------------------------------------
// Cadence
// ---------------------------------------------------------------------------

/// Ticks per second.
pub const TICK_HZ: u32 = 20;
/// The decision tick: one per 250 ms of game time, i.e. every 5th tick
/// (plan §2's `decision` row, and section 16's P1 row).
pub const DECISION_PERIOD: u32 = 5;
/// The longest segment of the 3/5/8 ladder, in ticks: 8 x 60 x 20.
pub const SEGMENT_TICKS: u32 = 9_600;
/// One game-minute, in ticks. The drift check of plan §3 step 6 buckets on this.
pub const MINUTE_TICKS: u32 = 1_200;

// ---------------------------------------------------------------------------
// Tuning. In the product these are rules-table data; in the toy they are const.
// ---------------------------------------------------------------------------

pub const UNIT_HP: i32 = 150;
pub const BEACON_HP: i32 = 12_000;
pub const STRUCT_HP: i32 = 2_400;

/// Q16.16 voxels per tick: 0.5 voxels/tick = 10 voxels/s.
pub const UNIT_SPEED_RAW: i32 = 32_768;
/// Angle units per tick: 512/65_536 of a turn = 2.8 deg/tick.
pub const TURN_RATE: u16 = 512;

pub const ATTACK_RANGE_VOXELS: i32 = 6;
pub const ARRIVE_RADIUS_VOXELS: i32 = 2;
pub const ATTACK_DAMAGE: i32 = 7;
pub const ATTACK_SPREAD: i32 = 3;
pub const ATTACK_COOLDOWN: u16 = 12;
pub const PROJECTILE_TTL: u16 = 10;

pub const UNIT_BOUNTY: i64 = 250;
pub const BEACON_BOUNTY: i64 = 1_200;
pub const STRUCT_BOUNTY: i64 = 600;

pub const BEACON_INCOME: i64 = 40;
/// Tuned so that the gate configuration browns out: at 4 seats / 300 units /
/// 40 beacons a seat supplies 950 kW and draws 975 kW, so the `power` phase
/// sheds one beacon per seat on most ticks. A brownout path that never fires is
/// a phase that is in the table but not in the measurement.
pub const BEACON_KW_SUPPLY: i32 = 95;
pub const BEACON_KW_DRAW: i32 = 40;
pub const STRUCT_KW_DRAW: i32 = 25;
pub const UNIT_KW: i32 = 1;
pub const UNIT_UPKEEP: i64 = 1;
pub const STRUCT_UPKEEP: i64 = 2;

/// Rebuild sweep period, in ticks (every 10 game-seconds). Without it the
/// population dies out over an 8-minute run and the last minute measures an
/// empty world — which would read as the leak plan §3 step 6 is hunting.
pub const REBUILD_PERIOD: u32 = 200;
pub const REBUILD_PER_SWEEP: usize = 8;
pub const REBUILD_COST: i64 = 200;

/// Structures per beacon.
pub const STRUCTURES_PER_BEACON: usize = 2;

/// The default match seed. Arbitrary, pinned, and the same across every run so
/// the hash streams are comparable.
pub const DEFAULT_MATCH_SEED: u64 = 0x1234_5678_9ABC_DEF0;

// ---------------------------------------------------------------------------
// G2's measured pathing costs
// ---------------------------------------------------------------------------

/// The G2 spike's measured numbers on the same machine, at 300 units under a
/// 20 craters/s destruction stream, cluster size 32:
///
/// ```json
/// {"repaths_per_crater_mean":9.45,"repaths_per_tick_peak":74,
///  "repath_ms_p50":0.77,"repath_ms_p99":1.84,"repair_ms_per_crater":2.5,
///  "repair_ms_p50_per_dirtied_cluster":1.48,"dirtied_clusters_per_crater":1.35,
///  "csr_rebuild_ms_per_tick":0.5,"no_path_fraction":0.045,"no_path_ms":0.0001}
/// ```
///
/// They are held here as integers (per-mille or nanoseconds) because the
/// library may not name a floating-point type; the harness turns them into
/// iteration counts for the synthetic workload after calibrating it against the
/// clock. Plan §2: *"the spike substitutes a fixed per-unit path-follow cost
/// plus a configurable repath rate, using G2's measured repath cost once it
/// exists"* — it exists, so these are measurements, not placeholders.
pub mod g2 {
    /// Repaths provoked per crater at 300 units, in per-mille: 9.45 -> 9_450.
    pub const REPATHS_PER_CRATER_MILLI: u64 = 9_450;
    /// The unit count G2 measured that rate at. Demand scales from here.
    pub const REPATHS_MEASURED_AT_UNITS: u64 = 300;
    /// Peak repaths in one tick that G2 saw. The burst model is calibrated to
    /// reproduce roughly this peak at 300 units.
    pub const REPATHS_PER_TICK_PEAK: u64 = 74;
    /// A repath at the median, in nanoseconds: 0.77 ms.
    pub const REPATH_NS_P50: u64 = 770_000;
    /// A repath at the 99th percentile, in nanoseconds: 1.84 ms.
    pub const REPATH_NS_P99: u64 = 1_840_000;
    /// One in this many served repaths is charged at the p99 cost instead of
    /// the p50 one, so the tail is in the distribution rather than assumed away.
    pub const TAIL_ONE_IN: u64 = 100;
    /// Cluster repair after one crater, in nanoseconds: 2.5 ms.
    pub const REPAIR_NS_PER_CRATER: u64 = 2_500_000;
    /// The abstract graph's CSR rebuild, per tick, in nanoseconds: 0.5 ms.
    pub const CSR_REBUILD_NS_PER_TICK: u64 = 500_000;
}

// ---------------------------------------------------------------------------
// The synthetic workload
// ---------------------------------------------------------------------------

/// A state-free integer workload, `iters` iterations long.
///
/// This is how G2's measured pathing costs are charged to the tick without
/// building G2 again. Three properties matter and all three are deliberate:
///
/// 1. **State-free.** The result is returned to the caller and folded into the
///    [`Work`] counters, never into hashed state. So the per-tick hash stream
///    does not depend on the iteration count, on the repath cap, or on the
///    machine the calibration was taken on — which is exactly what lets
///    `ci/spike-g3.yml` compare three operating systems' hash streams with
///    `cmp` even though all three calibrate differently.
/// 2. **Not elidable.** The chain is serially dependent and the result escapes
///    through the return value; the harness additionally puts it through
///    `std::hint::black_box`.
/// 3. **Roughly linear in `iters`**, with no branches and no memory traffic, so
///    one calibration constant converts nanoseconds into iterations.
#[must_use]
pub fn synthetic_work(iters: u64, seed: u64) -> u64 {
    let mut z = seed | 1;
    let mut i: u64 = 0;
    while i < iters {
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = z.wrapping_add(i);
        i += 1;
    }
    z
}

// ---------------------------------------------------------------------------
// Largest-remainder apportionment (copied from the G4 spike, behaviour-identical)
// ---------------------------------------------------------------------------

/// Apportion `bounty` over `entries` in proportion to damage, by largest
/// remainder, **ties to the lowest seat id**.
///
/// Exact integer arithmetic in `i128`; the returned shares sum to `bounty`.
/// `out` is cleared and reused rather than returned, so this call allocates
/// nothing once the caller's buffer has grown to [`tables::MAX_CREDITS`]. The
/// settlement of a death as a whole allocates nothing either — but that became
/// true only when [`tables::CreditTable::entries`] stopped returning a `Vec`;
/// until then it was the source of every allocation this spike measured.
pub fn apportion(bounty: i64, entries: &[(u8, u32)], out: &mut Vec<(u8, i64)>) {
    out.clear();
    if bounty <= 0 || entries.is_empty() {
        return;
    }
    let total: i128 = entries.iter().map(|e| i128::from(e.1)).sum();
    if total <= 0 {
        return;
    }
    let b = i128::from(bounty);
    // At most MAX_CREDITS entries, so these small fixed arrays never spill.
    let mut rem: [(i128, u8, usize); tables::MAX_CREDITS] = [(0, 0, 0); tables::MAX_CREDITS];
    let mut assigned: i64 = 0;
    for (idx, &(seat, dmg)) in entries.iter().enumerate() {
        let num = b * i128::from(dmg);
        let q = num / total;
        let r = num - q * total;
        let qi = i64::try_from(q).expect("apportion quota out of range");
        assigned = assigned.checked_add(qi).expect("apportion sum overflow");
        out.push((seat, qi));
        rem[idx] = (r, seat, idx);
    }
    let n = entries.len();
    // Largest remainder first; ties to the lowest seat id. The key ends in the
    // unique seat id, so the comparator is total.
    rem[..n].sort_unstable_by(|a, b2| b2.0.cmp(&a.0).then(a.1.cmp(&b2.1)));
    let mut leftover = bounty - assigned;
    let mut p = 0usize;
    while leftover > 0 && p < n {
        out[rem[p].2].1 += 1;
        leftover -= 1;
        p += 1;
    }
}
