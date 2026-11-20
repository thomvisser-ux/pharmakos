// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The work of reading a player's text is linear in its bytes (S3's `vfyq2`,
//! fix pass 1, item 1).
//!
//! A playbook arrives over a socket, so every scanner that reads one is a
//! parser of untrusted input, and its work must be bounded by a constant per
//! byte: a scan that starts again from some early byte for every token is
//! quadratic, and a 256 KiB frame then costs billions of byte reads (the
//! security review found the blanker doing it on unclosed block comments).
//!
//! Every scanner along the paths below adds the bytes it examines to the
//! work meter (`pharmakos_proto::json::work`): the grammar's tokens and
//! whitespace, the comment scanner, the blanker, the comment index and the
//! canonical writers. Each test reads the meter around one call, over each
//! shape at two sizes that differ by [`SCALE`] times, and holds the work per
//! input byte under [`WORK_PER_BYTE`] at both: a quadratic scan's work per
//! byte grows with the size and fails at the larger one, in milliseconds.
//!
//! The shapes are the security review's 22, made to measure, and the
//! comment and whitespace runs that stress the comment scanner and the
//! grammar's whitespace.

#![cfg(test)]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    reason = "a test: a panic is its failure report, and its arithmetic is on small counts it \
              made itself"
)]

use std::fmt::Write as _;

use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::json::work;

use super::{hand_over, verify_jsonc};
use crate::canonical::canonicalise_text;
use crate::jsonc::Document;

/// How much larger the second size is than the first.
const SCALE: usize = 16;

/// The most bytes any path may examine per byte of its input. Measured at
/// most 13 on these shapes; a scan that rereads its input once per token
/// reaches hundreds per byte at the larger size.
const WORK_PER_BYTE: u64 = 32;

/// A shape at about `size` bytes.
type Shape = fn(usize) -> String;

/// A playbook the shapes that need one are written into.
const MINIMAL: &str = concat!(
    "{\"schema_version\":{\"major\":1},",
    "\"meta\":{\"title\":\"t\",\"author_kind\":\"HUMAN\"},",
    "\"declarative\":{\"route\":[{\"label\":\"a\",\"hold\":{\"ms\":1000}}]},",
    "\"on_death\":{\"on_respawn\":\"CONTINUE\"},",
    "\"fallback\":{\"hold\":{\"at\":{\"beacon_anchor\":{\"safest\":{}}}}},",
    "\"kind\":\"PLAYBOOK\"}"
);

fn repeat_to(piece: &str, size: usize) -> String {
    piece.repeat((size / piece.len()).max(1))
}

fn object_of_keys(size: usize, key: impl Fn(usize) -> String) -> String {
    let mut out = String::from("{");
    let mut index = 0;
    while out.len() < size {
        if index > 0 {
            out.push(',');
        }
        let _ = write!(out, "\"{}\":0", key(index));
        index += 1;
    }
    out.push('}');
    out
}

/// The shapes, by name.
fn shapes() -> Vec<(&'static str, Shape)> {
    vec![
        ("distinct short keys", |n| {
            object_of_keys(n, |i| format!("{i:x}"))
        }),
        ("keys with a long common prefix", |n| {
            object_of_keys(n, |i| format!("{}{i:x}", "p".repeat(256)))
        }),
        ("sixteen long keys", |n| {
            let width = (n / 16).max(1);
            object_of_keys(n, |i| format!("{}{i:x}", "q".repeat(width)))
        }),
        ("one key written twice", |n| {
            let key = "z".repeat(n / 2);
            format!("{{\"{key}\":0,\"{key}\":1}}")
        }),
        ("a string of escapes", |n| {
            format!("\"{}\"", repeat_to("\\u0041", n))
        }),
        ("a string of surrogate pairs", |n| {
            format!("\"{}\"", repeat_to("\\ud83d\\ude00", n))
        }),
        ("a lone surrogate after a run", |n| {
            format!("\"{}\\ud800\"", "a".repeat(n))
        }),
        ("an escaped NUL after a run", |n| {
            format!("\"{}\\u0000\"", "a".repeat(n))
        }),
        ("a long number", |n| "9".repeat(n)),
        ("minus zero after whitespace", |n| {
            format!("{}-0", " ".repeat(n))
        }),
        ("a huge exponent", |n| format!("1e{}", "9".repeat(n))),
        ("a leading zero after whitespace", |n| {
            format!("{}012", " ".repeat(n))
        }),
        ("a plus after whitespace", |n| {
            format!("{}+1", " ".repeat(n))
        }),
        ("a hex number after whitespace", |n| {
            format!("{}0x10", " ".repeat(n))
        }),
        ("whitespace", |n| format!("{}1", " ".repeat(n))),
        ("empty arrays", |n| format!("[{}[]]", repeat_to("[],", n))),
        ("ones", |n| format!("[{}1]", repeat_to("1,", n))),
        ("empty objects", |n| {
            format!("[{}{{}}]", repeat_to("{},", n))
        }),
        ("deep then wide", |n| {
            format!(
                "{}{}1{}",
                "[".repeat(63),
                repeat_to("1,", n),
                "]".repeat(63)
            )
        }),
        ("a trailing comma after a run", |n| {
            format!("[{}]", repeat_to("1,", n))
        }),
        ("a BOM after whitespace", |n| {
            format!("{}\u{feff}{{}}", " ".repeat(n))
        }),
        ("a raw NUL in a string", |n| {
            format!("\"{}\0\"", "a".repeat(n))
        }),
        ("unclosed block comments", |n| repeat_to("/* ", n)),
        ("unclosed block comments after a playbook", |n| {
            format!("{MINIMAL}\n{}", repeat_to("/* ", n))
        }),
        ("line comments", |n| repeat_to("//", n)),
        ("closed block comments", |n| {
            format!("{}{MINIMAL}", repeat_to("/**/", n))
        }),
        ("line comments between members", |n| {
            format!("{}{MINIMAL}", repeat_to("// a note\n", n))
        }),
        ("stray slashes", |n| {
            format!("{MINIMAL}{}", repeat_to(" /", n))
        }),
        ("whitespace runs between members", |n| {
            MINIMAL.replace(',', &format!(",{}", " ".repeat(n / 8)))
        }),
        ("a commented route", |n| {
            let steps: Vec<String> = (0..(n / 48).max(1))
                .map(|i| format!("// step {i}\n{{\"label\":\"s{i}\",\"hold\":{{\"ms\":1000}}}}"))
                .collect();
            MINIMAL.replace(
                "[{\"label\":\"a\",\"hold\":{\"ms\":1000}}]",
                &format!("[{}]", steps.join(",")),
            )
        }),
    ]
}

/// The work one call does on `text`, per byte of it.
fn per_byte(text: &str, call: &dyn Fn(&str)) -> u64 {
    let _ = work::take();
    call(text);
    work::take() / u64::try_from(text.len().max(1)).expect("a length")
}

fn holds(path: &str, call: &dyn Fn(&str)) {
    let base: usize = 512;
    for (name, shape) in shapes() {
        for size in [base, base * SCALE] {
            let text = shape(size);
            let work = per_byte(&text, call);
            assert!(
                work <= WORK_PER_BYTE,
                "{path} on {name} at {} bytes examined {work} bytes per byte, past {WORK_PER_BYTE}",
                text.len()
            );
        }
    }
}

fn rules() -> pharmakos_sim::rules::RulesTable {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/plan-core sits two levels below the workspace root");
    pharmakos_sim::rules::RulesTable::load(&root.join("rules").join("rules.v1.json"))
        .expect("the committed rules table")
}

#[test]
fn handing_a_text_over_is_linear_in_its_bytes() {
    holds("hand_over", &|text| {
        let _ = hand_over(text);
    });
}

#[test]
fn verifying_a_text_reads_it_in_linear_work() {
    let rules = rules();
    let scope = pharmakos_verifier::Scope::new(
        pharmakos_sim::tables::SeatId::new(0),
        pharmakos_sim::knowledge::SeatEconomy::default(),
    );
    holds("verify_jsonc", &|text| {
        let _ = verify_jsonc(text, &[], &scope, &rules, Depth::Quick);
    });
}

#[test]
fn canonicalising_a_text_is_linear_in_its_bytes() {
    holds("canonicalise_text", &|text| {
        let _ = canonicalise_text(text);
    });
}

#[test]
fn parsing_a_document_is_linear_in_its_bytes() {
    holds("Document::parse", &|text| {
        let _ = Document::parse(text);
    });
}
