// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! S1's Build settings (S1's plan, task `build`, and its decision 5): the
//! targets in full -- their `order` and their rotation -- and protected areas
//! kept clear of construction; and decision 15's interface pricing brought to
//! the sim (the register's S1-51), so the Push charges a visit what the editor
//! quotes.
//!
//! Each acceptance line of the task has a test of its own name here:
//! `targets_build_in_their_order`, `nothing_is_built_inside_a_protected_area`
//! and `unaffordable_targets_wait_for_the_highest_order_affordable_one`. The
//! fourth, `the_plan_fingerprint_is_unmoved`, is in `tests/vectors.rs` beside
//! the other pinned values.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules, not an integration test's helpers. A panic is this file's failure report."
)]

use pharmakos_proto::gp;
use pharmakos_proto::json;
use pharmakos_sim::economy::Purchase;
use pharmakos_sim::events::EventKind;
use pharmakos_sim::interpreter::{Action, Plan, PlanError, Row};
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::math::quantity::Money;
use pharmakos_sim::runner::{MatchSettings, Runner};
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::snapshot::{Snapshot, SnapshotError};
use pharmakos_sim::tables::{AreaKind, BeaconId, ColumnBox, SeatId, StructureKind, TargetKind};
use pharmakos_sim::{RulesTable, World, WorldConfig, mandate_for};
use std::path::PathBuf;

/// The seed `tests/economy.rs` and `tests/mining.rs` play on.
const SEED: u64 = 0x00A1_B2C3_D4E5_F607;

/// Three seats, v1's most.
const SEATS: u32 = 3;

const SEAT: SeatId = SeatId::new(0);

/// Enough `$` that no target here waits for money unless a test says so.
const RICH: i64 = 100_000;

fn rules() -> RulesTable {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    RulesTable::load(&root.join("rules").join("rules.v1.json"))
        .expect("the committed rules table loads")
}

fn world() -> World {
    World::new(&WorldConfig {
        match_seed: SEED,
        seats: SEATS,
        units_per_seat: 0,
        rules: rules(),
        match_settings: MatchSettings {
            segment_lengths_ms: vec![180_000],
            round_limit: 1,
        },
    })
    .expect("the rules table describes a map")
}

/// Seat 0's core, its `b_00`, which starts on the Build writ.
fn core(world: &World) -> BeaconId {
    let row = world
        .beacons()
        .row_of_ordinal(SEAT, 0)
        .expect("seat 0 has a core");
    BeaconId::new(u32::try_from(row).expect("a beacon row"))
}

/// The core's anchor in whole voxels.
fn core_voxel(world: &World) -> [i32; 3] {
    let row = usize::try_from(core(world).raw()).expect("a row");
    let [x, y, z] = world
        .beacons()
        .positions()
        .get(row)
        .copied()
        .expect("the core stands somewhere");
    [x.floor_voxels(), y.floor_voxels(), z.floor_voxels()]
}

/// A whole playbook around one route step, as canonical `gp.v1` JSON.
fn playbook(route: &str) -> gp::v1::Playbook {
    let text = format!(
        concat!(
            "{{\"schema_version\":{{\"major\":1}},",
            "\"meta\":{{\"title\":\"build\",\"author_kind\":\"HUMAN\"}},",
            "\"declarative\":{{\"route\":[{route}]}},",
            "\"on_death\":{{\"on_respawn\":\"CONTINUE\",\"max_deaths_before_fallback\":3}},",
            "\"fallback\":{{\"hold\":{{\"at\":{{\"safest\":{{}}}}}}}},",
            "\"kind\":\"PLAYBOOK\"}}"
        ),
        route = route
    );
    json::decode(&text).unwrap_or_else(|error| panic!("the case decodes: {error}\n{text}"))
}

/// One Build target as canonical JSON: a generator at `at`, with an order and
/// a rotation.
fn target(at: [i32; 3], order: u32, turns: u32) -> String {
    let [x, y, z] = at;
    format!(
        r#"{{"blueprint_id":"generator","anchor":{{"voxel":{{"x":{x},"y":{y},"z":{z}}}}},"order":{order},"rotation_quarter_turns":{turns}}}"#
    )
}

/// A visit to `b_00` with one `set_mandate_settings` row writing `build`.
fn visit_writing(build: &str) -> String {
    format!(
        r#"{{"label":"build","interface":{{"beacon":{{"beacon_id":"b_00"}},"rows":[{{"set_mandate_settings":{{"build":{build}}}}}]}},"timeout_ms":120000}}"#
    )
}

/// Seal `route` for seat 0 over `world` and play `ticks` ticks of the Push,
/// returning the runner and every `structure_queued` event's tick.
fn play(world: World, route: &str, ticks: u32) -> (Runner, Vec<u32>, bool) {
    let plan = Plan::compile(&playbook(route), &rules()).expect("compiles");
    let mut runner = Runner::new(world);
    runner
        .seal_playbook(SEAT, plan)
        .expect("a Lull takes a seal");
    assert!(runner.begin_push());
    let mut queued = Vec::new();
    let mut failed = false;
    for _ in 0..ticks {
        if runner.step().is_none() {
            break;
        }
        for event in runner.events() {
            if event.seat == Some(SEAT) {
                if event.kind == EventKind::StructureQueued {
                    queued.push(event.tick.raw());
                }
                if event.kind == EventKind::StepFailed {
                    failed = true;
                }
            }
        }
        runner.clear_events();
    }
    (runner, queued, failed)
}

/// Seat 0's structures, in the order they were paid for (ascending id), as
/// `(column, rotation)`.
fn built(world: &World) -> Vec<([i32; 2], u8)> {
    let structures = world.structures();
    structures
        .seats()
        .iter()
        .zip(structures.positions())
        .zip(structures.rotations())
        .filter(|((owner, _), _)| **owner == SEAT.raw())
        .map(|((_, at), turns)| {
            let [x, y, _] = *at;
            ([x.floor_voxels(), y.floor_voxels()], *turns)
        })
        .collect()
}

fn column(at: [i32; 3]) -> [i32; 2] {
    let [x, y, _] = at;
    [x, y]
}

fn treasury(world: &World) -> Money {
    let row = world.seat_row(SEAT).expect("seat 0 is seated");
    world
        .seats()
        .treasuries()
        .get(row)
        .copied()
        .expect("a treasury")
}

/// S1's plan, decision 5: the mandate builds the **highest-order** target
/// first -- the lowest `order`, ties by list position -- and the structure
/// takes the target's rotation.
#[test]
fn targets_build_in_their_order() {
    let mut fixture = world();
    fixture.set_treasury(SEAT, Money::new(RICH));
    let [x, y, z] = core_voxel(&fixture);
    let last = [x + 6, y, z];
    let first = [x + 3, y + 2, z];
    let second = [x + 3, y - 2, z];
    let build = format!(
        r#"{{"targets":[{},{},{}]}}"#,
        target(last, 5, 0),
        target(first, 1, 3),
        target(second, 1, 2),
    );
    let (runner, queued, failed) = play(fixture, &visit_writing(&build), 800);
    assert!(!failed, "nothing in the visit fails");
    assert_eq!(queued.len(), 3, "every target is paid for: {queued:?}");
    assert_eq!(
        built(runner.world()),
        vec![(column(first), 3), (column(second), 2), (column(last), 0),],
        "order 1 before order 5, the two order 1 targets in list order, each \
         standing at its own rotation"
    );
    // The order and the rotation are hashed state and ride the snapshot.
    let world = runner.world();
    let targets = world.targets();
    let rows: Vec<(u32, u8)> = targets
        .kinds()
        .iter()
        .zip(targets.orders().iter().zip(targets.rotations()))
        .filter(|(kind, _)| **kind == TargetKind::Build.id())
        .map(|(_, (order, turns))| (*order, *turns))
        .collect();
    assert_eq!(rows, vec![(5, 0), (1, 3), (1, 2)], "the list as written");
    let snapshot = Snapshot::capture(world);
    let mut restored = self::world();
    snapshot
        .restore_into(&mut restored)
        .expect("the snapshot restores");
    assert_eq!(restored.state_hash(), world.state_hash());
    assert_eq!(restored.targets().orders(), world.targets().orders());
    assert_eq!(restored.targets().rotations(), world.targets().rotations());
    assert_eq!(
        restored.structures().rotations(),
        world.structures().rotations()
    );
}

/// S1's plan, decision 5: protected areas are kept clear of construction. The
/// target in the area is never paid for, though it is the highest-order one,
/// and the one outside it is. The protected target stands on the box's
/// **corner**: the box is tested edges included.
#[test]
fn nothing_is_built_inside_a_protected_area() {
    let mut fixture = world();
    fixture.set_treasury(SEAT, Money::new(RICH));
    let [x, y, z] = core_voxel(&fixture);
    let corner = [x + 8, y + 3, z];
    let outside = [x - 5, y, z];
    let build = format!(
        r#"{{"targets":[{},{}],"protected_areas":[{{"min":{{"x":{},"y":{},"z":{z}}},"max":{{"x":{},"y":{},"z":{z}}}}}]}}"#,
        target(corner, 0, 0),
        target(outside, 1, 0),
        x + 4,
        y - 1,
        x + 8,
        y + 3,
    );
    let (runner, queued, failed) = play(fixture, &visit_writing(&build), 1_200);
    assert!(!failed, "nothing in the visit fails");
    assert_eq!(queued.len(), 1, "one structure is paid for: {queued:?}");
    assert_eq!(
        built(runner.world()),
        vec![(column(outside), 0)],
        "only the target outside the protected area is built"
    );
    let world = runner.world();
    let request = mandate_for(MandateKind::Build)
        .expect("Build runs")
        .request(world, core(world));
    assert!(
        !matches!(
            request,
            Some(pharmakos_sim::SpendRequest {
                buys: Purchase::Structure(_),
                ..
            })
        ),
        "the protected target is not asked for: {request:?}"
    );
}

/// A protected area is **the box the author drew**, not a disc around it: a
/// target one column outside each of its four sides is built, and only the
/// one inside it is not (S1's `build` lane, from its review -- a covering
/// radius had protected the column east of this box, which the author never
/// drew).
#[test]
fn a_target_one_column_outside_a_protected_box_is_built() {
    let mut fixture = world();
    fixture.set_treasury(SEAT, Money::new(RICH));
    let [x, y, z] = core_voxel(&fixture);
    let inside = [x + 6, y + 1, z];
    let west = [x + 3, y + 1, z];
    let east = [x + 9, y + 1, z];
    let south = [x + 6, y - 2, z];
    let north = [x + 6, y + 4, z];
    let build = format!(
        r#"{{"targets":[{},{},{},{},{}],"protected_areas":[{{"min":{{"x":{},"y":{},"z":{z}}},"max":{{"x":{},"y":{},"z":{z}}}}}]}}"#,
        target(inside, 0, 0),
        target(west, 1, 0),
        target(east, 2, 0),
        target(south, 3, 0),
        target(north, 4, 0),
        x + 4,
        y - 1,
        x + 8,
        y + 3,
    );
    let (runner, queued, failed) = play(fixture, &visit_writing(&build), 1_600);
    assert!(!failed, "nothing in the visit fails");
    assert_eq!(
        built(runner.world()),
        vec![
            (column(west), 0),
            (column(east), 0),
            (column(south), 0),
            (column(north), 0),
        ],
        "every target outside the box is built, in its order, and the one          inside it is not: {queued:?}"
    );
}

/// A probe area is the box too: a scout standing on one of its columns is
/// inside it, whatever its height, and one column past its edge it walks to
/// the area's centre.
#[test]
fn a_scout_is_inside_a_probe_area_exactly_on_its_box() {
    let mut fixture = world();
    let beacon = core(&fixture);
    let [x, y, z] = core_voxel(&fixture);
    let voxel = |v: i32| i16::try_from(v).expect("a voxel coordinate");
    let at = |vx: i32, vy: i32, vz: i32| {
        [
            Fx::from_voxels(voxel(vx)),
            Fx::from_voxels(voxel(vy)),
            Fx::from_voxels(voxel(vz)),
        ]
    };
    let area = ColumnBox::new([voxel(x + 4), voxel(y - 1)], [voxel(x + 8), voxel(y + 3)])
        .expect("the box is the right way out");
    let centre = at(x + 6, y + 1, z);
    assert!(fixture.add_area(beacon, AreaKind::Probe, centre, area));
    for inside in [
        at(x + 4, y - 1, z),
        at(x + 8, y + 3, z + 20),
        at(x + 6, y + 1, z - 5),
    ] {
        assert_eq!(fixture.probe_to_enter(beacon, inside), None, "{inside:?}");
    }
    for outside in [
        at(x + 3, y + 1, z),
        at(x + 9, y + 1, z),
        at(x + 6, y - 2, z),
        at(x + 6, y + 4, z),
        // The corner the old covering disc reached diagonally.
        at(x + 9, y + 4, z),
    ] {
        assert_eq!(
            fixture.probe_to_enter(beacon, outside),
            Some(centre),
            "{outside:?}"
        );
    }
    // An inside-out box is no box at all, and a playbook that writes one is
    // refused at the seal.
    assert_eq!(ColumnBox::new([3, 0], [2, 0]), None);
    let build = format!(
        r#"{{"protected_areas":[{{"min":{{"x":{},"y":{y},"z":{z}}},"max":{{"x":{},"y":{y},"z":{z}}}}}]}}"#,
        x + 1,
        x,
    );
    assert_eq!(
        Plan::compile(&playbook(&visit_writing(&build)), &rules()).err(),
        Some(PlanError::AreaInsideOut)
    );
}

/// An area's box rides the snapshot, and a restore refuses, by name, a value
/// its column's type cannot hold -- a rotation past three quarter turns, an
/// inside-out box -- rather than reporting it as ragged columns.
#[test]
fn an_area_rides_the_snapshot_and_a_value_out_of_range_is_refused_by_name() {
    let mut fixture = world();
    let beacon = core(&fixture);
    let [x, y, z] = core_voxel(&fixture);
    let voxel = |v: i32| i16::try_from(v).expect("a voxel coordinate");
    let centre = [
        Fx::from_voxels(voxel(x + 6)),
        Fx::from_voxels(voxel(y + 1)),
        Fx::from_voxels(voxel(z)),
    ];
    let area = ColumnBox::new([voxel(x + 4), voxel(y - 1)], [voxel(x + 8), voxel(y + 3)])
        .expect("the box is the right way out");
    assert!(fixture.add_area(beacon, AreaKind::Protected, centre, area));
    assert!(fixture.add_target(
        beacon,
        StructureKind::Generator.id(),
        [
            Fx::from_voxels(voxel(x - 5)),
            Fx::from_voxels(voxel(y)),
            Fx::from_voxels(voxel(z)),
        ]
    ));
    let snapshot = Snapshot::capture(&fixture);
    let mut restored = world();
    snapshot
        .restore_into(&mut restored)
        .expect("the snapshot restores");
    assert_eq!(restored.targets().areas(), fixture.targets().areas());
    assert_eq!(restored.state_hash(), fixture.state_hash());

    let mut turned = snapshot.clone();
    *turned.target_rotation.first_mut().expect("a target row") = 4;
    assert_eq!(
        turned.restore_into(&mut world()),
        Err(SnapshotError::OutOfRange("target_rotation"))
    );
    let mut inside_out = snapshot.clone();
    let row = fixture
        .targets()
        .kinds()
        .iter()
        .position(|kind| *kind == TargetKind::Protected.id())
        .expect("the area's row");
    // `[min x, min y, max x, max y]`: a min x past the max x.
    *inside_out
        .target_area
        .get_mut(row * 4)
        .expect("the area's min x") = voxel(x + 9);
    assert_eq!(
        inside_out.restore_into(&mut world()),
        Err(SnapshotError::OutOfRange("target_area"))
    );
    let mut short = snapshot;
    short.target_area.pop();
    assert_eq!(
        short.restore_into(&mut world()),
        Err(SnapshotError::Ragged("target_area"))
    );
}

/// The Build mandate builds "the highest-order **affordable** target" (spec
/// section 6), and a Build row is not a spend (decision 7): a target the
/// treasury cannot cover waits, nothing fails, nothing is charged, and once
/// the treasury covers it the highest-order target is the one paid for.
#[test]
fn unaffordable_targets_wait_for_the_highest_order_affordable_one() {
    let mut fixture = world();
    let cost = fixture.structure_cost(StructureKind::Generator);
    fixture.set_treasury(SEAT, Money::ZERO);
    let [x, y, z] = core_voxel(&fixture);
    let low = [x + 3, y, z];
    let high = [x - 3, y, z];
    let build = format!(
        r#"{{"targets":[{},{}]}}"#,
        target(low, 7, 0),
        target(high, 2, 0)
    );
    let (mut runner, queued, failed) = play(fixture, &visit_writing(&build), 400);
    assert!(
        treasury(runner.world()).raw() < cost.raw(),
        "the starting drone's ore has not covered a Generator yet, so the test          still reads an unaffordable target"
    );
    assert!(!failed, "an order the treasury cannot cover fails nothing");
    assert!(queued.is_empty(), "nothing is paid for: {queued:?}");
    assert!(
        built(runner.world()).is_empty(),
        "and nothing stands, though the targets are written"
    );
    // The mandate still names the highest-order target, for the
    // Quartermaster to hold.
    let beacon = core(runner.world());
    let request = mandate_for(MandateKind::Build)
        .expect("Build runs")
        .request(runner.world(), beacon)
        .expect("an unpaid target is asked for");
    let row = match request.buys {
        Purchase::Structure(row) => usize::try_from(row).expect("a row"),
        Purchase::Unit(kind) => panic!("a structure, not a {kind:?}"),
    };
    assert_eq!(runner.world().targets().orders().get(row), Some(&2));

    // Covered: the highest-order target is paid for first, then the other.
    runner.world_mut().set_treasury(SEAT, Money::new(RICH));
    let mut paid = Vec::new();
    for _ in 0..40 {
        if runner.step().is_none() {
            break;
        }
        paid.extend(
            runner
                .events()
                .iter()
                .filter(|event| event.seat == Some(SEAT))
                .filter(|event| event.kind == EventKind::StructureQueued)
                .map(|event| event.tick.raw()),
        );
        runner.clear_events();
    }
    assert_eq!(paid.len(), 2, "both are paid once covered: {paid:?}");
    assert_eq!(
        built(runner.world()),
        vec![(column(high), 0), (column(low), 0)],
        "order 2 before order 7"
    );
}

/// The branch the test above cannot reach in S1, where a playbook names only
/// the Generator: with targets at **different** prices, the mandate asks for
/// the highest-order target the treasury covers, passing over a higher-order
/// one it does not; and when it covers none, for the highest-order one, which
/// the Quartermaster holds. The targets are written straight into the table,
/// as a capability catalogue (S4) would let a playbook write them.
#[test]
fn an_affordable_target_is_asked_for_before_a_higher_order_one_that_is_not() {
    let mut fixture = world();
    let beacon = core(&fixture);
    let [x, y, z] = core_voxel(&fixture);
    let voxel = |v: i32| Fx::from_voxels(i16::try_from(v).expect("a voxel coordinate"));
    let dear = StructureKind::Mortar;
    let cheap = StructureKind::SurveyPost;
    let (dear_cost, cheap_cost) = (fixture.structure_cost(dear), fixture.structure_cost(cheap));
    assert!(
        cheap_cost.raw() < dear_cost.raw(),
        "the committed rules price a Survey post below a mortar"
    );
    // List order is the build order here: the dear target first.
    assert!(fixture.add_target(beacon, dear.id(), [voxel(x + 3), voxel(y), voxel(z)]));
    assert!(fixture.add_target(beacon, cheap.id(), [voxel(x - 3), voxel(y), voxel(z)]));
    let asked = |world: &World| -> StructureKind {
        let request = mandate_for(MandateKind::Build)
            .expect("Build runs")
            .request(world, core(world))
            .expect("an unpaid target is asked for");
        let row = match request.buys {
            Purchase::Structure(row) => usize::try_from(row).expect("a row"),
            Purchase::Unit(kind) => panic!("a structure, not a {kind:?}"),
        };
        world
            .targets()
            .blueprints()
            .get(row)
            .copied()
            .and_then(StructureKind::from_id)
            .expect("a blueprint")
    };
    fixture.set_treasury(SEAT, cheap_cost);
    assert_eq!(
        asked(&fixture),
        cheap,
        "the cheap target is the one covered"
    );
    fixture.set_treasury(SEAT, Money::new(cheap_cost.raw() - 1));
    assert_eq!(asked(&fixture), dear, "none is covered: the highest order");
    fixture.set_treasury(SEAT, dear_cost);
    assert_eq!(asked(&fixture), dear, "both are covered: the highest order");
}

/// The compiled rows of a one-step route.
fn rows_of(route: &str) -> (Vec<Row>, Vec<i32>, Plan) {
    let plan = Plan::compile(&playbook(route), &rules()).expect("compiles");
    let (rows, durations) = match plan.route().first().map(|step| &step.action) {
        Some(
            Action::Interface {
                rows, durations, ..
            }
            | Action::PlaceBeacon {
                rows, durations, ..
            },
        ) => (rows.clone(), durations.clone()),
        other => panic!("a visit or a deploy: {other:?}"),
    };
    (rows, durations, plan)
}

/// The rate card the committed table sets.
fn times() -> gp::v1::rules_table::InterfaceTimes {
    rules()
        .message()
        .interface_times
        .expect("the committed table prices visits")
}

/// Decision 15, brought to the sim (the register's S1-51; decisions-log item
/// 133 (3) (b)): a Build target is its own row at `build_target_ms`, outside
/// the edit's cap, and each element of a protected-area or probe-area list is
/// one field of the edit. The figures are the verifier's own test's
/// (`crates/verifier/src/interface.rs`), so the Push and the editor agree.
#[test]
fn a_build_target_is_its_own_row_and_each_area_one_field() {
    let times = times();
    let area = r#"{"min":{"x":1,"y":1,"z":1},"max":{"x":2,"y":2,"z":2}}"#;
    let build = format!(
        r#"{{"targets":[{t},{t},{t}],"protected_areas":[{area},{area}]}}"#,
        t = target([1, 1, 1], 0, 0),
    );
    let (rows, durations, _) = rows_of(&visit_writing(&build));
    match rows.first() {
        Some(Row::Settings {
            fields, targets, ..
        }) => {
            assert_eq!(*fields, 2, "two areas, two fields; targets are not fields");
            assert_eq!(targets.len(), 3);
        }
        other => panic!("a settings row: {other:?}"),
    }
    assert_eq!(
        durations,
        vec![
            times.edit_settings_base_ms
                + times.edit_settings_per_field_ms
                + 3 * times.build_target_ms
        ],
        "2 s + 0.5 s for the two areas, then 3 x 2.5 s for the targets"
    );

    let probes = |count: usize| {
        let areas = vec![area; count].join(",");
        format!(
            r#"{{"label":"look","interface":{{"beacon":{{"beacon_id":"b_00"}},"rows":[{{"set_mandate_settings":{{"survey":{{"probe_areas":[{areas}],"scout_count":2}}}}}}]}},"timeout_ms":120000}}"#
        )
    };
    let (_, three, _) = rows_of(&probes(3));
    assert_eq!(
        three,
        vec![times.edit_settings_base_ms + 3 * times.edit_settings_per_field_ms],
        "three areas and a count are four fields"
    );
    let (_, twenty, _) = rows_of(&probes(20));
    assert_eq!(
        twenty,
        vec![times.edit_settings_max_ms],
        "and the cap still bounds the edit"
    );
}

/// Decisions-log item 133 (5): once a target list is not a settings field, a
/// `set_mandate` whose Build arm holds only targets must still carry them --
/// or it would bind nothing the gateway's `resolve_playbook` lists, and that
/// count guard refuses the playbook as `INTERNAL`. The same holds for a
/// `place_beacon`'s initial settings.
#[test]
fn a_switch_holding_only_build_targets_carries_them_and_binds_their_anchors() {
    let times = times();
    let on = r#"{"blueprint_id":"generator","anchor":{"on":{"vent":{"rank":"NEAREST"}}}}"#;
    let switch = format!(
        r#"{{"label":"switch","interface":{{"beacon":{{"beacon_id":"b_00"}},"rows":[{{"set_mandate":{{"build":{{"targets":[{on}]}}}}}}]}},"timeout_ms":120000}}"#
    );
    let (rows, durations, plan) = rows_of(&switch);
    let first = rows.first().expect("a row");
    match first {
        Row::Mandate {
            kind: MandateKind::Build,
            settings: Some(settings),
        } => match settings.as_ref() {
            Row::Settings {
                fields, targets, ..
            } => {
                assert_eq!(*fields, 0, "a target is not a field");
                assert_eq!(targets.len(), 1, "but it is carried");
            }
            other => panic!("carried settings: {other:?}"),
        },
        other => panic!("a switch carrying its targets: {other:?}"),
    }
    assert_eq!(first.binding_slots(), 1, "its `on` anchor is bound");
    assert_eq!(plan.max_bindings(), 1);
    assert_eq!(
        durations,
        vec![times.switch_mandate_ms + times.build_target_ms],
        "the switch, then the target at its own rate"
    );

    let [x, y, z] = core_voxel(&world());
    let place = format!(
        r#"{{"label":"place","place_beacon":{{"at":{{"voxel":{{"x":{},"y":{y},"z":{z}}}}},"initial":{{"mandate":{{"build":{{"targets":[{on}]}}}}}}}},"timeout_ms":120000}}"#,
        x + 3
    );
    let (rows, durations, _) = rows_of(&place);
    assert_eq!(rows.len(), 1, "the initial targets are filed as a row");
    assert_eq!(crate_binding_slots(&rows), 1);
    assert_eq!(durations, vec![times.build_target_ms]);
}

fn crate_binding_slots(rows: &[Row]) -> usize {
    pharmakos_sim::interpreter::binding_slots(rows)
}

/// What the sim refuses at the seal rather than reading another way: a
/// rotation past three quarter turns (never wrapped), a scout count past what
/// a beacon holds (never clamped), and a rules table with a negative
/// interface time (never read as free).
#[test]
fn a_rotation_a_scout_count_and_a_rate_out_of_range_are_refused_at_the_seal() {
    let build = format!(r#"{{"targets":[{}]}}"#, target([1, 1, 1], 0, 4));
    assert_eq!(
        Plan::compile(&playbook(&visit_writing(&build)), &rules()),
        Err(PlanError::RotationOutOfRange { found: 4 })
    );
    let scouts = r#"{"label":"look","interface":{"beacon":{"beacon_id":"b_00"},"rows":[{"set_mandate_settings":{"survey":{"scout_count":256}}}]},"timeout_ms":120000}"#;
    assert_eq!(
        Plan::compile(&playbook(scouts), &rules()),
        Err(PlanError::ScoutsOutOfRange {
            found: 256,
            most: u8::MAX
        })
    );
    let mut message = rules().message().clone();
    if let Some(times) = message.interface_times.as_mut() {
        times.build_target_ms = -1;
    }
    let negative = RulesTable::from_message(&message).expect("the table loads");
    assert_eq!(
        Plan::compile(&playbook(scouts), &negative),
        Err(PlanError::NegativeRate("interface_times.build_target_ms"))
    );
    message.interface_times = None;
    let gapped = RulesTable::from_message(&message).expect("the table loads");
    assert_eq!(
        Plan::compile(&playbook(scouts), &gapped),
        Err(PlanError::MissingBlock("rules.interface_times"))
    );
}

/// `structures.build_hp_per_second` is read from its row (the register's
/// S1-24) and refused at load unless it is whole hit points a tick.
#[test]
fn the_build_rate_is_the_rows_and_a_rate_with_a_remainder_is_refused() {
    let rules = rules();
    let per_second = rules
        .message()
        .structures
        .as_ref()
        .expect("a structures block")
        .build_hp_per_second;
    assert_eq!(
        u32::try_from(rules.build_hp_per_tick().raw()).ok(),
        per_second.checked_div(pharmakos_sim::math::quantity::TICK_HZ)
    );
    for refused in [0, 61] {
        let mut message = rules.message().clone();
        if let Some(block) = message.structures.as_mut() {
            block.build_hp_per_second = refused;
        }
        assert!(
            matches!(
                RulesTable::from_message(&message),
                Err(pharmakos_sim::RulesError::OutOfRange { ref field, .. })
                    if field == "structures.build_hp_per_second"
            ),
            "{refused} hit points a second"
        );
    }
}
