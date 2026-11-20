// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every Fix the verifier offers applies, and removes what it was offered
//! for, whichever name the file writes each field under (S3's `vfyq2`, fix
//! pass 1, item 9).
//!
//! A Fix is an RFC 6902 patch at a pointer the verifier builds from the
//! decoded playbook, so it names every field by its `.proto` name. A file may
//! write a field under its camelCase JSON name instead (proto3's JSON mapping
//! takes both), and `patch_plan` must still apply the Fix to the member the
//! file writes: the correctness review of PR #198 found the `E0209` window Fix
//! refused on a file that wrote `knownWithinMs`. So for every committed
//! verifier case, every diagnostic's every Fix is applied with plan-core's own
//! `patch_text` to the case as written and to the case with every field on
//! the Fix's pointer renamed to its JSON name; each must apply, and the
//! re-verified file must not raise that diagnostic at that pointer again.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::disallowed_methods,
    reason = "a test: a panic is its failure report, and its one read_dir listing is sorted \
              before use (clippy.toml's own sanction)"
)]

use std::path::{Path, PathBuf};

use pharmakos_plan_core::error::split_pointer;
use pharmakos_plan_core::patch::patch_text;
use pharmakos_plan_core::verify::verify_jsonc;
use pharmakos_proto::descriptor::{Message, ScalarKind, schema};
use pharmakos_proto::gp::api::v1::VerifyReport;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::json::{self, Json};
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::tables::SeatId;
use pharmakos_verifier::Scope;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/plan-core sits two levels below the workspace root")
        .to_path_buf()
}

fn cases() -> Vec<(String, String)> {
    let dir = root()
        .join("crates")
        .join("verifier")
        .join("tests")
        .join("cases");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("the verifier's cases")
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let name = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .expect("a file name");
            (
                name,
                std::fs::read_to_string(&path).expect("a committed case"),
            )
        })
        .collect()
}

fn check(text: &str, rules: &RulesTable) -> VerifyReport {
    let scope = Scope::new(SeatId::new(0), SeatEconomy::default());
    verify_jsonc(text, &[], &scope, rules, Depth::Full).expect("the rules table is complete")
}

/// `value` with every field on `pointer` renamed to its JSON name, where the
/// two differ: the file a player who writes camelCase would hold.
fn camel_on(value: &mut Json, pointer: &str) {
    fn walk(value: &mut Json, message: Option<&Message>, tokens: &[String]) {
        let Some((token, rest)) = tokens.split_first() else {
            return;
        };
        match value {
            Json::Object(members) => {
                let Some(message) = message else {
                    return;
                };
                let Some(field) = message.fields.iter().find(|field| field.name == *token) else {
                    return;
                };
                let nested = (field.kind == ScalarKind::Message)
                    .then(|| schema().message(&field.type_name))
                    .flatten();
                for (key, inner) in members.iter_mut() {
                    if key == token {
                        key.clone_from(&field.json_name);
                        walk(inner, nested, rest);
                    }
                }
            }
            Json::Array(items) => {
                let Ok(index) = token.parse::<usize>() else {
                    return;
                };
                if let Some(item) = items.get_mut(index) {
                    walk(item, message, rest);
                }
            }
            _ => {}
        }
    }
    let tokens = split_pointer(pointer).expect("a Fix's pointer splits");
    walk(value, schema().message("gp.v1.Playbook"), &tokens);
}

/// The pointers a Fix's patch acts on.
fn patch_paths(patch: &str) -> Vec<String> {
    let Ok(Json::Array(operations)) = json::read(patch) else {
        panic!("a Fix's patch is a list: {patch}");
    };
    operations
        .iter()
        .flat_map(|operation| {
            ["path", "from"]
                .into_iter()
                .filter_map(move |key| match operation.get(key) {
                    Some(Json::String(pointer)) => Some(pointer.clone()),
                    _ => None,
                })
        })
        .collect()
}

#[test]
fn every_fix_applies_and_removes_its_diagnostic_under_either_spelling() {
    let rules = RulesTable::load(&root().join("rules").join("rules.v1.json"))
        .expect("the committed rules table");
    let mut applied = 0_u32;
    let mut failures: Vec<String> = Vec::new();
    for (name, text) in cases() {
        let report = check(&text, &rules);
        for diagnostic in &report.diagnostics {
            for fix in &diagnostic.suggestions {
                let mut spellings = vec![("as written", text.clone())];
                if let Ok(mut tree) = json::read(&text) {
                    for path in patch_paths(&fix.json_patch) {
                        camel_on(&mut tree, &path);
                    }
                    let camel = json::write(&tree);
                    if camel != json::write(&json::read(&text).expect("read once")) {
                        spellings.push(("in camelCase", camel));
                    }
                }
                for (spelling, file) in spellings {
                    let what = format!(
                        "{name}: {} at {} ({spelling}), Fix {:?}",
                        diagnostic.code, diagnostic.path, fix.title
                    );
                    match patch_text(&file, &fix.json_patch) {
                        Ok((patched, _inverse)) => {
                            applied = applied.saturating_add(1);
                            let again = check(&patched, &rules);
                            if again.diagnostics.iter().any(|found| {
                                found.code == diagnostic.code && found.path == diagnostic.path
                            }) {
                                failures.push(format!("{what}: still raised after the Fix"));
                            }
                        }
                        Err(error) => failures.push(format!("{what}: refused: {error}")),
                    }
                }
            }
        }
    }
    println!("{applied} Fixes applied");
    assert!(applied > 50, "the cases offer Fixes: {applied}");
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
