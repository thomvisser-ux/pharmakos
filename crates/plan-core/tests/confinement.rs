// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The two rules this crate is confined by, asserted against its own **source
//! text**.
//!
//! The lints already deny most of it and `cargo xtask ci`'s `research-guard`
//! already proves the dependency edge. This file exists because of the lesson
//! the G2 and G3-prime spikes both wrote down, which `crates/sim` and
//! `crates/verifier` have each copied: **copy the test, do not merely rely on
//! the lint.** A lint can be switched off by an `#[allow]` somebody adds in a
//! hurry; a test that greps the source cannot be, and it fails in the pull
//! request that tries.
//!
//! # 1. No dry runs
//!
//! AGENTS.md section 3 rule 2: `plan-core` may estimate — pathfinder travel
//! over known terrain, interface-time arithmetic, `$` and `kW` projection,
//! placement legality, selector previews, mast coverage. It may never step the
//! sim, run mandates, programs, combat or construction, model enemy behaviour,
//! or evaluate rule conditions over a projected future. The crate depends on
//! `pharmakos-sim` for its **type surface only**, with `default-features =
//! false`, and this test asserts that no spelling of the stepping API appears
//! in the source at all.
//!
//! The travel estimator is the sharp case, and it is why the seam is a trait:
//! `crate::travel::TravelEstimator` names the shape decisions-log item 61
//! froze and **nothing implements it here**, so this crate cannot reach the
//! sim's search even by accident.
//!
//! # 2. Deterministic like the sim
//!
//! No hash container, no wall clock, no float. Prose that differed between
//! Windows, Linux and macOS would be a determinism hole in the editor rather
//! than a wording change, and `tests/golden/plan-core/README.md` says so.
//!
//! Prose **about** a banned name is allowed: a comment-only line is skipped,
//! for the same reason `crates/sim` and `crates/verifier` skip one. A rule has
//! to be writable down.

use std::path::PathBuf;

/// Spellings of the sim's stepping and world-building API.
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
        "plan-core reads a snapshot; it does not build a world to run",
    ),
    (
        "phase_",
        "a tick phase is the sim's, and nothing here may reach into one",
    ),
    (
        "research",
        "the research feature is the sim's alone and this crate must not name it",
    ),
];

/// Determinism hazards, the same list `crates/sim` and `crates/verifier` are
/// held to.
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
    ("f32", "plan-core is integer-only (AGENTS.md §4.2)"),
    ("f64", "plan-core is integer-only (AGENTS.md §4.2)"),
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
        "a panic in the plan core turns a bad playbook into a crash",
    ),
    (
        ".expect(",
        "a panic in the plan core turns a bad playbook into a crash",
    ),
];

/// Every `.rs` file under `crates/plan-core/src`, in path order.
///
/// `std::fs::read_dir` is on `clippy.toml`'s disallowed list because directory
/// order differs between ext4, NTFS and APFS. The sanctioned use is exactly
/// this one: collect, sort, then use.
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

/// The file's shipped code: comment-only lines removed, so prose about a
/// banned name is not mistaken for a use of it, and everything from the first
/// `#[cfg(test)]` onward removed, because a unit test is allowed to panic —
/// that is what a failing assertion is — and is not compiled into the game.
fn code_lines(text: &str) -> Vec<(usize, String)> {
    let shipped = match text.find("#[cfg(test)]") {
        Some(index) => text.get(..index).unwrap_or(text),
        None => text,
    };
    numbered(shipped)
}

/// Every line of the file's code, unit-test modules **included**: comment-only
/// lines removed and nothing else.
fn all_lines(text: &str) -> Vec<(usize, String)> {
    numbered(text)
}

fn numbered(text: &str) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim_start().starts_with("//"))
        .map(|(number, line)| (number.saturating_add(1), (*line).to_owned()))
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
    for expected in [
        "lib.rs",
        "jsonc.rs",
        "canonical.rs",
        "patch.rs",
        "render.rs",
    ] {
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
        for (number, line) in all_lines(&text) {
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
        "plan-core may estimate, never rehearse (AGENTS.md §3 rule 2):\n{}",
        findings.join("\n")
    );
}

#[test]
fn no_determinism_hazard_is_named_in_the_shipped_code() {
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
        "a plan-core rendering must be the same on Windows, Linux and macOS:\n{}",
        findings.join("\n")
    );
}

/// The only `#[allow]` this crate carries, and the reason `clippy.toml` itself
/// gives for it.
///
/// AGENTS.md section 4.9 is explicit that an `#[allow]` on a determinism lint
/// outside a walled crate is a workaround wearing a disguise. This test is the
/// inventory that keeps the claim honest: it names the one that is there, in
/// the module it is in, so a second one has to arrive in a pull request that
/// also edits this list.
///
/// The walk is `src/` only, and the name says so. This file carries a second
/// `#[allow]` of its own — the same sanctioned `read_dir`, in `sources` above
/// — and a test walking itself would be counting its own scaffolding.
#[test]
fn the_only_allow_in_src_is_the_sanctioned_read_dir() {
    let mut found: Vec<String> = Vec::new();
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (number, line) in all_lines(&text) {
            if line.contains("#[allow(") || line.contains("#![allow(") {
                found.push(format!("{}:{number}", path.display()));
            }
        }
    }
    assert_eq!(
        found.len(),
        1,
        "exactly one `#[allow]` under `src/`, in library.rs, for the read_dir clippy.toml          sanctions: {found:?}"
    );
    assert!(
        found.first().is_some_and(|at| at.contains("library.rs")),
        "the one allow moved: {found:?}"
    );
}

/// The `research` feature is a compile-time fact, not only a source-text one.
#[test]
fn the_crate_declares_no_features_of_its_own() {
    let manifest = std::fs::read_to_string("Cargo.toml").expect("this crate's own manifest");
    assert!(
        !manifest
            .lines()
            .any(|line| line.trim_start().starts_with("[features]")),
        "plan-core declares no [features] section: the research feature is the sim's alone \
         (AGENTS.md §3 rule 1)"
    );
    assert!(
        manifest.contains("pharmakos-sim = { workspace = true, default-features = false }"),
        "the sim dependency must read `default-features = false`, which is the form \
         `cargo xtask ci`'s research-guard checks for"
    );
}
