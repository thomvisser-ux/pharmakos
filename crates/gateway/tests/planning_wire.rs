// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Three findings T19's first pull request routed to the gateway (decisions-log
//! item 112 (3) and (5)): `get_draft`, the shape of `estimate_route`'s legs,
//! and a carried draft that outlived its round.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use pharmakos_gateway::fog::FogPolicy;
use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::token::Subject;
use pharmakos_proto::gp::api::v1::{EstimateRouteResponse, GetDraftResponse};
use pharmakos_proto::json::Json;
use pharmakos_sim::tables::SeatId;

use support::{
    LULL_MS, SEGMENT_MS, call, call_in_lull, code, core_beacon, hosted_as, quote, result,
    seat_token, text_of,
};

/// A result without its `_status` footer, which is the gateway's and not the
/// message's, so it can be decoded against the schema.
fn without_footer(value: &Json) -> Json {
    match value {
        Json::Object(entries) => Json::Object(
            entries
                .iter()
                .filter(|(key, _)| key != "_status")
                .cloned()
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Both of spec section 12's fog policies, as a match is made under each.
fn both_policies(match_id: &str) -> Vec<(&'static str, Surface)> {
    vec![
        (
            "fogged",
            hosted_as(match_id, FogPolicy::fogged(), 2, SEGMENT_MS, 3),
        ),
        (
            "casual",
            hosted_as(match_id, FogPolicy::casual(), 2, SEGMENT_MS, 3),
        ),
    ]
}

/// `get_draft` answers a seat its own draft, body and all, in the schema's
/// own `GetDraftResponse` shape -- under either fog policy, because a draft's
/// secrecy is not a question of fog.
#[test]
fn get_draft_answers_a_seat_its_own_draft() {
    for (policy, mut surface) in both_policies("m-t17-own-draft") {
        let token = seat_token(&mut surface, 0);
        let mut left = LULL_MS;
        let playbook = pharmakos_gateway::host::SAFE_PLAYBOOK;
        let _ = result(
            &call_in_lull(
                &mut surface,
                &token,
                &mut left,
                "save_draft",
                &format!(
                    r#"{{"playbook_jsonc":{},"label":"east plan","draft_id":"east"}}"#,
                    quote(playbook)
                ),
            ),
            "save_draft",
        );
        let answer = result(
            &call_in_lull(
                &mut surface,
                &token,
                &mut left,
                "get_draft",
                r#"{"draft_id":"east"}"#,
            ),
            "get_draft",
        );
        let decoded: GetDraftResponse =
            pharmakos_proto::json::decode_json(&without_footer(&answer))
                .unwrap_or_else(|error| panic!("{policy}: not a GetDraftResponse: {error}"));
        assert_eq!(
            decoded.playbook_jsonc, playbook,
            "{policy}: comments and all"
        );
        assert_eq!(decoded.label, "east plan", "{policy}");
        assert_eq!(decoded.round, 1, "{policy}");
    }
}

/// Another seat's draft id is `NOT_FOUND`, **word for word** the answer an id
/// nobody saved gets: the refusal never says whether another seat holds a
/// draft by that name. Under both fog policies.
#[test]
fn get_draft_never_reaches_another_seats_draft() {
    for (policy, mut surface) in both_policies("m-t17-other-draft") {
        let zero = seat_token(&mut surface, 0);
        let one = seat_token(&mut surface, 1);
        let mut left = LULL_MS;
        let _ = result(
            &call_in_lull(
                &mut surface,
                &zero,
                &mut left,
                "save_draft",
                &format!(
                    r#"{{"playbook_jsonc":{},"label":"secret","draft_id":"secret"}}"#,
                    quote(pharmakos_gateway::host::SAFE_PLAYBOOK)
                ),
            ),
            "save_draft",
        );
        let theirs = call_in_lull(
            &mut surface,
            &one,
            &mut left,
            "get_draft",
            r#"{"draft_id":"secret"}"#,
        );
        let nobodys = call_in_lull(
            &mut surface,
            &one,
            &mut left,
            "get_draft",
            r#"{"draft_id":"nothing-here"}"#,
        );
        assert_eq!(code(&theirs), "NOT_FOUND", "{policy}");
        assert_eq!(code(&nobodys), "NOT_FOUND", "{policy}");
        let message = |response: &Json| {
            response
                .get("error")
                .and_then(|error| error.get("message"))
                .cloned()
        };
        assert_eq!(
            message(&theirs),
            message(&nobodys),
            "{policy}: another seat's id and an unknown id are indistinguishable"
        );
        let text = format!("{theirs:?}");
        assert!(
            !text.contains("secret") && !text.contains("Safe playbook"),
            "{policy}: nothing of the other seat's draft came back: {text}"
        );

        // The spectator, the lobby and a Push get nothing either.
        let lobby = support::admin_token(&mut surface);
        let refused = call(
            &mut surface,
            &lobby,
            "get_draft",
            r#"{"draft_id":"secret"}"#,
        );
        assert_eq!(
            code(&refused),
            "FORBIDDEN_SCOPE",
            "{policy}: admin has no drafts"
        );
        surface.begin_push().expect("the Push begins");
        let closed = call(&mut surface, &zero, "get_draft", r#"{"draft_id":"secret"}"#);
        assert_eq!(
            code(&closed),
            "PHASE_CLOSED",
            "{policy}: planning closes in a Push"
        );
    }
}

/// `gp.api.v1.Leg.to` is a `gp.v1.Location`, so a leg's end is written
/// `{"voxel": {x, y, z}}` and not as a bare voxel. Pinned here, because the
/// walkthrough golden records summaries rather than fields and would not see
/// it: the answer decodes against the schema's own `EstimateRouteResponse`,
/// which a bare voxel does not.
#[test]
fn a_legs_end_is_written_as_the_location_the_schema_declares() {
    let mut surface = hosted_as("m-t17-leg", FogPolicy::fogged(), 2, SEGMENT_MS, 3);
    let token = seat_token(&mut surface, 0);
    let (_, [cx, cy, cz]) = core_beacon(&surface, 0);
    let [bx, by, bz] = support::commander_voxel(&surface, 0);
    let mut left = LULL_MS;
    let answer = result(
        &call_in_lull(
            &mut surface,
            &token,
            &mut left,
            "estimate_route",
            &format!(
                r#"{{"waypoints":[{{"voxel":{{"x":{bx},"y":{by},"z":{bz}}}}},{{"voxel":{{"x":{cx},"y":{cy},"z":{cz}}}}}]}}"#
            ),
        ),
        "estimate_route",
    );
    let legs = support::array_of(&answer, "legs");
    let to = legs
        .first()
        .and_then(|leg| leg.get("to"))
        .expect("a leg's end");
    let voxel = to.get("voxel").expect("the Location's `voxel` arm");
    assert_eq!(
        voxel.get("x"),
        Some(&Json::Number(cx.to_string())),
        "the leg ends at the second waypoint"
    );
    assert!(to.get("x").is_none(), "not a bare voxel: {to:?}");
    let decoded: EstimateRouteResponse =
        pharmakos_proto::json::decode_json(&without_footer(&answer))
            .unwrap_or_else(|error| panic!("not an EstimateRouteResponse: {error}"));
    let end = decoded
        .legs
        .first()
        .and_then(|leg| leg.to.as_ref())
        .expect("a leg with an end");
    assert!(
        matches!(
            end.place,
            Some(pharmakos_proto::gp::v1::location::Place::Voxel(ref at))
                if at.x == cx && at.y == cy && at.z == cz
        ),
        "{end:?}"
    );
}

/// Decisions-log item 112 (3): after a round the gateway filed the safe
/// playbook for, nothing is carried -- so last round's `carried` draft goes
/// rather than being offered as this round's.
#[test]
fn a_carried_draft_that_nothing_replaces_is_dropped() {
    let mut surface = hosted_as("m-t17-carried", FogPolicy::fogged(), 2, SEGMENT_MS, 3);
    let token = seat_token(&mut surface, 0);
    let mut left = LULL_MS;
    let submitted = support::walk_east(&surface, 0, 3, 0);
    let _ = result(
        &call_in_lull(
            &mut surface,
            &token,
            &mut left,
            "submit_plan",
            &format!(r#"{{"playbook_jsonc":{}}}"#, quote(&submitted)),
        ),
        "submit_plan",
    );
    let finish_round = |surface: &mut Surface| {
        surface.begin_push().expect("the Push begins");
        let _ = support::step(surface, 10_000);
        surface.end_recap().expect("the recap ends");
        surface.open_lull().expect("the next Lull");
    };
    finish_round(&mut surface);
    let own = |surface: &Surface| {
        surface
            .seat_state(Subject::Seat(SeatId::new(0)), SeatId::new(0))
            .expect("its own")
            .clone()
    };
    let round_two = own(&surface);
    assert_eq!(
        round_two.draft("carried").map(|draft| draft.round),
        Some(2),
        "round 1's playbook is carried into round 2"
    );
    assert!(round_two.continuity.is_some());

    // Round 2: the seat submits nothing, so the gateway files the safe
    // playbook, and round 3 has nothing of the seat's own to carry.
    finish_round(&mut surface);
    let round_three = own(&surface);
    assert_eq!(round_three.drafts.len(), 0, "{:?}", round_three.drafts);
    assert!(round_three.continuity.is_none());
    let token = seat_token(&mut surface, 0);
    let mut left = LULL_MS;
    let listed = result(
        &call_in_lull(&mut surface, &token, &mut left, "list_drafts", "{}"),
        "list_drafts",
    );
    assert!(support::array_of(&listed, "drafts").is_empty());
    let gone = call_in_lull(
        &mut surface,
        &token,
        &mut left,
        "get_draft",
        r#"{"draft_id":"carried"}"#,
    );
    assert_eq!(code(&gone), "NOT_FOUND");
    let _ = text_of(
        &result(
            &call_in_lull(&mut surface, &token, &mut left, "get_safe_plan", "{}"),
            "get_safe_plan",
        ),
        "playbook_jsonc",
    );
}
