// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The codec's grammar at its edges (S3's `vfy6`, follow-ups B and C).
//!
//! * **B.** Both JSON readers, the tree's (`json::read`) and the direct one
//!   (`json::decode_direct`), read numbers by RFC 8259's grammar: no `+`, no
//!   leading zero, a digit on each side of a `.` and after an exponent. A
//!   quoted integer, which the proto3 mapping also accepts, is held to the
//!   same integer grammar, so `"+0"` and `"00"` are refused as `+0` and `00`
//!   are.
//!
//! Every case is put to both readers, and both must answer alike: the direct
//! reader answers a document exactly when the exact path accepts it.

use pharmakos_proto::gp::v1::{Playbook, SurveySettings};
use pharmakos_proto::json::{self, Direct, Json};

/// Whether both readers accept `text` as a `T`, after checking that they
/// agree on it.
fn accepted<T>(text: &str) -> bool
where
    T: prost::Message + prost::Name + Default + PartialEq + core::fmt::Debug,
{
    let exact = json::decode_encoded::<T>(text);
    let direct = json::decode_direct::<T>(text);
    match (&exact, &direct) {
        (Ok(decoded), Direct::Wire(fast)) => assert_eq!(decoded, fast, "{text}"),
        (Err(_), Direct::NeedsExactPath(_)) => {}
        _ => panic!("the readers disagree on {text}: {exact:?} against {direct:?}"),
    }
    exact.is_ok()
}

#[test]
fn a_number_is_read_by_rfc_8259s_grammar() {
    for good in [
        "0", "-0", "7", "-7", "10", "1.5", "-0.5", "1e3", "1E+3", "1e-3", "-1.25e-7", "0e0",
    ] {
        assert!(
            matches!(json::read(good), Ok(Json::Number(ref lexeme)) if lexeme == good),
            "{good}"
        );
    }
    for bad in [
        "+1", "+0", "01", "00", "-01", "-00", "007", "1.", ".5", "-.5", "1.e3", "1e", "1e+", "1E-",
        "-", "--1", "+", "0x10", "1.5.5", "1e3e3", "1-2", "1+2",
    ] {
        assert!(json::read(bad).is_err(), "{bad} should be refused");
        assert!(
            json::read(&format!("[{bad}]")).is_err(),
            "[{bad}] should be refused"
        );
    }
}

#[test]
fn an_integer_field_takes_no_plus_and_no_leading_zero_bare_or_quoted() {
    let area = |x: &str| format!(r#"{{"probe_areas":[{{"min":{{"x":{x}}}}}]}}"#);
    let count = |n: &str| format!(r#"{{"scout_count":{n}}}"#);
    for good in [
        "0",
        "-0",
        "7",
        "-7",
        "2147483647",
        "\"0\"",
        "\"-0\"",
        "\"7\"",
        "\"-7\"",
    ] {
        assert!(accepted::<SurveySettings>(&area(good)), "x = {good}");
    }
    for good in ["0", "1", "\"0\"", "\"42\"", "4294967295"] {
        assert!(accepted::<SurveySettings>(&count(good)), "count = {good}");
    }
    for bad in [
        "+1", "+0", "01", "00", "-01", "\"+0\"", "\"+3\"", "\"00\"", "\"01\"", "\"-01\"", "\" 1\"",
        "\"1 \"", "\"\"", "\"-\"", "\"1.0\"", "\"1e3\"",
    ] {
        assert!(!accepted::<SurveySettings>(&area(bad)), "x = {bad}");
        assert!(!accepted::<SurveySettings>(&count(bad)), "count = {bad}");
    }
}

#[test]
fn a_quoted_zero_with_a_sign_or_a_leading_zero_is_refused_not_dropped_as_a_default() {
    // A zero at a field's default is dropped from the wire, so a spelling of
    // zero the grammar refuses must be refused before it can be dropped.
    for bad in ["\"+0\"", "\"00\"", "\"-00\"", "+0", "00"] {
        let text = format!(r#"{{"scout_count":{bad}}}"#);
        assert!(!accepted::<SurveySettings>(&text), "{text}");
    }
    // An enum given by number is an integer too.
    for bad in ["+0", "00", "\"+0\""] {
        let text = format!(r#"{{"kind":{bad}}}"#);
        assert!(!accepted::<Playbook>(&text), "{text}");
    }
    assert!(accepted::<Playbook>(r#"{"kind":0}"#));
    assert!(accepted::<Playbook>(r#"{"kind":-0}"#));
}
