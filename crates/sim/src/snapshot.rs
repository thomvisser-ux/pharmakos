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
use crate::pathing::router::{RestoredRouter, first_route_digest_mismatch};
use crate::runner::{DEFAULT_ROUND_LIMIT, MatchParts, MatchPhase, MatchState};
use crate::tables::{
    BeaconColumns, BeaconTable, CREDIT_SLOTS, CreditTable, SeatColumns, SeatId, SeatTable,
    SightingColumns, SightingTable, StructureColumns, StructureTable, TargetColumns, TargetTable,
    UnitColumns, UnitTable, WreckTable,
};
use crate::voxels::{CHUNK_VOXELS, VoxelStore};
use crate::world::{RestoredTables, World};
use serde::{Deserialize, Serialize};

/// Snapshot format version. Part of the bytes, checked on restore.
///
/// A bump is a contract change (AGENTS.md section 5) and invalidates every
/// committed save. **Version 2 is T5's**: it adds the unit kind column, the
/// beacon, structure and wreck tables, and the modified-chunk half of the
/// voxel store. **Version 3 is T7's**: it adds the router — each unit's walk
/// state, speed accumulator, route length, route cursor, partial flag and
/// route digest, the packed route nodes, and the repath queue's round-robin
/// cursor.
///
/// The pathing *graph* is deliberately not in the file: it is a pure function
/// of the chunk store and the cost rows, so a restore rebuilds it. The routes
/// are not, because a route is a decision a unit already made.
///
/// **Version 4 is T10's**: it adds the match state — the phase, the round, the
/// round limit, the effective per-round length list, the segment's start tick
/// and length, **the coming segment's length** and the outcome — and the three
/// per-seat match columns (the elimination tick, the commander's death count
/// for this Push and any pending respawn). The coming segment's length is the
/// number item 30 puts in this snapshot and nowhere else, and it is what
/// `plan-core`'s `context::segment_length_ms` reads.
///
/// The event bus is deliberately **not** in the file: it is derived output that
/// nothing in a tick reads, so a resumed match starts with an empty feed
/// ([`crate::events`]).
///
/// **Version 5 is T11's**: it adds the interpreter's per-seat state — the route
/// cursor, the highest step reached, the stage of the step in progress with its
/// start tick and deadline, the rule body running, the pinned selector target,
/// the visit with its row and commit tick, the reflex's armed and active flags
/// and its last-damage marker, the fallback's leg, the match-long commander
/// death count, and every handler's fire count and cooldown.
///
/// The **playbooks themselves are not in the file**, for the reason the rules
/// table is not: the sim is a pure function of `(map seed, playbooks, rules
/// hash)` and all three are inputs. A host resumes a save by re-sealing the
/// playbooks it saved beside it; `crate::interpreter::state::Interpreter::restore`
/// carries the PLACEHOLDER for the plan fingerprint T17 owes the save's stamp.
/// **Version 6 is T14's**: it adds the economy. Per unit, the home beacon, the
/// `$` it is carrying and the tick its timed work finishes at; per beacon, the
/// Quartermaster priority and the Survey scout count; per structure, the
/// under-construction flag; per seat, the Quartermaster's round-robin cursor;
/// and three new tables - every beacon's list settings (Build targets,
/// protected areas and probe areas), what each seat remembers seeing, and the
/// per-seat kill-credit counters.
///
/// The kill-credit counters are in the file although nothing fills them until
/// combat lands at S2, for the same reason they are in the hash: a field that
/// affects behaviour and is not carried is a restore that silently forgets it
/// (AGENTS.md section 4.8).
pub const SNAPSHOT_VERSION: u32 = 6;

/// A flat, fixed-width projection of the world.
///
/// Column-per-field rather than struct-per-row, matching the `SoA` tables: it
/// keeps every field fixed-width and it is what makes the byte count above
/// small enough to save at a Lull boundary without the player noticing.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
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
    /// The tick each seat was eliminated at, or
    /// [`crate::tables::NOT_ELIMINATED`].
    pub seat_eliminated_at: Vec<u32>,
    /// How many times each seat's commander has died in this Push (item 21).
    pub seat_commander_deaths: Vec<u32>,
    /// The tick each seat's commander is due back at, or
    /// [`crate::tables::NO_RESPAWN`].
    pub seat_respawn_due: Vec<u32>,
    /// The Quartermaster's round-robin cursor, per seat.
    pub seat_qm_cursor: Vec<u32>,

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
    /// Each unit's home beacon, or [`crate::tables::BeaconId::NONE`].
    pub unit_home: Vec<u32>,
    /// Carried but undelivered `$`, per unit.
    pub unit_carrying: Vec<i64>,
    /// The tick each unit's timed work finishes at, or
    /// [`crate::tables::NO_WORK`].
    pub unit_busy_until: Vec<u32>,

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
    /// The Quartermaster priority knob, per beacon.
    pub beacon_priority: Vec<u8>,
    /// The Survey mandate's scout count, per beacon.
    pub beacon_scouts: Vec<u8>,

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
    /// Whether each structure is still going up, `0` or `1`.
    pub structure_building: Vec<u8>,

    /// Wreck ids.
    pub wreck_id: Vec<u32>,
    /// Three raw Q16.16 coordinates per wreck.
    pub wreck_pos: Vec<i32>,
    /// Salvage value per wreck, in `$`.
    pub wreck_salvage: Vec<i64>,

    /// The beacon each list setting belongs to.
    pub target_beacon: Vec<u32>,
    /// [`crate::tables::TargetKind::id`] per list setting.
    pub target_kind: Vec<u8>,
    /// [`crate::tables::StructureKind::id`] per list setting, or zero.
    pub target_blueprint: Vec<u8>,
    /// Three raw Q16.16 coordinates per list setting's anchor.
    pub target_at: Vec<i32>,
    /// Radii in whole voxels, per list setting.
    pub target_radius: Vec<i32>,
    /// The structure realising each Build target.
    pub target_built: Vec<u32>,

    /// Whose memory each sighting is.
    pub sighting_seat: Vec<u8>,
    /// What was seen, as a [`crate::knowledge::AssetId`] raw value.
    pub sighting_asset: Vec<u32>,
    /// Who owns the thing seen.
    pub sighting_owner: Vec<u8>,
    /// [`crate::knowledge::AssetKind::id`] per sighting.
    pub sighting_kind: Vec<u8>,
    /// Three raw Q16.16 coordinates per sighting.
    pub sighting_at: Vec<i32>,
    /// The tick each sighting was taken at.
    pub sighting_seen_at: Vec<u32>,

    /// The assets that carry kill credit, ascending.
    pub credit_asset: Vec<u32>,
    /// [`crate::tables::CREDIT_SLOTS`] seat ids per asset, concatenated.
    pub credit_seat: Vec<u8>,
    /// [`crate::tables::CREDIT_SLOTS`] damage figures per asset, concatenated.
    pub credit_damage: Vec<i32>,

    /// The chunks an edit has touched since generation, ascending.
    pub modified_chunk: Vec<u32>,
    /// Those chunks' material bytes, [`CHUNK_VOXELS`] per entry of
    /// [`Snapshot::modified_chunk`], concatenated in the same order.
    pub modified_chunk_bytes: Vec<u8>,

    /// One digest per chunk, in chunk-index order. Checked against the store
    /// the restore rebuilds, both in length and value: see
    /// [`SnapshotError::ChunkDigest`].
    pub chunk_digest: Vec<u64>,

    /// Each unit's [`crate::pathing::router::WalkState::id`].
    pub unit_walk_state: Vec<u8>,
    /// Each unit's speed accumulator, in twentieths of a cost unit.
    pub unit_accumulator: Vec<i32>,
    /// How many nodes each unit's route holds.
    pub unit_route_len: Vec<u32>,
    /// How far along its route each unit is.
    pub unit_route_cursor: Vec<u32>,
    /// Each unit's route digest — the form the state hash sees.
    pub unit_route_hash: Vec<u64>,
    /// Whether each route stops short of its goal, `0` or `1`.
    pub unit_route_partial: Vec<u8>,
    /// Every live route's nodes, packed: one unit's `unit_route_len` nodes
    /// after another's, in unit order. Packed rather than strided because the
    /// stride is 1 024 nodes a unit and almost all of it is empty.
    pub route_nodes: Vec<u32>,
    /// The repath queue's round-robin cursor.
    pub repath_cursor: u32,

    /// [`crate::runner::MatchPhase::id`] — where the match is.
    pub match_phase: u8,
    /// Which round is being played, from one.
    pub match_round: u32,
    /// The host-set round limit.
    pub match_round_limit: u32,
    /// The effective per-round length list, in game milliseconds (item 40).
    /// Resolved once at construction from the host's list or the rules table's
    /// ladder, and carried here because it is what decides every later
    /// segment's length.
    pub match_segment_lengths_ms: Vec<i32>,
    /// The tick the running segment opened on.
    pub match_segment_started: u32,
    /// The running segment's length in game milliseconds; zero outside a Push.
    pub match_segment_length_ms: i32,
    /// **The coming segment's length, in game milliseconds** (item 30).
    ///
    /// The number `get_status`, `get_briefing`, the editor's clock, the fits
    /// pill and `render_plan` read — and the one `plan-core`'s
    /// `context::segment_length_ms` was waiting for. Read it from here and
    /// never from `rules.match.segment_lengths_ms`: the ladder is the host's
    /// setting, the snapshot is the fact.
    pub coming_segment_ms: i32,
    /// [`crate::runner::MatchEndReason::id`], or `0` while the match runs.
    pub match_end_reason: u8,
    /// The winning seat, or [`crate::tables::SeatId::NEUTRAL`] for none.
    pub match_winner: u8,
    /// The tick the match ended on; zero while it runs.
    pub match_ended_at: u32,

    /// Commander deaths this **match**, per seat (`gp.v1.CmdrDeaths`).
    pub plan_deaths_match: Vec<u32>,
    /// The route cursor, per seat, or [`crate::interpreter::state::NO_INDEX`]
    /// once the fallback has taken over.
    pub plan_cursor: Vec<u32>,
    /// The highest route index entered, per seat — what `step_reached` reads.
    pub plan_reached: Vec<u32>,
    /// [`crate::interpreter::VisitState::id`], per seat.
    pub plan_stage: Vec<u8>,
    /// The tick the step in progress started, per seat.
    pub plan_started: Vec<u32>,
    /// The tick it times out at, per seat.
    pub plan_deadline: Vec<u32>,
    /// The handler whose body is running, per seat.
    pub plan_rule: Vec<u32>,
    /// How far into that body, per seat.
    pub plan_rule_step: Vec<u32>,
    /// Whether the step in progress has a pinned target, per seat.
    pub plan_pinned: Vec<u8>,
    /// The pinned beacon, per seat.
    pub plan_pinned_beacon: Vec<u32>,
    /// The pinned place, three whole voxels per seat.
    pub plan_pinned_at: Vec<i32>,
    /// The beacon a visit or deploy is at, per seat.
    pub plan_visit_beacon: Vec<u32>,
    /// The row committing, per seat.
    pub plan_visit_row: Vec<u32>,
    /// The tick that row, handshake or deploy commits at, per seat.
    pub plan_visit_due: Vec<u32>,
    /// Whether the reflex may fire, per seat.
    pub plan_reflex_armed: Vec<u8>,
    /// Whether the reflex is walking the commander to safety, per seat.
    pub plan_reflex_active: Vec<u8>,
    /// The tick the commander last took damage, per seat.
    pub plan_last_damage: Vec<u32>,
    /// The commander's hit points at the last decision, per seat.
    pub plan_last_hp: Vec<i32>,
    /// The patrol waypoint the fallback is walking to, per seat.
    pub plan_fallback_leg: Vec<u32>,
    /// How many handlers each seat's sealed plan has.
    pub plan_rule_count: Vec<u32>,
    /// Per-handler fire counts, packed in seat order behind
    /// [`Snapshot::plan_rule_count`] — the shape the route nodes take, and for
    /// the same reason.
    pub plan_fires: Vec<u32>,
    /// Per-handler cooldown ticks, packed the same way.
    pub plan_ready: Vec<u32>,
}

/// An empty snapshot in an **opening Lull**, not an empty one in no phase at
/// all.
///
/// Written by hand rather than derived because a derived `Default` would give
/// `match_phase = 0`, which is not a phase this build defines, so a
/// `Snapshot { ..Default::default() }` — the shape three crates' tests build —
/// could be encoded but never restored. The same reasoning as [`Enc`]'s
/// hand-written `Default` in [`crate::encoding`]: a default that cannot be used
/// the documented way is a trap, not a convenience.
///
/// [`Enc`]: crate::encoding::Enc
impl Default for Snapshot {
    fn default() -> Snapshot {
        Snapshot {
            version: SNAPSHOT_VERSION,
            match_seed: 0,
            tick: 0,
            seat_id: Vec::new(),
            seat_treasury: Vec::new(),
            seat_supply: Vec::new(),
            seat_draw: Vec::new(),
            seat_eliminated_at: Vec::new(),
            seat_commander_deaths: Vec::new(),
            seat_respawn_due: Vec::new(),
            seat_qm_cursor: Vec::new(),
            unit_id: Vec::new(),
            unit_seat: Vec::new(),
            unit_kind: Vec::new(),
            unit_pos: Vec::new(),
            unit_dest: Vec::new(),
            unit_heading: Vec::new(),
            unit_hp: Vec::new(),
            unit_home: Vec::new(),
            unit_carrying: Vec::new(),
            unit_busy_until: Vec::new(),
            beacon_id: Vec::new(),
            beacon_seat: Vec::new(),
            beacon_pos: Vec::new(),
            beacon_mandate: Vec::new(),
            beacon_program: Vec::new(),
            beacon_hp: Vec::new(),
            beacon_dormant: Vec::new(),
            beacon_priority: Vec::new(),
            beacon_scouts: Vec::new(),
            structure_id: Vec::new(),
            structure_seat: Vec::new(),
            structure_kind: Vec::new(),
            structure_pos: Vec::new(),
            structure_hp: Vec::new(),
            structure_home: Vec::new(),
            structure_building: Vec::new(),
            wreck_id: Vec::new(),
            wreck_pos: Vec::new(),
            wreck_salvage: Vec::new(),
            target_beacon: Vec::new(),
            target_kind: Vec::new(),
            target_blueprint: Vec::new(),
            target_at: Vec::new(),
            target_radius: Vec::new(),
            target_built: Vec::new(),
            sighting_seat: Vec::new(),
            sighting_asset: Vec::new(),
            sighting_owner: Vec::new(),
            sighting_kind: Vec::new(),
            sighting_at: Vec::new(),
            sighting_seen_at: Vec::new(),
            credit_asset: Vec::new(),
            credit_seat: Vec::new(),
            credit_damage: Vec::new(),
            modified_chunk: Vec::new(),
            modified_chunk_bytes: Vec::new(),
            chunk_digest: Vec::new(),
            unit_walk_state: Vec::new(),
            unit_accumulator: Vec::new(),
            unit_route_len: Vec::new(),
            unit_route_cursor: Vec::new(),
            unit_route_hash: Vec::new(),
            unit_route_partial: Vec::new(),
            route_nodes: Vec::new(),
            repath_cursor: 0,
            match_phase: MatchPhase::Lull.id(),
            match_round: 1,
            match_round_limit: DEFAULT_ROUND_LIMIT,
            match_segment_lengths_ms: Vec::new(),
            match_segment_started: 0,
            match_segment_length_ms: 0,
            coming_segment_ms: 0,
            match_end_reason: 0,
            match_winner: SeatId::NEUTRAL.raw(),
            match_ended_at: 0,
            plan_deaths_match: Vec::new(),
            plan_cursor: Vec::new(),
            plan_reached: Vec::new(),
            plan_stage: Vec::new(),
            plan_started: Vec::new(),
            plan_deadline: Vec::new(),
            plan_rule: Vec::new(),
            plan_rule_step: Vec::new(),
            plan_pinned: Vec::new(),
            plan_pinned_beacon: Vec::new(),
            plan_pinned_at: Vec::new(),
            plan_visit_beacon: Vec::new(),
            plan_visit_row: Vec::new(),
            plan_visit_due: Vec::new(),
            plan_reflex_armed: Vec::new(),
            plan_reflex_active: Vec::new(),
            plan_last_damage: Vec::new(),
            plan_last_hp: Vec::new(),
            plan_fallback_leg: Vec::new(),
            plan_rule_count: Vec::new(),
            plan_fires: Vec::new(),
            plan_ready: Vec::new(),
        }
    }
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
    /// A carried route digest is not the digest of the route nodes beside it.
    /// The route reaches the state hash as a digest, so a world restored from
    /// such a file would walk one route and hash another until that unit's next
    /// repath — a desync with nothing red in front of it. Refused for the same
    /// reason [`SnapshotError::ChunkDigest`] is.
    RouteDigest {
        /// The unit the digests disagree at, the lowest one.
        unit: u32,
    },
    /// The match state names a phase or an end reason this build does not
    /// define. A file from another version or an edited one: refused rather
    /// than restored into a match that is in no phase at all, which is a world
    /// whose next tick would do nothing and whose hash would say so.
    MatchState {
        /// The phase byte the file carried.
        phase: u8,
        /// The end-reason byte the file carried.
        end_reason: u8,
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
            SnapshotError::RouteDigest { unit } => write!(
                f,
                "the snapshot's route digest for unit {unit} is not the digest of the route \
                 nodes it carries"
            ),
            SnapshotError::MatchState { phase, end_reason } => write!(
                f,
                "the snapshot's match phase {phase} or end reason {end_reason} is not one this \
                 build defines"
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
    #[allow(
        clippy::too_many_lines,
        reason = "one line per column of a flat projection, and the whole point of the projection is that it is one expression: a `Snapshot` cannot be built in halves, so splitting it would mean a second struct whose only job is to be moved into this one"
    )]
    pub fn capture(world: &World) -> Snapshot {
        let units = world.units();
        let seats = world.seats();
        let beacons = world.beacons();
        let structures = world.structures();
        let wrecks = world.wrecks();
        let targets = world.targets();
        let sightings = world.sightings();
        let credit = world.credit();
        let voxels = world.voxels();

        let state = world.match_state().to_parts();
        let plan = world.interpreter().to_parts();
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
            seat_eliminated_at: seats.eliminated_at().to_vec(),
            seat_commander_deaths: seats.commander_deaths().to_vec(),
            seat_respawn_due: seats.respawn_due().to_vec(),
            seat_qm_cursor: seats.qm_cursors().to_vec(),

            unit_id: units.ids().to_vec(),
            unit_seat: units.seats().to_vec(),
            unit_kind: units.kinds().to_vec(),
            unit_pos: axes_from_points(units.positions()),
            unit_dest: axes_from_points(units.destinations()),
            unit_heading: units.headings().iter().map(|a| a.raw()).collect(),
            unit_hp: units.hit_points().iter().map(|h| h.raw()).collect(),
            unit_home: units.homes().to_vec(),
            unit_carrying: units.carrying().iter().map(|m| m.raw()).collect(),
            unit_busy_until: units.busy_until().to_vec(),

            beacon_id: beacons.ids().to_vec(),
            beacon_seat: beacons.seats().to_vec(),
            beacon_pos: axes_from_points(beacons.positions()),
            beacon_mandate: beacons.mandates().to_vec(),
            beacon_program: beacons.programs().to_vec(),
            beacon_hp: beacons.hit_points().iter().map(|h| h.raw()).collect(),
            beacon_dormant: beacons.dormant().iter().map(|d| u8::from(*d)).collect(),
            beacon_priority: beacons.priorities().to_vec(),
            beacon_scouts: beacons.scout_counts().to_vec(),

            structure_id: structures.ids().to_vec(),
            structure_seat: structures.seats().to_vec(),
            structure_kind: structures.kinds().to_vec(),
            structure_pos: axes_from_points(structures.positions()),
            structure_hp: structures.hit_points().iter().map(|h| h.raw()).collect(),
            structure_home: structures.homes().to_vec(),
            structure_building: structures.building().iter().map(|b| u8::from(*b)).collect(),

            wreck_id: wrecks.ids().to_vec(),
            wreck_pos: axes_from_points(wrecks.positions()),
            wreck_salvage: wrecks.salvages().iter().map(|m| m.raw()).collect(),

            target_beacon: targets.beacons().to_vec(),
            target_kind: targets.kinds().to_vec(),
            target_blueprint: targets.blueprints().to_vec(),
            target_at: axes_from_points(targets.anchors()),
            target_radius: targets.radii().to_vec(),
            target_built: targets.built().to_vec(),

            sighting_seat: sightings.seats().to_vec(),
            sighting_asset: sightings.assets().to_vec(),
            sighting_owner: sightings.owners().to_vec(),
            sighting_kind: sightings.kinds().to_vec(),
            sighting_at: axes_from_points(sightings.places()),
            sighting_seen_at: sightings.seen_at().to_vec(),

            credit_asset: credit.assets().to_vec(),
            credit_seat: credit.seats().iter().flatten().copied().collect(),
            credit_damage: credit.damages().iter().flatten().copied().collect(),

            modified_chunk,
            modified_chunk_bytes,
            chunk_digest: world.chunks().as_slice().to_vec(),

            unit_walk_state: world.router().states().to_vec(),
            unit_accumulator: world.router().accumulators().to_vec(),
            unit_route_len: world.router().route_lengths().to_vec(),
            unit_route_cursor: world.router().route_cursors().to_vec(),
            unit_route_hash: world.router().route_hashes().to_vec(),
            unit_route_partial: world
                .router()
                .route_partials()
                .iter()
                .map(|partial| u8::from(*partial))
                .collect(),
            route_nodes: world.router().packed_routes(),
            repath_cursor: world.router().queue_cursor(),

            match_phase: state.phase,
            match_round: state.round,
            match_round_limit: state.round_limit,
            match_segment_lengths_ms: state.segment_lengths_ms,
            match_segment_started: state.segment_started,
            match_segment_length_ms: state.segment_length_ms,
            coming_segment_ms: state.coming_segment_ms,
            match_end_reason: state.end_reason,
            match_winner: state.winner,
            match_ended_at: state.ended_at,

            plan_deaths_match: plan.deaths_match,
            plan_cursor: plan.cursor,
            plan_reached: plan.reached,
            plan_stage: plan.stage,
            plan_started: plan.started,
            plan_deadline: plan.deadline,
            plan_rule: plan.rule,
            plan_rule_step: plan.rule_step,
            plan_pinned: plan.pinned,
            plan_pinned_beacon: plan.pinned_beacon,
            plan_pinned_at: plan.pinned_at,
            plan_visit_beacon: plan.visit_beacon,
            plan_visit_row: plan.visit_row,
            plan_visit_due: plan.visit_due,
            plan_reflex_armed: plan.reflex_armed,
            plan_reflex_active: plan.reflex_active,
            plan_last_damage: plan.last_damage,
            plan_last_hp: plan.last_hp,
            plan_fallback_leg: plan.fallback_leg,
            plan_rule_count: plan.rule_count,
            plan_fires: plan.fires,
            plan_ready: plan.ready,
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
    #[allow(
        clippy::too_many_lines,
        reason = "one block per table, and every block must run before the first assignment: a restore either applies in full or changes nothing, so the checks cannot be moved behind the writes"
    )]
    pub fn restore_into(&self, world: &mut World) -> Result<(), SnapshotError> {
        let seat_count = u32::try_from(self.seat_id.len()).unwrap_or(0);
        let mut seats = SeatTable::with_capacity(seat_count);
        if !seats.restore(SeatColumns {
            seat: self.seat_id.clone(),
            treasury: self.seat_treasury.iter().copied().map(Money::new).collect(),
            supply: self.seat_supply.iter().copied().map(Kw::new).collect(),
            draw: self.seat_draw.iter().copied().map(Kw::new).collect(),
            eliminated_at: self.seat_eliminated_at.clone(),
            commander_deaths: self.seat_commander_deaths.clone(),
            respawn_due: self.seat_respawn_due.clone(),
            qm_cursor: self.seat_qm_cursor.clone(),
        }) {
            return Err(SnapshotError::Ragged("seat"));
        }

        let match_state = self.restore_match()?;

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
            home: self.unit_home.clone(),
            carrying: self.unit_carrying.iter().copied().map(Money::new).collect(),
            busy_until: self.unit_busy_until.clone(),
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
            priority: self.beacon_priority.clone(),
            scouts: self.beacon_scouts.clone(),
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
            building: self.structure_building.iter().map(|b| *b != 0).collect(),
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

        let targets = self.restore_targets(world)?;
        let sightings = self.restore_sightings(world)?;
        let credit = self.restore_credit(world)?;

        let (voxels, chunks) = self.restore_store(world, seat_count)?;

        let unit_count = units.len();
        let router = self.restore_router()?;
        let plan = self.restore_plan();
        if !plan.is_consistent(self.seat_id.len()) {
            return Err(SnapshotError::Ragged("plan"));
        }
        if !world.restore_tables(RestoredTables {
            match_seed: self.match_seed,
            tick: Tick::new(self.tick),
            seats,
            units,
            beacons,
            structures,
            wrecks,
            targets,
            sightings,
            credit,
            voxels,
            chunks,
            router,
            match_state,
            plan,
        }) {
            return Err(SnapshotError::Unindexable { units: unit_count });
        }
        Ok(())
    }

    /// Every beacon's list settings, checked before anything is written.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Ragged`] when the columns disagree in length,
    /// when a kind byte names no kind this build defines, or when the file
    /// holds more rows than the receiving world has room for — the same three
    /// checks every other table gets, for the same reason: a restore either
    /// applies in full or changes nothing.
    fn restore_targets(&self, world: &World) -> Result<TargetTable, SnapshotError> {
        let capacity = world.targets().capacity();
        let mut targets = TargetTable::with_capacity(capacity);
        if !targets.restore(
            capacity,
            TargetColumns {
                beacon: self.target_beacon.clone(),
                kind: self.target_kind.clone(),
                blueprint: self.target_blueprint.clone(),
                at: points_from_axes(&self.target_at).ok_or(SnapshotError::Ragged("target_at"))?,
                radius: self.target_radius.clone(),
                built: self.target_built.clone(),
            },
        ) {
            return Err(SnapshotError::Ragged("target"));
        }
        Ok(targets)
    }

    /// What each seat remembers seeing.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Ragged`], including when the `(seat, asset)`
    /// key is not strictly ascending — which is the order the table's own
    /// search assumes, so a file that broke it would restore into a table whose
    /// lookups silently missed.
    fn restore_sightings(&self, world: &World) -> Result<SightingTable, SnapshotError> {
        let capacity = world.sightings().capacity();
        let mut sightings =
            SightingTable::with_room(world.seats().len(), world.sightings().per_seat());
        if !sightings.restore(
            capacity,
            SightingColumns {
                seat: self.sighting_seat.clone(),
                asset: self.sighting_asset.clone(),
                owner: self.sighting_owner.clone(),
                kind: self.sighting_kind.clone(),
                at: points_from_axes(&self.sighting_at)
                    .ok_or(SnapshotError::Ragged("sighting_at"))?,
                seen_at: self.sighting_seen_at.clone(),
            },
        ) {
            return Err(SnapshotError::Ragged("sighting"));
        }
        Ok(sightings)
    }

    /// The per-seat kill-credit counters, unpacked from their fixed stride.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Ragged`] when the packed columns are not
    /// exactly [`CREDIT_SLOTS`] entries per asset, or when the asset column is
    /// not strictly ascending.
    fn restore_credit(&self, world: &World) -> Result<CreditTable, SnapshotError> {
        let rows = self.credit_asset.len();
        let packed = rows.saturating_mul(CREDIT_SLOTS);
        if self.credit_seat.len() != packed || self.credit_damage.len() != packed {
            return Err(SnapshotError::Ragged("credit"));
        }
        let mut seats: Vec<[u8; CREDIT_SLOTS]> = Vec::with_capacity(rows);
        let mut damages: Vec<[i32; CREDIT_SLOTS]> = Vec::with_capacity(rows);
        for row in 0..rows {
            let from = row.saturating_mul(CREDIT_SLOTS);
            let to = from.saturating_add(CREDIT_SLOTS);
            let (Some(slot), Some(dealt)) = (
                self.credit_seat.get(from..to),
                self.credit_damage.get(from..to),
            ) else {
                return Err(SnapshotError::Ragged("credit"));
            };
            let mut seat = [SeatId::NEUTRAL.raw(); CREDIT_SLOTS];
            let mut damage = [0_i32; CREDIT_SLOTS];
            for at in 0..CREDIT_SLOTS {
                if let (Some(into), Some(value)) = (seat.get_mut(at), slot.get(at)) {
                    *into = *value;
                }
                if let (Some(into), Some(value)) = (damage.get_mut(at), dealt.get(at)) {
                    *into = *value;
                }
            }
            seats.push(seat);
            damages.push(damage);
        }
        let capacity = world.credit().capacity();
        let mut credit = CreditTable::with_capacity(capacity);
        if !credit.restore(capacity, self.credit_asset.clone(), seats, damages) {
            return Err(SnapshotError::Ragged("credit"));
        }
        Ok(credit)
    }

    /// The match state, refused at the door when its phase or end reason is
    /// not one this build defines.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::MatchState`]; see
    /// [`crate::runner::MatchState::from_parts`] for why structure is refused
    /// while values are normalised.
    fn restore_match(&self) -> Result<MatchState, SnapshotError> {
        MatchState::from_parts(MatchParts {
            phase: self.match_phase,
            round: self.match_round,
            round_limit: self.match_round_limit,
            segment_lengths_ms: self.match_segment_lengths_ms.clone(),
            segment_started: self.match_segment_started,
            segment_length_ms: self.match_segment_length_ms,
            coming_segment_ms: self.coming_segment_ms,
            end_reason: self.match_end_reason,
            winner: self.match_winner,
            ended_at: self.match_ended_at,
        })
        .ok_or(SnapshotError::MatchState {
            phase: self.match_phase,
            end_reason: self.match_end_reason,
        })
    }

    /// The interpreter's columns, as the state module's own shape.
    ///
    /// Checked by `PlanParts::is_consistent` at the call site above, before
    /// anything is written, so a ragged plan column is refused with
    /// [`SnapshotError::Ragged`] like every other ragged table rather than
    /// through the restore's generic failure.
    fn restore_plan(&self) -> crate::interpreter::PlanParts {
        crate::interpreter::PlanParts {
            deaths_match: self.plan_deaths_match.clone(),
            cursor: self.plan_cursor.clone(),
            reached: self.plan_reached.clone(),
            stage: self.plan_stage.clone(),
            started: self.plan_started.clone(),
            deadline: self.plan_deadline.clone(),
            rule: self.plan_rule.clone(),
            rule_step: self.plan_rule_step.clone(),
            pinned: self.plan_pinned.clone(),
            pinned_beacon: self.plan_pinned_beacon.clone(),
            pinned_at: self.plan_pinned_at.clone(),
            visit_beacon: self.plan_visit_beacon.clone(),
            visit_row: self.plan_visit_row.clone(),
            visit_due: self.plan_visit_due.clone(),
            reflex_armed: self.plan_reflex_armed.clone(),
            reflex_active: self.plan_reflex_active.clone(),
            last_damage: self.plan_last_damage.clone(),
            last_hp: self.plan_last_hp.clone(),
            fallback_leg: self.plan_fallback_leg.clone(),
            rule_count: self.plan_rule_count.clone(),
            fires: self.plan_fires.clone(),
            ready: self.plan_ready.clone(),
        }
    }

    /// The router's columns, checked against each other before anything is
    /// written.
    ///
    /// Split out of [`Snapshot::restore_into`] for the reason every other check
    /// there is: a restore either applies in full or changes nothing, so every
    /// raggedness check has to run before the first assignment.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Ragged`] when the per-unit columns disagree in
    /// length, or when the packed routes are not exactly as long as the lengths
    /// say, and [`SnapshotError::RouteDigest`] when a carried digest is not the
    /// digest of the nodes beside it — the claim in
    /// [`crate::world::World::encode`]'s docs, that a restore producing a
    /// different route shows up as a moved digest, is only true because it is
    /// checked here (the chunk store's digests get the same argument and the
    /// same check).
    fn restore_router(&self) -> Result<RestoredRouter, SnapshotError> {
        let walkers = self.unit_id.len();
        if self.unit_walk_state.len() != walkers
            || self.unit_accumulator.len() != walkers
            || self.unit_route_len.len() != walkers
            || self.unit_route_cursor.len() != walkers
            || self.unit_route_hash.len() != walkers
            || self.unit_route_partial.len() != walkers
        {
            return Err(SnapshotError::Ragged("router"));
        }
        let mut packed: usize = 0;
        for len in &self.unit_route_len {
            packed = packed.saturating_add(usize::try_from(*len).unwrap_or(usize::MAX));
        }
        if packed != self.route_nodes.len() {
            return Err(SnapshotError::Ragged("route_nodes"));
        }
        if let Some(unit) = first_route_digest_mismatch(
            &self.unit_route_len,
            &self.unit_route_hash,
            &self.route_nodes,
        ) {
            return Err(SnapshotError::RouteDigest { unit });
        }
        Ok(RestoredRouter {
            state: self.unit_walk_state.clone(),
            accumulator: self.unit_accumulator.clone(),
            route_len: self.unit_route_len.clone(),
            route_cursor: self.unit_route_cursor.clone(),
            route_hash: self.unit_route_hash.clone(),
            route_partial: self
                .unit_route_partial
                .iter()
                .map(|partial| *partial != 0)
                .collect(),
            nodes: self.route_nodes.clone(),
            cursor: self.repath_cursor,
        })
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
