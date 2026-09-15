// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Interface-time arithmetic, at spec section 5's rates.
//!
//! This is one of the four things spec section 11 lists as an **allowed
//! estimate**, and the only one of the four that is exact rather than
//! estimated: an interface row's duration is a number in the rules table, and
//! the sum of a visit's rows is what the commander will actually stand there
//! for. The editor's segment clock draws it as a bar with a tick at each
//! commit point, and `render_plan` says it in words.
//!
//! | Change | Row |
//! |---|---|
//! | Visit handshake, once per visit | `interface_times.visit_handshake_ms` |
//! | Set Quartermaster priority | `interface_times.set_priority_ms` |
//! | Edit mandate settings | `edit_settings_base_ms` + `edit_settings_per_field_ms` per **extra** field, capped at `edit_settings_max_ms` |
//! | Add or remove a build target | `interface_times.build_target_ms`, each |
//! | Queue a capability structure | `interface_times.queue_structure_ms` (plus the fabricator's own construction time, which is the mandate's and not counted here) |
//! | Switch mandate type | `interface_times.switch_mandate_ms` |
//! | Recycle beacon | `interface_times.recycle_ms` |
//! | `place_beacon` deploy | `interface_times.place_beacon_deploy_ms` |
//!
//! Every number is read from `gp.v1.RulesTable`; there is not a constant in
//! this file (AGENTS.md section 12).
//!
//! # Rows are atomic
//!
//! Spec section 5: each row "commits at the end of its own duration, so a
//! multi-field edit is all-or-nothing". [`Visit::commits`] is therefore the
//! running total after each row — the tick marks on the editor's clock — and
//! not merely a sum.

use pharmakos_proto::gp::v1::{
    InitialSettings, InterfaceRow, InterfaceStep, MandateSettings, PlaceBeaconStep, Playbook, Step,
    interface_row, step,
};
use pharmakos_proto::json;
use pharmakos_sim::math::quantity::Ms;
use pharmakos_sim::rules::RulesTable;

use crate::error::Error;

/// Spec section 5's rate card, read out of the rules table once.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rates {
    visit_handshake: Ms,
    set_priority: Ms,
    edit_settings_base: Ms,
    edit_settings_per_field: Ms,
    edit_settings_max: Ms,
    build_target: Ms,
    queue_structure: Ms,
    switch_mandate: Ms,
    recycle: Ms,
    place_beacon_deploy: Ms,
}

impl Rates {
    /// Reads the rate card.
    ///
    /// # Errors
    ///
    /// When the rules table carries no `interface_times` block. Substituting
    /// zeroes would make every visit free, which is the one answer that is
    /// never a safe default.
    pub fn from_rules(rules: &RulesTable) -> Result<Self, Error> {
        let times = rules.message().interface_times.as_ref().ok_or_else(|| {
            Error::at(
                "",
                "the rules table carries no `interface_times` block, so no visit can be priced",
            )
        })?;
        Ok(Self {
            visit_handshake: Ms::new(times.visit_handshake_ms),
            set_priority: Ms::new(times.set_priority_ms),
            edit_settings_base: Ms::new(times.edit_settings_base_ms),
            edit_settings_per_field: Ms::new(times.edit_settings_per_field_ms),
            edit_settings_max: Ms::new(times.edit_settings_max_ms),
            build_target: Ms::new(times.build_target_ms),
            queue_structure: Ms::new(times.queue_structure_ms),
            switch_mandate: Ms::new(times.switch_mandate_ms),
            recycle: Ms::new(times.recycle_ms),
            place_beacon_deploy: Ms::new(times.place_beacon_deploy_ms),
        })
    }

    /// The `place_beacon` deploy, which the commander must stay for.
    #[must_use]
    pub const fn place_beacon_deploy(&self) -> Ms {
        self.place_beacon_deploy
    }

    /// Editing mandate settings: base, plus one step per **extra** field,
    /// capped.
    #[must_use]
    pub fn edit_settings(&self, fields: u32) -> Ms {
        if fields == 0 {
            return Ms::new(0);
        }
        let extra = i64::from(fields.saturating_sub(1));
        let raw = i64::from(self.edit_settings_base.raw())
            .saturating_add(i64::from(self.edit_settings_per_field.raw()).saturating_mul(extra));
        let capped = raw.min(i64::from(self.edit_settings_max.raw()));
        Ms::new(i32::try_from(capped).unwrap_or(self.edit_settings_max.raw()))
    }
}

/// One visit, priced row by row.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Visit {
    /// The running total after each row commits, in the order the rows are
    /// written. Spec section 5: a row "commits at the end of its own
    /// duration", so these are the tick marks the editor draws.
    pub commits: Vec<Ms>,
    /// The whole visit.
    pub total: Ms,
}

impl Visit {
    fn from_rows(first: Ms, rows: impl IntoIterator<Item = Ms>) -> Self {
        let mut running = first;
        let mut commits = Vec::new();
        if first.raw() != 0 {
            commits.push(running);
        }
        for row in rows {
            running = Ms::new(running.raw().saturating_add(row.raw()));
            commits.push(running);
        }
        Self {
            commits,
            total: running,
        }
    }
}

/// What one interface row costs.
#[must_use]
pub fn row_ms(row: &InterfaceRow, rates: &Rates) -> Ms {
    match row.row.as_ref() {
        Some(interface_row::Row::SetMandate(settings)) => {
            // Switching the type, then the settings that came with it. The
            // switch clears the old settings, so everything written here is a
            // new edit (spec section 5, "Mandate switch").
            let switch = rates.switch_mandate;
            let edit = settings_ms(settings, rates);
            Ms::new(switch.raw().saturating_add(edit.raw()))
        }
        Some(interface_row::Row::SetMandateSettings(settings)) => settings_ms(settings, rates),
        Some(interface_row::Row::SetPriority(_)) => rates.set_priority,
        Some(interface_row::Row::Recycle(_)) => rates.recycle,
        Some(interface_row::Row::AddBuildTarget(_) | interface_row::Row::RemoveBuildTarget(_)) => {
            rates.build_target
        }
        Some(interface_row::Row::QueueStructure(_)) => rates.queue_structure,
        None => Ms::new(0),
    }
}

/// What a visit to a beacon costs: the handshake, then every row.
#[must_use]
pub fn interface_ms(visit: &InterfaceStep, rates: &Rates) -> Visit {
    Visit::from_rows(
        rates.visit_handshake,
        visit.rows.iter().map(|row| row_ms(row, rates)),
    )
}

/// What a `place_beacon` costs: the deploy, then the initial settings.
///
/// **PLACEHOLDER — whether a deploy also pays the visit handshake.** Spec
/// section 5 charges the handshake "once per visit" and spec section 10 says a
/// `place_beacon` is "12 s deploy, then any initial settings at interface
/// rates", with no handshake named; a deploy is not a visit to a beacon that
/// was already there. This module takes that reading and does **not** charge
/// it. The **owner** settles it at **T14**, with the mandate contract that
/// also decides what an omitted mandate setting means; `render_plan`'s prose
/// and the editor's clock move together if the reading changes.
#[must_use]
pub fn place_beacon_ms(place: &PlaceBeaconStep, rates: &Rates) -> Visit {
    Visit::from_rows(
        rates.place_beacon_deploy,
        place
            .initial
            .as_ref()
            .map(|initial| initial_rows(initial, rates))
            .unwrap_or_default(),
    )
}

/// The initial settings of a `place_beacon`, as the rows they are.
///
/// Spec section 5: "Choosing the mandate type is free. Every other initial
/// setting or target costs its normal interface time after the deploy." So the
/// mandate *arm* is free here — unlike `set_mandate` on an existing beacon,
/// which pays the switch — and what is inside it is an ordinary settings edit.
fn initial_rows(initial: &InitialSettings, rates: &Rates) -> Vec<Ms> {
    let mut rows = Vec::new();
    if initial.priority != 0 {
        rows.push(rates.set_priority);
    }
    if let Some(settings) = initial.mandate.as_ref() {
        let priced = settings_ms(settings, rates);
        if priced.raw() != 0 {
            rows.push(priced);
        }
    }
    rows
}

/// An edit of mandate settings: the settings fields at the edit rate, and each
/// build target at the build-target rate.
fn settings_ms(settings: &MandateSettings, rates: &Rates) -> Ms {
    let counted = count(settings);
    let edit = rates.edit_settings(counted.fields);
    let targets = i64::from(rates.build_target.raw()).saturating_mul(i64::from(counted.targets));
    let total = i64::from(edit.raw()).saturating_add(targets);
    Ms::new(i32::try_from(total).unwrap_or(i32::MAX))
}

/// What a settings message contains, for pricing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Counted {
    /// Settings fields that are set. The mandate *type* is not one of them.
    fields: u32,
    /// Build targets, which are their own row at their own rate.
    targets: u32,
}

/// The five writ arms. Selecting one is free; what is inside it is not.
const WRITS: &[&str] = &["build", "defend", "attack", "survey", "mine"];

/// Counts what is set, by asking the canonical encoder.
///
/// The canonical form omits a field at its proto3 default, so the members of
/// the encoded object **are** the fields the author set — which is the same
/// rule the size meter counts by and the same one the verifier reads. Counting
/// by hand instead would mean a new mandate field silently costing nothing
/// until somebody remembered to add it here.
///
/// **PLACEHOLDER — what "one field" means for the 0.5 s step.** Spec section 5
/// writes "2 s + 0.5 s per extra field" without saying what a field is when a
/// setting is a list (`protected_areas`, `probe_areas`). This module counts one
/// per set member of the settings message, so a list is one field however long
/// it is, and build targets are priced as their own rows because spec section 5
/// gives them a row of their own. The **owner** ratifies the reading at **S1**,
/// with the full mandate contract; `expand_east`'s four-and-a-half seconds is
/// the worked example either way.
fn count(settings: &MandateSettings) -> Counted {
    let Ok(json::Json::Object(entries)) = json::encode_json(settings) else {
        return Counted::default();
    };
    let mut counted = Counted::default();
    for (key, value) in &entries {
        if WRITS.contains(&key.as_str()) {
            let json::Json::Object(inner) = value else {
                continue;
            };
            for (name, item) in inner {
                if name == "targets" {
                    if let json::Json::Array(targets) = item {
                        counted.targets = counted
                            .targets
                            .saturating_add(u32::try_from(targets.len()).unwrap_or(u32::MAX));
                    }
                } else {
                    counted.fields = counted.fields.saturating_add(1);
                }
            }
        } else {
            counted.fields = counted.fields.saturating_add(1);
        }
    }
    counted
}

/// What one route step costs on site: nothing unless it is a visit or a
/// deploy.
#[must_use]
pub fn step_ms(entry: &Step, rates: &Rates) -> Ms {
    match entry.kind.as_ref() {
        Some(step::Kind::Interface(visit)) => interface_ms(visit, rates).total,
        Some(step::Kind::PlaceBeacon(place)) => place_beacon_ms(place, rates).total,
        _ => Ms::new(0),
    }
}

/// Every second the route spends standing at a beacon.
///
/// Handler bodies are **not** counted: a handler fires at most `max_fires`
/// times and may not fire at all, so adding its interface time to the route's
/// would be a claim about the future rather than arithmetic. The editor draws
/// worst-case handler detours in a second lane (spec section 13), which is the
/// place for it.
#[must_use]
pub fn route_ms(playbook: &Playbook, rates: &Rates) -> Ms {
    playbook.declarative.as_ref().map_or(Ms::new(0), |body| {
        body.route.iter().fold(Ms::new(0), |total, entry| {
            Ms::new(total.raw().saturating_add(step_ms(entry, rates).raw()))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::{Rates, place_beacon_ms, route_ms, row_ms};
    use pharmakos_proto::gp::v1::{
        InitialSettings, InterfaceRow, MandateSettings, MineSettings, PlaceBeaconStep,
        interface_row, mandate_settings,
    };
    use pharmakos_sim::math::quantity::Ms;
    use pharmakos_sim::rules::RulesTable;

    fn rules() -> RulesTable {
        RulesTable::load(std::path::Path::new("../../rules/rules.v1.json"))
            .expect("the committed rules table")
    }

    fn rates() -> Rates {
        Rates::from_rules(&rules()).expect("the rules table carries interface_times")
    }

    #[test]
    fn the_rate_card_is_spec_section_fives_table() {
        let rates = rates();
        assert_eq!(rates.visit_handshake, Ms::new(1500));
        assert_eq!(rates.recycle, Ms::new(10000));
        assert_eq!(rates.place_beacon_deploy(), Ms::new(12000));
    }

    #[test]
    fn editing_settings_is_base_plus_a_step_per_extra_field_and_is_capped() {
        let rates = rates();
        assert_eq!(rates.edit_settings(0), Ms::new(0));
        assert_eq!(rates.edit_settings(1), Ms::new(2000));
        assert_eq!(rates.edit_settings(3), Ms::new(3000));
        assert_eq!(rates.edit_settings(99), Ms::new(6000));
    }

    #[test]
    fn a_recycle_row_is_ten_seconds() {
        let row = InterfaceRow {
            row: Some(interface_row::Row::Recycle(
                pharmakos_proto::gp::v1::RecycleRow {},
            )),
        };
        assert_eq!(row_ms(&row, &rates()), Ms::new(10000));
    }

    /// The worked example's `place_east` step: 12 s of deploy, 1.5 s of
    /// priority, and three settings fields at 2 s + 2 x 0.5 s.
    #[test]
    fn the_worked_examples_deploy_is_sixteen_and_a_half_seconds() {
        let place = PlaceBeaconStep {
            at: None,
            tags: vec!["east".to_owned()],
            initial: Some(InitialSettings {
                priority: 2,
                mandate: Some(MandateSettings {
                    roe: 2,
                    retreat_hp_pct: 0,
                    mandate: Some(mandate_settings::Mandate::Mine(MineSettings {
                        dig_max_depth: 4,
                        flee_on_threat: true,
                        seam_choice: 0,
                        pillar_spacing: 0,
                    })),
                }),
            }),
        };
        let visit = place_beacon_ms(&place, &rates());
        assert_eq!(visit.total, Ms::new(16_500));
        assert_eq!(
            visit.commits,
            vec![Ms::new(12_000), Ms::new(13_500), Ms::new(16_500)]
        );
    }

    #[test]
    fn a_playbook_with_no_route_costs_nothing() {
        let playbook = pharmakos_proto::gp::v1::Playbook::default();
        assert_eq!(route_ms(&playbook, &rates()), Ms::new(0));
    }
}
