// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The rules table and `rules_hash` (decisions log item 78).
//!
//! **Tuning values are data** (AGENTS.md §12): K and B, 10/14/4, the move cost
//! per tick, the repath cap, the segment ladder and the CSR cell size sit in
//! one reviewable file — `rules/rules.v1.json`, the canonical JSON of one
//! [`gp::v1::RulesTable`] — rather than as constants sprinkled through the
//! code. The sim decodes that file with `pharmakos-proto`'s canonical JSON
//! codec and hashes the values it reads with the canonical encoder of item 48,
//! so `rules_hash` is one of the verifier's five inputs **by construction**
//! rather than by agreement.
//!
//! The sim is a pure function of `(map seed, playbooks, rules hash)`. That
//! sentence is only true if the rules table is an *input*: nothing in this
//! module enters the state hash, and `tests/determinism.rs` asserts it twice —
//! once over the canonical encoding and once over the chain itself.
//!
//! # The two shapes, and why there are two
//!
//! [`gp::v1::RulesTable`] is the **contract shape**: nested blocks, every row
//! the project will need, guarded by `buf breaking` (item 78). [`RulesTable`]
//! here is the **sim's view**: the flat subset the tick actually reads, in the
//! order [`RulesTable::encode`] walks it. The second is derived from the first
//! by [`RulesTable::from_message`] and by nothing else, so a row cannot reach
//! the sim without passing through the schema.
//!
//! | This struct | `gp.v1.RulesTable` |
//! |---|---|
//! | [`mesher_drain_surfaces`](RulesTable::mesher_drain_surfaces) | `mesher.surfaces_per_frame` |
//! | [`mesher_drain_bytes`](RulesTable::mesher_drain_bytes) | `mesher.bytes_per_frame` |
//! | [`step_cardinal`](RulesTable::step_cardinal) | `locomotion.step_cost_cardinal` |
//! | [`step_diagonal`](RulesTable::step_diagonal) | `locomotion.step_cost_diagonal` |
//! | [`climb_surcharge`](RulesTable::climb_surcharge) | `locomotion.climb_surcharge` |
//! | [`move_cost_per_tick`](RulesTable::move_cost_per_tick) | `locomotion.move_cost_per_tick` |
//! | [`repath_cap_per_tick`](RulesTable::repath_cap_per_tick) | `locomotion.repath_cap_per_tick` |
//! | [`csr_cell_size_voxels`](RulesTable::csr_cell_size_voxels) | `broadphase.cell_size_voxels` |
//! | [`segment_lengths_ms`](RulesTable::segment_lengths_ms) | `match.segment_lengths_ms` |
//!
//! The rows the schema carries and the sim does not yet read — the interface
//! times (T10, T11), the fog fraction and the cluster edge (T7), `lull_ms` (a
//! host and editor concern the sim never reads, AGENTS.md §4.5), and the
//! `economy` and `power` stubs (T14) — are decoded and then discarded here.
//! Each of them joins this struct and the **end** of [`RulesTable::encode`] in
//! the task that first reads it, which moves `rules_hash` once, additively,
//! with the movement explained in that pull request.

use crate::encoding::Enc;
use pharmakos_proto::gp;
use std::fmt;

/// The disk location of the canonical rules table, relative to the repository
/// root.
pub const RULES_PATH: &str = "rules/rules.v1.json";

/// The message the file on disk holds exactly one of.
pub const RULES_MESSAGE: &str = "gp.v1.RulesTable";

/// The rules *encoding's* version, part of the hashed bytes below.
///
/// Not the table's `revision`, which counts value changes and is the file's to
/// carry: this counts changes to [`RulesTable::encode`], and moving it is a
/// determinism change (AGENTS.md §5).
pub const RULES_VERSION: u32 = 1;

/// How many segment lengths the ladder may carry. Fixed so the table's
/// encoding has a bound; the skeleton's ladder is 3 / 5 / 8 minutes (item 68).
pub const MAX_SEGMENT_LENGTHS: usize = 8;

/// Every tuning value the sim reads, in the order the canonical encoder walks
/// them.
///
/// Adding a field is additive: append it, read it out of the schema in
/// [`RulesTable::from_message`], append its encoding at the **end** of
/// [`RulesTable::encode`], and say in the pull request that `rules_hash` moved
/// and why. Reordering is a contract change (AGENTS.md §5).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RulesTable {
    /// Surfaces the mesher may drain per frame. `K = 4` (item 54). From
    /// `mesher.surfaces_per_frame`.
    ///
    /// PLACEHOLDER: `tuning, owner — no measured frame-time reason separates
    /// K = 4 from K = 8 on the spike machine (item 54)`.
    pub mesher_drain_surfaces: u32,
    /// Bytes the mesher may drain per frame. `B = 512 KiB` (item 54). From
    /// `mesher.bytes_per_frame`.
    ///
    /// PLACEHOLDER: as above.
    pub mesher_drain_bytes: u32,
    /// Cost of a cardinal step. `10` (item 59). From
    /// `locomotion.step_cost_cardinal`.
    pub step_cardinal: i32,
    /// Cost of a diagonal step. `14` (item 59). From
    /// `locomotion.step_cost_diagonal`.
    pub step_diagonal: i32,
    /// Surcharge for a one-voxel climb. `4` (item 59). From
    /// `locomotion.climb_surcharge`.
    ///
    /// PLACEHOLDER: `no gameplay evidence behind it` (item 59) — owner, at S3.
    pub climb_surcharge: i32,
    /// Path cost a unit covers per tick. `3` (item 59). Cost to ticks rounds by
    /// **ceiling**, which is T7's to implement and item 59's to fix. From
    /// `locomotion.move_cost_per_tick`.
    pub move_cost_per_tick: i32,
    /// Repaths served per tick, round-robin by `(seat, beacon, unit)`. `16`
    /// (item 69). From `locomotion.repath_cap_per_tick`.
    ///
    /// PLACEHOLDER: re-derived at S2's exit once the burst frequency is a
    /// measurement (item 69) — owner.
    pub repath_cap_per_tick: u32,
    /// The broadphase's cell edge, in whole voxels. From
    /// `broadphase.cell_size_voxels`.
    ///
    /// PLACEHOLDER: `tuning, owner, S2 exit — tied to unit density` (item 67's
    /// caveat). A performance knob, so it must never reach hashed state; the
    /// broadphase returns candidates sorted by id precisely so that it cannot.
    pub csr_cell_size_voxels: i32,
    /// The per-round segment ladder, in game milliseconds (item 68: 3 / 5 / 8
    /// minutes). The runner reads the coming segment's length **from the frozen
    /// snapshot**, not from this row (T10). From `match.segment_lengths_ms`.
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
    /// The decode is `pharmakos-proto`'s and nobody else's (item 74): one
    /// codec, driven by the checked-in descriptor set, so the sim cannot read a
    /// file the verifier would reject. An unknown field is therefore
    /// **rejected** with a JSON Pointer, never ignored and never stripped (spec
    /// section 10, "Load never strips").
    ///
    /// # Errors
    ///
    /// Returns [`RulesError::Json`] carrying the codec's own pointer, or
    /// whatever [`RulesTable::from_message`] finds wrong with the values.
    pub fn from_canonical_json(text: &str) -> Result<RulesTable, RulesError> {
        let message: gp::v1::RulesTable =
            pharmakos_proto::json::decode(text).map_err(RulesError::Json)?;
        RulesTable::from_message(&message)
    }

    /// Take the sim's view of a decoded [`gp::v1::RulesTable`].
    ///
    /// Every block the sim reads is **required**. Proto3 cannot tell an absent
    /// message from an empty one, so a missing `locomotion` would otherwise
    /// arrive as a table of zeroes and a unit that never moves — a silent wrong
    /// answer, which is the failure mode the rules table exists to prevent.
    ///
    /// # Errors
    ///
    /// Returns [`RulesError::MissingBlock`] when a block the sim reads is
    /// absent, or [`RulesError::OutOfRange`] when a value does not fit the
    /// sim's own type for it.
    pub fn from_message(message: &gp::v1::RulesTable) -> Result<RulesTable, RulesError> {
        let locomotion = message
            .locomotion
            .as_ref()
            .ok_or(RulesError::MissingBlock("locomotion"))?;
        let broadphase = message
            .broadphase
            .as_ref()
            .ok_or(RulesError::MissingBlock("broadphase"))?;
        let mesher = message
            .mesher
            .as_ref()
            .ok_or(RulesError::MissingBlock("mesher"))?;
        let matched = message
            .r#match
            .as_ref()
            .ok_or(RulesError::MissingBlock("match"))?;

        if matched.segment_lengths_ms.is_empty() {
            return Err(RulesError::OutOfRange {
                field: "match.segment_lengths_ms".to_owned(),
                value: "an empty ladder; a match plays at least one segment".to_owned(),
            });
        }
        if matched.segment_lengths_ms.len() > MAX_SEGMENT_LENGTHS {
            return Err(RulesError::OutOfRange {
                field: "match.segment_lengths_ms".to_owned(),
                value: format!(
                    "{} entries; the encoding is bounded at {MAX_SEGMENT_LENGTHS}",
                    matched.segment_lengths_ms.len()
                ),
            });
        }

        Ok(RulesTable {
            mesher_drain_surfaces: mesher.surfaces_per_frame,
            mesher_drain_bytes: mesher.bytes_per_frame,
            step_cardinal: signed(
                "locomotion.step_cost_cardinal",
                locomotion.step_cost_cardinal,
            )?,
            step_diagonal: signed(
                "locomotion.step_cost_diagonal",
                locomotion.step_cost_diagonal,
            )?,
            climb_surcharge: signed("locomotion.climb_surcharge", locomotion.climb_surcharge)?,
            move_cost_per_tick: signed(
                "locomotion.move_cost_per_tick",
                locomotion.move_cost_per_tick,
            )?,
            repath_cap_per_tick: locomotion.repath_cap_per_tick,
            csr_cell_size_voxels: signed(
                "broadphase.cell_size_voxels",
                broadphase.cell_size_voxels,
            )?,
            segment_lengths_ms: matched.segment_lengths_ms.clone(),
        })
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

/// The schema spells the move costs `uint32`; the sim's path arithmetic is
/// signed, so the narrowing is a decision and gets one (AGENTS.md §4.3).
fn signed(field: &'static str, value: u32) -> Result<i32, RulesError> {
    i32::try_from(value).map_err(|_| RulesError::OutOfRange {
        field: field.to_owned(),
        value: value.to_string(),
    })
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
    /// The text is not canonical `gp.v1.RulesTable` JSON. Carries the codec's
    /// own JSON Pointer, so a diagnostic can point at the byte that caused it.
    Json(pharmakos_proto::json::Error),
    /// A block the sim reads is absent. Proto3 cannot tell that from a block of
    /// zeroes, so the sim refuses rather than guesses.
    MissingBlock(&'static str),
    /// A value is out of the range the sim's own type for it allows.
    OutOfRange {
        /// The field's path in the message.
        field: String,
        /// What the table said.
        value: String,
    },
}

impl fmt::Display for RulesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RulesError::Io { path, message } => write!(f, "reading {path}: {message}"),
            RulesError::Json(error) => write!(f, "{RULES_MESSAGE}: {error}"),
            RulesError::MissingBlock(name) => write!(
                f,
                "the `{name}` block is missing; the sim reads it and will not \
                 substitute zeroes for it"
            ),
            RulesError::OutOfRange { field, value } => {
                write!(f, "field `{field}`: `{value}` is out of range")
            }
        }
    }
}

impl std::error::Error for RulesError {}
