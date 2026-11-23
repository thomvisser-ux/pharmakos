// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The plan fingerprint of every committed JSONC playbook, three ways (S3's
//! `fpstream`).
//!
//! The gateway takes a seal's fingerprint from its text through this crate's
//! one JSONC decode ([`decode_jsonc`]), so this is where the JSONC corpus is
//! read: the template library (`library/*.jsonc`), the scenarios'
//! playbooks (`scenarios/**/*.playbook.jsonc`) and the example
//! (`examples/playbooks/*.jsonc`). For each, the fingerprint hashed in pieces
//! as the codec writes it (`plan_fingerprint_of`, over the decode's wire
//! bytes), the message's fingerprint (`plan_fingerprint`) and the reference
//! below, the computation as it was before `fpstream`, must be one value.
//! `crates/verifier/tests/fingerprint.rs` holds the same over the verifier's
//! cases and the golden playbooks, which are plain JSON.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml sets allow-expect-in-tests, but that configuration only recognises \
              #[test] functions and #[cfg(test)] modules, not an integration test's helper \
              functions. A panic is this file's failure report."
)]

use std::path::{Path, PathBuf};

use pharmakos_plan_core::decode_jsonc;
use pharmakos_proto::fingerprint::{PLAN_FINGERPRINT_DOMAIN, canonical_input};
use pharmakos_proto::gp::v1::Playbook;
use pharmakos_sim::encoding::digest;
use pharmakos_sim::hash::{plan_fingerprint, plan_fingerprint_of};

/// The fingerprint as it was computed before `fpstream`, kept as the
/// reference in tests only: the canonical text written whole, the domain put
/// before it in one buffer, and one call of the one-shot digest.
fn reference(playbook: &Playbook) -> u64 {
    let canonical = canonical_input(playbook).expect("a decoded playbook writes");
    let mut bytes = PLAN_FINGERPRINT_DOMAIN.to_vec();
    bytes.extend_from_slice(&canonical);
    digest(&bytes)
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Every file under `dir`, at any depth, whose name ends with `suffix`,
/// sorted.
fn files(dir: &Path, suffix: &str) -> Vec<PathBuf> {
    let mut stack = vec![dir.to_path_buf()];
    let mut found = Vec::new();
    while let Some(dir) = stack.pop() {
        #[allow(
            clippy::disallowed_methods,
            reason = "the listing is collected and sorted before use, which is clippy.toml's \
                      sanctioned use of read_dir"
        )]
        let entries = std::fs::read_dir(&dir).expect("a committed folder");
        for entry in entries {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(suffix))
            {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

#[test]
fn every_committed_jsonc_playbook_has_one_fingerprint_three_ways() {
    let corpora = [
        (
            "the template library",
            files(&root().join("library"), ".jsonc"),
            5_usize,
        ),
        (
            "the scenarios",
            files(&root().join("scenarios"), ".playbook.jsonc"),
            30,
        ),
        (
            "the example",
            files(&root().join("examples").join("playbooks"), ".jsonc"),
            1,
        ),
    ];
    let mut total = 0_usize;
    for (name, paths, at_least) in corpora {
        let mut agreed = 0_usize;
        for path in &paths {
            let text = std::fs::read_to_string(path).expect("a committed playbook");
            let canonical = decode_jsonc(&text).unwrap_or_else(|error| {
                panic!(
                    "{}: a committed playbook that does not decode: {error}",
                    path.display()
                )
            });
            let decoded = canonical.decoded();
            let streamed = plan_fingerprint_of(decoded).expect("its bytes write");
            assert_eq!(
                Ok(streamed),
                plan_fingerprint(decoded.message()),
                "{}: the message's fingerprint",
                path.display()
            );
            assert_eq!(
                streamed,
                reference(decoded.message()),
                "{}: the whole-buffer reference",
                path.display()
            );
            agreed = agreed.saturating_add(1);
        }
        assert!(
            agreed >= at_least,
            "{name}: only {agreed} playbooks, under the {at_least} committed"
        );
        total = total.saturating_add(agreed);
    }
    assert!(total >= 36, "only {total} JSONC playbooks in all");
}
