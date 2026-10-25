// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Defend mandate and the threat model (spec section 6's Defend row,
//! section 4's bodyguard; S2's plan, task `dfnd`, decisions 6, 12, 13 and 21,
//! decisions-log item 148).
//!
//! # The threat model (decision 12)
//!
//! An enemy asset's **threat** is its weapon's damage per second, from its
//! row ([`crate::arms::Threat`]), zero when it is unarmed. A seat reads only
//! the threats it **sees** this tick, through the sensor seam
//! ([`World::seen_by`]; item 148's seat-sight rule): a "known threat" is an
//! armed enemy unit, finished structure or core that one of the seat's
//! sensors reaches now. Everything below is built from that one reading:
//!
//! * `most_threatened` is the own beacon with the largest summed threat inside
//!   its sphere, ties to the lowest id ([`sphere_threat`]);
//! * `safest` subtracts that sum from its hit-point rank, after decision 6's
//!   repair term ([`crate::interpreter`]'s selector catalogue);
//! * a Mine beacon's `SAFEST` seam is the nearest with no known threat whose
//!   weapon reaches it, and `flee_on_threat` walks a mine drone home while a
//!   known threat is inside its beacon's sphere ([`threat_in_sphere`]);
//! * a Defend beacon's guards go after the highest threat inside its
//!   engagement radius, ties nearest then lowest id.
//!
//! *Downside, as ruled:* a model built from weapon rows only, blind to an
//! unarmed threat.
//!
//! # Defend (spec section 6's Defend row; decision 13)
//!
//! A Defend beacon holds its sphere. Its fabricator fields **raiders as
//! guards** while armed threats seen inside the sphere outrun the guards it
//! has (item 22's backlog rule, in the `defend_under_attack` band), and
//! **scouts as pickets** up to `picket_scouts`, as Survey fields its scouts
//! and as the verifier's projection spends them (decisions-log item 146 (3)
//! (h)). Auto-fortify and unit mix stay reserved for S3 (decision 13).
//!
//! Every tick, before the programs run, each Defend beacon's row is brought
//! up to date ([`update`]): its **quarry**, the one enemy unit all its guards
//! go after, and its **picket leg**, where on the ring its pickets stand.
//!
//! * **The quarry.** Kept while the beacon's `pursue` lets the guards follow
//!   it -- `NEVER`, while it is inside the engagement radius (or near a
//!   protected asset); `SPHERE_EDGE`, while it is inside the sphere;
//!   `WHILE_IN_REACH`, while the seat sees it -- and otherwise chosen again:
//!   first among the threats near the commander, when the protect list holds
//!   `my_commander` and the commander is inside the sphere (spec section 4:
//!   "guards escort the commander and intercept threats near it while it is
//!   inside this beacon's sphere"); then near each protected beacon inside the
//!   sphere, in list order; then inside the engagement radius. "Near" is
//!   within the engagement radius of that asset. Within a tier the highest
//!   threat wins, ties nearest that tier's centre, then lowest id (decision
//!   12). Only enemy units are quarries (decision 21: "a raider guarding under
//!   Defend engages enemy units inside the engagement radius").
//! * **The rules of engagement.** `HOLD_FIRE` chooses no quarry. An explicit
//!   `RETURN_FIRE` chooses only an asset that damaged one of the seat's own
//!   within the window, so its guards start no fight (item 148 (2)); an unset
//!   `roe` on a Defend beacon reads as `FREE_FIRE` (the owner's answer of
//!   2026-10-20; [`crate::world::Roe::of_setting`]).
//! * **A guard** walks to the quarry until it is in its weapon's range and
//!   holds there, where the combat phase fires at it; with no quarry it
//!   escorts the commander while the commander is inside the sphere, and
//!   otherwise idles at its beacon. A guard below full hit points with no
//!   quarry walks to the nearest friendly repair drone (spec: "damaged units
//!   fall back to the nearest friendly repair drone"), as does a damaged
//!   picket.
//! * **The picket.** Its scouts patrol a ring `defend.picket_radius_voxels`
//!   outside the sphere's edge, [`PICKET_LEGS`] points around the beacon, the
//!   pickets spaced evenly on it; once every picket stands at its point the
//!   leg moves on by one. A picket sees through its own unit vision, which is
//!   one more sensor of the seat (E10), so it warns of what comes before it
//!   reaches the sphere.
//!
//! Nothing here allocates on a tick without a Defend beacon, and the Defend
//! table is walked in ascending beacon id, the units in ascending row, so
//! every choice is a total order (AGENTS.md section 4.6).

use crate::arms::{Threat, WeaponRow};
use crate::ids::Row;
use crate::knowledge::AssetId;
use crate::math::fixed::{Fx, Sq};
use crate::seams::MandateKind;
use crate::tables::{
    BeaconId, DefendRow, NO_QUARRY, Pursue, SeatId, StructureKind, UnitId, UnitKind,
};
use crate::world::Roe;
use crate::world::World;

/// How many points the picket's ring has: the eight compass points around the
/// beacon. A leg is one of them.
pub const PICKET_LEGS: u8 = 8;

/// One known enemy asset: where it stands, its threat and its weapon.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Known {
    /// Where it stands.
    pub(crate) at: [Fx; 3],
    /// Its threat; never zero, since only an armed asset is a threat.
    pub(crate) threat: Threat,
    /// Its weapon.
    pub(crate) weapon: WeaponRow,
}

/// Call `each` with every armed enemy asset `seat` sees this tick: units in
/// ascending row, then finished structures, then cores. A pure read.
pub(crate) fn for_each_known_threat<F: FnMut(Known)>(world: &World, seat: SeatId, mut each: F) {
    let units = world.units();
    let rows = units
        .seats()
        .iter()
        .zip(units.kinds())
        .zip(units.hit_points())
        .zip(units.positions());
    for (((owner, kind), hp), at) in rows {
        if !hp.is_alive() || !enemy(seat, *owner) {
            continue;
        }
        let Some(weapon) =
            UnitKind::from_id(*kind).and_then(|kind| world.rules().unit(kind).weapon)
        else {
            continue;
        };
        report(world, seat, *at, weapon, &mut each);
    }
    let structures = world.structures();
    let rows = structures
        .seats()
        .iter()
        .zip(structures.kinds())
        .zip(structures.hit_points())
        .zip(structures.building())
        .zip(structures.positions());
    for ((((owner, kind), hp), building), at) in rows {
        if !hp.is_alive() || *building || !enemy(seat, *owner) {
            continue;
        }
        let Some(weapon) = StructureKind::from_id(*kind)
            .and_then(|kind| world.rules().structure(kind))
            .and_then(|row| row.weapon)
        else {
            continue;
        };
        report(world, seat, *at, weapon, &mut each);
    }
    let beacons = world.beacons();
    let rows = beacons
        .seats()
        .iter()
        .zip(beacons.hit_points())
        .zip(beacons.positions())
        .zip(Row::all());
    for (((owner, hp), at), row) in rows {
        if !hp.is_alive() || !enemy(seat, *owner) {
            continue;
        }
        let Some(id) = beacons.id_of(row) else {
            continue;
        };
        if crate::power::core_of(world, SeatId::new(*owner)) != Some(id) {
            continue;
        }
        report(world, seat, *at, world.rules().core_weapon(), &mut each);
    }
}

/// Whether `owner` is a seat other than `seat`: an enemy of it.
fn enemy(seat: SeatId, owner: u8) -> bool {
    SeatId::new(owner).is_some() && owner != seat.raw()
}

/// Hand one armed enemy asset to `each` when `seat` sees it.
fn report<F: FnMut(Known)>(
    world: &World,
    seat: SeatId,
    at: [Fx; 3],
    weapon: WeaponRow,
    each: &mut F,
) {
    if world.seen_by(seat.raw(), at) {
        each(Known {
            at,
            threat: Threat::of(Some(weapon)),
            weapon,
        });
    }
}

/// The summed threat, in thousandths of a hit point per second, of the armed
/// enemy assets `seat` sees inside the sphere centred on `centre` (decision
/// 12's `most_threatened` and `safest` term). Saturates by rule: a sum past
/// `u64` is no sum a map can hold.
#[must_use]
pub(crate) fn sphere_threat(world: &World, seat: SeatId, centre: [Fx; 3]) -> u64 {
    let limit = Sq::of_radius(world.sphere_radius());
    let mut sum: u64 = 0;
    for_each_known_threat(world, seat, |known| {
        if Sq::between(known.at, centre) <= limit {
            sum = sum.saturating_add(known.threat.milli_hp_per_second());
        }
    });
    sum
}

/// How many armed enemy assets `seat` sees inside the sphere centred on
/// `centre`: a Defend beacon's work in hand for its guards (decision 13).
#[must_use]
pub(crate) fn threats_in_sphere(world: &World, seat: SeatId, centre: [Fx; 3]) -> u32 {
    let limit = Sq::of_radius(world.sphere_radius());
    let mut count: u32 = 0;
    for_each_known_threat(world, seat, |known| {
        if Sq::between(known.at, centre) <= limit {
            count = count.saturating_add(1);
        }
    });
    count
}

/// Whether `seat` knows of a threat inside the sphere centred on `centre`
/// (decision 12's `flee_on_threat`).
#[must_use]
pub(crate) fn threat_in_sphere(world: &World, seat: SeatId, centre: [Fx; 3]) -> bool {
    threats_in_sphere(world, seat, centre) > 0
}

/// Whether `seat` has a live repair drone inside the sphere centred on
/// `centre` (decision 6's rescue term).
#[must_use]
pub(crate) fn repairs_in_sphere(world: &World, seat: SeatId, centre: [Fx; 3]) -> bool {
    let limit = Sq::of_radius(world.sphere_radius());
    let units = world.units();
    units
        .kinds()
        .iter()
        .zip(units.seats())
        .zip(units.hit_points())
        .zip(units.positions())
        .any(|(((kind, owner), hp), at)| {
            *kind == UnitKind::RepairDrone.id()
                && *owner == seat.raw()
                && hp.is_alive()
                && Sq::between(*at, centre) <= limit
        })
}

/// A Defend beacon as [`update`] and the programs read it.
#[derive(Clone, Copy, Debug)]
struct Post {
    seat: SeatId,
    centre: [Fx; 3],
    /// The sphere's radius.
    sphere: Fx,
    /// The engagement radius.
    engage: Fx,
    roe: Roe,
}

/// The engagement radius: `pct` percent of the sphere radius `sphere`,
/// rounded down to a raw Q16.16 step, the whole sphere at 100.
#[allow(
    clippy::integer_division,
    reason = "a percentage of a radius rounds down by rule, and the divisor is the constant 100"
)]
fn engagement(sphere: Fx, pct: u8) -> Fx {
    let raw = i64::from(sphere.raw()).saturating_mul(i64::from(pct.min(100))) / 100;
    // E7: cannot miss: at most 100 percent of an `Fx` is an `Fx`
    i32::try_from(raw).map_or(sphere, Fx::from_raw)
}

/// The Defend beacon `row` describes, or `None` when it is no live beacon of a
/// seat in the match.
fn post_of(world: &World, row: &DefendRow) -> Option<Post> {
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
    let roe = Roe::of_setting(beacons.commons().get(at)?.roe, MandateKind::Defend)?;
    let sphere = world.sphere_radius();
    Some(Post {
        seat,
        centre,
        sphere,
        engage: engagement(sphere, row.settings.engagement_reading()),
        roe,
    })
}

/// Bring every Defend beacon's quarry and picket leg up to date: the first
/// thing the programs phase does, so every guard and picket of a beacon reads
/// one answer this tick.
pub(crate) fn update(world: &mut World) {
    if world.defend().rows().is_empty() {
        return;
    }
    let mut index: usize = 0;
    while let Some(row) = world.defend().rows().get(index).copied() {
        index = index.saturating_add(1);
        let Some(post) = post_of(world, &row) else {
            continue;
        };
        let quarry = choose_quarry(world, &row, &post);
        let leg = next_leg(world, &row, &post);
        if let Some(slot) = world.defend_mut().row_mut(row.beacon) {
            slot.quarry = quarry;
            slot.leg = leg;
        }
    }
}

/// A candidate quarry: an enemy unit, its threat and its squared distance
/// from the tier's centre.
#[derive(Clone, Copy, Debug)]
struct Candidate {
    asset: AssetId,
    threat: Threat,
    d2: Sq,
}

impl Candidate {
    /// Whether `self` beats `held` (decision 12): the higher threat, then the
    /// nearer, then the lower id.
    fn beats(&self, held: &Candidate) -> bool {
        (core::cmp::Reverse(self.threat), self.d2, self.asset.raw())
            < (core::cmp::Reverse(held.threat), held.d2, held.asset.raw())
    }
}

/// The enemy units `seat` sees, with where each stands and its threat, in
/// ascending row: a quarry's candidates. Unarmed units are candidates too, at
/// no threat, so they rank after every armed one.
fn for_each_seen_enemy_unit<F: FnMut(AssetId, [Fx; 3], Threat)>(
    world: &World,
    seat: SeatId,
    mut each: F,
) {
    let units = world.units();
    let rows = units
        .seats()
        .iter()
        .zip(units.kinds())
        .zip(units.hit_points())
        .zip(units.positions())
        .zip(Row::all());
    for ((((owner, kind), hp), at), row) in rows {
        if !hp.is_alive() || !enemy(seat, *owner) || !world.seen_by(seat.raw(), *at) {
            continue;
        }
        let Some(asset) = units.id_of(row).and_then(|id| AssetId::of_unit(id).ok()) else {
            continue;
        };
        let weapon = UnitKind::from_id(*kind).and_then(|kind| world.rules().unit(kind).weapon);
        each(asset, *at, Threat::of(weapon));
    }
}

/// Where an enemy unit the seat sees stands, or `None` when it is dead, gone,
/// no enemy's or unseen.
fn seen_unit_at(world: &World, seat: SeatId, quarry: u32) -> Option<[Fx; 3]> {
    if quarry == NO_QUARRY {
        return None;
    }
    let asset = AssetId::new(quarry);
    if asset.tag() != AssetId::TAG_UNIT {
        return None;
    }
    let row = world.units().row_of(UnitId::new(asset.index())).ok()?;
    let index = row.index();
    let owner = world.units().seats().get(index).copied()?;
    let alive = world.units().hit_points().get(index)?.is_alive();
    let at = world.units().positions().get(index).copied()?;
    (alive && enemy(seat, owner) && world.seen_by(seat.raw(), at)).then_some(at)
}

/// The centres guards intercept threats near, in tier order (see the module
/// docs): the commander's while it is escorted and inside the sphere, then
/// each protected beacon's inside the sphere, then the beacon's own.
fn for_each_tier<F: FnMut([Fx; 3]) -> bool>(
    world: &World,
    row: &DefendRow,
    post: &Post,
    mut each: F,
) {
    let inside = |at: [Fx; 3]| Sq::between(at, post.centre) <= Sq::of_radius(post.sphere);
    if row.settings.escort
        && let Some(at) = commander_at(world, post.seat)
        && inside(at)
        && each(at)
    {
        return;
    }
    for protected in world.defend().protected_by(row.beacon) {
        let Some(at) = live_beacon_at(world, protected) else {
            continue;
        };
        if inside(at) && each(at) {
            return;
        }
    }
    each(post.centre);
}

/// Where a live beacon stands.
fn live_beacon_at(world: &World, beacon: BeaconId) -> Option<[Fx; 3]> {
    let at = world.beacons().row_of(beacon).ok()?.index();
    world
        .beacons()
        .hit_points()
        .get(at)?
        .is_alive()
        .then(|| world.beacons().positions().get(at).copied())?
}

/// Where `seat`'s living commander stands.
fn commander_at(world: &World, seat: SeatId) -> Option<[Fx; 3]> {
    let id = world.commander_of(seat)?;
    let at = world.units().row_of(id).ok()?.index();
    world
        .units()
        .hit_points()
        .get(at)?
        .is_alive()
        .then(|| world.units().positions().get(at).copied())?
}

/// The beacon's quarry this tick (see the module docs).
fn choose_quarry(world: &World, row: &DefendRow, post: &Post) -> u32 {
    if post.roe == Roe::Hold {
        return NO_QUARRY;
    }
    let attackers = (post.roe == Roe::Return).then(|| world.attackers_now());
    let allowed = |asset: AssetId| {
        attackers
            .as_ref()
            .is_none_or(|set| set.binary_search(&(post.seat.raw(), asset.raw())).is_ok())
    };
    let near = |centre: [Fx; 3], at: [Fx; 3]| Sq::between(centre, at) <= Sq::of_radius(post.engage);
    // Keep the quarry while pursue lets the guards follow it.
    if let Some(at) = seen_unit_at(world, post.seat, row.quarry)
        && allowed(AssetId::new(row.quarry))
    {
        let keep = match row.settings.pursue_reading() {
            Pursue::WhileInReach => true,
            Pursue::SphereEdge => Sq::between(at, post.centre) <= Sq::of_radius(post.sphere),
            Pursue::Never | Pursue::Unset => {
                let mut zoned = false;
                for_each_tier(world, row, post, |centre| {
                    zoned = near(centre, at);
                    zoned
                });
                zoned
            }
        };
        if keep {
            return row.quarry;
        }
    }
    let mut chosen = NO_QUARRY;
    for_each_tier(world, row, post, |centre| {
        let mut best: Option<Candidate> = None;
        for_each_seen_enemy_unit(world, post.seat, |asset, at, threat| {
            if !near(centre, at) || !allowed(asset) {
                return;
            }
            let found = Candidate {
                asset,
                threat,
                d2: Sq::between(centre, at),
            };
            if best.is_none_or(|held| found.beats(&held)) {
                best = Some(found);
            }
        });
        match best {
            Some(found) => {
                chosen = found.asset.raw();
                true
            }
            None => false,
        }
    });
    chosen
}

/// The living units of `kind` homed to `beacon`, in ascending row: a beacon's
/// guards or its pickets.
fn homed(world: &World, beacon: BeaconId, kind: UnitKind) -> impl Iterator<Item = u32> + '_ {
    let units = world.units();
    units
        .homes()
        .iter()
        .zip(units.kinds())
        .zip(units.hit_points())
        .zip(Row::all())
        .filter(move |(((home, unit_kind), hp), _)| {
            **home == beacon && **unit_kind == kind.id() && hp.is_alive()
        })
        .map(|(_, row)| row.raw())
}

/// The ring point picket `k` of `n` walks to on leg `leg`, or `None` when the
/// point is off the map.
fn picket_point(world: &World, post: &Post, leg: u8, k: usize, n: usize) -> Option<[Fx; 3]> {
    let legs = usize::from(PICKET_LEGS);
    let spread = k.checked_mul(legs)?.checked_div(n)?;
    let point = usize::from(leg).checked_add(spread)?.checked_rem(legs)?;
    let ring = i32::from(world.rules().sphere_radius_voxels())
        .checked_add(i32::from(world.rules().picket_radius_voxels()))?;
    // 181/256 is 1/sqrt(2) to three places: the diagonal points of the ring,
    // in integers (AGENTS.md section 4.2).
    let diagonal = ring.checked_mul(181)?.checked_div(256)?;
    let (dx, dy) = match point {
        0 => (ring, 0),
        1 => (diagonal, diagonal),
        2 => (0, ring),
        3 => (diagonal.checked_neg()?, diagonal),
        4 => (ring.checked_neg()?, 0),
        5 => (diagonal.checked_neg()?, diagonal.checked_neg()?),
        6 => (0, ring.checked_neg()?),
        _ => (diagonal, diagonal.checked_neg()?),
    };
    let [x, y, _] = post.centre;
    let column_x = x.floor_voxels().checked_add(dx)?;
    let column_y = y.floor_voxels().checked_add(dy)?;
    world.ground().standing(column_x, column_y)
}

/// The beacon's picket leg after this tick: one on once every picket stands
/// at its point (a picket whose point is off the map counts as standing
/// there, so the patrol walks past it), unchanged otherwise.
fn next_leg(world: &World, row: &DefendRow, post: &Post) -> u8 {
    let n = homed(world, row.beacon, UnitKind::Scout).count();
    if n == 0 {
        return row.leg;
    }
    let arrived = homed(world, row.beacon, UnitKind::Scout)
        .enumerate()
        .all(|(k, unit)| {
            let here = world
                .units()
                .positions()
                .get(crate::math::index::row(unit))
                .copied();
            match (picket_point(world, post, row.leg, k, n), here) {
                (Some(point), Some(here)) => {
                    crate::programs::within(here, point, world.arrive_radius())
                }
                _ => true,
            }
        });
    if arrived {
        // E7: rule: a leg is below `PICKET_LEGS`, so the next one wraps to 0
        row.leg
            .saturating_add(1)
            .checked_rem(PICKET_LEGS)
            .unwrap_or(0)
    } else {
        row.leg
    }
}

/// The Defend row of the beacon `unit` is homed to, with the beacon as a
/// post, or `None` when its home is no live Defend beacon.
fn post_of_unit(world: &World, unit: u32) -> Option<(DefendRow, Post)> {
    let home = world
        .units()
        .homes()
        .get(crate::math::index::row(unit))
        .copied()?;
    let row = *world.defend().row_of(home)?;
    let post = post_of(world, &row)?;
    Some((row, post))
}

/// Walk `unit` towards `to`, re-sending it only when its destination is not
/// already there, so a guard following a moving quarry does not ask for a new
/// route every tick it stays close to the old one.
fn steer(world: &mut World, unit: u32, to: [Fx; 3]) {
    let row = crate::math::index::row(unit);
    let held = world.units().destinations().get(row).copied();
    let radius = world.arrive_radius();
    if held.is_some_and(|held| crate::programs::within(held, to, radius)) {
        return;
    }
    world.send_unit(unit, to);
}

/// Whether `unit` is below its kind's full hit points.
fn damaged(world: &World, unit: u32) -> bool {
    let row = crate::math::index::row(unit);
    let (Some(kind), Some(hp)) = (
        world
            .units()
            .kinds()
            .get(row)
            .copied()
            .and_then(UnitKind::from_id),
        world.units().hit_points().get(row).copied(),
    ) else {
        return false;
    };
    hp < world.rules().unit(kind).hp
}

/// Where the nearest friendly live repair drone stands, ties to the lowest
/// row.
fn nearest_repair_drone(world: &World, seat: SeatId, here: [Fx; 3]) -> Option<[Fx; 3]> {
    let units = world.units();
    let mut best: Option<(Sq, [Fx; 3])> = None;
    let rows = units
        .kinds()
        .iter()
        .zip(units.seats())
        .zip(units.hit_points())
        .zip(units.positions());
    for (((kind, owner), hp), at) in rows {
        if *kind != UnitKind::RepairDrone.id() || *owner != seat.raw() || !hp.is_alive() {
            continue;
        }
        let d2 = Sq::between(here, *at);
        if best.is_none_or(|(held, _)| d2 < held) {
            best = Some((d2, *at));
        }
    }
    best.map(|(_, at)| at)
}

/// Fall back to the nearest friendly repair drone: `true` when the unit is
/// damaged and a drone stands somewhere to walk to, which is then the unit's
/// whole tick.
fn fall_back(world: &mut World, unit: u32, seat: SeatId) -> bool {
    if !damaged(world, unit) {
        return false;
    }
    let row = crate::math::index::row(unit);
    let Some(here) = world.units().positions().get(row).copied() else {
        return false;
    };
    let Some(drone) = nearest_repair_drone(world, seat, here) else {
        return false;
    };
    if crate::programs::within(here, drone, world.arrive_radius()) {
        world.park_unit(unit);
    } else {
        steer(world, unit, drone);
    }
    true
}

/// A raider's tick under a Defend beacon: `false` when its home is no Defend
/// beacon, so its program does what a raider does elsewhere.
pub(crate) fn run_guard(world: &mut World, unit: u32) -> bool {
    let Some((row, post)) = post_of_unit(world, unit) else {
        return false;
    };
    let index = crate::math::index::row(unit);
    let Some(here) = world.units().positions().get(index).copied() else {
        return true;
    };
    if let Some(at) = seen_unit_at(world, post.seat, row.quarry) {
        let reach = world
            .units()
            .kinds()
            .get(index)
            .copied()
            .and_then(UnitKind::from_id)
            .and_then(|kind| world.rules().unit(kind).weapon)
            .map(WeaponRow::range);
        match reach {
            Some(range) if crate::programs::within(here, at, range) => world.park_unit(unit),
            _ => steer(world, unit, at),
        }
        return true;
    }
    if fall_back(world, unit, post.seat) {
        return true;
    }
    let escorted = if row.settings.escort {
        commander_at(world, post.seat)
            .filter(|at| Sq::between(*at, post.centre) <= Sq::of_radius(post.sphere))
    } else {
        None
    };
    match escorted {
        Some(at) if crate::programs::within(here, at, world.arrive_radius()) => {
            world.park_unit(unit);
        }
        Some(at) => steer(world, unit, at),
        None => crate::programs::idle_at_home(world, unit),
    }
    true
}

/// A scout's tick under a Defend beacon, as a picket: `false` when its home is
/// no Defend beacon, so its program surveys as a scout does elsewhere.
pub(crate) fn run_picket(world: &mut World, unit: u32) -> bool {
    let Some((row, post)) = post_of_unit(world, unit) else {
        return false;
    };
    if fall_back(world, unit, post.seat) {
        return true;
    }
    let n = homed(world, row.beacon, UnitKind::Scout).count();
    let k = homed(world, row.beacon, UnitKind::Scout).position(|picket| picket == unit);
    let point = k.and_then(|k| picket_point(world, &post, row.leg, k, n));
    let Some(point) = point else {
        crate::programs::idle_at_home(world, unit);
        return true;
    };
    let here = world
        .units()
        .positions()
        .get(crate::math::index::row(unit))
        .copied();
    if here.is_some_and(|here| crate::programs::within(here, point, world.arrive_radius())) {
        world.park_unit(unit);
    } else {
        steer(world, unit, point);
    }
    true
}

/// The quarry a guard of `home` fires at, when its home is a Defend beacon and
/// the quarry is in `range` of `from` and seen: the combat phase's first
/// choice for a guard (decision 21).
#[must_use]
pub(crate) fn quarry_in_range(
    world: &World,
    home: BeaconId,
    from: [Fx; 3],
    range: Fx,
) -> Option<(UnitId, [Fx; 3])> {
    let row = world.defend().row_of(home)?;
    let seat = SeatId::new(
        world
            .beacons()
            .seats()
            .get(world.beacons().row_of(home).ok()?.index())
            .copied()?,
    );
    let at = seen_unit_at(world, seat, row.quarry)?;
    crate::programs::within(from, at, range)
        .then(|| (UnitId::new(AssetId::new(row.quarry).index()), at))
}

/// One entry of a Defend beacon's protect list as the compile reads it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Protect {
    /// `my_commander`: the guards escort the seat's commander while it is
    /// inside the sphere.
    Commander,
    /// One of the seat's own beacons, by name or selector, resolved once when
    /// the row commits and held from then on, as a step pins its selector.
    Beacon(crate::interpreter::BeaconSpec),
}

/// The Defend settings a committed settings row writes, each only when the
/// row names it: a proto3 scalar at its default is silent, a `picket_scouts`
/// written as 0 stands the picket down, and a protect list replaces the one
/// the beacon has only when it holds an entry (as Build's lists do).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct DefendEdit {
    /// `engagement_radius_pct`, 1 to 100.
    pub engagement_pct: Option<u8>,
    /// `pursue`.
    pub pursue: Option<Pursue>,
    /// `picket_scouts`.
    pub pickets: Option<u8>,
    /// The protect list, in order.
    pub protect: Option<Vec<Protect>>,
}
