// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The external seat: one seat of a headless match, played from outside the
//! game through [`pharmakos_gateway::serve::External`] (decisions-log item
//! 231).
//!
//! What is held here: the seat has a seat's rights and no others; the match
//! moves only by the host's hand, and only once the seat is ready; a match is
//! put down and taken up between any two calls; a request that is not one is
//! answered and never dropped; and the Lull's call budget is the one the
//! limits name.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules -- not \
              a helper function of an integration test"
)]

mod support;

use pharmakos_gateway::limit::{EXTERNAL_LIMITS, EXTERNAL_REFUSALS_IN_A_ROW};
use pharmakos_gateway::probation::Preset;
use pharmakos_gateway::rpc;
use pharmakos_gateway::serve::{Config, ConfigLine, External, NoOperators, Setup};
use pharmakos_proto::json::Json;
use std::path::Path;

use support::{SEED, data_root, rules_json};

/// A two-seat match of two rounds, each Push twenty ticks, with seat 0 played
/// from outside and seat 1 left to the safe playbook.
fn config(match_id: &str) -> Config {
    Config {
        match_id: match_id.to_owned(),
        seed: SEED,
        seats: 2,
        human_seat: Some(0),
        segment_lengths_ms: vec![1_000],
        round_limit: 2,
        preset: Preset::Custom,
    }
}

fn setup() -> Setup {
    Setup {
        rules_json: rules_json(),
        library: None,
        operators: Box::new(NoOperators),
    }
}

fn open(root: &Path, match_id: &str, resume: bool) -> External {
    External::open(
        setup(),
        root,
        &ConfigLine {
            config: config(match_id),
            resume,
        },
    )
    .expect("the match opens")
}

/// One call, as the program outside writes it.
fn ask(external: &mut External, method: &str, params: Json) -> Json {
    external.call(&rpc::request_text(
        &Json::Number(String::from("1")),
        method,
        params,
    ))
}

fn no_params() -> Json {
    Json::Object(Vec::new())
}

/// The `result` member, or a panic naming the refusal.
fn result(response: &Json, what: &str) -> Json {
    response
        .get("result")
        .cloned()
        .unwrap_or_else(|| panic!("{what} was refused: {:?}", response.get("error")))
}

/// The refusal's code name, or a panic showing the answer.
fn refusal(response: &Json, what: &str) -> String {
    match response
        .get("error")
        .and_then(|error| error.get("data"))
        .and_then(|data| data.get("code"))
    {
        Some(Json::String(code)) => code.clone(),
        _ => panic!("{what} was not refused with a code: {response:?}"),
    }
}

/// The phase and the round the `_status` footer of `get_status` names.
fn standing(external: &mut External) -> (String, String) {
    let answer = result(&ask(external, "get_status", no_params()), "get_status");
    let footer = answer
        .get("_status")
        .or_else(|| answer.get("status"))
        .expect("a status footer")
        .clone();
    let text = |key: &str| match footer.get(key) {
        Some(Json::String(text) | Json::Number(text)) => text.clone(),
        other => panic!("`{key}` in the footer is {other:?}"),
    };
    (text("phase"), text("round"))
}

fn audit_log(root: &Path, match_id: &str) -> String {
    let path = root.join("matches").join(match_id).join("audit.log");
    let text = std::fs::read_to_string(&path).expect("the audit log");
    assert!(!text.is_empty(), "an empty audit log at {}", path.display());
    text
}

#[test]
fn the_match_moves_only_when_the_seat_is_ready_and_then_to_its_end() {
    let root = data_root("external-rounds");
    let mut external = open(&root, "m-external-rounds", false);
    assert_eq!(
        standing(&mut external),
        (String::from("lull"), String::from("1"))
    );

    // Not ready: the host's hand moves nothing, however often it is asked.
    external.advance().expect("nothing to do");
    external.advance().expect("nothing to do");
    assert_eq!(
        standing(&mut external),
        (String::from("lull"), String::from("1"))
    );

    // Ready is answered where the match stands, and the Push is the host's.
    let ready = result(&ask(&mut external, "set_ready", no_params()), "set_ready");
    assert!(ready.get("status").is_some(), "{ready:?}");
    assert_eq!(
        standing(&mut external),
        (String::from("lull"), String::from("1"))
    );
    external.advance().expect("round 1 plays");
    assert_eq!(
        standing(&mut external),
        (String::from("lull"), String::from("2"))
    );
    assert!(!external.ended());

    // The recap of the round just played is told in the Lull that follows.
    let recap = result(&ask(&mut external, "get_recap", no_params()), "get_recap");
    assert!(recap.get("prose").is_some(), "{recap:?}");

    ask(&mut external, "set_ready", no_params());
    external
        .advance()
        .expect("round 2 plays, and the match ends");
    assert!(external.ended());
    assert_eq!(standing(&mut external).0, "ended");
    // An ended match stays ended, and a planning call says the phase is closed.
    external.advance().expect("nothing to do");
    let late = ask(&mut external, "set_ready", no_params());
    assert_eq!(refusal(&late, "set_ready after the end"), "PHASE_CLOSED");
    external.close().expect("the match is put down");

    let log = audit_log(&root, "m-external-rounds");
    assert!(log.contains("\texternal seat\tok"), "{log}");
    assert_eq!(log.matches("\tend lull\tok").count(), 2, "{log}");
    assert_eq!(log.matches("\tend recap\tok").count(), 2, "{log}");
    assert!(
        !log.contains("connections dropped"),
        "a host with no socket drops no connection and says nothing of any: {log}"
    );
}

#[test]
fn the_seat_has_a_seats_rights_and_no_others() {
    let root = data_root("external-rights");
    let mut external = open(&root, "m-external-rights", false);
    for (method, params) in [
        ("end_lull", no_params()),
        (
            "advance_push",
            Json::Object(vec![(
                String::from("ms"),
                Json::Number(String::from("1000")),
            )]),
        ),
        ("end_recap", no_params()),
        (
            "report_host_clock",
            Json::Object(vec![(
                String::from("elapsed_ms"),
                Json::Number(String::from("1000")),
            )]),
        ),
    ] {
        let answer = ask(&mut external, method, params);
        assert_eq!(refusal(&answer, method), "FORBIDDEN_SCOPE", "{method}");
    }
    // Nothing moved.
    assert_eq!(
        standing(&mut external),
        (String::from("lull"), String::from("1"))
    );
    // What a seat may do, it may: its own notebook.
    let saved = ask(
        &mut external,
        "save_notes",
        Json::Object(vec![(
            String::from("notes"),
            Json::String(String::from("hold the east vent")),
        )]),
    );
    result(&saved, "save_notes");
    external.close().expect("the match is put down");
}

#[test]
fn a_match_is_put_down_and_taken_up_between_two_calls() {
    let root = data_root("external-resume");
    let mut first = open(&root, "m-external-resume", false);
    result(
        &ask(
            &mut first,
            "save_notes",
            Json::Object(vec![(
                String::from("notes"),
                Json::String(String::from("carried across the put-down")),
            )]),
        ),
        "save_notes",
    );
    first.close().expect("put down in round 1's Lull");

    // A new match under a saved match's id is refused; the resume line is the
    // way back in.
    let again = External::open(
        setup(),
        &root,
        &ConfigLine {
            config: config("m-external-resume"),
            resume: false,
        },
    );
    assert!(again.is_err(), "a saved match was opened as new");

    let mut second = open(&root, "m-external-resume", true);
    assert_eq!(
        standing(&mut second),
        (String::from("lull"), String::from("1"))
    );
    let briefing = result(
        &ask(&mut second, "get_briefing", no_params()),
        "get_briefing",
    );
    let text = pharmakos_proto::json::write_line(&briefing);
    assert!(text.contains("carried across the put-down"), "{text}");

    // Ready is not part of a save (a Lull that is taken up opens with every
    // flag clear, as the lobby's does): put down before the host's hand has
    // moved, the seat says it again, and only then does the Push play.
    result(&ask(&mut second, "set_ready", no_params()), "set_ready");
    second.close().expect("put down, ready");
    let mut third = open(&root, "m-external-resume", true);
    assert_eq!(
        standing(&mut third),
        (String::from("lull"), String::from("1"))
    );
    result(&ask(&mut third, "set_ready", no_params()), "set_ready");
    third.advance().expect("round 1 plays");
    assert_eq!(
        standing(&mut third),
        (String::from("lull"), String::from("2"))
    );
    third.close().expect("put down in round 2's Lull");

    // Round 2's Lull is where the next process lands, with the notebook.
    let mut fourth = open(&root, "m-external-resume", true);
    assert_eq!(
        standing(&mut fourth),
        (String::from("lull"), String::from("2"))
    );
    fourth.close().expect("put down again");
}

#[test]
fn a_message_that_is_not_a_request_is_answered_and_never_dropped() {
    let root = data_root("external-malformed");
    let mut external = open(&root, "m-external-malformed", false);
    for text in ["", "not json", "[1,2]", "{\"jsonrpc\":\"2.0\"}"] {
        let answer = external.call(text);
        assert_eq!(answer.get("id"), Some(&Json::Null), "{text:?}");
        assert!(answer.get("error").is_some(), "{text:?}: {answer:?}");
    }
    // One byte past the largest message: refused unread, under a null id.
    let oversize = "x".repeat(pharmakos_gateway::frame::MAX_MESSAGE_BYTES + 1);
    let answer = external.call(&oversize);
    assert_eq!(answer.get("id"), Some(&Json::Null));
    let message = match answer.get("error").and_then(|error| error.get("message")) {
        Some(Json::String(message)) => message.clone(),
        other => panic!("no message: {other:?}"),
    };
    assert!(message.contains("at most"), "{message}");
    // Not text at all: answered, never repaired into something that parses.
    let answer =
        external.call_bytes(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"get_status\xff\"}");
    assert_eq!(answer.get("id"), Some(&Json::Null));
    assert!(answer.get("error").is_some(), "{answer:?}");
    // An unknown method is a refusal under the caller's own id.
    let unknown = ask(&mut external, "no_such_method", no_params());
    assert_eq!(unknown.get("id"), Some(&Json::Number(String::from("1"))));
    assert!(unknown.get("error").is_some());
    // And the seat is still served.
    assert_eq!(
        standing(&mut external),
        (String::from("lull"), String::from("1"))
    );
    external.close().expect("the match is put down");
    let log = audit_log(&root, "m-external-malformed");
    for action in ["call <oversize>", "call <malformed>", "call <not utf-8>"] {
        assert!(log.contains(action), "{action}: {log}");
    }
}

#[test]
fn a_lull_has_the_call_budget_the_limits_name() {
    let root = data_root("external-budget");
    let mut external = open(&root, "m-external-budget", false);
    for call in 0..EXTERNAL_LIMITS.per_tick {
        let answer = ask(&mut external, "get_status", no_params());
        assert!(
            answer.get("result").is_some(),
            "call {call} was refused: {answer:?}"
        );
    }
    let over = ask(&mut external, "get_status", no_params());
    assert_eq!(refusal(&over, "the call past the budget"), "RATE_LIMITED");
    external.close().expect("the match is put down");
}

/// The door closes on a program stuck in a loop (item 231 (11), reviews 1 and
/// 2): a message that was never a request never reaches the limiter and a
/// rate-limited call still leaves its line, so the host itself stops reading
/// after [`EXTERNAL_REFUSALS_IN_A_ROW`] of them in a row. What arrives after
/// that is counted in one line, never one line each; and the next process has
/// an open door.
#[test]
fn the_door_closes_after_refusals_in_a_row_and_the_log_stops_growing() {
    let root = data_root("external-door");
    let id = "m-external-door";
    let mut external = open(&root, id, false);
    let cap = EXTERNAL_REFUSALS_IN_A_ROW;

    // One short of the cap, then a call the limiter admits: the count starts
    // again, and the door stays open.
    for _ in 1..cap {
        let answer = external.call("not json");
        assert_eq!(answer.get("id"), Some(&Json::Null));
    }
    assert!(!external.door_closed());
    assert!(
        ask(&mut external, "get_status", no_params())
            .get("result")
            .is_some()
    );

    // The cap, in a row, of every kind that is counted: the door closes on
    // the last, which is still answered as what it was.
    for refusal in 0..cap {
        assert!(!external.door_closed(), "closed early, at {refusal}");
        let answer = if refusal % 2 == 0 {
            external.call("not json")
        } else {
            external.call_bytes(b"\xff")
        };
        assert!(answer.get("error").is_some());
    }
    assert!(external.door_closed());

    // After it: answered in the door's own words under the caller's id,
    // unread (an admitted call would have been answered), and with no line
    // of its own.
    let flood = 500_u32;
    for _ in 0..flood {
        let answer = ask(&mut external, "get_status", no_params());
        assert_eq!(refusal(&answer, "a call at a closed door"), "RATE_LIMITED");
        let message = match answer.get("error").and_then(|error| error.get("message")) {
            Some(Json::String(message)) => message.clone(),
            other => panic!("no message: {other:?}"),
        };
        assert!(message.contains("door is closed"), "{message}");
    }
    external.close().expect("the match is put down");

    let log = audit_log(&root, id);
    let lines = |needle: &str| log.lines().filter(|line| line.contains(needle)).count();
    assert_eq!(lines("external seat door closed"), 1, "{log}");
    assert_eq!(
        lines(&format!("external seat unanswered: {flood}")),
        1,
        "{log}"
    );
    let unread = lines("call <malformed>") + lines("call <not utf-8>");
    assert_eq!(
        u32::try_from(unread).expect("a small count"),
        cap.saturating_sub(1).saturating_add(cap),
        "one line a message that was read, and none after the door closed"
    );
    // A message that was never a request is audited under the outside seat's
    // handle, like every other line of its.
    let handle = log
        .lines()
        .find(|line| line.split('\t').nth(4) == Some("external seat"))
        .and_then(|line| line.split('\t').nth(3).map(str::to_owned))
        .expect("the line that names the outside seat's handle");
    assert_ne!(handle, "-");
    for line in log.lines().filter(|line| line.contains("call <malformed>")) {
        assert_eq!(line.split('\t').nth(3), Some(handle.as_str()), "{line}");
    }

    // The budget and the door are this process's: the next one is served.
    let mut next = open(&root, id, true);
    assert!(!next.door_closed());
    assert!(
        ask(&mut next, "get_status", no_params())
            .get("result")
            .is_some()
    );
    next.close().expect("put down again");
}

/// A call past the budget is a refusal the door counts, so a bridge that has
/// spent its Lull's budget is closed after the cap more, not left answering
/// `RATE_LIMITED` (and writing a line) for as long as it is fed.
#[test]
fn a_spent_budget_closes_the_door_after_the_cap_more() {
    let root = data_root("external-spent");
    let mut external = open(&root, "m-external-spent", false);
    for _ in 0..EXTERNAL_LIMITS.per_tick {
        assert!(
            ask(&mut external, "get_status", no_params())
                .get("result")
                .is_some()
        );
    }
    for _ in 0..EXTERNAL_REFUSALS_IN_A_ROW {
        assert!(!external.door_closed());
        let over = ask(&mut external, "set_ready", no_params());
        assert_eq!(refusal(&over, "a call past the budget"), "RATE_LIMITED");
    }
    assert!(external.door_closed());
    external.close().expect("the match is put down");
}

#[test]
fn a_line_with_no_seat_to_play_is_refused_before_anything_is_written() {
    let root = data_root("external-no-seat");
    let mut none = config("m-external-no-seat");
    none.human_seat = None;
    let refused = External::open(
        setup(),
        &root,
        &ConfigLine {
            config: none,
            resume: false,
        },
    )
    .expect_err("no seat to play");
    assert_eq!(
        refused.code(),
        pharmakos_gateway::error::Code::InvalidArgument
    );
    let mut past = config("m-external-no-seat");
    past.human_seat = Some(2);
    let refused = External::open(
        setup(),
        &root,
        &ConfigLine {
            config: past,
            resume: false,
        },
    )
    .expect_err("a seat the match has not got");
    assert_eq!(
        refused.code(),
        pharmakos_gateway::error::Code::InvalidArgument
    );
    assert!(
        !root.join("matches").join("m-external-no-seat").exists(),
        "a refused line left a match folder behind"
    );
}
