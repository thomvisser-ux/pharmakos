// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A mandate switch that carries settings commits whole or not at all (lane
//! `simroom`, S3 of its review): every way its settings can fail is tested
//! before the switch writes anything, a protect entry or an Attack target
//! that resolves to nothing included. A failed switch leaves the beacon's old
//! writ exactly as it was; and two lists committed in a row replace each
//! other exactly.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules — not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use pharmakos_sim::events::EventKind;
use pharmakos_sim::interpreter::{Plan, StepFailed, StepFailure};
use pharmakos_sim::runner::{DEFAULT_ROUND_LIMIT, Runner};
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::tables::{PRIORITY_NORMAL, SeatId};
use pharmakos_sim::{MatchSettings, RulesTable, World, WorldConfig};

fn rules() -> RulesTable {
    RulesTable::load(
        &std::path::Path::new("..")
            .join("..")
            .join("rules")
            .join("rules.v1.json"),
    )
    .expect("the committed rules table loads")
}

/// A two-seat match whose seat 0 holds a Build beacon, `b_01`, beside its
/// core; and seat 0's playbook visiting it with one row, `row`. Answers the
/// step failures seat 0 met and `b_01`'s writ once the visit is over.
fn switch(row: &str) -> (Vec<StepFailure>, u8) {
    let (failed, runner, beacon) = visit(row, 1, false);
    let row = runner
        .world()
        .beacons()
        .row_of(beacon)
        .expect("b_01 stands")
        .index();
    let writ = *runner
        .world()
        .beacons()
        .mandates()
        .get(row)
        .expect("b_01's writ");
    (failed, writ)
}

/// The match [`switch`] plays, with `rows` as the visit's rows: played
/// until `commits` rows have committed or a step fails, fresh or (when
/// `restored`) in a runner restored from the save of the Push's first
/// tick. Answers the step failures seat 0 met, the runner and `b_01`.
fn visit(
    rows: &str,
    commits: u32,
    restored: bool,
) -> (Vec<StepFailure>, Runner, pharmakos_sim::tables::BeaconId) {
    let (mut runner, beacon) = opened(rows);
    if restored {
        let bytes = runner
            .capture()
            .expect("a world between ticks")
            .to_bytes()
            .expect("the save encodes");
        let save = pharmakos_sim::snapshot::Snapshot::from_bytes(&bytes).expect("the save decodes");
        let (fresh, _) = opened(rows);
        runner = fresh;
        runner.restore(&save).expect("the save restores");
    }
    let mut failed = Vec::new();
    let mut committed: u32 = 0;
    for _ in 0..1_200 {
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
                committed += 1;
            }
        }
        runner.clear_events();
        if !failed.is_empty() || committed >= commits {
            break;
        }
    }
    (failed, runner, beacon)
}

/// The match, in its Push, before any tick: seat 0's playbook sealed.
fn opened(row: &str) -> (Runner, pharmakos_sim::tables::BeaconId) {
    let rules = rules();
    let mut world = World::new(&WorldConfig {
        match_seed: pharmakos_sim::DETERMINISM_MATCH_SEED,
        seats: 2,
        rules: rules.clone(),
        match_settings: MatchSettings {
            segment_lengths_ms: vec![600_000],
            round_limit: DEFAULT_ROUND_LIMIT,
        },
    })
    .expect("the rules table describes a two-seat map");
    let core_row = world
        .beacons()
        .seats()
        .iter()
        .position(|owner| *owner == 0)
        .expect("seat 0's core");
    let [cx, cy] = pharmakos_sim::targeting::column_of(
        *world
            .beacons()
            .positions()
            .get(core_row)
            .expect("the core's position"),
    );
    let at = world.standing_point([cx + 6, cy + 6, 0]);
    let beacon = world
        .place_beacon_directly(SeatId::new(0), at, MandateKind::Build, PRIORITY_NORMAL)
        .expect("a Build beacon");
    let text = format!(
        concat!(
            "{{\"schema_version\":{{\"major\":1}},",
            "\"meta\":{{\"title\":\"switch\",\"author_kind\":\"HUMAN\"}},",
            "\"declarative\":{{\"route\":[",
            "{{\"label\":\"switch\",\"interface\":{{\"beacon\":{{\"beacon_id\":\"b_01\"}},",
            "\"rows\":[{row}]}},\"timeout_ms\":60000,\"on_fail\":{{\"action\":\"SKIP\"}}}},",
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
    (runner, beacon)
}

/// A switch to Defend whose protect list names a beacon the seat does not
/// hold fails `no_target`, and `b_01` keeps the Build writ.
#[test]
fn a_switch_whose_protect_entry_resolves_to_nothing_keeps_the_old_writ() {
    let (failed, writ) = switch(
        "{\"set_mandate\":{\"defend\":{\"protect\":[{\"beacon\":{\"beacon_id\":\"b_07\"}}]}}}",
    );
    assert_eq!(failed, [StepFailure::NoTarget]);
    assert_eq!(
        writ,
        MandateKind::Build.id(),
        "the failed switch wrote nothing: the beacon keeps its old writ"
    );
}

/// A switch to Attack whose target is an enemy handle the seal could not pin
/// fails `no_target`, and `b_01` keeps the Build writ.
#[test]
fn a_switch_whose_attack_target_is_unpinned_keeps_the_old_writ() {
    let (failed, writ) = switch(concat!(
        "{\"set_mandate\":{\"attack\":{\"target\":{\"beacon\":{\"beacon_id\":\"e_09\"}},",
        "\"retarget\":\"NONE\",\"launch\":{\"segment_ms\":600000},",
        "\"after_action\":\"RETURN_HOME\",\"waves\":1}}}"
    ));
    assert_eq!(failed, [StepFailure::NoTarget]);
    assert_eq!(
        writ,
        MandateKind::Build.id(),
        "the failed switch wrote nothing: the beacon keeps its old writ"
    );
}

/// **Two protect lists committed in a row on one beacon: the second
/// replaces the first exactly** (lane `simroom`, fix pass 2), fresh and in a
/// world restored from the Push's first save. Each commit resolves into the
/// world's protect scratch, lent emptied: a scratch left holding the first
/// list would make the second the two lists' union.
#[test]
fn a_second_protect_list_replaces_the_first_exactly() {
    let rows = concat!(
        "{\"set_mandate\":{\"defend\":{\"protect\":[",
        "{\"beacon\":{\"beacon_id\":\"b_00\"}},{\"beacon\":{\"beacon_id\":\"b_01\"}}]}}},",
        "{\"set_mandate_settings\":{\"defend\":{\"protect\":[",
        "{\"beacon\":{\"beacon_id\":\"b_01\"}}]}}}"
    );
    for restored in [false, true] {
        let (failed, runner, beacon) = visit(rows, 2, restored);
        assert_eq!(failed, [], "restored {restored}: both rows commit");
        let protected: Vec<_> = runner.world().defend().protected_by(beacon).collect();
        assert_eq!(
            protected,
            [beacon],
            "restored {restored}: b_01 protects exactly the second list, itself"
        );
    }
}
