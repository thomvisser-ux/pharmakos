// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The two rules this crate is confined by, asserted against its own **source
//! text**.
//!
//! The lints already deny most of it, and `cargo xtask ci`'s `research-guard`
//! already proves the dependency edge. This file exists because of the one
//! lesson the G2 and G3-prime spikes both wrote down and `crates/sim` copied:
//! **copy the test, do not merely rely on the lint.** A lint can be switched off
//! by an `#[allow]` somebody adds in a hurry; a test that greps the source
//! cannot be, and it fails in the pull request that tries.
//!
//! # 1. No dry runs
//!
//! AGENTS.md section 3 rule 2: the verifier may estimate — pathfinder travel
//! over known terrain, interface-time arithmetic, `$` and `kW` projection,
//! placement legality, selector previews, mast coverage. It may never step the
//! sim, run mandates, programs, combat or construction, model enemy behaviour,
//! or evaluate rule conditions over a projected future. The crate depends on
//! `pharmakos-sim` for its **type surface only**, with `default-features =
//! false`, and this test asserts that no spelling of the stepping API appears in
//! the source at all.
//!
//! # 2. Deterministic like the sim
//!
//! No hash container, no wall clock, no float. A report that differed between
//! Windows, Linux and macOS would be a determinism hole in the verifier rather
//! than a report change, and `tests/golden/verifier/README.md` says so.
//!
//! Prose **about** a banned name is allowed: a comment-only line is skipped, for
//! the same reason `crates/sim/tests/confinement.rs` skips one. A rule has to be
//! writable down.

use std::path::PathBuf;

/// Spellings of the sim's stepping and world-building API.
///
/// `step(` catches `world.step(...)` and every wrapper whose name ends in
/// `step`; `phase_` catches a tick phase reached by name; `World::new` catches
/// building a world to run one.
const NO_DRY_RUNS: &[(&str, &str)] = &[
    (
        "step(",
        "stepping the sim is a dry run (AGENTS.md §3 rule 2)",
    ),
    (
        "fork",
        "fork exists only in the research build and no deterministic client may reach it",
    ),
    (
        "World::new",
        "the verifier reads a snapshot; it does not build a world to run",
    ),
    (
        "phase_",
        "a tick phase is the sim's, and nothing here may reach into one",
    ),
];

/// Determinism hazards, the same list `crates/sim` is held to.
const BANNED: &[(&str, &str)] = &[
    (
        "HashMap",
        "iteration order is randomly seeded per process (AGENTS.md §4.4)",
    ),
    (
        "HashSet",
        "iteration order is randomly seeded per process (AGENTS.md §4.4)",
    ),
    (
        "RandomState",
        "the per-process random seed behind HashMap's order",
    ),
    (
        "DefaultHasher",
        "SipHash with unstable output across toolchain versions",
    ),
    ("Instant", "wall-clock time (AGENTS.md §4.5)"),
    ("SystemTime", "wall-clock time, and it can move backwards"),
    ("f32", "the verifier is integer-only (AGENTS.md §4.2)"),
    ("f64", "the verifier is integer-only (AGENTS.md §4.2)"),
    ("rayon", "nothing here is parallelised (AGENTS.md §4.6)"),
    ("par_iter", "nothing here is parallelised"),
    (
        "thread_rng",
        "unseeded RNG; the project uses split seeded streams (AGENTS.md §4.7)",
    ),
    (
        "xxhash",
        "there is exactly one hash function and it is the sim's (AGENTS.md §5)",
    ),
    (
        ".unwrap()",
        "a panic in the verifier turns a bad playbook into a crash",
    ),
    (
        ".expect(",
        "a panic in the verifier turns a bad playbook into a crash",
    ),
];

/// Every `.rs` file under `crates/verifier/src`, in path order.
///
/// `std::fs::read_dir` is on `clippy.toml`'s disallowed list because directory
/// order differs between ext4, NTFS and APFS. The sanctioned use is exactly this
/// one: collect, sort, then use.
#[allow(
    clippy::disallowed_methods,
    reason = "clippy.toml's own reason string sanctions read_dir when the listing is collected and sorted before use, which is what happens below"
)]
fn sources() -> Vec<PathBuf> {
    let mut stack: Vec<PathBuf> = vec![PathBuf::from("src")];
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

/// The file's shipped code: comment-only lines removed, so prose about a banned
/// name is not mistaken for a use of it, and everything from the first
/// `#[cfg(test)]` onward removed, because a unit test is allowed to panic — that
/// is what a failing assertion is — and is not compiled into the game.
fn code_lines(text: &str) -> Vec<(usize, String)> {
    let shipped = match text.find("#[cfg(test)]") {
        Some(index) => text.get(..index).unwrap_or(text),
        None => text,
    };
    numbered(shipped)
}

/// Every line of the file's code, unit-test modules **included**: comment-only
/// lines removed and nothing else.
///
/// The `#[cfg(test)]` truncation in [`code_lines`] is load-bearing for the panic
/// needles and misleading for anything else. An `#[allow]` inside a `mod tests`
/// in `src/` is still an `#[allow]` in this crate's sources, and a scan that
/// stopped short of it would let the claim "this crate carries no `#[allow]` at
/// all" go on being made after it stopped being true.
fn all_lines(text: &str) -> Vec<(usize, String)> {
    numbered(text)
}

fn numbered(text: &str) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim_start().starts_with("//"))
        .map(|(number, line)| (number + 1, (*line).to_owned()))
        .collect()
}

#[test]
fn the_crate_has_sources_to_check() {
    let files = sources();
    assert!(
        files.len() >= 10,
        "the source walk found only {} files; it is looking in the wrong place and would pass \
         vacuously",
        files.len()
    );
    for expected in ["lib.rs", "decode.rs", "semantics.rs", "hash.rs"] {
        assert!(
            files.iter().any(|path| path.ends_with(expected)),
            "{expected} is outside the checked set"
        );
    }
}

#[test]
fn no_stepping_api_is_named_anywhere_in_the_crate() {
    let mut findings: Vec<String> = Vec::new();
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (number, line) in code_lines(&text) {
            for (needle, why) in NO_DRY_RUNS {
                if line.contains(needle) {
                    findings.push(format!(
                        "{}:{number}: `{needle}` — {why}\n    {}",
                        path.display(),
                        line.trim()
                    ));
                }
            }
        }
    }
    assert!(
        findings.is_empty(),
        "the no-dry-runs rule is broken in the source text:\n{}",
        findings.join("\n")
    );
}

#[test]
fn no_clock_and_no_hash_map_reach_the_crate() {
    let mut findings: Vec<String> = Vec::new();
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (number, line) in code_lines(&text) {
            for (needle, why) in BANNED {
                if line.contains(needle) {
                    findings.push(format!(
                        "{}:{number}: `{needle}` — {why}\n    {}",
                        path.display(),
                        line.trim()
                    ));
                }
            }
        }
    }
    assert!(
        findings.is_empty(),
        "the determinism rule set is broken in the source text:\n{}",
        findings.join("\n")
    );
}

#[test]
fn the_source_scan_is_not_looking_at_an_empty_set() {
    // The hole this closes is a scan that reads nothing and passes: if the walk
    // or the comment filter breaks, every test above goes green over an
    // unchecked crate. So assert that the filter keeps the code and drops the
    // prose, on a spelling that is in the source right now.
    let path = PathBuf::from("src").join("semantics.rs");
    let text = std::fs::read_to_string(&path).expect("semantics.rs is where it always was");
    let lines = code_lines(&text);
    assert!(lines.len() > 50, "the comment filter ate the file");
    assert!(
        text.contains("No dry runs"),
        "the heading this crate is confined by is no longer written in src/semantics.rs"
    );
    assert!(
        !lines.iter().any(|(_, line)| line.contains("No dry runs")),
        "the heading survives the comment filter, so the filter is not filtering"
    );
}

#[test]
fn the_manifest_declares_no_feature_and_takes_the_sim_without_default_features() {
    let manifest = std::fs::read_to_string("Cargo.toml").expect("the crate's own manifest");
    assert!(
        !manifest
            .lines()
            .any(|line| line.trim_start().starts_with("[features]")),
        "this crate declares a feature section; AGENTS.md §3 rule 1 says it declares none, so \
         that no feature of its own can ever forward one of the sim's. (The manifest's comment \
         about not having one is fine: the check reads section headers, not prose.)"
    );
    assert!(
        manifest.contains("pharmakos-sim = { workspace = true, default-features = false }"),
        "the sim dependency must be spelled out with default-features = false, because that \
         exact spelling is what a reader checks (AGENTS.md §3 rule 1)"
    );
}

#[test]
fn every_allow_is_audited_and_carries_a_reason() {
    let mut findings: Vec<String> = Vec::new();
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (number, line) in all_lines(&text) {
            if line.contains("#[allow(") || line.contains("#![allow(") {
                findings.push(format!("{}:{number}: {}", path.display(), line.trim()));
            }
        }
    }
    assert!(
        findings.is_empty(),
        "this crate carries no `#[allow]` at all, and adding one is a review item — a determinism \
         lint may never be switched off outside a walled crate (AGENTS.md §4.9):\n{}",
        findings.join("\n")
    );
}

#[test]
fn the_verifier_never_ranks() {
    // docs/design/targeting.md, "Surfaces": "The verifier checks vocabulary,
    // legality of placement (which arm where), filters and existence, never
    // rank." "Nearest" is the least estimated travel, and the estimator is the
    // sim's pathing module, which the gateway lends to `resolve_refs` and
    // `estimate_route`; a verifier that ranked would need it. So no source
    // here names the pathfinder at all, and the one place the estimate stage
    // is named is `lib.rs`'s pipeline. In particular the resolve stage, which
    // is where a description would be "resolved" if anything here resolved
    // one, never reaches `estimate` (S1's plan, task `tgtv`). This is a
    // tripwire over the text, not a proof: it catches the spellings that name
    // the pathfinder or the stage — a call, an alias, a braced import, a path
    // through `super`, a glob of the sim — and a determined rename could still
    // slip past it, which is what review is for.
    let mut findings: Vec<String> = Vec::new();
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let is_lib = path.ends_with("lib.rs");
        let is_resolve = path.ends_with("resolve.rs");
        for (number, line) in code_lines(&text) {
            if line.contains("pathing") {
                findings.push(format!(
                    "{}:{number}: names the sim's pathfinder\n    {}",
                    path.display(),
                    line.trim()
                ));
            }
            // An alias (`use crate::estimate as e;`), a braced import
            // (`use crate::{estimate, ..}`, on one line or one item per line)
            // and a path through `super` all name the module without the
            // `estimate::` a call through it carries.
            let trimmed = line.trim();
            let imports_it = (trimmed.starts_with("use ") || trimmed.starts_with("pub use "))
                && trimmed.contains("estimate")
                || trimmed.contains("crate::estimate")
                || trimmed.contains("super::estimate")
                || trimmed == "estimate,"
                || trimmed == "estimate"
                || trimmed.starts_with("estimate as ");
            if (line.contains("estimate::") || imports_it) && !is_lib {
                findings.push(format!(
                    "{}:{number}: reaches the estimate stage outside the pipeline\n    {}",
                    path.display(),
                    line.trim()
                ));
            }
            // A glob import of the sim, or of any of its modules, could bring
            // the pathfinder in without naming it.
            if line.contains("pharmakos_sim::") && line.contains('*') {
                findings.push(format!(
                    "{}:{number}: a glob import of the sim can reach the pathfinder unnamed
    {}",
                    path.display(),
                    line.trim()
                ));
            }
            if is_resolve && line.contains("estimate") {
                findings.push(format!(
                    "{}:{number}: the resolve stage names `estimate`\n    {}",
                    path.display(),
                    line.trim()
                ));
            }
        }
    }
    assert!(
        findings.is_empty(),
        "the verifier checks and never ranks:\n{}",
        findings.join("\n")
    );
    // The scan is looking at the right thing: lib.rs does call the stage.
    let lib = std::fs::read_to_string(PathBuf::from("src").join("lib.rs")).expect("lib.rs");
    assert!(
        code_lines(&lib)
            .iter()
            .any(|(_, line)| line.contains("estimate::run")),
        "lib.rs no longer runs the estimate stage, so this scan proves nothing"
    );
}

/// The fallbacks that stay, each with the rule that makes its answer exact
/// (S2's plan, engineering call E7, swept for this crate by task `vfy1`).
///
/// `(file, the line's code, why)`. The scan below finds **every** fallback
/// call in the shipped code (`unwrap_or`, `unwrap_or_default`,
/// `unwrap_or_else`, `map_or`, `map_or_else`), not only the two patterns E7
/// counted, so each default the crate reads in place of a value is either
/// rewritten or written down here with its reason. Rewritten so the failure
/// cannot be written: the map size read as an array, the template filled by
/// splitting at its braces, the walking bound dividing by a speed that cannot
/// be zero, the vent and its covering beacon read in one pattern, the decoder's
/// unknown-field name read with the field test, and the schema version read as
/// present or missing. Typed instead: `report_hash`'s length prefixes
/// (`HashError`).
const KEPT_FALLBACKS: &[(&str, &str, &str)] = &[
    (
        "catalogue.rs",
        "strings::text(self.code).map_or(\"\", |row| row.message)",
        "unreachable: `the_two_tables_name_the_same_codes` holds every catalogue code to a \
         string row, and the catalogue golden pins both",
    ),
    (
        "catalogue.rs",
        "strings::text(self.code).map_or(\"\", |row| row.beginner)",
        "unreachable, as the message above",
    ),
    (
        "estimate.rs",
        "let at = first.unwrap_or_else(|| ROUTE.to_owned());",
        "only a pointer: a refused projection always has a first step, and were it ever not \
         so, E0603 is still raised, at the route",
    ),
    (
        "interface.rs",
        "let extra = i32::try_from(fields.saturating_sub(1)).unwrap_or(i32::MAX);",
        "spec section 5's cap: the edit is min(base + per-field x extra, max), so an extra \
         past i32 prices at the cap, exactly as the true count would",
    ),
    (
        "interface.rs",
        ".unwrap_or_default(),",
        "an absent `initial` is proto3's empty message: it writes no row, which is what an \
         empty row list says",
    ),
    (
        "interface.rs",
        "let targets = Ms::new(i32::try_from(targets).unwrap_or(i32::MAX));",
        "a visit's figure saturates upwards past every segment, the module doc's rule; no \
         playbook inside the size budget comes near it",
    ),
    (
        "interface.rs",
        "u32::try_from(list.len()).unwrap_or(u32::MAX)",
        "a list's elements are priced under the edit's cap or saturate past every segment, so \
         a count past u32 prices the same",
    ),
    (
        "interface.rs",
        "playbook.declarative.as_ref().map_or(Ms::ZERO, |body| {",
        "a playbook with no declarative block has no route, so it stands at no beacon; the \
         block's absence is E0101's",
    ),
    (
        "report.rs",
        "catalogue::entry(diag.code).map_or(Severity::Error, |row| row.severity);",
        "unreachable: `nothing_outside_the_catalogue_is_ever_emitted` holds every emitted code \
         to a row, and an unknown code would refuse rather than pass",
    ),
    (
        "size.rs",
        "initial.mandate.as_ref().map_or(0, mandate_units)",
        "an absent mandate block holds no Build target or area, so it occupies no unit (item \
         94's unit list)",
    ),
    (
        "size.rs",
        "u32::try_from(len).unwrap_or(u32::MAX)",
        "the size meter is compared with a u32 budget row, so a saturated count fires E0110 as \
         the true one would; inside verify the input's u32 byte length bounds it",
    ),
    (
        "structure.rs",
        "Some(condition::Node::All(all)) => all.items.iter().map(tree_depth).max().unwrap_or(0),",
        "an empty group nests nothing below it, so it adds no depth; the empty group itself is \
         E0203's",
    ),
    (
        "structure.rs",
        "Some(condition::Node::Any(any)) => any.items.iter().map(tree_depth).max().unwrap_or(0),",
        "as an empty `all`, above",
    ),
    (
        "structure.rs",
        "Some(condition::Node::Not(not)) => not.item.as_deref().map_or(0, tree_depth),",
        "a `not` with no item nests nothing; the missing item is E0101's",
    ),
    (
        "structure.rs",
        "Some(condition::Node::Not(not)) => not.item.as_deref().map_or(0, tree_nodes),",
        "a `not` with no item holds no node below it; the missing item is E0101's",
    ),
    (
        "walking.rs",
        "Ms::from_ticks(u32::try_from(ticks).unwrap_or(u32::MAX))",
        "a lower bound lowered is still a lower bound, the one direction the walking bound may \
         err in",
    ),
];

/// Every fallback call in the crate's shipped code, as `(file name, the
/// line)`: `unwrap_or` (and so `unwrap_or_default` and `unwrap_or_else`),
/// `map_or` and `map_or_else`.
fn fallbacks() -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        for (_, line) in code_lines(&text) {
            if line.contains("unwrap_or")
                || line.contains("map_or(")
                || line.contains("map_or_else(")
            {
                found.push((name.clone(), line.trim().to_owned()));
            }
        }
    }
    found
}

#[test]
fn no_silent_fallback_is_left() {
    let found = fallbacks();
    let mut unexplained: Vec<String> = Vec::new();
    for (file, line) in &found {
        let kept = KEPT_FALLBACKS
            .iter()
            .any(|(kept_file, code, _)| kept_file == file && line.contains(code));
        if !kept {
            unexplained.push(format!("{file}: {line}"));
        }
    }
    assert!(
        unexplained.is_empty(),
        "a value falls back to a default instead of saying what went wrong. Return a typed \
         error, make the failure unrepresentable, or, where the default is a rule that leaves \
         the answer exact, add the line to KEPT_FALLBACKS with that rule:\n{}",
        unexplained.join("\n")
    );
    for (file, code, why) in KEPT_FALLBACKS {
        let count = found
            .iter()
            .filter(|(found_file, line)| found_file == file && line.contains(code))
            .count();
        assert_eq!(
            count, 1,
            "KEPT_FALLBACKS' entry in {file} (`{code}`, kept because {why}) matches {count} \
             lines; an entry that matches nothing is stale, and one that matches two hides one"
        );
        assert!(!why.is_empty(), "{file}: a kept fallback names its rule");
    }
}

#[test]
fn the_fallback_scan_finds_what_it_looks_for() {
    // The scan must see the kept fallbacks, or every assertion above passes
    // over nothing.
    let found = fallbacks();
    assert_eq!(
        found.len(),
        KEPT_FALLBACKS.len(),
        "the fallback scan sees {} lines and the list keeps {}",
        found.len(),
        KEPT_FALLBACKS.len()
    );
    assert!(
        found.iter().any(|(file, _)| file == "walking.rs"),
        "the fallback scan no longer sees walking.rs' kept conversion"
    );
}
