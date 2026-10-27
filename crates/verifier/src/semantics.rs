// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stage 4, **semantics**: the rules that need meaning.
//!
//! Where [`crate::structure`] asks "is this the right shape" and
//! [`crate::resolve`] asks "does this name anything", this stage asks "is this
//! allowed" — and it is the last stage of QUICK.
//!
//! # Termination, the half that needs positions
//!
//! Spec section 10: "a jump only ever goes forward, and every wait has a
//! timeout". Both are here, because both need the label table
//! [`crate::resolve`] built. Together with the size budget and the firing limits
//! they are the whole of why a playbook halts: the route is finite, it only ever
//! moves down it, and nothing in it can block for ever.
//!
//! # Placement, and what this stage cannot know
//!
//! A `place_beacon` site "must lie inside one of your own spheres, and not on
//! the column of one of your own live beacons" (`gp.v1.PlaceBeaconStep`). The
//! commander walks there first, and there is no placement range to stand
//! within any more (walk-in; decisions-log item 127 (12), amending item 11).
//! The sphere is a fact about the sealed playbook and the seat's view, so
//! `E0403` checks it; whether the walk arrives, and whether the site is still
//! legal when it does, are run-time failures that `on_fail` answers like any
//! other (`no_path`, `illegal_site`).
//!
//! # Targeting: what a name or a pick under `on` is
//!
//! `on` stands a structure on a heat vent (docs/design/targeting.md, "Sites":
//! the pick "ranks only vents"), so a seam under it — a seam pick, a seam's
//! name, or `covered {}` under a deploy that covers a seam — is `E0413`. This
//! is S1's targeting verifier's reading (task `tgtv`): the page defines `on`
//! over vents alone, and S1's only blueprint is the Generator.
//! `seam_choice: SAFEST` read as NEAREST until S2's threat model and was
//! `W0501` (decisions-log item 127 (9), S1's plan's decision 6); S2's `dfnd`
//! built the model, so nothing raises it now: SAFEST digs the nearest seam no
//! known threat's weapon reaches (S2's plan, decision 12).
//!
//! # Avoiding known threats with none sighted (task `vfy3`)
//!
//! A move paced `AVOID_KNOWN_THREATS` adds the rules row's surcharge per step
//! inside a known threat's range (decision 12), and a known threat is one the
//! seat sees when the step starts. A view with no sighting of another seat
//! has none, so the move routes as `DIRECT` would if it started now, and
//! `I0005` says so: the editor's explanation and no more.
//!
//! # S2's Defend and Attack, and the wall's sphere (task `vfy2`)
//!
//! A protect list names the seat's own beacons and an Attack target an
//! enemy's (`E0414`); an Attack target named by its handle, or the enemy
//! commander, rests on a sighting inside the reach window (`E0418`), the
//! sightings' ages read and compared and nothing ranked; a picket is as many
//! scouts as a beacon holds (`E0508`); and a wall box's columns stand inside
//! the sphere of the beacon whose mandate holds it (`E0416`), by the column
//! test the view allows.
//!
//! # No dry runs
//!
//! Everything below is a comparison between a number in the file, a number in
//! the rules table and a fact in the seat's own frozen view. Nothing is
//! projected, nothing is simulated, no condition is evaluated over a future
//! world (AGENTS.md section 3 rule 2).

use pharmakos_proto::gp::api::v1::patch_suggestion::Applicability;
use pharmakos_proto::gp::v1::beacon_filter::{MandateKind, Side};
use pharmakos_proto::gp::v1::feature_ref;
use pharmakos_proto::gp::v1::handler::Resume;
use pharmakos_proto::gp::v1::move_step::Pace;
use pharmakos_proto::gp::v1::on_fail::Action;
use pharmakos_proto::gp::v1::{
    Area, AttackSettings, BeaconFilter, BuildTarget, DefendSettings, FeatureRef, InterfaceStep,
    Location, MandateSettings, Playbook, Step, Voxel, attack_target, beacon_ref, interface_row,
    location, mandate_settings, protected_asset, step,
};

use crate::limits::Limits;
use crate::pointer;
use crate::report::{Builder, Diag, number, patch_add, patch_replace};
use crate::resolve::Symbols;
use crate::scope::{FeatureKind, Ownership, Scope};
use crate::strings;
use crate::structure::arm_name;
use crate::walk::{self, AreaOf, Carrier, Holder, List, Slot, Visit};

/// Run the stage.
pub(crate) fn run(
    playbook: &Playbook,
    scope: &Scope,
    limits: &Limits,
    symbols: &Symbols,
    out: &mut Builder,
) {
    let mut visitor = Semantics {
        scope,
        limits,
        symbols,
        out,
    };
    walk::walk(playbook, &mut visitor);

    // A handler's resume jump, which hangs off the handler rather than a step.
    if let Some(declarative) = playbook.declarative.as_ref() {
        for (index, handler) in declarative.handlers.iter().enumerate() {
            if handler.resume() == Resume::JumpForward && handler.resume_at_label.is_empty() {
                out.emit(Diag::new(
                    "E0304",
                    pointer::child(&pointer::at("/declarative/handlers", index), "resume"),
                ));
            }
        }
    }
}

impl Semantics<'_> {
    /// `I0005` (S2's plan, decision 12): a move paced `AVOID_KNOWN_THREATS`
    /// prices the threats the seat sees when the step starts, so with no
    /// sighting of another seat in the view there is nothing to route round,
    /// and a route started now goes as `DIRECT` does. The note reads whether
    /// the view holds a sighting and nothing more: it ranks none, prices no
    /// route and guesses at no threat the Push may bring (AGENTS.md section 3
    /// rule 2).
    fn pace(&mut self, at: &str, entry: &Step) {
        let Some(step::Kind::Move(walk_to)) = entry.kind.as_ref() else {
            return;
        };
        if walk_to.pace == i32::from(Pace::AvoidKnownThreats) && self.scope.sightings().is_empty() {
            self.out.emit(Diag::new(
                "I0005",
                pointer::child(&pointer::child(at, "move"), "pace"),
            ));
        }
    }
}

struct Semantics<'a> {
    scope: &'a Scope,
    limits: &'a Limits,
    symbols: &'a Symbols,
    out: &'a mut Builder,
}

impl Visit for Semantics<'_> {
    fn entry(&mut self, at: &str, entry: &Step, list: List, index: usize) {
        self.jump(at, entry, list, index);
        self.waits(at, entry);
        self.pace(at, entry);
    }

    fn interface(&mut self, at: &str, interface: &InterfaceStep) {
        let Some(target) = interface.beacon.as_ref() else {
            return;
        };
        let Some(beacon_ref::Ref::BeaconId(id)) = target.r#ref.as_ref() else {
            // A selector is late-bound: what it picks is not known until the
            // step starts, so nothing here can judge it (spec section 10).
            return;
        };
        let Some(known) = self.scope.beacon(id) else {
            // Unknown to the seat: `E0401` from the resolve stage already says
            // so, and saying it twice helps nobody.
            return;
        };
        if known.side == Ownership::EnemyKnown {
            self.out.emit(
                Diag::new(
                    "E0405",
                    pointer::child(&pointer::child(at, "beacon"), "beacon_id"),
                )
                .arg("id", id),
            );
            return;
        }
        let rows_base = pointer::child(at, "rows");
        for (index, row) in interface.rows.iter().enumerate() {
            let row_at = pointer::at(&rows_base, index);
            match row.row.as_ref() {
                Some(interface_row::Row::Recycle(_)) if known.is_core => {
                    self.out.emit(
                        Diag::new("E0504", pointer::child(&row_at, "recycle"))
                            .arg("id", id)
                            .related(pointer::child(&pointer::child(at, "beacon"), "beacon_id")),
                    );
                }
                Some(interface_row::Row::AddBuildTarget(_))
                    if known.mandate != MandateKind::Build =>
                {
                    self.out.emit(
                        Diag::new("E0503", pointer::child(&row_at, "add_build_target"))
                            .arg("id", id)
                            .arg("mandate", known.mandate.as_str_name())
                            .related(pointer::child(&pointer::child(at, "beacon"), "beacon_id")),
                    );
                }
                _ => {}
            }
        }
    }

    fn place_site(&mut self, at: &str, site: &Location) {
        let Some(location::Place::Voxel(voxel)) = site.place.as_ref() else {
            // A site given as a beacon anchor is that beacon's own place, which
            // is inside its own sphere by construction.
            return;
        };
        if !self.in_bounds(voxel) {
            // `E0402` already says the voxel is off the map.
            return;
        }
        if self.scope.own_beacons().next().is_none() {
            // The seat knows no beacon of its own, so there is no sphere to test
            // against. A seat always has a core, so this is a scope the gateway
            // could only build before a match starts.
            return;
        }
        let radius = i64::from(self.limits.beacon_sphere_radius_voxels());
        let inside = self
            .scope
            .own_beacons()
            .any(|beacon| squared_distance(&beacon.at, voxel) <= radius.saturating_mul(radius));
        if !inside {
            self.out.emit(
                Diag::new("E0403", pointer::child(at, "voxel"))
                    .arg("x", voxel.x)
                    .arg("y", voxel.y)
                    .arg("z", voxel.z)
                    .map_ref(*voxel),
            );
        }
    }

    fn voxel(&mut self, at: &str, voxel: &Voxel) {
        if self.in_bounds(voxel) {
            return;
        }
        let [sx, sy, sz] = self.limits.map_size();
        self.out.emit(
            Diag::new("E0402", at)
                .arg("x", voxel.x)
                .arg("y", voxel.y)
                .arg("z", voxel.z)
                .arg("sx", sx)
                .arg("sy", sy)
                .arg("sz", sz)
                .map_ref(*voxel),
        );
    }

    fn filter(&mut self, at: &str, filter: &BeaconFilter) {
        // `gp.v1.BeaconFilter`: "An enemy beacon carries none of your tags, so a
        // tag filter and ENEMY_KNOWN together select nothing — a verifier
        // warning, not an error."
        if filter.side() == Side::EnemyKnown && !filter.tags.is_empty() {
            self.out.emit(
                Diag::new("E0404", at)
                    .related(pointer::child(at, "tags"))
                    .fix(
                        "Drop the tag filter",
                        crate::report::patch_remove(&pointer::child(at, "tags")),
                        Applicability::MaybeIncorrect,
                    ),
            );
        }
    }

    fn area(&mut self, at: &str, area: &Area, _of: AreaOf) {
        let (Some(low), Some(high)) = (area.min.as_ref(), area.max.as_ref()) else {
            // A half-written area is `E0101`'s business, not this check's.
            return;
        };
        if low.x <= high.x && low.y <= high.y && low.z <= high.z {
            return;
        }
        self.out.emit(
            Diag::new("E0406", at)
                .arg("minx", low.x)
                .arg("miny", low.y)
                .arg("minz", low.z)
                .arg("maxx", high.x)
                .arg("maxy", high.y)
                .arg("maxz", high.z)
                .map_ref(*low)
                .map_ref(*high),
        );
    }

    fn location(&mut self, at: &str, place: &Location, slot: Slot<'_>) {
        let Some(location::Place::On(feature)) = place.place.as_ref() else {
            return;
        };
        let deploy = match slot {
            Slot::BuildAnchor { deploy } => deploy,
            Slot::RemoveAnchor => None,
            // `E0408` from the structure stage: `on` cannot stand here at all.
            Slot::PlaceSite | Slot::Elsewhere => return,
        };
        let Some(chosen) = feature.r#ref.as_ref() else {
            return;
        };
        let removing = matches!(slot, Slot::RemoveAnchor);
        let what = match chosen {
            // A description under remove_build_target is `E0410` already.
            feature_ref::Ref::Seam(_) if !removing => strings::SEAM_PICK.to_owned(),
            feature_ref::Ref::FeatureId(id) if self.is_seam(id) => format!("`{id}`"),
            feature_ref::Ref::Covered(_)
                if deploy.is_some_and(|cover| self.names_a_seam(cover)) =>
            {
                strings::COVERED_SEAM.to_owned()
            }
            _ => return,
        };
        self.out.emit(
            Diag::new(
                "E0413",
                pointer::child(&pointer::child(at, "on"), arm_name(chosen)),
            )
            .arg("what", what),
        );
    }

    fn mandate(&mut self, at: &str, settings: &MandateSettings, _carrier: Carrier) {
        if settings.mandate.is_none() {
            self.out.emit(Diag::new("E0502", at));
        }
        if let Some(mandate_settings::Mandate::Survey(survey)) = settings.mandate.as_ref() {
            self.scouts(
                &pointer::child(at, "survey"),
                "scout_count",
                survey.scout_count,
            );
        }
        match settings.mandate.as_ref() {
            Some(mandate_settings::Mandate::Defend(defend)) => {
                self.defend(&pointer::child(at, "defend"), defend);
            }
            Some(mandate_settings::Mandate::Attack(attack)) => {
                self.attack(&pointer::child(at, "attack"), attack);
            }
            _ => {}
        }
        self.percent(at, "retreat_hp_pct", settings.retreat_hp_pct);
        if let Some(mandate_settings::Mandate::Build(build)) = settings.mandate.as_ref() {
            self.percent(
                &pointer::child(at, "build"),
                "repair_threshold_pct",
                build.repair_threshold_pct,
            );
        }
    }

    fn held_target(&mut self, at: &str, target: &BuildTarget, holder: Holder<'_>) {
        self.anchor_sphere(at, target, holder);
        self.wall_sphere(at, target, holder);
    }

    fn build_target(&mut self, at: &str, target: &BuildTarget) {
        if target.rotation_quarter_turns <= 3 {
            return;
        }
        let at_field = pointer::child(at, "rotation_quarter_turns");
        self.out.emit(
            Diag::new("E0505", at_field.clone())
                .arg("found", target.rotation_quarter_turns)
                .fix(
                    "Bring the rotation into range",
                    patch_replace(&at_field, &number(target.rotation_quarter_turns % 4)),
                    Applicability::MaybeIncorrect,
                ),
        );
    }
}

impl Semantics<'_> {
    /// S2's Defend settings against the seat's view: a protect list names the
    /// seat's own beacons (`E0414`), and a picket is as many scouts as a
    /// beacon holds (`E0508`, as Survey's count).
    fn defend(&mut self, at: &str, defend: &DefendSettings) {
        let protect_at = pointer::child(at, "protect");
        for (index, entry) in defend.protect.iter().enumerate() {
            let Some(protected_asset::Asset::Beacon(beacon)) = entry.asset.as_ref() else {
                continue;
            };
            let Some(beacon_ref::Ref::BeaconId(id)) = beacon.r#ref.as_ref() else {
                continue;
            };
            if let Some(known) = self.scope.beacon(id)
                && known.side == Ownership::EnemyKnown
            {
                self.out.emit(
                    Diag::new(
                        "E0414",
                        pointer::child(
                            &pointer::child(&pointer::at(&protect_at, index), "beacon"),
                            "beacon_id",
                        ),
                    )
                    .arg("id", id)
                    .arg("side", strings::SIDE_ENEMY)
                    .arg("role", strings::ROLE_PROTECT)
                    .arg("wanted", strings::WANTED_OWN),
                );
            }
        }
        if let Some(pickets) = defend.picket_scouts {
            // The picket's ceiling is Defend's own byte column (the sim's
            // `DefendSettings::pickets`, S2's `dfnd`; register S2-18).
            self.scouts(at, "picket_scouts", pickets);
        }
    }

    /// S2's Attack target against the seat's view: a handle names an enemy
    /// beacon (`E0414`), and a target named by its handle, or the enemy
    /// commander, rests on a sighting inside the reach window (`E0418`).
    ///
    /// **Freshness is a plan-time test** (spec section 6: "Freshness is a
    /// qualification test at plan time only; the target is pinned at the
    /// seal"), and spec section 10 takes it from the seat's one reach window.
    /// The check reads the sightings' ages and compares them with the window:
    /// it never ranks a sighting, never guesses where an asset went since, and
    /// never models an enemy (AGENTS.md section 3 rule 2). A selector target
    /// takes its freshness from the same window when its step starts, in the
    /// sim, so nothing is judged here for one; a voxel target rests on no
    /// sighting.
    fn attack(&mut self, at: &str, attack: &AttackSettings) {
        let Some(target) = attack.target.as_ref() else {
            return;
        };
        let target_at = pointer::child(at, "target");
        let window = i64::from(self.limits.reach_memory_ms());
        match target.target.as_ref() {
            Some(attack_target::Target::Beacon(beacon)) => {
                let Some(beacon_ref::Ref::BeaconId(id)) = beacon.r#ref.as_ref() else {
                    return;
                };
                let here = pointer::child(&pointer::child(&target_at, "beacon"), "beacon_id");
                let Some(known) = self.scope.beacon(id) else {
                    // `E0401` from the resolve stage.
                    return;
                };
                if known.side == Ownership::Own {
                    self.out.emit(
                        Diag::new("E0414", here)
                            .arg("id", id)
                            .arg("side", strings::SIDE_OWN)
                            .arg("role", strings::ROLE_ATTACK_TARGET)
                            .arg("wanted", strings::WANTED_ENEMY),
                    );
                    return;
                }
                match self.scope.beacon_sighting(id) {
                    None => self.out.emit(
                        Diag::new("E0418", here)
                            .arg("what", strings::never_sighted(id))
                            .arg("max", window)
                            .map_ref(known.at),
                    ),
                    Some(sighting) if i64::from(sighting.age_ms) > window => self.out.emit(
                        Diag::new("E0418", here)
                            .arg(
                                "what",
                                strings::sighted_ago(&format!("`{id}`"), sighting.age_ms),
                            )
                            .arg("max", window)
                            .map_ref(sighting.at),
                    ),
                    Some(_) => {}
                }
            }
            Some(attack_target::Target::EnemyCommander(_)) => {
                let here = pointer::child(&target_at, "enemy_commander");
                // The freshest sighting of any enemy commander: an age is
                // compared, nothing is ranked, and which commander the target
                // pins is the sim's rule (S2's `atk`).
                let freshest = self
                    .scope
                    .commander_sightings()
                    .min_by_key(|seen| seen.age_ms);
                match freshest {
                    None => self.out.emit(
                        Diag::new("E0418", here)
                            .arg("what", strings::NO_COMMANDER_SIGHTED)
                            .arg("max", window),
                    ),
                    Some(sighting) if i64::from(sighting.age_ms) > window => self.out.emit(
                        Diag::new("E0418", here)
                            .arg(
                                "what",
                                strings::sighted_ago(strings::THE_ENEMY_COMMANDER, sighting.age_ms),
                            )
                            .arg("max", window)
                            .map_ref(sighting.at),
                    ),
                    Some(_) => {}
                }
            }
            Some(attack_target::Target::Voxel(_)) | None => {}
        }
    }

    /// `E0416`: a wall box's column outside the sphere of the beacon whose
    /// mandate holds the wall (decisions-log item 12: the sphere is
    /// building's only range).
    ///
    /// **The column test, which the file and the view can make.** A column
    /// whose distance from the beacon in x and y alone is past the radius has
    /// no voxel inside the sphere at any height, so it is refused here; the
    /// sphere is round, so the box's four corner columns are the farthest and
    /// are the ones tested. Whether every wall voxel, from the ground's top to
    /// `max.z`, lies inside the sphere in three dimensions needs the ground,
    /// which the view does not carry, and is the sim's to judge when it builds
    /// (S2's `site`). A beacon the file does not place or name by its handle
    /// (a deploy `covering` a feature, a selector) is not known until the
    /// Push, so nothing is judged for it.
    fn wall_sphere(&mut self, at: &str, target: &BuildTarget, holder: Holder<'_>) {
        let Some(wall_box) = target.wall_box.as_ref() else {
            return;
        };
        let (Some(low), Some(high)) = (wall_box.min.as_ref(), wall_box.max.as_ref()) else {
            return;
        };
        if low.x > high.x || low.y > high.y || low.z > high.z {
            return;
        }
        let Some(centre) = self.holder_centre(holder) else {
            return;
        };
        for (x, y) in [
            (low.x, low.y),
            (high.x, low.y),
            (low.x, high.y),
            (high.x, high.y),
        ] {
            if self.column_outside(&centre, x, y) {
                self.out.emit(
                    Diag::new("E0416", pointer::child(at, "wall_box"))
                        .arg("what", strings::WALL_BOX_COLUMN)
                        .arg("x", x)
                        .arg("y", y)
                        .map_ref(Voxel { x, y, z: centre.z }),
                );
                return;
            }
        }
    }

    /// `E0416` for a Build target's voxel anchor, whatever its blueprint (the
    /// Generator and S2's base kit alike): "Must lie inside one of your own
    /// spheres: inside the sphere of the beacon whose mandate holds the
    /// target" (`playbook.proto`'s `BuildTarget.anchor`). The same column
    /// test as [`Semantics::wall_sphere`], for the same reason: the view
    /// carries no ground, so the height half is the sim's at the site. An
    /// anchor `on` a feature is placed by the sim inside the sphere by
    /// construction, and an unnamed holder is not known until the Push.
    fn anchor_sphere(&mut self, at: &str, target: &BuildTarget, holder: Holder<'_>) {
        let Some(location::Place::Voxel(anchor)) = target
            .anchor
            .as_ref()
            .and_then(|anchor| anchor.place.as_ref())
        else {
            return;
        };
        let Some(centre) = self.holder_centre(holder) else {
            return;
        };
        if self.column_outside(&centre, anchor.x, anchor.y) {
            self.out.emit(
                Diag::new(
                    "E0416",
                    pointer::child(&pointer::child(at, "anchor"), "voxel"),
                )
                .arg("what", strings::ANCHOR_COLUMN)
                .arg("x", anchor.x)
                .arg("y", anchor.y)
                .map_ref(*anchor),
            );
        }
    }

    /// Where the beacon holding a target stands, when the file names it: a
    /// deploy's voxel site, or one of the seat's own beacons visited by its
    /// handle. Anything else is not known before the Push.
    fn holder_centre(&self, holder: Holder<'_>) -> Option<Voxel> {
        match holder {
            Holder::Site(voxel) => Some(*voxel),
            Holder::Visited(beacon) => {
                let Some(beacon_ref::Ref::BeaconId(id)) = beacon.r#ref.as_ref() else {
                    return None;
                };
                // Unknown is `E0401`'s, and an enemy beacon is `E0405`'s.
                self.scope
                    .beacon(id)
                    .filter(|known| known.side == Ownership::Own)
                    .map(|known| known.at)
            }
            Holder::Unnamed => None,
        }
    }

    /// Whether a column lies wholly outside the sphere about `centre`: its
    /// distance in x and y alone is past the radius, so no voxel of it at any
    /// height is inside.
    fn column_outside(&self, centre: &Voxel, x: i32, y: i32) -> bool {
        let radius = i64::from(self.limits.beacon_sphere_radius_voxels());
        let column = Voxel { x, y, z: centre.z };
        squared_distance(centre, &column) > radius.saturating_mul(radius)
    }

    /// "A jump only ever goes forward" — and it has to have somewhere to go.
    fn jump(&mut self, at: &str, entry: &Step, list: List, index: usize) {
        let Some(on_fail) = entry.on_fail.as_ref() else {
            return;
        };
        if on_fail.action() != Action::JumpForward {
            return;
        }
        let here = pointer::child(at, "on_fail");
        if on_fail.jump_to_label.is_empty() {
            self.out.emit(Diag::new("E0304", here));
            return;
        }
        let Some(target) = self.symbols.index_in(list, &on_fail.jump_to_label) else {
            // `E0305` from the resolve stage.
            return;
        };
        if target <= index {
            self.out.emit(
                Diag::new("E0301", pointer::child(&here, "jump_to_label"))
                    .arg("label", &on_fail.jump_to_label)
                    .related(list_entry(list, target)),
            );
        }
    }

    /// "Every wait has a timeout", and a hold lasts a positive time.
    fn waits(&mut self, at: &str, entry: &Step) {
        match entry.kind.as_ref() {
            Some(pharmakos_proto::gp::v1::step::Kind::WaitUntil(_)) if entry.timeout_ms <= 0 => {
                self.out
                    .emit(Diag::new("E0302", pointer::child(at, "timeout_ms")).fix(
                        "Add a timeout",
                        // `add`, and the label says why: a wait with no timeout
                        // is a step with **no `timeout_ms` member at all**, so
                        // an RFC 6902 `replace` would be refused by a
                        // conformant applier (see `crate::report`).
                        patch_add(&pointer::child(at, "timeout_ms"), &number(30_000)),
                        Applicability::HasPlaceholders,
                    ));
            }
            Some(pharmakos_proto::gp::v1::step::Kind::Hold(hold)) if hold.ms <= 0 => {
                let field = pointer::child(&pointer::child(at, "hold"), "ms");
                self.out
                    .emit(Diag::new("E0303", field.clone()).arg("found", hold.ms).fix(
                        "Hold for a while",
                        // `add`: `"hold": {}` is the ordinary way to write a
                        // hold with no duration, and there is nothing at
                        // `/hold/ms` for a `replace` to target.
                        patch_add(&field, &number(5_000)),
                        Applicability::HasPlaceholders,
                    ));
            }
            _ => {}
        }
    }

    /// S2-15: a `scout_count` past the byte a beacon's column holds, which
    /// the sim's compile refuses (`PlanError::ScoutsOutOfRange`). The most is
    /// the column's type, `u8`, and not a rules row: it is the sim's storage,
    /// a technical limit and not a balance cap, so it is read from the type
    /// rather than written down a second time.
    fn scouts(&mut self, at: &str, field: &'static str, found: u32) {
        let most = u32::from(u8::MAX);
        if found <= most {
            return;
        }
        let here = pointer::child(at, field);
        self.out.emit(
            Diag::new("E0508", here.clone())
                .arg("field", field)
                .arg("found", found)
                .arg("most", most)
                .fix(
                    "Ask for as many scouts as a beacon holds",
                    patch_replace(&here, &number(most)),
                    Applicability::MaybeIncorrect,
                ),
        );
    }

    /// "Integer percent, 0-100."
    fn percent(&mut self, at: &str, field: &'static str, found: u32) {
        if found <= 100 {
            return;
        }
        let here = pointer::child(at, field);
        self.out.emit(
            Diag::new("E0501", here.clone())
                .arg("field", field)
                .arg("found", found)
                .fix(
                    "Clamp it to 100",
                    patch_replace(&here, &number(100)),
                    Applicability::MaybeIncorrect,
                ),
        );
    }

    /// Whether the seat's view holds a feature of this name, and it is a seam.
    /// A name the view does not hold is `E0412`'s, not this stage's.
    fn is_seam(&self, feature_id: &str) -> bool {
        self.scope
            .feature(feature_id)
            .is_some_and(|known| known.kind == FeatureKind::Seam)
    }

    /// Whether a `covering` reference names a seam: a seam pick, or the name
    /// of a seam the view holds.
    fn names_a_seam(&self, cover: &FeatureRef) -> bool {
        match cover.r#ref.as_ref() {
            Some(feature_ref::Ref::Seam(_)) => true,
            Some(feature_ref::Ref::FeatureId(id)) => self.is_seam(id),
            _ => false,
        }
    }

    /// Whether a voxel is a place on this map at all.
    fn in_bounds(&self, voxel: &Voxel) -> bool {
        let [sx, sy, sz] = self.limits.map_size();
        let within = |value: i32, extent: i32| value >= 0 && value < extent;
        within(voxel.x, sx) && within(voxel.y, sy) && within(voxel.z, sz)
    }
}

/// The pointer to the step at `index` of the list `list` names.
fn list_entry(list: List, index: usize) -> String {
    match list {
        List::Route => pointer::at("/declarative/route", index),
        List::Body(handler) => pointer::at(
            &pointer::child(&pointer::at("/declarative/handlers", handler), "body"),
            index,
        ),
    }
}

/// Squared distance between two voxels, in whole voxels squared.
///
/// `i64` and squared, never a square root: a range check is `d2 <= r2` and there
/// is no float anywhere near it (AGENTS.md section 4.2). The map is 384 voxels
/// on its longest side, so the product cannot come close to the type's range —
/// and it saturates rather than wrapping, because overflow checks are on and a
/// panic inside the verifier would turn a silly playbook into a crash.
fn squared_distance(a: &Voxel, b: &Voxel) -> i64 {
    let axis = |one: i32, other: i32| {
        let delta = i64::from(one).saturating_sub(i64::from(other));
        delta.saturating_mul(delta)
    };
    axis(a.x, b.x)
        .saturating_add(axis(a.y, b.y))
        .saturating_add(axis(a.z, b.z))
}
