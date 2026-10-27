// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! No silent fallback is left in the bench's code (S2's plan, E7, task `g3h`).
//!
//! The rule is the workspace's: every predictable failure returns a typed error
//! ([`pharmakos_bench::Error`]), and no value falls back to a default (a
//! saturate, a clamp, a swallowed error, a magic value or a default-on-error)
//! on a reachable path. A figure that could not be taken is an error, never a
//! zero. This is the bench's sweep held in place: the scan
//! `crates/mesher/tests/fallbacks.rs` runs over the mesher, run over every
//! file under `crates/bench/src`. A default the scan finds is either a typed
//! error, a failure made unrepresentable, or the exact answer by a rule, and
//! only the last kind is listed, in [`KEPT_FALLBACKS`], with the rule.
//!
//! The bench is never shipped, but its figures decide a gate, so a fallback
//! that turned a failed measurement into a plausible number is the failure
//! this crate most needs to rule out.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test fails by panicking; an unreadable source file is a broken checkout"
)]

use std::path::{Path, PathBuf};

/// The defaults the bench keeps, as `(file, a substring of the trimmed line,
/// the rule that makes the default the exact answer)`. Each entry matches
/// exactly one line.
const KEPT_FALLBACKS: &[(&str, &str, &str)] = &[
    (
        "src/stats.rs",
        "let count = u128::try_from(sample.len()).ok()?;",
        "a `usize` fits a `u128` on every target Rust builds for, so the `None` is unreachable; \
     `mean` answers `None` only for an empty sample, which the division says",
    ),
    (
        "src/decision.rs",
        "let per_unit = u128::try_from(self.ns_per_unit).ok().filter(|ns| *ns > 0)?;",
        "a cost per unit of zero or below fills no tick, so no ceiling exists and `None` is \
         that answer, as the method's docs and its tests state",
    ),
    (
        "src/g3/variant.rs",
        "let raise = draw.saturating_sub(power.core_surplus_kw);",
        "the raise is how far the gate's draw passes the shipped surplus, and zero when the \n         surplus already covers it: the floor is the rule",
    ),
    (
        "tests/g3_real.rs",
        "let _ = ALLOCATIONS.try_with(",
        "a `const` thread-local with no destructor is always accessible, so the error arm is \
         unreachable, and an allocator has nowhere to report one",
    ),
    (
        "tests/harness.rs",
        ".unwrap_or_else(|error| panic!(",
        "a test fails by panicking: this is the failure, named, not a default",
    ),
];

/// The needles a fallback is written with, as the mesher's scan has them.
const FALLBACK_NEEDLES: &[&str] = &[
    "unwrap_or",
    "map_or(",
    "map_or_else(",
    ".ok()?",
    ".ok().and_then(",
    ".clamp(",
    ".max(0)",
    "saturating_",
    ".ok().filter(",
    "let _ =",
];

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("a source directory reads") {
        let path = entry.expect("a directory entry reads").path();
        if path.is_dir() {
            walk(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

/// The crate's source files, every directory under `src` and `tests`, sorted,
/// less this file, whose needles are its data.
fn sources() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    walk(&root().join("src"), &mut found);
    walk(&root().join("tests"), &mut found);
    found.retain(|path| name(path) != "tests/fallbacks.rs");
    found.sort();
    found
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A source file's path under the crate, with `/` between its parts.
fn name(path: &Path) -> String {
    path.strip_prefix(root())
        .expect("a source file under the crate")
        .to_string_lossy()
        .replace('\\', "/")
}

/// The file's code: comment-only lines removed, and everything from the first
/// `#[cfg(test)]` onward, as the mesher's scan reads its files.
fn code_lines(text: &str) -> Vec<String> {
    let shipped = match text.find("#[cfg(test)]") {
        Some(index) => text
            .get(..index)
            .expect("a match starts on a char boundary"),
        None => text,
    };
    shipped
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .map(str::to_owned)
        .collect()
}

/// The fallback lines of one file, trimmed: every line holding a needle, and
/// every `let Ok(..) .. else {` whose body returns anything but an `Err`.
fn fallback_lines(text: &str) -> Vec<String> {
    let lines = code_lines(text);
    let mut found: Vec<String> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if FALLBACK_NEEDLES.iter().any(|needle| line.contains(needle)) {
            found.push(line.trim().to_owned());
            continue;
        }
        if line.contains("let Ok(") && line.trim_end().ends_with("else {") {
            let body = lines.get(index + 1).map_or("", |next| next.trim());
            if body.starts_with("return") && !body.starts_with("return Err(") {
                found.push(line.trim().to_owned());
            }
        }
    }
    found
}

/// Every fallback in the crate's code, as `(file, the line)`.
fn fallbacks() -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for path in sources() {
        let text = std::fs::read_to_string(&path).expect("a source file reads");
        for line in fallback_lines(&text) {
            found.push((name(&path), line));
        }
    }
    found
}

/// The scan's positive control: planted text holding one of each form is
/// found, a typed `let Ok(..) else { return Err(..) }` and comment prose are
/// not, and nothing after `#[cfg(test)]` is read.
#[test]
fn the_fallback_scan_sees_every_form_it_names() {
    let planted = "fn a() {\n    let x = y.unwrap_or(0);\n    let x = y.unwrap_or_default();\n    \
                   let x = y.unwrap_or_else(f);\n    let x = y.map_or(0, f);\n    \
                   let x = y.map_or_else(g, f);\n    let x = y.ok()?;\n    let x = \
                   y.ok().and_then(f);\n    let x = y.clamp(1, 9);\n    let x = y.max(0);\n    \
                   let x = y.saturating_add(1);\n    let x = y.ok().filter(f);\n    \
                   let _ = y;\n    let Ok(x) = y else {\n        return 0;\n    };\n    let Ok(x) = y else {\n        \
                   return Err(e);\n    };\n    // y.unwrap_or(0) in prose\n}\n#[cfg(test)]\n\
                   fn b() { y.unwrap_or(0); }\n";
    assert_eq!(
        fallback_lines(planted),
        [
            "let x = y.unwrap_or(0);",
            "let x = y.unwrap_or_default();",
            "let x = y.unwrap_or_else(f);",
            "let x = y.map_or(0, f);",
            "let x = y.map_or_else(g, f);",
            "let x = y.ok()?;",
            "let x = y.ok().and_then(f);",
            "let x = y.clamp(1, 9);",
            "let x = y.max(0);",
            "let x = y.saturating_add(1);",
            "let x = y.ok().filter(f);",
            "let _ = y;",
            "let Ok(x) = y else {",
        ],
        "every form the scan names, once, and nothing else"
    );
}

/// No value in the bench's code falls back to a default instead of saying
/// what went wrong, except the defaults [`KEPT_FALLBACKS`] lists with the rule
/// that makes each one the exact answer.
#[test]
fn no_silent_fallback_is_left() {
    let found = fallbacks();
    let unexplained: Vec<String> = found
        .iter()
        .filter(|(file, line)| {
            !KEPT_FALLBACKS
                .iter()
                .any(|(kept_file, code, _)| kept_file == file && line.contains(code))
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
    for (file, code, why) in KEPT_FALLBACKS {
        let count = found
            .iter()
            .filter(|(found_file, line)| found_file == file && line.contains(code))
            .count();
        assert_eq!(
            count, 1,
            "KEPT_FALLBACKS' entry in {file} (`{code}`, kept because {why}) matches {count} \
             lines; an entry that matches nothing is stale, and one that matches two hides one"
        );
    }
}

/// The scan must read every file, the ones under `src/g3/` and `tests/` among them, or the
/// assertions above pass over nothing.
#[test]
fn the_fallback_scan_reads_the_whole_crate() {
    let names: Vec<String> = sources().iter().map(|path| name(path)).collect();
    assert_eq!(
        names,
        [
            "src/decision.rs",
            "src/error.rs",
            "src/g3/fourth.rs",
            "src/g3/layout.rs",
            "src/g3/segment.rs",
            "src/g3/sight.rs",
            "src/g3/stream.rs",
            "src/g3/support.rs",
            "src/g3/sweep.rs",
            "src/g3/variant.rs",
            "src/g3.rs",
            "src/heap.rs",
            "src/hosted.rs",
            "src/lib.rs",
            "src/quick.rs",
            "src/stats.rs",
            "tests/g3_real.rs",
            "tests/harness.rs",
            "tests/perf_alarm.rs",
        ],
        "the scan reads every source file of the crate; a new one joins this list"
    );
    assert_eq!(fallbacks().len(), KEPT_FALLBACKS.len());
}
