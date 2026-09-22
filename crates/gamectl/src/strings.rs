// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! This crate's section of the one English string table.
//!
//! **Every string a person reads is written here and nowhere else** — the help
//! text, the version line, each refusal, each report heading. AGENTS.md §12:
//! "Diagnostic codes and user-facing strings are English and live in one string
//! table". The other crates keep the same rule (`pharmakos_verifier::strings`,
//! `pharmakos_gateway::strings`), so translation at some later stage is one
//! pass over a known set of modules rather than a grep over the tree.
//!
//! It matters more here than elsewhere for a second reason: `lexopt` generates
//! no `--help` (decisions-log item 105 (3)). The help below is hand-written, so
//! it is the *only* thing that says what this binary does — nothing derives it
//! from the parser, and nothing would notice it going stale. Two things keep it
//! honest instead: [`HELP`] is built from [`crate::exit::Exit::ALL`] and
//! [`crate::cli::COMMANDS`] at run time rather than typed out twice, and
//! `tests/cli.rs` asserts that every command the parser accepts appears in it.

use std::fmt::Write as _;

use crate::cli::COMMANDS;
use crate::exit::Exit;

/// The binary's name, as it is spelled on a command line and in its own help.
pub const BINARY: &str = "gamectl";

/// The one-line summary under the name.
pub const TAGLINE: &str = "the Pharmakos command-line client: inspect a playbook, read the \
                           schema, play a scenario headless.";

/// `gamectl --version`.
#[must_use]
pub fn version() -> String {
    format!(
        "{BINARY} {}\nverifier {}\ngateway {}\n",
        env!("CARGO_PKG_VERSION"),
        pharmakos_verifier::VERIFIER_VERSION,
        pharmakos_gateway::GATEWAY_VERSION
    )
}

/// `gamectl --help`, and what a usage error points at.
///
/// Built from the command table and the exit-code table rather than typed out,
/// so a command or a code added to either appears here without a second edit.
#[must_use]
pub fn help() -> String {
    let mut text =
        format!("{BINARY} — {TAGLINE}\n\nusage: {BINARY} <command> [options]\n\ncommands:\n");
    for command in COMMANDS {
        let _ = writeln!(text, "  {:<26}{}", command.usage, command.about);
    }
    text.push_str(
        "\noptions:\n  \
         -h, --help                print this help and exit 0\n  \
         -V, --version             print the version of this build and exit 0\n      \
         --root <dir>              the repository root every other path is read against\n                            \
         (default: the working directory)\n\
         \nexit codes:\n",
    );
    for (code, meaning) in Exit::ALL {
        let _ = writeln!(text, "  {}  {meaning}", code.code());
    }
    text.push_str(
        "\nThere is no `connect`. Nothing outside the game attaches to a seat in v1\n\
         (AGENTS.md §11), and a flag that looked like one would be a promise this\n\
         build does not keep.\n",
    );
    text
}

// ---------------------------------------------------------------------------
// Usage
// ---------------------------------------------------------------------------

/// No command at all.
#[must_use]
pub fn no_command() -> String {
    format!("{BINARY}: no command. `{BINARY} --help` lists them.")
}

/// A command this build does not have.
#[must_use]
pub fn unknown_command(given: &str) -> String {
    if given == "connect" {
        return format!(
            "{BINARY}: there is no `connect`, and there will not be one in v1. Nothing outside \
             the game attaches to a seat (AGENTS.md §11); the Seat Gateway is internal and the \
             published API is v1.1."
        );
    }
    format!("{BINARY}: `{given}` is not a command. `{BINARY} --help` lists them.")
}

/// A subcommand of a command this build does not have.
#[must_use]
pub fn unknown_subcommand(command: &str, given: &str) -> String {
    format!("{BINARY} {command}: `{given}` is not a subcommand of `{command}`.")
}

/// A command that needs an operand and was given none.
#[must_use]
pub fn missing_operand(command: &str, operand: &str) -> String {
    format!("{BINARY} {command}: no {operand} given.")
}

/// A second operand where one was expected.
#[must_use]
pub fn extra_operand(command: &str, given: &str) -> String {
    format!("{BINARY} {command}: `{given}` is one operand too many.")
}

/// Whatever `lexopt` refused, with the command it was refused under.
#[must_use]
pub fn bad_argument(error: &lexopt::Error) -> String {
    format!("{BINARY}: {error}. `{BINARY} --help` lists the options.")
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

/// A file that could not be read.
#[must_use]
pub fn unreadable(what: &str, path: &str, why: &str) -> String {
    format!("{BINARY}: the {what} at `{path}` could not be read: {why}")
}

/// A path that leaves the repository, or that is not repository-relative.
#[must_use]
pub fn path_escapes(pointer: &str, path: &str) -> String {
    format!(
        "{pointer}: `{path}` must be a path from the repository root, with forward slashes and \
         no `..`"
    )
}

/// The rules table is missing a block the checks read.
#[must_use]
pub fn rules_gap(path: &str, why: &str) -> String {
    format!(
        "{BINARY}: the rules table at `{path}` does not carry a block every check reads: {why}. \
         Tuning values are data (AGENTS.md §12), so this is a table to fix rather than a default \
         to invent."
    )
}

// ---------------------------------------------------------------------------
// verify
// ---------------------------------------------------------------------------

/// The heading of a report, naming what was verified and against what.
#[must_use]
pub fn verify_heading(path: &str, depth: &str) -> String {
    format!("{path}\n  depth        {depth}")
}

/// The line that says a playbook may be sealed.
#[must_use]
pub fn verify_qualifies(size_units: u32, budget: u32) -> String {
    format!("  qualifies    yes ({size_units} of {budget} size units)")
}

/// The line that says it may not.
#[must_use]
pub fn verify_refuses(errors: usize) -> String {
    let plural = if errors == 1 { "" } else { "s" };
    format!("  qualifies    no ({errors} error{plural})")
}

/// The `report_hash` line: the number a pre-check and the check at submit
/// compare on (spec §11).
#[must_use]
pub fn verify_report_hash(hash: &str) -> String {
    format!("  report_hash  {hash}")
}

/// One diagnostic, as a line a person reads.
#[must_use]
pub fn verify_diagnostic(severity: &str, code: &str, path: &str, message: &str) -> String {
    format!("  {severity:<7} {code} {path}\n          {message}")
}

/// The sentence under a report, saying which seat view produced it.
#[must_use]
pub fn verify_footer() -> String {
    String::from(
        "  Verified against the reference seat view (`gamectl seat doctor` prints it): one seat, \
         a core Build beacon `b_01`, a Mine beacon `b_02` tagged `east`, one known enemy beacon \
         `e_01`. A playbook is verified against a real match's frozen snapshot by the gateway, \
         not here.",
    )
}

// ---------------------------------------------------------------------------
// schema and docs
// ---------------------------------------------------------------------------

/// A `--part` that names no message of `gp.v1`.
#[must_use]
pub fn unknown_part(part: &str, why: &str) -> String {
    format!("{BINARY} schema: `{part}` is not a part of the playbook schema: {why}")
}

// ---------------------------------------------------------------------------
// seat doctor
// ---------------------------------------------------------------------------

/// One check's line.
#[must_use]
pub fn doctor_line(state: &str, name: &str, detail: &str) -> String {
    format!("{state:<5} {name:<22} {detail}")
}

/// The heading above the reference seat view, which `gamectl verify`'s own
/// footer promises this command prints.
#[must_use]
pub fn doctor_reference_heading() -> String {
    String::from(
        "\nThe seat view `gamectl verify` answers a playbook against, which is a written-down \
         one\n      (crates/gamectl/src/seat.rs). A real match's frozen snapshot is the \
         gateway's, not this:",
    )
}

/// The closing line when every check passed.
#[must_use]
pub fn doctor_ok(checks: usize) -> String {
    format!("\n{checks} checks, all well. This seat can host a match.")
}

/// The closing line when one did not.
#[must_use]
pub fn doctor_failed(failed: usize, checks: usize) -> String {
    let plural = if failed == 1 { "" } else { "s" };
    format!(
        "\n{failed} of {checks} checks failed. The line{plural} marked FAIL say what is missing; \
         nothing above them is a guess."
    )
}

// ---------------------------------------------------------------------------
// scenario
// ---------------------------------------------------------------------------

/// A scenario file that is not a valid one, with every problem in it.
#[must_use]
pub fn scenario_invalid(path: &str, problems: &[(String, String)]) -> String {
    let mut text = format!("{path} is not a valid scenario file:\n");
    for (pointer, message) in problems {
        let place = if pointer.is_empty() {
            "(document)"
        } else {
            pointer.as_str()
        };
        let _ = writeln!(text, "  {place} — {message}");
    }
    text.push_str("scenarios/README.md documents the format.");
    text
}

/// The heading a run prints before its assertions.
#[must_use]
pub fn scenario_heading(name: &str, seats: usize, ticks: u32, rules_hash: &str) -> String {
    format!("{name}: {seats} seats, {ticks} ticks played, rules_hash {rules_hash}")
}

/// One assertion that held.
#[must_use]
pub fn scenario_assertion_ok(index: usize, what: &str) -> String {
    format!("  ok    /assertions/{index}  {what}")
}

/// One assertion that did not.
#[must_use]
pub fn scenario_assertion_failed(index: usize, what: &str, why: &str) -> String {
    format!("  FAIL  /assertions/{index}  {what}\n        {why}")
}

/// How an `event_fired` assertion reads in the report.
#[must_use]
pub fn scenario_event_fired(event: &str, seat: Option<u8>, by_tick: u32) -> String {
    match seat {
        Some(seat) => format!("`{event}` for seat {seat} by tick {by_tick}"),
        None => format!("`{event}` by tick {by_tick}"),
    }
}

/// Why an `event_fired` assertion did not hold.
#[must_use]
pub fn scenario_event_missing(event: &str, seat: Option<u8>, kinds: &[String]) -> String {
    let who = match seat {
        Some(seat) => format!(" for seat {seat}"),
        None => String::new(),
    };
    if kinds.is_empty() {
        return format!("`{event}`{who} never fired, and neither did anything else");
    }
    format!(
        "`{event}`{who} never fired. What did, in first-seen order: {}",
        kinds.join(", ")
    )
}

/// Why an `event_fired` assertion did not hold *in time*.
#[must_use]
pub fn scenario_event_late(event: &str, seat: Option<u8>, first: u32, by_tick: u32) -> String {
    let who = match seat {
        Some(seat) => format!(" for seat {seat}"),
        None => String::new(),
    };
    format!("`{event}`{who} first fired at tick {first}, which is after tick {by_tick}")
}

/// How a `hash_chain_equals` assertion reads in the report.
#[must_use]
pub fn scenario_hash_chain(golden: &str) -> String {
    format!("the per-tick chain equals `{golden}`")
}

/// The chain golden is not committed yet.
#[must_use]
pub fn scenario_chain_missing(golden: &str, actual: &str) -> String {
    format!(
        "nothing is committed at `{golden}`. The run's chain was written to `{actual}`; commit it \
         with `cargo xtask golden --bless` and say in the pull request what it covers."
    )
}

/// The chain moved: which tick first disagreed, and what the two sides say.
#[must_use]
pub fn scenario_chain_moved(
    golden: &str,
    line: usize,
    expected: &str,
    actual: &str,
    expected_lines: usize,
    actual_lines: usize,
) -> String {
    format!(
        "the chain moved. `{golden}` line {line} is\n          {expected}\n        and this run's \
         is\n          {actual}\n        ({expected_lines} committed lines against \
         {actual_lines} played ticks.) tests/golden/scenarios/README.md says what a diff there \
         means; the pull request has to say which rule moved it, and re-blessing it to turn a \
         red build green is the one thing that is never the answer."
    )
}

/// A `builtin` seat, which the skeleton has no operator for.
#[must_use]
pub fn scenario_builtin_seat(pointer: &str) -> String {
    format!(
        "{pointer}: a `builtin` seat plays the built-in operator, which is T18 and does not \
         exist in this build. Use `safe`, which files the one playbook that needs no situation \
         to be safe, or `playbook`."
    )
}

/// A playbook that the verifier refused, named by the seat that sealed it.
#[must_use]
pub fn scenario_playbook_refused(seat: u8, path: &str, why: &str) -> String {
    format!("seat {seat}'s playbook `{path}` cannot be sealed: {why}")
}

/// The segment did not end when the scenario said it would.
#[must_use]
pub fn scenario_segment_short(index: usize, played: u32, wanted: u32) -> String {
    format!(
        "segment {index} ended after {played} ticks and the scenario asks for {wanted}: the match \
         stopped early, so nothing below it was played"
    )
}

/// The closing line of a run that passed.
#[must_use]
pub fn scenario_passed(name: &str, assertions: usize) -> String {
    format!("\n{name}: {assertions} assertions, all held.")
}

/// The closing line of a run that did not.
#[must_use]
pub fn scenario_failed(name: &str, failed: usize, assertions: usize) -> String {
    let plural = if failed == 1 { "" } else { "s" };
    format!("\n{name}: {failed} of {assertions} assertion{plural} did not hold.")
}

/// A seat whose playbook could not go through `submit_plan`, and had to be
/// sealed by the harness instead.
///
/// Printed at the head of every run that used that door, whatever the
/// assertions did. A chain produced behind the verifier's door is a different
/// claim from one produced in front of it, and the difference has to be on the
/// page rather than in a source comment.
#[must_use]
pub fn scenario_harness_seal(seat: u8, path: &str, codes: &str) -> String {
    format!(
        "  note  seat {seat} sealed `{path}` through the HARNESS door, not `submit_plan`: it \
         does not qualify against seat {seat}'s own frozen snapshot on this map ({codes}). The \
         match played the orders the scenario names; no client could have given them. See \
         crates/gamectl/src/scenario/run.rs and decisions-log item 103 (3)."
    )
}
