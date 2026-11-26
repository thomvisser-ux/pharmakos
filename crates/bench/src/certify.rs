// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! P1's certifying run, every line it prints (S3's plan, section 5.2; items
//! 158 (2) and 162 (1); `cert3`).
//!
//! [`p1_lines`] measures everything P1 certifies in one run and answers the
//! lines `tests/perf_alarm.rs`'s `p1_certify` prints, each stamped with how
//! it was sampled ([`provenance::stamped`]): a line copied alone into a record
//! carries its call count, and a cut run's carries
//! [`provenance::CUT_BANNER`]. The pieces, in order: the commit; QUICK's and
//! FULL's notices over the committed cases and their verdict; every shape at
//! every size of the curve; a verdict for each shape at the committed budget;
//! E12's seed rule on the hosted FULL ([`seed_rule_notice`]); and QUICK's
//! cost at the transport cap.

use std::path::Path;

use pharmakos_proto::gp::api::v1::verify_plan::Depth;

use crate::error::Error;
use crate::full::{self, Bars, EndToEnd};
use crate::provenance::{self, Sampling};
use crate::quick::{self, Fixture, QuickPlan};
use crate::shapes::{self, SIZES, Shape};
use crate::stats::millis;

/// E12's seed rule: `BENCH_SEED` stays unless P1 FULL's report shows a second
/// seed's p99 more than this many per cent above it (S3's plan, E12).
pub const E12_PERCENT: u128 = 20;

/// The hosted FULL p99 on one seed: the worst end-to-end FULL p99 over the
/// shapes at the committed budget, and the shape, or the shapes that were
/// not sent.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SeedFull {
    /// The map seed.
    pub seed: u64,
    /// The worst FULL p99 over the shapes sent, and its shape.
    pub worst: Option<(Shape, u128)>,
    /// The shapes whose request one message cannot carry.
    pub not_sent: Vec<Shape>,
}

/// The worst hosted FULL p99 over `figures`.
fn seed_full(seed: u64, figures: &[(Shape, EndToEnd)]) -> SeedFull {
    let mut worst: Option<(Shape, u128)> = None;
    let mut not_sent: Vec<Shape> = Vec::new();
    for (shape, figure) in figures {
        match figure {
            EndToEnd::Measured { full, .. } => {
                if worst.is_none_or(|(_, kept)| full.p99_ns > kept) {
                    worst = Some((*shape, full.p99_ns));
                }
            }
            EndToEnd::OverCap { .. } => not_sent.push(*shape),
        }
    }
    SeedFull {
        seed,
        worst,
        not_sent,
    }
}

/// E12's verdict on the hosted FULL at the two seeds, with both figures.
/// Pure; compared in integers, `second * 100 > first * (100 + E12_PERCENT)`.
///
/// # Errors
///
/// [`Error::Empty`] when either seed sent no shape, which has no p99 to
/// compare.
pub fn seed_rule_notice(first: &SeedFull, second: &SeedFull, os: &str) -> Result<String, Error> {
    let figure = |seed: &SeedFull| {
        seed.worst.ok_or_else(|| {
            Error::Empty(format!(
                "seed 0x{:016x} sent no shape end to end, so E12 has no FULL p99 to read",
                seed.seed
            ))
        })
    };
    let ((first_shape, one), (second_shape, two)) = (figure(first)?, figure(second)?);
    let past = two.checked_mul(100).ok_or(Error::Overflow("a FULL p99"))?
        > one
            .checked_mul(100 + E12_PERCENT)
            .ok_or(Error::Overflow("a FULL p99"))?;
    let per_mille = two
        .checked_mul(1_000)
        .and_then(|scaled| scaled.checked_div(one))
        .ok_or(Error::Empty(
            "a first seed's FULL p99 of zero has no ratio".to_owned(),
        ))?;
    let verdict = if past {
        "PAST E12's rule: P1 reports both seeds, and BENCH_SEED's keeping is put to the owner"
    } else {
        "within E12's rule: BENCH_SEED stays"
    };
    Ok(format!(
        "::notice title=P1 E12 seed rule::on {os}, the hosted FULL (verify_plan at FULL end to \
         end, the worst p99 over the shapes at the committed budget): 0x{:016x} {} ({}), \
         0x{:016x} {} ({}), the second {}.{}% of the first; {verdict} (the rule: more than \
         {E12_PERCENT}% above the first)",
        first.seed,
        millis(one),
        first_shape.name(),
        second.seed,
        millis(two),
        second_shape.name(),
        per_mille.div_euclid(10),
        per_mille.rem_euclid(10),
    ))
}

/// The end-to-end figure of every shape at the committed budget on `seed`.
fn shapes_on(
    rules: &pharmakos_sim::rules::RulesTable,
    plan: QuickPlan,
    seed: u64,
    units: u32,
) -> Result<Vec<(Shape, EndToEnd)>, Error> {
    let mut figures: Vec<(Shape, EndToEnd)> = Vec::with_capacity(Shape::ALL.len());
    for shape in Shape::ALL {
        figures.push((
            shape,
            full::end_to_end_on(rules, plan, seed, |core| {
                shapes::canonical(&shape.playbook_at(units, core)?, shape.name())
            })?,
        ));
    }
    Ok(figures)
}

/// Every line P1's certifying run prints, stamped with `plan`'s sampling.
///
/// # Errors
///
/// As the measurements; the commit must be named ([`provenance::commit`]).
pub fn p1_lines(root: &Path, plan: QuickPlan, os: &str) -> Result<Vec<String>, Error> {
    let sampling = Sampling::calls(plan);
    let mut lines: Vec<String> = vec![provenance::notice(
        "P1",
        &provenance::commit(root)?,
        &provenance::plan_words(plan),
        os,
    )];
    let fixture = Fixture::committed(root)?;
    let cases = quick::committed_cases(root)?;
    let quick_timing = quick::measure(&fixture, &cases, plan)?;
    let full_timing = quick::measure_at(&fixture, &cases, plan, Depth::Full)?;
    lines.extend(quick::notices(&quick_timing, os));
    lines.push(full::cases_notice(&full_timing, os));
    lines.push(full::cases_verdict(
        &quick_timing,
        &full_timing,
        Bars::P1,
        os,
    ));
    let curve = full::measure_curve(root, &SIZES, plan)?;
    lines.push(full::provenance_notice(&curve, os));
    lines.extend(
        Shape::ALL
            .into_iter()
            .map(|shape| full::shape_notice(&curve, shape, os)),
    );
    lines.extend(full::budget_verdicts(&curve, Bars::P1, os)?);
    let at_budget: Vec<(Shape, EndToEnd)> = curve
        .points
        .iter()
        .find(|point| point.units == curve.budget_units)
        .map(|point| {
            point
                .shapes
                .iter()
                .map(|figure| (figure.shape, figure.end_to_end.clone()))
                .collect()
        })
        .ok_or_else(|| Error::Empty("the curve holds no point at the budget".to_owned()))?;
    let second = shapes_on(
        fixture.rules(),
        plan,
        pharmakos_sim::DETERMINISM_MATCH_SEED,
        curve.budget_units,
    )?;
    lines.push(seed_rule_notice(
        &seed_full(crate::decision::BENCH_SEED, &at_budget),
        &seed_full(pharmakos_sim::DETERMINISM_MATCH_SEED, &second),
        os,
    )?);
    lines.extend(
        full::measure_cap(root, plan)?
            .iter()
            .map(|figure| full::cap_notice(figure, plan, os)),
    );
    Ok(lines
        .iter()
        .map(|line| provenance::stamped(line, &sampling))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::Summary;

    fn measured(p99_ns: u128) -> EndToEnd {
        let summary = Summary {
            p50_ns: 1,
            p99_ns,
            mean_ns: 1,
            max_ns: p99_ns,
            samples: 1,
        };
        EndToEnd::Measured {
            quick: summary,
            full: summary,
            qualifies: true,
        }
    }

    #[test]
    fn e12_reads_the_hosted_full_at_both_seeds() {
        let first = seed_full(
            1,
            &[
                (Shape::Handlers24, measured(20_000_000)),
                (Shape::RouteGuards24, measured(10_000_000)),
            ],
        );
        assert_eq!(first.worst, Some((Shape::Handlers24, 20_000_000)));
        let at = |p99: u128| seed_full(2, &[(Shape::Handlers24, measured(p99))]);
        let within = seed_rule_notice(&first, &at(24_000_000), "linux").expect("a notice");
        assert!(
            within.ends_with(
                "the second 120.0% of the first; within E12's rule: BENCH_SEED stays (the \
                 rule: more than 20% above the first)"
            ),
            "{within}"
        );
        let past = seed_rule_notice(&first, &at(24_000_001), "linux").expect("a notice");
        assert!(past.contains("PAST E12's rule"), "{past}");
        let none = seed_full(3, &[(Shape::Handlers24, EndToEnd::OverCap { bytes: 1 })]);
        assert_eq!(none.not_sent, vec![Shape::Handlers24]);
        assert!(seed_rule_notice(&first, &none, "linux").is_err());
    }
}
