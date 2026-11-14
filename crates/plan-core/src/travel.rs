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
use pharmakos_sim::math::quantity::{MS_PER_TICK, Ms};

/// Why a travel time has no rendering: each is a figure no estimate holds, so
/// it is refused rather than clamped into one that reads as an answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TravelError {
    /// A leg's ticks, as game milliseconds, past what [`Ms`] holds.
    LegPastRange {
        /// The leg's ticks.
        ticks: u32,
    },
    /// The route's legs, summed, past what [`Ms`] holds.
    TotalPastRange,
    /// A travel time below zero, which no estimate is.
    Negative(Ms),
}

impl core::fmt::Display for TravelError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TravelError::LegPastRange { ticks } => write!(
                formatter,
                "a leg of {ticks} ticks is past the game milliseconds a travel time holds"
            ),
            TravelError::TotalPastRange => write!(
                formatter,
                "the route's legs add up past the game milliseconds a travel time holds"
            ),
            TravelError::Negative(ms) => {
                write!(formatter, "a travel time of {} ms is below zero", ms.raw())
            }
        }
    }
}

impl std::error::Error for TravelError {}

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
    ///
    /// # Errors
    ///
    /// [`TravelError::LegPastRange`] for ticks whose milliseconds pass what
    /// [`Ms`] holds, rather than the saturated figure that would read as one.
    pub fn ms(&self) -> Result<Ms, TravelError> {
        let ms = i64::from(self.ticks)
            .checked_mul(i64::from(MS_PER_TICK))
            .and_then(|ms| i32::try_from(ms).ok())
            .ok_or(TravelError::LegPastRange { ticks: self.ticks })?;
        Ok(Ms::new(ms))
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
    ///
    /// # Errors
    ///
    /// As [`Estimate::ms`] and [`whole_seconds`].
    pub fn render(&self) -> Result<String, TravelError> {
        match self.estimate {
            None => Ok(crate::strings::NO_ROUTE.to_owned()),
            Some(estimate) => Ok(bounded(whole_seconds(estimate.ms()?)?, self.fogged)),
        }
    }
}

/// `time`, labelled "at most" when it is a bound (rule 1).
fn bounded(time: String, bound: bool) -> String {
    if bound {
        format!("{} {time}", crate::strings::AT_MOST)
    } else {
        time
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
    ///
    /// # Errors
    ///
    /// As [`Estimate::ms`], and [`TravelError::TotalPastRange`] for legs that
    /// add up past what [`Ms`] holds, rather than the saturated sum.
    pub fn total(&self) -> Result<Option<Ms>, TravelError> {
        let mut total: i32 = 0;
        for leg in &self.legs {
            let Some(estimate) = leg.estimate else {
                return Ok(None);
            };
            total = total
                .checked_add(estimate.ms()?.raw())
                .ok_or(TravelError::TotalPastRange)?;
        }
        Ok(Some(Ms::new(total)))
    }

    /// The whole route as a sentence, marked as a bound when any leg is
    /// fogged.
    ///
    /// # Errors
    ///
    /// As [`Route::total`] and [`whole_seconds`].
    pub fn render(&self) -> Result<String, TravelError> {
        match self.total()? {
            None => Ok(crate::strings::NO_ROUTE.to_owned()),
            Some(total) => Ok(bounded(whole_seconds(total)?, self.is_bounded())),
        }
    }
}

/// Rounds **up** to the next whole second and writes it as `9 s` or `2:00`.
///
/// Up, always: the estimator is never optimistic, and rounding up is the only
/// direction that keeps that true of what the player reads (item 57). The
/// arithmetic is in `i64`, where a rounded `i32` of milliseconds cannot
/// overflow, so every travel time has its rendering.
///
/// # Errors
///
/// [`TravelError::Negative`] for a time below zero, which no estimate is.
pub fn whole_seconds(ms: Ms) -> Result<String, TravelError> {
    let raw = i64::from(ms.raw());
    if raw < 0 {
        return Err(TravelError::Negative(ms));
    }
    // `raw` is at most `i32::MAX`, so the sum is far inside `i64`.
    let seconds = (raw + 999).div_euclid(1000);
    if seconds < 60 {
        return Ok(format!("{seconds} s"));
    }
    let minutes = seconds.div_euclid(60);
    let rest = seconds.rem_euclid(60);
    Ok(format!("{minutes}:{rest:02}"))
}

#[cfg(test)]
mod tests {
    use super::{Estimate, Route, TravelError, TravelEstimator, whole_seconds};
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

    fn seconds(ms: i32) -> String {
        whole_seconds(Ms::new(ms)).expect("a travel time")
    }

    #[test]
    fn a_time_is_rounded_up_to_a_whole_second() {
        assert_eq!(seconds(0), "0 s");
        assert_eq!(seconds(1), "1 s");
        assert_eq!(seconds(1999), "2 s");
        assert_eq!(seconds(2000), "2 s");
        assert_eq!(seconds(59_001), "1:00");
        assert_eq!(seconds(120_000), "2:00");
        assert_eq!(seconds(125_400), "2:06");
        // The longest time `Ms` holds still has its rendering, rounded up.
        assert_eq!(seconds(i32::MAX), "35791:24");
    }

    /// E22 (S3's `gwr`): a figure no estimate holds is refused with its typed
    /// error, never clamped or saturated into one that reads as an answer.
    #[test]
    fn a_figure_no_estimate_holds_is_refused() {
        assert_eq!(
            whole_seconds(Ms::new(-1)),
            Err(TravelError::Negative(Ms::new(-1)))
        );
        let long = Estimate {
            cost: 1,
            ticks: u32::MAX,
            legs: 1,
        };
        assert_eq!(
            long.ms(),
            Err(TravelError::LegPastRange { ticks: u32::MAX })
        );
        let half = Estimate {
            cost: 1,
            ticks: u32::try_from(i32::MAX.div_euclid(50)).expect("a tick count"),
            legs: 1,
        };
        let route = Route {
            legs: vec![
                super::Leg {
                    to: voxel(1, 0, 1),
                    estimate: Some(half),
                    fogged: false,
                },
                super::Leg {
                    to: voxel(2, 0, 1),
                    estimate: Some(half),
                    fogged: false,
                },
            ],
        };
        assert_eq!(route.total(), Err(TravelError::TotalPastRange));
        assert_eq!(route.render(), Err(TravelError::TotalPastRange));
    }

    #[test]
    fn a_clear_leg_reads_as_a_time() {
        let route = Route::estimate(&[voxel(0, 0, 1), voxel(40, 0, 1)], &Double);
        assert!(!route.is_bounded());
        assert_eq!(route.render().as_deref(), Ok("2 s"));
    }

    #[test]
    fn a_fogged_leg_reads_as_a_bound_and_never_as_an_eta() {
        let route = Route::estimate(&[voxel(0, 0, 1), voxel(-40, 0, 1)], &Double);
        assert!(route.is_bounded());
        assert_eq!(route.render().as_deref(), Ok("at most 2 s"));
        assert_eq!(
            route.legs.first().map(super::Leg::render),
            Some(Ok("at most 2 s".to_owned()))
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
        assert!(route.render().is_ok_and(|text| text.starts_with("at most")));
    }

    #[test]
    fn no_route_is_a_normal_answer_and_stops_the_total() {
        let route = Route::estimate(&[voxel(5, 5, 5), voxel(0, 0, 0)], &Double);
        assert!(route.is_unreachable());
        assert_eq!(route.total(), Ok(None));
        assert_eq!(route.render().as_deref(), Ok("no route the seat knows of"));
    }

    #[test]
    fn one_waypoint_is_no_legs() {
        let route = Route::estimate(&[voxel(1, 1, 1)], &Double);
        assert!(route.legs.is_empty());
        assert_eq!(route.total(), Ok(Some(Ms::new(0))));
    }
}
