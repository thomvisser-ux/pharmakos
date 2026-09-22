// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The command line itself: what it accepts, what it refuses, and what it
//! exits with.
//!
//! The skeleton plan's T15 acceptance line ends "exit codes documented", and a
//! documented exit code that nothing asserts is a promise. Every code in
//! [`Exit::ALL`] is produced by a case below, so the table, the help text, the
//! generated reference and the behaviour are one thing rather than four.
//!
//! Everything runs **in process**: `pharmakos_gamectl::run` takes an argument
//! list and gives back what would have been printed and what would have been
//! exited with. No subprocess, no captured pipe, and therefore nothing to be
//! flaky about.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use pharmakos_gamectl::cli::{COMMANDS, OPTIONS};
use pharmakos_gamectl::exit::Exit;
use pharmakos_gamectl::{Outcome, run};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| panic!("crates/gamectl sits two levels below the workspace root"))
        .to_path_buf()
}

fn gamectl(args: &[&str]) -> Outcome {
    run(args.iter().map(|arg| (*arg).to_owned()), &root())
}

#[test]
fn the_help_lists_every_command_the_parser_accepts() {
    // `lexopt` generates no help (item 105 (3)), so nothing but this test
    // would notice the hand-written one going stale.
    let help = gamectl(&["--help"]);
    assert_eq!(help.code, Exit::Ok);
    for command in COMMANDS {
        assert!(
            help.out.contains(command.usage),
            "`{}` is a command and the help does not mention it",
            command.usage
        );
    }
    for (code, meaning) in Exit::ALL {
        assert!(
            help.out.contains(meaning),
            "exit code {} is documented in Exit::ALL and not in the help",
            code.code()
        );
    }
}

/// Every flag the parser accepts appears in the help **and** in the generated
/// reference.
///
/// A review found `--depth` in neither: parsed at `src/cli.rs`, exercised by
/// `tests/verify.rs`, and documented nowhere — so somebody reading `--help`
/// could not learn that a QUICK pass exists. The options are a table now, and
/// the check runs over the table rather than over a list written here, so a
/// flag added without a row fails `the_parser_accepts_exactly_the_flags_in_the_table`
/// below instead of passing quietly.
#[test]
fn the_help_and_the_reference_list_every_option() {
    let help = gamectl(&["--help"]);
    assert_eq!(help.code, Exit::Ok);
    let docs = gamectl(&["docs"]);
    assert_eq!(docs.code, Exit::Ok, "{}", docs.err);
    for option in OPTIONS {
        assert!(
            help.out.contains(option.flag),
            "`{}` is an option and the help does not mention it",
            option.flag
        );
        assert!(
            docs.out.contains(option.flag),
            "`{}` is an option and `gamectl docs` does not mention it",
            option.flag
        );
    }
    assert!(
        help.out.contains("--depth"),
        "the flag the review found missing, named on purpose so that removing the table's row \
         does not quietly remove the check with it"
    );
}

/// The table is the whole of what the parser accepts.
///
/// Read off the parser's own source text, which is the only way to catch the
/// failure this is for: a `Long("…")` arm added without a row in [`OPTIONS`]
/// is a flag the help cannot know about. `tests/confinement.rs` checks this
/// crate the same way and for the same reason.
#[test]
fn the_parser_accepts_exactly_the_flags_in_the_table() {
    let source = std::fs::read_to_string(
        root()
            .join("crates")
            .join("gamectl")
            .join("src")
            .join("cli.rs"),
    )
    .expect("the parser's source");
    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        let Some(rest) = trimmed.split("Long(\"").nth(1) else {
            continue;
        };
        let Some(name) = rest.split('"').next() else {
            continue;
        };
        let flag = format!("--{name}");
        assert!(
            OPTIONS.iter().any(|option| option.flag == flag),
            "the parser matches `{flag}` and the options table has no row for it, so neither the \
             help nor `gamectl docs` says it exists"
        );
    }
}

/// A flag is refused by a command that does not read it, rather than ignored.
#[test]
fn an_option_given_to_the_wrong_command_is_a_usage_error() {
    // Measured before the fix, all exiting 0 having done nothing: the flag was
    // parsed globally and read by one command each.
    for args in [
        &["docs", "--depth", "quick"][..],
        &[
            "verify",
            "--part",
            "step",
            "examples/playbooks/expand_east.jsonc",
        ],
        &["seat", "--depth", "quick", "doctor"],
        &["scenario", "--part", "step", "run", "a.scenario.jsonc"],
    ] {
        let outcome = gamectl(args);
        assert_eq!(outcome.code, Exit::Usage, "{args:?}: {}", outcome.out);
        assert!(
            outcome.err.contains("does nothing here"),
            "{args:?}: the refusal has to say why: {}",
            outcome.err
        );
    }
    // And each still reaches the command that does read it.
    assert_eq!(gamectl(&["schema", "--part", "step"]).code, Exit::Ok);
    assert_eq!(
        gamectl(&[
            "verify",
            "--depth",
            "quick",
            "examples/playbooks/expand_east.jsonc"
        ])
        .code,
        Exit::Ok
    );
}

#[test]
fn there_is_no_connect_and_the_refusal_says_which_version_it_belongs_to() {
    // AGENTS.md section 11. A flag that looked like one would be a promise
    // this build does not keep.
    let outcome = gamectl(&["connect"]);
    assert_eq!(outcome.code, Exit::Usage);
    assert!(outcome.err.contains("v1.1"), "{}", outcome.err);
    // The help says there is no `connect` — which is not the same as listing
    // one. The commands section must not carry a row for it.
    let help = gamectl(&["--help"]).out;
    assert!(
        help.contains("There is no `connect`"),
        "the help says so out loud, because somebody looking for it should be told rather than \
         left to infer it from an absence"
    );
    assert!(
        !COMMANDS.iter().any(|command| command.name == "connect"),
        "and the command table has no row for it"
    );
}

/// Every documented exit code, produced by something.
#[test]
fn every_documented_exit_code_is_reachable() {
    let cases: &[(Exit, &[&str])] = &[
        (Exit::Ok, &["--version"]),
        (Exit::Usage, &["fly"]),
        (Exit::Input, &["verify", "no/such/playbook.jsonc"]),
        (
            Exit::Failed,
            &[
                "verify",
                "crates/verifier/tests/cases/e0102_empty_route.json",
            ],
        ),
    ];
    for (expected, args) in cases {
        let outcome = gamectl(args);
        assert_eq!(outcome.code, *expected, "{args:?}: {}", outcome.err);
    }
    // `Exit::Internal` has no case here on purpose: every way to reach it is a
    // build that disagrees with its own generated tree, which cannot be
    // arranged from a command line without breaking the build first. It is
    // covered by `every_code_is_listed_once_and_in_order` in `exit.rs` and by
    // the error paths that return it being the only ones that can.
    assert!(
        Exit::ALL.iter().any(|(code, _)| *code == Exit::Internal),
        "and it is still documented"
    );
}

#[test]
fn a_refusal_goes_to_standard_error_and_leaves_standard_output_empty() {
    // So that `gamectl schema > file` never writes half a schema and an error
    // message into the same file.
    for args in [&["fly"][..], &["verify"], &["scenario", "run"]] {
        let outcome = gamectl(args);
        assert_ne!(outcome.code, Exit::Ok, "{args:?}");
        assert!(outcome.out.is_empty(), "{args:?} wrote to stdout");
        assert!(outcome.err.ends_with('\n'), "{args:?}");
    }
}

#[test]
fn the_scenario_help_probe_xtask_uses_exits_zero() {
    // `xtask/src/main.rs`'s `step_scenario` decides whether the runner exists
    // by running exactly this and taking a zero status as yes. If it stops
    // exiting zero the step turns itself off in silence, which is the one
    // failure mode the whole harness is built against.
    let outcome = gamectl(&["scenario", "--help"]);
    assert_eq!(
        outcome.code,
        Exit::Ok,
        "xtask's scenario step probes `gamectl scenario --help` and reads a non-zero status as \
         `no runner yet`"
    );
}

#[test]
fn schema_prints_the_same_document_get_schema_serves() {
    // Spec section 12: "get_schema and the gamectl docs output are generated
    // from the schema so they can't drift". This is the `gamectl` half of that
    // sentence, against the golden T13 committed for the gateway's half.
    let outcome = gamectl(&["schema"]);
    assert_eq!(outcome.code, Exit::Ok, "{}", outcome.err);
    let committed =
        std::fs::read_to_string(root().join("tests/golden/schema/get_schema/expected.json"))
            .expect("T13 committed get_schema's golden");
    assert_eq!(
        outcome.out,
        committed.replace("\r\n", "\n"),
        "`gamectl schema` and `get_schema` have drifted apart, which means one of them is \
         building its answer rather than serving the generated artefact"
    );
    // And a part that does not exist is the operand's fault, not the build's —
    // in **both** of the ways the gateway says so. A review measured
    // `--part Step` exiting 4, which `exit.rs` documents as "this build is
    // inconsistent with itself; nothing the caller did caused it": the
    // gateway answers `NOT_FOUND` for a well-formed unknown name and
    // `INVALID_ARGUMENT` for one that is not a lower_snake_case name at all,
    // and only the first was mapped.
    for wrong in ["nonesuch", "Step", "gp.v1.Playbook", "../../etc", "step "] {
        let outcome = gamectl(&["schema", "--part", wrong]);
        assert_eq!(
            outcome.code,
            Exit::Usage,
            "`--part {wrong}` is a misspelled operand, not a broken build: {}",
            outcome.err
        );
    }
    assert_eq!(gamectl(&["schema", "--part", "step"]).code, Exit::Ok);
}

#[test]
fn seat_doctor_passes_in_a_checkout() {
    // T21's acceptance line is "gamectl seat doctor passes from inside the
    // extracted zip"; a checkout is the only installation that exists today,
    // and a doctor that could not pass here could not pass there either.
    let outcome = gamectl(&["seat", "doctor"]);
    assert_eq!(outcome.code, Exit::Ok, "{}", outcome.out);
    for check in [
        "rules table",
        "schema",
        "verifier",
        "plan-core",
        "loopback",
        "host a match",
    ] {
        assert!(
            outcome.out.contains(check),
            "no `{check}` line:\n{}",
            outcome.out
        );
    }
    assert!(!outcome.out.contains("FAIL"), "{}", outcome.out);
}

#[test]
fn seat_doctor_fails_loudly_when_the_installation_is_not_one() {
    // The check that matters: a doctor that passed outside a checkout would be
    // a doctor that checks nothing. `--root` pointed at a directory with no
    // rules table is the cheapest way to be somewhere that is not an
    // installation.
    let outcome = run(["--root", "crates", "seat", "doctor"], &root());
    assert_eq!(outcome.code, Exit::Failed);
    assert!(outcome.err.contains("FAIL"), "{}", outcome.err);
    assert!(
        outcome.err.contains("rules/rules.v1.json"),
        "the line has to name what is missing: {}",
        outcome.err
    );
}
