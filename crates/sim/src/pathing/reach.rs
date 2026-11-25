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
use crate::reserved::Reserved;

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
    /// A dearest step the bucket ring cannot be sized for: not positive, or
    /// so dear that `2 × step + 1` buckets pass a `u32` (the rules' path-cost
    /// bound keeps every loaded table far inside both).
    Ring {
        /// The dearest step.
        max_step: i32,
    },
    /// A key whose bucket is not in the ring: a negative key, which no cost
    /// or `h` makes.
    Slot {
        /// The key.
        key: i64,
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
    /// The ring's length, `2 × max step + 1`: what a key is taken modulo.
    ring: core::num::NonZeroU32,
    /// The bucket being settled, sorted by column, and where in it.
    sorted: Reserved<Node>,
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
    spheres: Reserved<([i32; 2], i64)>,
    /// Each column's `h` in this search with the epoch it was computed in,
    /// computed once when the search first reaches the column; valid while
    /// the stamp is the search's epoch.
    heur: Vec<(u32, i64)>,
    /// How many `h` this search computed: one a column reached, each costing
    /// one octile a sphere. Counted work, never hashed.
    evaluations: Tally,
    /// The visits of the queue's own loops in this search (decisions-log
    /// item 175, dunit part 2), each counted where it happens and never
    /// hashed: a bucket the ring walk turns to ([`Reach::turns`]), a column
    /// moved from a bucket into its sorted batch ([`Reach::unlinked`]), and a
    /// cell a start's reset writes ([`Reach::filled`]).
    turns: Tally,
    unlinked: Tally,
    filled: Tally,
    /// How many times the queue moved to its next bucket, a measurement
    /// beside the turns: each move turns once to the bucket it starts on.
    advances: Tally,
    /// Whether the search in progress is directed by [`Reach::spheres`].
    directed: bool,
}

impl Reach {
    /// Room for a map of `nodes` columns whose dearest step costs
    /// `max_step` ([`crate::pathing::surface::StepCosts::max_step`]).
    ///
    /// # Errors
    ///
    /// [`ReachError::Ring`] for a dearest step that is not positive or whose
    /// ring of `2 × step + 1` buckets passes a `u32`. Neither comes from a
    /// loaded rules table: its cardinal step is non-zero by type and its
    /// dearest step is bounded by the map's path cost (`RulesTable`'s
    /// `path_cost_fits`), and a positive `i32` doubled plus one fits a `u32`.
    pub fn new(nodes: u32, max_step: i32) -> Result<Reach, ReachError> {
        let n = crate::math::index::row(nodes);
        // A step raises a key by at most twice the dearest step (its cost,
        // and the heuristic's rise across it), so this many buckets hold every
        // key the frontier holds at once.
        let ring = u32::try_from(max_step)
            .ok()
            .filter(|step| *step > 0)
            .and_then(|step| step.checked_mul(2))
            .and_then(|double| double.checked_add(1))
            .and_then(core::num::NonZeroU32::new)
            .ok_or(ReachError::Ring { max_step })?;
        let buckets = crate::math::index::row(ring.get());
        Ok(Reach {
            epoch: 0,
            stamp: vec![0; n],
            settled: vec![0; n],
            cost: vec![0; n],
            key: vec![0; n],
            listed: vec![0; n],
            next: vec![NONE; n],
            prev: vec![NONE; n],
            heads: vec![NONE; buckets],
            ring,
            sorted: Reserved::with_capacity(n),
            cursor: 0,
            at: 0,
            waiting: 0,
            site_epoch: 0,
            sites: vec![0; n],
            spheres: Reserved::with_capacity(SPHERE_ROOM),
            heur: vec![(0, 0); n],
            evaluations: Tally::ZERO,
            turns: Tally::ZERO,
            unlinked: Tally::ZERO,
            filled: Tally::ZERO,
            advances: Tally::ZERO,
            directed: false,
        })
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

    /// The buckets the last search's ring walk turned to, empty or not: each
    /// move to the next bucket reads the bucket it stands on, then each one
    /// past it until one holds a column.
    ///
    /// # Errors
    ///
    /// [`crate::pathing::search::TallyOverflow`] once the count passed
    /// `u64::MAX`.
    pub const fn turns(&self) -> Result<u64, crate::pathing::search::TallyOverflow> {
        self.turns.read()
    }

    /// The columns the last search moved from a bucket into its sorted
    /// batch: one each time a column's bucket is reached.
    ///
    /// # Errors
    ///
    /// [`crate::pathing::search::TallyOverflow`] once the count passed
    /// `u64::MAX`.
    pub const fn unlinked(&self) -> Result<u64, crate::pathing::search::TallyOverflow> {
        self.unlinked.read()
    }

    /// The cells the last search's start wrote to reset the queue: every
    /// bucket of the ring, and every column's five stamps once in each
    /// `u32::MAX` searches, when the generation wraps.
    ///
    /// # Errors
    ///
    /// [`crate::pathing::search::TallyOverflow`] once the count passed
    /// `u64::MAX`.
    pub const fn filled(&self) -> Result<u64, crate::pathing::search::TallyOverflow> {
        self.filled.read()
    }

    /// How many times the last search moved to its next bucket.
    ///
    /// # Errors
    ///
    /// [`crate::pathing::search::TallyOverflow`] once the count passed
    /// `u64::MAX`.
    pub const fn advances(&self) -> Result<u64, crate::pathing::search::TallyOverflow> {
        self.advances.read()
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
        self.turns = Tally::ZERO;
        self.unlinked = Tally::ZERO;
        self.filled = Tally::ZERO;
        self.advances = Tally::ZERO;
        if self.epoch == u32::MAX {
            self.stamp.fill(0);
            self.settled.fill(0);
            self.listed.fill(0);
            self.heur.fill((0, 0));
            self.epoch = 0;
            for cells in [
                self.stamp.len(),
                self.settled.len(),
                self.listed.len(),
                self.heur.len(),
            ] {
                self.filled.add(cells);
            }
        }
        self.epoch += 1;
        self.evaluations = Tally::ZERO;
        self.heads.fill(NONE);
        self.filled.add(self.heads.len());
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
            self.advance()?;
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
            self.lower(other, cost, key)?;
        }
        Ok(Some((node, here)))
    }

    /// The bucket a key waits in: the key modulo the ring's length.
    ///
    /// # Errors
    ///
    /// [`ReachError::Slot`] for a negative key, which no cost or `h` makes:
    /// a remainder by the ring's length is below it, a `u32`.
    fn slot(&self, key: i64) -> Result<usize, ReachError> {
        let at = key.rem_euclid(i64::from(self.ring.get()));
        u32::try_from(at)
            .map(crate::math::index::row)
            .map_err(|_| ReachError::Slot { key })
    }

    /// The head of the bucket `slot`, typed: a slot is a remainder by the
    /// ring's length, which is the list of heads' own.
    fn head(&self, slot: usize, key: i64) -> Result<u32, ReachError> {
        self.heads
            .get(slot)
            .copied()
            .ok_or(ReachError::Slot { key })
    }

    /// Move to the next key a bucket holds, and sort its columns: the key
    /// reached first, which a directed search's step can add to (a step that
    /// keeps the key), then each key above it. Every waiting key lies within
    /// the ring's length above the key reached, so one turn finds it. A
    /// bucket's columns are linked through `next`, ending at `NONE`, the
    /// empty link.
    fn advance(&mut self) -> Result<(), ReachError> {
        self.sorted.clear();
        self.cursor = 0;
        self.advances.bump();
        for turn in 0..=self.heads.len() {
            self.turns.bump();
            if turn > 0 {
                self.at += 1;
            }
            let key = self.at;
            let slot = self.slot(key)?;
            let mut node = self.head(slot, key)?;
            if node == NONE {
                continue;
            }
            *self.heads.get_mut(slot).ok_or(ReachError::Slot { key })? = NONE;
            while node != NONE {
                self.unlinked.bump();
                let index = crate::math::index::row(node);
                let off = ReachError::OffMap { node };
                self.sorted.push(node);
                *self.listed.get_mut(index).ok_or(off)? = 0;
                node = self.next.get(index).copied().ok_or(off)?;
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
            return Ok(());
        }
        Ok(())
    }

    /// Lower `node`'s cost to `cost` (its key to `key`) if that is lower,
    /// moving it to the bucket of its new key.
    fn lower(&mut self, node: Node, cost: i64, key: i64) -> Result<(), ReachError> {
        let index = crate::math::index::row(node);
        let off = ReachError::OffMap { node };
        let reached = *self.stamp.get(index).ok_or(off)? == self.epoch;
        if reached && *self.cost.get(index).ok_or(off)? <= cost {
            return Ok(());
        }
        if *self.listed.get(index).ok_or(off)? == self.epoch {
            self.unlink(node)?;
        }
        *self.cost.get_mut(index).ok_or(off)? = cost;
        *self.key.get_mut(index).ok_or(off)? = key;
        *self.stamp.get_mut(index).ok_or(off)? = self.epoch;
        let slot = self.slot(key)?;
        let head = self.head(slot, key)?;
        *self.next.get_mut(index).ok_or(off)? = head;
        *self.prev.get_mut(index).ok_or(off)? = NONE;
        if head != NONE {
            *self
                .prev
                .get_mut(crate::math::index::row(head))
                .ok_or(ReachError::OffMap { node: head })? = node;
        }
        *self.heads.get_mut(slot).ok_or(ReachError::Slot { key })? = node;
        *self.listed.get_mut(index).ok_or(off)? = self.epoch;
        self.waiting += 1;
        Ok(())
    }

    /// Take `node` out of its bucket.
    fn unlink(&mut self, node: Node) -> Result<(), ReachError> {
        let index = crate::math::index::row(node);
        let off = ReachError::OffMap { node };
        let before = *self.prev.get(index).ok_or(off)?;
        let after = *self.next.get(index).ok_or(off)?;
        let key = *self.key.get(index).ok_or(off)?;
        if before == NONE {
            let slot = self.slot(key)?;
            *self.heads.get_mut(slot).ok_or(ReachError::Slot { key })? = after;
        } else {
            *self
                .next
                .get_mut(crate::math::index::row(before))
                .ok_or(ReachError::OffMap { node: before })? = after;
        }
        if after != NONE {
            *self
                .prev
                .get_mut(crate::math::index::row(after))
                .ok_or(ReachError::OffMap { node: after })? = before;
        }
        *self.listed.get_mut(index).ok_or(off)? = 0;
        self.waiting -= 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! Dunit part 2: the queue's visit counters are what the search did. Run
    //! to exhaustion, every column linked into a bucket is moved into a batch
    //! once and popped once, so the columns moved are the pops but the start's
    //! own, which the start puts in its first batch itself; the ring
    //! walk reads the bucket it stands on at each move and one more bucket a
    //! key it climbs, so its turns are the moves plus the keys climbed, from
    //! the start's key to the last; and a start writes every bucket once.

    use super::Reach;
    use crate::pathing::search::Tally;
    use crate::pathing::surface::Node;

    fn world() -> crate::world::World {
        let path = crate::default_rules_path().unwrap_or_else(|| panic!("the rules table"));
        let rules = crate::rules::RulesTable::load(&path)
            .unwrap_or_else(|error| panic!("it loads: {error:?}"));
        crate::world::World::new(&crate::world::WorldConfig {
            match_seed: crate::DETERMINISM_MATCH_SEED,
            seats: 2,
            rules,
            match_settings: crate::runner::MatchSettings::default(),
        })
        .unwrap_or_else(|error| panic!("a world: {error:?}"))
    }

    fn count(tally: Result<u64, crate::pathing::search::TallyOverflow>) -> u64 {
        tally.unwrap_or_else(|_| panic!("a count"))
    }

    /// Search from `from` to exhaustion and hold the counters to the pops
    /// and keys: (turns, moves, columns moved, pops).
    fn exhausted(
        reach: &mut Reach,
        surface: &crate::pathing::surface::Surface,
        from: Node,
        directed: bool,
    ) -> (u64, u64, u64, u64) {
        let first = if directed {
            reach
                .begin_toward(surface, from)
                .unwrap_or_else(|error| panic!("a start: {error:?}"));
            reach.reached()
        } else {
            reach
                .begin(from)
                .unwrap_or_else(|error| panic!("a start: {error:?}"));
            0
        };
        let mut pops = Tally::ZERO;
        while reach
            .next(surface, &mut pops)
            .unwrap_or_else(|error| panic!("a step: {error:?}"))
            .is_some()
        {}
        let (turns, advances, moved, popped) = (
            count(reach.turns()),
            count(reach.advances()),
            count(reach.unlinked()),
            count(pops.read()),
        );
        let climbed = u64::try_from(reach.reached() - first).unwrap_or(u64::MAX);
        assert_eq!(
            moved + 1,
            popped,
            "every column moved into a batch is popped once, and the start, which a start puts in its batch itself"
        );
        assert_eq!(
            turns,
            advances + climbed,
            "a turn a move, and one a key climbed"
        );
        assert_eq!(
            count(reach.filled()),
            u64::try_from(reach.ring_len()).unwrap_or(0),
            "a start writes every bucket"
        );
        (turns, advances, moved, popped)
    }

    #[test]
    fn the_queues_counters_are_what_the_search_did() {
        let world = world();
        let surface = world.surface();
        let mut scratch =
            crate::pathing::search::Scratch::for_map(surface, world.clusters().size())
                .unwrap_or_else(|| panic!("a scratch for the map"));
        let reach = &mut scratch.reach;
        let [sx, sy, _] = surface.size();
        let from = surface
            .node_of(sx >> 1, sy >> 1)
            .unwrap_or_else(|| panic!("the middle column"));
        let undirected = exhausted(reach, surface, from, false);
        // Directed at a sphere in a corner, as a `toward` heads for its seat's.
        reach.clear_sites();
        reach.add_sphere([sx >> 3, sy >> 3], 0);
        let directed = exhausted(reach, surface, from, true);
        println!(
            "ring {}: undirected (turns, moves, moved, pops) {undirected:?}; directed {directed:?}",
            reach.ring_len()
        );
    }
}
