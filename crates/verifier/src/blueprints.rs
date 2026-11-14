// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The blueprints this build knows, by the id a playbook writes.
//!
//! A blueprint id is `gp.v1.RulesTable.Structures`' spelling of one of the
//! sim's [`StructureKind`]s (`BuildTarget.blueprint_id`), and the predicates
//! S3's `con6` added name one the same way (`BeaconHasStructure`,
//! `EnemyStructureNear`). A predicate may ask about any kind the sim defines,
//! built by a slot or not: which beacon has what is the Push's to answer, so
//! only an id that names no kind at all is refused (`E0506`, S3's `vfy6`).

use pharmakos_sim::tables::StructureKind;

/// Every kind the sim defines ([`StructureKind::ALL`]), with the id a
/// playbook writes for it, in the sim's order.
const IDS: [(&str, StructureKind); 6] = [
    ("generator", StructureKind::Generator),
    ("autocannon", StructureKind::Autocannon),
    ("mortar", StructureKind::Mortar),
    ("survey_post", StructureKind::SurveyPost),
    (
        pharmakos_sim::spire::SPIRE_BLUEPRINT,
        StructureKind::ResonanceSpire,
    ),
    ("wall", StructureKind::Wall),
];

/// The kind a blueprint id names, or `None` for an id this build does not
/// know.
#[must_use]
pub(crate) fn kind_of(id: &str) -> Option<StructureKind> {
    IDS.iter()
        .find(|(known, _)| *known == id)
        .map(|(_, kind)| *kind)
}

#[cfg(test)]
mod tests {
    use super::{IDS, kind_of};
    use pharmakos_sim::tables::StructureKind;

    #[test]
    fn every_kind_the_sim_defines_has_one_id_in_its_order() {
        let kinds: Vec<StructureKind> = IDS.iter().map(|(_, kind)| *kind).collect();
        assert_eq!(kinds, StructureKind::ALL);
        for (id, kind) in IDS {
            assert_eq!(kind_of(id), Some(kind));
        }
        assert_eq!(kind_of("death_ray"), None);
        assert_eq!(kind_of(""), None);
    }
}
