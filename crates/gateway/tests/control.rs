// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Match control, and the two determinism tests that moved here from T16.
//!
//! # Why the hash tests are here and not in the client
//!
//! They prove that no wall clock and no presentation concern leaked into the
//! sim: however the client chops the Push up, the match plays out identically.
//! T16 could only have proved it by putting a state hash on the wire, and a
//! state hash handed to the human seat's own process is the whole world in
//! eight bytes (decisions-log item 107 (3)). Here the chain is read **in
//! process**, off `TickReport::hash`, which is what item 106 (3) defines
//! parity as anyway.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use pharmakos_gateway::scopes::{ADMIN_METHODS, Scope, ScopeSet, method_wire_name, required};
use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::surface::control::{MAX_ADVANCE_MS, MAX_CLOCK_STEP_MS};
use pharmakos_gateway::token::{Subject, Token};
use pharmakos_proto::gp::api::v1::status::Phase;
use pharmakos_proto::json::Json;
use pharmakos_sim::math::quantity::Tick;
use pharmakos_sim::tables::SeatId;
use std::collections::BTreeSet;

use support::{LULL_MS, MATCH, SEGMENT_MS, admin_token, call, code, hosted, result, seat_token};

// ---------------------------------------------------------------------------
// Determinism, read in process
// ---------------------------------------------------------------------------

/// The per-tick state-hash chain of a whole segment, stepped one tick at a
/// time through the surface.
///
/// This is the path `gamectl scenario run` takes and the chain the
/// determinism goldens are of: read **in process**, off `TickReport::hash`,
/// and never put on a wire.
fn chain_in_process() -> Vec<u64> {
    let mut surface = hosted(2, SEGMENT_MS, 2);
    assert!(surface.begin_push().expect("a match"), "the Push");
    let mut chain: Vec<u64> = Vec::new();
    while let Some(report) = surface.step().expect("a match") {
        chain.push(report.hash);
    }
    chain
}

/// Drive a whole segment entirely through `advance_push`, in steps of `ms`.
///
/// Returns the state the segment ended on and how much game time the calls
/// reported running. The **chain** cannot come back this way, and that is the
/// design rather than a limitation of the test: the answer carries
/// `advanced_ms` and nothing else, so a client -- and this test -- learns the
/// pace it asked for and never the world's hash.
fn wire_run(ms: i32) -> (u64, i32, u32) {
    let mut surface = hosted(2, SEGMENT_MS, 2);
    let admin = admin_token(&mut surface);
    let _ = result(&call(&mut surface, &admin, "end_lull", "{}"), "end_lull");
    let mut advanced = 0_i32;
    let mut calls = 0_u32;
    while surface.time().phase == Phase::Push {
        let response = call(
            &mut surface,
            &admin,
            "advance_push",
            &format!(r#"{{"ms":{ms}}}"#),
        );
        let answer = result(&response, "advance_push");
        calls = calls.saturating_add(1);
        advanced = advanced.saturating_add(advanced_ms(&answer));
        assert!(calls < 200, "an advance that never reaches segment end");
    }
    let hash = surface.host().expect("a match").world().state_hash();
    (hash, advanced, calls)
}

/// The `advanced_ms` of one answer, and a check that the answer carries
/// nothing else.
fn advanced_ms(answer: &Json) -> i32 {
    let Json::Object(entries) = answer else {
        panic!("an object");
    };
    let keys: Vec<&str> = entries.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(
        keys,
        vec!["advanced_ms", "_status"],
        "`advance_push` answers how much game time it ran and the footer every result \
         carries. A tick or a state hash here would be the first time either reached the \
         wire, to a token held by the human seat's own process (item 107 (3))."
    );
    match answer.get("advanced_ms") {
        Some(Json::Number(lexeme)) => lexeme.parse::<i32>().expect("a whole number"),
        other => panic!("`advanced_ms` is a number, and it is {other:?}"),
    }
}

/// However the Push is chopped up, the match plays out identically.
///
/// The batch sizes are the ones a client actually produces: a 50 ms frame at
/// 1x, 100 ms at 2x, 200 ms at 4x, and the 60 000 ms step a skip repeats.
/// A wall clock or a presentation concern that had leaked into the sim would
/// show here as a terminal state that moved with the batch size.
#[test]
fn advancing_in_steps_of_50_100_200_and_60000_ms_yields_the_same_chain() {
    let reference = chain_in_process();
    assert_eq!(
        reference.len(),
        20,
        "1 000 ms at 50 ms a tick, read off TickReport::hash in process"
    );
    let terminal = reference.last().copied().expect("twenty ticks");
    assert_eq!(
        reference.iter().collect::<BTreeSet<&u64>>().len(),
        reference.len(),
        "twenty different states, so a chain that stopped moving would be visible"
    );

    for ms in [50_i32, 100, 200, MAX_ADVANCE_MS] {
        let (hash, advanced, calls) = wire_run(ms);
        assert_eq!(
            hash, terminal,
            "a Push advanced in {ms} ms steps ended on a different state"
        );
        assert_eq!(
            advanced, 1_000,
            "and the calls reported exactly the segment's own game time back ({calls} calls)"
        );
    }
}

/// Skipping to segment end leaves the world where playing it out does.
#[test]
fn skipping_to_segment_end_yields_the_same_terminal_hash_as_playing_it_out() {
    let played = chain_in_process();
    let terminal = played.last().copied().expect("twenty ticks");

    let (hash, advanced, calls) = wire_run(MAX_ADVANCE_MS);
    assert_eq!(
        hash, terminal,
        "a segment skipped and a segment watched end on the same state"
    );
    assert_eq!(advanced, 1_000, "the call stopped at segment end");
    assert_eq!(calls, 1, "one 60 s step covers a 1 s segment");
}

// ---------------------------------------------------------------------------
// Scope, phase and range
// ---------------------------------------------------------------------------

/// Every control method needs `admin`, read off the descriptor set rather than
/// off the handler.
#[test]
fn every_control_handler_needs_the_admin_scope_in_the_schema() {
    assert_eq!(
        ADMIN_METHODS.len(),
        4,
        "end_lull, advance_push, end_recap, report_host_clock"
    );
    for method in ADMIN_METHODS {
        assert_eq!(
            required(*method),
            Some(Scope::Admin),
            "{} is served by surface/control.rs and must need `admin`",
            method_wire_name(*method)
        );
    }
}

/// Only an admin token may drive the match.
#[test]
fn only_an_admin_token_can_end_a_lull_advance_a_push_or_end_a_recap() {
    let mut surface = hosted(2, SEGMENT_MS, 2);
    let seat = seat_token(&mut surface, 0);
    let (spectator, _) = surface
        .tokens()
        .mint(
            Subject::Spectator,
            MATCH,
            ScopeSet::of(&[Scope::Observe, Scope::SpectateNofog]),
            Tick::ZERO,
        )
        .expect("minted");

    for token in [&seat, &spectator] {
        for (method, params) in [
            ("end_lull", "{}"),
            ("advance_push", r#"{"ms":50}"#),
            ("end_recap", "{}"),
            ("report_host_clock", r#"{"elapsed_ms":50,"remaining_ms":0}"#),
        ] {
            let response = call(&mut surface, token, method, params);
            assert_eq!(
                code(&response),
                "FORBIDDEN_SCOPE",
                "`{method}` reached a token that is not the lobby's"
            );
        }
    }
    assert_eq!(surface.time().phase, Phase::Lull, "and nothing moved");

    // A seat token can never hold `admin` at all, which is the other end of
    // the same rule and is enforced at the mint.
    let refused = surface
        .tokens()
        .mint(
            Subject::Seat(SeatId::new(0)),
            MATCH,
            ScopeSet::of(&[Scope::Admin]),
            Tick::ZERO,
        )
        .expect_err("refused");
    assert_eq!(refused.code, pharmakos_gateway::error::Code::ForbiddenScope);
}

/// Out of phase is `PHASE_CLOSED`, and the match does not move.
#[test]
fn a_control_method_out_of_phase_is_phase_closed_and_changes_nothing() {
    let mut surface = hosted(2, SEGMENT_MS, 2);
    let admin = admin_token(&mut surface);

    // In a Lull: only `end_lull` and `report_host_clock` are open.
    for (method, params) in [("advance_push", r#"{"ms":50}"#), ("end_recap", "{}")] {
        let before = surface.time();
        let response = call(&mut surface, &admin, method, params);
        assert_eq!(code(&response), "PHASE_CLOSED", "`{method}` in a Lull");
        assert_eq!(surface.time().phase, before.phase, "`{method}` moved it");
    }

    // In a Push: only `advance_push`.
    let _ = result(&call(&mut surface, &admin, "end_lull", "{}"), "end_lull");
    for (method, params) in [
        ("end_lull", "{}"),
        ("end_recap", "{}"),
        ("report_host_clock", r#"{"elapsed_ms":50,"remaining_ms":0}"#),
    ] {
        let before = surface.time();
        let response = call(&mut surface, &admin, method, params);
        assert_eq!(code(&response), "PHASE_CLOSED", "`{method}` in a Push");
        assert_eq!(surface.time().tick, before.tick, "`{method}` moved it");
    }

    // In a recap: only `end_recap` and `report_host_clock`.
    let _ = result(
        &call(&mut surface, &admin, "advance_push", r#"{"ms":60000}"#),
        "advance_push",
    );
    for (method, params) in [("end_lull", "{}"), ("advance_push", r#"{"ms":50}"#)] {
        let response = call(&mut surface, &admin, method, params);
        assert_eq!(code(&response), "PHASE_CLOSED", "`{method}` in a recap");
    }
    let _ = result(&call(&mut surface, &admin, "end_recap", "{}"), "end_recap");
    assert_eq!(surface.time().round, 2, "and the next round opened");
}

/// An `ms` outside the range is refused, and refusing is not clamping.
#[test]
fn an_advance_outside_1_to_60000_ms_is_refused_not_clamped() {
    let mut surface = hosted(2, SEGMENT_MS, 2);
    let admin = admin_token(&mut surface);
    let _ = result(&call(&mut surface, &admin, "end_lull", "{}"), "end_lull");

    for asked in ["0", "-1", "60001", "2147483647"] {
        let before = surface.time().tick;
        let response = call(
            &mut surface,
            &admin,
            "advance_push",
            &format!(r#"{{"ms":{asked}}}"#),
        );
        assert_eq!(code(&response), "INVALID_ARGUMENT", "`ms` of {asked}");
        assert_eq!(
            surface.time().tick,
            before,
            "a refused advance ran no tick: {asked}"
        );
    }
    let response = call(&mut surface, &admin, "advance_push", "{}");
    assert_eq!(code(&response), "INVALID_ARGUMENT", "a missing `ms`");

    // And a request too small to fill a tick is a normal answer of zero.
    let answer = result(
        &call(&mut surface, &admin, "advance_push", r#"{"ms":49}"#),
        "advance_push",
    );
    assert_eq!(
        answer.get("advanced_ms"),
        Some(&Json::Number(String::from("0"))),
        "floored, and not an error: the client carries the remainder"
    );
    assert_eq!(MAX_ADVANCE_MS, 60_000);
}

/// A clock that runs backwards or jumps is refused, not clamped.
#[test]
fn a_host_clock_that_runs_backwards_or_jumps_is_refused_not_clamped() {
    let mut surface = hosted(2, SEGMENT_MS, 2);
    let admin = admin_token(&mut surface);
    let clock = |elapsed: i32, remaining: i32| {
        format!(r#"{{"elapsed_ms":{elapsed},"remaining_ms":{remaining}}}"#)
    };

    let _ = result(
        &call(
            &mut surface,
            &admin,
            "report_host_clock",
            &clock(10_000, 170_000),
        ),
        "report_host_clock",
    );
    let after = surface.time().tick;

    let response = call(
        &mut surface,
        &admin,
        "report_host_clock",
        &clock(9_000, 171_000),
    );
    assert_eq!(code(&response), "INVALID_ARGUMENT", "backwards");
    let response = call(
        &mut surface,
        &admin,
        "report_host_clock",
        &clock(
            10_000i32
                .saturating_add(MAX_CLOCK_STEP_MS)
                .saturating_add(1),
            0,
        ),
    );
    assert_eq!(code(&response), "INVALID_ARGUMENT", "a jump");
    let response = call(&mut surface, &admin, "report_host_clock", &clock(-1, 0));
    assert_eq!(code(&response), "INVALID_ARGUMENT", "negative");
    let response = call(&mut surface, &admin, "report_host_clock", "{}");
    assert_eq!(code(&response), "INVALID_ARGUMENT", "a missing clock");

    assert_eq!(
        surface.time().tick,
        after,
        "a refused report moved nothing: the limiter's clock is client-attested and \
         therefore bounded rather than trusted"
    );

    // A step of exactly the bound is fine, which is what "report in steps"
    // means for a client that stalled.
    let _ = result(
        &call(
            &mut surface,
            &admin,
            "report_host_clock",
            &clock(10_000i32.saturating_add(MAX_CLOCK_STEP_MS), 0),
        ),
        "report_host_clock",
    );
    assert!(surface.time().tick > after);
}

/// The rate budget keeps moving in a recap and after match end, because the
/// host's clock does.
#[test]
fn the_host_clock_moves_the_rate_budget_in_a_recap_and_after_match_end() {
    // Under the DEFAULT limits: eight calls a tick is what a real client has.
    let mut surface = hosted(2, SEGMENT_MS, 1);
    let admin = admin_token(&mut surface);
    let seat = seat_token(&mut surface, 0);
    let _ = result(&call(&mut surface, &admin, "end_lull", "{}"), "end_lull");
    let _ = result(
        &call(&mut surface, &admin, "advance_push", r#"{"ms":60000}"#),
        "advance_push",
    );
    assert_eq!(phase_name(&surface), "recap");

    // Spend the seat's whole per-tick budget in the recap.
    for index in 0..8 {
        let response = call(&mut surface, &seat, "get_status", "{}");
        assert!(
            response.get("result").is_some(),
            "call {index} of a token's per-tick budget"
        );
    }
    assert_eq!(
        code(&call(&mut surface, &seat, "get_status", "{}")),
        "RATE_LIMITED",
        "and the ninth is refused, because a recap spends no sim tick"
    );

    // The lobby reports its clock; the tick moves and the budget refills.
    let _ = result(
        &call(
            &mut surface,
            &admin,
            "report_host_clock",
            r#"{"elapsed_ms":1000,"remaining_ms":0}"#,
        ),
        "report_host_clock",
    );
    assert!(
        call(&mut surface, &seat, "get_status", "{}")
            .get("result")
            .is_some(),
        "a recap with a reported clock is a recap a client can still read"
    );

    // And after match end, which is where the full-map unlock lands.
    let _ = result(&call(&mut surface, &admin, "end_recap", "{}"), "end_recap");
    assert_eq!(phase_name(&surface), "ended");
    for _ in 0..8 {
        let _ = call(&mut surface, &seat, "get_status", "{}");
    }
    assert_eq!(
        code(&call(&mut surface, &seat, "get_status", "{}")),
        "RATE_LIMITED"
    );
    let _ = result(
        &call(
            &mut surface,
            &admin,
            "report_host_clock",
            r#"{"elapsed_ms":1000,"remaining_ms":0}"#,
        ),
        "report_host_clock",
    );
    assert!(
        call(&mut surface, &seat, "get_status", "{}")
            .get("result")
            .is_some(),
        "an ended match is where a seat reads the unlocked map, so its budget has to move"
    );
}

/// `all_ready` reaches the lobby, and nothing else about a seat does.
#[test]
fn all_ready_reaches_the_admin_and_nothing_else_about_a_seat_does() {
    let mut surface = hosted(2, SEGMENT_MS, 2);
    let admin = admin_token(&mut surface);
    let first = seat_token(&mut surface, 0);
    let second = seat_token(&mut surface, 1);
    let clock = |elapsed: i32| {
        format!(
            r#"{{"elapsed_ms":{elapsed},"remaining_ms":{}}}"#,
            LULL_MS.saturating_sub(elapsed)
        )
    };

    let answer = result(
        &call(&mut surface, &admin, "report_host_clock", &clock(1_000)),
        "report_host_clock",
    );
    assert_eq!(answer.get("all_ready"), Some(&Json::Bool(false)));
    let Json::Object(entries) = &answer else {
        panic!("an object");
    };
    let keys: Vec<&str> = entries.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(
        keys,
        vec!["all_ready", "_status"],
        "one aggregate bit and the footer every result carries: no seat is named, and \
         nothing of a seat's plan, draft or notebook is reported"
    );

    let _ = result(
        &call(&mut surface, &first, "set_ready", r#"{"ready":true}"#),
        "set_ready",
    );
    let answer = result(
        &call(&mut surface, &admin, "report_host_clock", &clock(2_000)),
        "report_host_clock",
    );
    assert_eq!(
        answer.get("all_ready"),
        Some(&Json::Bool(false)),
        "one of two seats"
    );

    let _ = result(
        &call(&mut surface, &second, "set_ready", r#"{"ready":true}"#),
        "set_ready",
    );
    let answer = result(
        &call(&mut surface, &admin, "report_host_clock", &clock(3_000)),
        "report_host_clock",
    );
    assert_eq!(answer.get("all_ready"), Some(&Json::Bool(true)));

    // And `admin` still reads nothing of either seat's own state.
    for method in ["list_drafts", "get_briefing", "get_safe_plan"] {
        let response = call(&mut surface, &admin, method, "{}");
        assert_ne!(
            code(&response),
            "<no error>",
            "`{method}` answered the lobby: admin never reads a seat's knowledge"
        );
    }
}

/// The phase the surface is in, as the wire spells it.
fn phase_name(surface: &Surface) -> String {
    pharmakos_gateway::time::phase_wire_name(surface.time().phase)
}

/// A token is never rendered into anything this suite writes.
#[test]
fn a_control_suite_never_prints_a_token() {
    let mut surface = hosted(2, SEGMENT_MS, 2);
    let admin: Token = admin_token(&mut surface);
    assert_eq!(format!("{admin:?}"), "Token(<redacted>)");
}
