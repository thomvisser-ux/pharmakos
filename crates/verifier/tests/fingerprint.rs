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
//! Since S3's `fpstream`, `plan_fingerprint_of` hashes its text in pieces as
//! the codec writes it, and never holds it whole. So every fingerprint here
//! is also held to [`reference`], the computation as it was before: the whole
//! text in one buffer and one call of the one-shot digest. The reference
//! lives in tests only (one fingerprint function ships), and the golden
//! playbooks under `tests/golden` join the corpus.
//!
//! The tests live here, beside the fingerprint's one caller in the decode
//! stage, because the sim's source is held to its tick's rules (no `expect`,
//! no `read_dir`) by `crates/sim/tests/confinement.rs`.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml sets allow-expect-in-tests, but that configuration only recognises               #[test] functions and #[cfg(test)] modules, not an integration test's helper               functions. A panic is this file's failure report."
)]

use pharmakos_proto::fingerprint::{PLAN_FINGERPRINT_DOMAIN, canonical_input};
use pharmakos_proto::gp::v1::Playbook;
use pharmakos_proto::json;
use pharmakos_sim::encoding::digest;
use pharmakos_sim::hash::{plan_fingerprint, plan_fingerprint_of};

/// The fingerprint as it was computed before S3's `fpstream`, kept here as
/// the reference and nowhere in shipped code: the playbook's canonical text
/// written whole ([`canonical_input`]), the domain put before it in one
/// buffer, and one call of the one-shot digest over that buffer.
fn reference(playbook: &Playbook) -> u64 {
    let canonical = canonical_input(playbook).expect("a decoded playbook writes");
    let mut bytes = PLAN_FINGERPRINT_DOMAIN.to_vec();
    bytes.extend_from_slice(&canonical);
    digest(&bytes)
}

/// Both fingerprints of one document, which must agree with each other and
/// with the whole-buffer [`reference`]; `None` when the document is not a
/// playbook.
fn both(text: &str) -> Option<(u64, u64)> {
    let decoded = json::decode_encoded::<Playbook>(text).ok()?;
    let old = plan_fingerprint(decoded.message()).expect("a decoded playbook writes");
    let new = plan_fingerprint_of(&decoded).expect("its bytes write");
    assert_eq!(new, old, "the two fingerprints of {text}");
    assert_eq!(
        new,
        reference(decoded.message()),
        "the streamed fingerprint and the whole-buffer reference of {text}"
    );
    Some((old, new))
}

/// Every committed playbook in plain JSON outside the verifier's cases: the
/// golden playbooks under `tests/golden`. The JSONC files (the template
/// library, the scenarios, the example) are read through plan-core's JSONC
/// decode in `crates/plan-core/tests/fingerprint.rs`.
#[test]
fn every_golden_playbook_has_one_fingerprint_three_ways() {
    let mut stack = vec![root().join("tests").join("golden")];
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    while let Some(dir) = stack.pop() {
        #[allow(
            clippy::disallowed_methods,
            reason = "the listing is collected and sorted before use, which is clippy.toml's \
                      sanctioned use of read_dir"
        )]
        let entries = std::fs::read_dir(&dir).expect("a golden folder");
        for entry in entries {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == "playbook.json" || name.ends_with(".playbook.json"))
            {
                paths.push(path);
            }
        }
    }
    paths.sort();
    let mut agreed = 0_usize;
    for path in &paths {
        let text = std::fs::read_to_string(path).expect("a golden playbook");
        assert!(
            both(&text).is_some(),
            "{}: a committed golden playbook that does not decode",
            path.display()
        );
        agreed = agreed.saturating_add(1);
    }
    assert!(agreed >= 6, "only {agreed} golden playbooks found");
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
        // Refused since S3's `vfy6`, where they once took the fingerprint's
        // second path: empty bytes spelled the long way, a field under both
        // its names. And a duplicate key, which is not JSON.
        r#"{"meta":{"title":"t","fingerprint":"===="}}"#,
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
    assert_eq!(decoded, documents.len().saturating_sub(3));
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

/// Strings of two-, three- and four-byte characters longer than one of the
/// codec's string pieces (1,365 bytes) and than its gathering buffer (8,192
/// bytes), shifted by an ASCII prefix of 0 to 3 bytes so a character
/// straddles every place a piece or the buffer could end (S3's `fpstream`,
/// fix pass 1: a cut that split a character, or replaced the halves, moves
/// the fingerprint, and the reference catches it).
#[test]
fn a_long_string_of_wide_characters_has_one_fingerprint_three_ways() {
    let mut agreed = 0_usize;
    for wide in ['\u{e9}', '\u{20ac}', '\u{1f600}'] {
        for prefix in 0..4_usize {
            for bytes in [1_365_usize, 1_366, 2_731, 8_192, 8_193, 20_000] {
                let count = bytes
                    .checked_div(wide.len_utf8())
                    .expect("a character is at least one byte")
                    .saturating_add(1);
                let title: String = "a"
                    .repeat(prefix)
                    .chars()
                    .chain(core::iter::repeat_n(wide, count))
                    .collect();
                let text = format!(
                    r#"{{"meta":{{"title":"{title}","note":"\n{title}"}},"kind":"PLAYBOOK"}}"#
                );
                assert!(
                    both(&text).is_some(),
                    "{} bytes of {wide:?} after {prefix} of ASCII",
                    title.len()
                );
                agreed = agreed.saturating_add(1);
            }
        }
    }
    assert_eq!(agreed, 72);
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

/// Three variants of a playbook document (PR #177's review), each with whether
/// it decodes when the document does: stamped with a fingerprint drawn from
/// `seed`; with one field written again under its JSON name, which the codec
/// refuses since S3's `vfy6` (it once made the bytes not plain, and the
/// fingerprint took a second path); and with defaults written out.
fn variants(text: &str, seed: u64) -> Vec<(String, bool)> {
    let Ok(value) = json::read(text) else {
        return Vec::new();
    };
    let mut out = Vec::new();

    let mut stamped = value.clone();
    if let Some(meta) = members(&mut stamped).and_then(|top| member_object(top, "meta")) {
        meta.retain(|(name, _)| name != "fingerprint");
        let stamp = pharmakos_proto::json::base64::encode(&mix(seed).to_be_bytes());
        meta.push(("fingerprint".to_owned(), json::Json::String(stamp)));
        out.push((json::write(&stamped), true));
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
            out.push((json::write(&twice), false));
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
        out.push((json::write(&defaults), true));
    }
    out
}

/// Every committed case, varied: a random stamp, a field in both spellings,
/// defaults written out. Each variant that decodes has one fingerprint both
/// ways, and a field in both spellings never decodes.
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
    let mut refused = 0_usize;
    for (seed, path) in (0_u64..).zip(&paths) {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let decodes = json::decode_encoded::<Playbook>(&text).is_ok();
        for (variant, decodes_too) in variants(&text, seed) {
            if both(&variant).is_some() {
                assert!(decodes_too, "a field in both spellings decoded: {variant}");
                agreed = agreed.saturating_add(1);
            } else if decodes && !decodes_too {
                refused = refused.saturating_add(1);
            }
        }
    }
    assert!(agreed > 200, "only {agreed} variants decoded");
    assert!(
        refused > 50,
        "only {refused} variants in both spellings refused"
    );
}
