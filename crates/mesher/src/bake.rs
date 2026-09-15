// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The flood-fill light bake, and the neighbour borders a [`ChunkView`] wants.
//!
//! Light is **presentation state**: the sim never carries it (decisions log section 2.7
//! item 92), so it is computed here, from the materials the client marshals across, and
//! baked into vertex colours on remesh (spec section 15's art-pipeline row).
//!
//! # Seed every sky cell, not the lowest one per column
//!
//! Spike G1 section 10.12, verbatim in substance: seeding only `(x, height, z)` leaves any
//! air cell whose only lit neighbour is a sky cell above an adjacent column's floor at
//! light 0 — a tunnel mouth in a cliff, a breached bunker, anything under an overhang
//! renders pitch black, and the whole world's `(material, light)` mask key degenerates to a
//! near-uniform maximum so the mesher is never exercised on light-driven quad fragmentation
//! at all.
//!
//! Seeding *all* of them cost 6.5 M queue entries on G1's world. Seeding exactly those with
//! a **taller horizontal neighbour** is exactly equivalent and costs a handful per column:
//! a sky cell can only lift a neighbour that is not itself sky, its vertical neighbours
//! never qualify (the cell above is sky; the cell below is below sky only at the column's
//! own floor, and that cell is solid by the definition of height), so the condition reduces
//! to "some horizontal neighbour column is taller than y".
//!
//! # Reach and pad are derived, never asserted
//!
//! [`LightParams::reach`] is `light_max / light_atten - 1` and [`LightParams::pad`] is
//! `reach + 1`. The two terms compose rather than compete: the light around an edit changes
//! out to `reach` voxels, and the mesher reads the *neighbour* cell one voxel outside the
//! chunk, so a light change at the edge of its reach still alters a face one voxel further
//! out. G1 found the old constant right for the wrong reason, and one short the moment the
//! attenuation changed — which is why nothing here is written down as a number.
//!
//! **With the committed rules table** (`light_max` 15, `light_atten` 1) the reach is 14
//! voxels and the pad is 15 — most of a chunk. G1 measured a dirty fan-out of at most 8
//! chunks per explosion, but that was at `light_atten` 3, where the pad is 5. At
//! attenuation 1 the pad crosses a chunk boundary from almost anywhere inside a chunk, so
//! the fan-out per edit is larger, and the drain queue absorbs more re-meshing per
//! explosion than G1's figure suggests.
//!
//! PLACEHOLDER: `light_max` and `light_atten` are Tuning values, owner decides at S6's art
//! polish; the fan-out they imply at the chosen pair is a number T12 measures on the first
//! real vista, against G1's 8 chunks per explosion at radius 4. Nothing in this crate may
//! choose them — they are rules-table rows, and the mesher cannot read the rules table.

use std::collections::VecDeque;
use std::fmt;

use crate::{
    AIR, BorderView, CHUNK_EDGE, CHUNK_VOLUME, ChunkGrid, ChunkView, FACE_AREA, Face, voxel_index,
};

/// Why a [`LightParams`] or a bake call was refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LightError {
    /// `light_max` was zero, or `light_atten` was zero or larger than `light_max`.
    Parameters {
        /// The maximum that was offered.
        light_max: u8,
        /// The attenuation that was offered.
        light_atten: u8,
    },
    /// The material array handed in was not the map's voxel count.
    Materials {
        /// The length that was offered.
        len: usize,
        /// The length the grid says the map has.
        expected: usize,
    },
}

impl fmt::Display for LightError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Parameters {
                light_max,
                light_atten,
            } => write!(
                f,
                "light_max must be at least 1 and light_atten between 1 and light_max, got \
                 light_max {light_max} and light_atten {light_atten}"
            ),
            Self::Materials { len, expected } => write!(
                f,
                "the map's materials must be {expected} bytes for this chunk grid, got {len}"
            ),
        }
    }
}

impl std::error::Error for LightError {}

/// The two rules-table rows the light bake needs, checked once at the boundary.
///
/// The caller fills these from `rules/rules.v1.json`'s `mesher` block — `light_max` and
/// `light_atten`. The mesher cannot read that file: it has no sim dependency, and the wall
/// is what stops it acquiring one.
///
/// The same value drives the shading ramp in [`Mesher`](crate::Mesher), which is why one
/// struct carries both uses: a bake and a shade that disagree about the maximum would give
/// a world that is correctly lit and wrongly coloured.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LightParams {
    light_max: u8,
    light_atten: u8,
}

impl LightParams {
    /// Takes the two rows, checking them.
    ///
    /// # Errors
    ///
    /// [`LightError::Parameters`] when `light_max` is zero, when `light_atten` is zero, or
    /// when the attenuation exceeds the maximum — each of which makes the reach meaningless
    /// rather than merely small.
    pub fn new(light_max: u8, light_atten: u8) -> Result<Self, LightError> {
        if light_max == 0 || light_atten == 0 || light_atten > light_max {
            return Err(LightError::Parameters {
                light_max,
                light_atten,
            });
        }
        Ok(Self {
            light_max,
            light_atten,
        })
    }

    /// The sky's light value, and the value a fully lit face is shaded at.
    #[must_use]
    pub const fn light_max(self) -> u8 {
        self.light_max
    }

    /// How much light one step of propagation costs.
    #[must_use]
    pub const fn light_atten(self) -> u8 {
        self.light_atten
    }

    /// How far light travels from a fully lit cell, in voxels: `light_max / light_atten - 1`.
    ///
    /// Derived, never asserted. Propagation is terminal at or below the attenuation — a
    /// cell with `light <= light_atten` lifts nothing — so a cell at `light_max` changes
    /// cells up to this many voxels away and no further.
    #[must_use]
    pub fn reach(self) -> i32 {
        let steps = u32::from(self.light_max)
            .checked_div(u32::from(self.light_atten))
            .unwrap_or(1);
        i32::try_from(steps.saturating_sub(1)).unwrap_or(0)
    }

    /// How far outside an edit the dirty set must reach, in voxels: [`reach`] plus one.
    ///
    /// The plus one is the mesher's own: it reads the neighbour cell one voxel outside the
    /// chunk, so a light change at the edge of the reach still alters a face one voxel
    /// further out. The two terms compose (G1 section 10.12).
    ///
    /// [`reach`]: LightParams::reach
    #[must_use]
    pub fn pad(self) -> i32 {
        self.reach().saturating_add(1)
    }
}

/// The map's baked light, owned by this crate.
///
/// Chunk-major, like the materials it is baked from: chunk `ci`'s light is the contiguous
/// [`CHUNK_VOLUME`] run at `ci * CHUNK_VOLUME`, in [`voxel_index`] order, so handing a
/// chunk's slice to a [`ChunkView`] is a borrow rather than a gather.
///
/// The materials are **not** owned here. They belong to the caller, which marshals them out
/// of the sim; every call that needs them takes them as a borrow of the same chunk-major
/// layout, `grid.voxel_count()` bytes long.
#[derive(Clone, Debug)]
pub struct LightField {
    grid: ChunkGrid,
    params: LightParams,
    /// One byte per voxel of the map, chunk-major.
    light: Vec<u8>,
    /// The topmost solid voxel plus one, per column, indexed `x + voxels_x * z`.
    heights: Vec<u16>,
    /// The flood fill's queue, kept so a rebake allocates nothing after the first.
    queue: VecDeque<[i32; 3]>,
}

impl LightField {
    /// An unlit field for `grid`, ready to bake.
    #[must_use]
    pub fn new(grid: ChunkGrid, params: LightParams) -> Self {
        let [voxels_x, _, voxels_z] = grid.voxels();
        let columns =
            usize::try_from(voxels_x).unwrap_or(0) * usize::try_from(voxels_z).unwrap_or(0);
        Self {
            grid,
            params,
            light: vec![0_u8; grid.voxel_count()],
            heights: vec![0_u16; columns],
            queue: VecDeque::new(),
        }
    }

    /// The grid this field covers.
    #[must_use]
    pub const fn grid(&self) -> ChunkGrid {
        self.grid
    }

    /// The parameters this field was baked with.
    #[must_use]
    pub const fn params(&self) -> LightParams {
        self.params
    }

    /// One chunk's light, in [`voxel_index`] order — what a [`ChunkView`] takes.
    ///
    /// Returns an empty slice for a chunk outside the grid, which is the same thing the
    /// caller would have to do with a [`None`].
    #[must_use]
    pub fn chunk_light(&self, chunk: u32) -> &[u8] {
        let at = usize::try_from(chunk).unwrap_or(usize::MAX);
        let from = at.saturating_mul(CHUNK_VOLUME);
        self.light
            .get(from..from.saturating_add(CHUNK_VOLUME))
            .unwrap_or(&[])
    }

    /// The whole field, chunk-major. For [`ChunkBorders::gather`], which walks neighbours.
    #[must_use]
    pub fn all(&self) -> &[u8] {
        &self.light
    }

    /// Bakes the whole map from scratch.
    ///
    /// # Errors
    ///
    /// [`LightError::Materials`] when `materials` is not `grid.voxel_count()` bytes.
    pub fn bake_all(&mut self, materials: &[u8]) -> Result<(), LightError> {
        self.check(materials)?;
        let [voxels_x, voxels_y, voxels_z] = self.grid.voxels();
        self.light.fill(0);
        self.recompute_heights(materials, 0, voxels_x - 1, 0, voxels_z - 1);

        self.queue.clear();
        for z in 0..voxels_z {
            for x in 0..voxels_x {
                let floor = self.height_at(x, z);
                for y in floor..voxels_y {
                    self.set_light(x, y, z, self.params.light_max);
                }
                // Every sky cell is a seed. Seeding exactly those with a taller horizontal
                // neighbour is equivalent and costs a handful per column — the module docs
                // carry the argument.
                let tallest = self.tallest_neighbour(x, z).min(voxels_y);
                for y in floor..tallest {
                    self.queue.push_back([x, y, z]);
                }
            }
        }
        self.flood(
            materials,
            [0, 0, 0],
            [voxels_x - 1, voxels_y - 1, voxels_z - 1],
        );
        Ok(())
    }

    /// Re-bakes the inclusive box `lo..=hi` after an edit, joining up with the light
    /// outside it.
    ///
    /// The caller passes the **edited** box; this widens it by nothing, because the caller
    /// is the one that knows how far the edit reached. Pass the box padded by
    /// [`LightParams::pad`] if what you have is the edit rather than its consequences.
    ///
    /// # Errors
    ///
    /// [`LightError::Materials`] when `materials` is not `grid.voxel_count()` bytes.
    pub fn rebake_box(
        &mut self,
        materials: &[u8],
        lo: [i32; 3],
        hi: [i32; 3],
    ) -> Result<(), LightError> {
        self.check(materials)?;
        let [voxels_x, voxels_y, voxels_z] = self.grid.voxels();
        let lo = [lo[0].max(0), lo[1].max(0), lo[2].max(0)];
        let hi = [
            hi[0].min(voxels_x - 1),
            hi[1].min(voxels_y - 1),
            hi[2].min(voxels_z - 1),
        ];
        if lo[0] > hi[0] || lo[1] > hi[1] || lo[2] > hi[2] {
            return Ok(());
        }

        // Heights are recomputed over whole columns: an edit at the top of a column moves
        // the column's floor, and the sky seeds below depend on it.
        self.recompute_heights(materials, lo[0], hi[0], lo[2], hi[2]);

        for z in lo[2]..=hi[2] {
            for x in lo[0]..=hi[0] {
                for y in lo[1]..=hi[1] {
                    self.set_light(x, y, z, 0);
                }
            }
        }

        self.queue.clear();
        // Sky seeds inside the box: every sky cell, for the reason the module docs give.
        // The box is small, so this one does not need the neighbour-height trick.
        for z in lo[2]..=hi[2] {
            for x in lo[0]..=hi[0] {
                let floor = self.height_at(x, z);
                for y in floor.max(lo[1])..=hi[1] {
                    self.set_light(x, y, z, self.params.light_max);
                    self.queue.push_back([x, y, z]);
                }
            }
        }
        self.push_shell_seeds(materials, lo, hi);
        self.flood(materials, lo, hi);
        Ok(())
    }

    /// The chunks an edit of the inclusive box `lo..=hi` dirties, in chunk-index order.
    ///
    /// The box is widened by [`LightParams::pad`] first, because the light around the edit
    /// changes that far out and the mesher reads one voxel further still. This is the
    /// fan-out G1 measured at 8 chunks per explosion with a pad of 5; at the committed
    /// table's pad of 15 it is larger, and that difference is the number T12 measures.
    ///
    /// Appends to `out` without clearing it, so a caller batching several edits allocates
    /// once. Duplicates are possible across calls and are the [`DrainQueue`]'s to coalesce.
    ///
    /// [`DrainQueue`]: crate::DrainQueue
    pub fn dirty_chunks(&self, lo: [i32; 3], hi: [i32; 3], out: &mut Vec<u32>) {
        let pad = self.params.pad();
        let edge = i32::try_from(CHUNK_EDGE).unwrap_or(32);
        let [chunks_x, chunks_y, chunks_z] = [
            i32::try_from(self.grid.chunks_x()).unwrap_or(0),
            i32::try_from(self.grid.chunks_y()).unwrap_or(0),
            i32::try_from(self.grid.chunks_z()).unwrap_or(0),
        ];
        let first = |value: i32, count: i32| -> i32 {
            value
                .saturating_sub(pad)
                .max(0)
                .checked_div(edge)
                .unwrap_or(0)
                .min(count.saturating_sub(1))
        };
        let last = |value: i32, count: i32| -> i32 {
            value
                .saturating_add(pad)
                .max(0)
                .checked_div(edge)
                .unwrap_or(0)
                .min(count.saturating_sub(1))
        };
        // The loop nesting is the chunk index's own order — y slowest, then z, then x — so
        // `out` comes back ascending without a sort.
        for cy in first(lo[1], chunks_y)..=last(hi[1], chunks_y) {
            for cz in first(lo[2], chunks_z)..=last(hi[2], chunks_z) {
                for cx in first(lo[0], chunks_x)..=last(hi[0], chunks_x) {
                    if let Some(index) = self.grid.chunk_index([cx, cy, cz]) {
                        out.push(index);
                    }
                }
            }
        }
    }

    /// The light value at a map voxel, or the sky's value outside the map.
    ///
    /// Outside is sky because there is nothing there to cast a shadow, and a rim of black
    /// faces around the map is the one thing this cannot be mistaken for.
    #[must_use]
    pub fn light_at(&self, x: i32, y: i32, z: i32) -> u8 {
        match self.voxel_offset(x, y, z) {
            Some(at) => self.light.get(at).copied().unwrap_or(0),
            None => self.params.light_max,
        }
    }

    /// The topmost solid voxel plus one in column `(x, z)`, or 0 outside the map.
    fn height_at(&self, x: i32, z: i32) -> i32 {
        let Some(at) = self.column_offset(x, z) else {
            return 0;
        };
        i32::from(self.heights.get(at).copied().unwrap_or(0))
    }

    /// The tallest of the four horizontal neighbour columns. Outside the map a column is
    /// closed solid and contributes nothing.
    fn tallest_neighbour(&self, x: i32, z: i32) -> i32 {
        let mut tallest = 0_i32;
        for [dx, dz] in [[1_i32, 0_i32], [-1, 0], [0, 1], [0, -1]] {
            tallest = tallest.max(self.height_at(x.saturating_add(dx), z.saturating_add(dz)));
        }
        tallest
    }

    /// Recomputes whole-column heights over an inclusive x and z range.
    fn recompute_heights(&mut self, materials: &[u8], x0: i32, x1: i32, z0: i32, z1: i32) {
        let [_, voxels_y, _] = self.grid.voxels();
        for z in z0..=z1 {
            for x in x0..=x1 {
                let mut height = 0_u16;
                let mut y = voxels_y - 1;
                while y >= 0 {
                    if self.material_at(materials, x, y, z) != AIR {
                        height = u16::try_from(y.saturating_add(1)).unwrap_or(u16::MAX);
                        break;
                    }
                    y -= 1;
                }
                if let Some(at) = self.column_offset(x, z) {
                    if let Some(slot) = self.heights.get_mut(at) {
                        *slot = height;
                    }
                }
            }
        }
    }

    /// Queues the lit air cells one voxel outside the box, so the re-bake joins up.
    fn push_shell_seeds(&mut self, materials: &[u8], lo: [i32; 3], hi: [i32; 3]) {
        for z in lo[2] - 1..=hi[2] + 1 {
            for y in lo[1] - 1..=hi[1] + 1 {
                for x in lo[0] - 1..=hi[0] + 1 {
                    let inside = x >= lo[0]
                        && x <= hi[0]
                        && y >= lo[1]
                        && y <= hi[1]
                        && z >= lo[2]
                        && z <= hi[2];
                    if inside || self.voxel_offset(x, y, z).is_none() {
                        continue;
                    }
                    if self.material_at(materials, x, y, z) == AIR
                        && self.light_at(x, y, z) > self.params.light_atten
                    {
                        self.queue.push_back([x, y, z]);
                    }
                }
            }
        }
    }

    /// Breadth-first propagation, writing only inside the inclusive box.
    fn flood(&mut self, materials: &[u8], lo: [i32; 3], hi: [i32; 3]) {
        const NEIGHBOURS: [[i32; 3]; 6] = [
            [1, 0, 0],
            [-1, 0, 0],
            [0, 1, 0],
            [0, -1, 0],
            [0, 0, 1],
            [0, 0, -1],
        ];
        while let Some([x, y, z]) = self.queue.pop_front() {
            let here = self.light_at(x, y, z);
            if here <= self.params.light_atten {
                continue;
            }
            let next = here.saturating_sub(self.params.light_atten);
            for [dx, dy, dz] in NEIGHBOURS {
                let (nx, ny, nz) = (
                    x.saturating_add(dx),
                    y.saturating_add(dy),
                    z.saturating_add(dz),
                );
                if nx < lo[0] || nx > hi[0] || ny < lo[1] || ny > hi[1] || nz < lo[2] || nz > hi[2]
                {
                    continue;
                }
                if self.voxel_offset(nx, ny, nz).is_none()
                    || self.material_at(materials, nx, ny, nz) != AIR
                    || self.light_at(nx, ny, nz) >= next
                {
                    continue;
                }
                self.set_light(nx, ny, nz, next);
                self.queue.push_back([nx, ny, nz]);
            }
        }
    }

    /// Writes one light value, ignoring a write outside the map.
    fn set_light(&mut self, x: i32, y: i32, z: i32, value: u8) {
        let Some(at) = self.voxel_offset(x, y, z) else {
            return;
        };
        if let Some(slot) = self.light.get_mut(at) {
            *slot = value;
        }
    }

    /// The material at a map voxel, or air outside the map.
    fn material_at(&self, materials: &[u8], x: i32, y: i32, z: i32) -> u8 {
        match self.voxel_offset(x, y, z) {
            Some(at) => materials.get(at).copied().unwrap_or(AIR),
            None => AIR,
        }
    }

    /// The chunk-major offset of a map voxel, or [`None`] outside the map.
    fn voxel_offset(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        map_offset(self.grid, x, y, z)
    }

    /// The index of column `(x, z)` in [`LightField::heights`], or [`None`] outside the map.
    fn column_offset(&self, x: i32, z: i32) -> Option<usize> {
        let [voxels_x, _, voxels_z] = self.grid.voxels();
        if x < 0 || z < 0 || x >= voxels_x || z >= voxels_z {
            return None;
        }
        let x = usize::try_from(x).ok()?;
        let z = usize::try_from(z).ok()?;
        let width = usize::try_from(voxels_x).ok()?;
        Some(x + width * z)
    }

    /// Checks the caller's material array against the grid.
    fn check(&self, materials: &[u8]) -> Result<(), LightError> {
        let expected = self.grid.voxel_count();
        if materials.len() == expected {
            Ok(())
        } else {
            Err(LightError::Materials {
                len: materials.len(),
                expected,
            })
        }
    }
}

/// The chunk-major offset of a map voxel, or [`None`] outside the map.
///
/// Chunk-major: the chunk's index times [`CHUNK_VOLUME`], plus [`voxel_index`] within it.
/// Free-standing because [`ChunkBorders::gather`] needs it without a [`LightField`].
#[must_use]
pub fn map_offset(grid: ChunkGrid, x: i32, y: i32, z: i32) -> Option<usize> {
    let [voxels_x, voxels_y, voxels_z] = grid.voxels();
    if x < 0 || y < 0 || z < 0 || x >= voxels_x || y >= voxels_y || z >= voxels_z {
        return None;
    }
    let edge = i32::try_from(CHUNK_EDGE).ok()?;
    let chunk = grid.chunk_index([
        x.checked_div(edge)?,
        y.checked_div(edge)?,
        z.checked_div(edge)?,
    ])?;
    let within = voxel_index(
        usize::try_from(x.checked_rem(edge)?).ok()?,
        usize::try_from(y.checked_rem(edge)?).ok()?,
        usize::try_from(z.checked_rem(edge)?).ok()?,
    );
    usize::try_from(chunk)
        .ok()?
        .checked_mul(CHUNK_VOLUME)?
        .checked_add(within)
}

/// The six neighbour boundary slices one chunk needs, gathered into owned scratch.
///
/// A chunk's `+y` and `-y` boundary layers happen to be contiguous in the neighbour's
/// array, but the other four are strided, so a [`ChunkView`]'s borders cannot in general be
/// borrows of a neighbour chunk. This type is the scratch they are gathered into: build one,
/// keep it, and [`ChunkBorders::gather`] refills it per chunk without allocating.
///
/// **A neighbour outside the map is synthesised as open sky** — air at `light_max` — rather
/// than left absent, so the map's outer rim is lit rather than a band of black faces. A
/// [`ChunkView`] with an absent border treats it as air at light 0, which is the right
/// answer for a caller with no light field and the wrong one here.
#[derive(Clone, Debug)]
pub struct ChunkBorders {
    materials: Vec<u8>,
    light: Vec<u8>,
}

impl Default for ChunkBorders {
    fn default() -> Self {
        Self::new()
    }
}

impl ChunkBorders {
    /// Scratch for six boundary slices, allocated once.
    #[must_use]
    pub fn new() -> Self {
        Self {
            materials: vec![AIR; 6 * FACE_AREA],
            light: vec![0_u8; 6 * FACE_AREA],
        }
    }

    /// Fills the six slices for chunk `chunk` of `grid` from the map's chunk-major
    /// materials and light.
    ///
    /// `light` is [`LightField::all`]. Allocates nothing.
    pub fn gather(
        &mut self,
        grid: ChunkGrid,
        materials: &[u8],
        light: &[u8],
        params: LightParams,
        chunk: u32,
    ) {
        let Some([cx, cy, cz]) = grid.chunk_coords(chunk) else {
            self.materials.fill(AIR);
            self.light.fill(params.light_max());
            return;
        };
        let edge = i32::try_from(CHUNK_EDGE).unwrap_or(32);
        let origin = [
            cx.saturating_mul(edge),
            cy.saturating_mul(edge),
            cz.saturating_mul(edge),
        ];

        for face in Face::ALL {
            let outside = if face.is_positive() { edge } else { -1 };
            let border_base = face.index().saturating_mul(FACE_AREA);
            for row in 0..CHUNK_EDGE {
                for column in 0..CHUNK_EDGE {
                    let quick = i32::try_from(column).unwrap_or(0);
                    let slow = i32::try_from(row).unwrap_or(0);
                    // The border's indexing: the two free axes, x fastest, then z, then y.
                    let local = match face.axis() {
                        0 => [outside, slow, quick], // free axes z (quick) and y (slow)
                        1 => [quick, outside, slow], // free axes x (quick) and z (slow)
                        _ => [quick, slow, outside], // free axes x (quick) and y (slow)
                    };
                    let at = border_base + row * CHUNK_EDGE + column;
                    let (material, value) = match map_offset(
                        grid,
                        origin[0].saturating_add(local[0]),
                        origin[1].saturating_add(local[1]),
                        origin[2].saturating_add(local[2]),
                    ) {
                        Some(offset) => (
                            materials.get(offset).copied().unwrap_or(AIR),
                            light.get(offset).copied().unwrap_or(0),
                        ),
                        // Outside the map: nothing is there, and it is lit as sky.
                        None => (AIR, params.light_max()),
                    };
                    if let Some(target) = self.materials.get_mut(at) {
                        *target = material;
                    }
                    if let Some(target) = self.light.get_mut(at) {
                        *target = value;
                    }
                }
            }
        }
    }

    /// The six borders, as a [`ChunkView`] takes them. Every slot is present.
    #[must_use]
    pub fn views(&self) -> [Option<BorderView<'_>>; 6] {
        let mut out: [Option<BorderView<'_>>; 6] = [None; 6];
        for face in Face::ALL {
            let from = face.index().saturating_mul(FACE_AREA);
            let to = from.saturating_add(FACE_AREA);
            let (Some(materials), Some(light)) =
                (self.materials.get(from..to), self.light.get(from..to))
            else {
                continue;
            };
            if let (Some(slot), Ok(view)) =
                (out.get_mut(face.index()), BorderView::new(materials, light))
            {
                *slot = Some(view);
            }
        }
        out
    }
}

/// The view of one chunk of a baked map: its materials, its light and its six borders.
///
/// All three borrows have to outlive the view, which is why this is a free function taking
/// them together rather than a method on [`LightField`].
///
/// `materials` is the map's chunk-major material array — the same one the bake was given.
///
/// # Errors
///
/// [`crate::ChunkInputError`] when the chunk index is outside the map, which shows up as a
/// materials or light slice of the wrong length.
pub fn chunk_view<'a>(
    materials: &'a [u8],
    field: &'a LightField,
    borders: &'a ChunkBorders,
    chunk: u32,
) -> Result<ChunkView<'a>, crate::ChunkInputError> {
    let at = usize::try_from(chunk).unwrap_or(usize::MAX);
    let from = at.saturating_mul(CHUNK_VOLUME);
    let slice = materials
        .get(from..from.saturating_add(CHUNK_VOLUME))
        .unwrap_or(&[]);
    ChunkView::new(slice, field.chunk_light(chunk), borders.views())
}

#[cfg(test)]
mod tests {
    use super::{ChunkBorders, LightError, LightField, LightParams, chunk_view, map_offset};
    use crate::{AIR, CHUNK_EDGE, ChunkGrid, Face, MeshBuffers, Mesher, voxel_index};

    /// A one-chunk map with a solid floor at y = 0..=3 and nothing else.
    fn floor_map(grid: ChunkGrid) -> Vec<u8> {
        let mut materials = vec![AIR; grid.voxel_count()];
        let [voxels_x, _, voxels_z] = grid.voxels();
        for z in 0..voxels_z {
            for x in 0..voxels_x {
                for y in 0..4 {
                    if let Some(at) = map_offset(grid, x, y, z) {
                        if let Some(slot) = materials.get_mut(at) {
                            *slot = 1;
                        }
                    }
                }
            }
        }
        materials
    }

    #[test]
    fn the_parameters_are_checked() {
        assert!(LightParams::new(0, 1).is_err());
        assert!(LightParams::new(15, 0).is_err());
        assert!(LightParams::new(15, 16).is_err());
        let params = LightParams::new(15, 1).expect("the committed rules-table pair");
        assert_eq!(params.reach(), 14);
        assert_eq!(params.pad(), 15);
        // G1's own pair, for comparison: reach 4, pad 5.
        let spike = LightParams::new(15, 3).expect("G1's pair");
        assert_eq!(spike.reach(), 4);
        assert_eq!(spike.pad(), 5);
    }

    #[test]
    fn the_sky_lights_the_open_air_and_the_floor_stays_dark_inside() {
        let grid = ChunkGrid::new(1, 1, 1).expect("a one-chunk grid is legal");
        let params = LightParams::new(15, 3).expect("G1's pair");
        let materials = floor_map(grid);
        let mut field = LightField::new(grid, params);
        field
            .bake_all(&materials)
            .expect("the map is the right size");

        assert_eq!(field.light_at(5, 10, 5), 15, "open sky");
        assert_eq!(field.light_at(5, 4, 5), 15, "the cell just above the floor");
        assert_eq!(field.light_at(5, 2, 5), 0, "inside the floor");
        // Outside the map is sky, so the rim is lit rather than black.
        assert_eq!(field.light_at(-1, 10, 5), 15);
    }

    #[test]
    fn an_overhang_is_lit_from_beside_it() {
        // A cliff: the left half of the map is 16 voxels tall, the right half 4. A tunnel
        // runs into the cliff at y = 5, whose only lit neighbour is a sky cell above the
        // right half's floor — the cell seeding the lowest sky cell per column would miss.
        let grid = ChunkGrid::new(1, 1, 1).expect("a one-chunk grid is legal");
        let params = LightParams::new(15, 3).expect("G1's pair");
        let mut materials = vec![AIR; grid.voxel_count()];
        let [voxels_x, _, voxels_z] = grid.voxels();
        for z in 0..voxels_z {
            for x in 0..voxels_x {
                let top = if x < 16 { 16 } else { 4 };
                for y in 0..top {
                    if let Some(at) = map_offset(grid, x, y, z) {
                        if let Some(slot) = materials.get_mut(at) {
                            *slot = 1;
                        }
                    }
                }
            }
        }
        // Bore a tunnel into the cliff at y = 5, z = 16, from x = 15 back to x = 10.
        for x in 10..16 {
            if let Some(at) = map_offset(grid, x, 5, 16) {
                if let Some(slot) = materials.get_mut(at) {
                    *slot = AIR;
                }
            }
        }
        let mut field = LightField::new(grid, params);
        field
            .bake_all(&materials)
            .expect("the map is the right size");

        assert_eq!(field.light_at(15, 5, 16), 12, "the tunnel mouth is lit");
        assert!(
            field.light_at(13, 5, 16) > 0,
            "two voxels in, the tunnel is still lit: {}",
            field.light_at(13, 5, 16)
        );
        assert_eq!(field.light_at(10, 5, 16), 0, "and dark at the far end");
    }

    #[test]
    fn a_rebake_matches_a_full_bake() {
        let grid = ChunkGrid::new(2, 1, 2).expect("a 2 x 1 x 2 grid is legal");
        let params = LightParams::new(15, 3).expect("G1's pair");
        let mut materials = floor_map(grid);
        let mut incremental = LightField::new(grid, params);
        incremental.bake_all(&materials).expect("the right size");

        // Blow a hole in the floor, then re-bake the padded box.
        for y in 0..4 {
            for z in 20..24 {
                for x in 20..24 {
                    if let Some(at) = map_offset(grid, x, y, z) {
                        if let Some(slot) = materials.get_mut(at) {
                            *slot = AIR;
                        }
                    }
                }
            }
        }
        let pad = params.pad();
        incremental
            .rebake_box(
                &materials,
                [20 - pad, 0 - pad, 20 - pad],
                [23 + pad, 3 + pad, 23 + pad],
            )
            .expect("the right size");

        let mut fresh = LightField::new(grid, params);
        fresh.bake_all(&materials).expect("the right size");
        assert_eq!(
            incremental.all(),
            fresh.all(),
            "an incremental re-bake must equal a bake from scratch"
        );
    }

    #[test]
    fn a_material_array_of_the_wrong_length_is_refused() {
        let grid = ChunkGrid::new(1, 1, 1).expect("a one-chunk grid is legal");
        let params = LightParams::new(15, 1).expect("the committed pair");
        let mut field = LightField::new(grid, params);
        assert_eq!(
            field.bake_all(&[]).unwrap_err(),
            LightError::Materials {
                len: 0,
                expected: grid.voxel_count()
            }
        );
    }

    #[test]
    fn the_dirty_set_is_the_padded_box_in_index_order() {
        let grid = ChunkGrid::new(4, 2, 4).expect("a 4 x 2 x 4 grid is legal");
        let params = LightParams::new(15, 3).expect("G1's pair, pad 5");
        let field = LightField::new(grid, params);
        let mut dirty = Vec::new();
        // A single voxel at a chunk corner: the pad reaches into all eight chunks around it.
        field.dirty_chunks([32, 32, 32], [32, 32, 32], &mut dirty);
        assert_eq!(dirty.len(), 8, "{dirty:?}");
        let mut sorted = dirty.clone();
        sorted.sort_unstable();
        assert_eq!(
            dirty, sorted,
            "the dirty set comes back in chunk-index order"
        );
    }

    #[test]
    fn a_gathered_border_hides_the_face_across_it() {
        let grid = ChunkGrid::new(2, 1, 1).expect("a 2 x 1 x 1 grid is legal");
        let params = LightParams::new(15, 3).expect("G1's pair");
        let materials = floor_map(grid);
        let mut field = LightField::new(grid, params);
        field.bake_all(&materials).expect("the right size");

        let mut borders = ChunkBorders::new();
        borders.gather(grid, &materials, field.all(), params, 0);
        let view = chunk_view(&materials, &field, &borders, 0).expect("chunk 0 exists");
        let mut mesher = Mesher::new(params);
        let mut out = MeshBuffers::empty();
        mesher
            .mesh_chunk_into(&view, &mut out)
            .expect("well under the cap");

        // The +x neighbour continues the floor, so no +x cap is emitted there; the -x side
        // is the map's rim and is. Four faces of the slab plus top and bottom, minus the +x
        // cap: five quads.
        assert_eq!(out.quad_count(), 5, "quads: {}", out.quad_count());
        assert!(borders.views().iter().all(Option::is_some));
        assert!(view.border(Face::PosX).is_some());
    }

    #[test]
    fn a_chunks_light_slice_is_a_borrow_of_the_field() {
        let grid = ChunkGrid::new(2, 1, 1).expect("a 2 x 1 x 1 grid is legal");
        let params = LightParams::new(15, 1).expect("the committed pair");
        let materials = floor_map(grid);
        let mut field = LightField::new(grid, params);
        field.bake_all(&materials).expect("the right size");
        assert_eq!(
            field.chunk_light(0).len(),
            CHUNK_EDGE * CHUNK_EDGE * CHUNK_EDGE
        );
        assert_eq!(field.chunk_light(9).len(), 0, "outside the grid");
        assert_eq!(
            field.chunk_light(0).get(voxel_index(5, 10, 5)).copied(),
            Some(15)
        );
    }
}
