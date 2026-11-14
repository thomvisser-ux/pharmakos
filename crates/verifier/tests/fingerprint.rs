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

/// A playbook that is one long string: the wire bytes and the canonical text
/// are about the same length, so a capacity hint of many times the wire size
/// would reserve tens of megabytes for a two-megabyte file (PR #177's
/// robustness review). The two fingerprints still agree, past every length
/// prefix width, escaped and not.
#[test]
fn a_long_single_string_playbook_has_one_fingerprint_both_ways() {
    for length in [127_usize, 16_384, 2_100_000] {
        let title = "t".repeat(length);
        for text in [
            format!(r#"{{"meta":{{"title":"{title}","author_kind":"HUMAN"}},"kind":"PLAYBOOK"}}"#),
            format!(r#"{{"kind":"PLAYBOOK","meta":{{"note":"\u0041\n{title}","title":"x"}}}}"#),
        ] {
            assert!(
                both(&text).is_some(),
                "a playbook of {length} bytes of text"
            );
        }
    }
}

/// splitmix64, for the stamps the variants below carry: seeded, and the same
/// on every platform.
fn mix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// The members of an object, to change in place.
fn members(value: &mut json::Json) -> Option<&mut Vec<(String, json::Json)>> {
    match value {
        json::Json::Object(entries) => Some(entries),
        _ => None,
    }
}

/// The object under `key`, added empty if the document has none.
fn member_object<'a>(
    entries: &'a mut Vec<(String, json::Json)>,
    key: &str,
) -> Option<&'a mut Vec<(String, json::Json)>> {
    if !entries.iter().any(|(name, _)| name == key) {
        entries.push((key.to_owned(), json::Json::Object(Vec::new())));
    }
    entries
        .iter_mut()
        .find(|(name, _)| name == key)
        .and_then(|(_, value)| members(value))
}

/// Three variants of a playbook document (PR #177's review): stamped with a
/// fingerprint drawn from `seed`; with one field written again under its JSON
/// name, which makes the bytes not plain; and with defaults written out.
fn variants(text: &str, seed: u64) -> Vec<String> {
    let Ok(value) = json::read(text) else {
        return Vec::new();
    };
    let mut out = Vec::new();

    let mut stamped = value.clone();
    if let Some(meta) = members(&mut stamped).and_then(|top| member_object(top, "meta")) {
        meta.retain(|(name, _)| name != "fingerprint");
        let stamp = pharmakos_proto::json::base64::encode(&mix(seed).to_be_bytes());
        meta.push(("fingerprint".to_owned(), json::Json::String(stamp)));
        out.push(json::write(&stamped));
    }

    let mut twice = value.clone();
    if let Some(top) = members(&mut twice) {
        let again = ["on_death", "schema_version"].into_iter().find_map(|name| {
            top.iter()
                .find(|(key, _)| key == name)
                .map(|(_, item)| (name, item.clone()))
        });
        if let Some((name, item)) = again {
            let camel = match name {
                "on_death" => "onDeath",
                _ => "schemaVersion",
            };
            top.push((camel.to_owned(), item));
            out.push(json::write(&twice));
        }
    }

    let mut defaults = value;
    if let Some(top) = members(&mut defaults) {
        if !top.iter().any(|(name, _)| name == "kind") {
            top.push((
                "kind".to_owned(),
                json::Json::String("KIND_UNSPECIFIED".to_owned()),
            ));
        }
        if let Some(version) = member_object(top, "schema_version")
            && !version.iter().any(|(name, _)| name == "minor")
        {
            version.push(("minor".to_owned(), json::Json::Number("0".to_owned())));
        }
        if let Some(meta) = member_object(top, "meta")
            && !meta.iter().any(|(name, _)| name == "note")
        {
            meta.push(("note".to_owned(), json::Json::String(String::new())));
        }
        out.push(json::write(&defaults));
    }
    out
}

/// Every committed case, varied: a random stamp, a field in both spellings,
/// defaults written out. Each variant that decodes has one fingerprint both
/// ways, and the variants reach both plain and not-plain bytes.
#[test]
fn every_committed_playbook_varied_has_one_fingerprint_both_ways() {
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
    let mut plain = 0_usize;
    let mut not_plain = 0_usize;
    for (seed, path) in (0_u64..).zip(&paths) {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for variant in variants(&text, seed) {
            if both(&variant).is_some() {
                agreed = agreed.saturating_add(1);
                let decoded = json::decode_encoded::<Playbook>(&variant).expect("it decoded");
                if decoded.encoded().is_plain() {
                    plain = plain.saturating_add(1);
                } else {
                    not_plain = not_plain.saturating_add(1);
                }
            }
        }
    }
    assert!(agreed > 200, "only {agreed} variants decoded");
    assert!(
        plain > 100 && not_plain > 50,
        "{plain} plain, {not_plain} not"
    );
}
