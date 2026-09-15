// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The playbook size meter (decisions-log item 94).
//!
//! Spec section 10 replaces the separate counts with **one** size budget:
//! "Steps, rules and their bodies, build targets and protected areas all count
//! toward it." Item 94 fixes what a unit is, so that the editor's meter and the
//! verifier count the same thing:
//!
//! > one unit per route step, per handler, per step in a handler body, per build
//! > target, per protected area and per patrol waypoint.
//!
//! **Conditions and settings count nothing**, because their cost is bounded by
//! the node they hang off — a condition is capped at 24 nodes and four levels by
//! spec section 10, and a mandate's settings are a fixed row shape.
//!
//! The worked example `examples/playbooks/expand_east.jsonc` is **6 units**:
//! three route steps, one handler, two steps in its body. That number is item
//! 94's own worked example and `expand_east_is_six_units` pins it.
//!
//! The budget itself is a rules-table row and never a constant here — see
//! [`crate::limits::Limits::size_budget_units`].

use pharmakos_proto::gp::v1::{
    BuildSettings, Fallback, InitialSettings, InterfaceRow, MandateSettings, Playbook, Step,
    fallback, interface_row, mandate_settings, step,
};

/// How many size units a playbook occupies.
///
/// Saturating rather than wrapping: overflow checks are on in every profile, and
/// a count that panics inside the verifier would turn a large playbook into a
/// crash instead of a diagnostic.
#[must_use]
pub fn size_units(playbook: &Playbook) -> u32 {
    let mut units: u32 = 0;
    if let Some(declarative) = playbook.declarative.as_ref() {
        for entry in &declarative.route {
            units = units.saturating_add(step_units(entry));
        }
        for handler in &declarative.handlers {
            // The handler itself, then each step of its body.
            units = units.saturating_add(1);
            for entry in &handler.body {
                units = units.saturating_add(step_units(entry));
            }
        }
    }
    if let Some(Fallback {
        posture: Some(fallback::Posture::Patrol(patrol)),
    }) = playbook.fallback.as_ref()
    {
        units = units.saturating_add(count(patrol.waypoints.len()));
    }
    units
}

/// One step, plus whatever it carries that counts.
fn step_units(entry: &Step) -> u32 {
    let mut units: u32 = 1;
    match entry.kind.as_ref() {
        Some(step::Kind::Interface(interface)) => {
            for row in &interface.rows {
                units = units.saturating_add(row_units(row));
            }
        }
        Some(step::Kind::PlaceBeacon(place)) => {
            if let Some(initial) = place.initial.as_ref() {
                units = units.saturating_add(initial_units(initial));
            }
        }
        _ => {}
    }
    units
}

/// One interface row's share: a build target added is a build target.
fn row_units(row: &InterfaceRow) -> u32 {
    match row.row.as_ref() {
        Some(
            interface_row::Row::SetMandate(settings)
            | interface_row::Row::SetMandateSettings(settings),
        ) => mandate_units(settings),
        Some(interface_row::Row::AddBuildTarget(add)) => u32::from(add.target.is_some()),
        _ => 0,
    }
}

/// A `place_beacon`'s initial settings are interface rows by another name.
fn initial_units(initial: &InitialSettings) -> u32 {
    initial.mandate.as_ref().map_or(0, mandate_units)
}

/// Build targets and protected areas inside one mandate's settings.
fn mandate_units(settings: &MandateSettings) -> u32 {
    match settings.mandate.as_ref() {
        Some(mandate_settings::Mandate::Build(build)) => build_units(build),
        _ => 0,
    }
}

fn build_units(build: &BuildSettings) -> u32 {
    count(build.targets.len()).saturating_add(count(build.protected_areas.len()))
}

/// A list length as a unit count.
fn count(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}
