// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fork equivalence — **research builds only**.
//!
//! The whole file is behind the feature, so `cargo test --workspace` without
//! `--features research` compiles nothing here. `cargo xtask ci` runs the
//! workspace's tests in both configurations, which is what stops either one
//! rotting.
//!
//! G4 asserted 20 of 20 forks equivalent with 10 of 10 parents unperturbed on a
//! toy sim. This re-asserts it against the real world type, which is the point
//! of the spike-to-crate handover: the spike's evidence is about the design,
//! and the crate owes its own evidence about the code.

#![cfg(feature = "research")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules — not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use pharmakos_sim::encoding::Enc;
use pharmakos_sim::research::{fork, fork_and_step};
use pharmakos_sim::{MatchSettings, RulesTable, World, WorldConfig};

fn world() -> World {
    let rules = RulesTable::load(
        &std::path::Path::new("..")
            .join("..")
            .join("rules")
            .join("rules.v1.json"),
    )
    .expect("the committed rules table loads");
    World::new(&WorldConfig {
        match_seed: pharmakos_sim::DETERMINISM_MATCH_SEED,
        seats: pharmakos_sim::DETERMINISM_SEATS,
        units_per_seat: pharmakos_sim::DETERMINISM_UNITS_PER_SEAT,
        rules,
        match_settings: MatchSettings::default(),
    })
    .expect("the rules table describes a map and a broadphase grid")
}

#[test]
fn a_fork_walks_the_same_timeline_as_its_parent() {
    let mut parent = world();
    let mut enc = Enc::with_capacity(64 * 1024);

    for round in 0..20 {
        for _ in 0..7 {
            let _ = parent.step(&mut enc);
        }
        let parent_hash_before = parent.state_hash();

        // What the parent itself would do over the next 13 ticks.
        let mut reference = parent.clone();
        let expected: Vec<u64> = (0..13).map(|_| reference.step(&mut enc)).collect();

        let (_child, trace) = fork_and_step(&parent, 13);

        assert_eq!(
            trace, expected,
            "round {round}: the fork diverged from the timeline it was taken from"
        );
        assert_eq!(
            parent.state_hash(),
            parent_hash_before,
            "round {round}: stepping a fork perturbed its parent"
        );
    }
}

#[test]
fn a_fork_is_an_independent_timeline() {
    let mut parent = world();
    let mut enc = Enc::with_capacity(64 * 1024);
    for _ in 0..40 {
        let _ = parent.step(&mut enc);
    }
    let before = parent.state_hash();
    let mut child = fork(&parent);
    assert_eq!(child.state_hash(), before, "a fresh fork is its parent");
    for _ in 0..25 {
        let _ = child.step(&mut enc);
    }
    assert_ne!(
        child.state_hash(),
        before,
        "a child that stepped 25 ticks and did not move is not a child"
    );
    assert_eq!(parent.state_hash(), before, "the parent moved");
}
