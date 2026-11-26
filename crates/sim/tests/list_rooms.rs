// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The list tables' rooms are each seat's own, and each holds a whole size
//! budget (S3's `tune3`; S3's plan, E17).
//!
//! A beacon's lists (Build targets, protected areas and Survey probe areas in
//! the list-settings table; Attack avoid areas in the Attack table) are kept
//! from round to round, so the verifier's size budget bounds one Push's
//! commits and never the tables: each table has a room, and a commit past it
//! writes nothing and fails its step `targets_unwritten`. Before `tune3` the
//! rooms were the world's: one seat's lists could fill a table and fail
//! another seat's legal step for a reason that seat could not see, the leak
//! per-seat room exists to close (`STRUCTURE_TABLE_ROOM`'s doc), and the
//! list-settings room (eight rows a beacon row) was smaller than the 128
//! rows one in-budget playbook may write on one beacon.
//!
//! The rules these tests hold:
//!
//! * a seat's room is at least the committed rules table's size budget at
//!   every seat count the host seats, so a playbook that spends its whole
//!   budget on one beacon's list fits a seat that holds none;
//! * a commit is tested against its own seat's room, and another seat's rows
//!   count for nothing;
//! * a restore refuses a save in which a seat holds more than its room.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules — not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use pharmakos_sim::events::EventKind;
use pharmakos_sim::interpreter::{MAX_HANDLERS, MAX_ROUTE_STEPS, Plan, StepFailed, StepFailure};
use pharmakos_sim::runner::{DEFAULT_ROUND_LIMIT, Runner};
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::snapshot::{Snapshot, SnapshotError};
use pharmakos_sim::tables::{BeaconId, ColumnBox, PRIORITY_NORMAL, SeatId};
use pharmakos_sim::world::{seat_avoid_room_of, seat_list_room_of};
use pharmakos_sim::{MatchSettings, RulesTable, World, WorldConfig};

/// The seats the host seats at most (AGENTS.md section 1: up to three).
const MOST_SEATS: u32 = 3;

fn rules_path() -> std::path::PathBuf {
    std::path::Path::new("..")
        .join("..")
        .join("rules")
        .join("rules.v1.json")
}

fn rules() -> RulesTable {
    RulesTable::load(&rules_path()).expect("the committed rules table loads")
}

/// `verifier.size_budget_units` of the committed rules table, read from the
/// file: the most list rows one playbook writes.
fn size_budget() -> u32 {
    let text = std::fs::read_to_string(rules_path()).expect("the rules file");
    let table: pharmakos_proto::gp::v1::RulesTable =
        pharmakos_proto::json::decode(&text).expect("the rules table decodes");
    table
        .verifier
        .expect("the verifier's rows")
        .size_budget_units
}

/// A two-seat world, each seat holding its core.
fn world() -> World {
    World::new(&WorldConfig {
        match_seed: pharmakos_sim::DETERMINISM_MATCH_SEED,
        seats: 2,
        rules: rules(),
        match_settings: MatchSettings {
            segment_lengths_ms: vec![900_000],
            round_limit: DEFAULT_ROUND_LIMIT,
        },
    })
    .expect("the rules table describes a two-seat map")
}

/// `seat`'s core and its column.
fn core_of(world: &World, seat: u8) -> (BeaconId, [i32; 2]) {
    let row = world
        .beacons()
        .seats()
        .iter()
        .position(|owner| *owner == seat)
        .expect("the seat's core");
    let id = BeaconId::new(*world.beacons().ids().get(row).expect("the core's id"));
    let column = pharmakos_sim::targeting::column_of(
        *world
            .beacons()
            .positions()
            .get(row)
            .expect("the core's position"),
    );
    (id, column)
}

/// Write `count` Build target rows on `beacon` directly, each on a voxel of
/// its own beside `origin`.
fn fill_targets(world: &mut World, beacon: BeaconId, origin: [i32; 2], count: u32) {
    for index in 0..count {
        let dx = i32::try_from(index % 32).expect("a column");
        let dy = i32::try_from(index.div_euclid(32)).expect("a column");
        let at = world.standing_point([origin[0] + dx, origin[1] + dy, 0]);
        world
            .add_target(
                beacon,
                pharmakos_sim::tables::StructureKind::Generator.id(),
                at,
            )
            .expect("the table has room for a fixture row");
    }
}

/// How many list-settings rows `seat`'s beacons hold.
fn rows_of(world: &World, seat: u8) -> usize {
    world
        .targets()
        .beacons()
        .iter()
        .filter(|beacon| {
            world
                .beacons()
                .row_of(**beacon)
                .ok()
                .and_then(|row| world.beacons().seats().get(row.index()).copied())
                == Some(seat)
        })
        .count()
}

/// A `protected_areas` settings row of `count` one-column areas beside
/// `origin`.
fn areas_row(origin: [i32; 2], count: u32) -> String {
    let areas: Vec<String> = (0..count)
        .map(|index| {
            let x = origin[0] + i32::try_from(index % 32).expect("a column");
            let y = origin[1] + i32::try_from(index.div_euclid(32)).expect("a column");
            format!(
                "{{\"min\":{{\"x\":{x},\"y\":{y},\"z\":0}},\"max\":{{\"x\":{x},\"y\":{y},\"z\":0}}}}"
            )
        })
        .collect();
    format!(
        "{{\"set_mandate_settings\":{{\"build\":{{\"protected_areas\":[{}]}}}}}}",
        areas.join(",")
    )
}

/// Seat 0's playbook visiting its second beacon, `b_01`, with `row`, sealed
/// on `world` and played until the row commits or its step fails. Answers
/// the failures seat 0 met and the runner.
fn visit(mut world: World, row: &str) -> (Vec<StepFailure>, Runner) {
    let rules = rules();
    let (_, [cx, cy]) = core_of(&world, 0);
    let at = world.standing_point([cx + 6, cy + 6, 0]);
    world
        .place_beacon_directly(SeatId::new(0), at, MandateKind::Build, PRIORITY_NORMAL)
        .expect("a Build beacon");
    let text = format!(
        concat!(
            "{{\"schema_version\":{{\"major\":1}},",
            "\"meta\":{{\"title\":\"lists\",\"author_kind\":\"HUMAN\"}},",
            "\"declarative\":{{\"route\":[",
            "{{\"label\":\"write\",\"interface\":{{\"beacon\":{{\"beacon_id\":\"b_01\"}},",
            "\"rows\":[{row}]}},\"timeout_ms\":800000,\"on_fail\":{{\"action\":\"SKIP\"}}}},",
            "{{\"label\":\"stand\",\"hold\":{{\"ms\":60000}}}}]}},",
            "\"on_death\":{{\"on_respawn\":\"CONTINUE\",\"max_deaths_before_fallback\":3}},",
            "\"fallback\":{{\"hold\":{{\"at\":{{\"safest\":{{}}}}}}}},",
            "\"kind\":\"PLAYBOOK\"}}"
        ),
        row = row,
    );
    let playbook: pharmakos_proto::gp::v1::Playbook =
        pharmakos_proto::json::decode(&text).expect("the playbook decodes");
    let plan = Plan::compile(&playbook, &rules).expect("it compiles");
    let mut runner = Runner::new(world).unwrap_or_else(|error| panic!("{error}"));
    runner
        .seal_playbook(SeatId::new(0), plan)
        .expect("a Lull takes a seal");
    assert!(runner.begin_push().unwrap(), "a match opens in a Lull");
    let mut failed = Vec::new();
    let mut committed = false;
    for _ in 0..17_000 {
        runner
            .step()
            .unwrap()
            .expect("the segment outlasts this test");
        for event in runner.events() {
            if event.seat != Some(SeatId::new(0)) {
                continue;
            }
            if event.kind == EventKind::StepFailed {
                failed.push(
                    StepFailed::decode(event.value)
                        .expect("a step_failed value decodes")
                        .reason,
                );
            } else if event.kind == EventKind::RowCommitted {
                committed = true;
            }
        }
        runner.clear_events();
        if committed || !failed.is_empty() {
            break;
        }
    }
    assert!(
        committed || !failed.is_empty(),
        "the visit neither committed nor failed inside the test's ticks"
    );
    (failed, runner)
}

/// **Every room holds a whole size budget**, at every seat count the host
/// seats: a seat's list-settings room and its avoid-area room are each at
/// least `verifier.size_budget_units` of the committed rules table, and so is
/// the interpreter's ceiling on route steps; its ceiling on handlers holds a
/// budget of handlers that each carry a body step. Raising the budget past a
/// room fails here, in the pull request that raises it.
#[test]
fn every_room_holds_a_whole_size_budget_at_every_seat_count() {
    let budget = size_budget();
    assert!(budget > 0, "the committed budget is a number");
    for seats in 1..=MOST_SEATS {
        assert!(
            seat_list_room_of(seats) >= budget,
            "{seats} seats: a seat's list room is {}, below the size budget {budget}",
            seat_list_room_of(seats)
        );
        assert!(
            seat_avoid_room_of(seats) >= budget,
            "{seats} seats: a seat's avoid-area room is {}, below the size budget {budget}",
            seat_avoid_room_of(seats)
        );
    }
    assert!(
        MAX_ROUTE_STEPS >= usize::try_from(budget).expect("a count"),
        "the interpreter's route ceiling {MAX_ROUTE_STEPS} is below the size budget {budget}"
    );
    // A handler that does anything costs two units: itself and a body step.
    assert!(
        MAX_HANDLERS.saturating_mul(2) >= usize::try_from(budget).expect("a count"),
        "a size budget of {budget} holds more working handlers than the ceiling {MAX_HANDLERS}"
    );
}

/// **A whole budget of list rows on one beacon commits, though the other
/// seat holds its whole room**: seat 1's core holds every row its seat's room
/// takes; seat 0's playbook writes a size budget of protected areas on its
/// `b_01`, and the row commits whole.
#[test]
fn a_budget_of_areas_on_one_beacon_commits_beside_another_seats_full_room() {
    let budget = size_budget();
    let mut world = world();
    let (other, at) = core_of(&world, 1);
    fill_targets(&mut world, other, at, seat_list_room_of(2));
    let (_, [cx, cy]) = core_of(&world, 0);
    let (failed, runner) = visit(world, &areas_row([cx - 16, cy + 10], budget));
    assert_eq!(failed, [], "the row commits");
    assert_eq!(
        rows_of(runner.world(), 0),
        usize::try_from(budget).expect("a count"),
        "seat 0's beacon holds the whole list"
    );
    assert_eq!(
        rows_of(runner.world(), 1),
        usize::try_from(seat_list_room_of(2)).expect("a count"),
        "seat 1's rows are untouched"
    );
}

/// **A seat's own room is the limit**: with seat 0's core holding all but
/// two rows of its room, a row writing two areas on `b_01` commits and one
/// writing three writes nothing and fails `targets_unwritten`, while seat 1
/// holds no row at all.
#[test]
fn a_commit_past_its_own_seats_room_writes_nothing_and_one_at_it_commits() {
    let room = seat_list_room_of(2);
    for (written, fits) in [(2_u32, true), (3, false)] {
        let mut world = world();
        let (core, [cx, cy]) = core_of(&world, 0);
        fill_targets(&mut world, core, [cx - 16, cy + 20], room - 2);
        let (failed, runner) = visit(world, &areas_row([cx - 16, cy + 10], written));
        let held = rows_of(runner.world(), 0);
        if fits {
            assert_eq!(failed, [], "two areas fill the room exactly");
            assert_eq!(held, usize::try_from(room).expect("a count"));
        } else {
            assert_eq!(failed, [StepFailure::TargetsUnwritten]);
            assert_eq!(
                held,
                usize::try_from(room - 2).expect("a count"),
                "the refused row wrote nothing"
            );
        }
        assert_eq!(rows_of(runner.world(), 1), 0);
    }
}

/// A runner on `world`, and the save of it.
fn saved(world: World) -> (Runner, Snapshot) {
    let runner = Runner::new(world).unwrap_or_else(|error| panic!("{error}"));
    let bytes = runner
        .capture()
        .expect("a world between ticks")
        .to_bytes()
        .expect("the save encodes");
    let save = Snapshot::from_bytes(&bytes).expect("the save decodes");
    (runner, save)
}

/// **A restore refuses a save in which a seat holds more list rows than its
/// room**, and takes one that holds the room exactly: no run writes the
/// first, since every commit is tested against the seat's room.
#[test]
fn a_restore_refuses_a_seat_past_its_list_room() {
    let room = seat_list_room_of(2);
    for (rows, over) in [(room, false), (room + 1, true)] {
        let mut world = world();
        let (core, at) = core_of(&world, 0);
        fill_targets(&mut world, core, at, rows);
        let (_, save) = saved(world);
        let mut receiver = Runner::new(self::world()).unwrap_or_else(|error| panic!("{error}"));
        let answer = receiver.restore(&save);
        if over {
            assert_eq!(
                answer,
                Err(SnapshotError::OverListRoom {
                    seat: 0,
                    rows,
                    room
                })
            );
        } else {
            assert_eq!(answer, Ok(()), "a seat at its room restores");
        }
    }
}

/// **A restore refuses a save in which a seat holds more avoid areas than
/// its room**, and takes one that holds the room exactly.
#[test]
fn a_restore_refuses_a_seat_past_its_avoid_room() {
    let room = seat_avoid_room_of(2);
    for (areas, over) in [(room, false), (room + 1, true)] {
        let mut world = world();
        let (core, [cx, cy]) = core_of(&world, 0);
        world.set_writ(core, MandateKind::Attack);
        let boxes: Vec<ColumnBox> = (0..areas)
            .map(|index| {
                let x = i16::try_from(cx).expect("a column")
                    + i16::try_from(index % 32).expect("a column");
                let y = i16::try_from(cy).expect("a column")
                    + i16::try_from(index.div_euclid(32)).expect("a column");
                ColumnBox::new([x, y], [x, y]).expect("a box")
            })
            .collect();
        assert!(world.set_attack(
            core,
            pharmakos_sim::attack::AttackSettings::default(),
            &boxes
        ));
        let (_, save) = saved(world);
        let mut receiver = Runner::new(self::world()).unwrap_or_else(|error| panic!("{error}"));
        let answer = receiver.restore(&save);
        if over {
            assert_eq!(
                answer,
                Err(SnapshotError::OverAvoidRoom {
                    seat: 0,
                    areas,
                    room
                })
            );
        } else {
            assert_eq!(answer, Ok(()), "a seat at its room restores");
        }
    }
}
