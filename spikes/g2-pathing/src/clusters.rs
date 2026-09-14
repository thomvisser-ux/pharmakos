// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later
#![deny(clippy::float_arithmetic, clippy::as_conversions)]
//! The HPA\* decomposition: clusters, entrances, transition nodes, intra-cluster
//! edges, the abstract graph — and the cheap connectivity oracle.
//!
//! Plan §2 fixes the shape: *"Entrances are maximal runs of mutually-walkable
//! border cells between adjacent clusters, one transition node per run (two for
//! long runs). The abstract graph holds intra-cluster edges precomputed by a
//! bounded A\* between each pair of a cluster's transition nodes."* The cluster
//! edge is a run-time parameter ([`Clusters::build`] takes 16, 32 or 64) so the
//! plan's cluster-size sweep is one command-line flag rather than three builds.
//! 32 is the default because a cluster then has a chunk's footprint, and
//! *"an explosion dirtied chunk (cx, cy)"* maps straight onto *"repair cluster
//! (cx, cy)"*.
//!
//! # Runs are defined so that they are internally connected
//!
//! A "maximal run of mutually walkable border cells" is the textbook
//! definition, and taken literally it has a hole: two cells adjacent along the
//! border can each be walkable and each be steppable *across* the border while
//! not being steppable *to each other* (a one-voxel wall running along the
//! border). A transition node placed in the middle of such a run would not
//! represent the whole run.
//!
//! So a run here is maximal subject to **both** conditions: each cell pair is
//! mutually walkable across the border, and consecutive cells of the run are
//! legal steps on *both* sides. That closes the hole and buys something
//! valuable: every cell of a run is in the same intra-cluster component as the
//! run's transition node, on both sides. Which is what makes the connectivity
//! oracle below exact rather than approximate.
//!
//! # No corner entrance, and what that costs
//!
//! Entrances are scanned on vertical and horizontal borders only
//! ([`Clusters::rebuild_vborder`], [`Clusters::rebuild_hborder`]), so a legal
//! diagonal step across a cluster **corner** — `(x, z)` in cluster A to
//! `(x + 1, z + 1)` in the diagonal cluster D — carries no abstract edge.
//!
//! Connectivity is unaffected, and that is not luck: [`crate::world::World::
//! neighbour`]'s companion rule makes a diagonal legal only when both
//! orthogonal companions are walkable and within one voxel of both endpoints,
//! which is exactly the condition under which the two cardinal crossings are
//! legal too. So the union-find still joins A and D, and the oracle stays exact
//! (`the_connectivity_oracle_agrees_with_dijkstra` is the check).
//!
//! The **estimate** does pay for it. The estimate is the unsmoothed abstract
//! cost, so a corner costs it two cardinal crossings (≥ 20) where the walker,
//! once the smoother has the refined path, cuts the corner for 14. That is part
//! of why HPA\* excess is worst exactly where the editor shows short routes: in
//! the recorded run the ≤32-cell band has excess p90 9.2% / p99 64.9% /
//! max 145.2%, against p90 2.6% for routes over 256 cells. Emitting a corner
//! transition at each of a cluster's four corners where the diagonal is legal
//! would fix it — four extra [`Trans`] per cluster, repairable from the same
//! dirty set, since a corner's legality depends only on the four columns around
//! it. Recorded rather than done, because it would change every number in the
//! measurement after the fact.
//!
//! # The connectivity oracle
//!
//! Plan §3 step 4 wants *"no path" to be a normal, cheap result, not a
//! pathological search* — a walker sealed behind its own moat is a legal game
//! state. So the decomposition also maintains:
//!
//! * per-cluster **local components** (a flood fill bounded to the cluster);
//! * a union-find over all `(cluster, local component)` pairs, unioned across
//!   every entrance.
//!
//! Two walkable cells are connected **iff** their local components share a
//! union-find root. (⇐ every union is a real connection. ⇒ a real path splits
//! into within-cluster segments and border crossings; every crossing lies in
//! some maximal run, and by the paragraph above the run's cells are in the same
//! local component as its transition node on each side — so the chain of unions
//! exists.) The lookup is two array reads and a comparison, and the labels are
//! flattened after each rebuild so there is no `find` loop at query time.

use crate::astar::{Bound, Scratch, astar, dijkstra_bounded};
use crate::world::{DEFAULT_ORDER, NODES, W, World, node_of};
use crate::{id32, ix};

/// A run longer than this gets two transition nodes instead of one (plan §2).
const LONG_RUN: i32 = 6;

/// `local_comp` value for a cell that is not walkable.
pub const NO_COMP: u16 = u16::MAX;

/// How a cluster's intra edges are computed.
///
/// Plan §2 says *"intra-cluster edges precomputed by a bounded A\* between each
/// pair of a cluster's transition nodes"*, and [`IntraMode::PairwiseAstar`] is
/// exactly that. It is also, on this terrain, the single most expensive thing
/// the spike does: a cluster here carries ~23 transition nodes (voxel terracing
/// produces far more entrances than a flat 2D grid with binary obstacles), so
/// pairwise is ~250 searches per cluster, and a repair touches five clusters.
///
/// [`IntraMode::PerNodeDijkstra`] computes the **same edge set with the same
/// costs** — one bounded Dijkstra per transition node reaches every other
/// transition node of the cluster in a single sweep — for about a fifth of the
/// work. The equivalence is not an argument, it is a test
/// (`both_intra_modes_produce_the_same_graph`). It is the default, and the
/// bench can measure either, because "how much does the specified method cost,
/// and does the obvious alternative change any number other than the clock" is
/// exactly what the spike is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntraMode {
    PairwiseAstar,
    PerNodeDijkstra,
}

/// One entrance crossing: `a` in the lower-indexed cluster, `b` in the higher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trans {
    pub a: u32,
    pub b: u32,
    pub cost: i32,
}

/// A precomputed intra-cluster edge between two transition cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntraEdge {
    pub a: u32,
    pub b: u32,
    pub cost: i32,
}

/// The decomposition and the abstract graph built from it.
#[derive(Debug)]
pub struct Clusters {
    pub size: i32,
    pub intra_mode: IntraMode,
    pub nx: i32,
    pub nz: i32,
    /// Cluster index per grid node.
    pub of_node: Vec<u16>,
    /// Component index within the cluster, or [`NO_COMP`].
    pub local_comp: Vec<u16>,
    /// Number of local components per cluster.
    pub comp_count: Vec<u16>,
    /// Vertical borders, indexed `cz * (nx - 1) + cx`.
    pub vborder: Vec<Vec<Trans>>,
    /// Horizontal borders, indexed `cz * nx + cx`.
    pub hborder: Vec<Vec<Trans>>,
    /// Cached transition cells per cluster, sorted and deduplicated. Recomputed
    /// only for the clusters a repair actually touched; the graph rebuild reads
    /// it rather than re-deriving one sorted vector per cluster every time.
    pub trans: Vec<Vec<u32>>,
    /// Intra-cluster edges, per cluster, keyed by grid cell (not abstract id),
    /// so a graph rebuild can renumber the abstract nodes freely.
    pub intra: Vec<Vec<IntraEdge>>,

    // ---- the built abstract graph -------------------------------------
    /// Abstract node → grid cell. Sorted by (cluster, cell).
    pub cells: Vec<u32>,
    /// Abstract node → cluster.
    pub node_cluster: Vec<u16>,
    /// Offsets into [`Clusters::cells`], length `n_clusters + 1`.
    pub cluster_start: Vec<u32>,
    /// CSR offsets, length `cells.len() + 1`.
    pub edge_start: Vec<u32>,
    pub edge_to: Vec<u32>,
    pub edge_cost: Vec<i32>,

    /// Prefix sums of [`Clusters::comp_count`].
    comp_offset: Vec<u32>,
    /// Flattened union-find label per global component.
    comp_label: Vec<u32>,

    /// Reusable work stack for the flood fills. Explicit, because plan §5 bans
    /// recursion (MSVC's ~1 MB thread stack).
    stack: Vec<u32>,
    /// Reusable directed-edge buffer for [`Clusters::rebuild_graph`]. Owned so
    /// that a repair — which happens 20 times a second — allocates nothing for
    /// it; the first version rebuilt the CSR through one temporary `Vec` per
    /// abstract node and cost ~3 400 allocations per crater.
    edge_build: Vec<(u32, u32, i32)>,
    /// Reusable degree/cursor buffer for the CSR fill.
    degree: Vec<u32>,
}

impl Clusters {
    #[must_use]
    pub fn n_clusters(&self) -> usize {
        usize::try_from(self.nx * self.nz).expect("cluster count fits usize")
    }

    #[inline]
    #[must_use]
    pub fn cidx(&self, cx: i32, cz: i32) -> usize {
        usize::try_from(cz * self.nx + cx).expect("cluster index is non-negative")
    }

    #[inline]
    #[must_use]
    pub fn cxz(&self, c: usize) -> (i32, i32) {
        let ci = i32::try_from(c).expect("cluster index fits i32");
        (ci % self.nx, ci / self.nx)
    }

    #[inline]
    #[must_use]
    fn cluster_u16(&self, c: usize) -> u16 {
        u16::try_from(c).expect("at most 576 clusters")
    }

    /// Build the decomposition from scratch.
    ///
    /// # Panics
    /// If `size` does not divide the map edge.
    #[must_use]
    pub fn build(world: &World, size: i32, sc: &mut Scratch) -> Self {
        Self::build_with(world, size, sc, IntraMode::PerNodeDijkstra)
    }

    /// Build the decomposition from scratch with an explicit intra-edge method.
    ///
    /// # Panics
    /// If `size` does not divide the map edge.
    #[must_use]
    pub fn build_with(world: &World, size: i32, sc: &mut Scratch, mode: IntraMode) -> Self {
        assert!(size > 0 && W % size == 0, "cluster size must divide {W}");
        let nx = W / size;
        let nz = W / size;
        let n = usize::try_from(nx * nz).expect("fits");
        let mut of_node = vec![0u16; NODES];
        for z in 0..W {
            for x in 0..W {
                let c = (z / size) * nx + (x / size);
                of_node[ix(node_of(x, z).expect("in bounds"))] =
                    u16::try_from(c).expect("at most 576 clusters");
            }
        }
        let nvb = usize::try_from((nx - 1) * nz).expect("fits");
        let nhb = usize::try_from(nx * (nz - 1)).expect("fits");
        let mut me = Self {
            size,
            intra_mode: mode,
            nx,
            nz,
            of_node,
            local_comp: vec![NO_COMP; NODES],
            comp_count: vec![0; n],
            vborder: vec![Vec::new(); nvb],
            hborder: vec![Vec::new(); nhb],
            trans: vec![Vec::new(); n],
            intra: vec![Vec::new(); n],
            cells: Vec::new(),
            node_cluster: Vec::new(),
            cluster_start: Vec::new(),
            edge_start: Vec::new(),
            edge_to: Vec::new(),
            edge_cost: Vec::new(),
            comp_offset: Vec::new(),
            comp_label: Vec::new(),
            stack: Vec::new(),
            edge_build: Vec::new(),
            degree: Vec::new(),
        };
        for c in 0..n {
            me.rebuild_local_comp(world, c);
        }
        for cz in 0..nz {
            for cx in 0..nx {
                if cx + 1 < nx {
                    me.rebuild_vborder(world, cx, cz);
                }
                if cz + 1 < nz {
                    me.rebuild_hborder(world, cx, cz);
                }
            }
        }
        for c in 0..n {
            me.rebuild_trans(c);
        }
        for c in 0..n {
            me.rebuild_intra(world, sc, c);
        }
        me.rebuild_graph();
        me
    }

    // ---- local components ---------------------------------------------

    /// Flood-fill the cluster into components, with an explicit work stack.
    pub fn rebuild_local_comp(&mut self, world: &World, c: usize) {
        let (cx, cz) = self.cxz(c);
        let cu = self.cluster_u16(c);
        let x0 = cx * self.size;
        let z0 = cz * self.size;
        for z in z0..z0 + self.size {
            for x in x0..x0 + self.size {
                self.local_comp[ix(node_of(x, z).expect("in bounds"))] = NO_COMP;
            }
        }
        let mut next: u16 = 0;
        for z in z0..z0 + self.size {
            for x in x0..x0 + self.size {
                let seed = node_of(x, z).expect("in bounds");
                if !world.walkable(seed) || self.local_comp[ix(seed)] != NO_COMP {
                    continue;
                }
                self.stack.clear();
                self.stack.push(seed);
                self.local_comp[ix(seed)] = next;
                while let Some(cur) = self.stack.pop() {
                    for k in DEFAULT_ORDER {
                        let Some((nb, _)) = world.neighbour(cur, usize::from(k)) else {
                            continue;
                        };
                        if self.of_node[ix(nb)] != cu || self.local_comp[ix(nb)] != NO_COMP {
                            continue;
                        }
                        self.local_comp[ix(nb)] = next;
                        self.stack.push(nb);
                    }
                }
                next = next
                    .checked_add(1)
                    .expect("at most 4096 cells in a cluster");
            }
        }
        self.comp_count[c] = next;
    }

    // ---- entrances ------------------------------------------------------

    /// Place the transition node(s) of one run: one in the middle of a short
    /// run, one at each end of a long one (plan §2).
    fn emit_run(out: &mut Vec<Trans>, world: &World, pairs: &[(u32, u32)], s: usize, e: usize) {
        let mut take = |i: usize| {
            let (a, b) = pairs[i];
            let cost = world
                .step_cost_between(a, b)
                .expect("run members are mutually walkable");
            out.push(Trans { a, b, cost });
        };
        let len = i32::try_from(e - s + 1).expect("a border is at most 64 cells");
        if len <= LONG_RUN {
            take((s + e) / 2);
        } else {
            take(s);
            take(e);
        }
    }

    /// Entrances on the vertical border between clusters `(cx, cz)` and
    /// `(cx + 1, cz)`.
    pub fn rebuild_vborder(&mut self, world: &World, cx: i32, cz: i32) {
        let bi = self.vborder_index(cx, cz);
        let xa = (cx + 1) * self.size - 1;
        let xb = xa + 1;
        let z0 = cz * self.size;
        let z1 = z0 + self.size - 1;
        let pairs: Vec<(u32, u32)> = (z0..=z1)
            .map(|z| {
                (
                    node_of(xa, z).expect("in bounds"),
                    node_of(xb, z).expect("in bounds"),
                )
            })
            .collect();
        let mut out = Vec::new();
        Self::scan_runs(world, &pairs, &mut out);
        self.vborder[bi] = out;
    }

    /// Entrances on the horizontal border between `(cx, cz)` and `(cx, cz + 1)`.
    pub fn rebuild_hborder(&mut self, world: &World, cx: i32, cz: i32) {
        let bi = self.hborder_index(cx, cz);
        let za = (cz + 1) * self.size - 1;
        let zb = za + 1;
        let x0 = cx * self.size;
        let x1 = x0 + self.size - 1;
        let pairs: Vec<(u32, u32)> = (x0..=x1)
            .map(|x| {
                (
                    node_of(x, za).expect("in bounds"),
                    node_of(x, zb).expect("in bounds"),
                )
            })
            .collect();
        let mut out = Vec::new();
        Self::scan_runs(world, &pairs, &mut out);
        self.hborder[bi] = out;
    }

    /// Maximal runs, under both conditions described in the module docs.
    fn scan_runs(world: &World, pairs: &[(u32, u32)], out: &mut Vec<Trans>) {
        let n = pairs.len();
        let ok = |i: usize| -> bool {
            let (a, b) = pairs[i];
            world.step_cost_between(a, b).is_some()
        };
        let linked = |i: usize, j: usize| -> bool {
            let (ai, bi) = pairs[i];
            let (aj, bj) = pairs[j];
            world.step_cost_between(ai, aj).is_some() && world.step_cost_between(bi, bj).is_some()
        };
        let mut i = 0;
        while i < n {
            if !ok(i) {
                i += 1;
                continue;
            }
            let s = i;
            let mut e = i;
            while e + 1 < n && ok(e + 1) && linked(e, e + 1) {
                e += 1;
            }
            Self::emit_run(out, world, pairs, s, e);
            i = e + 1;
        }
    }

    // ---- intra-cluster edges -------------------------------------------

    /// Recompute the cached transition-cell list of one cluster.
    pub fn rebuild_trans(&mut self, c: usize) {
        self.trans[c] = self.compute_trans_cells(c);
    }

    /// The cached transition cells of a cluster.
    #[must_use]
    pub fn trans_cells(&self, c: usize) -> &[u32] {
        &self.trans[c]
    }

    /// Index of the vertical border between `(cx, cz)` and `(cx + 1, cz)`.
    #[must_use]
    pub fn vborder_index(&self, cx: i32, cz: i32) -> usize {
        usize::try_from(cz * (self.nx - 1) + cx).expect("border index is non-negative")
    }

    /// Index of the horizontal border between `(cx, cz)` and `(cx, cz + 1)`.
    #[must_use]
    pub fn hborder_index(&self, cx: i32, cz: i32) -> usize {
        usize::try_from(cz * self.nx + cx).expect("border index is non-negative")
    }

    /// The transition cells of a cluster: everything its four borders put
    /// inside it, sorted and deduplicated so the abstract numbering is a
    /// function of the terrain alone.
    #[must_use]
    fn compute_trans_cells(&self, c: usize) -> Vec<u32> {
        let (cx, cz) = self.cxz(c);
        let mut v: Vec<u32> = Vec::new();
        if cx + 1 < self.nx {
            v.extend(self.vborder[self.vborder_index(cx, cz)].iter().map(|t| t.a));
        }
        if cx > 0 {
            v.extend(
                self.vborder[self.vborder_index(cx - 1, cz)]
                    .iter()
                    .map(|t| t.b),
            );
        }
        if cz + 1 < self.nz {
            v.extend(self.hborder[self.hborder_index(cx, cz)].iter().map(|t| t.a));
        }
        if cz > 0 {
            v.extend(
                self.hborder[self.hborder_index(cx, cz - 1)]
                    .iter()
                    .map(|t| t.b),
            );
        }
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Rebuild a cluster's intra edges: bounded A\* between each pair of its
    /// transition nodes (plan §2), or the equivalent per-node bounded Dijkstra
    /// — see [`IntraMode`], and `both_intra_modes_produce_the_same_graph`.
    ///
    /// Either way, pairs in different local components are skipped without a
    /// search: they provably have no intra-cluster path, and a failed search is
    /// the expensive case, because it exhausts the cluster before giving up.
    /// That is a pure speed-up — the edge set is identical with or without it,
    /// which the repair-equivalence tests check.
    pub fn rebuild_intra(&mut self, world: &World, sc: &mut Scratch, c: usize) {
        let cu = self.cluster_u16(c);
        let cells = core::mem::take(&mut self.trans[c]);
        let mut out: Vec<IntraEdge> = Vec::new();
        match self.intra_mode {
            IntraMode::PairwiseAstar => {
                let mut path = Vec::new();
                for (i, &a) in cells.iter().enumerate() {
                    for &b in &cells[i + 1..] {
                        if self.local_comp[ix(a)] == NO_COMP
                            || self.local_comp[ix(a)] != self.local_comp[ix(b)]
                        {
                            continue;
                        }
                        if let Some(cost) = astar(
                            world,
                            Bound::Cluster(&self.of_node, cu),
                            sc,
                            a,
                            b,
                            &mut path,
                        ) {
                            out.push(IntraEdge { a, b, cost });
                        }
                    }
                }
            }
            IntraMode::PerNodeDijkstra => {
                for (i, &a) in cells.iter().enumerate() {
                    if self.local_comp[ix(a)] == NO_COMP {
                        continue;
                    }
                    if cells[i + 1..]
                        .iter()
                        .all(|b| self.local_comp[ix(*b)] != self.local_comp[ix(a)])
                    {
                        continue;
                    }
                    dijkstra_bounded(world, Bound::Cluster(&self.of_node, cu), sc, a);
                    for &b in &cells[i + 1..] {
                        if self.local_comp[ix(a)] != self.local_comp[ix(b)] {
                            continue;
                        }
                        if let Some(cost) = sc.g_of(b) {
                            out.push(IntraEdge { a, b, cost });
                        }
                    }
                }
            }
        }
        self.trans[c] = cells;
        self.intra[c] = out;
    }

    // ---- the abstract graph --------------------------------------------

    /// Renumber the abstract nodes and rebuild the CSR and the connectivity
    /// labels. Cheap enough (tens of thousands of writes) to run after every
    /// repair, which is what makes "repair == from-scratch" testable.
    pub fn rebuild_graph(&mut self) {
        let n = self.n_clusters();
        self.cells.clear();
        self.node_cluster.clear();
        self.cluster_start.clear();
        self.cluster_start.push(0);
        for c in 0..n {
            let cu = self.cluster_u16(c);
            let tc = core::mem::take(&mut self.trans[c]);
            for cell in &tc {
                self.cells.push(*cell);
                self.node_cluster.push(cu);
            }
            self.trans[c] = tc;
            self.cluster_start.push(id32(self.cells.len()));
        }

        // One sorted directed-edge list, then a single CSR fill. Sorting by
        // `(from, to, cost)` leaves every adjacency row sorted by `(to, cost)`
        // for free — plan §5's "every sort key ends in a unique id" — and does
        // it with one reused buffer instead of a temporary per node.
        let nn = self.cells.len();
        let mut edges = core::mem::take(&mut self.edge_build);
        edges.clear();
        for c in 0..n {
            for e in &self.intra[c] {
                let ia = self
                    .abs_id(c, e.a)
                    .expect("intra edge endpoint is a transition cell");
                let ib = self
                    .abs_id(c, e.b)
                    .expect("intra edge endpoint is a transition cell");
                edges.push((ia, ib, e.cost));
                edges.push((ib, ia, e.cost));
            }
        }
        for cz in 0..self.nz {
            for cx in 0..self.nx {
                if cx + 1 < self.nx {
                    let bi = self.vborder_index(cx, cz);
                    let (ca, cb) = (self.cidx(cx, cz), self.cidx(cx + 1, cz));
                    for t in &self.vborder[bi] {
                        let ia = self
                            .abs_id(ca, t.a)
                            .expect("entrance cell is a transition cell");
                        let ib = self
                            .abs_id(cb, t.b)
                            .expect("entrance cell is a transition cell");
                        edges.push((ia, ib, t.cost));
                        edges.push((ib, ia, t.cost));
                    }
                }
                if cz + 1 < self.nz {
                    let bi = self.hborder_index(cx, cz);
                    let (ca, cb) = (self.cidx(cx, cz), self.cidx(cx, cz + 1));
                    for t in &self.hborder[bi] {
                        let ia = self
                            .abs_id(ca, t.a)
                            .expect("entrance cell is a transition cell");
                        let ib = self
                            .abs_id(cb, t.b)
                            .expect("entrance cell is a transition cell");
                        edges.push((ia, ib, t.cost));
                        edges.push((ib, ia, t.cost));
                    }
                }
            }
        }
        // Counting fill: degrees, prefix sums, one scatter pass. A global sort
        // of 38 000 tuples was measured at twice the cost of this, and the row
        // order it produced was the same one the in-place pass below produces.
        let mut deg = core::mem::take(&mut self.degree);
        deg.clear();
        deg.resize(nn + 1, 0);
        for &(a, _, _) in &edges {
            deg[ix(a)] += 1;
        }
        self.edge_start.clear();
        self.edge_start.reserve(nn + 1);
        let mut acc: u32 = 0;
        for d in deg.iter().take(nn) {
            self.edge_start.push(acc);
            acc += *d;
        }
        self.edge_start.push(acc);
        let total = ix(acc);
        self.edge_to.clear();
        self.edge_to.resize(total, 0);
        self.edge_cost.clear();
        self.edge_cost.resize(total, 0);
        deg.clear();
        deg.extend_from_slice(&self.edge_start[..nn]);
        for &(a, b, c) in &edges {
            let p = ix(deg[ix(a)]);
            self.edge_to[p] = b;
            self.edge_cost[p] = c;
            deg[ix(a)] += 1;
        }
        self.degree = deg;
        self.edge_build = edges;

        // Every adjacency row sorted by `(to, cost)`, in place. Plan §5: "every
        // sort key ends in a unique id", so the graph is a function of the
        // terrain and not of the order the borders happened to be visited in.
        // Rows hold ~11 entries, so an insertion sort over the two parallel
        // arrays beats anything that would need a temporary.
        for node in 0..nn {
            let s0 = ix(self.edge_start[node]);
            let e0 = ix(self.edge_start[node + 1]);
            for i in s0 + 1..e0 {
                let (kt, kc) = (self.edge_to[i], self.edge_cost[i]);
                let mut j = i;
                while j > s0 && (self.edge_to[j - 1], self.edge_cost[j - 1]) > (kt, kc) {
                    self.edge_to[j] = self.edge_to[j - 1];
                    self.edge_cost[j] = self.edge_cost[j - 1];
                    j -= 1;
                }
                self.edge_to[j] = kt;
                self.edge_cost[j] = kc;
            }
        }

        self.rebuild_components();
    }

    /// Abstract id of `cell` inside cluster `c`, by binary search over the
    /// cluster's sorted slice.
    #[must_use]
    pub fn abs_id(&self, c: usize, cell: u32) -> Option<u32> {
        let s = ix(self.cluster_start[c]);
        let e = ix(self.cluster_start[c + 1]);
        self.cells[s..e]
            .binary_search(&cell)
            .ok()
            .map(|off| id32(s + off))
    }

    fn rebuild_components(&mut self) {
        let n = self.n_clusters();
        self.comp_offset.clear();
        let mut acc: u32 = 0;
        for c in 0..n {
            self.comp_offset.push(acc);
            acc += u32::from(self.comp_count[c]);
        }
        self.comp_offset.push(acc);
        let total = ix(acc);
        let mut uf: Vec<u32> = (0..total).map(id32).collect();
        // Iterative find with path halving: no recursion (plan §5).
        fn find(uf: &mut [u32], mut x: u32) -> u32 {
            while uf[ix(x)] != x {
                let g = uf[ix(uf[ix(x)])];
                uf[ix(x)] = g;
                x = g;
            }
            x
        }
        let mut unions: Vec<(u32, u32)> = Vec::new();
        for cz in 0..self.nz {
            for cx in 0..self.nx {
                if cx + 1 < self.nx {
                    for t in &self.vborder[self.vborder_index(cx, cz)] {
                        unions.push((self.comp_id(t.a), self.comp_id(t.b)));
                    }
                }
                if cz + 1 < self.nz {
                    for t in &self.hborder[self.hborder_index(cx, cz)] {
                        unions.push((self.comp_id(t.a), self.comp_id(t.b)));
                    }
                }
            }
        }
        for (a, b) in unions {
            let (ra, rb) = (find(&mut uf, a), find(&mut uf, b));
            if ra != rb {
                // Union by smaller root, so the labels are a function of the
                // terrain rather than of the insertion order.
                if ra < rb {
                    uf[ix(rb)] = ra;
                } else {
                    uf[ix(ra)] = rb;
                }
            }
        }
        self.comp_label.clear();
        self.comp_label.resize(total, 0);
        for i in 0..total {
            let r = find(&mut uf, id32(i));
            self.comp_label[i] = r;
        }
    }

    /// Global component id of a walkable cell.
    #[inline]
    #[must_use]
    pub fn comp_id(&self, cell: u32) -> u32 {
        let c = usize::from(self.of_node[ix(cell)]);
        self.comp_offset[c] + u32::from(self.local_comp[ix(cell)])
    }

    /// **The cheap no-path answer.** Two array reads and a comparison; exact.
    #[inline]
    #[must_use]
    pub fn connected(&self, world: &World, a: u32, b: u32) -> bool {
        if !world.walkable(a) || !world.walkable(b) {
            return false;
        }
        if self.local_comp[ix(a)] == NO_COMP || self.local_comp[ix(b)] == NO_COMP {
            return false;
        }
        self.comp_label[ix(self.comp_id(a))] == self.comp_label[ix(self.comp_id(b))]
    }

    /// The flattened union-find label of a walkable cell. Diagnostic.
    #[must_use]
    pub fn comp_label_of(&self, cell: u32) -> u32 {
        self.comp_label[ix(self.comp_id(cell))]
    }

    #[must_use]
    pub fn n_nodes(&self) -> usize {
        self.cells.len()
    }

    /// Directed edge count (each undirected edge appears twice).
    #[must_use]
    pub fn n_edges(&self) -> usize {
        self.edge_to.len()
    }

    /// Bytes the abstract graph occupies, excluding the per-node `of_node` and
    /// `local_comp` maps, which are reported separately by the bench.
    #[must_use]
    pub fn graph_bytes(&self) -> usize {
        self.cells.len() * 4
            + self.node_cluster.len() * 2
            + self.cluster_start.len() * 4
            + self.edge_start.len() * 4
            + self.edge_to.len() * 4
            + self.edge_cost.len() * 4
            + self.comp_offset.len() * 4
            + self.comp_label.len() * 4
            + self.trans.iter().map(|v| v.len() * 4).sum::<usize>()
            + self.intra.iter().map(|v| v.len() * 12).sum::<usize>()
            + self.vborder.iter().map(|v| v.len() * 12).sum::<usize>()
            + self.hborder.iter().map(|v| v.len() * 12).sum::<usize>()
    }

    /// Bytes of the per-grid-node maps this structure keeps.
    #[must_use]
    pub const fn node_map_bytes() -> usize {
        NODES * (2 + 2)
    }

    /// A fingerprint of the whole abstract graph, used by the
    /// repair-equivalence test.
    #[must_use]
    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u32> = Vec::with_capacity(self.cells.len() * 2 + self.edge_to.len() * 2);
        buf.extend_from_slice(&self.cells);
        buf.extend(self.node_cluster.iter().map(|c| u32::from(*c)));
        buf.extend_from_slice(&self.cluster_start);
        buf.extend_from_slice(&self.edge_start);
        buf.extend_from_slice(&self.edge_to);
        buf.extend(
            self.edge_cost
                .iter()
                .map(|c| u32::try_from(*c).expect("edge costs are positive and bounded")),
        );
        buf.extend_from_slice(&self.comp_label);
        crate::hash::fnv1a64_nodes(&buf)
    }
}

#[cfg(test)]
mod tests {
    use super::{Clusters, NO_COMP};
    use crate::astar::{Bound, Scratch, astar, dijkstra_all};
    use crate::hash::Rng;
    use crate::world::{World, node_of};

    #[test]
    fn entrance_runs_are_internally_connected() {
        let w = World::generate(20_260_913);
        let mut sc = Scratch::new();
        let cl = Clusters::build(&w, 32, &mut sc);
        let mut path = Vec::new();
        let mut checked = 0;
        for bi in 0..cl.vborder.len() {
            for t in &cl.vborder[bi] {
                // The two sides of a transition are a legal step apart, and each
                // is walkable.
                assert!(w.walkable(t.a) && w.walkable(t.b));
                assert_eq!(w.step_cost_between(t.a, t.b), Some(t.cost));
                checked += 1;
            }
        }
        assert!(checked > 50, "only {checked} vertical transitions");
        // And intra edges really are intra-cluster paths of the stated cost.
        for c in 0..cl.n_clusters() {
            for e in cl.intra[c].iter().take(3) {
                let cu = u16::try_from(c).expect("fits");
                let got = astar(
                    &w,
                    Bound::Cluster(&cl.of_node, cu),
                    &mut sc,
                    e.a,
                    e.b,
                    &mut path,
                );
                assert_eq!(got, Some(e.cost));
            }
        }
    }

    #[test]
    fn the_connectivity_oracle_agrees_with_dijkstra() {
        let w = World::generate(20_260_913);
        let mut sc = Scratch::new();
        let cl = Clusters::build(&w, 32, &mut sc);
        let mut rng = Rng::new(0x5EED_0003);
        let mut sources = 0;
        for _ in 0..5 {
            let src = loop {
                let id = node_of(rng.below(384), rng.below(384)).expect("in bounds");
                if w.walkable(id) {
                    break id;
                }
            };
            let dist = dijkstra_all(&w, &mut sc, src);
            for i in (0..crate::world::NODES).step_by(313) {
                let id = crate::id32(i);
                if !w.walkable(id) {
                    continue;
                }
                let truth = dist[i] != i32::MAX;
                assert_eq!(
                    cl.connected(&w, src, id),
                    truth,
                    "oracle disagrees with Dijkstra for node {id}"
                );
            }
            sources += 1;
        }
        assert_eq!(sources, 5);
    }

    #[test]
    fn both_intra_modes_produce_the_same_graph() {
        // The claim `IntraMode`'s doc comment makes, as a test rather than as an
        // argument.
        let w = World::generate(20_260_913);
        let mut sc = Scratch::new();
        for size in [16, 32, 64] {
            let a = Clusters::build_with(&w, size, &mut sc, super::IntraMode::PairwiseAstar);
            let b = Clusters::build_with(&w, size, &mut sc, super::IntraMode::PerNodeDijkstra);
            assert_eq!(a.fingerprint(), b.fingerprint(), "size {size}");
            assert_eq!(a.intra, b.intra, "size {size}");
        }
    }

    #[test]
    fn the_generator_leaves_one_dominant_component() {
        // Not a style point: a map whose walkable set is shattered would make
        // every accuracy number a measurement of unreachability instead of of
        // pathing. Ridges and rings are supposed to force detours through
        // chokepoints, not to seal the lobes off.
        let w = World::generate(20_260_913);
        let mut sc = Scratch::new();
        let cl = Clusters::build(&w, 32, &mut sc);
        let mut labels: Vec<u32> = Vec::new();
        for i in 0..crate::world::NODES {
            let id = crate::id32(i);
            if w.walkable(id) {
                labels.push(cl.comp_label_of(id));
            }
        }
        labels.sort_unstable();
        let mut best = 0usize;
        let mut run = 0usize;
        let mut prev = u32::MAX;
        for l in labels {
            if l == prev {
                run += 1;
            } else {
                best = best.max(run);
                run = 1;
                prev = l;
            }
        }
        best = best.max(run);
        let walkable = w.walkable_count();
        assert!(
            best * 100 / walkable >= 60,
            "largest component is only {best} of {walkable} walkable cells"
        );
    }

    #[test]
    fn every_walkable_cell_has_a_local_component() {
        let w = World::generate(3);
        let mut sc = Scratch::new();
        let cl = Clusters::build(&w, 16, &mut sc);
        for i in (0..crate::world::NODES).step_by(97) {
            let id = crate::id32(i);
            assert_eq!(w.walkable(id), cl.local_comp[i] != NO_COMP);
        }
    }
}
