// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The seed file `gamectl scenario run --seeds <file>` reads (S2's plan, task
//! `night`; register S2-01).
//!
//! # The format
//!
//! One seed per line, each `0x` followed by **exactly sixteen lowercase hex
//! digits**, the spelling a scenario's own `map.seed` takes
//! (`scenarios/README.md`), so a seed can be copied between the two files
//! unchanged and no reader has to decide what `0x1` means. A `#` starts a
//! comment that runs to the end of its line, on a line of its own or after a
//! seed; spaces around a seed and blank lines are skipped. Order matters: a
//! seed's position in the file decides which seat the arm plays
//! ([`super::nightly`]), so the file is read in order and never sorted.
//!
//! # Untrusted input, so every length and count is capped
//!
//! A seed file is a file somebody names on a command line, and a workflow's
//! dispatch input can name any path in the checkout. The global rule for an
//! input decoder holds: the file's size, a line's length and the number of
//! seeds are each capped before anything is allocated for them, a duplicate
//! seed and a malformed line are refused with their line number, and nothing
//! else ever comes back — `tests/seeds.rs`' property test feeds the decoder
//! crafted files and holds that only a [`SeedFileError`] escapes.
//!
//! The caps are bounds, not tuning: the nightly set is about twenty seeds
//! (decisions-log item 144 (6)), and each cap sits far enough above what the
//! set will ever need that no honest file meets it.

use std::fmt;

/// The largest seed file read, in bytes: 64 KiB, about two thousand
/// commented lines.
pub const MAX_FILE_BYTES: usize = 64 * 1024;

/// The longest line read, in bytes, comment included.
pub const MAX_LINE_BYTES: usize = 256;

/// The most seeds one file may list. Each seed is a whole match, so a file at
/// the cap is already days of runner time.
pub const MAX_SEEDS: usize = 1024;

/// Hex digits in a seed, after its `0x`.
const SEED_DIGITS: usize = 16;

/// Why a seed file was refused. Line numbers count from 1.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SeedFileError {
    /// The file is larger than [`MAX_FILE_BYTES`].
    TooLarge,
    /// The file is not UTF-8.
    NotUtf8,
    /// A line holds a carriage return: a seed file is LF-only, as every file
    /// a run reads is (`.gitattributes`).
    CarriageReturn {
        /// The line.
        line: usize,
    },
    /// A line is longer than [`MAX_LINE_BYTES`].
    LineTooLong {
        /// The line.
        line: usize,
    },
    /// A line is neither blank, a comment, nor one seed spelled as the format
    /// says.
    Malformed {
        /// The line.
        line: usize,
    },
    /// A seed already listed on an earlier line.
    Duplicate {
        /// The line that repeats it.
        line: usize,
        /// The line that listed it first.
        first: usize,
    },
    /// More than [`MAX_SEEDS`] seeds.
    TooManySeeds {
        /// The line of the seed past the cap.
        line: usize,
    },
    /// No seed at all: a run over an empty set plays nothing and would report
    /// a win rate over zero seeds.
    Empty,
}

impl fmt::Display for SeedFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SeedFileError::TooLarge => write!(
                f,
                "the file is larger than {MAX_FILE_BYTES} bytes, the most a seed file holds"
            ),
            SeedFileError::NotUtf8 => f.write_str("the file is not UTF-8 text"),
            SeedFileError::CarriageReturn { line } => write!(
                f,
                "line {line} holds a carriage return; a seed file has LF endings"
            ),
            SeedFileError::LineTooLong { line } => {
                write!(f, "line {line} is longer than {MAX_LINE_BYTES} bytes")
            }
            SeedFileError::Malformed { line } => write!(
                f,
                "line {line} is not a seed: each seed is `0x` and exactly {SEED_DIGITS} lowercase \
                 hex digits, and a `#` starts a comment"
            ),
            SeedFileError::Duplicate { line, first } => write!(
                f,
                "line {line} repeats the seed on line {first}; a seed is played once"
            ),
            SeedFileError::TooManySeeds { line } => write!(
                f,
                "line {line} is seed number {}, and a seed file lists at most {MAX_SEEDS}",
                MAX_SEEDS.saturating_add(1)
            ),
            SeedFileError::Empty => f.write_str("the file lists no seed"),
        }
    }
}

impl std::error::Error for SeedFileError {}

/// One seed, and the line it came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Listed {
    /// The 64-bit map seed.
    pub seed: u64,
    /// Its line, from 1.
    pub line: usize,
}

/// Decode a seed file's bytes, in file order.
///
/// # Errors
///
/// A [`SeedFileError`] for the first thing wrong with the file, and nothing
/// else: no input makes this panic.
pub fn decode(bytes: &[u8]) -> Result<Vec<Listed>, SeedFileError> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(SeedFileError::TooLarge);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| SeedFileError::NotUtf8)?;
    let mut listed: Vec<Listed> = Vec::new();
    for (index, raw) in text.split('\n').enumerate() {
        let line = index.saturating_add(1);
        if raw.len() > MAX_LINE_BYTES {
            return Err(SeedFileError::LineTooLong { line });
        }
        if raw.contains('\r') {
            return Err(SeedFileError::CarriageReturn { line });
        }
        let content = match raw.split_once('#') {
            Some((before, _comment)) => before,
            None => raw,
        }
        .trim_matches(' ');
        if content.is_empty() {
            continue;
        }
        let seed = parse(content).ok_or(SeedFileError::Malformed { line })?;
        if let Some(earlier) = listed.iter().find(|item| item.seed == seed) {
            return Err(SeedFileError::Duplicate {
                line,
                first: earlier.line,
            });
        }
        if listed.len() >= MAX_SEEDS {
            return Err(SeedFileError::TooManySeeds { line });
        }
        listed.push(Listed { seed, line });
    }
    if listed.is_empty() {
        return Err(SeedFileError::Empty);
    }
    Ok(listed)
}

/// One seed as the format spells it, or `None`.
fn parse(text: &str) -> Option<u64> {
    let digits = text.strip_prefix("0x")?;
    if digits.len() != SEED_DIGITS
        || !digits
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    u64::from_str_radix(digits, 16).ok()
}

/// A seed as the format spells it.
#[must_use]
pub fn spell(seed: u64) -> String {
    format!("0x{seed:016x}")
}

#[cfg(test)]
mod tests {
    use super::{Listed, MAX_SEEDS, SeedFileError, decode, spell};

    #[test]
    fn seeds_comments_and_blank_lines() {
        let text = "# the set\n\n0x00000000ca5caded\n  0x0000000000000001  # one\n";
        assert_eq!(
            decode(text.as_bytes()),
            Ok(vec![
                Listed {
                    seed: 0xca5c_aded,
                    line: 3
                },
                Listed { seed: 1, line: 4 },
            ])
        );
        assert_eq!(spell(0xca5c_aded), "0x00000000ca5caded");
    }

    #[test]
    fn every_refusal_names_its_line() {
        for (text, error) in [
            ("0x1\n", SeedFileError::Malformed { line: 1 }),
            ("0x00000000CA5CADED\n", SeedFileError::Malformed { line: 1 }),
            ("\n12\n", SeedFileError::Malformed { line: 2 }),
            (
                "0x0000000000000001\n0x0000000000000001\n",
                SeedFileError::Duplicate { line: 2, first: 1 },
            ),
            (
                "0x0000000000000001\r\n",
                SeedFileError::CarriageReturn { line: 1 },
            ),
            ("# nothing\n", SeedFileError::Empty),
            ("", SeedFileError::Empty),
        ] {
            assert_eq!(decode(text.as_bytes()), Err(error), "{text:?}");
        }
        assert_eq!(decode(&[0xff, b'\n']), Err(SeedFileError::NotUtf8));
    }

    #[test]
    fn the_caps_hold() {
        let long = format!("0x0000000000000001 #{}\n", "x".repeat(300));
        assert_eq!(
            decode(long.as_bytes()),
            Err(SeedFileError::LineTooLong { line: 1 })
        );
        let mut many = String::new();
        for n in 0..=MAX_SEEDS {
            many.push_str(&spell(u64::try_from(n).unwrap()));
            many.push('\n');
        }
        assert_eq!(
            decode(many.as_bytes()),
            Err(SeedFileError::TooManySeeds {
                line: MAX_SEEDS + 1
            })
        );
        let big = vec![b'#'; super::MAX_FILE_BYTES + 1];
        assert_eq!(decode(&big), Err(SeedFileError::TooLarge));
    }
}
