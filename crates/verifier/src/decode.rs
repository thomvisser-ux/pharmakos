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

use pharmakos_proto::gp::api::v1::patch_suggestion::Applicability;
use pharmakos_proto::gp::v1::meta::AuthorKind;
use pharmakos_proto::gp::v1::playbook::Kind;
use pharmakos_proto::gp::v1::{Playbook, SchemaVersion};
use pharmakos_proto::json::{self, Json};
use pharmakos_proto::{SCHEMA_VERSION, schema_version};

use crate::hash;
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
    // SurveySettings: probe directions.
    "probe_directions",
];

/// What the codec says when a member names a field the message does not have,
/// with the member's own name in front of it: ``"`{name}` is not a field of
/// `{message}`. …"``.
///
/// The classification below is coupled to this wording on purpose and the
/// coupling is asserted by `the_codec_still_says_what_this_crate_reads` in
/// `tests/verifier.rs`: if `crates/proto` rewords it, that test goes red in the
/// pull request that reworded it, rather than `E0002` quietly becoming `E0001`.
///
/// The match is **anchored to the pointer** rather than searched for anywhere in
/// the text, and that is not fussiness. The codec's other messages quote author
/// text back: an enum value it does not know is reported as ``"`{value}` is not
/// a value of `{enum}`"``, so a file carrying `"author_kind": "is not a field"`
/// would match a floating substring and be reported as an unknown field named
/// `author_kind` — a real, declared field, with a patch offering to delete it.
/// Requiring the codec's sentence to *begin* with the pointer's own last token
/// cannot be provoked that way: the only string that still matches is a field
/// literally called `is not a field`, which no `.proto` can declare.
const UNKNOWN_FIELD_MARKER: &str = "is not a field";

/// Decode, or say why not.
///
/// Returns `None` when there is no playbook to hand the later stages. A version,
/// tag or fingerprint problem is reported **and** the playbook is returned: the
/// author gets the rest of the report in the same pass rather than one
/// diagnostic at a time.
pub(crate) fn run(bytes: &[u8], out: &mut Builder) -> Option<Playbook> {
    let text = match core::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => {
            out.emit(
                Diag::new("E0001", pointer::ROOT)
                    .arg("detail", format!("the bytes are not UTF-8 ({error})")),
            );
            return None;
        }
    };

    let value = match json::read(text) {
        Ok(value) => value,
        Err(error) => {
            out.emit(Diag::new("E0001", error.pointer.clone()).arg("detail", &error.message));
            return None;
        }
    };

    // Reserved names first, so a held-back construct is named as one rather than
    // reported as an unknown field by the codec that reaches it first.
    let mut reserved: Vec<(String, String)> = Vec::new();
    scan_reserved(&value, pointer::ROOT, &mut reserved);
    if !reserved.is_empty() {
        for (path, name) in &reserved {
            out.emit(Diag::new("E0003", path.clone()).arg("field", name).fix(
                format!("Delete `{name}`"),
                patch_remove(path),
                Applicability::MaybeIncorrect,
            ));
        }
        return None;
    }

    let playbook: Playbook = match json::decode_json(&value) {
        Ok(playbook) => playbook,
        Err(error) => {
            let name = pointer::last_token(&error.pointer).unwrap_or_default();
            if names_an_unknown_field(&error.message, &name) {
                out.emit(
                    Diag::new("E0002", error.pointer.clone())
                        .arg("field", &name)
                        .fix(
                            format!("Delete `{name}`"),
                            patch_remove(&error.pointer),
                            Applicability::MaybeIncorrect,
                        ),
                );
            } else {
                out.emit(Diag::new("E0001", error.pointer.clone()).arg("detail", &error.message));
            }
            return None;
        }
    };

    check_version(&playbook, out);
    check_kind(&playbook, out);
    check_author(&playbook, out);
    check_fingerprint(&playbook, out);
    Some(playbook)
}

/// Whether the codec's message is the "no such field" one, for the member the
/// error's pointer names.
///
/// See [`UNKNOWN_FIELD_MARKER`] for why this is anchored rather than searched.
fn names_an_unknown_field(message: &str, name: &str) -> bool {
    let mut opening = String::with_capacity(name.len().saturating_add(32));
    opening.push('`');
    opening.push_str(name);
    opening.push_str("` ");
    opening.push_str(UNKNOWN_FIELD_MARKER);
    opening.push_str(" of `");
    message.starts_with(&opening)
}

/// Every reserved name anywhere in the document, with the pointer to it.
fn scan_reserved(value: &Json, at: &str, found: &mut Vec<(String, String)>) {
    match value {
        Json::Object(entries) => {
            for (key, item) in entries {
                let here = pointer::child(at, key);
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
    let found = playbook
        .schema_version
        .unwrap_or(SchemaVersion { major: 0, minor: 0 });
    if found.major == major && found.minor <= minor {
        return;
    }
    out.emit(
        Diag::new("E0004", "/schema_version")
            .arg("found", format!("{}.{}", found.major, found.minor))
            .arg("supported", format!("{major}.{minor}"))
            .fix(
                format!("Set the schema version to {major}.{minor}"),
                patch_add("/schema_version", &schema_version_json(schema_version())),
                Applicability::MaybeIncorrect,
            ),
    );
}

/// `kind` is the envelope's kind tag and an unset one is an error, never a
/// default (decisions-log item 76).
fn check_kind(playbook: &Playbook, out: &mut Builder) {
    if playbook.kind() != Kind::Unspecified {
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
    match meta.author_kind() {
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
/// author's stale copy of a stamp the verifier owns.
fn check_fingerprint(playbook: &Playbook, out: &mut Builder) {
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
        .zip(hash::plan_fingerprint(playbook))
        .is_some_and(|(found, computed)| found == computed);
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
    use super::{RESERVED_NAMES, names_an_unknown_field};

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
    fn an_enum_value_that_quotes_the_marker_is_not_read_as_an_unknown_field() {
        // `crates/proto` reports an unknown enum value as "`{value}` is not a
        // value of `{enum}`", quoting author text. A file carrying
        // `"author_kind": "is not a field"` therefore produces a message with
        // this module's marker inside it — and must still be `E0001`, not an
        // `E0002` naming a real, declared field as unknown.
        assert!(names_an_unknown_field(
            "`wombat` is not a field of `gp.v1.Meta`. Unknown fields are rejected, never stripped.",
            "wombat",
        ));
        assert!(!names_an_unknown_field(
            "`is not a field` is not a value of `gp.v1.Meta.AuthorKind`",
            "author_kind",
        ));
        assert!(!names_an_unknown_field(
            "`WOMBAT` is not a value of `gp.v1.Meta.AuthorKind`",
            "author_kind",
        ));
    }
}
