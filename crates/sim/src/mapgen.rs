// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The seeded deterministic map generator (spec sections 3, 9 and 15; decisions
//! log items 55, 63, 65, 90, 91 and 95).
//!
//! A map is a pure function of `(match seed, rules table, occupied seats)`. It
//! draws from [`Stream::Map`] and from nowhere else, it is integer throughout,
//! and every number it uses comes from `rules/rules.v1.json` rather than from a
//! constant here — every exception is marked `PLACEHOLDER` below, names who
//! resolves it and when, and is a shape the rules table has no row for yet
//! (how lumpy the terrain is, how wide a seam's disc is, which sector a
//! starting feature is placed in).
//!
//! # What it produces
//!
//! 1. **Terrain.** An integer heightmap: two octaves of value noise over a
//!    lattice drawn on [`Stream::Map`], flattened inside each spawn zone, then
//!    closed to a **1-Lipschitz** field so that adjacent columns never differ by
//!    more than one voxel. No overhangs, so every column has exactly one
//!    walkable top and every column is reachable from every other by
//!    one-voxel steps. That is what makes vent reachability a property of the
//!    generator rather than a hope, and it is what T7's HPA\* graph is built on.
//! 2. **Spawn zones.** `map.spawn_zones` of them, `map.spawn_zone_radius_voxels`
//!    across. Each zone's disc is flattened to the height at its centre
//!    *before* the 1-Lipschitz closure runs, and the closure only ever lowers,
//!    so a zone is flat in the middle and its rim may be cut into a ramp where
//!    the ground outside is lower — never into a step of more than one voxel,
//!    which is the property that matters and the one
//!    `spawn_zones_are_flat_inside_and_ramped_at_the_rim` pins. Placed so the
//!    **spawn-distance rule** holds: the
//!    octile ground distance (10/14 per step, item 59) between any two occupied
//!    spawn centres is at least
//!    `raider_cost_per_second * numerator/denominator * segment_lengths_ms[0] / 1000`
//!    cost units, ceiling where it rounds. At the committed table that is
//!    3 240 cost units — about 324 cardinal voxels, 324 s of commander walking.
//!    Octile ground distance is a **lower bound** on any walked path, so the
//!    assertion is conservative in the right direction until T7 measures real
//!    routes (item 90).
//! 3. **Per occupied zone** (seat `i` gets zone `i`): a pre-placed core beacon
//!    on a Build mandate with `beacon.core_hp`, the commander,
//!    `units.starting_build_drones` build drones and
//!    `units.starting_mining_drones` mining drones beside it, a treasury of
//!    `economy.bmi_dollars * economy.starting_bmi_multiplier`, a supply of
//!    `power.core_surplus_kw`, a draw equal to the starting units' own, one
//!    heat vent at `map.start_vent_richness` in the band
//!    `map.vent_min_distance_voxels ..= map.vent_max_distance_voxels`, and one
//!    scrap seam of `economy.seam_voxels` voxels at `map.start_seam_richness`
//!    in `map.seam_min_distance_voxels ..= map.seam_max_distance_voxels`.
//!    **An unoccupied zone is terrain and nothing else** — no beacon, no vent,
//!    no seam (item 90).
//! 4. **Contested features toward the centre:** `map.contested_standard_vents`
//!    standard vents, `map.contested_rich_vents` rich vents and
//!    `map.contested_rich_seams` rich seams, each at least one beacon sphere
//!    clear of every spawn zone.
//!
//! # Two invariants the generator refuses to break
//!
//! * **The vent band is inside two beacon spheres.** Item 95 corrected item
//!   90's gloss: `commander.placement_range_voxels` is measured from the
//!   *commander* to the site, so the commander walks to the edge of the core's
//!   sphere and places a beacon up to `beacon.sphere_radius_voxels` from the
//!   core, whose own sphere then reaches twice that. The check here is against
//!   `2 * beacon.sphere_radius_voxels` and never against the placement range.
//! * **Total supply fits the per-map power ceiling.** Every vent on the map, if
//!   it carried a Generator, plus every occupied core's surplus, must be at
//!   most `power.map_ceiling_kw` (item 65's provisional 190 kW, with item 91's
//!   two `PLACEHOLDER`s inside it). A rules table that cannot satisfy that is a
//!   [`MapError`], not a panic.
//!
//! # Determinism
//!
//! Every draw is `StreamRng::new(seed, Stream::Map, phase, 0, index)`. There is
//! no tick and no seat at generation time, so the `tick` slot carries a
//! **generation phase** ([`Phase`]) and the `sub` slot carries the item's index
//! within that phase. Adding a phase with a fresh id is additive; renumbering
//! one moves every seed at once and is a determinism change (AGENTS.md
//! section 4.7).

use crate::math::fixed::{Angle, Fx, cos, sin};
use crate::math::quantity::{Hp, Kw, Money};
use crate::math::random::{Stream, StreamRng};
use crate::rules::RulesTable;
use crate::seams::{BeaconMandate, MandateKind, ProgramId};
use crate::tables::{BeaconId, BeaconTable, SeatId, UnitKind};
use crate::voxels::{Material, Richness, VoxelStore};
use pharmakos_proto::gp;
use serde::{Deserialize, Serialize};
use std::fmt;

/// The version stamped into a written map file. A bump makes older files
/// refuse to load rather than decode into a map of the wrong shape.
pub const MAP_FILE_VERSION: u32 = 1;

/// PLACEHOLDER: the terrain's own shape — the two noise octaves' lattice edges
/// and amplitudes, and the surface skin's depth — is **not** a rules-table row
/// today. The rows that exist describe the map's extent and its features; these
/// four describe how lumpy it is. Owner, at S4's symmetric map, which is the
/// stage that re-derives map generation anyway (skeleton plan decision 14).
const TERRAIN_COARSE_SHIFT: u32 = 6;
/// PLACEHOLDER: see [`TERRAIN_COARSE_SHIFT`].
const TERRAIN_COARSE_AMPLITUDE: i32 = 8;
/// PLACEHOLDER: see [`TERRAIN_COARSE_SHIFT`].
const TERRAIN_FINE_SHIFT: u32 = 4;
/// PLACEHOLDER: see [`TERRAIN_COARSE_SHIFT`].
const TERRAIN_FINE_AMPLITUDE: i32 = 3;
/// PLACEHOLDER: see [`TERRAIN_COARSE_SHIFT`]. How many voxels of the column's
/// top are dirt rather than stone.
const TERRAIN_SKIN_VOXELS: i32 = 3;

/// PLACEHOLDER: how far a zone centre may be jittered off its layout position,
/// in voxels. Four, inclusive at both ends, which leaves 30 cost units of slack
/// against the spawn-distance rule at the committed table — the arithmetic is
/// written out at [`zone_centres`]. Owner, with the rest of map generation at
/// S4.
const ZONE_JITTER_VOXELS: i32 = 4;

/// PLACEHOLDER: a heat vent is a three-by-three patch of surface voxels. The
/// patch's shape is tuning and nothing reads it yet — T14's Generator sits on
/// the vent, it does not mine it. Owner, at S1.
const VENT_PATCH_RADIUS: i32 = 1;

/// PLACEHOLDER: how many voxels of ore a seam column carries before the seam
/// moves to the next column. Four, so a 150-voxel seam is about 38 columns and
/// fits inside the disc below. Owner, at S1 with the mining rules.
const SEAM_VOXELS_PER_COLUMN: i32 = 4;

/// PLACEHOLDER: the radius of the disc a seam is laid into, in voxels. Five,
/// so `economy.seam_voxels` at [`SEAM_VOXELS_PER_COLUMN`] a column fits with
/// room to spare. It decides the same seam's shape that
/// [`SEAM_VOXELS_PER_COLUMN`] does, so it is the owner's at S1 with the mining
/// rules, and it moves every committed seed when it moves.
const SEAM_DISC_RADIUS: i32 = 5;

/// PLACEHOLDER: how many placements a contested feature may try before the
/// generator gives up and reports a rules table it cannot satisfy. Sixty-four,
/// which is far past what the committed table needs and still a bound rather
/// than a loop. Owner, at S4, with the rest of map generation.
const CONTESTED_ATTEMPTS: u32 = 64;

/// PLACEHOLDER: a sixth of a turn in [`Angle`] units — the half-width of the
/// inward sector a starting feature is placed in. It decides where every
/// starting vent and seam can land, so it moves every committed seed when it
/// moves. Owner, at S4's symmetric map.
const SECTOR_HALF_WIDTH: u16 = 10_922;

/// PLACEHOLDER: how far a contested feature must clear a *starting* vent or
/// seam, over and above the two features' own radii, in voxels. One, which is
/// the least that keeps the two discs disjoint so a contested seam can never
/// overwrite a vent whose power has already been counted. Owner, at S1, with
/// the mining rules that decide what a seam is worth.
const STARTING_FEATURE_CLEARANCE: i32 = 1;

/// Which draw of the generator a random number belongs to.
///
/// Carried in [`StreamRng`]'s `tick` slot, because generation happens before
/// tick zero. Ids are **additive only**: renumbering one re-rolls every map in
/// the committed seed set (AGENTS.md section 4.7).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Phase {
    /// The coarse terrain octave's lattice.
    TerrainCoarse,
    /// The fine terrain octave's lattice.
    TerrainFine,
    /// The spawn-zone layout, its rotation and its jitter.
    Layout,
    /// Each occupied zone's heat vent.
    StartVent,
    /// Each occupied zone's scrap seam.
    StartSeam,
    /// The contested vents and seams toward the centre.
    Contested,
}

impl Phase {
    /// The wire id. Additive only: never reuse, never renumber.
    #[must_use]
    pub const fn id(self) -> u32 {
        match self {
            Phase::TerrainCoarse => 0,
            Phase::TerrainFine => 1,
            Phase::Layout => 2,
            Phase::StartVent => 3,
            Phase::StartSeam => 4,
            Phase::Contested => 5,
        }
    }
}

/// What the generator could not do.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum MapError {
    /// A rules-table block the generator reads is absent. Proto3 cannot tell an
    /// absent message from an empty one, so the generator refuses rather than
    /// generating a map out of zeroes.
    MissingBlock(&'static str),
    /// A value is outside what the generator can build with.
    OutOfRange {
        /// The field's path in the message.
        field: &'static str,
        /// What is wrong with it.
        why: String,
    },
    /// The map's extent is not a whole number of 32-voxel chunks, or does not
    /// fit.
    BadExtent {
        /// What the table said.
        size: [u32; 3],
    },
    /// The spawn-distance rule cannot be met by the layout this table implies.
    SpawnsTooClose {
        /// The closest pair's octile ground cost.
        found: i64,
        /// What the rule asks for.
        required: i64,
    },
    /// Total generator supply would exceed the per-map power ceiling.
    PowerCeiling {
        /// What the map would supply if every vent carried a Generator.
        supply_kw: i32,
        /// `power.map_ceiling_kw`.
        ceiling_kw: i32,
    },
    /// A feature could not be placed anywhere the rules table allows.
    Unplaceable(&'static str),
    /// The rules table cannot describe a broadphase grid for the world's unit
    /// count. Not a map fault, but it is caught at the same moment and by the
    /// same call, so it shares the error type rather than adding a second one
    /// to every signature.
    Unindexable {
        /// How many units the world holds.
        units: u32,
    },
    /// A map file could not be encoded or decoded.
    Codec(String),
    /// A map file could not be read or written.
    Io {
        /// The path that was tried.
        path: String,
        /// The operating system's message.
        message: String,
    },
    /// A map file this build cannot read.
    FileVersion {
        /// What the bytes said.
        found: u32,
        /// What this build writes.
        expected: u32,
    },
}

impl fmt::Display for MapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MapError::MissingBlock(name) => write!(
                f,
                "the `{name}` block is missing; the map generator reads it and will not \
                 substitute zeroes for it"
            ),
            MapError::OutOfRange { field, why } => write!(f, "field `{field}`: {why}"),
            MapError::BadExtent { size } => write!(
                f,
                "map extent {size:?} is not a positive whole number of 32-voxel chunks"
            ),
            MapError::SpawnsTooClose { found, required } => write!(
                f,
                "the closest two spawn centres are {found} cost units apart; the spawn-distance \
                 rule asks for {required}"
            ),
            MapError::PowerCeiling {
                supply_kw,
                ceiling_kw,
            } => write!(
                f,
                "the map would supply {supply_kw} kW, over the per-map ceiling of {ceiling_kw} kW"
            ),
            MapError::Unplaceable(what) => {
                write!(f, "no legal position for {what} on this map")
            }
            MapError::Unindexable { units } => write!(
                f,
                "a broadphase grid for {units} units cannot be described by this rules table"
            ),
            MapError::Codec(message) => write!(f, "map file: {message}"),
            MapError::Io { path, message } => write!(f, "reading or writing {path}: {message}"),
            MapError::FileVersion { found, expected } => write!(
                f,
                "map file version {found} cannot be read by a build that writes version {expected}"
            ),
        }
    }
}

impl std::error::Error for MapError {}

/// The rules-table band a starting feature is placed in: how far from the core,
/// and at what grade.
///
/// One type rather than three parameters because the two placers below take the
/// same three rows and a caller that swapped `min` for `max` would produce a map
/// that is wrong and still generates.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Band {
    /// The nearest the feature may be, in whole voxels.
    pub min_voxels: i32,
    /// The furthest the feature may be, in whole voxels.
    pub max_voxels: i32,
    /// What grade to place it at.
    pub richness: Richness,
}

/// One placed vent or seam, as the report describes it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Feature {
    /// Its grade.
    pub richness: Richness,
    /// Where it is, in whole voxels; `z` is the **top solid voxel** of the
    /// column — the voxel the feature is made of, not the air above it.
    pub at: [i32; 3],
    /// How far from the zone's core it is, in whole voxels. Zero for a
    /// contested feature, which belongs to no core.
    pub from_core_voxels: i32,
    /// How many voxels the feature occupies: nine for a vent patch,
    /// `economy.seam_voxels` for a seam.
    pub voxels: u32,
}

/// One spawn zone, as the report describes it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ZoneReport {
    /// The seat that occupies it, or `None` when it is terrain only.
    pub seat: Option<u8>,
    /// The zone centre, in whole voxels; `z` is the **voxel a unit stands on**,
    /// one above the column's top solid voxel. (The other convention, the top
    /// solid voxel itself, is [`Feature::at`]'s: a core is something that
    /// stands on the map and a vent is something the map is made of.)
    pub centre: [i32; 3],
    /// The zone's heat vent, absent when the zone is unoccupied.
    pub vent: Option<Feature>,
    /// The zone's scrap seam, absent when the zone is unoccupied.
    pub seam: Option<Feature>,
}

/// What a generated map turned out to be. Printed by the tests, and the source
/// of the numbers in `tests/golden/mapgen/expected.digests.txt`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MapReport {
    /// The match seed the map was generated from.
    pub seed: u64,
    /// The map extent in voxels.
    pub size: [u32; 3],
    /// Every zone, in zone order.
    pub zones: Vec<ZoneReport>,
    /// The contested features, in placement order.
    pub contested: Vec<Feature>,
    /// The closest two **occupied** spawn centres, in octile ground cost.
    ///
    /// **Zero when fewer than two zones are occupied**, because there is no
    /// pair: a one-seat map has no separation, and the spawn-distance rule is
    /// vacuous rather than satisfied by an enormous number. The three fields
    /// below are zero with it.
    pub min_separation_cost: i64,
    /// What the spawn-distance rule asked for, in the same units. Zero when
    /// there is no occupied pair for it to ask about.
    pub required_separation_cost: i64,
    /// [`MapReport::min_separation_cost`] as seconds of raider walking,
    /// rounded up. Zero when there is no occupied pair.
    pub separation_raider_seconds: i64,
    /// [`MapReport::min_separation_cost`] as seconds of commander walking,
    /// rounded up. Zero when there is no occupied pair.
    pub separation_commander_seconds: i64,
    /// What the map would supply if every vent on it carried a Generator, plus
    /// every occupied core's surplus.
    pub total_supply_kw: i32,
    /// `power.map_ceiling_kw`.
    pub power_ceiling_kw: i32,
}

/// One unit of a seat's starting force, before it is given an id.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StartingUnit {
    /// Whose it is.
    pub seat: SeatId,
    /// What it is.
    pub kind: UnitKind,
    /// Where it stands, in Q16.16 voxels.
    pub at: [Fx; 3],
    /// What it is built with.
    pub hp: Hp,
}

/// Everything a generated map hands to [`crate::world::World::new`].
///
/// The unit **table** is not built here: the world knows how many rows it needs
/// for the harness walkers it adds on top, and a `SoA` table fixes its count at
/// construction (item 67).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GeneratedMap {
    /// The voxels.
    pub voxels: VoxelStore,
    /// The pre-placed core beacons, one per occupied zone, in zone order.
    pub beacons: BeaconTable,
    /// The starting force, in seat then kind order.
    pub starting_units: Vec<StartingUnit>,
    /// Per-seat starting treasury, indexed by seat.
    pub seat_treasury: Vec<Money>,
    /// Per-seat starting supply, indexed by seat.
    pub seat_supply: Vec<Kw>,
    /// Per-seat starting draw, indexed by seat.
    pub seat_draw: Vec<Kw>,
    /// What the map turned out to be.
    pub report: MapReport,
}

/// A map written to a file: the version, the seed it came from, and the store.
///
/// Fixed-width fields and postcard, like [`crate::snapshot::Snapshot`] and for
/// the same reasons. A map is a pure function of its seed, so this file is a
/// convenience for a tool that wants the voxels without the generator — never
/// the authority on what a seed means.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct MapFile {
    /// [`MAP_FILE_VERSION`] at the time of writing.
    pub version: u32,
    /// The match seed the map was generated from.
    pub seed: u64,
    /// The map extent in voxels.
    pub size: [u32; 3],
    /// Every chunk's 32 768 material bytes, in chunk-index order.
    pub chunks: Vec<u8>,
}

impl MapFile {
    /// Project a generated store into the flat form.
    #[must_use]
    pub fn capture(voxels: &VoxelStore, seed: u64) -> MapFile {
        let count = usize::try_from(voxels.chunk_count()).unwrap_or(0);
        let mut chunks: Vec<u8> =
            Vec::with_capacity(count.saturating_mul(crate::voxels::CHUNK_VOXELS));
        let mut chunk: u32 = 0;
        while chunk < voxels.chunk_count() {
            if let Some(bytes) = voxels.chunk_bytes(chunk) {
                chunks.extend_from_slice(bytes.as_slice());
            }
            chunk = chunk.saturating_add(1);
        }
        MapFile {
            version: MAP_FILE_VERSION,
            seed,
            size: voxels.size(),
            chunks,
        }
    }

    /// Encode to postcard bytes.
    ///
    /// # Errors
    ///
    /// Returns [`MapError::Codec`] when postcard refuses the value.
    pub fn to_bytes(&self) -> Result<Vec<u8>, MapError> {
        postcard::to_stdvec(self).map_err(|error| MapError::Codec(error.to_string()))
    }

    /// Decode from postcard bytes and check the version.
    ///
    /// # Errors
    ///
    /// Returns [`MapError::Codec`] when the bytes are not a map file, or
    /// [`MapError::FileVersion`] when they are one this build cannot read.
    pub fn from_bytes(bytes: &[u8]) -> Result<MapFile, MapError> {
        let file: MapFile =
            postcard::from_bytes(bytes).map_err(|error| MapError::Codec(error.to_string()))?;
        if file.version != MAP_FILE_VERSION {
            return Err(MapError::FileVersion {
                found: file.version,
                expected: MAP_FILE_VERSION,
            });
        }
        Ok(file)
    }

    /// Write the map to `path`, in binary, creating the directory.
    ///
    /// # Errors
    ///
    /// Returns [`MapError::Codec`] or [`MapError::Io`].
    pub fn write(&self, path: &std::path::Path) -> Result<(), MapError> {
        let bytes = self.to_bytes()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| MapError::Io {
                path: parent.display().to_string(),
                message: error.to_string(),
            })?;
        }
        std::fs::write(path, &bytes).map_err(|error| MapError::Io {
            path: path.display().to_string(),
            message: error.to_string(),
        })
    }

    /// Read a map file from `path`.
    ///
    /// # Errors
    ///
    /// Returns [`MapError::Io`], [`MapError::Codec`] or
    /// [`MapError::FileVersion`].
    pub fn read(path: &std::path::Path) -> Result<MapFile, MapError> {
        let bytes = std::fs::read(path).map_err(|error| MapError::Io {
            path: path.display().to_string(),
            message: error.to_string(),
        })?;
        MapFile::from_bytes(&bytes)
    }
}

// ---------------------------------------------------------------------------
// The generator
// ---------------------------------------------------------------------------

/// The rows the generator reads, pulled out of the message once.
struct Numbers<'a> {
    map: &'a gp::v1::rules_table::Map,
    beacon: &'a gp::v1::rules_table::Beacon,
    commander: &'a gp::v1::rules_table::Commander,
    units: &'a gp::v1::rules_table::Units,
    power: &'a gp::v1::rules_table::Power,
    economy: &'a gp::v1::rules_table::Economy,
    locomotion: &'a gp::v1::rules_table::Locomotion,
    first_segment_ms: i32,
}

/// Generate the map for `seed` under `rules`, with seats `0..seats` occupying
/// zones `0..seats`.
///
/// # Errors
///
/// Returns a [`MapError`] naming the rules-table row that made the map
/// impossible. Nothing here panics: a rules table that cannot describe a map is
/// a table to fix, not a crash.
pub fn generate(seed: u64, rules: &RulesTable, seats: u32) -> Result<GeneratedMap, MapError> {
    let numbers = Numbers::read(rules)?;
    let map = numbers.map;

    let size = [map.size_x, map.size_y, map.size_z];
    let mut voxels = VoxelStore::new(size).ok_or(MapError::BadExtent { size })?;
    let (width, depth) = (
        i32::try_from(map.size_x).unwrap_or(0),
        i32::try_from(map.size_y).unwrap_or(0),
    );
    let height_cap = i32::try_from(map.size_z).unwrap_or(0);
    if width <= 0 || depth <= 0 || height_cap <= 0 {
        return Err(MapError::BadExtent { size });
    }

    numbers.check_bands()?;

    // 1. Terrain, 2. zones, 3. flatten, 4. the 1-Lipschitz closure.
    let mut heights = terrain(seed, width, depth, height_cap);
    let radius = i32::try_from(map.spawn_zone_radius_voxels).unwrap_or(0);
    let centres = zone_centres(seed, width, depth, map.spawn_zones, radius)?;
    flatten_zones(&mut heights, width, depth, &centres, radius);
    smooth(&mut heights, width, depth);
    voxels.fill_from_heightmap(
        &heights,
        TERRAIN_SKIN_VOXELS,
        Material::DIRT,
        Material::STONE,
    );

    // 5. The spawn-distance rule, over the OCCUPIED centres only.
    //
    // `required_separation` is read even when there is no pair to apply it to,
    // because it is also a check on the rules table's own rows. What a map with
    // fewer than two occupied zones has is no *pair*: the rule is vacuous, and
    // the report says zero rather than an enormous number (see `MapReport`).
    let occupied = seats.min(map.spawn_zones);
    let required = numbers.required_separation()?;
    let pair = min_occupied_separation(&centres, occupied, &numbers);
    if let Some(found) = pair {
        if found < required {
            return Err(MapError::SpawnsTooClose { found, required });
        }
    }
    let min_separation = pair.unwrap_or(0);

    // 6. Every occupied zone's core, force, vent and seam.
    let mut placed = realise_zones(&mut voxels, seed, &numbers, &centres, occupied)?;

    // Seats beyond the zones the map carries are seated but unplaced: no core,
    // no force, no money, no power. The determinism harness runs four seats on
    // a three-zone map on purpose (a wider table is a wider hash), so this is a
    // state the sim has rather than an error.
    while u32::try_from(placed.seat_treasury.len()).unwrap_or(u32::MAX) < seats {
        placed.seat_treasury.push(Money::ZERO);
        placed.seat_supply.push(Kw::ZERO);
        placed.seat_draw.push(Kw::ZERO);
    }

    // 7. The contested features.
    let mut supply_kw = placed.supply_kw;
    let contested = place_contested(
        &mut voxels,
        seed,
        &numbers,
        &placed.zones,
        radius,
        [width, depth],
        &mut supply_kw,
    )?;

    // 8. The power ceiling.
    let ceiling = i32::try_from(numbers.power.map_ceiling_kw).unwrap_or(i32::MAX);
    if supply_kw > ceiling {
        return Err(MapError::PowerCeiling {
            supply_kw,
            ceiling_kw: ceiling,
        });
    }

    let (beacons, starting_units, seat_treasury, seat_supply, seat_draw, zones) = (
        placed.beacons,
        placed.starting_units,
        placed.seat_treasury,
        placed.seat_supply,
        placed.seat_draw,
        placed.zones,
    );

    let report = MapReport {
        seed,
        size,
        zones,
        contested,
        min_separation_cost: min_separation,
        required_separation_cost: pair.map_or(0, |_| required),
        separation_raider_seconds: div_ceil_i64(
            min_separation,
            i64::from(numbers.locomotion.raider_cost_per_second),
        ),
        separation_commander_seconds: div_ceil_i64(
            min_separation,
            i64::from(numbers.commander.cost_per_second),
        ),
        total_supply_kw: supply_kw,
        power_ceiling_kw: ceiling,
    };

    Ok(GeneratedMap {
        voxels,
        beacons,
        starting_units,
        seat_treasury,
        seat_supply,
        seat_draw,
        report,
    })
}

/// Everything the occupied zones put on the map.
#[derive(Clone, PartialEq, Eq, Debug)]
struct PlacedZones {
    beacons: BeaconTable,
    starting_units: Vec<StartingUnit>,
    seat_treasury: Vec<Money>,
    seat_supply: Vec<Kw>,
    seat_draw: Vec<Kw>,
    zones: Vec<ZoneReport>,
    supply_kw: i32,
}

/// Realise every zone: a core beacon, a starting force, a heat vent and a scrap
/// seam for the occupied ones, and terrain and nothing else for the rest.
fn realise_zones(
    voxels: &mut VoxelStore,
    seed: u64,
    numbers: &Numbers<'_>,
    centres: &[[i32; 3]],
    occupied: u32,
) -> Result<PlacedZones, MapError> {
    let map = numbers.map;
    let vent_richness =
        Richness::from_proto(map.start_vent_richness).ok_or(MapError::OutOfRange {
            field: "map.start_vent_richness",
            why: "an unset richness; the map generator will not guess a grade".to_owned(),
        })?;
    let seam_richness =
        Richness::from_proto(map.start_seam_richness).ok_or(MapError::OutOfRange {
            field: "map.start_seam_richness",
            why: "an unset richness; the map generator will not guess a grade".to_owned(),
        })?;
    let vent_band = Band {
        min_voxels: i32::try_from(map.vent_min_distance_voxels).unwrap_or(0),
        max_voxels: i32::try_from(map.vent_max_distance_voxels).unwrap_or(0),
        richness: vent_richness,
    };
    let seam_band = Band {
        min_voxels: i32::try_from(map.seam_min_distance_voxels).unwrap_or(0),
        max_voxels: i32::try_from(map.seam_max_distance_voxels).unwrap_or(0),
        richness: seam_richness,
    };
    let core_surplus = i32::try_from(numbers.power.core_surplus_kw).unwrap_or(0);

    let mut out = PlacedZones {
        beacons: BeaconTable::with_capacity(occupied),
        starting_units: Vec::new(),
        seat_treasury: Vec::new(),
        seat_supply: Vec::new(),
        seat_draw: Vec::new(),
        zones: Vec::with_capacity(centres.len()),
        supply_kw: 0,
    };

    for (index, centre) in centres.iter().enumerate() {
        let zone = u32::try_from(index).unwrap_or(u32::MAX);
        let column = (
            centre.first().copied().unwrap_or(0),
            centre.get(1).copied().unwrap_or(0),
        );
        let core = [column.0, column.1, voxels.standing_z(column.0, column.1)];
        if zone >= occupied {
            out.zones.push(ZoneReport {
                seat: None,
                centre: core,
                vent: None,
                seam: None,
            });
            continue;
        }

        let seat = SeatId::new(u8::try_from(zone).unwrap_or(u8::MAX));
        out.beacons.push(
            BeaconId::new(zone),
            seat,
            voxel_point(core),
            BeaconMandate {
                kind: MandateKind::Build,
                program_id: ProgramId::NONE,
            },
            Hp::new(i32::try_from(numbers.beacon.core_hp).unwrap_or(i32::MAX)),
            false,
        );
        numbers.push_starting_force(voxels, seat, core, &mut out.starting_units)?;
        out.seat_treasury.push(numbers.starting_treasury());
        out.seat_supply.push(Kw::new(core_surplus));
        out.seat_draw.push(numbers.starting_draw());
        out.supply_kw = out.supply_kw.saturating_add(core_surplus);

        let vent = place_vent(voxels, seed, zone, core, vent_band)?;
        let scrap = place_seam(
            voxels,
            seed,
            zone,
            core,
            seam_band,
            numbers.economy.seam_voxels,
        )?;
        out.supply_kw = out
            .supply_kw
            .saturating_add(numbers.generator_output(vent_richness));
        out.zones.push(ZoneReport {
            seat: Some(seat.raw()),
            centre: core,
            vent: Some(vent),
            seam: Some(scrap),
        });
    }
    Ok(out)
}

impl<'a> Numbers<'a> {
    fn read(rules: &'a RulesTable) -> Result<Numbers<'a>, MapError> {
        let message = rules.message();
        let matched = message
            .r#match
            .as_ref()
            .ok_or(MapError::MissingBlock("match"))?;
        let first_segment_ms = matched
            .segment_lengths_ms
            .first()
            .copied()
            .ok_or(MapError::MissingBlock("match.segment_lengths_ms"))?;
        Ok(Numbers {
            map: message.map.as_ref().ok_or(MapError::MissingBlock("map"))?,
            beacon: message
                .beacon
                .as_ref()
                .ok_or(MapError::MissingBlock("beacon"))?,
            commander: message
                .commander
                .as_ref()
                .ok_or(MapError::MissingBlock("commander"))?,
            units: message
                .units
                .as_ref()
                .ok_or(MapError::MissingBlock("units"))?,
            power: message
                .power
                .as_ref()
                .ok_or(MapError::MissingBlock("power"))?,
            economy: message
                .economy
                .as_ref()
                .ok_or(MapError::MissingBlock("economy"))?,
            locomotion: message
                .locomotion
                .as_ref()
                .ok_or(MapError::MissingBlock("locomotion"))?,
            first_segment_ms,
        })
    }

    /// The two band checks the generator makes before it places anything.
    fn check_bands(&self) -> Result<(), MapError> {
        let map = self.map;
        if map.vent_min_distance_voxels > map.vent_max_distance_voxels {
            return Err(MapError::OutOfRange {
                field: "map.vent_min_distance_voxels",
                why: "the vent band is inverted".to_owned(),
            });
        }
        if map.seam_min_distance_voxels > map.seam_max_distance_voxels {
            return Err(MapError::OutOfRange {
                field: "map.seam_min_distance_voxels",
                why: "the seam band is inverted".to_owned(),
            });
        }
        // Item 95: the reach is TWO beacon spheres — the commander walks to the
        // edge of the core's sphere and places a beacon whose own sphere
        // reaches as far again. Never the placement range, which is measured
        // from the commander to the site.
        let reach = self.beacon.sphere_radius_voxels.saturating_mul(2);
        if map.vent_max_distance_voxels > reach {
            return Err(MapError::OutOfRange {
                field: "map.vent_max_distance_voxels",
                why: format!(
                    "{} voxels is outside the {reach}-voxel reach of a beacon placed at the edge \
                     of the core's sphere (2 x beacon.sphere_radius_voxels; decisions log item 95)",
                    map.vent_max_distance_voxels
                ),
            });
        }
        // The seam is inside the core's own sphere, so the starting mining
        // drone digs from tick one (item 90).
        if map.seam_max_distance_voxels > self.beacon.sphere_radius_voxels {
            return Err(MapError::OutOfRange {
                field: "map.seam_max_distance_voxels",
                why: format!(
                    "{} voxels is outside the core's own {}-voxel sphere",
                    map.seam_max_distance_voxels, self.beacon.sphere_radius_voxels
                ),
            });
        }
        Ok(())
    }

    /// The spawn-distance rule in octile ground cost, rounded **up**:
    /// `raider_cost_per_second * numerator * first_segment_ms / (denominator * 1000)`.
    fn required_separation(&self) -> Result<i64, MapError> {
        let map = self.map;
        let denominator = i64::from(map.spawn_separation_pushes_denominator)
            .saturating_mul(1_000)
            .max(0);
        if denominator == 0 {
            return Err(MapError::OutOfRange {
                field: "map.spawn_separation_pushes_denominator",
                why: "zero; the spawn-distance rule is a fraction of a Push".to_owned(),
            });
        }
        let numerator = i64::from(self.locomotion.raider_cost_per_second)
            .saturating_mul(i64::from(map.spawn_separation_pushes_numerator))
            .saturating_mul(i64::from(self.first_segment_ms));
        Ok(div_ceil_i64(numerator, denominator))
    }

    /// The octile ground cost between two whole-voxel columns (item 59's
    /// 10 / 14 per step).
    fn octile(&self, a: [i32; 3], b: [i32; 3]) -> i64 {
        let dx = i64::from(a.first().copied().unwrap_or(0))
            .saturating_sub(i64::from(b.first().copied().unwrap_or(0)))
            .abs();
        let dy = i64::from(a.get(1).copied().unwrap_or(0))
            .saturating_sub(i64::from(b.get(1).copied().unwrap_or(0)))
            .abs();
        let (long, short) = if dx >= dy { (dx, dy) } else { (dy, dx) };
        i64::from(self.locomotion.step_cost_diagonal)
            .saturating_mul(short)
            .saturating_add(
                i64::from(self.locomotion.step_cost_cardinal)
                    .saturating_mul(long.saturating_sub(short)),
            )
    }

    fn starting_treasury(&self) -> Money {
        Money::new(
            i64::from(self.economy.bmi_dollars)
                .saturating_mul(i64::from(self.economy.starting_bmi_multiplier)),
        )
    }

    /// The starting force's own power draw.
    ///
    /// The commander has no `draw_kw` row of its own — item 90 gives one row
    /// for "1 kW per unit of every kind" — so the commander draws
    /// `power.kw_per_unit` and each drone draws its own kind's row. The core
    /// beacon draws nothing: item 90 says a beacon is net zero through its own
    /// key-core, and the pre-placed core is that core.
    fn starting_draw(&self) -> Kw {
        let build = self.units.build_drone.map_or(0, |k| k.draw_kw);
        let mine = self.units.mining_drone.map_or(0, |k| k.draw_kw);
        let total = u64::from(self.power.kw_per_unit)
            .saturating_add(
                u64::from(build).saturating_mul(u64::from(self.units.starting_build_drones)),
            )
            .saturating_add(
                u64::from(mine).saturating_mul(u64::from(self.units.starting_mining_drones)),
            );
        Kw::new(i32::try_from(total).unwrap_or(i32::MAX))
    }

    fn generator_output(&self, richness: Richness) -> i32 {
        let by = self.power.generator_output_kw.unwrap_or_default();
        let raw = match richness {
            Richness::Lean => by.lean,
            Richness::Standard => by.standard,
            Richness::Rich => by.rich,
        };
        i32::try_from(raw).unwrap_or(i32::MAX)
    }

    /// Place the commander and the starting drones around `core`.
    fn push_starting_force(
        &self,
        voxels: &VoxelStore,
        seat: SeatId,
        core: [i32; 3],
        out: &mut Vec<StartingUnit>,
    ) -> Result<(), MapError> {
        let commander_hp = Hp::new(i32::try_from(self.commander.hp).unwrap_or(i32::MAX));
        let build = self
            .units
            .build_drone
            .ok_or(MapError::MissingBlock("units.build_drone"))?;
        let mine = self
            .units
            .mining_drone
            .ok_or(MapError::MissingBlock("units.mining_drone"))?;

        let mut slot: u32 = 0;
        let mut place = |kind: UnitKind, hp: Hp, slot: &mut u32| {
            let offset = ring_offset(*slot);
            let x = core.first().copied().unwrap_or(0).saturating_add(offset[0]);
            let y = core.get(1).copied().unwrap_or(0).saturating_add(offset[1]);
            let z = voxels.standing_z(x, y);
            out.push(StartingUnit {
                seat,
                kind,
                at: voxel_point([x, y, z]),
                hp,
            });
            *slot = slot.saturating_add(1);
        };

        place(UnitKind::Commander, commander_hp, &mut slot);
        let mut n: u32 = 0;
        while n < self.units.starting_build_drones {
            place(
                UnitKind::BuildDrone,
                Hp::new(i32::try_from(build.hp).unwrap_or(i32::MAX)),
                &mut slot,
            );
            n = n.saturating_add(1);
        }
        let mut n: u32 = 0;
        while n < self.units.starting_mining_drones {
            place(
                UnitKind::MiningDrone,
                Hp::new(i32::try_from(mine.hp).unwrap_or(i32::MAX)),
                &mut slot,
            );
            n = n.saturating_add(1);
        }
        Ok(())
    }
}

/// The eight compass offsets a starting unit is placed on, in order, widening
/// by two voxels every eight units.
///
/// PLACEHOLDER: the two-voxel spacing baked into the offsets below is the
/// starting force's footprint, and nothing reads a unit's footprint yet.
/// Owner, at S1, with the first rules-table row that gives a unit a size.
const RING: [[i32; 2]; 8] = [
    [0, -2],
    [2, 0],
    [0, 2],
    [-2, 0],
    [2, -2],
    [2, 2],
    [-2, 2],
    [-2, -2],
];

#[allow(
    clippy::integer_division,
    reason = "the ring the slot falls on; both operands are non-negative, so the rounding is floor and that is the point"
)]
fn ring_offset(slot: u32) -> [i32; 2] {
    let ring = i32::try_from(slot / 8).unwrap_or(0).saturating_add(1);
    let index = usize::try_from(slot % 8).unwrap_or(0);
    let base = RING.get(index).copied().unwrap_or([0, 0]);
    [
        base.first().copied().unwrap_or(0).saturating_mul(ring),
        base.get(1).copied().unwrap_or(0).saturating_mul(ring),
    ]
}

/// Whole voxels to a Q16.16 point.
fn voxel_point(at: [i32; 3]) -> [Fx; 3] {
    let mut out = [Fx::ZERO; 3];
    for axis in 0..3 {
        let v = at.get(axis).copied().unwrap_or(0);
        if let (Some(slot), Ok(narrow)) = (out.get_mut(axis), i16::try_from(v)) {
            *slot = Fx::from_voxels(narrow);
        }
    }
    out
}

/// `ceil(a / b)` for non-negative `a` and positive `b`.
#[allow(
    clippy::integer_division,
    reason = "a ceiling division written out; item 59's cost-to-time rule is a ceiling and this is where it rounds"
)]
fn div_ceil_i64(a: i64, b: i64) -> i64 {
    if b <= 0 {
        return 0;
    }
    (a.saturating_add(b.saturating_sub(1))) / b
}

/// One draw from [`Stream::Map`], positioned at `(phase, index)`.
fn draw(seed: u64, phase: Phase, index: u32) -> StreamRng {
    StreamRng::new(seed, Stream::Map, phase.id(), 0, index)
}

// ---------------------------------------------------------------------------
// Terrain
// ---------------------------------------------------------------------------

/// One lattice corner's value, `0 ..= 255`.
fn corner(seed: u64, phase: Phase, lattice_width: u32, gx: u32, gy: u32) -> i32 {
    let index = gy.saturating_mul(lattice_width).saturating_add(gx);
    let bits = draw(seed, phase, index).next_u64();
    i32::try_from(bits >> 56).unwrap_or(0)
}

/// One octave of integer value noise at `(x, y)`, `0 ..= 255`.
///
/// Bilinear interpolation in integers: the two horizontal blends are exact and
/// the final `>> (2 * shift)` is a floor, which is the documented rounding —
/// both operands are non-negative, so it cannot surprise anybody.
fn octave(seed: u64, phase: Phase, shift: u32, lattice_width: u32, x: u32, y: u32) -> i32 {
    let cell = 1_i32 << shift;
    let gx = x >> shift;
    let gy = y >> shift;
    let fx = i32::try_from(x & ((1_u32 << shift) - 1)).unwrap_or(0);
    let fy = i32::try_from(y & ((1_u32 << shift) - 1)).unwrap_or(0);

    let v00 = corner(seed, phase, lattice_width, gx, gy);
    let v10 = corner(seed, phase, lattice_width, gx.saturating_add(1), gy);
    let v01 = corner(seed, phase, lattice_width, gx, gy.saturating_add(1));
    let v11 = corner(
        seed,
        phase,
        lattice_width,
        gx.saturating_add(1),
        gy.saturating_add(1),
    );

    let top = v00
        .saturating_mul(cell - fx)
        .saturating_add(v10.saturating_mul(fx));
    let bottom = v01
        .saturating_mul(cell - fx)
        .saturating_add(v11.saturating_mul(fx));
    let blended = top
        .saturating_mul(cell - fy)
        .saturating_add(bottom.saturating_mul(fy));
    blended >> (shift.saturating_mul(2))
}

/// The raw heightmap: how many solid voxels each column carries, row-major
/// (`height[y * width + x]`).
fn terrain(seed: u64, width: i32, depth: i32, height_cap: i32) -> Vec<i32> {
    let across = u32::try_from(width).unwrap_or(0);
    let down = u32::try_from(depth).unwrap_or(0);
    let coarse_width = (across >> TERRAIN_COARSE_SHIFT).saturating_add(2);
    let fine_width = (across >> TERRAIN_FINE_SHIFT).saturating_add(2);
    // The mean height is half the map's cap, which is what "a low-lying map"
    // means here: the sky has as much room as the rock.
    let base = height_cap >> 1;
    // PLACEHOLDER: the terrain's floor and its headroom under the map's cap, in
    // voxels. Four and eight, so no column is a bare floor and every column has
    // sky above it for a beacon sphere. Part of the same group as
    // `TERRAIN_COARSE_SHIFT`: the terrain's own shape, not a rules-table row.
    // Owner, at S4's symmetric map.
    let floor = 4;
    let ceiling = height_cap.saturating_sub(8).max(floor);

    let mut heights: Vec<i32> =
        Vec::with_capacity(usize::try_from(across.saturating_mul(down)).unwrap_or(0));
    let mut y: u32 = 0;
    while y < down {
        let mut x: u32 = 0;
        while x < across {
            let coarse = octave(
                seed,
                Phase::TerrainCoarse,
                TERRAIN_COARSE_SHIFT,
                coarse_width,
                x,
                y,
            );
            let fine = octave(
                seed,
                Phase::TerrainFine,
                TERRAIN_FINE_SHIFT,
                fine_width,
                x,
                y,
            );
            // `>> 7` is a floor toward negative infinity on a signed value,
            // which is this crate's documented shift rounding.
            let raw = base
                .saturating_add(
                    (coarse.saturating_sub(128)).saturating_mul(TERRAIN_COARSE_AMPLITUDE) >> 7,
                )
                .saturating_add(
                    (fine.saturating_sub(128)).saturating_mul(TERRAIN_FINE_AMPLITUDE) >> 7,
                );
            heights.push(raw.clamp(floor, ceiling));
            x = x.saturating_add(1);
        }
        y = y.saturating_add(1);
    }
    heights
}

/// Flatten each spawn zone's disc to the height at its centre.
fn flatten_zones(heights: &mut [i32], width: i32, depth: i32, centres: &[[i32; 3]], radius: i32) {
    let r2 = radius.saturating_mul(radius);
    for centre in centres {
        let cx = centre.first().copied().unwrap_or(0);
        let cy = centre.get(1).copied().unwrap_or(0);
        let Some(level) = height_at(heights, width, depth, cx, cy) else {
            continue;
        };
        let mut dy = -radius;
        while dy <= radius {
            let mut dx = -radius;
            while dx <= radius {
                if dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy)) <= r2 {
                    set_height(
                        heights,
                        width,
                        depth,
                        cx.saturating_add(dx),
                        cy.saturating_add(dy),
                        level,
                    );
                }
                dx = dx.saturating_add(1);
            }
            dy = dy.saturating_add(1);
        }
    }
}

/// Close the heightmap under "no cardinal neighbour differs by more than one
/// voxel", by lowering.
///
/// A forward raster scan and a backward one: the exact chamfer closure for the
/// 4-connected metric, `O(columns)` and allocation-free. What it buys is the
/// property the whole generator rests on — **every column is reachable from
/// every other by one-voxel steps**, so a vent placed anywhere is reachable
/// from its core, and the reachability test is a walk rather than a hope.
fn smooth(heights: &mut [i32], width: i32, depth: i32) {
    let mut y: i32 = 0;
    while y < depth {
        let mut x: i32 = 0;
        while x < width {
            let mut v = height_at(heights, width, depth, x, y).unwrap_or(0);
            if let Some(left) = height_at(heights, width, depth, x.saturating_sub(1), y) {
                v = v.min(left.saturating_add(1));
            }
            if let Some(down) = height_at(heights, width, depth, x, y.saturating_sub(1)) {
                v = v.min(down.saturating_add(1));
            }
            set_height(heights, width, depth, x, y, v);
            x = x.saturating_add(1);
        }
        y = y.saturating_add(1);
    }
    let mut y: i32 = depth.saturating_sub(1);
    while y >= 0 {
        let mut x: i32 = width.saturating_sub(1);
        while x >= 0 {
            let mut v = height_at(heights, width, depth, x, y).unwrap_or(0);
            if let Some(right) = height_at(heights, width, depth, x.saturating_add(1), y) {
                v = v.min(right.saturating_add(1));
            }
            if let Some(up) = height_at(heights, width, depth, x, y.saturating_add(1)) {
                v = v.min(up.saturating_add(1));
            }
            set_height(heights, width, depth, x, y, v);
            x = x.saturating_sub(1);
        }
        y = y.saturating_sub(1);
    }
}

fn height_index(width: i32, depth: i32, x: i32, y: i32) -> Option<usize> {
    if x < 0 || y < 0 || x >= width || y >= depth {
        return None;
    }
    usize::try_from(
        i64::from(y)
            .saturating_mul(i64::from(width))
            .saturating_add(i64::from(x)),
    )
    .ok()
}

fn height_at(heights: &[i32], width: i32, depth: i32, x: i32, y: i32) -> Option<i32> {
    heights.get(height_index(width, depth, x, y)?).copied()
}

fn set_height(heights: &mut [i32], width: i32, depth: i32, x: i32, y: i32, value: i32) {
    if let Some(index) = height_index(width, depth, x, y) {
        if let Some(slot) = heights.get_mut(index) {
            *slot = value;
        }
    }
}

// ---------------------------------------------------------------------------
// Zones
// ---------------------------------------------------------------------------

/// The spawn-zone centres, in zone order.
///
/// Exactly three zones are supported, and that is the whole of v1: the four
/// layouts below are the triangle with its apex north, south, east and west,
/// inset by the zone radius so no zone hangs off the map. One layout, one
/// rotation of which zone comes first and a `ZONE_JITTER_VOXELS` jitter per
/// zone are drawn from [`Phase::Layout`]; the separation the layouts give at the
/// committed table is 3 350 cost units against a requirement of 3 240. The
/// worst jitter takes 80 of that — [`StreamRng::range_i32`] is inclusive, so
/// each of the two zones can move a full `ZONE_JITTER_VOXELS` toward the other
/// along the axis that separates them, and eight cardinal voxels at
/// `locomotion.step_cost_cardinal` = 10 is 80 — leaving 30 cost units of slack.
/// The rule therefore holds by construction, and [`generate`] checks it anyway.
///
/// PLACEHOLDER: a free placement with a separation search, and the three-way
/// symmetry the spec asks for, belong to S4's symmetric map. Owner, at S4.
fn zone_centres(
    seed: u64,
    width: i32,
    depth: i32,
    zones: u32,
    radius: i32,
) -> Result<Vec<[i32; 3]>, MapError> {
    if zones != 3 {
        return Err(MapError::OutOfRange {
            field: "map.spawn_zones",
            why: format!(
                "{zones} zones; the skeleton's generator lays out exactly three (S4 owns the \
                 general case)"
            ),
        });
    }
    let lo_x = radius;
    let hi_x = width.saturating_sub(1).saturating_sub(radius);
    let lo_y = radius;
    let hi_y = depth.saturating_sub(1).saturating_sub(radius);
    let mid_x = width >> 1;
    let mid_y = depth >> 1;
    if lo_x >= hi_x || lo_y >= hi_y {
        return Err(MapError::OutOfRange {
            field: "map.spawn_zone_radius_voxels",
            why: "the zones do not fit on the map".to_owned(),
        });
    }

    let mut rng = draw(seed, Phase::Layout, 0);
    let layout = rng.range_i32(0, 3);
    let rotation = rng.range_i32(0, 2);
    let corners: [[i32; 2]; 3] = match layout {
        0 => [[lo_x, lo_y], [hi_x, lo_y], [mid_x, hi_y]],
        1 => [[lo_x, hi_y], [hi_x, hi_y], [mid_x, lo_y]],
        2 => [[lo_x, lo_y], [lo_x, hi_y], [hi_x, mid_y]],
        _ => [[hi_x, lo_y], [hi_x, hi_y], [lo_x, mid_y]],
    };

    let mut out: Vec<[i32; 3]> = Vec::with_capacity(3);
    let mut zone: u32 = 0;
    while zone < 3 {
        let pick =
            usize::try_from((i64::from(zone).saturating_add(i64::from(rotation))).rem_euclid(3))
                .unwrap_or(0);
        let base = corners.get(pick).copied().unwrap_or([mid_x, mid_y]);
        let mut jitter = draw(seed, Phase::Layout, zone.saturating_add(1));
        let dx = jitter.range_i32(-ZONE_JITTER_VOXELS, ZONE_JITTER_VOXELS);
        let dy = jitter.range_i32(-ZONE_JITTER_VOXELS, ZONE_JITTER_VOXELS);
        out.push([
            base.first()
                .copied()
                .unwrap_or(0)
                .saturating_add(dx)
                .clamp(lo_x, hi_x),
            base.get(1)
                .copied()
                .unwrap_or(0)
                .saturating_add(dy)
                .clamp(lo_y, hi_y),
            0,
        ]);
        zone = zone.saturating_add(1);
    }
    Ok(out)
}

/// The closest pair of **occupied** spawn centres, in octile ground cost, or
/// `None` when there is no pair.
///
/// `None` rather than [`i64::MAX`]: a saturating sentinel passes the rule *and*
/// divides into a nonsense travel time, and the travel times are two columns of
/// a committed golden. The caller decides what no pair means — [`generate`]
/// skips the check and reports zero.
fn min_occupied_separation(
    centres: &[[i32; 3]],
    occupied: u32,
    numbers: &Numbers<'_>,
) -> Option<i64> {
    let mut best: Option<i64> = None;
    let limit = usize::try_from(occupied).unwrap_or(0).min(centres.len());
    let mut a: usize = 0;
    while a < limit {
        let mut b = a.saturating_add(1);
        while b < limit {
            if let (Some(pa), Some(pb)) = (centres.get(a), centres.get(b)) {
                let cost = numbers.octile(*pa, *pb);
                best = Some(best.map_or(cost, |b: i64| b.min(cost)));
            }
            b = b.saturating_add(1);
        }
        a = a.saturating_add(1);
    }
    best
}

// ---------------------------------------------------------------------------
// Features
// ---------------------------------------------------------------------------

/// A point `distance` voxels from `from`, in a sector pointing at the map
/// centre, rounded to the nearest whole voxel.
///
/// Inward rather than in any direction for two reasons that agree: a vent
/// between the core and the contested middle is the walk the design wants, and
/// a spawn zone sits a zone radius from the map edge, so an outward offset of
/// up to `map.vent_max_distance_voxels` would fall off the map.
fn sector_point(rng: &mut StreamRng, from: [i32; 3], toward: [i32; 2], distance: i32) -> [i32; 2] {
    let dx = Fx::from_voxels(
        i16::try_from(
            toward
                .first()
                .copied()
                .unwrap_or(0)
                .saturating_sub(from.first().copied().unwrap_or(0)),
        )
        .unwrap_or(0),
    );
    let dy = Fx::from_voxels(
        i16::try_from(
            toward
                .get(1)
                .copied()
                .unwrap_or(0)
                .saturating_sub(from.get(1).copied().unwrap_or(0)),
        )
        .unwrap_or(0),
    );
    let base = Angle::from_delta(dx, dy);
    let spread = u16::try_from(rng.range_i32(0, i32::from(SECTOR_HALF_WIDTH).saturating_mul(2)))
        .unwrap_or(0);
    let angle = Angle::from_raw(
        base.raw()
            .wrapping_add(spread)
            .wrapping_sub(SECTOR_HALF_WIDTH),
    );

    let reach = Fx::from_voxels(i16::try_from(distance).unwrap_or(0));
    // Half a voxel, added before the floor, so the offset rounds to nearest and
    // the realised distance stays inside the band the rules table names.
    let half = Fx::from_raw(1 << 15);
    let ox = cos(angle)
        .saturating_mul_fx(reach)
        .saturating_add(half)
        .floor_voxels();
    let oy = sin(angle)
        .saturating_mul_fx(reach)
        .saturating_add(half)
        .floor_voxels();
    [
        from.first().copied().unwrap_or(0).saturating_add(ox),
        from.get(1).copied().unwrap_or(0).saturating_add(oy),
    ]
}

/// The squared whole-voxel distance between two columns.
fn columns_apart_squared(a: [i32; 3], b: [i32; 3]) -> i64 {
    let dx = i64::from(a.first().copied().unwrap_or(0))
        .saturating_sub(i64::from(b.first().copied().unwrap_or(0)));
    let dy = i64::from(a.get(1).copied().unwrap_or(0))
        .saturating_sub(i64::from(b.get(1).copied().unwrap_or(0)));
    dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
}

/// Replace the surface voxel of every column in a small patch with `material`.
fn stamp_surface_patch(
    voxels: &mut VoxelStore,
    at: [i32; 2],
    radius: i32,
    material: Material,
) -> u32 {
    let mut placed: u32 = 0;
    let mut dy = -radius;
    while dy <= radius {
        let mut dx = -radius;
        while dx <= radius {
            let x = at.first().copied().unwrap_or(0).saturating_add(dx);
            let y = at.get(1).copied().unwrap_or(0).saturating_add(dy);
            if let Some(z) = voxels.top_solid_z(x, y) {
                if voxels.set_pristine([x, y, z], material) {
                    placed = placed.saturating_add(1);
                }
            }
            dx = dx.saturating_add(1);
        }
        dy = dy.saturating_add(1);
    }
    placed
}

/// Place one heat vent in the band `min ..= max` voxels from `core`.
fn place_vent(
    voxels: &mut VoxelStore,
    seed: u64,
    index: u32,
    core: [i32; 3],
    band: Band,
) -> Result<Feature, MapError> {
    let spot = band_point(
        voxels,
        seed,
        Phase::StartVent,
        index,
        core,
        band,
        "a heat vent",
    )?;
    let apart2 = columns_apart_squared(spot, core);
    let placed = stamp_surface_patch(
        voxels,
        [
            spot.first().copied().unwrap_or(0),
            spot.get(1).copied().unwrap_or(0),
        ],
        VENT_PATCH_RADIUS,
        band.richness.vent(),
    );
    Ok(Feature {
        richness: band.richness,
        at: spot,
        from_core_voxels: whole_distance(apart2),
        voxels: placed,
    })
}

/// A surface column inside `band` of `core`, or the error that names what could
/// not be placed.
///
/// The distance is drawn with a voxel of margin at each end of the band, so the
/// round-to-nearest in [`sector_point`] cannot push the realised distance out of
/// it — and the realised distance is checked anyway, because "cannot" is a
/// claim and this is the place to make it true.
fn band_point(
    voxels: &VoxelStore,
    seed: u64,
    phase: Phase,
    index: u32,
    core: [i32; 3],
    band: Band,
    what: &'static str,
) -> Result<[i32; 3], MapError> {
    let size = voxels.size();
    let toward = [
        i32::try_from(size.first().copied().unwrap_or(0) >> 1).unwrap_or(0),
        i32::try_from(size.get(1).copied().unwrap_or(0) >> 1).unwrap_or(0),
    ];
    let mut rng = draw(seed, phase, index);
    let distance = rng.range_i32(
        band.min_voxels.saturating_add(1),
        band.max_voxels.saturating_sub(1).max(band.min_voxels),
    );
    let at = sector_point(&mut rng, core, toward, distance);
    let z = voxels
        .top_solid_z(
            at.first().copied().unwrap_or(0),
            at.get(1).copied().unwrap_or(0),
        )
        .ok_or(MapError::Unplaceable(what))?;
    let spot = [
        at.first().copied().unwrap_or(0),
        at.get(1).copied().unwrap_or(0),
        z,
    ];
    let apart2 = columns_apart_squared(spot, core);
    let lo = i64::from(band.min_voxels).saturating_mul(i64::from(band.min_voxels));
    let hi = i64::from(band.max_voxels).saturating_mul(i64::from(band.max_voxels));
    if apart2 < lo || apart2 > hi {
        return Err(MapError::Unplaceable(what));
    }
    Ok(spot)
}

/// The whole-voxel distance a squared distance rounds down to.
///
/// An integer square root by bisection: no `f64::sqrt`, which would round
/// differently on different targets (AGENTS.md section 4.2). Used only for the
/// report; every *decision* compares squared distances.
fn whole_distance(squared: i64) -> i32 {
    if squared <= 0 {
        return 0;
    }
    let mut lo: i64 = 0;
    let mut hi: i64 = 46_341; // floor(sqrt(i32::MAX)) + 1, so hi^2 stays in i64
    while lo < hi {
        let mid = lo.saturating_add(hi).saturating_add(1) >> 1;
        if mid.saturating_mul(mid) <= squared {
            lo = mid;
        } else {
            hi = mid.saturating_sub(1);
        }
    }
    i32::try_from(lo).unwrap_or(i32::MAX)
}

/// Every `(dx, dy)` within `radius`, ordered by squared distance then by `dy`
/// then by `dx` — a total order whose last key is unique, so the seam's shape
/// is the same on every machine.
fn disc_offsets(radius: i32) -> Vec<[i32; 2]> {
    let mut out: Vec<[i32; 2]> = Vec::new();
    let r2 = radius.saturating_mul(radius);
    let mut dy = -radius;
    while dy <= radius {
        let mut dx = -radius;
        while dx <= radius {
            if dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy)) <= r2 {
                out.push([dx, dy]);
            }
            dx = dx.saturating_add(1);
        }
        dy = dy.saturating_add(1);
    }
    // item 62: the key ends in `dx`, which is unique within a `dy` row, so the
    // order is total.
    out.sort_unstable_by_key(|o| {
        let dx = o.first().copied().unwrap_or(0);
        let dy = o.get(1).copied().unwrap_or(0);
        (
            dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy)),
            dy,
            dx,
        )
    });
    out
}

/// Lay `wanted` voxels of ore around `at`, **from the surface down**: the first
/// voxel written in a column is the column's own top solid voxel, so a seam is
/// visible from above, and the next [`SEAM_VOXELS_PER_COLUMN`] - 1 are the
/// voxels beneath it.
fn stamp_seam(voxels: &mut VoxelStore, at: [i32; 2], richness: Richness, wanted: u32) -> u32 {
    let ore = richness.ore();
    let mut placed: u32 = 0;
    for offset in disc_offsets(SEAM_DISC_RADIUS) {
        if placed >= wanted {
            break;
        }
        let x = at
            .first()
            .copied()
            .unwrap_or(0)
            .saturating_add(offset.first().copied().unwrap_or(0));
        let y = at
            .get(1)
            .copied()
            .unwrap_or(0)
            .saturating_add(offset.get(1).copied().unwrap_or(0));
        let Some(top) = voxels.top_solid_z(x, y) else {
            continue;
        };
        let mut k: i32 = 0;
        while k < SEAM_VOXELS_PER_COLUMN && placed < wanted {
            let z = top.saturating_sub(k);
            if z < 0 {
                break;
            }
            if voxels.set_pristine([x, y, z], ore) {
                placed = placed.saturating_add(1);
            }
            k = k.saturating_add(1);
        }
    }
    placed
}

/// Place one scrap seam in the band `min ..= max` voxels from `core`.
fn place_seam(
    voxels: &mut VoxelStore,
    seed: u64,
    index: u32,
    core: [i32; 3],
    band: Band,
    wanted: u32,
) -> Result<Feature, MapError> {
    let spot = band_point(
        voxels,
        seed,
        Phase::StartSeam,
        index,
        core,
        band,
        "a scrap seam",
    )?;
    let apart2 = columns_apart_squared(spot, core);
    let placed = stamp_seam(
        voxels,
        [
            spot.first().copied().unwrap_or(0),
            spot.get(1).copied().unwrap_or(0),
        ],
        band.richness,
        wanted,
    );
    if placed != wanted {
        return Err(MapError::Unplaceable("a scrap seam of the full size"));
    }
    Ok(Feature {
        richness: band.richness,
        at: spot,
        from_core_voxels: whole_distance(apart2),
        voxels: placed,
    })
}

/// The contested vents and seams toward the centre of the map.
///
/// Each is drawn inside the central half of the map and must clear every spawn
/// zone by one beacon sphere, every earlier contested feature by twice the seam
/// disc, and **every starting vent and seam by the two features' own radii**.
/// A placement that cannot be found in [`CONTESTED_ATTEMPTS`] tries is a rules
/// table this generator cannot satisfy, reported rather than looped over
/// forever.
///
/// The last of the three clearances is not decoration. A starting vent may sit
/// as far as `map.vent_max_distance_voxels` from its core, which is inside the
/// zone-plus-sphere ring the first clearance keeps clear, so without it a
/// contested seam's disc could overwrite a vent whose 20 kW had already been
/// added to the map's supply total — a power ceiling checked against a vent
/// that is no longer on the map.
fn place_contested(
    voxels: &mut VoxelStore,
    seed: u64,
    numbers: &Numbers<'_>,
    zones: &[ZoneReport],
    zone_radius: i32,
    extent: [i32; 2],
    supply_kw: &mut i32,
) -> Result<Vec<Feature>, MapError> {
    let width = extent.first().copied().unwrap_or(0);
    let depth = extent.get(1).copied().unwrap_or(0);
    let map = numbers.map;
    let sphere = i32::try_from(numbers.beacon.sphere_radius_voxels).unwrap_or(0);
    let clear = i64::from(zone_radius.saturating_add(sphere));
    let clear2 = clear.saturating_mul(clear);
    let apart = i64::from(SEAM_DISC_RADIUS.saturating_mul(4));
    let apart2 = apart.saturating_mul(apart);

    let lo_x = width >> 2;
    let hi_x = width.saturating_sub(width >> 2).saturating_sub(1);
    let lo_y = depth >> 2;
    let hi_y = depth.saturating_sub(depth >> 2).saturating_sub(1);

    // Vents first, then seams; standard before rich inside each. The order is
    // the order the ids are drawn in, so it is part of the generated map.
    let mut wanted: Vec<(Richness, bool)> = Vec::new();
    for _ in 0..map.contested_standard_vents {
        wanted.push((Richness::Standard, true));
    }
    for _ in 0..map.contested_rich_vents {
        wanted.push((Richness::Rich, true));
    }
    for _ in 0..map.contested_rich_seams {
        wanted.push((Richness::Rich, false));
    }

    // Every starting feature already on the map, with the radius its own stamp
    // covers: a vent is a `VENT_PATCH_RADIUS` patch and a seam a
    // `SEAM_DISC_RADIUS` disc.
    let mut starting: Vec<([i32; 3], i32)> = Vec::new();
    for zone in zones {
        if let Some(vent) = zone.vent {
            starting.push((vent.at, VENT_PATCH_RADIUS));
        }
        if let Some(seam) = zone.seam {
            starting.push((seam.at, SEAM_DISC_RADIUS));
        }
    }

    let mut placed: Vec<Feature> = Vec::with_capacity(wanted.len());
    for (index, (richness, is_vent)) in wanted.iter().enumerate() {
        let own_radius = if *is_vent {
            VENT_PATCH_RADIUS
        } else {
            SEAM_DISC_RADIUS
        };
        let item = u32::try_from(index).unwrap_or(u32::MAX);
        let mut attempt: u32 = 0;
        let mut site: Option<[i32; 3]> = None;
        while attempt < CONTESTED_ATTEMPTS {
            let mut rng = draw(
                seed,
                Phase::Contested,
                item.saturating_mul(CONTESTED_ATTEMPTS)
                    .saturating_add(attempt),
            );
            let x = rng.range_i32(lo_x, hi_x);
            let y = rng.range_i32(lo_y, hi_y);
            let Some(z) = voxels.top_solid_z(x, y) else {
                attempt = attempt.saturating_add(1);
                continue;
            };
            let candidate = [x, y, z];
            // Every zone centre, occupied or not: an unoccupied zone is still
            // a spawn a later match can use, and `ZoneReport::centre` carries
            // the same column the layout put there.
            let clears_zones = zones
                .iter()
                .all(|z| columns_apart_squared(candidate, z.centre) >= clear2);
            let clears_others = placed
                .iter()
                .all(|f| columns_apart_squared(candidate, f.at) >= apart2);
            let clears_starting = starting.iter().all(|(at, radius)| {
                let gap = i64::from(
                    own_radius
                        .saturating_add(*radius)
                        .saturating_add(STARTING_FEATURE_CLEARANCE),
                );
                columns_apart_squared(candidate, *at) >= gap.saturating_mul(gap)
            });
            if clears_zones && clears_others && clears_starting {
                site = Some(candidate);
                break;
            }
            attempt = attempt.saturating_add(1);
        }
        let Some(spot) = site else {
            return Err(MapError::Unplaceable("a contested feature"));
        };
        let at = [
            spot.first().copied().unwrap_or(0),
            spot.get(1).copied().unwrap_or(0),
        ];
        let voxels_placed = if *is_vent {
            *supply_kw = supply_kw.saturating_add(numbers.generator_output(*richness));
            stamp_surface_patch(voxels, at, VENT_PATCH_RADIUS, richness.vent())
        } else {
            let n = stamp_seam(voxels, at, *richness, numbers.economy.seam_voxels);
            if n != numbers.economy.seam_voxels {
                return Err(MapError::Unplaceable("a contested seam of the full size"));
            }
            n
        };
        placed.push(Feature {
            richness: *richness,
            at: spot,
            from_core_voxels: 0,
            voxels: voxels_placed,
        });
    }
    Ok(placed)
}
