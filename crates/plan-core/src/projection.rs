// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `$` and `kW` projection the Quartermaster's rules imply — spec section
//! 11's third allowed estimate.
//!
//! > `$` and kW projection (Quartermaster arithmetic)
//!
//! and, from the same section's diagnostic families:
//!
//! > Because `$` leaves the treasury the moment an order commits, this family
//! > also warns where a playbook's committed spending — deploys, queued
//! > structures, ordered units — outruns the projected treasury
//!
//! So the projection walks the **route** and adds up what the playbook itself
//! commits: a `place_beacon` buys a beacon, and a `queue_structure` row buys a
//! structure. Each of those adds `kW` draw as well, which is what turns
//! "affordable" into "affordable *and* powered".
//!
//! # What it deliberately does not count
//!
//! * **Units.** A beacon's fabricator produces drones because its *mandate*
//!   has outstanding work (spec section 5, Fabricator), not because the
//!   playbook ordered them. The playbook never names a unit. Counting a guess
//!   at the mandate's unit mix would be modelling behaviour, which is the "no
//!   dry runs" line (AGENTS.md section 3 rule 2). **PLACEHOLDER:** ordered
//!   units enter the projection at **S1**, with the economy and the real
//!   Quartermaster, and that is where `E0601`, `W0601` and `W0602` get their
//!   emitters (decisions-log item 82).
//! * **Build targets.** A Build mandate's target list is work it does over the
//!   segment out of the treasury as it goes, not money that leaves at seal.
//!   Same stage, same reason.
//! * **Income.** Mining and salvage are the only income during a Push and both
//!   depend on what happens; BMI settles at the recap. A projection that added
//!   forecast income would be a claim about the future rather than arithmetic
//!   over the file.
//!
//! Everything it does count is a row in `gp.v1.RulesTable`, never a constant.

use pharmakos_proto::gp::v1::{Playbook, RulesTable as RulesMessage, Step, interface_row, step};
use pharmakos_sim::math::quantity::{Kw, Money};
use pharmakos_sim::rules::RulesTable;

use crate::context::PlanContext;
use crate::error::Error;

/// What the playbook commits, and what the seat is left with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Projection {
    /// What the route spends the moment each order commits, in `$`.
    pub spend: Money,
    /// The treasury once all of it has left, in `$`. Negative means the
    /// playbook has ordered more than the seat can pay for.
    pub treasury_after: Money,
    /// The `kW` the route adds to the grid's draw.
    pub draw_added: Kw,
    /// Supply minus draw once the route has built everything, in `kW`.
    /// Negative means a shortfall, which is an error unless the playbook sets
    /// `allow_dormant_beacons` (spec section 11).
    pub headroom_after: Kw,
}

impl Projection {
    /// True when the treasury cannot cover what the route orders.
    #[must_use]
    pub const fn overspends(&self) -> bool {
        self.treasury_after.raw() < 0
    }

    /// True when the grid cannot carry what the route builds.
    #[must_use]
    pub const fn is_short_of_power(&self) -> bool {
        self.headroom_after.raw() < 0
    }
}

/// Projects a playbook against a seat's frozen snapshot.
///
/// # Errors
///
/// When the rules table carries no `structures` block, or when the arithmetic
/// would overflow — both reported rather than saturated, because a treasury
/// that silently clamps is a wrong answer in the reassuring direction
/// (AGENTS.md section 4.1).
pub fn project(
    playbook: &Playbook,
    context: &PlanContext,
    rules: &RulesTable,
) -> Result<Projection, Error> {
    let message = rules.message();
    let mut spend: i64 = 0;
    let mut draw: i64 = 0;

    let beacon = structure_row(message, "beacon")?;
    if let Some(body) = playbook.declarative.as_ref() {
        for entry in &body.route {
            let (cost, kilowatts) = step_cost(entry, message, beacon)?;
            spend = spend.saturating_add(cost);
            draw = draw.saturating_add(kilowatts);
        }
    }

    let spend = Money::new(spend);
    let draw_added =
        Kw::new(i32::try_from(draw).map_err(|_ignored| {
            Error::at("", "the route's added draw does not fit in a kW figure")
        })?);
    let treasury_after = context
        .treasury()
        .checked_sub(spend)
        .ok_or_else(|| Error::at("", "the projected treasury does not fit in a $ figure"))?;
    let headroom_after = context
        .headroom()
        .and_then(|headroom| headroom.checked_sub(draw_added))
        .ok_or_else(|| Error::at("", "the projected headroom does not fit in a kW figure"))?;

    Ok(Projection {
        spend,
        treasury_after,
        draw_added,
        headroom_after,
    })
}

/// What one route step commits, as `($, kW)`.
fn step_cost(
    entry: &Step,
    message: &RulesMessage,
    beacon: (i64, i64),
) -> Result<(i64, i64), Error> {
    match entry.kind.as_ref() {
        Some(step::Kind::PlaceBeacon(_)) => Ok(beacon),
        Some(step::Kind::Interface(visit)) => {
            let mut cost = 0i64;
            let mut draw = 0i64;
            for row in &visit.rows {
                if let Some(interface_row::Row::QueueStructure(queue)) = row.row.as_ref() {
                    let (structure_cost, structure_draw) =
                        structure_row(message, &queue.blueprint_id)?;
                    cost = cost.saturating_add(structure_cost);
                    draw = draw.saturating_add(structure_draw);
                }
            }
            Ok((cost, draw))
        }
        _ => Ok((0, 0)),
    }
}

/// A structure's `(cost_dollars, draw_kw)` from the rules table.
///
/// **PLACEHOLDER — the blueprint catalogue.** `QueueStructureRow.blueprint_id`
/// and `BuildTarget.blueprint_id` are strings until spec section 8's
/// capability contract is written, and `proto/gp/v1/playbook.proto` says so at
/// both fields. The ids below are the `gp.v1.RulesTable.Structures` field
/// names, which is the one spelling the project already uses — the verifier's
/// own fixtures write `"blueprint_id": "generator"`. The **owner** fixes the
/// catalogue at **S4**, with licences; when it becomes an enum this function
/// becomes a match on it and the strings go away.
fn structure_row(message: &RulesMessage, blueprint_id: &str) -> Result<(i64, i64), Error> {
    let structures = message.structures.as_ref().ok_or_else(|| {
        Error::at(
            "",
            "the rules table carries no `structures` block, so nothing can be priced",
        )
    })?;
    let row = match blueprint_id {
        "beacon" => structures.beacon.as_ref(),
        "generator" => structures.generator.as_ref(),
        "autocannon" => structures.autocannon.as_ref(),
        "mortar" => structures.mortar.as_ref(),
        "survey_post" => structures.survey_post.as_ref(),
        "resonance_spire" => structures.resonance_spire.as_ref(),
        other => {
            return Err(Error::at(
                "",
                format!("`{other}` is not a blueprint the rules table prices"),
            ));
        }
    };
    let row = row.ok_or_else(|| {
        Error::at(
            "",
            format!("the rules table has no row for the `{blueprint_id}` blueprint"),
        )
    })?;
    Ok((i64::from(row.cost_dollars), i64::from(row.draw_kw)))
}

#[cfg(test)]
mod tests {
    use super::project;
    use crate::context::PlanContext;
    use pharmakos_proto::gp::v1::{
        Declarative, InterfaceRow, InterfaceStep, PlaceBeaconStep, Playbook, QueueStructureRow,
        Step, interface_row, step,
    };
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

    fn playbook(route: Vec<Step>) -> Playbook {
        Playbook {
            declarative: Some(Declarative {
                route,
                ..Declarative::default()
            }),
            ..Playbook::default()
        }
    }

    fn place() -> Step {
        Step {
            kind: Some(step::Kind::PlaceBeacon(PlaceBeaconStep::default())),
            ..Step::default()
        }
    }

    fn queue(blueprint: &str) -> Step {
        Step {
            kind: Some(step::Kind::Interface(InterfaceStep {
                beacon: None,
                rows: vec![InterfaceRow {
                    row: Some(interface_row::Row::QueueStructure(QueueStructureRow {
                        blueprint_id: blueprint.to_owned(),
                    })),
                }],
            })),
            ..Step::default()
        }
    }

    #[test]
    fn a_deploy_costs_a_beacon_and_adds_its_draw() {
        let projection = project(&playbook(vec![place()]), &context(), &rules()).expect("priced");
        assert_eq!(projection.spend, Money::new(60));
        assert_eq!(projection.treasury_after, Money::new(140));
        assert_eq!(projection.draw_added, Kw::new(2));
        assert_eq!(projection.headroom_after, Kw::new(6));
        assert!(!projection.overspends());
        assert!(!projection.is_short_of_power());
    }

    #[test]
    fn a_queued_generator_costs_its_row_and_draws_nothing() {
        let projection =
            project(&playbook(vec![queue("generator")]), &context(), &rules()).expect("priced");
        assert_eq!(projection.spend, Money::new(80));
        assert_eq!(projection.draw_added, Kw::new(0));
    }

    #[test]
    fn overspending_and_a_shortfall_are_both_reported() {
        let route = vec![place(), place(), place(), place(), place()];
        let projection = project(&playbook(route), &context(), &rules()).expect("priced");
        assert_eq!(projection.spend, Money::new(300));
        assert!(projection.overspends());
        assert!(projection.is_short_of_power());
    }

    #[test]
    fn an_unknown_blueprint_is_an_error_rather_than_free() {
        assert!(project(&playbook(vec![queue("death_ray")]), &context(), &rules()).is_err());
    }

    #[test]
    fn a_route_that_orders_nothing_spends_nothing() {
        let projection = project(&playbook(Vec::new()), &context(), &rules()).expect("priced");
        assert_eq!(projection.spend, Money::new(0));
        assert_eq!(projection.treasury_after, Money::new(200));
    }
}
