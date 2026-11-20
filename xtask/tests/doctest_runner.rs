// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Rustdoc's half of xtask's doctest runner, pinned with a real doctest run.
//!
//! Under any configured runner, rustdoc does not set
//! `RUSTDOC_DOCTEST_BIN_PATH`, and its merged doctest bundle then runs every
//! doctest in one process: a doctest that calls `std::process::exit(0)` ends
//! the bundle green over a failing one. xtask's runner sets the variable for
//! every start of a doctest run (`xtask/src/runner.rs`), which makes the
//! bundle start each doctest as a process of its own again. This test runs a
//! crate with exactly that pair through the runner, spelled and given its
//! environment as xtask's checked doctest call gives them, and holds the run
//! red; with only the exiting doctest and a passing one, green. A rustdoc that
//! changes the mechanism fails it, on every operating system CI runs.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The host triple, from `rustc -vV`.
fn host() -> Result<String, String> {
    let output = Command::new("rustc")
        .arg("-vV")
        .output()
        .map_err(|error| format!("running `rustc -vV`: {error}"))?;
    let text = String::from_utf8(output.stdout)
        .map_err(|error| format!("`rustc -vV` printed something that is not UTF-8: {error}"))?;
    text.lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(|host| host.trim().to_owned())
        .ok_or_else(|| "`rustc -vV` names no host".to_owned())
}

/// A crate of its own, `doctests` as its library's doc comments, written
/// fresh under this test's temporary folder.
fn fixture(name: &str, doctests: &[&str]) -> Result<PathBuf, String> {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    match fs::remove_dir_all(&root) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("clearing {}: {error}", root.display())),
    }
    let write = |path: PathBuf, text: String| {
        fs::write(&path, text).map_err(|error| format!("writing {}: {error}", path.display()))
    };
    fs::create_dir_all(root.join("src"))
        .map_err(|error| format!("creating {}: {error}", root.display()))?;
    write(
        root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[workspace]\n"
        ),
    )?;
    let mut lib = String::new();
    for (index, body) in doctests.iter().enumerate() {
        writeln!(
            lib,
            "/// ```\n/// {body}\n/// ```\npub fn item_{index}() {{}}\n"
        )
        .map_err(|error| format!("composing the library: {error}"))?;
    }
    write(root.join("src").join("lib.rs"), lib)?;
    Ok(root)
}

/// Runs the fixture's doctests through xtask's runner as a checked doctest
/// call does: the runner pinned as one string, the run marked a doctest run,
/// a record folder of its own. Answers whether cargo succeeded, and how many
/// starts the runner recorded.
// `fs::read_dir` is banned for its order, which differs between file
// systems; here only the entries' count is read.
#[allow(clippy::disallowed_methods)]
fn run_doctests(root: &Path) -> Result<(bool, usize), String> {
    let xtask = env!("CARGO_BIN_EXE_xtask");
    if xtask.contains(char::is_whitespace) {
        return Err(format!(
            "this test names xtask by its absolute path, {xtask}, which holds whitespace"
        ));
    }
    let records = root.join("records");
    fs::create_dir_all(&records)
        .map_err(|error| format!("creating {}: {error}", records.display()))?;
    let cargo = std::env::var("CARGO")
        .map_err(|error| format!("cargo sets `CARGO` for its tests, and here: {error}"))?;
    let status = Command::new(cargo)
        .arg("--config")
        .arg(format!("target.{}.runner='{xtask} __run-test'", host()?))
        .args(["test", "--doc", "--quiet"])
        .current_dir(root)
        .env("CARGO_TARGET_DIR", root.join("target"))
        .env("PHARMAKOS_XTASK_TEST_RECORD", &records)
        .env("PHARMAKOS_XTASK_DOCTEST_RUN", "1")
        .env_remove("RUSTDOC_DOCTEST_BIN_PATH")
        .status()
        .map_err(|error| format!("running cargo: {error}"))?;
    let started = fs::read_dir(&records)
        .map_err(|error| format!("reading {}: {error}", records.display()))?
        .count();
    Ok((status.success(), started))
}

#[test]
fn a_doctest_that_exits_cannot_hide_a_failing_one() {
    let red = fixture(
        "doctest-exit-hides-failure",
        &["assert_eq!(1, 2);", "std::process::exit(0);"],
    )
    .expect("the fixture");
    let (success, started) = run_doctests(&red).expect("the run");
    assert!(
        !success,
        "the failing doctest was hidden by the one that exits"
    );
    assert!(
        started > 0,
        "the doctests did not go through xtask's runner"
    );

    let green = fixture(
        "doctest-exit-beside-a-pass",
        &["assert_eq!(1, 1);", "std::process::exit(0);"],
    )
    .expect("the fixture");
    let (success, started) = run_doctests(&green).expect("the run");
    assert!(success, "a passing doctest beside one that exits 0 failed");
    assert!(
        started > 0,
        "the doctests did not go through xtask's runner"
    );
}
