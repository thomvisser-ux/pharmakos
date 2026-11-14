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
    },
    /// A closed list, each value the JSON text to write.
    Choice {
        /// `"LE"` with its quotes, `true` without.
        values: Vec<String>,
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

/// The schema has no enum of a name this module asks for: this crate and the
/// generated descriptor disagreeing, which a test pins never happens.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UnknownEnum(pub &'static str);

impl core::fmt::Display for UnknownEnum {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "the schema declares no enum `{}`", self.0)
    }
}

impl std::error::Error for UnknownEnum {}

impl Shape {
    /// An integer shape.
    #[must_use]
    pub const fn integer(min: Option<i32>, max: Option<i32>, unit: &'static str) -> Shape {
        Shape::Integer { min, max, unit }
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
            values: vec![String::from("true"), String::from("false")],
        }
    }

    /// Every value of the schema's enum `full_name` but its unspecified zero,
    /// in declaration order, as the playbook's canonical JSON spells it.
    ///
    /// # Errors
    ///
    /// [`UnknownEnum`] when the schema declares no such enum.
    pub fn choice(full_name: &'static str) -> Result<Shape, UnknownEnum> {
        let declared = schema()
            .enumeration(full_name)
            .ok_or(UnknownEnum(full_name))?;
        Ok(Shape::Choice {
            values: declared
                .values
                .iter()
                .filter(|value| value.number != 0)
                .map(|value| format!("\"{}\"", value.name))
                .collect(),
        })
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
/// in this build: [`PLACE_ARMS`] less `toward` while the verifier refuses it
/// (`E0028`, until S3's `pred`), so a picker never offers an arm the verifier
/// would refuse, and offers it in the build whose verifier lifts the refusal.
#[must_use]
pub fn place_arms() -> Vec<&'static str> {
    PLACE_ARMS
        .into_iter()
        .filter(|arm| {
            *arm != "toward" || pharmakos_verifier::code_for_construct("site `toward`").is_none()
        })
        .collect()
}

/// The arms legal in a `BuildTarget.anchor`: the fixed ones and `on`.
pub const ANCHOR_ARMS: [&str; 4] = ["voxel", "beacon_anchor", "safest", "on"];

/// The arms of a `gp.v1.FeatureRef`.
pub const FEATURE_ARMS: [&str; 4] = ["feature_id", "vent", "seam", "covered"];

#[cfg(test)]
mod tests {
    use super::{
        ANCHOR_ARMS, BEACON_ARMS, FEATURE_ARMS, NEAR_ARMS, PLACE_ARMS, Shape, UnknownEnum,
    };
    use pharmakos_proto::descriptor::schema;

    #[test]
    fn a_choice_lists_the_schemas_values_but_the_unspecified_zero() {
        assert_eq!(
            Shape::choice("gp.v1.IntCompare.Op"),
            Ok(Shape::Choice {
                values: ["\"LT\"", "\"LE\"", "\"EQ\"", "\"NE\"", "\"GE\"", "\"GT\""]
                    .map(String::from)
                    .to_vec()
            })
        );
        assert_eq!(
            Shape::choice("gp.v1.NoSuchEnum"),
            Err(UnknownEnum("gp.v1.NoSuchEnum"))
        );
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
            ("gp.v1.FeatureRef", FEATURE_ARMS.as_slice()),
        ] {
            let declared = schema().message(message).expect("a declared message");
            for arm in arms {
                let field = declared
                    .field_by_name(arm)
                    .unwrap_or_else(|| panic!("{message} declares no `{arm}`"));
                assert!(
                    field.oneof_index.is_some(),
                    "{message}.{arm} is a oneof arm"
                );
            }
        }
    }
}
