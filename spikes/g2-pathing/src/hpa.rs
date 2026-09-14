// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later
#![deny(clippy::float_arithmetic, clippy::as_conversions)]
//! Abstract search over the cluster graph, per-leg refinement, and smoothing.
//!
//! Plan §2: *"A query inserts start and goal as temporary nodes, searches the
//! abstract graph, then refines each leg with a cluster-local A\*."* And §2
//! again, for the estimator: *"the same machinery with the refinement skipped …
//! That is the whole reason the estimate can be ≤1 ms while a path is ≤5 ms."*
//! Both live here; [`crate::estimate`] is a thin, contractual wrapper around
//! [`abstract_search`].
//!
//! # Temporary insertion, and its clean-up
//!
//! The two endpoints are inserted as abstract nodes numbered `n` and `n + 1`,
//! where `n` is the base node count. They are held in [`HpaScratch`], not in
//! [`crate::clusters::Clusters`], so "cleaning up the temporary nodes" is the
//! absence of a mutation rather than an undo: the abstract graph is never
//! touched by a query, and two queries can therefore never interfere. The base
//! CSR stays immutable between repairs, which is also what makes the repair
//! equivalence test meaningful.
//!
//! Insertion is one **bounded Dijkstra per endpoint**, swept over the
//! endpoint's own cluster. One sweep of at most 4 096 cells yields the cost from
//! the endpoint to *every* transition node of that cluster at once — k costs for
//! the price of one search instead of k bounded A\*s — and, when both endpoints
//! share a cluster, it yields the direct start→goal cost for free as well. That
//! last point matters: without a direct edge, a short query inside one cluster
//! would be forced out through an entrance and back, and the estimate would be
//! wildly pessimistic exactly where the editor shows short routes.
//!
//! # One abstract level, and what a second would cost
//!
//! Plan §2 asks the spike to measure whether a second level earns its keep at
//! this map size rather than to assume it, and the code is shaped so that the
//! answer can be a measurement rather than an opinion. [`abstract_search`]
//! reaches the graph only through the CSR triple (`edge_start`, `edge_to`,
//! `edge_cost`) plus the two temporary-node edge lists; it never asks a
//! [`crate::clusters::Clusters`] anything about geometry. A second level would
//! therefore be a second `Clusters` over a coarser grid — the same entrance
//! logic applied to clusters of clusters — and a two-stage search that ran this
//! function twice, with nothing here changing shape.
//!
//! The evidence about whether that would pay is **the split of a query's work**,
//! which the bench reports as `estimate.abstract_expansions` and
//! `estimate.low_expansions` from [`HpaScratch::abs_expansions`] and
//! [`crate::astar::Scratch::expansions`]. A second level can only ever make the
//! abstract search cheaper; it cannot touch the two bounded endpoint-insertion
//! sweeps, which are cluster-local by construction. So the ceiling on what a
//! second level could save is the abstract share alone, and if that share is
//! small next to the insertion sweeps, a second level cannot pay however good it
//! is. The cluster sweep (16/32/64) is a *different* measurement — it trades a
//! smaller abstract graph against costlier refinement and a coarser estimate —
//! and it is reported separately.
//!
//! # Fog is priced per abstract-edge *endpoint*, not per unknown cell
//!
//! Plan §3 step 7 states the rule as *"the ×1.5 cost rule applied to unknown
//! cells"*. What [`abstract_search`] actually applies is coarser, and the
//! difference is worth stating plainly rather than leaving for a reader to
//! discover: `price` inflates an edge when **either of its two endpoint cells**
//! is unknown, and those two cells are transition cells on cluster borders. The
//! interior of an intra edge — up to a cluster's width, 32 cells — is never
//! inspected. So an edge running entirely through a fogged block between two
//! clear border cells is priced as clear, and an edge that merely touches fog at
//! one border is priced ×1.5 over its whole length.
//!
//! That is deliberate — the estimator may not walk an edge's cells to count how
//! many are unknown, because walking them *is* the refinement it is defined not
//! to do ([`crate::estimate`]'s module docs own the choice) — but it means the
//! fogged error figures recorded for plan §3 step 7 are the error of a
//! **per-edge-endpoint approximation**, and the literal per-cell rule is
//! unmeasured. A cheap honest middle ground for S3, if the approximation turns
//! out to matter: price by the fog state of the edge's midpoint cell, or of the
//! cluster the edge lies in, which costs one extra array read.
//!
//! # Smoothing
//!
//! Plan §2 wants smoothing that *"never produces an unwalkable step"*. The
//! smoother here can only ever produce legal steps **by construction**: a
//! shortcut is accepted only if a greedy octile walk from `i` to `j` is legal at
//! every step, and only if it costs no more than the sub-path it replaces. It
//! can therefore neither introduce an illegal step nor make a path worse; the
//! only thing it can do is fail to find a shortcut.

use crate::astar::{Bound, Heap, Item, Scratch, astar, dijkstra_bounded};
use crate::clusters::Clusters;
use crate::cost::{fog_price, octile, path_cost};
use crate::world::{World, coord_of};
use crate::{id32, ix};

/// How far ahead the smoother looks for a shortcut.
const LOOKAHEAD: usize = 16;

/// Scratch for a whole HPA\* query: the low-level scratch plus the abstract
/// search arrays and the temporary-node edge lists. Reused across queries.
#[derive(Debug)]
pub struct HpaScratch {
    pub low: Scratch,
    epoch: u32,
    stamp: Vec<u32>,
    closed: Vec<u32>,
    g: Vec<i32>,
    came: Vec<u32>,
    heap: Heap,
    /// Edges out of the temporary start node: `(abstract node, cost)`.
    start_edges: Vec<(u32, i32)>,
    /// Edges into the temporary goal node, sorted by abstract node so the
    /// expansion can binary-search them.
    goal_edges: Vec<(u32, i32)>,
    /// The abstract node sequence of the last search.
    pub abs_path: Vec<u32>,
    leg: Vec<u32>,
    raw: Vec<u32>,
    line: Vec<u32>,
    prefix: Vec<i32>,
    /// Abstract nodes popped by the last abstract search. Diagnostic.
    pub abs_expansions: u64,
}

impl Default for HpaScratch {
    fn default() -> Self {
        Self::new()
    }
}

impl HpaScratch {
    #[must_use]
    pub fn new() -> Self {
        Self {
            low: Scratch::new(),
            epoch: 0,
            stamp: Vec::new(),
            closed: Vec::new(),
            g: Vec::new(),
            came: Vec::new(),
            heap: Heap::default(),
            start_edges: Vec::new(),
            goal_edges: Vec::new(),
            abs_path: Vec::new(),
            leg: Vec::new(),
            raw: Vec::new(),
            line: Vec::new(),
            prefix: Vec::new(),
            abs_expansions: 0,
        }
    }

    /// Bytes of abstract scratch, for the memory line of the results table.
    #[must_use]
    pub fn abstract_bytes(&self) -> usize {
        (self.stamp.len() + self.closed.len() + self.g.len() + self.came.len()) * 4
    }

    /// Grow the generation-stamped arrays to `n_abs` slots.
    ///
    /// **The epoch counter is not reset here, and must never be.** `resize`
    /// preserves the existing prefix, so the slots already in the array still
    /// carry stamps from epochs `1..=self.epoch`; restarting the counter at 0
    /// would make [`begin`](Self::begin) hand out epoch 1 again and every one of
    /// those stale slots would read back as "seen in the current query" (with a
    /// stale g-score) or "closed" (skipped at pop). The abstract graph grows
    /// whenever destruction cuts a corridor into new transition nodes, so that
    /// mis-read would fire continuously and a query's answer would depend on how
    /// many queries preceded it — a path that is not a pure function of terrain
    /// and endpoints, which is exactly what plan §5 exists to rule out. A
    /// monotonically increasing epoch over a freshly zeroed tail is correct with
    /// no clearing at all; wrap-around is handled in `begin`.
    fn ensure(&mut self, n_abs: usize) {
        if self.stamp.len() < n_abs {
            self.stamp.resize(n_abs, 0);
            self.closed.resize(n_abs, 0);
            self.g.resize(n_abs, 0);
            self.came.resize(n_abs, 0);
        }
    }

    fn begin(&mut self) {
        if self.epoch == u32::MAX {
            self.stamp.iter_mut().for_each(|s| *s = 0);
            self.closed.iter_mut().for_each(|s| *s = 0);
            self.epoch = 0;
        }
        self.epoch += 1;
        self.heap.clear();
    }
}

/// Result of an abstract search: the summed abstract cost, and the abstract
/// node sequence in [`HpaScratch::abs_path`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbstractResult {
    pub cost: i32,
    pub legs: usize,
}

/// Insert one endpoint: a bounded Dijkstra over its cluster, harvested into
/// `out` as edges to that cluster's transition nodes. Returns the cost to
/// `other` when `other` lies in the same cluster and is reachable inside it.
fn insert_endpoint(
    world: &World,
    cl: &Clusters,
    low: &mut Scratch,
    cell: u32,
    other: u32,
    out: &mut Vec<(u32, i32)>,
) -> Option<i32> {
    out.clear();
    let c = usize::from(cl.of_node[ix(cell)]);
    dijkstra_bounded(
        world,
        Bound::Cluster(&cl.of_node, cl.of_node[ix(cell)]),
        low,
        cell,
    );
    let s = ix(cl.cluster_start[c]);
    let e = ix(cl.cluster_start[c + 1]);
    for (off, t) in cl.cells[s..e].iter().enumerate() {
        if let Some(g) = low.g_of(*t) {
            out.push((id32(s + off), g));
        }
    }
    if cl.of_node[ix(other)] == cl.of_node[ix(cell)] {
        low.g_of(other)
    } else {
        None
    }
}

/// Search the abstract graph from `start` to `goal`, with the two endpoints
/// inserted temporarily. `fog`, when given, prices an edge touching an unknown
/// cell at ×1.5 ([`crate::cost::fog_price`]).
///
/// This is the **whole** of the P3 estimate query: no refinement, no stepping.
pub fn abstract_search(
    world: &World,
    cl: &Clusters,
    hs: &mut HpaScratch,
    start: u32,
    goal: u32,
    fog: Option<&[bool]>,
) -> Option<AbstractResult> {
    hs.abs_path.clear();
    if !world.walkable(start) || !world.walkable(goal) {
        return None;
    }
    if start == goal {
        hs.abs_path.push(0);
        return Some(AbstractResult { cost: 0, legs: 0 });
    }
    // The cheap no-path answer (plan §3 step 4): a sealed-in walker costs two
    // array reads, not an exhausted search.
    if !cl.connected(world, start, goal) {
        return None;
    }

    let n = cl.n_nodes();
    let temp_start = id32(n);
    let temp_goal = id32(n + 1);
    hs.ensure(n + 2);

    let mut start_edges = core::mem::take(&mut hs.start_edges);
    let mut goal_edges = core::mem::take(&mut hs.goal_edges);
    let direct = insert_endpoint(world, cl, &mut hs.low, start, goal, &mut start_edges);
    insert_endpoint(world, cl, &mut hs.low, goal, start, &mut goal_edges);
    goal_edges.sort_unstable();
    if let Some(d) = direct {
        start_edges.push((temp_goal, d));
    }
    start_edges.sort_unstable();
    hs.start_edges = start_edges;
    hs.goal_edges = goal_edges;

    let price = |c: i32, a_cell: u32, b_cell: u32| -> i32 {
        match fog {
            None => c,
            Some(mask) => {
                if mask[ix(a_cell)] || mask[ix(b_cell)] {
                    fog_price(c)
                } else {
                    c
                }
            }
        }
    };
    let cell_of = |node: u32| -> u32 {
        if node == temp_start {
            start
        } else if node == temp_goal {
            goal
        } else {
            cl.cells[ix(node)]
        }
    };
    let (gx, gz) = coord_of(goal);
    let h_of = |node: u32| -> i32 {
        let (x, z) = coord_of(cell_of(node));
        octile(gx - x, gz - z)
    };

    hs.begin();
    hs.abs_expansions = 0;
    hs.stamp[ix(temp_start)] = hs.epoch;
    hs.g[ix(temp_start)] = 0;
    hs.came[ix(temp_start)] = temp_start;
    let h0 = h_of(temp_start);
    hs.heap.push(Item {
        f: h0,
        h: h0,
        n: temp_start,
    });

    let mut found = false;
    while let Some(it) = hs.heap.pop() {
        if hs.closed[ix(it.n)] == hs.epoch {
            continue;
        }
        hs.closed[ix(it.n)] = hs.epoch;
        hs.abs_expansions += 1;
        if it.n == temp_goal {
            found = true;
            break;
        }
        let gcur = hs.g[ix(it.n)];
        let from_cell = cell_of(it.n);
        // Neighbours: the temporary start has its own list; a base node has its
        // CSR row plus, if it is one of the goal's insertion nodes, the edge to
        // the temporary goal.
        if it.n == temp_start {
            for k in 0..hs.start_edges.len() {
                let (nb, c) = hs.start_edges[k];
                relax(hs, nb, gcur + price(c, from_cell, cell_of(nb)), it.n, &h_of);
            }
        } else {
            let s = ix(cl.edge_start[ix(it.n)]);
            let e = ix(cl.edge_start[ix(it.n) + 1]);
            for p in s..e {
                let nb = cl.edge_to[p];
                let c = cl.edge_cost[p];
                relax(
                    hs,
                    nb,
                    gcur + price(c, from_cell, cl.cells[ix(nb)]),
                    it.n,
                    &h_of,
                );
            }
            if let Ok(k) = hs.goal_edges.binary_search_by_key(&it.n, |(a, _)| *a) {
                let c = hs.goal_edges[k].1;
                relax(hs, temp_goal, gcur + price(c, from_cell, goal), it.n, &h_of);
            }
        }
    }
    if !found {
        return None;
    }

    // Reconstruct, iteratively (plan §5: no recursion).
    let cost = hs.g[ix(temp_goal)];
    let mut cur = temp_goal;
    hs.abs_path.clear();
    loop {
        hs.abs_path.push(cur);
        if cur == temp_start {
            break;
        }
        cur = hs.came[ix(cur)];
    }
    hs.abs_path.reverse();
    let legs = hs.abs_path.len() - 1;
    Some(AbstractResult { cost, legs })
}

#[inline]
fn relax<F: Fn(u32) -> i32>(hs: &mut HpaScratch, nb: u32, ng: i32, from: u32, h_of: &F) {
    if hs.closed[ix(nb)] == hs.epoch {
        return;
    }
    if hs.stamp[ix(nb)] == hs.epoch && hs.g[ix(nb)] <= ng {
        return;
    }
    hs.stamp[ix(nb)] = hs.epoch;
    hs.g[ix(nb)] = ng;
    hs.came[ix(nb)] = from;
    let h = h_of(nb);
    hs.heap.push(Item {
        f: ng + h,
        h,
        n: nb,
    });
}

/// A full HPA\* path: abstract search, per-leg refinement, then smoothing.
/// Returns the cost of the path it wrote into `out`.
pub fn path(
    world: &World,
    cl: &Clusters,
    hs: &mut HpaScratch,
    start: u32,
    goal: u32,
    out: &mut Vec<u32>,
) -> Option<i32> {
    out.clear();
    let res = abstract_search(world, cl, hs, start, goal, None)?;
    if res.legs == 0 {
        out.push(start);
        return Some(0);
    }
    let n = cl.n_nodes();
    let temp_start = id32(n);
    let temp_goal = id32(n + 1);
    let cell_of = |node: u32| -> u32 {
        if node == temp_start {
            start
        } else if node == temp_goal {
            goal
        } else {
            cl.cells[ix(node)]
        }
    };

    let abs: Vec<u32> = core::mem::take(&mut hs.abs_path);
    let mut raw = core::mem::take(&mut hs.raw);
    let mut leg = core::mem::take(&mut hs.leg);
    raw.clear();
    raw.push(cell_of(abs[0]));
    let mut ok = true;
    for pair in abs.windows(2) {
        let (a, b) = (cell_of(pair[0]), cell_of(pair[1]));
        let ca = cl.of_node[ix(a)];
        let cb = cl.of_node[ix(b)];
        if ca == cb {
            if astar(
                world,
                Bound::Cluster(&cl.of_node, ca),
                &mut hs.low,
                a,
                b,
                &mut leg,
            )
            .is_none()
            {
                ok = false;
                break;
            }
            raw.extend_from_slice(&leg[1..]);
        } else {
            // An inter-cluster abstract edge is exactly one grid step.
            if world.step_cost_between(a, b).is_none() {
                ok = false;
                break;
            }
            raw.push(b);
        }
    }
    hs.abs_path = abs;
    hs.leg = leg;
    if !ok {
        hs.raw = raw;
        return None;
    }
    let mut line = core::mem::take(&mut hs.line);
    let mut prefix = core::mem::take(&mut hs.prefix);
    smooth_into(world, &raw, out, &mut line, &mut prefix);
    hs.raw = raw;
    hs.line = line;
    hs.prefix = prefix;
    path_cost(world, out)
}

/// The greedy octile walk from `a` to `b`: diagonal while both axes need
/// covering, then straight. Deterministic, integer, and validated step by step,
/// so it can only ever return a legal walk.
fn straight_walk(world: &World, a: u32, b: u32, out: &mut Vec<u32>) -> Option<i32> {
    out.clear();
    out.push(a);
    let (bx, bz) = coord_of(b);
    let mut cur = a;
    let mut total = 0;
    loop {
        let (x, z) = coord_of(cur);
        if x == bx && z == bz {
            return Some(total);
        }
        let nx = x + (bx - x).signum();
        let nz = z + (bz - z).signum();
        let next = crate::world::node_of(nx, nz)?;
        total += world.step_cost_between(cur, next)?;
        out.push(next);
        cur = next;
    }
}

/// Smooth `path` into `out`. See the module docs: legal by construction, and
/// never more expensive than the input.
///
/// An input that is not itself a legal walk is **refused**, not smoothed: `out`
/// is left empty and the function returns. The earlier formulation priced an
/// illegal step at `i32::MAX / 4` and summed it into the prefix array, which
/// with `overflow-checks = true` in every profile panics on the fourth such
/// step. Every caller inside the spike passes a freshly refined path, so that
/// was unreachable today — but `smooth_into` is public and the obvious next use
/// (smoothing a stale route after a terrain edit) lands straight on it, and a
/// refused shortcut is a better failure mode than an arithmetic panic.
pub fn smooth_into(
    world: &World,
    path: &[u32],
    out: &mut Vec<u32>,
    line: &mut Vec<u32>,
    prefix: &mut Vec<i32>,
) {
    out.clear();
    if path.is_empty() {
        return;
    }
    prefix.clear();
    prefix.push(0);
    let mut acc = 0;
    for pair in path.windows(2) {
        let Some(c) = world.step_cost_between(pair[0], pair[1]) else {
            out.clear();
            return;
        };
        acc += c;
        prefix.push(acc);
    }
    out.push(path[0]);
    let mut i = 0usize;
    while i + 1 < path.len() {
        let limit = (i + LOOKAHEAD).min(path.len() - 1);
        let mut chosen = i + 1;
        // Try the furthest shortcut first and stop at the first that is both
        // legal and no more expensive.
        let mut j = limit;
        while j > i + 1 {
            if let Some(c) = straight_walk(world, path[i], path[j], line)
                && c <= prefix[j] - prefix[i]
            {
                out.extend_from_slice(&line[1..]);
                chosen = j;
                break;
            }
            j -= 1;
        }
        if chosen == i + 1 {
            out.push(path[i + 1]);
        }
        i = chosen;
    }
}

#[cfg(test)]
mod tests {
    use super::{HpaScratch, abstract_search, path, smooth_into};
    use crate::astar::{Bound, Scratch, astar};
    use crate::clusters::Clusters;
    use crate::cost::path_cost;
    use crate::hash::Rng;
    use crate::world::{World, node_of};

    fn fixture(size: i32) -> (World, Clusters, Scratch) {
        let w = World::generate(20_260_913);
        let mut sc = Scratch::new();
        let cl = Clusters::build(&w, size, &mut sc);
        (w, cl, sc)
    }

    #[test]
    fn the_abstract_cost_is_the_cost_of_the_unsmoothed_refinement() {
        // This is the identity the estimator rests on: summing abstract edge
        // costs is not an approximation of the refined path, it IS its cost.
        // Everything the estimate is wrong by afterwards is either HPA*'s excess
        // over optimal or the smoother's gain — both measured by `accuracy`.
        let (w, cl, _sc) = fixture(32);
        let mut hs = HpaScratch::new();
        let mut out = Vec::new();
        let mut rng = Rng::new(0x5EED_0010);
        let mut checked = 0;
        for _ in 0..120 {
            let a = node_of(rng.below(384), rng.below(384)).expect("in bounds");
            let b = node_of(rng.below(384), rng.below(384)).expect("in bounds");
            if !w.walkable(a) || !w.walkable(b) {
                continue;
            }
            let Some(res) = abstract_search(&w, &cl, &mut hs, a, b, None) else {
                continue;
            };
            let c = path(&w, &cl, &mut hs, a, b, &mut out).expect("abstract found one");
            assert!(c <= res.cost, "smoothing made the path more expensive");
            assert_eq!(path_cost(&w, &out), Some(c));
            assert_eq!(out.first(), Some(&a));
            assert_eq!(out.last(), Some(&b));
            checked += 1;
        }
        assert!(checked > 20, "only {checked} reachable pairs");
    }

    #[test]
    fn a_reused_scratch_answers_exactly_like_a_fresh_one_as_the_graph_grows() {
        // The regression test for `HpaScratch::ensure`. A query's answer must be
        // a pure function of (terrain, start, goal) — never of how many queries
        // the scratch has served since the abstract graph last grew. Growth is
        // the interesting case, because that is when the stamp arrays gain slots
        // while the old ones keep their stamps; an epoch reset there made stale
        // slots read back as "seen" or "closed" and silently dropped nodes from
        // the search.
        use crate::repair::{Blast, carve, repair, surface_crater};

        let (mut w, mut cl, mut sc) = fixture(32);
        let mut hs = HpaScratch::new();
        let mut rng = Rng::new(0x5EED_0014);

        // Six probe pairs, and a warm-up so the reused scratch carries live
        // epochs before the graph is ever rebuilt.
        let mut probes: Vec<(u32, u32)> = Vec::new();
        while probes.len() < 6 {
            let a = node_of(rng.below(384), rng.below(384)).expect("in bounds");
            let b = node_of(rng.below(384), rng.below(384)).expect("in bounds");
            if a != b && w.walkable(a) && w.walkable(b) && cl.connected(&w, a, b) {
                probes.push((a, b));
            }
        }
        for (a, b) in &probes {
            let _ = abstract_search(&w, &cl, &mut hs, *a, *b, None);
        }

        let base_nodes = cl.n_nodes();
        let mut blast = Blast::default();
        let mut grew = false;
        let mut rounds = 0u32;
        let mut diffs: Vec<String> = Vec::new();
        let mut last_nodes = base_nodes;
        for k in 0..300u64 {
            let crater = surface_crater(&w, 40 + rng.below(300), 40 + rng.below(300), k, 3);
            carve(&mut w, &cl, &crater, &mut blast);
            if blast.dirty.is_empty() {
                continue;
            }
            let dirty = blast.dirty.clone();
            repair(&w, &mut cl, &mut sc, &dirty);
            if cl.n_nodes() > base_nodes {
                grew = true;
            }
            if cl.n_nodes() == last_nodes || rounds >= 24 {
                continue;
            }
            last_nodes = cl.n_nodes();
            rounds += 1;
            for (a, b) in &probes {
                let reused = abstract_search(&w, &cl, &mut hs, *a, *b, None)
                    .map(|r| (r.cost, hs.abs_path.clone()));
                let mut fresh = HpaScratch::new();
                let want = abstract_search(&w, &cl, &mut fresh, *a, *b, None)
                    .map(|r| (r.cost, fresh.abs_path.clone()));
                if reused != want {
                    diffs.push(format!(
                        "crater {k}: reused={:?} fresh={:?}",
                        reused.map(|r| r.0),
                        want.map(|r| r.0)
                    ));
                }
            }
        }
        assert!(
            grew,
            "the abstract graph never grew past {base_nodes} nodes, so this test checked nothing"
        );
        assert!(rounds >= 8, "only {rounds} probe rounds");
        assert!(
            diffs.is_empty(),
            "a reused scratch disagreed with a fresh one: {diffs:?}"
        );
    }

    #[test]
    fn hpa_is_never_cheaper_than_optimal() {
        let (w, cl, mut sc) = fixture(32);
        let mut hs = HpaScratch::new();
        let mut out = Vec::new();
        let mut opt = Vec::new();
        let mut rng = Rng::new(0x5EED_0011);
        let mut checked = 0;
        for _ in 0..80 {
            let a = node_of(rng.below(384), rng.below(384)).expect("in bounds");
            let b = node_of(rng.below(384), rng.below(384)).expect("in bounds");
            if !w.walkable(a) || !w.walkable(b) {
                continue;
            }
            let got = path(&w, &cl, &mut hs, a, b, &mut out);
            let want = astar(&w, Bound::Whole, &mut sc, a, b, &mut opt);
            assert_eq!(got.is_some(), want.is_some(), "reachability disagreement");
            if let (Some(g), Some(o)) = (got, want) {
                assert!(g >= o, "HPA* beat the optimal path, which is impossible");
                checked += 1;
            }
        }
        assert!(checked > 10, "only {checked} reachable pairs");
    }

    #[test]
    fn the_integrator_agrees_with_the_rounding_rule() {
        // `walk_ticks` steps a unit tick by tick; `ticks_for_cost` divides. They
        // must be the same number on every real path, or the estimator's licence
        // to divide instead of walk is void.
        use crate::cost::{path_cost, ticks_for_cost, walk_ticks};
        let (w, cl, _sc) = fixture(32);
        let mut hs = HpaScratch::new();
        let mut out = Vec::new();
        let mut rng = Rng::new(0x5EED_0013);
        let mut checked = 0;
        for _ in 0..80 {
            let a = node_of(rng.below(384), rng.below(384)).expect("in bounds");
            let b = node_of(rng.below(384), rng.below(384)).expect("in bounds");
            if !w.walkable(a) || !w.walkable(b) {
                continue;
            }
            let Some(cost) = path(&w, &cl, &mut hs, a, b, &mut out) else {
                continue;
            };
            assert_eq!(path_cost(&w, &out), Some(cost));
            assert_eq!(walk_ticks(&w, &out), Some(ticks_for_cost(cost)));
            checked += 1;
        }
        assert!(checked > 10, "only {checked} reachable pairs");
    }

    #[test]
    fn smoothing_never_yields_an_unwalkable_step() {
        let (w, _cl, mut sc) = fixture(32);
        let mut hs = HpaScratch::new();
        let mut raw = Vec::new();
        let mut out = Vec::new();
        let mut line = Vec::new();
        let mut prefix = Vec::new();
        let _ = &mut hs;
        let mut rng = Rng::new(0x5EED_0012);
        let mut checked = 0;
        for _ in 0..60 {
            let a = node_of(rng.below(384), rng.below(384)).expect("in bounds");
            let b = node_of(rng.below(384), rng.below(384)).expect("in bounds");
            if !w.walkable(a) || !w.walkable(b) {
                continue;
            }
            let Some(before) = astar(&w, Bound::Whole, &mut sc, a, b, &mut raw) else {
                continue;
            };
            smooth_into(&w, &raw, &mut out, &mut line, &mut prefix);
            // Every step legal…
            let after = path_cost(&w, &out).expect("smoothed path is walkable");
            // …and never worse than what it replaced.
            assert!(after <= before);
            assert_eq!(out.first(), Some(&a));
            assert_eq!(out.last(), Some(&b));
            checked += 1;
        }
        assert!(checked > 10, "only {checked} reachable pairs");
    }
}
