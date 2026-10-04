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
use pharmakos_sim::events::{Event, EventKind};
use pharmakos_sim::interpreter::state::StepFailure;
use pharmakos_sim::interpreter::{BeaconSpec, Plan, PlanError, resolve_beacon_in};
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::math::quantity::Hp;
use pharmakos_sim::runner::{MatchPhase, MatchSettings, Runner};
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::snapshot::Snapshot;
use pharmakos_sim::tables::{PRIORITY_NORMAL, SeatId, StructureKind, UnitId, own_beacon_name};
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
        units_per_seat: 0,
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
    let mut runner = Runner::new(world);
    runner
        .seal_playbook(SeatId::new(SEAT), plan)
        .expect("a Lull takes a seal");
    assert!(runner.begin_push(), "a match opens in a Lull");
    runner
}

/// Play the Push in progress to its end (or `limit` ticks), keeping seat 0's
/// events. `each` sees the world after every tick.
fn play(runner: &mut Runner, limit: u32, mut each: impl FnMut(&mut World, &[Event])) -> Vec<Event> {
    let mut kept: Vec<Event> = Vec::new();
    let mut ticks: u32 = 0;
    while ticks < limit && runner.phase() == MatchPhase::Push {
        let Some(_) = runner.step() else {
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
    let row = world
        .beacons()
        .row_of_ordinal(SeatId::new(SEAT), 0)
        .expect("seat 0's core is its b_00");
    world
        .beacons()
        .positions()
        .get(row)
        .copied()
        .expect("a position")
        .map(Fx::floor_voxels)
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
    let snapshot = Snapshot::capture(&world);
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

// ---------------------------------------------------------------------------
// The resolver in a world, and the three reading rules
// ---------------------------------------------------------------------------

#[test]
fn a_carried_covering_step_finds_no_vent_after_round_one_on_the_golden_seed() {
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
        if runner.phase() != MatchPhase::Recap || !runner.end_recap() {
            break;
        }
        if !runner.begin_push() {
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
        assert_eq!(failures(later), vec![id(StepFailure::IllegalSite)]);
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
                assert!(world.request_voxel_edit(VoxelEdit::Set {
                    at: [column.x, column.y, column.top],
                    material: Material::STONE,
                }));
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
    assert_eq!(
        failures(&events),
        vec![id(StepFailure::IllegalSite), id(StepFailure::NoTarget)]
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
    let commander = runner.world().commander_of(SeatId::new(SEAT));
    let treasury = |world: &World| world.seats().treasuries().first().expect("seat 0").raw();
    let before = treasury(runner.world());
    let mut killed = false;
    let events = play(&mut runner, 1_600, |world, events| {
        if !killed
            && events
                .iter()
                .any(|event| event.kind == EventKind::BeaconPlaced)
        {
            assert!(world.request_damage(DamageOrder {
                target: DamageTarget::Unit(UnitId::new(commander.raw())),
                amount: Hp::new(10_000),
                by: SeatId::NEUTRAL,
            }));
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
        assert!(runner.world_mut().request_voxel_edit(VoxelEdit::Set {
            at: [pit[0], pit[1], top - depth],
            material: Material::AIR,
        }));
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
    let saved = Snapshot::capture(&world);
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

#[test]
fn the_decision_tick_counts_its_evaluation_units() {
    // P1's counted half (S1's plan, section 5): one evaluation unit per
    // handler tried, per condition node evaluated, per selector candidate and
    // per "nearest" estimate, counted per seat per decision and never hashed.
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
        let mut runner = Runner::new(world_at(SEED, 2, &[180_000]));
        runner.seal_playbook(SeatId::new(SEAT), plan).unwrap();
        assert!(runner.begin_push());
        let _ = play(&mut runner, 6, |_, _| {});
        (
            runner.world().decision_work(0).expect("seat 0").spent(),
            runner.world().state_hash(),
        )
    };
    let (narrow_units, narrow_hash) = run(narrow);
    let (wide_units, wide_hash) = run(wide);
    assert_eq!(
        narrow_units, 3,
        "a handler tried, its node, the wait's node"
    );
    assert_eq!(wide_units, 4, "and one more for the `all`");
    assert_eq!(
        narrow_hash, wide_hash,
        "the count is not in the state hash: the same behaviour hashes the same"
    );

    // A covering step counts its candidates, its estimates and its spiral.
    let mut runner = open(world_at(SEED, 2, &[180_000]), COVER);
    let _ = play(&mut runner, 1, |_, _| {});
    let covering = runner.world().decision_work(0).unwrap().spent();
    assert!(
        covering > 12,
        "a covering resolution ranks every feature and walks a spiral: {covering}"
    );
}
