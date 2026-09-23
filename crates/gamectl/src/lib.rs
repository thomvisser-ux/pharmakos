// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `gamectl` — the command-line client. Role from spec §15 (Architecture),
//! clients layer, detailed in spec §12.
//!
//! Spec §12's five commands — `verify`, `schema`, `docs`, `scenario run`,
//! `seat doctor` — and `host`, which serves one match to the Godot client over
//! this process's own stdio pipe (decisions-log item 107 (2); [`host`]).
//! **There is no `connect`** — nothing outside the game attaches
//! to a seat in v1 (AGENTS.md §11), the Seat Gateway is internal, and the
//! published API is v1.1.
//!
//! # It is an ordinary client
//!
//! Same snapshot, same verifier, same submit path, no privileged reads. Every
//! command here is a thin rendering of somebody else's answer:
//!
//! | Command | Whose answer it prints |
//! |---|---|
//! | [`verify`] | `plan-core`'s canonical form through `pharmakos_verifier::verify` |
//! | [`schema`] | `pharmakos_gateway::schema`, the generator `get_schema` is served from |
//! | [`docs`] | the checked-in `gp.v1` descriptor set and the verifier's catalogue |
//! | [`scenario`] | a match hosted by the gateway's `Host` behind a `Surface` |
//! | [`doctor`] | all four of the above, asked whether they work here |
//!
//! Nothing in this crate re-implements a rule, re-words a diagnostic or does
//! arithmetic on `$`, `kW` or a duration. It is not a walled crate either
//! (AGENTS.md §4.9), so it reads no clock and contains no float: a CLI that
//! timed its own run would be the first wall-clock read in a deterministic
//! crate, and the lint set denies it.
//!
//! # A library with a four-line binary in front of it
//!
//! `main.rs` turns an [`exit::Exit`] into a process status and nothing else.
//! Everything testable is here, because `scenario run`'s acceptance is a hash
//! chain compared with another hash chain rather than a string compared with a
//! golden, and an integration test cannot call into a `[[bin]]`.

pub mod cli;
pub mod docs;
pub mod doctor;
pub mod exit;
pub mod host;
pub mod rules;
pub mod scenario;
pub mod schema;
pub mod seat;
pub mod strings;
pub mod verify;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::cli::Command;
use crate::exit::{Exit, Failure};

/// What one invocation produced: what to print where, and what to exit with.
///
/// Rendering into strings rather than writing to the process's own handles
/// keeps every command a pure function of its inputs, which is what lets
/// `tests/cli.rs` run the whole binary's behaviour in process with no
/// subprocess and no captured pipe. `host` is the one exception: it owns the
/// process's real standard input and output for the life of a match, writes
/// its announce line itself, and leaves `out` empty.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// The status to exit with.
    pub code: Exit,
    /// What goes to standard output.
    pub out: String,
    /// What goes to standard error.
    pub err: String,
}

/// Run one command line.
///
/// `args` is the argument list without the program name; `cwd` is the
/// directory relative paths are read against unless `--root` overrides it.
#[must_use]
pub fn run<I>(args: I, cwd: &Path) -> Outcome
where
    I: IntoIterator,
    I::Item: Into<OsString>,
{
    match cli::parse(args, cwd).and_then(|invocation| dispatch(&invocation)) {
        Ok(out) => Outcome {
            code: Exit::Ok,
            out,
            err: String::new(),
        },
        Err(failure) => Outcome {
            code: failure.code,
            out: String::new(),
            err: with_newline(failure.message),
        },
    }
}

fn dispatch(invocation: &cli::Invocation) -> Result<String, Failure> {
    let root = invocation.root.as_path();
    let rendered = match &invocation.command {
        // `host` writes its one announce line to standard output itself and
        // owns the process's standard input until it closes, so it is the one
        // command whose output is not a string returned here: it returns at
        // once, and nothing is appended to what it wrote, not even a newline.
        Command::Host => return host::run(root),
        Command::Help => Ok(strings::help()),
        Command::Version => Ok(strings::version()),
        Command::Verify { path, depth } => verify::run(root, path, *depth),
        Command::Schema { part } => schema::run(part),
        Command::Docs => docs::run(),
        Command::ScenarioRun { path } => scenario::run::run(root, path),
        Command::SeatDoctor => doctor::run(root),
    };
    rendered.map(with_newline)
}

/// Every message this binary prints ends in exactly one newline, wherever it
/// was built — a report that ends mid-line is one a shell prompt eats the end
/// of.
fn with_newline(mut text: String) -> String {
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// A path as this binary prints it: forward slashes, on every platform.
///
/// Messages and reports are read on Windows, Linux and macOS and are compared
/// across them; a backslash in one of them is a diff that means nothing.
#[must_use]
pub fn display(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Cargo's target directory, worked out from this executable's own location.
///
/// Two shapes, because this code runs in both: `<target>/<profile>/gamectl` when
/// `cargo run` built it, and `<target>/<profile>/deps/<test>` when it is an
/// integration test. The `deps` component is what tells them apart, which is
/// more honest than counting parents and hoping.
///
/// `None` when the executable is somewhere neither shape describes — a
/// packaged build, for instance. A caller treats that as "there is nowhere to
/// write a fresh output", never as a silent success.
#[must_use]
pub fn target_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let profile = exe.parent()?;
    let profile = if profile.file_name() == Some(std::ffi::OsStr::new("deps")) {
        profile.parent()?
    } else {
        profile
    };
    Some(profile.parent()?.to_path_buf())
}

/// Where the fresh output for a committed golden goes.
///
/// `tests/golden/README.md`'s convention, in code: a golden at
/// `tests/golden/<area>/<case>/expected.<ext>` is compared with
/// `<target>/golden/<area>/<case>/actual.<ext>`, and `<target>` honours
/// `CARGO_TARGET_DIR` because it is read from where the binary actually is.
///
/// `None` when [`target_dir`] is `None`, or when `golden` is not a path under
/// `tests/golden/` — which the scenario format refuses, so the second case is
/// a caller that did not read the file through [`scenario::load`].
#[must_use]
pub fn actual_for(golden: &str) -> Option<PathBuf> {
    let relative = golden.strip_prefix("tests/golden/")?;
    let (directory, file) = relative.rsplit_once('/')?;
    let fresh = file
        .strip_prefix("expected.")
        .map(|suffix| format!("actual.{suffix}"))?;
    let mut path = target_dir()?.join("golden");
    for part in directory.split('/') {
        path.push(part);
    }
    path.push(fresh);
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::{actual_for, display, run};
    use crate::exit::Exit;
    use std::path::Path;

    #[test]
    fn a_golden_maps_to_the_fresh_output_beside_it() {
        let mapped = actual_for("tests/golden/scenarios/expand-east-segment/expected.hashes.txt");
        let mapped = mapped.expect("this test runs from a cargo target directory");
        let printed = display(&mapped);
        assert!(
            printed.ends_with("golden/scenarios/expand-east-segment/actual.hashes.txt"),
            "{printed}"
        );
        assert_eq!(actual_for("somewhere/else/expected.hashes.txt"), None);
    }

    #[test]
    fn help_and_version_exit_zero_and_say_what_this_is() {
        for args in [&["--help"][..], &["--version"]] {
            let outcome = run(args.iter().map(|arg| (*arg).to_owned()), Path::new("."));
            assert_eq!(outcome.code, Exit::Ok, "{args:?}");
            assert!(outcome.out.ends_with('\n'));
            assert!(outcome.err.is_empty());
        }
    }

    #[test]
    fn an_unknown_command_is_a_usage_error_on_standard_error() {
        let outcome = run(["fly"], Path::new("."));
        assert_eq!(outcome.code, Exit::Usage);
        assert!(
            outcome.out.is_empty(),
            "nothing goes to stdout on a refusal"
        );
        assert!(outcome.err.contains("not a command"), "{}", outcome.err);
    }
}
