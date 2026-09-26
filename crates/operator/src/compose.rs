// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Composing a round: greedy insertion by utility per second, and the "why".
//!
//! Spec section 14: "compose visit goals by greedy insertion by utility per
//! second until the route fills its target share of the segment (the
//! segment's length comes from the snapshot)". At Easy the share is 70 %.
//! The top-k candidates are walked in rank order and each is inserted when
//! it is the same template's goal as the first, fits what is left of the
//! share, and is affordable from the treasury as it stands. The first goal
//! chooses the template; each further one becomes one more walk-and-place of
//! it -- a Mine beacon for Expand & Mine, a Build beacon with its Generator
//! for Hold & Build -- written as a JSON Patch the gateway's `patch_plan`
//! applies to the template's own instantiation, so comments and layout
//! survive.
//!
//! No lookahead of any kind: a goal is inserted on its own estimate, and
//! nothing asks what the match would do with it (AGENTS.md section 3 rule 2).

use pharmakos_proto::json::Json;

use crate::candidates::{Candidate, Goal, NoVent, Unfit};
use crate::easy::{
    EASY_ROUTE_FILL_PERCENT, EXPAND_AND_MINE, HOLD_AND_BUILD, PAGES_MAX, SAFE_REACH_MS,
};
use crate::playbook::{Declared, op, pointer_get, with_member, with_pointer};
use crate::situation::Situation;
use crate::terrain::Feature;
use crate::wire::{compact, number, string, voxel_json, voxel_location};

/// A composed round: the template, its goals, and the patch that fills it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Composed {
    /// Which template.
    pub(crate) template_id: &'static str,
    /// The goals, in insertion order.
    pub(crate) goals: Vec<Candidate>,
    /// The JSON Patch, applied to the template's own instantiation.
    pub(crate) patch: Vec<Json>,
    /// For each goal after the first, the patch that takes it out again:
    /// the repair of last resort.
    pub(crate) removals: Vec<Vec<Json>>,
}

/// The share of the segment the route may fill, game milliseconds.
pub(crate) fn fill_ms(situation: &Situation) -> i64 {
    situation
        .segment_ms
        .max(0)
        .saturating_mul(EASY_ROUTE_FILL_PERCENT)
        .checked_div(100)
        .unwrap_or(0)
}

/// Greedy insertion over the top-k, in rank order.
fn greedy(top: &[Candidate], fill: i64, treasury: i64) -> Vec<Candidate> {
    let mut goals: Vec<Candidate> = Vec::new();
    let (mut time, mut cost) = (0_i64, 0_i64);
    for candidate in top {
        if goals
            .first()
            .is_some_and(|first| first.goal.template() != candidate.goal.template())
        {
            continue;
        }
        let (then_time, then_cost) = (
            time.saturating_add(candidate.time_ms),
            cost.saturating_add(candidate.cost_dollars),
        );
        if then_time > fill || then_cost > treasury {
            continue;
        }
        time = then_time;
        cost = then_cost;
        goals.push(candidate.clone());
    }
    goals
}

/// Compose the round from the top-k, or `None` when nothing fits.
pub(crate) fn compose(
    situation: &Situation,
    top: &[Candidate],
    declared: &[Declared],
) -> Option<Composed> {
    let fill = fill_ms(situation);
    let goals = greedy(top, fill, situation.economy.treasury);
    let first = goals.first()?;
    let template = declared
        .iter()
        .find(|held| held.template_id == first.goal.template())?;
    let (template_id, mut patch, removals) = match first.goal {
        Goal::Mine { .. } => {
            let (patch, removals) = place_patch(template, &goals, None)?;
            (EXPAND_AND_MINE, patch, removals)
        }
        Goal::Generator { .. } => {
            let (patch, removals) = place_patch(template, &goals, Some(fill))?;
            (HOLD_AND_BUILD, patch, removals)
        }
    };
    let note = format!(
        "{} {}",
        seed_note(situation),
        why_for(&first.goal, first, goals.len(), fill)
    );
    let kind = if pointer_get(&template.body, "/meta/note").is_some() {
        "replace"
    } else {
        "add"
    };
    patch.push(op(kind, "/meta/note", Some(string(&note))));
    Some(Composed {
        template_id,
        goals,
        patch,
        removals,
    })
}

/// The route index a step pointer names: `/declarative/route/3/...` is 3.
fn route_index(pointer: &str) -> Option<usize> {
    pointer
        .strip_prefix("/declarative/route/")?
        .split('/')
        .next()?
        .parse::<usize>()
        .ok()
}

/// The values one goal puts into its walk-and-place: the walk's and the
/// site's location, and for a Generator the anchor's voxel, each with the
/// declared pointer it goes to.
fn values_of(template: &Declared, goal: &Goal) -> Option<Vec<(String, Json)>> {
    let walk = template.pointer_ending("/move/to")?.to_owned();
    let place = template.pointer_ending("/place_beacon/at")?.to_owned();
    match *goal {
        Goal::Mine { site, .. } => Some(vec![
            (walk, voxel_location(site)),
            (place, voxel_location(site)),
        ]),
        Goal::Generator { site, anchor, .. } => {
            let at = template.pointer_ending("/anchor/voxel")?.to_owned();
            Some(vec![
                (walk, voxel_location(site)),
                (place, voxel_location(site)),
                (at, voxel_json(anchor)),
            ])
        }
    }
}

/// A copy of a template's route step with each value set at its pointer
/// below the step, and its label given the goal's number: `to_site` becomes
/// `to_site_2`.
fn step_for(step: &Json, prefix: &str, values: &[(String, Json)], n: usize) -> Option<Json> {
    let label = crate::wire::text_of(step, "label");
    let mut out = with_member(step, "label", string(&format!("{label}_{n}")));
    for (pointer, value) in values {
        if let Some(below) = pointer
            .strip_prefix(prefix)
            .filter(|below| below.starts_with('/'))
        {
            out = with_pointer(&out, below, value.clone())?;
        }
    }
    Some(out)
}

/// Both place-a-beacon templates: the first goal into the declared walk,
/// site and (for Hold & Build) anchor, and one more walk-and-place per
/// further goal, inserted after the declared place. With `fill`, the declared
/// hold -- the step after the place -- fills what is left of the share; it is
/// replaced before any step is inserted ahead of it, since a JSON Patch
/// applies its operations in order.
fn place_patch(
    template: &Declared,
    goals: &[Candidate],
    fill: Option<i64>,
) -> Option<(Vec<Json>, Vec<Vec<Json>>)> {
    let walk = template.pointer_ending("/move/to")?.to_owned();
    let place = template.pointer_ending("/place_beacon/at")?.to_owned();
    let walk_index = route_index(&walk)?;
    let walk_prefix = format!("/declarative/route/{walk_index}");
    let walk_step = pointer_get(&template.body, &walk_prefix)?.clone();
    let place_index = route_index(&place)?;
    let place_prefix = format!("/declarative/route/{place_index}");
    let place_step = pointer_get(&template.body, &place_prefix)?.clone();
    let mut patch: Vec<Json> = Vec::new();
    let mut removals: Vec<Vec<Json>> = Vec::new();
    if let (Some(fill), Some(hold)) = (fill, template.pointer_ending("/hold/ms")) {
        let used = goals
            .iter()
            .fold(0_i64, |used, goal| used.saturating_add(goal.time_ms));
        let left = fill.saturating_sub(used);
        if left > 0 {
            patch.push(op("replace", hold, Some(number(left))));
        }
    }
    for (k, goal) in goals.iter().enumerate() {
        let values = values_of(template, &goal.goal)?;
        if k == 0 {
            for (pointer, value) in values {
                patch.push(op("replace", &pointer, Some(value)));
            }
            continue;
        }
        let n = k.saturating_add(1);
        let walk_to = step_for(&walk_step, &walk_prefix, &values, n)?;
        let place_it = step_for(&place_step, &place_prefix, &values, n)?;
        let first_index = place_index
            .saturating_add(1)
            .saturating_add(k.saturating_sub(1).saturating_mul(2));
        let second_index = first_index.saturating_add(1);
        patch.push(op(
            "add",
            &format!("/declarative/route/{first_index}"),
            Some(walk_to),
        ));
        patch.push(op(
            "add",
            &format!("/declarative/route/{second_index}"),
            Some(place_it),
        ));
        removals.push(vec![
            op(
                "remove",
                &format!("/declarative/route/{second_index}"),
                None,
            ),
            op("remove", &format!("/declarative/route/{first_index}"), None),
        ]);
    }
    Some((patch, removals))
}

/// The hold a Hold & Build suggestion fills the share with, if any is left.
pub(crate) fn hold_left(candidate: &Candidate, fill: i64) -> Option<i64> {
    Some(fill.saturating_sub(candidate.time_ms)).filter(|left| *left > 0)
}

/// Whole seconds, rounded up, for a sentence.
fn seconds(ms: i64) -> i64 {
    ms.max(0)
        .saturating_add(999)
        .checked_div(1_000)
        .unwrap_or(0)
}

/// "(x, y, z)".
fn place(at: [i32; 3]) -> String {
    let [x, y, z] = at;
    format!("({x}, {y}, {z})")
}

/// The seed line: read, recorded, and unused at Easy (decision C15).
///
/// PLACEHOLDER: whether and how Easy uses its (match, seat, round) seed --
/// "top-k" read as best-of-k with no draw -- is the owner's, at **S5**.
pub(crate) fn seed_note(situation: &Situation) -> String {
    format!(
        "Easy, seat {}, round {}, seed {} (recorded; Easy draws nothing at random).",
        situation.seat, situation.round, situation.match_seed
    )
}

/// A "why" with the seed line in front of it: every "why" Easy gives, for a
/// plan it seals and for each suggestion, records the seed (decision C15).
pub(crate) fn with_seed(situation: &Situation, why: &str) -> String {
    format!("{} {why}", seed_note(situation))
}

/// One sentence for why a goal was chosen.
///
/// PLACEHOLDER: the wording is the operator's, in English, until the one
/// string table exists. Owner, at **S6**.
pub(crate) fn why_for(goal: &Goal, candidate: &Candidate, goals: usize, fill: i64) -> String {
    let more = match goals {
        0 | 1 => String::new(),
        n => format!(" and {} more like it", n.saturating_sub(1)),
    };
    match goal {
        Goal::Mine {
            site,
            seam,
            richness,
            expands,
        } => format!(
            "A Mine beacon at {} beside the {} ore seam at {}{}{}: {} points for {} s of route \
             (Easy fills {} s of the segment).",
            place(*site),
            richness.name(),
            place(*seam),
            if *expands {
                ", on the edge of your sphere"
            } else {
                ""
            },
            more,
            candidate.utility,
            seconds(candidate.time_ms),
            seconds(fill)
        ),
        Goal::Generator {
            site,
            anchor,
            richness,
            expands,
        } => format!(
            "A Build beacon at {}{} whose sphere holds the {} heat vent at {}, with a Generator \
             on the vent as its first Build target{}: {} points for {} s of route (Easy fills {} \
             s of the segment).",
            place(*site),
            if *expands {
                ", on the edge of your sphere,"
            } else {
                ""
            },
            richness.name(),
            place(*anchor),
            more,
            candidate.utility,
            seconds(candidate.time_ms),
            seconds(fill)
        ),
    }
}

/// Why a template has no suggestion this round.
pub(crate) fn why_none(feature: Feature) -> String {
    format!(
        "No {} is where this template can use it this round, so its own values stand.",
        feature.name()
    )
}

/// Why Hold & Build has no suggestion this round, said about the vent
/// nearest the commander (decisions-log item 113 (15): a vent whose anchor no
/// site's sphere would hold is said to be one, since nothing else refuses it
/// before S3's diagnostic).
pub(crate) fn why_no_vent(no_vent: NoVent) -> String {
    let why = match no_vent {
        NoVent::NoCommander => {
            String::from("Your commander is not in view, so no heat vent was weighed")
        }
        NoVent::NoneSeen => String::from("No heat vent is in view"),
        NoVent::Tapped(at) => format!(
            "The heat vent at {} has a Generator of yours on it already",
            place(at)
        ),
        NoVent::Poor {
            at,
            price,
            treasury,
        } => format!(
            "A Build beacon, its drone and a Generator on the heat vent at {} cost $ {price}, \
             and the treasury holds $ {treasury}",
            place(at)
        ),
        NoVent::Unfit(at, Unfit::NoBeacon) => format!(
            "You have no beacon, so no site stands inside a sphere of yours to place one whose \
             sphere would hold the heat vent at {}",
            place(at)
        ),
        NoVent::Unfit(at, Unfit::Far) => format!(
            "The heat vent at {} is more than two sphere reaches from every beacon of yours, so \
             no new beacon's sphere at the edge of yours would hold it",
            place(at)
        ),
        NoVent::Unfit(at, Unfit::NoSite) => format!(
            "No site at the edge of your spheres puts the heat vent at {} inside the new \
             beacon's sphere: the ground between them rises or falls too far, and a Generator \
             outside its beacon's sphere is never built",
            place(at)
        ),
        NoVent::Dropped(at) => format!(
            "The heat vent at {} has a site, but no route to it was weighed this round as \
             reachable and worth something",
            place(at)
        ),
    };
    format!("{why}, so this template's own values stand.")
}

/// Why the safe playbook raises what it raises.
///
/// It follows the safe playbook's own reading of "short" and "at risk"
/// ([`crate::safe`]'s PLACEHOLDER, decisions-log item 113 (4)): it never says
/// power is not short while an own beacon is dark, never gives a headroom of
/// zero or more as a shortage, and never says no browned-out beacon is in
/// reach when the only dark beacon is the core. With nothing dark and the
/// headroom at zero or more, the text is the one T18 wrote.
///
/// When the view did not complete within [`PAGES_MAX`] pages the ground and
/// the commander were not all read, so nothing was planned from them; the
/// sentence says so rather than degrading quietly.
pub(crate) fn why_safe(situation: &Situation, raised: &[String]) -> String {
    let why = if !crate::safe::power_short(situation) {
        String::from(
            "Power is not short, so nothing is raised: move to the safest beacon and stay \
             with it.",
        )
    } else if !raised.is_empty() {
        format!(
            "Power is short {}: raise {} to HIGH, nearest first, so they are the first to \
             revive and the last to brown out.",
            shortage(situation),
            raised.join(" and ")
        )
    } else {
        format!(
            "Power is short {}, and {}, so nothing is raised: move to the safest beacon and \
             stay with it.",
            shortage(situation),
            nothing_raised(situation)
        )
    };
    if situation.view_complete {
        why
    } else {
        format!(
            "{why} (The view did not complete within {PAGES_MAX} pages, so the ground and the \
             commander were not all read and nothing else was planned.)"
        )
    }
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

/// Why power is short, as a "because" clause: the own beacons that are
/// browned out, the core named as such, and a headroom only when it is below
/// zero -- a headroom of zero or more is never given as a shortage.
fn shortage(situation: &Situation) -> String {
    let mut dark: Vec<String> = Vec::new();
    for beacon in situation.own_beacons().filter(|beacon| !beacon.powered) {
        if beacon.core {
            dark.insert(0, format!("the core {}", beacon.id));
        } else {
            dark.push(beacon.id.clone());
        }
    }
    let headroom = situation.economy.headroom_kw;
    match (dark.is_empty(), headroom < 0) {
        (false, false) => format!(
            "because {} {} browned out",
            listed(&dark),
            is_are(dark.len())
        ),
        (false, true) => format!(
            "because {} {} browned out and the headroom is {headroom} kW",
            listed(&dark),
            is_are(dark.len())
        ),
        _ => format!("because the headroom is {headroom} kW"),
    }
}

/// Why nothing is raised although power is short.
fn nothing_raised(situation: &Situation) -> String {
    if situation.own_beacons().any(crate::safe::at_risk) {
        return format!(
            "no browned-out beacon it may raise is within {} s",
            seconds(SAFE_REACH_MS)
        );
    }
    let high: Vec<String> = situation
        .own_beacons()
        .filter(|beacon| !beacon.core && !beacon.powered && beacon.priority == "HIGH")
        .map(|beacon| beacon.id.clone())
        .collect();
    let core_dark = situation
        .own_beacons()
        .any(|beacon| beacon.core && !beacon.powered);
    let mut reasons: Vec<String> = Vec::new();
    if !high.is_empty() {
        reasons.push(format!(
            "{} {} HIGH already",
            listed(&high),
            is_are(high.len())
        ));
    }
    if core_dark {
        reasons.push(String::from("the safe playbook never raises the core"));
    }
    if reasons.is_empty() {
        String::from("no beacon is browned out")
    } else {
        reasons.join(" and ")
    }
}

/// The compact JSON text of a voxel location, as a parameter value.
pub(crate) fn location_text(at: [i32; 3]) -> String {
    compact(&voxel_location(at))
}

/// The compact JSON text of a voxel, as a parameter value.
pub(crate) fn voxel_text(at: [i32; 3]) -> String {
    compact(&voxel_json(at))
}
