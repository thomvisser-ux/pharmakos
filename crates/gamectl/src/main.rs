// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `gamectl` binary: the process's edges, and nothing else.
//!
//! Everything this binary does lives in the library beside it
//! (`pharmakos_gamectl`), because `scenario run`'s acceptance is a hash chain
//! compared with another hash chain and an integration test cannot call into a
//! `[[bin]]`. What is left here is what a library must not do: read the real
//! argument list, read the real working directory, write to the real handles,
//! and exit with a number.

use std::io::Write as _;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    // A working directory that cannot be read is said, not guessed at: every
    // relative path is read against it, and `.` in its place was a silent
    // fallback S2's sweep (E7) removed.
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(error) => {
            let _ = writeln!(
                std::io::stderr(),
                "{}",
                pharmakos_gamectl::strings::no_working_directory(&error.to_string())
            );
            return ExitCode::from(pharmakos_gamectl::exit::Exit::Input.code());
        }
    };
    let outcome = pharmakos_gamectl::run(args, &cwd);

    // A closed pipe is not a failure of the command: `gamectl docs | head` is
    // an ordinary thing to type, and the exit code belongs to what was asked
    // rather than to whether the reader stayed to hear the answer.
    //
    // **Everything else is.** A review found the same `let _` swallowing a
    // full disc and a read-only target, so `gamectl docs > file` reported
    // success over a truncated file. Standard error is not checked the same
    // way: there would be nowhere to report the failure to.
    let written = std::io::stdout()
        .write_all(outcome.out.as_bytes())
        .and_then(|()| std::io::stdout().flush());
    let _ = std::io::stderr().write_all(outcome.err.as_bytes());
    if let Err(error) = written
        && error.kind() != std::io::ErrorKind::BrokenPipe
    {
        let _ = writeln!(
            std::io::stderr(),
            "{}",
            pharmakos_gamectl::strings::unwritable_output(&error.to_string())
        );
        return ExitCode::from(pharmakos_gamectl::exit::Exit::Internal.code());
    }
    ExitCode::from(outcome.code.code())
}
