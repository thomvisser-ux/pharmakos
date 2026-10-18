// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! No silent fallback is left in `crates/proto`'s hand-written sources (S2's
//! plan, E7).
//!
//! E7 names two patterns for the sim, `try_from(..).unwrap_or(..)` and
//! `.get(..)…unwrap_or(..)`, and has each crate's next S2 owner sweep its
//! own crate the same way, with the same test. This is that test for
//! `pharmakos-proto`. It reads one class wider than the two patterns, because
//! the sweep found the class here in four spellings: a conversion
//! (`try_from`), an indexed read (`get`, `first`, `last`) and a checked
//! operation (`checked_*`), each followed by an `unwrap_or` that answers a
//! made-up value where the code should either make the failure impossible
//! or say it happened.
//!
//! The scan is over statements, not lines, so a chain broken across lines is
//! still one statement: the source is cut at `;`, `{` and `}`, with `//`
//! comments removed first. The generated prost tree is left out: it is
//! `buf generate`'s, and `tests/generated.rs` holds it to a fresh generation.
//!
//! A statement may stay only through [`ALLOWED`], each entry with the reason
//! it is a real rule rather than a fallback. It holds one, the codec's
//! unknown-enum spelling, which the mapping requires; every other site the
//! sweep found was rewritten so the failure is unrepresentable (the chunk
//! codec and base64 read whole triples by type, the varint writer reads its
//! low byte off the value's bytes) or returns a typed error (the descriptor's
//! `int32`s, strings and reserved ranges, the writer's length prefix).

use std::fs;
use std::path::{Path, PathBuf};

/// Statements allowed to keep a fallback: (file, a fragment of the statement,
/// the reason). An entry needs its reason.
const ALLOWED: &[(&str, &str, &str)] = &[(
    "json/codec.rs",
    "declared.value_by_number(number).map_or_else(",
    "the proto3 JSON mapping's rule, not a fallback: an enum value the schema does not name is written as its number, so a reader of a newer value keeps it",
)];

/// What reads a value that may be missing or may not fit.
const RISKY: &[&str] = &["try_from(", ".get(", ".first()", ".last()", ".checked_"];

/// What answers a made-up value when it is: the prefix, so `unwrap_or`,
/// `unwrap_or_default` and `unwrap_or_else` all count.
const FALLBACK: &str = ".unwrap_or";

/// A `map_or` or `map_or_else` whose default is a constant answers a made-up
/// value just as `unwrap_or` does, whatever it reads; these are the openings
/// of such a default, whitespace removed.
const CONSTANT_DEFAULTS: &[&str] = &[
    ".map_or(0",
    ".map_or(1",
    ".map_or(2",
    ".map_or(3",
    ".map_or(4",
    ".map_or(5",
    ".map_or(6",
    ".map_or(7",
    ".map_or(8",
    ".map_or(9",
    ".map_or(-",
    ".map_or(\"",
    ".map_or(false",
    ".map_or(true",
    ".map_or(None",
    ".map_or(String::new()",
    ".map_or(Vec::new()",
    ".map_or(Default::default()",
    ".map_or(u",
    ".map_or(i",
    ".map_or_else(||",
    ".map_or_else(|_|",
];

fn source_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every `.rs` file under `directory`, in path order.
///
/// `std::fs::read_dir` is on `clippy.toml`'s disallowed list because directory
/// order differs between file systems; this listing is collected and sorted
/// before use, which is what the lint's reason string sanctions.
#[allow(
    clippy::disallowed_methods,
    reason = "clippy.toml sanctions read_dir when the listing is collected and sorted before use, which is what happens here"
)]
fn rust_files(directory: &Path, found: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()));
    entries.sort();
    for path in entries {
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "generated") {
                continue;
            }
            rust_files(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

/// The source with every `//` comment removed, outside string literals.
fn without_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let mut in_string = false;
        let mut escaped = false;
        let mut slash = false;
        let mut kept = String::with_capacity(line.len());
        for character in line.chars() {
            if in_string {
                in_string = escaped || character != '"';
                escaped = !escaped && character == '\\';
            } else if character == '"' {
                in_string = true;
            } else if character == '/' && slash {
                // The first slash of the pair was kept; drop it with the rest.
                kept.pop();
                break;
            }
            slash = !in_string && character == '/';
            kept.push(character);
        }
        out.push_str(&kept);
        out.push('\n');
    }
    out
}

/// One statement's text, whitespace collapsed, for the report.
fn collapsed(statement: &str) -> String {
    statement.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn flagged(statement: &str) -> bool {
    let tight: String = statement.split_whitespace().collect();
    (tight.contains(FALLBACK) && RISKY.iter().any(|risky| tight.contains(risky)))
        || CONSTANT_DEFAULTS
            .iter()
            .any(|opening| tight.contains(opening))
}

#[test]
fn no_silent_fallback_is_left() {
    for (_, _, reason) in ALLOWED {
        assert!(
            !reason.trim().is_empty(),
            "every allowed fallback names its reason"
        );
    }

    let root = source_root();
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    assert!(
        files.len() >= 8,
        "the scan found {} files under {}; it is not reading the crate",
        files.len(),
        root.display()
    );

    let mut found = Vec::new();
    for path in &files {
        let text =
            fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let relative = path
            .strip_prefix(&root)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
            .to_string_lossy()
            .replace('\\', "/");
        for statement in without_comments(&text).split([';', '{', '}']) {
            if !flagged(statement) {
                continue;
            }
            let shown = collapsed(statement);
            let allowed = ALLOWED
                .iter()
                .any(|(file, fragment, _)| *file == relative && shown.contains(fragment));
            if !allowed {
                found.push(format!("src/{relative}: {shown}"));
            }
        }
    }
    assert!(
        found.is_empty(),
        "a silent fallback in crates/proto (S2's plan, E7): make the failure \
         unrepresentable or return a typed error, or allow it in ALLOWED with the rule that \
         makes it right:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_scan_sees_each_spelling_of_the_class() {
    for statement in [
        "let length = u16::try_from(run).unwrap_or(0)",
        "let low = triple.get(1).copied().unwrap_or(0)",
        "let first = chunk\n    .first()\n    .copied()\n    .unwrap_or(0)",
        "let trailing = bytes.len().checked_rem(RUN_BYTES).unwrap_or(0)",
        "let low = triple.get(1).copied().unwrap_or_default()",
        "let n = u8::try_from(x).unwrap_or_else(|_| 0)",
        "let ms = resolved.travel_ms.map_or(0, Ms::raw)",
        "let text = found.map_or_else(|| String::from(\"none\"), str::to_owned)",
    ] {
        assert!(flagged(statement), "missed: {statement}");
    }
    assert!(!flagged(
        "let stripped = value_name.strip_prefix(&prefix).unwrap_or(value_name)"
    ));
    assert!(!flagged(&without_comments(
        "let ok = 1; // u16::try_from(run).unwrap_or(0)"
    )));
    assert!(flagged(&without_comments(
        "let url = \"a//b\"; let n = u8::try_from(x).unwrap_or(0)"
    )));
}
