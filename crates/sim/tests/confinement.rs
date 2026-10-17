// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The determinism rule set, asserted against the crate's own **source text**.
//!
//! The lints in `[workspace.lints]` and `clippy.toml` already deny all of this.
//! This file exists anyway, and it is the one lesson G2 §9.11 and G3′ §9.17
//! both wrote down: *copy the test, do not merely rely on the lint*. A lint can
//! be turned off in a manifest edit that reads like housekeeping; a test that
//! greps the source cannot be, and it fails in the pull request that tries.
//!
//! What it checks:
//!
//! * no `HashMap`, `HashSet`, `RandomState` or `DefaultHasher`;
//! * no `Instant`, `SystemTime` or any other wall clock — including in the
//!   determinism binary, which is inside the deny set because
//!   `cargo xtask clippy` pass 1 lints this package `--all-targets`;
//! * no `f32` or `f64`;
//! * nothing that writes sim state in parallel;
//! * no `unwrap()` or `expect()` reachable from a tick;
//! * every sort with a caller-supplied key — in **all** of its spellings, not
//!   just `sort_by_key` — carries a note at the call site saying its key ends
//!   in a unique id (item 62);
//! * every `#[allow]` in the crate names a lint on the audit list below and
//!   carries a `reason`, and the `as_conversions` family appears **only** under
//!   `src/math/`, which is the audited-widening-cast rule of AGENTS.md §4.3
//!   turned into a check.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules — not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::path::{Path, PathBuf};

/// The only lints this crate may locally allow, and why each is permitted.
///
/// `float_arithmetic`, `float_cmp`, `disallowed_types` and
/// `disallowed_methods` are deliberately absent: those are the determinism
/// bans, and AGENTS.md §5 says an `#[allow]` on one of them is a contract
/// change wearing a disguise.
const ALLOWED_LINTS: &[&str] = &[
    // The audited widening and narrowing casts (AGENTS.md §4.3).
    "clippy::as_conversions",
    "clippy::cast_possible_truncation",
    "clippy::cast_possible_wrap",
    "clippy::cast_sign_loss",
    // Indexing proved in bounds by a const fn's own loop condition.
    "clippy::indexing_slicing",
    // Division whose rounding is the point of the function it is in.
    "clippy::integer_division",
    // An empty tick-phase stub that keeps the tick's shape visible.
    "clippy::unused_self",
    // A row append with one argument per column of an SoA table, named at
    // every call site. Neither a determinism lint nor a judgement call: the
    // bulk form of the same thing is the `*Columns` struct beside it.
    "clippy::too_many_arguments",
    // A flat projection, or a constructor, whose whole point is that it is one
    // expression: a `Snapshot` cannot be built in halves and a restore's checks
    // cannot move behind its writes.
    "clippy::too_many_lines",
    // A hashed state struct whose flags are each one fixed-width field with
    // its own meaning (`PlanState`: pinned, the reflex's two, a resumed
    // placement); folding them into a state machine would change the
    // canonical encoding for no reader's gain (S1's targeting).
    "clippy::struct_excessive_bools",
];

/// Lints that may be allowed only under this path prefix.
const MATH_ONLY_LINTS: &[&str] = &[
    "clippy::as_conversions",
    "clippy::cast_possible_truncation",
    "clippy::cast_possible_wrap",
    "clippy::cast_sign_loss",
];

/// Substrings that must not appear in the crate's source, outside comments.
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
    ("f32", "the sim is integer-only (AGENTS.md §4.2)"),
    ("f64", "the sim is integer-only (AGENTS.md §4.2)"),
    (
        "rayon",
        "no phase that writes sim state may be parallelised (AGENTS.md §4.6)",
    ),
    (
        "par_iter",
        "no phase that writes sim state may be parallelised",
    ),
    (
        "thread::spawn",
        "a second thread near world state is the wrong kind of clever",
    ),
    (
        "thread_rng",
        "unseeded RNG; the sim uses split seeded streams (AGENTS.md §4.7)",
    ),
    (
        ".unwrap()",
        "a panic inside a tick is a crash, not a game state",
    ),
    (
        ".expect(",
        "a panic inside a tick is a crash, not a game state",
    ),
];

/// Every spelling of a sort or search that takes a caller-supplied key or
/// comparator.
///
/// Item 62's convention is that such a key **ends in a unique id**; a key that
/// can collide leaves the order of the colliding elements to the sort, and an
/// unstable sort says out loud that the order is then unspecified (AGENTS.md
/// §4.6). One spelling on this list is not enough: the hole that let a
/// non-total sighting key through was a ban on `sort_by_key(|` alone, which the
/// `sort_unstable_by_key(|` next to it does not contain.
///
/// A call site is sanctioned by carrying [`SORT_MARKER`] on the same line —
/// which is a human writing down that they read the key, rather than a list at
/// the top of this file that drifts away from the code it names.
const SORT_SPELLINGS: &[&str] = &[
    "sort_by_key(|",
    "sort_unstable_by_key(|",
    "sort_by_cached_key(|",
    "sort_by(|",
    "sort_unstable_by(|",
    "binary_search_by_key(|",
    "binary_search_by(|",
    "max_by_key(|",
    "min_by_key(|",
];

/// What a sanctioned sort site writes on its own line.
const SORT_MARKER: &str = "// item 62:";

/// Every `.rs` file under `crates/sim/src`, in path order.
///
/// `std::fs::read_dir` is on `clippy.toml`'s disallowed list because directory
/// order differs between ext4, NTFS and APFS. The sanctioned use is exactly
/// this one: collect, sort, then use.
#[allow(
    clippy::disallowed_methods,
    reason = "clippy.toml's own reason string sanctions read_dir when the listing is collected and sorted before use, which is what happens below"
)]
fn sources() -> Vec<PathBuf> {
    let mut stack: Vec<PathBuf> = vec![PathBuf::from("src")];
    let mut found: Vec<PathBuf> = Vec::new();
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut here: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        here.sort();
        for path in here {
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// The file's lines with comment-only lines removed, so prose about a banned
/// name is not mistaken for a use of it.
fn code_lines(text: &str) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim_start().starts_with("//"))
        .map(|(n, line)| (n + 1, (*line).to_owned()))
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
    assert!(
        files.iter().any(|p| p.ends_with("determinism.rs")),
        "the determinism binary must be inside the checked set"
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
fn every_sort_key_has_been_read_by_a_human() {
    // Item 62 as a test rather than as a habit: every sort with a
    // caller-supplied key must say, at the call site, that its key ends in a
    // unique id. The point is not the marker — it is that a new sort cannot be
    // added without someone writing the sentence.
    let mut findings: Vec<String> = Vec::new();
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            let Some(spelling) = SORT_SPELLINGS.iter().find(|s| line.contains(**s)) else {
                continue;
            };
            // The justification may sit on the call's own line or in the
            // comment immediately above it.
            let from = index.saturating_sub(3);
            let sanctioned = lines
                .get(from..=index)
                .unwrap_or_default()
                .iter()
                .any(|l| l.contains(SORT_MARKER));
            if !sanctioned {
                findings.push(format!(
                    "{}:{}: `{spelling}` with no `{SORT_MARKER}` note. Read the key: if it ends \
                     in a unique id, say so there; if it does not, the order is unspecified and \
                     two machines may disagree (AGENTS.md §4.6).\n    {}",
                    path.display(),
                    index.saturating_add(1),
                    line.trim()
                ));
            }
        }
    }
    assert!(
        findings.is_empty(),
        "a sort key reached the crate without being read:\n{}",
        findings.join("\n")
    );
}

#[test]
fn the_sort_check_is_not_looking_at_an_empty_set() {
    // The check above passes vacuously if the spellings stop matching the
    // crate's source — which is exactly how the hole it replaces survived.
    let mut sites = 0_usize;
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (_, line) in code_lines(&text) {
            if SORT_SPELLINGS.iter().any(|s| line.contains(*s)) {
                sites = sites.saturating_add(1);
            }
        }
    }
    assert!(
        sites >= 2,
        "the sort-key check found {sites} call sites; the crate has at least the two in \
         src/knowledge.rs, so the spellings no longer match the source"
    );
}

#[test]
fn every_allow_is_audited_and_carries_a_reason() {
    let mut findings: Vec<String> = Vec::new();
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            if !line.contains("#[allow(") && !line.contains("#![allow(") {
                continue;
            }
            // Gather the attribute, which may span several lines.
            let mut block = String::new();
            let mut cursor = index;
            while let Some(next) = lines.get(cursor) {
                block.push_str(next);
                block.push('\n');
                if next.contains(")]") {
                    break;
                }
                cursor += 1;
                if cursor > index + 12 {
                    break;
                }
            }
            let number = index + 1;
            if !block.contains("reason = \"") {
                findings.push(format!(
                    "{}:{number}: an #[allow] with no `reason`",
                    path.display()
                ));
            }
            for token in block.split(|c: char| !(c.is_alphanumeric() || c == ':' || c == '_')) {
                if !token.starts_with("clippy::") {
                    continue;
                }
                if !ALLOWED_LINTS.contains(&token) {
                    findings.push(format!(
                        "{}:{number}: `{token}` is not on the audit list in this test. If it is a \
                         determinism lint, AGENTS.md §5 says that is a contract change; if it is \
                         not, add it here with its reason.",
                        path.display()
                    ));
                }
                let in_math = path.to_string_lossy().contains("math");
                if MATH_ONLY_LINTS.contains(&token) && !in_math {
                    findings.push(format!(
                        "{}:{number}: `{token}` may be allowed only under src/math — the audited \
                         widening casts live in one module and nowhere else (AGENTS.md §4.3)",
                        path.display()
                    ));
                }
            }
        }
    }
    assert!(
        findings.is_empty(),
        "the #[allow] audit failed:\n{}",
        findings.join("\n")
    );
}

#[test]
fn fork_is_behind_the_research_feature_and_nowhere_else() {
    let text = std::fs::read_to_string(Path::new("src").join("lib.rs"))
        .expect("the crate root is readable");
    assert!(
        text.contains("#[cfg(feature = \"research\")]\npub mod research;"),
        "the research module must be gated at the `mod` declaration, not on its functions, so a \
         default build does not compile a line of it"
    );
    let manifest = std::fs::read_to_string("Cargo.toml").expect("the crate manifest is readable");
    assert!(
        manifest.contains("research = []"),
        "pharmakos-sim is the sole definer of the `research` feature"
    );
    let mut fork_files: Vec<PathBuf> = Vec::new();
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if text.contains("pub fn fork") {
            fork_files.push(path);
        }
    }
    assert_eq!(
        fork_files.len(),
        1,
        "`fork` must be defined in exactly one file, and it is src/research.rs: {fork_files:?}"
    );
}

#[test]
fn there_is_exactly_one_hash_seed_and_one_hash_function() {
    let mut seed_definitions = 0_usize;
    let mut hash_calls: Vec<String> = Vec::new();
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (number, line) in code_lines(&text) {
            if line.contains("const STATE_HASH_SEED") {
                seed_definitions += 1;
            }
            if line.contains("xxh3_64_with_seed") && !path.ends_with("encoding.rs") {
                hash_calls.push(format!("{}:{number}", path.display()));
            }
            for other in ["sha2::", "blake3", "md5", "siphash", "fnv"] {
                assert!(
                    !line.contains(other),
                    "{}:{number}: a second hash function reached the crate (AGENTS.md §5)",
                    path.display()
                );
            }
        }
    }
    assert_eq!(
        seed_definitions, 1,
        "there must be exactly one seed constant"
    );
    assert!(
        hash_calls.is_empty(),
        "the hash function is called from outside src/encoding.rs, which is how a second \
         encoding starts: {hash_calls:?}"
    );
}

/// What a sanctioned fallback writes on its own line, or on one of the three
/// lines above the `unwrap_or` (S2's plan, E7): `// E7: <why>`, naming the
/// rule that makes the default right, or why the read cannot miss.
const FALLBACK_MARKER: &str = "E7:";

/// The files whose fallbacks S2's `base` lane did not sweep, each with the
/// count it left and the reason, carried to the next owner of the sim (S2's
/// plan, risk R14). The count may only fall: a new fallback in any of these
/// files fails the test as surely as one anywhere else.
const CARRIED: &[(&str, usize, &str)] = &[
    (
        "src/world.rs",
        85,
        "row reads of the unit, beacon, structure, seat and target columns, the state hash's \
         walks among them, which `ids` retypes (E1: every row read through `row()`, typed id \
         columns)",
    ),
    (
        "src/tables.rs",
        15,
        "column reads inside the tables E1 retypes, carried to `ids`",
    ),
    (
        "src/programs.rs",
        21,
        "per-unit row reads of the programs, carried to `ids` with E1's row reuse",
    ),
    (
        "src/pathing/router.rs",
        27,
        "per-unit route columns, carried to `ids` with E1's reset of a reused row's walk state",
    ),
    (
        "src/interpreter/state.rs",
        37,
        "per-seat and per-handler interpreter columns, carried to `ids`, the sim's next owner",
    ),
    (
        "src/pathing/clusters.rs",
        31,
        "node-indexed arrays of the HPA* graph, sized at construction, carried to `ids`, the \
         sim's next owner",
    ),
];

/// The file's shipped code: everything before its first `#[cfg(test)] mod`,
/// because a unit test may fall back as it likes.
fn shipped(text: &str) -> &str {
    let mut at = 0;
    while let Some(found) = text.get(at..).and_then(|rest| rest.find("#[cfg(test)]")) {
        let start = at.saturating_add(found);
        let after = text
            .get(start.saturating_add("#[cfg(test)]".len())..)
            .unwrap_or_default();
        if after.trim_start().starts_with("mod ") {
            return text.get(..start).unwrap_or(text);
        }
        at = start.saturating_add(1);
    }
    text
}

/// Every unsanctioned fallback of E7's two patterns in `text`, as line
/// numbers: a `.unwrap_or` (or its `_default` and `_else` forms) whose
/// expression -- the text back to the statement's start, comments blanked --
/// converts with `try_from` or reads with `.get`, `.get_mut`, `.first` or
/// `.last`.
fn fallbacks(text: &str) -> Vec<usize> {
    let code: Vec<String> = shipped(text)
        .lines()
        .map(|line| match line.find("//") {
            Some(at) => line.get(..at).unwrap_or_default().to_owned(),
            None => line.to_owned(),
        })
        .collect();
    let raw: Vec<&str> = text.lines().collect();
    let joined = code.join("\n");
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(hit) = joined.get(from..).and_then(|rest| rest.find(".unwrap_or")) {
        let at = from.saturating_add(hit);
        from = at.saturating_add(1);
        let start = joined
            .get(..at)
            .and_then(|before| before.rfind([';', '{', '}']))
            .map_or(0, |boundary| boundary.saturating_add(1));
        let chain = joined.get(start..at).unwrap_or_default();
        let converts = chain.contains("try_from(");
        let reads = [".get(", ".get_mut(", ".first()", ".last()"]
            .iter()
            .any(|read| chain.contains(read));
        if !(converts || reads) {
            continue;
        }
        let line = joined
            .get(..at)
            .map_or(0, |before| before.matches('\n').count());
        let marked = (line.saturating_sub(3)..=line).any(|index| {
            raw.get(index)
                .is_some_and(|text| text.contains(FALLBACK_MARKER))
        });
        if !marked {
            found.push(line.saturating_add(1));
        }
    }
    found
}

/// S2's plan, E7: no silent fallback is left in the sim. A rules value is
/// refused at load; a conversion or a read that cannot fail by construction is
/// rewritten so its failure is unrepresentable; a reachable failure returns a
/// typed error; a default that is a real rule says so, at the site, with
/// [`FALLBACK_MARKER`]. This scan is what keeps the sweep from drifting back.
#[test]
fn no_silent_fallback_is_left() {
    let mut findings: Vec<String> = Vec::new();
    let mut carried_seen: Vec<(&str, usize)> = Vec::new();
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let lines = fallbacks(&text);
        let name = path.to_string_lossy().replace('\\', "/");
        if let Some((file, cap, why)) = CARRIED.iter().find(|(file, _, _)| name.ends_with(file)) {
            carried_seen.push((file, lines.len()));
            assert!(
                lines.len() <= *cap,
                "{name} holds {} unswept fallbacks, more than the {cap} carried ({why}): a new one \
                 was added at one of lines {lines:?}",
                lines.len()
            );
            continue;
        }
        for line in lines {
            findings.push(format!("{name}:{line}"));
        }
    }
    assert!(
        findings.is_empty(),
        "a silent fallback (S2's plan, E7): refuse it at load, make it unrepresentable, return a \
         typed error, or name the rule with `// E7: <why>`:\n{}",
        findings.join("\n")
    );
    for (file, cap, _) in CARRIED {
        assert!(
            carried_seen.iter().any(|(seen, _)| seen == file),
            "{file} is carried with {cap} fallbacks and no longer exists"
        );
    }
}

#[test]
fn the_fallback_scan_finds_what_it_is_for() {
    let positive = "fn f(v: &[u32], n: i64) -> u32 {\n    let a = v.get(3).copied().unwrap_or(0);\n    let b = u32::try_from(n).unwrap_or(u32::MAX);\n    a + b\n}\n";
    assert_eq!(fallbacks(positive), vec![2, 3], "both patterns are found");
    let marked = "fn f(v: &[u32]) -> u32 {\n    // E7: rule: an empty list reads as zero\n    v.first().copied().unwrap_or(0)\n}\n";
    assert!(fallbacks(marked).is_empty(), "a marked site is sanctioned");
    let elsewhere = "fn f(o: Option<u32>) -> u32 {\n    o.unwrap_or(0)\n}\n#[cfg(test)]\nmod tests {\n    fn g(v: &[u32]) -> u32 { v.get(0).copied().unwrap_or(1) }\n}\n";
    assert!(
        fallbacks(elsewhere).is_empty(),
        "neither a bare option nor a test module is the scan's business"
    );
}
