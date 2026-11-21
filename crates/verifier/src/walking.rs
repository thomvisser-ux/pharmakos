// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The commander's walking, as a **lower bound**.
//!
//! Spec section 11 allows "pathfinder travel estimates over known terrain",
//! and the estimator that makes them is the sim's pathing module, which the
//! gateway lends to `estimate_route` and `resolve_refs`. The verifier holds no
//! pathing graph and names none: `tests/confinement.rs`'s
//! `the_verifier_never_ranks` keeps the pathfinder out of this crate, because a
//! verifier that could price travel could rank by it, and ranking is the
//! sim's and the gateway's (docs/design/targeting.md, "Surfaces").
//!
//! What it can do without the graph is arithmetic that **no path can beat**:
//! the octile distance over the leg's horizontal delta, at the rules table's
//! step costs, divided by the commander's speed and rounded **down** to the
//! tick. Every real step costs at least its base rate (a climb only adds a
//! non-negative surcharge), so the walk the sim takes is never shorter than
//! this figure. It is the same admissible heuristic the sim's own search is
//! guided by, and it reads no terrain, so it knows nothing the snapshot would
//! have to be stepped to learn.
//!
//! **Why down.** The sim's walker adds its speed to an accumulator every tick
//! and spends it a step at a time, and it keeps what is left over from one leg
//! into the next. The leftover is less than one tick's speed, so a leg can end
//! one tick before `ceil(cost x TICK_HZ / speed)`, but never before
//! `floor(cost x TICK_HZ / speed)`. At the committed rows (step costs 10 and
//! 14, the commander's 10 a second) every step's price is a whole number of
//! ticks, nothing is ever left over, and the two roundings agree.
//!
//! A lower bound is the honest number for the two codes that use it: `I0001`
//! says how much walking the route is at least, and `W0701` adds the walking
//! to the interface time to say the route does not fit. Neither claims an
//! arrival time. `W0701` proves the route cannot fit at the ruled interface
//! rates; until the sim charges those rates the Push may charge a visit less
//! (`crate::interface`, "Until the sim prices a visit the same way").
//!
//! The gateway pricing each known leg with the sim's estimator and handing the
//! figures in with the seat's view would let `W0701` warn on a route that
//! probably does not fit rather than only on one that cannot. That needs a
//! `Scope` field the gateway fills, which is the same shape as the segment
//! clock and the "fits" pill (the register's S3-16).
//!
//! PLACEHOLDER: pathfinder-quality travel per known leg for W0701 — owner, S6, with the segment clock (S3's plan, section 2.1, row S3-19).
//!
//! # The radius at each end
//!
//! A leg is done when the commander stands within a radius of its target, by
//! the sim's 3-D test: `commander.arrive_radius_voxels` for a move, and
//! `commander.interface_range_voxels` for a visit or a deploy, which walk in to
//! that range before they start. Within a radius `r` in three dimensions, the
//! commander is within `r` on each horizontal axis, so each axis's distance is
//! reduced by the radius at the far end, and by the radius the commander may
//! have stopped short of the near end by, before the octile sum.

use core::num::{NonZeroU64, NonZeroU128};

use pharmakos_proto::gp::v1::Voxel;
use pharmakos_sim::math::quantity::{Ms, TICK_HZ};

use crate::interface::WideMs;
use pharmakos_sim::rules::RulesTable;

use crate::limits::RulesGap;

/// The rows the walking bound reads, out of the rules table once.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Walk {
    cardinal: i64,
    diagonal: i64,
    /// Positive by construction, so the bound divides by it without a case
    /// for zero.
    cost_per_second: NonZeroU64,
    arrive_radius: i64,
    interface_range: i64,
}

impl Walk {
    /// Reads the rows.
    ///
    /// # Errors
    ///
    /// [`RulesGap::MissingBlock`] without a `commander` block;
    /// [`RulesGap::Negative`] for a negative step cost or climb surcharge,
    /// either of which would let a real walk undercut the bound; and
    /// [`RulesGap::NotPositive`] for a commander speed of zero or less, which
    /// the bound divides by.
    pub fn from_rules(rules: &RulesTable) -> Result<Walk, RulesGap> {
        let commander = rules
            .message()
            .commander
            .as_ref()
            .ok_or(RulesGap::MissingBlock("commander"))?;
        let non_negative = |field: &'static str, value: i32| -> Result<i64, RulesGap> {
            if value < 0 {
                Err(RulesGap::Negative {
                    field,
                    value: i64::from(value),
                })
            } else {
                Ok(i64::from(value))
            }
        };
        let cardinal = non_negative("locomotion.step_cost_cardinal", rules.step_cardinal())?;
        let diagonal = non_negative("locomotion.step_cost_diagonal", rules.step_diagonal())?;
        non_negative("locomotion.climb_surcharge", rules.climb_surcharge())?;
        let speed = i64::from(rules.commander_cost_per_second());
        let cost_per_second =
            u64::try_from(speed)
                .ok()
                .and_then(NonZeroU64::new)
                .ok_or(RulesGap::NotPositive {
                    field: "commander.cost_per_second",
                    value: speed,
                })?;
        Ok(Walk {
            cardinal,
            diagonal,
            cost_per_second,
            arrive_radius: i64::from(commander.arrive_radius_voxels),
            interface_range: i64::from(commander.interface_range_voxels),
        })
    }

    /// How close a move stops: `commander.arrive_radius_voxels`.
    #[must_use]
    pub const fn arrive_radius(&self) -> i64 {
        self.arrive_radius
    }

    /// How close a visit or a deploy walks in to:
    /// `commander.interface_range_voxels`.
    #[must_use]
    pub const fn interface_range(&self) -> i64 {
        self.interface_range
    }

    /// The least walking one leg can take: from `from`, where the commander
    /// may have stopped up to `slack` voxels short, to within `radius` voxels
    /// of `to`.
    ///
    /// In `u128`, exact by the types (S3's `vfyq2`, fix pass 4, item 1: it
    /// saturated, and lowered a leg past `u32::MAX` ticks to it). `slack` and
    /// `radius` are `i64`s, so `reach` is under 2^64; an axis is two `i32`s
    /// apart, under 2^33; a step cost is an `i64`, its double under 2^64; so
    /// the cost is under 2^98, times `TICK_HZ` under 2^103, and the leg in
    /// milliseconds under 2^110. Nothing wraps.
    #[must_use]
    pub fn leg(&self, from: Voxel, slack: i64, to: Voxel, radius: i64) -> WideMs {
        let reach = i128::from(slack).wrapping_add(i128::from(radius)).max(0);
        let axis = |a: i32, b: i32| -> u128 {
            let apart = i128::from(b).wrapping_sub(i128::from(a)).unsigned_abs();
            // What is left past the reach, floored at none: the leg ends
            // anywhere within it. `reach` is at least zero, so its magnitude
            // is itself.
            let reach = reach.unsigned_abs();
            if apart > reach {
                apart.wrapping_sub(reach)
            } else {
                0
            }
        };
        let (dx, dy) = (axis(from.x, to.x), axis(from.y, to.y));
        let (low, high) = if dx < dy { (dx, dy) } else { (dy, dx) };
        // Whatever the rows say, a diagonal step is never charged more than
        // two cardinal ones, nor a straight voxel of progress less than the
        // cheaper step, so the sum stays a bound for any table that passes
        // `from_rules` (which refuses a negative step cost). With the
        // committed rows (10 and 14) it is the octile distance exactly.
        let cardinal = self.cardinal.unsigned_abs();
        let diagonal = self.diagonal.unsigned_abs();
        let per_diagonal = u128::from(diagonal).min(u128::from(cardinal).wrapping_mul(2));
        let per_straight = u128::from(cardinal.min(diagonal));
        let cost = per_diagonal
            .wrapping_mul(low)
            .wrapping_add(per_straight.wrapping_mul(high.wrapping_sub(low)));
        // The sim's walker arrives no sooner than tick
        // `floor(cost * TICK_HZ / speed)` (the module doc, "Why down"), and
        // the division is by a speed that cannot be zero, named rather than
        // written with `/`.
        let ticks = core::ops::Div::div(
            cost.wrapping_mul(u128::from(TICK_HZ)),
            NonZeroU128::from(self.cost_per_second),
        );
        // A tick's milliseconds, the sim's own conversion.
        let per_tick = Ms::from_ticks(1).raw().unsigned_abs();
        WideMs::of_millis(ticks.wrapping_mul(u128::from(per_tick)))
    }
}

#[cfg(test)]
mod tests {
    use super::Walk;
    use crate::interface::WideMs;
    use crate::limits::RulesGap;
    use pharmakos_proto::gp::v1::Voxel;
    use pharmakos_sim::math::quantity::Ms;
    use pharmakos_sim::rules::RulesTable;

    fn rules() -> RulesTable {
        RulesTable::load(std::path::Path::new("../../rules/rules.v1.json"))
            .expect("the committed rules table")
    }

    fn walk() -> Walk {
        Walk::from_rules(&rules()).expect("the committed table walks")
    }

    fn voxel(x: i32, y: i32) -> Voxel {
        Voxel { x, y, z: 50 }
    }

    /// Ten cost units a cardinal voxel at ten a second: a voxel a second.
    #[test]
    fn a_straight_leg_is_a_second_a_voxel_at_the_committed_rows() {
        let walk = walk();
        assert_eq!(
            walk.leg(voxel(0, 0), 0, voxel(16, 0), 0),
            WideMs::of(Ms::new(16_000))
        );
        assert_eq!(
            walk.leg(voxel(0, 0), 0, voxel(0, -16), 0),
            WideMs::of(Ms::new(16_000))
        );
    }

    /// Octile: fourteen a diagonal, ten a straight voxel.
    #[test]
    fn a_diagonal_leg_is_the_octile_distance() {
        // 10 diagonals and 5 straight: 140 + 50 = 190 cost units, 19 s.
        assert_eq!(
            walk().leg(voxel(0, 0), 0, voxel(15, 10), 0),
            WideMs::of(Ms::new(19_000))
        );
    }

    /// The radius at each end comes off each axis, never below zero.
    #[test]
    fn the_radii_at_both_ends_come_off_the_leg() {
        let walk = walk();
        assert_eq!(walk.arrive_radius(), 2);
        assert_eq!(walk.interface_range(), 4);
        // 16 voxels, less 2 of slack and 4 of range: 10 s.
        assert_eq!(
            walk.leg(voxel(0, 0), 2, voxel(16, 0), 4),
            WideMs::of(Ms::new(10_000))
        );
        // Already within range: no walking at all.
        assert_eq!(
            walk.leg(voxel(0, 0), 0, voxel(3, 3), 4),
            WideMs::of(Ms::new(0))
        );
    }

    /// Height is never charged: a climb only adds, so leaving it out keeps the
    /// figure a bound.
    #[test]
    fn height_is_never_charged() {
        let low = Voxel { x: 0, y: 0, z: 0 };
        let high = Voxel { x: 8, y: 0, z: 60 };
        assert_eq!(walk().leg(low, 0, high, 0), WideMs::of(Ms::new(8_000)));
    }

    /// A speed that does not divide a step's price rounds down: the sim's
    /// walker may carry part of a tick in from the leg before, so the bound
    /// must not round up past it.
    #[test]
    fn a_leg_rounds_down_when_the_speed_does_not_divide_it() {
        let mut message = rules().message().clone();
        if let Some(commander) = message.commander.as_mut() {
            commander.cost_per_second = 3;
        }
        let table = RulesTable::from_message(&message).expect("the sim's view builds");
        let walk = Walk::from_rules(&table).expect("a positive speed walks");
        // One cardinal voxel: 10 x 20 / 3 = 66.7 ticks, so 66 ticks, 3.3 s.
        assert_eq!(
            walk.leg(voxel(0, 0), 0, voxel(1, 0), 0),
            WideMs::of(Ms::new(3_300))
        );
    }

    /// Exact, not saturated (fix pass 4, item 1): the far corner is
    /// 2^32 - 1 diagonal steps away, at the diagonal's cost, in ticks at the
    /// committed speed, in milliseconds; past `i32::MAX`, as it is.
    #[test]
    fn a_leg_off_any_map_is_exact_rather_than_saturated() {
        let walk = walk();
        let far = walk.leg(voxel(i32::MIN, i32::MIN), 0, voxel(i32::MAX, i32::MAX), 0);
        let one = walk.leg(voxel(0, 0), 0, voxel(1, 1), 0);
        assert!(far > WideMs::of(Ms::new(i32::MAX)), "{far:?}");
        // Ticks floor per leg, so the far leg is at least the one-step leg
        // times the steps, and less than a step's tick more each.
        let steps = i128::from(u32::MAX);
        assert!(far.raw() >= one.raw() * steps, "{far:?} against {one:?}");
    }

    #[test]
    fn a_speed_of_zero_is_refused() {
        let mut message = rules().message().clone();
        if let Some(commander) = message.commander.as_mut() {
            commander.cost_per_second = 0;
        }
        let table = RulesTable::from_message(&message);
        // The sim's own view may refuse the row first; if it does not, the
        // verifier must.
        if let Ok(table) = table {
            assert_eq!(
                Walk::from_rules(&table),
                Err(RulesGap::NotPositive {
                    field: "commander.cost_per_second",
                    value: 0
                })
            );
        }
    }
}
