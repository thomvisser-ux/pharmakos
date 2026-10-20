// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The core's bore: the voxels under each seat's core that no blast cracks or
//! removes (decisions-log item 144 (1); S2's plan, E31).
//!
//! # What the bore is
//!
//! Spec section 5 runs the core's autocannon and its surplus "off the deep
//! bore", and item 144 (1) makes the bore never detachable; S2's `fire` makes
//! it unbreakable. It had no geometry until now ([`crate::power`] names it only
//! as the core's supply), so this module gives it one: **the core anchor's
//! footprint columns, from the bedrock layer up to the generated surface**. A
//! beacon is placed on one column (spec section 5), so a core's footprint is
//! that one column, and the bore is the column's voxels from `z = 0` to the
//! top solid voxel the generator left under the core. A wider footprint, if a
//! later stage gives a core one, is additive here.
//!
//! # A derived table, never state
//!
//! The bore is a pure function of the seed, the rules and the occupied seats:
//! the generator places the cores, so it records the bore with the map
//! ([`crate::mapgen::generate`]), and a restore regenerates it with the
//! pristine chunks rather than carrying it. It is never hashed per tick and
//! never in the snapshot, and its own golden pins it
//! (`tests/golden/mapgen/expected.bore.txt`), as the feature table's does
//! (AGENTS.md section 4.8's derived-table rule). Nothing a tick does writes it:
//! a bore voxel keeps its generated material because every blast skips it
//! ([`crate::voxels::VoxelStore::blast`]).

use crate::tables::SeatId;

/// One bore column: the seat whose core stands on it, the column, and the
/// highest voxel of it the bore holds (the generated surface), inclusive.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct BoreColumn {
    /// The seat whose core stands on the column.
    pub seat: SeatId,
    /// East, in whole voxels.
    pub x: i32,
    /// North, in whole voxels.
    pub y: i32,
    /// The highest bore voxel, the column's top solid voxel at generation.
    /// The bore is every voxel of the column from `z = 0` to this.
    pub top: i32,
}

/// Every seat's bore, in seat order: one column per occupied seat today.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Bore {
    columns: Vec<BoreColumn>,
}

impl Bore {
    /// A bore of these columns, kept in `(seat, x, y)` order so the golden
    /// and every walk read it in one order.
    #[must_use]
    pub fn new(mut columns: Vec<BoreColumn>) -> Bore {
        // item 62: the key is the whole column, (seat, x, y, top), and no two
        // columns of a bore are equal, so the order is total.
        columns.sort_unstable();
        Bore { columns }
    }

    /// Every bore column, in seat order.
    #[must_use]
    pub fn columns(&self) -> &[BoreColumn] {
        &self.columns
    }

    /// Whether the voxel `at` is part of the bore.
    #[must_use]
    pub fn contains(&self, at: [i32; 3]) -> bool {
        let [x, y, z] = at;
        self.columns
            .iter()
            .any(|column| column.x == x && column.y == y && z >= 0 && z <= column.top)
    }

    /// Whether any bore column lies inside the box of columns
    /// `[min_x, min_y]` to `[max_x, max_y]`, inclusive: a blast whose ball
    /// misses every column skips the per-voxel test.
    #[must_use]
    pub fn touches(&self, min: [i32; 2], max: [i32; 2]) -> bool {
        let ([lx, ly], [hx, hy]) = (min, max);
        self.columns
            .iter()
            .any(|column| column.x >= lx && column.x <= hx && column.y >= ly && column.y <= hy)
    }
}
