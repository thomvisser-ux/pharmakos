// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! **`the_full_map_unlock_comes_from_a_server_side_policy_change`** — the client's half.
//!
//! Spec section 3: the full map unlocks on elimination or at match end. Decisions-log item
//! 107 (5) makes that nothing but the viewer becoming unfogged ON THE SERVER: the next
//! `get_view` polls deliver every modified chunk's true bytes and every entity, under the
//! same token. T16a's test of the same name proves the server half. The client half is an
//! absence — no fog logic, no reveal toggle, nothing that could show more than the gateway
//! sent — and an absence is checked on the source text
//! (`docs/design/skeleton-plan-t16a-notes.md` section A (4)).
//!
//! Every GDScript file under `godot/` and every Rust file of this crate is read with its
//! comments removed, and none may name fog or a reveal in code or in a string. The
//! comments are allowed to — several of them say that this client has none.

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

/// Words that would be the beginning of a client-side visibility rule.
const NEEDLES: &[&str] = &["fog", "reveal", "unlock", "spectat"];

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every file with `extension` under `dir`, sorted. `fs::read_dir` is disallowed for its
/// unspecified order; sorting is the remedy the ban asks for, and this crate is walled
/// (AGENTS.md section 4.9).
fn files(dir: &Path, extension: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|name| name.to_str()) != Some(".godot") {
                files(&path, extension, out);
            }
        } else if path.extension().and_then(|value| value.to_str()) == Some(extension) {
            out.push(path);
        }
    }
    out.sort();
}

/// A line with its comment removed: `#` for GDScript, `//` for Rust. String literals are
/// kept, because a string naming a reveal is a reveal waiting for a caller.
fn without_comment(line: &str, gdscript: bool) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_string = false;
    let mut characters = line.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '"' {
            in_string = !in_string;
        }
        if !in_string {
            if gdscript && character == '#' {
                break;
            }
            if !gdscript && character == '/' && characters.peek() == Some(&'/') {
                break;
            }
        }
        out.push(character);
    }
    out
}

fn offences(paths: &[PathBuf], gdscript: bool) -> Vec<String> {
    let mut found = Vec::new();
    for path in paths {
        let text = fs::read_to_string(path).expect("a source file");
        for (number, line) in text.lines().enumerate() {
            let code = without_comment(line, gdscript).to_ascii_lowercase();
            for needle in NEEDLES {
                if code.contains(needle) {
                    found.push(format!(
                        "{}:{}: {}",
                        path.display(),
                        number + 1,
                        line.trim()
                    ));
                }
            }
        }
    }
    found
}

#[test]
fn the_full_map_unlock_comes_from_a_server_side_policy_change() {
    let mut scripts = Vec::new();
    files(
        &crate_root().join("..").join("..").join("godot"),
        "gd",
        &mut scripts,
    );
    assert!(
        scripts.len() >= 8,
        "the scan found only {} scripts under godot/",
        scripts.len()
    );
    let mut rust = Vec::new();
    files(&crate_root().join("src"), "rs", &mut rust);
    assert!(
        rust.len() >= 10,
        "the scan found only {} source files",
        rust.len()
    );

    let mut found = offences(&scripts, true);
    found.extend(offences(&rust, false));
    assert!(
        found.is_empty(),
        "the client names fog or a reveal in code. It holds no fog logic and no reveal \
         toggle: what a seat sees is the gateway's decision, and the full-map unlock is the \
         gateway's policy change arriving as ordinary view pages (decisions-log item 107 \
         (5)):\n{}",
        found.join("\n")
    );
}

#[test]
fn the_scan_would_catch_a_reveal_toggle() {
    let toggle = without_comment("\tif reveal_all: vista.show_everything()", true);
    assert!(NEEDLES.iter().any(|needle| toggle.contains(needle)));
    let commented = without_comment("# the client holds no fog logic", true);
    assert!(!NEEDLES.iter().any(|needle| commented.contains(needle)));
    let rust = without_comment("let unfog = true; // not fog", false);
    assert!(rust.contains("fog") && !rust.contains("// not"));
}
