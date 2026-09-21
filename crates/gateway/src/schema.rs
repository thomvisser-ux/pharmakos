// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `get_schema`: JSON Schema **generated from the schema**, so it cannot
//! drift.
//!
//! Spec section 12, "Generated docs": `get_schema` and the `gamectl` docs
//! output "are generated from the schema so they can't drift". This module is
//! the generating half, and `tests/golden/schema/` is the committed half —
//! that README states the rule this file exists to keep: "`get_schema`'s output
//! moved but the JSON Schema did not" means "the gateway is building its
//! answer rather than serving the generated artefact".
//!
//! So nothing here is written down twice. Every property, every enum value and
//! every nested reference is walked out of
//! [`pharmakos_proto::descriptor::schema()`] — the descriptor set `buf build`
//! produces from `proto/**` and `crates/proto` checks in — which is the same
//! source the canonical JSON codec, the reserved-number table and the
//! method/scope table are all driven from. A field added to `gp.v1` appears in
//! this output the moment the tree is regenerated, and the golden moves in the
//! pull request that regenerated it.
//!
//! # What is served, and what the `part` selects
//!
//! A seat authors exactly one thing — a playbook (AGENTS.md section 2) — so
//! the whole of what a seat can use is `gp.v1.Playbook` and everything it
//! reaches. `part` names a smaller root by its `lower_snake_case` short name,
//! which is the spelling spec section 12's worked example types
//! (`get_schema{part:"step"}`), and the slice is that message and everything
//! *it* reaches.
//!
//! `gp.api.v1` is deliberately not reachable from here. It is the transport,
//! not the vocabulary, and v1 publishes nothing (spec section 12); a seat that
//! wanted the method list has the method it is already calling.
//!
//! # Two things the reader should know about the dialect
//!
//! * **Property names are the `.proto` spelling**, because that is what the
//!   canonical form writes and what a player hand-edits (`crates/proto`'s
//!   codec docs). The proto3 JSON mapping also lets a *reader* accept the
//!   lowerCamelCase alternate, and this gateway's does; the schema describes
//!   the canonical form, and says so in its own `description`.
//! * **A `oneof` is written as pairwise exclusion** — one
//!   `{"not": {"required": [a, b]}}` per pair, under `allOf`. Longer than a
//!   `oneOf` of arms and exactly true: a proto3 `oneof` is *at most* one arm,
//!   never exactly one, and an editor that read "exactly one" would demand an
//!   arm on every optional group in the file.

use pharmakos_proto::descriptor::{Field, Message, ScalarKind, Schema, schema};
use pharmakos_proto::json::Json;
use std::collections::{BTreeMap, BTreeSet};

use crate::error::Error;

/// The JSON Schema dialect the generated document declares.
pub const DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";

/// The root a `part` of `""` names: everything a seat can author.
pub const ROOT: &str = "gp.v1.Playbook";

/// The package a `part` may name a root in.
const PACKAGE: &str = "gp.v1.";

/// The generated document for one `part`, as canonical JSON text.
///
/// The proto crate's writer, so the output is the one-entry-per-line form
/// every other JSON in this project is written as — which is what makes the
/// golden diffable (`tests/golden/README.md` rule 3).
///
/// # Errors
///
/// As [`document`].
pub fn text(part: &str) -> Result<String, Error> {
    Ok(pharmakos_proto::json::write(&document(part)?))
}

/// The generated document for one `part`.
///
/// # Errors
///
/// [`crate::error::Code::NotFound`] when `part` names no message of `gp.v1`,
/// and [`crate::error::Code::Internal`] when the checked-in descriptor set does
/// not carry the root — which would mean the generated tree and this build
/// disagree, and is nothing the caller did.
pub fn document(part: &str) -> Result<Json, Error> {
    let schema = schema();
    let root = root_of(schema, part)?;
    let message = schema.message(&root).ok_or_else(|| {
        Error::internal(format!("`{root}` is not in the checked-in descriptor set"))
    })?;

    // Ordered, and ordered twice over: the walk is breadth-first from the root
    // so a reader meets the types in the order the root reaches them, and the
    // definitions come out in name order so two machines write the same bytes
    // (AGENTS.md section 4.6).
    let mut defs: BTreeMap<String, Json> = BTreeMap::new();
    // Which full name each `$defs` key came from. A key is the full name's last
    // segment, which is short and readable and is what every `$ref` in the
    // document spells -- and which is **not unique inside `gp.v1`**:
    // `playbook.proto` already declares two enums called `Kind`. Today only one
    // of them is reachable from any root, so nothing collides; the day a field
    // references the other, one definition would quietly overwrite the other
    // and every `$ref: "#/$defs/Kind"` would point at the wrong type, with the
    // golden agreeing because the golden is the output. So the walk records
    // where each key came from and refuses to write a second type under a key
    // another type already holds.
    let mut source: BTreeMap<String, String> = BTreeMap::new();
    let mut enums: BTreeSet<String> = BTreeSet::new();
    let mut pending: Vec<String> = vec![message.full_name.clone()];
    let mut seen: BTreeSet<String> = BTreeSet::new();
    while let Some(name) = pending.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let Some(nested) = schema.message(&name) else {
            continue;
        };
        for field in &nested.fields {
            match field.kind {
                ScalarKind::Message => pending.push(field.type_name.clone()),
                ScalarKind::Enum => {
                    enums.insert(field.type_name.clone());
                }
                _ => {}
            }
        }
        claim(&mut source, &name)?;
        defs.insert(short(&name), message_schema(schema, nested));
    }
    for name in &enums {
        if let Some(declared) = schema.enumeration(name) {
            claim(&mut source, name)?;
            let values: Vec<Json> = declared
                .values
                .iter()
                .map(|value| Json::String(value.name.clone()))
                .collect();
            defs.insert(
                short(name),
                Json::Object(vec![
                    (String::from("type"), Json::String(String::from("string"))),
                    (
                        String::from("description"),
                        Json::String(format!(
                            "`{name}`. A playbook on disk carries the value name verbatim."
                        )),
                    ),
                    (String::from("enum"), Json::Array(values)),
                ]),
            );
        }
    }

    let definitions: Vec<(String, Json)> = defs.into_iter().collect();
    Ok(Json::Object(vec![
        (String::from("$schema"), Json::String(String::from(DIALECT))),
        (
            String::from("$id"),
            Json::String(format!("pharmakos:{}", message.full_name)),
        ),
        (
            String::from("title"),
            Json::String(message.full_name.clone()),
        ),
        (
            String::from("description"),
            Json::String(String::from(
                "Canonical proto3 JSON for the Pharmakos playbook vocabulary, generated from the \
                 checked-in descriptor set. Property names are the .proto spelling, which is what \
                 the canonical form writes and what a player hand-edits; a reader also accepts the \
                 lowerCamelCase alternates the proto3 JSON mapping defines. An unknown field is \
                 rejected and never stripped.",
            )),
        ),
        (
            String::from("$ref"),
            Json::String(format!("#/$defs/{}", short(&message.full_name))),
        ),
        (String::from("$defs"), Json::Object(definitions)),
    ]))
}

/// The full name a `part` selects.
fn root_of(schema: &Schema, part: &str) -> Result<String, Error> {
    if part.is_empty() {
        return Ok(String::from(ROOT));
    }
    if !part
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(Error::invalid(format!(
            "`{part}` is not a schema part: a part is the lower_snake_case name of a `gp.v1` \
             message, as in `get_schema{{part:\"step\"}}`"
        )));
    }
    let mut found: Vec<&str> = schema
        .messages()
        .map(|message| message.full_name.as_str())
        .filter(|name| name.starts_with(PACKAGE) && short_snake(name) == part)
        .collect();
    found.sort_unstable();
    match found.first() {
        Some(name) => Ok((*name).to_owned()),
        None => Err(Error::not_found(format!(
            "`{part}` is not a part of the playbook schema"
        ))),
    }
}

/// One message as a JSON Schema object.
fn message_schema(schema: &Schema, message: &Message) -> Json {
    let mut properties: Vec<(String, Json)> = Vec::new();
    for field in &message.fields {
        properties.push((field.name.clone(), field_schema(schema, field)));
    }
    let mut entries: Vec<(String, Json)> = vec![
        (String::from("type"), Json::String(String::from("object"))),
        (
            String::from("description"),
            Json::String(message.full_name.clone()),
        ),
        (
            String::from("additionalProperties"),
            // Spec section 10: "Load never strips". A schema that allowed an
            // unknown field would describe a file this gateway refuses.
            Json::Bool(false),
        ),
        (String::from("properties"), Json::Object(properties)),
    ];
    let exclusions = oneof_exclusions(message);
    if !exclusions.is_empty() {
        entries.push((String::from("allOf"), Json::Array(exclusions)));
    }
    Json::Object(entries)
}

/// One `{"not": {"required": [a, b]}}` per pair of arms of each `oneof`.
fn oneof_exclusions(message: &Message) -> Vec<Json> {
    let mut out: Vec<Json> = Vec::new();
    for (index, _) in message.oneofs.iter().enumerate() {
        let index = i32::try_from(index).unwrap_or(i32::MAX);
        let arms: Vec<&str> = message
            .fields
            .iter()
            .filter(|field| field.oneof_index == Some(index))
            .map(|field| field.name.as_str())
            .collect();
        for (first_index, first) in arms.iter().enumerate() {
            for second in arms.iter().skip(first_index.saturating_add(1)) {
                out.push(Json::Object(vec![(
                    String::from("not"),
                    Json::Object(vec![(
                        String::from("required"),
                        Json::Array(vec![
                            Json::String((*first).to_owned()),
                            Json::String((*second).to_owned()),
                        ]),
                    )]),
                )]));
            }
        }
    }
    out
}

/// One field as a JSON Schema value.
fn field_schema(schema: &Schema, field: &Field) -> Json {
    let inner = scalar_schema(schema, field);
    if field.repeated {
        return Json::Object(vec![
            (String::from("type"), Json::String(String::from("array"))),
            (String::from("items"), inner),
        ]);
    }
    inner
}

/// One field's element type.
fn scalar_schema(_schema: &Schema, field: &Field) -> Json {
    let simple = |name: &str| {
        Json::Object(vec![(
            String::from("type"),
            Json::String(String::from(name)),
        )])
    };
    match field.kind {
        ScalarKind::Message | ScalarKind::Enum => Json::Object(vec![(
            String::from("$ref"),
            Json::String(format!("#/$defs/{}", short(&field.type_name))),
        )]),
        ScalarKind::Bool => simple("boolean"),
        ScalarKind::String => simple("string"),
        ScalarKind::Bytes => Json::Object(vec![
            (String::from("type"), Json::String(String::from("string"))),
            (
                String::from("contentEncoding"),
                Json::String(String::from("base64")),
            ),
        ]),
        // There are no floating-point fields in either package and the codec
        // refuses one on sight, so this arm describes a field that cannot
        // exist. It is here because the descriptor's type enum has the
        // variant, and a schema that silently wrote `integer` for a float
        // would be the drift this module exists to make impossible.
        ScalarKind::Double | ScalarKind::Float => Json::Object(vec![(
            String::from("not"),
            Json::Object(vec![(
                String::from("description"),
                Json::String(String::from(
                    "a floating-point field: the sim is integer-only and the codec refuses one",
                )),
            )]),
        )]),
        _ => simple("integer"),
    }
}

/// Record which full name owns a `$defs` key, or refuse a second claimant.
///
/// Re-walking the same type is ordinary and is not a collision: the walk is
/// breadth-first over a graph and `seen` already stops it, but an enum reached
/// from two messages arrives here twice.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] when two different `gp.v1` types share a
/// last segment and both are reachable from this root. It is `INTERNAL` and not
/// a client's fault by construction: it means the schema grew a name clash the
/// generator cannot render, and the answer is to key `$defs` by something
/// longer -- not to serve a document whose `$ref`s point at the wrong type.
fn claim(source: &mut BTreeMap<String, String>, full_name: &str) -> Result<(), Error> {
    match source.insert(short(full_name), full_name.to_owned()) {
        Some(held) if held != full_name => Err(Error::internal(format!(
            "`{held}` and `{full_name}` would both be written as `$defs/{}`, so one would \
             overwrite the other and every reference to it would name the wrong type",
            short(full_name)
        ))),
        _ => Ok(()),
    }
}

/// A full name's last segment: `gp.v1.BeaconFilter.MandateKind` -> `MandateKind`.
fn short(full_name: &str) -> String {
    full_name.rsplit('.').next().unwrap_or(full_name).to_owned()
}

/// A full name's last segment in `lower_snake_case`, which is what a `part`
/// spells: `gp.v1.SchemaVersion` -> `schema_version`.
fn short_snake(full_name: &str) -> String {
    let mut out = String::new();
    for character in short(full_name).chars() {
        if character.is_ascii_uppercase() {
            if !out.is_empty() {
                out.push('_');
            }
            out.push(character.to_ascii_lowercase());
        } else {
            out.push(character);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{DIALECT, ROOT, document, short_snake, text};
    use crate::error::Code;
    use pharmakos_proto::json::Json;

    fn defs(value: &Json) -> Vec<String> {
        match value.get("$defs") {
            Some(Json::Object(entries)) => entries.iter().map(|(key, _)| key.clone()).collect(),
            _ => panic!("a $defs object"),
        }
    }

    #[test]
    fn the_whole_schema_is_the_playbook_and_everything_it_reaches() {
        let whole = document("").expect("a document");
        assert_eq!(whole.get("title"), Some(&Json::String(String::from(ROOT))));
        assert_eq!(
            whole.get("$schema"),
            Some(&Json::String(String::from(DIALECT)))
        );
        let names = defs(&whole);
        for wanted in ["Playbook", "Step", "Meta", "SchemaVersion", "Voxel"] {
            assert!(names.iter().any(|name| name == wanted), "{wanted} missing");
        }
        assert!(
            !names.iter().any(|name| name == "VerifyReport"),
            "gp.api.v1 is the transport, not the vocabulary"
        );
    }

    #[test]
    fn a_part_names_a_smaller_root_and_is_the_specs_own_spelling() {
        let slice = document("step").expect("a document");
        assert_eq!(
            slice.get("title"),
            Some(&Json::String(String::from("gp.v1.Step")))
        );
        let whole = defs(&document("").expect("a document")).len();
        assert!(defs(&slice).len() < whole, "a slice is smaller");
        assert_eq!(
            document("STEP").expect_err("refused").code,
            Code::InvalidArgument,
            "a part is lower case"
        );
        assert_eq!(
            document("no_such_message").expect_err("refused").code,
            Code::NotFound
        );
    }

    #[test]
    fn a_message_refuses_an_unknown_field_because_load_never_strips() {
        let whole = document("").expect("a document");
        let playbook = whole
            .get("$defs")
            .and_then(|defs| defs.get("Playbook"))
            .expect("the root definition");
        assert_eq!(
            playbook.get("additionalProperties"),
            Some(&Json::Bool(false))
        );
        assert!(
            playbook
                .get("properties")
                .and_then(|properties| properties.get("schema_version"))
                .is_some(),
            "the .proto spelling, not schemaVersion"
        );
    }

    #[test]
    fn a_oneof_is_written_as_pairwise_exclusion() {
        let slice = document("step").expect("a document");
        let step = slice
            .get("$defs")
            .and_then(|defs| defs.get("Step"))
            .expect("the step definition");
        let Some(Json::Array(exclusions)) = step.get("allOf") else {
            panic!("Step has a oneof and should carry its exclusions");
        };
        assert!(!exclusions.is_empty());
        for entry in exclusions {
            let required = entry
                .get("not")
                .and_then(|not| not.get("required"))
                .expect("a pair");
            match required {
                Json::Array(pair) => assert_eq!(pair.len(), 2),
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn no_two_types_of_the_vocabulary_share_a_defs_key() {
        // `document` refuses rather than overwriting, so this is a test that
        // the whole vocabulary still renders -- and the assertion that makes it
        // a test of the KEYS rather than of nothing: as many `$defs` entries as
        // the document has distinct `$ref` targets, with no key written twice.
        let whole = document("").expect("the vocabulary renders under short keys");
        let mut names = defs(&whole);
        let written = names.len();
        names.sort();
        names.dedup();
        assert_eq!(written, names.len(), "a `$defs` key was written twice");
        for part in ["step", "meta", "schema_version"] {
            let _ = document(part).expect("every part renders too");
        }
    }

    #[test]
    fn the_document_is_a_function_of_the_descriptor_set_alone() {
        assert_eq!(text("").expect("once"), text("").expect("twice"));
        assert!(text("").expect("text").ends_with('\n'));
    }

    #[test]
    fn a_short_name_snakes_the_way_the_spec_types_it() {
        assert_eq!(short_snake("gp.v1.Step"), "step");
        assert_eq!(short_snake("gp.v1.SchemaVersion"), "schema_version");
        assert_eq!(
            short_snake("gp.v1.BeaconFilter.MandateKind"),
            "mandate_kind"
        );
    }
}
