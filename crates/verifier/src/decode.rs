// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stage 1, **decode**: the bytes to a `gp.v1.Playbook`, strictly, at a
//! supported version.
//!
//! # Load never strips
//!
//! Spec section 10 is unambiguous, and it is the rule this stage exists to keep:
//! an out-of-vocabulary construct is **rejected with a code and a JSON
//! Pointer**, never silently dropped, on Load as well as on submit — so an
//! edited file either loads as written or says why it did not. Three codes carry
//! that promise:
//!
//! * `E0002` — a field the schema does not declare;
//! * `E0003` — a field name the schema **reserves**: `set_flag`, `clear_flag`,
//!   `branch`, `repeat`, the flag predicates, `dispatch`, `team_id` and the rest
//!   of the numbers held for later versions. The more specific code exists
//!   because "this word comes back in v1.1" is a different thing to tell an
//!   author than "there is no such word";
//! * `E0001` — anything else the codec refused: JSON that is not JSON, a value
//!   that does not fit its field, two arms of one choice set at once.
//!
//! Each carries the codec's own JSON Pointer, and each offers deleting the
//! offending member as a *suggestion* — the author's decision, in the editor,
//! rather than something the verifier did quietly on the way past.
//!
//! # One `path` that is not a node pointer
//!
//! `gp.api.v1.Diagnostic.path` is documented as "RFC 6901 JSON Pointer into the
//! playbook", and every diagnostic this crate raises after the document parses
//! is exactly that. `E0001` is the exception, and only in its **lexical** form:
//! when the text is not JSON at all there is no tree to point into, so
//! `crates/proto`'s reader reports the failure at a byte offset and spells it
//! `/byte/583`. That locator is forwarded verbatim rather than rewritten to the
//! root, because "column 583" is the useful answer and the root is not.
//!
//! An editor therefore branches on the shape: a `path` whose first token is
//! `byte` is an offset into the bytes it sent, and anything else is a node
//! pointer. `E0001` raised *after* the parse — a value that does not fit its
//! field, two arms of one choice set — carries an ordinary pointer like every
//! other code. `tests/golden/verifier/README.md` says the same thing for the
//! reader of the goldens.
//!
//! # The bytes are the bytes
//!
//! This stage decodes exactly the bytes `report_hash` is taken over. JSONC
//! comments are stripped by `plan-core` before the gateway calls the verifier
//! (decisions-log item 74, skeleton plan T8); nothing here accepts a comment,
//! and nothing here rewrites the input.

use std::borrow::Cow;

use pharmakos_proto::gp::api::v1::patch_suggestion::Applicability;
use pharmakos_proto::gp::v1::meta::AuthorKind;
use pharmakos_proto::gp::v1::playbook::Kind;
use pharmakos_proto::gp::v1::{Playbook, SchemaVersion};
use pharmakos_proto::json::grammar::quoted;
use pharmakos_proto::json::{self, Json};
use pharmakos_proto::{SCHEMA_VERSION, schema_version};

use crate::pointer;
use crate::report::{Builder, Diag, patch_add, patch_remove, patch_replace};

/// Field names `gp.v1` reserves, in the order the `.proto` files reserve them.
///
/// A name here is out of the v1 vocabulary **wherever it appears**, which is why
/// the scan is by name over the whole document rather than per message: a
/// reserved word in the wrong place is still a reserved word, and the author
/// wants to be told which word rather than which message did not declare it.
/// The codec would reject every one of these as an unknown field anyway; this
/// list is what turns that into the precise answer.
const RESERVED_NAMES: &[&str] = &[
    // Playbook: player Dispatches, back in v1.2 with the live conduit.
    "dispatch",
    // Meta: teams and more than three seats.
    "team_id",
    // Options: there is no reflex_hp_pct; the 20% reflex is fixed.
    "reflex_hp_pct",
    // Step: the vocabulary held back to v1.1 with the expert editor view.
    "set_flag",
    "clear_flag",
    "branch",
    "repeat",
    // Step: roadmap.
    "reassign",
    "logistics",
    "capture",
    // Condition: the flag predicates, held back with set_flag/clear_flag.
    "flag_set",
    "flag_clear",
    // Condition: radio predicates, roadmap.
    "link_up",
    "jammed",
    "satellite_pass",
    // Condition: grid islands, roadmap.
    "grid_island",
    // MandateSettings: the rest of the Common row.
    "report_kinds",
    "accept_broadcasts",
    // BuildSettings: the drone fields of the Build row.
    "repair_drones",
    "reclaim_drones",
    // DefendSettings: auto-fortify and unit mix, deferred to S3's rule list
    // (S2's plan's decision 13).
    "auto_fortify",
    "unit_mix",
    // AttackLaunch: the broadcast go-code, at S4 with the Radio Mast.
    "broadcast_received",
    // SurveySettings: probe directions.
    "probe_directions",
];

/// What the caller handed the decode stage beside the bytes (S3's `vfyq2`):
/// nothing, so it decodes them; the message decoded from exactly them; or the
/// refusal the one decode of them found.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Handed<'a> {
    /// The bytes alone ([`crate::Input::new`]).
    Bytes,
    /// Their message ([`crate::Input::canonical`]).
    Decoded(&'a json::Decoded<Playbook>),
    /// Their refusal ([`crate::Input::refused`]).
    Refused(&'a json::Refused),
}

/// What the decode stage hands the later stages: the playbook, and its plan
/// fingerprint, computed once here from the bytes it was decoded from.
pub(crate) struct Decoded<'a> {
    /// The playbook: the caller's, when it handed one in already decoded
    /// ([`crate::Input::canonical`]), or the one decoded here.
    pub(crate) playbook: Cow<'a, Playbook>,
    /// Its plan fingerprint, or why the codec could not write the canonical
    /// text it is taken over ([`pharmakos_sim::hash::plan_fingerprint_of`]):
    /// an engine fault, reported as `E0030` ([`check_fingerprint`]) and never
    /// swallowed into "no fingerprint" (S3's `fpstream`, part 2, fix pass 2).
    pub(crate) fingerprint: Result<u64, pharmakos_sim::hash::FingerprintError>,
}

/// Decode, or say why not.
///
/// Returns `None` when there is no playbook to hand the later stages. A version,
/// tag or fingerprint problem is reported **and** the playbook is returned: the
/// author gets the rest of the report in the same pass rather than one
/// diagnostic at a time.
///
/// # Two paths, one answer (S3's `vfyq`, decisions-log item 158)
///
/// A playbook the codec accepts is read in one pass, straight to its wire
/// bytes ([`json::decode_direct`]), and its fingerprint is written from those
/// bytes ([`pharmakos_sim::hash::plan_fingerprint_of`]) rather than by encoding
/// it again. Every other document takes the diagnostic path below, which reads
/// the tree, scans it for reserved names and decodes it, and reports what it
/// finds exactly as before.
///
/// The reserved-name scan is not skipped for the first path, it is answered:
/// a document the codec accepts has every member resolved to a field its
/// message declares, and no message in the schema declares a reserved name
/// (`no_message_declares_a_reserved_name` below), so the scan would find
/// nothing.
pub(crate) fn run<'a>(bytes: &[u8], handed: Handed<'a>, out: &mut Builder) -> Option<Decoded<'a>> {
    let handed = match handed {
        Handed::Bytes => None,
        Handed::Decoded(decoded) => Some(decoded),
        Handed::Refused(refused) => {
            diagnose_refused(refused, out);
            return None;
        }
    };
    if let Some(decoded) = handed {
        // Decoded from exactly `bytes` by the codec (`Input::canonical`):
        // what the first path below would answer, without reading them again.
        let fingerprint = pharmakos_sim::hash::plan_fingerprint_of(decoded);
        return Some(checked(
            Decoded {
                playbook: Cow::Borrowed(decoded.message()),
                fingerprint,
            },
            out,
        ));
    }
    let text = match core::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => {
            // `Utf8Error`'s own words, composed from its numbers.
            let detail = match error.error_len() {
                Some(length) => pharmakos_proto::said!(
                    "the bytes are not UTF-8 (invalid utf-8 sequence of {} bytes from index {})",
                    length,
                    error.valid_up_to()
                ),
                None => pharmakos_proto::said!(
                    "the bytes are not UTF-8 (incomplete utf-8 byte sequence from index {})",
                    error.valid_up_to()
                ),
            };
            out.emit(Diag::new("E0001", pointer::ROOT).arg("detail", detail));
            return None;
        }
    };

    let decoded = match json::decode_direct::<Playbook>(text) {
        json::Direct::Wire(decoded) => {
            let fingerprint = pharmakos_sim::hash::plan_fingerprint_of(&decoded);
            Decoded {
                playbook: Cow::Owned(decoded.into_message()),
                fingerprint,
            }
        }
        json::Direct::NeedsExactPath(_) => diagnose(text, out)?,
    };
    Some(checked(decoded, out))
}

/// The checks the decode stage makes of every playbook it hands on.
fn checked<'a>(decoded: Decoded<'a>, out: &mut Builder) -> Decoded<'a> {
    check_version(&decoded.playbook, out);
    check_kind(&decoded.playbook, out);
    check_author(&decoded.playbook, out);
    check_fingerprint(&decoded.playbook, &decoded.fingerprint, out);
    crate::enums::run(&decoded.playbook, out);
    decoded
}

/// The diagnostic path: the tree, the reserved names, then the codec's own
/// refusal, each reported as the first that applies.
fn diagnose(text: &str, out: &mut Builder) -> Option<Decoded<'static>> {
    let value = match json::read(text) {
        Ok(value) => value,
        Err(error) => {
            refuse(&error, out);
            return None;
        }
    };
    if named_reserved(&value, out) {
        return None;
    }
    match json::decode_json_encoded::<Playbook>(&value) {
        // The direct reader answers for every document the codec accepts, so
        // this arm is reached only if the two disagree; it answers as the
        // first path does, with the fingerprint written from the wire bytes
        // the tree was encoded to, so there is one fingerprint function.
        Ok(decoded) => {
            let fingerprint = pharmakos_sim::hash::plan_fingerprint_of(&decoded);
            Some(Decoded {
                playbook: Cow::Owned(decoded.into_message()),
                fingerprint,
            })
        }
        Err(error) => {
            refuse(&error, out);
            None
        }
    }
}

/// The diagnostic path for a text whose one decode already refused it
/// ([`crate::Input::refused`]; S3's `vfyq2`): what [`diagnose`] reports for
/// the same bytes, from the refusal and the tree that decode kept, without
/// reading the text again.
fn diagnose_refused(refused: &json::Refused, out: &mut Builder) {
    if let Some(tree) = refused.tree()
        && named_reserved(tree, out)
    {
        return;
    }
    refuse(refused.error(), out);
}

/// Reserved names first, so a held-back construct is named as one rather than
/// reported as an unknown field by the codec that reaches it first: each one
/// `E0003`, and whether there was any.
fn named_reserved(value: &Json, out: &mut Builder) -> bool {
    let mut reserved: Vec<(String, String)> = Vec::new();
    scan_reserved(value, pointer::ROOT, &mut reserved);
    for (path, name) in &reserved {
        out.emit(
            Diag::new("E0003", path.clone())
                .arg_text("field", name)
                .fix(
                    pharmakos_proto::said!("Delete `{}`", quoted(name)),
                    patch_remove(path),
                    Applicability::MaybeIncorrect,
                ),
        );
    }
    !reserved.is_empty()
}

/// The codec's refusal as the catalogue names it. The codec's kind says which
/// (S3's `vfy6`, the review of PR #179): the message is for the player and
/// never read here, so author text quoted back in it (an enum value it does
/// not know) cannot talk this into the wrong code. A pointer with no last
/// token is the document itself, which no unknown field can be: that error is
/// `E0001`'s.
fn refuse(error: &json::Error, out: &mut Builder) {
    let unknown = (error.kind() == json::ErrorKind::UnknownField)
        .then(|| pointer::last_token(error.pointer()))
        .flatten();
    let (path, cut) = bounded(error.pointer());
    if let Some(name) = unknown {
        let diag = Diag::new("E0002", path.clone().into_owned()).arg_text("field", &name);
        // A pointer past the bound addresses the object that holds the
        // member, the deepest place a report can name; a Fix would have to
        // spell the whole member name, so none is offered there, and the
        // message names the member (quoted, cut) and asks the author to take
        // it out, as E0002's beginner sentence says.
        out.emit(if cut {
            diag
        } else {
            diag.fix(
                pharmakos_proto::said!("Delete `{}`", quoted(&name)),
                patch_remove(&path),
                Applicability::MaybeIncorrect,
            )
        });
    } else {
        out.emit(Diag::new("E0001", path.into_owned()).arg("detail", error.said()));
    }
}

/// How many bytes `text` takes as the body of a JSON string: the report's
/// encoding, which escapes `"` and `\` in two bytes and every other control
/// character in at most six.
pub(crate) fn json_bytes(text: &str) -> usize {
    text.chars().fold(0_usize, |total, character| {
        let one = match character {
            '"' | '\\' => 2,
            other if u32::from(other) < 0x20 => 6,
            other => other.len_utf8(),
        };
        total.saturating_add(one)
    })
}

/// `pointer`, or the longest run of its whole tokens whose JSON is within
/// [`crate::report::MAX_POINTER_BYTES`] when it is not, and whether it was
/// cut. A cut pointer addresses an ancestor of the place, never a different
/// place: only a member name of the player's own (an unknown key, never one
/// of the schema's names) makes a pointer that long.
fn bounded(pointer: &str) -> (Cow<'_, str>, bool) {
    if json_bytes(pointer) <= crate::report::MAX_POINTER_BYTES {
        return (Cow::Borrowed(pointer), false);
    }
    let mut kept = String::new();
    let mut bytes = 0_usize;
    // A pointer is empty or starts with `/`, so the piece before the first
    // `/` is empty and skipped.
    for token in pointer.split('/').skip(1) {
        bytes = bytes.saturating_add(json_bytes(token)).saturating_add(1);
        if bytes > crate::report::MAX_POINTER_BYTES {
            break;
        }
        kept.push('/');
        kept.push_str(token);
    }
    (Cow::Owned(kept), true)
}

/// Every reserved name anywhere in the document, with the pointer to it, as
/// far as a report can point: a member whose pointer is past
/// [`crate::report::MAX_POINTER_BYTES`] is not descended into, because only
/// a member name of the player's own makes a pointer that long and no
/// schema name sits beneath one. The document is then refused by the codec
/// at the first member it does not know, which is never stripped either.
fn scan_reserved(value: &Json, at: &str, found: &mut Vec<(String, String)>) {
    match value {
        Json::Object(entries) => {
            for (key, item) in entries {
                let here = pointer::child(at, key);
                if json_bytes(&here) > crate::report::MAX_POINTER_BYTES {
                    continue;
                }
                if RESERVED_NAMES.contains(&key.as_str()) {
                    found.push((here.clone(), key.clone()));
                }
                scan_reserved(item, &here, found);
            }
        }
        Json::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                scan_reserved(item, &pointer::at(at, index), found);
            }
        }
        _ => {}
    }
}

/// "It decodes strictly at a supported schema version" (spec section 11).
///
/// The major version must be this build's. A **lower** minor is fine — minor
/// versions only add optional fields, so an older file is still a valid newer
/// one. A **higher** minor is refused here rather than left to fail as a crowd
/// of unknown fields, because "this file was written for a later version" is the
/// useful sentence and "`probe_directions` is not a field" is not.
fn check_version(playbook: &Playbook, out: &mut Builder) {
    let (major, minor) = SCHEMA_VERSION;
    // A file that names no version is said to name none, never read as 0.0.
    let found = match playbook.schema_version {
        Some(SchemaVersion {
            major: found_major,
            minor: found_minor,
        }) => {
            if found_major == major && found_minor <= minor {
                return;
            }
            pharmakos_proto::said!("{}.{}", found_major, found_minor)
        }
        None => pharmakos_proto::said!("missing"),
    };
    out.emit(
        Diag::new("E0004", "/schema_version")
            .arg("found", found)
            .arg("supported", pharmakos_proto::said!("{}.{}", major, minor))
            .fix(
                pharmakos_proto::said!("Set the schema version to {}.{}", major, minor),
                patch_add("/schema_version", &schema_version_json(schema_version())),
                Applicability::MaybeIncorrect,
            ),
    );
}

/// `kind` is the envelope's kind tag and an unset one is an error, never a
/// default (decisions-log item 76).
///
/// Read raw: a number `Kind` does not name is `E0018`'s, not an unset tag.
fn check_kind(playbook: &Playbook, out: &mut Builder) {
    if !matches!(Kind::try_from(playbook.kind), Ok(Kind::Unspecified)) {
        return;
    }
    out.emit(Diag::new("E0005", "/kind").fix(
        "Mark the file as a PLAYBOOK",
        // `add`, not `replace`: an unset `kind` is written by omitting the
        // member, so there is usually nothing at `/kind` to replace.
        patch_add("/kind", &Json::String("PLAYBOOK".to_owned())),
        Applicability::MaybeIncorrect,
    ));
}

/// `author_kind` is the other tag on the envelope. `SCRIPT` is a reserved value
/// that v1.1 fills in and v1 rejects (spec section 10, "Versioning and reserved
/// seams"); unset is an error like any other unset tag.
fn check_author(playbook: &Playbook, out: &mut Builder) {
    let Some(meta) = playbook.meta.as_ref() else {
        // A missing `meta` block is `E0101`, which the structure stage raises.
        return;
    };
    // Read raw: a number `AuthorKind` does not name is `E0018`'s.
    let Ok(author) = AuthorKind::try_from(meta.author_kind) else {
        return;
    };
    match author {
        AuthorKind::Script => out.emit(Diag::new("E0006", "/meta/author_kind").fix(
            "Mark the file as written by a HUMAN",
            patch_replace("/meta/author_kind", &Json::String("HUMAN".to_owned())),
            Applicability::MaybeIncorrect,
        )),
        AuthorKind::Unspecified => out.emit(Diag::new("E0007", "/meta/author_kind").fix(
            "Mark the file as written by a HUMAN",
            // `add`: unset means the member is absent. `E0006` above keeps
            // `replace`, because `SCRIPT` proves it is there.
            patch_add("/meta/author_kind", &Json::String("HUMAN".to_owned())),
            Applicability::MaybeIncorrect,
        )),
        AuthorKind::Human | AuthorKind::Builtin => {}
    }
}

/// "A file whose fingerprint does not match its bytes is a diagnostic, never a
/// silent fix" (`proto/gp/v1/playbook.proto`).
///
/// A warning rather than an error: the verifier writes the right fingerprint
/// into the report regardless, so the file is still a playbook — it is the
/// author's stale copy of a stamp the verifier owns. `computed` is that
/// fingerprint, which the decode stage computes once for the check and the
/// report alike.
///
/// When the codec could not write the canonical text the fingerprint is taken
/// over, there is no fingerprint to judge a stamp by, and saying the stamp is
/// stale (`E0008`) would blame the player's file for the verifier's own
/// fault. So that is `E0030`, an error about the engine and not the file,
/// whether or not the file carries a stamp: a report with no fingerprint
/// never qualifies, so nothing is sealed without one (S3's `fpstream`,
/// part 2, fix pass 2).
fn check_fingerprint(
    playbook: &Playbook,
    computed: &Result<u64, pharmakos_sim::hash::FingerprintError>,
    out: &mut Builder,
) {
    let Ok(computed) = computed else {
        out.emit(Diag::new("E0030", pointer::ROOT));
        return;
    };
    let Some(meta) = playbook.meta.as_ref() else {
        return;
    };
    if meta.fingerprint.is_empty() {
        return;
    }
    // `from_field` rather than a comparison of our own: `crates/proto` owns the
    // byte order (item 77), so the rule has one home, and a field of the wrong
    // length comes back as `None` there rather than needing a length check here.
    let matches = pharmakos_proto::fingerprint::from_field(&meta.fingerprint)
        .is_some_and(|found| found == *computed);
    if matches {
        return;
    }
    out.emit(Diag::new("E0008", "/meta/fingerprint").fix(
        "Remove the stale fingerprint",
        patch_remove("/meta/fingerprint"),
        Applicability::MachineApplicable,
    ));
}

/// A `SchemaVersion` as the canonical JSON writes it, for a patch value.
///
/// A zero is omitted, exactly as the mapping omits a default — so the
/// suggestion produces the same bytes the canonical writer would.
fn schema_version_json(version: SchemaVersion) -> Json {
    let mut entries: Vec<(String, Json)> = Vec::new();
    if version.major != 0 {
        entries.push(("major".to_owned(), crate::report::number(version.major)));
    }
    if version.minor != 0 {
        entries.push(("minor".to_owned(), crate::report::number(version.minor)));
    }
    Json::Object(entries)
}

#[cfg(test)]
mod tests {
    use super::RESERVED_NAMES;
    use pharmakos_proto::json::{self, ErrorKind};

    /// A fingerprint the codec could not write is `E0030`, an error about the
    /// verifier, and never `E0008`, which would tell the author to remove a
    /// stamp that may be right: with a stamp and without one (S3's
    /// `fpstream`, part 2, fix pass 2). The fault cannot be reached through a
    /// file, because the codec can write every playbook it decodes, so the
    /// check is held here with the error made by hand.
    #[test]
    fn a_fingerprint_the_codec_could_not_write_is_e0030_and_never_e0008() {
        let fault = || {
            Err(pharmakos_sim::hash::FingerprintError::Canonical(
                json::decode::<pharmakos_proto::gp::v1::Playbook>("{").expect_err("not JSON"),
            ))
        };
        let stamped = pharmakos_proto::gp::v1::Playbook {
            meta: Some(pharmakos_proto::gp::v1::Meta {
                fingerprint: pharmakos_proto::fingerprint::to_field(7),
                ..pharmakos_proto::gp::v1::Meta::default()
            }),
            ..pharmakos_proto::gp::v1::Playbook::default()
        };
        for playbook in [stamped, pharmakos_proto::gp::v1::Playbook::default()] {
            let mut out = crate::report::Builder::default();
            super::check_fingerprint(&playbook, &fault(), &mut out);
            assert!(out.has_errors(), "an engine fault is an error");
            let codes: Vec<String> = out
                .into_diagnostics()
                .into_iter()
                .map(|diagnostic| diagnostic.code)
                .collect();
            assert_eq!(codes, ["E0030"]);
        }
        // And a stamp that matches is left alone; one that does not is `E0008`.
        let matching = pharmakos_proto::gp::v1::Playbook {
            meta: Some(pharmakos_proto::gp::v1::Meta {
                fingerprint: pharmakos_proto::fingerprint::to_field(7),
                ..pharmakos_proto::gp::v1::Meta::default()
            }),
            ..pharmakos_proto::gp::v1::Playbook::default()
        };
        for (computed, expected) in [(7_u64, Vec::<&str>::new()), (8, vec!["E0008"])] {
            let mut out = crate::report::Builder::default();
            super::check_fingerprint(&matching, &Ok(computed), &mut out);
            let codes: Vec<String> = out
                .into_diagnostics()
                .into_iter()
                .map(|diagnostic| diagnostic.code)
                .collect();
            assert_eq!(codes, expected, "computed {computed}");
        }
    }

    /// What lets the decode stage's direct path skip the reserved-name scan: a
    /// document the codec accepts has every member resolved to a declared
    /// field, so if no message declares a reserved name, by either spelling,
    /// the scan cannot find one in it. A schema change that declares one turns
    /// this red, and the direct path must then scan as well.
    #[test]
    fn no_message_declares_a_reserved_name() {
        for message in pharmakos_proto::descriptor::schema().messages() {
            for field in &message.fields {
                for name in [&field.name, &field.json_name] {
                    assert!(
                        !RESERVED_NAMES.contains(&name.as_str()),
                        "`{}` declares `{name}`, a name `RESERVED_NAMES` holds",
                        message.full_name
                    );
                }
            }
        }
    }

    /// Every field name `gp.v1` reserves, read out of the committed `.proto`
    /// text.
    ///
    /// Field names are `lower_snake_case` and enum values are
    /// `UPPER_SNAKE_CASE` — `buf lint`'s own rule, which CI runs — so the two
    /// kinds of reservation are told apart by case. Only field names can appear
    /// as a member of a hand-written playbook, which is what the scan in this
    /// module is for.
    fn reserved_in_the_schema() -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        for file in ["playbook.proto", "rules.proto", "seams.proto"] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("proto")
                .join("gp")
                .join("v1")
                .join(file);
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
            for line in text.lines() {
                let trimmed = line.trim_start();
                let Some(rest) = trimmed.strip_prefix("reserved ") else {
                    continue;
                };
                for piece in rest.split('"').skip(1).step_by(2) {
                    if piece.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
                        found.push(piece.to_owned());
                    }
                }
            }
        }
        found.sort();
        found.dedup();
        found
    }

    #[test]
    fn the_held_back_list_is_every_name_the_schema_reserves() {
        // The list in this module is hand-ordered, so that `E0003`'s reasons
        // read in schema order. What must hold is that it is the *same set* the
        // `.proto` files reserve: a reservation added by a later schema pull
        // request would otherwise fall through to `E0002` — "there is no such
        // word" — when the true answer is "that word comes back in v1.1".
        let mut ours: Vec<String> = RESERVED_NAMES.iter().map(|&name| name.to_owned()).collect();
        ours.sort();
        ours.dedup();
        assert_eq!(
            ours,
            reserved_in_the_schema(),
            "`RESERVED_NAMES` and the reservations in `proto/gp/v1/**` have drifted apart; add \
             the new name here, with the comment saying which version brings it back"
        );
    }

    #[test]
    fn an_unknown_field_is_told_by_its_kind_not_its_words() {
        // `crates/proto` reports an unknown enum value as "`{value}` is not a
        // value of `{enum}`", quoting author text, so a file carrying
        // `"author_kind": "is not a field"` has the old marker in its message.
        // Its kind is still `Value`, and only `UnknownField` is `E0002`.
        let unknown = json::decode::<pharmakos_proto::gp::v1::Meta>(r#"{"wombat":1}"#)
            .expect_err("an unknown field is refused");
        assert_eq!(unknown.kind(), ErrorKind::UnknownField);
        let quoted =
            json::decode::<pharmakos_proto::gp::v1::Meta>(r#"{"author_kind":"is not a field"}"#)
                .expect_err("an unknown enum value is refused");
        assert_eq!(quoted.kind(), ErrorKind::Value);
        assert!(
            quoted.message().contains("is not a field"),
            "{}",
            quoted.message()
        );
    }
}
