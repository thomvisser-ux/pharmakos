// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The plan fingerprint, two ways (S3's `vfyq`, decisions-log item 158).
//!
//! The decode stage takes a playbook's fingerprint from the wire bytes it was
//! decoded from (`pharmakos_sim::hash::plan_fingerprint_of`) rather than by
//! encoding the playbook again (`plan_fingerprint`). Every fingerprint a
//! report or a save carries must be the one it was, so these tests hold the
//! two equal on every committed verifier case, on the three fingerprints the
//! sim pins, on stamped, doubled and odd files, and on the two shapes P1 is
//! measured at. `crates/proto`'s differential tests hold the canonical texts
//! the two hash equal on generated playbooks.
//!
//! The tests live here, beside the fingerprint's one caller in the decode
//! stage, because the sim's source is held to its tick's rules (no `expect`,
//! no `read_dir`) by `crates/sim/tests/confinement.rs`.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml sets allow-expect-in-tests, but that configuration only recognises               #[test] functions and #[cfg(test)] modules, not an integration test's helper               functions. A panic is this file's failure report."
)]

use pharmakos_proto::gp::v1::Playbook;
use pharmakos_proto::json;
use pharmakos_sim::hash::{plan_fingerprint, plan_fingerprint_of};

/// Both fingerprints of one document, which must agree; `None` when the
/// document is not a playbook.
fn both(text: &str) -> Option<(u64, u64)> {
    let decoded = json::decode_encoded::<Playbook>(text).ok()?;
    let old = plan_fingerprint(decoded.message()).expect("a decoded playbook writes");
    let new = plan_fingerprint_of(&decoded).expect("its bytes write");
    assert_eq!(new, old, "the two fingerprints of {text}");
    Some((old, new))
}

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

#[test]
fn every_committed_playbook_has_one_fingerprint_both_ways() {
    let cases = root()
        .join("crates")
        .join("verifier")
        .join("tests")
        .join("cases");
    #[allow(
        clippy::disallowed_methods,
        reason = "the listing is collected and sorted before use, which is clippy.toml's \
                  sanctioned use of read_dir"
    )]
    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(&cases)
        .expect("the verifier's cases")
        .map(|entry| entry.expect("a case").path())
        .filter(|path| path.extension().is_some_and(|it| it == "json"))
        .collect();
    paths.sort();
    let mut agreed = 0_usize;
    for path in &paths {
        // A case that is not UTF-8, not JSON or not a playbook has no
        // fingerprint either way; the rest must agree.
        let bytes = std::fs::read(path).expect("the case reads");
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        if both(&text).is_some() {
            agreed = agreed.saturating_add(1);
        }
    }
    assert!(
        agreed > 80,
        "only {agreed} of {} cases decoded",
        paths.len()
    );
}

#[test]
fn the_pinned_fingerprints_come_out_of_the_wire_too() {
    for (case, pinned) in [
        ("expand_east", 0xb4f7_b813_f481_07ad_u64),
        ("budget_128", 0x8c85_6401_4267_efef),
        ("cover_the_nearest_vent", 0xf936_18e4_e517_7bbd),
    ] {
        let path = root()
            .join("crates")
            .join("verifier")
            .join("tests")
            .join("cases")
            .join(format!("{case}.json"));
        let text = std::fs::read_to_string(path).expect("the case");
        assert_eq!(both(&text), Some((pinned, pinned)), "{case}");
    }
}

#[test]
fn a_stamped_doubled_or_odd_file_has_one_fingerprint_both_ways() {
    let documents = [
        // No meta, a meta that holds only the fingerprint, a stamp.
        r#"{"kind":"PLAYBOOK"}"#,
        r#"{"meta":{"fingerprint":"AAAAAAAAAAE="},"kind":"PLAYBOOK"}"#,
        r#"{"meta":{"title":"t","fingerprint":"//////////8="},"kind":"PLAYBOOK"}"#,
        // Empty bytes spelled the long way, which is not plain.
        r#"{"meta":{"title":"t","fingerprint":"===="}}"#,
        // A field spelled twice, which prost merges: not plain.
        r#"{"on_death":{"on_respawn":"CONTINUE"},"onDeath":{"max_deaths_before_fallback":1}}"#,
        r#"{"meta":{"title":"a"},"meta":{"title":"b"}}"#,
        // Defaults spelled out, keys out of order, escapes.
        r#"{"kind":"PLAYBOOK","schema_version":{"minor":0,"major":1},"meta":{"title":"té\n","author_kind":"HUMAN"}}"#,
    ];
    let mut decoded = 0_usize;
    for text in documents {
        if both(text).is_some() {
            decoded = decoded.saturating_add(1);
        }
    }
    // The one with a duplicate key is not JSON; the rest are playbooks.
    assert_eq!(decoded, documents.len().saturating_sub(1));
}

/// The two shapes P1 is measured at (decisions-log item 158 (1)): 63
/// handlers and 127 route steps, each with a 24-node condition.
#[test]
fn the_size_budget_shapes_have_one_fingerprint_both_ways() {
    let leaf =
        |value: usize| format!(r#"{{"segment_elapsed":{{"ms":{{"op":"EQ","value":{value}}}}}}}"#);
    let condition = |id: usize, next: &mut usize| {
        let leaves: Vec<String> = (0..21)
            .map(|_| {
                *next = next.saturating_add(1);
                leaf(*next)
            })
            .collect();
        let group = if id.is_multiple_of(2) { "any" } else { "all" };
        let wrap = |inner: String| format!(r#"{{"{group}":{{"items":[{inner}]}}}}"#);
        wrap(wrap(wrap(leaves.join(","))))
    };
    let tail = r#""on_death":{"on_respawn":"CONTINUE","max_deaths_before_fallback":1},"fallback":{"hold":{"at":{"beacon_anchor":{"safest":{}}}}}"#;
    let head = r#""schema_version":{"major":1},"meta":{"title":"p1","author_kind":"HUMAN"},"kind":"PLAYBOOK""#;
    let mut next = 0_usize;
    let handlers: Vec<String> = (0..63)
        .map(|id| {
            format!(
                r#"{{"id":"h{id:02}","when":{},"body":[{{"label":"h{id:02}_a","hold":{{"ms":1000}}}}],"resume":"CONTINUE","cooldown_ms":5000,"max_fires":2}}"#,
                condition(id, &mut next)
            )
        })
        .collect();
    let steps: Vec<String> = (0..127)
        .map(|id| {
            format!(
                r#"{{"label":"r{id:03}","skip_if":{},"hold":{{"ms":1000}}}}"#,
                condition(id, &mut next)
            )
        })
        .collect();
    let handler_shape = format!(
        r#"{{{head},"declarative":{{"route":[{{"label":"r00","hold":{{"ms":1000}}}}],"handlers":[{}]}},{tail}}}"#,
        handlers.join(",")
    );
    let route_shape = format!(
        r#"{{{head},"declarative":{{"route":[{}]}},{tail}}}"#,
        steps.join(",")
    );
    assert!(
        both(&handler_shape).is_some(),
        "the handler shape is a playbook"
    );
    assert!(
        both(&route_shape).is_some(),
        "the route shape is a playbook"
    );
}
