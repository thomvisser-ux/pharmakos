// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A resumed Lull reads the last Push as the live one does (lane `opsave`;
//! decisions-log item 177 (5)).
//!
//! A match is played live; at every Lull it is saved, the last Push's feed and
//! recap captured ([`Surface::last_push`]) and carried through their own text,
//! and a fresh surface is resumed from the save and given them back
//! ([`Surface::restore_last_push`]). Every seat-facing read of the resumed
//! Lull then answers byte for byte what the live one did, for every seat, the
//! lobby and a spectator. The view is the one read left out, and on purpose:
//! a resume mints a new view id so that no cursor or handle of the dead
//! process is read as the new one's (`tests/save.rs`,
//! `a_cursor_from_the_dead_process_is_stale_on_the_resumed_surface`).
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use pharmakos_gateway::error::Code;
use pharmakos_gateway::fog::FogPolicy;
use pharmakos_gateway::host::{Host, Settings};
use pharmakos_gateway::save::{Boundary, Continuation, SavedMatch};
use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::surface::last_push::LastPush;
use pharmakos_gateway::token::Token;
use pharmakos_proto::gp::api::v1::status::Phase;
use pharmakos_proto::json::{self, Json};
use pharmakos_sim::tables::SeatId;

use support::{
    FIRST_LULL_MS, SEED, admin_token, call, call_in_lull, hosted_with, quote, result, rules_json,
    seat_token, spectator_token, walk_east,
};

/// A segment long enough for a commander to walk: 60 ticks.
const SEGMENT_MS: i32 = 3_000;

/// Three rounds: two Lulls after a Push, and the opening one.
const ROUNDS: u32 = 3;

/// Both seats.
const SEATS: [SeatId; 2] = [SeatId::new(0), SeatId::new(1)];

const CONTINUATION: Continuation = Continuation {
    generation: 1,
    last_tick: 0,
};

fn new_match(match_id: &str) -> Surface {
    hosted_with(match_id, FogPolicy::fogged(), 2, SEGMENT_MS, ROUNDS, None)
}

/// The last Push as its save text carries it: rendered, written, read back.
fn through_the_text(last: &LastPush) -> LastPush {
    let text = json::write(&last.render());
    let read = LastPush::parse(&json::read(&text).expect("JSON")).expect("its own text");
    assert_eq!(&read, last, "the text carries all of it");
    read
}

/// A fresh surface resumed from `saved`, carrying `last` as its last Push,
/// or none (a format-version-3 save, made before the save carried it).
fn resume(match_id: &str, saved: &SavedMatch, last: Option<&LastPush>) -> Surface {
    let host = Host::open_from(
        &rules_json(),
        SEED,
        2,
        &Settings {
            segment_lengths_ms: vec![SEGMENT_MS],
            round_limit: ROUNDS,
        },
        None,
    )
    .expect("a match");
    let mut saved = saved.clone();
    saved.last_push = last.cloned();
    Surface::new(
        match_id,
        SEED,
        host.rules().clone(),
        FogPolicy::fogged(),
        &SEATS,
    )
    .expect("a surface")
    .resume(host, &saved, CONTINUATION)
    .expect("the Lull resumes")
}

/// What each seat submits: seat 0 walks further east each round, seat 1 in
/// rounds 1 and 3, so round 2 plays its safe playbook.
fn plan_round(surface: &mut Surface) {
    let round = surface.time().round;
    let east = i32::try_from(round)
        .expect("a small round")
        .saturating_add(3);
    let mut orders = vec![(0_u8, walk_east(surface, 0, east, 0))];
    if round != 2 {
        orders.push((1, walk_east(surface, 1, 3, 1)));
    }
    let mut left = FIRST_LULL_MS;
    for (seat, playbook) in orders {
        let token = seat_token(surface, seat);
        let answer = result(
            &call_in_lull(
                surface,
                &token,
                &mut left,
                "submit_plan",
                &format!("{{\"playbook_jsonc\":{}}}", quote(&playbook)),
            ),
            "submit_plan",
        );
        assert_eq!(
            answer.get("accepted"),
            Some(&Json::Bool(true)),
            "{answer:?}"
        );
    }
}

/// Play the Push to its end, close the recap and open the next Lull.
fn play_round(surface: &mut Surface) {
    assert!(surface.begin_push().expect("the Push begins"));
    while let Some(report) = surface.step().expect("a tick") {
        if report.segment_ended {
            break;
        }
    }
    assert!(surface.end_recap().expect("the recap ends"));
    if surface.time().phase == Phase::Lull {
        surface.open_lull().expect("the next Lull");
    }
}

/// Every page of `token`'s feed at `detail`, `limit` lines a page, each
/// answer's text.
fn feed_pages(surface: &mut Surface, token: &Token, detail: &str, limit: u32) -> Vec<String> {
    let mut pages = Vec::new();
    let mut cursor = String::new();
    for _ in 0..64 {
        let answer = call(
            surface,
            token,
            "get_segment_feed",
            &format!(
                "{{\"detail\":\"{detail}\",\"limit\":{limit},\"cursor\":{}}}",
                quote(&cursor)
            ),
        );
        pages.push(json::write(&answer));
        let page = result(&answer, "get_segment_feed");
        cursor = match page.get("next_cursor") {
            Some(Json::String(next)) => next.clone(),
            other => panic!("a cursor: {other:?}"),
        };
        let events = match page.get("events") {
            Some(Json::Array(events)) => events.len(),
            _ => 0,
        };
        if events == 0 {
            break;
        }
    }
    pages
}

/// Every seat-facing read of a Lull, for each caller: both seats, the lobby
/// and a no-fog spectator, each answer's text in a fixed order.
fn reads(surface: &mut Surface) -> Vec<(String, String)> {
    // Every read, past the rate limit's few calls a tick: the reads are the
    // test, and the limiter counts the same on both sides.
    surface.set_limits(pharmakos_gateway::limit::Limits {
        per_tick: 100_000,
        per_window: 1_000_000,
        window_ticks: 200,
    });
    let mut callers: Vec<(String, Token)> = SEATS
        .iter()
        .map(|seat| {
            (
                format!("seat {}", seat.raw()),
                seat_token(surface, seat.raw()),
            )
        })
        .collect();
    callers.push((String::from("admin"), admin_token(surface)));
    callers.push((String::from("spectator"), spectator_token(surface, true)));
    let mut out = Vec::new();
    for (who, token) in callers {
        for page in feed_pages(surface, &token, "full", 256) {
            out.push((format!("{who} get_segment_feed full"), page));
        }
        for page in feed_pages(surface, &token, "brief", 3) {
            out.push((format!("{who} get_segment_feed brief 3"), page));
        }
        for (method, params) in [
            ("get_status", "{}"),
            ("get_recap", "{}"),
            ("get_briefing", "{}"),
            ("list_beacons", "{}"),
            ("get_map_summary", "{}"),
            ("get_economy_forecast", "{}"),
            ("list_drafts", "{}"),
            ("wait_for", "{\"trigger\":\"feed_digest\",\"timeout_ms\":0}"),
        ] {
            let answer = call(surface, &token, method, params);
            out.push((format!("{who} {method}"), json::write(&answer)));
        }
    }
    out
}

/// The differential: at every Lull of a three-round match, the resumed Lull's
/// reads are the live Lull's, byte for byte.
#[test]
fn every_read_of_a_resumed_lull_is_the_live_lulls() {
    const MATCH_ID: &str = "m-opsave-reads";
    let mut live = new_match(MATCH_ID);
    let mut lulls = 0_u32;
    while live.time().phase == Phase::Lull {
        let saved = live.lull_save().expect("a Lull saves");
        let last = through_the_text(&live.last_push(Boundary::Lull).expect("the last Push"));
        assert_eq!(saved.last_push.as_ref(), Some(&last), "the save carries it");
        let mut resumed = resume(MATCH_ID, &saved, Some(&last));
        // The Lull's countdown is the client's, which it reports again after a
        // resume (decisions-log item 99): both sides report the same.
        for surface in [&mut live, &mut resumed] {
            let full = surface.lull_length(surface.time().round);
            surface
                .set_phase_remaining_ms(full)
                .expect("a custom match takes a countdown");
        }
        // The feed's clock, which dates every line the Lull publishes and
        // the view's `at_ms`: counted from the anchor the save carries.
        assert_eq!(
            resumed.segment_elapsed_ms(),
            live.segment_elapsed_ms(),
            "round {}: the feed's clock",
            live.time().round
        );
        let (want, got) = (reads(&mut live), reads(&mut resumed));
        assert_eq!(want.len(), got.len(), "round {}", live.time().round);
        for ((what, want), (_, got)) in want.iter().zip(&got) {
            assert_eq!(got, want, "round {}: {what}", live.time().round);
        }
        lulls = lulls.saturating_add(1);
        plan_round(&mut live);
        play_round(&mut live);
    }
    assert_eq!(lulls, ROUNDS, "every Lull was compared");
}

/// The feed a resumed Lull shows is the Push's own: its `segment_ended`, its
/// commanders' lines and its steps, with the same cursors, so a cursor the
/// dead process issued pages the resumed feed exactly where it left off.
#[test]
fn a_cursor_of_the_live_lull_pages_the_resumed_one() {
    const MATCH_ID: &str = "m-opsave-cursor";
    let mut live = new_match(MATCH_ID);
    plan_round(&mut live);
    play_round(&mut live);
    let saved = live.lull_save().expect("a Lull saves");
    let last = through_the_text(&live.last_push(Boundary::Lull).expect("the last Push"));
    let mut resumed = resume(MATCH_ID, &saved, Some(&last));
    let live_token = seat_token(&mut live, 0);
    let first = result(
        &call(
            &mut live,
            &live_token,
            "get_segment_feed",
            "{\"detail\":\"full\",\"limit\":2}",
        ),
        "get_segment_feed",
    );
    let cursor = match first.get("next_cursor") {
        Some(Json::String(cursor)) => cursor.clone(),
        other => panic!("a cursor: {other:?}"),
    };
    let params = format!(
        "{{\"detail\":\"full\",\"limit\":5,\"cursor\":{}}}",
        quote(&cursor)
    );
    let want = json::write(&call(&mut live, &live_token, "get_segment_feed", &params));
    let resumed_token = seat_token(&mut resumed, 0);
    let got = json::write(&call(
        &mut resumed,
        &resumed_token,
        "get_segment_feed",
        &params,
    ));
    assert_eq!(got, want, "the live cursor reads the resumed feed");
    assert!(want.contains("segment_ended"), "{want}");
}

/// Secrecy (AGENTS.md section 7): the save's feed is the whole bus, and a
/// resumed seat is shown of it only what the fog filter shows it, as live: no
/// line private to the other seat reaches it. In round 1 seat 1's playbook
/// holds before it walks, so its route has a second step and seat 0's has
/// not: "Step 2 started." is seat 1's own line, in the save and in seat 1's
/// resumed feed, and in nobody else's.
#[test]
fn a_resumed_seat_is_shown_no_line_of_the_other_seats() {
    const MATCH_ID: &str = "m-opsave-secret";
    const THEIRS: &str = "Step 2 started.";
    let mut live = new_match(MATCH_ID);
    plan_round(&mut live);
    play_round(&mut live);
    let saved = live.lull_save().expect("a Lull saves");
    let last = live.last_push(Boundary::Lull).expect("the last Push");
    assert!(
        json::write(&last.render()).contains(THEIRS),
        "the save carries seat 1's private line"
    );
    let mut resumed = resume(MATCH_ID, &saved, Some(&through_the_text(&last)));
    let shown =
        |surface: &mut Surface, token: &Token| feed_pages(surface, token, "full", 256).join("\n");
    let theirs = seat_token(&mut resumed, 1);
    assert!(
        shown(&mut resumed, &theirs).contains(THEIRS),
        "seat 1 is shown its own"
    );
    for token in [
        seat_token(&mut resumed, 0),
        admin_token(&mut resumed),
        spectator_token(&mut resumed, true),
    ] {
        assert!(
            !shown(&mut resumed, &token).contains(THEIRS),
            "a caller other than seat 1 is shown seat 1's line"
        );
    }
}

/// A save with no last Push -- one written before the save carried it --
/// resumes as it always did: a Lull after the first with a feed that shows no
/// Push, which the operator reads as `PushUnread::Resumed`.
#[test]
fn a_lull_resumed_without_the_last_push_shows_no_push() {
    const MATCH_ID: &str = "m-opsave-old";
    let mut live = new_match(MATCH_ID);
    plan_round(&mut live);
    play_round(&mut live);
    let saved = live.lull_save().expect("a Lull saves");
    let mut resumed = resume(MATCH_ID, &saved, None);
    let token = seat_token(&mut resumed, 0);
    let shown = feed_pages(&mut resumed, &token, "full", 256).join("\n");
    assert!(!shown.contains("segment_ended"), "{shown}");
}

/// The boundary decides the feed: a `lull` save carries it, a `sealed` one,
/// whose Push replays its own, does not, and a member that says otherwise is
/// refused as damaged.
#[test]
fn a_feed_on_the_wrong_boundary_is_refused() {
    const MATCH_ID: &str = "m-opsave-boundary";
    let mut live = new_match(MATCH_ID);
    plan_round(&mut live);
    play_round(&mut live);
    let saved = live.lull_save().expect("a Lull saves");
    let lull = live.last_push(Boundary::Lull).expect("the last Push");
    let sealed = live.last_push(Boundary::Sealed).expect("the last Push");
    assert!(json::write(&sealed.render()).contains("\"feed\": null"));
    let mut resumed = resume(MATCH_ID, &saved, None);
    let error = resumed
        .restore_last_push(Boundary::Lull, &sealed)
        .expect_err("a Lull's save carries its feed");
    assert_eq!(error.code(), Code::InvalidArgument);
    let error = resumed
        .restore_last_push(Boundary::Sealed, &lull)
        .expect_err("a Push's save carries none");
    assert_eq!(error.code(), Code::InvalidArgument);
}

// ---------------------------------------------------------------------------
// The decoder of a save's last Push, under damage: only the typed error
// escapes (the global verification rule for a decoder of untrusted input)
// ---------------------------------------------------------------------------

/// Cases the damage property plays: each one damaged member, parsed and, when
/// it parses, restored.
const DAMAGE_CASES: u64 = 3_000;

/// xorshift64*, std-only and seeded per case (`crates/proto/tests/hostile_input.rs`'s).
struct Rng(u64);

impl Rng {
    const fn new(case: u64) -> Self {
        Self(case.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03)
    }

    const fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A value in `0..bound`, `bound` at least 1.
    fn below(&mut self, bound: usize) -> usize {
        let bound = u64::try_from(bound.max(1)).expect("a small bound");
        usize::try_from(self.next() % bound).expect("below a usize bound")
    }
}

/// One of `items`, `items` not empty.
fn pick<'a, T>(items: &'a [T], rng: &mut Rng) -> &'a T {
    items
        .get(rng.below(items.len()))
        .expect("a pick from a list that is not empty")
}

/// How many nodes `value` holds, itself included.
fn nodes(value: &Json) -> usize {
    1 + match value {
        Json::Array(items) => items.iter().map(nodes).sum(),
        Json::Object(entries) => entries.iter().map(|(_, entry)| nodes(entry)).sum(),
        _ => 0,
    }
}

/// `value` with its `at`th node (pre-order) damaged one way `rng` picks.
fn damage(value: &Json, at: &mut usize, rng: &mut Rng) -> Json {
    if *at == 0 {
        *at = usize::MAX;
        return damaged_node(value, rng);
    }
    *at = at.saturating_sub(1);
    match value {
        Json::Array(items) => Json::Array(items.iter().map(|item| damage(item, at, rng)).collect()),
        Json::Object(entries) => Json::Object(
            entries
                .iter()
                .map(|(key, entry)| (key.clone(), damage(entry, at, rng)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// One node, damaged: a number moved or replaced, a string replaced, a list
/// cut or grown, a member dropped, renamed or added, a value of another type.
fn damaged_node(value: &Json, rng: &mut Rng) -> Json {
    const NUMBERS: [&str; 12] = [
        "0",
        "-1",
        "1",
        "7",
        "8",
        "255",
        "256",
        "4294967295",
        "4294967296",
        "18446744073709551616",
        "-9223372036854775809",
        "1.5",
    ];
    const STRINGS: [&str; 10] = [
        "",
        "public",
        "private",
        "world",
        "ledger_memo",
        "safe_playbook_filed",
        "first_death",
        "not_a_kind",
        "Step 1 started.",
        "Bad Kind",
    ];
    match (rng.below(4), value) {
        (0, _) => match rng.below(5) {
            0 => Json::Null,
            1 => Json::Bool(true),
            2 => Json::Array(Vec::new()),
            3 => Json::Object(Vec::new()),
            _ => Json::String(String::from("x")),
        },
        (_, Json::Number(lexeme)) => match rng.below(3) {
            0 => Json::Number(String::from(*pick(&NUMBERS, rng))),
            1 => Json::Number(format!("{lexeme}0")),
            _ => Json::Number(format!("-{lexeme}")),
        },
        (_, Json::String(_)) => Json::String(String::from(*pick(&STRINGS, rng))),
        (_, Json::Bool(flag)) => Json::Bool(!flag),
        (_, Json::Array(items)) => {
            let mut items = items.clone();
            match rng.below(3) {
                0 if !items.is_empty() => {
                    items.remove(rng.below(items.len()));
                }
                1 if !items.is_empty() => {
                    let copy = pick(&items, rng).clone();
                    let times = 1 + rng.below(600);
                    for _ in 0..times {
                        items.push(copy.clone());
                    }
                }
                _ => items.reverse(),
            }
            Json::Array(items)
        }
        (_, Json::Object(entries)) => {
            let mut entries = entries.clone();
            match rng.below(3) {
                0 if !entries.is_empty() => {
                    entries.remove(rng.below(entries.len()));
                }
                1 if !entries.is_empty() => {
                    let index = rng.below(entries.len());
                    if let Some((key, _)) = entries.get_mut(index) {
                        key.push('_');
                    }
                }
                _ => entries.push((String::from("extra"), Json::Null)),
            }
            Json::Object(entries)
        }
        (_, other) => other.clone(),
    }
}

/// Every damaged member is read and restored, or refused as
/// `INVALID_ARGUMENT`; nothing else escapes, and a refused restore leaves the
/// surface's reads as they were.
#[test]
fn a_damaged_last_push_is_refused_and_only_as_an_argument() {
    const MATCH_ID: &str = "m-opsave-damage";
    let mut live = new_match(MATCH_ID);
    plan_round(&mut live);
    play_round(&mut live);
    let saved = live.lull_save().expect("a Lull saves");
    let whole = live
        .last_push(Boundary::Lull)
        .expect("the last Push")
        .render();
    let mut resumed = resume(MATCH_ID, &saved, None);
    let count = nodes(&whole);
    let (mut refused, mut taken) = (0_u32, 0_u32);
    for case in 0..DAMAGE_CASES {
        let mut rng = Rng::new(case);
        let mut at = rng.below(count);
        let damaged = damage(&whole, &mut at, &mut rng);
        // Damage the JSON reader itself refuses (a leading zero) never reaches
        // this decoder: `Save::parse` refuses the file as not JSON first.
        let Ok(read) = json::read(&json::write(&damaged)) else {
            refused = refused.saturating_add(1);
            continue;
        };
        let before = format!("{:?}", resumed.feed());
        let outcome = LastPush::parse(&read)
            .and_then(|last| resumed.restore_last_push(Boundary::Lull, &last));
        match outcome {
            Ok(()) => taken = taken.saturating_add(1),
            Err(error) => {
                assert_eq!(
                    error.code(),
                    Code::InvalidArgument,
                    "case {case}: {error:?}"
                );
                assert!(
                    error
                        .message()
                        .starts_with("this is not a save this build can resume"),
                    "case {case}: {}",
                    error.message()
                );
                assert_eq!(
                    format!("{:?}", resumed.feed()),
                    before,
                    "case {case}: a refused restore changed the feed"
                );
                refused = refused.saturating_add(1);
            }
        }
    }
    assert!(
        refused > 1_000,
        "the damage refuses: {refused} of {DAMAGE_CASES}"
    );
    assert!(
        taken > 0,
        "and some damage is a member this build would write"
    );
}

// ---------------------------------------------------------------------------
// An ended match is not resumed (lane `opsave`, from `fixq-c`'s finding)
// ---------------------------------------------------------------------------

/// The match's end leaves an `ended` save for the host loop to write, in
/// place of the final round's `sealed` one, and neither the save's check nor
/// the surface's resume continues it: both refuse it as `INVALID_ARGUMENT`,
/// in the words the host says on its announce path. A recap is never saved
/// (`lull_save` answers `PHASE_CLOSED` there), so the final recap's only
/// save on disk is this one.
#[test]
fn an_ended_match_leaves_an_ended_save_that_does_not_resume() {
    use pharmakos_gateway::save::{Save, Stamp};
    const MATCH_ID: &str = "m-opsave-ended";
    let mut live = new_match(MATCH_ID);
    let mut last_save: Option<SavedMatch> = None;
    let mut refused_in_recap = false;
    while live.time().phase == Phase::Lull {
        plan_round(&mut live);
        assert!(live.begin_push().expect("the Push begins"));
        while let Some(report) = live.step().expect("a tick") {
            if report.segment_ended {
                break;
            }
        }
        if let Some(save) = live.take_persistence().save {
            last_save = Some(save);
        }
        if live.time().phase == Phase::Recap {
            let error = live.lull_save().expect_err("a recap is not saved");
            assert_eq!(error.code(), Code::PhaseClosed);
            refused_in_recap = true;
        }
        assert!(live.end_recap().expect("the recap ends"));
        if let Some(save) = live.take_persistence().save {
            last_save = Some(save);
        }
        if live.time().phase == Phase::Lull {
            live.open_lull().expect("the next Lull");
        }
    }
    assert_eq!(live.time().phase, Phase::Ended);
    assert!(refused_in_recap, "a recap was met and refused a save");
    let ended = last_save.expect("a save");
    assert_eq!(
        ended.boundary,
        Boundary::Ended,
        "the last save is the match's end"
    );

    let config = pharmakos_gateway::serve::Config {
        match_id: MATCH_ID.to_owned(),
        seed: SEED,
        seats: 2,
        human_seat: None,
        segment_lengths_ms: vec![SEGMENT_MS],
        round_limit: ROUNDS,
        preset: pharmakos_gateway::probation::Preset::Custom,
    };
    let hash = support::rules().rules_hash();
    let text = Save {
        stamp: Stamp::current(hash),
        config: config.clone(),
        state: ended.clone(),
    }
    .render();
    let read = Save::parse(&text).expect("an ended save reads");
    let error = read
        .check(&config, hash)
        .expect_err("an ended match is not resumed");
    assert_eq!(error.code(), Code::InvalidArgument);
    assert!(error.message().contains("has ended"), "{}", error.message());

    let host = Host::open_from(
        &rules_json(),
        SEED,
        2,
        &Settings {
            segment_lengths_ms: vec![SEGMENT_MS],
            round_limit: ROUNDS,
        },
        None,
    )
    .expect("a match");
    let error = Surface::new(
        MATCH_ID,
        SEED,
        host.rules().clone(),
        FogPolicy::fogged(),
        &SEATS,
    )
    .expect("a surface")
    .resume(host, &ended, CONTINUATION)
    .expect_err("the surface refuses it too");
    assert_eq!(error.code(), Code::InvalidArgument);
    assert!(error.message().contains("has ended"), "{}", error.message());
}

/// The line bound: a round-1 Lull's feed has run no tick, so it holds at most
/// `EVENT_BUS_CAPACITY` lines for each of the `DRAINS_BEYOND_TICKS` drains and
/// a few of the gateway's own; a member holding more is refused, never cut.
#[test]
fn a_feed_past_its_line_bound_is_refused() {
    const MATCH_ID: &str = "m-opsave-bound";
    let live = new_match(MATCH_ID);
    let saved = live.lull_save().expect("a Lull saves");
    let mut member = live
        .last_push(Boundary::Lull)
        .expect("the last Push")
        .render();
    // Pinned, not computed from the constants it checks: three drains of a
    // 512-event bus, a safe-playbook line for each of two seats, and one line
    // for each of the four memos.
    let most: usize = 3 * 512 + 2 + 4;
    if let Json::Object(entries) = &mut member
        && let Some((_, Json::Object(feed))) = entries.iter_mut().find(|(key, _)| key == "feed")
        && let Some((_, Json::Array(lines))) = feed.iter_mut().find(|(key, _)| key == "lines")
    {
        let one = lines.first().cloned().expect("a line");
        lines.clear();
        lines.resize(most, one.clone());
        let at_the_bound = LastPush::parse(&member).expect("it reads");
        let mut resumed = resume(MATCH_ID, &saved, None);
        resumed
            .restore_last_push(Boundary::Lull, &at_the_bound)
            .expect("at the bound");
        if let Some((_, Json::Object(feed))) = entries_of(&mut member)
            .iter_mut()
            .find(|(key, _)| key == "feed")
            && let Some((_, Json::Array(lines))) = feed.iter_mut().find(|(key, _)| key == "lines")
        {
            lines.push(one);
        }
        let past = LastPush::parse(&member).expect("it reads");
        let error = resumed
            .restore_last_push(Boundary::Lull, &past)
            .expect_err("past the bound");
        assert_eq!(error.code(), Code::InvalidArgument);
        assert!(error.message().contains("lines"), "{}", error.message());
    } else {
        panic!("a feed with lines: {member:?}");
    }
}

/// The entries of an object.
fn entries_of(value: &mut Json) -> &mut Vec<(String, Json)> {
    match value {
        Json::Object(entries) => entries,
        other => panic!("an object: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// One damaged member a restore-time check (fix pass 1, item 3)
// ---------------------------------------------------------------------------

/// The member's value at `path`, a key at a time.
fn at_path<'a>(value: &'a mut Json, path: &[&str]) -> &'a mut Json {
    path.iter().fold(value, |here, key| match here {
        Json::Object(entries) => {
            &mut entries
                .iter_mut()
                .find(|(name, _)| name == key)
                .unwrap_or_else(|| panic!("`{key}` in the member"))
                .1
        }
        other => panic!("an object at `{key}`: {other:?}"),
    })
}

/// A whole number, as the save spells one.
fn number(value: i128) -> Json {
    Json::Number(value.to_string())
}

/// The time of a member's last line.
fn last_at(member: &mut Json) -> Json {
    match at_path(member, &["feed", "lines"]) {
        Json::Array(lines) => lines
            .last()
            .and_then(|line| line.get("at_ms").cloned())
            .expect("a line's time"),
        other => panic!("lines: {other:?}"),
    }
}

/// A member with one more line at its end.
fn push_line(member: &mut Json, kind: &str, audience: Json, witnesses: Vec<Json>, memo: Json) {
    let at = last_at(member);
    if let Json::Array(lines) = at_path(member, &["feed", "lines"]) {
        lines.push(Json::Object(vec![
            (String::from("at_ms"), at),
            (String::from("kind"), Json::String(kind.to_owned())),
            (String::from("text"), Json::String(String::from("x"))),
            (String::from("audience"), audience),
            (String::from("witnesses"), Json::Array(witnesses)),
            (String::from("memo"), memo),
        ]));
    }
}

/// A world audience owned by `owner`.
fn world_of(owner: u8) -> Json {
    Json::Object(vec![
        (String::from("to"), Json::String(String::from("world"))),
        (String::from("owner"), number(i128::from(owner))),
        (
            String::from("at"),
            Json::Array(vec![number(0), number(0), number(0)]),
        ),
    ])
}

/// A private audience of `seat`.
fn private_of(seat: u8) -> Json {
    Json::Object(vec![
        (String::from("to"), Json::String(String::from("private"))),
        (String::from("seat"), number(i128::from(seat))),
    ])
}

/// One damage of a member, by name.
type Damage = Box<dyn Fn(&mut Json)>;

/// Each restore-time check's damage: its name, the words its refusal says,
/// and the damage. `rows` is the world's beacon rows; `anchor` the feed's
/// anchor.
fn restore_damages(rows: usize, anchor: i128) -> Vec<(&'static str, &'static str, Damage)> {
    let mut damages = record_damages(rows);
    damages.extend(feed_damages(anchor));
    damages
}

/// The lines' and the record's damages of [`restore_damages`].
fn record_damages(rows: usize) -> Vec<(&'static str, &'static str, Damage)> {
    vec![
        (
            "M16 the recap's round",
            "recap tells round",
            Box::new(|member| *at_path(member, &["recap", "facts", "round"]) = number(7)),
        ),
        (
            "M17 a line's owner among its witnesses",
            "witnessed by the seat it belongs to",
            Box::new(|member| {
                push_line(
                    member,
                    "beacon_placed",
                    world_of(0),
                    vec![number(0)],
                    Json::Null,
                );
            }),
        ),
        (
            "M18 a memo line its seat was not issued",
            "was not issued",
            Box::new(|member| {
                push_line(
                    member,
                    "ledger_memo",
                    private_of(0),
                    Vec::new(),
                    Json::String(String::from("first_death")),
                );
            }),
        ),
        (
            "M21 an unknown kind",
            "neither the sim nor the gateway writes",
            Box::new(|member| {
                push_line(
                    member,
                    "no_such_kind",
                    private_of(0),
                    Vec::new(),
                    Json::Null,
                );
            }),
        ),
        (
            "M22 more dark beacons than the world has rows",
            "dark beacons",
            Box::new(move |member| {
                let ordinals: Vec<Json> = (0..=rows)
                    .map(|ordinal| number(i128::try_from(ordinal).expect("small")))
                    .collect();
                *at_path(member, &["recap", "facts", "dark"]) =
                    Json::Array(vec![Json::Object(vec![
                        (String::from("seat"), number(0)),
                        (String::from("ordinals"), Json::Array(ordinals)),
                        (String::from("shed_kw"), number(0)),
                    ])]);
            }),
        ),
    ]
}

/// The feed's and the counts' damages of [`restore_damages`].
fn feed_damages(anchor: i128) -> Vec<(&'static str, &'static str, Damage)> {
    vec![
        (
            "the anchor far past the world's tick",
            "does not bear out",
            Box::new(|member| *at_path(member, &["feed", "anchor_tick"]) = number(4_000_000_000)),
        ),
        (
            "the anchor a tick off its Push's start",
            "is not where its segment began",
            Box::new(move |member| *at_path(member, &["feed", "anchor_tick"]) = number(anchor + 1)),
        ),
        (
            "a segment past the round's one",
            "does not bear out",
            Box::new(|member| *at_path(member, &["feed", "segment"]) = number(1)),
        ),
        (
            "a feed of a round the match is not near",
            "does not bear out",
            Box::new(|member| {
                *at_path(member, &["feed", "round"]) = number(7);
                *at_path(member, &["recap"]) =
                    Json::Object(vec![(String::from("none"), Json::Bool(true))]);
            }),
        ),
        (
            "a segment that ran more ticks than its round's length",
            "its round's length allows",
            Box::new(|member| {
                *at_path(member, &["recap", "facts", "ticks"]) = number(4_000_000_000);
            }),
        ),
        (
            "200 000 failed steps in a segment of 60 ticks",
            "failed steps",
            Box::new(|member| {
                let one = Json::Object(vec![
                    (String::from("seat"), number(0)),
                    (String::from("value"), number(1)),
                ]);
                *at_path(member, &["recap", "facts", "failures"]) = Json::Array(vec![one; 200_000]);
            }),
        ),
        (
            "drops since the segment's end past the bound",
            "events the bus dropped",
            Box::new(|member| *at_path(member, &["feed", "dropped_since"]) = number(0x1_0000_0000)),
        ),
        (
            "the record's drops past the bound",
            "events the bus dropped",
            Box::new(|member| {
                *at_path(member, &["recap", "facts", "dropped"]) = number(0x1_0000_0000);
            }),
        ),
        (
            "a lost record's drops past the bound",
            "events the bus dropped",
            Box::new(|member| {
                *at_path(member, &["recap"]) =
                    Json::Object(vec![(String::from("lost"), number(0x1_0000_0000))]);
            }),
        ),
    ]
}

/// Each check `restore_last_push` makes has its own damaged member, refused
/// as `INVALID_ARGUMENT` with its own words; after each, the good member
/// restores in the same process, and the feed is then the live one's.
#[test]
fn each_restore_check_refuses_its_own_damage_and_a_good_member_still_restores() {
    const MATCH_ID: &str = "m-opsave-checks";
    let mut live = new_match(MATCH_ID);
    plan_round(&mut live);
    play_round(&mut live);
    let saved = live.lull_save().expect("round 2's Lull saves");
    let good = live.last_push(Boundary::Lull).expect("the last Push");
    let whole = good.render();
    let rows = live.host().expect("a match").world().beacons().ids().len();
    let anchor = match at_path(&mut whole.clone(), &["feed", "anchor_tick"]) {
        Json::Number(lexeme) => lexeme.parse::<i128>().expect("a tick"),
        other => panic!("an anchor: {other:?}"),
    };
    let mut resumed = resume(MATCH_ID, &saved, None);
    for (what, words, damage) in restore_damages(rows, anchor) {
        let mut member = whole.clone();
        damage(&mut member);
        let outcome = LastPush::parse(&member)
            .and_then(|last| resumed.restore_last_push(Boundary::Lull, &last));
        let error = outcome.expect_err(what);
        assert_eq!(error.code(), Code::InvalidArgument, "{what}");
        assert!(
            error.message().contains(words),
            "{what}: {}",
            error.message()
        );
        resumed
            .restore_last_push(Boundary::Lull, &good)
            .unwrap_or_else(|error| panic!("{what}: the good member then restores: {error:?}"));
    }
    // Drops at the bound itself are a member this build would write.
    let mut at_bound = whole.clone();
    *at_path(&mut at_bound, &["feed", "dropped_since"]) = number(0xFFFF_FFFF);
    let at_bound = LastPush::parse(&at_bound).expect("it reads");
    resumed
        .restore_last_push(Boundary::Lull, &at_bound)
        .expect("drops at the bound restore");
    resumed
        .restore_last_push(Boundary::Lull, &good)
        .expect("the good member restores");
    let token = seat_token(&mut resumed, 0);
    let live_token = seat_token(&mut live, 0);
    assert_eq!(
        feed_pages(&mut resumed, &token, "full", 256),
        feed_pages(&mut live, &live_token, "full", 256),
        "the feed is the live one's after every refusal"
    );
}

/// Every end this build reaches is reported at the match's final tick, so the
/// `ended` save is made there and once: closing the final recap makes none
/// (its branch for an end no tick reported saves one only when none was).
/// And a save whose world has already ended is refused whatever boundary it
/// says, a version-3 `lull` one (no last Push) included: `Host::resume`
/// lands a save in a Lull only.
#[test]
fn every_end_is_reported_at_its_final_tick() {
    const MATCH_ID: &str = "m-opsave-final-tick";
    let mut live = new_match(MATCH_ID);
    let mut ended_at_a_tick = false;
    let mut ended: Option<SavedMatch> = None;
    while live.time().phase == Phase::Lull {
        plan_round(&mut live);
        assert!(live.begin_push().expect("the Push begins"));
        let _ = live.take_persistence();
        while let Some(report) = live.step().expect("a tick") {
            if report.match_ended {
                ended_at_a_tick = true;
                ended = live.take_persistence().save;
            }
            if report.segment_ended {
                break;
            }
        }
        let _ = live.take_persistence();
        assert!(live.end_recap().expect("the recap ends"));
        assert!(
            live.take_persistence().save.is_none(),
            "closing a recap saves nothing more"
        );
        if live.time().phase == Phase::Lull {
            live.open_lull().expect("the next Lull");
        }
    }
    assert!(ended_at_a_tick, "the end was reported at a tick");
    let mut ended = ended.expect("the ended save, made at that tick");
    assert_eq!(ended.boundary, Boundary::Ended);
    ended.boundary = Boundary::Lull;
    ended.last_push = None;
    let host = Host::open_from(
        &rules_json(),
        SEED,
        2,
        &Settings {
            segment_lengths_ms: vec![SEGMENT_MS],
            round_limit: ROUNDS,
        },
        None,
    )
    .expect("a match");
    let error = Surface::new(
        MATCH_ID,
        SEED,
        host.rules().clone(),
        FogPolicy::fogged(),
        &SEATS,
    )
    .expect("a surface")
    .resume(host, &ended, CONTINUATION)
    .expect_err("an ended world is not resumed");
    // Refused by the one restore, `Host::resume`, which lands a save only in
    // a Lull: a world that has ended is not one.
    assert_eq!(error.code(), Code::InvalidArgument);
    assert!(error.message().contains("ended"), "{}", error.message());
}

// ---------------------------------------------------------------------------
// The facts the owner approved: a death and failed steps (fix pass 1, item 2)
// ---------------------------------------------------------------------------

/// The hunt's segment: eight minutes, `scenarios/s2/hunt-the-commander`'s.
const HUNT_MS: i32 = 480_000;

/// The quarry's playbook with two first steps that fail, each for its own
/// reason: a beacon placed on the column of its own core, which no seat may
/// stack on (an illegal site), and a move to where it walks anyway given
/// 50 ms (out of time); then the walk the hunt cuts short.
fn quarry(core: [i32; 3]) -> String {
    let text = std::fs::read_to_string(
        support::workspace_root().join("scenarios/s2/hunt-the-commander.enemy.playbook.jsonc"),
    )
    .expect("the quarry's playbook");
    let walk = "{\"label\": \"south\"";
    assert!(text.contains(walk), "the quarry walks south");
    let [x, y, z] = core;
    let stack = format!(
        "{{\"label\": \"stack\", \"timeout_ms\": 60000, \"place_beacon\": {{\"at\": {{\"voxel\": \
         {{\"x\": {x}, \"y\": {y}, \"z\": {z}}}}}}}, \"on_fail\": {{\"action\": \"SKIP\"}}}},\n      "
    );
    let dash = "{\"label\": \"dash\", \"timeout_ms\": 50, \"move\": {\"to\": {\"voxel\": {\"x\": \
                358, \"y\": 60, \"z\": 37}}, \"pace\": \"DIRECT\"}, \"on_fail\": {\"action\": \
                \"SKIP\"}},\n      ";
    text.replacen(walk, &format!("{stack}{dash}{walk}"), 1)
}

/// Seat 0 hunts and seat 1 walks into sight, through `submit_plan`.
fn plan_the_hunt(surface: &mut Surface) {
    let hunter = std::fs::read_to_string(
        support::workspace_root().join("scenarios/s2/hunt-the-commander.playbook.jsonc"),
    )
    .expect("the hunter's playbook");
    let mut left = FIRST_LULL_MS;
    let (_, core) = support::core_beacon(surface, 1);
    for (seat, playbook) in [(0_u8, hunter), (1, quarry(core))] {
        let token = seat_token(surface, seat);
        let answer = result(
            &call_in_lull(
                surface,
                &token,
                &mut left,
                "submit_plan",
                &format!("{{\"playbook_jsonc\":{}}}", quote(&playbook)),
            ),
            "submit_plan",
        );
        assert_eq!(
            answer.get("accepted"),
            Some(&Json::Bool(true)),
            "{answer:?}"
        );
    }
}

/// A fresh surface resumed from `saved` of a match of `segment_ms` segments
/// and `rounds` rounds.
fn resume_hunt(match_id: &str, saved: &SavedMatch) -> Surface {
    let host = Host::open_from(
        &rules_json(),
        SEED,
        2,
        &Settings {
            segment_lengths_ms: vec![HUNT_MS],
            round_limit: 2,
        },
        None,
    )
    .expect("a match");
    Surface::new(
        match_id,
        SEED,
        host.rules().clone(),
        FogPolicy::fogged(),
        &SEATS,
    )
    .expect("a surface")
    .resume(host, saved, CONTINUATION)
    .expect("the Lull resumes")
}

/// The kinds and texts of a member's feed lines.
fn lines_of(member: &Json) -> Vec<(String, String)> {
    let Some(Json::Object(feed)) = member.get("feed") else {
        panic!("a feed: {member:?}");
    };
    let Some((_, Json::Array(lines))) = feed.iter().find(|(key, _)| key == "lines") else {
        panic!("lines");
    };
    lines
        .iter()
        .map(|line| match (line.get("kind"), line.get("text")) {
            (Some(Json::String(kind)), Some(Json::String(text))) => (kind.clone(), text.clone()),
            other => panic!("a line: {other:?}"),
        })
        .collect()
}

/// The differential on a match that holds what the owner's ruling is about:
/// seat 1's commander dies in the hunt, and its steps fail for two reasons
/// (a step out of time, a step its commander's death ended). The save carries
/// those lines by kind; the resumed Lull reads them as the live one does, its
/// feed's clock (the anchor every later line is dated from) is the live one's,
/// and the next round, played on both, is the same feed and recap.
#[test]
fn a_lull_after_a_death_and_failed_steps_resumes_as_played() {
    const MATCH_ID: &str = "m-opsave-hunt";
    let mut live = hosted_with(MATCH_ID, FogPolicy::fogged(), 2, HUNT_MS, 2, None);
    plan_the_hunt(&mut live);
    play_round(&mut live);
    assert_eq!(live.time().phase, Phase::Lull, "round 2's Lull");
    let saved = live.lull_save().expect("a Lull saves");
    let member = saved
        .last_push
        .as_ref()
        .expect("the save carries it")
        .render();
    let lines = lines_of(&member);
    assert!(
        lines.iter().any(|(kind, _)| kind == "commander_died"),
        "the save carries the death"
    );
    let mut reasons: Vec<&str> = lines
        .iter()
        .filter(|(kind, _)| kind == "step_failed")
        .map(|(_, text)| text.as_str())
        .collect();
    reasons.sort_unstable();
    reasons.dedup();
    assert!(
        reasons.len() >= 2,
        "the save carries failed steps for two reasons: {reasons:?} in {lines:?}"
    );

    let mut resumed = resume_hunt(MATCH_ID, &saved);
    for surface in [&mut live, &mut resumed] {
        let full = surface.lull_length(surface.time().round);
        surface
            .set_phase_remaining_ms(full)
            .expect("a custom match takes a countdown");
    }
    assert_eq!(
        resumed.segment_elapsed_ms(),
        live.segment_elapsed_ms(),
        "the feed's clock is the live one's"
    );
    let (want, got) = (reads(&mut live), reads(&mut resumed));
    assert_eq!(want.len(), got.len());
    for ((what, want), (_, got)) in want.iter().zip(&got) {
        assert_eq!(got, want, "round 2's Lull: {what}");
    }

    // The next round, played on both from here: its feed and its recap.
    for surface in [&mut live, &mut resumed] {
        plan_the_hunt(surface);
        play_round(surface);
    }
    let (want, got) = (reads(&mut live), reads(&mut resumed));
    for ((what, want), (_, got)) in want.iter().zip(&got) {
        assert_eq!(got, want, "after round 2: {what}");
    }
}
