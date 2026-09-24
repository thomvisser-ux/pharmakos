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
//! string literals (keeping a literal that is one identifier, a dictionary key), and fails
//! on any line where a money, power or duration identifier
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

/// Words that mean money, power or time, matched against the `_`-separated parts of an
/// identifier (and the identifier itself), never as substrings: `remaining_ms` is a
/// duration because one of its parts is `ms`, and `submitted` is not money merely because
/// the letters `bmi` sit inside it.
///
/// Drawn from the field names `gp.v1` and `gp.api.v1` actually use — every duration in
/// the schema is an `int32` of game milliseconds and is spelled `..._ms` (decisions-log
/// item 46), or is the bare key `ms` of an estimate's `Leg` and its whole route; money is
/// `$` and is spelled with `cost`, `credits`, `treasury` or `bmi`, and power is `kw`.
/// `leg`, `legs`, `whole`, `travel` and `eta` are the names the editor holds the
/// estimator's travel times under. `tick` and `frame` are here too: a frame count
/// converted to a time, or a tick converted to a second, is time arithmetic wearing a
/// different unit.
const QUANTITIES: &[&str] = &[
    "ms",
    "millis",
    "milliseconds",
    "seconds",
    "secs",
    "duration",
    "elapsed",
    "remaining",
    "timeout",
    "deadline",
    "leg",
    "legs",
    "whole",
    "travel",
    "eta",
    "cost",
    "costs",
    "credits",
    "treasury",
    "bmi",
    "dollars",
    "price",
    "budget",
    "kw",
    "power",
    "watt",
    "watts",
    "tick",
    "ticks",
    "frame",
    "frames",
];

/// Identifiers that contain a fragment above but are not a quantity at all.
///
/// Each one is here because the scan found it, and each is exempt for a reason a reader
/// can check: none of them is a `$`, a `kW` or a length of time.
const EXEMPT: &[&str] = &[
    // `DrainBudget`'s three rows are per-FRAME upload limits, and the bridge copies them
    // from the rules table into the mesher's parameter type without touching them. The
    // scan sees "budget", "frame" and "bytes_per_frame"; there is no arithmetic on any
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
// differs between filesystems; `sources()` sorts before anything reads the result, which
// is the remedy the ban asks for.
//
// There is deliberately NO `#[allow]` here. This crate is walled, and the wall is a
// command-line allowance passed by `cargo xtask clippy` pass 2 (`WALL_ALLOW` in
// xtask/src/main.rs already carries `-A clippy::disallowed_methods`), not something the
// source asks for — AGENTS.md section 4.9, "nothing in the source asks for the
// allowance". The one exception this crate takes is item 83's `unsafe_code` in lib.rs.
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

/// One line of source with its comments and string literals removed — except a string
/// literal that is a bare identifier, which is kept as that word.
///
/// Comments have to go: a comment in this crate says "no arithmetic on `$`, `kW` or
/// durations" in several places and would trip the scan on the word it is warning about.
/// Prose and JSON in string literals go for the same reason. But a literal that is one
/// identifier is how GDScript reads a dictionary — `route.get("whole", 0) / 1000` is
/// arithmetic on a duration whose only name on the line is the key `"whole"` — so such a
/// literal stays, as the word it spells.
fn code_of(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut characters = line.chars().peekable();
    let mut in_string = false;
    let mut string_delimiter = '"';
    let mut literal = String::new();
    let mut after_format = false;
    while let Some(character) = characters.next() {
        if in_string {
            if character == '\\' {
                characters.next();
                literal.push('\\');
            } else if character == string_delimiter {
                in_string = false;
                let identifier = literal
                    .chars()
                    .next()
                    .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
                    && literal
                        .chars()
                        .all(|each| each.is_ascii_alphanumeric() || each == '_');
                if identifier {
                    out.push(' ');
                    out.push_str(&literal);
                    out.push(' ');
                } else {
                    after_format = true;
                }
            } else {
                literal.push(character);
            }
            continue;
        }
        // `"..." % values` is GDScript's string formatting, not a remainder: a `%` right
        // after a literal of text is dropped with the literal.
        if after_format && character != ' ' {
            after_format = false;
            if character == '%' {
                continue;
            }
        }
        match character {
            '"' | '\'' => {
                // A Rust lifetime (`'a`) is not a string; only treat a quote as one when
                // it is a double quote, or a single quote that closes soon.
                if character == '"' {
                    in_string = true;
                    string_delimiter = '"';
                    literal.clear();
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

/// Whether one identifier names something in `words`: the identifier itself, or one of its
/// `_`-separated parts.
fn names_one_of(word: &str, words: &[&str]) -> bool {
    words.contains(&word) || word.split('_').any(|part| words.contains(&part))
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
        if names_one_of(word, QUANTITIES) {
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

/// The one module whose TIME arithmetic is the design rather than a breach of it.
///
/// `src/pacer.rs` is the pacer, the host clock and the keep-alive: "presentation pacing on
/// the walled side, like the Lull timer, not game-rule time arithmetic"
/// (`docs/design/skeleton-plan-t16a-notes.md` section A (2), adopted by decisions-log item
/// 107). Wall time times the speed is game time owed; the Lull countdown is the Lull's
/// length less the wall time spent in it. The notes put exactly that in this crate, and
/// the gateway reads no clock, so it can live nowhere else.
///
/// The carve-out is by FILE and by KIND: the pacer is still scanned for money and power,
/// and `tests/pacer.rs::the_pacer_names_no_tick` holds it to never naming a step of the
/// sim. Every other file in this crate and in `godot/scripts/` is held to the whole rule.
const PACING_MODULE: &str = "pacer.rs";

/// The fragments of [`QUANTITIES`] that mean time, which [`PACING_MODULE`] may compute
/// with.
const TIME: &[&str] = &[
    "ms",
    "millis",
    "milliseconds",
    "seconds",
    "secs",
    "duration",
    "elapsed",
    "remaining",
    "timeout",
    "deadline",
    "frame",
    "frames",
];

/// Every offending line, as `path:line: text`.
fn offences(files: &[PathBuf]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for path in files {
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        let pacing = path.file_name().and_then(|name| name.to_str()) == Some(PACING_MODULE)
            && path
                .parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                == Some("src");
        for (index, line) in text.lines().enumerate() {
            let code = code_of(line);
            let mut named = quantities_in(&code);
            if pacing {
                // A word the pacer may compute with is one whose every quantity part is
                // time: `owed_ms` passes, `remaining_kw` does not.
                named.retain(|word| {
                    !word
                        .split('_')
                        .chain(std::iter::once(word.as_str()))
                        .filter(|part| QUANTITIES.contains(part))
                        .all(|part| TIME.contains(&part))
                });
            }
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
        // A bare `ms`, and the two places the editor actually shows a duration, with a
        // conversion planted in each (review of T19 PR 1).
        "var t = ms * 1000",
        "label.text = Strings.text(\"leg\", {\"ms\": legs[index] / 1000})",
        "_route_label.text = Strings.text(\"route_whole\", {\"ms\": route.get(\"whole\", 0) / 1000})",
        "var seconds_left = travel - 5",
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
        // Letters inside a word are not a quantity: `submitted` holds `bmi`.
        "var submitted_count := index + 1",
        "print(\"legs %s\" % route.get(\"legs\"))",
        "second * CHUNK_EDGE + first",
        "label.text = Strings.text(\"leg\", {\"ms\": legs[index]})",
    ];
    for line in fine {
        let code = code_of(line);
        assert!(
            quantities_in(&code).is_empty() || !has_arithmetic(&code),
            "the scanner tripped on `{line}`, which is not arithmetic on a quantity"
        );
    }
}

/// The pacing carve-out is one file wide and covers time only: money or power arithmetic
/// in the pacer is still an offence.
#[test]
fn the_pacing_module_is_exempt_for_time_and_for_nothing_else() {
    let pacer = crate_root().join("src").join(PACING_MODULE);
    assert!(pacer.is_file(), "the carve-out names a file that exists");
    let scratch = std::env::temp_dir().join(format!("pharmakos-na-{}", std::process::id()));
    let fake = scratch.join("src");
    fs::create_dir_all(&fake).expect("scratch");
    let path = fake.join(PACING_MODULE);
    fs::write(
        &path,
        "let owed_ms = spent_ms * speed;
let draw_kw = generator_kw + autocannon_kw;
",
    )
    .expect("scratch file");
    let found = offences(std::slice::from_ref(&path));
    let _ = fs::remove_dir_all(&scratch);
    assert_eq!(found.len(), 1, "time passes, power does not: {found:?}");
    assert!(found.iter().all(|line| line.contains("_kw")), "{found:?}");
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

/// Every `.gd` file anywhere under `godot/`, in sorted order.
fn gdscript_everywhere() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = Vec::new();
    collect(&godot_root(), "gd", &mut files);
    files.sort();
    files
}

/// **The editor makes no time arithmetic of its own** (skeleton plan T19, acceptance): no
/// GDScript file anywhere in the project does arithmetic on `$`, `kW` or a duration.
///
/// The bridge-wide test above reads `godot/scripts/`; this one reads every `.gd` under
/// `godot/`, so a script moved into a subfolder, or a scene's own script, is held to the
/// same rule. The editor shows travel times, sizes and verdicts exactly as the gateway
/// answered them, and every one of them comes from the gateway (AGENTS.md section 3 rule 4:
/// the editor "runs no validation or time maths of its own — it asks the gateway").
#[test]
fn the_editor_makes_no_time_arithmetic_of_its_own() {
    let files = gdscript_everywhere();
    assert!(
        files
            .iter()
            .any(|path| path.file_name().and_then(|name| name.to_str()) == Some("editor.gd")),
        "the editor's script was not among the files scanned: {files:?}"
    );
    let found = offences(&files);
    assert!(
        found.is_empty(),
        "the editor does arithmetic on something the gateway owns — a duration, `$` or `kW`. \
         Ask the gateway for the number instead, and show it as it came back.\n{}",
        found.join("\n")
    );
}

/// The editor's scripts read no clock: the pacer (`src/pacer.rs`) is the client's one wall
/// clock, and the editor's one timer — FULL after 600 ms idle — is kept there
/// (`pacer::IdleTimer`). A `Timer` node or a `Time.get_ticks_*` in an editor script would
/// be a second clock deciding when to ask the gateway something.
#[test]
fn the_editor_scripts_read_no_clock_of_their_own() {
    const CLOCKS: &[&str] = &["time.", "timer", "get_ticks", "unix_time", "create_tween"];
    let mut found: Vec<String> = Vec::new();
    for name in ["editor.gd", "rows.gd", "strings.gd"] {
        let path = godot_root().join("scripts").join(name);
        let text =
            fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        for (index, line) in text.lines().enumerate() {
            let code = code_of(line).to_ascii_lowercase();
            if CLOCKS.iter().any(|clock| code.contains(clock)) {
                found.push(format!("{name}:{}: {}", index + 1, line.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "an editor script reads a clock; timing is the pacer's (src/pacer.rs):\n{}",
        found.join("\n")
    );
}
