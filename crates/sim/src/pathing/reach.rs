// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The grid optimum from one column to every column: Dijkstra over the
//! surface graph, undirected or directed at a set of spheres (lane `p3fix`).
//!
//! # What it is for
//!
//! A `toward` step's stepping stone is the legal site inside the seat's
//! spheres closest to the feature over the terrain ([`crate::targeting`]'s
//! "Two measures"): the least cost of any walk from the site to the feature,
//! over [`Surface::neighbour`], the walkers' own step rule at the rules' step
//! costs. One search outward from the feature gives every column it settles
//! that cost exactly. Directed at the seat's spheres ("Directed", below), it
//! settles columns in ascending `cost + h`; `targeting`'s `stone_site` stops
//! once the key passes the least cost a site settled at, and takes the lowest
//! `(y, x)` among the sites at that cost.
//!
//! **The bound.** A search pops at most the map's columns; its work a pop is
//! a constant (eight neighbours) and one `h` a column reached, each `h`
//! costing one octile a sphere, and a sphere is only one that listed a site,
//! at most a seat's beacons (`room_per_seat` of the world's 40, 20 at two
//! seats, and its core: 21). So a resolve costs at most a full directed
//! flood: 147 456 pops and as many `h`, measured at 74, 103 and 148 ms for
//! three seats at 1, 18 and 40 spheres (release, a machine not idle), against
//! 64 ms undirected. That bound is not reached: a resolve with no listed site
//! searches nothing, every listed site is in the target's component, and
//! the search stops at the first site; the largest resolve built (the
//! farthest vent behind a long wall) popped 46 791 nodes in 7.5 ms. Every
//! resolve also lists its sites first, directed or not: 36 513 sites over
//! 37 653 columns at a seat's most beacons, 2.5 ms.
//!
//! The first measure the stone had, the walk the router would serve from each
//! site, needed a lower bound to prune by and priced every site whose bound
//! fell short of the best walk; behind a wall the octile bound pruned
//! nothing (4.5 M nodes popped on the probation seed), and even this search's
//! exact bound left a band of sites on a far vent (2.9 M at worst), the gap
//! between the optimum and the router's walk. Nobody walks from the stone to
//! the feature when it is chosen, so that walk was never the measure.
//!
//! # Direction
//!
//! The search runs outward from the vent, where a walk runs toward it, so it
//! reads each step backwards. [`Surface::neighbour`]'s step rule is symmetric
//! in legality and in cost (a climb is charged either way, and a diagonal's
//! sides are checked against both endpoints), so the forward graph is its own
//! reverse. `tests/nearest.rs`'s `the_step_rule_is_symmetric` holds every
//! step of a generated, scarred map to it; an asymmetric rule would have to
//! read [`Surface::step_cost_between`] from each neighbour instead.
//!
//! # The queue, and its order
//!
//! Step costs are small integers (a cardinal or diagonal step and a climb,
//! the rules' `locomotion` rows, bounded at load so a path's cost fits an
//! `i32`), so the frontier is a bucket queue (Dial's): a ring of buckets
//! holds every key the frontier holds at once (below). A column sits in at
//! most one bucket, linked through two arrays and unlinked when its key is
//! lowered.
//!
//! **Undirected** ([`Reach::begin`]), keys are costs, and a step raises a
//! cost by at least one unit (the rules refuse a free step), so nothing joins
//! a bucket once it is reached: the queue sorts each bucket's columns when it
//! reaches it, and columns settle in ascending `(cost, column)`, the order a
//! binary heap over that key gives. That order is this API's promise;
//! `tests/nearest.rs`'s `the_grid_optimum_settles_in_cost_then_column_order`
//! holds it to a Dijkstra of its own. Measured against such a heap on full
//! floods of generated and scarred maps (fix pass 2), the order was identical
//! and the queue about 1.6 times faster (a full flood of the shipped map:
//! 23.6 ms against 37.3 ms, release, on a machine that was not idle).
//!
//! **Directed** ([`Reach::begin_toward`]), keys are `cost + h`, and a step
//! can keep a key, so a column can join the bucket being settled; the queue
//! reads that bucket again before it moves on. Columns settle in ascending
//! key, in the order each bucket's columns were linked: no order within a
//! key is promised, and the caller decides among equal keys by its own rule
//! (`stone_site`'s least `(cost, y, x)`, the one home of the stone's
//! tie-break).
//!
//! # Directed
//!
//! A `toward` search knows where the sites are: in the spheres that listed
//! them. So it runs best-first, keyed `cost + h`, where `h` is a lower bound
//! on the cost from a column to the nearest site: over the spheres that
//! listed at least one site, the octile cost from the column to the sphere's
//! centre less the sphere's reach, the most octile cost from its centre to
//! any site **it** listed (each site is listed by one sphere, the first that
//! holds it), floored at zero, and the least over spheres. A column's `h` is
//! computed once a search, when the search first reaches it, and read from
//! then on.
//!
//! - **Admissible.** The octile cost is a metric, and every step costs at
//!   least its octile length (a climb only adds), so a walk from `n` to a
//!   site `s` listed by sphere `b` costs at least `octile(n, s) ≥ octile(n,
//!   b) − octile(s, b) ≥ octile(n, b) − reach(b)`.
//! - **Consistent.** Each term changes across one step by at most that
//!   step's octile length, so `h(n) ≤ step(n, m) + h(m)`, and a key never
//!   falls along a step: a column settles at its least cost, once.
//!
//! A key rises across one step by at most that step's cost and its octile
//! length, so by less than twice the dearest step, and the ring has `2 · max
//! step + 1` buckets (a ring of `2 · max step` is enough whenever the climb
//! surcharge is above zero, since a step's octile length is then below the
//! dearest step; the committed table's is). Every site settles at `cost + 0`
//! (a site's own sphere reaches it), so the first site settled is at the
//! least cost. `tests/nearest.rs` holds `h`, as the production sphere list
//! gives it, to the true cost to the nearest site on generated and scarred
//! maps, and the stone to brute force.
//!
//! # No allocation
//!
//! Every buffer is one entry per column, or one per bucket, or one per beacon
//! row for the spheres, reserved where the scratch is made; a reached bucket's
//! columns are read from a buffer of one entry per column.

use crate::pathing::search::Tally;
use crate::pathing::surface::{NEIGHBOURS, Node, Surface};

/// No column: an empty link.
const NONE: u32 = u32::MAX;

/// The most beacon rows any table holds ([`crate::ids::ROOM_LIMIT`]): the
/// room the directed search's sphere list is reserved for.
const SPHERE_ROOM: usize = 4_095;
const _: () = assert!(
    crate::math::index::row(crate::ids::ROOM_LIMIT) <= SPHERE_ROOM,
    "the sphere list holds every beacon row"
);

/// Why a [`Reach`] search cannot go on: a column the search was handed that
/// it has no room for. Typed rather than read as "no more
/// columns" or "no direction": a caller that meets one counts it (the
/// targeting module's `Scratch::reach_faults`) and its tests hold the count
/// at zero.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReachError {
    /// A column that is not one of the map's the search was sized for: a
    /// start, or a column the queue holds.
    OffMap {
        /// The column.
        node: Node,
    },
}

/// The grid optimum's search, its bucket queue (the module docs), the
/// site marks a `toward` search reads beside it, and the spheres a directed
/// search heads for.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reach {
    /// The search's generation: a column's cost is this search's when its
    /// stamp equals it.
    epoch: u32,
    stamp: Vec<u32>,
    settled: Vec<u32>,
    /// Each column's cost from the start, `g`.
    cost: Vec<i64>,
    /// Each column's key, `g + h`, the bucket it waits in.
    key: Vec<i64>,
    /// The epoch a column was linked into a bucket in; 0 once unlinked.
    listed: Vec<u32>,
    next: Vec<u32>,
    prev: Vec<u32>,
    /// The ring of buckets, by key modulo its length.
    heads: Vec<u32>,
    /// The bucket being settled, sorted by column, and where in it.
    sorted: Vec<Node>,
    cursor: usize,
    /// The key the search has reached.
    at: i64,
    /// Columns linked into a bucket.
    waiting: usize,
    /// The marks' generation, apart from the search's so the sites can be
    /// marked before the search starts.
    site_epoch: u32,
    sites: Vec<u32>,
    /// The spheres a directed search heads for: each one's centre column and
    /// the most octile cost from it to any column of the sphere.
    spheres: Vec<([i32; 2], i64)>,
    /// Each column's `h` in this search with the epoch it was computed in,
    /// computed once when the search first reaches the column; valid while
    /// the stamp is the search's epoch.
    heur: Vec<(u32, i64)>,
    /// How many `h` this search computed: one a column reached, each costing
    /// one octile a sphere. Counted work, never hashed.
    evaluations: Tally,
    /// Whether the search in progress is directed by [`Reach::spheres`].
    directed: bool,
}

impl Reach {
    /// Room for a map of `nodes` columns whose dearest step costs
    /// `max_step` ([`crate::pathing::surface::StepCosts::max_step`]).
    #[must_use]
    pub fn new(nodes: u32, max_step: i32) -> Reach {
        let n = crate::math::index::row(nodes);
        // A step raises a key by at most twice the dearest step (its cost,
        // and the heuristic's rise across it), so this many buckets hold every
        // key the frontier holds at once.
        // E7: rule: a step costs at least one unit (the rules refuse a free one), so a ring of three holds unit steps
        let buckets = usize::try_from(max_step.max(1))
            .map_or(3, |step| step.saturating_mul(2).saturating_add(1));
        Reach {
            epoch: 0,
            stamp: vec![0; n],
            settled: vec![0; n],
            cost: vec![0; n],
            key: vec![0; n],
            listed: vec![0; n],
            next: vec![NONE; n],
            prev: vec![NONE; n],
            heads: vec![NONE; buckets],
            sorted: Vec::with_capacity(n),
            cursor: 0,
            at: 0,
            waiting: 0,
            site_epoch: 0,
            sites: vec![0; n],
            spheres: Vec::with_capacity(SPHERE_ROOM),
            heur: vec![(0, 0); n],
            evaluations: Tally::ZERO,
            directed: false,
        }
    }

    /// The ring's length: `2 × max step + 1` buckets (the module docs).
    #[must_use]
    pub fn ring_len(&self) -> usize {
        self.heads.len()
    }

    /// Forget every mark and every sphere.
    pub fn clear_sites(&mut self) {
        if self.site_epoch == u32::MAX {
            self.sites.fill(0);
            self.site_epoch = 0;
        }
        self.site_epoch += 1;
        self.spheres.clear();
    }

    /// Mark `node` a site; `true` when it was not marked yet. A column off
    /// the map is never marked.
    pub fn mark_site(&mut self, node: Node) -> bool {
        let epoch = self.site_epoch;
        match usize::try_from(node)
            .ok()
            .and_then(|index| self.sites.get_mut(index))
        {
            Some(mark) if *mark != epoch => {
                *mark = epoch;
                true
            }
            _ => false,
        }
    }

    /// Whether `node` is marked a site.
    #[must_use]
    pub fn is_site(&self, node: Node) -> bool {
        usize::try_from(node)
            .ok()
            .and_then(|index| self.sites.get(index))
            .is_some_and(|mark| *mark == self.site_epoch)
    }

    /// Add a sphere for a directed search: its centre column and its
    /// reach, the most octile cost from the centre to any site the sphere
    /// listed. Every listed site must lie within the reach of a sphere added,
    /// or the heuristic is not admissible (the module docs, "Directed").
    ///
    /// Never refused and never dropped: a dropped sphere would leave its sites
    /// outside every reach, and the search would not be admissible. The list
    /// is reserved for `SPHERE_ROOM` spheres, one a beacon row, and a caller
    /// adds at most one a beacon row of a table the world refuses past
    /// [`crate::ids::ROOM_LIMIT`] rows, so it never grows past the
    /// reservation (`tests/allocations.rs` holds a `toward` tick to no
    /// allocation).
    pub fn add_sphere(&mut self, centre: [i32; 2], reach: i64) {
        self.spheres.push((centre, reach));
    }

    /// The spheres a directed search heads for: each one's centre column and
    /// its reach.
    #[must_use]
    pub fn spheres(&self) -> &[([i32; 2], i64)] {
        &self.spheres
    }

    /// How many `h` the last search computed: one a column it reached.
    ///
    /// # Errors
    ///
    /// [`crate::pathing::search::TallyOverflow`] once the count passed
    /// `u64::MAX`.
    pub const fn evaluations(&self) -> Result<u64, crate::pathing::search::TallyOverflow> {
        self.evaluations.read()
    }

    /// Start a search from `from`, undirected: columns settle in ascending
    /// `(cost, column)`.
    ///
    /// # Errors
    ///
    /// [`ReachError::OffMap`] for a start that is not a column of the map.
    pub fn begin(&mut self, from: Node) -> Result<(), ReachError> {
        self.directed = false;
        self.start(from, 0)
    }

    /// Start a search from `from` directed at the spheres added since
    /// [`Reach::clear_sites`]: keys are `cost + h`, `h` the octile cost to the
    /// nearest sphere's centre less that sphere's reach, floored at zero (the
    /// module docs, "Directed").
    ///
    /// # Errors
    ///
    /// [`ReachError::OffMap`] for a start that is not a column of the map.
    pub fn begin_toward(&mut self, surface: &Surface, from: Node) -> Result<(), ReachError> {
        self.directed = !self.spheres.is_empty();
        self.start(from, 0)?;
        let first = self.cached_heuristic(surface, from)?;
        self.at = first;
        let slot = self
            .key
            .get_mut(crate::math::index::row(from))
            .ok_or(ReachError::OffMap { node: from })?;
        *slot = first;
        Ok(())
    }

    fn start(&mut self, from: Node, key: i64) -> Result<(), ReachError> {
        if self.epoch == u32::MAX {
            self.stamp.fill(0);
            self.settled.fill(0);
            self.listed.fill(0);
            self.heur.fill((0, 0));
            self.epoch = 0;
        }
        self.epoch += 1;
        self.evaluations = Tally::ZERO;
        self.heads.fill(NONE);
        self.sorted.clear();
        self.cursor = 0;
        self.at = key;
        self.waiting = 0;
        let index = crate::math::index::row(from);
        let off = ReachError::OffMap { node: from };
        *self.cost.get_mut(index).ok_or(off)? = 0;
        *self.key.get_mut(index).ok_or(off)? = key;
        *self.stamp.get_mut(index).ok_or(off)? = self.epoch;
        self.sorted.push(from);
        Ok(())
    }

    /// The key the search has reached: the last column settled was settled
    /// at it, and every column still to settle has a key no lower.
    #[must_use]
    pub const fn reached(&self) -> i64 {
        self.at
    }

    /// The least cost of any walk from `node` to the search's start, once the
    /// search has settled it; `None` before.
    #[must_use]
    pub fn cost_at(&self, node: Node) -> Option<i64> {
        let index = usize::try_from(node).ok()?;
        (self.settled.get(index).copied()? == self.epoch)
            .then(|| self.cost.get(index).copied())
            .flatten()
    }

    /// `h` of `node` in this search, computed the first time the search
    /// reaches the column and read from then on.
    fn cached_heuristic(&mut self, surface: &Surface, node: Node) -> Result<i64, ReachError> {
        let epoch = self.epoch;
        let held = *self
            .heur
            .get(crate::math::index::row(node))
            .ok_or(ReachError::OffMap { node })?;
        if held.0 == epoch {
            return Ok(held.1);
        }
        let h = self.heuristic(surface, node);
        self.evaluations.bump();
        let slot = self
            .heur
            .get_mut(crate::math::index::row(node))
            .ok_or(ReachError::OffMap { node })?;
        *slot = (epoch, h);
        Ok(h)
    }

    /// The directed search's lower bound on the cost from `node` to any
    /// site, computed from the spheres (uncached): zero for an undirected
    /// search.
    #[must_use]
    pub fn heuristic(&self, surface: &Surface, node: Node) -> i64 {
        // An undirected search, and a search with no sphere to head for
        // (which `begin_toward` makes undirected), has no `h`: zero by the
        // module docs' definition, not a default.
        if !self.directed || self.spheres.is_empty() {
            return 0;
        }
        let (x, y) = surface.coord_of(node);
        let costs = surface.costs();
        // Exact: a column's and a centre's coordinates are a map's, inside
        // an `i16` each (the rules refuse a longer axis), so their difference
        // fits an `i32`; an octile cost and a reach are an `i32`'s, so their
        // difference fits an `i64`. The list is not empty (above), so the
        // fold's start is replaced by its first sphere.
        self.spheres
            .iter()
            .map(|([cx, cy], reach)| (i64::from(costs.octile(x - cx, y - cy)) - reach).max(0))
            .fold(i64::MAX, i64::min)
    }

    /// Settle the next column and lower its neighbours' costs, computing each
    /// neighbour's `h` the first time the search reaches it; each column
    /// popped is counted in `popped`. Undirected, columns settle in ascending
    /// `(cost, column)`; directed, in ascending key, in no promised order
    /// within a key (the module docs, "The queue, and its order").
    ///
    /// `Ok(None)` when no column is left to settle.
    ///
    /// # Errors
    ///
    /// [`ReachError::OffMap`] for a queued column that is not one of the
    /// map's, never read as "no more columns".
    pub fn next(
        &mut self,
        surface: &Surface,
        popped: &mut Tally,
    ) -> Result<Option<(Node, i64)>, ReachError> {
        let node = loop {
            if let Some(node) = self.sorted.get(self.cursor).copied() {
                self.cursor += 1;
                break node;
            }
            if self.waiting == 0 {
                return Ok(None);
            }
            self.advance();
        };
        let index = crate::math::index::row(node);
        let off = ReachError::OffMap { node };
        *self.settled.get_mut(index).ok_or(off)? = self.epoch;
        let here = self.cost.get(index).copied().ok_or(off)?;
        popped.bump();
        for k in 0..NEIGHBOURS.len() {
            let Some((other, step)) = surface.neighbour(node, k) else {
                continue;
            };
            if self.cost_at(other).is_some() {
                continue;
            }
            // A walk holds at most one step per column, and a column count
            // is a `u32`, so a cost of `u32::MAX` steps of at most
            // `i32::MAX` each stays below `i64::MAX`.
            let cost = here + i64::from(step);
            let key = if self.directed {
                cost + self.cached_heuristic(surface, other)?
            } else {
                cost
            };
            self.lower(other, cost, key);
        }
        Ok(Some((node, here)))
    }

    /// The bucket a key waits in.
    fn slot(&self, key: i64) -> usize {
        // E7: cannot miss: the ring's length is twice a step cost plus one, far inside `i64`
        let ring = i64::try_from(self.heads.len()).unwrap_or(i64::MAX);
        // E7: cannot miss: a remainder by the ring's length is below it
        usize::try_from(key.rem_euclid(ring)).unwrap_or(0)
    }

    /// Move to the next key a bucket holds, and sort its columns: the key
    /// reached first, which a directed search's step can add to (a step that
    /// keeps the key), then each key above it. Every waiting key lies within
    /// the ring's length above the key reached, so one turn finds it.
    fn advance(&mut self) {
        self.sorted.clear();
        self.cursor = 0;
        for turn in 0..=self.heads.len() {
            if turn > 0 {
                self.at += 1;
            }
            let slot = self.slot(self.at);
            // E7: cannot miss: a slot is a remainder by the ring's length
            let mut node = self.heads.get(slot).copied().unwrap_or(NONE);
            if node == NONE {
                continue;
            }
            if let Some(head) = self.heads.get_mut(slot) {
                *head = NONE;
            }
            while node != NONE {
                let index = crate::math::index::row(node);
                self.sorted.push(node);
                if let Some(mark) = self.listed.get_mut(index) {
                    *mark = 0;
                }
                // E7: cannot miss: a linked column is a column of the map
                node = self.next.get(index).copied().unwrap_or(NONE);
            }
            self.waiting -= self.sorted.len();
            // The undirected API's order: `(cost, column)`, which its callers
            // read (`tests/nearest.rs`'s order test). A directed search's
            // caller decides by its own key (`targeting`'s `stone_site` takes
            // the least `(cost, y, x)` itself), so its buckets settle in the
            // order they were linked.
            if !self.directed {
                self.sorted.sort_unstable();
            }
            return;
        }
    }

    /// Lower `node`'s cost to `cost` (its key to `key`) if that is lower,
    /// moving it to the bucket of its new key.
    fn lower(&mut self, node: Node, cost: i64, key: i64) {
        let Ok(index) = usize::try_from(node) else {
            return;
        };
        let reached = self.stamp.get(index).copied() == Some(self.epoch);
        if reached && self.cost.get(index).is_some_and(|held| *held <= cost) {
            return;
        }
        if self.listed.get(index).copied() == Some(self.epoch) {
            self.unlink(node);
        }
        if let Some(slot) = self.cost.get_mut(index) {
            *slot = cost;
        }
        if let Some(slot) = self.key.get_mut(index) {
            *slot = key;
        }
        if let Some(slot) = self.stamp.get_mut(index) {
            *slot = self.epoch;
        }
        let slot = self.slot(key);
        // E7: cannot miss: a slot is a remainder by the ring's length
        let head = self.heads.get(slot).copied().unwrap_or(NONE);
        if let Some(link) = self.next.get_mut(index) {
            *link = head;
        }
        if let Some(link) = self.prev.get_mut(index) {
            *link = NONE;
        }
        if head != NONE
            && let Some(link) = self.prev.get_mut(crate::math::index::row(head))
        {
            *link = node;
        }
        if let Some(first) = self.heads.get_mut(slot) {
            *first = node;
        }
        if let Some(mark) = self.listed.get_mut(index) {
            *mark = self.epoch;
        }
        self.waiting += 1;
    }

    /// Take `node` out of its bucket.
    fn unlink(&mut self, node: Node) {
        let index = crate::math::index::row(node);
        let (Some(before), Some(after), Some(key)) = (
            self.prev.get(index).copied(),
            self.next.get(index).copied(),
            self.key.get(index).copied(),
        ) else {
            return;
        };
        if before == NONE {
            let slot = self.slot(key);
            if let Some(head) = self.heads.get_mut(slot) {
                *head = after;
            }
        } else if let Some(link) = self.next.get_mut(crate::math::index::row(before)) {
            *link = after;
        }
        if after != NONE
            && let Some(link) = self.prev.get_mut(crate::math::index::row(after))
        {
            *link = before;
        }
        if let Some(mark) = self.listed.get_mut(index) {
            *mark = 0;
        }
        self.waiting -= 1;
    }
}
