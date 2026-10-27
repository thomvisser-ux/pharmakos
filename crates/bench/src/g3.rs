// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! G3′-real's harness, S2's depth task (S2's plan, section 5 and task `g3h`).
//!
//! Spec section 16's G3′ row: "4 seats (headroom over v1's 3), 300 units, 40
//! beacons: tick p99 ≤50% of the tick; ≥4× speed on the headless sim
//! runner". S2's plan section 5 makes it G3′-a (the tick's p99 at most 25 ms)
//! and G3′-b (a mean at most 12.5 ms, at least 80 ticks a second), at 300
//! units and 40 beacons as world totals (item 63) with decision 2's three
//! seats, over a full 8-minute segment of real play. This module takes every
//! figure section 5 item 2 names; `g3r` certifies them on the owner's
//! machine, and here they are published, never gated (item 148's CI ruling).
//!
//! | Module | What |
//! |---|---|
//! | [`variant`] | The bench rules variant: the committed table with its power rows raised to the gate's load, its own `rules_hash` |
//! | [`layout`] | The gate's layout: 37 beacons and 100 units a seat, placed through `Host::seed_layout` |
//! | [`stream`] | The crater stream at item 65's 20 a second, filed through the host's seam |
//! | [`segment`] | The 8-minute segment, every tick timed phase by phase through the phase stepper |
//! | [`sight`] | `World::seen_by` restated and timed at the gate's load (item 148 (4)) |
//! | [`support`] | The support search's adversarial tick (item 138 (3)), and the footing query at its worst (item 148 (4)) |
//! | [`fourth`] | A fourth, unplaced seat's decision-tick cost (decision 2's separate line) |
//! | [`sweep`] | The power sweep: item 65's arithmetic with every input measured, for `tune2`'s power rows |
//!
//! # What the figures are, and what they are not yet
//!
//! The load is real play from the layout on, on the finished S2 sim (`dfnd`,
//! `atk` and `surv` merged; S2's plan, task `g3r`): Mine digs, Survey walks
//! its scouts, every core's autocannon fires at the enemy raiders in its
//! range, each Defend beacon's guards (an unset `roe` reads `FREE_FIRE`) go
//! after the enemy Attack raiders standing three voxels from it, craters land
//! at the budget's rate, regions collapse and units repath, all through the
//! sim's own phases. Two mandates do less than in a played match, because the
//! layout seam carries no mandate settings (`runner::LayoutBeacon`): an Attack
//! beacon has no target, so its laid-out raiders gather at it and never
//! launch, and a Build beacon has no blueprint, so nothing is built. The
//! repath cap bounds what a launched force's walk would add to Pathing, and
//! the footing query, which Build's structures lengthen, is also measured over
//! full tables on a world of the bench's own
//! ([`support::footing_at_room`]). Every notice names the seed, the variant's
//! `rules_hash` and the layout's digest, so a reader can reproduce the run.
//!
//! The notice text lives here, pure, so a unit test pins it word for word;
//! `tests/g3_real.rs` measures and prints.

pub mod fourth;
pub mod layout;
pub mod segment;
pub mod sight;
pub mod stream;
pub mod support;
pub mod sweep;
pub mod variant;

use pharmakos_sim::hex;

use crate::error::Error;
use crate::stats::{micros, millis, per_mille};
use fourth::FourthSeat;
use segment::SegmentReport;
use support::{FootingQuery, SupportReport};
use sweep::Sweep;

/// The prefix and format version of the one line `cargo xtask g3-budget`
/// reads (`xtask/src/g3.rs`, whose unit test holds the two spellings
/// together).
pub const BUDGET_REPORT_PREFIX: &str = "g3-budget-report v1";

/// The seed G3′ is measured and certified on: the seed every committed
/// scenario plays, so the map is one the project already pins. One seed,
/// played three times per operating system (S2's plan, section 5 item 4):
/// the repetitions measure the machine's noise, and the layout, not the map,
/// sets the load (`g3r`'s call, which the owner can veto).
pub const G3_SEED: u64 = 0x0000_0000_ca5c_aded;

/// The segment G3′ is measured over: the ladder's longest, 8 minutes.
pub const G3_SEGMENT_MS: i32 = 480_000;

/// G3′-a's bar, the tick's p99, in nanoseconds: half of a 50 ms tick.
pub const P99_BAR_NS: u128 = 25_000_000;

/// G3′-b's bar, the tick's mean, in nanoseconds: a quarter of a tick, 4x real
/// time.
pub const MEAN_BAR_NS: u128 = 12_500_000;

/// The ms figure of a duration, for a notice.
fn ms(ns: u128) -> String {
    millis(ns)
}

/// A per-mille figure as a decimal with one place: `125` is `12.5`.
fn percent(per_mille: u128) -> String {
    let whole = per_mille.div_euclid(10);
    let tenth = per_mille.rem_euclid(10);
    format!("{whole}.{tenth}%")
}

/// A thousandths figure with three places: `61500` is `61.500`.
fn thousandths(milli: u64) -> String {
    let whole = milli.div_euclid(1_000);
    let fraction = milli.rem_euclid(1_000);
    format!("{whole}.{fraction:03}")
}

/// The provenance notice: what reproduces the run.
#[must_use]
pub fn provenance_notice(report: &SegmentReport, os: &str) -> String {
    let p = &report.provenance;
    let mandates: Vec<String> = p
        .proportions
        .iter()
        .map(|(name, count)| format!("{name} {count}"))
        .collect();
    let overrides: Vec<String> = p
        .overrides
        .iter()
        .map(|row| format!("{} {} -> {}", row.row, row.shipped, row.bench))
        .collect();
    format!(
        "::notice title=G3' provenance::on {os}: seed 0x{:016x}, bench rules_hash {} (the \
         committed table's {} with {}), layout {} (3 seats x 100 laid-out units on top of each \
         seat's starting force, so a little over item 63's 300, and 40 beacons as world totals; \
         beacons by mandate: {}), every seat sealed at {} size units; craters {} filed at 20 a \
         second ({} refused as the queue full, {} draws redrawn); hash chain digest {}. On the \
         finished S2 sim: Defend's guards fight the enemy Attack raiders beside them and the \
         cores fire; the layout carries no mandate settings, so Attack has no target and never \
         launches and Build builds nothing",
        p.seed,
        hex(p.bench_rules_hash),
        hex(p.shipped_rules_hash),
        overrides.join(", "),
        hex(p.layout_digest),
        mandates.join(", "),
        p.units_per_seat,
        report.craters.filed,
        report.craters.queue_full,
        report.craters.redrawn,
        hex(report.chain_digest),
    )
}

/// The headline notice: G3′-a and G3′-b's figures.
///
/// # Errors
///
/// [`Error::Empty`] for a tick mean of zero, which has no speed.
pub fn tick_notice(report: &SegmentReport, os: &str) -> Result<String, Error> {
    let (per_second, real_time) = segment::speed_milli(report.tick.mean_ns)?;
    Ok(format!(
        "::notice title=G3' tick::tick p99 {}, p50 {}, mean {}, max {} on {os} over {} ticks of \
         one segment; {} ticks a second, {}x real time. G3'-a's bar is a p99 of at most {} and \
         G3'-b's a mean of at most {} (4x); published, never gated here, and certified by g3r \
         on the owner's machine",
        ms(report.tick.p99_ns),
        ms(report.tick.p50_ns),
        ms(report.tick.mean_ns),
        ms(report.tick.max_ns),
        report.ticks,
        thousandths(u64::try_from(per_second).map_err(|_| Error::Overflow("ticks a second"))?),
        thousandths(u64::try_from(real_time).map_err(|_| Error::Overflow("the speed"))?),
        ms(P99_BAR_NS),
        ms(MEAN_BAR_NS),
    ))
}

/// The per-phase notice.
///
/// # Errors
///
/// [`Error::Empty`] for a tick mean of zero.
pub fn phases_notice(report: &SegmentReport, os: &str) -> Result<String, Error> {
    let phases: Vec<String> = report
        .phases
        .iter()
        .map(|figure| {
            format!(
                "{:?} {} / {}",
                figure.phase,
                micros(figure.summary.mean_ns),
                micros(figure.summary.p99_ns)
            )
        })
        .collect();
    let broadphase = report
        .phases
        .iter()
        .find(|figure| figure.phase == pharmakos_sim::world::Phase::Broadphase)
        .map(|figure| figure.summary.mean_ns)
        .ok_or_else(|| Error::Drift("no Broadphase phase in the report".to_owned()))?;
    let share = per_mille(broadphase, report.tick.mean_ns)
        .ok_or_else(|| Error::Empty("a tick mean of zero".to_owned()))?;
    Ok(format!(
        "::notice title=G3' phases::mean / p99 per phase on {os}: {}; begin {} / {}; finish \
         (the hash and the gateway's per-tick bookkeeping) {} / {}. Tick minus pathing mean {} \
         (pathing is the sim's own, CSR rebuilt once in Broadphase); broadphase {} of the tick's \
         mean, with E11's pad",
        phases.join("; "),
        micros(report.begin.mean_ns),
        micros(report.begin.p99_ns),
        micros(report.finish.mean_ns),
        micros(report.finish.p99_ns),
        ms(report.tick_minus_pathing_mean_ns),
        percent(share),
    ))
}

/// The repath notice (item 69).
///
/// # Errors
///
/// [`Error::Empty`] for a segment of no tick.
pub fn repaths_notice(report: &SegmentReport, os: &str) -> Result<String, Error> {
    let r = &report.repaths;
    let bursts = per_mille(u128::from(r.ticks_at_cap), u128::from(report.ticks))
        .ok_or_else(|| Error::Empty("a segment of no tick".to_owned()))?;
    Ok(format!(
        "::notice title=G3' repaths::on {os}: the cap is {} a tick; {} repaths served over {} \
         ticks; {} ticks spent the whole cap, demand held back ({} of ticks), the longest run of \
         them {} ticks; the largest backlog after a serve {}",
        r.cap,
        r.served,
        report.ticks,
        r.ticks_at_cap,
        percent(bursts),
        r.longest_run_at_cap,
        r.max_backlog,
    ))
}

/// The drift notice (item 68; spike step 6).
#[must_use]
pub fn drift_notice(report: &SegmentReport, os: &str) -> String {
    let p99s: Vec<String> = report
        .minutes
        .iter()
        .map(|minute| format!("{} {}", minute.number, ms(minute.tick_p99_ns)))
        .collect();
    let heap = match report.heap_at_start {
        Some(start) => {
            let minutes: Vec<String> = report
                .minutes
                .iter()
                .map(|minute| match minute.live_heap_bytes {
                    Some(bytes) => format!("{} {bytes}", minute.number),
                    None => format!("{} not counted", minute.number),
                })
                .collect();
            format!(
                "live heap bytes at the Push's start {start}, at each minute's end {}",
                minutes.join(", ")
            )
        }
        None => "the heap was not counted".to_owned(),
    };
    let allocations = match report.allocations {
        Some(counted) => format!(
            "allocations per tick in begin and the phases mean {} (most {}), in finish mean {} \
             (most {})",
            thousandths(counted.phases_mean_milli),
            counted.phases_max,
            thousandths(counted.finish_mean_milli),
            counted.finish_max
        ),
        None => "allocations were not counted".to_owned(),
    };
    format!(
        "::notice title=G3' drift::on {os}: tick p99 by minute {}; {heap}; {allocations}",
        p99s.join(", ")
    )
}

/// The combat and bookkeeping notice: deaths and kill credit (G3′ section
/// 9.12), the event bus (E15) and the queues (E5).
///
/// # Errors
///
/// [`Error::Drift`] for a report with no `KillCredit` phase.
pub fn combat_notice(report: &SegmentReport, os: &str) -> Result<String, Error> {
    let credit = report
        .phases
        .iter()
        .find(|figure| figure.phase == pharmakos_sim::world::Phase::KillCredit)
        .map(|figure| figure.summary)
        .ok_or_else(|| Error::Drift("no KillCredit phase in the report".to_owned()))?;
    let minutes = u64::try_from(report.minutes.len()).map_err(|_| Error::Overflow("minutes"))?;
    let rate = report
        .deaths
        .checked_mul(1_000)
        .and_then(|scaled| scaled.checked_div(minutes))
        .ok_or_else(|| Error::Empty("a segment of no minute".to_owned()))?;
    let live = report
        .minutes
        .last()
        .map(|minute| minute.live_units)
        .ok_or_else(|| Error::Empty("a segment of no minute".to_owned()))?;
    Ok(format!(
        "::notice title=G3' combat::on {os}: {} deaths over the segment ({} a minute) and {} \
         kill credits booked; the KillCredit phase mean {}, p99 {}, max {}; {} units walking a \
         tick on average, {live} alive at the end; at most {} events in one tick against the \
         bus's {}; the damage and edit queues hold room for {} bytes; {} regions fell",
        report.deaths,
        thousandths(rate),
        report.kill_credits,
        micros(credit.mean_ns),
        micros(credit.p99_ns),
        micros(credit.max_ns),
        thousandths(report.movers_mean_milli),
        report.peak_events,
        segment::bus_capacity(),
        report.queue_bytes,
        report.regions_fallen,
    ))
}

/// The `seen_by` notice and the broadphase's growth with density, minute by
/// minute (item 148 (4); item 67's caveat).
#[must_use]
pub fn sight_notice(report: &SegmentReport, os: &str) -> String {
    let minutes: Vec<String> = report
        .minutes
        .iter()
        .map(|minute| {
            let sight = match minute.sight {
                Some(probe) => format!(
                    "{} armed and {} sensors, in range {} calls / {} sensor visits / {}, every \
                     target {} calls / {} visits / {}",
                    probe.armed,
                    probe.sensors,
                    probe.in_range.calls,
                    probe.in_range.visits,
                    micros(probe.in_range.ns),
                    probe.every_target.calls,
                    probe.every_target.visits,
                    micros(probe.every_target.ns),
                ),
                None => "not probed".to_owned(),
            };
            format!(
                "minute {}: {} units alive, broadphase mean {}, seen_by {sight}",
                minute.number,
                minute.live_units,
                micros(minute.broadphase_mean_ns),
            )
        })
        .collect();
    format!(
        "::notice title=G3' seen_by and density::on {os}, seen_by restated from the sim's \
         sensor walk and timed between ticks: {}",
        minutes.join("; ")
    )
}

/// The support search's adversarial tick.
#[must_use]
pub fn support_notice(report: &SupportReport, os: &str) -> String {
    let phases: Vec<String> = report
        .phases
        .iter()
        .map(|(phase, ns)| format!("{phase:?} {}", micros(*ns)))
        .collect();
    format!(
        "::notice title=G3' support search::the adversarial tick on {os} took {} ({}): {} \
         regions fell, {} voxels moved, in {} passes; the searches visited {} of the {} a tick \
         allows, {} seeds labelled supported for the budget and {} for the {}-voxel region \
         bound; built of {} voxels over {} ticks at the site {:?}",
        ms(report.tick_ns),
        phases.join(", "),
        report.regions,
        report.voxels,
        report.passes,
        report.visits,
        report.max_visits_per_tick,
        report.budget_skips,
        report.bound_skips,
        report.max_region_voxels,
        report.built_voxels,
        report.build_ticks,
        report.site,
    )
}

/// The fourth seat's line.
///
/// # Errors
///
/// As [`FourthSeat::cost_ns`].
pub fn fourth_notice(line: &FourthSeat, os: &str) -> Result<String, Error> {
    Ok(format!(
        "::notice title=G3' fourth seat::a fourth, unplaced seat sealed at the size budget adds \
         {} ns to a decision tick's Decision phase on {os} (three seats {}, four {}, mean per \
         decision tick over {} decision ticks of {} ticks, fastest of {} runs per tick); it spent \
         {} evaluation units on its last decision{}. Decision 2: measured at three placed seats, \
         the fourth reported apart",
        line.cost_ns()?,
        micros(line.three_ns),
        micros(line.four_ns),
        line.decision_ticks,
        line.ticks,
        fourth::RUNS,
        line.fourth_units,
        if line.fourth_units == 0 {
            " (an unplaced seat has no commander, so its playbook has nothing to run: the difference is the seat's dispatch and the runs' noise, not a playbook's cost)"
        } else {
            ""
        },
    ))
}

/// The footing query's line (item 148 (4); `site`'s worst case, #133).
///
/// # Errors
///
/// [`Error::Overflow`] for a time past a `u64` of thousandths of a nanosecond.
pub fn footing_query_notice(
    ended: &FootingQuery,
    full: &FootingQuery,
    os: &str,
) -> Result<String, Error> {
    Ok(format!(
        "::notice title=G3' footing query::on {os}, a set's footing check (a Mine dig) that \
         misses, a call (over {} calls), and a blast's ball of radius {} whose box meets a live \
         beacon's footing, {} voxels each asked (the worst case, (2r + 1)^3 voxels times the \
         rows), fastest of {} runs each: over full tables ({} structure and {} beacon rows, the \
         most a match holds, filled on a world of the bench's own) {} ns and {}; over the world \
         the segment ended on ({} structure and {} beacon rows) {} ns and {}",
        support::SET_CALLS,
        full.radius,
        full.ball_voxels,
        support::FOOTING_RUNS,
        full.structure_rows,
        full.beacon_rows,
        thousandths(per_call_milli_ns(full.set_ns)?),
        micros(full.blast_ns),
        ended.structure_rows,
        ended.beacon_rows,
        thousandths(per_call_milli_ns(ended.set_ns)?),
        micros(ended.blast_ns),
    ))
}

/// The line `cargo xtask g3-budget` reads: the ticks played and the ticks the
/// segment holds (so a short run is refused), and the tick's figures in whole
/// nanoseconds, `key=value`, after [`BUDGET_REPORT_PREFIX`].
#[must_use]
pub fn budget_report_line(report: &SegmentReport) -> String {
    format!(
        "{BUDGET_REPORT_PREFIX} ticks={} segment_ticks={} p50_ns={} p99_ns={} mean_ns={} \
         max_ns={}",
        report.ticks,
        report.segment_ticks,
        report.tick.p50_ns,
        report.tick.p99_ns,
        report.tick.mean_ns,
        report.tick.max_ns,
    )
}

/// A signed nanosecond figure as milliseconds with three places.
fn signed_ms(ns: i128) -> String {
    let sign = if ns < 0 { "-" } else { "" };
    format!("{sign}{}", ms(ns.unsigned_abs()))
}

/// The power sweep's notices: the points, then the fit and the budget it
/// gives (items 65 and 91; `tune2`'s inputs).
///
/// # Errors
///
/// [`Error::Overflow`] for a unit's cost past a `u64` or below zero.
pub fn sweep_notices(sweep: &Sweep, seed: u64, os: &str) -> Result<Vec<String>, Error> {
    let point = |p: &sweep::Point| {
        format!(
            "{}/4 of the units at {} craters a second (bench rules_hash {}, layout {}, {} \
             ticks): {} live units, tick mean {}, p99 {}, Pathing mean {}",
            p.share.get(),
            p.craters_per_second,
            hex(p.bench_rules_hash),
            hex(p.layout_digest),
            p.ticks,
            thousandths(p.live_units_milli),
            ms(p.mean_ns),
            ms(p.p99_ns),
            ms(p.pathing_mean_ns),
        )
    };
    let units: Vec<String> = sweep.units.iter().map(point).collect();
    let craters: Vec<String> = sweep.craters.iter().map(point).collect();
    // Picoseconds a unit, printed as nanoseconds with three places.
    let unit_ps = u64::try_from(sweep.unit_ps).map_err(|_| Error::Overflow("a unit's cost"))?;
    let reach = if sweep.extrapolated() {
        format!(
            "EXTRAPOLATED past the largest measured load ({} live units): re-measure at that \
             load before it is used (items 63 and 67)",
            thousandths(sweep.largest_measured_milli)
        )
    } else {
        format!(
            "inside the measured loads (up to {} live units)",
            thousandths(sweep.largest_measured_milli)
        )
    };
    Ok(vec![
        format!(
            "::notice title=G3' power sweep::on {os}, seed 0x{seed:016x}, the committed table's \
             rules_hash {}, each point an 8-minute segment of the gate's layout: {}; and at the \
             full share, the destruction rate's weight on the tick (not budgets): {}",
            hex(sweep.shipped_rules_hash),
            units.join("; "),
            craters.join("; "),
        ),
        format!(
            "::notice title=G3' power budget::on {os}, item 65's arithmetic measured: the tick's \
             mean fitted against the live units at 20 craters a second is {} fixed and crater \
             cost plus {} ns a live unit (worst point {} off the line); the {} mean bar less \
             power.reserve_percent {} leaves {}, which carries {} live units, {reach}; at \
             power.kw_per_unit {}, with the gate's 40 beacons at power.beacon_base_draw_kw {} \
             and 3 seats' power.revive_margin_kw {}, a map ceiling of {} kW (committed \
             power.map_ceiling_kw {}; structures' draw is not in it, as no structure is in the \
             measured load)",
            signed_ms(sweep.intercept_ns),
            thousandths(unit_ps),
            signed_ms(sweep.worst_residual_ns),
            ms(MEAN_BAR_NS),
            sweep.reserve_percent,
            signed_ms(sweep.budget_ns),
            sweep.units_carried,
            sweep.kw_per_unit,
            sweep.beacon_draw_kw,
            sweep.revive_margin_kw,
            sweep.ceiling_kw,
            sweep.shipped_ceiling_kw,
        ),
    ])
}

/// One `set` check's time, in thousandths of a nanosecond, from the time of
/// [`support::SET_CALLS`] of them.
fn per_call_milli_ns(total_ns: u128) -> Result<u64, Error> {
    let milli = total_ns
        .checked_mul(1_000)
        .and_then(|scaled| scaled.checked_div(u128::from(support::SET_CALLS)))
        .ok_or(Error::Overflow("a set check's time"))?;
    u64::try_from(milli).map_err(|_| Error::Overflow("a set check's time"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_small_formatters_keep_their_places() {
        assert_eq!(percent(125), "12.5%");
        assert_eq!(percent(7), "0.7%");
        assert_eq!(thousandths(61_500), "61.500");
        assert_eq!(thousandths(5), "0.005");
    }

    #[test]
    fn the_fourth_seat_notice_is_word_for_word() {
        let line = FourthSeat {
            ticks: 600,
            decision_ticks: 120,
            three_ns: 300_000,
            four_ns: 412_345,
            fourth_units: 57,
        };
        assert_eq!(
            fourth_notice(&line, "linux").expect("a notice"),
            "::notice title=G3' fourth seat::a fourth, unplaced seat sealed at the size budget \
             adds 112345 ns to a decision tick's Decision phase on linux (three seats 300.000 \
             us, four 412.345 us, mean per decision tick over 120 decision ticks of 600 ticks, \
             fastest of 3 runs per tick); it spent 57 evaluation units on its last decision. \
             Decision 2: measured at three placed seats, the fourth reported apart"
        );
    }
}
