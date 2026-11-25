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
//! * **a stone goal** (Hold & Build; the register's S5-04, S3's `opr3`):
//!   place a beacon `toward` a seam or vent that no site the seat's spheres
//!   allow covers, so a later round covers it from the stone's sphere. It is
//!   no enumerated candidate of its own: a Mine or Generator goal whose
//!   `covering` estimate is refused `NOT_FOUND` is weighed once more as one
//!   ([`evaluate`]), which is how Easy reaches past its first sphere
//!   (`docs/design/targeting.md`, "toward"). It is never offered to the
//!   wizard.
//!
//! # A stone's worth, and its chain (fix passes 2 and 3)
//!
//! A stone is charged its **own** price and time, and is worth two things,
//! each measured from the known positions -- the features' anchors, the
//! seat's own beacons and each one's listed sphere radius, a new beacon's
//! sphere the rules table's base:
//!
//! * **what it buys this round**: its own sphere, at
//!   [`EXPANSION_POINTS_PER_BEACON`] as every new sphere is; every other far
//!   feature it brings **into reach** (coverable from its sphere next round),
//!   at that feature's goal's worth over the two rounds it then takes; and
//!   every other far feature it brings **nearer**, at a beacon's price for
//!   each stone that feature's chain no longer needs ([`stone_buys`]);
//! * **the goal it serves**, at that goal's worth divided by the rounds still
//!   needed -- the stones its chain still needs, at least as many as the
//!   known positions say and one a round ([`stones_needed`]), and the goal's
//!   own round -- a round's share of a worth that arrives that many rounds on.
//!
//! Its time per second is its own walk and deploy and the served goal's own
//! time, which the second half of its worth needs; the other stones' price
//! and time are not charged to it (fix pass 3, item 1: charging the whole
//! chain made every stone of a 5- to 8-stone chain worth less than nothing,
//! so a seat with $900 sealed its safe playbook). So a goal placeable now of
//! the same worth, no further than the served goal, outranks a stone toward
//! it, which buys it only a round's share and waits a round.
//!
//! A chain the match's remaining rounds cannot finish would be worth only what
//! its stones buy on their own; but the round limit is on no seat-facing
//! field the operator reads (`gp.api.v1.Status` carries the round and not the
//! limit, and the briefing says the limit in prose only), so every chain is
//! weighed as if it can finish.
//!
//! PLACEHOLDER: a chain the match's remaining rounds cannot finish, weighed as one that can, the round limit being on no seat-facing field — owner, S5, with a `Status` field
//!
//! A chain once begun is **continued** until its feature is covered or shown
//! out of reach -- a `toward` toward it refused, or a stone no round's share
//! carries or worth nothing ([`chain_feature`]): no memory is kept
//! between rounds (the operator holds none, and keeps none through the
//! gateway), so the chain is read off the view. A stone is an own beacon,
//! not the core, whose sphere holds no live feature's anchor; the feature a
//! stone was placed toward is the far feature whose gap to the seat's spheres
//! it closed the most (the sim stands a stone where the spheres reach
//! furthest toward its feature, so its own sphere's radius is all of that
//! gap closed). While a far feature is a begun chain's, only its stone is
//! weighed; a feature out of reach weighs the rest.
//!
//! **No site is computed here.** Each placement's one estimate is
//! `estimate_route` from the commander, by way of the core when the seat has
//! one ([`evaluate`]), to a `covering` waypoint naming the feature, and its
//! last leg ends on the column the sim's own `cover` would choose (S1-20's need, discharged by `tgtw` #84, item 133); a feature that
//! no site covers is refused there and offers nothing. The skeleton's
//! `site_for`, its expansion heuristics and its sphere margin are gone with
//! it. Nothing looks ahead: the only evaluation is the estimate, which is an
//! allowed estimate and not a dry run (AGENTS.md section 3 rule 2).
//!
//! # Defence, and the guard post (S2's `oprd`)
//!
//! **A guard goal** (Defend the Core): place a Defend beacon `covering` the
//! live seam the seat covers nearest its core -- its starting seam, inside
//! the core's sphere -- that protects the core and "my commander". It is
//! enumerated every round the seat can afford one, first, so the cut never
//! drops it and the wizard is offered its site. In the round after a Push in
//! which the commander died it is funded ahead of the ranking, as ruled
//! ([`guard`]; [`crate::compose`]); otherwise it is ranked with every other
//! candidate, on its threat.
//!
//! Its **defence** term is the threat model's (S2's plan, decision 12): the
//! summed threat of the other seats' assets the view shows inside the core's
//! sphere, each its weapon's damage per second from the rules rows and zero
//! if unarmed ([`core_threat`]), at [`DEFENCE_POINTS_PER_HP_PER_SECOND`]; and
//! [`COMMANDER_DIED_POINTS`] in the round after a Push in which the seat's
//! commander died (spec section 14, "adds a Defend guard on its commander when
//! the commander has died recently", which decision 35 reads as the previous
//! round's Push). Its economy is its price, negated, its expansion nothing,
//! and it is charged no risk: the enemies near it are what it guards against.
//! Every other goal's defence is zero.
//!
//! # Attack, and the raid (S2's `opra`)
//!
//! **A raid goal** (Raid an Enemy Beacon): place an Attack beacon `covering`
//! the same seam a guard post would, beside the core, whose raiders gather at
//! its site and raid one **fixed** target (Easy's row: "fixed targets only"):
//! an enemy beacon by the `e_NN` the seat's own listing gives it, with a
//! sighting inside the rules table's `verifier.reach_memory_ms`, or an enemy
//! commander the view shows now, by its seat (`enemy_commander.seat`, the
//! owner's answer of 2026-10-26, which pins one seat's commander). Spec
//! section 14: Easy "scores enemy-commander raids like any other target", so
//! both kinds are one list, ordered by ground distance from the core, ties to
//! the lowest seat, then the beacon's name, cut at [`RAID_TARGETS_MAX`] and
//! enumerated right after the guard post, so the cut never drops them
//! ([`raid_goals`]).
//!
//! Each costs one estimate, from the commander by way of the core and the
//! post's `covering` site to the target: the last leg is the raiders' march,
//! and a target no route reaches is dropped. Its **attack** term is
//! [`RAID_POINTS`], less the threat the view shows within one sphere radius
//! of the target at [`DEFENCE_POINTS_PER_HP_PER_SECOND`] -- the same model as
//! the guard's defence, so a defended target is worth less, by the same rule
//! for a beacon and a commander. Its economy is its price negated (the
//! beacon and [`crate::easy::RAID_FORCE_SIZE`] raiders), its expansion
//! nothing, and its risk the usual one near its site. A raid is ranked among
//! the round's goals like any other ([`ranked`]); every raid names the same
//! post seam, so the composition's one-goal-per-feature rule places at most
//! one a round. Every other goal's attack is zero, but the Survey post's,
//! which is the raid it buys.
//!
//! # Vision, and the Survey post (S3's `opr3`)
//!
//! **A Survey goal**: place a Survey beacon `covering` the same seam beside
//! the core, whose scouts look over the half of the ground away from the core (spec section 14: the
//! operator "places a Survey post when the enemy sightings it needs are stale
//! or missing"; S3's plan, decision 3). The sightings a raid needs are an
//! enemy beacon's inside `verifier.reach_memory_ms` ([`raid_goals`]), so the
//! trigger is that none is held ([`sightings_stale`]); then one probe asks
//! whether a Survey beacon stands beside the core already ([`survey_post`]).
//! It is enumerated with the guard, so the cut never drops it, and **ranked**
//! with every other candidate (fix pass 2, item 2). Its worth is what it buys:
//! the sightings a raid needs, so it is scored as the raid it buys -- an
//! undefended target's [`RAID_POINTS`], less the raid's price, discounted by
//! the [`crate::easy::SURVEY_RAID_ROUNDS`] its worth takes to arrive, less its
//! own price -- over its own time and the raid's (at least its own: the raid
//! post stands on the same seam). So it never outranks the round's goals of
//! real worth, the seat's first Generator among them. It and the guard both
//! cover the post seam, so a round places one of them ([`crate::compose`]).
//! The sequence is: a round places the post, its scouts sight the enemy, the
//! next Lull holds a fresh sighting, a raid is weighed and the Survey post is
//! not.
//!
//! # The term that is zero at Easy, and why
//!
//! **Capability** is zero for every candidate. It is in the sum, with its
//! weight, so a later stage fills a term rather than change a formula: no
//! capability catalogue is on a wire until S4 to put a number on it. S3's one
//! capability, the Resonance Spire, is not ranked here at all: once the seat
//! holds its licence Easy funds it ahead of the ranking, as ruled
//! ([`crate::compose::spire`]), so no capability points are invented for it. A placed beacon adds no draw (since
//! T14b a beacon is net zero through its own key-core, decisions-log item 113
//! (5)), which S1's plan, decision 14, made the reading of the register's
//! S1-14, so no goal is charged a kW shortfall.

use pharmakos_proto::json::Json;

use crate::easy::{
    BALANCED, COMMANDER_DIED_POINTS, DEFENCE_POINTS_PER_HP_PER_SECOND, DOLLARS_PER_KW,
    EASY_CANDIDATES, EASY_TOP_K, EXPANSION_POINTS_PER_BEACON, RAID_POINTS, RAID_TARGETS_MAX,
    RISK_POINTS_PER_ENEMY, STONE_TARGETS_MAX, SURVEY_RAID_ROUNDS,
};
use crate::playbook::Declared;
use crate::situation::{Asset, Feature, Kind, LastPush, Situation, UnitKind};
use crate::tuning::{Richness, Tuning};
use crate::wire::{
    Misread, Refused, Wire, array_of, bool_of, int_of, location_voxel, malformed, object, string,
    text_of,
};

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
    /// Place a Defend beacon covering the seam `seam`, beside the core, that
    /// guards the core and the commander (the Defend the Core template; spec
    /// section 14's Defend guard, decision 35).
    Guard {
        /// The seam's name, `seam_<x>_<y>`: the live seam the seat covers
        /// nearest its core, which is its starting seam.
        seam: String,
    },
    /// Place an Attack beacon covering the seam `seam`, beside the core,
    /// whose raiders raid `target` (the Raid an Enemy Beacon template).
    Raid {
        /// The seam's name: the guard post's seam, beside the core.
        seam: String,
        /// What the raiders raid.
        target: RaidTarget,
    },
    /// Place a Survey beacon covering the seam `seam`, beside the core, whose
    /// scouts look over the half of the ground away from the core (spec section 14's Survey post; S3's
    /// plan, decision 3): placed when the enemy sightings Easy needs are
    /// stale or missing, so its scouts refresh them before the next Lull.
    Survey {
        /// The seam's name: the guard post's seam, beside the core.
        seam: String,
    },
    /// Place a stepping-stone beacon `toward` the feature `feature`, which
    /// no site the seat's spheres allow covers yet (the register's S5-04;
    /// `docs/design/targeting.md`, "toward"): a step of Hold & Build, as a
    /// tap is, that adds the sphere a later round covers the feature from.
    Stone {
        /// The far feature's name, `vent_<x>_<y>` or `seam_<x>_<y>`.
        feature: String,
        /// Vent or seam.
        kind: Kind,
        /// Its grade.
        grade: Richness,
    },
}

/// A raid's fixed target.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum RaidTarget {
    /// An enemy beacon, by the `e_NN` the seat's own listing gives it.
    Beacon {
        /// Its `e_NN`.
        id: String,
        /// Where the seat last saw it.
        at: [i32; 3],
    },
    /// The commander of one enemy seat, which the view shows now.
    Commander {
        /// Its seat.
        seat: u8,
        /// Where the view shows it.
        at: [i32; 3],
    },
}

impl RaidTarget {
    /// Where it is, as the seat knows it.
    pub(crate) const fn at(&self) -> [i32; 3] {
        match self {
            RaidTarget::Beacon { at, .. } | RaidTarget::Commander { at, .. } => *at,
        }
    }

    /// The `gp.v1.AttackTarget` a playbook writes for it.
    pub(crate) fn json(&self) -> Json {
        match self {
            RaidTarget::Beacon { id, .. } => {
                object(vec![("beacon", object(vec![("beacon_id", string(id))]))])
            }
            RaidTarget::Commander { seat, .. } => object(vec![(
                "enemy_commander",
                object(vec![("seat", crate::wire::number(i64::from(*seat)))]),
            )]),
        }
    }

    /// The waypoint an estimate walks to.
    fn waypoint(&self) -> Json {
        match self {
            RaidTarget::Beacon { id, .. } => object(vec![(
                "beacon_anchor",
                object(vec![("beacon_id", string(id))]),
            )]),
            RaidTarget::Commander { at, .. } => crate::wire::voxel_location(*at),
        }
    }
}

impl Goal {
    /// The goal a stone serves: the placement at its far feature, which the
    /// stone's sphere lets a later round make. `None` for any other goal.
    pub(crate) fn served(&self) -> Option<Goal> {
        match self {
            Goal::Stone {
                feature,
                kind: Kind::Seam,
                grade,
            } => Some(Goal::Mine {
                seam: feature.clone(),
                grade: *grade,
            }),
            Goal::Stone {
                feature,
                kind: Kind::Vent,
                grade,
            } => Some(Goal::Generator {
                vent: feature.clone(),
                grade: *grade,
            }),
            Goal::Mine { .. }
            | Goal::Generator { .. }
            | Goal::Tap { .. }
            | Goal::Guard { .. }
            | Goal::Raid { .. }
            | Goal::Survey { .. } => None,
        }
    }

    /// The template this goal fills.
    pub(crate) const fn template(&self) -> &'static str {
        match self {
            Goal::Mine { .. } => crate::easy::EXPAND_AND_MINE,
            Goal::Generator { .. } | Goal::Tap { .. } | Goal::Stone { .. } => {
                crate::easy::HOLD_AND_BUILD
            }
            Goal::Guard { .. } => crate::easy::DEFEND_THE_CORE,
            Goal::Raid { .. } => crate::easy::RAID_AN_ENEMY_BEACON,
            // Funded as the round's post, in front of another template's
            // goals, and alone the Safe Playbook template's frame with its
            // route filled ([`crate::compose`]), so no template is filled
            // with it.
            Goal::Survey { .. } => crate::easy::SAFE_PLAYBOOK,
        }
    }

    /// The feature the goal is about, by name: one goal per feature in a
    /// composed route.
    pub(crate) fn feature(&self) -> &str {
        match self {
            Goal::Mine { seam, .. }
            | Goal::Guard { seam }
            | Goal::Raid { seam, .. }
            | Goal::Survey { seam } => seam,
            Goal::Generator { vent, .. } | Goal::Tap { vent, .. } => vent,
            Goal::Stone { feature, .. } => feature,
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
    /// Game milliseconds the goal costs the route from the core: travel,
    /// bounded as there and back for a site the commander leaves again, plus
    /// the interface time the rules table gives the change.
    pub(crate) time_ms: i64,
    /// Game milliseconds from the commander to the core, which a composed
    /// route visits first ([`crate::compose::deepen_core`]): the same for
    /// every candidate of a round, and 0 when the seat is shown no core.
    pub(crate) lead_ms: i64,
    /// Game milliseconds of its walk from the core to `place`: the estimate's
    /// last leg, what a composed step's timeout is derived from
    /// ([`crate::compose::timeout_ms`]).
    pub(crate) walk_ms: i64,
    /// Game milliseconds its step costs on site, priced from the step as it
    /// is composed ([`crate::price`]).
    pub(crate) on_site_ms: i64,
    /// Whole $ it spends.
    pub(crate) cost_dollars: i64,
    /// Balanced utility, in points.
    pub(crate) utility: i64,
    /// Utility per second, exactly ([`Rate`]): what the ranking orders by.
    pub(crate) rate: Rate,
}

/// A candidate's utility per unit of time, kept as the fraction it is --
/// `points` over `per_ms` game milliseconds, `per_ms` above zero -- so two
/// rates compare exactly, by cross-multiplication (S3's `opr3`, fix pass 3,
/// item 2): a rate divided down to whole points a second tied 140 points in
/// 18 s with 240 in 30 s, and the tie went to the lower id, so the lower rate
/// could be funded first. Each product is of two `i64`s, which an `i128`
/// always holds, so the comparison never overflows.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Rate {
    points: i64,
    per_ms: i64,
}

impl Rate {
    /// Whether the rate is above zero: the worth it is a rate of is.
    pub(crate) const fn is_positive(self) -> bool {
        self.points > 0
    }

    /// `points` over `per_ms`; `None` when `per_ms` is not above zero, which
    /// no candidate's time is ([`Tuning::read`] refuses every interface time
    /// at zero).
    pub(crate) const fn new(points: i64, per_ms: i64) -> Option<Rate> {
        if per_ms > 0 {
            Some(Rate { points, per_ms })
        } else {
            None
        }
    }
}

impl Ord for Rate {
    fn cmp(&self, other: &Rate) -> std::cmp::Ordering {
        let mine = i128::from(self.points) * i128::from(other.per_ms);
        let theirs = i128::from(other.points) * i128::from(self.per_ms);
        mine.cmp(&theirs)
    }
}

impl PartialOrd for Rate {
    fn partial_cmp(&self, other: &Rate) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Rate {
    fn eq(&self, other: &Rate) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}

impl Eq for Rate {}

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
/// vent's anchor is within that beacon's sphere radius as listed (a necessary condition for the
/// sphere to hold it; the gateway answers the rest), in the order of the
/// vent's travel from the commander, then the vent's name, then the beacon's,
/// cut at Easy's candidate count. Nothing when the treasury cannot carry one.
fn tap_pairs(situation: &Situation, tuning: &Tuning) -> Vec<(String, Feature)> {
    if tuning.tap_cost_dollars > situation.economy.treasury {
        return Vec::new();
    }
    let mut pairs: Vec<(String, Feature)> = Vec::new();
    for vent in situation
        .features
        .iter()
        .filter(|feature| feature.kind == Kind::Vent && feature.live && feature.covered)
    {
        let [x, y] = vent.anchor;
        for beacon in situation.own_beacons().filter(|beacon| beacon.powered) {
            let [bx, by, _] = beacon.at;
            // The beacon's sphere as the sim sizes it now, grown by a finished
            // Spire; an own beacon always carries its radius on the wire.
            if beacon
                .sphere_radius_voxels
                .is_some_and(|radius| within([bx, by, 0], [x, y, 0], radius))
            {
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
    first(pairs, EASY_CANDIDATES)
}

/// The first `n` of `items`, in order: a cut by a `u32` count with no
/// conversion to `usize` that could fail (or be defaulted).
pub(crate) fn first<T>(items: Vec<T>, n: u32) -> Vec<T> {
    items.into_iter().zip(0..n).map(|(item, _)| item).collect()
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

/// The taps the gateway says are legal: one `resolve_refs` over a probe
/// playbook holding every candidate pair, then QUICK `verify_plan`s over the
/// pairs not yet answered, at most [`crate::easy::TAP_PROBE_VERIFIES`] of
/// them, and none when there is no pair; and, when the probe could not say,
/// why.
///
/// A pair is legal when its `on` reads the vent it names from that beacon
/// (`resolve_refs`: the sim's own `on_vent` rules) and the verifier raises no
/// `E0503` at its step (the beacon is on the Build mandate). A report lists
/// only so many diagnostics of one code (S3's `vcap`) and says, in its typed
/// `left_out` field (S3's `leftout`), when it left one out. The probe holds
/// no copy of that number: it asks about every pair not yet answered, and
/// when the report's `left_out` names `E0503` it takes the steps up to the
/// last `E0503` listed as answered (the report lists a code's first ones in
/// route order) and asks again from the step after it; when `left_out` does
/// not name `E0503`, every step asked about is answered. Nothing is read from
/// a sentence or a row count. When the probe cannot say ([`TapsUnread`]),
/// the operator cannot say a tap would hold, so it composes none, and the
/// reason is kept ([`crate::Played::taps_unread`]).
pub(crate) fn legal_taps(
    wire: &mut Wire<'_, '_>,
    situation: &Situation,
    tuning: &Tuning,
) -> (Vec<Tap>, Option<TapsUnread>) {
    let pairs = tap_pairs(situation, tuning);
    if pairs.is_empty() {
        return (Vec::new(), None);
    }
    match probe(wire, &pairs) {
        Ok(legal) => (
            pairs
                .into_iter()
                .zip(legal)
                .filter(|(_, legal)| *legal)
                .map(|((beacon, vent), _)| Tap { beacon, vent })
                .collect(),
            None,
        ),
        Err(unread) => (Vec::new(), Some(unread)),
    }
}

/// Why the tap probe could not say which taps are legal, so none is composed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TapsUnread {
    /// A call was refused, or its answer did not read (`MALFORMED`), or
    /// `verify_plan` answered with no report.
    Refused(Refused),
    /// The probe asked its [`crate::easy::TAP_PROBE_VERIFIES`] verifies and
    /// the last one's report still left an `E0503` out: the pairs answered,
    /// of how many.
    Spent {
        /// The pairs answered, from the first.
        answered: usize,
        /// The pairs asked about.
        pairs: usize,
    },
    /// A part's report says it left an `E0503` out and lists none, so no step
    /// of the part is answered and asking again would ask the same.
    LeftOutWithNoneListed {
        /// The part, from 0.
        part: usize,
    },
    /// A part's report names an `E0503` at a step the part does not hold.
    StepPastThePart {
        /// The part, from 0.
        part: usize,
        /// The pointer the `E0503` names.
        path: String,
    },
    /// The probe's arithmetic is past this target's `usize`.
    PastTheTarget,
}

impl From<Refused> for TapsUnread {
    fn from(refused: Refused) -> TapsUnread {
        TapsUnread::Refused(refused)
    }
}

/// The probe's calls, and which pairs they leave legal.
///
/// # Errors
///
/// [`TapsUnread`]: the first refusal, an answer that does not read, or one
/// the cap could have cut.
fn probe(wire: &mut Wire<'_, '_>, pairs: &[(String, Feature)]) -> Result<Vec<bool>, TapsUnread> {
    let text = probe_text(pairs);
    let resolved = wire.call(
        "resolve_refs",
        object(vec![("playbook_jsonc", string(&text))]),
    )?;
    let refs = |misread: Misread| misread.refused("resolve_refs");
    let mut legal = vec![false; pairs.len()];
    for reference in array_of(&resolved, "refs").map_err(refs)? {
        let Some(index) = route_index(text_of(reference, "pointer").map_err(refs)?) else {
            continue;
        };
        let reads = text_of(reference, "feature_id").map_err(refs)?;
        let failure = text_of(reference, "failure").map_err(refs)?;
        if let (Some(slot), Some((_, vent))) = (legal.get_mut(index), pairs.get(index)) {
            *slot = !reads.is_empty() && reads == vent.id && failure.is_empty();
        }
    }
    let verifies =
        usize::try_from(crate::easy::TAP_PROBE_VERIFIES).map_err(|_| TapsUnread::PastTheTarget)?;
    let mut start = 0_usize;
    for number in 0..verifies {
        let Some(part) = pairs.get(start..).filter(|part| !part.is_empty()) else {
            return Ok(legal);
        };
        let read = off_the_build_mandate(wire, number, part)?;
        for index in &read.off {
            let at = start.checked_add(*index).ok_or(TapsUnread::PastTheTarget)?;
            if let Some(slot) = legal.get_mut(at) {
                *slot = false;
            }
        }
        if !read.e0503_left_out {
            return Ok(legal);
        }
        // The report listed `E0503`s up to its last listed one, in route
        // order, and left the rest out: every step after it is unanswered.
        let last = read
            .off
            .iter()
            .max()
            .copied()
            .ok_or(TapsUnread::LeftOutWithNoneListed { part: number })?;
        start = start
            .checked_add(last)
            .and_then(|at| at.checked_add(1))
            .ok_or(TapsUnread::PastTheTarget)?;
    }
    if start >= pairs.len() {
        return Ok(legal);
    }
    Err(TapsUnread::Spent {
        answered: start,
        pairs: pairs.len(),
    })
}

/// What one probe verify's report says: the steps it lists an `E0503` at,
/// and whether its `left_out` field says it left an `E0503` out.
struct PartRead {
    /// Indices into the part of the steps the report lists an `E0503` at.
    off: Vec<usize>,
    /// Whether the report's `left_out` names `E0503`.
    e0503_left_out: bool,
}

/// One part's QUICK `verify_plan`: the steps of `part` (indices into it)
/// whose beacon the report lists as not on the Build mandate, and whether
/// the report left an `E0503` out; or why the answer cannot say so.
///
/// # Errors
///
/// [`TapsUnread`]: the call's refusal, an answer that does not read or
/// carries no `report`, or one that names a step the part does not hold.
fn off_the_build_mandate(
    wire: &mut Wire<'_, '_>,
    number: usize,
    part: &[(String, Feature)],
) -> Result<PartRead, TapsUnread> {
    let verified = wire.call(
        "verify_plan",
        object(vec![
            ("playbook_jsonc", string(&probe_text(part))),
            ("depth", string("quick")),
        ]),
    )?;
    let report = verified
        .get("report")
        .ok_or_else(|| malformed("verify_plan", "the answer carries no report"))?;
    let diagnostics = |misread: Misread| misread.refused("verify_plan");
    let mut off: Vec<usize> = Vec::new();
    for diagnostic in array_of(report, "diagnostics").map_err(diagnostics)? {
        if text_of(diagnostic, "code").map_err(diagnostics)? != "E0503" {
            continue;
        }
        let path = text_of(diagnostic, "path").map_err(diagnostics)?;
        let index = route_index(path)
            .filter(|index| *index < part.len())
            .ok_or_else(|| TapsUnread::StepPastThePart {
                part: number,
                path: path.to_owned(),
            })?;
        off.push(index);
    }
    // The report's own word for what it left out (S3's `leftout`); proto3
    // JSON leaves an empty list out.
    let mut e0503_left_out = false;
    for gap in array_of(report, "left_out").map_err(diagnostics)? {
        if text_of(gap, "code").map_err(diagnostics)? == "E0503" {
            e0503_left_out = true;
        }
    }
    Ok(PartRead {
        off,
        e0503_left_out,
    })
}

/// The goals the map offers this seat, before any estimate: live, affordable,
/// reachable, and in a total order -- the feature's travel from the commander
/// (the feature list's own "nearest"), then the feature's name, then a tap's
/// beacon -- cut at Easy's candidate count.
pub(crate) fn enumerate(
    situation: &Situation,
    tuning: &Tuning,
    taps: &[Tap],
    post: Post,
    raid: Post,
    survey: Post,
) -> Vec<Goal> {
    if situation.commander().is_none() {
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
    // The guard goes first, so that the cut never drops it, and the rest are
    // cut to the room it leaves.
    let guard = guard_goal(situation, tuning, post);
    let survey = survey_goal(situation, tuning, survey);
    let raids = if raid == Post::None {
        raid_goals(situation, tuning)
    } else {
        Vec::new()
    };
    // The guard, the Survey post and the raids go first, so that the cut
    // never drops them, and the rest are cut to the room they leave.
    let goals: Vec<Goal> = guard
        .into_iter()
        .chain(survey)
        .chain(raids)
        .chain(keyed.into_iter().map(|(_, goal)| goal))
        .collect();
    first(goals, EASY_CANDIDATES)
}

/// Whether a Defend post already stands beside the core.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Post {
    /// No Defend beacon of the seat's stands inside the core's sphere.
    None,
    /// One does: no second post is enumerated, so posts never stack.
    Stands,
    /// The probe was refused or did not read: whether one stands is unknown,
    /// and no post is enumerated, since a second one would be a beacon's
    /// price spent on a guess.
    Unknown,
}

/// The guard probe: one `estimate_route` from the commander to "the nearest
/// own Defend beacon" (`beacon_anchor` by the `nearest` selector filtered to
/// the Defend mandate), the sim's own ranking through the gateway. A
/// `NOT_FOUND` is the gateway's answer that the selector ranks nothing, so no
/// Defend beacon stands; a reached anchor inside the core's sphere is a post
/// beside the core. One call when the seat is shown its commander and its
/// core and the library lists the template, none otherwise
/// ([`crate::easy::GUARD_PROBE_CALLS`]).
pub(crate) fn defend_post(wire: &mut Wire<'_, '_>, situation: &Situation) -> Post {
    post_beside_core(wire, situation, "DEFEND", crate::easy::DEFEND_THE_CORE)
}

/// The raid probe, the guard probe's twin for "the nearest own Attack
/// beacon": a raid post beside the core already regroups and relaunches
/// every segment, so a second one is never enumerated while one stands, nor
/// while the probe cannot say ([`crate::easy::RAID_PROBE_CALLS`]).
pub(crate) fn raid_post(wire: &mut Wire<'_, '_>, situation: &Situation) -> Post {
    post_beside_core(wire, situation, "ATTACK", crate::easy::RAID_AN_ENEMY_BEACON)
}

/// Whether an own beacon on `mandate` stands inside the core's sphere: one
/// estimate to the nearest such beacon, when the seat is shown its commander
/// and its core and the library lists `template`, and none otherwise.
fn post_beside_core(
    wire: &mut Wire<'_, '_>,
    situation: &Situation,
    mandate: &str,
    template: &str,
) -> Post {
    let (Some(commander), Some(core)) = (situation.commander(), situation.core()) else {
        return Post::Unknown;
    };
    if !lists(situation, template) {
        return Post::Unknown;
    }
    let nearest_defend = object(vec![(
        "beacon_anchor",
        object(vec![(
            "nearest",
            object(vec![("filter", object(vec![("mandate", string(mandate))]))]),
        )]),
    )]);
    let params = object(vec![(
        "waypoints",
        Json::Array(vec![crate::wire::voxel_location(commander), nearest_defend]),
    )]);
    match wire.call("estimate_route", params) {
        Err(refused) if refused.code == "NOT_FOUND" => Post::None,
        Err(_) => Post::Unknown,
        Ok(estimate) => {
            let anchor = array_of(&estimate, "legs")
                .ok()
                .and_then(|legs| legs.last())
                .and_then(|leg| leg.get("to"))
                .map(location_voxel);
            match anchor {
                Some(Ok(at))
                    if core
                        .sphere_radius_voxels
                        .is_some_and(|radius| within(core.at, at, radius)) =>
                {
                    Post::Stands
                }
                Some(Ok(_)) => Post::None,
                // An answer that does not say where the beacon stands.
                Some(Err(_)) | None => Post::Unknown,
            }
        }
    }
}

/// Whether the enemy sightings Easy needs are stale or missing (spec section
/// 14: the operator "places a Survey post when the enemy sightings it needs
/// are stale or missing"): no enemy beacon the seat holds a sighting of
/// inside `verifier.reach_memory_ms`, the window a raid's target must be
/// sighted in (E0418), so exactly a round with no beacon to raid
/// ([`raid_goals`]). An enemy commander in sight is no sighting this needs:
/// it is never remembered, so it raids only while it is in sight.
pub(crate) fn sightings_stale(situation: &Situation, tuning: &Tuning) -> bool {
    !situation.beacons.iter().any(|beacon| {
        !beacon.own
            && beacon
                .sighting_age_ms
                .is_some_and(|age| age <= tuning.reach_memory_ms)
    })
}

/// The Survey probe, the guard probe's twin for "the nearest own Survey
/// beacon", made only while the sightings are stale or missing
/// ([`sightings_stale`]): a Survey post beside the core already scouts, so a
/// second one is never enumerated while one stands, nor while the probe
/// cannot say ([`crate::easy::SURVEY_PROBE_CALLS`]). A round with fresh
/// sightings makes no call and weighs no post. The post's step is the Defend
/// the Core template's own place step with its mandate made Survey's
/// ([`crate::compose`]), so the library must list that template.
pub(crate) fn survey_post(wire: &mut Wire<'_, '_>, situation: &Situation, tuning: &Tuning) -> Post {
    if !sightings_stale(situation, tuning) {
        return Post::Unknown;
    }
    post_beside_core(wire, situation, "SURVEY", crate::easy::DEFEND_THE_CORE)
}

/// The Survey post a round could place: covering the post seam beside the
/// core ([`post_seam`]), as the guard and the raid posts do. `None` when no
/// Survey beacon is known not to stand beside the core ([`survey_post`]),
/// the seat has no post seam, or the treasury cannot carry the beacon and its
/// scouts. The grid's headroom for its scouts is weighed with every other
/// addition's draw in one kW budget ([`crate::compose`]).
fn survey_goal(situation: &Situation, tuning: &Tuning, survey: Post) -> Option<Goal> {
    if survey != Post::None || tuning.survey_cost_dollars > situation.economy.treasury {
        return None;
    }
    post_seam(situation).map(|seam| Goal::Survey {
        seam: seam.id.clone(),
    })
}

/// The guard post a round could place: the Defend the Core template's step,
/// covering the live seam the seat covers nearest its core by ground
/// distance, ties to the lower anchor `y`, then `x` -- the starting seam,
/// which the core's sphere holds at every seed. `None` when the library lists
/// no Defend the Core template, the seat is shown no core, covers no live seam
/// inside the core's sphere or has a beacon there beside the core already, or
/// the treasury cannot carry the post. It is enumerated every round,
/// so the wizard is offered its site; whether a built-in seat places it is
/// its utility's ([`guard`]).
fn guard_goal(situation: &Situation, tuning: &Tuning, post: Post) -> Option<Goal> {
    if !lists(situation, crate::easy::DEFEND_THE_CORE)
        || tuning.guard_cost_dollars > situation.economy.treasury
        || post != Post::None
    {
        return None;
    }
    post_seam(situation).map(|seam| Goal::Guard {
        seam: seam.id.clone(),
    })
}

/// True when the library lists the template.
fn lists(situation: &Situation, template: &str) -> bool {
    situation.templates.iter().any(|listed| listed == template)
}

/// The seam a post beside the core covers: the live seam the seat covers
/// nearest its core by ground distance, inside the core's sphere, ties to the
/// lower anchor `y`, then `x` -- the starting seam, which the core's sphere
/// holds at every seed. `None` when the seat is shown no core or covers no
/// live seam inside its sphere.
fn post_seam(situation: &Situation) -> Option<&Feature> {
    let core = situation.core()?;
    let [cx, cy, _] = core.at;
    // The core's sphere as the sim sizes it now, grown by a finished Spire.
    let radius = core.sphere_radius_voxels?;
    situation
        .features
        .iter()
        .filter(|feature| feature.kind == Kind::Seam && feature.live && feature.covered)
        .filter_map(|feature| {
            let [x, y] = feature.anchor;
            // Only a seam inside the core's sphere: a post anywhere else would
            // not stand beside the core it guards.
            let d2 = distance2([x, y, 0], [cx, cy, 0])?;
            (d2 <= radius.checked_mul(radius)?).then_some(((d2, y, x), feature))
        })
        .min_by_key(|(key, _)| *key)
        .map(|(_, feature)| feature)
}

/// A raid target's place in [`raid_goals`]' order: whether its distance does
/// not fit, the distance, the seat, the beacon's name.
type RaidKey = (bool, Option<i64>, u8, String);

/// The raids a round could make: one goal per fixed target, the post beside
/// the core ([`post_seam`]) for all of them. A target is an enemy beacon with
/// a sighting inside `verifier.reach_memory_ms` (an older one is E0418's
/// refusal at the seal), or an enemy commander the view shows now, which is
/// a sighting of age 0 -- a remembered commander is on no seat-facing wire,
/// so Easy raids only one it sees. One list, ordered by ground distance from
/// the core, ties to the lowest seat, then a seat's commander before its
/// beacons (its key's name is empty), then the beacon's name, cut at
/// [`RAID_TARGETS_MAX`]. Nothing when the library lists no Raid template, the
/// seat has no post seam, or the treasury cannot carry the beacon and its
/// raiders.
fn raid_goals(situation: &Situation, tuning: &Tuning) -> Vec<Goal> {
    let (Some(core), Some(seam)) = (situation.core(), post_seam(situation)) else {
        return Vec::new();
    };
    if !lists(situation, crate::easy::RAID_AN_ENEMY_BEACON)
        || tuning.raid_cost_dollars > situation.economy.treasury
    {
        return Vec::new();
    }
    let ground = |at: [i32; 3]| {
        let ([x, y, _], [cx, cy, _]) = (at, core.at);
        distance2([x, y, 0], [cx, cy, 0])
    };
    // (whether the distance does not fit, the distance, the seat, the
    // beacon's name): a distance that does not fit sorts after every one that
    // does.
    let mut keyed: Vec<(RaidKey, RaidTarget)> = Vec::new();
    for beacon in situation.beacons.iter().filter(|beacon| !beacon.own) {
        let Some(age) = beacon.sighting_age_ms else {
            continue;
        };
        if age > tuning.reach_memory_ms {
            continue;
        }
        let d2 = ground(beacon.at);
        keyed.push((
            (d2.is_none(), d2, beacon.seat, beacon.id.clone()),
            RaidTarget::Beacon {
                id: beacon.id.clone(),
                at: beacon.at,
            },
        ));
    }
    for enemy in situation.enemies() {
        if enemy.asset != Asset::Unit(UnitKind::Commander) {
            continue;
        }
        let d2 = ground(enemy.at);
        keyed.push((
            (d2.is_none(), d2, enemy.seat, String::new()),
            RaidTarget::Commander {
                seat: enemy.seat,
                at: enemy.at,
            },
        ));
    }
    keyed.sort_by(|(a, _), (b, _)| a.cmp(b));
    first(keyed, RAID_TARGETS_MAX)
        .into_iter()
        .map(|(_, target)| Goal::Raid {
            seam: seam.id.clone(),
            target,
        })
        .collect()
}

/// Why Hold & Build has nothing to place this round, said about the vent
/// the template's own description would read: the nearest by travel that the
/// seat does not cover, ties to the lower anchor `y`, then the lower anchor
/// `x`, as the sim's "nearest" breaks them (`crates/sim/src/targeting.rs`).
/// The feature list's travel is whole game milliseconds where the sim ranks
/// by its integer estimator cost, so two vents whose costs differ by less
/// than a millisecond's worth read as a tie here.
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
    if situation.commander().is_none() {
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
        .min_by_key(|feature| {
            let [x, y] = feature.anchor;
            (feature.travel_ms, y, x)
        })
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

/// What one estimate says of a goal: the place its last leg ends on, that
/// leg's travel and the travel before it, `None` when the route does not
/// reach, or the [`Misread`] of an answer that does not read.
type Reading = Result<Option<([i32; 3], i64, i64)>, Misread>;

/// What one `estimate_route` answer says of a goal: the place its last leg
/// ends on, that leg's travel, and the travel before it; `None` when the
/// route does not reach.
///
/// # Errors
///
/// [`Misread`] for an answer that does not read, and for a reachable route
/// with no leg, a leg with no place, or a figure below zero.
fn reading(estimate: &Json) -> Reading {
    if !bool_of(estimate, "reachable")? {
        return Ok(None);
    }
    let legs = array_of(estimate, "legs")?;
    let last = legs.last().ok_or_else(|| Misread {
        key: String::from("legs"),
        wanted: "a route with a leg",
    })?;
    let place = location_voxel(last.get("to").ok_or_else(|| Misread {
        key: String::from("to"),
        wanted: "a leg's place",
    })?)?;
    let travel = int_of(last, "ms")?;
    let lead = int_of(estimate, "ms")?
        .checked_sub(travel)
        .filter(|lead| *lead >= 0 && travel >= 0)
        .ok_or_else(|| Misread {
            key: String::from("ms"),
            wanted: "a route's travel at least its last leg's",
        })?;
    Ok(Some((place, travel, lead)))
}

/// What one raid's `estimate_route` answer says: its post's site, the
/// commander's travel to it from the waypoint before, and the travel before
/// that; `None` when the route does not reach the target. The last leg, the
/// raiders' march from the site to the target, is what makes the target
/// reachable or not, and is not otherwise scored.
///
/// # Errors
///
/// [`Misread`] for an answer that does not read, and for a reachable route
/// with fewer than two legs, a leg with no place, or a figure below zero.
fn raid_reading(estimate: &Json) -> Reading {
    if !bool_of(estimate, "reachable")? {
        return Ok(None);
    }
    let short = || Misread {
        key: String::from("legs"),
        wanted: "a raid's route with a leg to its post and one to its target",
    };
    let legs = array_of(estimate, "legs")?;
    let (march, before) = legs.split_last().ok_or_else(short)?;
    let to_post = before.last().ok_or_else(short)?;
    let site = location_voxel(to_post.get("to").ok_or_else(|| Misread {
        key: String::from("to"),
        wanted: "a leg's place",
    })?)?;
    let travel = int_of(to_post, "ms")?;
    let march = int_of(march, "ms")?;
    let lead = int_of(estimate, "ms")?
        .checked_sub(travel)
        .and_then(|rest| rest.checked_sub(march))
        .filter(|lead| *lead >= 0 && travel >= 0 && march >= 0)
        .ok_or_else(|| Misread {
            key: String::from("ms"),
            wanted: "a route's travel at least its legs' to the post and the target",
        })?;
    Ok(Some((site, travel, lead)))
}

/// Evaluate every goal: one `estimate_route` each, then the Balanced
/// utility. A goal the estimate refuses, answers unreadably or calls
/// unreachable is dropped; the call still counts.
///
/// A composed route visits the core first ([`crate::compose::deepen_core`]),
/// so when the seat is shown a core each estimate's waypoints are the
/// commander, the core's anchor, and the goal: the first leg is the
/// candidate's `lead_ms`, the same for all of them, and the goal is costed
/// from the core, where the route's walk to it starts. A `covering` waypoint
/// ranks its site from the waypoint before it, as the step ranks from where
/// the commander stands when it starts.
///
/// # Stepping stones (the register's S5-04)
///
/// With `stones`, a Mine or Generator goal whose `covering` estimate the
/// gateway refuses `NOT_FOUND` -- no site the seat's spheres allow covers the
/// feature, which is why Easy used to stay inside its first sphere
/// (`docs/design/targeting.md`, "toward") -- is weighed once more, after
/// every first estimate, as a **stone**: one more `estimate_route` whose last
/// waypoint is `{"toward": {"feature": {"feature_id": ...}}}`, answered with
/// the site the sim's own `toward` would place it on. A `toward` the gateway
/// refuses drops the stone. The retries go in the enumeration's own order and
/// each keeps its goal's id, so ties still go to the lowest; at most
/// [`STONE_TARGETS_MAX`] are made, which is every refused placement, since a
/// goal is retried once at most. The advisor passes no `stones`: a stone is
/// never offered to the wizard ([`best_for`]), so it spends no call on one.
pub(crate) fn evaluate(
    wire: &mut Wire<'_, '_>,
    situation: &Situation,
    tuning: &Tuning,
    declared: &[Declared],
    goals: Vec<Goal>,
    stones: Option<&[String]>,
) -> Evaluated {
    let mut out = Evaluated::default();
    let Some(commander) = situation.commander() else {
        return out;
    };
    let mut far: Vec<(usize, Goal)> = Vec::new();
    for (id, goal) in goals.into_iter().enumerate() {
        let to = match &goal {
            // The site the beacon would stand on, chosen by the sim's own
            // `cover` (the register's S1-20): the last leg ends there.
            Goal::Mine { seam: name, .. }
            | Goal::Generator { vent: name, .. }
            | Goal::Guard { seam: name }
            | Goal::Survey { seam: name } => object(vec![(
                "covering",
                object(vec![("feature_id", string(name))]),
            )]),
            Goal::Tap { beacon, .. } => object(vec![(
                "beacon_anchor",
                object(vec![("beacon_id", string(beacon))]),
            )]),
            // The post's site, then the target: the raiders' march is the
            // last leg.
            Goal::Raid { seam, .. } => object(vec![(
                "covering",
                object(vec![("feature_id", string(seam))]),
            )]),
            Goal::Stone { feature, .. } => toward(feature),
        };
        let mut waypoints = from_the_core(situation, commander);
        waypoints.push(to);
        if let Goal::Raid { target, .. } = &goal {
            waypoints.push(target.waypoint());
        }
        match estimated(wire, waypoints, &mut out.refused) {
            Estimate::Answered(estimate) => {
                let read = match goal {
                    Goal::Raid { .. } => raid_reading(&estimate),
                    Goal::Mine { .. }
                    | Goal::Generator { .. }
                    | Goal::Tap { .. }
                    | Goal::Guard { .. }
                    | Goal::Survey { .. }
                    | Goal::Stone { .. } => reading(&estimate),
                };
                out.push(situation, tuning, declared, id, goal, &read);
            }
            // No site covers it: with `stones`, a Mine or Generator goal is
            // weighed again as a stone toward its feature.
            Estimate::NotFound => {
                // A stone that failed last round is not weighed again: its
                // chain is shown out of reach this round (fix pass 3, item 6).
                if let (Some(stone), Some(failed)) = (stone_for(&goal), stones)
                    && !failed.iter().any(|feature| feature == stone.feature())
                {
                    far.push((id, stone));
                }
            }
            Estimate::Refused => {}
        }
    }
    // A chain begun is continued (module docs): its stone is weighed alone,
    // and the rest only when its feature is shown out of reach this round --
    // a `toward` toward it refused, or a stone that no round's share could
    // carry or that is worth nothing.
    let far = first(far, STONE_TARGETS_MAX);
    let chain = chain_feature(situation, &far);
    let (begun, rest): (Vec<_>, Vec<_>) = far
        .into_iter()
        .partition(|(_, goal)| chain.as_deref() == Some(goal.feature()));
    let weighed = out.candidates.len();
    out.stones(wire, situation, tuning, declared, commander, begun);
    let continued = match crate::compose::fill_ms(situation) {
        Some(fill) => out
            .candidates
            .get(weighed..)
            .into_iter()
            .flatten()
            .any(|stone| stone.rate.is_positive() && stone.time_ms <= fill),
        None => false,
    };
    if !continued {
        out.stones(wire, situation, tuning, declared, commander, rest);
    }
    out
}

/// What [`evaluate`] weighed: every candidate whose estimate answered, in the
/// order they were estimated (a stone, weighed after the first estimates,
/// comes after them; every reader ranks by rate and id, or by kind, so the
/// order decides nothing), and every refusal of an estimate other than the
/// gateway's `NOT_FOUND`, which [`crate::easy`] reports with the round.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub(crate) struct Evaluated {
    /// The candidates.
    pub(crate) candidates: Vec<Candidate>,
    /// The estimates refused for any other reason than `NOT_FOUND`.
    pub(crate) refused: Vec<Refused>,
}

impl Evaluated {
    /// Weigh each stone: one `toward` estimate each. `NOT_FOUND` is the
    /// gateway's answer that no stone has a site: the stone is not weighed.
    /// Any other refusal is kept in `refused`.
    fn stones(
        &mut self,
        wire: &mut Wire<'_, '_>,
        situation: &Situation,
        tuning: &Tuning,
        declared: &[Declared],
        commander: [i32; 3],
        stones: Vec<(usize, Goal)>,
    ) {
        for (id, goal) in stones {
            let mut waypoints = from_the_core(situation, commander);
            waypoints.push(toward(goal.feature()));
            if let Estimate::Answered(estimate) = estimated(wire, waypoints, &mut self.refused) {
                let read = reading(&estimate);
                self.push(situation, tuning, declared, id, goal, &read);
            }
        }
    }

    /// Score one answered goal, or drop it: an answer that does not read or a
    /// route that does not reach is a goal not weighed, as the module docs
    /// say, and a goal whose step does not build has no price.
    fn push(
        &mut self,
        situation: &Situation,
        tuning: &Tuning,
        declared: &[Declared],
        id: usize,
        goal: Goal,
        read: &Reading,
    ) {
        let Ok(&Some((place, travel, lead))) = read.as_ref() else {
            return;
        };
        if let Some(mut candidate) = score(situation, tuning, declared, id, goal, place, travel) {
            candidate.lead_ms = lead;
            self.candidates.push(candidate);
        }
    }
}

/// One estimate's outcome.
enum Estimate {
    /// The gateway answered.
    Answered(Json),
    /// The gateway answered `NOT_FOUND`: no site, selector or stone answers
    /// the last waypoint.
    NotFound,
    /// Any other refusal, kept in the round's refusals.
    Refused,
}

/// One `estimate_route` over `waypoints`.
fn estimated(
    wire: &mut Wire<'_, '_>,
    waypoints: Vec<Json>,
    refused: &mut Vec<Refused>,
) -> Estimate {
    let params = object(vec![("waypoints", Json::Array(waypoints))]);
    match wire.call("estimate_route", params) {
        Ok(estimate) => Estimate::Answered(estimate),
        Err(refusal) if refusal.code == "NOT_FOUND" => Estimate::NotFound,
        Err(refusal) => {
            refused.push(refusal);
            Estimate::Refused
        }
    }
}

/// The waypoints every goal's estimate opens with: the commander, then the
/// core's anchor when the seat is shown a core.
fn from_the_core(situation: &Situation, commander: [i32; 3]) -> Vec<Json> {
    let mut waypoints = vec![crate::wire::voxel_location(commander)];
    if let Some(core) = situation.core() {
        waypoints.push(object(vec![(
            "beacon_anchor",
            object(vec![("beacon_id", string(&core.id))]),
        )]));
    }
    waypoints
}

/// A `toward` waypoint naming a feature: where a stone toward it would stand.
fn toward(feature: &str) -> Json {
    object(vec![(
        "toward",
        object(vec![(
            "feature",
            object(vec![("feature_id", string(feature))]),
        )]),
    )])
}

/// The stone a placement the gateway answered `NOT_FOUND` for is weighed as:
/// a Mine or Generator goal no site covers. Any other goal is not a far
/// feature.
fn stone_for(goal: &Goal) -> Option<Goal> {
    match goal {
        Goal::Mine { seam, grade } => Some(Goal::Stone {
            feature: seam.clone(),
            kind: Kind::Seam,
            grade: *grade,
        }),
        Goal::Generator { vent, grade } => Some(Goal::Stone {
            feature: vent.clone(),
            kind: Kind::Vent,
            grade: *grade,
        }),
        Goal::Tap { .. }
        | Goal::Guard { .. }
        | Goal::Raid { .. }
        | Goal::Survey { .. }
        | Goal::Stone { .. } => None,
    }
}

/// A beacon's gap to a feature's anchor, whole voxels over the ground: how far
/// past its sphere's listed radius the anchor lies (negative inside it);
/// `None` for a beacon with no radius on the wire, or a figure that does not
/// fit.
fn gap(beacon: &crate::situation::Beacon, anchor: [i32; 2]) -> Option<i64> {
    let [x, y] = anchor;
    let [bx, by, _] = beacon.at;
    distance2([bx, by, 0], [x, y, 0])?
        .checked_isqrt()?
        .checked_sub(beacon.sphere_radius_voxels?)
}

/// The seat's least gap to a feature's anchor over its own beacons, less
/// `without` if one is named.
fn least_gap(situation: &Situation, anchor: [i32; 2], without: Option<&str>) -> Option<i64> {
    situation
        .own_beacons()
        .filter(|beacon| without != Some(beacon.id.as_str()))
        .filter_map(|beacon| gap(beacon, anchor))
        .min()
}

/// The stones a gap still needs before a site inside the seat's spheres
/// covers its feature: none while the gap is at most `radius` (a new
/// beacon's base sphere), then `ceil(gap / radius) - 1`, each stone closing
/// at most one base radius of it.
fn stones_for_gap(gap: i64, radius: i64) -> Option<i64> {
    if gap <= radius {
        return Some(0);
    }
    gap.checked_add(radius.checked_sub(1)?)?
        .checked_div(radius)?
        .checked_sub(1)
}

/// The least walk from the seat's core to a site covering a feature at
/// `anchor`, game milliseconds: the site stands within the base radius of the
/// anchor, each step moves at most one voxel on each axis and costs at least
/// [`Tuning::step_cost_min`], at `commander.cost_per_second`. A floor, never
/// an estimate; `None` for a seat shown no core, or a figure that does not
/// fit.
fn walk_floor_ms(situation: &Situation, tuning: &Tuning, anchor: [i32; 2]) -> Option<i64> {
    let [cx, cy, _] = situation.core()?.at;
    let [x, y] = anchor;
    let across = |p: i32, q: i32| i64::from(p).checked_sub(i64::from(q)).map(i64::abs);
    let voxels = across(x, cx)?
        .max(across(y, cy)?)
        .checked_sub(tuning.sphere_radius_voxels)?
        .max(0);
    voxels
        .checked_mul(tuning.step_cost_min)?
        .checked_mul(1_000)?
        .checked_div(tuning.commander_cost_per_second)
}

/// A goal's points and its own time, for a stone's worth: the Mine or
/// Generator at `feature`, its travel no shorter than `travel_ms` (the
/// stone's, which a goal past it walks at least) nor than the floor to it
/// ([`walk_floor_ms`]), priced at `place` as composed. `None` for a goal whose
/// step does not build or price, or a figure that does not fit.
fn goal_worth(
    situation: &Situation,
    tuning: &Tuning,
    declared: &[Declared],
    goal: &Goal,
    anchor: [i32; 2],
    travel_ms: i64,
    place: [i32; 3],
) -> Option<(i64, i64)> {
    let travel = travel_ms.max(walk_floor_ms(situation, tuning, anchor)?);
    let on_site = priced(situation, tuning, declared, goal, place)?;
    let (_, economy, expansion, _, time_ms) = terms(situation, tuning, goal, travel, on_site)?;
    let points = BALANCED
        .economy
        .checked_mul(economy)?
        .checked_add(BALANCED.expansion.checked_mul(expansion)?)?;
    Some((points, time_ms))
}

/// What a stone standing at `site` buys this round besides its own sphere
/// (module docs, "A stone's worth, and its chain"), as thousandths of a point
/// and the game milliseconds the goals that worth needs take: every live far
/// feature but the one it serves that the seat's spheres do not reach yet,
/// brought **into reach** by the stone's sphere at its goal's worth over the
/// two rounds it then takes, with that goal's own time; or brought **nearer**
/// by more than a stone of its own would bring it, at a beacon's price for
/// each stone beyond that its chain no longer needs. A feature with no route,
/// a goal worth nothing, or a figure that does not fit adds nothing.
#[allow(
    clippy::too_many_arguments,
    reason = "each is one input of the stone's worth: the situation, its rows, the declared templates, the stone's site, travel and served feature"
)]
fn stone_buys(
    situation: &Situation,
    tuning: &Tuning,
    declared: &[Declared],
    site: [i32; 3],
    travel_ms: i64,
    served: &str,
) -> Option<(i64, i64)> {
    let radius = tuning.sphere_radius_voxels;
    let [sx, sy, _] = site;
    let (mut points, mut time) = (0_i64, 0_i64);
    for feature in situation.features.iter().filter(|feature| {
        feature.live && !feature.covered && feature.travel_ms.is_some() && feature.id != served
    }) {
        let Some(before) = least_gap(situation, feature.anchor, None) else {
            continue;
        };
        let before_stones = stones_for_gap(before, radius)?;
        if before_stones == 0 {
            // Placeable from the spheres already: its own goal, not the
            // stone's.
            continue;
        }
        let [x, y] = feature.anchor;
        let from_stone = distance2([sx, sy, 0], [x, y, 0])?
            .checked_isqrt()?
            .checked_sub(radius)?;
        let after_stones = stones_for_gap(before.min(from_stone), radius)?;
        if after_stones == 0 {
            let goal = match feature.kind {
                Kind::Seam => Goal::Mine {
                    seam: feature.id.clone(),
                    grade: feature.grade,
                },
                Kind::Vent => Goal::Generator {
                    vent: feature.id.clone(),
                    grade: feature.grade,
                },
            };
            let (worth, goal_ms) = goal_worth(
                situation,
                tuning,
                declared,
                &goal,
                feature.anchor,
                travel_ms,
                site,
            )?;
            if worth > 0 {
                points = points.checked_add(worth.checked_mul(1_000)?.checked_div(2)?)?;
                time = time.checked_add(goal_ms)?;
            }
        } else if let Some(beyond) = before_stones
            .checked_sub(after_stones)?
            .checked_sub(1)
            .filter(|beyond| *beyond > 0)
        {
            points = points.checked_add(
                tuning
                    .beacon_cost_dollars
                    .checked_mul(beyond)?
                    .checked_mul(1_000)?,
            )?;
        }
    }
    Some((points, time))
}

/// How many stepping stones a feature no site covers still needs, at least
/// (module docs, "A stone's worth, and its chain"): a beacon placed inside an
/// own sphere covers a feature within the base radius of it, so the feature is
/// coverable once its gap is at most the base radius, and each stone closes at
/// most one base radius of the gap. Over the ground, which is no further than
/// the sim's own distance, so the count is a lower bound; at least one,
/// because the gateway said no site covers it now. `None` for a feature the
/// view does not list, a seat shown no beacon with a radius, or a figure that
/// does not fit.
pub(crate) fn stones_needed(situation: &Situation, tuning: &Tuning, feature: &str) -> Option<i64> {
    let anchor = situation
        .features
        .iter()
        .find(|held| held.id == feature)?
        .anchor;
    let gap = least_gap(situation, anchor, None)?;
    Some(stones_for_gap(gap, tuning.sphere_radius_voxels)?.max(1))
}

/// The far feature a begun chain is toward, if one is (module docs, "A stone's
/// worth, and its chain"): over every stone the view shows -- an own beacon,
/// not the core, whose sphere holds no live feature's anchor -- and every far
/// feature this round refused for `covering`, the one whose gap to the seat's
/// spheres the stone closed the most, ties to the smaller gap left, then the
/// feature's name. `None` with no stone, or no stone that closed any far
/// feature's gap.
fn chain_feature(situation: &Situation, far: &[(usize, Goal)]) -> Option<String> {
    let holds_a_feature = |beacon: &crate::situation::Beacon| {
        situation
            .features
            .iter()
            .filter(|feature| feature.live)
            .any(|feature| gap(beacon, feature.anchor).is_some_and(|gap| gap <= 0))
    };
    let stones: Vec<&str> = situation
        .own_beacons()
        .filter(|beacon| !beacon.core && !holds_a_feature(beacon))
        .map(|beacon| beacon.id.as_str())
        .collect();
    let mut best: Option<((i64, i64), String)> = None;
    for (_, goal) in far {
        let Some(anchor) = situation
            .features
            .iter()
            .find(|feature| feature.id == goal.feature())
            .map(|feature| feature.anchor)
        else {
            continue;
        };
        let Some(with) = least_gap(situation, anchor, None) else {
            continue;
        };
        for stone in &stones {
            let Some(closed) = least_gap(situation, anchor, Some(stone))
                .and_then(|without| without.checked_sub(with))
            else {
                continue;
            };
            if closed <= 0 {
                continue;
            }
            // Most closed first, then the smaller gap left, then the name.
            let key = (closed, with.checked_neg()?);
            let better = match &best {
                None => true,
                Some((held, name)) => {
                    key > *held || (key == *held && goal.feature() < name.as_str())
                }
            };
            if better {
                best = Some((key, goal.feature().to_owned()));
            }
        }
    }
    best.map(|(_, feature)| feature)
}

/// The summed threat of the other seats' assets the view shows inside the
/// core's sphere, as `list_beacons` sizes it now (a finished Spire grows it),
/// in thousandths of a hit point per second (decision 12's model: each
/// asset's weapon's damage per second, zero if unarmed); `None` when the seat
/// is shown no core, or a sum does not fit.
pub(crate) fn core_threat(situation: &Situation, tuning: &Tuning) -> Option<u64> {
    let core = situation.core()?;
    threat_near(situation, tuning, core.at, core.sphere_radius_voxels?)
}

/// The summed threat of the other seats' assets the view shows within
/// `radius` of `at`, in thousandths of a hit point per second (decision
/// 12's model, as [`core_threat`] reads it); `None` when a sum does not fit.
fn threat_near(situation: &Situation, tuning: &Tuning, at: [i32; 3], radius: i64) -> Option<u64> {
    situation
        .enemies()
        .iter()
        .filter(|enemy| within(at, enemy.at, radius))
        .try_fold(0_u64, |sum, enemy| {
            sum.checked_add(tuning.threat(enemy.asset))
        })
}

/// A raid's attack, in points: [`RAID_POINTS`], less the threat that defends
/// its target at [`DEFENCE_POINTS_PER_HP_PER_SECOND`], by one rule for a
/// beacon and a commander (spec section 14); `None` when a figure does not
/// fit, which no rules table the operator reads can produce.
///
/// PLACEHOLDER: "defends" is within one sphere radius of the target, the sphere radius reused as a threat radius — owner, S5, with the Balanced weights
///
/// An enemy core's own weapon is not charged: no seat-facing wire says which
/// enemy beacon is a core (`gateway.proto`'s `BeaconSummary.core` is own
/// beacons only), so a raid on a core is weighed as a raid on any beacon.
///
/// PLACEHOLDER: a raid on an enemy core is not charged the core's weapon, which no seat-facing field reveals — owner, S5, with Normal and Hard
fn raid_attack(situation: &Situation, tuning: &Tuning, target: &RaidTarget) -> Option<i64> {
    // "Defends" is a threat radius, the base sphere's (the PLACEHOLDER
    // above), and no beacon's sphere as it stands.
    let radius = tuning.sphere_radius_voxels;
    let threat = i64::try_from(threat_near(situation, tuning, target.at(), radius)?).ok()?;
    RAID_POINTS.checked_sub(
        threat
            .checked_mul(DEFENCE_POINTS_PER_HP_PER_SECOND)?
            .checked_div(1_000)?,
    )
}

/// A guard post's defence, in points: the threat inside the core's sphere at
/// [`DEFENCE_POINTS_PER_HP_PER_SECOND`], and [`COMMANDER_DIED_POINTS`] when
/// the commander died in the last Push (decision 35); `None` when a figure
/// does not fit.
fn guard_defence(situation: &Situation, tuning: &Tuning) -> Option<i64> {
    let threat = i64::try_from(core_threat(situation, tuning)?).ok()?;
    let from_threat = threat
        .checked_mul(DEFENCE_POINTS_PER_HP_PER_SECOND)?
        .checked_div(1_000)?;
    let from_death = match situation.last_push {
        LastPush::Died => COMMANDER_DIED_POINTS,
        LastPush::Survived | LastPush::Unread(_) => 0,
    };
    from_threat.checked_add(from_death)
}

/// What a goal's step costs on site, priced from the step exactly as it is
/// composed ([`crate::compose::step_for`], [`crate::price::step_ms`]); `None`
/// when the step does not build (its template is not declared) or does not
/// price.
fn priced(
    situation: &Situation,
    tuning: &Tuning,
    declared: &[Declared],
    goal: &Goal,
    place: [i32; 3],
) -> Option<i64> {
    let step = crate::compose::step_for(goal, place, situation, declared)?;
    crate::price::step_ms(&step, tuning)
}

/// One goal's price and its four scored terms before risk and attack:
/// `(cost, economy, expansion, defence, time_ms)`; `None` when a figure does
/// not fit.
fn terms(
    situation: &Situation,
    tuning: &Tuning,
    goal: &Goal,
    travel_ms: i64,
    on_site: i64,
) -> Option<(i64, i64, i64, i64, i64)> {
    Some(match goal {
        Goal::Mine { grade, .. } => {
            let cost = tuning.beacon_cost_dollars;
            let ore = tuning.ore_yield(*grade).checked_mul(tuning.seam_voxels)?;
            (
                cost,
                ore.checked_sub(cost)?,
                EXPANSION_POINTS_PER_BEACON,
                0,
                // There, place, and back: the template's route returns home.
                // PLACEHOLDER: the way back costed as the way there (travel x 2), not estimated — owner, S5, with Normal and Hard
                travel_ms.checked_mul(2)?.checked_add(on_site)?,
            )
        }
        Goal::Generator { grade, .. } => {
            let cost = tuning.vent_cost_dollars;
            let power = tuning.generator_kw(*grade).checked_mul(DOLLARS_PER_KW)?;
            (
                cost,
                power.checked_sub(cost)?,
                EXPANSION_POINTS_PER_BEACON,
                0,
                // There, place, and commit the initial settings and the Build
                // target on site; the commander then holds by the new beacon
                // while its drone builds.
                travel_ms.checked_add(on_site)?,
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
                0,
                travel_ms.checked_add(on_site)?,
            )
        }
        Goal::Raid { .. } => {
            let cost = tuning.raid_cost_dollars;
            (
                cost,
                // A raid earns nothing the economy term counts: its worth is
                // its attack term.
                cost.checked_neg()?,
                // Beside the core, inside its sphere: no reach it adds.
                0,
                0,
                travel_ms.checked_add(on_site)?,
            )
        }
        Goal::Survey { .. } => {
            let cost = tuning.survey_cost_dollars;
            (
                cost,
                // A Survey post earns nothing of its own: its worth is the
                // raid its sightings buy (`score`), whose price it is charged
                // the round's share of, beside its own.
                cost.checked_neg()?
                    .checked_sub(tuning.raid_cost_dollars.checked_div(SURVEY_RAID_ROUNDS)?)?,
                // Beside the core, inside its sphere: no reach it adds.
                0,
                0,
                travel_ms.checked_add(on_site)?,
            )
        }
        // A stone is scored as the goal it serves (`score`); weighed alone it
        // is its price and its walk.
        Goal::Stone { .. } => {
            let cost = tuning.beacon_cost_dollars;
            (
                cost,
                cost.checked_neg()?,
                0,
                0,
                travel_ms.checked_add(on_site)?,
            )
        }
        Goal::Guard { .. } => {
            let cost = tuning.guard_cost_dollars;
            (
                cost,
                // A guard post earns nothing: its worth is its defence.
                cost.checked_neg()?,
                // Beside the core, inside its sphere: no reach it adds.
                0,
                guard_defence(situation, tuning)?,
                travel_ms.checked_add(on_site)?,
            )
        }
    })
}

/// One goal's features and Balanced utility; `None` when a figure does not
/// fit an `i64`, which no rules table the operator reads can produce, or the
/// goal takes no time, which no row the operator reads can make it (every
/// interface time is refused at zero, [`Tuning::read`]).
fn score(
    situation: &Situation,
    tuning: &Tuning,
    declared: &[Declared],
    id: usize,
    goal: Goal,
    place: [i32; 3],
    travel_ms: i64,
) -> Option<Candidate> {
    let on_site = priced(situation, tuning, declared, &goal, place)?;
    // A stone is charged its own price and time and is worth what it buys
    // this round and the goal it serves, discounted by the rounds still needed
    // (module docs, "A stone's worth, and its chain"); `served_milli` is that
    // half, in thousandths of a point, kept apart so the division by the
    // rounds is the only rounding, at a thousandth.
    //
    // PLACEHOLDER: a later round's worth read as a round's share of it, the served goal's worth divided by the chain's rounds — owner, S5, with the Balanced weights
    let (cost, economy, expansion, defence, time_ms, rate_ms, served_milli) =
        if let Some(served) = goal.served() {
            // The served goal costs its own time in the later round, from the
            // last stone's sphere: no shorter than this stone's walk, nor than
            // the floor to the feature.
            let anchor = situation
                .features
                .iter()
                .find(|feature| feature.id == goal.feature())?
                .anchor;
            let (served_points, served_ms) = goal_worth(
                situation, tuning, declared, &served, anchor, travel_ms, place,
            )?;
            let rounds = stones_needed(situation, tuning, goal.feature())?.checked_add(1)?;
            let (buys, buys_ms) = stone_buys(
                situation,
                tuning,
                declared,
                place,
                travel_ms,
                goal.feature(),
            )?;
            let served_milli = served_points
                .checked_mul(1_000)?
                .checked_div(rounds)?
                .checked_add(buys)?;
            let cost = tuning.beacon_cost_dollars;
            let round_ms = travel_ms.checked_add(on_site)?;
            (
                cost,
                cost.checked_neg()?,
                EXPANSION_POINTS_PER_BEACON,
                0,
                round_ms,
                round_ms.checked_add(served_ms)?.checked_add(buys_ms)?,
                served_milli,
            )
        } else {
            let (cost, economy, expansion, defence, time_ms) =
                terms(situation, tuning, &goal, travel_ms, on_site)?;
            // A Survey post's rate counts the raid it buys too, at least its own
            // time: the raid post stands on the same seam.
            let rate_ms = match goal {
                Goal::Survey { .. } => time_ms.checked_mul(2)?,
                Goal::Mine { .. }
                | Goal::Generator { .. }
                | Goal::Tap { .. }
                | Goal::Guard { .. }
                | Goal::Raid { .. }
                | Goal::Stone { .. } => time_ms,
            };
            (cost, economy, expansion, defence, time_ms, rate_ms, 0)
        };
    // Risk: what the seat can see of other seats near the goal.
    //
    // PLACEHOLDER: "near" is within one sphere radius of the goal, the sphere radius reused as a threat radius — owner, S5, with the Balanced weights
    // A guard post is charged none: the enemies near it are what it guards
    // against, and they are its defence term already.
    let near = match goal {
        // A Survey post stands where the guard would, beside the core.
        Goal::Guard { .. } | Goal::Survey { .. } => 0,
        Goal::Mine { .. }
        | Goal::Generator { .. }
        | Goal::Tap { .. }
        | Goal::Raid { .. }
        | Goal::Stone { .. } => situation
            .enemies()
            .iter()
            .filter(|enemy| within(place, enemy.at, tuning.sphere_radius_voxels))
            .count(),
    };
    let risk = i64::try_from(near)
        .ok()?
        .checked_mul(RISK_POINTS_PER_ENEMY)?;
    // Capability is zero at Easy, and attack is a raid's alone (module docs).
    let capability = 0_i64;
    let attack = match &goal {
        Goal::Raid { target, .. } => raid_attack(situation, tuning, target)?,
        // The raid a Survey post buys: an undefended target, as no target is
        // in sight, its worth a round's share of the rounds it takes.
        Goal::Survey { .. } => RAID_POINTS.checked_div(SURVEY_RAID_ROUNDS)?,
        Goal::Mine { .. }
        | Goal::Generator { .. }
        | Goal::Tap { .. }
        | Goal::Guard { .. }
        | Goal::Stone { .. } => 0,
    };
    let w = BALANCED;
    let utility = w
        .defence
        .checked_mul(defence)?
        .checked_add(w.economy.checked_mul(economy)?)?
        .checked_add(w.expansion.checked_mul(expansion)?)?
        .checked_add(w.capability.checked_mul(capability)?)?
        .checked_add(w.attack.checked_mul(attack)?)?
        .checked_sub(w.risk.checked_mul(risk)?)?;
    let utility_milli = utility.checked_mul(1_000)?.checked_add(served_milli)?;
    let rate = Rate::new(utility_milli, rate_ms.checked_mul(1_000)?)?;
    // Whole points, for the note: the worth rounded toward zero.
    let utility = utility_milli.checked_div(1_000)?;
    Some(Candidate {
        id,
        goal,
        place,
        time_ms,
        lead_ms: 0,
        walk_ms: travel_ms,
        on_site_ms: on_site,
        cost_dollars: cost,
        utility,
        rate,
    })
}

/// The top-k by utility per second, ties to the lowest id, of every
/// candidate worth something whose time fits `share` -- what the share holds
/// once whatever was funded ahead of the ranking has taken its part (fix pass
/// 2, item 3): a goal it cannot hold is no goal this round, and ranked among
/// the top-k it would only crowd out one that fits -- less the one funded
/// ahead of it (`ahead`, by id: the guard after the commander's death).
pub(crate) fn ranked(
    candidates: &[Candidate],
    share: i64,
    ahead: Option<usize>,
) -> Vec<&Candidate> {
    let mut ranked: Vec<&Candidate> = candidates
        .iter()
        .filter(|candidate| candidate.rate.is_positive())
        .filter(|candidate| candidate.time_ms <= share)
        .filter(|candidate| ahead != Some(candidate.id))
        .collect();
    ranked.sort_by_key(|candidate| (std::cmp::Reverse(candidate.rate), candidate.id));
    ranked.truncate(EASY_TOP_K);
    ranked
}

/// The guard post, when it was evaluated: what the wizard's Defend the Core
/// page is pre-filled with, whatever it is worth.
pub(crate) fn guard_site(candidates: &[Candidate]) -> Option<&Candidate> {
    candidates
        .iter()
        .find(|candidate| matches!(candidate.goal, Goal::Guard { .. }))
}

/// The guard post Easy funds ahead of the ranking in the round after a Push
/// in which its commander died: the evaluated one, when it is worth
/// something. Spec section 14: Easy "adds a Defend guard on its commander when
/// the commander has died recently", which decision 35 reads as the round
/// after a Push in which its commander died; decision 12's threat inside the
/// core's sphere scores it too, and ranks it in any other round.
pub(crate) fn guard(candidates: &[Candidate]) -> Option<&Candidate> {
    guard_site(candidates).filter(|candidate| candidate.rate.is_positive())
}

/// The raid the wizard's Raid an Enemy Beacon page is pre-filled with: the
/// first evaluated, which is the nearest target any route reaches (the
/// template's own "nearest enemy beacon you know of", with a commander in
/// sight weighed like one), whatever it is worth this round.
pub(crate) fn raid_site(candidates: &[Candidate]) -> Option<&Candidate> {
    candidates
        .iter()
        .find(|candidate| matches!(candidate.goal, Goal::Raid { .. }))
}

/// The best placement for one template among all that were evaluated, ties
/// to the lowest id: what the wizard is pre-filled with. A tap is not a
/// placement and no template parameter can say one, so it is never offered
/// to the wizard; nor is a stepping stone, which no template declares a
/// parameter for (the advisor weighs none, [`evaluate`]).
pub(crate) fn best_for<'c>(candidates: &'c [Candidate], template: &str) -> Option<&'c Candidate> {
    candidates
        .iter()
        .filter(|candidate| candidate.rate.is_positive() && candidate.goal.template() == template)
        .filter(|candidate| !matches!(candidate.goal, Goal::Tap { .. } | Goal::Stone { .. }))
        .min_by_key(|candidate| (std::cmp::Reverse(candidate.rate), candidate.id))
}
