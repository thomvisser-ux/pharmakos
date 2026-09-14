// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later
#![deny(clippy::float_arithmetic, clippy::as_conversions)]
//! Low-level A\* and exhaustive Dijkstra over the surface graph.
//!
//! Every design choice here comes straight out of the plan's §5 risk table:
//!
//! * **The open set is a hand-written binary heap** whose ordering is the
//!   *total* order `(f, h, node_id)` — [`Item::key`]. `std::collections::
//!   BinaryHeap` would have been fine for correctness and fatal for
//!   determinism: it gives no order among equal keys, and the order it does
//!   give depends on insertion history and on the standard library's own
//!   implementation, so two platforms can pop equal-cost nodes in different
//!   orders and return different, equally optimal paths. Paths are hashed sim
//!   state, so that breaks G4. Never compare on `f` alone.
//! * **No hash-keyed container anywhere** — the type the lint set bans, and the
//!   default habit in every A\* tutorial. The closed set and the g-scores are
//!   dense arrays indexed by node id, which is both the fastest option and
//!   inherently ordered. Sparse per-query state is **generation-stamped**: a `u32`
//!   generation counter per query, and a stamp array holding the generation a
//!   slot was last written in. Clearing is O(1) and order-free.
//! * **No recursion.** Path reconstruction walks a predecessor array into a
//!   `Vec`; the flood fills in [`crate::clusters`] use explicit work stacks.
//!   MSVC threads get ~1 MB of stack against Linux's ~8 MB, and a recursive
//!   flood fill over a 147 456-cell region overflows on Windows only.
//! * **Allocation-free in the steady state.** [`Scratch`] owns every array and
//!   is reused across queries; the bench counts allocations per query with a
//!   counting global allocator and expects zero once the pools are warm.
//!
//! Because the heuristic is consistent ([`crate::cost`]), a node's g-score is
//! final the first time it is popped, so the closed check is enough and there is
//! no decrease-key: stale heap entries are skipped on pop (lazy deletion).

use crate::cost::octile;
use crate::world::{DEFAULT_ORDER, NODES, World, coord_of};
use crate::{id32, ix};

/// Which nodes a search may touch. Bounded searches are how HPA\* keeps an
/// intra-cluster edge cheap, and how repair stays local.
#[derive(Clone, Copy, Debug)]
pub enum Bound<'a> {
    /// The whole map. Ground-truth searches only.
    Whole,
    /// One cluster of the decomposition.
    Cluster(&'a [u16], u16),
}

impl Bound<'_> {
    #[inline]
    #[must_use]
    fn admits(&self, id: u32) -> bool {
        match *self {
            Self::Whole => true,
            Self::Cluster(of_node, c) => of_node[ix(id)] == c,
        }
    }
}

/// A heap entry. `f` is `g + h`; `h` is kept so the tiebreak can use it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Item {
    pub(crate) f: i32,
    pub(crate) h: i32,
    pub(crate) n: u32,
}

impl Item {
    /// **The total order.** `(f, h, node_id)`, ascending in all three.
    ///
    /// Ascending `h` among equal `f` means preferring the node with the larger
    /// `g` — the one that is further along — which is the usual goal-ward
    /// tiebreak. `node_id` last makes the order total: no two distinct nodes can
    /// ever compare equal, so the pop sequence is a function of the graph and
    /// nothing else.
    #[inline]
    pub(crate) const fn key(self) -> (i32, i32, u32) {
        (self.f, self.h, self.n)
    }
}

/// A hand-written binary min-heap. Ours because the spike takes no
/// dependencies, and because the ordering had to be spelled out (above).
#[derive(Debug, Default)]
pub(crate) struct Heap {
    v: Vec<Item>,
}

impl Heap {
    pub(crate) fn clear(&mut self) {
        self.v.clear();
    }

    pub(crate) fn push(&mut self, it: Item) {
        self.v.push(it);
        let mut i = self.v.len() - 1;
        while i > 0 {
            let p = (i - 1) / 2;
            if self.v[i].key() < self.v[p].key() {
                self.v.swap(i, p);
                i = p;
            } else {
                break;
            }
        }
    }

    pub(crate) fn pop(&mut self) -> Option<Item> {
        if self.v.is_empty() {
            return None;
        }
        let last = self.v.len() - 1;
        self.v.swap(0, last);
        let out = self.v.pop();
        let n = self.v.len();
        let mut i = 0usize;
        loop {
            let l = 2 * i + 1;
            let r = l + 1;
            let mut best = i;
            if l < n && self.v[l].key() < self.v[best].key() {
                best = l;
            }
            if r < n && self.v[r].key() < self.v[best].key() {
                best = r;
            }
            if best == i {
                break;
            }
            self.v.swap(i, best);
            i = best;
        }
        out
    }
}

/// Per-query scratch: dense arrays plus the heap, reused across queries so the
/// steady state allocates nothing (plan §5 "allocator behaviour").
#[derive(Debug)]
pub struct Scratch {
    epoch: u32,
    stamp: Vec<u32>,
    closed: Vec<u32>,
    g: Vec<i32>,
    came: Vec<u32>,
    heap: Heap,
    /// Nodes popped since the last [`Scratch::reset_counters`]. Diagnostic.
    pub expansions: u64,
}

impl Default for Scratch {
    fn default() -> Self {
        Self::new()
    }
}

impl Scratch {
    #[must_use]
    pub fn new() -> Self {
        Self {
            epoch: 0,
            stamp: vec![0; NODES],
            closed: vec![0; NODES],
            g: vec![0; NODES],
            came: vec![0; NODES],
            heap: Heap::default(),
            expansions: 0,
        }
    }

    /// Bytes of dense scratch. Reported by the bench as "per-query scratch".
    #[must_use]
    pub const fn bytes() -> usize {
        NODES * (4 + 4 + 4 + 4)
    }

    pub fn reset_counters(&mut self) {
        self.expansions = 0;
    }

    /// Start a query. O(1) — the generation stamp invalidates every slot at
    /// once. The wrap branch is the only linear clear, once every 4.2 billion
    /// queries.
    fn begin(&mut self) {
        if self.epoch == u32::MAX {
            self.stamp.iter_mut().for_each(|s| *s = 0);
            self.closed.iter_mut().for_each(|s| *s = 0);
            self.epoch = 0;
        }
        self.epoch += 1;
        self.heap.clear();
    }

    #[inline]
    fn seen(&self, id: u32) -> bool {
        self.stamp[ix(id)] == self.epoch
    }

    #[inline]
    fn is_closed(&self, id: u32) -> bool {
        self.closed[ix(id)] == self.epoch
    }

    /// The g-score of `id` in the search that just ran, if it was reached.
    #[inline]
    #[must_use]
    pub fn g_of(&self, id: u32) -> Option<i32> {
        if self.stamp[ix(id)] == self.epoch {
            Some(self.g[ix(id)])
        } else {
            None
        }
    }

    /// Reconstruct the path into `out`, from the predecessor array. Iterative:
    /// plan §5 forbids recursion anywhere in this crate.
    fn reconstruct(&self, start: u32, goal: u32, out: &mut Vec<u32>) {
        out.clear();
        let mut cur = goal;
        loop {
            out.push(cur);
            if cur == start {
                break;
            }
            cur = self.came[ix(cur)];
        }
        out.reverse();
    }
}

/// A\* from `start` to `goal`, restricted by `bound`, expanding neighbours in
/// `order`. Returns the path cost and fills `out` with the node sequence.
///
/// `order` exists for one reason: the tiebreak-totality test permutes it and
/// demands a byte-identical path (plan §3 step 9, §5 "priority-queue
/// tiebreak"). Production callers use [`astar`].
pub fn astar_with_order(
    world: &World,
    bound: Bound<'_>,
    sc: &mut Scratch,
    start: u32,
    goal: u32,
    order: &[u8; 8],
    out: &mut Vec<u32>,
) -> Option<i32> {
    out.clear();
    if !world.walkable(start) || !world.walkable(goal) {
        return None;
    }
    if !bound.admits(start) || !bound.admits(goal) {
        return None;
    }
    sc.begin();
    let (gx, gz) = coord_of(goal);
    let (sx, sz) = coord_of(start);
    sc.stamp[ix(start)] = sc.epoch;
    sc.g[ix(start)] = 0;
    sc.came[ix(start)] = start;
    sc.heap.push(Item {
        f: octile(gx - sx, gz - sz),
        h: octile(gx - sx, gz - sz),
        n: start,
    });

    while let Some(it) = sc.heap.pop() {
        if sc.is_closed(it.n) {
            continue;
        }
        sc.closed[ix(it.n)] = sc.epoch;
        sc.expansions += 1;
        if it.n == goal {
            sc.reconstruct(start, goal, out);
            return Some(sc.g[ix(goal)]);
        }
        let gcur = sc.g[ix(it.n)];
        for k in order {
            let Some((nb, cost)) = world.neighbour(it.n, usize::from(*k)) else {
                continue;
            };
            if !bound.admits(nb) || sc.is_closed(nb) {
                continue;
            }
            let ng = gcur
                .checked_add(cost)
                .expect("path cost fits i32 (cost::MAX_PATH_COST)");
            if sc.seen(nb) && sc.g[ix(nb)] <= ng {
                continue;
            }
            sc.stamp[ix(nb)] = sc.epoch;
            sc.g[ix(nb)] = ng;
            sc.came[ix(nb)] = it.n;
            let (nx, nz) = coord_of(nb);
            let h = octile(gx - nx, gz - nz);
            sc.heap.push(Item {
                f: ng + h,
                h,
                n: nb,
            });
        }
    }
    None
}

/// A\* with the default expansion order.
pub fn astar(
    world: &World,
    bound: Bound<'_>,
    sc: &mut Scratch,
    start: u32,
    goal: u32,
    out: &mut Vec<u32>,
) -> Option<i32> {
    astar_with_order(world, bound, sc, start, goal, &DEFAULT_ORDER, out)
}

/// Dijkstra from `source`, bounded, exhausting everything reachable. The
/// distances stay in `sc` and are read back with [`Scratch::g_of`] — which is
/// how HPA\* inserts a temporary endpoint: one bounded sweep of the endpoint's
/// cluster gives the cost to every transition node of that cluster at once.
pub fn dijkstra_bounded(world: &World, bound: Bound<'_>, sc: &mut Scratch, source: u32) {
    sc.begin();
    if !world.walkable(source) || !bound.admits(source) {
        return;
    }
    sc.stamp[ix(source)] = sc.epoch;
    sc.g[ix(source)] = 0;
    sc.came[ix(source)] = source;
    sc.heap.push(Item {
        f: 0,
        h: 0,
        n: source,
    });
    while let Some(it) = sc.heap.pop() {
        if sc.is_closed(it.n) {
            continue;
        }
        sc.closed[ix(it.n)] = sc.epoch;
        sc.expansions += 1;
        let gcur = sc.g[ix(it.n)];
        for k in DEFAULT_ORDER {
            let Some((nb, cost)) = world.neighbour(it.n, usize::from(k)) else {
                continue;
            };
            if !bound.admits(nb) || sc.is_closed(nb) {
                continue;
            }
            let ng = gcur.checked_add(cost).expect("path cost fits i32");
            if sc.seen(nb) && sc.g[ix(nb)] <= ng {
                continue;
            }
            sc.stamp[ix(nb)] = sc.epoch;
            sc.g[ix(nb)] = ng;
            sc.came[ix(nb)] = it.n;
            sc.heap.push(Item { f: ng, h: 0, n: nb });
        }
    }
}

/// Exhaustive Dijkstra over the whole map, returned as a dense distance array
/// (`i32::MAX` where unreachable). This is the ground truth of plan §3 step 2;
/// it allocates, and it is never called from a measured path.
#[must_use]
pub fn dijkstra_all(world: &World, sc: &mut Scratch, source: u32) -> Vec<i32> {
    dijkstra_bounded(world, Bound::Whole, sc, source);
    let mut out = vec![i32::MAX; NODES];
    for (i, slot) in out.iter_mut().enumerate() {
        if let Some(g) = sc.g_of(id32(i)) {
            *slot = g;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Bound, Heap, Item, Scratch, astar, astar_with_order, dijkstra_all};
    use crate::cost::path_cost;
    use crate::hash::Rng;
    use crate::world::{DEFAULT_ORDER, World, node_of};

    #[test]
    fn the_heap_pops_in_the_total_order() {
        let mut h = Heap::default();
        // Equal f, equal h, different ids; and equal f, different h.
        let items = [
            Item { f: 10, h: 4, n: 7 },
            Item { f: 10, h: 4, n: 3 },
            Item { f: 10, h: 2, n: 9 },
            Item { f: 9, h: 9, n: 1 },
        ];
        for it in items {
            h.push(it);
        }
        let mut got = Vec::new();
        while let Some(it) = h.pop() {
            got.push(it.key());
        }
        assert_eq!(got, vec![(9, 9, 1), (10, 2, 9), (10, 4, 3), (10, 4, 7)]);
    }

    #[test]
    fn astar_equals_dijkstra_exactly() {
        let w = World::generate(20_260_913);
        let mut sc = Scratch::new();
        let mut out = Vec::new();
        let mut rng = Rng::new(0x5EED_0001);
        let mut checked = 0;
        for _ in 0..6 {
            let src = loop {
                let id = node_of(rng.below(384), rng.below(384)).expect("in bounds");
                if w.walkable(id) {
                    break id;
                }
            };
            let dist = dijkstra_all(&w, &mut sc, src);
            for _ in 0..40 {
                let dst = node_of(rng.below(384), rng.below(384)).expect("in bounds");
                if !w.walkable(dst) {
                    continue;
                }
                let a = astar(&w, Bound::Whole, &mut sc, src, dst, &mut out);
                let d = dist[crate::ix(dst)];
                if d == i32::MAX {
                    assert!(a.is_none(), "A* found a path Dijkstra says does not exist");
                } else {
                    assert_eq!(a, Some(d), "A* cost differs from Dijkstra");
                    assert_eq!(
                        path_cost(&w, &out),
                        Some(d),
                        "path does not cost what A* said"
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 50, "only {checked} reachable pairs were compared");
    }

    #[test]
    fn the_tiebreak_is_total_under_neighbour_permutation() {
        let w = World::generate(20_260_913);
        let mut sc = Scratch::new();
        let mut base = Vec::new();
        let mut other = Vec::new();
        let mut rng = Rng::new(0x5EED_0002);
        // A few fixed permutations of the expansion order. A non-total tiebreak
        // shows up here immediately: equal-f nodes would be popped in insertion
        // order, and insertion order is exactly what this permutes.
        let perms: [[u8; 8]; 4] = [
            DEFAULT_ORDER,
            [7, 6, 5, 4, 3, 2, 1, 0],
            [3, 1, 4, 0, 7, 2, 6, 5],
            [4, 2, 6, 1, 5, 0, 3, 7],
        ];
        let mut compared = 0;
        for _ in 0..40 {
            let a = node_of(rng.below(384), rng.below(384)).expect("in bounds");
            let b = node_of(rng.below(384), rng.below(384)).expect("in bounds");
            if !w.walkable(a) || !w.walkable(b) {
                continue;
            }
            let c0 = astar_with_order(&w, Bound::Whole, &mut sc, a, b, &perms[0], &mut base);
            for p in &perms[1..] {
                let c1 = astar_with_order(&w, Bound::Whole, &mut sc, a, b, p, &mut other);
                assert_eq!(c0, c1);
                assert_eq!(base, other, "permuting insertion order changed the path");
            }
            if c0.is_some() {
                compared += 1;
            }
        }
        assert!(compared > 5, "only {compared} pairs were reachable");
    }

    #[test]
    fn a_worst_case_single_region_map_does_not_recurse() {
        // Plan §5 "stack size": one flat region of 147 456 cells, flooded by an
        // exhaustive Dijkstra and then reconstructed corner to corner. If any
        // part of the search were recursive this would blow MSVC's ~1 MB stack.
        let w = World::flat(20);
        let mut sc = Scratch::new();
        let a = node_of(0, 0).expect("in bounds");
        let b = node_of(383, 383).expect("in bounds");
        let dist = dijkstra_all(&w, &mut sc, a);
        assert!(dist[crate::ix(b)] < i32::MAX);
        let mut out = Vec::new();
        let c = astar(&w, Bound::Whole, &mut sc, a, b, &mut out).expect("flat map is connected");
        assert_eq!(c, 383 * 14);
        assert_eq!(out.len(), 384);
        assert_eq!(path_cost(&w, &out), Some(c));
    }
}
