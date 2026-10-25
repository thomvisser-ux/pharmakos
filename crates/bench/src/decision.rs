// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A decision tick's wall-clock cost per size unit (P1, S1's half).
//!
//! Spec section 16's P1 row: "The per-rule cost on a decision tick (one per
//! 250 ms of game time) is measured against the 50 ms sim tick to set the
//! playbook size budget". S1's plan section 5 item 3 splits it: the counted
//! evaluation units are `tgt`'s, asserted inside the sim; the **wall-clock
//! cost in ns per size unit** is this module's, "stepping a hosted match whose
//! playbooks sit at the size budget".
//!
//! # The match
//!
//! Hosted exactly as `gamectl scenario run` hosts one: the gateway's [`Host`]
//! behind a [`Surface`], each seat's playbook arriving through `submit_plan`
//! with that seat's own token and sealed by [`Surface::begin_push`]. Nothing
//! here seals around the verifier's door: a seat whose submission is refused,
//! or that is not exactly on the size budget, stops the measurement with the
//! reason.
//!
//! Every seat seals [`hosted_budget_playbook`]: the verifier's committed
//! `budget_128` case (64 route steps and 16 handlers of three steps each, 128
//! units) with its sixteen `beacon_anchor { beacon_id: "b_01" }` targets
//! written as `beacon_anchor { safest {} }`. The case names the fixture's
//! core, which is no seat's name on a hosted map (and under targeting's
//! per-seat names a core is `b_00`), and a reference by role resolves for
//! every seat on every map; the size is unchanged, because a target counts
//! nothing (decisions-log item 94).
//!
//! # The figure
//!
//! A decision tick runs everything an ordinary tick runs plus the decisions
//! (`World::is_decision_tick`). So each decision tick is paired with the
//! ordinary ticks that follow it up to the next decision tick, and the excess
//! is the decision tick's time less their mean. The mean excess over the
//! segment, divided by the units the seats sealed (seats x 128), is the cost
//! per size unit per decision tick.
//!
//! Said honestly: the excess is **everything** that runs only on a decision
//! tick -- the playbook interpreter, and the programs', gathering's and
//! survey's decision-tick work -- so the per-unit figure is an upper bound on
//! the interpreter's own cost per unit, and it is a mean (one decision tick
//! is not the worst one). `tgt`'s counted evaluation units say how much of
//! the work is the playbook's.
//!
//! Noise: the same match is played [`DecisionPlan::runs`] times, and each tick
//! keeps the fastest of its runs, the mesher alarm's best-of-N for the same
//! reason (a single sample is preemption as much as code). The runs must
//! produce the same hash chain, which is what makes "the same tick" mean the
//! same work; a chain that differs stops the measurement.

use std::path::Path;
use std::time::Instant;

use pharmakos_proto::gp::v1::{
    BeaconRef, Location, Playbook, Safest, Step, beacon_ref, location, step,
};
use pharmakos_proto::json;
use pharmakos_sim::math::quantity::MS_PER_TICK;
use pharmakos_sim::rules::RulesTable;

use crate::error::Error;
use crate::hosted::{self, Opened, Opening};
use crate::quick::{BUDGET_CASE, cases_dir};
use crate::stats::{self, micros};

/// The map the decision-tick figure is measured on: the seed every committed
/// scenario plays (`scenarios/**`), so the map is one the project already
/// pins.
///
/// PLACEHOLDER: the map P1's per-unit figure is certified on — owner, at S3's exit, with FULL and the size budget, where P1 is certified again (item 139 (1) ruling 7: it is P1's map for S1).
pub const BENCH_SEED: u64 = 0x0000_0000_ca5c_aded;

/// The seats S1's plan section 5 item 3 states the implied budget for.
pub const BENCH_SEATS: u32 = 3;

/// How the match is played and sampled.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DecisionPlan {
    /// The map seed.
    pub seed: u64,
    /// How many seats, each sealing the budget playbook.
    pub seats: u32,
    /// The one segment's length, in game milliseconds.
    pub segment_ms: i32,
    /// How many times the match is played; each tick keeps its fastest run.
    pub runs: usize,
}

/// What [`measure`] found.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DecisionTiming {
    /// Seats that sealed the budget playbook.
    pub seats: u32,
    /// The size units each seat sealed: the size budget.
    pub units_per_seat: u32,
    /// Ticks played per run.
    pub ticks: usize,
    /// Decision ticks paired with at least one ordinary tick.
    pub windows: usize,
    /// The mean decision tick, in nanoseconds (fastest run per tick).
    pub decision_ns: u128,
    /// The mean ordinary tick, in nanoseconds (fastest run per tick).
    pub ordinary_ns: u128,
    /// The mean excess of a decision tick over its window's ordinary ticks.
    pub excess_ns: i128,
    /// The excess per size unit sealed, in nanoseconds; zero or below when the
    /// excess is lost in the noise.
    pub ns_per_unit: i128,
}

impl DecisionTiming {
    /// The units per seat whose decision-tick excess would fill a whole tick
    /// (`MS_PER_TICK`) with `seats` seats, at this cost per unit: the ceiling
    /// S3 sets the size budget under, never the budget itself. `None` when no
    /// cost per unit was measured.
    #[must_use]
    pub fn implied_units_per_seat(&self, seats: u32) -> Option<u128> {
        let per_unit = u128::try_from(self.ns_per_unit).ok().filter(|ns| *ns > 0)?;
        // `MS_PER_TICK` is a positive constant, so its magnitude is its value.
        // 50 ms in nanoseconds: a constant far inside a `u128`.
        let tick_ns = u128::from(MS_PER_TICK.unsigned_abs()) * 1_000_000;
        match per_unit.checked_mul(u128::from(seats)) {
            Some(per_tick) => tick_ns.checked_div(per_tick),
            // A cost past a `u128` a unit fits no unit in a tick: exactly zero.
            None => Some(0),
        }
    }
}

/// The budget playbook every bench seat seals, as canonical JSON: the
/// verifier's committed `budget_128` with each `beacon_anchor` named by id
/// written as `beacon_anchor { safest {} }` (see the module docs).
///
/// # Errors
///
/// When the case cannot be read or decoded, or names no beacon by id (then
/// it is no longer the case this module documents).
pub fn hosted_budget_playbook(root: &Path) -> Result<String, Error> {
    let path = cases_dir(root).join(format!("{BUDGET_CASE}.json"));
    let text = std::fs::read_to_string(&path).map_err(|error| Error::Read {
        path: path.clone(),
        reason: error.to_string(),
    })?;
    let mut playbook: Playbook = json::decode(&text).map_err(|error| Error::Codec {
        what: format!("decoding {}", path.display()),
        reason: error.to_string(),
    })?;
    let mut renamed = 0_usize;
    if let Some(declarative) = playbook.declarative.as_mut() {
        for entry in &mut declarative.route {
            renamed = renamed
                .checked_add(anchor_by_role(entry))
                .ok_or(Error::Overflow("the renamed anchors"))?;
        }
        for handler in &mut declarative.handlers {
            for entry in &mut handler.body {
                renamed = renamed
                    .checked_add(anchor_by_role(entry))
                    .ok_or(Error::Overflow("the renamed anchors"))?;
            }
        }
    }
    if renamed == 0 {
        return Err(Error::Drift(format!(
            "{} names no beacon by id any more; the bench's hosted playbook is documented as \
             that case with its named anchors written by role, so the module docs need saying \
             again",
            path.display()
        )));
    }
    json::encode(&playbook).map_err(|error| Error::Codec {
        what: "encoding the hosted playbook".to_owned(),
        reason: error.to_string(),
    })
}

/// A move to a beacon named by id becomes a move to the safest own beacon.
/// Answers how many it rewrote (zero or one).
fn anchor_by_role(entry: &mut Step) -> usize {
    let Some(step::Kind::Move(movement)) = entry.kind.as_mut() else {
        return 0;
    };
    let Some(Location {
        place: Some(location::Place::BeaconAnchor(anchor)),
    }) = movement.to.as_mut()
    else {
        return 0;
    };
    if !matches!(anchor.r#ref, Some(beacon_ref::Ref::BeaconId(_))) {
        return 0;
    }
    *anchor = BeaconRef {
        r#ref: Some(beacon_ref::Ref::Safest(Safest::default())),
    };
    1
}

/// One tick as one run saw it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Sample {
    ns: u128,
    hash: u64,
    decision: bool,
    last: bool,
}

/// Measure a decision tick's cost per size unit on `plan`'s match.
///
/// # Errors
///
/// When the match will not open, a seat's budget playbook is refused or is
/// not exactly on the size budget, the runs disagree about the hash chain, or
/// no decision tick was paired with an ordinary one.
pub fn measure(
    root: &Path,
    rules: &RulesTable,
    plan: DecisionPlan,
) -> Result<DecisionTiming, Error> {
    if plan.runs == 0 {
        return Err(Error::Empty(
            "a decision-tick measurement of zero runs measures nothing".to_owned(),
        ));
    }
    let playbook = hosted_budget_playbook(root)?;
    let mut best: Option<(Vec<Sample>, u32)> = None;
    for run in 0..plan.runs {
        let (samples, units) = play(rules, plan, &playbook)?;
        let Some((kept, _)) = best.as_mut() else {
            best = Some((samples, units));
            continue;
        };
        if samples.len() != kept.len()
            || samples
                .iter()
                .zip(kept.iter())
                .any(|(now, kept)| now.hash != kept.hash || now.decision != kept.decision)
        {
            return Err(Error::Drift(format!(
                "run {run} of the same match produced a different hash chain or decision \
                 cadence; the per-tick minimum would compare different work"
            )));
        }
        for (kept, now) in kept.iter_mut().zip(&samples) {
            kept.ns = kept.ns.min(now.ns);
        }
    }
    let (best, units_per_seat) =
        best.ok_or_else(|| Error::Empty("no run of the bench match was played".to_owned()))?;
    summarise(&best, plan.seats, units_per_seat)
}

/// A tick's nanoseconds as a signed figure.
fn signed(ns: u128) -> Result<i128, Error> {
    i128::try_from(ns).map_err(|_| Error::Overflow("a tick's nanoseconds"))
}

/// Pair each decision tick with the ordinary ticks after it, and average.
fn summarise(samples: &[Sample], seats: u32, units_per_seat: u32) -> Result<DecisionTiming, Error> {
    let played: Vec<&Sample> = samples.iter().filter(|sample| !sample.last).collect();
    let decision: Vec<u128> = played
        .iter()
        .filter(|sample| sample.decision)
        .map(|sample| sample.ns)
        .collect();
    let ordinary: Vec<u128> = played
        .iter()
        .filter(|sample| !sample.decision)
        .map(|sample| sample.ns)
        .collect();

    let mut excesses: Vec<i128> = Vec::new();
    let mut index = 0_usize;
    while let Some(sample) = played.get(index) {
        // `index` is below `played.len()` here, so the step cannot overflow.
        index += 1;
        if !sample.decision {
            continue;
        }
        let mut window: Vec<u128> = Vec::new();
        while let Some(next) = played.get(index) {
            if next.decision {
                break;
            }
            window.push(next.ns);
            index += 1;
        }
        // A decision tick with no ordinary tick after it has nothing to be
        // paired with, and is left out of the pairing by its definition.
        let Some(mean) = stats::mean(&window) else {
            continue;
        };
        let excess = signed(sample.ns)?
            .checked_sub(signed(mean)?)
            .ok_or(Error::Overflow("a decision tick's excess"))?;
        excesses.push(excess);
    }
    let windows = excesses.len();
    let excess_sum = excesses
        .iter()
        .try_fold(0_i128, |sum, excess| sum.checked_add(*excess))
        .ok_or(Error::Overflow("the summed excess"))?;
    let excess_ns = excess_sum
        .checked_div(i128::try_from(windows).map_err(|_| Error::Overflow("the window count"))?)
        .ok_or_else(|| {
            Error::Empty(
                "no decision tick was followed by an ordinary tick, so there is nothing to pair"
                    .to_owned(),
            )
        })?;
    let units = i128::from(seats)
        .checked_mul(i128::from(units_per_seat))
        .ok_or(Error::Overflow("the units sealed"))?;
    let ns_per_unit = excess_ns
        .checked_div(units)
        .ok_or_else(|| Error::Empty("no size unit was sealed to divide by".to_owned()))?;
    Ok(DecisionTiming {
        seats,
        units_per_seat,
        ticks: samples.len(),
        windows,
        decision_ns: stats::mean(&decision)
            .ok_or_else(|| Error::Empty("no decision tick was played".to_owned()))?,
        ordinary_ns: stats::mean(&ordinary)
            .ok_or_else(|| Error::Empty("no ordinary tick was played".to_owned()))?,
        excess_ns,
        ns_per_unit,
    })
}

/// Play the match once: open it, seal the budget playbook for every seat
/// through `submit_plan`, and time every tick of the one segment.
fn play(
    rules: &RulesTable,
    plan: DecisionPlan,
    playbook: &str,
) -> Result<(Vec<Sample>, u32), Error> {
    let opening = Opening {
        seed: plan.seed,
        seats: plan.seats,
        segment_ms: plan.segment_ms,
        rules,
        playbook,
    };
    let host = hosted::open_host(&opening)?;
    let Opened {
        mut surface,
        units_per_seat,
    } = hosted::attach(host, None, &opening)?;

    let mut samples: Vec<Sample> = Vec::new();
    loop {
        let started = Instant::now();
        let report = surface.step();
        let ns = started.elapsed().as_nanos();
        let Some(report) = report.map_err(|error| Error::gateway("stepping a tick", &error))?
        else {
            break;
        };
        let decision = hosted::world(&surface)?.is_decision_tick();
        samples.push(Sample {
            ns,
            hash: report.hash,
            decision,
            last: report.segment_ended,
        });
        if report.segment_ended {
            break;
        }
    }
    Ok((samples, units_per_seat))
}

/// The notice for one decision-tick measurement, word for word. Pure, so the
/// text is pinned by a unit test.
#[must_use]
pub fn notice(timing: &DecisionTiming, os: &str) -> String {
    let implied = match timing.implied_units_per_seat(BENCH_SEATS) {
        Some(units) => format!("{units} units per seat"),
        None => "none: no cost per unit was measured above the noise".to_owned(),
    };
    format!(
        "::notice title=decision tick per size unit::{} ns per size unit per decision tick on \
         {os} ({} seats x {} units sealed; a decision tick {} against an ordinary tick {}, mean \
         excess {} ns over {} windows of {} ticks, fastest run per tick); a whole {MS_PER_TICK} \
         ms tick with {BENCH_SEATS} seats would hold {implied}, a ceiling and not a budget",
        timing.ns_per_unit,
        timing.seats,
        timing.units_per_seat,
        micros(timing.decision_ns),
        micros(timing.ordinary_ns),
        timing.excess_ns,
        timing.windows,
        timing.ticks,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(ns: u128, decision: bool) -> Sample {
        Sample {
            ns,
            hash: 0,
            decision,
            last: false,
        }
    }

    #[test]
    fn a_decision_tick_is_paired_with_the_ordinary_ticks_after_it() {
        // Two windows: 900 over a mean of 100, and 700 over a mean of 200.
        let mut samples = vec![
            sample(900, true),
            sample(100, false),
            sample(100, false),
            sample(700, true),
            sample(150, false),
            sample(250, false),
            // The tick that closed the segment is never paired.
            Sample {
                ns: 50_000,
                hash: 0,
                decision: false,
                last: true,
            },
        ];
        let timing = summarise(&samples, 3, 128).expect("two windows");
        assert_eq!(timing.windows, 2);
        assert_eq!(timing.excess_ns, 650);
        assert_eq!(timing.ns_per_unit, 1);
        assert_eq!(timing.decision_ns, 800);
        assert_eq!(timing.ordinary_ns, 150);
        assert_eq!(timing.ticks, 7);

        // A decision tick at the very end has nothing to pair with.
        samples.insert(6, sample(10_000, true));
        assert_eq!(summarise(&samples, 3, 128).expect("still two").windows, 2);
        assert!(summarise(&[sample(5, true)], 3, 128).is_err());
    }

    #[test]
    fn the_implied_ceiling_fills_one_tick() {
        let timing = DecisionTiming {
            seats: 3,
            units_per_seat: 128,
            ticks: 1_200,
            windows: 239,
            decision_ns: 0,
            ordinary_ns: 0,
            excess_ns: 0,
            ns_per_unit: 100,
        };
        // 50 ms is 50 000 000 ns; three seats at 100 ns a unit is 300 ns a unit-row.
        assert_eq!(timing.implied_units_per_seat(3), Some(166_666));
        let free = DecisionTiming {
            ns_per_unit: 0,
            ..timing.clone()
        };
        assert_eq!(free.implied_units_per_seat(3), None);
        let negative = DecisionTiming {
            ns_per_unit: -4,
            ..timing
        };
        assert_eq!(negative.implied_units_per_seat(3), None);
    }

    #[test]
    fn the_notice_is_word_for_word() {
        let timing = DecisionTiming {
            seats: 3,
            units_per_seat: 128,
            ticks: 1_200,
            windows: 239,
            decision_ns: 412_345,
            ordinary_ns: 200_000,
            excess_ns: 212_345,
            ns_per_unit: 552,
        };
        assert_eq!(
            notice(&timing, "linux"),
            "::notice title=decision tick per size unit::552 ns per size unit per decision tick \
             on linux (3 seats x 128 units sealed; a decision tick 412.345 us against an \
             ordinary tick 200.000 us, mean excess 212345 ns over 239 windows of 1200 ticks, \
             fastest run per tick); a whole 50 ms tick with 3 seats would hold 30193 units per \
             seat, a ceiling and not a budget"
        );
    }

    #[test]
    fn the_hosted_playbook_is_the_budget_case_named_by_role() {
        let root = crate::workspace_root();
        let text = hosted_budget_playbook(&root).expect("the hosted playbook");
        let hosted: Playbook = json::decode(&text).expect("it decodes");
        let committed: Playbook = json::decode(
            &std::fs::read_to_string(cases_dir(&root).join("budget_128.json")).expect("read"),
        )
        .expect("the case decodes");
        assert_eq!(
            pharmakos_verifier::size_units(&hosted),
            pharmakos_verifier::size_units(&committed),
            "naming a target by role must not change the size"
        );
        assert!(!text.contains("beacon_id"), "{text}");
        // Everything else is the committed case: put the ids back and compare.
        let mut restored = hosted;
        let mut put_back = |entry: &mut Step| {
            if let Some(step::Kind::Move(movement)) = entry.kind.as_mut()
                && let Some(Location {
                    place: Some(location::Place::BeaconAnchor(anchor)),
                }) = movement.to.as_mut()
                && matches!(anchor.r#ref, Some(beacon_ref::Ref::Safest(_)))
            {
                anchor.r#ref = Some(beacon_ref::Ref::BeaconId("b_01".to_owned()));
            }
        };
        if let Some(declarative) = restored.declarative.as_mut() {
            declarative.route.iter_mut().for_each(&mut put_back);
            for handler in &mut declarative.handlers {
                handler.body.iter_mut().for_each(&mut put_back);
            }
        }
        assert_eq!(restored, committed);
    }
}
