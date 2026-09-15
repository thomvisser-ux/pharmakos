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
use crate::report::{Builder, Diag, patch_remove, patch_replace};

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

/// What the codec says when a member names a field the message does not have.
///
/// The classification below is coupled to this wording on purpose and the
/// coupling is asserted by `the_codec_still_says_what_this_crate_reads` in
/// `tests/verifier.rs`: if `crates/proto` rewords it, that test goes red in the
/// pull request that reworded it, rather than `E0002` quietly becoming `E0001`.
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
            if error.message.contains(UNKNOWN_FIELD_MARKER) {
                let name = pointer::last_token(&error.pointer).unwrap_or_default();
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
                patch_replace("/schema_version", &schema_version_json(schema_version())),
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
        patch_replace("/kind", &Json::String("PLAYBOOK".to_owned())),
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
            patch_replace("/meta/author_kind", &Json::String("HUMAN".to_owned())),
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
    let matches = hash::plan_fingerprint(playbook)
        .is_some_and(|digest| meta.fingerprint == digest.to_be_bytes());
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
