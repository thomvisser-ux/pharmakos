// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The two rules this crate is confined by, asserted against its own **source
//! text**.
//!
//! The lints already deny most of it, and `cargo xtask ci`'s `research-guard`
//! already proves the dependency edge. This file exists because of the one
//! lesson the G2 and G3-prime spikes both wrote down and `crates/sim` copied:
//! **copy the test, do not merely rely on the lint.** A lint can be switched off
//! by an `#[allow]` somebody adds in a hurry; a test that greps the source
//! cannot be, and it fails in the pull request that tries.
//!
//! # 1. No dry runs
//!
//! AGENTS.md section 3 rule 2: the verifier may estimate — pathfinder travel
//! over known terrain, interface-time arithmetic, `$` and `kW` projection,
//! placement legality, selector previews, mast coverage. It may never step the
//! sim, run mandates, programs, combat or construction, model enemy behaviour,
//! or evaluate rule conditions over a projected future. The crate depends on
//! `pharmakos-sim` for its **type surface only**, with `default-features =
//! false`, and this test asserts that no spelling of the stepping API appears in
//! the source at all.
//!
//! # 2. Deterministic like the sim
//!
//! No hash container, no wall clock, no float. A report that differed between
//! Windows, Linux and macOS would be a determinism hole in the verifier rather
//! than a report change, and `tests/golden/verifier/README.md` says so.
//!
//! Prose **about** a banned name is allowed: a comment-only line is skipped, for
//! the same reason `crates/sim/tests/confinement.rs` skips one. A rule has to be
//! writable down.

use std::path::PathBuf;

/// Spellings of the sim's stepping and world-building API.
///
/// `step(` catches `world.step(...)` and every wrapper whose name ends in
/// `step`; `phase_` catches a tick phase reached by name; `World::new` catches
/// building a world to run one.
const NO_DRY_RUNS: &[(&str, &str)] = &[
    (
        "step(",
        "stepping the sim is a dry run (AGENTS.md §3 rule 2)",
    ),
    (
        "fork",
        "fork exists only in the research build and no deterministic client may reach it",
    ),
    (
        "World::new",
        "the verifier reads a snapshot; it does not build a world to run",
    ),
    (
        "phase_",
        "a tick phase is the sim's, and nothing here may reach into one",
    ),
];

/// Determinism hazards, the same list `crates/sim` is held to.
const BANNED: &[(&str, &str)] = &[
    (
        "HashMap",
        "iteration order is randomly seeded per process (AGENTS.md §4.4)",
    ),
    (
        "HashSet",
        "iteration order is randomly seeded per process (AGENTS.md §4.4)",
    ),
    (
        "RandomState",
        "the per-process random seed behind HashMap's order",
    ),
    (
        "DefaultHasher",
        "SipHash with unstable output across toolchain versions",
    ),
    ("Instant", "wall-clock time (AGENTS.md §4.5)"),
    ("SystemTime", "wall-clock time, and it can move backwards"),
    ("f32", "the verifier is integer-only (AGENTS.md §4.2)"),
    ("f64", "the verifier is integer-only (AGENTS.md §4.2)"),
    ("rayon", "nothing here is parallelised (AGENTS.md §4.6)"),
    ("par_iter", "nothing here is parallelised"),
    (
        "thread_rng",
        "unseeded RNG; the project uses split seeded streams (AGENTS.md §4.7)",
    ),
    (
        "xxhash",
        "there is exactly one hash function and it is the sim's (AGENTS.md §5)",
    ),
    (
        ".unwrap()",
        "a panic in the verifier turns a bad playbook into a crash",
    ),
    (
        ".expect(",
        "a panic in the verifier turns a bad playbook into a crash",
    ),
];

/// Every `.rs` file under `crates/verifier/src`, in path order.
///
/// `std::fs::read_dir` is on `clippy.toml`'s disallowed list because directory
/// order differs between ext4, NTFS and APFS. The sanctioned use is exactly this
/// one: collect, sort, then use.
#[allow(
    clippy::disallowed_methods,
    reason = "clippy.toml's own reason string sanctions read_dir when the listing is collected and sorted before use, which is what happens below"
)]
fn sources() -> Vec<PathBuf> {
    let mut stack: Vec<PathBuf> = vec![PathBuf::from("src")];
    let mut found: Vec<PathBuf> = Vec::new();
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        let mut here: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .collect();
        here.sort();
        for path in here {
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// The file's shipped code: comment-only lines removed, so prose about a banned
/// name is not mistaken for a use of it, and everything from the first
/// `#[cfg(test)]` onward removed, because a unit test is allowed to panic — that
/// is what a failing assertion is — and is not compiled into the game.
fn code_lines(text: &str) -> Vec<(usize, String)> {
    let shipped = match text.find("#[cfg(test)]") {
        Some(index) => text.get(..index).unwrap_or(text),
        None => text,
    };
    shipped
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim_start().starts_with("//"))
        .map(|(number, line)| (number + 1, (*line).to_owned()))
        .collect()
}

#[test]
fn the_crate_has_sources_to_check() {
    let files = sources();
    assert!(
        files.len() >= 10,
        "the source walk found only {} files; it is looking in the wrong place and would pass \
         vacuously",
        files.len()
    );
    for expected in ["lib.rs", "decode.rs", "semantics.rs", "hash.rs"] {
        assert!(
            files.iter().any(|path| path.ends_with(expected)),
            "{expected} is outside the checked set"
        );
    }
}

#[test]
fn no_stepping_api_is_named_anywhere_in_the_crate() {
    let mut findings: Vec<String> = Vec::new();
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (number, line) in code_lines(&text) {
            for (needle, why) in NO_DRY_RUNS {
                if line.contains(needle) {
                    findings.push(format!(
                        "{}:{number}: `{needle}` — {why}\n    {}",
                        path.display(),
                        line.trim()
                    ));
                }
            }
        }
    }
    assert!(
        findings.is_empty(),
        "the no-dry-runs rule is broken in the source text:\n{}",
        findings.join("\n")
    );
}

#[test]
fn no_clock_and_no_hash_map_reach_the_crate() {
    let mut findings: Vec<String> = Vec::new();
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (number, line) in code_lines(&text) {
            for (needle, why) in BANNED {
                if line.contains(needle) {
                    findings.push(format!(
                        "{}:{number}: `{needle}` — {why}\n    {}",
                        path.display(),
                        line.trim()
                    ));
                }
            }
        }
    }
    assert!(
        findings.is_empty(),
        "the determinism rule set is broken in the source text:\n{}",
        findings.join("\n")
    );
}

#[test]
fn the_source_scan_is_not_looking_at_an_empty_set() {
    // The hole this closes is a scan that reads nothing and passes: if the walk
    // or the comment filter breaks, every test above goes green over an
    // unchecked crate. So assert that the filter keeps the code and drops the
    // prose, on a spelling that is in the source right now.
    let path = PathBuf::from("src").join("semantics.rs");
    let text = std::fs::read_to_string(&path).expect("semantics.rs is where it always was");
    let lines = code_lines(&text);
    assert!(lines.len() > 50, "the comment filter ate the file");
    assert!(
        text.contains("No dry runs"),
        "the heading this crate is confined by is no longer written in src/semantics.rs"
    );
    assert!(
        !lines.iter().any(|(_, line)| line.contains("No dry runs")),
        "the heading survives the comment filter, so the filter is not filtering"
    );
}

#[test]
fn the_manifest_declares_no_feature_and_takes_the_sim_without_default_features() {
    let manifest = std::fs::read_to_string("Cargo.toml").expect("the crate's own manifest");
    assert!(
        !manifest
            .lines()
            .any(|line| line.trim_start().starts_with("[features]")),
        "this crate declares a feature section; AGENTS.md §3 rule 1 says it declares none, so \
         that no feature of its own can ever forward one of the sim's. (The manifest's comment \
         about not having one is fine: the check reads section headers, not prose.)"
    );
    assert!(
        manifest.contains("pharmakos-sim = { workspace = true, default-features = false }"),
        "the sim dependency must be spelled out with default-features = false, because that \
         exact spelling is what a reader checks (AGENTS.md §3 rule 1)"
    );
}

#[test]
fn every_allow_is_audited_and_carries_a_reason() {
    let mut findings: Vec<String> = Vec::new();
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (number, line) in code_lines(&text) {
            if line.contains("#[allow(") || line.contains("#![allow(") {
                findings.push(format!("{}:{number}: {}", path.display(), line.trim()));
            }
        }
    }
    assert!(
        findings.is_empty(),
        "this crate carries no `#[allow]` at all, and adding one is a review item — a determinism \
         lint may never be switched off outside a walled crate (AGENTS.md §4.9):\n{}",
        findings.join("\n")
    );
}
