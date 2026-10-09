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
//! rather than a default. proto3 JSON reads an absent scalar as zero, so a
//! scalar row whose zero means nothing -- the sphere radius, both costs, the
//! seam's size and the three interface times -- is refused at zero too. The
//! build drone's cost is not: a drone that costs nothing is a rule a table
//! may state. The by-richness rows are refused only when the whole row is
//! absent.
//!
//! `structures.beacon.draw_kw` is **not read**, and no goal charges a placed
//! beacon any draw: since T14b a beacon is net zero on the grid through its own
//! key-core (decisions-log items 90 and 113 (5); `crates/sim/src/power.rs`),
//! which S1's plan, decision 14 (ruled by item 128), made the reading of the
//! register's S1-14 rather than a guess.

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

    /// The grade a proto name (`gp.v1.ByRichness.Richness`) spells: `"LEAN"`,
    /// `"STANDARD"` or `"RICH"`.
    pub(crate) fn of(name: &str) -> Option<Richness> {
        match name {
            "LEAN" => Some(Richness::Lean),
            "STANDARD" => Some(Richness::Standard),
            "RICH" => Some(Richness::Rich),
            _ => None,
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
    /// What a tap goal spends, whole $: `structures.generator.cost_dollars`
    /// plus `units.build_drone.cost_dollars` ([`crate::candidates`]). The
    /// beacon may hold a build drone already, which the wire does not say, so
    /// the drone is counted: an upper bound, so a tap the treasury cannot
    /// carry is never composed.
    pub(crate) tap_cost_dollars: i64,
    /// What a Generator goal spends, whole $: the new beacon, its Generator,
    /// and the build drone it fabricates first because the Generator is its
    /// work and it has no drone -- the rules table's price for the route.
    pub(crate) vent_cost_dollars: i64,
    /// `power.generator_output_kw`, by richness.
    generator_output_kw: ByRichness,
    /// `economy.ore_yield_per_voxel_dollars`, by richness.
    ore_yield_per_voxel_dollars: ByRichness,
    /// `economy.seam_voxels`.
    pub(crate) seam_voxels: i64,
    /// `interface_times.edit_settings_base_ms`: a one-field settings row,
    /// which is what the core's dig depth is ([`crate::compose`]'s
    /// `deepen_core`).
    pub(crate) edit_settings_base_ms: i64,
    /// `interface_times.build_target_ms`: one Build target, its own row since
    /// S1's plan, decision 15 (decisions-log item 128 (3) (g)), whether it is
    /// a placed beacon's initial target or an `add_build_target` row.
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
    /// operator scores with is absent, or is zero where zero means "missing"
    /// (the rows the module docs name).
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
        let build_drone = table
            .units
            .and_then(|units| units.build_drone)
            .ok_or_else(|| missing("units.build_drone"))?;
        // Zero (or, for a time, below it) where zero means nothing: refused.
        for (row, value) in [
            (
                "beacon.sphere_radius_voxels",
                i64::from(beacon.sphere_radius_voxels),
            ),
            (
                "structures.beacon.cost_dollars",
                i64::from(beacon_row.cost_dollars),
            ),
            (
                "structures.generator.cost_dollars",
                i64::from(generator_row.cost_dollars),
            ),
            ("economy.seam_voxels", i64::from(economy.seam_voxels)),
            (
                "interface_times.edit_settings_base_ms",
                i64::from(times.edit_settings_base_ms),
            ),
            (
                "interface_times.place_beacon_deploy_ms",
                i64::from(times.place_beacon_deploy_ms),
            ),
            (
                "interface_times.build_target_ms",
                i64::from(times.build_target_ms),
            ),
        ] {
            if value <= 0 {
                return Err(missing(row));
            }
        }
        // The two route prices, summed once here: rows are `u32`, so the sums
        // fit an `i64`, and a sum that did not would be a table refused
        // rather than a price saturated.
        let too_dear = || {
            RulesError(String::from(
                "a route's price does not fit a whole $ figure",
            ))
        };
        let tap_cost_dollars = i64::from(generator_row.cost_dollars)
            .checked_add(i64::from(build_drone.cost_dollars))
            .ok_or_else(too_dear)?;
        let vent_cost_dollars = tap_cost_dollars
            .checked_add(i64::from(beacon_row.cost_dollars))
            .ok_or_else(too_dear)?;
        Ok(Tuning {
            tap_cost_dollars,
            vent_cost_dollars,
            sphere_radius_voxels: i64::from(beacon.sphere_radius_voxels),
            beacon_cost_dollars: i64::from(beacon_row.cost_dollars),
            generator_output_kw: power
                .generator_output_kw
                .ok_or_else(|| missing("power.generator_output_kw"))?,
            ore_yield_per_voxel_dollars: economy
                .ore_yield_per_voxel_dollars
                .ok_or_else(|| missing("economy.ore_yield_per_voxel_dollars"))?,
            seam_voxels: i64::from(economy.seam_voxels),
            edit_settings_base_ms: i64::from(times.edit_settings_base_ms),
            place_beacon_deploy_ms: i64::from(times.place_beacon_deploy_ms),
            build_target_ms: i64::from(times.build_target_ms),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn committed() -> String {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rules/rules.v1.json");
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn the_committed_rules_read() {
        assert!(Tuning::read(&committed()).is_ok());
    }

    #[test]
    fn a_row_whose_zero_means_nothing_is_refused_at_zero() {
        for (row, from, to) in [
            (
                "economy.seam_voxels",
                "\"seam_voxels\": 150",
                "\"seam_voxels\": 0",
            ),
            (
                "interface_times.edit_settings_base_ms",
                "\"edit_settings_base_ms\": 2000",
                "\"edit_settings_base_ms\": 0",
            ),
        ] {
            let text = committed();
            let zeroed = text.replacen(from, to, 1);
            assert_ne!(zeroed, text, "the rules text still carries `{from}`");
            let error = Tuning::read(&zeroed).unwrap_err();
            assert!(error.0.contains(row), "{error}");
        }
    }
}
