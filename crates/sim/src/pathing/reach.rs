// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The grid optimum from one column to every column: Dijkstra over the
//! surface graph (lane `p3fix`).
//!
//! # What it is for
//!
//! A `toward` step weighs every legal site inside the seat's spheres by the
//! walk the router would serve from the site to the vent
//! ([`crate::targeting`]'s `stone_site`). Pricing a walk is a full route, so
//! the search prices sites in the order of a lower bound and stops once no
//! unpriced site's bound can beat the best walk found. The octile bound it
//! used first ignores terrain: behind a wall every site's octile bound sits
//! below every walk, nothing is pruned, and one resolve priced all 1 753 sites
//! of the probation seed (4.5 M nodes popped).
//!
//! This search gives every site the **least cost of any walk** to the vent
//! instead: one Dijkstra from the vent over [`Surface::neighbour`], the
//! walkers' own step rule at the rules' step costs. A served walk is a path on
//! that graph (the router writes adjacent columns, and
//! [`crate::pathing::search::route_cost`] prices it step by step at
//! [`Surface::step_cost_between`]), so it never costs less than the optimum,
//! and the optimum is a lower bound walls cannot fool. The search settles
//! columns in ascending cost, so the sites come out in the bound's own order,
//! and the caller stops it as soon as the frontier's cost passes the best walk.
//!
//! # Direction
//!
//! The search runs outward from the vent, where a walk runs toward it, so it
//! reads each step backwards. [`Surface::neighbour`]'s step rule is symmetric
//! in legality and in cost (a climb is charged either way, and a diagonal's
//! sides are checked against both endpoints), so the forward graph is its own
//! reverse. `tests/pathing.rs`'s `the_step_rule_is_symmetric` holds every
//! step of a generated, cratered map to it; an asymmetric rule would have to
//! read [`Surface::step_cost_between`] from each neighbour instead.
//!
//! # No allocation
//!
//! Every buffer is one entry per column, reserved where the scratch is made.
//! The heap is indexed (a column is in it at most once, its key lowered in
//! place), so it never holds more than the map's columns either.

use crate::pathing::search::Tally;
use crate::pathing::surface::{NEIGHBOURS, Node, Surface};

/// Dijkstra's buffers, one entry per column, and the site marks a `toward`
/// search reads beside them.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reach {
    /// The search's generation: a column's cost is this search's when its
    /// stamp equals it.
    epoch: u32,
    stamp: Vec<u32>,
    settled: Vec<u32>,
    cost: Vec<i64>,
    /// The indexed min-heap over `(cost, column)`.
    heap: Vec<Node>,
    /// Each column's place in `heap`, valid while it is stamped and unsettled.
    slot: Vec<u32>,
    /// The marks' generation, apart from the search's so the sites can be
    /// marked before the search starts.
    site_epoch: u32,
    sites: Vec<u32>,
}

impl Reach {
    /// Room for a map of `nodes` columns.
    #[must_use]
    pub fn new(nodes: u32) -> Reach {
        let n = crate::math::index::row(nodes);
        Reach {
            epoch: 0,
            stamp: vec![0; n],
            settled: vec![0; n],
            cost: vec![0; n],
            heap: Vec::with_capacity(n),
            slot: vec![0; n],
            site_epoch: 0,
            sites: vec![0; n],
        }
    }

    /// The heap's reserved room: every column at once, the most it can hold.
    #[must_use]
    pub fn heap_capacity(&self) -> usize {
        self.heap.capacity()
    }

    /// Forget every mark.
    pub fn clear_sites(&mut self) {
        if self.site_epoch == u32::MAX {
            self.sites.fill(0);
            self.site_epoch = 0;
        }
        self.site_epoch += 1;
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

    /// Start a search from `from`. A column off the map starts nothing.
    pub fn begin(&mut self, from: Node) {
        if self.epoch == u32::MAX {
            self.stamp.fill(0);
            self.settled.fill(0);
            self.epoch = 0;
        }
        self.epoch += 1;
        self.heap.clear();
        self.lower(from, 0);
    }

    /// The frontier's least cost: no column the search has not settled yet
    /// costs less. `None` once the search has settled everything it reaches.
    #[must_use]
    pub fn frontier(&self) -> Option<i64> {
        let first = self.heap.first().copied()?;
        self.cost_of(first)
    }

    /// Settle the next column, in ascending `(cost, column)`, and lower its
    /// neighbours' costs; each column popped is counted in `popped`.
    pub fn next(&mut self, surface: &Surface, popped: &mut Tally) -> Option<(Node, i64)> {
        let node = self.pop()?;
        let here = self.cost_of(node)?;
        if let Some(mark) = usize::try_from(node)
            .ok()
            .and_then(|index| self.settled.get_mut(index))
        {
            *mark = self.epoch;
        }
        popped.bump();
        for k in 0..NEIGHBOURS.len() {
            let Some((other, step)) = surface.neighbour(node, k) else {
                continue;
            };
            if self.is_settled(other) {
                continue;
            }
            // A walk holds at most one step per column, and a column count
            // is a `u32`, so a cost of `u32::MAX` steps of at most
            // `i32::MAX` each stays below `i64::MAX`.
            self.lower(other, here + i64::from(step));
        }
        Some((node, here))
    }

    /// The least cost of any walk from `node` to the search's start, once the
    /// search has settled it; `None` before.
    #[must_use]
    pub fn cost_at(&self, node: Node) -> Option<i64> {
        self.is_settled(node).then(|| self.cost_of(node)).flatten()
    }

    fn is_settled(&self, node: Node) -> bool {
        usize::try_from(node)
            .ok()
            .and_then(|index| self.settled.get(index))
            .is_some_and(|mark| *mark == self.epoch)
    }

    fn cost_of(&self, node: Node) -> Option<i64> {
        let index = usize::try_from(node).ok()?;
        (self.stamp.get(index).copied()? == self.epoch)
            .then(|| self.cost.get(index).copied())
            .flatten()
    }

    fn key(&self, node: Node) -> (i64, Node) {
        // E7: cannot miss: a column in the heap was stamped when it was pushed
        (self.cost_of(node).unwrap_or(i64::MAX), node)
    }

    /// Lower `node`'s cost to `cost` if that is lower, pushing it when the
    /// search has not reached it yet.
    fn lower(&mut self, node: Node, cost: i64) {
        let Ok(index) = usize::try_from(node) else {
            return;
        };
        let epoch = self.epoch;
        let (Some(stamp), Some(held)) = (self.stamp.get(index).copied(), self.cost.get(index))
        else {
            return;
        };
        let reached = stamp == epoch;
        if reached && *held <= cost {
            return;
        }
        if let Some(slot) = self.cost.get_mut(index) {
            *slot = cost;
        }
        if let Some(slot) = self.stamp.get_mut(index) {
            *slot = epoch;
        }
        let at = if reached {
            // E7: cannot miss: a reached, unsettled column holds its place in the heap
            self.slot.get(index).copied().unwrap_or(0)
        } else {
            // The heap holds each column at most once, so it never passes
            // the map's columns, the room reserved for it.
            // E7: cannot miss: the heap holds fewer columns than the map, whose count is a `u32`
            let at = u32::try_from(self.heap.len()).unwrap_or(u32::MAX);
            self.heap.push(node);
            at
        };
        self.sift_up(at);
    }

    fn pop(&mut self) -> Option<Node> {
        let first = self.heap.first().copied()?;
        let last = self.heap.pop()?;
        if !self.heap.is_empty() {
            if let Some(top) = self.heap.first_mut() {
                *top = last;
            }
            self.place(0);
            self.sift_down(0);
        }
        Some(first)
    }

    fn place(&mut self, at: u32) {
        if let Some(node) = self.heap.get(crate::math::index::row(at)).copied()
            && let Some(slot) = usize::try_from(node)
                .ok()
                .and_then(|index| self.slot.get_mut(index))
        {
            *slot = at;
        }
    }

    fn swap(&mut self, a: u32, b: u32) {
        self.heap
            .swap(crate::math::index::row(a), crate::math::index::row(b));
        self.place(a);
        self.place(b);
    }

    fn sift_up(&mut self, mut at: u32) {
        self.place(at);
        while at > 0 {
            let parent = (at - 1) >> 1;
            let (Some(child), Some(up)) = (
                self.heap.get(crate::math::index::row(at)).copied(),
                self.heap.get(crate::math::index::row(parent)).copied(),
            ) else {
                return;
            };
            if self.key(child) >= self.key(up) {
                return;
            }
            self.swap(at, parent);
            at = parent;
        }
    }

    fn sift_down(&mut self, mut at: u32) {
        loop {
            let Some(left) = at.checked_mul(2).and_then(|twice| twice.checked_add(1)) else {
                return;
            };
            let Some(right) = left.checked_add(1) else {
                return;
            };
            let mut least = at;
            for child in [left, right] {
                let (Some(c), Some(l)) = (
                    self.heap.get(crate::math::index::row(child)).copied(),
                    self.heap.get(crate::math::index::row(least)).copied(),
                ) else {
                    continue;
                };
                if self.key(c) < self.key(l) {
                    least = child;
                }
            }
            if least == at {
                return;
            }
            self.swap(at, least);
            at = least;
        }
    }
}
