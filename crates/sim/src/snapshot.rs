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
//! is subtly the wrong shape. The rules hash, the verifier version and the
//! plan fingerprints are checked by the **save** that carries a snapshot -- the
//! gateway's `save.json`, whose stamp holds them beside these bytes (T17; the
//! wave-6 notes, decision C10) -- so this format did not move for them.

use crate::chunks::ChunkDigests;
use crate::encoding::digest;
use crate::mapgen::{self, MapError};
use crate::math::fixed::{Angle, Fx};
use crate::math::quantity::{Hp, Kw, Money, Tick};
use crate::pathing::router::{RestoredRouter, first_route_digest_mismatch};
use crate::runner::{DEFAULT_ROUND_LIMIT, MatchParts, MatchPhase, MatchState};
use crate::tables::{
    Arms, BeaconColumns, BeaconId, BeaconTable, CREDIT_SLOTS, ColumnBox, Common, CreditTable,
    QuarterTurns, SeatColumns, SeatId, SeatTable, ShellTable, SightingColumns, SightingTable,
    StructureColumns, StructureId, StructureTable, Tally, TargetColumns, TargetTable, UnitColumns,
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
/// playbooks it saved beside it, each checked against the plan fingerprint the
/// save carries (T17; `crate::interpreter::state::Interpreter::restore`).
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
///
/// **Version 7 is S1's targeting** (decisions-log item 127 (12) and (13);
/// `docs/design/targeting.md`, contract pull requests 2 and 3). Beacon names
/// became **per seat**, so it adds each beacon's per-seat ordinal, what a
/// seat's `b_NN` names; and it adds the state the targeting design binds: per
/// Build target, the description it was written with and the feature it is
/// bound to; and per seat, the feature the step in progress is covering, the
/// binding each of its rows read when it started, and the beacon a restarted
/// placement resumes at with the row it had reached. A version-6 file is
/// refused rather than read: its `b_NN` numbers counted every seat's beacons,
/// and read as per-seat names they would point a seat's orders at other
/// beacons.
///
/// **Version 8 is S1's Mine mandate** (decisions-log items 127 (9) and 128 (3)
/// (e); S1's plan, task `mine`). It adds five per-beacon columns: the Mine
/// settings `dig_max_depth`, `pillar_spacing`, `seam_choice` and
/// `flee_on_threat`, which every beacon carries at their defaults until a
/// settings row writes them, and **the seam the beacon holds**, the Mine
/// program's choice kept until the seam is spent ([`crate::mining`]). A
/// version-7 file is refused rather than read: it carries no held seam, and a
/// world restored from it would choose afresh where the unbroken run kept its
/// choice -- a desync with nothing red in front of it. The gateway's save
/// refuses a version-7 snapshot by its stamp before it reaches this check.
///
/// **Version 9 is S1's Build settings** (S1's plan, task `build`, and its
/// decision 5). It adds each Build target's place in the build order and its
/// rotation (`BuildTarget.order` and `.rotation_quarter_turns`, which the
/// mandate now builds by), and each structure's rotation, the facing the
/// target that paid for it chose; and it keeps a protected or probe area as
/// the box the playbook wrote ([`crate::tables::ColumnBox`]) where version 8
/// kept a centre and a covering radius. A version-8 file is refused rather than
/// read: it carries no order, and a world restored from it would build its
/// targets in list order where the unbroken run builds them by `order`. The
/// gateway's save refuses a version-8 snapshot by its stamp first.
///
/// **Version 10 is S2's generational ids** (S2's plan, task `ids`, E1). Rows
/// are reused, so it adds each unit, beacon, structure and wreck row's free
/// flag, each structure's ruined-at tick (which decides the ruin the reserve
/// buries), and two per-seat counters: the placement counter a beacon's
/// `b_NN` is taken from, and the beacons lost. Every id now carries its row's
/// generation in the bits above the row, which every id a version-9 file holds
/// leaves at zero; a version-9 file is refused rather than read all the same,
/// because it carries no placement counter, and a world restored from it would
/// hand a dead beacon's name to the next one. The gateway's save refuses a
/// version-9 snapshot by its stamp first.
///
/// **Version 11 is S2's combat** (S2's plan, task `fire`; E12, E13, E15 and
/// E16). It adds every asset's combat memory -- the tick its weapon may next
/// fire on, and the last enemy that damaged it with the tick (units,
/// structures and beacons alike) -- each unit's retreat, each beacon's Common
/// settings (the rules of engagement and the retreat threshold), each seat's
/// tally (the enemy value it destroyed, its kills, its shared kills and its
/// losses), each wreck's tick, and a new table, the shells in flight: each
/// shell's aim point, landing tick, seat, firer and weapon. A version-10 file
/// is refused rather than read: it carries no cooldown, and a world restored
/// from it would fire on a tick the unbroken run held its fire. The gateway's
/// save refuses a version-10 snapshot by its stamp first.
///
/// **Version 12 is S2's Build settings and drone work** (S2's plan, task
/// `site`). It adds each beacon's Build settings -- `repair_threshold_pct`,
/// `rebuild_destroyed` and `terraform` -- and the repair and voxel work the
/// Quartermaster has paid its drones for and they have not yet done
/// ([`crate::site::BeaconSite`]), and each wall target's top (its box of
/// columns rides the target's footprint column). It also adds each shell's launch
/// point and fire tick, so a shell can be drawn in flight
/// ([`crate::tables::ShellTable::position_at`]). A version-11 file is refused
/// rather than read: it carries no settings, and a world restored from it
/// would never repair, rebuild or level where the unbroken run did. The
/// gateway's save refuses a version-11 snapshot by its stamp first.
///
/// **Version 13 is S2's Defend mandate** (S2's plan, task `dfnd`). It adds a
/// new table, every Defend beacon's settings -- the engagement radius, pursue,
/// the picket count and whether it escorts the commander -- with its guard
/// assignment and its picket leg, and the protect list's beacons; and the
/// explored table `surv` writes, each `(seat, cell)` a seat has explored. Both
/// hash nothing while empty (E29). A version-12 file is refused rather than
/// read: it carries no Defend settings, and a world restored from it would
/// field no guard where the unbroken run did. The gateway's save refuses a
/// version-12 snapshot by its stamp first.
pub const SNAPSHOT_VERSION: u32 = 13;

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
    /// Each seat's placement counter ([`crate::tables::SeatTable::placed`]).
    pub seat_placed: Vec<u32>,
    /// Each seat's beacons lost ([`crate::tables::SeatTable::beacons_lost`]).
    pub seat_beacons_lost: Vec<u32>,
    /// Each seat's enemy value destroyed, in `$` ([`crate::tables::Tally`]).
    pub seat_destroyed: Vec<i64>,
    /// Each seat's kills.
    pub seat_kills: Vec<u32>,
    /// Each seat's shared kills.
    pub seat_shared_kills: Vec<u32>,
    /// Each seat's losses.
    pub seat_losses: Vec<u32>,
    /// The last enemy to attack each seat's assets
    /// ([`crate::tables::SeatTable::attacks`]).
    pub seat_attacker: Vec<u32>,
    /// When.
    pub seat_attacked_at: Vec<u32>,

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
    /// Whether each unit row is free, `0` or `1` (S2's plan, E1).
    pub unit_free: Vec<u8>,
    /// The tick each unit's weapon may next fire on ([`crate::tables::Arms`]).
    pub unit_ready_at: Vec<u32>,
    /// The last enemy that damaged each unit, or
    /// [`crate::tables::NO_ATTACKER`].
    pub unit_attacker: Vec<u32>,
    /// When, or [`crate::tables::NEVER_ATTACKED`].
    pub unit_attacked_at: Vec<u32>,
    /// The target each unit's last return-fire shot answered
    /// ([`crate::tables::Arms::engaged`]).
    pub unit_engaged: Vec<u32>,
    /// The beacon each unit is retreating to, or [`BeaconId::NONE`].
    pub unit_retreat: Vec<u32>,

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
    /// Each beacon's per-seat ordinal, what a seat's `b_NN` names
    /// ([`crate::tables::BeaconTable::ordinals`]).
    pub beacon_ordinal: Vec<u32>,
    /// The Mine mandate's `dig_max_depth`, per beacon.
    pub beacon_dig_depth: Vec<u32>,
    /// The Mine mandate's `pillar_spacing`, per beacon.
    pub beacon_pillars: Vec<u32>,
    /// The Mine mandate's `seam_choice`, per beacon, as
    /// [`crate::mining::SeamChoice::id`].
    pub beacon_seam_choice: Vec<u8>,
    /// The Mine mandate's `flee_on_threat`, per beacon, `0` or `1`.
    pub beacon_flee: Vec<u8>,
    /// The seam each beacon holds, as an index into the map's feature table,
    /// or [`crate::targeting::NO_FEATURE`].
    pub beacon_seam: Vec<u32>,
    /// Whether each beacon row is free, `0` or `1`.
    pub beacon_free: Vec<u8>,
    /// The tick each beacon's weapon may next fire on: a core's.
    pub beacon_ready_at: Vec<u32>,
    /// The last enemy that damaged each beacon.
    pub beacon_attacker: Vec<u32>,
    /// When.
    pub beacon_attacked_at: Vec<u32>,
    /// The target each beacon's last return-fire shot answered.
    pub beacon_engaged: Vec<u32>,
    /// Each beacon's rules of engagement, `MandateSettings.roe`'s wire value.
    pub beacon_roe: Vec<u8>,
    /// Each beacon's retreat threshold, a whole percent.
    pub beacon_retreat_pct: Vec<u8>,
    /// Each beacon's `repair_threshold_pct`, a whole percent (S2's `site`).
    pub beacon_repair_threshold: Vec<u8>,
    /// Each beacon's `rebuild_destroyed`, 0 or 1.
    pub beacon_rebuild: Vec<u8>,
    /// Each beacon's `terraform`, [`crate::site::Terraform::id`].
    pub beacon_terraform: Vec<u8>,
    /// Each beacon's paid repair work, in `$`.
    pub beacon_repair_fund: Vec<i64>,
    /// Each beacon's paid voxel work, in `$`.
    pub beacon_build_fund: Vec<i64>,

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
    /// Each structure's rotation in quarter turns, 0 to 3.
    pub structure_rotation: Vec<u8>,
    /// The tick each ruin was ruined at, or [`crate::tables::NOT_RUINED`].
    pub structure_ruined_at: Vec<u32>,
    /// Whether each structure row is free, `0` or `1`.
    pub structure_free: Vec<u8>,
    /// The tick each structure's weapon may next fire on.
    pub structure_ready_at: Vec<u32>,
    /// The last enemy that damaged each structure.
    pub structure_attacker: Vec<u32>,
    /// When.
    pub structure_attacked_at: Vec<u32>,
    /// The target each structure's last return-fire shot answered.
    pub structure_engaged: Vec<u32>,

    /// Wreck ids.
    pub wreck_id: Vec<u32>,
    /// Three raw Q16.16 coordinates per wreck.
    pub wreck_pos: Vec<i32>,
    /// Salvage value per wreck, in `$`.
    pub wreck_salvage: Vec<i64>,
    /// Whether each wreck row is free, `0` or `1`.
    pub wreck_free: Vec<u8>,
    /// The tick each wreck was left on.
    pub wreck_left_at: Vec<u32>,

    /// Three raw Q16.16 coordinates per shell in flight: its aim point
    /// ([`crate::tables::ShellTable`]; S2's plan, E13).
    pub shell_aim: Vec<i32>,
    /// The tick each shell lands on.
    pub shell_lands_at: Vec<u32>,
    /// The seat each shell's firer belonged to.
    pub shell_seat: Vec<u8>,
    /// Each shell's firer, an [`crate::knowledge::AssetId`] raw value.
    pub shell_firer: Vec<u32>,
    /// Each shell's weapon ([`crate::tables::Armament::id`]).
    pub shell_weapon: Vec<u8>,
    /// Three raw Q16.16 coordinates per shell: where it was fired from (S2's
    /// `site`, version 12).
    pub shell_launch: Vec<i32>,
    /// The tick each shell was fired on.
    pub shell_fired_at: Vec<u32>,

    /// The beacon each list setting belongs to.
    pub target_beacon: Vec<u32>,
    /// [`crate::tables::TargetKind::id`] per list setting.
    pub target_kind: Vec<u8>,
    /// [`crate::tables::StructureKind::id`] per list setting, or zero.
    pub target_blueprint: Vec<u8>,
    /// Three raw Q16.16 coordinates per list setting's anchor.
    pub target_at: Vec<i32>,
    /// Four whole-voxel columns per list setting, its footprint: the area's
    /// `[min x, min y, max x, max y]` ([`crate::tables::ColumnBox`]), and four
    /// zeroes on a Build target.
    pub target_area: Vec<i16>,
    /// The structure realising each Build target.
    pub target_built: Vec<u32>,
    /// The feature each Build target is bound to, or
    /// [`crate::targeting::NO_FEATURE`].
    pub target_feature: Vec<u32>,
    /// How each Build target was written ([`crate::targeting::DESCRIPTION_VOXEL`]
    /// and its two siblings).
    pub target_desc: Vec<u8>,
    /// Each Build target's place in the build order; zero on other rows.
    pub target_order: Vec<u32>,
    /// Each Build target's rotation in quarter turns, 0 to 3; zero on other
    /// rows.
    pub target_rotation: Vec<u8>,
    /// Each wall target's top, [`crate::tables::NO_WALL`] on other rows (S2's
    /// `site`).
    pub target_wall_top: Vec<i16>,
    /// Whether each wall target's box has been filled whole once, 0 or 1.
    pub target_wall_done: Vec<u8>,

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
    /// The feature a `covering` step chose, per seat.
    pub plan_bound_feature: Vec<u32>,
    /// How many bindings each seat's step in progress holds.
    pub plan_binding_count: Vec<u32>,
    /// Each binding's feature, packed in seat order behind
    /// [`Snapshot::plan_binding_count`].
    pub plan_binding_feature: Vec<u32>,
    /// Each binding's column, three whole voxels per binding, packed the same
    /// way.
    pub plan_binding_at: Vec<i32>,
    /// Whether each seat's step is a placement resumed as a visit, `0` or `1`.
    pub plan_resumed: Vec<u8>,
    /// The beacon a restarted placement resumes at, per seat.
    pub plan_restart_beacon: Vec<u32>,
    /// The rule body that placement is in, per seat.
    pub plan_restart_rule: Vec<u32>,
    /// That placement's step index, per seat.
    pub plan_restart_at: Vec<u32>,
    /// The first initial row it had not committed, per seat.
    pub plan_restart_row: Vec<u32>,
    /// S2's `dfnd` (version 13): each Defend beacon, a [`BeaconId`] raw
    /// value, ascending.
    pub defend_beacon: Vec<u32>,
    /// Each Defend beacon's engagement radius percentage, `0` unwritten.
    pub defend_engagement: Vec<u8>,
    /// Each Defend beacon's pursue setting ([`crate::tables::Pursue::id`]).
    pub defend_pursue: Vec<u8>,
    /// Each Defend beacon's picket count.
    pub defend_pickets: Vec<u8>,
    /// Whether each Defend beacon escorts its seat's commander, `0` or `1`.
    pub defend_escort: Vec<u8>,
    /// Each Defend beacon's guard assignment, an
    /// [`crate::knowledge::AssetId`] raw value or [`crate::tables::NO_QUARRY`].
    pub defend_quarry: Vec<u32>,
    /// Each Defend beacon's picket leg.
    pub defend_leg: Vec<u8>,
    /// The protect list's defending beacon, per entry.
    pub defend_protect_by: Vec<u32>,
    /// The protect list's protected beacon, per entry.
    pub defend_protect_of: Vec<u32>,
    /// The explored table's seat, per row (S2's `surv`, laid out by `dfnd`).
    pub explored_seat: Vec<u8>,
    /// The explored table's cell, per row.
    pub explored_cell: Vec<u32>,
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
    #[allow(
        clippy::too_many_lines,
        reason = "one line per column of a flat projection, for the reason `capture` gives"
    )]
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
            seat_placed: Vec::new(),
            seat_beacons_lost: Vec::new(),
            seat_destroyed: Vec::new(),
            seat_kills: Vec::new(),
            seat_shared_kills: Vec::new(),
            seat_losses: Vec::new(),
            seat_attacker: Vec::new(),
            seat_attacked_at: Vec::new(),
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
            unit_free: Vec::new(),
            unit_ready_at: Vec::new(),
            unit_attacker: Vec::new(),
            unit_attacked_at: Vec::new(),
            unit_engaged: Vec::new(),
            unit_retreat: Vec::new(),
            beacon_id: Vec::new(),
            beacon_seat: Vec::new(),
            beacon_pos: Vec::new(),
            beacon_mandate: Vec::new(),
            beacon_program: Vec::new(),
            beacon_hp: Vec::new(),
            beacon_dormant: Vec::new(),
            beacon_priority: Vec::new(),
            beacon_scouts: Vec::new(),
            beacon_ordinal: Vec::new(),
            beacon_dig_depth: Vec::new(),
            beacon_pillars: Vec::new(),
            beacon_seam_choice: Vec::new(),
            beacon_flee: Vec::new(),
            beacon_seam: Vec::new(),
            beacon_free: Vec::new(),
            beacon_ready_at: Vec::new(),
            beacon_attacker: Vec::new(),
            beacon_attacked_at: Vec::new(),
            beacon_engaged: Vec::new(),
            beacon_roe: Vec::new(),
            beacon_retreat_pct: Vec::new(),
            beacon_repair_threshold: Vec::new(),
            beacon_rebuild: Vec::new(),
            beacon_terraform: Vec::new(),
            beacon_repair_fund: Vec::new(),
            beacon_build_fund: Vec::new(),
            structure_id: Vec::new(),
            structure_seat: Vec::new(),
            structure_kind: Vec::new(),
            structure_pos: Vec::new(),
            structure_hp: Vec::new(),
            structure_home: Vec::new(),
            structure_building: Vec::new(),
            structure_rotation: Vec::new(),
            structure_ruined_at: Vec::new(),
            structure_free: Vec::new(),
            structure_ready_at: Vec::new(),
            structure_attacker: Vec::new(),
            structure_attacked_at: Vec::new(),
            structure_engaged: Vec::new(),
            wreck_id: Vec::new(),
            wreck_pos: Vec::new(),
            wreck_salvage: Vec::new(),
            wreck_free: Vec::new(),
            wreck_left_at: Vec::new(),
            shell_aim: Vec::new(),
            shell_lands_at: Vec::new(),
            shell_seat: Vec::new(),
            shell_firer: Vec::new(),
            shell_weapon: Vec::new(),
            shell_launch: Vec::new(),
            shell_fired_at: Vec::new(),
            target_beacon: Vec::new(),
            target_kind: Vec::new(),
            target_blueprint: Vec::new(),
            target_at: Vec::new(),
            target_area: Vec::new(),
            target_built: Vec::new(),
            target_feature: Vec::new(),
            target_desc: Vec::new(),
            target_order: Vec::new(),
            target_rotation: Vec::new(),
            target_wall_top: Vec::new(),
            target_wall_done: Vec::new(),
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
            plan_bound_feature: Vec::new(),
            plan_binding_count: Vec::new(),
            plan_binding_feature: Vec::new(),
            plan_binding_at: Vec::new(),
            plan_resumed: Vec::new(),
            plan_restart_beacon: Vec::new(),
            plan_restart_rule: Vec::new(),
            plan_restart_at: Vec::new(),
            plan_restart_row: Vec::new(),
            defend_beacon: Vec::new(),
            defend_engagement: Vec::new(),
            defend_pursue: Vec::new(),
            defend_pickets: Vec::new(),
            defend_escort: Vec::new(),
            defend_quarry: Vec::new(),
            defend_leg: Vec::new(),
            defend_protect_by: Vec::new(),
            defend_protect_of: Vec::new(),
            explored_seat: Vec::new(),
            explored_cell: Vec::new(),
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
    /// A column holds a value its type refuses -- a rotation past three
    /// quarter turns, an inside-out area -- which no run of this sim writes:
    /// an edited file, named by its column.
    OutOfRange(&'static str),
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
    /// The snapshot holds more units than the receiving world's unit ceiling
    /// ([`World::unit_limit`]), which comes from the receiving world's own
    /// match configuration. The file was saved under another configuration
    /// (a larger `units_per_seat`, say): restoring it would either overflow
    /// the table or resume with a ceiling the unbroken run did not have.
    OverCeiling {
        /// How many units the snapshot holds.
        units: u32,
        /// The receiving world's ceiling.
        limit: u32,
    },
    /// The snapshot holds more beacon rows than a match of its own seat count
    /// can ever place (the cores plus every seat's share of the beacon room).
    /// The file describes no match this sim plays.
    OverBeaconCeiling {
        /// How many beacons the snapshot holds.
        beacons: u32,
        /// The ceiling for the snapshot's seat count.
        limit: u32,
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
    /// A tick is open between two of its phases: a restore into it would
    /// leave half a tick behind (S2's plan, E30).
    MidTick,
    /// The match settings are ones a lobby would be refused: a zero round
    /// limit, a ladder entry under a tick, a ladder past its bound (S2's plan,
    /// E7). Refused rather than normalised.
    Settings(crate::runner::SettingsError),
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
            SnapshotError::OutOfRange(column) => {
                write!(f, "the `{column}` column holds a value this build refuses")
            }
            SnapshotError::Map(error) => {
                write!(f, "regenerating the map from the snapshot's seed: {error}")
            }
            SnapshotError::Unindexable { units } => write!(
                f,
                "the snapshot's {units} units cannot be indexed by a broadphase grid built from \
                 this world's rules table"
            ),
            SnapshotError::OverCeiling { units, limit } => write!(
                f,
                "the snapshot holds {units} units and this world's unit ceiling is {limit}: it was saved under another match configuration"
            ),
            SnapshotError::OverBeaconCeiling { beacons, limit } => write!(
                f,
                "the snapshot holds {beacons} beacons and a match of its seat count places at most {limit}"
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
            SnapshotError::MidTick => write!(f, "a tick is open between two of its phases"),
            SnapshotError::Settings(error) => {
                write!(f, "the snapshot's match settings are refused: {error}")
            }
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
        let shells = world.shells();
        let defend = world.defend();
        let explored = world.explored();

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
            seat_placed: seats.placed().to_vec(),
            seat_beacons_lost: seats.beacons_lost().to_vec(),
            seat_destroyed: seats.tallies().iter().map(|t| t.destroyed.raw()).collect(),
            seat_kills: seats.tallies().iter().map(|t| t.kills).collect(),
            seat_shared_kills: seats.tallies().iter().map(|t| t.shared_kills).collect(),
            seat_losses: seats.tallies().iter().map(|t| t.losses).collect(),
            seat_attacker: seats.attacks().iter().map(|a| a.attacker).collect(),
            seat_attacked_at: seats.attacks().iter().map(|a| a.attacked_at).collect(),

            unit_id: units.ids().to_vec(),
            unit_seat: units.seats().to_vec(),
            unit_kind: units.kinds().to_vec(),
            unit_pos: axes_from_points(units.positions()),
            unit_dest: axes_from_points(units.destinations()),
            unit_heading: units.headings().iter().map(|a| a.raw()).collect(),
            unit_hp: units.hit_points().iter().map(|h| h.raw()).collect(),
            unit_home: units.homes().iter().map(|home| home.raw()).collect(),
            unit_carrying: units.carrying().iter().map(|m| m.raw()).collect(),
            unit_busy_until: units.busy_until().to_vec(),
            unit_free: units.frees().iter().map(|f| u8::from(*f)).collect(),
            unit_ready_at: units.arms().iter().map(|a| a.ready_at).collect(),
            unit_attacker: units.arms().iter().map(|a| a.attacker).collect(),
            unit_attacked_at: units.arms().iter().map(|a| a.attacked_at).collect(),
            unit_engaged: units.arms().iter().map(|a| a.engaged).collect(),
            unit_retreat: units.retreats().iter().map(|b| b.raw()).collect(),

            beacon_id: beacons.ids().to_vec(),
            beacon_seat: beacons.seats().to_vec(),
            beacon_pos: axes_from_points(beacons.positions()),
            beacon_mandate: beacons.mandates().to_vec(),
            beacon_program: beacons.programs().to_vec(),
            beacon_hp: beacons.hit_points().iter().map(|h| h.raw()).collect(),
            beacon_dormant: beacons.dormant().iter().map(|d| u8::from(*d)).collect(),
            beacon_priority: beacons.priorities().to_vec(),
            beacon_scouts: beacons.scout_counts().to_vec(),
            beacon_ordinal: beacons.ordinals().to_vec(),
            beacon_dig_depth: beacons.dig_depths().to_vec(),
            beacon_pillars: beacons.pillar_spacings().to_vec(),
            beacon_seam_choice: beacons.seam_choices().to_vec(),
            beacon_flee: beacons.flees().iter().map(|f| u8::from(*f)).collect(),
            beacon_seam: beacons.seams().to_vec(),
            beacon_free: beacons.frees().iter().map(|f| u8::from(*f)).collect(),
            beacon_ready_at: beacons.arms().iter().map(|a| a.ready_at).collect(),
            beacon_attacker: beacons.arms().iter().map(|a| a.attacker).collect(),
            beacon_attacked_at: beacons.arms().iter().map(|a| a.attacked_at).collect(),
            beacon_engaged: beacons.arms().iter().map(|a| a.engaged).collect(),
            beacon_roe: beacons.commons().iter().map(|c| c.roe).collect(),
            beacon_retreat_pct: beacons.commons().iter().map(|c| c.retreat_hp_pct).collect(),
            beacon_repair_threshold: beacons
                .sites()
                .iter()
                .map(|s| s.settings.repair_threshold_pct)
                .collect(),
            beacon_rebuild: beacons
                .sites()
                .iter()
                .map(|s| u8::from(s.settings.rebuild_destroyed))
                .collect(),
            beacon_terraform: beacons
                .sites()
                .iter()
                .map(|s| s.settings.terraform.id())
                .collect(),
            beacon_repair_fund: beacons
                .sites()
                .iter()
                .map(|s| s.repair_fund.raw())
                .collect(),
            beacon_build_fund: beacons.sites().iter().map(|s| s.build_fund.raw()).collect(),

            structure_id: structures.ids().to_vec(),
            structure_seat: structures.seats().to_vec(),
            structure_kind: structures.kinds().to_vec(),
            structure_pos: axes_from_points(structures.positions()),
            structure_hp: structures.hit_points().iter().map(|h| h.raw()).collect(),
            structure_home: structures.homes().iter().map(|home| home.raw()).collect(),
            structure_building: structures.building().iter().map(|b| u8::from(*b)).collect(),
            structure_rotation: structures.rotations().to_vec(),
            structure_ruined_at: structures.ruined_at().to_vec(),
            structure_free: structures.frees().iter().map(|f| u8::from(*f)).collect(),
            structure_ready_at: structures.arms().iter().map(|a| a.ready_at).collect(),
            structure_attacker: structures.arms().iter().map(|a| a.attacker).collect(),
            structure_attacked_at: structures.arms().iter().map(|a| a.attacked_at).collect(),
            structure_engaged: structures.arms().iter().map(|a| a.engaged).collect(),

            wreck_id: wrecks.ids().to_vec(),
            wreck_pos: axes_from_points(wrecks.positions()),
            wreck_salvage: wrecks.salvages().iter().map(|m| m.raw()).collect(),
            wreck_free: wrecks.frees().iter().map(|f| u8::from(*f)).collect(),
            wreck_left_at: wrecks.left_at().to_vec(),

            shell_aim: axes_from_points(shells.aims()),
            shell_lands_at: shells.lands_at().to_vec(),
            shell_seat: shells.seats().to_vec(),
            shell_firer: shells.firers().to_vec(),
            shell_weapon: shells.weapons().to_vec(),
            shell_launch: axes_from_points(shells.launches()),
            shell_fired_at: shells.fired_at().to_vec(),

            target_beacon: targets
                .beacons()
                .iter()
                .map(|beacon| beacon.raw())
                .collect(),
            target_kind: targets.kinds().to_vec(),
            target_blueprint: targets.blueprints().to_vec(),
            target_at: axes_from_points(targets.anchors()),
            target_area: axes_from_boxes(targets.areas()),
            target_built: targets.built().iter().map(|built| built.raw()).collect(),
            target_feature: targets.features().to_vec(),
            target_desc: targets.descriptions().to_vec(),
            target_order: targets.orders().to_vec(),
            target_rotation: targets.rotations().to_vec(),
            target_wall_top: targets.wall_tops().to_vec(),
            target_wall_done: targets.walls_done().iter().map(|d| u8::from(*d)).collect(),

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
            plan_bound_feature: plan.bound_feature,
            plan_binding_count: plan.binding_count,
            plan_binding_feature: plan.binding_feature,
            plan_binding_at: plan.binding_at,
            plan_resumed: plan.resumed,
            plan_restart_beacon: plan.restart_beacon,
            plan_restart_rule: plan.restart_rule,
            plan_restart_at: plan.restart_at,
            plan_restart_row: plan.restart_row,
            defend_beacon: defend.rows().iter().map(|row| row.beacon.raw()).collect(),
            defend_engagement: defend
                .rows()
                .iter()
                .map(|row| row.settings.engagement_pct)
                .collect(),
            defend_pursue: defend
                .rows()
                .iter()
                .map(|row| row.settings.pursue.id())
                .collect(),
            defend_pickets: defend
                .rows()
                .iter()
                .map(|row| row.settings.pickets)
                .collect(),
            defend_escort: defend
                .rows()
                .iter()
                .map(|row| u8::from(row.settings.escort))
                .collect(),
            defend_quarry: defend.rows().iter().map(|row| row.quarry).collect(),
            defend_leg: defend.rows().iter().map(|row| row.leg).collect(),
            defend_protect_by: defend.protected().iter().map(|(by, _)| by.raw()).collect(),
            defend_protect_of: defend.protected().iter().map(|(_, of)| of.raw()).collect(),
            explored_seat: explored.rows().iter().map(|(seat, _)| *seat).collect(),
            explored_cell: explored.rows().iter().map(|(_, cell)| *cell).collect(),
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
    /// (T17) rather than silently half-applied here.
    ///
    /// **The receiving world must be built from the snapshot's own match
    /// configuration** (its seat count, its `units_per_seat`), as
    /// `Host::resume` builds it from the config line. The world keeps its
    /// unit ceiling ([`World::unit_limit`]), which construction derived from
    /// that configuration's starting count, so a resumed match has exactly the
    /// room the unbroken run had. A snapshot with more units than that ceiling
    /// is refused ([`SnapshotError::OverCeiling`]); one with fewer restores,
    /// and its derived indexes are rebuilt for the restored tables before
    /// anything is written, but a world built under another configuration
    /// resumes with that configuration's ceiling, not the saved match's.
    ///
    /// The map is regenerated from [`Snapshot::match_seed`] under the receiving
    /// world's rules and the snapshot's own seat count, and the modified chunks
    /// are then written over it.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Ragged`] when the columns disagree in length,
    /// [`SnapshotError::Map`] when the map cannot be regenerated, or
    /// [`SnapshotError::OverCeiling`] when the snapshot holds more units than
    /// the receiving world's unit ceiling, [`SnapshotError::OverBeaconCeiling`]
    /// when it holds more beacons than a match of its seat count can place,
    /// or [`SnapshotError::Unindexable`]
    /// when the receiving world's rules table cannot describe a grid for its
    /// ceiling. Either way the world is left exactly as it was.
    #[allow(
        clippy::too_many_lines,
        reason = "one block per table, and every block must run before the first assignment: a restore either applies in full or changes nothing, so the checks cannot be moved behind the writes"
    )]
    pub fn restore_into(&self, world: &mut World) -> Result<(), SnapshotError> {
        // E7: rule: a capacity only; the seat table's restore checks the columns it is handed
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
            placed: self.seat_placed.clone(),
            beacons_lost: self.seat_beacons_lost.clone(),
            tally: tallies(self)?,
            attack: arms(
                &self
                    .seat_attacked_at
                    .iter()
                    .map(|_| 0)
                    .collect::<Vec<u32>>(),
                &self.seat_attacker,
                &self.seat_attacked_at,
                &self
                    .seat_attacked_at
                    .iter()
                    .map(|_| crate::tables::NO_ATTACKER)
                    .collect::<Vec<u32>>(),
                self.tick,
                "seat_attacker",
            )?,
        }) {
            return Err(SnapshotError::Ragged("seat"));
        }

        let match_state = self.restore_match()?;
        self.check_codes()?;
        self.check_rows()?;

        // E7: rule: a capacity only; the unit table's restore checks the columns it is handed
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
            home: self.unit_home.iter().copied().map(BeaconId::new).collect(),
            carrying: self.unit_carrying.iter().copied().map(Money::new).collect(),
            busy_until: self.unit_busy_until.clone(),
            arms: arms(
                &self.unit_ready_at,
                &self.unit_attacker,
                &self.unit_attacked_at,
                &self.unit_engaged,
                self.tick,
                "unit_ready_at",
            )?,
            retreat: self
                .unit_retreat
                .iter()
                .copied()
                .map(BeaconId::new)
                .collect(),
            free: flags(&self.unit_free, "unit_free")?,
        }) {
            return Err(SnapshotError::Ragged("unit"));
        }

        // The beacon ceiling is a function of the seat count alone
        // (`beacon_limit_of`), so a file holding more rows describes no match
        // this sim plays; refused before the table is built or checked, so a
        // crafted column costs nothing to turn away.
        let beacon_limit = crate::world::beacon_limit_of(seat_count);
        // E7: rule: a column past `u32` rows is past every beacon ceiling, and is refused just below
        let beacon_count = u32::try_from(self.beacon_id.len()).unwrap_or(u32::MAX);
        if beacon_count > beacon_limit {
            return Err(SnapshotError::OverBeaconCeiling {
                beacons: beacon_count,
                limit: beacon_limit,
            });
        }
        let mut beacons = BeaconTable::with_capacity(beacon_count);
        if !beacons.restore(BeaconColumns {
            id: self.beacon_id.clone(),
            seat: self.beacon_seat.clone(),
            pos: points_from_axes(&self.beacon_pos).ok_or(SnapshotError::Ragged("beacon_pos"))?,
            mandate: self.beacon_mandate.clone(),
            program: self.beacon_program.clone(),
            hp: self.beacon_hp.iter().copied().map(Hp::new).collect(),
            dormant: flags(&self.beacon_dormant, "beacon_dormant")?,
            priority: self.beacon_priority.clone(),
            scouts: self.beacon_scouts.clone(),
            ordinal: self.beacon_ordinal.clone(),
            dig_depth: self.beacon_dig_depth.clone(),
            pillars: self.beacon_pillars.clone(),
            seam_choice: self.beacon_seam_choice.clone(),
            flee: flags(&self.beacon_flee, "beacon_flee")?,
            seam: self.beacon_seam.clone(),
            arms: arms(
                &self.beacon_ready_at,
                &self.beacon_attacker,
                &self.beacon_attacked_at,
                &self.beacon_engaged,
                self.tick,
                "beacon_ready_at",
            )?,
            common: commons(&self.beacon_roe, &self.beacon_retreat_pct)?,
            site: self.sites()?,
            free: flags(&self.beacon_free, "beacon_free")?,
        }) {
            return Err(SnapshotError::Ragged("beacon"));
        }
        // Every beacon a seat holds carries an ordinal below the seat's
        // placement counter (S2's plan, E1): a name is taken from the counter
        // when the beacon is placed, so a file holding one at or past it names a
        // beacon no run of this sim placed.
        let named_past_the_counter = beacons
            .seats()
            .iter()
            .zip(beacons.ordinals())
            .zip(beacons.frees())
            .any(|((seat, ordinal), free)| {
                !*free
                    && self
                        .seat_placed
                        .get(usize::from(*seat))
                        .is_none_or(|placed| *ordinal >= *placed)
            });
        if named_past_the_counter {
            return Err(SnapshotError::OutOfRange("beacon_ordinal"));
        }

        let mut structures =
            // E7: rule: a capacity only; the structure table's restore checks the columns it is handed
            StructureTable::with_capacity(u32::try_from(self.structure_id.len()).unwrap_or(0));
        if !structures.restore(StructureColumns {
            id: self.structure_id.clone(),
            seat: self.structure_seat.clone(),
            kind: self.structure_kind.clone(),
            pos: points_from_axes(&self.structure_pos)
                .ok_or(SnapshotError::Ragged("structure_pos"))?,
            hp: self.structure_hp.iter().copied().map(Hp::new).collect(),
            home: self
                .structure_home
                .iter()
                .copied()
                .map(BeaconId::new)
                .collect(),
            building: flags(&self.structure_building, "structure_building")?,
            rotation: rotations(&self.structure_rotation, "structure_rotation")?,
            ruined_at: self.structure_ruined_at.clone(),
            arms: arms(
                &self.structure_ready_at,
                &self.structure_attacker,
                &self.structure_attacked_at,
                &self.structure_engaged,
                self.tick,
                "structure_ready_at",
            )?,
            free: flags(&self.structure_free, "structure_free")?,
        }) {
            return Err(SnapshotError::Ragged("structure"));
        }

        // E7: rule: a capacity only; the wreck table's restore checks the columns it is handed
        let mut wrecks = WreckTable::with_capacity(u32::try_from(self.wreck_id.len()).unwrap_or(0));
        if !wrecks.restore(
            self.wreck_id.clone(),
            points_from_axes(&self.wreck_pos).ok_or(SnapshotError::Ragged("wreck_pos"))?,
            self.wreck_salvage.iter().copied().map(Money::new).collect(),
            self.wreck_left_at.clone(),
            flags(&self.wreck_free, "wreck_free")?,
        ) {
            return Err(SnapshotError::Ragged("wreck"));
        }
        if wrecks.len() > world.wreck_limit() {
            return Err(SnapshotError::OutOfRange("wreck_id"));
        }
        let shells = self.restore_shells(world)?;
        let defend = self.restore_defend(&beacons)?;
        let explored = self.restore_explored()?;

        let targets = self.restore_targets(world)?;
        let sightings = self.restore_sightings(world)?;
        let credit = self.restore_credit(world)?;

        let (voxels, chunks, features, bore, generated) = self.restore_store(world, seat_count)?;
        // A held seam names a seam of the regenerated map, or none: an index
        // past the table, or one naming a vent, describes no beacon this sim
        // builds.
        let held_ok = beacons.seams().iter().all(|seam| {
            *seam == crate::targeting::NO_FEATURE
                || usize::try_from(*seam)
                    .ok()
                    .and_then(|index| features.get(index))
                    .is_some_and(|feature| feature.kind == crate::features::FeatureKind::Seam)
        });
        if !held_ok {
            return Err(SnapshotError::Ragged("beacon_seam"));
        }

        let unit_count = units.len();
        if unit_count > world.unit_limit() {
            return Err(SnapshotError::OverCeiling {
                units: unit_count,
                limit: world.unit_limit(),
            });
        }
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
            shells,
            defend,
            explored,
            voxels,
            chunks,
            features,
            bore,
            generated,
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
                beacon: self
                    .target_beacon
                    .iter()
                    .copied()
                    .map(BeaconId::new)
                    .collect(),
                kind: self.target_kind.clone(),
                blueprint: self.target_blueprint.clone(),
                at: points_from_axes(&self.target_at).ok_or(SnapshotError::Ragged("target_at"))?,
                area: boxes_from_axes(&self.target_area)?,
                built: self
                    .target_built
                    .iter()
                    .copied()
                    .map(StructureId::new)
                    .collect(),
                feature: self.target_feature.clone(),
                desc: self.target_desc.clone(),
                order: self.target_order.clone(),
                rotation: rotations(&self.target_rotation, "target_rotation")?,
                wall_top: self.target_wall_top.clone(),
                wall_done: flags(&self.target_wall_done, "target_wall_done")?,
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

    /// Every byte-coded column names a value this build defines (S2's #116
    /// reviews, C): a unit's kind and walk state, a structure's kind and a
    /// target's blueprint (a kind with a structure row, so never a wall; a
    /// blueprint may also be zero, on a row that names none), a
    /// beacon's mandate and priority, and a sighting's kind. The table
    /// restores check the columns they already know (a target's kind and
    /// description, a seam choice, a rotation, the flags, a visit's stage);
    /// these are the rest, refused here by column -- with every seat byte
    /// (a seat the file carries, or nobody where nobody is a value) and the
    /// interpreter's four flag bytes (`0` or `1`; the beacon, structure and
    /// route flags are `flags`' already). The beacon's seat is the beacon
    /// table's own check.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::OutOfRange`], naming the column.
    fn check_codes(&self) -> Result<(), SnapshotError> {
        use crate::tables::{
            PRIORITY_HIGH, PRIORITY_LOW, PRIORITY_NORMAL, StructureKind, UnitKind,
        };
        let row_kind =
            |id: &u8| StructureKind::from_id(*id).is_some_and(|kind| kind != StructureKind::Wall);
        let checks: [(&'static str, bool); 7] = [
            (
                "unit_kind",
                self.unit_kind
                    .iter()
                    .all(|id| UnitKind::from_id(*id).is_some()),
            ),
            (
                "unit_walk_state",
                self.unit_walk_state
                    .iter()
                    .all(|id| crate::pathing::router::WalkState::from_id(*id).is_some()),
            ),
            ("structure_kind", self.structure_kind.iter().all(row_kind)),
            (
                "target_blueprint",
                // A Build target may name a wall (S2's `site`), which no
                // structure row is.
                self.target_blueprint
                    .iter()
                    .all(|id| *id == 0 || StructureKind::from_id(*id).is_some()),
            ),
            (
                "beacon_mandate",
                self.beacon_mandate
                    .iter()
                    .all(|id| crate::seams::MandateKind::from_id(*id).is_some()),
            ),
            (
                "beacon_priority",
                self.beacon_priority
                    .iter()
                    .all(|id| [PRIORITY_LOW, PRIORITY_NORMAL, PRIORITY_HIGH].contains(id)),
            ),
            (
                "sighting_kind",
                self.sighting_kind
                    .iter()
                    .all(|id| crate::knowledge::AssetKind::from_id(*id).is_some()),
            ),
        ];
        // A seat byte names a seat the file carries; a sighting's owner and a
        // credit slot may also name nobody (`SeatId::NEUTRAL`), and the seat
        // table's own ids are its rows' indices.
        let seats = self.seat_id.len();
        let a_seat = |id: &u8| usize::from(*id) < seats;
        let a_seat_or_nobody = |id: &u8| *id == SeatId::NEUTRAL.raw() || a_seat(id);
        let a_flag = |byte: &u8| *byte <= 1;
        let more: [(&'static str, bool); 11] = [
            (
                "seat_id",
                self.seat_id
                    .iter()
                    .enumerate()
                    .all(|(index, id)| usize::from(*id) == index),
            ),
            // A free row is nobody's (S2's plan, E1), and so is a ruin.
            ("unit_seat", self.unit_seat.iter().all(a_seat_or_nobody)),
            ("beacon_seat", self.beacon_seat.iter().all(a_seat_or_nobody)),
            (
                "structure_seat",
                self.structure_seat.iter().all(a_seat_or_nobody),
            ),
            ("sighting_seat", self.sighting_seat.iter().all(a_seat)),
            (
                "sighting_owner",
                self.sighting_owner.iter().all(a_seat_or_nobody),
            ),
            ("credit_seat", self.credit_seat.iter().all(a_seat_or_nobody)),
            ("plan_pinned", self.plan_pinned.iter().all(a_flag)),
            (
                "plan_reflex_armed",
                self.plan_reflex_armed.iter().all(a_flag),
            ),
            (
                "plan_reflex_active",
                self.plan_reflex_active.iter().all(a_flag),
            ),
            ("plan_resumed", self.plan_resumed.iter().all(a_flag)),
        ];
        match checks.into_iter().chain(more).find(|(_, ok)| !ok) {
            Some((column, _)) => Err(SnapshotError::OutOfRange(column)),
            None => Ok(()),
        }
    }

    /// Every row is a row this sim writes (S2's plan, E1), beside
    /// [`Snapshot::check_codes`], each refusal naming its column:
    ///
    /// * every id names its own row (`unit_id`, `beacon_id`, `structure_id`,
    ///   `wreck_id`), and a seat's ordinals are unique among the beacons it
    ///   holds (`beacon_ordinal`);
    /// * a free row is nobody's, with no hit points and no home, and a free
    ///   structure is no ruin (`unit_free`, `beacon_free`, `structure_free`,
    ///   `wreck_free`); a live unit or beacon belongs to a real seat
    ///   (`unit_seat`, `beacon_seat`);
    /// * a live structure is nobody's exactly when it is a ruin, exactly when
    ///   its ruined-at tick is set, and that tick is not after the snapshot's
    ///   (`structure_ruined_at`);
    /// * no commander's row is free (`unit_free`), and every seat that has
    ///   placed a beacon still holds its core, ordinal 0 (`beacon_free`);
    /// * the ruins fit the reserve, and the structure and wreck tables their
    ///   rooms (`structure_ruined_at`, `structure_id`, `wreck_id`);
    /// * each seat has lost no more beacons than it placed, and its placement
    ///   counter has a next value (`seat_beacons_lost`, `seat_placed`).
    ///
    /// # Errors
    ///
    /// [`SnapshotError::OutOfRange`], naming the column.
    #[allow(
        clippy::too_many_lines,
        reason = "one check per rule of the row layout, every one before anything is written; split, the list would be spread over functions that each hold a piece of it"
    )]
    fn check_rows(&self) -> Result<(), SnapshotError> {
        use crate::tables::{NOT_RUINED, UnitKind, ids_name_their_rows};
        let refuse = |column: &'static str| Err(SnapshotError::OutOfRange(column));
        let nobody = SeatId::NEUTRAL.raw();
        let none = BeaconId::NONE.raw();
        let seats = self.seat_id.len();
        let a_seat = |seat: u8| usize::from(seat) < seats;
        for (column, ids) in [
            ("unit_id", &self.unit_id),
            ("beacon_id", &self.beacon_id),
            ("structure_id", &self.structure_id),
            ("wreck_id", &self.wreck_id),
        ] {
            if !ids_name_their_rows(ids) {
                return refuse(column);
            }
        }
        let mut held: std::collections::BTreeSet<(u8, u32)> = std::collections::BTreeSet::new();
        for ((seat, ordinal), free) in self
            .beacon_seat
            .iter()
            .zip(&self.beacon_ordinal)
            .zip(&self.beacon_free)
        {
            if *free == 0 && !held.insert((*seat, *ordinal)) {
                return refuse("beacon_ordinal");
            }
        }
        // Units: a free row is nobody's, with no hit points and no home; a
        // live one is a real seat's; a commander's row is never free.
        for ((((seat, hp), home), kind), free) in self
            .unit_seat
            .iter()
            .zip(&self.unit_hp)
            .zip(&self.unit_home)
            .zip(&self.unit_kind)
            .zip(&self.unit_free)
        {
            if *free != 0 {
                if *kind == UnitKind::Commander.id() {
                    return refuse("unit_free");
                }
                if *seat != nobody || *hp != 0 || *home != none {
                    return refuse("unit_free");
                }
            } else if !a_seat(*seat) {
                return refuse("unit_seat");
            }
        }
        for ((seat, hp), free) in self
            .beacon_seat
            .iter()
            .zip(&self.beacon_hp)
            .zip(&self.beacon_free)
        {
            if *free != 0 {
                if *seat != nobody || *hp != 0 {
                    return refuse("beacon_free");
                }
            } else if !a_seat(*seat) {
                return refuse("beacon_seat");
            }
        }
        // Every seat that has placed a beacon still holds its core.
        for (index, placed) in self.seat_placed.iter().enumerate() {
            let Ok(seat) = u8::try_from(index) else {
                return refuse("seat_placed");
            };
            if *placed > 0 && !held.contains(&(seat, 0)) {
                return refuse("beacon_free");
            }
        }
        let mut ruins: u32 = 0;
        for ((((seat, hp), home), ruined), free) in self
            .structure_seat
            .iter()
            .zip(&self.structure_hp)
            .zip(&self.structure_home)
            .zip(&self.structure_ruined_at)
            .zip(&self.structure_free)
        {
            if *free != 0 {
                if *seat != nobody || *hp != 0 || *home != none || *ruined != NOT_RUINED {
                    return refuse("structure_free");
                }
                continue;
            }
            let ruin = *seat == nobody;
            if ruin != (*ruined != NOT_RUINED) || (ruin && *ruined > self.tick) {
                return refuse("structure_ruined_at");
            }
            if ruin {
                ruins = ruins.saturating_add(1);
            } else if !a_seat(*seat) {
                return refuse("structure_seat");
            }
        }
        if ruins > crate::world::RUIN_RESERVE {
            return refuse("structure_ruined_at");
        }
        if self.structure_id.len() > crate::math::index::row(crate::world::STRUCTURE_ROWS) {
            return refuse("structure_id");
        }
        for (salvage, free) in self.wreck_salvage.iter().zip(&self.wreck_free) {
            if *free != 0 && *salvage != 0 {
                return refuse("wreck_free");
            }
        }
        for (placed, lost) in self.seat_placed.iter().zip(&self.seat_beacons_lost) {
            if *placed == u32::MAX {
                return refuse("seat_placed");
            }
            if lost > placed {
                return refuse("seat_beacons_lost");
            }
        }
        Ok(())
    }

    /// The match state, refused at the door when its phase or end reason is
    /// not one this build defines or its settings are ones a lobby refuses.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::MatchState`] for a phase or an end reason this
    /// build does not define, and [`SnapshotError::Settings`] for settings a
    /// lobby would be refused ([`crate::runner::MatchState::from_parts`]):
    /// neither is normalised.
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
        .map_err(|refused| match refused {
            crate::runner::PartsRefused::Structure => SnapshotError::MatchState {
                phase: self.match_phase,
                end_reason: self.match_end_reason,
            },
            crate::runner::PartsRefused::Settings(error) => SnapshotError::Settings(error),
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
            bound_feature: self.plan_bound_feature.clone(),
            binding_count: self.plan_binding_count.clone(),
            binding_feature: self.plan_binding_feature.clone(),
            binding_at: self.plan_binding_at.clone(),
            resumed: self.plan_resumed.clone(),
            restart_beacon: self.plan_restart_beacon.clone(),
            restart_rule: self.plan_restart_rule.clone(),
            restart_at: self.plan_restart_at.clone(),
            restart_row: self.plan_restart_row.clone(),
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
            packed = packed.saturating_add(crate::math::index::row(*len));
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
            route_partial: flags(&self.unit_route_partial, "unit_route_partial")?,
            nodes: self.route_nodes.clone(),
            cursor: self.repath_cursor,
        })
    }

    /// Rebuild the chunk store: the pristine layer from the seed, then the
    /// chunks an edit changed, then the digest column checked against both.
    /// The map's **feature table** comes back with the pristine layer: it is a
    /// pure function of the same three inputs and is regenerated, never
    /// carried (`crate::features`).
    ///
    /// Separate from [`Snapshot::restore_into`] because it is the half of a
    /// restore that is about the *map* rather than about the tables, and
    /// because a restore is the one place the digests — hashed state — are
    /// taken on a file's word unless somebody checks them.
    fn restore_store(
        &self,
        world: &World,
        seat_count: u32,
    ) -> Result<
        (
            VoxelStore,
            ChunkDigests,
            crate::features::FeatureTable,
            crate::bore::Bore,
            crate::terrain::GeneratedSurface,
        ),
        SnapshotError,
    > {
        if self.modified_chunk.len().saturating_mul(CHUNK_VOXELS) != self.modified_chunk_bytes.len()
        {
            return Err(SnapshotError::Ragged("modified_chunk"));
        }
        let generated = mapgen::generate(self.match_seed, world.rules(), seat_count)
            .map_err(SnapshotError::Map)?;
        let features = generated.features;
        let bore = generated.bore;
        let surface = generated.surface;
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
        if self.chunk_digest.len() != crate::math::index::row(voxels.chunk_count()) {
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
        Ok((voxels, chunks, features, bore, surface))
    }

    /// Each beacon's Build settings and paid work (S2's `site`), the five
    /// columns walked together.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::Ragged`] when they disagree in length, or hold a
    /// threshold past 100, a flag that is not 0 or 1, a terraform id this
    /// build does not name, or a negative fund.
    fn sites(&self) -> Result<Vec<crate::site::BeaconSite>, SnapshotError> {
        let n = self.beacon_repair_threshold.len();
        if self.beacon_rebuild.len() != n
            || self.beacon_terraform.len() != n
            || self.beacon_repair_fund.len() != n
            || self.beacon_build_fund.len() != n
        {
            return Err(SnapshotError::Ragged("beacon_repair_threshold"));
        }
        let rebuild = flags(&self.beacon_rebuild, "beacon_rebuild")?;
        let mut out = Vec::with_capacity(n);
        let rows = self
            .beacon_repair_threshold
            .iter()
            .zip(rebuild)
            .zip(self.beacon_terraform.iter())
            .zip(
                self.beacon_repair_fund
                    .iter()
                    .zip(self.beacon_build_fund.iter()),
            );
        for (((threshold, rebuild_destroyed), terraform), (repair, build)) in rows {
            let terraform = crate::site::Terraform::from_id(*terraform)
                .ok_or(SnapshotError::Ragged("beacon_terraform"))?;
            if *threshold > 100 {
                return Err(SnapshotError::Ragged("beacon_repair_threshold"));
            }
            if *repair < 0 || *build < 0 {
                return Err(SnapshotError::Ragged("beacon_repair_fund"));
            }
            out.push(crate::site::BeaconSite {
                settings: crate::site::BuildSettings {
                    repair_threshold_pct: *threshold,
                    rebuild_destroyed,
                    terraform,
                },
                repair_fund: Money::new(*repair),
                build_fund: Money::new(*build),
            });
        }
        Ok(out)
    }

    /// The shells in flight (S2's plan, E13), checked before anything is
    /// written: the columns are one length, every seat byte names a seat of
    /// the file, every weapon byte one this build arms, no shell has landed
    /// already, and the table fits the receiving world's room.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::Ragged`] for columns of different lengths or a table
    /// past the room, and [`SnapshotError::OutOfRange`] naming a column whose
    /// value describes no shell this sim fires.
    fn restore_shells(&self, world: &World) -> Result<ShellTable, SnapshotError> {
        let aims = points_from_axes(&self.shell_aim).ok_or(SnapshotError::Ragged("shell_aim"))?;
        let seats = self.seat_id.len();
        if self
            .shell_seat
            .iter()
            .any(|seat| usize::from(*seat) >= seats)
        {
            return Err(SnapshotError::OutOfRange("shell_seat"));
        }
        // Every weapon byte names an indirect weapon this build arms, and
        // every shell lands after the snapshot's tick and no later than that
        // weapon's longest flight from it.
        for (id, lands) in self.shell_weapon.iter().zip(&self.shell_lands_at) {
            let flight = crate::tables::Armament::from_id(*id)
                .and_then(|armament| world.weapon_of(armament))
                .and_then(crate::arms::WeaponRow::longest_flight_ticks)
                .ok_or(SnapshotError::OutOfRange("shell_weapon"))?;
            if *lands <= self.tick || *lands > self.tick.saturating_add(flight) {
                return Err(SnapshotError::OutOfRange("shell_lands_at"));
            }
        }
        // Every firer is a unit, a structure or a beacon.
        if self.shell_firer.iter().any(|firer| {
            !matches!(
                crate::knowledge::AssetId::new(*firer).tag(),
                crate::knowledge::AssetId::TAG_UNIT
                    | crate::knowledge::AssetId::TAG_STRUCTURE
                    | crate::knowledge::AssetId::TAG_BEACON
            )
        }) {
            return Err(SnapshotError::OutOfRange("shell_firer"));
        }
        let mut shells = ShellTable::with_room(world.shells().room());
        let launches =
            points_from_axes(&self.shell_launch).ok_or(SnapshotError::Ragged("shell_launch"))?;
        if !shells.restore(
            aims,
            self.shell_lands_at.clone(),
            (launches, self.shell_fired_at.clone()),
            self.shell_seat.clone(),
            self.shell_firer.clone(),
            self.shell_weapon.clone(),
        ) {
            return Err(SnapshotError::Ragged("shell"));
        }
        Ok(shells)
    }
}

impl Snapshot {
    /// The Defend table (S2's `dfnd`), checked before anything is written:
    /// the columns are one length, every row names a live beacon of the file
    /// on the Defend writ, ascending, every pursue byte is one this build
    /// names, every engagement percentage is at most 100, every quarry is a
    /// unit's id, every leg is a point of the picket's ring, and the protect
    /// list is grouped by a defending beacon that has a row and names live
    /// beacons.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::Ragged`] for columns of different lengths or an
    /// out-of-order table, and [`SnapshotError::OutOfRange`] naming a column
    /// whose value describes no Defend beacon this sim holds.
    fn restore_defend(
        &self,
        beacons: &BeaconTable,
    ) -> Result<crate::tables::DefendTable, SnapshotError> {
        use crate::tables::{DefendRow, DefendSettings, Pursue};
        let n = self.defend_beacon.len();
        if self.defend_engagement.len() != n
            || self.defend_pursue.len() != n
            || self.defend_pickets.len() != n
            || self.defend_escort.len() != n
            || self.defend_quarry.len() != n
            || self.defend_leg.len() != n
            || self.defend_protect_by.len() != self.defend_protect_of.len()
        {
            return Err(SnapshotError::Ragged("defend"));
        }
        let live_on = |raw: u32, defend: bool| -> bool {
            beacons.row_of(BeaconId::new(raw)).is_ok_and(|row| {
                let alive = beacons
                    .hit_points()
                    .get(row.index())
                    .is_some_and(|hp| hp.is_alive());
                let writ = beacons.mandates().get(row.index()).copied()
                    == Some(crate::seams::MandateKind::Defend.id());
                alive && (writ || !defend)
            })
        };
        let mut rows: Vec<DefendRow> = Vec::with_capacity(n);
        let columns = self
            .defend_beacon
            .iter()
            .zip(&self.defend_engagement)
            .zip(&self.defend_pursue)
            .zip(&self.defend_pickets)
            .zip(&self.defend_escort)
            .zip(&self.defend_quarry)
            .zip(&self.defend_leg);
        for ((((((beacon, engagement), pursue), pickets), escort), quarry), leg) in columns {
            if !live_on(*beacon, true) {
                return Err(SnapshotError::OutOfRange("defend_beacon"));
            }
            if *engagement > 100 {
                return Err(SnapshotError::OutOfRange("defend_engagement"));
            }
            let pursue =
                Pursue::from_id(*pursue).ok_or(SnapshotError::OutOfRange("defend_pursue"))?;
            let escort = match escort {
                0 => false,
                1 => true,
                _ => return Err(SnapshotError::OutOfRange("defend_escort")),
            };
            if *quarry != crate::tables::NO_QUARRY
                && crate::knowledge::AssetId::new(*quarry).tag()
                    != crate::knowledge::AssetId::TAG_UNIT
            {
                return Err(SnapshotError::OutOfRange("defend_quarry"));
            }
            if *leg >= crate::defend::PICKET_LEGS {
                return Err(SnapshotError::OutOfRange("defend_leg"));
            }
            rows.push(DefendRow {
                beacon: BeaconId::new(*beacon),
                settings: DefendSettings {
                    engagement_pct: *engagement,
                    pursue,
                    pickets: *pickets,
                    escort,
                },
                quarry: *quarry,
                leg: *leg,
            });
        }
        let protected: Vec<(BeaconId, BeaconId)> = self
            .defend_protect_by
            .iter()
            .zip(&self.defend_protect_of)
            .map(|(by, of)| (BeaconId::new(*by), BeaconId::new(*of)))
            .collect();
        if protected.iter().any(|(_, of)| !live_on(of.raw(), false)) {
            return Err(SnapshotError::OutOfRange("defend_protect_of"));
        }
        let mut table = crate::tables::DefendTable::with_room(beacons.len());
        if !table.restore(rows, protected) {
            return Err(SnapshotError::Ragged("defend"));
        }
        Ok(table)
    }

    /// The explored table (S2's `surv`, laid out by `dfnd`), checked before
    /// anything is written: the columns are one length, every seat byte names
    /// a seat of the file, and the rows are strictly ascending.
    ///
    /// # Errors
    ///
    /// [`SnapshotError::Ragged`] and [`SnapshotError::OutOfRange`].
    fn restore_explored(&self) -> Result<crate::tables::ExploredTable, SnapshotError> {
        if self.explored_seat.len() != self.explored_cell.len() {
            return Err(SnapshotError::Ragged("explored"));
        }
        let seats = self.seat_id.len();
        if self
            .explored_seat
            .iter()
            .any(|seat| usize::from(*seat) >= seats)
        {
            return Err(SnapshotError::OutOfRange("explored_seat"));
        }
        let mut table = crate::tables::ExploredTable::default();
        let rows = self
            .explored_seat
            .iter()
            .copied()
            .zip(self.explored_cell.iter().copied())
            .collect();
        if !table.restore(rows) {
            return Err(SnapshotError::Ragged("explored"));
        }
        Ok(table)
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

/// Each footprint as its four whole-voxel columns, in
/// [`Snapshot::target_area`]'s layout.
fn axes_from_boxes(areas: &[ColumnBox]) -> Vec<i16> {
    let mut out: Vec<i16> = Vec::with_capacity(areas.len().saturating_mul(4));
    for area in areas {
        let [lx, ly] = area.min_column();
        let [hx, hy] = area.max_column();
        out.extend_from_slice(&[lx, ly, hx, hy]);
    }
    out
}

/// [`axes_from_boxes`] read back: `Ragged` for a column that is not whole
/// boxes, and `OutOfRange` for an inside-out box, which no run writes.
fn boxes_from_axes(axes: &[i16]) -> Result<Vec<ColumnBox>, SnapshotError> {
    let (whole, remainder) = axes.as_chunks::<4>();
    if !remainder.is_empty() {
        return Err(SnapshotError::Ragged("target_area"));
    }
    whole
        .iter()
        .map(|&[lx, ly, hx, hy]| {
            ColumnBox::new([lx, ly], [hx, hy]).ok_or(SnapshotError::OutOfRange("target_area"))
        })
        .collect()
}

/// A rotation column checked byte by byte: `OutOfRange(name)` for a byte past
/// three quarter turns ([`QuarterTurns::new`]), which no run writes, rather
/// than the table's own refusal, which can only say the columns were ragged.
fn rotations(column: &[u8], name: &'static str) -> Result<Vec<u8>, SnapshotError> {
    if column
        .iter()
        .all(|turns| QuarterTurns::new(u32::from(*turns)).is_some())
    {
        Ok(column.to_vec())
    } else {
        Err(SnapshotError::OutOfRange(name))
    }
}

/// A column of flags, one byte each: `0` is false and `1` is true, and any
/// other byte is refused as `Ragged(name)`. The capture writes nothing else,
/// so a byte that is neither describes no world this sim saved, and reading
/// it as `true` would re-save a different file from the one restored.
/// Each seat's tally, the four columns walked together.
///
/// # Errors
///
/// [`SnapshotError::Ragged`] when the four columns disagree in length with the
/// seat column.
fn tallies(snapshot: &Snapshot) -> Result<Vec<Tally>, SnapshotError> {
    let n = snapshot.seat_id.len();
    if snapshot.seat_destroyed.len() != n
        || snapshot.seat_kills.len() != n
        || snapshot.seat_shared_kills.len() != n
        || snapshot.seat_losses.len() != n
    {
        return Err(SnapshotError::Ragged("seat_tally"));
    }
    // A tally only ever grows from zero, and a shared kill is a kill.
    if snapshot.seat_destroyed.iter().any(|value| *value < 0) {
        return Err(SnapshotError::OutOfRange("seat_destroyed"));
    }
    if snapshot
        .seat_shared_kills
        .iter()
        .zip(&snapshot.seat_kills)
        .any(|(shared, kills)| shared > kills)
    {
        return Err(SnapshotError::OutOfRange("seat_shared_kills"));
    }
    Ok(snapshot
        .seat_destroyed
        .iter()
        .zip(&snapshot.seat_kills)
        .zip(&snapshot.seat_shared_kills)
        .zip(&snapshot.seat_losses)
        .map(|(((destroyed, kills), shared_kills), losses)| Tally {
            destroyed: Money::new(*destroyed),
            kills: *kills,
            shared_kills: *shared_kills,
            losses: *losses,
        })
        .collect())
}

/// One table's combat memory, its four columns walked together.
///
/// # Errors
///
/// [`SnapshotError::Ragged`], naming the first column, when they disagree in
/// length, and [`SnapshotError::OutOfRange`] for an attack remembered from
/// after the snapshot's own tick, which no run of this sim records.
fn arms(
    ready_at: &[u32],
    attacker: &[u32],
    attacked_at: &[u32],
    engaged: &[u32],
    tick: u32,
    name: &'static str,
) -> Result<Vec<Arms>, SnapshotError> {
    if attacker.len() != ready_at.len()
        || attacked_at.len() != ready_at.len()
        || engaged.len() != ready_at.len()
    {
        return Err(SnapshotError::Ragged(name));
    }
    if attacked_at
        .iter()
        .any(|at| *at != crate::tables::NEVER_ATTACKED && *at > tick)
    {
        return Err(SnapshotError::OutOfRange(name));
    }
    Ok(ready_at
        .iter()
        .zip(attacker)
        .zip(attacked_at)
        .zip(engaged)
        .map(|(((ready_at, attacker), attacked_at), engaged)| Arms {
            ready_at: *ready_at,
            attacker: *attacker,
            attacked_at: *attacked_at,
            engaged: *engaged,
        })
        .collect())
}

/// Each beacon's Common settings, the two columns walked together.
///
/// # Errors
///
/// [`SnapshotError::Ragged`] when they disagree in length; the beacon
/// table's restore refuses a value out of range.
fn commons(roe: &[u8], retreat: &[u8]) -> Result<Vec<Common>, SnapshotError> {
    if roe.len() != retreat.len() {
        return Err(SnapshotError::Ragged("beacon_roe"));
    }
    Ok(roe
        .iter()
        .zip(retreat)
        .map(|(roe, retreat_hp_pct)| Common {
            roe: *roe,
            retreat_hp_pct: *retreat_hp_pct,
        })
        .collect())
}

fn flags(column: &[u8], name: &'static str) -> Result<Vec<bool>, SnapshotError> {
    column
        .iter()
        .map(|byte| match byte {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(SnapshotError::Ragged(name)),
        })
        .collect()
}

/// Three raw axes per point, in encoder order.
#[allow(
    clippy::integer_division,
    reason = "the length is a multiple of three by the test above, so the division is exact"
)]
fn points_from_axes(axes: &[i32]) -> Option<Vec<[Fx; 3]>> {
    if !axes.len().is_multiple_of(3) {
        return None;
    }
    let mut out = Vec::with_capacity(axes.len() / 3);
    for chunk in axes.as_chunks::<3>().0 {
        out.push([
            Fx::from_raw(*chunk.first()?),
            Fx::from_raw(*chunk.get(1)?),
            Fx::from_raw(*chunk.get(2)?),
        ]);
    }
    Some(out)
}
