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
//! `[to_safety, raise b_NN, raise b_MM, deepen_core]`, `to_safety` first and
//! taken from the template's own value. The flee handler and the shadow tail
//! are the template's. Every raise is one interface step with one row,
//! `set_priority` to `HIGH`: nothing here can recycle, switch a mandate or
//! place.
//!
//! # A raise relights, and the core outranks every knob
//!
//! Since `grid` #91 (decisions-log items 127 (7) and 135 (2) (a); the
//! register's S1-22) a priority raise re-applies the brownout order: in every
//! settle the sim's `swap_in` relights a dark beacon at once when shedding the
//! lit beacons that rank below it covers its load, with draw no higher than
//! supply after the swap, and in that rank the **core outranks every knob**,
//! so a dark core may shed a lit HIGH expansion. A raise is therefore not
//! only an order for the next revival and the next shed: it moves where the
//! raised beacon stands in the one order the grid keeps. The safe playbook
//! never raises the core, whose place in that order no knob changes.
//!
//! # "Power is short" and "at risk" (S1's plan, decision 14; the register's S1-21)
//!
//! Spec section 14 defines neither, and decision 14 (ruled by item 128) reads
//! them, now that a raise relights:
//!
//! * **power is short** when the seat's draw is above its supply. On the wire
//!   that shows two ways: `draw_kw_now` above `supply_kw_now`, which a settle
//!   leaves only when no shed can relieve the deficit (S1-33), and an own
//!   beacon dark, whose load is draw the brownout took off the grid because
//!   the supply did not carry it.
//! * a beacon is **at risk** when it is lit, not the core and not HIGH
//!   already, and it is **next in the shed order**: the brownout's own order
//!   over the seat's lit beacons -- lowest priority first, then furthest from
//!   the core, then the lowest id (`crates/sim/src/power.rs`'s `shed_key`) --
//!   whose first [`SAFE_ESTIMATES`] are estimated, those within
//!   [`SAFE_REACH_MS`] kept, and the first [`SAFE_MAX_RAISED`] of them
//!   raised, walked to nearest first. A raised beacon goes to the back of the
//!   order, which is "so they brown out last". What the operator cannot see
//!   is which beacons the order would skip because their shed relieves
//!   nothing (S1-33: nothing homed to them); one of those may be raised
//!   needlessly, which costs a visit and nothing else.
//!
//! # The core's dig depth
//!
//! The route ends at the core with the step that gives its Mine settings
//! Easy's dig depth ([`crate::compose::deepen_core`]; decisions-log item 133
//! (3) (a)), so a seat nobody edits, which is filed this playbook, keeps
//! earning from its starting seam below the top layer. It is the one
//! configuration the safe playbook writes: item 133 outranks spec section
//! 14's "leave configurations untouched" for it (docs/design/README.md's
//! precedence), and it still never recycles, switches a mandate or places.
//! The gateway's own fallback, for a seat no operator advises, is the
//! template with nothing raised and writes nothing.
//!
//! PLACEHOLDER: the rescue rule spec section 14 names is absent, because nothing can be rescued before combat; it is the template's to gain — owner, S2/S5

use std::fmt::Write as _;

use pharmakos_proto::json::Json;

use crate::candidates::distance2;
use crate::compose::deepen_core;
use crate::easy::{SAFE_ESTIMATES, SAFE_MAX_RAISED, SAFE_PLAYBOOK, SAFE_REACH_MS};
use crate::playbook::{Declared, instantiate};
use crate::situation::{Beacon, Priority, Situation};
use crate::wire::{Wire, bool_of, compact, int_of, object, string, voxel_location};

/// The safe playbook this round, what it raised, and whether it deepens the
/// core.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct SafePlan {
    /// The playbook, JSONC, as the gateway instantiated it.
    pub(crate) jsonc: String,
    /// The route parameter's value, compact JSON.
    pub(crate) route: String,
    /// True when `route` is the template's own value: nothing raised and no
    /// depth written.
    pub(crate) own: bool,
    /// The beacons raised, nearest first.
    pub(crate) raised: Vec<String>,
    /// The core whose dig depth the route writes, if it writes one.
    pub(crate) deepened: Option<String>,
}

/// Build the safe playbook from the template's own instantiation.
///
/// With no core to deepen and nothing to raise it **is** that instantiation,
/// and costs no call. Otherwise it is one instantiate with the route and one
/// FULL verify; a route that did not qualify falls back to the template's own
/// value, which is the gateway's own fallback and qualifies
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
    let as_written = SafePlan {
        jsonc: template.jsonc.clone(),
        route: compact(&own_route),
        own: true,
        raised: Vec::new(),
        deepened: None,
    };
    let Json::Array(own_steps) = own_route else {
        return Some(as_written);
    };
    let Some(to_safety) = own_steps.first().cloned() else {
        return Some(as_written);
    };

    let raised = to_raise(wire, situation);
    let core = situation.core().map(|core| core.id.clone());
    if raised.is_empty() && core.is_none() {
        return Some(as_written);
    }
    let mut steps = vec![to_safety];
    steps.extend(raised.iter().map(|beacon| raise(beacon)));
    if let Some(core) = &core {
        steps.push(deepen_core(core));
    }
    let route = compact(&Json::Array(steps));
    let Ok(filled) = instantiate(wire, SAFE_PLAYBOOK, &[(pointer, route.clone())]) else {
        return Some(as_written);
    };
    let verified = wire.call(
        "verify_plan",
        object(vec![
            ("playbook_jsonc", string(&filled.jsonc)),
            ("depth", string("full")),
        ]),
    );
    let qualifies = verified.is_ok_and(|answer| {
        answer
            .get("report")
            .is_some_and(|report| bool_of(report, "qualifies"))
    });
    if !qualifies {
        return Some(as_written);
    }
    Some(SafePlan {
        jsonc: filled.jsonc,
        route,
        own: false,
        raised,
        deepened: core,
    })
}

/// Power is short: the seat's draw is above its supply, read as the module
/// docs say -- the forecast's draw above its supply, or an own beacon dark.
pub(crate) fn power_short(situation: &Situation) -> bool {
    situation.economy.draw_kw > situation.economy.supply_kw
        || situation.own_beacons().any(|beacon| !beacon.powered)
}

/// A beacon the safe playbook may raise: own, lit, not the core, and below
/// HIGH (the module docs). The core is never raised, and a beacon already
/// HIGH has nothing left to gain from one.
pub(crate) fn at_risk(beacon: &Beacon) -> bool {
    beacon.own
        && !beacon.core
        && beacon.powered
        && matches!(beacon.priority, Some(Priority::Low | Priority::Normal))
}

/// Where a lit beacon stands in the brownout's order, first-shed first:
/// lowest priority, then furthest from the core, then the lowest id
/// (`crates/sim/src/power.rs`'s `shed_key`, read from what the wire shows).
/// A distance that does not fit, or a seat whose core is not shown, sorts as
/// the furthest, so a beacon the operator cannot place in the order is read
/// as the first at risk rather than the last.
fn shed_rank(
    beacon: &Beacon,
    core: Option<[i32; 3]>,
) -> (Option<Priority>, std::cmp::Reverse<(bool, i64)>, &str) {
    let distance = match core.and_then(|core| distance2(beacon.at, core)) {
        Some(d2) => (false, d2),
        None => (true, 0),
    };
    (beacon.priority, std::cmp::Reverse(distance), &beacon.id)
}

/// The beacons to raise: none unless power is short; else the beacons at
/// risk in the shed order, the first [`SAFE_ESTIMATES`] estimated from the
/// commander, those within [`SAFE_REACH_MS`] kept, the first
/// [`SAFE_MAX_RAISED`] of them in the shed order taken, and those walked to
/// nearest by travel first, ties to the lowest id.
fn to_raise(wire: &mut Wire<'_, '_>, situation: &Situation) -> Vec<String> {
    if !power_short(situation) {
        return Vec::new();
    }
    let Some(commander) = situation.commander else {
        return Vec::new();
    };
    let core = situation.core().map(|core| core.at);
    let mut next: Vec<&Beacon> = situation
        .own_beacons()
        .filter(|beacon| at_risk(beacon))
        .collect();
    next.sort_by(|a, b| shed_rank(a, core).cmp(&shed_rank(b, core)));
    next.truncate(usize::try_from(SAFE_ESTIMATES).unwrap_or(usize::MAX));
    let mut within: Vec<(i64, String)> = Vec::new();
    for beacon in next {
        let params = object(vec![(
            "waypoints",
            Json::Array(vec![
                voxel_location(commander),
                object(vec![(
                    "beacon_anchor",
                    object(vec![("beacon_id", string(&beacon.id))]),
                )]),
            ]),
        )]);
        let Ok(estimate) = wire.call("estimate_route", params) else {
            continue;
        };
        let ms = int_of(&estimate, "ms");
        if bool_of(&estimate, "reachable") && ms <= SAFE_REACH_MS {
            within.push((ms, beacon.id.clone()));
        }
    }
    within.truncate(SAFE_MAX_RAISED);
    within.sort_unstable();
    within.into_iter().map(|(_, id)| id).collect()
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

/// "a and b", "a, b and c": a list for a sentence.
fn listed(items: &[String]) -> String {
    match items.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

/// "is" or "are", for a list of `count`.
const fn is_are(count: usize) -> &'static str {
    if count == 1 { "is" } else { "are" }
}

/// Why the safe playbook raises what it raises, and what it writes.
///
/// It follows the reading of "short" and "at risk" in the module docs: it
/// never says power is not short while an own beacon is dark or the draw is
/// above the supply, and it names the core when the core is dark. When the
/// view did not complete within the operator's page bound the commander was
/// not read, so nothing was estimated; the sentence says so rather than
/// degrading quietly.
pub(crate) fn why_safe(situation: &Situation, plan: Option<&SafePlan>) -> String {
    let raised: &[String] = plan.map_or(&[], |plan| plan.raised.as_slice());
    let mut why = if !power_short(situation) {
        String::from(
            "Power is not short, so nothing is raised: move to the safest beacon and stay with \
             it.",
        )
    } else if !raised.is_empty() {
        format!(
            "Power is short {}: raise {} to HIGH, nearest first, so they are the last of your lit \
             beacons to brown out.",
            shortage(situation),
            listed(raised)
        )
    } else {
        format!(
            "Power is short {}, and {}, so nothing is raised: move to the safest beacon and stay \
             with it.",
            shortage(situation),
            nothing_raised(situation)
        )
    };
    if let Some(core) = plan.and_then(|plan| plan.deepened.as_deref()) {
        let _ = write!(
            why,
            " Then set the core {core} to dig {} voxels deep, so its starting drone keeps \
             working its seam below the top layer.",
            crate::easy::CORE_DIG_MAX_DEPTH
        );
    }
    if !situation.view_complete {
        let _ = write!(
            why,
            " (The view did not complete within {} pages, so the commander was not read and \
             nothing was estimated.)",
            crate::easy::PAGES_MAX
        );
    }
    why
}

/// Why power is short, as a "because" clause: the own beacons that are
/// browned out, the core named as such, and the draw above the supply when
/// it is.
fn shortage(situation: &Situation) -> String {
    let mut dark: Vec<String> = Vec::new();
    for beacon in situation.own_beacons().filter(|beacon| !beacon.powered) {
        if beacon.core {
            dark.insert(0, format!("the core {}", beacon.id));
        } else {
            dark.push(beacon.id.clone());
        }
    }
    let economy = situation.economy;
    let over = economy.draw_kw > economy.supply_kw;
    let drawn = format!(
        "the draw of {} kW is above the supply of {} kW",
        economy.draw_kw, economy.supply_kw
    );
    match (dark.is_empty(), over) {
        (false, false) => format!(
            "because {} {} browned out",
            listed(&dark),
            is_are(dark.len())
        ),
        (false, true) => format!(
            "because {} {} browned out and {drawn}",
            listed(&dark),
            is_are(dark.len())
        ),
        _ => format!("because {drawn}"),
    }
}

/// Why nothing is raised although power is short.
fn nothing_raised(situation: &Situation) -> String {
    if situation.own_beacons().any(at_risk) {
        if situation.commander.is_none() {
            return String::from("your commander is not in view, so no beacon was estimated");
        }
        return format!(
            "no lit beacon next in the brownout order is within {} s",
            crate::compose::seconds(SAFE_REACH_MS)
        );
    }
    let lit: Vec<&Beacon> = situation
        .own_beacons()
        .filter(|beacon| !beacon.core && beacon.powered)
        .collect();
    if lit.is_empty() {
        return String::from(
            "no beacon of yours but the core is lit, and the safe playbook never raises the core",
        );
    }
    let high: Vec<String> = lit
        .iter()
        .filter(|beacon| beacon.priority == Some(Priority::High))
        .map(|beacon| beacon.id.clone())
        .collect();
    if high.len() == lit.len() {
        return format!("{} {} HIGH already", listed(&high), is_are(high.len()));
    }
    String::from("no lit beacon of yours has a priority the operator reads")
}
