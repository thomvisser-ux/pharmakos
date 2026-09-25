// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The tuning rows the operator scores with, read from the public rules text.
//!
//! Decisions-log item 111, decision C17 (hole H12): no method on the wire
//! carries a cost, a `kW` rating or a yield, so `gamectl` hands the operator
//! factory the same rules text it hands the host (`Setup.rules_json`, the
//! committed `rules/rules.v1.json` every client pins), and the operator reads
//! the rows it needs through [`pharmakos_proto::json`]. It is public data, the
//! same file a v1.1 agent would read, so this is no privileged read of the
//! match.
//!
//! **No tuning value is a constant here** (AGENTS.md section 12): every
//! number below comes from a row, and a row that is missing is an error
//! rather than a default.

use pharmakos_proto::gp::v1::{ByRichness, RulesTable};

/// A rules text the operator cannot score with.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RulesError(pub String);

impl std::fmt::Display for RulesError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for RulesError {}

/// Lean, standard, rich: the grade a vent or a seam comes in, as an index
/// into a [`ByRichness`] row.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Richness {
    /// Lean.
    Lean,
    /// Standard.
    Standard,
    /// Rich.
    Rich,
}

impl Richness {
    /// The lower-case name, for the "why" note.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Richness::Lean => "lean",
            Richness::Standard => "standard",
            Richness::Rich => "rich",
        }
    }

    fn pick(self, row: &ByRichness) -> u32 {
        match self {
            Richness::Lean => row.lean,
            Richness::Standard => row.standard,
            Richness::Rich => row.rich,
        }
    }
}

/// The rows the operator reads, as whole numbers.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Tuning {
    /// `beacon.sphere_radius_voxels`.
    pub(crate) sphere_radius_voxels: i64,
    /// `structures.beacon.cost_dollars`.
    pub(crate) beacon_cost_dollars: i64,
    /// `structures.beacon.draw_kw`.
    pub(crate) beacon_draw_kw: i64,
    /// `structures.generator.cost_dollars`.
    pub(crate) generator_cost_dollars: i64,
    /// `power.generator_output_kw`, by richness.
    generator_output_kw: ByRichness,
    /// `economy.ore_yield_per_voxel_dollars`, by richness.
    ore_yield_per_voxel_dollars: ByRichness,
    /// `economy.seam_voxels`.
    pub(crate) seam_voxels: i64,
    /// `interface_times.visit_handshake_ms`.
    pub(crate) visit_handshake_ms: i64,
    /// `interface_times.build_target_ms`.
    pub(crate) build_target_ms: i64,
    /// `interface_times.place_beacon_deploy_ms`.
    pub(crate) place_beacon_deploy_ms: i64,
}

impl Tuning {
    /// Read the rows from the rules text.
    ///
    /// # Errors
    ///
    /// [`RulesError`] when the text is not a `gp.v1.RulesTable` or a row the
    /// operator scores with is absent or zero where zero means "missing".
    pub(crate) fn read(rules_json: &str) -> Result<Tuning, RulesError> {
        let table: RulesTable = pharmakos_proto::json::decode(rules_json)
            .map_err(|error| RulesError(format!("the rules text does not read: {error}")))?;
        let missing = |row: &str| RulesError(format!("the rules text has no `{row}` row"));
        let beacon = table.beacon.ok_or_else(|| missing("beacon"))?;
        let structures = table.structures.ok_or_else(|| missing("structures"))?;
        let beacon_row = structures
            .beacon
            .ok_or_else(|| missing("structures.beacon"))?;
        let generator_row = structures
            .generator
            .ok_or_else(|| missing("structures.generator"))?;
        let power = table.power.ok_or_else(|| missing("power"))?;
        let economy = table.economy.ok_or_else(|| missing("economy"))?;
        let times = table
            .interface_times
            .ok_or_else(|| missing("interface_times"))?;
        if beacon.sphere_radius_voxels == 0 {
            return Err(missing("beacon.sphere_radius_voxels"));
        }
        Ok(Tuning {
            sphere_radius_voxels: i64::from(beacon.sphere_radius_voxels),
            beacon_cost_dollars: i64::from(beacon_row.cost_dollars),
            beacon_draw_kw: i64::from(beacon_row.draw_kw),
            generator_cost_dollars: i64::from(generator_row.cost_dollars),
            generator_output_kw: power
                .generator_output_kw
                .ok_or_else(|| missing("power.generator_output_kw"))?,
            ore_yield_per_voxel_dollars: economy
                .ore_yield_per_voxel_dollars
                .ok_or_else(|| missing("economy.ore_yield_per_voxel_dollars"))?,
            seam_voxels: i64::from(economy.seam_voxels),
            visit_handshake_ms: i64::from(times.visit_handshake_ms),
            build_target_ms: i64::from(times.build_target_ms),
            place_beacon_deploy_ms: i64::from(times.place_beacon_deploy_ms),
        })
    }

    /// `power.generator_output_kw` for one grade.
    pub(crate) fn generator_kw(&self, richness: Richness) -> i64 {
        i64::from(richness.pick(&self.generator_output_kw))
    }

    /// `economy.ore_yield_per_voxel_dollars` for one grade.
    pub(crate) fn ore_yield(&self, richness: Richness) -> i64 {
        i64::from(richness.pick(&self.ore_yield_per_voxel_dollars))
    }
}
