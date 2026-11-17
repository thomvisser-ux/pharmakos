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
//! the gateway sent and nowhere else. A chip's editor follows its shape as sent: a number
//! field within the bounds sent, the values an enum's shape lists, or a pick on the map
//! within the arms the gateway names (AGENTS.md section 3 rule 4: no client holds the
//! per-position rules). Every edit is one JSON Patch the client composes from a click (a
//! `replace` at the chip's pointer, a `move` of one rule onto another's place), and the
//! patched text is `patch_plan`'s answer.
//!
//! Two shapes are drawn but not edited here, each a gap in the wire rather than a choice of
//! the client's (the main session's rulings on `ed3b`'s pause report):
//!
//! * an integer whose unit is `ms`: the sentence shows the gateway's own rendering (`2:00`),
//!   and a number field would ask the player for raw milliseconds, which the client may not
//!   convert. It waits on a typed unit hint from the wire ([`ReadOnly::Milliseconds`]);
//! * a reference to a step label or a rule id, which is picked from the playbook and not
//!   from the map: part 2's picker ([`ReadOnly::NotOnTheMap`]).

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

/// The unit whose integers the rule list does not edit: a duration, which the gateway renders
/// for reading and which a number field would ask for in raw game milliseconds.
const MILLISECONDS: &str = "ms";

/// Why a chip is drawn and not edited.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReadOnly {
    /// An integer in milliseconds: it waits on a typed unit hint from the wire.
    Milliseconds,
    /// A reference picked from the playbook (a step label, a rule id), not from the map:
    /// part 2's picker.
    NotOnTheMap,
    /// A chip whose shape the gateway left unset, or set to a kind this build does not know.
    NoShape,
}

impl ReadOnly {
    /// The name GDScript looks the words up by (`rule_chip_read_only_<name>`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Milliseconds => "ms",
            Self::NotOnTheMap => "not_on_the_map",
            Self::NoShape => "no_shape",
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

/// A chip's editor, by the shape the gateway sent.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ChipEditor {
    /// A whole number, within the bounds sent (an unset bound is one nothing sets), in `unit`.
    Number {
        /// The lowest value, as sent.
        min: Option<i32>,
        /// The highest value, as sent.
        max: Option<i32>,
        /// The unit drawn beside the field, as sent.
        unit: String,
        /// The value now, from `value_json`.
        now: i64,
    },
    /// A closed list: each value the JSON text to write, as sent.
    Choice {
        /// The values, in the gateway's order.
        values: Vec<String>,
    },
    /// A reference picked on the map, within the arms the gateway names.
    Pick {
        /// What the reference is.
        kind: PickKind,
        /// The oneof arms legal at this pointer, as sent.
        arms: Vec<String>,
    },
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
    for rule in &response.rules {
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
    }
    let segments = cut(&lines, &chips)?;
    Ok(RuleList {
        lines,
        segments,
        rules,
        chips,
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
    })
}

/// The editor a chip's shape asks for.
fn editor_of(chip: &Chip) -> Result<ChipEditor, BridgeError> {
    let Some(shape) = chip.shape.as_ref().and_then(|shape| shape.shape.as_ref()) else {
        return Ok(ChipEditor::ReadOnly(ReadOnly::NoShape));
    };
    Ok(match shape {
        Shape::Integer(integer) if integer.unit == MILLISECONDS => {
            ChipEditor::ReadOnly(ReadOnly::Milliseconds)
        }
        Shape::Integer(integer) => ChipEditor::Number {
            min: integer.min,
            max: integer.max,
            unit: integer.unit.clone(),
            now: integer_of(&chip.pointer, &chip.value_json)?,
        },
        Shape::Choice(choice) => ChipEditor::Choice {
            values: choice.values.clone(),
        },
        Shape::Reference(reference) => {
            let kind = match Kind::try_from(reference.kind) {
                Ok(Kind::Beacon) => PickKind::Beacon,
                Ok(Kind::Location) => PickKind::Location,
                Ok(Kind::Feature) => PickKind::Feature,
                Ok(Kind::StepLabel | Kind::HandlerId) => {
                    return Ok(ChipEditor::ReadOnly(ReadOnly::NotOnTheMap));
                }
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

/// An integer chip's value now. A value at its proto3 default is absent from the file and
/// has no chip (`render.rs`), so a chip always carries one.
fn integer_of(pointer: &str, value_json: &str) -> Result<i64, BridgeError> {
    match json::read(value_json) {
        Ok(Json::Number(number)) => number
            .parse::<i64>()
            .map_err(|_| BridgeError::Rpc(format!("the chip {pointer} holds {value_json}"))),
        _ => Err(BridgeError::Rpc(format!(
            "the integer chip {pointer} holds {value_json}, which is not an integer"
        ))),
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
    json::write(&Json::Array(vec![Json::Object(vec![
        ("op".to_owned(), Json::String("replace".to_owned())),
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
                ]
            }
        );
        assert!(
            list.segments
                .get(5)
                .expect("line 5")
                .iter()
                .any(|segment| segment.chip == Some(priority) && segment.text == "normal")
        );
    }

    #[test]
    fn each_shape_gets_its_editor_and_milliseconds_stay_the_gateways_text() {
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
                now: 40
            }
        );
        assert_eq!(
            by("/route/0/timeout_ms"),
            ChipEditor::ReadOnly(ReadOnly::Milliseconds)
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
