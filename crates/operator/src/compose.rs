// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Composing a round: greedy insertion by utility per second, and the "why".
//!
//! Spec section 14: "compose visit goals by greedy insertion by utility per
//! second until the route fills its target share of the segment (the
//! segment's length comes from the snapshot)". At Easy the share is 70 %.
//! The top-k candidates are walked in rank order and each is inserted when
//! it is the same template's goal as the first, names a feature no goal
//! before it names, fits what is left of the share, and is affordable from
//! the treasury as it stands. The first goal chooses the template; each
//! further one becomes one more step of it -- a Mine beacon covering its seam
//! for Expand & Mine; a Build beacon covering its vent, or a Generator added
//! to an existing beacon's list, for Hold & Build -- written as a JSON Patch
//! the gateway's `patch_plan` applies to the template's own instantiation, so
//! comments and layout survive.
//!
//! Every target the patch writes is a **name** (Easy's row: "fixed targets
//! only"; `docs/design/targeting.md`, its surfaces section): the template's description,
//! "the nearest seam (or vent) you do not cover yet", becomes the feature Easy
//! chose, `{"feature_id": "seam_348_31"}`. Since walk-in
//! (`docs/design/targeting.md`, "Companion changes") a `place_beacon` walks
//! to its site itself, so no walk is written before it.
//!
//! # The core's dig depth
//!
//! Every composed route **opens** with one more step, at the seat's core: a
//! settings row giving its Mine settings a `dig_max_depth`
//! ([`deepen_core`]; decisions-log item 133 (3) (a)). The core is on the
//! Build mandate, and its starting mining drone works the starting seam
//! because a unit's kind is its job (item 127 (6)); at the default depth of
//! 0 it takes the seam's top layer and stops, which is the demo's F2. The
//! safe playbook ends with the same step ([`crate::safe`]), so a seat nobody
//! edits keeps earning past round 2.
//!
//! It goes first so that it lands however the rest of the route runs: a
//! setting persists, and the commander opens round 1 beside its core, so
//! there it costs little more than its interface time. Every goal is costed
//! from the core ([`crate::candidates::evaluate`]). The share is the visit
//! goals' (spec section 14: "compose visit goals ... until the route fills
//! its target share"), so the core's step is not inserted into it: its cost
//! -- the walk from the commander to the core, each candidate's `lead_ms`,
//! and the interface time -- comes out of Hold & Build's hold, so that route
//! still ends at the share, and is otherwise carried by the part of the
//! segment Easy leaves unfilled. A hold with nothing left for it is taken out
//! rather than left at the template's own length.
//!
//! No lookahead of any kind: a goal is inserted on its own estimate, and
//! nothing asks what the match would do with it (AGENTS.md section 3 rule 2).

use pharmakos_proto::json::Json;

use crate::candidates::{Candidate, Goal, NoVent, route_index, tap_step};
use crate::easy::{CORE_DIG_MAX_DEPTH, EASY_ROUTE_FILL_PERCENT, EXPAND_AND_MINE, HOLD_AND_BUILD};
use crate::playbook::{Declared, op, pointer_get, with_member, with_pointer};
use crate::situation::{Kind, Situation};
use crate::tuning::Tuning;
use crate::wire::{number, object, string};

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

/// The share of the segment the route may fill, game milliseconds; `None`
/// when the segment's length does not fit the arithmetic.
pub(crate) fn fill_ms(situation: &Situation) -> Option<i64> {
    situation
        .segment_ms
        .max(0)
        .checked_mul(EASY_ROUTE_FILL_PERCENT)?
        .checked_div(100)
}

/// The step that gives the core's Mine settings Easy's dig depth
/// ([`CORE_DIG_MAX_DEPTH`]): one settings row at the core, skipped rather
/// than retried if it fails (the core gone, say). It switches no mandate,
/// recycles nothing and places nothing.
pub(crate) fn deepen_core(core: &str) -> Json {
    object(vec![
        ("label", string("deepen_core")),
        (
            "interface",
            object(vec![
                ("beacon", object(vec![("beacon_id", string(core))])),
                (
                    "rows",
                    Json::Array(vec![object(vec![(
                        "set_mandate_settings",
                        object(vec![(
                            "mine",
                            object(vec![(
                                "dig_max_depth",
                                number(i64::from(CORE_DIG_MAX_DEPTH)),
                            )]),
                        )]),
                    )])]),
                ),
            ]),
        ),
        ("on_fail", object(vec![("action", string("SKIP"))])),
    ])
}

/// Greedy insertion over the top-k, in rank order, into `share` game
/// milliseconds and the treasury.
fn greedy(top: &[Candidate], share: i64, treasury: i64) -> Vec<Candidate> {
    let mut goals: Vec<Candidate> = Vec::new();
    let (mut time, mut cost) = (0_i64, 0_i64);
    for candidate in top {
        if goals
            .first()
            .is_some_and(|first| first.goal.template() != candidate.goal.template())
        {
            continue;
        }
        if goals
            .iter()
            .any(|held| held.goal.feature() == candidate.goal.feature())
        {
            // One goal per feature: a second beacon covering one seam, or a
            // second Generator on one vent, would buy nothing the first
            // did not.
            continue;
        }
        let (Some(then_time), Some(then_cost)) = (
            time.checked_add(candidate.time_ms),
            cost.checked_add(candidate.cost_dollars),
        ) else {
            continue;
        };
        if then_time > share || then_cost > treasury {
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
    tuning: &Tuning,
    top: &[Candidate],
    declared: &[Declared],
) -> Option<Composed> {
    let fill = fill_ms(situation)?;
    let core = situation.core().map(|core| core.id.clone());
    // The walk to the core is one estimate's first leg, the same for every
    // candidate; the largest is taken so no reading of it is under-held. No
    // candidate at all is nothing to compose.
    let lead = top.iter().map(|candidate| candidate.lead_ms).max()?;
    let held_back = match core {
        Some(_) => tuning.edit_settings_base_ms.checked_add(lead)?,
        None => 0,
    };
    let goals = greedy(top, fill, situation.economy.treasury);
    let first = goals.first()?;
    let template_id = match first.goal {
        Goal::Mine { .. } => EXPAND_AND_MINE,
        Goal::Generator { .. } | Goal::Tap { .. } => HOLD_AND_BUILD,
    };
    let template = declared
        .iter()
        .find(|held| held.template_id == template_id)?;
    // The core's step goes in at route 0 as the patch's last operation, so
    // every pointer above it is the template's; the removals are applied to
    // the patched text, where each step stands one further on.
    let shift = usize::from(core.is_some());
    let (mut patch, removals) = place_patch(template, &goals, fill, held_back, shift)?;
    let note = format!(
        "{} {}",
        seed_note(situation),
        why_for(first, goals.len(), fill)
    );
    let kind = if pointer_get(&template.body, "/meta/note").is_some() {
        "replace"
    } else {
        "add"
    };
    patch.push(op(kind, "/meta/note", Some(string(&note))));
    if let Some(core) = core {
        patch.push(op("add", "/declarative/route/0", Some(deepen_core(&core))));
    }
    Some(Composed {
        template_id,
        goals,
        patch,
        removals,
    })
}

/// `{"feature_id": "<name>"}`: a feature reference by name.
fn named(feature: &str) -> Json {
    object(vec![("feature_id", string(feature))])
}

/// A copy of the template's place step naming `feature`, its label given the
/// goal's number: `place_mine` becomes `place_mine_2`.
fn place_copy(step: &Json, below: &str, feature: &str, n: usize) -> Option<Json> {
    let label = crate::wire::text_of(step, "label");
    let out = with_member(step, "label", string(&format!("{label}_{n}")));
    with_pointer(&out, below, named(feature))
}

/// The goals into the template: the first replaces the declared place
/// step's description with its name (a tap replaces the whole step with its
/// own `interface` step), and each further goal is one more step inserted
/// after it. Hold & Build's declared hold -- the step after the place --
/// fills what is left of the share once the core's step is paid for, or is
/// taken out when nothing is left, and either is done before any step is
/// inserted ahead of it, since a JSON Patch applies its operations in order.
/// Each removal names its step `shift`
/// places further on than the template does: where it stands once the
/// core's step is in front of it.
fn place_patch(
    template: &Declared,
    goals: &[Candidate],
    fill: i64,
    held_back: i64,
    shift: usize,
) -> Option<(Vec<Json>, Vec<Vec<Json>>)> {
    let covering = template
        .pointer_ending("/place_beacon/at/covering")?
        .to_owned();
    let place_index = route_index(&covering)?;
    let place_prefix = format!("/declarative/route/{place_index}");
    let place_step = pointer_get(&template.body, &place_prefix)?.clone();
    let below = covering.strip_prefix(&place_prefix)?.to_owned();
    let mut patch: Vec<Json> = Vec::new();
    let mut removals: Vec<Vec<Json>> = Vec::new();
    if let Some(hold) = template.pointer_ending("/hold/ms") {
        let used = goals
            .iter()
            .try_fold(0_i64, |used, goal| used.checked_add(goal.time_ms))?;
        let left = fill.checked_sub(used)?.checked_sub(held_back)?;
        if left > 0 {
            patch.push(op("replace", hold, Some(number(left))));
        } else {
            // A hold lasts a positive time, and none is left: the step goes,
            // rather than keeping the template's own length past the share.
            let step = hold.strip_suffix("/hold/ms")?;
            if route_index(step)? <= place_index {
                return None;
            }
            patch.push(op("remove", step, None));
        }
    }
    for (k, goal) in goals.iter().enumerate() {
        let n = k.checked_add(1)?;
        let step = match &goal.goal {
            Goal::Mine { seam: name, .. } | Goal::Generator { vent: name, .. } => {
                if k == 0 {
                    patch.push(op("replace", &covering, Some(named(name))));
                    continue;
                }
                place_copy(&place_step, &below, name, n)?
            }
            Goal::Tap { beacon, vent, .. } => {
                let label = if k == 0 {
                    String::from("add_generator")
                } else {
                    format!("add_generator_{n}")
                };
                let step = tap_step(&label, beacon, vent);
                if k == 0 {
                    patch.push(op("replace", &place_prefix, Some(step)));
                    continue;
                }
                step
            }
        };
        let index = place_index.checked_add(k)?;
        patch.push(op(
            "add",
            &format!("/declarative/route/{index}"),
            Some(step),
        ));
        let shifted = format!("/declarative/route/{}", index.checked_add(shift)?);
        removals.push(vec![op("remove", &shifted, None)]);
    }
    Some((patch, removals))
}

/// The hold a Hold & Build suggestion fills the share with, if any is left
/// once the goal and the core's depth are paid for.
pub(crate) fn hold_left(candidate: &Candidate, fill: i64, held_back: i64) -> Option<i64> {
    fill.checked_sub(candidate.time_ms)?
        .checked_sub(held_back)
        .filter(|left| *left > 0)
}

/// Whole seconds, rounded up, for a sentence: "42 s". A duration here is
/// never negative; one that were is written as it is, in milliseconds,
/// rather than read as none.
pub(crate) fn seconds(ms: i64) -> String {
    u64::try_from(ms).map_or_else(
        |_| format!("{ms} ms"),
        |ms| format!("{} s", ms.div_ceil(1_000)),
    )
}

/// "(x, y, z)".
fn place(at: [i32; 3]) -> String {
    let [x, y, z] = at;
    format!("({x}, {y}, {z})")
}

/// The seed line: read, recorded, and unused at Easy (decision C15).
///
/// PLACEHOLDER: whether and how Easy uses its (match, seat, round) seed, "top-k" read as best-of-k with no draw — owner, S5
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
/// PLACEHOLDER: the wording is the operator's, in English, until the one string table exists — owner, S6
pub(crate) fn why_for(candidate: &Candidate, goals: usize, fill: i64) -> String {
    let more = goals
        .checked_sub(1)
        .filter(|others| *others > 0)
        .map(|others| format!(" and {others} more like it"))
        .unwrap_or_default();
    let tail = format!(
        ": {} points for {} of route (Easy fills {} of the segment).",
        candidate.utility,
        seconds(candidate.time_ms),
        seconds(fill)
    );
    match &candidate.goal {
        Goal::Mine { seam, grade } => format!(
            "A Mine beacon covering the {} ore seam {seam}, at {} as its site reads \
             now{more}{tail}",
            grade.name(),
            place(candidate.place),
        ),
        Goal::Generator { vent, grade } => format!(
            "A Build beacon covering the {} heat vent {vent}, at {} as its site reads now, with \
             a Generator on the vent as its first Build target{more}{tail}",
            grade.name(),
            place(candidate.place),
        ),
        Goal::Tap {
            beacon,
            vent,
            grade,
        } => format!(
            "A Generator on the {} heat vent {vent}, added on site to the Build list of {beacon}, \
             whose sphere holds it{more}{tail}",
            grade.name(),
        ),
    }
}

/// Why a template has no suggestion this round.
pub(crate) fn why_none(kind: Kind) -> String {
    format!(
        "No {} you do not cover yet is within reach and worth a beacon this round, so this \
         template's own values stand.",
        kind.name()
    )
}

/// Why Hold & Build has no suggestion this round, said about the vent its
/// own description would read.
pub(crate) fn why_no_vent(no_vent: &NoVent) -> String {
    let why = match no_vent {
        NoVent::NoCommander => {
            String::from("Your commander is not in view, so no heat vent was weighed")
        }
        NoVent::NoneReachable => String::from("No live heat vent is within reach"),
        NoVent::AllCovered => {
            String::from("Every heat vent within reach is inside a sphere of yours already")
        }
        NoVent::Poor {
            vent,
            price,
            treasury,
        } => format!(
            "A Build beacon, its drone and a Generator on the heat vent {vent} cost $ {price}, \
             and the treasury holds $ {treasury}"
        ),
        NoVent::Dropped(vent) => format!(
            "The heat vent {vent} is within reach, but no route covering it was weighed this \
             round as reachable and worth something"
        ),
    };
    format!("{why}, so this template's own values stand.")
}
