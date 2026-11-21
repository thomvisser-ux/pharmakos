// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The resolver: where a description lands (`docs/design/targeting.md`,
//! "Nearest" and "Sites"; decisions-log item 127 (12)).
//!
//! Everything here **reads**. A step's `covering` site, a Build target's `on`
//! column and a held description's re-read are all answered from a [`Ground`]
//! -- the world's tables, its voxels, its pathing graph and its feature table,
//! borrowed read-only -- plus a search [`Scratch`] for the estimator. The sim
//! asks it from the decision phase; the gateway asks the same functions over
//! its hosted world with its own scratch (`resolve_refs`), which is what makes
//! "the gateway's pick equals the sim's" true by construction rather than by
//! agreement. The predicates a pick filters by -- [`matches_pick`],
//! [`on_candidate`], [`generator_on`] and the sphere test [`anchor_in_sphere`]
//! -- are public for the same reason, so a preview counts candidates with the
//! sim's own rule rather than a restatement of it (decisions-log items 133 (3)
//! (j) and 134 (2) (b)). Nothing here steps, forks or runs a mandate (AGENTS.md
//! section 3 rule 2).
//!
//! # Nearest
//!
//! # Two measures
//!
//! Every closeness the resolver ranks is a cost over the walkers' step rule,
//! exact for what it means (lane `p3fix`):
//!
//! - **A choice a unit then walks to** -- `cover`, `on_vent`, a Mine
//!   beacon's seam, any "nearest" from an origin a walker leaves -- ranks by
//!   **the walk the router will serve** that walker: [`Ranker`], below. It is
//!   priced under no surcharge because no walker of a ranked choice is served
//!   one: a commander's route is surcharged only while a `move` walks at
//!   `AVOID_KNOWN_THREATS`, and the compile refuses `covering` and `toward`
//!   in a `move`, so a ranked site is walked to by a `place_beacon`; the
//!   drones walk to a seam or a target with no surcharge at all.
//!   `tests/nearest.rs`'s `ranked_choices_are_walked_without_a_surcharge`
//!   pins that premise, and whoever widens the vocabulary meets it there.
//! - **A choice of closeness that no unit walks** -- `toward`'s stepping
//!   stone, the legal site closest to the feature it steps toward -- ranks by
//!   **the least cost of any walk over the terrain**, the grid optimum
//!   ([`crate::pathing::reach`]). Nobody walks from the stone to the feature
//!   when the stone is chosen (the commander walks to the stone), so no
//!   served walk is the measure; the grid optimum is exactly "closest over
//!   the ground", with no HPA\* artefact in it.
//!
//! # Nearest
//!
//! **The least travel from the origin to the feature. Unreachable candidates
//! are skipped. Ties go to the lowest anchor y, then x.** The metric is the
//! integer cost of the walk the router serves
//! ([`crate::pathing::estimate::walk_cost`]), near or far, so "nearest" is
//! the feature the walker reaches soonest, by construction and not by an
//! estimate's agreement with the walk (lane `p3fix`; before it, a far pair was
//! ranked at the abstract estimate, which charges the unsmoothed refinement,
//! and the order stepped where the near and far pricing met). It measures to
//! the feature's anchor column, or, when that column cannot be stood on or is
//! not connected to the origin, to the first standable connected footprint
//! column in `(y, x)` order.
//!
//! [`Ranker`] prices **at most one walk per candidate**, and only for a
//! candidate the octile lower bound has not already ruled out: a candidate is
//! emitted once its key `(cost, y, x)` is below every unpriced candidate's
//! `(bound, y, x)`, and because the octile bound never exceeds a served walk's
//! cost (`tests/targeting.rs` pins it), that is the candidate's true place in
//! the order. Candidates are opened in octile order, which opens exactly the
//! candidates whose bound is under the emitted cost: no order that prunes by
//! the octile bound alone opens fewer. Nothing is capped; what bounds the work
//! is the candidate count ([`MAX_FEATURES`]) and the router's own search.
//!
//! In S1 liveness and reachability read the **live** world, unfogged, as the
//! adopted design accepts for this stage (targeting.md, "Descriptions";
//! decisions-log item 108 (1)); S3 bases both on the seat's terrain knowledge.
//!
//! # The sphere first
//!
//! A named or `covered {}` `on` first tests the vent's **anchor point** (its
//! anchor column's standing point, [`Ground::anchor_point`]) against the
//! target sphere, and answers `no_target` outside it whatever stands on the
//! vent: no structure can move that point, so no structure outside the
//! target sphere decides a named vent (decisions-log item 133 (3) (c); item
//! 134). That is the claim, and no more: the target sphere is the one the
//! target is written into, which for a beacon not yet placed (`place_beacon`)
//! is centred at its site and may reach past every sphere the seat holds,
//! and the structure tests inside it read the whole footprint, whose columns
//! may lie just outside it. S1 is unfogged (item 108 (1)); S3's fog work
//! bases these reads on the seat's knowledge.
//! [`Ground::on_column`] cannot be that first test, because it picks a free
//! column by reading structures. Inside the sphere the structure tests follow
//! and answer `illegal_site`.
//!
//! # The count
//!
//! The counted forms [`cover_counted`] and [`on_vent_counted`] answer a miss
//! as a [`StepFailed`]: the reason, and how many candidates matched before
//! reachability was asked (the meaning of `resolve_refs`'s `matched`), which
//! the `step_failed` event carries so a recap can say "3 matched, none
//! reachable" (item 133 (3) (h)). [`cover`] and [`on_vent`] are the same
//! reads with the reason alone.
//!
//! # Allocation
//!
//! None: the ranker is a fixed array of [`MAX_FEATURES`] entries, the spiral's
//! offsets are computed once at construction ([`spiral_offsets`]) and each
//! ring is searched in place, and the estimator's buffers are the scratch's
//! (`tests/allocations.rs`).

use crate::features::{Feature, FeatureKind, FeatureTable, MAX_FEATURES};
use crate::interpreter::state::{StepFailed, StepFailure};
use crate::math::fixed::{Fx, Sq};
use crate::pathing::clusters::Clusters;
use crate::pathing::estimate::{ServedWalk, WalkError, walk_cost};
use crate::pathing::search::Scratch;
use crate::pathing::surface::{Node, Surface};
use crate::rules::RulesTable;
use crate::tables::{BeaconId, BeaconTable, SeatId, StructureTable, TargetKind, TargetTable};
use crate::voxels::VoxelStore;

/// What a playbook's `FeatureRef` asks for, compiled.
///
/// One type for both sites: `covering` takes a [`FeatureSpec::Name`] or a
/// [`FeatureSpec::Nearest`] of either kind; `on` takes a name, the nearest
/// vent, or [`FeatureSpec::Covered`] (`crate::interpreter::Plan::compile`
/// refuses the rest).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FeatureSpec {
    /// `feature_id`: a vent or seam by name.
    Name {
        /// The kind the name spells.
        kind: FeatureKind,
        /// The anchor column the name spells.
        anchor: [i32; 2],
    },
    /// A pick: `NEAREST` of a kind, optionally only those `UNCOVERED`.
    Nearest {
        /// Vents or seams.
        kind: FeatureKind,
        /// Only features outside every sphere of the seat's own living
        /// beacons, awake or dormant.
        uncovered: bool,
    },
    /// `covered {}`: the feature the beacon being written was placed to
    /// cover. Binds as a name at deploy.
    Covered,
}

impl FeatureSpec {
    /// The wire id of how a Build target was written, the `desc` its row
    /// carries ([`TargetTable::descriptions`]): a fixed voxel is 0, a name
    /// (and `covered {}`, which binds as one) is 1, and a description is 2.
    #[must_use]
    pub const fn description_id(self) -> u8 {
        match self {
            FeatureSpec::Name { .. } | FeatureSpec::Covered => DESCRIPTION_NAME,
            FeatureSpec::Nearest { .. } => DESCRIPTION_NEAREST_VENT,
        }
    }
}

/// A Build target written as a fixed voxel.
pub const DESCRIPTION_VOXEL: u8 = 0;

/// A Build target written as a name: it idles when its feature is lost.
pub const DESCRIPTION_NAME: u8 = 1;

/// A Build target written as "the nearest vent": it reads again when its
/// feature is lost (`docs/design/targeting.md`, "Three reading rules", 2).
pub const DESCRIPTION_NEAREST_VENT: u8 = 2;

/// "No feature": the value a Build target written as a voxel carries in its
/// feature column, and a step with no binding carries in its own.
pub const NO_FEATURE: u32 = u32::MAX;

/// Everything the resolver reads, borrowed from one world.
///
/// Read-only throughout; the estimator's scratch is passed beside it, which is
/// what lets a caller holding only a `&World` (a gateway preview) bring its
/// own.
#[derive(Clone, Copy, Debug)]
pub struct Ground<'a> {
    /// The pathing surface: standability and each column's top.
    pub surface: &'a Surface,
    /// The HPA\* decomposition: the connectivity oracle and the estimator's
    /// graph.
    pub clusters: &'a Clusters,
    /// The voxels, for a feature's liveness.
    pub voxels: &'a VoxelStore,
    /// Every vent and seam on the map.
    pub features: &'a FeatureTable,
    /// The beacons: spheres, and no stacking.
    pub beacons: &'a BeaconTable,
    /// The structures: one structure per voxel.
    pub structures: &'a StructureTable,
    /// The Build targets: a seat's own claims.
    pub targets: &'a TargetTable,
    /// The rules table: the sphere radius and the step costs.
    pub rules: &'a RulesTable,
    /// The spiral's offsets ([`spiral_offsets`]).
    pub spiral: &'a [[i32; 2]],
    /// Where the work a resolution does is counted: P1's evaluation units
    /// (S1's plan, section 5). Not hashed.
    pub work: &'a crate::seams::UnitTally,
}

impl Ground<'_> {
    /// Count `units` of evaluation work.
    fn charge(&self, units: u32) {
        self.work.add(units);
    }

    /// The **base** sphere radius, `beacon.sphere_radius_voxels`, which the
    /// rules table refuses at load unless it is a voxel length
    /// ([`RulesTable::sphere_radius_voxels`]): the sphere of a site where no
    /// beacon stands yet, which no Resonance Spire can have grown (S3's plan,
    /// decision 6). A placed beacon's is [`Ground::radius_of`].
    #[must_use]
    pub fn sphere_radius(&self) -> i32 {
        i32::from(self.rules.sphere_radius_voxels())
    }

    /// Whether `point` lies within the **base** sphere radius of `centre`:
    /// the sphere of a site where no beacon stands yet ([`Ground::sphere_radius`]).
    /// Squared distances in Q32.32, no square root (AGENTS.md section 4.2).
    #[must_use]
    pub fn in_sphere(&self, centre: [Fx; 3], point: [Fx; 3]) -> bool {
        let radius = Fx::from_voxels(self.rules.sphere_radius_voxels());
        Sq::between(centre, point) <= Sq::of_radius(radius)
    }

    /// The sphere radius of `beacon`, grown by a finished Resonance Spire
    /// ([`crate::spire::radius_of`]; S3's plan, E3), or the base for `None`,
    /// a site where no beacon stands yet.
    #[must_use]
    pub fn radius_of(&self, beacon: Option<BeaconId>) -> Fx {
        Fx::from_voxels(match beacon {
            Some(beacon) => {
                crate::spire::radius_of(self.rules, self.beacons, self.structures, beacon)
            }
            None => self.rules.sphere_radius_voxels(),
        })
    }

    /// Whether `point` lies inside the sphere centred on `centre` of `beacon`
    /// (its grown radius, [`Ground::radius_of`]), or of a site where no beacon
    /// stands yet for `None`.
    #[must_use]
    pub fn in_sphere_of(&self, beacon: Option<BeaconId>, centre: [Fx; 3], point: [Fx; 3]) -> bool {
        Sq::between(centre, point) <= Sq::of_radius(self.radius_of(beacon))
    }

    /// The point a unit or a structure stands at on the column `(x, y)`: one
    /// above its top solid voxel. `None` off the map.
    #[must_use]
    pub fn standing(&self, x: i32, y: i32) -> Option<[Fx; 3]> {
        let node = self.surface.node_of(x, y)?;
        Some([
            Fx::from_voxels(i16::try_from(x).ok()?),
            Fx::from_voxels(i16::try_from(y).ok()?),
            Fx::from_voxels(i16::try_from(self.surface.standing_z(node)).ok()?),
        ])
    }

    /// Whether the feature at `index` is still there
    /// ([`FeatureTable::is_live`]).
    #[must_use]
    pub fn is_live(&self, index: usize) -> bool {
        self.features.is_live(index, self.surface, self.voxels)
    }

    /// Whether `point` lies inside the sphere of one of `seat`'s own living
    /// beacons, **awake or dormant**: a dormant beacon keeps its sphere.
    #[must_use]
    pub fn inside_own_sphere(&self, seat: SeatId, point: [Fx; 3]) -> bool {
        // Each beacon's own radius, grown by a finished Spire (decision 6: the
        // anchor for placing the next beacon grows).
        self.beacons
            .seats()
            .iter()
            .zip(self.beacons.hit_points())
            .zip(self.beacons.positions())
            .enumerate()
            .filter(|(_, ((owner, hp), _))| **owner == seat.raw() && hp.is_alive())
            .any(|(row, (_, centre))| {
                crate::spire::inside_at_row(
                    self.rules,
                    self.beacons,
                    self.structures,
                    row,
                    *centre,
                    point,
                )
            })
    }

    /// **No stacking**: whether one of `seat`'s own live beacons stands on the
    /// column `(x, y)` (`docs/design/targeting.md`, "Companion changes").
    #[must_use]
    pub fn stacks_on_own(&self, seat: SeatId, x: i32, y: i32) -> bool {
        self.own_live_beacons(seat).any(|at| same_column(at, x, y))
    }

    /// Where each of `seat`'s own living beacons stands, in row order.
    fn own_live_beacons(&self, seat: SeatId) -> impl Iterator<Item = [Fx; 3]> + '_ {
        self.beacons
            .seats()
            .iter()
            .zip(self.beacons.hit_points())
            .zip(self.beacons.positions())
            .filter(move |((owner, hp), _)| **owner == seat.raw() && hp.is_alive())
            .map(|(_, at)| *at)
    }

    /// **One structure per voxel**: whether a structure with hit points left
    /// occupies the column `(x, y)`, whoever owns it -- a ruin included, since
    /// a ruin keeps its hit points in S1. The owner column is not read; a
    /// structure at zero hit points frees its column.
    #[must_use]
    pub fn structure_on(&self, x: i32, y: i32) -> bool {
        self.structures
            .hit_points()
            .iter()
            .zip(self.structures.positions())
            .any(|(hp, at)| hp.is_alive() && same_column(*at, x, y))
    }

    /// Whether a Build target of one of `seat`'s own beacons -- `except`
    /// excluded -- is anchored on the column `(x, y)`. A queued target is a
    /// per-seat claim, so another seat's stays hidden.
    #[must_use]
    pub fn own_target_on(&self, seat: SeatId, except: Option<BeaconId>, x: i32, y: i32) -> bool {
        let build = TargetKind::Build.id();
        self.targets
            .beacons()
            .iter()
            .zip(self.targets.kinds())
            .zip(self.targets.anchors())
            .any(|((beacon, kind), at)| {
                if except.is_some_and(|skip| skip == *beacon) || *kind != build {
                    return false;
                }
                // The target's beacon, read through its id's row and checked
                // by generation (S2's plan, E1): an id that names no row the
                // table still holds is no beacon of this seat's.
                let owned = self
                    .beacons
                    .row_of(*beacon)
                    .ok()
                    .and_then(|row| self.beacons.seats().get(row.index()).copied())
                    == Some(seat.raw());
                owned && same_column(*at, x, y)
            })
    }

    /// The feature's **`on` column**: its anchor column if it is free,
    /// otherwise the next free footprint column in `(y, x)` order. Free means
    /// no live structure stands on it and no Build target of `seat` (but
    /// `except`'s) claims it. `None` when no footprint column is free.
    #[must_use]
    pub fn on_column(
        &self,
        index: usize,
        seat: SeatId,
        except: Option<BeaconId>,
    ) -> Option<[i32; 2]> {
        let feature = self.features.get(index)?;
        let free = |x: i32, y: i32| -> bool {
            !self.structure_on(x, y) && !self.own_target_on(seat, except, x, y)
        };
        if let Some(column) = feature.anchor_column()
            && free(column.x, column.y)
        {
            return Some([column.x, column.y]);
        }
        feature
            .footprint
            .iter()
            .find(|column| free(column.x, column.y))
            .map(|column| [column.x, column.y])
    }

    /// The point a feature is measured and covered at: its anchor column's
    /// standing point (or its first footprint column's, should the anchor
    /// column be missing). No structure moves it. `None` when the feature is
    /// not in the table or the column is off the map.
    #[must_use]
    pub fn anchor_point(&self, index: usize) -> Option<[Fx; 3]> {
        let feature = self.features.get(index)?;
        let column = feature
            .anchor_column()
            .or_else(|| feature.footprint.first().copied())?;
        self.standing(column.x, column.y)
    }

    /// The column a "nearest" measures to from `origin`: the anchor column if
    /// it can be stood on and is connected to the origin, otherwise the first
    /// standable connected footprint column in `(y, x)` order. `None` when no
    /// footprint column is reachable, which makes the feature no candidate.
    /// Public so that a reader pricing the same walk another way (the
    /// gateway's `estimate_route`, in its agreement test) names the column
    /// this rule names rather than restating it.
    #[must_use]
    pub fn measure_to(&self, index: usize, origin: Node) -> Option<Node> {
        let feature = self.features.get(index)?;
        let reachable = |x: i32, y: i32| -> Option<Node> {
            let node = self.surface.node_of(x, y)?;
            (self.surface.walkable(node) && self.clusters.connected(self.surface, origin, node))
                .then_some(node)
        };
        if let Some(column) = feature.anchor_column()
            && let Some(node) = reachable(column.x, column.y)
        {
            return Some(node);
        }
        feature
            .footprint
            .iter()
            .find_map(|column| reachable(column.x, column.y))
    }

    /// The integer cost of the walk the router serves from `from` to `to`
    /// ([`walk_cost`]): what a choice a unit then walks to ranks by
    /// ([`Ranker`]; the module docs' "Two measures").
    fn travel(&self, scratch: &mut Scratch, from: Node, to: Node) -> Result<ServedWalk, WalkError> {
        self.charge(1);
        walk_cost(self.surface, self.clusters, scratch, (from, to))
    }
}

/// Whether a point floors onto the column `(x, y)`.
fn same_column(at: [Fx; 3], x: i32, y: i32) -> bool {
    let [ax, ay, _] = at;
    ax.floor_voxels() == x && ay.floor_voxels() == y
}

/// The column a point floors onto.
#[must_use]
pub fn column_of(at: [Fx; 3]) -> [i32; 2] {
    let [x, y, _] = at;
    [x.floor_voxels(), y.floor_voxels()]
}

/// One candidate of a [`Ranker`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Entry {
    /// The feature's index in the table.
    feature: usize,
    /// Its anchor y, then x: the tie-break.
    y: i32,
    x: i32,
    /// Where it is measured to.
    goal: Node,
    /// The octile lower bound on its cost.
    bound: i64,
    /// Its served walk's cost, once priced.
    cost: i64,
    /// Its served walk's partial routes, once priced.
    partials: u32,
    /// 0 unpriced, 1 priced, 2 emitted or unreachable.
    state: u8,
}

/// "Nearest", one candidate at a time, in `(cost, anchor y, anchor x)` order.
///
/// Each candidate costs at most one priced walk, and only when the bounds of
/// the others cannot already place it (the module docs).
#[derive(Clone, Debug)]
pub struct Ranker {
    entries: [Entry; MAX_FEATURES],
    len: usize,
    /// The origin's node; `None` when the origin column is off the map, which
    /// reaches nothing.
    origin: Option<Node>,
    /// How many features `keep` admitted, before reachability was asked.
    matched: u32,
}

impl Ranker {
    /// The candidates `keep` admits, ranked from the column `origin`.
    ///
    /// A candidate with no reachable footprint column is dropped here: an
    /// unreachable feature is not a candidate (targeting.md, "Nearest"). It
    /// still counts in [`Ranker::matched`], which is asked before
    /// reachability. The feature table holds at most [`MAX_FEATURES`] rows
    /// (`FeatureTable::new` refuses more), so every admitted, reachable
    /// feature has an entry.
    pub fn new<F>(ground: &Ground<'_>, origin: [i32; 2], mut keep: F) -> Ranker
    where
        F: FnMut(usize) -> bool,
    {
        let [ox, oy] = origin;
        let start = ground.surface.node_of(ox, oy);
        let mut ranker = Ranker {
            entries: [Entry::default(); MAX_FEATURES],
            len: 0,
            origin: start,
            matched: 0,
        };
        for (index, feature) in ground.features.features().iter().enumerate() {
            ground.charge(1);
            if !keep(index) {
                continue;
            }
            ranker.matched += 1;
            if let Some(start) = start
                && let Some(goal) = ground.measure_to(index, start)
                && let Some(slot) = ranker.entries.get_mut(ranker.len)
            {
                let [x, y] = feature.anchor;
                *slot = Entry {
                    feature: index,
                    y,
                    x,
                    goal,
                    bound: i64::from(ground.surface.heuristic(start, goal)),
                    cost: 0,
                    partials: 0,
                    state: 0,
                };
                ranker.len += 1;
            }
        }
        ranker
    }

    /// Whether no candidate is reachable: nothing for [`Ranker::next`] to
    /// emit.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// How many features `keep` admitted, reachable or not: the count a
    /// `step_failed` carries and `resolve_refs` calls `matched`.
    #[must_use]
    pub const fn matched(&self) -> u32 {
        self.matched
    }

    /// The next candidate, nearest first, with its served walk (ranked by its
    /// cost); `None` when every candidate has been emitted or found
    /// unreachable.
    pub fn next(
        &mut self,
        ground: &Ground<'_>,
        scratch: &mut Scratch,
    ) -> Option<(usize, ServedWalk)> {
        let origin = self.origin?;
        loop {
            let live = self.entries.get(..self.len)?;
            // item 62: anchors are unique per feature, so both keys below end
            // in a unique `(y, x)`.
            let best_priced = live
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.state == 1)
                // item 62: `(y, x)` is a unique anchor.
                .min_by_key(|(_, entry)| (entry.cost, entry.y, entry.x))
                .map(|(at, entry)| (at, (entry.cost, entry.y, entry.x)));
            let best_bound = live
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.state == 0)
                // item 62: `(y, x)` is a unique anchor.
                .min_by_key(|(_, entry)| (entry.bound, entry.y, entry.x))
                .map(|(at, entry)| (at, (entry.bound, entry.y, entry.x)));
            match (best_priced, best_bound) {
                // The best priced candidate is ahead of every bound still
                // unpriced, so its place in the order is final.
                (Some((at, key)), bound) if bound.is_none_or(|(_, other)| key < other) => {
                    let entry = self.entries.get_mut(at)?;
                    entry.state = 2;
                    let walk = ServedWalk {
                        cost: entry.cost,
                        partials: entry.partials,
                    };
                    return Some((entry.feature, walk));
                }
                (_, Some((at, _))) => {
                    let entry = self.entries.get_mut(at)?;
                    match ground.travel(scratch, origin, entry.goal) {
                        Ok(walk) => {
                            entry.cost = walk.cost;
                            entry.partials = walk.partials;
                            entry.state = 1;
                        }
                        // The router serves no walk to it: no candidate
                        // (targeting.md, "Nearest": unreachable candidates
                        // are skipped).
                        Err(WalkError::NoPath(_)) => entry.state = 2,
                        // A chain whose queries stop falling is a walk no
                        // walker finishes: no candidate, and counted under
                        // its own reason (`Scratch::stalled`), never read as
                        // unreachable. The chain's argument says none occurs
                        // (`walk_cost`, "The chain ends").
                        Err(WalkError::Stalled { .. }) => {
                            entry.state = 2;
                            scratch.stalled.bump();
                        }
                    }
                }
                // Nothing priced is waiting and nothing is left to price (a
                // priced candidate with no bound left is emitted by the first
                // arm).
                (_, None) => return None,
            }
        }
    }
}

/// Every `[dx, dy]` within `radius`, in `(dx² + dy², dy, dx)` order: the
/// spiral a `covering` site is searched along, computed once at
/// construction so a decision allocates nothing. A negative radius has no
/// offsets.
#[must_use]
pub fn spiral_offsets(radius: i32) -> Vec<[i32; 2]> {
    // Before `-radius`, which `i32::MIN` would overflow.
    if radius < 0 {
        return Vec::new();
    }
    let limit = i64::from(radius) * i64::from(radius);
    let mut out: Vec<[i32; 2]> = Vec::new();
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if ring_key([dx, dy]) <= limit {
                out.push([dx, dy]);
            }
        }
    }
    // item 62: the key ends in `dx`, unique within a `dy` row, so it is total.
    out.sort_unstable_by_key(|&[dx, dy]| (ring_key([dx, dy]), dy, dx));
    out
}

/// The `covering` site for `feature`: the spiral's first column, in order of
/// squared distance from the feature's anchor, then squared distance from the
/// commander's column, then y, then x, that is a **legal site** (inside one of
/// `seat`'s own spheres, not on the column of one of its own live beacons),
/// standable, outside every feature's footprint, **reachable** by the
/// commander, and whose sphere holds the feature's `on` column
/// (`docs/design/targeting.md`, "Sites") **and** its anchor point
/// ([`anchor_in_sphere`]). `None` when no such column exists.
///
/// The anchor point is the sphere test a `covered {}` `on` asks first, so a
/// site whose sphere held the `on` column alone (a structure on the anchor
/// column moves the `on` column off it) would bind a `covering` its own
/// `covered {}` then answers `no_target`, and leave the vent reading as
/// `UNCOVERED` after the beacon lands. Where the anchor column is free the
/// two points are one and the test adds nothing.
///
/// Each ring of the spiral (the offsets at one squared distance) is searched
/// in place for its least `(distance from the commander, y, x)` among the
/// columns that pass, so no ring is ever truncated to fit a buffer.
#[must_use]
pub fn covering_site(
    ground: &Ground<'_>,
    seat: SeatId,
    commander: [Fx; 3],
    feature: usize,
) -> Option<[i32; 2]> {
    let [ax, ay] = ground.features.get(feature)?.anchor;
    let [ox, oy] = ground.on_column(feature, seat, None)?;
    let on_point = ground.standing(ox, oy)?;
    let anchor_point = ground.anchor_point(feature)?;
    let [cx, cy] = column_of(commander);
    let start = ground.surface.node_of(cx, cy)?;

    for ring in ground
        .spiral
        .chunk_by(|left, right| ring_key(*left) == ring_key(*right))
    {
        let mut best: Option<(i64, i32, i32)> = None;
        for &[dx, dy] in ring {
            ground.charge(1);
            // An offset that overflows a coordinate names no column.
            let (Some(x), Some(y)) = (ax.checked_add(dx), ay.checked_add(dy)) else {
                continue;
            };
            let Some(node) = ground.surface.node_of(x, y) else {
                continue;
            };
            let Some(site) = ground.standing(x, y) else {
                continue;
            };
            if !ground.surface.walkable(node)
                || ground.features.at_column(x, y).is_some()
                || !ground.in_sphere(site, on_point)
                || !ground.in_sphere(site, anchor_point)
                || !ground.inside_own_sphere(seat, site)
                || ground.stacks_on_own(seat, x, y)
                || !ground.clusters.connected(ground.surface, start, node)
            {
                continue;
            }
            let dxc = i64::from(x) - i64::from(cx);
            let dyc = i64::from(y) - i64::from(cy);
            // item 62: the key ends in `(y, x)`, unique per column, so it is
            // total.
            let key = (dxc * dxc + dyc * dyc, y, x);
            if best.is_none_or(|held| key < held) {
                best = Some(key);
            }
        }
        if let Some((_, y, x)) = best {
            return Some([x, y]);
        }
    }
    None
}

/// An offset's squared distance, the key the spiral's rings share.
fn ring_key(offset: [i32; 2]) -> i64 {
    let [dx, dy] = offset.map(i64::from);
    dx * dx + dy * dy
}

/// What `covering` resolved to: the feature and the site column.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cover {
    /// The feature the beacon will cover.
    pub feature: usize,
    /// The site column the commander walks to and deploys on.
    pub site: [i32; 2],
}

/// Resolve a `covering` site for `seat`, ranked from the commander's column
/// when the step starts.
///
/// [`cover_counted`] with the reason alone.
///
/// # Errors
///
/// `no_target` when a name is absent or lost, or a description matches
/// nothing reachable; `illegal_site` when candidates match but none has a
/// legal site (targeting.md's failure table).
pub fn cover(
    ground: &Ground<'_>,
    scratch: &mut Scratch,
    seat: SeatId,
    commander: [Fx; 3],
    spec: FeatureSpec,
) -> Result<Cover, StepFailure> {
    cover_counted(ground, scratch, seat, commander, spec).map_err(|failed| failed.reason)
}

/// [`cover`], with how many candidates matched on a miss.
///
/// # Errors
///
/// As [`cover`], counted ([`StepFailed::counted`]): a name or `covered {}`
/// counts 1 when it names a live feature of its kind and 0 otherwise, and a
/// description counts what [`matches_pick`] admitted, reachable or not.
pub fn cover_counted(
    ground: &Ground<'_>,
    scratch: &mut Scratch,
    seat: SeatId,
    commander: [Fx; 3],
    spec: FeatureSpec,
) -> Result<Cover, StepFailed> {
    match spec {
        FeatureSpec::Name { kind, anchor } => {
            let feature = ground
                .features
                .index_of(kind, anchor)
                .filter(|index| ground.is_live(*index))
                .ok_or(StepFailed::counted(StepFailure::NoTarget, 0))?;
            let site = covering_site(ground, seat, commander, feature)
                .ok_or(StepFailed::counted(StepFailure::IllegalSite, 1))?;
            Ok(Cover { feature, site })
        }
        FeatureSpec::Nearest { kind, uncovered } => {
            let mut ranker = Ranker::new(ground, column_of(commander), |index| {
                matches_pick(ground, seat, index, kind, uncovered)
            });
            let mut reached = false;
            while let Some((feature, _)) = ranker.next(ground, scratch) {
                reached = true;
                if let Some(site) = covering_site(ground, seat, commander, feature) {
                    return Ok(Cover { feature, site });
                }
            }
            let reason = if reached {
                StepFailure::IllegalSite
            } else {
                StepFailure::NoTarget
            };
            Err(StepFailed::counted(reason, ranker.matched()))
        }
        // `covered {}` is legal only under `on` (the compile refuses it here).
        FeatureSpec::Covered => Err(StepFailed::counted(StepFailure::NoTarget, 0)),
    }
}

/// One legal site of a `toward` search: its column and the node it stands
/// on, each column once. Kept in a buffer the caller owns; the world's own is
/// reserved for every column of the map, the most a search can list, so a
/// decision that searches allocates nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct StoneCandidate {
    y: i32,
    x: i32,
    node: Node,
}

impl StoneCandidate {
    /// The site's column, `[x, y]`.
    #[must_use]
    pub const fn column(&self) -> [i32; 2] {
        [self.x, self.y]
    }

    /// The node the site stands on.
    #[must_use]
    pub const fn node(&self) -> Node {
        self.node
    }
}

/// What `toward` resolved to: the feature it steps toward and the site
/// column the commander walks to and deploys on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stone {
    /// The feature the stone steps toward.
    pub feature: usize,
    /// The site column.
    pub site: [i32; 2],
    /// How many legal sites the search weighed for it: what a miss would
    /// count, kept on a hit for the measurement of the search's cost.
    pub weighed: u32,
}

/// The stepping-stone site toward `feature` (S3's plan, E23; S1's plan,
/// decision 3; task `pred`): **the legal site, inside `seat`'s spheres,
/// nearest the feature**, where "nearest" is targeting's (the module docs):
/// the **least walking cost over the terrain** from the candidate site to the
/// feature's anchor column ([`Ground::measure_to`]): the grid optimum over
/// the walkers' own step rule at the rules' step costs, not a served walk
/// (the module docs' "Two measures": nobody walks from the stone to the
/// feature when it is chosen). A candidate from which the feature cannot be
/// reached is skipped; ties go to the lowest site y, then x. Straight-line
/// distance is not used.
///
/// A candidate is a column within the sphere of one of `seat`'s own living
/// beacons (its own radius, grown by a finished Spire), that is a **legal
/// site** (inside one of the seat's own spheres, not on the column of one of
/// its own live beacons), standable, walkable, outside every feature's
/// footprint and connected to the commander's column, exactly as
/// [`covering_site`] admits a column. One search outward from the feature,
/// directed at the spheres that listed a site ([`crate::pathing::reach`],
/// "Directed"), settles columns in ascending `cost + h`, and
/// [`closest_site`] takes the least `(cost, y, x)` among the sites settled
/// until the key passes the least cost: a resolve pops only the columns whose
/// key is under the stone's cost, at most the map's, and computes one `h` a
/// column it reaches, each costing one octile a sphere that listed a site.
///
/// **The terrain is the live world's, unfogged** (`Fog::Clear`): E23's "the
/// terrain the seat knows" reads every column until S3-05's terrain mask
/// lands at S4 (S3's plan, decision 2; decisions-log item 156 (3)), as every
/// other read of targeting's does (decisions-log item 108 (1)).
///
/// Answers the stone and how many legal sites it weighed; or that there is
/// no legal site, before any search; or that the feature is out of the
/// commander's reach, before any search ([`Sited`]).
fn stone_site(
    ground: &Ground<'_>,
    scratch: &mut Scratch,
    (start, weighed): (Node, u32),
    feature: usize,
) -> Sited {
    // Every site is connected to the commander's column, so all of them
    // measure to the one column the commander would: the feature's anchor
    // column, or its first footprint column a walker reaches. A feature the
    // commander cannot reach is no target, and nothing is searched.
    let Some(goal) = ground.measure_to(feature, start) else {
        return Sited::Unreached(weighed);
    };
    match closest_site(scratch, ground.surface, goal) {
        Some(site) => Sited::Stone(site, weighed),
        // E7: cannot miss: every site is in the commander's component and so is the goal, so the search settles a site before it runs out
        None => Sited::Unreached(weighed),
    }
}

/// **The stone's rule, its one home**: of the sites marked in the scratch's
/// search, the one closest to `goal` over the terrain, ties to the lowest
/// `(y, x)`; `None` when the search reaches none.
///
/// The grid optimum from the goal outward, directed at the spheres added
/// ([`crate::pathing::reach`], "Directed"): every column settles at its least
/// cost, in ascending key `cost + h`, and a site's key is its cost. So the
/// first site settled is at the least cost `c`, and every site at `c` settles
/// before a key past `c`; the search promises no order among equal keys, so
/// the least `(cost, y, x)` is taken here, over every site settled until the
/// key passes `c`.
pub(crate) fn closest_site(
    scratch: &mut Scratch,
    surface: &Surface,
    goal: Node,
) -> Option<[i32; 2]> {
    scratch.reach.begin_toward(surface, goal);
    let mut best: Option<(i64, i32, i32)> = None;
    while let Some((node, cost)) = scratch.reach.next(surface, &mut scratch.expansions) {
        if let Some((least, y, x)) = best
            && scratch.reach.reached() > least
        {
            return Some([x, y]);
        }
        if scratch.reach.is_site(node) {
            let (x, y) = surface.coord_of(node);
            if best.is_none_or(|held| (cost, y, x) < held) {
                best = Some((cost, y, x));
            }
        }
    }
    best.map(|(_, y, x)| [x, y])
}

/// Every legal site of a `toward` for `seat`'s commander at `commander`
/// (the columns [`stone_site`] weighs), each marked once in the scratch and
/// listed in `candidates`, and each own sphere added for the directed
/// search: the commander's node and how many sites there are, or `None`
/// when the commander stands on no column. The set is the same for every
/// feature, so a resolve makes it once, before any search.
fn legal_sites(
    ground: &Ground<'_>,
    scratch: &mut Scratch,
    candidates: &mut Vec<StoneCandidate>,
    seat: SeatId,
    commander: [Fx; 3],
) -> Option<(Node, u32)> {
    candidates.clear();
    scratch.reach.clear_sites();
    let [cx, cy] = column_of(commander);
    let start = ground.surface.node_of(cx, cy)?;
    // Every legal site, once: a column two spheres hold is one site. Each is
    // a distinct column of the map, whose count is a `u32`, so the count
    // cannot pass it.
    let mut weighed: u32 = 0;
    let beacons = ground.beacons;
    for (((owner, hp), centre), id) in beacons
        .seats()
        .iter()
        .zip(beacons.hit_points())
        .zip(beacons.positions())
        .zip(beacons.ids())
    {
        if *owner != seat.raw() || !hp.is_alive() {
            continue;
        }
        let radius = ground.radius_of(Some(BeaconId::new(*id))).floor_voxels();
        let limit = i64::from(radius) * i64::from(radius);
        let [bx, by] = column_of(*centre);
        // This sphere's own sites: how many, and the most octile cost from
        // its centre to any of them, its reach for the directed search.
        let mut listed = false;
        let mut reach: i64 = 0;
        let costs = ground.surface.costs();
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if i64::from(dx) * i64::from(dx) + i64::from(dy) * i64::from(dy) > limit {
                    continue;
                }
                ground.charge(1);
                let (Some(x), Some(y)) = (bx.checked_add(dx), by.checked_add(dy)) else {
                    continue;
                };
                let Some(node) = ground.surface.node_of(x, y) else {
                    continue;
                };
                let Some(site) = ground.standing(x, y) else {
                    continue;
                };
                if !ground.surface.walkable(node)
                    || ground.features.at_column(x, y).is_some()
                    || !ground.inside_own_sphere(seat, site)
                    || ground.stacks_on_own(seat, x, y)
                    || !ground.clusters.connected(ground.surface, start, node)
                    || !scratch.reach.mark_site(node)
                {
                    continue;
                }
                candidates.push(StoneCandidate { y, x, node });
                weighed += 1;
                listed = true;
                reach = reach.max(i64::from(costs.octile(dx, dy)));
            }
        }
        // A sphere that listed no site heads the directed search nowhere.
        if listed {
            scratch.reach.add_sphere([bx, by], reach);
        }
    }
    Some((start, weighed))
}

/// What [`stone_site`] found, given legal sites to weigh.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Sited {
    /// The stone's column, and the legal sites weighed.
    Stone([i32; 2], u32),
    /// Legal sites, weighed, but the feature is not one the commander can
    /// reach. A `nearest` never ranks such a feature.
    Unreached(u32),
}

/// Resolve a `toward` site for `seat` (S3's plan, E23; task `pred`): the
/// feature it names, or the one its pick ranks first from the commander's
/// column as a `covering`'s does, and the stepping-stone site toward it
/// ([`stone_site`]). A pick whose nearest feature has no site tries the next,
/// as `covering` does. `candidates` is the caller's buffer.
///
/// # Errors
///
/// `no_target` when a name is absent or lost, or a description matches
/// nothing reachable, counted as [`cover_counted`] counts it; `illegal_site`
/// when a feature was found and no legal site reaches it, counted with **how
/// many legal sites it weighed** (S1's plan, decision 3: "a `toward` that
/// finds no site fails its step and says why, with how many sites it
/// weighed").
pub fn toward_counted(
    ground: &Ground<'_>,
    scratch: &mut Scratch,
    candidates: &mut Vec<StoneCandidate>,
    seat: SeatId,
    commander: [Fx; 3],
    spec: FeatureSpec,
) -> Result<Stone, StepFailed> {
    match spec {
        FeatureSpec::Name { kind, anchor } => {
            let feature = ground
                .features
                .index_of(kind, anchor)
                .filter(|index| ground.is_live(*index))
                .ok_or(StepFailed::counted(StepFailure::NoTarget, 0))?;
            // No legal site: refused before any search.
            let sites = legal_sites(ground, scratch, candidates, seat, commander)
                .filter(|(_, weighed)| *weighed > 0)
                .ok_or(StepFailed::counted(StepFailure::IllegalSite, 0))?;
            match stone_site(ground, scratch, sites, feature) {
                Sited::Stone(site, weighed) => Ok(Stone {
                    feature,
                    site,
                    weighed,
                }),
                Sited::Unreached(weighed) => {
                    Err(StepFailed::counted(StepFailure::IllegalSite, weighed))
                }
            }
        }
        FeatureSpec::Nearest { kind, uncovered } => {
            let mut ranker = Ranker::new(ground, column_of(commander), |index| {
                matches_pick(ground, seat, index, kind, uncovered)
            });
            // No legal site is no site for any feature: refused before the
            // ranker prices a walk or the stone's search starts, as
            // `illegal_site` when a feature is in reach and `no_target` when
            // none is.
            let Some(sites) = legal_sites(ground, scratch, candidates, seat, commander)
                .filter(|(_, weighed)| *weighed > 0)
            else {
                return Err(if ranker.is_empty() {
                    StepFailed::counted(StepFailure::NoTarget, ranker.matched())
                } else {
                    StepFailed::counted(StepFailure::IllegalSite, 0)
                });
            };
            let mut reached = false;
            // The sites weighed for every feature are the same legal sites:
            // the candidate set is built from the seat's spheres and the
            // commander's column alone, and only the feature each is measured
            // to differs. So the count is that set's size, never a sum over
            // the features tried (fix pass 1, C5).
            let mut weighed: u32 = 0;
            while let Some((feature, _)) = ranker.next(ground, scratch) {
                reached = true;
                match stone_site(ground, scratch, sites, feature) {
                    Sited::Stone(site, count) => {
                        return Ok(Stone {
                            feature,
                            site,
                            weighed: count,
                        });
                    }
                    // The ranker emits only features the commander reaches,
                    // from the same column the sites are reached from, so
                    // none is unreached; were one, the next is tried.
                    Sited::Unreached(count) => weighed = count,
                }
            }
            if reached {
                Err(StepFailed::counted(StepFailure::IllegalSite, weighed))
            } else {
                Err(StepFailed::counted(StepFailure::NoTarget, ranker.matched()))
            }
        }
        // `covered {}` is legal only under `on` (the compile refuses it here).
        FeatureSpec::Covered => Err(StepFailed::counted(StepFailure::NoTarget, 0)),
    }
}

/// Whether the feature at `index` passes a `covering` pick's filters: its
/// kind, alive, and -- for `UNCOVERED` -- its anchor point outside every
/// sphere of `seat`'s own living beacons, awake or dormant. Never reads
/// another seat's state. `false` for an index the table does not hold.
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
    ground
        .anchor_point(index)
        .is_some_and(|point| !ground.inside_own_sphere(seat, point))
}

/// Whether a **Generator** of any seat stands, alive, on the column `(x, y)`:
/// one Generator per vent, since a second tap supplies 0 kW (`power.rs`'s
/// `supply_of`).
#[must_use]
pub fn generator_on(ground: &Ground<'_>, x: i32, y: i32) -> bool {
    let generator = crate::tables::StructureKind::Generator.id();
    ground
        .structures
        .kinds()
        .iter()
        .zip(ground.structures.hit_points())
        .zip(ground.structures.positions())
        .any(|((kind, hp), at)| *kind == generator && hp.is_alive() && same_column(*at, x, y))
}

/// **The sphere test** a named or `covered {}` `on` asks first: whether the
/// feature at `index` has its anchor point ([`Ground::anchor_point`]) within
/// the beacon sphere's radius of `centre`, the sphere the target is written
/// into. No structure moves that point, so no structure outside that sphere
/// decides the answer (decisions-log item 133 (3) (c)); the module docs say
/// what the claim does not cover. [`covering_site`] asks it of every site, so
/// a `covering` never picks a site its own `covered {}` fails. `false` for
/// an index the table does not hold.
#[must_use]
pub fn anchor_in_sphere(ground: &Ground<'_>, centre: [Fx; 3], index: usize) -> bool {
    anchor_in_sphere_of(ground, None, centre, index)
}

/// [`anchor_in_sphere`] for the sphere of `beacon`, centred on `centre`: its
/// radius grown by a finished Resonance Spire (S3's plan, decision 6: the
/// sphere's coverage of vents grows), or the base for `None`, a site where no
/// beacon stands yet.
#[must_use]
pub fn anchor_in_sphere_of(
    ground: &Ground<'_>,
    beacon: Option<BeaconId>,
    centre: [Fx; 3],
    index: usize,
) -> bool {
    ground
        .anchor_point(index)
        .is_some_and(|point| ground.in_sphere_of(beacon, centre, point))
}

/// Whether a live Generator of any seat, or a Build target of `seat`'s (but
/// `except`'s), stands on any column of the feature's footprint.
fn vent_taken(ground: &Ground<'_>, seat: SeatId, except: Option<BeaconId>, vent: &Feature) -> bool {
    vent.footprint.iter().any(|column| {
        generator_on(ground, column.x, column.y)
            || ground.own_target_on(seat, except, column.x, column.y)
    })
}

/// What an `on` resolved to: the feature and the column the structure stands
/// on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OnSite {
    /// The vent.
    pub feature: usize,
    /// Its `on` column.
    pub column: [i32; 2],
}

/// Resolve an `on` anchor for a Build target of `seat`'s, written into the
/// beacon whose sphere is centred at `centre` (`except` is that beacon, or
/// `None` for one not yet placed), ranked from that centre's column.
/// `covered` is the feature a `covering` step bound, which `covered {}` names.
///
/// [`on_vent_counted`] with the reason alone.
///
/// # Errors
///
/// As [`on_vent_counted`].
pub fn on_vent(
    ground: &Ground<'_>,
    scratch: &mut Scratch,
    seat: SeatId,
    centre: [Fx; 3],
    except: Option<BeaconId>,
    covered: Option<usize>,
    spec: FeatureSpec,
) -> Result<OnSite, StepFailure> {
    on_vent_counted(ground, scratch, seat, centre, except, covered, spec)
        .map_err(|failed| failed.reason)
}

/// [`on_vent`], with how many candidates matched on a miss.
///
/// `on` ranks **only vents** whose `on` column lies inside the sphere, and a
/// vent is a candidate only if no live Generator of any seat and no Build
/// target of this seat (but `except`'s) stands on its footprint
/// ([`on_candidate`]; targeting.md, "Sites").
///
/// A name, or `covered {}`, is not ranked, and is read **sphere first**: a
/// vent whose anchor point lies outside the sphere ([`anchor_in_sphere`]) is
/// `no_target` whatever stands on it, as a description would find it no
/// candidate. Inside the sphere it must still be a legal `on` site -- one
/// structure per voxel and one Generator per vent -- so a vent a live
/// Generator of any seat or another of this seat's Build targets already
/// stands on, a vent with no free column, and a vent whose free `on` column
/// lies outside the sphere are each `illegal_site`.
///
/// # Errors
///
/// Counted ([`StepFailed::counted`]): `no_target` with 0 when a name is
/// absent, lost, not a vent, or outside the sphere; `illegal_site` with 1 when
/// a named vent inside the sphere has no legal `on` column; `no_target` with
/// what [`on_candidate`] admitted when a description reaches nothing.
pub fn on_vent_counted(
    ground: &Ground<'_>,
    scratch: &mut Scratch,
    seat: SeatId,
    centre: [Fx; 3],
    except: Option<BeaconId>,
    covered: Option<usize>,
    spec: FeatureSpec,
) -> Result<OnSite, StepFailed> {
    let named = |feature: Option<usize>| -> Result<OnSite, StepFailed> {
        let missed = StepFailed::counted(StepFailure::NoTarget, 0);
        let feature = feature.ok_or(missed)?;
        let found = ground.features.get(feature).ok_or(missed)?;
        if found.kind != FeatureKind::Vent
            || !ground.is_live(feature)
            || !anchor_in_sphere_of(ground, except, centre, feature)
        {
            return Err(missed);
        }
        let illegal = StepFailed::counted(StepFailure::IllegalSite, 1);
        if vent_taken(ground, seat, except, found) {
            return Err(illegal);
        }
        let [x, y] = ground.on_column(feature, seat, except).ok_or(illegal)?;
        let inside = ground
            .standing(x, y)
            .is_some_and(|point| ground.in_sphere_of(except, centre, point));
        if !inside {
            return Err(illegal);
        }
        Ok(OnSite {
            feature,
            column: [x, y],
        })
    };
    match spec {
        FeatureSpec::Name { kind, anchor } => named(ground.features.index_of(kind, anchor)),
        FeatureSpec::Covered => named(covered),
        FeatureSpec::Nearest { .. } => {
            let mut ranker = Ranker::new(ground, column_of(centre), |index| {
                on_candidate(ground, seat, except, centre, index)
            });
            let missed = StepFailed::counted(StepFailure::NoTarget, ranker.matched());
            let (feature, _) = ranker.next(ground, scratch).ok_or(missed)?;
            // The candidate rule read this column, so a ranked vent has one.
            let column = ground.on_column(feature, seat, except).ok_or(missed)?;
            Ok(OnSite { feature, column })
        }
    }
}

/// Whether the vent at `index` is an `on` candidate for a beacon of `seat`
/// centred at `centre` (`except` is that beacon, or `None` for one not yet
/// placed): a live vent no live Generator of any seat and no Build target of
/// this seat (but `except`'s) stands on, whose `on` column
/// ([`Ground::on_column`]) lies inside the sphere. `false` for an index the
/// table does not hold.
#[must_use]
pub fn on_candidate(
    ground: &Ground<'_>,
    seat: SeatId,
    except: Option<BeaconId>,
    centre: [Fx; 3],
    index: usize,
) -> bool {
    let Some(feature) = ground.features.get(index) else {
        return false;
    };
    if feature.kind != FeatureKind::Vent || !ground.is_live(index) {
        return false;
    }
    if vent_taken(ground, seat, except, feature) {
        return false;
    }
    ground
        .on_column(index, seat, except)
        .and_then(|[x, y]| ground.standing(x, y))
        .is_some_and(|point| ground.in_sphere_of(except, centre, point))
}

#[cfg(test)]
mod tests {
    use super::{Ground, Ranker, spiral_offsets};
    use crate::features::{Feature, FeatureKind, FeatureTable, FootprintColumn};
    use crate::pathing::clusters::Clusters;
    use crate::pathing::search::Scratch;
    use crate::pathing::surface::{StepCosts, Surface};
    use crate::rules::RulesTable;
    use crate::seams::UnitTally;
    use crate::tables::{BeaconTable, StructureTable, TargetTable};
    use crate::voxels::{Material, Richness, VoxelStore};

    /// The map edge of the synthetic maps below, in voxels: two 32-voxel
    /// clusters a side.
    const EDGE: i32 = 64;

    /// The ground height of the synthetic maps, in solid voxels.
    const GROUND: i32 = 12;

    fn rules() -> RulesTable {
        RulesTable::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("rules")
                .join("rules.v1.json"),
        )
        .unwrap_or_else(|error| panic!("the committed rules table loads: {error}"))
    }

    /// A synthetic map: flat ground of [`GROUND`] voxels, lowered to two
    /// voxels wherever `ravine` says, with a three-by-three lean vent stamped
    /// at each of `vents`.
    struct Map {
        voxels: VoxelStore,
        surface: Surface,
        clusters: Clusters,
        scratch: Scratch,
        features: FeatureTable,
    }

    fn synthetic(ravine: impl Fn(i32, i32) -> bool, vents: &[[i32; 2]]) -> Map {
        let size = u32::try_from(EDGE).unwrap_or_else(|error| panic!("a small edge: {error}"));
        let mut voxels =
            VoxelStore::new([size, size, 32]).unwrap_or_else(|| panic!("a whole number of chunks"));
        let mut heights: Vec<i32> = Vec::new();
        for y in 0..EDGE {
            for x in 0..EDGE {
                heights.push(if ravine(x, y) { 2 } else { GROUND });
            }
        }
        voxels.fill_from_heightmap(&heights, 0, 3, Material::DIRT, Material::STONE);
        let mut features: Vec<Feature> = Vec::new();
        for anchor in vents {
            let mut footprint: Vec<FootprintColumn> = Vec::new();
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (x, y) = (anchor[0] + dx, anchor[1] + dy);
                    let top = GROUND - 1;
                    assert!(voxels.set_pristine([x, y, top], Material::VENT_LEAN));
                    footprint.push(FootprintColumn { x, y, top });
                }
            }
            features.push(Feature::new(
                FeatureKind::Vent,
                *anchor,
                Richness::Lean,
                footprint,
            ));
        }
        let ten = core::num::NonZeroI32::new(10).unwrap_or_else(|| panic!("ten"));
        let surface = Surface::new(&voxels, StepCosts::new(ten, 14, 4, 2))
            .unwrap_or_else(|| panic!("a surface"));
        let mut scratch = Scratch::for_map(&surface, 32).unwrap_or_else(|| panic!("a scratch"));
        let clusters = Clusters::new(
            &surface,
            crate::pathing::ClusterShape {
                size: 32,
                long_run: 6,
                smooth_lookahead: 16,
            },
            &mut scratch,
        )
        .unwrap_or_else(|| panic!("clusters"));
        Map {
            voxels,
            surface,
            clusters,
            scratch,
            features: FeatureTable::new(features)
                .unwrap_or_else(|error| panic!("vents apart: {error}")),
        }
    }

    /// The first feature "nearest" ranks from `origin`, by name.
    fn nearest(map: &mut Map, origin: [i32; 2]) -> String {
        ranked(map, origin, 1)
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("a reachable vent"))
            .0
    }

    /// The first `count` features "nearest" ranks from `origin`, in order,
    /// by name and with their travel cost.
    fn ranked(map: &mut Map, origin: [i32; 2], count: usize) -> Vec<(String, i64)> {
        let (mut out, _) = ranked_all(map, origin);
        out.truncate(count);
        out
    }

    /// Every feature "nearest" ranks from `origin`, in order, by name and
    /// with its travel cost, and how many the filter matched before
    /// reachability ([`Ranker::matched`]).
    fn ranked_all(map: &mut Map, origin: [i32; 2]) -> (Vec<(String, i64)>, u32) {
        let rules = rules();
        let beacons = BeaconTable::with_capacity(0);
        let structures = StructureTable::with_capacity(0);
        let targets = TargetTable::with_capacity(0);
        let spiral = spiral_offsets(24);
        let work = UnitTally::new();
        let ground = Ground {
            surface: &map.surface,
            clusters: &map.clusters,
            voxels: &map.voxels,
            features: &map.features,
            beacons: &beacons,
            structures: &structures,
            targets: &targets,
            rules: &rules,
            spiral: &spiral,
            work: &work,
        };
        let mut ranker = Ranker::new(&ground, origin, |_| true);
        let mut out: Vec<(String, i64)> = Vec::new();
        while let Some((feature, walk)) = ranker.next(&ground, &mut map.scratch) {
            let cost = walk.cost;
            let name = map
                .features
                .get(feature)
                .unwrap_or_else(|| panic!("in the table"))
                .name();
            out.push((name, cost));
        }
        (out, ranker.matched())
    }

    #[test]
    fn the_nearest_vent_is_the_least_travel_not_the_least_distance() {
        // A ravine, two voxels deep where the ground is twelve, runs from the
        // map's southern edge to y = 49 between x = 28 and 30. A walker climbs
        // one voxel at a time (item 59), so nothing crosses it; the way round
        // is past its end. From (10, 10), vent A at (40, 10) is 30 voxels away
        // in a straight line but across the ravine; vent B at (10, 45) is 35
        // voxels away and on the same side. Straight-line or octile distance
        // would pick A; the estimator's travel picks B (targeting.md,
        // "Nearest": "a vent across a ravine loses").
        let mut map = synthetic(
            |x, y| (28..=30).contains(&x) && y < 50,
            &[[40, 10], [10, 45]],
        );
        let order = ranked(&mut map, [10, 10], 2);
        let names: Vec<&str> = order.iter().map(|(name, _)| name.as_str()).collect();
        // Both are reachable -- A by the way round past the ravine's end -- so
        // A is ranked second, at a higher travel cost, rather than skipped: the
        // test separates "least travel" from "reachable at all".
        assert_eq!(names, ["vent_10_45", "vent_40_10"]);
        let costs: Vec<i64> = order.iter().map(|(_, cost)| *cost).collect();
        assert!(costs.first() < costs.get(1), "{costs:?}");
    }

    #[test]
    fn ties_go_to_anchor_y_then_x() {
        // Four vents ten voxels from (32, 32) on flat ground: every one costs
        // the same ten cardinal steps, so the order is the tie-break's,
        // `(cost, anchor y, anchor x)`: the lowest y first...
        let mut map = synthetic(|_, _| false, &[[22, 32], [42, 32], [32, 42], [32, 22]]);
        assert_eq!(nearest(&mut map, [32, 32]), "vent_32_22");
        // ...and with the y tied, the lowest x.
        let mut map = synthetic(|_, _| false, &[[42, 32], [22, 32]]);
        assert_eq!(nearest(&mut map, [32, 32]), "vent_22_32");
    }

    #[test]
    fn an_unreachable_vent_is_no_candidate() {
        // A vent walled in by the ravine on every side is not a candidate:
        // "nearest" skips what the commander cannot reach rather than
        // answering with it.
        let mut map = synthetic(walled_in, &[[22, 22], [50, 50]]);
        assert_eq!(nearest(&mut map, [5, 5]), "vent_50_50");
    }

    /// A vent walled in by the ravine, at (22, 22).
    fn walled_in(x: i32, y: i32) -> bool {
        (18..=26).contains(&x)
            && (18..=26).contains(&y)
            && !((21..=23).contains(&x) && (21..=23).contains(&y))
    }

    #[test]
    fn an_unreachable_vent_still_counts_as_matched() {
        // The count is asked before reachability, from an origin on the map:
        // both vents match, and only the reachable one is ranked...
        let mut map = synthetic(walled_in, &[[22, 22], [50, 50]]);
        let (order, matched) = ranked_all(&mut map, [5, 5]);
        let names: Vec<&str> = order.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["vent_50_50"]);
        assert_eq!(matched, 2);
        // ...and a walled-in vent alone ranks nothing yet counts 1: the
        // "1 matched, none reachable" a `no_target` carries.
        let mut map = synthetic(walled_in, &[[22, 22]]);
        assert_eq!(ranked_all(&mut map, [5, 5]), (Vec::new(), 1));
    }

    /// The stone's tie-break lives in [`super::closest_site`] alone: on
    /// flat ground, two sites at one least cost where the directed queue
    /// settles the one with the higher `(y, x)` first (a column can join the
    /// bucket being settled, and buckets keep no order within a key), the
    /// stone is still the lower. The pair is found by search over the map's
    /// columns, and some pair must exist, so the case is real.
    #[test]
    fn the_stone_tie_goes_to_the_lowest_site_whatever_the_queue_settles_first() {
        use crate::pathing::search::Tally;
        let mut map = synthetic(|_, _| false, &[]);
        let surface = &map.surface;
        let costs = surface.costs();
        let goal = surface
            .node_of(40, 40)
            .unwrap_or_else(|| panic!("the goal"));
        let centre = [20, 20];
        let radius = 9;
        let mut cases = 0;
        let mut first_wrong = 0;
        // Every pair of columns of the sphere at one least cost from the goal.
        let mut disk: Vec<(i64, [i32; 2])> = Vec::new();
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy > radius * radius {
                    continue;
                }
                let (x, y) = (centre[0] + dx, centre[1] + dy);
                let cost = i64::from(costs.octile(40 - x, 40 - y));
                disk.push((cost, [x, y]));
            }
        }
        for (i, (cost_a, a)) in disk.iter().enumerate() {
            for (cost_b, b) in disk.iter().skip(i + 1) {
                if cost_a != cost_b {
                    continue;
                }
                let (node_a, node_b) = (
                    map.surface
                        .node_of(a[0], a[1])
                        .unwrap_or_else(|| panic!("a")),
                    map.surface
                        .node_of(b[0], b[1])
                        .unwrap_or_else(|| panic!("b")),
                );
                // Only these two are sites; the sphere reaches both.
                let reach = i64::from(costs.octile(a[0] - centre[0], a[1] - centre[1]))
                    .max(i64::from(costs.octile(b[0] - centre[0], b[1] - centre[1])));
                map.scratch.reach.clear_sites();
                map.scratch.reach.mark_site(node_a);
                map.scratch.reach.mark_site(node_b);
                map.scratch.reach.add_sphere(centre, reach);
                // Which the queue settles first.
                map.scratch.reach.begin_toward(&map.surface, goal);
                let mut tally = Tally::ZERO;
                let mut first = None;
                while let Some((node, _)) = map.scratch.reach.next(&map.surface, &mut tally) {
                    if map.scratch.reach.is_site(node) {
                        first = Some(map.surface.coord_of(node));
                        break;
                    }
                }
                let lower = if (a[1], a[0]) < (b[1], b[0]) { *a } else { *b };
                let stone = super::closest_site(&mut map.scratch, &map.surface, goal);
                assert_eq!(stone, Some(lower), "sites {a:?} and {b:?} at {cost_a}");
                cases += 1;
                if first.map(|(x, y)| [x, y]) != Some(lower) {
                    first_wrong += 1;
                }
            }
        }
        assert!(cases > 10, "pairs at one cost: {cases}");
        assert!(
            first_wrong > 0,
            "no pair where the queue settles the higher site first: the case is not built"
        );
    }

    #[test]
    fn a_negative_radius_has_no_offsets() {
        // `i32::MIN` included, whose negation would overflow.
        assert_eq!(spiral_offsets(-5), Vec::<[i32; 2]>::new());
        assert_eq!(spiral_offsets(i32::MIN), Vec::<[i32; 2]>::new());
        assert_eq!(spiral_offsets(0), vec![[0, 0]]);
    }

    /// Reading rule 2 in a world (`docs/design/targeting.md`, "Three reading
    /// rules"): an unbuilt Build target bound to a vent that is lost reads its
    /// description again when it was written as "the nearest vent", and idles
    /// in place when it was written as a name.
    fn reread_after_loss(desc: u8) -> Reread {
        use crate::math::quantity::Money;
        use crate::runner::{MatchSettings, Runner};
        use crate::seams::MandateKind;
        use crate::tables::{PRIORITY_NORMAL, SeatId, StructureKind};
        use crate::voxels::VoxelEdit;
        use crate::world::{World, WorldConfig};

        // The golden seed carries two vents 24 voxels apart, `vent_103_111`
        // and `vent_98_135`; a beacon between them holds both in its
        // 24-voxel sphere, and no third vent is in it.
        let mut world = World::new(&WorldConfig {
            match_seed: 0x0000_0000_ca5c_aded,
            seats: 2,
            rules: rules(),
            match_settings: MatchSettings {
                segment_lengths_ms: vec![180_000],
                round_limit: 1,
            },
        })
        .unwrap_or_else(|error| panic!("the golden seed generates: {error}"));
        let lost = world
            .features()
            .index_of_name("vent_103_111")
            .unwrap_or_else(|| panic!("the golden seed carries vent_103_111"));
        let kept = world
            .features()
            .index_of_name("vent_98_135")
            .unwrap_or_else(|| panic!("the golden seed carries vent_98_135"));
        let stand = |world: &World, index: usize| {
            let feature = world
                .features()
                .get(index)
                .unwrap_or_else(|| panic!("in the table"));
            let [x, y] = feature.anchor;
            world
                .ground()
                .standing(x, y)
                .unwrap_or_else(|| panic!("the anchor stands"))
        };
        let lost_at = stand(&world, lost);
        let kept_at = stand(&world, kept);
        let seat = SeatId::new(0);
        let centre = world
            .ground()
            .standing(100, 123)
            .unwrap_or_else(|| panic!("the midpoint stands"));
        let beacon = world
            .place_beacon_directly(seat, centre, MandateKind::Build, PRIORITY_NORMAL)
            .unwrap_or_else(|| panic!("room for a beacon"));
        // No money, so the Quartermaster never pays for the target and it
        // stays unbuilt, which is what rule 2 reads.
        world.set_treasury(seat, Money::new(0));
        let lost_id = u32::try_from(lost).unwrap_or_else(|error| panic!("{error}"));
        let kept_id = u32::try_from(kept).unwrap_or_else(|error| panic!("{error}"));
        assert!(
            world
                .add_bound_target(
                    beacon,
                    StructureKind::Generator.id(),
                    lost_at,
                    (lost_id, desc),
                    crate::tables::BuildOrder::NONE,
                    None,
                )
                .is_ok()
        );
        let footprint = world
            .features()
            .get(lost)
            .unwrap_or_else(|| panic!("in the table"))
            .footprint
            .clone();
        for column in &footprint {
            assert!(
                world
                    .request_voxel_edit(VoxelEdit::Set {
                        at: [column.x, column.y, column.top],
                        material: Material::STONE,
                        by: None,
                    })
                    .is_ok()
            );
        }
        let mut runner =
            Runner::new(world).unwrap_or_else(|error| panic!("a world from `World::new`: {error}"));
        assert_eq!(runner.begin_push(), Ok(true), "a match opens in a Lull");
        for _ in 0..20 {
            assert!(matches!(runner.step(), Ok(Some(_))), "the Push runs");
        }
        let world = runner.world();
        assert!(!world.feature_is_live(lost), "the vent is lost");
        let targets = world.targets();
        assert_eq!(targets.len(), 1, "one target, never a second");
        let feature = targets.features().first().copied().unwrap_or(u32::MAX);
        let anchor = targets
            .anchors()
            .first()
            .copied()
            .unwrap_or_else(|| panic!("an anchor"));
        Reread {
            feature,
            anchor,
            lost: (lost_id, lost_at),
            kept: (kept_id, kept_at),
        }
    }

    /// What [`reread_after_loss`] found: the target's bound feature and
    /// anchor after the loss, and each vent's index and standing anchor.
    struct Reread {
        feature: u32,
        anchor: [crate::math::fixed::Fx; 3],
        lost: (u32, [crate::math::fixed::Fx; 3]),
        kept: (u32, [crate::math::fixed::Fx; 3]),
    }

    #[test]
    fn a_lost_description_reads_again_and_a_lost_name_idles() {
        let described = reread_after_loss(super::DESCRIPTION_NEAREST_VENT);
        assert_eq!(
            described.feature, described.kept.0,
            "the description moved to the other vent"
        );
        assert_eq!(described.anchor, described.kept.1, "and so did its anchor");

        let named = reread_after_loss(super::DESCRIPTION_NAME);
        assert_eq!(named.feature, named.lost.0, "a name is not read again");
        assert_eq!(named.anchor, named.lost.1, "it idles where it was");
    }
}
