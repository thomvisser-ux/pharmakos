// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The numbers the checks compare against, read from the rules table.
//!
//! **Tuning values are data** (AGENTS.md section 12). Not one number below is a
//! constant in this crate: the size budget, the cooldown floor, the `max_fires`
//! ceiling, the notebook length, the reach window, the map extent and the
//! beacon sphere all come from `rules/rules.v1.json` through
//! [`RulesTable::message`], and they are covered by `rules_hash` — so a tuning
//! pull request that moves one of them moves every `report_hash` with it, by
//! construction rather than by agreement (decisions-log item 89).
//!
//! # The two numbers that are not rows, and why
//!
//! A condition's depth and node limits are **spec** numbers, not tuning ones:
//! spec section 10 fixes them at four levels and twenty-four nodes in the same
//! breath as "integer comparisons only". They are [`CONDITION_MAX_DEPTH`] and
//! [`CONDITION_MAX_NODES`] here.
//!
//! **PLACEHOLDER — if either becomes a tuning number it needs a rules-table row
//! of its own, like every other tuning value (owner, S3, with the rule list at
//! full depth).** They are written here rather than in `rules/rules.v1.json`
//! because inventing a row the owner has not decided on would be the larger
//! mistake, and because `rules.proto` is a contract file this task does not
//! touch.
//!
//! # A missing block is refused, never defaulted
//!
//! Proto3 cannot tell an absent message from an empty one, so a rules table with
//! no `verifier` block would arrive as a size budget of zero and reject every
//! playbook ever written. [`Limits::from_rules`] returns [`RulesGap`] instead,
//! which is the same choice `pharmakos_sim::rules::RulesTable::from_message`
//! makes for the blocks the sim reads and for the same reason.

use core::fmt;

use pharmakos_sim::rules::RulesTable;

/// How deeply a condition may nest (spec section 10, "Conditions").
pub const CONDITION_MAX_DEPTH: u32 = 4;

/// How many nodes one condition may hold (spec section 10, "Conditions").
pub const CONDITION_MAX_NODES: u32 = 24;

/// The rules table cannot answer something the verifier reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RulesGap {
    /// A block the verifier reads is absent. Proto3 cannot tell that from a
    /// block of zeroes, so the verifier refuses rather than guesses.
    MissingBlock(&'static str),
    /// A value does not fit the signed type the checks compare in.
    OutOfRange {
        /// The field's path in `gp.v1.RulesTable`.
        field: &'static str,
        /// What the table said.
        value: u32,
    },
}

impl fmt::Display for RulesGap {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RulesGap::MissingBlock(block) => write!(
                formatter,
                "the rules table has no `{block}` block; the verifier reads it and will not \
                 substitute zeroes for it"
            ),
            RulesGap::OutOfRange { field, value } => write!(
                formatter,
                "the rules table's `{field}` is {value}, which the verifier's signed comparison \
                 cannot hold"
            ),
        }
    }
}

impl std::error::Error for RulesGap {}

/// Every rules-table number the QUICK stages compare against.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Limits {
    size_budget_units: u32,
    handler_cooldown_min_ms: i32,
    max_fires_max: u32,
    notebook_max_chars: u32,
    reach_memory_ms: i32,
    decision_tick_ms: i32,
    map_size: [i32; 3],
    beacon_sphere_radius_voxels: i32,
}

impl Limits {
    /// Read the numbers out of a rules table.
    ///
    /// # Errors
    ///
    /// Returns [`RulesGap`] naming the first block the verifier reads that the
    /// table does not carry.
    pub fn from_rules(rules: &RulesTable) -> Result<Limits, RulesGap> {
        let message = rules.message();
        let verifier = message
            .verifier
            .as_ref()
            .ok_or(RulesGap::MissingBlock("verifier"))?;
        let map = message.map.as_ref().ok_or(RulesGap::MissingBlock("map"))?;
        let beacon = message
            .beacon
            .as_ref()
            .ok_or(RulesGap::MissingBlock("beacon"))?;
        let matched = message
            .r#match
            .as_ref()
            .ok_or(RulesGap::MissingBlock("match"))?;

        Ok(Limits {
            size_budget_units: verifier.size_budget_units,
            handler_cooldown_min_ms: verifier.handler_cooldown_min_ms,
            max_fires_max: verifier.max_fires_max,
            notebook_max_chars: verifier.notebook_max_chars,
            reach_memory_ms: verifier.reach_memory_ms,
            decision_tick_ms: matched.decision_tick_ms,
            map_size: [
                signed("map.size_x", map.size_x)?,
                signed("map.size_y", map.size_y)?,
                signed("map.size_z", map.size_z)?,
            ],
            beacon_sphere_radius_voxels: signed(
                "beacon.sphere_radius_voxels",
                beacon.sphere_radius_voxels,
            )?,
        })
    }

    /// The playbook size budget, in size units (`verifier.size_budget_units`).
    ///
    /// PLACEHOLDER: tuning, **owner**, at **S3's exit** — P1 sets the real
    /// number from the measured per-rule cost per decision tick (item 94). The
    /// row carries the same note; nothing here may hard-code a number beside it.
    #[must_use]
    pub const fn size_budget_units(&self) -> u32 {
        self.size_budget_units
    }

    /// The floor under a handler's cooldown, in game milliseconds
    /// (`verifier.handler_cooldown_min_ms`).
    #[must_use]
    pub const fn handler_cooldown_min_ms(&self) -> i32 {
        self.handler_cooldown_min_ms
    }

    /// The ceiling on a handler's `max_fires` (`verifier.max_fires_max`).
    #[must_use]
    pub const fn max_fires_max(&self) -> u32 {
        self.max_fires_max
    }

    /// The seat notebook's length in characters (`verifier.notebook_max_chars`).
    ///
    /// Read but not yet compared against: the notebook is gateway state rather
    /// than part of the playbook file, so `E0112` is a catalogue row with no
    /// emitter until T9 hands the text over.
    #[must_use]
    pub const fn notebook_max_chars(&self) -> u32 {
        self.notebook_max_chars
    }

    /// How long a seat remembers a seen thing, in game milliseconds
    /// (`verifier.reach_memory_ms`).
    ///
    /// The one reach window of spec section 10: Attack target selectors take
    /// their freshness from it rather than declaring their own. Read but not yet
    /// compared against — `W0703` waits for the sightings that would be stale.
    ///
    /// PLACEHOLDER: tuning, **owner**, at **S2/S3** (item 90).
    #[must_use]
    pub const fn reach_memory_ms(&self) -> i32 {
        self.reach_memory_ms
    }

    /// How often the interpreter takes a decision, in game milliseconds
    /// (`match.decision_tick_ms`).
    ///
    /// The verifier compares nothing against it. It is here because it is the
    /// denominator of gate P1: the per-rule cost **per decision tick** is what
    /// sets the size budget at S3's exit, so the number the budget is derived
    /// from is reachable from the same place the budget is.
    #[must_use]
    pub const fn decision_tick_ms(&self) -> i32 {
        self.decision_tick_ms
    }

    /// The map's extent in voxels, east, north and up (`map.size_x/y/z`).
    #[must_use]
    pub const fn map_size(&self) -> [i32; 3] {
        self.map_size
    }

    /// A beacon's sphere radius in voxels (`beacon.sphere_radius_voxels`).
    #[must_use]
    pub const fn beacon_sphere_radius_voxels(&self) -> i32 {
        self.beacon_sphere_radius_voxels
    }
}

/// The schema spells several extents `uint32`; the checks here are signed,
/// because a playbook may name a negative voxel and the comparison has to be
/// able to say so. The narrowing is a decision, so it gets one
/// (AGENTS.md section 4.3).
fn signed(field: &'static str, value: u32) -> Result<i32, RulesGap> {
    i32::try_from(value).map_err(|_| RulesGap::OutOfRange { field, value })
}
