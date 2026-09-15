// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The rules table and `rules_hash` (decisions log item 78).
//!
//! **Tuning values are data** (AGENTS.md §12): K and B, 10/14/4, the move cost
//! per tick, the repath cap, the segment ladder and the CSR cell size sit in
//! one reviewable file — `rules/rules.v1.json`, canonical JSON — rather than as
//! constants sprinkled through the code. The sim loads that file and hashes it
//! with the canonical encoder of item 48, so `rules_hash` is one of the
//! verifier's five inputs **by construction** rather than by agreement.
//!
//! The sim is a pure function of `(map seed, playbooks, rules hash)`. That
//! sentence is only true if the rules table is an *input*: nothing in this
//! module enters the state hash, and `tests/determinism.rs` asserts it.
//!
//! # PLACEHOLDER — this struct is a stand-in for `gp.v1.RulesTable`
//!
//! Item 78 puts the table's *shape* in the proto, where `buf breaking` guards
//! it, and its *values* in the JSON, where a tuning change is an ordinary pull
//! request. T1 defines `gp.v1.RulesTable` in parallel with this task. Until it
//! merges, [`RulesTable`] is a local Rust struct **with the same field names**
//! and [`RulesTable::from_canonical_json`] is a minimal reader for exactly that
//! shape.
//!
//! When T1 merges, three things happen in one pull request and nothing else
//! changes: `pharmakos-proto` becomes a dependency, [`json`] is deleted, and
//! [`RulesTable::from_canonical_json`] becomes a thin call into the proto
//! crate's canonical JSON codec. [`RulesTable::encode`] — the part that decides
//! `rules_hash` — does not move. *(who resolves: T1's author, at T1's merge.)*

use crate::encoding::Enc;
use std::fmt;

/// The disk location of the canonical rules table, relative to the repository
/// root.
pub const RULES_PATH: &str = "rules/rules.v1.json";

/// The rules table's own version, part of the hashed encoding.
pub const RULES_VERSION: u32 = 1;

/// How many segment lengths the ladder may carry. Fixed so the table's
/// encoding has a bound; the skeleton's ladder is 3 / 5 / 8 minutes (item 68).
pub const MAX_SEGMENT_LENGTHS: usize = 8;

/// Every tuning value the sim reads, in the order the canonical encoder walks
/// them.
///
/// Adding a field is additive: append it, append its encoding at the end of
/// [`RulesTable::encode`], and say in the pull request that `rules_hash` moved
/// and why. Reordering or renumbering is a contract change (AGENTS.md §5).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RulesTable {
    /// Surfaces the mesher may drain per frame. `K = 4` (item 54).
    ///
    /// PLACEHOLDER: `tuning, owner — no measured frame-time reason separates
    /// K = 4 from K = 8 on the spike machine (item 54)`.
    pub mesher_drain_surfaces: u32,
    /// Bytes the mesher may drain per frame. `B = 512 KiB` (item 54).
    ///
    /// PLACEHOLDER: as above.
    pub mesher_drain_bytes: u32,
    /// Cost of a cardinal step. `10` (item 59).
    pub step_cardinal: i32,
    /// Cost of a diagonal step. `14` (item 59).
    pub step_diagonal: i32,
    /// Surcharge for a one-voxel climb. `4` (item 59).
    ///
    /// PLACEHOLDER: `no gameplay evidence behind it` (item 59) — owner, at S3.
    pub climb_surcharge: i32,
    /// Path cost a unit covers per tick. `3` (item 59). Cost to ticks rounds by
    /// **ceiling**, which is T7's to implement and item 59's to fix.
    pub move_cost_per_tick: i32,
    /// Repaths served per tick, round-robin by `(seat, beacon, unit)`. `16`
    /// (item 69).
    ///
    /// PLACEHOLDER: re-derived at S2's exit once the burst frequency is a
    /// measurement (item 69) — owner.
    pub repath_cap_per_tick: u32,
    /// The broadphase's cell edge, in whole voxels.
    ///
    /// PLACEHOLDER: `tuning, owner, S2 exit — tied to unit density` (item 67's
    /// caveat). A performance knob, so it must never reach hashed state; the
    /// broadphase returns candidates sorted by id precisely so that it cannot.
    pub csr_cell_size_voxels: i32,
    /// The per-round segment ladder, in game milliseconds (item 68: 3 / 5 / 8
    /// minutes). The runner reads the coming segment's length **from the frozen
    /// snapshot**, not from this row (T10).
    pub segment_lengths_ms: Vec<i32>,
}

impl RulesTable {
    /// Append the table to the canonical encoding, in declared order.
    ///
    /// This is the function `rules_hash` is, and it is determinism code: a
    /// change to it moves every `report_hash` the verifier has ever produced.
    pub fn encode(&self, enc: &mut Enc) {
        enc.u32(RULES_VERSION);
        enc.u32(self.mesher_drain_surfaces);
        enc.u32(self.mesher_drain_bytes);
        enc.i32(self.step_cardinal);
        enc.i32(self.step_diagonal);
        enc.i32(self.climb_surcharge);
        enc.i32(self.move_cost_per_tick);
        enc.u32(self.repath_cap_per_tick);
        enc.i32(self.csr_cell_size_voxels);
        enc.len(u32::try_from(self.segment_lengths_ms.len()).unwrap_or(u32::MAX));
        for ms in &self.segment_lengths_ms {
            enc.i32(*ms);
        }
    }

    /// The rules hash: xxh3-64 over the canonical encoding at the project seed.
    ///
    /// One of the verifier's five `report_hash` inputs, and stamped into every
    /// save so a save made under different rules refuses to load (T17).
    #[must_use]
    pub fn rules_hash(&self) -> u64 {
        let mut enc = Enc::with_capacity(128);
        self.encode(&mut enc);
        enc.finish()
    }

    /// Read a rules table from canonical proto JSON.
    ///
    /// Unknown fields are **rejected**, not ignored — the same rule submitted
    /// playbooks are held to (AGENTS.md §5).
    ///
    /// # Errors
    ///
    /// Returns [`RulesError`] naming the field and what was wrong with it.
    pub fn from_canonical_json(text: &str) -> Result<RulesTable, RulesError> {
        json::parse(text)
    }

    /// Read the rules table from a file.
    ///
    /// # Errors
    ///
    /// Returns [`RulesError::Io`] when the file cannot be read, or whatever
    /// [`RulesTable::from_canonical_json`] returns.
    pub fn load(path: &std::path::Path) -> Result<RulesTable, RulesError> {
        let text = std::fs::read_to_string(path).map_err(|error| RulesError::Io {
            path: path.display().to_string(),
            message: error.to_string(),
        })?;
        RulesTable::from_canonical_json(&text)
    }
}

/// What went wrong reading a rules table.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum RulesError {
    /// The file could not be read.
    Io {
        /// The path that was tried.
        path: String,
        /// The operating system's message.
        message: String,
    },
    /// The text is not the JSON object the table's shape calls for.
    Syntax {
        /// Byte offset into the text.
        offset: usize,
        /// What was expected there.
        expected: &'static str,
    },
    /// A field the table requires is missing.
    MissingField(&'static str),
    /// A field appears twice.
    DuplicateField(String),
    /// A field the table does not define appears.
    UnknownField(String),
    /// A value is out of the range its field allows.
    OutOfRange {
        /// The field's canonical JSON name.
        field: String,
        /// What the text said.
        value: String,
    },
}

impl fmt::Display for RulesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RulesError::Io { path, message } => write!(f, "reading {path}: {message}"),
            RulesError::Syntax { offset, expected } => {
                write!(f, "at byte {offset}: expected {expected}")
            }
            RulesError::MissingField(name) => write!(f, "missing field `{name}`"),
            RulesError::DuplicateField(name) => write!(f, "field `{name}` appears twice"),
            RulesError::UnknownField(name) => write!(
                f,
                "unknown field `{name}`; an unknown field is rejected, never ignored"
            ),
            RulesError::OutOfRange { field, value } => {
                write!(f, "field `{field}`: `{value}` is out of range")
            }
        }
    }
}

impl std::error::Error for RulesError {}

/// PLACEHOLDER — the temporary canonical-JSON reader.
///
/// Item 78 rejected "a plain JSONC file with a hand-written schema (a second
/// parser in the sim)" and chose `gp.v1.RulesTable` read through the proto
/// crate's canonical JSON codec. This module is the scaffold that lets T1 and
/// T2 merge in either order, and it is **deleted** when T1 lands. It is
/// deliberately narrow: a flat object of `int32` scalars and one flat array of
/// `int32`, no nesting, no floats, no strings, no comments — which is exactly
/// what canonical proto JSON emits for the shape above (item 46: durations are
/// `int32`, so they are bare numbers).
mod json {
    use super::{MAX_SEGMENT_LENGTHS, RulesError, RulesTable};

    /// Every field the table defines, in canonical JSON (lowerCamelCase) form.
    const FIELDS: [&str; 9] = [
        "mesherDrainSurfaces",
        "mesherDrainBytes",
        "stepCardinal",
        "stepDiagonal",
        "climbSurcharge",
        "moveCostPerTick",
        "repathCapPerTick",
        "csrCellSizeVoxels",
        "segmentLengthsMs",
    ];

    struct Scanner<'a> {
        text: &'a [u8],
        at: usize,
    }

    impl<'a> Scanner<'a> {
        fn new(text: &'a str) -> Scanner<'a> {
            Scanner {
                text: text.as_bytes(),
                at: 0,
            }
        }

        fn peek(&self) -> Option<u8> {
            self.text.get(self.at).copied()
        }

        fn bump(&mut self) {
            self.at = self.at.saturating_add(1);
        }

        fn skip_space(&mut self) {
            while let Some(b) = self.peek() {
                if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                    self.bump();
                } else {
                    break;
                }
            }
        }

        fn require(&mut self, byte: u8, expected: &'static str) -> Result<(), RulesError> {
            self.skip_space();
            if self.peek() == Some(byte) {
                self.bump();
                Ok(())
            } else {
                Err(RulesError::Syntax {
                    offset: self.at,
                    expected,
                })
            }
        }

        fn string(&mut self) -> Result<String, RulesError> {
            self.require(b'"', "a field name in double quotes")?;
            let start = self.at;
            while let Some(b) = self.peek() {
                if b == b'"' {
                    let bytes = self.text.get(start..self.at).unwrap_or(&[]);
                    let name =
                        String::from_utf8(bytes.to_vec()).map_err(|_| RulesError::Syntax {
                            offset: start,
                            expected: "a field name in UTF-8",
                        })?;
                    self.bump();
                    return Ok(name);
                }
                if b == b'\\' {
                    // No field name in this shape needs an escape, and
                    // accepting one would mean a second unescaping path.
                    return Err(RulesError::Syntax {
                        offset: self.at,
                        expected: "a field name without escapes",
                    });
                }
                self.bump();
            }
            Err(RulesError::Syntax {
                offset: self.at,
                expected: "a closing quote",
            })
        }

        fn integer(&mut self) -> Result<i64, RulesError> {
            self.skip_space();
            let start = self.at;
            if self.peek() == Some(b'-') {
                self.bump();
            }
            let digits_from = self.at;
            while let Some(b) = self.peek() {
                if b.is_ascii_digit() {
                    self.bump();
                } else {
                    break;
                }
            }
            if self.at == digits_from {
                return Err(RulesError::Syntax {
                    offset: start,
                    expected: "a whole number",
                });
            }
            // Canonical JSON for an int32 is a bare integer. A decimal point or
            // an exponent is a float, and the sim has no floats.
            if matches!(self.peek(), Some(b'.' | b'e' | b'E')) {
                return Err(RulesError::Syntax {
                    offset: self.at,
                    expected: "a whole number (the rules table holds no floats)",
                });
            }
            let bytes = self.text.get(start..self.at).unwrap_or(&[]);
            let text = core::str::from_utf8(bytes).map_err(|_| RulesError::Syntax {
                offset: start,
                expected: "a whole number in UTF-8",
            })?;
            text.parse::<i64>().map_err(|_| RulesError::Syntax {
                offset: start,
                expected: "a whole number that fits in 64 bits",
            })
        }
    }

    fn as_i32(field: &str, value: i64) -> Result<i32, RulesError> {
        i32::try_from(value).map_err(|_| RulesError::OutOfRange {
            field: field.to_owned(),
            value: value.to_string(),
        })
    }

    fn as_u32(field: &str, value: i64) -> Result<u32, RulesError> {
        u32::try_from(value).map_err(|_| RulesError::OutOfRange {
            field: field.to_owned(),
            value: value.to_string(),
        })
    }

    /// Parse the whole document. Flat by construction, so there is no recursion
    /// (item 51).
    pub(super) fn parse(text: &str) -> Result<RulesTable, RulesError> {
        let mut scanner = Scanner::new(text);
        let mut seen: Vec<String> = Vec::with_capacity(FIELDS.len());
        let mut scalars: [Option<i64>; 8] = [None; 8];
        let mut segments: Option<Vec<i32>> = None;

        scanner.require(b'{', "an object")?;
        scanner.skip_space();
        if scanner.peek() != Some(b'}') {
            loop {
                let name = scanner.string()?;
                if seen.contains(&name) {
                    return Err(RulesError::DuplicateField(name));
                }
                let Some(index) = FIELDS.iter().position(|f| *f == name) else {
                    return Err(RulesError::UnknownField(name));
                };
                seen.push(name.clone());
                scanner.require(b':', "`:` after a field name")?;

                if index == 8 {
                    segments = Some(parse_segment_array(&mut scanner, &name)?);
                } else {
                    let value = scanner.integer()?;
                    if let Some(slot) = scalars.get_mut(index) {
                        *slot = Some(value);
                    }
                }

                scanner.skip_space();
                match scanner.peek() {
                    Some(b',') => scanner.bump(),
                    Some(b'}') => break,
                    _ => {
                        return Err(RulesError::Syntax {
                            offset: scanner.at,
                            expected: "`,` or `}`",
                        });
                    }
                }
            }
        }
        scanner.require(b'}', "`}`")?;
        scanner.skip_space();
        if scanner.peek().is_some() {
            return Err(RulesError::Syntax {
                offset: scanner.at,
                expected: "end of document",
            });
        }

        let get = |index: usize| -> Result<i64, RulesError> {
            scalars
                .get(index)
                .copied()
                .flatten()
                .ok_or(RulesError::MissingField(
                    FIELDS.get(index).copied().unwrap_or("?"),
                ))
        };

        Ok(RulesTable {
            mesher_drain_surfaces: as_u32(FIELDS[0], get(0)?)?,
            mesher_drain_bytes: as_u32(FIELDS[1], get(1)?)?,
            step_cardinal: as_i32(FIELDS[2], get(2)?)?,
            step_diagonal: as_i32(FIELDS[3], get(3)?)?,
            climb_surcharge: as_i32(FIELDS[4], get(4)?)?,
            move_cost_per_tick: as_i32(FIELDS[5], get(5)?)?,
            repath_cap_per_tick: as_u32(FIELDS[6], get(6)?)?,
            csr_cell_size_voxels: as_i32(FIELDS[7], get(7)?)?,
            segment_lengths_ms: segments.ok_or(RulesError::MissingField(FIELDS[8]))?,
        })
    }

    fn parse_segment_array(scanner: &mut Scanner<'_>, field: &str) -> Result<Vec<i32>, RulesError> {
        scanner.require(b'[', "an array of whole numbers")?;
        let mut out: Vec<i32> = Vec::with_capacity(MAX_SEGMENT_LENGTHS);
        scanner.skip_space();
        if scanner.peek() == Some(b']') {
            scanner.bump();
            return Ok(out);
        }
        loop {
            let value = scanner.integer()?;
            if out.len() >= MAX_SEGMENT_LENGTHS {
                return Err(RulesError::OutOfRange {
                    field: field.to_owned(),
                    value: format!("more than {MAX_SEGMENT_LENGTHS} entries"),
                });
            }
            out.push(as_i32(field, value)?);
            scanner.skip_space();
            match scanner.peek() {
                Some(b',') => scanner.bump(),
                Some(b']') => {
                    scanner.bump();
                    return Ok(out);
                }
                _ => {
                    return Err(RulesError::Syntax {
                        offset: scanner.at,
                        expected: "`,` or `]`",
                    });
                }
            }
        }
    }
}
