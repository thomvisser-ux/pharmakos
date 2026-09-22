// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The one place the world's types become the wire's.
//!
//! Every method of the slice answers with `gp.api.v1` shapes over `gp.v1`
//! values, and every one of those conversions is a decision: a fixed-point
//! position becomes a whole voxel by *flooring*, a [`BeaconId`] becomes the
//! `b_04` the spec's worked example types, a [`MandateKind`] of the sim becomes
//! the one the verifier's [`KnownBeacon`] carries. Written once here rather
//! than four times in four handlers, because two handlers that round a
//! position differently would show a player a beacon in two places.
//!
//! Nothing here reads a rules row, steps anything or decides who may see what.
//! Fog is [`crate::fog`]'s and the tick is [`crate::time`]'s.

use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::gp::v1::beacon_filter::MandateKind as WireMandate;
use pharmakos_sim::knowledge::Position;
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::tables::{BeaconId, SeatId, StructureKind, UnitKind};

/// How many digits a rendered beacon id pads to.
///
/// Two, so the spec's own `get_beacon{b_04}` is what this gateway hands out
/// for beacon 4. A match with a hundred beacons writes three digits and reads
/// back correctly either way -- [`beacon_id_from`] parses the number, not the
/// padding, so `b_4` and `b_04` are the same beacon and `b_004` is too.
pub const BEACON_ID_DIGITS: usize = 2;

/// The prefix every beacon id carries.
pub const BEACON_ID_PREFIX: &str = "b_";

/// A fixed-point position as the whole voxel it stands in.
///
/// **Floors**, on every axis, including below zero -- [`Fx::floor_voxels`] is
/// the sim's own rule and this is the only rounding the wire ever sees. A
/// truncation toward zero would put a unit at `x = -0.5` in voxel 0 and a unit
/// at `x = 0.5` in voxel 0 as well, which is two different places with one
/// name.
#[must_use]
pub fn voxel_of(at: [Fx; 3]) -> Voxel {
    let axis = |index: usize| at.get(index).map_or(0, |value| value.floor_voxels());
    Voxel {
        x: axis(0),
        y: axis(1),
        z: axis(2),
    }
}

/// A knowledge position as the whole voxel it stands in.
#[must_use]
pub fn voxel_of_position(at: Position) -> Voxel {
    voxel_of(at.to_array())
}

/// A beacon's id on the wire: `b_04`.
#[must_use]
pub fn beacon_id(beacon: BeaconId) -> String {
    format!(
        "{BEACON_ID_PREFIX}{:0width$}",
        beacon.raw(),
        width = BEACON_ID_DIGITS
    )
}

/// The beacon a wire id names, or `None` when the text is not one.
///
/// Decided on **characters**, never on a parse that would also take `b_+4`,
/// `b_ 4` or `b_4\n`: the digits must be ASCII digits and nothing else, which
/// is the same rule `plan-core`'s template ids are decided by and for the same
/// cross-platform reason (decisions-log item 100's closing note).
#[must_use]
pub fn beacon_id_from(text: &str) -> Option<BeaconId> {
    let digits = text.strip_prefix(BEACON_ID_PREFIX)?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse::<u32>().ok().map(BeaconId::new)
}

/// The sim's mandate as the one `gp.v1.BeaconFilter` spells.
///
/// The verifier's [`pharmakos_verifier::KnownBeacon`] carries the wire enum,
/// so the translation happens here rather than in the handler that builds a
/// scope. A mandate this build does not map is `UNSPECIFIED` rather than a
/// guess: an unset enum is a verifier error and never a default
/// (`playbook.proto`), so handing it one it can complain about is right and
/// inventing `BUILD` would not be.
#[must_use]
pub const fn mandate_of(mandate: MandateKind) -> WireMandate {
    match mandate {
        MandateKind::None => WireMandate::Unspecified,
        MandateKind::Build => WireMandate::Build,
        MandateKind::Defend => WireMandate::Defend,
        MandateKind::Attack => WireMandate::Attack,
        MandateKind::Mine => WireMandate::Mine,
        MandateKind::Survey => WireMandate::Survey,
    }
}

/// A mandate, as a sentence names it.
///
/// English, from the one string table's side of this crate, and not the
/// `Debug` spelling: a `Debug` derive is for a developer reading a panic and
/// changes whenever somebody renames a variant.
#[must_use]
pub const fn mandate_name(mandate: MandateKind) -> &'static str {
    match mandate {
        MandateKind::None => "no",
        MandateKind::Build => "Build",
        MandateKind::Defend => "Defend",
        MandateKind::Attack => "Attack",
        MandateKind::Mine => "Mine",
        MandateKind::Survey => "Survey",
    }
}

/// The mandate a stored wire id names, or [`MandateKind::None`] for one this
/// build does not define.
#[must_use]
pub fn mandate_from_id(id: u8) -> MandateKind {
    match id {
        1 => MandateKind::Build,
        2 => MandateKind::Defend,
        3 => MandateKind::Attack,
        4 => MandateKind::Mine,
        5 => MandateKind::Survey,
        _ => MandateKind::None,
    }
}

/// A unit kind as `gp.api.v1.ViewEntity.subtype` spells it.
///
/// `lower_snake_case`, like every other free-form name this project puts on
/// the wire, and written out here rather than taken from a `Debug` derive: a
/// `Debug` spelling changes whenever somebody renames a variant, and a client
/// draws a model from this string.
#[must_use]
pub const fn unit_subtype(kind: UnitKind) -> &'static str {
    match kind {
        UnitKind::Commander => "commander",
        UnitKind::BuildDrone => "build_drone",
        UnitKind::MiningDrone => "mining_drone",
        UnitKind::RepairDrone => "repair_drone",
        UnitKind::Raider => "raider",
        UnitKind::Scout => "scout",
    }
}

/// A structure kind as `gp.api.v1.ViewEntity.subtype` spells it.
#[must_use]
pub const fn structure_subtype(kind: StructureKind) -> &'static str {
    match kind {
        StructureKind::Generator => "generator",
        StructureKind::Autocannon => "autocannon",
        StructureKind::Mortar => "mortar",
        StructureKind::SurveyPost => "survey_post",
        StructureKind::ResonanceSpire => "resonance_spire",
        StructureKind::Wall => "wall",
    }
}

/// A seat as `gp.api.v1.ViewEntity.owner` spells it: `seat.0`.
///
/// The same spelling [`crate::token::Subject::render`] writes into the audit
/// log, because no answer of this gateway has ever carried a bare seat
/// *number* and one would be a second way to name the same thing. Seats are
/// zero-based in every golden this project has.
#[must_use]
pub fn owner_text(seat: SeatId) -> String {
    format!("seat.{}", seat.raw())
}

/// A 64-bit match seed as the quoted hexadecimal `gp.api.v1` carries.
///
/// `0x` and sixteen lower-case digits, which is the spelling
/// `scenarios/**.scenario.jsonc` uses for the same number, so the two compare
/// by eye.
#[must_use]
pub fn seed_text(seed: u64) -> String {
    format!("0x{seed:016x}")
}

#[cfg(test)]
mod tests {
    use super::{
        beacon_id, beacon_id_from, mandate_from_id, mandate_name, mandate_of, seed_text, voxel_of,
        voxel_of_position,
    };
    use pharmakos_proto::gp::v1::beacon_filter::MandateKind as WireMandate;
    use pharmakos_sim::knowledge::Position;
    use pharmakos_sim::math::fixed::Fx;
    use pharmakos_sim::seams::MandateKind;
    use pharmakos_sim::tables::BeaconId;

    #[test]
    fn a_position_floors_onto_its_voxel_on_every_axis() {
        // Half a voxel, written as a shift rather than a division: `integer_division`
        // is denied because rounding must be a decision, and a shift has none to make.
        let half = Fx::from_raw(Fx::ONE.raw() >> 1_i32);
        let at = [
            Fx::from_voxels(3).saturating_add(half),
            Fx::from_voxels(-1).saturating_add(half),
            Fx::from_voxels(-1),
        ];
        let voxel = voxel_of(at);
        assert_eq!((voxel.x, voxel.y, voxel.z), (3, -1, -1));
        assert_eq!(voxel_of_position(Position::from_array(at)), voxel);
    }

    #[test]
    fn a_beacon_id_is_the_specs_own_spelling_and_round_trips() {
        assert_eq!(beacon_id(BeaconId::new(4)), "b_04");
        assert_eq!(beacon_id(BeaconId::new(123)), "b_123");
        assert_eq!(beacon_id_from("b_04"), Some(BeaconId::new(4)));
        assert_eq!(beacon_id_from("b_4"), Some(BeaconId::new(4)));
        for hostile in ["", "b_", "04", "b_+4", "b_ 4", "b_4x", "B_04", "b_-1"] {
            assert_eq!(beacon_id_from(hostile), None, "`{hostile}` is not an id");
        }
    }

    #[test]
    fn a_mandate_maps_both_ways_and_an_unknown_one_is_unspecified() {
        assert_eq!(mandate_of(MandateKind::Build), WireMandate::Build);
        assert_eq!(mandate_of(MandateKind::None), WireMandate::Unspecified);
        assert_eq!(mandate_from_id(4), MandateKind::Mine);
        assert_eq!(mandate_from_id(200), MandateKind::None);
        assert_eq!(mandate_name(MandateKind::Build), "Build");
        assert_eq!(mandate_name(MandateKind::None), "no");
        for kind in [
            MandateKind::Build,
            MandateKind::Defend,
            MandateKind::Attack,
            MandateKind::Mine,
            MandateKind::Survey,
        ] {
            assert_eq!(mandate_from_id(kind.id()), kind);
        }
    }

    #[test]
    fn a_seed_reads_as_the_scenario_files_spell_it() {
        assert_eq!(seed_text(0x0000_0000_ca5c_aded), "0x00000000ca5caded");
    }
}
