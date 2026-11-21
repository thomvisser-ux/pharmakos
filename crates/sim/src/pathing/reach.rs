// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The grid optimum from one column to every column: Dijkstra over the
//! surface graph (lane `p3fix`).
//!
//! # What it is for
//!
//! A `toward` step's stepping stone is the legal site inside the seat's
//! spheres closest to the feature over the terrain ([`crate::targeting`]'s
//! "Two measures"): the least cost of any walk from the site to the feature,
//! over [`Surface::neighbour`], the walkers' own step rule at the rules' step
//! costs. One Dijkstra outward from the feature gives every column that cost
//! exactly, settling them cheapest first in `(cost, column)` order, so the
//! first legal site it settles is the stone and the search stops there. Its
//! work is the columns cheaper than the stone, and at most the map's.
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
//! the rules' `locomotion` rows), so the frontier is a bucket queue (Dial's):
//! a ring of `max step + 1` buckets holds every cost the frontier can hold at
//! once, since a column waits at most one step's cost above the cost the
//! search has reached. A column sits in at most one bucket, linked through two
//! arrays and unlinked when its cost is lowered. When the search reaches a
//! bucket it sorts the bucket's columns and settles them in that order, so
//! columns settle in ascending `(cost, column)`, the order a binary heap over
//! that key gives: a step costs at least one unit (the rules refuse a free
//! step), so nothing joins a bucket once it is reached. Measured against such
//! a heap on full floods of generated and scarred maps (lane `p3fix`, fix pass
//! 2), the order was identical and the queue about 1.6 times faster (a full
//! flood of the shipped map: 23.6 ms against 37.3 ms, release, on a machine
//! that was not idle); `tests/nearest.rs`'s
//! `the_grid_optimum_settles_in_cost_then_column_order` holds the order to a
//! Dijkstra of its own.
//!
//! # No allocation
//!
//! Every buffer is one entry per column, or one per bucket, reserved where the
//! scratch is made; a reached bucket's columns are sorted in a buffer of one
//! entry per column.

use crate::pathing::search::Tally;
use crate::pathing::surface::{NEIGHBOURS, Node, Surface};

/// No column: an empty link.
const NONE: u32 = u32::MAX;

/// The grid optimum's search, its bucket queue (the module docs), and the
/// site marks a `toward` search reads beside it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reach {
    epoch: u32,
    stamp: Vec<u32>,
    settled: Vec<u32>,
    cost: Vec<i64>,
    /// The epoch a column was linked into a bucket in; 0 once unlinked.
    listed: Vec<u32>,
    next: Vec<u32>,
    prev: Vec<u32>,
    heads: Vec<u32>,
    /// The bucket being settled, sorted by column, and where in it.
    sorted: Vec<Node>,
    cursor: usize,
    /// The cost the search has reached.
    at: i64,
    /// Columns linked into a bucket.
    waiting: usize,
    /// The marks' generation, apart from the search's so the sites can be
    /// marked before the search starts.
    site_epoch: u32,
    sites: Vec<u32>,
}

impl Reach {
    /// Room for a map of `nodes` columns whose dearest step costs
    /// `max_step` ([`crate::pathing::surface::StepCosts::max_step`]).
    #[must_use]
    pub fn new(nodes: u32, max_step: i32) -> Reach {
        let n = crate::math::index::row(nodes);
        // E7: rule: a step costs at least one unit (the rules refuse a free one), so a ring of two holds unit steps
        let buckets = usize::try_from(max_step.max(1)).map_or(2, |step| step.saturating_add(1));
        Reach {
            epoch: 0,
            stamp: vec![0; n],
            settled: vec![0; n],
            cost: vec![0; n],
            listed: vec![0; n],
            next: vec![NONE; n],
            prev: vec![NONE; n],
            heads: vec![NONE; buckets],
            sorted: Vec::with_capacity(n),
            cursor: 0,
            at: 0,
            waiting: 0,
            site_epoch: 0,
            sites: vec![0; n],
        }
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

    /// Start a search from `from`.
    pub fn begin(&mut self, from: Node) {
        if self.epoch == u32::MAX {
            self.stamp.fill(0);
            self.settled.fill(0);
            self.listed.fill(0);
            self.epoch = 0;
        }
        self.epoch += 1;
        self.heads.fill(NONE);
        self.sorted.clear();
        self.cursor = 0;
        self.at = 0;
        self.waiting = 0;
        self.lower(from, 0);
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

    /// Settle the next column, in ascending `(cost, column)`, and lower its
    /// neighbours' costs; each column popped is counted in `popped`.
    pub fn next(&mut self, surface: &Surface, popped: &mut Tally) -> Option<(Node, i64)> {
        let node = loop {
            if let Some(node) = self.sorted.get(self.cursor).copied() {
                self.cursor += 1;
                break node;
            }
            if self.waiting == 0 {
                return None;
            }
            self.advance();
        };
        let index = usize::try_from(node).ok()?;
        if let Some(mark) = self.settled.get_mut(index) {
            *mark = self.epoch;
        }
        let here = self.at;
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
            self.lower(other, here + i64::from(step));
        }
        Some((node, here))
    }

    /// Move to the next cost a bucket holds, and sort its columns. Every
    /// waiting cost lies within the ring's length above the cost reached, so
    /// one turn of the ring finds it.
    fn advance(&mut self) {
        self.sorted.clear();
        self.cursor = 0;
        // E7: cannot miss: the ring's length is a step cost plus one, far inside `i64`
        let ring = i64::try_from(self.heads.len()).unwrap_or(i64::MAX);
        for _ in 0..self.heads.len() {
            self.at += 1;
            let slot = usize::try_from(self.at.rem_euclid(ring)).unwrap_or(0); // E7: cannot miss: a remainder by the ring's length is below it
            // E7: cannot miss: a slot is a remainder by the ring's length
            let mut node = self.heads.get(slot).copied().unwrap_or(NONE);
            if node == NONE {
                continue;
            }
            if let Some(head) = self.heads.get_mut(slot) {
                *head = NONE;
            }
            while node != NONE {
                let index = crate::math::index::row(node);
                self.sorted.push(node);
                if let Some(mark) = self.listed.get_mut(index) {
                    *mark = 0;
                }
                // E7: cannot miss: a linked column is a column of the map
                node = self.next.get(index).copied().unwrap_or(NONE);
            }
            self.waiting -= self.sorted.len();
            self.sorted.sort_unstable();
            return;
        }
    }

    fn lower(&mut self, node: Node, cost: i64) {
        let Ok(index) = usize::try_from(node) else {
            return;
        };
        let reached = self.stamp.get(index).copied() == Some(self.epoch);
        if reached && self.cost.get(index).is_some_and(|held| *held <= cost) {
            return;
        }
        if self.listed.get(index).copied() == Some(self.epoch) {
            self.unlink(node);
        }
        if let Some(slot) = self.cost.get_mut(index) {
            *slot = cost;
        }
        if let Some(slot) = self.stamp.get_mut(index) {
            *slot = self.epoch;
        }
        if !reached && cost == 0 {
            // The start, at the cost reached: settled from the current bucket
            // at once rather than a turn of the ring later.
            self.sorted.push(node);
            return;
        }
        // E7: cannot miss: the ring's length is a step cost plus one, far inside `i64`
        let ring = i64::try_from(self.heads.len()).unwrap_or(i64::MAX);
        let slot = usize::try_from(cost.rem_euclid(ring)).unwrap_or(0); // E7: cannot miss: a remainder by the ring's length is below it
        // E7: cannot miss: as above
        let head = self.heads.get(slot).copied().unwrap_or(NONE);
        if let Some(link) = self.next.get_mut(index) {
            *link = head;
        }
        if let Some(link) = self.prev.get_mut(index) {
            *link = NONE;
        }
        if head != NONE
            && let Some(link) = self.prev.get_mut(crate::math::index::row(head))
        {
            *link = node;
        }
        if let Some(first) = self.heads.get_mut(slot) {
            *first = node;
        }
        if let Some(mark) = self.listed.get_mut(index) {
            *mark = self.epoch;
        }
        self.waiting += 1;
    }

    fn unlink(&mut self, node: Node) {
        let index = crate::math::index::row(node);
        let (Some(before), Some(after), Some(cost)) = (
            self.prev.get(index).copied(),
            self.next.get(index).copied(),
            self.cost.get(index).copied(),
        ) else {
            return;
        };
        if before == NONE {
            // E7: cannot miss: the ring's length is a step cost plus one, far inside `i64`
            let ring = i64::try_from(self.heads.len()).unwrap_or(i64::MAX);
            let slot = usize::try_from(cost.rem_euclid(ring)).unwrap_or(0); // E7: cannot miss: a remainder by the ring's length is below it
            if let Some(head) = self.heads.get_mut(slot) {
                *head = after;
            }
        } else if let Some(link) = self.next.get_mut(crate::math::index::row(before)) {
            *link = after;
        }
        if after != NONE
            && let Some(link) = self.prev.get_mut(crate::math::index::row(after))
        {
            *link = before;
        }
        if let Some(mark) = self.listed.get_mut(index) {
            *mark = 0;
        }
        self.waiting -= 1;
    }
}
