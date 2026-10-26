// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The constructs the sim refuses at the seal, refused here first, each by a
//! code of its own (S2's plan, engineering call E8; task `vfy1`).
//!
//! # Why the verifier says it too
//!
//! A playbook in the v1 vocabulary can name a construct whose **effect** needs
//! a stage this build does not have: the sim's `Plan::compile` refuses it as
//! `PlanError::NotAtThisStage`, naming the construct and the stage that gives
//! it one. Until S2 the verifier let such a playbook qualify, so the editor
//! said "qualifies" and `submit_plan` then refused it at its second door as
//! `INVALID_ARGUMENT` (`crates/gateway/src/surface/planning.rs`): two verdicts
//! on one file. Each construct now has an error here, raised in QUICK so the
//! editor shows it on every edit, with a JSON Pointer to the node.
//!
//! # Keyed by the construct's name
//!
//! [`REFUSED_CONSTRUCTS`] pairs each construct, spelt exactly as the sim's
//! `NotAtThisStage { construct, .. }` spells it, with its code, and
//! [`code_for_construct`] reads it. The cross-check that holds the two lists to
//! each other is the sim's lane's (`base`, its `tests/refusals.rs` in this
//! crate): it walks the sim's refusal table and fails while any construct has
//! no code here. A lane that gives a construct its effect lifts the sim's entry
//! and this row together, and the code keeps its number, unemitted, for ever
//! (codes are append-only, [`crate::catalogue`]).
//!
//! # Exactly where the sim refuses, and nowhere else
//!
//! Each check stands where the sim's compile reaches the construct, so a file
//! the sim seals is never refused here:
//!
//! * a `broadcast`'s place is not compiled at all (radio is S4's), so nothing
//!   under one is judged;
//! * the selector in a Build target's `beacon_anchor` and every selector in an
//!   Attack arm are never compiled, because the construct around them is
//!   refused first, so they are not judged either;
//! * a `set_mandate` row whose Attack arm is empty writes nothing the sim
//!   refuses (`compile_switch_settings`): only an arm in a
//!   `set_mandate_settings` row, a `place_beacon`'s `initial`, or a non-empty
//!   one in a switch, is `E0013`.
//!
//! S2's `dfnd` lifted `E0012` (Defend's settings), `E0014` (`most_threatened`)
//! and `E0017` (`beacon_under_attack`), which the sim compiles now: a Defend
//! arm is legal wherever it is written, an empty one in a deploy's `initial`
//! included (decisions-log item 145 (5)), and the selectors in it, under
//! `most_threatened` and in a `beacon_under_attack` are judged like any other
//! (`side: ENEMY_KNOWN` in a protect list's selector is still `E0015`, beside
//! the semantic check's `E0415`, decisions-log item 146 (3) (h)).
//!
//! Nothing here reads the seat's view: every question is about the file alone,
//! which is why the stage is structure's.

use pharmakos_proto::gp::v1::beacon_filter::Side;
use pharmakos_proto::gp::v1::{
    AttackSettings, BeaconFilter, BuildTarget, InterfaceStep, Location, MandateSettings, Playbook,
    interface_row, location, mandate_settings,
};

use crate::pointer;
use crate::report::{Builder, Diag};
use crate::walk::{self, Carrier, Slot, Visit};

/// Every construct the sim refuses at the seal today, as the sim's
/// `PlanError::NotAtThisStage` spells it, with the code this crate refuses it
/// by.
///
/// Ordered by code. The words are the sim's, byte for byte, because they are
/// the key the cross-check reads; the sentence a player sees is the string
/// table's.
pub const REFUSED_CONSTRUCTS: &[(&str, &str)] = &[
    ("interface row `queue_structure`", "E0009"),
    (
        "a Build target naming a blueprint other than `generator`",
        "E0010",
    ),
    (
        "a Build target anchored on a beacon selector rather than on a voxel or `on` a vent",
        "E0011",
    ),
    ("Attack mandate settings", "E0013"),
    ("selector filter `side: ENEMY_KNOWN`", "E0015"),
    ("selector filter `tags`", "E0016"),
];

/// The code this crate refuses a construct by, keyed by the sim's name for it,
/// or `None` when the verifier has no code for it.
#[must_use]
pub fn code_for_construct(construct: &str) -> Option<&'static str> {
    REFUSED_CONSTRUCTS
        .iter()
        .find(|(name, _)| *name == construct)
        .map(|(_, code)| *code)
}

/// The blueprints a Build target may name before S4's capability catalogue:
/// the Generator and S2's base kit (`playbook.proto`'s `BuildTarget`), which
/// the sim's `compile_build_target` builds since S2's `site` lane lifted the
/// base kit's refusal (`E0019`, kept in the catalogue and raised by nothing,
/// decisions-log item 145 (5)); every other id is S4's (`E0010`).
const BUILDABLE: [&str; 4] = ["generator", "autocannon", "mortar", "wall"];

/// Run the check.
pub(crate) fn run(playbook: &Playbook, out: &mut Builder) {
    walk::walk(playbook, &mut Refusals { out });
}

struct Refusals<'a> {
    out: &'a mut Builder,
}

/// Whether a pointer lies where the sim's compile never reads a selector, so
/// that nothing there is judged:
///
/// * under a `broadcast` step, whose place the sim never compiles;
/// * under a Build target's (or a removal's) `beacon_anchor`, which the sim
///   refuses before it reads the selector in it;
/// * under an Attack arm, whose settings the sim refuses as a whole (`E0013`)
///   before it reads a selector in them (S2's wire, `con4`).
///
/// **For the lane that lifts `E0013` (S2's `atk`).** Once the sim compiles an
/// arm, it reads the selectors in it, so `/attack/` leaves this list in the
/// same pull request, as `/defend/` and `/beacon_under_attack/` left it with
/// S2's `dfnd`. An Attack target's
/// selector must rank known enemy beacons (`E0415`, the structure stage), and
/// `side: ENEMY_KNOWN` is itself `E0015` until S3's knowledge store, so from
/// then until S3 an Attack target is a handle, a voxel or the enemy
/// commander, never a selector.
///
/// A pointer token is a field name or an index, and each of `broadcast`,
/// `attack` and the pair `anchor`/`beacon_anchor` names exactly one place in
/// `gp.v1`, so the segments are unambiguous (`/attack/` is not
/// `/beacon_under_attack/`, which a pointer token never contains whole).
fn not_compiled(at: &str) -> bool {
    at.contains("/broadcast/") || at.contains("/anchor/beacon_anchor") || at.contains("/attack/")
}

impl Visit for Refusals<'_> {
    fn interface(&mut self, at: &str, interface: &InterfaceStep) {
        let rows_at = pointer::child(at, "rows");
        for (index, row) in interface.rows.iter().enumerate() {
            if let Some(interface_row::Row::QueueStructure(_)) = row.row.as_ref() {
                self.out.emit(Diag::new(
                    "E0009",
                    pointer::child(&pointer::at(&rows_at, index), "queue_structure"),
                ));
            }
        }
    }

    fn build_target(&mut self, at: &str, target: &BuildTarget) {
        if BUILDABLE.contains(&target.blueprint_id.as_str()) {
            return;
        }
        // The sim refuses an empty id as a blueprint other than `generator`;
        // here it is the missing field it is, as a queued structure's is.
        if target.blueprint_id.is_empty() {
            self.out.emit(
                Diag::new("E0101", pointer::child(at, "blueprint_id"))
                    .arg("what", "`blueprint_id`"),
            );
            return;
        }
        self.out.emit(
            Diag::new("E0010", pointer::child(at, "blueprint_id")).arg("id", &target.blueprint_id),
        );
    }

    fn location(&mut self, at: &str, place: &Location, slot: Slot<'_>) {
        if !matches!(slot, Slot::BuildAnchor { .. } | Slot::RemoveAnchor) {
            return;
        }
        let arm = match place.place.as_ref() {
            Some(location::Place::BeaconAnchor(_)) => "beacon_anchor",
            Some(location::Place::Safest(_)) => "safest",
            _ => return,
        };
        self.out
            .emit(Diag::new("E0011", pointer::child(at, arm)).arg("arm", arm));
    }

    fn mandate(&mut self, at: &str, settings: &MandateSettings, carrier: Carrier) {
        let (code, arm, empty) = match settings.mandate.as_ref() {
            Some(mandate_settings::Mandate::Attack(attack)) => {
                ("E0013", "attack", *attack == AttackSettings::default())
            }
            _ => return,
        };
        // A switch to an S2 writ with nothing in its arm writes nothing the
        // sim refuses: the switch alone is legal (the module doc).
        if carrier == Carrier::Switch && empty {
            return;
        }
        self.out.emit(Diag::new(code, pointer::child(at, arm)));
    }

    fn filter(&mut self, at: &str, filter: &BeaconFilter) {
        // In the places `not_compiled` names the sim reads no filter.
        if not_compiled(at) {
            return;
        }
        // Read raw: a number `Side` does not name is `E0018`'s, never `OWN`.
        if Side::try_from(filter.side) == Ok(Side::EnemyKnown) {
            self.out
                .emit(Diag::new("E0015", pointer::child(at, "side")));
        }
        if !filter.tags.is_empty() {
            self.out
                .emit(Diag::new("E0016", pointer::child(at, "tags")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{REFUSED_CONSTRUCTS, code_for_construct};
    use crate::catalogue::{Emitter, Stage, entry};
    use pharmakos_proto::gp::api::v1::diagnostic::Severity;

    #[test]
    fn every_refused_construct_has_an_error_the_structure_stage_raises() {
        for (construct, code) in REFUSED_CONSTRUCTS {
            let row = entry(code).unwrap_or_else(|| panic!("{code} is not in the catalogue"));
            assert_eq!(row.severity, Severity::Error, "{construct}: {code}");
            assert_eq!(
                row.emitter,
                Emitter::Stage(Stage::Structure),
                "{construct}: {code}"
            );
            assert_eq!(code_for_construct(construct), Some(*code));
        }
        assert_eq!(code_for_construct("a construct nobody refuses"), None);
    }

    #[test]
    fn a_construct_and_a_code_each_appear_once() {
        for (index, (construct, code)) in REFUSED_CONSTRUCTS.iter().enumerate() {
            for (other, (other_construct, other_code)) in REFUSED_CONSTRUCTS.iter().enumerate() {
                if index != other {
                    assert_ne!(construct, other_construct);
                    assert_ne!(code, other_code);
                }
            }
        }
    }
}
