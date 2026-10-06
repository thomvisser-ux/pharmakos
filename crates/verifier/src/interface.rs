// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Interface-time arithmetic, at spec section 5's rates.
//!
//! This is one of the four things spec section 11 lists as an **allowed
//! estimate**, and the only one of the four that is exact rather than
//! estimated: an interface row's duration is a number in the rules table, and
//! the sum of a visit's rows is what the commander will actually stand there
//! for. The editor's segment clock draws it as a bar with a tick at each
//! commit point, `render_plan` says it in words, and the estimate stage adds it
//! to the route's walking to say whether the route fits the coming segment.
//!
//! # Why it lives in the verifier
//!
//! It was `plan-core`'s, and `plan-core` depends on this crate, so the
//! estimate stage could not call it without a dependency cycle (S1's plan,
//! task `proj`, and its risk R7). It moved **down** here, once, and
//! `plan-core` re-exports this module unchanged: one home for the rate card,
//! so the prose, the editor's clock and the verifier's schedule check can
//! never price one visit three ways.
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
//! this file (AGENTS.md section 12). Spec section 5's times are **ratified as
//! those rows** (S1's plan, decision 15; decisions-log item 128 (3) (g); the
//! register's U-04): the rows are the rates, and a tuning pull request that
//! moves one moves every price here with it.
//!
//! # Rows are atomic
//!
//! Spec section 5: each row "commits at the end of its own duration, so a
//! multi-field edit is all-or-nothing". [`Visit::commits`] is therefore the
//! running total after each row — the tick marks on the editor's clock — and
//! not merely a sum.
//!
//! # Sums saturate, and only upwards
//!
//! A visit's total is a sum of non-negative rows ([`Rates::from_rules`]
//! refuses a negative rate), so the only way a sum can leave `i32` is upwards,
//! past twenty-four days of game time. It saturates at [`i32::MAX`] there
//! rather than failing: every consumer compares the figure against a segment
//! of minutes, and a saturated figure still exceeds it, so saturation can make
//! the schedule check fire and can never make it fall silent.

use pharmakos_proto::gp::v1::{
    AttackSettings, BuildSettings, DefendSettings, InitialSettings, InterfaceRow, InterfaceStep,
    MandateSettings, MineSettings, PlaceBeaconStep, Playbook, Step, SurveySettings, interface_row,
    mandate_settings, step,
};
use pharmakos_sim::math::quantity::Ms;
use pharmakos_sim::rules::RulesTable;

use crate::limits::RulesGap;

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
    /// [`RulesGap::MissingBlock`] when the rules table carries no
    /// `interface_times` block: substituting zeroes would make every visit
    /// free, which is the one answer that is never a safe default.
    /// [`RulesGap::Negative`] when a rate is below zero, which no duration can
    /// be.
    pub fn from_rules(rules: &RulesTable) -> Result<Rates, RulesGap> {
        let times = rules
            .message()
            .interface_times
            .as_ref()
            .ok_or(RulesGap::MissingBlock("interface_times"))?;
        Ok(Rates {
            visit_handshake: rate(
                "interface_times.visit_handshake_ms",
                times.visit_handshake_ms,
            )?,
            set_priority: rate("interface_times.set_priority_ms", times.set_priority_ms)?,
            edit_settings_base: rate(
                "interface_times.edit_settings_base_ms",
                times.edit_settings_base_ms,
            )?,
            edit_settings_per_field: rate(
                "interface_times.edit_settings_per_field_ms",
                times.edit_settings_per_field_ms,
            )?,
            edit_settings_max: rate(
                "interface_times.edit_settings_max_ms",
                times.edit_settings_max_ms,
            )?,
            build_target: rate("interface_times.build_target_ms", times.build_target_ms)?,
            queue_structure: rate(
                "interface_times.queue_structure_ms",
                times.queue_structure_ms,
            )?,
            switch_mandate: rate("interface_times.switch_mandate_ms", times.switch_mandate_ms)?,
            recycle: rate("interface_times.recycle_ms", times.recycle_ms)?,
            place_beacon_deploy: rate(
                "interface_times.place_beacon_deploy_ms",
                times.place_beacon_deploy_ms,
            )?,
        })
    }

    /// The visit handshake, paid once per visit to a beacon that is already
    /// there.
    #[must_use]
    pub const fn visit_handshake(&self) -> Ms {
        self.visit_handshake
    }

    /// The `place_beacon` deploy, which the commander must stay for.
    #[must_use]
    pub const fn place_beacon_deploy(&self) -> Ms {
        self.place_beacon_deploy
    }

    /// Adding or removing one Build target.
    #[must_use]
    pub const fn build_target(&self) -> Ms {
        self.build_target
    }

    /// Recycling a beacon.
    #[must_use]
    pub const fn recycle(&self) -> Ms {
        self.recycle
    }

    /// Editing mandate settings: base, plus one step per **extra** field,
    /// capped.
    #[must_use]
    pub fn edit_settings(&self, fields: u32) -> Ms {
        if fields == 0 {
            return Ms::ZERO;
        }
        // Every rate is non-negative (`from_rules`), so the uncapped figure can
        // only grow; saturating it before the cap is exact, because the cap is
        // itself an `i32` and the smaller of the two is what is charged.
        let extra = i32::try_from(fields.saturating_sub(1)).unwrap_or(i32::MAX);
        let uncapped = self
            .edit_settings_base
            .raw()
            .saturating_add(self.edit_settings_per_field.raw().saturating_mul(extra));
        Ms::new(uncapped.min(self.edit_settings_max.raw()))
    }
}

/// One rate, refused when negative.
fn rate(field: &'static str, value: i32) -> Result<Ms, RulesGap> {
    if value < 0 {
        return Err(RulesGap::Negative {
            field,
            value: i64::from(value),
        });
    }
    Ok(Ms::new(value))
}

/// Two durations added, saturating upwards (see the module doc).
fn plus(left: Ms, right: Ms) -> Ms {
    Ms::new(left.raw().saturating_add(right.raw()))
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
    fn from_rows(first: Ms, rows: impl IntoIterator<Item = Ms>) -> Visit {
        let mut running = first;
        let mut commits = Vec::new();
        if first.raw() != 0 {
            commits.push(running);
        }
        for row in rows {
            running = plus(running, row);
            commits.push(running);
        }
        Visit {
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
            plus(rates.switch_mandate, settings_ms(settings, rates))
        }
        Some(interface_row::Row::SetMandateSettings(settings)) => settings_ms(settings, rates),
        Some(interface_row::Row::SetPriority(_)) => rates.set_priority,
        Some(interface_row::Row::Recycle(_)) => rates.recycle,
        Some(interface_row::Row::AddBuildTarget(_) | interface_row::Row::RemoveBuildTarget(_)) => {
            rates.build_target
        }
        Some(interface_row::Row::QueueStructure(_)) => rates.queue_structure,
        None => Ms::ZERO,
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
/// **A deploy pays no separate handshake.** Spec section 5 charges the
/// handshake "once per visit", and spec section 10 says a `place_beacon` is
/// "12 s deploy, then any initial settings at interface rates", with no
/// handshake named: a deploy is not a visit to a beacon that was already
/// there. The owner ratified that reading (S1's plan, decision 15;
/// decisions-log item 128 (3) (g); the register's X-05), so the deploy is the
/// first row and nothing comes before it.
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
    let each = i64::from(rates.build_target.raw());
    let targets = each.saturating_mul(i64::from(counted.targets));
    // Saturating upwards, as the module doc says: a figure past `i32` is past
    // every segment, and saying so is the only thing it is used for.
    let targets = Ms::new(i32::try_from(targets).unwrap_or(i32::MAX));
    plus(edit, targets)
}

/// What a settings message contains, for pricing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Counted {
    /// Settings fields the edit writes. The mandate *type* is not one of
    /// them, and each element of a list setting is one.
    pub fields: u32,
    /// Build targets, which are their own row at their own rate.
    pub targets: u32,
}

/// One for a field written at a non-default value, which is what proto3's
/// canonical JSON keeps and what the author therefore set.
fn set(written: bool) -> u32 {
    u32::from(written)
}

/// The elements of a list, each one field.
fn each<T>(list: &[T]) -> u32 {
    u32::try_from(list.len()).unwrap_or(u32::MAX)
}

/// Counts what an edit of `settings` writes.
///
/// A field counts when the author set it — a non-default value, which is the
/// same rule the canonical form, the size meter and the sim's compile count
/// by. The mandate *type* is free (spec section 5) and is never a field.
///
/// **What "one field" means for a list (S1-23, ruled).** Spec section 5 writes
/// "2 s + 0.5 s per extra field" without saying what a field is when a setting
/// is a list (`protected_areas`, `probe_areas`). The owner ruled it (S1's
/// plan, decision 15; decisions-log item 128 (3) (g)): **each changed element
/// of a list counts as one field**, and the 6 s cap still bounds the edit. A
/// Build target is not a field: spec section 5 gives targets a row of their
/// own, "add or remove a build target, 2.5 s each", so they are counted apart
/// and priced at that rate.
///
/// Every settings message is taken apart by an **exhaustive** destructuring
/// pattern, so a field added to the schema does not compile here until
/// somebody decides what it costs — a new field can never silently cost
/// nothing.
///
/// PLACEHOLDER: the sim's own pricing does not count this way yet —
/// `crates/sim/src/interpreter.rs`'s `mandate_fields` still counts a list as
/// one field whatever its length and a Build target list as one field of the
/// edit, so a playbook with a two-element list, or with targets, stands at a
/// beacon for a different time in the Push from the one quoted here. The
/// owner's ruling is this function's; bringing the sim to it moves the chains
/// of every scenario whose playbook writes such a list, so it is a `crates/sim`
/// lane's (the `build` task, S1's Build settings, which owns that file next) —
/// owner of `crates/sim`, S1 wave 4.
#[must_use]
pub fn count(settings: &MandateSettings) -> Counted {
    let MandateSettings {
        roe,
        retreat_hp_pct,
        mandate,
    } = settings;
    let mut counted = Counted {
        fields: set(*roe != 0).saturating_add(set(*retreat_hp_pct != 0)),
        targets: 0,
    };
    let (fields, targets) = match mandate {
        Some(mandate_settings::Mandate::Build(build)) => {
            let BuildSettings {
                targets,
                repair_threshold_pct,
                rebuild_destroyed,
                terraform,
                protected_areas,
            } = build;
            (
                set(*repair_threshold_pct != 0)
                    .saturating_add(set(*rebuild_destroyed))
                    .saturating_add(set(*terraform != 0))
                    .saturating_add(each(protected_areas)),
                each(targets),
            )
        }
        Some(mandate_settings::Mandate::Survey(survey)) => {
            let SurveySettings {
                probe_areas,
                scout_count,
            } = survey;
            (each(probe_areas).saturating_add(set(*scout_count != 0)), 0)
        }
        Some(mandate_settings::Mandate::Mine(mine)) => {
            let MineSettings {
                dig_max_depth,
                flee_on_threat,
                seam_choice,
                pillar_spacing,
            } = mine;
            (
                set(*dig_max_depth != 0)
                    .saturating_add(set(*flee_on_threat))
                    .saturating_add(set(*seam_choice != 0))
                    .saturating_add(set(*pillar_spacing != 0)),
                0,
            )
        }
        // Defend and Attack hold their field numbers until S2 and have no
        // field to write yet; the patterns name the empty messages so that
        // the day they gain one, this match stops compiling.
        Some(
            mandate_settings::Mandate::Defend(DefendSettings {})
            | mandate_settings::Mandate::Attack(AttackSettings {}),
        )
        | None => (0, 0),
    };
    counted.fields = counted.fields.saturating_add(fields);
    counted.targets = targets;
    counted
}

/// What one route step costs on site: nothing unless it is a visit or a
/// deploy.
#[must_use]
pub fn step_ms(entry: &Step, rates: &Rates) -> Ms {
    match entry.kind.as_ref() {
        Some(step::Kind::Interface(visit)) => interface_ms(visit, rates).total,
        Some(step::Kind::PlaceBeacon(place)) => place_beacon_ms(place, rates).total,
        _ => Ms::ZERO,
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
    playbook.declarative.as_ref().map_or(Ms::ZERO, |body| {
        body.route
            .iter()
            .fold(Ms::ZERO, |total, entry| plus(total, step_ms(entry, rates)))
    })
}

#[cfg(test)]
mod tests {
    use super::{Counted, Rates, count, place_beacon_ms, route_ms, row_ms};
    use crate::limits::RulesGap;
    use pharmakos_proto::gp::v1::{
        Area, BuildSettings, BuildTarget, InitialSettings, InterfaceRow, MandateSettings,
        MineSettings, PlaceBeaconStep, SurveySettings, Voxel, interface_row, mandate_settings,
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

    fn area() -> Area {
        Area {
            min: Some(Voxel { x: 1, y: 1, z: 1 }),
            max: Some(Voxel { x: 2, y: 2, z: 2 }),
        }
    }

    #[test]
    fn the_rate_card_is_spec_section_fives_table() {
        let rates = rates();
        assert_eq!(rates.visit_handshake(), Ms::new(1500));
        assert_eq!(rates.recycle(), Ms::new(10000));
        assert_eq!(rates.place_beacon_deploy(), Ms::new(12000));
        assert_eq!(rates.build_target(), Ms::new(2500));
    }

    #[test]
    fn a_missing_or_negative_rate_is_refused_rather_than_free() {
        let mut message = rules().message().clone();
        message.interface_times = None;
        let gapped = RulesTable::from_message(&message).expect("the sim's view still builds");
        assert_eq!(
            Rates::from_rules(&gapped),
            Err(RulesGap::MissingBlock("interface_times"))
        );

        let mut message = rules().message().clone();
        if let Some(times) = message.interface_times.as_mut() {
            times.recycle_ms = -1;
        }
        let negative = RulesTable::from_message(&message).expect("the sim's view still builds");
        assert_eq!(
            Rates::from_rules(&negative),
            Err(RulesGap::Negative {
                field: "interface_times.recycle_ms",
                value: -1
            })
        );
    }

    #[test]
    fn editing_settings_is_base_plus_a_step_per_extra_field_and_is_capped() {
        let rates = rates();
        assert_eq!(rates.edit_settings(0), Ms::new(0));
        assert_eq!(rates.edit_settings(1), Ms::new(2000));
        assert_eq!(rates.edit_settings(3), Ms::new(3000));
        assert_eq!(rates.edit_settings(99), Ms::new(6000));
        assert_eq!(rates.edit_settings(u32::MAX), Ms::new(6000));
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
    /// priority, and three settings fields at 2 s + 2 x 0.5 s. No handshake:
    /// a deploy is not a visit (X-05, decision 15).
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

    /// S1-23, decision 15: each element of a list is one field, and the cap
    /// still bounds the edit.
    #[test]
    fn each_element_of_a_list_is_one_field_and_the_cap_still_bounds_the_edit() {
        let survey = |areas: usize, scouts: u32| MandateSettings {
            roe: 0,
            retreat_hp_pct: 0,
            mandate: Some(mandate_settings::Mandate::Survey(SurveySettings {
                probe_areas: vec![area(); areas],
                scout_count: scouts,
            })),
        };
        assert_eq!(
            count(&survey(3, 2)),
            Counted {
                fields: 4,
                targets: 0
            }
        );
        let row = |settings: MandateSettings| InterfaceRow {
            row: Some(interface_row::Row::SetMandateSettings(settings)),
        };
        // One area: 2 s. Three areas and a count: 2 s + 3 x 0.5 s.
        assert_eq!(row_ms(&row(survey(1, 0)), &rates()), Ms::new(2000));
        assert_eq!(row_ms(&row(survey(3, 2)), &rates()), Ms::new(3500));
        // Twenty areas would be 11.5 s uncapped; the cap is 6 s.
        assert_eq!(row_ms(&row(survey(20, 0)), &rates()), Ms::new(6000));
    }

    /// A Build target is its own row at its own rate, outside the edit's cap,
    /// and protected areas are list fields of the edit.
    #[test]
    fn build_targets_are_their_own_rows_and_protected_areas_are_fields() {
        let target = BuildTarget {
            blueprint_id: "generator".to_owned(),
            anchor: None,
            order: 0,
            rotation_quarter_turns: 0,
        };
        let settings = MandateSettings {
            roe: 0,
            retreat_hp_pct: 0,
            mandate: Some(mandate_settings::Mandate::Build(BuildSettings {
                targets: vec![target.clone(), target.clone(), target],
                repair_threshold_pct: 0,
                rebuild_destroyed: false,
                terraform: 0,
                protected_areas: vec![area(), area()],
            })),
        };
        assert_eq!(
            count(&settings),
            Counted {
                fields: 2,
                targets: 3
            }
        );
        let row = InterfaceRow {
            row: Some(interface_row::Row::SetMandateSettings(settings)),
        };
        // 2 s + 0.5 s for the two areas, then 3 x 2.5 s for the targets.
        assert_eq!(row_ms(&row, &rates()), Ms::new(10_000));
    }

    #[test]
    fn a_playbook_with_no_route_costs_nothing() {
        let playbook = pharmakos_proto::gp::v1::Playbook::default();
        assert_eq!(route_ms(&playbook, &rates()), Ms::new(0));
    }
}
