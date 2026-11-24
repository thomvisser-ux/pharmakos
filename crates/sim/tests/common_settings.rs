// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Common row's `retreat_hp_pct` carries presence (decisions-log item
//! 177 (3)): a written 0 turns a beacon's retreat off, is one field of the
//! edit and is priced as one, and a row that leaves the field out leaves the
//! beacon's threshold as it is.
//!
//! Before the field was `optional`, a written 0 and an omitted field were one
//! value: a visit writing `"retreat_hp_pct": 0` was charged nothing for it and
//! changed nothing, so a player could not turn a retreat off.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules, not an \
              integration test's helpers. A panic is this file's failure report."
)]

use std::path::PathBuf;

use pharmakos_proto::gp;
use pharmakos_proto::json;
use pharmakos_sim::events::EventKind;
use pharmakos_sim::interpreter::{Action, Plan, PlanError, Row};
use pharmakos_sim::runner::{MatchSettings, Runner};
use pharmakos_sim::tables::{BeaconId, Common, SeatId};
use pharmakos_sim::{RulesTable, World, WorldConfig};

/// The seed `tests/build.rs`, `tests/economy.rs` and `tests/mining.rs` play on.
const SEED: u64 = 0x00A1_B2C3_D4E5_F607;

const SEAT: SeatId = SeatId::new(0);

/// `MandateSettings.Roe`'s `HOLD_FIRE`.
const HOLD_FIRE: u8 = 1;

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
        seats: 3,
        rules: rules(),
        match_settings: MatchSettings {
            segment_lengths_ms: vec![180_000],
            round_limit: 1,
        },
    })
    .expect("the rules table describes a map")
}

/// Seat 0's core, its `b_00`.
fn core(world: &World) -> BeaconId {
    let row = world
        .beacons()
        .row_of_ordinal(SEAT, 0)
        .expect("seat 0 has a core");
    BeaconId::new(u32::try_from(row).expect("a beacon row"))
}

fn common_of(world: &World, beacon: BeaconId) -> Common {
    let row = usize::try_from(beacon.raw()).expect("a row");
    world
        .beacons()
        .commons()
        .get(row)
        .copied()
        .expect("the beacon has Common settings")
}

/// A whole playbook around one route step, as canonical `gp.v1` JSON.
fn playbook(route: &str) -> gp::v1::Playbook {
    let text = format!(
        concat!(
            "{{\"schema_version\":{{\"major\":1}},",
            "\"meta\":{{\"title\":\"common\",\"author_kind\":\"HUMAN\"}},",
            "\"declarative\":{{\"route\":[{route}]}},",
            "\"on_death\":{{\"on_respawn\":\"CONTINUE\",\"max_deaths_before_fallback\":3}},",
            "\"fallback\":{{\"hold\":{{\"at\":{{\"safest\":{{}}}}}}}},",
            "\"kind\":\"PLAYBOOK\"}}"
        ),
        route = route
    );
    json::decode(&text).unwrap_or_else(|error| panic!("the case decodes: {error}\n{text}"))
}

/// A visit to `b_00` with the given rows.
fn visit_with(rows: &str) -> String {
    format!(
        r#"{{"label":"visit","interface":{{"beacon":{{"beacon_id":"b_00"}},"rows":[{rows}]}},"timeout_ms":120000}}"#
    )
}

/// Seat 0's core starting at `start`, then `route` sealed and played until
/// its visit ends; the world after.
fn visit(start: Common, route: &str) -> World {
    let mut fixture = world();
    let home = core(&fixture);
    fixture.set_common(home, start);
    let plan = Plan::compile(&playbook(route), &rules()).expect("compiles");
    let mut runner = Runner::new(fixture).unwrap_or_else(|error| panic!("{error}"));
    runner
        .seal_playbook(SEAT, plan)
        .expect("a Lull takes a seal");
    assert!(runner.begin_push().unwrap());
    let mut ended = false;
    let mut ticks: u32 = 0;
    while !ended && ticks < 3_000 {
        runner.step().unwrap();
        ended = runner
            .events()
            .iter()
            .any(|event| event.kind == EventKind::VisitEnded && event.seat == Some(SEAT));
        runner.clear_events();
        ticks += 1;
    }
    assert!(ended, "the visit ends");
    runner.into_world()
}

/// The rows of the first route step, an interface step.
fn rows_of(compiled: &Plan) -> &[Row] {
    match compiled.route().first().map(|step| &step.action) {
        Some(Action::Interface { rows, .. }) => rows,
        other => panic!("an interface step: {other:?}"),
    }
}

const RETREATING: Common = Common {
    roe: 0,
    retreat_hp_pct: 50,
};

#[test]
fn a_written_zero_turns_a_beacons_retreat_off() {
    let after = visit(
        RETREATING,
        &visit_with(r#"{"set_mandate_settings":{"retreat_hp_pct":0}}"#),
    );
    assert_eq!(
        common_of(&after, core(&after)),
        Common {
            roe: 0,
            retreat_hp_pct: 0,
        },
        "a written 0 sets the threshold to 0, which never retreats"
    );
}

#[test]
fn a_row_that_leaves_the_threshold_out_leaves_it_alone() {
    let after = visit(
        RETREATING,
        &visit_with(r#"{"set_mandate_settings":{"roe":"HOLD_FIRE"}}"#),
    );
    assert_eq!(
        common_of(&after, core(&after)),
        Common {
            roe: HOLD_FIRE,
            retreat_hp_pct: 50,
        },
        "an omitted threshold is not a written 0"
    );
}

#[test]
fn a_written_threshold_still_sets_it() {
    let after = visit(
        RETREATING,
        &visit_with(r#"{"set_mandate_settings":{"retreat_hp_pct":30}}"#),
    );
    assert_eq!(common_of(&after, core(&after)).retreat_hp_pct, 30);
}

#[test]
fn a_written_zero_is_one_field_of_its_edit() {
    // Spec section 5's "2 s + 0.5 s per extra field": a written 0 is a field,
    // as the verifier's `count` and the operator's price read it, so the visit
    // stands in the Push for the time the editor quotes.
    let route = visit_with(concat!(
        r#"{"set_mandate_settings":{"retreat_hp_pct":0}},"#,
        r#"{"set_mandate_settings":{"retreat_hp_pct":0,"roe":"HOLD_FIRE"}},"#,
        r#"{"set_mandate":{"defend":{},"retreat_hp_pct":0}},"#,
        r#"{"set_mandate":{"defend":{}}}"#
    ));
    let compiled = Plan::compile(&playbook(&route), &rules()).expect("compiles");
    let rows = rows_of(&compiled);
    let times = rules().message().interface_times.expect("interface times");
    let card = compiled.interface_times();
    let priced: Vec<Result<i32, PlanError>> =
        rows.iter().map(|row| row.duration_ms(card)).collect();
    assert_eq!(
        priced,
        vec![
            Ok(times.edit_settings_base_ms),
            Ok(times.edit_settings_base_ms + times.edit_settings_per_field_ms),
            Ok(times.switch_mandate_ms + times.edit_settings_base_ms),
            Ok(times.switch_mandate_ms),
        ],
        "one field, two fields, the switch and one field, the switch alone"
    );
    assert!(
        rows.first().expect("a row").writes(),
        "a row writing only a 0 writes something"
    );
}

#[test]
fn a_written_zero_in_a_new_beacons_settings_is_filed() {
    // A `place_beacon` files its initial settings only when they write
    // something; a written 0 does, and costs its field.
    let route = r#"{"label":"deploy","place_beacon":{"at":{"voxel":{"x":27,"y":356,"z":29}},"initial":{"mandate":{"build":{},"retreat_hp_pct":0}}},"timeout_ms":30000,"on_fail":{"action":"SKIP"}}"#;
    let compiled = Plan::compile(&playbook(route), &rules()).expect("compiles");
    let rows = match compiled.route().first().map(|step| &step.action) {
        Some(Action::PlaceBeacon { rows, .. }) => rows,
        other => panic!("a deploy: {other:?}"),
    };
    let times = rules().message().interface_times.expect("interface times");
    let card = compiled.interface_times();
    assert_eq!(rows.len(), 1, "the settings row is filed");
    assert_eq!(
        rows.first().expect("a row").duration_ms(card),
        Ok(times.edit_settings_base_ms)
    );
}

#[test]
fn a_written_threshold_past_100_is_still_refused() {
    let route = visit_with(r#"{"set_mandate_settings":{"retreat_hp_pct":101}}"#);
    assert_eq!(
        Plan::compile(&playbook(&route), &rules()).map(|_| ()),
        Err(PlanError::RetreatOutOfRange { found: 101 })
    );
}
