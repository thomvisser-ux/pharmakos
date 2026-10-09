// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `$` and `kW` projection the Quartermaster's rules imply, against the
//! seat's frozen snapshot.
//!
//! The arithmetic is `pharmakos_verifier::projection`'s. It lived here until
//! S1, when the verifier's estimate stage needed it and this crate's
//! dependency on the verifier made the call a cycle, so it moved **down** into
//! the verifier and this module re-exports it (S1's plan, task `proj`). What
//! stays here is the one thing the verifier does not have: a [`PlanContext`],
//! the snapshot read for one seat, which [`project`] hands across as the
//! seat's economy. One price list, one projection, whichever caller asks.

pub use pharmakos_verifier::projection::{Cost, Prices, Projection, ProjectionError, step_cost};

use pharmakos_proto::gp::v1::Playbook;
use pharmakos_sim::rules::RulesTable;

use crate::context::PlanContext;
use crate::error::Error;

/// Projects a playbook's route against a seat's frozen snapshot.
///
/// # Errors
///
/// When the rules table does not carry a price the projection reads, or the
/// verifier's projection refuses the route (a blueprint the table does not
/// price, or a sum that leaves its type).
pub fn project(
    playbook: &Playbook,
    context: &PlanContext,
    rules: &RulesTable,
) -> Result<Projection, Error> {
    let prices = Prices::from_rules(rules)?;
    Ok(pharmakos_verifier::projection::project(
        playbook,
        context.economy(),
        &prices,
    )?)
}

#[cfg(test)]
mod tests {
    use super::project;
    use crate::context::PlanContext;
    use pharmakos_proto::gp::v1::{Declarative, PlaceBeaconStep, Playbook, Step, step};
    use pharmakos_sim::math::quantity::{Kw, Money};
    use pharmakos_sim::rules::RulesTable;
    use pharmakos_sim::snapshot::Snapshot;

    fn rules() -> RulesTable {
        RulesTable::load(std::path::Path::new("../../rules/rules.v1.json"))
            .expect("the committed rules table")
    }

    fn context() -> PlanContext {
        let snapshot = Snapshot {
            seat_id: vec![0],
            seat_treasury: vec![200],
            seat_supply: vec![10],
            seat_draw: vec![2],
            ..Snapshot::default()
        };
        PlanContext::from_snapshot(&snapshot, 0).expect("seat 0")
    }

    fn deploys(count: usize) -> Playbook {
        let place = Step {
            kind: Some(step::Kind::PlaceBeacon(PlaceBeaconStep::default())),
            ..Step::default()
        };
        Playbook {
            declarative: Some(Declarative {
                route: vec![place; count],
                ..Declarative::default()
            }),
            ..Playbook::default()
        }
    }

    /// The snapshot's seat row is what the verifier's projection runs
    /// against: five deploys at `$` 60 outrun a `$` 200 treasury, and a
    /// beacon adds no draw (a key-core nets out its own base).
    #[test]
    fn the_snapshot_is_the_economy_the_projection_reads() {
        let projection = project(&deploys(5), &context(), &rules()).expect("priced");
        assert_eq!(projection.spend, Money::new(300));
        assert_eq!(projection.treasury_after, Money::new(-100));
        assert!(projection.overspends());
        assert_eq!(projection.headroom_after, Kw::new(8));
        assert!(!projection.is_short_of_power());
    }

    #[test]
    fn a_missing_price_is_an_error_rather_than_free() {
        // The deploy's price, `structures.beacon`: a table without the whole
        // `structures` block no longer loads at all, since the sim refuses at
        // load a table its power phase cannot read (decisions-log item 135
        // (2) (b)), so the gap is the one row the projection alone reads.
        let mut message = rules().message().clone();
        if let Some(block) = message.structures.as_mut() {
            block.beacon = None;
        }
        let gapped = RulesTable::from_message(&message).expect("the sim's view still builds");
        let error = project(&deploys(1), &context(), &gapped).expect_err("nothing is priced");
        assert!(error.message.contains("structures"), "{error}");
    }
}
