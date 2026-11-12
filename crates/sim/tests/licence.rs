// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Ledger's licences (S3's `lic`; S3's plan, E2 and decision 6).
//!
//! * the licence is earned at a settlement when two of the seat's finished,
//!   powered Generators stand on two **different** vents, and not for two on
//!   one vent, one alone, or one still going up;
//! * the award is in the settlement tick's hash and survives a save and a
//!   restore;
//! * an eliminated seat's licences are voided on the tick it falls;
//! * a table holding no licence hashes nothing for it, so a chain whose seats
//!   earn nothing does not move (item 136's block rule; the committed goldens
//!   are the rest of that proof).

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report, and that covers an out-of-bounds index in an assertion for the same reason: these are panic lints, not determinism lints."
)]

use std::path::PathBuf;

use pharmakos_proto::gp::v1::Licence;
use pharmakos_sim::encoding::Enc;
use pharmakos_sim::events::{Event, EventKind};
use pharmakos_sim::features::FeatureKind;
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::math::fixed::Sq;
use pharmakos_sim::math::quantity::Damage;
use pharmakos_sim::runner::Runner;
use pharmakos_sim::snapshot::Snapshot;
use pharmakos_sim::tables::{BeaconId, SeatId, StructureKind};
use pharmakos_sim::{
    DamageOrder, DamageTarget, MatchSettings, RulesTable, World, WorldConfig, default_rules_path,
};

/// The golden seed every fixture plays (`scenarios/`' `0xca5caded`).
const SEED: u64 = 0x0000_0000_ca5c_aded;

fn rules() -> RulesTable {
    let path = default_rules_path().unwrap_or_else(|| PathBuf::from("rules/rules.v1.json"));
    RulesTable::load(&path).expect("the committed rules table loads")
}

fn world(seats: u32) -> World {
    World::new(&WorldConfig {
        match_seed: SEED,
        seats,
        rules: rules(),
        match_settings: MatchSettings {
            segment_lengths_ms: vec![10_000, 10_000, 10_000],
            round_limit: 3,
        },
    })
    .expect("the world generates")
}

fn core_of(world: &World, seat: SeatId) -> BeaconId {
    let index = world
        .beacons()
        .seats()
        .iter()
        .position(|owner| *owner == seat.raw())
        .expect("every occupied seat has a core");
    BeaconId::new(world.beacons().ids()[index])
}

fn at_of(world: &World, beacon: BeaconId) -> [Fx; 3] {
    let row = world.beacons().row_of(beacon).expect("a live beacon");
    world.beacons().positions()[row.index()]
}

/// The standing points of `count` different vents, nearest `from` first, each
/// on a footprint column whose top voxel is vent material.
fn vent_stands(world: &World, from: [Fx; 3], count: usize) -> Vec<[Fx; 3]> {
    let mut vents: Vec<(Sq, usize, [Fx; 3])> = Vec::new();
    for index in 0..world.features().len() {
        let feature = world.features().get(index).expect("a feature");
        if feature.kind != FeatureKind::Vent {
            continue;
        }
        let stand = feature.footprint.iter().find_map(|column| {
            let z = world.voxels().top_solid_z(column.x, column.y)?;
            world
                .voxels()
                .get([column.x, column.y, z])
                .and_then(pharmakos_sim::Material::vent_richness)?;
            Some([
                Fx::from_voxels(i16::try_from(column.x).ok()?),
                Fx::from_voxels(i16::try_from(column.y).ok()?),
                Fx::from_voxels(i16::try_from(z + 1).ok()?),
            ])
        });
        if let Some(stand) = stand {
            // Ties to the lower feature index: the key ends in a unique id.
            vents.push((Sq::between(from, stand), index, stand));
        }
    }
    vents.sort_unstable_by_key(|(distance, index, _)| (*distance, *index));
    assert!(vents.len() >= count, "the map holds {count} vents");
    vents.into_iter().take(count).map(|(_, _, at)| at).collect()
}

/// Play the Push in progress to its end, keeping every event.
fn play_segment(runner: &mut Runner, feed: &mut Vec<Event>) {
    if runner.phase() == pharmakos_sim::MatchPhase::Recap {
        assert!(
            runner.end_recap().expect("the runner answers"),
            "the recap closes"
        );
    }
    assert!(
        runner.begin_push().expect("the runner answers"),
        "the Push begins"
    );
    feed.extend_from_slice(runner.events());
    runner.clear_events();
    while runner
        .step()
        .expect("the tick runs")
        .is_some_and(|report| !report.segment_ended)
    {
        feed.extend_from_slice(runner.events());
        runner.clear_events();
    }
    feed.extend_from_slice(runner.events());
    runner.clear_events();
}

fn awards(feed: &[Event]) -> Vec<(u8, i64)> {
    feed.iter()
        .filter(|event| event.kind == EventKind::LicenceAwarded)
        .map(|event| (event.seat.map_or(u8::MAX, SeatId::raw), event.value))
        .collect()
}

/// A runner whose seat 0 has a finished Generator on each of `stands`, all
/// homed to its lit core.
fn with_generators(stands: &[[Fx; 3]]) -> Runner {
    let mut world = world(2);
    let seat = SeatId::new(0);
    let core = core_of(&world, seat);
    for at in stands {
        world
            .raise_structure(seat, core, StructureKind::Generator, *at)
            .expect("room for a Generator");
    }
    Runner::new(world)
}

#[test]
fn generators_on_two_vents_earn_the_licence_at_the_settlement() {
    let probe = world(2);
    let core = core_of(&probe, SeatId::new(0));
    let stands = vent_stands(&probe, at_of(&probe, core), 2);
    let mut runner = with_generators(&stands);
    let mut feed = Vec::new();
    play_segment(&mut runner, &mut feed);

    assert_eq!(
        awards(&feed),
        vec![(0, 1)],
        "seat 0 earns the Resonance Spire's licence, and seat 1, with no Generator, nothing"
    );
    let settled = feed
        .iter()
        .rposition(|event| event.kind == EventKind::Settled)
        .expect("the Ledger settles");
    let awarded = feed
        .iter()
        .position(|event| event.kind == EventKind::LicenceAwarded)
        .expect("the award");
    assert!(awarded > settled, "the check runs after the income");
    let world = runner.world();
    assert!(
        world
            .licences()
            .holds(SeatId::new(0), Licence::ResonanceSpire)
    );
    assert_eq!(world.licences().rows()[0].round, 1, "round 1's settlement");

    // A licence is permanent and never awarded twice.
    let mut feed = Vec::new();
    play_segment(&mut runner, &mut feed);
    assert!(awards(&feed).is_empty(), "no second award: {feed:?}");
    assert_eq!(runner.world().licences().rows().len(), 1);
}

#[test]
fn two_generators_on_one_vent_or_one_alone_earn_nothing() {
    let probe = world(2);
    let core = core_of(&probe, SeatId::new(0));
    let first = vent_stands(&probe, at_of(&probe, core), 1)[0];
    // A second column of the same vent's footprint.
    let x = first[0].floor_voxels();
    let y = first[1].floor_voxels();
    let vent = probe
        .features()
        .at_column(x, y)
        .expect("the stand is on a vent");
    let second = probe
        .features()
        .get(vent)
        .expect("the vent")
        .footprint
        .iter()
        .filter(|column| (column.x, column.y) != (x, y))
        .find_map(|column| {
            let z = probe.voxels().top_solid_z(column.x, column.y)?;
            probe
                .voxels()
                .get([column.x, column.y, z])
                .and_then(pharmakos_sim::Material::vent_richness)?;
            Some([
                Fx::from_voxels(i16::try_from(column.x).ok()?),
                Fx::from_voxels(i16::try_from(column.y).ok()?),
                Fx::from_voxels(i16::try_from(z + 1).ok()?),
            ])
        })
        .expect("a vent of more than one column");

    for stands in [vec![first], vec![first, second]] {
        let mut runner = with_generators(&stands);
        let mut feed = Vec::new();
        play_segment(&mut runner, &mut feed);
        assert!(
            awards(&feed).is_empty(),
            "{} Generator(s) on one vent earn nothing",
            stands.len()
        );
        assert!(runner.world().licences().is_empty());
    }
}

#[test]
fn a_licence_held_survives_a_save_and_a_restore() {
    let probe = world(2);
    let core = core_of(&probe, SeatId::new(0));
    let stands = vent_stands(&probe, at_of(&probe, core), 2);
    let mut runner = with_generators(&stands);
    let mut feed = Vec::new();
    play_segment(&mut runner, &mut feed);
    let saved = runner.world().clone();
    assert!(!saved.licences().is_empty(), "a licence is held");

    let bytes = Snapshot::capture(&saved).to_bytes().expect("encoding");
    let decoded = Snapshot::from_bytes(&bytes).expect("decoding");
    assert_eq!(decoded.licence_seat, vec![0]);
    assert_eq!(decoded.licence_kind, vec![1]);
    assert_eq!(decoded.licence_round, vec![1]);
    let mut restored = world(2);
    decoded.restore_into(&mut restored).expect("restoring");
    assert_eq!(restored.licences(), saved.licences());
    assert_eq!(restored.state_hash(), saved.state_hash());

    // And it keeps agreeing past the restore.
    let mut enc = Enc::with_capacity(64 * 1024);
    let mut continued = saved.clone();
    assert_eq!(
        restored.step(&mut enc).expect("a tick"),
        continued.step(&mut enc).expect("a tick")
    );
}

#[test]
fn a_restore_refuses_a_licence_no_run_writes() {
    let mut world = world(2);
    world.award_licence_directly(SeatId::new(1), Licence::ResonanceSpire, 1);
    let good = Snapshot::capture(&world);

    let mut unknown = good.clone();
    unknown.licence_kind = vec![0];
    let mut ragged = good.clone();
    ragged.licence_round = Vec::new();
    let mut stranger = good.clone();
    stranger.licence_seat = vec![7];
    let mut twice = good.clone();
    twice.licence_seat = vec![1, 1];
    twice.licence_kind = vec![1, 1];
    twice.licence_round = vec![1, 2];
    let cases: Vec<(&str, Snapshot)> = vec![
        ("an unknown licence", unknown),
        ("a ragged table", ragged),
        ("a seat the file does not hold", stranger),
        ("a licence held twice", twice),
    ];
    for (name, snapshot) in cases {
        let mut into = self::world(2);
        assert!(
            snapshot.restore_into(&mut into).is_err(),
            "{name} is refused"
        );
        assert!(into.licences().is_empty(), "{name}: nothing written");
    }
    let mut into = self::world(2);
    good.restore_into(&mut into)
        .expect("the good file restores");
    assert!(
        into.licences()
            .holds(SeatId::new(1), Licence::ResonanceSpire)
    );
}

#[test]
fn an_eliminated_seats_licences_are_voided_on_the_tick_it_falls() {
    let mut world = world(3);
    world.award_licence_directly(SeatId::new(2), Licence::ResonanceSpire, 1);
    world.award_licence_directly(SeatId::new(0), Licence::ResonanceSpire, 1);
    let mut runner = Runner::new(world);
    assert!(runner.begin_push().expect("the runner answers"));
    runner.clear_events();
    let core = core_of(runner.world(), SeatId::new(2));
    runner
        .world_mut()
        .request_damage(DamageOrder {
            target: DamageTarget::Beacon(core),
            amount: Damage::new(1_000_000).expect("a damage"),
            by: SeatId::NEUTRAL,
            source: None,
        })
        .expect("the damage queue has room");
    runner.step().expect("the tick runs").expect("a tick");
    let events = runner.events();
    let eliminated = events
        .iter()
        .position(|event| event.kind == EventKind::SeatEliminated)
        .expect("seat 2 falls");
    let voided = events
        .iter()
        .position(|event| event.kind == EventKind::LicenceVoided)
        .expect("its licence is voided on the same tick");
    assert!(voided > eliminated);
    assert_eq!(events[voided].seat, Some(SeatId::new(2)));
    assert_eq!(events[voided].value, 1);
    let licences = runner.world().licences();
    assert!(!licences.holds(SeatId::new(2), Licence::ResonanceSpire));
    assert!(
        licences.holds(SeatId::new(0), Licence::ResonanceSpire),
        "a standing seat keeps its own"
    );
}

#[test]
fn an_empty_licence_table_hashes_nothing() {
    // Item 136's block rule: the block is absent while the table is empty, so
    // every chain committed before the table existed holds; a licence moves the
    // encoding by exactly the block's bytes.
    let mut world = world(2);
    let mut enc = Enc::with_capacity(64 * 1024);
    world.encode(&mut enc);
    let bare = enc.as_bytes().to_vec();
    world.award_licence_directly(SeatId::new(0), Licence::ResonanceSpire, 1);
    world.encode(&mut enc);
    let held = enc.as_bytes().to_vec();
    assert_eq!(
        &held[..bare.len()],
        &bare[..],
        "everything before the block is unchanged"
    );
    assert_eq!(
        held.len() - bare.len(),
        1 + 4 + 1 + 1 + 4,
        "the tag, the count and one row of seat, licence and round"
    );
}
