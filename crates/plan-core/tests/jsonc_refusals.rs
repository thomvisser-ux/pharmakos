// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every refusal of the JSONC reader, in its words and at its byte (S3's
//! `vfyq2`, fix pass 1, item 11).
//!
//! `vfyq2` replaced the JSONC reader's own scanner with the one grammar the
//! codec reads by (`pharmakos_proto::json::grammar`), so a text the editor
//! reads (`Document::parse`, behind `render_plan`, `patch_plan`,
//! `instantiate_template` and canonicalisation) is refused in the grammar's
//! words, at the grammar's byte, as the verifier's `E0001` refuses it. That
//! changed what a player sees for most syntax errors. Each row below pins the
//! refusal the reader makes now, beside what `main` (before PR #198) said, so
//! a change of either is a red test and the PR's table is this table.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml's allow-expect-in-tests only recognises #[test] functions, not an \
              integration test's helpers. A panic is this file's failure report."
)]

use pharmakos_plan_core::canonical::canonicalise_text;
use pharmakos_plan_core::jsonc::{CommentKind, Document};
use pharmakos_plan_core::patch::patch_text;
use pharmakos_proto::json::grammar;

const MINIMAL: &str = concat!(
    "{\"schema_version\":{\"major\":1},",
    "\"meta\":{\"title\":\"t\",\"author_kind\":\"HUMAN\"},",
    "\"declarative\":{\"route\":[{\"label\":\"a\",\"hold\":{\"ms\":1000}}]},",
    "\"on_death\":{\"on_respawn\":\"CONTINUE\"},",
    "\"fallback\":{\"hold\":{\"at\":{\"beacon_anchor\":{\"safest\":{}}}}},",
    "\"kind\":\"PLAYBOOK\"}"
);

/// One refusal: the text, the pointer and words it is refused with now, and
/// the pointer and words `main` refused it with.
struct Row {
    text: &'static str,
    pointer: &'static str,
    message: &'static str,
    main_pointer: &'static str,
    main_message: &'static str,
}

const fn row(
    text: &'static str,
    pointer: &'static str,
    message: &'static str,
    main_pointer: &'static str,
    main_message: &'static str,
) -> Row {
    Row {
        text,
        pointer,
        message,
        main_pointer,
        main_message,
    }
}

const ROWS: &[Row] = &[
    // A key that is not a string.
    row(
        "{1:2}",
        "/byte/1",
        grammar::EXPECTED_QUOTE,
        "/byte/1",
        "expected a string",
    ),
    // A lone `/` where a value goes: no comment opens there, so it is a byte
    // that starts no value.
    row(
        "{\"a\": / 1}",
        "/byte/6",
        grammar::EXPECTED_VALUE,
        "/byte/6",
        "a lone `/` is neither a comment nor a value",
    ),
    row(
        "{\"a\": x}",
        "/byte/6",
        grammar::EXPECTED_VALUE,
        "/byte/6",
        "expected a JSON value",
    ),
    row(
        "{\"a\": -}",
        "/byte/7",
        grammar::NUMBER_WITHOUT_DIGIT,
        "/byte/7",
        "a number needs at least one digit",
    ),
    // A misspelt literal is refused at the first byte that differs.
    row(
        "{\"a\": tru}",
        "/byte/9",
        "expected `true`",
        "/byte/6",
        "expected `true`",
    ),
    // A `\u` escape is refused at the first byte that is not a hex digit.
    row(
        "{\"a\": \"\\u00\"}",
        "/byte/11",
        grammar::NON_HEX_DIGIT,
        "/byte/9",
        "`\\u` needs four hex digits",
    ),
    row(
        "{\"a\": \"\\u00zz\"}",
        "/byte/11",
        grammar::NON_HEX_DIGIT,
        "/byte/9",
        "`\\u` needs four hex digits",
    ),
    // RFC 8259 has no leading zero; `main`'s JSONC reader read `01` as a
    // number and wrote it back out, so `patch_plan` answered text that is not
    // JSON. Now it is refused, as the codec always refused it.
    row(
        "{\"a\": 01}",
        "/byte/7",
        grammar::LEADING_ZERO,
        "",
        "accepted",
    ),
    row(
        "{\"a\": 1.}",
        "/byte/8",
        grammar::FRACTION_WITHOUT_DIGIT,
        "/byte/8",
        "a number needs at least one digit after the point",
    ),
    row(
        "{\"a\": 1e}",
        "/byte/8",
        grammar::EXPONENT_WITHOUT_DIGIT,
        "/byte/8",
        "a number needs at least one digit in its exponent",
    ),
    row(
        "{\"a\": \"x",
        "/byte/8",
        grammar::UNTERMINATED_STRING,
        "/byte/8",
        "a string was never closed",
    ),
    row(
        "{\"a\": \"\u{1}\"}",
        "/byte/7",
        grammar::RAW_CONTROL,
        "/byte/7",
        "a raw control character is not allowed in a string",
    ),
    // An escape that is wrong as a whole is refused at its backslash (fix
    // pass 2, item D5: it pointed past the escape).
    row(
        "{\"a\": \"\\x\"}",
        "/byte/7",
        "unknown escape `\\x`",
        "/byte/9",
        "unknown escape sequence",
    ),
    row(
        "{\"a\": \"\\",
        "/byte/8",
        grammar::ENDED_IN_ESCAPE,
        "/byte/8",
        "an escape ran off the end of the document",
    ),
    row(
        "{\"a\": \"\\ud800\"}",
        "/byte/13",
        grammar::EXPECTED_BACKSLASH,
        "/byte/13",
        "a high surrogate needs a low surrogate after it",
    ),
    row(
        "{\"a\": \"\\ud800\\u0041\"}",
        "/byte/13",
        grammar::LONE_HIGH_SURROGATE,
        "/byte/19",
        "a high surrogate needs a low surrogate after it",
    ),
    row(
        "{\"a\": \"\\udc00\"}",
        "/byte/7",
        grammar::NOT_A_SCALAR,
        "/byte/13",
        "that escape is not a character",
    ),
    row(
        "{\"a\": 1,}",
        "/byte/8",
        grammar::TRAILING_COMMA,
        "/byte/8",
        "a trailing comma is not canonical JSON",
    ),
    row(
        "[1,]",
        "/byte/3",
        grammar::TRAILING_COMMA,
        "/byte/3",
        "a trailing comma is not canonical JSON",
    ),
    row(
        "{\"a\": 1 \"b\": 2}",
        "/byte/8",
        grammar::EXPECTED_COMMA_OR_BRACE,
        "/byte/8",
        "expected `,` or `}`",
    ),
    row(
        "[1 2]",
        "/byte/3",
        grammar::EXPECTED_COMMA_OR_BRACKET,
        "/byte/3",
        "expected `,` or `]`",
    ),
    row(
        "{\"a\" 1}",
        "/byte/5",
        grammar::EXPECTED_COLON,
        "/byte/5",
        "expected `:` after a key",
    ),
    row(
        "{\"a\": 1, \"a\": 2}",
        "/byte/9",
        "duplicate key `a`",
        "/byte/9",
        "`a` is written twice in the same object",
    ),
    row(
        "{\"a\": 1} x",
        "/byte/9",
        grammar::TRAILING_CONTENT,
        "/byte/9",
        "trailing content after the JSON value",
    ),
    row(
        "{\"a\": ",
        "/byte/6",
        grammar::ENDED_BEFORE_VALUE,
        "/byte/6",
        "the document ended where a value was expected",
    ),
    row(
        "",
        "/byte/0",
        grammar::ENDED_BEFORE_VALUE,
        "/byte/0",
        "the document ended where a value was expected",
    ),
    // A `/*` with no `*/` after it is not a comment (the PR's stated change):
    // the reader refuses the `/` where it stands, as the codec does.
    row(
        "{\"a\": 1 /* never",
        "/byte/8",
        grammar::EXPECTED_COMMA_OR_BRACE,
        "/byte/16",
        "a block comment was never closed",
    ),
];

#[test]
fn every_refusal_is_the_grammars_in_its_words_and_at_its_byte() {
    let mut wrong = Vec::new();
    for row in ROWS {
        let answer = Document::parse(row.text).map(|_| ());
        let expected = Err((row.pointer.to_owned(), row.message.to_owned()));
        let got = answer.map_err(|error| (error.pointer().to_owned(), error.message().to_owned()));
        if got != expected {
            wrong.push(format!(
                "{:?}: {got:?}, expected {:?} {:?} (main: {} {:?})",
                row.text, row.pointer, row.message, row.main_pointer, row.main_message
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// `patch_plan` on a file holding a leading zero refuses, where `main`
/// patched it and answered text that is not JSON.
#[test]
fn a_file_with_a_leading_zero_is_refused_by_patch_and_canonicalise() {
    let text = "{\"a\": 01, \"b\": 2}";
    let patched = patch_text(text, r#"[{"op":"replace","path":"/b","value":3}]"#);
    let error = patched.expect_err("a leading zero is not JSON");
    assert_eq!(
        (error.pointer(), error.message()),
        ("/byte/7", grammar::LEADING_ZERO)
    );
    let error = canonicalise_text(text).expect_err("a leading zero is not JSON");
    assert_eq!(error.message(), grammar::LEADING_ZERO);
}

/// `/*/ … */` is one block comment, closed by the first `*/` that starts after
/// its `/*`. `main`'s reader split it into `/*/` and dropped the tail, so the
/// canonical text did not parse.
#[test]
fn a_block_comment_opening_slash_star_slash_is_one_comment() {
    let text = format!("/*/ x */ {MINIMAL}");
    let text = text.as_str();
    let document = Document::parse(text).expect("one comment, then a value");
    let comments = document.comments();
    assert_eq!(comments.len(), 1, "{comments:?}");
    let comment = comments.first().expect("one comment");
    assert_eq!(comment.text, "/*/ x */");
    assert_eq!(comment.kind, CommentKind::Block);
    let canonical = canonicalise_text(text).expect("a canonical form");
    assert!(canonical.text.contains("/*/ x */"), "{}", canonical.text);
    assert!(
        Document::parse(&canonical.text).is_ok(),
        "{}",
        canonical.text
    );
}
