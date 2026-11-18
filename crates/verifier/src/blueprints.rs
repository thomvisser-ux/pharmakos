// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The blueprints this build knows, by the id a playbook writes, and where a
//! playbook may name each one.
//!
//! A blueprint id is `gp.v1.RulesTable.Structures`' spelling of one of the
//! sim's [`StructureKind`]s (`BuildTarget.blueprint_id`), and the predicates
//! S3's `con6` added name one the same way (`BeaconHasStructure`,
//! `EnemyStructureNear`). A predicate may ask about any kind the sim defines,
//! built by a slot or not: which beacon has what is the Push's to answer, so
//! only an id that names no kind at all is refused (`E0506`, S3's `vfy6`).
//!
//! # Where each one may stand: [`BLUEPRINT_SLOTS`]
//!
//! The table is the verifier's per-slot legality, public so that a client
//! that offers a player a choice of blueprint reads the verifier's answer
//! rather than asking it over a probe playbook or keeping a list of its own
//! (S3's `vfy6`, follow-up E; `plan-core`'s `vocabulary::blueprints_by_slot`
//! is its first reader). The checks read it too, so the two cannot drift:
//! `E0009` refuses a `queue_structure` row for a kind it does not mark
//! `queued`, `E0010` a Build target for a kind it does not mark
//! `build_target`, `E0011` a Build target anchored on a beacon selector or
//! `safest`, and `E0417` a wall's anchor that is not a voxel inside its box.
//! `tests/blueprints.rs` holds every row to the refusals, and the `queued`
//! column to the sim's own [`pharmakos_sim::spire::queued_kind`].

use pharmakos_sim::tables::StructureKind;

/// How a Build target naming a blueprint is anchored.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Anchor {
    /// On a voxel, or `on` a feature: `E0011` refuses a beacon selector or
    /// `safest`, which the sim refuses before it reads them.
    VoxelOrFeature,
    /// On a voxel inside the target's `wall_box`, which the target must
    /// carry: `E0417` refuses any other anchor. Only a wall fills a box, and
    /// `E0509` refuses a box on any other blueprint.
    VoxelInsideBox,
}

/// Where a playbook may name one blueprint, as this build's checks judge it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BlueprintSlots {
    /// The sim's kind.
    pub kind: StructureKind,
    /// The id a playbook writes.
    pub id: &'static str,
    /// Whether a Build target may name it: `E0010` refuses every other id.
    pub build_target: bool,
    /// Whether a `queue_structure` row may name it: `E0009` refuses every
    /// other id. The sim's [`pharmakos_sim::spire::queued_kind`] says the
    /// same, and a test holds the two together.
    pub queued: bool,
    /// How a Build target naming it is anchored.
    pub anchor: Anchor,
}

/// Every kind the sim defines ([`StructureKind::ALL`]), in the sim's order,
/// with the id a playbook writes for it and the slots that may name it.
///
/// A Build target may name the Generator and S2's base kit
/// (`playbook.proto`'s `BuildTarget`), which the sim's `compile_build_target`
/// builds since S2's `site` lane lifted the base kit's refusal (`E0019`, kept
/// in the catalogue and raised by nothing, decisions-log item 145 (5)); every
/// other id is S4's capability catalogue's. A queue may name the Resonance
/// Spire, since S3's `lic` gave it an effect; its licence and its one per
/// beacon are the semantic stage's (`E0507`, `E0513`).
pub const BLUEPRINT_SLOTS: [BlueprintSlots; 6] = [
    BlueprintSlots {
        kind: StructureKind::Generator,
        id: "generator",
        build_target: true,
        queued: false,
        anchor: Anchor::VoxelOrFeature,
    },
    BlueprintSlots {
        kind: StructureKind::Autocannon,
        id: "autocannon",
        build_target: true,
        queued: false,
        anchor: Anchor::VoxelOrFeature,
    },
    BlueprintSlots {
        kind: StructureKind::Mortar,
        id: "mortar",
        build_target: true,
        queued: false,
        anchor: Anchor::VoxelOrFeature,
    },
    BlueprintSlots {
        kind: StructureKind::SurveyPost,
        id: "survey_post",
        build_target: false,
        queued: false,
        anchor: Anchor::VoxelOrFeature,
    },
    BlueprintSlots {
        kind: StructureKind::ResonanceSpire,
        id: pharmakos_sim::spire::SPIRE_BLUEPRINT,
        build_target: false,
        queued: true,
        anchor: Anchor::VoxelOrFeature,
    },
    BlueprintSlots {
        kind: StructureKind::Wall,
        id: "wall",
        build_target: true,
        queued: false,
        anchor: Anchor::VoxelInsideBox,
    },
];

impl BlueprintSlots {
    /// Whether a structure row of this kind can stand in this build: one a
    /// Build target or a queue raises, and not the one that fills a box (a
    /// wall, which is voxels). A predicate may name only such a kind (`E0514`;
    /// S3's `pred`, fix pass 1); the sim's `StructureKind::can_stand_as_a_row`
    /// says the same, and a test holds the two together.
    #[must_use]
    pub const fn stands_as_a_structure(&self) -> bool {
        (self.build_target || self.queued) && !matches!(self.anchor, Anchor::VoxelInsideBox)
    }
}

/// The row for a kind: every kind the sim defines has one, in
/// [`StructureKind::ALL`]'s order, so this cannot miss.
#[must_use]
pub const fn slots_for(kind: StructureKind) -> &'static BlueprintSlots {
    match kind {
        StructureKind::Generator => &BLUEPRINT_SLOTS[0],
        StructureKind::Autocannon => &BLUEPRINT_SLOTS[1],
        StructureKind::Mortar => &BLUEPRINT_SLOTS[2],
        StructureKind::SurveyPost => &BLUEPRINT_SLOTS[3],
        StructureKind::ResonanceSpire => &BLUEPRINT_SLOTS[4],
        StructureKind::Wall => &BLUEPRINT_SLOTS[5],
    }
}

/// The row for a blueprint id, or `None` for an id this build does not know.
#[must_use]
pub fn slots_of(id: &str) -> Option<&'static BlueprintSlots> {
    BLUEPRINT_SLOTS.iter().find(|row| row.id == id)
}

/// Whether a Build target may name `id` (`E0010` otherwise).
#[must_use]
pub(crate) fn takes_build_target(id: &str) -> bool {
    slots_of(id).is_some_and(|row| row.build_target)
}

/// Whether a `queue_structure` row may name `id` (`E0009` otherwise).
#[must_use]
pub(crate) fn takes_queue(id: &str) -> bool {
    slots_of(id).is_some_and(|row| row.queued)
}

/// Whether a Build target naming `id` fills a box (`BuildTarget.wall_box`).
#[must_use]
pub(crate) fn fills_a_box(id: &str) -> bool {
    slots_of(id).is_some_and(|row| row.anchor == Anchor::VoxelInsideBox)
}

#[cfg(test)]
mod tests {
    use super::{BLUEPRINT_SLOTS, slots_of};
    use pharmakos_sim::tables::StructureKind;

    #[test]
    fn every_kind_the_sim_defines_has_one_id_in_its_order() {
        let kinds: Vec<StructureKind> = BLUEPRINT_SLOTS.iter().map(|row| row.kind).collect();
        assert_eq!(kinds, StructureKind::ALL);
        for row in BLUEPRINT_SLOTS {
            assert_eq!(slots_of(row.id).map(|found| found.kind), Some(row.kind));
            assert_eq!(slots_of(row.id), Some(&row));
            assert_eq!(super::slots_for(row.kind), &row);
        }
        assert_eq!(slots_of("death_ray").map(|found| found.kind), None);
        assert_eq!(slots_of("").map(|found| found.kind), None);
    }
}
