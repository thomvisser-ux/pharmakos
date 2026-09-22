// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What this crate may not do, asserted over its own source text.
//!
//! The lints catch most of it and cannot catch any of this: a crate that
//! reaches past the gateway into the sim's `Runner` compiles cleanly, and so
//! does one that quietly grows a second copy of a user-facing string. The
//! gateway keeps a file of this shape for the same reason
//! (`crates/gateway/tests/confinement.rs`), and T13b's review found that its
//! needle was the wrong spelling — so these are chosen to be the spellings
//! somebody would actually write.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::path::{Path, PathBuf};

/// Every source file of this crate, named rather than listed.
///
/// `std::fs::read_dir` is a disallowed method (`clippy.toml`: directory order
/// differs between ext4, NTFS and APFS), and a checker that walked the tree
/// would in any case pass on a crate with a new file in it. A module added
/// without a line here fails `every_module_is_checked`.
const SOURCES: &[&str] = &[
    "src/cli.rs",
    "src/docs.rs",
    "src/doctor.rs",
    "src/exit.rs",
    "src/lib.rs",
    "src/main.rs",
    "src/rules.rs",
    "src/scenario.rs",
    "src/scenario/run.rs",
    "src/schema.rs",
    "src/seat.rs",
    "src/strings.rs",
    "src/verify.rs",
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
            "{relative} is on the list and is not in the crate"
        );
    }
    // And the list is the whole of `mod`: a module declared in lib.rs and
    // missing from SOURCES would be unchecked.
    let lib = source("src/lib.rs");
    for line in lib.lines() {
        let Some(name) = line
            .strip_prefix("pub mod ")
            .and_then(|rest| rest.strip_suffix(';'))
        else {
            continue;
        };
        let file = format!("src/{name}.rs");
        assert!(
            SOURCES.contains(&file.as_str()),
            "`pub mod {name};` is declared and {file} is not on this file's list"
        );
    }
}

/// Decisions-log item 106 (3): "the crate map gives `gamectl` no edge to the
/// sim, so never name the sim's `Runner`".
///
/// The crate *does* take `pharmakos-sim`, and must: the gateway's public API is
/// written in the sim's types, so `Surface::new` cannot be called without
/// naming a `RulesTable` and `Host::open` without a `WorldConfig`. The line the
/// crate is held to is the one item 106 (3) draws — host through the same
/// `Surface` and `Host`, never around them — and these are the spellings that
/// would go around them.
///
/// # What this does *not* forbid, since a review asked
///
/// The crate reads the runner: `crate::scenario::run` calls `Host::runner()`
/// for the tick a Lull is standing on, the phase a refused `begin_push` was in,
/// and the round a seal belongs to. `.runner()` is deliberately **not** a
/// needle, because the guard is against *driving* a `Runner`, not against
/// naming one — and `Host::runner` hands out a `&Runner` whose every stepping
/// method needs `&mut`, so the reach cannot grow into a second way to play a
/// match. If T16a gives `Surface` accessors for those three values, the runner
/// takes them and `.runner()` joins the list below.
#[test]
fn nothing_here_drives_a_match_except_through_the_gateway() {
    const FORBIDDEN: &[(&str, &str)] = &[
        (
            "Runner::",
            "a match is stepped by the gateway's `Host`, which is the only place in the project \
             that drives a `Runner` (decisions-log item 103 (9), item 106 (3))",
        ),
        (
            "runner::Runner",
            "same rule, the other spelling: importing the type is how reaching for it starts",
        ),
        (
            "seal_playbook",
            "the sim's own seal. A playbook reaches a match through `submit_plan` or, for a \
             scenario the verifier refuses, through this crate's one documented harness door -- \
             and both of those go through `Surface`",
        ),
        (
            "world_mut",
            "a mutable world outside the tick is how a harness starts writing sim state",
        ),
        (
            "fork(",
            "`fork` lives behind the sim's `research` feature and nothing shipped may reach it \
             (AGENTS.md section 3 rule 1)",
        ),
    ];
    for relative in SOURCES {
        let text = source(relative);
        for (needle, why) in FORBIDDEN {
            assert!(!text.contains(needle), "{relative} names `{needle}`: {why}");
        }
    }
}

/// `gamectl` is not a walled crate (AGENTS.md §4.9), so it reads no clock and
/// contains no float. The lints deny most of this; a `Duration` on a socket
/// option would not be denied and is not here either.
#[test]
fn there_is_no_clock_and_no_float() {
    const FORBIDDEN: &[&str] = &[
        "Instant",
        "SystemTime",
        "std::time",
        "Duration",
        "f32",
        "f64",
        "HashMap",
        "HashSet",
    ];
    for relative in SOURCES {
        let text = source(relative);
        for needle in FORBIDDEN {
            assert!(
                !text.contains(needle),
                "{relative} names `{needle}`: gamectl is a deterministic crate on the near side \
                 of the wall, and a CLI that timed its own run would be the first wall-clock read \
                 in one"
            );
        }
    }
}

/// AGENTS.md §11: no `connect`, and nothing that would become one.
#[test]
fn nothing_here_attaches_to_a_seat_from_outside() {
    const FORBIDDEN: &[(&str, &str)] = &[
        (
            "TcpStream",
            "a client connection. `seat doctor` asks whether the loopback addresses accept a \
             bind, which is a question about this machine; opening one would be `connect` by \
             another name (AGENTS.md section 11)",
        ),
        (
            "http://",
            "no shipped crate gets an HTTP client: playtest bundles are files a tester sends by \
             hand and there is no network telemetry (AGENTS.md section 7)",
        ),
        (
            "0.0.0.0",
            "the gateway binds 127.0.0.1 and ::1 and nothing else, with no flag that widens it",
        ),
    ];
    for relative in SOURCES {
        let text = source(relative);
        for (needle, why) in FORBIDDEN {
            assert!(!text.contains(needle), "{relative} names `{needle}`: {why}");
        }
    }
    // And `connect` appears in exactly one place: the sentence that says there
    // is not one.
    let table = source("src/cli.rs");
    assert!(
        !table.contains("name: \"connect\""),
        "the command table has no row for it"
    );
}

/// AGENTS.md §12, as much of it as a source-text check can hold: **no message
/// names the binary outside the string table.**
///
/// Named for what it does rather than for what one would like it to do. A
/// review found the earlier name — `every_user_facing_string_is_in_the_string_
/// table` — claiming a guarantee this cannot make: the doctor's `detail` lines
/// and the scenario format's diagnostics are written beside the conditions they
/// describe, and `src/strings.rs`'s header now says so. What *is* enforceable
/// is the tell: a message addressed to a person from this binary names the
/// binary, and the binary's name appearing in a `format!` outside `strings.rs`
/// is a message that grew somewhere else.
///
/// Both spellings are needles, because `strings::BINARY` is the other way to
/// write it and the earlier single needle let `src/cli.rs` past.
#[test]
fn no_message_names_the_binary_outside_the_string_table() {
    for relative in SOURCES {
        if *relative == "src/strings.rs" {
            continue;
        }
        let text = source(relative);
        for (number, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            // Doc comments and ordinary comments are not strings anybody
            // prints, and they talk about the binary constantly.
            if trimmed.starts_with("//") {
                continue;
            }
            let names_it = line.contains("gamectl") || line.contains("strings::BINARY");
            assert!(
                !(line.contains("format!") && names_it),
                "{relative}:{}: a message naming the binary is built outside the string table:\n  \
                 {line}",
                number.saturating_add(1)
            );
        }
    }
}

/// The scenario runner writes fresh outputs; nothing in this crate writes into
/// the repository's committed goldens.
///
/// Checked at the only place a write can happen rather than by looking for the
/// word: one module calls `std::fs::write`, and the path it is given comes from
/// `actual_for`, which strips `expected.` and produces `actual.`. A producer
/// that could write its own expectation is a producer that cannot fail
/// (`tests/golden/README.md` rule 4).
#[test]
fn nothing_here_writes_a_committed_golden() {
    let mut writers: Vec<&str> = Vec::new();
    for relative in SOURCES {
        let text = source(relative);
        for line in text.lines() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            if line.contains("fs::write") || line.contains("fs::copy") {
                if !writers.contains(relative) {
                    writers.push(relative);
                }
                assert!(
                    !line.contains("expected."),
                    "{relative} writes to a committed golden:\n  {line}"
                );
            }
        }
    }
    assert_eq!(
        writers,
        vec!["src/scenario/run.rs"],
        "one module writes a file, and it writes the fresh chain the `golden` step compares. A \
         second writer is a second place a golden could be written from"
    );
    assert!(
        source("src/lib.rs").contains("strip_prefix(\"expected.\")"),
        "`actual_for` is what turns a committed golden's path into the fresh one beside it"
    );
}

/// The research-feature ban, as far as this crate can enforce it on itself.
///
/// **Weaker than what the other four crates get, and said so out loud.**
/// `cargo xtask ci`'s research guard walks `cargo tree -e features` for
/// `plan-core`, `verifier`, `operator` and `gateway`; `gamectl` is on neither
/// that list nor the wall guard's, so what stands in for them here is a read of
/// the manifest text. A manifest read cannot see a *transitive* enablement —
/// some future dependency turning `research` on further down the graph would
/// pass this test and fail the walk. There is no live violation today
/// (`cargo tree -e features -p pharmakos-gamectl` mentions `research` nowhere),
/// and the real fix is one line in `xtask/src/main.rs`: `gamectl` added to
/// `GUARDED_PACKAGES` and to `WALL_GUARDED_PACKAGES`.
///
/// PLACEHOLDER: that one-line change. `xtask`'s definition of what `ci` runs is
/// a contract path (AGENTS.md §5) and this lane does not own it. **OWNER**, at
/// T20's harness half-week, where it sits beside the other two `xtask` items
/// this skeleton has booked there.
#[test]
fn the_crate_takes_the_sim_with_default_features_off() {
    let manifest = std::fs::read_to_string(crate_root().join("Cargo.toml")).expect("the manifest");
    assert!(
        manifest.contains("pharmakos-sim       = { workspace = true }"),
        "the sim is taken through the workspace entry, which carries \
         `default-features = false`:\n{manifest}"
    );
    assert!(
        !manifest.contains("[features]") && !manifest.contains("features = [\"research\"]"),
        "and this crate declares no features of its own and never names the sim's"
    );
    let workspace = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("the workspace root")
            .join("Cargo.toml"),
    )
    .expect("the workspace manifest");
    assert!(
        workspace.contains("pharmakos-sim       = { path = \"crates/sim\", version = \"0\", default-features = false }"),
        "and that entry still has default-features off"
    );
}
