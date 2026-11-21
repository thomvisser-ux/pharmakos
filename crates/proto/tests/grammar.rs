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

/// One quoting rule for a player's text in a refusal (S3's `vfyq2`, fix
/// pass 1, item 5): controls as `\u00XX`, at most `MAX_QUOTED_CHARS`
/// characters, the rest `…`.
#[test]
fn a_quote_escapes_controls_and_stops_at_the_cap() {
    use pharmakos_proto::json::grammar::{MAX_QUOTED_CHARS, quoted};
    let shown = |text: &str| quoted(text).as_str().to_owned();
    assert_eq!(shown("plain é"), "plain é");
    assert_eq!(shown("\u{1b}]0;x\u{7}"), r"\u001b]0;x\u0007");
    assert_eq!(shown("a\u{7f}b\nc"), r"a\u007fb\u000ac");
    let exact = "k".repeat(MAX_QUOTED_CHARS);
    assert_eq!(shown(&exact), exact);
    let long = "é".repeat(200_000);
    let capped = shown(&long);
    assert_eq!(capped.chars().count(), MAX_QUOTED_CHARS + 1);
    assert!(capped.ends_with('…'), "{capped}");
    // The characters that reorder or hide text (fix pass 2, item B), each
    // range at its ends.
    for character in [
        '\u{061c}', '\u{200b}', '\u{200f}', '\u{2028}', '\u{2029}', '\u{202a}', '\u{202e}',
        '\u{2066}', '\u{2069}', '\u{feff}',
    ] {
        let written = format!("\\u{:04x}", u32::from(character));
        assert_eq!(shown(&format!("a{character}b")), format!("a{written}b"));
    }
    // Their neighbours are shown as themselves (U+206A, a neighbour before
    // fix pass 3's item M3, is a format character and is in the table now).
    for character in [
        '\u{200a}', '\u{2010}', '\u{2027}', '\u{202f}', '\u{2065}', '\u{2070}',
    ] {
        let text = format!("a{character}b");
        assert_eq!(shown(&text), text);
    }
}

/// A pointer is quoted token by token (S3's `vfyq2`, fix pass 2b): one of the
/// schema's names, however deep, comes back exactly; a token that is the
/// player's own text is escaped and capped alone, the rest of the pointer kept.
#[test]
fn a_pointer_is_quoted_token_by_token() {
    use pharmakos_proto::json::grammar::{MAX_QUOTED_CHARS, quoted_pointer};
    let long =
        "/declarative/route/0/place_beacon/initial/mandate/defend/protect/0/beacon/beacon_id";
    assert!(long.chars().count() > MAX_QUOTED_CHARS);
    assert_eq!(quoted_pointer(long).as_str(), long);
    assert_eq!(quoted_pointer("").as_str(), "");
    assert_eq!(quoted_pointer("/a~1b/~0").as_str(), "/a~1b/~0");
    let key = format!("{}{}", char::from(0x1b), "k".repeat(200));
    let shown = quoted_pointer(&format!("/meta/{key}/x"));
    let expected = format!(
        "/meta/\\u001b{}\u{2026}/x",
        "k".repeat(MAX_QUOTED_CHARS - 1)
    );
    assert_eq!(shown.as_str(), expected);
}

/// Every format character, line and paragraph separator, control character
/// and noncharacter is escaped (S3's `vfyq2`, fix pass 3, item M3). The
/// review's twenty, which the hand-picked table let through raw, are named:
/// tag characters among them, which can spell a hidden message.
#[test]
fn the_reviews_unlisted_format_characters_are_escaped() {
    use pharmakos_proto::json::grammar::quoted;
    let unlisted: [u32; 22] = [
        0x00AD, 0x0600, 0x06DD, 0x070F, 0x08E2, 0x180E, 0x2060, 0x2061, 0x2064, 0x206A, 0x206F,
        0xFFF9, 0xFFFB, 0x1_10BD, 0x1_3430, 0x1_BCA0, 0x1_D173, 0xE_0001, 0xE_0041, 0xE_007F,
        // Two noncharacters.
        0xFDD0, 0x10_FFFF,
    ];
    let mut raw: Vec<String> = Vec::new();
    for point in unlisted {
        let character = char::from_u32(point).expect("a scalar value");
        if quoted(&format!("a{character}b"))
            .as_str()
            .contains(character)
        {
            raw.push(format!("U+{point:04X}"));
        }
    }
    assert!(raw.is_empty(), "shown raw: {raw:?}");
}

/// Every code point of every committed range is escaped, and a sample of
/// code points outside them comes back as itself (fix pass 3, item M3).
#[test]
fn every_hidden_range_is_escaped_and_its_neighbours_are_not() {
    use pharmakos_proto::json::grammar::{HIDDEN, quoted};
    let mut previous: Option<char> = None;
    for (low, high) in HIDDEN {
        assert!(low <= high, "{low:?} to {high:?}");
        if let Some(previous) = previous {
            assert!(
                u32::from(*low) > u32::from(previous).saturating_add(1),
                "ranges are ascending, disjoint and merged: {low:?}"
            );
        }
        previous = Some(*high);
        for point in u32::from(*low)..=u32::from(*high) {
            let character = char::from_u32(point).expect("a scalar value");
            assert!(
                !quoted(&character.to_string()).as_str().contains(character),
                "U+{point:04X} is shown raw"
            );
        }
    }
    for character in [
        'a', 'é', ' ', '~', '€', '\u{2027}', '\u{2030}', '\u{2070}', '\u{FFFD}', '😀',
    ] {
        let inside = HIDDEN
            .iter()
            .any(|(low, high)| (*low..=*high).contains(&character));
        assert!(!inside, "{character:?} is a neighbour, not a member");
        assert_eq!(
            quoted(&character.to_string()).as_str(),
            character.to_string(),
            "{character:?}"
        );
    }
}

/// The table and the quoting function agree on every `char` (fix pass 3, item
/// M3): the table is searched here by a plain scan, independently of the
/// binary search the quoting function uses.
#[test]
fn the_table_and_the_quote_agree_on_every_char() {
    use pharmakos_proto::json::grammar::{HIDDEN, quoted};
    let mut disagree: Vec<String> = Vec::new();
    let mut walked: u32 = 0;
    for point in 0..=0x10_FFFF_u32 {
        let Some(character) = char::from_u32(point) else {
            continue;
        };
        walked = walked.saturating_add(1);
        let listed = HIDDEN
            .iter()
            .any(|(low, high)| *low <= character && character <= *high);
        let mut one = [0_u8; 4];
        let shown_raw = quoted(character.encode_utf8(&mut one))
            .as_str()
            .contains(character);
        if listed == shown_raw {
            disagree.push(format!("U+{point:04X}"));
        }
    }
    assert_eq!(walked, 1_112_064, "every scalar value");
    assert!(
        disagree.is_empty(),
        "{} disagree: {:?}",
        disagree.len(),
        disagree.iter().take(20).collect::<Vec<_>>()
    );
}

/// The default-ignorable code points are escaped too (S3's `vfyq2`, fix pass
/// 4, item 2): the review's 4,036 invisible code points the categories alone
/// let through, sampled at each range's ends.
#[test]
fn the_default_ignorables_are_escaped() {
    use pharmakos_proto::json::grammar::quoted;
    let ignorable: [u32; 21] = [
        0x034F, 0x115F, 0x1160, 0x17B4, 0x17B5, 0x180B, 0x180D, 0x180F, 0x2065, 0x3164, 0xFE00,
        0xFE0F, 0xFFA0, 0xFFF0, 0xFFF8, 0xE_0000, 0xE_0002, 0xE_001F, 0xE_0080, 0xE_0100, 0xE_0FFF,
    ];
    let raw: Vec<String> = ignorable
        .iter()
        .filter_map(|point| char::from_u32(*point))
        .filter(|character| {
            quoted(&format!("a{character}b"))
                .as_str()
                .contains(*character)
        })
        .map(|character| format!("U+{:04X}", u32::from(character)))
        .collect();
    assert!(raw.is_empty(), "shown raw: {raw:?}");
}
