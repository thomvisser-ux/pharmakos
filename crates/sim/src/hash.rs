// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The plan fingerprint (decisions-log item 77; the register's S1-42).
//!
//! `gp.v1.Meta.fingerprint` is
//!
//! ```text
//!     xxh3-64( PLAN_FINGERPRINT_DOMAIN || canonical_json_bytes )
//! ```
//!
//! under the project's one seed, where the canonical bytes are the playbook's
//! own canonical JSON with `meta.fingerprint` cleared. `crates/proto`'s
//! `fingerprint` module owns the **rule** -- the domain separator, the byte
//! order and what is hashed ([`canonical_input`]) -- and this module owns the
//! **arithmetic**, beside the state hash it shares a function and a seed with:
//! [`digest`] is xxh3-64 at [`crate::encoding::STATE_HASH_SEED`], and there is
//! no second hash function in the project (AGENTS.md section 5).
//!
//! It is determinism code (AGENTS.md section 5): a fingerprint is stamped into
//! every report and every save, so moving this function's value would refuse
//! every save already written. It moved here from `pharmakos-verifier`'s
//! `hash` module in S1's `build` lane **with its value unmoved**, and
//! `the_plan_fingerprint_is_unmoved` (this crate's `tests/vectors.rs`) holds it
//! to the value the committed verifier goldens carry. The verifier and the
//! gateway call this function; the verifier's `hash::plan_fingerprint` is now a
//! thin reader of it.

use crate::encoding::digest;
use pharmakos_proto::fingerprint::{PLAN_FINGERPRINT_DOMAIN, canonical_input};
use pharmakos_proto::gp::v1::Playbook;
use pharmakos_proto::json::{self, Decoded};

/// Why a playbook has no fingerprint.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum FingerprintError {
    /// The playbook cannot be written as canonical JSON, so there are no bytes
    /// to take a fingerprint over. For a message just decoded from canonical
    /// JSON this means the codec has lost a field it can read but not write.
    Canonical(json::Error),
}

impl core::fmt::Display for FingerprintError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FingerprintError::Canonical(error) => write!(
                f,
                "the playbook cannot be written as canonical JSON, so it has no fingerprint: \
                 {error}"
            ),
        }
    }
}

impl std::error::Error for FingerprintError {}

/// The plan fingerprint of a decoded playbook (decisions-log item 77).
///
/// `xxh3-64( PLAN_FINGERPRINT_DOMAIN || canonical_json_bytes )` under the
/// project's one seed. Clearing `meta.fingerprint` before hashing is
/// [`canonical_input`]'s, and it is what makes the fingerprint checkable:
/// recomputing over the file as it stands would hash the old fingerprint into
/// the new one.
///
/// # Errors
///
/// [`FingerprintError::Canonical`] when the playbook cannot be written as
/// canonical JSON.
pub fn plan_fingerprint(playbook: &Playbook) -> Result<u64, FingerprintError> {
    let canonical = canonical_input(playbook).map_err(FingerprintError::Canonical)?;
    let mut bytes = PLAN_FINGERPRINT_DOMAIN.to_vec();
    bytes.extend_from_slice(&canonical);
    Ok(digest(&bytes))
}

/// [`PLAN_FINGERPRINT_DOMAIN`] as text. It is ASCII, and the compiler checks
/// that it is UTF-8, so the canonical text can be written after it in one
/// buffer.
const DOMAIN_TEXT: &str = match core::str::from_utf8(PLAN_FINGERPRINT_DOMAIN) {
    Ok(text) => text,
    Err(_) => panic!("PLAN_FINGERPRINT_DOMAIN is not UTF-8"),
};

/// How many bytes of canonical text a byte of a playbook's wire bytes makes,
/// about: a capacity hint, so the text is not copied as it grows. The text is
/// indented a level for every message it nests, and a condition nests deep:
/// the size budget's shapes measure 23 and 24.
const TEXT_PER_WIRE_BYTE: usize = 24;

/// The field [`canonical_input`] clears, by its path of `.proto` field names.
const FINGERPRINT_FIELD: &[&str] = &["meta", "fingerprint"];

/// The plan fingerprint of a playbook decoded from JSON, from the wire bytes it
/// was decoded from: the value [`plan_fingerprint`] gives the playbook, without
/// encoding it again.
///
/// The canonical bytes are [`json::write_wire_into`]'s over those bytes with
/// `meta.fingerprint` left out, which is the text [`canonical_input`] writes
/// for the playbook when the bytes are plain ([`json::Encoded::is_plain`]):
/// every singular field once, nothing at a default it would drop. Bytes that
/// are not plain, from a file that spells a field twice, are answered by
/// [`plan_fingerprint`] itself.
///
/// # Errors
///
/// [`FingerprintError::Canonical`] when the bytes cannot be written as
/// canonical JSON, as [`plan_fingerprint`].
pub fn plan_fingerprint_of(decoded: &Decoded<Playbook>) -> Result<u64, FingerprintError> {
    let encoded = decoded.encoded();
    if !encoded.is_plain() {
        return plan_fingerprint(decoded.message());
    }
    let mut text = String::with_capacity(
        DOMAIN_TEXT
            .len()
            .saturating_add(encoded.bytes().len().saturating_mul(TEXT_PER_WIRE_BYTE)),
    );
    text.push_str(DOMAIN_TEXT);
    json::write_wire_into(
        "gp.v1.Playbook",
        encoded.bytes(),
        FINGERPRINT_FIELD,
        &mut text,
    )
    .map_err(FingerprintError::Canonical)?;
    Ok(digest(text.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::{plan_fingerprint, plan_fingerprint_of};
    use pharmakos_proto::gp::v1::Playbook;
    use pharmakos_proto::json;

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
        let leaf = |value: usize| {
            format!(r#"{{"segment_elapsed":{{"ms":{{"op":"EQ","value":{value}}}}}}}"#)
        };
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
}
