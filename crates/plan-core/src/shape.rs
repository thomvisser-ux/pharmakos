// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What an editable value may hold: the one description a rendered chip, a
//! vocabulary entry's parameter and a beacon's setting share
//! (`gp.api.v1.ValueShape`; S3's plan, E8), so a client has one editor per
//! shape and holds no rule of its own (AGENTS.md section 3 rule 4).
//!
//! # Where each part comes from
//!
//! * **An enum's values** come from the schema: [`Shape::choice`] reads the
//!   enum's declaration ([`pharmakos_proto::descriptor::schema`]) and lists
//!   every value but the unspecified zero, as the playbook's canonical JSON
//!   spells it (`"LE"`, with its quotes). A value the schema adds is offered
//!   the build it lands in, and nothing here lists one by hand.
//! * **An integer's bounds** are the effective ones the verifier enforces at
//!   that pointer: the rules table's row where one bounds it (the cooldown
//!   floor, the `max_fires` ceiling), the field type's floor (a `uint32` is
//!   never below zero, a duration the verifier refuses below zero), and the
//!   unit's own limits (a percentage is 0 to 100), whichever is tighter. A
//!   bound nothing sets is `None`, and a ceiling past what `IntegerShape`'s
//!   `int32` carries -- a `uint32`'s own -- is one the wire cannot say, so it
//!   is `None` too. `tests/plan_core.rs`' agreement test writes each chip's
//!   bounds and one step past them into a playbook and holds the verifier to
//!   them, so this module and the verifier cannot drift apart unseen.
//! * **A reference's arms** are the oneof arms legal at its pointer, so the
//!   client knows, for instance, that `covering` is offered only where a
//!   beacon is placed.

use pharmakos_proto::descriptor::schema;

use crate::strings as s;

/// A duration in game milliseconds.
pub const UNIT_MS: &str = "ms";
/// A whole percentage.
pub const UNIT_PERCENT: &str = "%";
/// Whole dollars.
pub const UNIT_DOLLARS: &str = "$";
/// Whole kilowatts.
pub const UNIT_KW: &str = "kW";
/// Whole voxels.
pub const UNIT_VOXELS: &str = "voxels";
/// A bare count.
pub const UNIT_COUNT: &str = "";
/// Whole or decimal seconds: the unit a [`UNIT_MS`] value is shown and typed
/// in ([`Shape::display_unit`]; S3's `fixq`). The file still holds
/// milliseconds; the gateway converts, and the client converts nothing.
pub const UNIT_SECONDS: &str = "s";

/// What a value picked from the world or the playbook is
/// (`gp.api.v1.ReferenceShape.Kind`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReferenceKind {
    /// A `gp.v1.BeaconRef`.
    Beacon,
    /// A `gp.v1.Location`, or the `gp.v1.NearPlace` that spells its fixed arms.
    Location,
    /// A `gp.v1.FeatureRef`.
    Feature,
    /// A `Step.label` of this playbook's route.
    StepLabel,
    /// A `Handler.id` of this playbook.
    HandlerId,
}

impl ReferenceKind {
    /// The value's name in `gp.api.v1.ReferenceShape.Kind`.
    #[must_use]
    pub const fn proto_name(self) -> &'static str {
        match self {
            ReferenceKind::Beacon => "BEACON",
            ReferenceKind::Location => "LOCATION",
            ReferenceKind::Feature => "FEATURE",
            ReferenceKind::StepLabel => "STEP_LABEL",
            ReferenceKind::HandlerId => "HANDLER_ID",
        }
    }
}

/// What an editable value may hold (`gp.api.v1.ValueShape`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Shape {
    /// A whole number within inclusive bounds; `None` is a bound nothing sets.
    Integer {
        /// The least value the verifier accepts here.
        min: Option<i32>,
        /// The greatest value the verifier accepts here.
        max: Option<i32>,
        /// [`UNIT_MS`], [`UNIT_PERCENT`], [`UNIT_DOLLARS`], [`UNIT_KW`],
        /// [`UNIT_VOXELS`] or [`UNIT_COUNT`].
        unit: &'static str,
        /// What a 0 does here, in the player's words ([`crate::strings`]),
        /// where the sim reads a 0 as turning what the setting governs off
        /// (`gp.api.v1.IntegerShape.zero_title`); `None` where a 0 is just a
        /// number. Only the named constructors below set it, each for a
        /// setting whose bounds admit a 0 (S3's `zerotitle`).
        zero: Option<&'static str>,
    },
    /// A closed list, each value the JSON text to write with its words.
    Choice {
        /// Each value, in the order a menu offers them.
        options: Vec<Choice>,
    },
    /// A value picked rather than typed.
    Reference {
        /// What is picked.
        kind: ReferenceKind,
        /// The oneof arm names legal at this pointer; empty for a kind that
        /// is a plain string (a label, a rule id).
        arms: Vec<&'static str>,
    },
}

/// One value of a [`Shape::Choice`] (`gp.api.v1.EnumShape`'s `values` and
/// `titles`, which the wire carries as two lists of one length).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Choice {
    /// The JSON text to write: `"LE"` with its quotes, `true` without.
    pub json: String,
    /// The value in the player's words ([`crate::strings`]), which a menu
    /// draws in place of the JSON text.
    pub title: &'static str,
}

impl Choice {
    /// A value and its words.
    #[must_use]
    pub fn new(json: impl Into<String>, title: &'static str) -> Choice {
        Choice {
            json: json.into(),
            title,
        }
    }
}

/// An enum this module asks for and the schema disagreeing: the schema
/// declares no enum of that name, or declares a value this module has no
/// words for. Either is this crate and the generated descriptor out of step,
/// which `tests` pin never happens.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum UnknownEnum {
    /// No enum of this full name.
    Enum(&'static str),
    /// A value of this enum, by its name, that [`choice_title`] has no words
    /// for.
    Untitled(&'static str, String),
}

/// The enum's name is the schema's, a `&'static str`, and a value's name is
/// the schema's too (S3's `vfyq2`, fix pass 3, item M2 (d)).
impl pharmakos_proto::json::grammar::EngineWords for UnknownEnum {}
impl pharmakos_proto::json::grammar::door::EngineSeal for UnknownEnum {}

impl core::fmt::Display for UnknownEnum {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            UnknownEnum::Enum(name) => write!(formatter, "the schema declares no enum `{name}`"),
            UnknownEnum::Untitled(name, value) => write!(
                formatter,
                "the schema's enum `{name}` declares `{value}`, which has no words here"
            ),
        }
    }
}

impl std::error::Error for UnknownEnum {}

impl Shape {
    /// An integer shape.
    #[must_use]
    pub const fn integer(min: Option<i32>, max: Option<i32>, unit: &'static str) -> Shape {
        Shape::Integer {
            min,
            max,
            unit,
            zero: None,
        }
    }

    /// The Common row's `retreat_hp_pct`: a percentage whose 0 turns the
    /// beacon's retreat off (decisions-log item 177 (3); the sim's
    /// `below_retreat` reads a threshold of 0 as never below).
    #[must_use]
    pub const fn retreat_threshold() -> Shape {
        Shape::Integer {
            min: Some(0),
            max: Some(100),
            unit: UNIT_PERCENT,
            zero: Some(s::RETREAT_OFF),
        }
    }

    /// A Build arm's `repair_threshold_pct`: a percentage whose 0 never
    /// repairs a structure (decision 38; the sim's `repair_work` counts no
    /// structure under a threshold of 0, and its units are still repaired).
    #[must_use]
    pub const fn repair_threshold() -> Shape {
        Shape::Integer {
            min: Some(0),
            max: Some(100),
            unit: UNIT_PERCENT,
            zero: Some(s::REPAIR_NEVER),
        }
    }

    /// A Defend arm's `picket_scouts`: a count up to a beacon's byte column
    /// (`E0508`) whose 0 stands the picket down (the sim's Defend mandate
    /// asks the fabricator for no scout under a count of 0).
    #[must_use]
    pub const fn picket_scouts() -> Shape {
        Shape::Integer {
            min: Some(0),
            max: Some(255),
            unit: UNIT_COUNT,
            zero: Some(s::PICKET_NONE),
        }
    }

    /// An Attack arm's `launch.force_size`: a count of raiders whose 0 is not
    /// "none": the sim fields one raider and launches it at once, ungathered
    /// (`attack.rs`'s `wanted_force` and `by_size`).
    #[must_use]
    pub const fn force_size() -> Shape {
        Shape::Integer {
            min: Some(0),
            max: None,
            unit: UNIT_COUNT,
            zero: Some(s::LAUNCH_FIRST_RAIDER),
        }
    }

    /// A Survey arm's `scout_count`: a count up to a beacon's byte column
    /// (`E0508`) whose 0 fields no scout (the sim's Survey mandate asks the
    /// fabricator for none under a count of 0; decisions-log item 184 (3)
    /// gave the field presence, so a written 0 is held).
    #[must_use]
    pub const fn scout_count() -> Shape {
        Shape::Integer {
            min: Some(0),
            max: Some(255),
            unit: UNIT_COUNT,
            zero: Some(s::SCOUTS_NONE),
        }
    }

    /// A Mine arm's `dig_max_depth`: voxels below the seam's original top,
    /// whose 0 digs the exposed rim only (the sim's miner takes no voxel
    /// deeper than the depth; item 184 (3)).
    #[must_use]
    pub const fn dig_max_depth() -> Shape {
        Shape::Integer {
            min: Some(0),
            max: None,
            unit: UNIT_VOXELS,
            zero: Some(s::DIG_RIM_ONLY),
        }
    }

    /// A Mine arm's `pillar_spacing`: columns between pillars, whose 0 leaves
    /// no pillar (the sim's `pillar` reads a spacing of 0 as none; item 184
    /// (10)).
    #[must_use]
    pub const fn pillar_spacing() -> Shape {
        Shape::Integer {
            min: Some(0),
            max: None,
            unit: UNIT_VOXELS,
            zero: Some(s::PILLARS_NONE),
        }
    }

    /// The words for what a 0 does in this shape
    /// (`gp.api.v1.IntegerShape.zero_title`), `None` for a shape with none or
    /// one that is not an integer.
    #[must_use]
    pub const fn zero_title(&self) -> Option<&'static str> {
        match self {
            Shape::Integer { zero, .. } => *zero,
            Shape::Choice { .. } | Shape::Reference { .. } => None,
        }
    }

    /// The words for `value` in this shape, when it has words of its own: the
    /// shape's [`Shape::zero_title`] when `value` is 0, `None` otherwise
    /// (`gp.api.v1.Chip.value_title` and `BeaconSettingRow.value_title`). The
    /// gateway's one decision of it, so a client tests no value.
    #[must_use]
    pub const fn value_title(&self, value: i64) -> Option<&'static str> {
        if value == 0 { self.zero_title() } else { None }
    }

    /// The least and the most as they are shown and typed, in
    /// [`Shape::display_unit`], each `None` exactly when its bound is
    /// (`gp.api.v1.IntegerShape.min_text`, `max_text`), written by
    /// [`shown_text`], the one writer of a chip's `value_text`; both `None`
    /// for a shape that is not an integer.
    #[must_use]
    pub fn bound_texts(&self) -> (Option<String>, Option<String>) {
        match self {
            Shape::Integer { min, max, unit, .. } => (
                min.map(|least| shown_text(unit, i64::from(least))),
                max.map(|most| shown_text(unit, i64::from(most))),
            ),
            Shape::Choice { .. } | Shape::Reference { .. } => (None, None),
        }
    }

    /// A whole percentage, 0 to 100 (spec section 10: "Percent, integer.
    /// 0-100").
    #[must_use]
    pub const fn percent() -> Shape {
        Shape::integer(Some(0), Some(100), UNIT_PERCENT)
    }

    /// A duration the verifier refuses below zero, with no ceiling.
    #[must_use]
    pub const fn duration() -> Shape {
        Shape::integer(Some(0), None, UNIT_MS)
    }

    /// A `uint32` count: never below zero, and a ceiling the wire's `int32`
    /// cannot say.
    #[must_use]
    pub const fn count(unit: &'static str) -> Shape {
        Shape::integer(Some(0), None, unit)
    }

    /// `true` or `false`.
    #[must_use]
    pub fn boolean() -> Shape {
        Shape::Choice {
            options: vec![
                Choice::new("true", s::CHOICE_TRUE),
                Choice::new("false", s::CHOICE_FALSE),
            ],
        }
    }

    /// Every value of the schema's enum `full_name` but its unspecified zero,
    /// in declaration order, as the playbook's canonical JSON spells it, each
    /// with its words ([`choice_title`]).
    ///
    /// # Errors
    ///
    /// [`UnknownEnum`] when the schema declares no such enum, or a value this
    /// module has no words for.
    pub fn choice(full_name: &'static str) -> Result<Shape, UnknownEnum> {
        let declared = schema()
            .enumeration(full_name)
            .ok_or(UnknownEnum::Enum(full_name))?;
        Ok(Shape::Choice {
            options: declared
                .values
                .iter()
                .filter(|value| value.number != 0)
                .map(|value| {
                    choice_title(full_name, &value.name)
                        .map(|title| Choice::new(format!("\"{}\"", value.name), title))
                        .ok_or_else(|| UnknownEnum::Untitled(full_name, value.name.clone()))
                })
                .collect::<Result<Vec<Choice>, UnknownEnum>>()?,
        })
    }

    /// The unit the value is shown and typed in: [`UNIT_SECONDS`] for a
    /// [`UNIT_MS`] integer, the integer's own unit for any other, and `None`
    /// for a shape that is not an integer (`gp.api.v1.IntegerShape.display_unit`).
    #[must_use]
    pub fn display_unit(&self) -> Option<&'static str> {
        match self {
            Shape::Integer { unit, .. } => Some(display_unit(unit)),
            Shape::Choice { .. } | Shape::Reference { .. } => None,
        }
    }

    /// A reference shape.
    #[must_use]
    pub fn reference(kind: ReferenceKind, arms: &[&'static str]) -> Shape {
        Shape::Reference {
            kind,
            arms: arms.to_vec(),
        }
    }
}

/// The unit a value in `unit` is shown and typed in: [`UNIT_SECONDS`] for
/// [`UNIT_MS`], `unit` itself for every other.
#[must_use]
pub fn display_unit(unit: &'static str) -> &'static str {
    if unit == UNIT_MS { UNIT_SECONDS } else { unit }
}

/// `value`, in `unit`, written as it is shown and typed in
/// [`display_unit`]`(unit)`: a [`UNIT_MS`] value in exact seconds, with no
/// more decimals than it needs (`1500` is `"1.5"`, `8000` is `"8"`, `1` is
/// `"0.001"`, `-500` is `"-0.5"`), and any other as the whole number it is.
/// Exact, never rounded, so [`typed_value`] reads it back to `value`.
#[must_use]
pub fn shown_text(unit: &str, value: i64) -> String {
    if unit != UNIT_MS {
        return value.to_string();
    }
    let sign = if value < 0 { "-" } else { "" };
    let magnitude = value.unsigned_abs();
    let seconds = magnitude.div_euclid(1000);
    let rest = magnitude.rem_euclid(1000);
    if rest == 0 {
        return format!("{sign}{seconds}");
    }
    let fraction = format!("{rest:03}");
    format!("{sign}{seconds}.{}", fraction.trim_end_matches('0'))
}

/// Why typed text is not a value of an integer shape ([`typed_value`]).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Typed {
    /// Nothing but whitespace was typed.
    Empty,
    /// The text is not a number written as [`typed_value`] reads one.
    NotANumber,
    /// A fraction in a unit that takes whole numbers only.
    NotWhole,
    /// A duration finer than a game millisecond, which the file cannot hold:
    /// refused, never rounded.
    FinerThanAMillisecond,
    /// A number past what any playbook integer holds.
    TooLarge,
    /// Below the least the verifier accepts here: the least as it is shown,
    /// and the unit it is shown in.
    BelowLeast(String, &'static str),
    /// Above the most the verifier accepts here: the most as it is shown, and
    /// the unit it is shown in.
    AboveMost(String, &'static str),
}

/// The value typed text names in an integer shape, in the shape's own unit:
/// `"1.5"` in a [`UNIT_MS`] shape is 1500, and `"40"` in a percentage is 40.
///
/// The text is read in [`Shape::display_unit`], after its leading and
/// trailing whitespace: an optional `-`, at least one digit, and, for a
/// duration only, a `.` and up to three digits past it (more are refused
/// unless they are zeros: a value finer than a millisecond is
/// [`Typed::FinerThanAMillisecond`], never rounded). In any other unit a `.`
/// is [`Typed::NotWhole`], whatever follows it. No `+`, no exponent, no
/// separators. The value must lie within the shape's bounds. Integer
/// arithmetic only, every step checked.
///
/// # Errors
///
/// [`Typed`]; and [`Typed::NotANumber`] for a shape that is not an integer.
pub fn typed_value(shape: &Shape, typed: &str) -> Result<i64, Typed> {
    let Shape::Integer { min, max, unit, .. } = shape else {
        return Err(Typed::NotANumber);
    };
    let text = typed.trim();
    if text.is_empty() {
        return Err(Typed::Empty);
    }
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (whole, fraction) = match digits.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (digits, None),
    };
    let all_digits =
        |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    if !all_digits(whole) {
        return Err(Typed::NotANumber);
    }
    let scale: i64 = if *unit == UNIT_MS { 1000 } else { 1 };
    // A `.` is read only in a unit with a fraction: in a whole-number unit
    // "40.0" is refused, never read as 40 (S3's `fixq`, review fix pass 1).
    if scale == 1 && fraction.is_some() {
        return Err(Typed::NotWhole);
    }
    if fraction.is_some_and(|fraction| !all_digits(fraction)) {
        return Err(Typed::NotANumber);
    }
    let fraction = fraction.map(|fraction| fraction.trim_end_matches('0'));
    let thousandths: i64 = match fraction {
        None | Some("") => 0,
        Some(fraction) if fraction.len() > 3 => return Err(Typed::FinerThanAMillisecond),
        Some(fraction) => {
            let read: i64 = fraction.parse().map_err(|_| Typed::NotANumber)?;
            // Pad to thousandths: "5" is 500, "05" is 50, "005" is 5.
            let pad = match fraction.len() {
                1 => 100,
                2 => 10,
                _ => 1,
            };
            read.checked_mul(pad).ok_or(Typed::TooLarge)?
        }
    };
    let whole: i64 = whole.parse().map_err(|_| Typed::TooLarge)?;
    let magnitude = whole
        .checked_mul(scale)
        .and_then(|value| value.checked_add(thousandths))
        .ok_or(Typed::TooLarge)?;
    let value = if negative {
        magnitude.checked_neg().ok_or(Typed::TooLarge)?
    } else {
        magnitude
    };
    if let Some(least) = min
        && value < i64::from(*least)
    {
        return Err(Typed::BelowLeast(
            shown_text(unit, i64::from(*least)),
            display_unit(unit),
        ));
    }
    if let Some(most) = max
        && value > i64::from(*most)
    {
        return Err(Typed::AboveMost(
            shown_text(unit, i64::from(*most)),
            display_unit(unit),
        ));
    }
    Ok(value)
}

/// The words for value `value` (its schema name, `"LE"` unquoted) of the
/// schema's enum `full_name`, for every enum a chip or a vocabulary
/// parameter offers as a choice; `None` for any other. Where the prose has the
/// word for a value already, it is that word ([`crate::strings`]'s `CHOICE_*`
/// note). `tests` hold every value of every enum offered here to a title.
#[must_use]
pub fn choice_title(full_name: &str, value: &str) -> Option<&'static str> {
    let comparison = |value: &str| match value {
        "LT" => Some(s::CMP_LT),
        "LE" => Some(s::CMP_LE),
        "EQ" => Some(s::CMP_EQ),
        "NE" => Some(s::CMP_NE),
        "GE" => Some(s::CMP_GE),
        "GT" => Some(s::CMP_GT),
        _ => None,
    };
    match full_name {
        "gp.v1.IntCompare.Op" | "gp.v1.CmdrHpPct.Cmp" => comparison(value),
        "gp.v1.BeaconUnitsByRole.Role" => match value {
            "BUILD_DRONE" => Some(s::ROLE_BUILD_DRONE),
            "MINING_DRONE" => Some(s::ROLE_MINING_DRONE),
            "REPAIR_DRONE" => Some(s::ROLE_REPAIR_DRONE),
            "RAIDER" => Some(s::ROLE_RAIDER),
            "SCOUT" => Some(s::ROLE_SCOUT),
            _ => None,
        },
        "gp.v1.BeaconFilter.MandateKind" => match value {
            "BUILD" => Some(s::MANDATE_BUILD),
            "DEFEND" => Some(s::MANDATE_DEFEND),
            "ATTACK" => Some(s::MANDATE_ATTACK),
            "SURVEY" => Some(s::MANDATE_SURVEY),
            "MINE" => Some(s::MANDATE_MINE),
            _ => None,
        },
        "gp.v1.Freshness.Kind" => match value {
            "VISIBLE_NOW" => Some(s::FRESH_NOW),
            "KNOWN_WITHIN" => Some(s::CHOICE_KNOWN_WITHIN),
            _ => None,
        },
        "gp.v1.MandateSettings.Roe" => match value {
            "HOLD_FIRE" => Some(s::CHOICE_HOLD_FIRE),
            "RETURN_FIRE" => Some(s::CHOICE_RETURN_FIRE),
            "FREE_FIRE" => Some(s::CHOICE_FREE_FIRE),
            _ => None,
        },
        "gp.v1.BuildSettings.Terraform" => match value {
            "NONE" => Some(s::CHOICE_TERRAFORM_NONE),
            "FILL" => Some(s::CHOICE_TERRAFORM_FILL),
            "DIG" => Some(s::CHOICE_TERRAFORM_DIG),
            "BOTH" => Some(s::CHOICE_TERRAFORM_BOTH),
            _ => None,
        },
        "gp.v1.DefendSettings.Pursue" => match value {
            "NEVER" => Some(s::PURSUE_NEVER),
            "SPHERE_EDGE" => Some(s::PURSUE_SPHERE_EDGE),
            "WHILE_IN_REACH" => Some(s::PURSUE_WHILE_IN_REACH),
            _ => None,
        },
        "gp.v1.AttackSettings.Retarget" => match value {
            "NONE" => Some(s::RETARGET_NONE),
            "NEAREST_KNOWN" => Some(s::RETARGET_NEAREST),
            _ => None,
        },
        "gp.v1.AttackSettings.AfterAction" => match value {
            "REGROUP_AT_STAGING" => Some(s::CHOICE_AFTER_REGROUP),
            "RETURN_HOME" => Some(s::CHOICE_AFTER_HOME),
            _ => None,
        },
        "gp.v1.MineSettings.SeamChoice" => match value {
            "RICHEST" => Some(s::CHOICE_SEAM_RICHEST),
            "NEAREST" => Some(s::CHOICE_SEAM_NEAREST),
            "SAFEST" => Some(s::CHOICE_SEAM_SAFEST),
            _ => None,
        },
        "gp.v1.MoveStep.Pace" => match value {
            "DIRECT" => Some(s::CHOICE_PACE_DIRECT),
            "AVOID_KNOWN_THREATS" => Some(s::CHOICE_PACE_AVOID),
            _ => None,
        },
        "gp.v1.InterfaceRow.QuartermasterPriority" => match value {
            "LOW" => Some(s::PRIORITY_LOW),
            "NORMAL" => Some(s::PRIORITY_NORMAL),
            "HIGH" => Some(s::PRIORITY_HIGH),
            _ => None,
        },
        _ => None,
    }
}

/// Every enum a chip or a vocabulary parameter offers as a choice: the ones
/// [`choice_title`] has words for, which `tests` hold to the schema.
pub const CHOICE_ENUMS: [&str; 13] = [
    "gp.v1.IntCompare.Op",
    "gp.v1.CmdrHpPct.Cmp",
    "gp.v1.BeaconUnitsByRole.Role",
    "gp.v1.BeaconFilter.MandateKind",
    "gp.v1.Freshness.Kind",
    "gp.v1.MandateSettings.Roe",
    "gp.v1.BuildSettings.Terraform",
    "gp.v1.DefendSettings.Pursue",
    "gp.v1.AttackSettings.Retarget",
    "gp.v1.AttackSettings.AfterAction",
    "gp.v1.MineSettings.SeamChoice",
    "gp.v1.MoveStep.Pace",
    "gp.v1.InterfaceRow.QuartermasterPriority",
];

/// The arms of a `gp.v1.BeaconRef`.
pub const BEACON_ARMS: [&str; 5] = [
    "beacon_id",
    "nearest",
    "weakest",
    "most_threatened",
    "safest",
];

/// The arms of a `gp.v1.NearPlace`: a `Location`'s fixed ones.
pub const NEAR_ARMS: [&str; 3] = ["voxel", "beacon_anchor", "safest"];

/// The arms legal in a `PlaceBeaconStep.at`: the fixed ones, `covering` and
/// `toward`.
pub const PLACE_ARMS: [&str; 5] = ["voxel", "beacon_anchor", "safest", "covering", "toward"];

/// The arms legal in a `PlaceBeaconStep.at` that the verifier does not refuse
/// in this build: [`PLACE_ARMS`], less `toward` in a build whose verifier
/// refuses it (`E0028`, which S3's `pred` retired), so a picker never offers
/// an arm the verifier would refuse.
#[must_use]
pub fn place_arms() -> Vec<&'static str> {
    PLACE_ARMS
        .into_iter()
        .filter(|arm| {
            *arm != "toward" || pharmakos_verifier::code_for_construct("site `toward`").is_none()
        })
        .collect()
}

/// The arms of a `BuildTarget.anchor` and a `RemoveBuildTargetRow.anchor`:
/// the fixed ones and `on`.
pub const ANCHOR_ARMS: [&str; 4] = ["voxel", "beacon_anchor", "safest", "on"];

/// The arms legal in a Build target's anchor, and a removal's, that the
/// verifier does not refuse in this build: [`ANCHOR_ARMS`] less the two that
/// place it by a beacon (`beacon_anchor`, `safest`) while the verifier
/// refuses that (`E0011`), as [`place_arms`] leaves out `toward`.
#[must_use]
pub fn anchor_arms() -> Vec<&'static str> {
    let by_beacon = pharmakos_verifier::code_for_construct(
        "a Build target anchored on a beacon selector rather than on a voxel or `on` a vent",
    )
    .is_some();
    ANCHOR_ARMS
        .into_iter()
        .filter(|arm| !(by_beacon && matches!(*arm, "beacon_anchor" | "safest")))
        .collect()
}

/// The arm legal in a `wall` target's anchor: a voxel inside its box (the
/// sim's `compile_wall_box`, the verifier's `E0417`).
pub const WALL_ANCHOR_ARMS: [&str; 1] = ["voxel"];

/// The arms legal in an Attack target's `beacon`: a handle, or `nearest`
/// with `side: ENEMY_KNOWN`; the verifier refuses every other selector there
/// (`E0015`, `E0415`).
pub const ATTACK_TARGET_ARMS: [&str; 2] = ["beacon_id", "nearest"];

/// The arms of a `gp.v1.FeatureRef`.
pub const FEATURE_ARMS: [&str; 4] = ["feature_id", "vent", "seam", "covered"];

#[cfg(test)]
mod tests {
    use super::{
        ANCHOR_ARMS, ATTACK_TARGET_ARMS, BEACON_ARMS, FEATURE_ARMS, NEAR_ARMS, PLACE_ARMS, Shape,
        UnknownEnum, WALL_ANCHOR_ARMS,
    };
    use pharmakos_proto::descriptor::schema;

    #[test]
    fn a_choice_lists_the_schemas_values_but_the_unspecified_zero() {
        let Ok(Shape::Choice { options }) = Shape::choice("gp.v1.IntCompare.Op") else {
            panic!("a choice");
        };
        assert_eq!(
            options
                .iter()
                .map(|option| option.json.as_str())
                .collect::<Vec<_>>(),
            ["\"LT\"", "\"LE\"", "\"EQ\"", "\"NE\"", "\"GE\"", "\"GT\""]
        );
        assert_eq!(
            options
                .iter()
                .map(|option| option.title)
                .collect::<Vec<_>>(),
            [
                "below",
                "at or below",
                "exactly",
                "anything but",
                "at or above",
                "above"
            ]
        );
        assert_eq!(
            Shape::choice("gp.v1.NoSuchEnum"),
            Err(UnknownEnum::Enum("gp.v1.NoSuchEnum"))
        );
        // An enum this module offers no choice of has no words, and asking
        // for it as a choice is refused by name, never offered untitled.
        assert_eq!(
            Shape::choice("gp.v1.BeaconFilter.Side"),
            Err(UnknownEnum::Untitled(
                "gp.v1.BeaconFilter.Side",
                String::from("OWN")
            ))
        );
    }

    /// Every value but the zero of every enum offered as a choice has words,
    /// none of them empty and none quoted, so a menu never draws a value's
    /// JSON text (S3's `fixq`, item 6).
    #[test]
    fn every_offered_enum_has_words_for_every_value() {
        for name in super::CHOICE_ENUMS {
            let Ok(Shape::Choice { options }) = Shape::choice(name) else {
                panic!("{name} is a choice with words for every value");
            };
            assert!(!options.is_empty(), "{name}");
            for option in options {
                assert!(
                    !option.title.is_empty() && !option.title.contains('"'),
                    "{name}: {option:?}"
                );
            }
        }
        let Shape::Choice { options } = Shape::boolean() else {
            panic!("a choice");
        };
        assert_eq!(
            options
                .iter()
                .map(|option| (option.json.as_str(), option.title))
                .collect::<Vec<_>>(),
            [("true", "yes"), ("false", "no")]
        );
    }

    /// A duration is shown and typed in seconds, and every other integer in
    /// its own unit (S3's `fixq`, item 7).
    #[test]
    fn a_duration_is_shown_in_seconds() {
        assert_eq!(Shape::duration().display_unit(), Some("s"));
        assert_eq!(Shape::percent().display_unit(), Some("%"));
        assert_eq!(Shape::count("").display_unit(), Some(""));
        assert_eq!(Shape::boolean().display_unit(), None);
    }

    /// A zero's title is said only for a 0, only by the three settings whose 0
    /// turns what they govern off, and each of their bounds admits the 0 it
    /// names (S3's `zerotitle`).
    #[test]
    fn a_zero_title_names_only_a_zero_its_bounds_admit() {
        use crate::strings as s;
        for (shape, title) in [
            (Shape::retreat_threshold(), s::RETREAT_OFF),
            (Shape::repair_threshold(), s::REPAIR_NEVER),
            (Shape::picket_scouts(), s::PICKET_NONE),
        ] {
            assert_eq!(shape.zero_title(), Some(title));
            assert_eq!(shape.value_title(0), Some(title));
            assert_eq!(shape.value_title(1), None, "{title}: 1 is just a number");
            assert_eq!(shape.value_title(-1), None, "{title}: -1 is no zero");
            let Shape::Integer { min, max, .. } = shape else {
                panic!("an integer");
            };
            assert!(
                min.is_some_and(|least| least <= 0),
                "{title}: 0 is admitted"
            );
            assert!(max.is_none_or(|most| most >= 0), "{title}: 0 is admitted");
        }
        for plain in [
            Shape::percent(),
            Shape::duration(),
            Shape::count(super::UNIT_COUNT),
            Shape::integer(None, None, super::UNIT_KW),
        ] {
            assert_eq!(plain.zero_title(), None);
            assert_eq!(plain.value_title(0), None);
        }
        assert_eq!(Shape::boolean().value_title(0), None);
        // The picket's ceiling is the beacon's byte column, as the verifier's
        // `E0508` holds it.
        assert_eq!(
            Shape::picket_scouts(),
            Shape::Integer {
                min: Some(0),
                max: Some(i32::from(u8::MAX)),
                unit: super::UNIT_COUNT,
                zero: Some(s::PICKET_NONE),
            }
        );
    }

    /// Each bound's text is the bound as `shown_text` writes a chip's value,
    /// in the unit it is typed in, and present exactly when the bound is (S3's
    /// `zerotitle`: a duration's bounds were said in milliseconds beside a
    /// field typed in seconds).
    #[test]
    fn a_bound_is_said_in_the_unit_it_is_typed_in() {
        assert_eq!(
            Shape::integer(Some(500), Some(90_000), super::UNIT_MS).bound_texts(),
            (Some(String::from("0.5")), Some(String::from("90")))
        );
        assert_eq!(
            Shape::duration().bound_texts(),
            (Some(String::from("0")), None)
        );
        assert_eq!(
            Shape::percent().bound_texts(),
            (Some(String::from("0")), Some(String::from("100")))
        );
        assert_eq!(
            Shape::integer(None, None, super::UNIT_DOLLARS).bound_texts(),
            (None, None)
        );
        assert_eq!(Shape::boolean().bound_texts(), (None, None));
        for value in [-1_500_i32, -1, 0, 1, 999, 1_000, 1_500, 86_400_000] {
            let (least, most) =
                Shape::integer(Some(value), Some(value), super::UNIT_MS).bound_texts();
            assert_eq!(
                least,
                Some(super::shown_text(super::UNIT_MS, i64::from(value)))
            );
            assert_eq!(least, most);
        }
    }

    /// Every arm list names arms its message's oneof declares, so a rename in
    /// the schema fails here rather than in the client.
    #[test]
    fn every_arm_is_one_the_schema_declares() {
        for (message, arms) in [
            ("gp.v1.BeaconRef", BEACON_ARMS.as_slice()),
            ("gp.v1.NearPlace", NEAR_ARMS.as_slice()),
            ("gp.v1.Location", PLACE_ARMS.as_slice()),
            ("gp.v1.Location", ANCHOR_ARMS.as_slice()),
            ("gp.v1.Location", WALL_ANCHOR_ARMS.as_slice()),
            ("gp.v1.BeaconRef", ATTACK_TARGET_ARMS.as_slice()),
            ("gp.v1.FeatureRef", FEATURE_ARMS.as_slice()),
        ] {
            let declared = schema().message(message).expect("a declared message");
            for arm in arms {
                let field = declared
                    .field_by_name(arm)
                    .unwrap_or_else(|| panic!("{message} declares no `{arm}`"));
                assert!(field.is_arm(), "{message}.{arm} is a oneof arm");
            }
        }
    }
}
