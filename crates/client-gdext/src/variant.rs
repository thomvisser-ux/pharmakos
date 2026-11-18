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

/// A count that Godot's `int` (an `i64`) cannot hold: its value, as digits.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct NotAnInt(pub(crate) String);

/// A count as Godot's `int`, which is an `i64`, or the typed refusal [`NotAnInt`] for one it
/// cannot hold: never clamped to `i64::MAX` (the rulings on part 2, carried from part 1's
/// review). It is the one place the bridge narrows a count, so the rule is written once.
pub(crate) fn godot_int<T: TryInto<i64> + Copy + std::fmt::Display>(
    count: T,
) -> Result<i64, NotAnInt> {
    count.try_into().map_err(|_| NotAnInt(count.to_string()))
}

/// A count as the `Variant` a dictionary holds ([`godot_int`]). One past `i64` is not clamped:
/// it reaches GDScript as `{"not_an_int": "<its digits>"}`, a typed refusal in the value's own
/// place that no script can read as a number, and the log says which value it was.
pub(crate) fn int_variant<T: TryInto<i64> + Copy + std::fmt::Display>(count: T) -> Variant {
    match godot_int(count) {
        Ok(value) => value.to_variant(),
        Err(NotAnInt(digits)) => {
            godot::global::godot_error!(
                "[pharmakos] a count of {digits} is past what Godot's int holds; it is sent as                  a refusal, not as a number"
            );
            let mut refusal = VarDictionary::new();
            refusal.set(&"not_an_int".to_variant(), &digits.to_variant());
            refusal.to_variant()
        }
    }
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

/// An integer field of a gateway answer read laxly by name, as proto3 JSON writes it: an
/// absent field is 0 (proto3's own rule for an unwritten integer), and a present one is a
/// number or, as proto3 JSON's parsers accept for every integer type, a quoted number.
/// The one reader the rig and the editor share, so the two never disagree about a value.
///
/// # Errors
///
/// [`crate::error::BridgeError::Rpc`] for a field that is neither, or past an `i64`.
pub(crate) fn proto3_integer(answer: &Json, name: &str) -> Result<i64, crate::error::BridgeError> {
    let refused = || crate::error::BridgeError::Rpc(format!("`{name}` is not an integer"));
    match answer.get(name) {
        None => Ok(0),
        Some(Json::Number(lexeme) | Json::String(lexeme)) => {
            lexeme.parse::<i64>().map_err(|_| refused())
        }
        Some(_) => Err(refused()),
    }
}

#[cfg(test)]
mod tests {
    use super::{NotAnInt, godot_int};

    /// A count `i64` holds is itself; one it does not is refused with its digits, never
    /// clamped to `i64::MAX`.
    #[test]
    fn a_count_past_i64_is_refused_not_clamped() {
        assert_eq!(godot_int(42_u64), Ok(42));
        assert_eq!(godot_int(usize::MIN), Ok(0));
        assert_eq!(
            godot_int(u64::MAX),
            Err(NotAnInt("18446744073709551615".to_owned()))
        );
        assert_eq!(godot_int(i64::MAX.unsigned_abs()), Ok(i64::MAX));
        assert_eq!(
            godot_int(i64::MAX.unsigned_abs().saturating_add(1)),
            Err(NotAnInt("9223372036854775808".to_owned()))
        );
    }
}
