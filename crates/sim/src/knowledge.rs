// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What one seat knows — the public knowledge surface.
//!
//! This module and [`crate::snapshot`] and [`crate::rules`] are the reason T2
//! ships its **public type surface on day one**: `plan-core`, `verifier`,
//! `gateway` and `operator` depend on `pharmakos-sim` with
//! `default-features = false` for exactly these types, and they must not have to
//! queue behind the rest of the sim (skeleton plan §4, mitigation 1).
//!
//! # What a knowledge type is, and is not
//!
//! Knowledge is **fog-limited and per seat**. It is produced by the sim and
//! filtered by the gateway's fog policy (AGENTS.md §7); no type here is a
//! window onto another seat's state, and nothing here can be stepped.
//!
//! A sighting carries an **age**, which accrues on match game time and is
//! refreshed closest-to-leaving-the-window first (spec section 6, Survey).
//! Ages are [`crate::math::quantity::Ms`], never ticks, because that is what a
//! playbook compares against.
//!
//! # No dry runs
//!
//! `plan-core` and `verifier` may read these types and estimate over them. They
//! may never step or fork the sim, run mandates or programs, or evaluate a rule
//! condition over a projected future (AGENTS.md §3 rule 2). Nothing in this
//! module exposes a step.
//!
//! PLACEHOLDER: the sighting catalogue grows with the stages that produce it.
//! T5 resolved the first half of it — beacons, structures and wrecks exist as
//! tables and [`AssetId`] carries a kind tag for them — and what is still open
//! is kill credit and the economy at T14 and radio at S4. Each addition is an
//! ordinary extension of [`SeatKnowledge`]; none of it changes the shape a
//! client compiles against.

use crate::math::fixed::Fx;
use crate::math::quantity::{Kw, Money, Ms, Tick};
use crate::tables::{BeaconId, SeatId, StructureId, UnitId, WreckId};

/// What kind of thing was seen.
///
/// The discriminants are written out in [`AssetKind::id`] rather than taken
/// from the enum's order, for the same reason stream ids are: a reordering must
/// not be able to change a wire value by accident.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum AssetKind {
    /// Not known.
    #[default]
    Unknown,
    /// A commander.
    Commander,
    /// A unit that is not the commander.
    Unit,
    /// A beacon.
    Beacon,
    /// A structure that is not a beacon.
    Structure,
    /// A wreck.
    Wreck,
}

impl AssetKind {
    /// The wire id. Additive only.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            AssetKind::Unknown => 0,
            AssetKind::Commander => 1,
            AssetKind::Unit => 2,
            AssetKind::Beacon => 3,
            AssetKind::Structure => 4,
            AssetKind::Wreck => 5,
        }
    }
}

/// A place on the map, in Q16.16 voxels.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Position {
    /// East.
    pub x: Fx,
    /// North.
    pub y: Fx,
    /// Up.
    pub z: Fx,
}

impl Position {
    /// The three axes in the order the canonical encoder walks them.
    #[must_use]
    pub const fn to_array(self) -> [Fx; 3] {
        [self.x, self.y, self.z]
    }

    /// Build from the three axes in encoder order.
    #[must_use]
    pub const fn from_array(a: [Fx; 3]) -> Position {
        Position {
            x: a[0],
            y: a[1],
            z: a[2],
        }
    }
}

/// What a sighting is *of*: the identity of the thing seen.
///
/// A sighting list is sorted, and item 62's convention is that a sort key ends
/// in a **unique id**. Without an identity there is no such key — two of a
/// seat's units remembered at the same voxel would compare equal, and the order
/// two machines put them in would be whatever the sort happened to do
/// (AGENTS.md §4.6). So identity is part of the type, from the first day the
/// type exists.
///
/// # The id space, widened additively at T5
///
/// The world now holds four kinds of thing a seat can see, and their ids are
/// four independent dense sequences: a unit `7`, a beacon `7`, a structure `7`
/// and a wreck `7` all exist at once. So an `AssetId` is a **kind tag in the
/// top four bits and an index in the low 28**:
///
/// | Tag | Range | What |
/// |---|---|---|
/// | 0 | `0x0000_0000 ..= 0x0FFF_FFFF` | a unit |
/// | 1 | `0x1000_0000 ..= 0x1FFF_FFFF` | a beacon |
/// | 2 | `0x2000_0000 ..= 0x2FFF_FFFF` | a structure |
/// | 3 | `0x3000_0000 ..= 0x3FFF_FFFF` | a wreck |
///
/// Units keep tag zero on purpose: [`AssetId::of_unit`] is the same function it
/// was before T5 and every id it has ever produced still means the same thing,
/// so the widening is additive rather than a renumbering. Tags 4 to 15 are
/// unallocated and take the next kinds. The world total is 300 units and 40
/// beacons (item 63), so 2^28 per kind is not a limit anybody will meet; it is
/// chosen because four bits is the smallest tag that leaves the index
/// comfortably wide.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct AssetId(u32);

impl AssetId {
    /// How far the kind tag is shifted.
    const TAG_SHIFT: u32 = 28;

    /// The low bits an index occupies.
    const INDEX_MASK: u32 = (1 << AssetId::TAG_SHIFT) - 1;

    /// The tag of a unit — zero, so [`AssetId::of_unit`] is unchanged.
    pub const TAG_UNIT: u8 = 0;
    /// The tag of a beacon.
    pub const TAG_BEACON: u8 = 1;
    /// The tag of a structure.
    pub const TAG_STRUCTURE: u8 = 2;
    /// The tag of a wreck.
    pub const TAG_WRECK: u8 = 3;

    /// Build from a raw id.
    #[must_use]
    pub const fn new(raw: u32) -> AssetId {
        AssetId(raw)
    }

    /// Build from a kind tag and an index within that kind.
    ///
    /// An index that does not fit the low 28 bits is truncated to them, which
    /// cannot happen at any world size v1 allows and is a deterministic answer
    /// rather than a panic inside a tick if it ever does.
    #[must_use]
    pub fn tagged(tag: u8, index: u32) -> AssetId {
        AssetId((u32::from(tag) << AssetId::TAG_SHIFT) | (index & AssetId::INDEX_MASK))
    }

    /// The id of a unit, seen.
    #[must_use]
    pub fn of_unit(unit: UnitId) -> AssetId {
        AssetId::tagged(AssetId::TAG_UNIT, unit.raw())
    }

    /// The id of a beacon, seen.
    #[must_use]
    pub fn of_beacon(beacon: BeaconId) -> AssetId {
        AssetId::tagged(AssetId::TAG_BEACON, beacon.raw())
    }

    /// The id of a structure, seen.
    #[must_use]
    pub fn of_structure(structure: StructureId) -> AssetId {
        AssetId::tagged(AssetId::TAG_STRUCTURE, structure.raw())
    }

    /// The id of a wreck, seen.
    #[must_use]
    pub fn of_wreck(wreck: WreckId) -> AssetId {
        AssetId::tagged(AssetId::TAG_WRECK, wreck.raw())
    }

    /// Which kind this id belongs to.
    #[must_use]
    pub fn tag(self) -> u8 {
        // The shift leaves four bits, so the narrowing cannot truncate.
        u8::try_from(self.0 >> AssetId::TAG_SHIFT).unwrap_or(0)
    }

    /// The index within the kind.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0 & AssetId::INDEX_MASK
    }

    /// The raw id, for sort keys and the canonical encoder.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// One thing a seat has seen, with how stale the sighting is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sighting {
    /// Which thing was seen. Unique within a sighting list: a second sighting
    /// of the same asset **replaces** the memory of it rather than joining it
    /// (see [`SeatKnowledge::push_sighting`]).
    pub id: AssetId,
    /// Which seat owns the thing seen.
    pub owner: SeatId,
    /// What was seen.
    pub kind: AssetKind,
    /// Where it was when it was seen. A sighting is a memory of a place, not a
    /// live position: the editor must render it as one (spec section 13).
    pub at: Position,
    /// How long ago, in game milliseconds. Accrues on match game time.
    pub age: Ms,
}

/// A seat's own economy, as the seat itself sees it.
///
/// Live standings show a seat its **own** score and rank only (spec section 3),
/// which is a gateway policy; the shape is here so the policy has something to
/// filter.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SeatEconomy {
    /// The single treasury.
    pub treasury: Money,
    /// Power supply.
    pub supply: Kw,
    /// Power draw.
    pub draw: Kw,
}

impl SeatEconomy {
    /// Supply minus draw. `None` on overflow, which a rules table cannot
    /// produce and a corrupt snapshot can.
    #[must_use]
    pub const fn headroom(self) -> Option<Kw> {
        self.supply.checked_sub(self.draw)
    }
}

/// One of a seat's own units, as the seat sees it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OwnUnit {
    /// The unit's id.
    pub id: UnitId,
    /// Where it is.
    pub at: Position,
    /// Where it is walking to.
    pub destination: Position,
}

/// Everything one seat knows at one tick.
///
/// Ordered containers only: a `Vec` walked in id order, never a hash map
/// (AGENTS.md §4.4). Every list this type hands out is sorted by a key that
/// ends in a unique id, so two machines iterate it the same way.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SeatKnowledge {
    seat: SeatId,
    tick: Tick,
    economy: SeatEconomy,
    own_units: Vec<OwnUnit>,
    sightings: Vec<Sighting>,
}

impl SeatKnowledge {
    /// An empty view for `seat` at `tick`.
    #[must_use]
    pub const fn new(seat: SeatId, tick: Tick) -> SeatKnowledge {
        SeatKnowledge {
            seat,
            tick,
            economy: SeatEconomy {
                treasury: Money::ZERO,
                supply: Kw::ZERO,
                draw: Kw::ZERO,
            },
            own_units: Vec::new(),
            sightings: Vec::new(),
        }
    }

    /// Whose view this is.
    #[must_use]
    pub const fn seat(&self) -> SeatId {
        self.seat
    }

    /// The tick it was taken at.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// The seat's own economy.
    #[must_use]
    pub const fn economy(&self) -> SeatEconomy {
        self.economy
    }

    /// Set the seat's own economy.
    pub const fn set_economy(&mut self, economy: SeatEconomy) {
        self.economy = economy;
    }

    /// The seat's own units, in ascending unit-id order.
    #[must_use]
    pub fn own_units(&self) -> &[OwnUnit] {
        &self.own_units
    }

    /// Add one of the seat's own units, keeping the list in unit-id order.
    ///
    /// A unit already in the list is **replaced**, not joined: a seat holds one
    /// record per unit, which is what makes the unit id a unique key.
    pub fn push_own_unit(&mut self, unit: OwnUnit) {
        match self.own_units.iter_mut().find(|u| u.id == unit.id) {
            Some(existing) => *existing = unit,
            None => self.own_units.push(unit),
        }
        // item 62: the key is the unit id, which is unique by the replacement
        // above, so the order is total and an unstable sort is safe.
        self.own_units.sort_unstable_by_key(|u| u.id.raw());
    }

    /// Everything the seat has seen, in `(owner, kind, x, y, z, id)` order.
    ///
    /// The key **ends in the asset id** (item 62): every field before it can
    /// collide — two of a seat's units remembered at the same voxel is an
    /// ordinary state — and a key that can collide is a key whose order two
    /// machines can disagree about.
    #[must_use]
    pub fn sightings(&self) -> &[Sighting] {
        &self.sightings
    }

    /// Record a sighting, keeping the list in its declared order.
    ///
    /// A sighting of an asset already in the list **replaces** it: a seat
    /// remembers one place per asset, refreshed as it is seen again (spec
    /// section 6, Survey). That is also what keeps [`Sighting::id`] unique
    /// across the list, and therefore what makes the sort key below total.
    pub fn push_sighting(&mut self, sighting: Sighting) {
        match self.sightings.iter_mut().find(|s| s.id == sighting.id) {
            Some(existing) => *existing = sighting,
            None => self.sightings.push(sighting),
        }
        // item 62: the key ends in the asset id, which is unique by the
        // replacement above, so the order is total.
        self.sightings.sort_unstable_by_key(|s| {
            (
                s.owner.raw(),
                s.kind.id(),
                s.at.x.raw(),
                s.at.y.raw(),
                s.at.z.raw(),
                s.id.raw(),
            )
        });
        debug_assert!(
            self.sightings
                .windows(2)
                .all(|w| w.first().map(|s| s.id) != w.get(1).map(|s| s.id)),
            "a sighting list holds one record per asset; a duplicate id makes the sort key \
             partial (item 62)"
        );
    }

    /// Forget everything, keeping the allocations.
    pub fn clear(&mut self) {
        self.own_units.clear();
        self.sightings.clear();
    }
}
