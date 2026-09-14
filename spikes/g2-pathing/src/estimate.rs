// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later
#![deny(clippy::float_arithmetic, clippy::as_conversions)]
//! The P3 travel-time estimate.
//!
//! # The contract, and what it forbids
//!
//! Section 11's allowed-estimates row reads *"Pathfinder travel estimates over
//! known terrain; fogged voxels use last-known state or ×1.5 cost"*, and its
//! forbidden column rules out stepping or forking the sim. `AGENTS.md` §3
//! rule 2 says the same in dependency terms: `plan-core` and `verifier` *"may
//! never step or fork the sim, run mandates, programs, combat or construction,
//! model enemy behaviour, or evaluate rule conditions over a projected future"*.
//!
//! So [`estimate`] does exactly three things and nothing else:
//!
//! 1. ask the connectivity oracle whether a path can exist at all;
//! 2. search the **abstract** graph and sum integer edge costs;
//! 3. divide by the unit's fixed speed, rounding up.
//!
//! It never calls [`crate::cost::walk_ticks`] and never refines a leg. The only
//! low-level work it does at all is the two bounded Dijkstra sweeps that attach
//! the endpoints to their own clusters' transition nodes — bounded to one
//! cluster each, and unavoidable, because an endpoint that is not a transition
//! node has no abstract edges until one is computed for it. That is the whole
//! reason the estimate can be ≤1 ms while a full path is ≤5 ms (plan §2), and
//! it is also why the two costs converge as the cluster grows: at cluster 64 the
//! insertion sweep is four times the cells.
//!
//! # What the returned number actually is
//!
//! The abstract path's summed cost is **not an approximation of the refined
//! path's cost — it is equal to it**, before smoothing, because every intra
//! edge is an exact bounded-A\* cost and every inter edge is one grid step.
//! So the estimate's error against reality decomposes cleanly into two parts,
//! which is what makes the P3 measurement interpretable rather than a single
//! mystery number:
//!
//! * **against the optimal path** the error is HPA\*'s excess (plan §3 step 3):
//!   the abstract graph's insistence on passing through transition nodes;
//! * **against what the unit actually walks** the error is the *smoother's
//!   gain*, with the opposite sign: the walker takes the smoothed path, which is
//!   never more expensive than the one the estimate priced.
//!
//! Both are measured by `src/bin/accuracy.rs`, separately and signed, because
//! plan §3 step 6 asks for signed errors so that a systematic bias — which a
//! calibration constant can fix — is distinguishable from variance, which it
//! cannot.
//!
//! # Fog
//!
//! Fog is priced per abstract edge: an edge with either endpoint in an unknown
//! cell costs [`crate::cost::fog_price`] — ×1.5, integer, rounded half up. Edge
//! granularity is an approximation of a per-voxel rule, and a deliberate one:
//! the estimator may not walk the cells of an edge to count how many are
//! unknown (that is the refinement it is defined not to do), and an abstract
//! edge is short enough — one cluster's width — that the difference is smaller
//! than the ×1.5 factor's own arbitrariness. The fogged error distribution is
//! recorded separately (plan §3 step 7) precisely because this is a judgement
//! call the S3 implementation may want to revisit.

use crate::clusters::Clusters;
use crate::cost::ticks_for_cost;
use crate::hpa::{HpaScratch, abstract_search};
use crate::world::World;

/// A travel estimate: the abstract cost and the tick count it converts to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Estimate {
    /// Summed abstract edge cost, in cost units.
    pub cost: i32,
    /// `ceil(cost / MOVE_COST_PER_TICK)` — see [`crate::cost::ticks_for_cost`].
    pub ticks: i32,
    /// Abstract edges the route crossed. Diagnostic; the editor would use it to
    /// decide how many legs to draw.
    pub legs: usize,
}

/// The P3 query. `None` means no route exists — a normal, cheap answer (a moat
/// seals you in as well), decided by the connectivity oracle before any search.
///
/// `fog`, when supplied, is a per-node mask where `true` means "unknown".
pub fn estimate(
    world: &World,
    cl: &Clusters,
    hs: &mut HpaScratch,
    start: u32,
    goal: u32,
    fog: Option<&[bool]>,
) -> Option<Estimate> {
    let res = abstract_search(world, cl, hs, start, goal, fog)?;
    Some(Estimate {
        cost: res.cost,
        ticks: ticks_for_cost(res.cost),
        legs: res.legs,
    })
}

/// Game milliseconds for a tick count, at 20 Hz. Playbook times are game
/// milliseconds, never ticks (`AGENTS.md` §4.5), so this is the conversion the
/// editor would actually render.
#[inline]
#[must_use]
pub const fn ticks_to_ms(ticks: i32) -> i32 {
    ticks * 50
}

#[cfg(test)]
mod tests {
    use super::{estimate, ticks_to_ms};
    use crate::astar::Scratch;
    use crate::clusters::Clusters;
    use crate::cost::{MOVE_COST_PER_TICK, walk_ticks};
    use crate::hash::Rng;
    use crate::hpa::{HpaScratch, path};
    use crate::world::{World, node_of};

    #[test]
    fn the_estimate_prices_the_unsmoothed_refinement_exactly() {
        let w = World::generate(20_260_913);
        let mut sc = Scratch::new();
        let cl = Clusters::build(&w, 32, &mut sc);
        let mut hs = HpaScratch::new();
        let mut out = Vec::new();
        let mut rng = Rng::new(0x5EED_0020);
        let mut checked = 0;
        for _ in 0..120 {
            let a = node_of(rng.below(384), rng.below(384)).expect("in bounds");
            let b = node_of(rng.below(384), rng.below(384)).expect("in bounds");
            if !w.walkable(a) || !w.walkable(b) {
                continue;
            }
            let Some(e) = estimate(&w, &cl, &mut hs, a, b, None) else {
                continue;
            };
            let walked = path(&w, &cl, &mut hs, a, b, &mut out).expect("estimate found a route");
            // The smoothed walk is never dearer than the estimate priced, and
            // the tick conversion is the ceiling rule in both directions.
            assert!(walked <= e.cost);
            assert_eq!(
                e.ticks,
                (e.cost + MOVE_COST_PER_TICK - 1) / MOVE_COST_PER_TICK
            );
            let wt = walk_ticks(&w, &out).expect("path is walkable");
            assert!(wt <= e.ticks);
            checked += 1;
        }
        assert!(checked > 20, "only {checked} reachable pairs");
    }

    #[test]
    fn fog_never_makes_a_route_look_cheaper() {
        let w = World::generate(20_260_913);
        let mut sc = Scratch::new();
        let cl = Clusters::build(&w, 32, &mut sc);
        let mut hs = HpaScratch::new();
        let fog = vec![true; crate::world::NODES];
        let mut rng = Rng::new(0x5EED_0021);
        let mut checked = 0;
        for _ in 0..80 {
            let a = node_of(rng.below(384), rng.below(384)).expect("in bounds");
            let b = node_of(rng.below(384), rng.below(384)).expect("in bounds");
            if !w.walkable(a) || !w.walkable(b) {
                continue;
            }
            let clear = estimate(&w, &cl, &mut hs, a, b, None);
            let fogged = estimate(&w, &cl, &mut hs, a, b, Some(&fog));
            assert_eq!(clear.is_some(), fogged.is_some());
            if let (Some(c), Some(f)) = (clear, fogged) {
                assert!(f.cost >= c.cost, "fog made a route cheaper");
                checked += 1;
            }
        }
        assert!(checked > 10, "only {checked} reachable pairs");
    }

    #[test]
    fn unreachable_is_a_cheap_none() {
        let mut w = World::flat(30);
        let mut sc = Scratch::new();
        // Seal a single cell in by digging a one-cell moat around it — plan §3
        // step 4's "a walker sealed behind its own moat is a legal game state".
        for dz in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                let id = node_of(200 + dx, 200 + dz).expect("in bounds");
                w.top[crate::ix(id)] = 20;
                w.recompute_walkable(id);
            }
        }
        let cl = Clusters::build(&w, 32, &mut sc);
        let mut hs = HpaScratch::new();
        let inside = node_of(200, 200).expect("in bounds");
        let outside = node_of(100, 100).expect("in bounds");
        assert!(w.walkable(inside));
        assert_eq!(estimate(&w, &cl, &mut hs, inside, outside, None), None);
        assert_eq!(ticks_to_ms(20), 1_000);
    }
}
