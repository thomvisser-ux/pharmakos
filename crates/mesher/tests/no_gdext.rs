// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The headless CPU proxy's one structural requirement: **this crate links without gdext**.
//!
//! Decisions log section 2.7 item 56 rejected putting the mesher inside `client-gdext`
//! precisely because "nothing headless can reuse it without linking gdext", and skeleton
//! plan T4's acceptance is a geometry golden "compared across Windows and Linux" with no
//! GPU. `tests/geometry.rs` is that proxy; this file is what keeps it honest.
//!
//! `cargo xtask wall-guard` checks the other direction — that no deterministic crate
//! depends on this one. Nothing in the toolchain checks this one, because a `godot`
//! dependency here would be a perfectly ordinary Cargo edit that broke a property nobody
//! was asserting.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules — \
              not an integration test's helper functions. A panic is this file's failure \
              report (clippy.toml's own wording)."
)]

use std::fs;
use std::path::Path;

/// Crate names that would put an engine, a script runtime or the sim between CI and the
/// geometry it checks.
///
/// `godot` and `gdext` are item 56's rule. `pharmakos-sim` is decision 17's and item 92's:
/// the mesher owns its own [`ChunkView`](pharmakos_mesher::ChunkView) and the sim owns its
/// own chunk store, and `client-gdext` transposes between them — a dependency either way
/// would be the edge `wall-guard` exists to reason about.
const FORBIDDEN: &[&str] = &["godot", "gdext", "pharmakos-sim", "pharmakos_sim"];

#[test]
fn the_manifest_names_no_engine_and_no_sim() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let text = fs::read_to_string(&manifest)
        .unwrap_or_else(|error| panic!("reading {}: {error}", manifest.display()));

    // Dependency sections only: `[package] description` says "links without gdext" on
    // purpose, and a scan that read the prose would fail on the sentence stating the rule.
    let mut in_dependencies = false;
    for line in text.lines() {
        let code = line.split('#').next().unwrap_or("").trim();
        if code.starts_with('[') {
            in_dependencies = code.contains("dependencies");
            continue;
        }
        if code.is_empty() || !in_dependencies {
            continue;
        }
        for name in FORBIDDEN {
            assert!(
                !code.contains(name),
                "crates/mesher/Cargo.toml names `{name}` in `{code}`. This crate links \
                 without gdext and without the sim, which is what lets the headless CPU \
                 proxy and the CI geometry check reuse the real mesher (decisions log \
                 section 2.7 items 56 and 92). If the dependency is genuinely needed, that \
                 is a contract change: open a pull request and stop."
            );
        }
    }
}

#[test]
fn the_shipped_crate_still_has_no_dependencies_at_all() {
    // The `[dependencies]` section is empty on purpose, and the geometry golden's hash
    // function is a dev dependency so that it stays that way. Anything that appears here
    // ships in the client.
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let text = fs::read_to_string(&manifest)
        .unwrap_or_else(|error| panic!("reading {}: {error}", manifest.display()));

    let mut in_dependencies = false;
    let mut entries: Vec<&str> = Vec::new();
    for line in text.lines() {
        let code = line.split('#').next().unwrap_or("").trim();
        if code.starts_with('[') {
            in_dependencies = code == "[dependencies]";
            continue;
        }
        if in_dependencies && !code.is_empty() {
            entries.push(code);
        }
    }
    assert!(
        entries.is_empty(),
        "crates/mesher's [dependencies] is meant to be empty; found {entries:?}"
    );
}
