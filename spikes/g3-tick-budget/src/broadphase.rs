// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The uniform grid, and the range queries over it.
//!
//! Plan §2's first phase row: *"rebuild/refresh the uniform grid; range queries
//! in squared distance (Q32.32, no square roots) — section 15's maths row; the
//! dominant cost in most RTS ticks"*.
//!
//! Shape: a flat 16x16 grid of 24-voxel cells over the 384x384 footprint, with
//! the cell sized to the largest query radius so that a radius query touches at
//! most 3x3 cells. Storage is CSR — a `start` offset per cell plus one flat
//! `items` array — rebuilt from scratch every tick by a counting sort. Rebuild
//! is O(items + cells) with no allocation after construction, which matters
//! more than it looks: an allocating broadphase would move the answer to plan
//! §5's "allocator" row and make the budget platform-dependent for a reason
//! that has nothing to do with the sim.
//!
//! The nearest-enemy query is an outward ring search with an exact stopping
//! rule: a cell at Chebyshev ring `r` from the query cell is at least
//! `(r - 1) * CELL_VOXELS` away horizontally, and three-dimensional distance is
//! never smaller than horizontal distance, so once the best squared distance
//! found is **strictly** below `((r - 1) * CELL)^2` no unscanned cell can hold
//! anything closer or equally close. Strictly, so that a tie is always fully
//! scanned and the `(d2, id)` tiebreak stays total — which is what
//! `tests/` checks against a brute-force scan over 1_000 random queries.

use crate::fixed::{Fx, Sq};
use crate::{MAP_W_VOXELS, QUERY_RADIUS_VOXELS};

/// Cell edge, in whole voxels. Equal to the largest query radius.
pub const CELL_VOXELS: i32 = QUERY_RADIUS_VOXELS;

/// Cells along one axis.
///
/// Written as a literal rather than as `MAP_W_VOXELS / CELL_VOXELS`, because
/// that quotient is an `i32` and `usize::try_from` is not a `const fn` — so the
/// only way to spell the division would be an `as` cast, which the crate root
/// denies. The two assertions below are the audit: the build fails if the
/// literal and the geometry ever disagree.
pub const GRID_W: usize = 16;

/// Total cells.
pub const GRID_CELLS: usize = GRID_W * GRID_W;

/// Sentinel cell for an item that is not in the grid this tick (dead, or a slot
/// that was never populated).
pub const NO_CELL: u16 = u16::MAX;

const _: () = assert!(MAP_W_VOXELS % CELL_VOXELS == 0, "cells must tile the map");
const _: () = assert!(MAP_W_VOXELS == CELL_VOXELS * 16, "GRID_W literal is stale");

/// The cell a horizontal position falls in, clamped into the grid.
#[must_use]
pub fn cell_of(p: [Fx; 3]) -> u16 {
    let cell_raw = CELL_VOXELS
        .checked_shl(16)
        .expect("cell size in Q16.16 fits i32");
    let cx =
        (p[0].raw() / cell_raw).clamp(0, i32::try_from(GRID_W - 1).expect("grid width fits i32"));
    let cy =
        (p[1].raw() / cell_raw).clamp(0, i32::try_from(GRID_W - 1).expect("grid width fits i32"));
    let w = i32::try_from(GRID_W).expect("grid width fits i32");
    u16::try_from(cy * w + cx).expect("cell index fits u16")
}

/// A CSR bucket grid. `start[c] .. start[c + 1]` indexes `items`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Grid {
    start: Vec<u32>,
    items: Vec<u32>,
    cursor: Vec<u32>,
}

impl Grid {
    #[must_use]
    pub fn with_capacity(items: usize) -> Grid {
        Grid {
            start: vec![0; GRID_CELLS + 1],
            items: vec![0; items],
            cursor: vec![0; GRID_CELLS + 1],
        }
    }

    /// Counting sort of `cells` (one entry per item, [`NO_CELL`] to exclude)
    /// into the CSR arrays. Allocation-free after construction.
    pub fn rebuild(&mut self, cells: &[u16]) {
        debug_assert!(cells.len() <= self.items.len());
        self.start.fill(0);
        let mut live: u32 = 0;
        for &c in cells {
            if c == NO_CELL {
                continue;
            }
            self.start[usize::from(c) + 1] += 1;
            live += 1;
        }
        for c in 0..GRID_CELLS {
            self.start[c + 1] += self.start[c];
        }
        self.cursor.copy_from_slice(&self.start);
        for (i, &c) in cells.iter().enumerate() {
            if c == NO_CELL {
                continue;
            }
            let slot = usize::try_from(self.cursor[usize::from(c)]).expect("csr slot");
            self.items[slot] = u32::try_from(i).expect("item index fits u32");
            self.cursor[usize::from(c)] += 1;
        }
        debug_assert_eq!(self.start[GRID_CELLS], live);
        let _ = live;
    }

    /// The item indices in one cell, in insertion order — which is item-index
    /// order, because the counting sort is stable by construction.
    #[must_use]
    pub fn cell(&self, c: usize) -> &[u32] {
        let a = usize::try_from(self.start[c]).expect("csr start");
        let b = usize::try_from(self.start[c + 1]).expect("csr end");
        &self.items[a..b]
    }

    #[must_use]
    pub fn occupancy(&self) -> u32 {
        self.start[GRID_CELLS]
    }
}

/// One candidate: squared distance first, then the unique asset id. Ordering
/// tuples of these is what makes every nearest-enemy answer total.
pub type Candidate = (i64, u32);

/// A flat view of one searchable population.
///
/// Positions and seats are borrowed from the caller's SoA tables, so the query
/// reads the same memory the rest of the tick does — which is the point of the
/// SoA layout and is what plan §3 step 3's "time per unit" is measuring.
#[derive(Clone, Copy, Debug)]
pub struct Population<'a> {
    pub grid: &'a Grid,
    pub pos: &'a [[Fx; 3]],
    pub seat: &'a [u8],
    pub id: &'a [u32],
}

/// Fold one cell's enemies into `best`.
fn scan_cell(
    p: Population<'_>,
    from: [Fx; 3],
    my_seat: u8,
    x: i32,
    y: i32,
    best: &mut Option<Candidate>,
    examined: &mut u32,
) {
    let w = i32::try_from(GRID_W).expect("grid width fits i32");
    if x < 0 || y < 0 || x >= w || y >= w {
        return;
    }
    let c = usize::try_from(y * w + x).expect("cell index");
    for &item in p.grid.cell(c) {
        let i = usize::try_from(item).expect("item index");
        if p.seat[i] == my_seat {
            continue;
        }
        *examined += 1;
        let cand: Candidate = (Sq::between(from, p.pos[i]).0, p.id[i]);
        if best.is_none_or(|b| cand < b) {
            *best = Some(cand);
        }
    }
}

/// Scan one Chebyshev ring of cells — its border only, since the interior was
/// covered by the rings before it.
fn scan_ring(
    p: Population<'_>,
    from: [Fx; 3],
    my_seat: u8,
    ring: i32,
    centre: (i32, i32),
    best: &mut Option<Candidate>,
    examined: &mut u32,
) {
    let (cx, cy) = centre;
    if ring == 0 {
        scan_cell(p, from, my_seat, cx, cy, best, examined);
        return;
    }
    let mut x = cx - ring;
    while x <= cx + ring {
        scan_cell(p, from, my_seat, x, cy - ring, best, examined);
        scan_cell(p, from, my_seat, x, cy + ring, best, examined);
        x += 1;
    }
    let mut y = cy - ring + 1;
    while y < cy + ring {
        scan_cell(p, from, my_seat, cx - ring, y, best, examined);
        scan_cell(p, from, my_seat, cx + ring, y, best, examined);
        y += 1;
    }
}

/// Nearest enemy in one population, exactly, with `(d2, id)` as the total order.
#[must_use]
pub fn nearest_enemy(
    p: Population<'_>,
    from: [Fx; 3],
    my_seat: u8,
    examined: &mut u32,
) -> Option<Candidate> {
    let w = i32::try_from(GRID_W).expect("grid width fits i32");
    let c0 = cell_of(from);
    let centre = (i32::from(c0) % w, i32::from(c0) / w);
    let cell_raw = i64::from(CELL_VOXELS) << 16;
    let mut best: Option<Candidate> = None;
    let mut ring: i32 = 0;
    while ring < w {
        // Strictly below, so a tie at the ring boundary is still fully scanned
        // and the `(d2, id)` order stays total.
        if ring >= 1
            && let Some(b) = best
        {
            let edge = i64::from(ring - 1) * cell_raw;
            if b.0 < edge * edge {
                break;
            }
        }
        scan_ring(p, from, my_seat, ring, centre, &mut best, examined);
        ring += 1;
    }
    best
}

/// The same answer by a linear scan. Used by the tests as the oracle, and
/// never by the tick.
#[must_use]
pub fn nearest_enemy_brute(
    pos: &[[Fx; 3]],
    seat: &[u8],
    id: &[u32],
    live: &[bool],
    from: [Fx; 3],
    my_seat: u8,
) -> Option<Candidate> {
    let mut best: Option<Candidate> = None;
    for i in 0..pos.len() {
        if !live[i] || seat[i] == my_seat {
            continue;
        }
        let cand: Candidate = (Sq::between(from, pos[i]).0, id[i]);
        if best.is_none_or(|b| cand < b) {
            best = Some(cand);
        }
    }
    best
}
