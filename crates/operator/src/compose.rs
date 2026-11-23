// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Composing a round: greedy insertion by utility per second, and the "why".
//!
//! Spec section 14: "compose visit goals by greedy insertion by utility per
//! second until the route fills its target share of the segment (the
//! segment's length comes from the snapshot)". At Easy the share is 70 %.
//!
//! # One ranking
//!
//! Every candidate -- a placement, a tap, a stone, a raid and the guard post
//! -- is scored in the same terms ([`crate::candidates`]) and
//! funded in score order from **one** budget: the share, the treasury and the
//! kW headroom (the forecast's supply less its draw; S3's `opr3`, fix pass 2,
//! item 2). Two things are funded ahead of the ranking, as ruled: the guard
//! post in the round after a Push in which the commander died (spec section
//! 14: Easy "adds a Defend guard on its commander when the commander has died
//! recently", decision 35), and the Resonance Spire once the licence is held
//! (S3's plan, decisions 6 and 13). The top-k of what they leave -- every
//! candidate worth something whose time fits the share they leave, by utility
//! per second, ties to the lowest id -- is walked in rank order:
//!
//! * the **guard post** is funded when no post is yet: it covers the post
//!   seam beside the core; its step goes in front of the template's, beside
//!   the core where the route opens;
//! * a **goal** is funded when it is the same template's goal as the first
//!   (the first chooses the template), names a feature no goal or post before
//!   it names, is the round's one stone if it is a stone, and fits what is
//!   left of the budget, its timeout fitting the round.
//!
//! Each further goal becomes one more step of the template -- a Mine beacon
//! covering its seam for Expand & Mine; a Build beacon covering its vent, a
//! Generator added to an existing beacon's list, or a stepping stone placed
//! `toward` a far feature, for Hold & Build -- written as a JSON Patch the
//! gateway's `patch_plan` applies to the template's own instantiation, so
//! comments and layout survive. A round with the guard and no goal fills
//! Defend the Core with it. (The Survey post was cut: S3's plan, cut 3.)
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
//! # Each step's timeout
//!
//! A `place_beacon`'s `timeout_ms` bounds its walk up to arrival (the sim's
//! `begin_visit` and `begin_deploy` stop the clock), and a step that runs past
//! it fails `timeout`. So every placement Easy composes -- the template's own
//! place step, each copy of it, a stone, the guard post --
//! carries a timeout **derived from its own price** ([`timeout_ms`]), never
//! the template's literal (S3's `opr3`, fix pass 2, item 1: Hold & Build's
//! 60 s, copied onto a stone whose priced walk was 80 s, failed it from round
//! 2): the walk back from the step before it, its own walk and its on-site
//! time as [`crate::price`] gives it, plus the travel estimate's own
//! allowance for the wait before a walk starts (`walk_allowance_ms` in
//! [`crate::tuning`], a derived bound). A step whose timeout is longer than
//! the segment cannot finish in the round, and is not composed. A tap and the
//! core's own steps are `interface` steps with no timeout, as the library's
//! own `interface` steps have none.
//!
//! No lookahead of any kind: a goal is inserted on its own estimate, and
//! nothing asks what the match would do with it (AGENTS.md section 3 rule 2).

use pharmakos_proto::json::Json;

use crate::candidates::{Candidate, Goal, NoVent, RaidTarget, core_threat, route_index, tap_step};
use crate::easy::{CORE_DIG_MAX_DEPTH, DEFEND_THE_CORE, EASY_ROUTE_FILL_PERCENT, SAFE_PLAYBOOK};
use crate::playbook::{Declared, op, pointer_get, with_member, with_pointer};
use crate::situation::{Kind, LastPush, PushUnread, Situation};
use crate::tuning::Tuning;
use crate::wire::{number, object, string, voxel_json};

/// A composed round: the template and the patch that fills it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Composed {
    /// Which template.
    pub(crate) template_id: &'static str,
    /// The JSON Patch, applied to the template's own instantiation.
    pub(crate) patch: Vec<Json>,
    /// For each goal after the first, the patch that takes it out again:
    /// the repair of last resort.
    pub(crate) removals: Vec<Vec<Json>>,
    /// Why, in the sentences the playbook's note carries after its seed line.
    pub(crate) why: String,
}

/// The Resonance Spire Easy queues at its core (S3's plan, `opr3`; decisions
/// 6 and 13): once its seat holds the licence, while no Spire of the seat's
/// stands or is going up, and while the grid's headroom carries the Spire's
/// draw. It is **funded ahead of the ranking** (module docs, "One ranking"):
/// its price and draw come out of the budget before any ranked candidate's,
/// and its time out of the hold.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Spire {
    /// The core's `b_NN`, where it is queued.
    pub(crate) core: String,
    /// Game milliseconds it costs the route: one more visit at the core, its
    /// handshake and its one `queue_structure` row ([`crate::price`]).
    pub(crate) time_ms: i64,
    /// Whole $ the row takes when it commits.
    pub(crate) cost_dollars: i64,
    /// Whole kW it draws once it stands.
    pub(crate) draw_kw: i64,
}

/// The Spire Easy would queue this round, or `None`: no licence, no core, a
/// view that did not complete (so a Spire already queued could not be seen),
/// a Spire of the seat's in view, or a core whose sphere is grown past the
/// base (a finished Spire stands on it). The treasury and the kW budget are
/// the composition's to weigh ([`compose`]).
///
/// One Spire per beacon is the sim's rule (`spire_present`, and the
/// verifier's E0513), and Easy queues one only at its core, so any Spire of
/// the seat's is the core's.
pub(crate) fn spire(situation: &Situation, tuning: &Tuning) -> Option<Spire> {
    if !situation.spire_licensed || !situation.view_complete || situation.own_spire_seen {
        return None;
    }
    let core = situation.core()?;
    if core.sphere_radius_voxels? != tuning.sphere_radius_voxels {
        return None;
    }
    Some(Spire {
        core: core.id.clone(),
        time_ms: crate::price::step_ms(&spire_step(&core.id), tuning)?,
        cost_dollars: tuning.spire_cost_dollars,
        draw_kw: tuning.spire_draw_kw,
    })
}

/// The step that queues the Resonance Spire at the core: one
/// `queue_structure` row, skipped rather than retried if it fails (the
/// licence voided, the core gone, a Spire already going up).
pub(crate) fn spire_step(core: &str) -> Json {
    object(vec![
        ("label", string("queue_spire")),
        (
            "interface",
            object(vec![
                ("beacon", object(vec![("beacon_id", string(core))])),
                (
                    "rows",
                    Json::Array(vec![object(vec![(
                        "queue_structure",
                        object(vec![("blueprint_id", string("resonance_spire"))]),
                    )])]),
                ),
            ]),
        ),
        ("on_fail", object(vec![("action", string("SKIP"))])),
    ])
}

/// Why the Spire is queued.
///
/// PLACEHOLDER: the wording is the operator's, in English, until the one string table exists — owner, S6
pub(crate) fn spire_why(spire: &Spire, tuning: &Tuning, alone: bool) -> String {
    format!(
        "{} a Resonance Spire at the core {}, which your licence allows, for $ {} and {} kW \
         of draw once it stands.",
        if alone { "Queue" } else { "Then queue" },
        spire.core,
        spire.cost_dollars,
        tuning.spire_draw_kw
    )
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

/// A composed step's timeout, game milliseconds: the walk back from the step
/// before it (`back_ms`), the step's own walk from the core and its on-site
/// time, and the travel estimate's allowance for the wait before a walk starts
/// ([`crate::tuning::Tuning::walk_allowance_ms`]); `None` when it does not fit
/// a step's `int32`.
///
/// Every goal is estimated from the core, so a step after another starts its
/// walk where that one stood: its walk is bounded by the way back to the core
/// plus its own way out, and the way back is costed as the way there.
///
/// PLACEHOLDER: the way back to the core costed as the way there, not estimated — owner, S5, with Normal and Hard
pub(crate) fn timeout_ms(back_ms: i64, step: &Candidate, tuning: &Tuning) -> Option<i64> {
    let timeout = back_ms
        .checked_add(step.walk_ms)?
        .checked_add(step.on_site_ms)?
        .checked_add(tuning.walk_allowance_ms)?;
    (timeout <= i64::from(i32::MAX)).then_some(timeout)
}

/// A step's timeout when it fits the round: one past the segment cannot
/// finish before the segment ends, so the step is not composed.
fn timeout_within(
    back_ms: i64,
    step: &Candidate,
    tuning: &Tuning,
    situation: &Situation,
) -> Option<i64> {
    timeout_ms(back_ms, step, tuning).filter(|timeout| *timeout <= situation.segment_ms)
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

/// The declared template of this id.
fn declared_template<'d>(declared: &'d [Declared], template_id: &str) -> Option<&'d Declared> {
    declared.iter().find(|held| held.template_id == template_id)
}

/// The walk back the template's first goal starts with: none for a template
/// whose place step is not its first (Defend the Core's and Raid's open with
/// their own `to_core`, so the commander stands at the core when it starts),
/// else the walk back from the last post in front of the template's steps.
/// `None` when the template is not declared or names no place step.
fn lead_back(declared: &[Declared], template_id: &str, posts_back_ms: i64) -> Option<i64> {
    let template = declared_template(declared, template_id)?;
    let index = route_index(template.pointer_ending("/place_beacon/at/covering")?)?;
    Some(if index > 0 { 0 } else { posts_back_ms })
}

/// The post a round places in front of the template's steps: the guard
/// post, covering the post seam beside the core ([`crate::candidates`]).
#[derive(Clone, Copy)]
enum Post<'a> {
    /// A Defend beacon guarding the core and the commander.
    Guard(&'a Candidate),
}

impl<'a> Post<'a> {
    /// The candidate it is.
    const fn candidate(self) -> &'a Candidate {
        match self {
            Post::Guard(candidate) => candidate,
        }
    }
}

/// What one round funds.
struct Funded<'a> {
    /// The post in front of the template's steps, if one is funded.
    post: Option<Post<'a>>,
    /// The Spire queued at the core, if it is funded.
    spire: Option<&'a Spire>,
    /// The goals, in rank order.
    goals: Vec<Candidate>,
}

/// How many of the evaluated candidates the round's budget would fund on
/// their own: each one funded alone, by the same rules as the round (its
/// worth above zero, its time, price and draw within the budget, its timeout
/// within the segment). What a round that seals its safe playbook is held
/// to: zero (S3's `opr3`, fix pass 3, item 1).
pub(crate) fn fundable(
    situation: &Situation,
    tuning: &Tuning,
    evaluated: &[Candidate],
    declared: &[Declared],
) -> usize {
    let Some(fill) = fill_ms(situation) else {
        return 0;
    };
    evaluated
        .iter()
        .filter(|candidate| {
            fund(
                situation,
                tuning,
                declared,
                std::slice::from_ref(*candidate),
                None,
                fill,
            )
            .is_some_and(|funded| funded.post.is_some() || !funded.goals.is_empty())
        })
        .count()
}

/// Fund the round from one budget (module docs, "One ranking"): the guard
/// post after the commander's death and the Spire first, as ruled, then the
/// top-k of the rest in rank order. `None` when the headroom does not fit the
/// arithmetic.
fn fund<'a>(
    situation: &Situation,
    tuning: &Tuning,
    declared: &[Declared],
    evaluated: &'a [Candidate],
    spire: Option<&'a Spire>,
    fill: i64,
) -> Option<Funded<'a>> {
    let defend = declared_template(declared, DEFEND_THE_CORE).is_some();
    let mut purse = Purse {
        share: fill,
        treasury: situation.economy.treasury,
        headroom: situation
            .economy
            .supply_kw
            .checked_sub(situation.economy.draw_kw)?,
    };
    let mut funded = Funded {
        post: None,
        spire: None,
        goals: Vec::new(),
    };
    if defend && matches!(situation.last_push, LastPush::Died) {
        funded.post = crate::candidates::guard(evaluated)
            .filter(|guard| timeout_within(0, guard, tuning, situation).is_some())
            .filter(|guard| purse.spend(guard.time_ms, guard.cost_dollars, tuning.guard_draw_kw))
            .map(Post::Guard);
    }
    // Its step is at the core the route opens at, so a seat shown no core
    // queues no Spire; its time comes out of the hold, as the core's step's
    // does, rather than the share.
    let core = situation.core().map(|core| core.id.as_str());
    funded.spire = spire
        .filter(|spire| core == Some(spire.core.as_str()))
        .filter(|spire| purse.spend(0, spire.cost_dollars, spire.draw_kw));
    let ahead = funded.post.map(|post| post.candidate().id);
    for candidate in crate::candidates::ranked(evaluated, purse.share, ahead) {
        match candidate.goal {
            Goal::Guard { .. } => {
                if defend {
                    fund_post(
                        situation,
                        tuning,
                        declared,
                        &mut purse,
                        &mut funded,
                        candidate,
                    );
                }
            }
            Goal::Mine { .. }
            | Goal::Generator { .. }
            | Goal::Tap { .. }
            | Goal::Raid { .. }
            | Goal::Stone { .. } => {
                fund_goal(
                    situation,
                    tuning,
                    declared,
                    &mut purse,
                    &mut funded,
                    candidate,
                );
            }
        }
    }
    Some(funded)
}

/// Fund a post when no post is funded yet and no goal names its seam, its
/// timeout and the first goal's (whose walk starts at the post's site) fit
/// the round, and the budget carries its time, its price and its scouts'
/// draw.
fn fund_post<'a>(
    situation: &Situation,
    tuning: &Tuning,
    declared: &[Declared],
    purse: &mut Purse,
    funded: &mut Funded<'a>,
    candidate: &'a Candidate,
) {
    if funded.post.is_some()
        || funded
            .goals
            .iter()
            .any(|goal| goal.goal.feature() == candidate.goal.feature())
        || timeout_within(0, candidate, tuning, situation).is_none()
    {
        return;
    }
    if let Some(first) = funded.goals.first()
        && lead_back(declared, first.goal.template(), candidate.walk_ms)
            .and_then(|back| timeout_within(back, first, tuning, situation))
            .is_none()
    {
        return;
    }
    let (post, draw) = match candidate.goal {
        Goal::Guard { .. }
        | Goal::Mine { .. }
        | Goal::Generator { .. }
        | Goal::Tap { .. }
        | Goal::Raid { .. }
        | Goal::Stone { .. } => (Post::Guard(candidate), tuning.guard_draw_kw),
    };
    if purse.spend(candidate.time_ms, candidate.cost_dollars, draw) {
        funded.post = Some(post);
    }
}

/// Fund a goal when it is the same template's as the first, names a feature
/// no goal or post before it names (a second beacon covering one seam, or a
/// second Generator on one vent, would buy nothing the first did not, and a
/// raid post covers the posts' seam), is the round's one stone if it is a
/// stone (a round walks toward one far feature, and the next round's scoring
/// says whether the same one is still the one to reach), its timeout fits
/// the round, and the budget carries its time and its price.
fn fund_goal(
    situation: &Situation,
    tuning: &Tuning,
    declared: &[Declared],
    purse: &mut Purse,
    funded: &mut Funded<'_>,
    candidate: &Candidate,
) {
    let goals = &funded.goals;
    let other_template = goals
        .first()
        .is_some_and(|first| first.goal.template() != candidate.goal.template());
    let named_before = goals
        .iter()
        .chain(funded.post.map(Post::candidate))
        .any(|held| held.goal.feature() == candidate.goal.feature());
    let second_stone = matches!(candidate.goal, Goal::Stone { .. })
        && goals
            .iter()
            .any(|held| matches!(held.goal, Goal::Stone { .. }));
    if other_template || named_before || second_stone {
        return;
    }
    let back = match goals.last() {
        Some(before) => Some(before.walk_ms),
        None => lead_back(declared, candidate.goal.template(), walk_of(funded.post)),
    };
    if back
        .and_then(|back| timeout_within(back, candidate, tuning, situation))
        .is_none()
    {
        return;
    }
    if purse.spend(candidate.time_ms, candidate.cost_dollars, 0) {
        funded.goals.push(candidate.clone());
    }
}

/// A post's walk from the core, and none without a post.
const fn walk_of(post: Option<Post<'_>>) -> i64 {
    match post {
        Some(post) => post.candidate().walk_ms,
        None => 0,
    }
}

/// What the hold gives up for what stands in front of the template's steps:
/// the core's step, the walk to the core, the Spire's step and the post's;
/// `None` when the seat is shown a core but no candidate gave the walk to it.
fn held_back(
    tuning: &Tuning,
    core: Option<&str>,
    lead: Option<i64>,
    spire: Option<&Spire>,
    post: Option<Post<'_>>,
) -> Option<i64> {
    let (Some(core), Some(lead)) = (core, lead) else {
        return core.is_none().then_some(0);
    };
    let spire_ms = match spire {
        Some(spire) => spire.time_ms,
        None => 0,
    };
    let post_ms = match post {
        Some(post) => post.candidate().time_ms,
        None => 0,
    };
    crate::price::step_ms(&deepen_core(core), tuning)?
        .checked_add(lead)?
        .checked_add(spire_ms)?
        .checked_add(post_ms)
}

/// Compose the round from every evaluated candidate and the Spire, or `None`
/// when nothing is funded (module docs, "One ranking").
pub(crate) fn compose(
    situation: &Situation,
    tuning: &Tuning,
    evaluated: &[Candidate],
    spire: Option<&Spire>,
    declared: &[Declared],
) -> Option<Composed> {
    let fill = fill_ms(situation)?;
    let core = situation.core().map(|core| core.id.clone());
    let Funded { post, spire, goals } = fund(situation, tuning, declared, evaluated, spire, fill)?;
    // The walk to the core: one estimate's first leg, the same for every
    // candidate, the largest taken so no reading of it is under-held.
    let lead = goals
        .iter()
        .chain(post.map(Post::candidate))
        .map(|candidate| candidate.lead_ms)
        .max();
    // The core's step goes in at route 0 as the patch's last operation, the
    // Spire's just before it and the post's before that, so every pointer
    // above them is the template's; the removals are applied to the patched
    // text, where each step stands one further on for each of them.
    let core_shift = usize::from(core.is_some()).checked_add(usize::from(spire.is_some()))?;
    let post_step = match post {
        Some(post) => Some(post_step(tuning, declared, post)?),
        None => None,
    };
    let (template, goals, (mut patch, mut removals), front) = match (goals.first(), post) {
        (Some(first), _) => {
            let template = declared_template(declared, first.goal.template())?;
            let filled = Fill {
                share_ms: fill,
                held_back: held_back(tuning, core.as_deref(), lead, spire, post)?,
                shift: core_shift.checked_add(usize::from(post.is_some()))?,
                lead_back: lead_back(declared, &template.template_id, walk_of(post))?,
            };
            let parts = place_patch(template, &goals, filled, tuning)?;
            (template, goals, parts, post_step)
        }
        // Alone, the guard fills the Defend template itself.
        (None, Some(Post::Guard(guard))) => {
            let template = declared_template(declared, DEFEND_THE_CORE)?;
            let goals = vec![guard.clone()];
            let filled = Fill {
                share_ms: fill,
                held_back: held_back(tuning, core.as_deref(), lead, spire, None)?,
                shift: core_shift,
                lead_back: lead_back(declared, &template.template_id, 0)?,
            };
            let parts = place_patch(template, &goals, filled, tuning)?;
            (template, goals, parts, None)
        }
        (None, None) => {
            return additions_alone(situation, tuning, spire, core.as_deref()?, declared);
        }
    };
    let template_id = goals.first()?.goal.template();
    let posted = front.as_ref().map(|(post, _)| *post);
    let why = round_why(situation, tuning, &goals, posted, spire, fill)?;
    patch.push(note_op(template, &with_seed(situation, &why)));
    front_steps(
        &mut patch,
        &mut removals,
        front,
        spire,
        core.as_deref(),
        core_shift,
    );
    Some(Composed {
        template_id,
        patch,
        removals,
        why,
    })
}

/// The steps in front of the template's, put in at route 0 one after another
/// so each lands ahead of the last: the post's (with, for the guard, the
/// removal that takes it out, last of all: the repair of last resort removes
/// the goals inserted after the first before the guard), the Spire's and the
/// core's.
fn front_steps(
    patch: &mut Vec<Json>,
    removals: &mut Vec<Vec<Json>>,
    front: Option<(Post<'_>, Json)>,
    spire: Option<&Spire>,
    core: Option<&str>,
    core_shift: usize,
) {
    let mut front_step = |step: Json| patch.push(op("add", "/declarative/route/0", Some(step)));
    if let Some((post, step)) = front {
        front_step(step);
        if matches!(post, Post::Guard(_)) {
            removals.insert(
                0,
                vec![op(
                    "remove",
                    &format!("/declarative/route/{core_shift}"),
                    None,
                )],
            );
        }
    }
    if let Some(spire) = spire {
        front_step(spire_step(&spire.core));
    }
    if let Some(core) = core {
        front_step(deepen_core(core));
    }
}

/// A round whose only addition is the Spire, with no goal and no guard: the
/// Safe Playbook template, its route filled with the template's own walk to
/// safety, the core's dig depth and the Spire's step, and its note Easy's.
/// The safe playbook itself never queues it (spec section 14: it "leaves configurations untouched", never
/// places, and a person's timeout must not spend their $), so this is Easy's
/// own plan on that template's frame, submitted and reported as its own.
fn additions_alone(
    situation: &Situation,
    tuning: &Tuning,
    spire: Option<&Spire>,
    core: &str,
    declared: &[Declared],
) -> Option<Composed> {
    let spire = spire?;
    let template = declared
        .iter()
        .find(|held| held.template_id == SAFE_PLAYBOOK)?;
    let pointer = template.pointer_ending("/declarative/route")?;
    let Json::Array(own) = template.own_value(pointer)? else {
        return None;
    };
    let route = vec![
        own.first()?.clone(),
        deepen_core(core),
        spire_step(&spire.core),
    ];
    let why = spire_why(spire, tuning, true);
    Some(Composed {
        template_id: SAFE_PLAYBOOK,
        patch: vec![
            op("replace", pointer, Some(Json::Array(route))),
            note_op(template, &with_seed(situation, &why)),
        ],
        removals: Vec::new(),
        why,
    })
}

/// The one budget a round is funded from, as it stands.
struct Purse {
    share: i64,
    treasury: i64,
    headroom: i64,
}

impl Purse {
    /// Spend `time`, `dollars` and `kw` when all three fit, and say whether
    /// they did; a figure that does not fit is not spent. A figure spends
    /// nothing at zero, so it is not held to what it leaves: a goal that
    /// fields no draw is funded on a grid already short, as the verifier's
    /// E0601 is raised only on draw a route adds.
    fn spend(&mut self, time: i64, dollars: i64, kw: i64) -> bool {
        let fits = |left: i64, spent: i64| spent <= 0 || left >= 0;
        match (
            self.share.checked_sub(time),
            self.treasury.checked_sub(dollars),
            self.headroom.checked_sub(kw),
        ) {
            (Some(share), Some(treasury), Some(headroom))
                if fits(share, time) && fits(treasury, dollars) && fits(headroom, kw) =>
            {
                *self = Purse {
                    share,
                    treasury,
                    headroom,
                };
                true
            }
            _ => false,
        }
    }
}

/// The post's step, with its own timeout ([`timeout_ms`]): the guard's copy
/// of the Defend the Core template's place step, first after the core's
/// steps. `None` when the copy does not build or its timeout does not fit.
fn post_step<'a>(
    tuning: &Tuning,
    declared: &[Declared],
    post: Post<'a>,
) -> Option<(Post<'a>, Json)> {
    let defend = declared_template(declared, DEFEND_THE_CORE)?;
    let candidate = post.candidate();
    let seam = candidate.goal.feature();
    let step = match post {
        Post::Guard(_) => guard_copy(defend, seam)?,
    };
    Some((post, timed(&step, timeout_ms(0, candidate, tuning)?)))
}

/// A step with its `timeout_ms` set.
fn timed(step: &Json, timeout_ms: i64) -> Json {
    with_member(step, "timeout_ms", number(timeout_ms))
}

/// The step a goal is composed as, for pricing it ([`crate::price`]): the
/// declared template's place step naming the goal's feature (a Mine beacon,
/// a Generator's Build beacon, the guard post), the Raid template's with its
/// target, its staging at `place` and no retarget, a tap's `interface` step, or a stone's `toward` placement (a step of Hold
/// & Build's, so only while that template is declared). The timeout each
/// carries when composed is no price ([`timeout_ms`]). `None` when the
/// template it comes from is not declared.
pub(crate) fn step_for(goal: &Goal, place: [i32; 3], declared: &[Declared]) -> Option<Json> {
    let template = |id: &str| declared.iter().find(|held| held.template_id == id);
    let place_step = |id: &str| -> Option<(Json, String, String)> {
        let template = template(id)?;
        let covering = template.pointer_ending("/place_beacon/at/covering")?;
        let prefix = format!("/declarative/route/{}", route_index(covering)?);
        let step = pointer_get(&template.body, &prefix)?.clone();
        let below = covering.strip_prefix(&prefix)?.to_owned();
        Some((step, below, prefix))
    };
    match goal {
        Goal::Tap { beacon, vent, .. } => Some(tap_step("add_generator", beacon, vent)),
        // Priced as composed, less the timeout, which is no price.
        Goal::Stone { feature, .. } => {
            place_step(crate::easy::HOLD_AND_BUILD)?;
            Some(stone_step("stone", feature, None))
        }
        Goal::Guard { seam } => guard_copy(template(DEFEND_THE_CORE)?, seam),
        Goal::Mine { seam: name, .. } | Goal::Generator { vent: name, .. } => {
            let (step, below, _) = place_step(goal.template())?;
            with_pointer(&step, &below, named(name))
        }
        Goal::Raid { seam, target } => {
            let raid = template(crate::easy::RAID_AN_ENEMY_BEACON)?;
            let (mut step, below, prefix) = place_step(crate::easy::RAID_AN_ENEMY_BEACON)?;
            let target_at = raid.pointer_ending("/attack/target")?;
            let staging_at = raid.pointer_ending("/attack/staging")?;
            let retarget_at = format!("{}/retarget", target_at.strip_suffix("/target")?);
            step = with_pointer(&step, &below, named(seam))?;
            for (pointer, value) in [
                (target_at, target.json()),
                (staging_at, voxel_json(place)),
                (retarget_at.as_str(), string("NONE")),
            ] {
                step = with_pointer(&step, pointer.strip_prefix(&prefix)?, value)?;
            }
            Some(step)
        }
    }
}

/// Why the round is composed as it is: its first goal, counting the rest; the
/// guard post beside them and its reason; the Spire. `None` with no goal.
fn round_why(
    situation: &Situation,
    tuning: &Tuning,
    goals: &[Candidate],
    post: Option<Post<'_>>,
    spire: Option<&Spire>,
    fill: i64,
) -> Option<String> {
    let first = goals.first()?;
    let mut why = why_for(first, goals.len(), fill);
    let guard = post.map(Post::candidate);
    if let Some(guard) = guard {
        why = format!("{why} {}", why_for(guard, 1, fill));
    }
    if guard.is_some() || first.goal.template() == DEFEND_THE_CORE {
        why = format!("{why} {}", guard_reason(situation, tuning));
    }
    if let Some(spire) = spire {
        why = format!("{why} {}", spire_why(spire, tuning, false));
    }
    Some(why)
}

/// The operation that writes the round's note: a `replace` where the template
/// has one, an `add` where it has none.
fn note_op(template: &Declared, note: &str) -> Json {
    let kind = if pointer_get(&template.body, "/meta/note").is_some() {
        "replace"
    } else {
        "add"
    };
    op(kind, "/meta/note", Some(string(note)))
}

/// `{"feature_id": "<name>"}`: a feature reference by name.
fn named(feature: &str) -> Json {
    object(vec![("feature_id", string(feature))])
}

/// A copy of the template's place step naming `feature`, its label given the
/// goal's number: `place_mine` becomes `place_mine_2`.
fn place_copy(step: &Json, below: &str, feature: &str, n: usize) -> Option<Json> {
    let label = crate::wire::text_of(step, "label").ok()?;
    let out = with_member(step, "label", string(&format!("{label}_{n}")));
    with_pointer(&out, below, named(feature))
}

/// The Defend the Core template's place step, naming the guard's seam, as
/// the template writes it: what a route of another template gains for the
/// guard post.
fn guard_copy(defend: &Declared, seam: &str) -> Option<Json> {
    let covering = defend.pointer_ending("/place_beacon/at/covering")?;
    let index = route_index(covering)?;
    let prefix = format!("/declarative/route/{index}");
    let step = pointer_get(&defend.body, &prefix)?;
    with_pointer(step, covering.strip_prefix(&prefix)?, named(seam))
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
/// core's step is in front of it. Every placement gets its own timeout
/// ([`timeout_ms`]), the first from `lead_back`, each further one from the
/// walk of the goal before it.
fn place_patch(
    template: &Declared,
    goals: &[Candidate],
    filled: Fill,
    tuning: &Tuning,
) -> Option<(Vec<Json>, Vec<Vec<Json>>)> {
    let Fill {
        share_ms: fill,
        held_back,
        shift,
        lead_back,
    } = filled;
    let covering = template
        .pointer_ending("/place_beacon/at/covering")?
        .to_owned();
    let place_index = route_index(&covering)?;
    let place_prefix = format!("/declarative/route/{place_index}");
    let place_step = pointer_get(&template.body, &place_prefix)?.clone();
    let below = covering.strip_prefix(&place_prefix)?.to_owned();
    // A raid's two more settings, which only the Raid template declares.
    let raid_target = template.pointer_ending("/attack/target").map(str::to_owned);
    let raid_staging = template
        .pointer_ending("/attack/staging")
        .map(str::to_owned);
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
    let timeout_at = format!("{place_prefix}/timeout_ms");
    let mut back = lead_back;
    for (k, goal) in goals.iter().enumerate() {
        let n = k.checked_add(1)?;
        let timeout = timeout_ms(back, goal, tuning)?;
        back = goal.walk_ms;
        let step = match &goal.goal {
            Goal::Mine { seam: name, .. }
            | Goal::Generator { vent: name, .. }
            | Goal::Guard { seam: name } => {
                if k == 0 {
                    patch.push(op("replace", &covering, Some(named(name))));
                    patch.push(op("add", &timeout_at, Some(number(timeout))));
                    continue;
                }
                timed(&place_copy(&place_step, &below, name, n)?, timeout)
            }
            Goal::Raid { seam, target } => {
                // The post's seam, its target, and its staging at the site
                // the estimate answered, so the raiders gather at home.
                // Easy's targets are fixed (spec section 14's Easy column:
                // "Fixed only", retargeting a rule of Normal's), so it seals
                // `NONE` where the template says `NEAREST_KNOWN`.
                let retarget = format!(
                    "{}/retarget",
                    raid_target.as_deref()?.strip_suffix("/target")?
                );
                let settings = [
                    (covering.as_str(), named(seam)),
                    (raid_target.as_deref()?, target.json()),
                    (raid_staging.as_deref()?, voxel_json(goal.place)),
                    (retarget.as_str(), string("NONE")),
                ];
                if k == 0 {
                    for (pointer, value) in settings {
                        patch.push(op("replace", pointer, Some(value)));
                    }
                    patch.push(op("add", &timeout_at, Some(number(timeout))));
                    continue;
                }
                let mut step = place_copy(&place_step, &below, seam, n)?;
                for (pointer, value) in settings.into_iter().skip(1) {
                    step = with_pointer(&step, pointer.strip_prefix(&place_prefix)?, value)?;
                }
                timed(&step, timeout)
            }
            Goal::Stone { feature, .. } | Goal::Tap { vent: feature, .. } => {
                let step = match &goal.goal {
                    // An `interface` step, with no timeout (module docs).
                    Goal::Tap { beacon, .. } => {
                        tap_step(&numbered("add_generator", k, n), beacon, feature)
                    }
                    _ => stone_step(&numbered("stone", k, n), feature, Some(timeout)),
                };
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

/// What [`place_patch`] fills the template's route with besides the goals.
#[derive(Clone, Copy)]
struct Fill {
    /// The share of the segment the route may fill.
    share_ms: i64,
    /// What the hold gives up for the steps in front of the template's.
    held_back: i64,
    /// How many steps stand in front of the template's.
    shift: usize,
    /// The walk back the first goal starts with ([`lead_back`]).
    lead_back: i64,
}

/// A whole step's label: the first goal's as given, each further one
/// numbered, `stone` then `stone_2`.
fn numbered(label: &str, k: usize, n: usize) -> String {
    if k == 0 {
        label.to_owned()
    } else {
        format!("{label}_{n}")
    }
}

/// A stepping-stone step: a beacon placed `toward` a named feature, which
/// the sim stands where the seat's spheres reach furthest toward it, with the
/// timeout derived from its own price ([`timeout_ms`]; none when it is only
/// priced) and skipped rather than retried if it finds no site
/// (`scenarios/s3/toward-stepping-stone.playbook.jsonc`'s shape).
pub(crate) fn stone_step(label: &str, feature: &str, timeout_ms: Option<i64>) -> Json {
    let mut members = vec![
        ("label", string(label)),
        (
            "place_beacon",
            object(vec![(
                "at",
                object(vec![("toward", object(vec![("feature", named(feature))]))]),
            )]),
        ),
    ];
    if let Some(timeout_ms) = timeout_ms {
        members.push(("timeout_ms", number(timeout_ms)));
    }
    members.push(("on_fail", object(vec![("action", string("SKIP"))])));
    object(members)
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

/// Why a newcomer starts with Hold & Build: the guided first pick's reason
/// (spec section 13; S3's plan, decision 5 and E25), shown beside the template
/// in a Probation match's first Lull. One sentence for a person new to the
/// game, with no seed line and no scoring: a suggestion's `why` is the
/// operator's bookkeeping, and this is not.
///
/// PLACEHOLDER: the wording is the operator's, in English, until the one string table exists — owner, S6
pub const START_HERE_WHY: &str = "Start here: this places a beacon by the nearest heat vent \
     you do not cover and builds a Generator on it, which raises your power, and power is what \
     every later structure draws.";

/// The seed line: read, recorded, and unused at Easy (decision C15).
///
/// From S3 a seat's token is not told the match seed (decisions-log item 127
/// (13); S3's plan, E18), and the operator's in-process seats hold seat tokens
/// like any other (AGENTS.md section 3 rule 3), so `get_map_summary` answers
/// none and the line says so rather than naming an empty seed.
///
/// PLACEHOLDER: whether and how Easy uses its (match, seat, round) seed, "top-k" read as best-of-k with no draw — owner, S5
pub(crate) fn seed_note(situation: &Situation) -> String {
    if situation.match_seed.is_empty() {
        format!(
            "Easy, seat {}, round {}, told no seed (Easy draws nothing at random).",
            situation.seat, situation.round
        )
    } else {
        format!(
            "Easy, seat {}, round {}, seed {} (recorded; Easy draws nothing at random).",
            situation.seat, situation.round, situation.match_seed
        )
    }
}

/// A "why" with the seed line in front of it: every "why" Easy gives, for a
/// plan it seals and for each suggestion, records the seed (decision C15),
/// and says so when the last Push's feed could not tell whether the
/// commander died, which is why no guard post was added for it: the feed was
/// not read far enough, or it shows no Push (a resumed Lull).
pub(crate) fn with_seed(situation: &Situation, why: &str) -> String {
    match situation.last_push {
        LastPush::Unread(PushUnread::Resumed) => format!(
            "{} {why} (The last Push is not in this Lull's feed, as in a Lull resumed from a \
             save, so whether your commander died in it is not known, and no guard post was \
             added for it.)",
            seed_note(situation)
        ),
        LastPush::Unread(PushUnread::PastPages) => format!(
            "{} {why} (A commander died where you could see in the last Push, and the feed was \
             not read within {} pages far enough to say whose, so no guard post was added for \
             it.)",
            seed_note(situation),
            crate::easy::PAGES_MAX
        ),
        LastPush::Survived | LastPush::Died => format!("{} {why}", seed_note(situation)),
    }
}

/// One sentence for why a goal was chosen.
///
/// PLACEHOLDER: the wording is the operator's, in English, until the one string table exists — owner, S6
pub(crate) fn why_for(candidate: &Candidate, goals: usize, fill: i64) -> String {
    // One goal says nothing more; each further one is counted.
    let more = match goals.checked_sub(1) {
        Some(others) if others > 0 => format!(" and {others} more like it"),
        Some(_) | None => String::new(),
    };
    let tail = format!(
        ": {} points for {} of route (Easy fills {} of the segment).",
        candidate.utility,
        seconds(candidate.time_ms),
        seconds(fill)
    );
    match &candidate.goal {
        Goal::Mine { seam, grade } => format!(
            "A Mine beacon covering the {} scrap seam {seam}, at {} as its site reads \
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
        Goal::Guard { seam } => format!(
            "A Defend beacon beside the core, covering the scrap seam {seam}, at {} as its site \
             reads now, guarding the core and your commander{more}{tail}",
            place(candidate.place),
        ),
        Goal::Raid { seam, target } => format!(
            "An Attack beacon beside the core, covering the scrap seam {seam}, at {} as its site \
             reads now, whose raiders gather there and raid {}{more}{tail}",
            place(candidate.place),
            raid_words(target),
        ),
        Goal::Stone {
            feature,
            kind,
            grade,
        } => format!(
            "A stepping-stone beacon toward the {} {} {feature}, which no site your spheres allow \
             covers yet, at {} as its site reads now, so a later round can place a beacon \
             covering it{more}{tail}",
            grade.name(),
            kind.name(),
            place(candidate.place),
        ),
    }
}

/// Why a guard post is worth placing: decision 35's death and decision 12's
/// threat, as the operator read them.
///
/// PLACEHOLDER: the wording is the operator's, in English, until the one string table exists — owner, S6
pub(crate) fn guard_reason(situation: &Situation, tuning: &Tuning) -> String {
    let died = match situation.last_push {
        LastPush::Died => Some("your commander died in the last Push"),
        LastPush::Survived | LastPush::Unread(_) => None,
    };
    let threat = core_threat(situation, tuning)
        .filter(|threat| *threat > 0)
        .map(|threat| {
            format!(
                "the enemies in sight inside your core's sphere deal {} HP a second",
                threat.div_ceil(1_000)
            )
        });
    match (died, threat) {
        (Some(died), Some(threat)) => format!("It guards because {died}, and {threat}."),
        (Some(died), None) => format!("It guards because {died}."),
        (None, Some(threat)) => format!("It guards because {threat}."),
        (None, None) => match situation.last_push {
            LastPush::Unread(PushUnread::PastPages) => String::from(
                "No threat is in sight inside your core's sphere, and the last Push's feed was not \
                 read far enough to say whether your commander died.",
            ),
            LastPush::Unread(PushUnread::Resumed) => String::from(
                "No threat is in sight inside your core's sphere, and the last Push is not in \
                 this Lull's feed to say whether your commander died.",
            ),
            LastPush::Survived | LastPush::Died => String::from(
                "No threat is in sight inside your core's sphere and your commander did not die \
                 in the last Push.",
            ),
        },
    }
}

/// A raid's target, in words.
///
/// PLACEHOLDER: the wording is the operator's, in English, until the one string table exists — owner, S6
pub(crate) fn raid_words(target: &RaidTarget) -> String {
    match target {
        RaidTarget::Beacon { id, at } => {
            format!("the enemy beacon {id}, last seen at {}", place(*at))
        }
        RaidTarget::Commander { seat, at } => {
            format!("the commander of seat {seat}, in sight at {}", place(*at))
        }
    }
}

/// Why Raid an Enemy Beacon has no suggestion this round.
pub(crate) fn why_no_raid() -> String {
    String::from(
        "No raid was weighed this round: it needs your core and your commander in view, a live \
         scrap seam inside your core's sphere for the post, the treasury for the post and its \
         raiders, and an enemy beacon you saw within the reach window or an enemy commander in \
         sight that a route reaches, so this template's own values stand.",
    )
}

/// Why a template has no suggestion this round.
pub(crate) fn why_none(kind: Kind) -> String {
    format!(
        "No {} you do not cover yet is within reach and worth a beacon this round, so this \
         template's own values stand.",
        kind.name()
    )
}

/// Why Defend the Core has no suggestion this round.
pub(crate) fn why_no_guard() -> String {
    String::from(
        "No guard post was weighed this round: it needs your core and your commander in view, a \
         live scrap seam inside your core's sphere, no Defend beacon of yours beside the core \
         already, and the treasury for the post and its picket, so this template's own values \
         stand.",
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
