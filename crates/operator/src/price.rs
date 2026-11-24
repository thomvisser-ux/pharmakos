// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a composed step costs on site, at spec section 5's rates.
//!
//! Every step Easy writes -- a placement, a tap, the core's dig depth, the
//! Spire, a stone, a post -- is priced here and nowhere else, from the step's
//! own JSON as it goes into the route, at the public rules text's interface
//! rates ([`crate::tuning`]). The rules are spec section 5's, as the verifier
//! states them (`crates/verifier/src/interface.rs`, decision 15 of S1's plan):
//!
//! * an `interface` step pays the **visit handshake** once, then each row;
//! * a `place_beacon` pays the **deploy**, then its initial settings as rows,
//!   with no handshake (the mandate type is free);
//! * a settings edit pays `edit_settings_base_ms` and
//!   `edit_settings_per_field_ms` per **extra** field, capped at
//!   `edit_settings_max_ms`; each element of a list setting is one field, and
//!   a field with presence counts whenever it is written;
//! * each Build target is its own row at `build_target_ms`; a priority row is
//!   `set_priority_ms`; a `queue_structure` row `queue_structure_ms`; a
//!   mandate switch pays `switch_mandate_ms` and its settings; a recycle
//!   `recycle_ms`.
//!
//! The operator depends on `pharmakos-proto` alone (AGENTS.md section 3 rule
//! 3), so it cannot call the verifier's arithmetic; it restates it, and
//! `crates/gamectl/tests/pricing.rs` feeds every step kind Easy composes
//! through both and holds them equal. Every settings message is taken apart by
//! an exhaustive pattern, as the verifier's is, so a field the schema gains
//! does not compile here until it is priced.

use pharmakos_proto::gp::v1::{
    AttackLaunch, AttackRetreat, AttackSettings, BuildSettings, DefendSettings, InitialSettings,
    InterfaceRow, MandateSettings, MineSettings, Step, SurveySettings, interface_row,
    mandate_settings, step,
};
use pharmakos_proto::json::Json;

use crate::tuning::Tuning;

/// A step's on-site time, game milliseconds: an `interface` step's handshake
/// and rows, a `place_beacon`'s deploy and initial rows, and nothing for any
/// other step (a walk is travel, which the estimate prices). `None` when the
/// JSON is not a `gp.v1.Step` or a sum does not fit, which no step Easy
/// writes under a rules table it reads can produce.
pub(crate) fn step_ms(step: &Json, tuning: &Tuning) -> Option<i64> {
    let step: Step = pharmakos_proto::json::decode_json(step).ok()?;
    match step.kind.as_ref() {
        Some(step::Kind::Interface(visit)) => visit
            .rows
            .iter()
            .try_fold(tuning.times.visit_handshake, |total, row| {
                total.checked_add(row_ms(row, tuning)?)
            }),
        Some(step::Kind::PlaceBeacon(place)) => {
            let initial = match place.initial.as_ref() {
                Some(initial) => initial_ms(initial, tuning)?,
                None => 0,
            };
            tuning.times.place_beacon_deploy.checked_add(initial)
        }
        _ => Some(0),
    }
}

/// One interface row.
fn row_ms(row: &InterfaceRow, tuning: &Tuning) -> Option<i64> {
    match row.row.as_ref() {
        Some(interface_row::Row::SetMandate(settings)) => tuning
            .times
            .switch_mandate
            .checked_add(settings_ms(settings, tuning)?),
        Some(interface_row::Row::SetMandateSettings(settings)) => settings_ms(settings, tuning),
        Some(interface_row::Row::SetPriority(_)) => Some(tuning.times.set_priority),
        Some(interface_row::Row::Recycle(_)) => Some(tuning.times.recycle),
        Some(interface_row::Row::AddBuildTarget(_) | interface_row::Row::RemoveBuildTarget(_)) => {
            Some(tuning.times.build_target)
        }
        Some(interface_row::Row::QueueStructure(_)) => Some(tuning.times.queue_structure),
        None => Some(0),
    }
}

/// A placed beacon's initial settings: the settings edit, then the priority.
fn initial_ms(initial: &InitialSettings, tuning: &Tuning) -> Option<i64> {
    let settings = match initial.mandate.as_ref() {
        Some(settings) => settings_ms(settings, tuning)?,
        None => 0,
    };
    let priority = if initial.priority == 0 {
        0
    } else {
        tuning.times.set_priority
    };
    settings.checked_add(priority)
}

/// An edit of mandate settings: the fields at the edit rate, each Build
/// target at its own.
fn settings_ms(settings: &MandateSettings, tuning: &Tuning) -> Option<i64> {
    let (fields, targets) = count(settings)?;
    let edit = match fields.checked_sub(1) {
        None => 0,
        Some(extra) => tuning
            .times
            .edit_settings_base
            .checked_add(tuning.times.edit_settings_per_field.checked_mul(extra)?)?
            .min(tuning.times.edit_settings_max),
    };
    edit.checked_add(tuning.times.build_target.checked_mul(targets)?)
}

/// One when a field is written.
fn set(written: bool) -> i64 {
    i64::from(written)
}

/// A list's elements, one field each.
fn each<T>(list: &[T]) -> Option<i64> {
    i64::try_from(list.len()).ok()
}

/// Counts added.
fn add(parts: &[i64]) -> Option<i64> {
    parts
        .iter()
        .try_fold(0_i64, |total, part| total.checked_add(*part))
}

/// The settings fields an edit writes and its Build targets, by the
/// verifier's `count` rule (the module docs).
fn count(settings: &MandateSettings) -> Option<(i64, i64)> {
    let MandateSettings {
        roe,
        retreat_hp_pct,
        mandate,
    } = settings;
    let common = add(&[set(*roe != 0), set(retreat_hp_pct.is_some())])?;
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
                add(&[
                    set(repair_threshold_pct.is_some()),
                    set(rebuild_destroyed.is_some()),
                    set(*terraform != 0),
                    each(protected_areas)?,
                ])?,
                each(targets)?,
            )
        }
        Some(mandate_settings::Mandate::Survey(survey)) => {
            let SurveySettings {
                probe_areas,
                scout_count,
            } = survey;
            (add(&[each(probe_areas)?, set(*scout_count != 0)])?, 0)
        }
        Some(mandate_settings::Mandate::Mine(mine)) => {
            let MineSettings {
                dig_max_depth,
                flee_on_threat,
                seam_choice,
                pillar_spacing,
            } = mine;
            (
                add(&[
                    set(*dig_max_depth != 0),
                    set(flee_on_threat.is_some()),
                    set(*seam_choice != 0),
                    set(*pillar_spacing != 0),
                ])?,
                0,
            )
        }
        Some(mandate_settings::Mandate::Defend(defend)) => (defend_fields(defend)?, 0),
        Some(mandate_settings::Mandate::Attack(attack)) => (attack_fields(attack)?, 0),
        None => (0, 0),
    };
    Some((common.checked_add(fields)?, targets))
}

/// The fields a Defend arm writes.
fn defend_fields(defend: &DefendSettings) -> Option<i64> {
    let DefendSettings {
        protect,
        engagement_radius_pct,
        pursue,
        picket_scouts,
    } = defend;
    add(&[
        each(protect)?,
        set(*engagement_radius_pct != 0),
        set(*pursue != 0),
        set(picket_scouts.is_some()),
    ])
}

/// The fields an Attack arm writes.
fn attack_fields(attack: &AttackSettings) -> Option<i64> {
    let AttackSettings {
        target,
        retarget,
        staging,
        launch,
        retreat,
        after_action,
        avoid_areas,
        breach_allowed,
        waves,
    } = attack;
    let launch_fields = match launch {
        Some(AttackLaunch {
            force_size,
            segment_ms,
        }) => add(&[set(force_size.is_some()), set(segment_ms.is_some())])?,
        None => 0,
    };
    let retreat_fields = match retreat {
        Some(AttackRetreat {
            force_below_pct,
            after_launch_ms,
        }) => add(&[
            set(force_below_pct.is_some()),
            set(after_launch_ms.is_some()),
        ])?,
        None => 0,
    };
    add(&[
        set(target.is_some()),
        set(*retarget != 0),
        set(staging.is_some()),
        launch_fields,
        retreat_fields,
        set(*after_action != 0),
        each(avoid_areas)?,
        set(breach_allowed.is_some()),
        set(*waves != 0),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tuning() -> Tuning {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rules/rules.v1.json");
        Tuning::read(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// A visit to `b_00` with one `set_mandate_settings` row, as the JSON a
    /// composed step is priced from.
    fn visit(settings: &str) -> Json {
        let text = format!(
            r#"{{"label":"visit","interface":{{"beacon":{{"beacon_id":"b_00"}},"rows":[{{"set_mandate_settings":{settings}}}]}},"timeout_ms":120000}}"#
        );
        let step: Step = pharmakos_proto::json::decode(&text).unwrap();
        pharmakos_proto::json::encode_json(&step).unwrap()
    }

    /// Decisions-log item 177 (3): `retreat_hp_pct` has presence, so a
    /// written 0 is one field of the edit, as the verifier's `count` and the
    /// sim's compile charge it, and an omitted one is none.
    #[test]
    fn a_written_zero_retreat_is_one_field() {
        let tuning = tuning();
        let handshake = tuning.times.visit_handshake;
        let base = tuning.times.edit_settings_base;
        assert_eq!(
            step_ms(&visit(r#"{"retreat_hp_pct":0}"#), &tuning),
            Some(handshake + base)
        );
        assert_eq!(
            step_ms(&visit(r#"{"retreat_hp_pct":40}"#), &tuning),
            Some(handshake + base)
        );
        assert_eq!(
            step_ms(&visit(r#"{"roe":"HOLD_FIRE"}"#), &tuning),
            Some(handshake + base),
            "an omitted threshold is no field"
        );
        assert_eq!(
            step_ms(&visit(r#"{"roe":"HOLD_FIRE","retreat_hp_pct":0}"#), &tuning),
            Some(handshake + base + tuning.times.edit_settings_per_field)
        );
    }
}
