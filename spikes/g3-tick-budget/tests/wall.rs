// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The half of the determinism wall that clippy cannot express.
//!
//! `clippy.toml`'s `disallowed-types` is crate-wide and has no per-path
//! allow-list, while plan §5 sanctions the clock, floating point and statistics
//! in `src/bin/` — and only there. So the clock ban and the hash-container ban
//! are checked here instead, against the library's source text, read at test
//! time. Crude on purpose: it travels with the code rather than with the working
//! directory, it runs in CI on all three operating systems, and unlike a lint
//! configuration it cannot be silently widened by an `#[allow]`.
//!
//! The G2 spike does the same thing for the same reason, with one addition
//! here: this file also asserts the confinement **in the other direction** —
//! that the clock does appear in `src/bin/` — so the test cannot pass by the
//! harness quietly losing its ability to measure anything.
//!
//! This file lives in `tests/`, not `src/`, so it may spell the banned names
//! out. That is the whole reason it is a separate file: G2 had to assemble the
//! strings from fragments because its checker lived inside the code it checked.

use std::fs;
use std::path::{Path, PathBuf};

const BANNED: [&str; 8] = [
    "Instant",
    "SystemTime",
    "Duration",
    "HashMap",
    "HashSet",
    "RandomState",
    "f32",
    "f64",
];

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn library_modules() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in fs::read_dir(src_dir()).expect("src/ is readable") {
        let entry = entry.expect("a readable directory entry");
        if !entry.file_type().expect("a file type").is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".rs") {
            continue;
        }
        let text = fs::read_to_string(entry.path()).expect("module is readable");
        out.push((name, text));
    }
    out
}

fn binaries() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in fs::read_dir(src_dir().join("bin")).expect("src/bin is readable") {
        let entry = entry.expect("a readable directory entry");
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".rs") {
            continue;
        }
        let text = fs::read_to_string(entry.path()).expect("binary is readable");
        out.push((name, text));
    }
    out
}

/// The library reads no clock, names no floating-point type and uses no
/// hash-keyed container — including in prose, because a doc comment that spells
/// the name out would make the check unfalsifiable for the next person who runs
/// it.
#[test]
fn library_has_no_clock_no_hash_container_and_no_float_type() {
    let modules = library_modules();
    assert!(
        modules.len() >= 10,
        "expected the ten library modules, found {}",
        modules.len()
    );
    let mut bad: Vec<String> = Vec::new();
    for (name, text) in &modules {
        for b in BANNED {
            if text.contains(b) {
                bad.push(format!("{name} mentions {b}"));
            }
        }
    }
    assert!(bad.is_empty(), "determinism wall breached: {bad:?}");
}

/// Every phase of the tick lives in one of the files just checked. A module
/// added under `src/` is picked up automatically by the directory walk above,
/// which is the hole G2 had to plug with a second test; here there is no
/// hand-maintained list to go stale.
#[test]
fn every_library_module_is_covered_by_the_scan() {
    let names: Vec<String> = library_modules().into_iter().map(|(n, _)| n).collect();
    for expected in [
        "lib.rs",
        "tick.rs",
        "tables.rs",
        "broadphase.rs",
        "power.rs",
        "quartermaster.rs",
        "decision.rs",
        "voxels.rs",
        "hash.rs",
        "fixed.rs",
        "rng.rs",
    ] {
        assert!(
            names.iter().any(|n| n == expected),
            "{expected} is missing from src/ — the module list in the plan and the code disagree"
        );
    }
}

/// The confinement, asserted in the direction that can fail silently: the
/// harness binaries *do* read the clock. Without this, deleting every
/// measurement from the spike would make the test suite greener.
#[test]
fn the_harness_binaries_are_the_only_place_the_clock_lives() {
    let bins = binaries();
    assert_eq!(bins.len(), 2, "expected tickbench and costmodel");
    for (name, text) in &bins {
        assert!(
            text.contains("std::time::Instant"),
            "{name} does not read the clock, so it cannot be measuring anything"
        );
    }
}

/// The crate root carries the denials that cover every library module through
/// the module tree, and the spike-local `clippy.toml` exists to stop the
/// product's configuration leaking in through clippy's ancestor walk.
#[test]
fn the_crate_root_denies_the_determinism_lints() {
    let lib = fs::read_to_string(src_dir().join("lib.rs")).expect("lib.rs is readable");
    for attr in [
        "#![deny(clippy::as_conversions)]",
        "#![deny(clippy::float_arithmetic)]",
        "#![deny(clippy::disallowed_types)]",
    ] {
        assert!(lib.contains(attr), "src/lib.rs is missing {attr}");
    }
    let clippy = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("clippy.toml"))
        .expect("clippy.toml is readable");
    assert!(
        clippy.contains("msrv = \"1.98.1\""),
        "the spike-local clippy.toml must pin the same MSRV as rust-toolchain.toml"
    );
}
