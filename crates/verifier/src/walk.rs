// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! One walk of a playbook, shared by the stages that need the same leaves.
//!
//! The resolve stage wants every beacon reference and every condition; the
//! semantics stage wants every voxel, every area, every mandate and every build
//! target. Written twice they would drift, and a leaf one walk reaches and the
//! other does not is a check that silently stops running. So the walk is written
//! once, here, and a stage implements [`Visit`] for the leaves it cares about.
//!
//! Every callback is handed the **JSON Pointer to the node**, built as the walk
//! descends, so a diagnostic's path is a fact of the traversal rather than
//! something a call site reassembles. It is built in place ([`Here`]): one
//! buffer for the whole walk, a token pushed on the way down and popped on the
//! way back, so a callback that raises nothing costs no allocation, and one
//! that raises a diagnostic copies the pointer then (S3's `vfyq`).
//!
//! The order is document order — route before handlers, `on_death` before
//! `fallback`, a list in its own order — which is the order diagnostics come out
//! in, and therefore part of what `report_hash` covers.

use pharmakos_proto::gp::v1::{
    Area, AttackSettings, BeaconFilter, BeaconRef, BuildTarget, Condition, DefendSettings,
    FeatureRef, InterfaceStep, Location, MandateSettings, Playbook, Step, attack_target,
    beacon_ref, broadcast_step, condition, fallback, interface_row, location, mandate_settings,
    protected_asset, step,
};

use crate::pointer::{self, Here};

/// Which list a step belongs to: the route, or one handler's body.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum List {
    /// The route itself.
    Route,
    /// The body of the handler at this index.
    Body(usize),
}

/// Where a `Location` stands, which is what decides which of targeting's arms
/// it may hold (docs/design/targeting.md, "Sites").
///
/// The walk knows this and a visitor handed only the `Location` would not: a
/// Build target's anchor and a move's destination are the same message.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Slot<'a> {
    /// `PlaceBeaconStep.at`, the one place `covering` is legal.
    PlaceSite,
    /// `BuildTarget.anchor`, the one place `on` is legal as a description:
    /// in `add_build_target`, a Build mandate's settings, or a `place_beacon`'s
    /// `initial`.
    BuildAnchor {
        /// The `covering` reference of the `place_beacon` whose `initial`
        /// holds the target, when there is one: the only context in which
        /// `covered {}` means anything.
        deploy: Option<&'a FeatureRef>,
    },
    /// `RemoveBuildTargetRow.anchor`, where `on` is legal as a name only.
    RemoveAnchor,
    /// Anywhere else: a move's destination, a broadcast's place, the
    /// fallback's hold or patrol. Neither arm is legal here.
    Elsewhere,
}

/// Which beacon's mandate holds a Build target, as the file names it: what a
/// wall box's sphere is measured from (S2's `vfy2`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Holder<'a> {
    /// The beacon a `place_beacon` puts up at this voxel, whose `initial`
    /// holds the target.
    Site(&'a pharmakos_proto::gp::v1::Voxel),
    /// The beacon an `interface` step visits.
    Visited(&'a BeaconRef),
    /// A beacon the file does not name: a deploy `covering` a feature, whose
    /// site the sim chooses, or a step that names no place.
    Unnamed,
}

/// Which field holds an [`Area`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum AreaOf {
    /// A Build mandate's `protected_areas`.
    Protected,
    /// A Survey mandate's `probe_areas`.
    Probe,
    /// An Attack mandate's `avoid_areas`.
    Avoid,
    /// A `wall` target's `wall_box`, whose own checks are the structure
    /// stage's `Sites::wall`.
    WallBox,
}

impl AreaOf {
    /// The field's JSON name.
    pub(crate) const fn field(self) -> &'static str {
        match self {
            AreaOf::Protected => "protected_areas",
            AreaOf::Probe => "probe_areas",
            AreaOf::Avoid => "avoid_areas",
            AreaOf::WallBox => "wall_box",
        }
    }
}

/// What carries a mandate settings block, which decides how an omitted field in
/// it reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Carrier {
    /// A `place_beacon`'s `initial.mandate`: the beacon starts from nothing, so
    /// an omitted field takes its default.
    Initial,
    /// A `set_mandate` row: the switch clears the beacon's settings, so an
    /// omitted field takes its default there too.
    Switch,
    /// A `set_mandate_settings` row, an edit "within the current mandate"
    /// (`playbook.proto`): an omitted field is one the row does not write, and
    /// the sim prices the row by the fields it does write.
    Edit,
}

/// The leaves a stage can ask to be shown.
///
/// Every method has a default that does nothing, so a stage writes only what it
/// checks.
pub(crate) trait Visit {
    /// One step, with the list it belongs to and its index in that list.
    fn entry(&mut self, _at: &str, _entry: &Step, _list: List, _index: usize) {}
    /// One `interface` step, whole, so a row can be judged against its beacon.
    fn interface(&mut self, _at: &str, _interface: &InterfaceStep) {}
    /// One `place_beacon` site, as a location.
    fn place_site(&mut self, _at: &str, _site: &Location) {}
    /// Any location, `place_beacon`'s site included, with the slot it stands
    /// in.
    fn location(&mut self, _at: &str, _location: &Location, _slot: Slot<'_>) {}
    /// Any beacon reference, fixed or late-bound.
    fn beacon(&mut self, _at: &str, _reference: &BeaconRef) {}
    /// Any selector filter.
    fn filter(&mut self, _at: &str, _filter: &BeaconFilter) {}
    /// Any voxel.
    fn voxel(&mut self, _at: &str, _voxel: &pharmakos_proto::gp::v1::Voxel) {}
    /// Any area, with the field that holds it.
    fn area(&mut self, _at: &str, _area: &Area, _of: AreaOf) {}
    /// Any condition node, all the way down.
    fn condition(&mut self, _at: &str, _condition: &Condition) {}
    /// Any mandate settings block, with the row or step that carries it.
    fn mandate(&mut self, _at: &str, _settings: &MandateSettings, _carrier: Carrier) {}
    /// Any build target.
    fn build_target(&mut self, _at: &str, _target: &BuildTarget) {}
    /// Any build target, with the beacon whose mandate holds it.
    fn held_target(&mut self, _at: &str, _target: &BuildTarget, _holder: Holder<'_>) {}
}

/// Walk the whole playbook in document order.
pub(crate) fn walk(playbook: &Playbook, visitor: &mut dyn Visit) {
    let mut here = Here::new(pointer::ROOT);
    if let Some(declarative) = playbook.declarative.as_ref() {
        let declarative_at = here.push("declarative");
        let route_at = here.push("route");
        for (index, entry) in declarative.route.iter().enumerate() {
            let at = here.push_index(index);
            visitor.entry(here.as_str(), entry, List::Route, index);
            walk_entry(entry, &mut here, visitor);
            here.pop(at);
        }
        here.pop(route_at);
        let handler_list = here.push("handlers");
        for (handler_index, handler) in declarative.handlers.iter().enumerate() {
            let handler_at = here.push_index(handler_index);
            if let Some(when) = handler.when.as_ref() {
                let when_at = here.push("when");
                walk_condition(when, &mut here, visitor);
                here.pop(when_at);
            }
            let body_at = here.push("body");
            for (index, entry) in handler.body.iter().enumerate() {
                let at = here.push_index(index);
                visitor.entry(here.as_str(), entry, List::Body(handler_index), index);
                walk_entry(entry, &mut here, visitor);
                here.pop(at);
            }
            here.pop(body_at);
            here.pop(handler_at);
        }
        here.pop(handler_list);
        here.pop(declarative_at);
    }
    if let Some(tail) = playbook.fallback.as_ref() {
        let fallback_at = here.push("fallback");
        match tail.posture.as_ref() {
            Some(fallback::Posture::Hold(hold)) => {
                let hold_at = here.push("hold");
                if let Some(place) = hold.at.as_ref() {
                    let at = here.push("at");
                    walk_location(place, &mut here, Slot::Elsewhere, visitor);
                    here.pop(at);
                }
                here.pop(hold_at);
            }
            Some(fallback::Posture::Shadow(shadow)) => {
                let shadow_at = here.push("shadow");
                if let Some(beacon) = shadow.beacon.as_ref() {
                    let at = here.push("beacon");
                    walk_beacon(beacon, &mut here, visitor);
                    here.pop(at);
                }
                here.pop(shadow_at);
            }
            Some(fallback::Posture::Patrol(patrol)) => {
                let waypoint_list = here.push_path(&["patrol", "waypoints"]);
                for (index, waypoint) in patrol.waypoints.iter().enumerate() {
                    let at = here.push_index(index);
                    walk_location(waypoint, &mut here, Slot::Elsewhere, visitor);
                    here.pop(at);
                }
                here.pop(waypoint_list);
            }
            None => {}
        }
        here.pop(fallback_at);
    }
}

/// One step, with `here` at the step. Every walk below returns with `here`
/// where it found it.
fn walk_entry(entry: &Step, here: &mut Here, visitor: &mut dyn Visit) {
    if let Some(guard) = entry.skip_if.as_ref() {
        let at = here.push("skip_if");
        walk_condition(guard, here, visitor);
        here.pop(at);
    }
    match entry.kind.as_ref() {
        Some(step::Kind::Move(walk_to)) => {
            if let Some(place) = walk_to.to.as_ref() {
                let at = here.push_path(&["move", "to"]);
                walk_location(place, here, Slot::Elsewhere, visitor);
                here.pop(at);
            }
        }
        Some(step::Kind::Interface(interface)) => {
            let at = here.push("interface");
            visitor.interface(here.as_str(), interface);
            if let Some(beacon) = interface.beacon.as_ref() {
                let beacon_at = here.push("beacon");
                walk_beacon(beacon, here, visitor);
                here.pop(beacon_at);
            }
            let row_list = here.push("rows");
            for (index, row) in interface.rows.iter().enumerate() {
                let holder = match interface.beacon.as_ref() {
                    Some(beacon) => Holder::Visited(beacon),
                    None => Holder::Unnamed,
                };
                let row_at = here.push_index(index);
                walk_row(row.row.as_ref(), here, holder, visitor);
                here.pop(row_at);
            }
            here.pop(row_list);
            here.pop(at);
        }
        Some(step::Kind::PlaceBeacon(place)) => {
            let at = here.push("place_beacon");
            if let Some(site) = place.at.as_ref() {
                let site_at = here.push("at");
                visitor.place_site(here.as_str(), site);
                walk_location(site, here, Slot::PlaceSite, visitor);
                here.pop(site_at);
            }
            let deploy = match place.at.as_ref().and_then(|site| site.place.as_ref()) {
                Some(location::Place::Covering(feature)) => Some(feature),
                _ => None,
            };
            let holder = match place.at.as_ref().and_then(|site| site.place.as_ref()) {
                Some(location::Place::Voxel(voxel)) => Holder::Site(voxel),
                _ => Holder::Unnamed,
            };
            if let Some(initial) = place.initial.as_ref()
                && let Some(settings) = initial.mandate.as_ref()
            {
                let initial_at = here.push_path(&["initial", "mandate"]);
                walk_mandate(settings, here, Carrier::Initial, deploy, holder, visitor);
                here.pop(initial_at);
            }
            here.pop(at);
        }
        Some(step::Kind::WaitUntil(wait)) => {
            if let Some(condition) = wait.condition.as_ref() {
                let at = here.push_path(&["wait_until", "condition"]);
                walk_condition(condition, here, visitor);
                here.pop(at);
            }
        }
        Some(step::Kind::Broadcast(broadcast)) => {
            let place = match broadcast.payload.as_ref() {
                Some(broadcast_step::Payload::MarkTarget(mark)) => {
                    mark.at.as_ref().map(|place| ("mark_target", place))
                }
                Some(broadcast_step::Payload::ThreatAt(threat)) => {
                    threat.at.as_ref().map(|place| ("threat_at", place))
                }
                Some(broadcast_step::Payload::Rally(rally)) => {
                    rally.at.as_ref().map(|place| ("rally", place))
                }
                Some(broadcast_step::Payload::GoCode(_)) | None => None,
            };
            if let Some((name, location)) = place {
                let at = here.push_path(&["broadcast", name, "at"]);
                walk_location(location, here, Slot::Elsewhere, visitor);
                here.pop(at);
            }
        }
        Some(step::Kind::Hold(_)) | None => {}
    }
}

fn walk_row(
    row: Option<&interface_row::Row>,
    here: &mut Here,
    holder: Holder<'_>,
    visitor: &mut dyn Visit,
) {
    match row {
        Some(interface_row::Row::SetMandate(settings)) => {
            let at = here.push("set_mandate");
            walk_mandate(settings, here, Carrier::Switch, None, holder, visitor);
            here.pop(at);
        }
        Some(interface_row::Row::SetMandateSettings(settings)) => {
            let at = here.push("set_mandate_settings");
            walk_mandate(settings, here, Carrier::Edit, None, holder, visitor);
            here.pop(at);
        }
        Some(interface_row::Row::AddBuildTarget(add)) => {
            if let Some(target) = add.target.as_ref() {
                let at = here.push_path(&["add_build_target", "target"]);
                walk_build_target(target, here, None, holder, visitor);
                here.pop(at);
            }
        }
        Some(interface_row::Row::RemoveBuildTarget(remove)) => {
            if let Some(anchor) = remove.anchor.as_ref() {
                let at = here.push_path(&["remove_build_target", "anchor"]);
                walk_location(anchor, here, Slot::RemoveAnchor, visitor);
                here.pop(at);
            }
        }
        _ => {}
    }
}

fn walk_mandate(
    settings: &MandateSettings,
    here: &mut Here,
    carrier: Carrier,
    deploy: Option<&FeatureRef>,
    holder: Holder<'_>,
    visitor: &mut dyn Visit,
) {
    visitor.mandate(here.as_str(), settings, carrier);
    match settings.mandate.as_ref() {
        Some(mandate_settings::Mandate::Build(build)) => {
            let at = here.push("build");
            let target_list = here.push("targets");
            for (index, target) in build.targets.iter().enumerate() {
                let target_at = here.push_index(index);
                walk_build_target(target, here, deploy, holder, visitor);
                here.pop(target_at);
            }
            here.pop(target_list);
            let area_list = here.push("protected_areas");
            for (index, area) in build.protected_areas.iter().enumerate() {
                let area_at = here.push_index(index);
                walk_area(area, here, AreaOf::Protected, visitor);
                here.pop(area_at);
            }
            here.pop(area_list);
            here.pop(at);
        }
        Some(mandate_settings::Mandate::Survey(survey)) => {
            let at = here.push_path(&["survey", "probe_areas"]);
            for (index, area) in survey.probe_areas.iter().enumerate() {
                let area_at = here.push_index(index);
                walk_area(area, here, AreaOf::Probe, visitor);
                here.pop(area_at);
            }
            here.pop(at);
        }
        Some(mandate_settings::Mandate::Defend(defend)) => {
            let at = here.push("defend");
            walk_defend(defend, here, visitor);
            here.pop(at);
        }
        Some(mandate_settings::Mandate::Attack(attack)) => {
            let at = here.push("attack");
            walk_attack(attack, here, visitor);
            here.pop(at);
        }
        Some(mandate_settings::Mandate::Mine(_)) | None => {}
    }
}

/// S2's Defend settings (`con4`): the beacons on the protect list.
///
/// The sim refused the whole arm until S2's `dfnd` lane built it (`E0012`,
/// raised by nothing now); the leaves are walked so that every stage's checks
/// reach them, as they reach the same leaves anywhere else.
fn walk_defend(defend: &DefendSettings, here: &mut Here, visitor: &mut dyn Visit) {
    let protect_at = here.push("protect");
    for (index, entry) in defend.protect.iter().enumerate() {
        if let Some(protected_asset::Asset::Beacon(beacon)) = entry.asset.as_ref() {
            let at = here.push_index(index);
            let beacon_at = here.push("beacon");
            walk_beacon(beacon, here, visitor);
            here.pop(beacon_at);
            here.pop(at);
        }
    }
    here.pop(protect_at);
}

/// S2's Attack settings (`con4`): the target, the staging point and the avoid
/// areas. Refused as a whole until S2's `atk` lane built it (`E0013`, raised
/// by nothing now), and walked for the reason [`walk_defend`] gives.
fn walk_attack(attack: &AttackSettings, here: &mut Here, visitor: &mut dyn Visit) {
    if let Some(target) = attack.target.as_ref() {
        let target_at = here.push("target");
        match target.target.as_ref() {
            Some(attack_target::Target::Beacon(beacon)) => {
                let at = here.push("beacon");
                walk_beacon(beacon, here, visitor);
                here.pop(at);
            }
            Some(attack_target::Target::Voxel(voxel)) => {
                let at = here.push("voxel");
                visitor.voxel(here.as_str(), voxel);
                here.pop(at);
            }
            Some(attack_target::Target::EnemyCommander(_)) | None => {}
        }
        here.pop(target_at);
    }
    if let Some(staging) = attack.staging.as_ref() {
        let at = here.push("staging");
        visitor.voxel(here.as_str(), staging);
        here.pop(at);
    }
    let areas_at = here.push("avoid_areas");
    for (index, area) in attack.avoid_areas.iter().enumerate() {
        let at = here.push_index(index);
        walk_area(area, here, AreaOf::Avoid, visitor);
        here.pop(at);
    }
    here.pop(areas_at);
}

fn walk_build_target(
    target: &BuildTarget,
    here: &mut Here,
    deploy: Option<&FeatureRef>,
    holder: Holder<'_>,
    visitor: &mut dyn Visit,
) {
    visitor.build_target(here.as_str(), target);
    visitor.held_target(here.as_str(), target, holder);
    if let Some(anchor) = target.anchor.as_ref() {
        let at = here.push("anchor");
        walk_location(anchor, here, Slot::BuildAnchor { deploy }, visitor);
        here.pop(at);
    }
    // A `wall` target's box (S2's plan, task `con4`), an area like any other.
    if let Some(wall_box) = target.wall_box.as_ref() {
        let at = here.push("wall_box");
        walk_area(wall_box, here, AreaOf::WallBox, visitor);
        here.pop(at);
    }
}

fn walk_area(area: &Area, here: &mut Here, of: AreaOf, visitor: &mut dyn Visit) {
    visitor.area(here.as_str(), area, of);
    if let Some(corner) = area.min.as_ref() {
        let at = here.push("min");
        visitor.voxel(here.as_str(), corner);
        here.pop(at);
    }
    if let Some(corner) = area.max.as_ref() {
        let at = here.push("max");
        visitor.voxel(here.as_str(), corner);
        here.pop(at);
    }
}

fn walk_location(place: &Location, here: &mut Here, slot: Slot<'_>, visitor: &mut dyn Visit) {
    visitor.location(here.as_str(), place, slot);
    match place.place.as_ref() {
        Some(location::Place::Voxel(voxel)) => {
            let at = here.push("voxel");
            visitor.voxel(here.as_str(), voxel);
            here.pop(at);
        }
        Some(location::Place::BeaconAnchor(beacon)) => {
            let at = here.push("beacon_anchor");
            walk_beacon(beacon, here, visitor);
            here.pop(at);
        }
        // Targeting's sites carry no voxel and no beacon to visit: a stage
        // that checks them reads the `FeatureRef` from `location`, with the
        // slot that decides whether the arm may stand there.
        Some(
            location::Place::Safest(_)
            | location::Place::On(_)
            | location::Place::Covering(_)
            | location::Place::Toward(_),
        )
        | None => {}
    }
}

fn walk_beacon(reference: &BeaconRef, here: &mut Here, visitor: &mut dyn Visit) {
    visitor.beacon(here.as_str(), reference);
    let filter = match reference.r#ref.as_ref() {
        Some(beacon_ref::Ref::Nearest(nearest)) => {
            nearest.filter.as_ref().map(|found| ("nearest", found))
        }
        Some(beacon_ref::Ref::Weakest(weakest)) => {
            weakest.filter.as_ref().map(|found| ("weakest", found))
        }
        Some(beacon_ref::Ref::MostThreatened(most)) => {
            most.filter.as_ref().map(|found| ("most_threatened", found))
        }
        _ => None,
    };
    if let Some((name, found)) = filter {
        let at = here.push_path(&[name, "filter"]);
        visitor.filter(here.as_str(), found);
        here.pop(at);
    }
}

fn walk_condition(node: &Condition, here: &mut Here, visitor: &mut dyn Visit) {
    visitor.condition(here.as_str(), node);
    match node.node.as_ref() {
        Some(condition::Node::All(all)) => {
            let at = here.push_path(&["all", "items"]);
            walk_items(&all.items, here, visitor);
            here.pop(at);
        }
        Some(condition::Node::Any(any)) => {
            let at = here.push_path(&["any", "items"]);
            walk_items(&any.items, here, visitor);
            here.pop(at);
        }
        Some(condition::Node::Not(not)) => {
            if let Some(item) = not.item.as_ref() {
                let at = here.push_path(&["not", "item"]);
                walk_condition(item, here, visitor);
                here.pop(at);
            }
        }
        Some(condition::Node::BeaconHpPct(predicate)) => {
            walk_predicate_beacon(predicate.beacon.as_ref(), "beacon_hp_pct", here, visitor);
        }
        Some(condition::Node::BeaconUnderAttack(predicate)) => {
            walk_predicate_beacon(
                predicate.beacon.as_ref(),
                "beacon_under_attack",
                here,
                visitor,
            );
        }
        Some(condition::Node::BeaconPowered(predicate)) => {
            walk_predicate_beacon(predicate.beacon.as_ref(), "beacon_powered", here, visitor);
        }
        _ => {}
    }
}

fn walk_items(items: &[Condition], here: &mut Here, visitor: &mut dyn Visit) {
    for (index, item) in items.iter().enumerate() {
        let at = here.push_index(index);
        walk_condition(item, here, visitor);
        here.pop(at);
    }
}

fn walk_predicate_beacon(
    beacon: Option<&BeaconRef>,
    predicate: &str,
    here: &mut Here,
    visitor: &mut dyn Visit,
) {
    if let Some(reference) = beacon {
        let at = here.push_path(&[predicate, "beacon"]);
        walk_beacon(reference, here, visitor);
        here.pop(at);
    }
}
