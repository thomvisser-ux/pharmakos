// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The type door has no back way in (S3's `vfyq2`, fix pass 4, item L-c, and
//! fix pass 5, item L4).
//!
//! What the **type system** holds: a `Said` is built only from a
//! `&'static str`, a number, a `char`, a `Quoted` (escaped and capped), an
//! `EngineWords` type's `Display` (escaped and capped) or another `Said`; it
//! has no `From<String>` and its field is private; `Quoted`'s field is
//! private; the three refusal types have no public field. So a raw `String`
//! does not compile where a person is shown text.
//!
//! What **these scans** hold, because the language cannot:
//!
//! * a `String` made `&'static str`: no identifier `leak`, `into_raw` or
//!   `forget` in a crate behind the door (proto, plan-core, verifier,
//!   gateway), and every `static` there is on a reviewed list, so no text is
//!   kept for ever and handed out as `'static`;
//! * `EngineWords` (and its seal, `door::Sealed`) is implemented only in a
//!   crate behind the door's `src`, never in a test, a bench or another
//!   crate: a trait a downstream crate can name cannot be sealed by the type
//!   system across crates;
//! * `Said::compose`, which takes any `&'static str` as a template, is
//!   called by `said!` alone: a macro must call a public function, so it
//!   cannot be private.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml's allow-expect-in-tests only recognises #[test] functions, not an \
              integration test's helpers. A panic is this file's failure report."
)]

use std::path::{Path, PathBuf};

/// Every `.rs` file under `dir`, sorted.
#[allow(
    clippy::disallowed_methods,
    reason = "clippy.toml's own reason string sanctions read_dir when the listing is collected \
              and sorted before use, which is what happens below"
)]
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("a source folder")
        .map(|entry| entry.expect("an entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

/// The three refusals a person is shown have no public field (S3's `vfyq2`,
/// fix pass 4, item L-b, and 4b): `json::Error`, plan-core's `Error` and the
/// gateway's `Error`. A public field is a writable one, so a caller holding
/// an error could put raw text in its words, or move its pointer, after the
/// constructor that took a `Said`; with every field private, only the
/// owning crate's constructors make one, and a caller reads through getters
/// (`message()`, `said()`, `pointer()`, and the codes). The repository has
/// no compile-fail harness, so the reviewer's bypass probe (`error.message =
/// raw`) is pinned here by the declaration it needs.
#[test]
fn the_refusals_have_no_public_field() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crates folder")
        .to_path_buf();
    for file in [
        "proto/src/json/value.rs",
        "plan-core/src/error.rs",
        "gateway/src/error.rs",
    ] {
        let text = std::fs::read_to_string(crates.join(file)).expect("a source file");
        let start = text.find("pub struct Error {").expect("the error type");
        let body = text
            .get(start..)
            .and_then(|rest| rest.find("\n}").and_then(|end| rest.get(..end)))
            .expect("the struct's body");
        let public: Vec<&str> = body
            .lines()
            .skip(1)
            .map(str::trim)
            .filter(|line| line.starts_with("pub ") || line.starts_with("pub("))
            .filter(|line| !line.starts_with("pub(crate)") && !line.starts_with("pub(super)"))
            .collect();
        assert!(
            public.is_empty(),
            "{file}: a public field on a refusal: {public:?}"
        );
        assert!(
            text.contains("pub fn message(&self) -> &str"),
            "{file}: no message() getter"
        );
    }
}

/// The crates behind the door.
const BEHIND_THE_DOOR: [&str; 4] = ["proto", "plan-core", "verifier", "gateway"];

/// Identifiers that make a `&'static` borrow out of an owner, or keep one
/// alive by forgetting it.
const LEAK_IDENTIFIERS: [&str; 3] = ["leak", "into_raw", "forget"];

/// Every `static` the crates behind the door declare, each reviewed: what it
/// holds and why no player text reaches a `Said` through it. A new one fails
/// the scan until it is reviewed and listed.
const REVIEWED_STATICS: [(&str, &str); 6] = [
    // The compiled-in descriptor set, parsed once: the schema's own names.
    ("proto/src/descriptor.rs", "SCHEMA"),
    // The field index built from that schema: the schema's own names.
    ("proto/src/json/index.rs", "INDEX"),
    // A count of characters examined: a number.
    ("proto/src/json/work.rs", "EXAMINED"),
    // A test's committed rules table (a `#[cfg(test)]` module).
    ("plan-core/src/verify/differential.rs", "RULES"),
    // Tests' fixtures (`#[cfg(test)]` modules).
    ("gateway/src/surface.rs", "REMEMBERED"),
    ("gateway/src/surface.rs", "OPENING"),
];

/// The code of a line: its string literals' contents and its `//` comment
/// dropped, so words in a message or a comment are not taken for code.
fn code(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut previous = ' ';
    for character in line.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
                out.push('"');
            }
        } else if character == '"' {
            in_string = true;
            out.push('"');
        } else if character == '/' && previous == '/' {
            out.pop();
            break;
        } else {
            out.push(character);
        }
        previous = character;
    }
    out
}

/// The identifiers on a line of code, in order.
fn identifiers(code: &str) -> Vec<&str> {
    code.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty())
        .collect()
}

/// The crates folder.
fn crates() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crates folder")
        .to_path_buf()
}

/// Every `.rs` file of the crates behind the door's `src`.
fn behind_the_door() -> Vec<PathBuf> {
    let crates = crates();
    let mut files = Vec::new();
    for name in BEHIND_THE_DOOR {
        sources(&crates.join(name).join("src"), &mut files);
    }
    assert!(
        files.len() > 40,
        "the scan sees the sources: {}",
        files.len()
    );
    files
}

/// Every `.rs` file of every crate and of `xtask`, `src`, tests and benches
/// alike.
fn everywhere() -> Vec<PathBuf> {
    let crates = crates();
    let mut files = Vec::new();
    sources(&crates, &mut files);
    if let Some(root) = crates.parent() {
        sources(&root.join("xtask"), &mut files);
    }
    files.retain(|file| !file.components().any(|part| part.as_os_str() == "target"));
    files
}

/// The path of `file` under the crates folder, with `/` separators.
fn relative(file: &Path) -> String {
    // A file the walk found under the crates folder; one that is not shows
    // its whole path, which names it as well.
    let shown = file.strip_prefix(crates()).unwrap_or(file);
    shown.to_string_lossy().replace('\\', "/")
}

#[test]
fn no_crate_behind_the_door_makes_a_static_str_from_a_string() {
    let mut found = Vec::new();
    for file in behind_the_door() {
        let text = std::fs::read_to_string(&file).expect("a source file");
        for (number, line) in text.lines().enumerate() {
            let code = code(line);
            let words = identifiers(&code);
            for leak in LEAK_IDENTIFIERS {
                if words.contains(&leak) {
                    found.push(format!(
                        "{}:{}: `{leak}`: {}",
                        relative(&file),
                        number + 1,
                        line.trim()
                    ));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "a leak behind the door:\n{}",
        found.join("\n")
    );
}

/// No text is kept for ever behind the door: every `static` (and every
/// `thread_local!`) is on [`REVIEWED_STATICS`] (fix pass 5, item L4).
#[test]
fn every_static_behind_the_door_is_reviewed() {
    let mut found = Vec::new();
    let mut seen = Vec::new();
    for file in behind_the_door() {
        let path = relative(&file);
        let text = std::fs::read_to_string(&file).expect("a source file");
        for (number, line) in text.lines().enumerate() {
            let code = code(line);
            let words = identifiers(&code);
            let Some(at) = words.iter().position(|word| *word == "static") else {
                continue;
            };
            // `'static` is a lifetime, not an item.
            let item = code
                .split_whitespace()
                .any(|token| token == "static" || token.ends_with("(static"));
            if !item {
                continue;
            }
            let name = words
                .get(at.saturating_add(1))
                .filter(|word| **word != "mut")
                .copied()
                .unwrap_or("mut");
            seen.push((path.clone(), name.to_owned()));
            if !REVIEWED_STATICS.contains(&(path.as_str(), name)) {
                found.push(format!("{path}:{}: {}", number + 1, line.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "a static behind the door no review has read:\n{}",
        found.join("\n")
    );
    for (file, name) in REVIEWED_STATICS {
        assert!(
            seen.iter().any(|(at, named)| at == file && named == name),
            "{file}: `{name}` is listed and no longer declared"
        );
    }
}

/// `EngineWords` and its seal are implemented only behind the door, in `src`
/// (fix pass 5, item L4): an implementation in a test, a bench, the client,
/// the operator, `gamectl` or `xtask` is refused.
#[test]
fn engine_words_are_declared_only_behind_the_door() {
    let mut found = Vec::new();
    let mut declared = 0_u32;
    for file in everywhere() {
        let path = relative(&file);
        let behind = BEHIND_THE_DOOR
            .iter()
            .any(|name| path.starts_with(&format!("{name}/src/")));
        let text = std::fs::read_to_string(&file).expect("a source file");
        let lines: Vec<&str> = text.lines().collect();
        for (number, line) in lines.iter().enumerate() {
            // An `impl` may break its line before `for`.
            let joined = format!(
                "{} {}",
                code(line),
                lines
                    .get(number.saturating_add(1))
                    .map_or_else(String::new, |next| code(next))
            );
            let words = identifiers(&joined);
            let implements = words.first() == Some(&"impl")
                && words.windows(2).any(|pair| {
                    matches!(pair, [trait_name, "for"] if *trait_name == "EngineWords" || *trait_name == "Sealed")
                });
            if !implements {
                continue;
            }
            declared = declared.saturating_add(1);
            if !behind {
                found.push(format!("{path}:{}: {}", number + 1, line.trim()));
            }
        }
    }
    assert!(declared > 20, "the scan sees the declarations: {declared}");
    assert!(
        found.is_empty(),
        "EngineWords declared outside the door:\n{}",
        found.join("\n")
    );
}

/// `Said::compose` is called by `said!` alone (fix pass 5, item L4).
#[test]
fn said_compose_is_called_by_the_macro_alone() {
    let mut found = Vec::new();
    for file in everywhere() {
        let path = relative(&file);
        let text = std::fs::read_to_string(&file).expect("a source file");
        for (number, line) in text.lines().enumerate() {
            let code = code(line);
            let calls = code.contains("Said::compose(") || code.contains("Said::compose_checked(");
            let the_door = path == "proto/src/json/grammar.rs";
            if calls && !the_door {
                found.push(format!("{path}:{}: {}", number + 1, line.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "a template composed outside said!:\n{}",
        found.join("\n")
    );
}
