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
//! chooses the template; each further one becomes one more step of it -- one
//! more walk-and-place for Expand & Mine, one more Build row for Hold &
//! Build -- written as a JSON Patch the gateway's `patch_plan` applies to the
//! template's own instantiation, so comments and layout survive.
//!
//! No lookahead of any kind: a goal is inserted on its own estimate, and
//! nothing asks what the match would do with it (AGENTS.md section 3 rule 2).

use pharmakos_proto::json::Json;

use crate::candidates::{Candidate, Goal};
use crate::easy::{EASY_ROUTE_FILL_PERCENT, EXPAND_AND_MINE, HOLD_AND_BUILD};
use crate::playbook::{Declared, op, pointer_get, with_member};
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
            let (patch, removals) = mine_patch(template, &goals)?;
            (EXPAND_AND_MINE, patch, removals)
        }
        Goal::Generator { .. } => {
            let (patch, removals) = build_patch(template, &goals, fill)?;
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

/// Expand & Mine: the first site into the declared walk and place, and one
/// more walk-and-place per further site, before the walk home.
fn mine_patch(template: &Declared, goals: &[Candidate]) -> Option<(Vec<Json>, Vec<Vec<Json>>)> {
    let walk = template.pointer_ending("/move/to")?.to_owned();
    let place = template.pointer_ending("/place_beacon/at")?.to_owned();
    let walk_step = pointer_get(
        &template.body,
        &format!("/declarative/route/{}", route_index(&walk)?),
    )?
    .clone();
    let place_index = route_index(&place)?;
    let place_step =
        pointer_get(&template.body, &format!("/declarative/route/{place_index}"))?.clone();
    let mut patch: Vec<Json> = Vec::new();
    let mut removals: Vec<Vec<Json>> = Vec::new();
    for (k, goal) in goals.iter().enumerate() {
        let Goal::Mine { site, .. } = goal.goal else {
            continue;
        };
        let at = voxel_location(site);
        if k == 0 {
            patch.push(op("replace", &walk, Some(at.clone())));
            patch.push(op("replace", &place, Some(at)));
            continue;
        }
        let n = k.saturating_add(1);
        let walk_to = with_member(
            &with_member(&walk_step, "label", string(&format!("to_site_{n}"))),
            "move",
            with_member(walk_step.get("move")?, "to", at.clone()),
        );
        let place_it = with_member(
            &with_member(&place_step, "label", string(&format!("place_mine_{n}"))),
            "place_beacon",
            with_member(place_step.get("place_beacon")?, "at", at),
        );
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

/// Hold & Build: the first vent into the declared anchor, one more Build row
/// per further vent, and the hold filling what is left of the share.
fn build_patch(
    template: &Declared,
    goals: &[Candidate],
    fill: i64,
) -> Option<(Vec<Json>, Vec<Vec<Json>>)> {
    let anchor = template.pointer_ending("/anchor/voxel")?.to_owned();
    // `.../rows/<j>/add_build_target/target/anchor/voxel`.
    let (rows, rest) = anchor.split_once("/rows/")?;
    let row_index = rest.split('/').next()?.parse::<usize>().ok()?;
    let row = pointer_get(&template.body, &format!("{rows}/rows/{row_index}"))?.clone();
    let mut patch: Vec<Json> = Vec::new();
    let mut removals: Vec<Vec<Json>> = Vec::new();
    let mut used: i64 = 0;
    for (k, goal) in goals.iter().enumerate() {
        let Goal::Generator { anchor: at, .. } = goal.goal else {
            continue;
        };
        used = used.saturating_add(goal.time_ms);
        if k == 0 {
            patch.push(op("replace", &anchor, Some(voxel_json(at))));
            continue;
        }
        let target = row.get("add_build_target")?.get("target")?;
        let order = crate::wire::int_of(target, "order").saturating_add(i64::try_from(k).ok()?);
        let moved = with_member(
            &with_member(
                target,
                "anchor",
                with_member(target.get("anchor")?, "voxel", voxel_json(at)),
            ),
            "order",
            number(order),
        );
        let new_row = with_member(
            &row,
            "add_build_target",
            with_member(row.get("add_build_target")?, "target", moved),
        );
        let row_path = format!("{rows}/rows/{}", row_index.saturating_add(k));
        patch.push(op("add", &row_path, Some(new_row)));
        removals.push(vec![op("remove", &row_path, None)]);
    }
    if let Some(hold) = template.pointer_ending("/hold/ms") {
        let left = fill.saturating_sub(used);
        if left > 0 {
            patch.push(op("replace", hold, Some(number(left))));
        }
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
            anchor,
            richness,
            core,
        } => format!(
            "A Generator on the {} heat vent at {} inside {core}'s sphere{}: {} points for {} s \
             of route (Easy fills {} s of the segment).",
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

/// Why the safe playbook raises what it raises.
pub(crate) fn why_safe(situation: &Situation, raised: &[String]) -> String {
    if raised.is_empty() {
        if situation.economy.headroom_kw < 0 {
            return format!(
                "Power is short ({} kW) and no browned-out beacon is within 60 s, so nothing is \
                 raised: move to the safest beacon and stay with it.",
                situation.economy.headroom_kw
            );
        }
        return String::from(
            "Power is not short, so nothing is raised: move to the safest beacon and stay with it.",
        );
    }
    format!(
        "Power is short ({} kW): raise {} to HIGH, nearest first, so they brown out last.",
        situation.economy.headroom_kw,
        raised.join(" and ")
    )
}

/// The compact JSON text of a voxel location, as a parameter value.
pub(crate) fn location_text(at: [i32; 3]) -> String {
    compact(&voxel_location(at))
}

/// The compact JSON text of a voxel, as a parameter value.
pub(crate) fn voxel_text(at: [i32; 3]) -> String {
    compact(&voxel_json(at))
}
