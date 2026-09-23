// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The command line: what this binary accepts, and nothing else.
//!
//! Parsing is `lexopt` (decisions-log item 105 (3)), which is a parser and not
//! a framework: it has no dependencies, no proc macros and **no generated
//! `--help`**. The help is hand-written in [`crate::strings`] and built from
//! [`COMMANDS`] and [`OPTIONS`] below, so the two tables here are the single
//! list of what exists — the parser reads them, the help prints them,
//! `gamectl docs` prints them, and `tests/cli.rs` asserts that all of those
//! agree. [`OPTIONS`] is the younger of the two and exists because a review
//! found `--depth` parsed, tested and in neither the help nor the reference.
//!
//! # There is no `connect`
//!
//! AGENTS.md §11: nothing outside the game attaches to a seat in v1. An
//! unknown command is a usage error, and `connect` gets a sentence of its own
//! ([`crate::strings::unknown_command`]) rather than the generic one, because
//! somebody typing it has read a roadmap and deserves to be told which version
//! it belongs to rather than that they made a typo.
//!
//! # `--help` wins wherever it appears
//!
//! `gamectl scenario --help` prints the help and exits 0, and so does
//! `gamectl --help scenario`. That is not only a courtesy: `cargo xtask ci`'s
//! `scenario` step probes this binary with exactly `scenario --help` to decide
//! whether the runner exists yet, and takes a zero status as "it does"
//! (`xtask/src/main.rs`, `step_scenario`). Changing what that probe returns
//! turns the step off in silence.

use std::ffi::OsString;
use std::path::PathBuf;

use pharmakos_proto::gp::api::v1::verify_plan::Depth;

use crate::exit::Failure;
use crate::strings;

/// One row of the command table: what it is called, how it is spelled in the
/// help, and the half-line under it.
#[derive(Clone, Copy, Debug)]
pub struct CommandRow {
    /// The first operand that selects it.
    pub name: &'static str,
    /// How the help spells the whole invocation.
    pub usage: &'static str,
    /// What it does, in one half-line.
    pub about: &'static str,
}

/// Every command, in the order the help prints them.
///
/// Spec §12's list, in full: `verify`, `schema`, `docs`, `scenario run`,
/// `seat doctor` — and `host`, the child process the Godot lobby spawns
/// (decisions-log item 107 (2)). Nothing else, and in particular no `connect`.
pub const COMMANDS: &[CommandRow] = &[
    CommandRow {
        name: "verify",
        usage: "verify <playbook.jsonc>",
        about: "inspect a playbook and print its report and report_hash",
    },
    CommandRow {
        name: "schema",
        usage: "schema [--part <name>]",
        about: "print the JSON Schema a seat authors against, generated from gp.v1",
    },
    CommandRow {
        name: "docs",
        usage: "docs",
        about: "print the generated reference: the schema's shape and every diagnostic",
    },
    CommandRow {
        name: "scenario",
        usage: "scenario run <file>",
        about: "play a scenario headless and check its assertions on events and hashes",
    },
    CommandRow {
        name: "seat",
        usage: "seat doctor",
        about: "check this installation and report what is wrong",
    },
    CommandRow {
        name: "host",
        usage: "host",
        about: "serve one match to the game client over this process's stdio pipe; the \
                lobby starts it, and it refuses a terminal",
    },
];

/// One row of the options table: how the help spells a flag, whose it is, and
/// the half-line under it.
///
/// It exists because a review found `--depth` — a real, parsed, tested flag —
/// in neither `gamectl --help` nor `gamectl docs`, which is exactly the
/// staleness [`crate::strings`]'s own header says nothing would notice. The
/// options are now a table for the same reason the commands are: the parser
/// reads it, the help prints it, `docs` prints it, and `tests/cli.rs` asserts
/// that every flag the parser accepts has a row.
#[derive(Clone, Copy, Debug)]
pub struct OptionRow {
    /// How the help spells it, long form and short form together.
    pub spelling: &'static str,
    /// The long flag the parser matches on, which is also what a refusal
    /// names.
    pub flag: &'static str,
    /// The command it belongs to, or the empty string when it belongs to all
    /// of them.
    pub command: &'static str,
    /// What it does, in one half-line.
    pub about: &'static str,
}

/// Every option, in the order the help prints them.
pub const OPTIONS: &[OptionRow] = &[
    OptionRow {
        spelling: "-h, --help",
        flag: "--help",
        command: "",
        about: "print this help and exit 0",
    },
    OptionRow {
        spelling: "-V, --version",
        flag: "--version",
        command: "",
        about: "print the version of this build and exit 0",
    },
    OptionRow {
        spelling: "    --root <dir>",
        flag: "--root",
        command: "",
        about: "the repository root every other path is read against (default: the working \
                directory)",
    },
    OptionRow {
        spelling: "    --part <name>",
        flag: "--part",
        command: "schema",
        about: "one message of gp.v1 by its lower_snake_case name, instead of the whole playbook",
    },
    OptionRow {
        spelling: "    --depth <quick|full>",
        flag: "--depth",
        command: "verify",
        about: "how deep to run the pipeline (default: full, which is what submit_plan runs)",
    },
];

/// What was asked for.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Command {
    /// Print the help. Exit 0.
    Help,
    /// Print the version of this build. Exit 0.
    Version,
    /// Inspect one playbook file.
    Verify {
        /// The `.jsonc` file to inspect.
        path: PathBuf,
        /// How deep to run the pipeline. FULL by default, which is what
        /// `submit_plan` always runs (spec §11).
        depth: Depth,
    },
    /// Print the generated JSON Schema for one part of `gp.v1`.
    Schema {
        /// The part's `lower_snake_case` short name, or empty for the whole
        /// playbook.
        part: String,
    },
    /// Print the generated reference documentation.
    Docs,
    /// Play one scenario file.
    ScenarioRun {
        /// The `.scenario.jsonc` file to play.
        path: PathBuf,
    },
    /// Check this installation.
    SeatDoctor,
    /// Serve one match to the Godot client over this process's own stdio
    /// pipe, until standard input closes (decisions-log item 107 (2)).
    Host,
}

/// One whole command line, parsed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Invocation {
    /// The repository root every relative path is read against. The working
    /// directory unless `--root` said otherwise.
    ///
    /// It exists because `cargo xtask ci` invokes this binary through
    /// `cargo run` from the workspace root and passes repository-relative
    /// paths, and because an integration test has no working directory of its
    /// own to lean on.
    pub root: PathBuf,
    /// What to do.
    pub command: Command,
}

/// Parses a command line.
///
/// `args` is the argument list **without** the program name, as
/// `std::env::args_os().skip(1)` gives it.
///
/// # Errors
///
/// [`crate::exit::Exit::Usage`], always: everything this function can object
/// to is something the caller typed.
pub fn parse<I>(args: I, cwd: &std::path::Path) -> Result<Invocation, Failure>
where
    I: IntoIterator,
    I::Item: Into<OsString>,
{
    use lexopt::prelude::{Long, Short, Value};

    let mut parser = lexopt::Parser::from_args(args);
    let mut root: Option<PathBuf> = None;
    let mut part: Option<String> = None;
    // `None` rather than `Depth::Full`, so that "the caller said nothing" and
    // "the caller said `full`" are different facts: a review found `--depth`
    // silently ignored on every command but `verify`, and a default cannot be
    // refused.
    let mut depth: Option<Depth> = None;
    let mut operands: Vec<OsString> = Vec::new();

    loop {
        let next = parser
            .next()
            .map_err(|error| Failure::usage(strings::bad_argument(&error)))?;
        let Some(arg) = next else { break };
        match arg {
            Short('h') | Long("help") => {
                return Ok(Invocation {
                    root: cwd.to_path_buf(),
                    command: Command::Help,
                });
            }
            Short('V') | Long("version") => {
                return Ok(Invocation {
                    root: cwd.to_path_buf(),
                    command: Command::Version,
                });
            }
            Long("root") => {
                let value = parser
                    .value()
                    .map_err(|error| Failure::usage(strings::bad_argument(&error)))?;
                root = Some(PathBuf::from(value));
            }
            Long("part") => {
                let value = parser
                    .value()
                    .map_err(|error| Failure::usage(strings::bad_argument(&error)))?;
                part = Some(value.to_string_lossy().into_owned());
            }
            Long("depth") => {
                let value = parser
                    .value()
                    .map_err(|error| Failure::usage(strings::bad_argument(&error)))?;
                depth = match value.to_string_lossy().as_ref() {
                    "quick" => Some(Depth::Quick),
                    "full" => Some(Depth::Full),
                    other => return Err(Failure::usage(strings::bad_depth(other))),
                };
            }
            Value(value) => operands.push(value),
            other => {
                return Err(Failure::usage(strings::bad_argument(&other.unexpected())));
            }
        }
    }

    let root = root.unwrap_or_else(|| cwd.to_path_buf());
    let command = command_of(&operands, part, depth)?;
    Ok(Invocation { root, command })
}

/// The operands, once the flags are out of the way.
fn command_of(
    operands: &[OsString],
    part: Option<String>,
    depth: Option<Depth>,
) -> Result<Command, Failure> {
    let Some(first) = operands.first() else {
        return Err(Failure::usage(strings::no_command()));
    };
    let name = first.to_string_lossy().into_owned();
    let rest = operands.get(1..).unwrap_or_default();
    // A flag that means nothing to this command is refused, not ignored — the
    // stance this crate already takes on an extra operand and on an unknown
    // key in a scenario file, applied to the third way of typing something
    // that will not happen. A review found `gamectl docs --depth quick`
    // exiting 0 having done nothing with it.
    if COMMANDS.iter().any(|row| row.name == name) {
        flags_belong_to(&name, part.is_some(), depth.is_some())?;
    }

    match name.as_str() {
        "verify" => {
            let path = one_operand("verify", "playbook", rest)?;
            Ok(Command::Verify {
                path,
                // FULL by default, which is what `submit_plan` always runs
                // (spec §11).
                depth: depth.unwrap_or(Depth::Full),
            })
        }
        "schema" => {
            no_operands("schema", rest)?;
            Ok(Command::Schema {
                part: part.unwrap_or_default(),
            })
        }
        "docs" => {
            no_operands("docs", rest)?;
            Ok(Command::Docs)
        }
        "scenario" => {
            let Some(sub) = rest.first() else {
                return Err(Failure::usage(strings::missing_operand(
                    "scenario",
                    "subcommand (`run`)",
                )));
            };
            let sub = sub.to_string_lossy().into_owned();
            if sub != "run" {
                return Err(Failure::usage(strings::unknown_subcommand(
                    "scenario", &sub,
                )));
            }
            let path = one_operand(
                "scenario run",
                "scenario file",
                rest.get(1..).unwrap_or_default(),
            )?;
            Ok(Command::ScenarioRun { path })
        }
        "seat" => {
            let Some(sub) = rest.first() else {
                return Err(Failure::usage(strings::missing_operand(
                    "seat",
                    "subcommand (`doctor`)",
                )));
            };
            let sub = sub.to_string_lossy().into_owned();
            if sub != "doctor" {
                return Err(Failure::usage(strings::unknown_subcommand("seat", &sub)));
            }
            no_operands("seat doctor", rest.get(1..).unwrap_or_default())?;
            Ok(Command::SeatDoctor)
        }
        "host" => {
            no_operands("host", rest)?;
            Ok(Command::Host)
        }
        other => Err(Failure::usage(strings::unknown_command(other))),
    }
}

/// Every flag given belongs to the command it was given to.
///
/// The general options ([`OPTIONS`] rows with an empty `command`) belong to
/// all of them and are not checked here; the two that belong to one command
/// each are.
fn flags_belong_to(command: &str, part: bool, depth: bool) -> Result<(), Failure> {
    for (flag, given) in [("--part", part), ("--depth", depth)] {
        if !given {
            continue;
        }
        let owner = OPTIONS
            .iter()
            .find(|row| row.flag == flag)
            .map_or("", |row| row.command);
        if command != owner {
            return Err(Failure::usage(strings::flag_elsewhere(
                command, flag, owner,
            )));
        }
    }
    Ok(())
}

/// Exactly one operand, or a usage error naming what was wanted.
fn one_operand(command: &str, operand: &str, rest: &[OsString]) -> Result<PathBuf, Failure> {
    let Some(first) = rest.first() else {
        return Err(Failure::usage(strings::missing_operand(command, operand)));
    };
    if let Some(extra) = rest.get(1) {
        return Err(Failure::usage(strings::extra_operand(
            command,
            &extra.to_string_lossy(),
        )));
    }
    Ok(PathBuf::from(first))
}

/// No operands at all.
fn no_operands(command: &str, rest: &[OsString]) -> Result<(), Failure> {
    if let Some(extra) = rest.first() {
        return Err(Failure::usage(strings::extra_operand(
            command,
            &extra.to_string_lossy(),
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{COMMANDS, Command, parse};
    use crate::exit::Exit;
    use pharmakos_proto::gp::api::v1::verify_plan::Depth;
    use std::path::Path;

    fn parsed(args: &[&str]) -> Result<Command, Exit> {
        parse(args.iter().map(|arg| (*arg).to_owned()), Path::new("."))
            .map(|invocation| invocation.command)
            .map_err(|failure| failure.code)
    }

    #[test]
    fn every_command_in_the_table_parses() {
        // The table is what the help prints. A row nothing parses is a line of
        // documentation for a command that does not exist.
        for row in COMMANDS {
            let mut args: Vec<&str> = vec![row.name];
            match row.name {
                "verify" => args.push("a.jsonc"),
                "scenario" => args.extend(["run", "a.scenario.jsonc"]),
                "seat" => args.push("doctor"),
                _ => {}
            }
            assert!(
                parsed(&args).is_ok(),
                "`{}` is in the command table and does not parse",
                row.usage
            );
        }
    }

    #[test]
    fn help_wins_wherever_it_appears() {
        // `cargo xtask ci`'s scenario step probes with exactly this, and takes
        // a zero status as "the runner exists".
        for args in [
            &["--help"][..],
            &["-h"],
            &["scenario", "--help"],
            &["--help", "scenario"],
            &["seat", "--help"],
        ] {
            assert_eq!(parsed(args), Ok(Command::Help), "{args:?}");
        }
    }

    #[test]
    fn connect_is_refused_by_name() {
        let error = parse(["connect"], Path::new(".")).expect_err("there is no connect");
        assert_eq!(error.code, Exit::Usage);
        assert!(
            error.message.contains("v1.1"),
            "somebody typing `connect` has read a roadmap: {}",
            error.message
        );
    }

    #[test]
    fn a_missing_operand_is_a_usage_error_and_not_a_default() {
        for args in [
            &["verify"][..],
            &["scenario"],
            &["scenario", "run"],
            &["seat"],
        ] {
            assert_eq!(parsed(args), Err(Exit::Usage), "{args:?}");
        }
    }

    #[test]
    fn depth_and_part_reach_their_commands() {
        assert_eq!(
            parsed(&["verify", "--depth", "quick", "a.jsonc"]),
            Ok(Command::Verify {
                path: "a.jsonc".into(),
                depth: Depth::Quick
            })
        );
        assert_eq!(
            parsed(&["schema", "--part", "step"]),
            Ok(Command::Schema {
                part: String::from("step")
            })
        );
        assert_eq!(
            parsed(&["verify", "--depth", "deep", "a.jsonc"]),
            Err(Exit::Usage)
        );
    }
}
