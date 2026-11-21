// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A playbook handed over canonicalised is reported exactly as its canonical
//! text is (S3's `vfyq2`, decode once).
//!
//! `plan-core` canonicalises a player's text by decoding it, and hands the
//! verifier the canonical text together with the message it decoded
//! (`Input::canonical`), so the verifier hashes the text and does not decode
//! it a second time. The report must not tell the two doors apart: for every
//! committed case and the two shapes `quick_cost.rs` times, at both depths,
//! `Input::canonical` and `Input::new` over the same canonical text give the
//! same report, byte for byte, `report_hash` and plan fingerprint included.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml's allow-expect-in-tests only recognises #[test] functions, not an \
              integration test's helpers. A panic is this file's failure report."
)]

use std::path::PathBuf;

use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::gp::v1::Playbook;
use pharmakos_proto::json;
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::snapshot::{SNAPSHOT_VERSION, Snapshot};
use pharmakos_sim::tables::SeatId;
use pharmakos_verifier::{Input, Scope, verify};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// Every committed case's text, in name order.
#[allow(
    clippy::disallowed_methods,
    reason = "clippy.toml's own reason string sanctions read_dir when the listing is collected and sorted before use, which is what happens below"
)]
fn cases() -> Vec<(String, String)> {
    let dir = workspace_root()
        .join("crates")
        .join("verifier")
        .join("tests")
        .join("cases");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("the cases directory")
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
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .expect("a file name");
            (
                name,
                std::fs::read_to_string(&path).expect("a committed case"),
            )
        })
        .collect()
}

#[test]
fn a_canonicalised_playbook_is_reported_as_its_canonical_text_is() {
    let rules = RulesTable::load(&workspace_root().join("rules").join("rules.v1.json"))
        .expect("the committed rules table");
    let snapshot = Snapshot {
        version: SNAPSHOT_VERSION,
        match_seed: 0x0102_0304_0506_0708,
        tick: 3_600,
        ..Snapshot::default()
    }
    .to_bytes()
    .expect("the fixture snapshot");
    let scope = Scope::new(SeatId::new(0), SeatEconomy::default());
    let mut compared = 0_u32;
    for (name, text) in cases() {
        let Ok(canonical) = json::canonical_decode::<Playbook>(&text) else {
            continue;
        };
        for depth in [Depth::Quick, Depth::Full] {
            let by_text = Input::new(canonical.text().as_bytes(), &snapshot, &scope, &rules)
                .expect("an input");
            let by_message =
                Input::canonical(&canonical, &snapshot, &scope, &rules).expect("an input");
            assert_eq!(
                verify(&by_message, depth),
                verify(&by_text, depth),
                "{name} at {depth:?}"
            );
        }
        compared = compared.saturating_add(1);
    }
    assert!(
        compared > 80,
        "the cases a canonical form exists for: {compared}"
    );
}
