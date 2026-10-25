// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The bench rules variant (S2's plan, section 5 and E30): the committed rules
//! table with named overrides, built in memory, never written to disk and
//! never shipped, its own `rules_hash` printed in every report.
//!
//! Real play cannot reach the gate's load: a core's `power.core_surplus_kw`
//! powers ten units, so the 100 units and 13 beacons a seat fields at the gate
//! (decision 2) would brown out. The variant raises exactly the rows that stop
//! the gate's load from being powered, and nothing else:
//!
//! | Row | Shipped | Bench |
//! |---|---|---|
//! | `power.core_surplus_kw` | as committed | the gate's draw per seat plus `power.revive_margin_kw` |
//! | `power.map_ceiling_kw` | as committed | raised by the same amount per seat, so the generator's ceiling check (total supply at most the ceiling) still holds on the raised surplus |
//!
//! The fielding totals are world constants already at 300 units and 40
//! beacons (E30), so no row moves for them. Every other row, the weapons, the
//! collapse bounds, the repath cap and the broadphase cell among them, is the
//! committed value, so the figures are the shipped rules' figures under more
//! power.

use std::path::Path;

use pharmakos_sim::rules::RulesTable;

use crate::error::Error;

/// The gate's load per seat that the variant powers: what one seat fields at
/// the gate (decision 2), the generator's starting force included.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SeatLoad {
    /// Units the layout gives the seat, on top of its starting force.
    pub laid_out_units: u32,
    /// Beacons the layout gives the seat, on top of its core.
    pub laid_out_beacons: u32,
}

/// One named override, as the report prints it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Override {
    /// The row's path in the rules table.
    pub row: &'static str,
    /// The committed value.
    pub shipped: u32,
    /// The bench's value.
    pub bench: u32,
}

/// The variant and how it was made.
#[derive(Clone, Debug)]
pub struct BenchRules {
    /// The variant.
    pub rules: RulesTable,
    /// The committed table's `rules_hash`.
    pub shipped_hash: u64,
    /// The variant's `rules_hash`.
    pub bench_hash: u64,
    /// Every row the variant moved, in the order applied.
    pub overrides: Vec<Override>,
}

/// Build the variant from the committed table at `root`, powering `load` on
/// each of `seats` seats.
///
/// # Errors
///
/// [`Error::Rules`] when the committed table lacks a block the variant reads,
/// or the variant itself is refused at load; [`Error::Overflow`] when the draw
/// does not fit a `kW` row.
pub fn bench_rules(root: &Path, seats: u32, load: SeatLoad) -> Result<BenchRules, Error> {
    let shipped = crate::quick::committed_rules(root)?;
    let mut message = shipped.message().clone();
    let units = message
        .units
        .as_ref()
        .ok_or_else(|| Error::Rules("no `units` block".to_owned()))?;
    let starting = units
        .starting_build_drones
        .checked_add(units.starting_mining_drones)
        .ok_or(Error::Overflow("the starting force"))?;
    let power = message
        .power
        .as_mut()
        .ok_or_else(|| Error::Rules("no `power` block".to_owned()))?;
    // Every unit draws `kw_per_unit` and the commander nothing, and every
    // beacon `beacon_base_draw_kw`, the core among them (`crates/sim/src/
    // power.rs`, `crates/sim/src/rules.rs`'s `UnitRow::draw`); the margin is
    // the one a dark beacon needs to relight.
    let fielded = load
        .laid_out_units
        .checked_add(starting)
        .ok_or(Error::Overflow("the units a seat fields"))?;
    let beacons = load
        .laid_out_beacons
        .checked_add(1)
        .ok_or(Error::Overflow("the beacons a seat holds"))?;
    let draw = fielded
        .checked_mul(power.kw_per_unit)
        .and_then(|units| {
            beacons
                .checked_mul(power.beacon_base_draw_kw)
                .and_then(|beacons| units.checked_add(beacons))
        })
        .and_then(|draw| draw.checked_add(power.revive_margin_kw))
        .ok_or(Error::Overflow("the gate's draw per seat"))?;
    // No raise when the shipped surplus already covers the draw: the floor at
    // zero is the rule, not a fallback (tests/fallbacks.rs keeps it).
    let raise = draw.saturating_sub(power.core_surplus_kw);
    let surplus = Override {
        row: "power.core_surplus_kw",
        shipped: power.core_surplus_kw,
        bench: power.core_surplus_kw.max(draw),
    };
    let ceiling = Override {
        row: "power.map_ceiling_kw",
        shipped: power.map_ceiling_kw,
        bench: raise
            .checked_mul(seats)
            .and_then(|raised| power.map_ceiling_kw.checked_add(raised))
            .ok_or(Error::Overflow("the raised power ceiling"))?,
    };
    power.core_surplus_kw = surplus.bench;
    power.map_ceiling_kw = ceiling.bench;
    let rules = RulesTable::from_message(&message)
        .map_err(|error| Error::Rules(format!("the bench variant is refused at load: {error}")))?;
    Ok(BenchRules {
        shipped_hash: shipped.rules_hash(),
        bench_hash: rules.rules_hash(),
        rules,
        overrides: vec![surplus, ceiling],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_variant_moves_only_its_named_rows() {
        let root = crate::workspace_root();
        let load = SeatLoad {
            laid_out_units: 100,
            laid_out_beacons: 13,
        };
        let bench = bench_rules(&root, 3, load).expect("the variant");
        let shipped = crate::quick::committed_rules(&root).expect("the committed rules");
        assert_ne!(bench.bench_hash, bench.shipped_hash);
        assert_eq!(bench.shipped_hash, shipped.rules_hash());
        let mut expected = shipped.message().clone();
        let rows: Vec<&str> = bench.overrides.iter().map(|row| row.row).collect();
        assert_eq!(rows, ["power.core_surplus_kw", "power.map_ceiling_kw"]);
        let surplus = bench.overrides.first().expect("the surplus").bench;
        {
            let power = expected.power.as_mut().expect("a power block");
            power.core_surplus_kw = surplus;
            power.map_ceiling_kw = bench.overrides.get(1).expect("the ceiling").bench;
        }
        assert_eq!(bench.rules.message(), &expected, "nothing else moved");
        // 100 laid out and 3 starting at 1 kW, 14 beacons at 2 kW, and the
        // 2 kW margin, against the committed rows.
        assert_eq!(surplus, 103 + 28 + 2);
        let again = bench_rules(&root, 3, load).expect("the variant again");
        assert_eq!(
            again.bench_hash, bench.bench_hash,
            "a pure function of its inputs"
        );
    }
}
