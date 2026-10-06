// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The map's vents and seams as the planning surfaces read them, over the
//! hosted world (`docs/design/targeting.md`, "Surfaces"; S1's plan, task
//! `tgtw`).
//!
//! # The sim's resolver, lent
//!
//! "Nearest" is ranked where the pathing graph is: in the sim for the tick, and
//! here for previews. **Here does not mean a second resolver.** Every pick this
//! module answers comes from the sim's own functions over the world the gateway
//! hosts -- [`pharmakos_sim::targeting::cover`] for a `covering` site,
//! [`pharmakos_sim::targeting::on_vent`] for an `on` column, and
//! [`pharmakos_sim::targeting::Ranker`] for the order "nearest" puts
//! candidates in -- which is what makes "the gateway's pick equals the sim's"
//! true by construction (`crates/sim/src/targeting.rs`'s module docs say the
//! same from the other side). Nothing here steps, forks or runs a mandate
//! (AGENTS.md section 3 rule 2): it reads the tables, the voxels and the graph,
//! and estimates travel over them with a search scratch of its own.
//!
//! The ground this module reads is [`World::ground`] with one field replaced:
//! the evaluation-unit tally P1 counts a decision's work in. The world's own
//! tally is reset before every decision, so a gateway read could not have
//! moved a measurement, but a read that wrote nothing at all into the hosted
//! world is the simpler thing to state, so the gateway counts into its own
//! ([`lend`]).
//!
//! # Two filters, restated
//!
//! The sim's "which features does this pick consider" predicates for a
//! `covering` pick and for an `on` pick are private to its module
//! (`matches_pick` and `on_candidate` in `crates/sim/src/targeting.rs`). The
//! **pick** never needs them here -- it is the sim's own `cover` or `on_vent`
//! -- but `gp.api.v1.ResolvedRef` also lists every candidate in rank order
//! (the chip's "now" and "next") and counts how many matched before
//! reachability was asked (the recap's "3 matched, none reachable"), and both
//! of those need the filter. So the two predicates are restated below, from
//! the sim's public reads only, and the ranking they feed is still the sim's
//! [`Ranker`]. `tests/targeting.rs` holds the restatement to the sim's pick,
//! for a `covering` and for an `on {vent: NEAREST}` alike: the first listed
//! candidate is the feature the sim binds when the step starts. And the
//! module checks itself as it answers: a "nearest" pick the restated filter
//! did not list is refused as [`crate::error::Code::Internal`] rather than
//! answered, so a drift between the copy and the sim surfaces as an error and
//! never as a wrong chip. A sim change that makes the two predicates public
//! lets this module drop its copies; the pull request that brought this
//! module says so.

use pharmakos_proto::gp::v1::location::Place;
use pharmakos_proto::gp::v1::{
    BeaconRef, FeatureRef, InterfaceRow, InterfaceStep, MandateSettings, PlaceBeaconStep, Playbook,
    Step, Voxel,
};
use pharmakos_sim::features::{Feature, FeatureKind};
use pharmakos_sim::interpreter::{
    Action, BeaconSpec, Place as PlanPlace, Plan, StepFailure, beacon_spec_of, binding_slots,
    resolve_beacon_in,
};
use pharmakos_sim::math::fixed::{Fx, Sq};
use pharmakos_sim::math::quantity::Ms;
use pharmakos_sim::pathing::{Scratch, Speed, ticks_for_cost};
use pharmakos_sim::seams::UnitTally;
use pharmakos_sim::snapshot::Snapshot;
use pharmakos_sim::tables::{BeaconId, SeatId, StructureKind};
use pharmakos_sim::targeting::{FeatureSpec, Ground, Ranker, column_of, cover, on_vent};
use pharmakos_sim::voxels::Richness;
use pharmakos_sim::world::World;

use crate::error::Error;

/// One vent or seam, as a seat's planning surfaces read it.
///
/// The fields both `gp.api.v1.MapFeature` and the verifier's
/// [`pharmakos_verifier::KnownFeature`] carry, computed once here so the two
/// cannot spell one fact two ways.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FeatureRead {
    /// Its row in the world's feature table.
    pub index: usize,
    /// Its name: `vent_<x>_<y>` or `seam_<x>_<y>`.
    pub name: String,
    /// Vent or seam.
    pub kind: FeatureKind,
    /// The map generator's grade.
    pub grade: Richness,
    /// The anchor column's x.
    pub x: i32,
    /// The anchor column's y.
    pub y: i32,
    /// False once it is lost (the sim's [`World::feature_is_live`]).
    pub live: bool,
    /// The per-seat ordinal of the asking seat's own living beacon whose
    /// sphere holds the feature's anchor point, the lowest when several do;
    /// `None` when it is uncovered, and always `None` for a caller that is no
    /// seat.
    pub covered_by: Option<u32>,
}

/// The world's resolver view, counting its work into `tally` rather than into
/// the world's own decision counter (see the module docs).
#[must_use]
pub fn lend<'a>(world: &'a World, tally: &'a UnitTally) -> Ground<'a> {
    Ground {
        work: tally,
        ..world.ground()
    }
}

/// A search scratch sized for the world's pathing graph, for one read.
///
/// Built per call rather than kept: a scratch is search state the estimator
/// overwrites from scratch on every query (that is what lets the sim reuse
/// one across a whole match), and a fresh one is a few megabytes of zeroes on
/// the shipped map, which the methods that use it -- a preview per edit, a
/// summary per Lull -- can afford.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] when the rules table's cluster edge does
/// not size a scratch for this map, which the world's own construction would
/// already have refused.
pub fn scratch_for(world: &World) -> Result<Scratch, Error> {
    Scratch::for_map(world.surface(), world.rules().hpa_cluster_voxels())
        .ok_or_else(|| Error::internal("this map and cluster edge do not size a search scratch"))
}

/// Where `seat`'s commander stands, or `None` when it is dead: the origin a
/// `covering` description ranks from when its step starts (the sim's
/// `commander_at`, read the same way).
#[must_use]
pub fn commander_point(world: &World, seat: SeatId) -> Option<[Fx; 3]> {
    let commander = world.commander_of(seat);
    if !commander.is_some() {
        return None;
    }
    let index = usize::try_from(commander.raw()).ok()?;
    let alive = world
        .units()
        .hit_points()
        .get(index)
        .is_some_and(|hp| hp.is_alive());
    if !alive {
        return None;
    }
    world.units().positions().get(index).copied()
}

/// Whether `b` lies within `radius` of `a`: the sim's sphere test, squared
/// distances in Q32.32 and no square root (AGENTS.md section 4.2).
#[must_use]
pub fn within(a: [Fx; 3], b: [Fx; 3], radius: Fx) -> bool {
    Sq::between(a, b) <= Sq::of_radius(radius)
}

/// The beacon sphere's radius (`beacon.sphere_radius_voxels`), as the
/// fixed-point length [`within`] compares against.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] when the rules table's radius does not fit
/// a voxel coordinate. The sim's own sphere test clamps such a radius; the
/// gateway refuses instead, because a clamped radius here would be a preview
/// quietly disagreeing with a run.
pub fn sphere_radius(ground: &Ground<'_>) -> Result<Fx, Error> {
    i16::try_from(ground.sphere_radius())
        .map(Fx::from_voxels)
        .map_err(|_| Error::internal("the rules table's sphere radius is not a voxel length"))
}

/// The point a feature is measured and covered at: its anchor column's
/// standing point, or its first footprint column's should the generator have
/// stamped no anchor column (the sim's `anchor_point`).
#[must_use]
pub fn anchor_point(ground: &Ground<'_>, feature: &Feature) -> Option<[Fx; 3]> {
    let column = feature
        .anchor_column()
        .or_else(|| feature.footprint.first().copied())?;
    ground.standing(column.x, column.y)
}

/// The per-seat ordinal of `seat`'s own **living** beacon -- awake or
/// dormant, since a dormant beacon keeps its sphere -- whose sphere holds
/// `point`, the lowest when several do. Never reads another seat's beacons.
#[must_use]
pub fn covering_beacon(
    ground: &Ground<'_>,
    seat: SeatId,
    point: [Fx; 3],
    radius: Fx,
) -> Option<u32> {
    let beacons = ground.beacons;
    let rows = beacons.ids().len();
    (0..rows)
        .filter(|row| beacons.seats().get(*row).copied() == Some(seat.raw()))
        .filter(|row| {
            beacons
                .hit_points()
                .get(*row)
                .is_some_and(|hp| hp.is_alive())
        })
        .filter(|row| {
            beacons
                .positions()
                .get(*row)
                .copied()
                .is_some_and(|centre| within(centre, point, radius))
        })
        .filter_map(|row| beacons.ordinals().get(row).copied())
        .min()
}

/// Every feature on the map, in feature id order (`gp.api.v1.MapFeature`'s
/// order, and the order [`pharmakos_verifier::Scope`] keeps), with the asking
/// seat's coverage when there is one.
///
/// # Errors
///
/// As [`sphere_radius`].
pub fn feature_reads(world: &World, seat: Option<SeatId>) -> Result<Vec<FeatureRead>, Error> {
    let tally = UnitTally::new();
    let ground = lend(world, &tally);
    let radius = sphere_radius(&ground)?;
    let mut reads: Vec<FeatureRead> = ground
        .features
        .features()
        .iter()
        .enumerate()
        .map(|(index, feature)| {
            let [x, y] = feature.anchor;
            FeatureRead {
                index,
                name: feature.name(),
                kind: feature.kind,
                grade: feature.grade,
                x,
                y,
                live: ground.is_live(index),
                covered_by: seat.and_then(|seat| {
                    anchor_point(&ground, feature)
                        .and_then(|at| covering_beacon(&ground, seat, at, radius))
                }),
            }
        })
        .collect();
    // item 62: a name is unique per feature (the map generator refuses a
    // duplicate anchor), so the key is total.
    reads.sort_unstable_by(|left, right| left.name.cmp(&right.name));
    Ok(reads)
}

/// **The sim's `matches_pick`, restated** (see the module docs): whether the
/// feature at `index` passes a `covering` pick's filters -- its kind, alive,
/// and for `UNCOVERED` outside every sphere of `seat`'s own living beacons.
#[must_use]
pub fn matches_pick(
    ground: &Ground<'_>,
    seat: SeatId,
    index: usize,
    kind: FeatureKind,
    uncovered: bool,
) -> bool {
    let Some(feature) = ground.features.get(index) else {
        return false;
    };
    if feature.kind != kind || !ground.is_live(index) {
        return false;
    }
    if !uncovered {
        return true;
    }
    anchor_point(ground, feature).is_some_and(|point| !ground.inside_own_sphere(seat, point))
}

/// **The sim's `on_candidate`, restated** (see the module docs): whether the
/// vent at `index` is an `on` candidate for a beacon of `seat` centred at
/// `centre` -- a live vent no live Generator of any seat and no Build target
/// of this seat (but `except`'s) stands on, whose `on` column lies inside the
/// sphere of `radius` ([`sphere_radius`]).
#[must_use]
pub fn on_candidate(
    ground: &Ground<'_>,
    seat: SeatId,
    except: Option<BeaconId>,
    centre: [Fx; 3],
    radius: Fx,
    index: usize,
) -> bool {
    let Some(feature) = ground.features.get(index) else {
        return false;
    };
    if feature.kind != FeatureKind::Vent || !ground.is_live(index) {
        return false;
    }
    let taken = feature.footprint.iter().any(|column| {
        generator_on(ground, column.x, column.y)
            || ground.own_target_on(seat, except, column.x, column.y)
    });
    if taken {
        return false;
    }
    ground
        .on_column(index, seat, except)
        .and_then(|[x, y]| ground.standing(x, y))
        .is_some_and(|point| within(centre, point, radius))
}

/// Whether a live Generator of any seat stands on the column (the sim's
/// `generator_on`, restated for [`on_candidate`]).
fn generator_on(ground: &Ground<'_>, x: i32, y: i32) -> bool {
    let structures = ground.structures;
    let generator = StructureKind::Generator.id();
    (0..structures.ids().len()).any(|row| {
        structures.kinds().get(row).copied() == Some(generator)
            && structures
                .hit_points()
                .get(row)
                .is_some_and(|hp| hp.is_alive())
            && structures.positions().get(row).copied().is_some_and(|at| {
                at.first().map(|value| value.floor_voxels()) == Some(x)
                    && at.get(1).map(|value| value.floor_voxels()) == Some(y)
            })
    })
}

/// Every candidate `keep` admits, nearest first by the sim's [`Ranker`], with
/// the estimator's cost of each: `(feature index, cost)`. An unreachable
/// candidate is not listed (targeting.md, "Nearest").
pub fn ranked<F>(
    ground: &Ground<'_>,
    scratch: &mut Scratch,
    origin: [i32; 2],
    keep: F,
) -> Vec<(usize, i64)>
where
    F: FnMut(usize) -> bool,
{
    let mut ranker = Ranker::new(ground, origin, keep);
    let mut out: Vec<(usize, i64)> = Vec::new();
    while let Some(found) = ranker.next(ground, scratch) {
        out.push(found);
    }
    out
}

/// An estimator cost as the game milliseconds the commander takes to walk it:
/// the same `ceil(cost * 20 / cost_per_second)` ticks `estimate_route`'s legs
/// are in, so a chip's figure and a route's agree. `None` when the rules
/// table gives the commander no speed or the cost is out of range.
#[must_use]
pub fn travel_ms(world: &World, cost: i64) -> Option<Ms> {
    let speed = Speed::commander(world.rules());
    let cost = i32::try_from(cost).ok()?;
    let ticks = ticks_for_cost(cost, speed.cost_per_second)?;
    Some(Ms::from_ticks(u32::try_from(ticks).ok()?))
}

// ---------------------------------------------------------------------------
// resolve_refs: every FeatureRef of a playbook, read as its step would read it
// ---------------------------------------------------------------------------

/// Where a `gp.v1.FeatureRef` is written, which decides what it may say and
/// what it is read from (`docs/design/targeting.md`, "Sites").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Site {
    /// `place_beacon.at`'s `covering`: ranked from the commander's column.
    Covering,
    /// A Build target's `on`, ranked from the beacon the target is written
    /// into. `covering` says whether it is in the initial settings of a
    /// `place_beacon` whose `at` is a `covering` arm, the one place `covered
    /// {}` is legal.
    On {
        /// Inside a `covering` placement's initial settings.
        covering: bool,
    },
    /// `remove_build_target`'s `on {feature_id}`: a name, never ranked, and
    /// read when the row commits rather than when the step starts.
    Removal,
}

/// One candidate a reference could read, and how far it is from the
/// reference's origin.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Candidate {
    /// Its name.
    pub feature_id: String,
    /// Game milliseconds from the origin, by the estimator "nearest" ranks
    /// with.
    pub travel_ms: Ms,
}

/// What one `gp.v1.FeatureRef` reads now: one `gp.api.v1.ResolvedRef`, plus
/// what the Lull's "this round" sentence needs.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Resolved {
    /// RFC 6901 pointer to the `FeatureRef` in the file.
    pub pointer: String,
    /// The route step it is written in, from 0; `None` inside a handler body,
    /// which may never run.
    pub route_step: Option<usize>,
    /// Where it is written.
    pub site: Site,
    /// The kind it asks for, when the reference says (a name or a pick);
    /// `None` for `covered {}`.
    pub kind: Option<FeatureKind>,
    /// The feature it reads; empty when it reads nothing.
    pub feature_id: String,
    /// Travel from the origin to that feature, by the estimator "nearest"
    /// ranks with. `None` when it reads nothing, and when it reads a feature
    /// it **names** (a `feature_id`, or `covered {}`) that the origin cannot
    /// reach: a name is never ranked, so the sim binds it reachable or not.
    /// The wire's `travel_ms` has no "unreachable" value, so it is written 0
    /// there, beside a `feature_id` and an empty candidate list -- which is
    /// how a reader tells it from a feature at the origin (a reachable named
    /// feature is always its own one candidate).
    pub travel_ms: Option<Ms>,
    /// Every reachable candidate that matched, nearest first.
    pub candidates: Vec<Candidate>,
    /// How many features matched before reachability was asked.
    pub matched: u32,
    /// The step failure the sim would answer; `None` when it reads a feature.
    pub failure: Option<StepFailure>,
    /// For a `covering` that reads a feature: the column the beacon would
    /// stand on, as a whole voxel on the ground.
    pub site_voxel: Option<Voxel>,
}

/// A `gp.v1.FeatureRef` as the compiled spec the sim's resolver takes, for the
/// site it is written at.
///
/// The same reading `Plan::compile` gives the reference (`crates/sim`'s
/// `compile_feature_ref`): an unset rank is an error and never `NEAREST`;
/// under `covering` a coverage is required, under `on` it must be omitted and
/// only vents are ranked; `covered {}` is legal only in a `covering`
/// placement's initial settings; a removal names its target and never
/// describes it. A caller that has compiled the playbook first never sees the
/// error; `estimate_route`, which takes one `Location`, can.
///
/// # Errors
///
/// A sentence naming what is wrong with the reference.
pub fn spec_of(reference: &FeatureRef, site: Site) -> Result<FeatureSpec, String> {
    use pharmakos_proto::gp::v1::feature_ref::{Coverage, Rank, Ref};
    let pick = |kind: FeatureKind, rank: i32, coverage: i32| -> Result<FeatureSpec, String> {
        match Rank::try_from(rank) {
            Ok(Rank::Nearest) => {}
            Ok(Rank::Unspecified) => {
                return Err(String::from(
                    "a pick's `rank` is required, and S1's is `NEAREST`",
                ));
            }
            Err(_) => return Err(format!("`rank` {rank} is not a rank this build knows")),
        }
        let coverage = Coverage::try_from(coverage)
            .map_err(|_| format!("`coverage` {coverage} is not a coverage this build knows"))?;
        match site {
            Site::Covering => match coverage {
                Coverage::Unspecified => Err(String::from(
                    "under `covering` a pick's `coverage` is required: `ANY` or `UNCOVERED`",
                )),
                Coverage::Any => Ok(FeatureSpec::Nearest {
                    kind,
                    uncovered: false,
                }),
                Coverage::Uncovered => Ok(FeatureSpec::Nearest {
                    kind,
                    uncovered: true,
                }),
            },
            Site::On { .. } => {
                if coverage != Coverage::Unspecified {
                    return Err(String::from(
                        "`on` takes no `coverage`: it ranks the free vents inside the beacon's \
                         sphere",
                    ));
                }
                if kind != FeatureKind::Vent {
                    return Err(String::from(
                        "`on` ranks only vents: a Generator stands on a vent",
                    ));
                }
                Ok(FeatureSpec::Nearest {
                    kind,
                    uncovered: false,
                })
            }
            Site::Removal => Err(String::from(
                "a `remove_build_target` names its target `on {feature_id}`, never by a \
                 description",
            )),
        }
    };
    match reference.r#ref.as_ref() {
        Some(Ref::FeatureId(id)) => {
            let (kind, anchor) =
                pharmakos_sim::features::parse_feature_name(id).ok_or_else(|| {
                    format!("`{id}` is not a feature name: `vent_<x>_<y>` or `seam_<x>_<y>`")
                })?;
            Ok(FeatureSpec::Name { kind, anchor })
        }
        Some(Ref::Vent(vent)) => pick(FeatureKind::Vent, vent.rank, vent.coverage),
        Some(Ref::Seam(seam)) => pick(FeatureKind::Seam, seam.rank, seam.coverage),
        Some(Ref::Covered(_)) => match site {
            Site::On { covering: true } => Ok(FeatureSpec::Covered),
            Site::On { covering: false } | Site::Covering | Site::Removal => Err(String::from(
                "`covered {}` is legal only under `on`, in the initial settings of a \
                 `place_beacon` whose `at` is a `covering` arm",
            )),
        },
        None => Err(String::from(
            "a `FeatureRef` names a feature or describes one",
        )),
    }
}

/// The kind a spec asks for, when it says.
const fn kind_of(spec: FeatureSpec) -> Option<FeatureKind> {
    match spec {
        FeatureSpec::Name { kind, .. } | FeatureSpec::Nearest { kind, .. } => Some(kind),
        FeatureSpec::Covered => None,
    }
}

/// Everything one resolution reads, borrowed once.
struct Reader<'a> {
    world: &'a World,
    ground: Ground<'a>,
    scratch: Scratch,
    seat: SeatId,
    commander: Option<[Fx; 3]>,
    radius: Fx,
}

impl Reader<'_> {
    /// A feature's name, by index.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] for an index the feature table does
    /// not hold, which only a drift between the sim's answer and this
    /// module's reading could produce.
    fn name(&self, index: usize) -> Result<String, Error> {
        self.ground
            .features
            .get(index)
            .map(Feature::name)
            .ok_or_else(|| Error::internal("the sim answered a feature the table does not hold"))
    }

    /// The candidates `keep` admits, ranked from `origin`, named and timed.
    fn candidates<F>(&mut self, origin: [Fx; 3], keep: F) -> Result<Vec<(usize, Candidate)>, Error>
    where
        F: FnMut(usize) -> bool,
    {
        let found = ranked(&self.ground, &mut self.scratch, column_of(origin), keep);
        found
            .into_iter()
            .map(|(index, cost)| {
                let travel_ms = travel_ms(self.world, cost).ok_or_else(|| {
                    Error::internal("an estimate's cost does not convert to game milliseconds")
                })?;
                Ok((
                    index,
                    Candidate {
                        feature_id: self.name(index)?,
                        travel_ms,
                    },
                ))
            })
            .collect()
    }

    /// How many features `keep` admits, before reachability is asked.
    fn matched<F>(&self, keep: F) -> Result<u32, Error>
    where
        F: FnMut(&usize) -> bool,
    {
        u32::try_from((0..self.ground.features.len()).filter(keep).count())
            .map_err(|_| Error::internal("the feature table holds more rows than a count names"))
    }

    /// The pick's travel, from the listing it was ranked in.
    ///
    /// A **ranked** pick (`spec` is "nearest") is always listed: the sim
    /// ranked it with the same [`Ranker`] over the same filter, so a pick the
    /// listing lacks means the restated filter has drifted from the sim's, and
    /// that is refused rather than answered with a made-up travel. A **named**
    /// pick is listed only when the origin reaches it, and `None` says it
    /// does not ([`Resolved::travel_ms`]).
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] for a ranked pick the listing lacks.
    fn travel_of(
        listed: &[(usize, Candidate)],
        feature: usize,
        spec: FeatureSpec,
    ) -> Result<Option<Ms>, Error> {
        let found = listed
            .iter()
            .find(|(index, _)| *index == feature)
            .map(|(_, candidate)| candidate.travel_ms);
        match (spec, found) {
            (FeatureSpec::Nearest { .. }, None) => Err(Error::internal(
                "the sim ranked a feature the gateway's restated filter did not list: the two \
                 have drifted apart",
            )),
            (_, found) => Ok(found),
        }
    }

    /// A `covering` site, read as the sim reads it when the step starts: the
    /// pick is [`pharmakos_sim::targeting::cover`]'s. Returns the reference's
    /// answer, the feature it bound and the site's standing point.
    fn covering(
        &mut self,
        pointer: String,
        route_step: Option<usize>,
        spec: FeatureSpec,
    ) -> Result<CoveringRead, Error> {
        let mut resolved = empty(pointer, route_step, Site::Covering, kind_of(spec));
        let Some(commander) = self.commander else {
            resolved.failure = Some(StepFailure::CommanderDead);
            return Ok(CoveringRead::failed(resolved));
        };
        let seat = self.seat;
        let ground = self.ground;
        let (listed, matched) = match spec {
            FeatureSpec::Nearest { kind, uncovered } => (
                self.candidates(commander, |index| {
                    matches_pick(&ground, seat, index, kind, uncovered)
                })?,
                self.matched(|index| matches_pick(&ground, seat, *index, kind, uncovered))?,
            ),
            FeatureSpec::Name { kind, anchor } => {
                let named = ground
                    .features
                    .index_of(kind, anchor)
                    .filter(|index| ground.is_live(*index));
                match named {
                    Some(named) => (self.candidates(commander, |index| index == named)?, 1),
                    None => (Vec::new(), 0),
                }
            }
            FeatureSpec::Covered => (Vec::new(), 0),
        };
        resolved.matched = matched;
        resolved.candidates = listed
            .iter()
            .map(|(_, candidate)| candidate.clone())
            .collect();
        match cover(&ground, &mut self.scratch, seat, commander, spec) {
            Ok(found) => {
                let [x, y] = found.site;
                let point = ground
                    .standing(x, y)
                    .ok_or_else(|| Error::internal("the sim chose a covering site off the map"))?;
                resolved.feature_id = self.name(found.feature)?;
                resolved.travel_ms = Reader::travel_of(&listed, found.feature, spec)?;
                resolved.site_voxel = Some(crate::view::voxel_of(point));
                Ok(CoveringRead {
                    resolved,
                    feature: Some(found.feature),
                    point: Some(point),
                })
            }
            Err(failure) => {
                resolved.failure = Some(failure);
                Ok(CoveringRead::failed(resolved))
            }
        }
    }

    /// An `on` anchor, read as the sim reads it when the step starts: the
    /// pick is [`pharmakos_sim::targeting::on_vent`]'s, from the sphere the
    /// target is written into (`centre`, of the beacon `except`, or of a site
    /// not yet placed), with `covered` the feature a `covering` bound.
    fn on(
        &mut self,
        pointer: String,
        route_step: Option<usize>,
        at: OnAt,
    ) -> Result<Resolved, Error> {
        let OnAt {
            covering,
            spec,
            centre,
            except,
            covered,
        } = at;
        let mut resolved = empty(pointer, route_step, Site::On { covering }, kind_of(spec));
        let seat = self.seat;
        let ground = self.ground;
        let radius = self.radius;
        // A named vent, or the one `covered {}` names: a candidate when it is
        // a live vent at all, which is what "matched" counts for a name.
        let single = |index: Option<usize>| -> Option<usize> {
            index.filter(|held| {
                ground.is_live(*held)
                    && ground
                        .features
                        .get(*held)
                        .is_some_and(|feature| feature.kind == FeatureKind::Vent)
            })
        };
        let named = match spec {
            FeatureSpec::Nearest { .. } => None,
            FeatureSpec::Name { kind, anchor } => single(ground.features.index_of(kind, anchor)),
            FeatureSpec::Covered => single(covered),
        };
        let (listed, matched) = match (spec, named) {
            (FeatureSpec::Nearest { .. }, _) => (
                self.candidates(centre, |index| {
                    on_candidate(&ground, seat, except, centre, radius, index)
                })?,
                self.matched(|index| on_candidate(&ground, seat, except, centre, radius, *index))?,
            ),
            (_, Some(named)) => (self.candidates(centre, |index| index == named)?, 1),
            (_, None) => (Vec::new(), 0),
        };
        resolved.matched = matched;
        resolved.candidates = listed
            .iter()
            .map(|(_, candidate)| candidate.clone())
            .collect();
        match on_vent(
            &ground,
            &mut self.scratch,
            seat,
            centre,
            except,
            covered,
            spec,
        ) {
            Ok(found) => {
                resolved.feature_id = self.name(found.feature)?;
                resolved.travel_ms = Reader::travel_of(&listed, found.feature, spec)?;
            }
            Err(failure) => resolved.failure = Some(failure),
        }
        Ok(resolved)
    }

    /// A removal's `on {feature_id}`: a name, read when the row commits and
    /// never ranked. It reads the feature the table holds under that name,
    /// lost or not, because a removal is keyed by the name.
    fn removal(
        &self,
        pointer: String,
        route_step: Option<usize>,
        spec: FeatureSpec,
    ) -> Result<Resolved, Error> {
        let mut resolved = empty(pointer, route_step, Site::Removal, kind_of(spec));
        let found = match spec {
            FeatureSpec::Name { kind, anchor } => self.ground.features.index_of(kind, anchor),
            FeatureSpec::Nearest { .. } | FeatureSpec::Covered => None,
        };
        match found {
            Some(index) => {
                resolved.matched = 1;
                resolved.feature_id = self.name(index)?;
            }
            None => resolved.failure = Some(StepFailure::NoTarget),
        }
        Ok(resolved)
    }
}

/// What a `covering` read: the reference's answer, the feature it bound and
/// the site's standing point (both `None` when it failed).
#[derive(Clone, PartialEq, Eq, Debug)]
struct CoveringRead {
    resolved: Resolved,
    feature: Option<usize>,
    point: Option<[Fx; 3]>,
}

impl CoveringRead {
    /// A `covering` that read nothing.
    const fn failed(resolved: Resolved) -> CoveringRead {
        CoveringRead {
            resolved,
            feature: None,
            point: None,
        }
    }
}

/// Where an `on` anchor is read from.
#[derive(Clone, Copy, Debug)]
struct OnAt {
    /// Inside a `covering` placement's initial settings.
    covering: bool,
    /// What it asks for.
    spec: FeatureSpec,
    /// The centre of the sphere the target is written into.
    centre: [Fx; 3],
    /// The beacon that sphere is, when it stands already.
    except: Option<BeaconId>,
    /// The feature the step's `covering` bound, which `covered {}` names.
    covered: Option<usize>,
}

/// A reference that has read nothing yet.
fn empty(
    pointer: String,
    route_step: Option<usize>,
    site: Site,
    kind: Option<FeatureKind>,
) -> Resolved {
    Resolved {
        pointer,
        route_step,
        site,
        kind,
        feature_id: String::new(),
        travel_ms: None,
        candidates: Vec::new(),
        matched: 0,
        failure: None,
        site_voxel: None,
    }
}

/// A reference its step never reads, because the step failed before it got
/// there: it answers the failure its step ends on.
fn unread(
    pointer: String,
    route_step: Option<usize>,
    site: Site,
    kind: Option<FeatureKind>,
    failure: StepFailure,
) -> Resolved {
    let mut resolved = empty(pointer, route_step, site, kind);
    resolved.failure = Some(failure);
    resolved
}

/// Every `gp.v1.FeatureRef` in `playbook` -- each `covering` and each `on` --
/// in file order, read over the hosted world as the sim reads it when its step
/// starts, from where things stand now.
///
/// **A reading rule per site, each the sim's** (the step start in
/// `crates/sim/src/interpreter/exec.rs`):
///
/// * a `place_beacon` reads its `at` first -- a `covering` ranks from the
///   commander's column, a voxel stands on the ground, a beacon is resolved by
///   the sim's snapshot-level resolver -- and the site must be legal before
///   its initial settings' `on` anchors are read, each from that site;
/// * an `interface` resolves its beacon first, and reads each row's `on`
///   anchors from that beacon, in row order -- a `set_mandate` row's Build
///   targets among them, read like any other row's: the sim's `bind_rows`
///   reads a switch's carried settings at step start, from the same beacon
///   and with the same `except`, before any row commits, so the switch
///   clearing the old targets when it commits does not change the reading;
/// * a step stops reading at its first failure: a reference after it is never
///   read, and answers the failure its step would end on. A removal's name is
///   read when its row commits, so it never ends the step.
///
/// A step that comes after another step that moves the commander may read
/// differently when it runs: `gateway.proto` says so, and the chip says "read
/// when the step starts".
///
/// **`plan` is `playbook` as the sim compiled it**, and the reading is held to
/// it: the `covering` sites and the `on` anchors listed here are exactly the
/// ones the compiled plan binds when its steps start
/// ([`pharmakos_sim::interpreter::Row::binding_slots`]), counted, or the
/// answer is refused: a preview that read what the run never reads, or missed
/// what it does, would be a promise the run does not keep. A `set_mandate`
/// row is the case in point: the skeleton's sim compiled it as the writ alone
/// and bound nothing in it, and since S1's `mine` lane it carries the settings
/// it writes after the switch and binds their `on` anchors (decisions-log
/// item 131 (5)), so this walk lists them -- except where the sim compiles no
/// settings at all (an empty Defend or Attack arm), which has no Build target
/// to list either.
///
/// # Errors
///
/// [`crate::error::Code::InvalidArgument`] naming the pointer of a reference
/// this build cannot read where it is written, and
/// [`crate::error::Code::Internal`] when the map does not size a search, an
/// estimate does not convert, or the walk and the compiled plan disagree about
/// what the playbook binds.
pub fn resolve_playbook(
    world: &World,
    snapshot: &Snapshot,
    seat: SeatId,
    playbook: &Playbook,
    plan: &Plan,
) -> Result<Vec<Resolved>, Error> {
    let tally = UnitTally::new();
    let ground = lend(world, &tally);
    let mut reader = Reader {
        world,
        ground,
        scratch: scratch_for(world)?,
        seat,
        commander: commander_point(world, seat),
        radius: sphere_radius(&ground)?,
    };
    let mut out: Vec<Resolved> = Vec::new();
    let Some(declarative) = playbook.declarative.as_ref() else {
        return Ok(out);
    };
    for (index, step) in declarative.route.iter().enumerate() {
        let at = format!("/declarative/route/{index}");
        read_step(&mut reader, snapshot, &at, Some(index), step, &mut out)?;
    }
    for (handler, rule) in declarative.handlers.iter().enumerate() {
        for (index, step) in rule.body.iter().enumerate() {
            let at = format!("/declarative/handlers/{handler}/body/{index}");
            read_step(&mut reader, snapshot, &at, None, step, &mut out)?;
        }
    }
    let (coverings, anchors) = bindings_of(plan);
    let listed = |site: fn(Site) -> bool| out.iter().filter(|resolved| site(resolved.site)).count();
    let listed_coverings = listed(|site| site == Site::Covering);
    let listed_anchors = listed(|site| matches!(site, Site::On { .. }));
    if (listed_coverings, listed_anchors) != (coverings, anchors) {
        return Err(Error::internal(format!(
            "the gateway reads {listed_coverings} `covering` sites and {listed_anchors} `on` \
             anchors in this playbook, and the sim's compile binds {coverings} and {anchors}: \
             the two readings have drifted apart"
        )));
    }
    Ok(out)
}

/// How many `covering` sites and how many `on` anchors a compiled plan binds
/// when its steps start, route and handler bodies alike.
fn bindings_of(plan: &Plan) -> (usize, usize) {
    plan.route()
        .iter()
        .chain(plan.handlers().iter().flat_map(|rule| rule.body.iter()))
        .fold((0, 0), |(coverings, anchors), step| match &step.action {
            Action::PlaceBeacon { at, rows, .. } => (
                coverings.saturating_add(usize::from(matches!(at, PlanPlace::Covering(_)))),
                anchors.saturating_add(binding_slots(rows)),
            ),
            Action::Interface { rows, .. } => {
                (coverings, anchors.saturating_add(binding_slots(rows)))
            }
            Action::Move { .. }
            | Action::WaitUntil(_)
            | Action::Hold { .. }
            | Action::Broadcast => (coverings, anchors),
        })
}

/// The `on` anchors one set of mandate settings writes, with their pointers.
fn build_anchors<'a>(
    settings: Option<&'a MandateSettings>,
    at: &str,
) -> Vec<(String, &'a FeatureRef)> {
    use pharmakos_proto::gp::v1::mandate_settings::Mandate;
    let Some(Mandate::Build(build)) = settings.and_then(|held| held.mandate.as_ref()) else {
        return Vec::new();
    };
    build
        .targets
        .iter()
        .enumerate()
        .filter_map(
            |(index, target)| match target.anchor.as_ref()?.place.as_ref()? {
                Place::On(reference) => {
                    Some((format!("{at}/build/targets/{index}/anchor/on"), reference))
                }
                Place::Voxel(_)
                | Place::BeaconAnchor(_)
                | Place::Safest(_)
                | Place::Covering(_) => None,
            },
        )
        .collect()
}

/// The `FeatureRef`s one interface row writes, with their pointers and sites.
fn row_anchors<'a>(row: &'a InterfaceRow, at: &str) -> Vec<(String, &'a FeatureRef, Site)> {
    use pharmakos_proto::gp::v1::interface_row::Row;
    let on = Site::On { covering: false };
    let anchor_on =
        |location: Option<&'a pharmakos_proto::gp::v1::Location>| -> Option<&'a FeatureRef> {
            match location?.place.as_ref()? {
                Place::On(reference) => Some(reference),
                Place::Voxel(_)
                | Place::BeaconAnchor(_)
                | Place::Safest(_)
                | Place::Covering(_) => None,
            }
        };
    match row.row.as_ref() {
        Some(Row::SetMandateSettings(settings)) => {
            build_anchors(Some(settings), &format!("{at}/set_mandate_settings"))
                .into_iter()
                .map(|(pointer, reference)| (pointer, reference, on))
                .collect()
        }
        // A switch binds what the settings it carries bind (the sim's
        // `bind_rows`): its Build targets' `on` anchors, in list order, read
        // from the visited beacon like any other row's. Only a Build arm
        // carries targets, so an empty Defend or Attack arm, which the sim
        // compiles to no settings, lists nothing here either.
        Some(Row::SetMandate(settings)) => {
            build_anchors(Some(settings), &format!("{at}/set_mandate"))
                .into_iter()
                .map(|(pointer, reference)| (pointer, reference, on))
                .collect()
        }
        Some(Row::AddBuildTarget(add)) => anchor_on(
            add.target
                .as_ref()
                .and_then(|target| target.anchor.as_ref()),
        )
        .map(|reference| {
            vec![(
                format!("{at}/add_build_target/target/anchor/on"),
                reference,
                on,
            )]
        })
        .unwrap_or_default(),
        Some(Row::RemoveBuildTarget(remove)) => anchor_on(remove.anchor.as_ref())
            .map(|reference| {
                vec![(
                    format!("{at}/remove_build_target/anchor/on"),
                    reference,
                    Site::Removal,
                )]
            })
            .unwrap_or_default(),
        Some(Row::SetPriority(_) | Row::Recycle(_) | Row::QueueStructure(_)) | None => Vec::new(),
    }
}

/// One reference compiled for its site, or a refusal naming its pointer.
fn compiled(reference: &FeatureRef, site: Site, pointer: &str) -> Result<FeatureSpec, Error> {
    spec_of(reference, site).map_err(|why| Error::invalid(format!("{pointer}: {why}")))
}

/// One step's references.
fn read_step(
    reader: &mut Reader<'_>,
    snapshot: &Snapshot,
    at: &str,
    route_step: Option<usize>,
    step: &Step,
    out: &mut Vec<Resolved>,
) -> Result<(), Error> {
    use pharmakos_proto::gp::v1::step::Kind;
    match step.kind.as_ref() {
        Some(Kind::PlaceBeacon(place)) => {
            read_placement(reader, snapshot, at, route_step, place, out)
        }
        Some(Kind::Interface(interface)) => {
            read_interface(reader, snapshot, at, route_step, interface, out)
        }
        Some(Kind::Move(_) | Kind::WaitUntil(_) | Kind::Hold(_) | Kind::Broadcast(_)) | None => {
            Ok(())
        }
    }
}

/// A `place_beacon`'s references: its `at`, then its initial settings' `on`
/// anchors.
fn read_placement(
    reader: &mut Reader<'_>,
    snapshot: &Snapshot,
    at: &str,
    route_step: Option<usize>,
    place: &PlaceBeaconStep,
    out: &mut Vec<Resolved>,
) -> Result<(), Error> {
    let base = format!("{at}/place_beacon");
    let placement = place
        .at
        .as_ref()
        .and_then(|location| location.place.as_ref());
    let covering = matches!(placement, Some(Place::Covering(_)));
    // The site, the feature a `covering` bound, and the failure the step ends
    // on before it reads its initial settings, if it does.
    let (centre, covered, mut failure) = match placement {
        Some(Place::Covering(reference)) => {
            let pointer = format!("{base}/at/covering");
            let spec = compiled(reference, Site::Covering, &pointer)?;
            let read = reader.covering(pointer, route_step, spec)?;
            let failure = read.resolved.failure;
            out.push(read.resolved);
            (read.point, read.feature, failure)
        }
        Some(Place::Voxel(voxel)) => {
            let point = reader.ground.standing(voxel.x, voxel.y);
            (
                point,
                None,
                point.is_none().then_some(StepFailure::IllegalSite),
            )
        }
        Some(Place::BeaconAnchor(_) | Place::Safest(_)) => {
            let point = beacon_place(reader, snapshot, placement);
            (
                point,
                None,
                point.is_none().then_some(StepFailure::NoTarget),
            )
        }
        Some(Place::On(_)) | None => (None, None, Some(StepFailure::NoTarget)),
    };
    // The sim's `site_is_legal`, checked before any initial row is read: a
    // site inside one of the seat's own spheres, not on the column of one of
    // its own live beacons. A `covering` site is legal by construction.
    if failure.is_none()
        && let Some(site) = centre
    {
        let [x, y] = column_of(site);
        let legal = reader.ground.inside_own_sphere(reader.seat, site)
            && !reader.ground.stacks_on_own(reader.seat, x, y);
        if !legal {
            failure = Some(StepFailure::IllegalSite);
        }
    }
    let initial = place
        .initial
        .as_ref()
        .and_then(|initial| initial.mandate.as_ref());
    let site = Site::On { covering };
    for (pointer, reference) in build_anchors(initial, &format!("{base}/initial/mandate")) {
        let spec = compiled(reference, site, &pointer)?;
        let resolved = match (failure, centre) {
            (None, Some(centre)) => reader.on(
                pointer,
                route_step,
                OnAt {
                    covering,
                    spec,
                    centre,
                    except: None,
                    covered,
                },
            )?,
            (Some(failed), _) => unread(pointer, route_step, site, kind_of(spec), failed),
            (None, None) => unread(
                pointer,
                route_step,
                site,
                kind_of(spec),
                StepFailure::NoTarget,
            ),
        };
        if failure.is_none() {
            failure = resolved.failure;
        }
        out.push(resolved);
    }
    Ok(())
}

/// An `interface`'s references: each row's `on` anchors, read from the
/// beacon it visits.
fn read_interface(
    reader: &mut Reader<'_>,
    snapshot: &Snapshot,
    at: &str,
    route_step: Option<usize>,
    interface: &InterfaceStep,
    out: &mut Vec<Resolved>,
) -> Result<(), Error> {
    let base = format!("{at}/interface");
    let beacon = interface_beacon(reader, snapshot, interface.beacon.as_ref());
    let mut failure = beacon.is_none().then_some(StepFailure::NoTarget);
    for (index, row) in interface.rows.iter().enumerate() {
        for (pointer, reference, site) in row_anchors(row, &format!("{base}/rows/{index}")) {
            let spec = compiled(reference, site, &pointer)?;
            let resolved = match (site, failure, beacon) {
                (_, Some(failed), _) => unread(pointer, route_step, site, kind_of(spec), failed),
                (Site::Removal, None, _) => reader.removal(pointer, route_step, spec)?,
                (_, None, Some((id, centre))) => reader.on(
                    pointer,
                    route_step,
                    OnAt {
                        covering: false,
                        spec,
                        centre,
                        except: Some(id),
                        covered: None,
                    },
                )?,
                (_, None, None) => unread(
                    pointer,
                    route_step,
                    site,
                    kind_of(spec),
                    StepFailure::NoTarget,
                ),
            };
            // A removal is read when its row commits, not when the step
            // starts, so its answer never ends the step.
            if failure.is_none() && site != Site::Removal {
                failure = resolved.failure;
            }
            out.push(resolved);
        }
    }
    Ok(())
}

/// Where a `place_beacon` whose `at` names a beacon would stand: that
/// beacon's anchor, resolved by the sim's snapshot-level resolver from where
/// the commander stands. `None` when it resolves to nothing.
fn beacon_place(
    reader: &Reader<'_>,
    snapshot: &Snapshot,
    placement: Option<&Place>,
) -> Option<[Fx; 3]> {
    let spec = match placement? {
        Place::BeaconAnchor(reference) => beacon_spec_of(reference, reader.world.rules()).ok()?,
        Place::Safest(_) => BeaconSpec::Safest,
        Place::Voxel(_) | Place::On(_) | Place::Covering(_) => return None,
    };
    let id = resolve_beacon_in(snapshot, reader.world.rules(), reader.seat, spec, None)?;
    beacon_point(reader.world, id)
}

/// The beacon an `interface` step visits, and where it stands: the sim's
/// snapshot-level resolver, from where the commander stands. `None` when it
/// resolves to nothing -- hidden, absent and someone else's alike, which is
/// one `no_target` (`docs/design/targeting.md`, "Failure").
fn interface_beacon(
    reader: &Reader<'_>,
    snapshot: &Snapshot,
    reference: Option<&BeaconRef>,
) -> Option<(BeaconId, [Fx; 3])> {
    let spec = beacon_spec_of(reference?, reader.world.rules()).ok()?;
    let id = resolve_beacon_in(snapshot, reader.world.rules(), reader.seat, spec, None)?;
    Some((id, beacon_point(reader.world, id)?))
}

/// Where a beacon stands, by id (a beacon's id is its row in the table).
fn beacon_point(world: &World, id: BeaconId) -> Option<[Fx; 3]> {
    let row = usize::try_from(id.raw()).ok()?;
    let beacons = world.beacons();
    if beacons.ids().get(row).copied() != Some(id.raw()) {
        return None;
    }
    beacons.positions().get(row).copied()
}
