// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Placing a lobby layout (S2's plan, E30; S3's plan, E13), out of `World`
//! (lane `settle`): the placement reads and writes the world only through a
//! [`Placement`], a view whose world is a private field of its own module, so
//! nothing here can name the state the snapshot's door reads (the open phase
//! and the pathing graph's repairer). The door passes a world once, before
//! placement ([`crate::snapshot::Snapshot::door_for_layout`]), and the world
//! the placement hands back is the closed world it was given.

mod placement;

pub(crate) use placement::Placement;

use crate::math::fixed::Fx;
use crate::runner::{Layout, LayoutMandateSettings, LayoutRefused, LayoutSetting, LayoutSettings};
use crate::seams::MandateKind;
use crate::tables::{SeatId, UnitKind};

/// Place a lobby layout: every beacon of it, with its mandate, its settings
/// rows (S3's plan, E13) and its starting units fabricated at it, or
/// nothing (S2's plan, E30). The runner is the caller, and checks the lobby
/// ([`crate::runner::Runner::seed_layout_with_settings`]).
///
/// Through the constructors play uses -- [`World::place_beacon`](crate::world::World::place_beacon) and
/// [`World::fabricate`](crate::world::World::fabricate) -- so a layout's beacons and units are rows like
/// any other, inside each seat's shares of the world totals. Everything is
/// checked before the first row is written, so a refused layout leaves the
/// world as it was.
///
/// # Errors
///
/// [`LayoutRefused`]'s world refusals: a seat this match has not got, a
/// column with no ground to stand on, a share that cannot hold it, and a
/// settings row it cannot take ([`check_layout_settings`]).
pub(crate) fn place(
    mut view: Placement<'_>,
    layout: &Layout,
    settings: &[LayoutSettings],
) -> Result<(), LayoutRefused> {
    check_layout_settings(&view, layout, settings)?;
    let seats = view.seat_count();
    let beacon_share = view.beacon_share();
    let no_room = |what: &'static str| LayoutRefused::NoRoom { what };
    if layout.beacons.iter().any(|beacon| {
        beacon
            .units
            .iter()
            .any(|(kind, _)| *kind == UnitKind::Commander)
    }) {
        return Err(LayoutRefused::Commander);
    }
    let mut sites: Vec<[Fx; 3]> = Vec::with_capacity(layout.beacons.len());
    for (index, beacon) in layout.beacons.iter().enumerate() {
        if u32::from(beacon.seat.raw()) >= seats {
            return Err(LayoutRefused::NoSuchSeat { seat: beacon.seat });
        }
        let [x, y] = beacon.column;
        let site = view.standing(x, y).ok_or(LayoutRefused::OffTheMap {
            column: beacon.column,
        })?;
        sites.push(site);
        // This seat's beacons and units in the layout up to and including
        // this one, on top of what it holds already.
        let of_seat = || {
            layout
                .beacons
                .iter()
                .take(index.saturating_add(1))
                .filter(|other| other.seat == beacon.seat)
        };
        let placed = u32::try_from(of_seat().count())
            .ok()
            .and_then(|count| {
                view.beacons()
                    .count_of(beacon.seat)
                    .checked_sub(1)
                    .and_then(|held| held.checked_add(count))
            })
            .ok_or(no_room("beacon"))?;
        if placed > beacon_share {
            return Err(no_room("beacon"));
        }
        let mut units: u32 = view.units_held_by(beacon.seat);
        for other in of_seat() {
            for (_, count) in &other.units {
                units = units.checked_add(*count).ok_or(no_room("unit"))?;
            }
        }
        if units > view.unit_room_of(beacon.seat) {
            return Err(no_room("unit"));
        }
    }
    let beacons = u32::try_from(layout.beacons.len())
        .ok()
        .and_then(|count| view.beacons().len().checked_add(count))
        .ok_or(no_room("beacon"))?;
    if beacons > view.beacon_limit() {
        return Err(no_room("beacon"));
    }
    for (index, (beacon, site)) in layout.beacons.iter().zip(sites).enumerate() {
        let id = view
            .place_beacon(beacon.seat, site, beacon.mandate)
            .map_err(|_| no_room("beacon"))?;
        if let Some(row) = settings.iter().find(|row| row.beacon == index) {
            let LayoutMandateSettings::Attack(attack) = row.settings;
            if !view.set_attack(id, attack) {
                return Err(LayoutRefused::WrongWrit { index });
            }
        }
        for (kind, count) in &beacon.units {
            for _ in 0..*count {
                view.fabricate(beacon.seat, id, *kind)
                    .ok_or(no_room("unit"))?;
            }
        }
    }
    Ok(())
}

/// Every settings row of a layout checked against the layout and this
/// world, before anything is placed. The target a row names is a beacon the
/// world already holds, a core for a lobby layout: another row's beacon has
/// no id until it is placed.
///
/// # Errors
///
/// [`LayoutRefused::NoSuchLayoutBeacon`], [`LayoutRefused::SettingsTwice`],
/// [`LayoutRefused::WrongWrit`] and [`LayoutRefused::BadSetting`].
fn check_layout_settings(
    view: &Placement<'_>,
    layout: &Layout,
    settings: &[LayoutSettings],
) -> Result<(), LayoutRefused> {
    for (at, row) in settings.iter().enumerate() {
        let index = row.beacon;
        let beacon = layout
            .beacons
            .get(index)
            .ok_or(LayoutRefused::NoSuchLayoutBeacon { index })?;
        if settings.iter().take(at).any(|other| other.beacon == index) {
            return Err(LayoutRefused::SettingsTwice { index });
        }
        let LayoutMandateSettings::Attack(attack) = row.settings;
        if beacon.mandate != MandateKind::Attack {
            return Err(LayoutRefused::WrongWrit { index });
        }
        check_layout_attack(view, beacon.seat, attack)
            .map_err(|setting| LayoutRefused::BadSetting { index, setting })?;
    }
    Ok(())
}

/// What the compile refuses of a playbook's Attack row
/// (`compile_attack_settings`), asked of a layout's, with its target read
/// against this world. A playbook's empty Attack arm is legal, and a
/// layout's is not: a row exists to make a force raid, so one with no target
/// or no launch condition is refused.
fn check_layout_attack(
    view: &Placement<'_>,
    seat: SeatId,
    attack: crate::attack::AttackSettings,
) -> Result<(), LayoutSetting> {
    use crate::attack::Aim;
    let last = view.rules().map_last_voxel();
    let on_the_map = |at: [i16; 3]| {
        at.iter()
            .zip(last)
            .all(|(axis, limit)| (0..=limit).contains(axis))
    };
    let seats = view.seat_count();
    let aimed = match attack.aim {
        Aim::Unset => false,
        Aim::Commander(None) | Aim::NearestKnownBeacon => true,
        Aim::Commander(Some(target)) => target != seat.raw() && u32::from(target) < seats,
        Aim::Voxel(at) => on_the_map(at),
        Aim::Beacon(target) => view.beacons().row_of(target).is_ok_and(|row| {
            let index = row.index();
            let enemy = view
                .beacons()
                .seats()
                .get(index)
                .is_some_and(|owner| *owner != seat.raw());
            let alive = view
                .beacons()
                .hit_points()
                .get(index)
                .is_some_and(|hp| hp.is_alive());
            enemy && alive
        }),
    };
    if !aimed {
        return Err(LayoutSetting::Aim);
    }
    if attack.force_size.is_none() && attack.launch_ms.is_none() {
        return Err(LayoutSetting::Launch);
    }
    if attack.staging.is_some_and(|at| !on_the_map(at)) {
        return Err(LayoutSetting::Staging);
    }
    if attack.launch_ms.is_some_and(|ms| ms < 0) {
        return Err(LayoutSetting::LaunchMs);
    }
    if attack
        .retreat_pct
        .is_some_and(|pct| !(1..=100).contains(&pct))
    {
        return Err(LayoutSetting::RetreatPct);
    }
    if attack.retreat_ms.is_some_and(|ms| ms < 0) {
        return Err(LayoutSetting::RetreatMs);
    }
    Ok(())
}
