// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The gateway's spelling of an enum value, translated back to the schema's.
//!
//! Decisions-log item 80: the gateway writes an enum value on the wire as a **lower-case
//! string** with the enum's own prefix dropped — a phase is `"lull"`, an entity kind is
//! `"unit"` — where canonical proto JSON, which `pharmakos-proto`'s codec reads, writes the
//! value name verbatim (`"LULL"`, `"UNIT"`). The translation is `pharmakos-proto`'s own
//! [`scope::from_wire_name`], applied here field by field against the schema, so this crate
//! keeps no table of its own and a value the schema does not declare is left exactly as it
//! arrived — for the codec to refuse with a pointer, as it refuses anything else it does
//! not know.
//!
//! Nothing else is changed: a field name, a number and a string that is not an enum value
//! pass through untouched.

use pharmakos_proto::descriptor::{self, ScalarKind};
use pharmakos_proto::json::Json;
use pharmakos_proto::scope;

/// `value`, read as the message `full_name`, with every enum field spelt as the schema
/// spells it.
#[must_use]
pub fn canonical(full_name: &str, value: &Json) -> Json {
    let Some(message) = descriptor::schema().message(full_name) else {
        return value.clone();
    };
    let Json::Object(entries) = value else {
        return value.clone();
    };
    let translated = entries
        .iter()
        .map(|(key, entry)| {
            let field = message
                .field_by_name(key)
                .or_else(|| message.fields.iter().find(|field| field.json_name == *key));
            let converted = match field {
                Some(field) => {
                    let one = |item: &Json| convert(field.kind, &field.type_name, item);
                    match entry {
                        Json::Array(items) if field.repeated => {
                            Json::Array(items.iter().map(one).collect())
                        }
                        other => one(other),
                    }
                }
                None => entry.clone(),
            };
            (key.clone(), converted)
        })
        .collect();
    Json::Object(translated)
}

/// One value of one field.
fn convert(kind: ScalarKind, type_name: &str, value: &Json) -> Json {
    match (kind, value) {
        (ScalarKind::Enum, Json::String(wire)) => {
            Json::String(scope::from_wire_name(type_name, wire).unwrap_or_else(|| wire.clone()))
        }
        (ScalarKind::Message, Json::Object(_)) => canonical(type_name, value),
        _ => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::canonical;
    use pharmakos_proto::json::{Json, read, write};

    #[test]
    fn a_lower_case_phase_becomes_the_value_name() {
        let status = read(r#"{"phase":"lull","round":1}"#).expect("json");
        let out = canonical("gp.api.v1.Status", &status);
        assert_eq!(out.get("phase"), Some(&Json::String("LULL".to_owned())));
    }

    #[test]
    fn nested_and_repeated_enums_are_translated_and_nothing_else_moves() {
        let view = read(
            r#"{"at_ms":5,"entities":[{"id":"u_1","kind":"unit","subtype":"lull"}],"complete":true}"#,
        )
        .expect("json");
        let out = write(&canonical("gp.api.v1.GetViewResponse", &view));
        assert!(out.contains("\"UNIT\""), "{out}");
        assert!(
            out.contains("\"lull\""),
            "a string field is not an enum: {out}"
        );
    }

    #[test]
    fn a_value_the_schema_does_not_declare_is_left_for_the_codec_to_refuse() {
        let status = read(r#"{"phase":"siesta"}"#).expect("json");
        let out = canonical("gp.api.v1.Status", &status);
        assert_eq!(out.get("phase"), Some(&Json::String("siesta".to_owned())));
    }
}
