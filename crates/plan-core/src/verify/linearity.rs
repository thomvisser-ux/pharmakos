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

/// The bytes one call examines on `text`.
fn work_of(text: &str, call: &dyn Fn(&str)) -> u64 {
    work::take().expect("the meter counts");
    call(text);
    u64::try_from(work::take().expect("the meter counts")).expect("a count")
}

/// The work one call does on `text`, per byte of it.
fn per_byte(text: &str, call: &dyn Fn(&str)) -> u64 {
    work_of(text, call) / u64::try_from(text.len().max(1)).expect("a length")
}

/// The sizes each shape is measured at: 512 B to 512 KiB (fix pass 2, item
/// D1), the last at the playbook's cap.
const SIZES: [usize; 4] = [512, 8 * 1024, 32 * 1024, 512 * 1024];

/// Holds `call` to its work on every shape at every size (fix pass 2, item
/// D1): at least one byte examined per byte of input, so a scanner that reads
/// without counting fails; at most [`WORK_PER_BYTE`]; and from each size to
/// the next, the work grows at most 1.25 times as fast as the input. A text
/// `refused_unread` holds (past the cap, for `verify_jsonc`) must show no
/// work at all, since it is refused before anything reads it.
fn holds(path: &str, call: &dyn Fn(&str), refused_unread: &dyn Fn(&str) -> bool) {
    for (name, shape) in shapes() {
        let mut previous: Option<(u64, u64)> = None;
        for size in SIZES {
            let text = shape(size);
            let length = u64::try_from(text.len()).expect("a length");
            let work = work_of(&text, call);
            if refused_unread(&text) {
                assert_eq!(
                    work, 0,
                    "{path} on {name} at {length} bytes read a refused text"
                );
                previous = None;
                continue;
            }
            // Every path reads by one grammar, so each reads at least as far
            // as the JSONC reader does: the whole text, or up to the byte it
            // refuses the text at.
            let read = Document::parse(&text)
                .err()
                .and_then(|refused| {
                    refused
                        .pointer
                        .strip_prefix("/byte/")
                        .and_then(|at| at.parse::<u64>().ok())
                })
                .map_or(length, |at| at.min(length));
            assert!(
                work >= read,
                "{path} on {name} at {length} bytes examined {work} bytes, fewer than the \
                 {read} it read"
            );
            assert!(
                work <= WORK_PER_BYTE * length,
                "{path} on {name} at {length} bytes examined {} bytes per byte, past \
                 {WORK_PER_BYTE}",
                work / length
            );
            if let Some((was_length, was_work)) = previous {
                // work / was_work <= 1.25 * length / was_length, in integers.
                assert!(
                    4 * work * was_length <= 5 * was_work * length,
                    "{path} on {name}: from {was_length} to {length} bytes the work grew from \
                     {was_work} to {work}, faster than the input"
                );
            }
            previous = Some((length, work));
        }
    }
}

/// No text is refused unread.
fn reads_all(_: &str) -> bool {
    false
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
    holds(
        "hand_over",
        &|text| {
            let _ = hand_over(text);
        },
        &reads_all,
    );
}

#[test]
fn verifying_a_text_reads_it_in_linear_work() {
    let rules = rules();
    let scope = pharmakos_verifier::Scope::new(
        pharmakos_sim::tables::SeatId::new(0),
        pharmakos_sim::knowledge::SeatEconomy::default(),
    );
    holds(
        "verify_jsonc",
        &|text| {
            let _ = verify_jsonc(text, &[], &scope, &rules, Depth::Quick);
        },
        &pharmakos_verifier::is_too_long,
    );
}

#[test]
fn canonicalising_a_text_is_linear_in_its_bytes() {
    holds(
        "canonicalise_text",
        &|text| {
            let _ = canonicalise_text(text);
        },
        &reads_all,
    );
}

#[test]
fn parsing_a_document_is_linear_in_its_bytes() {
    holds(
        "Document::parse",
        &|text| {
            let _ = Document::parse(text);
        },
        &reads_all,
    );
}

/// Pointer-heavy patches at about `size` bytes (S3's `vfyq2`, fix pass 2,
/// item A2): one pointer of as many tokens as a playbook can be deep, each
/// token long; and one of more tokens than that, which is refused.
fn pointers(size: usize) -> [(&'static str, String); 2] {
    let tokens = crate::error::MAX_POINTER_TOKENS;
    [
        (
            "long tokens",
            format!("/{}", "k".repeat(size / tokens)).repeat(tokens),
        ),
        ("too many tokens", "/k".repeat(size / 2)),
    ]
}

fn pointer_patches(size: usize) -> Vec<(&'static str, String)> {
    let quote = |pointer: &str| {
        pharmakos_proto::json::write(&pharmakos_proto::json::Json::String(pointer.to_owned()))
    };
    let mut found = Vec::new();
    for (name, pointer) in pointers(size) {
        let quoted = quote(&pointer);
        found.push((
            name,
            format!(r#"[{{"op":"test","path":{quoted},"value":1}}]"#),
        ));
        found.push((name, format!(r#"[{{"op":"remove","path":{quoted}}}]"#)));
        found.push((
            name,
            format!(r#"[{{"op":"copy","from":{quoted},"path":"/b"}}]"#),
        ));
    }
    found
}

#[test]
fn patching_by_a_long_pointer_is_linear_in_its_bytes() {
    for size in [512, 512 * SCALE] {
        for (name, patch) in pointer_patches(size) {
            let work = per_byte(&patch, &|patch| {
                let _ = crate::patch::patch_text(MINIMAL, patch);
            });
            assert!(
                work <= WORK_PER_BYTE,
                "patch_text by {name} at {} bytes examined {work} bytes per byte",
                patch.len()
            );
        }
        for (name, pointer) in pointers(size) {
            work::take().expect("the meter counts");
            let _ = crate::library::instantiate(
                MINIMAL,
                &[crate::library::Parameter {
                    name: pointer.clone(),
                    value: "1".to_owned(),
                }],
                &[],
            );
            let input = u64::try_from(pointer.len() + MINIMAL.len()).expect("a length");
            let per =
                u64::try_from(work::take().expect("the meter counts")).expect("a count") / input;
            assert!(
                per <= WORK_PER_BYTE,
                "instantiate by {name} at {input} bytes examined {per} bytes per byte"
            );
        }
    }
}
