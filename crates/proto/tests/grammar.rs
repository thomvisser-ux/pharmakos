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
//! * **C.** A field written under both its names, and base64 padding that
//!   does not complete the last quantum, are refused (below).
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

// * **C.** A field written under both its `.proto` name and its JSON name is
//   refused, as a key written twice is, whatever either member holds; and
//   base64 padding that does not complete the last quantum is refused. Both
//   once made bytes the codec accepted but that did not hold what prost would
//   write for the message, which the plan fingerprint then had to work round.

#[test]
fn a_field_under_both_its_names_is_refused_by_both_readers() {
    for text in [
        r#"{"on_death":{},"onDeath":{}}"#,
        r#"{"onDeath":{},"on_death":{}}"#,
        r#"{"on_death":null,"onDeath":{}}"#,
        r#"{"on_death":{},"onDeath":null}"#,
        r#"{"on_death":null,"onDeath":null}"#,
        r#"{"meta":{"author_kind":"HUMAN","authorKind":"HUMAN"}}"#,
        r#"{"schema_version":{"major":1},"schemaVersion":{"minor":2}}"#,
        r#"{"on_death":{"on_respawn":"CONTINUE","onRespawn":"CONTINUE"}}"#,
    ] {
        assert!(!accepted::<Playbook>(text), "{text}");
    }
    // A repeated field too: its two spellings would otherwise concatenate.
    assert!(!accepted::<SurveySettings>(
        r#"{"probe_areas":[{"min":{"x":1}}],"probeAreas":[{"max":{"y":2}}]}"#
    ));
    // Either name alone is fine.
    assert!(accepted::<Playbook>(
        r#"{"on_death":{"on_respawn":"CONTINUE"}}"#
    ));
    assert!(accepted::<Playbook>(
        r#"{"onDeath":{"onRespawn":"CONTINUE"}}"#
    ));
}

#[test]
fn base64_padding_alone_is_refused_not_read_as_empty_bytes() {
    for bad in ["====", "=", "==", "AQ=", "AQ===", "AQID=", "AQID===="] {
        let text = format!(r#"{{"meta":{{"fingerprint":"{bad}"}}}}"#);
        assert!(!accepted::<Playbook>(&text), "{text}");
    }
    for good in ["", "AQ==", "AQ", "AQI=", "AQI", "AAAAAAAAAAE="] {
        let text = format!(r#"{{"meta":{{"fingerprint":"{good}"}}}}"#);
        assert!(accepted::<Playbook>(&text), "{text}");
    }
}

/// A whole number too large for 64 bits is out of range, not "not a whole
/// number" (the review of PR #179).
#[test]
fn an_overlong_integer_is_out_of_range() {
    for (text, kind) in [
        (r#"{"scout_count":99999999999999999999}"#, "unsigned"),
        (
            r#"{"probe_areas":[{"min":{"x":99999999999999999999}}]}"#,
            "signed",
        ),
        (
            r#"{"probe_areas":[{"min":{"x":-99999999999999999999}}]}"#,
            "signed, negative",
        ),
    ] {
        let error = json::decode::<SurveySettings>(text).expect_err(kind);
        assert_eq!(error.kind, json::ErrorKind::Integer, "{kind}");
        assert!(
            error.message.contains("out of range"),
            "{kind}: {}",
            error.message
        );
    }
    let negative = json::decode::<SurveySettings>(r#"{"scout_count":-1}"#).expect_err("-1");
    assert!(
        negative.message.contains("not a non-negative whole number"),
        "{}",
        negative.message
    );
}
