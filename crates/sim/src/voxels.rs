// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The 32-cubed copy-on-write chunk store (decisions log items 55, 66 and 92).
//!
//! # What a store is
//!
//! A map of `size_x * size_y * size_z` voxels, one `u8` material each, cut into
//! 32-cubed chunks. The skeleton's map is 384 x 384 x 64 from the rules table's
//! `map` block, which is 12 x 12 x 2 = 288 chunks; the "64-layer cap" the
//! skeleton plan names is `map.size_z`, not a separate rule.
//!
//! **Copy on write, concretely.** A chunk is an `Arc<[u8; 32768]>`. Cloning a
//! whole store clones 288 `Arc`s and nothing else, and the first write to a
//! chunk after a clone goes through [`Arc::make_mut`], which copies that one
//! chunk and leaves its neighbours shared. That is what makes [`crate::world::World`]
//! cheap to clone for a save or a `research` fork, and it is what the snapshot
//! exploits: the *pristine* chunks are exactly the generator's output, so a
//! restore regenerates them from the seed and carries only the modified ones in
//! the file.
//!
//! # The store enters the state hash as per-chunk digests, never as bytes
//!
//! Item 66 measured the alternative: hashing the store's 9.4 MB would cost
//! 0.83 ms, 43x the whole eight-table hash and three times the entire
//! non-pathing tick. [`VoxelStore::settle`] refreshes the digest of each chunk
//! an edit touched, in ascending chunk index, and every other chunk contributes
//! the eight bytes it contributed last tick. The digests themselves live in
//! [`ChunkDigests`], which is the hashed table; this module owns the voxels and
//! the refresh.
//!
//! # The two orders, and why they are written down
//!
//! * **Voxel order inside a chunk** is `x` east fastest, then `y` north, then
//!   `z` up: `index = x + 32*y + 1024*z` (item 92). The per-chunk digest is
//!   taken over the bytes in exactly that order, so the order is determinism
//!   code. `crates/mesher` owns a different order for its own reasons and
//!   `client-gdext` transposes as it marshals; neither crate depends on the
//!   other (item 92, and `cargo xtask wall-guard` keeps it that way).
//! * **Chunk order across the map** is the same shape one level up:
//!   `chunk = cx + chunks_x*cy + chunks_x*chunks_y*cz`. Chunk index order is
//!   the order the digests are encoded in and the order every dirty set and
//!   crater report comes back in.
//!
//! # No division in the address arithmetic
//!
//! The chunk edge is a power of two, so a voxel address splits by a shift and a
//! mask rather than by a division whose rounding would have to be argued about
//! (`clippy::integer_division` is denied workspace-wide, and for a good
//! reason). [`CHUNK_SHIFT`] and [`CHUNK_MASK`] are that power of two, asserted
//! against [`CHUNK_EDGE`] by a compile-time check below.

use std::sync::Arc;

use crate::chunks::ChunkDigests;
use crate::encoding::{Enc, digest};

/// The chunk edge, in voxels. Item 55: 32, and not a rules-table row — it is
/// the snapshot and state-hash layout rather than a tuning value.
pub const CHUNK_EDGE: u32 = 32;

/// `log2(CHUNK_EDGE)`: a voxel address splits by this shift.
pub const CHUNK_SHIFT: u32 = 5;

/// `CHUNK_EDGE - 1`: the in-chunk part of a voxel address.
pub const CHUNK_MASK: u32 = 31;

/// Voxels in one chunk: `32 * 32 * 32`.
pub const CHUNK_VOXELS: usize = 32 * 32 * 32;

// The shift and the edge must describe the same number, or every address in
// this module is wrong in a way no test would obviously localise.
const _: () = assert!(1_u32 << CHUNK_SHIFT == CHUNK_EDGE);
const _: () = assert!(CHUNK_MASK == CHUNK_EDGE - 1);

/// How rich a heat vent or a scrap seam is (spec sections 7 and 9; item 90).
///
/// Three grades and no others, mirroring `gp.v1.ByRichness.Richness`. The sim
/// carries its own enum rather than the generated one so that the wire ids of
/// the *materials* below are this crate's to keep stable.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Richness {
    /// Lean: the grade every seat's starting vent is placed at.
    Lean,
    /// Standard: the grade every seat's starting seam is placed at.
    Standard,
    /// Rich: contested, toward the centre of the map.
    Rich,
}

impl Richness {
    /// Every grade, in ascending order. Walked by the tests and by the
    /// generator's power accounting.
    pub const ALL: [Richness; 3] = [Richness::Lean, Richness::Standard, Richness::Rich];

    /// The wire id. Additive only: never reuse, never renumber.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            Richness::Lean => 1,
            Richness::Standard => 2,
            Richness::Rich => 3,
        }
    }

    /// The grade a `gp.v1.ByRichness.Richness` names, or `None` for the
    /// `RICHNESS_UNSPECIFIED` sentinel and for anything the schema does not
    /// know. An unset richness is a rules-table mistake, never a default
    /// (the schema's own comment says so).
    #[must_use]
    pub const fn from_proto(value: i32) -> Option<Richness> {
        match value {
            1 => Some(Richness::Lean),
            2 => Some(Richness::Standard),
            3 => Some(Richness::Rich),
            _ => None,
        }
    }

    /// The ore material a seam of this grade is made of.
    #[must_use]
    pub const fn ore(self) -> Material {
        match self {
            Richness::Lean => Material::ORE_LEAN,
            Richness::Standard => Material::ORE_STANDARD,
            Richness::Rich => Material::ORE_RICH,
        }
    }

    /// The vent material a heat vent of this grade is made of.
    #[must_use]
    pub const fn vent(self) -> Material {
        match self {
            Richness::Lean => Material::VENT_LEAN,
            Richness::Standard => Material::VENT_STANDARD,
            Richness::Rich => Material::VENT_RICH,
        }
    }
}

/// One voxel's material.
///
/// A `u8` per voxel, because that is the whole store's memory budget: 9.4 MB at
/// the skeleton's map size, measured in G1 and G3'. The ids below are **wire
/// values**: they reach the per-chunk digests and therefore the state hash, so
/// they are additive only — a new material takes the next free id and nothing
/// is ever renumbered (the same rule as [`crate::math::random::Stream::id`]).
///
/// The v1 catalogue, in one place:
///
/// | Id | Name | What it is |
/// |---|---|---|
/// | 0 | [`Material::AIR`] | nothing; the only non-solid material |
/// | 1 | [`Material::DIRT`] | the surface skin of the generated terrain |
/// | 2 | [`Material::STONE`] | the bulk of the terrain below the skin |
/// | 3 | [`Material::ORE_LEAN`] | a scrap seam voxel, lean |
/// | 4 | [`Material::ORE_STANDARD`] | a scrap seam voxel, standard |
/// | 5 | [`Material::ORE_RICH`] | a scrap seam voxel, rich |
/// | 6 | [`Material::VENT_LEAN`] | a heat vent voxel, lean |
/// | 7 | [`Material::VENT_STANDARD`] | a heat vent voxel, standard |
/// | 8 | [`Material::VENT_RICH`] | a heat vent voxel, rich |
///
/// Richness is three materials rather than one material and a side table
/// because a side table would be a second source of truth for something the
/// per-chunk digest already covers — and a side table that is not hashed is
/// exactly the latent desync AGENTS.md section 4.8 is about. Walls, built
/// structures and ruins are S2's and take ids from 9 up.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Material(u8);

impl Material {
    /// Nothing. The only material a unit can stand in rather than on.
    pub const AIR: Material = Material(0);
    /// The surface skin of the generated terrain.
    pub const DIRT: Material = Material(1);
    /// The bulk of the terrain below the skin.
    pub const STONE: Material = Material(2);
    /// A scrap seam voxel, lean.
    pub const ORE_LEAN: Material = Material(3);
    /// A scrap seam voxel, standard.
    pub const ORE_STANDARD: Material = Material(4);
    /// A scrap seam voxel, rich.
    pub const ORE_RICH: Material = Material(5);
    /// A heat vent voxel, lean.
    pub const VENT_LEAN: Material = Material(6);
    /// A heat vent voxel, standard.
    pub const VENT_STANDARD: Material = Material(7);
    /// A heat vent voxel, rich.
    pub const VENT_RICH: Material = Material(8);

    /// Every material v1 defines, in ascending id order.
    pub const ALL: [Material; 9] = [
        Material::AIR,
        Material::DIRT,
        Material::STONE,
        Material::ORE_LEAN,
        Material::ORE_STANDARD,
        Material::ORE_RICH,
        Material::VENT_LEAN,
        Material::VENT_STANDARD,
        Material::VENT_RICH,
    ];

    /// Build from a raw wire id. Any `u8` is representable; an id v1 does not
    /// define is a material this build cannot draw, not an error.
    #[must_use]
    pub const fn from_raw(raw: u8) -> Material {
        Material(raw)
    }

    /// The raw wire id, for the chunk bytes and therefore for the digest.
    #[must_use]
    pub const fn raw(self) -> u8 {
        self.0
    }

    /// Whether this is [`Material::AIR`].
    #[must_use]
    pub const fn is_air(self) -> bool {
        self.0 == Material::AIR.0
    }

    /// Whether a unit can stand on this voxel. Everything but air, today.
    #[must_use]
    pub const fn is_solid(self) -> bool {
        !self.is_air()
    }

    /// The grade of a scrap seam voxel, or `None` when this is not ore.
    #[must_use]
    pub const fn ore_richness(self) -> Option<Richness> {
        match self.0 {
            3 => Some(Richness::Lean),
            4 => Some(Richness::Standard),
            5 => Some(Richness::Rich),
            _ => None,
        }
    }

    /// The grade of a heat vent voxel, or `None` when this is not a vent.
    #[must_use]
    pub const fn vent_richness(self) -> Option<Richness> {
        match self.0 {
            6 => Some(Richness::Lean),
            7 => Some(Richness::Standard),
            8 => Some(Richness::Rich),
            _ => None,
        }
    }

    /// The material's name, for a report or a diagnostic. Not a user-facing
    /// string: those live in one English string table (AGENTS.md section 12).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self.0 {
            0 => "air",
            1 => "dirt",
            2 => "stone",
            3 => "ore_lean",
            4 => "ore_standard",
            5 => "ore_rich",
            6 => "vent_lean",
            7 => "vent_standard",
            8 => "vent_rich",
            _ => "unknown",
        }
    }
}

/// One edit the [`Voxels`](crate::world::Phase::Voxels) phase applies.
///
/// The queue is filled by other phases and drained by the voxel phase, so a
/// write to the store is never interleaved with a read of it inside a tick.
/// Nothing fills the queue yet — combat's craters are S2's and construction's
/// sets are T14's; [`crate::world::World::request_voxel_edit`] documents the
/// order a filler owes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VoxelEdit {
    /// Set one voxel to a material.
    Set {
        /// Where, in whole voxels.
        at: [i32; 3],
        /// What to write.
        material: Material,
    },
    /// Turn a ball of whole voxels to air.
    Crater {
        /// The ball's centre, in whole voxels.
        centre: [i32; 3],
        /// The ball's radius, in whole voxels.
        radius: i32,
    },
}

/// The chunk store: the voxels, their copy-on-write sharing, and the two sets
/// the tick and the presentation side read.
///
/// Not hashed directly: [`VoxelStore::settle`] writes this store's contribution
/// into the [`ChunkDigests`] table, and *that* is what the canonical encoder
/// walks (item 66).
#[derive(PartialEq, Eq)]
pub struct VoxelStore {
    size: [u32; 3],
    chunks_per_axis: [u32; 3],
    chunks: Vec<Arc<[u8; CHUNK_VOXELS]>>,
    /// True once a chunk has been written since generation. Drives the
    /// snapshot: a pristine chunk is regenerated from the seed on restore.
    modified: Vec<bool>,
    /// Edited since the last [`VoxelStore::settle`], with a flag per chunk so a
    /// repeated edit cannot enter the list twice.
    pending: Vec<u32>,
    pending_flag: Vec<bool>,
    /// What the last [`VoxelStore::settle`] refreshed, ascending. The
    /// presentation side drains this; the drain *queue* (K / B / age / camera)
    /// is the mesher's and is not here (item 54).
    settled: Vec<u32>,
    /// Scratch for [`VoxelStore::crater`]'s touched set, reset through the
    /// caller's own list so a crater allocates nothing.
    touch_flag: Vec<bool>,
}

// Written out rather than derived for one reason: a derived `Clone` copies the
// scratch vectors' *lengths* and not their capacities, so a cloned store — a
// save, a `world.clone()`, a `research` fork — would allocate on its first
// settling tick and break the zero-allocations-per-tick property
// (`crates/sim/tests/allocations.rs`, G3' section 9.17) in the clone rather
// than in the original, which is exactly where nobody looks. The values are
// cloned faithfully; only the spare capacity is restored.
impl Clone for VoxelStore {
    fn clone(&self) -> VoxelStore {
        let n = self.chunks.len();
        let mut pending = Vec::with_capacity(n);
        pending.extend_from_slice(&self.pending);
        let mut settled = Vec::with_capacity(n);
        settled.extend_from_slice(&self.settled);
        VoxelStore {
            size: self.size,
            chunks_per_axis: self.chunks_per_axis,
            chunks: self.chunks.clone(),
            modified: self.modified.clone(),
            pending,
            pending_flag: self.pending_flag.clone(),
            settled,
            touch_flag: self.touch_flag.clone(),
        }
    }
}

// `[u8; 32768]` has a `Debug`, and printing 288 of them into an assertion
// message is not a service to anybody. The world derives `Debug`, so the store
// needs one; this is the one that is useful to read.
impl core::fmt::Debug for VoxelStore {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("VoxelStore")
            .field("size", &self.size)
            .field("chunks_per_axis", &self.chunks_per_axis)
            .field("chunks", &self.chunks.len())
            .field("modified", &self.modified.iter().filter(|m| **m).count())
            .field("pending", &self.pending.len())
            .field("settled", &self.settled.len())
            .finish_non_exhaustive()
    }
}

impl VoxelStore {
    /// An all-air store covering `size` voxels.
    ///
    /// Returns `None` unless every axis is positive and a whole number of
    /// chunks — a map that is not a whole number of chunks would leave a partial
    /// chunk whose digest covers voxels that are not on the map, which is a
    /// rules-table mistake caught once here rather than every tick.
    #[must_use]
    pub fn new(size: [u32; 3]) -> Option<VoxelStore> {
        let mut chunks_per_axis = [0_u32; 3];
        for axis in 0..3 {
            let extent = *size.get(axis)?;
            if extent == 0 || extent & CHUNK_MASK != 0 {
                return None;
            }
            *chunks_per_axis.get_mut(axis)? = extent >> CHUNK_SHIFT;
        }
        let count = chunks_per_axis
            .first()?
            .checked_mul(*chunks_per_axis.get(1)?)?
            .checked_mul(*chunks_per_axis.get(2)?)?;
        let n = usize::try_from(count).ok()?;

        let mut chunks: Vec<Arc<[u8; CHUNK_VOXELS]>> = Vec::with_capacity(n);
        // One shared air chunk: the store starts entirely shared and pays for a
        // copy only where the generator writes. Built through a `Vec` rather
        // than as an array literal so the 32 KiB never sits on the stack.
        let zeros: Box<[u8]> = vec![0_u8; CHUNK_VOXELS].into_boxed_slice();
        let boxed: Box<[u8; CHUNK_VOXELS]> = Box::try_from(zeros).ok()?;
        let air: Arc<[u8; CHUNK_VOXELS]> = Arc::from(boxed);
        for _ in 0..n {
            chunks.push(Arc::clone(&air));
        }

        Some(VoxelStore {
            size,
            chunks_per_axis,
            chunks,
            modified: vec![false; n],
            pending: Vec::with_capacity(n),
            pending_flag: vec![false; n],
            settled: Vec::with_capacity(n),
            touch_flag: vec![false; n],
        })
    }

    /// The map extent in voxels, `[x, y, z]`.
    #[must_use]
    pub const fn size(&self) -> [u32; 3] {
        self.size
    }

    /// Chunks along each axis, `[x, y, z]`.
    #[must_use]
    pub const fn chunks_per_axis(&self) -> [u32; 3] {
        self.chunks_per_axis
    }

    /// How many chunks the store covers.
    #[must_use]
    pub fn chunk_count(&self) -> u32 {
        u32::try_from(self.chunks.len()).unwrap_or(u32::MAX)
    }

    /// Whether `at` is a voxel on this map.
    #[must_use]
    pub fn contains(&self, at: [i32; 3]) -> bool {
        self.address(at).is_some()
    }

    /// `at` split into `(chunk index, index within the chunk)`, or `None` when
    /// it is off the map.
    ///
    /// The in-chunk index is `x + 32*y + 1024*z` and the chunk index is
    /// `cx + chunks_x*cy + chunks_x*chunks_y*cz` — see the module docs; both are
    /// determinism code, because the first decides the bytes a digest is taken
    /// over and the second decides the order the digests are encoded in.
    #[must_use]
    pub fn address(&self, at: [i32; 3]) -> Option<(u32, usize)> {
        let mut voxel = [0_u32; 3];
        for axis in 0..3 {
            let v = u32::try_from(*at.get(axis)?).ok()?;
            if v >= *self.size.get(axis)? {
                return None;
            }
            *voxel.get_mut(axis)? = v;
        }
        let (x, y, z) = (*voxel.first()?, *voxel.get(1)?, *voxel.get(2)?);
        let chunk = (x >> CHUNK_SHIFT)
            + (y >> CHUNK_SHIFT) * self.chunks_per_axis.first()?
            + (z >> CHUNK_SHIFT) * self.chunks_per_axis.first()? * self.chunks_per_axis.get(1)?;
        let inside = (x & CHUNK_MASK)
            + ((y & CHUNK_MASK) << CHUNK_SHIFT)
            + ((z & CHUNK_MASK) << (CHUNK_SHIFT * 2));
        Some((chunk, usize::try_from(inside).ok()?))
    }

    /// The origin voxel of a chunk, or `None` when the index is off the map.
    #[must_use]
    pub fn chunk_origin(&self, chunk: u32) -> Option<[i32; 3]> {
        if chunk >= self.chunk_count() {
            return None;
        }
        let across = *self.chunks_per_axis.first()?;
        let plane = across.checked_mul(*self.chunks_per_axis.get(1)?)?;
        let cz = chunk.checked_div(plane)?;
        let rest = chunk.checked_rem(plane)?;
        let cy = rest.checked_div(across)?;
        let cx = rest.checked_rem(across)?;
        Some([
            i32::try_from(cx << CHUNK_SHIFT).ok()?,
            i32::try_from(cy << CHUNK_SHIFT).ok()?,
            i32::try_from(cz << CHUNK_SHIFT).ok()?,
        ])
    }

    /// The material at `at`, or `None` when it is off the map.
    #[must_use]
    pub fn get(&self, at: [i32; 3]) -> Option<Material> {
        let (chunk, inside) = self.address(at)?;
        let bytes = self.chunks.get(usize::try_from(chunk).ok()?)?;
        Some(Material::from_raw(*bytes.get(inside)?))
    }

    /// Write one voxel. Returns `true` when the material actually changed.
    ///
    /// A write that changes nothing marks nothing: it neither dirties the
    /// chunk's digest nor makes the chunk modified, so a no-op edit cannot grow
    /// a snapshot or move a digest.
    pub fn set(&mut self, at: [i32; 3], material: Material) -> bool {
        let Some((chunk, inside)) = self.address(at) else {
            return false;
        };
        let Ok(index) = usize::try_from(chunk) else {
            return false;
        };
        let Some(slot) = self.chunks.get_mut(index) else {
            return false;
        };
        if slot.get(inside).copied() == Some(material.raw()) {
            return false;
        }
        // Copy on write: this is the only place a chunk is un-shared.
        let data = Arc::make_mut(slot);
        let Some(cell) = data.get_mut(inside) else {
            return false;
        };
        *cell = material.raw();
        self.mark(chunk);
        true
    }

    // -----------------------------------------------------------------------
    // The pristine layer: written by the generator, never by a tick
    // -----------------------------------------------------------------------

    /// Write one voxel of the **pristine** layer.
    ///
    /// Generation only, and `crate::mapgen` is the only caller. It does not
    /// mark the chunk modified and it does not dirty the chunk's digest,
    /// because the pristine layer is by definition what a restore regenerates
    /// from the seed and what [`VoxelStore::settle_all`] digests once at
    /// construction. The edit path a tick uses is [`VoxelStore::set`].
    ///
    /// Returns `true` when the material actually changed.
    pub(crate) fn set_pristine(&mut self, at: [i32; 3], material: Material) -> bool {
        let Some((chunk, inside)) = self.address(at) else {
            return false;
        };
        let Ok(index) = usize::try_from(chunk) else {
            return false;
        };
        let Some(slot) = self.chunks.get_mut(index) else {
            return false;
        };
        if slot.get(inside).copied() == Some(material.raw()) {
            return false;
        }
        let data = Arc::make_mut(slot);
        let Some(cell) = data.get_mut(inside) else {
            return false;
        };
        *cell = material.raw();
        true
    }

    /// Fill the pristine layer from a heightmap.
    ///
    /// `height[y * size_x + x]` is how many solid voxels the column carries, so
    /// `z` in `0..height` is solid and everything above it is air. The top
    /// `skin_depth` voxels of a column are `skin` and the rest are `bulk`.
    ///
    /// Written as a bulk fill rather than as a few million [`VoxelStore::set`]
    /// calls for one reason and it is not elegance: the generator runs once per
    /// `World::new`, `World::new` runs dozens of times in this crate's test
    /// suite, and the per-voxel path costs an address decode and a
    /// copy-on-write check each time. This walks chunk by chunk and un-shares
    /// each chunk once.
    ///
    /// Returns how many voxels were written.
    pub(crate) fn fill_from_heightmap(
        &mut self,
        height: &[i32],
        skin_depth: i32,
        skin: Material,
        bulk: Material,
    ) -> u32 {
        let Ok(width) = i32::try_from(self.size.first().copied().unwrap_or(0)) else {
            return 0;
        };
        let edge = i32::try_from(CHUNK_EDGE).unwrap_or(0);
        let mut written: u32 = 0;
        let mut chunk: u32 = 0;
        while chunk < self.chunk_count() {
            let Some(origin) = self.chunk_origin(chunk) else {
                chunk = chunk.saturating_add(1);
                continue;
            };
            let (ox, oy, oz) = (
                origin.first().copied().unwrap_or(0),
                origin.get(1).copied().unwrap_or(0),
                origin.get(2).copied().unwrap_or(0),
            );
            let Ok(index) = usize::try_from(chunk) else {
                chunk = chunk.saturating_add(1);
                continue;
            };
            let Some(slot) = self.chunks.get_mut(index) else {
                chunk = chunk.saturating_add(1);
                continue;
            };
            let data = Arc::make_mut(slot);
            let mut ly: i32 = 0;
            while ly < edge {
                let mut lx: i32 = 0;
                while lx < edge {
                    let column = i64::from(oy.saturating_add(ly))
                        .saturating_mul(i64::from(width))
                        .saturating_add(i64::from(ox.saturating_add(lx)));
                    let top = usize::try_from(column)
                        .ok()
                        .and_then(|c| height.get(c))
                        .copied()
                        .unwrap_or(0);
                    let hi = top.min(oz.saturating_add(edge));
                    let mut z = oz;
                    while z < hi {
                        let material = if z >= top.saturating_sub(skin_depth) {
                            skin
                        } else {
                            bulk
                        };
                        let inside = lx
                            .saturating_add(ly << CHUNK_SHIFT)
                            .saturating_add(z.saturating_sub(oz) << (CHUNK_SHIFT * 2));
                        if let Ok(at) = usize::try_from(inside) {
                            if let Some(cell) = data.get_mut(at) {
                                *cell = material.raw();
                                written = written.saturating_add(1);
                            }
                        }
                        z = z.saturating_add(1);
                    }
                    lx = lx.saturating_add(1);
                }
                ly = ly.saturating_add(1);
            }
            chunk = chunk.saturating_add(1);
        }
        written
    }

    /// Turn a ball of `radius` whole voxels centred on `centre` to air.
    ///
    /// `touched` is cleared and filled with the chunks in which at least one
    /// voxel actually changed, **ascending by chunk index**. It is the caller's
    /// buffer, so a crater inside a tick allocates nothing once the buffer has
    /// been used once. The return value is how many voxels became air.
    ///
    /// The ball is the same shape the three spikes used,
    /// `dx^2 + dy^2 + dz^2 <= r^2`, so G1's and G3''s destruction measurements
    /// describe this primitive.
    pub fn crater(&mut self, centre: [i32; 3], radius: i32, touched: &mut Vec<u32>) -> u32 {
        touched.clear();
        if radius < 0 {
            return 0;
        }
        let Some(r2) = radius.checked_mul(radius) else {
            return 0;
        };
        let (cx, cy, cz) = (
            centre.first().copied().unwrap_or(0),
            centre.get(1).copied().unwrap_or(0),
            centre.get(2).copied().unwrap_or(0),
        );

        let mut removed: u32 = 0;
        let mut dz = -radius;
        while dz <= radius {
            let mut dy = -radius;
            while dy <= radius {
                let mut dx = -radius;
                while dx <= radius {
                    let d2 = dx
                        .saturating_mul(dx)
                        .saturating_add(dy.saturating_mul(dy))
                        .saturating_add(dz.saturating_mul(dz));
                    if d2 > r2 {
                        dx = dx.saturating_add(1);
                        continue;
                    }
                    let at = [
                        cx.saturating_add(dx),
                        cy.saturating_add(dy),
                        cz.saturating_add(dz),
                    ];
                    if let Some((chunk, _)) = self.address(at) {
                        if self.set(at, Material::AIR) {
                            removed = removed.saturating_add(1);
                            if let Ok(index) = usize::try_from(chunk) {
                                if matches!(self.touch_flag.get(index), Some(false)) {
                                    if let Some(flag) = self.touch_flag.get_mut(index) {
                                        *flag = true;
                                    }
                                    touched.push(chunk);
                                }
                            }
                        }
                    }
                    dx = dx.saturating_add(1);
                }
                dy = dy.saturating_add(1);
            }
            dz = dz.saturating_add(1);
        }

        for chunk in touched.iter() {
            if let Ok(index) = usize::try_from(*chunk) {
                if let Some(flag) = self.touch_flag.get_mut(index) {
                    *flag = false;
                }
            }
        }
        // item 62: the key IS the chunk index, which is unique in this list by
        // the `touch_flag` guard above, so the order is total.
        touched.sort_unstable();
        removed
    }

    /// Record that `chunk` has been written.
    fn mark(&mut self, chunk: u32) {
        let Ok(index) = usize::try_from(chunk) else {
            return;
        };
        if let Some(flag) = self.modified.get_mut(index) {
            *flag = true;
        }
        if matches!(self.pending_flag.get(index), Some(false)) {
            if let Some(flag) = self.pending_flag.get_mut(index) {
                *flag = true;
            }
            self.pending.push(chunk);
        }
    }

    /// Refresh the digest of every chunk edited since the last call, in
    /// ascending chunk index, and publish that set through
    /// [`VoxelStore::settled`].
    ///
    /// This is the whole of the store's contribution to the state hash: the
    /// encoder never sees a voxel byte. Returns how many digests were
    /// refreshed. Allocation-free once the buffers have been used.
    pub fn settle(&mut self, digests: &mut ChunkDigests) -> u32 {
        // item 62: the key IS the chunk index, unique in `pending` by the flag
        // guard in `mark`, so the order is total.
        self.pending.sort_unstable();
        self.settled.clear();
        let mut refreshed: u32 = 0;
        for chunk in &self.pending {
            let Ok(index) = usize::try_from(*chunk) else {
                continue;
            };
            if let Some(bytes) = self.chunks.get(index) {
                digests.refresh(*chunk, digest(bytes.as_slice()));
                refreshed = refreshed.saturating_add(1);
            }
            if let Some(flag) = self.pending_flag.get_mut(index) {
                *flag = false;
            }
            self.settled.push(*chunk);
        }
        self.pending.clear();
        refreshed
    }

    /// Refresh **every** chunk's digest. Construction and restore only: the
    /// tick uses [`VoxelStore::settle`].
    pub fn settle_all(&mut self, digests: &mut ChunkDigests) {
        for (index, bytes) in self.chunks.iter().enumerate() {
            let Ok(chunk) = u32::try_from(index) else {
                continue;
            };
            digests.refresh(chunk, digest(bytes.as_slice()));
        }
        self.pending_flag.fill(false);
        self.pending.clear();
        self.settled.clear();
    }

    /// The chunks the last [`VoxelStore::settle`] refreshed, ascending.
    ///
    /// The presentation side drains this to learn what to remesh. The drain
    /// *queue* — K surfaces, B bytes, the age term, nearest-camera ordering —
    /// belongs to `crates/mesher` (item 54) and is deliberately not here: the
    /// sim says what changed, the mesher decides what it can afford.
    #[must_use]
    pub fn settled(&self) -> &[u32] {
        &self.settled
    }

    /// One chunk's 32 768 material bytes, borrowed, in this crate's voxel order.
    ///
    /// Item 92: the mesher owns its own `ChunkView` in its own order and
    /// `client-gdext` transposes between the two as it marshals. What the sim
    /// owes is this borrow and the address arithmetic above; it takes no
    /// dependency on the mesher and the mesher takes none on it.
    #[must_use]
    pub fn chunk_bytes(&self, chunk: u32) -> Option<&[u8; CHUNK_VOXELS]> {
        self.chunks
            .get(usize::try_from(chunk).ok()?)
            .map(Arc::as_ref)
    }

    /// Whether a chunk has been written since the store was generated.
    #[must_use]
    pub fn is_modified(&self, chunk: u32) -> bool {
        usize::try_from(chunk)
            .ok()
            .and_then(|index| self.modified.get(index))
            .copied()
            .unwrap_or(false)
    }

    /// Every modified chunk index, ascending — the snapshot's view.
    ///
    /// Allocates, and is meant to: it is called at a save, never inside a tick.
    #[must_use]
    pub fn modified_indices(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for (index, flag) in self.modified.iter().enumerate() {
            if *flag {
                if let Ok(chunk) = u32::try_from(index) {
                    out.push(chunk);
                }
            }
        }
        out
    }

    /// Overwrite one chunk wholesale and mark it modified. Restore only.
    ///
    /// Returns `false` when the index is off the map or `bytes` is not exactly
    /// [`CHUNK_VOXELS`] long, which is what a truncated snapshot looks like.
    pub(crate) fn restore_chunk(&mut self, chunk: u32, bytes: &[u8]) -> bool {
        if bytes.len() != CHUNK_VOXELS {
            return false;
        }
        let Ok(index) = usize::try_from(chunk) else {
            return false;
        };
        let Some(slot) = self.chunks.get_mut(index) else {
            return false;
        };
        let data = Arc::make_mut(slot);
        data.copy_from_slice(bytes);
        if let Some(flag) = self.modified.get_mut(index) {
            *flag = true;
        }
        true
    }

    /// The `z` of the highest solid voxel in the column at `(x, y)`, or `None`
    /// for an off-map column or one that is air all the way down.
    ///
    /// The generated terrain is a heightmap — no overhangs — so this is *the*
    /// surface of the column and not merely the first one from above. T7's
    /// HPA\* graph is built on it; destruction can turn a column to air, and
    /// `None` is then the honest answer rather than a floor at zero.
    #[must_use]
    pub fn top_solid_z(&self, x: i32, y: i32) -> Option<i32> {
        let top = i32::try_from(*self.size.get(2)?).ok()?.checked_sub(1)?;
        let mut z = top;
        loop {
            if self.get([x, y, z])?.is_solid() {
                return Some(z);
            }
            if z <= 0 {
                return None;
            }
            z = z.saturating_sub(1);
        }
    }

    /// The `z` a unit standing on the column at `(x, y)` occupies: one above
    /// the top solid voxel, or zero for a column with no floor.
    #[must_use]
    pub fn standing_z(&self, x: i32, y: i32) -> i32 {
        self.top_solid_z(x, y)
            .and_then(|z| z.checked_add(1))
            .unwrap_or(0)
    }

    /// One digest over the whole store: the map extent, then every chunk's own
    /// digest in chunk-index order.
    ///
    /// Not part of the tick. It is the number `tests/golden/mapgen` commits per
    /// seed, and the reason it is a digest of digests rather than of bytes is
    /// the same reason the state hash is (item 66).
    #[must_use]
    pub fn store_digest(&self) -> u64 {
        let mut enc = Enc::with_capacity(self.chunks.len().saturating_mul(8).saturating_add(32));
        for axis in self.size {
            enc.u32(axis);
        }
        enc.len(self.chunk_count());
        for bytes in &self.chunks {
            enc.u64(digest(bytes.as_slice()));
        }
        enc.finish()
    }
}
