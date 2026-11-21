// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The verifier and the sim's compile agree, held by a table (S3's `vfyq2`,
//! fix pass 2, item C1).
//!
//! "What qualifies at FULL compiles" was held by `fuzz.rs` over one-node
//! edits of two bases. This holds it by construction of the table: every
//! `pharmakos_sim::interpreter::PlanError` variant is classed by `classify`,
//! a `match` with no wildcard, so a variant the sim adds does not compile here
//! until it is classed. A reachable variant names the codes the verifier
//! refuses that shape with; an unreachable one says why no decoded playbook
//! can raise it. Each reachable variant has a text the verifier refuses with
//! one of its codes AND that `Plan::compile`, the verifier bypassed, refuses
//! with that variant. A text the verifier accepted would be a defect in the
//! verifier, fixed there, never a row deleted.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml's allow-expect-in-tests only recognises #[test] functions, not an \
              integration test's helpers. A panic is this file's failure report."
)]

use std::collections::BTreeSet;

use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::gp::v1::Playbook;
use pharmakos_proto::json;
use pharmakos_sim::interpreter::Plan;
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::tables::SeatId;
use pharmakos_verifier::{Input, Scope, verify};

mod plan_error_table;

use plan_error_table::{Row, VARIANTS, classify, every_variant, root, texts, unreachable_samples};

#[test]
fn every_plan_error_the_compile_raises_the_verifier_refuses_first() {
    let rules = RulesTable::load(&root().join("rules").join("rules.v1.json"))
        .expect("the committed rules table");
    let scope = Scope::new(SeatId::new(0), SeatEconomy::default());
    let mut covered: BTreeSet<&'static str> = BTreeSet::new();
    let mut failures = Vec::new();
    for (name, text) in texts() {
        let playbook: Playbook =
            json::decode(&text).unwrap_or_else(|error| panic!("{name}: decodes: {error}"));
        let Err(error) = Plan::compile(&playbook, &rules) else {
            failures.push(format!("{name}: the compile accepts it"));
            continue;
        };
        let (variant, row) = classify(&error);
        covered.insert(variant);
        let codes = match row {
            Row::Refused(codes) => codes,
            Row::Unreachable(why) => {
                failures.push(format!(
                    "{name}: raised {variant}, classed unreachable: {why}"
                ));
                continue;
            }
        };
        let input = Input::new(text.as_bytes(), &[], &scope, &rules)
            .unwrap_or_else(|error| panic!("{name}: the input: {error}"));
        let report = verify(&input, Depth::Full);
        if report.qualifies {
            failures.push(format!(
                "{name}: the verifier qualifies what the compile refuses ({error:?})"
            ));
        }
        if !report
            .diagnostics
            .iter()
            .any(|diagnostic| codes.contains(&diagnostic.code.as_str()))
        {
            failures.push(format!(
                "{name}: {variant} is refused by none of {codes:?}; the report raised {:?}",
                report
                    .diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.code.as_str())
                    .collect::<Vec<_>>()
            ));
        }
    }
    for sample in unreachable_samples() {
        let (variant, row) = classify(&sample);
        assert!(
            matches!(row, Row::Unreachable(_)),
            "{variant} is sampled as unreachable"
        );
        covered.insert(variant);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // The rules column (fix pass 3, item M4): every reachable variant whose
    // limit is a rules value has the verifier and the compile read the same
    // one; the rest read none. Exhaustive by `rules_of`'s match.
    for sample in every_variant() {
        let (variant, _) = classify(&sample);
        match plan_error_table::rules_of(&sample) {
            plan_error_table::Rules::Independent => {}
            plan_error_table::Rules::SameValue(value) => {
                assert!(!value.is_empty(), "{variant} names the value it reads");
            }
        }
    }
    // The instance list a count by variant walks names every variant once.
    let named: BTreeSet<&'static str> = every_variant()
        .iter()
        .map(|error| classify(error).0)
        .collect();
    assert_eq!(
        named.len(),
        VARIANTS,
        "every_variant names each variant once"
    );
    assert_eq!(
        covered.len(),
        VARIANTS,
        "every PlanError variant has a text or an unreachable sample: {covered:?}"
    );
}
