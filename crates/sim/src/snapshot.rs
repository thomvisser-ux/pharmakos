// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Save and restore (decisions log item 49).
//!
//! [`Snapshot`] is a flat projection of [`World`] whose **fields are all
//! fixed-width**: no `usize` or `isize` field, no pointer, no enum, and every
//! `Option` and every `bool` reduced to a sentinel or a byte, so the encoding
//! has no tag whose layout could differ between targets.
//!
//! The format is **postcard 1.1.x** (`default-features = false`, `use-std`)
//! with serde derives. Measured in G4 at 4 563 B for the toy world at tick
//! 4 800, 0.04 ms to save and 0.24-0.32 ms to restore, byte-identical across
//! ubuntu, windows and macOS. rkyv (7 160 B, a much heavier dependency tree)
//! and a hand-written codec were both rejected.
//!
//! # The chunk store is not in the file, and that is what copy-on-write bought
//!
//! A map is a pure function of `(match seed, rules table, occupied seats)`, so
//! the **pristine** chunks — the generator's own output — are regenerated on
//! restore rather than carried. What the file holds is the list of chunks an
//! edit has touched since generation and their 32 768 bytes each, plus the
//! per-chunk digests exactly as the hash sees them. A match that has destroyed
//! nothing therefore saves nothing of its 9.4 MB map; a match that has cratered
//! forty chunks saves 1.3 MB of them. The digests are carried deliberately:
//! they are what the hash is over, so [`Snapshot::restore_into`] checks the
//! column's length **and** every digest against the store it has just rebuilt
//! and refuses the file with [`SnapshotError::ChunkDigest`] when the two
//! disagree. A file written by another generator, another rules table or an
//! editor is caught at the door rather than by a desync three operating systems
//! later.
//!
//! # The one bend in the "never serialise `usize`" rule, stated at the encoder
//!
//! A `Vec`'s length is supplied by the format, not by us, and postcard writes
//! it as a **canonical LEB128 varint of the host's `usize`** with no width
//! padding. The bytes therefore agree between a 32-bit and a 64-bit host for
//! every length below 2^32, which covers every snapshot this game can produce
//! by many orders of magnitude. That is a *value* identity, not a type-level
//! one. It is written down here rather than waved away because the 32-bit web
//! build on the roadmap lives inside it and a future format change must know
//! the guarantee it is replacing.
//!
//! # The version is in the bytes
//!
//! [`SNAPSHOT_VERSION`] is the first field and is checked on restore. A save
//! from a different version refuses to load rather than producing a world that
//! is subtly the wrong shape. T17 adds the rules hash and the verifier version
//! to the same check.

use crate::chunks::ChunkDigests;
use crate::encoding::digest;
use crate::mapgen::{self, MapError};
use crate::math::fixed::{Angle, Fx};
use crate::math::quantity::{Hp, Kw, Money, Tick};
use crate::tables::{
    BeaconColumns, BeaconTable, SeatTable, StructureColumns, StructureTable, UnitColumns,
    UnitTable, WreckTable,
};
use crate::voxels::{CHUNK_VOXELS, VoxelStore};
use crate::world::{RestoredTables, World};
use serde::{Deserialize, Serialize};

/// Snapshot format version. Part of the bytes, checked on restore.
///
/// A bump is a contract change (AGENTS.md section 5) and invalidates every
/// committed save. **Version 2 is T5's**: it adds the unit kind column, the
/// beacon, structure and wreck tables, and the modified-chunk half of the
/// voxel store.
pub const SNAPSHOT_VERSION: u32 = 2;

/// A flat, fixed-width projection of the world.
///
/// Column-per-field rather than struct-per-row, matching the `SoA` tables: it
/// keeps every field fixed-width and it is what makes the byte count above
/// small enough to save at a Lull boundary without the player noticing.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    /// [`SNAPSHOT_VERSION`] at the time of writing.
    pub version: u32,
    /// The match seed, which is also the map seed
    /// ([`crate::world::WorldConfig::match_seed`]) — so this one number is what
    /// the pristine chunks are regenerated from.
    pub match_seed: u64,
    /// The tick the snapshot was taken at.
    pub tick: u32,

    /// Seat ids.
    pub seat_id: Vec<u8>,
    /// Per-seat treasury, in `$`.
    pub seat_treasury: Vec<i64>,
    /// Per-seat power supply, in `kW`.
    pub seat_supply: Vec<i32>,
    /// Per-seat power draw, in `kW`.
    pub seat_draw: Vec<i32>,

    /// Unit ids.
    pub unit_id: Vec<u32>,
    /// The seat each unit belongs to.
    pub unit_seat: Vec<u8>,
    /// Each unit's [`crate::tables::UnitKind::id`].
    pub unit_kind: Vec<u8>,
    /// Three raw Q16.16 coordinates per unit: x, y, z.
    pub unit_pos: Vec<i32>,
    /// Three raw Q16.16 coordinates per unit's destination.
    pub unit_dest: Vec<i32>,
    /// Raw heading units per unit.
    pub unit_heading: Vec<u16>,
    /// Hit points per unit.
    pub unit_hp: Vec<i32>,

    /// Beacon ids.
    pub beacon_id: Vec<u32>,
    /// The seat each beacon belongs to.
    pub beacon_seat: Vec<u8>,
    /// Three raw Q16.16 coordinates per beacon.
    pub beacon_pos: Vec<i32>,
    /// Each beacon's [`crate::seams::MandateKind::id`].
    pub beacon_mandate: Vec<u8>,
    /// Each beacon's `program_id` seam.
    pub beacon_program: Vec<u32>,
    /// Hit points per beacon.
    pub beacon_hp: Vec<i32>,
    /// Dormancy per beacon, `0` or `1` — a byte rather than a `bool`, so the
    /// encoding has no type whose width the format decides.
    pub beacon_dormant: Vec<u8>,

    /// Structure ids.
    pub structure_id: Vec<u32>,
    /// The seat each structure belongs to.
    pub structure_seat: Vec<u8>,
    /// Each structure's [`crate::tables::StructureKind::id`].
    pub structure_kind: Vec<u8>,
    /// Three raw Q16.16 coordinates per structure.
    pub structure_pos: Vec<i32>,
    /// Hit points per structure.
    pub structure_hp: Vec<i32>,
    /// Each structure's home beacon, or [`crate::tables::BeaconId::NONE`].
    pub structure_home: Vec<u32>,

    /// Wreck ids.
    pub wreck_id: Vec<u32>,
    /// Three raw Q16.16 coordinates per wreck.
    pub wreck_pos: Vec<i32>,
    /// Salvage value per wreck, in `$`.
    pub wreck_salvage: Vec<i64>,

    /// The chunks an edit has touched since generation, ascending.
    pub modified_chunk: Vec<u32>,
    /// Those chunks' material bytes, [`CHUNK_VOXELS`] per entry of
    /// [`Snapshot::modified_chunk`], concatenated in the same order.
    pub modified_chunk_bytes: Vec<u8>,

    /// One digest per chunk, in chunk-index order. Checked against the store
    /// the restore rebuilds, both in length and value: see
    /// [`SnapshotError::ChunkDigest`].
    pub chunk_digest: Vec<u64>,
}

/// What went wrong saving or restoring.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SnapshotError {
    /// The bytes are not a snapshot this build can read.
    Version {
        /// What the bytes said.
        found: u32,
        /// What this build writes.
        expected: u32,
    },
    /// postcard could not decode the bytes.
    Decode(String),
    /// postcard could not encode the snapshot.
    Encode(String),
    /// The decoded columns disagree in length: a truncated or edited file.
    Ragged(&'static str),
    /// The map could not be regenerated from the snapshot's seed under the
    /// receiving world's rules table.
    Map(MapError),
    /// The restored tables describe a world the receiving world's derived
    /// indexes cannot cover — a snapshot with more units than the broadphase
    /// grid can hold. Refused rather than restored into a world whose every
    /// later query would come back empty.
    Unindexable {
        /// How many units the snapshot holds.
        units: u32,
    },
    /// A carried chunk digest is not the digest of the chunk the restore
    /// rebuilt. The file describes a store this build does not produce — a
    /// different generator, a different rules table, or edited bytes — and a
    /// world restored from it would hash something its own voxels disagree
    /// with. Refused at the door rather than three operating systems later.
    ChunkDigest {
        /// The chunk the digests disagree at, the lowest one.
        chunk: u32,
    },
}

impl core::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SnapshotError::Version { found, expected } => write!(
                f,
                "snapshot version {found} cannot be read by a build that writes version {expected}"
            ),
            SnapshotError::Decode(message) => write!(f, "decoding the snapshot: {message}"),
            SnapshotError::Encode(message) => write!(f, "encoding the snapshot: {message}"),
            SnapshotError::Ragged(table) => {
                write!(f, "the `{table}` columns disagree in length")
            }
            SnapshotError::Map(error) => {
                write!(f, "regenerating the map from the snapshot's seed: {error}")
            }
            SnapshotError::Unindexable { units } => write!(
                f,
                "the snapshot's {units} units cannot be indexed by a broadphase grid built from \
                 this world's rules table"
            ),
            SnapshotError::ChunkDigest { chunk } => write!(
                f,
                "the snapshot's digest for chunk {chunk} is not the digest of the chunk this \
                 build rebuilt from the same seed"
            ),
        }
    }
}

impl std::error::Error for SnapshotError {}

impl Snapshot {
    /// Project a world into the flat form. Lossless over hashed state.
    ///
    /// Derived state — the broadphase, the work counter — is deliberately
    /// absent: the grid is sized from the restored tables by
    /// [`Snapshot::restore_into`] and filled at the next tick's first phase, so
    /// putting it in the file would be a second source of truth. The pristine
    /// voxels are absent for the same reason one level up: the seed is the
    /// source of truth, and the file carries only what an edit has changed.
    #[must_use]
    pub fn capture(world: &World) -> Snapshot {
        let units = world.units();
        let seats = world.seats();
        let beacons = world.beacons();
        let structures = world.structures();
        let wrecks = world.wrecks();
        let voxels = world.voxels();

        let modified_chunk = voxels.modified_indices();
        let mut modified_chunk_bytes: Vec<u8> =
            Vec::with_capacity(modified_chunk.len().saturating_mul(CHUNK_VOXELS));
        for chunk in &modified_chunk {
            if let Some(bytes) = voxels.chunk_bytes(*chunk) {
                modified_chunk_bytes.extend_from_slice(bytes.as_slice());
            }
        }

        Snapshot {
            version: SNAPSHOT_VERSION,
            match_seed: world.match_seed(),
            tick: world.tick().raw(),

            seat_id: seats.seats().to_vec(),
            seat_treasury: seats.treasuries().iter().map(|m| m.raw()).collect(),
            seat_supply: seats.supplies().iter().map(|k| k.raw()).collect(),
            seat_draw: seats.draws().iter().map(|k| k.raw()).collect(),

            unit_id: units.ids().to_vec(),
            unit_seat: units.seats().to_vec(),
            unit_kind: units.kinds().to_vec(),
            unit_pos: axes_from_points(units.positions()),
            unit_dest: axes_from_points(units.destinations()),
            unit_heading: units.headings().iter().map(|a| a.raw()).collect(),
            unit_hp: units.hit_points().iter().map(|h| h.raw()).collect(),

            beacon_id: beacons.ids().to_vec(),
            beacon_seat: beacons.seats().to_vec(),
            beacon_pos: axes_from_points(beacons.positions()),
            beacon_mandate: beacons.mandates().to_vec(),
            beacon_program: beacons.programs().to_vec(),
            beacon_hp: beacons.hit_points().iter().map(|h| h.raw()).collect(),
            beacon_dormant: beacons.dormant().iter().map(|d| u8::from(*d)).collect(),

            structure_id: structures.ids().to_vec(),
            structure_seat: structures.seats().to_vec(),
            structure_kind: structures.kinds().to_vec(),
            structure_pos: axes_from_points(structures.positions()),
            structure_hp: structures.hit_points().iter().map(|h| h.raw()).collect(),
            structure_home: structures.homes().to_vec(),

            wreck_id: wrecks.ids().to_vec(),
            wreck_pos: axes_from_points(wrecks.positions()),
            wreck_salvage: wrecks.salvages().iter().map(|m| m.raw()).collect(),

            modified_chunk,
            modified_chunk_bytes,
            chunk_digest: world.chunks().as_slice().to_vec(),
        }
    }

    /// Encode to postcard bytes.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Encode`] when postcard refuses the value,
    /// which for this shape means the process is out of memory.
    pub fn to_bytes(&self) -> Result<Vec<u8>, SnapshotError> {
        postcard::to_stdvec(self).map_err(|error| SnapshotError::Encode(error.to_string()))
    }

    /// Decode from postcard bytes and check the version.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Decode`] when the bytes are not a snapshot, or
    /// [`SnapshotError::Version`] when they are one this build cannot read.
    pub fn from_bytes(bytes: &[u8]) -> Result<Snapshot, SnapshotError> {
        let snapshot: Snapshot = postcard::from_bytes(bytes)
            .map_err(|error| SnapshotError::Decode(error.to_string()))?;
        if snapshot.version != SNAPSHOT_VERSION {
            return Err(SnapshotError::Version {
                found: snapshot.version,
                expected: SNAPSHOT_VERSION,
            });
        }
        Ok(snapshot)
    }

    /// Write the snapshot back into `world`.
    ///
    /// The world keeps its rules table, so restoring into a world built under
    /// different rules is caught by the rules hash at the save's own level
    /// (T17) rather than silently half-applied here. Its derived indexes are
    /// **resized to the restored tables** before anything is written, because a
    /// snapshot may hold more units than the receiving world was built for.
    ///
    /// The map is regenerated from [`Snapshot::match_seed`] under the receiving
    /// world's rules and the snapshot's own seat count, and the modified chunks
    /// are then written over it.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Ragged`] when the columns disagree in length,
    /// [`SnapshotError::Map`] when the map cannot be regenerated, or
    /// [`SnapshotError::Unindexable`] when the receiving world's rules table
    /// cannot describe a grid for the restored unit count. Either way the world
    /// is left exactly as it was.
    pub fn restore_into(&self, world: &mut World) -> Result<(), SnapshotError> {
        let seat_count = u32::try_from(self.seat_id.len()).unwrap_or(0);
        let mut seats = SeatTable::with_capacity(seat_count);
        if !seats.restore(
            self.seat_id.clone(),
            self.seat_treasury.iter().copied().map(Money::new).collect(),
            self.seat_supply.iter().copied().map(Kw::new).collect(),
            self.seat_draw.iter().copied().map(Kw::new).collect(),
        ) {
            return Err(SnapshotError::Ragged("seat"));
        }

        let mut units = UnitTable::with_capacity(u32::try_from(self.unit_id.len()).unwrap_or(0));
        if !units.restore(UnitColumns {
            id: self.unit_id.clone(),
            seat: self.unit_seat.clone(),
            kind: self.unit_kind.clone(),
            pos: points_from_axes(&self.unit_pos).ok_or(SnapshotError::Ragged("unit_pos"))?,
            dest: points_from_axes(&self.unit_dest).ok_or(SnapshotError::Ragged("unit_dest"))?,
            heading: self
                .unit_heading
                .iter()
                .copied()
                .map(Angle::from_raw)
                .collect(),
            hp: self.unit_hp.iter().copied().map(Hp::new).collect(),
        }) {
            return Err(SnapshotError::Ragged("unit"));
        }

        let mut beacons =
            BeaconTable::with_capacity(u32::try_from(self.beacon_id.len()).unwrap_or(0));
        if !beacons.restore(BeaconColumns {
            id: self.beacon_id.clone(),
            seat: self.beacon_seat.clone(),
            pos: points_from_axes(&self.beacon_pos).ok_or(SnapshotError::Ragged("beacon_pos"))?,
            mandate: self.beacon_mandate.clone(),
            program: self.beacon_program.clone(),
            hp: self.beacon_hp.iter().copied().map(Hp::new).collect(),
            dormant: self.beacon_dormant.iter().map(|d| *d != 0).collect(),
        }) {
            return Err(SnapshotError::Ragged("beacon"));
        }

        let mut structures =
            StructureTable::with_capacity(u32::try_from(self.structure_id.len()).unwrap_or(0));
        if !structures.restore(StructureColumns {
            id: self.structure_id.clone(),
            seat: self.structure_seat.clone(),
            kind: self.structure_kind.clone(),
            pos: points_from_axes(&self.structure_pos)
                .ok_or(SnapshotError::Ragged("structure_pos"))?,
            hp: self.structure_hp.iter().copied().map(Hp::new).collect(),
            home: self.structure_home.clone(),
        }) {
            return Err(SnapshotError::Ragged("structure"));
        }

        let mut wrecks = WreckTable::with_capacity(u32::try_from(self.wreck_id.len()).unwrap_or(0));
        if !wrecks.restore(
            self.wreck_id.clone(),
            points_from_axes(&self.wreck_pos).ok_or(SnapshotError::Ragged("wreck_pos"))?,
            self.wreck_salvage.iter().copied().map(Money::new).collect(),
        ) {
            return Err(SnapshotError::Ragged("wreck"));
        }

        let (voxels, chunks) = self.restore_store(world, seat_count)?;

        let unit_count = units.len();
        if !world.restore_tables(RestoredTables {
            match_seed: self.match_seed,
            tick: Tick::new(self.tick),
            seats,
            units,
            beacons,
            structures,
            wrecks,
            voxels,
            chunks,
        }) {
            return Err(SnapshotError::Unindexable { units: unit_count });
        }
        Ok(())
    }

    /// Rebuild the chunk store: the pristine layer from the seed, then the
    /// chunks an edit changed, then the digest column checked against both.
    ///
    /// Separate from [`Snapshot::restore_into`] because it is the half of a
    /// restore that is about the *map* rather than about the tables, and
    /// because a restore is the one place the digests — hashed state — are
    /// taken on a file's word unless somebody checks them.
    fn restore_store(
        &self,
        world: &World,
        seat_count: u32,
    ) -> Result<(VoxelStore, ChunkDigests), SnapshotError> {
        if self.modified_chunk.len().saturating_mul(CHUNK_VOXELS) != self.modified_chunk_bytes.len()
        {
            return Err(SnapshotError::Ragged("modified_chunk"));
        }
        let generated = mapgen::generate(self.match_seed, world.rules(), seat_count)
            .map_err(SnapshotError::Map)?;
        let mut voxels = generated.voxels;
        for (slot, chunk) in self.modified_chunk.iter().enumerate() {
            let from = slot.saturating_mul(CHUNK_VOXELS);
            let to = from.saturating_add(CHUNK_VOXELS);
            let Some(bytes) = self.modified_chunk_bytes.get(from..to) else {
                return Err(SnapshotError::Ragged("modified_chunk_bytes"));
            };
            if !voxels.restore_chunk(*chunk, bytes) {
                return Err(SnapshotError::Ragged("modified_chunk"));
            }
        }

        // The digests are hashed state, so a file whose digest column does not
        // describe the store just rebuilt is refused rather than loaded (see the
        // module docs: the claim there is only true if it is checked here). The
        // cost is one pass of xxh3 over 9.4 MB at a restore, a fraction of
        // regenerating the map above, and it is never paid inside a tick.
        if self.chunk_digest.len() != usize::try_from(voxels.chunk_count()).unwrap_or(usize::MAX) {
            return Err(SnapshotError::Ragged("chunk_digest"));
        }
        let mut chunks = ChunkDigests::new(0);
        chunks.restore(self.chunk_digest.clone());
        let mut chunk: u32 = 0;
        while chunk < voxels.chunk_count() {
            let Some(bytes) = voxels.chunk_bytes(chunk) else {
                return Err(SnapshotError::Ragged("chunk_digest"));
            };
            if chunks.get(chunk) != Some(digest(bytes.as_slice())) {
                return Err(SnapshotError::ChunkDigest { chunk });
            }
            chunk = chunk.saturating_add(1);
        }
        Ok((voxels, chunks))
    }
}

/// Three raw axes per point, in encoder order.
fn axes_from_points(points: &[[Fx; 3]]) -> Vec<i32> {
    let mut out: Vec<i32> = Vec::with_capacity(points.len().saturating_mul(3));
    for point in points {
        for axis in point {
            out.push(axis.raw());
        }
    }
    out
}

/// Three raw axes per point, in encoder order.
#[allow(
    clippy::integer_division,
    reason = "the length is a multiple of three by the test above, so the division is exact"
)]
fn points_from_axes(axes: &[i32]) -> Option<Vec<[Fx; 3]>> {
    if axes.len() % 3 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(axes.len() / 3);
    for chunk in axes.chunks_exact(3) {
        out.push([
            Fx::from_raw(*chunk.first()?),
            Fx::from_raw(*chunk.get(1)?),
            Fx::from_raw(*chunk.get(2)?),
        ]);
    }
    Some(out)
}
