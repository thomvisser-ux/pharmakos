// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Attack mandate: raiders gathered, launched, hunting, breaching and
//! coming home (spec section 6's Attack row, section 4's hunting and section
//! 9's breach; S2's plan, task `atk`, decisions 9, 10, 12, 14, 40 and 41,
//! decisions-log items 13, 104, 148 and the owner's answer of 2026-10-26).
//!
//! # One beacon, one force, one row
//!
//! An Attack beacon's fabricator fields **raiders on the work in hand**: while
//! the force gathers, as many as its launch's `force_size`, or one when only
//! the segment time is written, and none while the beacon has no target
//! (decision 13's reading, applied to Attack as `dfnd` applied it to Defend).
//! Every tick, before the programs run, each Attack beacon's row is brought up
//! to date ([`update`]), and its raiders read that one answer:
//!
//! * **Gathering.** The raiders walk to the staging point (the beacon itself
//!   when none is written) and stand there. The force **launches** as soon as
//!   any condition its `launch` writes holds: `force_size` raiders standing at
//!   the staging point, or `segment_ms` of the segment gone with at least one
//!   raider fielded; at most `waves` launches a segment (the first cut of
//!   section 4.5 was not taken, so `waves` is read). The broadcast go-code is
//!   S4's and is reserved in the schema.
//! * **The target is pinned at the seal** (item 13): an enemy beacon named by
//!   its `e_NN` handle is pinned to the beacon the handle named when the
//!   playbook was sealed ([`crate::interpreter::Plan::pin_foreign`]) and
//!   launch never re-tests it; a voxel is a voxel. **The enemy commander**,
//!   unset, is the nearest enemy commander the seat has a sighting of inside
//!   `verifier.reach_memory_ms` at launch -- nearest the staging point, ties to
//!   the lowest seat -- or, with `seat` set, that seat's commander only (the
//!   owner's answer of 2026-10-26). It is read from the seat's own sighting
//!   table, never from ground truth. With no such sighting the force does not
//!   launch. **The nearest known enemy beacon** (`nearest` with the filter
//!   `side: ENEMY_KNOWN`, S2's `atk2`, decisions-log item 150 (2)) is chosen
//!   the same way: at launch, the nearest enemy beacon the seat holds a fresh
//!   sighting of and does not see gone, nearest the staging point, ties to the
//!   lowest id. With none the force does not launch either, and each such
//!   decision is counted (`launch_without_known_beacon`), never sent to a
//!   default.
//! * **Launched.** The raiders walk to the target and fight it. A beacon
//!   target is attacked at its **weak point** (decision 14): the target
//!   beacon's lowest-HP finished structure inside its sphere that the seat
//!   sees, ties nearest the beacon then lowest id, else the beacon. A hunted
//!   commander is **followed live** while any of the seat's sensors sees it
//!   (spec section 4, the same reach as plan time), and otherwise sought at
//!   the seat's last sighting of it; once that sighting is older than the
//!   reach window the commander is lost. A target that moved or died is
//!   decided **on arrival** (item 13): `NEAREST_KNOWN` retargets the nearest
//!   enemy beacon the seat holds a fresh sighting of, nearest where the lost
//!   target was, ties to the lowest id; `NONE` ends the attack. Raiding a
//!   voxel, a raider stops to fire at any enemy the seat sees in its reach,
//!   and the attack ends once the force stands at the voxel with none left.
//! * **Retreat** (`retreat`): fewer than `force_below_pct` of the launch force
//!   alive, or `after_launch_ms` since the launch, ends the attack, as does a
//!   force wiped out.
//! * **After-action**: `REGROUP_AT_STAGING` gathers again at once;
//!   `RETURN_HOME` walks home first and gathers once every raider is back.
//!
//! # The rules of engagement
//!
//! An unset `roe` on an Attack beacon reads as `FREE_FIRE` (item 148 (2);
//! [`crate::world::Roe::of_setting`]). Under an explicit `RETURN_FIRE` the
//! raiders start no fight: the force gathers and never launches, and answers
//! only an attacker (item 148 (2): "a raider on it never starts a fight or
//! raids"); `HOLD_FIRE` never fires and so never launches either.
//!
//! # Breach (E23, decisions 9, 40 and 41)
//!
//! A launched force whose way to its target is sealed -- the connectivity
//! oracle says the lead raider's column and the target's are apart -- stops,
//! unless the beacon's `breach_allowed` is set. With it, the beacon asks the
//! **breach search** ([`breach_search`], the pathfinder's second cost mode
//! over the same 2.5D graph) for the cheapest way through, on which a column
//! whose top is another seat's or a neutral wall is passable at
//! [`BREACH_COLUMN_COST`]; the search's first wall column is the breach point
//! and the column before it the plant point. Then:
//!
//! * the seat's mortars in range of the breach point, under `FREE_FIRE`,
//!   shell its top wall voxel ([`breach_aim`]; decision 41: an enemy wall is a
//!   target for a breach only);
//! * the beacon buys a **demolition charge** at
//!   `structures.demolition_charge_cost_dollars` (a Quartermaster request in
//!   the units band) and loads it on one raider of the force, the lowest row;
//!   the carrier walks to the plant point and plants it, and the charge
//!   detonates `combat.charge_fuse_ms` later ([`Charge`]), while the rest of
//!   the force walks on as far as the seal lets it. A carrier killed before
//!   it plants drops a charge that never fires (decision 9), and the beacon
//!   buys another;
//! * the charge **cuts flat**: every voxel inside `combat.charge_radius_voxels`
//!   of the breach column, above the wall's footing, wall or terrain, cracked
//!   or whole, stone included, is removed (decisions 10 and 40;
//!   [`crate::voxels::VoxelStore::cut`]), with the carrier's seat on the edit.
//!   The cut spares bedrock, the core's bore and every footing (item 148 (2));
//!   a wall is filled to the ground, so nothing above the cut is left
//!   unsupported and a breach drops nothing (decision 39).
//!
//! The breach is over once the breach column's top is no wall any more, or
//! the oracle joins the lead raider to the target again; the force walks on.
//!
//! # State, and its hash
//!
//! The Attack table ([`AttackTable`]) holds a row per Attack beacon, the
//! avoid areas, and the planted charges, hashed as one block after the
//! explored table that **writes nothing while it holds nothing** (S2's plan,
//! E29). The table is walked in ascending beacon id and the units in
//! ascending row, so every choice is a total order (AGENTS.md section 4.6).

use crate::events::{Emission, EventKind};
use crate::ids::Row;
use crate::knowledge::{AssetId, AssetKind, Position};
use crate::math::fixed::{Fx, Sq};
use crate::math::quantity::{Hp, Money, Ms};
use crate::pathing::surface::{NEIGHBOURS, Node, Surface};
use crate::rules::RulesError;
use crate::seams::MandateKind;
use crate::tables::{BeaconId, ColumnBox, SeatId, StructureId, UnitId, UnitKind};
use crate::voxels::VoxelEdit;
use crate::world::{Roe, World};

/// No target: a row whose force has nothing to go after, or a voxel target.
pub const NO_TARGET: u32 = u32::MAX;

/// Never: a force that has not launched.
pub const NEVER: u32 = u32::MAX;

/// No carrier: a charge nobody carries yet.
pub const NO_CARRIER: u32 = u32::MAX;

/// What one wall column costs the breach search to pass, on top of the step
/// onto it: the demolition cost of E23's second cost mode.
///
/// PLACEHOLDER: the breach search's demolition cost per wall column, ten cardinal steps — owner, at S2's demo, with `tune2`'s report.
pub const BREACH_COLUMN_COST: i32 = 100;

/// How many columns one breach search may close before it gives up, counted
/// under [`AttackCounts::breach_exhausted`]: the bound that keeps a search
/// inside a tick's budget (E5's rule for every bounded queue).
///
/// PLACEHOLDER: the breach search's budget, sixteen clusters' worth of columns — owner, at S3, with the pathing certification.
pub const BREACH_EXPANSIONS: u32 = 16_384;

/// The breach search's heap: eight pushes a closed column, the most a search
/// of [`BREACH_EXPANSIONS`] columns can push, so the heap never grows.
const BREACH_HEAP: usize = 16_384 * 8;

/// What an Attack beacon attacks, as its row holds it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Aim {
    /// Nothing written: an empty Attack arm, legal at a deploy and a switch
    /// (decisions-log item 146 (3) (j)), which fields nothing and never
    /// launches.
    #[default]
    Unset,
    /// An enemy beacon, pinned at the seal.
    Beacon(BeaconId),
    /// A fixed voxel.
    Voxel([i16; 3]),
    /// The enemy commander: any enemy seat's, or the one seat named.
    Commander(Option<u8>),
    /// The nearest enemy beacon the seat knows of, chosen at launch (S2's
    /// `atk2`).
    NearestKnownBeacon,
}

impl Aim {
    /// The tag the hash and the snapshot write.
    #[must_use]
    pub const fn tag(self) -> u8 {
        match self {
            Aim::Unset => 0,
            Aim::Beacon(_) => 1,
            Aim::Voxel(_) => 2,
            Aim::Commander(None) => 3,
            Aim::Commander(Some(_)) => 4,
            Aim::NearestKnownBeacon => 5,
        }
    }
}

/// `AttackSettings.Retarget`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Retarget {
    /// Not written, which reads as [`Retarget::None`].
    #[default]
    Unset,
    /// Keep the pinned target: the after-action decides.
    None,
    /// Retarget the nearest enemy beacon the seat knows of.
    NearestKnown,
}

impl Retarget {
    /// The wire value, which is also the hashed byte.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            Retarget::Unset => 0,
            Retarget::None => 1,
            Retarget::NearestKnown => 2,
        }
    }

    /// The setting a wire value names, or `None` for one this build does not
    /// name.
    #[must_use]
    pub const fn from_id(id: u8) -> Option<Retarget> {
        match id {
            0 => Some(Retarget::Unset),
            1 => Some(Retarget::None),
            2 => Some(Retarget::NearestKnown),
            _ => None,
        }
    }
}

/// `AttackSettings.AfterAction`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AfterAction {
    /// Not written, which reads as [`AfterAction::Regroup`].
    #[default]
    Unset,
    /// The surviving force walks back to the staging point.
    Regroup,
    /// The surviving force walks back to its home beacon.
    ReturnHome,
}

impl AfterAction {
    /// The wire value, which is also the hashed byte.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            AfterAction::Unset => 0,
            AfterAction::Regroup => 1,
            AfterAction::ReturnHome => 2,
        }
    }

    /// The setting a wire value names, or `None` for one this build does not
    /// name.
    #[must_use]
    pub const fn from_id(id: u8) -> Option<AfterAction> {
        match id {
            0 => Some(AfterAction::Unset),
            1 => Some(AfterAction::Regroup),
            2 => Some(AfterAction::ReturnHome),
            _ => None,
        }
    }
}

/// An Attack beacon's settings as the sim holds them (spec section 6's Attack
/// row; S2's plan, decision 14).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct AttackSettings {
    /// What to attack.
    pub aim: Aim,
    /// What a lost target does on arrival.
    pub retarget: Retarget,
    /// Where the force gathers, or `None` for the beacon itself.
    pub staging: Option<[i16; 3]>,
    /// Launch once this many raiders stand at the staging point.
    pub force_size: Option<u32>,
    /// Launch once this many game milliseconds of the segment have passed.
    pub launch_ms: Option<i32>,
    /// Break off below this percentage of the launch force, 1 to 100.
    pub retreat_pct: Option<u8>,
    /// Break off this many game milliseconds after the launch.
    pub retreat_ms: Option<i32>,
    /// What the surviving force does when the attack is over.
    pub after: AfterAction,
    /// Whether the force may breach a wall that seals its way.
    pub breach: bool,
    /// Launches a segment, `0` when no row has written it.
    pub waves: u32,
}

impl AttackSettings {
    /// The retarget rule the force reads: the one written, or
    /// [`Retarget::None`] when none is (an empty Attack arm).
    ///
    /// PLACEHOLDER: an empty Attack arm reads its retarget as NONE, its after-action as `REGROUP_AT_STAGING`, its staging as the beacon and its waves as one — owner, S2 demo.
    #[must_use]
    pub const fn retarget_reading(self) -> Retarget {
        match self.retarget {
            Retarget::Unset => Retarget::None,
            other => other,
        }
    }

    /// The after-action the force reads: the one written, or
    /// [`AfterAction::Regroup`] (see [`AttackSettings::retarget_reading`]).
    #[must_use]
    pub const fn after_reading(self) -> AfterAction {
        match self.after {
            AfterAction::Unset => AfterAction::Regroup,
            other => other,
        }
    }

    /// The launches a segment the force reads: the number written, or one
    /// (see [`AttackSettings::retarget_reading`]).
    #[must_use]
    pub const fn waves_reading(self) -> u32 {
        if self.waves == 0 { 1 } else { self.waves }
    }

    /// How many raiders the fabricator keeps fielded while the force gathers
    /// (decision 13's work in hand): `force_size`, or one when only the
    /// segment time is written, and none without a target or a launch.
    #[must_use]
    pub fn wanted_force(self) -> u32 {
        if self.aim == Aim::Unset {
            return 0;
        }
        match (self.force_size, self.launch_ms) {
            // A force of none is one: a launch the fabricator never fields
            // for would wait for ever.
            (Some(size), _) => size.max(1),
            (None, Some(_)) => 1,
            (None, None) => 0,
        }
    }
}

/// Where an Attack beacon's force is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Phase {
    /// At the staging point, waiting for its launch condition.
    #[default]
    Gathering,
    /// On its way to its target, or fighting it.
    Launched,
    /// Walking home after a `RETURN_HOME` after-action.
    Returning,
}

impl Phase {
    /// The hashed byte.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            Phase::Gathering => 0,
            Phase::Launched => 1,
            Phase::Returning => 2,
        }
    }

    /// The phase a hashed byte names.
    #[must_use]
    pub const fn from_id(id: u8) -> Option<Phase> {
        match id {
            0 => Some(Phase::Gathering),
            1 => Some(Phase::Launched),
            2 => Some(Phase::Returning),
            _ => None,
        }
    }
}

/// Where a breach's demolition charge is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ChargeState {
    /// None is wanted: the mortars are shelling, or the charge has fired.
    #[default]
    None,
    /// The breach wants a charge, which the beacon asks the Quartermaster for.
    Wanted,
    /// Bought and carried by the breach's carrier to the plant point.
    Loaded,
    /// Planted: it is in [`AttackTable::charges`] until it fires.
    Planted,
}

impl ChargeState {
    /// The hashed byte.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            ChargeState::None => 0,
            ChargeState::Wanted => 1,
            ChargeState::Loaded => 2,
            ChargeState::Planted => 3,
        }
    }

    /// The state a hashed byte names.
    #[must_use]
    pub const fn from_id(id: u8) -> Option<ChargeState> {
        match id {
            0 => Some(ChargeState::None),
            1 => Some(ChargeState::Wanted),
            2 => Some(ChargeState::Loaded),
            3 => Some(ChargeState::Planted),
            _ => None,
        }
    }
}

/// A breach in hand: where the wall is, where the charge is planted from, and
/// the charge.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Breach {
    /// The breach column's top wall voxel, whole voxels: what the mortars aim
    /// at.
    pub wall: [i32; 3],
    /// The column before it on the breach search's way, as the voxel a unit
    /// stands on there: where the carrier plants.
    pub plant: [i32; 3],
    /// The `z` of the wall's footing: the top voxel under the wall that is no
    /// wall. The charge cuts everything above it.
    pub footing: i32,
    /// The charge.
    pub charge: ChargeState,
    /// The raider carrying it, a [`UnitId`] raw value, or [`NO_CARRIER`].
    pub carrier: u32,
}

/// One Attack beacon's settings and its force's state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AttackRow {
    /// The beacon, with its generation.
    pub beacon: BeaconId,
    /// Its settings.
    pub settings: AttackSettings,
    /// Where its force is.
    pub phase: Phase,
    /// What the launched force goes after: an [`AssetId`] raw value (an enemy
    /// beacon or a hunted commander), or [`NO_TARGET`].
    pub target: u32,
    /// The weak point this tick (decision 14): an [`AssetId`] raw value, or
    /// [`NO_TARGET`].
    pub mark: u32,
    /// Where the force is going: its target where the seat last knew it.
    pub aim_at: [Fx; 3],
    /// The tick the force launched on, or [`NEVER`].
    pub launched_at: u32,
    /// How many raiders it launched with.
    pub launch_force: u32,
    /// How many times it has launched this segment.
    pub waves_used: u32,
    /// The tick the segment those launches counted in opened on.
    pub wave_segment: u32,
    /// The breach in hand, if any.
    pub breach: Option<Breach>,
}

impl AttackRow {
    /// A fresh row for `beacon`: no settings, gathering.
    #[must_use]
    pub const fn fresh(beacon: BeaconId) -> AttackRow {
        AttackRow {
            beacon,
            settings: AttackSettings {
                aim: Aim::Unset,
                retarget: Retarget::Unset,
                staging: None,
                force_size: None,
                launch_ms: None,
                retreat_pct: None,
                retreat_ms: None,
                after: AfterAction::Unset,
                breach: false,
                waves: 0,
            },
            phase: Phase::Gathering,
            target: NO_TARGET,
            mark: NO_TARGET,
            aim_at: [Fx::from_raw(0); 3],
            launched_at: NEVER,
            launch_force: 0,
            waves_used: 0,
            wave_segment: 0,
            breach: None,
        }
    }
}

/// A planted demolition charge (decision 9): it fires when its fuse runs out,
/// whatever becomes of its beacon or its carrier, since it is in the world.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Charge {
    /// The Attack beacon whose breach planted it.
    pub beacon: BeaconId,
    /// The carrier's seat, which the cut carries.
    pub seat: SeatId,
    /// The breach column at the wall's footing: `[x, y, footing]`.
    pub centre: [i32; 3],
    /// The tick it fires on.
    pub fires_at: u32,
}

/// Every Attack beacon's row, every avoid area, and every planted charge
/// (S2's `atk`).
///
/// **Hashed as one block that writes nothing while the table holds nothing**
/// (S2's plan, E29). A row is opened when a beacon takes the Attack writ and
/// closed when it leaves it or dies (the world's `set_beacon_mandate`,
/// `place_beacon` and `end_mandate`), so a row always names a live Attack
/// beacon; a charge outlives both.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct AttackTable {
    rows: Vec<AttackRow>,
    avoid: Vec<(BeaconId, ColumnBox)>,
    charges: Vec<Charge>,
}

impl AttackTable {
    /// The table's tag in the state hash (E29), after the explored table's 4.
    pub const HASH_TAG: u8 = 5;

    /// An empty table with room for `beacons` rows and as many charges, so a
    /// row opened or a charge planted inside a tick allocates nothing.
    #[must_use]
    pub fn with_room(beacons: u32) -> AttackTable {
        let room = crate::math::index::row(beacons);
        AttackTable {
            rows: Vec::with_capacity(room),
            avoid: Vec::new(),
            charges: Vec::with_capacity(room),
        }
    }

    /// The rows, in ascending beacon id.
    #[must_use]
    pub fn rows(&self) -> &[AttackRow] {
        &self.rows
    }

    /// Make room for `beacons` rows and charges, and `areas` avoid areas, so
    /// opening a row, planting a charge or committing an avoid list inside a
    /// tick allocates nothing (a restored table holds only its own rows).
    pub fn reserve(&mut self, beacons: u32, areas: usize) {
        let room = crate::math::index::row(beacons);
        self.rows.reserve(room.saturating_sub(self.rows.len()));
        self.charges
            .reserve(room.saturating_sub(self.charges.len()));
        self.avoid.reserve(areas.saturating_sub(self.avoid.len()));
    }

    /// The avoid areas, `(attack beacon, area)`, grouped by beacon in
    /// ascending id and in list order within a group.
    #[must_use]
    pub fn avoid(&self) -> &[(BeaconId, ColumnBox)] {
        &self.avoid
    }

    /// The planted charges, in planting order.
    #[must_use]
    pub fn charges(&self) -> &[Charge] {
        &self.charges
    }

    /// Whether the table holds nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty() && self.avoid.is_empty() && self.charges.is_empty()
    }

    fn index_of(&self, beacon: BeaconId) -> Result<usize, usize> {
        self.rows
            .binary_search_by_key(&beacon.raw(), |row| row.beacon.raw())
    }

    /// The row of `beacon`, if it is an Attack beacon.
    #[must_use]
    pub fn row_of(&self, beacon: BeaconId) -> Option<&AttackRow> {
        self.index_of(beacon).ok().and_then(|at| self.rows.get(at))
    }

    /// The row of `beacon`, to write.
    pub fn row_mut(&mut self, beacon: BeaconId) -> Option<&mut AttackRow> {
        let at = self.index_of(beacon).ok()?;
        self.rows.get_mut(at)
    }

    /// Give `beacon` a fresh row, or leave the one it has.
    pub fn open(&mut self, beacon: BeaconId) {
        if let Err(at) = self.index_of(beacon) {
            self.rows.insert(at, AttackRow::fresh(beacon));
        }
    }

    /// Remove `beacon`'s row and its avoid areas: a beacon that leaves the
    /// Attack writ or dies. Its planted charges stay in the world.
    pub fn close(&mut self, beacon: BeaconId) {
        if let Ok(at) = self.index_of(beacon) {
            self.rows.remove(at);
        }
        self.avoid.retain(|(by, _)| *by != beacon);
    }

    /// Replace `beacon`'s avoid areas with `list`, in order. Does nothing for
    /// a beacon with no row.
    pub fn set_avoid(&mut self, beacon: BeaconId, list: &[ColumnBox]) {
        if self.index_of(beacon).is_err() {
            return;
        }
        self.avoid.retain(|(by, _)| *by != beacon);
        let mut index = self
            .avoid
            .partition_point(|(by, _)| by.raw() < beacon.raw());
        for area in list {
            self.avoid.insert(index, (beacon, *area));
            index = index.saturating_add(1);
        }
    }

    /// The avoid areas of `beacon`, in list order.
    pub fn avoid_of(&self, beacon: BeaconId) -> impl Iterator<Item = ColumnBox> + '_ {
        self.avoid
            .iter()
            .filter(move |(by, _)| *by == beacon)
            .map(|(_, area)| *area)
    }

    /// File a planted charge.
    pub fn plant(&mut self, charge: Charge) {
        self.charges.push(charge);
    }

    /// Take every charge whose fuse has run out by `tick`, in planting order,
    /// into `out`.
    pub fn take_fired(&mut self, tick: u32, out: &mut Vec<Charge>) {
        out.clear();
        let mut at: usize = 0;
        while let Some(charge) = self.charges.get(at).copied() {
            if charge.fires_at <= tick {
                out.push(charge);
                self.charges.remove(at);
            } else {
                at = at.saturating_add(1);
            }
        }
    }

    /// Replace the table from a restored snapshot. `false`, changing nothing,
    /// when the rows are not in strictly ascending beacon id, or the avoid
    /// list is not grouped in ascending order or names a beacon with no row.
    pub fn restore(
        &mut self,
        rows: Vec<AttackRow>,
        avoid: Vec<(BeaconId, ColumnBox)>,
        charges: Vec<Charge>,
    ) -> bool {
        let ascending = rows
            .windows(2)
            .all(|pair| matches!(pair, [a, b] if a.beacon.raw() < b.beacon.raw()));
        let grouped = avoid
            .windows(2)
            .all(|pair| matches!(pair, [a, b] if a.0.raw() <= b.0.raw()));
        let owned = avoid.iter().all(|(by, _)| {
            rows.binary_search_by_key(&by.raw(), |row| row.beacon.raw())
                .is_ok()
        });
        if !ascending || !grouped || !owned {
            return false;
        }
        self.rows.clear();
        self.rows.extend(rows);
        self.avoid = avoid;
        self.charges.clear();
        self.charges.extend(charges);
        true
    }

    /// Append the table to the canonical encoding: nothing while it holds
    /// nothing (E29), otherwise its tag, its rows in ascending beacon id, its
    /// avoid areas and its charges, every optional field behind its presence
    /// byte.
    pub fn encode(&self, enc: &mut crate::encoding::Enc) {
        if self.is_empty() {
            return;
        }
        enc.u8(Self::HASH_TAG);
        enc.len(table_len(self.rows.len()));
        for row in &self.rows {
            encode_row(enc, row);
        }
        enc.len(table_len(self.avoid.len()));
        for (by, area) in &self.avoid {
            enc.u32(by.raw());
            for axis in area.min_column().into_iter().chain(area.max_column()) {
                enc.i32(i32::from(axis));
            }
        }
        enc.len(table_len(self.charges.len()));
        for charge in &self.charges {
            enc.u32(charge.beacon.raw());
            enc.u8(charge.seat.raw());
            for axis in charge.centre {
                enc.i32(axis);
            }
            enc.u32(charge.fires_at);
        }
    }
}

/// One row, in declared field order.
fn encode_row(enc: &mut crate::encoding::Enc, row: &AttackRow) {
    let settings = &row.settings;
    enc.u32(row.beacon.raw());
    enc.u8(settings.aim.tag());
    match settings.aim {
        Aim::Unset | Aim::Commander(None) | Aim::NearestKnownBeacon => {}
        Aim::Beacon(beacon) => enc.u32(beacon.raw()),
        Aim::Voxel(at) => {
            for axis in at {
                enc.i32(i32::from(axis));
            }
        }
        Aim::Commander(Some(seat)) => enc.u8(seat),
    }
    enc.u8(settings.retarget.id());
    enc.bool(settings.staging.is_some());
    if let Some(at) = settings.staging {
        for axis in at {
            enc.i32(i32::from(axis));
        }
    }
    enc.bool(settings.force_size.is_some());
    if let Some(size) = settings.force_size {
        enc.u32(size);
    }
    enc.bool(settings.launch_ms.is_some());
    if let Some(ms) = settings.launch_ms {
        enc.i32(ms);
    }
    enc.bool(settings.retreat_pct.is_some());
    if let Some(pct) = settings.retreat_pct {
        enc.u8(pct);
    }
    enc.bool(settings.retreat_ms.is_some());
    if let Some(ms) = settings.retreat_ms {
        enc.i32(ms);
    }
    enc.u8(settings.after.id());
    enc.bool(settings.breach);
    enc.u32(settings.waves);
    enc.u8(row.phase.id());
    enc.u32(row.target);
    enc.u32(row.mark);
    for axis in row.aim_at {
        enc.i32(axis.raw());
    }
    enc.u32(row.launched_at);
    enc.u32(row.launch_force);
    enc.u32(row.waves_used);
    enc.u32(row.wave_segment);
    enc.bool(row.breach.is_some());
    if let Some(breach) = row.breach {
        for axis in breach.wall.into_iter().chain(breach.plant) {
            enc.i32(axis);
        }
        enc.i32(breach.footing);
        enc.u8(breach.charge.id());
        enc.u32(breach.carrier);
    }
}

/// A table's length as the hash's `u32` count: an Attack row per beacon, an
/// avoid area per list field and a charge per breach, all far below it.
fn table_len(len: usize) -> u32 {
    // E7: rule: no table this world holds reaches `u32::MAX` rows
    u32::try_from(len).unwrap_or(u32::MAX)
}

/// What one committed settings row names about an Attack beacon's target
/// (S2's `atk`): an enemy beacon by its handle, pinned at the seal or not, a
/// voxel, or the enemy commander.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TargetSpec {
    /// An enemy beacon by its `e_NN` handle, and the beacon the seal pinned
    /// it to ([`crate::interpreter::Plan::pin_foreign`]), `None` until then
    /// or when the handle named nothing at the seal.
    Foreign {
        /// The handle's number.
        handle: u32,
        /// The beacon it was pinned to.
        pinned: Option<BeaconId>,
    },
    /// A fixed voxel.
    Voxel([i16; 3]),
    /// The enemy commander, of the seat named or any.
    Commander(Option<u8>),
    /// The nearest enemy beacon the seat knows of, chosen at launch: the
    /// selector `nearest` with the filter `side: ENEMY_KNOWN` and nothing
    /// else (S2's `atk2`).
    NearestKnownBeacon,
}

/// The Attack settings a committed settings row writes, each only when the
/// row names it: a proto3 scalar at its default is silent, a written `launch`
/// or `retreat` block replaces both of its conditions, and an avoid list
/// replaces the one the beacon has only when it holds an entry (as Build's
/// lists do).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct AttackEdit {
    /// `target`.
    pub target: Option<TargetSpec>,
    /// `retarget`.
    pub retarget: Option<Retarget>,
    /// `staging`.
    pub staging: Option<[i16; 3]>,
    /// `launch`: `force_size` and `segment_ms`.
    pub launch: Option<(Option<u32>, Option<i32>)>,
    /// `retreat`: `force_below_pct` and `after_launch_ms`.
    pub retreat: Option<(Option<u8>, Option<i32>)>,
    /// `after_action`.
    pub after: Option<AfterAction>,
    /// `breach_allowed`.
    pub breach: Option<bool>,
    /// `waves`.
    pub waves: Option<u32>,
    /// `avoid_areas`, in order.
    pub avoid: Option<Vec<ColumnBox>>,
}

/// Why an attack ended: the value of an `attack_ended` event.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ended {
    /// The target was destroyed or is gone, and the retarget rule found none.
    TargetGone,
    /// A retreat condition held.
    Retreat,
    /// Every raider of the force died.
    ForceLost,
    /// The way to the target is sealed and the beacon may not breach, or the
    /// breach search found no wall to breach.
    Sealed,
    /// A voxel target: the force stands there with nothing in reach.
    Arrived,
    /// The beacon's `roe` was changed from `FREE_FIRE` after the launch.
    StoodDown,
}

impl Ended {
    /// The event's value.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            Ended::TargetGone => 1,
            Ended::Retreat => 2,
            Ended::ForceLost => 3,
            Ended::Sealed => 4,
            Ended::Arrived => 5,
            Ended::StoodDown => 6,
        }
    }
}

/// The Attack mandate's counters, diagnostic only and never hashed: every
/// breach the search could not give, under its reason (E4).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct AttackCounts {
    /// A breach search that spent [`BREACH_EXPANSIONS`] and found no way.
    pub breach_exhausted: u64,
    /// A breach search whose way crossed no wall: the seal is not a wall's.
    pub breach_no_wall: u64,
    /// A breach search that found no way at all, wall or none.
    pub breach_no_way: u64,
    /// A charge's cut the edit queue could not take.
    pub cut_refused: u64,
    /// A launch condition that held with nothing to launch at: a hunt with
    /// no fresh sighting of an enemy commander, counted per decision.
    pub launch_without_target: u64,
    /// A launch condition that held for a raid on the nearest known enemy
    /// beacon ([`Aim::NearestKnownBeacon`]) while the seat held no fresh
    /// sighting of one it does not see gone, counted per decision.
    pub launch_without_known_beacon: u64,
}

/// The rows the Attack mandate reads, typed at load (S2's `atk`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AttackRows {
    /// `combat.threat_step_cost_multiplier`, at least one: how many times its
    /// ordinary cost a step inside a known threat's range, or inside an
    /// Attack beacon's avoid area, costs a route that avoids it.
    pub step_multiplier: i32,
    /// `combat.charge_radius_voxels`, at least one: the charge's cut.
    pub charge_radius: i32,
    /// `combat.charge_fuse_ms`, never negative.
    pub charge_fuse: Ms,
    /// `structures.demolition_charge_cost_dollars`.
    pub charge_cost: Money,
    /// `verifier.reach_memory_ms`, never negative: how long a seat's sighting
    /// counts, the reach window an Attack target was qualified against.
    pub reach_memory: Ms,
}

impl AttackRows {
    /// Read the rows, refusing what the tick could not use.
    ///
    /// # Errors
    ///
    /// [`RulesError::MissingBlock`] for an absent `combat`, `structures` or
    /// `verifier` block, and [`RulesError::OutOfRange`] for a multiplier or a
    /// radius of nought, a radius past a voxel length, or a negative fuse or
    /// window.
    pub fn read(message: &pharmakos_proto::gp::v1::RulesTable) -> Result<AttackRows, RulesError> {
        let combat = message
            .combat
            .as_ref()
            .ok_or(RulesError::MissingBlock("combat"))?;
        let structures = message
            .structures
            .as_ref()
            .ok_or(RulesError::MissingBlock("structures"))?;
        let verifier = message
            .verifier
            .as_ref()
            .ok_or(RulesError::MissingBlock("verifier"))?;
        let out_of_range = |field: &str, value: String| RulesError::OutOfRange {
            field: field.to_owned(),
            value,
        };
        let step_multiplier = i32::try_from(combat.threat_step_cost_multiplier)
            .ok()
            .filter(|multiplier| *multiplier >= 1)
            .ok_or_else(|| {
                out_of_range(
                    "combat.threat_step_cost_multiplier",
                    format!(
                        "{}; a surcharge is at least one times a step's cost",
                        combat.threat_step_cost_multiplier
                    ),
                )
            })?;
        let charge_radius = i16::try_from(combat.charge_radius_voxels)
            .ok()
            .filter(|radius| *radius >= 1)
            .map(i32::from)
            .ok_or_else(|| {
                out_of_range(
                    "combat.charge_radius_voxels",
                    format!(
                        "{}; a charge cuts at least one voxel, at most a voxel length",
                        combat.charge_radius_voxels
                    ),
                )
            })?;
        if combat.charge_fuse_ms < 0 {
            return Err(out_of_range(
                "combat.charge_fuse_ms",
                format!("{}; a fuse runs forward", combat.charge_fuse_ms),
            ));
        }
        if verifier.reach_memory_ms < 0 {
            return Err(out_of_range(
                "verifier.reach_memory_ms",
                format!("{}; a window runs forward", verifier.reach_memory_ms),
            ));
        }
        Ok(AttackRows {
            step_multiplier,
            charge_radius,
            charge_fuse: Ms::new(combat.charge_fuse_ms),
            charge_cost: Money::new(i64::from(structures.demolition_charge_cost_dollars)),
            reach_memory: Ms::new(verifier.reach_memory_ms),
        })
    }
}

/// An Attack beacon as [`update`] and the raiders read it.
#[derive(Clone, Copy, Debug)]
struct Post {
    seat: SeatId,
    /// Where its force gathers: the staging point, or the beacon.
    staging: [Fx; 3],
    roe: Roe,
}

/// A whole-voxel triple widened to `i32`.
fn widen(at: [i16; 3]) -> [i32; 3] {
    at.map(i32::from)
}

/// The Attack beacon `row` describes, or `None` when it is no live beacon of a
/// seat in the match.
fn post_of(world: &World, row: &AttackRow) -> Option<Post> {
    let beacons = world.beacons();
    let at = beacons.row_of(row.beacon).ok()?.index();
    if !beacons.hit_points().get(at)?.is_alive() {
        return None;
    }
    let seat = SeatId::new(beacons.seats().get(at).copied()?);
    if !seat.is_some() {
        return None;
    }
    let centre = beacons.positions().get(at).copied()?;
    let roe = Roe::of_setting(beacons.commons().get(at)?.roe, MandateKind::Attack)?;
    let staging = row
        .settings
        .staging
        .map_or(centre, |at| world.standing_point(widen(at)));
    Some(Post { seat, staging, roe })
}

/// Bring every Attack beacon's force up to date, then fire every charge whose
/// fuse has run out: the second thing the programs phase does, after Defend's
/// update, so every raider of a beacon reads one answer this tick.
pub(crate) fn update(world: &mut World) {
    let mut index: usize = 0;
    while let Some(row) = world.attack().rows().get(index).copied() {
        index = index.saturating_add(1);
        let Some(post) = post_of(world, &row) else {
            continue;
        };
        let next = advance(world, row, &post);
        if let Some(slot) = world.attack_mut().row_mut(row.beacon) {
            // A charge planted this tick by `plant_charge` is the program's,
            // and the programs run after this; nothing else writes a row
            // between the read above and this write.
            *slot = next;
        }
    }
    // Last, so a row reads its charge as planted on the tick it fires and
    // finds the cut done on the next: the voxel phase applies it this tick.
    fire_charges(world);
}

/// File the cut of every charge whose fuse has run out, in planting order
/// (decision 40): the edit carries the carrier's seat, and the voxel phase of
/// this tick applies it.
fn fire_charges(world: &mut World) {
    if world.attack().charges().is_empty() {
        return;
    }
    let tick = world.tick();
    let mut fired = world.take_charge_scratch();
    world.attack_mut().take_fired(tick.raw(), &mut fired);
    let radius = world.rules().attack().charge_radius;
    for charge in &fired {
        let edit = VoxelEdit::Cut {
            centre: charge.centre,
            radius,
            by: Some(charge.seat),
        };
        if world.file_voxel_edit(edit).is_err() {
            world.count_cut_refused();
            continue;
        }
        let [x, y, z] = charge.centre;
        world.emit(
            tick,
            Emission::of(EventKind::ChargeDetonated)
                .seat(charge.seat)
                .at(Position::from_array(voxel_centre([
                    x,
                    y,
                    z.saturating_add(1),
                ])))
                .value(i64::from(radius)),
        );
        if let Some(row) = world.attack_mut().row_mut(charge.beacon)
            && let Some(breach) = row.breach.as_mut()
            && breach.charge == ChargeState::Planted
        {
            breach.charge = ChargeState::None;
        }
    }
    world.put_charge_scratch(fired);
}

/// The point at the centre-bottom of a voxel, where a unit standing in it is.
fn voxel_centre(at: [i32; 3]) -> [Fx; 3] {
    // E7: rule: a voxel off the map stands at the origin; every voxel of the map is a coordinate (`RulesTable::map_last_voxel`)
    at.map(|axis| Fx::from_voxels(i16::try_from(axis).unwrap_or(0)))
}

/// One row's tick.
fn advance(world: &mut World, mut row: AttackRow, post: &Post) -> AttackRow {
    let segment = world.match_state().segment_started().raw();
    if row.wave_segment != segment {
        row.wave_segment = segment;
        row.waves_used = 0;
    }
    row.mark = NO_TARGET;
    match row.phase {
        Phase::Gathering => gather(world, &mut row, post),
        Phase::Launched => press(world, &mut row, post),
        Phase::Returning => {
            let radius = world.arrive_radius();
            let back = crate::defend::live_beacon_at(world, row.beacon).is_none_or(|home| {
                crate::defend::homed(world, row.beacon, UnitKind::Raider).all(|unit| {
                    position_of(world, unit)
                        .is_none_or(|here| crate::programs::within(here, home, radius))
                })
            });
            if back {
                row.phase = Phase::Gathering;
            }
        }
    }
    row
}

/// Where a unit stands.
fn position_of(world: &World, unit: u32) -> Option<[Fx; 3]> {
    world
        .units()
        .positions()
        .get(crate::math::index::row(unit))
        .copied()
}

/// How many living raiders the beacon has: its force.
fn force_of(world: &World, beacon: BeaconId) -> u32 {
    let count = crate::defend::homed(world, beacon, UnitKind::Raider).count();
    // E7: cannot miss: a count of rows is below the unit ceiling, a `u32`
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// The force's lead: its lowest-row living raider, the one whose column the
/// oracle is asked about and who carries a charge.
fn lead_of(world: &World, beacon: BeaconId) -> Option<u32> {
    crate::defend::homed(world, beacon, UnitKind::Raider).next()
}

/// The gathering force's tick: launch when a condition holds (see the module
/// docs).
fn gather(world: &mut World, row: &mut AttackRow, post: &Post) {
    if post.roe != Roe::Free || row.waves_used >= row.settings.waves_reading() {
        return;
    }
    let force = force_of(world, row.beacon);
    if force == 0 {
        return;
    }
    let radius = world.arrive_radius();
    let staged = crate::defend::homed(world, row.beacon, UnitKind::Raider)
        .filter(|unit| {
            position_of(world, *unit)
                .is_some_and(|here| crate::programs::within(here, post.staging, radius))
        })
        .count();
    // E7: cannot miss: a count of rows is below the unit ceiling, a `u32`
    let staged = u32::try_from(staged).unwrap_or(u32::MAX);
    let elapsed = Ms::from_ticks(world.tick().since(world.match_state().segment_started()));
    let by_size = row.settings.force_size.is_some_and(|size| staged >= size);
    let by_time = row.settings.launch_ms.is_some_and(|ms| elapsed.raw() >= ms);
    if !by_size && !by_time {
        return;
    }
    let Some((target, aim)) = launch_target(world, row, post) else {
        // Counted, never silent (E4): a hunt or a raid on the nearest known
        // beacon with no fresh sighting waits, each under its own reason.
        if row.settings.aim == Aim::NearestKnownBeacon {
            world.count_launch_without_known_beacon();
        } else {
            world.count_launch_without_target();
        }
        return;
    };
    let tick = world.tick();
    row.phase = Phase::Launched;
    row.target = target;
    row.aim_at = aim;
    row.launched_at = tick.raw();
    row.launch_force = force;
    row.waves_used = row.waves_used.saturating_add(1);
    row.breach = None;
    if let Ok(subject) = AssetId::of_beacon(row.beacon) {
        world.emit(
            tick,
            Emission::of(EventKind::AttackLaunched)
                .seat(post.seat)
                .subject(subject)
                .at(Position::from_array(aim))
                .value(i64::from(force)),
        );
    }
}

/// What a launch goes after and where: the pinned beacon where the seat last
/// saw it, a voxel, or the nearest enemy commander or enemy beacon the seat
/// has a fresh sighting of. `None` when there is nothing to launch at.
fn launch_target(world: &World, row: &AttackRow, post: &Post) -> Option<(u32, [Fx; 3])> {
    match row.settings.aim {
        Aim::Unset => None,
        Aim::Voxel(at) => Some((NO_TARGET, world.standing_point(widen(at)))),
        Aim::Beacon(beacon) => {
            let asset = AssetId::of_beacon(beacon).ok()?;
            // Launch never re-tests the pinned target (item 13): the force
            // walks to where the seat last knew it, and decides on arrival.
            // A target the seat holds no sighting of and that is gone is
            // decided at once, from the staging point.
            // E7: rule: a pinned target the seat no longer remembers is sought from the staging point, where the arrival rule decides it at once; never from ground truth
            let place =
                sighting_of(world, post.seat, asset.raw()).map_or(post.staging, |(at, _)| at);
            Some((asset.raw(), place))
        }
        Aim::Commander(seat) => nearest_commander(world, post.seat, seat, post.staging),
        // No beacon is excluded at launch: `NO_TARGET` names no asset.
        Aim::NearestKnownBeacon => nearest_known_beacon(world, post.seat, NO_TARGET, post.staging),
    }
}

/// The seat's sighting of `asset`: where it last saw it and the tick it saw it
/// on.
fn sighting_of(world: &World, seat: SeatId, asset: u32) -> Option<([Fx; 3], u32)> {
    let table = world.sightings();
    let rows = table
        .seats()
        .iter()
        .zip(table.assets())
        .zip(table.places())
        .zip(table.seen_at());
    for (((by, of), at), seen) in rows {
        if *by == seat.raw() && *of == asset {
            return Some((*at, *seen));
        }
    }
    None
}

/// Whether a sighting taken on `seen` is inside the reach window now.
fn fresh(world: &World, seen: u32) -> bool {
    let age = Ms::from_ticks(world.tick().raw().saturating_sub(seen));
    age.raw() <= world.rules().attack().reach_memory.raw()
}

/// One row of the seat's sighting table, as the two choices below read it.
#[derive(Clone, Copy, Debug)]
struct Seen {
    asset: u32,
    owner: u8,
    kind: u8,
    at: [Fx; 3],
    tick: u32,
}

/// Call `each` with every sighting `seat` holds, in the table's order.
fn for_each_sighting<F: FnMut(Seen)>(world: &World, seat: SeatId, mut each: F) {
    let table = world.sightings();
    let rows = table
        .seats()
        .iter()
        .zip(table.assets())
        .zip(table.owners())
        .zip(table.kinds())
        .zip(table.places())
        .zip(table.seen_at());
    for (((((by, asset), owner), kind), at), seen) in rows {
        if *by == seat.raw() {
            each(Seen {
                asset: *asset,
                owner: *owner,
                kind: *kind,
                at: *at,
                tick: *seen,
            });
        }
    }
}

/// The nearest enemy commander `seat` has a fresh sighting of, nearest
/// `from`, ties to the lowest seat id; only `only`'s when it names a seat
/// (the owner's answer of 2026-10-26).
fn nearest_commander(
    world: &World,
    seat: SeatId,
    only: Option<u8>,
    from: [Fx; 3],
) -> Option<(u32, [Fx; 3])> {
    let mut best: Option<((Sq, u8), u32, [Fx; 3])> = None;
    for_each_sighting(world, seat, |seen| {
        if seen.kind != AssetKind::Commander.id()
            || !crate::defend::enemy(seat, seen.owner)
            || only.is_some_and(|only| only != seen.owner)
            || !fresh(world, seen.tick)
        {
            return;
        }
        let key = (Sq::between(seen.at, from), seen.owner);
        if best.is_none_or(|(held, ..)| key < held) {
            best = Some((key, seen.asset, seen.at));
        }
    });
    best.map(|(_, asset, at)| (asset, at))
}

/// The nearest enemy beacon `seat` holds a fresh sighting of, nearest `from`,
/// ties to the lowest id, other than `not` and other than one the seat sees
/// is gone: `NEAREST_KNOWN`'s retarget, and [`Aim::NearestKnownBeacon`]'s
/// launch, with `not` as [`NO_TARGET`].
fn nearest_known_beacon(
    world: &World,
    seat: SeatId,
    not: u32,
    from: [Fx; 3],
) -> Option<(u32, [Fx; 3])> {
    let mut best: Option<((Sq, u32), [Fx; 3])> = None;
    for_each_sighting(world, seat, |seen| {
        if seen.kind != AssetKind::Beacon.id()
            || seen.asset == not
            || !crate::defend::enemy(seat, seen.owner)
            || !fresh(world, seen.tick)
        {
            return;
        }
        if !alive(world, seen.asset) && world.seen_by(seat.raw(), seen.at) {
            return;
        }
        let key = (Sq::between(seen.at, from), seen.asset);
        if best.is_none_or(|(held, _)| key < held) {
            best = Some((key, seen.at));
        }
    });
    best.map(|((_, asset), at)| (asset, at))
}

/// A hunted commander as the seat knows it: where it stands when a sensor
/// sees it now (`true`), else where the seat last saw it while that sighting
/// is fresh (`false`); `None` once it is dead or the sighting has gone stale.
fn track_commander(world: &World, seat: SeatId, asset: u32) -> Option<([Fx; 3], bool)> {
    // Seen alive, it is followed; seen dead, it is gone. Unseen, its death is
    // nothing the seat knows (the owner's answer of 2026-10-26: the seat's
    // knowledge, never ground truth), so the hunt goes on to the sighting.
    if let Some(here) = asset_at(world, asset)
        && world.seen_by(seat.raw(), here)
    {
        return alive(world, asset).then_some((here, true));
    }
    let (at, seen) = sighting_of(world, seat, asset)?;
    fresh(world, seen).then_some((at, false))
}

/// The weak point of the enemy beacon `beacon` (decision 14): its seat's
/// lowest-HP finished structure inside its sphere that `seat` sees, ties
/// nearest the beacon then lowest id, else the beacon.
fn weak_point(world: &World, seat: SeatId, beacon: BeaconId) -> Option<u32> {
    let centre = crate::defend::live_beacon_at(world, beacon)?;
    let owner = world
        .beacons()
        .seats()
        .get(world.beacons().row_of(beacon).ok()?.index())
        .copied()?;
    let limit = Sq::of_radius(world.sphere_radius());
    let structures = world.structures();
    let rows = structures
        .seats()
        .iter()
        .zip(structures.hit_points())
        .zip(structures.building())
        .zip(structures.positions())
        .zip(Row::all());
    let mut best: Option<(i32, Sq, u32)> = None;
    for ((((by, hp), building), at), row) in rows {
        if *by != owner
            || !hp.is_alive()
            || *building
            || Sq::between(*at, centre) > limit
            || !world.seen_by(seat.raw(), *at)
        {
            continue;
        }
        let Some(asset) = structures
            .id_of(row)
            .and_then(|id| AssetId::of_structure(id).ok())
        else {
            continue;
        };
        let key = (hp.raw(), Sq::between(*at, centre), asset.raw());
        if best.is_none_or(|held| key < held) {
            best = Some(key);
        }
    }
    match best {
        Some((_, _, asset)) => Some(asset),
        None => AssetId::of_beacon(beacon).ok().map(AssetId::raw),
    }
}

/// Whether an asset the force goes after is alive.
fn alive(world: &World, asset: u32) -> bool {
    if asset == NO_TARGET {
        return false;
    }
    let id = AssetId::new(asset);
    match id.tag() {
        AssetId::TAG_BEACON => {
            crate::defend::live_beacon_at(world, BeaconId::new(id.index())).is_some()
        }
        AssetId::TAG_UNIT => world
            .units()
            .row_of(UnitId::new(id.index()))
            .ok()
            .and_then(|row| world.units().hit_points().get(row.index()).copied())
            .is_some_and(Hp::is_alive),
        AssetId::TAG_STRUCTURE => world
            .structures()
            .row_of(StructureId::new(id.index()))
            .ok()
            .and_then(|row| world.structures().hit_points().get(row.index()).copied())
            .is_some_and(Hp::is_alive),
        _ => false,
    }
}

/// Where a live asset stands.
fn asset_at(world: &World, asset: u32) -> Option<[Fx; 3]> {
    if asset == NO_TARGET {
        return None;
    }
    let id = AssetId::new(asset);
    match id.tag() {
        AssetId::TAG_BEACON => crate::defend::live_beacon_at(world, BeaconId::new(id.index())),
        AssetId::TAG_UNIT => {
            let row = world.units().row_of(UnitId::new(id.index())).ok()?.index();
            world.units().positions().get(row).copied()
        }
        AssetId::TAG_STRUCTURE => {
            let row = world
                .structures()
                .row_of(StructureId::new(id.index()))
                .ok()?
                .index();
            world.structures().positions().get(row).copied()
        }
        _ => None,
    }
}

/// End the attack: announce why and apply the after-action.
fn end(world: &mut World, row: &mut AttackRow, post: &Post, why: Ended) {
    row.target = NO_TARGET;
    row.mark = NO_TARGET;
    row.breach = None;
    row.launched_at = NEVER;
    row.phase = match row.settings.after_reading() {
        AfterAction::ReturnHome => Phase::Returning,
        AfterAction::Regroup | AfterAction::Unset => Phase::Gathering,
    };
    if let Ok(subject) = AssetId::of_beacon(row.beacon) {
        world.emit(
            world.tick(),
            Emission::of(EventKind::AttackEnded)
                .seat(post.seat)
                .subject(subject)
                .at(Position::from_array(row.aim_at))
                .value(i64::from(why.id())),
        );
    }
}

/// Whether a retreat condition holds for a force of `force` raiders.
fn retreats(world: &World, row: &AttackRow, force: u32) -> bool {
    let thinned = row.settings.retreat_pct.is_some_and(|pct| {
        u64::from(force).saturating_mul(100)
            < u64::from(pct).saturating_mul(u64::from(row.launch_force))
    });
    let timed = row.settings.retreat_ms.is_some_and(|ms| {
        Ms::from_ticks(world.tick().raw().saturating_sub(row.launched_at)).raw() >= ms
    });
    thinned || timed
}

/// The launched force's tick (see the module docs).
fn press(world: &mut World, row: &mut AttackRow, post: &Post) {
    lose_dead_carriers_charge(world, row, post);
    // Off FREE_FIRE the force starts no fight (item 148 (2)), launched or
    // not: a `roe` written on site after the launch stands it down.
    if post.roe != Roe::Free {
        end(world, row, post, Ended::StoodDown);
        return;
    }
    let force = force_of(world, row.beacon);
    if force == 0 {
        end(world, row, post, Ended::ForceLost);
        return;
    }
    if retreats(world, row, force) {
        end(world, row, post, Ended::Retreat);
        return;
    }
    let lead = lead_of(world, row.beacon).and_then(|unit| position_of(world, unit));
    // A hunt that retargeted to a beacon is a beacon attack from then on.
    let hunting = AssetId::new(row.target).tag() == AssetId::TAG_UNIT;
    let lost = match row.settings.aim {
        Aim::Commander(_) if hunting => match track_commander(world, post.seat, row.target) {
            Some((at, live)) => {
                row.aim_at = at;
                // Arrived where the seat last saw it, and it is not there.
                !live && world.seen_by(post.seat.raw(), at)
            }
            None => true,
        },
        Aim::Beacon(_) | Aim::Commander(_) | Aim::NearestKnownBeacon => {
            if alive(world, row.target) {
                if let Some(mark) = weak_point(
                    world,
                    post.seat,
                    BeaconId::new(AssetId::new(row.target).index()),
                ) {
                    row.mark = mark;
                }
                false
            } else {
                // Decided on arrival: the seat sees the place, and nothing
                // is there.
                world.seen_by(post.seat.raw(), row.aim_at)
            }
        }
        Aim::Voxel(_) => {
            let arrived = lead.is_some_and(|here| {
                crate::programs::within(here, row.aim_at, world.arrive_radius())
            });
            if arrived && !enemy_in_reach(world, post.seat, row.aim_at) {
                end(world, row, post, Ended::Arrived);
                return;
            }
            false
        }
        Aim::Unset => true,
    };
    if lost {
        let retarget = match row.settings.retarget_reading() {
            Retarget::NearestKnown => {
                nearest_known_beacon(world, post.seat, row.target, row.aim_at)
            }
            Retarget::None | Retarget::Unset => None,
        };
        let Some((target, at)) = retarget else {
            end(world, row, post, Ended::TargetGone);
            return;
        };
        row.target = target;
        row.aim_at = at;
        row.breach = None;
        if let Some(mark) = weak_point(
            world,
            post.seat,
            BeaconId::new(AssetId::new(target).index()),
        ) {
            row.mark = mark;
        }
    }
    let destination = destination_of(world, row);
    seal_or_breach(world, row, post, lead, destination);
}

/// Whether `seat` sees an enemy unit, structure or beacon within a raider's
/// weapon range of `at`.
fn enemy_in_reach(world: &World, seat: SeatId, at: [Fx; 3]) -> bool {
    let reach = world
        .rules()
        .unit(UnitKind::Raider)
        .weapon
        .map_or(world.arrive_radius(), crate::arms::WeaponRow::range);
    let limit = Sq::of_radius(reach);
    let near = |owner: u8, here: [Fx; 3], hp: Hp| {
        hp.is_alive()
            && crate::defend::enemy(seat, owner)
            && Sq::between(here, at) <= limit
            && world.seen_by(seat.raw(), here)
    };
    let units = world.units();
    let structures = world.structures();
    let beacons = world.beacons();
    units
        .seats()
        .iter()
        .zip(units.positions())
        .zip(units.hit_points())
        .any(|((owner, here), hp)| near(*owner, *here, *hp))
        || structures
            .seats()
            .iter()
            .zip(structures.positions())
            .zip(structures.hit_points())
            .any(|((owner, here), hp)| near(*owner, *here, *hp))
        || beacons
            .seats()
            .iter()
            .zip(beacons.positions())
            .zip(beacons.hit_points())
            .any(|((owner, here), hp)| near(*owner, *here, *hp))
}

/// Where the launched force walks: its weak point, else its target.
fn destination_of(world: &World, row: &AttackRow) -> [Fx; 3] {
    // E7: rule: no live weak point this tick is the target's own place
    asset_at(world, row.mark).unwrap_or(row.aim_at)
}

/// The column a point stands on.
fn node_of_point(surface: &Surface, at: [Fx; 3]) -> Option<Node> {
    let [x, y, _] = at;
    surface.node_of(x.floor_voxels(), y.floor_voxels())
}

/// The sealed way, or the breach that opens it (see the module docs).
fn seal_or_breach(
    world: &mut World,
    row: &mut AttackRow,
    post: &Post,
    lead: Option<[Fx; 3]>,
    destination: [Fx; 3],
) {
    let (Some(from), Some(to)) = (
        lead.and_then(|at| node_of_point(world.surface(), at)),
        node_of_point(world.surface(), destination),
    ) else {
        return;
    };
    if world.clusters().connected(world.surface(), from, to) {
        // The way is open: a breach in hand is over, whatever opened it. A
        // planted charge still fires; it is in the world.
        row.breach = None;
        return;
    }
    if !row.settings.breach {
        end(world, row, post, Ended::Sealed);
        return;
    }
    let Some(breach) = row.breach else {
        let mut scratch = world.take_breach_scratch();
        let found = breach_search(world, &mut scratch, post.seat, from, to);
        world.put_breach_scratch(scratch);
        match found {
            Ok(breach) => row.breach = Some(breach),
            // A way that crosses no wall is a way a walker takes: the
            // oracle has not caught up with an edit yet (it is repaired in
            // the next Pathing phase), so the force walks on.
            Err(BreachRefused::NoWall) => world.count_breach_refused(BreachRefused::NoWall),
            Err(refused) => {
                world.count_breach_refused(refused);
                end(world, row, post, Ended::Sealed);
            }
        }
        return;
    };
    if !wall_stands(world, breach.wall) {
        // The wall at the breach is gone: the next tick searches again if the
        // way is still sealed.
        row.breach = None;
        return;
    }
    match breach.charge {
        ChargeState::None => {
            // The charge fired and the wall at the breach still stands: the
            // cut could not reach it, so no charge will, and the way stays
            // sealed.
            end(world, row, post, Ended::Sealed);
            return;
        }
        ChargeState::Loaded | ChargeState::Wanted | ChargeState::Planted => {}
    }
    row.breach = Some(breach);
}

/// A carrier killed before it plants drops a charge that never fires
/// (decision 9): announced, and the breach wants another. Run first in a
/// launched force's tick, so a carrier that was the last of its force is
/// still announced before the force is found lost.
fn lose_dead_carriers_charge(world: &mut World, row: &mut AttackRow, post: &Post) {
    let Some(breach) = row.breach.as_mut() else {
        return;
    };
    if breach.charge != ChargeState::Loaded || alive(world, breach.carrier_asset()) {
        return;
    }
    let plant = voxel_centre(breach.plant);
    breach.charge = ChargeState::Wanted;
    breach.carrier = NO_CARRIER;
    if let Ok(subject) = AssetId::of_beacon(row.beacon) {
        world.emit(
            world.tick(),
            Emission::of(EventKind::ChargeLost)
                .seat(post.seat)
                .subject(subject)
                .at(Position::from_array(plant)),
        );
    }
}

impl Breach {
    /// The carrier as an [`AssetId`] raw value, or [`NO_TARGET`].
    #[must_use]
    pub fn carrier_asset(self) -> u32 {
        if self.carrier == NO_CARRIER {
            return NO_TARGET;
        }
        AssetId::of_unit(UnitId::new(self.carrier)).map_or(NO_TARGET, AssetId::raw)
    }
}

/// Whether the voxel `at` is still a wall voxel.
fn wall_stands(world: &World, at: [i32; 3]) -> bool {
    world
        .voxels()
        .get(at)
        .is_some_and(|material| material.wall_of().is_some())
}

/// A raider's tick under an Attack beacon: `false` when its home is no Attack
/// beacon, so its program does what a raider does elsewhere.
pub(crate) fn run_raider(world: &mut World, unit: u32) -> bool {
    let index = crate::math::index::row(unit);
    let Some(home) = world.units().homes().get(index).copied() else {
        return false;
    };
    let Some(row) = world.attack().row_of(home).copied() else {
        return false;
    };
    let Some(post) = post_of(world, &row) else {
        crate::programs::idle_at_home(world, unit);
        return true;
    };
    let Some(here) = position_of(world, unit) else {
        return true;
    };
    let radius = world.arrive_radius();
    match row.phase {
        Phase::Gathering => {
            if crate::programs::within(here, post.staging, radius) {
                world.park_unit(unit);
            } else {
                crate::defend::steer(world, unit, post.staging);
            }
        }
        Phase::Returning => crate::programs::idle_at_home(world, unit),
        Phase::Launched => {
            if let Some(breach) = row.breach {
                // The carrier walks to the wall and plants; the rest of the
                // force walks on as far as the seal lets it, so it is at the
                // gap when the charge opens it.
                let me = Row::of_index(index)
                    .and_then(|at| world.units().id_of(at))
                    .map_or(NO_CARRIER, UnitId::raw);
                if breach.charge == ChargeState::Loaded && breach.carrier == me {
                    let plant = voxel_centre([
                        breach.plant[0],
                        breach.plant[1],
                        breach.plant[2].saturating_add(1),
                    ]);
                    if crate::programs::within(here, plant, radius) {
                        world.park_unit(unit);
                        plant_charge(world, &row, &post, breach, me);
                    } else {
                        crate::defend::steer(world, unit, plant);
                    }
                    return true;
                }
            }
            let destination = destination_of(world, &row);
            // A raider stops where it can fire: at its target or weak point in
            // range, or, raiding a voxel, at any enemy the seat sees in reach.
            let at_target = (row.mark != NO_TARGET || row.target != NO_TARGET)
                && world
                    .rules()
                    .unit(UnitKind::Raider)
                    .weapon
                    .map(crate::arms::WeaponRow::range)
                    .is_some_and(|range| crate::programs::within(here, destination, range));
            let fighting = at_target
                || (matches!(row.settings.aim, Aim::Voxel(_))
                    && enemy_in_reach(world, post.seat, here));
            if fighting || crate::programs::within(here, destination, radius) {
                world.park_unit(unit);
            } else {
                crate::defend::steer(world, unit, destination);
            }
        }
    }
    true
}

/// Plant a breach's charge where its carrier stands (decision 9).
fn plant_charge(world: &mut World, row: &AttackRow, post: &Post, breach: Breach, carrier: u32) {
    let [x, y, _] = breach.wall;
    let fuse = world.rules().attack().charge_fuse.to_ticks_ceil();
    let tick = world.tick();
    // E7: cannot miss: a fuse of at most `i32::MAX` ms after a tick of a segment is far below `u32::MAX` ticks
    let fires_at = tick.raw().saturating_add(fuse);
    world.attack_mut().plant(Charge {
        beacon: row.beacon,
        seat: post.seat,
        centre: [x, y, breach.footing],
        fires_at,
    });
    if let Some(slot) = world.attack_mut().row_mut(row.beacon)
        && let Some(held) = slot.breach.as_mut()
    {
        held.charge = ChargeState::Planted;
    }
    if let Ok(subject) = AssetId::of_unit(UnitId::new(carrier)) {
        world.emit(
            tick,
            Emission::of(EventKind::ChargePlanted)
                .seat(post.seat)
                .subject(subject)
                .at(Position::from_array(voxel_centre(breach.plant)))
                .value(i64::from(fires_at)),
        );
    }
}

/// Load a bought charge on its carrier: the force's lowest-row living raider
/// (decision 9). `false` when the beacon has no breach that wants one or no
/// raider to carry it, and then nothing is charged.
pub(crate) fn load_charge(world: &mut World, beacon: BeaconId) -> bool {
    let Some(carrier) = lead_of(world, beacon)
        .and_then(|unit| Row::of_index(crate::math::index::row(unit)))
        .and_then(|at| world.units().id_of(at))
    else {
        return false;
    };
    let Some(row) = world.attack_mut().row_mut(beacon) else {
        return false;
    };
    let Some(breach) = row.breach.as_mut() else {
        return false;
    };
    if breach.charge != ChargeState::Wanted {
        return false;
    }
    breach.charge = ChargeState::Loaded;
    breach.carrier = carrier.raw();
    true
}

/// Whether the beacon's breach wants a charge bought: the Attack mandate's
/// request.
#[must_use]
pub(crate) fn wants_charge(world: &World, beacon: BeaconId) -> bool {
    world
        .attack()
        .row_of(beacon)
        .and_then(|row| row.breach)
        .is_some_and(|breach| breach.charge == ChargeState::Wanted)
}

/// The breach point a mortar of `seat` at `from` with `range` is asked to
/// shell (decision 41): the top wall voxel of the first breach in ascending
/// beacon order that lies in range, still stands and the seat sees.
#[must_use]
pub(crate) fn breach_aim(world: &World, seat: u8, from: [Fx; 3], range: Fx) -> Option<[Fx; 3]> {
    let limit = Sq::of_radius(range);
    for row in world.attack().rows() {
        let Some(breach) = row.breach else {
            continue;
        };
        let owner = world
            .beacons()
            .row_of(row.beacon)
            .ok()
            .and_then(|at| world.beacons().seats().get(at.index()).copied());
        if owner != Some(seat) || !wall_stands(world, breach.wall) {
            continue;
        }
        let aim = voxel_centre(breach.wall);
        if Sq::between(from, aim) <= limit && world.seen_by(seat, aim) {
            return Some(aim);
        }
    }
    None
}

/// The target a raider of the Attack beacon `home` fires at first, when it is
/// in `range` of `from` and the seat sees it: a hunted commander, else the
/// weak point (decision 14). The combat phase's first choice for an Attack
/// raider.
#[must_use]
pub(crate) fn raid_target(
    world: &World,
    home: BeaconId,
    from: [Fx; 3],
    range: Fx,
) -> Option<(AssetId, [Fx; 3])> {
    let row = world.attack().row_of(home)?;
    if row.phase != Phase::Launched {
        return None;
    }
    let seat = world
        .beacons()
        .seats()
        .get(world.beacons().row_of(home).ok()?.index())
        .copied()?;
    for asset in [row.target, row.mark] {
        if !alive(world, asset) {
            continue;
        }
        let Some(at) = asset_at(world, asset) else {
            continue;
        };
        if crate::programs::within(from, at, range) && world.seen_by(seat, at) {
            return Some((AssetId::new(asset), at));
        }
    }
    None
}

/// Why a breach search gave no breach, counted under [`AttackCounts`] (E4).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BreachRefused {
    /// It spent [`BREACH_EXPANSIONS`] without reaching the goal.
    Exhausted,
    /// It reached the goal without crossing a wall: the seal is not a wall's.
    NoWall,
    /// No way exists even through walls.
    NoWay,
}

/// The breach search's own buffers: the per-column stamps, costs and
/// predecessors, and the heap, reserved once with the world so a search
/// allocates nothing.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct BreachScratch {
    epoch: u32,
    stamp: Vec<u32>,
    closed: Vec<u32>,
    g: Vec<i32>,
    came: Vec<u32>,
    heap: crate::pathing::search::Heap,
}

impl BreachScratch {
    /// Buffers for a graph of `nodes` columns.
    #[must_use]
    pub fn new(nodes: u32) -> BreachScratch {
        let n = crate::math::index::row(nodes);
        BreachScratch {
            epoch: 0,
            stamp: vec![0; n],
            closed: vec![0; n],
            g: vec![0; n],
            came: vec![0; n],
            heap: crate::pathing::search::Heap::with_capacity(BREACH_HEAP),
        }
    }

    fn begin(&mut self) {
        if self.epoch == u32::MAX {
            self.stamp.fill(0);
            self.closed.fill(0);
            self.epoch = 0;
        }
        self.epoch = self.epoch.saturating_add(1);
        self.heap.clear();
    }

    fn slot(node: Node) -> Option<usize> {
        usize::try_from(node).ok()
    }

    fn seen(&self, node: Node) -> bool {
        Self::slot(node)
            .and_then(|at| self.stamp.get(at).copied())
            .is_some_and(|stamp| stamp == self.epoch)
    }

    fn is_closed(&self, node: Node) -> bool {
        Self::slot(node)
            .and_then(|at| self.closed.get(at).copied())
            .is_some_and(|stamp| stamp == self.epoch)
    }

    fn close(&mut self, node: Node) {
        if let Some(slot) = Self::slot(node).and_then(|at| self.closed.get_mut(at)) {
            *slot = self.epoch;
        }
    }

    fn cost(&self, node: Node) -> Option<i32> {
        if !self.seen(node) {
            return None;
        }
        Self::slot(node).and_then(|at| self.g.get(at).copied())
    }

    fn reach(&mut self, node: Node, cost: i32, from: Node) {
        let Some(at) = Self::slot(node) else {
            return;
        };
        if let Some(slot) = self.stamp.get_mut(at) {
            *slot = self.epoch;
        }
        if let Some(slot) = self.g.get_mut(at) {
            *slot = cost;
        }
        if let Some(slot) = self.came.get_mut(at) {
            *slot = from;
        }
    }

    fn came_from(&self, node: Node) -> Option<Node> {
        Self::slot(node).and_then(|at| self.came.get(at).copied())
    }
}

/// A column as the breach search reads it: the top a walker stands on there,
/// and, for a breachable wall column, its footing. A wall column of another
/// seat, or a neutral one, stands on its footing; the seat's own wall is no
/// breach (spec section 9's friendly-fire avoidance) and stands on its top.
fn breach_column(world: &World, seat: SeatId, node: Node) -> Option<(i32, Option<i32>)> {
    let surface = world.surface();
    if !surface.walkable(node) {
        return None;
    }
    let top = surface.top(node);
    let (x, y) = surface.coord_of(node);
    let material = world.voxels().get([x, y, top])?;
    if material.wall_of().is_none() || world.wall_owner([x, y, top]) == Some(seat) {
        return Some((top, None));
    }
    let mut z = top;
    while z >= 0
        && world
            .voxels()
            .get([x, y, z])
            .is_some_and(|below| below.wall_of().is_some())
    {
        z = z.saturating_sub(1);
    }
    let footing = world
        .voxels()
        .get([x, y, z])
        .filter(|ground| ground.is_solid())
        .map(|_| z)?;
    Some((footing, Some(footing)))
}

/// The breach search (E23's second cost mode): A\* over the 2.5D graph from
/// `start` to `goal`, on which a breachable wall column stands on its footing
/// and costs [`BREACH_COLUMN_COST`] on top of the step onto it. Its way's
/// first wall column is the breach, the column before it the plant point.
///
/// Bounded by [`BREACH_EXPANSIONS`] closed columns, and ordered `(f, h,
/// node)` as every search here is (item 62), so the answer is a function of
/// the world.
///
/// # Errors
///
/// [`BreachRefused`], naming why there is no breach.
#[allow(
    clippy::too_many_lines,
    reason = "one A* with its eight-neighbour legality written out beside the breach mode's two differences, then the walk back; split, the scratch and the column reads would be handed between functions as long parameter lists"
)]
pub fn breach_search(
    world: &World,
    scratch: &mut BreachScratch,
    seat: SeatId,
    start: Node,
    goal: Node,
) -> Result<Breach, BreachRefused> {
    use crate::pathing::search::Item;
    let surface = world.surface();
    let costs = surface.costs();
    scratch.begin();
    scratch.reach(start, 0, start);
    let h0 = surface.heuristic(start, goal);
    scratch.heap.push(Item {
        f: h0,
        h: h0,
        n: start,
    });
    let mut expansions: u32 = 0;
    let mut found = false;
    while let Some(item) = scratch.heap.pop() {
        if scratch.is_closed(item.n) {
            continue;
        }
        scratch.close(item.n);
        if item.n == goal {
            found = true;
            break;
        }
        expansions = expansions.saturating_add(1);
        if expansions > BREACH_EXPANSIONS {
            return Err(BreachRefused::Exhausted);
        }
        let Some((from_top, _)) = breach_column(world, seat, item.n) else {
            continue;
        };
        // E7: cannot miss: a popped column was reached when it was pushed
        let Some(here) = scratch.cost(item.n) else {
            continue;
        };
        let (x, y) = surface.coord_of(item.n);
        for (dx, dy) in NEIGHBOURS {
            let Some(next) = x
                .checked_add(dx)
                .zip(y.checked_add(dy))
                .and_then(|(nx, ny)| surface.node_of(nx, ny))
            else {
                continue;
            };
            if scratch.is_closed(next) {
                continue;
            }
            let Some((to_top, wall)) = breach_column(world, seat, next) else {
                continue;
            };
            if to_top.saturating_sub(from_top).abs() > 1 {
                continue;
            }
            if dx != 0 && dy != 0 {
                let sides = [
                    surface.node_of(x.saturating_add(dx), y),
                    surface.node_of(x, y.saturating_add(dy)),
                ];
                let clear = sides.iter().all(|side| {
                    side.and_then(|side| breach_column(world, seat, side))
                        .is_some_and(|(top, _)| {
                            top.saturating_sub(from_top).abs() <= 1
                                && top.saturating_sub(to_top).abs() <= 1
                        })
                });
                if !clear {
                    continue;
                }
            }
            let step = if dx != 0 && dy != 0 {
                costs.diagonal()
            } else {
                costs.cardinal()
            };
            let climb = if to_top == from_top { 0 } else { costs.climb() };
            let demolish = if wall.is_some() {
                BREACH_COLUMN_COST
            } else {
                0
            };
            let Some(cost) = here
                .checked_add(step)
                .and_then(|cost| cost.checked_add(climb))
                .and_then(|cost| cost.checked_add(demolish))
            else {
                continue;
            };
            if scratch.cost(next).is_some_and(|held| held <= cost) {
                continue;
            }
            if scratch.heap.len() >= BREACH_HEAP {
                return Err(BreachRefused::Exhausted);
            }
            scratch.reach(next, cost, item.n);
            let h = surface.heuristic(next, goal);
            scratch.heap.push(Item {
                f: cost.saturating_add(h),
                h,
                n: next,
            });
        }
    }
    if !found {
        return Err(BreachRefused::NoWay);
    }
    // Walk the way back; the last wall column met is the one nearest the
    // start, which is the breach.
    let mut breach: Option<Breach> = None;
    let mut current = goal;
    let mut guard: u32 = 0;
    while current != start && guard <= BREACH_EXPANSIONS {
        guard = guard.saturating_add(1);
        let Some(previous) = scratch.came_from(current) else {
            break;
        };
        if let Some((_, Some(footing))) = breach_column(world, seat, current)
            && let Some((before, None)) = breach_column(world, seat, previous)
        {
            let (x, y) = surface.coord_of(current);
            let (px, py) = surface.coord_of(previous);
            breach = Some(Breach {
                wall: [x, y, surface.top(current)],
                plant: [px, py, before],
                footing,
                charge: ChargeState::Wanted,
                carrier: NO_CARRIER,
            });
        }
        current = previous;
    }
    breach.ok_or(BreachRefused::NoWall)
}

/// The Attack table as a snapshot carries it (S2's `atk`, snapshot version
/// 14): flat columns, one entry per row, avoid area or charge, every
/// optional field behind a bit of `flags` and written as zero while unset.
#[derive(Clone, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct AttackColumns {
    /// Each Attack beacon, a [`BeaconId`] raw value, ascending.
    pub beacon: Vec<u32>,
    /// Each target's [`Aim::tag`].
    pub aim: Vec<u8>,
    /// Each target's beacon raw value, or its named seat; zero otherwise.
    pub aim_value: Vec<u32>,
    /// Each voxel target's three axes; zeroes otherwise.
    pub aim_voxel: Vec<i32>,
    /// Each retarget rule's [`Retarget::id`].
    pub retarget: Vec<u8>,
    /// Each after-action's [`AfterAction::id`].
    pub after: Vec<u8>,
    /// Each row's presence bits: 1 staging, 2 force size, 4 launch time, 8
    /// retreat percentage, 16 retreat time, 32 `breach_allowed`, 64 a breach
    /// in hand.
    pub flags: Vec<u8>,
    /// Each staging point's three axes.
    pub staging: Vec<i32>,
    /// Each launch force size.
    pub force_size: Vec<u32>,
    /// Each launch segment time.
    pub launch_ms: Vec<i32>,
    /// Each retreat percentage.
    pub retreat_pct: Vec<u8>,
    /// Each retreat time.
    pub retreat_ms: Vec<i32>,
    /// Each `waves`.
    pub waves: Vec<u32>,
    /// Each force's [`Phase::id`].
    pub phase: Vec<u8>,
    /// Each target asset.
    pub target: Vec<u32>,
    /// Each weak point.
    pub mark: Vec<u32>,
    /// Each aim point's three raw Q16.16 axes.
    pub aim_at: Vec<i32>,
    /// Each launch tick.
    pub launched_at: Vec<u32>,
    /// Each launch force.
    pub launch_force: Vec<u32>,
    /// Each launch count this segment.
    pub waves_used: Vec<u32>,
    /// Each segment those launches counted in.
    pub wave_segment: Vec<u32>,
    /// Each breach's wall voxel, plant voxel and footing: seven axes a row.
    pub breach: Vec<i32>,
    /// Each breach's [`ChargeState::id`].
    pub charge: Vec<u8>,
    /// Each breach's carrier.
    pub carrier: Vec<u32>,
    /// The avoid list's beacon, per entry.
    pub avoid_by: Vec<u32>,
    /// The avoid list's box, four axes an entry: min `x`, `y`, max `x`, `y`.
    pub avoid_box: Vec<i32>,
    /// Each planted charge's beacon.
    pub charge_beacon: Vec<u32>,
    /// Each planted charge's seat.
    pub charge_seat: Vec<u8>,
    /// Each planted charge's centre, three axes.
    pub charge_at: Vec<i32>,
    /// Each planted charge's firing tick.
    pub charge_fires: Vec<u32>,
}

/// The presence bits of [`AttackColumns::flags`].
const HAS_STAGING: u8 = 1;
const HAS_FORCE: u8 = 2;
const HAS_LAUNCH_MS: u8 = 4;
const HAS_RETREAT_PCT: u8 = 8;
const HAS_RETREAT_MS: u8 = 16;
const BREACH_ALLOWED: u8 = 32;
const HAS_BREACH: u8 = 64;

impl AttackColumns {
    /// The columns of `table`.
    #[must_use]
    pub fn of(table: &AttackTable) -> AttackColumns {
        let mut out = AttackColumns::default();
        for row in table.rows() {
            let settings = row.settings;
            out.beacon.push(row.beacon.raw());
            out.aim.push(settings.aim.tag());
            let (value, voxel) = match settings.aim {
                Aim::Unset | Aim::Commander(None) | Aim::NearestKnownBeacon => (0, [0; 3]),
                Aim::Beacon(beacon) => (beacon.raw(), [0; 3]),
                Aim::Voxel(at) => (0, widen(at)),
                Aim::Commander(Some(seat)) => (u32::from(seat), [0; 3]),
            };
            out.aim_value.push(value);
            out.aim_voxel.extend(voxel);
            out.retarget.push(settings.retarget.id());
            out.after.push(settings.after.id());
            let mut flags = 0_u8;
            let bit = |present: bool, flag: u8| if present { flag } else { 0 };
            flags |= bit(settings.staging.is_some(), HAS_STAGING);
            flags |= bit(settings.force_size.is_some(), HAS_FORCE);
            flags |= bit(settings.launch_ms.is_some(), HAS_LAUNCH_MS);
            flags |= bit(settings.retreat_pct.is_some(), HAS_RETREAT_PCT);
            flags |= bit(settings.retreat_ms.is_some(), HAS_RETREAT_MS);
            flags |= bit(settings.breach, BREACH_ALLOWED);
            flags |= bit(row.breach.is_some(), HAS_BREACH);
            out.flags.push(flags);
            out.staging.extend(settings.staging.map_or([0; 3], widen));
            out.force_size.push(settings.force_size.unwrap_or(0));
            out.launch_ms.push(settings.launch_ms.unwrap_or(0));
            out.retreat_pct.push(settings.retreat_pct.unwrap_or(0));
            out.retreat_ms.push(settings.retreat_ms.unwrap_or(0));
            out.waves.push(settings.waves);
            out.phase.push(row.phase.id());
            out.target.push(row.target);
            out.mark.push(row.mark);
            out.aim_at.extend(row.aim_at.map(Fx::raw));
            out.launched_at.push(row.launched_at);
            out.launch_force.push(row.launch_force);
            out.waves_used.push(row.waves_used);
            out.wave_segment.push(row.wave_segment);
            let (axes, charge, carrier) = row.breach.map_or(([0; 7], 0, 0), |breach| {
                let [wx, wy, wz] = breach.wall;
                let [px, py, pz] = breach.plant;
                (
                    [wx, wy, wz, px, py, pz, breach.footing],
                    breach.charge.id(),
                    breach.carrier,
                )
            });
            out.breach.extend(axes);
            out.charge.push(charge);
            out.carrier.push(carrier);
        }
        for (by, area) in table.avoid() {
            out.avoid_by.push(by.raw());
            let [lx, ly] = area.min_column();
            let [hx, hy] = area.max_column();
            out.avoid_box.extend([lx, ly, hx, hy].map(i32::from));
        }
        for charge in table.charges() {
            out.charge_beacon.push(charge.beacon.raw());
            out.charge_seat.push(charge.seat.raw());
            out.charge_at.extend(charge.centre);
            out.charge_fires.push(charge.fires_at);
        }
        out
    }

    /// The table these columns describe, checked before anything is written:
    /// the columns are one length a row (three, four or seven axes where a
    /// row carries a point), every row names a live beacon on the Attack
    /// writ, ascending, and every live Attack beacon holds one; every byte is
    /// one this build names, an unset field is written as zero, a voxel is a
    /// coordinate, a percentage is 1 to 100, an avoid area is the right way
    /// out and belongs to a row, and a charge names a seat of the file.
    /// `live_on` answers whether a beacon raw value names a live beacon on
    /// the Attack writ, and `attack_beacons` every such beacon, ascending.
    ///
    /// # Errors
    ///
    /// `Err(column)`, naming the column a check refuses.
    pub fn restore(
        &self,
        live_on: impl Fn(u32) -> bool,
        attack_beacons: &[u32],
        seats: usize,
    ) -> Result<AttackTable, &'static str> {
        let n = self.beacon.len();
        let ragged = [
            (self.aim.len(), 1, "attack_aim"),
            (self.aim_value.len(), 1, "attack_aim_value"),
            (self.aim_voxel.len(), 3, "attack_aim_voxel"),
            (self.retarget.len(), 1, "attack_retarget"),
            (self.after.len(), 1, "attack_after"),
            (self.flags.len(), 1, "attack_flags"),
            (self.staging.len(), 3, "attack_staging"),
            (self.force_size.len(), 1, "attack_force_size"),
            (self.launch_ms.len(), 1, "attack_launch_ms"),
            (self.retreat_pct.len(), 1, "attack_retreat_pct"),
            (self.retreat_ms.len(), 1, "attack_retreat_ms"),
            (self.waves.len(), 1, "attack_waves"),
            (self.phase.len(), 1, "attack_phase"),
            (self.target.len(), 1, "attack_target"),
            (self.mark.len(), 1, "attack_mark"),
            (self.aim_at.len(), 3, "attack_aim_at"),
            (self.launched_at.len(), 1, "attack_launched_at"),
            (self.launch_force.len(), 1, "attack_launch_force"),
            (self.waves_used.len(), 1, "attack_waves_used"),
            (self.wave_segment.len(), 1, "attack_wave_segment"),
            (self.breach.len(), 7, "attack_breach"),
            (self.charge.len(), 1, "attack_charge"),
            (self.carrier.len(), 1, "attack_carrier"),
        ];
        for (len, per, column) in ragged {
            if Some(len) != n.checked_mul(per) {
                return Err(column);
            }
        }
        let mut rows: Vec<AttackRow> = Vec::with_capacity(n);
        for at in 0..n {
            rows.push(self.row(at, &live_on)?);
        }
        let listed: Vec<u32> = rows.iter().map(|row| row.beacon.raw()).collect();
        if listed != attack_beacons {
            return Err("attack_beacon");
        }
        let avoid = self.avoid_list()?;
        let charges = self.charge_list(seats)?;
        let mut table = AttackTable::with_room(u32::try_from(n).map_err(|_| "attack_beacon")?);
        if !table.restore(rows, avoid, charges) {
            return Err("attack_beacon");
        }
        Ok(table)
    }

    /// Row `at`, checked.
    #[allow(
        clippy::too_many_lines,
        reason = "one check per column of the row, in declared order, so a reader can hold the restore against `AttackColumns::of` line by line"
    )]
    fn row(&self, at: usize, live_on: &impl Fn(u32) -> bool) -> Result<AttackRow, &'static str> {
        let one = |column: &[u8], name: &'static str| column.get(at).copied().ok_or(name);
        let word = |column: &[u32], name: &'static str| column.get(at).copied().ok_or(name);
        let signed = |column: &[i32], name: &'static str| column.get(at).copied().ok_or(name);
        let axes =
            |column: &[i32], per: usize, name: &'static str| -> Result<Vec<i32>, &'static str> {
                let start = at.checked_mul(per).ok_or(name)?;
                let end = start.checked_add(per).ok_or(name)?;
                column.get(start..end).map(<[i32]>::to_vec).ok_or(name)
            };
        let voxel = |values: &[i32], name: &'static str| -> Result<[i16; 3], &'static str> {
            let mut out = [0_i16; 3];
            for (slot, value) in out.iter_mut().zip(values) {
                *slot = i16::try_from(*value).map_err(|_| name)?;
            }
            Ok(out)
        };
        let beacon = word(&self.beacon, "attack_beacon")?;
        if !live_on(beacon) {
            return Err("attack_beacon");
        }
        let flags = one(&self.flags, "attack_flags")?;
        if flags
            & !(HAS_STAGING
                | HAS_FORCE
                | HAS_LAUNCH_MS
                | HAS_RETREAT_PCT
                | HAS_RETREAT_MS
                | BREACH_ALLOWED
                | HAS_BREACH)
            != 0
        {
            return Err("attack_flags");
        }
        let value = word(&self.aim_value, "attack_aim_value")?;
        let aim_voxel = axes(&self.aim_voxel, 3, "attack_aim_voxel")?;
        let zero_voxel = aim_voxel.iter().all(|axis| *axis == 0);
        let aim = match one(&self.aim, "attack_aim")? {
            0 if value == 0 && zero_voxel => Aim::Unset,
            1 if zero_voxel => Aim::Beacon(BeaconId::new(value)),
            2 if value == 0 => Aim::Voxel(voxel(&aim_voxel, "attack_aim_voxel")?),
            3 if value == 0 && zero_voxel => Aim::Commander(None),
            4 if zero_voxel => {
                Aim::Commander(Some(u8::try_from(value).map_err(|_| "attack_aim_value")?))
            }
            5 if value == 0 && zero_voxel => Aim::NearestKnownBeacon,
            _ => return Err("attack_aim"),
        };
        // An unset optional is written as zero: anything else is an edited
        // file.
        let optional =
            |present: bool, zero: bool, name: &'static str| -> Result<bool, &'static str> {
                if !present && !zero {
                    return Err(name);
                }
                Ok(present)
            };
        let staging_axes = axes(&self.staging, 3, "attack_staging")?;
        let staging = if optional(
            flags & HAS_STAGING != 0,
            staging_axes.iter().all(|axis| *axis == 0),
            "attack_staging",
        )? {
            Some(voxel(&staging_axes, "attack_staging")?)
        } else {
            None
        };
        let force = word(&self.force_size, "attack_force_size")?;
        let force_size =
            optional(flags & HAS_FORCE != 0, force == 0, "attack_force_size")?.then_some(force);
        let launch = signed(&self.launch_ms, "attack_launch_ms")?;
        let launch_ms = optional(flags & HAS_LAUNCH_MS != 0, launch == 0, "attack_launch_ms")?
            .then_some(launch);
        if launch_ms.is_some_and(|ms| ms < 0) {
            return Err("attack_launch_ms");
        }
        let pct = one(&self.retreat_pct, "attack_retreat_pct")?;
        let retreat_pct =
            optional(flags & HAS_RETREAT_PCT != 0, pct == 0, "attack_retreat_pct")?.then_some(pct);
        if retreat_pct.is_some_and(|pct| !(1..=100).contains(&pct)) {
            return Err("attack_retreat_pct");
        }
        let after_ms = signed(&self.retreat_ms, "attack_retreat_ms")?;
        let retreat_ms = optional(
            flags & HAS_RETREAT_MS != 0,
            after_ms == 0,
            "attack_retreat_ms",
        )?
        .then_some(after_ms);
        if retreat_ms.is_some_and(|ms| ms < 0) {
            return Err("attack_retreat_ms");
        }
        let settings = AttackSettings {
            aim,
            retarget: Retarget::from_id(one(&self.retarget, "attack_retarget")?)
                .ok_or("attack_retarget")?,
            staging,
            force_size,
            launch_ms,
            retreat_pct,
            retreat_ms,
            after: AfterAction::from_id(one(&self.after, "attack_after")?).ok_or("attack_after")?,
            breach: flags & BREACH_ALLOWED != 0,
            waves: word(&self.waves, "attack_waves")?,
        };
        let aim_at_axes = axes(&self.aim_at, 3, "attack_aim_at")?;
        let mut aim_at = [Fx::from_raw(0); 3];
        for (slot, value) in aim_at.iter_mut().zip(&aim_at_axes) {
            *slot = Fx::from_raw(*value);
        }
        let breach_axes = axes(&self.breach, 7, "attack_breach")?;
        let charge = one(&self.charge, "attack_charge")?;
        let carrier = word(&self.carrier, "attack_carrier")?;
        let breach = if flags & HAS_BREACH != 0 {
            let [wx, wy, wz, px, py, pz, footing] =
                <[i32; 7]>::try_from(breach_axes.as_slice()).map_err(|_| "attack_breach")?;
            let charge = ChargeState::from_id(charge).ok_or("attack_charge")?;
            if charge == ChargeState::Loaded && carrier == NO_CARRIER {
                return Err("attack_carrier");
            }
            Some(Breach {
                wall: [wx, wy, wz],
                plant: [px, py, pz],
                footing,
                charge,
                carrier,
            })
        } else {
            if breach_axes.iter().any(|axis| *axis != 0) || charge != 0 || carrier != 0 {
                return Err("attack_breach");
            }
            None
        };
        let phase = Phase::from_id(one(&self.phase, "attack_phase")?).ok_or("attack_phase")?;
        Ok(AttackRow {
            beacon: BeaconId::new(beacon),
            settings,
            phase,
            target: word(&self.target, "attack_target")?,
            mark: word(&self.mark, "attack_mark")?,
            aim_at,
            launched_at: word(&self.launched_at, "attack_launched_at")?,
            launch_force: word(&self.launch_force, "attack_launch_force")?,
            waves_used: word(&self.waves_used, "attack_waves_used")?,
            wave_segment: word(&self.wave_segment, "attack_wave_segment")?,
            breach,
        })
    }

    /// The avoid list, checked: one box of four axes per entry, each the
    /// right way out.
    fn avoid_list(&self) -> Result<Vec<(BeaconId, ColumnBox)>, &'static str> {
        if Some(self.avoid_box.len()) != self.avoid_by.len().checked_mul(4) {
            return Err("attack_avoid_box");
        }
        let mut out = Vec::with_capacity(self.avoid_by.len());
        for (by, axes) in self.avoid_by.iter().zip(self.avoid_box.as_chunks::<4>().0) {
            let mut values = [0_i16; 4];
            for (slot, value) in values.iter_mut().zip(axes) {
                *slot = i16::try_from(*value).map_err(|_| "attack_avoid_box")?;
            }
            let [lx, ly, hx, hy] = values;
            let area = ColumnBox::new([lx, ly], [hx, hy]).ok_or("attack_avoid_box")?;
            out.push((BeaconId::new(*by), area));
        }
        Ok(out)
    }

    /// The planted charges, checked: one centre of three axes each, and a
    /// seat of the file.
    fn charge_list(&self, seats: usize) -> Result<Vec<Charge>, &'static str> {
        let n = self.charge_beacon.len();
        if self.charge_seat.len() != n
            || self.charge_fires.len() != n
            || Some(self.charge_at.len()) != n.checked_mul(3)
        {
            return Err("attack_charge_at");
        }
        let mut out = Vec::with_capacity(n);
        let rows = self
            .charge_beacon
            .iter()
            .zip(&self.charge_seat)
            .zip(&self.charge_fires)
            .zip(self.charge_at.as_chunks::<3>().0);
        for (((beacon, seat), fires), centre) in rows {
            if usize::from(*seat) >= seats {
                return Err("attack_charge_seat");
            }
            let centre = *centre;
            out.push(Charge {
                beacon: BeaconId::new(*beacon),
                seat: SeatId::new(*seat),
                centre,
                fires_at: *fires,
            });
        }
        Ok(out)
    }
}
