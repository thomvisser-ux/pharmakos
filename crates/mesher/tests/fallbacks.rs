// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! No silent fallback is left in the mesher's shipped code (S2's plan, E7, task `view`).
//!
//! The rule is the workspace's: every predictable failure returns a typed error, and no
//! value falls back to a default (a saturate, a clamp, a swallowed error, a magic value or
//! a default-on-error) on a reachable path. Each crate's next owner sweeps its own, and this
//! is the mesher's sweep held in place: the same scan `crates/gateway/tests/confinement.rs`
//! runs over the gateway, run over `crates/mesher/src`. A default the scan finds is either
//! a typed error, a failure made unrepresentable, or the exact answer by a rule, and only
//! the last kind is listed, in [`KEPT_FALLBACKS`], with the rule that makes it exact.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test fails by panicking; an unreadable source file is a broken checkout"
)]

use std::path::PathBuf;

/// The defaults the mesher keeps, as `(file name, a substring of the trimmed line, the rule
/// that makes the default the exact answer)`. Each entry matches exactly one line.
const KEPT_FALLBACKS: &[(&str, &str, &str)] = &[
    (
        "bake.rs",
        "let widened_lo = lo.map(|value| value.saturating_sub(margin).max(0));",
        "a box is cut to the map, whose low edge is voxel 0 on every axis; the high edge is \
         cut beside it, and a box the cut empties is `None`",
    ),
    (
        "bake.rs",
        "low = low.min(was.min(now).saturating_sub(pad).max(0));",
        "a relight's box reaches down to a moved column's lower height less the pad and is \
         cut to the map's low edge, voxel 0, as every box is",
    ),
    (
        "bake.rs",
        ".map_or(self.params.light_max(), |value| *value)",
        "outside the map reads as sky (`light_at`'s documented rule), and an offset inside \
         the map is inside the field, which is the grid's voxel count long",
    ),
    (
        "bake.rs",
        ".map_or(0, |height| *height)",
        "a column outside the map is closed and contributes no height (`tallest_neighbour`'s \
         rule), and a column inside it is inside `heights`, which is the grid's column count long",
    ),
    (
        "bake.rs",
        ".map_or(AIR, |material| *material)",
        "outside the map is air (`material_at`'s rule), and `check` has held the materials to \
         the grid's voxel count before any bake reads one",
    ),
    (
        "lib.rs",
        "let index = i32::try_from(index).ok()?;",
        "`chunk_coords` answers `None` for a chunk outside the grid, and an index past \
         `i32::MAX` is past every grid, which holds fewer than 2^17 chunks",
    ),
];

/// The needles a fallback is written with: the calls that answer a default for a failure
/// (`unwrap_or`, and so `unwrap_or_default` and `unwrap_or_else`; `map_or`; `map_or_else`),
/// and the forms that turn a failure into one quietly -- an error read as nothing
/// (`.ok()?`, `.ok().and_then(`), a value pushed into range (`.clamp(`, `.max(0)`), and
/// `let Ok(..) else` answering with a value rather than an error.
const FALLBACK_NEEDLES: &[&str] = &[
    "unwrap_or",
    "map_or(",
    "map_or_else(",
    ".ok()?",
    ".ok().and_then(",
    ".clamp(",
    ".max(0)",
];

/// The crate's source files, sorted.
fn sources() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found: Vec<PathBuf> = std::fs::read_dir(&root)
        .expect("the crate's src directory reads")
        .map(|entry| entry.expect("a directory entry reads").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .collect();
    found.sort();
    found
}

/// The file's shipped code: comment-only lines removed, so prose *about* a fallback is not
/// mistaken for one, and everything from the first `#[cfg(test)]` onward removed, because a
/// unit test is not compiled into the game.
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

/// The fallback lines of one file's shipped code, trimmed: every line holding a needle, and
/// every `let Ok(..) .. else {` whose body returns anything but an `Err` (that line is the
/// one reported).
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

/// Every fallback in the crate's shipped code, as `(file name, the line)`.
fn fallbacks() -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for path in sources() {
        let text = std::fs::read_to_string(&path).expect("a source file reads");
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .expect("a source file has a name");
        for line in fallback_lines(&text) {
            found.push((name.clone(), line));
        }
    }
    found
}

/// The scan's positive control: planted text holding one of each form is found, a typed
/// `let Ok(..) else { return Err(..) }` and comment prose are not, and nothing after
/// `#[cfg(test)]` is read.
#[test]
fn the_fallback_scan_sees_every_form_it_names() {
    let planted = "fn a() {\n    let x = y.unwrap_or(0);\n    let x = y.unwrap_or_default();\n    \
                   let x = y.unwrap_or_else(f);\n    let x = y.map_or(0, f);\n    \
                   let x = y.map_or_else(g, f);\n    let x = y.ok()?;\n    let x = \
                   y.ok().and_then(f);\n    let x = y.clamp(1, 9);\n    let x = y.max(0);\n    \
                   let Ok(x) = y else {\n        return 0;\n    };\n    let Ok(x) = y else {\n        \
                   return Err(e);\n    };\n    // y.unwrap_or(0) in prose\n}\n#[cfg(test)]\n\
                   fn b() { y.unwrap_or(0); }\n";
    let found = fallback_lines(planted);
    assert_eq!(
        found,
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
            "let Ok(x) = y else {",
        ],
        "every form the scan names, once, and nothing else"
    );
}

/// No value in the mesher's shipped code falls back to a default instead of saying what
/// went wrong, except the defaults [`KEPT_FALLBACKS`] lists with the rule that makes each
/// one the exact answer. A new fallback fails here until it is a typed error or takes its
/// line and its reason in the list.
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
        assert!(!why.is_empty(), "{file}: a kept fallback names its rule");
    }
}

/// The scan must see the kept fallbacks, or every assertion above passes over nothing.
#[test]
fn the_fallback_scan_finds_what_it_looks_for() {
    let found = fallbacks();
    assert_eq!(
        found.len(),
        KEPT_FALLBACKS.len(),
        "the fallback scan sees {} lines and the list keeps {}",
        found.len(),
        KEPT_FALLBACKS.len()
    );
    let names: Vec<String> = sources()
        .iter()
        .filter_map(|path| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        ["bake.rs", "lib.rs", "sweep.rs", "upload.rs"],
        "the scan reads every source file of the crate; a new one joins this list"
    );
    assert!(
        found.iter().any(|(file, _)| file == "lib.rs"),
        "the fallback scan no longer sees lib.rs' kept read"
    );
}
