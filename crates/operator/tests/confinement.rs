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

/// A refused call, or an answer the operator cannot read, is a candidate,
/// a plan or a probe that does not hold: the reason the policy entries below
/// share. Each is the operator's documented decision about what to plan with
/// what the gateway told it, never a value made up in place of one.
const POLICY: &str = "a refused or unreadable answer is a plan, candidate or probe that does not \
                      hold; the module docs state the policy, and the round goes on with what \
                      the gateway did say";

/// The defaults and swallows this crate keeps, as `(file, the trimmed line
/// exactly, how many lines it is, the rule)`.
const KEPT_FALLBACKS: &[(&str, &str, usize, &str)] = &[
    (
        "src/candidates.rs",
        ".ok()",
        2,
        "`route_index`: a pointer that names no index names no route step; the probe's \
         anchor: an answer that does not say where the Defend beacon stands is Post::Unknown, \
         a typed answer",
    ),
    (
        "src/candidates.rs",
        ".ok()?",
        1,
        "a count of enemies is a `usize`, which fits an `i64` on every target; a score that \
         did not fit is a candidate dropped, as `score`'s docs say",
    ),
    ("src/candidates.rs", "Err(_) => Post::Unknown,", 1, POLICY),
    ("src/candidates.rs", "Err(_) => Vec::new(),", 1, POLICY),
    (
        "src/candidates.rs",
        "let Ok(Some((place, travel, lead))) = read else {",
        1,
        POLICY,
    ),
    (
        "src/candidates.rs",
        "let Ok(estimate) = wire.call(\"estimate_route\", params) else {",
        1,
        POLICY,
    ),
    (
        "src/candidates.rs",
        "let threat = i64::try_from(core_threat(situation, tuning)?).ok()?;",
        1,
        "a threat past `i64` is no score at all, and the guard is then not weighed",
    ),
    (
        "src/candidates.rs",
        "let threat = i64::try_from(threat_near(situation, tuning, target.at())?).ok()?;",
        1,
        "a threat past `i64` is no score at all, and the raid is then not weighed",
    ),
    (
        "src/compose.rs",
        "let label = crate::wire::text_of(step, \"label\").ok()?;",
        1,
        POLICY,
    ),
    (
        "src/compose.rs",
        "u64::try_from(ms).map_or_else(",
        1,
        "a duration a sentence names is never negative; one that were is written as the \
         milliseconds it is rather than read as none, so both arms say the figure exactly",
    ),
    (
        "src/easy.rs",
        ".is_ok_and(|answer| bool_of(&answer, \"accepted\") == Ok(true))",
        1,
        "only an answer that reads `accepted: true` is an acceptance",
    ),
    ("src/easy.rs", ".ok()?;", 2, POLICY),
    (
        "src/easy.rs",
        "if bool_of(report, \"qualifies\").ok()? {",
        1,
        POLICY,
    ),
    (
        "src/easy.rs",
        "if let Ok(situation) = read {",
        1,
        "the refusal is kept in `refused` one line before",
    ),
    (
        "src/easy.rs",
        "let next = if let Some(json_patch) = machine_fix(report).ok()? {",
        1,
        POLICY,
    ),
    (
        "src/easy.rs",
        "let out = text_of(&answer, \"playbook_jsonc\").ok()?;",
        1,
        POLICY,
    ),
    (
        "src/playbook.rs",
        ".and_then(|(_, value)| pharmakos_proto::json::read(value).ok())",
        1,
        "a declared value that does not read is no value the operator can copy from",
    ),
    (
        "src/playbook.rs",
        "Json::Array(items) => items.get(token.parse::<usize>().ok()?)?,",
        1,
        "RFC 6901: an array token that is not an index names nothing",
    ),
    (
        "src/playbook.rs",
        "let Ok(answer) = instantiate(wire, template_id, &[]) else {",
        1,
        POLICY,
    ),
    (
        "src/playbook.rs",
        "let index = token.parse::<usize>().ok()?;",
        1,
        "RFC 6901: an array token that is not an index names nothing",
    ),
    (
        "src/safe.rs",
        ".and_then(|digits| digits.parse::<u32>().ok());",
        1,
        "a name that is not `b_<number>` sorts after every numbered one, the order's rule",
    ),
    (
        "src/safe.rs",
        ".is_some_and(|report| bool_of(report, \"qualifies\") == Ok(true))",
        1,
        "only a report that reads `qualifies: true` qualifies",
    ),
    (
        "src/safe.rs",
        "let Ok(estimate) = wire.call(\"estimate_route\", params) else {",
        1,
        POLICY,
    ),
    (
        "src/safe.rs",
        "let Ok(filled) = instantiate(wire, SAFE_PLAYBOOK, &[(pointer, route.clone())]) else {",
        1,
        "the template's own value is the gateway's own fallback and qualifies \
         (`the_gateways_fallback_is_the_safe_template_with_nothing_raised`)",
    ),
    (
        "src/tuning.rs",
        ".ok()",
        1,
        "a cooldown that is not positive is refused as a RulesError on the next line",
    ),
    (
        "src/wire.rs",
        "Json::Number(lexeme) | Json::String(lexeme) => lexeme.parse::<i64>().ok(),",
        1,
        "`int` answers whether a value is a whole number; its callers refuse a misread",
    ),
];

/// A source file's shipped code, each line trimmed: comment-only lines
/// removed, and everything from its first `#[cfg(test)]` onward.
fn shipped_lines(text: &str) -> Vec<String> {
    let shipped = match text.find("#[cfg(test)]") {
        Some(index) => text
            .get(..index)
            .expect("a match starts on a char boundary"),
        None => text,
    };
    shipped
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .map(|line| line.trim().to_owned())
        .collect()
}

/// The needles a fallback or a swallow is written with: the verifier's and
/// the sim's (`unwrap_or`, `map_or`), and the ways a typed error is dropped
/// (`.ok()`, `let Ok(`, `Err(_) =>`, `== Ok(`).
const NEEDLES: &[&str] = &[
    "unwrap_or",
    "map_or(",
    "map_or_else(",
    ".ok()",
    "let Ok(",
    "Err(_) =>",
    "== Ok(",
];

/// Every needle's line in the crate's shipped code, as `(file, the line)`.
fn fallbacks() -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for relative in SOURCES {
        for line in shipped_lines(&source(relative)) {
            if NEEDLES.iter().any(|needle| line.contains(needle)) {
                found.push(((*relative).to_owned(), line));
            }
        }
    }
    found
}

/// No silent fallback is left in the operator (S2's plan, E7, task `oprd`;
/// decisions-log item 136 (5)): the wire's readers answer proto3's default
/// only for an **absent** member and refuse a misshapen one, and every line
/// the scan finds is a typed error, a failure made unrepresentable, the exact
/// answer by a rule, or the operator's stated policy for an answer that does
/// not hold, each listed in [`KEPT_FALLBACKS`] with its reason and count.
#[test]
fn no_silent_fallback_is_left() {
    let found = fallbacks();
    let unexplained: Vec<String> = found
        .iter()
        .filter(|(file, line)| {
            !KEPT_FALLBACKS
                .iter()
                .any(|(kept, code, _, _)| kept == file && line == code)
        })
        .map(|(file, line)| format!("{file}: {line}"))
        .collect();
    assert!(
        unexplained.is_empty(),
        "a value falls back to a default, or an error is dropped, instead of saying what went \
         wrong. Return a typed error, make the failure unrepresentable, or, where the default \
         is a rule or the drop is stated policy, add the line to KEPT_FALLBACKS with it:\n{}",
        unexplained.join("\n")
    );
    for (file, code, count, why) in KEPT_FALLBACKS {
        let matched = found
            .iter()
            .filter(|(found_file, line)| found_file == file && line == code)
            .count();
        assert_eq!(
            matched, *count,
            "KEPT_FALLBACKS' entry in {file} (`{code}`, kept because {why}) matches {matched} \
             lines, not {count}"
        );
    }
    // The scan sees what it keeps, or every assertion above passes over
    // nothing.
    let kept: usize = KEPT_FALLBACKS.iter().map(|(_, _, count, _)| count).sum();
    assert_eq!(found.len(), kept);
}
