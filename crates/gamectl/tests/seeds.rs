// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The seed file decoder's property test (S2's plan, task `night`: "a property
//! test holds that a crafted file only ever returns the typed error").
//!
//! The decoder reads a file a command line names, so it is untrusted input,
//! and the global rule for such a decoder holds: whatever the bytes, it
//! answers with seeds or with one of [`SeedFileError`]'s variants, and never
//! panics. The generator is the project's own seeded split streams
//! (`pharmakos_sim::math::random::StreamRng`, as the verifier's fuzz target
//! uses), so a failing case is reproducible from its index alone, and it
//! writes *shaped* files — seeds, near-seeds, comments, blank lines, carriage
//! returns, bytes that are not UTF-8, overlong lines, repeats — because
//! uniform noise would meet one refusal almost every time.
//!
//! Beyond "no panic", every answer is checked against what the file says:
//! accepted seeds are distinct, at most the cap, and listed on increasing
//! lines that exist in the file; a refusal's line exists in the file.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use pharmakos_gamectl::scenario::seeds::{self, MAX_LINE_BYTES, MAX_SEEDS, SeedFileError};
use pharmakos_sim::math::random::{Stream, StreamRng};

/// The generator's seed. Any value; fixed so a failure reproduces.
const PROPERTY_SEED: u64 = 0x5eed_f11e_0000_0001;

/// Cases per run: cheap, since each file is small.
const CASES: u32 = 4_000;

/// One crafted line. A `clean` file draws only lines the format accepts
/// (blank, a comment, a fresh seed with or without a trailing comment), so
/// accepted files are as common as refused ones.
fn line(rng: &mut StreamRng, pool: &[u64], clean: bool) -> Vec<u8> {
    let pick = |rng: &mut StreamRng, len: usize| -> usize {
        let top = i32::try_from(len).unwrap().saturating_sub(1);
        usize::try_from(rng.range_i32(0, top)).unwrap()
    };
    let draw = rng.range_i32(0, 11);
    let draw = if clean {
        *[0, 1, 4, 11]
            .get(usize::try_from(draw.rem_euclid(4)).unwrap())
            .unwrap()
    } else {
        draw
    };
    match draw {
        0 => Vec::new(),
        1 => b"# a comment".to_vec(),
        2 | 3 => seeds::spell(*pool.get(pick(rng, pool.len())).unwrap()).into_bytes(),
        4 => format!("  {}  # trailing", seeds::spell(rng.next_u64())).into_bytes(),
        5 => seeds::spell(rng.next_u64()).to_uppercase().into_bytes(),
        6 => {
            // A seed one digit short or long.
            let mut text = seeds::spell(rng.next_u64());
            if rng.range_i32(0, 1) == 0 {
                text.pop();
            } else {
                text.push('7');
            }
            text.into_bytes()
        }
        7 => format!("{}\r", seeds::spell(rng.next_u64())).into_bytes(),
        8 => vec![0xff, 0xfe, b'0', b'x'],
        9 => vec![b'#'; MAX_LINE_BYTES.saturating_add(1)],
        10 => {
            // Bytes from a small alphabet the format uses, in any order.
            let alphabet = b"0x#a f9\t";
            let len = pick(rng, 40);
            (0..len)
                .map(|_| *alphabet.get(pick(rng, alphabet.len())).unwrap())
                .collect()
        }
        _ => seeds::spell(rng.next_u64()).into_bytes(),
    }
}

/// One crafted file: up to sixty lines, each joined by LF.
fn file(index: u32) -> Vec<u8> {
    let mut rng = StreamRng::new(PROPERTY_SEED, Stream::Map, index, 0, 0);
    let pool: Vec<u64> = (0..4).map(|_| rng.next_u64()).collect();
    let clean = rng.range_i32(0, 1) == 0;
    let lines = rng.range_i32(0, 60);
    let mut bytes = Vec::new();
    for at in 0..lines {
        if at > 0 {
            bytes.push(b'\n');
        }
        bytes.extend(line(&mut rng, &pool, clean));
    }
    if rng.range_i32(0, 1) == 0 {
        bytes.push(b'\n');
    }
    bytes
}

/// How many lines the decoder can number in `bytes`.
fn lines_in(bytes: &[u8]) -> usize {
    bytes.split(|byte| *byte == b'\n').count()
}

#[test]
fn a_crafted_file_only_ever_returns_seeds_or_the_typed_error() {
    let mut accepted = 0_u32;
    let mut refused = 0_u32;
    for index in 0..CASES {
        let bytes = file(index);
        let lines = lines_in(&bytes);
        match seeds::decode(&bytes) {
            Ok(listed) => {
                accepted = accepted.saturating_add(1);
                assert!(!listed.is_empty(), "case {index}: an empty set is refused");
                assert!(listed.len() <= MAX_SEEDS, "case {index}");
                for (first, second) in listed.iter().zip(listed.iter().skip(1)) {
                    assert!(first.line < second.line, "case {index}: lines in order");
                }
                for (at, item) in listed.iter().enumerate() {
                    assert!(item.line >= 1 && item.line <= lines, "case {index}");
                    assert!(
                        listed
                            .iter()
                            .skip(at.saturating_add(1))
                            .all(|other| other.seed != item.seed),
                        "case {index}: seed {} listed twice",
                        seeds::spell(item.seed)
                    );
                }
            }
            Err(error) => {
                refused = refused.saturating_add(1);
                let line = match error {
                    SeedFileError::CarriageReturn { line }
                    | SeedFileError::LineTooLong { line }
                    | SeedFileError::Malformed { line }
                    | SeedFileError::TooManySeeds { line } => Some(line),
                    SeedFileError::Duplicate { line, first } => {
                        assert!(first < line, "case {index}: {error}");
                        Some(line)
                    }
                    SeedFileError::TooLarge | SeedFileError::NotUtf8 | SeedFileError::Empty => None,
                };
                if let Some(line) = line {
                    assert!(line >= 1 && line <= lines, "case {index}: {error}");
                }
                assert!(!error.to_string().is_empty(), "case {index}");
            }
        }
    }
    // Both halves were exercised, or the property is being asserted over
    // nothing.
    assert!(
        accepted > CASES.checked_div(20).unwrap(),
        "only {accepted} files were accepted"
    );
    assert!(
        refused > CASES.checked_div(20).unwrap(),
        "only {refused} files were refused"
    );
}

#[test]
fn a_file_past_the_caps_is_refused_and_not_read_in_full() {
    let big = vec![b'#'; seeds::MAX_FILE_BYTES.saturating_add(1)];
    assert_eq!(seeds::decode(&big), Err(SeedFileError::TooLarge));
    let mut many = String::new();
    for n in 0..=MAX_SEEDS {
        many.push_str(&seeds::spell(u64::try_from(n).unwrap()));
        many.push('\n');
    }
    assert!(
        many.len() <= seeds::MAX_FILE_BYTES,
        "the count cap is met first"
    );
    assert!(matches!(
        seeds::decode(many.as_bytes()),
        Err(SeedFileError::TooManySeeds { .. })
    ));
}

#[test]
fn the_committed_seed_sets_decode() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf();
    let nightly = seeds::decode(&std::fs::read(root.join("scenarios/nightly/seeds.txt")).unwrap())
        .expect("the nightly set decodes");
    assert_eq!(
        nightly.len(),
        20,
        "decisions-log item 144 (6): about twenty seeds"
    );
    let two = seeds::decode(
        &std::fs::read(root.join("scenarios/nightly/fixture/two.seeds.txt")).unwrap(),
    )
    .expect("the fixture's pair decodes");
    let firsts: Vec<u64> = nightly.iter().take(2).map(|item| item.seed).collect();
    let pair: Vec<u64> = two.iter().map(|item| item.seed).collect();
    assert_eq!(
        pair, firsts,
        "the fixture's two are the nightly set's first two"
    );
}
