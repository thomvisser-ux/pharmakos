// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `gamectl docs`, and the golden that stops it drifting.
//!
//! Spec §12: "`get_schema` and the `gamectl` docs output are generated from the
//! schema so they can't drift." A generated document is only unable to drift
//! while something compares it with what it is generated from, and this is
//! that something: it writes the fresh output where `cargo xtask ci`'s `golden`
//! step looks for it and compares it with the committed copy in the same
//! breath, so `cargo test` alone catches a moved reference.
//!
//! **What a diff here means** is in `tests/golden/docs/README.md`, and the
//! interesting case is the one this shape catches: documentation that moved
//! while the schema did not, which means `docs.rs` started building an answer
//! instead of reading one.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use pharmakos_gamectl::exit::Exit;
use pharmakos_gamectl::{run, target_dir};
use std::path::{Path, PathBuf};

/// The committed copy, and therefore the fresh one beside it.
const GOLDEN: &str = "tests/golden/docs/reference/expected.docs.txt";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| panic!("crates/gamectl sits two levels below the workspace root"))
        .to_path_buf()
}

#[test]
fn the_generated_reference_matches_its_golden() {
    let outcome = run(["docs"], &root());
    assert_eq!(outcome.code, Exit::Ok, "{}", outcome.err);

    // Rule 1 of tests/golden/README.md: a missing fresh output is a failure,
    // not a skip. Written first, so a comparison that fails still leaves the
    // new document on disk to be looked at.
    let fresh = target_dir()
        .expect("an integration test runs from a cargo target directory")
        .join("golden")
        .join("docs")
        .join("reference")
        .join("actual.docs.txt");
    if let Some(parent) = fresh.parent() {
        std::fs::create_dir_all(parent)
            .unwrap_or_else(|error| panic!("creating {}: {error}", parent.display()));
    }
    std::fs::write(&fresh, outcome.out.as_bytes())
        .unwrap_or_else(|error| panic!("writing {}: {error}", fresh.display()));

    let committed = std::fs::read_to_string(root().join(GOLDEN))
        .unwrap_or_else(|error| panic!("reading {GOLDEN}: {error}"));
    let committed = committed.replace("\r\n", "\n");
    if committed == outcome.out {
        return;
    }
    let at = committed
        .lines()
        .zip(outcome.out.lines())
        .position(|(left, right)| left != right)
        .unwrap_or_else(|| committed.lines().count().min(outcome.out.lines().count()));
    panic!(
        "the generated reference moved at line {}:\n  committed  {}\n  generated  {}\n\n\
         tests/golden/docs/README.md says what each kind of diff there means. Re-bless with \
         `cargo xtask golden --bless` and say in the pull request which source moved.",
        at.saturating_add(1),
        committed.lines().nth(at).unwrap_or("<end of file>"),
        outcome.out.lines().nth(at).unwrap_or("<end of output>"),
    );
}

/// The reference is generated, so it has to contain what it is generated from
/// rather than a summary of it.
#[test]
fn the_reference_carries_every_diagnostic_code_and_the_whole_command_table() {
    let outcome = run(["docs"], &root());
    for entry in pharmakos_verifier::catalogue::CATALOGUE {
        assert!(
            outcome.out.contains(entry.code),
            "`{}` is in the catalogue and not in the generated reference",
            entry.code
        );
    }
    for command in pharmakos_gamectl::cli::COMMANDS {
        assert!(
            outcome.out.contains(command.usage),
            "`{}` is a command and not in the generated reference",
            command.usage
        );
    }
    // And the schema's own root, so a reference generated from an empty
    // descriptor set could not pass.
    assert!(outcome.out.contains("gp.v1.Playbook"), "{}", outcome.out);
}

/// `gp.api.v1` is deliberately absent: it is the transport, not the
/// vocabulary, and v1 publishes nothing (AGENTS.md §11).
#[test]
fn the_reference_publishes_no_api() {
    let outcome = run(["docs"], &root());
    assert!(
        !outcome.out.contains("gp.api.v1."),
        "the generated reference documents what a seat authors, not the method surface: \
         llms.txt, the agent guide and the published schema docs ship with v1.1"
    );
}
