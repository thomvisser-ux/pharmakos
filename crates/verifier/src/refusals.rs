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
//! * the selector in a Build target's `beacon_anchor` is never compiled,
//!   because the construct around it is refused first, so it is not judged
//!   either.
//!
//! S2's `atk` lifted `E0013` (Attack's settings), which the sim compiles now:
//! an Attack arm is legal wherever it is written, an empty one in a deploy's
//! `initial` included (decisions-log item 146 (3) (j)), and the selectors in
//! it are judged like any other (an Attack target's selector ranking own
//! beacons is the structure stage's `E0415`, and `side: ENEMY_KNOWN` is still
//! `E0015`, so until S3 an Attack target is a handle, a voxel or the enemy
//! commander).
//!
//! S2's `atk2` (decisions-log item 150 (2)) lifted `E0015` for exactly one
//! shape, which the sim compiles now: `nearest` with the filter
//! `side: ENEMY_KNOWN` and no other filter field, as an Attack target
//! ([`is_nearest_known_enemy_beacon`]). The sim chooses that beacon at launch;
//! nothing here ranks it. `weakest` and `most_threatened` over enemy beacons,
//! this filter with a `mandate` or `tags`, and `side: ENEMY_KNOWN` anywhere
//! but an Attack target are still `E0015`, as the sim still refuses them.
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
    BeaconFilter, BeaconRef, BuildTarget, InterfaceStep, Location, Playbook, beacon_ref,
    interface_row, location,
};

use crate::pointer;
use crate::report::{Builder, Diag};
use crate::walk::{self, Slot, Visit};

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
///   refuses before it reads the selector in it.
///
/// S2's `atk` took `/attack/` out of this list as it lifted `E0013`, as S2's
/// `dfnd` took `/defend/` and `/beacon_under_attack/` out with `E0012` and
/// `E0017`: the sim reads the selectors in an Attack arm now.
///
/// A pointer token is a field name or an index, and `broadcast` and the pair
/// `anchor`/`beacon_anchor` each name exactly one place in `gp.v1`, so the
/// segments are unambiguous.
fn not_compiled(at: &str) -> bool {
    at.contains("/broadcast/") || at.contains("/anchor/beacon_anchor")
}

/// Whether a beacon reference is "the nearest known enemy beacon": `nearest`
/// with the filter `side: ENEMY_KNOWN` and no other filter field, the one
/// selector the sim takes as an Attack target before S3 (S2's `atk2`; the
/// sim's `nearest_known_enemy_beacon` in `interpreter.rs` reads the same
/// fields the same way). It names the selector; which beacon it is, is the
/// sim's, at launch.
#[must_use]
pub(crate) fn is_nearest_known_enemy_beacon(beacon: &BeaconRef) -> bool {
    let Some(beacon_ref::Ref::Nearest(nearest)) = beacon.r#ref.as_ref() else {
        return false;
    };
    nearest.filter.as_ref().is_some_and(lifted_filter)
}

/// The filter half of [`is_nearest_known_enemy_beacon`], read raw: a number
/// `Side` or `MandateKind` does not name is not this filter.
fn lifted_filter(filter: &BeaconFilter) -> bool {
    Side::try_from(filter.side) == Ok(Side::EnemyKnown)
        && filter.mandate == 0
        && filter.tags.is_empty()
}

/// Whether a filter at `at` is the filter of an Attack target's `nearest`
/// selector. A pointer token is a field name or an index, `attack` names
/// exactly one place in `gp.v1` (`MandateSettings.attack`), and `target`,
/// `beacon` and `nearest` under it name one path each, so the suffix is
/// unambiguous.
fn attack_target_nearest(at: &str) -> bool {
    at.ends_with("/attack/target/beacon/nearest/filter")
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

    fn filter(&mut self, at: &str, filter: &BeaconFilter) {
        // In the places `not_compiled` names the sim reads no filter.
        if not_compiled(at) {
            return;
        }
        // Read raw: a number `Side` does not name is `E0018`'s, never `OWN`.
        // The nearest known enemy beacon as an Attack target is the one
        // shape the sim compiles (S2's `atk2`).
        if Side::try_from(filter.side) == Ok(Side::EnemyKnown)
            && !(attack_target_nearest(at) && lifted_filter(filter))
        {
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
