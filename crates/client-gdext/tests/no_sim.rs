// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! **No dependency edge on `pharmakos-sim`** — skeleton plan T12's acceptance, in code.
//!
//! The crate map grants this crate gdext, `pharmakos-proto` and the mesher's public
//! surface, "and nothing else". The sim is the crate that owns hashed state; this one is
//! walled, so floats, `as` casts, hash maps and clocks are legal inside it. An edge
//! between them in either direction is the thing the wall exists to prevent, and
//! `cargo xtask wall-guard` checks the direction that matters most — that no
//! deterministic crate depends on a walled one. It does **not** check this direction,
//! because a walled crate depending on the sim would not be a lint failure anywhere: it
//! would be an ordinary Cargo edit that quietly put the determinism crate's stepping API
//! within the bridge's reach.
//!
//! So this file checks it, and it checks the **transitive** closure rather than the
//! manifest alone — a direct edge is the easy case, and an edge arriving through a third
//! crate is the one that would go unnoticed. The closure is read out of the committed
//! `Cargo.lock`, which is the resolved graph CI builds from, with no subprocess and no
//! second resolver.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules — \
              not an integration test's helper functions. A panic is this file's failure \
              report (clippy.toml's own wording)."
)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// The crate this file is about.
const SELF: &str = "pharmakos-client-gdext";

/// Names that must not appear anywhere in this crate's dependency closure.
///
/// `pharmakos-sim` is the crate map's rule and T12's acceptance. The other three are the
/// crates the map does not grant this one either — an edge to any of them would mean the
/// thin client had started reaching for a rule, a verdict or a socket.
const FORBIDDEN: &[&str] = &[
    "pharmakos-sim",
    "pharmakos-verifier",
    "pharmakos-plan-core",
    "pharmakos-gateway",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("the workspace root is two levels above this crate")
}

/// `name -> its direct dependency names`, read out of `Cargo.lock`.
///
/// A hand-rolled reader rather than a TOML dependency: the file's shape is fixed and
/// simple, and a test that guards a dependency rule should not add one to do it.
fn lock_graph(text: &str) -> BTreeMap<String, Vec<String>> {
    let mut graph: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut name: Option<String> = None;
    let mut dependencies: Vec<String> = Vec::new();
    let mut in_dependencies = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "[[package]]" {
            if let Some(package) = name.take() {
                graph.insert(package, std::mem::take(&mut dependencies));
            }
            dependencies.clear();
            in_dependencies = false;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("name = ") {
            name = Some(rest.trim_matches('"').to_owned());
            in_dependencies = false;
            continue;
        }
        if trimmed == "dependencies = [" {
            in_dependencies = true;
            continue;
        }
        if in_dependencies {
            if trimmed == "]" {
                in_dependencies = false;
                continue;
            }
            // Entries are `"name",` or `"name version",` or `"name version (source)",`.
            let entry = trimmed.trim_end_matches(',').trim_matches('"');
            if let Some(first) = entry.split_whitespace().next() {
                dependencies.push(first.to_owned());
            }
        }
    }
    if let Some(package) = name {
        graph.insert(package, dependencies);
    }
    graph
}

/// Every crate reachable from `root`, including `root` itself.
fn closure(graph: &BTreeMap<String, Vec<String>>, root: &str) -> BTreeSet<String> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut queue: Vec<String> = vec![root.to_owned()];
    while let Some(name) = queue.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        if let Some(edges) = graph.get(&name) {
            for edge in edges {
                queue.push(edge.clone());
            }
        }
    }
    seen
}

#[test]
fn the_resolved_graph_never_reaches_the_sim() {
    let lock_path = workspace_root().join("Cargo.lock");
    let text = fs::read_to_string(&lock_path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", lock_path.display()));
    let graph = lock_graph(&text);
    assert!(
        graph.contains_key(SELF),
        "Cargo.lock does not list `{SELF}`; the reader above has stopped matching the file"
    );

    let reachable = closure(&graph, SELF);
    // Sanity: the closure is real. If the reader broke, `reachable` would be one name and
    // the assertion below would pass for the wrong reason.
    assert!(
        reachable.contains("pharmakos-mesher") && reachable.contains("godot"),
        "the closure of `{SELF}` is missing crates it certainly has: {reachable:?}"
    );

    for name in FORBIDDEN {
        assert!(
            !reachable.contains(*name),
            "`{SELF}` reaches `{name}` in the resolved graph. The crate map grants this crate \
             gdext, pharmakos-proto and the mesher's public surface and nothing else (AGENTS.md \
             section 3; skeleton plan T12). If the dependency is genuinely needed, that is a \
             contract change: open a pull request and stop."
        );
    }
}

#[test]
fn the_manifest_names_no_forbidden_crate_directly() {
    // The transitive check above subsumes this one, but it reads `Cargo.lock`; this one
    // reads the manifest, so a direct edge is caught even in a tree whose lock file is
    // stale for some other reason.
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let text = fs::read_to_string(&manifest)
        .unwrap_or_else(|error| panic!("reading {}: {error}", manifest.display()));

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
                "crates/client-gdext/Cargo.toml names `{name}` in `{code}`"
            );
        }
    }
}

#[test]
fn the_lock_reader_understands_the_files_shape() {
    // A negative control for the reader itself: a graph it parses wrongly would make the
    // assertions above pass over a real edge.
    let sample = "\
[[package]]\n\
name = \"a\"\n\
version = \"0.1.0\"\n\
dependencies = [\n\
 \"b\",\n\
 \"c 1.0.0\",\n\
]\n\
\n\
[[package]]\n\
name = \"b\"\n\
version = \"0.1.0\"\n\
dependencies = [\n\
 \"forbidden\",\n\
]\n\
\n\
[[package]]\n\
name = \"forbidden\"\n\
version = \"0.1.0\"\n";
    let graph = lock_graph(sample);
    assert_eq!(graph.get("a").map(Vec::len), Some(2));
    let reachable = closure(&graph, "a");
    assert!(
        reachable.contains("forbidden"),
        "the reader must follow an edge two hops out, which is the case this file exists \
         for: {reachable:?}"
    );
}
