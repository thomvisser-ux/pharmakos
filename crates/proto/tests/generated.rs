// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The checked-in generated tree matches a fresh generation.
//!
//! `src/generated` is committed so that a plain `cargo build` needs neither
//! `buf` nor `protoc` and a schema change arrives as a reviewable diff. The
//! cost of that decision is that the tree can go stale, and this is the test
//! that stops it: it regenerates into a scratch directory and compares.
//!
//! It **skips with a named reason** when the tools are not installed, the way
//! `cargo xtask ci`'s own steps do. A developer without `buf` still gets a
//! green test run; CI, which installs both, gets the check.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Files `buf` writes that are committed under `crates/proto/src/generated`.
const GENERATED: &[&str] = &["gp/v1/gp.v1.rs", "gp/api/v1/gp.api.v1.rs"];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| panic!("crates/proto sits two levels below the workspace root"))
        .to_path_buf()
}

fn tool_available(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

#[test]
fn the_checked_in_tree_matches_a_fresh_generation() {
    if !tool_available("buf") {
        println!(
            "skipped: buf is not installed (https://buf.build/docs/installation). \
             The committed tree under crates/proto/src/generated is therefore unchecked \
             in this run; CI installs buf and checks it."
        );
        return;
    }
    if !tool_available("protoc-gen-prost") {
        println!(
            "skipped: protoc-gen-prost is not installed. \
             `cargo install protoc-gen-prost` enables this check."
        );
        return;
    }

    let root = workspace_root();
    let scratch = Path::new(env!("CARGO_TARGET_TMPDIR")).join("regenerated");
    // A stale scratch tree would compare equal to itself rather than to a
    // fresh generation, so it goes first.
    let _ignored = fs::remove_dir_all(&scratch);
    let descriptor_out = scratch
        .join("crates")
        .join("proto")
        .join("src")
        .join("generated");
    fs::create_dir_all(&descriptor_out).expect("creating the scratch output directory");

    let descriptor_path = descriptor_out.join("descriptor.binpb");
    run(
        &root,
        "buf",
        &[
            "build",
            "proto",
            "--exclude-source-info",
            "--as-file-descriptor-set",
            "-o",
            &descriptor_path.to_string_lossy(),
        ],
    );
    run(
        &root,
        "buf",
        &[
            "generate",
            "proto",
            "--template",
            "proto/buf.gen.yaml",
            "-o",
            &scratch.to_string_lossy(),
        ],
    );

    let committed = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("generated");

    compare(
        &committed.join("descriptor.binpb"),
        &descriptor_path,
        "the descriptor set",
    );
    for relative in GENERATED {
        let mut fresh = descriptor_out.clone();
        let mut mine = committed.clone();
        for part in relative.split('/') {
            fresh.push(part);
            mine.push(part);
        }
        compare(&mine, &fresh, relative);
    }
}

fn run(cwd: &Path, program: &str, args: &[&str]) {
    let output = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|error| panic!("running {program}: {error}"));
    assert!(
        output.status.success(),
        "{program} {args:?} failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn compare(committed: &Path, fresh: &Path, label: &str) {
    let committed_bytes = fs::read(committed)
        .unwrap_or_else(|error| panic!("reading {}: {error}", committed.display()));
    let fresh_bytes =
        fs::read(fresh).unwrap_or_else(|error| panic!("reading {}: {error}", fresh.display()));
    assert!(
        committed_bytes == fresh_bytes,
        "{label} is out of date ({} bytes committed, {} bytes fresh).\n\
         Regenerate from the workspace root:\n  \
         buf build proto --exclude-source-info --as-file-descriptor-set \
         -o crates/proto/src/generated/descriptor.binpb\n  \
         buf generate proto --template proto/buf.gen.yaml -o .",
        committed_bytes.len(),
        fresh_bytes.len(),
    );
}
