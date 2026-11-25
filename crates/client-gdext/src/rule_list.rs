// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The rule list as the gateway rendered it (S3's plan, task `ed3b`; spec section 13): each
//! line of `render_plan`'s prose, cut into plain text and the chips the gateway placed on it,
//! each rule and route step with the lines that are its own, and the JSON Patch each edit of
//! the list composes.
//!
//! **Nothing here parses a sentence.** Where a chip stands, what may be written there and
//! what it holds now are `RenderPlanResponse.rules`' (`gp.api.v1.Chip`: its line, its span in
//! code points, its `ValueShape` and its `value_json`); this module cuts the line at the spans
//! the gateway sent and nowhere else. A chip's editor follows its shape as sent: the value
//! field (text, read by the bridge; the bounds sent are said beside it), the values an enum's shape lists, or a pick on the map
//! within the arms the gateway names (AGENTS.md section 3 rule 4: no client holds the
//! per-position rules). Every edit is one JSON Patch the client composes from a click (a
//! `replace` at the chip's pointer, a `move` of one rule onto another's place), and the
//! patched text is `patch_plan`'s answer.
//!
//! A number is shown and typed in its shape's display unit, seconds for a duration (the owner's
//! ruling of 2026-11-15, decisions-log item 160, kept by the gateway converting; S3's `fixq`): the
//! field pre-fills with the chip's `value_text` and sends what was typed to `set_value_text`, so
//! the client reads, converts and compares no number. Whether a value lies outside its shape is
//! the chip's `outside`, as sent; a choice is drawn by its `titles` and writes its value as sent;
//! every chip's edit is the op its `pick_op` names (`add` for a value at its proto3 default).
//!
//! A reference to a step label or a rule id is picked from the playbook, not from the map
//! (part 2, [`ChipEditor::Name`]): from the labels or rule ids the player's own file holds, in
//! file order ([`crate::vocabulary::step_labels`], [`crate::vocabulary::rule_ids`]), and the
//! gateway's check says whether the pick is legal.

use pharmakos_proto::gp::api::v1::condition_slot::{PickOp, State};
use pharmakos_proto::gp::api::v1::reference_shape::Kind;
use pharmakos_proto::gp::api::v1::value_shape::Shape;
use pharmakos_proto::gp::api::v1::{Chip, RenderPlanResponse};
use pharmakos_proto::json::{self, Json};

use crate::editor::Target;
use crate::enums;
use crate::error::BridgeError;
use crate::view::split_footer;

/// The diagnostic code whose two rules the connector joins: W0702, a rule an earlier rule
/// wins every time both are true (`crates/verifier/src/lint.rs`, `shadowed`; its `path` is
/// the later rule's condition and its one related path the earlier one's). The connector's
/// reading of the verifier's catalogue, as `editor::DORMANT_CODES` is the checkbox's.
pub const CONFLICT_CODE: &str = "W0702";

/// Why a chip is drawn and not edited.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReadOnly {
    /// A chip whose shape the gateway left unset, or set to a kind this build does not know.
    NoShape,
    /// A value that does not fit the shape sent with it (a choice whose titles do not pair with
    /// its values): drawn as it came, with the reason, and not edited.
    Unreadable,
}

impl ReadOnly {
    /// The name GDScript looks the words up by (`rule_chip_read_only_<name>`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::NoShape => "no_shape",
            Self::Unreadable => "unreadable",
        }
    }
}

/// What a map pick may name for a reference chip, as the gateway's shape says.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PickKind {
    /// A `gp.v1.BeaconRef`: a beacon by its id.
    Beacon,
    /// A `gp.v1.Location`: ground, a beacon's anchor, a site covering a feature.
    Location,
    /// A `gp.v1.FeatureRef`: a feature by its name.
    Feature,
}

impl PickKind {
    /// The name GDScript reads.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Beacon => "beacon",
            Self::Location => "location",
            Self::Feature => "feature",
        }
    }
}

/// What a name chip is picked from: the playbook's own names of that kind.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NameKind {
    /// A route step's label (`ReferenceShape.Kind.STEP_LABEL`).
    StepLabel,
    /// A rule's id (`ReferenceShape.Kind.HANDLER_ID`).
    RuleId,
}

impl NameKind {
    /// The name GDScript reads, and the state key its choices are listed under
    /// (`step_label` reads `step_labels`, `rule_id` reads `rule_ids`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::StepLabel => "step_label",
            Self::RuleId => "rule_id",
        }
    }
}

/// A chip's editor, by the shape the gateway sent.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ChipEditor {
    /// A number, typed as text in `display_unit` and read by the gateway (`set_value_text`).
    Number {
        /// The lowest value, as sent, in `unit` (an unset bound is one nothing sets).
        min: Option<i32>,
        /// The highest value, as sent, in `unit`.
        max: Option<i32>,
        /// The unit the value is held in and the bounds are in, as sent.
        unit: String,
        /// The unit it is shown and typed in, as sent: `s` for an `ms` value.
        display_unit: String,
        /// The value now in `display_unit`, as sent: what the field pre-fills with.
        value_text: String,
        /// `min` as it is shown and typed, in `display_unit`, as sent: present exactly when `min`
        /// is (an answer that pairs them otherwise is refused, [`editor_of`]).
        min_text: Option<String>,
        /// `max` as it is shown and typed, in `display_unit`, as sent, paired with `max` as
        /// `min_text` is with `min`.
        max_text: Option<String>,
        /// What a 0 does here, in the gateway's words, as sent (`IntegerShape.zero_title`):
        /// said beside the bounds before the player types a value, never decided here (S3's
        /// `zerotitle`, fix pass 1).
        zero_title: Option<String>,
        /// The value now in the gateway's words, as sent, when it has words of its own (a 0
        /// whose shape names what it does: "retreat off"); drawn beside `value_text`, never
        /// decided here (S3's `zerotitle`).
        value_title: Option<String>,
    },
    /// A closed list: each value the JSON text to write, and its title, as sent.
    Choice {
        /// The values, in the gateway's order.
        values: Vec<String>,
        /// Each value in the player's words, one per value, in the same order.
        titles: Vec<String>,
    },
    /// A reference picked on the map, within the arms the gateway names.
    Pick {
        /// What the reference is.
        kind: PickKind,
        /// The oneof arms legal at this pointer, as sent.
        arms: Vec<String>,
    },
    /// A name picked from the playbook's own (a step label, a rule id), listed in file order.
    Name(NameKind),
    /// Drawn, not edited.
    ReadOnly(ReadOnly),
}

/// One chip of the rule list (`RuleChip`, distinct from S1's targeting `Chip`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RuleChip {
    /// The value's JSON Pointer in the playbook: a `replace`'s path.
    pub pointer: String,
    /// The line of the prose it stands on.
    pub line: usize,
    /// Where it starts on that line, in code points, as sent.
    pub start: usize,
    /// The text it covers on that line, as the gateway rendered it.
    pub text: String,
    /// The value now, as JSON text in the playbook's canonical spelling, as sent.
    pub value_json: String,
    /// Its editor.
    pub editor: ChipEditor,
    /// Whether the value now lies outside its shape, as the gateway said (`Chip.outside`): the
    /// field then shows it as it is and says so. The client compares nothing.
    pub outside: bool,
    /// The op an edit of this chip sends at its pointer, as the gateway named it
    /// (`Chip.pick_op`): `replace` for a value the file writes, `add` for one at its proto3
    /// default. [`SlotOp::None`] for an op this build does not know, and then no edit goes out.
    pub op: SlotOp,
}

/// One rule or route step of the rendering.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RuleRow {
    /// Its JSON Pointer in the playbook: what a reordering's `move` names.
    pub pointer: String,
    /// The line of the prose that is its sentence.
    pub line: usize,
    /// How many lines are its own, its sentence included.
    pub line_count: usize,
}

impl RuleRow {
    /// The array it stands in (`/declarative/handlers`): rules move only within their own.
    #[must_use]
    pub fn parent(&self) -> &str {
        match self.pointer.rsplit_once('/') {
            Some((parent, _)) => parent,
            None => "",
        }
    }

    /// Whether line `line` of the prose is one of this rule's own.
    #[must_use]
    pub fn holds_line(&self, line: usize) -> bool {
        line.checked_sub(self.line)
            .is_some_and(|after| after < self.line_count)
    }

    /// Whether `pointer` is this rule's or inside it.
    #[must_use]
    pub fn holds(&self, pointer: &str) -> bool {
        pointer == self.pointer
            || pointer
                .strip_prefix(self.pointer.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
    }
}

/// What a condition slot holds, as the gateway sent it (`gp.api.v1.ConditionSlot.State`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    /// No condition there.
    Empty,
    /// One predicate.
    Leaf,
    /// `all`, `any` or `not`: its items follow as slots of their own. Not a pick target.
    Group,
    /// A state the gateway left unset, or one this build does not know.
    Unknown,
}

impl SlotState {
    /// The name GDScript reads.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Leaf => "leaf",
            Self::Group => "group",
            Self::Unknown => "unknown",
        }
    }
}

/// The JSON Patch op a pick at a slot sends, as the gateway names it
/// (`gp.api.v1.ConditionSlot.PickOp`): the client never chooses one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotOp {
    /// Nothing is at the pointer: the patch inserts the member.
    Add,
    /// Something is at the pointer: the patch replaces it, an array item in place.
    Replace,
    /// No op: a group slot, or an op this build does not know. No pick goes out.
    None,
}

impl SlotOp {
    /// The name GDScript reads.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Replace => "replace",
            Self::None => "none",
        }
    }
}

/// One condition slot of the rendering: where a condition stands or may stand, in the
/// gateway's words, and the op a pick there sends.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RuleSlot {
    /// Its JSON Pointer, as sent.
    pub pointer: String,
    /// Its name in the player's words, as sent ("when, item 1 of all").
    pub title: String,
    /// The line of its rule's sentence, as sent.
    pub line: usize,
    /// The rule it was sent under, by its index in [`RuleList::rules`].
    pub rule: Option<usize>,
    /// What it holds.
    pub state: SlotState,
    /// The predicate or group it holds, as sent; empty for an empty slot.
    pub predicate: String,
    /// The op a pick there sends.
    pub op: SlotOp,
}

impl RuleSlot {
    /// Whether the picker is offered on this slot: an empty or a leaf slot whose op the gateway
    /// named. A group, a state or an op this build does not know is no pick target.
    #[must_use]
    pub const fn pickable(&self) -> bool {
        matches!(self.state, SlotState::Empty | SlotState::Leaf)
            && matches!(self.op, SlotOp::Add | SlotOp::Replace)
    }
}

/// A run of one line: plain text, or the text of the chip whose index it carries.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Segment {
    /// The text, exactly as the line has it.
    pub text: String,
    /// The chip it is, by its index in [`RuleList::chips`]; `None` for plain text.
    pub chip: Option<usize>,
}

/// The rule list for one text: the prose's lines, the rules, and the chips.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct RuleList {
    /// The prose, one entry per line, each as it came.
    pub lines: Vec<String>,
    /// Each line cut at the chips on it.
    pub segments: Vec<Vec<Segment>>,
    /// The rules and route steps, in file order.
    pub rules: Vec<RuleRow>,
    /// The chips, in the order the answer lists them.
    pub chips: Vec<RuleChip>,
    /// The condition slots, in the order the answer lists them.
    pub slots: Vec<RuleSlot>,
}

impl RuleList {
    /// The rule whose sentence is line `line`, if one is.
    #[must_use]
    pub fn rule_at(&self, line: usize) -> Option<usize> {
        self.rules.iter().position(|rule| rule.line == line)
    }

    /// The rule that holds `pointer`, if one does: the innermost, so a rule's body step is
    /// not taken for the rule.
    #[must_use]
    pub fn rule_holding(&self, pointer: &str) -> Option<usize> {
        self.rules
            .iter()
            .enumerate()
            .filter(|(_, rule)| rule.holds(pointer))
            .max_by_key(|(_, rule)| rule.pointer.len())
            .map(|(index, _)| index)
    }
}

/// A `render_plan` answer as the rule list.
///
/// # Errors
///
/// [`BridgeError::Schema`] when the answer is not a `RenderPlanResponse`, and
/// [`BridgeError::Rpc`] when a rule or a chip stands outside the prose, a chip's span is not
/// inside its line, or two chips on one line overlap: the cut is the gateway's, and an answer
/// that cannot be cut where it says is refused whole rather than drawn wrong.
pub fn rule_list_of(result: &Json) -> Result<RuleList, BridgeError> {
    let (answer, _) = split_footer(result);
    let canonical = enums::canonical("gp.api.v1.RenderPlanResponse", &answer);
    let response: RenderPlanResponse = json::decode_json(&canonical)?;
    let lines: Vec<String> = response.prose.lines().map(str::to_owned).collect();
    let mut rules = Vec::with_capacity(response.rules.len());
    let mut chips = Vec::new();
    let mut slots = Vec::new();
    for (number, rule) in response.rules.iter().enumerate() {
        let line = index(rule.line, "a rule's line")?;
        let line_count = index(rule.line_count, "a rule's line count")?;
        let last = line
            .checked_add(line_count)
            .ok_or_else(|| BridgeError::Rpc("a rule's lines overflow".to_owned()))?;
        if line_count == 0 || last > lines.len() {
            return Err(BridgeError::Rpc(format!(
                "the rule {} stands on lines {line}+{line_count} of {}",
                rule.pointer,
                lines.len()
            )));
        }
        rules.push(RuleRow {
            pointer: rule.pointer.clone(),
            line,
            line_count,
        });
        for chip in &rule.chips {
            chips.push(rule_chip(chip, &lines)?);
        }
        for slot in &rule.slots {
            slots.push(RuleSlot {
                pointer: slot.pointer.clone(),
                title: slot.title.clone(),
                line: index(slot.line, "a slot's line")?,
                rule: Some(number),
                state: match State::try_from(slot.state) {
                    Ok(State::Empty) => SlotState::Empty,
                    Ok(State::Leaf) => SlotState::Leaf,
                    Ok(State::Group) => SlotState::Group,
                    Ok(State::Unspecified) | Err(_) => SlotState::Unknown,
                },
                predicate: slot.predicate.clone(),
                op: match PickOp::try_from(slot.pick_op) {
                    Ok(PickOp::Add) => SlotOp::Add,
                    Ok(PickOp::Replace) => SlotOp::Replace,
                    Ok(PickOp::Unspecified) | Err(_) => SlotOp::None,
                },
            });
        }
    }
    let segments = cut(&lines, &chips)?;
    Ok(RuleList {
        lines,
        segments,
        rules,
        chips,
        slots,
    })
}

/// The rule list from one `render_plan` answer's text: a whole JSON-RPC answer, or its result.
///
/// # Errors
///
/// [`BridgeError::Schema`] for text that is not JSON, [`BridgeError::Rpc`] for a refusal, and
/// whatever [`rule_list_of`] refuses.
pub fn rule_list_of_text(text: &str) -> Result<RuleList, BridgeError> {
    let frame = json::read(text)?;
    if frame.get("error").is_some() {
        return Err(BridgeError::Rpc(
            "the answer is a refusal, not a rendering".to_owned(),
        ));
    }
    match frame.get("result") {
        Some(result) => rule_list_of(result),
        None => rule_list_of(&frame),
    }
}

/// One chip as the rule list holds it: its text cut from its line at the span sent.
fn rule_chip(chip: &Chip, lines: &[String]) -> Result<RuleChip, BridgeError> {
    let line = index(chip.line, "a chip's line")?;
    let (start, end) = (
        index(chip.start, "a chip's start")?,
        index(chip.end, "a chip's end")?,
    );
    let text = lines
        .get(line)
        .and_then(|on| span(on, start, end))
        .ok_or_else(|| {
            BridgeError::Rpc(format!(
                "the chip {} spans {start}..{end} of line {line}, which it is not inside",
                chip.pointer
            ))
        })?;
    Ok(RuleChip {
        pointer: chip.pointer.clone(),
        line,
        start,
        text,
        value_json: chip.value_json.clone(),
        editor: editor_of(chip)?,
        outside: chip.outside,
        op: op_of(chip.pick_op),
    })
}

/// A number's bound as the gateway sent its text: the text when both the bound and its text are
/// sent, `None` when neither is.
///
/// # Errors
///
/// [`BridgeError::Rpc`] when one is sent without the other (S3's `zerotitle`).
pub(crate) fn bound_text(
    pointer: &str,
    which: &str,
    bound: Option<i32>,
    text: &str,
) -> Result<Option<String>, BridgeError> {
    match (bound, text.is_empty()) {
        (Some(_), false) => Ok(Some(text.to_owned())),
        (None, true) => Ok(None),
        (Some(_), true) => Err(BridgeError::Rpc(format!(
            "the number at {pointer} sends its {which} bound without the bound's text"
        ))),
        (None, false) => Err(BridgeError::Rpc(format!(
            "the number at {pointer} sends text for a {which} bound it does not set"
        ))),
    }
}

/// The refusal of a value title the chip's shape does not name as its zero's.
fn untitled(pointer: &str) -> BridgeError {
    BridgeError::Rpc(format!(
        "the value at {pointer} carries a title its shape does not name for a zero"
    ))
}

/// The editor a chip's shape asks for: a rule-list chip's, and a beacon setting's, which shares
/// the shape (`gp.api.v1.ValueShape`: "one editor per shape").
///
/// # Errors
///
/// [`BridgeError::Rpc`] for a choice whose titles do not pair with its values, one for one; for
/// a number whose bound and the bound's text are not both sent or both left out; and for a value
/// title that is not its shape's zero title (a title on a non-number, or one the shape does not
/// name). Each is the gateway's answer disagreeing with itself, refused rather than drawn with a
/// piece missing (S3's `zerotitle`).
pub(crate) fn editor_of(chip: &Chip) -> Result<ChipEditor, BridgeError> {
    let Some(shape) = chip.shape.as_ref().and_then(|shape| shape.shape.as_ref()) else {
        return if chip.value_title.is_empty() {
            Ok(ChipEditor::ReadOnly(ReadOnly::NoShape))
        } else {
            Err(untitled(&chip.pointer))
        };
    };
    if !chip.value_title.is_empty() && !matches!(shape, Shape::Integer(_)) {
        return Err(untitled(&chip.pointer));
    }
    Ok(match shape {
        Shape::Integer(integer) => {
            let min_text = bound_text(&chip.pointer, "least", integer.min, &integer.min_text)?;
            let max_text = bound_text(&chip.pointer, "most", integer.max, &integer.max_text)?;
            let value_title = if chip.value_title.is_empty() {
                None
            } else if chip.value_title == integer.zero_title {
                Some(chip.value_title.clone())
            } else {
                return Err(untitled(&chip.pointer));
            };
            ChipEditor::Number {
                min: integer.min,
                max: integer.max,
                unit: integer.unit.clone(),
                display_unit: integer.display_unit.clone(),
                value_text: chip.value_text.clone(),
                min_text,
                max_text,
                zero_title: (!integer.zero_title.is_empty()).then(|| integer.zero_title.clone()),
                value_title,
            }
        }
        Shape::Choice(choice) => {
            if choice.titles.len() != choice.values.len() {
                return Err(BridgeError::Rpc(format!(
                    "the choice at {} names {} values and {} titles",
                    chip.pointer,
                    choice.values.len(),
                    choice.titles.len()
                )));
            }
            ChipEditor::Choice {
                values: choice.values.clone(),
                titles: choice.titles.clone(),
            }
        }
        Shape::Reference(reference) => {
            let kind = match Kind::try_from(reference.kind) {
                Ok(Kind::Beacon) => PickKind::Beacon,
                Ok(Kind::Location) => PickKind::Location,
                Ok(Kind::Feature) => PickKind::Feature,
                Ok(Kind::StepLabel) => return Ok(ChipEditor::Name(NameKind::StepLabel)),
                Ok(Kind::HandlerId) => return Ok(ChipEditor::Name(NameKind::RuleId)),
                Ok(Kind::Unspecified) | Err(_) => {
                    return Ok(ChipEditor::ReadOnly(ReadOnly::NoShape));
                }
            };
            ChipEditor::Pick {
                kind,
                arms: reference.arms.clone(),
            }
        }
    })
}

/// The op a wire `PickOp` names; [`SlotOp::None`] for unset or unknown, so no edit is guessed.
pub(crate) fn op_of(pick_op: i32) -> SlotOp {
    match PickOp::try_from(pick_op) {
        Ok(PickOp::Add) => SlotOp::Add,
        Ok(PickOp::Replace) => SlotOp::Replace,
        Ok(PickOp::Unspecified) | Err(_) => SlotOp::None,
    }
}

/// A wire index as a `usize`.
fn index(value: u32, what: &str) -> Result<usize, BridgeError> {
    usize::try_from(value).map_err(|_| BridgeError::Rpc(format!("{what} is {value}")))
}

/// The code points `start..end` of `line`, or `None` when the span is empty or not inside it.
fn span(line: &str, start: usize, end: usize) -> Option<String> {
    let length = end.checked_sub(start).filter(|length| *length > 0)?;
    let count = line.chars().count();
    (end <= count).then(|| line.chars().skip(start).take(length).collect())
}

/// Every line cut at the chips on it, in the order they stand.
fn cut(lines: &[String], chips: &[RuleChip]) -> Result<Vec<Vec<Segment>>, BridgeError> {
    lines
        .iter()
        .enumerate()
        .map(|(number, line)| {
            let mut on: Vec<(usize, &RuleChip)> = chips
                .iter()
                .enumerate()
                .filter(|(_, chip)| chip.line == number)
                .collect();
            on.sort_by_key(|(_, chip)| chip.start);
            let characters: Vec<char> = line.chars().collect();
            let mut segments = Vec::new();
            let mut at = 0_usize;
            for (index, chip) in on {
                let start = chip.start;
                let length = chip.text.chars().count();
                let end = start.checked_add(length).ok_or_else(|| overlap(number))?;
                let before = characters.get(at..start).ok_or_else(|| overlap(number))?;
                if !before.is_empty() {
                    segments.push(Segment {
                        text: before.iter().collect(),
                        chip: None,
                    });
                }
                segments.push(Segment {
                    text: chip.text.clone(),
                    chip: Some(index),
                });
                at = end;
            }
            let rest = characters.get(at..).ok_or_else(|| overlap(number))?;
            if !rest.is_empty() || segments.is_empty() {
                segments.push(Segment {
                    text: rest.iter().collect(),
                    chip: None,
                });
            }
            Ok(segments)
        })
        .collect()
}

/// Two chips on line `line` overlap, or one runs past its end.
fn overlap(line: usize) -> BridgeError {
    BridgeError::Rpc(format!("the chips on line {line} overlap"))
}

/// Why a map pick names nothing for a chip.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PickRefusal {
    /// What was clicked is not what the chip names (ground for a beacon).
    NotThisKind,
    /// The gateway does not list the arm the click would write (`covering` where only a
    /// place-beacon step takes one): the arm's name.
    ArmNotHere(&'static str),
}

impl PickRefusal {
    /// The name GDScript looks the words up by (`status_pick_<name>`).
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::NotThisKind => "not_this_kind",
            Self::ArmNotHere(_) => "arm_not_here",
        }
    }
}

/// The value a map pick writes for a reference chip of `kind`, in the arm the click names,
/// when the gateway lists that arm here. Composed from the click (AGENTS.md section 3 rule 4:
/// "the client may compose a JSON Patch from a click"); whether it is legal is the verifier's
/// next report.
///
/// # Errors
///
/// [`PickRefusal`] when the click names nothing this chip takes.
pub fn picked_value(kind: PickKind, arms: &[String], target: &Target) -> Result<Json, PickRefusal> {
    let (arm, value) = match (kind, target) {
        (PickKind::Location, Target::Voxel(at)) => match crate::editor::voxel_location(*at) {
            Json::Object(mut voxel) => match voxel.pop() {
                Some((_, inner)) => ("voxel", inner),
                None => return Err(PickRefusal::NotThisKind),
            },
            _ => return Err(PickRefusal::NotThisKind),
        },
        (PickKind::Location, Target::Beacon(id)) => ("beacon_anchor", by_name("beacon_id", id)),
        (PickKind::Location, Target::Feature(id)) => ("covering", by_name("feature_id", id)),
        (PickKind::Beacon, Target::Beacon(id)) => ("beacon_id", Json::String(id.clone())),
        (PickKind::Feature, Target::Feature(id)) => ("feature_id", Json::String(id.clone())),
        _ => return Err(PickRefusal::NotThisKind),
    };
    if !arms.iter().any(|listed| listed == arm) {
        return Err(PickRefusal::ArmNotHere(arm));
    }
    Ok(Json::Object(vec![(arm.to_owned(), value)]))
}

/// `{"<key>": "<value>"}`.
fn by_name(key: &str, value: &str) -> Json {
    Json::Object(vec![(key.to_owned(), Json::String(value.to_owned()))])
}

/// The RFC 6902 patch that writes `value` at `pointer`: one chip's edit.
#[must_use]
pub fn replace_patch(pointer: &str, value: Json) -> String {
    op_patch("replace", pointer, value)
}

/// The RFC 6902 patch a chip's edit sends: `value` written at `pointer` with the op the gateway
/// named for the chip ([`RuleChip::op`]); `None` for no op, which sends nothing.
#[must_use]
pub fn chip_patch(op: SlotOp, pointer: &str, value: Json) -> Option<String> {
    match op {
        SlotOp::Add => Some(op_patch("add", pointer, value)),
        SlotOp::Replace => Some(op_patch("replace", pointer, value)),
        SlotOp::None => None,
    }
}

/// The RFC 6902 patch that removes the rule or step at `pointer`: Remove on the rule list.
#[must_use]
pub fn remove_patch(pointer: &str) -> String {
    json::write(&Json::Array(vec![Json::Object(vec![
        ("op".to_owned(), Json::String("remove".to_owned())),
        ("path".to_owned(), Json::String(pointer.to_owned())),
    ])]))
}

fn op_patch(op: &str, pointer: &str, value: Json) -> String {
    json::write(&Json::Array(vec![Json::Object(vec![
        ("op".to_owned(), Json::String(op.to_owned())),
        ("path".to_owned(), Json::String(pointer.to_owned())),
        ("value".to_owned(), value),
    ])]))
}

/// The RFC 6902 patch that moves the rule at `from` to the place of the rule at `to`, in the
/// same array: a drag of one rule onto another.
#[must_use]
pub fn move_patch(from: &str, to: &str) -> String {
    json::write(&Json::Array(vec![Json::Object(vec![
        ("op".to_owned(), Json::String("move".to_owned())),
        ("from".to_owned(), Json::String(from.to_owned())),
        ("path".to_owned(), Json::String(to.to_owned())),
    ])]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gateway's own committed answer for the Expand & Mine demo, chips and all.
    const EXPAND_AND_MINE: &str = include_str!(
        "../../../tests/golden/gateway/demo_expand_and_mine_render/expected.response.json"
    );

    fn golden() -> RuleList {
        let envelope = json::read(EXPAND_AND_MINE).expect("json");
        let result = envelope.get("result").expect("a result");
        rule_list_of(result).expect("the golden reads")
    }

    fn compact(text: &str) -> String {
        text.split_whitespace().collect()
    }

    #[test]
    fn the_golden_is_cut_where_the_gateway_placed_its_chips() {
        let list = golden();
        assert_eq!(list.lines.len(), list.segments.len());
        for (line, segments) in list.lines.iter().zip(&list.segments) {
            let joined: String = segments
                .iter()
                .map(|segment| segment.text.as_str())
                .collect();
            assert_eq!(&joined, line, "the cut drops and adds nothing");
        }
        let first = list.rules.first().expect("a rule");
        assert_eq!(first.pointer, "/declarative/route/0");
        assert_eq!((first.line, first.line_count), (5, 3));
        assert_eq!(first.parent(), "/declarative/route");
        let priority = list
            .chips
            .iter()
            .position(|chip| chip.pointer.ends_with("/initial/priority"))
            .expect("the priority chip");
        let chip = list.chips.get(priority).expect("a chip");
        assert_eq!(chip.text, "normal");
        assert_eq!(chip.value_json, "\"NORMAL\"");
        assert_eq!(
            chip.editor,
            ChipEditor::Choice {
                values: vec![
                    "\"LOW\"".to_owned(),
                    "\"NORMAL\"".to_owned(),
                    "\"HIGH\"".to_owned()
                ],
                titles: vec!["low".to_owned(), "normal".to_owned(), "high".to_owned()]
            }
        );
        assert_eq!(chip.op, SlotOp::Replace, "the op is the gateway's");
        assert!(!chip.outside);
        assert!(
            list.segments
                .get(5)
                .expect("line 5")
                .iter()
                .any(|segment| segment.chip == Some(priority) && segment.text == "normal")
        );
    }

    #[test]
    fn each_shape_gets_its_editor_and_a_duration_is_typed_in_seconds() {
        let list = golden();
        let by = |suffix: &str| {
            list.chips
                .iter()
                .find(|chip| chip.pointer.ends_with(suffix))
                .unwrap_or_else(|| panic!("no chip {suffix}"))
                .editor
                .clone()
        };
        assert_eq!(
            by("/cmdr_hp_pct/pct"),
            ChipEditor::Number {
                min: Some(0),
                max: Some(100),
                unit: "%".to_owned(),
                display_unit: "%".to_owned(),
                value_text: "40".to_owned(),
                min_text: Some("0".to_owned()),
                max_text: Some("100".to_owned()),
                zero_title: None,
                value_title: None,
            }
        );
        // A duration is a number like any other, shown and typed in the gateway's seconds
        // (S3's `fixq-c`, item 7): it was drawn read-only while the wire had no display unit.
        assert_eq!(
            by("/route/0/timeout_ms"),
            ChipEditor::Number {
                min: Some(0),
                max: None,
                unit: "ms".to_owned(),
                display_unit: "s".to_owned(),
                value_text: "120".to_owned(),
                min_text: Some("0".to_owned()),
                max_text: None,
                zero_title: None,
                value_title: None,
            }
        );
        assert_eq!(
            by("/route/1/move/to"),
            ChipEditor::Pick {
                kind: PickKind::Location,
                arms: vec![
                    "voxel".to_owned(),
                    "beacon_anchor".to_owned(),
                    "safest".to_owned()
                ]
            }
        );
    }

    #[test]
    fn a_rule_holds_its_own_pointers_and_the_innermost_rule_wins() {
        let list = golden();
        let rule = list
            .rule_holding("/declarative/handlers/0/when")
            .and_then(|index| list.rules.get(index))
            .expect("the rule");
        assert_eq!(rule.pointer, "/declarative/handlers/0");
        assert_eq!(list.rule_holding("/declarative/handlers/01"), None);
        assert_eq!(list.rule_at(5), Some(0));
    }

    #[test]
    fn a_chip_outside_its_line_or_over_another_is_refused_whole() {
        let base = r#"{"prose":"Route\n  1. go: walk to x.\n","rules":[{"pointer":"/declarative/route/0","line":1,"line_count":1,"chips":[CHIPS]}]}"#;
        for chips in [
            r#"{"pointer":"/a","line":1,"start":10,"end":99,"value_json":"1"}"#,
            r#"{"pointer":"/a","line":7,"start":0,"end":1,"value_json":"1"}"#,
            r#"{"pointer":"/a","line":1,"start":4,"end":4,"value_json":"1"}"#,
            r#"{"pointer":"/a","line":1,"start":2,"end":6,"value_json":"1"},{"pointer":"/b","line":1,"start":4,"end":8,"value_json":"1"}"#,
        ] {
            let text = base.replace("CHIPS", chips);
            assert!(
                rule_list_of(&json::read(&text).expect("json")).is_err(),
                "{chips}"
            );
        }
        let bad_rule = r#"{"prose":"a\n","rules":[{"pointer":"/declarative/route/0","line":0,"line_count":2}]}"#;
        assert!(rule_list_of(&json::read(bad_rule).expect("json")).is_err());
    }

    /// A number's bounds come with their text in the unit it is typed in, and its value with the
    /// gateway's words for it when it has words of its own; each piece is carried as sent, and one
    /// sent without its pair is refused, never drawn with the piece missing (S3's `zerotitle`).
    #[test]
    fn a_numbers_bound_texts_and_value_title_are_the_gateways_and_paired() {
        let answer = |chip: &str| {
            json::read(&format!(
                r#"{{"prose":"Route\n  1. go: retreat off (0%).\n","rules":[{{"pointer":"/declarative/route/0","line":1,"line_count":1,"chips":[{chip}]}}]}}"#
            ))
            .expect("json")
        };
        let chip = |shape: &str, title: &str| {
            format!(
                r#"{{"pointer":"/r","line":1,"start":22,"end":23,"shape":{shape},"value_json":"0","value_text":"0","value_title":"{title}","pick_op":"replace"}}"#
            )
        };
        let titled = r#"{"integer":{"min":0,"max":100,"unit":"%","display_unit":"%","zero_title":"retreat off","min_text":"0","max_text":"100"}}"#;
        let read = rule_list_of(&answer(&chip(titled, "retreat off"))).expect("reads");
        assert_eq!(
            read.chips.first().map(|chip| chip.editor.clone()),
            Some(ChipEditor::Number {
                min: Some(0),
                max: Some(100),
                unit: "%".to_owned(),
                display_unit: "%".to_owned(),
                value_text: "0".to_owned(),
                min_text: Some("0".to_owned()),
                max_text: Some("100".to_owned()),
                zero_title: Some("retreat off".to_owned()),
                value_title: Some("retreat off".to_owned()),
            })
        );
        // No title sent for the value: none drawn, and the client tests no value to make one;
        // the shape's words for a 0 are still carried, for the bounds note to say before the
        // player types one (fix pass 1, item 3).
        let read = rule_list_of(&answer(&chip(titled, ""))).expect("reads");
        assert!(matches!(
            read.chips.first().map(|chip| &chip.editor),
            Some(ChipEditor::Number {
                value_title: None,
                zero_title: Some(zero),
                ..
            }) if zero == "retreat off"
        ));
        for (shape, title, why) in [
            (
                r#"{"integer":{"min":0,"max":100,"unit":"%","display_unit":"%","max_text":"100"}}"#,
                "",
                "a least without its text",
            ),
            (
                r#"{"integer":{"max":100,"unit":"%","display_unit":"%","min_text":"0","max_text":"100"}}"#,
                "",
                "text for a least it does not set",
            ),
            (
                r#"{"integer":{"min":0,"max":100,"unit":"%","display_unit":"%","min_text":"0"}}"#,
                "",
                "a most without its text",
            ),
            (
                r#"{"integer":{"min":0,"max":100,"unit":"%","display_unit":"%","min_text":"0","max_text":"100"}}"#,
                "retreat off",
                "a title its shape does not name",
            ),
            (titled, "no picket", "a title that is not the shape's"),
            (
                r#"{"choice":{"values":["true"],"titles":["yes"]}}"#,
                "retreat off",
                "a title on a choice",
            ),
        ] {
            assert!(rule_list_of(&answer(&chip(shape, title))).is_err(), "{why}");
        }
    }

    /// Whether a value lies outside its shape is the gateway's `outside`, carried as sent, and the
    /// op a chip's edit sends is its `pick_op`: `add` for a value at its proto3 default, and no op
    /// for one this build does not know (S3's `fixq-c`, items 11 and 14). A choice whose titles do
    /// not pair with its values is refused whole. Nothing here compares a value with a bound.
    #[test]
    fn outside_and_the_op_are_the_gateways_and_a_choice_pairs_its_titles() {
        let answer = |chip: &str| {
            json::read(&format!(
                r#"{{"prose":"Route\n  1. go: wait 12 ticks.\n","rules":[{{"pointer":"/declarative/route/0","line":1,"line_count":1,"chips":[{chip}]}}]}}"#
            ))
            .expect("json")
        };
        let read = |chip: &str| {
            rule_list_of(&answer(chip))
                .expect("reads")
                .chips
                .first()
                .cloned()
                .expect("a chip")
        };
        let past = read(
            r#"{"pointer":"/declarative/route/0/wait/n","line":1,"start":16,"end":18,"shape":{"integer":{"max":5,"max_text":"5"}},"value_json":"12","value_text":"12","outside":true,"pick_op":"replace"}"#,
        );
        assert!(past.outside, "the gateway's word, carried");
        let inside = read(
            r#"{"pointer":"/declarative/route/0/wait/n","line":1,"start":16,"end":18,"shape":{"integer":{"max":5,"max_text":"5"}},"value_json":"12","value_text":"12","pick_op":"replace"}"#,
        );
        assert!(!inside.outside, "no comparison of the client's own");
        let default = read(
            r#"{"pointer":"/declarative/route/0/wait/n","line":1,"start":16,"end":18,"shape":{"integer":{}},"value_json":"0","value_text":"0","pick_op":"add"}"#,
        );
        assert_eq!(default.op, SlotOp::Add);
        let unknown = read(
            r#"{"pointer":"/declarative/route/0/wait/n","line":1,"start":16,"end":18,"shape":{"integer":{}},"value_json":"0","value_text":"0"}"#,
        );
        assert_eq!(unknown.op, SlotOp::None);
        assert!(
            rule_list_of(&answer(
                r#"{"pointer":"/a","line":1,"start":16,"end":18,"shape":{"choice":{"values":["\"A\"","\"B\""],"titles":["a"]}},"value_json":"\"A\"","pick_op":"replace"}"#
            ))
            .is_err(),
            "a title short"
        );
        let compact = |patch: &str| json::read(patch).map(|value| json::write(&value));
        assert_eq!(
            chip_patch(SlotOp::Add, "/a", Json::Number("3".to_owned()))
                .map(|patch| compact(&patch)),
            Some(compact(r#"[{"op":"add","path":"/a","value":3}]"#))
        );
        assert_eq!(chip_patch(SlotOp::None, "/a", Json::Null), None);
        assert_eq!(
            compact(&remove_patch("/declarative/handlers/1")),
            compact(r#"[{"op":"remove","path":"/declarative/handlers/1"}]"#)
        );
    }

    /// A slot takes a pick only when it is empty or a leaf with an op the gateway named: the
    /// golden's group (`when` holding `all`) does not, its two leaves and the empty `skip_if`s do.
    #[test]
    fn only_an_empty_or_a_leaf_slot_with_an_op_is_pickable() {
        let list = golden();
        let pickable = |pointer: &str| {
            list.slots
                .iter()
                .find(|slot| slot.pointer == pointer)
                .unwrap_or_else(|| panic!("no slot {pointer}"))
                .pickable()
        };
        assert!(!pickable("/declarative/handlers/0/when"));
        assert!(pickable("/declarative/handlers/0/when/all/items/0"));
        assert!(pickable("/declarative/route/0/skip_if"));
        let mut leaf = list
            .slots
            .iter()
            .find(|slot| slot.state == SlotState::Leaf)
            .cloned()
            .expect("a leaf");
        leaf.op = SlotOp::None;
        assert!(!leaf.pickable(), "a leaf with no op is no pick target");
        let mut group = leaf;
        group.state = SlotState::Group;
        group.op = SlotOp::Replace;
        assert!(
            !group.pickable(),
            "a group is no pick target whatever op it carries"
        );
        group.state = SlotState::Unknown;
        assert!(!group.pickable(), "nor is a state this build does not know");
    }

    /// An op or a state STRING this build does not know (the gateway writes lower-case names;
    /// the schema's own `ADD` is a name it knows) is the decode's own refusal of the whole
    /// rendering, never a slot drawn with a guessed op.
    #[test]
    fn an_unknown_op_or_state_string_refuses_the_rendering() {
        for (op, state) in [
            ("insert", "leaf"),
            ("Add", "leaf"),
            ("add", "branch"),
            ("PICK_OP_ADD", "STATE_LEAF"),
        ] {
            let text = format!(
                r#"{{"prose":"Rules\n  r1: when x.\n","rules":[{{"pointer":"/declarative/handlers/0","line":1,"line_count":1,"slots":[{{"pointer":"/declarative/handlers/0/when","title":"when","line":1,"state":"{state}","predicate":"cmdr_hp_pct","pick_op":"{op}"}}]}}]}}"#
            );
            assert!(
                rule_list_of(&json::read(&text).expect("json")).is_err(),
                "op {op}, state {state}"
            );
        }
    }

    #[test]
    fn the_patches_are_exact() {
        assert_eq!(
            compact(&replace_patch(
                "/declarative/handlers/0/when/all/items/0/cmdr_hp_pct/pct",
                Json::Number("35".to_owned())
            )),
            r#"[{"op":"replace","path":"/declarative/handlers/0/when/all/items/0/cmdr_hp_pct/pct","value":35}]"#
        );
        assert_eq!(
            compact(&move_patch(
                "/declarative/handlers/2",
                "/declarative/handlers/0"
            )),
            r#"[{"op":"move","from":"/declarative/handlers/2","path":"/declarative/handlers/0"}]"#
        );
    }

    #[test]
    fn a_map_pick_writes_the_arm_the_click_names_when_the_gateway_lists_it() {
        let location = [
            "voxel".to_owned(),
            "beacon_anchor".to_owned(),
            "covering".to_owned(),
        ];
        let written = |kind, arms: &[String], target| {
            picked_value(kind, arms, &target).map(|value| compact(&json::write(&value)))
        };
        assert_eq!(
            written(PickKind::Location, &location, Target::Voxel([1, 2, 3])),
            Ok(r#"{"voxel":{"x":1,"y":2,"z":3}}"#.to_owned())
        );
        assert_eq!(
            written(
                PickKind::Location,
                &location,
                Target::Beacon("b_01".to_owned())
            ),
            Ok(r#"{"beacon_anchor":{"beacon_id":"b_01"}}"#.to_owned())
        );
        assert_eq!(
            written(
                PickKind::Location,
                &location,
                Target::Feature("vent_1_2".to_owned())
            ),
            Ok(r#"{"covering":{"feature_id":"vent_1_2"}}"#.to_owned())
        );
        assert_eq!(
            written(
                PickKind::Location,
                location.get(..2).expect("two arms"),
                Target::Feature("vent_1_2".to_owned())
            ),
            Err(PickRefusal::ArmNotHere("covering"))
        );
        let beacon_id = ["beacon_id".to_owned()];
        assert_eq!(
            written(
                PickKind::Beacon,
                &beacon_id,
                Target::Beacon("b_00".to_owned())
            ),
            Ok(r#"{"beacon_id":"b_00"}"#.to_owned())
        );
        assert_eq!(
            written(PickKind::Beacon, &beacon_id, Target::Voxel([1, 2, 3])),
            Err(PickRefusal::NotThisKind)
        );
    }
}
