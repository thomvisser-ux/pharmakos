// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The per-frame upload budget and its drain order — decisions log section 2.7 item 54, in
//! full.
//!
//! The sim says *which* chunks changed. This says *when* each one is meshed and uploaded,
//! and it is on the presentation side of the wall because its order depends on the camera
//! (see the crate docs). Integer throughout all the same: the camera arrives as a chunk
//! coordinate, distances are compared squared, and nothing is sorted by a float key — a
//! float sort key is the classic way to make a queue order machine-dependent (AGENTS.md
//! section 4.6), and this queue's order is one of the things the geometry goldens and the
//! latency measurement both rest on.
//!
//! # Item 54's order, exactly
//!
//! 1. **Anything older than `age_frames` first.** Distance alone has unbounded worst-case
//!    latency by construction: a far chunk can be deferred for ever while nearer ones keep
//!    being re-dirtied. G1 never saw it bite (max queue depth 4 at K = 4, latency max 1
//!    frame), so the ageing term ships as **insurance with no measurement behind it** —
//!    labelled as such by item 54 and repeated here so nobody reads the code as evidence.
//! 2. **Then nearest to the camera**, by integer squared chunk distance.
//! 3. **Then by chunk index**, so the order is total and reproducible.
//!
//! A chunk re-dirtied while queued is **coalesced** and keeps its **earliest** issue frame,
//! so the age measured is the age of the oldest edit the upload finally makes visible —
//! the conservative reading of G1-a.
//!
//! The **first chunk of a frame always goes through**, even when it alone exceeds the byte
//! budget: a surface larger than the whole budget must not be able to stall the queue for
//! ever. After that, a chunk that would take the frame past `bytes_per_frame` waits.
//!
//! # The numbers are parameters
//!
//! `surfaces_per_frame` (K = 4), `bytes_per_frame` (B = 512 KiB) and `age_frames` (2) are
//! rules-table rows, stamped into the rules hash. The mesher cannot read the rules table —
//! that would be a dependency edge into the sim, which is what `wall-guard` forbids — so
//! they arrive as [`DrainBudget`], filled by `pharmakos-client-gdext` (T12).
//!
//! PLACEHOLDER: K = 4 and B = 512 KiB carry item 54's own caveat — *no measured frame-time
//! reason separates K = 4 from K = 8 on the spike machine, so the choice is structural, a
//! bound on per-frame work*; and B is one doubling above the point where B stopped binding,
//! a margin for a heavier vertex format rather than an optimum. Owner re-derives both at
//! S2's exit with the real world under load.

use std::collections::BTreeMap;

use crate::ChunkGrid;

/// The three rules-table rows the drain queue needs.
///
/// `surfaces_per_frame` of zero drains nothing, which is a legal way for a caller to pause
/// uploads for a frame; `bytes_per_frame` of zero still lets the first chunk through, by
/// the rule above.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DrainBudget {
    /// K: how many chunk surfaces may be uploaded in one frame.
    pub surfaces_per_frame: u32,
    /// B: how many bytes may be uploaded in one frame.
    pub bytes_per_frame: u64,
    /// A queued chunk this many frames old or older jumps the distance ordering.
    pub age_frames: u64,
}

/// The bounded per-frame upload queue.
///
/// Push the sim's dirty chunks as they arrive; call [`DrainQueue::drain`] once a frame with
/// the camera's chunk and a function that says what each chunk's surface would cost.
#[derive(Clone, Debug)]
pub struct DrainQueue {
    grid: ChunkGrid,
    budget: DrainBudget,
    /// Chunk index to its earliest issue frame. Ordered, so iteration is reproducible.
    pending: BTreeMap<u32, u64>,
    /// Sort scratch, kept so a steady-state drain allocates nothing.
    order: Vec<(bool, i64, u32)>,
    max_depth: usize,
    coalesced: u64,
    enqueued: u64,
}

impl DrainQueue {
    /// An empty queue over `grid`, with `budget`'s three rows.
    #[must_use]
    pub fn new(grid: ChunkGrid, budget: DrainBudget) -> Self {
        Self {
            grid,
            budget,
            pending: BTreeMap::new(),
            order: Vec::new(),
            max_depth: 0,
            coalesced: 0,
            enqueued: 0,
        }
    }

    /// The budget this queue drains under.
    #[must_use]
    pub const fn budget(&self) -> DrainBudget {
        self.budget
    }

    /// Replaces the budget, for a caller that re-reads the rules table between matches.
    pub fn set_budget(&mut self, budget: DrainBudget) {
        self.budget = budget;
    }

    /// Queues one dirty chunk, issued on `issue_frame`.
    ///
    /// A chunk already queued is **coalesced**: it is not queued twice, and it keeps the
    /// earlier of the two issue frames.
    pub fn push(&mut self, chunk: u32, issue_frame: u64) {
        if let Some(existing) = self.pending.get_mut(&chunk) {
            self.coalesced = self.coalesced.saturating_add(1);
            if issue_frame < *existing {
                *existing = issue_frame;
            }
        } else {
            self.pending.insert(chunk, issue_frame);
            self.enqueued = self.enqueued.saturating_add(1);
        }
        self.max_depth = self.max_depth.max(self.pending.len());
    }

    /// How many chunks are waiting.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// True when nothing is waiting.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// The deepest the queue has ever been — "does the queue ever run away".
    #[must_use]
    pub const fn max_depth(&self) -> usize {
        self.max_depth
    }

    /// How many pushes hit a chunk that was already queued.
    #[must_use]
    pub const fn coalesced(&self) -> u64 {
        self.coalesced
    }

    /// How many distinct chunks have been queued.
    #[must_use]
    pub const fn enqueued(&self) -> u64 {
        self.enqueued
    }

    /// The age in frames of every chunk still queued at `frame`, ascending by chunk index.
    ///
    /// Appends to `out`. Without this the latency tail of a *failing* configuration is
    /// flattered: only uploaded chunks contribute a sample, and the worst-aged chunks are
    /// exactly the ones a runaway queue never reaches (G1 section 10.5).
    pub fn pending_ages(&self, frame: u64, out: &mut Vec<u64>) {
        for issue in self.pending.values() {
            out.push(frame.saturating_sub(*issue));
        }
    }

    /// Takes this frame's chunks, in item 54's order, and removes them from the queue.
    ///
    /// `byte_size_of` says what a chunk's surface would cost to upload — the caller's own
    /// measure, usually [`gpu_surface_bytes`](crate::gpu_surface_bytes) over the last mesh
    /// it made of that chunk, or an estimate for one it has never meshed.
    #[must_use]
    pub fn drain<F>(&mut self, frame: u64, camera_chunk: [i32; 3], byte_size_of: F) -> Vec<u32>
    where
        F: Fn(u32) -> usize,
    {
        let mut out = Vec::new();
        self.drain_into(frame, camera_chunk, byte_size_of, &mut out);
        out
    }

    /// [`DrainQueue::drain`] into a buffer the caller keeps, which allocates nothing once
    /// both it and the queue's own scratch are warm.
    ///
    /// Appends; `out` is not cleared.
    pub fn drain_into<F>(
        &mut self,
        frame: u64,
        camera_chunk: [i32; 3],
        byte_size_of: F,
        out: &mut Vec<u32>,
    ) where
        F: Fn(u32) -> usize,
    {
        if self.budget.surfaces_per_frame == 0 || self.pending.is_empty() {
            return;
        }

        self.order.clear();
        for (chunk, issue) in &self.pending {
            // `false` sorts first, so the key is "fresh", not "stale".
            let fresh = frame.saturating_sub(*issue) < self.budget.age_frames;
            self.order
                .push((fresh, self.distance_squared(*chunk, camera_chunk), *chunk));
        }
        // Every key ends in the chunk index, so the order is total and the sort's
        // behaviour on equal elements cannot matter.
        self.order.sort_unstable();

        let first_taken = out.len();
        let mut surfaces = 0_u32;
        let mut bytes = 0_u64;
        for (_, _, chunk) in &self.order {
            if surfaces >= self.budget.surfaces_per_frame {
                break;
            }
            let size = u64::try_from(byte_size_of(*chunk)).unwrap_or(u64::MAX);
            // The first chunk of a frame always goes through, even if it alone exceeds B.
            if surfaces > 0 && bytes.saturating_add(size) > self.budget.bytes_per_frame {
                break;
            }
            out.push(*chunk);
            surfaces = surfaces.saturating_add(1);
            bytes = bytes.saturating_add(size);
        }

        for chunk in out.iter().skip(first_taken) {
            self.pending.remove(chunk);
        }
    }

    /// Squared distance from the camera's chunk to `chunk`, in chunks.
    ///
    /// Squared, and integer: a square root would be a float, and a float sort key is what
    /// AGENTS.md section 4.6 bans. A chunk outside the grid sorts last rather than
    /// panicking — it can only get here if the sim named a chunk this queue's grid does not
    /// have, which is a bug in the caller and not a reason to take the frame down.
    fn distance_squared(&self, chunk: u32, camera_chunk: [i32; 3]) -> i64 {
        let Some(coords) = self.grid.chunk_coords(chunk) else {
            return i64::MAX;
        };
        let mut total = 0_i64;
        for axis in 0..3_usize {
            let here = i64::from(coords.get(axis).copied().unwrap_or(0));
            let there = i64::from(camera_chunk.get(axis).copied().unwrap_or(0));
            let delta = here.saturating_sub(there);
            total = total.saturating_add(delta.saturating_mul(delta));
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::{DrainBudget, DrainQueue};
    use crate::ChunkGrid;

    /// The committed rules table's `mesher` block, as T12 will fill it.
    const RULES: DrainBudget = DrainBudget {
        surfaces_per_frame: 4,
        bytes_per_frame: 524_288,
        age_frames: 2,
    };

    fn queue(budget: DrainBudget) -> DrainQueue {
        DrainQueue::new(
            ChunkGrid::new(8, 2, 8).expect("an 8 x 2 x 8 grid is legal"),
            budget,
        )
    }

    #[test]
    fn coalescing_keeps_the_earliest_issue_frame() {
        let mut drain = queue(RULES);
        drain.push(5, 10);
        drain.push(5, 12);
        drain.push(5, 7);
        assert_eq!(drain.len(), 1);
        assert_eq!(drain.coalesced(), 2);
        assert_eq!(drain.enqueued(), 1);
        // At frame 9 the entry is two frames old, so the ageing term has it.
        let mut ages = Vec::new();
        drain.pending_ages(9, &mut ages);
        assert_eq!(ages, vec![2]);
    }

    #[test]
    fn the_order_is_stale_then_nearest_then_index() {
        let grid = ChunkGrid::new(8, 2, 8).expect("an 8 x 2 x 8 grid is legal");
        let near = grid.chunk_index([2, 0, 2]).expect("inside the grid");
        let far = grid.chunk_index([7, 1, 7]).expect("inside the grid");
        let mut drain = queue(RULES);

        // Both fresh: the nearer one wins.
        drain.push(far, 10);
        drain.push(near, 10);
        assert_eq!(drain.drain(10, [2, 0, 2], |_| 0), vec![near, far]);

        // The far one is two frames old at frame 12, the near one fresh: age wins.
        drain.push(far, 10);
        drain.push(near, 12);
        assert_eq!(drain.drain(12, [2, 0, 2], |_| 0), vec![far, near]);

        // One frame younger and distance wins again.
        drain.push(far, 10);
        drain.push(near, 11);
        assert_eq!(drain.drain(11, [2, 0, 2], |_| 0), vec![near, far]);
    }

    #[test]
    fn ties_break_on_the_chunk_index() {
        let grid = ChunkGrid::new(8, 2, 8).expect("an 8 x 2 x 8 grid is legal");
        let left = grid.chunk_index([2, 0, 4]).expect("inside the grid");
        let right = grid.chunk_index([4, 0, 4]).expect("inside the grid");
        assert!(left < right);
        let mut drain = queue(RULES);
        drain.push(right, 0);
        drain.push(left, 0);
        // The camera sits exactly between them.
        assert_eq!(drain.drain(0, [3, 0, 4], |_| 0), vec![left, right]);
    }

    #[test]
    fn the_written_out_order_of_one_frame() {
        // Eight chunks, all dirtied at frame 0; camera at chunk (0, 0, 0); K = 4, B huge.
        // Distances squared from (0,0,0), then index:
        //   (1,0,0) = 1   (0,0,1) = 1   (1,0,1) = 2   (2,0,0) = 4
        //   (0,0,2) = 4   (2,0,2) = 8   (3,0,3) = 18  (7,1,7) = 99
        // Indices: x + 8*z + 64*y, so (1,0,0)=1 and (0,0,1)=8 — the tie at distance 1
        // breaks on the index, and 4 surfaces come back.
        let grid = ChunkGrid::new(8, 2, 8).expect("an 8 x 2 x 8 grid is legal");
        let mut drain = queue(RULES);
        for coords in [
            [7, 1, 7],
            [3, 0, 3],
            [2, 0, 2],
            [0, 0, 2],
            [2, 0, 0],
            [1, 0, 1],
            [0, 0, 1],
            [1, 0, 0],
        ] {
            drain.push(grid.chunk_index(coords).expect("inside the grid"), 0);
        }
        let taken = drain.drain(0, [0, 0, 0], |_| 1_024);
        let expected: Vec<u32> = [[1, 0, 0], [0, 0, 1], [1, 0, 1], [2, 0, 0]]
            .into_iter()
            .map(|coords| grid.chunk_index(coords).expect("inside the grid"))
            .collect();
        assert_eq!(taken, expected);
        assert_eq!(drain.len(), 4, "the other four wait for the next frame");

        // Next frame, still at the origin: (0,0,2) at distance 4 ties (2,0,0), which has
        // gone, so the remaining four come back nearest-first.
        let taken = drain.drain(1, [0, 0, 0], |_| 1_024);
        let expected: Vec<u32> = [[0, 0, 2], [2, 0, 2], [3, 0, 3], [7, 1, 7]]
            .into_iter()
            .map(|coords| grid.chunk_index(coords).expect("inside the grid"))
            .collect();
        assert_eq!(taken, expected);
        assert!(drain.is_empty());
    }

    #[test]
    fn the_byte_budget_binds_after_the_first_chunk() {
        let mut drain = queue(DrainBudget {
            surfaces_per_frame: 8,
            bytes_per_frame: 250,
            age_frames: 2,
        });
        for chunk in 0..6_u32 {
            drain.push(chunk, 0);
        }
        // Two 100-byte surfaces fit; the third would take the frame to 300.
        assert_eq!(drain.drain(0, [0, 0, 0], |_| 100).len(), 2);
        assert_eq!(drain.len(), 4);
    }

    #[test]
    fn one_oversized_surface_never_stalls_the_queue() {
        let mut drain = queue(DrainBudget {
            surfaces_per_frame: 8,
            bytes_per_frame: 10,
            age_frames: 2,
        });
        for chunk in 0..4_u32 {
            drain.push(chunk, 0);
        }
        let taken = drain.drain(0, [0, 0, 0], |_| 9_999);
        assert_eq!(taken.len(), 1, "the first chunk always goes through");
        assert_eq!(drain.len(), 3);
    }

    #[test]
    fn a_surface_budget_of_zero_drains_nothing() {
        let mut drain = queue(DrainBudget {
            surfaces_per_frame: 0,
            bytes_per_frame: 524_288,
            age_frames: 2,
        });
        drain.push(1, 0);
        assert!(drain.drain(0, [0, 0, 0], |_| 0).is_empty());
        assert_eq!(drain.len(), 1);
        assert_eq!(drain.max_depth(), 1);
    }

    #[test]
    fn a_chunk_outside_the_grid_sorts_last_rather_than_panicking() {
        let mut drain = queue(RULES);
        drain.push(9_999, 0);
        drain.push(0, 0);
        assert_eq!(drain.drain(0, [0, 0, 0], |_| 0), vec![0, 9_999]);
    }
}
