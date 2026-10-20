// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! **No silent fallback is left in the bridge** (S2's plan, E7, task `view`: each
//! crate's next S2 owner sweeps its own).
//!
//! The global rule: every predictable failure returns a typed error, and no value
//! falls back to a default -- saturated, clamped, swallowed, a magic value or a
//! default on error -- on a reachable path. This is the gateway's scan
//! (`crates/gateway/tests/confinement.rs`, `no_silent_fallback_is_left`) run over
//! this crate's `src/*.rs`: the same needles, the same `let Ok(..) else` form, and
//! the same reading of shipped code only (everything before a file's first
//! `#[cfg(test)]`, comment-only lines dropped). What is left is a default that
//! **is** the answer by a stated rule, listed in [`KEPT_FALLBACKS`] with that rule.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules — \
              not an integration test's helper functions. A panic is this file's failure \
              report (clippy.toml's own wording)."
)]

use std::path::PathBuf;

/// Every fallback call left in the crate's shipped code, each with the rule
/// that makes its default the exact answer rather than a guess: `(file name, a
/// unique substring of the trimmed line, why)`.
///
/// The sweep found 113 lines. Each that hid a failure now returns a typed
/// [`BridgeError`](pharmakos_client_gdext::error::BridgeError) or is written so the
/// failure cannot happen (a widening proved at compile time, a non-zero unit, a
/// table read whole); the caught-panic defaults, the counts handed to Godot's
/// `int` and the texts GDScript reads `""` for none each go through one helper,
/// so each rule is written once. What is left is a default that **is** the
/// answer, and says so here.
const KEPT_FALLBACKS: &[(&str, &str, &str)] = &[
    (
        "panics.rs",
        "self.guard(what, body).unwrap_or(caught)",
        "a caught panic is counted and named by the guard, reported by `caught_panics` and `panic_report`, and fails CI on a non-zero count; `caught` is the #[func]'s documented answer for a call that did not complete",
    ),
    (
        "variant.rs",
        "count.try_into().unwrap_or(i64::MAX)",
        "Godot's `int` is an i64 and every count handed to it steps from zero by a bounded amount per call or frame, so none reaches i64::MAX in a process's life; at the bound it is shown as the largest int, a presentation rule for counts nothing decides on (`godot_int`)",
    ),
    (
        "variant.rs",
        "text.unwrap_or_default()",
        "GDScript reads `\"\"` as none for every text key and byte array the bridge documents so; a refusal, an error, a file name or a sentence is never empty, and an empty playbook is the same bytes either way (`text_or_empty`)",
    ),
    (
        "bridge.rs",
        "self.apply_view(&result).unwrap_or_default()",
        "an empty dictionary is `view_apply`'s documented refusal: `apply_view` logged the reason and kept the model as it was",
    ),
    (
        "bridge.rs",
        "&applied.unwrap_or_default().to_variant()",
        "a refused page is `view`'s empty dictionary, as in `view_apply`: the reason is logged and the rig is told (`view_refused`), which counts it and asks for a keyframe again",
    ),
    (
        "bridge.rs",
        "self.view.as_ref().map_or(0, ViewModel::pending)",
        "before `configure` there is no view model, so no chunk waits to be drawn",
    ),
    (
        "view.rs",
        "self.map.as_ref().map_or(0, |map| map.queue.len())",
        "before a keyframe builds the map there is no drain queue, so no chunk waits on it",
    ),
    (
        "view.rs",
        "let top = i32::try_from(top).unwrap_or(i32::MAX);",
        "every chunk origin is an i32, so no voxel the client holds lies above i32::MAX and a search from there misses nothing",
    ),
    (
        "view.rs",
        "sizes.get(chunk_slot(chunk)).copied().unwrap_or(0)",
        "the drain queue holds only this grid's chunks and `build` gives each a size slot starting at 0 bytes, the cost of a surface not yet uploaded; the mesher's drain callback answers bytes, not an error",
    ),
    (
        "editor_view.rs",
        "editor.notes_saved().map_or(-1, i64::from)",
        "a stored character count is never negative, so -1 is `notes_saved`'s \"not saved yet\", the default `editor.gd` reads the key with",
    ),
    (
        "editor_view.rs",
        "let (kind, x, y) = name.map_or((\"\", \"\", \"\")",
        "a feature name this build cannot spell has no parts, and its empty `kind` is what makes the panel show the raw id (`name_into`'s doc)",
    ),
    (
        "editor_view.rs",
        "&value.map_or(0, i64::from).to_variant()",
        "an absent optional figure is said by `has_<key>` false beside it, never as 0 (item 135 (2) (g)); `editor.gd` reads the value only when `has_<key>` is true",
    ),
    (
        "enums.rs",
        "scope::from_wire_name(type_name, wire).unwrap_or_else(|| wire.clone())",
        "a value the schema does not declare is passed on exactly as it arrived, for the codec to refuse with a pointer (the module doc): the refusal is the codec's, not a default",
    ),
    (
        "pacer.rs",
        "i32::try_from(ms).unwrap_or(i32::MAX)",
        "`report_host_clock`'s fields are int32: a phase longer than i32::MAX ms (24.8 days) is reported at that bound, a step of zero that moves the gateway's clock no further rather than backwards (`wire_ms`)",
    ),
    (
        "pacer.rs",
        "self.origin.elapsed().as_micros()).unwrap_or(u64::MAX)",
        "the one wall clock reads microseconds in a u64, which overflows 584 000 years after the client started; it stays monotone at the bound",
    ),
    (
        "rig.rs",
        ".map_or(self.phase, |status| Phase::of(status.phase));",
        "a meter answer with no footer is labelled with the phase the rig is in, as the recap's round is; a footer this build cannot read is refused",
    ),
    (
        "rig.rs",
        ".map_or(self.round, |status| status.round);",
        "a recap answer with no footer is the rig's round (`take_recap`'s doc); a footer this build cannot read is refused",
    ),
    (
        "surface.rs",
        "channel.clamp(0.0, 1.0) * 255.0",
        "the probe models the engine's float-to-unorm8 colour conversion, which clamps a channel to [0, 1]: the clamp is the conversion being modelled",
    ),
    (
        "surface.rs",
        ".map_or(\"none matched\", ColourQuantisation::name);",
        "the log line's words for a probe that matched no conversion",
    ),
    (
        "surface.rs",
        "stride.map_or_else(|| \"?\".to_owned()",
        "the log line spells a stride the probe did not learn `?`",
    ),
    (
        "targeting.rs",
        "let after = pick.map_or(0, |index| index.saturating_add(1));",
        "a pick the candidates do not list (a named pick the origin cannot reach, `ResolvedRef`'s doc) has no candidate ranked before it, so every candidate comes after it",
    ),
    (
        "targeting.rs",
        ".map_or(ChipForm::Unknown, form_of);",
        "the lax read of the player's own text: a reference this walk cannot find is `Unknown`, which decides nothing; the verifier has the last word on the file",
    ),
    (
        "targeting.rs",
        "token.parse::<usize>().ok()?.checked_add(1)",
        "RFC 6901: a route token that is not a decimal index names no route step",
    ),
    (
        "targeting.rs",
        "items.get(token.parse::<usize>().ok()?)?",
        "RFC 6901: a token that is not a decimal index names no array element",
    ),
];

/// The needles a fallback is written with, as the gateway's scan names them:
/// `unwrap_or` (and so `unwrap_or_default` and `unwrap_or_else`), `map_or`,
/// `map_or_else`, an error read as nothing (`.ok()?`, `.ok().and_then(`) and a
/// value pushed into range (`.clamp(`, `.max(0)`).
const FALLBACK_NEEDLES: &[&str] = &[
    "unwrap_or",
    "map_or(",
    "map_or_else(",
    ".ok()?",
    ".ok().and_then(",
    ".clamp(",
    ".max(0)",
];

/// This crate's `src/*.rs`, read from the directory and sorted.
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

/// The file's shipped code: comment-only lines removed, and everything from the
/// first `#[cfg(test)]` onward removed, because a unit test may panic and is not
/// compiled into the game.
fn code_lines(text: &str) -> Vec<String> {
    let shipped = match text.find("#[cfg(test)]") {
        Some(index) => text
            .get(..index)
            .expect("a match's index is a char boundary"),
        None => text,
    };
    shipped
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .map(str::to_owned)
        .collect()
}

/// The fallback lines of one file's shipped code, trimmed: every line holding
/// a needle, and every `let Ok(..) .. else {` whose next line returns anything
/// but an `Err` (that line is the one reported).
fn fallback_lines(text: &str) -> Vec<String> {
    let lines = code_lines(text);
    let mut found: Vec<String> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if FALLBACK_NEEDLES.iter().any(|needle| line.contains(needle)) {
            found.push(line.trim().to_owned());
            continue;
        }
        if line.contains("let Ok(")
            && line.trim_end().ends_with("else {")
            && let Some(next) = lines.get(index.saturating_add(1))
        {
            let body = next.trim();
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
            .expect("a source file has a name")
            .to_string_lossy()
            .into_owned();
        for line in fallback_lines(&text) {
            found.push((name.clone(), line));
        }
    }
    found
}

/// The scan's positive control: planted text holding one of each form is
/// found, a typed `let Ok(..) else { return Err(..) }` and comment prose are
/// not, and nothing after `#[cfg(test)]` is read.
#[test]
fn the_fallback_scan_sees_every_form_it_names() {
    let planted = "fn a() {\n    let x = y.unwrap_or(0);\n    let x = y.map_or(0, f);\n    \
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

/// No value in the crate's shipped code falls back to a default instead of
/// saying what went wrong, except the defaults [`KEPT_FALLBACKS`] lists with
/// the rule that makes each one exact. A new fallback fails here until it is a
/// typed error or takes its line and its reason in the list.
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
         the answer exact, add the line to KEPT_FALLBACKS with that rule ({} lines):\n{}",
        unexplained.len(),
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

/// The scan must see the kept fallbacks, or every assertion above passes over
/// nothing.
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
    assert!(
        found.iter().any(|(file, _)| file == "panics.rs"),
        "the fallback scan no longer sees panics.rs' kept `guard_or`"
    );
}
