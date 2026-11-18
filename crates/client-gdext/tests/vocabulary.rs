// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! **The client names no word of the vocabulary** (S3's plan, task `ed3b`, part 2; decided by
//! the brief, the owner can veto): every predicate, mandate and blueprint the pickers and the
//! Visit & change rows offer is `get_vocabulary`'s, drawn as sent. This scan reads the names out
//! of the committed answer (`godot/fixtures/vocabulary.json`), so a new name there needs no
//! change here, and fails when the client's code names one: a Rust source under `src/` (its unit
//! tests left out) or a GDScript under `godot/scripts/` (the checks left out, which pin what a
//! real host answers).
//!
//! Code only: a comment may say "a Build beacon". A string literal is code, because a
//! dictionary key or a match arm is how a script or a reader would name a word.
//!
//! A few of the vocabulary's names are ordinary words the client used before the vocabulary
//! existed, for things that are not a vocabulary pick: [`OTHER_MEANINGS`] lists each by file
//! and name, with what it means there. A new use of one of them anywhere else fails.

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

use pharmakos_proto::json::{self, Json};

/// A vocabulary name the client's code says with another meaning: the file, the name, and
/// what it means there.
const OTHER_MEANINGS: &[(&str, &str, &str)] = &[
    (
        "editor.rs",
        "attack",
        "the editor's field holding the Attack targets the S2 targeting reader found",
    ),
    (
        "targeting.rs",
        "attack",
        "S2's `attack_targets`, the lax reader of the Attack beacon's targets (AGENTS.md \
         section 3 rule 4 names it)",
    ),
    ("bridge.rs", "wall", "the bridge's wall clock"),
    (
        "view.rs",
        "build",
        "the view model's own `build` of its chunk grid",
    ),
    (
        "editor.gd",
        "treasury",
        "the meter's frame key for the gateway's `treasury_now`",
    ),
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the workspace root")
        .to_path_buf()
}

/// Every name the committed vocabulary answer offers: predicates, mandates and blueprints.
fn vocabulary_names() -> Vec<String> {
    let text = fs::read_to_string(
        root()
            .join("godot")
            .join("fixtures")
            .join("vocabulary.json"),
    )
    .expect("the fixture");
    let frame = json::read(&text).expect("json");
    let result = frame.get("result").expect("a result");
    let mut names: Vec<String> = Vec::new();
    let mut take = |entries: Option<&Json>| {
        if let Some(Json::Array(entries)) = entries {
            for entry in entries {
                if let Some(Json::String(name)) = entry.get("name")
                    && !name.is_empty()
                    && !names.contains(name)
                {
                    names.push(name.clone());
                }
            }
        }
    };
    if let Some(Json::Array(families)) = result.get("families") {
        for family in families {
            take(family.get("predicates"));
        }
    }
    take(result.get("mandates"));
    take(result.get("blueprints"));
    names
}

/// The client's sources: `src/*.rs` and `godot/scripts/*.gd`, the checks left out.
fn client_sources() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = Vec::new();
    for (dir, extension) in [
        (Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), "rs"),
        (root().join("godot").join("scripts"), "gd"),
    ] {
        for entry in fs::read_dir(&dir).expect("a source folder").flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if path.extension().and_then(|value| value.to_str()) == Some(extension)
                && !name.ends_with("_check.gd")
            {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// `text` with its comments and its prose gone: a Rust file's unit tests and `//` comments, a
/// script's `#` comments, and every string literal that is not one identifier (a sentence of the
/// string table, an error message). A literal that is one identifier is kept as that word,
/// because a dictionary key or a match arm is how code names a word.
fn code_of(text: &str, rust: bool) -> String {
    let text = if rust {
        text.split("#[cfg(test)]").next().unwrap_or("")
    } else {
        text
    };
    let comment = if rust { "//" } else { "#" };
    text.lines()
        .map(|line| code_of_line(line, comment))
        .collect::<Vec<_>>()
        .join("\n")
}

fn code_of_line(line: &str, comment: &str) -> String {
    let mut out = String::new();
    let mut literal = String::new();
    let mut in_string = false;
    let mut characters = line.chars().peekable();
    while let Some(character) = characters.next() {
        if in_string {
            if character == '\\' {
                characters.next();
            } else if character == '"' {
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
                }
            } else {
                literal.push(character);
            }
            continue;
        }
        if character == '"' {
            in_string = true;
            literal.clear();
            continue;
        }
        let starts_comment = match comment {
            "//" => character == '/' && characters.peek() == Some(&'/'),
            _ => character == '#',
        };
        if starts_comment {
            break;
        }
        out.push(character);
    }
    out
}

/// Whether `code` names `needle` whole.
fn names(code: &str, needle: &str) -> bool {
    let identifier = |character: Option<char>| {
        character.is_some_and(|character| character.is_ascii_alphanumeric() || character == '_')
    };
    code.match_indices(needle).any(|(at, _)| {
        let before = code.get(..at).and_then(|head| head.chars().next_back());
        let after = code
            .get(at.saturating_add(needle.len())..)
            .and_then(|tail| tail.chars().next());
        !identifier(before) && !identifier(after)
    })
}

#[test]
fn the_client_names_no_word_of_the_vocabulary() {
    // The scan is not vacuous.
    assert!(names("\"cmdr_hp_pct\": 1", "cmdr_hp_pct"));
    assert!(!names("my_cmdr_hp_pct", "cmdr_hp_pct"));
    assert_eq!(code_of("x = 1 # generator", false).trim(), "x = 1");
    assert!(code_of("x = \"generator\"", false).contains("generator"));
    assert!(!code_of("x = \"a generator here\"", false).contains("generator"));
    let needles = vocabulary_names();
    for wanted in ["cmdr_hp_pct", "step_reached"] {
        assert!(
            needles.iter().any(|needle| needle == wanted),
            "the fixture lists `{wanted}`: {needles:?}"
        );
    }
    assert!(
        needles.len() >= 20,
        "the scan read the predicates, the mandates and the blueprints: {needles:?}"
    );
    let mut found: Vec<String> = Vec::new();
    let mut used_exceptions: Vec<(&str, &str)> = Vec::new();
    for path in client_sources() {
        let text = fs::read_to_string(&path).expect("a source");
        let rust = path.extension().and_then(|value| value.to_str()) == Some("rs");
        let code = code_of(&text, rust);
        let file = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_owned();
        for needle in &needles {
            if !names(&code, needle) {
                continue;
            }
            match OTHER_MEANINGS
                .iter()
                .find(|(listed, name, _)| *listed == file && name == needle)
            {
                Some((listed, name, _)) => used_exceptions.push((listed, name)),
                None => found.push(format!("{file} names `{needle}`")),
            }
        }
    }
    assert!(
        found.is_empty(),
        "every predicate, mandate and blueprint is get_vocabulary's, drawn as sent: the client \
         names none of them.\n{}",
        found.join("\n")
    );
    for (file, name, _) in OTHER_MEANINGS {
        assert!(
            used_exceptions.contains(&(file, name)),
            "OTHER_MEANINGS lists `{name}` in {file}, which no longer says it: drop the line"
        );
    }
}
