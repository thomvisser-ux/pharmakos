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
//! # What a candidate is at the skeleton
//!
//! Easy fills two templates' goals, and a candidate is one goal:
//!
//! * **a Mine site** (Expand & Mine): a place beside an ore seam where a
//!   Mine beacon may be placed -- inside one of the seat's own spheres, so the
//!   placement is legal -- whose sphere then holds the seam. A seam already
//!   inside the sphere of one of the seat's own non-core beacons is taken to
//!   be mined already and offers nothing. A seam just outside every own
//!   sphere is still reachable by one **expansion**: the site is then on the
//!   sphere's edge on the line towards it.
//! * **a Generator anchor** (Hold & Build): a heat vent inside the seat's
//!   **core**'s sphere with no own Generator on it. Hold & Build visits the
//!   safest own beacon, which is the core while every beacon is whole, and a
//!   target outside the visited beacon's sphere builds nothing; the wire
//!   carries no beacon hit points, so "the core" is the reading an ordinary
//!   client can make.
//!
//! Every target is a fixed voxel or a fixed own `b_NN` (Easy's row: "fixed
//! targets only"), and nothing looks ahead: the only evaluation is the
//! estimate, which is an allowed estimate and not a dry run (AGENTS.md
//! section 3 rule 2).
//!
//! # The terms that are zero at Easy, and why
//!
//! **Defence** (a Defend guard is section 14's "Vision and hunting" and S5's),
//! **capability** (no licence or capability is on any wire until S4) and
//! **attack** (fixed targets only; aiming at an enemy is S2's) are zero for
//! every candidate the skeleton can make. They are in the sum, with their
//! weights, so S2 to S5 fill a term rather than change a formula.

use crate::easy::{
    BALANCED, DOLLARS_PER_KW, EASY_CANDIDATES, EASY_TOP_K, EXPANSION_POINTS_PER_BEACON,
    RISK_POINTS_PER_ENEMY, SPHERE_MARGIN_VOXELS,
};
use crate::situation::Situation;
use crate::terrain::{Feature, Patch};
use crate::tuning::{Richness, Tuning};
use crate::wire::{
    Wire, array_of, bool_of, int_of, location_voxel, object, string, voxel_location,
};
use pharmakos_proto::json::Json;

/// One goal a template can be filled with.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Goal {
    /// Place a Mine beacon at `site`, beside the seam that stands at `seam`.
    Mine {
        /// Where the beacon goes, and where the commander walks to place it.
        site: [i32; 3],
        /// The seam's centre column.
        seam: [i32; 3],
        /// The seam's grade.
        richness: Richness,
        /// True when the seam is outside every own sphere today.
        expands: bool,
    },
    /// Add a Generator on the vent at `anchor` to the core's Build list.
    Generator {
        /// The vent's centre column, where the Generator stands.
        anchor: [i32; 3],
        /// The vent's grade.
        richness: Richness,
        /// The core it is built from, `b_NN`.
        core: String,
    },
}

impl Goal {
    /// The template this goal fills.
    pub(crate) const fn template(&self) -> &'static str {
        match self {
            Goal::Mine { .. } => crate::easy::EXPAND_AND_MINE,
            Goal::Generator { .. } => crate::easy::HOLD_AND_BUILD,
        }
    }

    /// Where the goal is, for the total order and the "why".
    pub(crate) const fn at(&self) -> [i32; 3] {
        match self {
            Goal::Mine { site, .. } => *site,
            Goal::Generator { anchor, .. } => *anchor,
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

/// The whole-voxel squared distance between two voxels.
pub(crate) fn distance2(a: [i32; 3], b: [i32; 3]) -> i64 {
    let square = |p: i32, q: i32| {
        let d = i64::from(p).saturating_sub(i64::from(q));
        d.saturating_mul(d)
    };
    let ([ax, ay, az], [bx, by, bz]) = (a, b);
    square(ax, bx)
        .saturating_add(square(ay, by))
        .saturating_add(square(az, bz))
}

/// The squared distance in the ground plane.
pub(crate) fn distance2_xy(a: [i32; 3], b: [i32; 3]) -> i64 {
    distance2([a[0], a[1], 0], [b[0], b[1], 0])
}

/// True when `at` is inside a sphere of radius `radius` less the margin
/// around `centre`, by squared distance (AGENTS.md section 4.2: no square
/// root for a range check).
fn inside(centre: [i32; 3], at: [i32; 3], radius: i64) -> bool {
    let reach = radius.saturating_sub(SPHERE_MARGIN_VOXELS).max(0);
    distance2(centre, at) <= reach.saturating_mul(reach)
}

/// The goals the map offers this seat, before any call: legal, affordable,
/// and in a total order, nearest the commander first, cut at Easy's
/// candidate count.
pub(crate) fn enumerate(situation: &Situation, tuning: &Tuning) -> Vec<Goal> {
    let Some(commander) = situation.commander else {
        return Vec::new();
    };
    let radius = tuning.sphere_radius_voxels;
    let treasury = situation.economy.treasury;
    let own: Vec<&crate::situation::Beacon> = situation.own_beacons().collect();
    let mut goals: Vec<Goal> = Vec::new();
    for patch in situation.terrain.patches() {
        match patch.feature {
            Feature::Vent => {
                if tuning.generator_cost_dollars > treasury {
                    continue;
                }
                // PLACEHOLDER: a Generator goal is a vent inside the core's
                // sphere only, the core standing in for "the safest beacon"
                // Hold & Build visits (the wire carries no beacon hit points
                // and no grid). Owner, at **S1**, with the grid on the wire.
                let Some(core) = situation.core() else {
                    continue;
                };
                let tapped = situation.own_generators.iter().any(|generator| {
                    patch
                        .columns
                        .iter()
                        .any(|column| column[0] == generator[0] && column[1] == generator[1])
                });
                if tapped || !inside(core.at, patch.centre, radius) {
                    continue;
                }
                goals.push(Goal::Generator {
                    anchor: patch.centre,
                    richness: patch.richness,
                    core: core.id.clone(),
                });
            }
            Feature::Seam => {
                if tuning.beacon_cost_dollars > treasury {
                    continue;
                }
                if let Some(goal) = mine_site(situation, &own, &patch, radius) {
                    goals.push(goal);
                }
            }
        }
    }
    goals.sort_by_key(|goal| {
        let at = goal.at();
        (distance2_xy(commander, at), at, goal.template())
    });
    goals.dedup_by(|a, b| a.at() == b.at());
    goals.truncate(usize::try_from(EASY_CANDIDATES).unwrap_or(usize::MAX));
    goals
}

/// The Mine site for one seam, if the seat can place one there.
fn mine_site(
    situation: &Situation,
    own: &[&crate::situation::Beacon],
    patch: &Patch,
    radius: i64,
) -> Option<Goal> {
    // Already worked: a seam inside one of the seat's own non-core beacons'
    // spheres. (A non-core beacon at the skeleton is one a Mine goal
    // placed; BeaconSummary carries no mandate to say so for certain.)
    //
    // PLACEHOLDER: "inside an own non-core sphere" read as "mined already".
    // Owner, at **S1**, when a beacon's mandate is on the wire.
    if own
        .iter()
        .any(|beacon| !beacon.core && inside(beacon.at, patch.centre, radius))
    {
        return None;
    }
    // The own beacon nearest the seam, ties to the lowest id (`own` is
    // ascending by id and `min_by_key` keeps the first of equals).
    let nearest = own
        .iter()
        .min_by_key(|beacon| distance2_xy(beacon.at, patch.centre))?;
    let terrain = &situation.terrain;
    if own
        .iter()
        .any(|beacon| inside(beacon.at, patch.centre, radius))
    {
        // Inside a sphere today: beside the seam, one column out from its
        // edge nearest that beacon.
        let edge = patch
            .columns
            .iter()
            .min_by_key(|column| (distance2_xy(nearest.at, **column), column[0], column[1]))?;
        let step = |to: i32, from: i32| to.saturating_sub(from).signum();
        let x = edge[0].saturating_add(step(nearest.at[0], edge[0]));
        let y = edge[1].saturating_add(step(nearest.at[1], edge[1]));
        let site = terrain.stand(x, y)?;
        if !own.iter().any(|beacon| inside(beacon.at, site, radius)) {
            return None;
        }
        return Some(Goal::Mine {
            site,
            seam: patch.centre,
            richness: patch.richness,
            expands: false,
        });
    }
    // Outside every sphere: one expansion, on the nearest sphere's edge
    // towards the seam, when the seam is then inside the new sphere.
    //
    // PLACEHOLDER: one expansion at most, so a seam further than twice the
    // reach from every own beacon offers nothing. Owner, at **S5**.
    let reach = radius.saturating_sub(SPHERE_MARGIN_VOXELS).max(0);
    let apart = distance2_xy(nearest.at, patch.centre).isqrt();
    if apart == 0 || apart > reach.saturating_mul(2) {
        return None;
    }
    let along = |from: i32, to: i32| -> Option<i32> {
        let delta = i64::from(to).saturating_sub(i64::from(from));
        let moved = delta.saturating_mul(reach).checked_div(apart)?;
        i32::try_from(i64::from(from).saturating_add(moved)).ok()
    };
    let x = along(nearest.at[0], patch.centre[0])?;
    let y = along(nearest.at[1], patch.centre[1])?;
    let site = terrain.stand(x, y)?;
    if !inside(nearest.at, site, radius) || !inside(site, patch.centre, radius) {
        return None;
    }
    Some(Goal::Mine {
        site,
        seam: patch.centre,
        richness: patch.richness,
        expands: true,
    })
}

/// Evaluate every goal: one `estimate_route` each from the commander, then
/// the Balanced utility. A goal the estimate calls unreachable, or that a
/// refusal left unestimated, is dropped; the call still counts.
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
        let destination = match &goal {
            Goal::Mine { site, .. } => voxel_location(*site),
            // Hold & Build walks to the beacon and changes it on site; the
            // drones walk to the vent.
            Goal::Generator { core, .. } => object(vec![(
                "beacon_anchor",
                object(vec![("beacon_id", string(core))]),
            )]),
        };
        let params = object(vec![(
            "waypoints",
            Json::Array(vec![voxel_location(commander), destination]),
        )]);
        let Ok(estimate) = wire.call("estimate_route", params) else {
            continue;
        };
        if !bool_of(&estimate, "reachable") {
            continue;
        }
        // A site the estimate's last leg does not end on -- a column the
        // estimator would not stand a walker in -- is not a site. `Leg.to`
        // is read in the declared shape, a `gp.v1.Location`, and in the bare
        // voxel `main` answered before T17 fixed it.
        if let Goal::Mine { site, .. } = &goal {
            let ends = array_of(&estimate, "legs")
                .last()
                .and_then(|leg| leg.get("to"))
                .and_then(location_voxel);
            if ends.is_some_and(|to| to[0] != site[0] || to[1] != site[1]) {
                continue;
            }
        }
        let travel = int_of(&estimate, "ms").max(0);
        if let Some(candidate) = score(situation, tuning, id, goal, travel) {
            out.push(candidate);
        }
    }
    out
}

/// One goal's features and Balanced utility.
fn score(
    situation: &Situation,
    tuning: &Tuning,
    id: usize,
    goal: Goal,
    travel_ms: i64,
) -> Option<Candidate> {
    let radius = tuning.sphere_radius_voxels;
    let headroom = situation.economy.headroom_kw;
    let (cost, economy, expansion, added_draw_kw, time_ms) = match &goal {
        Goal::Mine { richness, .. } => {
            let cost = tuning.beacon_cost_dollars;
            let ore = tuning
                .ore_yield(*richness)
                .saturating_mul(tuning.seam_voxels);
            (
                cost,
                ore.saturating_sub(cost),
                EXPANSION_POINTS_PER_BEACON,
                tuning.beacon_draw_kw,
                // There, place, and back: the route returns home.
                // PLACEHOLDER: the way back is costed as the way there
                // (travel x 2) rather than estimated. Owner, at **S5**.
                travel_ms
                    .saturating_mul(2)
                    .saturating_add(tuning.place_beacon_deploy_ms),
            )
        }
        Goal::Generator { richness, .. } => {
            let cost = tuning.generator_cost_dollars;
            let power = tuning
                .generator_kw(*richness)
                .saturating_mul(DOLLARS_PER_KW);
            (
                cost,
                power.saturating_sub(cost),
                0,
                0,
                // There and change it on site; the commander then holds.
                travel_ms
                    .saturating_add(tuning.visit_handshake_ms)
                    .saturating_add(tuning.build_target_ms),
            )
        }
    };
    // Risk: what the seat can see of other seats near the goal, and any kW
    // the goal would leave the grid short by.
    //
    // PLACEHOLDER: "near" is within one sphere radius of the goal, the
    // sphere radius reused as a threat radius. Owner, at **S5**, with the
    // Balanced weights.
    let near = situation
        .enemies
        .iter()
        .filter(|enemy| inside(goal.at(), **enemy, radius))
        .count();
    let short_kw = added_draw_kw.saturating_sub(headroom).max(0);
    let risk = i64::try_from(near)
        .unwrap_or(i64::MAX)
        .saturating_mul(RISK_POINTS_PER_ENEMY)
        .saturating_add(short_kw.saturating_mul(DOLLARS_PER_KW));
    // Defence, capability and attack are zero at Easy (module docs).
    let (defence, capability, attack) = (0_i64, 0_i64, 0_i64);
    let w = BALANCED;
    let utility = w
        .defence
        .saturating_mul(defence)
        .saturating_add(w.economy.saturating_mul(economy))
        .saturating_add(w.expansion.saturating_mul(expansion))
        .saturating_add(w.capability.saturating_mul(capability))
        .saturating_add(w.attack.saturating_mul(attack))
        .saturating_sub(w.risk.saturating_mul(risk));
    let rate = utility.saturating_mul(1_000).checked_div(time_ms.max(1))?;
    Some(Candidate {
        id,
        goal,
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

/// The best candidate for one template among all that were evaluated, ties
/// to the lowest id: what the wizard is pre-filled with.
pub(crate) fn best_for<'c>(candidates: &'c [Candidate], template: &str) -> Option<&'c Candidate> {
    candidates
        .iter()
        .filter(|candidate| candidate.utility > 0 && candidate.goal.template() == template)
        .min_by_key(|candidate| (std::cmp::Reverse(candidate.rate), candidate.id))
}
