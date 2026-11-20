// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! xtask's own test runner: what cargo starts every test executable through,
//! doctests included, in a checked run.
//!
//! A checked run ([`crate::resolved::execute`]) hands cargo
//! `target.<host>.runner='<this xtask> __run-test'` on its command line
//! ([`crate::resolved::Host::runner_config`]), which a config file's
//! `target.<host>.runner` and `target.'cfg(…)'.runner` cannot override
//! (measured with cargo 1.98.1: the command line's string replaces a file's
//! string, for the triple and for a `cfg(…)`, and beside a file's array cargo
//! refuses to merge and fails, so the run goes red and nothing runs). Cargo
//! then starts each test executable, and rustdoc each doctest, as
//! `<xtask> __run-test <executable> <args…>`.
//!
//! The runner records the start, then runs the executable with standard input,
//! output and error inherited and its exit status passed through exactly. The
//! record is one file per start, in a folder fresh for each run, under the
//! target directory, named to the runner by [`RECORD_VAR`]: each runner process
//! creates its own file with `create_new`, named by its process id, so
//! rustdoc's concurrent doctests never write the same file. A runner started
//! without the variable, or unable to write its record, runs nothing and fails.
//!
//! The check then compares the executables it reported with the starts the
//! records name ([`crate::resolved::execute`]); nothing reads what cargo or
//! libtest prints.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

/// The hidden subcommand cargo starts the runner with.
pub(crate) const SUBCOMMAND: &str = "__run-test";

/// The variable naming the folder a run's records go to.
pub(crate) const RECORD_VAR: &str = "PHARMAKOS_XTASK_TEST_RECORD";

/// The variable xtask's checked doctest call sets to `1`, telling the runner
/// that every start of this run is a doctest executable.
pub(crate) const DOCTEST_RUN_VAR: &str = "PHARMAKOS_XTASK_DOCTEST_RUN";

/// Rustdoc's own variable: set, the merged doctest bundle starts each doctest
/// as a process of its own, by this path.
pub(crate) const DOCTEST_BIN_VAR: &str = "RUSTDOC_DOCTEST_BIN_PATH";

/// The folder, under the target directory, that holds each run's record
/// folder.
pub(crate) const RECORDS_FOLDER: &str = "xtask-test-runs";

/// Every record file's name ends so.
const RECORD_SUFFIX: &str = ".start";

/// Why the runner ran nothing.
#[derive(Debug)]
pub(crate) enum RunnerError {
    /// Started without [`RECORD_VAR`], or with it empty: not by a checked run.
    NoRecordFolder,
    /// Started with no executable to run.
    NoExecutable,
    /// [`DOCTEST_RUN_VAR`] set to anything but `1`.
    BadDoctestRun { value: OsString },
    /// The executable's path, made absolute, is not Unicode, so it cannot be
    /// recorded as the check's report names it.
    NotUnicode { path: OsString },
    /// The executable's path could not be made absolute.
    Absolute { error: std::io::Error },
    /// The record could not be written.
    Record {
        folder: PathBuf,
        error: std::io::Error,
    },
    /// The executable could not be started.
    Launch {
        executable: String,
        error: std::io::Error,
    },
    /// The executable ended with no exit code. Only the Windows launch can
    /// meet it, where `ExitStatus::code` is always `Some`; it is typed rather
    /// than unwrapped.
    #[cfg_attr(
        unix,
        allow(dead_code, reason = "constructed only by the Windows launch")
    )]
    NoExitCode { executable: String },
}

impl fmt::Display for RunnerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRecordFolder => write!(
                f,
                "`{RECORD_VAR}` is not set, so this was not started by a checked run, and \
                 a start it made could not be recorded"
            ),
            Self::NoExecutable => write!(f, "no executable was named"),
            Self::BadDoctestRun { value } => write!(
                f,
                "`{DOCTEST_RUN_VAR}` is `{}`; xtask sets it to `1` for a doctest run, and to \
                 nothing else",
                value.display()
            ),
            Self::NotUnicode { path } => {
                write!(f, "the executable's path {} is not Unicode", path.display())
            }
            Self::Absolute { error } => {
                write!(
                    f,
                    "the executable's path could not be made absolute: {error}"
                )
            }
            Self::Record { folder, error } => write!(
                f,
                "the start could not be recorded in {}: {error}",
                folder.display()
            ),
            Self::Launch { executable, error } => {
                write!(f, "{executable} could not be started: {error}")
            }
            Self::NoExitCode { executable } => {
                write!(f, "{executable} ended with no exit code")
            }
        }
    }
}

/// The runner's entry point: `args` are what follows [`SUBCOMMAND`], the
/// executable and its arguments. Never returns on success: on Unix the
/// process becomes the test (`exec`), so its signals and status are the
/// test's own; on Windows it exits with the test's exit code, an abort's
/// included.
pub(crate) fn main(args: &[OsString]) -> std::process::ExitCode {
    let error = match prepare(
        args,
        std::env::var_os(RECORD_VAR),
        std::env::var_os(DOCTEST_RUN_VAR),
    ) {
        Ok((command, executable)) => launch(command, executable),
        Err(error) => error,
    };
    eprintln!("xtask test runner: {error}; nothing was run");
    std::process::ExitCode::FAILURE
}

/// Records the start and returns the command that runs it, or why nothing
/// may run. `folder` is [`RECORD_VAR`]'s value, `doctest_run`
/// [`DOCTEST_RUN_VAR`]'s.
///
/// In a doctest run (xtask's checked `--doc` call sets [`DOCTEST_RUN_VAR`];
/// the runner does not guess from what it is handed, which for rustdoc's
/// merged doctests is the bundle's path and nothing else) every start gets
/// [`DOCTEST_BIN_VAR`] naming its own executable. Rustdoc sets that variable
/// itself only when no runner is configured; without it the merged bundle
/// runs every doctest in one process, so a doctest that calls
/// `std::process::exit(0)` ends the bundle green over a failing one. With it,
/// the bundle starts each doctest as a process of its own again, as plain
/// cargo does. A standalone doctest (one rustdoc does not merge) is the
/// doctest's own `main`, which never reads the variable. Outside a doctest
/// run the variable is removed, so a value from the caller's environment
/// cannot reach a bundle. `xtask/tests/doctest_runner.rs` pins rustdoc's half
/// with a real failing doctest beside one that exits 0.
fn prepare(
    args: &[OsString],
    folder: Option<OsString>,
    doctest_run: Option<OsString>,
) -> Result<(Command, String), RunnerError> {
    let folder = folder
        .filter(|folder| !folder.is_empty())
        .ok_or(RunnerError::NoRecordFolder)?;
    let doctest_run = match doctest_run {
        None => false,
        Some(value) if value == "1" => true,
        Some(value) => return Err(RunnerError::BadDoctestRun { value }),
    };
    let (executable, rest) = args.split_first().ok_or(RunnerError::NoExecutable)?;
    let absolute =
        std::path::absolute(executable).map_err(|error| RunnerError::Absolute { error })?;
    let text = absolute
        .to_str()
        .ok_or_else(|| RunnerError::NotUnicode {
            path: absolute.clone().into_os_string(),
        })?
        .to_owned();
    record_start(Path::new(&folder), &text)?;
    let mut command = Command::new(&absolute);
    command
        .args(rest)
        .env_remove(RECORD_VAR)
        .env_remove(DOCTEST_RUN_VAR);
    if doctest_run && absolute.as_os_str().is_empty() {
        command.env(DOCTEST_BIN_VAR, &absolute);
    } else {
        command.env_remove(DOCTEST_BIN_VAR);
    }
    Ok((command, text))
}

/// Becomes the test: the error is all `exec` can return.
#[cfg(unix)]
fn launch(mut command: Command, executable: String) -> RunnerError {
    use std::os::unix::process::CommandExt;
    let error = command.exec();
    RunnerError::Launch { executable, error }
}

/// Runs the test with every standard stream inherited and exits with its
/// code.
#[cfg(not(unix))]
fn launch(mut command: Command, executable: String) -> RunnerError {
    match command.status() {
        Ok(status) => match exit_code(status) {
            Some(code) => std::process::exit(code),
            None => RunnerError::NoExitCode { executable },
        },
        Err(error) => RunnerError::Launch { executable, error },
    }
}

/// The code to exit with to pass `status` through: on Windows a process's
/// whole 32-bit exit code, an abort's (`0xC0000409`) included, which
/// `std::process::exit` hands back to `ExitProcess` unchanged.
#[cfg_attr(unix, allow(dead_code, reason = "Unix execs; the tests use it there"))]
fn exit_code(status: ExitStatus) -> Option<i32> {
    status.code()
}

/// Writes one record of a start of `executable` in `folder`: a new file of
/// its own, named by this process's id and the first free attempt number.
pub(crate) fn record_start(folder: &Path, executable: &str) -> Result<(), RunnerError> {
    let id = std::process::id();
    for attempt in 0..u32::MAX {
        let path = folder.join(format!("{id}-{attempt}{RECORD_SUFFIX}"));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                return file.write_all(executable.as_bytes()).map_err(|error| {
                    RunnerError::Record {
                        folder: folder.to_path_buf(),
                        error,
                    }
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(RunnerError::Record {
                    folder: folder.to_path_buf(),
                    error,
                });
            }
        }
    }
    Err(RunnerError::Record {
        folder: folder.to_path_buf(),
        error: std::io::Error::other("every record name for this process id is taken"),
    })
}

/// A record folder of its own for one run: `<target>/xtask-test-runs/<pid>-<n>`,
/// created here, so it holds nothing an earlier run left.
pub(crate) fn fresh_folder(target_dir: &Path) -> Result<PathBuf, String> {
    let parent = target_dir.join(RECORDS_FOLDER);
    fs::create_dir_all(&parent)
        .map_err(|error| format!("creating {}: {error}", parent.display()))?;
    let id = std::process::id();
    for attempt in 0..u32::MAX {
        let folder = parent.join(format!("{id}-{attempt}"));
        match fs::create_dir(&folder) {
            Ok(()) => return Ok(folder),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("creating {}: {error}", folder.display())),
        }
    }
    Err(format!(
        "every record folder name for process {id} under {} is taken",
        parent.display()
    ))
}

/// Runs `run` in a record folder of its own ([`fresh_folder`]), reads the
/// starts it recorded ([`read_records`]) and removes the folder, on every
/// path, a failed launch included: the one owner of the folder's life. An
/// error of the run comes first; a failure to remove the folder is an error
/// too, never dropped.
pub(crate) fn with_record_folder<T>(
    target_dir: &Path,
    run: impl FnOnce(&Path) -> Result<T, String>,
) -> Result<(T, Vec<String>), String> {
    let folder = fresh_folder(target_dir)?;
    let outcome =
        run(&folder).and_then(|value| read_records(&folder).map(|records| (value, records)))?;
    let outcome: Result<(T, Vec<String>), String> = Ok(outcome);
    let removed = fs::remove_dir_all(&folder)
        .map_err(|error| format!("removing the record folder {}: {error}", folder.display()));
    match (outcome, removed) {
        (Ok(done), Ok(())) => Ok(done),
        (Ok(_), Err(removal)) => Err(removal),
        (Err(error), Ok(())) => Err(error),
        (Err(error), Err(removal)) => Err(format!("{error}; and {removal}")),
    }
}

/// The starts `folder` records, sorted: every file in it must be a record,
/// holding a path.
// `fs::read_dir` is banned because its order differs between file systems;
// allowed here because the records are sorted before anything reads them,
// which is the remedy the ban asks for.
#[allow(clippy::disallowed_methods)]
pub(crate) fn read_records(folder: &Path) -> Result<Vec<String>, String> {
    let entries =
        fs::read_dir(folder).map_err(|error| format!("reading {}: {error}", folder.display()))?;
    let mut starts: Vec<String> = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|error| format!("reading {}: {error}", folder.display()))?
            .path();
        let is_record = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(RECORD_SUFFIX));
        if !is_record {
            return Err(format!(
                "{} is in a run's record folder and is not a record",
                path.display()
            ));
        }
        let text = fs::read_to_string(&path)
            .map_err(|error| format!("reading {}: {error}", path.display()))?;
        if text.is_empty() {
            return Err(format!(
                "{} records a start with no path: its runner stopped while writing it",
                path.display()
            ));
        }
        starts.push(text);
    }
    starts.sort();
    Ok(starts)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of its own under the test's temporary directory.
    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir()
            .join("xtask-runner-tests")
            .join(format!("{}-{name}", std::process::id()));
        match fs::remove_dir_all(&folder) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("clearing {}: {error}", folder.display()),
        }
        fs::create_dir_all(&folder).expect("the scratch folder is created");
        folder
    }

    /// Without its folder the runner records nothing and runs nothing.
    #[test]
    fn without_its_folder_the_runner_runs_nothing() {
        let args = [OsString::from("/bin/true")];
        assert!(matches!(
            prepare(&args, None, None),
            Err(RunnerError::NoRecordFolder)
        ));
        assert!(matches!(
            prepare(&args, Some(OsString::new()), None),
            Err(RunnerError::NoRecordFolder)
        ));
        let folder = scratch("empty");
        assert!(matches!(
            prepare(&[], Some(folder.clone().into_os_string()), None),
            Err(RunnerError::NoExecutable)
        ));
        assert_eq!(read_records(&folder), Ok(Vec::new()));
        // A folder it cannot write to: nothing runs.
        let missing = folder.join("missing");
        assert!(matches!(
            prepare(&args, Some(missing.into_os_string()), None),
            Err(RunnerError::Record { .. })
        ));
    }

    /// A start is recorded once, absolute, and the command runs exactly that
    /// file with the arguments given, without the record variable.
    #[test]
    fn a_start_is_recorded_and_the_command_is_the_test() {
        let folder = scratch("one");
        let args = [
            OsString::from("target/debug/deps/x-1"),
            OsString::from("--quiet"),
            OsString::from("a b"),
        ];
        let (command, executable) =
            prepare(&args, Some(folder.clone().into_os_string()), None).expect("it prepares");
        let absolute = std::path::absolute("target/debug/deps/x-1").expect("absolute");
        assert_eq!(Path::new(&executable), absolute);
        assert_eq!(command.get_program(), absolute.as_os_str());
        assert_eq!(command.get_args().collect::<Vec<_>>(), ["--quiet", "a b"]);
        assert!(
            command
                .get_envs()
                .any(|(key, value)| key == RECORD_VAR && value.is_none())
        );
        assert_eq!(read_records(&folder), Ok(vec![executable]));
        // Outside a doctest run, rustdoc's variable is removed, so a value
        // from the caller's environment never reaches a bundle.
        assert!(
            command
                .get_envs()
                .any(|(key, value)| key == DOCTEST_BIN_VAR && value.is_none())
        );
    }

    /// In a doctest run every start gets rustdoc's variable naming its own
    /// executable, so the merged bundle runs each doctest as a process of its
    /// own; the run's own variable never reaches the test; a value other
    /// than `1` is refused and runs nothing.
    #[test]
    fn a_doctest_run_names_each_start_to_rustdoc() {
        let folder = scratch("doctest");
        let args = [OsString::from("tmp/rustdoctest/rust_out")];
        let (command, executable) = prepare(
            &args,
            Some(folder.clone().into_os_string()),
            Some(OsString::from("1")),
        )
        .expect("it prepares");
        let envs: Vec<(&std::ffi::OsStr, Option<&std::ffi::OsStr>)> = command.get_envs().collect();
        assert!(envs.contains(&(
            std::ffi::OsStr::new(DOCTEST_BIN_VAR),
            Some(std::ffi::OsStr::new(&executable))
        )));
        assert!(envs.contains(&(std::ffi::OsStr::new(DOCTEST_RUN_VAR), None)));
        assert!(matches!(
            prepare(
                &args,
                Some(folder.clone().into_os_string()),
                Some(OsString::from("yes"))
            ),
            Err(RunnerError::BadDoctestRun { .. })
        ));
        assert_eq!(read_records(&folder), Ok(vec![executable]));
    }

    /// The record folder is removed on every path: after a run that fails
    /// (a launch that never happened) as after one that succeeds, and its
    /// records are read before it goes.
    #[test]
    fn the_record_folder_goes_on_every_path() {
        let target = scratch("lifecycle");
        let mut seen: Vec<PathBuf> = Vec::new();
        let failed: Result<((), Vec<String>), String> = with_record_folder(&target, |folder| {
            seen.push(folder.to_path_buf());
            Err("failed to launch `cargo`".to_owned())
        });
        assert_eq!(failed, Err("failed to launch `cargo`".to_owned()));
        let done = with_record_folder(&target, |folder| {
            seen.push(folder.to_path_buf());
            record_start(folder, "/t/x-1").map_err(|error| error.to_string())
        });
        assert_eq!(done, Ok(((), vec!["/t/x-1".to_owned()])));
        assert_eq!(seen.len(), 2);
        for folder in seen {
            assert!(!folder.exists(), "{} was left behind", folder.display());
        }
    }

    /// Records from many starts, as rustdoc's concurrent doctests make them,
    /// never share a file.
    #[test]
    fn concurrent_starts_each_get_a_record() {
        let folder = scratch("many");
        std::thread::scope(|scope| {
            for index in 0..16 {
                let folder = &folder;
                scope.spawn(move || {
                    record_start(folder, &format!("/t/doctest-{index:02}")).expect("it records");
                });
            }
        });
        let records = read_records(&folder).expect("it reads");
        let expected: Vec<String> = (0..16)
            .map(|index| format!("/t/doctest-{index:02}"))
            .collect();
        assert_eq!(records, expected);
    }

    /// A folder holding anything but records, or a record with no path, is
    /// refused.
    #[test]
    fn a_record_folder_holds_records_only() {
        let folder = scratch("stray");
        fs::write(folder.join("stray.txt"), "x").expect("it writes");
        assert!(read_records(&folder).is_err());
        let folder = scratch("blank");
        fs::write(folder.join("1-0.start"), "").expect("it writes");
        assert!(read_records(&folder).is_err());
    }

    /// Each run's folder is new.
    #[test]
    fn each_run_gets_a_fresh_folder() {
        let target = scratch("target");
        let first = fresh_folder(&target).expect("one");
        let second = fresh_folder(&target).expect("two");
        assert_ne!(first, second);
        assert!(first.starts_with(target.join(RECORDS_FOLDER)));
        assert_eq!(read_records(&second), Ok(Vec::new()));
    }

    /// The exit code passed on is the child's own, an abort's included.
    #[cfg(windows)]
    #[test]
    fn the_exit_code_is_passed_through() {
        let shell = std::env::var_os("ComSpec").expect("Windows names its shell");
        for (asked, code) in [("exit 7", 7), ("exit -1073740791", -1_073_740_791)] {
            let status = Command::new(&shell)
                .args(["/c", asked])
                .status()
                .expect("the shell runs");
            assert_eq!(exit_code(status), Some(code), "{asked}");
        }
    }
}
