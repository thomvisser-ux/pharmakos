// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stage 2, **structure**: shape and limits.
//!
//! Everything that can be decided by walking the tree once, with no symbol table
//! and no knowledge of the world: the blocks a playbook must have, the names its
//! parts must carry, the ranges its numbers must sit in, the shape of its
//! conditions, and the one size budget.
//!
//! # Limits and termination (spec section 10)
//!
//! "Every playbook halts. These limits are technical — bounded cost and
//! guaranteed termination — and in the story they are the Ledger's
//! seal-inspection rules." This stage owns three of the four:
//!
//! * **Size** — one budget, and `size_units` is on the report whether the
//!   playbook fits or not, so the editor's meter reads the verifier's number
//!   rather than counting for itself.
//! * **Rule firing** — a cooldown at or above the floor, and `max_fires` between
//!   one and the ceiling. Both numbers are rules-table rows.
//! * **Conditions** — four levels and twenty-four nodes, integer comparisons
//!   only.
//!
//! The fourth, "jumps go forward and every wait has a timeout", needs the label
//! table and belongs to [`crate::semantics`].
//!
//! # Durations are `int32` milliseconds and are never negative
//!
//! Decisions-log item 46: every duration in `gp.v1` is `int32` game
//! milliseconds, so the canonical JSON writes a bare number and spec section
//! 10's worked example loads as written — and a negative one is **rejected here,
//! with a JSON Pointer**, rather than at decode where there would be no pointer
//! to give. `E0109` is that rejection.
//!
//! # Targeting's sites: which arm is legal where
//!
//! S1's targeting vocabulary (docs/design/targeting.md, "Sites") adds two
//! `Location` arms, each a `gp.v1.FeatureRef`, and each is legal in exactly one
//! place: `covering` in a `place_beacon`'s `at`, and `on` in a Build target's
//! anchor (plus, as a `feature_id` only, `remove_build_target`'s). `covered {}`
//! is legal only under `on` in a Build target inside the `initial` of a
//! `place_beacon` whose `at` is `covering`. Whether an arm may stand where it
//! stands is a question about the file alone, so it is asked here, by
//! [`Sites`] over the shared walk, which knows the slot each `Location` is in;
//! `E0407` to `E0411` are its answers, and an unset rank, an unset choice and,
//! under `covering`, an unset coverage are `E0111`, like every other unset
//! choice. Whether a name exists is [`crate::resolve`]'s question and whether
//! the thing named is a vent is [`crate::semantics`]'s. Nothing here ranks.
//!
//! # Omitted settings (S1-39)
//!
//! An omitted `MoveStep.pace`, `MineSettings.seam_choice`,
//! `MineSettings.pillar_spacing` and `BuildSettings.terraform` read as named
//! defaults, each with an `I0003` note; see [`walk_action`]. From S2 so does
//! `MandateSettings.roe`, by writ (S2's plan, decision 4; decisions-log item
//! 148 (2)).
//!
//! # A wall's box (S2's wire, task `con4`)
//!
//! `BuildTarget.wall_box` is a `wall` target's voxels and is refused on any
//! other blueprint (`E0509`): a structure's footprint is its blueprint's, and a
//! box written on a `generator` would otherwise be read by nothing, which is a
//! silent strip. A `wall` target was refused for its blueprint (`E0019`, with
//! the rest of S2's base kit) until S2's `site` lane built walls; that code is
//! retired and raised by nothing now. From S2's `vfy2` a `wall` target's own shape is checked here too
//! (`Sites::wall`): its box is required (`E0101`), it is not turned
//! (`E0511`), its anchor is a voxel inside the box (`E0417`), and the box can
//! stand `materials.wall_min_height_voxels` above the ground (`E0512`).
//!
//! # Defend and Attack (S2's `vfy2`)
//!
//! An empty arm requires nothing here (its refusal in a deploy's `initial` is
//! `E0012` or `E0013`, as the sim's); an arm that writes anything in a deploy
//! or a switch writes every required field; an edit writes what it changes,
//! held to its range (`Sites::defend`, `Sites::attack`). An omitted `roe`
//! reads by writ with `I0003`, the fifth named default: `FREE_FIRE` on a
//! Defend or an Attack beacon and `RETURN_FIRE` on the others (decisions-log
//! item 148 (2)), and a Build beacon's repair threshold of 0 is `I0004` (S2's
//! plan, decisions 4 and 38). A protected, probe or avoid area missing a
//! corner is `E0101`, as a wall's box is (decisions-log item 146 (4)).
//! The sim's handler ceiling is `E0113`.

use pharmakos_proto::gp::api::v1::patch_suggestion::Applicability;
use pharmakos_proto::gp::v1::beacon_filter::Side;
use pharmakos_proto::gp::v1::build_settings::Terraform;
use pharmakos_proto::gp::v1::defend_settings::Pursue;
use pharmakos_proto::gp::v1::feature_ref::{self, Coverage, Rank};
use pharmakos_proto::gp::v1::handler::Resume;
use pharmakos_proto::gp::v1::mandate_settings::Roe;
use pharmakos_proto::gp::v1::mine_settings::SeamChoice;
use pharmakos_proto::gp::v1::move_step::Pace;
use pharmakos_proto::gp::v1::on_death::OnRespawn;
use pharmakos_proto::gp::v1::{
    Area, AttackSettings, BeaconFilter, BeaconRef, BuildTarget, Condition, DefendSettings,
    FeatureRef, Handler, IntCompare, Location, MandateSettings, Playbook, ProtectedAsset, Step,
    attack_target, beacon_ref, condition, fallback, int_compare, interface_row, location,
    mandate_settings, protected_asset, step,
};
use pharmakos_proto::json::Json;

use crate::limits::{CONDITION_MAX_DEPTH, CONDITION_MAX_NODES, Limits};
use crate::pointer;
use crate::report::{Builder, Diag, number, patch_add, patch_remove, patch_replace};
use crate::size;
use crate::strings;
use crate::walk::{self, AreaOf, Carrier, List, Slot, Visit};

use std::collections::BTreeMap;

/// The one blueprint that fills a box (`BuildTarget.wall_box`).
const WALL: &str = "wall";

/// Run the stage.
pub(crate) fn run(playbook: &Playbook, limits: &Limits, out: &mut Builder) {
    required_blocks(playbook, out);
    budget(playbook, limits, out);

    if let Some(declarative) = playbook.declarative.as_ref() {
        if declarative.route.is_empty() {
            out.emit(Diag::new("E0102", "/declarative/route"));
        }
        for (index, entry) in declarative.route.iter().enumerate() {
            walk_route_entry(entry, &pointer::at("/declarative/route", index), out);
        }
        unique_names(
            declarative.route.iter().map(|entry| entry.label.as_str()),
            "/declarative/route",
            "within the route",
            "label",
            out,
        );
        for (index, handler) in declarative.handlers.iter().enumerate() {
            walk_handler(
                handler,
                &pointer::at("/declarative/handlers", index),
                limits,
                out,
            );
        }
        unique_names(
            declarative
                .handlers
                .iter()
                .map(|handler| handler.id.as_str()),
            "/declarative/handlers",
            "within the playbook",
            "id",
            out,
        );
        handler_ceiling(declarative.handlers.len(), out);
    }

    if let Some(on_death) = playbook.on_death.as_ref()
        && on_death.on_respawn == i32::from(OnRespawn::Unspecified)
    {
        out.emit(
            Diag::new("E0111", "/on_death/on_respawn")
                .arg("field", "on_respawn")
                .fix(
                    "Continue the route after a respawn",
                    // `add`: an unset enum is written by omitting the
                    // member, so a `replace` would have nothing to land on.
                    patch_add(
                        "/on_death/on_respawn",
                        &pharmakos_proto::json::Json::String("CONTINUE".to_owned()),
                    ),
                    Applicability::MaybeIncorrect,
                ),
        );
    }

    if let Some(tail) = playbook.fallback.as_ref() {
        // The guaranteed tail runs for the rest of the Push once the route is
        // done, so its places are checked exactly as a step's are: a fallback
        // that holds nowhere is the same bug as a move that walks nowhere, and
        // it lasts longer.
        match tail.posture.as_ref() {
            None => out.emit(Diag::new("E0111", "/fallback").arg("field", "posture")),
            Some(fallback::Posture::Hold(hold)) => {
                check_location(hold.at.as_ref(), "/fallback/hold/at", "`at`", out);
            }
            Some(fallback::Posture::Shadow(shadow)) => match shadow.beacon.as_ref() {
                None => {
                    out.emit(Diag::new("E0101", "/fallback/shadow/beacon").arg("what", "`beacon`"));
                }
                Some(target) => check_beacon_ref(target, "/fallback/shadow/beacon", out),
            },
            Some(fallback::Posture::Patrol(patrol)) => {
                let at = "/fallback/patrol/waypoints";
                for (index, waypoint) in patrol.waypoints.iter().enumerate() {
                    check_location(Some(waypoint), &pointer::at(at, index), "`waypoint`", out);
                }
            }
        }
    }

    walk::walk(playbook, &mut Sites { limits, out });
}

/// The four blocks every playbook carries (`proto/gp/v1/playbook.proto`:
/// "Required" means the verifier rejects the playbook without it).
fn required_blocks(playbook: &Playbook, out: &mut Builder) {
    if playbook.meta.is_none() {
        out.emit(Diag::new("E0101", "/meta").arg("what", "`meta`"));
    }
    if playbook.declarative.is_none() {
        out.emit(Diag::new("E0101", "/declarative").arg("what", "`declarative`"));
    }
    if playbook.on_death.is_none() {
        out.emit(
            Diag::new("E0101", "/on_death")
                .arg("what", "the `on_death` block")
                .fix(
                    "Add an on_death block",
                    patch_add("/on_death", &on_death_stub()),
                    Applicability::HasPlaceholders,
                ),
        );
    }
    if playbook.fallback.is_none() {
        out.emit(
            Diag::new("E0101", "/fallback")
                .arg("what", "the `fallback` block")
                .fix(
                    "Add a fallback that holds at the safest beacon",
                    patch_add("/fallback", &fallback_stub()),
                    Applicability::HasPlaceholders,
                ),
        );
    }
}

/// `{"on_respawn":"CONTINUE","max_deaths_before_fallback":2}`.
///
/// `HAS_PLACEHOLDERS`, not `MACHINE_APPLICABLE`: how many deaths a playbook
/// tolerates is the author's judgement and this suggestion only makes the file
/// loadable.
fn on_death_stub() -> pharmakos_proto::json::Json {
    pharmakos_proto::json::Json::Object(vec![
        (
            "on_respawn".to_owned(),
            pharmakos_proto::json::Json::String("CONTINUE".to_owned()),
        ),
        ("max_deaths_before_fallback".to_owned(), number(2)),
    ])
}

/// `{"hold":{"at":{"beacon_anchor":{"safest":{}}}}}` — the guaranteed tail of
/// spec section 10, in its plainest form.
fn fallback_stub() -> pharmakos_proto::json::Json {
    use pharmakos_proto::json::Json;
    Json::Object(vec![(
        "hold".to_owned(),
        Json::Object(vec![(
            "at".to_owned(),
            Json::Object(vec![(
                "beacon_anchor".to_owned(),
                Json::Object(vec![("safest".to_owned(), Json::Object(Vec::new()))]),
            )]),
        )]),
    )])
}

/// The one size budget (item 94).
fn budget(playbook: &Playbook, limits: &Limits, out: &mut Builder) {
    let units = size::size_units(playbook);
    if units > limits.size_budget_units() {
        out.emit(
            Diag::new("E0110", "/declarative")
                .arg("found", units)
                .arg("budget", limits.size_budget_units()),
        );
    }
}

/// `E0113`: more handlers than the sim compiles (decisions-log item 145 (5)).
///
/// The ceiling is the sim's own `MAX_HANDLERS`, read here and never written a
/// second time, so the editor's verdict and the seal's agree on it. It is
/// reachable inside the size budget: a handler with an empty body costs one
/// unit, and the budget is above the ceiling. The diagnostic points at the
/// first handler past it.
fn handler_ceiling(found: usize, out: &mut Builder) {
    let max = pharmakos_sim::interpreter::MAX_HANDLERS;
    if found <= max {
        return;
    }
    out.emit(
        Diag::new("E0113", pointer::at("/declarative/handlers", max))
            .arg("found", found)
            .arg("max", max),
    );
}

/// One route step.
fn walk_route_entry(entry: &Step, at: &str, out: &mut Builder) {
    if entry.label.is_empty() {
        out.emit(Diag::new("E0104", at).arg("what", "the step"));
    }
    if entry.timeout_ms < 0 {
        negative(out, at, "timeout_ms", entry.timeout_ms);
    }
    if let Some(guard) = entry.skip_if.as_ref() {
        walk_condition_root(guard, &pointer::child(at, "skip_if"), out);
    }
    let Some(kind) = entry.kind.as_ref() else {
        out.emit(Diag::new("E0103", at));
        return;
    };
    walk_action(kind, at, out);
}

/// One step's action: present, and naming what it needs.
///
/// **How an omitted enum or setting inside the body reads (the register's
/// S1-39), decided.** `proto/gp/v1/playbook.proto`'s header says an unset enum
/// is a verifier error, and spec section 10's worked example — which
/// `examples/playbooks/expand_east.jsonc` copies verbatim — omits three
/// settings the rule would then refuse: `MoveStep.pace` on its third step, and
/// `MineSettings.seam_choice` and `pillar_spacing`. S1's plan's decision 11,
/// ruled with the plan by decisions-log item 128, answers all of them at once:
/// an omitted `pace` reads as DIRECT, `seam_choice` as NEAREST,
/// `pillar_spacing` as 0 (no pillars) and `BuildSettings.terraform` as NONE,
/// each with an `I0003` note, so the spec's example verifies and says what it
/// was read as. They are four invented defaults, named as such in the plan and
/// the header; targeting's new `*_UNSPECIFIED` values stay errors, as adopted.
/// [`Sites`] raises the notes over the shared walk, in document order. The
/// envelope's own tags — `kind` and `author_kind` — are unaffected and are
/// checked at decode, because item 76 settles them by name.
fn walk_action(kind: &step::Kind, at: &str, out: &mut Builder) {
    match kind {
        step::Kind::Move(walk_to) => {
            let here = pointer::child(at, "move");
            check_location(
                walk_to.to.as_ref(),
                &pointer::child(&here, "to"),
                "`to`",
                out,
            );
        }
        step::Kind::Interface(interface) => {
            let here = pointer::child(at, "interface");
            match interface.beacon.as_ref() {
                None => out.emit(
                    Diag::new("E0101", pointer::child(&here, "beacon")).arg("what", "`beacon`"),
                ),
                Some(target) => {
                    check_beacon_ref(target, &pointer::child(&here, "beacon"), out);
                }
            }
            for (index, row) in interface.rows.iter().enumerate() {
                let row_at = pointer::at(&pointer::child(&here, "rows"), index);
                match row.row.as_ref() {
                    None => out.emit(Diag::new("E0101", row_at).arg("what", "the interface row")),
                    Some(interface_row::Row::QueueStructure(queue)) => {
                        if queue.blueprint_id.is_empty() {
                            out.emit(
                                Diag::new("E0101", pointer::child(&row_at, "queue_structure"))
                                    .arg("what", "`blueprint_id`"),
                            );
                        }
                    }
                    Some(_) => {}
                }
            }
        }
        step::Kind::PlaceBeacon(place) => {
            let here = pointer::child(at, "place_beacon");
            check_location(place.at.as_ref(), &pointer::child(&here, "at"), "`at`", out);
        }
        step::Kind::WaitUntil(wait) => {
            let here = pointer::child(at, "wait_until");
            match wait.condition.as_ref() {
                None => out.emit(
                    Diag::new("E0101", pointer::child(&here, "condition"))
                        .arg("what", "`condition`"),
                ),
                Some(condition) => {
                    walk_condition_root(condition, &pointer::child(&here, "condition"), out);
                }
            }
        }
        step::Kind::Hold(hold) => {
            if hold.ms < 0 {
                negative(out, &pointer::child(at, "hold"), "ms", hold.ms);
            }
        }
        step::Kind::Broadcast(broadcast) => {
            if broadcast.payload.is_none() {
                out.emit(
                    Diag::new("E0101", pointer::child(at, "broadcast")).arg("what", "the payload"),
                );
            }
        }
    }
}

/// A place a step names: present at all, and naming a place.
///
/// `Location.place` is a choice set exactly like `Step.kind`, `Fallback.posture`
/// and `Handler.resume`, and this stage's rule for all of them is the schema's:
/// an unset choice is an error, never a default. It is easy to miss on a
/// `Location` because the message can be *written* — `"to": {}` is valid JSON
/// and decodes without complaint — and a step that walks nowhere would otherwise
/// seal clean and then do nothing for a whole Push, which is the most expensive
/// way for a playbook to be wrong.
///
/// `what` names the field for `E0101` when the whole message is absent; the
/// unset-choice case is `E0111`, pointed at the `Location` itself, because there
/// is no member under it to point at.
fn check_location(place: Option<&Location>, at: &str, what: &str, out: &mut Builder) {
    let Some(place) = place else {
        out.emit(Diag::new("E0101", at.to_owned()).arg("what", what));
        return;
    };
    match place.place.as_ref() {
        None => out.emit(Diag::new("E0111", at.to_owned()).arg("field", "place")),
        Some(location::Place::BeaconAnchor(target)) => {
            check_beacon_ref(target, &pointer::child(at, "beacon_anchor"), out);
        }
        // Targeting's two sites name a place too, so there is nothing more to
        // check here: [`Sites`] judges them in the slot they stand in, a Build
        // target's anchor included, which this function is never handed.
        Some(
            location::Place::Voxel(_)
            | location::Place::Safest(_)
            | location::Place::On(_)
            | location::Place::Covering(_),
        ) => {}
    }
}

/// Targeting's sites, judged where they stand, and the omitted settings'
/// notes (S1-39): the two checks of this stage that need the shared walk.
///
/// This replaces the refusal S1's targeting proto shipped (task `con2`), which
/// emitted `E0003` at either arm wherever it stood until the rules below
/// landed; `E0003` is decode's again, for the words held back to a later
/// version, and `on` and `covering` are v1 vocabulary (decisions-log item 130
/// (4)).
struct Sites<'a> {
    limits: &'a Limits,
    out: &'a mut Builder,
}

/// Which site arm a `FeatureRef` is under, with what that arm reads.
#[derive(Clone, Copy)]
enum Arm<'a> {
    /// `covering`, in a `place_beacon`'s `at`.
    Covering,
    /// `on`, in a Build target's anchor; `deploy` is the enclosing
    /// `place_beacon`'s `covering`, if the target is in its `initial`.
    On { deploy: Option<&'a FeatureRef> },
}

impl Visit for Sites<'_> {
    fn entry(&mut self, at: &str, entry: &Step, _list: List, _index: usize) {
        if let Some(step::Kind::Move(walk_to)) = entry.kind.as_ref()
            && walk_to.pace == i32::from(Pace::Unspecified)
        {
            self.default_note(&pointer::child(at, "move"), "pace", "DIRECT", true);
        }
    }

    /// A half-written area has no voxels to protect, probe or avoid, and the
    /// sim refuses one at compile (`compile_area`'s
    /// `MissingBlock("area.min")`), so each missing corner is `E0101`,
    /// as a wall's box already is (decisions-log item 146 (4)). A wall's box
    /// is [`Sites::wall`]'s, which names it so.
    fn area(&mut self, at: &str, area: &Area, of: AreaOf) {
        match of {
            AreaOf::WallBox => return,
            AreaOf::Protected | AreaOf::Probe | AreaOf::Avoid => {}
        }
        for (present, corner) in [(area.min.is_some(), "min"), (area.max.is_some(), "max")] {
            if !present {
                self.out.emit(
                    Diag::new("E0101", pointer::child(at, corner))
                        .arg("what", format!("`{}.{corner}`", of.field())),
                );
            }
        }
    }

    fn location(&mut self, at: &str, place: &Location, slot: Slot<'_>) {
        match place.place.as_ref() {
            Some(location::Place::Covering(feature)) => {
                let here = pointer::child(at, "covering");
                if matches!(slot, Slot::PlaceSite) {
                    self.feature(feature, &here, Arm::Covering);
                } else {
                    self.out.emit(Diag::new("E0407", here));
                }
            }
            Some(location::Place::On(feature)) => {
                let here = pointer::child(at, "on");
                match slot {
                    Slot::BuildAnchor { deploy } => {
                        self.feature(feature, &here, Arm::On { deploy });
                    }
                    Slot::RemoveAnchor => match feature.r#ref.as_ref() {
                        None => self.out.emit(Diag::new("E0111", here).arg("field", "ref")),
                        Some(feature_ref::Ref::FeatureId(_)) => {}
                        Some(description) => self.out.emit(Diag::new(
                            "E0410",
                            pointer::child(&here, arm_name(description)),
                        )),
                    },
                    Slot::PlaceSite | Slot::Elsewhere => self.out.emit(Diag::new("E0408", here)),
                }
            }
            _ => {}
        }
    }

    fn build_target(&mut self, at: &str, target: &BuildTarget) {
        if target.blueprint_id == WALL {
            self.wall(at, target);
            return;
        }
        // An empty blueprint id is `E0101`'s (the refusals' check), and a box
        // under it would say nothing more.
        if target.wall_box.is_none() || target.blueprint_id.is_empty() {
            return;
        }
        let here = pointer::child(at, "wall_box");
        self.out.emit(
            Diag::new("E0509", here.clone())
                .arg("id", &target.blueprint_id)
                .fix(
                    strings::FIX_DROP_WALL_BOX,
                    patch_remove(&here),
                    // The author may have meant a wall rather than no box.
                    Applicability::MaybeIncorrect,
                ),
        );
    }

    fn mandate(&mut self, at: &str, settings: &MandateSettings, carrier: Carrier) {
        // A `set_mandate_settings` row edits the current mandate: a field it
        // leaves out is one it does not write, not one it sets to the default,
        // and writing the default out would add a field the sim charges for.
        // So the note, and its Fix, stand only where the beacon's settings
        // start from nothing.
        //
        // S2's Defend and Attack arms are checked under every carrier, for
        // the ranges of what they write, and, outside an edit, for what they
        // must write (`defend` and `attack` below).
        match settings.mandate.as_ref() {
            Some(mandate_settings::Mandate::Defend(defend)) => {
                self.defend(&pointer::child(at, "defend"), defend, carrier);
            }
            Some(mandate_settings::Mandate::Attack(attack)) => {
                self.attack(&pointer::child(at, "attack"), attack, carrier);
            }
            _ => {}
        }
        if carrier == Carrier::Edit {
            return;
        }
        // S2's plan, decision 4, as decisions-log item 148 (2) amends it: an
        // omitted `roe` reads by writ, FREE_FIRE on a Defend or an Attack
        // beacon and RETURN_FIRE on the others (the sim's `Roe::of_setting`,
        // whose Defend and Attack arms are S2's `dfnd`'s and `atk`'s),
        // said where the settings start from nothing and the block names a
        // writ (a block with none is `E0502`'s).
        if let Some(mandate) = settings.mandate.as_ref()
            && settings.roe == i32::from(Roe::Unspecified)
        {
            self.default_note(at, "roe", omitted_roe(mandate), true);
        }
        match settings.mandate.as_ref() {
            Some(mandate_settings::Mandate::Build(build)) => {
                let here = pointer::child(at, "build");
                if build.terraform == i32::from(Terraform::Unspecified) {
                    self.default_note(&here, "terraform", "NONE", true);
                }
                // S2's plan, decision 38: the threshold is read as written, and
                // 0 (omitted or written) means a structure is never repaired.
                // No Fix: what percentage to repair at is the author's call.
                if build.repair_threshold_pct == 0 {
                    self.out.emit(Diag::new(
                        "I0004",
                        pointer::child(&here, "repair_threshold_pct"),
                    ));
                }
            }
            Some(mandate_settings::Mandate::Mine(mine)) => {
                let here = pointer::child(at, "mine");
                if mine.seam_choice == i32::from(SeamChoice::Unspecified) {
                    self.default_note(&here, "seam_choice", "NEAREST", true);
                }
                if mine.pillar_spacing == 0 {
                    // No Fix: proto3 writes a zero by leaving it out, so there
                    // is no spelling of "0" for the note to go quiet on.
                    self.default_note(&here, "pillar_spacing", strings::NO_PILLARS, false);
                }
            }
            _ => {}
        }
    }
}

impl Sites<'_> {
    /// A `wall` target's own shape (S2's plan, decisions 10 and 40; `con4`'s
    /// `BuildTarget.wall_box`): the box is required, the wall is not turned,
    /// its anchor is a voxel inside the box, and the box is tall enough.
    ///
    /// **The height test is the half the file can decide.** A wall fills each
    /// column from the ground's top up to `max.z`, and `min.z` lies at or below
    /// the ground, so a column's wall is at most `max.z - min.z` voxels tall.
    /// When that is below `materials.wall_min_height_voxels`, no column can
    /// stand tall enough and `E0512` refuses the box. Where the ground actually
    /// is, the view does not say, so whether `min.z` reaches it and whether
    /// every column clears the minimum over it are the sim's to judge when it
    /// builds the wall (S2's `site`). Whether every column lies inside the
    /// holding beacon's sphere reads the view, and is the semantics stage's
    /// (`E0416`).
    fn wall(&mut self, at: &str, target: &BuildTarget) {
        if target.rotation_quarter_turns != 0 {
            let here = pointer::child(at, "rotation_quarter_turns");
            self.out.emit(
                Diag::new("E0511", here.clone())
                    .arg("found", target.rotation_quarter_turns)
                    .fix(
                        strings::FIX_DROP_ROTATION,
                        patch_remove(&here),
                        // A wall's rotation is 0 and nothing reads another.
                        Applicability::MachineApplicable,
                    ),
            );
        }
        let Some(wall_box) = target.wall_box.as_ref() else {
            self.out
                .emit(Diag::new("E0101", pointer::child(at, "wall_box")).arg("what", "`wall_box`"));
            return;
        };
        // A half-written box has no voxels to fill or to price, so each
        // missing corner is required, as the seal's price door needs it.
        let box_at = pointer::child(at, "wall_box");
        for (present, corner) in [
            (wall_box.min.is_some(), "min"),
            (wall_box.max.is_some(), "max"),
        ] {
            if !present {
                self.out.emit(
                    Diag::new("E0101", pointer::child(&box_at, corner))
                        .arg("what", format!("`wall_box.{corner}`")),
                );
            }
        }
        let (Some(low), Some(high)) = (wall_box.min.as_ref(), wall_box.max.as_ref()) else {
            return;
        };
        if low.x > high.x || low.y > high.y || low.z > high.z {
            // `E0406`'s: an inside-out box has no height and no inside.
            return;
        }
        let tall = i64::from(high.z).saturating_sub(i64::from(low.z));
        let least = i64::from(self.limits.wall_min_height_voxels());
        if tall < least {
            self.out.emit(
                Diag::new("E0512", pointer::child(at, "wall_box"))
                    .arg("found", tall)
                    .arg("min", least)
                    .map_ref(*low)
                    .map_ref(*high),
            );
        }
        let anchor_at = pointer::child(at, "anchor");
        match target
            .anchor
            .as_ref()
            .and_then(|anchor| anchor.place.as_ref())
        {
            // A target with no anchor names nothing to remove it by, which is
            // the same for every blueprint and is not the wall's question.
            None => {}
            Some(location::Place::Voxel(voxel)) => {
                let within = |value: i32, from: i32, to: i32| value >= from && value <= to;
                if !(within(voxel.x, low.x, high.x)
                    && within(voxel.y, low.y, high.y)
                    && within(voxel.z, low.z, high.z))
                {
                    self.out.emit(
                        Diag::new("E0417", pointer::child(&anchor_at, "voxel"))
                            .arg("found", strings::ANCHOR_OUTSIDE_BOX)
                            .map_ref(*voxel),
                    );
                }
            }
            Some(_) => self
                .out
                .emit(Diag::new("E0417", anchor_at).arg("found", strings::ANCHOR_NOT_A_VOXEL)),
        }
    }

    /// S2's Defend settings (`con4`; S2's plan, decision 13), as the file
    /// alone can judge them.
    ///
    /// **Requiredness follows the carrier.** An empty arm writes no field, so
    /// nothing is required of it here. Whether it is legal is the sim's refusal
    /// table's, which `crate::refusals` mirrors: an empty arm in a switch is a
    /// choice of writ the sim accepts (`compile_switch_settings`), and so,
    /// since S2's `dfnd` and `atk`, is an empty arm in a deploy's `initial`
    /// (decisions-log items 145 (5) and 146 (3) (j); `E0012` and `E0013` are
    /// raised by nothing now). An arm that
    /// writes anything in a deploy or a switch starts the beacon's settings
    /// from nothing, so it must write every required field. An edit
    /// (`set_mandate_settings`) writes only what it changes, so nothing is
    /// required of it, and what it does write is still held to its range.
    fn defend(&mut self, at: &str, defend: &DefendSettings, carrier: Carrier) {
        if *defend == DefendSettings::default() {
            return;
        }
        if carrier != Carrier::Edit {
            if defend.engagement_radius_pct == 0 {
                self.out.emit(
                    Diag::new("E0101", pointer::child(at, "engagement_radius_pct"))
                        .arg("what", "`engagement_radius_pct`"),
                );
            }
            if defend.pursue == i32::from(Pursue::Unspecified) {
                self.out
                    .emit(Diag::new("E0111", pointer::child(at, "pursue")).arg("field", "pursue"));
            }
        }
        if defend.engagement_radius_pct > 100 {
            self.range(
                at,
                "engagement_radius_pct",
                defend.engagement_radius_pct,
                strings::RANGE_ONE_TO_HUNDRED,
                Some(100),
            );
        }
        let protect_at = pointer::child(at, "protect");
        // Keyed on the entry's structure, through its derived `Debug` text:
        // two entries are the same exactly when they print the same, the lookup
        // is logarithmic rather than a scan per entry, and the key cannot fail
        // to build (the crate takes no `prost` dependency of its own, so the
        // entry's wire encoding is not in reach).
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();
        for (index, entry) in defend.protect.iter().enumerate() {
            let here = pointer::at(&protect_at, index);
            match entry.asset.as_ref() {
                None => {
                    self.out
                        .emit(Diag::new("E0111", here.clone()).arg("field", "asset"));
                }
                Some(protected_asset::Asset::Beacon(beacon)) => {
                    let beacon_at = pointer::child(&here, "beacon");
                    if let Some((selector, filter)) = selector_filter(beacon)
                        && Side::try_from(filter_side(filter)) == Ok(Side::EnemyKnown)
                    {
                        self.out.emit(
                            Diag::new("E0415", pointer::child(&beacon_at, selector))
                                .arg("role", strings::ROLE_PROTECT)
                                .arg("found", strings::RANKS_ENEMY)
                                .arg("wanted", strings::RANKS_OWN),
                        );
                    }
                }
                Some(protected_asset::Asset::MyCommander(_)) => {}
            }
            if entry.asset.is_none() {
                continue;
            }
            // A repeated entry is harmless (the guards escort it once), so it is
            // advice, W0502, and never a refusal: no rule refuses it (AGENTS.md
            // section 12, "don't invent rules").
            let key = format!("{entry:?}");
            match seen.get(&key) {
                Some(first) => self.out.emit(
                    Diag::new("W0502", here)
                        .arg("name", asset_name(entry))
                        .related(pointer::at(&protect_at, *first)),
                ),
                None => {
                    seen.insert(key, index);
                }
            }
        }
    }

    /// S2's Attack settings (`con4`; S2's plan, decision 14), as the file
    /// alone can judge them. Requiredness follows the carrier, as
    /// [`Sites::defend`] says.
    ///
    /// Whether a target named by its handle is the seat's own, and whether it
    /// was seen recently enough, read the view, and are the semantics stage's
    /// (`E0414`, `E0418`).
    fn attack(&mut self, at: &str, attack: &AttackSettings, carrier: Carrier) {
        if *attack == AttackSettings::default() {
            return;
        }
        if carrier != Carrier::Edit {
            for (present, field) in [
                (attack.target.is_some(), "target"),
                (attack.staging.is_some(), "staging"),
                (attack.launch.is_some(), "launch"),
            ] {
                if !present {
                    self.out.emit(
                        Diag::new("E0101", pointer::child(at, field))
                            .arg("what", format!("`{field}`")),
                    );
                }
            }
            for (value, field) in [
                (attack.retarget, "retarget"),
                (attack.after_action, "after_action"),
            ] {
                if value == 0 {
                    self.out
                        .emit(Diag::new("E0111", pointer::child(at, field)).arg("field", field));
                }
            }
            if attack.waves == 0 {
                self.range(at, "waves", 0, strings::RANGE_ONE_OR_MORE, Some(1));
            }
        }
        if let Some(target) = attack.target.as_ref() {
            let target_at = pointer::child(at, "target");
            match target.target.as_ref() {
                None => self
                    .out
                    .emit(Diag::new("E0111", target_at).arg("field", "target")),
                Some(attack_target::Target::Beacon(beacon)) => {
                    let beacon_at = pointer::child(&target_at, "beacon");
                    match beacon.r#ref.as_ref() {
                        Some(beacon_ref::Ref::Safest(_)) => self.out.emit(
                            Diag::new("E0415", pointer::child(&beacon_at, "safest"))
                                .arg("role", strings::ROLE_ATTACK_TARGET)
                                .arg("found", strings::RANKS_SAFEST)
                                .arg("wanted", strings::RANKS_ENEMY),
                        ),
                        _ => {
                            if let Some((selector, filter)) = selector_filter(beacon) {
                                // Read raw: a number `Side` does not name is
                                // `E0018`'s, and says nothing here.
                                let side = Side::try_from(filter_side(filter));
                                if matches!(side, Ok(Side::Unspecified | Side::Own)) {
                                    self.out.emit(
                                        Diag::new("E0415", pointer::child(&beacon_at, selector))
                                            .arg("role", strings::ROLE_ATTACK_TARGET)
                                            .arg("found", strings::RANKS_OWN)
                                            .arg("wanted", strings::RANKS_ENEMY),
                                    );
                                }
                            }
                        }
                    }
                }
                Some(
                    attack_target::Target::Voxel(_) | attack_target::Target::EnemyCommander(_),
                ) => {}
            }
        }
        if let Some(launch) = attack.launch.as_ref() {
            let launch_at = pointer::child(at, "launch");
            if launch.force_size.is_none() && launch.segment_ms.is_none() {
                self.out.emit(
                    Diag::new("E0101", launch_at.clone()).arg("what", strings::A_LAUNCH_CONDITION),
                );
            }
            if let Some(ms) = launch.segment_ms
                && ms < 0
            {
                negative(self.out, &launch_at, "segment_ms", ms);
            }
        }
        if let Some(retreat) = attack.retreat.as_ref() {
            let retreat_at = pointer::child(at, "retreat");
            if let Some(pct) = retreat.force_below_pct
                && !(1..=100).contains(&pct)
            {
                self.range(
                    &retreat_at,
                    "force_below_pct",
                    pct,
                    strings::RANGE_ONE_TO_HUNDRED,
                    Some(pct.clamp(1, 100)),
                );
            }
            if let Some(ms) = retreat.after_launch_ms
                && ms < 0
            {
                negative(self.out, &retreat_at, "after_launch_ms", ms);
            }
        }
    }

    /// `E0510`: a setting outside its range, with a Fix to `fixed` where
    /// there is an obvious nearest value.
    fn range(
        &mut self,
        at: &str,
        field: &'static str,
        found: u32,
        range: &str,
        fixed: Option<u32>,
    ) {
        let here = pointer::child(at, field);
        let mut diag = Diag::new("E0510", here.clone())
            .arg("field", field)
            .arg("found", found)
            .arg("range", range);
        if let Some(value) = fixed {
            diag = diag.fix(
                format!("Set {field} to {value}"),
                // `add`: the out-of-range value may be the absent one.
                patch_add(&here, &number(value)),
                Applicability::MaybeIncorrect,
            );
        }
        self.out.emit(diag);
    }

    /// One `FeatureRef` under an arm it may stand under.
    fn feature(&mut self, feature: &FeatureRef, at: &str, arm: Arm<'_>) {
        let Some(chosen) = feature.r#ref.as_ref() else {
            self.out.emit(Diag::new("E0111", at).arg("field", "ref"));
            return;
        };
        let pick = match chosen {
            // A name is checked for existence by the resolve stage and for its
            // kind by the semantics stage; its shape here is a string.
            feature_ref::Ref::FeatureId(_) => return,
            feature_ref::Ref::Covered(_) => {
                if !matches!(arm, Arm::On { deploy: Some(_) }) {
                    self.out
                        .emit(Diag::new("E0409", pointer::child(at, "covered")));
                }
                return;
            }
            feature_ref::Ref::Vent(pick) => (pick.rank, pick.coverage),
            feature_ref::Ref::Seam(pick) => (pick.rank, pick.coverage),
        };
        let here = pointer::child(at, arm_name(chosen));
        let (rank, coverage) = pick;
        if rank == i32::from(Rank::Unspecified) {
            self.out.emit(
                Diag::new("E0111", pointer::child(&here, "rank"))
                    .arg("field", "rank")
                    .fix(
                        strings::FIX_RANK_NEAREST,
                        // `add`: an unset enum is written by leaving it out.
                        patch_add(
                            &pointer::child(&here, "rank"),
                            &Json::String("NEAREST".to_owned()),
                        ),
                        Applicability::MaybeIncorrect,
                    ),
            );
        }
        match arm {
            Arm::Covering if coverage == i32::from(Coverage::Unspecified) => self.out.emit(
                Diag::new("E0111", pointer::child(&here, "coverage"))
                    .arg("field", "coverage")
                    .fix(
                        strings::FIX_COVERAGE_UNCOVERED,
                        patch_add(
                            &pointer::child(&here, "coverage"),
                            &Json::String("UNCOVERED".to_owned()),
                        ),
                        Applicability::MaybeIncorrect,
                    ),
            ),
            Arm::On { .. } if coverage != i32::from(Coverage::Unspecified) => {
                self.out
                    .emit(Diag::new("E0411", pointer::child(&here, "coverage")).fix(
                        strings::FIX_DROP_COVERAGE,
                        // The member is present by construction (it is not the
                        // default), and taking it out changes nothing the sim
                        // reads, which is what makes the fix machine-applicable.
                        patch_remove(&pointer::child(&here, "coverage")),
                        Applicability::MachineApplicable,
                    ));
            }
            Arm::Covering | Arm::On { .. } => {}
        }
    }

    /// `I0003`: an omitted setting and the named default it reads as.
    fn default_note(&mut self, at: &str, field: &'static str, value: &str, fixable: bool) {
        let here = pointer::child(at, field);
        let mut diag = Diag::new("I0003", here.clone())
            .arg("field", field)
            .arg("value", value);
        if fixable {
            diag = diag.fix(
                strings::FIX_WRITE_DEFAULT,
                patch_add(&here, &Json::String(value.to_owned())),
                Applicability::MachineApplicable,
            );
        }
        self.out.emit(diag);
    }
}

/// What an omitted `roe` reads as on a beacon of this writ (decisions-log
/// item 148 (2)): a Defend or an Attack beacon fires freely, and every other
/// returns fire only.
fn omitted_roe(mandate: &mandate_settings::Mandate) -> &'static str {
    match mandate {
        mandate_settings::Mandate::Defend(_) | mandate_settings::Mandate::Attack(_) => {
            Roe::FreeFire.as_str_name()
        }
        mandate_settings::Mandate::Build(_)
        | mandate_settings::Mandate::Mine(_)
        | mandate_settings::Mandate::Survey(_) => Roe::ReturnFire.as_str_name(),
    }
}

/// A ranking selector's JSON name and its filter, if `beacon` is one: the
/// three selectors that take a filter. A fixed id and `safest` are not.
fn selector_filter(beacon: &BeaconRef) -> Option<(&'static str, Option<&BeaconFilter>)> {
    match beacon.r#ref.as_ref()? {
        beacon_ref::Ref::Nearest(pick) => Some(("nearest", pick.filter.as_ref())),
        beacon_ref::Ref::Weakest(pick) => Some(("weakest", pick.filter.as_ref())),
        beacon_ref::Ref::MostThreatened(pick) => Some(("most_threatened", pick.filter.as_ref())),
        beacon_ref::Ref::BeaconId(_) | beacon_ref::Ref::Safest(_) => None,
    }
}

/// A filter's side as written, raw. No filter at all writes no side, which is
/// the side a bare selector has (`BeaconFilter`'s named exception: it reads
/// as `OWN`), so it is the same number.
fn filter_side(filter: Option<&BeaconFilter>) -> i32 {
    match filter {
        Some(found) => found.side,
        None => i32::from(Side::Unspecified),
    }
}

/// How `E0105` names a repeated protect-list entry.
fn asset_name(entry: &ProtectedAsset) -> String {
    match entry.asset.as_ref() {
        Some(protected_asset::Asset::Beacon(beacon)) => match beacon.r#ref.as_ref() {
            Some(beacon_ref::Ref::BeaconId(id)) => id.clone(),
            Some(beacon_ref::Ref::Safest(_)) => "safest".to_owned(),
            Some(beacon_ref::Ref::Nearest(_)) => "nearest".to_owned(),
            Some(beacon_ref::Ref::Weakest(_)) => "weakest".to_owned(),
            Some(beacon_ref::Ref::MostThreatened(_)) => "most_threatened".to_owned(),
            None => "beacon".to_owned(),
        },
        Some(protected_asset::Asset::MyCommander(_)) => strings::MY_COMMANDER.to_owned(),
        None => "asset".to_owned(),
    }
}

/// The JSON name of a `FeatureRef`'s arm, for a pointer.
pub(crate) const fn arm_name(chosen: &feature_ref::Ref) -> &'static str {
    match chosen {
        feature_ref::Ref::FeatureId(_) => "feature_id",
        feature_ref::Ref::Vent(_) => "vent",
        feature_ref::Ref::Seam(_) => "seam",
        feature_ref::Ref::Covered(_) => "covered",
    }
}

/// A beacon reference that names a beacon: a fixed id or one of the four
/// selectors, never nothing.
///
/// The same rule as [`check_location`], one level down. Which arm is set is not
/// this stage's business — a fixed id is resolved by [`crate::resolve`] and a
/// selector is late-bound and resolves at step start — but *that* one is set is.
fn check_beacon_ref(target: &BeaconRef, at: &str, out: &mut Builder) {
    if target.r#ref.is_none() {
        out.emit(Diag::new("E0111", at.to_owned()).arg("field", "ref"));
    }
}

/// One handler: its name, its condition, its firing limits and its body.
fn walk_handler(handler: &Handler, at: &str, limits: &Limits, out: &mut Builder) {
    if handler.id.is_empty() {
        out.emit(Diag::new("E0104", at).arg("what", "the handler"));
    }
    match handler.when.as_ref() {
        None => out.emit(Diag::new("E0106", at)),
        Some(when) => walk_condition_root(when, &pointer::child(at, "when"), out),
    }
    firing_limits(handler, at, limits, out);
    if handler.resume == i32::from(Resume::Unspecified) {
        out.emit(
            Diag::new("E0111", pointer::child(at, "resume"))
                .arg("field", "resume")
                .fix(
                    "Continue the route after the body",
                    // `add`, for the same reason `on_respawn` uses it.
                    patch_add(
                        &pointer::child(at, "resume"),
                        &pharmakos_proto::json::Json::String("CONTINUE".to_owned()),
                    ),
                    Applicability::MaybeIncorrect,
                ),
        );
    }
    let body_at = pointer::child(at, "body");
    for (index, entry) in handler.body.iter().enumerate() {
        walk_route_entry(entry, &pointer::at(&body_at, index), out);
    }
    unique_names(
        handler.body.iter().map(|entry| entry.label.as_str()),
        &body_at,
        "within a handler body",
        "label",
        out,
    );
}

/// "Each rule has a cooldown >= 5 s and fires at most 1-8 times" (spec section
/// 10), with both numbers read from the rules table.
fn firing_limits(handler: &Handler, at: &str, limits: &Limits, out: &mut Builder) {
    let ceiling = limits.max_fires_max();
    if handler.max_fires < 1 || handler.max_fires > ceiling {
        let clamped = handler.max_fires.clamp(1, ceiling.max(1));
        out.emit(
            Diag::new("E0107", pointer::child(at, "max_fires"))
                .arg("found", handler.max_fires)
                .arg("max", ceiling)
                .fix(
                    format!("Set max_fires to {clamped}"),
                    // `add`: the out-of-range value may be the *absent* one — a
                    // handler that never wrote `max_fires` reads as zero, which
                    // is below the floor of one and is what fires this.
                    patch_add(&pointer::child(at, "max_fires"), &number(clamped)),
                    Applicability::MaybeIncorrect,
                ),
        );
    }
    let floor = limits.handler_cooldown_min_ms();
    if handler.cooldown_ms < 0 {
        negative(out, at, "cooldown_ms", handler.cooldown_ms);
    } else if handler.cooldown_ms < floor {
        out.emit(
            Diag::new("E0108", pointer::child(at, "cooldown_ms"))
                .arg("found", handler.cooldown_ms)
                .arg("min", floor)
                .fix(
                    format!("Set the cooldown to {floor} ms"),
                    // `add`, and here it matters most: this is the one
                    // `MACHINE_APPLICABLE` fix in the stage, so the editor
                    // applies it unattended — and a handler with no
                    // `cooldown_ms` at all is the ordinary way to be below the
                    // floor.
                    patch_add(&pointer::child(at, "cooldown_ms"), &number(floor)),
                    Applicability::MachineApplicable,
                ),
        );
    }
}

/// A duration that ran backwards (item 46).
fn negative(out: &mut Builder, at: &str, field: &'static str, found: i32) {
    out.emit(
        Diag::new("E0109", pointer::child(at, field))
            .arg("field", field)
            .arg("found", found)
            .fix(
                format!("Set {field} to 0"),
                patch_replace(&pointer::child(at, field), &number(0)),
                Applicability::MaybeIncorrect,
            ),
    );
}

/// Names must be unique in the list that addresses them.
///
/// The route's labels are what a jump and `step_reached` name; a handler body's
/// labels are what a jump inside that body names; a handler's id is what
/// `rule_fired` names. Each list is checked against itself, which is the scope
/// the references actually use — a body label may repeat a route label without
/// ambiguity, and nothing in the vocabulary can confuse the two.
fn unique_names<'a>(
    names: impl Iterator<Item = &'a str>,
    at: &str,
    scope: &'static str,
    field: &'static str,
    out: &mut Builder,
) {
    // An ordered map rather than a linear scan, and the choice is load-bearing
    // twice over. `BTreeMap` keeps iteration order a function of the keys
    // (AGENTS.md section 4.6) where a `HashMap` would not; and the lookup is
    // logarithmic where the scan it replaces was linear, which over a list this
    // walks once made the whole check quadratic in the number of steps. This is
    // the one crate that reads hand-written files and the gateway calls QUICK on
    // every edit (spec section 11 budgets it at 5 ms p99), so the cost of a
    // large playbook is not hypothetical. Diagnostics are still emitted in index
    // order, so nothing about the report moves.
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for (index, name) in names.enumerate() {
        if name.is_empty() {
            continue;
        }
        match seen.get(name) {
            Some(first) => out.emit(
                Diag::new("E0105", pointer::child(&pointer::at(at, index), field))
                    .arg("name", name)
                    .arg("scope", scope)
                    .related(pointer::child(&pointer::at(at, *first), field)),
            ),
            None => {
                seen.insert(name, index);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Conditions
// ---------------------------------------------------------------------------

/// One whole condition: its two limits, then its shape.
fn walk_condition_root(condition: &Condition, at: &str, out: &mut Builder) {
    let depth = tree_depth(condition);
    if depth > CONDITION_MAX_DEPTH {
        out.emit(
            Diag::new("E0204", at)
                .arg("found", depth)
                .arg("max", CONDITION_MAX_DEPTH),
        );
    }
    let nodes = tree_nodes(condition);
    if nodes > CONDITION_MAX_NODES {
        out.emit(
            Diag::new("E0205", at)
                .arg("found", nodes)
                .arg("max", CONDITION_MAX_NODES),
        );
    }
    walk_condition(condition, at, out);
}

/// How deeply a condition nests. A bare predicate is one level.
fn tree_depth(condition: &Condition) -> u32 {
    let inner = match condition.node.as_ref() {
        Some(condition::Node::All(all)) => all.items.iter().map(tree_depth).max().unwrap_or(0),
        Some(condition::Node::Any(any)) => any.items.iter().map(tree_depth).max().unwrap_or(0),
        Some(condition::Node::Not(not)) => not.item.as_deref().map_or(0, tree_depth),
        _ => 0,
    };
    inner.saturating_add(1)
}

/// How many nodes a condition holds, itself included.
fn tree_nodes(condition: &Condition) -> u32 {
    let inner: u32 = match condition.node.as_ref() {
        Some(condition::Node::All(all)) => all
            .items
            .iter()
            .map(tree_nodes)
            .fold(0, u32::saturating_add),
        Some(condition::Node::Any(any)) => any
            .items
            .iter()
            .map(tree_nodes)
            .fold(0, u32::saturating_add),
        Some(condition::Node::Not(not)) => not.item.as_deref().map_or(0, tree_nodes),
        _ => 0,
    };
    inner.saturating_add(1)
}

/// One condition node's shape, and then its children.
fn walk_condition(condition: &Condition, at: &str, out: &mut Builder) {
    let Some(node) = condition.node.as_ref() else {
        out.emit(Diag::new("E0206", at));
        return;
    };
    match node {
        condition::Node::All(all) => {
            let here = pointer::child(at, "all");
            if all.items.is_empty() {
                out.emit(Diag::new("E0203", here.clone()).arg("node", "all"));
            }
            walk_items(&all.items, &pointer::child(&here, "items"), out);
        }
        condition::Node::Any(any) => {
            let here = pointer::child(at, "any");
            if any.items.is_empty() {
                out.emit(Diag::new("E0203", here.clone()).arg("node", "any"));
            }
            walk_items(&any.items, &pointer::child(&here, "items"), out);
        }
        condition::Node::Not(not) => {
            let here = pointer::child(at, "not");
            match not.item.as_ref() {
                None => out.emit(Diag::new("E0203", here).arg("node", "not")),
                Some(inner) => walk_condition(inner, &pointer::child(&here, "item"), out),
            }
        }
        other => walk_predicate(other, at, out),
    }
}

fn walk_items(items: &[Condition], at: &str, out: &mut Builder) {
    for (index, item) in items.iter().enumerate() {
        walk_condition(item, &pointer::at(at, index), out);
    }
}

/// One leaf predicate: its comparison, its ranges, its durations.
fn walk_predicate(node: &condition::Node, at: &str, out: &mut Builder) {
    match node {
        condition::Node::CmdrHpPct(predicate) => {
            let here = pointer::child(at, "cmdr_hp_pct");
            if predicate.cmp() == pharmakos_proto::gp::v1::cmdr_hp_pct::Cmp::Unspecified {
                out.emit(Diag::new("E0201", pointer::child(&here, "cmp")));
            }
            percent(out, &pointer::child(&here, "pct"), i64::from(predicate.pct));
        }
        condition::Node::CmdrTookDamageWithin(predicate) => {
            let here = pointer::child(at, "cmdr_took_damage_within");
            if predicate.ms < 0 {
                negative(out, &here, "ms", predicate.ms);
            }
        }
        condition::Node::CmdrDeaths(predicate) => {
            compare(
                predicate.deaths.as_ref(),
                &pointer::child(at, "cmdr_deaths"),
                "deaths",
                false,
                out,
            );
        }
        condition::Node::SegmentElapsed(predicate) => {
            compare(
                predicate.ms.as_ref(),
                &pointer::child(at, "segment_elapsed"),
                "ms",
                false,
                out,
            );
        }
        condition::Node::BeaconHpPct(predicate) => {
            let here = pointer::child(at, "beacon_hp_pct");
            beacon_ref(predicate.beacon.is_some(), &here, out);
            compare(predicate.pct.as_ref(), &here, "pct", true, out);
        }
        condition::Node::BeaconUnderAttack(predicate) => {
            let here = pointer::child(at, "beacon_under_attack");
            beacon_ref(predicate.beacon.is_some(), &here, out);
            if predicate.within_ms < 0 {
                negative(out, &here, "within_ms", predicate.within_ms);
            }
        }
        condition::Node::BeaconPowered(predicate) => {
            beacon_ref(
                predicate.beacon.is_some(),
                &pointer::child(at, "beacon_powered"),
                out,
            );
        }
        condition::Node::Treasury(predicate) => {
            compare(
                predicate.dollars.as_ref(),
                &pointer::child(at, "treasury"),
                "dollars",
                false,
                out,
            );
        }
        condition::Node::KwHeadroom(predicate) => {
            compare(
                predicate.kw.as_ref(),
                &pointer::child(at, "kw_headroom"),
                "kw",
                false,
                out,
            );
        }
        condition::Node::StepReached(predicate) => {
            if predicate.label.is_empty() {
                out.emit(
                    Diag::new(
                        "E0101",
                        pointer::child(&pointer::child(at, "step_reached"), "label"),
                    )
                    .arg("what", "`label`"),
                );
            }
        }
        condition::Node::RuleFired(predicate) => {
            if predicate.handler_id.is_empty() {
                out.emit(
                    Diag::new(
                        "E0101",
                        pointer::child(&pointer::child(at, "rule_fired"), "handler_id"),
                    )
                    .arg("what", "`handler_id`"),
                );
            }
        }
        condition::Node::All(_) | condition::Node::Any(_) | condition::Node::Not(_) => {}
    }
}

/// A predicate's `BeaconRef`, which is required wherever it appears.
fn beacon_ref(present: bool, at: &str, out: &mut Builder) {
    if !present {
        out.emit(Diag::new("E0101", pointer::child(at, "beacon")).arg("what", "`beacon`"));
    }
}

/// One `IntCompare`: present, with an operator, and inside its range if it is a
/// percentage.
fn compare(
    value: Option<&IntCompare>,
    at: &str,
    field: &'static str,
    is_percent: bool,
    out: &mut Builder,
) {
    let here = pointer::child(at, field);
    let Some(comparison) = value else {
        out.emit(Diag::new("E0101", here).arg("what", format!("`{field}`")));
        return;
    };
    if comparison.op() == int_compare::Op::Unspecified {
        out.emit(Diag::new("E0201", pointer::child(&here, "op")));
    }
    if is_percent {
        percent(
            out,
            &pointer::child(&here, "value"),
            i64::from(comparison.value),
        );
    }
}

/// "Percent, integer. 0-100."
fn percent(out: &mut Builder, at: &str, found: i64) {
    if (0..=100).contains(&found) {
        return;
    }
    let clamped = found.clamp(0, 100);
    out.emit(Diag::new("E0202", at).arg("found", found).fix(
        format!("Clamp it to {clamped}"),
        patch_replace(at, &number(clamped)),
        Applicability::MaybeIncorrect,
    ));
}
