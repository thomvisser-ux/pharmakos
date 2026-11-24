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

use crate::editor::{
    Check, Editor, FieldAsk, ReadFailed, Route, Row, RuleEditRefusal, Selector, SizeMeter, Target,
    Unchecked, rows_of,
};
use crate::error::BridgeError;
use crate::rig::{Meter, Recap};
use crate::rule_list::{ChipEditor, NameKind, RuleChip, RuleList};
use crate::targeting::{
    AttackAim, AttackMark, BuildAnchor, BuildMark, Chip, ChipFeature, FeatureKind, FeatureName,
};
use crate::variant::{int_variant, text_or_empty};
use crate::vocabulary::{AnchorShape, BeaconDetail, Entry, ParameterShape, Setting, Vocabulary};
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

/// The route as the panel draws it: the polyline, the legs' times, and what the gateway
/// said about it.
fn route_dictionary(route: &Route) -> VarDictionary {
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
    drawn.set(&"whole_text".to_variant(), &route.whole_text.to_variant());
    let mut texts = PackedStringArray::new();
    for text in &route.leg_texts {
        texts.push(text.as_str());
    }
    drawn.set(&"leg_texts".to_variant(), &texts.to_variant());
    drawn.set(&"reachable".to_variant(), &route.reachable.to_variant());
    drawn.set(&"readable".to_variant(), &route.readable.to_variant());
    drawn.set(&"current".to_variant(), &route.current.to_variant());
    drawn.set(
        &"found_nothing".to_variant(),
        &route.found_nothing.to_variant(),
    );
    drawn
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

    put("route", route_dictionary(editor.route()).to_variant());

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

    let submitted = editor.submitted();
    put("submitted", submitted.is_some().to_variant());
    put(
        "accepted",
        submitted.is_some_and(|done| done.accepted).to_variant(),
    );
    put("beacon_prose", editor.beacon_prose().to_variant());
    put("status_key", editor.status().key.to_variant());
    put("status_detail", editor.status().detail.to_variant());
    put("refusals", i64::from(editor.refusals()).to_variant());

    put(
        "templates",
        templates_array(editor.templates()).to_variant(),
    );
    put("templates_known", editor.templates_known().to_variant());
    let mut templates_read = VarDictionary::new();
    failed_into(&mut templates_read, editor.templates_failed());
    put("templates_read", templates_read.to_variant());
    put("wizard", wizard_dictionary(editor.wizard()).to_variant());
    let mut prose = PackedStringArray::new();
    for line in editor.prose() {
        prose.push(line.as_str());
    }
    put("prose", prose.to_variant());
    put("prose_current", editor.prose_current().to_variant());

    put("chips", chips_array(editor.chips()).to_variant());
    put("chips_current", editor.chips_current().to_variant());
    put(
        "this_round",
        text_or_empty(editor.this_round()).to_variant(),
    );
    put("size", size_dictionary(editor.size_meter()).to_variant());
    put("door", editor.door().name().to_variant());
    // Why each check will not come, by its line: `{"rows": {"key", "detail"}, ...}`, the
    // key "" while the check is on its way (finding K; a refusal is its own check's).
    let mut unchecked = VarDictionary::new();
    for check in Check::ALL {
        unchecked.set(
            &check.name().to_variant(),
            &unchecked_dictionary(editor.unchecked(check)).to_variant(),
        );
    }
    put("unchecked", unchecked.to_variant());
    // Why the template list will not come, in the checks' words (`{key, detail}`).
    put(
        "templates_unchecked",
        unchecked_dictionary(editor.templates_unchecked()).to_variant(),
    );
    put("seal", editor.seal_line().name().to_variant());
    put("filed", editor.filed().to_variant());
    put("file", text_or_empty(editor.file()).to_variant());
    put("last_file", text_or_empty(editor.last_file()).to_variant());
    put("dormant", dormant_dictionary(editor).to_variant());
    put_rule_list(&mut state, editor);
    put_marks(&mut state, editor);
    state
}

/// The map's marks: `attack_targets` ([`attack_array`]) and `build_targets` ([`build_array`]).
fn put_marks(state: &mut VarDictionary, editor: &Editor) {
    state.set(
        &"attack_targets".to_variant(),
        &attack_array(editor.attack_targets()).to_variant(),
    );
    state.set(
        &"build_targets".to_variant(),
        &build_array(editor.build_targets()).to_variant(),
    );
}

/// The E0601 checkbox as the panel draws it: `shown`, `checked`, `enabled` (only the door
/// disables it) and `refused` (why the last click was not taken, by name; "" for none).
fn dormant_dictionary(editor: &Editor) -> VarDictionary {
    let dormant = editor.dormant();
    let mut option = VarDictionary::new();
    option.set(&"shown".to_variant(), &dormant.shown.to_variant());
    option.set(&"checked".to_variant(), &dormant.checked.to_variant());
    option.set(&"enabled".to_variant(), &dormant.enabled.to_variant());
    // Why the last click was refused, by name ("" for none), drawn beside the box.
    let refused = match dormant.refused {
        Some(refusal) => refusal.name(),
        None => "",
    };
    option.set(&"refused".to_variant(), &refused.to_variant());
    option
}

/// The rule list's keys of the editor's state (S3's `ed3b`): `rule_list`
/// ([`rule_list_dictionary`]), `rules_editable`, `rules_blocked` (why it takes no edit, by name; "" when it takes one), `rules_rendering` (the rendering id an editor names back; -1 before one), `picking` (the chip a pick is on for, -1 for
/// none), `conflicts` (each `{earlier, later, sentence}`, rule indices) and `rule_refusal`
/// (empty, or `{rule, key, detail}`: the refusal of the last rule-list edit, the gateway's or
/// this client's, the string table's key for its words, and the rule it is shown on, -1 for
/// none) and `field` (what became of the last value a value field sent).
fn put_rule_list(state: &mut VarDictionary, editor: &Editor) {
    let mut put = |key: &str, value: Variant| state.set(&key.to_variant(), &value);
    put(
        "rule_list",
        rule_list_dictionary(editor.rule_list()).to_variant(),
    );
    put("rules_editable", editor.rules_editable().to_variant());
    // Why the list takes no edit, by its reason's name ("" when it takes one), and the
    // rendering an editor opened now names back (-1 before one).
    // The rendering id is converted as a typed refusal, never saturated: an id GDScript's
    // `int` cannot hold blocks the list (`rendering_out_of_range`), so no control names it back.
    let (rendering, blocked) = match editor.rules_rendering().map(i64::try_from) {
        Some(Ok(rendering)) => (rendering, editor.rules_blocked()),
        Some(Err(_)) => (-1, Some(RuleEditRefusal::RenderingOutOfRange)),
        None => (-1, editor.rules_blocked()),
    };
    let blocked = match blocked {
        Some(refusal) => refusal.name(),
        None => "",
    };
    put("rules_blocked", blocked.to_variant());
    put("rules_rendering", rendering.to_variant());
    put("picking", index_or_none(editor.picking()));
    put("anchoring", editor.anchoring().to_variant());
    let mut labels = PackedStringArray::new();
    for label in editor.name_choices(NameKind::StepLabel) {
        labels.push(label.as_str());
    }
    put("step_labels", labels.to_variant());
    let mut ids = PackedStringArray::new();
    for id in editor.name_choices(NameKind::RuleId) {
        ids.push(id.as_str());
    }
    put("rule_ids", ids.to_variant());
    put(
        "vocabulary",
        vocabulary_dictionary(editor.vocabulary(), editor.vocabulary_failed()).to_variant(),
    );
    put(
        "beacon_detail",
        beacon_detail_dictionary(editor.beacon_detail(), editor.beacon_failed()).to_variant(),
    );
    let mut conflicts = VarArray::new();
    for conflict in editor.conflicts() {
        let mut one = VarDictionary::new();
        one.set(&"earlier".to_variant(), &count(conflict.earlier));
        one.set(&"later".to_variant(), &count(conflict.later));
        one.set(&"sentence".to_variant(), &conflict.sentence.to_variant());
        conflicts.push(&one.to_variant());
    }
    put("conflicts", conflicts.to_variant());
    let mut refusal = VarDictionary::new();
    if let Some(refused) = editor.rule_refusal() {
        let rule = editor.rule_list().rule_holding(&refused.pointer);
        refusal.set(&"rule".to_variant(), &index_or_none(rule));
        refusal.set(&"key".to_variant(), &refused.key.to_variant());
        refusal.set(&"detail".to_variant(), &refused.detail.to_variant());
    }
    put("rule_refusal", refusal.to_variant());
    // What became of the last value a value field sent, for its popup: `state` (`nothing`,
    // `waiting`, `refused`, `landed`), and for a refusal its words' `key` and `detail`.
    let mut field = VarDictionary::new();
    let ask = editor.field_ask();
    field.set(&"state".to_variant(), &ask.name().to_variant());
    let (key, detail) = match ask {
        FieldAsk::Refused { key, detail } => (key.as_str(), detail.as_str()),
        FieldAsk::Nothing | FieldAsk::Waiting | FieldAsk::Landed => ("", ""),
    };
    field.set(&"key".to_variant(), &key.to_variant());
    field.set(&"detail".to_variant(), &detail.to_variant());
    put("field", field.to_variant());
}

/// The rule list as a dictionary (S3's `ed3b`): `lines`, one per line of the prose, each
/// `{segments, rule, in_rule}` (`segments` its runs, `{text, chip}`, with `chip` -1 for plain
/// text; `rule` the rule whose sentence the line is, `in_rule` the rule whose lines hold it,
/// each -1 for none); `rules`, each `{pointer, parent, line, line_count}`; and `chips`
/// ([`rule_chip_dictionary`]).
pub(crate) fn rule_list_dictionary(list: &RuleList) -> VarDictionary {
    let mut lines = VarArray::new();
    for (number, segments) in list.segments.iter().enumerate() {
        let mut runs = VarArray::new();
        for segment in segments {
            let mut run = VarDictionary::new();
            run.set(&"text".to_variant(), &segment.text.to_variant());
            run.set(&"chip".to_variant(), &index_or_none(segment.chip));
            runs.push(&run.to_variant());
        }
        let mut line = VarDictionary::new();
        line.set(&"segments".to_variant(), &runs.to_variant());
        line.set(&"rule".to_variant(), &index_or_none(list.rule_at(number)));
        let holder = list.rules.iter().position(|rule| rule.holds_line(number));
        line.set(&"in_rule".to_variant(), &index_or_none(holder));
        lines.push(&line.to_variant());
    }
    let mut rules = VarArray::new();
    for rule in &list.rules {
        let mut one = VarDictionary::new();
        one.set(&"pointer".to_variant(), &rule.pointer.to_variant());
        one.set(&"parent".to_variant(), &rule.parent().to_variant());
        one.set(&"line".to_variant(), &count(rule.line));
        one.set(&"line_count".to_variant(), &count(rule.line_count));
        rules.push(&one.to_variant());
    }
    let mut chips = VarArray::new();
    for chip in &list.chips {
        chips.push(&rule_chip_dictionary(chip).to_variant());
    }
    let mut slots = VarArray::new();
    for slot in &list.slots {
        let mut one = VarDictionary::new();
        one.set(&"pointer".to_variant(), &slot.pointer.to_variant());
        one.set(&"title".to_variant(), &slot.title.to_variant());
        one.set(&"line".to_variant(), &int_variant(slot.line));
        one.set(&"rule".to_variant(), &index_or_none(slot.rule));
        one.set(&"state".to_variant(), &slot.state.name().to_variant());
        one.set(&"predicate".to_variant(), &slot.predicate.to_variant());
        one.set(&"pick_op".to_variant(), &slot.op.name().to_variant());
        one.set(&"pickable".to_variant(), &slot.pickable().to_variant());
        slots.push(&one.to_variant());
    }
    let mut out = VarDictionary::new();
    out.set(&"lines".to_variant(), &lines.to_variant());
    out.set(&"rules".to_variant(), &rules.to_variant());
    out.set(&"chips".to_variant(), &chips.to_variant());
    out.set(&"slots".to_variant(), &slots.to_variant());
    out
}

/// One rule-list chip as a dictionary: `pointer`, `text` and `value_json` as sent, `outside`
/// (the gateway's word that the value lies outside its shape), `pick_op` (the op its edit sends,
/// `add`, `replace` or `none`), and its editor ([`editor_into`]).
fn rule_chip_dictionary(chip: &RuleChip) -> VarDictionary {
    let mut out = VarDictionary::new();
    out.set(&"pointer".to_variant(), &chip.pointer.to_variant());
    out.set(&"text".to_variant(), &chip.text.to_variant());
    out.set(&"value_json".to_variant(), &chip.value_json.to_variant());
    out.set(&"outside".to_variant(), &chip.outside.to_variant());
    out.set(&"pick_op".to_variant(), &chip.op.name().to_variant());
    editor_into(&mut out, &chip.editor);
    out
}

/// A chip's or a setting's editor into `out`: `editor` is `number` (with `min` and `max`, each
/// beside a `has_` flag that is false when the gateway set no bound, both in `unit`; and
/// `display_unit` and `value_text`, what the field is typed in and pre-fills with, as sent),
/// `choice` (with `values`, the JSON texts, and `titles`, their words, as sent), `pick` (with
/// `kind` and `arms`), `name` (with `kind`) or `read_only` (with `why`).
fn editor_into(out: &mut VarDictionary, editor: &ChipEditor) {
    let named = match editor {
        ChipEditor::Number {
            min,
            max,
            unit,
            display_unit,
            value_text,
        } => {
            for (key, bound) in [("min", min), ("max", max)] {
                out.set(
                    &format!("has_{key}").to_variant(),
                    &bound.is_some().to_variant(),
                );
                if let Some(bound) = bound {
                    out.set(&key.to_variant(), &i64::from(*bound).to_variant());
                }
            }
            out.set(&"unit".to_variant(), &unit.to_variant());
            out.set(&"display_unit".to_variant(), &display_unit.to_variant());
            out.set(&"value_text".to_variant(), &value_text.to_variant());
            "number"
        }
        ChipEditor::Choice { values, titles } => {
            out.set(&"values".to_variant(), &packed(values).to_variant());
            out.set(&"titles".to_variant(), &packed(titles).to_variant());
            "choice"
        }
        ChipEditor::Pick { kind, arms } => {
            out.set(&"kind".to_variant(), &kind.name().to_variant());
            out.set(&"arms".to_variant(), &packed(arms).to_variant());
            "pick"
        }
        ChipEditor::Name(kind) => {
            out.set(&"kind".to_variant(), &kind.name().to_variant());
            "name"
        }
        ChipEditor::ReadOnly(why) => {
            out.set(&"why".to_variant(), &why.name().to_variant());
            "read_only"
        }
    };
    out.set(&"editor".to_variant(), &named.to_variant());
}

/// Texts as GDScript's packed array, in order.
fn packed(texts: &[String]) -> PackedStringArray {
    let mut listed = PackedStringArray::new();
    for text in texts {
        listed.push(text.as_str());
    }
    listed
}

/// The words the pickers draw (decisions-log item 160 (4)): `known` (whether this Lull's answer is on
/// hand), `failed` (`""`, `refused` or `unreadable`) with `detail`, and `families` (each
/// `{family, title, locked, predicates}`), `mandates`, `blueprints` (Add a build target's),
/// `queue_blueprints` (Queue a structure's) and `groups`, every entry [`entry_dictionary`].
/// `locked` is the gateway's sentence, `""` when the entry can be used.
pub(crate) fn vocabulary_dictionary(
    words: Option<&Vocabulary>,
    failed: Option<&ReadFailed>,
) -> VarDictionary {
    let mut out = VarDictionary::new();
    out.set(&"known".to_variant(), &words.is_some().to_variant());
    failed_into(&mut out, failed);
    let mut families = VarArray::new();
    let mut mandates = VarArray::new();
    let mut blueprints = VarArray::new();
    let mut queue_blueprints = VarArray::new();
    let mut groups = VarArray::new();
    if let Some(words) = words {
        for family in &words.families {
            let mut one = VarDictionary::new();
            one.set(&"family".to_variant(), &family.id.to_variant());
            one.set(&"title".to_variant(), &family.title.to_variant());
            one.set(
                &"locked".to_variant(),
                &text_or_empty(family.locked.as_deref()).to_variant(),
            );
            let mut predicates = VarArray::new();
            for entry in &family.predicates {
                predicates.push(&entry_dictionary(entry).to_variant());
            }
            one.set(&"predicates".to_variant(), &predicates.to_variant());
            families.push(&one.to_variant());
        }
        for entry in &words.mandates {
            mandates.push(&entry_dictionary(entry).to_variant());
        }
        for (list, entries) in [
            (&mut blueprints, &words.blueprints),
            (&mut queue_blueprints, &words.queue_blueprints),
            (&mut groups, &words.groups),
        ] {
            for entry in entries {
                list.push(&entry_dictionary(entry).to_variant());
            }
        }
    }
    out.set(&"families".to_variant(), &families.to_variant());
    out.set(&"mandates".to_variant(), &mandates.to_variant());
    out.set(&"blueprints".to_variant(), &blueprints.to_variant());
    out.set(
        &"queue_blueprints".to_variant(),
        &queue_blueprints.to_variant(),
    );
    out.set(&"groups".to_variant(), &groups.to_variant());
    out
}

/// Why something the panel waits for will not come, as `{key, detail}`: `key` is `refused`,
/// `unreadable` or the shut door's name, and `""` while it is on its way.
fn unchecked_dictionary(why: Option<Unchecked>) -> VarDictionary {
    let (key, detail) = match why {
        None => (String::new(), String::new()),
        Some(Unchecked::Refused(said)) => ("refused".to_owned(), said),
        Some(Unchecked::Unreadable(said)) => ("unreadable".to_owned(), said),
        Some(Unchecked::TooLarge(bytes)) => ("too_large".to_owned(), bytes),
        Some(Unchecked::ClosedOn(code)) => ("closed_on".to_owned(), code),
        Some(Unchecked::Closed(door)) => (door.name().to_owned(), String::new()),
    };
    let mut out = VarDictionary::new();
    out.set(&"key".to_variant(), &key.to_variant());
    out.set(&"detail".to_variant(), &detail.to_variant());
    out
}

/// The template list as the panel draws it: each template's `id`, `title` and `summary`, and
/// the guided first pick's mark, `guided`, with the gateway's sentence, `guided_reason`, as
/// sent (S3's plan, decision 5); `guided_reason` is empty on every unmarked template.
pub(crate) fn templates_array(templates: &[crate::wizard::TemplateRow]) -> VarArray {
    let mut out = VarArray::new();
    for template in templates {
        let mut one = VarDictionary::new();
        one.set(&"id".to_variant(), &template.id.to_variant());
        one.set(&"title".to_variant(), &template.title.to_variant());
        one.set(&"summary".to_variant(), &template.summary.to_variant());
        one.set(
            &"guided".to_variant(),
            &template.guided.is_some().to_variant(),
        );
        one.set(
            &"guided_reason".to_variant(),
            &text_or_empty(template.guided.as_deref()).to_variant(),
        );
        out.push(&one.to_variant());
    }
    out
}

/// Why a read did not come, under `failed` (`refused` or `unreadable`, `""` when it came or is
/// on its way) and `detail` (the gateway's words or the decode's reason).
fn failed_into(out: &mut VarDictionary, failed: Option<&ReadFailed>) {
    let (key, detail) = match failed {
        Some(failed) => (failed.name(), failed.detail()),
        None => ("", ""),
    };
    out.set(&"failed".to_variant(), &key.to_variant());
    out.set(&"detail".to_variant(), &detail.to_variant());
}

/// One vocabulary entry: `name`, `title`, `locked` and `anchor_shape` (`point`, `box` or `""`)
/// as sent, and `parameters`, each `{pointer, title, shape}` with `shape` `number` (with
/// `has_min`/`min`, `has_max`/`max`, `unit` and `display_unit`), `choice` (with `values` and
/// `titles`), `reference` (with `kind`) or `unknown`.
fn entry_dictionary(entry: &Entry) -> VarDictionary {
    let mut one = VarDictionary::new();
    one.set(&"name".to_variant(), &entry.name.to_variant());
    one.set(&"title".to_variant(), &entry.title.to_variant());
    one.set(
        &"locked".to_variant(),
        &text_or_empty(entry.locked.as_deref()).to_variant(),
    );
    let anchor = match entry.anchor_shape {
        AnchorShape::Point => "point",
        AnchorShape::Box => "box",
        AnchorShape::Unnamed => "",
    };
    one.set(&"anchor_shape".to_variant(), &anchor.to_variant());
    let mut parameters = VarArray::new();
    for parameter in &entry.parameters {
        let mut drawn = VarDictionary::new();
        drawn.set(&"pointer".to_variant(), &parameter.pointer.to_variant());
        drawn.set(&"title".to_variant(), &parameter.title.to_variant());
        drawn.set(&"shape".to_variant(), &parameter.shape.name().to_variant());
        match &parameter.shape {
            ParameterShape::Number {
                min,
                max,
                unit,
                display_unit,
            } => {
                for (key, bound) in [("min", min), ("max", max)] {
                    drawn.set(
                        &format!("has_{key}").to_variant(),
                        &bound.is_some().to_variant(),
                    );
                    if let Some(bound) = bound {
                        drawn.set(&key.to_variant(), &i64::from(*bound).to_variant());
                    }
                }
                drawn.set(&"unit".to_variant(), &unit.to_variant());
                drawn.set(&"display_unit".to_variant(), &display_unit.to_variant());
            }
            ParameterShape::Choice { values, titles } => {
                drawn.set(&"values".to_variant(), &packed(values).to_variant());
                drawn.set(&"titles".to_variant(), &packed(titles).to_variant());
            }
            ParameterShape::Reference { kind } => {
                drawn.set(&"kind".to_variant(), &kind.to_variant());
            }
            ParameterShape::Unknown => {}
        }
        parameters.push(&drawn.to_variant());
    }
    one.set(&"parameters".to_variant(), &parameters.to_variant());
    one
}

/// The detail of the beacon last described (decisions-log item 160 (4)): `known`, `failed` with
/// `detail` as for the vocabulary, `id`, `mandate` (as the wire spells it, `""` for none),
/// `settings` (each a chip-shaped dictionary, [`setting_dictionary`]) and `targets` (each
/// `{blueprint_id, line}`, the line the gateway's own words for the target).
fn beacon_detail_dictionary(
    detail: Option<&BeaconDetail>,
    failed: Option<&ReadFailed>,
) -> VarDictionary {
    let mut out = VarDictionary::new();
    out.set(&"known".to_variant(), &detail.is_some().to_variant());
    failed_into(&mut out, failed);
    let mut settings = VarArray::new();
    let mut targets = VarArray::new();
    if let Some(detail) = detail {
        out.set(&"id".to_variant(), &detail.id.to_variant());
        out.set(&"mandate".to_variant(), &detail.mandate.to_variant());
        out.set(&"priority".to_variant(), &detail.priority.to_variant());
        for setting in &detail.settings {
            settings.push(&setting_dictionary(setting).to_variant());
        }
        for target in &detail.targets {
            let mut one = VarDictionary::new();
            one.set(
                &"blueprint_id".to_variant(),
                &target.blueprint_id.to_variant(),
            );
            one.set(&"line".to_variant(), &target.line.to_variant());
            targets.push(&one.to_variant());
        }
    }
    out.set(&"settings".to_variant(), &settings.to_variant());
    out.set(&"targets".to_variant(), &targets.to_variant());
    out
}

/// One setting as the Visit & change menu draws it: `title` (also its `text`), `pointer` and
/// `value_json` as sent, and its editor as a chip's ([`editor_into`]). A setting row carries no
/// `outside` and no `pick_op`: the gateway sends neither for one, and none is made up.
fn setting_dictionary(setting: &Setting) -> VarDictionary {
    let mut out = VarDictionary::new();
    out.set(&"pointer".to_variant(), &setting.pointer.to_variant());
    out.set(&"text".to_variant(), &setting.title.to_variant());
    out.set(&"value_json".to_variant(), &setting.value_json.to_variant());
    editor_into(&mut out, &setting.editor);
    out.set(&"title".to_variant(), &setting.title.to_variant());
    out.set(
        &"unreadable".to_variant(),
        &text_or_empty(setting.unreadable.as_deref()).to_variant(),
    );
    out
}

/// An index as GDScript reads it, -1 for none: the panel's own spelling of "no chip" or "no
/// rule", which no index can be.
fn index_or_none(index: Option<usize>) -> Variant {
    match index {
        Some(index) => count(index),
        None => (-1_i64).to_variant(),
    }
}

/// The size meter as a dictionary: empty before a report names a budget; otherwise `units`
/// and `budget`, as the verifier counted them, and `refusal`, its sentence or empty (S3's
/// plan, E9).
fn size_dictionary(size: Option<&SizeMeter>) -> VarDictionary {
    let mut out = VarDictionary::new();
    if let Some(size) = size {
        out.set(&"units".to_variant(), &i64::from(size.units).to_variant());
        out.set(&"budget".to_variant(), &i64::from(size.budget).to_variant());
        out.set(
            &"refusal".to_variant(),
            &text_or_empty(size.refusal.as_deref()).to_variant(),
        );
    }
    out
}

/// One chip as a dictionary: `pointer`, `step` (from 1; 0 for a reference outside the
/// route), `arm` (`covering`, `on`, `other`), `form` (`name`, `description`,
/// `covered`, `unknown`), `now` and `next` (each empty, or a feature: `id`, `kind`, `x`,
/// `y` as the name spells them, and `travel_text`, the gateway's rendered travel), `failure`,
/// `matched` and `reachable` (whether a route reaches what it reads, as the gateway said).
fn chip_dictionary(chip: &Chip) -> VarDictionary {
    let mut out = VarDictionary::new();
    out.set(&"pointer".to_variant(), &chip.pointer.to_variant());
    out.set(&"step".to_variant(), &step_count(chip.step));
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
    out.set(&"reachable".to_variant(), &chip.reachable.to_variant());
    out
}

/// The chips as an array of [`chip_dictionary`]s, in the file's order.
fn chips_array(chips: &[Chip]) -> VarArray {
    let mut out = VarArray::new();
    for chip in chips {
        out.push(&chip_dictionary(chip).to_variant());
    }
    out
}

/// The Attack targets as an array of [`attack_dictionary`]s, in the file's order.
fn attack_array(marks: &[AttackMark]) -> VarArray {
    let mut out = VarArray::new();
    for mark in marks {
        out.push(&attack_dictionary(mark).to_variant());
    }
    out
}

/// One Attack beacon's target as a dictionary: `pointer`, `step` (from 1; 0 outside the
/// route), `aim` (`voxel`, `beacon`, `selector`, `enemy_commander`, `unknown`), and `at` (a
/// `Vector3i`, sim axes) for a voxel or `id` (the name the player wrote) for a beacon.
fn attack_dictionary(mark: &AttackMark) -> VarDictionary {
    let mut out = VarDictionary::new();
    out.set(&"pointer".to_variant(), &mark.pointer.to_variant());
    out.set(&"step".to_variant(), &step_count(mark.step));
    out.set(&"aim".to_variant(), &mark.aim.name().to_variant());
    match &mark.aim {
        AttackAim::Voxel([x, y, z]) => {
            out.set(&"at".to_variant(), &Vector3i::new(*x, *y, *z).to_variant());
        }
        AttackAim::Beacon(id) => out.set(&"id".to_variant(), &id.to_variant()),
        AttackAim::Selector | AttackAim::EnemyCommander | AttackAim::Unknown => {}
    }
    out
}

/// The Build targets as an array, in the file's order: each `pointer`, `step` (from 1; 0 outside
/// the route), `blueprint` as written, `anchor` (`voxel`, `on`, `unknown`) with `at` (a
/// `Vector3i`, sim axes) for a voxel or `feature` (its name) for `on`, and `has_box`, with
/// `box_min` and `box_max` (`Vector3i`s) for a wall's box as written.
fn build_array(marks: &[BuildMark]) -> VarArray {
    let mut out = VarArray::new();
    for mark in marks {
        let mut one = VarDictionary::new();
        one.set(&"pointer".to_variant(), &mark.pointer.to_variant());
        one.set(&"step".to_variant(), &step_count(mark.step));
        one.set(&"blueprint".to_variant(), &mark.blueprint.to_variant());
        one.set(&"anchor".to_variant(), &mark.anchor.name().to_variant());
        match &mark.anchor {
            BuildAnchor::Voxel([x, y, z]) => {
                one.set(&"at".to_variant(), &Vector3i::new(*x, *y, *z).to_variant());
            }
            BuildAnchor::On(id) => one.set(&"feature".to_variant(), &id.to_variant()),
            BuildAnchor::Unknown => {}
        }
        one.set(
            &"has_box".to_variant(),
            &mark.wall_box.is_some().to_variant(),
        );
        if let Some([[a, b, c], [d, e, f]]) = mark.wall_box {
            one.set(
                &"box_min".to_variant(),
                &Vector3i::new(a, b, c).to_variant(),
            );
            one.set(
                &"box_max".to_variant(),
                &Vector3i::new(d, e, f).to_variant(),
            );
        }
        out.push(&one.to_variant());
    }
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
        &"travel_text".to_variant(),
        &feature.travel_text.to_variant(),
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
    out.set(&"refusal".to_variant(), &recap.refusal.to_variant());
    let mut tallies = VarArray::new();
    for seat in &recap.seat_tallies {
        let mut one = VarDictionary::new();
        one.set(&"seat".to_variant(), &seat.seat.to_variant());
        one.set(&"kills".to_variant(), &i64::from(seat.kills).to_variant());
        one.set(
            &"shared_kills".to_variant(),
            &i64::from(seat.shared_kills).to_variant(),
        );
        one.set(&"losses".to_variant(), &i64::from(seat.losses).to_variant());
        one.set(
            &"value_destroyed_dollars".to_variant(),
            &i64::from(seat.value_destroyed_dollars).to_variant(),
        );
        tallies.push(&one.to_variant());
    }
    out.set(&"seat_tallies".to_variant(), &tallies.to_variant());
    // The Ledger's memos, in issue order, each `{id, text}` as sent; empty outside Probation.
    let mut memos = VarArray::new();
    for memo in &recap.memos {
        let mut one = VarDictionary::new();
        one.set(&"id".to_variant(), &memo.id.to_variant());
        one.set(&"text".to_variant(), &memo.text.to_variant());
        memos.push(&one.to_variant());
    }
    out.set(&"memos".to_variant(), &memos.to_variant());
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
    // Before the gateway's first answer there is no instance to draw: the wizard's own
    // keys below say so (`answered` false), on a dictionary holding nothing else.
    let mut out = match wizard.instance.as_ref() {
        Some(instance) => instance_dictionary(instance),
        None => VarDictionary::new(),
    };
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
        &text_or_empty(wizard.refusal.as_deref()).to_variant(),
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
    int_variant(value)
}

fn count(value: usize) -> Variant {
    int_variant(value)
}

/// A route step numbered from 1 as GDScript reads it, 0 for none: the panel's own
/// spelling of "outside the route", which no step number can be.
fn step_count(step: Option<usize>) -> Variant {
    match step {
        Some(step) => count(step),
        None => 0_i64.to_variant(),
    }
}
