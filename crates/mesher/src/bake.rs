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
//! [`LightParams::reach`] is `(light_max - 1) / light_atten` and [`LightParams::pad`] is
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
//! PLACEHOLDER: `light_max` and `light_atten`, Tuning values — owner, at S6's art polish.
//! The fan-out they imply at the chosen pair is a number T12 measures on the first
//! real vista, against G1's 8 chunks per explosion at radius 4. Nothing in this crate may
//! choose them — they are rules-table rows, and the mesher cannot read the rules table.

use std::collections::VecDeque;
use std::fmt;
use std::num::NonZeroU8;

use crate::{
    AIR, BorderView, CHUNK_EDGE, CHUNK_VOLUME, ChunkGrid, ChunkView, FACE_AREA, Face, voxel_index,
    widen,
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
    light_max: NonZeroU8,
    light_atten: NonZeroU8,
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
        let refused = LightError::Parameters {
            light_max,
            light_atten,
        };
        let (Some(max), Some(atten)) = (NonZeroU8::new(light_max), NonZeroU8::new(light_atten))
        else {
            return Err(refused);
        };
        if atten > max {
            return Err(refused);
        }
        Ok(Self {
            light_max: max,
            light_atten: atten,
        })
    }

    /// The sky's light value, and the value a fully lit face is shaded at.
    #[must_use]
    pub const fn light_max(self) -> u8 {
        self.light_max.get()
    }

    /// [`LightParams::light_max`], typed as never zero, for the shading ramp's division.
    #[must_use]
    pub(crate) const fn light_max_nonzero(self) -> NonZeroU8 {
        self.light_max
    }

    /// How much light one step of propagation costs.
    #[must_use]
    pub const fn light_atten(self) -> u8 {
        self.light_atten.get()
    }

    /// How far light travels from a fully lit cell, in voxels: `(light_max - 1) /
    /// light_atten`.
    ///
    /// Derived from the flood's own rule, never asserted. [`LightField`]'s propagation
    /// writes a neighbour only when the value it would take is above zero, so the cell `d`
    /// steps from a cell at `light_max` is lit exactly while `light_max` exceeds
    /// `d * light_atten` — that is, while `d <= (light_max - 1) / light_atten`. The
    /// terminal condition (a cell at `light <= light_atten` lifts nothing) is the same
    /// inequality read from the other end and adds no further step.
    ///
    /// **Not `light_max / light_atten - 1`**, which is what this said first and is one
    /// voxel short whenever the attenuation does not divide the maximum: at `light_max` 15
    /// and `light_atten` 2 the flood reaches 7 voxels and that formula says 6, so the pad
    /// built on it leaves a chunk whose faces did change colour off the dirty set. The
    /// committed pair divides exactly, which is why nothing fired — exactly the shape of
    /// failure G1 warns about, one row of the rules table away.
    /// `the_reach_is_the_floods_own_measured_reach` walks the non-dividing pairs against a
    /// real flood rather than against a written-down number.
    #[must_use]
    pub fn reach(self) -> i32 {
        // `light_max` is at least 1, so the subtraction is exact, and the attenuation is a
        // `NonZeroU8`, so the division cannot fail.
        let steps = (self.light_max.get() - 1).div_euclid(self.light_atten.get());
        i32::from(steps)
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
    ///
    /// `i32`, the type of a map coordinate, so a column as tall as any legal grid holds its
    /// height whole.
    heights: Vec<i32>,
    /// The flood fill's queue, kept so a rebake allocates nothing after the first.
    queue: VecDeque<[i32; 3]>,
    /// The light before a [`LightField::relight`], kept so a relight allocates nothing
    /// after the first: a box's copy, or the whole field's before a whole bake.
    previous: Vec<u8>,
}

impl LightField {
    /// An unlit field for `grid`, ready to bake.
    #[must_use]
    pub fn new(grid: ChunkGrid, params: LightParams) -> Self {
        Self {
            grid,
            params,
            light: vec![0_u8; grid.voxel_count()],
            heights: vec![0_i32; grid.columns()],
            queue: VecDeque::new(),
            previous: Vec::new(),
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
    /// Returns an empty slice for a chunk outside the grid, which [`ChunkView::new`]
    /// refuses as [`crate::ChunkInputError::Light`]; [`chunk_view`] refuses such a chunk
    /// itself, as [`crate::ChunkInputError::OutsideGrid`].
    #[must_use]
    pub fn chunk_light(&self, chunk: u32) -> &[u8] {
        self.chunk_light_of(chunk).unwrap_or(&[])
    }

    /// One chunk's light, or [`None`] for a chunk outside the grid.
    fn chunk_light_of(&self, chunk: u32) -> Option<&[u8]> {
        self.light.get(chunk_range(chunk)?)
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
                    self.set_light(x, y, z, self.params.light_max());
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
    /// The caller passes the **edited** box — the voxels it changed — and this widens it by
    /// [`LightParams::pad`] itself, which is the same box [`LightField::dirty_chunks`]
    /// takes and pads. The two are deliberately symmetric: a caller handing the same edit
    /// to both used to get a dirty set that named chunks this call had not re-lit, so the
    /// remesh uploaded stale light. Passing an already-padded box is still correct, merely
    /// wider than it needs to be.
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
        // The pad is this call's, not the caller's — see the doc comment. A box the cut to
        // the map empties re-lights nothing.
        let Some((lo, hi)) = self.padded_box(lo, hi) else {
            return Ok(());
        };

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
                    self.set_light(x, y, z, self.params.light_max());
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
    ///
    /// A box whose padded extent misses the map dirties nothing.
    pub fn dirty_chunks(&self, lo: [i32; 3], hi: [i32; 3], out: &mut Vec<u32>) {
        if let Some((lo, hi)) = self.padded_box(lo, hi) {
            self.chunks_of(lo, hi, out);
        }
    }

    /// Appends the chunks an inclusive box inside the map reaches, in chunk-index order.
    fn chunks_of(&self, lo: [i32; 3], hi: [i32; 3], out: &mut Vec<u32>) {
        // The box is inside the map, so every coordinate is non-negative and the Euclidean
        // division is the chunk it falls in.
        let [x0, y0, z0] = lo.map(|value| value.div_euclid(CHUNK_EDGE_I32));
        let [x1, y1, z1] = hi.map(|value| value.div_euclid(CHUNK_EDGE_I32));
        // The loop nesting is the chunk index's own order — y slowest, then z, then x — so
        // `out` comes back ascending without a sort.
        for cy in y0..=y1 {
            for cz in z0..=z1 {
                for cx in x0..=x1 {
                    if let Some(index) = self.grid.chunk_index([cx, cy, cz]) {
                        out.push(index);
                    }
                }
            }
        }
    }

    /// Re-lights the map after a batch of edits and appends exactly the chunks whose mesh
    /// the batch changed, in chunk-index order, each once.
    ///
    /// `edits` are the **edited** boxes, inclusive, in map voxels: the voxels whose
    /// material changed, which the caller has already written into `materials`. This is the
    /// bounded incremental relight a crater stream needs (S2's plan, task `view`, against
    /// G1's three frames). The light is re-baked box by box ([`LightField::rebake_box`])
    /// while the padded boxes together hold fewer voxels than the map, and in one whole
    /// bake otherwise; that choice is a cost cut with no effect on any pixel, since the two
    /// bakes agree. Either way the light before and after is compared, and a chunk is dirty
    /// only when a voxel that changed, in material or in light, lies inside it or one voxel
    /// outside it, which is exactly what the mesher reads (a [`ChunkView`]'s one-voxel
    /// borders). A re-bake that moves no light therefore dirties no chunk beyond the edit,
    /// where [`LightField::dirty_chunks`] names every chunk the pad reaches.
    ///
    /// # Errors
    ///
    /// [`LightError::Materials`] when `materials` is not `grid.voxel_count()` bytes; nothing
    /// is re-lit then.
    pub fn relight(
        &mut self,
        materials: &[u8],
        edits: &[([i32; 3], [i32; 3])],
        out: &mut Vec<u32>,
    ) -> Result<(), LightError> {
        self.check(materials)?;
        let mut changed: Vec<([i32; 3], [i32; 3])> = Vec::with_capacity(edits.len());
        let padded: Vec<([i32; 3], [i32; 3])> = edits
            .iter()
            .filter_map(|(lo, hi)| self.padded_box(*lo, *hi))
            .collect();
        let volume = padded.iter().fold(0_usize, |sum, (lo, hi)| {
            sum.saturating_add(box_volume(*lo, *hi))
        });
        if volume >= self.grid.voxel_count() {
            self.previous.clone_from(&self.light);
            self.bake_all(materials)?;
            self.whole_changes(&mut changed);
        } else {
            for (lo, hi) in edits {
                let Some((padded_lo, padded_hi)) = self.padded_box(*lo, *hi) else {
                    continue;
                };
                self.snapshot(padded_lo, padded_hi);
                self.rebake_box(materials, *lo, *hi)?;
                if let Some(moved) = self.box_changes(padded_lo, padded_hi) {
                    changed.push(moved);
                }
            }
        }
        changed.extend(edits.iter().copied());
        let from = out.len();
        for (lo, hi) in &changed {
            self.touching(*lo, *hi, out);
        }
        let mut added = out.split_off(from);
        added.sort_unstable();
        added.dedup();
        out.extend(added);
        Ok(())
    }

    /// The edited box widened by the pad and cut to the map, as [`LightField::rebake_box`]
    /// widens it; [`None`] when nothing of it is inside the map.
    fn padded_box(&self, lo: [i32; 3], hi: [i32; 3]) -> Option<([i32; 3], [i32; 3])> {
        self.cut_box(lo, hi, self.params.pad())
    }

    /// The inclusive box widened by `margin` on every side and cut to the map; [`None`]
    /// when nothing of it is inside the map.
    fn cut_box(&self, lo: [i32; 3], hi: [i32; 3], margin: i32) -> Option<([i32; 3], [i32; 3])> {
        let [voxels_x, voxels_y, voxels_z] = self.grid.voxels();
        let widened_lo = lo.map(|value| value.saturating_sub(margin).max(0));
        let widened_hi = [
            hi[0].saturating_add(margin).min(voxels_x - 1),
            hi[1].saturating_add(margin).min(voxels_y - 1),
            hi[2].saturating_add(margin).min(voxels_z - 1),
        ];
        (widened_lo[0] <= widened_hi[0]
            && widened_lo[1] <= widened_hi[1]
            && widened_lo[2] <= widened_hi[2])
            .then_some((widened_lo, widened_hi))
    }

    /// Copies the light inside an inclusive box into the scratch, in the box's own order.
    fn snapshot(&mut self, lo: [i32; 3], hi: [i32; 3]) {
        self.previous.clear();
        for y in lo[1]..=hi[1] {
            for z in lo[2]..=hi[2] {
                for x in lo[0]..=hi[0] {
                    let value = self.light_at(x, y, z);
                    self.previous.push(value);
                }
            }
        }
    }

    /// The bounding box of the voxels inside an inclusive box whose light differs from the
    /// copy [`LightField::snapshot`] took of it.
    fn box_changes(&self, lo: [i32; 3], hi: [i32; 3]) -> Option<([i32; 3], [i32; 3])> {
        let mut bounds: Option<([i32; 3], [i32; 3])> = None;
        let mut before = self.previous.iter();
        for y in lo[1]..=hi[1] {
            for z in lo[2]..=hi[2] {
                for x in lo[0]..=hi[0] {
                    if before.next() != Some(&self.light_at(x, y, z)) {
                        bounds = Some(grow(bounds, [x, y, z]));
                    }
                }
            }
        }
        bounds
    }

    /// One bounding box per chunk whose light differs from the copy of the whole field
    /// taken before a whole bake.
    fn whole_changes(&self, out: &mut Vec<([i32; 3], [i32; 3])>) {
        for index in 0..self.grid.chunk_count() {
            let Some(coords) = self.grid.chunk_coords(index) else {
                continue;
            };
            let corner = coords.map(|value| value.saturating_mul(CHUNK_EDGE_I32));
            let mut bounds: Option<([i32; 3], [i32; 3])> = None;
            for y in corner[1]..corner[1].saturating_add(CHUNK_EDGE_I32) {
                for z in corner[2]..corner[2].saturating_add(CHUNK_EDGE_I32) {
                    for x in corner[0]..corner[0].saturating_add(CHUNK_EDGE_I32) {
                        let differs = self
                            .voxel_offset(x, y, z)
                            .is_some_and(|at| self.light.get(at) != self.previous.get(at));
                        if differs {
                            bounds = Some(grow(bounds, [x, y, z]));
                        }
                    }
                }
            }
            if let Some(found) = bounds {
                out.push(found);
            }
        }
    }

    /// Appends the chunks an inclusive box of changed voxels reaches, one voxel outside it
    /// included, because the mesher reads a neighbour's boundary voxel.
    ///
    /// A box that misses the map, that one voxel included, reaches no chunk.
    fn touching(&self, lo: [i32; 3], hi: [i32; 3], out: &mut Vec<u32>) {
        if let Some((lo, hi)) = self.cut_box(lo, hi, 1) {
            self.chunks_of(lo, hi, out);
        }
    }

    /// The light value at a map voxel, or the sky's value outside the map.
    ///
    /// Outside reads as sky because there is nothing there to cast a shadow, and a rim of
    /// black faces around the map is the one thing this cannot be mistaken for.
    ///
    /// **That value is a shading convention for the border read, not a light source.** The
    /// flood seeds only sky columns *inside* the map, so light never enters across the
    /// map's outer face: a sealed cavity that opens onto the rim renders dark behind a rim
    /// face shaded at `light_max`. The two rules agree everywhere the map's edge is solid,
    /// which is every map the generator makes today.
    ///
    /// PLACEHOLDER: whether the outside of the map lights its interior — owner, at S4's symmetric map, the first place a map could be open at its rim.
    /// Seeding the six
    /// outer faces in [`LightField::bake_all`] — is a lighting rule nothing in the spec or
    /// the decisions log states, so it is not invented here. Owner decides if a map is ever
    /// open at its rim; S4's symmetric map is the first place that could arise, and the
    /// geometry golden moves if the answer is yes.
    #[must_use]
    pub fn light_at(&self, x: i32, y: i32, z: i32) -> u8 {
        self.voxel_offset(x, y, z)
            .and_then(|at| self.light.get(at))
            .map_or(self.params.light_max(), |value| *value)
    }

    /// The topmost solid voxel plus one in column `(x, z)`, or 0 outside the map.
    fn height_at(&self, x: i32, z: i32) -> i32 {
        self.column_offset(x, z)
            .and_then(|at| self.heights.get(at))
            .map_or(0, |height| *height)
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
                let mut height = 0_i32;
                let mut y = voxels_y - 1;
                while y >= 0 {
                    if self.material_at(materials, x, y, z) != AIR {
                        // `y` is below the map's height, an `i32`, so `y + 1` fits.
                        height = y + 1;
                        break;
                    }
                    y -= 1;
                }
                if let Some(at) = self.column_offset(x, z)
                    && let Some(slot) = self.heights.get_mut(at)
                {
                    *slot = height;
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
                        && self.light_at(x, y, z) > self.params.light_atten()
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
            if here <= self.params.light_atten() {
                continue;
            }
            let next = here.saturating_sub(self.params.light_atten());
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
        self.voxel_offset(x, y, z)
            .and_then(|at| materials.get(at))
            .map_or(AIR, |material| *material)
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
        // Every coordinate is non-negative here, so its magnitude is its value, and the
        // column count fits a `usize` (the grid's constructor checked it).
        let (x, z, width) = (
            widen(x.unsigned_abs()),
            widen(z.unsigned_abs()),
            widen(voxels_x.unsigned_abs()),
        );
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
    // Every coordinate is non-negative here, so the Euclidean division and remainder are
    // the chunk and the place within it, and a remainder's magnitude is its value.
    let chunk = grid.chunk_index([
        x.div_euclid(CHUNK_EDGE_I32),
        y.div_euclid(CHUNK_EDGE_I32),
        z.div_euclid(CHUNK_EDGE_I32),
    ])?;
    let local = |value: i32| widen(value.rem_euclid(CHUNK_EDGE_I32).unsigned_abs());
    let within = voxel_index(local(x), local(y), local(z));
    widen(chunk).checked_mul(CHUNK_VOLUME)?.checked_add(within)
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
/// answer for a caller with no light field and the wrong one here. This is the same
/// convention [`LightField::light_at`] applies outside the map, and it has the same limit:
/// it shades the rim, it does not light the interior — the flood never propagates inward
/// from outside the map.
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
        let origin = [
            cx.saturating_mul(CHUNK_EDGE_I32),
            cy.saturating_mul(CHUNK_EDGE_I32),
            cz.saturating_mul(CHUNK_EDGE_I32),
        ];

        // The scratch is six FACE_AREA slices in `Face::ALL` order, and each slice is walked
        // in the border's own indexing: the two free axes, the quicker one fastest.
        let faces = self
            .materials
            .as_chunks_mut::<FACE_AREA>()
            .0
            .iter_mut()
            .zip(self.light.as_chunks_mut::<FACE_AREA>().0.iter_mut());
        for (face, (border_materials, border_light)) in Face::ALL.into_iter().zip(faces) {
            let outside = if face.is_positive() {
                CHUNK_EDGE_I32
            } else {
                -1
            };
            let cells = (0..CHUNK_EDGE_I32)
                .flat_map(|slow| (0..CHUNK_EDGE_I32).map(move |quick| (slow, quick)))
                .zip(border_materials.iter_mut().zip(border_light.iter_mut()));
            for ((slow, quick), (material_slot, light_slot)) in cells {
                let local = match face.axis() {
                    0 => [outside, slow, quick], // free axes z (quick) and y (slow)
                    1 => [quick, outside, slow], // free axes x (quick) and z (slow)
                    _ => [quick, slow, outside], // free axes x (quick) and y (slow)
                };
                // Outside the map nothing is there, and it is lit as sky.
                let (material, value) = map_offset(
                    grid,
                    origin[0].saturating_add(local[0]),
                    origin[1].saturating_add(local[1]),
                    origin[2].saturating_add(local[2]),
                )
                .and_then(|offset| materials.get(offset).zip(light.get(offset)))
                .map_or((AIR, params.light_max()), |(material, value)| {
                    (*material, *value)
                });
                *material_slot = material;
                *light_slot = value;
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
    let outside = crate::ChunkInputError::OutsideGrid {
        chunk,
        chunks: field.grid().chunk_count(),
    };
    if chunk >= field.grid().chunk_count() {
        return Err(outside);
    }
    let range = chunk_range(chunk).ok_or(outside)?;
    // The bytes the map's array holds for this chunk: fewer than a chunk exactly when the
    // array is shorter than the grid says, which `ChunkView::new` would refuse.
    let held = materials.len().saturating_sub(range.start);
    let slice = materials
        .get(range)
        .ok_or(crate::ChunkInputError::Materials { len: held })?;
    let light = field.chunk_light_of(chunk).ok_or(outside)?;
    ChunkView::new(slice, light, borders.views())
}

/// The chunk-major range of chunk `chunk`'s voxels, or [`None`] past `usize`'s reach.
fn chunk_range(chunk: u32) -> Option<std::ops::Range<usize>> {
    let from = widen(chunk).checked_mul(CHUNK_VOLUME)?;
    Some(from..from.checked_add(CHUNK_VOLUME)?)
}

/// [`CHUNK_EDGE`] as an `i32`, for map-voxel arithmetic.
const CHUNK_EDGE_I32: i32 = 32;

// The two spellings of the chunk edge cannot drift apart.
const _: () = assert!(CHUNK_EDGE_I32 as usize == CHUNK_EDGE);

/// The voxels an inclusive box holds; zero for a box with an axis inside out.
fn box_volume(lo: [i32; 3], hi: [i32; 3]) -> usize {
    // Saturating is exact for this function's one use: the volume is compared with the
    // map's voxel count, and any product too large for a `usize` exceeds that.
    lo.iter()
        .zip(hi.iter())
        .map(|(low, high)| {
            if low <= high {
                widen(high.abs_diff(*low)).saturating_add(1)
            } else {
                0
            }
        })
        .fold(1_usize, usize::saturating_mul)
}

/// An inclusive bounding box grown to take in one more voxel.
fn grow(bounds: Option<([i32; 3], [i32; 3])>, at: [i32; 3]) -> ([i32; 3], [i32; 3]) {
    match bounds {
        None => (at, at),
        Some((lo, hi)) => (
            [lo[0].min(at[0]), lo[1].min(at[1]), lo[2].min(at[2])],
            [hi[0].max(at[0]), hi[1].max(at[1]), hi[2].max(at[2])],
        ),
    }
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
                    if let Some(at) = map_offset(grid, x, y, z)
                        && let Some(slot) = materials.get_mut(at)
                    {
                        *slot = 1;
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
        // A pair the attenuation does not divide: the flood reaches 7, not 6.
        let uneven = LightParams::new(15, 2).expect("a legal pair");
        assert_eq!(uneven.reach(), 7);
        assert_eq!(uneven.pad(), 8);
    }

    /// A one-chunk map that is solid except for a roofed corridor at `y = 10, z = 16`
    /// running from `x = 1` to the far wall, lit only through an open shaft at `x = 0`.
    ///
    /// The shaft column's floor is `y = 10`, so every cell above it is a sky seed; every
    /// other way into the corridor is solid. The furthest lit `x` is therefore the flood's
    /// own reach, measured rather than asserted.
    fn corridor_map(grid: ChunkGrid) -> Vec<u8> {
        let [voxels_x, voxels_y, voxels_z] = grid.voxels();
        let mut materials = vec![AIR; grid.voxel_count()];
        for z in 0..voxels_z {
            for x in 0..voxels_x {
                let top = if x == 0 { 10 } else { 21 };
                for y in 0..top.min(voxels_y) {
                    if let Some(at) = map_offset(grid, x, y, z)
                        && let Some(slot) = materials.get_mut(at)
                    {
                        *slot = 1;
                    }
                }
            }
        }
        for x in 1..voxels_x {
            if let Some(at) = map_offset(grid, x, 10, 16)
                && let Some(slot) = materials.get_mut(at)
            {
                *slot = AIR;
            }
        }
        materials
    }

    #[test]
    fn the_reach_is_the_floods_own_measured_reach() {
        let grid = ChunkGrid::new(1, 1, 1).expect("a one-chunk grid is legal");
        let materials = corridor_map(grid);
        // The dividing pairs and, the point of the test, the ones that do not divide.
        for [light_max, light_atten] in [
            [15_u8, 1_u8],
            [15, 2],
            [15, 3],
            [15, 4],
            [15, 5],
            [15, 7],
            [10, 3],
            [16, 3],
            [8, 2],
        ] {
            let params = LightParams::new(light_max, light_atten).expect("a legal pair");
            let mut field = LightField::new(grid, params);
            field
                .bake_all(&materials)
                .expect("the map is the right size");
            let [voxels_x, _, _] = grid.voxels();
            let mut furthest = 0_i32;
            for x in 0..voxels_x {
                if field.light_at(x, 10, 16) > 0 {
                    furthest = furthest.max(x);
                }
            }
            assert_eq!(
                params.reach(),
                furthest,
                "light_max {light_max} / light_atten {light_atten}: the derived reach must \
                 be the flood's own"
            );
        }
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
                    if let Some(at) = map_offset(grid, x, y, z)
                        && let Some(slot) = materials.get_mut(at)
                    {
                        *slot = 1;
                    }
                }
            }
        }
        // Bore a tunnel into the cliff at y = 5, z = 16, from x = 15 back to x = 10.
        for x in 10..16 {
            if let Some(at) = map_offset(grid, x, 5, 16)
                && let Some(slot) = materials.get_mut(at)
            {
                *slot = AIR;
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
                    if let Some(at) = map_offset(grid, x, y, z)
                        && let Some(slot) = materials.get_mut(at)
                    {
                        *slot = AIR;
                    }
                }
            }
        }
        // The **edited** box, unpadded: the call pads it itself, exactly as
        // `dirty_chunks` does, and the two must take the same argument.
        incremental
            .rebake_box(&materials, [20, 0, 20], [23, 3, 23])
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
    fn the_dirty_set_covers_every_chunk_whose_light_moved() {
        // The whole point of deriving the reach: a caller hands the **edited** box to
        // `rebake_box` and to `dirty_chunks`, and no chunk whose faces changed colour may
        // be left off the second. The pair is chosen so the attenuation does not divide
        // the maximum, which is where the old `light_max / light_atten - 1` was one short.
        let grid = ChunkGrid::new(3, 1, 1).expect("a 3 x 1 x 1 grid is legal");
        let params = LightParams::new(15, 2).expect("a legal, non-dividing pair");
        let [voxels_x, voxels_y, voxels_z] = grid.voxels();

        // Solid to y = 20 everywhere, with a roofed corridor at y = 10, z = 16 that ends
        // exactly at the first chunk boundary: chunk 1's solid voxel at x = 32 reads the
        // corridor cell at x = 31 when its -x face is shaded.
        let mut materials = vec![AIR; grid.voxel_count()];
        for z in 0..voxels_z {
            for x in 0..voxels_x {
                for y in 0..21.min(voxels_y) {
                    if let Some(at) = map_offset(grid, x, y, z)
                        && let Some(slot) = materials.get_mut(at)
                    {
                        *slot = 1;
                    }
                }
            }
        }
        for x in 0..CHUNK_EDGE {
            let x = i32::try_from(x).expect("32 fits in an i32");
            if let Some(at) = map_offset(grid, x, 10, 16)
                && let Some(slot) = materials.get_mut(at)
            {
                *slot = AIR;
            }
        }

        let mut field = LightField::new(grid, params);
        field.bake_all(&materials).expect("the right size");
        let before = field.all().to_vec();
        assert_eq!(field.light_at(31, 10, 16), 0, "the corridor starts dark");

        // The edit: a sky shaft down column x = 24, seven voxels from the boundary.
        let (lo, hi) = ([24, 11, 16], [24, 20, 16]);
        for y in lo[1]..=hi[1] {
            if let Some(at) = map_offset(grid, lo[0], y, lo[2])
                && let Some(slot) = materials.get_mut(at)
            {
                *slot = AIR;
            }
        }
        field
            .rebake_box(&materials, lo, hi)
            .expect("the right size");
        let mut fresh = LightField::new(grid, params);
        fresh.bake_all(&materials).expect("the right size");
        assert_eq!(
            field.all(),
            fresh.all(),
            "an unpadded, edited-box re-bake must equal a bake from scratch"
        );
        assert_eq!(
            field.light_at(31, 10, 16),
            1,
            "the corridor is lit out to the boundary: reach 7 from x = 24"
        );

        // Every chunk whose padded read region changed — the 34-cubed region the mesher
        // reads, so one voxel outside the chunk counts.
        let edge = i32::try_from(CHUNK_EDGE).expect("32 fits in an i32");
        let mut changed = Vec::new();
        for index in 0..grid.chunk_count() {
            let coords = grid.chunk_coords(index).expect("inside the grid");
            let origin = [coords[0] * edge, coords[1] * edge, coords[2] * edge];
            let moved = (-1..=edge).any(|dy| {
                (-1..=edge).any(|dz| {
                    (-1..=edge).any(|dx| {
                        let (x, y, z) = (origin[0] + dx, origin[1] + dy, origin[2] + dz);
                        match map_offset(grid, x, y, z) {
                            Some(at) => before.get(at).copied() != field.all().get(at).copied(),
                            None => false,
                        }
                    })
                })
            });
            if moved {
                changed.push(index);
            }
        }
        assert!(
            changed.contains(&1),
            "chunk 1 reads the corridor cell at x = 31, whose light moved"
        );

        let mut dirty = Vec::new();
        field.dirty_chunks(lo, hi, &mut dirty);
        for index in changed {
            assert!(
                dirty.contains(&index),
                "chunk {index} changed but the dirty set is {dirty:?}"
            );
        }
    }

    /// One chunk's mesh: its positions, colours and indices.
    type Meshed = (Vec<[f32; 3]>, Vec<[u8; 4]>, Vec<u16>);

    /// Every chunk's mesh, digested as its buffers: what a relight's dirty set must cover.
    fn meshes(
        grid: ChunkGrid,
        materials: &[u8],
        field: &LightField,
        params: LightParams,
    ) -> Vec<Meshed> {
        let mut mesher = Mesher::new(params);
        let mut borders = ChunkBorders::new();
        let mut buffers = MeshBuffers::empty();
        (0..grid.chunk_count())
            .map(|chunk| {
                borders.gather(grid, materials, field.all(), params, chunk);
                let view = chunk_view(materials, field, &borders, chunk).expect("inside");
                mesher
                    .mesh_chunk_into(&view, &mut buffers)
                    .expect("under the cap");
                (
                    buffers.positions().to_vec(),
                    buffers.colours().to_vec(),
                    buffers.indices().to_vec(),
                )
            })
            .collect()
    }

    /// A crater stream through `relight`, in batches small enough for the box path and
    /// one batch large enough for the whole bake: after each, the light is a bake from
    /// scratch's, and every chunk whose mesh changed is in the dirty set, which names no
    /// chunk twice and comes back in index order.
    #[test]
    fn a_relight_is_a_whole_bake_and_dirties_every_chunk_whose_mesh_moved() {
        let grid = ChunkGrid::new(3, 2, 3).expect("a 3 x 2 x 3 grid is legal");
        let params = LightParams::new(15, 1).expect("the committed pair");
        let [voxels_x, _, voxels_z] = grid.voxels();
        let mut materials = vec![AIR; grid.voxel_count()];
        for z in 0..voxels_z {
            for x in 0..voxels_x {
                for y in 0..24 {
                    if let Some(at) = map_offset(grid, x, y, z)
                        && let Some(slot) = materials.get_mut(at)
                    {
                        *slot = if y < 20 { 1 } else { 2 };
                    }
                }
            }
        }
        let mut field = LightField::new(grid, params);
        field.bake_all(&materials).expect("the right size");
        let mut seed = 0x2545_f491_u32;
        let mut next = |below: i32| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            i32::try_from(seed >> 16).expect("16 bits fit") % below
        };
        // A crater's padded box is at most 41 voxels a side, about 69 000 voxels against
        // this map's 589 824, so the small batches take the box path and the batch of 40
        // the whole bake.
        for batch in [1_usize, 3, 5, 2, 40] {
            let before = meshes(grid, &materials, &field, params);
            let mut edits = Vec::new();
            for _ in 0..batch {
                let centre = [next(voxels_x), 20 + next(6), next(voxels_z)];
                let radius = 2 + next(4);
                let mut bounds: Option<([i32; 3], [i32; 3])> = None;
                for y in centre[1] - radius..=centre[1] + radius {
                    for z in centre[2] - radius..=centre[2] + radius {
                        for x in centre[0] - radius..=centre[0] + radius {
                            let d = [x - centre[0], y - centre[1], z - centre[2]];
                            if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] > radius * radius {
                                continue;
                            }
                            if let Some(at) = map_offset(grid, x, y, z)
                                && let Some(slot) = materials.get_mut(at)
                                && *slot != AIR
                            {
                                *slot = AIR;
                                bounds = Some(super::grow(bounds, [x, y, z]));
                            }
                        }
                    }
                }
                edits.extend(bounds);
            }
            let mut dirty = Vec::new();
            field
                .relight(&materials, &edits, &mut dirty)
                .expect("the right size");
            let mut fresh = LightField::new(grid, params);
            fresh.bake_all(&materials).expect("the right size");
            assert!(
                field.all() == fresh.all(),
                "batch of {batch}: the relit field is a bake from scratch's"
            );
            let mut sorted = dirty.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(dirty, sorted, "batch of {batch}: in index order, each once");
            let after = meshes(grid, &materials, &field, params);
            for (chunk, (was, now)) in before.iter().zip(&after).enumerate() {
                let chunk = u32::try_from(chunk).expect("a chunk index");
                if was != now {
                    assert!(
                        dirty.contains(&chunk),
                        "batch of {batch}: chunk {chunk}'s mesh moved, dirty is {dirty:?}"
                    );
                }
            }
        }
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
        let borders = ChunkBorders::new();
        assert_eq!(
            chunk_view(&materials, &field, &borders, 9).map(|_| ()),
            Err(crate::ChunkInputError::OutsideGrid {
                chunk: 9,
                chunks: 2
            }),
            "a chunk outside the grid is refused as such, not as a short slice"
        );
        assert_eq!(
            chunk_view(
                materials.get(..100).expect("the map is longer"),
                &field,
                &borders,
                0
            )
            .map(|_| ()),
            Err(crate::ChunkInputError::Materials { len: 100 }),
            "a map array shorter than the chunk says how many bytes it holds for it"
        );
    }

    /// An edit whose padded box misses the map dirties nothing, where it used to be pushed
    /// onto the map's edge and dirty the rim chunk there.
    #[test]
    fn an_edit_off_the_map_dirties_nothing() {
        let grid = ChunkGrid::new(2, 1, 1).expect("a 2 x 1 x 1 grid is legal");
        let params = LightParams::new(15, 1).expect("the committed pair");
        let field = LightField::new(grid, params);
        let mut out = Vec::new();
        field.dirty_chunks([-100, 0, 0], [-90, 4, 4], &mut out);
        field.dirty_chunks([200, 0, 0], [210, 4, 4], &mut out);
        assert!(out.is_empty(), "dirtied {out:?}");
        // One inside the pad of the low edge still reaches the first chunk.
        field.dirty_chunks([-10, 0, 0], [-5, 4, 4], &mut out);
        assert_eq!(out, [0]);
    }
}
