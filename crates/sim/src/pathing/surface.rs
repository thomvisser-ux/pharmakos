// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The search graph: one node per voxel column, over the chunk store.
//!
//! # The graph is a surface, not a volume
//!
//! [`crate::mapgen`] produces an integer heightmap closed to a 1-Lipschitz
//! field, so every column has exactly one surface and neither overhangs nor
//! bridges exist. One node per column is therefore lossless as well as cheap:
//! 384 × 384 = 147 456 nodes rather than 9.4 million voxels, and the node id
//! **is** the column index `x + y * size_x` as a `u32` (never a `usize`, so
//! target pointer width cannot reach anything hashed).
//!
//! A crater can turn a column to air. [`Surface::top`] is then `-1` and the
//! column is not walkable — which is the honest answer, and the state that
//! seals a walker in (item 60).
//!
//! # The cost model is the rules table's
//!
//! `locomotion.step_cost_cardinal` = 10, `step_cost_diagonal` = 14 and
//! `climb_surcharge` = 4, read once into [`StepCosts`] and never written down
//! here (item 59; AGENTS.md §12). 10/14 is the integer octile approximation, so
//! there is no square root and therefore no float anywhere in the search, and
//! the heuristic in [`crate::pathing::search`] stays admissible and consistent
//! under it.
//!
//! The surcharge is charged for a one-voxel step **up or down**, which keeps
//! the edge relation symmetric: a route and its reverse cost the same, which is
//! what the repair tests and the reversed-route assertions rest on.

use crate::pathing::HEADROOM_VOXELS;
use crate::voxels::{CHUNK_EDGE, VoxelStore};

/// A node of the search graph: one voxel column, `x + y * size_x`.
pub type Node = u32;

/// The eight neighbour offsets, in a fixed, documented order.
///
/// Expansion order is part of the determinism contract: two machines must
/// expand a node's neighbours in the same order. The order is only *visible*
/// through the tiebreak, which item 62 makes total, and
/// `the_tiebreak_survives_a_neighbour_permutation` in `tests/pathing.rs`
/// permutes this array and demands an identical route.
pub const NEIGHBOURS: [(i32, i32); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// The default expansion order: the identity over [`NEIGHBOURS`].
pub const DEFAULT_ORDER: [u8; 8] = [0, 1, 2, 3, 4, 5, 6, 7];

/// The integer cost model, read from the rules table (item 59).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StepCosts {
    cardinal: i32,
    diagonal: i32,
    climb: i32,
}

impl StepCosts {
    /// Take the three rows the search reads.
    #[must_use]
    pub const fn new(cardinal: i32, diagonal: i32, climb: i32) -> StepCosts {
        StepCosts {
            cardinal,
            diagonal,
            climb,
        }
    }

    /// The three rows of a rules table, in one call.
    #[must_use]
    pub const fn from_rules(rules: &crate::rules::RulesTable) -> StepCosts {
        StepCosts::new(
            rules.step_cardinal(),
            rules.step_diagonal(),
            rules.climb_surcharge(),
        )
    }

    /// Cost of a cardinal step on level ground.
    #[must_use]
    pub const fn cardinal(self) -> i32 {
        self.cardinal
    }

    /// Cost of a diagonal step on level ground.
    #[must_use]
    pub const fn diagonal(self) -> i32 {
        self.diagonal
    }

    /// The one-voxel climb surcharge.
    #[must_use]
    pub const fn climb(self) -> i32 {
        self.climb
    }

    /// The most one step can cost: a diagonal with a climb.
    #[must_use]
    pub const fn max_step(self) -> i32 {
        self.diagonal.saturating_add(self.climb)
    }

    /// The octile heuristic over a horizontal delta.
    ///
    /// `14 * min + 10 * (max - min)`, computed from the footprint only: it
    /// never looks at height. That is what keeps it **admissible** — every real
    /// step costs its base rate plus a non-negative surcharge, so charging base
    /// rates alone is a lower bound — and **consistent**, so a node's g-score is
    /// final the first time it is popped and the search needs no decrease-key.
    #[must_use]
    pub const fn octile(self, dx: i32, dy: i32) -> i32 {
        let ax = dx.abs();
        let ay = dy.abs();
        let (lo, hi) = if ax < ay { (ax, ay) } else { (ay, ax) };
        self.diagonal
            .saturating_mul(lo)
            .saturating_add(self.cardinal.saturating_mul(hi.saturating_sub(lo)))
    }
}

/// The surface graph over a chunk store.
///
/// Derived state: a pure function of the store and the cost rows, rebuilt from
/// both on a restore rather than carried in a snapshot.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Surface {
    size: [i32; 3],
    costs: StepCosts,
    /// Top solid `z` per column, `-1` where the column is air all the way down.
    top: Vec<i16>,
    /// Whether the column can be stood on.
    walk: Vec<bool>,
}

impl Surface {
    /// Build the graph from a store.
    ///
    /// `None` when the store's extent does not fit the `i32` coordinates the
    /// search uses, which no rules table this sim accepts can produce.
    #[must_use]
    pub fn new(store: &VoxelStore, costs: StepCosts) -> Option<Surface> {
        let size = store.size();
        let sx = i32::try_from(*size.first()?).ok()?;
        let sy = i32::try_from(*size.get(1)?).ok()?;
        let sz = i32::try_from(*size.get(2)?).ok()?;
        let count = usize::try_from(sx.checked_mul(sy)?).ok()?;
        let mut surface = Surface {
            size: [sx, sy, sz],
            costs,
            top: vec![-1; count],
            walk: vec![false; count],
        };
        surface.refresh_all(store);
        Some(surface)
    }

    /// The map extent in voxels, `[x, y, z]`.
    #[must_use]
    pub const fn size(&self) -> [i32; 3] {
        self.size
    }

    /// The cost model this graph prices steps at.
    #[must_use]
    pub const fn costs(&self) -> StepCosts {
        self.costs
    }

    /// How many nodes the graph holds: one per column.
    #[must_use]
    pub fn node_count(&self) -> u32 {
        u32::try_from(self.top.len()).unwrap_or(0)
    }

    /// Column coordinates to a node id, or `None` off the map.
    #[must_use]
    pub fn node_of(&self, x: i32, y: i32) -> Option<Node> {
        let sx = *self.size.first()?;
        let sy = *self.size.get(1)?;
        if x < 0 || y < 0 || x >= sx || y >= sy {
            return None;
        }
        u32::try_from(y.checked_mul(sx)?.checked_add(x)?).ok()
    }

    /// A node id back to column coordinates.
    ///
    /// Returns `(0, 0)` for an id off the map, which cannot arise from
    /// [`Surface::node_of`] and is a caller bug rather than a game state.
    #[must_use]
    pub fn coord_of(&self, node: Node) -> (i32, i32) {
        let sx = self.size.first().copied().unwrap_or(1).max(1);
        let Ok(linear) = i32::try_from(node) else {
            return (0, 0);
        };
        (linear.rem_euclid(sx), linear.div_euclid(sx))
    }

    /// The top solid `z` of a column, or `-1` when it has no floor.
    #[must_use]
    pub fn top(&self, node: Node) -> i32 {
        let Ok(index) = usize::try_from(node) else {
            return -1;
        };
        i32::from(self.top.get(index).copied().unwrap_or(-1))
    }

    /// Whether a column can be stood on.
    #[must_use]
    pub fn walkable(&self, node: Node) -> bool {
        let Ok(index) = usize::try_from(node) else {
            return false;
        };
        self.walk.get(index).copied().unwrap_or(false)
    }

    /// The `z` a unit standing on this column occupies.
    #[must_use]
    pub fn standing_z(&self, node: Node) -> i32 {
        self.top(node).saturating_add(1).max(0)
    }

    /// Recompute every column. Construction and restore only.
    pub fn refresh_all(&mut self, store: &VoxelStore) {
        let sx = self.size.first().copied().unwrap_or(0);
        let sy = self.size.get(1).copied().unwrap_or(0);
        let mut y = 0;
        while y < sy {
            let mut x = 0;
            while x < sx {
                self.refresh_column(store, x, y);
                x = x.saturating_add(1);
            }
            y = y.saturating_add(1);
        }
    }

    /// Recompute the columns a chunk's footprint covers.
    ///
    /// Called from the voxel phase with the chunks
    /// [`VoxelStore::settle`](crate::voxels::VoxelStore::settle) just settled,
    /// so the tops never lag the bytes: a walker's next step is validated
    /// against the terrain as it is, and the *cluster* repair that follows a
    /// tick later (the phase order puts `Pathing` before `Voxels`) only ever
    /// changes which routes exist, never whether a step is legal.
    pub fn refresh_chunk(&mut self, store: &VoxelStore, chunk: u32) {
        let Some(origin) = store.chunk_origin(chunk) else {
            return;
        };
        let x0 = origin.first().copied().unwrap_or(0);
        let y0 = origin.get(1).copied().unwrap_or(0);
        let edge = i32::try_from(CHUNK_EDGE).unwrap_or(32);
        let mut dy = 0;
        while dy < edge {
            let mut dx = 0;
            while dx < edge {
                self.refresh_column(store, x0.saturating_add(dx), y0.saturating_add(dy));
                dx = dx.saturating_add(1);
            }
            dy = dy.saturating_add(1);
        }
    }

    /// Recompute one column's top and walkability from the store.
    fn refresh_column(&mut self, store: &VoxelStore, x: i32, y: i32) {
        let Some(node) = self.node_of(x, y) else {
            return;
        };
        let Ok(index) = usize::try_from(node) else {
            return;
        };
        let top = store.top_solid_z(x, y).unwrap_or(-1);
        let sz = self.size.get(2).copied().unwrap_or(0);
        let walkable = top >= 0 && top.saturating_add(HEADROOM_VOXELS) < sz;
        if let Some(slot) = self.top.get_mut(index) {
            *slot = i16::try_from(top).unwrap_or(-1);
        }
        if let Some(slot) = self.walk.get_mut(index) {
            *slot = walkable;
        }
    }

    /// The `k`-th neighbour of `node`, with the cost of stepping to it, or
    /// `None` when that step is illegal.
    ///
    /// Legality, in full:
    ///
    /// * both columns walkable;
    /// * `|delta top| <= 1` — the one-voxel climb of spec section 9;
    /// * for a diagonal, both orthogonal companions walkable and within one
    ///   voxel of **both** endpoints. Checking against both endpoints rather
    ///   than only the origin is what keeps the edge relation symmetric, which
    ///   the connectivity oracle and the repair equivalence both rely on.
    #[must_use]
    pub fn neighbour(&self, node: Node, k: usize) -> Option<(Node, i32)> {
        let (dx, dy) = NEIGHBOURS.get(k).copied()?;
        let (x, y) = self.coord_of(node);
        let other = self.node_of(x.checked_add(dx)?, y.checked_add(dy)?)?;
        if !self.walkable(node) || !self.walkable(other) {
            return None;
        }
        let from = self.top(node);
        let to = self.top(other);
        if to.saturating_sub(from).abs() > 1 {
            return None;
        }
        let climb = if to == from { 0 } else { self.costs.climb() };
        if dx != 0 && dy != 0 {
            let side_x = self.node_of(x.checked_add(dx)?, y)?;
            let side_y = self.node_of(x, y.checked_add(dy)?)?;
            if !self.walkable(side_x) || !self.walkable(side_y) {
                return None;
            }
            let tx = self.top(side_x);
            let ty = self.top(side_y);
            if tx.saturating_sub(from).abs() > 1
                || ty.saturating_sub(from).abs() > 1
                || tx.saturating_sub(to).abs() > 1
                || ty.saturating_sub(to).abs() > 1
            {
                return None;
            }
            return Some((other, self.costs.diagonal().saturating_add(climb)));
        }
        Some((other, self.costs.cardinal().saturating_add(climb)))
    }

    /// The cost of the step between two adjacent columns, or `None` when it is
    /// not a legal step.
    #[must_use]
    pub fn step_cost_between(&self, from: Node, to: Node) -> Option<i32> {
        let mut k = 0;
        while k < NEIGHBOURS.len() {
            if let Some((other, cost)) = self.neighbour(from, k)
                && other == to
            {
                return Some(cost);
            }
            k = k.saturating_add(1);
        }
        None
    }

    /// The heuristic between two nodes: octile over the footprint.
    #[must_use]
    pub fn heuristic(&self, from: Node, to: Node) -> i32 {
        let (fx, fy) = self.coord_of(from);
        let (tx, ty) = self.coord_of(to);
        self.costs
            .octile(tx.saturating_sub(fx), ty.saturating_sub(fy))
    }

    /// How many walkable columns the graph holds. Diagnostic; never in a tick.
    #[must_use]
    pub fn walkable_count(&self) -> u32 {
        u32::try_from(self.walk.iter().filter(|w| **w).count()).unwrap_or(u32::MAX)
    }

    /// How many **directed steps** the graph holds.
    ///
    /// G2 §9.9's lesson, and the reason `tests/pathing.rs` reports this rather
    /// than a cell count: on a 1-Lipschitz map every column is walkable by
    /// construction, so a walkable-column count measures the clamp and not the
    /// terrain. What destruction actually removes is steps, and what a walker
    /// is sealed in by is a component. Diagnostic; never in a tick.
    #[must_use]
    pub fn directed_step_count(&self) -> u64 {
        let mut total: u64 = 0;
        let mut node: Node = 0;
        while node < self.node_count() {
            let mut k = 0;
            while k < NEIGHBOURS.len() {
                if self.neighbour(node, k).is_some() {
                    total = total.saturating_add(1);
                }
                k = k.saturating_add(1);
            }
            node = node.saturating_add(1);
        }
        total
    }

    /// The largest path cost this map can produce, as an `i64`.
    ///
    /// Asserted against `i32::MAX` at construction of a [`crate::world::World`]:
    /// a simple path visits at most one column each, so no route can cost more
    /// than `columns * max_step`. At 147 456 columns and 18 per step that is
    /// 2 654 208, three orders of magnitude of headroom — and overflow checks
    /// stay on in every profile, so a surprise is a panic rather than a wrap.
    #[must_use]
    pub fn max_path_cost(&self) -> i64 {
        i64::from(self.node_count()).saturating_mul(i64::from(self.costs.max_step()))
    }
}
