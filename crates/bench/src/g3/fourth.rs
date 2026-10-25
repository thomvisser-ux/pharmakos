// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A fourth, unplaced seat's decision-tick cost, the separate line decision 2
//! asks for beside the three-seat measurement.
//!
//! The spec's G3′ row names four seats; v1 seats three, the generator places
//! three zones, and the gateway refuses a fourth (`Host::check_seats`,
//! `MAX_SEATS`; S2's plan, E19). The sim seats four, the fourth with no core,
//! force, money or power. So this line, and only this line, drives the sim's
//! own match driver from outside, as `crates/sim/tests/` and the determinism
//! harness do: [`Runner`], each seat's playbook compiled by the sim's
//! [`Plan::compile`] (the call the gateway's `submit_plan` makes after the
//! verifier accepts, `crates/gateway/src/surface/planning.rs`) from the same
//! budget playbook P1 seals, which the gateway's door has accepted at the size
//! budget for the three seats it seats ([`crate::hosted`]).
//!
//! **What is measured.** The same seed is played twice, three seats sealed and
//! then four, every tick stepped phase by phase and the Decision phase timed
//! alone. Each configuration is played [`RUNS`] times and each tick keeps its
//! fastest Decision phase; the line is the difference of the two means per
//! decision tick, and the fourth seat's counted evaluation units
//! (`World::decision_work`) say how much of the playbook it ran. No layout:
//! at four seats a seat's share of the world totals is 75 units and 10
//! beacons, which decision 2's 100 a seat does not fit, so both runs are the
//! generated map alone.

use std::path::Path;
use std::time::Instant;

use pharmakos_proto::gp::v1::Playbook;
use pharmakos_proto::json;
use pharmakos_sim::interpreter::Plan;
use pharmakos_sim::math::quantity::MS_PER_TICK;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::runner::{MatchSettings, Runner};
use pharmakos_sim::world::{Phase, World, WorldConfig};

use crate::decision::hosted_budget_playbook;
use crate::error::Error;
use crate::hosted::seat_ids;

/// How many times each configuration is played.
pub const RUNS: usize = 3;

/// What the line measured.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FourthSeat {
    /// Ticks per run.
    pub ticks: u64,
    /// Decision ticks per run.
    pub decision_ticks: u64,
    /// The Decision phase's mean per decision tick with three seats sealed,
    /// in nanoseconds.
    pub three_ns: u128,
    /// The same with the fourth, unplaced seat sealed too.
    pub four_ns: u128,
    /// The fourth seat's evaluation units on its last decision.
    pub fourth_units: u32,
}

impl FourthSeat {
    /// The fourth seat's cost per decision tick: four less three, in
    /// nanoseconds, negative when it is lost in the noise.
    ///
    /// # Errors
    ///
    /// [`Error::Overflow`] for a figure past an `i128`.
    pub fn cost_ns(&self) -> Result<i128, Error> {
        let four = i128::try_from(self.four_ns).map_err(|_| Error::Overflow("a decision mean"))?;
        let three =
            i128::try_from(self.three_ns).map_err(|_| Error::Overflow("a decision mean"))?;
        four.checked_sub(three)
            .ok_or(Error::Overflow("the fourth seat's cost"))
    }
}

/// One run: the Decision phase's time on every tick, and the fourth seat's
/// units at the end (when there is one).
fn play(
    rules: &RulesTable,
    seed: u64,
    seats: u32,
    segment_ms: i32,
    playbook: &Playbook,
) -> Result<(Vec<u128>, Option<u32>), Error> {
    let world = World::new(&WorldConfig {
        match_seed: seed,
        seats,
        units_per_seat: 0,
        rules: rules.clone(),
        match_settings: MatchSettings {
            segment_lengths_ms: vec![segment_ms],
            round_limit: 1,
        },
    })
    .map_err(|error| Error::Sim {
        during: "building the fourth seat's world",
        reason: error.to_string(),
    })?;
    let mut runner = Runner::new(world);
    for seat in seat_ids(seats)? {
        let plan = Plan::compile(playbook, rules).map_err(|error| Error::Sim {
            during: "compiling the budget playbook",
            reason: format!("{error:?}"),
        })?;
        runner
            .seal_playbook(seat, plan)
            .map_err(|error| Error::Sim {
                during: "sealing the budget playbook",
                reason: format!("{error:?}"),
            })?;
    }
    if !runner.begin_push().map_err(|error| Error::Sim {
        during: "opening the Push",
        reason: format!("{error:?}"),
    })? {
        return Err(Error::Sim {
            during: "opening the Push",
            reason: "the match was not in its Lull".to_owned(),
        });
    }
    let mut decision: Vec<u128> = Vec::new();
    loop {
        let refused = |during: &'static str| {
            move |error: pharmakos_sim::world::TickError| Error::Sim {
                during,
                reason: format!("{error:?}"),
            }
        };
        let Some(mut tick) = runner.phased_tick().map_err(refused("opening a tick"))? else {
            break;
        };
        let mut ns: u128 = 0;
        loop {
            let next = tick.next_phase();
            let started = Instant::now();
            let ran = tick.run_next_phase().map_err(refused("running a phase"))?;
            let elapsed = started.elapsed().as_nanos();
            if ran.is_none() {
                break;
            }
            if next == Some(Phase::Decision) {
                ns = elapsed;
            }
        }
        decision.push(ns);
        let report = tick.finish().map_err(refused("finishing a tick"))?;
        if report.segment_ended {
            break;
        }
    }
    let fourth = runner
        .world()
        .decision_work(3)
        .map(pharmakos_sim::seams::WorkCounter::spent);
    Ok((decision, fourth))
}

/// The mean of the per-tick fastest Decision phase over `runs` runs, per
/// decision tick.
fn configuration(
    rules: &RulesTable,
    seed: u64,
    seats: u32,
    segment_ms: i32,
    playbook: &Playbook,
) -> Result<(u128, u64, u64, Option<u32>), Error> {
    let (mut best, mut fourth) = play(rules, seed, seats, segment_ms, playbook)?;
    for _ in 1..RUNS {
        let (run, units) = play(rules, seed, seats, segment_ms, playbook)?;
        if run.len() != best.len() {
            return Err(Error::Drift(format!(
                "two runs of the {seats}-seat match played {} and {} ticks",
                best.len(),
                run.len()
            )));
        }
        for (kept, now) in best.iter_mut().zip(&run) {
            *kept = (*kept).min(*now);
        }
        fourth = units;
    }
    let ticks = u64::try_from(best.len()).map_err(|_| Error::Overflow("the tick count"))?;
    let cadence = u64::try_from(rules.decision_tick_ms())
        .map_err(|_| Error::Rules("match.decision_tick_ms is not positive".to_owned()))?;
    let tick_ms = u64::try_from(MS_PER_TICK).map_err(|_| Error::Overflow("the tick length"))?;
    let decision_ticks = ticks
        .checked_mul(tick_ms)
        .and_then(|ms| ms.checked_div(cadence))
        .ok_or_else(|| Error::Rules("match.decision_tick_ms is zero".to_owned()))?;
    let sum = best
        .iter()
        .try_fold(0_u128, |sum, ns| sum.checked_add(*ns))
        .ok_or(Error::Overflow("the Decision phase's sum"))?;
    let mean = sum
        .checked_div(u128::from(decision_ticks))
        .ok_or_else(|| Error::Empty("the segment held no decision tick".to_owned()))?;
    Ok((mean, ticks, decision_ticks, fourth))
}

/// Measure the fourth seat's line on `seed` over a segment of `segment_ms`.
///
/// # Errors
///
/// When the budget playbook cannot be read or compiled, the sim refuses the
/// world or a seal, or the two configurations play different lengths.
pub fn measure(
    root: &Path,
    rules: &RulesTable,
    seed: u64,
    segment_ms: i32,
) -> Result<FourthSeat, Error> {
    let text = hosted_budget_playbook(root)?;
    let playbook: Playbook = json::decode(&text).map_err(|error| Error::Codec {
        what: "decoding the hosted budget playbook".to_owned(),
        reason: error.to_string(),
    })?;
    let (three_ns, ticks, decision_ticks, _) =
        configuration(rules, seed, 3, segment_ms, &playbook)?;
    let (four_ns, four_ticks, _, fourth) = configuration(rules, seed, 4, segment_ms, &playbook)?;
    if four_ticks != ticks {
        return Err(Error::Drift(format!(
            "the three-seat match played {ticks} ticks and the four-seat match {four_ticks}"
        )));
    }
    Ok(FourthSeat {
        ticks,
        decision_ticks,
        three_ns,
        four_ns,
        fourth_units: fourth
            .ok_or_else(|| Error::Drift("the four-seat world has no fourth seat".to_owned()))?,
    })
}
