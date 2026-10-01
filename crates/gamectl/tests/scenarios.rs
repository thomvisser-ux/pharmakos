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
const SCENARIOS: &[&str] = &[
    AGAINST_EASY,
    THREE_ROUNDS,
    COMPLETING,
    REFUSING,
    ROUND_LIMIT_AUDIT,
    UNAFFORDABLE_DEPLOY,
];

/// The scenario against the built-in operator: seat 1 is `builtin`, played by
/// Easy, and seat 0 builds a Generator on its vent (T18b; decisions-log item
/// 113 (6) and (8)). It is also the `builtin` seat's test: every committed
/// scenario is played by the test below, so it costs no second match.
const AGAINST_EASY: &str = "scenarios/skeleton/against-easy.scenario.jsonc";

/// The demo scenario: three rounds against Easy (T20; decisions-log item
/// 116 (6)(c)).
const THREE_ROUNDS: &str = "scenarios/skeleton/against-easy-three-rounds.scenario.jsonc";

/// The scenario whose route finishes (decisions-log item 103 (3)).
const COMPLETING: &str = "scenarios/skeleton/deploy-and-visit.scenario.jsonc";

/// The scenario that pins the interpreter refusing loudly (T11's).
const REFUSING: &str = "scenarios/skeleton/expand-east-segment.scenario.jsonc";

/// A match played to its round limit, whose winner the final audit names
/// (S1's `fixs`; register X-08).
const ROUND_LIMIT_AUDIT: &str = "scenarios/s1/round-limit-audit.scenario.jsonc";

/// A deploy the treasury cannot cover fails its step, and `on_fail` takes it
/// (S1's `fixs`; register X-12, decision 7).
const UNAFFORDABLE_DEPLOY: &str = "scenarios/s1/unaffordable-deploy.scenario.jsonc";

/// **The exact set of scenarios allowed to seal through the harness door.**
///
/// The door is unscoped in the runner — anything `submit_plan` explicitly
/// refuses takes it, with a `note` line and exit 0 — and a review is right
/// that a scenario acquiring it later would be invisible in CI, because
/// `xtask`'s `scenario` step reads the child's exit status and nothing else.
/// Scoping it properly means an opt-in key in the file, which
/// `xtask/src/scenario.rs` would have to know about, and `xtask` is a contract
/// path (AGENTS.md §5) — so it rides with the owner's ruling on the door.
///
/// This list is the interim that costs nothing: the door is pinned to the one
/// file that needs it, in a test that already plays every committed scenario.
/// A scenario that starts using it, or one that stops, turns `cargo test` red
/// with the reason in the message.
const HARNESS_DOOR: &[&str] = &[REFUSING];

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

/// Where a doctored `hash_chain_equals` golden points: outside `tests/golden/`.
///
/// T20's review found two hazards in the chain tests (decisions-log item
/// 118 (4), T20's items 11 and 12). The moved-chain test checked a doctored
/// chain against the committed golden, so the runner wrote the doctored chain
/// to `<target>/golden/scenarios/deploy-and-visit/actual.hashes.txt`, the path
/// `every_committed_scenario_passes_on_its_events_and_its_hashes` writes in
/// parallel and the `golden` step reads afterwards; and the missing-chain case
/// pointed its golden at `tests/golden/scenarios/doctored-missing-chain/`, so its
/// fresh chain landed among the scenario kinds the cross-OS guard uploads and
/// compares, where it alone would satisfy "at least one scenario chain".
///
/// Both now load the scenario, repoint its chain assertion here, and call
/// `scenario::run::check` directly. `actual_for` answers `None` for a path
/// outside `tests/golden/`, so nothing is written under `<target>/golden/`; the
/// loader, which refuses such a path in a file, is not asked.
fn scratch_golden(name: &str) -> PathBuf {
    let dir = pharmakos_gamectl::target_dir()
        .expect("an integration test runs from a cargo target directory")
        .join("scratch")
        .join("scenarios");
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|error| panic!("creating {}: {error}", dir.display()));
    dir.join(format!("{name}.hashes.txt"))
}

/// The scenario at `source`, loaded, with its one chain assertion pointed at
/// `golden`.
fn with_chain_golden(source: &str, golden: &Path) -> scenario::Scenario {
    let mut loaded = scenario::load(&root(), Path::new(source)).expect("a valid scenario");
    let mut repointed = 0_usize;
    for assertion in &mut loaded.assertions {
        if let scenario::Assertion::HashChainEquals { golden: path } = assertion {
            *path = golden.to_string_lossy().into_owned();
            repointed = repointed.saturating_add(1);
        }
    }
    assert_eq!(repointed, 1, "{source} has exactly one chain assertion");
    loaded
}

/// The fresh chain a committed scenario's golden is compared with.
fn committed_actual(name: &str) -> PathBuf {
    pharmakos_gamectl::target_dir()
        .expect("an integration test runs from a cargo target directory")
        .join("golden")
        .join("scenarios")
        .join(name)
        .join("actual.hashes.txt")
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
        // And it went through the door it is allowed to go through. Checked
        // here rather than in a test of its own so that it costs no extra
        // match: every scenario is already played on this line.
        let used_the_door = outcome.out.contains("HARNESS door");
        assert_eq!(
            used_the_door,
            HARNESS_DOOR.contains(source),
            "{source} {} the harness door. That list is the exact set allowed to use it; a \
             scenario that starts using it has stopped being a claim about what a client could \
             cause, and one that stops no longer needs the door.\n{}",
            if used_the_door { "used" } else { "did not use" },
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
    for (stage, names) in [
        (
            "skeleton",
            &[
                "against-easy",
                "against-easy-three-rounds",
                "deploy-and-visit",
                "expand-east-segment",
            ][..],
        ),
        ("s1", &["round-limit-audit", "unaffordable-deploy"][..]),
    ] {
        let dir = root().join("scenarios").join(stage);
        for name in names {
            assert!(
                dir.join(format!("{name}{SUFFIX}")).is_file(),
                "{stage}/{name} is listed here and is not committed"
            );
        }
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
/// Two doctorings of the file, one per way a scenario's events can be wrong
/// about the run: an event that never fires, and an event that fires too late.
/// Each is written to a scratch file outside the tree, so the real one is never
/// touched. The two ways its chain can be wrong, missing and moved, are the two
/// tests after this one, which doctor the loaded scenario rather than the file
/// ([`scratch_golden`] says why).
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

/// A chain assertion whose golden is missing says that nothing is committed
/// there and how to commit it, and writes no fresh chain anywhere the `golden`
/// step or the cross-OS guard reads (T20's item 12, [`scratch_golden`]).
#[test]
fn a_missing_chain_says_nothing_is_committed_and_writes_no_actual() {
    // The by-product the old version of this case left in a warm target
    // directory; it must not come back.
    let stray = committed_actual("doctored-missing-chain");
    if let Some(dir) = stray.parent() {
        let _ = std::fs::remove_dir_all(dir);
    }
    let golden = scratch_golden("doctored-missing-chain");
    let _ = std::fs::remove_file(&golden);
    let scenario = with_chain_golden(COMPLETING, &golden);
    let played = scenario::run::play(&root(), &scenario).expect("it plays");

    let (report, failed) = scenario::run::check(&root(), &scenario, &played).expect("a report");
    assert_eq!(failed, 1, "only the chain assertion fails:\n{report}");
    for phrase in ["nothing is committed", "cargo xtask golden --bless"] {
        assert!(
            report.contains(phrase),
            "the diff has to be readable, and `{phrase}` is not in it:\n{report}"
        );
    }
    assert!(
        stray.parent().is_none_or(|dir| !dir.exists()),
        "a doctored chain was written among the scenario kinds, under {}",
        stray.display()
    );
}

/// A chain that moved by one line names the line, both sides of it, and the
/// two lengths — which is the whole of "a readable diff" for a hash chain. The
/// doctored chain is checked against a copy of the committed one outside
/// `tests/golden/`, so it is never written where the committed scenario's
/// fresh chain goes (T20's item 11, [`scratch_golden`]).
#[test]
fn a_moved_chain_names_the_first_tick_that_disagrees() {
    let committed = std::fs::read(
        root()
            .join("tests")
            .join("golden")
            .join("scenarios")
            .join("deploy-and-visit")
            .join("expected.hashes.txt"),
    )
    .expect("deploy-and-visit's committed chain");
    let golden = scratch_golden("deploy-and-visit.moved");
    std::fs::write(&golden, &committed)
        .unwrap_or_else(|error| panic!("writing {}: {error}", golden.display()));
    let scenario = with_chain_golden(COMPLETING, &golden);
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
    let _ = std::fs::remove_file(&golden);
    // Nothing doctored reached the fresh chain the `golden` step compares with
    // the committed one. Before, this test wrote the doctored chain there.
    let actual = committed_actual("deploy-and-visit");
    if let Ok(fresh) = std::fs::read_to_string(&actual) {
        assert!(
            !fresh.contains("601\t0000000000000000"),
            "the doctored chain was written to {}",
            actual.display()
        );
    }
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

/// A `builtin` seat whose rules table Easy cannot score with is an input
/// error that names the seat's pointer and the rules path, rather than
/// whatever the gateway happened to say (the review of T18b: the refusal it
/// replaced had a test, and this error had none).
///
/// The committed scenario plays from a scratch root -- its own file and its
/// playbook copied byte for byte -- whose `rules/rules.v1.json` is the
/// committed text with one row Easy reads set to zero
/// (`interface_times.edit_settings_base_ms`, the row `Tuning`'s own unit test
/// zeroes). The sim reads that table; the operator refuses it.
#[test]
fn a_builtin_seat_easy_cannot_score_for_is_an_input_error() {
    let repository = root();
    let scratch_root = pharmakos_gamectl::target_dir()
        .expect("an integration test runs from a cargo target directory")
        .join("scratch")
        .join(format!("builtin-root-{}", std::process::id()));
    let playbook = "scenarios/skeleton/against-easy.playbook.jsonc";
    // Named with SCRATCH's suffix, not SUFFIX, for the reason that constant
    // gives.
    let doctored = format!("scenarios/skeleton/against-easy{SCRATCH}");
    std::fs::create_dir_all(scratch_root.join("scenarios/skeleton")).expect("the folder");
    std::fs::create_dir_all(scratch_root.join("rules")).expect("the folder");
    std::fs::copy(repository.join(playbook), scratch_root.join(playbook)).expect("the playbook");
    std::fs::copy(repository.join(AGAINST_EASY), scratch_root.join(&doctored))
        .expect("the scenario");
    let rules = std::fs::read_to_string(repository.join("rules/rules.v1.json")).expect("rules");
    let zeroed = rules.replace(
        "\"edit_settings_base_ms\": 2000",
        "\"edit_settings_base_ms\": 0",
    );
    assert_ne!(zeroed, rules, "the rules text still carries the row");
    std::fs::write(scratch_root.join("rules/rules.v1.json"), zeroed).expect("the rules");

    let outcome = pharmakos_gamectl::run(
        ["scenario", "run", doctored.as_str()]
            .iter()
            .map(|arg| (*arg).to_owned()),
        &scratch_root,
    );
    let _ = std::fs::remove_dir_all(&scratch_root);

    assert_eq!(outcome.code, Exit::Input, "{}{}", outcome.out, outcome.err);
    for expected in [
        "/seats/1/kind",
        "rules/rules.v1.json",
        "edit_settings_base_ms",
    ] {
        assert!(
            outcome.err.contains(expected),
            "`{expected}` is missing:\n{}",
            outcome.err
        );
    }
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

    // That `deploy-and-visit` does *not* print the note — a note on every run
    // is a note nobody reads — is asserted by
    // `every_committed_scenario_passes_on_its_events_and_its_hashes` against
    // [`HARNESS_DOOR`], which covers every committed scenario rather than this
    // one pair and plays no extra match to do it.
}
