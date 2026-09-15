// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The rules table and `rules_hash` (decisions log item 78).
//!
//! **Tuning values are data** (AGENTS.md §12): K and B, 10/14/4, the move cost
//! per tick, the repath cap, the segment ladder and the CSR cell size sit in
//! one reviewable file — `rules/rules.v1.json`, the canonical JSON of one
//! [`gp::v1::RulesTable`] — rather than as constants sprinkled through the
//! code. The sim decodes that file with `pharmakos-proto`'s canonical JSON
//! codec and hashes it with the canonical encoder of item 48, so `rules_hash`
//! is one of the verifier's five `report_hash` inputs **by construction**
//! rather than by agreement.
//!
//! The sim is a pure function of `(map seed, playbooks, rules hash)`. That
//! sentence is only true if the rules table is an *input*: nothing in this
//! module enters the state hash, and `tests/determinism.rs` asserts it twice —
//! once over the canonical encoding and once over the chain itself.
//!
//! # `rules_hash` covers the whole table, not the rows the sim reads
//!
//! [`RulesTable::encode`] hashes the **canonical JSON of the whole decoded
//! message**: every row, including the ones this crate never looks at, plus
//! `revision` and `note`.
//!
//! It is deliberately not a walk over the sim's own fields, because the rows
//! the sim does not read are exactly the ones the *verifier* and *plan-core*
//! will: `interface_times.*` is T10's and T11's interface arithmetic,
//! `locomotion.fog_cost_*` and `hpa_cluster_voxels` are T7's estimator (item
//! 61), `match.lull_ms` is the host's. A hash over nine hand-picked fields
//! would let a tuning pull request change
//! `interface_times.place_beacon_deploy_ms` — changing what the verifier
//! reports — while leaving `report_hash`, and every report cached under it,
//! exactly where it was. Hashing the message closes that by construction, and
//! it cannot be reopened by forgetting to mirror a new row here.
//!
//! Two consequences, both intended:
//!
//! * Editing `note` moves `rules_hash`. The hash identifies the *table*, and
//!   `revision` is meant to move whenever a value does, so an edit that moves
//!   neither is an edit that changed nothing.
//! * The canonical form is the codec's, so a reformatted file hashes the same
//!   as the file it was reformatted from. `crates/proto`'s
//!   `the_rules_table_is_in_canonical_form` keeps the committed file identical
//!   to that form anyway.
//!
//! # The two shapes, and why there are two
//!
//! [`gp::v1::RulesTable`] is the **contract shape**: nested blocks, every row
//! the project will need, guarded by `buf breaking` (item 78). [`RulesTable`]
//! here is the **sim's view**: the flat subset the tick actually reads, derived
//! by [`RulesTable::from_message`] and by nothing else, so a row cannot reach
//! the sim without passing through the schema. The view is read-only accessors
//! rather than public fields, because a field that could be mutated after
//! construction could disagree with the message the hash is over — which is
//! the same bug in a smaller box.
//!
//! | Accessor | `gp.v1.RulesTable` |
//! |---|---|
//! | [`mesher_drain_surfaces`](RulesTable::mesher_drain_surfaces) | `mesher.surfaces_per_frame` |
//! | [`mesher_drain_bytes`](RulesTable::mesher_drain_bytes) | `mesher.bytes_per_frame` |
//! | [`step_cardinal`](RulesTable::step_cardinal) | `locomotion.step_cost_cardinal` |
//! | [`step_diagonal`](RulesTable::step_diagonal) | `locomotion.step_cost_diagonal` |
//! | [`climb_surcharge`](RulesTable::climb_surcharge) | `locomotion.climb_surcharge` |
//! | [`move_cost_per_tick`](RulesTable::move_cost_per_tick) | `locomotion.move_cost_per_tick` (superseded by item 90's per-kind rows; unread) |
//! | [`repath_cap_per_tick`](RulesTable::repath_cap_per_tick) | `locomotion.repath_cap_per_tick` |
//! | [`csr_cell_size_voxels`](RulesTable::csr_cell_size_voxels) | `broadphase.cell_size_voxels` |
//! | [`segment_lengths_ms`](RulesTable::segment_lengths_ms) | `match.segment_lengths_ms` |
//! | [`map_size_voxels`](RulesTable::map_size_voxels) | `map.size_x` / `size_y` / `size_z` |
//! | [`commander_cost_per_second`](RulesTable::commander_cost_per_second) | `commander.cost_per_second` |
//! | [`unit_hp`](RulesTable::unit_hp), [`unit_draw_kw`](RulesTable::unit_draw_kw) | `units.<kind>.hp` / `.draw_kw` |
//!
//! The map generator ([`crate::mapgen`]) reads a dozen more rows — the whole of
//! `map`, `beacon`, `power`, `economy` and `units` — and reads them from
//! [`RulesTable::message`] rather than through accessors here. That is
//! deliberate: those rows are read **once, at generation**, by one module that
//! already has to report a rules-table mistake as a typed error, and mirroring
//! them into this flat view would be a second copy of numbers the view's own
//! rule (read-only accessors, derived from the message and nothing else) exists
//! to prevent. The rows the *tick* reads are the ones that earn an accessor.
//!
//! A row the sim starts reading joins that table and gains an accessor in the
//! task that first reads it. That is an ordinary change now: it does not move
//! `rules_hash`, because the hash already covered the row.

use crate::encoding::Enc;
use crate::math::quantity::{Hp, Kw};
use crate::tables::UnitKind;
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

/// One rules table: the decoded message, its canonical bytes, and the flat
/// view the tick reads.
///
/// Built only by [`RulesTable::from_message`] and the two loaders above it, so
/// the view and the bytes can never describe two different tables.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RulesTable {
    /// The whole decoded table, kept so `rules_hash` can cover all of it.
    message: gp::v1::RulesTable,
    /// `message` rendered back through the canonical JSON codec, once, at
    /// construction. These are the bytes [`RulesTable::encode`] hashes.
    canonical: String,
    /// Surfaces the mesher may drain per frame. `K = 4` (item 54). From
    /// `mesher.surfaces_per_frame`.
    ///
    /// PLACEHOLDER: `tuning, owner — no measured frame-time reason separates
    /// K = 4 from K = 8 on the spike machine (item 54)`.
    mesher_drain_surfaces: u32,
    /// Bytes the mesher may drain per frame. `B = 512 KiB` (item 54). From
    /// `mesher.bytes_per_frame`.
    ///
    /// PLACEHOLDER: as above.
    mesher_drain_bytes: u32,
    /// Cost of a cardinal step. `10` (item 59). From
    /// `locomotion.step_cost_cardinal`.
    step_cardinal: i32,
    /// Cost of a diagonal step. `14` (item 59). From
    /// `locomotion.step_cost_diagonal`.
    step_diagonal: i32,
    /// Surcharge for a one-voxel climb. `4` (item 59). From
    /// `locomotion.climb_surcharge`.
    ///
    /// PLACEHOLDER: `no gameplay evidence behind it` (item 59) — owner, at S3.
    climb_surcharge: i32,
    /// Path cost a unit covers per tick. `3` (item 59). From
    /// `locomotion.move_cost_per_tick`.
    ///
    /// **SUPERSEDED** by item 90's per-kind `locomotion.*_cost_per_second`
    /// rows, which express the same model in cost units per second: T7
    /// integrates those with an accumulator (add `cost_per_second` per tick,
    /// spend one cost unit per 20 accumulated) and estimates with
    /// `ceil(cost * 20 / cost_per_second)` ticks, which is where item 59's
    /// **ceiling** rule now lives. Nothing reads this row; it stays in the
    /// schema because deleting a field is a `buf breaking` failure in
    /// `WIRE_JSON` mode.
    move_cost_per_tick: i32,
    /// Repaths served per tick, round-robin by `(seat, beacon, unit)`. `16`
    /// (item 69). From `locomotion.repath_cap_per_tick`.
    ///
    /// PLACEHOLDER: re-derived at S2's exit once the burst frequency is a
    /// measurement (item 69) — owner.
    repath_cap_per_tick: u32,
    /// The broadphase's cell edge, in whole voxels. From
    /// `broadphase.cell_size_voxels`.
    ///
    /// PLACEHOLDER: `tuning, owner, S2 exit — tied to unit density` (item 67's
    /// caveat). A performance knob, so it must never reach hashed state; the
    /// broadphase takes its query radius in voxels and filters it exactly,
    /// precisely so that it cannot.
    csr_cell_size_voxels: i32,
    /// The per-round segment ladder, in game milliseconds (item 68: 3 / 5 / 8
    /// minutes). The runner reads the coming segment's length **from the frozen
    /// snapshot**, not from this row (T10). From `match.segment_lengths_ms`.
    segment_lengths_ms: Vec<i32>,
    /// The map extent in voxels, `[x, y, z]`. From `map.size_*`.
    ///
    /// PLACEHOLDER: `tuning, owner, at the walking skeleton's demo` (item 90).
    map_size_voxels: [i32; 3],
    /// The commander's walking speed in cost units per second. `10` (item 90).
    /// From `commander.cost_per_second`.
    ///
    /// PLACEHOLDER: `tuning, owner, at the walking skeleton's demo` (item 90).
    commander_cost_per_second: i32,
}

impl RulesTable {
    /// Append the table to the canonical encoding: the encoding version, then
    /// the canonical JSON of the whole decoded message, length-prefixed.
    ///
    /// This is the function `rules_hash` is, and it is determinism code: a
    /// change to it moves every `report_hash` the verifier has ever produced.
    /// It covers every row of the table rather than the subset the sim reads —
    /// see this module's header for why that is the load-bearing part.
    pub fn encode(&self, enc: &mut Enc) {
        enc.u32(RULES_VERSION);
        let bytes = self.canonical.as_bytes();
        enc.len(u32::try_from(bytes.len()).unwrap_or(u32::MAX));
        enc.bytes(bytes);
    }

    /// The rules hash: xxh3-64 over the canonical encoding at the project seed.
    ///
    /// One of the verifier's five `report_hash` inputs, and stamped into every
    /// save so a save made under different rules refuses to load (T17).
    #[must_use]
    pub fn rules_hash(&self) -> u64 {
        let mut enc = Enc::with_capacity(self.canonical.len().saturating_add(16));
        self.encode(&mut enc);
        enc.finish()
    }

    /// The whole decoded table, for a caller that needs a row the sim's own
    /// view does not carry.
    #[must_use]
    pub const fn message(&self) -> &gp::v1::RulesTable {
        &self.message
    }

    /// The canonical JSON `rules_hash` is taken over.
    #[must_use]
    pub fn canonical_json(&self) -> &str {
        &self.canonical
    }

    /// Surfaces the mesher may drain per frame (`mesher.surfaces_per_frame`).
    #[must_use]
    pub const fn mesher_drain_surfaces(&self) -> u32 {
        self.mesher_drain_surfaces
    }

    /// Bytes the mesher may drain per frame (`mesher.bytes_per_frame`).
    #[must_use]
    pub const fn mesher_drain_bytes(&self) -> u32 {
        self.mesher_drain_bytes
    }

    /// Cost of a cardinal step (`locomotion.step_cost_cardinal`).
    #[must_use]
    pub const fn step_cardinal(&self) -> i32 {
        self.step_cardinal
    }

    /// Cost of a diagonal step (`locomotion.step_cost_diagonal`).
    #[must_use]
    pub const fn step_diagonal(&self) -> i32 {
        self.step_diagonal
    }

    /// Surcharge for a one-voxel climb (`locomotion.climb_surcharge`).
    #[must_use]
    pub const fn climb_surcharge(&self) -> i32 {
        self.climb_surcharge
    }

    /// Path cost a unit covers per tick (`locomotion.move_cost_per_tick`).
    ///
    /// **Superseded** by item 90's per-kind `locomotion.*_cost_per_second`
    /// rows; see the field's documentation. Nothing reads this, and T7 should
    /// reach for the per-kind rows instead.
    #[must_use]
    pub const fn move_cost_per_tick(&self) -> i32 {
        self.move_cost_per_tick
    }

    /// Repaths served per tick (`locomotion.repath_cap_per_tick`).
    #[must_use]
    pub const fn repath_cap_per_tick(&self) -> u32 {
        self.repath_cap_per_tick
    }

    /// The broadphase's cell edge in voxels (`broadphase.cell_size_voxels`).
    #[must_use]
    pub const fn csr_cell_size_voxels(&self) -> i32 {
        self.csr_cell_size_voxels
    }

    /// The per-round segment ladder (`match.segment_lengths_ms`).
    #[must_use]
    pub fn segment_lengths_ms(&self) -> &[i32] {
        &self.segment_lengths_ms
    }

    /// The map extent in voxels, `[x, y, z]` (`map.size_x` / `size_y` /
    /// `size_z`).
    #[must_use]
    pub const fn map_size_voxels(&self) -> [i32; 3] {
        self.map_size_voxels
    }

    /// The commander's walking speed in cost units per second
    /// (`commander.cost_per_second`).
    #[must_use]
    pub const fn commander_cost_per_second(&self) -> i32 {
        self.commander_cost_per_second
    }

    /// One unit kind's hit points (`units.<kind>.hp`), or `None` when the
    /// `units` block or that kind's row is absent.
    ///
    /// Read off the message rather than mirrored into the view above, because
    /// the sim reads a *kind's* row and there are six of them: six accessors
    /// would be six chances for one to fall out of step with the schema.
    #[must_use]
    pub fn unit_hp(&self, kind: UnitKind) -> Option<Hp> {
        let row = self.unit_row(kind)?;
        Some(Hp::new(i32::try_from(row.hp).unwrap_or(i32::MAX)))
    }

    /// One unit kind's power draw (`units.<kind>.draw_kw`), or `None` when the
    /// `units` block or that kind's row is absent.
    #[must_use]
    pub fn unit_draw_kw(&self, kind: UnitKind) -> Option<Kw> {
        let row = self.unit_row(kind)?;
        Some(Kw::new(i32::try_from(row.draw_kw).unwrap_or(i32::MAX)))
    }

    /// One unit kind's row. The commander is not a unit kind in the schema —
    /// it has a block of its own — so it has no row here.
    fn unit_row(&self, kind: UnitKind) -> Option<gp::v1::rules_table::UnitKind> {
        let units = self.message.units.as_ref()?;
        match kind {
            UnitKind::Commander => None,
            UnitKind::BuildDrone => units.build_drone,
            UnitKind::MiningDrone => units.mining_drone,
            UnitKind::RepairDrone => units.repair_drone,
            UnitKind::Raider => units.raider,
            UnitKind::Scout => units.scout,
        }
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
    /// absent, [`RulesError::OutOfRange`] when a value does not fit the sim's
    /// own type for it, or [`RulesError::Json`] when the message will not
    /// render back to canonical JSON — which is what `rules_hash` is over, so
    /// a table that cannot be rendered is a table that cannot be hashed and
    /// must not become a [`RulesTable`].
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
        let map = message
            .map
            .as_ref()
            .ok_or(RulesError::MissingBlock("map"))?;
        let commander = message
            .commander
            .as_ref()
            .ok_or(RulesError::MissingBlock("commander"))?;

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

        let canonical = pharmakos_proto::json::encode(message).map_err(RulesError::Json)?;

        Ok(RulesTable {
            message: message.clone(),
            canonical,
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
            map_size_voxels: [
                signed("map.size_x", map.size_x)?,
                signed("map.size_y", map.size_y)?,
                signed("map.size_z", map.size_z)?,
            ],
            commander_cost_per_second: signed(
                "commander.cost_per_second",
                commander.cost_per_second,
            )?,
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
    /// The text is not canonical `gp.v1.RulesTable` JSON, or a decoded table
    /// will not render back to it. Carries the codec's own JSON Pointer, so a
    /// diagnostic can point at the byte that caused it.
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
