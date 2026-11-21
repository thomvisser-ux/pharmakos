// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The view a layout is placed through: the world as a private field, and the
//! reads and constructors placement uses, nothing else. Its parent module,
//! the placement itself, cannot reach the field (a parent does not see a
//! child's private items), so it cannot mark a chunk or a cluster or open a
//! tick: the state the snapshot's door reads is out of its reach by type.

use crate::attack::AttackSettings;
use crate::math::fixed::Fx;
use crate::rules::RulesTable;
use crate::seams::MandateKind;
use crate::tables::{BeaconId, BeaconTable, SeatId, UnitId, UnitKind};
use crate::world::{PlaceRefused, World};

/// A world held for one lobby layout (see the module docs).
#[derive(Debug)]
pub(crate) struct Placement<'w> {
    world: &'w mut World,
}

impl<'w> Placement<'w> {
    /// The view of `world`, for the snapshot's door to hand placement.
    pub(crate) const fn new(world: &'w mut World) -> Placement<'w> {
        Placement { world }
    }

    /// How many seats the match has.
    pub(crate) const fn seat_count(&self) -> u32 {
        self.world.seats().len()
    }

    /// Each seat's share of the world's beacon rows.
    pub(crate) const fn beacon_share(&self) -> u32 {
        self.world.beacon_share()
    }

    /// The most beacons the world holds.
    pub(crate) const fn beacon_limit(&self) -> u32 {
        self.world.beacon_limit()
    }

    /// The beacons, to read.
    pub(crate) const fn beacons(&self) -> &BeaconTable {
        self.world.beacons()
    }

    /// The rules table.
    pub(crate) const fn rules(&self) -> &RulesTable {
        self.world.rules()
    }

    /// The point a walker stands at on column `(x, y)`.
    pub(crate) fn standing(&self, x: i32, y: i32) -> Option<[Fx; 3]> {
        self.world.ground().standing(x, y)
    }

    /// The units `seat` holds.
    pub(crate) fn units_held_by(&self, seat: SeatId) -> u32 {
        self.world.units_held_by(seat)
    }

    /// The units `seat` may hold.
    pub(crate) fn unit_room_of(&self, seat: SeatId) -> u32 {
        self.world.unit_room_of(seat)
    }

    /// Place a beacon, as play's deploy does.
    pub(crate) fn place_beacon(
        &mut self,
        seat: SeatId,
        at: [Fx; 3],
        mandate: MandateKind,
    ) -> Result<BeaconId, PlaceRefused> {
        self.world.place_beacon(seat, at, mandate)
    }

    /// Write an Attack beacon's settings, with no avoid areas.
    pub(crate) fn set_attack(&mut self, beacon: BeaconId, settings: AttackSettings) -> bool {
        self.world.set_attack(beacon, settings, &[])
    }

    /// Fabricate a unit at a beacon, as a factory does.
    pub(crate) fn fabricate(
        &mut self,
        seat: SeatId,
        beacon: BeaconId,
        kind: UnitKind,
    ) -> Option<UnitId> {
        self.world.fabricate(seat, beacon, kind)
    }
}
