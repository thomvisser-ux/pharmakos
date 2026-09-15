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
//! # Placement, and the one thing this stage cannot know
//!
//! A `place_beacon` site "must lie inside one of your own spheres **and** within
//! the commander's placement range" (`gp.v1.PlaceBeaconStep`). Only the first
//! half is a fact about the sealed playbook: the second is measured from the
//! commander to the site *at the moment the step runs*, and the commander walks
//! there first. So `E0403` checks the sphere and says so, and the placement
//! range is a run-time failure that `on_fail` answers like any other.
//!
//! # No dry runs
//!
//! Everything below is a comparison between a number in the file, a number in
//! the rules table and a fact in the seat's own frozen view. Nothing is
//! projected, nothing is simulated, no condition is evaluated over a future
//! world (AGENTS.md section 3 rule 2).

use pharmakos_proto::gp::api::v1::patch_suggestion::Applicability;
use pharmakos_proto::gp::v1::beacon_filter::{MandateKind, Side};
use pharmakos_proto::gp::v1::handler::Resume;
use pharmakos_proto::gp::v1::on_fail::Action;
use pharmakos_proto::gp::v1::{
    Area, BeaconFilter, BuildTarget, InterfaceStep, Location, MandateSettings, Playbook, Step,
    Voxel, beacon_ref, interface_row, location, mandate_settings,
};

use crate::limits::Limits;
use crate::pointer;
use crate::report::{Builder, Diag, number, patch_replace};
use crate::resolve::Symbols;
use crate::scope::Scope;
use crate::walk::{self, List, Visit};

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
        if known.side == Side::EnemyKnown {
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
        let size = self.limits.map_size();
        self.out.emit(
            Diag::new("E0402", at)
                .arg("x", voxel.x)
                .arg("y", voxel.y)
                .arg("z", voxel.z)
                .arg("sx", size.first().copied().unwrap_or(0))
                .arg("sy", size.get(1).copied().unwrap_or(0))
                .arg("sz", size.get(2).copied().unwrap_or(0))
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

    fn area(&mut self, at: &str, area: &Area) {
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

    fn mandate(&mut self, at: &str, settings: &MandateSettings) {
        if settings.mandate.is_none() {
            self.out.emit(Diag::new("E0502", at));
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
                        patch_replace(&pointer::child(at, "timeout_ms"), &number(30_000)),
                        Applicability::HasPlaceholders,
                    ));
            }
            Some(pharmakos_proto::gp::v1::step::Kind::Hold(hold)) if hold.ms <= 0 => {
                let field = pointer::child(&pointer::child(at, "hold"), "ms");
                self.out
                    .emit(Diag::new("E0303", field.clone()).arg("found", hold.ms).fix(
                        "Hold for a while",
                        patch_replace(&field, &number(5_000)),
                        Applicability::HasPlaceholders,
                    ));
            }
            _ => {}
        }
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

    /// Whether a voxel is a place on this map at all.
    fn in_bounds(&self, voxel: &Voxel) -> bool {
        let size = self.limits.map_size();
        let within = |value: i32, extent: Option<i32>| {
            extent.is_some_and(|extent| value >= 0 && value < extent)
        };
        within(voxel.x, size.first().copied())
            && within(voxel.y, size.get(1).copied())
            && within(voxel.z, size.get(2).copied())
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
