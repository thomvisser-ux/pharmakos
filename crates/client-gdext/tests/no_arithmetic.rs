// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! **A source-level test that the bridge performs no arithmetic on `$`, `kW` or
//! durations** — skeleton plan T12's acceptance, word for word.
//!
//! AGENTS.md section 3 rule 4: "`client-gdext` contains marshalling and nothing else: no
//! rules, no time arithmetic, no validation, no gameplay decisions. The editor 'runs no
//! validation or time maths of its own' — it asks the gateway." The rule is easy to
//! believe today, when the bridge has no reason to touch a cost; it is easy to break in
//! six months, when a view wants a number rounded and the round trip to the gateway looks
//! like one call too many. A doc comment does not survive that. A test does.
//!
//! # What it does, and what it deliberately does not
//!
//! It **reads this crate's own source and the GDScript beside it**, strips comments and
//! string literals, and fails on any line where a money, power or duration identifier
//! appears next to an arithmetic operator. It is a text scan, not a type system — it will
//! not catch arithmetic on a variable named `x` that happens to hold a cost, and it does
//! not pretend to. What it does catch is the shape the rule is actually broken in:
//! somebody writing `remaining_ms / 1000` or `cost * count` in a bridge file.
//!
//! The scanner is itself tested, on snippets that must trip it and snippets that must not
//! (`the_scanner_catches_what_it_is_for` and `the_scanner_does_not_trip_on_ordinary_code`).
//! Without that, a scanner with a broken pattern would report "nothing found" for ever and
//! this file would be the constant the plan says not to assert.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules — \
              not an integration test's helper functions. A panic is this file's failure \
              report (clippy.toml's own wording)."
)]

use std::fs;
use std::path::{Path, PathBuf};

/// Fragments of an identifier that mean money, power or time.
///
/// Drawn from the field names `gp.v1` and `gp.api.v1` actually use — every duration in
/// the schema is an `int32` of game milliseconds and is spelled `..._ms` (decisions-log
/// item 46), money is `$` and is spelled with `cost`, `credits`, `treasury` or `bmi`, and
/// power is `kw`. `tick` and `frame` are here too: a frame count converted to a time, or
/// a tick converted to a second, is time arithmetic wearing a different unit.
const QUANTITIES: &[&str] = &[
    "_ms",
    "ms_",
    "millis",
    "seconds",
    "duration",
    "elapsed",
    "remaining",
    "timeout",
    "deadline",
    "cost",
    "credits",
    "treasury",
    "bmi",
    "dollars",
    "price",
    "budget_",
    "_kw",
    "kw_",
    "power",
    "watt",
    "tick",
    "frame",
];

/// Identifiers that contain a fragment above but are not a quantity at all.
///
/// Each one is here because the scan found it, and each is exempt for a reason a reader
/// can check: none of them is a `$`, a `kW` or a length of time.
const EXEMPT: &[&str] = &[
    // `DrainBudget`'s three rows are per-FRAME upload limits, and the bridge copies them
    // from the rules table into the mesher's parameter type without touching them. The
    // scan sees "budget_", "frame" and "bytes_per_frame"; there is no arithmetic on any
    // of them in this crate, and if one ever appears this exemption does not hide it,
    // because the exemption is by IDENTIFIER and the operator check still runs on the
    // rest of the line.
    "age_frames",
    "surfaces_per_frame",
    "bytes_per_frame",
    "budget",
    // `framework`-shaped words and Godot's own frame callbacks. `_process(delta)` is a
    // frame hook, not a duration the bridge computes with.
    "frame_post_draw",
    "physics_jitter_fix",
];

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn godot_root() -> PathBuf {
    crate_root().join("..").join("..").join("godot")
}

/// Every `.rs` under `src/`, and every `.gd` under `godot/scripts/`, in sorted order.
///
/// Both sides of the seam, because the rule binds both: "the editor runs none of its own,
/// and neither does the bridge".
fn sources() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = Vec::new();
    collect(&crate_root().join("src"), "rs", &mut files);
    collect(&godot_root().join("scripts"), "gd", &mut files);
    files.sort();
    assert!(
        files.len() >= 8,
        "the scan found only {} files; it is meant to read the whole bridge and the \
         GDScript beside it",
        files.len()
    );
    files
}

// `fs::read_dir` is on clippy.toml's disallowed-methods list because directory order
// differs between filesystems. Allowed here because `sources()` sorts before anything
// reads the result, which is the remedy the ban asks for.
#[allow(clippy::disallowed_methods)]
fn collect(dir: &Path, extension: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, extension, out);
        } else if path.extension().and_then(|value| value.to_str()) == Some(extension) {
            out.push(path);
        }
    }
}

/// One line of source with its comments and string literals removed.
///
/// Both have to go. A comment in this crate says "no arithmetic on `$`, `kW` or
/// durations" in several places and would trip the scan on the word it is warning about;
/// a string literal carries diagnostic prose and JSON with the same words in it.
fn code_of(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut characters = line.chars().peekable();
    let mut in_string = false;
    let mut string_delimiter = '"';
    while let Some(character) = characters.next() {
        if in_string {
            if character == '\\' {
                characters.next();
            } else if character == string_delimiter {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' | '\'' => {
                // A Rust lifetime (`'a`) is not a string; only treat a quote as one when
                // it is a double quote, or a single quote that closes soon.
                if character == '"' {
                    in_string = true;
                    string_delimiter = '"';
                    continue;
                }
                out.push(character);
            }
            '/' if characters.peek() == Some(&'/') => break,
            '#' => break, // GDScript comment
            _ => out.push(character),
        }
    }
    out
}

/// The quantity identifiers a line names, ignoring the exempt ones.
fn quantities_in(code: &str) -> Vec<String> {
    let lowered = code.to_ascii_lowercase();
    let mut found: Vec<String> = Vec::new();
    for word in
        lowered.split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
    {
        if word.is_empty() || EXEMPT.contains(&word) {
            continue;
        }
        if QUANTITIES.iter().any(|fragment| word.contains(fragment)) {
            found.push(word.to_owned());
        }
    }
    found
}

/// Whether a line carries an arithmetic operator, as opposed to one of the many other
/// things those characters mean in Rust.
fn has_arithmetic(code: &str) -> bool {
    let bytes = code.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        let previous = index.checked_sub(1).and_then(|before| bytes.get(before));
        let next = bytes.get(index.saturating_add(1));
        match byte {
            // `->` is a return type, `-` after `<` or `=` is an arrow or a comparison.
            b'-' => {
                if next != Some(&b'>') && previous != Some(&b'<') && previous != Some(&b'=') {
                    return true;
                }
            }
            // `*` is also a dereference (`&mut *x`), a raw pointer (`*const u8`) and a
            // glob import (`use a::*`). A multiplication is either spaced on both sides
            // or has its left operand right against it; none of the three is.
            b'*' => {
                let spaced = previous == Some(&b' ') && next == Some(&b' ');
                let left_operand = previous.is_some_and(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b')' | b']')
                });
                if spaced || left_operand {
                    return true;
                }
            }
            // `//` is a comment (already stripped) and a `/` inside a path is inside a
            // string literal (already stripped), so what is left is division. `+` is
            // also a trait bound (`impl A + B`) — counted anyway, because a bound never
            // sits on a line that also names a quantity.
            b'/' | b'%' | b'+' => return true,
            _ => {}
        }
    }
    false
}

/// Every offending line, as `path:line: text`.
fn offences(files: &[PathBuf]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for path in files {
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        for (index, line) in text.lines().enumerate() {
            let code = code_of(line);
            let named = quantities_in(&code);
            if named.is_empty() || !has_arithmetic(&code) {
                continue;
            }
            found.push(format!(
                "{}:{}: {} [{}]",
                path.display(),
                index.saturating_add(1),
                line.trim(),
                named.join(", ")
            ));
        }
    }
    found
}

#[test]
fn the_bridge_performs_no_arithmetic_on_money_power_or_durations() {
    let files = sources();
    let found = offences(&files);
    assert!(
        found.is_empty(),
        "the bridge marshals and decides nothing (AGENTS.md section 3 rule 4): no rules, no \
         validation, no time maths, on either side of the seam. These lines do arithmetic on \
         something the gateway owns — ask it instead.\n{}",
        found.join("\n")
    );
}

#[test]
fn the_scanner_catches_what_it_is_for() {
    // Each of these is a way the rule is actually broken.
    let broken = [
        "let seconds = status.phase_remaining_ms / 1000;",
        "let total = beacon.cost * count;",
        "self.treasury -= spend;",
        "var left := remaining_ms - elapsed_ms",
        "let draw = generator_kw + autocannon_kw;",
    ];
    for line in broken {
        let code = code_of(line);
        assert!(
            !quantities_in(&code).is_empty() && has_arithmetic(&code),
            "the scanner missed `{line}`, so it would miss the real thing"
        );
    }
}

#[test]
fn the_scanner_does_not_trip_on_ordinary_code() {
    let fine = [
        "let value = positions.len() + indices.len();",
        "use godot::prelude::*;",
        "fn caught(&self) -> u64 {",
        "let name = format!(\"chunk_{chunk}\");",
        "// the phase_remaining_ms field is never read here",
        "# remaining_ms belongs to the gateway",
        "report.set(&\"bytes_per_frame\".to_variant(), &value);",
    ];
    for line in fine {
        let code = code_of(line);
        assert!(
            quantities_in(&code).is_empty() || !has_arithmetic(&code),
            "the scanner tripped on `{line}`, which is not arithmetic on a quantity"
        );
    }
}

#[test]
fn the_scan_reads_the_gdscript_as_well_as_the_rust() {
    let files = sources();
    assert!(
        files
            .iter()
            .any(|path| path.extension().and_then(|value| value.to_str()) == Some("gd")),
        "the GDScript side of the seam was not scanned; the rule binds the editor too"
    );
    assert!(
        files.iter().any(|path| path.ends_with("bridge.rs")),
        "the bridge module itself was not scanned"
    );
}
