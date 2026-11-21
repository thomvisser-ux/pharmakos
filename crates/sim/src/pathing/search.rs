// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The low-level search: a hand-written binary min-heap ordered
//! `(f, h, node_id)`, and the dense generation-stamped scratch it works over.
//!
//! # The order is the contract (item 62)
//!
//! Search nodes are ordered `(f, h, node_id)`, **ascending in all three**.
//! `f = g + h`; ascending `h` among equal `f` prefers the node further along;
//! `node_id` last makes the order *total*, so no two distinct nodes ever
//! compare equal and the pop sequence is a function of the graph and nothing
//! else. `std::collections::BinaryHeap` was rejected for exactly this: it gives
//! no order among equal keys, and the order it does give depends on insertion
//! history and on the standard library's implementation — two platforms can
//! then return different, equally optimal routes, and a route is hashed state.
//!
//! The total-order key is what makes the *container* irrelevant, which is the
//! point of the decision rather than a side effect of it.
//!
//! # No hash map, no recursion, no allocation in the steady state
//!
//! The closed set and the g-scores are dense arrays indexed by node id, which
//! is both the fastest option and inherently ordered. Sparse per-query state is
//! **generation-stamped**: a `u32` epoch per query and a stamp array holding
//! the epoch a slot was last written in, so clearing is free and order-free.
//! Reconstruction walks a predecessor array into a buffer rather than
//! recursing — MSVC gives a thread about 1 MB of stack against Linux's 8, and a
//! recursive fill over 147 456 columns overflows on Windows only.
//!
//! Every buffer lives in [`Scratch`] and is reused across queries. The heaps
//! are reserved at construction for the **bounded** searches the tick runs: an
//! endpoint sweep, an intra-cluster edge and a leg refinement are each bounded
//! to one cluster, so they can push at most eight entries per cluster cell. An
//! unbounded search over the whole map is a test's ground truth and is never
//! run inside a tick.

use crate::pathing::surface::{DEFAULT_ORDER, NEIGHBOURS, Node, Surface};

/// One heap entry. `f` is `g + h`; `h` is kept so the tiebreak can use it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Item {
    pub(crate) f: i32,
    pub(crate) h: i32,
    pub(crate) n: u32,
}

impl Item {
    /// **The total order**: `(f, h, node_id)`, ascending in all three.
    pub(crate) const fn key(self) -> (i32, i32, u32) {
        (self.f, self.h, self.n)
    }
}

/// A hand-written binary min-heap over [`Item::key`].
#[derive(PartialEq, Eq, Debug, Default)]
pub(crate) struct Heap {
    items: Vec<Item>,
}

/// A clone keeps the reservation, not only the entries: a heap is reserved
/// once so a search never allocates, and a cloned or forked world's first
/// search must not either (a derived clone would reserve only the length).
impl Clone for Heap {
    fn clone(&self) -> Heap {
        let mut items = Vec::with_capacity(self.items.capacity());
        items.extend_from_slice(&self.items);
        Heap { items }
    }
}

impl Heap {
    pub(crate) fn with_capacity(capacity: usize) -> Heap {
        Heap {
            items: Vec::with_capacity(capacity),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.items.clear();
    }

    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }

    pub(crate) fn push(&mut self, item: Item) {
        self.items.push(item);
        let mut index = self.items.len().saturating_sub(1);
        while index > 0 {
            let parent = index.saturating_sub(1).div_euclid(2);
            let (here, up) = (self.key_at(index), self.key_at(parent));
            if here < up {
                self.items.swap(index, parent);
                index = parent;
            } else {
                break;
            }
        }
    }

    pub(crate) fn pop(&mut self) -> Option<Item> {
        let last = self.items.len().checked_sub(1)?;
        self.items.swap(0, last);
        let out = self.items.pop();
        let len = self.items.len();
        let mut index = 0_usize;
        loop {
            let left = index.saturating_mul(2).saturating_add(1);
            let right = left.saturating_add(1);
            let mut best = index;
            if left < len && self.key_at(left) < self.key_at(best) {
                best = left;
            }
            if right < len && self.key_at(right) < self.key_at(best) {
                best = right;
            }
            if best == index {
                break;
            }
            self.items.swap(index, best);
            index = best;
        }
        out
    }

    /// The key of the entry at `index`, or the largest possible key when the
    /// index is off the end — which the loops above never ask for, and which
    /// keeps every comparison total without an index panic.
    fn key_at(&self, index: usize) -> (i32, i32, u32) {
        self.items
            .get(index)
            // E7: rule: an index past the heap sorts after every item in it
            .map_or((i32::MAX, i32::MAX, u32::MAX), |item| item.key())
    }
}

/// Which nodes a search may touch.
///
/// Bounded searches are what keeps an intra-cluster edge cheap and a repair
/// local, and they are the only kind a tick runs.
#[derive(Clone, Copy, Debug)]
pub enum Bound<'a> {
    /// The whole map. Ground-truth searches only, never inside a tick.
    Whole,
    /// One cluster: the per-node cluster map, and the cluster to stay in.
    Cluster(&'a [u16], u16),
}

impl Bound<'_> {
    fn admits(self, node: Node) -> bool {
        match self {
            Bound::Whole => true,
            Bound::Cluster(of_node, cluster) => usize::try_from(node)
                .ok()
                .and_then(|index| of_node.get(index).copied())
                .is_some_and(|found| found == cluster),
        }
    }
}

/// Every buffer a query needs, owned once and reused.
///
/// Public because [`crate::pathing::estimate::estimate`] takes it — item 61's
/// signature is `(world, clusters, scratch, start, goal, fog)` — but its
/// contents are the module's own: a caller hands one in and reads the answer
/// out of the return value.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Scratch {
    // --- the low-level half, indexed by column ---
    epoch: u32,
    stamp: Vec<u32>,
    closed: Vec<u32>,
    g: Vec<i32>,
    came: Vec<u32>,
    heap: Heap,

    // --- the abstract half, indexed by abstract node ---
    pub(crate) abstract_epoch: u32,
    pub(crate) abstract_stamp: Vec<u32>,
    pub(crate) abstract_closed: Vec<u32>,
    pub(crate) abstract_g: Vec<i32>,
    pub(crate) abstract_came: Vec<u32>,
    pub(crate) abstract_heap: Heap,
    pub(crate) start_edges: Vec<(u32, i32)>,
    pub(crate) goal_edges: Vec<(u32, i32)>,
    pub(crate) abstract_path: Vec<u32>,

    // --- route assembly ---
    pub(crate) raw: Vec<u32>,
    pub(crate) leg: Vec<u32>,
    pub(crate) line: Vec<u32>,
    pub(crate) prefix: Vec<i32>,
    /// The route a near pair's estimate prices
    /// ([`crate::pathing::estimate`]'s "Near pairs"): the router's own route,
    /// written here rather than into a caller's buffer so that an estimate,
    /// inside a tick or out of one, allocates nothing.
    pub(crate) served: Vec<u32>,
    /// The grid optimum's search ([`crate::pathing::reach`]): what a `toward`
    /// step's sites are bounded by.
    pub(crate) reach: crate::pathing::reach::Reach,

    // --- counters, diagnostic only ---
    pub(crate) expansions: Tally,
    pub(crate) endpoint_expansions: Tally,
    pub(crate) abstract_expansions: Tally,
    pub(crate) near_queries: Tally,
    pub(crate) walks: Tally,
    pub(crate) stalled: Tally,
}

/// A diagnostic count that never reads wrong: it counts up by a checked add,
/// and a count that would pass `u64::MAX` becomes [`Tally::Overflowed`], which
/// its reader is told as [`TallyOverflow`] rather than as a number. Neither
/// wraps nor saturates (lane `p3fix`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tally {
    /// The events counted since the last reset.
    Count(u64),
    /// More than `u64::MAX` events since the last reset.
    Overflowed,
}

impl Tally {
    /// A count of nothing.
    pub const ZERO: Tally = Tally::Count(0);

    /// Count one more event.
    pub const fn bump(&mut self) {
        *self = match *self {
            Tally::Count(count) => match count.checked_add(1) {
                Some(next) => Tally::Count(next),
                None => Tally::Overflowed,
            },
            Tally::Overflowed => Tally::Overflowed,
        };
    }

    /// The count, or that it overflowed.
    ///
    /// # Errors
    ///
    /// [`TallyOverflow`] once more than `u64::MAX` events were counted.
    pub const fn read(self) -> Result<u64, TallyOverflow> {
        match self {
            Tally::Count(count) => Ok(count),
            Tally::Overflowed => Err(TallyOverflow),
        }
    }
}

/// A diagnostic count passed `u64::MAX` since its last reset.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TallyOverflow;

impl core::fmt::Display for TallyOverflow {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("a diagnostic count passed u64::MAX since its last reset")
    }
}

impl std::error::Error for TallyOverflow {}

impl Scratch {
    /// Room for a graph of `nodes` columns and `abstract_nodes` abstract nodes.
    ///
    /// `cluster_cells` sizes the heaps: every search a tick runs is bounded to
    /// one cluster, and a bounded search pushes at most one entry per examined
    /// directed edge, which is eight per cell. `max_step`, the dearest step
    /// a walker takes, sizes the grid optimum's bucket ring
    /// ([`crate::pathing::reach`]).
    #[must_use]
    pub fn new(nodes: u32, abstract_nodes: u32, cluster_cells: u32, max_step: i32) -> Scratch {
        let n = crate::math::index::row(nodes);
        let a = crate::math::index::row(abstract_nodes).saturating_add(2);
        let low_heap = crate::math::index::row(cluster_cells.saturating_mul(8));
        Scratch {
            epoch: 0,
            stamp: vec![0; n],
            closed: vec![0; n],
            g: vec![0; n],
            came: vec![0; n],
            heap: Heap::with_capacity(low_heap),
            abstract_epoch: 0,
            abstract_stamp: vec![0; a],
            abstract_closed: vec![0; a],
            abstract_g: vec![0; a],
            abstract_came: vec![0; a],
            // Four entries per abstract node: measured graphs carry about ten
            // directed edges a node and a query touches a fraction of them.
            abstract_heap: Heap::with_capacity(a.saturating_mul(4)),
            start_edges: Vec::with_capacity(a.min(1024)),
            goal_edges: Vec::with_capacity(a.min(1024)),
            // A reconstructed abstract path visits each abstract node at most
            // once and the two temporary endpoint nodes besides, and the
            // reconstruction's own guard is that same `a`, so this is a bound
            // and not a measurement — unlike `abstract_heap` above, which is a
            // measured capacity and says so.
            abstract_path: Vec::with_capacity(a.saturating_add(2)),
            raw: Vec::with_capacity(crate::math::index::row(crate::pathing::MAX_ROUTE_NODES)),
            leg: Vec::with_capacity(crate::math::index::row(cluster_cells)),
            line: Vec::with_capacity(crate::math::index::row(cluster_cells)),
            prefix: Vec::with_capacity(
                crate::math::index::row(crate::pathing::MAX_ROUTE_NODES).saturating_add(1),
            ),
            // A smoothed route holds no more nodes than the raw walk it
            // smooths (each shortcut is a straight walk no longer than the
            // stretch it replaces), and the raw walk stops at
            // `MAX_ROUTE_NODES`: the same bound as `raw`.
            served: Vec::with_capacity(crate::math::index::row(crate::pathing::MAX_ROUTE_NODES)),
            reach: crate::pathing::reach::Reach::new(nodes, max_step),
            expansions: Tally::ZERO,
            endpoint_expansions: Tally::ZERO,
            abstract_expansions: Tally::ZERO,
            near_queries: Tally::ZERO,
            walks: Tally::ZERO,
            stalled: Tally::ZERO,
        }
    }

    /// The scratch a decomposition of `surface` at a cluster edge of
    /// `cluster_voxels` needs.
    ///
    /// The abstract numbering is `cluster * max_trans + slot`, so its span is
    /// arithmetic the caller can do before the decomposition exists — which it
    /// has to, because building the decomposition's intra edges is itself a
    /// search and needs this scratch. [`Clusters::new`] is checked against it.
    ///
    /// `None` when the cluster edge cannot decompose the map.
    ///
    /// [`Clusters::new`]: crate::pathing::clusters::Clusters::new
    #[must_use]
    pub fn for_map(surface: &Surface, cluster_voxels: i32) -> Option<Scratch> {
        let capacity = Scratch::abstract_capacity_for(surface, cluster_voxels)?;
        let cells = u32::try_from(cluster_voxels.checked_mul(cluster_voxels)?).ok()?;
        Some(Scratch::new(
            surface.node_count(),
            capacity,
            cells,
            surface.costs().max_step(),
        ))
    }

    /// How many abstract node slots a decomposition of `surface` at this
    /// cluster edge spans: `clusters * 4 * cluster_edge`, the bound
    /// [`crate::pathing::clusters`]'s module docs derive — four borders of
    /// `cluster_edge` cells each, and a border of `n` cells can hold `n`
    /// maximal runs rather than `n / 2`.
    ///
    /// `None` when the cluster edge cannot decompose the map.
    #[must_use]
    pub fn abstract_capacity_for(surface: &Surface, cluster_voxels: i32) -> Option<u32> {
        let extent = surface.size();
        let sx = extent.first().copied()?;
        let sy = extent.get(1).copied()?;
        if cluster_voxels <= 0 {
            return None;
        }
        if sx.checked_rem(cluster_voxels)? != 0 || sy.checked_rem(cluster_voxels)? != 0 {
            return None;
        }
        let nx = sx.checked_div(cluster_voxels)?;
        let ny = sy.checked_div(cluster_voxels)?;
        let per_border = cluster_voxels;
        u32::try_from(
            nx.checked_mul(ny)?
                .checked_mul(4)?
                .checked_mul(per_border)?,
        )
        .ok()
    }

    /// Nodes popped by the low-level searches since the last reset.
    ///
    /// # Errors
    ///
    /// [`TallyOverflow`] once the count passed `u64::MAX`.
    pub const fn expansions(&self) -> Result<u64, TallyOverflow> {
        self.expansions.read()
    }

    /// Nodes popped by the endpoint-insertion sweeps since the last reset.
    ///
    /// The number spike G2 §9.11 says S3 needs and nobody else produces: in the
    /// spike, at cluster 32, the insertion sweeps were 91 % of a median query,
    /// which is why the lever on the estimate budget is the sweep and not a
    /// second abstract level (item 58).
    ///
    /// # Errors
    ///
    /// [`TallyOverflow`] once the count passed `u64::MAX`.
    pub const fn endpoint_expansions(&self) -> Result<u64, TallyOverflow> {
        self.endpoint_expansions.read()
    }

    /// Abstract nodes popped since the last reset.
    ///
    /// # Errors
    ///
    /// [`TallyOverflow`] once the count passed `u64::MAX`.
    pub const fn abstract_expansions(&self) -> Result<u64, TallyOverflow> {
        self.abstract_expansions.read()
    }

    /// Estimates priced at the served route since the last reset (a near
    /// pair, [`crate::pathing::estimate`]). Diagnostic: it is how a test
    /// knows a tick or a draw exercised the near branch at all.
    ///
    /// # Errors
    ///
    /// [`TallyOverflow`] once the count passed `u64::MAX`.
    pub const fn near_queries(&self) -> Result<u64, TallyOverflow> {
        self.near_queries.read()
    }

    /// The grid optimum's search as the last `toward` resolve left it: its
    /// spheres and their reach, and its counted `h` evaluations
    /// ([`crate::pathing::reach`]).
    #[must_use]
    pub const fn reach(&self) -> &crate::pathing::reach::Reach {
        &self.reach
    }

    /// Walks priced at the router's served routes since the last reset
    /// ([`crate::pathing::estimate::walk_cost`], what "nearest" ranks by).
    /// Diagnostic: it is how a test knows a tick ranked by the served walk.
    ///
    /// # Errors
    ///
    /// [`TallyOverflow`] once the count passed `u64::MAX`.
    pub const fn walks(&self) -> Result<u64, TallyOverflow> {
        self.walks.read()
    }

    /// Walks a ranker met whose chain of partial routes stalled
    /// ([`crate::pathing::estimate::WalkError::Stalled`]) since the last
    /// reset: candidates dropped as walks no walker finishes, counted here so
    /// the drop is read, never silent. The chain's own argument says none
    /// occurs; `tests/nearest.rs` holds it at zero.
    ///
    /// # Errors
    ///
    /// [`TallyOverflow`] once the count passed `u64::MAX`.
    pub const fn stalled(&self) -> Result<u64, TallyOverflow> {
        self.stalled.read()
    }

    /// Zero the six counters.
    pub const fn reset_counters(&mut self) {
        self.expansions = Tally::ZERO;
        self.endpoint_expansions = Tally::ZERO;
        self.abstract_expansions = Tally::ZERO;
        self.near_queries = Tally::ZERO;
        self.walks = Tally::ZERO;
        self.stalled = Tally::ZERO;
    }

    /// The largest the low-level heap has grown. Diagnostic: it is what the
    /// construction-time reservation has to cover for a tick to allocate
    /// nothing.
    #[must_use]
    pub fn low_heap_len(&self) -> usize {
        self.heap.len()
    }

    /// Start a low-level query. `O(1)`: the epoch invalidates every slot at
    /// once, and the wrap branch is the only linear clear — once every 4.2
    /// billion queries.
    fn begin(&mut self) {
        if self.epoch == u32::MAX {
            self.stamp.fill(0);
            self.closed.fill(0);
            self.epoch = 0;
        }
        self.epoch = self.epoch.saturating_add(1);
        self.heap.clear();
    }

    fn seen(&self, node: Node) -> bool {
        usize::try_from(node)
            .ok()
            .and_then(|index| self.stamp.get(index).copied())
            .is_some_and(|stamp| stamp == self.epoch)
    }

    fn is_closed(&self, node: Node) -> bool {
        usize::try_from(node)
            .ok()
            .and_then(|index| self.closed.get(index).copied())
            .is_some_and(|stamp| stamp == self.epoch)
    }

    fn close(&mut self, node: Node) {
        if let Ok(index) = usize::try_from(node)
            && let Some(slot) = self.closed.get_mut(index)
        {
            *slot = self.epoch;
        }
    }

    fn set_reached(&mut self, node: Node, cost: i32, from: Node) {
        let Ok(index) = usize::try_from(node) else {
            return;
        };
        if let Some(slot) = self.stamp.get_mut(index) {
            *slot = self.epoch;
        }
        if let Some(slot) = self.g.get_mut(index) {
            *slot = cost;
        }
        if let Some(slot) = self.came.get_mut(index) {
            *slot = from;
        }
    }

    fn cost_at(&self, node: Node) -> i32 {
        usize::try_from(node)
            .ok()
            .and_then(|index| self.g.get(index).copied())
            // E7: rule: a node the search never reached costs nothing yet
            .unwrap_or(0)
    }

    /// The cost to `node` in the query that just ran, if it was reached.
    ///
    /// This is how an endpoint is inserted into the abstract graph: one bounded
    /// sweep of its cluster gives the cost to every transition node of that
    /// cluster at once.
    #[must_use]
    pub fn cost_of(&self, node: Node) -> Option<i32> {
        if self.seen(node) {
            Some(self.cost_at(node))
        } else {
            None
        }
    }

    /// Walk the predecessor array from `goal` back to `start`, into `out`.
    fn reconstruct(&self, start: Node, goal: Node, out: &mut Vec<Node>) {
        out.clear();
        let mut current = goal;
        let mut guard: u32 = 0;
        // E7: cannot miss: one entry per node of a map, whose node ids are `u32`s
        let limit = self.came.len().try_into().unwrap_or(u32::MAX);
        loop {
            out.push(current);
            if current == start || guard >= limit {
                break;
            }
            let next = usize::try_from(current)
                .ok()
                .and_then(|index| self.came.get(index).copied())
                // E7: rule: a node the search never reached ends the walk back at the start
                .unwrap_or(start);
            current = next;
            guard = guard.saturating_add(1);
        }
        out.reverse();
    }
}

/// A\* from `start` to `goal` inside `bound`, expanding neighbours in `order`.
///
/// Returns the route's cost and fills `out` with the column sequence.
///
/// `order` exists for one reason: `tests/pathing.rs` permutes it and demands a
/// byte-identical route, which is what makes item 62's total order a checked
/// property rather than a claim.
pub fn astar_with_order(
    surface: &Surface,
    bound: Bound<'_>,
    scratch: &mut Scratch,
    start: Node,
    goal: Node,
    order: [u8; 8],
    out: &mut Vec<Node>,
) -> Option<i32> {
    astar_inner(
        surface,
        bound,
        scratch,
        (start, goal),
        order,
        crate::pathing::route::Surcharge::NONE,
        out,
    )
}

/// A\* with the default expansion order under a route surcharge (S2's
/// `atk`): a step onto a surcharged column costs the multiplier times its
/// ordinary cost. The heuristic stays octile at the ordinary costs, which a
/// surcharge of one or more never makes inadmissible.
pub fn astar_costed(
    surface: &Surface,
    bound: Bound<'_>,
    scratch: &mut Scratch,
    (start, goal): (Node, Node),
    surcharge: crate::pathing::route::Surcharge<'_>,
    out: &mut Vec<Node>,
) -> Option<i32> {
    astar_inner(
        surface,
        bound,
        scratch,
        (start, goal),
        DEFAULT_ORDER,
        surcharge,
        out,
    )
}

fn astar_inner(
    surface: &Surface,
    bound: Bound<'_>,
    scratch: &mut Scratch,
    (start, goal): (Node, Node),
    order: [u8; 8],
    surcharge: crate::pathing::route::Surcharge<'_>,
    out: &mut Vec<Node>,
) -> Option<i32> {
    out.clear();
    if !surface.walkable(start) || !surface.walkable(goal) {
        return None;
    }
    if !bound.admits(start) || !bound.admits(goal) {
        return None;
    }
    scratch.begin();
    let h0 = surface.heuristic(start, goal);
    scratch.set_reached(start, 0, start);
    scratch.heap.push(Item {
        f: h0,
        h: h0,
        n: start,
    });

    while let Some(item) = scratch.heap.pop() {
        if scratch.is_closed(item.n) {
            continue;
        }
        scratch.close(item.n);
        scratch.expansions.bump();
        if item.n == goal {
            let cost = scratch.cost_at(goal);
            scratch.reconstruct(start, goal, out);
            return Some(cost);
        }
        let here = scratch.cost_at(item.n);
        for k in order {
            let Some((next, step)) = surface.neighbour(item.n, usize::from(k)) else {
                continue;
            };
            if !bound.admits(next) || scratch.is_closed(next) {
                continue;
            }
            let Some(cost) = here.checked_add(surcharge.step(surface, next, step)) else {
                continue;
            };
            if scratch.seen(next) && scratch.cost_at(next) <= cost {
                continue;
            }
            scratch.set_reached(next, cost, item.n);
            let h = surface.heuristic(next, goal);
            scratch.heap.push(Item {
                f: cost.saturating_add(h),
                h,
                n: next,
            });
        }
    }
    None
}

/// A\* with the default expansion order.
pub fn astar(
    surface: &Surface,
    bound: Bound<'_>,
    scratch: &mut Scratch,
    start: Node,
    goal: Node,
    out: &mut Vec<Node>,
) -> Option<i32> {
    astar_with_order(surface, bound, scratch, start, goal, DEFAULT_ORDER, out)
}

/// Dijkstra from `source`, bounded, exhausting everything reachable inside the
/// bound. The costs stay in `scratch` and are read back with
/// [`Scratch::cost_of`].
///
/// `endpoint` says whether the sweep's expansions are charged to
/// [`Scratch::endpoint_expansions`] as well as to the general counter, so the
/// endpoint-sweep share of a query is a measurement rather than an estimate.
pub fn dijkstra_bounded(
    surface: &Surface,
    bound: Bound<'_>,
    scratch: &mut Scratch,
    source: Node,
    endpoint: bool,
) {
    scratch.begin();
    if !surface.walkable(source) || !bound.admits(source) {
        return;
    }
    scratch.set_reached(source, 0, source);
    scratch.heap.push(Item {
        f: 0,
        h: 0,
        n: source,
    });
    while let Some(item) = scratch.heap.pop() {
        if scratch.is_closed(item.n) {
            continue;
        }
        scratch.close(item.n);
        scratch.expansions.bump();
        if endpoint {
            scratch.endpoint_expansions.bump();
        }
        let here = scratch.cost_at(item.n);
        for k in DEFAULT_ORDER {
            let Some((next, step)) = surface.neighbour(item.n, usize::from(k)) else {
                continue;
            };
            if !bound.admits(next) || scratch.is_closed(next) {
                continue;
            }
            let Some(cost) = here.checked_add(step) else {
                continue;
            };
            if scratch.seen(next) && scratch.cost_at(next) <= cost {
                continue;
            }
            scratch.set_reached(next, cost, item.n);
            scratch.heap.push(Item {
                f: cost,
                h: 0,
                n: next,
            });
        }
    }
}

/// The total cost of walking `route`, or `None` when a step on it is illegal —
/// which is what a stale route looks like after the terrain moved.
#[must_use]
pub fn route_cost(surface: &Surface, route: &[Node]) -> Option<i32> {
    let mut total: i32 = 0;
    for pair in route.windows(2) {
        let from = pair.first().copied()?;
        let to = pair.get(1).copied()?;
        total = total.checked_add(surface.step_cost_between(from, to)?)?;
    }
    Some(total)
}

/// How many neighbour offsets there are. Kept beside the search so a caller
/// permuting the order cannot disagree with it.
#[must_use]
pub const fn neighbour_count() -> usize {
    NEIGHBOURS.len()
}

#[cfg(test)]
mod tests {
    use super::{Scratch, Tally, TallyOverflow};
    use crate::math::index::row;
    use crate::pathing::MAX_ROUTE_NODES;

    /// The served route's buffer is reserved where the scratch is made, for
    /// the longest route the router writes, whatever the map: a near
    /// estimate never grows it (lane `p3fix`).
    #[test]
    fn the_served_buffer_holds_the_longest_route_from_the_start() {
        for (nodes, abstract_nodes, cells) in [(1, 1, 1), (64, 16, 16), (65_536, 8_192, 1_024)] {
            let scratch = Scratch::new(nodes, abstract_nodes, cells, 18);
            assert!(
                scratch.served.capacity() >= row(MAX_ROUTE_NODES),
                "a scratch for {nodes} columns reserves {} served nodes, under {MAX_ROUTE_NODES}",
                scratch.served.capacity()
            );
        }
    }

    /// A tally counts by one, and past `u64::MAX` it says it overflowed
    /// rather than wrap or stick at the top.
    #[test]
    fn a_tally_counts_and_reports_its_overflow() {
        let mut tally = Tally::ZERO;
        tally.bump();
        tally.bump();
        assert_eq!(tally.read(), Ok(2));
        let mut top = Tally::Count(u64::MAX - 1);
        top.bump();
        assert_eq!(top.read(), Ok(u64::MAX));
        top.bump();
        assert_eq!(top.read(), Err(TallyOverflow));
        top.bump();
        assert_eq!(
            top.read(),
            Err(TallyOverflow),
            "an overflowed tally stays overflowed"
        );
    }
}
