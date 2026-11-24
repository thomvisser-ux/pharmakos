// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Targeting's determinism and behaviour (S1's plan, task `tgt`;
//! `docs/design/targeting.md`; decisions-log item 127 (12) and (13)).
//!
//! The vocabulary landed with S1's targeting proto (`con2`), whose
//! `tests/targeting_refused.rs` pinned the two new sites refused until their
//! behaviour existed; this file replaces it with the behaviour. The resolver's
//! own two rules -- "nearest" is travel, not distance, and ties go to the
//! lowest anchor y then x -- are pinned against synthetic maps beside the
//! resolver (`src/targeting.rs`); this file plays them in a world.
//!
//! The world is the golden seed `0x00000000ca5caded` at two seats, the map
//! every committed S1 scenario plays: seat 0's core stands at (358, 24) and
//! its starting vent is `vent_324_16`, about 35 voxels away, beyond the core's
//! own 24-voxel sphere and inside the reach of a beacon placed at its edge.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report."
)]

use std::path::PathBuf;

use pharmakos_proto::gp;
use pharmakos_proto::json;
use pharmakos_sim::encoding::Enc;
use pharmakos_sim::events::{Event, EventKind};
use pharmakos_sim::features::FeatureKind;
use pharmakos_sim::interpreter::state::{StepFailed, StepFailedError, StepFailure};
use pharmakos_sim::interpreter::{BeaconSpec, Plan, PlanError, resolve_beacon_in};
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::pathing::search::Scratch;
use pharmakos_sim::runner::{MatchPhase, MatchSettings, Runner};
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::snapshot::Snapshot;
use pharmakos_sim::tables::{
    BeaconId, PRIORITY_NORMAL, SeatId, StructureKind, UnitId, own_beacon_name,
};
use pharmakos_sim::targeting::{
    FeatureSpec, NO_FEATURE, Ranker, anchor_in_sphere, covering_site, on_vent_counted,
};
use pharmakos_sim::voxels::{Material, VoxelEdit};
use pharmakos_sim::world::{DamageOrder, DamageTarget, room_per_seat};
use pharmakos_sim::{RulesTable, World, WorldConfig};

/// The golden seed: every committed S1 scenario's map.
const SEED: u64 = 0x0000_0000_ca5c_aded;

/// The seat every test drives.
const SEAT: u8 = 0;

/// Seat 0's starting vent on the golden seed at two seats.
const START_VENT: &str = "vent_324_16";

fn repo_root() -> PathBuf {
    let here = PathBuf::from(".");
    if here.join("rules").join("rules.v1.json").is_file() {
        return here;
    }
    PathBuf::from("..").join("..")
}

fn rules() -> RulesTable {
    RulesTable::load(&repo_root().join("rules").join("rules.v1.json"))
        .expect("the committed rules table loads")
}

fn world_at(seed: u64, seats: u32, segments: &[i32]) -> World {
    World::new(&WorldConfig {
        match_seed: seed,
        seats,
        rules: rules(),
        match_settings: MatchSettings {
            segment_lengths_ms: segments.to_vec(),
            round_limit: u32::try_from(segments.len()).expect("a few segments"),
        },
    })
    .expect("the rules table describes a map")
}

/// A whole playbook around `route`, as canonical `gp.v1` JSON.
fn playbook(route: &str) -> gp::v1::Playbook {
    let text = format!(
        concat!(
            "{{\"schema_version\":{{\"major\":1}},",
            "\"meta\":{{\"title\":\"targeting\",\"author_kind\":\"HUMAN\"}},",
            "\"declarative\":{{\"route\":[{route}]}},",
            "\"on_death\":{{\"on_respawn\":\"CONTINUE\",\"max_deaths_before_fallback\":3}},",
            "\"fallback\":{{\"hold\":{{\"at\":{{\"safest\":{{}}}}}}}},",
            "\"kind\":\"PLAYBOOK\"}}"
        ),
        route = route
    );
    json::decode(&text).unwrap_or_else(|error| panic!("the case decodes: {error}\n{text}"))
}

fn compile(route: &str) -> Result<Plan, PlanError> {
    Plan::compile(&playbook(route), &rules())
}

/// A runner on `world` with seat 0's `route` sealed, its first Push open.
fn open(world: World, route: &str) -> Runner {
    let plan = compile(route).unwrap_or_else(|error| panic!("the route compiles: {error}"));
    let mut runner = Runner::new(world).unwrap_or_else(|error| panic!("{error}"));
    runner
        .seal_playbook(SeatId::new(SEAT), plan)
        .expect("a Lull takes a seal");
    assert!(runner.begin_push().unwrap(), "a match opens in a Lull");
    runner
}

/// Play the Push in progress to its end (or `limit` ticks), keeping seat 0's
/// events. `each` sees the world after every tick.
fn play(runner: &mut Runner, limit: u32, mut each: impl FnMut(&mut World, &[Event])) -> Vec<Event> {
    let mut kept: Vec<Event> = Vec::new();
    let mut ticks: u32 = 0;
    while ticks < limit && runner.phase() == MatchPhase::Push {
        let Some(_) = runner.step().unwrap() else {
            break;
        };
        let events = runner.events().to_vec();
        runner.clear_events();
        each(runner.world_mut(), &events);
        kept.extend(
            events
                .into_iter()
                .filter(|event| event.seat == Some(SeatId::new(SEAT))),
        );
        ticks += 1;
    }
    kept
}

fn failures(events: &[Event]) -> Vec<i64> {
    events
        .iter()
        .filter(|event| event.kind == EventKind::StepFailed)
        .map(|event| event.value)
        .collect()
}

fn count(events: &[Event], kind: EventKind) -> usize {
    events.iter().filter(|event| event.kind == kind).count()
}

fn id(failure: StepFailure) -> i64 {
    i64::from(failure.id())
}

/// Seat 0's core column on the golden seed, and its standing height.
fn core_voxel(world: &World) -> [i32; 3] {
    core_voxel_of(world, SeatId::new(SEAT))
}

/// `seat`'s core column, and its standing height.
fn core_voxel_of(world: &World, seat: SeatId) -> [i32; 3] {
    let row = world
        .beacons()
        .row_of_ordinal(seat, 0)
        .expect("a seat's core is its b_00");
    world
        .beacons()
        .positions()
        .get(row)
        .copied()
        .expect("a position")
        .map(Fx::floor_voxels)
}

/// The core of `seat`, as a beacon id.
fn core_of(world: &World, seat: SeatId) -> BeaconId {
    let row = world
        .beacons()
        .row_of_ordinal(seat, 0)
        .expect("a seat's core is its b_00");
    BeaconId::new(u32::try_from(row).expect("a few beacons"))
}

/// The standing voxel `dx` east and `dy` north of `seat`'s core.
fn beside(world: &World, seat: SeatId, dx: i32, dy: i32) -> [i32; 3] {
    let core = core_voxel_of(world, seat);
    let (x, y) = (core[0] + dx, core[1] + dy);
    [x, y, world.voxels().standing_z(x, y)]
}

/// Step a Push up to `limit` ticks, keeping every seat's events.
fn play_all(runner: &mut Runner, limit: u32) -> Vec<Event> {
    let mut kept: Vec<Event> = Vec::new();
    let mut ticks: u32 = 0;
    while ticks < limit && runner.phase() == MatchPhase::Push {
        if runner.step().unwrap().is_none() {
            break;
        }
        kept.extend(runner.events().iter().copied());
        runner.clear_events();
        ticks += 1;
    }
    kept
}

/// Every `StructureQueued` of `seat`, by where it stands.
fn queued_by(events: &[Event], seat: SeatId) -> Vec<Option<[Fx; 3]>> {
    events
        .iter()
        .filter(|event| event.kind == EventKind::StructureQueued && event.seat == Some(seat))
        .map(|event| event.at.map(pharmakos_sim::knowledge::Position::to_array))
        .collect()
}

fn point(voxel: [i32; 3]) -> [Fx; 3] {
    voxel.map(|axis| Fx::from_voxels(i16::try_from(axis).expect("on the map")))
}

/// The voxel `dx` east of seat 0's core, standing on the ground.
fn beside_core(world: &World, dx: i32) -> [i32; 3] {
    let core = core_voxel(world);
    let x = core[0] + dx;
    let y = core[1];
    [x, y, world.voxels().standing_z(x, y)]
}

/// The covering step: the nearest vent seat 0 does not yet cover, with a
/// Generator on the vent it chose.
const COVER: &str = concat!(
    r#"{"label":"cover","place_beacon":{"at":{"covering":{"vent":{"rank":"NEAREST","coverage":"UNCOVERED"}}},"#,
    r#""initial":{"mandate":{"build":{"targets":[{"blueprint_id":"generator","anchor":{"on":{"covered":{}}}}]}}}},"#,
    r#""timeout_ms":120000,"on_fail":{"action":"SKIP"}}"#
);

// ---------------------------------------------------------------------------
// The vocabulary
// ---------------------------------------------------------------------------

#[test]
fn covering_and_on_compile_where_they_are_legal() {
    // The two sites `con2` refused at compile now have their effect.
    compile(COVER).unwrap_or_else(|error| panic!("covering compiles: {error}"));
    compile(
        r#"{"label":"mine","place_beacon":{"at":{"covering":{"seam":{"rank":"NEAREST","coverage":"ANY"}}}}}"#,
    )
    .unwrap_or_else(|error| panic!("covering a seam compiles: {error}"));
    compile(concat!(
        r#"{"label":"build","interface":{"beacon":{"beacon_id":"b_00"},"rows":[{"add_build_target":"#,
        r#"{"target":{"blueprint_id":"generator","anchor":{"on":{"vent":{"rank":"NEAREST"}}}}}}]}}"#
    ))
    .unwrap_or_else(|error| panic!("`on` the nearest vent compiles: {error}"));
    compile(concat!(
        r#"{"label":"unbuild","interface":{"beacon":{"beacon_id":"b_00"},"rows":[{"remove_build_target":"#,
        r#"{"anchor":{"on":{"feature_id":"vent_120_88"}}}}]}}"#
    ))
    .unwrap_or_else(|error| panic!("removing by feature compiles: {error}"));
}

#[test]
fn a_targeting_construct_where_it_is_not_legal_is_refused() {
    let misplaced = |route: &str| match compile(route) {
        Err(PlanError::MisplacedTarget(_)) => {}
        other => panic!("refused as misplaced, not {other:?}\n{route}"),
    };
    // `covering` outside a placement's site.
    misplaced(
        r#"{"label":"walk","move":{"to":{"covering":{"vent":{"rank":"NEAREST","coverage":"ANY"}}}}}"#,
    );
    // `on` as a place.
    misplaced(r#"{"label":"walk","move":{"to":{"on":{"feature_id":"vent_1_2"}}}}"#);
    // A coverage filter under `on`, and a seam under `on`.
    misplaced(concat!(
        r#"{"label":"b","interface":{"beacon":{"beacon_id":"b_00"},"rows":[{"add_build_target":"#,
        r#"{"target":{"blueprint_id":"generator","anchor":{"on":{"vent":{"rank":"NEAREST","coverage":"ANY"}}}}}}]}}"#
    ));
    misplaced(concat!(
        r#"{"label":"b","interface":{"beacon":{"beacon_id":"b_00"},"rows":[{"add_build_target":"#,
        r#"{"target":{"blueprint_id":"generator","anchor":{"on":{"seam":{"rank":"NEAREST"}}}}}}]}}"#
    ));
    // `covered {}` outside a covering placement's initial settings.
    misplaced(concat!(
        r#"{"label":"place","place_beacon":{"at":{"voxel":{"x":100,"y":100,"z":40}},"#,
        r#""initial":{"mandate":{"build":{"targets":[{"blueprint_id":"generator","#,
        r#""anchor":{"on":{"covered":{}}}}]}}}}}"#
    ));
    // A description in a removal.
    misplaced(concat!(
        r#"{"label":"b","interface":{"beacon":{"beacon_id":"b_00"},"rows":[{"remove_build_target":"#,
        r#"{"anchor":{"on":{"vent":{"rank":"NEAREST"}}}}}]}}"#
    ));
    // An unset rank, and an unset coverage under `covering`, are errors, never
    // read as NEAREST or ANY.
    assert!(matches!(
        compile(r#"{"label":"c","place_beacon":{"at":{"covering":{"vent":{"coverage":"ANY"}}}}}"#),
        Err(PlanError::UnsetEnum(_))
    ));
    assert!(matches!(
        compile(r#"{"label":"c","place_beacon":{"at":{"covering":{"vent":{"rank":"NEAREST"}}}}}"#),
        Err(PlanError::UnsetEnum(_))
    ));
    // A name that is not a feature name.
    assert!(matches!(
        compile(r#"{"label":"c","place_beacon":{"at":{"covering":{"feature_id":"vent_-1_2"}}}}"#),
        Err(PlanError::UnknownFeature(_))
    ));
}

// ---------------------------------------------------------------------------
// Names and room
// ---------------------------------------------------------------------------

#[test]
fn a_seats_core_is_b_00_and_ids_are_per_seat() {
    let mut world = world_at(SEED, 3, &[180_000]);
    // Every seat's core is its own `b_00`.
    for seat in 0..3_u8 {
        let row = world
            .beacons()
            .row_of_ordinal(SeatId::new(seat), 0)
            .expect("a core");
        assert_eq!(world.beacons().seats().get(row), Some(&seat));
        let ordinal = world
            .beacons()
            .ordinals()
            .get(row)
            .copied()
            .expect("an ordinal");
        assert_eq!(own_beacon_name(ordinal), "b_00");
    }
    // Seat 1 places first and seat 0 second: under the pre-S1 global numbering
    // seat 0's beacon would have been `b_04`, telling it how many the others
    // had. Per seat, it is seat 0's first placed beacon, `b_01`.
    let theirs = world
        .place_beacon_directly(
            SeatId::new(1),
            point(core_voxel(&world).map(|axis| axis + 1)),
            MandateKind::Mine,
            PRIORITY_NORMAL,
        )
        .expect("room");
    let ours = world
        .place_beacon_directly(
            SeatId::new(SEAT),
            point(beside_core(&world, 4)),
            MandateKind::Mine,
            PRIORITY_NORMAL,
        )
        .expect("room");
    let ordinal = |beacon: pharmakos_sim::tables::BeaconId| {
        world
            .beacons()
            .ordinals()
            .get(usize::try_from(beacon.raw()).unwrap())
            .copied()
            .expect("an ordinal")
    };
    assert_eq!((ordinal(theirs), ordinal(ours)), (1, 1));

    // A `b_NN` resolves among the seat's own beacons only, over the frozen
    // snapshot exactly as in the live world.
    let snapshot = Snapshot::capture(&world).expect("a world between ticks");
    assert_eq!(
        resolve_beacon_in(
            &snapshot,
            world.rules(),
            SeatId::new(SEAT),
            BeaconSpec::Own(1),
            None
        ),
        Some(ours)
    );
    assert_eq!(
        resolve_beacon_in(
            &snapshot,
            world.rules(),
            SeatId::new(1),
            BeaconSpec::Own(1),
            None
        ),
        Some(theirs)
    );
    assert_eq!(
        resolve_beacon_in(
            &snapshot,
            world.rules(),
            SeatId::new(2),
            BeaconSpec::Own(1),
            None
        ),
        None,
        "seat 2 has placed nothing"
    );
    assert_eq!(
        resolve_beacon_in(
            &snapshot,
            world.rules(),
            SeatId::new(SEAT),
            BeaconSpec::Foreign(1),
            None
        ),
        None,
        "another seat's beacon is never a seat's own order's target"
    );
}

#[test]
fn table_room_is_the_world_total_divided_by_the_seat_count() {
    // Item 127 (12), amending item 63: each seat may place 40 / 3 = 13 beacons
    // on top of its core, rounded down, and a full share stops that seat and
    // no other.
    assert_eq!(room_per_seat(40, 3), 13);
    assert_eq!(room_per_seat(40, 2), 20);
    assert_eq!(room_per_seat(300, 3), 100);
    let mut world = world_at(SEED, 3, &[180_000]);
    let core = core_voxel(&world);
    let mut placed = 0_u32;
    while world
        .place_beacon_directly(
            SeatId::new(SEAT),
            point([
                core[0] - 1,
                core[1] - i32::try_from(placed).unwrap() - 1,
                core[2],
            ]),
            MandateKind::Mine,
            PRIORITY_NORMAL,
        )
        .is_some()
    {
        placed += 1;
        assert!(placed <= 13, "seat 0 placed past its share");
    }
    assert_eq!(placed, 13, "a seat places exactly its share");
    assert!(
        world
            .place_beacon_directly(
                SeatId::new(1),
                point(core),
                MandateKind::Mine,
                PRIORITY_NORMAL
            )
            .is_some(),
        "another seat's share is untouched"
    );
}

#[test]
fn a_seats_structure_share_counts_its_live_structures_not_its_ruins() {
    // The structure share is per seat like the beacon and unit shares, and
    // since S2 (S2's plan, E1) counts the seat's live structures: a ruin is
    // nobody's and holds a row of the ruin reserve, so one seat's churn cannot
    // fill the table and refuse another seat's build, and a seat whose
    // outpost fell builds again. At three seats each share is 120 / 3 = 40.
    assert_eq!(room_per_seat(120, 3), 40);
    let mut world = world_at(SEED, 3, &[180_000]);
    let zero = SeatId::new(0);
    let one = SeatId::new(1);
    let outpost = world
        .place_beacon_directly(
            zero,
            point(beside(&world, zero, 3, 0)),
            MandateKind::Build,
            PRIORITY_NORMAL,
        )
        .expect("room");
    for step in 0..40 {
        let at = beside(&world, zero, -2 - step, 6);
        world
            .raise_structure(zero, outpost, StructureKind::Generator, point(at))
            .expect("room in the table");
    }
    // The outpost falls, and its forty structures become neutral ruins.
    assert!(
        world
            .request_damage(DamageOrder {
                target: DamageTarget::Beacon(outpost),
                amount: pharmakos_sim::math::quantity::Damage::new(i64::from(1_000_000)).unwrap(),
                by: SeatId::NEUTRAL,
                source: None,
            })
            .is_ok()
    );
    for seat in [zero, one] {
        let core = core_of(&world, seat);
        world.set_writ(core, MandateKind::Build);
        assert!(
            world
                .add_target(
                    core,
                    StructureKind::Generator.id(),
                    point(beside(&world, seat, 1, 0))
                )
                .is_ok()
        );
    }
    let mut runner = Runner::new(world).unwrap_or_else(|error| panic!("{error}"));
    assert!(runner.begin_push().unwrap(), "a match opens in a Lull");
    let events = play_all(&mut runner, 200);
    assert_eq!(
        count(&events, EventKind::StructureRuined),
        40,
        "the outpost's structures are ruins"
    );
    assert_eq!(
        queued_by(&events, zero).len(),
        1,
        "seat 0's ruins fill no share, so it builds again"
    );
    assert_eq!(
        queued_by(&events, one).len(),
        1,
        "and seat 1's share is untouched"
    );
}

#[test]
fn one_structure_per_voxel_holds_across_seats() {
    // targeting.md, "Sites": construction is refused where a structure of any
    // seat stands, so the first seat to build wins and the other's target is
    // never paid for. The Build mandate skips such a target rather than asking
    // for it, so the target behind it in the list is still served.
    let mut world = world_at(SEED, 2, &[180_000]);
    let zero = SeatId::new(0);
    let one = SeatId::new(1);
    let taken = point(beside(&world, one, 1, 0));
    let free = point(beside(&world, one, 2, 0));
    world
        .raise_structure(zero, core_of(&world, zero), StructureKind::Generator, taken)
        .expect("room");
    let core = core_of(&world, one);
    world.set_writ(core, MandateKind::Build);
    for at in [taken, free] {
        assert!(
            world
                .add_target(core, StructureKind::Generator.id(), at)
                .is_ok()
        );
    }
    let cost = world.structure_cost(StructureKind::Generator).raw();
    let purse = |world: &World| world.seats().treasuries().get(1).expect("seat 1").raw();
    let before = purse(&world);
    let mut runner = Runner::new(world).unwrap_or_else(|error| panic!("{error}"));
    assert!(runner.begin_push().unwrap(), "a match opens in a Lull");
    let events = play_all(&mut runner, 200);
    assert_eq!(
        queued_by(&events, one),
        vec![Some(free)],
        "seat 1 builds only where nobody stands"
    );
    assert_eq!(
        before - purse(runner.world()),
        cost,
        "and pays for that one Generator alone"
    );
}

// ---------------------------------------------------------------------------
// The resolver in a world, and the three reading rules
// ---------------------------------------------------------------------------

#[test]
fn a_carried_covering_step_has_no_legal_site_after_round_one_on_the_golden_seed() {
    // targeting.md's finding, played: round 1 covers seat 0's starting vent and
    // builds a Generator on it; in rounds 2 and 3 the same sealed step reads
    // its description again (a carried playbook re-reads its descriptions
    // every round), and the nearest vents seat 0 does not cover are contested
    // ones about 80 voxels out, beyond one beacon's reach. They match the
    // description, and none of them has a legal site, which the failure table
    // answers `illegal_site` (targeting.md, "Failure": "candidates match but
    // none has a legal site").
    let world = world_at(SEED, 2, &[180_000, 180_000, 180_000]);
    let mut runner = open(world, COVER);
    let mut rounds: Vec<Vec<Event>> = Vec::new();
    loop {
        rounds.push(play(&mut runner, u32::MAX, |_, _| {}));
        if runner.phase() != MatchPhase::Recap || !runner.end_recap().unwrap() {
            break;
        }
        if !runner.begin_push().unwrap() {
            break;
        }
    }
    assert_eq!(rounds.len(), 3, "three rounds were played");
    let first = rounds.first().expect("round 1");
    assert_eq!(
        count(first, EventKind::BeaconPlaced),
        1,
        "round 1 covers the vent"
    );
    assert_eq!(
        count(first, EventKind::StructureQueued),
        1,
        "and pays for the Generator `on` the vent it covered"
    );
    assert!(failures(first).is_empty(), "{:?}", failures(first));
    for later in rounds.iter().skip(1) {
        assert_eq!(count(later, EventKind::BeaconPlaced), 0, "no vent to cover");
        // `illegal_site`, not `no_target`: the description matches, the sites
        // do not (targeting.md's failure table; the plan's acceptance line
        // says `no_target`, and the PR records the disagreement for a ruling).
        // The count is what the description matched (S1's `fog`): every
        // vent seat 0 does not cover, so at least one.
        let failed = failed(later);
        assert_eq!(
            failed.iter().map(|one| one.reason).collect::<Vec<_>>(),
            vec![StepFailure::IllegalSite]
        );
        assert!(
            failed.iter().all(|one| one.matched.is_some_and(|n| n >= 1)),
            "{failed:?}"
        );
    }
}

#[test]
fn a_held_target_fails_only_when_lost() {
    // Round 1 again: the step binds `vent_324_16` when it starts, walks there,
    // covers it with its own beacon and builds on it -- the holder's own acts,
    // which never disqualify its own target (filters are not re-checked while a
    // target is held), so the step completes.
    let world = world_at(SEED, 2, &[180_000]);
    let mut runner = open(world, COVER);
    let events = play(&mut runner, 2_000, |_, _| {});
    assert_eq!(count(&events, EventKind::StepCompleted), 1);
    assert!(failures(&events).is_empty());

    // The same step, but the vent is destroyed while the commander walks:
    // every footprint column's top vent voxel goes, so no exposed vent material
    // is left, and the step fails `feature_lost` at its next decision.
    let world = world_at(SEED, 2, &[180_000]);
    let vent = world
        .features()
        .index_of_name(START_VENT)
        .expect("seat 0's starting vent");
    let footprint = world.features().get(vent).unwrap().footprint.clone();
    let mut runner = open(world, COVER);
    let mut destroyed = false;
    let events = play(&mut runner, 400, |world, _| {
        if !destroyed && world.tick().raw() == 100 {
            for column in &footprint {
                assert!(
                    world
                        .request_voxel_edit(VoxelEdit::Set {
                            at: [column.x, column.y, column.top],
                            material: Material::STONE,
                            by: None,
                        })
                        .is_ok()
                );
            }
            destroyed = true;
        }
    });
    assert_eq!(failures(&events), vec![id(StepFailure::FeatureLost)]);
    assert_eq!(count(&events, EventKind::BeaconPlaced), 0);
}

#[test]
fn a_second_generator_on_a_covered_vent_is_illegal_site() {
    // One structure per voxel, and one Generator per vent: a Generator of any
    // seat already standing on `vent_324_16` makes the vent no legal `on`
    // site. Named, it fails `illegal_site`; described, it is no candidate, and
    // with nothing else in the sphere the description matches nothing.
    let mut world = world_at(SEED, 2, &[180_000]);
    let vent = world.features().index_of_name(START_VENT).unwrap();
    let anchor = world.features().get(vent).unwrap().anchor;
    let site = [335, 18, world.voxels().standing_z(335, 18)];
    let beacon = world
        .place_beacon_directly(
            SeatId::new(SEAT),
            point(site),
            MandateKind::Build,
            PRIORITY_NORMAL,
        )
        .expect("room");
    let at = [
        anchor[0],
        anchor[1],
        world.voxels().standing_z(anchor[0], anchor[1]),
    ];
    world
        .raise_structure(SeatId::new(1), beacon, StructureKind::Generator, point(at))
        .expect("room");
    let build = |anchor: &str| {
        format!(
            concat!(
                r#"{{"label":"build","interface":{{"beacon":{{"beacon_id":"b_01"}},"rows":[{{"add_build_target":"#,
                r#"{{"target":{{"blueprint_id":"generator","anchor":{{"on":{anchor}}}}}}}}}]}},"#,
                r#""timeout_ms":60000,"on_fail":{{"action":"SKIP"}}}}"#
            ),
            anchor = anchor
        )
    };
    let named = build(&format!(r#"{{"feature_id":"{START_VENT}"}}"#));
    let described = build(r#"{"vent":{"rank":"NEAREST"}}"#);
    let route = format!("{named},{described}");
    let mut runner = open(world, &route);
    let events = play(&mut runner, 200, |_, _| {});
    // Counted (S1's `fog`): the name matched its one vent, inside the
    // sphere; the description matched none.
    assert_eq!(
        failed(&events),
        vec![
            StepFailed::counted(StepFailure::IllegalSite, 1),
            StepFailed::counted(StepFailure::NoTarget, 0)
        ]
    );
}

#[test]
fn stacking_on_an_own_beacon_is_illegal() {
    // No stacking (targeting.md, "Companion changes"): a site on the column of
    // one of the seat's own live beacons is `illegal_site`, refused when the
    // step starts and before anything is charged.
    let world = world_at(SEED, 2, &[180_000]);
    let core = core_voxel(&world);
    let beside = beside_core(&world, 3);
    let route = format!(
        concat!(
            r#"{{"label":"stack","place_beacon":{{"at":{{"voxel":{{"x":{x},"y":{y},"z":{z}}}}}}},"on_fail":{{"action":"SKIP"}}}},"#,
            r#"{{"label":"beside","place_beacon":{{"at":{{"voxel":{{"x":{bx},"y":{by},"z":{bz}}}}}}},"timeout_ms":30000,"on_fail":{{"action":"SKIP"}}}}"#
        ),
        x = core[0],
        y = core[1],
        z = core[2],
        bx = beside[0],
        by = beside[1],
        bz = beside[2]
    );
    let mut runner = open(world, &route);
    let events = play(&mut runner, 600, |_, _| {});
    assert_eq!(failures(&events), vec![id(StepFailure::IllegalSite)]);
    assert_eq!(
        count(&events, EventKind::BeaconPlaced),
        1,
        "the control lands"
    );
}

#[test]
fn a_restarted_step_resumes_at_the_beacon_it_placed() {
    // Restart keeps the placed beacon (targeting.md, "Companion changes"): the
    // commander dies after the deploy, while the initial rows commit; when it
    // comes back the step resumes as a visit to the beacon it placed, with the
    // rows it had left, and never pays for a second beacon.
    let world = world_at(SEED, 2, &[180_000]);
    let site = beside_core(&world, 3);
    let route = format!(
        concat!(
            r#"{{"label":"place","place_beacon":{{"at":{{"voxel":{{"x":{x},"y":{y},"z":{z}}}}},"#,
            r#""initial":{{"priority":"HIGH","mandate":{{"mine":{{"dig_max_depth":2,"pillar_spacing":3}}}}}}}},"#,
            r#""timeout_ms":60000,"on_fail":{{"action":"SKIP"}}}}"#
        ),
        x = site[0],
        y = site[1],
        z = site[2]
    );
    let mut runner = open(world, &route);
    let commander = runner
        .world()
        .commander_of(SeatId::new(SEAT))
        .expect("an occupied seat has a commander");
    let treasury = |world: &World| world.seats().treasuries().first().expect("seat 0").raw();
    let before = treasury(runner.world());
    let mut killed = false;
    let events = play(&mut runner, 1_600, |world, events| {
        if !killed
            && events
                .iter()
                .any(|event| event.kind == EventKind::BeaconPlaced)
        {
            assert!(
                world
                    .request_damage(DamageOrder {
                        target: DamageTarget::Unit(UnitId::new(commander.raw())),
                        amount: pharmakos_sim::math::quantity::Damage::new(i64::from(10_000))
                            .unwrap(),
                        by: SeatId::NEUTRAL,
                        source: None,
                    })
                    .is_ok()
            );
            killed = true;
        }
    });
    assert!(killed, "the deploy landed");
    assert_eq!(
        count(&events, EventKind::BeaconPlaced),
        1,
        "one beacon, ever"
    );
    assert_eq!(
        failures(&events),
        vec![id(StepFailure::CommanderDead)],
        "death interrupts the commit"
    );
    let placed = events
        .iter()
        .find(|event| event.kind == EventKind::BeaconPlaced)
        .and_then(|event| event.subject)
        .unwrap();
    let visits: Vec<_> = events
        .iter()
        .filter(|event| event.kind == EventKind::VisitStarted)
        .map(|event| event.subject)
        .collect();
    assert_eq!(visits, vec![Some(placed)], "the restart is a visit to it");
    assert_eq!(
        count(&events, EventKind::StepCompleted),
        1,
        "and the step completes"
    );
    assert_eq!(
        count(&events, EventKind::RowCommitted),
        2,
        "both rows commit"
    );
    let cost = runner.world().beacon_cost().raw();
    let spent = before - treasury(runner.world());
    assert!(
        spent < 2 * cost,
        "one beacon's cost, not two: spent {spent} of a {cost} beacon"
    );
}

/// Capture `world`, restore the bytes into `fresh` (a world built and sealed
/// as `world` was), and assert the two hash the same now and after one more
/// tick. Returns the decoded snapshot, for the caller to see what it carried.
fn assert_round_trips(world: &World, fresh: &World, enc: &mut Enc) -> Snapshot {
    let bytes = Snapshot::capture(world)
        .expect("a world between ticks")
        .to_bytes()
        .expect("encodes");
    let decoded = Snapshot::from_bytes(&bytes).expect("decodes");
    let mut restored = fresh.clone();
    decoded.restore_into(&mut restored).expect("restores");
    let tick = world.tick().raw();
    assert_eq!(
        restored.state_hash(),
        world.state_hash(),
        "tick {tick}: the restored world does not hash to the saved one"
    );
    let mut continued = world.clone();
    assert_eq!(
        restored.step(enc).unwrap(),
        continued.step(enc).unwrap(),
        "tick {tick}: the restored world diverges on the next tick"
    );
    decoded
}

/// Whether a tick is worth a round-trip: one where the step or its beacon
/// changed state. A restore regenerates the map, so a checkpoint every few
/// ticks would make this the suite's slowest test; these are the ticks where
/// the new columns change (a step binds, a beacon lands, a row commits a bound
/// target, the target is paid for, a restart resumes as a visit).
fn checkpoint(events: &[Event]) -> bool {
    events.iter().any(|event| {
        matches!(
            event.kind,
            EventKind::StepStarted
                | EventKind::BeaconPlaced
                | EventKind::RowCommitted
                | EventKind::StructureQueued
                | EventKind::VisitStarted
        )
    })
}

#[test]
fn a_bound_step_and_a_resumed_visit_round_trip_through_a_snapshot() {
    // Risk R2's mitigation for S1's new hashed state: the covering step's
    // bindings, a bound Build target, and a restarted placement resumed as a
    // visit all survive a save and restore mid-flight, hash-identically, and
    // the restored world takes the same next tick. The determinism suite's
    // playbook (Hold & Build's instantiation since S1's real segment) does
    // write a target `on` a covered vent, but its round-trips do not choose
    // their snapshot ticks for these columns; this test does.
    let mut enc = Enc::with_capacity(64 * 1024);
    let sealed = || {
        let mut world = world_at(SEED, 2, &[180_000]);
        assert!(world.seal_playbook(SeatId::new(SEAT), compile(COVER).expect("compiles")));
        world
    };
    let fresh = sealed();

    // Covering, walking, deploying and committing, untouched.
    let mut runner = open(world_at(SEED, 2, &[180_000]), COVER);
    let (mut bound, mut target) = (false, false);
    let events = play(&mut runner, 1_600, |world, events| {
        if checkpoint(events) {
            let carried = assert_round_trips(world, &fresh, &mut enc);
            bound |= !carried.plan_binding_feature.is_empty();
            target |= carried
                .target_feature
                .iter()
                .any(|feature| *feature != NO_FEATURE);
        }
    });
    assert_eq!(
        count(&events, EventKind::StepCompleted),
        1,
        "the step completes"
    );
    assert!(bound, "a checkpoint carried the step's bindings");
    assert!(target, "a checkpoint carried a bound Build target");

    // The same step, with the commander killed once the beacon lands: the
    // step restarts and resumes as a visit to the beacon it placed.
    let mut runner = open(world_at(SEED, 2, &[180_000]), COVER);
    let commander = runner
        .world()
        .commander_of(SeatId::new(SEAT))
        .expect("an occupied seat has a commander");
    let (mut killed, mut resumed) = (false, false);
    let events = play(&mut runner, 2_400, |world, events| {
        // Checkpoints come before the kill is filed: a damage order waits for
        // the next tick's combat phase as a host input, and a save is taken
        // at a Lull boundary, where none is pending, so the snapshot does not
        // carry one.
        if killed && checkpoint(events) {
            let carried = assert_round_trips(world, &fresh, &mut enc);
            resumed |= carried.plan_resumed.iter().any(|flag| *flag != 0);
        }
        if !killed
            && events
                .iter()
                .any(|event| event.kind == EventKind::BeaconPlaced)
        {
            assert!(
                world
                    .request_damage(DamageOrder {
                        target: DamageTarget::Unit(UnitId::new(commander.raw())),
                        amount: pharmakos_sim::math::quantity::Damage::new(i64::from(10_000))
                            .unwrap(),
                        by: SeatId::NEUTRAL,
                        source: None,
                    })
                    .is_ok()
            );
            killed = true;
        }
    });
    assert!(killed, "the deploy landed");
    assert!(resumed, "a checkpoint carried the resumed visit");
    assert_eq!(
        count(&events, EventKind::BeaconPlaced),
        1,
        "one beacon, ever"
    );
}

#[test]
fn the_walk_in_times_out_with_no_path() {
    // Walk-in (targeting.md, "Companion changes"): `place_beacon` walks to its
    // site first and the step's `timeout_ms` bounds the walk, so a far site
    // with a short timeout fails `timeout` -- before the deploy, so nothing is
    // charged -- and a site the commander cannot reach fails `no_path` at once.
    let world = world_at(SEED, 2, &[180_000]);
    let far = beside_core(&world, -20);
    let pit = beside_core(&world, 6);
    let route = format!(
        concat!(
            r#"{{"label":"settle","hold":{{"ms":1000}}}},"#,
            r#"{{"label":"far","place_beacon":{{"at":{{"voxel":{{"x":{fx},"y":{fy},"z":{fz}}}}}}},"timeout_ms":3000,"on_fail":{{"action":"SKIP"}}}},"#,
            r#"{{"label":"pit","place_beacon":{{"at":{{"voxel":{{"x":{px},"y":{py},"z":{pz}}}}}}},"timeout_ms":30000,"on_fail":{{"action":"SKIP"}}}}"#
        ),
        fx = far[0],
        fy = far[1],
        fz = far[2],
        px = pit[0],
        py = pit[1],
        pz = pit[2]
    );
    let mut runner = open(world, &route);
    // A one-column shaft four voxels deep: its floor stands, and a walker
    // that climbs one voxel at a time can neither get down to it nor out.
    let top = runner.world().voxels().top_solid_z(pit[0], pit[1]).unwrap();
    for depth in 0..4 {
        assert!(
            runner
                .world_mut()
                .request_voxel_edit(VoxelEdit::Set {
                    at: [pit[0], pit[1], top - depth],
                    material: Material::AIR,
                    by: None,
                })
                .is_ok()
        );
    }
    let treasury = *runner.world().seats().treasuries().first().expect("seat 0");
    let events = play(&mut runner, 200, |_, _| {});
    assert_eq!(
        failures(&events),
        vec![id(StepFailure::Timeout), id(StepFailure::NoPath)]
    );
    assert_eq!(
        runner
            .world()
            .seats()
            .treasuries()
            .first()
            .expect("seat 0")
            .raw()
            - treasury.raw(),
        events
            .iter()
            .filter(|event| event.kind == EventKind::OreDelivered)
            .map(|event| event.value)
            .sum::<i64>(),
        "neither failure was charged: only ore moved the treasury"
    );
}

// ---------------------------------------------------------------------------
// The feature table and the counter
// ---------------------------------------------------------------------------

#[test]
fn the_feature_table_is_regenerated_identically_on_restore() {
    // The table is a pure function of the seed, the rules and the occupied
    // seats, and is regenerated with the map rather than carried: a world of
    // another seed that restores this snapshot ends up with this seed's
    // features, exactly.
    let world = world_at(SEED, 2, &[180_000]);
    let saved = Snapshot::capture(&world).expect("a world between ticks");
    let mut other = world_at(pharmakos_sim::DETERMINISM_MATCH_SEED, 2, &[180_000]);
    assert_ne!(other.features(), world.features());
    saved.restore_into(&mut other).expect("the save restores");
    assert_eq!(other.features(), world.features());
    let names: Vec<String> = other
        .features()
        .features()
        .iter()
        .map(pharmakos_sim::features::Feature::name)
        .collect();
    assert!(names.contains(&START_VENT.to_owned()), "{names:?}");
}

// ---------------------------------------------------------------------------
// The threat scans a decision reads, counted independently (lane `dunit`)
// ---------------------------------------------------------------------------

/// The sensor positions a walk asking whether `seat` sees `point` reads: each
/// position up to and including the first sensor of `seat`'s that sees it, or
/// every position when none does. Counted from `World::sensor_at`'s own
/// positions, not from the sim's walk.
fn seen_rows(world: &World, seat: SeatId, point: [Fx; 3]) -> u64 {
    let total = world.beacons().seats().len()
        + world.units().seats().len()
        + world.structures().seats().len();
    let mut at = 0_usize;
    while let Some((sensor, next)) = world.sensor_at(at) {
        at = next;
        if sensor.seat == seat && sensor.sees(&pharmakos_sim::sight::RadiusOnly, point) {
            return u64::try_from(at).unwrap();
        }
    }
    u64::try_from(total).unwrap()
}

/// The rows one known-threat scan for `seat` reads, counted from the tables:
/// every unit, structure and beacon row; for each armed living enemy unit,
/// finished structure and core, the sensor walk asking whether `seat` sees
/// it ([`seen_rows`]); and for each living enemy beacon, every beacon row
/// (the search for its seat's core).
fn threat_scan_rows(world: &World, seat: SeatId) -> u64 {
    let rules = world.rules();
    let enemy = |owner: u8| SeatId::new(owner).is_some() && owner != seat.raw();
    let beacon_rows = u64::try_from(world.beacons().seats().len()).unwrap();
    let mut rows = u64::try_from(
        world.units().seats().len()
            + world.structures().seats().len()
            + world.beacons().seats().len(),
    )
    .unwrap();
    let units = world.units();
    for (((owner, kind), hp), at) in units
        .seats()
        .iter()
        .zip(units.kinds())
        .zip(units.hit_points())
        .zip(units.positions())
    {
        let armed = pharmakos_sim::tables::UnitKind::from_id(*kind)
            .and_then(|kind| rules.unit(kind).weapon)
            .is_some();
        if hp.is_alive() && enemy(*owner) && armed {
            rows += seen_rows(world, seat, *at);
        }
    }
    let structures = world.structures();
    for ((((owner, kind), hp), building), at) in structures
        .seats()
        .iter()
        .zip(structures.kinds())
        .zip(structures.hit_points())
        .zip(structures.building())
        .zip(structures.positions())
    {
        let armed = StructureKind::from_id(*kind)
            .and_then(|kind| rules.structure(kind))
            .and_then(|row| row.weapon)
            .is_some();
        if hp.is_alive() && !*building && enemy(*owner) && armed {
            rows += seen_rows(world, seat, *at);
        }
    }
    let beacons = world.beacons();
    for (((owner, hp), at), id) in beacons
        .seats()
        .iter()
        .zip(beacons.hit_points())
        .zip(beacons.positions())
        .zip(beacons.ids())
    {
        if !hp.is_alive() || !enemy(*owner) {
            continue;
        }
        rows += beacon_rows;
        // A seat's core is its beacon of least id.
        let core = beacons
            .seats()
            .iter()
            .zip(beacons.ids())
            .filter(|(seat, _)| *seat == owner)
            .map(|(_, id)| *id)
            .min();
        if core == Some(*id) {
            rows += seen_rows(world, seat, *at);
        }
    }
    rows
}

/// The unit rows a repair-drone search for `seat` round `centre` reads: each
/// row up to and including the first living repair drone of `seat`'s inside
/// the radius, or every row.
fn repair_rows(world: &World, seat: SeatId, centre: [Fx; 3], radius: Fx) -> u64 {
    let units = world.units();
    let limit = pharmakos_sim::math::fixed::Sq::of_radius(radius);
    let found = units
        .kinds()
        .iter()
        .zip(units.seats())
        .zip(units.hit_points())
        .zip(units.positions())
        .position(|(((kind, owner), hp), at)| {
            *kind == pharmakos_sim::tables::UnitKind::RepairDrone.id()
                && *owner == seat.raw()
                && hp.is_alive()
                && pharmakos_sim::math::fixed::Sq::between(*at, centre) <= limit
        });
    u64::try_from(found.map_or(units.seats().len(), |row| row + 1)).unwrap()
}

/// The rows a radius test reads on a map with no Resonance Spire: every kind
/// byte of the structure table, the Spire test's (dunit part 2).
fn radius_rows(world: &World) -> u64 {
    let kinds = world.structures().kinds();
    assert!(
        !kinds.contains(&pharmakos_sim::tables::StructureKind::ResonanceSpire.id()),
        "the fixture holds no Spire"
    );
    u64::try_from(kinds.len()).unwrap()
}

/// Kill seat 0's starting mining drone before the Push's first tick, so no
/// beacon of the seat mines and a decision's seam upkeep is only the scan
/// [`upkeep_rows`] counts: these tests count what a step or a handler
/// charges, and a seam kept or chosen would add a dig search to every
/// decision (`tests/mining.rs` and `mining.rs`'s own tests count those).
fn without_its_miner(runner: &mut Runner) {
    let units = runner.world().units();
    let drones: Vec<u32> = units
        .kinds()
        .iter()
        .zip(units.seats())
        .zip(units.ids())
        .zip(units.hit_points())
        .filter(|(((kind, owner), _), hp)| {
            **kind == pharmakos_sim::tables::UnitKind::MiningDrone.id()
                && **owner == SEAT
                && hp.is_alive()
        })
        .map(|(((_, _), id), _)| *id)
        .collect();
    assert!(!drones.is_empty(), "seat 0 starts with a miner");
    for id in drones {
        runner
            .world_mut()
            .request_damage(DamageOrder {
                target: DamageTarget::Unit(UnitId::new(id)),
                amount: pharmakos_sim::math::quantity::Damage::new(1_000_000).unwrap(),
                by: SeatId::NEUTRAL,
                source: None,
            })
            .expect("a damage order between ticks");
    }
}

/// What a decision's seam upkeep (`mining::refresh_seams`) reads when no
/// beacon of seat 0's mines: one unit a beacon row, and for each of the
/// seat's living beacons whose writ is not Mine, every unit row the miner
/// test reads (no living miner is homed to it, so it reads them all).
fn upkeep_rows(world: &World) -> u64 {
    let beacons = world.beacons();
    let unit_rows = u64::try_from(world.units().seats().len()).unwrap();
    let mut rows = u64::try_from(beacons.ids().len()).unwrap();
    for ((owner, hp), mandate) in beacons
        .seats()
        .iter()
        .zip(beacons.hit_points())
        .zip(beacons.mandates())
    {
        if *owner == SEAT && hp.is_alive() && *mandate != MandateKind::Mine.id() {
            rows += unit_rows;
        }
    }
    rows
}

/// The beacon rows a `b_00` reads to find seat 0's core: every row up to the
/// core's (dunit part 2: `row_of_ordinal` charges one a row it reads).
fn b_00_rows(world: &World) -> u64 {
    u64::try_from(
        world
            .beacons()
            .row_of_ordinal(SeatId::new(SEAT), 0)
            .expect("a core")
            + 1,
    )
    .unwrap()
}

/// Seat 0's living beacons, in row order: each one's id and where it stands.
fn own_beacons(world: &World) -> Vec<(BeaconId, [Fx; 3])> {
    let beacons = world.beacons();
    beacons
        .seats()
        .iter()
        .zip(beacons.hit_points())
        .zip(beacons.ids())
        .zip(beacons.positions())
        .filter(|(((owner, hp), _), _)| **owner == SEAT && hp.is_alive())
        .map(|((_, id), at)| (BeaconId::new(*id), *at))
        .collect()
}

/// A world on the golden seed where seat 0 sees an armed enemy: seat 1's
/// outpost beacon beside seat 0's core with a raider of seat 1's on it, and,
/// when `drone` is set, a repair drone of seat 0's in the core's sphere.
fn with_an_enemy_in_sight(drone: bool) -> World {
    let mut world = world_at(SEED, 2, &[180_000]);
    let zero = SeatId::new(SEAT);
    let one = SeatId::new(1);
    let (core, centre) = own_beacons(&world).first().copied().expect("a core");
    let [cx, cy] = pharmakos_sim::targeting::column_of(centre);
    let at = world.standing_point([cx + 6, cy + 2, 0]);
    let outpost = world
        .place_beacon_directly(one, at, MandateKind::Build, PRIORITY_NORMAL)
        .expect("room for the enemy's beacon");
    world.set_common(
        outpost,
        pharmakos_sim::tables::Common {
            roe: 1,
            retreat_hp_pct: 0,
        },
    );
    world
        .field_unit(one, outpost, pharmakos_sim::tables::UnitKind::Raider, at)
        .expect("room for the raider");
    if drone {
        let near = world.standing_point([cx + 2, cy - 2, 0]);
        world
            .field_unit(
                zero,
                core,
                pharmakos_sim::tables::UnitKind::RepairDrone,
                near,
            )
            .expect("room for the drone");
    }
    world
}

/// The `safest` handler's decision on the sixth tick: its units and the world
/// it ran on.
fn safest_decision(world: World, wound: bool) -> Runner {
    let wait = concat!(
        r#"{"label":"wait","wait_until":{"condition":{"segment_elapsed":{"ms":{"op":"GE","value":900000}}}},"#,
        r#""timeout_ms":600000}"#
    );
    // A handler whose condition ranks the seat's beacons by `safest` and is
    // never true: tried (1), its node (1), the ranking's candidates (one a
    // beacon row) and, for each of the seat's own living beacons it weighs,
    // the threat scan's rows (and, with the commander wounded, the repair
    // search's).
    let handler = concat!(
        r#"{"id":"never","when":{"beacon_hp_pct":{"beacon":{"safest":{}},"pct":{"op":"LT","value":0}}},"#,
        r#""body":[{"label":"pause","hold":{"ms":1000}}],"resume":"CONTINUE","cooldown_ms":5000,"max_fires":1}"#
    );
    let mut book = playbook(wait);
    book.declarative.as_mut().unwrap().handlers = vec![json::decode(handler).expect("a handler")];
    let plan = Plan::compile(&book, &rules()).expect("compiles");
    let mut runner = Runner::new(world).unwrap_or_else(|error| panic!("{error}"));
    runner.seal_playbook(SeatId::new(SEAT), plan).unwrap();
    assert!(runner.begin_push().unwrap());
    without_its_miner(&mut runner);
    // The sixth tick. The wound lands on the fifth, before the sixth's
    // decision.
    let _ = play(&mut runner, 5, |_, _| {});
    if wound {
        let commander = runner
            .world()
            .commander_of(SeatId::new(SEAT))
            .expect("a commander");
        assert!(
            runner
                .world_mut()
                .request_damage(DamageOrder {
                    target: DamageTarget::Unit(UnitId::new(commander.raw())),
                    amount: pharmakos_sim::math::quantity::Damage::new(1).unwrap(),
                    by: SeatId::NEUTRAL,
                    source: None,
                })
                .is_ok()
        );
    }
    let _ = play(&mut runner, 1, |_, _| {});
    runner
}

/// A `safest` selector reads each candidate beacon's sphere threat, a scan
/// of every unit, structure and beacon row for the armed enemies the seat
/// sees, with the sensor walk each armed enemy asks and the core search each
/// enemy beacon asks: one unit a row each reads (lane `dunit`), beside the
/// selector's one unit a candidate. Here seat 0 sees an armed enemy beside
/// its core. Before the lane the scan was charged nothing; before its fix
/// pass 1 the sensor walks and core searches were charged nothing.
#[test]
fn a_safest_selector_counts_the_threat_scan_it_reads() {
    let runner = safest_decision(with_an_enemy_in_sight(false), false);
    let world = runner.world();
    let seat = SeatId::new(SEAT);
    let units = world.decision_work(0).expect("seat 0").expect("measured");
    let beacon_rows = u64::try_from(world.beacons().seats().len()).unwrap();
    let own = own_beacons(world);
    let scan = threat_scan_rows(world, seat);
    let outer = u64::try_from(
        world.units().seats().len()
            + world.structures().seats().len()
            + world.beacons().seats().len(),
    )
    .unwrap();
    println!(
        "safest: {units} units; {beacon_rows} beacon rows, {} own, {scan} rows a scan ({outer} outer)",
        own.len()
    );
    assert!(!own.is_empty(), "the selector weighs a beacon");
    assert!(
        scan > outer,
        "the scan asks sensor walks and core searches beside its own rows"
    );
    assert_eq!(
        units,
        3 + beacon_rows
            + u64::try_from(own.len()).unwrap() * (radius_rows(world) + scan)
            + upkeep_rows(world),
        "the handler, its node, the wait's node, the candidates, each candidate's sphere and \
         threat scan, and the seam upkeep"
    );
}

/// With the commander wounded, `safest` also asks each candidate whether a
/// repair drone of the seat's stands in its sphere: one unit a unit row the
/// search reads, up to the drone that answers (lane `dunit`, fix pass 1).
#[test]
fn a_wounded_safest_selector_counts_the_repair_search_it_reads() {
    let runner = safest_decision(with_an_enemy_in_sight(true), true);
    let world = runner.world();
    let seat = SeatId::new(SEAT);
    let units = world.decision_work(0).expect("seat 0").expect("measured");
    let beacon_rows = u64::try_from(world.beacons().seats().len()).unwrap();
    let own = own_beacons(world);
    let scan = threat_scan_rows(world, seat);
    let repairs: u64 = own
        .iter()
        .map(|(id, centre)| repair_rows(world, seat, *centre, world.sphere_radius_of(*id)))
        .sum();
    let all_units = u64::try_from(world.units().seats().len()).unwrap();
    println!("wounded safest: {units} units; {repairs} repair rows of {all_units} unit rows");
    assert!(
        repairs < all_units * u64::try_from(own.len()).unwrap(),
        "the search stops at the drone"
    );
    assert_eq!(
        units,
        3 + beacon_rows
            + u64::try_from(own.len()).unwrap() * (2 * radius_rows(world) + scan)
            + repairs
            + upkeep_rows(world),
        "the handler, its node, the wait's node, the candidates, each candidate's two spheres, \
         the threat scans, the repair searches and the seam upkeep"
    );
}

/// An enemy predicate reads the sighting table, which every seat shares: one
/// unit a sighting row it reads, the seat's own and every other seat's (dunit
/// part 2; before it, the other seats' rows it skipped were charged nothing).
/// Here seat 0 sees seat 1's outpost and raider, and seat 1 sees seat 0's,
/// so the table holds rows of both.
#[test]
fn an_enemy_predicate_counts_every_sighting_row_it_reads() {
    let wait = concat!(
        r#"{"label":"wait","wait_until":{"condition":{"segment_elapsed":{"ms":{"op":"GE","value":900000}}}},"#,
        r#""timeout_ms":600000}"#
    );
    // Never true while seat 0 knows of seat 1's outpost: tried (1), its `not`
    // (1) and the predicate under it (1), with the rows the predicate reads.
    let handler = concat!(
        r#"{"id":"never","when":{"not":{"item":{"enemy_beacon_known":{"freshness":{"kind":"KNOWN_WITHIN","known_within_ms":30000}}}}},"#,
        r#""body":[{"label":"pause","hold":{"ms":1000}}],"resume":"CONTINUE","cooldown_ms":5000,"max_fires":1}"#
    );
    let mut book = playbook(wait);
    book.declarative.as_mut().unwrap().handlers = vec![json::decode(handler).expect("a handler")];
    let plan = Plan::compile(&book, &rules()).expect("compiles");
    let mut runner =
        Runner::new(with_an_enemy_in_sight(false)).unwrap_or_else(|error| panic!("{error}"));
    runner.seal_playbook(SeatId::new(SEAT), plan).unwrap();
    assert!(runner.begin_push().unwrap());
    without_its_miner(&mut runner);
    let _ = play(&mut runner, 6, |_, _| {});
    let world = runner.world();
    let units = world.decision_work(0).expect("seat 0").expect("measured");
    let sightings = world.sightings().seats();
    let others = sightings.iter().filter(|by| **by != SEAT).count();
    println!(
        "enemy predicate: {units} units; {} sighting rows, {others} of other seats",
        sightings.len()
    );
    assert!(others > 0, "the table holds another seat's rows");
    assert!(sightings.contains(&SEAT), "and seat 0's own");
    assert_eq!(
        units,
        4 + u64::try_from(sightings.len()).unwrap() + upkeep_rows(world),
        "the handler, its `not`, the predicate, every sighting row, the wait's node and the seam upkeep"
    );
}

/// Every decision's figure over a Push of seat 0's `route` followed by a long
/// wait (so the route never ends and no fallback decides), `ticks` ticks of
/// it, with seat 0's miner killed before the first ([`without_its_miner`])
/// and each decision's seam upkeep ([`upkeep_rows`]) taken off, each value
/// read once on its decision's tick and kept when it differs from the last.
fn later_decisions(world: World, route: &str, ticks: u32) -> Vec<u64> {
    let wait = concat!(
        r#"{"label":"after","wait_until":{"condition":{"segment_elapsed":{"ms":{"op":"GE","value":900000}}}},"#,
        r#""timeout_ms":600000}"#
    );
    let mut runner = open(world, &format!("{route},{wait}"));
    without_its_miner(&mut runner);
    let mut seen: Vec<u64> = Vec::new();
    let _ = play(&mut runner, ticks, |world, _| {
        if !world.is_decision_tick() {
            return;
        }
        let value = world.decision_work(0).expect("seat 0").expect("measured") - upkeep_rows(world);
        if seen.last() != Some(&value) {
            seen.push(value);
        }
    });
    seen
}

/// A row committing protected boxes writes each box once: one decision unit
/// a box, at the commit (lane `dunit`, fix pass 1).
#[test]
fn a_committed_row_counts_the_boxes_it_writes() {
    let fixture = world_at(SEED, 2, &[180_000]);
    let [x, y, z] = core_voxel(&fixture);
    let boxed = |dx: i32| {
        format!(
            r#"{{"min":{{"x":{},"y":{},"z":{z}}},"max":{{"x":{},"y":{},"z":{z}}}}}"#,
            x + dx,
            y - 1,
            x + dx + 1,
            y + 1
        )
    };
    let route = format!(
        r#"{{"label":"build","interface":{{"beacon":{{"beacon_id":"b_00"}},"rows":[{{"set_mandate_settings":{{"build":{{"protected_areas":[{},{},{}]}}}}}}]}},"timeout_ms":120000}}"#,
        boxed(3),
        boxed(6),
        boxed(9)
    );
    let units = later_decisions(fixture, &route, 1_200);
    println!("boxes: {units:?}");
    assert_eq!(
        units,
        vec![b_00_rows(&world_at(SEED, 2, &[180_000])), 0, 3, 1],
        "the step's start finds b_00, the walk and interface charge nothing, the commit its three boxes, then the wait one node a decision"
    );
}

/// A row adding a wall target checks its box column by column before it is
/// written: one decision unit a column checked (lane `dunit`, fix pass 1).
#[test]
fn a_committed_wall_counts_the_columns_it_checks() {
    let fixture = world_at(SEED, 2, &[180_000]);
    let [x, y, _] = core_voxel(&fixture);
    // A three-by-two box four columns east of the core, from the lowest
    // ground under it to three voxels over the highest.
    let (lx, ly, hx, hy) = (x + 4, y - 1, x + 6, y);
    let mut grounds = Vec::new();
    for cy in ly..=hy {
        for cx in lx..=hx {
            grounds.push(fixture.voxels().top_solid_z(cx, cy).expect("ground"));
        }
    }
    let floor = *grounds.iter().min().unwrap();
    let top = *grounds.iter().max().unwrap() + 3;
    let route = format!(
        r#"{{"label":"wall","interface":{{"beacon":{{"beacon_id":"b_00"}},"rows":[{{"add_build_target":{{"target":{{"blueprint_id":"wall","anchor":{{"voxel":{{"x":{lx},"y":{ly},"z":{}}}}},"wall_box":{{"min":{{"x":{lx},"y":{ly},"z":{floor}}},"max":{{"x":{hx},"y":{hy},"z":{top}}}}}}}}}}}]}},"timeout_ms":120000}}"#,
        floor + 1
    );
    let units = later_decisions(fixture, &route, 1_200);
    println!("wall: {units:?}");
    let columns = u64::try_from(grounds.len()).unwrap();
    assert_eq!(columns, 6, "a three-by-two box");
    assert_eq!(
        units,
        vec![b_00_rows(&world_at(SEED, 2, &[180_000])), 0, columns, 1],
        "the step's start finds b_00, the walk and interface charge nothing, the commit its six columns, then the wait one node a decision"
    );
}

/// A Defend protect list's beacon is pinned as its row commits, inside the
/// decision: a `safest` pick ranks the seat's beacons (one unit a beacon
/// row) and reads each own living beacon's threat scan, all of it charged to
/// the decision (lane `dunit`, fix pass 1). Before the pass the commit read
/// it through a board that charged nothing.
#[test]
fn a_committed_protect_list_counts_the_ranking_it_reads() {
    let wait = concat!(
        r#"{"label":"after","wait_until":{"condition":{"segment_elapsed":{"ms":{"op":"GE","value":900000}}}},"#,
        r#""timeout_ms":600000}"#
    );
    let route = concat!(
        r#"{"label":"defend","interface":{"beacon":{"beacon_id":"b_00"},"rows":[{"set_mandate":{"defend":{"#,
        r#""protect":[{"beacon":{"safest":{}}}],"engagement_radius_pct":100,"pursue":"SPHERE_EDGE"}}}]},"#,
        r#""timeout_ms":120000}"#
    );
    let mut runner = open(world_at(SEED, 2, &[180_000]), &format!("{route},{wait}"));
    without_its_miner(&mut runner);
    // Each new figure, less the decision's seam upkeep, with the count the
    // same decision's protect pick should charge, taken from the world it
    // ran on.
    let mut seen: Vec<(u64, u64)> = Vec::new();
    let _ = play(&mut runner, 1_200, |world, _| {
        if !world.is_decision_tick() {
            return;
        }
        let value = world.decision_work(0).expect("seat 0").expect("measured") - upkeep_rows(world);
        let beacon_rows = u64::try_from(world.beacons().seats().len()).unwrap();
        let own = u64::try_from(own_beacons(world).len()).unwrap();
        let pick =
            beacon_rows + own * (radius_rows(world) + threat_scan_rows(world, SeatId::new(SEAT)));
        if seen.last().map(|(units, _)| *units) != Some(value) {
            seen.push((value, pick));
        }
    });
    println!("protect: {seen:?}");
    let units: Vec<u64> = seen.iter().map(|(units, _)| *units).collect();
    let pick = seen.get(2).map(|(_, pick)| *pick).expect("a commit");
    assert!(pick > 0, "the pick ranks and scans");
    assert_eq!(
        units,
        vec![b_00_rows(&world_at(SEED, 2, &[180_000])), 0, pick, 1],
        "the step's start finds b_00; nothing while the commander walks and interfaces; at the commit the pick; then the wait one node a decision"
    );
}

#[test]
fn the_decision_tick_counts_its_evaluation_units() {
    // P1's counted half (S1's plan, section 5): one evaluation unit per
    // primitive step -- a handler tried, a condition node evaluated, a
    // selector candidate, a feature scanned, a column scanned, a node a
    // search pops -- counted per seat per decision and never hashed.
    //
    // One handler whose one-node condition is false, and a wait on a one-node
    // condition: each decision tries the handler (1), evaluates its condition
    // (1) and the wait's (1).
    let narrow = concat!(
        r#"{"label":"wait","wait_until":{"condition":{"segment_elapsed":{"ms":{"op":"GE","value":900000}}}},"#,
        r#""timeout_ms":600000}"#
    );
    // The same wait, written as two nodes deep: the same behaviour, more work.
    let wide = concat!(
        r#"{"label":"wait","wait_until":{"condition":{"all":{"items":[{"segment_elapsed":{"ms":{"op":"GE","value":900000}}}]}}},"#,
        r#""timeout_ms":600000}"#
    );
    let handler = concat!(
        r#"{"id":"never","when":{"treasury":{"dollars":{"op":"LT","value":0}}},"#,
        r#""body":[{"label":"pause","hold":{"ms":1000}}],"resume":"CONTINUE","cooldown_ms":5000,"max_fires":1}"#
    );
    let run = |route: &str| {
        let mut book = playbook(route);
        book.declarative.as_mut().unwrap().handlers =
            vec![json::decode(handler).expect("a handler")];
        let plan = Plan::compile(&book, &rules()).expect("compiles");
        let mut runner =
            Runner::new(world_at(SEED, 2, &[180_000])).unwrap_or_else(|error| panic!("{error}"));
        runner.seal_playbook(SeatId::new(SEAT), plan).unwrap();
        assert!(runner.begin_push().unwrap());
        let _ = play(&mut runner, 6, |_, _| {});
        (
            runner
                .world()
                .decision_work(0)
                .expect("seat 0")
                .expect("measured"),
            runner.world().state_hash(),
        )
    };
    let (narrow_units, narrow_hash) = run(narrow);
    let (wide_units, wide_hash) = run(wide);
    // The sixth decision's seam upkeep, measured alone: the same world with
    // no playbook sealed, whose decisions are the upkeep and nothing else.
    // By the sixth the core holds its seam, and keeping it is a dig search
    // (dunit part 2; `mining.rs`'s `a_kept_seam_charges_its_dig_search`
    // counts it).
    let upkeep = {
        let mut runner =
            Runner::new(world_at(SEED, 2, &[180_000])).unwrap_or_else(|error| panic!("{error}"));
        assert!(runner.begin_push().unwrap());
        let _ = play(&mut runner, 6, |_, _| {});
        runner.world().decision_work(0).unwrap().expect("measured")
    };
    assert!(upkeep > 0, "keeping a seam searches it");
    assert_eq!(
        narrow_units,
        3 + upkeep,
        "a handler tried, its node, the wait's node, and the seam upkeep"
    );
    assert_eq!(wide_units, 4 + upkeep, "and one more for the `all`");
    assert_eq!(
        narrow_hash, wide_hash,
        "the count is not in the state hash: the same behaviour hashes the same"
    );
}

/// A covering step's first decision: its resolve, the binding of its
/// `initial` target and the core's seam choice, each measured alone, and
/// nothing else.
#[test]
fn a_covering_decision_counts_its_resolve_binding_and_seam_choice() {
    // A covering step counts its candidates, its walks' pops and its spiral.
    let fixture = world_at(SEED, 2, &[180_000]);
    // The resolve alone, on a fresh tally and a fresh scratch: one unit a
    // feature the ranker scans, one a node each served walk pops, one a
    // spiral column the covering search tries.
    let resolve = {
        let commander = fixture
            .commander_of(SeatId::new(SEAT))
            .expect("a commander");
        let row = fixture.units().row_of(commander).expect("a live commander");
        let start = fixture
            .units()
            .positions()
            .get(row.index())
            .copied()
            .expect("the commander's row");
        let tally = pharmakos_sim::seams::UnitTally::new();
        let ground = pharmakos_sim::targeting::Ground {
            work: &tally,
            ..fixture.ground()
        };
        let mut scratch = Scratch::for_map(fixture.surface(), fixture.clusters().size()).unwrap();
        let cover = pharmakos_sim::targeting::cover_counted(
            &ground,
            &mut scratch,
            SeatId::new(SEAT),
            start,
            pharmakos_sim::targeting::FeatureSpec::Nearest {
                kind: FeatureKind::Vent,
                uncovered: true,
            },
        )
        .expect("a covering site");
        (tally.read().expect("measured"), cover)
    };
    // The step's `initial` Build target, `on covered {}`, bound at the step's
    // start against the site's sphere: the vent's liveness, its anchor in the
    // sphere, the claims on its footprint, its `on` column and that column in
    // the sphere (dunit part 2: each charged what it reads), measured alone.
    let (resolve, cover) = resolve;
    let bind = {
        let tally = pharmakos_sim::seams::UnitTally::new();
        let ground = pharmakos_sim::targeting::Ground {
            work: &tally,
            ..fixture.ground()
        };
        let mut scratch = Scratch::for_map(fixture.surface(), fixture.clusters().size()).unwrap();
        let [x, y] = cover.site;
        let centre = fixture.ground().standing(x, y).expect("the site's column");
        on_vent_counted(
            &ground,
            &mut scratch,
            SeatId::new(SEAT),
            centre,
            None,
            Some(cover.feature),
            FeatureSpec::Covered,
        )
        .expect("the covered vent binds");
        tally.read().expect("measured")
    };
    assert!(bind > 0, "binding the covered vent reads its footprint");
    // The core's seam choice, which the first decision also makes (its
    // starting mining drone gives it one, `pharmakos_sim::mining`), measured
    // alone: the same first decision with no playbook sealed
    // (`tests/mining.rs`'s `a_seam_choice_counts_its_estimates...` holds its
    // parts, one a feature ranked and one a node its walk pops).
    let seam = {
        let mut runner = Runner::new(fixture.clone()).unwrap_or_else(|error| panic!("{error}"));
        assert!(runner.begin_push().unwrap());
        let _ = play(&mut runner, 1, |_, _| {});
        runner.world().decision_work(0).unwrap().expect("measured")
    };
    let mut runner = open(fixture, COVER);
    let _ = play(&mut runner, 1, |_, _| {});
    let covering = runner.world().decision_work(0).unwrap().expect("measured");
    // The decision is the resolve, the binding and the seam choice, and
    // nothing else.
    assert_eq!(covering, resolve + bind + seam);
    // Pinned exactly on the golden seed, so a change to what the resolver
    // charges shows here rather than drifting under a lower bound: one unit a
    // visit of each loop the decision runs and one a node a search pops
    // (dunit part 2). Before lane `dunit` a served walk charged 1 whatever it
    // popped, and this read 423 (412 for the resolve and 11 for the seam
    // choice); after its part 1, 4,716 (the predicates each column and
    // candidate asks, the binding and the seam's dig searches charged
    // nothing).
    println!(
        "covering: {covering} = {resolve} for the resolve + {bind} for the binding + {seam} for the seam choice"
    );
    assert_eq!(
        covering, 10_919,
        "a covering resolution ranks every feature, walks and searches a spiral: {covering}"
    );
}

#[test]
fn a_set_mandate_whose_target_is_lost_writes_nothing() {
    // A `set_mandate` row switches the writ and writes the settings it
    // carries, as one row, all or nothing: a Build switch whose `on` target is
    // lost while the commander interfaces fails `feature_lost` and leaves the
    // beacon's old writ and its Mine settings exactly as they were.
    let mut world = world_at(SEED, 2, &[180_000]);
    let site = [335, 18, world.voxels().standing_z(335, 18)];
    let beacon = world
        .place_beacon_directly(
            SeatId::new(SEAT),
            point(site),
            MandateKind::Survey,
            PRIORITY_NORMAL,
        )
        .expect("room");
    world.set_mine_settings(
        beacon,
        pharmakos_sim::mining::MineSettings {
            dig_max_depth: 3,
            pillar_spacing: 0,
            seam_choice: pharmakos_sim::mining::SeamChoice::Richest,
            flee_on_threat: false,
        },
    );
    let row = usize::try_from(beacon.raw()).unwrap();
    let settings_before = world.beacons().mine_settings(row);
    let writ_before = world.beacons().mandates().get(row).copied();
    let vent = world
        .features()
        .index_of_name(START_VENT)
        .expect("seat 0's starting vent");
    let footprint = world.features().get(vent).unwrap().footprint.clone();
    let route = format!(
        concat!(
            r#"{{"label":"sw","interface":{{"beacon":{{"beacon_id":"b_01"}},"rows":[{{"set_mandate":"#,
            r#"{{"build":{{"targets":[{{"blueprint_id":"generator","anchor":{{"on":{{"feature_id":"{v}"}}}}}}]}}}}}}]}},"#,
            r#""timeout_ms":120000,"on_fail":{{"action":"SKIP"}}}}"#
        ),
        v = START_VENT
    );
    let mut runner = open(world, &route);
    let mut started = false;
    let mut destroyed = false;
    let events = play(&mut runner, 2_400, |world, events| {
        if events.iter().any(|e| e.kind == EventKind::VisitStarted) {
            started = true;
        }
        if started && !destroyed {
            for column in &footprint {
                assert!(
                    world
                        .request_voxel_edit(VoxelEdit::Set {
                            at: [column.x, column.y, column.top],
                            material: Material::STONE,
                            by: None,
                        })
                        .is_ok()
                );
            }
            destroyed = true;
        }
    });
    assert!(destroyed, "the visit started");
    assert_eq!(failures(&events), vec![id(StepFailure::FeatureLost)]);
    assert_eq!(count(&events, EventKind::RowCommitted), 0);
    let world = runner.world();
    assert_eq!(world.beacons().mandates().get(row).copied(), writ_before);
    assert_eq!(world.beacons().mine_settings(row), settings_before);
}

// ---------------------------------------------------------------------------
// The fog follow-up: the sphere first, and the count on `step_failed`
// (S1's plan, task `fog`; decisions-log items 133 (3) (c), (h) and 134)
// ---------------------------------------------------------------------------

/// Every `step_failed` among `events`, decoded through the sim's one layout.
fn failed(events: &[Event]) -> Vec<StepFailed> {
    failures(events)
        .into_iter()
        .map(|value| StepFailed::decode(value).expect("the sim writes values that decode"))
        .collect()
}

/// An `on` read over `world`, as the decision phase reads it, with a scratch
/// of the caller's (the gateway's way of asking).
fn on_read(
    world: &World,
    centre: [Fx; 3],
    covered: Option<usize>,
    spec: FeatureSpec,
) -> Result<pharmakos_sim::targeting::OnSite, StepFailed> {
    let mut scratch = Scratch::for_map(world.surface(), world.rules().hpa_cluster_voxels())
        .expect("a scratch for the map");
    on_vent_counted(
        &world.ground(),
        &mut scratch,
        SeatId::new(SEAT),
        centre,
        None,
        covered,
        spec,
    )
}

/// The standing point of seat 0's core: 35 voxels from `vent_324_16`, so the
/// vent's anchor point is outside the core's 24-voxel sphere.
fn core_point(world: &World) -> [Fx; 3] {
    point(core_voxel(world))
}

/// The starting vent's index, and its name as a spec.
fn start_vent(world: &World) -> (usize, FeatureSpec) {
    let vent = world
        .features()
        .index_of_name(START_VENT)
        .expect("seat 0's starting vent");
    let anchor = world.features().get(vent).unwrap().anchor;
    (
        vent,
        FeatureSpec::Name {
            kind: FeatureKind::Vent,
            anchor,
        },
    )
}

/// A live Generator of seat 1's on the vent's anchor column.
fn generator_on_vent(world: &mut World, vent: usize) {
    let [x, y] = world.features().get(vent).unwrap().anchor;
    let one = SeatId::new(1);
    let home = core_of(world, one);
    world
        .raise_structure(
            one,
            home,
            StructureKind::Generator,
            point([x, y, world.voxels().standing_z(x, y)]),
        )
        .expect("room");
}

/// A route that interfaces seat 0's core and adds a Generator target `on`
/// the starting vent by name.
fn named_on_core() -> String {
    format!(
        concat!(
            r#"{{"label":"named","interface":{{"beacon":{{"beacon_id":"b_00"}},"rows":[{{"add_build_target":"#,
            r#"{{"target":{{"blueprint_id":"generator","anchor":{{"on":{{"feature_id":"{v}"}}}}}}}}}}]}},"#,
            r#""timeout_ms":60000,"on_fail":{{"action":"SKIP"}}}}"#
        ),
        v = START_VENT
    )
}

#[test]
fn a_named_vent_outside_the_sphere_is_no_target_whatever_stands_on_it() {
    // Seat 0's core sphere does not hold `vent_324_16`'s anchor point. With
    // nothing on the vent and with another seat's live Generator on it, the
    // answer is the same `no_target`, counted 0, so the step reveals nothing
    // about a structure the seat cannot see (item 133 (3) (c)). Before the
    // sphere test came first the second read `illegal_site` and the first
    // bound a vent outside the sphere.
    for occupied in [false, true] {
        let mut world = world_at(SEED, 2, &[180_000]);
        let (vent, spec) = start_vent(&world);
        if occupied {
            generator_on_vent(&mut world, vent);
        }
        let centre = core_point(&world);
        assert!(
            !anchor_in_sphere(&world.ground(), centre, vent),
            "the fixture's vent lies outside the core's sphere"
        );
        assert_eq!(
            on_read(&world, centre, None, spec),
            Err(StepFailed::counted(StepFailure::NoTarget, 0)),
            "occupied: {occupied}"
        );
        let mut runner = open(world, &named_on_core());
        let events = play(&mut runner, 200, |_, _| {});
        assert_eq!(
            failed(&events),
            vec![StepFailed::counted(StepFailure::NoTarget, 0)],
            "occupied: {occupied}"
        );
        assert_eq!(count(&events, EventKind::RowCommitted), 0);
    }
}

#[test]
fn a_covered_vent_outside_the_sphere_is_no_target_whatever_stands_on_it() {
    // `covered {}` binds as a name, so it reads sphere first too: the vent a
    // `covering` bound, read from a sphere that does not hold its anchor
    // point, is `no_target` with or without a Generator on it.
    for occupied in [false, true] {
        let mut world = world_at(SEED, 2, &[180_000]);
        let (vent, _) = start_vent(&world);
        if occupied {
            generator_on_vent(&mut world, vent);
        }
        let centre = core_point(&world);
        assert_eq!(
            on_read(&world, centre, Some(vent), FeatureSpec::Covered),
            Err(StepFailed::counted(StepFailure::NoTarget, 0)),
            "occupied: {occupied}"
        );
    }
}

#[test]
fn a_named_vent_inside_the_sphere_with_a_generator_is_still_illegal_site() {
    // Inside the sphere the structure tests follow the sphere test: a vent
    // another seat's live Generator stands on is `illegal_site`, counted 1 (a
    // name matched one vent), while the same vent with nothing on it binds.
    let mut world = world_at(SEED, 2, &[180_000]);
    let (vent, spec) = start_vent(&world);
    let centre = point([335, 18, world.voxels().standing_z(335, 18)]);
    assert!(anchor_in_sphere(&world.ground(), centre, vent));
    let free = on_read(&world, centre, None, spec).expect("a free vent inside binds");
    assert_eq!(free.feature, vent);
    generator_on_vent(&mut world, vent);
    assert_eq!(
        on_read(&world, centre, None, spec),
        Err(StepFailed::counted(StepFailure::IllegalSite, 1))
    );
}

#[test]
fn a_named_vent_whose_free_on_column_is_outside_the_sphere_is_illegal_site() {
    // A structure that is not a Generator on the vent's anchor column moves
    // its `on` column to the next free footprint column. A sphere that holds
    // the anchor point and not that column passes the sphere test and fails
    // the site: `illegal_site`, counted 1.
    let mut world = world_at(SEED, 2, &[180_000]);
    let (vent, spec) = start_vent(&world);
    let [x, y] = world.features().get(vent).unwrap().anchor;
    let zero = SeatId::new(SEAT);
    let home = core_of(&world, zero);
    world
        .raise_structure(
            zero,
            home,
            StructureKind::SurveyPost,
            point([x, y, world.voxels().standing_z(x, y)]),
        )
        .expect("room");
    let ground = world.ground();
    let anchor = ground.anchor_point(vent).expect("the anchor stands");
    let [ox, oy] = ground
        .on_column(vent, zero, None)
        .expect("a free footprint column");
    assert_ne!([ox, oy], [x, y], "the anchor column is taken");
    let on = ground.standing(ox, oy).expect("the on column stands");
    let [ax, ay, az] = anchor;
    let reach = ground.sphere_radius() + 1;
    let centre = (-reach..=reach)
        .flat_map(|dy| (-reach..=reach).map(move |dx| (dx, dy)))
        .map(|(dx, dy)| {
            [
                Fx::from_voxels(i16::try_from(ax.floor_voxels() + dx).expect("on the map")),
                Fx::from_voxels(i16::try_from(ay.floor_voxels() + dy).expect("on the map")),
                az,
            ]
        })
        .find(|centre| ground.in_sphere(*centre, anchor) && !ground.in_sphere(*centre, on))
        .expect("a sphere's edge between the anchor point and the on column");
    assert!(anchor_in_sphere(&ground, centre, vent));
    assert_eq!(
        on_read(&world, centre, None, spec),
        Err(StepFailed::counted(StepFailure::IllegalSite, 1))
    );
}

#[test]
fn a_covering_site_holds_the_anchor_point_its_covered_on_tests_first() {
    // A structure that is not a Generator on a vent's anchor column moves its
    // `on` column off the anchor point. A `covering` whose site's sphere held
    // the `on` column alone would bind a site from which its own `covered {}`
    // answers `no_target` (the sphere test reads the anchor point first), and
    // the vent would still read as `UNCOVERED` once the beacon landed. The
    // fixture is the second review's of PR #90: `vent_103_111` with a
    // SurveyPost of seat 0's on its anchor column, and a beacon of seat 0's
    // due north of it, its sphere reaching the vent's edge.
    let zero = SeatId::new(SEAT);
    let mut base = world_at(SEED, 2, &[180_000]);
    let vent = base
        .features()
        .index_of_name("vent_103_111")
        .expect("the golden seed carries the vent");
    let [ax, ay] = base.features().get(vent).unwrap().anchor;
    let home = core_of(&base, zero);
    base.raise_structure(
        zero,
        home,
        StructureKind::SurveyPost,
        point([ax, ay, base.voxels().standing_z(ax, ay)]),
    )
    .expect("room");
    let mut sited: Vec<i32> = Vec::new();
    for by in [64, 65] {
        let mut world = base.clone();
        let centre = point([ax, by, world.voxels().standing_z(ax, by)]);
        world
            .place_beacon_directly(zero, centre, MandateKind::Build, PRIORITY_NORMAL)
            .expect("room for a beacon");
        let ground = world.ground();
        let on = ground.on_column(vent, zero, None).expect("a free column");
        assert_ne!(on, [ax, ay], "the anchor column is taken");
        let Some([sx, sy]) = covering_site(&ground, zero, centre, vent) else {
            continue;
        };
        let site = ground.standing(sx, sy).expect("a site stands");
        assert!(
            anchor_in_sphere(&ground, site, vent),
            "beacon at y {by}: the site [{sx}, {sy}] holds the anchor point"
        );
        let bound =
            on_read(&world, site, Some(vent), FeatureSpec::Covered).unwrap_or_else(|failed| {
                panic!("beacon at y {by}: covered {{}} binds, not {failed:?}")
            });
        assert_eq!(
            (bound.feature, bound.column),
            (vent, on),
            "beacon at y {by}"
        );
        sited.push(by);
    }
    // At the review's own beacon (y 64) the site it found, [103, 87], held the
    // `on` column and not the anchor point; no site holds both now, so the
    // `covering` fails `illegal_site` rather than its `covered {}`. One voxel
    // closer, a site holds both and the `covered {}` binds.
    assert_eq!(sited, [65]);
}

#[test]
fn a_description_counts_what_it_matched_before_reachability() {
    // "3 matched, none reachable": the count is what the filter admitted,
    // asked before reachability. From an origin off the map nothing is
    // reachable, so every live vent is matched and none is ranked.
    let world = world_at(SEED, 2, &[180_000]);
    let ground = world.ground();
    let seat = SeatId::new(SEAT);
    let pick = |index: usize| {
        pharmakos_sim::targeting::matches_pick(&ground, seat, index, FeatureKind::Vent, false)
    };
    let vents = (0..world.features().len())
        .filter(|index| pick(*index))
        .count();
    assert!(vents > 1, "the golden seed carries several live vents");
    let mut ranker = Ranker::new(&ground, [-1_000, -1_000], pick);
    assert_eq!(
        usize::try_from(ranker.matched()).expect("a few"),
        vents,
        "matched before reachability"
    );
    let mut scratch = Scratch::for_map(world.surface(), world.rules().hpa_cluster_voxels())
        .expect("a scratch for the map");
    assert_eq!(ranker.next(&ground, &mut scratch), None, "none reachable");
}

#[test]
fn a_step_failed_value_decodes_to_its_reason_and_its_count() {
    for reason in StepFailure::ALL {
        for matched in [0, 1, 3, u32::MAX] {
            let failed = StepFailed::counted(reason, matched);
            assert_eq!(StepFailed::decode(failed.value()), Ok(failed));
        }
    }
    assert_eq!(
        StepFailed::decode(StepFailed::counted(StepFailure::NoTarget, 3).value()),
        Ok(StepFailed {
            reason: StepFailure::NoTarget,
            matched: Some(3),
        })
    );
}

#[test]
fn a_bare_reason_id_decodes_as_the_reason_with_no_count() {
    // Every value a `step_failed` carried before the count existed reads as
    // it always did: a scenario's or a transcript's bare id.
    for reason in StepFailure::ALL {
        let value = i64::from(reason.id());
        assert_eq!(StepFailed::decode(value), Ok(StepFailed::bare(reason)));
        assert_eq!(StepFailed::bare(reason).value(), value);
        assert_eq!(StepFailed::from(reason), StepFailed::bare(reason));
    }
    assert_ne!(
        StepFailed::counted(StepFailure::NoTarget, 0).value(),
        StepFailed::bare(StepFailure::NoTarget).value(),
        "an absent count is not a count of 0"
    );
}

#[test]
fn a_value_that_does_not_decode_is_refused_with_a_typed_error() {
    let retired = i64::from(StepFailure::RETIRED_NOT_OWN);
    assert_eq!(
        StepFailed::decode(retired),
        Err(StepFailedError::UnknownReason { value: retired })
    );
    assert_eq!(
        StepFailed::decode(0),
        Err(StepFailedError::UnknownReason { value: 0 })
    );
    assert_eq!(
        StepFailed::decode(-1),
        Err(StepFailedError::ReservedBits { value: -1 })
    );
    for value in [1 << 9, (1 << 15) | 2, (1 << 48) | 2] {
        assert_eq!(
            StepFailed::decode(value),
            Err(StepFailedError::ReservedBits { value })
        );
    }
    let stray = (3 << 16) | 2;
    assert_eq!(
        StepFailed::decode(stray),
        Err(StepFailedError::StrayCount { value: stray })
    );
    assert_eq!(StepFailedError::StrayCount { value: stray }.value(), stray);
}

#[test]
fn a_rules_table_without_a_voxel_length_sphere_does_not_load() {
    // The resolver reads `beacon.sphere_radius_voxels` as it is, so the table
    // refuses one it could not read: a missing block, or a radius past a
    // voxel coordinate (no clamp and no zero in its place).
    let mut message = rules().message().clone();
    message.beacon = None;
    assert_eq!(
        RulesTable::from_message(&message),
        Err(pharmakos_sim::RulesError::MissingBlock("beacon"))
    );
    let mut message = rules().message().clone();
    if let Some(beacon) = message.beacon.as_mut() {
        beacon.sphere_radius_voxels = 40_000;
    }
    assert!(matches!(
        RulesTable::from_message(&message),
        Err(pharmakos_sim::RulesError::OutOfRange { field, .. })
            if field == "beacon.sphere_radius_voxels"
    ));
}
