// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Mine mandate's four settings and the seam a Mine beacon holds (spec
//! section 6's Mine row; decisions-log items 127 (9) and 128 (3) (e); S1's
//! plan, decision 6; register S1-38).
//!
//! # The four settings
//!
//! Spec section 6: "Seam choice: richest / nearest / safest; max dig depth;
//! pillar spacing; flee on threat. Never digs under structures." Decision 6
//! (item 128) gave them their meanings, and this module is where they act:
//!
//! * **`dig_max_depth`** counts voxels below the seam's **original** top
//!   surface: the top solid voxel of each footprint column when the generator
//!   stamped it ([`crate::features::FootprintColumn::top`]). `0` is the top
//!   layer only.
//! * **`pillar_spacing`** `N` leaves every `N`th column in x **and** in y
//!   undug, counted from the seam's anchor column, so the anchor column is
//!   always a pillar and `N = 1` leaves every column; `0` is no pillars. A dig
//!   pattern and not a collapse rule: there is no collapse before S2, so in
//!   S1 a non-zero spacing only costs yield, which decision 6 says out loud.
//! * **`seam_choice`**: which seam of the ones the beacon can reach. `NEAREST`
//!   is targeting's "nearest" (`docs/design/targeting.md`, "Nearest") measured
//!   from the beacon's anchor; `RICHEST` is the most **remaining** yield, ore
//!   voxels times the grade's `$` per voxel, among the reachable seams with
//!   work left for this beacon, ties by nearest; `SAFEST` is the nearest of
//!   them with no known threat whose weapon reaches its anchor (S2's
//!   decision 12, built by `dfnd`; [`crate::defend`]), and none when every
//!   one is threatened.
//! * **`flee_on_threat`** walks the beacon's mining drones home, dropping the
//!   dig in hand, while its seat knows of a threat inside the beacon's
//!   sphere (decision 12; [`crate::programs`]).
//!
//! And the rule that is not a setting: **never under structures**. A voxel in
//! or beside the column of any live structure or beacon, any seat's, is never
//! dug (decision 6: "in or beside the footprint column").
//!
//! # The held seam
//!
//! The choice is **held until the seam is spent** (targeting.md, "Three
//! reading rules", the named exception for a mandate's own `seam_choice`): a
//! beacon keeps the seam it chose while the seam has work left for it, and
//! chooses again only when it has none. **Spent** here means spent *for this
//! beacon*: no ore voxel is left that its settings allow and its drones can
//! safely take. That is narrower than targeting's "lost" ([`crate::features`]:
//! no ore left at all), which is unchanged: a seam worked to its depth limit,
//! or down to the ramps the pit-safe rule below leaves standing, still holds
//! ore and still reads as live on the map, but it reads as spent to the beacon
//! that worked it, so that beacon moves on and its Mine mandate stops asking
//! for drones.
//!
//! The held seam is a per-beacon column of the beacon table, hashed, in the
//! snapshot and in the goldens like every other order (AGENTS.md section 4.8).
//! It is chosen on the decision tick, inside the owning seat's decision
//! ([`refresh_seams`]), so the "nearest" estimates a choice runs are counted on
//! that seat's evaluation units (P1's counted half, S1's plan section 5), and
//! a switch of writ or a written `seam_choice` releases it.
//!
//! # Pit-safe digging
//!
//! The skeleton dug only a seam's **exposed rim**: a drone stood on undisturbed
//! dirt level with the ore, so a seam's top layer was taken from its edges and
//! nothing below it ever was (the demo's F2). S1 replaces that rule with a
//! pit-safe one, which is what lets a drone go below the rim at all:
//!
//! * A drone digs the **exposed** ore voxel of a column -- its top solid voxel
//!   -- and never stands on the column it is digging.
//! * It stands on a neighbouring column whose top is **level with the ore or
//!   one below it**, so after the dig the step between the stand and the dug
//!   column is still a legal one-voxel step (`locomotion.climb_surcharge`,
//!   item 59). A higher stand would leave a two-voxel wall behind it.
//! * The stand must be connected to the drone's home beacon on the pathing
//!   graph, so a drone never chooses a place it cannot walk home from.
//! * **And a dig never strands anything.** Over the seam's footprint and the
//!   ring of columns around it, every column that could walk out to that ring
//!   before the dig (by one-voxel cardinal steps) must still be able to after
//!   it. A dig that would cut the last staircase out of a pit is refused, so a
//!   seam is worked down in terraces and the ramps it needs stay in the ground.
//!   Without this rule a drone working the bottom of a pit dug its own way out
//!   from under itself and ended the segment parked as sealed in (item 60).
//!
//! # Allocation and cost
//!
//! Nothing here allocates: a seam's candidate voxels go through a fixed array
//! of [`crate::features::MAX_FOOTPRINT_COLUMNS`], and the flood behind the
//! pit-safe rule runs over a fixed grid of [`BOX_CELLS`] cells. A seam's
//! search reads its own footprint -- about forty columns at S1's seam shape --
//! rather than the whole sphere the skeleton swept.

use crate::features::{Feature, FeatureKind, MAX_FEATURES, MAX_FOOTPRINT_COLUMNS};
use crate::math::fixed::{Fx, Sq};
use crate::pathing::surface::Node;
use crate::seams::{MandateKind, Meter};
use crate::tables::{BeaconId, SeatId, UnitKind};
use crate::targeting::{NO_FEATURE, Ranker};
use crate::voxels::Material;
use crate::world::World;
use pharmakos_proto::gp;

/// Which seam a Mine beacon chooses (`gp.v1.MineSettings.SeamChoice`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum SeamChoice {
    /// The most remaining yield among the reachable seams with work left for
    /// the beacon, ties by nearest.
    Richest,
    /// Targeting's "nearest", measured from the beacon's anchor. What an
    /// omitted setting reads as (S1's plan, decision 11; the verifier's
    /// I0003 says so).
    #[default]
    Nearest,
    /// The nearest seam with no known threat whose weapon reaches its anchor
    /// (S2's decision 12).
    Safest,
}

impl SeamChoice {
    /// The wire id: `gp.v1.MineSettings.SeamChoice`'s own numbers, which are
    /// what the beacon table stores and hashes. Additive only.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            SeamChoice::Richest => 1,
            SeamChoice::Nearest => 2,
            SeamChoice::Safest => 3,
        }
    }

    /// The choice a wire id names, or `None` for one this build does not
    /// define (`0`, `SEAM_CHOICE_UNSPECIFIED`, included: an unset choice is
    /// not stored, it is read as the default where it is compiled).
    #[must_use]
    pub const fn from_id(id: u8) -> Option<SeamChoice> {
        match id {
            1 => Some(SeamChoice::Richest),
            2 => Some(SeamChoice::Nearest),
            3 => Some(SeamChoice::Safest),
            _ => None,
        }
    }

    /// The choice a `gp.v1` enum value names: `None` for
    /// `SEAM_CHOICE_UNSPECIFIED`, an error for a value the schema does not
    /// define.
    ///
    /// # Errors
    ///
    /// The value back, when `gp.v1.MineSettings.SeamChoice` has no such
    /// number.
    pub fn from_wire(value: i32) -> Result<Option<SeamChoice>, i32> {
        match gp::v1::mine_settings::SeamChoice::try_from(value) {
            Ok(gp::v1::mine_settings::SeamChoice::Unspecified) => Ok(None),
            Ok(gp::v1::mine_settings::SeamChoice::Richest) => Ok(Some(SeamChoice::Richest)),
            Ok(gp::v1::mine_settings::SeamChoice::Nearest) => Ok(Some(SeamChoice::Nearest)),
            Ok(gp::v1::mine_settings::SeamChoice::Safest) => Ok(Some(SeamChoice::Safest)),
            Err(_) => Err(value),
        }
    }
}

/// One beacon's Mine settings, as the beacon table holds them.
///
/// [`MineSettings::default`] is what a new beacon and a mandate switch start
/// from: depth 0 (the top layer only), no pillars, `NEAREST`, no flight --
/// spec section 6's settings at their proto3 zero, with the unset choice read
/// as `NEAREST` (S1's plan, decision 11).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MineSettings {
    /// Voxels below the seam's original top surface the beacon may dig.
    pub dig_max_depth: u32,
    /// Every `N`th column in x and y left undug; `0` is none.
    pub pillar_spacing: u32,
    /// Which seam the beacon chooses.
    pub seam_choice: SeamChoice,
    /// Walk the beacon's mining drones home while a known threat is inside
    /// its sphere (S2's decision 12).
    pub flee_on_threat: bool,
}

/// What a committed settings row writes into a beacon's Mine settings.
///
/// One `Option` per setting, because an **edit** writes only the settings it
/// names (`set_mandate_settings`: "changes settings within the current
/// mandate"), and proto3 cannot tell an omitted scalar from its zero: a zero
/// here is a setting the row is silent about, and is left alone. The one
/// exception is `flee_on_threat`, which carries presence (S3's plan, task
/// `con5b`), so a written `false` is an edit like any other. A mandate
/// switch and a new beacon start from [`MineSettings::default`] first, which
/// is how an omitted setting there reads as its default.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MineEdit {
    /// `dig_max_depth`, when the row writes a non-zero one.
    pub dig_max_depth: Option<u32>,
    /// `pillar_spacing`, when the row writes a non-zero one.
    pub pillar_spacing: Option<u32>,
    /// `seam_choice`, when the row writes one.
    pub seam_choice: Option<SeamChoice>,
    /// `flee_on_threat`, when the row writes it, `false` included.
    pub flee_on_threat: Option<bool>,
}

impl MineEdit {
    /// The edit a `gp.v1.MineSettings` message writes.
    ///
    /// # Errors
    ///
    /// The `seam_choice` value, when the schema does not define it.
    pub fn of(message: &gp::v1::MineSettings) -> Result<MineEdit, i32> {
        Ok(MineEdit {
            dig_max_depth: (message.dig_max_depth != 0).then_some(message.dig_max_depth),
            pillar_spacing: (message.pillar_spacing != 0).then_some(message.pillar_spacing),
            seam_choice: SeamChoice::from_wire(message.seam_choice)?,
            flee_on_threat: message.flee_on_threat,
        })
    }

    /// `settings` with this edit written over it.
    #[must_use]
    pub fn applied_to(self, settings: MineSettings) -> MineSettings {
        MineSettings {
            dig_max_depth: self.dig_max_depth.unwrap_or(settings.dig_max_depth),
            pillar_spacing: self.pillar_spacing.unwrap_or(settings.pillar_spacing),
            seam_choice: self.seam_choice.unwrap_or(settings.seam_choice),
            flee_on_threat: self.flee_on_threat.unwrap_or(settings.flee_on_threat),
        }
    }
}

/// One dig: the ore voxel a drone takes and where it stands to take it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Dig {
    /// The ore voxel, in whole voxels.
    pub ore: [i32; 3],
    /// The standing point beside it.
    pub stand: [Fx; 3],
}

/// The cells of the grid the pit-safe flood runs over: the seam's footprint
/// box and a ring of one column around it.
///
/// A ceiling rather than a tuning value, so the flood allocates nothing: S1's
/// seam is a disc of radius five, an eleven-column box and thirteen with its
/// ring, 169 cells. [`crate::features::FeatureTable::new`] refuses a seam whose
/// box would not fit, so no seam this build generates is ever refused here.
pub const BOX_CELLS: usize = 1024;

/// The eight columns around an ore column, in the order a stand is looked for:
/// east, north, west, south, then the four diagonals. A fixed order is what
/// makes the choice a total one.
const AROUND: [[i32; 2]; 8] = [
    [1, 0],
    [0, 1],
    [-1, 0],
    [0, -1],
    [1, 1],
    [-1, 1],
    [-1, -1],
    [1, -1],
];

/// The four cardinal steps the flood takes.
const CARDINAL: [[i32; 2]; 4] = [[1, 0], [0, 1], [-1, 0], [0, -1]];

/// The Mine settings of the beacon at `row`, or `None` when the row does not
/// exist: a beacon that is not there has no settings, and reading it as the
/// defaults would mine for it.
#[must_use]
pub fn settings_at(world: &World, row: usize) -> Option<MineSettings> {
    world.beacons().mine_settings(row)
}

/// The next dig for a drone homed to `beacon`: the nearest workable ore voxel
/// of the seam the beacon holds, and its stand. `None` when the beacon holds
/// no seam or the seam has no work left for it.
///
/// "Nearest" here is the skeleton's own order inside one seam: squared
/// distance from the beacon's anchor, then x, then y, then z, a total order
/// (item 62).
#[must_use]
pub fn next_dig(world: &World, beacon: BeaconId) -> Option<Dig> {
    let row = world.beacons().row_of(beacon).ok()?.index();
    let seam = world.beacons().seams().get(row).copied()?;
    if seam == NO_FEATURE {
        return None;
    }
    dig_in(
        world,
        row,
        usize::try_from(seam).ok()?,
        settings_at(world, row)?,
        &(),
    )
}

/// Whether a drone homed to `beacon` must wait a tick before it takes its
/// dig: an edit queued earlier this tick already reaches the box the pit-safe
/// flood runs over (the held seam's footprint box and its ring).
///
/// [`next_dig`] judges a dig against the world as the last voxel phase left
/// it, and the queue is applied after every program has run, so two digs in
/// one box on one tick would each be judged safe alone -- and between them
/// could cut the last ramp out of the pit, or take the same voxel twice. The
/// later one, in unit-id order, waits for the earlier to land.
#[must_use]
pub fn dig_waits(world: &World, beacon: BeaconId) -> bool {
    let Some(seam) = world
        .beacons()
        .row_of(beacon)
        .ok()
        .and_then(|row| world.beacons().seams().get(row.index()).copied())
        .filter(|seam| *seam != NO_FEATURE)
        .and_then(|seam| usize::try_from(seam).ok())
    else {
        return false;
    };
    let Some(feature) = world.features().get(seam) else {
        return false;
    };
    let [min_x, min_y, max_x, max_y] = feature.bounds;
    world.edit_queued_within([
        min_x.saturating_sub(1),
        min_y.saturating_sub(1),
        max_x.saturating_add(1),
        max_y.saturating_add(1),
    ])
}

/// How much ore the seam at `seam` still holds for the beacon at `row`, in
/// `$`: every ore voxel its settings allow it to take -- inside its sphere,
/// within its depth, off its pillars and clear of structures -- at the
/// seam's own yield per voxel. "Richest means richest remaining" (spec
/// section 6).
///
/// `None` when `seam` names no feature or `row` no beacon: a seam that is not
/// there holds nothing for a beacon that is not there, and saying 0 would
/// rank it.
#[must_use]
///
/// What it visits is reported to `meter` ([`crate::seams::Meter`];
/// decisions-log item 175): the rows the sphere's Spire test reads, one unit
/// a footprint column, the rows each column's structure test reads
/// (`blocked`), and one unit a voxel of a column's depth. A decision's
/// seam choice passes its tally; any other reader passes `()`.
pub fn remaining_yield<M: Meter + ?Sized>(
    world: &World,
    (row, seam): (usize, usize),
    settings: MineSettings,
    meter: &M,
) -> Option<i64> {
    let feature = world.features().get(seam)?;
    let centre = world.beacons().positions().get(row).copied()?;
    let limit = Sq::of_radius(Fx::from_voxels(crate::spire::metered::radius_at_row(
        world.rules(),
        world.beacons(),
        world.structures(),
        row,
        meter,
    )));
    let mut total: i64 = 0;
    let mut read: usize = 0;
    for column in &feature.footprint {
        read += 1;
        if pillar(feature, settings, column.x, column.y)
            || blocked(world, column.x, column.y, meter)
        {
            continue;
        }
        let deepest = i64::from(column.top).saturating_sub(i64::from(settings.dig_max_depth));
        let mut z = column.top;
        while i64::from(z) >= deepest && z >= 0 {
            read += 1;
            if let Some(dollars) = world.ore_yield_at([column.x, column.y, z])
                && point(column.x, column.y, z)
                    .is_some_and(|voxel| Sq::between(voxel, centre) <= limit)
            {
                total = total.saturating_add(dollars.raw());
            }
            z = z.saturating_sub(1);
        }
    }
    meter.rows(read);
    Some(total)
}

/// The next dig in the seam at `seam` for the beacon at `row`, under
/// `settings`.
///
/// What it visits is reported to `meter` ([`crate::seams::Meter`];
/// decisions-log item 175), one unit a visit of each loop: the rows the
/// sphere's Spire test reads, a footprint column, the rows each column's
/// structure test reads ([`blocked`]), a cell of the box the grid is read
/// from and a footprint column it marks ([`Grid::of`]), each flood's cells
/// and steps ([`Grid::flood`]), a candidate tried, a column its stand search
/// tries ([`stand_for`]), a voxel its dug top search reads ([`top_below`]),
/// and a cell the strand test compares ([`Grid::strands_nothing`]). The sort
/// of the candidates, at most [`MAX_FOOTPRINT_COLUMNS`] of them, is the
/// standard library's and is not counted. A drone's own dig, in the Programs
/// phase, passes `()`; a seat's decision, which keeps or chooses its
/// beacons' seams, passes its tally.
fn dig_in<M: Meter + ?Sized>(
    world: &World,
    row: usize,
    seam: usize,
    settings: MineSettings,
    meter: &M,
) -> Option<Dig> {
    let feature = world.features().get(seam)?;
    if feature.kind != FeatureKind::Seam {
        return None;
    }
    let centre = world.beacons().positions().get(row).copied()?;
    let radius = Fx::from_voxels(crate::spire::metered::radius_at_row(
        world.rules(),
        world.beacons(),
        world.structures(),
        row,
        meter,
    ));
    if !reaches(centre, radius, feature) {
        return None;
    }
    let [hx, hy] = crate::targeting::column_of(centre);
    let home = world.surface().node_of(hx, hy)?;
    // The cheap tests first, every footprint column at most once: exposed ore,
    // inside the sphere, within the depth, off the pillars, clear of
    // structures. What passes is sorted by the dig order.
    let limit = Sq::of_radius(radius);
    let mut keys = [(Sq::ZERO, 0_i32, 0_i32, 0_i32); MAX_FOOTPRINT_COLUMNS];
    let mut count: usize = 0;
    let mut read: usize = 0;
    for column in &feature.footprint {
        read += 1;
        let Some(node) = world.surface().node_of(column.x, column.y) else {
            continue;
        };
        let z = world.surface().top(node);
        if z < 0
            || world
                .voxels()
                .get([column.x, column.y, z])
                .and_then(Material::ore_richness)
                .is_none()
        {
            continue;
        }
        let depth = i64::from(column.top).saturating_sub(i64::from(z));
        if depth < 0 || depth > i64::from(settings.dig_max_depth) {
            continue;
        }
        if pillar(feature, settings, column.x, column.y)
            || blocked(world, column.x, column.y, meter)
        {
            continue;
        }
        // A voxel no coordinate holds is off every map, so in no sphere.
        let Some(voxel) = point(column.x, column.y, z) else {
            continue;
        };
        let distance = Sq::between(voxel, centre);
        if distance > limit {
            continue;
        }
        if let Some(slot) = keys.get_mut(count) {
            *slot = (distance, column.x, column.y, z);
            count = count.saturating_add(1);
        }
    }
    meter.rows(read);
    let candidates = keys.get_mut(..count)?;
    if candidates.is_empty() {
        return None;
    }
    // item 62: a column appears once, so `(x, y)` is unique and the key total.
    candidates.sort_unstable();
    let grid = Grid::of(world, feature, meter)?;
    let before = grid.flood(None, meter);
    for (_, x, y, z) in candidates.iter().copied() {
        meter.rows(1);
        let Some(stand) = stand_for(world, &grid, &before, home, [x, y, z], meter) else {
            continue;
        };
        let Some(cell) = grid.index(x, y) else {
            continue;
        };
        let after = grid.flood(
            Some(Dug {
                cell,
                top: top_below(world, [x, y, z], meter),
            }),
            meter,
        );
        // Mine never undermines (decisions-log item 144 (3)): a footing is
        // anchored ground that ignores every edit (the owner's ruling of
        // 2026-10-20), so no dig can drop a structure or a beacon, and the
        // dig needs no support search of its own.
        if grid.strands_nothing(&before, &after, meter) {
            return Some(Dig {
                ore: [x, y, z],
                stand,
            });
        }
    }
    None
}

/// Where a drone stands to dig `ore`: the first of the eight columns around
/// it, in [`AROUND`]'s order, that is walkable, has its top level with the ore
/// or one voxel below it, walks out of the seam's box (`before`) and is
/// connected to the home beacon's column. One unit to `meter` a column it
/// tries.
fn stand_for<M: Meter + ?Sized>(
    world: &World,
    grid: &Grid,
    before: &Reach,
    home: Node,
    ore: [i32; 3],
    meter: &M,
) -> Option<[Fx; 3]> {
    let [x, y, z] = ore;
    for [dx, dy] in AROUND {
        meter.rows(1);
        let nx = x.saturating_add(dx);
        let ny = y.saturating_add(dy);
        let Some(node) = world.surface().node_of(nx, ny) else {
            continue;
        };
        if !world.surface().walkable(node) {
            continue;
        }
        let top = world.surface().top(node);
        if top != z && top != z.saturating_sub(1) {
            continue;
        }
        if !grid.index(nx, ny).is_some_and(|cell| before.reached(cell)) {
            continue;
        }
        if !world.clusters().connected(world.surface(), node, home) {
            continue;
        }
        return Some(world.standing_point([nx, ny, top]));
    }
    None
}

/// Whether the column `(x, y)` is one of the seam's pillars under `settings`.
fn pillar(feature: &Feature, settings: MineSettings, x: i32, y: i32) -> bool {
    if settings.pillar_spacing == 0 {
        return false;
    }
    let spacing = i64::from(settings.pillar_spacing);
    let [ax, ay] = feature.anchor.map(i64::from);
    let on = |at: i32, anchor: i64| {
        i64::from(at)
            .saturating_sub(anchor)
            .checked_rem_euclid(spacing)
            == Some(0)
    };
    on(x, ax) && on(y, ay)
}

/// **Never under structures**: whether the column `(x, y)` is in or beside
/// (one column, diagonals included) the column of a live structure or a live
/// beacon of any seat. One unit to `meter` a structure row and a beacon row
/// it reads, up to the one that blocks the column.
fn blocked<M: Meter + ?Sized>(world: &World, x: i32, y: i32, meter: &M) -> bool {
    let near = |at: [Fx; 3]| {
        let [cx, cy] = crate::targeting::column_of(at);
        cx.saturating_sub(x).abs() <= 1 && cy.saturating_sub(y).abs() <= 1
    };
    let mut read: usize = 0;
    let structures = world.structures();
    let built = structures
        .positions()
        .iter()
        .zip(structures.hit_points())
        .inspect(|_| read += 1)
        .any(|(at, hp)| hp.is_alive() && near(*at));
    if built {
        meter.rows(read);
        return true;
    }
    let beacons = world.beacons();
    let by_beacon = beacons
        .positions()
        .iter()
        .zip(beacons.hit_points())
        .inspect(|_| read += 1)
        .any(|(at, hp)| hp.is_alive() && near(*at));
    meter.rows(read);
    by_beacon
}

/// Whether any footprint column of `feature` can lie inside the sphere around
/// `centre`, of radius `radius` (the beacon's own): a bounding-box test, so a
/// seam across the map costs one comparison.
fn reaches(centre: [Fx; 3], radius: Fx, feature: &Feature) -> bool {
    let reach = radius.floor_voxels().max(0);
    let [cx, cy] = crate::targeting::column_of(centre);
    let [min_x, min_y, max_x, max_y] = feature.bounds;
    cx.saturating_add(reach) >= min_x
        && cx.saturating_sub(reach) <= max_x
        && cy.saturating_add(reach) >= min_y
        && cy.saturating_sub(reach) <= max_y
}

/// A point at whole-voxel coordinates, or `None` for one no voxel coordinate
/// holds -- off every map, so in no sphere -- rather than a point at the
/// origin.
fn point(x: i32, y: i32, z: i32) -> Option<[Fx; 3]> {
    Some([
        Fx::from_voxels(i16::try_from(x).ok()?),
        Fx::from_voxels(i16::try_from(y).ok()?),
        Fx::from_voxels(i16::try_from(z).ok()?),
    ])
}

/// The seam's footprint box with a ring of one column around it: each
/// column's top and walkability, and whether it is a footprint column.
struct Grid {
    /// The box's lowest x and y.
    x0: i32,
    y0: i32,
    /// Its width and height, ring included.
    w: i32,
    h: i32,
    /// How many cells the box holds, `w * h`, at most [`BOX_CELLS`].
    cells: usize,
    /// Each cell's top solid z, or `-1`.
    top: [i32; BOX_CELLS],
    /// Whether each cell can be stood on.
    walk: [bool; BOX_CELLS],
    /// Whether each cell is one of the seam's footprint columns.
    seam: [bool; BOX_CELLS],
}

/// A dig under test, as the flood sees it: the dug cell and its top once its
/// exposed voxel is gone.
#[derive(Clone, Copy)]
struct Dug {
    cell: usize,
    top: i32,
}

/// Which cells of a [`Grid`] can walk out to its ring.
struct Reach {
    reached: [bool; BOX_CELLS],
}

impl Reach {
    fn reached(&self, cell: usize) -> bool {
        // E7: rule: a cell outside the box is not reached
        self.reached.get(cell).copied().unwrap_or(false)
    }
}

impl Grid {
    /// The grid around `feature`, or `None` when its box does not fit
    /// [`BOX_CELLS`] (which [`crate::features::FeatureTable::new`] rules out).
    /// One unit to `meter` a cell of the box it reads and a footprint column
    /// it marks.
    fn of<M: Meter + ?Sized>(world: &World, feature: &Feature, meter: &M) -> Option<Grid> {
        let [min_x, min_y, max_x, max_y] = feature.bounds;
        let x0 = min_x.checked_sub(1)?;
        let y0 = min_y.checked_sub(1)?;
        let w = max_x.checked_sub(x0)?.checked_add(2)?;
        let h = max_y.checked_sub(y0)?.checked_add(2)?;
        let cells = usize::try_from(w.checked_mul(h)?).ok()?;
        if cells > BOX_CELLS {
            return None;
        }
        let mut grid = Grid {
            x0,
            y0,
            w,
            h,
            cells,
            top: [-1; BOX_CELLS],
            walk: [false; BOX_CELLS],
            seam: [false; BOX_CELLS],
        };
        let mut read: usize = 0;
        let mut cy: i32 = 0;
        while cy < h {
            let mut cx: i32 = 0;
            while cx < w {
                read += 1;
                let x = x0.saturating_add(cx);
                let y = y0.saturating_add(cy);
                if let (Some(cell), Some(node)) = (grid.index(x, y), world.surface().node_of(x, y))
                {
                    if let Some(slot) = grid.top.get_mut(cell) {
                        *slot = world.surface().top(node);
                    }
                    if let Some(slot) = grid.walk.get_mut(cell) {
                        *slot = world.surface().walkable(node);
                    }
                }
                cx = cx.saturating_add(1);
            }
            cy = cy.saturating_add(1);
        }
        for column in &feature.footprint {
            read += 1;
            if let Some(cell) = grid.index(column.x, column.y)
                && let Some(slot) = grid.seam.get_mut(cell)
            {
                *slot = true;
            }
        }
        meter.rows(read);
        Some(grid)
    }

    /// The cell of the column `(x, y)`, or `None` outside the box.
    fn index(&self, x: i32, y: i32) -> Option<usize> {
        let cx = x.checked_sub(self.x0)?;
        let cy = y.checked_sub(self.y0)?;
        if cx < 0 || cy < 0 || cx >= self.w || cy >= self.h {
            return None;
        }
        usize::try_from(cy.checked_mul(self.w)?.checked_add(cx)?).ok()
    }

    /// A cell's top and walkability, with `dug`'s exposed voxel taken out;
    /// `None` for a cell past the box's arrays, which no walk reaches.
    fn column(&self, cell: usize, dug: Option<Dug>) -> Option<(i32, bool)> {
        let walk = self.walk.get(cell).copied()?;
        if let Some(dug) = dug
            && dug.cell == cell
        {
            return Some((dug.top, walk && dug.top >= 0));
        }
        Some((self.top.get(cell).copied()?, walk))
    }

    /// Every cell that can walk, by one-voxel cardinal steps inside the box,
    /// to a walkable cell that is not a footprint column -- the ring and any
    /// undisturbed ground inside the box -- with `dug`'s exposed voxel taken
    /// out when it is `Some`. One unit to `meter` a cell it seeds from, a
    /// cell it takes off its stack and a step it tries from it.
    fn flood<M: Meter + ?Sized>(&self, dug: Option<Dug>, meter: &M) -> Reach {
        let mut reach = Reach {
            reached: [false; BOX_CELLS],
        };
        let mut stack = [0_u16; BOX_CELLS];
        let mut depth: usize = 0;
        let mut cell: usize = 0;
        let mut read: usize = 0;
        while cell < self.cells {
            read += 1;
            let walk = self.column(cell, dug).is_some_and(|(_, walk)| walk);
            if walk
                // E7: rule: a cell outside the box holds no seam that could stop a walk
                && !self.seam.get(cell).copied().unwrap_or(true)
                && let Some(slot) = reach.reached.get_mut(cell)
                && let Some(top) = stack.get_mut(depth)
                && let Ok(id) = u16::try_from(cell)
            {
                *slot = true;
                *top = id;
                depth = depth.saturating_add(1);
            }
            cell = cell.saturating_add(1);
        }
        while depth > 0 {
            read += 1;
            depth = depth.saturating_sub(1);
            let Some(here) = stack.get(depth).copied().map(usize::from) else {
                break;
            };
            let Some((from, _)) = self.column(here, dug) else {
                continue;
            };
            let Ok(linear) = i32::try_from(here) else {
                continue;
            };
            // The width is at least the ring's two columns, so both are
            // `Some`; a cell that had neither would have no place to step from.
            let (Some(hx), Some(hy)) = (linear.checked_rem(self.w), linear.checked_div(self.w))
            else {
                continue;
            };
            for [dx, dy] in CARDINAL {
                read += 1;
                let Some(next) = self.index(
                    self.x0.saturating_add(hx).saturating_add(dx),
                    self.y0.saturating_add(hy).saturating_add(dy),
                ) else {
                    continue;
                };
                if reach.reached(next) {
                    continue;
                }
                let Some((to, walk)) = self.column(next, dug) else {
                    continue;
                };
                if !walk || to.saturating_sub(from).abs() > 1 {
                    continue;
                }
                if let Some(slot) = reach.reached.get_mut(next)
                    && let Some(top) = stack.get_mut(depth)
                    && let Ok(id) = u16::try_from(next)
                {
                    *slot = true;
                    *top = id;
                    depth = depth.saturating_add(1);
                }
            }
        }
        meter.rows(read);
        reach
    }

    /// Whether every cell that walked out before the dig still does after it.
    /// One unit to `meter` a cell it compares, up to the first stranded.
    fn strands_nothing<M: Meter + ?Sized>(&self, before: &Reach, after: &Reach, meter: &M) -> bool {
        let mut read: usize = 0;
        let kept = (0..self.cells).all(|cell| {
            read += 1;
            !before.reached(cell) || after.reached(cell)
        });
        meter.rows(read);
        kept
    }
}

/// The top solid z of the column `(x, y)` once its voxel at `top` is gone:
/// the first solid voxel below it, or `-1`. One unit to `meter` a voxel it
/// reads.
fn top_below<M: Meter + ?Sized>(world: &World, [x, y, top]: [i32; 3], meter: &M) -> i32 {
    let mut z = top.saturating_sub(1);
    let mut read: usize = 0;
    while z >= 0 {
        read += 1;
        if world.voxels().get([x, y, z]).is_some_and(|m| !m.is_air()) {
            meter.rows(read);
            return z;
        }
        z = z.saturating_sub(1);
    }
    meter.rows(read);
    -1
}

/// Whether the beacon at `row` mines: its writ is Mine, or a living mining
/// drone is homed to it (a unit's kind is its job, item 127 (6), so the
/// starting mining drone homed to a Build core keeps earning). One unit to
/// `meter` a unit row it reads, up to the drone.
fn mines<M: Meter + ?Sized>(world: &World, row: usize, beacon: BeaconId, meter: &M) -> bool {
    if world.beacons().mandates().get(row).copied() == Some(MandateKind::Mine.id()) {
        return true;
    }
    let units = world.units();
    let miner = UnitKind::MiningDrone.id();
    let mut read: usize = 0;
    let found = units
        .homes()
        .iter()
        .zip(units.kinds())
        .zip(units.hit_points())
        .inspect(|_| read += 1)
        .any(|((home, kind), hp)| *home == beacon && *kind == miner && hp.is_alive());
    meter.rows(read);
    found
}

/// Keep every mining beacon of `seat` holding a seam with work left for it.
///
/// Run once per decision tick, inside `seat`'s own decision, in ascending
/// beacon order: a beacon whose held seam still has work keeps it, and one
/// whose seam is spent -- or that holds none -- chooses again, which may find
/// nothing. Everything it visits is counted on the decision's units (one a
/// beacon row, the unit rows [`mines`] reads, and what the keeping
/// [`dig_in`] and the choice [`choose_seam`] visit).
pub(crate) fn refresh_seams(world: &mut World, seat: SeatId) {
    let count = world.beacons().ids().len();
    let mut row: usize = 0;
    while row < count {
        world.decision_meter().rows(1);
        let beacons = world.beacons();
        let owned = beacons.seats().get(row).copied() == Some(seat.raw());
        let alive = beacons
            .hit_points()
            .get(row)
            .is_some_and(|hp| hp.is_alive());
        let beacon = beacons.ids().get(row).copied().map(BeaconId::new);
        if owned
            && alive
            && let Some(beacon) = beacon
            && mines(world, row, beacon, world.decision_meter())
            && let Some(held) = world.beacons().seams().get(row).copied()
            && let Some(settings) = settings_at(world, row)
        {
            let keeps = held != NO_FEATURE
                && usize::try_from(held)
                    .ok()
                    .and_then(|seam| dig_in(world, row, seam, settings, world.decision_meter()))
                    .is_some();
            if !keeps {
                let chosen = choose_seam(world, row, settings);
                if let Some(slot) = world.beacons_mut().seams_mut().get_mut(row) {
                    *slot = chosen;
                }
            }
        }
        row = row.saturating_add(1);
    }
}

/// The seam the beacon at `row` chooses under `settings`, or [`NO_FEATURE`].
///
/// The candidates are the seams with work left for the beacon; "nearest" ranks
/// them by the estimator's travel from the beacon's anchor and skips the
/// unreachable (`crate::targeting::Ranker`). `NEAREST` and `SAFEST` take the
/// first; `RICHEST` takes the most remaining yield, ties to the nearer.
fn choose_seam(world: &mut World, row: usize, settings: MineSettings) -> u32 {
    let mut work = [false; MAX_FEATURES];
    let mut wealth = [0_i64; MAX_FEATURES];
    let features = world.features().len().min(MAX_FEATURES);
    let mut index: usize = 0;
    while index < features {
        // One unit a feature row asked, beside what its dig search visits.
        world.decision_meter().rows(1);
        if dig_in(world, row, index, settings, world.decision_meter()).is_some() {
            if let Some(slot) = work.get_mut(index) {
                *slot = true;
            }
            if settings.seam_choice == SeamChoice::Richest
                && let Some(slot) = wealth.get_mut(index)
                && let Some(left) =
                    remaining_yield(world, (row, index), settings, world.decision_meter())
            {
                *slot = left;
            }
        }
        index = index.saturating_add(1);
    }
    // No seam has work left for the beacon: nothing to rank, so nothing is
    // charged. A spent beacon asks again every decision, and charging a
    // ranking with no candidates would inflate the counted work for nothing.
    if !work.iter().any(|has| *has) {
        return NO_FEATURE;
    }
    let Some(origin) = world
        .beacons()
        .positions()
        .get(row)
        .copied()
        .map(crate::targeting::column_of)
    else {
        return NO_FEATURE;
    };
    // `SAFEST`'s known threats, read before the ranker borrows the world: each
    // armed enemy asset the beacon's seat sees, with its weapon's reach
    // (decision 12). Read only for a `SAFEST` choice, on the decision tick a
    // seam is chosen.
    let mut threats = world.take_threat_scratch();
    if settings.seam_choice == SeamChoice::Safest
        && let Some(seat) = world
            .beacons()
            .seats()
            .get(row)
            .copied()
            .map(crate::tables::SeatId::new)
    {
        // The scan is the seat's decision's work, one unit a row it reads.
        crate::defend::for_each_known_threat_metered(
            world,
            seat,
            world.decision_meter(),
            |known| {
                threats.push((known.at, Sq::of_radius(known.weapon.range())));
            },
        );
    }
    let (ground, scratch) = world.ground_and_scratch();
    let mut ranker = Ranker::new(&ground, origin, |index| {
        // E7: rule: an index the work table does not hold is no work
        work.get(index).copied().unwrap_or(false)
    });
    let mut best: Option<(usize, i64)> = None;
    while let Some((index, _)) = ranker.next(&ground, scratch) {
        // The ranker admits only indexes the work table marked, which are
        // the wealth table's too.
        let Some(yield_left) = wealth.get(index).copied() else {
            continue;
        };
        match settings.seam_choice {
            SeamChoice::Nearest => {
                best = Some((index, yield_left));
                break;
            }
            SeamChoice::Safest => {
                // The anchor's footprint test and one unit a threat read, up
                // to the one whose reach covers it.
                let mut read: usize = 0;
                let threatened =
                    crate::targeting::metered::anchor_point(&ground, index, ground.work)
                        .is_none_or(|anchor| {
                            threats.iter().any(|(at, reach)| {
                                read += 1;
                                Sq::between(*at, anchor) <= *reach
                            })
                        });
                ground.work.rows(read);
                if !threatened {
                    best = Some((index, yield_left));
                    break;
                }
            }
            // Nearest first, so a strictly greater yield is the only thing
            // that displaces the holder: ties stay with the nearer.
            SeamChoice::Richest => {
                if best.is_none_or(|(_, held)| yield_left > held) {
                    best = Some((index, yield_left));
                }
            }
        }
    }
    world.put_threat_scratch(threats);
    best.and_then(|(index, _)| u32::try_from(index).ok())
        // E7: rule: no seam found is `NO_FEATURE`; a feature index past `u32` names no feature a beacon can hold
        .unwrap_or(NO_FEATURE)
}

#[cfg(test)]
mod tests {
    //! S3's plan, task `con5b`: `flee_on_threat` has presence, so an edit row
    //! that writes `false` turns it off, and one that omits it leaves it.

    use super::{MineEdit, MineSettings};
    use pharmakos_proto::gp;

    fn fleeing() -> MineSettings {
        MineSettings {
            flee_on_threat: true,
            ..MineSettings::default()
        }
    }

    /// The edit, and what it leaves of a beacon that flees. The sim's source
    /// holds no `.expect(` (`tests/confinement.rs`), so the result is compared
    /// whole.
    fn edit(flee_on_threat: Option<bool>) -> Result<(Option<bool>, bool), i32> {
        MineEdit::of(&gp::v1::MineSettings {
            flee_on_threat,
            ..gp::v1::MineSettings::default()
        })
        .map(|edit| {
            (
                edit.flee_on_threat,
                edit.applied_to(fleeing()).flee_on_threat,
            )
        })
    }

    #[test]
    fn a_written_false_turns_flee_on_threat_off() {
        assert_eq!(edit(Some(false)), Ok((Some(false), false)));
    }

    #[test]
    fn an_omitted_flee_on_threat_leaves_the_setting_alone() {
        assert_eq!(edit(None), Ok((None, true)));
    }
}

#[cfg(test)]
mod visits {
    //! Dunit part 2: a seam's dig search, its yield and a decision's seam
    //! upkeep each charge one unit a visit of each loop they run, and the
    //! count equals the test's own count of those visits, made from the
    //! tables and from what each search answered, never from the meter.

    use super::{
        AROUND, Dug, Grid, MineSettings, Reach, dig_in, pillar, point, reaches, remaining_yield,
    };
    use crate::features::FeatureKind;
    use crate::math::fixed::{Fx, Sq};
    use crate::seams::UnitTally;
    use crate::tables::SeatId;
    use crate::voxels::Material;
    use crate::world::World;

    fn world() -> World {
        let rules = crate::rules::RulesTable::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("rules")
                .join("rules.v1.json"),
        )
        .unwrap_or_else(|error| panic!("the committed rules table loads: {error}"));
        World::new(&crate::world::WorldConfig {
            match_seed: crate::DETERMINISM_MATCH_SEED,
            seats: 2,
            rules,
            match_settings: crate::runner::MatchSettings::default(),
        })
        .unwrap_or_else(|error| panic!("the committed table describes a world: {error}"))
    }

    /// The units `run` charges on a fresh tally.
    fn charged(run: impl FnOnce(&UnitTally)) -> u64 {
        let tally = UnitTally::new();
        run(&tally);
        tally
            .read()
            .unwrap_or_else(|error| panic!("a measured count: {error}"))
    }

    /// The rows a radius of the beacon in `row` reads with no Spire on the
    /// map: every kind byte of the structure table.
    fn radius_rows(world: &World) -> u64 {
        assert!(
            !crate::spire::holds_a_spire(world.structures().kinds()),
            "the fixture holds no Spire"
        );
        u64::try_from(world.structures().kinds().len()).unwrap_or(u64::MAX)
    }

    /// [`super::blocked`]'s visits: structure rows up to the one beside the
    /// column, then, with none, beacon rows up to the one beside it.
    fn blocked_rows(world: &World, x: i32, y: i32) -> (bool, u64) {
        let near = |at: [Fx; 3]| {
            let [cx, cy] = crate::targeting::column_of(at);
            (cx - x).abs() <= 1 && (cy - y).abs() <= 1
        };
        let mut rows = 0;
        let structures = world.structures();
        for (at, hp) in structures.positions().iter().zip(structures.hit_points()) {
            rows += 1;
            if hp.is_alive() && near(*at) {
                return (true, rows);
            }
        }
        let beacons = world.beacons();
        for (at, hp) in beacons.positions().iter().zip(beacons.hit_points()) {
            rows += 1;
            if hp.is_alive() && near(*at) {
                return (true, rows);
            }
        }
        (false, rows)
    }

    /// A flood's visits, from what it answered: every cell it seeds from,
    /// and each cell it reached taken off the stack once with its four steps.
    fn flood_rows(grid: &Grid, reach: &Reach) -> u64 {
        let reached = reach
            .reached
            .iter()
            .take(grid.cells)
            .filter(|r| **r)
            .count();
        u64::try_from(grid.cells + 5 * reached).unwrap_or(u64::MAX)
    }

    /// The independent count of one [`dig_in`]: its answer is the sim's, read
    /// from the run with `()`, and every visit is counted here.
    fn dig_in_rows(world: &World, row: usize, seam: usize, settings: MineSettings) -> u64 {
        let Some(feature) = world.features().get(seam) else {
            return 0;
        };
        if feature.kind != FeatureKind::Seam {
            return 0;
        }
        let Some(centre) = world.beacons().positions().get(row).copied() else {
            return 0;
        };
        let mut rows = radius_rows(world);
        let radius = world.sphere_radius_at_row(row);
        if !reaches(centre, radius, feature) {
            return rows;
        }
        let [hx, hy] = crate::targeting::column_of(centre);
        let Some(home) = world.surface().node_of(hx, hy) else {
            return rows;
        };
        let (mut candidates, read) =
            candidate_rows(world, feature, (centre, Sq::of_radius(radius)), settings);
        rows += read;
        if candidates.is_empty() {
            return rows;
        }
        candidates.sort_unstable();
        let Some(grid) = Grid::of(world, feature, &()) else {
            return rows;
        };
        rows += u64::try_from(grid.cells + feature.footprint.len()).unwrap_or(u64::MAX);
        let before = grid.flood(None, &());
        rows += flood_rows(&grid, &before);
        for (_, x, y, z) in candidates {
            rows += 1;
            let (stand, tried) = stand_rows(world, (&grid, &before), home, [x, y, z]);
            rows += tried;
            let Some(cell) = grid.index(x, y).filter(|_| stand) else {
                continue;
            };
            let mut below = z - 1;
            loop {
                if below < 0 {
                    break;
                }
                rows += 1;
                if world
                    .voxels()
                    .get([x, y, below])
                    .is_some_and(|m| !m.is_air())
                {
                    break;
                }
                below -= 1;
            }
            let after = grid.flood(
                Some(Dug {
                    cell,
                    top: below.max(-1),
                }),
                &(),
            );
            rows += flood_rows(&grid, &after);
            let mut kept = true;
            for at in 0..grid.cells {
                rows += 1;
                if before.reached(at) && !after.reached(at) {
                    kept = false;
                    break;
                }
            }
            if kept {
                return rows;
            }
        }
        rows
    }

    /// The dig search's first loop, counted: a footprint column read, and
    /// the rows each structure test reads; and the ore columns that pass.
    fn candidate_rows(
        world: &World,
        feature: &crate::features::Feature,
        (centre, limit): ([Fx; 3], Sq),
        settings: MineSettings,
    ) -> (Vec<(Sq, i32, i32, i32)>, u64) {
        let mut rows = 0;
        let mut candidates: Vec<(Sq, i32, i32, i32)> = Vec::new();
        for column in &feature.footprint {
            rows += 1;
            let Some(node) = world.surface().node_of(column.x, column.y) else {
                continue;
            };
            let z = world.surface().top(node);
            if z < 0
                || world
                    .voxels()
                    .get([column.x, column.y, z])
                    .and_then(Material::ore_richness)
                    .is_none()
            {
                continue;
            }
            let depth = i64::from(column.top) - i64::from(z);
            if depth < 0 || depth > i64::from(settings.dig_max_depth) {
                continue;
            }
            if pillar(feature, settings, column.x, column.y) {
                continue;
            }
            let (blocked, read) = blocked_rows(world, column.x, column.y);
            rows += read;
            if blocked {
                continue;
            }
            let Some(voxel) = point(column.x, column.y, z) else {
                continue;
            };
            let distance = Sq::between(voxel, centre);
            if distance <= limit {
                candidates.push((distance, column.x, column.y, z));
            }
        }
        (candidates, rows)
    }

    /// [`super::stand_for`]'s visits: a column around the ore tried, up to
    /// the first a drone can stand on.
    fn stand_rows(
        world: &World,
        (grid, before): (&Grid, &Reach),
        home: crate::pathing::surface::Node,
        [x, y, z]: [i32; 3],
    ) -> (bool, u64) {
        let mut rows = 0;
        for [dx, dy] in AROUND {
            rows += 1;
            let (nx, ny) = (x + dx, y + dy);
            let Some(node) = world.surface().node_of(nx, ny) else {
                continue;
            };
            let top = world.surface().top(node);
            if world.surface().walkable(node)
                && (top == z || top == z - 1)
                && grid.index(nx, ny).is_some_and(|cell| before.reached(cell))
                && world.clusters().connected(world.surface(), node, home)
            {
                return (true, rows);
            }
        }
        (false, rows)
    }

    /// The core of seat 0: its row and its Mine settings.
    fn core(world: &World) -> (usize, MineSettings) {
        let row = world
            .beacons()
            .row_of_ordinal(SeatId::new(0), 0)
            .unwrap_or_else(|| panic!("seat 0 has a core"));
        let settings = super::settings_at(world, row).unwrap_or_else(|| panic!("its settings"));
        (row, settings)
    }

    #[test]
    fn a_dig_search_charges_what_it_visits() {
        dig_searches_charge_what_they_visit(&world());
        dig_searches_charge_what_they_visit(&with_structures());
    }

    /// The fixture with structure rows for [`super::blocked`] to read: two
    /// Generators of seat 1's on its own core's column, far from seat 0's
    /// seams.
    fn with_structures() -> World {
        let mut world = world();
        let row = world
            .beacons()
            .row_of_ordinal(SeatId::new(1), 0)
            .unwrap_or_else(|| panic!("seat 1 has a core"));
        let core = crate::tables::BeaconId::new(
            world
                .beacons()
                .ids()
                .get(row)
                .copied()
                .unwrap_or_else(|| panic!("its id")),
        );
        let at = world
            .beacons()
            .positions()
            .get(row)
            .copied()
            .unwrap_or_else(|| panic!("its place"));
        for _ in 0..2 {
            let _ = world
                .raise_structure(
                    SeatId::new(1),
                    core,
                    crate::tables::StructureKind::Generator,
                    at,
                )
                .unwrap_or_else(|| panic!("room for a structure"));
        }
        assert!(world.structures().ids().len() >= 2, "the table holds rows");
        world
    }

    fn dig_searches_charge_what_they_visit(world: &World) {
        let (row, settings) = core(world);
        let mut searched = 0;
        for seam in 0..world.features().len() {
            let units = charged(|meter| {
                let _ = dig_in(world, row, seam, settings, meter);
            });
            let counted = dig_in_rows(world, row, seam, settings);
            assert_eq!(units, counted, "seam {seam}: the visits, counted");
            if world
                .features()
                .get(seam)
                .is_some_and(|f| f.kind == FeatureKind::Seam)
                && dig_in(world, row, seam, settings, &()).is_some()
            {
                searched += 1;
            }
        }
        assert!(searched > 0, "the core has a seam to dig");
    }

    #[test]
    fn a_seams_yield_charges_what_it_reads() {
        yields_charge_what_they_read(&world());
        yields_charge_what_they_read(&with_structures());
    }

    fn yields_charge_what_they_read(world: &World) {
        let (row, settings) = core(world);
        for seam in 0..world.features().len() {
            let Some(feature) = world.features().get(seam) else {
                continue;
            };
            let units = charged(|meter| {
                let _ = remaining_yield(world, (row, seam), settings, meter);
            });
            let mut rows = radius_rows(world);
            for column in &feature.footprint {
                rows += 1;
                if pillar(feature, settings, column.x, column.y) {
                    continue;
                }
                let (blocked, read) = blocked_rows(world, column.x, column.y);
                rows += read;
                if blocked {
                    continue;
                }
                let deepest = i64::from(column.top) - i64::from(settings.dig_max_depth);
                let mut z = column.top;
                while i64::from(z) >= deepest && z >= 0 {
                    rows += 1;
                    z -= 1;
                }
            }
            assert_eq!(units, rows, "seam {seam}: the visits, counted");
        }
    }

    #[test]
    fn a_kept_seam_charges_its_dig_search() {
        let mut world = world();
        let seat = SeatId::new(0);
        // The first upkeep chooses; the second keeps what it chose.
        world.decision_meter().reset();
        super::refresh_seams(&mut world, seat);
        let (row, settings) = core(&world);
        let held = world
            .beacons()
            .seams()
            .get(row)
            .copied()
            .unwrap_or(super::NO_FEATURE);
        assert_ne!(held, super::NO_FEATURE, "the core chose a seam");
        world.decision_meter().reset();
        super::refresh_seams(&mut world, seat);
        let units = world
            .decision_meter()
            .read()
            .unwrap_or_else(|error| panic!("a measured count: {error}"));
        // Every beacon row; for each of seat 0's living beacons, the unit
        // rows its miner test reads (a Mine writ reads none); and the kept
        // seam's dig search.
        let mut rows = 0;
        let beacons = world.beacons();
        for at in 0..beacons.ids().len() {
            rows += 1;
            let owned = beacons.seats().get(at).copied() == Some(seat.raw());
            let alive = beacons.hit_points().get(at).is_some_and(|hp| hp.is_alive());
            if !owned || !alive {
                continue;
            }
            if beacons.mandates().get(at).copied() == Some(crate::seams::MandateKind::Mine.id()) {
                continue;
            }
            let id = beacons
                .ids()
                .get(at)
                .copied()
                .map(crate::tables::BeaconId::new);
            let units_table = world.units();
            for ((home, kind), hp) in units_table
                .homes()
                .iter()
                .zip(units_table.kinds())
                .zip(units_table.hit_points())
            {
                rows += 1;
                if Some(*home) == id
                    && *kind == crate::tables::UnitKind::MiningDrone.id()
                    && hp.is_alive()
                {
                    break;
                }
            }
        }
        let kept_seam = usize::try_from(held).unwrap_or(usize::MAX);
        rows += dig_in_rows(&world, row, kept_seam, settings);
        assert_eq!(units, rows, "the upkeep's visits, counted");
    }

    /// The upkeep's scan of `seat`'s beacons, counted: one unit a beacon row,
    /// and every unit row the core's miner test reads up to its drone.
    fn scan_rows(world: &World, seat: SeatId) -> u64 {
        let beacons = world.beacons();
        let units = world.units();
        let mut rows = u64::try_from(beacons.ids().len()).unwrap_or(u64::MAX);
        for at in 0..beacons.ids().len() {
            let owned = beacons.seats().get(at).copied() == Some(seat.raw());
            let alive = beacons.hit_points().get(at).is_some_and(|hp| hp.is_alive());
            let mine =
                beacons.mandates().get(at).copied() == Some(crate::seams::MandateKind::Mine.id());
            if !owned || !alive || mine {
                continue;
            }
            let id = beacons
                .ids()
                .get(at)
                .copied()
                .map(crate::tables::BeaconId::new);
            for ((home, kind), hp) in units
                .homes()
                .iter()
                .zip(units.kinds())
                .zip(units.hit_points())
            {
                rows += 1;
                if Some(*home) == id
                    && *kind == crate::tables::UnitKind::MiningDrone.id()
                    && hp.is_alive()
                {
                    break;
                }
            }
        }
        rows
    }

    /// The first upkeep of a seat whose core holds no seam chooses one: the
    /// scan, one unit a feature asked with each feature's dig search, and the
    /// ranking of the features with work, which `tests/nearest.rs`'s
    /// `decision_units_equal_the_primitive_steps_counted` counts exactly and
    /// which is measured here alone, on a fresh tally and a fresh scratch.
    #[test]
    fn a_seam_choice_charges_what_it_visits() {
        let mut world = world();
        let seat = SeatId::new(0);
        let (row, settings) = core(&world);
        assert_eq!(
            world.beacons().seams().get(row).copied(),
            Some(super::NO_FEATURE)
        );
        let features = world.features().len();
        let work: Vec<bool> = (0..features)
            .map(|seam| dig_in(&world, row, seam, settings, &()).is_some())
            .collect();
        let mut rows = scan_rows(&world, seat);
        for seam in 0..features {
            rows += 1 + dig_in_rows(&world, row, seam, settings);
        }
        let ranking = charged(|meter| {
            let ground = crate::targeting::Ground {
                work: meter,
                ..world.ground()
            };
            let origin = crate::targeting::column_of(
                world
                    .beacons()
                    .positions()
                    .get(row)
                    .copied()
                    .unwrap_or_else(|| panic!("the core's row")),
            );
            let mut scratch =
                crate::pathing::search::Scratch::for_map(world.surface(), world.clusters().size())
                    .unwrap_or_else(|| panic!("a scratch for the map"));
            let mut ranker = crate::targeting::Ranker::new(&ground, origin, |at| {
                work.get(at).copied().unwrap_or(false)
            });
            assert!(
                ranker.next(&ground, &mut scratch).is_some(),
                "a seam is reached"
            );
        });
        assert!(ranking > 0, "the ranking prices a walk");
        world.decision_meter().reset();
        super::refresh_seams(&mut world, seat);
        let units = world
            .decision_meter()
            .read()
            .unwrap_or_else(|error| panic!("a measured count: {error}"));
        assert_eq!(units, rows + ranking, "the choice's visits, counted");
    }

    /// A beacon none of whose seams has work left (every column a pillar)
    /// asks every feature's dig search on every decision and ranks nothing.
    #[test]
    fn a_spent_beacon_charges_its_searches_and_no_ranking() {
        let mut world = world();
        let seat = SeatId::new(0);
        let (row, _) = core(&world);
        let beacon = crate::tables::BeaconId::new(
            world
                .beacons()
                .ids()
                .get(row)
                .copied()
                .unwrap_or_else(|| panic!("the core's id")),
        );
        world.edit_mine_settings(
            beacon,
            super::MineEdit {
                pillar_spacing: Some(1),
                ..super::MineEdit::default()
            },
        );
        let (_, settings) = core(&world);
        let mut rows = scan_rows(&world, seat);
        for seam in 0..world.features().len() {
            assert!(
                dig_in(&world, row, seam, settings, &()).is_none(),
                "no work left"
            );
            rows += 1 + dig_in_rows(&world, row, seam, settings);
        }
        world.decision_meter().reset();
        super::refresh_seams(&mut world, seat);
        let units = world
            .decision_meter()
            .read()
            .unwrap_or_else(|error| panic!("a measured count: {error}"));
        assert_eq!(units, rows, "the searches, and no ranking");
    }
}
