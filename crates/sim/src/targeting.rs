// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The resolver: where a description lands (`docs/design/targeting.md`,
//! "Nearest" and "Sites"; decisions-log item 127 (12)).
//!
//! Everything here **reads**. A step's `covering` site, a Build target's `on`
//! column and a held description's re-read are all answered from a [`Ground`]
//! -- the world's tables, its voxels, its pathing graph and its feature table,
//! borrowed read-only -- plus a search [`Scratch`] for the estimator. The sim
//! asks it from the decision phase; the gateway can ask the same functions over
//! its hosted world with its own scratch (`resolve_refs`, a later lane), which
//! is what makes "the gateway's pick equals the sim's" true by construction
//! rather than by agreement. Nothing here steps, forks or runs a mandate
//! (AGENTS.md section 3 rule 2).
//!
//! # Nearest
//!
//! **The least estimated travel from the origin to the feature. Unreachable
//! candidates are skipped. Ties go to the lowest anchor y, then x.** The metric
//! is the item-61 estimator's integer `cost` ([`crate::pathing::estimate`]),
//! from which the editor's ETA is derived, so what a chip says and what the sim
//! picks agree. It measures to the feature's anchor column, or, when that
//! column cannot be stood on or is not connected to the origin, to the first
//! standable connected footprint column in `(y, x)` order.
//!
//! [`Ranker`] runs at most **one estimate per candidate**, and only for a
//! candidate the octile lower bound has not already ruled out: a candidate is
//! emitted once its key `(cost, y, x)` is below every unestimated candidate's
//! `(bound, y, x)`, and because a bound never exceeds its cost, that is the
//! candidate's true place in the order. Six or fewer candidates per kind on an
//! S1 map, so the budget S1 measures is small.
//!
//! In S1 liveness and reachability read the **live** world, unfogged, as the
//! adopted design accepts for this stage (targeting.md, "Descriptions";
//! decisions-log item 108 (1)); S3 bases both on the seat's terrain knowledge.
//!
//! # Allocation
//!
//! None: the ranker is a fixed array of [`MAX_FEATURES`] entries, the spiral's
//! offsets are computed once at construction ([`spiral_offsets`]) and its
//! per-ring candidates go through a fixed buffer, and the estimator's buffers
//! are the scratch's (`tests/allocations.rs`).

use crate::features::{FeatureKind, FeatureTable, MAX_FEATURES};
use crate::interpreter::cond::within;
use crate::interpreter::state::StepFailure;
use crate::math::fixed::Fx;
use crate::pathing::clusters::Clusters;
use crate::pathing::estimate::{Fog, Speed, estimate};
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

    /// `beacon.sphere_radius_voxels`.
    #[must_use]
    pub fn sphere_radius(&self) -> i32 {
        self.rules
            .message()
            .beacon
            .as_ref()
            .and_then(|block| i32::try_from(block.sphere_radius_voxels).ok())
            .unwrap_or(0)
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
        let radius = self.sphere_radius();
        let count = usize::try_from(self.beacons.len()).unwrap_or(0);
        (0..count).any(|row| {
            self.beacons.seats().get(row).copied() == Some(seat.raw())
                && self
                    .beacons
                    .hit_points()
                    .get(row)
                    .is_some_and(|hp| hp.is_alive())
                && self
                    .beacons
                    .positions()
                    .get(row)
                    .copied()
                    .is_some_and(|centre| within(centre, point, radius))
        })
    }

    /// **No stacking**: whether one of `seat`'s own live beacons stands on the
    /// column `(x, y)` (`docs/design/targeting.md`, "Companion changes").
    #[must_use]
    pub fn stacks_on_own(&self, seat: SeatId, x: i32, y: i32) -> bool {
        let count = usize::try_from(self.beacons.len()).unwrap_or(0);
        (0..count).any(|row| {
            self.beacons.seats().get(row).copied() == Some(seat.raw())
                && self
                    .beacons
                    .hit_points()
                    .get(row)
                    .is_some_and(|hp| hp.is_alive())
                && self
                    .beacons
                    .positions()
                    .get(row)
                    .copied()
                    .is_some_and(|at| same_column(at, x, y))
        })
    }

    /// **One structure per voxel**: whether a live structure of any seat --
    /// or a ruin, which owns nothing and still stands -- occupies the column
    /// `(x, y)`.
    #[must_use]
    pub fn structure_on(&self, x: i32, y: i32) -> bool {
        let count = usize::try_from(self.structures.len()).unwrap_or(0);
        (0..count).any(|row| {
            self.structures
                .hit_points()
                .get(row)
                .is_some_and(|hp| hp.is_alive())
                && self
                    .structures
                    .positions()
                    .get(row)
                    .copied()
                    .is_some_and(|at| same_column(at, x, y))
        })
    }

    /// Whether a **Generator** of any seat stands, alive, on the column.
    fn generator_on(&self, x: i32, y: i32) -> bool {
        let count = usize::try_from(self.structures.len()).unwrap_or(0);
        let generator = crate::tables::StructureKind::Generator.id();
        (0..count).any(|row| {
            self.structures.kinds().get(row).copied() == Some(generator)
                && self
                    .structures
                    .hit_points()
                    .get(row)
                    .is_some_and(|hp| hp.is_alive())
                && self
                    .structures
                    .positions()
                    .get(row)
                    .copied()
                    .is_some_and(|at| same_column(at, x, y))
        })
    }

    /// Whether a Build target of one of `seat`'s own beacons -- `except`
    /// excluded -- is anchored on the column `(x, y)`. A queued target is a
    /// per-seat claim, so another seat's stays hidden.
    #[must_use]
    pub fn own_target_on(&self, seat: SeatId, except: Option<BeaconId>, x: i32, y: i32) -> bool {
        let count = usize::try_from(self.targets.len()).unwrap_or(0);
        (0..count).any(|row| {
            let Some(beacon) = self.targets.beacons().get(row).copied() else {
                return false;
            };
            if except.is_some_and(|skip| skip.raw() == beacon) {
                return false;
            }
            let owned = usize::try_from(beacon)
                .ok()
                .and_then(|at| self.beacons.seats().get(at).copied())
                == Some(seat.raw());
            owned
                && self.targets.kinds().get(row).copied() == Some(TargetKind::Build.id())
                && self
                    .targets
                    .anchors()
                    .get(row)
                    .copied()
                    .is_some_and(|at| same_column(at, x, y))
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
    /// column be missing).
    fn anchor_point(&self, index: usize) -> Option<[Fx; 3]> {
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
    fn measure_to(&self, index: usize, origin: Node) -> Option<Node> {
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

    /// The item-61 estimate's integer cost from `from` to `to`, or `None` when
    /// no route exists.
    fn travel(&self, scratch: &mut Scratch, from: Node, to: Node) -> Option<i64> {
        self.charge(1);
        estimate(
            self.surface,
            self.clusters,
            scratch,
            from,
            to,
            Fog::Clear,
            Speed::commander(self.rules),
        )
        .map(|found| i64::from(found.cost))
    }
}

/// Whether a point floors onto the column `(x, y)`.
fn same_column(at: [Fx; 3], x: i32, y: i32) -> bool {
    at.first().map(|value| value.floor_voxels()) == Some(x)
        && at.get(1).map(|value| value.floor_voxels()) == Some(y)
}

/// The column a point floors onto.
#[must_use]
pub fn column_of(at: [Fx; 3]) -> [i32; 2] {
    [
        at.first().map_or(0, |value| value.floor_voxels()),
        at.get(1).map_or(0, |value| value.floor_voxels()),
    ]
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
    /// Its estimated cost, once estimated.
    cost: i64,
    /// 0 unestimated, 1 estimated, 2 emitted or unreachable.
    state: u8,
}

/// "Nearest", one candidate at a time, in `(cost, anchor y, anchor x)` order.
///
/// Each candidate costs at most one estimate, and only when the bounds of the
/// others cannot already place it (the module docs).
#[derive(Clone, Debug)]
pub struct Ranker {
    entries: [Entry; MAX_FEATURES],
    len: usize,
    origin: Node,
}

impl Ranker {
    /// The candidates `keep` admits, ranked from the column `origin`.
    ///
    /// A candidate with no reachable footprint column is dropped here: an
    /// unreachable feature is not a candidate (targeting.md, "Nearest").
    pub fn new<F>(ground: &Ground<'_>, origin: [i32; 2], mut keep: F) -> Ranker
    where
        F: FnMut(usize) -> bool,
    {
        let mut ranker = Ranker {
            entries: [Entry::default(); MAX_FEATURES],
            len: 0,
            origin: 0,
        };
        let Some(start) = ground.surface.node_of(
            origin.first().copied().unwrap_or(0),
            origin.get(1).copied().unwrap_or(0),
        ) else {
            return ranker;
        };
        ranker.origin = start;
        let count = ground.features.len().min(MAX_FEATURES);
        let mut index: usize = 0;
        while index < count {
            ground.charge(1);
            if keep(index)
                && let Some(goal) = ground.measure_to(index, start)
                && let Some(feature) = ground.features.get(index)
                && let Some(slot) = ranker.entries.get_mut(ranker.len)
            {
                *slot = Entry {
                    feature: index,
                    y: feature.anchor.get(1).copied().unwrap_or(0),
                    x: feature.anchor.first().copied().unwrap_or(0),
                    goal,
                    bound: i64::from(ground.surface.heuristic(start, goal)),
                    cost: 0,
                    state: 0,
                };
                ranker.len = ranker.len.saturating_add(1);
            }
            index = index.saturating_add(1);
        }
        ranker
    }

    /// The next candidate, nearest first, with its estimated cost; `None` when
    /// every candidate has been emitted or found unreachable.
    pub fn next(&mut self, ground: &Ground<'_>, scratch: &mut Scratch) -> Option<(usize, i64)> {
        loop {
            let live = self.entries.get(..self.len)?;
            // item 62: anchors are unique per feature, so both keys below end
            // in a unique `(y, x)`.
            let best_estimated = live
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
            match (best_estimated, best_bound) {
                // The best estimated candidate is ahead of every bound still
                // unestimated, so its place in the order is final.
                (Some((at, key)), bound) if bound.is_none_or(|(_, other)| key < other) => {
                    let entry = self.entries.get_mut(at)?;
                    entry.state = 2;
                    return Some((entry.feature, entry.cost));
                }
                (_, Some((at, _))) => {
                    let origin = self.origin;
                    let entry = self.entries.get_mut(at)?;
                    match ground.travel(scratch, origin, entry.goal) {
                        Some(cost) => {
                            entry.cost = cost;
                            entry.state = 1;
                        }
                        None => entry.state = 2,
                    }
                }
                // Nothing estimated is waiting and nothing is left to
                // estimate (an estimated candidate with no bound left is
                // emitted by the first arm).
                (_, None) => return None,
            }
        }
    }
}

/// Every `[dx, dy]` within `radius`, in `(dx² + dy², dy, dx)` order: the
/// spiral a `covering` site is searched along, computed once at
/// construction so a decision allocates nothing.
#[must_use]
pub fn spiral_offsets(radius: i32) -> Vec<[i32; 2]> {
    let reach = radius.max(0);
    let limit = i64::from(reach).saturating_mul(i64::from(reach));
    let mut out: Vec<[i32; 2]> = Vec::new();
    let mut dy = -reach;
    while dy <= reach {
        let mut dx = -reach;
        while dx <= reach {
            let d2 = i64::from(dx)
                .saturating_mul(i64::from(dx))
                .saturating_add(i64::from(dy).saturating_mul(i64::from(dy)));
            if d2 <= limit {
                out.push([dx, dy]);
            }
            dx = dx.saturating_add(1);
        }
        dy = dy.saturating_add(1);
    }
    // item 62: the key ends in `dx`, unique within a `dy` row, so it is total.
    out.sort_unstable_by_key(|offset| {
        let dx = i64::from(offset.first().copied().unwrap_or(0));
        let dy = i64::from(offset.get(1).copied().unwrap_or(0));
        (dx * dx + dy * dy, dy, dx)
    });
    out
}

/// The most columns one ring of the spiral can hold.
///
/// A ring is the offsets at one squared distance, and no integer is a sum of
/// two squares in more than a few dozen ways at a sphere's radius; a ring
/// past this is truncated rather than allocated for, which no committed
/// radius reaches.
const RING_ROOM: usize = 64;

/// The `covering` site for `feature`: the spiral's first column, in order of
/// squared distance from the feature's anchor, then squared distance from the
/// commander's column, then y, then x, that is a **legal site** (inside one of
/// `seat`'s own spheres, not on the column of one of its own live beacons),
/// standable, outside every feature's footprint, **reachable** by the
/// commander, and whose sphere holds the feature's `on` column
/// (`docs/design/targeting.md`, "Sites"). `None` when no such column exists.
#[must_use]
pub fn covering_site(
    ground: &Ground<'_>,
    seat: SeatId,
    commander: [Fx; 3],
    feature: usize,
) -> Option<[i32; 2]> {
    let radius = ground.sphere_radius();
    let anchor = ground.features.get(feature)?.anchor;
    let ax = anchor.first().copied().unwrap_or(0);
    let ay = anchor.get(1).copied().unwrap_or(0);
    let on = ground.on_column(feature, seat, None)?;
    let on_point = ground.standing(
        on.first().copied().unwrap_or(0),
        on.get(1).copied().unwrap_or(0),
    )?;
    let [cx, cy] = column_of(commander);
    let start = ground.surface.node_of(cx, cy)?;

    let mut ring: [(i64, i32, i32); RING_ROOM] = [(0, 0, 0); RING_ROOM];
    let mut at: usize = 0;
    while at < ground.spiral.len() {
        // One ring: every offset at this squared distance.
        let d2 = ring_key(ground.spiral.get(at));
        let mut filled: usize = 0;
        while at < ground.spiral.len() && ring_key(ground.spiral.get(at)) == d2 {
            let offset = ground.spiral.get(at).copied().unwrap_or([0, 0]);
            at = at.saturating_add(1);
            ground.charge(1);
            let x = ax.saturating_add(offset.first().copied().unwrap_or(0));
            let y = ay.saturating_add(offset.get(1).copied().unwrap_or(0));
            let Some(node) = ground.surface.node_of(x, y) else {
                continue;
            };
            let Some(site) = ground.standing(x, y) else {
                continue;
            };
            if !ground.surface.walkable(node)
                || ground.features.at_column(x, y).is_some()
                || !within(site, on_point, radius)
                || !ground.inside_own_sphere(seat, site)
                || ground.stacks_on_own(seat, x, y)
            {
                continue;
            }
            let dxc = i64::from(x.saturating_sub(cx));
            let dyc = i64::from(y.saturating_sub(cy));
            if let Some(slot) = ring.get_mut(filled) {
                *slot = (dxc * dxc + dyc * dyc, y, x);
                filled = filled.saturating_add(1);
            }
        }
        let candidates = ring.get_mut(..filled).unwrap_or_default();
        // item 62: the key ends in `x`, unique within a ring row, so it is
        // total.
        candidates.sort_unstable();
        for (_, y, x) in candidates.iter() {
            if let Some(node) = ground.surface.node_of(*x, *y)
                && ground.clusters.connected(ground.surface, start, node)
            {
                return Some([*x, *y]);
            }
        }
    }
    None
}

/// An offset's squared distance, the key the spiral's rings share.
fn ring_key(offset: Option<&[i32; 2]>) -> i64 {
    let dx = i64::from(offset.and_then(|o| o.first()).copied().unwrap_or(0));
    let dy = i64::from(offset.and_then(|o| o.get(1)).copied().unwrap_or(0));
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
    match spec {
        FeatureSpec::Name { kind, anchor } => {
            let feature = ground
                .features
                .index_of(kind, anchor)
                .filter(|index| ground.is_live(*index))
                .ok_or(StepFailure::NoTarget)?;
            let site =
                covering_site(ground, seat, commander, feature).ok_or(StepFailure::IllegalSite)?;
            Ok(Cover { feature, site })
        }
        FeatureSpec::Nearest { kind, uncovered } => {
            let mut ranker = Ranker::new(ground, column_of(commander), |index| {
                matches_pick(ground, seat, index, kind, uncovered)
            });
            let mut matched = false;
            while let Some((feature, _)) = ranker.next(ground, scratch) {
                matched = true;
                if let Some(site) = covering_site(ground, seat, commander, feature) {
                    return Ok(Cover { feature, site });
                }
            }
            Err(if matched {
                StepFailure::IllegalSite
            } else {
                StepFailure::NoTarget
            })
        }
        // `covered {}` is legal only under `on` (the compile refuses it here).
        FeatureSpec::Covered => Err(StepFailure::NoTarget),
    }
}

/// Whether the feature at `index` passes a pick's filters: its kind, alive,
/// and -- for `UNCOVERED` -- outside every sphere of `seat`'s own living
/// beacons, awake or dormant. Never reads another seat's state.
fn matches_pick(
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
/// `on` ranks **only vents** whose `on` column lies inside that sphere, and a
/// vent is a candidate only if no live Generator of any seat and no Build
/// target of this seat (but `except`'s) stands on its footprint
/// (targeting.md, "Sites").
///
/// # Errors
///
/// `no_target` when a name is absent, lost or not a vent, or nothing matches;
/// `illegal_site` when a named vent has no free column.
pub fn on_vent(
    ground: &Ground<'_>,
    scratch: &mut Scratch,
    seat: SeatId,
    centre: [Fx; 3],
    except: Option<BeaconId>,
    covered: Option<usize>,
    spec: FeatureSpec,
) -> Result<OnSite, StepFailure> {
    // A name is not ranked, but the vent it names must still be a legal `on`
    // site: one structure per voxel and one Generator per vent, so a vent a
    // live Generator of any seat -- or another of this seat's Build targets --
    // already stands on is `illegal_site` rather than a second tap that would
    // supply nothing (`power.rs`'s `supply_of`).
    let named = |feature: usize| -> Result<OnSite, StepFailure> {
        let Some(found) = ground.features.get(feature) else {
            return Err(StepFailure::NoTarget);
        };
        if found.kind != FeatureKind::Vent || !ground.is_live(feature) {
            return Err(StepFailure::NoTarget);
        }
        let taken = found.footprint.iter().any(|column| {
            ground.generator_on(column.x, column.y)
                || ground.own_target_on(seat, except, column.x, column.y)
        });
        if taken {
            return Err(StepFailure::IllegalSite);
        }
        let column = ground
            .on_column(feature, seat, except)
            .ok_or(StepFailure::IllegalSite)?;
        Ok(OnSite { feature, column })
    };
    match spec {
        FeatureSpec::Name { kind, anchor } => {
            let feature = ground
                .features
                .index_of(kind, anchor)
                .ok_or(StepFailure::NoTarget)?;
            named(feature)
        }
        FeatureSpec::Covered => named(covered.ok_or(StepFailure::NoTarget)?),
        FeatureSpec::Nearest { .. } => {
            let radius = ground.sphere_radius();
            let mut ranker = Ranker::new(ground, column_of(centre), |index| {
                on_candidate(ground, seat, except, centre, radius, index)
            });
            let (feature, _) = ranker.next(ground, scratch).ok_or(StepFailure::NoTarget)?;
            let column = ground
                .on_column(feature, seat, except)
                .ok_or(StepFailure::NoTarget)?;
            Ok(OnSite { feature, column })
        }
    }
}

/// Whether the vent at `index` is an `on` candidate for a beacon of `seat`
/// centred at `centre`.
fn on_candidate(
    ground: &Ground<'_>,
    seat: SeatId,
    except: Option<BeaconId>,
    centre: [Fx; 3],
    radius: i32,
    index: usize,
) -> bool {
    let Some(feature) = ground.features.get(index) else {
        return false;
    };
    if feature.kind != FeatureKind::Vent || !ground.is_live(index) {
        return false;
    }
    let taken = feature.footprint.iter().any(|column| {
        ground.generator_on(column.x, column.y)
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
        voxels.fill_from_heightmap(&heights, 3, Material::DIRT, Material::STONE);
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
        let surface =
            Surface::new(&voxels, StepCosts::new(10, 14, 4)).unwrap_or_else(|| panic!("a surface"));
        let mut scratch = Scratch::for_map(&surface, 32).unwrap_or_else(|| panic!("a scratch"));
        let clusters =
            Clusters::new(&surface, 32, &mut scratch).unwrap_or_else(|| panic!("clusters"));
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
        let (feature, _) = ranker
            .next(&ground, &mut map.scratch)
            .unwrap_or_else(|| panic!("a reachable vent"));
        map.features
            .get(feature)
            .unwrap_or_else(|| panic!("in the table"))
            .name()
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
        assert_eq!(nearest(&mut map, [10, 10]), "vent_10_45");
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
        let mut map = synthetic(
            |x, y| {
                (18..=26).contains(&x)
                    && (18..=26).contains(&y)
                    && !((21..=23).contains(&x) && (21..=23).contains(&y))
            },
            &[[22, 22], [50, 50]],
        );
        assert_eq!(nearest(&mut map, [5, 5]), "vent_50_50");
    }
}
