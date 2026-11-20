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
//! crate with exactly that pair through the runner, spelled by xtask itself
//! and given its environment as xtask's checked doctest call gives them, and
//! holds the run
//! red; with only the exiting doctest and a passing one, green. A rustdoc that
//! changes the mechanism fails it, on every operating system CI runs.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
/// call does: the runner's `--config` value as xtask itself spells it for the
/// folder cargo works in (`xtask __runner-config <folder>`, the one rule),
/// the run marked a doctest run, a record folder of its own. Cargo works in
/// the target directory, where xtask's binary lies, so the runner is named
/// relative to it, as an in-tree `target` is in CI, whatever the checkout's
/// path holds; the fixture is reached by `--manifest-path`. Answers whether
/// cargo succeeded, and how many starts the runner recorded.
// `fs::read_dir` is banned for its order, which differs between file
// systems; here only the entries' count is read.
#[allow(clippy::disallowed_methods)]
fn run_doctests(root: &Path) -> Result<(bool, usize), String> {
    let xtask = env!("CARGO_BIN_EXE_xtask");
    let target = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .parent()
        .ok_or_else(|| "the test's temporary folder has no parent".to_owned())?;
    let asked = Command::new(xtask)
        .arg("__runner-config")
        .arg(target)
        .output()
        .map_err(|error| format!("running `xtask __runner-config`: {error}"))?;
    if !asked.status.success() {
        return Err(format!(
            "`xtask __runner-config` refused: {}",
            String::from_utf8_lossy(&asked.stderr)
        ));
    }
    let runner = String::from_utf8(asked.stdout).map_err(|error| {
        format!("`xtask __runner-config` printed something that is not UTF-8: {error}")
    })?;
    let records = root.join("records");
    fs::create_dir_all(&records)
        .map_err(|error| format!("creating {}: {error}", records.display()))?;
    let cargo = std::env::var("CARGO")
        .map_err(|error| format!("cargo sets `CARGO` for its tests, and here: {error}"))?;
    let status = Command::new(cargo)
        .arg("--config")
        .arg(runner.trim_end())
        .args(["test", "--doc", "--quiet", "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .current_dir(target)
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
