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
//! step costs, divided by the commander's speed and rounded up to the tick the
//! way the sim's walker arrives. Every real step costs at least its base rate
//! (a climb only adds a non-negative surcharge), so the walk the sim takes is
//! never shorter than this figure. It is the same admissible heuristic the
//! sim's own search is guided by, and it reads no terrain, so it knows nothing
//! the snapshot would have to be stepped to learn.
//!
//! A lower bound is the honest number for the two codes that use it: `W0701`
//! says a route **cannot** fit the coming segment, which a lower bound proves
//! and an estimate would only suggest, and `I0001` says how much walking the
//! route is at least. Neither claims an arrival time.
//!
//! PLACEHOLDER: pathfinder-quality travel for the schedule check — the gateway
//! pricing each known leg with the sim's estimator and handing the figures in
//! with the seat's view, so `W0701` can warn on a route that probably does not
//! fit rather than only on one that cannot. That needs a `Scope` field the
//! gateway fills, which is the same shape as the segment clock and the
//! "fits" pill (the register's S3-16); owner, at S3, with the segment clock.
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

use pharmakos_proto::gp::v1::Voxel;
use pharmakos_sim::math::quantity::{Ms, TICK_HZ};
use pharmakos_sim::rules::RulesTable;

use crate::limits::RulesGap;

/// The rows the walking bound reads, out of the rules table once.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Walk {
    cardinal: i64,
    diagonal: i64,
    cost_per_second: i64,
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
        let cost_per_second = i64::from(rules.commander_cost_per_second());
        if cost_per_second <= 0 {
            return Err(RulesGap::NotPositive {
                field: "commander.cost_per_second",
                value: cost_per_second,
            });
        }
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
    /// Saturating upwards only, past any segment: a figure that large is a
    /// leg off the map, which the placement checks name, and here it still
    /// reads as "longer than the segment".
    #[must_use]
    pub fn leg(&self, from: Voxel, slack: i64, to: Voxel, radius: i64) -> Ms {
        let reach = slack.saturating_add(radius).max(0);
        let axis = |a: i32, b: i32| -> i64 {
            i64::from(b)
                .saturating_sub(i64::from(a))
                .saturating_abs()
                .saturating_sub(reach)
                .max(0)
        };
        let (dx, dy) = (axis(from.x, to.x), axis(from.y, to.y));
        let (low, high) = if dx < dy { (dx, dy) } else { (dy, dx) };
        // Whatever the rows say, a diagonal step is never charged more than
        // two cardinal ones, nor a straight voxel of progress less than the
        // cheaper step, so the sum stays a bound for any table that passes
        // `from_rules`. With the committed rows (10 and 14) it is the octile
        // distance exactly.
        let per_diagonal = self.diagonal.min(self.cardinal.saturating_mul(2));
        let per_straight = self.cardinal.min(self.diagonal);
        let cost = per_diagonal
            .saturating_mul(low)
            .saturating_add(per_straight.saturating_mul(high.saturating_sub(low)));
        // The sim's walker adds its speed to an accumulator each tick and
        // arrives on tick `ceil(cost * TICK_HZ / speed)`, so the bound rounds
        // the same way.
        let ticks = cost
            .saturating_mul(i64::from(TICK_HZ))
            .saturating_add(self.cost_per_second.saturating_sub(1))
            .checked_div(self.cost_per_second)
            .unwrap_or(i64::MAX);
        Ms::from_ticks(u32::try_from(ticks).unwrap_or(u32::MAX))
    }
}

#[cfg(test)]
mod tests {
    use super::Walk;
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
        assert_eq!(walk.leg(voxel(0, 0), 0, voxel(16, 0), 0), Ms::new(16_000));
        assert_eq!(walk.leg(voxel(0, 0), 0, voxel(0, -16), 0), Ms::new(16_000));
    }

    /// Octile: fourteen a diagonal, ten a straight voxel.
    #[test]
    fn a_diagonal_leg_is_the_octile_distance() {
        // 10 diagonals and 5 straight: 140 + 50 = 190 cost units, 19 s.
        assert_eq!(
            walk().leg(voxel(0, 0), 0, voxel(15, 10), 0),
            Ms::new(19_000)
        );
    }

    /// The radius at each end comes off each axis, never below zero.
    #[test]
    fn the_radii_at_both_ends_come_off_the_leg() {
        let walk = walk();
        assert_eq!(walk.arrive_radius(), 2);
        assert_eq!(walk.interface_range(), 4);
        // 16 voxels, less 2 of slack and 4 of range: 10 s.
        assert_eq!(walk.leg(voxel(0, 0), 2, voxel(16, 0), 4), Ms::new(10_000));
        // Already within range: no walking at all.
        assert_eq!(walk.leg(voxel(0, 0), 0, voxel(3, 3), 4), Ms::new(0));
    }

    /// Height is never charged: a climb only adds, so leaving it out keeps the
    /// figure a bound.
    #[test]
    fn height_is_never_charged() {
        let low = Voxel { x: 0, y: 0, z: 0 };
        let high = Voxel { x: 8, y: 0, z: 60 };
        assert_eq!(walk().leg(low, 0, high, 0), Ms::new(8_000));
    }

    #[test]
    fn a_leg_off_any_map_saturates_rather_than_wraps() {
        let far = walk().leg(voxel(i32::MIN, i32::MIN), 0, voxel(i32::MAX, i32::MAX), 0);
        assert_eq!(far, Ms::new(i32::MAX));
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
