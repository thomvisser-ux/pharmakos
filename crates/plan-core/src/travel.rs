// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The pass-through to the sim's travel estimator, and the two rendering rules
//! the editor is held to.
//!
//! # The seam
//!
//! Decisions-log item 61 froze the estimator's signature:
//!
//! > `estimate(world, clusters, scratch, start, goal, fog) -> Option<Estimate {
//! > cost, ticks, legs }>`; `None` means no route and is answered by the
//! > connectivity oracle before any search … The estimator is the abstract
//! > search alone: it never refines, never steps, never forks.
//!
//! `world`, `clusters`, `scratch` and `fog` are the estimator's own state, so
//! what crosses into this crate is [`TravelEstimator`]: the frozen function
//! with its first four arguments bound, plus the one question the *seat's fog*
//! answers and a playbook cannot — whether a leg crosses ground the seat has
//! not seen. `gp.api.v1.Leg` carries exactly that `fogged` flag, so the shape
//! is the gateway's, not an invention here.
//!
//! **The implementation is not in this crate and must not be.** T7 builds the
//! estimator in `pharmakos-sim`; the one-line impl over its function is added
//! when T7 is on `main`. plan-core names no sim symbol that does not exist
//! today, and `tests/confinement.rs` asserts that this crate reaches no
//! stepping API at all.
//!
//! # The two rendering rules
//!
//! Items 57 and 61 are a promise to the player, and they are rules about
//! *rendering*, which is why they live here rather than in the estimator:
//!
//! 1. **A fogged leg is drawn as the upper bound it is, never as an ETA.** Fog
//!    is priced `cost * 3 / 2` per abstract edge, measured at 48.6 % p90 error
//!    and capped at +50 % over the clear estimate. So the leg is labelled "at
//!    most", the route total says how much of it is a bound, and the editor
//!    draws it dashed (spec section 13).
//! 2. **A time shown to the player is rounded generously, in whole seconds.**
//!    The estimator is *never optimistic* on static terrain — that is part of
//!    item 61's contract rather than an implementation detail — and the
//!    rounding depends on it: rounding **up** to the next whole second can only
//!    ever make the promise safer. The short-route band (≤ 32 cells) has p90
//!    9.2 % and max 66.6 %, "one second on a two-second route", which is
//!    exactly the error a whole second absorbs.
//!
//! Recorded honestly, because item 57 records it: the promise is made on static
//! terrain and a Push is not static. Under 20 craters/s the same estimate is
//! 12.5 % p90 against actual arrival, and the player sees the discrepancy at
//! arrival rather than as a caveat at plan time. Nothing here claims otherwise.

use pharmakos_proto::gp::v1::Voxel;
use pharmakos_sim::math::quantity::Ms;

/// What the estimator answers, in item 61's own three fields.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Estimate {
    /// Path cost in the locomotion table's units.
    pub cost: i32,
    /// Travel in sim ticks. Ceiling, never floor — the estimator is never
    /// optimistic.
    pub ticks: u32,
    /// How many abstract edges the route crosses. The editor draws the
    /// polyline from these.
    pub legs: u32,
}

impl Estimate {
    /// The estimate as game milliseconds, which is what every playbook
    /// duration is in and what `gp.api.v1.Leg.ms` carries.
    #[must_use]
    pub fn ms(&self) -> Ms {
        Ms::from_ticks(self.ticks)
    }
}

/// The sim's estimator, as this crate sees it.
///
/// One implementation is the real one, over T7's `estimate`. The other is a
/// test double, which is how this crate's tests and goldens run before T7 is
/// on `main`.
pub trait TravelEstimator {
    /// Item 61's frozen function, with the world, clusters, scratch and fog
    /// bound by the implementor.
    ///
    /// `None` is the connectivity oracle's answer — no route — and it is a
    /// normal, cheap result: a moat seals you in as surely as a wall.
    fn estimate(&self, start: Voxel, goal: Voxel) -> Option<Estimate>;

    /// Whether this leg crosses ground the seat has not seen.
    ///
    /// The seat's fog is the estimator's input, not the playbook's, so only
    /// the implementor can answer. `gp.api.v1.Leg.fogged` is this flag.
    fn is_fogged(&self, start: Voxel, goal: Voxel) -> bool;
}

/// One leg of a route, rendered under the two rules.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Leg {
    /// Where the leg ends.
    pub to: Voxel,
    /// The estimate, or `None` when there is no route.
    pub estimate: Option<Estimate>,
    /// True when the leg crosses ground the seat has not seen, so the number
    /// is a bound rather than an estimate.
    pub fogged: bool,
}

impl Leg {
    /// The leg as a sentence, under both rules.
    #[must_use]
    pub fn render(&self) -> String {
        match self.estimate {
            None => crate::strings::NO_ROUTE.to_owned(),
            Some(estimate) if self.fogged => {
                format!(
                    "{} {}",
                    crate::strings::AT_MOST,
                    whole_seconds(estimate.ms())
                )
            }
            Some(estimate) => whole_seconds(estimate.ms()),
        }
    }
}

/// A whole route, leg by leg.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Route {
    /// The legs, in order.
    pub legs: Vec<Leg>,
}

impl Route {
    /// Estimates a route of two or more waypoints.
    ///
    /// Each consecutive pair is one leg, which is the shape
    /// `gp.api.v1.EstimateRouteResponse` has.
    #[must_use]
    pub fn estimate(waypoints: &[Voxel], estimator: &impl TravelEstimator) -> Self {
        let mut legs = Vec::new();
        for pair in waypoints.windows(2) {
            let (Some(start), Some(goal)) = (pair.first(), pair.get(1)) else {
                continue;
            };
            legs.push(Leg {
                to: *goal,
                estimate: estimator.estimate(*start, *goal),
                fogged: estimator.is_fogged(*start, *goal),
            });
        }
        Self { legs }
    }

    /// True when any leg has no route at all. The whole-route total is then
    /// not a number the editor may show.
    #[must_use]
    pub fn is_unreachable(&self) -> bool {
        self.legs.iter().any(|leg| leg.estimate.is_none())
    }

    /// True when any leg crosses fog, so the total is a bound.
    #[must_use]
    pub fn is_bounded(&self) -> bool {
        self.legs.iter().any(|leg| leg.fogged)
    }

    /// The whole route's travel, or `None` when a leg has no route.
    ///
    /// Clear legs contribute their estimate and fogged legs their bound, which
    /// is item 61's rule for the total: "the clear legs' time plus the fogged
    /// bound, marked as a bound".
    #[must_use]
    pub fn total(&self) -> Option<Ms> {
        let mut total: i32 = 0;
        for leg in &self.legs {
            let estimate = leg.estimate?;
            total = total.saturating_add(estimate.ms().raw());
        }
        Some(Ms::new(total))
    }

    /// The whole route as a sentence, marked as a bound when any leg is
    /// fogged.
    #[must_use]
    pub fn render(&self) -> String {
        match self.total() {
            None => crate::strings::NO_ROUTE.to_owned(),
            Some(total) if self.is_bounded() => {
                format!("{} {}", crate::strings::AT_MOST, whole_seconds(total))
            }
            Some(total) => whole_seconds(total),
        }
    }
}

/// The largest whole second a `Ms` can hold, used when rounding up would
/// overflow. A route this long is not a route, but the rendering must not
/// panic on one.
const MAX_SECONDS: i32 = i32::MAX.wrapping_div(1000);

/// Rounds **up** to the next whole second and writes it as `9 s` or `2:00`.
///
/// Up, always: the estimator is never optimistic, and rounding up is the only
/// direction that keeps that true of what the player reads (item 57).
#[must_use]
pub fn whole_seconds(ms: Ms) -> String {
    let raw = ms.raw().max(0);
    let seconds = raw
        .checked_add(999)
        .and_then(|rounded| rounded.checked_div(1000))
        .unwrap_or(MAX_SECONDS);
    if seconds < 60 {
        return format!("{seconds} s");
    }
    let minutes = seconds.checked_div(60).unwrap_or(0);
    let rest = seconds.saturating_sub(minutes.saturating_mul(60));
    format!("{minutes}:{rest:02}")
}

#[cfg(test)]
mod tests {
    use super::{Estimate, Route, TravelEstimator, whole_seconds};
    use pharmakos_proto::gp::v1::Voxel;
    use pharmakos_sim::math::quantity::Ms;

    /// A test double for the estimator T7 builds: cost and ticks from the
    /// Manhattan distance, fog wherever x is negative, no route to the origin.
    struct Double;

    impl TravelEstimator for Double {
        fn estimate(&self, start: Voxel, goal: Voxel) -> Option<Estimate> {
            if goal.x == 0 && goal.y == 0 && goal.z == 0 {
                return None;
            }
            let steps = goal
                .x
                .saturating_sub(start.x)
                .saturating_abs()
                .saturating_add(goal.y.saturating_sub(start.y).saturating_abs())
                .saturating_add(goal.z.saturating_sub(start.z).saturating_abs());
            let ticks = u32::try_from(steps).unwrap_or(0);
            Some(Estimate {
                cost: steps.saturating_mul(10),
                ticks,
                legs: 1,
            })
        }

        fn is_fogged(&self, _start: Voxel, goal: Voxel) -> bool {
            goal.x < 0
        }
    }

    fn voxel(x: i32, y: i32, z: i32) -> Voxel {
        Voxel { x, y, z }
    }

    #[test]
    fn a_time_is_rounded_up_to_a_whole_second() {
        assert_eq!(whole_seconds(Ms::new(0)), "0 s");
        assert_eq!(whole_seconds(Ms::new(1)), "1 s");
        assert_eq!(whole_seconds(Ms::new(1999)), "2 s");
        assert_eq!(whole_seconds(Ms::new(2000)), "2 s");
        assert_eq!(whole_seconds(Ms::new(59_001)), "1:00");
        assert_eq!(whole_seconds(Ms::new(120_000)), "2:00");
        assert_eq!(whole_seconds(Ms::new(125_400)), "2:06");
    }

    #[test]
    fn a_clear_leg_reads_as_a_time() {
        let route = Route::estimate(&[voxel(0, 0, 1), voxel(40, 0, 1)], &Double);
        assert!(!route.is_bounded());
        assert_eq!(route.render(), "2 s");
    }

    #[test]
    fn a_fogged_leg_reads_as_a_bound_and_never_as_an_eta() {
        let route = Route::estimate(&[voxel(0, 0, 1), voxel(-40, 0, 1)], &Double);
        assert!(route.is_bounded());
        assert_eq!(route.render(), "at most 2 s");
        assert_eq!(
            route.legs.first().map(super::Leg::render),
            Some("at most 2 s".to_owned())
        );
    }

    #[test]
    fn a_route_with_a_fogged_leg_marks_the_whole_total_as_a_bound() {
        let route = Route::estimate(
            &[voxel(0, 0, 1), voxel(40, 0, 1), voxel(-40, 0, 1)],
            &Double,
        );
        assert_eq!(route.legs.len(), 2);
        assert!(route.is_bounded());
        assert!(route.render().starts_with("at most"));
    }

    #[test]
    fn no_route_is_a_normal_answer_and_stops_the_total() {
        let route = Route::estimate(&[voxel(5, 5, 5), voxel(0, 0, 0)], &Double);
        assert!(route.is_unreachable());
        assert_eq!(route.total(), None);
        assert_eq!(route.render(), "no route the seat knows of");
    }

    #[test]
    fn one_waypoint_is_no_legs() {
        let route = Route::estimate(&[voxel(1, 1, 1)], &Double);
        assert!(route.legs.is_empty());
        assert_eq!(route.total(), Some(Ms::new(0)));
    }
}
