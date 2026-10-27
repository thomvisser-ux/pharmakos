// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The power sweep: item 65's arithmetic with every input a measurement (S2's
//! plan, section 5 item 3, and task `g3r`; items 65 and 91).
//!
//! Item 65 derived the per-map power budget from the spike's tick model: the
//! 12.5 ms mean budget less `power.reserve_percent`, less the tick's fixed
//! cost and the crater stream's at 20 a second, over one unit's cost at that
//! rate, times `power.kw_per_unit`. Here each of those is measured on the
//! finished sim: the gate's segment is played at three shares of its units
//! ([`UNIT_SHARES`]) at 20 craters a second, and the tick's mean is fitted
//! against the mean of the live units by least squares, in integers. The
//! intercept is the fixed and crater cost together, the slope one unit's.
//! The segment is also played at the full share at the other crater rates
//! item 65 quotes ([`CRATER_RATES`]), so the destruction rate's weight is
//! recorded beside the budget it sets, as item 65 records it.
//!
//! **The ceiling.** `power.map_ceiling_kw` caps what the map supplies, and the
//! supply powers beacons as well as units, so the ceiling the fit gives is the
//! carried units' draw (`power.kw_per_unit` each) plus the gate's 40 beacons'
//! (`power.beacon_base_draw_kw` each) and each seat's
//! `power.revive_margin_kw`. Structures' draw is not in it: the gate's Build
//! beacons build nothing (item 149 (4)), so no structure's cost is in the
//! measured tick either.
//!
//! **What this does not measure.** The fit is the mean's (G3′-b); the p99
//! (G3′-a) does not reduce to a unit count, as item 65 found, and is the
//! segment's own figure. A unit's cost is fitted at 20 craters a second only:
//! the other rates are measured at the full share and are **not budgets**,
//! only the destruction rate's weight on the tick. A load the fit carries past
//! the largest one measured is an extrapolation, and the notice says so:
//! items 63 and 67 found the broadphase super-linear in density, so it is
//! re-measured at that load before it is used.

use std::path::Path;

use super::layout::Quarters;
use super::segment::{self, SegmentPlan, SegmentReport};
use super::{G3_SEGMENT_MS, MEAN_BAR_NS};
use crate::error::Error;
use crate::heap::HeapCounter;

/// The shares of the gate's units the fit is taken over, in quarters: five
/// points, so the line is tested by three degrees of freedom.
pub const UNIT_SHARES: [u8; 5] = [0, 1, 2, 3, 4];

/// The crater rates the full share is also played at, a second (item 65's
/// 0, 10 and 40 beside the budget's 20).
pub const CRATER_RATES: [u32; 3] = [0, 10, 40];

/// The rate the budget is set with (item 65).
pub const BUDGET_CRATERS_PER_SECOND: u32 = 20;

/// One played point.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Point {
    /// The share of the gate's units laid out.
    pub share: Quarters,
    /// The crater rate, a second.
    pub craters_per_second: u32,
    /// The mean of the live units per tick, in thousandths.
    pub live_units_milli: u64,
    /// The tick's mean, in nanoseconds.
    pub mean_ns: u128,
    /// The tick's p99, in nanoseconds.
    pub p99_ns: u128,
    /// The Pathing phase's mean, in nanoseconds.
    pub pathing_mean_ns: u128,
    /// The point's bench variant's `rules_hash` (the surplus is raised to
    /// the point's own load).
    pub bench_rules_hash: u64,
    /// The point's layout's fingerprint.
    pub layout_digest: u64,
    /// Ticks played.
    pub ticks: u64,
}

/// The fit and the budget it gives.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Sweep {
    /// The points at the budget's crater rate, by share.
    pub units: Vec<Point>,
    /// The full share at the other crater rates.
    pub craters: Vec<Point>,
    /// The fit's intercept: the tick's fixed and crater cost, in nanoseconds.
    pub intercept_ns: i128,
    /// The fit's slope: one live unit's cost, in picoseconds.
    pub unit_ps: i128,
    /// The largest distance of a point from the fit, in nanoseconds.
    pub worst_residual_ns: i128,
    /// `power.reserve_percent`, as committed.
    pub reserve_percent: u32,
    /// `power.kw_per_unit`, as committed.
    pub kw_per_unit: u32,
    /// `power.map_ceiling_kw`, as committed.
    pub shipped_ceiling_kw: u32,
    /// The mean the reserve leaves, in nanoseconds.
    pub budget_ns: i128,
    /// The live units the fit says the budget carries.
    pub units_carried: u64,
    /// The largest mean of live units a point measured, in thousandths.
    pub largest_measured_milli: u64,
    /// `power.beacon_base_draw_kw`, as committed.
    pub beacon_draw_kw: u32,
    /// `power.revive_margin_kw`, as committed.
    pub revive_margin_kw: u32,
    /// The ceiling the fit gives: the carried units', the gate's beacons' and
    /// the seats' margins' draw.
    pub ceiling_kw: u64,
    /// The committed table's `rules_hash`.
    pub shipped_rules_hash: u64,
}

impl Sweep {
    /// Whether the carried load lies past the largest one measured.
    #[must_use]
    pub fn extrapolated(&self) -> bool {
        u128::from(self.units_carried) * 1_000 > u128::from(self.largest_measured_milli)
    }
}

/// One point's segment.
fn play(
    root: &Path,
    seed: u64,
    share: Quarters,
    craters_per_second: u32,
    heap: Option<&dyn HeapCounter>,
) -> Result<Point, Error> {
    let report: SegmentReport = segment::measure(
        root,
        SegmentPlan {
            seed,
            segment_ms: G3_SEGMENT_MS,
            craters_per_second,
            probe_sight: false,
            share,
        },
        heap,
    )?;
    let pathing_mean_ns = report
        .phases
        .iter()
        .find(|figure| figure.phase == pharmakos_sim::world::Phase::Pathing)
        .map(|figure| figure.summary.mean_ns)
        .ok_or_else(|| Error::Drift("no Pathing phase was timed".to_owned()))?;
    Ok(Point {
        share,
        craters_per_second,
        live_units_milli: report.live_units_mean_milli,
        mean_ns: report.tick.mean_ns,
        p99_ns: report.tick.p99_ns,
        pathing_mean_ns,
        bench_rules_hash: report.provenance.bench_rules_hash,
        layout_digest: report.provenance.layout_digest,
        ticks: report.ticks,
    })
}

/// The least-squares line through `points` of (live units in thousandths,
/// tick mean in ns): the intercept in ns, the slope in ps a unit, and the
/// worst residual in ns. Integer arithmetic throughout.
///
/// # Errors
///
/// [`Error::Empty`] for fewer than two points or points that all hold the
/// same load, which have no slope; [`Error::Overflow`] past `i128`.
pub fn fit(points: &[(u64, u128)]) -> Result<(i128, i128, i128), Error> {
    let overflow = || Error::Overflow("the power sweep's fit");
    let n = i128::try_from(points.len()).map_err(|_| overflow())?;
    if n < 2 {
        return Err(Error::Empty("a fit needs two points".to_owned()));
    }
    let xs: Vec<i128> = points.iter().map(|(x, _)| i128::from(*x)).collect();
    let ys: Vec<i128> = points
        .iter()
        .map(|(_, y)| i128::try_from(*y).map_err(|_| overflow()))
        .collect::<Result<_, _>>()?;
    let sum = |values: &[i128]| -> Result<i128, Error> {
        values
            .iter()
            .try_fold(0_i128, |sum, value| sum.checked_add(*value))
            .ok_or_else(overflow)
    };
    // Scaled by n so the means stay whole: X = n x - sum(x).
    let (sx, sy) = (sum(&xs)?, sum(&ys)?);
    let mut sxy: i128 = 0;
    let mut sxx: i128 = 0;
    for (x, y) in xs.iter().zip(&ys) {
        let dx = x
            .checked_mul(n)
            .and_then(|v| v.checked_sub(sx))
            .ok_or_else(overflow)?;
        let dy = y
            .checked_mul(n)
            .and_then(|v| v.checked_sub(sy))
            .ok_or_else(overflow)?;
        sxy = dx
            .checked_mul(dy)
            .and_then(|v| sxy.checked_add(v))
            .ok_or_else(overflow)?;
        sxx = dx
            .checked_mul(dx)
            .and_then(|v| sxx.checked_add(v))
            .ok_or_else(overflow)?;
    }
    if sxx == 0 {
        return Err(Error::Empty(
            "every point of the sweep held the same load, which has no slope".to_owned(),
        ));
    }
    // y is ns and x thousandths of a unit, so sxy / sxx is ns per milli-unit,
    // a microsecond a unit; times a million, picoseconds a unit.
    let unit_ps = sxy
        .checked_mul(1_000_000)
        .and_then(|v| v.checked_div(sxx))
        .ok_or_else(overflow)?;
    // intercept = mean(y) - slope * mean(x), with slope in ns per milli-unit.
    let intercept_ns = sxx
        .checked_mul(sy)
        .and_then(|v| sxy.checked_mul(sx).and_then(|w| v.checked_sub(w)))
        .and_then(|v| sxx.checked_mul(n).and_then(|d| v.checked_div(d)))
        .ok_or_else(overflow)?;
    let mut worst: i128 = 0;
    for (x, y) in xs.iter().zip(&ys) {
        let predicted = unit_ps
            .checked_mul(*x)
            .and_then(|v| v.checked_div(1_000_000))
            .and_then(|v| v.checked_add(intercept_ns))
            .ok_or_else(overflow)?;
        let residual = y.checked_sub(predicted).ok_or_else(overflow)?.abs();
        worst = worst.max(residual);
    }
    Ok((intercept_ns, unit_ps, worst))
}

/// Play the sweep and derive the budget.
///
/// # Errors
///
/// As [`segment::measure`] for any point; [`Error::Rules`] when the committed
/// table has no `power` block; as [`fit`].
pub fn measure(root: &Path, seed: u64, heap: Option<&dyn HeapCounter>) -> Result<Sweep, Error> {
    let overflow = || Error::Overflow("the power budget");
    let mut units: Vec<Point> = Vec::with_capacity(UNIT_SHARES.len());
    for quarters in UNIT_SHARES {
        let share = Quarters::new(quarters)
            .ok_or_else(|| Error::Drift(format!("{quarters} quarters is past the full share")))?;
        units.push(play(root, seed, share, BUDGET_CRATERS_PER_SECOND, heap)?);
    }
    let mut craters: Vec<Point> = Vec::with_capacity(CRATER_RATES.len());
    for rate in CRATER_RATES {
        craters.push(play(root, seed, Quarters::FULL, rate, heap)?);
    }
    let samples: Vec<(u64, u128)> = units
        .iter()
        .map(|point| (point.live_units_milli, point.mean_ns))
        .collect();
    let (intercept_ns, unit_ps, worst_residual_ns) = fit(&samples)?;
    let shipped = crate::quick::committed_rules(root)?;
    let power = shipped
        .message()
        .power
        .as_ref()
        .ok_or_else(|| Error::Rules("no `power` block".to_owned()))?;
    let (reserve_percent, kw_per_unit, shipped_ceiling_kw, beacon_draw_kw, revive_margin_kw) = (
        power.reserve_percent,
        power.kw_per_unit,
        power.map_ceiling_kw,
        power.beacon_base_draw_kw,
        power.revive_margin_kw,
    );
    if reserve_percent >= 100 {
        return Err(Error::Rules(format!(
            "power.reserve_percent {reserve_percent} leaves no budget"
        )));
    }
    if unit_ps <= 0 {
        return Err(Error::Drift(format!(
            "the sweep fits a unit's cost of {unit_ps} ps, which carries no budget: more units \
             did not cost more"
        )));
    }
    let bar = i128::try_from(MEAN_BAR_NS).map_err(|_| overflow())?;
    let budget_ns = bar
        .checked_mul(i128::from(
            100_u32.checked_sub(reserve_percent).ok_or_else(overflow)?,
        ))
        .and_then(|v| v.checked_div(100))
        .ok_or_else(overflow)?;
    if budget_ns <= intercept_ns {
        return Err(Error::Drift(format!(
            "the reserve leaves a mean of {budget_ns} ns, at or below the fit's fixed and crater \
             cost of {intercept_ns} ns: no unit fits, so the per-tick lever is the fallback's \
             (S2's plan, section 5)"
        )));
    }
    // (budget - intercept) ns over unit_ps ps a unit: scale ns to ps first.
    let units_carried = budget_ns
        .checked_sub(intercept_ns)
        .and_then(|v| v.checked_mul(1_000))
        .and_then(|v| v.checked_div(unit_ps))
        .and_then(|v| u64::try_from(v).ok())
        .ok_or_else(overflow)?;
    let largest_measured_milli = units
        .iter()
        .map(|point| point.live_units_milli)
        .max()
        .ok_or_else(|| Error::Empty("the sweep played no point".to_owned()))?;
    let seats = u64::from(super::layout::GATE_SEATS);
    let beacons = u64::from(super::layout::LAID_OUT_BEACONS)
        .checked_add(seats)
        .ok_or_else(overflow)?;
    let ceiling_kw = units_carried
        .checked_mul(u64::from(kw_per_unit))
        .and_then(|units| {
            beacons
                .checked_mul(u64::from(beacon_draw_kw))
                .and_then(|draw| units.checked_add(draw))
        })
        .and_then(|draw| {
            seats
                .checked_mul(u64::from(revive_margin_kw))
                .and_then(|margin| draw.checked_add(margin))
        })
        .ok_or_else(overflow)?;
    Ok(Sweep {
        units,
        craters,
        intercept_ns,
        unit_ps,
        worst_residual_ns,
        reserve_percent,
        kw_per_unit,
        shipped_ceiling_kw,
        budget_ns,
        units_carried,
        largest_measured_milli,
        beacon_draw_kw,
        revive_margin_kw,
        ceiling_kw,
        shipped_rules_hash: shipped.rules_hash(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_unit_shares_are_whole_quarters() {
        assert!(UNIT_SHARES.iter().all(|q| Quarters::new(*q).is_some()));
    }

    #[test]
    fn the_fit_recovers_a_line_exactly() {
        // 4 ms fixed and 10 us a unit, at 0, 150 and 300 units.
        let points = [(0, 4_000_000), (150_000, 5_500_000), (300_000, 7_000_000)];
        assert_eq!(fit(&points).expect("a fit"), (4_000_000, 10_000_000, 0));
    }

    #[test]
    fn a_fit_without_a_slope_is_refused() {
        assert!(matches!(fit(&[(1, 1)]), Err(Error::Empty(_))));
        assert!(matches!(fit(&[(5, 1), (5, 2)]), Err(Error::Empty(_))));
    }
}
