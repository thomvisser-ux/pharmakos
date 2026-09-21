// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The adapter between `plan-core`'s [`TravelEstimator`] seam and the sim's
//! travel estimator.
//!
//! Decisions-log item 100 (1) puts this file here and says why:
//!
//! > item 61's frozen signature is amended to what
//! > `crates/sim/src/pathing/estimate.rs` exports -- the first parameter is
//! > the sim's `&Surface` rather than the world, item 90's `Speed` is a seventh
//! > parameter, and `Estimate` carries an additive `fogged` field;
//! > plan-core's `TravelEstimator::estimate(start, goal)` stays the seam, and
//! > the adapter that owns a surface, a cluster decomposition, a scratch and a
//! > speed and implements it lives in the gateway, T13's, **because plan-core
//! > has no world**.
//!
//! # It owns its search structures, and that is the point
//!
//! [`RouteAdapter`] builds its **own** [`Surface`], [`Clusters`] and
//! [`Scratch`] from the world's voxel store, and never borrows the runner's.
//! That costs a second copy of the abstract graph (item 100 (3) accepted the
//! 14 MB at cluster 32), and it buys the one property this crate has to be
//! able to state plainly: **`estimate_route` cannot reach the sim's stepping
//! API, because it never has a `World` in its hand at all** (AGENTS.md section
//! 3 rule 2, "no dry runs"). A structural answer rather than a rule somebody
//! has to keep remembering.
//!
//! The structures are rebuilt from the world when the host says the terrain
//! moved ([`RouteAdapter::refresh`]), which at the skeleton is once per segment
//! boundary. Nothing here is a cache of *estimates*: item 61 says there is no
//! estimate, route or per-unit path cache, and there is none. What is held is
//! the graph the query runs over, which is the estimator's input.
//!
//! # Fog
//!
//! [`Fog`] is a mask the **gateway** produces, one entry per column, `true`
//! where the asking seat has never seen it -- "the sim only prices what it is
//! handed, which is what keeps fog a per-match server-side policy rather than
//! something baked into the graph" (`crates/sim/src/pathing/estimate.rs`).
//!
//! At the skeleton the sim computes no per-seat vision, so there is no mask to
//! hand it and every query runs [`Fog::Clear`]. That is stated rather than
//! hidden: [`RouteAdapter::set_unknown_columns`] is where a mask arrives when
//! one exists, `is_fogged` answers `false` without a search while the mask is
//! empty, and the editor therefore draws no dashed leg yet. A mask of "the
//! seat has seen nothing" would be the opposite mistake -- every leg a bound,
//! every route priced at 3/2, and an editor full of caveats the world does not
//! justify.

use std::cell::RefCell;

use pharmakos_plan_core::travel::{Estimate, TravelEstimator};
use pharmakos_proto::gp::v1::Voxel;
use pharmakos_sim::pathing::surface::StepCosts;
use pharmakos_sim::pathing::{Clusters, Fog, Node, Scratch, Speed, Surface, estimate};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::voxels::VoxelStore;

use crate::error::Error;

/// The sim's travel estimator, with its graph, its scratch and its speed bound.
///
/// `Debug` by hand: [`Scratch`] is a dense arena and printing it would be
/// megabytes of nothing (`missing_debug_implementations` is denied
/// workspace-wide, so the impl has to exist).
pub struct RouteAdapter {
    surface: Surface,
    clusters: Clusters,
    /// `RefCell` because [`TravelEstimator::estimate`] takes `&self` and the
    /// search needs `&mut Scratch`. Single-threaded, never held across a call,
    /// and `borrow_mut` failing would be a bug in this file rather than a
    /// state a caller can reach.
    scratch: RefCell<Scratch>,
    speed: Speed,
    cluster_voxels: i32,
    /// One entry per column, `true` where the asking seat has never seen it.
    /// Empty means [`Fog::Clear`] -- see the module docs.
    unknown: Vec<bool>,
}

impl std::fmt::Debug for RouteAdapter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RouteAdapter")
            .field("nodes", &self.surface.node_count())
            .field("cluster_voxels", &self.cluster_voxels)
            .field("speed", &self.speed)
            .field(
                "fogged_columns",
                &self.unknown.iter().fold(0_usize, |sum, hidden| {
                    if *hidden { sum.saturating_add(1) } else { sum }
                }),
            )
            // The three search structures are dense arenas: printing one would
            // be megabytes of nothing.
            .finish_non_exhaustive()
    }
}

impl RouteAdapter {
    /// Build the adapter's own graph over a world's voxel store.
    ///
    /// `speed` is the walker the editor renders a route at, which is the
    /// commander until a playbook names another (item 57).
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when the store or the rules table will
    /// not produce a graph -- a map with no walkable column, or a cluster edge
    /// the extent cannot be divided by. Neither is something a caller did.
    pub fn new(voxels: &VoxelStore, rules: &RulesTable) -> Result<RouteAdapter, Error> {
        let cluster_voxels = rules.hpa_cluster_voxels();
        let surface = Surface::new(voxels, StepCosts::from_rules(rules)).ok_or_else(|| {
            Error::internal("this map has no walkable surface to estimate a route over")
        })?;
        let mut scratch = Scratch::for_map(&surface, cluster_voxels).ok_or_else(|| {
            Error::internal("this map and cluster edge do not size a search scratch")
        })?;
        let clusters = Clusters::new(&surface, cluster_voxels, &mut scratch).ok_or_else(|| {
            Error::internal("this map and cluster edge do not decompose into clusters")
        })?;
        Ok(RouteAdapter {
            surface,
            clusters,
            scratch: RefCell::new(scratch),
            speed: Speed::commander(rules),
            cluster_voxels,
            unknown: Vec::new(),
        })
    }

    /// Rebuild the graph, because the terrain moved.
    ///
    /// The whole graph rather than the dirtied clusters: the sim's own eager
    /// repair (item 60) runs inside the tick on the world's copy, and this
    /// copy is read once per Lull, so a rebuild at the segment boundary is
    /// both simpler and impossible to get subtly wrong. A Push that craters
    /// half the map costs one rebuild here, off the tick.
    ///
    /// # Errors
    ///
    /// As [`RouteAdapter::new`].
    pub fn refresh(&mut self, voxels: &VoxelStore, rules: &RulesTable) -> Result<(), Error> {
        let rebuilt = RouteAdapter::new(voxels, rules)?;
        let unknown = std::mem::take(&mut self.unknown);
        *self = rebuilt;
        self.unknown = unknown;
        Ok(())
    }

    /// Hand the adapter the asking seat's fog mask: one entry per column,
    /// `true` where that seat has never seen it.
    ///
    /// An empty slice is [`Fog::Clear`]. A slice of the wrong length is
    /// refused rather than padded, because a short mask would silently price
    /// the tail of the map as seen.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when the mask is neither empty nor one
    /// entry per column.
    pub fn set_unknown_columns(&mut self, unknown: &[bool]) -> Result<(), Error> {
        let nodes = usize::try_from(self.surface.node_count()).unwrap_or(usize::MAX);
        if !unknown.is_empty() && unknown.len() != nodes {
            return Err(Error::internal(format!(
                "a fog mask is one entry per column: this map has {nodes} and the mask has {}",
                unknown.len()
            )));
        }
        self.unknown = unknown.to_vec();
        Ok(())
    }

    /// How many columns the graph has. For a host sizing a fog mask.
    #[must_use]
    pub fn columns(&self) -> u32 {
        self.surface.node_count()
    }

    /// The cluster edge the graph was decomposed at, in voxels.
    #[must_use]
    pub const fn cluster_voxels(&self) -> i32 {
        self.cluster_voxels
    }

    /// The node a voxel's column is, or `None` when the voxel is off the map.
    fn node_of(&self, at: Voxel) -> Option<Node> {
        self.surface.node_of(at.x, at.y)
    }

    /// The fog the estimator prices this query under.
    fn fog(&self) -> Fog<'_> {
        if self.unknown.is_empty() {
            Fog::Clear
        } else {
            Fog::Unknown(&self.unknown)
        }
    }

    /// One query, straight through to item 61's function.
    fn query(&self, start: Voxel, goal: Voxel) -> Option<pharmakos_sim::pathing::Estimate> {
        let start = self.node_of(start)?;
        let goal = self.node_of(goal)?;
        let mut scratch = self.scratch.try_borrow_mut().ok()?;
        estimate(
            &self.surface,
            &self.clusters,
            &mut scratch,
            start,
            goal,
            self.fog(),
            self.speed,
        )
    }
}

impl TravelEstimator for RouteAdapter {
    fn estimate(&self, start: Voxel, goal: Voxel) -> Option<Estimate> {
        let found = self.query(start, goal)?;
        // The sim counts ticks as `i32` and plan-core as `u32`; the estimator
        // never returns a negative one, and a conversion is a decision
        // (AGENTS.md section 4.3), so a negative is no estimate rather than a
        // number nobody meant.
        let ticks = u32::try_from(found.ticks).ok()?;
        Some(Estimate {
            cost: found.cost,
            ticks,
            legs: found.legs,
        })
    }

    fn is_fogged(&self, start: Voxel, goal: Voxel) -> bool {
        // No mask, no fogged leg, and no second search to find that out.
        if self.unknown.is_empty() {
            return false;
        }
        self.query(start, goal).is_some_and(|found| found.fogged)
    }
}

#[cfg(test)]
mod tests {
    use super::RouteAdapter;
    use pharmakos_plan_core::travel::TravelEstimator;
    use pharmakos_proto::gp::v1::Voxel;
    use pharmakos_sim::rules::RulesTable;
    use pharmakos_sim::world::{World, WorldConfig};

    fn rules() -> RulesTable {
        RulesTable::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("rules")
                .join("rules.v1.json"),
        )
        .expect("the shipped rules table")
    }

    fn world() -> World {
        World::new(&WorldConfig {
            match_seed: 0x0000_0000_ca5c_aded,
            seats: 2,
            units_per_seat: 0,
            rules: rules(),
            match_settings: pharmakos_sim::runner::MatchSettings::default(),
        })
        .expect("a generated world")
    }

    #[test]
    fn a_route_over_the_generated_map_is_estimated_and_is_not_fogged() {
        let world = world();
        let adapter = RouteAdapter::new(world.voxels(), world.rules()).expect("a graph");
        assert!(adapter.columns() > 0);
        assert_eq!(adapter.cluster_voxels(), world.rules().hpa_cluster_voxels());

        // The commander and its own core beacon: two places the generator put
        // on the same map for the same seat, so a route between them is a
        // route the match itself depends on.
        let seat = pharmakos_sim::tables::SeatId::new(0);
        let commander = world.commander_of(seat);
        let row = world
            .units()
            .ids()
            .iter()
            .position(|id| *id == commander.raw())
            .expect("the commander is in the unit table");
        let start = world
            .units()
            .positions()
            .get(row)
            .copied()
            .map(crate::view::voxel_of)
            .expect("the commander is on the map");
        let beacon = world
            .beacons()
            .seats()
            .iter()
            .position(|held| *held == seat.raw())
            .expect("the seat has a core beacon");
        let goal = world
            .beacons()
            .positions()
            .get(beacon)
            .copied()
            .map(crate::view::voxel_of)
            .expect("the beacon is on the map");

        let found = adapter.estimate(start, goal);
        assert!(
            found.is_some(),
            "a commander can walk to its own core: {start:?} -> {goal:?}"
        );
        assert!(
            !adapter.is_fogged(start, goal),
            "no vision, no mask, no dashed leg"
        );
    }

    #[test]
    fn a_voxel_off_the_map_has_no_route_rather_than_a_panic() {
        let world = world();
        let adapter = RouteAdapter::new(world.voxels(), world.rules()).expect("a graph");
        let off = Voxel { x: -1, y: -1, z: 0 };
        let on = Voxel { x: 0, y: 0, z: 0 };
        assert_eq!(adapter.estimate(off, on), None);
        assert_eq!(adapter.estimate(on, off), None);
        assert!(!adapter.is_fogged(off, on));
    }

    #[test]
    fn a_fog_mask_of_the_wrong_length_is_refused_rather_than_padded() {
        let world = world();
        let mut adapter = RouteAdapter::new(world.voxels(), world.rules()).expect("a graph");
        assert!(adapter.set_unknown_columns(&[]).is_ok(), "empty is Clear");
        assert!(
            adapter.set_unknown_columns(&[true, false]).is_err(),
            "a short mask would price the tail of the map as seen"
        );
        let all = vec![false; usize::try_from(adapter.columns()).expect("fits")];
        assert!(adapter.set_unknown_columns(&all).is_ok());
    }
}
