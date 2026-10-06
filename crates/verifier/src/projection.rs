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
//! orders, step by step, at the rules table's prices and the sim's own draw
//! rules. It moved here from `plan-core` with the interface arithmetic, so the
//! estimate stage can read it (S1's plan, task `proj`); `plan-core` re-exports
//! it.
//!
//! # What it counts (S1-47)
//!
//! | Order | `$` | `kW` |
//! |---|---|---|
//! | A `place_beacon` | `structures.beacon.cost_dollars`, charged when the deploy starts (X-12) | none: a beacon is net zero through its own key-core (`pharmakos_sim::power`) |
//! | A Build target, in a deploy's `initial`, a `set_mandate` or `set_mandate_settings` row, or an `add_build_target` row | its blueprint's `cost_dollars` | the blueprint's draw, by the sim's own rule (a Generator and an autocannon draw nothing) |
//! | A queued capability structure | its blueprint's `cost_dollars` | its draw, as above |
//! | A Survey mandate's `scout_count` | that many `units.scout.cost_dollars` | that many `power.kw_per_unit` |
//!
//! Scouts are the one unit a playbook **orders**: spec section 6's Survey row
//! makes the count a setting, and the sim's Survey mandate fields scouts until
//! the beacon has that many. The sim stores the count in a `u8` and clamps a
//! larger one to 255 when the row commits (`set_beacon_scouts`); the projection
//! counts the number the author wrote, in full, because that is what the
//! playbook orders, and a count past the sim's column is past every supply the
//! rules table can give a seat anyway.
//!
//! # How sure it is: what an order adds, and what it may add
//!
//! The view carries no unit roster and no beacon's settings, so the projection
//! cannot always tell how much of an order is **new**. It sorts every order
//! into one of two piles ([`Orders`]):
//!
//! * **Adds**: draw and spend the route certainly orders. A deploy and
//!   everything in its `initial` (the beacon is new, so nothing fields yet); an
//!   `add_build_target` and a `queue_structure` row; and a `set_mandate` row
//!   that switches a beacon the view names by id to a writ the view says it is
//!   not on.
//! * **May add, at most**: an edit of a beacon that may already field what it
//!   orders. A `set_mandate_settings` row, which edits within the current writ
//!   (the sim fills a Survey count rather than adding to it, and keeps a Build
//!   target's structure when an edit re-lists its anchor), and a `set_mandate`
//!   row on a beacon the view does not name, or names on the same writ. These
//!   are counted in full, as an upper bound.
//!
//! The estimate stage tests the first pile alone against `E0601`, spec section
//! 11's one FULL error, so the error is never raised on draw the route may not
//! add; and the two piles together against the warnings (`W0604` for power,
//! `W0602` for money), so an upper bound errs towards a warning and never
//! towards silence. [`project`] and [`step_cost`], which `plan-core` re-exports,
//! sum both piles: they are the upper bound.
//!
//! A Build target is not money that leaves at the seal — the mandate builds
//! "the highest-order affordable target" over the segment, and an unaffordable
//! one waits — but it is spending the playbook commits the seat to, which is
//! what the warning is about (the register's S1-47).
//!
//! # What it deliberately does not count
//!
//! * **Drones the fabricator makes for work in hand.** A Build or Mine beacon
//!   fields drones because its mandate has outstanding work (spec section 5,
//!   Fabricator; decisions-log item 22), not because the playbook ordered
//!   them. Counting a guess at that would be modelling the mandate, which is
//!   the "no dry runs" line (AGENTS.md section 3 rule 2).
//! * **Income and supply to come.** Mining, salvage and a Generator's output
//!   all depend on what happens; BMI settles at the recap. A projection that
//!   credited them would be a claim about the future rather than arithmetic
//!   over the file, so a Generator the route builds adds no supply here, and a
//!   recycle refunds nothing (its refund follows the beacon's HP at the
//!   time).
//! * **Handler bodies, `on_death` and the fallback.** A handler fires at most
//!   `max_fires` times and may not fire at all; counting it would be the same
//!   claim about the future.
//!
//! Every price is a row in `gp.v1.RulesTable`, never a constant.

use pharmakos_proto::gp::v1::beacon_filter::MandateKind;
use pharmakos_proto::gp::v1::{
    BuildTarget, InterfaceStep, MandateSettings, Playbook, Step, beacon_ref, interface_row,
    mandate_settings, step,
};
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::math::quantity::{Kw, Money};
use pharmakos_sim::power::PowerRules;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::tables::StructureKind;

use crate::limits::RulesGap;
use crate::scope::Scope;

/// The blueprints the rules table prices, by the id a playbook writes.
///
/// `QueueStructureRow.blueprint_id` and `BuildTarget.blueprint_id` are strings
/// until spec section 8's capability contract is written, and
/// `proto/gp/v1/playbook.proto` says so at both fields. The ids are the
/// `gp.v1.RulesTable.Structures` field names, which is the one spelling the
/// project already uses. When the catalogue becomes an enum this table becomes
/// a match on it and the strings go away.
///
/// PLACEHOLDER: the blueprint catalogue as a typed list rather than strings — owner, S4, with licences.
const BLUEPRINTS: [(&str, StructureKind); 5] = [
    ("generator", StructureKind::Generator),
    ("autocannon", StructureKind::Autocannon),
    ("mortar", StructureKind::Mortar),
    ("survey_post", StructureKind::SurveyPost),
    ("resonance_spire", StructureKind::ResonanceSpire),
];

/// What one order costs: `$` out of the treasury and `kW` onto the grid.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cost {
    /// `$`, charged when the order commits.
    pub spend: Money,
    /// `kW` the order adds to the grid's draw.
    pub draw: Kw,
}

impl Cost {
    /// Nothing.
    pub const ZERO: Cost = Cost {
        spend: Money::ZERO,
        draw: Kw::ZERO,
    };

    /// Two costs added.
    ///
    /// # Errors
    ///
    /// [`ProjectionError::Overflow`] when either sum leaves its type.
    pub fn plus(self, other: Cost) -> Result<Cost, ProjectionError> {
        Ok(Cost {
            spend: self
                .spend
                .checked_add(other.spend)
                .ok_or(ProjectionError::Overflow)?,
            draw: self
                .draw
                .checked_add(other.draw)
                .ok_or(ProjectionError::Overflow)?,
        })
    }
}

/// A `$` and `kW` total over a route, wide enough that no playbook can leave
/// it.
///
/// Every term is a price the rules table holds as an unsigned 32-bit row,
/// widened, times a count a playbook holds as an unsigned 32-bit field: below
/// 2^64 apiece. An `i128` sum of such terms cannot leave its type before the
/// route has 2^63 of them, which no playbook the decoder accepts comes near, so
/// a figure here is the exact sum of what the playbook wrote, whatever it
/// wrote. That is the difference from [`Cost`], whose `i64` and `i32` a single
/// hostile `scout_count` can leave, and why the estimate stage tests the route
/// in this type: an order too large for [`Cost`] is past every supply, and has
/// to say so rather than fall silent.
///
/// The additions saturate, which by the argument above never happens; were it
/// to, every term is non-negative, so a saturated figure would still exceed
/// every room and treasury it is compared with, and the comparison would still
/// come out the way the exact sum would.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Tally {
    /// `$`.
    pub spend: i128,
    /// `kW`.
    pub draw: i128,
}

impl Tally {
    /// Nothing.
    pub const ZERO: Tally = Tally { spend: 0, draw: 0 };

    /// One order at `price`.
    #[must_use]
    pub fn of(price: Cost) -> Tally {
        Tally {
            spend: i128::from(price.spend.raw()),
            draw: i128::from(price.draw.raw()),
        }
    }

    /// `count` orders at `price`, exactly.
    #[must_use]
    pub fn times(price: Cost, count: u32) -> Tally {
        let count = i128::from(count);
        Tally {
            // |i64| x u32 and |i32| x u32 are both below 2^96: the products
            // are exact in `i128`, and saturation (see the type's doc) is
            // never reached.
            spend: i128::from(price.spend.raw()).saturating_mul(count),
            draw: i128::from(price.draw.raw()).saturating_mul(count),
        }
    }

    /// Two tallies added (see the type's doc for why this saturates).
    #[must_use]
    pub fn plus(self, other: Tally) -> Tally {
        Tally {
            spend: self.spend.saturating_add(other.spend),
            draw: self.draw.saturating_add(other.draw),
        }
    }

    /// The tally as a [`Cost`].
    ///
    /// # Errors
    ///
    /// [`ProjectionError::Overflow`] when either figure leaves the sim's `$`
    /// or `kW` type.
    pub fn narrow(self) -> Result<Cost, ProjectionError> {
        Ok(Cost {
            spend: Money::new(i64::try_from(self.spend).map_err(|_| ProjectionError::Overflow)?),
            draw: Kw::new(i32::try_from(self.draw).map_err(|_| ProjectionError::Overflow)?),
        })
    }
}

/// What one route step orders, sorted by how sure the projection is that the
/// order is new (see the module doc, "How sure it is").
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Orders {
    /// What the step certainly adds.
    pub adds: Tally,
    /// What the step may add, counted in full: an upper bound.
    pub at_most: Tally,
}

impl Orders {
    /// Both piles, as one upper bound.
    #[must_use]
    pub fn upper(self) -> Tally {
        self.adds.plus(self.at_most)
    }
}

/// Why a playbook could not be priced.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ProjectionError {
    /// A Build target or a queued structure names a blueprint the rules table
    /// does not price. The sim refuses it at the seal's compile (only the
    /// Generator is buildable before S4), and `E0506` is the verifier code
    /// that will name it once the capability catalogue exists (S4).
    UnknownBlueprint(String),
    /// A figure left the sim's `$` or `kW` type. One `scout_count` near the
    /// top of its `u32` is enough, and costs one size unit, so a playbook
    /// inside the size budget can reach it: [`project`] and [`step_cost`]
    /// refuse it rather than wrap. The estimate stage never meets it, because
    /// it sums in [`Tally`].
    Overflow,
}

impl core::fmt::Display for ProjectionError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ProjectionError::UnknownBlueprint(id) => {
                write!(
                    formatter,
                    "`{id}` is not a blueprint the rules table prices"
                )
            }
            ProjectionError::Overflow => {
                write!(
                    formatter,
                    "the projection does not fit in a `$` or `kW` figure"
                )
            }
        }
    }
}

impl std::error::Error for ProjectionError {}

/// The price list, read out of the rules table once.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Prices {
    beacon: Cost,
    scout: Cost,
    structures: [Cost; 5],
}

impl Prices {
    /// Reads the prices.
    ///
    /// # Errors
    ///
    /// [`RulesGap::MissingBlock`] when the table carries no `structures`,
    /// `units` or `power` block, or no row for the beacon, the scout or a
    /// blueprint: a price the table does not carry is never zero.
    pub fn from_rules(rules: &RulesTable) -> Result<Prices, RulesGap> {
        let message = rules.message();
        let structures = message
            .structures
            .as_ref()
            .ok_or(RulesGap::MissingBlock("structures"))?;
        let units = message
            .units
            .as_ref()
            .ok_or(RulesGap::MissingBlock("units"))?;
        // The block must be there (a draw the table does not carry is never
        // zero), but every `kW` row in it is read through the sim.
        message
            .power
            .as_ref()
            .ok_or(RulesGap::MissingBlock("power"))?;
        // The sim's own reading of every `kW` row, so "what draws" has one
        // home: a Generator and an autocannon draw nothing, whatever their
        // rows would say, and a row past the sim's signed `kW` reads as the sim
        // reads it.
        let draws = PowerRules::of(rules);
        let per_unit = Kw::new(draws.per_unit);
        let beacon = structures
            .beacon
            .as_ref()
            .ok_or(RulesGap::MissingBlock("structures.beacon"))?;
        let scout = units
            .scout
            .as_ref()
            .ok_or(RulesGap::MissingBlock("units.scout"))?;
        let row = |kind: StructureKind| -> Result<Cost, RulesGap> {
            let (found, field) = match kind {
                StructureKind::Generator => (structures.generator.as_ref(), "structures.generator"),
                StructureKind::Autocannon => {
                    (structures.autocannon.as_ref(), "structures.autocannon")
                }
                StructureKind::Mortar => (structures.mortar.as_ref(), "structures.mortar"),
                StructureKind::SurveyPost => {
                    (structures.survey_post.as_ref(), "structures.survey_post")
                }
                StructureKind::ResonanceSpire => (
                    structures.resonance_spire.as_ref(),
                    "structures.resonance_spire",
                ),
                // A wall is priced per voxel and is no blueprint a playbook
                // names; `BLUEPRINTS` never asks for it.
                StructureKind::Wall => (None, "structures.wall_cost_per_voxel"),
            };
            let found = found.ok_or(RulesGap::MissingBlock(field))?;
            Ok(Cost {
                spend: Money::new(i64::from(found.cost_dollars)),
                draw: Kw::new(draws.structure_draw(kind)),
            })
        };
        Ok(Prices {
            beacon: Cost {
                spend: Money::new(i64::from(beacon.cost_dollars)),
                draw: Kw::ZERO,
            },
            scout: Cost {
                spend: Money::new(i64::from(scout.cost_dollars)),
                draw: per_unit,
            },
            structures: [
                row(StructureKind::Generator)?,
                row(StructureKind::Autocannon)?,
                row(StructureKind::Mortar)?,
                row(StructureKind::SurveyPost)?,
                row(StructureKind::ResonanceSpire)?,
            ],
        })
    }

    /// A placed beacon: its price, and no draw.
    #[must_use]
    pub const fn beacon(&self) -> Cost {
        self.beacon
    }

    /// One scout.
    #[must_use]
    pub const fn scout(&self) -> Cost {
        self.scout
    }

    /// One structure of the blueprint a playbook names.
    ///
    /// # Errors
    ///
    /// [`ProjectionError::UnknownBlueprint`] for an id the table does not
    /// price.
    pub fn blueprint(&self, blueprint_id: &str) -> Result<Cost, ProjectionError> {
        BLUEPRINTS
            .iter()
            .zip(self.structures.iter())
            .find(|((id, _), _)| *id == blueprint_id)
            .map(|(_, cost)| *cost)
            .ok_or_else(|| ProjectionError::UnknownBlueprint(blueprint_id.to_owned()))
    }
}

/// What one route step orders, as one upper bound in the sim's types.
///
/// # Errors
///
/// As [`Prices::blueprint`], and [`ProjectionError::Overflow`] when the step's
/// orders leave the sim's `$` or `kW` type.
pub fn step_cost(entry: &Step, prices: &Prices) -> Result<Cost, ProjectionError> {
    orders(entry, prices, None)?.upper().narrow()
}

/// What one route step orders, sorted into what it adds and what it may add
/// (the module doc, "How sure it is"), exactly.
///
/// `scope` is the seat's view, which says which writ a beacon named by id is
/// on, so a `set_mandate` row can be told apart from a switch.
///
/// # Errors
///
/// As [`Prices::blueprint`]: a blueprint the rules table does not price. Never
/// [`ProjectionError::Overflow`], because a [`Tally`] holds any playbook's
/// orders.
pub fn step_orders(
    entry: &Step,
    prices: &Prices,
    scope: &Scope,
) -> Result<Orders, ProjectionError> {
    orders(entry, prices, Some(scope))
}

/// [`step_orders`], with or without a view. Without one, no `set_mandate` row
/// can be shown to be a switch, so it is counted in the upper pile; the sum of
/// the two piles is the same either way.
fn orders(entry: &Step, prices: &Prices, scope: Option<&Scope>) -> Result<Orders, ProjectionError> {
    match entry.kind.as_ref() {
        Some(step::Kind::PlaceBeacon(place)) => {
            // A new beacon fields nothing yet, so everything its initial
            // settings order is new.
            let initial = match place
                .initial
                .as_ref()
                .and_then(|initial| initial.mandate.as_ref())
            {
                Some(settings) => settings_tally(settings, prices)?,
                None => Tally::ZERO,
            };
            Ok(Orders {
                adds: Tally::of(prices.beacon()).plus(initial),
                at_most: Tally::ZERO,
            })
        }
        Some(step::Kind::Interface(visit)) => {
            let mut found = Orders::default();
            for row in &visit.rows {
                match row.row.as_ref() {
                    Some(interface_row::Row::SetMandate(settings)) => {
                        let ordered = settings_tally(settings, prices)?;
                        if is_a_switch(visit, settings, scope) {
                            found.adds = found.adds.plus(ordered);
                        } else {
                            found.at_most = found.at_most.plus(ordered);
                        }
                    }
                    Some(interface_row::Row::SetMandateSettings(settings)) => {
                        found.at_most = found.at_most.plus(settings_tally(settings, prices)?);
                    }
                    Some(interface_row::Row::AddBuildTarget(add)) => {
                        if let Some(target) = add.target.as_ref() {
                            found.adds = found.adds.plus(target_tally(target, prices)?);
                        }
                    }
                    Some(interface_row::Row::QueueStructure(queue)) => {
                        found.adds = found
                            .adds
                            .plus(Tally::of(prices.blueprint(&queue.blueprint_id)?));
                    }
                    Some(
                        interface_row::Row::SetPriority(_)
                        | interface_row::Row::Recycle(_)
                        | interface_row::Row::RemoveBuildTarget(_),
                    )
                    | None => {}
                }
            }
            Ok(found)
        }
        Some(
            step::Kind::Move(_)
            | step::Kind::WaitUntil(_)
            | step::Kind::Hold(_)
            | step::Kind::Broadcast(_),
        )
        | None => Ok(Orders::default()),
    }
}

/// True when a `set_mandate` row certainly switches its beacon's writ: the
/// beacon is named by an id the view knows, and the view says it is on a
/// different writ. A beacon named by a description, or one already on the
/// writ the row names, may already field what the row orders.
fn is_a_switch(visit: &InterfaceStep, settings: &MandateSettings, scope: Option<&Scope>) -> bool {
    let Some(scope) = scope else {
        return false;
    };
    let Some(beacon_ref::Ref::BeaconId(id)) =
        visit.beacon.as_ref().and_then(|at| at.r#ref.as_ref())
    else {
        return false;
    };
    let Some(known) = scope.beacon(id) else {
        return false;
    };
    let writ = match settings.mandate.as_ref() {
        Some(mandate_settings::Mandate::Build(_)) => MandateKind::Build,
        Some(mandate_settings::Mandate::Survey(_)) => MandateKind::Survey,
        Some(mandate_settings::Mandate::Mine(_)) => MandateKind::Mine,
        Some(mandate_settings::Mandate::Defend(_)) => MandateKind::Defend,
        Some(mandate_settings::Mandate::Attack(_)) => MandateKind::Attack,
        None => return false,
    };
    known.mandate != writ
}

/// What a settings block orders: its Build targets, or its scouts.
fn settings_tally(settings: &MandateSettings, prices: &Prices) -> Result<Tally, ProjectionError> {
    match settings.mandate.as_ref() {
        Some(mandate_settings::Mandate::Build(build)) => {
            let mut total = Tally::ZERO;
            for target in &build.targets {
                total = total.plus(target_tally(target, prices)?);
            }
            Ok(total)
        }
        Some(mandate_settings::Mandate::Survey(survey)) => {
            Ok(Tally::times(prices.scout(), survey.scout_count))
        }
        Some(
            mandate_settings::Mandate::Mine(_)
            | mandate_settings::Mandate::Defend(_)
            | mandate_settings::Mandate::Attack(_),
        )
        | None => Ok(Tally::ZERO),
    }
}

/// One Build target: one structure of its blueprint.
fn target_tally(target: &BuildTarget, prices: &Prices) -> Result<Tally, ProjectionError> {
    prices.blueprint(&target.blueprint_id).map(Tally::of)
}

/// What the playbook orders, and what the seat is left with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Projection {
    /// What the route orders, in `$`.
    pub spend: Money,
    /// The treasury once all of it has left, in `$`. Negative means the
    /// playbook has ordered more than the seat can pay for.
    pub treasury_after: Money,
    /// The `kW` the route adds to the grid's draw.
    pub draw_added: Kw,
    /// Supply minus draw once the route has fielded everything, in `kW`.
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

    /// True when the grid cannot carry what the route fields.
    #[must_use]
    pub const fn is_short_of_power(&self) -> bool {
        self.headroom_after.raw() < 0
    }
}

/// Projects a playbook's route against the seat's economy.
///
/// # Errors
///
/// As [`step_cost`], and [`ProjectionError::Overflow`] when the treasury or
/// the headroom left would not fit its type — reported rather than saturated,
/// because a treasury that silently clamps is a wrong answer in the
/// reassuring direction (AGENTS.md section 4.1).
pub fn project(
    playbook: &Playbook,
    economy: SeatEconomy,
    prices: &Prices,
) -> Result<Projection, ProjectionError> {
    let mut wide = Tally::ZERO;
    if let Some(body) = playbook.declarative.as_ref() {
        for entry in &body.route {
            wide = wide.plus(orders(entry, prices, None)?.upper());
        }
    }
    let total = wide.narrow()?;
    let treasury_after = economy
        .treasury
        .checked_sub(total.spend)
        .ok_or(ProjectionError::Overflow)?;
    let headroom_after = economy
        .supply
        .checked_sub(economy.draw)
        .and_then(|headroom| headroom.checked_sub(total.draw))
        .ok_or(ProjectionError::Overflow)?;
    Ok(Projection {
        spend: total.spend,
        treasury_after,
        draw_added: total.draw,
        headroom_after,
    })
}

#[cfg(test)]
mod tests {
    use super::{Cost, Prices, ProjectionError, Tally, project};
    use crate::limits::RulesGap;
    use pharmakos_proto::gp::v1::{
        BuildSettings, BuildTarget, Declarative, InitialSettings, InterfaceRow, InterfaceStep,
        MandateSettings, PlaceBeaconStep, Playbook, QueueStructureRow, Step, SurveySettings,
        interface_row, mandate_settings, step,
    };
    use pharmakos_sim::knowledge::SeatEconomy;
    use pharmakos_sim::math::quantity::{Kw, Money};
    use pharmakos_sim::rules::RulesTable;

    fn rules() -> RulesTable {
        RulesTable::load(std::path::Path::new("../../rules/rules.v1.json"))
            .expect("the committed rules table")
    }

    fn prices() -> Prices {
        Prices::from_rules(&rules()).expect("the committed table prices everything")
    }

    fn economy() -> SeatEconomy {
        SeatEconomy {
            treasury: Money::new(200),
            supply: Kw::new(10),
            draw: Kw::new(2),
        }
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

    fn place(mandate: Option<mandate_settings::Mandate>) -> Step {
        Step {
            kind: Some(step::Kind::PlaceBeacon(PlaceBeaconStep {
                at: None,
                tags: Vec::new(),
                initial: mandate.map(|mandate| InitialSettings {
                    priority: 0,
                    mandate: Some(MandateSettings {
                        roe: 0,
                        retreat_hp_pct: 0,
                        mandate: Some(mandate),
                    }),
                }),
            })),
            ..Step::default()
        }
    }

    fn visit(row: interface_row::Row) -> Step {
        Step {
            kind: Some(step::Kind::Interface(InterfaceStep {
                beacon: None,
                rows: vec![InterfaceRow { row: Some(row) }],
            })),
            ..Step::default()
        }
    }

    fn generator() -> BuildTarget {
        BuildTarget {
            blueprint_id: "generator".to_owned(),
            anchor: None,
            rotation_quarter_turns: 0,
            order: 0,
        }
    }

    /// X-12's charge, and the key-core's net zero: a deploy costs a beacon's
    /// `$` 60 and adds no draw.
    #[test]
    fn a_deploy_costs_a_beacon_and_adds_no_draw() {
        let projection =
            project(&playbook(vec![place(None)]), economy(), &prices()).expect("priced");
        assert_eq!(projection.spend, Money::new(60));
        assert_eq!(projection.treasury_after, Money::new(140));
        assert_eq!(projection.draw_added, Kw::new(0));
        assert_eq!(projection.headroom_after, Kw::new(8));
        assert!(!projection.overspends());
        assert!(!projection.is_short_of_power());
    }

    /// S1-47: a Build target is priced at its blueprint, wherever it is
    /// written, and a Generator draws nothing.
    #[test]
    fn build_targets_cost_their_blueprint_wherever_they_are_written() {
        let build = || {
            mandate_settings::Mandate::Build(BuildSettings {
                targets: vec![generator(), generator()],
                ..BuildSettings::default()
            })
        };
        let deploy =
            project(&playbook(vec![place(Some(build()))]), economy(), &prices()).expect("priced");
        assert_eq!(deploy.spend, Money::new(60 + 2 * 80));
        assert_eq!(deploy.draw_added, Kw::new(0));
        assert!(deploy.overspends());

        let added = project(
            &playbook(vec![visit(interface_row::Row::AddBuildTarget(
                pharmakos_proto::gp::v1::AddBuildTargetRow {
                    target: Some(generator()),
                },
            ))]),
            economy(),
            &prices(),
        )
        .expect("priced");
        assert_eq!(added.spend, Money::new(80));
    }

    /// S1-47: the one unit a playbook orders is a Survey's scout count.
    #[test]
    fn ordered_scouts_cost_their_price_and_draw_a_unit_each() {
        let survey = mandate_settings::Mandate::Survey(SurveySettings {
            probe_areas: Vec::new(),
            scout_count: 9,
        });
        let projection =
            project(&playbook(vec![place(Some(survey))]), economy(), &prices()).expect("priced");
        assert_eq!(projection.spend, Money::new(60 + 9 * 10));
        assert_eq!(projection.draw_added, Kw::new(9));
        assert_eq!(projection.headroom_after, Kw::new(-1));
        assert!(projection.is_short_of_power());
    }

    #[test]
    fn a_queued_structure_costs_its_row_and_draws_by_the_sims_rule() {
        let queue = |id: &str| {
            visit(interface_row::Row::QueueStructure(QueueStructureRow {
                blueprint_id: id.to_owned(),
            }))
        };
        let mortar =
            project(&playbook(vec![queue("mortar")]), economy(), &prices()).expect("priced");
        assert_eq!(mortar.spend, Money::new(90));
        assert_eq!(mortar.draw_added, Kw::new(3));
        // An autocannon runs off the deep bore, outside the grid.
        let cannon =
            project(&playbook(vec![queue("autocannon")]), economy(), &prices()).expect("priced");
        assert_eq!(cannon.draw_added, Kw::new(0));
    }

    #[test]
    fn an_unknown_blueprint_is_an_error_rather_than_free() {
        let target = BuildTarget {
            blueprint_id: "death_ray".to_owned(),
            ..generator()
        };
        let build = mandate_settings::Mandate::Build(BuildSettings {
            targets: vec![target],
            ..BuildSettings::default()
        });
        assert_eq!(
            project(&playbook(vec![place(Some(build))]), economy(), &prices()),
            Err(ProjectionError::UnknownBlueprint("death_ray".to_owned()))
        );
    }

    #[test]
    fn a_route_that_orders_nothing_spends_nothing() {
        let projection = project(&playbook(Vec::new()), economy(), &prices()).expect("priced");
        assert_eq!(projection.spend, Money::new(0));
        assert_eq!(projection.treasury_after, Money::new(200));
    }

    #[test]
    fn an_overflowing_order_is_refused_rather_than_wrapped() {
        assert_eq!(
            Cost {
                spend: Money::new(i64::MAX),
                draw: Kw::new(0)
            }
            .plus(Cost {
                spend: Money::new(1),
                draw: Kw::new(0)
            }),
            Err(ProjectionError::Overflow)
        );
        // One hostile count is past the sim's types, and the upper bound
        // refuses it; the tally the estimate stage reads holds it exactly.
        let survey = mandate_settings::Mandate::Survey(SurveySettings {
            probe_areas: Vec::new(),
            scout_count: u32::MAX,
        });
        let playbook = playbook(vec![place(Some(survey))]);
        assert_eq!(
            project(&playbook, economy(), &prices()),
            Err(ProjectionError::Overflow)
        );
        let tally = Tally::times(prices().scout(), u32::MAX);
        assert_eq!(tally.draw, i128::from(u32::MAX));
        assert_eq!(tally.spend, 10 * i128::from(u32::MAX));
    }

    #[test]
    fn a_missing_price_is_refused_rather_than_zero() {
        let mut message = rules().message().clone();
        if let Some(units) = message.units.as_mut() {
            units.scout = None;
        }
        let gapped = RulesTable::from_message(&message).expect("the sim's view still builds");
        assert_eq!(
            Prices::from_rules(&gapped),
            Err(RulesGap::MissingBlock("units.scout"))
        );
    }
}
