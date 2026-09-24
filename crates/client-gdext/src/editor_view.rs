// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The editor's state as the Godot types GDScript reads, and the map menu's target as the
//! Rust type the editor takes.
//!
//! Marshalling only, kept out of `bridge.rs` so the `#[func]`s there stay three lines each:
//! take Godot's types apart, call [`crate::editor`], put Godot's types back together.

use godot::builtin::{GString, PackedInt64Array, VarArray, VarDictionary, Variant, Vector3i};
use godot::meta::ToGodot;

use pharmakos_proto::gp::api::v1::VerifyReport;
use pharmakos_proto::json;

use crate::editor::{Editor, Row, Selector, Target, rows_of};
use crate::error::BridgeError;

/// The menu's target, from the dictionary GDScript builds: `voxel` (a `Vector3i` in the
/// sim's axes), or `selector` (`nearest`, `weakest`, `safest`, `most_threatened`), or
/// `beacon` (a `b_NN` id). `None` when it names none of the three.
#[must_use]
pub(crate) fn target_of(dictionary: &VarDictionary) -> Option<Target> {
    let entry = |key: &str| dictionary.get(&key.to_variant());
    if let Some(voxel) = entry("voxel").and_then(|value| value.try_to::<Vector3i>().ok()) {
        return Some(Target::Voxel([voxel.x, voxel.y, voxel.z]));
    }
    let named = |key: &str| {
        entry(key)
            .and_then(|value| value.try_to::<GString>().ok())
            .map(|value| value.to_string())
            .filter(|value| !value.is_empty())
    };
    if let Some(selector) = named("selector") {
        return Selector::from_name(&selector).map(Target::Selector);
    }
    named("beacon").map(Target::Beacon)
}

/// Validation rows as an array of dictionaries: `severity`, `code`, `pointer`, `sentence`,
/// `message`, and `fixes` (an array of `{title, index}`, the machine-applicable fixes only).
#[must_use]
pub(crate) fn rows_array(rows: &[Row]) -> VarArray {
    let mut out = VarArray::new();
    for row in rows {
        let mut dictionary = VarDictionary::new();
        dictionary.set(&"severity".to_variant(), &row.severity.name().to_variant());
        dictionary.set(&"code".to_variant(), &row.code.to_variant());
        dictionary.set(&"pointer".to_variant(), &row.pointer.to_variant());
        dictionary.set(&"sentence".to_variant(), &row.sentence.to_variant());
        dictionary.set(&"message".to_variant(), &row.message.to_variant());
        let mut fixes = VarArray::new();
        for (index, fix) in row.fixes.iter().enumerate() {
            let mut one = VarDictionary::new();
            one.set(&"title".to_variant(), &fix.title.to_variant());
            one.set(&"index".to_variant(), &count(index));
            fixes.push(&one.to_variant());
        }
        dictionary.set(&"fixes".to_variant(), &fixes.to_variant());
        out.push(&dictionary.to_variant());
    }
    out
}

/// Rows from a `gp.api.v1.VerifyReport` JSON text, for a scene that draws rows with no
/// gateway behind it (`scenes/rows_shot.tscn`). The text is read against the schema, with
/// enum values in either spelling.
///
/// # Errors
///
/// [`BridgeError::Schema`] when the text is not a `VerifyReport`.
pub(crate) fn rows_of_report_text(text: &str) -> Result<Vec<Row>, BridgeError> {
    let value = crate::enums::canonical("gp.api.v1.VerifyReport", &json::read(text)?);
    let report: VerifyReport = json::decode_json(&value)?;
    Ok(rows_of(&report))
}

/// Everything the editor's panel draws, as one dictionary.
#[must_use]
pub(crate) fn state_dictionary(editor: &Editor) -> VarDictionary {
    let mut state = VarDictionary::new();
    let mut put = |key: &str, value: Variant| state.set(&key.to_variant(), &value);
    put("changes", counter(editor.changes()));
    put("has_text", editor.has_text().to_variant());
    put("revision", counter(editor.revision()));
    put("undo_depth", count(editor.undo_depth()));
    put("busy", editor.busy().to_variant());
    put("settled", editor.settled().to_variant());
    put("pending", editor.has_pending_jobs().to_variant());
    put("rows", rows_array(editor.rows()).to_variant());
    put("verdict", editor.verdict().name().to_variant());
    put("rows_current", editor.rows_current().to_variant());
    put("qualifies", editor.qualifies().to_variant());

    let route = editor.route();
    let mut points = VarArray::new();
    for [x, y, z] in &route.points {
        points.push(&Vector3i::new(*x, *y, *z).to_variant());
    }
    let mut legs = PackedInt64Array::new();
    for leg in &route.legs {
        legs.push(*leg);
    }
    let mut drawn = VarDictionary::new();
    drawn.set(&"points".to_variant(), &points.to_variant());
    drawn.set(&"legs".to_variant(), &legs.to_variant());
    drawn.set(&"whole".to_variant(), &route.whole.to_variant());
    drawn.set(&"reachable".to_variant(), &route.reachable.to_variant());
    drawn.set(&"readable".to_variant(), &route.readable.to_variant());
    drawn.set(&"current".to_variant(), &route.current.to_variant());
    put("route", drawn.to_variant());

    let mut ghost = VarDictionary::new();
    if let Some(shown) = editor.ghost() {
        let [x, y, z] = shown.at;
        ghost.set(&"at".to_variant(), &Vector3i::new(x, y, z).to_variant());
        ghost.set(&"state".to_variant(), &shown.state.name().to_variant());
        ghost.set(&"sentence".to_variant(), &shown.sentence.to_variant());
    }
    put("ghost", ghost.to_variant());

    put("notes", editor.notes().to_variant());
    put("notes_known", editor.notes_known().to_variant());
    put(
        "notes_saved",
        editor.notes_saved().map_or(-1, i64::from).to_variant(),
    );
    let mut drafts = VarArray::new();
    for draft in editor.drafts() {
        let mut one = VarDictionary::new();
        one.set(&"id".to_variant(), &draft.id.to_variant());
        one.set(&"label".to_variant(), &draft.label.to_variant());
        one.set(&"round".to_variant(), &i64::from(draft.round).to_variant());
        drafts.push(&one.to_variant());
    }
    put("drafts", drafts.to_variant());
    put("drafts_known", editor.drafts_known().to_variant());

    let (submitted, accepted) = editor
        .submitted()
        .map_or((false, false), |outcome| (true, outcome.accepted));
    put("submitted", submitted.to_variant());
    put("accepted", accepted.to_variant());
    put("beacon_prose", editor.beacon_prose().to_variant());
    put("status_key", editor.status().key.to_variant());
    put("status_detail", editor.status().detail.to_variant());
    put("refusals", i64::from(editor.refusals()).to_variant());
    state
}

fn counter(value: u64) -> Variant {
    i64::try_from(value).unwrap_or(i64::MAX).to_variant()
}

fn count(value: usize) -> Variant {
    i64::try_from(value).unwrap_or(i64::MAX).to_variant()
}
