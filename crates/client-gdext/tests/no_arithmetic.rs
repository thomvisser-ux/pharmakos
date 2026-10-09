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
//! appears next to an arithmetic operator **or an arithmetic method** — `saturating_add`,
//! `checked_div`, `wrapping_mul`, `abs_diff`, `rem_euclid`, `.sum()`, GDScript's
//! `posmod` and `snapped` and the like ([`ARITHMETIC_METHODS`]; the register's S1-44,
//! ruled by S1's plan's decision 17, decisions-log item 128). It is a text scan, not a type
//! system — it will not catch arithmetic on a variable named `x` that happens to hold a
//! cost, and it does not pretend to. What it does catch is the shape the rule is actually
//! broken in: somebody writing `remaining_ms / 1000`, `cost * count` or
//! `deadline_us.saturating_add(period_us)` in a bridge file.
//!
//! A unit-test module in `src/` (everything after a file's `#[cfg(test)]` line) is not
//! scanned: it drives a stand-in clock and answers as the gateway would, which is a test
//! driver's arithmetic, never compiled into the client, as the checks' waits are a test
//! driver's clock (`no_script_reads_a_clock_but_the_id_helper_and_the_checks_waits`).
//!
//! One sum outside the pacer is the design, named here and nowhere wider:
//! [`SCHEDULING_SUMS`], `rig.rs`'s keyframe back-off, which is connection scheduling
//! (AGENTS.md section 3 rule 4's "the connection scheduling in `rig.rs`").
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
/// item 46), or is the bare key `ms` of an estimate's `Leg` and its whole route, and the
/// client's own wall durations are microseconds spelled `..._us` (`src/pacer.rs`); money is
/// `$` and is spelled with `cost`, `credits`, `treasury` or `bmi`, and power is `kw`.
/// `leg`, `legs`, `whole`, `travel` and `eta` are the names the editor holds the
/// estimator's travel times under. `tick` and `frame` are here too: a frame count
/// converted to a time, or a tick converted to a second, is time arithmetic wearing a
/// different unit.
const QUANTITIES: &[&str] = &[
    "ms",
    "millis",
    "milliseconds",
    "us",
    "micros",
    "microseconds",
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

/// The prefixes of Rust's integer arithmetic methods: `checked_add`, `saturating_sub`,
/// `wrapping_mul`, `overflowing_neg` and the rest of each family.
const METHOD_PREFIXES: &[&str] = &[
    "checked_",
    "saturating_",
    "wrapping_",
    "overflowing_",
    "unchecked_",
    "strict_",
];

/// Arithmetic spelt as a method or a function call, in either language: the operations the
/// families above prefix, the bare methods that compute (`pow`, `abs_diff`, `rem_euclid`,
/// an iterator's `sum` and `product`), and GDScript's arithmetic functions (`posmod`,
/// `fmod`, `snapped`). A call of one of these is arithmetic as surely as an operator is,
/// which the operator scan alone let through (the register's S1-44).
const ARITHMETIC_METHODS: &[&str] = &[
    "add",
    "sub",
    "mul",
    "div",
    "rem",
    "neg",
    "pow",
    "abs",
    "shl",
    "shr",
    "abs_diff",
    "div_euclid",
    "rem_euclid",
    "div_ceil",
    "div_floor",
    "next_multiple_of",
    "midpoint",
    "isqrt",
    "sum",
    "product",
    "posmod",
    "fposmod",
    "fmod",
    "snapped",
    "snappedi",
    "snappedf",
];

/// `std::time::Duration`'s unit conversions and scalings. Turning milliseconds into
/// seconds is the arithmetic the rule forbids, spelt through a type instead of `/ 1000`
/// (review of `ui`).
const DURATION_CONVERSIONS: &[&str] = &[
    "as_secs",
    "as_secs_f32",
    "as_secs_f64",
    "as_millis",
    "as_millis_f32",
    "as_millis_f64",
    "as_micros",
    "as_nanos",
    "subsec_millis",
    "subsec_micros",
    "subsec_nanos",
    "from_secs",
    "from_secs_f32",
    "from_secs_f64",
    "from_millis",
    "from_micros",
    "from_nanos",
    "mul_f32",
    "mul_f64",
    "div_f32",
    "div_f64",
    "div_duration_f32",
    "div_duration_f64",
];

/// Whether `word` names an arithmetic method: a family's prefix before an operation
/// (`checked_add`, `saturating_add_signed`), a bare one ([`ARITHMETIC_METHODS`]), or a
/// duration's conversion ([`DURATION_CONVERSIONS`]).
fn is_arithmetic_method(word: &str) -> bool {
    if DURATION_CONVERSIONS.contains(&word) {
        return true;
    }
    let operation = METHOD_PREFIXES
        .iter()
        .find_map(|prefix| word.strip_prefix(prefix))
        .unwrap_or(word);
    ARITHMETIC_METHODS.contains(&operation)
        || ARITHMETIC_METHODS.iter().any(|base| {
            operation
                .strip_prefix(base)
                .is_some_and(|rest| rest.starts_with('_') && operation != word)
        })
}

/// Whether a line calls or names an arithmetic method: such a word before `(` (spaces
/// allowed between, as GDScript allows them), before a turbofish (`sum::<u64>()`), or
/// after `::` as a path handed on uncalled (`fold(0, i64::saturating_add)`).
fn has_arithmetic_call(code: &str) -> bool {
    let characters: Vec<char> = code.chars().collect();
    let is_word = |character: &char| character.is_ascii_alphanumeric() || *character == '_';
    let mut index = 0;
    while let Some(character) = characters.get(index) {
        if !is_word(character) {
            index = index.saturating_add(1);
            continue;
        }
        let start = index;
        while characters.get(index).is_some_and(is_word) {
            index = index.saturating_add(1);
        }
        let word: String = characters
            .get(start..index)
            .unwrap_or_default()
            .iter()
            .collect::<String>()
            .to_ascii_lowercase();
        if !is_arithmetic_method(&word) {
            continue;
        }
        let path =
            start >= 2 && characters.get(start.saturating_sub(2)..start) == Some(&[':', ':'][..]);
        let mut after = index;
        while characters.get(after) == Some(&' ') {
            after = after.saturating_add(1);
        }
        let called = characters.get(after) == Some(&'(')
            || characters.get(index..index.saturating_add(3)) == Some(&[':', ':', '<'][..]);
        if path || called {
            return true;
        }
    }
    false
}

/// Whether a line carries an arithmetic operator or an arithmetic method, as opposed to
/// one of the many other things those characters mean in Rust.
fn has_arithmetic(code: &str) -> bool {
    if has_arithmetic_call(code) {
        return true;
    }
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

/// **The sums outside the pacer that are connection scheduling, by file and exact line.**
///
/// `src/rig.rs`'s keyframe back-off: after a `get_view` page the bridge refused, the seat
/// asks for a keyframe again no sooner than one keep-alive interval from now
/// (`keyframe_not_before_us`), so a page this build can never read is not asked for in a
/// loop. It is a question of *when* to ask, which AGENTS.md section 3 rule 4 gives the rig
/// ("the connection scheduling in `rig.rs`"), and S1's plan's decision 17 (item 128) names
/// it in that list. The allowance is that one line of code, word for word, in that one
/// file, and it appears there once (`the_scheduling_sums_are_one_line_each`): any other
/// line, the same identifier with a different computation on it included, is still an
/// offence.
const SCHEDULING_SUMS: &[(&str, &str)] = &[(
    "rig.rs",
    "self.keyframe_not_before_us = self.timing.now_us().saturating_add(KEEPALIVE_US);",
)];

/// The identifier each of [`SCHEDULING_SUMS`]'s lines assigns, for the one-line check.
const SCHEDULING_IDENTIFIERS: &[&str] = &["keyframe_not_before_us"];

/// Whether `code`, a line of `file` in `src/`, is one of [`SCHEDULING_SUMS`], word for
/// word once comments are stripped.
fn scheduling_sum(file: &str, code: &str) -> bool {
    SCHEDULING_SUMS
        .iter()
        .any(|(owner, line)| *owner == file && code.trim() == *line)
}

/// Whether every quantity part of `word` is a time word.
fn only_time(word: &str) -> bool {
    word.split('_')
        .chain(std::iter::once(word))
        .filter(|part| QUANTITIES.contains(part))
        .all(|part| TIME.contains(&part))
}

/// The fragments of [`QUANTITIES`] that mean time, which [`PACING_MODULE`] may compute
/// with.
const TIME: &[&str] = &[
    "ms",
    "millis",
    "milliseconds",
    "us",
    "micros",
    "microseconds",
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

/// How many leading lines of a Rust file are scanned: all of them, unless the file ends in
/// its unit-test module, in which case every line before that module's `#[cfg(test)]`.
///
/// The module is recognised in exactly one shape, the one rustfmt gives every test module
/// in this crate: a `#[cfg(test)]` line, then `mod tests {`, and that module's closing `}`
/// in column 0 as the file's last non-blank line, with no second `#[cfg(test)]` anywhere.
/// Any other placement (a `#[cfg(test)]` helper, import or module mid-file) skips nothing,
/// so the code after it is still scanned and a stray attribute fails loudly instead of
/// hiding the rest of the file (`every_test_module_is_the_files_last_item` holds `src/` to
/// the shape).
fn scanned_lines(text: &str) -> usize {
    let lines: Vec<&str> = text.lines().collect();
    let all = lines.len();
    let marks: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim() == "#[cfg(test)]")
        .map(|(index, _)| index)
        .collect();
    let [start] = marks.as_slice() else {
        return all;
    };
    let opens = lines
        .get(start.saturating_add(1))
        .is_some_and(|line| *line == "mod tests {");
    let last = lines.iter().rposition(|line| !line.trim().is_empty());
    let closes = lines
        .iter()
        .enumerate()
        .skip(start.saturating_add(2))
        .find(|(_, line)| **line == "}")
        .map(|(index, _)| index);
    if opens && closes.is_some() && closes == last {
        *start
    } else {
        all
    }
}

/// Every offending line, as `path:line: text`.
fn offences(files: &[PathBuf]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for path in files {
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        let file = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let in_src = path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            == Some("src");
        let pacing = in_src && file == PACING_MODULE;
        let rust = path.extension().and_then(|value| value.to_str()) == Some("rs");
        // A unit-test module is a test driver, never compiled into the client (the module
        // doc says why), and is skipped only in its one shape: see `scanned_lines`.
        let scanned = if rust {
            scanned_lines(&text)
        } else {
            text.lines().count()
        };
        for (index, line) in text.lines().enumerate().take(scanned) {
            let code = code_of(line);
            let mut named = quantities_in(&code);
            if named.is_empty() || !has_arithmetic(&code) {
                continue;
            }
            if in_src && scheduling_sum(file, &code) {
                continue;
            }
            if pacing {
                // A word the pacer may compute with is one whose every quantity part is
                // time: `owed_ms` passes, `remaining_kw` does not.
                named.retain(|word| !only_time(word));
            }
            if named.is_empty() {
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
        // Arithmetic spelt as a method (the register's S1-44).
        "let left = remaining_ms.saturating_sub(spent_ms);",
        "let at = self.timing.now_us().saturating_add(KEEPALIVE_US);",
        "let total = costs.iter().copied().sum::<i64>();",
        "let whole = legs.iter().sum();",
        "let dollars = treasury.checked_div(2)?;",
        "let gap = deadline_us.abs_diff(now_us);",
        "let wrapped = draw_kw.wrapping_add(1);",
        "var cell := posmod(eta, 60)",
        "var shown := snapped(travel, 1000)",
        // Spellings the first widening missed (review of `ui`): a method path handed on
        // uncalled, a space before the parenthesis, and a duration's unit conversion.
        "let total_ms = legs_ms.iter().copied().fold(0, i64::saturating_add);",
        "let total_ms = legs_ms.iter().copied().reduce(u64::wrapping_add);",
        "var cell := posmod (eta, 60)",
        "let secs = Duration::from_millis(remaining_ms).as_secs();",
        "let shown = Duration::from_millis(travel_ms);",
        "let left = remaining.as_millis();",
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
        // A method that is not arithmetic, on a line that names a quantity, is not one.
        "let shown = clock_text(owed_ms.min(limit));",
        "self.calls = self.calls.saturating_add(1);",
        "let addr = payload.address(travel_ms);",
        "let sub = remaining.subscriber();",
        "let summary_ms = shown(remaining_ms);",
        "use std::time::Duration; // remaining_ms",
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

/// The scanner reads method calls: a family's prefix before an operation, and the bare
/// operations, and nothing that merely starts with one of their letters.
#[test]
fn the_scanner_reads_an_arithmetic_method_by_its_name() {
    for name in [
        "checked_add",
        "saturating_sub",
        "wrapping_mul",
        "overflowing_neg",
        "saturating_add_signed",
        "checked_div_euclid",
        "abs_diff",
        "rem_euclid",
        "sum",
        "posmod",
    ] {
        assert!(is_arithmetic_method(name), "{name}");
    }
    for name in [
        "address",
        "subscriber",
        "summary",
        "min",
        "max",
        "checked",
        "added",
        "text",
    ] {
        assert!(!is_arithmetic_method(name), "{name}");
    }
}

/// A unit-test module is not scanned, and the line before it still is.
#[test]
fn a_unit_test_module_is_a_driver_and_is_not_scanned() {
    let scratch = std::env::temp_dir().join(format!("pharmakos-nt-{}", std::process::id()));
    let fake = scratch.join("src");
    fs::create_dir_all(&fake).expect("scratch");
    let path = fake.join("view.rs");
    fs::write(
        &path,
        "let shown = remaining_ms - spent_ms;
#[cfg(test)]
mod tests {
    let now = 1 + PACER_PERIOD_US;
}
",
    )
    .expect("scratch file");
    let found = offences(std::slice::from_ref(&path));
    let _ = fs::remove_dir_all(&scratch);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found.iter().all(|line| line.contains(":1:")), "{found:?}");
}

/// A `#[cfg(test)]` anywhere but on the file's trailing test module skips nothing: a
/// helper, an import or a second module mid-file leaves the rest of the file scanned
/// (review of `ui`: a mid-file `#[cfg(test)]` used to end the scan).
#[test]
fn a_stray_test_attribute_hides_nothing() {
    let scratch = std::env::temp_dir().join(format!("pharmakos-ns-{}", std::process::id()));
    let fake = scratch.join("src");
    fs::create_dir_all(&fake).expect("scratch");
    let cases = [
        // A test helper before production code.
        "#[cfg(test)]
fn helper() {}
pub fn shown(remaining_ms: u64) -> u64 { remaining_ms / 1000 }
",
        // A test module that does not run to the end of the file.
        "#[cfg(test)]
mod tests {
}
pub fn shown(remaining_ms: u64) -> u64 { remaining_ms / 1000 }
",
        // Two test attributes, the second on a trailing module.
        "#[cfg(test)]
use std::fmt;
pub fn shown(remaining_ms: u64) -> u64 { remaining_ms / 1000 }
#[cfg(test)]
mod tests {
}
",
    ];
    let mut missed: Vec<&str> = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let path = fake.join(format!("case{index}.rs"));
        fs::write(&path, case).expect("scratch file");
        if offences(std::slice::from_ref(&path)).is_empty() {
            missed.push(case);
        }
    }
    let _ = fs::remove_dir_all(&scratch);
    assert!(missed.is_empty(), "the scan stopped early on {missed:?}");
}

/// Every `#[cfg(test)]` in `src/` is the one shape [`scanned_lines`] skips: the file's
/// only test attribute, on a `mod tests` that runs to the end of the file. A file that
/// breaks the shape is scanned whole, and this names it rather than leave the reason to
/// a confusing offence.
#[test]
fn every_test_module_is_the_files_last_item() {
    let mut wrong: Vec<String> = Vec::new();
    for path in sources() {
        if path.extension().and_then(|value| value.to_str()) != Some("rs") {
            continue;
        }
        let text =
            fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let marked = text.lines().any(|line| line.trim() == "#[cfg(test)]");
        if marked && scanned_lines(&text) == text.lines().count() {
            wrong.push(path.display().to_string());
        }
    }
    assert!(
        wrong.is_empty(),
        "these files carry a #[cfg(test)] that is not their trailing `mod tests`: {wrong:?}"
    );
}

/// **The scheduling sums are one line each**: each allowed line appears exactly once in its
/// file, the identifier it assigns carries arithmetic on no other line, and the allowance
/// is the line word for word: the same identifier with any other computation is an offence.
#[test]
fn the_scheduling_sums_are_one_line_each() {
    for ((owner, allowed), identifier) in SCHEDULING_SUMS.iter().zip(SCHEDULING_IDENTIFIERS) {
        assert!(
            allowed.contains(identifier),
            "{allowed} assigns {identifier}"
        );
        let path = crate_root().join("src").join(owner);
        let text =
            fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let lines: Vec<String> = text
            .lines()
            .take(scanned_lines(&text))
            .map(code_of)
            .filter(|code| code.contains(identifier) && has_arithmetic(code))
            .map(|code| code.trim().to_owned())
            .collect();
        assert_eq!(
            lines,
            vec![(*allowed).to_owned()],
            "src/{owner}'s {identifier} is allowed its one sum and no other"
        );
    }
    assert_eq!(SCHEDULING_SUMS.len(), SCHEDULING_IDENTIFIERS.len());
    let code =
        code_of("self.keyframe_not_before_us = self.timing.now_us().saturating_add(KEEPALIVE_US);");
    assert!(scheduling_sum("rig.rs", &code));
    assert!(!scheduling_sum("editor.rs", &code), "one file wide");
    for other in [
        "self.keyframe_not_before_us = now_us.saturating_add(cost_us);",
        "let shown_secs = (self.keyframe_not_before_us - phase_remaining_ms) / 1000;",
    ] {
        assert!(!scheduling_sum("rig.rs", &code_of(other)), "{other}");
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

/// The clock words an editor script may not use.
const CLOCKS: &[&str] = &["time.", "timer", "get_ticks", "unix_time", "create_tween"];

/// The editor's scripts read no clock: the pacer (`src/pacer.rs`) is the client's one wall
/// clock, and the editor's one timer — FULL after 600 ms idle — is kept there
/// (`pacer::IdleTimer`). A `Timer` node or a `Time.get_ticks_*` in an editor script would
/// be a second clock deciding when to ask the gateway something. Pull request 2's editor
/// scripts are on the list too: the wizard's pages, the rule list and the wizard's
/// hostless scene. (The lobby shows the pacer's countdown and names a match only
/// through `host_link.gd`'s helper, which the next test holds to its one clock read.)
#[test]
fn the_editor_scripts_read_no_clock_of_their_own() {
    let mut found: Vec<String> = Vec::new();
    for name in [
        "editor.gd",
        "rows.gd",
        "strings.gd",
        "wizard.gd",
        "rule_list.gd",
        "wizard_shot.gd",
    ] {
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

/// **The match-id helper reads the clock once, and computes nothing with it** (decisions-log
/// item 113 (12): from T19 pull request 2, "the match-id helper in
/// `godot/scripts/host_link.gd`, which reads the clock once to name a match and computes
/// nothing with it" joins AGENTS.md section 3 rule 4's list).
///
/// In `host_link.gd`, the only line that names a clock is the helper's one `return`, which
/// formats the seconds into the id and carries no arithmetic operator (the format string
/// and its `%` are stripped as prose, so `-3600` or `+ 1` on the clock still trips it); and
/// the scripts that name a match go through the helper. The next test holds every other
/// script to reading no clock at all, so no second clock read can appear anywhere.
#[test]
fn the_match_id_helper_is_the_one_clock_read_and_computes_nothing() {
    let path = godot_root().join("scripts").join("host_link.gd");
    let text =
        fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let lines: Vec<&str> = text.lines().collect();
    let reads: Vec<(usize, &str)> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| {
            let code = code_of(line).to_ascii_lowercase();
            CLOCKS.iter().any(|clock| code.contains(clock))
        })
        .map(|(index, line)| (index, *line))
        .collect();
    assert_eq!(
        reads.len(),
        1,
        "host_link.gd reads a clock in more than one place, or in none: {reads:?}"
    );
    let (index, line) = reads.first().copied().expect("one read");
    let helper = lines
        .get(..index)
        .and_then(|before| {
            before
                .iter()
                .rev()
                .find(|line| line.starts_with("static func "))
        })
        .copied()
        .unwrap_or_default();
    assert!(
        helper.starts_with("static func match_id("),
        "the clock is read outside the match-id helper, in `{helper}`"
    );
    let code = code_of(line);
    assert!(
        code.trim_start().starts_with("return ") && !has_arithmetic(&code),
        "the helper computes with the clock instead of only naming the match with it: {line}"
    );
    for name in ["lobby.gd", "watch_check.gd"] {
        let script = fs::read_to_string(godot_root().join("scripts").join(name)).expect("a script");
        assert!(
            script.contains("HostLink.match_id("),
            "{name} names its match without the helper"
        );
        assert!(
            !script.contains("get_process_id()"),
            "{name} builds a match id of its own"
        );
    }
}

/// The readers of a clock, as they appear in GDScript once lower-cased: `Time.*`, the
/// engine's tick counters, a `Timer` node, a scene-tree timer and a tween.
const CLOCK_READERS: &[&str] = &[
    "time.",
    "get_ticks",
    "unix_time",
    "create_timer",
    "timer.new",
    "create_tween",
];

/// **No script under `godot/` reads a clock, but the named two** (AGENTS.md section 3 rule
/// 4, and decisions-log item 113 (12)): the client's wall clock is the pacer's
/// (`src/pacer.rs`), and a script's only clock read is `host_link.gd`'s match-id helper,
/// which the test above holds to its one line. The other exception is the two checks'
/// `create_timer` waits, `watch_check.gd`'s and the shipped `smoke_check.gd`'s (decisions-log
/// item 117 (7)): a check is a test driver, a wait is how it gives a real host time, and its
/// waits are bounded; neither reads any other clock. Every other `.gd` under `godot/` — the lobby that names
/// and remembers matches included — reads none. (The word `timer` alone is not a reader:
/// the lobby shows the pacer's countdown under that dictionary key.)
#[test]
fn no_script_reads_a_clock_but_the_id_helper_and_the_checks_waits() {
    let mut scripts: Vec<PathBuf> = Vec::new();
    collect(&godot_root(), "gd", &mut scripts);
    scripts.sort();
    assert!(
        scripts.len() >= 10,
        "the scan found the scripts: {scripts:?}"
    );
    let mut found: Vec<String> = Vec::new();
    for path in &scripts {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if name == "host_link.gd" {
            continue; // the_match_id_helper_is_the_one_clock_read_and_computes_nothing
        }
        let text =
            fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        for (index, line) in text.lines().enumerate() {
            let code = code_of(line).to_ascii_lowercase();
            let readers: Vec<&&str> = CLOCK_READERS
                .iter()
                .filter(|reader| code.contains(**reader))
                .collect();
            let allowed = (name == "watch_check.gd" || name == "smoke_check.gd")
                && readers.iter().all(|r| **r == "create_timer");
            if !readers.is_empty() && !allowed {
                found.push(format!("{}:{}: {}", path.display(), index + 1, line.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "a script reads a clock; the pacer (src/pacer.rs) is the client's clock, and \
         host_link.gd's match_id is the one read that names a match:\n{}",
        found.join("\n")
    );
}
