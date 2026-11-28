// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `gamectl arena`, played (decisions-log item 233).
//!
//! * the result's shape is pinned by a golden, the committed three-way
//!   match-up over two seeds, six matches
//!   (`tests/golden/scenarios/arena-result/expected.result.json`), which this
//!   file produces for the `golden` step;
//! * every match-up under `scenarios/arena/` is read as one and holds its
//!   event assertions on its own seed, so a match-up cannot rot unplayed: the
//!   `scenario` step validates them and plays none;
//! * a nightly arm is a match-up of two, and the text form and `--out`;
//! * the run's refusals, each with a test: a `sealed` seat, `--operator` with
//!   no built-in contestant, a seed file that is refused, and a playbook
//!   `submit_plan` refuses.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::path::{Path, PathBuf};

use pharmakos_gamectl::cli::Operator;
use pharmakos_gamectl::exit::Exit;
use pharmakos_gamectl::scenario::arena::{self, Contestant};
use pharmakos_gamectl::scenario::seeds::Listed;
use pharmakos_gamectl::scenario::{self, Home};

/// Every match-up under `scenarios/arena/`. Written out, as the nightly's
/// arms are; `the_match_ups_are_the_whole_set` checks the directory against
/// it.
const MATCH_UPS: &[&str] = &[THREE_WAY];

/// The three kinds of contestant: Easy, the placeholder playbook and the safe
/// playbook, two one-minute rounds. The result golden's match-up.
const THREE_WAY: &str = "scenarios/arena/three-way.scenario.jsonc";

/// A nightly arm, which seats two contestants: Easy and the placeholder
/// playbook.
const ARM: &str = "scenarios/nightly/fixture/result-golden.scenario.jsonc";

/// The two seeds the golden is played over.
const SEEDS: &str = "scenarios/nightly/fixture/two.seeds.txt";

/// The result golden.
const RESULT_GOLDEN: &str = "tests/golden/scenarios/arena-result/expected.result.json";

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
        .join("arena")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|error| panic!("creating {}: {error}", dir.display()));
    dir
}

/// One key's value on every match's line of a JSON result, in order.
fn each<'a>(result: &'a str, key: &str, next: &str) -> Vec<&'a str> {
    let opens = format!("\"{key}\": ");
    let closes = format!(", \"{next}\"");
    result
        .lines()
        .filter_map(|line| line.split_once(opens.as_str()))
        .map(|(_, rest)| {
            rest.split_once(closes.as_str())
                .unwrap_or_else(|| panic!("`{next}` follows `{key}`"))
                .0
        })
        .collect()
}

/// The result's shape, pinned: three contestants over two seeds, each seed
/// under each of the three rotations. This test is the golden's producer.
#[test]
fn three_contestants_over_two_seeds_write_the_committed_result() {
    let outcome = gamectl(&[
        "arena",
        THREE_WAY,
        "--operator",
        "easy",
        "--seeds",
        SEEDS,
        "--format",
        "json",
    ]);
    assert_eq!(outcome.code, Exit::Ok, "{}", outcome.err);
    let actual = pharmakos_gamectl::actual_for(RESULT_GOLDEN).expect("a target directory");
    std::fs::create_dir_all(actual.parent().expect("a parent")).expect("the golden's directory");
    std::fs::write(&actual, outcome.out.as_bytes()).expect("the fresh result");
    // A golden that is not there is a red test naming it, never an empty text
    // compared with the fresh one.
    let committed = std::fs::read_to_string(root().join(RESULT_GOLDEN))
        .unwrap_or_else(|error| panic!("reading {RESULT_GOLDEN}: {error}"));
    assert_eq!(
        committed.replace("\r\n", "\n"),
        outcome.out,
        "the arena result moved; the fresh one is at {}. tests/golden/scenarios/README.md says \
         what a move means",
        actual.display()
    );
    // Six matches: each seed under rotations 0, 1 and 2, in that order, each
    // contestant on each seat once a seed.
    assert_eq!(
        each(&outcome.out, "rotation", "contestant_by_seat"),
        ["0", "1", "2", "0", "1", "2"]
    );
    assert_eq!(
        each(&outcome.out, "contestant_by_seat", "winners"),
        [
            "[0, 1, 2]",
            "[2, 0, 1]",
            "[1, 2, 0]",
            "[0, 1, 2]",
            "[2, 0, 1]",
            "[1, 2, 0]"
        ]
    );
    // The operator is said on the contestant that plays it, a playbook's
    // fingerprint on the playbook, and the safe playbook carries neither.
    for wanted in [
        "{\"contestant\": 0, \"kind\": \"builtin\", \"operator\": \"easy\"}",
        "{\"contestant\": 1, \"kind\": \"playbook\", \"playbook\": \
         \"scenarios/nightly/placeholder.playbook.jsonc\", \"plan_fingerprint\": \"",
        "{\"contestant\": 2, \"kind\": \"safe\"}",
    ] {
        assert!(
            outcome.out.contains(wanted),
            "no `{wanted}` in:\n{}",
            outcome.out
        );
    }
}

/// Every `*.scenario.jsonc` under `scenarios/arena/`, sorted.
#[allow(
    clippy::disallowed_methods,
    reason = "clippy.toml's own reason string sanctions read_dir when the listing is collected and sorted before use, which is what happens below"
)]
fn match_up_files() -> Vec<String> {
    let base = root().join("scenarios").join("arena");
    let mut stack = vec![base];
    let mut found = Vec::new();
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("scenarios/arena is readable") {
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

/// Every match-up is listed here, is read as a match-up, seats contestants,
/// and holds its event assertions on its own seed through `scenario run`.
#[test]
fn the_match_ups_are_the_whole_set_and_each_plays_its_own_seed() {
    let mut listed: Vec<String> = MATCH_UPS.iter().map(|file| (*file).to_owned()).collect();
    listed.sort();
    assert_eq!(
        match_up_files(),
        listed,
        "a match-up under scenarios/arena/ is missing from MATCH_UPS, so nothing on a pull \
         request checks it plays"
    );
    for file in MATCH_UPS {
        let loaded = scenario::load(&root(), Path::new(file)).expect("a valid match-up");
        assert_eq!(loaded.home, Home::MatchUp, "{file} is read as a match-up");
        arena::contestants_of(&loaded)
            .unwrap_or_else(|failure| panic!("{file}: {}", failure.message));
        let outcome = gamectl(&["scenario", "run", file]);
        assert_eq!(
            outcome.code,
            Exit::Ok,
            "{file}: {}{}",
            outcome.out,
            outcome.err
        );
    }
}

/// A scratch repository root holding the rules table, the placeholder
/// playbook, the seed file and one match-up, so the reader's match-up rules
/// are met by a file that really lives under `scenarios/arena/` and the
/// repository's own folder is never written.
fn scratch_root(name: &str, match_up: &str) -> PathBuf {
    let dir = scratch(name);
    for file in [
        "rules/rules.v1.json",
        "scenarios/nightly/placeholder.playbook.jsonc",
        SEEDS,
    ] {
        let to = dir.join(file);
        std::fs::create_dir_all(to.parent().expect("a parent")).expect("a directory");
        std::fs::copy(root().join(file), &to).expect("a copy");
    }
    let to = dir.join(DOCTORED);
    std::fs::create_dir_all(to.parent().expect("a parent")).expect("a directory");
    std::fs::write(&to, match_up).expect("the match-up");
    dir
}

/// The match-up a scratch root holds.
const DOCTORED: &str = "scenarios/arena/doctored.scenario.jsonc";

/// A match-up is held to its rules where it lives: no hash chain, and seats
/// that are contestants.
#[test]
fn a_match_up_with_a_chain_or_a_sealed_seat_is_refused() {
    let text = std::fs::read_to_string(root().join(THREE_WAY)).expect("the match-up");
    // The committed text, moved: the scratch root reads it as a match-up.
    let dir = scratch_root("moved", &text);
    let loaded = scenario::load(&dir, Path::new(DOCTORED)).expect("the match-up, moved");
    assert_eq!(loaded.home, Home::MatchUp);
    for (name, from, to, wanted) in [
        (
            "chain",
            "\"assertions\": [",
            "\"assertions\": [\n    { \"assert\": \"hash_chain_equals\", \"golden\": \
             \"tests/golden/scenarios/arena-result/expected.hashes.txt\" },",
            "an arena match-up carries no hash chain",
        ),
        (
            "sealed",
            "{ \"seat\": 2, \"kind\": \"safe\" }",
            "{ \"seat\": 2, \"kind\": \"sealed\", \"sealed\": \"scenarios/arena/doctored/seat-2\" }",
            "an arena match-up seats two or three contestants",
        ),
        (
            "alone",
            ",\n    { \"seat\": 1, \"kind\": \"playbook\", \"playbook\": \
             \"scenarios/nightly/placeholder.playbook.jsonc\" },\n    { \"seat\": 2, \"kind\": \
             \"safe\" }",
            "",
            "an arena match-up seats two or three contestants",
        ),
    ] {
        assert!(text.contains(from), "the match-up holds `{from}`");
        let dir = scratch_root(name, &text.replacen(from, to, 1));
        let outcome = pharmakos_gamectl::run(
            ["arena", DOCTORED, "--seeds", SEEDS]
                .iter()
                .map(|arg| (*arg).to_owned()),
            &dir,
        );
        assert_eq!(outcome.code, Exit::Input, "{name}: {}", outcome.err);
        assert!(outcome.err.contains(wanted), "{name}: {}", outcome.err);
    }
}

/// A nightly arm seats two contestants, so the arena plays it; the text form
/// says the counts in a person's words, and `--out` writes the result to a
/// file that ends its last line.
#[test]
fn an_arm_is_a_match_up_of_two_and_out_writes_the_text() {
    let dir = scratch("out");
    let out = dir.join("result.txt");
    let outcome = gamectl(&[
        "arena",
        ARM,
        "--seeds",
        SEEDS,
        "--out",
        &out.to_string_lossy(),
    ]);
    assert_eq!(outcome.code, Exit::Ok, "{}", outcome.err);
    assert!(
        outcome.out.contains("4 match(es) played") && outcome.out.contains("result.txt"),
        "{}",
        outcome.out
    );
    let written = std::fs::read_to_string(&out).expect("the result file");
    for wanted in [
        "contestant 0: the built-in operator (easy)",
        "contestant 1: the playbook scenarios/nightly/placeholder.playbook.jsonc (plan fingerprint ",
        "rotation 1, the seats holding contestants 1/0",
        "of 4 match(es), win_rate_percent",
    ] {
        assert!(written.contains(wanted), "no `{wanted}` in:\n{written}");
    }
    assert!(written.ends_with('\n'), "the file ends its last line");
}

/// The counts hold together over a played run: the winners as contestants
/// are the match's own winners seated, and every match is a win, a share or a
/// loss for every contestant.
#[test]
fn a_played_run_counts_every_match_for_every_contestant() {
    let loaded = scenario::load(&root(), Path::new(THREE_WAY)).expect("the match-up");
    let contestants = arena::contestants_of(&loaded).expect("contestants");
    let seeds = [Listed {
        seed: 0x0102_0304_0506_0708,
        line: 1,
    }];
    let result = arena::play_seeds(
        &root(),
        &loaded,
        &contestants,
        &seeds,
        Operator::Easy,
        "one seed",
    )
    .unwrap_or_else(|failure| panic!("three contestants: {}", failure.message));
    assert_eq!(result.matches.len(), 3);
    for played in &result.matches {
        assert_eq!(played.launches.len(), 3, "a list of launches a seat");
        assert_eq!(played.hunts_unlaunched.len(), 3);
        assert_eq!(
            played.ending.audit.len(),
            3,
            "the audit decides between three"
        );
        let mut from_seats: Vec<usize> = played
            .ending
            .winners
            .iter()
            .map(|seat| {
                *played
                    .contestant_by_seat
                    .get(usize::from(*seat))
                    .expect("a winner is a seat of the match")
            })
            .collect();
        from_seats.sort_unstable();
        assert_eq!(played.winning_contestants, from_seats);
    }
    assert_eq!(result.standings.len(), 3);
    for standing in &result.standings {
        assert_eq!(standing.matches, 3);
        assert_eq!(
            standing.wins + standing.shared + standing.losses,
            3,
            "every match is a win, a share or a loss"
        );
        assert_eq!(
            standing.wins_by_seat.iter().sum::<usize>(),
            standing.wins,
            "{standing:?}"
        );
    }
    // Only the playbook contestant seals a file of its own.
    let sealed: Vec<bool> = result.fingerprints.iter().map(Option::is_some).collect();
    assert_eq!(sealed, [false, true, false]);
}

/// A playbook `submit_plan` refuses fails the run with exit 3, in the arena's
/// words, naming where the contestants sat: never the harness door.
#[test]
fn a_refused_playbook_fails_the_run_and_names_the_seating() {
    let dir = scratch("refused");
    let broken = dir.join("broken.playbook.jsonc");
    std::fs::write(&broken, "{ \"format\": \"nothing\" }\n").expect("the broken playbook");
    let loaded = scenario::load(&root(), Path::new(ARM)).expect("the arm");
    let contestants = [
        Contestant::Builtin,
        Contestant::Playbook(broken.to_string_lossy().into_owned()),
    ];
    let seeds = [Listed {
        seed: 0x0102_0304_0506_0708,
        line: 4,
    }];
    let failure = arena::play_seeds(
        &root(),
        &loaded,
        &contestants,
        &seeds,
        Operator::Easy,
        "the seeds",
    )
    .expect_err("the door refuses it");
    assert_eq!(failure.code, Exit::Failed, "{}", failure.message);
    for wanted in [
        "line 4 of the seeds",
        "rotation 0, the seats holding contestants 0/1",
        "seat 1's playbook",
        "in round 1",
        "this contestant is broken",
    ] {
        assert!(
            failure.message.contains(wanted),
            "no `{wanted}` in: {}",
            failure.message
        );
    }
}

#[test]
fn a_sealed_seat_is_no_contestant() {
    let outcome = gamectl(&[
        "arena",
        "scenarios/s4/sealed-replay.scenario.jsonc",
        "--seeds",
        SEEDS,
    ]);
    assert_eq!(outcome.code, Exit::Input, "{}", outcome.err);
    assert!(outcome.err.contains("`sealed`"), "{}", outcome.err);
}

#[test]
fn operator_with_no_builtin_contestant_is_refused() {
    // A playbook seat and a `safe` seat.
    let outcome = gamectl(&[
        "arena",
        "scenarios/skeleton/deploy-and-visit.scenario.jsonc",
        "--operator",
        "easy",
        "--seeds",
        SEEDS,
    ]);
    assert_eq!(outcome.code, Exit::Input, "{}", outcome.err);
    assert!(
        outcome.err.contains("gamectl arena: `--operator`"),
        "{}",
        outcome.err
    );
}

#[test]
fn a_refused_seed_file_is_an_input_error() {
    let dir = scratch("seeds");
    let seeds = dir.join("bad.seeds.txt");
    std::fs::write(&seeds, "0x0102030405060708\nnot a seed\n").expect("the seed file");
    let outcome = gamectl(&["arena", ARM, "--seeds", &seeds.to_string_lossy()]);
    assert_eq!(outcome.code, Exit::Input, "{}", outcome.err);
    assert!(outcome.err.contains("line 2"), "{}", outcome.err);
    let missing = gamectl(&[
        "arena",
        ARM,
        "--seeds",
        &dir.join("none.txt").to_string_lossy(),
    ]);
    assert_eq!(missing.code, Exit::Input, "{}", missing.err);
}
