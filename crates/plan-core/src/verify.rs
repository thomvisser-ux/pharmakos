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
use pharmakos_proto::gp::v1::Playbook;
use pharmakos_proto::json::{self, grammar, work};
use pharmakos_sim::rules::RulesTable;
use pharmakos_verifier::{Input, Scope};

use crate::error::Error;
use crate::jsonc::comment_end;

/// Verifies a JSONC playbook file.
///
/// `snapshot` is the seat's frozen planning snapshot as postcard bytes, and
/// `scope` is the seat's view of it — both opaque here and both hashed inputs
/// of the report.
///
/// # Errors
///
/// When the input cannot be assembled ([`pharmakos_verifier::InputError`]):
/// the rules table does not carry a block the checks read, or carries one out
/// of range, or an input is past `report_hash`'s length prefix. Anything
/// wrong with the *playbook* comes back as a diagnostic inside the report,
/// which is the difference between a verifier and a parser.
pub fn verify_jsonc(
    playbook_jsonc: &str,
    snapshot: &[u8],
    scope: &Scope,
    rules: &RulesTable,
    depth: Depth,
) -> Result<VerifyReport, Error> {
    // The cap is on the text the player wrote, here where it enters, and
    // never on the canonical text below, which runs to several times its
    // length (`pharmakos_verifier::MAX_PLAYBOOK_CHARS`). A text past it is not
    // canonicalised, blanked or copied: it is `E0029`, at once.
    if pharmakos_verifier::is_too_long(playbook_jsonc) {
        let input = Input::too_long(snapshot, scope, rules).map_err(Error::from)?;
        return Ok(pharmakos_verifier::verify(&input, depth));
    }
    let handed = hand_over(playbook_jsonc);
    // A canonical form is handed over with the message it was decoded to, so
    // the verifier hashes its text and does not decode it again.
    let input = match &handed {
        HandedOver::Canonical(canonical) => Input::canonical(canonical, snapshot, scope, rules),
        HandedOver::Blanked(text) => Input::new(text.as_bytes(), snapshot, scope, rules),
    }
    .map_err(Error::from)?;
    Ok(pharmakos_verifier::verify(&input, depth))
}

/// The bytes [`verify_jsonc`] hands the verifier, and which they are.
#[derive(Clone, PartialEq, Debug)]
enum HandedOver {
    /// The file's canonical JSON, comments discarded, with its message.
    Canonical(Box<json::Canonicalised<Playbook>>),
    /// The file as written with its comments blanked ([`blank_comments`]): it
    /// has no canonical form. Any such file, syntax error or schema refusal
    /// alike: see the module doc for why the comment-invariance promise is
    /// conditional on this arm.
    Blanked(String),
}

/// The bytes the verifier is handed for a player's text (S3's `vfyq2`): the
/// comments blanked ([`blank_comments`]), the blanked text decoded through the
/// codec ([`json::decode_encoded`], which reads it in one pass when it can),
/// and the canonical form written from the message; or the blanked text when
/// it has no canonical form.
///
/// **It answers what [`crate::canonical::canonicalise_text`]'s `json` answers**, and nothing is
/// sent another way. The JSONC reader and the codec's reader are one grammar
/// (`pharmakos_proto::json::grammar`), the one reading its comments as trivia
/// and the other reading their blanks as whitespace, and both recognise a
/// comment by one scanner ([`crate::jsonc`]'s `comment_end`). So a text one
/// accepts the other accepts, as the same tree, and a text one refuses the
/// other refuses at the same byte in the same words.
/// The `differential` tests (`the_readers_agree_*`) hold the two
/// to the same bytes over every committed file, their mutations, the edge
/// cases and arbitrary bytes, with no exception.
fn hand_over(text: &str) -> HandedOver {
    let blanked = blank_comments(text);
    match lean_canonical(&blanked) {
        Ok(canonical) => HandedOver::Canonical(Box::new(canonical)),
        Err(_no_canonical_form) => HandedOver::Blanked(blanked),
    }
}

/// The canonical JSON of a JSON text with no comments, with its message, as
/// [`crate::canonical::canonicalise_text`] writes its `json`.
fn lean_canonical(json_text: &str) -> Result<json::Canonicalised<Playbook>, json::Error> {
    json::canonical_decode::<Playbook>(json_text)
}

/// The file with every closed `//` and `/* */` comment blanked to spaces,
/// line breaks kept, so every byte offset is the offset in `text`.
///
/// A comment is what [`crate::jsonc`]'s `comment_end` says is one, outside a
/// string, where the string is what the grammar reads as one: the JSONC
/// reader's own rules, so a comment this blanks is one that reader reads as
/// trivia. A block comment that never closes is not one, and its `/` is left
/// in place for the codec to refuse at that byte, as the JSONC reader does; so
/// is the rest of a text once a string in it breaks the grammar, where both
/// readers stop. A comment's bytes are replaced one for one, so a multi-byte
/// character inside one becomes as many spaces as it had bytes and the result
/// is still UTF-8.
fn blank_comments(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    // Up to where the current comment is blanked, and up to where the current
    // string, or the rest of a text whose string broke the grammar, is copied.
    let mut blank_until = 0usize;
    let mut copy_until = 0usize;
    for (at, character) in text.char_indices() {
        if at < blank_until {
            if character == '\n' || character == '\r' {
                out.push(character);
            } else {
                for _ in 0..character.len_utf8() {
                    out.push(' ');
                }
            }
            continue;
        }
        if at < copy_until {
            out.push(character);
            continue;
        }
        match character {
            '"' => {
                copy_until = match grammar::string(text, at, None) {
                    Ok((end, _)) => end,
                    Err(_breaks_the_grammar) => text.len(),
                };
                out.push(character);
            }
            '/' => {
                // A `/` that opens no closed comment: the grammar refuses the
                // text at or before it, so nothing after it is read as code or
                // comment, and the blanker stops looking: one scan to the end,
                // never one for each `/*` after it.
                if let Some((end, _)) = comment_end(bytes, at) {
                    blank_until = end;
                    out.push(' ');
                } else {
                    copy_until = text.len();
                    out.push(character);
                }
            }
            other => out.push(other),
        }
    }
    work::add(text.len());
    out
}

#[cfg(test)]
mod differential;
#[cfg(test)]
mod linearity;

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
