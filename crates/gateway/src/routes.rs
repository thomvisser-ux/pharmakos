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
//! # One graph, the hosted world's
//!
//! [`RouteAdapter`] borrows the hosted world's own [`Surface`] and
//! [`Clusters`] ([`RouteAdapter::over`]) and searches in the match's one
//! planning scratch ([`PlanningScratch`]). It used to build a second copy of the graph from the
//! voxel store and rebuild it at every Lull; that copy is gone (S3's `p3fix`,
//! the gateway step), for three reasons:
//!
//! * **One measure.** `estimate_route` prices a leg at the walk the router
//!   serves ([`pharmakos_sim::pathing::estimate()`], which for a clear query is
//!   [`pharmakos_sim::pathing::estimate::walk_cost`]'s walk), and so do
//!   `get_map_summary`'s `travel_ms` and `resolve_refs`' candidates, through
//!   the sim's [`pharmakos_sim::targeting::Ranker`], over the world's graph
//!   ([`crate::targeting`]). Over one graph the four readers -- a chip, a
//!   binding, a route and the Push -- agree by construction, not because a
//!   rebuild was remembered: `crates/gateway/tests/targeting.rs`'s
//!   `every_lull_reads_and_plays_on_as_its_save_does` holds them equal at
//!   every Lull of a three-round match, live and resumed.
//! * **The graph is already settled.** The sim settles its graph in the
//!   phase that edits the terrain (lane `settle`), and nothing steps the
//!   world in a Lull, so the world's graph *is* the terrain a Lull plans on.
//!   A copy could only be that graph or a stale one.
//! * **What the copy bought holds without it.** The copy's case was that
//!   `estimate_route` should never hold a `World`, so it cannot reach the
//!   sim's stepping API (AGENTS.md section 3 rule 2, "no dry runs"). The
//!   adapter holds a [`Surface`] and a [`Clusters`] by shared reference and
//!   no `World`: a surface answers "is this step legal and what does it
//!   cost", the clusters "is there a route at all", and neither can step
//!   anything. The structural answer stands, without 14 MB of second graph
//!   (item 100 (3)) and a rebuild per Lull.
//!
//! Nothing here is a cache of *estimates*: item 61 says there is no estimate,
//! route or per-unit path cache, and there is none.
//!
//! # One scratch, the hosted match's
//!
//! A [`Scratch`] is the searches' working memory: dense arrays, one entry per
//! column, megabytes on the shipped map, which every search overwrites by an
//! epoch stamp, so that an answer never depends on what the scratch held
//! before (that is what lets the sim keep one for a whole match, and what a
//! restored match's hash chain proves at every resume). Every planning read
//! used to make its own and drop it, which cost a few megabytes of zeroes per
//! call: several times P3's 1 ms bar for `estimate_route` on its own (S3's
//! `p3est`, on `cert3`'s measurement). [`PlanningScratch`] is made once, at
//! the match's first planning read, kept by the [`crate::host::Host`], and
//! lent to each read in turn: the route estimate here, and
//! [`crate::targeting`]'s rankings, sites and previews. It holds no answer and
//! is no part of the match: never hashed, never saved, and a resumed match
//! makes its own.
//!
//! # Fog
//!
//! [`Fog`] is a mask the **gateway** produces, one entry per column, `true`
//! where the asking seat has never seen it -- "the sim only prices what it is
//! handed, which is what keeps fog a per-match server-side policy rather than
//! something baked into the graph" (`crates/sim/src/pathing/estimate.rs`).
//!
//! The sim does compute per-seat vision, through its one sensor interface
//! (S2's plan, E10), but it keeps no record of which **columns** a seat has
//! seen: the knowledge store holds sightings of assets, not explored terrain,
//! and the terrain mask is the knowledge store's (the view feed's own marker
//! says the same). So there is no mask to hand the estimator yet, and every
//! query runs [`Fog::Clear`]: a route is priced over the ground as it stands,
//! voxel edits a seat has not seen included -- accepted until the mask (the
//! main session's ruling R4 on the reviews of #128). That is stated rather
//! than hidden: [`RouteAdapter::with_unknown_columns`] is where a mask arrives
//! when one exists, `is_fogged` answers `false` without a search while there
//! is none, and the editor therefore draws no dashed leg yet. A mask of "the
//! seat has seen nothing" would be the opposite mistake -- every leg a bound,
//! every route priced at 3/2, and an editor full of caveats the world does not
//! justify.
//!
//! # A fault is raised, not read as "no route"
//!
//! plan-core's [`TravelEstimator`] answers `Option`: `None` is the
//! connectivity oracle's "no route", a normal answer. A fault in the query
//! itself -- no scratch for the map, the scratch already borrowed, a walk the
//! sim could not price ([`pharmakos_sim::pathing::estimate::EstimateError`]),
//! a negative tick count -- is not "no route", and the trait has no way to say
//! so; changing it is plan-core's (carried). So the adapter keeps the first
//! fault it meets ([`RouteAdapter::take_fault`]) and its caller raises it as a
//! typed error after the call, rather than drawing an unreachable leg over a
//! bug.

use pharmakos_proto::said;
use std::cell::{Cell, RefCell, RefMut};

use pharmakos_plan_core::travel::{Estimate, TravelEstimator};
use pharmakos_proto::gp::v1::Voxel;
use pharmakos_sim::pathing::{Clusters, Fog, Node, Scratch, Speed, Surface, estimate};
use pharmakos_sim::world::World;

use crate::error::Error;

/// The hosted match's one search scratch for planning reads (the module
/// docs, "One scratch").
///
/// A `RefCell` because every reader holds the host by shared reference and a
/// search needs `&mut Scratch`; an `Option` because the scratch is made at the
/// first read that searches, so that opening a match cannot fail on it and a
/// match nobody plans in never pays for it. Single-threaded, and lent for one
/// search at a time: a read that finds it already lent is a fault, raised
/// typed, never a second scratch made on the quiet.
///
/// `Debug` by hand: [`Scratch`] is a dense arena and printing it would be
/// megabytes of nothing (`missing_debug_implementations` is denied
/// workspace-wide, so the impl has to exist).
pub struct PlanningScratch(RefCell<Option<Scratch>>);

impl std::fmt::Debug for PlanningScratch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let made = self.0.try_borrow().map(|held| held.is_some());
        formatter
            .debug_struct("PlanningScratch")
            .field("made", &made)
            .finish()
    }
}

impl Default for PlanningScratch {
    fn default() -> PlanningScratch {
        PlanningScratch::new()
    }
}

impl PlanningScratch {
    /// No scratch yet: the first [`PlanningScratch::lend`] makes it.
    #[must_use]
    pub const fn new() -> PlanningScratch {
        PlanningScratch(RefCell::new(None))
    }

    /// The scratch, for one search over `surface` at `cluster_voxels`, made
    /// now if this is the match's first.
    ///
    /// Every caller hands it the hosted world's own surface and cluster edge,
    /// which do not change while a match is hosted (a voxel edit moves no
    /// column, and a resume restores into the map its config line generated),
    /// so the scratch made for the first read is sized for every later one.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when the scratch is already lent (a
    /// read that searches inside another read's search), and when the map and
    /// cluster edge do not size one, which the world's own construction would
    /// already have refused.
    pub fn lend(
        &self,
        surface: &Surface,
        cluster_voxels: i32,
    ) -> Result<RefMut<'_, Scratch>, Error> {
        let mut held = self.0.try_borrow_mut().map_err(|_| {
            Error::internal(said!(
                "a planning read found the match's search scratch already in use"
            ))
        })?;
        if held.is_none() {
            *held = Scratch::for_map(surface, cluster_voxels);
        }
        RefMut::filter_map(held, Option::as_mut).map_err(|_| {
            Error::internal(said!(
                "this map and cluster edge do not size a search scratch"
            ))
        })
    }

    /// How many walks the scratch has priced since it was made, or `None`
    /// while no read has needed it: the sim's own tally
    /// ([`Scratch::walks`]), for a test that holds a read to pricing none.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when the scratch is lent, or its
    /// tally passed what it counts.
    pub fn walks(&self) -> Result<Option<u64>, Error> {
        let held = self.0.try_borrow().map_err(|_| {
            Error::internal(said!(
                "the match's search scratch is in use and its tally cannot be read"
            ))
        })?;
        held.as_ref()
            .map(|scratch| {
                scratch.walks().map_err(|_| {
                    Error::internal(said!(
                        "the search scratch's walk tally passed what it counts"
                    ))
                })
            })
            .transpose()
    }
}

/// The sim's travel estimator over the hosted world's graph, searching in the
/// match's planning scratch, at the commander's speed.
///
/// `Debug` by hand: a held fault is an [`Error`] in a `Cell`, which derives
/// none.
pub struct RouteAdapter<'a> {
    surface: &'a Surface,
    clusters: &'a Clusters,
    /// The match's one planning scratch, borrowed for each query
    /// ([`RouteAdapter::query`]) and never held across one.
    scratch: &'a PlanningScratch,
    speed: Speed,
    cluster_voxels: i32,
    /// One entry per column, `true` where the asking seat has never seen it.
    /// Empty means [`Fog::Clear`] -- see the module docs.
    unknown: &'a [bool],
    /// The first fault a query met since the last [`RouteAdapter::take_fault`]
    /// (the module docs, "A fault is raised").
    fault: Cell<Option<Error>>,
}

impl std::fmt::Debug for RouteAdapter<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RouteAdapter")
            .field("nodes", &self.surface.node_count())
            .field("cluster_voxels", &self.cluster_voxels)
            .field("speed", &self.speed)
            .field(
                "fogged_columns",
                &self.unknown.iter().filter(|hidden| **hidden).count(),
            )
            .field("scratch", self.scratch)
            .finish_non_exhaustive()
    }
}

impl<'a> RouteAdapter<'a> {
    /// The estimator over `world`'s own graph, searching in `scratch`, at the
    /// commander's speed, under no fog mask (the module docs, "Fog").
    ///
    /// `speed` is the walker the editor renders a route at, which is the
    /// commander until a playbook names another (item 57). The adapter keeps
    /// the world's surface and clusters and none of the rest of it.
    #[must_use]
    pub fn over(world: &'a World, scratch: &'a PlanningScratch) -> RouteAdapter<'a> {
        RouteAdapter {
            surface: world.surface(),
            clusters: world.clusters(),
            scratch,
            speed: Speed::commander_of(world),
            cluster_voxels: world.rules().hpa_cluster_voxels(),
            unknown: &[],
            fault: Cell::new(None),
        }
    }

    /// The same estimator under the asking seat's fog mask: one entry per
    /// column, `true` where that seat has never seen it.
    ///
    /// An empty slice is [`Fog::Clear`]. A slice of the wrong length is
    /// refused rather than padded, because a short mask would silently price
    /// the tail of the map as seen.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when the mask is neither empty nor one
    /// entry per column.
    pub fn with_unknown_columns(self, unknown: &'a [bool]) -> Result<RouteAdapter<'a>, Error> {
        let nodes = usize::try_from(self.surface.node_count()).map_err(|_| {
            Error::internal(said!(
                "this map has more columns than this host's usize counts"
            ))
        })?;
        if !unknown.is_empty() && unknown.len() != nodes {
            return Err(Error::internal(said!(
                "a fog mask is one entry per column: this map has {} and the mask has {}",
                nodes,
                unknown.len()
            )));
        }
        Ok(RouteAdapter { unknown, ..self })
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
            Fog::Unknown(self.unknown)
        }
    }

    /// The first fault a query has met since this was last asked, taken, so
    /// that the caller of a [`TravelEstimator`] call raises it.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`]: the fault, as the query met it.
    pub fn take_fault(&self) -> Result<(), Error> {
        match self.fault.take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Keep `error` as this adapter's fault, unless one is held already: the
    /// first is the one that explains the rest.
    fn fault(&self, error: Error) {
        let first = match self.fault.take() {
            Some(held) => held,
            None => error,
        };
        self.fault.set(Some(first));
    }

    /// One query, straight through to item 61's function. A voxel off the map
    /// has no route, which is an answer; no scratch for the map, a scratch
    /// already borrowed and a walk the sim could not price are faults
    /// ([`RouteAdapter::take_fault`]).
    fn query(&self, start: Voxel, goal: Voxel) -> Option<pharmakos_sim::pathing::Estimate> {
        let start = self.node_of(start)?;
        let goal = self.node_of(goal)?;
        let mut scratch = match self.scratch.lend(self.surface, self.cluster_voxels) {
            Ok(scratch) => scratch,
            Err(error) => {
                self.fault(error);
                return None;
            }
        };
        match estimate(
            self.surface,
            self.clusters,
            &mut scratch,
            start,
            goal,
            self.fog(),
            self.speed,
        ) {
            Ok(found) => found,
            Err(error) => {
                self.fault(Error::internal(said!(
                    "the sim priced no walk for a route's leg: {}",
                    crate::error::engine(&error)
                )));
                None
            }
        }
    }
}

impl TravelEstimator for RouteAdapter<'_> {
    fn estimate(&self, start: Voxel, goal: Voxel) -> Option<Estimate> {
        let found = self.query(start, goal)?;
        // The sim counts ticks as `i32` and plan-core as `u32`; the estimator
        // never returns a negative one, and a conversion is a decision
        // (AGENTS.md section 4.3), so a negative is a fault the caller raises
        // rather than a number nobody meant or a route that is not there.
        let Ok(ticks) = u32::try_from(found.ticks) else {
            self.fault(Error::internal(said!(
                "the estimator priced a leg at {} ticks, below zero",
                found.ticks
            )));
            return None;
        };
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
    use super::{PlanningScratch, RouteAdapter};
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
            rules: rules(),
            match_settings: pharmakos_sim::runner::MatchSettings::default(),
        })
        .expect("a generated world")
    }

    #[test]
    fn a_route_over_the_generated_map_is_estimated_and_is_not_fogged() {
        let world = world();
        let scratch = PlanningScratch::new();
        let adapter = RouteAdapter::over(&world, &scratch);
        assert!(adapter.columns() > 0);
        assert_eq!(adapter.cluster_voxels(), world.rules().hpa_cluster_voxels());

        // The commander and its own core beacon: two places the generator put
        // on the same map for the same seat, so a route between them is a
        // route the match itself depends on.
        let seat = pharmakos_sim::tables::SeatId::new(0);
        let commander = world.commander_of(seat).expect("seat 0 has a commander");
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
        assert!(adapter.take_fault().is_ok(), "no query faulted");
    }

    #[test]
    fn a_voxel_off_the_map_has_no_route_rather_than_a_panic() {
        let world = world();
        let scratch = PlanningScratch::new();
        let adapter = RouteAdapter::over(&world, &scratch);
        let off = Voxel { x: -1, y: -1, z: 0 };
        let on = Voxel { x: 0, y: 0, z: 0 };
        assert_eq!(adapter.estimate(off, on), None);
        assert_eq!(adapter.estimate(on, off), None);
        assert!(!adapter.is_fogged(off, on));
        assert!(adapter.take_fault().is_ok(), "off the map is an answer");
    }

    #[test]
    fn a_fog_mask_of_the_wrong_length_is_refused_rather_than_padded() {
        let world = world();
        let scratch = PlanningScratch::new();
        assert!(
            RouteAdapter::over(&world, &scratch)
                .with_unknown_columns(&[])
                .is_ok(),
            "empty is Clear"
        );
        assert!(
            RouteAdapter::over(&world, &scratch)
                .with_unknown_columns(&[true, false])
                .is_err(),
            "a short mask would price the tail of the map as seen"
        );
        let all = vec![
            false;
            usize::try_from(RouteAdapter::over(&world, &scratch).columns())
                .expect("fits")
        ];
        assert!(
            RouteAdapter::over(&world, &scratch)
                .with_unknown_columns(&all)
                .is_ok()
        );
    }

    /// Columns spread over the whole map, corner to corner.
    fn places() -> Vec<Voxel> {
        let mut out = Vec::new();
        for x in (8..384).step_by(92) {
            for y in (8..384).step_by(92) {
                out.push(Voxel { x, y, z: 0 });
            }
        }
        out
    }

    /// S3's `p3est`: the match's planning scratch is working memory and no
    /// cache. Each leg's answer from a scratch that has served every other
    /// leg, in either order, is the answer a scratch made for that leg alone
    /// gives.
    #[test]
    fn an_answer_does_not_depend_on_what_the_scratch_held() {
        let world = world();
        let places = places();
        let legs: Vec<(Voxel, Voxel)> = places
            .iter()
            .zip(places.iter().rev())
            .map(|(from, to)| (*from, *to))
            .collect();
        assert!(legs.len() >= 16, "a spread of legs: {}", legs.len());
        let alone: Vec<_> = legs
            .iter()
            .map(|(from, to)| {
                let scratch = PlanningScratch::new();
                let adapter = RouteAdapter::over(&world, &scratch);
                let found = adapter.estimate(*from, *to);
                assert!(adapter.take_fault().is_ok(), "no query faulted");
                found
            })
            .collect();
        assert!(
            alone.iter().filter(|found| found.is_some()).count() >= 8,
            "most of the spread is walkable, or this compares nothing: {alone:?}"
        );
        let shared = PlanningScratch::new();
        let adapter = RouteAdapter::over(&world, &shared);
        let forward: Vec<_> = legs
            .iter()
            .map(|(from, to)| adapter.estimate(*from, *to))
            .collect();
        let mut backward: Vec<_> = legs
            .iter()
            .rev()
            .map(|(from, to)| adapter.estimate(*from, *to))
            .collect();
        backward.reverse();
        assert!(adapter.take_fault().is_ok(), "no query faulted");
        assert_eq!(forward, alone, "after every earlier leg");
        assert_eq!(backward, alone, "after every leg, in the other order");
    }

    /// The scratch is made by the first read that searches, and lent for one
    /// search at a time: a read that finds it lent is a fault raised typed,
    /// never a second scratch and never "no route".
    #[test]
    fn the_scratch_is_made_once_and_lent_one_search_at_a_time() {
        use crate::error::Code;
        let world = world();
        let edge = world.rules().hpa_cluster_voxels();
        let scratch = PlanningScratch::new();
        assert_eq!(
            scratch.walks().expect("not lent"),
            None,
            "no read has searched, so there is no scratch"
        );
        let lent = scratch.lend(world.surface(), edge).expect("the first loan");
        assert_eq!(
            scratch
                .lend(world.surface(), edge)
                .err()
                .map(|error| error.code()),
            Some(Code::Internal),
            "a second loan while the first is out"
        );
        assert_eq!(
            scratch.walks().err().map(|error| error.code()),
            Some(Code::Internal),
            "its tally is not read while it is lent"
        );
        // A route query while the scratch is lent elsewhere is a fault the
        // caller raises, not an unreachable leg.
        let adapter = RouteAdapter::over(&world, &scratch);
        let [from, to] = [
            Voxel { x: 8, y: 8, z: 0 },
            Voxel {
                x: 100,
                y: 100,
                z: 0,
            },
        ];
        assert_eq!(adapter.estimate(from, to), None);
        assert_eq!(
            adapter.take_fault().err().map(|error| error.code()),
            Some(Code::Internal)
        );
        drop(lent);
        assert_eq!(
            scratch.walks().expect("returned"),
            Some(0),
            "made, and it has priced no walk"
        );
        assert!(scratch.lend(world.surface(), edge).is_ok(), "lent again");
        assert!(
            adapter.estimate(from, to).is_some(),
            "and the query answers"
        );
        assert!(adapter.take_fault().is_ok());
    }
}
