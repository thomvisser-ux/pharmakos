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
//! for Expand & Mine; a Build beacon covering its vent, a Generator added to
//! an existing beacon's list, or a stepping stone placed `toward` a far
//! feature, for Hold & Build -- written as a JSON Patch
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

use crate::candidates::{Candidate, Goal, NoVent, RaidTarget, core_threat, route_index, tap_step};
use crate::easy::{
    CORE_DIG_MAX_DEPTH, DEFEND_THE_CORE, EASY_ROUTE_FILL_PERCENT, SAFE_PLAYBOOK, SURVEY_SCOUT_COUNT,
};
use crate::playbook::{Declared, op, pointer_get, with_member, with_pointer};
use crate::situation::{Kind, LastPush, Situation};
use crate::tuning::Tuning;
use crate::wire::{number, object, string, voxel_json};

/// A composed round: the template, its goals, and the patch that fills it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Composed {
    /// Which template.
    pub(crate) template_id: &'static str,
    /// The goals, in insertion order. With no goal but the guard post, the
    /// guard is the one goal of the Defend the Core template.
    pub(crate) goals: Vec<Candidate>,
    /// The guard post added beside the goals of another template, if one is.
    pub(crate) guard: Option<Candidate>,
    /// The JSON Patch, applied to the template's own instantiation.
    pub(crate) patch: Vec<Json>,
    /// For each goal after the first, the patch that takes it out again:
    /// the repair of last resort.
    pub(crate) removals: Vec<Vec<Json>>,
    /// The Resonance Spire queued at the core this round, if one is.
    pub(crate) spire: Option<Spire>,
    /// The Survey post added beside the goals, if one is.
    pub(crate) survey: Option<Candidate>,
    /// Why, in the sentences the playbook's note carries after its seed line.
    pub(crate) why: String,
}

/// The Resonance Spire Easy queues at its core (S3's plan, `opr3`; decisions
/// 6 and 13): once its seat holds the licence, while no Spire of the seat's
/// stands or is going up, and while the grid's headroom carries the Spire's
/// draw. It is **added** beside the round's goals, as the guard post is, and
/// never ranked among them: its price comes out of the treasury and its time
/// out of the hold before the greedy insertion.
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
        if matches!(candidate.goal, Goal::Stone { .. })
            && goals
                .iter()
                .any(|held| matches!(held.goal, Goal::Stone { .. }))
        {
            // One stone chain at a time: a round walks toward one far
            // feature, and the next round's scoring says whether the same
            // feature is still the best to reach (it is nearer by a sphere,
            // so it usually is) or now coverable from the stone.
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

/// Compose the round from the top-k and the guard post, or `None` when
/// nothing fits.
///
/// The guard, when there is one worth placing ([`crate::candidates::guard`]),
/// is **added** beside the goals rather than ranked among them (spec section
/// 14: Easy "adds a Defend guard on its commander"): its time comes out of the
/// share and its price out of the treasury before the greedy insertion, and
/// its step goes into the route right after the core's, beside the core where
/// the commander opens the route. With no goal beside it, the Defend the Core
/// template is the round's playbook. A guard the library has no Defend the
/// Core template for is not placed.
pub(crate) fn compose(
    situation: &Situation,
    tuning: &Tuning,
    top: &[Candidate],
    guard: Option<&Candidate>,
    survey: Option<&Candidate>,
    spire: Option<&Spire>,
    declared: &[Declared],
) -> Option<Composed> {
    let fill = fill_ms(situation)?;
    let core = situation.core().map(|core| core.id.clone());
    let defend = declared
        .iter()
        .find(|held| held.template_id == DEFEND_THE_CORE);
    let opening_of = opening(top, guard, survey, defend.is_some());
    if opening_of.lead.is_none() && spire.is_none() {
        return None;
    }
    let (
        Added {
            guard,
            guard_ms,
            spire,
            survey,
            held_back,
            ..
        },
        goals,
    ) = weigh_and_insert(
        situation,
        tuning,
        top,
        opening_of,
        spire,
        core.as_deref(),
        fill,
    )?;
    let mut posts = post_steps(situation, defend, guard, survey)?;
    // The core's step goes in at route 0 as the patch's last operation, the
    // Spire's just before it, the guard's before that and the Survey post's
    // before that, so every pointer above them is the template's; the
    // removals are applied to the patched text, where each step stands one
    // further on for each of them.
    let core_shift = usize::from(core.is_some()).checked_add(usize::from(spire.is_some()))?;
    let front = usize::from(posts.survey.is_some());
    let (template, goals, patch_parts) = match (goals.first(), guard) {
        (Some(first), _) => {
            let template = declared
                .iter()
                .find(|held| held.template_id == first.goal.template())?;
            let shift = core_shift
                .checked_add(usize::from(posts.guard.is_some()))?
                .checked_add(front)?;
            let parts = place_patch(
                template,
                &goals,
                fill,
                held_back?.checked_add(guard_ms)?,
                shift,
            )?;
            (template, goals, parts)
        }
        (None, Some(guard)) => {
            let template = defend?;
            let goals = vec![guard.clone()];
            let parts = place_patch(
                template,
                &goals,
                fill,
                held_back?,
                core_shift.checked_add(front)?,
            )?;
            // Alone, the guard fills the Defend template itself.
            posts.guard = None;
            (template, goals, parts)
        }
        (None, None) => {
            return additions_alone(
                situation,
                tuning,
                spire,
                posts.survey,
                core.as_deref()?,
                declared,
            );
        }
    };
    let (mut patch, mut removals) = patch_parts;
    let template_id = goals.first()?.goal.template();
    let why = round_why(situation, tuning, &goals, &posts, spire, fill)?;
    patch.push(note_op(template, &with_seed(situation, &why)));
    let survey = posts.survey.as_ref().map(|(survey, _)| (*survey).clone());
    let guard = front_steps(
        &mut patch,
        &mut removals,
        posts,
        spire,
        core.as_deref(),
        core_shift,
    );
    Some(Composed {
        template_id,
        goals,
        guard,
        patch,
        removals,
        spire: spire.cloned(),
        survey,
        why,
    })
}

/// The steps in front of the template's, put in at route 0 one after another
/// so each lands ahead of the last: the Survey post's, the guard's (with the
/// removal that takes it out, last of all: the repair of last resort removes
/// the goals inserted after the first before the guard), the Spire's and the
/// core's. Answers the guard placed.
fn front_steps(
    patch: &mut Vec<Json>,
    removals: &mut Vec<Vec<Json>>,
    posts: PostSteps<'_>,
    spire: Option<&Spire>,
    core: Option<&str>,
    core_shift: usize,
) -> Option<Candidate> {
    let mut front = |step: Json| patch.push(op("add", "/declarative/route/0", Some(step)));
    if let Some((_, step)) = posts.survey {
        front(step);
    }
    let guard = posts.guard.map(|(guard, step)| {
        front(step);
        removals.insert(
            0,
            vec![op(
                "remove",
                &format!("/declarative/route/{core_shift}"),
                None,
            )],
        );
        guard
    });
    if let Some(spire) = spire {
        front(spire_step(&spire.core));
    }
    if let Some(core) = core {
        front(deepen_core(core));
    }
    guard
}

/// The Survey post's step: the Defend the Core template's own place step
/// covering the post seam by name, its label, tags and initial settings made
/// a Survey post's -- the Survey mandate with one probe area, the half of the
/// ground away from the seat's core ([`far_half`]), and [`SURVEY_SCOUT_COUNT`]
/// scouts -- so its walk, timeout and `on_fail` are the library's.
fn survey_copy(situation: &Situation, defend: &Declared, seam: &str) -> Option<Json> {
    let step = guard_copy(defend, seam)?;
    let step = with_member(&step, "label", string("survey_post"));
    let place = step.get("place_beacon")?;
    let place = with_member(place, "tags", Json::Array(vec![string("survey")]));
    let (min, max) = far_half(situation)?;
    let initial = object(vec![
        ("priority", string("NORMAL")),
        (
            "mandate",
            object(vec![(
                "survey",
                object(vec![
                    (
                        "probe_areas",
                        Json::Array(vec![object(vec![
                            ("min", voxel_json(min)),
                            ("max", voxel_json(max)),
                        ])]),
                    ),
                    ("scout_count", number(i64::from(SURVEY_SCOUT_COUNT))),
                ]),
            )]),
        ),
    ]);
    let place = with_member(&place, "initial", initial);
    Some(with_member(&step, "place_beacon", place))
}

/// The half of the ground away from the seat's own core, as `(min, max)`
/// voxels, all heights: the map is split at its middle across the axis on
/// which the core stands further from the middle (`x` on a tie), and the half
/// the core is not in is the box. Every other seat's core stands somewhere in
/// the ground, and the scouts scan unexplored cells nearest first, so a box
/// that starts across the middle reaches the enemy sooner than one that
/// starts at home (measured on the committed seeds: 3 of 12 posts find an
/// enemy beacon in their own Push, against 1 of 12 for the whole map).
/// `None` for a map size or a core position that does not fit the
/// arithmetic, which no map does.
fn far_half(situation: &Situation) -> Option<([i32; 3], [i32; 3])> {
    let [cx, cy, _] = situation.core()?.at;
    let [x, y, z] = situation.map_size;
    let max = [x.checked_sub(1)?, y.checked_sub(1)?, z.checked_sub(1)?];
    let [mx, my] = [max[0].checked_div(2)?, max[1].checked_div(2)?];
    let off = |at: i32, mid: i32| i64::from(at).checked_sub(i64::from(mid)).map(i64::abs);
    Some(if off(cx, mx)? >= off(cy, my)? {
        if cx <= mx {
            ([mx, 0, 0], max)
        } else {
            ([0, 0, 0], [mx, max[1], max[2]])
        }
    } else if cy <= my {
        ([0, my, 0], max)
    } else {
        ([0, 0, 0], [max[0], my, max[2]])
    })
}

/// Why the Survey post is placed: spec section 14's trigger, as Easy read it.
///
/// PLACEHOLDER: the wording is the operator's, in English, until the one string table exists — owner, S6
fn survey_why(survey: &Candidate, tuning: &Tuning, fill: i64) -> String {
    let seam = survey.goal.feature();
    format!(
        "A Survey beacon beside the core, covering the scrap seam {seam}, at {} as its site \
         reads now, whose {SURVEY_SCOUT_COUNT} scouts look over the far half of the map, because you hold \
         no sighting of an enemy beacon from the last {}: {} of route (Easy fills {} of the \
         segment).",
        place(survey.place),
        seconds(tuning.reach_memory_ms),
        seconds(survey.time_ms),
        seconds(fill)
    )
}

/// A round whose only additions are the Spire and the Survey post, with no
/// goal and no guard: the Safe Playbook template, its route filled with the
/// template's own walk to safety, the core's dig depth, the Spire's step and
/// the Survey post's, and its note Easy's. The safe playbook itself never
/// adds either (spec section 14: it "leaves configurations untouched", never
/// places, and a person's timeout must not spend their $), so this is Easy's
/// own plan on that template's frame, submitted and reported as its own.
fn additions_alone(
    situation: &Situation,
    tuning: &Tuning,
    spire: Option<&Spire>,
    survey: Option<(&Candidate, Json)>,
    core: &str,
    declared: &[Declared],
) -> Option<Composed> {
    if spire.is_none() && survey.is_none() {
        return None;
    }
    let template = declared
        .iter()
        .find(|held| held.template_id == SAFE_PLAYBOOK)?;
    let pointer = template.pointer_ending("/declarative/route")?;
    let Json::Array(own) = template.own_value(pointer)? else {
        return None;
    };
    let fill = fill_ms(situation)?;
    let mut route = vec![own.first()?.clone(), deepen_core(core)];
    let mut why: Vec<String> = Vec::new();
    if let Some(spire) = spire {
        route.push(spire_step(&spire.core));
        why.push(spire_why(spire, tuning, true));
    }
    if let Some((survey, step)) = &survey {
        route.push(step.clone());
        why.push(survey_why(survey, tuning, fill));
    }
    let why = why.join(" ");
    Some(Composed {
        template_id: SAFE_PLAYBOOK,
        goals: Vec::new(),
        guard: None,
        patch: vec![
            op("replace", pointer, Some(Json::Array(route))),
            note_op(template, &with_seed(situation, &why)),
        ],
        removals: Vec::new(),
        spire: spire.cloned(),
        survey: survey.map(|(survey, _)| survey.clone()),
        why,
    })
}

/// The additions weighed against one budget ([`added`]), then the goals the
/// greedy insertion fits into what they leave, less any on a post's seam.
fn weigh_and_insert<'a>(
    situation: &Situation,
    tuning: &Tuning,
    top: &[Candidate],
    opening: Opening<'a>,
    spire: Option<&'a Spire>,
    core: Option<&str>,
    fill: i64,
) -> Option<(Added<'a>, Vec<Candidate>)> {
    let weighed = added(
        situation,
        tuning,
        Weighed {
            fill,
            core,
            lead: opening.lead,
            guard: opening.guard,
            spire,
            survey: opening.survey,
        },
    )?;
    let top = beside_the_posts(top, weighed.guard, weighed.survey);
    let goals = greedy(&top, weighed.share, weighed.treasury);
    Some((weighed, goals))
}

/// The posts a round can add, and the walk to the core.
#[derive(Clone, Copy)]
struct Opening<'a> {
    guard: Option<&'a Candidate>,
    survey: Option<&'a Candidate>,
    /// The walk to the core: one estimate's first leg, the same for every
    /// candidate, the largest taken so no reading of it is under-held; `None`
    /// with no candidate, a round composed only for the Spire, whose step
    /// stands at the core the route opens at.
    lead: Option<i64>,
}

/// The guard's step and the Survey post's (its mandate made Survey's) are the
/// Defend the Core template's place step, so without it there is neither. The
/// Survey post yields to a raid worth placing (a commander in sight, whose
/// raid post would stand on the post seam): a target already in view is what
/// the post is for. It yields to the guard in [`added`].
fn opening<'a>(
    top: &[Candidate],
    guard: Option<&'a Candidate>,
    survey: Option<&'a Candidate>,
    defend: bool,
) -> Opening<'a> {
    let guard = guard.filter(|_| defend);
    let raid = top
        .iter()
        .any(|candidate| matches!(candidate.goal, Goal::Raid { .. }));
    let survey = survey.filter(|_| defend && !raid);
    let lead = top
        .iter()
        .chain(guard)
        .chain(survey)
        .map(|candidate| candidate.lead_ms)
        .max();
    Opening {
        guard,
        survey,
        lead,
    }
}

/// What the additions beside the goals leave the greedy insertion.
struct Added<'a> {
    /// The guard post that is placed, if one is.
    guard: Option<&'a Candidate>,
    /// Its time, 0 without one.
    guard_ms: i64,
    /// The Spire that is queued, if one is.
    spire: Option<&'a Spire>,
    /// The Survey post that is placed, if one is.
    survey: Option<&'a Candidate>,
    /// The share left for the goals.
    share: i64,
    /// The treasury left for the goals.
    treasury: i64,
    /// What the hold gives up for what stands in front of the template's
    /// steps, the guard's aside: the core's step, the Spire's and the Survey
    /// post's, and the walk to the core; `None` only when there is no
    /// candidate, a round composed for the Spire alone, which has no hold to
    /// take it from.
    held_back: Option<i64>,
}

/// What [`added`] weighs.
#[derive(Clone, Copy)]
struct Weighed<'a, 'c> {
    fill: i64,
    core: Option<&'c str>,
    lead: Option<i64>,
    guard: Option<&'a Candidate>,
    spire: Option<&'a Spire>,
    survey: Option<&'a Candidate>,
}

/// What the additions spend, in the order they claim it.
struct Purse {
    share: i64,
    treasury: i64,
    headroom: i64,
}

impl Purse {
    /// Spend `time`, `dollars` and `kw` when all three fit, and say whether
    /// they did; a figure that does not fit is not spent.
    fn spend(&mut self, time: i64, dollars: i64, kw: i64) -> bool {
        match (
            self.share.checked_sub(time),
            self.treasury.checked_sub(dollars),
            self.headroom.checked_sub(kw),
        ) {
            (Some(share), Some(treasury), Some(headroom))
                if share >= 0 && treasury >= 0 && headroom >= 0 =>
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

/// The guard post, the Spire and the Survey post, weighed before the goals,
/// in that order, against **one** budget: the share, the treasury and the kW
/// headroom (the forecast's supply less its draw). Each takes its time, price
/// and new draw from what the one before left, or is not added: the guard's
/// picket scout, the Spire's own draw and the Survey post's scouts, each from
/// its rules rows, so the round never fields more draw than the grid carries
/// (a unit fielded past the supply is the verifier's E0601). The Spire's time
/// comes out of the hold, as the core's step does, rather than the share; the
/// Survey post yields its seam to the guard. Its step is at the core the route
/// opens at, so a seat shown no core queues no Spire. `None` when a figure
/// does not fit.
fn added<'a>(
    situation: &Situation,
    tuning: &Tuning,
    weighed: Weighed<'a, '_>,
) -> Option<Added<'a>> {
    let Weighed {
        fill,
        core,
        lead,
        guard,
        spire,
        survey,
    } = weighed;
    let mut purse = Purse {
        share: fill,
        treasury: situation.economy.treasury,
        headroom: situation
            .economy
            .supply_kw
            .checked_sub(situation.economy.draw_kw)?,
    };
    let guard =
        guard.filter(|guard| purse.spend(guard.time_ms, guard.cost_dollars, tuning.guard_draw_kw));
    let guard_ms = time_of(guard);
    let spire = spire
        .filter(|spire| core == Some(spire.core.as_str()))
        .filter(|spire| purse.spend(0, spire.cost_dollars, spire.draw_kw));
    let survey = survey
        .filter(|survey| guard.is_none_or(|guard| guard.goal.feature() != survey.goal.feature()))
        .filter(|survey| purse.spend(survey.time_ms, survey.cost_dollars, tuning.survey_draw_kw));
    let held_back = match (core, lead) {
        (None, _) => Some(0),
        (Some(core), Some(lead)) => {
            let deepen = crate::price::step_ms(&deepen_core(core), tuning)?;
            let spire_ms = match spire {
                Some(spire) => spire.time_ms,
                None => 0,
            };
            let survey_ms = time_of(survey);
            deepen
                .checked_add(lead)?
                .checked_add(spire_ms)?
                .checked_add(survey_ms)
        }
        (Some(_), None) => None,
    };
    Some(Added {
        guard,
        guard_ms,
        spire,
        survey,
        share: purse.share,
        treasury: purse.treasury,
        held_back,
    })
}

/// A post's time on the route, and none without a post.
const fn time_of(post: Option<&Candidate>) -> i64 {
    match post {
        Some(post) => post.time_ms,
        None => 0,
    }
}

/// The guard post's and the Survey post's steps, each copied from the Defend
/// the Core template's place step. A guard's step is used only beside another
/// template's goals (alone, it fills the Defend template itself).
struct PostSteps<'c> {
    guard: Option<(Candidate, Json)>,
    survey: Option<(&'c Candidate, Json)>,
}

/// The posts' steps; `None` when a copy does not build.
fn post_steps<'c>(
    situation: &Situation,
    defend: Option<&Declared>,
    guard: Option<&Candidate>,
    survey: Option<&'c Candidate>,
) -> Option<PostSteps<'c>> {
    let guard = match (guard, defend) {
        (Some(guard), Some(defend)) => {
            Some((guard.clone(), guard_copy(defend, guard.goal.feature())?))
        }
        _ => None,
    };
    let survey = match (survey, defend) {
        (Some(survey), Some(defend)) => Some((
            survey,
            survey_copy(situation, defend, survey.goal.feature())?,
        )),
        _ => None,
    };
    Some(PostSteps { guard, survey })
}

/// The step a goal is composed as, for pricing it ([`crate::price`]): the
/// declared template's place step naming the goal's feature (a Mine beacon,
/// a Generator's Build beacon, the guard post), the Raid template's with its
/// target, its staging at `place` and no retarget, the Survey post's copy,
/// a tap's `interface` step, or a stone's `toward` placement with Hold &
/// Build's own timeout. `None` when the template it comes from is not
/// declared.
pub(crate) fn step_for(
    goal: &Goal,
    place: [i32; 3],
    situation: &Situation,
    declared: &[Declared],
) -> Option<Json> {
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
        Goal::Stone { feature, .. } => {
            let (step, _, _) = place_step(crate::easy::HOLD_AND_BUILD)?;
            Some(stone_step(
                "stone",
                feature,
                step.get("timeout_ms")?.clone(),
            ))
        }
        Goal::Survey { seam } => survey_copy(situation, template(DEFEND_THE_CORE)?, seam),
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
    posts: &PostSteps<'_>,
    spire: Option<&Spire>,
    fill: i64,
) -> Option<String> {
    let guard = posts.guard.as_ref().map(|(guard, _)| guard);
    let survey = posts.survey.as_ref().map(|(survey, _)| *survey);
    let first = goals.first()?;
    let mut why = why_for(first, goals.len(), fill);
    if let Some(guard) = guard {
        why = format!("{why} {}", why_for(guard, 1, fill));
    }
    if guard.is_some() || first.goal.template() == DEFEND_THE_CORE {
        why = format!("{why} {}", guard_reason(situation, tuning));
    }
    if let Some(spire) = spire {
        why = format!("{why} {}", spire_why(spire, tuning, false));
    }
    if let Some(survey) = survey {
        why = format!("{why} {}", survey_why(survey, tuning, fill));
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

/// The top-k less any goal on the guard's seam or the Survey post's: a raid
/// post covers the seam the posts do, beside the core, so with a post placed
/// the two would contend for one site, and the round makes no raid.
fn beside_the_posts(
    top: &[Candidate],
    guard: Option<&Candidate>,
    survey: Option<&Candidate>,
) -> Vec<Candidate> {
    top.iter()
        .filter(|candidate| {
            guard
                .into_iter()
                .chain(survey)
                .all(|post| post.goal.feature() != candidate.goal.feature())
        })
        .cloned()
        .collect()
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
    for (k, goal) in goals.iter().enumerate() {
        let n = k.checked_add(1)?;
        let step = match &goal.goal {
            Goal::Mine { seam: name, .. }
            | Goal::Generator { vent: name, .. }
            | Goal::Guard { seam: name } => {
                if k == 0 {
                    patch.push(op("replace", &covering, Some(named(name))));
                    continue;
                }
                place_copy(&place_step, &below, name, n)?
            }
            // A Survey post is added beside the goals by its own step
            // (`survey_copy`) and is never ranked among them (`top_k`), so no
            // template is filled with one: a goal list holding one is not
            // composed.
            Goal::Survey { .. } => return None,
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
                    continue;
                }
                let mut step = place_copy(&place_step, &below, seam, n)?;
                for (pointer, value) in settings.into_iter().skip(1) {
                    step = with_pointer(&step, pointer.strip_prefix(&place_prefix)?, value)?;
                }
                step
            }
            Goal::Stone { feature, .. } | Goal::Tap { vent: feature, .. } => {
                let step = match &goal.goal {
                    Goal::Tap { beacon, .. } => {
                        tap_step(&numbered("add_generator", k, n), beacon, feature)
                    }
                    _ => stone_step(
                        &numbered("stone", k, n),
                        feature,
                        place_step.get("timeout_ms")?.clone(),
                    ),
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
/// the sim stands where the seat's spheres reach furthest toward it, with
/// the template's own place step's timeout and skipped rather than retried if
/// it finds no site (`scenarios/s3/toward-stepping-stone.playbook.jsonc`'s
/// shape).
pub(crate) fn stone_step(label: &str, feature: &str, timeout_ms: Json) -> Json {
    object(vec![
        ("label", string(label)),
        (
            "place_beacon",
            object(vec![(
                "at",
                object(vec![("toward", object(vec![("feature", named(feature))]))]),
            )]),
        ),
        ("timeout_ms", timeout_ms),
        ("on_fail", object(vec![("action", string("SKIP"))])),
    ])
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
/// and says so when the last Push's feed was not read far enough to tell
/// whether the commander died, which is why no guard post was added for it.
pub(crate) fn with_seed(situation: &Situation, why: &str) -> String {
    match situation.last_push {
        LastPush::Unread => format!(
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
        Goal::Survey { seam } => format!(
            "A Survey beacon beside the core, covering the scrap seam {seam}, at {} as its site \
             reads now, whose scouts look over the far half of the map{more}{tail}",
            place(candidate.place),
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
        LastPush::Survived | LastPush::Unread => None,
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
            LastPush::Unread => String::from(
                "No threat is in sight inside your core's sphere, and the last Push's feed was not \
                 read far enough to say whether your commander died.",
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
