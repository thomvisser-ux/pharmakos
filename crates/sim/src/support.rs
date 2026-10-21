// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The support rule: what falls when a voxel's last footing is cut (S2's plan,
//! task `fall`; decisions-log items 138 (3) and 144 (1) to (3); decisions 11,
//! 37 and 39).
//!
//! # The rule
//!
//! A solid voxel is **supported** when a face-connected path of solid voxels
//! joins it to the bedrock layer or to a core's bore ([`crate::bore`]), which
//! is never detachable. A face-connected set of solid voxels with no such path
//! is a **detached region**, and it falls, whatever detached it: rigidly,
//! straight down, until it rests on something. Everything standing on it falls
//! with it and takes the rules table's flat `collapse.fall_damage_hp` once
//! (item 144 (1)); an asset any of whose cells a region comes to rest in takes
//! the same and stands on the first air above it (decision 11). The damage's
//! kill credit goes to the seats whose edits this tick removed a voxel next to
//! the region, split equally among them (item 144 (2), decision 37).
//!
//! # One interface, so physics can replace it
//!
//! The Voxels phase calls the rule through [`Collapse`]: given this tick's
//! changed voxels, find the regions that fall, drop them, and answer where
//! each went and what stood on it. Item 138 (3)'s physics collapse replaces
//! the implementation, [`SupportRule`], and not its caller.
//!
//! # Incremental, totally ordered, bounded
//!
//! * **Seeds.** Only voxels that might have lost support are examined: the
//!   solid face-neighbours of every voxel an edit turned to air this tick, and
//!   every voxel an edit wrote solid into air (a placed voxel is supported only
//!   if it touches support), deduplicated and sorted by voxel index (the chunk
//!   store's linear index, chunk-major: `chunk * 32768 + x + 32 y + 1024 z`).
//!   The lists are sized from the edit queue's worst case (E5): every edit the
//!   queue can hold, each a ball of the largest crater a weapon files. A tick
//!   past that, which only a host's oversized test crater can reach, is
//!   [`CollapseError::Room`], never a silent drop.
//! * **Searches.** Each seed not yet labelled starts a breadth-first search
//!   over solid face-neighbours, enumerated `-z, +x, -x, +y, -y, +z`. One
//!   visited set and one "supported" label per voxel serve the whole tick, so
//!   no voxel is searched twice in a tick. A search ends **supported** the
//!   moment it reaches a voxel whose own column is solid from the bottom up to
//!   it (a per-column height, read once a tick), a bore voxel, or a voxel
//!   already labelled supported; it ends **detached** when its frontier
//!   empties. A detached region is identified by its lowest voxel index.
//! * **Bounds** (decision 11). A search that would visit more than
//!   `collapse.max_region_voxels` voxels is labelled supported: a mass that
//!   large is wedged into the ground around it. The tick's searches together
//!   visit at most `collapse.max_visits_per_tick`; once that is spent every
//!   seed not yet labelled is labelled supported, counted under
//!   [`CollapseSkip::TickBudget`], and nothing is carried into the next tick,
//!   so the rule holds no state between ticks.
//! * **The fall, iterated.** Regions fall in ascending order of their lowest
//!   voxel's `z`, then of their identifying index. Each drops by the smallest
//!   air gap under any of its voxels, measured against the drops already made
//!   this tick, and the pass repeats, in the order recomputed from the
//!   regions' current lowest voxels, until a pass moves nothing: two regions
//!   can interlock, so the first to fall can come to rest on one that then
//!   falls away. A region that moves comes to rest on something on which, at
//!   that moment, nothing of it rests, so no two regions hold each other up,
//!   and each pass that moves anything lowers some region by a voxel at least:
//!   the passes are bounded by the regions times the map's height, and one
//!   past that bound is [`CollapseError::PassBound`], a typed error the tick
//!   returns, never a silent stop. Every voxel leaves and returns through
//!   [`VoxelStore::set`], which marks its chunk (AGENTS.md section 4.8).
//!
//! # Mine never undermines (item 144 (3))
//!
//! [`World::dig_keeps_structures_standing`] runs the same bounded search
//! read-only, over the world as the edits already queued this tick will leave
//! it, with the dig's voxel as air, and refuses a dig that would detach a
//! region carrying a seat's structure or beacon. The queue at the moment a dig
//! is chosen and filed holds exactly the edits the Voxels phase will apply
//! before it, so the check at filing is the check at application for every
//! edit queued before the dig. That is the plan's apply-time re-check moved to
//! the filing, a correction this lane made: at application the phase could
//! not tell which drone had been paid for the dig. An edit filed **after** the
//! dig on the same tick (a shell landing in Combat) is not seen, so a dig and
//! a later crater can still drop a structure together; the fall is then
//! credited to both seats, as decision 37 says.
//!
//! The check is a pure read: it builds its own scratch per call, and only on
//! the slow path, when a face of the dig is not grounded by its own column. A
//! dig into ordinary ground allocates nothing.
//!
//! # Allocations
//!
//! None in the Voxels phase: the two label sets are a bit per voxel of the
//! map, and every list is sized at construction from the rules rows, the
//! table rooms and the edit queue.
//!
//! # A note for the roadmap
//!
//! The owner's roadmap entry "some materials like dirt to be displaceable but
//! not destroyable" is not built. Two places would have to change with it:
//! the removal hook in [`VoxelStore::blast`] reports only a voxel the blast
//! turned to air (`next.is_air()`), so a voxel displaced rather than removed
//! needs its own report; and [`Thought`] repeats the blast's ball, bore and
//! `after_blast` test to see the queued edits, so any new blast outcome must
//! be mirrored there (one shared predicate in `voxels.rs` would end the copy).
//! A displaced voxel that lands in air is already a seed, as every solid
//! write into air is.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::bore::Bore;
use crate::events::{Emission, EventKind};
use crate::ids::Row;
use crate::knowledge::{AssetId, Position};
use crate::mapgen::MapError;
use crate::math::fixed::Fx;
use crate::math::quantity::Damage;
use crate::rules::RulesTable;
use crate::tables::{SeatId, StructureKind, UnitKind, WreckId};
use crate::voxels::{
    BlastLevels, Blasted, CHUNK_MASK, CHUNK_SHIFT, Material, VoxelEdit, VoxelStore,
};
use crate::world::{DamageOrder, DamageTarget, World};

/// The face-neighbour order every search enumerates: `-z, +x, -x, +y, -y, +z`.
const NEIGHBOURS: [[i32; 3]; 6] = [
    [0, 0, -1],
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, 1],
];

/// `log2` of the voxels in a chunk: a voxel index is `chunk << 15 | inside`.
const CHUNK_BITS: u32 = CHUNK_SHIFT * 3;

/// The most seats the credit's seat set holds: one bit a seat.
const MAX_SEATS: u32 = 64;

/// The seed mark of a voxel written solid into air: it seeds a search, and
/// cuts nothing, so it adds no seat to a region's cutters. Above every seat a
/// world can hold ([`MAX_SEATS`]) and below [`SeatId::NEUTRAL`].
const PLACED: u8 = 254;

/// Why a seed was labelled supported without its search having found ground.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum CollapseSkip {
    /// Its search reached more than `collapse.max_region_voxels` voxels: a
    /// mass that large is wedged into the ground around it (decision 11).
    RegionBound,
    /// The tick's `collapse.max_visits_per_tick` was spent before its search
    /// could finish, or before it could start.
    TickBudget,
}

impl CollapseSkip {
    /// Every reason.
    pub const ALL: [CollapseSkip; 2] = [CollapseSkip::RegionBound, CollapseSkip::TickBudget];

    /// The reason's name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            CollapseSkip::RegionBound => "region_bound",
            CollapseSkip::TickBudget => "tick_budget",
        }
    }

    const fn slot(self) -> usize {
        match self {
            CollapseSkip::RegionBound => 0,
            CollapseSkip::TickBudget => 1,
        }
    }
}

/// Why the support rule refused a Mine dig (item 144 (3)).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum DigRefused {
    /// The dig would detach a region carrying a seat's structure or beacon.
    WouldUndermine,
    /// The check could not run to its end (a voxel index named no voxel), so
    /// the dig is refused rather than taken unchecked. No legal world reaches
    /// it.
    Unchecked,
}

impl DigRefused {
    /// Every reason.
    pub const ALL: [DigRefused; 2] = [DigRefused::WouldUndermine, DigRefused::Unchecked];

    /// The reason's name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            DigRefused::WouldUndermine => "would_undermine",
            DigRefused::Unchecked => "unchecked",
        }
    }

    const fn slot(self) -> usize {
        match self {
            DigRefused::WouldUndermine => 0,
            DigRefused::Unchecked => 1,
        }
    }
}

/// A fault in the support rule, returned by the tick as
/// [`crate::world::TickError::Collapse`]. Every one is unreachable in a legal
/// world; each is typed so that none is a silent drop.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CollapseError {
    /// The fall took more passes than its bound, the regions times the map's
    /// height and one: the argument in the module docs says no world reaches
    /// it.
    PassBound {
        /// The passes the fall had taken.
        passes: u64,
        /// The bound.
        bound: u64,
    },
    /// A list sized at construction had no room: more voxels changed in one
    /// tick than every edit the queue can hold could change, which only a
    /// host's test crater larger than any weapon's reaches.
    Room {
        /// Which list.
        list: &'static str,
    },
    /// A voxel index or a position named no voxel of the map, or an asset was
    /// moved past what a position can hold.
    OffTheMap,
    /// A fall-damage order did not fit the damage queue, which the batches
    /// make unreachable: each batch is applied before the next would overfill
    /// the queue, and one asset's orders never outnumber its seats and nobody.
    DamageQueue,
}

impl core::fmt::Display for CollapseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CollapseError::PassBound { passes, bound } => write!(
                f,
                "the collapse took {passes} passes, past its bound of {bound}"
            ),
            CollapseError::Room { list } => {
                write!(f, "the collapse's {list} list had no room")
            }
            CollapseError::OffTheMap => write!(f, "the collapse named a voxel off the map"),
            CollapseError::DamageQueue => {
                write!(f, "a fall-damage order did not fit the damage queue")
            }
        }
    }
}

impl std::error::Error for CollapseError {}

/// What the support rule did since the world was built. Diagnostic: never
/// hashed, never snapshotted.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CollapseCounts {
    /// Regions that fell.
    pub regions: u64,
    /// Voxels the falls moved, a voxel once per region however far it fell.
    pub voxels: u64,
    /// Assets that fell with a region or were under one that landed.
    pub assets: u64,
    /// The passes the last tick's fall took (zero for a tick with no region).
    pub last_passes: u64,
    /// The voxels the last tick's searches visited.
    pub last_visits: usize,
    skipped: [u64; 2],
    refused: [u64; 2],
}

impl CollapseCounts {
    /// Seeds labelled supported for `reason`.
    #[must_use]
    pub fn skipped(&self, reason: CollapseSkip) -> u64 {
        self.skipped.get(reason.slot()).copied().into_iter().sum()
    }

    /// Mine dig checks that refused, for `reason`. Every evaluation counts:
    /// Mine's choice is asked on more paths than the filing (the seam choice,
    /// the mandate's work count, a drone's walk), so this is how often the
    /// check said no, not how many digs a drone gave up.
    #[must_use]
    pub fn refused(&self, reason: DigRefused) -> u64 {
        self.refused.get(reason.slot()).copied().into_iter().sum()
    }

    fn skip(&mut self, reason: CollapseSkip, count: u64) {
        if let Some(slot) = self.skipped.get_mut(reason.slot()) {
            *slot = slot.saturating_add(count);
        }
    }
}

/// The seats whose edits cut one region, as a set: a bit a seat, and nobody's
/// (a seat-less edit) beside them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Cutters {
    seats: u64,
    nobody: bool,
}

impl Cutters {
    /// Nobody alone: a commander's fall, and a region no removal cut (one an
    /// edit built into air), credit nobody.
    const NOBODY: Cutters = Cutters {
        seats: 0,
        nobody: true,
    };

    /// Add a cutter: a seat, or [`SeatId::NEUTRAL`] for nobody. A placed
    /// voxel's mark cuts nothing.
    fn add(&mut self, seat: u8) {
        if seat == PLACED {
            return;
        }
        if seat == SeatId::NEUTRAL.raw() {
            self.nobody = true;
        } else if let Some(bit) = 1_u64.checked_shl(u32::from(seat)) {
            self.seats |= bit;
        }
    }

    /// How many distinct cutters: each seat, and nobody once.
    #[must_use]
    pub const fn count(self) -> u32 {
        let nobody = if self.nobody { 1 } else { 0 };
        self.seats.count_ones().saturating_add(nobody)
    }

    /// The cutters in ascending seat order, nobody ([`SeatId::NEUTRAL`]) last.
    pub fn each(self) -> impl Iterator<Item = SeatId> {
        let seats = self.seats;
        let nobody = self.nobody;
        (0_u8..64)
            .filter(move |seat| (seats >> seat) & 1 == 1)
            .map(SeatId::new)
            .chain(nobody.then_some(SeatId::NEUTRAL))
    }
}

/// One detached region of a tick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Region {
    /// Where its voxels start in [`SupportRule::list`].
    start: usize,
    /// How many voxels it holds.
    len: u32,
    /// Its identifying voxel: the lowest voxel index it held when it was found.
    id: u32,
    /// The `z` of its lowest voxel now.
    lowest_z: i32,
    /// How far it has fallen this tick, in voxels.
    drop: i32,
    /// The seats whose edits this tick removed a voxel next to it.
    cutters: Cutters,
}

impl Region {
    const fn range(&self) -> core::ops::Range<usize> {
        self.start..self.start.saturating_add(crate::math::index::row(self.len))
    }
}

/// The labels one search reads and writes: the tick's bit sets, or a Mine
/// check's own sets.
trait Labels {
    fn visited(&self, index: u32) -> bool;
    fn visit(&mut self, index: u32);
    fn supported(&self, index: u32) -> bool;
    fn support(&mut self, index: u32);
}

/// A bit per voxel of the map.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
struct Bits(Vec<u64>);

impl Bits {
    fn get(&self, index: u32) -> bool {
        self.0
            .get(crate::math::index::row(index >> 6))
            .is_some_and(|word| (word >> (index & 63)) & 1 == 1)
    }

    fn set(&mut self, index: u32) {
        if let Some(word) = self.0.get_mut(crate::math::index::row(index >> 6)) {
            *word |= 1 << (index & 63);
        }
    }

    fn clear(&mut self, index: u32) {
        if let Some(word) = self.0.get_mut(crate::math::index::row(index >> 6)) {
            *word &= !(1 << (index & 63));
        }
    }
}

/// The tick's two bit sets.
struct TickLabels<'a> {
    visited: &'a mut Bits,
    supported: &'a mut Bits,
}

impl Labels for TickLabels<'_> {
    fn visited(&self, index: u32) -> bool {
        self.visited.get(index)
    }
    fn visit(&mut self, index: u32) {
        self.visited.set(index);
    }
    fn supported(&self, index: u32) -> bool {
        self.supported.get(index)
    }
    fn support(&mut self, index: u32) {
        self.supported.set(index);
    }
}

/// A Mine check's own sets, built per call on its slow path.
#[derive(Default)]
struct CallLabels {
    visited: BTreeSet<u32>,
    supported: BTreeSet<u32>,
}

impl Labels for CallLabels {
    fn visited(&self, index: u32) -> bool {
        self.visited.contains(&index)
    }
    fn visit(&mut self, index: u32) {
        self.visited.insert(index);
    }
    fn supported(&self, index: u32) -> bool {
        self.supported.contains(&index)
    }
    fn support(&mut self, index: u32) {
        self.supported.insert(index);
    }
}

/// How solid a voxel is to a search: the store as it is, or as the queued
/// edits will leave it.
trait Solidity {
    fn solid(&self, at: [i32; 3]) -> bool;
}

/// The store as the edits left it: what the Voxels phase searches.
struct Settled<'a>(&'a VoxelStore);

impl Solidity for Settled<'_> {
    fn solid(&self, at: [i32; 3]) -> bool {
        self.0.get(at).is_some_and(Material::is_solid)
    }
}

/// The store as the edits already queued this tick, and then one dig, will
/// leave it: what Mine's check searches (item 144 (3)).
///
/// The queue is pre-indexed once per check by the box its edits reach, so a
/// read outside it costs one comparison, not a walk of the queue.
struct Thought<'a> {
    store: &'a VoxelStore,
    queue: &'a [VoxelEdit],
    levels: BlastLevels,
    bore: &'a Bore,
    dug: [i32; 3],
    /// `[min x, min y, min z, max x, max y, max z]` of every queued edit's
    /// reach, or `None` for an empty queue.
    reach: Option<[i32; 6]>,
}

impl<'a> Thought<'a> {
    fn new(
        store: &'a VoxelStore,
        queue: &'a [VoxelEdit],
        levels: BlastLevels,
        bore: &'a Bore,
        dug: [i32; 3],
    ) -> Thought<'a> {
        let mut reach: Option<[i32; 6]> = None;
        for edit in queue {
            let (centre, radius) = match *edit {
                VoxelEdit::Set { at, .. } => (at, 0),
                VoxelEdit::Crater { centre, radius, .. } => (centre, radius.max(0)),
            };
            let low = centre.map(|axis| axis.saturating_sub(radius));
            let high = centre.map(|axis| axis.saturating_add(radius));
            let mut next = [low[0], low[1], low[2], high[0], high[1], high[2]];
            if let Some(held) = reach {
                for (slot, (axis, was)) in next.iter_mut().zip(held.iter().enumerate()) {
                    *slot = if axis < 3 {
                        (*slot).min(*was)
                    } else {
                        (*slot).max(*was)
                    };
                }
            }
            reach = Some(next);
        }
        Thought {
            store,
            queue,
            levels,
            bore,
            dug,
            reach,
        }
    }

    fn reached(&self, at: [i32; 3]) -> bool {
        let [x, y, z] = at;
        self.reach.is_some_and(|[a, b, c, d, e, f]| {
            x >= a && y >= b && z >= c && x <= d && y <= e && z <= f
        })
    }
}

impl Solidity for Thought<'_> {
    fn solid(&self, at: [i32; 3]) -> bool {
        if at == self.dug {
            return false;
        }
        let Some(mut material) = self.store.get(at) else {
            return false;
        };
        if !self.reached(at) {
            return material.is_solid();
        }
        for edit in self.queue {
            match *edit {
                VoxelEdit::Set {
                    at: written,
                    material: next,
                    ..
                } => {
                    if written == at {
                        material = next;
                    }
                }
                VoxelEdit::Crater { centre, radius, .. } => {
                    // The ball `VoxelStore::blast` walks, the bore skipped:
                    // a copy of its test, which the module docs name.
                    let [x, y, z] = at;
                    let [cx, cy, cz] = centre;
                    let (dx, dy, dz) = (
                        x.saturating_sub(cx),
                        y.saturating_sub(cy),
                        z.saturating_sub(cz),
                    );
                    let d2 = dx
                        .saturating_mul(dx)
                        .saturating_add(dy.saturating_mul(dy))
                        .saturating_add(dz.saturating_mul(dz));
                    let inside =
                        radius >= 0 && radius.checked_mul(radius).is_some_and(|r2| d2 <= r2);
                    if inside
                        && !self.bore.contains(at)
                        && let Blasted::Into(next) = material.after_blast(self.levels)
                    {
                        material = next;
                    }
                }
            }
        }
        material.is_solid()
    }
}

/// How a search ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Outcome {
    /// It reached ground, the bore or a supported voxel.
    Supported,
    /// Its frontier emptied: a detached region.
    Detached,
    /// It would have passed the region bound.
    RegionBound,
    /// It would have passed the tick's budget.
    Budget,
}

/// How many voxels of the column `(x, y)` are solid from the bottom up, read
/// through `view`.
fn column_height<V: Solidity>(view: &V, x: i32, y: i32, size_z: i32) -> i32 {
    let mut height = 0;
    while height < size_z && view.solid([x, y, height]) {
        height = height.saturating_add(1);
    }
    height
}

/// What one search needs besides its labels.
struct Walk<'a, V: Solidity> {
    view: &'a V,
    store: &'a VoxelStore,
    bore: &'a Bore,
    max_region: usize,
    /// The list's length the search may not pass: the tick's budget.
    limit: usize,
}

/// One breadth-first search from `seed`, its voxels appended to `list`.
/// Every voxel of a search that does not end detached is labelled supported.
///
/// A neighbour is tested for ground and for a supported label **before** the
/// bounds, so a search about to end supported is never counted as skipped.
fn search<V: Solidity, L: Labels>(
    walk: &Walk<'_, V>,
    labels: &mut L,
    list: &mut Vec<u32>,
    height: &mut dyn FnMut(i32, i32) -> i32,
    seed: u32,
) -> Result<Outcome, CollapseError> {
    let start = list.len();
    let outcome = walk_from(walk, labels, list, height, seed, start)?;
    if outcome != Outcome::Detached {
        for index in list.iter().skip(start) {
            labels.support(*index);
        }
    }
    Ok(outcome)
}

fn walk_from<V: Solidity, L: Labels>(
    walk: &Walk<'_, V>,
    labels: &mut L,
    list: &mut Vec<u32>,
    height: &mut dyn FnMut(i32, i32) -> i32,
    seed: u32,
    start: usize,
) -> Result<Outcome, CollapseError> {
    let seed_at = at_of(walk.store, seed).ok_or(CollapseError::OffTheMap)?;
    if list.len() >= walk.limit {
        return Ok(Outcome::Budget);
    }
    if list.len() >= list.capacity() {
        return Err(CollapseError::Room { list: "search" });
    }
    list.push(seed);
    labels.visit(seed);
    let grounded = |at: [i32; 3], height: &mut dyn FnMut(i32, i32) -> i32| {
        let [x, y, z] = at;
        z < height(x, y) || walk.bore.contains(at)
    };
    if grounded(seed_at, height) {
        return Ok(Outcome::Supported);
    }
    let mut head = start;
    while let Some(voxel) = list.get(head).copied() {
        head = head.saturating_add(1);
        let at = at_of(walk.store, voxel).ok_or(CollapseError::OffTheMap)?;
        for step in NEIGHBOURS {
            let next = add(at, step);
            let Some(index) = index_of(walk.store, next) else {
                continue;
            };
            if !walk.view.solid(next) {
                continue;
            }
            if labels.visited(index) {
                // A voxel this search reached already, or one an earlier
                // search labelled supported. An earlier search's detached
                // region cannot be reached: it would be this one.
                if labels.supported(index) {
                    return Ok(Outcome::Supported);
                }
                continue;
            }
            if grounded(next, height) {
                return Ok(Outcome::Supported);
            }
            if list.len().saturating_sub(start) >= walk.max_region {
                return Ok(Outcome::RegionBound);
            }
            if list.len() >= walk.limit {
                return Ok(Outcome::Budget);
            }
            if list.len() >= list.capacity() {
                return Err(CollapseError::Room { list: "search" });
            }
            list.push(index);
            labels.visit(index);
        }
    }
    Ok(Outcome::Detached)
}

/// The rooms the rule is built with: the rows, the map, and the lists' sizes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Room {
    max_region: u32,
    max_visits: u32,
    size: [i32; 3],
    words: usize,
    columns: usize,
    list: usize,
    changed: usize,
}

impl Room {
    /// The rooms for a map of `size` voxels under the rules table's
    /// `collapse` rows, with an edit queue of `edits` edits.
    fn of(rules: &RulesTable, size: [i32; 3], edits: usize) -> Result<Room, MapError> {
        let collapse = rules
            .message()
            .collapse
            .as_ref()
            .ok_or(MapError::MissingBlock("collapse"))?;
        let out_of_range = |field: &'static str, why: &str| MapError::OutOfRange {
            field,
            why: String::from(why),
        };
        let unsized_room = MapError::Unsized { queue: "collapse" };
        if collapse.max_region_voxels == 0 {
            return Err(out_of_range(
                "collapse.max_region_voxels",
                "zero: no region could ever fall",
            ));
        }
        if collapse.max_visits_per_tick == 0 {
            return Err(out_of_range(
                "collapse.max_visits_per_tick",
                "zero: no search could ever run",
            ));
        }
        let [sx, sy, sz] = size;
        let (Ok(sx), Ok(sy), Ok(sz)) = (u64::try_from(sx), u64::try_from(sy), u64::try_from(sz))
        else {
            return Err(unsized_room);
        };
        let voxels = sx
            .checked_mul(sy)
            .and_then(|plane| plane.checked_mul(sz))
            .filter(|voxels| u32::try_from(*voxels).is_ok())
            .ok_or(unsized_room.clone())?;
        if u64::from(collapse.max_visits_per_tick) > voxels {
            return Err(out_of_range(
                "collapse.max_visits_per_tick",
                "larger than the map's voxels",
            ));
        }
        // E5: every edit the queue holds, each the largest ball a weapon
        // files (a set is one voxel, at most one ball's worth).
        let radius = largest_crater(rules);
        let edge = u64::from(radius.unsigned_abs())
            .checked_mul(2)
            .and_then(|d| d.checked_add(1))
            .ok_or(unsized_room.clone())?;
        let ball = edge
            .checked_mul(edge)
            .and_then(|plane| plane.checked_mul(edge))
            .ok_or(unsized_room.clone())?;
        let changed = u64::try_from(edits)
            .ok()
            .and_then(|edits| edits.checked_mul(ball))
            .ok_or(unsized_room.clone())?;
        let to_room = |value: u64| usize::try_from(value).map_err(|_| unsized_room.clone());
        Ok(Room {
            max_region: collapse.max_region_voxels,
            max_visits: collapse.max_visits_per_tick,
            size,
            words: to_room(voxels.div_ceil(64))?,
            columns: to_room(sx.saturating_mul(sy))?,
            list: to_room(u64::from(collapse.max_visits_per_tick))?,
            changed: to_room(changed)?,
        })
    }
}

/// The largest crater any weapon files, in whole voxels: the ball E5 sizes
/// the seed lists by.
fn largest_crater(rules: &RulesTable) -> i16 {
    let crater = |weapon: Option<crate::arms::WeaponRow>| {
        weapon
            .and_then(|weapon| weapon.projectile)
            .map(|shell| shell.crater_radius_voxels)
    };
    let mut largest = crater(Some(rules.core_weapon())).into_iter().max();
    for kind in UnitKind::ALL {
        largest = largest.max(crater(rules.unit(kind).weapon));
    }
    for kind in StructureKind::ALL {
        largest = largest.max(crater(rules.structure(kind).and_then(|row| row.weapon)));
    }
    // A table with no shell at all files sets only: one voxel each.
    match largest {
        Some(radius) => radius.max(0),
        None => 0,
    }
}

/// The support rule, as [`Collapse`] calls it: a tick's searches and falls,
/// all of it cleared before the tick ends.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct SupportRule {
    room: Room,
    /// Voxels any search this tick reached.
    visited: Bits,
    /// Of those, the ones labelled supported. During the fall, `visited`
    /// marks the moving region's current voxels instead.
    supported: Bits,
    /// Every voxel the tick's searches reached, in visit order; a detached
    /// region is a range of it, rewritten as the region falls.
    list: Vec<u32>,
    /// The material of each voxel of `list`, read before anything moves.
    materials: Vec<u8>,
    regions: Vec<Region>,
    /// Region indices in fall order, recomputed each pass.
    order: Vec<usize>,
    /// `(voxel, seat)` of every voxel an edit turned to air this tick, and
    /// `(voxel, PLACED)` of every voxel an edit wrote solid into air.
    changed: Vec<([i32; 3], u8)>,
    /// `(voxel index, seat or PLACED)`: the seeds `changed` gives.
    seeds: Vec<(u32, u8)>,
    /// `(voxel index, region)` of every region voxel where it was found.
    members: Vec<(u32, usize)>,
    /// `(voxel index, region)` of every voxel a fallen region came to rest in.
    landed: Vec<(u32, usize)>,
    /// Per column, the search stamp its height was read at, and the height.
    column_stamp: Vec<u32>,
    column_height: Vec<i32>,
    stamp: u32,
    /// A change the list had no room for this tick: the tick's error.
    overflow: bool,
    /// This tick's skips, by [`CollapseSkip::slot`].
    skipped: [u64; 2],
    /// The passes this tick's fall took.
    passes: u64,
}

impl SupportRule {
    /// A rule with fresh, empty scratch of `room`'s sizes.
    fn with_room(room: Room) -> SupportRule {
        SupportRule {
            room,
            visited: Bits(vec![0; room.words]),
            supported: Bits(vec![0; room.words]),
            list: Vec::with_capacity(room.list),
            materials: Vec::with_capacity(room.list),
            regions: Vec::with_capacity(room.list),
            order: Vec::with_capacity(room.list),
            changed: Vec::with_capacity(room.changed),
            seeds: Vec::with_capacity(room.changed.saturating_mul(6)),
            members: Vec::with_capacity(room.list),
            landed: Vec::with_capacity(room.list),
            column_stamp: vec![0; room.columns],
            column_height: vec![0; room.columns],
            stamp: 0,
            overflow: false,
            skipped: [0; 2],
            passes: 0,
        }
    }

    /// Note one changed voxel, with its seed mark.
    fn record(&mut self, at: [i32; 3], mark: u8) {
        if self.changed.len() >= self.changed.capacity() {
            self.overflow = true;
            return;
        }
        self.changed.push((at, mark));
    }

    fn next_stamp(&mut self) {
        if let Some(stamp) = self.stamp.checked_add(1) {
            self.stamp = stamp;
        } else {
            // After four billion searches the stamps start again, with every
            // column read afresh.
            self.column_stamp.fill(0);
            self.stamp = 1;
        }
    }

    /// Clear both label sets by walking the list.
    fn clear_labels(&mut self) {
        for index in &self.list {
            self.visited.clear(*index);
            self.supported.clear(*index);
        }
    }

    /// End a tick: the labels cleared, every list emptied.
    fn finish(&mut self) {
        self.clear_labels();
        self.list.clear();
        self.materials.clear();
        self.regions.clear();
        self.order.clear();
        self.changed.clear();
        self.seeds.clear();
        self.members.clear();
        self.landed.clear();
        self.overflow = false;
    }

    /// Find this tick's seeds from its changes, deduplicated and in order.
    fn gather_seeds(&mut self, store: &VoxelStore) -> Result<(), CollapseError> {
        let view = Settled(store);
        for (at, mark) in &self.changed {
            let at = *at;
            if *mark == PLACED {
                // A voxel written solid into air seeds itself, if it is
                // still solid once every edit has landed.
                if view.solid(at) {
                    let index = index_of(store, at).ok_or(CollapseError::OffTheMap)?;
                    if self.seeds.len() >= self.seeds.capacity() {
                        return Err(CollapseError::Room { list: "seed" });
                    }
                    self.seeds.push((index, PLACED));
                }
                continue;
            }
            for step in NEIGHBOURS {
                let next = add(at, step);
                if !view.solid(next) {
                    continue;
                }
                let Some(index) = index_of(store, next) else {
                    continue;
                };
                if self.seeds.len() >= self.seeds.capacity() {
                    return Err(CollapseError::Room { list: "seed" });
                }
                self.seeds.push((index, *mark));
            }
        }
        // item 62: the key is the whole entry, so the order is total.
        self.seeds.sort_unstable();
        self.seeds.dedup();
        Ok(())
    }

    /// Search every seed in order, and record the detached regions.
    fn find_regions(&mut self, store: &VoxelStore, bore: &Bore) -> Result<(), CollapseError> {
        self.next_stamp();
        let view = Settled(store);
        let walk = Walk {
            view: &view,
            store,
            bore,
            max_region: crate::math::index::row(self.room.max_region),
            limit: crate::math::index::row(self.room.max_visits),
        };
        let [sx, _, sz] = self.room.size;
        let stamp = self.stamp;
        let mut at = 0;
        let mut previous: Option<u32> = None;
        while let Some((seed, _)) = self.seeds.get(at).copied() {
            at = at.saturating_add(1);
            if previous == Some(seed) {
                continue;
            }
            previous = Some(seed);
            if self.visited.get(seed) {
                continue;
            }
            if self.list.len() >= walk.limit {
                self.skip(CollapseSkip::TickBudget);
                continue;
            }
            let start = self.list.len();
            let (column_stamp, heights) = (&mut self.column_stamp, &mut self.column_height);
            let mut height = |x: i32, y: i32| -> i32 {
                let column = i64::from(y)
                    .checked_mul(i64::from(sx))
                    .and_then(|row| row.checked_add(i64::from(x)))
                    .and_then(|column| usize::try_from(column).ok());
                let cached = column.and_then(|column| {
                    (column_stamp.get(column) == Some(&stamp))
                        .then(|| heights.get(column).copied())
                        .flatten()
                });
                if let Some(cached) = cached {
                    return cached;
                }
                let found = column_height(&view, x, y, sz);
                if let Some(column) = column
                    && let (Some(slot_stamp), Some(slot)) =
                        (column_stamp.get_mut(column), heights.get_mut(column))
                {
                    *slot_stamp = stamp;
                    *slot = found;
                }
                found
            };
            let mut labels = TickLabels {
                visited: &mut self.visited,
                supported: &mut self.supported,
            };
            match search(&walk, &mut labels, &mut self.list, &mut height, seed)? {
                Outcome::Supported => {}
                Outcome::RegionBound => self.skip(CollapseSkip::RegionBound),
                Outcome::Budget => self.skip(CollapseSkip::TickBudget),
                Outcome::Detached => self.record_region(store, start)?,
            }
        }
        Ok(())
    }

    fn skip(&mut self, reason: CollapseSkip) {
        if let Some(slot) = self.skipped.get_mut(reason.slot()) {
            *slot = slot.saturating_add(1);
        }
    }

    /// Record the list's voxels from `start` as one detached region.
    fn record_region(&mut self, store: &VoxelStore, start: usize) -> Result<(), CollapseError> {
        let voxels = self
            .list
            .get(start..)
            .ok_or(CollapseError::Room { list: "search" })?;
        let mut id = u32::MAX;
        let mut lowest_z = i32::MAX;
        let mut cutters = Cutters::default();
        for voxel in voxels {
            id = id.min(*voxel);
            let [_, _, z] = at_of(store, *voxel).ok_or(CollapseError::OffTheMap)?;
            lowest_z = lowest_z.min(z);
            let from = self.seeds.partition_point(|(seed, _)| seed < voxel);
            for (seed, seat) in self.seeds.iter().skip(from) {
                if seed != voxel {
                    break;
                }
                cutters.add(*seat);
            }
        }
        let len =
            u32::try_from(voxels.len()).map_err(|_| CollapseError::Room { list: "region" })?;
        if self.regions.len() >= self.regions.capacity() {
            return Err(CollapseError::Room { list: "region" });
        }
        self.regions.push(Region {
            start,
            len,
            id,
            lowest_z,
            drop: 0,
            cutters,
        });
        Ok(())
    }

    /// The fall: every region dropped, the passes repeated until none moves.
    fn fall(&mut self, store: &mut VoxelStore) -> Result<(), CollapseError> {
        // Where each region was found, for its riders, and each voxel's
        // material, before anything moves.
        for (number, region) in self.regions.iter().enumerate() {
            for voxel in self.list.get(region.range()).into_iter().flatten() {
                if self.members.len() >= self.members.capacity() {
                    return Err(CollapseError::Room { list: "member" });
                }
                self.members.push((*voxel, number));
            }
        }
        // item 62: a voxel is in one region, so the key is unique.
        self.members.sort_unstable();
        self.clear_labels();
        for voxel in &self.list {
            let material = at_of(store, *voxel)
                .and_then(|at| store.get(at))
                .ok_or(CollapseError::OffTheMap)?;
            self.materials.push(material.raw());
        }
        let [_, _, height] = self.room.size;
        let regions = u64::try_from(self.regions.len())
            .map_err(|_| CollapseError::Room { list: "region" })?;
        let bound = regions
            .saturating_mul(u64::from(height.unsigned_abs()))
            .saturating_add(1);
        self.passes = 0;
        loop {
            self.passes = self.passes.saturating_add(1);
            if self.passes > bound {
                return Err(CollapseError::PassBound {
                    passes: self.passes,
                    bound,
                });
            }
            self.order.clear();
            self.order.extend(0..self.regions.len());
            let regions = &self.regions;
            // item 62: the identifying index is unique, so the key is total.
            self.order.sort_unstable_by_key(|number| {
                regions
                    .get(*number)
                    .map(|region| (region.lowest_z, region.id))
            });
            let mut moved = false;
            let mut at = 0;
            while let Some(number) = self.order.get(at).copied() {
                at = at.saturating_add(1);
                if self.drop_region(store, number)? {
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        for (number, region) in self.regions.iter().enumerate() {
            if region.drop == 0 {
                continue;
            }
            for voxel in self.list.get(region.range()).into_iter().flatten() {
                if self.landed.len() >= self.landed.capacity() {
                    return Err(CollapseError::Room { list: "landed" });
                }
                self.landed.push((*voxel, number));
            }
        }
        // item 62: a voxel holds one region at rest, so the key is unique.
        self.landed.sort_unstable();
        Ok(())
    }

    /// Drop one region by the smallest air gap under it. Returns whether it
    /// moved.
    fn drop_region(
        &mut self,
        store: &mut VoxelStore,
        number: usize,
    ) -> Result<bool, CollapseError> {
        let range = self
            .regions
            .get(number)
            .map(Region::range)
            .ok_or(CollapseError::Room { list: "region" })?;
        // Mark the region's voxels where they are now, so a voxel of it under
        // another is not read as ground.
        for voxel in self.list.get(range.clone()).into_iter().flatten() {
            self.visited.set(*voxel);
        }
        let mut gap = i32::MAX;
        for voxel in self.list.get(range.clone()).into_iter().flatten() {
            let [x, y, z] = at_of(store, *voxel).ok_or(CollapseError::OffTheMap)?;
            let below = index_of(store, [x, y, z.saturating_sub(1)]);
            if below.is_some_and(|below| self.visited.get(below)) {
                continue;
            }
            // Below the map's floor is solid: the bottom layer is bedrock,
            // which nothing removes, so no region reaches it.
            let mut depth: i32 = 0;
            let mut probe = z.saturating_sub(1);
            while probe >= 0 && store.get([x, y, probe]).is_some_and(Material::is_air) {
                depth = depth.saturating_add(1);
                probe = probe.saturating_sub(1);
            }
            gap = gap.min(depth);
        }
        for voxel in self.list.get(range.clone()).into_iter().flatten() {
            self.visited.clear(*voxel);
        }
        if gap == 0 || gap == i32::MAX {
            return Ok(false);
        }
        // Rigidly: every voxel leaves, then every voxel lands, each through
        // `set`, which marks its chunk (AGENTS.md section 4.8).
        for voxel in self.list.get(range.clone()).into_iter().flatten() {
            let at = at_of(store, *voxel).ok_or(CollapseError::OffTheMap)?;
            store.set(at, Material::AIR);
        }
        let mut lowest_z = i32::MAX;
        for slot in range {
            let (Some(voxel), Some(material)) = (
                self.list.get(slot).copied(),
                self.materials.get(slot).copied(),
            ) else {
                return Err(CollapseError::Room { list: "material" });
            };
            let [x, y, z] = at_of(store, voxel).ok_or(CollapseError::OffTheMap)?;
            let to = [x, y, z.saturating_sub(gap)];
            store.set(to, Material::from_raw(material));
            lowest_z = lowest_z.min(z.saturating_sub(gap));
            let index = index_of(store, to).ok_or(CollapseError::OffTheMap)?;
            if let Some(entry) = self.list.get_mut(slot) {
                *entry = index;
            }
        }
        if let Some(region) = self.regions.get_mut(number) {
            region.drop = region.drop.saturating_add(gap);
            region.lowest_z = lowest_z;
        }
        Ok(true)
    }

    fn region_of(entries: &[(u32, usize)], regions: &[Region], voxel: u32) -> Option<Region> {
        let at = entries
            .binary_search_by_key(&voxel, |(index, _)| *index)
            .ok()?;
        let (_, number) = entries.get(at).copied()?;
        regions.get(number).copied()
    }
}

/// The interface the Voxels phase calls (item 138 (3)): what replaces this
/// rule with physics replaces the implementation, not the caller.
pub trait Collapse {
    /// Note one voxel an edit turned to air this tick, with the edit's seat.
    fn note_removed(&mut self, at: [i32; 3], by: Option<SeatId>);

    /// Note one voxel an edit wrote solid into air this tick.
    fn note_placed(&mut self, at: [i32; 3]);

    /// Given this tick's changed voxels, find the regions that fall and drop
    /// each in `store`.
    ///
    /// # Errors
    ///
    /// [`CollapseError`]: the fall's pass bound, or a list or index fault no
    /// legal world reaches.
    fn collapse(&mut self, store: &mut VoxelStore, bore: &Bore) -> Result<(), CollapseError>;

    /// The fallen region an asset stood on, by the voxel under it before the
    /// fall: how far it fell, and its cutters.
    fn rider(&self, store: &VoxelStore, standing: [i32; 3]) -> Option<(i32, Cutters)>;

    /// The cutters of the fallen region that came to rest in a voxel.
    fn landed(&self, store: &VoxelStore, cell: [i32; 3]) -> Option<Cutters>;

    /// Every region that fell this tick, as `(where its identifying voxel came
    /// to rest, the voxels it holds)`, in the order they were found.
    ///
    /// # Errors
    ///
    /// [`CollapseError::OffTheMap`] for an identifying index off the map.
    fn each_fallen(
        &self,
        store: &VoxelStore,
        out: &mut dyn FnMut([i32; 3], u32),
    ) -> Result<(), CollapseError>;

    /// End the tick: nothing kept for the next.
    fn finish(&mut self);
}

impl Collapse for SupportRule {
    fn note_removed(&mut self, at: [i32; 3], by: Option<SeatId>) {
        // A seat-less edit is nobody's, which a seat column writes as NEUTRAL.
        let seat = match by {
            Some(seat) => seat.raw(),
            None => SeatId::NEUTRAL.raw(),
        };
        self.record(at, seat);
    }

    fn note_placed(&mut self, at: [i32; 3]) {
        self.record(at, PLACED);
    }

    fn collapse(&mut self, store: &mut VoxelStore, bore: &Bore) -> Result<(), CollapseError> {
        self.passes = 0;
        if self.overflow {
            return Err(CollapseError::Room { list: "change" });
        }
        if self.changed.is_empty() {
            return Ok(());
        }
        self.gather_seeds(store)?;
        self.find_regions(store, bore)?;
        if self.regions.is_empty() {
            return Ok(());
        }
        self.fall(store)
    }

    fn rider(&self, store: &VoxelStore, standing: [i32; 3]) -> Option<(i32, Cutters)> {
        let voxel = index_of(store, standing)?;
        SupportRule::region_of(&self.members, &self.regions, voxel)
            .filter(|region| region.drop > 0)
            .map(|region| (region.drop, region.cutters))
    }

    fn landed(&self, store: &VoxelStore, cell: [i32; 3]) -> Option<Cutters> {
        let voxel = index_of(store, cell)?;
        SupportRule::region_of(&self.landed, &self.regions, voxel).map(|region| region.cutters)
    }

    fn each_fallen(
        &self,
        store: &VoxelStore,
        out: &mut dyn FnMut([i32; 3], u32),
    ) -> Result<(), CollapseError> {
        for region in &self.regions {
            if region.drop == 0 {
                continue;
            }
            // The identifying voxel was found where its index says and fell
            // rigidly by the region's drop.
            let [x, y, z] = at_of(store, region.id).ok_or(CollapseError::OffTheMap)?;
            out([x, y, z.saturating_sub(region.drop)], region.len);
        }
        Ok(())
    }

    fn finish(&mut self) {
        SupportRule::finish(self);
    }
}

/// The support rule's scratch, one unhashed field of the world (S2's plan,
/// `fall`): everything a tick's collapse uses, sized at construction, holding
/// nothing between ticks but its diagnostic counts.
///
/// Compares equal to any other scratch, because it is not state: no field of
/// it outlives a tick except the counts, which are diagnostics like
/// [`crate::world::DamageDrops`]; and it is never hashed or snapshotted.
#[derive(Debug, Default)]
pub struct Scratch {
    rule: SupportRule,
    room: Room,
    counts: CollapseCounts,
    /// The Mine check's refusals, by [`DigRefused::slot`]: counted through a
    /// shared read, so atomic. A diagnostic only; nothing reads it back into
    /// the tick.
    refused: [AtomicU64; 2],
    fall_damage: i32,
    /// The cells a unit stands in, from its feet up: the headroom row.
    headroom: i32,
    /// Assets the tick's falls damage: `(target, the cutters to credit)`.
    victims: Vec<(DamageTarget, Cutters)>,
}

// Written out rather than derived, as `VoxelStore`'s is: a derived clone
// copies the lists' lengths and not their capacities, so a cloned world would
// allocate in its first collapse. The scratch holds nothing between ticks, so
// a clone is fresh scratch of the same rooms, with the counts.
impl Clone for Scratch {
    fn clone(&self) -> Scratch {
        let refused = [0, 1].map(|slot| {
            AtomicU64::new(
                self.refused
                    .get(slot)
                    .map(|count| count.load(Ordering::Relaxed))
                    .into_iter()
                    .sum(),
            )
        });
        Scratch {
            rule: SupportRule::with_room(self.room),
            room: self.room,
            counts: self.counts,
            refused,
            fall_damage: self.fall_damage,
            headroom: self.headroom,
            victims: Vec::with_capacity(self.victims.capacity()),
        }
    }
}

impl PartialEq for Scratch {
    fn eq(&self, _: &Scratch) -> bool {
        true
    }
}

impl Eq for Scratch {}

impl Scratch {
    /// The scratch for a world under `rules`, whose map is `size` voxels,
    /// with `assets` rows that can take damage (units, structures, beacons),
    /// `seats` seats and an edit queue of `edits` edits.
    ///
    /// # Errors
    ///
    /// [`MapError::MissingBlock`] without a `collapse` block,
    /// [`MapError::OutOfRange`] for a zero bound, a budget larger than the
    /// map, a fall damage past a hit point's range or more seats than the
    /// credit's seat set holds, and [`MapError::Unsized`] when a room does
    /// not fit.
    pub fn new(
        rules: &RulesTable,
        size: [i32; 3],
        assets: u32,
        seats: u32,
        edits: usize,
    ) -> Result<Scratch, MapError> {
        if seats > MAX_SEATS {
            return Err(MapError::OutOfRange {
                field: "seats",
                why: format!("{seats} seats; the collapse's credit holds {MAX_SEATS}"),
            });
        }
        let room = Room::of(rules, size, edits)?;
        let fall_damage = rules
            .message()
            .collapse
            .as_ref()
            .map(|collapse| collapse.fall_damage_hp)
            .ok_or(MapError::MissingBlock("collapse"))?;
        let fall_damage = i32::try_from(fall_damage).map_err(|_| MapError::OutOfRange {
            field: "collapse.fall_damage_hp",
            why: format!("{fall_damage}: past the most hit points an asset can lose"),
        })?;
        Ok(Scratch {
            rule: SupportRule::with_room(room),
            room,
            counts: CollapseCounts::default(),
            refused: [AtomicU64::new(0), AtomicU64::new(0)],
            fall_damage,
            // An asset stands in one cell at least, whatever the row says.
            headroom: rules.headroom_voxels().max(1),
            victims: Vec::with_capacity(crate::math::index::row(assets)),
        })
    }

    /// Note one voxel an edit turned to air this tick (the Voxels phase's
    /// edit application).
    pub(crate) fn note_removed(&mut self, at: [i32; 3], by: Option<SeatId>) {
        Collapse::note_removed(&mut self.rule, at, by);
    }

    /// Note one voxel an edit wrote solid into air this tick.
    pub(crate) fn note_placed(&mut self, at: [i32; 3]) {
        Collapse::note_placed(&mut self.rule, at);
    }

    /// What the support rule has done.
    #[must_use]
    pub fn counts(&self) -> CollapseCounts {
        let mut counts = self.counts;
        for (slot, count) in counts.refused.iter_mut().zip(&self.refused) {
            *slot = count.load(Ordering::Relaxed);
        }
        counts
    }

    fn refuse(&self, reason: DigRefused) {
        if let Some(count) = self.refused.get(reason.slot()) {
            count.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// `at` as a voxel index, `chunk << 15 | inside`, or `None` off the map.
fn index_of(store: &VoxelStore, at: [i32; 3]) -> Option<u32> {
    let (chunk, inside) = store.address(at)?;
    let inside = u32::try_from(inside).ok()?;
    chunk.checked_shl(CHUNK_BITS).map(|base| base | inside)
}

/// The voxel an index names.
fn at_of(store: &VoxelStore, index: u32) -> Option<[i32; 3]> {
    let chunk = index >> CHUNK_BITS;
    let inside = index & ((1 << CHUNK_BITS) - 1);
    let [ox, oy, oz] = store.chunk_origin(chunk)?;
    let lx = i32::try_from(inside & CHUNK_MASK).ok()?;
    let ly = i32::try_from((inside >> CHUNK_SHIFT) & CHUNK_MASK).ok()?;
    let lz = i32::try_from(inside >> (CHUNK_SHIFT * 2)).ok()?;
    Some([
        ox.checked_add(lx)?,
        oy.checked_add(ly)?,
        oz.checked_add(lz)?,
    ])
}

fn add(at: [i32; 3], step: [i32; 3]) -> [i32; 3] {
    let [x, y, z] = at;
    let [dx, dy, dz] = step;
    [
        x.saturating_add(dx),
        y.saturating_add(dy),
        z.saturating_add(dz),
    ]
}

/// The voxel an asset at `point` stands on: the one under its feet.
fn standing_voxel(point: [Fx; 3]) -> [i32; 3] {
    let [x, y, z] = point;
    [
        x.floor_voxels(),
        y.floor_voxels(),
        z.floor_voxels().saturating_sub(1),
    ]
}

/// The voxel an asset at `point` stands in: its feet.
fn cell_of(point: [Fx; 3]) -> [i32; 3] {
    let [x, y, z] = point;
    [x.floor_voxels(), y.floor_voxels(), z.floor_voxels()]
}

/// A voxel's corner as a point.
fn point_of(at: [i32; 3]) -> Option<[Fx; 3]> {
    let [x, y, z] = at;
    Some([
        Fx::from_voxels(i16::try_from(x).ok()?),
        Fx::from_voxels(i16::try_from(y).ok()?),
        Fx::from_voxels(i16::try_from(z).ok()?),
    ])
}

/// Which table an asset that can fall lives in, in the order they are walked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Table {
    Unit,
    Beacon,
    Structure,
    Wreck,
}

/// One asset a fall reached, read from its row.
#[derive(Clone, Copy)]
struct Faller {
    point: [Fx; 3],
    seat: u8,
    subject: Result<AssetId, crate::ids::IdError>,
    /// What its damage hits: none for a wreck, which has no hit points.
    target: Option<DamageTarget>,
    commander: bool,
    /// The cells it stands in, from its feet up: a unit's headroom; one for a
    /// beacon, a structure or a wreck. Every structure and beacon stands on
    /// one column today (spec section 5; the structure table holds no
    /// footprint), so its anchor column is its whole footprint; a wider
    /// footprint, when a stage gives one, is additive here.
    height: i32,
}

impl World {
    /// The support rule's counts ([`CollapseCounts`]).
    #[must_use]
    pub fn collapse_counts(&self) -> CollapseCounts {
        self.support.counts()
    }

    /// The tick's collapse, at the end of the Voxels phase (S2's plan, `fall`
    /// and E2): the search and the fall over this tick's changes, every asset
    /// on or under a fallen region moved and its fall damage filed, the damage
    /// applied by the combat phase's apply function and settled by the
    /// kill-credit phase's settle function, so the Match phase sees a death on
    /// the tick it happened.
    pub(super) fn collapse_tick(&mut self, scratch: &mut Scratch) -> Result<(), CollapseError> {
        let outcome = Collapse::collapse(&mut scratch.rule, &mut self.voxels, &self.bore);
        let rule = &mut scratch.rule;
        let (skipped, passes, visits) = (rule.skipped, rule.passes, rule.list.len());
        rule.skipped = [0; 2];
        for (reason, count) in CollapseSkip::ALL.into_iter().zip(skipped) {
            scratch.counts.skip(reason, count);
        }
        scratch.counts.last_passes = passes;
        scratch.counts.last_visits = visits;
        let result = match outcome {
            Ok(()) => self.fall_assets(scratch),
            Err(error) => Err(error),
        };
        Collapse::finish(&mut scratch.rule);
        scratch.victims.clear();
        result
    }

    /// Announce every fallen region, and move, announce and damage every
    /// asset it carried or landed on.
    fn fall_assets(&mut self, scratch: &mut Scratch) -> Result<(), CollapseError> {
        let mut regions: u64 = 0;
        let mut voxels: u64 = 0;
        {
            let tick = self.tick;
            let store = &self.voxels;
            let events = &mut self.events;
            Collapse::each_fallen(&scratch.rule, store, &mut |at, len| {
                regions = regions.saturating_add(1);
                voxels = voxels.saturating_add(u64::from(len));
                let mut emission = Emission::of(EventKind::RegionCollapsed).value(i64::from(len));
                if let Some(point) = point_of(at) {
                    emission = emission.at(Position::from_array(point));
                }
                events.emit(tick, emission);
            })?;
        }
        if regions == 0 {
            return Ok(());
        }
        scratch.counts.regions = scratch.counts.regions.saturating_add(regions);
        scratch.counts.voxels = scratch.counts.voxels.saturating_add(voxels);
        let damage = scratch.fall_damage;
        let headroom = scratch.headroom;
        let mut assets: u64 = 0;
        for table in [Table::Unit, Table::Beacon, Table::Structure, Table::Wreck] {
            let rows = match table {
                Table::Unit => self.units.len(),
                Table::Beacon => self.beacons.len(),
                Table::Structure => self.structures.len(),
                Table::Wreck => self.wrecks.len(),
            };
            for index in 0..crate::math::index::row(rows) {
                let Some(faller) = self.faller(table, index, headroom) else {
                    continue;
                };
                let Some((target, cutters)) =
                    self.fall_one(&scratch.rule, table, index, faller, damage)?
                else {
                    continue;
                };
                assets = assets.saturating_add(1);
                if let Some(target) = target
                    && damage > 0
                {
                    if scratch.victims.len() >= scratch.victims.capacity() {
                        return Err(CollapseError::Room { list: "victim" });
                    }
                    scratch.victims.push((target, cutters));
                }
            }
        }
        scratch.counts.assets = scratch.counts.assets.saturating_add(assets);
        if scratch.victims.is_empty() {
            return Ok(());
        }
        // item 62: a target is one asset, so the key is unique.
        scratch.victims.sort_unstable_by_key(|(target, _)| *target);
        self.file_fall_damage(&scratch.victims, i64::from(damage))
    }

    /// The asset in `table`'s row `index`, when it is one that can fall: a
    /// live unit, beacon or structure, or a wreck.
    fn faller(&self, table: Table, index: usize, headroom: i32) -> Option<Faller> {
        let row = Row::of_index(index)?;
        match table {
            Table::Unit => {
                let id = self.units.id_of(row)?;
                if !self.units.hit_points().get(index)?.is_alive() {
                    return None;
                }
                let kind = self.units.kinds().get(index).copied();
                Some(Faller {
                    point: *self.units.positions().get(index)?,
                    seat: *self.units.seats().get(index)?,
                    subject: AssetId::of_unit(id),
                    target: Some(DamageTarget::Unit(id)),
                    commander: kind.and_then(UnitKind::from_id) == Some(UnitKind::Commander),
                    height: headroom,
                })
            }
            Table::Beacon => {
                let id = self.beacons.id_of(row)?;
                if !self.beacons.hit_points().get(index)?.is_alive() {
                    return None;
                }
                Some(Faller {
                    point: *self.beacons.positions().get(index)?,
                    seat: *self.beacons.seats().get(index)?,
                    subject: AssetId::of_beacon(id),
                    target: Some(DamageTarget::Beacon(id)),
                    commander: false,
                    height: 1,
                })
            }
            Table::Structure => {
                let id = self.structures.id_of(row)?;
                if !self.structures.hit_points().get(index)?.is_alive() {
                    return None;
                }
                Some(Faller {
                    point: *self.structures.positions().get(index)?,
                    seat: *self.structures.seats().get(index)?,
                    subject: AssetId::of_structure(id),
                    target: Some(DamageTarget::Structure(id)),
                    commander: false,
                    height: 1,
                })
            }
            Table::Wreck => {
                if *self.wrecks.frees().get(index)? {
                    return None;
                }
                let id = WreckId::new(*self.wrecks.ids().get(index)?);
                Some(Faller {
                    point: *self.wrecks.positions().get(index)?,
                    seat: SeatId::NEUTRAL.raw(),
                    subject: AssetId::of_wreck(id),
                    target: None,
                    commander: false,
                    height: 1,
                })
            }
        }
    }

    /// The cutters of a fallen region that came to rest in any of the cells
    /// an asset of `height` at `feet` stands in.
    fn landed_on(
        rule: &SupportRule,
        store: &VoxelStore,
        feet: [i32; 3],
        height: i32,
    ) -> Option<Cutters> {
        let [x, y, z] = feet;
        (0..height).find_map(|up| rule.landed(store, [x, y, z.saturating_add(up)]))
    }

    /// Fall one asset, if a fallen region carried it or came to rest in any of
    /// its cells: move it, announce it, and answer its damage target (none
    /// for a wreck) and the cutters its damage credits (nobody, for a
    /// commander, or for a region no removal cut).
    ///
    /// A rider moves down with its region; an asset a region came to rest in,
    /// a rider among them when a second region landed on the first, stands on
    /// the first air above it, `height` cells tall. Either way it takes the
    /// fall damage once.
    fn fall_one(
        &mut self,
        rule: &SupportRule,
        table: Table,
        index: usize,
        faller: Faller,
        damage: i32,
    ) -> Result<Option<(Option<DamageTarget>, Cutters)>, CollapseError> {
        let point = faller.point;
        let mut moved = point;
        let mut cutters = None;
        if let Some((drop, riding)) = rule.rider(&self.voxels, standing_voxel(point)) {
            let [x, y, z] = point;
            let by = Fx::from_voxels(i16::try_from(drop).map_err(|_| CollapseError::OffTheMap)?);
            moved = [x, y, z.checked_sub(by).ok_or(CollapseError::OffTheMap)?];
            cutters = Some(riding);
        }
        if let Some(landing) = World::landed_on(rule, &self.voxels, cell_of(moved), faller.height) {
            // Decision 11: it stands on the new top, the first air above its
            // feet tall enough for it.
            let [cx, cy, cz] = cell_of(moved);
            let [_, _, size_z] = self.rules.map_size_voxels();
            let mut feet = cz;
            while (0..faller.height).any(|up| {
                self.voxels
                    .get([cx, cy, feet.saturating_add(up)])
                    .is_some_and(Material::is_solid)
            }) {
                feet = feet.saturating_add(1);
                if feet >= size_z {
                    return Err(CollapseError::OffTheMap);
                }
            }
            let [x, y, _] = moved;
            moved = [
                x,
                y,
                Fx::from_voxels(i16::try_from(feet).map_err(|_| CollapseError::OffTheMap)?),
            ];
            cutters = Some(cutters.unwrap_or(landing));
        }
        let Some(cutters) = cutters else {
            return Ok(None);
        };
        let slot = match table {
            Table::Unit => self.units.positions_mut().get_mut(index),
            Table::Beacon => self.beacons.positions_mut().get_mut(index),
            Table::Structure => self.structures.positions_mut().get_mut(index),
            Table::Wreck => self.wrecks.positions_mut().get_mut(index),
        };
        let slot = slot.ok_or(CollapseError::OffTheMap)?;
        *slot = moved;
        let filed = if faller.target.is_some() { damage } else { 0 };
        let mut emission = Emission::of(EventKind::AssetFell)
            .subject_of(faller.subject)
            .at(Position::from_array(moved))
            .value(i64::from(filed));
        if SeatId::new(faller.seat).is_some() {
            emission = emission.seat(SeatId::new(faller.seat));
        }
        self.emit(self.tick, emission);
        let cutters = if faller.commander || cutters.count() == 0 {
            Cutters::NOBODY
        } else {
            cutters
        };
        Ok(Some((faller.target, cutters)))
    }

    /// File every victim's fall damage and apply it, in batches that never
    /// split one asset's orders, then settle the deaths (E2).
    ///
    /// Each victim's damage is split equally among its cutters, as integers by
    /// largest remainder with ties to the lowest seat (decision 37): one order
    /// per cutter, each carrying its seat, so the combat phase's apply function
    /// books the credit, the asset's own seat's share and nobody's crediting
    /// nobody (item 144 (2)). The apply function groups a target's orders, so
    /// a batch that holds all of one asset's orders applies exactly as one
    /// queue holding every victim's would.
    fn file_fall_damage(
        &mut self,
        victims: &[(DamageTarget, Cutters)],
        damage: i64,
    ) -> Result<(), CollapseError> {
        let room = self.rooms.damage.saturating_mul(2);
        for (target, cutters) in victims {
            // A victim always has a cutter: `fall_one` gives nobody to a
            // region no removal cut.
            let count = cutters.count().max(1);
            let orders = crate::math::index::row(count);
            if self.damage.len().saturating_add(orders) > room {
                self.apply_damage();
            }
            let count = i64::from(count);
            let (Some(each), Some(mut extra)) =
                (damage.checked_div(count), damage.checked_rem(count))
            else {
                return Err(CollapseError::DamageQueue);
            };
            for seat in cutters.each() {
                let mut amount = each;
                if extra > 0 {
                    amount = amount.saturating_add(1);
                    extra = extra.saturating_sub(1);
                }
                // A share of zero hit points is no order: the damage is
                // smaller than the cutters, and the lower seats took it all.
                if amount == 0 {
                    continue;
                }
                let amount = Damage::new(amount).map_err(|_| CollapseError::DamageQueue)?;
                self.push_damage(
                    DamageOrder {
                        target: *target,
                        amount,
                        by: seat,
                        source: None,
                    },
                    room,
                )
                .map_err(|_| CollapseError::DamageQueue)?;
            }
        }
        self.apply_damage();
        self.settle_deaths();
        Ok(())
    }

    /// Whether a Mine dig of `at` leaves every seat's structure and beacon
    /// standing (item 144 (3)): the support search, run read-only over the
    /// world as the edits queued this tick and then the dig will leave it,
    /// finds no detached region that carries one. A refusal is counted under
    /// [`DigRefused`]. Mine's dig choice asks it, and a test may.
    ///
    /// A pure read: it builds its own scratch, and only when a face of the
    /// dig is not grounded by its own column. Its searches count against
    /// neither collapse row's budget; each is bounded by
    /// `collapse.max_region_voxels`.
    #[must_use]
    pub fn dig_keeps_structures_standing(&self, at: [i32; 3]) -> bool {
        let refused = match self.undermines(at) {
            Ok(false) => return true,
            Ok(true) => DigRefused::WouldUndermine,
            Err(reason) => reason,
        };
        self.support.refuse(refused);
        false
    }

    fn undermines(&self, dug: [i32; 3]) -> Result<bool, DigRefused> {
        let view = Thought::new(
            &self.voxels,
            &self.edits,
            self.rules.combat().blast_levels,
            &self.bore,
            dug,
        );
        let [_, _, size_z] = self.support.room.size;
        // The fast path: every solid face of the dig grounded by its own
        // column, or a bore voxel. Nothing is allocated.
        let mut faces = [None; 6];
        for (step, face) in NEIGHBOURS.into_iter().zip(faces.iter_mut()) {
            let next = add(dug, step);
            if !view.solid(next) {
                continue;
            }
            let [x, y, z] = next;
            if z < column_height(&view, x, y, size_z) || self.bore.contains(next) {
                continue;
            }
            *face = Some(index_of(&self.voxels, next).ok_or(DigRefused::Unchecked)?);
        }
        if faces.iter().all(Option::is_none) {
            return Ok(false);
        }
        // The slow path: the search from each ungrounded face, over sets of
        // this call's own.
        let max_region = crate::math::index::row(self.support.room.max_region);
        let mut list: Vec<u32> = Vec::with_capacity(max_region.saturating_add(1).saturating_mul(6));
        let mut labels = CallLabels::default();
        let walk = Walk {
            view: &view,
            store: &self.voxels,
            bore: &self.bore,
            max_region,
            limit: list.capacity(),
        };
        let mut height = |x: i32, y: i32| column_height(&view, x, y, size_z);
        let mut detached = false;
        for seed in faces.into_iter().flatten() {
            if labels.visited(seed) {
                continue;
            }
            match search(&walk, &mut labels, &mut list, &mut height, seed) {
                // A detached region's voxels stay visited and unlabelled:
                // that is how a structure standing on it is found below.
                Ok(Outcome::Detached) => detached = true,
                Ok(Outcome::Supported | Outcome::RegionBound) => {}
                Ok(Outcome::Budget) | Err(_) => return Err(DigRefused::Unchecked),
            }
        }
        if !detached {
            return Ok(false);
        }
        let on_region = |point: [Fx; 3]| {
            index_of(&self.voxels, standing_voxel(point))
                .is_some_and(|voxel| labels.visited(voxel) && !labels.supported(voxel))
        };
        let structures = self
            .structures
            .positions()
            .iter()
            .zip(self.structures.hit_points())
            .zip(self.structures.seats())
            .any(|((point, hp), seat)| {
                hp.is_alive() && SeatId::new(*seat).is_some() && on_region(*point)
            });
        let beacons = self
            .beacons
            .positions()
            .iter()
            .zip(self.beacons.hit_points())
            .any(|(point, hp)| hp.is_alive() && on_region(*point));
        Ok(structures || beacons)
    }
}
