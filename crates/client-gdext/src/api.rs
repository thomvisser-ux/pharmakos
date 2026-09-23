// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Gateway results in, canonical JSON out — the other half of what the bridge marshals.
//!
//! The editor and the views ask the gateway for things and get `gp.api.v1` results back.
//! Between the socket and the GDScript there has to be exactly one step, and this is it:
//! **decode the text against the schema, and hand the value on**. Nothing here validates a
//! playbook, prices anything, converts a duration or decides what a result means; the
//! editor "runs no validation or time maths of its own" and neither does the bridge
//! (AGENTS.md section 3 rule 4).
//!
//! Two things make that more than a promise:
//!
//! * the method-to-type mapping is **derived from the schema**, not written down here.
//!   `Method::GET_STATUS` is `gp.api.v1.GetStatusResponse` by the mechanical rule the
//!   `.proto` already follows, and [`response_type`] checks the schema declares it. A
//!   hand-kept table would be a second source of truth for the wire, which is precisely
//!   what "Protobuf is the single source" forbids;
//! * the decode is `pharmakos-proto`'s, so an **unknown field is rejected with a JSON
//!   Pointer** rather than stripped (spec section 10, "Load never strips"). A bridge that
//!   parsed the JSON itself would be a second, laxer reader of the gateway's wire.
//!
//! # Until the gateway is there
//!
//! T12's bridge meets committed fixtures, not a live server — "which is marshalling and
//! needs no live server; it meets the real gateway at T16" (skeleton plan T12, Needs).
//! The fixtures under `tests/fixtures/` are generated **through this crate's own codec**
//! rather than typed by hand, so they are canonical by construction; `tests/fixtures.rs`
//! regenerates and compares them.

use pharmakos_proto::gp::api::v1::Method;
use pharmakos_proto::json::{self, Json};
use pharmakos_proto::{descriptor, scope};

use crate::error::BridgeError;

/// The `gp.api.v1` method a wire name stands for.
///
/// The spelling is the gateway's own, lower case and dotted where the spec dots it, and
/// the translation is `pharmakos-proto`'s [`scope::from_wire_name`] rather than a second
/// copy of the rule.
///
/// # Errors
///
/// [`BridgeError::UnknownMethod`] when the schema's `Method` enum declares no such value.
pub fn method_from_wire(wire: &str) -> Result<Method, BridgeError> {
    let unknown = || BridgeError::UnknownMethod {
        name: wire.to_owned(),
    };
    let value_name = scope::from_wire_name("gp.api.v1.Method", wire).ok_or_else(unknown)?;
    let declared = descriptor::schema()
        .enumeration("gp.api.v1.Method")
        .ok_or_else(unknown)?;
    let value = declared.value_by_name(&value_name).ok_or_else(unknown)?;
    Method::try_from(value.number)
        .ok()
        .filter(|method| *method != Method::Unspecified)
        .ok_or_else(unknown)
}

/// The fully-qualified name of the message a method answers with.
///
/// Mechanical, and checked: `METHOD_GET_STATUS` becomes `gp.api.v1.GetStatusResponse`, and
/// the schema is asked whether it declares that message. If the `.proto` ever names a
/// response differently, this returns an error at the seam instead of a bridge quietly
/// decoding into the wrong type.
///
/// # Errors
///
/// [`BridgeError::UnknownMethod`] for `METHOD_UNSPECIFIED`, or when the derived name is
/// not a message in the schema.
pub fn response_type(method: Method) -> Result<String, BridgeError> {
    let value_name = method.as_str_name();
    let unknown = || BridgeError::UnknownMethod {
        name: value_name.to_owned(),
    };
    if method == Method::Unspecified {
        return Err(unknown());
    }
    let stem = value_name.strip_prefix("METHOD_").ok_or_else(unknown)?;
    let full_name = format!("gp.api.v1.{}Response", pascal_case(stem));
    if descriptor::schema().message(&full_name).is_none() {
        return Err(BridgeError::UnknownMethod { name: full_name });
    }
    Ok(full_name)
}

/// One gateway result, read against the schema and handed on as canonical JSON.
///
/// The round trip through the wire form is what does the checking: the text is read, laid
/// out against the message the method answers with, and written back in canonical order.
/// A field the message does not declare stops it here, with a pointer into the document.
///
/// # Errors
///
/// [`BridgeError::UnknownMethod`] when the method is not one the schema declares, and
/// [`BridgeError::Schema`] when the text is not a valid result of that type.
pub fn decode_result(wire_method: &str, text: &str) -> Result<Json, BridgeError> {
    let method = method_from_wire(wire_method)?;
    let full_name = response_type(method)?;
    // What the live gateway sends is not quite canonical proto JSON, in two ways the
    // T12 fixtures did not show: every result carries the `_status` footer, which is an
    // envelope concern and not a field of the message, and an enum value is spelt the
    // lower-case wire way (decisions-log item 80). The footer is set aside and the enum
    // spelling translated back through the schema (`crate::enums`); everything else is
    // read exactly as strictly as before.
    let (body, _) = crate::view::split_footer(&json::read(text)?);
    let value = crate::enums::canonical(&full_name, &body);
    let bytes = json::json_to_wire(&full_name, &value)?;
    Ok(json::wire_to_json(&full_name, &bytes)?)
}

/// The canonical text of a gateway result, for a fixture or a log line.
///
/// # Errors
///
/// As [`decode_result`].
pub fn canonical_result(wire_method: &str, text: &str) -> Result<String, BridgeError> {
    Ok(json::write(&decode_result(wire_method, text)?))
}

/// `GET_STATUS` becomes `GetStatus`, `LIST_BEACONS` becomes `ListBeacons`.
///
/// Protobuf's own name mapping, and the only string handling in this file.
fn pascal_case(screaming: &str) -> String {
    let mut out = String::with_capacity(screaming.len());
    for word in screaming.split('_') {
        let mut characters = word.chars();
        if let Some(first) = characters.next() {
            out.extend(first.to_uppercase());
            out.push_str(&characters.as_str().to_ascii_lowercase());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wire_method_name_resolves_through_the_schema() {
        assert_eq!(
            method_from_wire("get_status").expect("a declared method"),
            Method::GetStatus
        );
        assert_eq!(
            method_from_wire("submit_plan").expect("a declared method"),
            Method::SubmitPlan
        );
    }

    #[test]
    fn a_method_the_schema_does_not_declare_is_refused() {
        let error = method_from_wire("summon_a_second_seat").expect_err("no such method");
        assert!(
            matches!(error, BridgeError::UnknownMethod { .. }),
            "{error}"
        );
    }

    /// The four methods `gateway.proto` says have no request/response pair yet, in its
    /// own words: "`query_area`, `list_known_enemies`, `get_reports` and
    /// `get_capabilities`. That is the third case, not an oversight."
    const PAIRLESS: &[&str] = &[
        "METHOD_GET_CAPABILITIES",
        "METHOD_GET_REPORTS",
        "METHOD_LIST_KNOWN_ENEMIES",
        "METHOD_QUERY_AREA",
    ];

    #[test]
    fn every_declared_method_has_a_response_message_except_the_four_that_say_they_do_not() {
        // The mechanical rule, checked against the whole enum rather than against the two
        // methods a test happened to name, and the pairless set pinned by name rather than
        // skipped. A fifth method losing its response type is then a red test here — in
        // the bridge that depends on the convention — rather than a run-time surprise in
        // the editor; and a pair arriving for one of these four is a red test too, which
        // is the reminder to delete its line.
        let declared = descriptor::schema()
            .enumeration("gp.api.v1.Method")
            .expect("gp.api.v1.Method is in the schema");
        let mut pairless: Vec<String> = Vec::new();
        let mut checked = 0_u32;
        for value in &declared.values {
            let Ok(method) = Method::try_from(value.number) else {
                continue;
            };
            if method == Method::Unspecified {
                continue;
            }
            match response_type(method) {
                Ok(_) => checked = checked.saturating_add(1),
                Err(_) => pairless.push(value.name.clone()),
            }
        }
        pairless.sort();
        assert_eq!(pairless, PAIRLESS, "the pairless set moved");
        assert!(checked > 10, "only {checked} methods were checked");
    }

    #[test]
    fn a_method_with_no_pair_yet_is_refused_rather_than_guessed_at() {
        let error = decode_result("query_area", "{}")
            .expect_err("query_area has no response message until S3's knowledge store");
        assert!(
            matches!(error, BridgeError::UnknownMethod { .. }),
            "{error}"
        );
    }

    #[test]
    fn a_result_round_trips_into_canonical_form() {
        let canonical = canonical_result(
            "get_status",
            r#"{"status":{"phase":"LULL","phaseRemainingMs":90000,"round":1}}"#,
        )
        .expect("a valid GetStatusResponse");
        assert!(canonical.contains("\"phase\""), "{canonical}");
        // Read it back: the canonical form is itself a valid result.
        canonical_result("get_status", &canonical).expect("canonical form round-trips");
    }

    #[test]
    fn an_unknown_field_is_rejected_with_a_pointer_rather_than_stripped() {
        let error = decode_result("get_status", r#"{"status":{"phase":"LULL"},"extra":1}"#)
            .expect_err("`extra` is not a field of GetStatusResponse");
        let BridgeError::Schema { pointer, message } = error else {
            panic!("expected a schema error");
        };
        assert!(
            pointer.contains("extra") || message.contains("extra"),
            "{pointer}: {message}"
        );
    }

    #[test]
    fn pascal_case_follows_protobufs_own_mapping() {
        assert_eq!(pascal_case("GET_STATUS"), "GetStatus");
        assert_eq!(pascal_case("LIST_BEACONS"), "ListBeacons");
        assert_eq!(pascal_case("GET_ECONOMY_FORECAST"), "GetEconomyForecast");
    }
}
