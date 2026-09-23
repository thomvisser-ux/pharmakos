// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The rules-table rows the mesher needs, marshalled into the mesher's parameter types.
//!
//! The mesher cannot read `rules/rules.v1.json` itself: that would need
//! `pharmakos_sim::rules::RulesTable`, and a dependency edge from a walled crate into the
//! determinism crate is the edge `wall-guard` exists to forbid. So item 92 gives the job
//! to this crate — "the rules-table values the mesher needs (K, B, the age term,
//! `LIGHT_MAX`, `LIGHT_ATTEN`) cross the wall as integer parameters on the call, never as
//! constants in the mesher and never through a sim dependency."
//!
//! The bridge reads them the one way a thin client may: it decodes the canonical `gp.v1`
//! JSON of a [`RulesTable`] with `pharmakos-proto`'s codec and copies five fields out.
//! It holds **no constant of its own** — a table without a `mesher` row is an error, not
//! a default (AGENTS.md section 12: "tuning values are data ... not constants sprinkled
//! through the code"). There is no arithmetic here beyond the range checks a `u32` needs
//! to become a `u8`, and there is none at all on money, power or time.

use pharmakos_mesher::{DrainBudget, LightParams};
use pharmakos_proto::gp::v1::RulesTable;
use pharmakos_proto::json;

use crate::error::BridgeError;

/// The mesher's five parameters, taken from one rules table.
///
/// Held together rather than passed as five numbers so that a caller cannot fill the
/// drain budget from one revision of the table and the light from another.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MesherRules {
    /// The table revision these came from, so a diagnostic can name it.
    pub revision: u32,
    /// K, B and the ageing term — decisions log item 54.
    pub budget: DrainBudget,
    /// `LIGHT_MAX` and `LIGHT_ATTEN`, from which the mesher derives its dirty pad.
    pub light: LightParams,
}

/// Reads a rules table from its canonical `gp.v1` JSON text.
///
/// Strict, because the codec is: an unknown field is rejected with a JSON Pointer rather
/// than stripped, which is the whole reason the bridge decodes through the schema instead
/// of reaching into the text itself.
///
/// # Errors
///
/// [`BridgeError::Schema`] when the text is not a valid `gp.v1.RulesTable`.
pub fn table_from_json(text: &str) -> Result<RulesTable, BridgeError> {
    Ok(json::decode::<RulesTable>(text)?)
}

/// The five mesher parameters out of a decoded table.
///
/// # Errors
///
/// [`BridgeError::MissingRules`] when the table has no `mesher` row, and
/// [`BridgeError::RulesOutOfRange`] when a value does not fit the parameter it fills —
/// a light value above 255, or a `light_atten` of zero, which the mesher refuses because
/// a fall of nothing per voxel makes the light reach unbounded.
pub fn mesher_rules(table: &RulesTable) -> Result<MesherRules, BridgeError> {
    let row = table
        .mesher
        .as_ref()
        .ok_or(BridgeError::MissingRules { row: "mesher" })?;

    let budget = DrainBudget {
        surfaces_per_frame: row.surfaces_per_frame,
        bytes_per_frame: u64::from(row.bytes_per_frame),
        age_frames: u64::from(row.age_frames),
    };

    let light_max = light_byte("light_max", row.light_max)?;
    let light_atten = light_byte("light_atten", row.light_atten)?;
    // The mesher's own boundary decides whether the pair is legal; the bridge does not
    // second-guess it, and does not carry a copy of the rule.
    let light =
        LightParams::new(light_max, light_atten).map_err(|_| BridgeError::RulesOutOfRange {
            field: "light_max/light_atten",
            value: row.light_atten,
            because: "the mesher refuses the pair: light_max must be non-zero and light_atten \
                  must be between 1 and light_max, or the light reach is unbounded",
        })?;

    Ok(MesherRules {
        revision: table.revision,
        budget,
        light,
    })
}

/// The Lull's planning timer, `match.lull_ms`, in milliseconds.
///
/// The rules table's own comment says the row is "read by the host and the editor, never
/// by the sim", and the gateway's `_status` footer shows the countdown the client reports
/// rather than one of its own (skeleton-plan-t16a-notes.md section D: "the client counts,
/// the gateway is told"), so the watch rig takes the Lull's length from here. `None` when
/// the table carries no `match` block or a length of zero or less, which the rig treats
/// as an untimed Lull rather than inventing a default.
#[must_use]
pub fn lull_ms(table: &RulesTable) -> Option<u32> {
    table
        .r#match
        .as_ref()
        .and_then(|row| u32::try_from(row.lull_ms).ok())
        .filter(|length| *length > 0)
}

/// A light value narrowed from the table's `uint32` to the byte the mesher stores.
fn light_byte(field: &'static str, value: u32) -> Result<u8, BridgeError> {
    u8::try_from(value).map_err(|_| BridgeError::RulesOutOfRange {
        field,
        value,
        because: "a light value is one byte; the bake stores it per voxel",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pharmakos_proto::gp::v1::rules_table::Mesher;

    fn table_with(row: Option<Mesher>) -> RulesTable {
        RulesTable {
            revision: 7,
            mesher: row,
            ..RulesTable::default()
        }
    }

    fn row() -> Mesher {
        Mesher {
            surfaces_per_frame: 4,
            bytes_per_frame: 524_288,
            age_frames: 2,
            light_max: 15,
            light_atten: 1,
        }
    }

    #[test]
    fn the_five_values_arrive_unchanged() {
        let rules = mesher_rules(&table_with(Some(row()))).expect("a complete table");
        assert_eq!(rules.revision, 7);
        assert_eq!(rules.budget.surfaces_per_frame, 4);
        assert_eq!(rules.budget.bytes_per_frame, 524_288);
        assert_eq!(rules.budget.age_frames, 2);
        assert_eq!(rules.light.light_max(), 15);
        assert_eq!(rules.light.light_atten(), 1);
    }

    #[test]
    fn a_table_without_a_mesher_row_is_an_error_and_not_a_default() {
        let error = mesher_rules(&table_with(None)).expect_err("no row, no defaults");
        assert_eq!(error, BridgeError::MissingRules { row: "mesher" });
    }

    #[test]
    fn a_light_value_that_is_not_a_byte_is_refused() {
        let mut bad = row();
        bad.light_max = 300;
        let error = mesher_rules(&table_with(Some(bad))).expect_err("300 is not a byte");
        assert!(
            matches!(
                error,
                BridgeError::RulesOutOfRange {
                    field: "light_max",
                    value: 300,
                    ..
                }
            ),
            "{error}"
        );
    }

    #[test]
    fn a_zero_attenuation_is_refused_by_the_meshers_own_boundary() {
        let mut bad = row();
        bad.light_atten = 0;
        let error = mesher_rules(&table_with(Some(bad))).expect_err("zero fall per voxel");
        assert!(
            matches!(error, BridgeError::RulesOutOfRange { .. }),
            "{error}"
        );
    }

    /// The committed table is what ships, so the bridge is tested against it rather than
    /// against a fixture that could drift from it. The values are item 54's.
    #[test]
    fn the_committed_rules_table_carries_item_54s_budget() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("rules")
            .join("rules.v1.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
        let table = table_from_json(&text).expect("the committed table is canonical gp.v1 JSON");
        let rules = mesher_rules(&table).expect("the committed table has a mesher row");
        assert_eq!(rules.budget.surfaces_per_frame, 4, "K (item 54)");
        assert_eq!(rules.budget.bytes_per_frame, 512 * 1024, "B (item 54)");
        assert_eq!(rules.budget.age_frames, 2, "the ageing term (item 54)");
        assert_eq!(rules.light.light_max(), 15, "the light ceiling (item 54)");
        assert_eq!(rules.light.light_atten(), 1, "the fall per voxel (item 54)");
    }

    #[test]
    fn a_text_the_schema_rejects_comes_back_with_a_pointer() {
        let error = table_from_json("{\"revision\": 1, \"not_a_field\": 2}")
            .expect_err("an unknown field is rejected, never stripped");
        assert!(matches!(error, BridgeError::Schema { .. }), "{error}");
    }
}
