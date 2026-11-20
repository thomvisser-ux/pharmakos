// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A patch cannot grow a document without bound (S3's `vfyq2`, fix pass 1,
//! item 2).
//!
//! `patch_plan` takes a JSON Patch from a socket. RFC 6902's `copy` copies a
//! subtree, the root included, so each `copy` of the root into a member
//! doubles the document: the security review of PR #198 measured 66 MB from
//! 18 operations (675 bytes of patch) and an aborted process from about 40.
//! The budget is an invariant of `apply`: at most
//! [`MAX_PATCH_OPERATIONS`] operations; the document, after each operation,
//! within the playbook's cap in characters and the grammar's nesting limit;
//! a `copy` refused before it copies what would pass the cap; and the inverse
//! patch within the same cap. Each refusal is typed by its [`Bound`].

#![allow(
    clippy::expect_used,
    reason = "clippy.toml's allow-expect-in-tests only recognises #[test] functions, not an               integration test's helpers. A panic is this file's failure report."
)]

use pharmakos_plan_core::error::Bound;
use pharmakos_plan_core::jsonc::Document;

use pharmakos_plan_core::patch::{MAX_PATCH_OPERATIONS, patch_text};
use pharmakos_proto::json::grammar::MAX_DEPTH;
use pharmakos_verifier::MAX_PLAYBOOK_CHARS;
use std::path::{Path, PathBuf};

const MINIMAL: &str = concat!(
    "{\"schema_version\":{\"major\":1},",
    "\"meta\":{\"title\":\"t\",\"author_kind\":\"HUMAN\"},",
    "\"declarative\":{\"route\":[{\"label\":\"a\",\"hold\":{\"ms\":1000}}]},",
    "\"on_death\":{\"on_respawn\":\"CONTINUE\"},",
    "\"fallback\":{\"hold\":{\"at\":{\"beacon_anchor\":{\"safest\":{}}}}},",
    "\"kind\":\"PLAYBOOK\"}"
);

fn bound_of(text: &str, patch: &str) -> Option<Bound> {
    match patch_text(text, patch) {
        Ok(_) => None,
        Err(error) => error.bound,
    }
}

#[test]
fn copying_the_root_into_itself_is_refused_before_it_passes_the_cap() {
    let operations: Vec<String> = (0..18)
        .map(|index| format!(r#"{{"op":"copy","from":"","path":"/x{index}"}}"#))
        .collect();
    let patch = format!("[{}]", operations.join(","));
    assert_eq!(bound_of(MINIMAL, &patch), Some(Bound::Size));
}

#[test]
fn more_operations_than_the_cap_are_refused_before_any_is_applied() {
    let operations: Vec<String> = (0..=MAX_PATCH_OPERATIONS)
        .map(|_| r#"{"op":"test","path":"/kind","value":"PLAYBOOK"}"#.to_owned())
        .collect();
    let patch = format!("[{}]", operations.join(","));
    assert_eq!(bound_of(MINIMAL, &patch), Some(Bound::Operations));
    let at_the_cap = format!("[{}]", operations.get(1..).expect("operations").join(","));
    assert_eq!(bound_of(MINIMAL, &at_the_cap), None);
}

#[test]
fn a_value_nested_past_the_grammars_limit_is_refused() {
    // The title holds `MAX_DEPTH - 2` nested arrays around a number, so the
    // number stands inside exactly `MAX_DEPTH` containers with the root and
    // `meta`: the most the grammar reads. A patch's own value cannot be that
    // deep (the patch's array and object hold it), so the depth comes from
    // copying the title into itself.
    let arrays = MAX_DEPTH - 2;
    let title = format!("{}1{}", "[".repeat(arrays), "]".repeat(arrays));
    let text = MINIMAL.replace("\"title\":\"t\"", &format!("\"title\":{title}"));
    assert!(Document::parse(&text).is_ok(), "the file is at the limit");
    let level = r#"[{"op":"copy","from":"/meta/title/0","path":"/meta/title/-"}]"#;
    assert!(patch_text(&text, level).is_ok(), "a copy at the same depth");
    let deeper = r#"[{"op":"copy","from":"/meta/title","path":"/meta/title/0"}]"#;
    assert_eq!(bound_of(&text, deeper), Some(Bound::Depth));
}

#[test]
fn an_inverse_past_the_cap_is_refused() {
    let big = "x".repeat(MAX_PLAYBOOK_CHARS >> 1);
    let text = MINIMAL.replace("\"title\":\"t\"", &format!("\"title\":\"{big}\""));
    let swap = format!(
        r#"[{{"op":"replace","path":"/meta/title","value":"s"}},{{"op":"replace","path":"/meta/title","value":"{big}"}},{{"op":"replace","path":"/meta/title","value":"s"}}]"#
    );
    assert_eq!(bound_of(&text, &swap), Some(Bound::Inverse));
}
/// splitmix64.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            usize::try_from(self.next() % u64::try_from(n).expect("a count"))
                .expect("below a usize")
        }
    }
}

/// Every pointer to a node in `value`.
fn pointers(value: &pharmakos_proto::json::Json, at: &str, out: &mut Vec<String>) {
    use pharmakos_proto::json::Json;
    out.push(at.to_owned());
    match value {
        Json::Object(members) => {
            for (key, inner) in members {
                let key = key.replace('~', "~0").replace('/', "~1");
                pointers(inner, &format!("{at}/{key}"), out);
            }
        }
        Json::Array(items) => {
            for (index, inner) in items.iter().enumerate() {
                pointers(inner, &format!("{at}/{index}"), out);
            }
        }
        _ => {}
    }
}

#[allow(
    clippy::disallowed_methods,
    reason = "clippy.toml's own reason string sanctions read_dir when the listing is collected               and sorted before use, which is what happens below"
)]
fn playbooks() -> Vec<String> {
    let root: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the workspace root")
        .to_path_buf();
    let mut found = Vec::new();
    for dir in [
        "library",
        "examples/playbooks",
        "crates/plan-core/tests/cases",
    ] {
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
            continue;
        };
        let mut paths: Vec<PathBuf> = entries
            .map(|entry| entry.expect("an entry").path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "jsonc")
            })
            .collect();
        paths.sort();
        for path in paths {
            found.push(std::fs::read_to_string(&path).expect("a committed playbook"));
        }
    }
    found.push(MINIMAL.to_owned());
    found
}

/// Over seeded patches of every operation, aimed at every node of committed
/// playbooks, many of them copies that double the document: only the typed
/// error ever escapes, and no document or inverse handed back passes the cap.
#[test]
fn no_seeded_patch_grows_a_document_past_the_cap() {
    let mut rng = Rng(0x0b0a_7c4f);
    let mut refused_by_size = 0;
    let mut applied = 0;
    for text in playbooks() {
        let tree = Document::parse(&text)
            .expect("a committed playbook")
            .to_json();
        let mut all = Vec::new();
        pointers(&tree, "", &mut all);
        for _ in 0..200 {
            let count = 1 + rng.below(40);
            let operations: Vec<String> = (0..count)
                .map(|index| {
                    // Half the copies are of the root: each doubles the document.
                    let from = if rng.below(2) == 0 {
                        ""
                    } else {
                        all.get(rng.below(all.len())).expect("a pointer").as_str()
                    };
                    let path = format!(
                        "{}/x{index}",
                        all.get(rng.below(all.len())).expect("a pointer")
                    );
                    match rng.below(6) {
                        0..=2 => {
                            format!(r#"{{"op":"copy","from":"{from}","path":"/x{index}"}}"#)
                        }
                        3 => format!(r#"{{"op":"copy","from":"{from}","path":"{path}"}}"#),
                        4 => format!(r#"{{"op":"move","from":"{from}","path":"/m{index}"}}"#),
                        _ => format!(r#"{{"op":"remove","path":"{from}"}}"#),
                    }
                })
                .collect();
            let patch = format!("[{}]", operations.join(","));
            match patch_text(&text, &patch) {
                Ok((patched, inverse)) => {
                    applied += 1;
                    assert!(
                        patched.chars().count() <= MAX_PLAYBOOK_CHARS + 2 * MAX_PATCH_OPERATIONS
                    );
                    assert!(inverse.chars().count() <= 2 * MAX_PLAYBOOK_CHARS);
                }
                Err(error) => {
                    if error.bound == Some(Bound::Size) {
                        refused_by_size += 1;
                    }
                }
            }
        }
    }
    println!("{applied} applied, {refused_by_size} refused at the cap");
    assert!(
        refused_by_size > 0 && applied > 0,
        "{applied} {refused_by_size}"
    );
}
