// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What this crate may not be, asserted from outside it.
//!
//! AGENTS.md section 3 rule 3: the operator is not privileged. Decisions-log
//! item 111 (decision C4) makes that a **dependency fact** -- the crate depends
//! on `pharmakos-proto` alone, so no sim or gateway type is even nameable here
//! -- and keeps a source-text test beside it, as the plan's T18 acceptance
//! line asks (`the_operator_names_no_sim_internal_type`). The crate is also
//! deterministic without a clock of any kind (spec section 14: "its budget is
//! measured in evaluation units rather than wall time").
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::path::{Path, PathBuf};
use std::process::Command;

use pharmakos_proto::json::{Json, read};

/// Every source file of this crate, named rather than listed:
/// `std::fs::read_dir` is a disallowed method (`clippy.toml`), and a module
/// added without a line here fails `every_module_is_checked`.
const SOURCES: &[&str] = &[
    "src/candidates.rs",
    "src/compose.rs",
    "src/easy.rs",
    "src/lib.rs",
    "src/playbook.rs",
    "src/safe.rs",
    "src/situation.rs",
    "src/terrain.rs",
    "src/tuning.rs",
    "src/wire.rs",
];

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn source(relative: &str) -> String {
    let path = crate_root().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

#[test]
fn every_module_is_checked() {
    for relative in SOURCES {
        assert!(
            crate_root().join(relative).is_file(),
            "{relative} is listed and absent"
        );
    }
    for line in source("src/lib.rs").lines() {
        let name = line
            .strip_prefix("pub mod ")
            .or_else(|| line.strip_prefix("mod "))
            .and_then(|rest| rest.strip_suffix(';'));
        if let Some(name) = name {
            let file = format!("src/{name}.rs");
            assert!(
                SOURCES.contains(&file.as_str()),
                "`mod {name};` is declared and {file} is not on this file's list"
            );
        }
    }
}

/// Assert that no source file contains any of `needles`, naming each hit.
fn assert_absent(needles: &[(&str, &str)]) {
    let mut hits: Vec<String> = Vec::new();
    for relative in SOURCES {
        let text = source(relative);
        for (number, line) in text.lines().enumerate() {
            for (needle, why) in needles {
                if line.contains(needle) {
                    hits.push(format!(
                        "{relative}:{}: `{needle}` -- {why}",
                        number.saturating_add(1)
                    ));
                }
            }
        }
    }
    assert!(hits.is_empty(), "{}", hits.join("\n"));
}

/// Decision C4, the cargo-metadata half: the crate's **normal** dependencies
/// are `pharmakos-proto` and nothing else, it has no build dependencies, and
/// it declares no `[features]`, so it can neither name a sim or gateway type
/// in a shipped build nor reach the sim's `research` feature.
#[test]
fn the_operator_crate_depends_on_proto_alone() {
    let manifest = crate_root().join("Cargo.toml");
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .arg("--manifest-path")
        .arg(&manifest)
        .output()
        .expect("cargo metadata runs");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).expect("UTF-8");
    let metadata = read(&text).expect("cargo metadata's JSON");
    let Some(Json::Array(packages)) = metadata.get("packages") else {
        panic!("cargo metadata lists packages");
    };
    let operator = packages
        .iter()
        .find(|package| {
            package.get("name") == Some(&Json::String(String::from("pharmakos-operator")))
        })
        .expect("the operator is a workspace member");
    let Some(Json::Array(dependencies)) = operator.get("dependencies") else {
        panic!("a package lists its dependencies");
    };
    let mut normal: Vec<String> = Vec::new();
    for dependency in dependencies {
        let name = match dependency.get("name") {
            Some(Json::String(name)) => name.clone(),
            other => panic!("a dependency has a name, not {other:?}"),
        };
        match dependency.get("kind") {
            None | Some(Json::Null) => normal.push(name),
            Some(Json::String(kind)) if kind == "dev" => {}
            other => panic!("`{name}` is a {other:?} dependency; the operator takes none"),
        }
    }
    assert_eq!(
        normal,
        ["pharmakos-proto"],
        "the operator depends on proto alone"
    );
    assert_eq!(
        operator.get("features"),
        Some(&Json::Object(Vec::new())),
        "no [features]: the research feature is unreachable from here"
    );
}

/// The plan's T18 acceptance line, the source-text half beside the metadata
/// walk: no sim-internal type, no gateway type, and no privileged path is
/// named anywhere in the crate's source.
#[test]
fn the_operator_names_no_sim_internal_type() {
    assert_absent(&[
        (
            "pharmakos_sim",
            "the sim is not a dependency and must not become one",
        ),
        (
            "pharmakos_gateway",
            "the operator is the gateway's client, not its code",
        ),
        (
            "pharmakos_plan_core",
            "verify, patch and render go through the gateway",
        ),
        (
            "pharmakos_verifier",
            "the verifier is reached through `verify_plan`",
        ),
        ("Runner", "stepping a match is the gateway's host alone"),
        (
            "Surface",
            "the operator sees the match through its call closure",
        ),
        (
            "SeatId",
            "a sim type: a seat is a `u8` on this side of the wire",
        ),
        (
            "BeaconId",
            "a sim type: a beacon is its `b_NN` on this side of the wire",
        ),
        (
            "seams::Operator",
            "the sim's per-tick execution seam is not the planner",
        ),
    ]);
}

/// Spec section 14: "its budget is measured in evaluation units rather than
/// wall time". No clock, no sleep, no random source, and never the phase
/// timer the host clock moves.
#[test]
fn the_operator_reads_no_clock() {
    assert_absent(&[
        // Spelt so that "instantiate" is not a hit.
        ("Instant::", "a wall clock"),
        ("::Instant", "a wall clock"),
        ("SystemTime", "a wall clock"),
        ("std::time", "a wall clock, or a duration measured on one"),
        ("sleep", "waiting on the host's time"),
        (
            "\"phase_remaining_ms\"",
            "the Lull's timer, which the host clock moves",
        ),
        ("rand::", "Easy draws nothing at random (decision C15)"),
        ("getrandom", "Easy draws nothing at random (decision C15)"),
    ]);
}

/// The paths above are relative to this crate; say where it is if one moves.
#[test]
fn the_crate_root_is_where_the_list_says() {
    assert!(
        Path::new(&crate_root())
            .join("src")
            .join("lib.rs")
            .is_file()
    );
}
