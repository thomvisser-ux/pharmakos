// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every committed scenario, played.
//!
//! This file does two jobs, and the second is the one that is easy to miss.
//!
//! 1. **It is the acceptance test.** AGENTS.md §10 item 4: "headless scenario
//!    runs pass on assertions over events *and* hashes, not just 'it didn't
//!    crash'." Every file under `scenarios/` is played here and every assertion
//!    in it is checked, which is the same work `cargo xtask ci`'s `scenario`
//!    step does by shelling the binary — done here as well so that a developer
//!    running `cargo test` finds a broken scenario without waiting for the last
//!    step of the suite.
//!
//! 2. **It is the producer of the fresh output the `golden` step compares.**
//!    `tests/golden/README.md` rule 1: a missing fresh output is a failure, not
//!    a skip. The `golden` step runs *before* the `scenario` step, so a chain
//!    that only the binary produced would not exist yet when the comparison
//!    happens. Playing the scenarios here writes every
//!    `<target>/golden/scenarios/<name>/actual.hashes.txt` during `cargo test`,
//!    where the comparison can see it.
//!
//! `expand-east-segment`'s chain has a second producer, `crates/sim/tests/
//! scenario.rs`, which drives the sim directly. **That is the point rather than
//! a duplication**: the two write the same bytes to the same path, so the day
//! the gateway-hosted path and the direct path disagree, one of them overwrites
//! the other and the `golden` step is red. Cargo runs test binaries one after
//! another, so there is no race to worry about — and `tests/parity.rs` makes
//! the same claim on purpose rather than by side effect.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use pharmakos_gamectl::exit::Exit;
use pharmakos_gamectl::scenario::{self, SUFFIX};
use std::path::{Path, PathBuf};

/// Every committed scenario, named the way the command line names them.
///
/// Written out rather than discovered: `std::fs::read_dir` is a disallowed
/// method (`clippy.toml`) because directory order differs between ext4, NTFS
/// and APFS, and a list that feeds a golden run must not be platform-dependent.
/// `cargo xtask ci`'s `scenario` step walks the tree and sorts; this list is
/// the same set, and `the_committed_list_is_the_whole_set` checks it has not
/// fallen behind.
const SCENARIOS: &[&str] = &[COMPLETING, REFUSING];

/// The scenario whose route finishes (decisions-log item 103 (3)).
const COMPLETING: &str = "scenarios/skeleton/deploy-and-visit.scenario.jsonc";

/// The scenario that pins the interpreter refusing loudly (T11's).
const REFUSING: &str = "scenarios/skeleton/expand-east-segment.scenario.jsonc";

/// What a doctored copy is named.
///
/// **Not** [`SUFFIX`], and that is the whole point of the constant: a scratch
/// file that ended `.scenario.jsonc` would be collected by `cargo xtask ci`'s
/// `scenario` step, so one left behind by a test that panicked between writing
/// it and removing it would turn the next build red for a reason nobody could
/// find. The runner loads whatever path it is given, so the suffix costs
/// nothing. (The tests remove the file *before* they assert, for the same
/// reason.)
const SCRATCH: &str = ".doctored.jsonc";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| panic!("crates/gamectl sits two levels below the workspace root"))
        .to_path_buf()
}

/// Where a doctored copy is written: the target directory, not the repository.
///
/// A review found these three tests writing into `scenarios/skeleton/` and
/// deleting the file afterwards. [`SCRATCH`] already kept the `scenario` step
/// away from a leftover, but nothing kept `reuse` away: a run that panicked
/// between the write and the remove would leave an untracked file with no SPDX
/// header in the tree, and `cargo xtask ci` runs `reuse` after `test` — so the
/// next build would go red on licensing for a reason unrelated to the change.
/// Outside the tree there is nothing to leave behind.
///
/// The runner joins an absolute operand as an absolute path, and every path
/// *inside* a scenario file is resolved against `--root` rather than against
/// the file, so a scenario plays identically from here.
fn scratch(name: &str) -> PathBuf {
    let dir = pharmakos_gamectl::target_dir()
        .expect("an integration test runs from a cargo target directory")
        .join("scratch")
        .join("scenarios");
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|error| panic!("creating {}: {error}", dir.display()));
    dir.join(format!("{name}{SCRATCH}"))
}

fn gamectl(args: &[&str]) -> pharmakos_gamectl::Outcome {
    pharmakos_gamectl::run(args.iter().map(|arg| (*arg).to_owned()), &root())
}

#[test]
fn every_committed_scenario_passes_on_its_events_and_its_hashes() {
    for source in SCENARIOS {
        let outcome = gamectl(&["scenario", "run", source]);
        assert_eq!(
            outcome.code,
            Exit::Ok,
            "{source} did not pass:\n{}{}",
            outcome.out,
            outcome.err
        );
        assert!(
            outcome.out.contains("all held"),
            "{source}:\n{}",
            outcome.out
        );
    }
}

#[test]
fn the_committed_list_is_the_whole_set() {
    // Every file the step would find is in SCENARIOS, checked by name rather
    // than by listing the directory. A scenario added without a line here is a
    // scenario this test does not play and whose fresh output the `golden`
    // step therefore cannot find.
    let dir = root().join("scenarios").join("skeleton");
    for name in ["deploy-and-visit", "expand-east-segment"] {
        assert!(
            dir.join(format!("{name}{SUFFIX}")).is_file(),
            "{name} is listed here and is not committed"
        );
    }
    for source in SCENARIOS {
        assert!(
            root().join(source).is_file(),
            "{source} is listed here and is not committed"
        );
        assert!(source.ends_with(SUFFIX), "{source}");
    }
}

/// The acceptance line: "a deliberately doctored assertion produces a readable
/// diff and a non-zero exit".
///
/// Three doctorings, one per way a scenario can be wrong about the run: an
/// event that never fires, an event that fires too late, and a hash chain that
/// moved. Each is written to a scratch file beside the real one so the real one
/// is never touched.
#[test]
fn a_doctored_assertion_fails_readably_and_exits_non_zero() {
    let source = root().join(COMPLETING);
    let original = std::fs::read_to_string(&source).expect("the committed scenario");

    let cases: &[(&str, &str, &str, &[&str])] = &[
        (
            "doctored-missing-event",
            "\"event\": \"beacon_placed\", \"seat\": 0, \"by_tick\": 550",
            "\"event\": \"beacon_destroyed\", \"seat\": 0, \"by_tick\": 550",
            &["never fired", "What did, in first-seen order"],
        ),
        (
            "doctored-late-event",
            "\"event\": \"beacon_placed\", \"seat\": 0, \"by_tick\": 550",
            "\"event\": \"beacon_placed\", \"seat\": 0, \"by_tick\": 10",
            &["first fired at tick", "which is after tick 10"],
        ),
        (
            "doctored-moved-chain",
            "tests/golden/scenarios/deploy-and-visit/expected.hashes.txt",
            "tests/golden/scenarios/doctored-moved-chain/expected.hashes.txt",
            &["nothing is committed", "cargo xtask golden --bless"],
        ),
    ];

    for (name, from, to, wanted) in cases {
        assert!(
            original.contains(from),
            "{name}: the committed scenario no longer contains `{from}`, so this doctoring \
             would be a no-op and the test would pass by not testing"
        );
        let doctored = original.replacen(from, to, 1).replace(
            "\"name\": \"deploy-and-visit\"",
            &format!("\"name\": \"{name}\""),
        );
        let path = scratch(name);
        std::fs::write(&path, doctored.as_bytes())
            .unwrap_or_else(|error| panic!("writing {}: {error}", path.display()));

        let named = path.to_string_lossy().into_owned();
        let outcome = gamectl(&["scenario", "run", &named]);
        let _ = std::fs::remove_file(&path);

        assert_ne!(outcome.code, Exit::Ok, "{name} passed after being doctored");
        let report = format!("{}{}", outcome.out, outcome.err);
        assert!(report.contains("FAIL"), "{name}: {report}");
        for phrase in *wanted {
            assert!(
                report.contains(phrase),
                "{name}: the diff has to be readable, and `{phrase}` is not in it:\n{report}"
            );
        }
    }
}

/// A chain that moved by one line names the line, both sides of it, and the
/// two lengths — which is the whole of "a readable diff" for a hash chain.
#[test]
fn a_moved_chain_names_the_first_tick_that_disagrees() {
    let scenario = scenario::load(&root(), Path::new(COMPLETING)).expect("a valid scenario");
    let mut played = scenario::run::play(&root(), &scenario).expect("it plays");
    // One line, deliberately wrong, in the middle: a comparison that only ever
    // looked at the first or the last line would pass this.
    let lines: Vec<&str> = played.chain.lines().collect();
    let at = lines.len().checked_div(2).unwrap_or(0);
    let doctored: Vec<String> = lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            if index == at {
                String::from("601\t0000000000000000")
            } else {
                (*line).to_owned()
            }
        })
        .collect();
    played.chain = format!("{}\n", doctored.join("\n"));

    let (report, failed) = scenario::run::check(&root(), &scenario, &played).expect("a report");
    assert_eq!(failed, 1, "only the chain assertion moved:\n{report}");
    assert!(
        report.contains(&format!("line {}", at.saturating_add(1))),
        "the report has to name the first line that disagrees:\n{report}"
    );
    assert!(
        report.contains("0000000000000000"),
        "and what this run said there:\n{report}"
    );
    assert!(
        report.contains("1200 committed lines against 1200 played ticks"),
        "and both lengths, because a short chain and a moved chain are different news:\n{report}"
    );
}

/// A malformed scenario is refused with a pointer at every problem, and it is
/// an input error rather than a failed assertion: nothing was played, so
/// nothing held or did not hold.
#[test]
fn a_malformed_scenario_is_refused_with_a_pointer_at_every_problem() {
    let path = scratch("malformed");
    std::fs::write(
        &path,
        concat!(
            "{\n",
            "  \"format\": \"pharmakos.scenario.v2\",\n",
            "  \"name\": \"malformed\",\n",
            "  \"nonsense\": true,\n",
            "  \"map\": { \"seed\": \"0xCA5CADED\", \"generator\": \"skeleton\" },\n",
            "  \"seats\": [ { \"seat\": 1, \"kind\": \"wizard\" } ],\n",
            "  \"segments\": [ { \"index\": 0, \"length_ms\": -5 } ],\n",
            "  \"assertions\": [ { \"assert\": \"terminal_hash\" } ]\n",
            "}\n",
        )
        .as_bytes(),
    )
    .expect("the scratch file");
    let named = path.to_string_lossy().into_owned();
    let outcome = gamectl(&["scenario", "run", &named]);
    let _ = std::fs::remove_file(&path);

    assert_eq!(outcome.code, Exit::Input, "{}", outcome.err);
    for pointer in [
        "/format",
        "/nonsense",
        "/map/seed",
        "/seats/0/seat",
        "/seats/0/kind",
        "/segments/0/length_ms",
        "/assertions/0/assert",
        "/assertions",
    ] {
        assert!(
            outcome.err.contains(pointer),
            "every problem gets a pointer, and `{pointer}` is missing:\n{}",
            outcome.err
        );
    }
    assert!(
        outcome.err.contains("terminal_hash"),
        "a reserved assertion name says which decision holds it, not `unknown`:\n{}",
        outcome.err
    );
}

/// A `builtin` seat names the task that will make it playable, rather than
/// failing with whatever the gateway happened to say.
#[test]
fn a_builtin_seat_names_the_task_that_owes_the_operator() {
    let source = root().join(COMPLETING);
    let original = std::fs::read_to_string(&source).expect("the committed scenario");
    let doctored = original
        .replace(
            "{ \"seat\": 1, \"kind\": \"safe\" }",
            "{ \"seat\": 1, \"kind\": \"builtin\" }",
        )
        .replace(
            "\"name\": \"deploy-and-visit\"",
            "\"name\": \"builtin-seat\"",
        );
    let path = scratch("builtin-seat");
    std::fs::write(&path, doctored.as_bytes()).expect("the scratch file");
    let named = path.to_string_lossy().into_owned();
    let outcome = gamectl(&["scenario", "run", &named]);
    let _ = std::fs::remove_file(&path);

    assert_eq!(outcome.code, Exit::Input, "{}", outcome.err);
    assert!(outcome.err.contains("T18"), "{}", outcome.err);
    assert!(outcome.err.contains("/seats/1/kind"), "{}", outcome.err);
}

/// The sim is a pure function of (map seed, playbooks, rules hash), and a
/// scenario is that sentence written down. Two runs of one file in one process
/// is the cheapest test of it there is.
#[test]
fn one_scenario_played_twice_produces_one_chain() {
    let scenario = scenario::load(&root(), Path::new(COMPLETING)).expect("a valid scenario");
    let first = scenario::run::play(&root(), &scenario).expect("it plays");
    let second = scenario::run::play(&root(), &scenario).expect("it plays again");
    assert_eq!(
        first.chain, second.chain,
        "two runs of one scenario disagreed"
    );
    assert_eq!(first.events, second.events);
}

/// `expand-east-segment` seals a playbook no seat could submit, and the run
/// says so on the page rather than in a source comment.
#[test]
fn a_scenario_sealed_behind_the_verifiers_door_says_so_in_its_report() {
    let outcome = gamectl(&["scenario", "run", REFUSING]);
    assert_eq!(outcome.code, Exit::Ok, "{}{}", outcome.out, outcome.err);
    assert!(
        outcome.out.contains("HARNESS door"),
        "the report has to say which door the orders came through:\n{}",
        outcome.out
    );
    assert!(
        outcome.out.contains("E0403"),
        "and why the real one refused them:\n{}",
        outcome.out
    );

    // And the other one did not need it: a note that appeared on every run
    // would be a note nobody reads.
    let clean = gamectl(&["scenario", "run", COMPLETING]);
    assert_eq!(clean.code, Exit::Ok, "{}{}", clean.out, clean.err);
    assert!(
        !clean.out.contains("HARNESS door"),
        "deploy-and-visit's playbook goes through `submit_plan`:\n{}",
        clean.out
    );
}
