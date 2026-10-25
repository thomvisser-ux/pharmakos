// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The generated surface: each column's top solid voxel as the generator left
//! it (decisions-log item 144 (5); S2's plan, task `site`).
//!
//! # What it is for
//!
//! Terraform's `FILL` and `BOTH` refill craters and trenches inside a Build
//! beacon's sphere "back to the generated surface". The chunk store holds the
//! terrain as it is now; this table holds the height it was made at, so a
//! refill knows where to stop. It is read by [`crate::site`]'s refill and by
//! nothing else.
//!
//! # A derived table, never state
//!
//! The surface is a pure function of the seed, the rules and the occupied
//! seats, exactly as the feature table and the core's bore are: the generator
//! records it with the map ([`crate::mapgen::generate`]), a restore regenerates
//! it with the pristine chunks rather than carrying it, and nothing a tick does
//! writes it. So it is never hashed per tick and never in the snapshot, and its
//! own golden pins it (`tests/golden/mapgen/expected.surface.txt`), as
//! AGENTS.md section 4.8's derived-table rule asks.

use crate::encoding::Enc;
use crate::voxels::VoxelStore;

/// One column's top solid voxel at generation, for every column of the map,
/// in `(y, x)` order.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct GeneratedSurface {
    width: i32,
    depth: i32,
    tops: Vec<i16>,
}

/// What a column with no solid voxel at generation reads as: none, which the
/// generator never makes (bedrock is solid, `materials.bedrock_layers_voxels`
/// at least one), and a refill of it would find no ground to stop at.
pub const NO_GROUND: i16 = -1;

impl GeneratedSurface {
    /// Read the surface off a freshly generated store: each column's top solid
    /// voxel, [`NO_GROUND`] for one with none.
    #[must_use]
    pub fn of(store: &VoxelStore) -> GeneratedSurface {
        let [sx, sy, _] = store.size();
        // E7: cannot miss: a map axis is a voxel coordinate (`RulesTable::map_last_voxel`)
        let width = i32::try_from(sx).unwrap_or(0);
        // E7: cannot miss: a map axis is a voxel coordinate (`RulesTable::map_last_voxel`)
        let depth = i32::try_from(sy).unwrap_or(0);
        // E7: cannot miss: two voxel coordinates multiply inside a `usize`
        let columns =
            usize::try_from(i64::from(width).saturating_mul(i64::from(depth))).unwrap_or(0);
        let mut tops = Vec::with_capacity(columns);
        for y in 0..depth {
            for x in 0..width {
                let found = store.top_solid_z(x, y);
                // E7: rule: a column with no solid voxel has no ground; a height is a voxel coordinate
                let top = found
                    .and_then(|z| i16::try_from(z).ok())
                    .unwrap_or(NO_GROUND);
                tops.push(top);
            }
        }
        GeneratedSurface { width, depth, tops }
    }

    /// The column `(x, y)`'s top solid voxel at generation, or `None` off the
    /// map or for a column the generator left with no ground.
    #[must_use]
    pub fn top(&self, x: i32, y: i32) -> Option<i32> {
        if x < 0 || y < 0 || x >= self.width || y >= self.depth {
            return None;
        }
        let index = i64::from(y)
            .checked_mul(i64::from(self.width))?
            .checked_add(i64::from(x))?;
        let top = *self.tops.get(usize::try_from(index).ok()?)?;
        (top != NO_GROUND).then_some(i32::from(top))
    }

    /// How many columns the table holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tops.len()
    }

    /// Whether the table holds no column, which only the default does.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tops.is_empty()
    }

    /// The lowest and the highest top the table holds, or `None` when empty.
    #[must_use]
    pub fn range(&self) -> Option<(i32, i32)> {
        let low = self
            .tops
            .iter()
            .copied()
            .filter(|top| *top != NO_GROUND)
            .min()?;
        let high = self.tops.iter().copied().max()?;
        Some((i32::from(low), i32::from(high)))
    }

    /// One digest over the whole table: its extent, then every top in
    /// `(y, x)` order. The number `tests/golden/mapgen/expected.surface.txt`
    /// commits per seed.
    #[must_use]
    pub fn digest(&self) -> u64 {
        let mut enc = Enc::with_capacity(self.tops.len().saturating_mul(2).saturating_add(16));
        enc.i32(self.width);
        enc.i32(self.depth);
        for top in &self.tops {
            enc.i32(i32::from(*top));
        }
        enc.finish()
    }
}
