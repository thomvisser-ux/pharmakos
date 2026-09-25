// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The safe playbook: spec section 14, as the operator's parameters to the
//! library's Safe Playbook template.
//!
//! > Leave configurations untouched and move to the safest beacon. If power is
//! > short and up to 2 at-risk beacons are within 60 s travel, raise their
//! > Quartermaster priority (nearest first; never recycle, switch mandate, or
//! > place) so they brown out last. Otherwise shadow the safest beacon, with
//! > the flee and rescue rules. It always qualifies.
//!
//! **Written once, tested twice** (decisions-log item 81): the playbook is
//! `library/safe_playbook.jsonc` instantiated through the gateway, never a
//! copy of it here. The template declares **the whole route** as its one
//! parameter (T18a's departure, item 112 (1)), because a pointer cannot name a
//! step that exists only when raised; so the operator passes
//! `[to_safety, raise b_NN, raise b_MM]`, `to_safety` first and taken from the
//! template's own value. The flee handler and the shadow tail are the
//! template's. Every raise is one interface step with one row, `set_priority`
//! to `HIGH`: nothing here can recycle, switch a mandate or place.
//!
//! PLACEHOLDER: "power is short" is read as the seat's own `headroom_kw_now`
//! below zero at the frozen snapshot, and an "at-risk" beacon as an own
//! non-core beacon with `powered = false` (spec section 14 defines neither).
//! Owner, at **S1**, with the grid.
//!
//! PLACEHOLDER: the rescue rule spec section 14 names is absent, because
//! nothing can be rescued before combat; it is the template's to gain. Owner,
//! at **S2/S5**.

use pharmakos_proto::json::Json;

use crate::candidates::distance2_xy;
use crate::easy::{SAFE_ESTIMATES, SAFE_MAX_RAISED, SAFE_PLAYBOOK, SAFE_REACH_MS};
use crate::playbook::{Declared, instantiate};
use crate::situation::Situation;
use crate::wire::{Wire, bool_of, compact, int_of, object, string, voxel_location};

/// The safe playbook this round, and what it raised.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct SafePlan {
    /// The playbook, JSONC, as the gateway instantiated it.
    pub(crate) jsonc: String,
    /// The route parameter's value, compact JSON.
    pub(crate) route: String,
    /// The beacons raised, nearest first.
    pub(crate) raised: Vec<String>,
}

/// Build the safe playbook from the template's own instantiation.
///
/// With nothing to raise it **is** that instantiation, and costs no call.
/// With something to raise it is one instantiate with the route and one FULL
/// verify; a raised playbook that did not qualify falls back to the one with
/// nothing raised, which is the gateway's own fallback and qualifies
/// (`the_gateways_fallback_is_the_safe_template_with_nothing_raised`).
///
/// `None` when the library has no Safe Playbook template: the gateway then
/// files its own fallback, which is that template with nothing raised.
pub(crate) fn safe_playbook(
    wire: &mut Wire<'_, '_>,
    situation: &Situation,
    declared: &[Declared],
) -> Option<SafePlan> {
    let template = declared
        .iter()
        .find(|held| held.template_id == SAFE_PLAYBOOK)?;
    let pointer = template.pointer_ending("/declarative/route")?.to_owned();
    let own_route = template.own_value(&pointer)?;
    let nothing_raised = SafePlan {
        jsonc: template.jsonc.clone(),
        route: compact(&own_route),
        raised: Vec::new(),
    };
    let Json::Array(own_steps) = own_route else {
        return Some(nothing_raised);
    };
    let Some(to_safety) = own_steps.first().cloned() else {
        return Some(nothing_raised);
    };

    let raised = at_risk(wire, situation);
    if raised.is_empty() {
        return Some(nothing_raised);
    }
    let mut steps = vec![to_safety];
    steps.extend(raised.iter().map(|beacon| raise(beacon)));
    let route = compact(&Json::Array(steps));
    let Ok(filled) = instantiate(wire, SAFE_PLAYBOOK, &[(pointer, route.clone())]) else {
        return Some(nothing_raised);
    };
    let verified = wire.call(
        "verify_plan",
        object(vec![
            ("playbook_jsonc", string(&filled.jsonc)),
            ("depth", string("full")),
        ]),
    );
    let qualifies = verified
        .ok()
        .and_then(|answer| {
            answer
                .get("report")
                .map(|report| bool_of(report, "qualifies"))
        })
        .unwrap_or(false);
    if !qualifies {
        return Some(nothing_raised);
    }
    Some(SafePlan {
        jsonc: filled.jsonc,
        route,
        raised,
    })
}

/// The beacons to raise: none unless power is short; else the own non-core
/// beacons that are browned out, the [`SAFE_ESTIMATES`] nearest the
/// commander in the ground plane estimated from it, those within
/// [`SAFE_REACH_MS`] kept, nearest by travel first, ties to the lowest id,
/// at most [`SAFE_MAX_RAISED`].
fn at_risk(wire: &mut Wire<'_, '_>, situation: &Situation) -> Vec<String> {
    if situation.economy.headroom_kw >= 0 {
        return Vec::new();
    }
    let Some(commander) = situation.commander else {
        return Vec::new();
    };
    let mut dark: Vec<(i64, &str)> = situation
        .own_beacons()
        .filter(|beacon| !beacon.core && !beacon.powered)
        .map(|beacon| (distance2_xy(commander, beacon.at), beacon.id.as_str()))
        .collect();
    dark.sort_unstable();
    dark.truncate(usize::try_from(SAFE_ESTIMATES).unwrap_or(usize::MAX));
    let mut reach: Vec<(i64, String)> = Vec::new();
    for (_, id) in dark {
        let params = object(vec![(
            "waypoints",
            Json::Array(vec![
                voxel_location(commander),
                object(vec![(
                    "beacon_anchor",
                    object(vec![("beacon_id", string(id))]),
                )]),
            ]),
        )]);
        let Ok(estimate) = wire.call("estimate_route", params) else {
            continue;
        };
        let ms = int_of(&estimate, "ms");
        if bool_of(&estimate, "reachable") && ms <= SAFE_REACH_MS {
            reach.push((ms, id.to_owned()));
        }
    }
    reach.sort_unstable();
    reach
        .into_iter()
        .take(SAFE_MAX_RAISED)
        .map(|(_, id)| id)
        .collect()
}

/// One raise: an interface step at a fixed own beacon, one row, priority
/// HIGH, skipped rather than retried if it fails.
fn raise(beacon: &str) -> Json {
    object(vec![
        ("label", string(&format!("raise_{beacon}"))),
        (
            "interface",
            object(vec![
                ("beacon", object(vec![("beacon_id", string(beacon))])),
                (
                    "rows",
                    Json::Array(vec![object(vec![("set_priority", string("HIGH"))])]),
                ),
            ]),
        ),
        ("on_fail", object(vec![("action", string("SKIP"))])),
    ])
}
