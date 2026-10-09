// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The editor's state as the Godot types GDScript reads, and the map menu's target as the
//! Rust type the editor takes.
//!
//! Marshalling only, kept out of `bridge.rs` so the `#[func]`s there stay three lines each:
//! take Godot's types apart, call [`crate::editor`], put Godot's types back together.

use godot::builtin::{
    GString, PackedInt64Array, PackedStringArray, VarArray, VarDictionary, Variant, Vector3i,
};
use godot::meta::ToGodot;

use pharmakos_proto::gp::api::v1::VerifyReport;
use pharmakos_proto::json;

use crate::editor::{Editor, Row, Selector, Target, rows_of};
use crate::error::BridgeError;
use crate::rig::{Meter, Recap};
use crate::targeting::{Chip, ChipFeature, FeatureKind, FeatureName};
use crate::wizard::{Instance, Wizard};

/// The menu's target, from the dictionary GDScript builds: `voxel` (a `Vector3i` in the
/// sim's axes), or `selector` (`nearest`, `weakest`, `safest`, `most_threatened`), or
/// `feature` (a feature's name, `vent_<x>_<y>`), or `nearest_uncovered` (a feature kind,
/// `vent` or `seam`: Alt-click's "the nearest vent you can cover"), or `beacon` (a
/// `b_NN` id). `None` when it names none of them.
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
    if let Some(feature) = named("feature") {
        return Some(Target::Feature(feature));
    }
    if let Some(kind) = named("nearest_uncovered") {
        return FeatureKind::from_name(&kind).map(Target::NearestUncovered);
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

    let mut templates = VarArray::new();
    for template in editor.templates() {
        let mut one = VarDictionary::new();
        one.set(&"id".to_variant(), &template.id.to_variant());
        one.set(&"title".to_variant(), &template.title.to_variant());
        one.set(&"summary".to_variant(), &template.summary.to_variant());
        templates.push(&one.to_variant());
    }
    put("templates", templates.to_variant());
    put("templates_known", editor.templates_known().to_variant());
    put("wizard", wizard_dictionary(editor.wizard()).to_variant());
    let mut prose = PackedStringArray::new();
    for line in editor.prose() {
        prose.push(line.as_str());
    }
    put("prose", prose.to_variant());
    put("prose_current", editor.prose_current().to_variant());

    let mut chips = VarArray::new();
    for chip in editor.chips() {
        chips.push(&chip_dictionary(chip).to_variant());
    }
    put("chips", chips.to_variant());
    put("chips_current", editor.chips_current().to_variant());
    put(
        "this_round",
        editor.this_round().unwrap_or_default().to_variant(),
    );
    let dormant = editor.dormant();
    let mut option = VarDictionary::new();
    option.set(&"shown".to_variant(), &dormant.shown.to_variant());
    option.set(&"checked".to_variant(), &dormant.checked.to_variant());
    option.set(&"enabled".to_variant(), &dormant.enabled.to_variant());
    put("dormant", option.to_variant());
    state
}

/// One chip as a dictionary: `pointer`, `step` (from 1; 0 for a reference outside the
/// route), `arm` (`covering`, `on`, `other`), `form` (`name`, `description`,
/// `covered`, `unknown`), `now` and `next` (each empty, or a feature: `id`, `kind`, `x`,
/// `y` as the name spells them, and `travel_ms` as the gateway answered), `failure` and
/// `matched`.
fn chip_dictionary(chip: &Chip) -> VarDictionary {
    let mut out = VarDictionary::new();
    out.set(&"pointer".to_variant(), &chip.pointer.to_variant());
    out.set(&"step".to_variant(), &count(chip.step.unwrap_or(0)));
    out.set(&"arm".to_variant(), &chip.arm.name().to_variant());
    out.set(&"form".to_variant(), &chip.form.name().to_variant());
    out.set(
        &"now".to_variant(),
        &chip_feature(chip.now.as_ref()).to_variant(),
    );
    out.set(
        &"next".to_variant(),
        &chip_feature(chip.next.as_ref()).to_variant(),
    );
    out.set(&"failure".to_variant(), &chip.failure.to_variant());
    out.set(
        &"matched".to_variant(),
        &i64::from(chip.matched).to_variant(),
    );
    out
}

/// One chip candidate as a dictionary; empty for none.
fn chip_feature(feature: Option<&ChipFeature>) -> VarDictionary {
    let mut out = VarDictionary::new();
    let Some(feature) = feature else {
        return out;
    };
    out.set(&"id".to_variant(), &feature.id.to_variant());
    name_into(&mut out, feature.name.as_ref());
    out.set(
        &"travel_ms".to_variant(),
        &i64::from(feature.travel_ms).to_variant(),
    );
    out
}

/// A feature name's parts, `kind`, `x` and `y` as the name spells them, put into `out`;
/// `kind` is empty for a name this build cannot spell, which the panel shows as its id.
pub(crate) fn name_into(out: &mut VarDictionary, name: Option<&FeatureName>) {
    let (kind, x, y) = name.map_or(("", "", ""), |name| {
        (name.kind.name(), name.x.as_str(), name.y.as_str())
    });
    out.set(&"kind".to_variant(), &kind.to_variant());
    out.set(&"x".to_variant(), &x.to_variant());
    out.set(&"y".to_variant(), &y.to_variant());
}

/// The recap as a dictionary: `answers` (zero before the first), `round` and `prose`, as
/// the gateway wrote it.
#[must_use]
pub(crate) fn recap_dictionary(recap: &Recap) -> VarDictionary {
    let mut out = VarDictionary::new();
    out.set(&"answers".to_variant(), &counter(recap.answers));
    out.set(&"round".to_variant(), &i64::from(recap.round).to_variant());
    out.set(&"prose".to_variant(), &recap.prose.to_variant());
    out
}

/// The wizard as a dictionary: empty when it is closed; otherwise `template_id`, `current`
/// (the pages answer the newest ask, so Use may be pressed), `answered` (an answer has come),
/// `refusal` (the gateway's, as it came; empty when none), and the instance's `pages`, `why`
/// and `playbook_jsonc` ([`instance_dictionary`]) once one has come.
#[must_use]
pub(crate) fn wizard_dictionary(wizard: Option<&Wizard>) -> VarDictionary {
    let Some(wizard) = wizard else {
        return VarDictionary::new();
    };
    let mut out = wizard
        .instance
        .as_ref()
        .map(instance_dictionary)
        .unwrap_or_default();
    out.set(
        &"template_id".to_variant(),
        &wizard.template_id.to_variant(),
    );
    out.set(&"current".to_variant(), &wizard.current().to_variant());
    out.set(
        &"answered".to_variant(),
        &wizard.instance.is_some().to_variant(),
    );
    out.set(
        &"refusal".to_variant(),
        &wizard.refusal.clone().unwrap_or_default().to_variant(),
    );
    out
}

/// One `instantiate_template` answer: `pages` (an array of `{pointer, label, value,
/// suggested}`, in the gateway's order), `why` and `playbook_jsonc`, each as it came.
#[must_use]
pub(crate) fn instance_dictionary(instance: &Instance) -> VarDictionary {
    let mut out = VarDictionary::new();
    let mut pages = VarArray::new();
    for page in &instance.pages {
        let mut one = VarDictionary::new();
        one.set(&"pointer".to_variant(), &page.pointer.to_variant());
        one.set(&"label".to_variant(), &page.label.to_variant());
        one.set(&"value".to_variant(), &page.value.to_variant());
        one.set(&"suggested".to_variant(), &page.suggested.to_variant());
        pages.push(&one.to_variant());
    }
    out.set(&"pages".to_variant(), &pages.to_variant());
    out.set(&"why".to_variant(), &instance.why.to_variant());
    out.set(
        &"playbook_jsonc".to_variant(),
        &instance.playbook_jsonc.to_variant(),
    );
    out
}

/// The meter as a dictionary: `answers` (zero before the first), `phase` (the phase the
/// last answer was served in), the four fields under their wire names, as they came, and
/// `bmi_next_dollars` and `committed_dollars` as they came, each with a `has_` flag that
/// is false when the answer left the field out (which is not 0).
#[must_use]
pub(crate) fn meter_dictionary(meter: Meter) -> VarDictionary {
    let mut out = VarDictionary::new();
    out.set(&"answers".to_variant(), &counter(meter.answers));
    out.set(&"phase".to_variant(), &meter.phase.name().to_variant());
    out.set(
        &"treasury_now".to_variant(),
        &i64::from(meter.treasury_now).to_variant(),
    );
    out.set(
        &"supply_kw_now".to_variant(),
        &i64::from(meter.supply_kw_now).to_variant(),
    );
    out.set(
        &"draw_kw_now".to_variant(),
        &i64::from(meter.draw_kw_now).to_variant(),
    );
    out.set(
        &"headroom_kw_now".to_variant(),
        &i64::from(meter.headroom_kw_now).to_variant(),
    );
    for (key, value) in [
        ("bmi_next_dollars", meter.bmi_next_dollars),
        ("committed_dollars", meter.committed_dollars),
    ] {
        out.set(
            &format!("has_{key}").to_variant(),
            &value.is_some().to_variant(),
        );
        out.set(&key.to_variant(), &value.map_or(0, i64::from).to_variant());
    }
    out
}

fn counter(value: u64) -> Variant {
    i64::try_from(value).unwrap_or(i64::MAX).to_variant()
}

fn count(value: usize) -> Variant {
    i64::try_from(value).unwrap_or(i64::MAX).to_variant()
}
