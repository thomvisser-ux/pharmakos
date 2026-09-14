// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later
#![deny(clippy::float_arithmetic, clippy::as_conversions)]
//! The integer cost model, the heuristic, and the cost→ticks rounding rule.
//!
//! Plan §2: *"Costs are integers: 10 for a cardinal step, 14 for a diagonal (the
//! standard integer octile approximation), plus an integer surcharge for a
//! 1-voxel climb. No floats — the lint set forbids them and the heuristic must
//! stay admissible under integer scaling."*
//!
//! # Why the climb surcharge does not break the heuristic
//!
//! The heuristic is the plain octile distance,
//! `h = 14·min(|Δx|,|Δz|) + 10·(max - min)`, computed from the 2D footprint
//! only — it never looks at height.
//!
//! *Admissible.* Any real path from `a` to `b` makes at least `max(|Δx|,|Δz|)`
//! steps, of which at least `min(|Δx|,|Δz|)` must be diagonal to cover both
//! axes. Charging those steps their *base* rates 14 and 10 and nothing else is
//! therefore a lower bound on the real cost, because every real step costs its
//! base rate **plus a non-negative surcharge**. Adding [`CLIMB_SURCHARGE`] can
//! only push the true cost up, never down, so `h` stays below it. Making the
//! heuristic aware of height — say by adding `CLIMB_SURCHARGE · |Δtop|` — is
//! precisely the mistake the plan's §3 step 2 exactness check would catch: a
//! route may descend and re-ascend for free-er than the straight line suggests,
//! and the bound would be violated.
//!
//! *Consistent.* For neighbours `a → b`, `|h(a) - h(b)| <= base(a,b)` is the
//! standard octile result (a single step changes the octile distance by at most
//! the cost it is charged at base rates). Since `cost(a,b) = base(a,b) +
//! surcharge >= base(a,b)`, consistency survives the surcharge as well. A
//! consistent heuristic is what lets [`crate::astar`] close a node the first
//! time it is popped, and it is what makes A\* equal to Dijkstra exactly — the
//! assertion plan §3 step 2 spends 1 000 pairs on.
//!
//! # Overflow
//!
//! Plan §5: costs are `i32` with a documented maximum. A simple path visits at
//! most [`crate::world::NODES`] cells, so no path can cost more than
//! [`MAX_PATH_COST`], which [`assert_cost_headroom`] checks against `i32::MAX`
//! at start-up. 147 456 × 18 = 2 654 208, three orders of magnitude of
//! headroom — and `overflow-checks = true` in every profile turns any surprise
//! into a panic rather than a wrap.

use crate::world::NODES;

/// Cardinal step. The octile 10/14 pair.
pub const STEP_CARDINAL: i32 = 10;
/// Diagonal step. 14/10 = 1.4 ≈ √2, the standard integer approximation, and the
/// reason no square root and therefore no float is needed anywhere.
pub const STEP_DIAGONAL: i32 = 14;

/// Extra cost for a step that changes height by one voxel, up **or** down.
///
/// Charged in both directions on purpose: it keeps the edge relation symmetric,
/// so a path and its reverse cost the same, which the tests lean on and which a
/// real sim wants for e.g. a retreat estimate. 4 is 40% of a cardinal step —
/// enough that a walker prefers a flat detour of up to four cells over one
/// climb, which is roughly how a voxel unit should behave.
///
// PLACEHOLDER: tuning, owner, S3 — the surcharge is a rules-table value in the
// product (AGENTS.md §12 "Tuning values are data"), stamped into the rules hash.
// The spike only has to show that an integer surcharge keeps the heuristic
// admissible, which it does for any value >= 0.
pub const CLIMB_SURCHARGE: i32 = 4;

/// The most a single step can cost.
pub const MAX_STEP_COST: i32 = STEP_DIAGONAL + CLIMB_SURCHARGE;

/// Upper bound on any simple path's cost on this map (plan §5).
///
/// Written out as a literal product because a widening cast is exactly what
/// this file's `#![deny(clippy::as_conversions)]` forbids and `i64::try_from`
/// is not `const`. The two compile-time assertions below tie it to the real
/// values, so it cannot drift if either changes.
pub const MAX_PATH_COST: i64 = 147_456 * 18;
const _: () = assert!(NODES == 147_456);
const _: () = assert!(MAX_STEP_COST == 18);

/// Cost units a unit consumes per 20 Hz tick — its fixed speed.
///
/// 3 per tick against a cardinal step of 10 means a cell takes
/// `ceil(10/3) = 4` ticks, i.e. 5 cells per second. At a half-metre voxel that
/// is 2.5 m/s: a brisk vehicle, slow enough that a 300-cell crossing is a
/// couple of minutes, which is the Push length the design assumes.
///
/// It is deliberately **not** a divisor of 10 or 14, so the rounding rule below
/// is actually exercised rather than being a no-op.
///
// PLACEHOLDER: tuning, owner, S3 — per-unit speeds are a rules-table value; the
// spike needs one number so that "ticks" means something.
pub const MOVE_COST_PER_TICK: i32 = 3;

/// Cost of one step.
#[inline]
#[must_use]
pub const fn step_cost(diagonal: bool, dh: i32) -> i32 {
    let base = if diagonal {
        STEP_DIAGONAL
    } else {
        STEP_CARDINAL
    };
    if dh == 0 {
        base
    } else {
        base + CLIMB_SURCHARGE
    }
}

/// The octile heuristic. Admissible and consistent under this cost model — see
/// the module docs for why the climb surcharge does not disturb either.
#[inline]
#[must_use]
pub const fn octile(dx: i32, dz: i32) -> i32 {
    let ax = dx.abs();
    let az = dz.abs();
    let (lo, hi) = if ax < az { (ax, az) } else { (az, ax) };
    STEP_DIAGONAL * lo + STEP_CARDINAL * (hi - lo)
}

/// **The rounding rule.** Cost → ticks, rounding *up*.
///
/// The unit accumulates [`MOVE_COST_PER_TICK`] cost units per tick and carries
/// the remainder between steps, so a path of total cost `c` is finished on tick
/// `ceil(c / MOVE_COST_PER_TICK)` exactly — this function and
/// [`walk_ticks`] are two computations of the same number, and a test asserts
/// they agree on every path it walks.
///
/// Rounding up rather than to nearest is the honest direction for an ETA: the
/// unit has not arrived until the tick on which it arrives. It also makes the
/// estimator's error one-sided at the sub-tick scale, which keeps the signed
/// error distribution readable (plan §3 step 6 asks for signed errors precisely
/// so systematic bias is visible).
#[inline]
#[must_use]
pub const fn ticks_for_cost(cost: i32) -> i32 {
    if cost <= 0 {
        return 0;
    }
    (cost + MOVE_COST_PER_TICK - 1) / MOVE_COST_PER_TICK
}

/// The fog price: ×1.5, in integers, rounding up.
///
/// Section 11's allowed-estimates row is *"fogged voxels use last-known state
/// or ×1.5 cost"*. `(c * 3 + 1) / 2` is ×1.5 rounded half-up; `c` is bounded by
/// [`MAX_PATH_COST`] so `c * 3` cannot overflow an `i32` for any single edge,
/// and the estimator applies it per edge, never to an accumulated total.
#[inline]
#[must_use]
pub const fn fog_price(cost: i32) -> i32 {
    (cost * 3 + 1) / 2
}

/// Start-up assertion that the map cannot produce a path that overflows the
/// cost width (plan §5 "integer overflow"). Returns the bound it checked.
///
/// # Panics
/// If the bound does not fit an `i32`, which would mean the cost width is wrong
/// for this map size — a build-time-scale mistake, worth a panic at start-up
/// rather than a wrap under one seed on one platform.
pub fn assert_cost_headroom(walkable_cells: usize) -> i64 {
    let bound =
        i64::try_from(walkable_cells).expect("cell count fits i64") * i64::from(MAX_STEP_COST);
    assert!(
        bound <= i64::from(i32::MAX),
        "a path could cost {bound}, which does not fit i32"
    );
    assert!(bound <= MAX_PATH_COST, "more walkable cells than columns");
    bound
}

/// Step a unit along `path`, one 20 Hz tick at a time, and return the tick it
/// arrives on.
///
/// This is the movement integrator plan §3 step 6 asks for, and it is written
/// as a literal tick loop rather than a division on purpose: it is what
/// produces `walked_ticks`, *"the actual tick count when a unit is stepped
/// along the refined HPA\* path by the toy's movement integrator, which is what
/// a player actually experiences"*. It genuinely walks — which is exactly why
/// it lives here and not in [`crate::estimate`], whose contract (section 11's
/// forbidden column) is that it may never step anything.
///
/// The unit gains [`MOVE_COST_PER_TICK`] cost units per tick and **carries the
/// remainder between steps**, so the answer is `ceil(total / MOVE_COST_PER_TICK)`
/// exactly — which [`ticks_for_cost`] computes in one division, and which
/// `the_integrator_agrees_with_the_rounding_rule` asserts on real paths. That
/// equality is the whole reason the estimator is allowed to divide instead of
/// walk.
///
/// Returns `None` if any step on the path is illegal, which is how the caller
/// learns that terrain moved under a stale path.
#[must_use]
pub fn walk_ticks(world: &crate::world::World, path: &[u32]) -> Option<i32> {
    if path.len() < 2 {
        return Some(0);
    }
    let mut idx = 0usize;
    let mut carry: i32 = 0;
    let mut tick: i32 = 0;
    while idx + 1 < path.len() {
        tick = tick.checked_add(1)?;
        carry = carry.checked_add(MOVE_COST_PER_TICK)?;
        while idx + 1 < path.len() {
            let c = world.step_cost_between(path[idx], path[idx + 1])?;
            if carry < c {
                break;
            }
            carry -= c;
            idx += 1;
        }
    }
    Some(tick)
}

/// Total cost of a path, or `None` if a step is illegal.
#[must_use]
pub fn path_cost(world: &crate::world::World, path: &[u32]) -> Option<i32> {
    let mut total: i32 = 0;
    for pair in path.windows(2) {
        total = total.checked_add(world.step_cost_between(pair[0], pair[1])?)?;
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::{
        CLIMB_SURCHARGE, MAX_PATH_COST, MAX_STEP_COST, MOVE_COST_PER_TICK, STEP_CARDINAL,
        STEP_DIAGONAL, assert_cost_headroom, fog_price, octile, step_cost, ticks_for_cost,
    };
    use crate::world::NODES;

    #[test]
    fn the_rounding_rule_is_ceiling() {
        assert_eq!(MOVE_COST_PER_TICK, 3);
        assert_eq!(ticks_for_cost(0), 0);
        assert_eq!(ticks_for_cost(1), 1);
        assert_eq!(ticks_for_cost(3), 1);
        assert_eq!(ticks_for_cost(4), 2);
        assert_eq!(ticks_for_cost(10), 4); // one cardinal step
        assert_eq!(ticks_for_cost(14), 5); // one diagonal step
        assert_eq!(ticks_for_cost(18), 6); // diagonal + climb
        // Ceiling, for every residue, over a wide range.
        for c in 1..10_000 {
            let t = ticks_for_cost(c);
            assert!((t - 1) * MOVE_COST_PER_TICK < c);
            assert!(t * MOVE_COST_PER_TICK >= c);
        }
    }

    #[test]
    fn the_fog_rule_is_three_halves_rounded_up() {
        assert_eq!(fog_price(10), 15);
        assert_eq!(fog_price(14), 21);
        assert_eq!(fog_price(1), 2);
        for c in 0..5_000 {
            let f = fog_price(c);
            assert!(f * 2 >= c * 3);
            assert!((f - 1) * 2 < c * 3);
        }
    }

    #[test]
    fn octile_is_a_lower_bound_on_any_step_sequence() {
        // One step never covers more ground than its base rate buys.
        for dx in -3..=3 {
            for dz in -3..=3 {
                let h = octile(dx, dz);
                let steps = dx.abs().max(dz.abs());
                assert!(h <= steps * STEP_DIAGONAL);
                assert!(h >= steps * STEP_CARDINAL);
            }
        }
        assert_eq!(octile(0, 0), 0);
        assert_eq!(octile(1, 0), STEP_CARDINAL);
        assert_eq!(octile(1, 1), STEP_DIAGONAL);
        assert_eq!(octile(3, 1), STEP_DIAGONAL + 2 * STEP_CARDINAL);
    }

    #[test]
    fn the_heuristic_is_consistent_under_the_surcharge() {
        // |h(a) - h(b)| <= cost(a,b) for every neighbour pair and every legal
        // height change, which is the consistency condition.
        for gx in -4..=4 {
            for gz in -4..=4 {
                for (dx, dz) in crate::world::NEIGHBOURS {
                    let ha = octile(gx, gz);
                    let hb = octile(gx - dx, gz - dz);
                    for dh in [-1, 0, 1] {
                        let c = step_cost(dx != 0 && dz != 0, dh);
                        assert!((ha - hb).abs() <= c, "h jumped by more than the edge cost");
                    }
                }
            }
        }
    }

    #[test]
    fn the_longest_possible_path_does_not_overflow() {
        assert_eq!(MAX_STEP_COST, STEP_DIAGONAL + CLIMB_SURCHARGE);
        assert!(MAX_PATH_COST < i64::from(i32::MAX));
        let bound = assert_cost_headroom(NODES);
        assert_eq!(bound, MAX_PATH_COST);
        // And the sum really is representable: accumulate it as i32.
        let mut acc: i32 = 0;
        for _ in 0..NODES {
            acc = acc.checked_add(MAX_STEP_COST).expect("no overflow");
        }
        assert_eq!(i64::from(acc), MAX_PATH_COST);
    }
}
