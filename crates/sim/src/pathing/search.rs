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
use crate::reserved::Reserved;

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

/// A hand-written binary min-heap over [`Item::key`]. Its items are
/// [`Reserved`]: a heap is reserved once so a search never allocates, and a
/// cloned or forked world's first search must not either.
///
/// **A sift is not counted work** ([`Scratch::visits`]), where the grid
/// optimum's bucket queue counts its own loops
/// ([`crate::pathing::reach::Reach`]). The two differ in what bounds them. A
/// sift's steps are bounded by what is already counted: a push follows one
/// counted neighbour slot or abstract edge, a pop is one counted pop or stale
/// entry, and each sifts at most the heap's depth, the base-two logarithm of
/// its length. The bucket queue's ring walk and unlinking are bounded by the
/// ring's length and a bucket's, which no pop or slot counts, so they are
/// counted themselves. A sort's compares are left uncounted for the same
/// reason ([`crate::seams::UnitTally`]).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub(crate) struct Heap {
    items: Reserved<Item>,
}

impl Heap {
    pub(crate) fn with_capacity(capacity: usize) -> Heap {
        Heap {
            items: Reserved::with_capacity(capacity),
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
    /// How many columns a cluster holds in the decomposition this was sized
    /// for ([`Scratch::new`]'s `cluster_cells`): the room the cluster-bounded
    /// buffers (`heap`, `leg`, `line`) were reserved from, kept so that
    /// [`Scratch::serves`] can answer for them as it does for the indexed
    /// ones.
    cluster_cells: u32,
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
    pub(crate) start_edges: Reserved<(u32, i32)>,
    pub(crate) goal_edges: Reserved<(u32, i32)>,
    pub(crate) abstract_path: Reserved<u32>,

    // --- route assembly ---
    pub(crate) raw: Reserved<u32>,
    pub(crate) leg: Reserved<u32>,
    pub(crate) line: Reserved<u32>,
    pub(crate) prefix: Reserved<i32>,
    /// The route a served walk prices, for a clear estimate and for every
    /// "nearest" ([`crate::pathing::estimate`]'s "A clear query"): the
    /// router's own route,
    /// written here rather than into a caller's buffer so that an estimate,
    /// inside a tick or out of one, allocates nothing.
    pub(crate) served: Reserved<u32>,
    /// The grid optimum's search ([`crate::pathing::reach`]): what a `toward`
    /// step's sites are bounded by.
    pub(crate) reach: crate::pathing::reach::Reach,

    // --- counters, diagnostic only ---
    pub(crate) expansions: Tally,
    pub(crate) endpoint_expansions: Tally,
    pub(crate) abstract_expansions: Tally,
    /// The visits of a query's loops that pop nothing ([`Scratch::visits`]).
    pub(crate) visits: Tally,
    pub(crate) served_estimates: Tally,
    pub(crate) walks: Tally,
    pub(crate) stalled: Tally,
    pub(crate) reach_faults: Tally,
    pub(crate) survey_unserved: Tally,
    pub(crate) survey_out_of_reach: Tally,
    pub(crate) survey_scout_lost: Tally,
    pub(crate) survey_crowded_out: Tally,
    pub(crate) survey_under_threat: Tally,
    /// Needed Survey sightings past the most due a decision weighs
    /// (`crate::survey::Drop::Unweighed`), each once a Push.
    pub(crate) survey_unweighed: Tally,
    pub(crate) survey_drops_unlogged: Tally,
    /// What naming a Survey drop's reason remembers of the running Push, and
    /// its drop log ([`crate::survey::Memory`]).
    pub(crate) survey: crate::survey::Memory,
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

    /// Count `events` more, all at once (a fill of that many cells).
    pub(crate) fn add(&mut self, events: usize) {
        *self = match (*self, u64::try_from(events)) {
            (Tally::Count(count), Ok(events)) => match count.checked_add(events) {
                Some(next) => Tally::Count(next),
                None => Tally::Overflowed,
            },
            (Tally::Count(_), Err(_)) | (Tally::Overflowed, _) => Tally::Overflowed,
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

/// A search was handed a scratch that does not serve its graph
/// ([`Scratch::serves`], or [`Scratch::serves_surface`] for a search over the
/// columns alone): one sized for another map or another cluster edge.
///
/// **Every public function that searches in a scratch refuses it before it
/// searches**, each in its own result. Over the columns alone
/// ([`Scratch::serves_surface`]): [`astar`], [`astar_with_order`],
/// [`astar_costed`] and [`dijkstra_bounded`]. Over the decomposition
/// ([`Scratch::serves`]):
/// [`abstract_search`](crate::pathing::route::abstract_search);
/// [`route`](crate::pathing::route::route),
/// [`route_with`](crate::pathing::route::route_with) and
/// [`walk_cost`](crate::pathing::estimate::walk_cost), as
/// [`Unanswered::Misfit`]; [`estimate`](crate::pathing::estimate::estimate),
/// as [`EstimateError::Misfit`](crate::pathing::estimate::EstimateError::Misfit);
/// [`repair`](crate::pathing::repair::repair) and the decomposition's
/// rebuilds; the router's [`serve`](crate::pathing::router::Router::serve);
/// and the resolvers ([`crate::targeting`]'s `cover`, `on_vent`, `toward` and
/// its ranker), as [`Unanswered::Misfit`]. A refused call changes nothing and
/// counts nothing.
///
/// The world searches in its own scratch through the crate's unchecked
/// twins of these, and holds that scratch to its graph as each tick opens,
/// before each of its phases and before a restore
/// ([`crate::world::TickError::MisfitScratch`],
/// [`crate::snapshot::SnapshotError::MisfitScratch`]).
/// [`Clusters::new`](crate::pathing::clusters::Clusters::new) refuses as
/// `None`.
///
/// A search in a scratch that does not serve would end (a node with no slot
/// is never queued, [`Scratch::queue`]) and answer "no route" from slots it
/// does not have, which reads as an answer and is none.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Misfit;

impl core::fmt::Display for Misfit {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(
            "the search scratch was sized for another map or cluster edge than this graph's",
        )
    }
}

impl std::error::Error for Misfit {}

/// Why a public search that has reasons of its own gave no result: it never
/// ran, because the scratch does not serve the graph ([`Misfit`]); or it ran,
/// and gave its own reason `E`. Two cases and never one, so that a reason
/// about the ground (no route, a step that finds no target) is always a
/// statement about the ground, and a caller's wrong scratch is never read as
/// one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unanswered<E> {
    /// The scratch handed in does not serve the graph
    /// ([`Scratch::serves`]): nothing was searched.
    Misfit,
    /// The search ran, and this is its reason.
    Ran(E),
}

impl<E> Unanswered<E> {
    /// The same refusal with the search's reason mapped.
    pub fn map<F>(self, with: impl FnOnce(E) -> F) -> Unanswered<F> {
        match self {
            Unanswered::Misfit => Unanswered::Misfit,
            Unanswered::Ran(reason) => Unanswered::Ran(with(reason)),
        }
    }
}

impl<E> From<Misfit> for Unanswered<E> {
    fn from(_: Misfit) -> Unanswered<E> {
        Unanswered::Misfit
    }
}

impl<E: core::fmt::Display> core::fmt::Display for Unanswered<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Unanswered::Misfit => Misfit.fmt(f),
            Unanswered::Ran(reason) => reason.fmt(f),
        }
    }
}

impl Scratch {
    /// Room for a graph of `nodes` columns and `abstract_nodes` abstract nodes.
    ///
    /// `cluster_cells` sizes the heaps: every search a tick runs is bounded to
    /// one cluster, and a bounded search pushes at most one entry per examined
    /// directed edge, which is eight per cell. `max_step`, the dearest step
    /// a walker takes, sizes the grid optimum's bucket ring
    /// ([`crate::pathing::reach`]). `None` when the ring cannot be sized for
    /// that step ([`crate::pathing::reach::ReachError::Ring`]), which no
    /// loaded rules table's dearest step makes.
    #[must_use]
    pub fn new(
        nodes: u32,
        abstract_nodes: u32,
        cluster_cells: u32,
        max_step: i32,
    ) -> Option<Scratch> {
        let reach = crate::pathing::reach::Reach::new(nodes, max_step).ok()?;
        let n = crate::math::index::row(nodes);
        let a = crate::math::index::row(abstract_nodes).saturating_add(2);
        let low_heap = crate::math::index::row(cluster_cells.saturating_mul(8));
        Some(Scratch {
            cluster_cells,
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
            start_edges: Reserved::with_capacity(a.min(1024)),
            goal_edges: Reserved::with_capacity(a.min(1024)),
            // A reconstructed abstract path visits each abstract node at most
            // once and the two temporary endpoint nodes besides, and the
            // reconstruction's own guard is that same `a`, so this is a bound
            // and not a measurement — unlike `abstract_heap` above, which is a
            // measured capacity and says so.
            abstract_path: Reserved::with_capacity(a.saturating_add(2)),
            raw: Reserved::with_capacity(crate::math::index::row(crate::pathing::MAX_ROUTE_NODES)),
            leg: Reserved::with_capacity(crate::math::index::row(cluster_cells)),
            line: Reserved::with_capacity(crate::math::index::row(cluster_cells)),
            prefix: Reserved::with_capacity(
                crate::math::index::row(crate::pathing::MAX_ROUTE_NODES).saturating_add(1),
            ),
            // A smoothed route holds no more nodes than the raw walk it
            // smooths (each shortcut is a straight walk no longer than the
            // stretch it replaces), and the raw walk stops at
            // `MAX_ROUTE_NODES`: the same bound as `raw`.
            served: Reserved::with_capacity(crate::math::index::row(
                crate::pathing::MAX_ROUTE_NODES,
            )),
            reach,
            expansions: Tally::ZERO,
            endpoint_expansions: Tally::ZERO,
            abstract_expansions: Tally::ZERO,
            visits: Tally::ZERO,
            served_estimates: Tally::ZERO,
            walks: Tally::ZERO,
            stalled: Tally::ZERO,
            reach_faults: Tally::ZERO,
            survey_unserved: Tally::ZERO,
            survey_out_of_reach: Tally::ZERO,
            survey_scout_lost: Tally::ZERO,
            survey_crowded_out: Tally::ZERO,
            survey_under_threat: Tally::ZERO,
            survey_unweighed: Tally::ZERO,
            survey_drops_unlogged: Tally::ZERO,
            survey: crate::survey::Memory::new(),
        })
    }

    /// The scratch a decomposition of `surface` at a cluster edge of
    /// `cluster_voxels` needs.
    ///
    /// The abstract numbering is `cluster * max_trans + slot`, so its span is
    /// arithmetic the caller can do before the decomposition exists — which it
    /// has to, because building the decomposition's intra edges is itself a
    /// search and needs this scratch. [`Clusters::new`] is checked against it.
    ///
    /// `None` when the cluster edge cannot decompose the map, or the
    /// grid optimum's ring cannot be sized for the map's dearest step
    /// ([`Scratch::new`]).
    ///
    /// [`Clusters::new`]: crate::pathing::clusters::Clusters::new
    #[must_use]
    pub fn for_map(surface: &Surface, cluster_voxels: i32) -> Option<Scratch> {
        let capacity = Scratch::abstract_capacity_for(surface, cluster_voxels)?;
        let cells = u32::try_from(cluster_voxels.checked_mul(cluster_voxels)?).ok()?;
        Scratch::new(
            surface.node_count(),
            capacity,
            cells,
            surface.costs().max_step(),
        )
    }

    /// Whether this scratch has the room every search over `surface` and its
    /// decomposition `clusters` writes: a slot for every column, a slot for
    /// every abstract node the decomposition numbers and the two temporary
    /// endpoints a query adds, the room a search bounded to one cluster was
    /// reserved for (so that a search over it grows no buffer), and a grid
    /// optimum sized for the surface's columns and its dearest step:
    /// [`Scratch::new`]'s four inputs, each at least what this graph asks.
    ///
    /// **Every search's precondition, and every public search holds its
    /// scratch to it** ([`Misfit`] lists them). [`Scratch::for_map`] at the
    /// decomposition's own cluster edge serves it, and
    /// [`Clusters::new`](crate::pathing::clusters::Clusters::new) refuses a
    /// scratch that does not. A reader that keeps a scratch of its own beside
    /// a graph it borrows (the gateway's planning scratch) asks this before
    /// it lends one, so that its refusal is its own; a search it called
    /// anyway would refuse too.
    #[must_use]
    pub fn serves(&self, surface: &Surface, clusters: &crate::pathing::clusters::Clusters) -> bool {
        let slots = crate::math::index::row(clusters.abstract_capacity()).saturating_add(2);
        crate::math::index::row(self.cluster_cells) >= clusters.cluster_cells()
            && self.serves_surface(surface)
            && self.abstract_stamp.len() >= slots
            && self.abstract_closed.len() >= slots
            && self.abstract_g.len() >= slots
            && self.abstract_came.len() >= slots
            && self
                .reach
                .serves(surface.node_count(), surface.costs().max_step())
    }

    /// Whether this scratch has a slot for every column of `surface`: the
    /// part of [`Scratch::serves`] a search over the columns alone needs
    /// ([`astar`], [`dijkstra_bounded`]), which reads no decomposition. Such
    /// a search bounded to one cluster grows no buffer in a scratch that
    /// [`Scratch::serves`] the decomposition as well; one over the whole map
    /// ([`Bound::Whole`]) grows its heap, and is run by no tick.
    #[must_use]
    pub fn serves_surface(&self, surface: &Surface) -> bool {
        let columns = crate::math::index::row(surface.node_count());
        self.stamp.len() >= columns
            && self.closed.len() >= columns
            && self.g.len() >= columns
            && self.came.len() >= columns
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

    /// Visits, since the last reset, of the loops a query runs that pop
    /// nothing (S3's `walk3`), each counted where it happens: a stale heap
    /// entry a search skips, a neighbour slot a pop tries, a column a walk
    /// back reads, a slot an endpoint's insertion reads, an abstract edge
    /// relaxed, an abstract node a reconstruction reads and one its fog scan
    /// reads, a leg refined and a column of it copied, a step the smoother
    /// prices, a stretch it starts from, a shortcut it tries with every step
    /// of the straight walk it tries it by, a step the served route is priced
    /// at, and a cell an epoch's wrap rewrites.
    ///
    /// With the pops this is every loop of a served walk's search, so the
    /// counted work of a walk no longer rests on its pops bounding the rest:
    /// an endpoint inserted by lookup pops nothing and is counted by the
    /// slots it reads. A heap's own sift is not counted, as a sort's
    /// compares are not ([`crate::seams::UnitTally`]).
    ///
    /// # Errors
    ///
    /// [`TallyOverflow`] once the count passed `u64::MAX`.
    pub const fn visits(&self) -> Result<u64, TallyOverflow> {
        self.visits.read()
    }

    /// Estimates priced at the served walk since the last reset (every clear
    /// query, [`crate::pathing::estimate()`]). Diagnostic: it is how a test
    /// knows a query took the served walk and not the fogged abstract answer.
    ///
    /// # Errors
    ///
    /// [`TallyOverflow`] once the count passed `u64::MAX`.
    pub const fn served_estimates(&self) -> Result<u64, TallyOverflow> {
        self.served_estimates.read()
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

    /// Survey re-checks the router served no walk to since the last reset
    /// ([`crate::pathing::estimate::WalkError::NoPath`] met while a scout's
    /// walk to a due sighting was priced, `crate::survey`): a scout dropped
    /// as a candidate for that sighting, counted here so the drop is read,
    /// never silent.
    ///
    /// # Errors
    ///
    /// [`TallyOverflow`] once the count passed `u64::MAX`.
    pub const fn survey_unserved(&self) -> Result<u64, TallyOverflow> {
        self.survey_unserved.read()
    }

    /// Needed Survey sightings dropped as out of reach since the last reset,
    /// each once a Push (`crate::survey::Drop::OutOfReach`): no scout of the
    /// seat, setting off at once for it alone, reaches it before the Lull,
    /// even at the octile bound.
    ///
    /// The drop tallies and their log ([`Scratch::survey_drops`]) are a
    /// measurement: in the scratch, never hashed, never snapshotted, zero
    /// after a resume; tests alone read them.
    ///
    /// # Errors
    ///
    /// [`TallyOverflow`] once the count passed `u64::MAX`.
    pub const fn survey_out_of_reach(&self) -> Result<u64, TallyOverflow> {
        self.survey_out_of_reach.read()
    }

    /// Needed Survey sightings dropped because a scout was lost since the
    /// last reset, each once a Push (`crate::survey::Drop::ScoutLost`).
    ///
    /// # Errors
    ///
    /// [`TallyOverflow`] once the count passed `u64::MAX`.
    pub const fn survey_scout_lost(&self) -> Result<u64, TallyOverflow> {
        self.survey_scout_lost.read()
    }

    /// Needed Survey sightings a scout alone would reach that no list took
    /// since the last reset, each once a Push
    /// (`crate::survey::Drop::CrowdedOut`).
    ///
    /// # Errors
    ///
    /// [`TallyOverflow`] once the count passed `u64::MAX`.
    pub const fn survey_crowded_out(&self) -> Result<u64, TallyOverflow> {
        self.survey_crowded_out.read()
    }

    /// Needed Survey sightings dropped because a threat the seat sees now
    /// covers them since the last reset, each once a Push
    /// (`crate::survey::Drop::UnderThreat`).
    ///
    /// # Errors
    ///
    /// [`TallyOverflow`] once the count passed `u64::MAX`.
    pub const fn survey_under_threat(&self) -> Result<u64, TallyOverflow> {
        self.survey_under_threat.read()
    }

    /// Needed Survey sightings that ranked past the most due a decision
    /// weighs since the last reset, each once a Push
    /// (`crate::survey::Drop::Unweighed`): a seat held more needed sightings
    /// than one decision's lists take, and these were asked of no list.
    ///
    /// # Errors
    ///
    /// [`TallyOverflow`] once the count passed `u64::MAX`.
    pub const fn survey_unweighed(&self) -> Result<u64, TallyOverflow> {
        self.survey_unweighed.read()
    }

    /// Survey drops, listings or scouts of a Push past the room
    /// [`crate::survey::Memory`] reserves: counted here, so a drop whose
    /// reason the memory could not name is never silent, and the drop is not
    /// counted again on every decision.
    ///
    /// # Errors
    ///
    /// [`TallyOverflow`] once the count passed `u64::MAX`.
    pub const fn survey_drops_unlogged(&self) -> Result<u64, TallyOverflow> {
        self.survey_drops_unlogged.read()
    }

    /// The running Push's Survey drops, once each for every seat, asset and
    /// reason: the seat, the sighting's asset id, the reason and the decision
    /// tick it was last dropped on. Emptied on the first Survey decision of
    /// every Push, so a Push with no drop reads an empty log.
    #[must_use]
    pub fn survey_drops(&self) -> &[(u8, u32, crate::survey::Drop, u32)] {
        self.survey.log()
    }

    /// Log a Survey drop of the running Push on decision tick `tick`, once a
    /// Push for each seat, asset and reason, and count it under its reason.
    pub(crate) fn survey_drop(
        &mut self,
        seat: u8,
        asset: u32,
        reason: crate::survey::Drop,
        tick: u32,
    ) {
        match self.survey.log_drop(seat, asset, reason, tick) {
            Ok(false) => {}
            Ok(true) => match reason {
                crate::survey::Drop::OutOfReach => self.survey_out_of_reach.bump(),
                crate::survey::Drop::ScoutLost => self.survey_scout_lost.bump(),
                crate::survey::Drop::CrowdedOut => self.survey_crowded_out.bump(),
                crate::survey::Drop::UnderThreat => self.survey_under_threat.bump(),
                crate::survey::Drop::Unweighed => self.survey_unweighed.bump(),
            },
            Err(crate::survey::Full) => self.survey_drops_unlogged.bump(),
        }
    }

    /// Grid-optimum searches that met a [`crate::pathing::reach::ReachError`]
    /// since the last reset: a `toward` with no stone for it, counted here so
    /// the miss is read, never silent. Nothing hands the search a column it
    /// has no room for; `tests/nearest.rs` holds the count at zero.
    ///
    /// # Errors
    ///
    /// [`TallyOverflow`] once the count passed `u64::MAX`.
    pub const fn reach_faults(&self) -> Result<u64, TallyOverflow> {
        self.reach_faults.read()
    }

    /// Zero the counters.
    pub fn reset_counters(&mut self) {
        self.expansions = Tally::ZERO;
        self.endpoint_expansions = Tally::ZERO;
        self.abstract_expansions = Tally::ZERO;
        self.visits = Tally::ZERO;
        self.served_estimates = Tally::ZERO;
        self.walks = Tally::ZERO;
        self.stalled = Tally::ZERO;
        self.reach_faults = Tally::ZERO;
        self.survey_unserved = Tally::ZERO;
        self.survey_out_of_reach = Tally::ZERO;
        self.survey_scout_lost = Tally::ZERO;
        self.survey_crowded_out = Tally::ZERO;
        self.survey_under_threat = Tally::ZERO;
        self.survey_unweighed = Tally::ZERO;
        self.survey_drops_unlogged = Tally::ZERO;
        self.survey.clear();
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
            self.visits.add(self.stamp.len());
            self.visits.add(self.closed.len());
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

    /// Take the cheapest open column off the low-level heap: its entry, with
    /// every stale entry met on the way skipped and counted as one visit each
    /// ([`Scratch::visits`]); `None` once the heap is empty. A stale entry is
    /// one whose column is already closed: a column reached again at a lower
    /// cost is queued again, and its dearer entry stays in the heap until it
    /// surfaces.
    ///
    /// **The one way a column leaves the low-level heap**, as
    /// [`Scratch::queue`] is the one way in, so that a stale entry skipped is
    /// counted in one place for every search.
    pub(crate) fn pop_open(&mut self) -> Option<Item> {
        while let Some(item) = self.heap.pop() {
            if self.is_closed(item.n) {
                self.visits.bump();
                continue;
            }
            return Some(item);
        }
        None
    }

    /// Record `node` as reached at `cost` from `from` and queue it at the key
    /// `(cost + h, h, node)`, both or neither; and whether it was queued:
    /// `false`, with nothing recorded or queued, for a column this scratch
    /// has no slot for.
    ///
    /// **The one way a column enters the low-level heap**, so that a column
    /// with no record cannot be queued: it would never read as seen or
    /// closed, each neighbour would queue it again, and the search would
    /// never end and its heap would grow until the machine had no memory
    /// left (found by a review mutant of the gateway's planning scratch, S3's
    /// `p3est`). A scratch that serves its graph ([`Scratch::serves`]) has a
    /// slot for every column, and every public search holds its scratch to
    /// that before it searches ([`Misfit`]), so `false` is met by no search
    /// the crate runs; what it is for is that a search in any scratch ends.
    pub(crate) fn queue(&mut self, node: Node, cost: i32, from: Node, h: i32) -> bool {
        let Ok(index) = usize::try_from(node) else {
            return false;
        };
        let (Some(stamp), Some(g), Some(came)) = (
            self.stamp.get_mut(index),
            self.g.get_mut(index),
            self.came.get_mut(index),
        ) else {
            return false;
        };
        *stamp = self.epoch;
        *g = cost;
        *came = from;
        self.heap.push(Item {
            f: cost.saturating_add(h),
            h,
            n: node,
        });
        true
    }

    /// [`Scratch::queue`] for the abstract search: record the abstract node
    /// `node` as reached at `cost` from `from` and queue it at the key
    /// `(cost + h, h, node)`, both or neither; `false`, with nothing recorded
    /// or queued, for a node this scratch has no slot for. The one way a node
    /// enters the abstract heap, for the same reason.
    pub(crate) fn queue_abstract(&mut self, node: u32, cost: i32, from: u32, h: i32) -> bool {
        let epoch = self.abstract_epoch;
        let Ok(index) = usize::try_from(node) else {
            return false;
        };
        let (Some(stamp), Some(g), Some(came)) = (
            self.abstract_stamp.get_mut(index),
            self.abstract_g.get_mut(index),
            self.abstract_came.get_mut(index),
        ) else {
            return false;
        };
        *stamp = epoch;
        *g = cost;
        *came = from;
        self.abstract_heap.push(Item {
            f: cost.saturating_add(h),
            h,
            n: node,
        });
        true
    }

    /// Whether the abstract search under way has closed the abstract node
    /// `node`.
    pub(crate) fn abstract_is_closed(&self, node: u32) -> bool {
        usize::try_from(node)
            .ok()
            .and_then(|index| self.abstract_closed.get(index).copied())
            .is_some_and(|stamp| stamp == self.abstract_epoch)
    }

    /// Close the abstract node `node` for the abstract search under way.
    pub(crate) fn abstract_close(&mut self, node: u32) {
        let epoch = self.abstract_epoch;
        if let Ok(index) = usize::try_from(node)
            && let Some(slot) = self.abstract_closed.get_mut(index)
        {
            *slot = epoch;
        }
    }

    /// [`Scratch::pop_open`] for the abstract search: the cheapest open
    /// abstract node's entry, with every stale entry met on the way skipped
    /// and counted as one visit each. The one way a node leaves the abstract
    /// heap, for the same reason.
    pub(crate) fn pop_open_abstract(&mut self) -> Option<Item> {
        while let Some(item) = self.abstract_heap.pop() {
            if self.abstract_is_closed(item.n) {
                self.visits.bump();
                continue;
            }
            return Some(item);
        }
        None
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
///
/// # Errors
///
/// [`Misfit`] for a scratch with no slot for some column of `surface`
/// ([`Scratch::serves_surface`]), before any search. `Ok(None)` is no route.
pub fn astar_with_order(
    surface: &Surface,
    bound: Bound<'_>,
    scratch: &mut Scratch,
    start: Node,
    goal: Node,
    order: [u8; 8],
    out: &mut Vec<Node>,
) -> Result<Option<i32>, Misfit> {
    if !scratch.serves_surface(surface) {
        return Err(Misfit);
    }
    Ok(astar_inner(
        surface,
        bound,
        scratch,
        (start, goal),
        order,
        crate::pathing::route::Surcharge::NONE,
        out,
    ))
}

/// A\* with the default expansion order under a route surcharge (S2's
/// `atk`): a step onto a surcharged column costs the multiplier times its
/// ordinary cost. The heuristic stays octile at the ordinary costs, which a
/// surcharge of one or more never makes inadmissible.
///
/// # Errors
///
/// As [`astar_with_order`].
pub fn astar_costed(
    surface: &Surface,
    bound: Bound<'_>,
    scratch: &mut Scratch,
    (start, goal): (Node, Node),
    surcharge: crate::pathing::route::Surcharge<'_>,
    out: &mut Vec<Node>,
) -> Result<Option<i32>, Misfit> {
    if !scratch.serves_surface(surface) {
        return Err(Misfit);
    }
    Ok(astar_served(
        surface,
        bound,
        scratch,
        (start, goal),
        surcharge,
        out,
    ))
}

/// [`astar_costed`] in a scratch its caller has already held to
/// [`Scratch::serves`]: a route's leg refinement and an endpoint's direct
/// cost, each inside a query whose own door refused a misfit scratch.
pub(crate) fn astar_served(
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
    if !scratch.queue(start, 0, start, h0) {
        return None;
    }

    while let Some(item) = scratch.pop_open() {
        scratch.close(item.n);
        scratch.expansions.bump();
        if item.n == goal {
            let cost = scratch.cost_at(goal);
            scratch.reconstruct(start, goal, out);
            // One a column the walk back read.
            scratch.visits.add(out.len());
            return Some(cost);
        }
        let here = scratch.cost_at(item.n);
        // One a neighbour slot tried: the loop below never leaves early.
        scratch.visits.add(order.len());
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
            scratch.queue(next, cost, item.n, surface.heuristic(next, goal));
        }
    }
    None
}

/// A\* with the default expansion order.
///
/// # Errors
///
/// As [`astar_with_order`].
pub fn astar(
    surface: &Surface,
    bound: Bound<'_>,
    scratch: &mut Scratch,
    start: Node,
    goal: Node,
    out: &mut Vec<Node>,
) -> Result<Option<i32>, Misfit> {
    astar_with_order(surface, bound, scratch, start, goal, DEFAULT_ORDER, out)
}

/// Dijkstra from `source`, bounded, exhausting everything reachable inside the
/// bound. The costs stay in `scratch` and are read back with
/// [`Scratch::cost_of`].
///
/// `endpoint` says whether the sweep's expansions are charged to
/// [`Scratch::endpoint_expansions`] as well as to the general counter, so the
/// endpoint-sweep share of a query is a measurement rather than an estimate.
///
/// # Errors
///
/// [`Misfit`] for a scratch with no slot for some column of `surface`
/// ([`Scratch::serves_surface`]), before any search: the costs a sweep in it
/// left would read as "not reached" for columns it could not record.
pub fn dijkstra_bounded(
    surface: &Surface,
    bound: Bound<'_>,
    scratch: &mut Scratch,
    source: Node,
    endpoint: bool,
) -> Result<(), Misfit> {
    if !scratch.serves_surface(surface) {
        return Err(Misfit);
    }
    sweep_served(surface, bound, scratch, source, endpoint);
    Ok(())
}

/// [`dijkstra_bounded`] in a scratch its caller has already held to
/// [`Scratch::serves`]: an endpoint's insertion sweep inside a query, and a
/// cluster's inner edges inside a rebuild, each behind a door that refused a
/// misfit scratch.
pub(crate) fn sweep_served(
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
    if !scratch.queue(source, 0, source, 0) {
        return;
    }
    while let Some(item) = scratch.pop_open() {
        scratch.close(item.n);
        scratch.expansions.bump();
        if endpoint {
            scratch.endpoint_expansions.bump();
        }
        let here = scratch.cost_at(item.n);
        // One a neighbour slot tried: the loop below never leaves early.
        scratch.visits.add(DEFAULT_ORDER.len());
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
            scratch.queue(next, cost, item.n, 0);
        }
    }
}

/// The total cost of walking `route`, or `None` when a step on it is illegal —
/// which is what a stale route looks like after the terrain moved. Each step
/// priced is counted in `visits` ([`Scratch::visits`]).
#[must_use]
pub fn route_cost(surface: &Surface, route: &[Node], visits: &mut Tally) -> Option<i32> {
    let mut total: i32 = 0;
    for pair in route.windows(2) {
        visits.bump();
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
            let scratch = Scratch::new(nodes, abstract_nodes, cells, 18)
                .unwrap_or_else(|| panic!("a ring for 18"));
            assert!(
                scratch.served.capacity() >= row(MAX_ROUTE_NODES),
                "a scratch for {nodes} columns reserves {} served nodes, under {MAX_ROUTE_NODES}",
                scratch.served.capacity()
            );
        }
    }

    /// A column or an abstract node this scratch has no slot for is never
    /// queued, and nothing is recorded for it: the rule that ends a search in
    /// any scratch ([`Scratch::queue`]). One with a slot is recorded and
    /// queued at its key.
    #[test]
    fn a_node_with_no_slot_is_never_queued() {
        // Four columns and four abstract nodes (and the two temporary ones).
        let mut scratch = Scratch::new(4, 4, 4, 18).unwrap_or_else(|| panic!("a ring for 18"));
        scratch.epoch = 1;
        scratch.abstract_epoch = 1;
        for node in [4_u32, 5, 1_000, u32::MAX] {
            assert!(!scratch.queue(node, 7, 0, 3), "column {node} has no slot");
            assert!(!scratch.seen(node), "column {node} is not recorded");
        }
        assert_eq!(scratch.heap.len(), 0, "and nothing is queued");
        assert!(scratch.queue(3, 7, 0, 3), "column 3 has a slot");
        assert!(scratch.seen(3));
        assert_eq!(scratch.cost_of(3), Some(7));
        assert_eq!(
            scratch.heap.pop().map(super::Item::key),
            Some((10, 3, 3)),
            "queued at cost plus h, then h, then the column"
        );

        for node in [6_u32, 7, 1_000, u32::MAX] {
            assert!(
                !scratch.queue_abstract(node, 7, 0, 3),
                "abstract node {node} has no slot"
            );
        }
        assert_eq!(scratch.abstract_heap.len(), 0, "and nothing is queued");
        assert!(
            scratch.queue_abstract(5, 7, 0, 3),
            "the second temporary endpoint has a slot"
        );
        assert_eq!(
            scratch.abstract_heap.pop().map(super::Item::key),
            Some((10, 3, 5))
        );
        assert_eq!(scratch.abstract_g.get(5).copied(), Some(7));
    }

    /// A stale entry, one whose node closed at a lower cost after it was
    /// queued, is skipped as it surfaces and counted as one visit, in each
    /// heap's one exit ([`Scratch::pop_open`],
    /// [`Scratch::pop_open_abstract`]); an open node's entry is returned and
    /// counts nothing there.
    #[test]
    fn a_stale_entry_is_skipped_and_counted_as_it_surfaces() {
        let mut scratch = Scratch::new(4, 4, 4, 18).unwrap_or_else(|| panic!("a ring for 18"));
        scratch.epoch = 1;
        scratch.abstract_epoch = 1;
        // Column 2 is reached at 9 and again at 5, column 1 at 7: three
        // entries for two columns.
        assert!(
            scratch.queue(2, 9, 0, 0) && scratch.queue(2, 5, 0, 0) && scratch.queue(1, 7, 0, 0)
        );
        let first = scratch.pop_open().map(super::Item::key);
        assert_eq!(first, Some((5, 0, 2)), "the cheaper entry of column 2");
        scratch.close(2);
        let second = scratch.pop_open().map(super::Item::key);
        assert_eq!(second, Some((7, 0, 1)));
        scratch.close(1);
        assert_eq!(
            scratch.visits(),
            Ok(0),
            "an open column's entry is no visit"
        );
        assert_eq!(scratch.pop_open(), None, "column 2's entry at 9 is stale");
        assert_eq!(scratch.visits(), Ok(1), "and is counted once");
        assert_eq!(scratch.pop_open(), None, "an empty heap");
        assert_eq!(scratch.visits(), Ok(1), "counts nothing");

        // The same three entries in the abstract heap.
        assert!(
            scratch.queue_abstract(2, 9, 0, 0)
                && scratch.queue_abstract(2, 5, 0, 0)
                && scratch.queue_abstract(1, 7, 0, 0)
        );
        let first = scratch.pop_open_abstract().map(super::Item::key);
        assert_eq!(first, Some((5, 0, 2)));
        assert!(!scratch.abstract_is_closed(2), "taken, and not yet closed");
        scratch.abstract_close(2);
        assert!(scratch.abstract_is_closed(2));
        let second = scratch.pop_open_abstract().map(super::Item::key);
        assert_eq!(second, Some((7, 0, 1)));
        scratch.abstract_close(1);
        assert_eq!(scratch.visits(), Ok(1), "an open node's entry is no visit");
        assert_eq!(scratch.pop_open_abstract(), None, "node 2's entry at 9");
        assert_eq!(scratch.visits(), Ok(2), "is stale, and is counted once");
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
