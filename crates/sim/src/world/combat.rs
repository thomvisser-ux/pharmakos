// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Combat: who fires, at what, the shells in flight, the damage that lands and
//! the deaths that follow (S2's plan, task `fire`).
//!
//! # The combat phase, in three steps (E2)
//!
//! 1. **Fire.** Every armed asset chooses its target from the post-Movement
//!    state and fires, in `(seat, kind, id)` order (E12), where the kind is the
//!    asset's id tag (a unit, a beacon, a structure). A target is found over
//!    the padded broadphase, tested at the units' current positions (E11), and
//!    every range test compares squared distances (AGENTS.md section 4.2). A
//!    direct shot files its damage order; a mortar files a shell (E13).
//!    Nothing fired is applied before step 3, so **fire is simultaneous**: no
//!    seat's shot lands before another's choice (decision 3).
//! 2. **Land.** Every shell whose flight ends this tick lands where it was
//!    aimed, with no lead, so a unit that kept walking is missed (decision 3):
//!    it damages every asset inside its splash radius, the firer's own
//!    included (spec section 9's friendly fire), and files a blast carrying its
//!    seat (decision 10; item 144 (2)).
//! 3. **Apply.** Every order filed so far is applied target by target (E32):
//!    the hit points the tick removed from a target -- the orders' sum,
//!    clamped to what was there -- are split among the dealing seats in
//!    proportion to their orders' amounts, as integers by largest remainder
//!    with ties to the lowest seat, the owner's own and seat-less damage
//!    crediting nobody (item 17). The orders are walked through an index
//!    sorted in place by `(target, filing index)`, so nothing allocates.
//!
//! # The rules of engagement (decision 4)
//!
//! `HOLD_FIRE` never fires. `RETURN_FIRE` fires only at an enemy asset that
//! damaged one of the seat's own assets within the rules table's under-attack
//! window, read from the attacker memory every applied order fills
//! ([`crate::tables::Arms`]), and an asset in range of the firer. `FREE_FIRE`
//! fires at any enemy in range, by the asset's targeting rule. An unset `roe`
//! reads as `RETURN_FIRE`. The core's autocannon, which has no mandate of its
//! own, fires as `FREE_FIRE` (decision 8); every other armed asset follows its
//! home beacon's Common settings. Return fire is announced
//! (`returned_fire`), once per engagement rather than once per shot: the shot
//! that opens it, fired after the weapon stood ready for at least a tick.
//!
//! # Targeting (decisions 8, 21 and 41)
//!
//! * **The core's autocannon**: the nearest enemy unit, the commander
//!   included. Never a structure or a beacon.
//! * **The autocannon**: the nearest enemy unit, else the nearest enemy
//!   structure or beacon.
//! * **The mortar**: the enemy asset in range with the most enemy assets
//!   inside its splash radius, ties nearest then lowest id, and **never** a
//!   point whose splash holds an own unit, structure, beacon or own wall voxel
//!   (spec section 9: "the built-in mandates aim to avoid your own assets").
//! * **The raider**: under `FREE_FIRE`, the nearest enemy structure or
//!   beacon, then any enemy unit; under `RETURN_FIRE`, only the asset that hit
//!   its seat, so it starts no fight (the owner's answer of 2026-10-20).
//!
//! Every weapon chooses only a target its seat sees this tick
//! (`World::seen_by`; the owner's answer of 2026-10-20).
//!
//! Ties everywhere are the squared distance, then the lowest id (E12). Enemy
//! wall voxels are no asset and no `FREE_FIRE` target: a breach shells them,
//! and the breach is S2's `atk` (decision 41).
//!
//! # Deaths (E2, E15, E16)
//!
//! The settle function runs in the kill-credit phase, after combat, and is the
//! one `fall` runs again after the voxel phase: kill credit is booked (and the
//! destroyed value and the kill counters with it, [`crate::credit::settle`]),
//! and every unit and structure the tick killed is counted among its seat's
//! losses and leaves a wreck worth `economy.salvage_percent` of its build cost
//! in a table as large as the unit and structure rooms together (decision
//! 25). A wreck that finds the table full buries the oldest wreck first,
//! counted and announced (`wreck_buried`; the ash buries old wrecks). A
//! commander is no asset and leaves none of this (it respawns, item 21); a
//! destroyed beacon is the Match phase's (`beacon_destroyed`, its structures
//! ruined), and a ruin destroyed leaves nothing, being nobody's.
//!
//! # `Stream::Combat`
//!
//! Spread, where a weapon has any, is drawn from `Stream::Combat` keyed by
//! `(tick, seat, asset)` and from nowhere else (AGENTS.md section 4.7, E12).
//! No weapon row carries a spread in S2, so nothing draws from it yet; the
//! stream stays reserved for this module, and `tests/combat.rs`'
//! `the_combat_stream_is_drawn_from_nowhere_else` holds every other module
//! away from it.

use super::{DamageDrop, DamageOrder, DamageTarget, World};
use crate::arms::{Threat, WeaponRow};
use crate::events::{Emission, EventKind};
use crate::ids::Row;
use crate::knowledge::{AssetId, Position};
use crate::math::fixed::{Fx, Sq};
use crate::math::quantity::{Damage, Hp, Money};
use crate::runner::MatchPhase;
use crate::tables::{
    Armament, Arms, BeaconId, SeatId, StructureId, StructureKind, Tally, UnitId, UnitKind,
};
use crate::voxels::{Material, VoxelEdit};

/// The rules of engagement as the combat phase reads them (decision 4).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Roe {
    /// `HOLD_FIRE`: never fires.
    Hold,
    /// `RETURN_FIRE`: fires only at an enemy asset that damaged one of the
    /// seat's own assets within the under-attack window.
    Return,
    /// `FREE_FIRE`: fires at any enemy in range.
    Free,
}

impl Roe {
    /// The rules of engagement a beacon's Common `roe` byte reads as, on a
    /// beacon whose writ is `mandate`: a byte this build does not name is
    /// `None` (the beacon table's restore refuses one, and a compiled playbook
    /// cannot write one).
    ///
    /// **The unset default is the mandate's** (the owner's answer of
    /// 2026-10-20): an unset `0` reads as `RETURN_FIRE` (decision 4, the fifth
    /// omitted-setting default) on every writ but Attack, where it reads as
    /// `FREE_FIRE`. Attack does not compile until S2's `atk`, so no Attack
    /// beacon exists yet and its arm is `atk`'s to add here, beside the
    /// Attack mandate it builds; until then every writ reads unset as
    /// `RETURN_FIRE`.
    #[must_use]
    pub const fn of_setting(roe: u8, mandate: crate::seams::MandateKind) -> Option<Roe> {
        match (roe, mandate) {
            (0 | 2, _) => Some(Roe::Return),
            (1, _) => Some(Roe::Hold),
            (3, _) => Some(Roe::Free),
            _ => None,
        }
    }
}

/// Why combat could not file what it chose (an unhashed diagnostic, counted
/// under its reason, never silent). Every room is sized so that a legal world
/// cannot fill it (E5, E13), so each count stays at zero; the tests hold it
/// there.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum CombatDrop {
    /// A shot found the damage queue full.
    DamageQueueFull,
    /// A shell found the shell table full.
    ShellsFull,
    /// A landing's blast found the edit queue full.
    EditQueueFull,
    /// A kill-credit share found its asset's three slots taken, or the
    /// counter table full.
    CreditUnbooked,
    /// A wreck found its table full with no wreck in it to bury.
    NoWreckRoom,
    /// A shell named a weapon this build does not arm, or one with no shell.
    UnknownWeapon,
    /// A landing's blast named no voxel of the map.
    BlastOffTheMap,
}

impl CombatDrop {
    /// Every reason.
    pub const ALL: [CombatDrop; 7] = [
        CombatDrop::DamageQueueFull,
        CombatDrop::ShellsFull,
        CombatDrop::EditQueueFull,
        CombatDrop::CreditUnbooked,
        CombatDrop::NoWreckRoom,
        CombatDrop::UnknownWeapon,
        CombatDrop::BlastOffTheMap,
    ];

    /// The reason's name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            CombatDrop::DamageQueueFull => "damage_queue_full",
            CombatDrop::ShellsFull => "shells_full",
            CombatDrop::EditQueueFull => "edit_queue_full",
            CombatDrop::CreditUnbooked => "credit_unbooked",
            CombatDrop::NoWreckRoom => "no_wreck_room",
            CombatDrop::UnknownWeapon => "unknown_weapon",
            CombatDrop::BlastOffTheMap => "blast_off_the_map",
        }
    }

    const fn slot(self) -> usize {
        match self {
            CombatDrop::DamageQueueFull => 0,
            CombatDrop::ShellsFull => 1,
            CombatDrop::EditQueueFull => 2,
            CombatDrop::CreditUnbooked => 3,
            CombatDrop::NoWreckRoom => 4,
            CombatDrop::UnknownWeapon => 5,
            CombatDrop::BlastOffTheMap => 6,
        }
    }
}

/// What the combat phase did, counted since the world was built: the unhashed
/// work counter beside [`crate::seams::WorkCounter`] that shots are counted in
/// instead of an event per shot (E15), and every [`CombatDrop`]. Diagnostic,
/// never hashed, never snapshotted.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CombatCounts {
    shots: u64,
    shells_fired: u64,
    shells_landed: u64,
    drops: [u64; 7],
}

impl CombatCounts {
    /// Shots fired, direct and indirect.
    #[must_use]
    pub const fn shots(&self) -> u64 {
        self.shots
    }

    /// Shells fired.
    #[must_use]
    pub const fn shells_fired(&self) -> u64 {
        self.shells_fired
    }

    /// Shells landed.
    #[must_use]
    pub const fn shells_landed(&self) -> u64 {
        self.shells_landed
    }

    /// How many were dropped for `reason`.
    #[must_use]
    pub fn dropped(&self, reason: CombatDrop) -> u64 {
        let Some(count) = self.drops.get(reason.slot()) else {
            return 0;
        };
        *count
    }

    /// How many were dropped in all.
    #[must_use]
    pub fn dropped_total(&self) -> u64 {
        self.drops.iter().fold(0, |sum, n| sum.saturating_add(*n))
    }

    /// Count one drop. A `u64` of drops is never reached, so the add
    /// saturates by rule rather than failing a tick over a diagnostic.
    fn drop(&mut self, reason: CombatDrop) {
        if let Some(count) = self.drops.get_mut(reason.slot()) {
            *count = count.saturating_add(1);
        }
    }
}

/// One armed asset in the firing order: `(seat, tag, id)`, its row beside.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(super) struct Firer {
    seat: u8,
    tag: u8,
    id: u32,
    row: Row,
}

/// A candidate target: what an order names, its id, where it stands and its
/// squared distance from the firer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Mark {
    target: DamageTarget,
    asset: AssetId,
    at: [Fx; 3],
    d2: Sq,
}

impl Mark {
    /// The tie-break key: nearest, then lowest id (E12).
    fn key(&self) -> (Sq, u32) {
        (self.d2, self.asset.raw())
    }
}

/// The nearer of two marks by [`Mark::key`].
fn nearer(held: Option<Mark>, found: Mark) -> Mark {
    match held {
        Some(held) if held.key() <= found.key() => held,
        _ => found,
    }
}

/// What one firer is, read once.
#[derive(Clone, Copy, Debug)]
struct Gun {
    seat: u8,
    asset: AssetId,
    at: [Fx; 3],
    weapon: WeaponRow,
    armament: Armament,
    roe: Roe,
}

/// Which targets a gun's rule reaches.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Rule {
    /// The core's autocannon: enemy units only.
    UnitsOnly,
    /// The autocannon: units first, then structures and beacons.
    UnitsThenSites,
    /// The raider: structures and beacons, then attackers.
    SitesThenAttackers,
    /// The mortar: the biggest clump, never over an own asset.
    Clump,
}

/// The distance between two points, Q16.16: the integer square root of their
/// squared distance (`u64::isqrt`, exact and integer, AGENTS.md section 4.2).
/// `None` past an `Fx`, which no map reaches.
fn distance(a: [Fx; 3], b: [Fx; 3]) -> Option<Fx> {
    let squared = u64::try_from(Sq::between(a, b).raw()).ok()?;
    i32::try_from(squared.isqrt()).ok().map(Fx::from_raw)
}

/// The voxel a point lies in.
fn voxel_of(point: [Fx; 3]) -> [i32; 3] {
    let [x, y, z] = point;
    [x.floor_voxels(), y.floor_voxels(), z.floor_voxels()]
}

impl World {
    /// The combat phase: fire, land, apply (E2). Firing runs only in a Push;
    /// shells land and orders apply whenever a tick runs, so a host's order
    /// filed between ticks lands at the next one.
    pub(super) fn run_combat(&mut self) {
        if self.match_state.phase() == MatchPhase::Push {
            self.fire_all();
        }
        self.land_shells();
        self.apply_damage();
    }

    /// Every armed asset that can, fires, in `(seat, kind, id)` order (E12).
    fn fire_all(&mut self) {
        let tick = self.tick.raw();
        let mut order = core::mem::take(&mut self.fire_order);
        let mut attackers = core::mem::take(&mut self.attackers);
        let mut rows = core::mem::take(&mut self.fire_rows);
        let mut splash = core::mem::take(&mut self.splash_rows);
        self.gather_firers(&mut order);
        // item 62: the key is (seat, tag, id, row), and an id is unique within
        // its tag, so the order is total.
        order.sort_unstable();
        self.gather_attackers(tick, &mut attackers);
        for firer in &order {
            self.fire_one(*firer, &attackers, &mut rows, &mut splash);
        }
        order.clear();
        attackers.clear();
        rows.clear();
        splash.clear();
        self.fire_order = order;
        self.attackers = attackers;
        self.fire_rows = rows;
        self.splash_rows = splash;
    }

    /// Every asset that carries a weapon and is still its seat's: units and
    /// structures by their kind's row, and every seat's core.
    fn gather_firers(&self, out: &mut Vec<Firer>) {
        out.clear();
        let units = self
            .units
            .ids()
            .iter()
            .zip(self.units.seats())
            .zip(self.units.kinds())
            .zip(self.units.hit_points())
            .zip(Row::all());
        for ((((id, seat), kind), hp), row) in units {
            let armed =
                UnitKind::from_id(*kind).is_some_and(|kind| self.rules.unit(kind).weapon.is_some());
            if armed && hp.is_alive() && SeatId::new(*seat).is_some() {
                out.push(Firer {
                    seat: *seat,
                    tag: AssetId::TAG_UNIT,
                    id: *id,
                    row,
                });
            }
        }
        let structures = self
            .structures
            .ids()
            .iter()
            .zip(self.structures.seats())
            .zip(self.structures.kinds())
            .zip(self.structures.hit_points())
            .zip(self.structures.building())
            .zip(Row::all());
        for (((((id, seat), kind), hp), building), row) in structures {
            let armed = StructureKind::from_id(*kind)
                .and_then(|kind| self.rules.structure(kind))
                .is_some_and(|row| row.weapon.is_some());
            if armed && hp.is_alive() && !*building && SeatId::new(*seat).is_some() {
                out.push(Firer {
                    seat: *seat,
                    tag: AssetId::TAG_STRUCTURE,
                    id: *id,
                    row,
                });
            }
        }
        let beacons = self
            .beacons
            .ids()
            .iter()
            .zip(self.beacons.seats())
            .zip(self.beacons.hit_points())
            .zip(Row::all());
        for (((id, seat), hp), row) in beacons {
            let core = crate::power::core_of(self, SeatId::new(*seat)) == Some(BeaconId::new(*id));
            if core && hp.is_alive() && SeatId::new(*seat).is_some() {
                out.push(Firer {
                    seat: *seat,
                    tag: AssetId::TAG_BEACON,
                    id: *id,
                    row,
                });
            }
        }
    }

    /// Every `(seat, attacker)` pair an own asset of that seat remembers from
    /// inside the under-attack window, sorted and without repeats: the set
    /// `RETURN_FIRE` answers from (decision 4). Read before anybody fires, so
    /// it is the attacks of earlier ticks, never this tick's.
    fn gather_attackers(&self, tick: u32, out: &mut Vec<(u8, u32)>) {
        out.clear();
        let window = self.rules.combat().under_attack_window_ms.to_ticks_ceil();
        let fresh = |arms: &Arms| {
            arms.attacker != crate::tables::NO_ATTACKER
                && arms.attacked_at != crate::tables::NEVER_ATTACKED
                && tick.saturating_sub(arms.attacked_at) <= window
        };
        let tables = [
            (
                self.units.seats(),
                self.units.arms(),
                self.units.hit_points(),
            ),
            (
                self.structures.seats(),
                self.structures.arms(),
                self.structures.hit_points(),
            ),
            (
                self.beacons.seats(),
                self.beacons.arms(),
                self.beacons.hit_points(),
            ),
        ];
        for (seats, arms, hps) in tables {
            for ((seat, arms), hp) in seats.iter().zip(arms).zip(hps) {
                if hp.is_alive() && SeatId::new(*seat).is_some() && fresh(arms) {
                    out.push((*seat, arms.attacker));
                }
            }
        }
        // Each seat's own memory of the last attack on any of its assets, so
        // an attacker whose victim died is answered too.
        for (seat, arms) in self.seats.seats().iter().zip(self.seats.attacks()) {
            if fresh(arms) {
                out.push((*seat, arms.attacker));
            }
        }
        // item 62: the key is the whole pair, and `dedup` removes the only
        // equal elements, so the order is total.
        out.sort_unstable();
        out.dedup();
    }

    /// What one firer is, or `None` when it cannot fire this tick: its weapon
    /// is cooling down, its home is dark or gone (a unit's and a structure's;
    /// a core runs off the bore and is never shed, spec section 5), or its
    /// rules of engagement hold its fire.
    fn gun_of(&self, firer: Firer) -> Option<Gun> {
        let tick = self.tick.raw();
        let index = firer.row.index();
        let (arms, at, weapon, armament, home) = match firer.tag {
            AssetId::TAG_UNIT => {
                let kind = UnitKind::from_id(*self.units.kinds().get(index)?)?;
                (
                    *self.units.arms().get(index)?,
                    *self.units.positions().get(index)?,
                    self.rules.unit(kind).weapon?,
                    Armament::Unit(kind),
                    Some(self.units.homes().get(index).copied()?),
                )
            }
            AssetId::TAG_STRUCTURE => {
                let kind = StructureKind::from_id(*self.structures.kinds().get(index)?)?;
                (
                    *self.structures.arms().get(index)?,
                    *self.structures.positions().get(index)?,
                    self.rules.structure(kind)?.weapon?,
                    Armament::Structure(kind),
                    Some(self.structures.homes().get(index).copied()?),
                )
            }
            _ => (
                *self.beacons.arms().get(index)?,
                *self.beacons.positions().get(index)?,
                self.rules.core_weapon(),
                Armament::Core,
                None,
            ),
        };
        if tick < arms.ready_at {
            return None;
        }
        let roe = match home {
            None => Roe::Free,
            Some(home) => {
                if !crate::power::beacon_is_live(self, home) {
                    return None;
                }
                let row = self.beacons.row_of(home).ok()?;
                let mandate =
                    crate::seams::MandateKind::from_id(*self.beacons.mandates().get(row.index())?)?;
                Roe::of_setting(self.beacons.commons().get(row.index())?.roe, mandate)?
            }
        };
        if roe == Roe::Hold {
            return None;
        }
        let asset = match firer.tag {
            AssetId::TAG_UNIT => AssetId::of_unit(UnitId::new(firer.id)).ok()?,
            AssetId::TAG_STRUCTURE => AssetId::of_structure(StructureId::new(firer.id)).ok()?,
            _ => AssetId::of_beacon(BeaconId::new(firer.id)).ok()?,
        };
        Some(Gun {
            seat: firer.seat,
            asset,
            at,
            weapon,
            armament,
            roe,
        })
    }

    /// One firer's choice and shot.
    fn fire_one(
        &mut self,
        firer: Firer,
        attackers: &[(u8, u32)],
        rows: &mut Vec<Row>,
        splash: &mut Vec<Row>,
    ) {
        let Some(gun) = self.gun_of(firer) else {
            return;
        };
        let rule = match gun.armament {
            Armament::Core => Rule::UnitsOnly,
            Armament::Unit(_) => Rule::SitesThenAttackers,
            Armament::Structure(_) if gun.weapon.projectile.is_some() => Rule::Clump,
            Armament::Structure(_) => Rule::UnitsThenSites,
        };
        let only = (gun.roe == Roe::Return).then_some(attackers);
        let Some(mark) = self.choose(&gun, rule, only, rows, splash) else {
            return;
        };
        let tick = self.tick.raw();
        let shot_away = match gun.weapon.projectile {
            None => {
                let order = DamageOrder {
                    target: mark.target,
                    amount: gun.weapon.damage,
                    by: SeatId::new(gun.seat),
                    source: Some(gun.asset),
                };
                let room = self.rooms.damage.saturating_mul(2);
                if self.push_damage(order, room).is_ok() {
                    true
                } else {
                    self.combat.drop(CombatDrop::DamageQueueFull);
                    false
                }
            }
            Some(shell) => {
                let flight = distance(gun.at, mark.at).and_then(|d| shell.flight_ticks(d));
                let lands = flight.and_then(|flight| tick.checked_add(flight));
                let filed = lands.is_some_and(|lands| {
                    self.shells
                        .push(
                            mark.at,
                            lands,
                            SeatId::new(gun.seat),
                            gun.asset.raw(),
                            gun.armament,
                        )
                        .is_ok()
                });
                if filed {
                    self.combat.shells_fired = self.combat.shells_fired.saturating_add(1);
                } else {
                    self.combat.drop(CombatDrop::ShellsFull);
                }
                filed
            }
        };
        if !shot_away {
            return;
        }
        self.combat.shots = self.combat.shots.saturating_add(1);
        let ready_at = tick.saturating_add(gun.weapon.cooldown_ticks());
        self.set_ready_at(firer, ready_at);
        // Return fire is announced, once per engagement: the shot that opens
        // it, after the weapon stood ready for a tick or more (spec section
        // 6's Common row).
        if gun.roe == Roe::Return && self.set_engaged(firer, mark.asset) {
            self.emit(
                self.tick,
                Emission::of(EventKind::ReturnedFire)
                    .seat(SeatId::new(gun.seat))
                    .subject(gun.asset)
                    .at(Position::from_array(gun.at)),
            );
        }
    }

    /// Record the target a return-fire shot answered ([`Arms::engaged`]):
    /// `true` when it opens a new engagement, which is announced.
    fn set_engaged(&mut self, firer: Firer, target: AssetId) -> bool {
        let index = firer.row.index();
        let slot = match firer.tag {
            AssetId::TAG_UNIT => self.units.arms_mut().get_mut(index),
            AssetId::TAG_STRUCTURE => self.structures.arms_mut().get_mut(index),
            _ => self.beacons.arms_mut().get_mut(index),
        };
        let Some(arms) = slot else {
            return false;
        };
        let opens = arms.engaged != target.raw();
        arms.engaged = target.raw();
        opens
    }

    /// Write a firer's next ready tick.
    fn set_ready_at(&mut self, firer: Firer, ready_at: u32) {
        let index = firer.row.index();
        let slot = match firer.tag {
            AssetId::TAG_UNIT => self.units.arms_mut().get_mut(index),
            AssetId::TAG_STRUCTURE => self.structures.arms_mut().get_mut(index),
            _ => self.beacons.arms_mut().get_mut(index),
        };
        if let Some(arms) = slot {
            arms.ready_at = ready_at;
        }
    }

    /// The gun's target under its rule, or `None`. `only`, under
    /// `RETURN_FIRE`, is the attacker set the target must be in.
    fn choose(
        &self,
        gun: &Gun,
        rule: Rule,
        only: Option<&[(u8, u32)]>,
        rows: &mut Vec<Row>,
        splash: &mut Vec<Row>,
    ) -> Option<Mark> {
        let range = gun.weapon.range();
        match rule {
            Rule::UnitsOnly => self.nearest_unit(gun, range, only, rows),
            Rule::UnitsThenSites => self
                .nearest_unit(gun, range, only, rows)
                .or_else(|| self.nearest_site(gun, range, only)),
            // The raider's job is its target structure or beacon (decision 21),
            // under `FREE_FIRE`, then any enemy unit it sees in reach. Under
            // `RETURN_FIRE` it is literal (the owner's answer of 2026-10-20):
            // it starts no fight and raids nothing, and its only shots answer
            // an asset that hit one of its seat's own, units first.
            Rule::SitesThenAttackers => match only {
                Some(_) => self
                    .nearest_unit(gun, range, only, rows)
                    .or_else(|| self.nearest_site(gun, range, only)),
                None => self
                    .nearest_site(gun, range, None)
                    .or_else(|| self.nearest_unit(gun, range, None, rows)),
            },
            Rule::Clump => self.clump(gun, range, only, rows, splash),
        }
    }

    /// Whether `asset`, owned by an enemy of `seat`, may be fired at under
    /// `only`: always, unless `only` is the attacker set and it is not in it.
    fn may_target(seat: u8, asset: AssetId, only: Option<&[(u8, u32)]>) -> bool {
        only.is_none_or(|set| set.binary_search(&(seat, asset.raw())).is_ok())
    }

    /// Whether `point` lies in `seat`'s live vision this tick: some sensor of
    /// the seat ([`World::sensor_at`]) reaches it, under the radius-only
    /// model ([`crate::sight::RadiusOnly`], item 144 (4)), which is the only
    /// line-of-sight model today; a later model replaces the argument here.
    /// A dormant beacon's dark units and structures are no sensors, and a
    /// core's sphere still is (decisions-log item 146).
    ///
    /// It walks every sensor for each candidate target, so its cost is
    /// targets times sensors per tick; G3'-real's tick-time gate measures it.
    ///
    /// **Every weapon chooses only what its seat sees** (the owner's answer
    /// of 2026-10-20): vision gates the choice of target, range is still the
    /// weapon's own squared-distance test, and an attacker the seat cannot
    /// see is not answered. The mortar's friendly-fire test stays on ground
    /// truth, since it is about the firer's own assets, and a shell already
    /// in flight lands whatever is seen. Walked sensor by sensor, so nothing
    /// allocates inside the tick.
    fn seen_by(&self, seat: u8, point: [Fx; 3]) -> bool {
        let mut at: usize = 0;
        while let Some((sensor, next)) = self.sensor_at(at) {
            at = next;
            if sensor.seat.raw() == seat && sensor.sees(&crate::sight::RadiusOnly, point) {
                return true;
            }
        }
        false
    }

    /// The nearest enemy unit in range, alive, the commander included.
    fn nearest_unit(
        &self,
        gun: &Gun,
        range: Fx,
        only: Option<&[(u8, u32)]>,
        rows: &mut Vec<Row>,
    ) -> Option<Mark> {
        self.units_within(gun.at, range, rows);
        let mut best: Option<Mark> = None;
        for row in rows.iter() {
            let Some(mark) = self.enemy_unit_mark(gun, *row) else {
                continue;
            };
            if Self::may_target(gun.seat, mark.asset, only) && self.seen_by(gun.seat, mark.at) {
                best = Some(nearer(best, mark));
            }
        }
        best
    }

    /// The unit in `row` as a mark for `gun`, when it is a live enemy's.
    fn enemy_unit_mark(&self, gun: &Gun, row: Row) -> Option<Mark> {
        let index = row.index();
        let seat = *self.units.seats().get(index)?;
        let hp = *self.units.hit_points().get(index)?;
        if !hp.is_alive() || !SeatId::new(seat).is_some() || seat == gun.seat {
            return None;
        }
        let id = self.units.id_of(row)?;
        let at = *self.units.positions().get(index)?;
        Some(Mark {
            target: DamageTarget::Unit(id),
            asset: AssetId::of_unit(id).ok()?,
            at,
            d2: Sq::between(gun.at, at),
        })
    }

    /// The nearest enemy structure or beacon in range, alive.
    fn nearest_site(&self, gun: &Gun, range: Fx, only: Option<&[(u8, u32)]>) -> Option<Mark> {
        let limit = Sq::of_radius(range);
        let mut best: Option<Mark> = None;
        self.for_each_enemy_site(gun.seat, |mark_target, asset, at| {
            let d2 = Sq::between(gun.at, at);
            if d2 <= limit && Self::may_target(gun.seat, asset, only) && self.seen_by(gun.seat, at)
            {
                best = Some(nearer(
                    best,
                    Mark {
                        target: mark_target,
                        asset,
                        at,
                        d2,
                    },
                ));
            }
        });
        best
    }

    /// Call `f` with every live enemy structure and beacon of `seat`'s, in
    /// table and row order.
    fn for_each_enemy_site(&self, seat: u8, mut f: impl FnMut(DamageTarget, AssetId, [Fx; 3])) {
        let structures = self
            .structures
            .ids()
            .iter()
            .zip(self.structures.seats())
            .zip(self.structures.hit_points())
            .zip(self.structures.positions())
            .zip(self.structures.frees());
        for ((((id, owner), hp), at), free) in structures {
            if *free || !hp.is_alive() || !SeatId::new(*owner).is_some() || *owner == seat {
                continue;
            }
            let id = StructureId::new(*id);
            if let Ok(asset) = AssetId::of_structure(id) {
                f(DamageTarget::Structure(id), asset, *at);
            }
        }
        let beacons = self
            .beacons
            .ids()
            .iter()
            .zip(self.beacons.seats())
            .zip(self.beacons.hit_points())
            .zip(self.beacons.positions())
            .zip(self.beacons.frees());
        for ((((id, owner), hp), at), free) in beacons {
            if *free || !hp.is_alive() || !SeatId::new(*owner).is_some() || *owner == seat {
                continue;
            }
            let id = BeaconId::new(*id);
            if let Ok(asset) = AssetId::of_beacon(id) {
                f(DamageTarget::Beacon(id), asset, *at);
            }
        }
    }

    /// The mortar's choice (decision 41): the enemy asset in range with the
    /// most enemy assets inside its splash radius, ties nearest then lowest
    /// id, never one whose splash holds an own asset or an own wall voxel.
    fn clump(
        &self,
        gun: &Gun,
        range: Fx,
        only: Option<&[(u8, u32)]>,
        rows: &mut Vec<Row>,
        splash: &mut Vec<Row>,
    ) -> Option<Mark> {
        let shell = gun.weapon.projectile?;
        let radius = shell.splash();
        let mut best: Option<(u32, Mark)> = None;
        let mut consider = |world: &World, mark: Mark, splash: &mut Vec<Row>| {
            if !Self::may_target(gun.seat, mark.asset, only)
                || !world.seen_by(gun.seat, mark.at)
                || !world.splash_is_clear(gun.seat, mark.at, radius, splash)
            {
                return;
            }
            let count = world.enemies_in_splash(gun.seat, mark.at, radius, splash);
            let better = best.is_none_or(|(held, held_mark)| {
                count > held || (count == held && mark.key() < held_mark.key())
            });
            if better {
                best = Some((count, mark));
            }
        };
        self.units_within(gun.at, range, rows);
        for row in rows.iter() {
            if let Some(mark) = self.enemy_unit_mark(gun, *row) {
                consider(self, mark, splash);
            }
        }
        let limit = Sq::of_radius(range);
        self.for_each_enemy_site(gun.seat, |target, asset, at| {
            let d2 = Sq::between(gun.at, at);
            if d2 <= limit {
                consider(
                    self,
                    Mark {
                        target,
                        asset,
                        at,
                        d2,
                    },
                    splash,
                );
            }
        });
        best.map(|(_, mark)| mark)
    }

    /// Whether a splash of `radius` at `point` holds nothing of `seat`'s: no
    /// own unit, structure or beacon alive, and no wall voxel that is `seat`'s
    /// now ([`World::wall_owner`]).
    fn splash_is_clear(&self, seat: u8, point: [Fx; 3], radius: Fx, rows: &mut Vec<Row>) -> bool {
        let limit = Sq::of_radius(radius);
        self.units_within(point, radius, rows);
        for row in rows.iter() {
            let index = row.index();
            let own = self.units.seats().get(index) == Some(&seat)
                && self
                    .units
                    .hit_points()
                    .get(index)
                    .is_some_and(|hp| hp.is_alive());
            if own {
                return false;
            }
        }
        let tables = [
            (
                self.structures.seats(),
                self.structures.hit_points(),
                self.structures.positions(),
            ),
            (
                self.beacons.seats(),
                self.beacons.hit_points(),
                self.beacons.positions(),
            ),
        ];
        for (seats, hps, positions) in tables {
            for ((owner, hp), at) in seats.iter().zip(hps).zip(positions) {
                if *owner == seat && hp.is_alive() && Sq::between(point, *at) <= limit {
                    return false;
                }
            }
        }
        !self.own_wall_within(SeatId::new(seat), point, radius)
    }

    /// Whether a wall voxel `seat` owns now lies inside the ball of `radius`
    /// around `point`, voxel centres tested against the squared radius.
    fn own_wall_within(&self, seat: SeatId, point: [Fx; 3], radius: Fx) -> bool {
        let reach = radius.floor_voxels().saturating_add(1);
        let [cx, cy, cz] = voxel_of(point);
        let limit = Sq::of_radius(radius);
        let mut dz = -reach;
        while dz <= reach {
            let mut dy = -reach;
            while dy <= reach {
                let mut dx = -reach;
                while dx <= reach {
                    let at = [
                        cx.saturating_add(dx),
                        cy.saturating_add(dy),
                        cz.saturating_add(dz),
                    ];
                    dx = dx.saturating_add(1);
                    let built_by_seat = self
                        .voxels
                        .get(at)
                        .and_then(Material::wall_of)
                        .is_some_and(|(builder, _)| builder == seat);
                    if !built_by_seat {
                        continue;
                    }
                    let Some(centre) = voxel_point(at) else {
                        continue;
                    };
                    if Sq::between(point, centre) <= limit && self.wall_owner(at) == Some(seat) {
                        return true;
                    }
                }
                dy = dy.saturating_add(1);
            }
            dz = dz.saturating_add(1);
        }
        false
    }

    /// How many live enemy assets of `seat`'s, each in the seat's sight, lie
    /// inside a splash of `radius` at `point`: a clump is what the seat can
    /// see of one.
    fn enemies_in_splash(&self, seat: u8, point: [Fx; 3], radius: Fx, rows: &mut Vec<Row>) -> u32 {
        let enemy = |owner: u8| SeatId::new(owner).is_some() && owner != seat;
        let limit = Sq::of_radius(radius);
        let mut count: u32 = 0;
        self.units_within(point, radius, rows);
        for row in rows.iter() {
            let index = row.index();
            let hostile = self
                .units
                .seats()
                .get(index)
                .is_some_and(|owner| enemy(*owner))
                && self
                    .units
                    .hit_points()
                    .get(index)
                    .is_some_and(|hp| hp.is_alive())
                && self
                    .units
                    .positions()
                    .get(index)
                    .is_some_and(|at| self.seen_by(seat, *at));
            if hostile {
                count = count.saturating_add(1);
            }
        }
        let tables = [
            (
                self.structures.seats(),
                self.structures.hit_points(),
                self.structures.positions(),
            ),
            (
                self.beacons.seats(),
                self.beacons.hit_points(),
                self.beacons.positions(),
            ),
        ];
        for (seats, hps, positions) in tables {
            for ((owner, hp), at) in seats.iter().zip(hps).zip(positions) {
                if enemy(*owner)
                    && hp.is_alive()
                    && Sq::between(point, *at) <= limit
                    && self.seen_by(seat, *at)
                {
                    count = count.saturating_add(1);
                }
            }
        }
        count
    }

    /// The weapon an armament fires, from the rules table.
    pub(crate) fn weapon_of(&self, armament: Armament) -> Option<WeaponRow> {
        match armament {
            Armament::Core => Some(self.rules.core_weapon()),
            Armament::Unit(kind) => self.rules.unit(kind).weapon,
            Armament::Structure(kind) => self.rules.structure(kind)?.weapon,
        }
    }

    /// Land every shell whose flight ends this tick (E13), in filing order:
    /// its splash's damage orders, then its blast, then `shell_landed`.
    fn land_shells(&mut self) {
        if self.shells.is_empty() {
            return;
        }
        let tick = self.tick.raw();
        let mut rows = core::mem::take(&mut self.splash_rows);
        let mut at: usize = 0;
        while at < self.shells.len() {
            let (Some(aim), Some(lands), Some(seat), Some(firer), Some(weapon)) = (
                self.shells.aims().get(at).copied(),
                self.shells.lands_at().get(at).copied(),
                self.shells.seats().get(at).copied(),
                self.shells.firers().get(at).copied(),
                self.shells.weapons().get(at).copied(),
            ) else {
                break;
            };
            at = at.saturating_add(1);
            if lands > tick {
                continue;
            }
            self.combat.shells_landed = self.combat.shells_landed.saturating_add(1);
            let row = Armament::from_id(weapon).and_then(|armament| self.weapon_of(armament));
            let Some((damage, shell)) =
                row.and_then(|row| row.projectile.map(|shell| (row.damage, shell)))
            else {
                self.combat.drop(CombatDrop::UnknownWeapon);
                continue;
            };
            self.splash_damage(
                aim,
                shell.splash(),
                damage,
                SeatId::new(seat),
                AssetId::new(firer),
                &mut rows,
            );
            let blast = VoxelEdit::Crater {
                centre: voxel_of(aim),
                radius: i32::from(shell.crater_radius_voxels),
                by: Some(SeatId::new(seat)),
            };
            match self.file_voxel_edit(blast) {
                Ok(()) => {}
                Err(super::FileRefused::OffTheMap) => self.combat.drop(CombatDrop::BlastOffTheMap),
                Err(_) => self.combat.drop(CombatDrop::EditQueueFull),
            }
            self.emit(
                self.tick,
                Emission::of(EventKind::ShellLanded)
                    .seat(SeatId::new(seat))
                    .at(Position::from_array(aim))
                    .value(i64::from(weapon)),
            );
        }
        rows.clear();
        self.splash_rows = rows;
        self.shells.remove_landed(tick);
    }

    /// File one order for every live asset inside a splash: units, then
    /// structures (a ruin among them), then beacons, each in row order, the
    /// firer's own seat included (spec section 9's friendly fire).
    fn splash_damage(
        &mut self,
        point: [Fx; 3],
        radius: Fx,
        amount: Damage,
        by: SeatId,
        source: AssetId,
        rows: &mut Vec<Row>,
    ) {
        let limit = Sq::of_radius(radius);
        let room = self.rooms.damage.saturating_mul(2);
        self.units_within(point, radius, rows);
        let mut index: usize = 0;
        while let Some(row) = rows.get(index).copied() {
            index = index.saturating_add(1);
            let alive = self
                .units
                .hit_points()
                .get(row.index())
                .is_some_and(|hp| hp.is_alive());
            let Some(id) = self.units.id_of(row) else {
                continue;
            };
            if alive {
                self.splash_order(DamageTarget::Unit(id), amount, by, source, room);
            }
        }
        let mut row_index: usize = 0;
        while let Some(row) = Row::of_index(row_index) {
            row_index = row_index.saturating_add(1);
            if row.index() >= self.structures.ids().len() {
                break;
            }
            let hit = self
                .structures
                .hit_points()
                .get(row.index())
                .is_some_and(|hp| hp.is_alive())
                && self
                    .structures
                    .positions()
                    .get(row.index())
                    .is_some_and(|at| Sq::between(point, *at) <= limit);
            if let (true, Some(id)) = (hit, self.structures.id_of(row)) {
                self.splash_order(DamageTarget::Structure(id), amount, by, source, room);
            }
        }
        let mut row_index: usize = 0;
        while let Some(row) = Row::of_index(row_index) {
            row_index = row_index.saturating_add(1);
            if row.index() >= self.beacons.ids().len() {
                break;
            }
            let hit = self
                .beacons
                .hit_points()
                .get(row.index())
                .is_some_and(|hp| hp.is_alive())
                && self
                    .beacons
                    .positions()
                    .get(row.index())
                    .is_some_and(|at| Sq::between(point, *at) <= limit);
            if let (true, Some(id)) = (hit, self.beacons.id_of(row)) {
                self.splash_order(DamageTarget::Beacon(id), amount, by, source, room);
            }
        }
    }

    fn splash_order(
        &mut self,
        target: DamageTarget,
        amount: Damage,
        by: SeatId,
        source: AssetId,
        room: usize,
    ) {
        let order = DamageOrder {
            target,
            amount,
            by,
            source: Some(source),
        };
        if self.push_damage(order, room).is_err() {
            self.combat.drop(CombatDrop::DamageQueueFull);
        }
    }

    /// Apply every queued order, target by target (E2, E32), and empty the
    /// queue. Walked through an index sorted in place, so nothing allocates
    /// and an order filed while it runs is never met: nothing files during it.
    pub(super) fn apply_damage(&mut self) {
        if self.damage.is_empty() {
            return;
        }
        let mut index = core::mem::take(&mut self.damage_index);
        index.clear();
        let mut filed: u32 = 0;
        for _ in &self.damage {
            index.push(filed);
            filed = filed.saturating_add(1);
        }
        let damage = &self.damage;
        // item 62: the key is (target, filing index), and the filing index is
        // unique, so the order is total.
        index.sort_unstable_by_key(|at| {
            (
                damage
                    .get(crate::math::index::row(*at))
                    .map(|order| order.target),
                *at,
            )
        });
        let mut at: usize = 0;
        while let Some(first) = index.get(at).copied() {
            let Some(target) = self
                .damage
                .get(crate::math::index::row(first))
                .map(|order| order.target)
            else {
                break;
            };
            let mut end = at.saturating_add(1);
            while index
                .get(end)
                .and_then(|next| self.damage.get(crate::math::index::row(*next)))
                .is_some_and(|order| order.target == target)
            {
                end = end.saturating_add(1);
            }
            if let Some(group) = index.get(at..end) {
                self.apply_group(target, group);
            }
            at = end;
        }
        index.clear();
        self.damage_index = index;
        self.damage.clear();
    }

    /// Apply one target's orders: the credit split, the drain, the attacker
    /// memory, and a death or a retreat.
    #[allow(
        clippy::too_many_lines,
        reason = "one order group's whole landing -- the row, the dealers, the split, the drain, the memory, the death or the retreat -- read and written once each, in the order the rule states them"
    )]
    fn apply_group(&mut self, target: DamageTarget, group: &[u32]) {
        let row = match self.target_row(target) {
            Ok(row) => row,
            Err(reason) => {
                for _ in group {
                    self.damage_drops.count(reason);
                }
                return;
            }
        };
        let index = row.index();
        let (hp, owner, asset) = match target {
            DamageTarget::Unit(unit) => (
                self.units.hit_points().get(index).copied(),
                self.units.seats().get(index).copied(),
                AssetId::of_unit(unit),
            ),
            DamageTarget::Beacon(beacon) => (
                self.beacons.hit_points().get(index).copied(),
                self.beacons.seats().get(index).copied(),
                AssetId::of_beacon(beacon),
            ),
            DamageTarget::Structure(structure) => (
                self.structures.hit_points().get(index).copied(),
                self.structures.seats().get(index).copied(),
                AssetId::of_structure(structure),
            ),
        };
        let (Some(hp), Some(owner), Ok(asset)) = (hp, owner, asset) else {
            for _ in group {
                self.damage_drops.count(DamageDrop::NoSuchRow);
            }
            return;
        };
        if !hp.is_alive() {
            for _ in group {
                self.damage_drops.count(DamageDrop::AlreadyDead);
            }
            return;
        }
        // The dealers, one bucket per seat, ascending: an enemy seat's own
        // bucket, and nobody's (`SeatId::NEUTRAL`) for the owner's own and
        // seat-less damage, which shares in the clamp and credits nobody.
        let has_owner = SeatId::new(owner).is_some();
        let mut buckets = core::mem::take(&mut self.dealers);
        buckets.clear();
        let mut total: i64 = 0;
        // The attacker remembered: the enemy order of the largest amount,
        // then the lowest seat, then the earliest filed (E12; the sim's
        // ties go to the lowest seat).
        let mut last_enemy: Option<(i64, u8, AssetId)> = None;
        for at in group {
            let Some(order) = self.damage.get(crate::math::index::row(*at)).copied() else {
                continue;
            };
            let amount = i64::from(order.amount.raw());
            total = total.saturating_add(amount);
            let enemy = has_owner && order.by.is_some() && order.by.raw() != owner;
            let bucket = if enemy {
                order.by.raw()
            } else {
                SeatId::NEUTRAL.raw()
            };
            match buckets.binary_search_by_key(&bucket, |(seat, _)| *seat) {
                Ok(found) => {
                    if let Some((_, sum)) = buckets.get_mut(found) {
                        *sum = sum.saturating_add(amount);
                    }
                }
                Err(place) => buckets.insert(place, (bucket, amount)),
            }
            if enemy && let Some(source) = order.source {
                let better = last_enemy.is_none_or(|(held, seat, _)| {
                    amount > held || (amount == held && order.by.raw() < seat)
                });
                if better {
                    last_enemy = Some((amount, order.by.raw(), source));
                }
            }
        }
        let removed = total.min(i64::from(hp.raw()));
        let mut split = core::mem::take(&mut self.dealt_split);
        let mut topped = core::mem::take(&mut self.topped);
        crate::credit::apportion_wide(removed, &buckets, &mut split, &mut topped);
        for (seat, share) in &split {
            if !SeatId::new(*seat).is_some() || *share <= 0 {
                continue;
            }
            let booked = i32::try_from(*share)
                .is_ok_and(|share| self.credit.credit(asset.raw(), SeatId::new(*seat), share));
            if !booked {
                self.combat.drop(CombatDrop::CreditUnbooked);
            }
            self.count_dealt(*seat, owner, *share);
        }
        split.clear();
        buckets.clear();
        self.dealt_split = split;
        self.dealers = buckets;
        self.topped = topped;
        let left = match Damage::new(removed) {
            Ok(damage) => hp.damaged_by(damage),
            Err(_) => hp,
        };
        let tick = self.tick.raw();
        let hp_slot = match target {
            DamageTarget::Unit(_) => self.units.hit_points_mut().get_mut(index),
            DamageTarget::Beacon(_) => self.beacons.hit_points_mut().get_mut(index),
            DamageTarget::Structure(_) => self.structures.hit_points_mut().get_mut(index),
        };
        if let Some(slot) = hp_slot {
            *slot = left;
        }
        if let Some((_, _, source)) = last_enemy {
            // The seat remembers the attack too, so `RETURN_FIRE` answers an
            // asset that killed its victim outright (decision 4: "damaged
            // one of the seat's own assets", dead ones among them).
            if let Some(row) = self.seat_row(SeatId::new(owner))
                && let Some(memory) = self.seats.attacks_mut().get_mut(row)
            {
                memory.attacker = source.raw();
                memory.attacked_at = tick;
            }
            let arms = match target {
                DamageTarget::Unit(_) => self.units.arms_mut().get_mut(index),
                DamageTarget::Beacon(_) => self.beacons.arms_mut().get_mut(index),
                DamageTarget::Structure(_) => self.structures.arms_mut().get_mut(index),
            };
            if let Some(arms) = arms {
                arms.attacker = source.raw();
                arms.attacked_at = tick;
            }
        }
        if !left.is_alive() {
            self.deaths.push(target);
        } else if let DamageTarget::Unit(unit) = target {
            self.consider_retreat(unit, row);
        }
    }

    /// The row a damage order's target lives in, checked by generation (S2's
    /// plan, E1): an order against an asset whose row has since been freed or
    /// reused is [`DamageDrop::StaleGeneration`], never a hit on whatever
    /// took the row.
    fn target_row(&self, target: DamageTarget) -> Result<Row, DamageDrop> {
        let found = match target {
            DamageTarget::Unit(unit) => self.units.row_of(unit),
            DamageTarget::Beacon(beacon) => self.beacons.row_of(beacon),
            DamageTarget::Structure(structure) => self.structures.row_of(structure),
        };
        found.map_err(|error| match error {
            crate::ids::RefError::Stale => DamageDrop::StaleGeneration,
            crate::ids::RefError::Id(_) | crate::ids::RefError::NoSuchRow => DamageDrop::NoSuchRow,
        })
    }

    /// Count `share` hit points `by` dealt to `owner`'s assets: the unhashed
    /// seat-against-seat counter decision 36's kingmaking check reads (E15).
    fn count_dealt(&mut self, by: u8, owner: u8, share: i64) {
        let seats = crate::math::index::row(self.seats.len());
        let (by, owner) = (usize::from(by), usize::from(owner));
        if by >= seats || owner >= seats {
            return;
        }
        let Some(slot) = by
            .checked_mul(seats)
            .and_then(|base| base.checked_add(owner))
            .and_then(|at| self.damage_dealt.get_mut(at))
        else {
            return;
        };
        *slot = slot.saturating_add(share.unsigned_abs());
    }

    /// The hit points `by` has dealt to `owner`'s assets this match, as
    /// credited (E15; decision 36): unhashed, never snapshotted, zero for a
    /// seat this world has not got.
    #[must_use]
    pub fn damage_dealt(&self, by: SeatId, owner: SeatId) -> u64 {
        let seats = crate::math::index::row(self.seats.len());
        let (by, owner) = (usize::from(by.raw()), usize::from(owner.raw()));
        if by >= seats || owner >= seats {
            return 0;
        }
        let Some(dealt) = by
            .checked_mul(seats)
            .and_then(|base| base.checked_add(owner))
            .and_then(|at| self.damage_dealt.get(at))
        else {
            return 0;
        };
        *dealt
    }

    /// A unit that survived its damage retreats once it is below its home
    /// beacon's `retreat_hp_pct` (decision 4): to the nearest own live beacon
    /// whose sphere holds a live own repair drone, else to its home, and the
    /// retreat is announced. A commander never retreats this way: it has the
    /// reflex (item 21's 20 % rule).
    fn consider_retreat(&mut self, unit: UnitId, row: Row) {
        let index = row.index();
        let (Some(kind), Some(seat), Some(hp), Some(home), Some(retreat), Some(at)) = (
            self.units
                .kinds()
                .get(index)
                .copied()
                .and_then(UnitKind::from_id),
            self.units.seats().get(index).copied(),
            self.units.hit_points().get(index).copied(),
            self.units.home(row),
            self.units.retreats().get(index).copied(),
            self.units.positions().get(index).copied(),
        ) else {
            return;
        };
        if kind == UnitKind::Commander || retreat.is_some() {
            return;
        }
        if !self.below_retreat(kind, hp, home) {
            return;
        }
        let Some(to) = self.retreat_beacon(SeatId::new(seat), at, home) else {
            return;
        };
        if let Some(slot) = self.units.retreats_mut().get_mut(index) {
            *slot = to;
        }
        let point = self
            .beacons
            .row_of(to)
            .ok()
            .and_then(|beacon| self.beacons.positions().get(beacon.index()).copied());
        let mut emission = Emission::of(EventKind::RetreatAnnounced)
            .seat(SeatId::new(seat))
            .subject_of(AssetId::of_unit(unit))
            .value(i64::from(to.raw()));
        if let Some(point) = point {
            emission = emission.at(Position::from_array(point));
        }
        self.emit(self.tick, emission);
    }

    /// Whether `hp` of a `kind` homed to `home` is below the home's retreat
    /// threshold: `hp * 100 < pct * max`, in integers. A threshold of zero
    /// never retreats, and a home since freed has none.
    pub(crate) fn below_retreat(&self, kind: UnitKind, hp: Hp, home: BeaconId) -> bool {
        let Ok(row) = self.beacons.row_of(home) else {
            return false;
        };
        let Some(common) = self.beacons.commons().get(row.index()) else {
            return false;
        };
        if common.retreat_hp_pct == 0 {
            return false;
        }
        let max = i64::from(self.unit_max_hp(kind).raw());
        i64::from(hp.raw()).saturating_mul(100)
            < i64::from(common.retreat_hp_pct).saturating_mul(max)
    }

    /// Where a unit of `seat` at `from` retreats to (decision 4): the nearest
    /// own live beacon whose sphere holds a live own repair drone, ties to the
    /// lowest id, else its `home` while that is alive.
    pub(crate) fn retreat_beacon(
        &self,
        seat: SeatId,
        from: [Fx; 3],
        home: BeaconId,
    ) -> Option<BeaconId> {
        let mut best: Option<(Sq, u32)> = None;
        let beacons = self
            .beacons
            .ids()
            .iter()
            .zip(self.beacons.seats())
            .zip(self.beacons.hit_points())
            .zip(self.beacons.positions());
        for (((id, owner), hp), at) in beacons {
            if *owner != seat.raw() || !hp.is_alive() {
                continue;
            }
            let id = BeaconId::new(*id);
            if !self.repairs_at(seat, id) {
                continue;
            }
            let d2 = Sq::between(from, *at);
            if best.is_none_or(|(held, held_id)| (d2, id.raw()) < (held, held_id)) {
                best = Some((d2, id.raw()));
            }
        }
        if let Some((_, id)) = best {
            return Some(BeaconId::new(id));
        }
        let home_row = self.beacons.row_of(home).ok()?;
        self.beacons
            .hit_points()
            .get(home_row.index())
            .is_some_and(|hp| hp.is_alive())
            .then_some(home)
    }

    /// Whether a live repair drone of `seat`'s stands inside `beacon`'s
    /// sphere: where a retreating unit waits to be repaired (decision 4).
    pub(crate) fn repairs_at(&self, seat: SeatId, beacon: BeaconId) -> bool {
        self.units
            .kinds()
            .iter()
            .zip(self.units.seats())
            .zip(self.units.hit_points())
            .zip(self.units.positions())
            .any(|(((kind, unit_seat), unit_hp), unit_at)| {
                *kind == UnitKind::RepairDrone.id()
                    && *unit_seat == seat.raw()
                    && unit_hp.is_alive()
                    && crate::mandate::inside_sphere(self, beacon, *unit_at)
            })
    }

    /// The settle function (E2): kill credit, the destroyed value and the kill
    /// counters ([`crate::credit::settle`]), then every unit and structure the
    /// tick killed: its seat's loss, `unit_destroyed` or
    /// `structure_destroyed`, and its wreck. Runs in the kill-credit phase,
    /// after combat; `fall` runs it again after the voxel phase.
    pub(super) fn settle_deaths(&mut self) {
        let mut shares = core::mem::take(&mut self.credit_shares);
        let mut split = core::mem::take(&mut self.credit_split);
        crate::credit::settle(self, &mut shares, &mut split);
        shares.clear();
        split.clear();
        self.credit_shares = shares;
        self.credit_split = split;
        let mut deaths = core::mem::take(&mut self.deaths);
        for target in &deaths {
            self.settle_death(*target);
        }
        deaths.clear();
        self.deaths = deaths;
    }

    /// One death's loss, event and wreck.
    fn settle_death(&mut self, target: DamageTarget) {
        let Ok(row) = self.target_row(target) else {
            return;
        };
        let index = row.index();
        let (seat, at, cost, kind_id, asset, kind) = match target {
            DamageTarget::Unit(unit) => {
                let Some(kind) = self
                    .units
                    .kinds()
                    .get(index)
                    .copied()
                    .and_then(UnitKind::from_id)
                else {
                    return;
                };
                if kind == UnitKind::Commander {
                    return;
                }
                (
                    self.units.seats().get(index).copied(),
                    self.units.positions().get(index).copied(),
                    self.unit_cost(kind),
                    kind.id(),
                    AssetId::of_unit(unit),
                    EventKind::UnitDestroyed,
                )
            }
            DamageTarget::Structure(structure) => {
                let Some(kind) = self
                    .structures
                    .kinds()
                    .get(index)
                    .copied()
                    .and_then(StructureKind::from_id)
                else {
                    return;
                };
                (
                    self.structures.seats().get(index).copied(),
                    self.structures.positions().get(index).copied(),
                    self.structure_cost(kind),
                    kind.id(),
                    AssetId::of_structure(structure),
                    EventKind::StructureDestroyed,
                )
            }
            // A beacon's death is the Match phase's: `beacon_destroyed`, its
            // structures ruined and its loss counted at the edge.
            DamageTarget::Beacon(_) => return,
        };
        let (Some(seat), Some(at)) = (seat, at) else {
            return;
        };
        let seat = SeatId::new(seat);
        let mut emission = Emission::of(kind)
            .subject_of(asset)
            .at(Position::from_array(at))
            .value(i64::from(kind_id));
        if seat.is_some() {
            emission = emission.seat(seat);
        }
        self.emit(self.tick, emission);
        // A ruin is nobody's: its destruction is no seat's loss and leaves no
        // wreck (it was already salvage).
        if !seat.is_some() {
            return;
        }
        self.count_loss(seat);
        let salvage = crate::economy::percent_of(cost, self.rules.combat().salvage_percent);
        if salvage.raw() > 0 {
            self.leave_wreck(at, salvage);
        }
    }

    /// Count one of `seat`'s assets lost (E15).
    pub(super) fn count_loss(&mut self, seat: SeatId) {
        if let Some(tally) = self.tally_mut(seat) {
            tally.losses = tally.losses.saturating_add(1);
        }
    }

    /// `seat`'s tally, to write into, or `None` for a seat this world has not
    /// got.
    pub(crate) fn tally_mut(&mut self, seat: SeatId) -> Option<&mut Tally> {
        let row = self.seat_row(seat)?;
        self.seats.tallies_mut().get_mut(row)
    }

    /// `seat`'s tally for the match ([`Tally`]; S2's plan, E15), or `None`
    /// for a seat this world has not got.
    #[must_use]
    pub fn tally(&self, seat: SeatId) -> Option<Tally> {
        let row = self.seat_row(seat)?;
        self.seats.tallies().get(row).copied()
    }

    /// Leave a wreck worth `salvage` at `at` (E16), burying the oldest wreck
    /// first when the table is full (decision 25), and announce it.
    fn leave_wreck(&mut self, at: [Fx; 3], salvage: Money) {
        let tick = self.tick.raw();
        let placed = match self.wrecks.place(self.wreck_limit, at, salvage, tick) {
            Ok(placed) => Ok(placed),
            Err(_) => {
                if self.bury_oldest_wreck(at) {
                    self.wrecks.place(self.wreck_limit, at, salvage, tick)
                } else {
                    Err(crate::tables::TableFull {
                        room: self.wreck_limit,
                    })
                }
            }
        };
        let Ok(placed) = placed else {
            self.combat.drop(CombatDrop::NoWreckRoom);
            return;
        };
        self.emit(
            self.tick,
            Emission::of(EventKind::WreckLeft)
                .subject_of(AssetId::of_wreck(placed.id))
                .at(Position::from_array(at))
                .value(salvage.raw()),
        );
    }

    /// Bury the oldest unsalvaged wreck -- the earliest left, ties to the
    /// lowest id -- freeing its row, and say so. `false` when no wreck stands.
    ///
    /// The event always carries a place, as every world line must (the
    /// gateway refuses a world event with none): the buried wreck's, or, for
    /// a row whose place cannot be read, `instead`, where the new wreck lies.
    fn bury_oldest_wreck(&mut self, instead: [Fx; 3]) -> bool {
        let mut oldest: Option<(u32, u32, Row)> = None;
        let rows = self
            .wrecks
            .ids()
            .iter()
            .zip(self.wrecks.left_at())
            .zip(self.wrecks.frees())
            .zip(Row::all());
        for (((id, left), free), row) in rows {
            if *free {
                continue;
            }
            if oldest.is_none_or(|(at, held, _)| (*left, *id) < (at, held)) {
                oldest = Some((*left, *id, row));
            }
        }
        let Some((_, id, row)) = oldest else {
            return false;
        };
        let at = match self.wrecks.positions().get(row.index()) {
            Some(at) => *at,
            None => instead,
        };
        if self.wrecks.release(row) {
            self.refs.retired();
        }
        self.emit(
            self.tick,
            Emission::of(EventKind::WreckBuried)
                .subject_of(AssetId::of_wreck(crate::tables::WreckId::new(id)))
                .at(Position::from_array(at)),
        );
        true
    }

    /// Whose a wall voxel is **now** (decision 42; E14): the seat its material
    /// names, while the voxel lies inside one of that seat's live spheres, and
    /// nobody's otherwise; `None` too for a voxel that is no wall. Read, never
    /// written: a beacon's death rewrites no voxel.
    #[must_use]
    pub fn wall_owner(&self, at: [i32; 3]) -> Option<SeatId> {
        let (builder, _) = self.voxels.get(at)?.wall_of()?;
        let point = voxel_point(at)?;
        self.ground()
            .inside_own_sphere(builder, point)
            .then_some(builder)
    }

    /// An asset's threat (decision 12): its weapon's damage per second from
    /// its row, zero for an unarmed one, a dead one or one this world cannot
    /// find. A pure function of the rules and the asset's kind; `dfnd` and
    /// `atk` read it.
    #[must_use]
    pub fn threat_of(&self, asset: AssetId) -> Threat {
        let weapon = match asset.tag() {
            AssetId::TAG_UNIT => {
                let Ok(row) = self.units.row_of(UnitId::new(asset.index())) else {
                    return Threat::NONE;
                };
                self.units
                    .kinds()
                    .get(row.index())
                    .copied()
                    .and_then(UnitKind::from_id)
                    .and_then(|kind| self.rules.unit(kind).weapon)
            }
            AssetId::TAG_STRUCTURE => {
                let Ok(row) = self.structures.row_of(StructureId::new(asset.index())) else {
                    return Threat::NONE;
                };
                self.structures
                    .kinds()
                    .get(row.index())
                    .copied()
                    .and_then(StructureKind::from_id)
                    .and_then(|kind| self.rules.structure(kind))
                    .and_then(|row| row.weapon)
            }
            AssetId::TAG_BEACON => {
                let beacon = BeaconId::new(asset.index());
                let Ok(row) = self.beacons.row_of(beacon) else {
                    return Threat::NONE;
                };
                let seat = self
                    .beacons
                    .seats()
                    .get(row.index())
                    .copied()
                    .map(SeatId::new);
                let core = seat.and_then(|seat| crate::power::core_of(self, seat)) == Some(beacon);
                core.then(|| self.rules.core_weapon())
            }
            _ => None,
        };
        Threat::of(weapon)
    }

    /// The combat phase's counters (shots, shells and every [`CombatDrop`]).
    /// Diagnostic; never hashed.
    #[must_use]
    pub const fn combat_counts(&self) -> CombatCounts {
        self.combat
    }

    /// Fire a mortar's shell at `aim`, between ticks (fixture API: `atk`'s
    /// breach is the game path that asks a mortar to shell a wall, decision
    /// 41). The shot is the mortar's own: in a Push, its home lit, in range,
    /// off cooldown, its weapon's flight, and its cooldown spent. It does
    /// **not** avoid the firer's own assets, so a test can shell one; the
    /// breach that uses it is owed the friendly-fire test of
    /// [`World::wall_owner`] and the mortar's own aim (spec section 9).
    ///
    /// # Errors
    ///
    /// [`FireRefused`], naming why the mortar cannot fire.
    pub fn order_shell(&mut self, structure: StructureId, aim: [Fx; 3]) -> Result<(), FireRefused> {
        if self.open_phase.is_some() {
            return Err(FireRefused::MidTick);
        }
        if self.match_state.phase() != MatchPhase::Push {
            return Err(FireRefused::NotInAPush);
        }
        let row = self
            .structures
            .row_of(structure)
            .map_err(|_| FireRefused::NoSuchAsset)?;
        let index = row.index();
        let (Some(kind), Some(seat), Some(hp), Some(building), Some(at), Some(arms)) = (
            self.structures
                .kinds()
                .get(index)
                .copied()
                .and_then(StructureKind::from_id),
            self.structures.seats().get(index).copied(),
            self.structures.hit_points().get(index).copied(),
            self.structures.building().get(index).copied(),
            self.structures.positions().get(index).copied(),
            self.structures.arms().get(index).copied(),
        ) else {
            return Err(FireRefused::NoSuchAsset);
        };
        let home = self.structures.homes().get(index).copied();
        let lit = home.is_some_and(|home| crate::power::beacon_is_live(self, home));
        if !hp.is_alive() || building || !SeatId::new(seat).is_some() || !lit {
            return Err(FireRefused::CannotFire);
        }
        let weapon = self
            .rules
            .structure(kind)
            .and_then(|row| row.weapon)
            .ok_or(FireRefused::NoShell)?;
        let shell = weapon.projectile.ok_or(FireRefused::NoShell)?;
        // Filed between ticks, the shot is the next tick's: it fires then,
        // lands a flight after it, and spends its cooldown from it, so the
        // fixture keeps the spacing the shell room is sized by (E13).
        let tick = self.tick.raw().saturating_add(1);
        if tick < arms.ready_at {
            return Err(FireRefused::CoolingDown);
        }
        if Sq::between(at, aim) > Sq::of_radius(weapon.range()) {
            return Err(FireRefused::OutOfRange);
        }
        let flight = distance(at, aim)
            .and_then(|d| shell.flight_ticks(d))
            .ok_or(FireRefused::OutOfRange)?;
        let lands = tick.checked_add(flight).ok_or(FireRefused::OutOfRange)?;
        let asset = AssetId::of_structure(structure).map_err(|_| FireRefused::NoSuchAsset)?;
        self.shells
            .push(
                aim,
                lands,
                SeatId::new(seat),
                asset.raw(),
                Armament::Structure(kind),
            )
            .map_err(|_| FireRefused::ShellsFull)?;
        self.combat.shells_fired = self.combat.shells_fired.saturating_add(1);
        self.combat.shots = self.combat.shots.saturating_add(1);
        if let Some(slot) = self.structures.arms_mut().get_mut(index) {
            slot.ready_at = tick.saturating_add(weapon.cooldown_ticks());
        }
        Ok(())
    }
}

/// Why [`World::order_shell`] refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FireRefused {
    /// A tick is open between two of its phases.
    MidTick,
    /// The id names no live row.
    NoSuchAsset,
    /// The match is not in a Push.
    NotInAPush,
    /// The structure is dead, still going up, a ruin, or its home is dark.
    CannotFire,
    /// Its kind fires no shell.
    NoShell,
    /// Its weapon is cooling down.
    CoolingDown,
    /// The aim point is out of its range.
    OutOfRange,
    /// The shell table is full.
    ShellsFull,
}

impl core::fmt::Display for FireRefused {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let why = match self {
            FireRefused::MidTick => "a tick is open between two of its phases",
            FireRefused::NoSuchAsset => "the id names no structure",
            FireRefused::NotInAPush => "the match is not in a Push",
            FireRefused::CannotFire => {
                "the structure is dead, going up, a ruin, or its home is dark"
            }
            FireRefused::NoShell => "the structure fires no shell",
            FireRefused::CoolingDown => "its weapon is cooling down",
            FireRefused::OutOfRange => "the aim point is out of its range",
            FireRefused::ShellsFull => "the shell table is full",
        };
        f.write_str(why)
    }
}

impl std::error::Error for FireRefused {}

/// The centre-bottom point of a voxel, as an asset standing in it would be
/// placed: whole voxels, Q16.16.
fn voxel_point(at: [i32; 3]) -> Option<[Fx; 3]> {
    let [x, y, z] = at;
    Some([
        Fx::from_voxels(i16::try_from(x).ok()?),
        Fx::from_voxels(i16::try_from(y).ok()?),
        Fx::from_voxels(i16::try_from(z).ok()?),
    ])
}
