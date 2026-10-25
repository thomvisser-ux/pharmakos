// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `World::seen_by` at the gate's load (item 148 (4)): what every weapon's
//! choice of target pays to ask "does my seat see that".
//!
//! `seen_by` is private to the sim's combat module, and this lane holds no
//! sim place, so the probe **restates its body** from the sim's public pieces
//! -- [`World::sensor_at`]'s walk and [`Sensor::sees`] under the
//! [`RadiusOnly`] model, the walk `crates/sim/src/world/combat.rs` writes out
//! -- and times it between ticks on the hosted world, read only. Its cost is
//! targets times sensors per armed asset per tick, so the probe reports two
//! sweeps:
//!
//! * **in range**: every armed asset (each live unit or structure whose kind
//!   carries a weapon, and each seat's core, whose autocannon fires) asks
//!   `seen_by` of every live enemy its rule walks (the core's units only; every
//!   other rule units, structures and beacons) inside its weapon's range: the
//!   most a choice of target asks, since the sim stops at the first table that
//!   answers and skips a gun on cooldown or holding fire;
//! * **every target**: the same assets ask it of every such enemy on the map, the bound the docs' "targets times sensors" names,
//!   and what a tick would pay if every enemy stood in range.
//!
//! The combat phase's own time, which includes the real calls, is the
//! segment's per-phase figure; the probe is what isolates the term.

use std::time::Instant;

use pharmakos_sim::math::fixed::{Fx, Sq};
use pharmakos_sim::sight::{RadiusOnly, Sensor};
use pharmakos_sim::tables::{SeatId, StructureKind, UnitKind};
use pharmakos_sim::world::World;

use crate::error::Error;

/// One sweep's counts and time.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Sweep {
    /// `seen_by` calls made.
    pub calls: u64,
    /// Sensors visited over those calls.
    pub visits: u64,
    /// Wall-clock time of the sweep, in nanoseconds, best of the probe's runs.
    pub ns: u128,
}

/// What one probe found.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SightProbe {
    /// Armed assets that asked.
    pub armed: u64,
    /// Sensors in the world.
    pub sensors: u64,
    /// The in-range sweep.
    pub in_range: Sweep,
    /// The every-target sweep.
    pub every_target: Sweep,
}

/// How many times each sweep is timed, the fastest kept.
const RUNS: usize = 3;

/// `seen_by`, restated: whether some sensor of `seat` reaches `point`, and how
/// many sensors the walk visited to say so.
fn seen_by(world: &World, seat: u8, point: [Fx; 3]) -> (bool, u64) {
    let mut at: usize = 0;
    let mut visits: u64 = 0;
    while let Some((sensor, next)) = world.sensor_at(at) {
        at = next;
        visits += 1;
        if sensor.seat.raw() == seat && Sensor::sees(&sensor, &RadiusOnly, point) {
            return (true, visits);
        }
    }
    (false, visits)
}

/// An asset that can be asked about or can ask: its seat and position, and its
/// weapon's range in voxels when it has one.
#[derive(Clone, Copy, Debug)]
struct Asset {
    seat: u8,
    at: [Fx; 3],
    range: Option<i16>,
    /// A structure or a beacon: what the sim's `for_each_enemy_site` walks.
    site: bool,
    /// The gun's rule asks about enemy units only: the core's autocannon
    /// (`Rule::UnitsOnly` in `crates/sim/src/world/combat.rs`). Every other
    /// rule reaches units and sites both.
    units_only: bool,
}

impl Asset {
    /// Whether `gun`'s rule could ask about `self`: an enemy, of a table the
    /// rule walks.
    fn askable_by(&self, gun: &Asset) -> bool {
        self.seat != gun.seat && !(gun.units_only && self.site)
    }
}

/// Every live unit, structure and beacon, with each armed one's range.
fn assets(world: &World) -> Vec<Asset> {
    let rules = world.rules();
    let units = world.units();
    let mut found: Vec<Asset> = Vec::new();
    for (((seat, kind), at), hp) in units
        .seats()
        .iter()
        .zip(units.kinds())
        .zip(units.positions())
        .zip(units.hit_points())
    {
        if !hp.is_alive() {
            continue;
        }
        let range = UnitKind::from_id(*kind)
            .and_then(|kind| rules.unit(kind).weapon)
            .map(|weapon| weapon.range_voxels);
        found.push(Asset {
            seat: *seat,
            at: *at,
            range,
            site: false,
            units_only: false,
        });
    }
    // A structure fires and is fired at while it is live and a seat's, as the
    // sim's `gun_of` and `for_each_enemy_site` read it.
    let structures = world.structures();
    for ((((seat, kind), at), hp), free) in structures
        .seats()
        .iter()
        .zip(structures.kinds())
        .zip(structures.positions())
        .zip(structures.hit_points())
        .zip(structures.frees())
    {
        if *free || !hp.is_alive() || !SeatId::new(*seat).is_some() {
            continue;
        }
        let range = StructureKind::from_id(*kind)
            .and_then(|kind| rules.structure(kind))
            .and_then(|row| row.weapon)
            .map(|weapon| weapon.range_voxels);
        found.push(Asset {
            seat: *seat,
            at: *at,
            range,
            site: true,
            units_only: false,
        });
    }
    let beacons = world.beacons();
    let core_range = rules.core_weapon().range_voxels;
    let mut cored: Vec<u8> = Vec::new();
    // A seat's core is its lowest-id beacon row (`crates/sim/src/power.rs`'s
    // `core_of`); rows are walked in id order, so the first of each seat's.
    let mut rows: Vec<(u32, u8, [Fx; 3], bool)> = beacons
        .ids()
        .iter()
        .zip(beacons.seats())
        .zip(beacons.positions())
        .zip(beacons.hit_points())
        .map(|(((id, seat), at), hp)| (*id, *seat, *at, hp.is_alive()))
        .collect();
    rows.sort_unstable_by_key(|(id, ..)| *id);
    for (_, seat, at, alive) in rows {
        let is_core = !cored.contains(&seat);
        if is_core {
            cored.push(seat);
        }
        if !alive {
            continue;
        }
        found.push(Asset {
            seat,
            at,
            range: is_core.then_some(core_range),
            site: true,
            units_only: is_core,
        });
    }
    found
}

/// One timed pass: every armed asset asks about every enemy asset `within`
/// admits.
fn pass(world: &World, assets: &[Asset], within: &impl Fn(&Asset, &Asset) -> bool) -> Sweep {
    let mut calls: u64 = 0;
    let mut visits: u64 = 0;
    let started = Instant::now();
    for gun in assets.iter().filter(|asset| asset.range.is_some()) {
        for target in assets.iter().filter(|asset| asset.askable_by(gun)) {
            if !within(gun, target) {
                continue;
            }
            let (seen, walked) = seen_by(world, gun.seat, target.at);
            std::hint::black_box(seen);
            calls += 1;
            visits += walked;
        }
    }
    let ns = started.elapsed().as_nanos();
    Sweep { calls, visits, ns }
}

/// One sweep, [`RUNS`] passes, the fastest kept (the counts are the same on
/// every pass: the world does not move between them).
fn sweep(world: &World, assets: &[Asset], within: impl Fn(&Asset, &Asset) -> bool) -> Sweep {
    let mut best = pass(world, assets, &within);
    for _ in 1..RUNS {
        let run = pass(world, assets, &within);
        if run.ns < best.ns {
            best = run;
        }
    }
    best
}

/// Probe `seen_by` on `world` as it stands between two ticks.
///
/// # Errors
///
/// [`Error::Empty`] when nothing in the world is armed, which would make both
/// sweeps a measurement of nothing.
pub fn probe(world: &World) -> Result<SightProbe, Error> {
    let assets = assets(world);
    let armed = assets.iter().filter(|asset| asset.range.is_some()).count();
    if armed == 0 {
        return Err(Error::Empty(
            "no armed asset stands in the world".to_owned(),
        ));
    }
    let mut sensors: u64 = 0;
    let mut at: usize = 0;
    while let Some((_, next)) = world.sensor_at(at) {
        at = next;
        sensors += 1;
    }
    let in_range = sweep(world, &assets, |gun, target| {
        gun.range.is_some_and(|range| {
            Sq::between(gun.at, target.at) <= Sq::of_radius(Fx::from_voxels(range))
        })
    });
    let every_target = sweep(world, &assets, |_, _| true);
    Ok(SightProbe {
        armed: u64::try_from(armed).map_err(|_| Error::Overflow("the armed count"))?,
        sensors,
        in_range,
        every_target,
    })
}
