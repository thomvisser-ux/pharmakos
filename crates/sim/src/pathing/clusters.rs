// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The HPA\* decomposition: clusters, entrances, transition nodes, per-cluster
//! intra edges, the abstract graph — and the connectivity oracle.
//!
//! # Cluster 32 is a chunk footprint, and there is one abstract level
//!
//! Item 58, on measurements rather than taste: at 32 the repath p99 was
//! 1.84 ms, the estimate p99 0.82 ms and the error p90 2.6 %, the only size
//! that passes all three. A cluster equals a chunk, so a crater's dirty chunk
//! list *is* its dirty cluster list with no translation. One abstract level,
//! because endpoint insertion was 91 % of a median query's expansions in the
//! spike (nearly all of it on the sim's own map, [`crate::pathing::estimate`])
//! and is cluster-local by construction, so a second level could save little.
//!
//! # Runs are defined so that they are internally connected
//!
//! An entrance is a maximal run of border cells that can be stepped across —
//! and, taken literally, that definition has a hole: two cells adjacent along
//! the border can each be steppable *across* it while not being steppable *to
//! each other* (a one-voxel wall running along the border). A transition node
//! in the middle of such a run would not represent the run.
//!
//! So a run here is maximal subject to **both** conditions: each pair is a
//! legal step across the border, and consecutive pairs are legal steps on
//! *both* sides. That closes the hole and buys the thing the oracle needs:
//! every cell of a run is in the same intra-cluster component as the run's
//! transition node, on both sides.
//!
//! # The connectivity oracle
//!
//! "No path" fired on 4.5 % of the spike's repaths and has to be a normal,
//! cheap result rather than an exhausted search — a walker sealed behind its
//! own crater is a legal game state (item 60). So the decomposition also keeps
//! per-cluster **local components** (a flood fill bounded to the cluster) and a
//! union-find over every `(cluster, local component)` pair, unioned across
//! every entrance and flattened after each rebuild.
//!
//! Two walkable columns are connected **iff** their local components share a
//! label. (Every union is a real connection; and a real path splits into
//! within-cluster segments and border crossings, every crossing lies in some
//! maximal run, and by the paragraph above a run's cells share a component with
//! its transition node on each side — so the chain of unions exists.) The
//! lookup is two array reads and a comparison.
//!
//! # No corner entrance, measured and not built
//!
//! Entrances are scanned on vertical and horizontal borders only, so a legal
//! diagonal step across a cluster *corner* carries no abstract edge.
//! Connectivity is unaffected — [`Surface::neighbour`]'s companion rule makes a
//! diagonal legal only where both cardinal crossings are legal too — and the
//! abstract cost pays two cardinal crossings where the walker, after smoothing,
//! cuts the corner for one diagonal.
//!
//! Spike G2 expected that to be most of the short routes' error. Lane
//! `p3fix` measured it on the sim's map (1 080 commanders walked over three
//! seeds, S3's P3 gate): a cluster corner carried under 1 % of the estimate's
//! excess on every band of 32 cells and under (12 % on the 32-to-64 band), and
//! a single border crossing whose transition lies off the straight line carried
//! 90 % to 99 % of it. The corner entrance would have changed the abstract
//! graph, and so every served route, for under 1 % of the error. What fixed
//! the error is pricing a clear query at the walk the router serves, near or
//! far ([`crate::pathing::estimate`]): the walker's smoothed route already
//! cuts the corner, so the corner costs the estimate nothing.
//!
//! What the corner entrance would be, should a later measurement want it: four
//! extra transitions a cluster, where the diagonal across the corner is legal,
//! repairable from the same dirty set because a corner's legality depends only
//! on the four columns around it. It changes the abstract graph, so it moves
//! every route the router serves and every chain that walks one.
//!
//! # Every buffer is sized at construction, at a bound rather than a guess
//!
//! A border of `n` cells holds at most `n` maximal runs, and that bound is
//! **`n`, not `n / 2`**: two adjacent border cells can each be crossable while
//! not being steppable to each other along the border — a one-voxel wall
//! running down it, which is exactly the case the run definition above exists
//! for — so runs need no gap cell between them and a comb of 32 single-cell
//! runs is a state a crater can leave behind. A run emits one transition, or
//! two at its ends when it is longer than `pathing.long_run` ([`ClusterShape::long_run`]), and a two-transition
//! run is at least `long_run + 1` cells long, so `n` bounds a border's
//! transitions either way.
//!
//! From there: `4 * cluster_edge` bounds a cluster's transition cells, and
//! `max_trans * (max_trans - 1)` bounds its directed intra edges. Those are the
//! strides below, and **nothing here ever grows**, which is what makes a repair
//! inside a tick allocation-free. The price is arrays sized for a fragmentation
//! no measured map has produced — about 14 MB at cluster 32 against the 9.4 MB
//! the chunk store itself costs — and the price is worth paying, because the
//! alternative is a transition silently dropped on overflow: deterministic, but
//! a route that exists and cannot be found, and an oracle that says connected
//! where the search says no. `a_border_holds_every_run_it_finds` builds the
//! comb and checks that all 32 survive.
//!
//! # Transition fields: an endpoint is inserted by lookup
//!
//! A query's two endpoints each need the cost to every transition cell of
//! their own cluster. A bounded sweep from the endpoint gives them, and on the
//! sim's own map the two sweeps were 2 048 of a served walk's 2 628 grid pops
//! (S3's `walk3`, measured over 130 walks on the golden seed: each sweep
//! exhausts its 32 by 32 cluster).
//!
//! The same numbers already pass through here. [`Clusters::rebuild_intra`]
//! sweeps the cluster from each transition cell to price its intra edges, and
//! **a step costs the same in both directions** ([`Surface::step_over`]: the
//! step rule reads both ends and a diagonal's two sides against both, and a
//! climb is charged for any change of height, up or down), so the cost from a
//! transition cell to a column is the cost from that column to it. The sweep's
//! whole result is therefore kept, as the cluster's **transition fields**: for
//! each transition slot, the cost to every column of the cluster. Inserting an
//! endpoint reads one entry per slot ([`Clusters::field`]), and a cluster's
//! fields are rebuilt exactly when its intra edges are, by the same sweep, so
//! a repair leaves them what a fresh build makes.
//!
//! The answers are the sweep's, to the unit: the fields are a stored copy of
//! a Dijkstra's costs, and `tests/pathing.rs` holds every entry of every
//! field to a sweep from its column, on fresh ground and after craters. No
//! route, estimate or chain moves.
//!
//! Two bounds, each with the sweep as its fallback rather than a wrong answer
//! ([`Clusters::field`] answers `None`, and the caller sweeps as it always
//! did):
//!
//! * a cluster keeps at most [`FIELDS_PER_CLUSTER`] fields. The bound on a
//!   cluster's transitions is `4 * edge` (above), and a field for each at
//!   cluster 32 would be 38 MB of derived state. A fresh map's clusters hold
//!   three or four transitions each (all 2 880 clusters of the twenty nightly
//!   seeds); walls and craters cut runs and add some, and the bound is set
//!   from where they add the most that was measured (the constant's own
//!   docs). A cluster past the bound keeps none and is swept.
//! * an entry is a `u16`, and its last value says "no walk". A cost at that
//!   value or past it leaves its cluster with no fields. Under the committed
//!   table's step costs no cluster has one (the dearest walk inside a cluster
//!   of 32 is under 1 024 steps of at most 18); a loadable table's can, since
//!   the rules' path-cost bound admits a dearest step in the thousands, and
//!   `tests/pathing.rs`'s `a_cost_past_a_field_entry_leaves_its_cluster_swept`
//!   holds the three cases either side of the bound to the sweep's answers.
//!
//! The bound on fields is exact (`a_cluster_keeps_its_fields_up_to_the_bound_exactly`):
//! a cluster with as many transition cells as the bound keeps its fields, and
//! one more leaves it swept.

use crate::pathing::search::{Bound, Misfit, Scratch, sweep_served};
use crate::pathing::surface::{DEFAULT_ORDER, Node, Surface};
use crate::reserved::Reserved;

/// `local_comp` of a column that cannot be stood on.
pub const NO_COMP: u16 = u16::MAX;

/// How many transition fields a cluster keeps (the module docs' "Transition
/// fields"; [`ClusterShape::fields`]): a cluster with more transition cells
/// than this keeps none, and its endpoints are inserted by the sweep.
///
/// A bound on memory, not on the game: nothing a seat can do changes an
/// answer through it, only how the answer is found. At cluster 32 on the
/// shipped map it is 18.9 MB (144 clusters, 64 fields, 1 024 `u16` entries).
///
/// Sized by the busiest ground measured, not the quietest: over the gate's
/// 8-minute segment (G3′-real's layout of 40 beacons with its walls and
/// structures, under the crater stream, with and without raids) the busiest
/// cluster held 44 transition cells, and 16 clusters of the 144 held more
/// than 16 from the first minute on, 25 by the last. At 16 fields those
/// clusters, the built-up ones a match's walks start and end in, would all
/// be swept; at 64 every cluster of that segment keeps its fields, with the
/// busiest at two thirds of the room.
pub const FIELDS_PER_CLUSTER: usize = 64;

/// A field entry for a column its transition cell does not reach.
const NO_FIELD_COST: u16 = u16::MAX;

/// The largest cluster edge the decomposition indexes.
///
/// A cluster of edge `n` holds at most `4 * n` transitions (the derivation is
/// in the module docs above) and at most `4n * (4n - 1)` directed intra edges.
/// The intra rows index those edges with `u16` offsets, and `4n(4n - 1)` is
/// 65 280 at `n = 64` against 65 792 at `n = 65`: one step past this ceiling,
/// `set_intra_row` would start dropping writes it cannot convert
/// and the graph would quietly lose edges. 64 is also the top of the cluster
/// sweep G2 measured, and 32 is the decided value (item 58).
/// `the_cluster_edge_ceiling_is_what_the_row_offsets_can_index` pins the
/// arithmetic so a widened cap cannot truncate a row unnoticed.
pub const MAX_CLUSTER_EDGE: i32 = 64;

/// The decomposition's shape, read from the rules table (the register's
/// S3-10; S3's `lic`): the cluster edge, how long a border run is before it
/// takes two transitions, and how far ahead the smoother looks; and, from no
/// rules row, how many transition fields a cluster keeps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClusterShape {
    /// How many transition fields a cluster keeps: [`FIELDS_PER_CLUSTER`] for
    /// every shape read from a rules table. No answer depends on it (the
    /// module docs' "Transition fields"), which is what
    /// [`ClusterShape::with_fields`] is for: a test's decomposition that
    /// keeps none answers every query by the sweep, and is held equal to one
    /// that keeps them.
    ///
    /// A bound, so a decomposition keeps the lesser of it and the most
    /// transitions a cluster of its edge can have, `4 * edge` (the module
    /// docs): a cluster never has more transitions than that, so room for
    /// more fields would never be read, and [`Clusters::shape`] reports the
    /// bound the decomposition keeps.
    pub fields: usize,
    /// The cluster edge in voxels, `locomotion.hpa_cluster_voxels`.
    pub size: i32,
    /// A border's open run longer than this many cells takes a transition at
    /// each end; a run no longer takes one, in its middle (`pathing.long_run`,
    /// spike G2's `LONG_RUN`). At least one.
    pub long_run: i32,
    /// How many route nodes ahead of its current one the smoother looks for a
    /// straight line (`pathing.smooth_lookahead`, spike G2's
    /// `SMOOTH_LOOKAHEAD`). At most [`crate::pathing::MAX_ROUTE_NODES`].
    pub smooth_lookahead: usize,
}

impl ClusterShape {
    /// Read the shape from the rules table, at the cluster edge `size` the
    /// table's typed read of `locomotion.hpa_cluster_voxels` gave
    /// ([`crate::rules::RulesTable::hpa_cluster_voxels`]).
    ///
    /// # Errors
    ///
    /// [`crate::rules::RulesError::MissingBlock`] for an absent `pathing`
    /// block, and [`crate::rules::RulesError::OutOfRange`] for a `long_run`
    /// below one -- a one-cell run emits two transitions, and a comb of them
    /// overflows a border's room and silently drops transitions, a route that
    /// exists and cannot be found (decisions-log item 156) -- or past an
    /// `i32`, or a `smooth_lookahead` above [`crate::pathing::MAX_ROUTE_NODES`],
    /// the most a route holds.
    pub fn read(
        message: &pharmakos_proto::gp::v1::RulesTable,
        size: i32,
    ) -> Result<ClusterShape, crate::rules::RulesError> {
        use crate::rules::RulesError;
        let pathing = message
            .pathing
            .as_ref()
            .ok_or(RulesError::MissingBlock("pathing"))?;
        let long_run = i32::try_from(pathing.long_run)
            .ok()
            .filter(|run| *run >= 1)
            .ok_or_else(|| RulesError::OutOfRange {
                field: "pathing.long_run".to_owned(),
                value: format!(
                    "{}; a run of one cell takes two transitions, and a comb of them drops some",
                    pathing.long_run
                ),
            })?;
        let smooth_lookahead = (pathing.smooth_lookahead <= crate::pathing::MAX_ROUTE_NODES)
            .then(|| crate::math::index::row(pathing.smooth_lookahead))
            .ok_or_else(|| RulesError::OutOfRange {
                field: "pathing.smooth_lookahead".to_owned(),
                value: format!(
                    "{}; the smoother looks at most a route's {} nodes ahead",
                    pathing.smooth_lookahead,
                    crate::pathing::MAX_ROUTE_NODES
                ),
            })?;
        Ok(ClusterShape {
            fields: FIELDS_PER_CLUSTER,
            size,
            long_run,
            smooth_lookahead,
        })
    }

    /// The same shape keeping `fields` transition fields a cluster: none, for
    /// the decomposition a test holds the fields to.
    #[must_use]
    pub const fn with_fields(self, fields: usize) -> ClusterShape {
        ClusterShape { fields, ..self }
    }

    /// The same shape at another cluster edge: a test's decomposition, or the
    /// cluster-size sweep.
    #[must_use]
    pub const fn with_size(self, size: i32) -> ClusterShape {
        ClusterShape { size, ..self }
    }
}

/// One border crossing: `a` in this cluster, `b` in the neighbouring one.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Trans {
    /// The cell on the lower-indexed cluster's side.
    pub a: Node,
    /// The cell on the higher-indexed cluster's side.
    pub b: Node,
    /// The cost of the single step between them.
    pub cost: i32,
}

/// The decomposition and the abstract graph over it.
///
/// Derived state: a pure function of the [`Surface`] and the cluster edge, so
/// it is neither hashed nor snapshotted. What makes that safe is that a rebuild
/// from scratch and an incremental repair produce the same graph, which is what
/// `tests/pathing.rs`'s three repair tests assert.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Clusters {
    size: i32,
    fields: usize,
    long_run: i32,
    smooth_lookahead: usize,
    nx: i32,
    ny: i32,
    max_trans_per_border: usize,
    max_trans: usize,
    max_intra: usize,

    of_node: Vec<u16>,
    local_comp: Vec<u16>,
    comp_count: Vec<u16>,

    vborder_len: Vec<u16>,
    vborder: Vec<Trans>,
    hborder_len: Vec<u16>,
    hborder: Vec<Trans>,

    trans_len: Vec<u16>,
    trans_cell: Vec<Node>,

    intra_row: Vec<u16>,
    intra_to: Vec<u16>,
    intra_cost: Vec<i32>,

    inter_row: Vec<u16>,
    inter_to: Vec<u32>,
    inter_cost: Vec<i32>,

    /// Whether each cluster's fields are whole ([`Clusters::field`]).
    fielded: Vec<bool>,
    /// The transition fields: `fields` blocks a cluster, each one
    /// entry a column of the cluster in `(y, x)` order.
    field: Vec<u16>,

    comp_offset: Vec<u32>,
    comp_label: Vec<u32>,
    union_find: Vec<u32>,

    stack: Reserved<Node>,
    gather: Reserved<Node>,
}

/// One cluster's transition fields read at one column ([`Clusters::field`]).
#[derive(Clone, Copy, Debug)]
pub struct FieldAt<'a> {
    block: &'a [u16],
    cells: usize,
    entry: usize,
}

impl FieldAt<'_> {
    /// The cost between the column and the cluster's transition `slot`, or
    /// `None` when no walk inside the cluster joins them.
    #[must_use]
    pub fn cost(&self, slot: usize) -> Option<i32> {
        let at = slot.checked_mul(self.cells)?.checked_add(self.entry)?;
        self.block
            .get(at)
            .copied()
            .filter(|cost| *cost != NO_FIELD_COST)
            .map(i32::from)
    }
}

/// A column that is not one of the graph's ([`Clusters::touch`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OffGraph(pub Node);

impl Clusters {
    /// Build the decomposition over `surface` at a cluster edge of `size`.
    ///
    /// `None` when `size` does not divide the map's footprint, is not positive,
    /// is above [`MAX_CLUSTER_EDGE`], or decomposes the map into more clusters
    /// than the `u16` tags the decomposition indexes with can name — all four
    /// of which are rules-table mistakes caught once, here, rather than every
    /// tick. The fourth matters because the conversions that write those tags
    /// are fallible and silent: a cluster above `u16::MAX` would leave every
    /// column in it at `of_node = 0` and `local_comp = NO_COMP`, so the oracle
    /// would answer confidently and wrongly rather than refusing to build.
    ///
    /// `None` as well when `scratch` does not serve the decomposition
    /// ([`Scratch::serves`]): building the inner edges is itself a search,
    /// and a scratch sized for another map or another cluster edge has no
    /// slot for some of what this one numbers. [`Scratch::for_map`] at this
    /// `size` always serves it.
    #[must_use]
    pub fn new(surface: &Surface, shape: ClusterShape, scratch: &mut Scratch) -> Option<Clusters> {
        let size = shape.size;
        let extent = surface.size();
        let sx = extent.first().copied()?;
        let sy = extent.get(1).copied()?;
        if size <= 0 || size > MAX_CLUSTER_EDGE || sx % size != 0 || sy % size != 0 {
            return None;
        }
        let nx = sx.checked_div(size)?;
        let ny = sy.checked_div(size)?;
        let clusters = usize::try_from(nx.checked_mul(ny)?).ok()?;
        if clusters > usize::from(u16::MAX).saturating_add(1) {
            return None;
        }
        let nodes = usize::try_from(surface.node_count()).ok()?;
        let per_border = usize::try_from(size).ok()?;
        let max_trans = per_border.checked_mul(4)?;
        let max_intra = max_trans.checked_mul(max_trans.saturating_sub(1))?;
        let cells = per_border.checked_mul(per_border)?;
        // No cluster has more than `max_trans` transitions, so no more
        // fields than that are ever kept, whatever the shape allows
        // (`ClusterShape::fields`): the fields' room is bounded by the map
        // and the cluster edge, never by the number a caller passed.
        let kept = shape.fields.min(max_trans);
        let fields = clusters.checked_mul(kept)?.checked_mul(cells)?;
        let vborders = usize::try_from(nx.saturating_sub(1).max(0).checked_mul(ny)?).ok()?;
        let hborders = usize::try_from(nx.checked_mul(ny.saturating_sub(1).max(0))?).ok()?;

        let mut built = Clusters {
            size,
            fields: kept,
            long_run: shape.long_run,
            smooth_lookahead: shape.smooth_lookahead,
            nx,
            ny,
            max_trans_per_border: per_border,
            max_trans,
            max_intra,
            of_node: vec![0; nodes],
            local_comp: vec![NO_COMP; nodes],
            comp_count: vec![0; clusters],
            vborder_len: vec![0; vborders],
            vborder: vec![Trans::default(); vborders.saturating_mul(per_border)],
            hborder_len: vec![0; hborders],
            hborder: vec![Trans::default(); hborders.saturating_mul(per_border)],
            trans_len: vec![0; clusters],
            trans_cell: vec![0; clusters.saturating_mul(max_trans)],
            intra_row: vec![0; clusters.saturating_mul(max_trans.saturating_add(1))],
            intra_to: vec![0; clusters.saturating_mul(max_intra)],
            intra_cost: vec![0; clusters.saturating_mul(max_intra)],
            inter_row: vec![0; clusters.saturating_mul(max_trans.saturating_add(1))],
            inter_to: vec![0; clusters.saturating_mul(max_trans)],
            inter_cost: vec![0; clusters.saturating_mul(max_trans)],
            fielded: vec![false; clusters],
            field: vec![NO_FIELD_COST; fields],
            comp_offset: vec![0; clusters.saturating_add(1)],
            comp_label: vec![0; nodes],
            union_find: vec![0; nodes],
            // `size` is a positive edge at most `MAX_CLUSTER_EDGE` (checked above).
            stack: Reserved::with_capacity(crate::math::index::row(
                size.unsigned_abs().saturating_mul(size.unsigned_abs()),
            )),
            gather: Reserved::with_capacity(max_trans),
        };

        let mut y = 0;
        while y < sy {
            let mut x = 0;
            while x < sx {
                if let Some(node) = surface.node_of(x, y)
                    && let Ok(index) = usize::try_from(node)
                    && let Some(slot) = built.of_node.get_mut(index)
                    && let Ok(cluster) = u16::try_from(
                        y.div_euclid(size)
                            .saturating_mul(nx)
                            .saturating_add(x.div_euclid(size)),
                    )
                {
                    *slot = cluster;
                }
                x = x.saturating_add(1);
            }
            y = y.saturating_add(1);
        }

        if !scratch.serves(surface, &built) {
            return None;
        }
        built.rebuild_all_served(surface, scratch);
        Some(built)
    }

    /// Rebuild everything from the surface. Restore, and the from-scratch
    /// side of the repair-equivalence tests.
    ///
    /// # Errors
    ///
    /// [`Misfit`] for a scratch that does not serve `surface` and this
    /// decomposition ([`Scratch::serves`]), before anything is rebuilt: the
    /// inner edges are found by searches, and searches in such a scratch
    /// would leave clusters with no inner edges and empty fields.
    pub fn rebuild_all(&mut self, surface: &Surface, scratch: &mut Scratch) -> Result<(), Misfit> {
        if !scratch.serves(surface, self) {
            return Err(Misfit);
        }
        self.rebuild_all_served(surface, scratch);
        Ok(())
    }

    /// [`Clusters::rebuild_all`] in a scratch already held to
    /// [`Scratch::serves`]: construction's, and a restore's in the world's own
    /// scratch, which was made with this graph.
    pub(crate) fn rebuild_all_served(&mut self, surface: &Surface, scratch: &mut Scratch) {
        let clusters = self.cluster_count();
        let mut c = 0;
        while c < clusters {
            self.rebuild_local_comp(surface, c);
            c = c.saturating_add(1);
        }
        let mut cy = 0;
        while cy < self.ny {
            let mut cx = 0;
            while cx < self.nx {
                if cx.saturating_add(1) < self.nx {
                    self.rebuild_vborder(surface, cx, cy);
                }
                if cy.saturating_add(1) < self.ny {
                    self.rebuild_hborder(surface, cx, cy);
                }
                cx = cx.saturating_add(1);
            }
            cy = cy.saturating_add(1);
        }
        let mut c = 0;
        while c < clusters {
            self.rebuild_trans(c);
            c = c.saturating_add(1);
        }
        let mut c = 0;
        while c < clusters {
            self.rebuild_intra_served(surface, scratch, c);
            self.rebuild_inter(c);
            c = c.saturating_add(1);
        }
        self.rebuild_components();
    }

    // -- geometry -----------------------------------------------------------

    /// The cluster edge in voxels.
    #[must_use]
    pub const fn size(&self) -> i32 {
        self.size
    }

    /// The shape the decomposition was built with ([`ClusterShape`]).
    #[must_use]
    pub const fn shape(&self) -> ClusterShape {
        ClusterShape {
            fields: self.fields,
            size: self.size,
            long_run: self.long_run,
            smooth_lookahead: self.smooth_lookahead,
        }
    }

    /// Clusters along x and y.
    #[must_use]
    pub const fn counts(&self) -> [i32; 2] {
        [self.nx, self.ny]
    }

    /// How many clusters the map holds.
    #[must_use]
    pub fn cluster_count(&self) -> usize {
        // Both are positive and their product was checked at construction to
        // fit `u16::MAX + 1`, so the product cannot saturate.
        crate::math::index::row(
            self.nx
                .unsigned_abs()
                .saturating_mul(self.ny.unsigned_abs()),
        )
    }

    /// The most transition nodes one cluster can hold: the stride of the
    /// abstract numbering.
    #[must_use]
    pub const fn max_trans(&self) -> usize {
        self.max_trans
    }

    /// How many abstract node slots the numbering spans.
    #[must_use]
    pub fn abstract_capacity(&self) -> u32 {
        // E7: rule: a numbering past `u32` saturates, and `build_graph` refuses a scratch that disagrees with it
        u32::try_from(self.cluster_count().saturating_mul(self.max_trans)).unwrap_or(u32::MAX)
    }

    /// The cluster index of `(cx, cy)`.
    #[must_use]
    pub fn cluster_index(&self, cx: i32, cy: i32) -> Option<usize> {
        if cx < 0 || cy < 0 || cx >= self.nx || cy >= self.ny {
            return None;
        }
        usize::try_from(cy.checked_mul(self.nx)?.checked_add(cx)?).ok()
    }

    /// The `(cx, cy)` of a cluster index.
    #[must_use]
    pub fn cluster_coord(&self, cluster: usize) -> (i32, i32) {
        let nx = self.nx.max(1);
        let Ok(index) = i32::try_from(cluster) else {
            return (0, 0);
        };
        (index.rem_euclid(nx), index.div_euclid(nx))
    }

    /// The cluster a column belongs to.
    #[must_use]
    pub fn cluster_of(&self, node: Node) -> u16 {
        usize::try_from(node)
            .ok()
            .and_then(|index| self.of_node.get(index).copied())
            // E7: cannot miss: the graph's arrays are sized at construction for every column, cluster and slot the decomposition numbers (`Clusters::new`)
            .unwrap_or(0)
    }

    /// Whether the clusters of `a` and `b` touch: the same cluster, or two
    /// that share a border or a corner (at most one cluster apart on each
    /// axis): two columns at most one cluster edge apart on both axes always
    /// pass it. The estimator reads only its refusal, a column off the graph
    /// ([`crate::pathing::estimate`]); since lane `p3fix` prices every clear
    /// query at the served walk, it no longer splits near pairs from far, and
    /// the tests draw near pairs by it.
    ///
    /// # Errors
    ///
    /// [`OffGraph`] when either column is not one of the graph's: neither
    /// near nor far, since no search can start or end there.
    pub fn touch(&self, a: Node, b: Node) -> Result<bool, OffGraph> {
        let tag = |node: Node| {
            usize::try_from(node)
                .ok()
                .and_then(|index| self.of_node.get(index).copied())
                .ok_or(OffGraph(node))
        };
        let (ta, tb) = (tag(a)?, tag(b)?);
        let (ax, ay) = self.cluster_coord(usize::from(ta));
        let (bx, by) = self.cluster_coord(usize::from(tb));
        Ok(ax.abs_diff(bx) <= 1 && ay.abs_diff(by) <= 1)
    }

    /// The per-column cluster map, for a bounded search.
    #[must_use]
    pub fn of_node(&self) -> &[u16] {
        &self.of_node
    }

    /// The local component of a column, or [`NO_COMP`].
    #[must_use]
    pub fn local_comp(&self, node: Node) -> u16 {
        usize::try_from(node)
            .ok()
            .and_then(|index| self.local_comp.get(index).copied())
            // E7: cannot miss: the graph's arrays are sized at construction for every column, cluster and slot the decomposition numbers (`Clusters::new`)
            .unwrap_or(NO_COMP)
    }

    // -- local components ---------------------------------------------------

    /// Flood-fill one cluster into components, with an explicit work stack: no
    /// recursion anywhere in this crate.
    pub fn rebuild_local_comp(&mut self, surface: &Surface, cluster: usize) {
        let (cx, cy) = self.cluster_coord(cluster);
        let Ok(tag) = u16::try_from(cluster) else {
            return;
        };
        let x0 = cx.saturating_mul(self.size);
        let y0 = cy.saturating_mul(self.size);
        let mut y = y0;
        while y < y0.saturating_add(self.size) {
            let mut x = x0;
            while x < x0.saturating_add(self.size) {
                if let Some(node) = surface.node_of(x, y) {
                    self.set_local_comp(node, NO_COMP);
                }
                x = x.saturating_add(1);
            }
            y = y.saturating_add(1);
        }

        let mut next: u16 = 0;
        let mut y = y0;
        while y < y0.saturating_add(self.size) {
            let mut x = x0;
            while x < x0.saturating_add(self.size) {
                let seed = surface.node_of(x, y);
                x = x.saturating_add(1);
                let Some(seed) = seed else {
                    continue;
                };
                if !surface.walkable(seed) || self.local_comp(seed) != NO_COMP {
                    continue;
                }
                self.stack.clear();
                self.stack.push(seed);
                self.set_local_comp(seed, next);
                while let Some(current) = self.stack.pop() {
                    for k in DEFAULT_ORDER {
                        let Some((other, _)) = surface.neighbour(current, usize::from(k)) else {
                            continue;
                        };
                        if self.cluster_of(other) != tag || self.local_comp(other) != NO_COMP {
                            continue;
                        }
                        self.set_local_comp(other, next);
                        self.stack.push(other);
                    }
                }
                next = next.saturating_add(1);
            }
            y = y.saturating_add(1);
        }
        if let Some(slot) = self.comp_count.get_mut(cluster) {
            *slot = next;
        }
    }

    fn set_local_comp(&mut self, node: Node, value: u16) {
        if let Ok(index) = usize::try_from(node)
            && let Some(slot) = self.local_comp.get_mut(index)
        {
            *slot = value;
        }
    }

    // -- entrances ----------------------------------------------------------

    /// The index of the vertical border between `(cx, cy)` and `(cx + 1, cy)`.
    #[must_use]
    pub fn vborder_index(&self, cx: i32, cy: i32) -> Option<usize> {
        if cx < 0 || cy < 0 || cx.saturating_add(1) >= self.nx || cy >= self.ny {
            return None;
        }
        usize::try_from(cy.checked_mul(self.nx.saturating_sub(1))?.checked_add(cx)?).ok()
    }

    /// The index of the horizontal border between `(cx, cy)` and `(cx, cy + 1)`.
    #[must_use]
    pub fn hborder_index(&self, cx: i32, cy: i32) -> Option<usize> {
        if cx < 0 || cy < 0 || cx >= self.nx || cy.saturating_add(1) >= self.ny {
            return None;
        }
        usize::try_from(cy.checked_mul(self.nx)?.checked_add(cx)?).ok()
    }

    /// The transitions on one vertical border.
    #[must_use]
    pub fn vborder(&self, index: usize) -> &[Trans] {
        // E7: cannot miss: the graph's arrays are sized at construction for every column, cluster and slot the decomposition numbers (`Clusters::new`)
        let len = usize::from(self.vborder_len.get(index).copied().unwrap_or(0));
        let base = index.saturating_mul(self.max_trans_per_border);
        self.vborder
            .get(base..base.saturating_add(len))
            // E7: cannot miss: the graph's arrays are sized at construction for every column, cluster and slot the decomposition numbers (`Clusters::new`)
            .unwrap_or(&[])
    }

    /// The transitions on one horizontal border.
    #[must_use]
    pub fn hborder(&self, index: usize) -> &[Trans] {
        // E7: cannot miss: the graph's arrays are sized at construction for every column, cluster and slot the decomposition numbers (`Clusters::new`)
        let len = usize::from(self.hborder_len.get(index).copied().unwrap_or(0));
        let base = index.saturating_mul(self.max_trans_per_border);
        self.hborder
            .get(base..base.saturating_add(len))
            // E7: cannot miss: the graph's arrays are sized at construction for every column, cluster and slot the decomposition numbers (`Clusters::new`)
            .unwrap_or(&[])
    }

    /// Rescan the vertical border between `(cx, cy)` and `(cx + 1, cy)`.
    ///
    /// Returns `true` when the transition set changed, which is how a repair
    /// learns that the *neighbour's* intra edges went stale even though no
    /// voxel inside it moved.
    pub fn rebuild_vborder(&mut self, surface: &Surface, cx: i32, cy: i32) -> bool {
        let Some(index) = self.vborder_index(cx, cy) else {
            return false;
        };
        let xa = cx
            .saturating_add(1)
            .saturating_mul(self.size)
            .saturating_sub(1);
        let y0 = cy.saturating_mul(self.size);
        let base = index.saturating_mul(self.max_trans_per_border);
        // E7: cannot miss: the graph's arrays are sized at construction for every column, cluster and slot the decomposition numbers (`Clusters::new`)
        let before_len = usize::from(self.vborder_len.get(index).copied().unwrap_or(0));
        let mut written: usize = 0;
        let mut changed = false;

        let mut cursor = 0;
        while cursor < self.size {
            let pair = |offset: i32| -> Option<(Node, Node)> {
                let y = y0.checked_add(offset)?;
                Some((
                    surface.node_of(xa, y)?,
                    surface.node_of(xa.checked_add(1)?, y)?,
                ))
            };
            let Some(run_end) = Clusters::scan_run(surface, self.size, cursor, &pair) else {
                cursor = cursor.saturating_add(1);
                continue;
            };
            for offset in Clusters::run_marks(self.long_run, cursor, run_end)
                .into_iter()
                .flatten()
            {
                let Some((a, b)) = pair(offset) else {
                    continue;
                };
                let Some(cost) = surface.step_cost_between(a, b) else {
                    continue;
                };
                let entry = Trans { a, b, cost };
                let at = base.saturating_add(written);
                if written < self.max_trans_per_border {
                    if self.vborder.get(at).copied() != Some(entry) {
                        changed = true;
                    }
                    if let Some(slot) = self.vborder.get_mut(at) {
                        *slot = entry;
                    }
                    written = written.saturating_add(1);
                }
            }
            cursor = run_end.saturating_add(1);
        }
        if written != before_len {
            changed = true;
        }
        Clusters::clear_tail(&mut self.vborder, base, written, before_len);
        if let Ok(len) = u16::try_from(written)
            && let Some(slot) = self.vborder_len.get_mut(index)
        {
            *slot = len;
        }
        changed
    }

    /// Rescan the horizontal border between `(cx, cy)` and `(cx, cy + 1)`.
    pub fn rebuild_hborder(&mut self, surface: &Surface, cx: i32, cy: i32) -> bool {
        let Some(index) = self.hborder_index(cx, cy) else {
            return false;
        };
        let ya = cy
            .saturating_add(1)
            .saturating_mul(self.size)
            .saturating_sub(1);
        let x0 = cx.saturating_mul(self.size);
        let base = index.saturating_mul(self.max_trans_per_border);
        // E7: cannot miss: the graph's arrays are sized at construction for every column, cluster and slot the decomposition numbers (`Clusters::new`)
        let before_len = usize::from(self.hborder_len.get(index).copied().unwrap_or(0));
        let mut written: usize = 0;
        let mut changed = false;

        let mut cursor = 0;
        while cursor < self.size {
            let pair = |offset: i32| -> Option<(Node, Node)> {
                let x = x0.checked_add(offset)?;
                Some((
                    surface.node_of(x, ya)?,
                    surface.node_of(x, ya.checked_add(1)?)?,
                ))
            };
            let Some(run_end) = Clusters::scan_run(surface, self.size, cursor, &pair) else {
                cursor = cursor.saturating_add(1);
                continue;
            };
            for offset in Clusters::run_marks(self.long_run, cursor, run_end)
                .into_iter()
                .flatten()
            {
                let Some((a, b)) = pair(offset) else {
                    continue;
                };
                let Some(cost) = surface.step_cost_between(a, b) else {
                    continue;
                };
                let entry = Trans { a, b, cost };
                let at = base.saturating_add(written);
                if written < self.max_trans_per_border {
                    if self.hborder.get(at).copied() != Some(entry) {
                        changed = true;
                    }
                    if let Some(slot) = self.hborder.get_mut(at) {
                        *slot = entry;
                    }
                    written = written.saturating_add(1);
                }
            }
            cursor = run_end.saturating_add(1);
        }
        if written != before_len {
            changed = true;
        }
        Clusters::clear_tail(&mut self.hborder, base, written, before_len);
        if let Ok(len) = u16::try_from(written)
            && let Some(slot) = self.hborder_len.get_mut(index)
        {
            *slot = len;
        }
        changed
    }

    /// The end offset of the maximal run starting at `from`, or `None` when no
    /// run starts there.
    ///
    /// Maximal under **both** conditions of the module docs: each pair is a
    /// legal crossing, and consecutive pairs are legal steps on both sides.
    fn scan_run(
        surface: &Surface,
        size: i32,
        from: i32,
        pair: &impl Fn(i32) -> Option<(Node, Node)>,
    ) -> Option<i32> {
        let crossable = |offset: i32| -> bool {
            pair(offset).is_some_and(|(a, b)| surface.step_cost_between(a, b).is_some())
        };
        let linked = |first: i32, second: i32| -> bool {
            match (pair(first), pair(second)) {
                (Some((a0, b0)), Some((a1, b1))) => {
                    surface.step_cost_between(a0, a1).is_some()
                        && surface.step_cost_between(b0, b1).is_some()
                }
                _ => false,
            }
        };
        if !crossable(from) {
            return None;
        }
        let mut end = from;
        while end.saturating_add(1) < size
            && crossable(end.saturating_add(1))
            && linked(end, end.saturating_add(1))
        {
            end = end.saturating_add(1);
        }
        Some(end)
    }

    /// Where a run's transition nodes sit: the middle of a short run, both ends
    /// of a long one (item 58's shape, spike G2's `LONG_RUN`).
    fn run_marks(long_run: i32, start: i32, end: i32) -> [Option<i32>; 2] {
        let length = end.saturating_sub(start).saturating_add(1);
        if length <= long_run {
            [Some(start.saturating_add(end).div_euclid(2)), None]
        } else {
            [Some(start), Some(end)]
        }
    }

    // -- transition cells ---------------------------------------------------

    /// Recompute one cluster's transition cells: everything its four borders
    /// put inside it, sorted and deduplicated, so the abstract numbering is a
    /// function of the terrain alone.
    pub fn rebuild_trans(&mut self, cluster: usize) {
        let (cx, cy) = self.cluster_coord(cluster);
        let mut gather = core::mem::take(&mut self.gather);
        gather.clear();
        if let Some(index) = self.vborder_index(cx, cy) {
            for trans in self.vborder(index) {
                gather.push(trans.a);
            }
        }
        if let Some(index) = self.vborder_index(cx.saturating_sub(1), cy) {
            for trans in self.vborder(index) {
                gather.push(trans.b);
            }
        }
        if let Some(index) = self.hborder_index(cx, cy) {
            for trans in self.hborder(index) {
                gather.push(trans.a);
            }
        }
        if let Some(index) = self.hborder_index(cx, cy.saturating_sub(1)) {
            for trans in self.hborder(index) {
                gather.push(trans.b);
            }
        }
        // The key IS the column id, which is unique after the dedup below, so
        // the order is total (item 62).
        gather.sort_unstable();
        gather.dedup();
        let base = cluster.saturating_mul(self.max_trans);
        let mut written = 0;
        for cell in gather.iter().take(self.max_trans) {
            if let Some(slot) = self.trans_cell.get_mut(base.saturating_add(written)) {
                *slot = *cell;
            }
            written = written.saturating_add(1);
        }
        if let Ok(len) = u16::try_from(written)
            && let Some(slot) = self.trans_len.get_mut(cluster)
        {
            let before = core::mem::replace(slot, len);
            Clusters::clear_tail(&mut self.trans_cell, base, written, usize::from(before));
        }
        gather.clear();
        self.gather = gather;
    }

    /// One cluster's transition cells, ascending.
    #[must_use]
    pub fn trans_cells(&self, cluster: usize) -> &[Node] {
        // E7: cannot miss: the graph's arrays are sized at construction for every column, cluster and slot the decomposition numbers (`Clusters::new`)
        let len = usize::from(self.trans_len.get(cluster).copied().unwrap_or(0));
        let base = cluster.saturating_mul(self.max_trans);
        self.trans_cell
            .get(base..base.saturating_add(len))
            // E7: cannot miss: the graph's arrays are sized at construction for every column, cluster and slot the decomposition numbers (`Clusters::new`)
            .unwrap_or(&[])
    }

    /// The abstract node id of a transition cell inside a cluster.
    ///
    /// The numbering is `cluster * max_trans + slot`, so it is stable under a
    /// repair that does not change that cluster's transition set — and it never
    /// renumbers another cluster's nodes, which is what lets a repair touch five
    /// clusters instead of rebuilding a global graph.
    #[must_use]
    pub fn abstract_of(&self, cluster: usize, cell: Node) -> Option<u32> {
        let slot = self.trans_cells(cluster).binary_search(&cell).ok()?;
        u32::try_from(cluster.checked_mul(self.max_trans)?.checked_add(slot)?).ok()
    }

    /// The cluster and slot of an abstract node id.
    #[must_use]
    pub fn split_abstract(&self, node: u32) -> (usize, usize) {
        let index = crate::math::index::row(node);
        let stride = self.max_trans.max(1);
        (index.div_euclid(stride), index.rem_euclid(stride))
    }

    /// The column an abstract node stands on.
    #[must_use]
    pub fn abstract_cell(&self, node: u32) -> Option<Node> {
        let (cluster, slot) = self.split_abstract(node);
        self.trans_cells(cluster).get(slot).copied()
    }

    // -- intra edges --------------------------------------------------------

    /// Rebuild one cluster's intra edges: one bounded Dijkstra per transition
    /// cell, which reaches every other transition cell of the cluster in a
    /// single sweep.
    ///
    /// The plan's wording is "a bounded A\* between each pair"; the sweep
    /// computes the **same edge set with the same costs** for a fifth of the
    /// work. `an_intra_edge_is_the_cost_of_the_path_it_stands_for` in
    /// `tests/pathing.rs` is the check on the costs half, and it is a
    /// **sampled** one — the first two targets of every transition slot on
    /// every seventh cluster, each re-walked with a bounded A\* inside the
    /// cluster. Nothing compares the edge *sets* exhaustively; that is the half
    /// of the claim the argument still carries.
    ///
    /// # Errors
    ///
    /// [`Misfit`] for a scratch that does not serve `surface` and this
    /// decomposition ([`Scratch::serves`]), before anything is rebuilt.
    pub fn rebuild_intra(
        &mut self,
        surface: &Surface,
        scratch: &mut Scratch,
        cluster: usize,
    ) -> Result<(), Misfit> {
        if !scratch.serves(surface, self) {
            return Err(Misfit);
        }
        self.rebuild_intra_served(surface, scratch, cluster);
        Ok(())
    }

    /// [`Clusters::rebuild_intra`] in a scratch already held to
    /// [`Scratch::serves`]: a whole rebuild's and a repair's, each behind its
    /// own door.
    pub(crate) fn rebuild_intra_served(
        &mut self,
        surface: &Surface,
        scratch: &mut Scratch,
        cluster: usize,
    ) {
        // E7: cannot miss: a decomposition names its clusters with `u16` tags (`Clusters::new` refuses more), and every caller passes one of them
        let Ok(tag) = u16::try_from(cluster) else {
            return;
        };
        let count = self.trans_cells(cluster).len();
        let row_base = cluster.saturating_mul(self.max_trans.saturating_add(1));
        let edge_base = cluster.saturating_mul(self.max_intra);
        // The fields are whole only if every slot has one and every cost
        // fits an entry (the module docs' "Transition fields").
        let mut whole = count <= self.fields;
        let mut written: usize = 0;
        let mut from_slot = 0;
        while from_slot < count {
            self.set_intra_row(row_base.saturating_add(from_slot), written);
            let from_cell = self.trans_cells(cluster).get(from_slot).copied();
            let swept = from_cell.is_some_and(|cell| self.local_comp(cell) != NO_COMP);
            if let Some(from_cell) = from_cell
                && swept
            {
                sweep_served(
                    surface,
                    Bound::Cluster(&self.of_node, tag),
                    scratch,
                    from_cell,
                    false,
                );
            }
            if whole {
                whole = self.keep_field(surface, scratch, cluster, from_slot, swept);
            }
            if swept {
                let mut to_slot = 0;
                while to_slot < count {
                    if to_slot != from_slot
                        && let Some(to_cell) = self.trans_cells(cluster).get(to_slot).copied()
                        && let Some(cost) = scratch.cost_of(to_cell)
                        && written < self.max_intra
                        && let Ok(slot) = u16::try_from(to_slot)
                    {
                        let at = edge_base.saturating_add(written);
                        if let Some(target) = self.intra_to.get_mut(at) {
                            *target = slot;
                        }
                        if let Some(target) = self.intra_cost.get_mut(at) {
                            *target = cost;
                        }
                        written = written.saturating_add(1);
                    }
                    to_slot = to_slot.saturating_add(1);
                }
            }
            from_slot = from_slot.saturating_add(1);
        }
        // The block's last row holds its total; the write that replaces it
        // hands back the total an earlier build left, up to which its edges
        // are cleared.
        let mut before_total = None;
        let mut tail = count;
        while tail <= self.max_trans {
            before_total = self.set_intra_row(row_base.saturating_add(tail), written);
            tail = tail.saturating_add(1);
        }
        if let Some(before) = before_total {
            Clusters::clear_edges(
                (&mut self.intra_to, &mut self.intra_cost),
                edge_base,
                written,
                usize::from(before),
            );
        }
        // A cluster's fields are whole or empty, and the blocks past its
        // slots are empty either way, so a repaired decomposition holds what
        // a fresh build does, entry for entry.
        self.clear_fields(cluster, if whole { count } else { 0 });
        // E7: cannot miss: one flag a cluster, sized at construction (`Clusters::new`)
        if let Some(flag) = self.fielded.get_mut(cluster) {
            *flag = whole;
        }
    }

    /// How many columns a cluster holds: a field's length, and the room a
    /// search bounded to one cluster needs
    /// ([`Scratch::serves`](crate::pathing::search::Scratch::serves)).
    pub(crate) fn cluster_cells(&self) -> usize {
        self.max_trans_per_border
            .saturating_mul(self.max_trans_per_border)
    }

    /// Where a cluster's field for `slot` starts in [`Clusters::field`]'s
    /// table.
    fn field_base(&self, cluster: usize, slot: usize) -> usize {
        // E7: cannot miss: the table's whole length, `clusters * fields * cells`, was computed
        // checked at construction (`Clusters::new`), and every caller passes a cluster of the
        // decomposition and a slot no greater than `fields`, so no product here passes it
        cluster
            .saturating_mul(self.fields)
            .saturating_add(slot)
            .saturating_mul(self.cluster_cells())
    }

    /// A column's place in its cluster's fields: `(y, x)` order from the
    /// cluster's corner.
    fn field_entry(&self, surface: &Surface, cell: Node) -> usize {
        let (x, y) = surface.coord_of(cell);
        // E7: cannot miss: the cluster edge is positive (`Clusters::new` refuses any other), so
        // the floor is never taken; it keeps the remainders below total without a panic path,
        // and the products below are of two numbers under `MAX_CLUSTER_EDGE`
        let size = self.size.max(1);
        crate::math::index::row(
            y.rem_euclid(size)
                .saturating_mul(size)
                .saturating_add(x.rem_euclid(size))
                .unsigned_abs(),
        )
    }

    /// Keep the sweep `scratch` just ran from the cluster's transition `slot`
    /// as that slot's field, or an empty field when the slot's cell was not
    /// swept (`swept` false: it stands in no component). `false` when a cost
    /// does not fit an entry, which leaves the cluster with no fields.
    fn keep_field(
        &mut self,
        surface: &Surface,
        scratch: &Scratch,
        cluster: usize,
        slot: usize,
        swept: bool,
    ) -> bool {
        let (cx, cy) = self.cluster_coord(cluster);
        let size = self.size;
        let base = self.field_base(cluster, slot);
        let mut fits = true;
        let mut entry = 0_usize;
        let mut dy = 0;
        while dy < size {
            let mut dx = 0;
            while dx < size {
                let cost = if swept {
                    surface
                        .node_of(
                            cx.saturating_mul(size).saturating_add(dx),
                            cy.saturating_mul(size).saturating_add(dy),
                        )
                        .and_then(|node| scratch.cost_of(node))
                } else {
                    None
                };
                let kept = match cost.map(u16::try_from) {
                    None => NO_FIELD_COST,
                    Some(Ok(cost)) if cost != NO_FIELD_COST => cost,
                    Some(_) => {
                        fits = false;
                        NO_FIELD_COST
                    }
                };
                // E7: cannot miss: `base` is a field of this cluster and `entry` a column of
                // it, inside the table sized for every cluster's fields (`Clusters::new`)
                if let Some(target) = self.field.get_mut(base.saturating_add(entry)) {
                    *target = kept;
                }
                entry = entry.saturating_add(1);
                dx = dx.saturating_add(1);
            }
            dy = dy.saturating_add(1);
        }
        fits
    }

    /// Empty a cluster's field blocks from slot `from` on.
    fn clear_fields(&mut self, cluster: usize, from: usize) {
        // A slot past the bound has no block: the cluster's blocks end at
        // `fields`, and from there on there is nothing to empty.
        let start = self.field_base(cluster, from.min(self.fields));
        let end = self.field_base(cluster, self.fields);
        // E7: cannot miss: both ends are block starts of this cluster, in order, inside the
        // table sized for every cluster's fields (`Clusters::new`)
        if let Some(stale) = self.field.get_mut(start..end) {
            stale.fill(NO_FIELD_COST);
        }
    }

    /// The transition fields of the cluster `cell` stands in, read at `cell`
    /// (the module docs' "Transition fields"): the cost of the walk inside
    /// the cluster between `cell` and each of the cluster's transition cells,
    /// what a sweep bounded to the cluster gives from either end.
    ///
    /// `None` when the cluster keeps no fields (more transition cells than
    /// [`ClusterShape::fields`], or a cost past an entry), and the caller
    /// sweeps.
    #[must_use]
    pub fn field(&self, surface: &Surface, cell: Node) -> Option<FieldAt<'_>> {
        let cluster = usize::from(self.cluster_of(cell));
        if !self.fielded.get(cluster).copied()? {
            return None;
        }
        let cells = self.cluster_cells();
        let block = self
            .field
            .get(self.field_base(cluster, 0)..self.field_base(cluster, self.fields))?;
        Some(FieldAt {
            block,
            cells,
            entry: self.field_entry(surface, cell),
        })
    }

    /// Reset `values[base + from .. base + to]` to the default a fresh build
    /// leaves there: what a rebuild wrote no further than `from` that an
    /// earlier build had written up to `to`. A repaired decomposition is
    /// then the one a fresh build makes, field for field, and not only
    /// through the length-bounded readers (lane `settle`). No allocation.
    fn clear_tail<T: Copy + Default>(values: &mut [T], base: usize, from: usize, to: usize) {
        if let Some(stale) = values
            .get_mut(base..)
            .and_then(|block| block.get_mut(from..to))
        {
            stale.fill(T::default());
        }
    }

    /// [`Clusters::clear_tail`] over an edge table's two parallel arrays.
    fn clear_edges<T: Copy + Default>(
        (to, cost): (&mut [T], &mut [i32]),
        base: usize,
        from: usize,
        upto: usize,
    ) {
        Clusters::clear_tail(to, base, from, upto);
        Clusters::clear_tail(cost, base, from, upto);
    }

    /// Write one intra row, handing back the value it replaced.
    fn set_intra_row(&mut self, at: usize, value: usize) -> Option<u16> {
        let value = u16::try_from(value).ok()?;
        self.intra_row
            .get_mut(at)
            .map(|slot| core::mem::replace(slot, value))
    }

    /// One transition slot's intra edges, as `(slot, cost)` pairs.
    #[must_use]
    pub fn intra_edges(&self, cluster: usize, slot: usize) -> (&[u16], &[i32]) {
        let row_base = cluster.saturating_mul(self.max_trans.saturating_add(1));
        let from = usize::from(
            self.intra_row
                .get(row_base.saturating_add(slot))
                .copied()
                // E7: cannot miss: the graph's arrays are sized at construction for every column, cluster and slot the decomposition numbers (`Clusters::new`)
                .unwrap_or(0),
        );
        let to = usize::from(
            self.intra_row
                .get(row_base.saturating_add(slot).saturating_add(1))
                .copied()
                // E7: cannot miss: the graph's arrays are sized at construction for every column, cluster and slot the decomposition numbers (`Clusters::new`)
                .unwrap_or(0),
        );
        if to < from {
            return (&[], &[]);
        }
        let edge_base = cluster.saturating_mul(self.max_intra);
        (
            self.intra_to
                .get(edge_base.saturating_add(from)..edge_base.saturating_add(to))
                // E7: cannot miss: the graph's arrays are sized at construction for every column, cluster and slot the decomposition numbers (`Clusters::new`)
                .unwrap_or(&[]),
            self.intra_cost
                .get(edge_base.saturating_add(from)..edge_base.saturating_add(to))
                // E7: cannot miss: the graph's arrays are sized at construction for every column, cluster and slot the decomposition numbers (`Clusters::new`)
                .unwrap_or(&[]),
        )
    }

    // -- inter edges --------------------------------------------------------

    /// Rebuild one cluster's border edges: for each transition on one of its
    /// four borders, the abstract node on the far side and the cost of the
    /// single step across.
    pub fn rebuild_inter(&mut self, cluster: usize) {
        let (cx, cy) = self.cluster_coord(cluster);
        let count = self.trans_cells(cluster).len();
        let row_base = cluster.saturating_mul(self.max_trans.saturating_add(1));
        let edge_base = cluster.saturating_mul(self.max_trans);
        // The four borders, in a fixed order: east, west, north, south; each
        // as (border index, whether this cluster is the border's `a` side,
        // the vertical table or the horizontal one, the cluster beyond).
        let borders = [
            self.vborder_index(cx, cy).map(|index| {
                (
                    index,
                    true,
                    true,
                    self.cluster_index(cx.saturating_add(1), cy),
                )
            }),
            self.vborder_index(cx.saturating_sub(1), cy).map(|index| {
                (
                    index,
                    false,
                    true,
                    self.cluster_index(cx.saturating_sub(1), cy),
                )
            }),
            self.hborder_index(cx, cy).map(|index| {
                (
                    index,
                    true,
                    false,
                    self.cluster_index(cx, cy.saturating_add(1)),
                )
            }),
            self.hborder_index(cx, cy.saturating_sub(1)).map(|index| {
                (
                    index,
                    false,
                    false,
                    self.cluster_index(cx, cy.saturating_sub(1)),
                )
            }),
        ];
        let mut written: usize = 0;
        let mut slot = 0;
        while slot < count {
            if let Some(at) = self.inter_row.get_mut(row_base.saturating_add(slot))
                && let Ok(value) = u16::try_from(written)
            {
                *at = value;
            }
            let cell = self.trans_cells(cluster).get(slot).copied();
            if let Some(cell) = cell {
                for (index, near_is_a, vertical, far) in borders.into_iter().flatten() {
                    let Some(far) = far else {
                        continue;
                    };
                    let border = if vertical {
                        self.vborder(index)
                    } else {
                        self.hborder(index)
                    };
                    if let Some((target, cost)) = self.border_edge(border, cell, near_is_a, far)
                        && written < self.max_trans
                    {
                        let at = edge_base.saturating_add(written);
                        if let Some(slot) = self.inter_to.get_mut(at) {
                            *slot = target;
                        }
                        if let Some(slot) = self.inter_cost.get_mut(at) {
                            *slot = cost;
                        }
                        written = written.saturating_add(1);
                    }
                }
            }
            slot = slot.saturating_add(1);
        }
        // As in `rebuild_intra`: the write of the block's total hands back
        // the one an earlier build left.
        let mut before_total = None;
        let mut tail = count;
        while tail <= self.max_trans {
            before_total = u16::try_from(written).ok().and_then(|value| {
                self.inter_row
                    .get_mut(row_base.saturating_add(tail))
                    .map(|at| core::mem::replace(at, value))
            });
            tail = tail.saturating_add(1);
        }
        if let Some(before) = before_total {
            Clusters::clear_edges(
                (&mut self.inter_to, &mut self.inter_cost),
                edge_base,
                written,
                usize::from(before),
            );
        }
    }

    /// The border edge out of `cell` across one border: the abstract node of
    /// the transition's far cell in cluster `far`, and the step's cost. The
    /// last transition on the border that starts at `cell` wins, as it always
    /// has.
    fn border_edge(
        &self,
        border: &[Trans],
        cell: Node,
        near_is_a: bool,
        far: usize,
    ) -> Option<(u32, i32)> {
        let mut found: Option<(Node, i32)> = None;
        for trans in border {
            let (near, other) = if near_is_a {
                (trans.a, trans.b)
            } else {
                (trans.b, trans.a)
            };
            if near == cell {
                found = Some((other, trans.cost));
            }
        }
        let (other, cost) = found?;
        Some((self.abstract_of(far, other)?, cost))
    }

    /// One transition slot's border edges, as `(abstract node, cost)`.
    #[must_use]
    pub fn inter_edges(&self, cluster: usize, slot: usize) -> (&[u32], &[i32]) {
        let row_base = cluster.saturating_mul(self.max_trans.saturating_add(1));
        let from = usize::from(
            self.inter_row
                .get(row_base.saturating_add(slot))
                .copied()
                // E7: cannot miss: the graph's arrays are sized at construction for every column, cluster and slot the decomposition numbers (`Clusters::new`)
                .unwrap_or(0),
        );
        let to = usize::from(
            self.inter_row
                .get(row_base.saturating_add(slot).saturating_add(1))
                .copied()
                // E7: cannot miss: the graph's arrays are sized at construction for every column, cluster and slot the decomposition numbers (`Clusters::new`)
                .unwrap_or(0),
        );
        if to < from {
            return (&[], &[]);
        }
        let edge_base = cluster.saturating_mul(self.max_trans);
        (
            self.inter_to
                .get(edge_base.saturating_add(from)..edge_base.saturating_add(to))
                // E7: cannot miss: the graph's arrays are sized at construction for every column, cluster and slot the decomposition numbers (`Clusters::new`)
                .unwrap_or(&[]),
            self.inter_cost
                .get(edge_base.saturating_add(from)..edge_base.saturating_add(to))
                // E7: cannot miss: the graph's arrays are sized at construction for every column, cluster and slot the decomposition numbers (`Clusters::new`)
                .unwrap_or(&[]),
        )
    }

    // -- connectivity -------------------------------------------------------

    /// Rebuild the union-find over every `(cluster, local component)` pair and
    /// flatten the labels.
    ///
    /// One rebuild per tick, after the dirty clusters have been repaired
    /// (item 60). Allocation-free: the arrays are sized for the map's columns,
    /// which bounds the number of local components.
    pub fn rebuild_components(&mut self) {
        let clusters = self.cluster_count();
        let mut total: u32 = 0;
        let mut c = 0;
        while c < clusters {
            if let Some(slot) = self.comp_offset.get_mut(c) {
                *slot = total;
            }
            // E7: cannot miss: the graph's arrays are sized at construction for every column, cluster and slot the decomposition numbers (`Clusters::new`)
            total = total.saturating_add(u32::from(self.comp_count.get(c).copied().unwrap_or(0)));
            c = c.saturating_add(1);
        }
        // The offset past the last cluster holds the total; its write hands
        // back the total an earlier build left.
        let before_total = self
            .comp_offset
            .get_mut(clusters)
            .map(|slot| crate::math::index::row(core::mem::replace(slot, total)));
        let count = crate::math::index::row(total);
        let mut index = 0;
        while index < count {
            if let Some(slot) = self.union_find.get_mut(index)
                && let Ok(value) = u32::try_from(index)
            {
                *slot = value;
            }
            index = index.saturating_add(1);
        }

        let mut cy = 0;
        while cy < self.ny {
            let mut cx = 0;
            while cx < self.nx {
                if let Some(index) = self.vborder_index(cx, cy) {
                    let mut at = 0;
                    while at < self.vborder(index).len() {
                        if let Some(trans) = self.vborder(index).get(at).copied() {
                            let (a, b) = (self.component_id(trans.a), self.component_id(trans.b));
                            self.union(a, b);
                        }
                        at = at.saturating_add(1);
                    }
                }
                if let Some(index) = self.hborder_index(cx, cy) {
                    let mut at = 0;
                    while at < self.hborder(index).len() {
                        if let Some(trans) = self.hborder(index).get(at).copied() {
                            let (a, b) = (self.component_id(trans.a), self.component_id(trans.b));
                            self.union(a, b);
                        }
                        at = at.saturating_add(1);
                    }
                }
                cx = cx.saturating_add(1);
            }
            cy = cy.saturating_add(1);
        }

        let mut index = 0;
        while index < count {
            let Ok(id) = u32::try_from(index) else {
                break;
            };
            let root = self.find(id);
            if let Some(slot) = self.comp_label.get_mut(index) {
                *slot = root;
            }
            index = index.saturating_add(1);
        }
        if let Some(before) = before_total {
            Clusters::clear_tail(&mut self.comp_label, 0, count, before);
            Clusters::clear_tail(&mut self.union_find, 0, count, before);
        }
    }

    /// Iterative find with path halving: no recursion (MSVC's 1 MB stack).
    fn find(&mut self, start: u32) -> u32 {
        let mut current = start;
        let mut guard: u32 = 0;
        // E7: cannot miss: a count of the graph's own entries, bounded by the map's columns, fits a `u32`
        let limit = u32::try_from(self.union_find.len()).unwrap_or(u32::MAX);
        loop {
            let parent = self.parent(current);
            if parent == current || guard >= limit {
                return current;
            }
            let grand = self.parent(parent);
            if let Ok(index) = usize::try_from(current)
                && let Some(slot) = self.union_find.get_mut(index)
            {
                *slot = grand;
            }
            current = grand;
            guard = guard.saturating_add(1);
        }
    }

    fn parent(&self, node: u32) -> u32 {
        usize::try_from(node)
            .ok()
            .and_then(|index| self.union_find.get(index).copied())
            // E7: rule: a node outside the union-find is its own root
            .unwrap_or(node)
    }

    /// Union by **smaller root**, so the labels are a function of the terrain
    /// rather than of the order the borders happened to be visited in.
    fn union(&mut self, a: u32, b: u32) {
        if a == u32::MAX || b == u32::MAX {
            return;
        }
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return;
        }
        let (keep, drop) = if ra < rb { (ra, rb) } else { (rb, ra) };
        if let Ok(index) = usize::try_from(drop)
            && let Some(slot) = self.union_find.get_mut(index)
        {
            *slot = keep;
        }
    }

    /// The global component id of a walkable column, or `u32::MAX`.
    #[must_use]
    pub fn component_id(&self, node: Node) -> u32 {
        let local = self.local_comp(node);
        if local == NO_COMP {
            return u32::MAX;
        }
        let cluster = usize::from(self.cluster_of(node));
        self.comp_offset
            .get(cluster)
            .copied()
            // E7: cannot miss: the graph's arrays are sized at construction for every column, cluster and slot the decomposition numbers (`Clusters::new`)
            .unwrap_or(u32::MAX)
            .saturating_add(u32::from(local))
    }

    /// The flattened union-find label of a walkable column. Diagnostic.
    #[must_use]
    pub fn component_label(&self, node: Node) -> u32 {
        let id = self.component_id(node);
        usize::try_from(id)
            .ok()
            .and_then(|index| self.comp_label.get(index).copied())
            // E7: cannot miss: the graph's arrays are sized at construction for every column, cluster and slot the decomposition numbers (`Clusters::new`)
            .unwrap_or(u32::MAX)
    }

    /// **The cheap no-path answer**: two array reads and a comparison, exact.
    #[must_use]
    pub fn connected(&self, surface: &Surface, a: Node, b: Node) -> bool {
        if !surface.walkable(a) || !surface.walkable(b) {
            return false;
        }
        let (la, lb) = (self.component_label(a), self.component_label(b));
        la != u32::MAX && la == lb
    }

    // -- diagnostics --------------------------------------------------------

    /// How many transition nodes the whole graph carries.
    #[must_use]
    pub fn transition_count(&self) -> u32 {
        let mut total: u32 = 0;
        let mut c = 0;
        while c < self.cluster_count() {
            // E7: cannot miss: the graph's arrays are sized at construction for every column, cluster and slot the decomposition numbers (`Clusters::new`)
            total = total.saturating_add(u32::from(self.trans_len.get(c).copied().unwrap_or(0)));
            c = c.saturating_add(1);
        }
        total
    }

    /// How many directed abstract edges the whole graph carries.
    #[must_use]
    pub fn abstract_edge_count(&self) -> u32 {
        let mut total: u32 = 0;
        let mut c = 0;
        while c < self.cluster_count() {
            let count = self.trans_cells(c).len();
            let mut slot = 0;
            while slot < count {
                let (to, _) = self.intra_edges(c, slot);
                let (inter, _) = self.inter_edges(c, slot);
                total = total
                    // E7: cannot miss: a count of the graph's own entries, bounded by the map's columns, fits a `u32`
                    .saturating_add(u32::try_from(to.len()).unwrap_or(0))
                    // E7: cannot miss: a count of the graph's own entries, bounded by the map's columns, fits a `u32`
                    .saturating_add(u32::try_from(inter.len()).unwrap_or(0));
                slot = slot.saturating_add(1);
            }
            c = c.saturating_add(1);
        }
        total
    }

    /// A digest of the transition fields: which clusters keep them, and
    /// every entry. What the repair-equivalence tests compare beside
    /// [`Clusters::fingerprint`], which the path-hash golden pins and which
    /// therefore stays the abstract graph's alone.
    #[must_use]
    pub fn field_fingerprint(&self) -> u64 {
        let mut enc = crate::encoding::Enc::with_capacity(
            self.field
                .len()
                .saturating_mul(2)
                .saturating_add(self.fielded.len()),
        );
        for whole in &self.fielded {
            enc.bool(*whole);
        }
        for entry in &self.field {
            enc.u16(*entry);
        }
        enc.finish()
    }

    /// How many clusters keep their transition fields. Diagnostic; never in
    /// a tick.
    #[must_use]
    pub fn fielded_count(&self) -> usize {
        self.fielded.iter().filter(|whole| **whole).count()
    }

    /// A digest of the whole abstract graph: what the repair-equivalence tests
    /// compare, and what makes "repair equals a from-scratch build" a byte
    /// assertion rather than a spot check.
    #[must_use]
    pub fn fingerprint(&self) -> u64 {
        let mut enc = crate::encoding::Enc::with_capacity(64 * 1024);
        // E7: cannot miss: a count of the graph's own entries, bounded by the map's columns, fits a `u32`
        enc.u32(u32::try_from(self.cluster_count()).unwrap_or(0));
        let mut c = 0;
        while c < self.cluster_count() {
            let cells = self.trans_cells(c);
            // E7: cannot miss: a count of the graph's own entries, bounded by the map's columns, fits a `u32`
            enc.len(u32::try_from(cells.len()).unwrap_or(0));
            for cell in cells {
                enc.u32(*cell);
                enc.u32(self.component_label(*cell));
            }
            let mut slot = 0;
            while slot < cells.len() {
                let (to, cost) = self.intra_edges(c, slot);
                // E7: cannot miss: a count of the graph's own entries, bounded by the map's columns, fits a `u32`
                enc.len(u32::try_from(to.len()).unwrap_or(0));
                for (target, price) in to.iter().zip(cost.iter()) {
                    enc.u16(*target);
                    enc.i32(*price);
                }
                let (inter, inter_cost) = self.inter_edges(c, slot);
                // E7: cannot miss: a count of the graph's own entries, bounded by the map's columns, fits a `u32`
                enc.len(u32::try_from(inter.len()).unwrap_or(0));
                for (target, price) in inter.iter().zip(inter_cost.iter()) {
                    enc.u32(*target);
                    enc.i32(*price);
                }
                slot = slot.saturating_add(1);
            }
            c = c.saturating_add(1);
        }
        enc.finish()
    }
}
