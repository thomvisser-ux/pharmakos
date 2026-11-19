// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The words a seat may write and one own beacon's detail, as the gateway sends them, and the
//! JSON each pick of them composes (S3's plan, task `ed3b`, part 2: the condition picker, the
//! step-label and rule-id picks, and the Visit & change rows; register rows S3-17 and S3-01).
//!
//! **The client holds no word of its own.** Every family, predicate, parameter title, lock,
//! mandate and blueprint is `get_vocabulary`'s, and every setting, its title, its value now and
//! what may be written there is `get_beacon`'s, each drawn as sent and in the order sent
//! (AGENTS.md section 3 rule 4). A lock is the gateway's `locked` sentence: a locked entry is
//! drawn greyed with that sentence and cannot be chosen, and nothing here decides what is
//! locked. A pick composes one JSON value from what was sent (a predicate's `skeleton_json`
//! wrapped under its name, a setting nested from its pointer, a mandate's skeleton under its
//! name), and whether the result is legal is the verifier's next report.
//!
//! Two readers of the player's own file live here, as lax as the route's ([`step_labels`],
//! [`rule_ids`]): the labels and rule ids a step-label or rule-id chip is picked from, in file
//! order. The wire names the kind of the reference and sends no choices, and the gateway's
//! check, not the client, says whether a pick is a legal one (the main session's ruling on
//! decisions-log item 160 (4)).

use pharmakos_proto::gp::api::v1::reference_shape::Kind;
use pharmakos_proto::gp::api::v1::value_shape::Shape;
use pharmakos_proto::gp::api::v1::{
    Chip, GetBeaconResponse, GetVocabularyResponse, ValueShape, VocabularyEntry,
};
use pharmakos_proto::json::{self, Json};

use crate::editor::lax_document;
use crate::enums;
use crate::error::BridgeError;
use crate::rule_list::{self, ChipEditor};
use crate::view::split_footer;

/// The unit of a duration, which the client never asks the player to type (the owner's ruling
/// of 2026-11-15, which holds for the picker's parameters as for the rule list's chips).
const MILLISECONDS: &str = "ms";

/// What a picker's parameter holds, by the shape the gateway sent: drawn beside its title,
/// never edited on the picker's page (once a predicate is in the file its values are chips).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ParameterShape {
    /// A whole number within the bounds sent, in `unit`.
    Number {
        /// The lowest value, as sent.
        min: Option<i32>,
        /// The highest value, as sent.
        max: Option<i32>,
        /// The unit, as sent.
        unit: String,
    },
    /// A duration: drawn with no field, as the rule list draws a duration's chip.
    Milliseconds,
    /// A closed list: each value the JSON text the gateway sent.
    Choice {
        /// The values, in the gateway's order.
        values: Vec<String>,
    },
    /// A reference picked from the map or the playbook: the reference's kind, by name.
    Reference {
        /// `beacon`, `location`, `feature`, `step_label` or `rule_id`.
        kind: &'static str,
    },
    /// A shape the gateway left unset, or set to a kind this build does not know.
    Unknown,
}

impl ParameterShape {
    /// The name GDScript reads.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Number { .. } => "number",
            Self::Milliseconds => "ms",
            Self::Choice { .. } => "choice",
            Self::Reference { .. } => "reference",
            Self::Unknown => "unknown",
        }
    }
}

/// One parameter of a vocabulary entry.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Parameter {
    /// Its pointer inside the entry's own object, as sent.
    pub pointer: String,
    /// Its name in the player's words, as sent.
    pub title: String,
    /// What it holds.
    pub shape: ParameterShape,
}

/// One entry: a predicate, a mandate or a blueprint.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    /// The word a playbook writes; empty on an entry the schema does not hold yet.
    pub name: String,
    /// Its name in the player's words.
    pub title: String,
    /// Its parameters, in the gateway's order.
    pub parameters: Vec<Parameter>,
    /// What a pick inserts, as JSON text; empty on a locked entry.
    pub skeleton_json: String,
    /// Why this seat cannot use it, in the gateway's words; `None` when it can.
    pub locked: Option<String>,
}

/// One condition family.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Family {
    /// Its stable id, as sent.
    pub id: String,
    /// Its name in the player's words.
    pub title: String,
    /// Why the whole family cannot be used, in the gateway's words; `None` when it can.
    pub locked: Option<String>,
    /// Its predicates, in the gateway's order.
    pub predicates: Vec<Entry>,
}

/// The words, as `get_vocabulary` last answered this Lull.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Vocabulary {
    /// The condition families, in the gateway's order.
    pub families: Vec<Family>,
    /// The mandates a beacon may carry.
    pub mandates: Vec<Entry>,
    /// The blueprints a Build target or a queued structure may name.
    pub blueprints: Vec<Entry>,
}

/// A `get_vocabulary` answer.
///
/// # Errors
///
/// [`BridgeError::Schema`] when the answer is not a `GetVocabularyResponse`.
pub fn vocabulary_of(result: &Json) -> Result<Vocabulary, BridgeError> {
    let (answer, _) = split_footer(result);
    let canonical = enums::canonical("gp.api.v1.GetVocabularyResponse", &answer);
    let response: GetVocabularyResponse = json::decode_json(&canonical)?;
    Ok(Vocabulary {
        families: response
            .families
            .into_iter()
            .map(|family| Family {
                id: family.family,
                title: family.title,
                locked: family.locked,
                predicates: family.predicates.into_iter().map(entry_of).collect(),
            })
            .collect(),
        mandates: response.mandates.into_iter().map(entry_of).collect(),
        blueprints: response.blueprints.into_iter().map(entry_of).collect(),
    })
}

/// The vocabulary from one `get_vocabulary` answer's text: a whole JSON-RPC answer, or its
/// result. What the hostless picker scene draws.
///
/// # Errors
///
/// [`BridgeError::Schema`] for text that is not JSON, [`BridgeError::Rpc`] for a refusal, and
/// whatever [`vocabulary_of`] refuses.
pub fn vocabulary_of_text(text: &str) -> Result<Vocabulary, BridgeError> {
    let frame = json::read(text)?;
    if frame.get("error").is_some() {
        return Err(BridgeError::Rpc(
            "the answer is a refusal, not a vocabulary".to_owned(),
        ));
    }
    match frame.get("result") {
        Some(result) => vocabulary_of(result),
        None => vocabulary_of(&frame),
    }
}

fn entry_of(entry: VocabularyEntry) -> Entry {
    Entry {
        name: entry.name,
        title: entry.title,
        parameters: entry
            .parameters
            .into_iter()
            .map(|parameter| Parameter {
                shape: parameter_shape(parameter.shape.as_ref()),
                pointer: parameter.pointer,
                title: parameter.title,
            })
            .collect(),
        skeleton_json: entry.skeleton_json,
        locked: entry.locked,
    }
}

fn parameter_shape(shape: Option<&ValueShape>) -> ParameterShape {
    match shape.and_then(|shape| shape.shape.as_ref()) {
        Some(Shape::Integer(integer)) if integer.unit == MILLISECONDS => {
            ParameterShape::Milliseconds
        }
        Some(Shape::Integer(integer)) => ParameterShape::Number {
            min: integer.min,
            max: integer.max,
            unit: integer.unit.clone(),
        },
        Some(Shape::Choice(choice)) => ParameterShape::Choice {
            values: choice.values.clone(),
        },
        Some(Shape::Reference(reference)) => match Kind::try_from(reference.kind) {
            Ok(Kind::Beacon) => ParameterShape::Reference { kind: "beacon" },
            Ok(Kind::Location) => ParameterShape::Reference { kind: "location" },
            Ok(Kind::Feature) => ParameterShape::Reference { kind: "feature" },
            Ok(Kind::StepLabel) => ParameterShape::Reference { kind: "step_label" },
            Ok(Kind::HandlerId) => ParameterShape::Reference { kind: "rule_id" },
            Ok(Kind::Unspecified) | Err(_) => ParameterShape::Unknown,
        },
        None => ParameterShape::Unknown,
    }
}

/// Why a pick from the vocabulary or the detail composes nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PickError {
    /// No entry at that index.
    NoSuchEntry,
    /// The gateway locked the entry or its family.
    Locked,
    /// The gateway's skeleton or value text is not JSON.
    NotJson,
    /// A setting's pointer names no field.
    NoPointer,
    /// The entry carries no lock and no name: the answer named no word to write.
    Unnamed,
}

/// The value a predicate pick writes into a condition slot: `{"<name>": <skeleton>}`, from
/// family `family`'s predicate `entry` as sent.
///
/// # Errors
///
/// [`PickError`] for an index the vocabulary does not hold, a family or entry the gateway
/// locked (an entry with no name is one the schema does not hold yet, which the gateway always
/// locks), or a skeleton that is not JSON.
pub fn predicate_value(
    vocabulary: &Vocabulary,
    family: usize,
    entry: usize,
) -> Result<Json, PickError> {
    let family = vocabulary
        .families
        .get(family)
        .ok_or(PickError::NoSuchEntry)?;
    if family.locked.is_some() {
        return Err(PickError::Locked);
    }
    let entry = family.predicates.get(entry).ok_or(PickError::NoSuchEntry)?;
    named_skeleton(entry)
}

/// `{"<name>": <skeleton>}` for an entry the gateway did not lock.
///
/// # Errors
///
/// [`PickError::Locked`] for a locked or nameless entry, [`PickError::NotJson`] for a skeleton
/// that is not JSON.
pub fn named_skeleton(entry: &Entry) -> Result<Json, PickError> {
    usable_name(entry)?;
    let skeleton = json::read(&entry.skeleton_json).map_err(|_| PickError::NotJson)?;
    Ok(Json::Object(vec![(entry.name.clone(), skeleton)]))
}

/// The RFC 6902 patch that puts `value` at `pointer`: a `replace` where a value stands, an
/// `add` into a slot that holds none.
#[must_use]
pub fn slot_patch(pointer: &str, value: Json, present: bool) -> String {
    let op = if present { "replace" } else { "add" };
    json::write(&Json::Array(vec![Json::Object(vec![
        ("op".to_owned(), Json::String(op.to_owned())),
        ("path".to_owned(), Json::String(pointer.to_owned())),
        ("value".to_owned(), value),
    ])]))
}

// --- One own beacon's detail ------------------------------------------------------------

/// One setting of a beacon's mandate, as sent, with the editor its shape asks for.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Setting {
    /// Its pointer inside `gp.v1.MandateSettings`, as sent.
    pub pointer: String,
    /// Its name in the player's words.
    pub title: String,
    /// The value now, as JSON text, as sent.
    pub value_json: String,
    /// Its editor, as a rule-list chip's would be.
    pub editor: ChipEditor,
}

/// One of a Build beacon's targets, as sent.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HeldTarget {
    /// The blueprint, as sent.
    pub blueprint_id: String,
    /// Its anchor, a `gp.v1.Location` in the playbook's canonical spelling: what a removal
    /// names.
    pub anchor: Json,
    /// The anchor as JSON text, for the menu to show as it came.
    pub anchor_text: String,
}

/// What `get_beacon` said about one of this seat's beacons.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BeaconDetail {
    /// The beacon, by the id the click named.
    pub id: String,
    /// The mandate it carries, as the wire spells it (lower case), or empty for none: drawn
    /// with the vocabulary's title for the mandate of that name, when there is one.
    pub mandate: String,
    /// Its Quartermaster priority now, as the summary's wire spells it (`normal`), or empty
    /// when the summary sent none: the Visit & change menu marks the priority row it names.
    pub priority: String,
    /// The settings its mandate has now, in the gateway's order.
    pub settings: Vec<Setting>,
    /// A Build beacon's targets, in the gateway's order.
    pub targets: Vec<HeldTarget>,
}

/// A `get_beacon` answer's detail, for the beacon `id`. Another seat's beacon has none of it:
/// the gateway leaves all three fields out, and so the detail is empty. A setting whose value
/// does not fit its shape is kept as a read-only row with its reason
/// ([`crate::rule_list::ReadOnly::Unreadable`]), so one bad row loses nothing else.
///
/// # Errors
///
/// [`BridgeError::Schema`] when the answer is not a `GetBeaconResponse`, and
/// [`BridgeError::Rpc`] for a target with no anchor.
pub fn beacon_detail_of(id: &str, result: &Json) -> Result<(String, BeaconDetail), BridgeError> {
    let (answer, _) = split_footer(result);
    let canonical = enums::canonical("gp.api.v1.GetBeaconResponse", &answer);
    let response: GetBeaconResponse = json::decode_json(&canonical)?;
    let mandate = match answer.get("mandate") {
        Some(Json::String(mandate)) => mandate.clone(),
        _ => String::new(),
    };
    let priority = match answer
        .get("summary")
        .and_then(|summary| summary.get("priority"))
    {
        Some(Json::String(priority)) => priority.clone(),
        _ => String::new(),
    };
    let settings = response
        .settings
        .into_iter()
        .map(|row| {
            let chip = Chip {
                pointer: row.pointer.clone(),
                value_json: row.value_json.clone(),
                shape: row.shape,
                ..Chip::default()
            };
            let editor = match rule_list::editor_of(&chip) {
                Ok(editor) => editor,
                Err(_) => ChipEditor::ReadOnly(rule_list::ReadOnly::Unreadable),
            };
            Setting {
                editor,
                pointer: row.pointer,
                title: row.title,
                value_json: row.value_json,
            }
        })
        .collect::<Vec<_>>();
    let mut targets = Vec::with_capacity(response.build_targets.len());
    if let Some(Json::Array(held)) = canonical.get("build_targets") {
        for target in held {
            let anchor = target.get("anchor").cloned().ok_or_else(|| {
                BridgeError::Rpc("a target with no anchor, which removes nothing".to_owned())
            })?;
            let blueprint_id = match target.get("blueprint_id") {
                Some(Json::String(blueprint)) => blueprint.clone(),
                _ => String::new(),
            };
            targets.push(HeldTarget {
                blueprint_id,
                anchor_text: compact(&anchor),
                anchor,
            });
        }
    }
    Ok((
        response.prose,
        BeaconDetail {
            id: id.to_owned(),
            mandate,
            priority,
            settings,
            targets,
        },
    ))
}

/// JSON text on one line, as it came: what the menu shows of an anchor.
fn compact(value: &Json) -> String {
    json::write(value)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The `gp.v1.MandateSettings` object that sets the one field at `pointer` to `value`: the
/// pointer's segments, outermost first, each an object around the next
/// (`/build/rebuild_destroyed` and `true` give `{"build":{"rebuild_destroyed":true}}`).
///
/// # Errors
///
/// [`PickError::NoPointer`] for a pointer that names no field (empty, or not starting `/`).
pub fn nested_setting(pointer: &str, value: Json) -> Result<Json, PickError> {
    let Some(rest) = pointer.strip_prefix('/') else {
        return Err(PickError::NoPointer);
    };
    let segments: Vec<String> = rest.split('/').map(unescape).collect();
    if segments.iter().any(String::is_empty) {
        return Err(PickError::NoPointer);
    }
    Ok(segments
        .into_iter()
        .rev()
        .fold(value, |inner, key| Json::Object(vec![(key, inner)])))
}

/// One RFC 6901 reference token, unescaped: `~1` is `/`, `~0` is `~`.
fn unescape(token: &str) -> String {
    token.replace("~1", "/").replace("~0", "~")
}

/// The interface row that changes one setting.
#[must_use]
pub fn settings_row(settings: Json) -> Json {
    Json::Object(vec![("set_mandate_settings".to_owned(), settings)])
}

/// The interface row that switches the beacon to `mandate`, as sent.
///
/// # Errors
///
/// [`PickError`] as [`named_skeleton`] refuses.
pub fn mandate_row(mandate: &Entry) -> Result<Json, PickError> {
    Ok(Json::Object(vec![(
        "set_mandate".to_owned(),
        named_skeleton(mandate)?,
    )]))
}

/// A Build target's anchor on the feature `id`, `{"on":{"feature_id":"<id>"}}`: a vent a
/// Generator stands on, named as `RemoveBuildTargetRow`'s comment names one.
#[must_use]
pub fn on_feature(id: &str) -> Json {
    Json::Object(vec![(
        "on".to_owned(),
        Json::Object(vec![("feature_id".to_owned(), Json::String(id.to_owned()))]),
    )])
}

/// The interface row that adds a Build target of `blueprint` anchored at `anchor`.
///
/// # Errors
///
/// [`PickError::Locked`] for a blueprint the gateway locked or one with no name.
pub fn add_target_row(blueprint: &Entry, anchor: Json) -> Result<Json, PickError> {
    let id = usable_name(blueprint)?;
    Ok(Json::Object(vec![(
        "add_build_target".to_owned(),
        Json::Object(vec![(
            "target".to_owned(),
            Json::Object(vec![
                ("blueprint_id".to_owned(), Json::String(id)),
                ("anchor".to_owned(), anchor),
            ]),
        )]),
    )]))
}

/// The interface row that removes the Build target anchored at `target`'s anchor, as sent.
#[must_use]
pub fn remove_target_row(target: &HeldTarget) -> Json {
    Json::Object(vec![(
        "remove_build_target".to_owned(),
        Json::Object(vec![("anchor".to_owned(), target.anchor.clone())]),
    )])
}

/// The interface row that queues `blueprint` on the beacon's fabricator.
///
/// # Errors
///
/// [`PickError::Locked`] for a blueprint the gateway locked or one with no name.
pub fn queue_row(blueprint: &Entry) -> Result<Json, PickError> {
    let id = usable_name(blueprint)?;
    Ok(Json::Object(vec![(
        "queue_structure".to_owned(),
        Json::Object(vec![("blueprint_id".to_owned(), Json::String(id))]),
    )]))
}

/// The word an entry writes: refused as [`PickError::Locked`] when the gateway locked it (its
/// reason is drawn beside it), and as [`PickError::Unnamed`] when it carries no lock and no name,
/// which says what is true: the answer named nothing to write.
///
/// # Errors
///
/// As above.
pub fn usable_name(entry: &Entry) -> Result<String, PickError> {
    if entry.locked.is_some() {
        return Err(PickError::Locked);
    }
    if entry.name.is_empty() {
        return Err(PickError::Unnamed);
    }
    Ok(entry.name.clone())
}

/// The route step that visits `beacon` and applies `row`, labelled `label`: one step per row
/// (decided; the owner can veto): a row is never merged into an earlier visit, which would make
/// the client reason about which step is "the same visit" and what it costs.
#[must_use]
pub fn visit_step(beacon: &str, row: Json, label: &str) -> Json {
    Json::Object(vec![
        ("label".to_owned(), Json::String(label.to_owned())),
        (
            "interface".to_owned(),
            Json::Object(vec![
                (
                    "beacon".to_owned(),
                    Json::Object(vec![(
                        "beacon_id".to_owned(),
                        Json::String(beacon.to_owned()),
                    )]),
                ),
                ("rows".to_owned(), Json::Array(vec![row])),
            ]),
        ),
    ])
}

// --- The labels and rule ids a reference chip is picked from ---------------------------

/// The route's step labels, read laxly out of the player's text, in file order, each once (a
/// label the file repeats is listed where it first stands; a step with no label lists
/// nothing). Which one is legal at a chip is the gateway's check, never this list's.
#[must_use]
pub fn step_labels(text: &str) -> Vec<String> {
    names_under(text, "route", "label")
}

/// The rules' ids, read laxly out of the player's text, in file order, each once.
#[must_use]
pub fn rule_ids(text: &str) -> Vec<String> {
    names_under(text, "handlers", "id")
}

fn names_under(text: &str, list: &str, key: &str) -> Vec<String> {
    let Some(document) = lax_document(text) else {
        return Vec::new();
    };
    let Some(Json::Array(items)) = document
        .get("declarative")
        .and_then(|value| value.get(list))
    else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for item in items {
        if let Some(Json::String(name)) = item.get(key)
            && !name.is_empty()
            && !out.contains(name)
        {
            out.push(name.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gateway's `get_vocabulary` golden, by its byte copy (`godot/fixtures/vocabulary.json`).
    const VOCABULARY: &str = include_str!("../../../godot/fixtures/vocabulary.json");
    /// The gateway's `get_beacon` golden for seat 0's core, by its byte copy.
    const BEACON: &str = include_str!("../../../godot/fixtures/beacon_core.json");
    /// The gateway's `get_beacon` golden for a Build beacon with targets (a wall among them).
    const BUILD: &str = include_str!("../../../godot/fixtures/beacon_build.json");

    fn entry<'a>(entries: &'a [Entry], name: &str) -> &'a Entry {
        entries
            .iter()
            .find(|entry| entry.name == name)
            .unwrap_or_else(|| panic!("no entry {name}"))
    }

    fn parameter<'a>(entry: &'a Entry, pointer: &str) -> &'a Parameter {
        entry
            .parameters
            .iter()
            .find(|parameter| parameter.pointer == pointer)
            .unwrap_or_else(|| panic!("no parameter {pointer}"))
    }

    fn compact(value: &Json) -> String {
        json::write(value).split_whitespace().collect()
    }

    fn vocabulary() -> Vocabulary {
        vocabulary_of_text(VOCABULARY).expect("the fixture reads")
    }

    fn family(vocabulary: &Vocabulary, id: &str) -> usize {
        vocabulary
            .families
            .iter()
            .position(|family| family.id == id)
            .expect("the family")
    }

    #[test]
    fn the_vocabulary_is_read_as_sent_with_its_locks() {
        let words = vocabulary();
        assert!(!words.families.is_empty());
        let clock = words.families.get(family(&words, "clock")).expect("clock");
        let rest = clock
            .predicates
            .iter()
            .find(|entry| entry.name.is_empty())
            .expect("the locked rest of the clock");
        assert_eq!(rest.locked.as_deref(), Some("from S6"));
        let elapsed = entry(&clock.predicates, "segment_elapsed");
        assert_eq!(
            parameter(elapsed, "/ms/value").shape,
            ParameterShape::Milliseconds
        );
        let compared = parameter(elapsed, "/ms/op");
        assert!(
            matches!(&compared.shape, ParameterShape::Choice { values } if values.first().map(String::as_str) == Some("\"LT\""))
        );
        assert!(!words.mandates.is_empty());
        assert!(
            words
                .blueprints
                .iter()
                .any(|blueprint| blueprint.locked.is_some()),
            "a blueprint the seat holds no licence for is locked, with its reason"
        );
    }

    #[test]
    fn a_predicate_pick_wraps_its_skeleton_under_its_name() {
        let words = vocabulary();
        let commander = family(&words, "commander");
        let at = words
            .families
            .get(commander)
            .and_then(|family| {
                family
                    .predicates
                    .iter()
                    .position(|entry| entry.name == "cmdr_hp_pct")
            })
            .expect("cmdr_hp_pct");
        let value = predicate_value(&words, commander, at).expect("composed");
        assert_eq!(compact(&value), r#"{"cmdr_hp_pct":{"cmp":"LE","pct":40}}"#);
        assert_eq!(
            compact(
                &json::read(&slot_patch("/declarative/handlers/0/when", value, true))
                    .expect("json")
            ),
            r#"[{"op":"replace","path":"/declarative/handlers/0/when","value":{"cmdr_hp_pct":{"cmp":"LE","pct":40}}}]"#
        );
        assert!(
            slot_patch("/declarative/route/0/skip_if", Json::Null, false)
                .contains(r#""op": "add""#)
                || slot_patch("/declarative/route/0/skip_if", Json::Null, false)
                    .contains(r#""op":"add""#)
        );
    }

    #[test]
    fn a_locked_entry_or_family_composes_nothing() {
        let words = vocabulary();
        let clock = family(&words, "clock");
        let rest = words
            .families
            .get(clock)
            .and_then(|family| {
                family
                    .predicates
                    .iter()
                    .position(|entry| entry.name.is_empty())
            })
            .expect("locked");
        assert_eq!(predicate_value(&words, clock, rest), Err(PickError::Locked));
        let locked_family = words
            .families
            .iter()
            .position(|family| family.locked.is_some())
            .expect("a locked family");
        assert_eq!(
            predicate_value(&words, locked_family, 0),
            Err(PickError::Locked)
        );
        assert_eq!(
            predicate_value(&words, words.families.len(), 0),
            Err(PickError::NoSuchEntry)
        );
    }

    #[test]
    fn the_beacon_detail_is_read_and_another_seats_has_none() {
        let frame = json::read(BEACON).expect("json");
        let result = frame.get("result").expect("a result");
        let (prose, detail) = beacon_detail_of("b_00", result).expect("reads");
        assert!(!prose.is_empty());
        assert_eq!(detail.mandate, "build");
        let rebuild = detail
            .settings
            .iter()
            .find(|setting| setting.pointer == "/build/rebuild_destroyed")
            .expect("the setting");
        assert_eq!(
            rebuild.editor,
            ChipEditor::Choice {
                values: vec!["true".to_owned(), "false".to_owned()]
            }
        );
        let foreign =
            json::read(r#"{"summary":{"beacon_id":"e_00"},"prose":"Theirs."}"#).expect("json");
        let (_, theirs) = beacon_detail_of("e_00", &foreign).expect("reads");
        assert_eq!(theirs.mandate, "");
        assert!(theirs.settings.is_empty() && theirs.targets.is_empty());
    }

    /// The Build beacon's golden: every target read with its anchor as sent, a wall's and a
    /// described vent's included, and a removal names the anchor in the playbook's canonical
    /// spelling (the wire's lower-case `nearest` back to `NEAREST`).
    #[test]
    fn a_build_beacons_targets_are_read_as_sent() {
        let frame = json::read(BUILD).expect("json");
        let (_, detail) =
            beacon_detail_of("b_00", frame.get("result").expect("a result")).expect("reads");
        let blueprints: Vec<&str> = detail
            .targets
            .iter()
            .map(|target| target.blueprint_id.as_str())
            .collect();
        assert!(blueprints.contains(&"wall"), "{blueprints:?}");
        let described = detail
            .targets
            .iter()
            .find(|target| target.anchor_text.contains("\"vent\""))
            .expect("a target on a described vent");
        assert_eq!(
            compact(&remove_target_row(described)),
            r#"{"remove_build_target":{"anchor":{"on":{"vent":{"rank":"NEAREST"}}}}}"#
        );
    }

    #[test]
    fn each_row_is_exact() {
        let settings =
            nested_setting("/build/rebuild_destroyed", Json::Bool(true)).expect("a pointer");
        assert_eq!(
            compact(&visit_step("b_00", settings_row(settings), "visit_1")),
            r#"{"label":"visit_1","interface":{"beacon":{"beacon_id":"b_00"},"rows":[{"set_mandate_settings":{"build":{"rebuild_destroyed":true}}}]}}"#
        );
        assert_eq!(
            compact(&settings_row(
                nested_setting("/retreat_hp_pct", Json::Number("30".to_owned()))
                    .expect("a pointer")
            )),
            r#"{"set_mandate_settings":{"retreat_hp_pct":30}}"#
        );
        assert_eq!(nested_setting("", Json::Null), Err(PickError::NoPointer));
        assert_eq!(
            nested_setting("/a//b", Json::Null),
            Err(PickError::NoPointer)
        );
        let entry = |name: &str, skeleton: &str, locked: Option<&str>| Entry {
            name: name.to_owned(),
            title: name.to_owned(),
            parameters: Vec::new(),
            skeleton_json: skeleton.to_owned(),
            locked: locked.map(str::to_owned),
        };
        assert_eq!(
            compact(&mandate_row(&entry("defend", "{}", None)).expect("a row")),
            r#"{"set_mandate":{"defend":{}}}"#
        );
        let anchor = json::read(r#"{"voxel":{"x":1,"y":2,"z":3}}"#).expect("json");
        assert_eq!(
            compact(&add_target_row(&entry("generator", "", None), anchor.clone()).expect("a row")),
            r#"{"add_build_target":{"target":{"blueprint_id":"generator","anchor":{"voxel":{"x":1,"y":2,"z":3}}}}}"#
        );
        assert_eq!(
            add_target_row(
                &entry("resonance_spire", "", Some("a licence")),
                anchor.clone()
            ),
            Err(PickError::Locked)
        );
        assert_eq!(
            compact(&remove_target_row(&HeldTarget {
                blueprint_id: "generator".to_owned(),
                anchor: json::read(r#"{"on":{"feature_id":"vent_120_88"}}"#).expect("json"),
                anchor_text: String::new(),
            })),
            r#"{"remove_build_target":{"anchor":{"on":{"feature_id":"vent_120_88"}}}}"#
        );
        assert_eq!(
            compact(&queue_row(&entry("resonance_spire", "", None)).expect("a row")),
            r#"{"queue_structure":{"blueprint_id":"resonance_spire"}}"#
        );
        assert_eq!(
            queue_row(&entry("", "", Some("from S4"))),
            Err(PickError::Locked)
        );
    }

    #[test]
    fn labels_and_rule_ids_are_listed_in_file_order_once_each() {
        let text = r#"{
          // a comment
          "declarative": {
            "route": [
              {"label": "b", "hold_ms": 1},
              {"hold_ms": 2},
              {"label": "a", "hold_ms": 3},
              {"label": "b", "hold_ms": 4}
            ],
            "handlers": [{"id": "r2"}, {"id": "r1"}, {"when": {}}]
          }
        }"#;
        assert_eq!(step_labels(text), ["b", "a"]);
        assert_eq!(rule_ids(text), ["r2", "r1"]);
        assert!(step_labels("not json").is_empty());
    }
}
