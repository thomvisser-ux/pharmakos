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
//! for. A file with no canonical form keeps a narrower promise, by the rule
//! below: rewording a comment before the byte the grammar refuses, at its
//! length, does not move its `report_hash`; rewording one after that byte
//! does, and so does any change of length, since every offset is the file's.
//!
//! **A file with no canonical form is handed over as it stands.** That is a
//! file that does not parse, and equally a well-formed file the schema refuses
//! — an unknown field, two oneof arms set, a value that does not fit its
//! field. There is no canonical form of either, and this crate must not invent
//! a diagnostic of its own: the verifier owns the catalogue. So the codec's
//! one decode hands its typed refusal to the verifier (`Input::refused`),
//! which names it — `E0001` with a `/byte/<offset>` pointer for a syntax
//! error, the shape decisions-log item 96 (2) settled for exactly this case —
//! without reading the text a second time.
//!
//! **What such a report's `report_hash` covers is one rule**: the file's
//! comments blanked up to the byte the grammar refuses it at, and everything
//! from that byte on exactly as written ([`refused_bytes`]). Past a refusal
//! there is no lexical structure to trust, so nothing there is read as a
//! comment. A file the schema refuses is JSON throughout, so all of its
//! comments are blanked.
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
/// of the report. [`verify_jsonc_canonical`] answers the same report with the
/// playbook it was taken over.
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
    verify_jsonc_canonical(playbook_jsonc, snapshot, scope, rules, depth).map(Verified::into_report)
}

/// What [`verify_jsonc_canonical`] answers: the report, and the one decode it
/// was taken over when the file has a canonical form (S3's `vfyq2`, fix pass
/// 1, item 3).
///
/// A caller that goes on to act on a qualifying file (the gateway's
/// `submit_plan` compiles, prices and seals it) acts on
/// [`Verified::Canonical`]'s `canonical`, so the playbook it acts on is the
/// one the report was taken over, by construction, and the file is decoded
/// once from the socket to the seal.
#[derive(Clone, PartialEq, Debug)]
pub enum Verified {
    /// The file has a canonical form: the report taken over it, and it.
    Canonical {
        /// The verifier's report.
        report: VerifyReport,
        /// The canonical form the report's bytes are, with the message they
        /// were decoded to.
        canonical: Box<json::Canonicalised<Playbook>>,
    },
    /// The file has no canonical form, or is past the cap: a report that
    /// never qualifies, and nothing to act on.
    Refused {
        /// The verifier's report.
        report: VerifyReport,
    },
}

impl Verified {
    /// The report, either way.
    #[must_use]
    pub const fn report(&self) -> &VerifyReport {
        match self {
            Self::Canonical { report, .. } | Self::Refused { report } => report,
        }
    }

    /// The report, either way, by value.
    #[must_use]
    pub fn into_report(self) -> VerifyReport {
        match self {
            Self::Canonical { report, .. } | Self::Refused { report } => report,
        }
    }
}

/// [`verify_jsonc`], answering the canonical form beside the report.
///
/// # Errors
///
/// As [`verify_jsonc`].
pub fn verify_jsonc_canonical(
    playbook_jsonc: &str,
    snapshot: &[u8],
    scope: &Scope,
    rules: &RulesTable,
    depth: Depth,
) -> Result<Verified, Error> {
    // The cap is on the text the player wrote, here where it enters, and
    // never on the canonical text below, which runs to several times its
    // length (`pharmakos_verifier::MAX_PLAYBOOK_CHARS`). A text past it is not
    // canonicalised, blanked or copied: it is `E0029`, at once.
    if pharmakos_verifier::is_too_long(playbook_jsonc) {
        let input = Input::too_long(snapshot, scope, rules).map_err(Error::from)?;
        return Ok(Verified::Refused {
            report: pharmakos_verifier::verify(&input, depth),
        });
    }
    // The one decode's answer goes to the verifier whole: a canonical form
    // with the message it was decoded to, or the refusal with the tree it
    // kept, so the verifier hashes the bytes and reads nothing a second time.
    Ok(match hand_over(playbook_jsonc)? {
        HandedOver::Canonical(canonical) => {
            let input =
                Input::canonical(&canonical, snapshot, scope, rules).map_err(Error::from)?;
            let report = pharmakos_verifier::verify(&input, depth);
            Verified::Canonical { report, canonical }
        }
        HandedOver::Refused { bytes, refused } => {
            let input = Input::refused(bytes.as_bytes(), &refused, snapshot, scope, rules)
                .map_err(Error::from)?;
            Verified::Refused {
                report: pharmakos_verifier::verify(&input, depth),
            }
        }
    })
}

/// A JSONC playbook file's canonical form, decoded once: the decode
/// [`verify_jsonc`] takes its report over, for a reader that needs the
/// playbook and not a report (the gateway's `render_plan`, `resolve_refs`,
/// its compile of a file it holds) and no comment (S3's `vfyq2`, fix pass 1,
/// item 3). [`crate::canonical::canonicalise_text`] reads the same file to the
/// same message and keeps its comments, for a reader that writes the file
/// back.
///
/// # Errors
///
/// A text past `pharmakos_verifier::MAX_PLAYBOOK_CHARS` is refused by
/// [`crate::error::Bound::Size`]; a text with no canonical form is refused at
/// the codec's pointer, in its words, which are the JSONC reader's.
pub fn decode_jsonc(playbook_jsonc: &str) -> Result<json::Canonicalised<Playbook>, Error> {
    if pharmakos_verifier::is_too_long(playbook_jsonc) {
        return Err(Error::bounded(
            crate::error::Bound::Size,
            "",
            format!(
                "the playbook is past {} characters",
                pharmakos_verifier::MAX_PLAYBOOK_CHARS
            ),
        ));
    }
    json::canonical_decode::<Playbook>(&blank_comments(playbook_jsonc))
        .map_err(|refused| Error::from(refused.error().clone()))
}

/// The bytes [`verify_jsonc`] hands the verifier, and which they are.
#[derive(Clone, PartialEq, Debug)]
enum HandedOver {
    /// The file's canonical JSON, comments discarded, with its message.
    Canonical(Box<json::Canonicalised<Playbook>>),
    /// A file with no canonical form, syntax error or schema refusal alike:
    /// its bytes by the module's rule ([`refused_bytes`]), and the refusal its
    /// one decode found.
    Refused {
        /// The bytes `report_hash` is taken over.
        bytes: String,
        /// Why it has no canonical form.
        refused: Box<json::Refused>,
    },
}

/// The bytes the verifier is handed for a player's text (S3's `vfyq2`): the
/// comments blanked ([`blank_comments`]), the blanked text decoded through the
/// codec once ([`json::canonical_decode`], which reads it in one pass when it
/// can), and the canonical form written from the message; or, when it has
/// none, the text by [`refused_bytes`]'s rule with the refusal.
///
/// **It answers what [`crate::canonical::canonicalise_text`] answers**, and
/// nothing is sent another way. The JSONC reader and the codec's reader are
/// one grammar (`pharmakos_proto::json::grammar`), the one reading its
/// comments as trivia and the other reading their blanks as whitespace, and
/// both recognise a comment by one scanner ([`crate::jsonc`]'s
/// `comment_end`). So a text one accepts the other accepts, as the same tree,
/// and a text one refuses the other refuses at the same byte in the same
/// words. The `differential` tests (`the_readers_agree_*`) hold the two to the
/// same bytes over every committed file, their mutations, the edge cases and
/// arbitrary text, with no exception.
fn hand_over(text: &str) -> Result<HandedOver, Error> {
    let blanked = blank_comments(text);
    Ok(match json::canonical_decode::<Playbook>(&blanked) {
        Ok(canonical) => HandedOver::Canonical(Box::new(canonical)),
        Err(refused) => HandedOver::Refused {
            bytes: refused_bytes(text, &blanked, refused.at())?,
            refused: Box::new(refused),
        },
    })
}

/// The bytes a text with no canonical form is hashed as: **its comments
/// blanked up to the byte the grammar refuses it at, and everything from
/// that byte on as written** (S3's `vfyq2`, fix pass 1, item 10). Past a
/// refusal there is no lexical structure to trust, so nothing there is read
/// as a comment; a text the schema refuses is JSON throughout, so all of its
/// comments are blanked. Every byte offset is the file's, and rewording a
/// comment before the refusal, at its length, does not move the report.
fn refused_bytes(text: &str, blanked: &str, at: Option<usize>) -> Result<String, Error> {
    let Some(at) = at else {
        return Ok(blanked.to_owned());
    };
    // `at` is a byte the reader stopped at in the blanked text: never inside a
    // blanked comment (its spaces are whitespace the reader passes, and its
    // first byte, the comment's `/`, is where a refusal inside one lands), so
    // it is a character boundary of both texts.
    match (blanked.get(..at), text.get(at..)) {
        (Some(head), Some(tail)) => {
            let mut bytes = String::with_capacity(text.len());
            bytes.push_str(head);
            bytes.push_str(tail);
            Ok(bytes)
        }
        // The reader never stops past the end or inside a character: this
        // would be the codec and this module disagreeing, said as such.
        _ => Err(Error::at(
            "",
            format!(
                "the codec refused this text at byte {at}, which is not a character boundary \
                 of it: this build cannot say what to verify"
            ),
        )),
    }
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

    /// A line break inside a blanked comment stays a line break, so a line
    /// number the verifier reports is the line in the file the author wrote.
    #[test]
    fn blanking_keeps_every_line_break_inside_a_comment() {
        let text = "{ /* one\ntwo\r\nthree */ \"a\" // x\n: 1 }";
        let blanked = blank_comments(text);
        assert_eq!(blanked.len(), text.len());
        let breaks = |of: &str| -> Vec<(usize, char)> {
            of.char_indices()
                .filter(|(_, character)| *character == '\n' || *character == '\r')
                .collect()
        };
        assert_eq!(breaks(&blanked), breaks(text));
        assert_eq!(blanked, "{       \n   \r\n         \"a\"     \n: 1 }");
    }

    /// `/*/` does not close itself: a block comment closes at the first `*/`
    /// that starts after its `/*`.
    #[test]
    fn a_block_comment_does_not_close_on_its_own_slash() {
        assert_eq!(blank_comments("/*/ x */1"), "        1");
        let unclosed = "1 /*/";
        assert_eq!(blank_comments(unclosed), unclosed);
    }

    /// A text past `MAX_PLAYBOOK_CHARS` is `E0029` and nothing else: it is not
    /// blanked, decoded or hashed.
    #[test]
    fn a_text_past_the_cap_is_refused_whole() {
        let past = format!(
            "{MINIMAL}{}",
            " ".repeat(pharmakos_verifier::MAX_PLAYBOOK_CHARS)
        );
        let report = verify_jsonc(&past, &[], &scope(), &rules(), Depth::Quick).expect("a report");
        let codes: Vec<&str> = report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect();
        assert_eq!(codes, ["E0029"]);
        assert!(!report.qualifies);
        assert!(report.plan_fingerprint.is_empty());
    }

    /// What a refused text's `report_hash` covers (S3's `vfyq2`, fix pass 1,
    /// item 10, the correctness review's 97-byte case, one key renamed at its
    /// length for `tests/confinement.rs`'s word list): its comments blanked
    /// up to the byte the grammar refuses, and everything from there exactly
    /// as written. `main` blanked every comment of such a text, past the
    /// refusal too, so a `// c` after it did not move the hash.
    #[test]
    fn a_refused_text_is_hashed_blanked_up_to_its_refusal_and_verbatim_after() {
        const TEXT: &str = "// header\n{\"status\":{ \"phase\":\n\"LULL\",\"clock_remaining_ms\" : // c\n90000 // c\n,\"roun\0d\" // c\n:1 }}";
        const HASHED: &str = "         \n{\"status\":{ \"phase\":\n\"LULL\",\"clock_remaining_ms\" :     \n90000     \n,\"roun\0d\" // c\n:1 }}";
        assert_eq!(TEXT.len(), 97);
        assert_eq!(HASHED.len(), TEXT.len());
        let report_of = |text: &str| {
            verify_jsonc(text, &[], &scope(), &rules(), Depth::Quick).expect("a report")
        };
        let report = report_of(TEXT);
        assert!(!report.qualifies);
        let rules = rules();
        let scope = scope();
        let input = pharmakos_verifier::Input::new(HASHED.as_bytes(), &[], &scope, &rules)
            .expect("an input");
        let expected = pharmakos_verifier::verify(&input, Depth::Quick);
        assert_eq!(report.report_hash, expected.report_hash);
        assert_eq!(report.diagnostics, expected.diagnostics);
        // Rewording a comment before the refusal does not move the report;
        // rewording one after it does, because past a refusal nothing is read
        // as a comment. (A comment's length is kept, as every byte offset is
        // the file's, so a longer one moves the report wherever it is.)
        let before = TEXT.replace("// header", "// HEADER");
        assert_eq!(report_of(&before).report_hash, report.report_hash);
        let after = TEXT.replace("\"roun\0d\" // c", "\"roun\0d\" // d");
        assert_ne!(report_of(&after).report_hash, report.report_hash);
    }

    #[test]
    fn quick_and_full_are_both_reachable() {
        let quick = verify_jsonc(MINIMAL, &[], &scope(), &rules(), Depth::Quick).expect("a report");
        let full = verify_jsonc(MINIMAL, &[], &scope(), &rules(), Depth::Full).expect("a report");
        assert_ne!(quick.report_hash, full.report_hash);
    }
}
