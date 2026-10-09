// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Candidates, their situation features, and their utility.
//!
//! Spec section 14, steps two to four at Easy's row: integer situation
//! features; the template parameters' options, enumerated in a **total order**
//! and cut at Easy's 30 candidates; one travel estimate each through
//! `estimate_route` (its only evaluation, and the only call a candidate
//! costs); utility scored as defence + economy + expansion + capability +
//! attack, less risk, on the Balanced weighting; and the top 3 by utility per
//! second, **ties to the lowest id** (decision C15: no random draw at Easy).
//!
//! # What a candidate is
//!
//! A candidate is one goal, and every goal names its feature: Easy's row says
//! "fixed targets only", and since S1's targeting a fixed target is a **name**
//! (`docs/design/targeting.md`, its surfaces section: "the templates are rewritten with
//! descriptions ...; Easy emits names").
//!
//! * **a Mine goal** (Expand & Mine): place a Mine beacon `covering` a seam the
//!   seat's spheres do not cover yet, named `seam_<x>_<y>`. A seam the wire
//!   says is `covered` is inside one of the seat's own spheres already and is
//!   worked from there (the core's starting drone works the starting seam, a
//!   unit's kind being its job, item 127 (6)), so it offers no Mine goal:
//!   S1-16's "inside an own non-core sphere, read as mined already" is
//!   replaced by the gateway's own coverage (S1's plan, decision 14).
//! * **a Generator goal** (Hold & Build): place a Build beacon `covering` a
//!   vent the seat does not cover yet, `vent_<x>_<y>`, whose initial Build
//!   target is a Generator `on` the vent it covers.
//! * **a tap goal** (Hold & Build; the register's S1-15, decision 14): add a
//!   Generator `on` a vent the seat already covers to the Build list of an
//!   existing own beacon whose sphere holds it, through one `interface` step.
//!   Which beacon may take it is the gateway's to say, twice, before anything
//!   is estimated ([`legal_taps`]): `resolve_refs` answers whether the `on`
//!   reads from that beacon (the vent inside its sphere, no Generator of any
//!   seat on it and no Build target of the seat's already claiming it), and a
//!   QUICK `verify_plan` answers `E0503` for a beacon that is not on the Build
//!   mandate, which the wire carries nowhere else.
//!
//! **No site is computed here.** Each placement's one estimate is
//! `estimate_route` from the commander to a `covering` waypoint naming the
//! feature, and its last leg ends on the column the sim's own `cover` would
//! choose (S1-20's need, discharged by `tgtw` #84, item 133); a feature that
//! no site covers is refused there and offers nothing. The skeleton's
//! `site_for`, its expansion heuristics and its sphere margin are gone with
//! it. Nothing looks ahead: the only evaluation is the estimate, which is an
//! allowed estimate and not a dry run (AGENTS.md section 3 rule 2).
//!
//! # The terms that are zero at Easy, and why
//!
//! **Defence** (a Defend guard is section 14's "Vision and hunting" and S5's),
//! **capability** (no licence or capability is on any wire until S4) and
//! **attack** (fixed targets only; aiming at an enemy is S2's) are zero for
//! every candidate. They are in the sum, with their weights, so S2 to S5 fill
//! a term rather than change a formula. A placed beacon adds no draw (since
//! T14b a beacon is net zero through its own key-core, decisions-log item 113
//! (5)), which S1's plan, decision 14, made the reading of the register's
//! S1-14, so no goal is charged a kW shortfall.

use pharmakos_proto::json::Json;

use crate::easy::{
    BALANCED, DOLLARS_PER_KW, EASY_CANDIDATES, EASY_TOP_K, EXPANSION_POINTS_PER_BEACON,
    RISK_POINTS_PER_ENEMY,
};
use crate::situation::{Feature, Kind, Situation};
use crate::tuning::{Richness, Tuning};
use crate::wire::{Wire, array_of, bool_of, int_of, location_voxel, object, string, text_of};

/// One goal a template can be filled with.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Goal {
    /// Place a Mine beacon covering the seam `seam`.
    Mine {
        /// The seam's name, `seam_<x>_<y>`.
        seam: String,
        /// Its grade.
        grade: Richness,
    },
    /// Place a Build beacon covering the vent `vent`, whose initial Build
    /// target is a Generator on the vent it covers.
    Generator {
        /// The vent's name, `vent_<x>_<y>`.
        vent: String,
        /// Its grade.
        grade: Richness,
    },
    /// Add a Generator on the vent `vent` to the Build list of the existing
    /// own beacon `beacon` (the register's S1-15).
    Tap {
        /// The beacon's `b_NN`.
        beacon: String,
        /// The vent's name, `vent_<x>_<y>`.
        vent: String,
        /// Its grade.
        grade: Richness,
    },
}

impl Goal {
    /// The template this goal fills.
    pub(crate) const fn template(&self) -> &'static str {
        match self {
            Goal::Mine { .. } => crate::easy::EXPAND_AND_MINE,
            Goal::Generator { .. } | Goal::Tap { .. } => crate::easy::HOLD_AND_BUILD,
        }
    }

    /// The feature the goal is about, by name: one goal per feature in a
    /// composed route.
    pub(crate) fn feature(&self) -> &str {
        match self {
            Goal::Mine { seam, .. } => seam,
            Goal::Generator { vent, .. } | Goal::Tap { vent, .. } => vent,
        }
    }
}

/// One evaluated candidate.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Candidate {
    /// Its place in the enumeration's total order: the tie-breaker.
    pub(crate) id: usize,
    /// The goal.
    pub(crate) goal: Goal,
    /// Where the commander goes for it: the site the estimate answered for a
    /// placement, the beacon for a tap.
    pub(crate) place: [i32; 3],
    /// Game milliseconds the goal costs the route: travel, bounded as there
    /// and back for a site the commander leaves again, plus the interface
    /// time the rules table gives the change.
    pub(crate) time_ms: i64,
    /// Whole $ it spends.
    pub(crate) cost_dollars: i64,
    /// Balanced utility, in points.
    pub(crate) utility: i64,
    /// Utility per second of `time_ms`, times 1 000.
    pub(crate) rate: i64,
}

/// The whole-voxel squared distance between two voxels, or `None` when it
/// does not fit: a distance that large is within no radius.
pub(crate) fn distance2(a: [i32; 3], b: [i32; 3]) -> Option<i64> {
    let square = |p: i32, q: i32| {
        let d = i64::from(p).checked_sub(i64::from(q))?;
        d.checked_mul(d)
    };
    let ([ax, ay, az], [bx, by, bz]) = (a, b);
    square(ax, bx)?
        .checked_add(square(ay, by)?)?
        .checked_add(square(az, bz)?)
}

/// True when `at` lies within `radius` of `centre`, by squared distance
/// (AGENTS.md section 4.2: no square root for a range check).
fn within(centre: [i32; 3], at: [i32; 3], radius: i64) -> bool {
    match (distance2(centre, at), radius.checked_mul(radius)) {
        (Some(d2), Some(r2)) => d2 <= r2,
        _ => false,
    }
}

/// A tap the gateway has said is legal: the vent's Generator may go on this
/// beacon's Build list.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Tap {
    /// The beacon's `b_NN`.
    pub(crate) beacon: String,
    /// The vent, as the feature list reads it.
    pub(crate) vent: Feature,
}

/// The `(beacon, vent)` pairs a tap could be: every live vent the seat
/// covers, beside every own lit beacon whose ground-plane distance to the
/// vent's anchor is within the sphere radius (a necessary condition for the
/// sphere to hold it; the gateway answers the rest), in the order of the
/// vent's travel from the commander, then the vent's name, then the beacon's,
/// cut at Easy's candidate count. Nothing when the treasury cannot carry one.
fn tap_pairs(situation: &Situation, tuning: &Tuning) -> Vec<(String, Feature)> {
    if tuning.tap_cost_dollars > situation.economy.treasury {
        return Vec::new();
    }
    let radius = tuning.sphere_radius_voxels;
    let mut pairs: Vec<(String, Feature)> = Vec::new();
    for vent in situation
        .features
        .iter()
        .filter(|feature| feature.kind == Kind::Vent && feature.live && feature.covered)
    {
        let [x, y] = vent.anchor;
        for beacon in situation.own_beacons().filter(|beacon| beacon.powered) {
            let [bx, by, _] = beacon.at;
            if within([bx, by, 0], [x, y, 0], radius) {
                pairs.push((beacon.id.clone(), vent.clone()));
            }
        }
    }
    pairs.sort_by(|(a_beacon, a), (b_beacon, b)| {
        (a.travel_ms.is_none(), a.travel_ms, &a.id, a_beacon).cmp(&(
            b.travel_ms.is_none(),
            b.travel_ms,
            &b.id,
            b_beacon,
        ))
    });
    pairs.truncate(usize::try_from(EASY_CANDIDATES).unwrap_or(usize::MAX));
    pairs
}

/// One `interface` step adding a Generator `on` a named vent to a beacon's
/// Build list: what a tap goal writes into the route, and what the probe
/// below asks about.
pub(crate) fn tap_step(label: &str, beacon: &str, vent: &str) -> Json {
    object(vec![
        ("label", string(label)),
        (
            "interface",
            object(vec![
                ("beacon", object(vec![("beacon_id", string(beacon))])),
                (
                    "rows",
                    Json::Array(vec![object(vec![(
                        "add_build_target",
                        object(vec![(
                            "target",
                            object(vec![
                                ("blueprint_id", string("generator")),
                                (
                                    "anchor",
                                    object(vec![(
                                        "on",
                                        object(vec![("feature_id", string(vent))]),
                                    )]),
                                ),
                            ]),
                        )]),
                    )])]),
                ),
            ]),
        ),
        ("on_fail", object(vec![("action", string("SKIP"))])),
    ])
}

/// The probe playbook: one tap step per pair, in order, and nothing else the
/// gateway would need to rank. It is asked about and never submitted.
fn probe_text(pairs: &[(String, Feature)]) -> String {
    let route: Vec<Json> = pairs
        .iter()
        .enumerate()
        .map(|(index, (beacon, vent))| tap_step(&format!("probe_{index}"), beacon, &vent.id))
        .collect();
    let probe = object(vec![
        (
            "schema_version",
            object(vec![
                ("major", Json::Number(String::from("1"))),
                ("minor", Json::Number(String::from("0"))),
            ]),
        ),
        (
            "meta",
            object(vec![
                ("title", string("Easy's tap probe")),
                ("author_kind", string("BUILTIN")),
            ]),
        ),
        ("kind", string("PLAYBOOK")),
        ("declarative", object(vec![("route", Json::Array(route))])),
        ("on_death", object(vec![("on_respawn", string("CONTINUE"))])),
        (
            "fallback",
            object(vec![(
                "hold",
                object(vec![("at", object(vec![("safest", object(vec![]))]))]),
            )]),
        ),
    ]);
    crate::wire::compact(&probe)
}

/// The route index a pointer into a route step names:
/// `/declarative/route/3/...` is 3.
pub(crate) fn route_index(pointer: &str) -> Option<usize> {
    pointer
        .strip_prefix("/declarative/route/")?
        .split('/')
        .next()?
        .parse::<usize>()
        .ok()
}

/// The taps the gateway says are legal, at most two calls: one
/// `resolve_refs` and one QUICK `verify_plan` over a probe playbook holding
/// every candidate pair, and none when there is no pair.
///
/// A pair is legal when its `on` reads the vent it names from that beacon
/// (`resolve_refs`: the sim's own `on_vent` rules) and the verifier raises no
/// `E0503` at its step (the beacon is on the Build mandate). A refusal of
/// either call leaves no tap: the operator cannot then say a tap would hold,
/// so it composes none.
pub(crate) fn legal_taps(
    wire: &mut Wire<'_, '_>,
    situation: &Situation,
    tuning: &Tuning,
) -> Vec<Tap> {
    let pairs = tap_pairs(situation, tuning);
    if pairs.is_empty() {
        return Vec::new();
    }
    let text = probe_text(&pairs);
    let Ok(resolved) = wire.call(
        "resolve_refs",
        object(vec![("playbook_jsonc", string(&text))]),
    ) else {
        return Vec::new();
    };
    let mut legal = vec![false; pairs.len()];
    for reference in array_of(&resolved, "refs") {
        let Some(index) = route_index(text_of(reference, "pointer")) else {
            continue;
        };
        let reads = text_of(reference, "feature_id");
        if let (Some(slot), Some((_, vent))) = (legal.get_mut(index), pairs.get(index)) {
            *slot =
                !reads.is_empty() && reads == vent.id && text_of(reference, "failure").is_empty();
        }
    }
    let Ok(verified) = wire.call(
        "verify_plan",
        object(vec![
            ("playbook_jsonc", string(&text)),
            ("depth", string("quick")),
        ]),
    ) else {
        return Vec::new();
    };
    let report = verified.get("report").cloned().unwrap_or(Json::Null);
    for diagnostic in array_of(&report, "diagnostics") {
        if text_of(diagnostic, "code") != "E0503" {
            continue;
        }
        if let Some(slot) =
            route_index(text_of(diagnostic, "path")).and_then(|index| legal.get_mut(index))
        {
            *slot = false;
        }
    }
    pairs
        .into_iter()
        .zip(legal)
        .filter(|(_, legal)| *legal)
        .map(|((beacon, vent), _)| Tap { beacon, vent })
        .collect()
}

/// The goals the map offers this seat, before any estimate: live, affordable,
/// reachable, and in a total order -- the feature's travel from the commander
/// (the feature list's own "nearest"), then the feature's name, then a tap's
/// beacon -- cut at Easy's candidate count.
pub(crate) fn enumerate(situation: &Situation, tuning: &Tuning, taps: &[Tap]) -> Vec<Goal> {
    if situation.commander.is_none() {
        return Vec::new();
    }
    let treasury = situation.economy.treasury;
    let mut keyed: Vec<((bool, i64, String, String), Goal)> = Vec::new();
    for feature in &situation.features {
        let Some(travel) = feature.travel_ms else {
            continue;
        };
        if !feature.live || feature.covered {
            continue;
        }
        let goal = match feature.kind {
            Kind::Seam if tuning.beacon_cost_dollars <= treasury => Goal::Mine {
                seam: feature.id.clone(),
                grade: feature.grade,
            },
            Kind::Vent if tuning.vent_cost_dollars <= treasury => Goal::Generator {
                vent: feature.id.clone(),
                grade: feature.grade,
            },
            Kind::Seam | Kind::Vent => continue,
        };
        keyed.push(((false, travel, feature.id.clone(), String::new()), goal));
    }
    for tap in taps {
        // A vent the commander has no route to still has a beacon that may
        // reach it: such a tap sorts after every travel that was answered.
        let (unreached, travel) = match tap.vent.travel_ms {
            Some(travel) => (false, travel),
            None => (true, 0),
        };
        keyed.push((
            (unreached, travel, tap.vent.id.clone(), tap.beacon.clone()),
            Goal::Tap {
                beacon: tap.beacon.clone(),
                vent: tap.vent.id.clone(),
                grade: tap.vent.grade,
            },
        ));
    }
    keyed.sort_by(|(a, _), (b, _)| a.cmp(b));
    keyed.truncate(usize::try_from(EASY_CANDIDATES).unwrap_or(usize::MAX));
    keyed.into_iter().map(|(_, goal)| goal).collect()
}

/// Why Hold & Build has nothing to place this round, said about the vent
/// the template's own description would read: the nearest by travel that the
/// seat does not cover, ties to the lowest name.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum NoVent {
    /// The seat's commander is not in view, so nothing was weighed.
    NoCommander,
    /// Every live vent the seat can reach is covered by its spheres already.
    AllCovered,
    /// No live vent is reachable from the commander.
    NoneReachable,
    /// The treasury holds less than the route's price (the tuning's
    /// `vent_cost_dollars`).
    Poor {
        /// The vent's name.
        vent: String,
        /// The route's price, whole $.
        price: i64,
        /// The treasury, whole $.
        treasury: i64,
    },
    /// It was a goal, and its estimate or its worth dropped it.
    Dropped(String),
}

/// Why no vent is a goal worth filling Hold & Build with.
pub(crate) fn no_vent(situation: &Situation, tuning: &Tuning) -> NoVent {
    if situation.commander.is_none() {
        return NoVent::NoCommander;
    }
    let reachable: Vec<&Feature> = situation
        .features
        .iter()
        .filter(|feature| feature.kind == Kind::Vent && feature.live)
        .filter(|feature| feature.travel_ms.is_some())
        .collect();
    if reachable.is_empty() {
        return NoVent::NoneReachable;
    }
    let Some(nearest) = reachable
        .iter()
        .filter(|feature| !feature.covered)
        .min_by(|a, b| (a.travel_ms, &a.id).cmp(&(b.travel_ms, &b.id)))
    else {
        return NoVent::AllCovered;
    };
    let treasury = situation.economy.treasury;
    let price = tuning.vent_cost_dollars;
    if price <= treasury {
        NoVent::Dropped(nearest.id.clone())
    } else {
        NoVent::Poor {
            vent: nearest.id.clone(),
            price,
            treasury,
        }
    }
}

/// The place an `estimate_route` answer's last leg ends on, as a voxel.
fn last_leg_to(estimate: &Json) -> Option<[i32; 3]> {
    array_of(estimate, "legs")
        .last()
        .and_then(|leg| leg.get("to"))
        .and_then(location_voxel)
}

/// Evaluate every goal: one `estimate_route` each from the commander, then
/// the Balanced utility. A goal the estimate refuses or calls unreachable is
/// dropped; the call still counts.
pub(crate) fn evaluate(
    wire: &mut Wire<'_, '_>,
    situation: &Situation,
    tuning: &Tuning,
    goals: Vec<Goal>,
) -> Vec<Candidate> {
    let Some(commander) = situation.commander else {
        return Vec::new();
    };
    let mut out: Vec<Candidate> = Vec::new();
    for (id, goal) in goals.into_iter().enumerate() {
        let to = match &goal {
            // The site the beacon would stand on, chosen by the sim's own
            // `cover` (the register's S1-20): the last leg ends there.
            Goal::Mine { seam: name, .. } | Goal::Generator { vent: name, .. } => object(vec![(
                "covering",
                object(vec![("feature_id", string(name))]),
            )]),
            Goal::Tap { beacon, .. } => object(vec![(
                "beacon_anchor",
                object(vec![("beacon_id", string(beacon))]),
            )]),
        };
        let params = object(vec![(
            "waypoints",
            Json::Array(vec![crate::wire::voxel_location(commander), to]),
        )]);
        let Ok(estimate) = wire.call("estimate_route", params) else {
            continue;
        };
        if !bool_of(&estimate, "reachable") {
            continue;
        }
        let Some(place) = last_leg_to(&estimate) else {
            continue;
        };
        let travel = int_of(&estimate, "ms");
        if travel < 0 {
            continue;
        }
        if let Some(candidate) = score(situation, tuning, id, goal, place, travel) {
            out.push(candidate);
        }
    }
    out
}

/// One goal's features and Balanced utility; `None` when a figure does not
/// fit an `i64`, which no rules table the operator reads can produce.
fn score(
    situation: &Situation,
    tuning: &Tuning,
    id: usize,
    goal: Goal,
    place: [i32; 3],
    travel_ms: i64,
) -> Option<Candidate> {
    let (cost, economy, expansion, time_ms) = match &goal {
        Goal::Mine { grade, .. } => {
            let cost = tuning.beacon_cost_dollars;
            let ore = tuning.ore_yield(*grade).checked_mul(tuning.seam_voxels)?;
            (
                cost,
                ore.checked_sub(cost)?,
                EXPANSION_POINTS_PER_BEACON,
                // There, place, and back: the template's route returns home.
                // PLACEHOLDER: the way back costed as the way there (travel x 2), not estimated — owner, S5, with Normal and Hard
                travel_ms
                    .checked_mul(2)?
                    .checked_add(tuning.place_beacon_deploy_ms)?,
            )
        }
        Goal::Generator { grade, .. } => {
            let cost = tuning.vent_cost_dollars;
            let power = tuning.generator_kw(*grade).checked_mul(DOLLARS_PER_KW)?;
            (
                cost,
                power.checked_sub(cost)?,
                EXPANSION_POINTS_PER_BEACON,
                // There, place, and commit the initial Build target -- its own
                // row since decision 15 -- on site; the commander then holds
                // by the new beacon while its drone builds.
                travel_ms
                    .checked_add(tuning.place_beacon_deploy_ms)?
                    .checked_add(tuning.build_target_ms)?,
            )
        }
        Goal::Tap { grade, .. } => {
            let cost = tuning.tap_cost_dollars;
            let power = tuning.generator_kw(*grade).checked_mul(DOLLARS_PER_KW)?;
            (
                cost,
                power.checked_sub(cost)?,
                // No new beacon, so no new sphere.
                0,
                travel_ms.checked_add(tuning.build_target_ms)?,
            )
        }
    };
    // Risk: what the seat can see of other seats near the goal.
    //
    // PLACEHOLDER: "near" is within one sphere radius of the goal, the sphere radius reused as a threat radius — owner, S5, with the Balanced weights
    let near = situation
        .enemies
        .iter()
        .filter(|enemy| within(place, **enemy, tuning.sphere_radius_voxels))
        .count();
    let risk = i64::try_from(near)
        .ok()?
        .checked_mul(RISK_POINTS_PER_ENEMY)?;
    // Defence, capability and attack are zero at Easy (module docs).
    let (defence, capability, attack) = (0_i64, 0_i64, 0_i64);
    let w = BALANCED;
    let utility = w
        .defence
        .checked_mul(defence)?
        .checked_add(w.economy.checked_mul(economy)?)?
        .checked_add(w.expansion.checked_mul(expansion)?)?
        .checked_add(w.capability.checked_mul(capability)?)?
        .checked_add(w.attack.checked_mul(attack)?)?
        .checked_sub(w.risk.checked_mul(risk)?)?;
    let rate = utility.checked_mul(1_000)?.checked_div(time_ms.max(1))?;
    Some(Candidate {
        id,
        goal,
        place,
        time_ms,
        cost_dollars: cost,
        utility,
        rate,
    })
}

/// The top-k by utility per second, ties to the lowest id, less any
/// candidate worth nothing.
pub(crate) fn top_k(candidates: &[Candidate]) -> Vec<Candidate> {
    let mut ranked: Vec<Candidate> = candidates
        .iter()
        .filter(|candidate| candidate.utility > 0)
        .cloned()
        .collect();
    ranked.sort_by_key(|candidate| (std::cmp::Reverse(candidate.rate), candidate.id));
    ranked.truncate(EASY_TOP_K);
    ranked
}

/// The best placement for one template among all that were evaluated, ties
/// to the lowest id: what the wizard is pre-filled with. A tap is not a
/// placement and no template parameter can say one, so it is never offered
/// to the wizard.
pub(crate) fn best_for<'c>(candidates: &'c [Candidate], template: &str) -> Option<&'c Candidate> {
    candidates
        .iter()
        .filter(|candidate| candidate.utility > 0 && candidate.goal.template() == template)
        .filter(|candidate| !matches!(candidate.goal, Goal::Tap { .. }))
        .min_by_key(|candidate| (std::cmp::Reverse(candidate.rate), candidate.id))
}
