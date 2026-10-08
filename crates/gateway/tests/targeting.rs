// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Targeting's surfaces (S1's plan, task `tgtw`; `docs/design/targeting.md`,
//! "Surfaces"): `resolve_refs`, `estimate_route` accepting `covering`,
//! `get_map_summary.features`, the verifier's `Scope` features, the Lull's
//! "this round" sentence and the recap's "why a step found nothing".
//!
//! The acceptance lines this file carries:
//!
//! * `resolve_refs_answers_as_the_sim_would_at_step_start` -- the gateway's
//!   pick equals the sim's on the golden seed: the seat resolves its playbook
//!   in the Lull, the Push runs, and the feature, the site and the `on`
//!   binding the sim bound when the step started are the ones the gateway
//!   named;
//! * `estimate_route_returns_the_covering_site` -- a `covering` waypoint's leg
//!   ends on the column the sim then walks to and deploys on;
//! * `a_hidden_or_foreign_name_answers_no_target` -- an absent feature name, a
//!   `b_NN` the seat does not have, another seat's `e_NN` it has been shown
//!   and one nobody minted all answer the same `no_target`, so a guess
//!   reveals nothing.
//!
//! And `an_on_nearest_reads_the_vent_the_sim_binds` holds the gateway's
//! restated `on` filter to the sim's `on {vent: NEAREST}` binding, as the
//! first acceptance line holds its `covering` filter;
//! `a_set_mandate_rows_on_reads_the_vent_the_sim_binds` holds a switch's
//! carried Build targets to the same, since the `mine` lane made the sim bind
//! them -- the `on` column as well as the vent -- and
//! `set_mandate_rows_are_counted_with_the_rows_beside_them` holds a visit's
//! rows to the sim's bindings pick by pick, in row order (the `econ` lane,
//! from review B's nits on `tgtw`; decisions-log item 133 (5)).
//! `resolve_refs_in_a_push_is_phase_closed` is item 133 (3) (f): a preview is
//! asked outside a Push, as `estimate_route`'s `covering` is.
//!
//! Beside them, two goldens in `tests/golden/gateway/`: seat 0's whole
//! `get_map_summary` answer in the opening Lull (`map_summary`) and its whole
//! `resolve_refs` answer for the cover-and-build playbook (`resolve_refs`).
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use pharmakos_gateway::fog::FogPolicy;
use pharmakos_gateway::rpc;
use pharmakos_gateway::scopes::{Scope, ScopeSet};
use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::token::{Subject, Token};
use pharmakos_proto::json::Json;
use pharmakos_sim::events::EventKind;
use pharmakos_sim::math::quantity::Ms;
use pharmakos_sim::tables::SeatId;

use support::{FIRST_LULL_MS, LULL_MS, MATCH, array_of, call_in_lull, result, text_of};

/// A Push long enough for the covering beacon to land and its Generator to be
/// paid for: on the golden seed the beacon lands at tick 721 and the
/// Generator is queued at tick 766 (`scenarios/s1/cover-nearest-vent`).
const SEGMENT_MS: i32 = 60_000;

/// The playbook `scenarios/s1/cover-nearest-vent` seals: one step, a beacon
/// covering the nearest vent the seat does not cover, with a Generator `on`
/// the vent it covered.
fn cover_and_build() -> String {
    std::fs::read_to_string(
        support::workspace_root()
            .join("scenarios")
            .join("s1")
            .join("cover-nearest-vent.playbook.jsonc"),
    )
    .expect("the committed scenario playbook")
}

/// A two-seat match on the golden seed, in its opening Lull, with seat 0's
/// token.
fn opening() -> (Surface, Token) {
    let mut surface = support::hosted(2, SEGMENT_MS, 3);
    let token = support::seat_token(&mut surface, 0);
    (surface, token)
}

/// `resolve_refs` for `playbook`, as the answer's `refs`.
fn refs(surface: &mut Surface, token: &Token, left: &mut i32, playbook: &str) -> Vec<Json> {
    let answer = call_in_lull(
        surface,
        token,
        left,
        "resolve_refs",
        &format!(r#"{{"playbook_jsonc":{}}}"#, support::quote(playbook)),
    );
    array_of(&result(&answer, "resolve_refs"), "refs")
}

/// One integer field.
fn number(value: &Json, key: &str) -> i64 {
    match value.get(key) {
        Some(Json::Number(lexeme)) => lexeme.parse().expect("an integer"),
        other => panic!("`{key}` is a number, and it is {other:?}"),
    }
}

/// The name of the feature at `index` in the hosted world's table.
fn feature_name(surface: &Surface, index: u32) -> String {
    let host = surface.host().expect("hosted");
    host.world()
        .features()
        .get(usize::try_from(index).expect("fits"))
        .expect("a feature")
        .name()
}

/// Run the Push until the sim has started seat 0's first step, and return
/// the step's state as the sim holds it then.
fn first_step_state(surface: &mut Surface) -> pharmakos_sim::interpreter::PlanState {
    surface.begin_push().expect("the Push begins");
    for _ in 0..40 {
        let started = surface
            .feed()
            .events()
            .iter()
            .any(|event| event.kind.name() == EventKind::StepStarted.name());
        if started {
            break;
        }
        let _ = support::step(surface, 1);
    }
    surface
        .host()
        .expect("hosted")
        .world()
        .interpreter()
        .state(0)
        .cloned()
        .expect("seat 0 has a state")
}

#[test]
fn resolve_refs_answers_as_the_sim_would_at_step_start() {
    let (mut surface, token) = opening();
    let mut left = FIRST_LULL_MS;
    let playbook = cover_and_build();
    let answered = refs(&mut surface, &token, &mut left, &playbook);
    assert_eq!(
        answered.len(),
        2,
        "one `covering` and one `on`: {answered:?}"
    );

    let covering = answered.first().expect("the covering");
    assert_eq!(
        text_of(covering, "pointer"),
        "/declarative/route/0/place_beacon/at/covering"
    );
    assert_eq!(text_of(covering, "failure"), "", "it reads a vent");
    let picked = text_of(covering, "feature_id");
    assert!(picked.starts_with("vent_"), "{picked}");
    assert!(number(covering, "travel_ms") > 0);
    let candidates = array_of(covering, "candidates");
    assert!(
        number(covering, "matched") >= i64::try_from(candidates.len()).expect("fits"),
        "a reachable candidate is a matched one"
    );
    assert_eq!(
        candidates.first().map(|first| text_of(first, "feature_id")),
        Some(picked.clone()),
        "round 1's nearest uncovered vent has a legal site, so the pick is the first candidate"
    );

    let on = answered.get(1).expect("the on");
    assert_eq!(
        text_of(on, "pointer"),
        "/declarative/route/0/place_beacon/initial/mandate/build/targets/0/anchor/on"
    );
    assert_eq!(
        text_of(on, "feature_id"),
        picked,
        "`covered {{}}` names the vent the covering bound"
    );

    // The seat seals it, the Push starts, and the sim reads the same step.
    let sealed = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "submit_plan",
        &format!(r#"{{"playbook_jsonc":{}}}"#, support::quote(&playbook)),
    );
    assert_eq!(
        result(&sealed, "submit_plan").get("accepted"),
        Some(&Json::Bool(true))
    );
    let state = first_step_state(&mut surface);
    assert_eq!(
        feature_name(&surface, state.bound_feature),
        picked,
        "the sim's covering bound the vent the gateway named"
    );
    let binding = state.bindings.first().expect("the step bound its `on`");
    assert_eq!(
        feature_name(&surface, binding.feature),
        picked,
        "and its `on {{covered {{}}}}` the same vent"
    );
}

#[test]
fn estimate_route_returns_the_covering_site() {
    let (mut surface, token) = opening();
    let mut left = FIRST_LULL_MS;
    let [x, y, z] = support::commander_voxel(&surface, 0);
    let answer = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "estimate_route",
        &format!(
            r#"{{"waypoints":[{{"voxel":{{"x":{x},"y":{y},"z":{z}}}}},{{"covering":{{"vent":{{"rank":"NEAREST","coverage":"UNCOVERED"}}}}}}]}}"#
        ),
    );
    let route = result(&answer, "estimate_route");
    assert_eq!(route.get("reachable"), Some(&Json::Bool(true)));
    let legs = array_of(&route, "legs");
    let to = legs
        .first()
        .and_then(|leg| leg.get("to"))
        .and_then(|to| to.get("voxel"))
        .cloned()
        .expect("the leg ends on a voxel");
    let site = [number(&to, "x"), number(&to, "y")];
    assert!(number(legs.first().expect("a leg"), "ms") > 0);

    // The sim walks to that site and deploys on it.
    let playbook = cover_and_build();
    let _ = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "submit_plan",
        &format!(r#"{{"playbook_jsonc":{}}}"#, support::quote(&playbook)),
    );
    let state = first_step_state(&mut surface);
    assert_eq!(
        [i64::from(state.pinned_at[0]), i64::from(state.pinned_at[1])],
        site,
        "the step's site is the leg's end"
    );

    // In a Push the hosted world has moved on from the frozen one the
    // route's other legs read, so a `covering` waypoint is a planning
    // question asked in the wrong phase.
    let in_push = support::call(
        &mut surface,
        &token,
        "estimate_route",
        &format!(
            r#"{{"waypoints":[{{"voxel":{{"x":{x},"y":{y},"z":{z}}}}},{{"covering":{{"vent":{{"rank":"NEAREST","coverage":"UNCOVERED"}}}}}}]}}"#
        ),
    );
    assert_eq!(support::code(&in_push), "PHASE_CLOSED", "{in_push:?}");

    // A covering that covers nothing is NOT_FOUND, naming the step failure.
    let mut fresh = opening();
    let mut left = FIRST_LULL_MS;
    let refused = call_in_lull(
        &mut fresh.0,
        &fresh.1,
        &mut left,
        "estimate_route",
        &format!(
            r#"{{"waypoints":[{{"voxel":{{"x":{x},"y":{y},"z":{z}}}}},{{"covering":{{"feature_id":"vent_1_1"}}}}]}}"#
        ),
    );
    assert_eq!(support::code(&refused), "NOT_FOUND");
}

#[test]
fn a_hidden_or_foreign_name_answers_no_target() {
    let (mut surface, token) = opening();
    let mut left = FIRST_LULL_MS;
    let place = |name: &str| {
        format!(
            r#"{{"schema_version":{{"major":1}},"meta":{{"title":"t","author_kind":"HUMAN"}},"kind":"PLAYBOOK",
            "declarative":{{"route":[{{"label":"p","place_beacon":{{"at":{{"covering":{{"feature_id":"{name}"}}}}}},"on_fail":{{"action":"SKIP"}}}}]}},
            "on_death":{{"on_respawn":"CONTINUE"}},"fallback":{{"hold":{{"at":{{"safest":{{}}}}}}}}}}"#
        )
    };
    let visit = |beacon: &str| {
        format!(
            r#"{{"schema_version":{{"major":1}},"meta":{{"title":"t","author_kind":"HUMAN"}},"kind":"PLAYBOOK",
            "declarative":{{"route":[{{"label":"v","interface":{{"beacon":{{"beacon_id":"{beacon}"}},"rows":[
              {{"add_build_target":{{"target":{{"blueprint_id":"generator","anchor":{{"on":{{"vent":{{"rank":"NEAREST"}}}}}}}}}}}}]}},"on_fail":{{"action":"SKIP"}}}}]}},
            "on_death":{{"on_respawn":"CONTINUE"}},"fallback":{{"hold":{{"at":{{"safest":{{}}}}}}}}}}"#
        )
    };
    // The answer without its pointer: what a seat learns from it.
    let learned = |answer: &[Json]| -> Vec<(String, String, i64, usize, String)> {
        answer
            .iter()
            .map(|one| {
                (
                    text_of(one, "feature_id"),
                    text_of(one, "failure"),
                    number(one, "travel_ms"),
                    array_of(one, "candidates").len(),
                    pharmakos_proto::json::write(one.get("matched").expect("matched")),
                )
            })
            .collect()
    };

    // A feature name the map does not have, by both arms that take a name.
    let absent = refs(&mut surface, &token, &mut left, &place("vent_1_1"));
    assert_eq!(
        learned(&absent),
        vec![(
            String::new(),
            String::from("no_target"),
            0,
            0,
            String::from("0\n")
        )]
    );

    // A beacon the seat does not have and a handle nobody minted: one answer,
    // the same as the absent feature's.
    let mine_absent = learned(&refs(&mut surface, &token, &mut left, &visit("b_07")));
    let never = learned(&refs(&mut surface, &token, &mut left, &visit("e_99")));
    assert_eq!(mine_absent, learned(&absent));
    assert_eq!(never, mine_absent);

    // Another seat's beacon the seat **has** been shown: in an unfogged match
    // `list_beacons` names seat 1's core `e_01` to seat 0, and that real name
    // still answers exactly what an absent one does.
    let mut casual = support::hosted_casual(2, SEGMENT_MS, 3);
    let seen = support::seat_token(&mut casual, 0);
    let mut casual_left = FIRST_LULL_MS;
    let listed = result(
        &call_in_lull(&mut casual, &seen, &mut casual_left, "list_beacons", "{}"),
        "list_beacons",
    );
    let names: Vec<String> = array_of(&listed, "beacons")
        .iter()
        .map(|beacon| text_of(beacon, "beacon_id"))
        .collect();
    assert!(
        names.iter().any(|name| name == "e_01"),
        "seat 1's core is shown to seat 0 as `e_01`: {names:?}"
    );
    let foreign = learned(&refs(&mut casual, &seen, &mut casual_left, &visit("e_01")));
    assert_eq!(foreign, mine_absent);

    // The seat's own core, by contrast, reads a vent inside its sphere or says
    // why not -- and either way through the sim's own `on` resolver.
    let own = refs(&mut surface, &token, &mut left, &visit("b_00"));
    assert_eq!(own.len(), 1);
}

#[test]
fn an_on_nearest_reads_the_vent_the_sim_binds() {
    let (mut surface, token) = opening();
    let mut left = FIRST_LULL_MS;
    // The scenario's playbook with its `on` ranked rather than named: the
    // nearest free vent inside the new beacon's sphere, which on the golden
    // seed is the vent the covering bound.
    let playbook = cover_and_build().replace(
        r#""on": {"covered": {}}"#,
        r#""on": {"vent": {"rank": "NEAREST"}}"#,
    );
    assert_ne!(playbook, cover_and_build(), "the `on` was rewritten");
    let answered = refs(&mut surface, &token, &mut left, &playbook);
    let on = answered.get(1).expect("the on");
    assert_eq!(text_of(on, "failure"), "", "it reads a vent: {on:?}");
    let picked = text_of(on, "feature_id");
    let candidates = array_of(on, "candidates");
    assert_eq!(
        candidates.first().map(|first| text_of(first, "feature_id")),
        Some(picked.clone()),
        "a ranked pick is the first listed candidate"
    );
    assert_eq!(
        number(on, "matched"),
        i64::try_from(candidates.len()).expect("fits"),
        "every free vent inside the sphere is reachable from its centre here"
    );

    let sealed = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "submit_plan",
        &format!(r#"{{"playbook_jsonc":{}}}"#, support::quote(&playbook)),
    );
    assert_eq!(
        result(&sealed, "submit_plan").get("accepted"),
        Some(&Json::Bool(true)),
        "{sealed:?}"
    );
    let state = first_step_state(&mut surface);
    let binding = state.bindings.first().expect("the step bound its `on`");
    assert_eq!(
        feature_name(&surface, binding.feature),
        picked,
        "the sim's `on {{vent: NEAREST}}` bound the vent the gateway named"
    );
}

/// A one-step playbook visiting the seat's beacon `beacon` with `rows`.
fn visit(beacon: &str, rows: &str) -> String {
    format!(
        r#"{{"schema_version":{{"major":1}},"meta":{{"title":"t","author_kind":"HUMAN"}},"kind":"PLAYBOOK",
        "declarative":{{"route":[{{"label":"v","interface":{{"beacon":{{"beacon_id":"{beacon}"}},"rows":[{rows}]}},"on_fail":{{"action":"SKIP"}}}}]}},
        "on_death":{{"on_respawn":"CONTINUE"}},"fallback":{{"hold":{{"at":{{"safest":{{}}}}}}}}}}"#
    )
}

/// A `set_mandate` row switching to Build with one Generator `on` the
/// nearest vent.
const SWITCH_TO_BUILD_ON_NEAREST: &str = r#"{"set_mandate":{"build":{"targets":[
    {"blueprint_id":"generator","anchor":{"on":{"vent":{"rank":"NEAREST"}}}}]}}}"#;

#[test]
fn a_set_mandate_rows_on_reads_the_vent_the_sim_binds() {
    let (mut surface, token) = round_two_with_a_beacon_on_a_free_vent("");
    let mut left = LULL_MS;

    // Since the `mine` lane a switch carries the settings it writes, and the
    // sim binds their `on` anchors when the step starts: the walk lists them,
    // where it used to list nothing and the count guard then refused.
    let playbook = visit("b_01", SWITCH_TO_BUILD_ON_NEAREST);
    let answered = refs(&mut surface, &token, &mut left, &playbook);
    // The column as well as the feature (review B's nit, decisions-log item
    // 133 (5)): the `on` column the sim's own resolver reads over the frozen
    // world in the Lull, from `b_01`'s sphere, is the column the step binds.
    let lull_column = nearest_on_column(&surface, 1);
    assert_eq!(answered.len(), 1, "the switch's one `on`: {answered:?}");
    let on = answered.first().expect("the on");
    assert_eq!(
        text_of(on, "pointer"),
        "/declarative/route/0/interface/rows/0/set_mandate/build/targets/0/anchor/on"
    );
    assert_eq!(text_of(on, "failure"), "", "it reads a vent: {on:?}");
    let picked = text_of(on, "feature_id");
    assert_eq!(
        array_of(on, "candidates")
            .first()
            .map(|first| text_of(first, "feature_id")),
        Some(picked.clone()),
        "a ranked pick is the first listed candidate"
    );

    let sealed = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "submit_plan",
        &format!(r#"{{"playbook_jsonc":{}}}"#, support::quote(&playbook)),
    );
    assert_eq!(
        result(&sealed, "submit_plan").get("accepted"),
        Some(&Json::Bool(true)),
        "{sealed:?}"
    );
    // The Lull's "this round" sentence reads the seal through the same walk,
    // so the briefing must still answer over a sealed switch with an `on`
    // (before the walk followed the switch, the count guard refused it and
    // took the whole briefing down). The route reads no `covering`, so there
    // is no sentence to say.
    let briefing = call_in_lull(&mut surface, &token, &mut left, "get_briefing", "{}");
    assert!(
        !text_of(&result(&briefing, "get_briefing"), "prose").contains("This round:"),
        "an interface step reads no `covering`: {briefing:?}"
    );
    // The feed is the segment's, so round 1's events are gone by now.
    let state = first_step_state(&mut surface);
    assert_eq!(state.bindings.len(), 1, "the step bound the switch's `on`");
    let binding = state.bindings.first().expect("the binding");
    assert_eq!(
        feature_name(&surface, binding.feature),
        picked,
        "the sim bound the vent the gateway named for the switch's target"
    );
    assert_eq!(
        [binding.at[0], binding.at[1]],
        lull_column,
        "and the `on` column the Lull read, not only its vent"
    );
}

/// Round 2's Lull of a match whose round 1 placed `b_01` covering the nearest
/// vent and built nothing on it, so `b_01`'s sphere holds a free vent (the
/// core's holds none on the golden seed: its starting vent lies outside it).
///
/// `initial` is the placement's initial settings, as JSON members to splice
/// after its `at` (empty for none): a beacon placed with `"initial":
/// {"mandate": {"build": {}}}` stands on the Build writ in round 2, with no
/// target written yet.
fn round_two_with_a_beacon_on_a_free_vent(initial: &str) -> (Surface, Token) {
    let (mut surface, token) = opening();
    let mut left = FIRST_LULL_MS;
    // Round 1 places `b_01` covering the nearest vent and builds nothing on
    // it, so round 2 has a beacon whose sphere holds a free vent (the core's
    // holds none on the golden seed: its starting vent lies outside it).
    let cover_only = format!(
        r#"{{"schema_version":{{"major":1}},"meta":{{"title":"t","author_kind":"HUMAN"}},"kind":"PLAYBOOK",
        "declarative":{{"route":[{{"label":"p","place_beacon":{{"at":{{"covering":{{"vent":{{"rank":"NEAREST","coverage":"UNCOVERED"}}}}}}{initial}}},"on_fail":{{"action":"SKIP"}}}}]}},
        "on_death":{{"on_respawn":"CONTINUE"}},"fallback":{{"hold":{{"at":{{"safest":{{}}}}}}}}}}"#
    );
    let sealed = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "submit_plan",
        &format!(r#"{{"playbook_jsonc":{}}}"#, support::quote(&cover_only)),
    );
    assert_eq!(
        result(&sealed, "submit_plan").get("accepted"),
        Some(&Json::Bool(true)),
        "{sealed:?}"
    );
    surface.begin_push().expect("the Push begins");
    let _ = support::step(&mut surface, 10_000);
    surface.end_recap().expect("the recap ends");
    surface.set_phase_remaining_ms(Ms::new(LULL_MS));
    surface.open_lull().expect("round 2's Lull");
    (surface, token)
}

/// The `on` column the sim's own resolver reads over the hosted world for an
/// `on {vent: NEAREST}` written into seat 0's beacon `b_NN` with ordinal
/// `ordinal`: `pharmakos_sim::targeting::on_vent`, lent the world as
/// `resolve_refs` lends it.
fn nearest_on_column(surface: &Surface, ordinal: u32) -> [i32; 2] {
    use pharmakos_gateway::targeting::{Site, lend, scratch_for, spec_of};
    let world = surface.host().expect("hosted").world();
    let beacons = world.beacons();
    let row = (0..beacons.ids().len())
        .find(|row| {
            beacons.seats().get(*row).copied() == Some(0)
                && beacons.ordinals().get(*row).copied() == Some(ordinal)
        })
        .expect("seat 0 holds the beacon");
    let id = pharmakos_sim::tables::BeaconId::new(beacons.ids().get(row).copied().expect("an id"));
    let centre = beacons.positions().get(row).copied().expect("a position");
    let tally = pharmakos_sim::seams::UnitTally::new();
    let ground = lend(world, &tally);
    let mut scratch = scratch_for(world).expect("a search scratch");
    let reference: pharmakos_proto::gp::v1::FeatureRef = pharmakos_proto::json::decode_json(
        &pharmakos_proto::json::read(r#"{"vent": {"rank": "NEAREST"}}"#).expect("JSON"),
    )
    .expect("a FeatureRef");
    let spec = spec_of(&reference, Site::On { covering: false }).expect("a legal `on`");
    pharmakos_sim::targeting::on_vent(
        &ground,
        &mut scratch,
        SeatId::new(0),
        centre,
        Some(id),
        None,
        spec,
    )
    .expect("the sim reads a vent")
    .column
}

#[test]
fn set_mandate_rows_are_counted_with_the_rows_beside_them() {
    let (mut surface, token) = opening();
    let mut left = FIRST_LULL_MS;
    // A switch between two other rows that bind, and an empty Defend arm the
    // sim compiles to no settings: three `on` anchors, in row order, and the
    // count guard (which refuses a drift as INTERNAL) answers.
    let rows = format!(
        r#"{{"add_build_target":{{"target":{{"blueprint_id":"generator","anchor":{{"on":{{"vent":{{"rank":"NEAREST"}}}}}}}}}}}},
        {SWITCH_TO_BUILD_ON_NEAREST},
        {{"set_mandate":{{"defend":{{}}}}}},
        {{"set_mandate_settings":{{"build":{{"targets":[
          {{"blueprint_id":"generator","anchor":{{"on":{{"vent":{{"rank":"NEAREST"}}}}}}}}]}}}}}}"#
    );
    let answer = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "resolve_refs",
        &format!(
            r#"{{"playbook_jsonc":{}}}"#,
            support::quote(&visit("b_00", &rows))
        ),
    );
    let answered = array_of(&result(&answer, "resolve_refs"), "refs");
    let pointers: Vec<String> = answered.iter().map(|one| text_of(one, "pointer")).collect();
    let at = "/declarative/route/0/interface/rows";
    assert_eq!(
        pointers,
        vec![
            format!("{at}/0/add_build_target/target/anchor/on"),
            format!("{at}/1/set_mandate/build/targets/0/anchor/on"),
            format!("{at}/3/set_mandate_settings/build/targets/0/anchor/on"),
        ]
    );

    // The picks as well as the order (review B's nit, decisions-log item 133
    // (5)): sealed, the step binds, row by row, the vents the gateway named,
    // and fails where the gateway said it would, with the failure it named.
    let named: Vec<(String, String)> = answered
        .iter()
        .map(|one| (text_of(one, "feature_id"), text_of(one, "failure")))
        .collect();
    let sealed = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "submit_plan",
        &format!(
            r#"{{"playbook_jsonc":{}}}"#,
            support::quote(&visit("b_00", &rows))
        ),
    );
    assert_eq!(
        result(&sealed, "submit_plan").get("accepted"),
        Some(&Json::Bool(true)),
        "{sealed:?}"
    );
    let state = first_step_state(&mut surface);
    let bound: Vec<String> = state
        .bindings
        .iter()
        .map(|binding| feature_name(&surface, binding.feature))
        .collect();
    let read: Vec<String> = named
        .iter()
        .take_while(|(_, failure)| failure.is_empty())
        .map(|(feature, _)| feature.clone())
        .collect();
    assert_eq!(
        bound, read,
        "the sim bound what the gateway read, in row order"
    );
    if let Some((_, failure)) = named.iter().find(|(_, failure)| !failure.is_empty()) {
        let _ = support::step(&mut surface, 40);
        let failed = surface
            .feed()
            .events()
            .iter()
            .any(|event| event.text.contains(&format!("({failure})")));
        assert!(
            failed,
            "the step failed `{failure}`, as the gateway read it"
        );
    }
}

/// The picks of a visit's rows, compared with the sim's bindings row by row
/// (review B's nit on `tgtw`, decisions-log item 133 (5)).
#[test]
fn a_visits_rows_bind_the_vents_the_gateway_named_in_row_order() {
    // On the golden seed the core's sphere holds no free vent, so the rows
    // of `set_mandate_rows_are_counted_with_the_rows_beside_them` all read
    // `no_target`. The same three rows on `b_01`, whose sphere
    // holds one, read vents, and the sim binds exactly those, row by row.
    // `b_01` is placed on the Build writ, so an added target, a switch and a
    // settings edit are all legal on it, in that order (the verifier reads an
    // edit against the writ the snapshot holds, not one a row before it
    // switches to).
    let (mut surface, token) =
        round_two_with_a_beacon_on_a_free_vent(r#","initial":{"mandate":{"build":{}}}"#);
    let mut left = LULL_MS;
    let rows = format!(
        r#"{{"add_build_target":{{"target":{{"blueprint_id":"generator","anchor":{{"on":{{"vent":{{"rank":"NEAREST"}}}}}}}}}}}},
        {SWITCH_TO_BUILD_ON_NEAREST},
        {{"set_mandate_settings":{{"build":{{"targets":[
          {{"blueprint_id":"generator","anchor":{{"on":{{"vent":{{"rank":"NEAREST"}}}}}}}}]}}}}}}"#
    );
    let playbook = visit("b_01", &rows);
    let answered = refs(&mut surface, &token, &mut left, &playbook);
    let read: Vec<String> = answered
        .iter()
        .map(|one| {
            assert_eq!(
                text_of(one, "failure"),
                "",
                "each row reads a vent: {one:?}"
            );
            text_of(one, "feature_id")
        })
        .collect();
    assert_eq!(read.len(), 3, "three `on` anchors: {answered:?}");
    let sealed = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "submit_plan",
        &format!(r#"{{"playbook_jsonc":{}}}"#, support::quote(&playbook)),
    );
    assert_eq!(
        result(&sealed, "submit_plan").get("accepted"),
        Some(&Json::Bool(true)),
        "{sealed:?}"
    );
    let state = first_step_state(&mut surface);
    let bound: Vec<String> = state
        .bindings
        .iter()
        .map(|binding| feature_name(&surface, binding.feature))
        .collect();
    assert_eq!(bound, read, "the sim bound each row's vent, in row order");
}

#[test]
fn a_non_seat_is_told_the_map_and_nothing_about_a_seat() {
    let (mut surface, token) = opening();
    let mut left = FIRST_LULL_MS;
    let summary = result(
        &call_in_lull(&mut surface, &token, &mut left, "get_map_summary", "{}"),
        "get_map_summary",
    );
    let features = array_of(&summary, "features");
    assert!(!features.is_empty(), "the golden seed lays vents and seams");
    let names: Vec<String> = features
        .iter()
        .map(|feature| text_of(feature, "feature_id"))
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "in feature id order");
    for feature in &features {
        for key in ["covered", "travel_ms", "reachable", "live"] {
            assert!(feature.get(key).is_some(), "a seat is told `{key}`");
        }
    }
    // Seat 0's starting seam lies inside its core's sphere, its starting vent
    // outside it (the generator's distance bands, rules `map.*`).
    assert!(
        features.iter().any(|feature| {
            text_of(feature, "kind") == "seam" && feature.get("covered") == Some(&Json::Bool(true))
        }),
        "the starting seam is covered"
    );
    write_golden(
        "map_summary",
        &rpc::render(&call_in_lull(
            &mut surface,
            &token,
            &mut left,
            "get_map_summary",
            "{}",
        )),
    );

    let admin = support::admin_token(&mut surface);
    let answer = result(
        &call_in_lull(&mut surface, &admin, &mut left, "get_map_summary", "{}"),
        "get_map_summary",
    );
    for feature in array_of(&answer, "features") {
        for key in ["covered", "travel_ms", "reachable"] {
            assert!(feature.get(key).is_none(), "nobody's `{key}`");
        }
        assert!(feature.get("live").is_some());
    }
}

#[test]
fn resolve_refs_is_goldened_for_the_cover_and_build_playbook() {
    let (mut surface, token) = opening();
    let mut left = FIRST_LULL_MS;
    let answer = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "resolve_refs",
        &format!(
            r#"{{"playbook_jsonc":{}}}"#,
            support::quote(&cover_and_build())
        ),
    );
    let _ = result(&answer, "resolve_refs");
    write_golden("resolve_refs", &rpc::render(&answer));
}

#[test]
fn a_playbook_this_build_cannot_execute_reads_nothing() {
    let (mut surface, token) = opening();
    let mut left = FIRST_LULL_MS;
    // `covering` in a `move` is legal nowhere but a placement's `at`.
    let misplaced = r#"{"schema_version":{"major":1},"meta":{"title":"t","author_kind":"HUMAN"},"kind":"PLAYBOOK",
        "declarative":{"route":[{"label":"m","move":{"to":{"covering":{"vent":{"rank":"NEAREST","coverage":"ANY"}}}}}]},
        "on_death":{"on_respawn":"CONTINUE"},"fallback":{"hold":{"at":{"safest":{}}}}}"#;
    let answer = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "resolve_refs",
        &format!(r#"{{"playbook_jsonc":{}}}"#, support::quote(misplaced)),
    );
    assert_eq!(support::code(&answer), "INVALID_ARGUMENT");
    let half_typed = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "resolve_refs",
        r#"{"playbook_jsonc":"{\"declarative\": "}"#,
    );
    assert_eq!(support::code(&half_typed), "INVALID_ARGUMENT");
}

#[test]
fn the_lull_says_what_this_round_will_do_and_round_two_finds_no_vent() {
    let mut surface = support::hosted_as(MATCH, FogPolicy::fogged(), 2, SEGMENT_MS, 3);
    let token = support::seat_token(&mut surface, 0);
    let mut left = FIRST_LULL_MS;
    let playbook = cover_and_build();
    let briefing = |surface: &mut Surface, left: &mut i32| -> String {
        text_of(
            &result(
                &call_in_lull(surface, &token, left, "get_briefing", "{}"),
                "get_briefing",
            ),
            "prose",
        )
    };
    assert!(
        !briefing(&mut surface, &mut left).contains("This round:"),
        "nothing sealed and nothing carried: nothing to say"
    );
    let _ = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "submit_plan",
        &format!(r#"{{"playbook_jsonc":{}}}"#, support::quote(&playbook)),
    );
    let round_one = briefing(&mut surface, &mut left);
    assert!(
        round_one.contains("This round: step 1 places a new beacon near ("),
        "{round_one}"
    );
    assert!(round_one.ends_with(", $ 60."), "{round_one}");

    // A re-seal changes the sentence at once: covering the nearest seam,
    // which the core already covers, places the beacon somewhere else (and
    // builds nothing, since a Generator never stands on a seam).
    let seam = r#"{"schema_version":{"major":1},"meta":{"title":"t","author_kind":"HUMAN"},"kind":"PLAYBOOK",
        "declarative":{"route":[{"label":"p","place_beacon":{"at":{"covering":{"seam":{"rank":"NEAREST","coverage":"ANY"}}}},"on_fail":{"action":"SKIP"}}]},
        "on_death":{"on_respawn":"CONTINUE"},"fallback":{"hold":{"at":{"safest":{}}}}}"#;
    let resealed = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "submit_plan",
        &format!(r#"{{"playbook_jsonc":{}}}"#, support::quote(seam)),
    );
    assert_eq!(
        result(&resealed, "submit_plan").get("accepted"),
        Some(&Json::Bool(true)),
        "{resealed:?}"
    );
    let resealed_says = briefing(&mut surface, &mut left);
    assert!(
        resealed_says.contains("This round: step 1"),
        "{resealed_says}"
    );
    assert_ne!(
        resealed_says, round_one,
        "the re-seal is what the Lull reads"
    );

    // Back to the vent playbook, which round 2 then carries.
    let _ = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "submit_plan",
        &format!(r#"{{"playbook_jsonc":{}}}"#, support::quote(&playbook)),
    );
    assert_eq!(briefing(&mut surface, &mut left), round_one);

    // Round 1 covers the starting vent; round 2's carried copy finds none it
    // can cover (targeting.md, "What the map means for it"; item 131 (4) (a)).
    surface.begin_push().expect("the Push begins");
    let _ = support::step(&mut surface, 10_000);
    surface.end_recap().expect("the recap ends");
    surface.set_phase_remaining_ms(Ms::new(LULL_MS));
    surface.open_lull().expect("round 2's Lull");
    let mut left = LULL_MS;
    let round_two = briefing(&mut surface, &mut left);
    assert!(
        round_two.ends_with("This round: step 1 finds no vent you can cover."),
        "{round_two}"
    );

    // The advisor reads this briefing with a seat token that never holds
    // `plan.submit` (decisions-log item 111): it is told the briefing without
    // the sentence, which comes from the seat's own playbook.
    let advisor = advisor_token(&mut surface, 0);
    let advised = text_of(
        &result(
            &call_in_lull(&mut surface, &advisor, &mut left, "get_briefing", "{}"),
            "get_briefing",
        ),
        "prose",
    );
    assert!(!advised.contains("This round:"), "{advised}");
    assert!(round_two.starts_with(&advised), "{advised} / {round_two}");

    // The carried draft's id is the seat's to overwrite, with anything at
    // all; the sentence reads the seal, never the draft list, so it neither
    // moves nor breaks.
    let saved = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "save_draft",
        r#"{"draft_id":"carried","playbook_jsonc":"{ not json"}"#,
    );
    let _ = result(&saved, "save_draft");
    assert_eq!(briefing(&mut surface, &mut left), round_two);
}

/// A token shaped as the built-in advisor's for `seat`: the seat's subject,
/// with `observe`, `plan` and `docs` and never `plan.submit`
/// (`pharmakos_gateway::serve`'s advisor scopes).
fn advisor_token(surface: &mut Surface, seat: u8) -> Token {
    let scopes = ScopeSet::of(&[Scope::Observe, Scope::Plan, Scope::Docs]);
    let tick = surface.time().tick;
    let held = surface.match_id().to_owned();
    let (token, _) = surface
        .tokens()
        .mint(Subject::Seat(SeatId::new(seat)), &held, scopes, tick)
        .expect("minted");
    token
}

#[test]
fn a_step_that_found_nothing_says_why_in_the_feed() {
    let event = pharmakos_sim::events::Event {
        tick: pharmakos_sim::math::quantity::Tick::new(1),
        seq: 0,
        kind: EventKind::StepFailed,
        seat: Some(SeatId::new(0)),
        subject: None,
        at: None,
        value: i64::from(pharmakos_sim::interpreter::StepFailure::NoTarget.id()),
    };
    let line = pharmakos_gateway::strings::event_text(&event);
    assert!(line.contains("(no_target)"), "{line}");
    assert!(line.contains("found nothing"), "{line}");
    assert!(!line.contains("code"), "named, not numbered: {line}");
}

/// Write the fresh answer and compare it with the committed golden, as
/// `tests/demo_playbooks.rs` does for its own cases.
fn write_golden(case: &str, rendered: &str) {
    let file = "response.json";
    assert!(!rendered.contains('\r') && rendered.ends_with('\n'));
    let fresh = support::target_dir()
        .join("golden")
        .join("gateway")
        .join(case);
    std::fs::create_dir_all(&fresh).expect("the fresh output's folder");
    std::fs::write(fresh.join(format!("actual.{file}")), rendered).expect("the fresh output");
    let committed = support::workspace_root()
        .join("tests")
        .join("golden")
        .join("gateway")
        .join(case)
        .join(format!("expected.{file}"));
    let Ok(expected) = std::fs::read_to_string(&committed) else {
        panic!(
            "no committed golden at {}. A new case is committed by copying the fresh output \
             beside it, byte for byte, once it is right; `cargo xtask golden --bless` rewrites \
             only goldens that exist.",
            committed.display()
        );
    };
    assert_eq!(
        expected,
        rendered,
        "the golden at {} moved. tests/golden/gateway/README.md says what a diff means; the \
         pull request has to say which input moved it.",
        committed.display()
    );
}

#[test]
fn the_verifier_scope_carries_the_maps_features() {
    let (mut surface, token) = opening();
    let mut left = FIRST_LULL_MS;
    let summary = result(
        &call_in_lull(&mut surface, &token, &mut left, "get_map_summary", "{}"),
        "get_map_summary",
    );
    let vent = array_of(&summary, "features")
        .into_iter()
        .map(|feature| text_of(&feature, "feature_id"))
        .find(|name| name.starts_with("vent_"))
        .expect("a vent");
    let codes = |surface: &mut Surface, left: &mut i32, name: &str| -> Vec<String> {
        let playbook = format!(
            r#"{{"schema_version":{{"major":1}},"meta":{{"title":"t","author_kind":"HUMAN"}},"kind":"PLAYBOOK",
            "declarative":{{"route":[{{"label":"p","place_beacon":{{"at":{{"covering":{{"feature_id":"{name}"}}}}}},"on_fail":{{"action":"SKIP"}}}}]}},
            "on_death":{{"on_respawn":"CONTINUE"}},"fallback":{{"hold":{{"at":{{"safest":{{}}}}}}}}}}"#
        );
        let answer = call_in_lull(
            surface,
            &token,
            left,
            "verify_plan",
            &format!(
                r#"{{"depth":"full","playbook_jsonc":{}}}"#,
                support::quote(&playbook)
            ),
        );
        let report = result(&answer, "verify_plan")
            .get("report")
            .cloned()
            .expect("a report");
        array_of(&report, "diagnostics")
            .iter()
            .map(|diagnostic| text_of(diagnostic, "code"))
            .collect()
    };
    assert!(
        !codes(&mut surface, &mut left, &vent).contains(&String::from("E0412")),
        "a vent the map has is in the seat's view"
    );
    assert!(
        codes(&mut surface, &mut left, "vent_1_1").contains(&String::from("E0412")),
        "a vent it has not is not"
    );
}

#[test]
fn resolve_refs_in_a_push_is_phase_closed() {
    // Decisions-log item 133 (3) (f): in a Push the hosted world is the live
    // one, and a preview over it would rank over the live map, which
    // `estimate_route`'s `covering` already refuses. What a client is told,
    // through the wire: here the dispatcher's planning door answers first
    // (`resolve_refs` is `plan`-scoped, and that door closed every planning
    // method in a Push before `econ`), so this case holds the answer and not
    // the handler's own gate, which `surface::tests::
    // resolve_refs_closes_its_own_door_in_a_push` reaches directly.
    let mut surface = support::hosted(2, SEGMENT_MS, 3);
    let token = support::seat_token(&mut surface, 0);
    let playbook = cover_and_build();
    let params = format!(r#"{{"playbook_jsonc":{}}}"#, support::quote(&playbook));
    let mut left = FIRST_LULL_MS;
    let in_lull = call_in_lull(&mut surface, &token, &mut left, "resolve_refs", &params);
    let _ = result(&in_lull, "resolve_refs in the Lull");

    assert!(surface.begin_push().expect("the Push begins"));
    let _ = support::step(&mut surface, 5);
    let in_push = support::call(&mut surface, &token, "resolve_refs", &params);
    assert_eq!(support::code(&in_push), "PHASE_CLOSED", "{in_push:?}");
}
