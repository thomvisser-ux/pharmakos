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
//! check, not the client, says whether a pick is a legal one: the client lists names and
//! judges none (decisions-log item 160 (4)).

use pharmakos_proto::gp::api::v1::reference_shape::Kind;
use pharmakos_proto::gp::api::v1::value_shape::Shape;
use pharmakos_proto::gp::api::v1::vocabulary_entry::AnchorShape as WireAnchorShape;
use pharmakos_proto::gp::api::v1::{
    Chip, GetBeaconResponse, GetVocabularyResponse, ValueShape, VocabularyEntry,
};
use pharmakos_proto::json::{self, Json};

use crate::editor::lax_document;
use crate::enums;
use crate::error::BridgeError;
use crate::rule_list::{self, ChipEditor};
use crate::view::split_footer;

/// What a picker's parameter holds, by the shape the gateway sent: drawn beside its title,
/// never edited on the picker's page (once a predicate is in the file its values are chips).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ParameterShape {
    /// A number within the bounds sent, in `unit`, shown in `display_unit` (seconds for a
    /// duration, S3's `fixq`).
    Number {
        /// The lowest value, as sent, in `unit`.
        min: Option<i32>,
        /// The highest value, as sent, in `unit`.
        max: Option<i32>,
        /// The unit the value and its bounds are in, as sent.
        unit: String,
        /// The unit it is shown and typed in, as sent.
        display_unit: String,
        /// `min` as it is shown and typed, in `display_unit`, as sent: present exactly when `min`
        /// is (S3's `zerotitle`).
        min_text: Option<String>,
        /// `max` as it is shown and typed, in `display_unit`, as sent, paired with `max`.
        max_text: Option<String>,
    },
    /// A closed list: each value the JSON text the gateway sent, and its title.
    Choice {
        /// The values, in the gateway's order.
        values: Vec<String>,
        /// Each value in the player's words, one per value: an answer whose titles do not
        /// pair with its values is refused ([`vocabulary_of`]).
        titles: Vec<String>,
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
    /// On a blueprint, how its Build target is anchored, as the gateway read it from the
    /// verifier's table (`VocabularyEntry.anchor_shape`).
    pub anchor_shape: AnchorShape,
}

/// How a blueprint's Build target is anchored, as `get_vocabulary` names it (S3's `fixq`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnchorShape {
    /// One place: a map click on ground, or on a vent.
    Point,
    /// A box a wall fills: two map clicks, its corners, composed by the gateway
    /// (`compose_wall_box`).
    Box,
    /// Unset (every entry that is no blueprint) or a value this build has no name for: no
    /// anchor pick is guessed for it.
    Unnamed,
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
    /// The blueprints, as a Build target may name them (Add a build target's list).
    pub blueprints: Vec<Entry>,
    /// The blueprints, as a `queue_structure` row may name them, each locked by the verifier's
    /// own rule for a queue (Queue a structure's list; S3's `fixq`). Drawn as sent, never
    /// filtered.
    pub queue_blueprints: Vec<Entry>,
    /// The condition groups a picker may insert (`all`, `any`, `not`), each with its skeleton
    /// of empty items.
    pub groups: Vec<Entry>,
}

/// A `get_vocabulary` answer.
///
/// # Errors
///
/// [`BridgeError::Schema`] when the answer is not a `GetVocabularyResponse`, and
/// [`BridgeError::Rpc`] for a choice whose titles do not pair with its values one for one (S3's
/// `fixq-c`, fix pass 1: a choice drawn with a title short would draw a value's JSON, or the
/// wrong words beside a value): the picker's line says why.
pub fn vocabulary_of(result: &Json) -> Result<Vocabulary, BridgeError> {
    let (answer, _) = split_footer(result);
    let canonical = enums::canonical("gp.api.v1.GetVocabularyResponse", &answer);
    let response: GetVocabularyResponse = json::decode_json(&canonical)?;
    let entries = |list: Vec<VocabularyEntry>| -> Result<Vec<Entry>, BridgeError> {
        list.into_iter().map(entry_of).collect()
    };
    let mut families = Vec::with_capacity(response.families.len());
    for family in response.families {
        families.push(Family {
            id: family.family,
            title: family.title,
            locked: family.locked,
            predicates: entries(family.predicates)?,
        });
    }
    Ok(Vocabulary {
        families,
        mandates: entries(response.mandates)?,
        blueprints: entries(response.blueprints)?,
        queue_blueprints: entries(response.queue_blueprints)?,
        groups: entries(response.groups)?,
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

fn entry_of(entry: VocabularyEntry) -> Result<Entry, BridgeError> {
    let mut parameters = Vec::with_capacity(entry.parameters.len());
    for parameter in entry.parameters {
        parameters.push(Parameter {
            shape: parameter_shape(&entry.name, &parameter.pointer, parameter.shape.as_ref())?,
            pointer: parameter.pointer,
            title: parameter.title,
        });
    }
    Ok(Entry {
        name: entry.name,
        title: entry.title,
        parameters,
        skeleton_json: entry.skeleton_json,
        locked: entry.locked,
        anchor_shape: match WireAnchorShape::try_from(entry.anchor_shape) {
            Ok(WireAnchorShape::Point) => AnchorShape::Point,
            Ok(WireAnchorShape::Box) => AnchorShape::Box,
            Ok(WireAnchorShape::Unspecified) | Err(_) => AnchorShape::Unnamed,
        },
    })
}

fn parameter_shape(
    entry: &str,
    pointer: &str,
    shape: Option<&ValueShape>,
) -> Result<ParameterShape, BridgeError> {
    if let Some(Shape::Choice(choice)) = shape.and_then(|shape| shape.shape.as_ref())
        && choice.titles.len() != choice.values.len()
    {
        return Err(BridgeError::Rpc(format!(
            "the choice at {entry}{pointer} names {} values and {} titles",
            choice.values.len(),
            choice.titles.len()
        )));
    }
    Ok(match shape.and_then(|shape| shape.shape.as_ref()) {
        Some(Shape::Integer(integer)) => {
            let at = format!("{entry}{pointer}");
            ParameterShape::Number {
                min: integer.min,
                max: integer.max,
                unit: integer.unit.clone(),
                display_unit: integer.display_unit.clone(),
                min_text: rule_list::bound_text(&at, "least", integer.min, &integer.min_text)?,
                max_text: rule_list::bound_text(&at, "most", integer.max, &integer.max_text)?,
            }
        }
        Some(Shape::Choice(choice)) => ParameterShape::Choice {
            values: choice.values.clone(),
            titles: choice.titles.clone(),
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
    })
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
    /// Why its value could not be read against its shape, in the decode's words, when it could
    /// not: the row is then read-only ([`crate::rule_list::ReadOnly::Unreadable`]) and says this.
    pub unreadable: Option<String>,
}

/// One of a Build beacon's targets, as sent.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HeldTarget {
    /// The blueprint, as sent.
    pub blueprint_id: String,
    /// Its anchor, a `gp.v1.Location` in the playbook's canonical spelling: what a removal
    /// names.
    pub anchor: Json,
    /// The target in the player's words, the gateway's (`GetBeaconResponse.build_target_lines`,
    /// S3's `fixq`): what the Remove list draws, never an anchor's JSON.
    pub line: String,
}

/// One list of a beacon's mandate, as sent (`gp.api.v1.BeaconListRow`; S3's `edlists`): drawn
/// and emptied, never read for a rule.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HeldList {
    /// Its name in the player's words.
    pub title: String,
    /// One sentence per entry, the gateway's, in its order. Empty for an empty list.
    pub entry_lines: Vec<String>,
    /// The flag that empties it: what the clear writes `true`.
    pub clear_pointer: String,
    /// The clear as an action, in the gateway's words: sent only for a list with entries, and
    /// empty otherwise. The clear is offered exactly when it is not empty.
    pub clear_title: String,
    /// What an empty list is, in the gateway's words: sent only for a list with none, and
    /// empty otherwise. Drawn when it is not empty.
    pub empty_title: String,
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
    /// The lists its mandate holds, in the gateway's order (S3's `edlists`).
    pub lists: Vec<HeldList>,
}

/// A `get_beacon` answer's detail, for the beacon `id`. Another seat's beacon has none of it:
/// the gateway leaves all three fields out, and so the detail is empty. A setting whose value
/// does not fit its shape is kept as a read-only row with its reason
/// ([`crate::rule_list::ReadOnly::Unreadable`]), so one bad row loses nothing else.
///
/// # Errors
///
/// [`BridgeError::Schema`] when the answer is not a `GetBeaconResponse`, and
/// [`BridgeError::Rpc`] for a target with no anchor, or for a list of target lines that does not
/// pair with the targets one for one (a line drawn beside the wrong target removes the wrong one).
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
                value_text: row.value_text.clone(),
                value_title: row.value_title.clone(),
                shape: row.shape,
                ..Chip::default()
            };
            // One unreadable row loses nothing else, and its cause is kept for the row to say.
            let (editor, unreadable) = match rule_list::editor_of(&chip) {
                Ok(editor) => (editor, None),
                Err(error) => (
                    ChipEditor::ReadOnly(rule_list::ReadOnly::Unreadable),
                    Some(error.to_string()),
                ),
            };
            Setting {
                editor,
                unreadable,
                pointer: row.pointer,
                title: row.title,
                value_json: row.value_json,
            }
        })
        .collect::<Vec<_>>();
    let mut targets = Vec::with_capacity(response.build_targets.len());
    if response.build_target_lines.len() != response.build_targets.len() {
        return Err(BridgeError::Rpc(format!(
            "{} build targets came with {} lines",
            response.build_targets.len(),
            response.build_target_lines.len()
        )));
    }
    if let Some(Json::Array(held)) = canonical.get("build_targets") {
        for (target, line) in held.iter().zip(&response.build_target_lines) {
            let anchor = target.get("anchor").cloned().ok_or_else(|| {
                BridgeError::Rpc("a target with no anchor, which removes nothing".to_owned())
            })?;
            let blueprint_id = match target.get("blueprint_id") {
                Some(Json::String(blueprint)) => blueprint.clone(),
                _ => String::new(),
            };
            targets.push(HeldTarget {
                blueprint_id,
                anchor,
                line: line.clone(),
            });
        }
    }
    let lists = response
        .lists
        .into_iter()
        .map(|row| {
            // A clear the gateway named no flag for clears nothing: refused, never guessed.
            if row.clear_pointer.is_empty() {
                return Err(BridgeError::Rpc(format!(
                    "the list `{}` came with no clear pointer",
                    row.title
                )));
            }
            Ok(HeldList {
                title: row.title,
                entry_lines: row.entry_lines,
                clear_pointer: row.clear_pointer,
                clear_title: row.clear_title,
                empty_title: row.empty_title,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((
        response.prose,
        BeaconDetail {
            id: id.to_owned(),
            mandate,
            priority,
            settings,
            targets,
            lists,
        },
    ))
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

/// The `gp.v1.MandateSettings` object that holds every object the setting at `pointer` sits in,
/// and not the setting: `/retreat_hp_pct` gives `{}`, `/build/repair_threshold_pct` gives
/// `{"build":{}}`. What a Visit & change number row is appended with, before `set_value_text`
/// writes the typed value into it (S3's `fixq`: the gateway reads typed text, the client none).
///
/// # Errors
///
/// [`PickError::NoPointer`] for a pointer that names no field.
pub fn setting_parents(pointer: &str) -> Result<Json, PickError> {
    let Some(rest) = pointer.strip_prefix('/') else {
        return Err(PickError::NoPointer);
    };
    let mut segments: Vec<String> = rest.split('/').map(unescape).collect();
    if segments.iter().any(String::is_empty) || segments.pop().is_none() {
        return Err(PickError::NoPointer);
    }
    Ok(segments
        .into_iter()
        .rev()
        .fold(Json::Object(Vec::new()), |inner, key| {
            Json::Object(vec![(key, inner)])
        }))
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

/// The interface row that empties `list`: one `set_mandate_settings` row writing its clear
/// pointer, as sent, `true` (S3's `edlists`). Whether the row is legal is the verifier's.
///
/// # Errors
///
/// [`PickError::NoPointer`] for a clear pointer that names no field.
pub fn clear_row(list: &HeldList) -> Result<Json, PickError> {
    Ok(settings_row(nested_setting(
        &list.clear_pointer,
        Json::Bool(true),
    )?))
}

/// The interface row that adds the Build target `target` as the gateway composed it
/// (`compose_wall_box`'s answer, a wall's box and its anchor), written as it came.
#[must_use]
pub fn add_composed_target_row(target: Json) -> Json {
    Json::Object(vec![(
        "add_build_target".to_owned(),
        Json::Object(vec![("target".to_owned(), target)]),
    )])
}

/// The value a group pick writes into a condition slot: `{"<name>": <skeleton>}`, from group
/// `which` as `get_vocabulary` sent it (`all`, `any`, `not`), whose items are then slots of
/// their own.
///
/// # Errors
///
/// As [`named_skeleton`], and [`PickError::NoSuchEntry`] for a group the vocabulary does not
/// hold.
pub fn group_value(vocabulary: &Vocabulary, which: usize) -> Result<Json, PickError> {
    named_skeleton(vocabulary.groups.get(which).ok_or(PickError::NoSuchEntry)?)
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
        // A duration is a number shown in the gateway's seconds (S3's `fixq-c`, item 7).
        assert!(matches!(
            &parameter(elapsed, "/ms/value").shape,
            ParameterShape::Number { unit, display_unit, .. } if unit == "ms" && display_unit == "s"
        ));
        let compared = parameter(elapsed, "/ms/op");
        assert!(
            matches!(&compared.shape, ParameterShape::Choice { values, titles } if values.first().map(String::as_str) == Some("\"LT\"") && titles.first().map(String::as_str) == Some("below"))
        );
        // Each list as sent: the Queue's own, every kind but the Spire locked by its rule; the
        // wall the one blueprint a box anchors; the three groups.
        let queue_locks: Vec<(&str, bool)> = words
            .queue_blueprints
            .iter()
            .map(|entry| (entry.name.as_str(), entry.locked.is_some()))
            .collect();
        assert!(
            queue_locks.contains(&("generator", true)),
            "{queue_locks:?}"
        );
        assert!(queue_locks.contains(&("wall", true)), "{queue_locks:?}");
        let shapes: Vec<(&str, AnchorShape)> = words
            .blueprints
            .iter()
            .map(|entry| (entry.name.as_str(), entry.anchor_shape))
            .collect();
        assert!(shapes.contains(&("wall", AnchorShape::Box)), "{shapes:?}");
        assert!(
            shapes.contains(&("generator", AnchorShape::Point)),
            "{shapes:?}"
        );
        let groups: Vec<&str> = words
            .groups
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(groups, ["all", "any", "not"]);
        assert_eq!(
            compact(&group_value(&words, 2).expect("not")),
            r#"{"not":{"item":{}}}"#
        );
        assert_eq!(group_value(&words, 3), Err(PickError::NoSuchEntry));
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
                values: vec!["true".to_owned(), "false".to_owned()],
                titles: vec!["yes".to_owned(), "no".to_owned()]
            }
        );
        // A percentage setting carries its value in its display unit, as sent.
        let retreat = detail
            .settings
            .iter()
            .find(|setting| setting.pointer == "/retreat_hp_pct")
            .expect("the setting");
        assert!(
            matches!(&retreat.editor, ChipEditor::Number { value_text, display_unit, .. } if value_text == "0" && display_unit == "%"),
            "{:?}",
            retreat.editor
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
        // Each target in the gateway's own words, never its anchor's JSON (S3's `fixq-c`).
        for target in &detail.targets {
            assert!(
                !target.line.is_empty() && !target.line.contains('{'),
                "{target:?}"
            );
        }
        let described = detail
            .targets
            .iter()
            .find(|target| compact(&target.anchor).contains("\"vent\""))
            .expect("a target on a described vent");
        assert_eq!(
            compact(&remove_target_row(described)),
            r#"{"remove_build_target":{"anchor":{"on":{"vent":{"rank":"NEAREST"}}}}}"#
        );
    }

    /// A picker's choice whose titles do not pair with its values refuses the whole answer (fix
    /// pass 1): the picker's line says why, and no value's JSON is drawn in a title's place.
    #[test]
    fn a_choice_whose_titles_do_not_pair_refuses_the_vocabulary() {
        let answer = |titles: &str| {
            json::read(&format!(
                r#"{{"families":[{{"family":"f","title":"F","predicates":[{{"name":"p","title":"P","parameters":[{{"pointer":"/op","title":"Compared","shape":{{"choice":{{"values":["\"LT\"","\"GT\""],"titles":{titles}}}}}}}],"skeleton_json":"{{}}"}}]}}]}}"#
            ))
            .expect("json")
        };
        assert!(
            vocabulary_of(&answer(r#"["below"]"#)).is_err(),
            "one title short"
        );
        assert!(vocabulary_of(&answer("[]")).is_err(), "no titles");
        assert!(vocabulary_of(&answer(r#"["below","above"]"#)).is_ok());
    }

    /// The Remove list draws each target's line beside it, so lines that do not pair with the
    /// targets one for one are refused whole: a line beside the wrong target removes the wrong one.
    #[test]
    fn target_lines_that_do_not_pair_with_the_targets_are_refused() {
        let short = json::read(
            r#"{"mandate":"build","build_targets":[{"blueprint_id":"generator","anchor":{"voxel":{"x":1,"y":2,"z":3}}}],"build_target_lines":[]}"#,
        )
        .expect("json");
        assert!(beacon_detail_of("b_00", &short).is_err());
        let paired = json::read(
            r#"{"mandate":"build","build_targets":[{"blueprint_id":"generator","anchor":{"voxel":{"x":1,"y":2,"z":3}}}],"build_target_lines":["Generator at voxel 1, 2, 3"]}"#,
        )
        .expect("json");
        let (_, detail) = beacon_detail_of("b_00", &paired).expect("reads");
        assert_eq!(
            detail.targets.first().map(|target| target.line.as_str()),
            Some("Generator at voxel 1, 2, 3")
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
        assert_eq!(
            compact(&settings_row(
                setting_parents("/retreat_hp_pct").expect("a pointer")
            )),
            r#"{"set_mandate_settings":{}}"#
        );
        assert_eq!(
            compact(&setting_parents("/build/repair_threshold_pct").expect("a pointer")),
            r#"{"build":{}}"#
        );
        assert_eq!(setting_parents("/"), Err(PickError::NoPointer));
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
            anchor_shape: AnchorShape::Point,
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
                line: String::new(),
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
