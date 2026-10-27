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
//! drops it and the wizard is offered its site; it is **added** to a built-in
//! seat's route beside the round's goals, not ranked among them, whenever its
//! utility is above zero ([`guard`]; [`crate::compose`]).
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
//! the round's goals like any other ([`top_k`]); every raid names the same
//! post seam, so the composition's one-goal-per-feature rule places at most
//! one a round. Every other goal's attack is zero.
//!
//! # The term that is zero at Easy, and why
//!
//! **Capability** (no licence or capability is on any wire until S4) is zero
//! for every candidate. It is in the sum, with its weight, so a later stage
//! fills a term rather than change a formula. A placed beacon adds no draw (since
//! T14b a beacon is net zero through its own key-core, decisions-log item 113
//! (5)), which S1's plan, decision 14, made the reading of the register's
//! S1-14, so no goal is charged a kW shortfall.

use pharmakos_proto::json::Json;

use crate::easy::{
    BALANCED, COMMANDER_DIED_POINTS, DEFENCE_POINTS_PER_HP_PER_SECOND, DOLLARS_PER_KW,
    EASY_CANDIDATES, EASY_TOP_K, EXPANSION_POINTS_PER_BEACON, RAID_POINTS, RAID_TARGETS_MAX,
    RISK_POINTS_PER_ENEMY,
};
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
    /// The template this goal fills.
    pub(crate) const fn template(&self) -> &'static str {
        match self {
            Goal::Mine { .. } => crate::easy::EXPAND_AND_MINE,
            Goal::Generator { .. } | Goal::Tap { .. } => crate::easy::HOLD_AND_BUILD,
            Goal::Guard { .. } => crate::easy::DEFEND_THE_CORE,
            Goal::Raid { .. } => crate::easy::RAID_AN_ENEMY_BEACON,
        }
    }

    /// The feature the goal is about, by name: one goal per feature in a
    /// composed route.
    pub(crate) fn feature(&self) -> &str {
        match self {
            Goal::Mine { seam, .. } | Goal::Guard { seam } | Goal::Raid { seam, .. } => seam,
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
    /// Game milliseconds the goal costs the route from the core: travel,
    /// bounded as there and back for a site the commander leaves again, plus
    /// the interface time the rules table gives the change.
    pub(crate) time_ms: i64,
    /// Game milliseconds from the commander to the core, which a composed
    /// route visits first ([`crate::compose::deepen_core`]): the same for
    /// every candidate of a round, and 0 when the seat is shown no core.
    pub(crate) lead_ms: i64,
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

/// The taps the gateway says are legal, at most two calls: one
/// `resolve_refs` and one QUICK `verify_plan` over a probe playbook holding
/// every candidate pair, and none when there is no pair.
///
/// A pair is legal when its `on` reads the vent it names from that beacon
/// (`resolve_refs`: the sim's own `on_vent` rules) and the verifier raises no
/// `E0503` at its step (the beacon is on the Build mandate). A refusal of
/// either call leaves no tap: the operator cannot then say a tap would hold,
/// so it composes none. An answer that does not read ([`Misread`]), and an
/// answer to `verify_plan` that carries no `report`, are refusals too:
/// without them the operator cannot say the `on` reads or no `E0503` was
/// raised.
pub(crate) fn legal_taps(
    wire: &mut Wire<'_, '_>,
    situation: &Situation,
    tuning: &Tuning,
) -> Vec<Tap> {
    let pairs = tap_pairs(situation, tuning);
    if pairs.is_empty() {
        return Vec::new();
    }
    match probe(wire, &pairs) {
        Ok(legal) => pairs
            .into_iter()
            .zip(legal)
            .filter(|(_, legal)| *legal)
            .map(|((beacon, vent), _)| Tap { beacon, vent })
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// The probe's two calls, and which pairs they leave legal.
///
/// # Errors
///
/// The first refusal, and `MALFORMED` for an answer that does not read.
fn probe(wire: &mut Wire<'_, '_>, pairs: &[(String, Feature)]) -> Result<Vec<bool>, Refused> {
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
    let verified = wire.call(
        "verify_plan",
        object(vec![
            ("playbook_jsonc", string(&text)),
            ("depth", string("quick")),
        ]),
    )?;
    let report = verified
        .get("report")
        .ok_or_else(|| malformed("verify_plan", "the answer carries no report"))?;
    let diagnostics = |misread: Misread| misread.refused("verify_plan");
    for diagnostic in array_of(report, "diagnostics").map_err(diagnostics)? {
        if text_of(diagnostic, "code").map_err(diagnostics)? != "E0503" {
            continue;
        }
        if let Some(slot) = route_index(text_of(diagnostic, "path").map_err(diagnostics)?)
            .and_then(|index| legal.get_mut(index))
        {
            *slot = false;
        }
    }
    Ok(legal)
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
) -> Vec<Goal> {
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
    // The guard goes first, so that the cut never drops it, and the rest are
    // cut to the room it leaves.
    let guard = guard_goal(situation, tuning, post);
    let raids = raid_goals(situation, tuning);
    // The guard and the raids go first, so that the cut never drops them, and
    // the rest are cut to the room they leave.
    let goals: Vec<Goal> = guard
        .into_iter()
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
pub(crate) fn defend_post(wire: &mut Wire<'_, '_>, situation: &Situation, tuning: &Tuning) -> Post {
    let (Some(commander), Some(core)) = (situation.commander, situation.core()) else {
        return Post::Unknown;
    };
    if !situation
        .templates
        .iter()
        .any(|listed| listed == crate::easy::DEFEND_THE_CORE)
    {
        return Post::Unknown;
    }
    let nearest_defend = object(vec![(
        "beacon_anchor",
        object(vec![(
            "nearest",
            object(vec![(
                "filter",
                object(vec![("mandate", string("DEFEND"))]),
            )]),
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
                Some(Ok(at)) if within(core.at, at, tuning.sphere_radius_voxels) => Post::Stands,
                Some(Ok(_)) => Post::None,
                // An answer that does not say where the beacon stands.
                Some(Err(_)) | None => Post::Unknown,
            }
        }
    }
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
    post_seam(situation, tuning).map(|seam| Goal::Guard {
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
fn post_seam<'s>(situation: &'s Situation, tuning: &Tuning) -> Option<&'s Feature> {
    let core = situation.core()?;
    let [cx, cy, _] = core.at;
    let radius = tuning.sphere_radius_voxels;
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
/// the core, ties to the lowest seat, then the beacon's name, cut at
/// [`RAID_TARGETS_MAX`]. Nothing when the library lists no Raid template, the
/// seat has no post seam, or the treasury cannot carry the beacon and its
/// raiders.
fn raid_goals(situation: &Situation, tuning: &Tuning) -> Vec<Goal> {
    let (Some(core), Some(seam)) = (situation.core(), post_seam(situation, tuning)) else {
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
    for enemy in &situation.enemies {
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

/// What one `estimate_route` answer says of a goal: the place its last leg
/// ends on, that leg's travel, and the travel before it; `None` when the
/// route does not reach.
///
/// # Errors
///
/// [`Misread`] for an answer that does not read, and for a reachable route
/// with no leg, a leg with no place, or a figure below zero.
fn reading(estimate: &Json) -> Result<Option<([i32; 3], i64, i64)>, Misread> {
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
fn raid_reading(estimate: &Json) -> Result<Option<([i32; 3], i64, i64)>, Misread> {
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
            Goal::Mine { seam: name, .. }
            | Goal::Generator { vent: name, .. }
            | Goal::Guard { seam: name } => object(vec![(
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
        };
        let mut waypoints = vec![crate::wire::voxel_location(commander)];
        if let Some(core) = situation.core() {
            waypoints.push(object(vec![(
                "beacon_anchor",
                object(vec![("beacon_id", string(&core.id))]),
            )]));
        }
        waypoints.push(to);
        if let Goal::Raid { target, .. } = &goal {
            waypoints.push(target.waypoint());
        }
        let params = object(vec![("waypoints", Json::Array(waypoints))]);
        let Ok(estimate) = wire.call("estimate_route", params) else {
            continue;
        };
        let read = match goal {
            Goal::Raid { .. } => raid_reading(&estimate),
            Goal::Mine { .. } | Goal::Generator { .. } | Goal::Tap { .. } | Goal::Guard { .. } => {
                reading(&estimate)
            }
        };
        let Ok(Some((place, travel, lead))) = read else {
            continue;
        };
        if let Some(mut candidate) = score(situation, tuning, id, goal, place, travel) {
            candidate.lead_ms = lead;
            out.push(candidate);
        }
    }
    out
}

/// The summed threat of the other seats' assets the view shows inside the
/// core's sphere, in thousandths of a hit point per second (decision 12's
/// model: each asset's weapon's damage per second, zero if unarmed); `None`
/// when the seat is shown no core, or a sum does not fit.
pub(crate) fn core_threat(situation: &Situation, tuning: &Tuning) -> Option<u64> {
    threat_near(situation, tuning, situation.core()?.at)
}

/// The summed threat of the other seats' assets the view shows within one
/// sphere radius of `at`, in thousandths of a hit point per second (decision
/// 12's model, as [`core_threat`] reads it); `None` when a sum does not fit.
fn threat_near(situation: &Situation, tuning: &Tuning, at: [i32; 3]) -> Option<u64> {
    situation
        .enemies
        .iter()
        .filter(|enemy| within(at, enemy.at, tuning.sphere_radius_voxels))
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
fn raid_attack(situation: &Situation, tuning: &Tuning, target: &RaidTarget) -> Option<i64> {
    let threat = i64::try_from(threat_near(situation, tuning, target.at())?).ok()?;
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
        LastPush::Survived | LastPush::Unread => 0,
    };
    from_threat.checked_add(from_death)
}

/// The time a post beside the core costs the route: there, place, then the
/// priority row and the mandate's own row (Defend's or Attack's), costed at
/// the edit's cap, an upper bound.
fn post_ms(tuning: &Tuning, travel_ms: i64) -> Option<i64> {
    travel_ms
        .checked_add(tuning.place_beacon_deploy_ms)?
        .checked_add(tuning.set_priority_ms)?
        .checked_add(tuning.edit_settings_max_ms)
}

/// One goal's features and Balanced utility; `None` when a figure does not
/// fit an `i64`, which no rules table the operator reads can produce, or the
/// goal takes no time, which no row the operator reads can make it (every
/// interface time is refused at zero, [`Tuning::read`]).
fn score(
    situation: &Situation,
    tuning: &Tuning,
    id: usize,
    goal: Goal,
    place: [i32; 3],
    travel_ms: i64,
) -> Option<Candidate> {
    let (cost, economy, expansion, defence, time_ms) = match &goal {
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
                0,
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
                0,
                travel_ms.checked_add(tuning.build_target_ms)?,
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
                // As the guard's: the Attack row in place of the Defend row.
                post_ms(tuning, travel_ms)?,
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
                post_ms(tuning, travel_ms)?,
            )
        }
    };
    // Risk: what the seat can see of other seats near the goal.
    //
    // PLACEHOLDER: "near" is within one sphere radius of the goal, the sphere radius reused as a threat radius — owner, S5, with the Balanced weights
    // A guard post is charged none: the enemies near it are what it guards
    // against, and they are its defence term already.
    let near = match goal {
        Goal::Guard { .. } => 0,
        Goal::Mine { .. } | Goal::Generator { .. } | Goal::Tap { .. } | Goal::Raid { .. } => {
            situation
                .enemies
                .iter()
                .filter(|enemy| within(place, enemy.at, tuning.sphere_radius_voxels))
                .count()
        }
    };
    let risk = i64::try_from(near)
        .ok()?
        .checked_mul(RISK_POINTS_PER_ENEMY)?;
    // Capability is zero at Easy, and attack is a raid's alone (module docs).
    let capability = 0_i64;
    let attack = match &goal {
        Goal::Raid { target, .. } => raid_attack(situation, tuning, target)?,
        Goal::Mine { .. } | Goal::Generator { .. } | Goal::Tap { .. } | Goal::Guard { .. } => 0,
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
    let rate = utility.checked_mul(1_000)?.checked_div(time_ms)?;
    Some(Candidate {
        id,
        goal,
        place,
        time_ms,
        lead_ms: 0,
        cost_dollars: cost,
        utility,
        rate,
    })
}

/// The top-k by utility per second, ties to the lowest id, less any
/// candidate worth nothing and less the guard, which is added beside the
/// goals rather than ranked among them ([`guard`]).
pub(crate) fn top_k(candidates: &[Candidate]) -> Vec<Candidate> {
    let mut ranked: Vec<Candidate> = candidates
        .iter()
        .filter(|candidate| candidate.utility > 0)
        .filter(|candidate| !matches!(candidate.goal, Goal::Guard { .. }))
        .cloned()
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

/// The guard post Easy adds to its route: the evaluated one, when it is worth
/// something. Spec section 14: Easy "adds a Defend guard on its commander when
/// the commander has died recently", which decision 35 reads as the round
/// after a Push in which its commander died, and decision 12's threat inside
/// the core's sphere scores it too.
pub(crate) fn guard(candidates: &[Candidate]) -> Option<&Candidate> {
    guard_site(candidates).filter(|candidate| candidate.utility > 0)
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
/// to the wizard.
pub(crate) fn best_for<'c>(candidates: &'c [Candidate], template: &str) -> Option<&'c Candidate> {
    candidates
        .iter()
        .filter(|candidate| candidate.utility > 0 && candidate.goal.template() == template)
        .filter(|candidate| !matches!(candidate.goal, Goal::Tap { .. }))
        .min_by_key(|candidate| (std::cmp::Reverse(candidate.rate), candidate.id))
}
