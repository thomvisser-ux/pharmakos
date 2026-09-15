// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The rules this crate is confined by, asserted against its own **source
//! text** and against the schema's vocabulary.
//!
//! The lints deny most of it and `cargo xtask ci`'s `research-guard` proves the
//! dependency edge. This file exists because of the lesson the G2 and G3-prime
//! spikes both wrote down and `crates/sim` and `crates/verifier` both copied:
//! **copy the test, do not merely rely on the lint.** A lint can be switched off
//! by an `#[allow]` somebody adds in a hurry; a test that greps the source
//! cannot be, and it fails in the pull request that tries.
//!
//! Four rules, and the gateway's list differs from the verifier's in two ways
//! that are worth stating rather than quietly leaving out:
//!
//! 1. **No clock, no hash container, no float.** The same list as every
//!    deterministic crate (AGENTS.md sections 4.2, 4.4, 4.5). The gateway is not
//!    a walled crate, so it gets no allowance at all.
//! 2. **No wildcard binding.** `0.0.0.0`, `::`, `UNSPECIFIED` -- the half of
//!    "localhost only" that a type cannot enforce (AGENTS.md section 7).
//! 3. **No outbound connection.** The gateway accepts; it never dials. "No
//!    network telemetry" (AGENTS.md section 7) is a rule about the shipped
//!    binary, and the shipped binary has no client in it.
//! 4. **The filesystem lives in one module.** `std::fs` appears in
//!    `src/cache.rs` and nowhere else, so "no filesystem access through
//!    playbooks" is checkable rather than asserted.
//!
//! And then the rule that is about the *schema* rather than the source: a
//! playbook is data with a closed vocabulary, and no field of that vocabulary
//! names a path, a URL or a socket.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use pharmakos_proto::descriptor::schema;
use std::path::PathBuf;

/// Determinism hazards: the list `crates/sim` and `crates/verifier` are held to.
const BANNED: &[(&str, &str)] = &[
    (
        "HashMap",
        "iteration order is randomly seeded per process (AGENTS.md section 4.4)",
    ),
    (
        "HashSet",
        "iteration order is randomly seeded per process (AGENTS.md section 4.4)",
    ),
    (
        "RandomState",
        "the per-process random seed behind HashMap's order",
    ),
    (
        "DefaultHasher",
        "SipHash with unstable output across toolchain versions",
    ),
    (
        "Instant",
        "wall-clock time; the gateway is not a walled crate (AGENTS.md section 4.5)",
    ),
    ("SystemTime", "wall-clock time, and it can move backwards"),
    ("f32", "the gateway is integer-only (AGENTS.md section 4.2)"),
    ("f64", "the gateway is integer-only (AGENTS.md section 4.2)"),
    (
        "rayon",
        "nothing here is parallelised (AGENTS.md section 4.6)",
    ),
    ("par_iter", "nothing here is parallelised"),
    (
        "thread_rng",
        "a token comes from the OS entropy source, and nothing else here is random",
    ),
    (
        "xxhash",
        "there is exactly one hash function and it is the sim's, reached through \
         pharmakos_sim::digest (AGENTS.md section 5)",
    ),
    (
        ".unwrap()",
        "a panic in the gateway takes the whole match down, and the seat that caused it may \
         have been the one probing",
    ),
];

/// Wildcard addresses: the half of "localhost only" a type cannot enforce.
const NO_WILDCARD_BINDING: &[(&str, &str)] = &[
    (
        "0.0.0.0",
        "the gateway binds loopback only (AGENTS.md section 7)",
    ),
    ("UNSPECIFIED", "a wildcard address by another name"),
    (
        "to_socket_addrs",
        "a name resolved to an address is not a loopback literal",
    ),
];

/// The gateway accepts connections. It never makes one.
const NO_OUTBOUND: &[(&str, &str)] = &[
    (
        "TcpStream::connect",
        "the gateway never dials out: no telemetry, no update check, nothing (AGENTS.md \
         section 7)",
    ),
    (
        "UdpSocket",
        "there is one transport and it is a loopback TCP listener",
    ),
    ("reqwest", "no HTTP client reaches a shipped crate"),
];

/// "No dry runs": the gateway is a client of the sim's *types*, never of its
/// stepping API (AGENTS.md section 3 rules 1 and 2).
///
/// `phase_` is deliberately **not** on this list, unlike `crates/verifier`'s:
/// `phase_remaining_ms` is spec section 12's own field name for the `_status`
/// footer's timer, and a needle that fired on it would be a needle somebody
/// deletes rather than a rule somebody keeps.
const NO_DRY_RUNS: &[(&str, &str)] = &[
    (
        "World::new",
        "the gateway reads a snapshot; it does not build a world to run",
    ),
    (
        ".step(",
        "stepping the sim is a dry run (AGENTS.md section 3 rule 2)",
    ),
    (
        "fork",
        "fork exists only in the research build and no seat may reach it",
    ),
    (
        "research",
        "the gateway must never name the sim's research feature",
    ),
];

/// The `#[allow]`s this crate is permitted, each with the reason it is not a
/// determinism allowance.
///
/// A determinism lint is never on this list and never will be: AGENTS.md section
/// 5 says an `#[allow]` that defeats one is a contract change wearing a
/// disguise.
const PERMITTED_ALLOWS: &[&str] = &["clippy::struct_field_names"];

/// Every `.rs` file under `crates/gateway/src`, in path order.
///
/// `std::fs::read_dir` is on `clippy.toml`'s disallowed list because directory
/// order differs between ext4, NTFS and APFS. The sanctioned use is exactly this
/// one, and `clippy.toml`'s own reason string says so: collect, sort, then use.
#[allow(
    clippy::disallowed_methods,
    reason = "clippy.toml sanctions read_dir when the listing is collected and sorted before use, which is what happens here"
)]
fn sources() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut stack: Vec<PathBuf> = vec![root];
    let mut found: Vec<PathBuf> = Vec::new();
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        let mut here: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .collect();
        here.sort();
        for path in here {
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// The file's shipped code: comment-only lines removed, so prose *about* a
/// banned name is not mistaken for a use of it, and everything from the first
/// `#[cfg(test)]` onward removed, because a unit test is allowed to panic --
/// that is what a failing assertion is -- and is not compiled into the game.
fn code_lines(text: &str) -> Vec<(usize, String)> {
    let shipped = match text.find("#[cfg(test)]") {
        Some(index) => text.get(..index).unwrap_or(text),
        None => text,
    };
    shipped
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim_start().starts_with("//"))
        .map(|(number, line)| (number.saturating_add(1), (*line).to_owned()))
        .collect()
}

fn scan(needles: &[(&str, &str)]) -> Vec<String> {
    let mut findings: Vec<String> = Vec::new();
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (number, line) in code_lines(&text) {
            for (needle, why) in needles {
                if line.contains(needle) {
                    findings.push(format!(
                        "{}:{number}: `{needle}` -- {why}\n    {}",
                        path.display(),
                        line.trim()
                    ));
                }
            }
        }
    }
    findings
}

#[test]
fn the_crate_has_sources_to_check() {
    let files = sources();
    assert!(
        files.len() >= 15,
        "the source walk found only {} files; it is looking in the wrong place and would pass \
         vacuously",
        files.len()
    );
    for expected in ["lib.rs", "token.rs", "fog.rs", "handshake.rs", "cache.rs"] {
        assert!(
            files.iter().any(|path| path.ends_with(expected)),
            "{expected} is outside the checked set"
        );
    }
}

#[test]
fn the_source_scan_is_not_looking_at_an_empty_set() {
    // The hole this closes is a scan that reads nothing and passes. Assert that
    // the filter keeps the code and drops the prose, on a spelling that is in
    // the source right now.
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("fog.rs");
    let text = std::fs::read_to_string(&path).expect("fog.rs is where it always was");
    let lines = code_lines(&text);
    assert!(lines.len() > 50, "the comment filter ate the file");
    assert!(
        text.contains("HashSet"),
        "fog.rs's own prose explains why it is a BTreeSet; if that sentence went, so did the \
         thing this test is proving it can see past"
    );
    assert!(
        !lines.iter().any(|(_, line)| line.contains("HashSet")),
        "the prose survives the comment filter, so the filter is not filtering"
    );
}

#[test]
fn no_clock_no_hash_container_and_no_float_reach_the_crate() {
    let findings = scan(BANNED);
    assert!(
        findings.is_empty(),
        "the determinism rule set is broken in the source text:\n{}",
        findings.join("\n")
    );
}

#[test]
fn no_wildcard_binding_is_written_anywhere() {
    let findings = scan(NO_WILDCARD_BINDING);
    assert!(
        findings.is_empty(),
        "the gateway binds 127.0.0.1 and ::1 and nothing else:\n{}",
        findings.join("\n")
    );
}

#[test]
fn the_gateway_never_dials_out() {
    let findings = scan(NO_OUTBOUND);
    assert!(
        findings.is_empty(),
        "the gateway accepts connections and makes none:\n{}",
        findings.join("\n")
    );
}

#[test]
fn no_stepping_api_is_named_anywhere_in_the_crate() {
    let findings = scan(NO_DRY_RUNS);
    assert!(
        findings.is_empty(),
        "the no-dry-runs rule is broken in the source text:\n{}",
        findings.join("\n")
    );
}

/// AGENTS.md section 7: "No filesystem or network access through playbooks."
///
/// The source half. `std::fs` lives in `src/cache.rs`, which is the private
/// match cache, and in no other module -- so every path this crate opens is
/// built from a validated match id and a fixed file name, and there is no second
/// place for a caller-supplied string to become one.
#[test]
fn the_filesystem_lives_in_one_module() {
    let mut elsewhere: Vec<String> = Vec::new();
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let is_cache = path.ends_with("cache.rs");
        for (number, line) in code_lines(&text) {
            let touches_filesystem = line.contains("std::fs")
                || line.contains("fs::")
                || line.contains("File::")
                || line.contains("OpenOptions");
            if touches_filesystem && !is_cache {
                elsewhere.push(format!("{}:{number}: {}", path.display(), line.trim()));
            }
        }
    }
    assert!(
        elsewhere.is_empty(),
        "the gateway touches the filesystem outside src/cache.rs:\n{}",
        elsewhere.join("\n")
    );

    let cache = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("cache.rs"),
    )
    .expect("cache.rs");
    assert!(
        cache.contains("use std::fs;"),
        "cache.rs is the module this test is about; if it stopped using the filesystem the \
         test above would pass vacuously"
    );
}

/// AGENTS.md section 7, the schema half: "A playbook is data with a closed
/// vocabulary. There is no verb that reads a path, opens a socket, shells out or
/// loads code, and none may be added."
///
/// Asserted over the checked-in descriptor set rather than over this crate,
/// because that is where the claim actually lives: if `gp.v1` ever grew a
/// `file`, a `url` or a `command` field, the gateway would be handing it to
/// something, and no amount of care in this crate would help.
#[test]
fn nothing_a_playbook_can_contain_names_a_path_a_url_or_a_socket() {
    // `Diagnostic.path` and `Diagnostic.related_paths` are RFC 6901 JSON
    // Pointers into a playbook -- `/declarative/route/0/move` -- and not
    // filesystem paths. The name is the verifier's and the whole project's, and
    // the field carries a pointer in every one of the 48 committed verifier
    // report goldens. Named here so the rule can be checked without the one
    // exception quietly widening it.
    const POINTERS_NOT_PATHS: &[&str] = &[
        "gp.api.v1.Diagnostic.path",
        "gp.api.v1.Diagnostic.related_paths",
    ];
    const FORBIDDEN: &[&str] = &[
        "file",
        "filename",
        "filepath",
        "dir",
        "directory",
        "url",
        "uri",
        "host",
        "hostname",
        "port",
        "socket",
        "address",
        "endpoint",
        "command",
        "cmd",
        "exec",
        "shell",
        "script_path",
        "load",
        "import",
        "include",
        "require",
        "module",
    ];

    let mut findings: Vec<String> = Vec::new();
    let mut fields = 0_usize;
    for message in schema().messages() {
        if !message.full_name.starts_with("gp.") {
            continue;
        }
        for field in &message.fields {
            fields = fields.saturating_add(1);
            let full = format!("{}.{}", message.full_name, field.name);
            if POINTERS_NOT_PATHS.contains(&full.as_str()) {
                continue;
            }
            let name = field.name.to_ascii_lowercase();
            for forbidden in FORBIDDEN {
                let matches_word = name == *forbidden
                    || name.starts_with(&format!("{forbidden}_"))
                    || name.ends_with(&format!("_{forbidden}"))
                    || name.contains(&format!("_{forbidden}_"));
                if matches_word {
                    findings.push(format!(
                        "{full} names `{forbidden}`: a playbook is data with a closed \
                         vocabulary and nothing in it reaches the filesystem or a socket \
                         (AGENTS.md section 7)"
                    ));
                }
            }
        }
    }
    assert!(
        fields > 100,
        "the schema walk found only {fields} fields and would pass vacuously"
    );
    assert!(findings.is_empty(), "{}", findings.join("\n"));
}

/// AGENTS.md section 5: an `#[allow]` that defeats a determinism lint is a
/// contract change wearing a disguise.
#[test]
fn no_allow_defeats_a_determinism_lint() {
    let mut findings: Vec<String> = Vec::new();
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for named in allows(&text) {
            if !PERMITTED_ALLOWS.contains(&named.as_str()) {
                findings.push(format!("{}: #[allow({named})]", path.display()));
            }
        }
    }
    assert!(
        findings.is_empty(),
        "an #[allow] this crate has not accounted for. If it is a determinism lint the answer          is not an allow (AGENTS.md section 5); if it is not, add it to PERMITTED_ALLOWS with          the reason:
{}",
        findings.join("
")
    );
    // And the scan can see one: the permitted allow is in the source right now.
    let surface = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("surface.rs"),
    )
    .expect("surface.rs");
    assert_eq!(
        allows(&surface),
        vec![String::from("clippy::struct_field_names")],
        "if this list is empty the test above passes vacuously"
    );
}

/// Every lint named by an `#[allow]` or `#![allow]` in `text`, attribute
/// bodies spanning several lines included -- rustfmt writes one that way as soon
/// as it carries a `reason`, and a line-at-a-time scan would miss exactly those.
fn allows(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(index) = rest.find("#[allow(").or_else(|| rest.find("#![allow(")) {
        let after = rest.get(index..).unwrap_or_default();
        let body_at = after.find('(').map_or(0, |at| at.saturating_add(1));
        let body = after.get(body_at..).unwrap_or_default();
        let end = body.find(")]").unwrap_or(body.len());
        let inner = body.get(..end).unwrap_or_default();
        let name = inner
            .split(',')
            .next()
            .unwrap_or_default()
            .split_whitespace()
            .collect::<String>();
        if !name.is_empty() {
            found.push(name);
        }
        rest = body.get(end..).unwrap_or_default();
    }
    found
}

/// The manifest half of the research ban, which `cargo xtask ci`'s
/// `research-guard` also proves from the dependency graph.
#[test]
fn the_manifest_declares_no_feature_and_takes_the_sim_without_default_features() {
    let manifest =
        std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
            .expect("the crate's own manifest");
    assert!(
        !manifest.lines().any(|line| line.trim() == "[features]"),
        "the gateway declares no features: only crates/sim defines `research`"
    );
    assert!(
        manifest.contains("pharmakos-sim = { workspace = true, default-features = false }"),
        "the sim is taken for its type surface only"
    );
    assert!(
        !manifest.contains("pharmakos-mesher"),
        "the mesher is a walled crate and `cargo xtask wall-guard` fails over this edge"
    );
    // Item 99's one off-list dependency, and the only one.
    assert!(
        manifest.contains("getrandom = { workspace = true }"),
        "{manifest}"
    );
    for banned in [
        "tungstenite",
        "tokio",
        "serde_json",
        "rand",
        "sha1",
        "base64",
        "subtle",
    ] {
        assert!(
            !manifest.contains(&format!("\n{banned} ")),
            "`{banned}` is not on the approved list (AGENTS.md section 3 rule 5)"
        );
    }
}
