// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The pipeline from a JSONC file to a verifier report.
//!
//! `plan-core` runs in process with the Seat Gateway and is the layer between
//! the **file** and the **verifier**: the verifier takes bytes and hashes what
//! it decodes (`pharmakos_verifier::Input::new`), so somebody has to decide
//! *which* bytes. This module does, and the decision is worth stating:
//!
//! **The verifier sees the canonical form.** A file's comments and formatting
//! are the author's, not the plan's, so two files that differ only in their
//! comments produce the same `report_hash` — a player can annotate a playbook
//! without moving its hash, and the pre-check at edit time and the check at
//! submit compare, which is what spec section 11's "byte-identical" promise is
//! for. That promise is **conditional on there being a canonical form**: for a
//! file the paragraph below covers there is none, and adding a comment to such
//! a file does move its `report_hash`. Such a report never qualifies, and the
//! pre-check and submit take the same path, so the two still agree with each
//! other.
//!
//! **A file with no canonical form is handed over as it stands.** That is a
//! file that does not parse, and equally a well-formed file the schema refuses
//! — an unknown field, two oneof arms set, a value that does not fit its
//! field. There is no canonical form of either, and this crate must not invent
//! a diagnostic of its own: the verifier owns the catalogue. So the raw bytes
//! go through and come back as a diagnostic — `E0001` with a `/byte/<offset>`
//! pointer for a syntax error, which is the shape decisions-log item 96 (2)
//! settled for exactly this case.
//!
//! **Its comments are blanked first, byte for byte.** The verifier reads
//! JSON, not JSONC, so a commented file handed over raw failed at its first
//! comment -- `E0001` at `/byte/0` -- and a commented file carrying an
//! out-of-vocabulary construct never reached the decode that names the
//! construct with its code and JSON Pointer (decisions-log item 112 (3), found
//! by the editor). Every byte of a comment becomes a space and every line
//! break stays where it was, so a byte offset the verifier reports is the
//! same offset in the file the author wrote, and a commented file gets the
//! same code and pointer as the same file without its comments.

use pharmakos_proto::gp::api::v1::VerifyReport;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_sim::rules::RulesTable;
use pharmakos_verifier::{Input, Scope};

use crate::canonical::canonicalise_text;
use crate::error::Error;

/// Verifies a JSONC playbook file.
///
/// `snapshot` is the seat's frozen planning snapshot as postcard bytes, and
/// `scope` is the seat's view of it — both opaque here and both hashed inputs
/// of the report.
///
/// # Errors
///
/// Only when the rules table does not carry a block the checks read. Anything
/// wrong with the *playbook* comes back as a diagnostic inside the report,
/// which is the difference between a verifier and a parser.
pub fn verify_jsonc(
    playbook_jsonc: &str,
    snapshot: &[u8],
    scope: &Scope,
    rules: &RulesTable,
    depth: Depth,
) -> Result<VerifyReport, Error> {
    let canonical = canonicalise_text(playbook_jsonc).map(|canonical| canonical.json);
    let blanked: String;
    let bytes: &[u8] = match canonical.as_ref() {
        Ok(json) => json.as_bytes(),
        // Any file with no canonical form, syntax error or schema refusal
        // alike: see the module doc for why the comment-invariance promise is
        // conditional on this branch not being taken.
        Err(_no_canonical_form) => {
            blanked = blank_comments(playbook_jsonc);
            blanked.as_bytes()
        }
    };
    let input = Input::new(bytes, snapshot, scope, rules)
        .map_err(|gap| Error::at("", format!("the rules table is incomplete: {gap}")))?;
    Ok(pharmakos_verifier::verify(&input, depth))
}

/// The file with every `//` and `/* */` comment blanked to spaces, line
/// breaks kept, so every byte offset is the offset in `text`.
///
/// A comment is recognised outside a string only, the same rule the JSONC
/// layer reads comments by; a `//` inside a string value is text. An
/// unterminated block comment is blanked to the end of the file, which leaves
/// the verifier to report whatever the file then lacks. A comment's bytes are
/// replaced one for one, so a multi-byte character inside one becomes as many
/// spaces as it had bytes and the result is still UTF-8.
fn blank_comments(text: &str) -> String {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum At {
        Code,
        Text,
        Escape,
        Line,
        Block,
    }
    let mut out: Vec<u8> = Vec::with_capacity(text.len());
    let mut at = At::Code;
    let mut bytes = text.bytes().peekable();
    while let Some(byte) = bytes.next() {
        match at {
            At::Code => {
                if byte == b'/' && bytes.peek() == Some(&b'/') {
                    let _ = bytes.next();
                    out.extend_from_slice(b"  ");
                    at = At::Line;
                } else if byte == b'/' && bytes.peek() == Some(&b'*') {
                    let _ = bytes.next();
                    out.extend_from_slice(b"  ");
                    at = At::Block;
                } else {
                    if byte == b'"' {
                        at = At::Text;
                    }
                    out.push(byte);
                }
            }
            At::Text => {
                if byte == b'\\' {
                    at = At::Escape;
                } else if byte == b'"' {
                    at = At::Code;
                }
                out.push(byte);
            }
            At::Escape => {
                at = At::Text;
                out.push(byte);
            }
            At::Line => {
                if byte == b'\n' {
                    at = At::Code;
                    out.push(byte);
                } else if byte == b'\r' {
                    out.push(byte);
                } else {
                    out.push(b' ');
                }
            }
            At::Block => {
                if byte == b'*' && bytes.peek() == Some(&b'/') {
                    let _ = bytes.next();
                    out.extend_from_slice(b"  ");
                    at = At::Code;
                } else if byte == b'\n' || byte == b'\r' {
                    out.push(byte);
                } else {
                    out.push(b' ');
                }
            }
        }
    }
    // Only ASCII bytes were replaced, each by an ASCII space, and only whole
    // characters were inside a comment, so this cannot fail; the fallback is
    // the text as written, which is what this branch handed over before.
    String::from_utf8(out).unwrap_or_else(|_| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{blank_comments, verify_jsonc};
    use pharmakos_proto::gp::api::v1::verify_plan::Depth;
    use pharmakos_sim::rules::RulesTable;
    use pharmakos_verifier::Scope;

    const MINIMAL: &str = concat!(
        "{\"schema_version\":{\"major\":1},",
        "\"meta\":{\"title\":\"t\",\"author_kind\":\"HUMAN\"},",
        "\"declarative\":{\"route\":[{\"label\":\"a\",\"hold\":{\"ms\":1000}}]},",
        "\"on_death\":{\"on_respawn\":\"CONTINUE\"},",
        "\"fallback\":{\"hold\":{\"at\":{\"beacon_anchor\":{\"safest\":{}}}}},",
        "\"kind\":\"PLAYBOOK\"}"
    );

    fn rules() -> RulesTable {
        RulesTable::load(std::path::Path::new("../../rules/rules.v1.json"))
            .expect("the committed rules table")
    }

    fn scope() -> Scope {
        Scope::new(
            pharmakos_sim::tables::SeatId::new(0),
            pharmakos_sim::knowledge::SeatEconomy {
                treasury: pharmakos_sim::math::quantity::Money::new(200),
                supply: pharmakos_sim::math::quantity::Kw::new(10),
                draw: pharmakos_sim::math::quantity::Kw::new(2),
            },
        )
    }

    #[test]
    fn a_file_qualifies_through_the_canonical_form() {
        let report = verify_jsonc(MINIMAL, &[], &scope(), &rules(), Depth::Full)
            .expect("the rules table is complete");
        assert!(report.qualifies, "{:?}", report.diagnostics);
    }

    #[test]
    fn comments_do_not_move_the_report_hash() {
        let plain = verify_jsonc(MINIMAL, &[], &scope(), &rules(), Depth::Full).expect("a report");
        let noisy = verify_jsonc(
            &format!("// a note\n{MINIMAL}\n"),
            &[],
            &scope(),
            &rules(),
            Depth::Full,
        )
        .expect("a report");
        assert_eq!(plain.report_hash, noisy.report_hash);
        assert_eq!(plain.plan_fingerprint, noisy.plan_fingerprint);
    }

    #[test]
    fn a_file_that_is_not_json_comes_back_as_a_diagnostic_at_a_byte() {
        let report = verify_jsonc("{ this is not json", &[], &scope(), &rules(), Depth::Quick)
            .expect("a report");
        assert!(!report.qualifies);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.path.starts_with("/byte/")),
            "{:?}",
            report.diagnostics
        );
    }

    /// Decisions-log item 112 (3): a comment used to hide the construct
    /// behind `E0001` at `/byte/0`, because the verifier cannot read one.
    #[test]
    fn a_commented_out_of_vocabulary_construct_gets_the_same_code_and_pointer_as_a_plain_one() {
        let plain = MINIMAL.replace(
            "{\"label\":\"a\",\"hold\":{\"ms\":1000}}",
            "{\"label\":\"a\",\"hold\":{\"ms\":1000},\"set_flag\":{\"name\":\"x\"}}",
        );
        assert_ne!(plain, MINIMAL, "the construct went in");
        let commented = format!(
            "// A route that sets a flag, which v1 does not have.\n{}\n/* the end */\n",
            plain.replace("\"declarative\"", "/* the plan */ \"declarative\"")
        );
        let of = |text: &str| {
            let report =
                verify_jsonc(text, &[], &scope(), &rules(), Depth::Full).expect("a report");
            assert!(!report.qualifies);
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.clone(), diagnostic.path.clone()))
                .collect::<Vec<(String, String)>>()
        };
        let expected = of(&plain);
        assert!(
            !expected.is_empty() && expected.iter().all(|(_, path)| !path.starts_with("/byte/")),
            "the plain file is named by its construct: {expected:?}"
        );
        assert_eq!(of(&commented), expected);
    }

    #[test]
    fn blanking_keeps_every_byte_offset_and_every_string() {
        let text = "{\"a\": \"// not a comment\", /* \u{e9} */ \"b\": 1 // end\n}\r\n";
        let blanked = blank_comments(text);
        assert_eq!(blanked.len(), text.len());
        assert_eq!(
            blanked,
            "{\"a\": \"// not a comment\",          \"b\": 1       \n}\r\n"
        );
    }

    #[test]
    fn quick_and_full_are_both_reachable() {
        let quick = verify_jsonc(MINIMAL, &[], &scope(), &rules(), Depth::Quick).expect("a report");
        let full = verify_jsonc(MINIMAL, &[], &scope(), &rules(), Depth::Full).expect("a report");
        assert_ne!(quick.report_hash, full.report_hash);
    }
}
