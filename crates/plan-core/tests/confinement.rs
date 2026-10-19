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
        "exactly one `#[allow]` under `src/`, in library.rs, for the read_dir clippy.toml \
         sanctions: {found:?}"
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

/// Every fallback this crate's shipped code keeps, with the rule that makes
/// its answer exact rather than a silent default, and how many lines carry it
/// (S2's plan, E7's sweep over `crates/plan-core`; task `vfy2`).
///
/// The sweep fixed the three that could answer wrong: `verify.rs`' blanked
/// file, which now builds a `String` by construction rather than converting
/// bytes back with the raw text as its fallback; `library.rs`' filled
/// parameter, which refuses a declared parameter the template does not hold
/// rather than filling it with nothing; and `render.rs`' list joiner, which
/// splits the list rather than reading its ends through defaults.
const KEPT_FALLBACKS: &[(&str, &str, usize, &str)] = &[
    (
        "canonical.rs",
        "split_pointer(pointer).unwrap_or_default()",
        1,
        "the pointer is an anchor the parser built, which always splits; an empty token list \
         relocates the comment to the root, which exists",
    ),
    (
        "canonical.rs",
        "char::from_digit(nibble, 16).unwrap_or('0')",
        1,
        "a nibble is masked to 0-15, every one of which is a hex digit",
    ),
    (
        "jsonc.rs",
        ".unwrap_or_else(|| escape(key));",
        1,
        "a key moved to another name is written in that name's canonical spelling, the \
         module's stated rule",
    ),
    (
        "jsonc.rs",
        "trivia.get(..at).unwrap_or_default()",
        2,
        "`at` is where `find` found a newline, so it is a character boundary inside the text",
    ),
    (
        "jsonc.rs",
        "trivia.get(at..).unwrap_or_default()",
        2,
        "as the head, above",
    ),
    (
        "jsonc.rs",
        ".map_or(trivia.len(), |offset|",
        2,
        "a comment with no end runs to the end of its trivia, which is how the reader reads an \
         unterminated one",
    ),
    (
        "jsonc.rs",
        "self.text.get(self.pos..).unwrap_or_default()",
        1,
        "`pos` is a character boundary the reader advanced to, and an empty rest is refused on \
         the next line as a string that ended inside a character",
    ),
    (
        "jsonc.rs",
        "char::from_digit(nibble, 16).unwrap_or('0')",
        1,
        "as canonical.rs': a masked nibble is a hex digit",
    ),
    (
        "library.rs",
        ".unwrap_or_default()",
        2,
        "a template with no `meta` has no title to list (the verifier names the missing block), \n         and a note with no first line has no blurb",
    ),
    (
        "patch.rs",
        "position.unwrap_or(len).min(len)",
        1,
        "no remembered position means a new member, which goes last; a remembered one past the \
         end goes last as well, which is where the member it followed now ends",
    ),
    (
        "render.rs",
        ".unwrap_or(s::UNTITLED);",
        1,
        "a playbook with no title is named as untitled, in words the string table holds",
    ),
    (
        "render.rs",
        "target.map_or_else(String::new, |target| target.blueprint_id.clone())",
        1,
        "a row with no target names no blueprint; the verifier refuses the row",
    ),
    (
        "render.rs",
        ".map_or_else(String::new, |area| {",
        1,
        "a target with no box says nothing of one",
    ),
    (
        "render.rs",
        "at.map_or_else(|| s::PLACE_NOWHERE.to_owned(), voxel)",
        1,
        "a corner the file leaves out is said to be nowhere the file names",
    ),
    (
        "render.rs",
        "test.map_or_else(",
        3,
        "a predicate with no comparison is said to compare to nothing the file names",
    ),
    (
        "render.rs",
        ".checked_div(",
        3,
        "each divisor is a non-zero constant, so the division is never refused",
    ),
    (
        "travel.rs",
        ".unwrap_or(MAX_SECONDS);",
        1,
        "rounding up past i32 saturates upwards, the one direction the estimator may err in",
    ),
    (
        "travel.rs",
        "seconds.checked_div(60).unwrap_or(0)",
        1,
        "the divisor is a non-zero constant",
    ),
];

/// Every fallback call in the crate's shipped code, as `(file name, the
/// line)`: `unwrap_or` (and so `unwrap_or_default` and `unwrap_or_else`),
/// `map_or` and `map_or_else`.
fn fallbacks() -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(name) = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
        else {
            continue;
        };
        for (_, line) in code_lines(&text) {
            if line.contains("unwrap_or")
                || line.contains("map_or(")
                || line.contains("map_or_else(")
            {
                found.push((name.clone(), line.trim().to_owned()));
            }
        }
    }
    found
}

#[test]
fn no_silent_fallback_is_left() {
    let found = fallbacks();
    let unexplained: Vec<String> = found
        .iter()
        .filter(|(file, line)| {
            !KEPT_FALLBACKS
                .iter()
                .any(|(kept_file, code, _, _)| kept_file == file && line.contains(code))
        })
        .map(|(file, line)| format!("{file}: {line}"))
        .collect();
    assert!(
        unexplained.is_empty(),
        "a value falls back to a default instead of saying what went wrong. Return a typed \
         error, make the failure unrepresentable, or, where the default is a rule that leaves \
         the answer exact, add the line to KEPT_FALLBACKS with that rule:\n{}",
        unexplained.join("\n")
    );
    for (file, code, lines, why) in KEPT_FALLBACKS {
        let count = found
            .iter()
            .filter(|(found_file, line)| found_file == file && line.contains(code))
            .count();
        assert_eq!(
            count, *lines,
            "KEPT_FALLBACKS' entry in {file} (`{code}`, kept because {why}) matches {count} \
             lines, not {lines}; a stale entry or an unseen second use hides one"
        );
    }
}
