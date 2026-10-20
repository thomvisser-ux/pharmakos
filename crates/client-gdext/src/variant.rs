// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Canonical JSON to Godot `Variant`, and nothing else.
//!
//! The last step of a gateway result's journey: [`crate::api`] has already read the text
//! against the schema, so what arrives here is a value the schema accepts, and this file
//! puts it into the types GDScript can hold. Objects become `Dictionary`, arrays become
//! `Array`, and the leaves keep their meaning.
//!
//! # Numbers
//!
//! `gp.v1` and `gp.api.v1` have **no floating-point fields** — the canonical codec
//! refuses them on principle — so every number that reaches this file is an integer, and
//! the conversion is `i64`. A lexeme too large for an `i64` (a `uint64` above
//! `i64::MAX`, which canonical proto JSON writes as a quoted string anyway) is passed on
//! as its **lexeme**, unchanged, rather than as a float: silently turning a 64-bit id
//! into a `f64` would lose the bottom eleven bits of it, and choosing to lose them would
//! be the bridge deciding something.

use godot::builtin::{VarArray, VarDictionary, Variant};
use godot::meta::ToGodot;
use pharmakos_proto::json::Json;

/// One canonical-JSON value as a Godot `Variant`.
pub(crate) fn json_to_variant(value: &Json) -> Variant {
    match value {
        Json::Null => Variant::nil(),
        Json::Bool(flag) => flag.to_variant(),
        Json::Number(lexeme) => match lexeme.parse::<i64>() {
            Ok(number) => number.to_variant(),
            // Not a float, and not an error either: the caller gets the digits it was
            // sent. See the module header.
            Err(_) => lexeme.to_variant(),
        },
        Json::String(text) => text.to_variant(),
        Json::Array(items) => {
            let mut array = VarArray::new();
            for item in items {
                array.push(&json_to_variant(item));
            }
            array.to_variant()
        }
        Json::Object(entries) => {
            let mut dictionary = VarDictionary::new();
            for (key, entry) in entries {
                dictionary.set(&key.to_variant(), &json_to_variant(entry));
            }
            dictionary.to_variant()
        }
    }
}

/// A count as Godot's `int`, which is an `i64`.
///
/// Every count the bridge hands GDScript (calls, bytes, chunks, vertices, panics) steps
/// by a bounded amount per call or per frame from zero, so none reaches `i64::MAX`
/// (9.2 × 10¹⁸) in the life of a process; at that bound the count is shown as
/// `i64::MAX`, the largest `int` GDScript holds. A presentation rule: GDScript prints and
/// compares these counts, and nothing decides on them. It is the one place the bridge
/// narrows a count, so the rule is written once.
pub(crate) fn godot_int<T: TryInto<i64>>(count: T) -> i64 {
    count.try_into().unwrap_or(i64::MAX)
}

/// A text the bridge may not have, as GDScript reads it: `""` is none.
///
/// Every dictionary key and byte array that carries one says so in its own doc ("empty
/// when there was none", "empty before a Load"). A refusal, an error, a file name or a
/// sentence is never the empty text, so `""` cannot be mistaken for one; a playbook's
/// bytes may be empty, and then the two answers are the same bytes Save would write.
pub(crate) fn text_or_empty(text: Option<&str>) -> &str {
    text.unwrap_or_default()
}
