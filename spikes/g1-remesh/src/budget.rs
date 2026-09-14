// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later
#![deny(clippy::float_arithmetic)]
//! The bounded per-frame upload queue: *K* chunk surfaces and *B* bytes,
//! whichever binds first, drained nearest-to-camera first with the chunk index
//! as the tie-break (G1 plan §2).
//!
//! **Sim side of the wall** — the drain *order* is a policy decision and is
//! integer throughout: the camera position is rounded to voxels before it gets
//! here and distances are compared squared, never as float lengths and never
//! square-rooted. Sorting by a float key is the classic way to make a queue
//! order machine-dependent (AGENTS.md §4.6), and this queue's order is one of
//! the numbers the spike reports, so it has to be reproducible.
//!
//! Coalescing: a chunk re-dirtied while still queued is **not** queued twice —
//! the entry keeps the *earliest* issue frame, so the measured latency is the
//! age of the oldest edit the upload finally makes visible. That is the
//! conservative reading of G1-a.

use std::collections::BTreeMap;

use crate::chunk::CHUNK_EDGE_I;
use crate::world::chunk_origin;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueEntry {
    pub chunk: u32,
    pub issue_frame: u64,
}

#[derive(Default)]
pub struct UploadQueue {
    pending: BTreeMap<u32, u64>,
    /// Deepest the queue ever got — "does the queue ever run away".
    pub max_depth: usize,
    /// Pushes that hit an already-pending chunk.
    pub coalesced: u64,
    pub enqueued: u64,
}

impl UploadQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, chunk: u32, issue_frame: u64) {
        match self.pending.get_mut(&chunk) {
            Some(f) => {
                self.coalesced += 1;
                if issue_frame < *f {
                    *f = issue_frame;
                }
            }
            None => {
                self.pending.insert(chunk, issue_frame);
                self.enqueued += 1;
            }
        }
        if self.pending.len() > self.max_depth {
            self.max_depth = self.pending.len();
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn clear(&mut self) {
        self.pending.clear();
    }

    /// Squared distance from the camera voxel to the chunk centre. Integer.
    #[inline]
    fn dist2(chunk: u32, cam: (i32, i32, i32)) -> i64 {
        let (ox, oy, oz) = chunk_origin(chunk as usize);
        let half = CHUNK_EDGE_I / 2;
        let dx = i64::from(ox + half - cam.0);
        let dy = i64::from(oy + half - cam.1);
        let dz = i64::from(oz + half - cam.2);
        dx * dx + dy * dy + dz * dz
    }

    /// The next at most `k` chunks in drain order: **anything stale first**,
    /// then nearest to the camera, ties broken by chunk index.
    ///
    /// Distance alone has unbounded worst-case latency by construction — a far
    /// chunk can be deferred for ever while nearer ones keep being re-dirtied.
    /// It never bit in the measurement (max queue depth 4 at K = 4, latency max
    /// 1 frame), but G1-a *is* a latency gate and this is the policy the spike
    /// recommends to the skeleton, so the bound is in the policy rather than in
    /// a footnote. `MAX_AGE_FRAMES` is 2 against a 3-frame gate.
    pub fn plan(&self, cam: (i32, i32, i32), k: usize, frame: u64) -> Vec<QueueEntry> {
        let mut v: Vec<(bool, i64, u32, u64)> = self
            .pending
            .iter()
            .map(|(&c, &f)| {
                // `false` sorts first, so "not stale" must be the true-ish key.
                let fresh = frame.saturating_sub(f) < MAX_AGE_FRAMES;
                (fresh, Self::dist2(c, cam), c, f)
            })
            .collect();
        v.sort_unstable_by_key(|&(fresh, d, c, _)| (fresh, d, c));
        v.truncate(k);
        v.into_iter()
            .map(|(_, _, chunk, issue_frame)| QueueEntry { chunk, issue_frame })
            .collect()
    }

    pub fn commit(&mut self, chunk: u32) {
        self.pending.remove(&chunk);
    }

    /// Age in frames of every entry still queued at `frame`, so a summary can
    /// say what the chunks it never uploaded were waiting for. Without this the
    /// latency tail of a *failing* configuration is flattered: only uploaded
    /// chunks contribute a sample, and the worst-aged chunks are exactly the
    /// ones a runaway queue never gets to.
    pub fn pending_ages(&self, frame: u64) -> Vec<u64> {
        self.pending
            .values()
            .map(|&f| frame.saturating_sub(f))
            .collect()
    }
}

/// A queued chunk this old jumps the distance ordering. 2 frames against G1-a's
/// 3-frame gate leaves one frame of slack for the upload itself.
pub const MAX_AGE_FRAMES: u64 = 2;

#[derive(Clone, Copy, Debug)]
pub struct Budget {
    /// Max chunk surfaces uploaded per frame. `usize::MAX` = unbounded.
    pub k: usize,
    /// Max bytes uploaded per frame. `usize::MAX` = unbounded.
    pub b: usize,
}

impl Budget {
    pub const UNBOUNDED: Self = Self {
        k: usize::MAX,
        b: usize::MAX,
    };
}

#[derive(Clone, Debug, Default)]
pub struct DrainResult {
    pub uploaded: usize,
    pub bytes: usize,
    /// `upload_frame - issue_frame` for every chunk uploaded this frame.
    pub latencies: Vec<u32>,
    /// Chunks that were next in order but left queued because *B* was reached.
    pub deferred_for_bytes: usize,
}

/// Drain the queue for one frame.
///
/// `upload(chunk, issue_frame) -> bytes` does the meshing and the actual upload
/// and reports what it cost. The first chunk of a frame is always uploaded even
/// if it alone exceeds *B*: a surface larger than the whole byte budget must not
/// be able to stall the queue forever.
pub fn drain<F>(
    q: &mut UploadQueue,
    cam: (i32, i32, i32),
    budget: Budget,
    frame: u64,
    mut upload: F,
) -> DrainResult
where
    F: FnMut(u32, u64) -> usize,
{
    let mut out = DrainResult::default();
    if budget.k == 0 {
        return out;
    }
    let plan = q.plan(cam, budget.k, frame);
    let planned = plan.len();
    for (i, e) in plan.into_iter().enumerate() {
        let bytes = upload(e.chunk, e.issue_frame);
        q.commit(e.chunk);
        out.uploaded += 1;
        out.bytes += bytes;
        out.latencies
            .push(u32::try_from(frame.saturating_sub(e.issue_frame)).unwrap_or(u32::MAX));
        if out.bytes >= budget.b {
            // Byte budget spent. Whatever was still planned waits a frame.
            out.deferred_for_bytes = planned - i - 1;
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::chunk_index;

    #[test]
    fn coalescing_keeps_the_earliest_issue_frame() {
        let mut q = UploadQueue::new();
        q.push(5, 10);
        q.push(5, 12);
        q.push(5, 7);
        assert_eq!(q.len(), 1);
        assert_eq!(q.coalesced, 2);
        let p = q.plan((0, 0, 0), 4, 12);
        assert_eq!(p[0].issue_frame, 7);
    }

    #[test]
    fn drain_order_is_nearest_camera_then_index() {
        let mut q = UploadQueue::new();
        let near = u32::try_from(chunk_index(3, 1, 3)).unwrap();
        let far = u32::try_from(chunk_index(11, 1, 11)).unwrap();
        q.push(far, 0);
        q.push(near, 0);
        // Camera sits on top of the "near" chunk.
        let cam = (3 * 32 + 16, 32 + 16, 3 * 32 + 16);
        let p = q.plan(cam, 2, 0);
        assert_eq!(p[0].chunk, near);
        assert_eq!(p[1].chunk, far);
    }

    #[test]
    fn ties_break_on_chunk_index() {
        let mut q = UploadQueue::new();
        // Two chunks equidistant from a camera exactly between them.
        let a = u32::try_from(chunk_index(5, 0, 5)).unwrap();
        let b = u32::try_from(chunk_index(6, 0, 5)).unwrap();
        q.push(b, 0);
        q.push(a, 0);
        let cam = (6 * 32, 16, 5 * 32 + 16);
        let p = q.plan(cam, 2, 0);
        assert!(p[0].chunk < p[1].chunk);
    }

    #[test]
    fn a_stale_chunk_jumps_the_distance_ordering() {
        let mut q = UploadQueue::new();
        let near = u32::try_from(chunk_index(3, 1, 3)).unwrap();
        let far = u32::try_from(chunk_index(11, 1, 11)).unwrap();
        let cam = (3 * 32 + 16, 32 + 16, 3 * 32 + 16);
        // `far` was dirtied MAX_AGE_FRAMES ago; `near` this frame.
        q.push(far, 10);
        q.push(near, 10 + MAX_AGE_FRAMES);
        let p = q.plan(cam, 2, 10 + MAX_AGE_FRAMES);
        assert_eq!(p[0].chunk, far, "the stale chunk goes first");
        assert_eq!(p[1].chunk, near);
        // One frame younger and distance wins again.
        let p = q.plan(cam, 2, 9 + MAX_AGE_FRAMES);
        assert_eq!(p[0].chunk, near);
    }

    #[test]
    fn pending_ages_reports_what_the_queue_never_uploaded() {
        let mut q = UploadQueue::new();
        q.push(1, 5);
        q.push(2, 9);
        let mut ages = q.pending_ages(10);
        ages.sort_unstable();
        assert_eq!(ages, vec![1, 5]);
    }

    #[test]
    fn k_and_b_both_bind_and_one_chunk_always_gets_through() {
        let mut q = UploadQueue::new();
        for c in 0..10u32 {
            q.push(c, 0);
        }
        // K binds.
        let r = drain(
            &mut q,
            (0, 0, 0),
            Budget {
                k: 3,
                b: usize::MAX,
            },
            1,
            |_, _| 100,
        );
        assert_eq!(r.uploaded, 3);
        assert_eq!(r.bytes, 300);
        assert!(r.latencies.iter().all(|&l| l == 1));
        // B binds: 250 bytes allows two 100-byte surfaces, the third is deferred.
        let r = drain(&mut q, (0, 0, 0), Budget { k: 8, b: 250 }, 2, |_, _| 100);
        assert_eq!(
            r.uploaded, 3,
            "stops once bytes >= B, after the crossing upload"
        );
        // A single oversized surface is never allowed to stall the queue.
        let r = drain(&mut q, (0, 0, 0), Budget { k: 8, b: 10 }, 3, |_, _| 9_999);
        assert_eq!(r.uploaded, 1);
    }
}
