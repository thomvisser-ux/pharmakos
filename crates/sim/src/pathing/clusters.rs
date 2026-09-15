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
//! because endpoint insertion was 91 % of a median query's expansions and is
//! cluster-local by construction, so a second level could save at most 9 %.
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
//! # No corner entrance, recorded and deliberately not built
//!
//! Entrances are scanned on vertical and horizontal borders only, so a legal
//! diagonal step across a cluster *corner* carries no abstract edge.
//! Connectivity is unaffected — [`Surface::neighbour`]'s companion rule makes a
//! diagonal legal only where both cardinal crossings are legal too — but the
//! **estimate** pays: a corner costs it two cardinal crossings where the walker,
//! after smoothing, cuts it for one diagonal. That is most of the ≤32-cell
//! band's error, and G2 carried the fix to S3 rather than building it
//! (skeleton plan §2.1).
//!
//! PLACEHOLDER: the corner entrance — four extra transitions a cluster, where
//! the diagonal across the corner is legal, repairable from the same dirty set
//! because a corner's legality depends only on the four columns around it. It
//! is recorded here and deliberately not built, so that S3 inherits the fix
//! rather than rediscovering it (owner, at S3).
//!
//! # Every buffer is sized at construction, at a bound rather than a guess
//!
//! A border of `n` cells holds at most `n` maximal runs, and that bound is
//! **`n`, not `n / 2`**: two adjacent border cells can each be crossable while
//! not being steppable to each other along the border — a one-voxel wall
//! running down it, which is exactly the case the run definition above exists
//! for — so runs need no gap cell between them and a comb of 32 single-cell
//! runs is a state a crater can leave behind. A run emits one transition, or
//! two at its ends when it is longer than [`LONG_RUN`], and a two-transition
//! run is at least `LONG_RUN + 1` cells long, so `n` bounds a border's
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

use crate::pathing::LONG_RUN;
use crate::pathing::search::{Bound, Scratch, dijkstra_bounded};
use crate::pathing::surface::{DEFAULT_ORDER, Node, Surface};

/// `local_comp` of a column that cannot be stood on.
pub const NO_COMP: u16 = u16::MAX;

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

    comp_offset: Vec<u32>,
    comp_label: Vec<u32>,
    union_find: Vec<u32>,

    stack: Vec<Node>,
    gather: Vec<Node>,
}

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
    #[must_use]
    pub fn new(surface: &Surface, size: i32, scratch: &mut Scratch) -> Option<Clusters> {
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
        let vborders = usize::try_from(nx.saturating_sub(1).max(0).checked_mul(ny)?).ok()?;
        let hborders = usize::try_from(nx.checked_mul(ny.saturating_sub(1).max(0))?).ok()?;

        let mut built = Clusters {
            size,
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
            comp_offset: vec![0; clusters.saturating_add(1)],
            comp_label: vec![0; nodes],
            union_find: vec![0; nodes],
            stack: Vec::with_capacity(usize::try_from(size.saturating_mul(size)).unwrap_or(0)),
            gather: Vec::with_capacity(max_trans),
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

        built.rebuild_all(surface, scratch);
        Some(built)
    }

    /// Rebuild everything from the surface. Construction, restore, and the
    /// from-scratch side of the repair-equivalence tests.
    pub fn rebuild_all(&mut self, surface: &Surface, scratch: &mut Scratch) {
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
            self.rebuild_intra(surface, scratch, c);
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

    /// Clusters along x and y.
    #[must_use]
    pub const fn counts(&self) -> [i32; 2] {
        [self.nx, self.ny]
    }

    /// How many clusters the map holds.
    #[must_use]
    pub fn cluster_count(&self) -> usize {
        usize::try_from(self.nx.saturating_mul(self.ny)).unwrap_or(0)
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
            .unwrap_or(0)
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
        let len = usize::from(self.vborder_len.get(index).copied().unwrap_or(0));
        let base = index.saturating_mul(self.max_trans_per_border);
        self.vborder
            .get(base..base.saturating_add(len))
            .unwrap_or(&[])
    }

    /// The transitions on one horizontal border.
    #[must_use]
    pub fn hborder(&self, index: usize) -> &[Trans] {
        let len = usize::from(self.hborder_len.get(index).copied().unwrap_or(0));
        let base = index.saturating_mul(self.max_trans_per_border);
        self.hborder
            .get(base..base.saturating_add(len))
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
            for offset in Clusters::run_marks(cursor, run_end).into_iter().flatten() {
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
            for offset in Clusters::run_marks(cursor, run_end).into_iter().flatten() {
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
    fn run_marks(start: i32, end: i32) -> [Option<i32>; 2] {
        let length = end.saturating_sub(start).saturating_add(1);
        if length <= LONG_RUN {
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
            *slot = len;
        }
        self.gather = gather;
    }

    /// One cluster's transition cells, ascending.
    #[must_use]
    pub fn trans_cells(&self, cluster: usize) -> &[Node] {
        let len = usize::from(self.trans_len.get(cluster).copied().unwrap_or(0));
        let base = cluster.saturating_mul(self.max_trans);
        self.trans_cell
            .get(base..base.saturating_add(len))
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
        let index = usize::try_from(node).unwrap_or(0);
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
    pub fn rebuild_intra(&mut self, surface: &Surface, scratch: &mut Scratch, cluster: usize) {
        let Ok(tag) = u16::try_from(cluster) else {
            return;
        };
        let count = self.trans_cells(cluster).len();
        let row_base = cluster.saturating_mul(self.max_trans.saturating_add(1));
        let edge_base = cluster.saturating_mul(self.max_intra);
        let mut written: usize = 0;
        let mut from_slot = 0;
        while from_slot < count {
            self.set_intra_row(row_base.saturating_add(from_slot), written);
            let from_cell = self.trans_cells(cluster).get(from_slot).copied();
            if let Some(from_cell) = from_cell
                && self.local_comp(from_cell) != NO_COMP
            {
                dijkstra_bounded(
                    surface,
                    Bound::Cluster(&self.of_node, tag),
                    scratch,
                    from_cell,
                    false,
                );
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
        let mut tail = count;
        while tail <= self.max_trans {
            self.set_intra_row(row_base.saturating_add(tail), written);
            tail = tail.saturating_add(1);
        }
    }

    fn set_intra_row(&mut self, at: usize, value: usize) {
        if let Ok(value) = u16::try_from(value)
            && let Some(slot) = self.intra_row.get_mut(at)
        {
            *slot = value;
        }
    }

    /// One transition slot's intra edges, as `(slot, cost)` pairs.
    #[must_use]
    pub fn intra_edges(&self, cluster: usize, slot: usize) -> (&[u16], &[i32]) {
        let row_base = cluster.saturating_mul(self.max_trans.saturating_add(1));
        let from = usize::from(
            self.intra_row
                .get(row_base.saturating_add(slot))
                .copied()
                .unwrap_or(0),
        );
        let to = usize::from(
            self.intra_row
                .get(row_base.saturating_add(slot).saturating_add(1))
                .copied()
                .unwrap_or(0),
        );
        if to < from {
            return (&[], &[]);
        }
        let edge_base = cluster.saturating_mul(self.max_intra);
        (
            self.intra_to
                .get(edge_base.saturating_add(from)..edge_base.saturating_add(to))
                .unwrap_or(&[]),
            self.intra_cost
                .get(edge_base.saturating_add(from)..edge_base.saturating_add(to))
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
                // The four borders, in a fixed order: east, west, north, south.
                let east = self
                    .vborder_index(cx, cy)
                    .map(|index| (index, true, self.cluster_index(cx.saturating_add(1), cy)));
                let west = self
                    .vborder_index(cx.saturating_sub(1), cy)
                    .map(|index| (index, false, self.cluster_index(cx.saturating_sub(1), cy)));
                for entry in [east, west].into_iter().flatten() {
                    let (index, near_is_a, far) = entry;
                    let Some(far) = far else {
                        continue;
                    };
                    let mut found: Option<(Node, i32)> = None;
                    for trans in self.vborder(index) {
                        let (near, other) = if near_is_a {
                            (trans.a, trans.b)
                        } else {
                            (trans.b, trans.a)
                        };
                        if near == cell {
                            found = Some((other, trans.cost));
                        }
                    }
                    if let Some((other, cost)) = found
                        && let Some(target) = self.abstract_of(far, other)
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
                let north = self
                    .hborder_index(cx, cy)
                    .map(|index| (index, true, self.cluster_index(cx, cy.saturating_add(1))));
                let south = self
                    .hborder_index(cx, cy.saturating_sub(1))
                    .map(|index| (index, false, self.cluster_index(cx, cy.saturating_sub(1))));
                for entry in [north, south].into_iter().flatten() {
                    let (index, near_is_a, far) = entry;
                    let Some(far) = far else {
                        continue;
                    };
                    let mut found: Option<(Node, i32)> = None;
                    for trans in self.hborder(index) {
                        let (near, other) = if near_is_a {
                            (trans.a, trans.b)
                        } else {
                            (trans.b, trans.a)
                        };
                        if near == cell {
                            found = Some((other, trans.cost));
                        }
                    }
                    if let Some((other, cost)) = found
                        && let Some(target) = self.abstract_of(far, other)
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
        let mut tail = count;
        while tail <= self.max_trans {
            if let Some(at) = self.inter_row.get_mut(row_base.saturating_add(tail))
                && let Ok(value) = u16::try_from(written)
            {
                *at = value;
            }
            tail = tail.saturating_add(1);
        }
    }

    /// One transition slot's border edges, as `(abstract node, cost)`.
    #[must_use]
    pub fn inter_edges(&self, cluster: usize, slot: usize) -> (&[u32], &[i32]) {
        let row_base = cluster.saturating_mul(self.max_trans.saturating_add(1));
        let from = usize::from(
            self.inter_row
                .get(row_base.saturating_add(slot))
                .copied()
                .unwrap_or(0),
        );
        let to = usize::from(
            self.inter_row
                .get(row_base.saturating_add(slot).saturating_add(1))
                .copied()
                .unwrap_or(0),
        );
        if to < from {
            return (&[], &[]);
        }
        let edge_base = cluster.saturating_mul(self.max_trans);
        (
            self.inter_to
                .get(edge_base.saturating_add(from)..edge_base.saturating_add(to))
                .unwrap_or(&[]),
            self.inter_cost
                .get(edge_base.saturating_add(from)..edge_base.saturating_add(to))
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
            total = total.saturating_add(u32::from(self.comp_count.get(c).copied().unwrap_or(0)));
            c = c.saturating_add(1);
        }
        if let Some(slot) = self.comp_offset.get_mut(clusters) {
            *slot = total;
        }
        let count = usize::try_from(total).unwrap_or(0);
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
    }

    /// Iterative find with path halving: no recursion (MSVC's 1 MB stack).
    fn find(&mut self, start: u32) -> u32 {
        let mut current = start;
        let mut guard: u32 = 0;
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
                    .saturating_add(u32::try_from(to.len()).unwrap_or(0))
                    .saturating_add(u32::try_from(inter.len()).unwrap_or(0));
                slot = slot.saturating_add(1);
            }
            c = c.saturating_add(1);
        }
        total
    }

    /// A digest of the whole abstract graph: what the repair-equivalence tests
    /// compare, and what makes "repair equals a from-scratch build" a byte
    /// assertion rather than a spot check.
    #[must_use]
    pub fn fingerprint(&self) -> u64 {
        let mut enc = crate::encoding::Enc::with_capacity(64 * 1024);
        enc.u32(u32::try_from(self.cluster_count()).unwrap_or(0));
        let mut c = 0;
        while c < self.cluster_count() {
            let cells = self.trans_cells(c);
            enc.len(u32::try_from(cells.len()).unwrap_or(0));
            for cell in cells {
                enc.u32(*cell);
                enc.u32(self.component_label(*cell));
            }
            let mut slot = 0;
            while slot < cells.len() {
                let (to, cost) = self.intra_edges(c, slot);
                enc.len(u32::try_from(to.len()).unwrap_or(0));
                for (target, price) in to.iter().zip(cost.iter()) {
                    enc.u16(*target);
                    enc.i32(*price);
                }
                let (inter, inter_cost) = self.inter_edges(c, slot);
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
