// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The method/scope table, and the JSON-RPC wire spellings.
//!
//! Two things live here, both of them boundary concerns the gateway inherits
//! rather than invents:
//!
//! * **Which scope a method needs.** The answer is an annotation on the method
//!   itself in `proto/gp/api/v1/gateway.proto` — a custom `EnumValueOptions`
//!   extension — and [`required_scope`] reads it back out of the checked-in
//!   descriptor set. There is no second table to keep in step, which is what
//!   "machine-readable per-method annotation" has to mean if it is to be worth
//!   anything.
//! * **How an enum-valued parameter is spelled on the wire.** Decisions-log
//!   item 80: lower-case strings — `verify_plan{depth:"quick"}`, spec section
//!   12's own example — translated at the boundary. [`wire_name`] and
//!   [`from_wire_name`] are that translation, and they are tested in both
//!   directions.
//!
//! Note the asymmetry with `gp.v1`, which is deliberate: a playbook on disk
//! carries the enum value name verbatim (`"author_kind": "HUMAN"`), because
//! the file is canonical proto JSON that a player hand-edits. The gateway's
//! params are not a proto JSON document; they are JSON-RPC parameters, and the
//! spec writes them lower-case.

use crate::descriptor::schema;
use crate::gp::api::v1::{Method, Scope};

/// The fully-qualified name of the annotation declared in `gateway.proto`.
const REQUIRED_SCOPE: &str = "gp.api.v1.required_scope";

/// The scope a token must hold to call this method.
///
/// `None` for `METHOD_UNSPECIFIED`, and for any method that has not been given
/// an annotation — which the `every_method_declares_a_scope` test makes sure
/// never happens.
#[must_use]
pub fn required_scope(method: Method) -> Option<Scope> {
    let schema = schema();
    let extension = schema.extension_number(REQUIRED_SCOPE)?;
    let declared = schema.enumeration("gp.api.v1.Method")?;
    let value = declared.value_by_number(method.into())?;
    let raw = value.option_varint(extension)?;
    let number = i32::try_from(raw).ok()?;
    Scope::try_from(number)
        .ok()
        .filter(|scope| *scope != Scope::Unspecified)
}

/// The lower-case string a value of an enum-valued JSON-RPC parameter takes on
/// the wire: `Depth::Quick` becomes `"quick"`, `Detail::Standard` becomes
/// `"standard"`, `Scope::PlanSubmit` becomes `"plan.submit"`.
///
/// The rule is mechanical, and it has to be, because it applies to every enum
/// the gateway exposes as a param: drop the enum's own `SCREAMING_PREFIX_`,
/// lower-case what is left, and turn `_` into `.` only for the scope names,
/// which spec section 12 spells with a dot.
#[must_use]
pub fn wire_name(enum_full_name: &str, value_name: &str) -> String {
    let prefix = screaming_prefix(enum_full_name);
    let stripped = value_name.strip_prefix(&prefix).unwrap_or(value_name);
    let lowered = stripped.to_ascii_lowercase();
    if enum_full_name == "gp.api.v1.Scope" {
        lowered.replace('_', ".")
    } else {
        lowered
    }
}

/// The enum value name a wire spelling stands for, or `None` if the enum has
/// no such value.
#[must_use]
pub fn from_wire_name(enum_full_name: &str, wire: &str) -> Option<String> {
    let declared = schema().enumeration(enum_full_name)?;
    declared
        .values
        .iter()
        .find(|value| wire_name(enum_full_name, &value.name) == wire)
        .map(|value| value.name.clone())
}

/// `gp.api.v1.Scope` becomes `SCOPE_`, `gp.api.v1.VerifyPlan.Depth` becomes
/// `DEPTH_`. The prefix Protobuf style puts on every value of an enum, which
/// the wire does not want to see.
fn screaming_prefix(enum_full_name: &str) -> String {
    let leaf = enum_full_name.rsplit('.').next().unwrap_or(enum_full_name);
    let mut prefix = String::new();
    for (index, character) in leaf.char_indices() {
        if character.is_ascii_uppercase() && index > 0 {
            prefix.push('_');
        }
        prefix.push(character.to_ascii_uppercase());
    }
    prefix.push('_');
    prefix
}
