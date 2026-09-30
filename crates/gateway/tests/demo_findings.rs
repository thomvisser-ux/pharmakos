// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The gateway's share of the skeleton demo's findings (item 126 (3); S1's
//! plan, the `fixs` lane).
//!
//! * **F5, a timeout's filing.** A seat that sealed nothing reads, in its own
//!   feed, that the Lull ran out and the safe playbook was filed for it
//!   (decision 8, ruled by item 128); another seat reads nothing of it.
//! * **`estimate_route`'s selector leg.** A `nearest` leg is estimated to the
//!   beacon a decision would walk to, resolved over the frozen snapshot by the
//!   sim's own selector catalogue, not to the seat's first beacon.
//!
//! The two wording findings that are a pure function of one event, "1 route
//! step" and the fallback's posture by name, are pinned beside the templates
//! in `src/strings.rs`.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use pharmakos_gateway::surface::{SAFE_PLAYBOOK_FILED_KIND, Surface};
use pharmakos_gateway::token::Token;
use pharmakos_proto::gp::api::v1::status::Phase;
use pharmakos_proto::json::Json;
use support::{
    LULL_MS, array_of, call_in_lull, core_beacon, hosted, quote, result, seat_token, step, text_of,
};

/// Every `(kind, text)` on one viewer's page of the current segment's feed.
fn feed(surface: &mut Surface, token: &Token, left: &mut i32) -> Vec<(String, String)> {
    let response = call_in_lull(
        surface,
        token,
        left,
        "get_segment_feed",
        "{\"detail\":\"full\",\"limit\":256}",
    );
    array_of(&result(&response, "get_segment_feed"), "events")
        .iter()
        .map(|event| (text_of(event, "kind"), text_of(event, "text")))
        .collect()
}

#[test]
fn a_timeout_filing_is_named_in_the_seats_own_feed() {
    let mut surface = hosted(2, 1_000, 3);
    let mut left = LULL_MS;
    let seat0 = seat_token(&mut surface, 0);
    let seat1 = seat_token(&mut surface, 1);
    // Seat 1 seals its own playbook; seat 0 seals nothing, so the Lull runs
    // out on it and the gateway files the safe playbook.
    let own = support::walk_east(&surface, 1, 2, 0);
    let submitted = call_in_lull(
        &mut surface,
        &seat1,
        &mut left,
        "submit_plan",
        &format!("{{\"playbook_jsonc\":{}}}", quote(&own)),
    );
    assert_eq!(
        result(&submitted, "submit_plan").get("accepted"),
        Some(&Json::Bool(true))
    );
    surface.begin_push().expect("the Push begins");
    let _ = step(&mut surface, 2);

    let filed = pharmakos_gateway::strings::SAFE_PLAYBOOK_FILED;
    let mine = feed(&mut surface, &seat0, &mut left);
    assert_eq!(
        mine.first(),
        Some(&(SAFE_PLAYBOOK_FILED_KIND.to_owned(), filed.to_owned())),
        "seat 0's feed opens by saying its playbook was filed for it: {mine:?}"
    );
    assert_eq!(
        filed,
        "The Lull ran out, so the safe playbook was filed for you."
    );
    let sealed = mine
        .iter()
        .find(|(kind, _)| kind == "plan_sealed")
        .expect("and the seal itself follows");
    assert!(sealed.1.starts_with("The playbook of seat 0 was sealed: "));

    let theirs = feed(&mut surface, &seat1, &mut left);
    assert!(
        theirs
            .iter()
            .all(|(kind, _)| kind != SAFE_PLAYBOOK_FILED_KIND),
        "seat 1 sealed its own and is told nothing of seat 0's filing: {theirs:?}"
    );
}

/// Walk eight voxels west of seat 0's core, deploy a beacon there, then hold:
/// the committed `deploy-and-visit` scenario's first two steps, on the same
/// map.
const DEPLOY_WEST: &str = r#"{"schema_version": {"major": 1},
 "meta": {"title": "West", "author_kind": "HUMAN"},
 "kind": "PLAYBOOK",
 "declarative": {"route": [
   {"label": "to_site", "timeout_ms": 30000,
    "move": {"to": {"voxel": {"x": 350, "y": 22, "z": 36}}, "pace": "DIRECT"},
    "on_fail": {"action": "ABORT_ROUTE"}},
   {"label": "place_west", "timeout_ms": 30000,
    "place_beacon": {"at": {"voxel": {"x": 350, "y": 22, "z": 36}},
      "initial": {"mandate": {"roe": "RETURN_FIRE", "build": {}}}},
    "on_fail": {"action": "ABORT_ROUTE"}}
 ]},
 "on_death": {"on_respawn": "CONTINUE"},
 "fallback": {"hold": {"at": {"voxel": {"x": 350, "y": 22, "z": 36}}}}}"#;

#[test]
fn estimate_route_sends_a_nearest_leg_to_the_nearest_beacon() {
    let mut surface = hosted(2, 40_000, 3);
    let mut left = LULL_MS;
    let token = seat_token(&mut surface, 0);
    let submitted = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "submit_plan",
        &format!("{{\"playbook_jsonc\":{}}}", quote(DEPLOY_WEST)),
    );
    assert_eq!(
        result(&submitted, "submit_plan").get("accepted"),
        Some(&Json::Bool(true)),
        "{submitted:?}"
    );
    surface.begin_push().expect("the Push begins");
    let _ = step(&mut surface, 1_000);
    assert!(surface.end_recap().expect("the recap ends"));
    assert_eq!(surface.time().phase, Phase::Lull);
    surface.open_lull().expect("the next Lull");
    left = LULL_MS;

    let (core, core_at) = core_beacon(&surface, 0);
    let placed = support::beacons_of(&surface, 0)
        .into_iter()
        .find(|id| *id != core)
        .expect("the deploy placed a second beacon");
    let world = surface.host().expect("hosted").world();
    let row = usize::try_from(placed.raw()).unwrap();
    let placed_at = pharmakos_gateway::view::voxel_of(
        world
            .beacons()
            .positions()
            .get(row)
            .copied()
            .expect("a position"),
    );
    assert_ne!(
        [placed_at.x, placed_at.y, placed_at.z],
        core_at,
        "two beacons in two places"
    );

    // From where the commander stands, beside the new beacon, `nearest` is the
    // new beacon: it is the one a decision would walk to, and the old answer
    // (the seat's first beacon, its core) is eight voxels further.
    let [x, y, z] = support::commander_voxel(&surface, 0);
    let answer = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "estimate_route",
        &format!(
            r#"{{"waypoints": [{{"voxel": {{"x": {x}, "y": {y}, "z": {z}}}}}, {{"beacon_anchor": {{"nearest": {{}}}}}}, {{"beacon_anchor": {{"safest": {{}}}}}}]}}"#
        ),
    );
    let legs = array_of(&result(&answer, "estimate_route"), "legs");
    let to = |leg: &Json| -> [i32; 3] {
        let voxel = leg
            .get("to")
            .and_then(|to| to.get("voxel"))
            .expect("a leg ends at a voxel");
        ["x", "y", "z"].map(|axis| match voxel.get(axis) {
            Some(Json::Number(text)) => text.parse().expect("a whole voxel"),
            other => panic!("{axis} is a number, and it is {other:?}"),
        })
    };
    assert_eq!(
        to(legs.first().expect("a first leg")),
        [placed_at.x, placed_at.y, placed_at.z],
        "the nearest leg ends at the beacon nearest the commander, not at the seat's first"
    );
    assert_eq!(
        to(legs.get(1).expect("a second leg")),
        core_at,
        "and `safest` still ranks by condition, which puts the core (full hit points, and \
         the lower id on a tie) first"
    );
}
