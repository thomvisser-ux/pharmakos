// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The exit codes, in one place, and the failure type every command returns.
//!
//! The skeleton plan's T15 acceptance line ends "exit codes documented", and
//! this module is where they are documented *once*: the table below, the help
//! text in [`crate::strings`] and `docs/` all read from [`Exit::ALL`], so a
//! code cannot be added in one of the three and missed in the other two.
//!
//! # Why five, and why this split
//!
//! The split is the one a script needs to branch on. A caller wants to know
//! three things and they are three different answers: *did I ask wrongly*
//! (fix the command line), *could the tool read what I named* (fix the file),
//! and *did the thing I asked about hold* (fix the playbook, the scenario or
//! the installation). The last of those is the only one that is a real answer
//! rather than an error, and it is the one a pull request's CI branches on.
//!
//! `1` is the usage code because that is what a shell user expects from a
//! misspelled flag, and because the pre-T15 stub already exited 1 for
//! everything — a script written against the stub now gets 1 only when it was
//! actually wrong.
//!
//! Exit codes are a published surface the moment anything shells out to this
//! binary, and `cargo xtask ci`'s `scenario` step does. **Codes are
//! append-only**: a number never changes meaning, and a new outcome takes a
//! new number.

/// What the process exits with.
///
/// `u8`, because that is what every platform can actually carry: Windows takes
/// a full `i32` and Unix keeps the low eight bits of one, so a code above 255
/// would mean two different things on two operating systems.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(u8)]
pub enum Exit {
    /// Everything asked for held.
    Ok = 0,
    /// The command line was wrong: an unknown subcommand, a missing operand,
    /// a flag this build does not have. Nothing was read and nothing was run.
    Usage = 1,
    /// A file named on the command line could not be read, or is not the kind
    /// of file it was named as. The command was well formed; its input was
    /// not there.
    Input = 2,
    /// **The answer, and not an error**: the playbook does not qualify, a
    /// scenario assertion did not hold, a `seat doctor` check failed. The tool
    /// did exactly what was asked and the news is bad.
    Failed = 3,
    /// This build is inconsistent with itself — a generated artefact that does
    /// not match its schema, a rules table missing a block every check reads.
    /// Nothing the caller did could have caused or avoided it.
    Internal = 4,
}

impl Exit {
    /// Every code, in numeric order, with the one-line meaning the help text
    /// and the generated documentation both print.
    pub const ALL: &'static [(Exit, &'static str)] = &[
        (Exit::Ok, "everything asked for held"),
        (Exit::Usage, "the command line was wrong; nothing ran"),
        (
            Exit::Input,
            "a file named on the command line could not be read",
        ),
        (
            Exit::Failed,
            "the answer is no: a playbook did not qualify, an assertion did not hold, a check \
             failed",
        ),
        (
            Exit::Internal,
            "this build is inconsistent with itself; nothing the caller did caused it",
        ),
    ];

    /// The number the process exits with.
    ///
    /// Written out rather than `self as u8`: `as_conversions` is denied
    /// workspace-wide (AGENTS.md §4.3) and a discriminant cast is exactly the
    /// kind of silent conversion the ban is about. A match also means a variant
    /// added without a number fails to compile here.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Exit::Ok => 0,
            Exit::Usage => 1,
            Exit::Input => 2,
            Exit::Failed => 3,
            Exit::Internal => 4,
        }
    }
}

/// A command that did not succeed: the code to exit with and what to say on
/// standard error.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Failure {
    /// Which of [`Exit`]'s outcomes this is.
    pub code: Exit,
    /// The sentence printed on standard error, from [`crate::strings`].
    pub message: String,
}

impl Failure {
    /// A failure with a code and a message.
    #[must_use]
    pub fn new(code: Exit, message: impl Into<String>) -> Failure {
        Failure {
            code,
            message: message.into(),
        }
    }

    /// [`Exit::Usage`].
    #[must_use]
    pub fn usage(message: impl Into<String>) -> Failure {
        Failure::new(Exit::Usage, message)
    }

    /// [`Exit::Input`].
    #[must_use]
    pub fn input(message: impl Into<String>) -> Failure {
        Failure::new(Exit::Input, message)
    }

    /// [`Exit::Failed`].
    #[must_use]
    pub fn failed(message: impl Into<String>) -> Failure {
        Failure::new(Exit::Failed, message)
    }

    /// [`Exit::Internal`].
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Failure {
        Failure::new(Exit::Internal, message)
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::Exit;

    #[test]
    fn every_code_is_listed_once_and_in_order() {
        let codes: Vec<u8> = Exit::ALL.iter().map(|(code, _)| code.code()).collect();
        assert_eq!(
            codes,
            vec![0, 1, 2, 3, 4],
            "Exit::ALL is what the help text and the generated documentation print; a code \
             missing from it is a code nobody is told about"
        );
    }
}
