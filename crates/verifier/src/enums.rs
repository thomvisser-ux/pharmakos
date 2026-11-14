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
//! reader later is already covered. S3's `con6` predicates and `toward` are
//! read here while the sim refuses them; a selector inside one is not, because
//! the walk does not enter a refused predicate until S3's `vfy6`.
//! An unset enum (zero) is the stages' own business: `E0005`, `E0007`, `E0111`
//! and the omitted-setting notes say what it means where it stands.
//!
//! It runs with the decode stage, before any stage reads an enum.

use pharmakos_proto::gp::v1::{
    BeaconFilter, Condition, FeatureRef, Freshness, IntCompare, InterfaceStep, Location,
    MandateSettings, Playbook, Step, attack_settings, beacon_filter, beacon_units_by_role,
    build_settings, cmdr_hp_pct, condition, defend_settings, feature_ref, freshness, handler,
    int_compare, interface_row, location, mandate_settings, meta, mine_settings, move_step,
    on_death, on_fail, playbook, step,
};

use crate::pointer;
use crate::report::{Builder, Diag};
use crate::walk::{self, Carrier, List, Slot, Visit};

/// Run the check.
pub(crate) fn run(document: &Playbook, out: &mut Builder) {
    let mut check = Enums { out };
    check.raw::<playbook::Kind>("/kind", &[], "kind", document.kind);
    if let Some(about) = document.meta.as_ref() {
        check.raw::<meta::AuthorKind>("/meta/author_kind", &[], "author_kind", about.author_kind);
    }
    if let Some(death) = document.on_death.as_ref() {
        check.raw::<on_death::OnRespawn>(
            "/on_death/on_respawn",
            &[],
            "on_respawn",
            death.on_respawn,
        );
    }
    if let Some(declarative) = document.declarative.as_ref() {
        for (index, rule) in declarative.handlers.iter().enumerate() {
            if handler::Resume::try_from(rule.resume).is_err() {
                check.raw::<handler::Resume>(
                    &pointer::at("/declarative/handlers", index),
                    &["resume"],
                    "resume",
                    rule.resume,
                );
            }
        }
    }
    walk::walk(document, &mut check);
}

struct Enums<'a> {
    out: &'a mut Builder,
}

impl Enums<'_> {
    /// `E0018` when `value` is not a number `E` names, at the pointer `at`
    /// plus `tail`, which is built only then.
    fn raw<E: TryFrom<i32>>(&mut self, at: &str, tail: &[&str], field: &'static str, value: i32) {
        if E::try_from(value).is_ok() {
            return;
        }
        self.out.emit(
            Diag::new("E0018", pointer::join(at, tail))
                .arg("field", field)
                .arg("value", value),
        );
    }

    /// An integer comparison's operator.
    fn compare(&mut self, at: &str, tail: [&str; 2], compare: Option<&IntCompare>) {
        if let Some(compare) = compare {
            let [first, second] = tail;
            self.raw::<int_compare::Op>(at, &[first, second, "op"], "op", compare.op);
        }
    }

    /// An enemy predicate's freshness, under the predicate `predicate` at
    /// `at`.
    fn freshness(&mut self, at: &str, predicate: &str, window: Option<&Freshness>) {
        if let Some(window) = window {
            self.raw::<freshness::Kind>(at, &[predicate, "freshness", "kind"], "kind", window.kind);
        }
    }

    /// A pick's rank and coverage.
    fn pick(&mut self, at: &str, arm: &str, rank: i32, coverage: i32) {
        self.raw::<feature_ref::Rank>(at, &[arm, "rank"], "rank", rank);
        self.raw::<feature_ref::Coverage>(at, &[arm, "coverage"], "coverage", coverage);
    }

    /// A feature reference's pick, if it holds one.
    fn feature(&mut self, at: &str, reference: &FeatureRef) {
        match reference.r#ref.as_ref() {
            Some(feature_ref::Ref::Vent(vent)) => {
                self.pick(at, "vent", vent.rank, vent.coverage);
            }
            Some(feature_ref::Ref::Seam(seam)) => {
                self.pick(at, "seam", seam.rank, seam.coverage);
            }
            Some(feature_ref::Ref::FeatureId(_) | feature_ref::Ref::Covered(_)) | None => {}
        }
    }
}

impl Visit for Enums<'_> {
    fn entry(&mut self, at: &str, entry: &Step, _list: List, _index: usize) {
        if let Some(fail) = entry.on_fail.as_ref() {
            self.raw::<on_fail::Action>(at, &["on_fail", "action"], "action", fail.action);
        }
        match entry.kind.as_ref() {
            Some(step::Kind::Move(walk_to)) => {
                self.raw::<move_step::Pace>(at, &["move", "pace"], "pace", walk_to.pace);
            }
            Some(step::Kind::PlaceBeacon(place)) => {
                if let Some(initial) = place.initial.as_ref() {
                    self.raw::<interface_row::QuartermasterPriority>(
                        at,
                        &["place_beacon", "initial", "priority"],
                        "priority",
                        initial.priority,
                    );
                }
            }
            _ => {}
        }
    }

    fn interface(&mut self, at: &str, interface: &InterfaceStep) {
        for (index, row) in interface.rows.iter().enumerate() {
            if let Some(interface_row::Row::SetPriority(priority)) = row.row.as_ref()
                && interface_row::QuartermasterPriority::try_from(*priority).is_err()
            {
                self.raw::<interface_row::QuartermasterPriority>(
                    &pointer::at(&pointer::child(at, "rows"), index),
                    &["set_priority"],
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
            Some(location::Place::Toward(toward)) => {
                if let Some(feature) = toward.feature.as_ref() {
                    self.feature(&pointer::join(at, &["toward", "feature"]), feature);
                }
            }
            _ => {}
        }
    }

    fn filter(&mut self, at: &str, filter: &BeaconFilter) {
        self.raw::<beacon_filter::Side>(at, &["side"], "side", filter.side);
        self.raw::<beacon_filter::MandateKind>(at, &["mandate"], "mandate", filter.mandate);
    }

    fn mandate(&mut self, at: &str, settings: &MandateSettings, _carrier: Carrier) {
        self.raw::<mandate_settings::Roe>(at, &["roe"], "roe", settings.roe);
        match settings.mandate.as_ref() {
            Some(mandate_settings::Mandate::Build(build)) => {
                self.raw::<build_settings::Terraform>(
                    at,
                    &["build", "terraform"],
                    "terraform",
                    build.terraform,
                );
            }
            Some(mandate_settings::Mandate::Mine(mine)) => {
                self.raw::<mine_settings::SeamChoice>(
                    at,
                    &["mine", "seam_choice"],
                    "seam_choice",
                    mine.seam_choice,
                );
            }
            // S2's settings (`con4`): read raw like every other enum, as they
            // were while the sim refused the whole arm (until S2's `dfnd` and
            // `atk`), so that the day each was read, an unknown number already
            // was not.
            Some(mandate_settings::Mandate::Defend(defend)) => {
                self.raw::<defend_settings::Pursue>(
                    at,
                    &["defend", "pursue"],
                    "pursue",
                    defend.pursue,
                );
            }
            Some(mandate_settings::Mandate::Attack(attack)) => {
                self.raw::<attack_settings::Retarget>(
                    at,
                    &["attack", "retarget"],
                    "retarget",
                    attack.retarget,
                );
                self.raw::<attack_settings::AfterAction>(
                    at,
                    &["attack", "after_action"],
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
                self.raw::<cmdr_hp_pct::Cmp>(at, &["cmdr_hp_pct", "cmp"], "cmp", predicate.cmp);
            }
            Some(condition::Node::CmdrDeaths(predicate)) => {
                self.compare(at, ["cmdr_deaths", "deaths"], predicate.deaths.as_ref());
            }
            Some(condition::Node::SegmentElapsed(predicate)) => {
                self.compare(at, ["segment_elapsed", "ms"], predicate.ms.as_ref());
            }
            Some(condition::Node::BeaconHpPct(predicate)) => {
                self.compare(at, ["beacon_hp_pct", "pct"], predicate.pct.as_ref());
            }
            Some(condition::Node::Treasury(predicate)) => {
                self.compare(at, ["treasury", "dollars"], predicate.dollars.as_ref());
            }
            Some(condition::Node::KwHeadroom(predicate)) => {
                self.compare(at, ["kw_headroom", "kw"], predicate.kw.as_ref());
            }
            Some(condition::Node::BeaconUnitsByRole(predicate)) => {
                self.raw::<beacon_units_by_role::Role>(
                    at,
                    &["beacon_units_by_role", "role"],
                    "role",
                    predicate.role,
                );
                self.compare(
                    at,
                    ["beacon_units_by_role", "count"],
                    predicate.count.as_ref(),
                );
            }
            Some(condition::Node::BeaconMandate(predicate)) => {
                self.raw::<beacon_filter::MandateKind>(
                    at,
                    &["beacon_mandate", "mandate"],
                    "mandate",
                    predicate.mandate,
                );
            }
            Some(condition::Node::EnemyUnitsNear(predicate)) => {
                self.compare(at, ["enemy_units_near", "count"], predicate.count.as_ref());
                self.freshness(at, "enemy_units_near", predicate.freshness.as_ref());
            }
            Some(condition::Node::EnemyBeaconKnown(predicate)) => {
                self.freshness(at, "enemy_beacon_known", predicate.freshness.as_ref());
            }
            Some(condition::Node::EnemyStructureNear(predicate)) => {
                self.freshness(at, "enemy_structure_near", predicate.freshness.as_ref());
            }
            Some(condition::Node::KwSupply(predicate)) => {
                self.compare(at, ["kw_supply", "kw"], predicate.kw.as_ref());
            }
            Some(condition::Node::KwDraw(predicate)) => {
                self.compare(at, ["kw_draw", "kw"], predicate.kw.as_ref());
            }
            _ => {}
        }
    }
}
