// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The type door has no back way in (S3's `vfyq2`, fix pass 4, item L-c).
//!
//! A `Said` is built from a `&'static str`, a number, a `char`, a `Quoted` or
//! another `Said`, so a raw `String` does not compile where a person is
//! shown text. A `String` made `&'static str` would walk through the literal
//! door: `Box::leak`, `String::leak`, `Box::into_raw`, `mem::forget` of an
//! owner whose borrow is kept, or `Vec::leak`. No source of a crate behind the
//! door (proto, plan-core, verifier, gateway) names any of them.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml's allow-expect-in-tests only recognises #[test] functions, not an \
              integration test's helpers. A panic is this file's failure report."
)]

use std::path::{Path, PathBuf};

/// What makes a `&'static` borrow out of an owner.
const LEAKS: [&str; 6] = [
    "Box::leak",
    "String::leak",
    "Vec::leak",
    ".leak()",
    "Box::into_raw",
    "mem::forget",
];

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

#[test]
fn no_crate_behind_the_door_makes_a_static_str_from_a_string() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crates folder")
        .to_path_buf();
    let mut files = Vec::new();
    for name in ["proto", "plan-core", "verifier", "gateway"] {
        sources(&crates.join(name).join("src"), &mut files);
    }
    assert!(
        files.len() > 40,
        "the scan sees the sources: {}",
        files.len()
    );
    let mut found = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).expect("a source file");
        for (number, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            for leak in LEAKS {
                if code.contains(leak) {
                    found.push(format!(
                        "{}:{}: {}",
                        file.display(),
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
