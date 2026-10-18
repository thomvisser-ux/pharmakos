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
//! writes *shaped* files: seeds, near-seeds, comments, blank lines, carriage
//! returns, bytes that are not UTF-8, overlong lines and repeats, because
//! uniform noise would meet one refusal almost every time.
//!
//! **It has an oracle.** Every generated line carries the class it was
//! generated as ([`Line`]), and [`expected`] computes from the classes alone,
//! never from the text, what the decoder must answer: the seeds in order, or
//! the one refusal the format's order of checks gives (the file's encoding,
//! then line by line its length, a carriage return, a malformed line, a
//! repeat, the count). Each case asserts the decoder's answer equals it.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use pharmakos_gamectl::scenario::seeds::{self, Listed, MAX_LINE_BYTES, MAX_SEEDS, SeedFileError};
use pharmakos_sim::math::random::{Stream, StreamRng};

/// The generator's seed. Any value; fixed so a failure reproduces.
const PROPERTY_SEED: u64 = 0x5eed_f11e_0000_0001;

/// Cases per run: cheap, since each file is small.
const CASES: u32 = 4_000;

/// What a generated line is, which is all the oracle reads.
#[derive(Clone, Copy, Debug)]
enum Line {
    /// Nothing, or spaces: skipped.
    Blank,
    /// A `#` comment on its own: skipped.
    Comment,
    /// This seed, spelled as the format says, with or without spaces and a
    /// trailing comment.
    Seed(u64),
    /// Not a seed, not blank and not a comment: refused as malformed.
    Malformed,
    /// A seed followed by a carriage return.
    CarriageReturn,
    /// Bytes that are not UTF-8, which refuse the whole file.
    NotUtf8,
    /// A comment longer than the line cap.
    TooLong,
}

fn pick(rng: &mut StreamRng, len: usize) -> usize {
    let top = i32::try_from(len).unwrap().saturating_sub(1);
    usize::try_from(rng.range_i32(0, top)).unwrap()
}

/// One crafted line and its class. A `clean` file draws only lines the format
/// accepts, so accepted files are as common as refused ones.
fn line(rng: &mut StreamRng, pool: &[u64], clean: bool) -> (Vec<u8>, Line) {
    let draw = rng.range_i32(0, 11);
    let draw = if clean {
        *[0, 1, 4, 11]
            .get(usize::try_from(draw.rem_euclid(4)).unwrap())
            .unwrap()
    } else {
        draw
    };
    match draw {
        0 => (b"   ".to_vec(), Line::Blank),
        1 => (b"# a comment".to_vec(), Line::Comment),
        2 | 3 => {
            let seed = *pool.get(pick(rng, pool.len())).unwrap();
            (seeds::spell(seed).into_bytes(), Line::Seed(seed))
        }
        4 => {
            let seed = rng.next_u64();
            (
                format!("  {}  # trailing", seeds::spell(seed)).into_bytes(),
                Line::Seed(seed),
            )
        }
        5 => (
            seeds::spell(rng.next_u64()).to_uppercase().into_bytes(),
            Line::Malformed,
        ),
        6 => {
            // A seed one digit short or long.
            let mut text = seeds::spell(rng.next_u64());
            if rng.range_i32(0, 1) == 0 {
                text.pop();
            } else {
                text.push('7');
            }
            (text.into_bytes(), Line::Malformed)
        }
        7 => (
            format!("{}\r", seeds::spell(rng.next_u64())).into_bytes(),
            Line::CarriageReturn,
        ),
        8 => (vec![0xff, 0xfe, b'0', b'x'], Line::NotUtf8),
        9 => (vec![b'#'; MAX_LINE_BYTES.saturating_add(1)], Line::TooLong),
        10 => {
            // `zz` and then bytes from the format's own alphabet, `#` left
            // out: never blank, never a comment, never a seed.
            let alphabet = b"0xaf9 ";
            let len = pick(rng, 20);
            let mut text = b"zz".to_vec();
            text.extend((0..len).map(|_| *alphabet.get(pick(rng, alphabet.len())).unwrap()));
            (text, Line::Malformed)
        }
        _ => {
            let seed = rng.next_u64();
            (seeds::spell(seed).into_bytes(), Line::Seed(seed))
        }
    }
}

/// One crafted file, up to sixty lines joined by LF, and the class of every
/// line the decoder will number (a trailing LF makes a last, blank line).
fn file(index: u32) -> (Vec<u8>, Vec<Line>) {
    let mut rng = StreamRng::new(PROPERTY_SEED, Stream::Map, index, 0, 0);
    let pool: Vec<u64> = (0..4).map(|_| rng.next_u64()).collect();
    let clean = rng.range_i32(0, 1) == 0;
    let lines = rng.range_i32(0, 60);
    let mut bytes = Vec::new();
    let mut classes = Vec::new();
    for at in 0..lines {
        if at > 0 {
            bytes.push(b'\n');
        }
        let (text, class) = line(&mut rng, &pool, clean);
        bytes.extend(text);
        classes.push(class);
    }
    if lines == 0 {
        classes.push(Line::Blank);
    }
    if rng.range_i32(0, 1) == 0 {
        bytes.push(b'\n');
        classes.push(Line::Blank);
    }
    (bytes, classes)
}

/// What the decoder must answer for a file of these classes.
fn expected(classes: &[Line]) -> Result<Vec<Listed>, SeedFileError> {
    if classes.iter().any(|class| matches!(class, Line::NotUtf8)) {
        return Err(SeedFileError::NotUtf8);
    }
    let mut listed: Vec<Listed> = Vec::new();
    for (index, class) in classes.iter().enumerate() {
        let line = index.saturating_add(1);
        match *class {
            Line::TooLong => return Err(SeedFileError::LineTooLong { line }),
            Line::CarriageReturn => return Err(SeedFileError::CarriageReturn { line }),
            Line::Malformed => return Err(SeedFileError::Malformed { line }),
            // A line that is not UTF-8 refused the whole file above.
            Line::Blank | Line::Comment | Line::NotUtf8 => {}
            Line::Seed(seed) => {
                if let Some(first) = listed.iter().find(|item| item.seed == seed) {
                    return Err(SeedFileError::Duplicate {
                        line,
                        first: first.line,
                    });
                }
                if listed.len() >= MAX_SEEDS {
                    return Err(SeedFileError::TooManySeeds { line });
                }
                listed.push(Listed { seed, line });
            }
        }
    }
    if listed.is_empty() {
        return Err(SeedFileError::Empty);
    }
    Ok(listed)
}

#[test]
fn a_crafted_file_gets_exactly_the_answer_its_lines_call_for() {
    let mut accepted = 0_u32;
    let mut refused = 0_u32;
    for index in 0..CASES {
        let (bytes, classes) = file(index);
        let answer = seeds::decode(&bytes);
        assert_eq!(answer, expected(&classes), "case {index}: {classes:?}");
        match answer {
            Ok(_) => accepted = accepted.saturating_add(1),
            Err(error) => {
                refused = refused.saturating_add(1);
                assert!(!error.to_string().is_empty(), "case {index}");
            }
        }
    }
    // Both halves were exercised, or the property is being asserted over
    // nothing.
    let floor = CASES.checked_div(20).unwrap();
    assert!(accepted > floor, "only {accepted} files were accepted");
    assert!(refused > floor, "only {refused} files were refused");
}

#[test]
fn a_file_past_the_caps_is_refused() {
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
    assert_eq!(
        seeds::decode(many.as_bytes()),
        Err(SeedFileError::TooManySeeds {
            line: MAX_SEEDS.saturating_add(1)
        })
    );
}

/// The balance check's twenty (`tests/balance.rs`' `seeds`), restated: the
/// determinism harness's seed, the scenario files' seed, then n times the
/// 64-bit golden ratio for n = 1 to 18. The nightly set is these, in this
/// order, so the nightly and the balance report play the same maps (S2's
/// plan, task `arms`).
fn balance_seeds() -> Vec<u64> {
    [pharmakos_sim::DETERMINISM_MATCH_SEED, 0x0000_0000_ca5c_aded]
        .into_iter()
        .chain((1..=18_u64).map(|n| n.wrapping_mul(0x9e37_79b9_7f4a_7c15)))
        .collect()
}

#[test]
fn the_committed_seed_sets_are_the_balance_checks() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf();
    let read = |relative: &str| -> Vec<u64> {
        seeds::decode(&std::fs::read(root.join(relative)).unwrap())
            .unwrap_or_else(|error| panic!("{relative}: {error}"))
            .iter()
            .map(|item| item.seed)
            .collect()
    };
    let nightly = read("scenarios/nightly/seeds.txt");
    assert_eq!(
        nightly,
        balance_seeds(),
        "the nightly set is the balance check's twenty"
    );
    let two = read("scenarios/nightly/fixture/two.seeds.txt");
    assert_eq!(
        two,
        nightly.iter().take(2).copied().collect::<Vec<u64>>(),
        "the fixture's pair is the nightly set's first two"
    );
}
