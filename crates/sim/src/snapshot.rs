// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Save and restore (decisions log item 49).
//!
//! [`Snapshot`] is a flat projection of [`World`] whose **fields are all
//! fixed-width**: no `usize` or `isize` field, no pointer, no enum, and every
//! `Option` reduced to a sentinel so the encoding has no tag byte whose layout
//! could differ between targets.
//!
//! The format is **postcard 1.1.x** (`default-features = false`, `use-std`)
//! with serde derives. Measured in G4 at 4 563 B for the toy world at tick
//! 4 800, 0.04 ms to save and 0.24–0.32 ms to restore, byte-identical across
//! ubuntu, windows and macOS. rkyv (7 160 B, a much heavier dependency tree)
//! and a hand-written codec were both rejected.
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
use crate::math::fixed::{Angle, Fx};
use crate::math::quantity::{Hp, Kw, Money, Tick};
use crate::tables::{SeatTable, UnitTable};
use crate::world::World;
use serde::{Deserialize, Serialize};

/// Snapshot format version. Part of the bytes, checked on restore.
///
/// A bump is a contract change (AGENTS.md §5) and invalidates every committed
/// save.
pub const SNAPSHOT_VERSION: u32 = 1;

/// A flat, fixed-width projection of the world.
///
/// Column-per-field rather than struct-per-row, matching the `SoA` tables: it
/// keeps every field fixed-width and it is what makes the byte count above
/// small enough to save at a Lull boundary without the player noticing.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    /// [`SNAPSHOT_VERSION`] at the time of writing.
    pub version: u32,
    /// The match seed.
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
    /// Three raw Q16.16 coordinates per unit: x, y, z.
    pub unit_pos: Vec<i32>,
    /// Three raw Q16.16 coordinates per unit's destination.
    pub unit_dest: Vec<i32>,
    /// Raw heading units per unit.
    pub unit_heading: Vec<u16>,
    /// Hit points per unit.
    pub unit_hp: Vec<i32>,

    /// One digest per chunk, in chunk-index order.
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
    /// The restored tables describe a world the receiving world's derived
    /// indexes cannot cover — a snapshot with more units than the broadphase
    /// grid can hold. Refused rather than restored into a world whose every
    /// later query would come back empty.
    Unindexable {
        /// How many units the snapshot holds.
        units: u32,
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
            SnapshotError::Unindexable { units } => write!(
                f,
                "the snapshot's {units} units cannot be indexed by a broadphase grid built from \
                 this world's rules table"
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
    /// putting it in the file would be a second source of truth.
    #[must_use]
    pub fn capture(world: &World) -> Snapshot {
        let units = world.units();
        let seats = world.seats();
        let mut snapshot = Snapshot {
            version: SNAPSHOT_VERSION,
            match_seed: world.match_seed(),
            tick: world.tick().raw(),
            seat_id: seats.seats().to_vec(),
            seat_treasury: seats.treasuries().iter().map(|m| m.raw()).collect(),
            seat_supply: seats.supplies().iter().map(|k| k.raw()).collect(),
            seat_draw: seats.draws().iter().map(|k| k.raw()).collect(),
            unit_id: units.ids().to_vec(),
            unit_seat: units.seats().to_vec(),
            unit_pos: Vec::with_capacity(units.positions().len().saturating_mul(3)),
            unit_dest: Vec::with_capacity(units.destinations().len().saturating_mul(3)),
            unit_heading: units.headings().iter().map(|a| a.raw()).collect(),
            unit_hp: units.hit_points().iter().map(|h| h.raw()).collect(),
            chunk_digest: world.chunks().as_slice().to_vec(),
        };
        for point in units.positions() {
            for axis in point {
                snapshot.unit_pos.push(axis.raw());
            }
        }
        for point in units.destinations() {
            for axis in point {
                snapshot.unit_dest.push(axis.raw());
            }
        }
        snapshot
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
    /// # Errors
    ///
    /// Returns [`SnapshotError::Ragged`] when the columns disagree in length,
    /// or [`SnapshotError::Unindexable`] when the receiving world's rules table
    /// cannot describe a grid for the restored unit count. Either way the world
    /// is left exactly as it was.
    pub fn restore_into(&self, world: &mut World) -> Result<(), SnapshotError> {
        let mut seats = SeatTable::with_capacity(u32::try_from(self.seat_id.len()).unwrap_or(0));
        if !seats.restore(
            self.seat_id.clone(),
            self.seat_treasury.iter().copied().map(Money::new).collect(),
            self.seat_supply.iter().copied().map(Kw::new).collect(),
            self.seat_draw.iter().copied().map(Kw::new).collect(),
        ) {
            return Err(SnapshotError::Ragged("seat"));
        }

        let positions =
            points_from_axes(&self.unit_pos).ok_or(SnapshotError::Ragged("unit_pos"))?;
        let destinations =
            points_from_axes(&self.unit_dest).ok_or(SnapshotError::Ragged("unit_dest"))?;
        let mut units = UnitTable::with_capacity(u32::try_from(self.unit_id.len()).unwrap_or(0));
        if !units.restore(
            self.unit_id.clone(),
            self.unit_seat.clone(),
            positions,
            destinations,
            self.unit_heading
                .iter()
                .copied()
                .map(Angle::from_raw)
                .collect(),
            self.unit_hp.iter().copied().map(Hp::new).collect(),
        ) {
            return Err(SnapshotError::Ragged("unit"));
        }

        let mut chunks = ChunkDigests::new(0);
        chunks.restore(self.chunk_digest.clone());

        let unit_count = units.len();
        if !world.restore_tables(self.match_seed, Tick::new(self.tick), seats, units, chunks) {
            return Err(SnapshotError::Unindexable { units: unit_count });
        }
        Ok(())
    }
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
