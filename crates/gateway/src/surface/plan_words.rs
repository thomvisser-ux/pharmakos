// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The gateway's own sentence for a `PlanError`, in a module of its own (S3's
//! `vfyq2`, fix pass 5, item L3).
//!
//! The sim's `Display` for its compile refusal writes the player's ids and
//! labels raw. So this file is the one place in the gateway that names the
//! type, and it says one with `said!` alone: `tests/confinement.rs` holds the
//! type's name to this file, holds this file to no `format!`,
//! `format_args!`, `write!`, `to_string` or `Debug`, and holds every call of
//! a sim function that refuses with one to pass its error straight here.

use pharmakos_proto::json::grammar::{Said, quoted};
use pharmakos_proto::said;

/// The sim's reading of a beacon reference, its refusal in the gateway's
/// own sentence (S3's `vfyq2`, fix pass 6, item 5): the one place the
/// gateway reaches `beacon_spec_of`, so its `PlanError` is never held
/// anywhere else.
///
/// # Errors
///
/// [`plan_refusal`]'s sentence for the sim's refusal.
pub(crate) fn beacon_spec(
    reference: &pharmakos_proto::gp::v1::BeaconRef,
    rules: &pharmakos_sim::rules::RulesTable,
) -> Result<pharmakos_sim::interpreter::BeaconSpec, Said> {
    pharmakos_sim::interpreter::beacon_spec_of(reference, rules)
        .map_err(|error| plan_refusal(&error))
}

/// A `PlanError` as a seat reads it (S3's `vfyq2`, fix pass 3, item M2 (a)):
/// one sentence per variant, matched with no wildcard, so a variant the sim
/// adds does not compile here until it has one. The words are the sim's own
/// (its `Display`), but composed from the variant's fields with `said!`, and
/// every field that is the player's text (a blueprint id, a label, a handler
/// id, a beacon id, a feature id) is [`quoted`]: the sim's `Display` writes
/// them raw, and the sim is not this lane's to change. The `&'static str`
/// fields are the sim's own words.
#[allow(
    clippy::too_many_lines,
    reason = "one arm per PlanError variant, each the sentence a seat reads; split, the \
              sentences would be spelled in two places"
)]
#[must_use]
pub fn plan_refusal(error: &pharmakos_sim::interpreter::PlanError) -> Said {
    use pharmakos_sim::interpreter::{ClearRefusal, PlanError as E};
    match error {
        E::MissingBlock(name) => said!(
            "the `{}` block is missing; the interpreter reads it and will not substitute a \
             default",
            *name
        ),
        E::UnsetEnum(name) => said!("`{}` is unset, and an unset enum is an error", *name),
        E::UnknownEnum { field, value } => said!(
            "`{}` carries {}, which this build does not name; a value it cannot read is refused \
             rather than defaulted",
            *field,
            *value
        ),
        E::StepWithoutAction => "a step names no action".into(),
        E::UnknownBlueprint(id) => said!("`{}` names no blueprint this build knows", quoted(id)),
        E::BlueprintNeverStands(id) => said!(
            "`{}` never stands as a structure, so a predicate asking about one could never be \
             true",
            quoted(id)
        ),
        E::NoRadius(name) => said!(
            "`{}` is 0, which is no radius; an enemy predicate's radius is required",
            *name
        ),
        E::WindowUnderVisibleNow { found } => said!(
            "`known_within_ms` is {} ms, and a freshness of VISIBLE_NOW reads no window",
            *found
        ),
        E::NegativeDuration(name) => {
            said!("`{}` is negative; durations are game milliseconds", *name)
        }
        E::WaitWithoutTimeout => "a `wait_until` carries no timeout; every wait has one, which \
                                  is half of why every playbook halts"
            .into(),
        E::BackwardJump { from, to } => said!(
            "a jump from route step {} names step {}; a jump only ever goes forward",
            *from,
            *to
        ),
        E::VoxelOutOfMap { axis, value, limit } => said!(
            "a voxel's {} of {} is outside the map, which runs 0 to {}",
            *axis,
            *value,
            limit.saturating_sub(1)
        ),
        E::VoxelOutOfRange { axis, value } => said!(
            "a voxel's {} of {} is past the sim's voxel coordinates",
            *axis,
            *value
        ),
        E::NegativeRate(field) => said!(
            "the rules table's `{}` is negative; an interface time is a duration",
            *field
        ),
        E::RowTooLong => {
            "a row would take longer to commit than a signed 32-bit count of milliseconds".into()
        }
        E::ScoutsOutOfRange { found, most } => said!(
            "a `scout_count` or `picket_scouts` of {} is past the {} a beacon holds",
            *found,
            *most
        ),
        E::AttackTargetNotEnemy => "an Attack target names no enemy beacon: it is an enemy \
                                    beacon's `e_NN`, a voxel, the enemy commander or the nearest \
                                    known enemy beacon"
            .into(),
        E::CommanderSeatOutOfRange { found } => said!(
            "an `enemy_commander.seat` of {} is past any seat a match holds",
            *found
        ),
        E::ForceBelowOutOfRange { found } => said!(
            "a `force_below_pct` of {} is outside 1 to 100 percent of the launch force",
            *found
        ),
        E::EngagementOutOfRange { found } => said!(
            "an `engagement_radius_pct` of {} is past 100; it is a percentage of the sphere \
             radius, 1 to 100",
            *found
        ),
        E::ClearRefused { list, why } => match why {
            ClearRefusal::BesideAList => said!(
                "`clear_{}` stands beside a `{}` that names entries; a row either replaces the \
                 list or empties it",
                *list,
                *list
            ),
            ClearRefusal::NothingToClear => said!(
                "`clear_{}` stands in settings that start from nothing, where there is no `{}` \
                 to empty; it belongs in a `set_mandate_settings` row",
                *list,
                *list
            ),
        },
        E::RetreatOutOfRange { found } => said!(
            "a `retreat_hp_pct` of {} is past 100 percent of a unit's hit points",
            *found
        ),
        E::RotationOutOfRange { found } => said!(
            "a `rotation_quarter_turns` of {} is past three quarter turns",
            *found
        ),
        E::WallBoxOffAWall { blueprint } => said!(
            "a `{}` target carries a `wall_box`; only a `wall` target fills a box",
            quoted(blueprint)
        ),
        E::AreaInsideOut => "an area's `min` exceeds its `max` in x or y".into(),
        E::RepairThresholdOutOfRange { found } => said!(
            "a `repair_threshold_pct` of {} is past 100 percent of a structure's hit points",
            *found
        ),
        E::WallWithoutBox => "a `wall` target carries no `wall_box`".into(),
        E::WallBoxInsideOut => "a `wall_box`'s `min.z` exceeds its `max.z`".into(),
        E::WallTurned { found } => said!(
            "a `wall` target is turned {} quarter turns; a wall's box is not turned",
            *found
        ),
        E::WallAnchorOutsideBox => {
            "a `wall` target's anchor is no voxel inside its `wall_box`".into()
        }
        E::UnknownLabel(label) => {
            said!("`{}` names no step in this playbook's route", quoted(label))
        }
        E::UnknownHandler(id) => said!("`{}` names no handler in this playbook", quoted(id)),
        E::UnknownFeature(id) => said!(
            "`{}` is not a feature name of the `vent_<x>_<y>` or `seam_<x>_<y>` form",
            quoted(id)
        ),
        E::MisplacedTarget(why) => (*why).into(),
        E::UnknownBeacon(id) => said!(
            "`{}` is not a beacon reference of the `b_NN` or `e_NN` form",
            quoted(id)
        ),
        E::CooldownTooShort { found, least } => said!(
            "a handler's cooldown of {} ms is below the {} ms the rules table sets",
            *found,
            *least
        ),
        E::FiresOutOfRange { found, most } => said!(
            "a handler's `max_fires` of {} is outside 1 to {}",
            *found,
            *most
        ),
        E::ConditionTooDeep { limit } => said!("a condition tree nests deeper than {}", *limit),
        E::ConditionTooBig { limit } => said!("a condition tree holds more than {} nodes", *limit),
        E::TooManySteps { found, limit } => said!(
            "the route holds {} steps; the ceiling is {}",
            *found,
            *limit
        ),
        E::TooManyHandlers { found, limit } => said!(
            "the playbook holds {} handlers; the ceiling is {}",
            *found,
            *limit
        ),
        E::NotAtThisStage { construct, stage } => said!(
            "{} is in the v1 vocabulary but has no effect until {}; it is refused rather than \
             silently ignored",
            *construct,
            *stage
        ),
    }
}
