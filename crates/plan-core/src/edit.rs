// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Two edits the editor asks for rather than composes (S3's `fixq`): a value
//! typed as text ([`set_value_text`]) and a new rule ([`new_rule`]).
//!
//! Each answers as `patch_plan` does: the patched file, comments and
//! formatting kept, and the undo [`crate::patch::apply`] hands back, never
//! one built by hand. Each composes one forward patch and applies it through
//! the same door a client's own patch goes through, so the strict reader, the
//! file's spelling of a field and the refusal of a field written twice all
//! hold for it too.
//!
//! # Why the gateway parses a typed value
//!
//! A duration is shown in seconds and held in game milliseconds, and the client
//! may not convert time (AGENTS.md section 3 rule 4; the owner's ruling of
//! 2026-11-15, kept by having the gateway convert). So every typed integer, a
//! duration or not, comes here as the text the player typed: one reader of
//! typed numbers ([`crate::shape::typed_value`]), not one in the client and
//! another here.

use pharmakos_proto::descriptor::{ScalarKind, schema};
use pharmakos_proto::json::grammar::{Said, quoted};
use pharmakos_proto::said;
use pharmakos_sim::rules::RulesTable;

use crate::context::PlanContext;
use crate::error::{Error, push_pointer, split_pointer};
use crate::render::PickOp;
use crate::shape::{Shape, Typed, typed_value};
use crate::strings as s;

/// Why [`set_value_text`] wrote nothing.
#[derive(Clone, PartialEq, Debug)]
pub enum SetValueError {
    /// The file has no canonical form, so no value in it has a shape yet.
    NoCanonicalForm(Error),
    /// Nothing at the pointer takes a typed number: no chip of an integer
    /// shape and no mandate setting of one stands there.
    NoNumberHere,
    /// The text is not a value of the shape there.
    Typed(Typed),
    /// The value is past what the field at the pointer holds (a `uint32`
    /// below zero, an `int32` past its range).
    DoesNotFit,
    /// The rendering or the patch refused: this crate's own error, or the
    /// patch's refusal of the file (the patch module's words).
    Refused(Error),
}

/// A new rule, written ([`new_rule`]).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NewRule {
    /// The patched file.
    pub playbook_jsonc: String,
    /// The undo, as text.
    pub inverse_json_patch: String,
    /// The new rule's JSON Pointer, `/declarative/handlers/<n>`.
    pub pointer: String,
    /// Its id.
    pub id: String,
}

/// Why [`new_rule`] wrote nothing.
#[derive(Clone, PartialEq, Debug)]
pub enum NewRuleError {
    /// The file has no canonical form, so its rules cannot be counted.
    NoCanonicalForm(Error),
    /// The playbook already holds the most rules the sim runs
    /// ([`pharmakos_sim::interpreter::MAX_HANDLERS`]), which the verifier's
    /// `E0113` reads too.
    Full(usize),
    /// The rules table lacks the block the new rule's firing limits read.
    Rules(Error),
    /// The patch refused the file.
    Refused(Error),
}

/// The patched file and the undo's text, as `patch_plan` answers.
pub type Patched = (String, String);

/// The value `typed` names at `pointer` of the playbook `playbook_jsonc`,
/// written there.
///
/// The shape there is the rendering's chip at exactly that pointer, which
/// names the patch op too ([`crate::render::Chip::pick`]); where no chip
/// stands, a mandate setting the rendering does not list (a
/// `set_mandate_settings` row's `/retreat_hp_pct`) takes the vocabulary's
/// shape for it, and an `add`. The value must also fit the field the pointer
/// names, by the schema's type.
///
/// # Errors
///
/// [`SetValueError`].
pub fn set_value_text(
    playbook_jsonc: &str,
    pointer: &str,
    typed: &str,
    context: &PlanContext,
    rules: &RulesTable,
) -> Result<Patched, SetValueError> {
    let canonical = crate::decode_jsonc(playbook_jsonc).map_err(SetValueError::NoCanonicalForm)?;
    let rendering = crate::render(canonical.decoded().message(), context, rules)
        .map_err(SetValueError::Refused)?;
    let chip = rendering
        .rules
        .iter()
        .flat_map(|rule| &rule.chips)
        .find(|chip| chip.pointer == pointer);
    let (shape, op) = match chip {
        Some(chip) => (chip.shape.clone(), chip.pick),
        None => (
            setting_shape(pointer)
                .map_err(SetValueError::Refused)?
                .ok_or(SetValueError::NoNumberHere)?,
            PickOp::Add,
        ),
    };
    if !matches!(shape, Shape::Integer { .. }) {
        return Err(SetValueError::NoNumberHere);
    }
    let value = typed_value(&shape, typed).map_err(SetValueError::Typed)?;
    let tokens = split_pointer(pointer).map_err(SetValueError::Refused)?;
    if !fits(field_kind(&tokens), value) {
        return Err(SetValueError::DoesNotFit);
    }
    let patch =
        pharmakos_proto::json::Json::Array(vec![pharmakos_proto::json::Json::Object(vec![
            (
                String::from("op"),
                pharmakos_proto::json::Json::String(String::from(match op {
                    PickOp::Add => "add",
                    PickOp::Replace => "replace",
                })),
            ),
            (
                String::from("path"),
                pharmakos_proto::json::Json::String(pointer.to_owned()),
            ),
            (
                String::from("value"),
                pharmakos_proto::json::Json::Number(value.to_string()),
            ),
        ])]);
    crate::patch_text(playbook_jsonc, &pharmakos_proto::json::write(&patch))
        .map_err(SetValueError::Refused)
}

/// A refusal of [`set_value_text`] in the player's words, the typed text
/// quoted where it is echoed (AGENTS.md section 7, the quoting door).
#[must_use]
pub fn set_value_words(error: &SetValueError, typed: &str) -> Said {
    match error {
        SetValueError::NoCanonicalForm(why) => said!(
            "this file has no canonical form yet, so no value in it can be typed: {}",
            why.said()
        ),
        SetValueError::NoNumberHere => Said::from(s::TYPED_NO_NUMBER_HERE),
        SetValueError::Typed(Typed::Empty) => Said::from(s::TYPED_EMPTY),
        SetValueError::Typed(Typed::NotANumber) => {
            said!("`{}` is not a number", quoted(typed))
        }
        SetValueError::Typed(Typed::NotWhole) => {
            said!(
                "`{}` is not a whole number, and this takes one",
                quoted(typed)
            )
        }
        SetValueError::Typed(Typed::FinerThanAMillisecond) => said!(
            "`{}` is finer than a thousandth of a second, which a playbook cannot hold",
            quoted(typed)
        ),
        SetValueError::Typed(Typed::TooLarge) | SetValueError::DoesNotFit => {
            said!("`{}` is past what this value can hold", quoted(typed))
        }
        SetValueError::Typed(Typed::BelowLeast(least, unit)) => said!(
            "`{}` is below the least this takes, {} {}",
            quoted(typed),
            quoted(least),
            *unit
        ),
        SetValueError::Typed(Typed::AboveMost(most, unit)) => said!(
            "`{}` is above the most this takes, {} {}",
            quoted(typed),
            quoted(most),
            *unit
        ),
        SetValueError::Refused(why) => why.shown(),
    }
}

/// The shape of a mandate setting the rendering lists no chip for, by its
/// pointer, from the vocabulary ([`crate::vocabulary::common_parameters`] and
/// each writ's parameters): the setting at a pointer that runs through a
/// `set_mandate_settings` row, a `set_mandate` row or a deploy's
/// `initial/mandate`, past which come the setting's own tokens. `None` for any
/// other pointer.
///
/// # Errors
///
/// A pointer that is not RFC 6901's; and the vocabulary disagreeing with the
/// schema, which its tests pin never happens.
fn setting_shape(pointer: &str) -> Result<Option<Shape>, Error> {
    let tokens = split_pointer(pointer)?;
    let start = tokens.iter().enumerate().rev().find_map(|(index, token)| {
        let opens = matches!(token.as_str(), "set_mandate_settings" | "set_mandate")
            || (token == "mandate"
                && index
                    .checked_sub(1)
                    .and_then(|before| tokens.get(before))
                    .is_some_and(|before| before == "initial"));
        opens.then_some(index)
    });
    let Some(start) = start.and_then(|start| start.checked_add(1)) else {
        return Ok(None);
    };
    let Some(rest) = tokens.get(start..) else {
        return Ok(None);
    };
    let inner = rest.iter().fold(String::new(), |pointer, token| {
        push_pointer(&pointer, token)
    });
    let shown =
        |error: crate::vocabulary::VocabularyError| Error::at(pointer, Said::engine_error(&error));
    if let Some(found) = crate::vocabulary::common_parameters()
        .map_err(shown)?
        .into_iter()
        .find(|parameter| parameter.pointer == inner)
    {
        return Ok(Some(found.shape));
    }
    let Some((writ, setting)) = rest.split_first() else {
        return Ok(None);
    };
    let setting = setting.iter().fold(String::new(), |pointer, token| {
        push_pointer(&pointer, token)
    });
    let words = crate::vocabulary::vocabulary(&[]).map_err(shown)?;
    Ok(words
        .mandates
        .into_iter()
        .find(|entry| entry.name == writ.as_str())
        .and_then(|entry| {
            entry
                .parameters
                .into_iter()
                .find(|parameter| parameter.pointer == setting)
        })
        .map(|parameter| parameter.shape))
}

/// The scalar kind of the field `pointer` names, walking the schema from
/// `gp.v1.Playbook` (an array index steps into a repeated field's message);
/// `None` where the schema names no field. `tokens` is the pointer, split.
fn field_kind(tokens: &[String]) -> Option<ScalarKind> {
    let schema = schema();
    let mut message = schema.message(crate::canonical::PLAYBOOK_MESSAGE)?;
    let mut kind = None;
    let mut in_list = false;
    for token in tokens {
        if in_list {
            // An index into the list just named: the message stays the
            // list's.
            in_list = false;
            continue;
        }
        let field = message.field_by_name(token)?;
        kind = Some(field.kind);
        if field.kind == ScalarKind::Message {
            message = schema.message(&field.type_name)?;
            in_list = field.repeated;
        }
    }
    kind
}

/// Whether `value` fits a field of `kind`; a field the schema does not name
/// fits nothing.
fn fits(kind: Option<ScalarKind>, value: i64) -> bool {
    match kind {
        Some(ScalarKind::Int32 | ScalarKind::Sint32 | ScalarKind::Sfixed32) => {
            i32::try_from(value).is_ok()
        }
        Some(ScalarKind::Uint32 | ScalarKind::Fixed32) => u32::try_from(value).is_ok(),
        _ => false,
    }
}

/// A new, empty rule at the end of the playbook's rules: a fresh id
/// (`rule_<n>`, the least `n` from one past the count that no rule holds),
/// `resume: CONTINUE`, firing once with the rules table's least cooldown, and
/// no `when` and no body, so its condition slot is empty for the picker
/// (S3's `fixq`, item 8). It writes no flag, branch or repeat (AGENTS.md
/// section 11): none is in the vocabulary.
///
/// # Errors
///
/// [`NewRuleError`].
pub fn new_rule(playbook_jsonc: &str, rules: &RulesTable) -> Result<NewRule, NewRuleError> {
    let canonical = crate::decode_jsonc(playbook_jsonc).map_err(NewRuleError::NoCanonicalForm)?;
    let playbook = canonical.decoded().message();
    let handlers: &[pharmakos_proto::gp::v1::Handler] = match playbook.declarative.as_ref() {
        Some(body) => &body.handlers,
        None => &[],
    };
    let count = handlers.len();
    if count >= pharmakos_sim::interpreter::MAX_HANDLERS {
        return Err(NewRuleError::Full(count));
    }
    let limits = pharmakos_verifier::Limits::from_rules(rules).map_err(|gap| {
        NewRuleError::Rules(Error::at(
            "",
            said!(
                "the rules table is incomplete: {}",
                Said::engine_error(&gap)
            ),
        ))
    })?;
    // `count + 1` candidates from `rule_<count + 1>` on, of which the
    // `count` rules hold at most `count`: one is always free.
    let id = (0..=count)
        .filter_map(|step| count.checked_add(step)?.checked_add(1))
        .map(|number| format!("rule_{number}"))
        .find(|id| handlers.iter().all(|handler| handler.id != *id))
        .ok_or_else(|| {
            NewRuleError::Refused(Error::at("/declarative/handlers", s::NEW_RULE_NO_ID))
        })?;
    let rule = pharmakos_proto::json::Json::Object(vec![
        (
            String::from("id"),
            pharmakos_proto::json::Json::String(id.clone()),
        ),
        (
            String::from("resume"),
            pharmakos_proto::json::Json::String(String::from("CONTINUE")),
        ),
        (
            String::from("max_fires"),
            pharmakos_proto::json::Json::Number(String::from("1")),
        ),
        (
            String::from("cooldown_ms"),
            pharmakos_proto::json::Json::Number(limits.handler_cooldown_min_ms().to_string()),
        ),
    ]);
    // The deepest container the file holds: the rules list, the declarative
    // block, or neither.
    let (at, value) = match playbook.declarative.as_ref() {
        Some(body) if !body.handlers.is_empty() => (String::from("/declarative/handlers/-"), rule),
        Some(_) => (
            String::from("/declarative/handlers"),
            pharmakos_proto::json::Json::Array(vec![rule]),
        ),
        None => (
            String::from("/declarative"),
            pharmakos_proto::json::Json::Object(vec![(
                String::from("handlers"),
                pharmakos_proto::json::Json::Array(vec![rule]),
            )]),
        ),
    };
    let patch =
        pharmakos_proto::json::Json::Array(vec![pharmakos_proto::json::Json::Object(vec![
            (
                String::from("op"),
                pharmakos_proto::json::Json::String(String::from("add")),
            ),
            (
                String::from("path"),
                pharmakos_proto::json::Json::String(at),
            ),
            (String::from("value"), value),
        ])]);
    let (patched, inverse) =
        crate::patch_text(playbook_jsonc, &pharmakos_proto::json::write(&patch))
            .map_err(NewRuleError::Refused)?;
    Ok(NewRule {
        playbook_jsonc: patched,
        inverse_json_patch: inverse,
        pointer: format!("/declarative/handlers/{count}"),
        id,
    })
}

/// A refusal of [`new_rule`] in the player's words.
#[must_use]
pub fn new_rule_words(error: &NewRuleError) -> Said {
    match error {
        NewRuleError::NoCanonicalForm(why) => said!(
            "this file has no canonical form yet, so a rule cannot be added to it: {}",
            why.said()
        ),
        NewRuleError::Full(count) => said!(
            "this playbook holds {} rules, the most a playbook may hold, so no rule can be added",
            count
        ),
        NewRuleError::Rules(why) | NewRuleError::Refused(why) => why.shown(),
    }
}
