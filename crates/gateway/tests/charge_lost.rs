// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The feed's sentence for a lost demolition charge (lane `settle`): the
//! sim announces `charge_lost` whenever a charge bought and carried is lost
//! before it is planted, its carrier killed (decision 9) or its breach let go
//! (the attack ended, the way opened, a retarget, a re-plan that found no
//! breach). The event carries no cause, so the sentence names none: a played
//! retreat (no carrier killed) and a played killed carrier both read it.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use pharmakos_gateway::surface::Surface;
use pharmakos_sim::attack::{Aim, AttackSettings, ChargeState};
use pharmakos_sim::math::quantity::Damage;
use pharmakos_sim::runner::{Layout, LayoutBeacon, LayoutMandateSettings, LayoutSettings};
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::tables::{BeaconId, SeatId, UnitId, UnitKind};
use pharmakos_sim::voxels::{Material, VoxelEdit};
use pharmakos_sim::world::{DamageOrder, DamageTarget};

use support::{array_of, call_in_lull, result, seat_token, text_of};

/// The sentence every `charge_lost` of seat 0 reads in its own feed.
const LOST: &str =
    "A demolition charge of seat 0 was lost before it was planted; it will never fire.";

/// A hosted match laid out for a raid: seat 0's Attack beacon at the field
/// between the cores with two raiders, `breach_allowed`, a retreat below 60 %
/// of its force, aimed at a point twenty-four columns west, inside a ring of
/// seat 1's wall three high, four columns out, filed for the Push's first
/// tick. The Push begun and
/// played until the breach's charge is carried: the surface and the beacon.
fn carrying() -> (Surface, BeaconId) {
    let mut surface = support::hosted(2, 240_000, 1);
    let (_, a) = support::core_beacon(&surface, 0);
    let (_, b) = support::core_beacon(&surface, 1);
    let field = [i32::midpoint(a[0], b[0]), i32::midpoint(a[1], b[1])];
    let inside = [field[0] - 24, field[1]];
    let world = surface.host().expect("hosted").world();
    let inside_z = world.standing_point([inside[0], inside[1], 0])[2].floor_voxels();
    let mut ring = Vec::new();
    for dy in -4..=4_i32 {
        for dx in -4..=4_i32 {
            if dx.abs() != 4 && dy.abs() != 4 {
                continue;
            }
            let (x, y) = (inside[0] + dx, inside[1] + dy);
            let top = world.standing_point([x, y, 0])[2].floor_voxels();
            for z in top..top + 3 {
                ring.push([x, y, z]);
            }
        }
    }
    let wall = Material::wall(SeatId::new(1), false).expect("a wall id");
    let aim = [inside[0], inside[1], inside_z].map(|axis| i16::try_from(axis).expect("a voxel"));
    surface
        .host_mut()
        .expect("hosted")
        .seed_layout_with_settings(
            &Layout {
                beacons: vec![LayoutBeacon {
                    seat: SeatId::new(0),
                    column: field,
                    mandate: MandateKind::Attack,
                    units: vec![(UnitKind::Raider, 2)],
                }],
            },
            &[LayoutSettings {
                beacon: 0,
                settings: LayoutMandateSettings::Attack(AttackSettings {
                    aim: Aim::Voxel(aim),
                    force_size: Some(2),
                    waves: 1,
                    breach: true,
                    retreat_pct: Some(60),
                    ..AttackSettings::default()
                }),
            }],
        )
        .expect("the lobby takes the layout");
    let post = *support::beacons_of(&surface, 0)
        .last()
        .expect("the Attack beacon");
    surface.begin_push().expect("the Push begins");
    for at in ring {
        surface
            .host_mut()
            .expect("hosted")
            .file_voxel_edit(VoxelEdit::Set {
                at,
                material: wall,
                by: None,
            })
            .expect("the queue takes a wall voxel");
    }
    for _ in 0..4_000 {
        if charge_of(&surface, post) == Some(ChargeState::Loaded) {
            return (surface, post);
        }
        assert_eq!(support::step(&mut surface, 1), 1, "the Push runs");
    }
    panic!("the breach's charge was never carried");
}

/// The charge of the breach `post` holds, if it holds one.
fn charge_of(surface: &Surface, post: BeaconId) -> Option<ChargeState> {
    surface
        .host()
        .expect("hosted")
        .world()
        .attack()
        .row_of(post)
        .and_then(|row| row.breach)
        .map(|breach| breach.charge)
}

/// The breach's carrier, and the other raider of seat 0.
fn raiders(surface: &Surface, post: BeaconId) -> (UnitId, UnitId) {
    let world = surface.host().expect("hosted").world();
    let carrier = world
        .attack()
        .row_of(post)
        .and_then(|row| row.breach)
        .expect("a breach")
        .carrier;
    let units = world.units();
    let other = units
        .ids()
        .iter()
        .zip(units.seats())
        .zip(units.homes())
        .find(|((id, seat), home)| **seat == 0 && **home == post && **id != carrier)
        .map(|((id, _), _)| UnitId::new(*id))
        .expect("the raider that is not the carrier");
    (UnitId::new(carrier), other)
}

/// Kill `unit` with damage from seat 1.
fn kill(surface: &mut Surface, unit: UnitId) {
    surface
        .host_mut()
        .expect("hosted")
        .file_damage(DamageOrder {
            target: DamageTarget::Unit(unit),
            amount: Damage::new(1_000_000).expect("a positive amount"),
            by: SeatId::new(1),
            source: None,
        })
        .expect("the damage is filed");
}

/// Every `charge_lost` sentence on seat 0's page of the segment's feed.
fn lost_sentences(surface: &mut Surface) -> Vec<String> {
    let token = seat_token(surface, 0);
    let mut left = support::LULL_MS;
    let response = call_in_lull(
        surface,
        &token,
        &mut left,
        "get_segment_feed",
        "{\"detail\":\"full\",\"limit\":256}",
    );
    array_of(&result(&response, "get_segment_feed"), "events")
        .iter()
        .filter(|event| text_of(event, "kind") == "charge_lost")
        .map(|event| text_of(event, "text"))
        .collect()
}

/// A retreat while the charge is carried: the raider that is not the carrier
/// killed, the force below 60 %, the attack over with no carrier killed. The
/// charge is lost, and the feed says so without naming a cause.
#[test]
fn a_charge_lost_to_a_retreat_reads_as_lost() {
    let (mut surface, post) = carrying();
    let (_, other) = raiders(&surface, post);
    kill(&mut surface, other);
    let _ = support::step(&mut surface, 5);
    assert_eq!(charge_of(&surface, post), None, "the breach is let go");
    assert_eq!(lost_sentences(&mut surface), vec![String::from(LOST)]);
}

/// The carrier killed while it carries the charge (decision 9): the same
/// sentence.
#[test]
fn a_charge_lost_with_its_carrier_reads_the_same() {
    let (mut surface, post) = carrying();
    let (carrier, _) = raiders(&surface, post);
    kill(&mut surface, carrier);
    let _ = support::step(&mut surface, 5);
    assert_eq!(lost_sentences(&mut surface), vec![String::from(LOST)]);
}
