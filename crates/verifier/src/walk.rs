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
//! something a call site reassembles.
//!
//! The order is document order — route before handlers, `on_death` before
//! `fallback`, a list in its own order — which is the order diagnostics come out
//! in, and therefore part of what `report_hash` covers.

use pharmakos_proto::gp::v1::{
    Area, BeaconFilter, BeaconRef, BuildTarget, Condition, InterfaceStep, Location,
    MandateSettings, Playbook, Step, beacon_ref, broadcast_step, condition, fallback,
    interface_row, location, mandate_settings, step,
};

use crate::pointer;

/// Which list a step belongs to: the route, or one handler's body.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum List {
    /// The route itself.
    Route,
    /// The body of the handler at this index.
    Body(usize),
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
    /// Any location, `place_beacon`'s site included.
    fn location(&mut self, _at: &str, _location: &Location) {}
    /// Any beacon reference, fixed or late-bound.
    fn beacon(&mut self, _at: &str, _reference: &BeaconRef) {}
    /// Any selector filter.
    fn filter(&mut self, _at: &str, _filter: &BeaconFilter) {}
    /// Any voxel.
    fn voxel(&mut self, _at: &str, _voxel: &pharmakos_proto::gp::v1::Voxel) {}
    /// Any area.
    fn area(&mut self, _at: &str, _area: &Area) {}
    /// Any condition node, all the way down.
    fn condition(&mut self, _at: &str, _condition: &Condition) {}
    /// Any mandate settings block.
    fn mandate(&mut self, _at: &str, _settings: &MandateSettings) {}
    /// Any build target.
    fn build_target(&mut self, _at: &str, _target: &BuildTarget) {}
}

/// Walk the whole playbook in document order.
pub(crate) fn walk(playbook: &Playbook, visitor: &mut dyn Visit) {
    if let Some(declarative) = playbook.declarative.as_ref() {
        for (index, entry) in declarative.route.iter().enumerate() {
            let at = pointer::at("/declarative/route", index);
            visitor.entry(&at, entry, List::Route, index);
            walk_entry(entry, &at, visitor);
        }
        for (handler_index, handler) in declarative.handlers.iter().enumerate() {
            let handler_at = pointer::at("/declarative/handlers", handler_index);
            if let Some(when) = handler.when.as_ref() {
                walk_condition(when, &pointer::child(&handler_at, "when"), visitor);
            }
            let body_at = pointer::child(&handler_at, "body");
            for (index, entry) in handler.body.iter().enumerate() {
                let at = pointer::at(&body_at, index);
                visitor.entry(&at, entry, List::Body(handler_index), index);
                walk_entry(entry, &at, visitor);
            }
        }
    }
    if let Some(tail) = playbook.fallback.as_ref() {
        match tail.posture.as_ref() {
            Some(fallback::Posture::Hold(hold)) => {
                let at = pointer::child("/fallback", "hold");
                if let Some(place) = hold.at.as_ref() {
                    walk_location(place, &pointer::child(&at, "at"), visitor);
                }
            }
            Some(fallback::Posture::Shadow(shadow)) => {
                let at = pointer::child("/fallback", "shadow");
                if let Some(beacon) = shadow.beacon.as_ref() {
                    walk_beacon(beacon, &pointer::child(&at, "beacon"), visitor);
                }
            }
            Some(fallback::Posture::Patrol(patrol)) => {
                let at = pointer::child(&pointer::child("/fallback", "patrol"), "waypoints");
                for (index, waypoint) in patrol.waypoints.iter().enumerate() {
                    walk_location(waypoint, &pointer::at(&at, index), visitor);
                }
            }
            None => {}
        }
    }
}

fn walk_entry(entry: &Step, at: &str, visitor: &mut dyn Visit) {
    if let Some(guard) = entry.skip_if.as_ref() {
        walk_condition(guard, &pointer::child(at, "skip_if"), visitor);
    }
    match entry.kind.as_ref() {
        Some(step::Kind::Move(walk_to)) => {
            let here = pointer::child(at, "move");
            if let Some(place) = walk_to.to.as_ref() {
                walk_location(place, &pointer::child(&here, "to"), visitor);
            }
        }
        Some(step::Kind::Interface(interface)) => {
            let here = pointer::child(at, "interface");
            visitor.interface(&here, interface);
            if let Some(beacon) = interface.beacon.as_ref() {
                walk_beacon(beacon, &pointer::child(&here, "beacon"), visitor);
            }
            let rows_at = pointer::child(&here, "rows");
            for (index, row) in interface.rows.iter().enumerate() {
                walk_row(row.row.as_ref(), &pointer::at(&rows_at, index), visitor);
            }
        }
        Some(step::Kind::PlaceBeacon(place)) => {
            let here = pointer::child(at, "place_beacon");
            if let Some(site) = place.at.as_ref() {
                let site_at = pointer::child(&here, "at");
                visitor.place_site(&site_at, site);
                walk_location(site, &site_at, visitor);
            }
            if let Some(initial) = place.initial.as_ref() {
                if let Some(settings) = initial.mandate.as_ref() {
                    walk_mandate(
                        settings,
                        &pointer::child(&pointer::child(&here, "initial"), "mandate"),
                        visitor,
                    );
                }
            }
        }
        Some(step::Kind::WaitUntil(wait)) => {
            let here = pointer::child(at, "wait_until");
            if let Some(condition) = wait.condition.as_ref() {
                walk_condition(condition, &pointer::child(&here, "condition"), visitor);
            }
        }
        Some(step::Kind::Broadcast(broadcast)) => {
            let here = pointer::child(at, "broadcast");
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
                walk_location(
                    location,
                    &pointer::child(&pointer::child(&here, name), "at"),
                    visitor,
                );
            }
        }
        Some(step::Kind::Hold(_)) | None => {}
    }
}

fn walk_row(row: Option<&interface_row::Row>, at: &str, visitor: &mut dyn Visit) {
    match row {
        Some(interface_row::Row::SetMandate(settings)) => {
            walk_mandate(settings, &pointer::child(at, "set_mandate"), visitor);
        }
        Some(interface_row::Row::SetMandateSettings(settings)) => {
            walk_mandate(
                settings,
                &pointer::child(at, "set_mandate_settings"),
                visitor,
            );
        }
        Some(interface_row::Row::AddBuildTarget(add)) => {
            if let Some(target) = add.target.as_ref() {
                walk_build_target(
                    target,
                    &pointer::child(&pointer::child(at, "add_build_target"), "target"),
                    visitor,
                );
            }
        }
        Some(interface_row::Row::RemoveBuildTarget(remove)) => {
            if let Some(anchor) = remove.anchor.as_ref() {
                walk_location(
                    anchor,
                    &pointer::child(&pointer::child(at, "remove_build_target"), "anchor"),
                    visitor,
                );
            }
        }
        _ => {}
    }
}

fn walk_mandate(settings: &MandateSettings, at: &str, visitor: &mut dyn Visit) {
    visitor.mandate(at, settings);
    match settings.mandate.as_ref() {
        Some(mandate_settings::Mandate::Build(build)) => {
            let here = pointer::child(at, "build");
            let targets_at = pointer::child(&here, "targets");
            for (index, target) in build.targets.iter().enumerate() {
                walk_build_target(target, &pointer::at(&targets_at, index), visitor);
            }
            let areas_at = pointer::child(&here, "protected_areas");
            for (index, area) in build.protected_areas.iter().enumerate() {
                walk_area(area, &pointer::at(&areas_at, index), visitor);
            }
        }
        Some(mandate_settings::Mandate::Survey(survey)) => {
            let areas_at = pointer::child(&pointer::child(at, "survey"), "probe_areas");
            for (index, area) in survey.probe_areas.iter().enumerate() {
                walk_area(area, &pointer::at(&areas_at, index), visitor);
            }
        }
        _ => {}
    }
}

fn walk_build_target(target: &BuildTarget, at: &str, visitor: &mut dyn Visit) {
    visitor.build_target(at, target);
    if let Some(anchor) = target.anchor.as_ref() {
        walk_location(anchor, &pointer::child(at, "anchor"), visitor);
    }
}

fn walk_area(area: &Area, at: &str, visitor: &mut dyn Visit) {
    visitor.area(at, area);
    if let Some(corner) = area.min.as_ref() {
        visitor.voxel(&pointer::child(at, "min"), corner);
    }
    if let Some(corner) = area.max.as_ref() {
        visitor.voxel(&pointer::child(at, "max"), corner);
    }
}

fn walk_location(place: &Location, at: &str, visitor: &mut dyn Visit) {
    visitor.location(at, place);
    match place.place.as_ref() {
        Some(location::Place::Voxel(voxel)) => visitor.voxel(&pointer::child(at, "voxel"), voxel),
        Some(location::Place::BeaconAnchor(beacon)) => {
            walk_beacon(beacon, &pointer::child(at, "beacon_anchor"), visitor);
        }
        Some(location::Place::Safest(_)) | None => {}
    }
}

fn walk_beacon(reference: &BeaconRef, at: &str, visitor: &mut dyn Visit) {
    visitor.beacon(at, reference);
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
        visitor.filter(&pointer::child(&pointer::child(at, name), "filter"), found);
    }
}

fn walk_condition(node: &Condition, at: &str, visitor: &mut dyn Visit) {
    visitor.condition(at, node);
    match node.node.as_ref() {
        Some(condition::Node::All(all)) => {
            let items_at = pointer::child(&pointer::child(at, "all"), "items");
            for (index, item) in all.items.iter().enumerate() {
                walk_condition(item, &pointer::at(&items_at, index), visitor);
            }
        }
        Some(condition::Node::Any(any)) => {
            let items_at = pointer::child(&pointer::child(at, "any"), "items");
            for (index, item) in any.items.iter().enumerate() {
                walk_condition(item, &pointer::at(&items_at, index), visitor);
            }
        }
        Some(condition::Node::Not(not)) => {
            if let Some(item) = not.item.as_ref() {
                walk_condition(
                    item,
                    &pointer::child(&pointer::child(at, "not"), "item"),
                    visitor,
                );
            }
        }
        Some(condition::Node::BeaconHpPct(predicate)) => {
            walk_predicate_beacon(
                predicate.beacon.as_ref(),
                &pointer::child(at, "beacon_hp_pct"),
                visitor,
            );
        }
        Some(condition::Node::BeaconUnderAttack(predicate)) => {
            walk_predicate_beacon(
                predicate.beacon.as_ref(),
                &pointer::child(at, "beacon_under_attack"),
                visitor,
            );
        }
        Some(condition::Node::BeaconPowered(predicate)) => {
            walk_predicate_beacon(
                predicate.beacon.as_ref(),
                &pointer::child(at, "beacon_powered"),
                visitor,
            );
        }
        _ => {}
    }
}

fn walk_predicate_beacon(beacon: Option<&BeaconRef>, at: &str, visitor: &mut dyn Visit) {
    if let Some(reference) = beacon {
        walk_beacon(reference, &pointer::child(at, "beacon"), visitor);
    }
}
