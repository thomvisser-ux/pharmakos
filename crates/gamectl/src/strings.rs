// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! This crate's section of the one English string table.
//!
//! The help text, the version line, each refusal, each report heading and every
//! sentence the reports are made of live here. AGENTS.md §12: "Diagnostic codes
//! and user-facing strings are English and live in one string table". The other
//! crates keep the same rule (`pharmakos_verifier::strings`,
//! `pharmakos_gateway::strings`), so translation at some later stage is one
//! pass over a known set of modules rather than a grep over the tree.
//!
//! # What is *not* here, said plainly
//!
//! An earlier draft of this header claimed every string a person reads is
//! written here and nowhere else, and a review found that false. Two kinds of
//! text are still written beside the code that raises them:
//!
//! * the `detail` of each `gamectl seat doctor` check, and the six check names
//!   (`crate::doctor`);
//! * the per-problem messages of the scenario format's diagnostics
//!   (`crate::scenario`) and a handful of the runner's
//!   (`crate::scenario::run`).
//!
//! Both are *structured diagnostics*: a message that only makes sense standing
//! next to the condition it describes, the way the verifier's catalogue entries
//! sit next to their checks. Moving them would put a hundred one-use functions
//! here and a hundred call sites there. `tests/confinement.rs` holds the line
//! that is actually enforceable — no message **names the binary** outside this
//! module — and says so under that name rather than standing in for a
//! guarantee nothing checks.
//!
//! PLACEHOLDER: whether the doctor details and the scenario diagnostics join a string table — owner, at the stage that first wants a second language.
//! Whether the doctor details and the scenario diagnostics belong
//! in a string table at translation time, or whether a catalogue beside the
//! checks is the right shape for them as it is for the verifier's. **OWNER**,
//! at the stage that first wants a second language (AGENTS.md §11 puts
//! translation past v1).
//!
//! It matters more here than elsewhere for a second reason: `lexopt` generates
//! no `--help` (decisions-log item 105 (3)). The help below is hand-written, so
//! it is the *only* thing that says what this binary does — nothing derives it
//! from the parser, and nothing would notice it going stale. Three things keep
//! it honest instead: [`help`] is built from [`crate::exit::Exit::ALL`],
//! [`crate::cli::COMMANDS`] and [`crate::cli::OPTIONS`] at run time rather than
//! typed out twice; `tests/cli.rs` asserts that every command and every flag
//! the parser accepts appears in it; and that last table exists because a
//! review found this prediction coming true for `--depth`, which was parsed,
//! tested and in neither the help nor `gamectl docs`.

use std::fmt::Write as _;

// The leaves are shown escaped, and never a sentence (S3's `vfyq2`, fix
// pass 5, item L9, and fix pass 6, item 1): a path, an operand or a name the
// user typed, a scenario file's names, pointers and lines, each shadowed at
// its function's top by `escaped` (whole), `quoted` (an operand, capped) or
// `quoted_pointer` (a pointer, token by token), or escaped where it is
// composed into a message (`scenario.rs`). A sentence gamectl composes, a
// `why` or a `detail`, is passed whole: escaping it would flatten its lines
// (`\u000a`) and an escaped `Quoted` ignores a column's width.

use crate::cli::{COMMANDS, OPTIONS};
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
/// Built from the command table, the **options table** and the exit-code table
/// rather than typed out, so a command, a flag or a code added to any of them
/// appears here without a second edit. The options table is the newest of the
/// three and is there because a review found `--depth` parsed, tested and
/// undocumented: a hand-written options block is exactly the thing this
/// module's header says nothing would notice going stale.
#[must_use]
pub fn help() -> String {
    let mut text =
        format!("{BINARY} — {TAGLINE}\n\nusage: {BINARY} <command> [options]\n\ncommands:\n");
    for command in COMMANDS {
        let _ = writeln!(text, "  {:<26}{}", command.usage, command.about);
    }
    text.push_str("\noptions:\n");
    for option in OPTIONS {
        let _ = writeln!(text, "  {:<26}{}", option.spelling, option_about(option));
    }
    text.push_str("\nexit codes:\n");
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

/// One option's half-line, with the command it belongs to in front of it when
/// it belongs to one.
///
/// `gamectl docs --depth quick` is now a usage error, so the help has to say
/// whose flag it is before somebody types it.
#[must_use]
pub fn option_about(option: &crate::cli::OptionRow) -> String {
    if option.command.is_empty() {
        return option.about.to_owned();
    }
    format!("{} only: {}", option.command, option.about)
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
    let connect = given == "connect";
    let given = pharmakos_proto::json::grammar::quoted(given);
    if connect {
        return format!(
            "{BINARY}: there is no `connect`, and there will not be one in v1. Nothing outside \
             the game attaches to a seat (AGENTS.md §11); the Seat Gateway is internal and the \
             published API is v1.1."
        );
    }
    format!("{BINARY}: `{given}` is not a command. `{BINARY} --help` lists them.")
}

/// `gamectl host` started with a terminal on standard output.
///
/// The announce line carries two bearer tokens, and a token printed to a
/// console is a token in a scrollback buffer (decisions-log item 107 (2)).
#[must_use]
pub fn host_on_a_terminal() -> String {
    format!(
        "{BINARY} host: standard output is a terminal, so nothing was started. This command \
         is the game's own match host: the lobby starts it with a pipe on both ends, and its \
         one line of output carries the match's tokens, which must never reach a console."
    )
}

/// `gamectl host`'s line for a built-in seat's round with something refused
/// (`crate::host::refusals_said`): the seat, the round, and what was refused.
#[must_use]
pub fn operator_round(seat: u8, round: u32, what: &str) -> String {
    format!("Easy, seat {seat}, round {round}: {what}")
}

/// The refusal that kept a built-in seat's round from being read.
#[must_use]
pub fn operator_read_refused(method: &str, code: &str, message: &str) -> String {
    format!(
        "the round was not read, because the gateway refused {method} with {code} ({message}), \
         so it planned nothing."
    )
}

/// The tail of a built-in seat's line that follows lines standard error did
/// not take: how many, by reason.
#[must_use]
pub fn operator_unsaid(earlier: &[(&str, String)]) -> String {
    let each: Vec<String> = earlier
        .iter()
        .map(|(reason, count)| format!("{reason} {count}"))
        .collect();
    format!("(Earlier lines not written: {}.)", each.join(", "))
}

/// `gamectl host`'s last line, for the built-in seats' lines standard error
/// did not take and no later line said: how many, by reason.
#[must_use]
pub fn operator_unsaid_at_close(earlier: &[(&str, String)]) -> String {
    let each: Vec<String> = earlier
        .iter()
        .map(|(reason, count)| format!("{reason} {count}"))
        .collect();
    format!(
        "The match host ended with built-in seats' lines not written: {}.",
        each.join(", ")
    )
}

/// A built-in seat's Lull shows no Push in its feed (one resumed from a save),
/// so which of its stones failed is not known.
#[must_use]
pub fn operator_push_not_in_feed() -> String {
    String::from(
        "The last Push is not in this Lull's feed (a Lull resumed from a save opens with an \
         empty one), so whether a stone failed in it is not known and every stone was weighed.",
    )
}

/// A built-in seat's failed steps were not all read within the pages a round
/// reads.
#[must_use]
pub fn operator_failures_past_pages(pages: u32) -> String {
    format!(
        "The last Push's failed steps were not all read within {pages} pages, so a stone whose \
         failure was not read was weighed."
    )
}

/// A built-in seat's steps failed in the last Push, and the gateway carried
/// no seal of its own into the Lull to say which were stones.
#[must_use]
pub fn operator_no_carried_seal() -> String {
    String::from(
        "Steps failed in the last Push, and the gateway carried no seal of the seat's own into \
         this Lull, so which were stones is not known and every stone was weighed.",
    )
}

/// A built-in seat's carried seal is stamped with another round.
#[must_use]
pub fn operator_carried_of_round(round: i64) -> String {
    format!(
        "Steps failed in the last Push, and the seal the gateway carried is stamped with round \
         {round}, so which were stones is not known and every stone was weighed."
    )
}

/// A built-in seat's carried seal was refused or does not read.
#[must_use]
pub fn operator_carried_unread(method: &str, code: &str, message: &str) -> String {
    format!(
        "The gateway refused {method} with {code} ({message}), so which of the last Push's \
         failed steps were stones is not known and every stone was weighed."
    )
}

/// The route estimates a built-in seat's round had refused for another reason
/// than `NOT_FOUND`, each as its code and message.
#[must_use]
pub fn operator_estimates_refused(refused: &[(&str, &str)]) -> String {
    let each: Vec<String> = refused
        .iter()
        .map(|(code, message)| format!("{code} ({message})"))
        .collect();
    let count = match refused.len() {
        1 => String::from("1 route estimate was"),
        many => format!("{many} route estimates were"),
    };
    format!(
        "{count} refused, each dropping the goal it weighed: {}.",
        each.join("; ")
    )
}

/// `gamectl host` could not start or keep its match.
#[must_use]
pub fn host_refused(why: &str) -> String {
    format!("{BINARY} host: the match could not be hosted: {why}")
}

/// A subcommand of a command this build does not have.
#[must_use]
pub fn unknown_subcommand(command: &str, given: &str) -> String {
    let given = pharmakos_proto::json::grammar::quoted(given);
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
    let given = pharmakos_proto::json::grammar::quoted(given);
    format!("{BINARY} {command}: `{given}` is one operand too many.")
}

/// A `--depth` that is neither of the two the verifier has.
///
/// Here rather than in the parser, which is where a review found it: the
/// parser built it from [`BINARY`] and the confinement test's needle was
/// looking for the literal name.
#[must_use]
pub fn bad_depth(given: &str) -> String {
    let given = pharmakos_proto::json::grammar::quoted(given);
    format!("{BINARY}: `--depth {given}` is neither `quick` nor `full`.")
}

/// A flag given to a command that does not read it.
///
/// Refused rather than ignored, which is the stance the rest of this crate
/// already takes: an extra operand is an error ([`extra_operand`]) and an
/// unknown key in a scenario file is an error, and a flag that does nothing is
/// the third way of typing something that will not happen.
#[must_use]
pub fn flag_elsewhere(command: &str, flag: &str, owners: &[&str]) -> String {
    let owners: Vec<String> = owners.iter().map(|owner| format!("`{owner}`'s")).collect();
    format!(
        "{BINARY} {command}: `{flag}` is {} option and does nothing here. It is refused \
         rather than ignored, so that a command line that reads as though it asked for something \
         never quietly did not.",
        owners.join(" or ")
    )
}

/// A flag given twice.
#[must_use]
pub fn flag_repeated(flag: &str) -> String {
    format!(
        "{BINARY}: `{flag}` is given twice. It is refused rather than the second value \
         quietly winning: give it once."
    )
}

/// The working directory could not be read, so no relative path can be.
#[must_use]
pub fn no_working_directory(why: &str) -> String {
    format!(
        "{BINARY}: the working directory cannot be read ({why}), and every path is read against it."
    )
}

/// The operating system would not make the thread a command runs on.
#[must_use]
pub fn no_command_thread(why: &str) -> String {
    format!("{BINARY}: the command's thread could not be started ({why}), so nothing ran.")
}

/// The command's thread ended in a panic: a bug, whatever the input.
#[must_use]
pub fn command_panicked() -> String {
    format!("{BINARY}: the command stopped on an internal error; this is a bug.")
}

/// A flag's value that is none of the values it takes, naming them.
#[must_use]
pub fn bad_value(flag: &str, given: &str, values: &[&str]) -> String {
    let given = pharmakos_proto::json::grammar::quoted(given);
    let named: Vec<String> = values.iter().map(|value| format!("`{value}`")).collect();
    format!(
        "{BINARY}: `{flag} {given}` is not a value it takes; it takes {}.",
        named.join(" or ")
    )
}

/// A flag that only means something beside another one, given without it.
#[must_use]
pub fn flag_needs(command: &str, flag: &str, needs: &str) -> String {
    format!(
        "{BINARY} {command}: `{flag}` shapes the result a run over `{needs}` writes, and there is \
         no `{needs}` here. It is refused rather than ignored."
    )
}

/// Whatever `lexopt` refused, with the command it was refused under.
#[must_use]
pub fn bad_argument(error: &lexopt::Error) -> String {
    format!("{BINARY}: {error}. `{BINARY} --help` lists the options.")
}

// ---------------------------------------------------------------------------
// Input and output
// ---------------------------------------------------------------------------

/// Standard output could not be written — a full disc, a read-only target.
///
/// Not a closed pipe, which is ordinary (`gamectl docs | head`) and is not a
/// failure of the command. This one is: `gamectl docs > file` that reported
/// success over a truncated file is a generated reference nobody would know to
/// distrust.
#[must_use]
pub fn unwritable_output(why: &str) -> String {
    format!(
        "{BINARY}: standard output could not be written, so what it printed is incomplete: {why}"
    )
}

/// A file that could not be read.
#[must_use]
pub fn unreadable(what: &str, path: &str, why: &str) -> String {
    let path = pharmakos_proto::json::grammar::escaped(path);
    format!("{BINARY}: the {what} at `{path}` could not be read: {why}")
}

/// A path that leaves the repository, or that is not repository-relative.
///
/// No pointer of its own: every caller is a scenario-format problem, and
/// [`scenario_invalid`] renders the pointer in front of the message. Printing
/// it twice is what a review found here.
#[must_use]
pub fn path_escapes(path: &str) -> String {
    let path = pharmakos_proto::json::grammar::escaped(path);
    format!("`{path}` must be a path from the repository root, with forward slashes and no `..`")
}

/// The rules table is missing a block the checks read.
#[must_use]
pub fn rules_gap(path: &str, why: &str) -> String {
    let path = pharmakos_proto::json::grammar::escaped(path);
    format!(
        "{BINARY}: the rules table at `{path}` does not carry a block every check reads: {why}. \
         Tuning values are data (AGENTS.md §12), so this is a table to fix rather than a default \
         to invent."
    )
}

// ---------------------------------------------------------------------------
// verify
// ---------------------------------------------------------------------------

/// The heading of a report, naming what was verified, how deep, and against
/// which view (S3's plan, E19): `view` is [`verify_view_reference`] or
/// [`verify_view_preset`]. No line of it begins with the word `error`, which
/// the headless watch check reads as a diagnostic (`watch_check.gd`).
#[must_use]
pub fn verify_heading(path: &str, depth: &str, view: &str) -> String {
    let path = pharmakos_proto::json::grammar::escaped(path);
    format!("{path}\n  depth        {depth}\n  view         {view}")
}

/// The heading's view when the file is verified against the reference seat
/// view ([`crate::seat`]), the default.
#[must_use]
pub fn verify_view_reference() -> String {
    format!("the reference view (not a match's snapshot; `{BINARY} seat doctor` prints it)")
}

/// The heading's view under `--preset`: the lobby's match of that preset,
/// its seed, and the seat whose opening Lull snapshot it is.
#[must_use]
pub fn verify_view_preset(preset: &str, seed: u64, seat: u8) -> String {
    format!("{preset}, seed {seed}, seat {seat}'s opening Lull snapshot")
}

/// The sentence after the diagnostics of a reference-view report that
/// carries `E0403`: in a match the beacon may be legal (decisions-log item
/// 117 (1); `crate::seat`). Composed from the report's codes, never by
/// checking anything again.
#[must_use]
pub fn verify_e0403_note() -> String {
    String::from(
        "  E0403 here is against the reference view. In a match the gateway checks against the \
         seat's own frozen snapshot, where this beacon may be legal.",
    )
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
///
/// The path is shown through the quoting rule token by token (S3's `vfyq2`,
/// fix pass 2, item B, and 2b): it is exact as data in the report, and a
/// pointer of the schema's names is printed exactly, however long, because a
/// reader takes it back off this line; only a token that is the player's own
/// text (an unknown key) is escaped and capped, since a terminal is not the
/// place for its ESC, BEL or U+202E. The message is the verifier's, already
/// quoted where it carries player text.
#[must_use]
pub fn verify_diagnostic(severity: &str, code: &str, path: &str, message: &str) -> String {
    let path = pharmakos_proto::json::grammar::quoted_pointer(path);
    format!("  {severity:<7} {code} {path}\n          {message}")
}

/// The sentence under a report verified against a preset's match, saying
/// where the view came from.
#[must_use]
pub fn verify_footer_preset(preset: &str, seed: u64, seats: u32, seat: u8) -> String {
    format!(
        "  Verified by the gateway against seat {seat}'s frozen snapshot in the opening Lull of \
         the {preset} match the lobby opens (seed {seed}, {seats} seats), hosted here as the \
         lobby hosts it. A later Lull's snapshot is that round's own."
    )
}

/// The lobby's config line, which this build copies, refused by this
/// build's gateway.
#[must_use]
pub fn lobby_line_refused(why: &str) -> String {
    format!(
        "{BINARY} verify --preset: the lobby's config line this build carries is one its own \
         gateway refuses: {why}"
    )
}

/// The lobby's match would not open, or the gateway refused a host's call.
#[must_use]
pub fn lobby_match_refused(why: &str) -> String {
    format!("{BINARY} verify --preset: the lobby's match would not open here: {why}")
}

/// A file past the size cap in bytes, under `--preset`: refused by this
/// command, read no further than the bound, with the file's size when the
/// file system says it (`size`), and no figure made up when it does not.
#[must_use]
pub fn verify_preset_too_long(path: &str, size: Option<u64>, read: usize, chars: usize) -> String {
    let path = pharmakos_proto::json::grammar::escaped(path);
    let size = match size {
        Some(bytes) => format!("is {bytes} bytes"),
        None => String::from("is longer"),
    };
    format!(
        "{BINARY} verify --preset: `{path}` {size}: past {read} bytes, a playbook is past the \
         {chars}-character cap whatever the bytes hold. It was read no further than that, and \
         the gateway was not asked."
    )
}

/// The gateway refused the text before verifying it, as the lobby's match
/// would: the answer, not an error.
#[must_use]
pub fn lobby_text_refused(why: &str) -> String {
    format!(
        "{BINARY} verify --preset: the gateway refuses this playbook before verifying it, as \
         the lobby's match would: {why}"
    )
}

/// The gateway's `verify_plan` answer is not a report this build reads.
#[must_use]
pub fn lobby_answer_unread(why: &str) -> String {
    format!(
        "{BINARY} verify --preset: the gateway's `verify_plan` answer is not a report this \
         build reads: {why}"
    )
}

/// The sentence under a report, saying which seat view produced it.
#[must_use]
pub fn verify_footer() -> String {
    String::from(
        "  Verified against the reference seat view (`gamectl seat doctor` prints it): one seat, \
         a core Build beacon `b_01` (the fixture's own id; a live seat's core is `b_00`), a Mine \
         beacon `b_02` tagged `east`, one known enemy beacon `e_01`. A real match's frozen \
         snapshot is the gateway's: `--preset probation` has it verify against the lobby's \
         Probation match.",
    )
}

// ---------------------------------------------------------------------------
// schema and docs
// ---------------------------------------------------------------------------

/// A `--part` that names no message of `gp.v1`.
///
/// The gateway's own sentence with this binary's name in front of it, and not
/// a second copy of it: `schema::text` already answers "`x` is not a part of
/// the playbook schema", and the draft this replaces said it twice.
#[must_use]
pub fn unknown_part(why: &str) -> String {
    format!("{BINARY} schema: {why}")
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
    let path = pharmakos_proto::json::grammar::escaped(path);
    let mut text = format!("{path} is not a valid scenario file:\n");
    for (pointer, message) in problems {
        let place = if pointer.is_empty() {
            "(document)".to_owned()
        } else {
            pharmakos_proto::json::grammar::quoted_pointer(pointer).to_string()
        };
        let _ = writeln!(text, "  {place} — {message}");
    }
    text.push_str("scenarios/README.md documents the format.");
    text
}

/// A scenario whose segments add up to more ticks than a `u32` counts.
#[must_use]
pub fn scenario_too_long(source: &str) -> String {
    let source = pharmakos_proto::json::grammar::escaped(source);
    format!(
        "{BINARY} scenario run: `{source}` lists segments that add up to more ticks than this \
         runner counts (a u32)."
    )
}

/// The heading a run prints before its assertions.
#[must_use]
pub fn scenario_heading(name: &str, seats: usize, ticks: u32, rules_hash: &str) -> String {
    let name = pharmakos_proto::json::grammar::escaped(name);
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
    let event = pharmakos_proto::json::grammar::escaped(event);
    match seat {
        Some(seat) => format!("`{event}` for seat {seat} by tick {by_tick}"),
        None => format!("`{event}` by tick {by_tick}"),
    }
}

/// Why an `event_fired` assertion did not hold.
#[must_use]
pub fn scenario_event_missing(event: &str, seat: Option<u8>, kinds: &[String]) -> String {
    let event = pharmakos_proto::json::grammar::escaped(event);
    let who = match seat {
        Some(seat) => format!(" for seat {seat}"),
        None => String::new(),
    };
    if kinds.is_empty() {
        return format!("`{event}`{who} never fired, and neither did anything else");
    }
    format!(
        "`{event}`{who} never fired. What did, in first-seen order: {}",
        pharmakos_proto::json::grammar::escaped(&kinds.join(", "))
    )
}

/// Why an `event_fired` assertion did not hold *in time*.
#[must_use]
pub fn scenario_event_late(event: &str, seat: Option<u8>, first: u32, by_tick: u32) -> String {
    let event = pharmakos_proto::json::grammar::escaped(event);
    let who = match seat {
        Some(seat) => format!(" for seat {seat}"),
        None => String::new(),
    };
    format!("`{event}`{who} first fired at tick {first}, which is after tick {by_tick}")
}

/// How a `hash_chain_equals` assertion reads in the report.
#[must_use]
pub fn scenario_hash_chain(golden: &str) -> String {
    let golden = pharmakos_proto::json::grammar::escaped(golden);
    format!("the per-tick chain equals `{golden}`")
}

/// A run whose fresh chain has nowhere to be written.
#[must_use]
pub fn scenario_chain_unwritable(golden: &str, why: &str) -> String {
    let golden = pharmakos_proto::json::grammar::escaped(golden);
    format!(
        "{BINARY} scenario run: this run's chain for `{golden}` has nowhere to be written, so \
         the `golden` step could not compare it: {why}"
    )
}

/// The chain golden is not committed yet.
#[must_use]
pub fn scenario_chain_missing(golden: &str, actual: &str) -> String {
    let golden = pharmakos_proto::json::grammar::escaped(golden);
    let actual = pharmakos_proto::json::grammar::escaped(actual);
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
    let golden = pharmakos_proto::json::grammar::escaped(golden);
    let expected = pharmakos_proto::json::grammar::escaped(expected);
    let actual = pharmakos_proto::json::grammar::escaped(actual);
    format!(
        "the chain moved. `{golden}` line {line} is\n          {expected}\n        and this run's \
         is\n          {actual}\n        ({expected_lines} committed lines against \
         {actual_lines} played ticks.) tests/golden/scenarios/README.md says what a diff there \
         means; the pull request has to say which rule moved it, and re-blessing it to turn a \
         red build green is the one thing that is never the answer."
    )
}

/// A `builtin` seat that Easy cannot be seated for: the rules text the
/// scenario names lacks a row the operator scores with.
#[must_use]
pub fn scenario_builtin_unseated(pointer: &str, rules: &str, why: &str) -> String {
    let pointer = pharmakos_proto::json::grammar::quoted_pointer(pointer);
    let rules = pharmakos_proto::json::grammar::escaped(rules);
    format!(
        "{pointer}: a `builtin` seat plays the built-in operator, Easy, which cannot score with \
         the rules table `{rules}` this scenario names: {why}. Name a rules table with the rows \
         the operator reads, or make the seat `safe` or `playbook`."
    )
}

/// A playbook that the verifier refused, named by the seat that sealed it.
#[must_use]
pub fn scenario_playbook_refused(seat: u8, path: &str, why: &str) -> String {
    let path = pharmakos_proto::json::grammar::escaped(path);
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
    let name = pharmakos_proto::json::grammar::escaped(name);
    format!("\n{name}: {assertions} assertions, all held.")
}

/// The closing line of a run that did not.
#[must_use]
pub fn scenario_failed(name: &str, failed: usize, assertions: usize) -> String {
    let name = pharmakos_proto::json::grammar::escaped(name);
    // The suffix belongs to `assertions`, which is the noun it is attached to:
    // "1 of 9 assertions did not hold", never "1 of 9 assertion".
    let plural = if assertions == 1 { "" } else { "s" };
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
    let path = pharmakos_proto::json::grammar::escaped(path);
    format!(
        "  note  seat {seat} sealed `{path}` through the HARNESS door, not `submit_plan`: it \
         does not qualify against seat {seat}'s own frozen snapshot on this map ({codes}). The \
         match played the orders the scenario names; no client could have given them. See \
         crates/gamectl/src/scenario/run.rs and decisions-log item 103 (3)."
    )
}

// ---------------------------------------------------------------------------
// scenario run --seeds (the nightly harness; S2's plan, task `night`)
// ---------------------------------------------------------------------------

/// An arm's playbook that `submit_plan` refused during a run over a seed
/// file. A nightly result is a claim about a match a player could have
/// played, so the harness door is not taken.
#[must_use]
pub fn nightly_arm_refused(seat: u8, path: &str, round: u32, codes: &str) -> String {
    let path = pharmakos_proto::json::grammar::escaped(path);
    format!(
        "seat {seat}'s playbook `{path}` was refused by `submit_plan` in round {round} ({codes}). \
         A run over a seed file plays every playbook through the verifier's door and never the \
         harness's, so the arm is broken on this seed rather than a result."
    )
}

/// A scenario run over a seed file that does not seat one `playbook` seat
/// and one `builtin` seat.
#[must_use]
pub fn nightly_layout(path: &str) -> String {
    let path = pharmakos_proto::json::grammar::escaped(path);
    format!(
        "{path}: a run over a seed file plays an arm against the built-in operator, so the \
         scenario seats exactly two seats, one `playbook` seat (the arm) and one `builtin` seat \
         (S2's plan, decision 29)."
    )
}

/// `--operator` given for a scenario that seats no `builtin` seat.
#[must_use]
pub fn operator_unused(path: &str) -> String {
    let path = pharmakos_proto::json::grammar::escaped(path);
    format!(
        "{BINARY} scenario run: `--operator` names the operator a `builtin` seat plays, and \
         `{path}` seats none. It is refused rather than ignored."
    )
}

/// The seed file, refused, with the decoder's own sentence.
#[must_use]
pub fn seeds_refused(path: &str, why: &str) -> String {
    let path = pharmakos_proto::json::grammar::escaped(path);
    format!("the seed file `{path}` cannot be read as one: {why}.")
}

/// The result could not be written to `--out`.
#[must_use]
pub fn result_unwritable(path: &str, why: &str) -> String {
    let path = pharmakos_proto::json::grammar::escaped(path);
    format!("the result could not be written to `{path}`: {why}")
}

/// The line a run over a seed file prints when its result went to a file.
#[must_use]
pub fn result_written(path: &str, seeds: usize, wins: usize) -> String {
    let path = pharmakos_proto::json::grammar::escaped(path);
    format!("{seeds} seed(s) played, the arm the sole winner of {wins}; the result is in `{path}`.")
}
