// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The nightly harness, checked on every pull request (S2's plan, task
//! `night`, and its E20).
//!
//! The nightly workflow plays Rusher, Turtle and Hunter against Easy over
//! twenty seeds, which no pull request waits for. What a pull request can
//! check cheaply is checked here, so an arm cannot rot between two nights:
//!
//! * every arm's playbook **qualifies**, and each arm plays **one round on its
//!   own seed** through `submit_plan`, with no refusal and its event
//!   assertions held, from each of the two seats the nightly puts it on;
//! * the result's shape is pinned by a golden, a two-seed run of the result
//!   fixture (`tests/golden/scenarios/nightly-result/expected.result.json`),
//!   which this file produces for the `golden` step;
//! * the two readers' arm rules (no hash chain, one `playbook` seat and one
//!   `builtin` seat) and the run's refusals, each with a test.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::path::{Path, PathBuf};

use pharmakos_gamectl::exit::Exit;
use pharmakos_gamectl::scenario::{self, Seat, SeatKind, run};

/// Every arm the workflow plays, as its matrix names them, and the result
/// fixture. Written out rather than discovered, as `tests/scenarios.rs`'
/// list is; `the_arms_are_the_whole_set` checks the directory against it.
const ARMS: &[&str] = &[RUSHER, TURTLE, HUNTER, FIXTURE];

/// Each entry of [`ARMS`] has a test of its own below that plays it; this is
/// how many, so an arm added to the list without one fails
/// `the_arms_are_the_whole_set`.
const ARMS_PLAYED: usize = 4;

/// The three arms the workflow's matrix plays.
const RUSHER: &str = "scenarios/nightly/rusher.scenario.jsonc";
const TURTLE: &str = "scenarios/nightly/turtle.scenario.jsonc";
const HUNTER: &str = "scenarios/nightly/hunter.scenario.jsonc";

/// The arm the result golden is played from.
const FIXTURE: &str = "scenarios/nightly/fixture/result-golden.scenario.jsonc";

/// The two seeds it is played over.
const FIXTURE_SEEDS: &str = "scenarios/nightly/fixture/two.seeds.txt";

/// The result golden.
const RESULT_GOLDEN: &str = "tests/golden/scenarios/nightly-result/expected.result.json";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| panic!("crates/gamectl sits two levels below the workspace root"))
        .to_path_buf()
}

fn gamectl(args: &[&str]) -> pharmakos_gamectl::Outcome {
    pharmakos_gamectl::run(args.iter().map(|arg| (*arg).to_owned()), &root())
}

/// A scratch directory under the target directory, emptied.
fn scratch(name: &str) -> PathBuf {
    let dir = pharmakos_gamectl::target_dir()
        .expect("an integration test runs from a cargo target directory")
        .join("scratch")
        .join("nightly")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|error| panic!("creating {}: {error}", dir.display()));
    dir
}

/// Every `*.scenario.jsonc` under `scenarios/nightly/`, sorted.
#[allow(
    clippy::disallowed_methods,
    reason = "clippy.toml's own reason string sanctions read_dir when the listing is collected and sorted before use, which is what happens below"
)]
fn arm_files() -> Vec<String> {
    let base = root().join("scenarios").join("nightly");
    let mut stack = vec![base];
    let mut found = Vec::new();
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("scenarios/nightly is readable") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.to_string_lossy().ends_with(scenario::SUFFIX) {
                let relative = path.strip_prefix(root()).expect("under the root");
                found.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    found.sort();
    found
}

#[test]
fn the_arms_are_the_whole_set() {
    let mut listed: Vec<String> = ARMS.iter().map(|arm| (*arm).to_owned()).collect();
    listed.sort();
    assert_eq!(
        arm_files(),
        listed,
        "an arm under scenarios/nightly/ is missing from ARMS, so nothing on a pull request \
         checks it plays"
    );
    for arm in ARMS {
        let loaded = scenario::load(&root(), Path::new(arm)).expect("a valid arm");
        assert!(loaded.nightly, "{arm} is read as an arm");
    }
    assert_eq!(ARMS.len(), ARMS_PLAYED, "every arm has its own round test");
}

/// The arm's own scenario with the arm on `arm_seat` and Easy on the other,
/// cut to its first round, as the nightly would seat it.
fn one_round(arm: &str, arm_seat: u8) -> scenario::Scenario {
    let loaded = scenario::load(&root(), Path::new(arm)).expect("a valid arm");
    let playbook = loaded
        .seats
        .iter()
        .find_map(|seat| match &seat.kind {
            SeatKind::Playbook(path) => Some(path.clone()),
            _ => None,
        })
        .expect("an arm seats a playbook");
    let seats = [0_u8, 1]
        .into_iter()
        .map(|seat| Seat {
            seat,
            kind: if seat == arm_seat {
                SeatKind::Playbook(playbook.clone())
            } else {
                SeatKind::Builtin
            },
        })
        .collect();
    let mut cut = scenario::Scenario { seats, ..loaded };
    cut.segments.truncate(1);
    cut
}

/// E20: an arm's playbook qualifies, and the arm plays a round on its own
/// seed through `submit_plan` from either seat, with no refusal and both seats
/// sealed. On the file's own seat layout the round is checked against the
/// file's own event assertions, through `run::check`, and every one must hold,
/// so an arm's assertions are never dead text. One test per arm, so the rounds
/// run side by side.
fn qualifies_and_plays_a_round_through_the_door(arm: &str) {
    let loaded = scenario::load(&root(), Path::new(arm)).expect("a valid arm");
    for seat in &loaded.seats {
        if let SeatKind::Playbook(path) = &seat.kind {
            let verified = gamectl(&["verify", path]);
            assert_eq!(
                verified.code,
                Exit::Ok,
                "{arm}: its playbook {path} does not qualify:\n{}{}",
                verified.out,
                verified.err
            );
        }
    }
    let own_seat = loaded
        .seats
        .iter()
        .find(|seat| matches!(seat.kind, SeatKind::Playbook(_)))
        .map(|seat| seat.seat)
        .expect("an arm seats a playbook");
    for arm_seat in [0_u8, 1] {
        let cut = one_round(arm, arm_seat);
        let played = run::play(&root(), &cut)
            .unwrap_or_else(|failure| panic!("{arm} on seat {arm_seat}: {}", failure.message));
        if arm_seat == own_seat {
            let (report, failed) = run::check(&root(), &cut, &played)
                .unwrap_or_else(|failure| panic!("{arm}: {}", failure.message));
            assert_eq!(
                failed, 0,
                "{arm}'s own assertions on its own seed:\n{report}"
            );
            assert!(!cut.assertions.is_empty(), "{arm} asserts on events");
        }
        assert!(
            played.notes.is_empty(),
            "{arm} on seat {arm_seat} went through the harness door: {:?}",
            played.notes
        );
        assert_eq!(played.segments.len(), 1, "{arm}: one round");
        for seat in [0_u8, 1] {
            assert!(
                played
                    .events
                    .iter()
                    .any(|fired| fired.kind == "plan_sealed" && fired.seat == Some(seat)),
                "{arm} with the arm on seat {arm_seat}: seat {seat} sealed nothing"
            );
        }
    }
}

#[test]
fn rusher_qualifies_and_plays_a_round_through_the_door() {
    qualifies_and_plays_a_round_through_the_door(RUSHER);
}

#[test]
fn turtle_qualifies_and_plays_a_round_through_the_door() {
    qualifies_and_plays_a_round_through_the_door(TURTLE);
}

#[test]
fn hunter_qualifies_and_plays_a_round_through_the_door() {
    qualifies_and_plays_a_round_through_the_door(HUNTER);
}

#[test]
fn the_result_fixture_qualifies_and_plays_a_round_through_the_door() {
    qualifies_and_plays_a_round_through_the_door(FIXTURE);
}

/// The result's shape, pinned: a two-seed run of the result fixture, the arm
/// on seat 0 for the first seed and on seat 1 for the second. This test is the
/// golden's producer.
#[test]
fn a_two_seed_run_writes_the_committed_result() {
    let outcome = gamectl(&[
        "scenario",
        "run",
        FIXTURE,
        "--operator",
        "easy",
        "--seeds",
        FIXTURE_SEEDS,
        "--format",
        "json",
    ]);
    assert_eq!(outcome.code, Exit::Ok, "{}", outcome.err);
    let actual = pharmakos_gamectl::actual_for(RESULT_GOLDEN).expect("a target directory");
    std::fs::create_dir_all(actual.parent().expect("a parent")).expect("the golden's directory");
    std::fs::write(&actual, outcome.out.as_bytes()).expect("the fresh result");
    let committed = std::fs::read_to_string(root().join(RESULT_GOLDEN)).unwrap_or_default();
    assert_eq!(
        committed.replace("\r\n", "\n"),
        outcome.out,
        "the nightly result moved; the fresh one is at {}. tests/golden/scenarios/README.md \
         says what a move means",
        actual.display()
    );
    // And the text form says the same thing in a person's words.
    let text = gamectl(&["scenario", "run", FIXTURE, "--seeds", FIXTURE_SEEDS]);
    assert_eq!(text.code, Exit::Ok, "{}", text.err);
    assert!(text.out.contains("win_rate_percent"), "{}", text.out);
}

#[test]
fn out_writes_the_result_to_a_file() {
    let dir = scratch("out");
    let out = dir.join("result.json");
    let outcome = gamectl(&[
        "scenario",
        "run",
        FIXTURE,
        "--seeds",
        FIXTURE_SEEDS,
        "--format",
        "json",
        "--out",
        &out.to_string_lossy(),
    ]);
    assert_eq!(outcome.code, Exit::Ok, "{}", outcome.err);
    let written = std::fs::read_to_string(&out).expect("the result file");
    assert!(written.contains("\"win_rate_percent\": "), "{written}");
    assert!(outcome.out.contains("result.json"), "{}", outcome.out);
}

#[test]
fn a_run_over_seeds_refuses_a_scenario_that_is_not_an_arm() {
    // A playbook seat and a `safe` seat: no built-in operator to play against.
    let outcome = gamectl(&[
        "scenario",
        "run",
        "scenarios/skeleton/deploy-and-visit.scenario.jsonc",
        "--seeds",
        FIXTURE_SEEDS,
    ]);
    assert_eq!(outcome.code, Exit::Input, "{}", outcome.err);
    assert!(outcome.err.contains("decision 29"), "{}", outcome.err);
}

#[test]
fn a_refused_seed_file_is_an_input_error_naming_its_line() {
    let dir = scratch("seeds");
    for (body, wanted) in [
        ("0x0000000000000001\n0x0000000000000001\n", "line 2"),
        ("0x1\n", "line 1"),
        ("# nothing\n", "no seed"),
    ] {
        let path = dir.join("bad.seeds.txt");
        std::fs::write(&path, body).expect("a scratch seed file");
        let outcome = gamectl(&[
            "scenario",
            "run",
            FIXTURE,
            "--seeds",
            &path.to_string_lossy(),
        ]);
        assert_eq!(outcome.code, Exit::Input, "{body:?}: {}", outcome.err);
        assert!(outcome.err.contains(wanted), "{body:?}: {}", outcome.err);
    }
    let missing = gamectl(&["scenario", "run", FIXTURE, "--seeds", "no/such/seeds.txt"]);
    assert_eq!(missing.code, Exit::Input, "{}", missing.err);
}

#[test]
fn operator_is_refused_for_a_scenario_with_no_builtin_seat() {
    let outcome = gamectl(&[
        "scenario",
        "run",
        "scenarios/skeleton/deploy-and-visit.scenario.jsonc",
        "--operator",
        "easy",
    ]);
    assert_eq!(outcome.code, Exit::Input, "{}", outcome.err);
    assert!(outcome.err.contains("seats none"), "{}", outcome.err);
}

/// A scratch repository root holding the rules table, the placeholder
/// playbook and one arm file, so the reader's arm rules can be met by a file
/// that really lives under `scenarios/nightly/`.
fn scratch_root(name: &str, arm: &str) -> PathBuf {
    let dir = scratch(name);
    for file in [
        "rules/rules.v1.json",
        "scenarios/nightly/placeholder.playbook.jsonc",
    ] {
        let to = dir.join(file);
        std::fs::create_dir_all(to.parent().expect("a parent")).expect("a directory");
        std::fs::copy(root().join(file), &to).expect("a copy");
    }
    let to = dir.join("scenarios/nightly/doctored.scenario.jsonc");
    std::fs::write(&to, arm).expect("the arm");
    dir
}

#[test]
fn the_reader_holds_an_arm_to_an_arms_rules() {
    let text = std::fs::read_to_string(root().join("scenarios/nightly/rusher.scenario.jsonc"))
        .expect("rusher");
    let chained = text.replace(
        "\"note\": \"Easy sealed its own.\" }",
        "\"note\": \"Easy sealed its own.\" },\n    { \"assert\": \"hash_chain_equals\", \"golden\": \"tests/golden/scenarios/rusher/expected.hashes.txt\" }",
    );
    assert_ne!(chained, text, "the doctoring applied");
    let three = text.replace(
        "{ \"seat\": 1, \"kind\": \"builtin\" }",
        "{ \"seat\": 1, \"kind\": \"builtin\" },\n    { \"seat\": 2, \"kind\": \"builtin\" }",
    );
    assert_ne!(three, text, "the doctoring applied");
    for (name, body, pointer) in [
        ("chained", chained, "/assertions/2/assert"),
        ("three", three, "/seats"),
    ] {
        let dir = scratch_root(name, &body);
        let refused = scenario::load(&dir, Path::new("scenarios/nightly/doctored.scenario.jsonc"))
            .expect_err("refused");
        assert_eq!(refused.code, Exit::Input, "{name}");
        assert!(
            refused.message.contains(pointer),
            "{name}: {}",
            refused.message
        );
    }
    // The undoctored arm loads from the scratch root, and is an arm there.
    let dir = scratch_root("plain", &text);
    let loaded = scenario::load(&dir, Path::new("scenarios/nightly/doctored.scenario.jsonc"))
        .expect("the arm as committed");
    assert!(loaded.nightly);
    // And the same text outside scenarios/nightly/ is an ordinary scenario,
    // which must carry a chain.
    let elsewhere = dir.join("elsewhere.scenario.jsonc");
    std::fs::write(&elsewhere, &text).expect("a copy");
    let refused = scenario::load(&dir, Path::new("elsewhere.scenario.jsonc")).expect_err("refused");
    assert!(
        refused.message.contains("AND on the hash chain"),
        "{}",
        refused.message
    );
}
