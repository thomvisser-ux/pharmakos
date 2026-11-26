// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every certifying command's lines (S3's plan, sections 5.1 and 5.2; items
//! 158 (2) and 162 (1); `cert3`), each assembled here and stamped from what
//! ran, so a test that is not ignored can run every line of a cut or unasked
//! run of each through the one predicate ([`provenance::is_certifying`]):
//! [`p1_lines`], [`p3_lines`], [`kinds_lines`], [`watch_lines`] and
//! [`raids_lines`]. The commands themselves only measure and print.
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
use crate::g3::segment::SegmentReport;
use crate::g3::watch::{self, WatchReport};
use crate::kinds::{self, KindFigure, KindPlan};
use crate::p3::{self, P3Report};
use crate::provenance::{self, Sampling};
use crate::quick::{self, Fixture, QuickPlan};
use crate::shapes::{self, SIZES, Shape};
use crate::stats::millis;

/// E12's seed rule: `BENCH_SEED` stays unless P1 FULL's report shows a second
/// seed's p99 more than this many per cent above it (S3's plan, E12).
pub const E12_PERCENT: u128 = 20;

/// E12's own words for a second seed past its rule (S3's plan, E12), which
/// the verdict prints as they stand.
pub const E12_PAST_WORDS: &str = "`cert3` adds that seed and reports both";

/// The hosted FULL p99 on one seed: each shape's end-to-end FULL p99 at the
/// committed budget, and the shapes that were not sent.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SeedFull {
    /// The map seed.
    pub seed: u64,
    /// The FULL p99 of every shape sent, in [`Shape::ALL`]'s order.
    pub sent: Vec<(Shape, u128)>,
    /// The shapes whose request one message cannot carry.
    pub not_sent: Vec<Shape>,
}

/// Each shape's hosted FULL p99 in `figures`, and the shapes not sent.
fn seed_full(seed: u64, figures: &[(Shape, EndToEnd)]) -> SeedFull {
    let mut sent: Vec<(Shape, u128)> = Vec::new();
    let mut not_sent: Vec<Shape> = Vec::new();
    for (shape, figure) in figures {
        match figure {
            EndToEnd::Measured { full, .. } => sent.push((*shape, full.p99_ns)),
            EndToEnd::OverCap { .. } => not_sent.push(*shape),
        }
    }
    SeedFull {
        seed,
        sent,
        not_sent,
    }
}

/// The shapes' names, or "none".
fn names(shapes: &[Shape]) -> String {
    if shapes.is_empty() {
        "none".to_owned()
    } else {
        shapes
            .iter()
            .map(|shape| shape.name())
            .collect::<Vec<&str>>()
            .join(", ")
    }
}

/// E12's verdict on the hosted FULL at the two seeds, with both figures: the
/// worst p99 of each seed over the shapes **both** seeds sent, so the two
/// figures compare one set. Each seed's unsent shapes are named, and so are
/// the shapes left out of the comparison because one seed sent them and the
/// other did not. Pure; compared in integers,
/// `second * 100 > first * (100 + E12_PERCENT)`.
///
/// # Errors
///
/// [`Error::Empty`] when no shape was sent on both seeds, which leaves no
/// p99 to compare, or the first seed's worst is zero.
pub fn seed_rule_notice(first: &SeedFull, second: &SeedFull, os: &str) -> Result<String, Error> {
    let p99_of = |seed: &SeedFull, shape: Shape| {
        seed.sent
            .iter()
            .find(|(sent, _)| *sent == shape)
            .map(|(_, p99)| *p99)
    };
    let mut both: Vec<(Shape, u128, u128)> = Vec::new();
    let mut left_out: Vec<Shape> = Vec::new();
    for shape in Shape::ALL {
        match (p99_of(first, shape), p99_of(second, shape)) {
            (Some(one), Some(two)) => both.push((shape, one, two)),
            (None, None) => {}
            _ => left_out.push(shape),
        }
    }
    let worst = |pick: fn(&(Shape, u128, u128)) -> u128| {
        both.iter()
            .fold(None, |kept: Option<(Shape, u128)>, figure| match kept {
                Some((_, p99)) if p99 >= pick(figure) => kept,
                _ => Some((figure.0, pick(figure))),
            })
            .ok_or_else(|| {
                Error::Empty(format!(
                    "seeds 0x{:016x} and 0x{:016x} sent no shape end to end in common, so E12 \
                     has no FULL p99 to compare",
                    first.seed, second.seed
                ))
            })
    };
    let (first_shape, one) = worst(|figure| figure.1)?;
    let (second_shape, two) = worst(|figure| figure.2)?;
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
        format!("PAST E12's rule: {E12_PAST_WORDS}")
    } else {
        "within E12's rule: BENCH_SEED stays".to_owned()
    };
    let compared = if left_out.is_empty() {
        "every shape either seed sent".to_owned()
    } else {
        format!(
            "the shapes both seeds sent, leaving out {} (sent on one seed only)",
            names(&left_out)
        )
    };
    Ok(format!(
        "::notice title=P1 E12 seed rule::on {os}, the hosted FULL (verify_plan at FULL end to \
         end, the worst p99 over {compared}, at the committed budget): 0x{:016x} {} ({}), \
         0x{:016x} {} ({}), the second {}.{}% of the first; {verdict} (the rule: more than \
         {E12_PERCENT}% above the first); not sent: 0x{:016x} {}, 0x{:016x} {}",
        first.seed,
        millis(one),
        first_shape.name(),
        second.seed,
        millis(two),
        second_shape.name(),
        per_mille.div_euclid(10),
        per_mille.rem_euclid(10),
        first.seed,
        names(&first.not_sent),
        second.seed,
        names(&second.not_sent),
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

/// Every line P1's certifying run prints, stamped with how it was sampled:
/// whether it was asked to certify (`certify`) and the calls that actually
/// ran a figure, read back from the measurements, never from `plan`.
///
/// # Errors
///
/// As the measurements; the commit must be named ([`provenance::commit`]);
/// [`Error::Drift`] when the measurements ran different call counts.
pub fn p1_lines(
    root: &Path,
    plan: QuickPlan,
    certify: provenance::Certify,
    os: &str,
) -> Result<Vec<String>, Error> {
    let planned = Sampling::calls(plan, certify);
    let role = planned.role();
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
    lines.extend(quick::notices(&quick_timing, role, os));
    lines.push(full::cases_notice(&full_timing, role, os));
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
            .map(|shape| full::shape_notice(&curve, shape, role, os)),
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
    // The count each figure rests on, as it ran: every measurement must have
    // run the same calls, or the stamp would speak for one of them only.
    let ran = [
        quick_timing.calls_per_case,
        full_timing.calls_per_case,
        curve.plan.calls,
    ];
    if ran
        .iter()
        .any(|calls| *calls != quick_timing.calls_per_case)
    {
        return Err(Error::Drift(format!(
            "P1's measurements ran {ran:?} calls a figure, not one count"
        )));
    }
    let sampling = Sampling::calls(
        QuickPlan {
            warmup: plan.warmup,
            calls: quick_timing.calls_per_case,
        },
        certify,
    );
    Ok(lines
        .iter()
        .map(|line| provenance::stamped(line, &sampling))
        .collect())
}

/// Every line, stamped with one sampling.
fn stamp_all(lines: &[String], sampling: &Sampling) -> Vec<String> {
    lines
        .iter()
        .map(|line| provenance::stamped(line, sampling))
        .collect()
}

/// Every line P3's certifying command prints for `report`: the commit, a
/// notice a seed and the summary, each stamped with the seeds and the pairs
/// that ran ([`p3::sampling`]).
///
/// # Errors
///
/// As [`provenance::commit`] and [`p3::summary_notice`].
pub fn p3_lines(
    root: &Path,
    report: &P3Report,
    certify: provenance::Certify,
    os: &str,
) -> Result<Vec<String>, Error> {
    let sampling = p3::sampling(report, certify)?;
    let mut lines: Vec<String> = vec![provenance::notice(
        "P3",
        &provenance::commit(root)?,
        &format!("{} seeds x {} pairs", report.seeds.len(), report.pairs),
        os,
    )];
    lines.extend(
        report
            .seeds
            .iter()
            .map(|seed| p3::seed_notice(report, seed, os)),
    );
    lines.push(p3::summary_notice(report, sampling.role(), os)?);
    Ok(stamp_all(&lines, &sampling))
}

/// Every line the decision-tick kinds command prints: the commit, each
/// seed's kinds and their agreement, and the seed gap, each stamped with the
/// runs and the ticks every kind's match played ([`kinds::sampling`]).
///
/// # Errors
///
/// As the notices; [`Error::Empty`] unless there are exactly two seeds.
pub fn kinds_lines(
    root: &Path,
    seeds: &[(u64, (Vec<KindFigure>, u128))],
    plan: KindPlan,
    certify: provenance::Certify,
    os: &str,
) -> Result<Vec<String>, Error> {
    let every: Vec<KindFigure> = seeds
        .iter()
        .flat_map(|(_, (figures, _))| figures.iter().cloned())
        .collect();
    let sampling = kinds::sampling(&every, certify)?;
    let mut lines: Vec<String> = vec![provenance::notice(
        "decision tick by kind",
        &provenance::commit(root)?,
        &format!(
            "every kind's match {} times over a {} s segment",
            plan.runs,
            plan.segment_ms.div_euclid(1_000)
        ),
        os,
    )];
    for (seed, (figures, resolution)) in seeds {
        lines.push(kinds::kinds_notice(figures, *resolution, *seed, plan, os));
        lines.push(kinds::agreement_notice(
            &kinds::agreement(figures)?,
            *seed,
            os,
        )?);
    }
    match seeds {
        [(_, (first, _)), (_, (second, _))] => {
            lines.push(kinds::seed_gap_notice(first, second, os)?);
        }
        _ => {
            return Err(Error::Empty(
                "the kinds are measured on two seeds".to_owned(),
            ));
        }
    }
    Ok(stamp_all(&lines, &sampling))
}

/// Every line the watch command prints: the commit, the segment's provenance,
/// the view feed's bytes, the recompute, the sightings and the breach search,
/// each stamped with the ticks the segment played.
///
/// # Errors
///
/// As [`provenance::commit`] and [`crate::g3::ticks_sampling`].
pub fn watch_lines(
    root: &Path,
    report: &WatchReport,
    certify: provenance::Certify,
    os: &str,
) -> Result<Vec<String>, Error> {
    let sampling = crate::g3::ticks_sampling(report.ticks, certify)?;
    let lines = [
        provenance::notice("G3' watch", &provenance::commit(root)?, "one segment", os),
        watch::provenance_notice(report, os),
        watch::view_notice(report, os),
        watch::recompute_notice(report, os),
        watch::sightings_notice(report, os),
        watch::breach_notice(report, os),
    ];
    Ok(stamp_all(&lines, &sampling))
}

/// Every line the raids command prints: the commit, stamped from the fewest
/// ticks any segment played, then each segment's lines, stamped from its own.
///
/// # Errors
///
/// [`Error::Empty`] for no segment; as the notices otherwise.
pub fn raids_lines(
    root: &Path,
    reports: &[SegmentReport],
    certify: provenance::Certify,
    os: &str,
) -> Result<Vec<String>, Error> {
    let fewest = reports
        .iter()
        .map(|report| report.ticks)
        .min()
        .ok_or_else(|| Error::Empty("no G3' segment was played".to_owned()))?;
    let mut lines = vec![provenance::stamped(
        &provenance::notice(
            "G3' with raids",
            &provenance::commit(root)?,
            "one segment a load",
            os,
        ),
        &crate::g3::ticks_sampling(fewest, certify)?,
    )];
    for report in reports {
        let sampling = crate::g3::sampling(report, certify)?;
        let segment = [
            crate::g3::provenance_notice(report, os),
            crate::g3::tick_notice(report, sampling.role(), os)?,
            crate::g3::tick_verdict(report, crate::g3::P99_BAR_NS, crate::g3::MEAN_BAR_NS, os),
            crate::g3::phases_notice(report, os)?,
            crate::g3::repaths_notice(report, os)?,
            crate::g3::combat_notice(report, os)?,
        ];
        lines.extend(stamp_all(&segment, &sampling));
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::Summary;

    /// B3: QUICK and FULL carry different p99s, so a rule that read QUICK
    /// would read a tenth of the figure and be told apart.
    fn measured(p99_ns: u128) -> EndToEnd {
        let summary = |p99_ns: u128| Summary {
            p50_ns: 1,
            p99_ns,
            mean_ns: 1,
            max_ns: p99_ns,
            samples: 1,
        };
        EndToEnd::Measured {
            quick: summary(p99_ns.div_euclid(10)),
            full: summary(p99_ns),
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
        assert_eq!(
            first.sent,
            vec![
                (Shape::Handlers24, 20_000_000),
                (Shape::RouteGuards24, 10_000_000)
            ],
            "the FULL p99, not QUICK's"
        );
        let at = |p99: u128| {
            seed_full(
                2,
                &[
                    (Shape::Handlers24, measured(p99)),
                    (Shape::RouteGuards24, measured(1)),
                ],
            )
        };
        let within = seed_rule_notice(&first, &at(24_000_000), "linux").expect("a notice");
        assert!(
            within.contains(
                "the worst p99 over every shape either seed sent, at the committed budget): \
                 0x0000000000000001 20.000 ms (handlers_24_node), 0x0000000000000002 24.000 ms \
                 (handlers_24_node), the second 120.0% of the first; within E12's rule: \
                 BENCH_SEED stays (the rule: more than 20% above the first); not sent: \
                 0x0000000000000001 none, 0x0000000000000002 none"
            ),
            "{within}"
        );
        let past = seed_rule_notice(&first, &at(24_000_001), "linux").expect("a notice");
        assert!(
            past.contains("; PAST E12's rule: `cert3` adds that seed and reports both (the rule"),
            "{past}"
        );
        let none = seed_full(3, &[(Shape::Handlers24, EndToEnd::OverCap { bytes: 1 })]);
        assert_eq!(none.not_sent, vec![Shape::Handlers24]);
        assert!(seed_rule_notice(&first, &none, "linux").is_err());
    }

    /// B2: a shape one seed sent and the other could not leaves the
    /// comparison by name, and each seed's unsent shapes are named.
    #[test]
    fn e12_compares_only_the_shapes_both_seeds_sent() {
        let first = seed_full(
            1,
            &[
                (Shape::Handlers24, measured(10_000_000)),
                (Shape::RouteGuards24, measured(90_000_000)),
            ],
        );
        let second = seed_full(
            2,
            &[
                (Shape::Handlers24, measured(11_000_000)),
                (Shape::RouteGuards24, EndToEnd::OverCap { bytes: 1 }),
            ],
        );
        let line = seed_rule_notice(&first, &second, "linux").expect("a notice");
        // Compared over handlers_24 alone: 11 against 10, within; the first
        // seed's 90 ms on the shape the second never sent is not its worst.
        assert!(
            line.contains(
                "the worst p99 over the shapes both seeds sent, leaving out route_guards_24_node \
                 (sent on one seed only), at the committed budget): 0x0000000000000001 \
                 10.000 ms (handlers_24_node), 0x0000000000000002 11.000 ms (handlers_24_node), the \
                 second 110.0% of the first; within E12's rule"
            ),
            "{line}"
        );
        assert!(
            line.ends_with(
                "not sent: 0x0000000000000001 none, 0x0000000000000002 route_guards_24_node"
            ),
            "{line}"
        );
    }

    /// F5: the PAST verdict prints E12's own words, as S3's plan has them.
    #[test]
    fn the_past_verdict_is_e12s_own_words() {
        let plan =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/design/s3-plan.md");
        let text = std::fs::read_to_string(&plan).expect("S3's plan");
        let e12 = text
            .lines()
            .skip_while(|line| !line.starts_with("- **E12."))
            .take_while(|line| line.starts_with("- **E12.") || line.starts_with("  "))
            .collect::<Vec<&str>>()
            .iter()
            .map(|line| line.trim())
            .collect::<Vec<&str>>()
            .join(" ");
        assert!(e12.contains(E12_PAST_WORDS), "E12 reads: {e12}");
    }
}
