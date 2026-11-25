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
//! `on` listing (the sim's own filter since `fog`) to the sim's
//! `on {vent: NEAREST}` binding, as the first acceptance line holds its
//! `covering` listing;
//! `a_named_vent_outside_the_sphere_matches_nothing_as_the_sim_reports` holds
//! a named `on` to the sim's sphere-first reading (0 matched, `no_target`);
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

use support::{FIRST_LULL_MS, LULL_MS, MATCH, array_of, call_in_lull, code, result, text_of};

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
    // `travel_ms` is written only beside `reachable: true` (decisions-log
    // item 133 (3) (i)), so a reference that reads nothing carries none.
    let learned = |answer: &[Json]| -> Vec<(String, String, Option<i64>, usize, String)> {
        answer
            .iter()
            .map(|one| {
                (
                    text_of(one, "feature_id"),
                    text_of(one, "failure"),
                    one.get("travel_ms").map(|_| number(one, "travel_ms")),
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
            None,
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
    surface
        .set_phase_remaining_ms(Ms::new(LULL_MS))
        .expect("a custom match takes a countdown");
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
        for key in ["covered", "reachable", "live"] {
            assert!(feature.get(key).is_some(), "a seat is told `{key}`");
        }
        // `travel_ms` beside `reachable: true` and never beside `false`, so a
        // 0 is a feature at the commander's feet and never "unreachable"
        // (decisions-log item 133 (3) (i)).
        assert_eq!(
            feature.get("travel_ms").is_some(),
            feature.get("reachable") == Some(&Json::Bool(true)),
            "{feature:?}"
        );
    }
    // Seat 0's starting seam lies inside its core's sphere, its starting vent
    // outside it (the generator's distance bands, rules `map.*`).
    assert!(
        features.iter().any(|feature| {
            text_of(feature, "kind") == "seam" && feature.get("covered") == Some(&Json::Bool(true))
        }),
        "the starting seam is covered"
    );
    support::write_golden(
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
    support::write_golden("resolve_refs", &rpc::render(&answer));
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
    // S3's `pred`: `toward` compiles in a placement's `at` and `resolve_refs`
    // reads it through the sim's own stepping-stone search, answering a
    // reference at its feature's pointer (here a pick, ranked as a
    // `covering`'s is) rather than a method error.
    let toward = r#"{"schema_version":{"major":1},"meta":{"title":"t","author_kind":"HUMAN"},"kind":"PLAYBOOK",
        "declarative":{"route":[{"label":"p","place_beacon":{"at":{"toward":{"feature":{"vent":{"rank":"NEAREST","coverage":"UNCOVERED"}}}}}}]},
        "on_death":{"on_respawn":"CONTINUE"},"fallback":{"hold":{"at":{"safest":{}}}}}"#;
    let answer = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "resolve_refs",
        &format!(r#"{{"playbook_jsonc":{}}}"#, support::quote(toward)),
    );
    let _ = result(&answer, "resolve_refs");
    assert!(
        rpc::render(&answer).contains("/declarative/route/0/place_beacon/at/toward/feature"),
        "the `toward` is read at its feature's pointer: {}",
        rpc::render(&answer)
    );
    // And outside a placement's `at` it is misplaced, as `covering` is.
    let misplaced_toward = r#"{"schema_version":{"major":1},"meta":{"title":"t","author_kind":"HUMAN"},"kind":"PLAYBOOK",
        "declarative":{"route":[{"label":"m","move":{"to":{"toward":{"feature":{"vent":{"rank":"NEAREST","coverage":"ANY"}}}}}}]},
        "on_death":{"on_respawn":"CONTINUE"},"fallback":{"hold":{"at":{"safest":{}}}}}"#;
    let answer = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "resolve_refs",
        &format!(
            r#"{{"playbook_jsonc":{}}}"#,
            support::quote(misplaced_toward)
        ),
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
    surface
        .set_phase_remaining_ms(Ms::new(LULL_MS))
        .expect("a custom match takes a countdown");
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

    // The carried draft's id is the gateway's, which no seat writes (lane
    // `opsave`'s draft rule): saving over it is refused, and the sentence,
    // which reads the seal and never the draft list, neither moves nor breaks;
    // nor does it when the seat saves the same text under an id of its own.
    for (id, wanted) in [("carried", "INVALID_ARGUMENT"), ("mine", "<no error>")] {
        let params = format!("{{\"draft_id\":\"{id}\",\"playbook_jsonc\":\"{{ not json\"}}");
        let saved = call_in_lull(&mut surface, &token, &mut left, "save_draft", &params);
        assert_eq!(code(&saved), wanted, "{id}: {saved:?}");
        assert_eq!(briefing(&mut surface, &mut left), round_two);
    }
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

#[test]
fn a_named_vent_outside_the_sphere_matches_nothing_as_the_sim_reports() {
    use pharmakos_gateway::targeting::{Site, commander_point, lend, scratch_for, spec_of};
    use pharmakos_sim::features::FeatureKind;
    use pharmakos_sim::interpreter::StepFailure;
    use pharmakos_sim::targeting::{anchor_in_sphere, cover, on_vent_counted};

    let (mut surface, token) = opening();
    let mut left = FIRST_LULL_MS;
    // Where the covering lands, by the sim's own resolver, and a live vent
    // whose anchor point lies outside the sphere centred there.
    let (centre, far, far_index) = {
        let world = surface.host().expect("hosted").world();
        let tally = pharmakos_sim::seams::UnitTally::new();
        let ground = lend(world, &tally);
        let mut scratch = scratch_for(world).expect("a search scratch");
        let reference: pharmakos_proto::gp::v1::FeatureRef = pharmakos_proto::json::decode_json(
            &pharmakos_proto::json::read(
                r#"{"vent": {"rank": "NEAREST", "coverage": "UNCOVERED"}}"#,
            )
            .expect("JSON"),
        )
        .expect("a FeatureRef");
        let spec = spec_of(&reference, Site::Covering).expect("a legal `covering`");
        let commander = commander_point(world, SeatId::new(0)).expect("a living commander");
        let found = cover(&ground, &mut scratch, SeatId::new(0), commander, spec)
            .expect("the sim covers a vent");
        let [x, y] = found.site;
        let centre = ground.standing(x, y).expect("a site on the map");
        let far_index = (0..ground.features.len())
            .find(|index| {
                ground
                    .features
                    .get(*index)
                    .is_some_and(|feature| feature.kind == FeatureKind::Vent)
                    && ground.is_live(*index)
                    && !anchor_in_sphere(&ground, centre, *index)
            })
            .expect("the golden seed has a live vent outside the new beacon's sphere");
        let far = ground.features.get(far_index).expect("a feature").name();
        (centre, far, far_index)
    };

    // The scenario's playbook with its `on` naming that vent.
    let playbook = cover_and_build().replace(
        r#""on": {"covered": {}}"#,
        &format!(r#""on": {{"feature_id": "{far}"}}"#),
    );
    assert_ne!(playbook, cover_and_build(), "the `on` was rewritten");
    let answered = refs(&mut surface, &token, &mut left, &playbook);
    let on = answered.get(1).expect("the on");
    assert_eq!(text_of(on, "failure"), "no_target", "{on:?}");
    assert_eq!(
        number(on, "matched"),
        0,
        "a named vent outside the sphere is no candidate: {on:?}"
    );
    assert!(array_of(on, "candidates").is_empty(), "{on:?}");

    // The sim's own reading of the same name, from the same centre.
    let world = surface.host().expect("hosted").world();
    let tally = pharmakos_sim::seams::UnitTally::new();
    let ground = lend(world, &tally);
    let mut scratch = scratch_for(world).expect("a search scratch");
    let reference: pharmakos_proto::gp::v1::FeatureRef = pharmakos_proto::json::decode_json(
        &pharmakos_proto::json::read(&format!(r#"{{"feature_id": "{far}"}}"#)).expect("JSON"),
    )
    .expect("a FeatureRef");
    let spec = spec_of(&reference, Site::On { covering: true }).expect("a legal `on`");
    let missed = on_vent_counted(
        &ground,
        &mut scratch,
        SeatId::new(0),
        centre,
        None,
        None,
        spec,
    )
    .expect_err("the sim reads no vent outside the sphere");
    assert_eq!(missed.reason, StepFailure::NoTarget);
    assert_eq!(
        missed.matched,
        Some(0),
        "the sim counts the name outside the sphere as matching nothing (feature {far_index})"
    );

    // Sealed and played, the step fails as both read it, and the recap names
    // why in the seat's own words: matched nothing, so the feed's words.
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
    surface.begin_push().expect("the Push begins");
    let _ = support::step(&mut surface, 10_000);
    let recap = result(
        &support::call(&mut surface, &token, "get_recap", "{}"),
        "get_recap",
    );
    let said =
        pharmakos_gateway::strings::recap_step_failed(missed.value(), core::num::NonZeroUsize::MIN);
    assert!(
        text_of(&recap, "prose").contains(&said),
        "the recap says `{said}`: {recap:?}"
    );
}

/// S3's `lic` (decision 6): `covering_beacon` asks the sim's own sphere test
/// for a placed beacon, so a finished Resonance Spire's larger sphere covers
/// for the scope's `covered_by`, `get_map_summary` and W0704/W0706 exactly
/// where the sim's `covering` does.
#[test]
fn covering_beacon_reads_a_spires_grown_sphere() {
    use pharmakos_sim::math::fixed::Fx;
    let rules = pharmakos_sim::RulesTable::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("rules")
            .join("rules.v1.json"),
    )
    .expect("the committed rules table loads");
    let base = rules.sphere_radius_voxels();
    let mut world = pharmakos_sim::World::new(&pharmakos_sim::WorldConfig {
        match_seed: 0x0000_0000_ca5c_aded,
        seats: 2,
        rules,
        match_settings: pharmakos_sim::MatchSettings {
            segment_lengths_ms: vec![180_000],
            round_limit: 1,
        },
    })
    .expect("a world");
    let seat = SeatId::new(0);
    let row = world
        .beacons()
        .seats()
        .iter()
        .position(|owner| *owner == 0)
        .expect("seat 0's core");
    let core = pharmakos_sim::tables::BeaconId::new(
        world.beacons().ids().get(row).copied().expect("an id"),
    );
    let at = world
        .beacons()
        .positions()
        .get(row)
        .copied()
        .expect("a place");
    let east = |voxels: i16| [at[0].saturating_add(Fx::from_voxels(voxels)), at[1], at[2]];
    let ring = east(base + 4);
    assert_eq!(
        pharmakos_gateway::targeting::covering_beacon(&world.ground(), seat, ring),
        None,
        "past the base sphere, no beacon covers it"
    );
    world
        .raise_structure(
            seat,
            core,
            pharmakos_sim::tables::StructureKind::ResonanceSpire,
            east(1),
        )
        .expect("room for a Spire");
    assert_eq!(
        pharmakos_gateway::targeting::covering_beacon(&world.ground(), seat, ring),
        Some(0),
        "the core's grown sphere covers it"
    );
}

/// The settings of [`every_lull_reads_and_plays_on_as_its_save_does`]: a
/// Push of 200 ticks, three rounds.
const SHORT_SEGMENT_MS: i32 = 10_000;

/// See [`SHORT_SEGMENT_MS`].
const SHORT_ROUNDS: u32 = 3;

/// The three planning reads that read the hosted world's pathing graph, as
/// seat 0 makes them, each answer's `result` with its `_status` footer taken
/// out (the footer is the clock's, which a resume restarts).
fn graph_reads(surface: &mut Surface, vent: &str) -> Vec<Json> {
    let token = support::seat_token(surface, 0);
    let mut left = LULL_MS;
    let [x, y, z] = support::commander_voxel(surface, 0);
    let calls = [
        ("get_map_summary", String::from("{}")),
        (
            "resolve_refs",
            format!(
                r#"{{"playbook_jsonc":{}}}"#,
                support::quote(&cover_and_build())
            ),
        ),
        (
            "estimate_route",
            format!(
                r#"{{"waypoints":[{{"voxel":{{"x":{x},"y":{y},"z":{z}}}}},{{"covering":{{"feature_id":"{vent}"}}}}]}}"#
            ),
        ),
    ];
    calls
        .iter()
        .map(|(method, params)| {
            let answer = call_in_lull(surface, &token, &mut left, method, params);
            match result(&answer, method) {
                Json::Object(fields) => Json::Object(
                    fields
                        .into_iter()
                        .filter(|(key, _)| key != "_status")
                        .collect(),
                ),
                other => other,
            }
        })
        .collect()
}

/// Seat 0's nearest vent by its own `get_map_summary`, and three craters at
/// the quarters of the route its commander would walk there.
fn nearest_vent_and_its_way(surface: &mut Surface) -> (String, Vec<[i32; 3]>) {
    use pharmakos_sim::pathing::{Scratch, route::route};
    let token = support::seat_token(surface, 0);
    let mut left = FIRST_LULL_MS;
    let summary = result(
        &call_in_lull(surface, &token, &mut left, "get_map_summary", "{}"),
        "get_map_summary",
    );
    let (vent, column) = array_of(&summary, "features")
        .into_iter()
        .filter(|feature| text_of(feature, "kind") == "vent")
        .filter(|feature| feature.get("reachable") == Some(&Json::Bool(true)))
        .min_by_key(|feature| (number(feature, "travel_ms"), text_of(feature, "feature_id")))
        .map(|feature| {
            (
                text_of(&feature, "feature_id"),
                [number(&feature, "x"), number(&feature, "y")],
            )
        })
        .expect("seat 0 reaches a vent");
    let world = surface.host().expect("hosted").world();
    let [cx, cy, _] = support::commander_voxel(surface, 0);
    let start = world
        .surface()
        .node_of(cx, cy)
        .expect("the commander's column");
    let goal = world
        .surface()
        .node_of(
            i32::try_from(column[0]).expect("a column"),
            i32::try_from(column[1]).expect("a column"),
        )
        .expect("the vent's column");
    let mut scratch =
        Scratch::for_map(world.surface(), world.clusters().size()).expect("a scratch");
    let mut walk = Vec::new();
    route(
        world.surface(),
        world.clusters(),
        &mut scratch,
        start,
        goal,
        &mut walk,
    )
    .expect("the commander's way to the vent");
    let craters = [1_usize, 2, 3]
        .into_iter()
        .map(|quarter| {
            let node = walk
                .get((walk.len() * quarter).div_euclid(4))
                .copied()
                .expect("a column of the way");
            let (x, y) = world.surface().coord_of(node);
            [x, y, world.voxels().top_solid_z(x, y).expect("a floor")]
        })
        .collect();
    (vent, craters)
}

/// A one-step playbook whose `place_beacon` covers the nearest feature of
/// `kind` (`vent` or `seam`), covered or not: `resolve_refs` lists every
/// reachable feature of the kind as a candidate, ranked from the commander.
fn covering_nearest(kind: &str) -> String {
    format!(
        r#"{{
  "schema_version": {{"major": 1, "minor": 0}},
  "meta": {{"title": "Cover the nearest {kind}", "author_kind": "HUMAN"}},
  "kind": "PLAYBOOK",
  "declarative": {{
    "route": [
      {{"label": "cover",
        "place_beacon": {{"at": {{"covering": {{"{kind}": {{"rank": "NEAREST", "coverage": "ANY"}}}}}}}},
        "timeout_ms": 120000, "on_fail": {{"action": "SKIP"}}}}
    ],
    "handlers": []
  }},
  "on_death": {{"on_respawn": "CONTINUE", "max_deaths_before_fallback": 2}},
  "fallback": {{"hold": {{"at": {{"safest": {{}}}}}}}}
}}"#
    )
}

/// Seat 0's commander's voxel, and for each feature `reached` names the
/// column the sim's ranker measures it at from there (`Ground::measure_to`),
/// as a voxel on the ground.
fn measured_goals(
    surface: &Surface,
    reached: &[(String, i64)],
) -> ([i32; 3], Vec<(String, [i32; 3])>) {
    let world = surface.host().expect("hosted").world();
    let [cx, cy, cz] = support::commander_voxel(surface, 0);
    let origin = world
        .surface()
        .node_of(cx, cy)
        .expect("the commander's column");
    let ground = world.ground();
    let reads = pharmakos_gateway::targeting::feature_reads(world, Some(SeatId::new(0)));
    let goals = reached
        .iter()
        .map(|(name, _)| {
            let read = reads
                .iter()
                .find(|read| &read.name == name)
                .expect("a summary feature is in the table");
            let column = ground
                .measure_to(read.index, origin)
                .unwrap_or_else(|| panic!("{name} is reached and measured to nowhere"));
            let (x, y) = world.surface().coord_of(column);
            let z = world.voxels().top_solid_z(x, y).expect("a floor");
            (name.clone(), [x, y, z])
        })
        .collect();
    ([cx, cy, cz], goals)
}

/// **One measure across the four readers** (S3's `p3fix`, the gateway step):
/// for seat 0 in this Lull, every reachable feature's `get_map_summary`
/// `travel_ms`, the `travel_ms` `resolve_refs` lists it at as a `covering`
/// candidate, and `estimate_route`'s leg from the commander's voxel to the
/// column the sim's ranker measures it at (`Ground::measure_to`) are one
/// figure: the walk the router serves, which is what the Push walks. Before
/// the step `estimate_route` priced a far leg at the abstract graph's
/// unsmoothed cost, over a second copy of the graph, and the chip and the
/// route disagreed. Returns how many features it compared.
fn assert_one_measure(surface: &mut Surface, round: u32, how: &str) -> usize {
    let token = support::seat_token(surface, 0);
    let mut left = LULL_MS;
    let summary = result(
        &call_in_lull(surface, &token, &mut left, "get_map_summary", "{}"),
        "get_map_summary",
    );
    let reached: Vec<(String, i64)> = array_of(&summary, "features")
        .iter()
        .filter(|feature| feature.get("reachable") == Some(&Json::Bool(true)))
        .map(|feature| (text_of(feature, "feature_id"), number(feature, "travel_ms")))
        .collect();
    assert!(
        !reached.is_empty(),
        "round {round}, {how}: seat 0 reaches no feature, so nothing is compared"
    );
    let summary_ms = |name: &str| -> i64 {
        reached.iter().find(|(id, _)| id == name).map_or_else(
            || panic!("round {round}, {how}: {name} is a candidate the summary does not reach"),
            |(_, ms)| *ms,
        )
    };

    // `resolve_refs`: every candidate of a `covering` of each kind.
    let mut listed = 0_usize;
    for kind in ["vent", "seam"] {
        let answer = result(
            &call_in_lull(
                surface,
                &token,
                &mut left,
                "resolve_refs",
                &format!(
                    r#"{{"playbook_jsonc":{}}}"#,
                    support::quote(&covering_nearest(kind))
                ),
            ),
            "resolve_refs",
        );
        let refs = array_of(&answer, "refs");
        let [reference] = refs.as_slice() else {
            panic!("round {round}, {how}: one reference, and this is {refs:?}");
        };
        for candidate in array_of(reference, "candidates") {
            let name = text_of(&candidate, "feature_id");
            assert_eq!(
                number(&candidate, "travel_ms"),
                summary_ms(&name),
                "round {round}, {how}: resolve_refs binds {name} at another figure than the chip's"
            );
            listed += 1;
        }
    }
    assert_eq!(
        listed,
        reached.len(),
        "round {round}, {how}: resolve_refs lists a reachable feature set other than the summary's"
    );

    // `estimate_route`: the commander out to each feature and back, legs
    // 0, 2, 4, ... being the walks out.
    let (start, goals) = measured_goals(surface, &reached);
    let voxel = |[x, y, z]: [i32; 3]| format!(r#"{{"voxel":{{"x":{x},"y":{y},"z":{z}}}}}"#);
    // Thirty-one features a call: 63 waypoints, inside `MAX_WAYPOINTS`.
    for chunk in goals.chunks(31) {
        let mut waypoints = vec![voxel(start)];
        for (_, goal) in chunk {
            waypoints.push(voxel(*goal));
            waypoints.push(voxel(start));
        }
        let answer = result(
            &call_in_lull(
                surface,
                &token,
                &mut left,
                "estimate_route",
                &format!(r#"{{"waypoints":[{}]}}"#, waypoints.join(",")),
            ),
            "estimate_route",
        );
        let legs = array_of(&answer, "legs");
        for (index, (name, _)) in chunk.iter().enumerate() {
            let out = legs
                .get(2 * index)
                .unwrap_or_else(|| panic!("round {round}, {how}: no leg out to {name}"));
            assert_eq!(
                number(out, "ms"),
                summary_ms(name),
                "round {round}, {how}: estimate_route walks to {name} at another figure than the chip's"
            );
        }
    }
    goals.len()
}

/// Four walls six voxels above the ground they stand on, across the whole map
/// but four rows at alternate ends, at x `x0 + 4`, `+ 6`, `+ 8` and `+ 10`
/// (`crates/sim/tests/pathing.rs`'s serpentine, one fold, on the natural
/// terrain rather than a levelled strip, so the ground either side stays
/// joined to the map), filed into `world`'s edit queue and settled. A walk
/// from west of the walls to east of them winds up and down the corridors
/// between them: more than `MAX_ROUTE_NODES` columns, so the router serves it
/// in more than one route.
fn walls_in(world: &mut pharmakos_sim::world::World, x0: i32) {
    use pharmakos_sim::voxels::{Material, VoxelEdit};
    let mut enc = pharmakos_sim::encoding::Enc::with_capacity(64 * 1024);
    let size = world.surface().size();
    for offset in [4, 6, 8, 10] {
        let x = x0 + offset;
        for y in 0..size[1] {
            let far_gap = y >= size[1] - 4;
            let near_gap = y < 4;
            let open = if (offset - 4) % 4 == 0 {
                far_gap
            } else {
                near_gap
            };
            if open {
                continue;
            }
            let ground = world.voxels().top_solid_z(x, y).unwrap_or(-1);
            for z in ground + 1..=ground + 6 {
                let edit = VoxelEdit::Set {
                    at: [x, y, z],
                    material: Material::STONE,
                    by: None,
                };
                if world.request_voxel_edit(edit).is_err() {
                    world.step(&mut enc).expect("the edits settle");
                    world
                        .request_voxel_edit(edit)
                        .expect("an emptied queue takes the edit");
                }
            }
        }
    }
    while world.queued_voxel_edits() > 0 {
        world.step(&mut enc).expect("the edits settle");
    }
}

/// **One figure for a walk the router serves in two routes** (S3's `p3fix`,
/// fix pass 5, item 3): a two-seat casual match whose world has four walls
/// in a serpentine beside seat 0's commander ([`walls_in`]), resumed into a
/// fresh host through the save's one restore. A feature beyond the walls is
/// reached by a walk that chains a partial route (asserted from the sim's own
/// `walk_cost` on the hosted world), and for every feature seat 0
/// reaches, `get_map_summary`'s `travel_ms`, `resolve_refs`' candidate time
/// and `estimate_route`'s leg are one figure ([`assert_one_measure`]).
/// Before this test a
/// mutant that dropped the partial routes from the chip and `resolve_refs`
/// alone survived every gateway and gamectl test: no committed case had a
/// walk needing two routes.
#[test]
fn a_walk_in_two_routes_reads_one_figure_everywhere() {
    use pharmakos_sim::pathing::estimate::walk_cost;
    let config = pharmakos_sim::world::WorldConfig {
        match_seed: support::SEED,
        seats: 2,
        rules: support::rules(),
        match_settings: pharmakos_sim::runner::MatchSettings {
            segment_lengths_ms: vec![SEGMENT_MS],
            round_limit: 3,
        },
    };
    let mut world = pharmakos_sim::world::World::new(&config).expect("the world generates");
    let commander = world.commander_of(SeatId::new(0)).expect("a commander");
    let [sx, sy, _] = {
        let row = world.units().row_of(commander).expect("in the table");
        let at = world
            .units()
            .positions()
            .get(row.index())
            .copied()
            .expect("a place");
        [at[0].floor_voxels(), at[1].floor_voxels(), 0]
    };
    // The walls just east of the commander, or just west of it when the map
    // has more room that side (mirrored by walling from x0 = sx - 16).
    let width = world.surface().size()[0];
    let x0 = if sx + 20 < width - sx {
        sx + 2
    } else {
        sx - 16
    };
    walls_in(&mut world, x0);
    let bytes = pharmakos_sim::snapshot::Snapshot::capture(&world)
        .expect("a world between ticks")
        .to_bytes()
        .expect("it encodes");
    let mut host = pharmakos_gateway::host::Host::open(&config, None).expect("a match");
    host.resume(&bytes).expect("the walled world resumes");
    let mut surface = Surface::new(
        MATCH,
        support::SEED,
        support::rules(),
        FogPolicy::casual(),
        &[SeatId::new(0), SeatId::new(1)],
    )
    .expect("a match id");
    surface.attach(host).expect("attached");
    surface
        .set_phase_remaining_ms(Ms::new(FIRST_LULL_MS))
        .expect("a custom match takes a countdown");
    surface.open_lull().expect("the opening Lull");

    // The sim's own walks on the hosted world: at least one chains.
    let chained = {
        let world = surface.host().expect("hosted").world();
        let origin = world
            .surface()
            .node_of(sx, sy)
            .expect("the commander's column");
        let ground = world.ground();
        let mut scratch = pharmakos_sim::pathing::Scratch::for_map(
            world.surface(),
            world.rules().hpa_cluster_voxels(),
        )
        .expect("a scratch");
        let walks: Vec<_> = (0..world.features().features().len())
            .filter_map(|index| ground.measure_to(index, origin))
            .filter_map(|goal| {
                walk_cost(
                    world.surface(),
                    world.clusters(),
                    &mut scratch,
                    (origin, goal),
                )
                .ok()
            })
            .collect();
        walks.iter().filter(|walk| walk.partials >= 1).count()
    };
    assert!(
        chained >= 1,
        "a feature's walk needs a second route, or this tests nothing"
    );
    let compared = assert_one_measure(&mut surface, 1, "the serpentine");
    println!(
        "::notice::the serpentine: {compared} features compared, {chained} of them walked in more \
         than one route"
    );
}

/// The match of [`every_lull_reads_and_plays_on_as_its_save_does`], resumed
/// from `saved` on a fresh surface and host.
fn resumed_from(saved: &pharmakos_gateway::save::SavedMatch) -> Surface {
    let host = pharmakos_gateway::host::Host::open(
        &pharmakos_sim::world::WorldConfig {
            match_seed: support::SEED,
            seats: 2,
            rules: support::rules(),
            match_settings: pharmakos_sim::runner::MatchSettings {
                segment_lengths_ms: vec![SHORT_SEGMENT_MS],
                round_limit: SHORT_ROUNDS,
            },
        },
        None,
    )
    .expect("a match");
    Surface::new(
        MATCH,
        support::SEED,
        support::rules(),
        FogPolicy::fogged(),
        &[SeatId::new(0), SeatId::new(1)],
    )
    .expect("a surface")
    .resume(
        host,
        saved,
        pharmakos_gateway::save::Continuation {
            generation: 1,
            last_tick: 0,
        },
    )
    .expect("the Lull resumes")
}

/// The voxels of a wall five high round seat 0's commander: the eight
/// columns about it, from one above each column's top.
fn wall_round_the_commander(surface: &Surface) -> Vec<[i32; 3]> {
    let [x, y, _] = support::commander_voxel(surface, 0);
    let world = surface.host().expect("hosted").world();
    let mut wall = Vec::new();
    for dy in -1..=1_i32 {
        for dx in -1..=1_i32 {
            if (dx, dy) == (0, 0) {
                continue;
            }
            let top = world
                .voxels()
                .top_solid_z(x + dx, y + dy)
                .expect("a column with a floor");
            for z in top + 1..=top + 5 {
                wall.push([x + dx, y + dy, z]);
            }
        }
    }
    wall
}

/// Whether seat 0's commander is sealed in: the router holds it parked, with
/// no way to its destination, and counts it in `sealed_units`.
fn commander_sealed(surface: &Surface) -> bool {
    use pharmakos_sim::pathing::router::WalkState;
    let world = surface.host().expect("hosted").world();
    let commander = world
        .commander_of(SeatId::new(0))
        .expect("an occupied seat has a commander");
    let row = world
        .units()
        .ids()
        .iter()
        .position(|id| *id == commander.raw())
        .and_then(|row| u32::try_from(row).ok())
        .expect("the commander is in the unit table");
    world.router().state(row) == WalkState::Sealed && world.sealed_units() >= 1
}

/// File one `Set` of `material` per voxel of `wall` through the host.
fn file_wall(surface: &mut Surface, wall: &[[i32; 3]], material: pharmakos_sim::voxels::Material) {
    use pharmakos_sim::voxels::VoxelEdit;
    for &at in wall {
        surface
            .host_mut()
            .expect("hosted")
            .file_voxel_edit(VoxelEdit::Set {
                at,
                material,
                by: None,
            })
            .expect("the queue takes a wall voxel");
    }
}

/// Play one Push, filing `craters` on its last tick, with `wall` taken down
/// on the same tick, and return its chain: each tick and its state hash.
fn push_cratered_on_its_last_tick(
    surface: &mut Surface,
    craters: &[[i32; 3]],
    wall: &[[i32; 3]],
) -> Vec<(u32, u64)> {
    use pharmakos_sim::voxels::VoxelEdit;
    surface.begin_push().expect("the Push begins");
    let ticks = surface
        .host()
        .expect("hosted")
        .world()
        .match_state()
        .segment_ticks();
    let mut chain = Vec::new();
    for tick in 1..=ticks {
        if tick == ticks {
            // The wall's precondition: the commander it holds is sealed in
            // until the last tick takes the wall down.
            assert!(
                wall.is_empty() || commander_sealed(surface),
                "the wall holds seat 0's commander sealed in before its last tick"
            );
            for &centre in craters {
                surface
                    .host_mut()
                    .expect("hosted")
                    .file_voxel_edit(VoxelEdit::Crater {
                        centre,
                        radius: 3,
                        by: None,
                    })
                    .expect("the queue takes a crater");
            }
            file_wall(surface, wall, pharmakos_sim::voxels::Material::AIR);
        }
        let report = surface
            .step()
            .expect("a hosted match")
            .expect("the tick runs in the Push");
        chain.push((report.tick.raw(), report.hash));
    }
    assert_eq!(support::step(surface, 1), 0, "the Push has ended");
    chain
}

/// Every Lull of a hosted three-round match reads the graph its save
/// restores, and plays on to the chain its save plays on to (lane `settle`).
/// Each Push craters seat 0's way to its nearest vent on its last tick, so
/// every Lull after the first opens on a terrain edit. In round 1 seat 0's
/// commander, sealed to walk forty columns, is walled in for the whole Push and its
/// wall comes down on the last tick: the Voxels phase that takes it down
/// re-arms it, so at round 2's Lull it is waiting rather than sealed, saved
/// that way, and walks in round 2 live and resumed alike (a dropped
/// re-arm leaves it sealed in place). The graph is settled
/// in the phase that edits the terrain, so at every Lull the hosted world
/// that `get_map_summary`, `resolve_refs` and `estimate_route` read is the one
/// a resume from that Lull's save rebuilds: the answers are byte-equal, and
/// the next Push's chain is the same tick for tick. Before the lane the last
/// tick's marks waited for the next Push's Pathing phase: the live Lull read a
/// stale graph over a fresh surface and the resumed one a fresh graph, and
/// round 2's `get_map_summary` priced seat 0's walk to `seam_348_31` at
/// 13 400 ms live and 12 800 ms resumed.
///
/// At each of those Lulls, live and resumed, the four readers price one walk
/// ([`assert_one_measure`]; S3's `p3fix`, the gateway step): the chip's
/// `travel_ms`, `resolve_refs`' candidates and `estimate_route`'s legs, for
/// every feature seat 0 reaches, are the walk the router serves.
#[test]
fn every_lull_reads_and_plays_on_as_its_save_does() {
    let mut surface = support::hosted(2, SHORT_SEGMENT_MS, SHORT_ROUNDS);
    let (vent, craters) = nearest_vent_and_its_way(&mut surface);
    let walled_at = support::commander_voxel(&surface, 0);
    for round in 1..=SHORT_ROUNDS {
        let token = support::seat_token(&mut surface, 0);
        let mut left = LULL_MS;
        // Forty columns towards the middle of the map.
        let toward = if walled_at[0] > 192 { -40 } else { 40 };
        let east = support::walk_east(&surface, 0, toward, 4);
        let sealed = call_in_lull(
            &mut surface,
            &token,
            &mut left,
            "submit_plan",
            &format!(r#"{{"playbook_jsonc":{}}}"#, support::quote(&east)),
        );
        assert_eq!(
            result(&sealed, "submit_plan").get("accepted"),
            Some(&Json::Bool(true)),
            "round {round}: seat 0 seals its walk"
        );
        let live = graph_reads(&mut surface, &vent);
        let mut resumed = resumed_from(&surface.lull_save().expect("a Lull saves"));
        let again = graph_reads(&mut resumed, &vent);
        let compared = assert_one_measure(&mut surface, round, "live");
        assert_eq!(
            assert_one_measure(&mut resumed, round, "resumed"),
            compared,
            "round {round}: the resumed Lull reaches another set of features"
        );
        println!("::notice::round {round}: one measure over {compared} features, live and resumed");
        for ((method, live), again) in ["get_map_summary", "resolve_refs", "estimate_route"]
            .into_iter()
            .zip(&live)
            .zip(&again)
        {
            assert_eq!(
                pharmakos_proto::json::write(live),
                pharmakos_proto::json::write(again),
                "round {round}: {method} reads one graph in the live Lull and another after its save resumes"
            );
        }
        let wall = if round == 1 {
            wall_round_the_commander(&surface)
        } else {
            Vec::new()
        };
        file_wall(&mut surface, &wall, pharmakos_sim::voxels::Material::STONE);
        file_wall(&mut resumed, &wall, pharmakos_sim::voxels::Material::STONE);
        let played = push_cratered_on_its_last_tick(&mut surface, &craters, &wall);
        let replayed = push_cratered_on_its_last_tick(&mut resumed, &craters, &wall);
        assert_eq!(
            played, replayed,
            "round {round}: the Push after the Lull plays one chain live and another after its save resumes"
        );
        let [x, _, _] = support::commander_voxel(&surface, 0);
        if round == 1 {
            assert_eq!(
                x, walled_at[0],
                "round 1: the wall holds the commander in place"
            );
        } else {
            assert!(
                (x - walled_at[0]).abs() >= 5,
                "round {round}: the commander freed on round 1's last tick walks (at x {x}, walled at {})",
                walled_at[0]
            );
        }
        if round < SHORT_ROUNDS {
            surface.end_recap().expect("the recap ends");
            surface
                .set_phase_remaining_ms(Ms::new(LULL_MS))
                .expect("a custom match takes a countdown");
            surface.open_lull().expect("the next round's Lull");
        }
    }
}

// ---------------------------------------------------------------------------
// The map summary's travel times outside a Lull (S3's `mapw`; the owner's
// ruling of 2026-11-21, "Omit them in a Push", taken to every phase that is
// not a Lull as `estimate_route`'s door is)
// ---------------------------------------------------------------------------

/// A two-seat gateway on the golden seed with no match hosted yet: the lobby.
fn in_the_lobby() -> Surface {
    Surface::new(
        MATCH,
        support::SEED,
        support::rules(),
        FogPolicy::fogged(),
        &[SeatId::new(0), SeatId::new(1)],
    )
    .expect("a match id")
}

/// Host a one-round match on the golden seed in the lobby's `surface`: its
/// runner opens in round 1's Lull, which the attach takes.
fn host_one_round(surface: &mut Surface) {
    let host = pharmakos_gateway::host::Host::open(
        &pharmakos_sim::world::WorldConfig {
            match_seed: support::SEED,
            seats: 2,
            rules: support::rules(),
            match_settings: pharmakos_sim::runner::MatchSettings {
                segment_lengths_ms: vec![support::SEGMENT_MS],
                round_limit: 1,
            },
        },
        None,
    )
    .expect("a match");
    surface.attach(host).expect("attached");
}

/// Seat 0's `get_map_summary.features`, asked through the dispatcher with no
/// clock reported (outside a Lull the gateway's ticks are the sim's).
fn features_outside_a_lull(surface: &mut Surface) -> Vec<Json> {
    let token = support::seat_token(surface, 0);
    let answer = support::call(surface, &token, "get_map_summary", "{}");
    array_of(&result(&answer, "get_map_summary"), "features")
}

/// Every feature of `features` carries the seat's `covered` and `live` and
/// neither travel member: not a time, and not a `reachable: false` that would
/// read as "no route reaches it".
fn assert_travel_withheld(features: &[Json], phase: &str) {
    assert!(
        !features.is_empty(),
        "{phase}: the golden seed lays vents and seams"
    );
    for feature in features {
        for key in ["travel_ms", "reachable"] {
            assert!(
                feature.get(key).is_none(),
                "{phase}: `{key}` is withheld outside a Lull: {feature:?}"
            );
        }
        for key in ["covered", "live"] {
            assert!(
                feature.get(key).is_some(),
                "{phase}: `{key}` is still told: {feature:?}"
            );
        }
    }
}

/// Outside a Lull -- a Push, a recap, an ended match -- every feature of a
/// seat's `get_map_summary` carries neither `travel_ms` nor `reachable`: in a
/// Push the hosted world is the live one, unfogged, and a recap's is the
/// Push's end state, so a travel time there would tell the seat about
/// terrain it cannot see. The rest of the answer still comes. The lobby hosts
/// no match, so it has no map to summarise and says nothing about travel; a
/// hosted match opens in round 1's Lull.
#[test]
fn outside_a_lull_the_map_summary_withholds_every_travel_time() {
    let mut surface = in_the_lobby();
    let token = support::seat_token(&mut surface, 0);
    let in_lobby = support::call(&mut surface, &token, "get_map_summary", "{}");
    assert!(
        in_lobby.get("result").is_none(),
        "the lobby hosts no map: {in_lobby:?}"
    );
    host_one_round(&mut surface);
    assert_eq!(
        surface.time().phase,
        pharmakos_proto::gp::api::v1::status::Phase::Lull,
        "a hosted match opens in its first Lull"
    );

    surface
        .set_phase_remaining_ms(Ms::new(FIRST_LULL_MS))
        .expect("a custom match takes a countdown");
    surface.open_lull().expect("the opening Lull");
    let token = support::seat_token(&mut surface, 0);
    let mut left = FIRST_LULL_MS;
    let in_lull = array_of(
        &result(
            &call_in_lull(&mut surface, &token, &mut left, "get_map_summary", "{}"),
            "get_map_summary",
        ),
        "features",
    );
    assert!(
        in_lull
            .iter()
            .all(|feature| feature.get("reachable").is_some()),
        "the positive control: in a Lull every feature says whether it is reached"
    );

    assert!(surface.begin_push().expect("the Push begins"));
    let _ = support::step(&mut surface, 3);
    assert_travel_withheld(&features_outside_a_lull(&mut surface), "a Push");

    let _ = support::step(&mut surface, 1_000);
    assert_eq!(
        surface.time().phase,
        pharmakos_proto::gp::api::v1::status::Phase::Recap,
        "the Push has ended"
    );
    assert_travel_withheld(&features_outside_a_lull(&mut surface), "a recap");

    assert!(surface.end_recap().expect("the recap ends"));
    assert_eq!(
        surface.time().phase,
        pharmakos_proto::gp::api::v1::status::Phase::Ended,
        "a one-round match ends at its recap's close"
    );
    assert_travel_withheld(&features_outside_a_lull(&mut surface), "an ended match");
}

/// A match resumed from its Lull's save withholds the same members in the
/// Push that follows as the live match does: the two Pushes' answers are
/// byte for byte one answer.
#[test]
fn a_resumed_push_withholds_travel_as_the_live_one_does() {
    let mut surface = support::hosted(2, SHORT_SEGMENT_MS, SHORT_ROUNDS);
    let mut resumed = resumed_from(&surface.lull_save().expect("a Lull saves"));
    let mut answers = Vec::new();
    for surface in [&mut surface, &mut resumed] {
        assert!(surface.begin_push().expect("the Push begins"));
        let _ = support::step(surface, 3);
        let token = support::seat_token(surface, 0);
        let answer = result(
            &support::call(surface, &token, "get_map_summary", "{}"),
            "get_map_summary",
        );
        assert_travel_withheld(&array_of(&answer, "features"), "a Push");
        answers.push(pharmakos_proto::json::write(&Json::Object(match answer {
            Json::Object(fields) => fields
                .into_iter()
                .filter(|(key, _)| key != "_status")
                .collect(),
            other => panic!("an object, and it is {other:?}"),
        })));
    }
    assert_eq!(answers.first(), answers.get(1), "live and resumed");
}
