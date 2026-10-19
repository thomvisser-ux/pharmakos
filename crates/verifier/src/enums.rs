// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `E0018`: an enum carrying a wire value this build does not name.
//!
//! Proto3's JSON accepts an enum as its number as well as its name, and prost
//! keeps an unknown number as the raw `i32` it arrived as. Its generated
//! getter (`step.pace()`, `filter.side()`) answers the enum's **default** for
//! such a number, so a stage that read through the getter would see `"pace":
//! 9` as an unset pace, and `"side": 3` as `OWN`: a value nobody wrote, read in
//! the place of one this build cannot read. That is the silent strip spec
//! section 10 forbids ("Load never strips"; AGENTS.md section 11), and the
//! sim's compile refuses the same values as `PlanError::UnknownEnum`.
//!
//! So this check reads every enum field of the playbook **raw**, by `try_from`
//! on the `i32`, and refuses a number the enum does not name, with a pointer
//! to the field. It reads every enum field of `gp.v1`'s playbook vocabulary,
//! not only the ones the sim's compile reads today, so a field that gains a
//! reader later is already covered. (`Freshness.kind` is held for S3's
//! enemy-knowledge predicates and no field of the v1 vocabulary carries one.)
//! An unset enum (zero) is the stages' own business: `E0005`, `E0007`, `E0111`
//! and the omitted-setting notes say what it means where it stands.
//!
//! It runs with the decode stage, before any stage reads an enum.

use pharmakos_proto::gp::v1::{
    BeaconFilter, Condition, FeatureRef, IntCompare, InterfaceStep, Location, MandateSettings,
    Playbook, Step, attack_settings, beacon_filter, build_settings, cmdr_hp_pct, condition,
    defend_settings, feature_ref, handler, int_compare, interface_row, location, mandate_settings,
    meta, mine_settings, move_step, on_death, on_fail, playbook, step,
};

use crate::pointer;
use crate::report::{Builder, Diag};
use crate::walk::{self, Carrier, List, Slot, Visit};

/// Run the check.
pub(crate) fn run(document: &Playbook, out: &mut Builder) {
    let mut check = Enums { out };
    check.raw::<playbook::Kind>("/kind", "kind", document.kind);
    if let Some(about) = document.meta.as_ref() {
        check.raw::<meta::AuthorKind>("/meta/author_kind", "author_kind", about.author_kind);
    }
    if let Some(death) = document.on_death.as_ref() {
        check.raw::<on_death::OnRespawn>("/on_death/on_respawn", "on_respawn", death.on_respawn);
    }
    if let Some(declarative) = document.declarative.as_ref() {
        for (index, rule) in declarative.handlers.iter().enumerate() {
            check.raw::<handler::Resume>(
                &pointer::child(&pointer::at("/declarative/handlers", index), "resume"),
                "resume",
                rule.resume,
            );
        }
    }
    walk::walk(document, &mut check);
}

struct Enums<'a> {
    out: &'a mut Builder,
}

impl Enums<'_> {
    /// `E0018` when `value` is not a number `E` names.
    fn raw<E: TryFrom<i32>>(&mut self, at: &str, field: &'static str, value: i32) {
        if E::try_from(value).is_ok() {
            return;
        }
        self.out.emit(
            Diag::new("E0018", at)
                .arg("field", field)
                .arg("value", value),
        );
    }

    /// An integer comparison's operator.
    fn compare(&mut self, at: &str, compare: Option<&IntCompare>) {
        if let Some(compare) = compare {
            self.raw::<int_compare::Op>(&pointer::child(at, "op"), "op", compare.op);
        }
    }

    /// A pick's rank and coverage.
    fn pick(&mut self, at: &str, rank: i32, coverage: i32) {
        self.raw::<feature_ref::Rank>(&pointer::child(at, "rank"), "rank", rank);
        self.raw::<feature_ref::Coverage>(&pointer::child(at, "coverage"), "coverage", coverage);
    }

    /// A feature reference's pick, if it holds one.
    fn feature(&mut self, at: &str, reference: &FeatureRef) {
        match reference.r#ref.as_ref() {
            Some(feature_ref::Ref::Vent(vent)) => {
                self.pick(&pointer::child(at, "vent"), vent.rank, vent.coverage);
            }
            Some(feature_ref::Ref::Seam(seam)) => {
                self.pick(&pointer::child(at, "seam"), seam.rank, seam.coverage);
            }
            Some(feature_ref::Ref::FeatureId(_) | feature_ref::Ref::Covered(_)) | None => {}
        }
    }
}

impl Visit for Enums<'_> {
    fn entry(&mut self, at: &str, entry: &Step, _list: List, _index: usize) {
        if let Some(fail) = entry.on_fail.as_ref() {
            self.raw::<on_fail::Action>(
                &pointer::child(&pointer::child(at, "on_fail"), "action"),
                "action",
                fail.action,
            );
        }
        match entry.kind.as_ref() {
            Some(step::Kind::Move(walk_to)) => {
                self.raw::<move_step::Pace>(
                    &pointer::child(&pointer::child(at, "move"), "pace"),
                    "pace",
                    walk_to.pace,
                );
            }
            Some(step::Kind::PlaceBeacon(place)) => {
                if let Some(initial) = place.initial.as_ref() {
                    self.raw::<interface_row::QuartermasterPriority>(
                        &pointer::child(
                            &pointer::child(&pointer::child(at, "place_beacon"), "initial"),
                            "priority",
                        ),
                        "priority",
                        initial.priority,
                    );
                }
            }
            _ => {}
        }
    }

    fn interface(&mut self, at: &str, interface: &InterfaceStep) {
        let rows_at = pointer::child(at, "rows");
        for (index, row) in interface.rows.iter().enumerate() {
            if let Some(interface_row::Row::SetPriority(priority)) = row.row.as_ref() {
                self.raw::<interface_row::QuartermasterPriority>(
                    &pointer::child(&pointer::at(&rows_at, index), "set_priority"),
                    "set_priority",
                    *priority,
                );
            }
        }
    }

    fn location(&mut self, at: &str, place: &Location, _slot: Slot<'_>) {
        match place.place.as_ref() {
            Some(location::Place::On(feature)) => self.feature(&pointer::child(at, "on"), feature),
            Some(location::Place::Covering(feature)) => {
                self.feature(&pointer::child(at, "covering"), feature);
            }
            _ => {}
        }
    }

    fn filter(&mut self, at: &str, filter: &BeaconFilter) {
        self.raw::<beacon_filter::Side>(&pointer::child(at, "side"), "side", filter.side);
        self.raw::<beacon_filter::MandateKind>(
            &pointer::child(at, "mandate"),
            "mandate",
            filter.mandate,
        );
    }

    fn mandate(&mut self, at: &str, settings: &MandateSettings, _carrier: Carrier) {
        self.raw::<mandate_settings::Roe>(&pointer::child(at, "roe"), "roe", settings.roe);
        match settings.mandate.as_ref() {
            Some(mandate_settings::Mandate::Build(build)) => {
                self.raw::<build_settings::Terraform>(
                    &pointer::child(&pointer::child(at, "build"), "terraform"),
                    "terraform",
                    build.terraform,
                );
            }
            Some(mandate_settings::Mandate::Mine(mine)) => {
                self.raw::<mine_settings::SeamChoice>(
                    &pointer::child(&pointer::child(at, "mine"), "seam_choice"),
                    "seam_choice",
                    mine.seam_choice,
                );
            }
            // S2's settings (`con4`): read raw like every other enum, although
            // the sim refuses the whole arm until S2's Defend and Attack lanes,
            // so that the day they are read, an unknown number already is not.
            Some(mandate_settings::Mandate::Defend(defend)) => {
                self.raw::<defend_settings::Pursue>(
                    &pointer::child(&pointer::child(at, "defend"), "pursue"),
                    "pursue",
                    defend.pursue,
                );
            }
            Some(mandate_settings::Mandate::Attack(attack)) => {
                let here = pointer::child(at, "attack");
                self.raw::<attack_settings::Retarget>(
                    &pointer::child(&here, "retarget"),
                    "retarget",
                    attack.retarget,
                );
                self.raw::<attack_settings::AfterAction>(
                    &pointer::child(&here, "after_action"),
                    "after_action",
                    attack.after_action,
                );
            }
            Some(mandate_settings::Mandate::Survey(_)) | None => {}
        }
    }

    fn condition(&mut self, at: &str, node: &Condition) {
        match node.node.as_ref() {
            Some(condition::Node::CmdrHpPct(predicate)) => {
                self.raw::<cmdr_hp_pct::Cmp>(
                    &pointer::child(&pointer::child(at, "cmdr_hp_pct"), "cmp"),
                    "cmp",
                    predicate.cmp,
                );
            }
            Some(condition::Node::CmdrDeaths(predicate)) => self.compare(
                &pointer::child(&pointer::child(at, "cmdr_deaths"), "deaths"),
                predicate.deaths.as_ref(),
            ),
            Some(condition::Node::SegmentElapsed(predicate)) => self.compare(
                &pointer::child(&pointer::child(at, "segment_elapsed"), "ms"),
                predicate.ms.as_ref(),
            ),
            Some(condition::Node::BeaconHpPct(predicate)) => self.compare(
                &pointer::child(&pointer::child(at, "beacon_hp_pct"), "pct"),
                predicate.pct.as_ref(),
            ),
            Some(condition::Node::Treasury(predicate)) => self.compare(
                &pointer::child(&pointer::child(at, "treasury"), "dollars"),
                predicate.dollars.as_ref(),
            ),
            Some(condition::Node::KwHeadroom(predicate)) => self.compare(
                &pointer::child(&pointer::child(at, "kw_headroom"), "kw"),
                predicate.kw.as_ref(),
            ),
            _ => {}
        }
    }
}
