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
//!   work left for this beacon, ties by nearest; `SAFEST` reads as `NEAREST`
//!   until S2's threat model, and the verifier warns that it does (W0501).
//! * **`flee_on_threat`** is stored and does nothing until S2, which brings
//!   the threats to flee from.
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
use crate::seams::MandateKind;
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
    /// Reads as [`SeamChoice::Nearest`] until S2's threat model (decision 6),
    /// and the verifier warns that it does.
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
    /// Stored; does nothing until S2.
    pub flee_on_threat: bool,
}

/// What a committed settings row writes into a beacon's Mine settings.
///
/// One `Option` per setting, because an **edit** writes only the settings it
/// names (`set_mandate_settings`: "changes settings within the current
/// mandate"), and proto3 cannot tell an omitted scalar from its zero: a zero
/// here is a setting the row is silent about, and is left alone. A mandate
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
    /// `flee_on_threat`, when the row writes `true`.
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
            flee_on_threat: message.flee_on_threat.then_some(true),
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

/// The Mine settings of the beacon at `row`, or the defaults when the row does
/// not exist.
#[must_use]
pub fn settings_at(world: &World, row: usize) -> MineSettings {
    world.beacons().mine_settings(row).unwrap_or_default()
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
    let row = usize::try_from(beacon.raw()).ok()?;
    let seam = world.beacons().seams().get(row).copied()?;
    if seam == NO_FEATURE {
        return None;
    }
    dig_in(
        world,
        row,
        usize::try_from(seam).ok()?,
        settings_at(world, row),
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
    let Some(seam) = usize::try_from(beacon.raw())
        .ok()
        .and_then(|row| world.beacons().seams().get(row).copied())
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
#[must_use]
pub fn remaining_yield(world: &World, row: usize, seam: usize, settings: MineSettings) -> i64 {
    let Some(feature) = world.features().get(seam) else {
        return 0;
    };
    let Some(centre) = world.beacons().positions().get(row).copied() else {
        return 0;
    };
    let limit = Sq::of_radius(world.sphere_radius());
    let mut total: i64 = 0;
    for column in &feature.footprint {
        if pillar(feature, settings, column.x, column.y) || blocked(world, column.x, column.y) {
            continue;
        }
        let deepest = i64::from(column.top).saturating_sub(i64::from(settings.dig_max_depth));
        let mut z = column.top;
        while i64::from(z) >= deepest && z >= 0 {
            if let Some(dollars) = world.ore_yield_at([column.x, column.y, z])
                && Sq::between(point(column.x, column.y, z), centre) <= limit
            {
                total = total.saturating_add(dollars.raw());
            }
            z = z.saturating_sub(1);
        }
    }
    total
}

/// The next dig in the seam at `seam` for the beacon at `row`, under
/// `settings`.
fn dig_in(world: &World, row: usize, seam: usize, settings: MineSettings) -> Option<Dig> {
    let feature = world.features().get(seam)?;
    if feature.kind != FeatureKind::Seam {
        return None;
    }
    let centre = world.beacons().positions().get(row).copied()?;
    if !reaches(world, centre, feature) {
        return None;
    }
    let [hx, hy] = crate::targeting::column_of(centre);
    let home = world.surface().node_of(hx, hy)?;
    // The cheap tests first, every footprint column at most once: exposed ore,
    // inside the sphere, within the depth, off the pillars, clear of
    // structures. What passes is sorted by the dig order.
    let limit = Sq::of_radius(world.sphere_radius());
    let mut keys = [(Sq::ZERO, 0_i32, 0_i32, 0_i32); MAX_FOOTPRINT_COLUMNS];
    let mut count: usize = 0;
    for column in &feature.footprint {
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
        if pillar(feature, settings, column.x, column.y) || blocked(world, column.x, column.y) {
            continue;
        }
        let distance = Sq::between(point(column.x, column.y, z), centre);
        if distance > limit {
            continue;
        }
        if let Some(slot) = keys.get_mut(count) {
            *slot = (distance, column.x, column.y, z);
            count = count.saturating_add(1);
        }
    }
    let candidates = keys.get_mut(..count)?;
    if candidates.is_empty() {
        return None;
    }
    // item 62: a column appears once, so `(x, y)` is unique and the key total.
    candidates.sort_unstable();
    let grid = Grid::of(world, feature)?;
    let before = grid.flood(None);
    for (_, x, y, z) in candidates.iter().copied() {
        let Some(stand) = stand_for(world, &grid, &before, home, [x, y, z]) else {
            continue;
        };
        let Some(cell) = grid.index(x, y) else {
            continue;
        };
        let after = grid.flood(Some(Dug {
            cell,
            top: top_below(world, x, y, z),
        }));
        if grid.strands_nothing(&before, &after) {
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
/// connected to the home beacon's column.
fn stand_for(
    world: &World,
    grid: &Grid,
    before: &Reach,
    home: Node,
    ore: [i32; 3],
) -> Option<[Fx; 3]> {
    let [x, y, z] = ore;
    for [dx, dy] in AROUND {
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
    let ax = i64::from(feature.anchor.first().copied().unwrap_or(0));
    let ay = i64::from(feature.anchor.get(1).copied().unwrap_or(0));
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
/// beacon of any seat.
fn blocked(world: &World, x: i32, y: i32) -> bool {
    let near = |at: [Fx; 3]| {
        let [cx, cy] = crate::targeting::column_of(at);
        cx.saturating_sub(x).abs() <= 1 && cy.saturating_sub(y).abs() <= 1
    };
    let structures = world.structures();
    let built = structures
        .positions()
        .iter()
        .zip(structures.hit_points())
        .any(|(at, hp)| hp.is_alive() && near(*at));
    if built {
        return true;
    }
    let beacons = world.beacons();
    beacons
        .positions()
        .iter()
        .zip(beacons.hit_points())
        .any(|(at, hp)| hp.is_alive() && near(*at))
}

/// Whether any footprint column of `feature` can lie inside the sphere around
/// `centre`: a bounding-box test, so a seam across the map costs one
/// comparison.
fn reaches(world: &World, centre: [Fx; 3], feature: &Feature) -> bool {
    let reach = world.sphere_radius().floor_voxels().max(0);
    let [cx, cy] = crate::targeting::column_of(centre);
    let [min_x, min_y, max_x, max_y] = feature.bounds;
    cx.saturating_add(reach) >= min_x
        && cx.saturating_sub(reach) <= max_x
        && cy.saturating_add(reach) >= min_y
        && cy.saturating_sub(reach) <= max_y
}

/// A point at whole-voxel coordinates.
fn point(x: i32, y: i32, z: i32) -> [Fx; 3] {
    [
        Fx::from_voxels(i16::try_from(x).unwrap_or(0)),
        Fx::from_voxels(i16::try_from(y).unwrap_or(0)),
        Fx::from_voxels(i16::try_from(z).unwrap_or(0)),
    ]
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
    /// Each cell's top solid z, or `-1`.
    top: [i16; BOX_CELLS],
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
        self.reached.get(cell).copied().unwrap_or(false)
    }
}

impl Grid {
    /// The grid around `feature`, or `None` when its box does not fit
    /// [`BOX_CELLS`] (which [`crate::features::FeatureTable::new`] rules out).
    fn of(world: &World, feature: &Feature) -> Option<Grid> {
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
            top: [-1; BOX_CELLS],
            walk: [false; BOX_CELLS],
            seam: [false; BOX_CELLS],
        };
        let mut cy: i32 = 0;
        while cy < h {
            let mut cx: i32 = 0;
            while cx < w {
                let x = x0.saturating_add(cx);
                let y = y0.saturating_add(cy);
                if let (Some(cell), Some(node)) = (grid.index(x, y), world.surface().node_of(x, y))
                {
                    let top = world.surface().top(node);
                    if let Some(slot) = grid.top.get_mut(cell) {
                        *slot = i16::try_from(top).unwrap_or(-1);
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
            if let Some(cell) = grid.index(column.x, column.y)
                && let Some(slot) = grid.seam.get_mut(cell)
            {
                *slot = true;
            }
        }
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

    /// A cell's top and walkability, with `dug`'s exposed voxel taken out.
    fn column(&self, cell: usize, dug: Option<Dug>) -> (i32, bool) {
        let walk = self.walk.get(cell).copied().unwrap_or(false);
        if let Some(dug) = dug
            && dug.cell == cell
        {
            return (dug.top, walk && dug.top >= 0);
        }
        (i32::from(self.top.get(cell).copied().unwrap_or(-1)), walk)
    }

    /// Every cell that can walk, by one-voxel cardinal steps inside the box,
    /// to a walkable cell that is not a footprint column -- the ring and any
    /// undisturbed ground inside the box -- with `dug`'s exposed voxel taken
    /// out when it is `Some`.
    fn flood(&self, dug: Option<Dug>) -> Reach {
        let mut reach = Reach {
            reached: [false; BOX_CELLS],
        };
        let mut stack = [0_u16; BOX_CELLS];
        let mut depth: usize = 0;
        let cells = usize::try_from(self.w.saturating_mul(self.h)).unwrap_or(0);
        let mut cell: usize = 0;
        while cell < cells {
            let (_, walk) = self.column(cell, dug);
            if walk
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
            depth = depth.saturating_sub(1);
            let Some(here) = stack.get(depth).copied().map(usize::from) else {
                break;
            };
            let (from, _) = self.column(here, dug);
            let Ok(linear) = i32::try_from(here) else {
                continue;
            };
            let hx = linear.checked_rem(self.w).unwrap_or(0);
            let hy = linear.checked_div(self.w).unwrap_or(0);
            for [dx, dy] in CARDINAL {
                let Some(next) = self.index(
                    self.x0.saturating_add(hx).saturating_add(dx),
                    self.y0.saturating_add(hy).saturating_add(dy),
                ) else {
                    continue;
                };
                if reach.reached(next) {
                    continue;
                }
                let (to, walk) = self.column(next, dug);
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
        reach
    }

    /// Whether every cell that walked out before the dig still does after it.
    fn strands_nothing(&self, before: &Reach, after: &Reach) -> bool {
        let cells = usize::try_from(self.w.saturating_mul(self.h)).unwrap_or(0);
        (0..cells).all(|cell| !before.reached(cell) || after.reached(cell))
    }
}

/// The top solid z of the column `(x, y)` once its voxel at `top` is gone:
/// the first solid voxel below it, or `-1`.
fn top_below(world: &World, x: i32, y: i32, top: i32) -> i32 {
    let mut z = top.saturating_sub(1);
    while z >= 0 {
        if world.voxels().get([x, y, z]).is_some_and(|m| !m.is_air()) {
            return z;
        }
        z = z.saturating_sub(1);
    }
    -1
}

/// Whether the beacon at `row` mines: its writ is Mine, or a living mining
/// drone is homed to it (a unit's kind is its job, item 127 (6), so the
/// starting mining drone homed to a Build core keeps earning).
fn mines(world: &World, row: usize, beacon: BeaconId) -> bool {
    if world.beacons().mandates().get(row).copied() == Some(MandateKind::Mine.id()) {
        return true;
    }
    let units = world.units();
    let miner = UnitKind::MiningDrone.id();
    units
        .homes()
        .iter()
        .zip(units.kinds())
        .zip(units.hit_points())
        .any(|((home, kind), hp)| *home == beacon.raw() && *kind == miner && hp.is_alive())
}

/// Keep every mining beacon of `seat` holding a seam with work left for it.
///
/// Run once per decision tick, inside `seat`'s own decision, in ascending
/// beacon order: a beacon whose held seam still has work keeps it, and one
/// whose seam is spent -- or that holds none -- chooses again, which may find
/// nothing. The "nearest" estimates a choice runs are counted on the decision's
/// evaluation units.
pub(crate) fn refresh_seams(world: &mut World, seat: SeatId) {
    let count = usize::try_from(world.beacons().len()).unwrap_or(0);
    let mut row: usize = 0;
    while row < count {
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
            && mines(world, row, beacon)
        {
            let held = world
                .beacons()
                .seams()
                .get(row)
                .copied()
                .unwrap_or(NO_FEATURE);
            let settings = settings_at(world, row);
            let keeps = held != NO_FEATURE
                && usize::try_from(held)
                    .ok()
                    .and_then(|seam| dig_in(world, row, seam, settings))
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
        if dig_in(world, row, index, settings).is_some() {
            if let Some(slot) = work.get_mut(index) {
                *slot = true;
            }
            if settings.seam_choice == SeamChoice::Richest
                && let Some(slot) = wealth.get_mut(index)
            {
                *slot = remaining_yield(world, row, index, settings);
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
    let (ground, scratch) = world.ground_and_scratch();
    let mut ranker = Ranker::new(&ground, origin, |index| {
        work.get(index).copied().unwrap_or(false)
    });
    let mut best: Option<(usize, i64)> = None;
    while let Some((index, _)) = ranker.next(&ground, scratch) {
        let yield_left = wealth.get(index).copied().unwrap_or(0);
        match settings.seam_choice {
            SeamChoice::Nearest | SeamChoice::Safest => {
                best = Some((index, yield_left));
                break;
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
    best.and_then(|(index, _)| u32::try_from(index).ok())
        .unwrap_or(NO_FEATURE)
}
