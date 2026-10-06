// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! QUICK's wall-clock time over the committed verifier cases (P1, S1's half).
//!
//! Spec section 16's P1 row asks for "QUICK <=5 ms p99"; S1's plan section 5
//! item 1 says over what: "the committed verifier cases and `budget_128`". So
//! the inputs here are exactly the verifier goldens' inputs -- every playbook
//! under `crates/verifier/tests/cases/`, each against the **one** fixture
//! `crates/verifier/tests/verifier.rs` writes out (`fixture_scope`,
//! `fixture_snapshot`, and for the cases its `MAP_CASES`, `SHORT_CASES` and
//! `SEGMENT_CASES` name, `map_scope`, `short_scope` and `segment_snapshot`)
//! and `tests/golden/verifier/README.md` tabulates. That fixture lives in an
//! integration test, which no other crate can import, so [`fixture_scope`],
//! [`map_scope`], [`MAP_CASES`], [`fixture_snapshot`], [`short_scope`],
//! [`SHORT_CASES`], [`segment_snapshot`] and [`SEGMENT_CASES`] restate it, and
//! `tests/harness.rs` (`every_case_under_the_bench_fixture_is_its_committed_report`)
//! holds the restatement to the committed reports: every
//! case's report under this fixture must equal its `expected.report.json` byte
//! for byte. A fixture that drifted from the verifier's would time inputs no
//! golden describes, and that test is what turns the drift red.
//!
//! # What one timed call is
//!
//! [`Input::new`] then [`verify`] at [`Depth::Quick`], which is what every
//! QUICK call in the gateway runs (`plan-core`'s `verify_jsonc` builds a fresh
//! `Input` each time, and `Input::new` computes `rules_hash` and reads the
//! limits, the interface rates, the prices and the walking rows). `plan-core`'s canonicalisation in front of it and the JSON-RPC
//! framing around it are not timed: the figure is the verifier's.
//!
//! Each call is timed **once**, never best-of-N: P1's bar is a p99 over calls,
//! and a player waiting on the editor waits on a single call, preemption
//! included. That makes the figure noisier on a shared runner, which is one
//! reason it is a notice and never a gate (decisions-log item 128 (3) (b)).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use pharmakos_proto::gp::api::v1::VerifyReport;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::gp::v1::beacon_filter::MandateKind;
use pharmakos_proto::gp::v1::by_richness::Richness;
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::math::quantity::{Kw, Money};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::snapshot::{SNAPSHOT_VERSION, Snapshot};
use pharmakos_sim::tables::SeatId;
use pharmakos_verifier::{FeatureKind, Input, KnownBeacon, KnownFeature, Ownership, Scope, verify};

use crate::stats::{Summary, micros};

/// The case that sits exactly on the size budget, published on its own as well
/// as in the pool (S1's plan section 5 item 1).
pub const BUDGET_CASE: &str = "budget_128";

/// One committed verifier case: its name and its exact bytes.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Case {
    /// The file stem, which is also its golden directory's name.
    pub name: String,
    /// The playbook bytes, exactly as committed.
    pub bytes: Vec<u8>,
}

/// Where the committed cases live, under the workspace root.
#[must_use]
pub fn cases_dir(root: &Path) -> PathBuf {
    root.join("crates")
        .join("verifier")
        .join("tests")
        .join("cases")
}

/// Every committed case, in name order.
///
/// # Errors
///
/// When the directory cannot be read, holds no case, or holds no
/// [`BUDGET_CASE`]: a measurement over fewer inputs than it names is not one.
pub fn committed_cases(root: &Path) -> Result<Vec<Case>, String> {
    let dir = cases_dir(root);
    let entries =
        fs::read_dir(&dir).map_err(|error| format!("reading {}: {error}", dir.display()))?;
    let mut cases: Vec<Case> = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|error| format!("reading {}: {error}", dir.display()))?
            .path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let bytes =
            fs::read(&path).map_err(|error| format!("reading {}: {error}", path.display()))?;
        cases.push(Case {
            name: name.to_owned(),
            bytes,
        });
    }
    cases.sort_by(|left, right| left.name.cmp(&right.name));
    if cases.is_empty() {
        return Err(format!("{} holds no case", dir.display()));
    }
    if !cases.iter().any(|case| case.name == BUDGET_CASE) {
        return Err(format!("{} holds no {BUDGET_CASE}.json", dir.display()));
    }
    Ok(cases)
}

/// The rules table every case is verified against: the committed one.
///
/// # Errors
///
/// When `rules/rules.v1.json` cannot be read or decoded.
pub fn committed_rules(root: &Path) -> Result<RulesTable, String> {
    let path = root.join("rules").join("rules.v1.json");
    RulesTable::load(&path).map_err(|error| format!("reading {}: {error}", path.display()))
}

/// The seat's frozen planning snapshot, as `crates/verifier/tests/verifier.rs`
/// writes it in `fixture_snapshot`.
///
/// # Errors
///
/// When the snapshot will not encode.
pub fn fixture_snapshot() -> Result<Vec<u8>, String> {
    Snapshot {
        version: SNAPSHOT_VERSION,
        match_seed: 0x0102_0304_0506_0708,
        tick: 3_600,
        ..Snapshot::default()
    }
    .to_bytes()
    .map_err(|error| format!("encoding the fixture snapshot: {error}"))
}

/// The seat's frozen snapshot with a coming segment in it, as
/// `crates/verifier/tests/verifier.rs` writes it in `segment_snapshot`.
///
/// # Errors
///
/// When the snapshot will not encode.
pub fn segment_snapshot() -> Result<Vec<u8>, String> {
    Snapshot {
        version: SNAPSHOT_VERSION,
        match_seed: 0x0102_0304_0506_0708,
        tick: 3_600,
        coming_segment_ms: 180_000,
        ..Snapshot::default()
    }
    .to_bytes()
    .map_err(|error| format!("encoding the segment snapshot: {error}"))
}

/// The cases checked against [`segment_snapshot`], as
/// `crates/verifier/tests/verifier.rs` lists them in `SEGMENT_CASES`.
pub const SEGMENT_CASES: &[&str] = &["w0701_a_route_longer_than_the_segment"];

fn beacon(
    id: &str,
    side: Ownership,
    mandate: MandateKind,
    at: (i32, i32, i32),
    is_core: bool,
    tags: &[&str],
) -> KnownBeacon {
    KnownBeacon {
        beacon_id: id.to_owned(),
        owner: if side == Ownership::EnemyKnown {
            SeatId::new(1)
        } else {
            SeatId::new(0)
        },
        side,
        mandate,
        tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
        at: Voxel {
            x: at.0,
            y: at.1,
            z: at.2,
        },
        is_core,
    }
}

/// The seat's view every case is checked against, as
/// `crates/verifier/tests/verifier.rs` writes it in `fixture_scope`.
#[must_use]
pub fn fixture_scope() -> Scope {
    Scope::new(
        SeatId::new(0),
        SeatEconomy {
            treasury: Money::new(200),
            supply: Kw::new(10),
            draw: Kw::new(4),
        },
    )
    .with_beacon(beacon(
        "b_01",
        Ownership::Own,
        MandateKind::Build,
        (80, 11, 55),
        true,
        &[],
    ))
    .with_beacon(beacon(
        "b_02",
        Ownership::Own,
        MandateKind::Mine,
        (100, 20, 58),
        false,
        &["east"],
    ))
    .with_beacon(beacon(
        "e_01",
        Ownership::EnemyKnown,
        MandateKind::Unspecified,
        (300, 300, 40),
        false,
        &[],
    ))
}

/// [`fixture_scope`] with a grid that is already short, as
/// `crates/verifier/tests/verifier.rs` writes it in `short_scope`.
#[must_use]
pub fn short_scope() -> Scope {
    let plain = fixture_scope();
    let mut short = Scope::new(
        plain.seat(),
        SeatEconomy {
            treasury: plain.economy().treasury,
            supply: Kw::new(4),
            draw: Kw::new(10),
        },
    );
    for known in plain.beacons() {
        short.push_beacon(known.clone());
    }
    short
}

/// The cases checked against [`short_scope`], as
/// `crates/verifier/tests/verifier.rs` lists them in `SHORT_CASES`.
pub const SHORT_CASES: &[&str] = &["w0601_an_existing_shortfall"];

/// The cases checked against [`map_scope`], as
/// `crates/verifier/tests/verifier.rs` lists them in `MAP_CASES`: every case
/// that names or describes a feature, and the one whose lint reads the map
/// around a fixed site.
pub const MAP_CASES: &[&str] = &[
    "cover_the_nearest_vent",
    "e0111_covering_without_coverage",
    "e0111_pick_without_a_rank",
    "e0407_covering_outside_a_place_step",
    "e0408_on_outside_a_build_target",
    "e0409_covered_outside_a_covering_deploy",
    "e0410_removing_by_a_description",
    "e0411_coverage_under_on",
    "e0412_unknown_feature",
    "e0413_a_seam_under_on",
    "w0704_a_lost_feature",
    "w0705_an_earlier_step_places_a_beacon",
    "w0706_a_fixed_site_by_a_covered_vent",
];

fn feature(
    id: &str,
    kind: FeatureKind,
    grade: Richness,
    at: (i32, i32),
    live: bool,
    covered_by: Option<&str>,
) -> KnownFeature {
    KnownFeature {
        feature_id: id.to_owned(),
        kind,
        grade,
        x: at.0,
        y: at.1,
        live,
        covered_by: covered_by.map(str::to_owned),
    }
}

/// [`fixture_scope`] with the map's features and the commander's position, as
/// `crates/verifier/tests/verifier.rs` writes it in `map_scope`.
#[must_use]
pub fn map_scope() -> Scope {
    fixture_scope()
        .with_feature(feature(
            "seam_104_24",
            FeatureKind::Seam,
            Richness::Standard,
            (104, 24),
            true,
            Some("b_02"),
        ))
        .with_feature(feature(
            "seam_200_60",
            FeatureKind::Seam,
            Richness::Rich,
            (200, 60),
            true,
            None,
        ))
        .with_feature(feature(
            "vent_150_25",
            FeatureKind::Vent,
            Richness::Standard,
            (150, 25),
            true,
            None,
        ))
        .with_feature(feature(
            "vent_40_40",
            FeatureKind::Vent,
            Richness::Lean,
            (40, 40),
            false,
            None,
        ))
        .with_feature(feature(
            "vent_90_20",
            FeatureKind::Vent,
            Richness::Lean,
            (90, 20),
            true,
            Some("b_01"),
        ))
        .with_commander(Voxel {
            x: 82,
            y: 13,
            z: 55,
        })
}

/// Everything one QUICK call is a function of, apart from the playbook.
#[derive(Debug)]
pub struct Fixture {
    rules: RulesTable,
    scope: Scope,
    map_scope: Scope,
    short_scope: Scope,
    snapshot: Vec<u8>,
    segment_snapshot: Vec<u8>,
}

impl Fixture {
    /// The committed rules table, [`fixture_scope`], [`map_scope`],
    /// [`short_scope`], [`fixture_snapshot`] and [`segment_snapshot`].
    ///
    /// # Errors
    ///
    /// As [`committed_rules`], [`fixture_snapshot`] and [`segment_snapshot`].
    pub fn committed(root: &Path) -> Result<Fixture, String> {
        Ok(Fixture {
            rules: committed_rules(root)?,
            scope: fixture_scope(),
            map_scope: map_scope(),
            short_scope: short_scope(),
            snapshot: fixture_snapshot()?,
            segment_snapshot: segment_snapshot()?,
        })
    }

    /// The view a case is checked against: [`map_scope`] for the cases
    /// [`MAP_CASES`] names, [`short_scope`] for the cases [`SHORT_CASES`]
    /// names, [`fixture_scope`] for every other.
    #[must_use]
    pub fn scope_for(&self, case: &Case) -> &Scope {
        if MAP_CASES.contains(&case.name.as_str()) {
            &self.map_scope
        } else if SHORT_CASES.contains(&case.name.as_str()) {
            &self.short_scope
        } else {
            &self.scope
        }
    }

    /// The snapshot a case is checked against: [`segment_snapshot`] for the
    /// cases [`SEGMENT_CASES`] names, [`fixture_snapshot`] for every other.
    #[must_use]
    pub fn snapshot_for(&self, case: &Case) -> &[u8] {
        if SEGMENT_CASES.contains(&case.name.as_str()) {
            &self.segment_snapshot
        } else {
            &self.snapshot
        }
    }

    /// One report, as the verifier goldens are made: `Input::new` then
    /// `verify` at `depth`, against the case's view.
    ///
    /// # Errors
    ///
    /// When the rules table lacks a block the checks read.
    pub fn report(&self, case: &Case, depth: Depth) -> Result<VerifyReport, String> {
        let scope = self.scope_for(case);
        let input = Input::new(&case.bytes, self.snapshot_for(case), scope, &self.rules)
            .map_err(|gap| format!("assembling the verifier's input: {gap}"))?;
        Ok(verify(&input, depth))
    }

    /// One timed QUICK call, in nanoseconds.
    ///
    /// # Errors
    ///
    /// As [`Fixture::report`]; the error is checked after the clock stops.
    pub fn time_quick(&self, case: &Case) -> Result<u128, String> {
        let started = Instant::now();
        let report = self.report(case, Depth::Quick);
        let elapsed = started.elapsed().as_nanos();
        let report = report?;
        // Read after the clock stops, so the optimiser cannot drop the call.
        std::hint::black_box(&report);
        Ok(elapsed)
    }
}

/// How many calls of each case are timed, and how many are thrown away first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QuickPlan {
    /// Untimed calls per case before the sample, so the sample measures the
    /// verifier rather than the first touch of its code and data.
    pub warmup: usize,
    /// Timed calls per case.
    pub calls: usize,
}

/// What [`measure`] found.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct QuickTiming {
    /// How many committed cases were timed.
    pub cases: usize,
    /// Timed calls per case.
    pub calls_per_case: usize,
    /// Every timed call of every case, pooled.
    pub pooled: Summary,
    /// [`BUDGET_CASE`]'s calls alone.
    pub budget: Summary,
    /// The case whose own p99 was the highest, and that p99.
    pub slowest: (String, u128),
}

/// Time QUICK over `cases`, each `plan.calls` times after `plan.warmup`
/// untimed calls, case after case in name order.
///
/// # Errors
///
/// When `cases` holds no [`BUDGET_CASE`], `plan.calls` is zero, or a call
/// cannot assemble its input.
pub fn measure(fixture: &Fixture, cases: &[Case], plan: QuickPlan) -> Result<QuickTiming, String> {
    if plan.calls == 0 {
        return Err("a QUICK measurement of zero calls measures nothing".to_owned());
    }
    let mut pooled: Vec<u128> = Vec::with_capacity(cases.len().saturating_mul(plan.calls));
    let mut budget: Option<Summary> = None;
    let mut slowest: (String, u128) = (String::new(), 0);
    for case in cases {
        for _ in 0..plan.warmup {
            fixture.time_quick(case)?;
        }
        let mut own: Vec<u128> = Vec::with_capacity(plan.calls);
        for _ in 0..plan.calls {
            own.push(fixture.time_quick(case)?);
        }
        pooled.extend_from_slice(&own);
        let summary = Summary::of(own);
        if summary.p99_ns > slowest.1 || slowest.0.is_empty() {
            slowest = (case.name.clone(), summary.p99_ns);
        }
        if case.name == BUDGET_CASE {
            budget = Some(summary);
        }
    }
    let budget = budget.ok_or_else(|| format!("the cases hold no {BUDGET_CASE}"))?;
    Ok(QuickTiming {
        cases: cases.len(),
        calls_per_case: plan.calls,
        pooled: Summary::of(pooled),
        budget,
        slowest,
    })
}

/// The notices for one QUICK measurement, word for word. Pure, so the text is
/// pinned by a unit test.
#[must_use]
pub fn notices(timing: &QuickTiming, os: &str) -> Vec<String> {
    vec![
        format!(
            "::notice title=QUICK p99::{} p99, {} p50 on {os}, over {} committed verifier cases \
             x {} calls each, timed singly (Input::new then verify at QUICK); spec section 16's \
             P1 bar is 5 ms p99, published here and never gated",
            micros(timing.pooled.p99_ns),
            micros(timing.pooled.p50_ns),
            timing.cases,
            timing.calls_per_case,
        ),
        format!(
            "::notice title=QUICK p99 at the size budget::{BUDGET_CASE}: {} p99, {} p50 on {os} \
             over {} calls; slowest case by p99: {} at {}",
            micros(timing.budget.p99_ns),
            micros(timing.budget.p50_ns),
            timing.budget.samples,
            timing.slowest.0,
            micros(timing.slowest.1),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notices_are_word_for_word() {
        let timing = QuickTiming {
            cases: 48,
            calls_per_case: 200,
            pooled: Summary {
                p50_ns: 41_000,
                p99_ns: 1_250_500,
                samples: 9_600,
            },
            budget: Summary {
                p50_ns: 300_000,
                p99_ns: 400_001,
                samples: 200,
            },
            slowest: ("budget_128".to_owned(), 400_001),
        };
        assert_eq!(
            notices(&timing, "windows"),
            vec![
                "::notice title=QUICK p99::1250.500 us p99, 41.000 us p50 on windows, over 48 \
                 committed verifier cases x 200 calls each, timed singly (Input::new then verify \
                 at QUICK); spec section 16's P1 bar is 5 ms p99, published here and never gated"
                    .to_owned(),
                "::notice title=QUICK p99 at the size budget::budget_128: 400.001 us p99, \
                 300.000 us p50 on windows over 200 calls; slowest case by p99: budget_128 at \
                 400.001 us"
                    .to_owned(),
            ]
        );
    }

    #[test]
    fn zero_calls_is_refused() {
        let root = crate::workspace_root();
        let fixture = Fixture::committed(&root).expect("the committed fixture");
        let cases = committed_cases(&root).expect("the committed cases");
        assert!(
            measure(
                &fixture,
                &cases,
                QuickPlan {
                    warmup: 0,
                    calls: 0
                }
            )
            .is_err()
        );
    }
}
