// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The hosted match's one planning scratch, through the door (S3's `p3est`).
//!
//! `src/routes.rs` ("One scratch") says a match makes one search scratch, at
//! its first planning read that searches, keeps it for the match, and lends
//! it to every such read; and that a scratch is working memory, so an answer
//! never depends on what it held. The unit tests there hold the type to the
//! first half and the route estimate to the second. These hold the hosted
//! match to both, over every method that searches:
//!
//! * `get_map_summary` (a walk to every feature),
//! * `resolve_refs` (rankings and sites),
//! * `estimate_route`, with a voxel leg, a `covering` leg, a `toward` leg
//!   and a `beacon_anchor` leg.
//!
//! **One scratch, kept.** Across three rounds, with a Push between each pair
//! of Lulls, the host has made one scratch after the first read and still one
//! after the last, and the scratch's own count of search work, which a fresh
//! scratch starts at nothing, never falls: not within a Lull and not from one
//! Lull to the next. A scratch made per read, or one dropped where a Lull
//! ends and made again in the next, fails one or the other (the review's
//! mutants `m1` and `m8`, which no test before this file caught).
//!
//! **Working memory, not a cache.** At every Lull each read's answer from the
//! live host, whose scratch has by then served every earlier read of the
//! match in an order that changes by round, is byte for byte the answer of a
//! host resumed from that Lull's save that makes that read and no other, in a
//! scratch made for it alone.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use pharmakos_gateway::fog::FogPolicy;
use pharmakos_gateway::surface::Surface;
use pharmakos_proto::json::Json;
use pharmakos_sim::math::quantity::Ms;
use pharmakos_sim::tables::SeatId;

use support::{FIRST_LULL_MS, LULL_MS, MATCH, array_of, call_in_lull, result, text_of};

/// A Push of 200 ticks.
const SEGMENT_MS: i32 = 10_000;

/// Three rounds: three Lulls, and a Push between each pair.
const ROUNDS: u32 = 3;

/// One planning read that searches: what this file calls it, the method and
/// its parameters.
type Read = (&'static str, &'static str, String);

/// Every planning read that searches, as seat 0 makes it in the Lull
/// `surface` stands in, with `vent` the feature the legs and the playbook
/// name.
fn reads(surface: &Surface, vent: &str) -> Vec<Read> {
    let [x, y, z] = support::commander_voxel(surface, 0);
    let from = format!(r#"{{"voxel":{{"x":{x},"y":{y},"z":{z}}}}}"#);
    let playbook = std::fs::read_to_string(
        support::workspace_root()
            .join("scenarios")
            .join("s1")
            .join("cover-nearest-vent.playbook.jsonc"),
    )
    .expect("the committed scenario playbook");
    let leg = |to: String| format!(r#"{{"waypoints":[{from},{to}]}}"#);
    vec![
        ("the map summary", "get_map_summary", String::from("{}")),
        (
            "the references",
            "resolve_refs",
            format!(r#"{{"playbook_jsonc":{}}}"#, support::quote(&playbook)),
        ),
        (
            "a voxel leg",
            "estimate_route",
            leg(format!(
                r#"{{"voxel":{{"x":{},"y":{},"z":{z}}}}}"#,
                if x > 192 { x - 60 } else { x + 60 },
                if y > 192 { y - 60 } else { y + 60 },
            )),
        ),
        (
            "a covering leg",
            "estimate_route",
            leg(String::from(
                r#"{"covering":{"vent":{"rank":"NEAREST","coverage":"ANY"}}}"#,
            )),
        ),
        (
            "a toward leg",
            "estimate_route",
            leg(format!(
                r#"{{"toward":{{"feature":{{"feature_id":"{vent}"}}}}}}"#
            )),
        ),
        (
            "a beacon leg",
            "estimate_route",
            leg(String::from(r#"{"beacon_anchor":{"nearest":{}}}"#)),
        ),
    ]
}

/// One read's whole answer as seat 0 gets it, a refusal included, written as
/// text, with the `_status` footer taken out of a result (the footer is the
/// clock's, which a resume restarts).
fn ask(surface: &mut Surface, left: &mut i32, read: &Read) -> String {
    let token = support::seat_token(surface, 0);
    let (_, method, params) = read;
    let answer = call_in_lull(surface, &token, left, method, params);
    let Json::Object(members) = answer else {
        panic!("{method}: the answer is not an object");
    };
    let stripped = members
        .into_iter()
        .map(|(key, value)| match value {
            Json::Object(fields) if key == "result" => (
                key,
                Json::Object(
                    fields
                        .into_iter()
                        .filter(|(field, _)| field != "_status")
                        .collect(),
                ),
            ),
            other => (key, other),
        })
        .collect();
    pharmakos_proto::json::write(&Json::Object(stripped))
}

/// How many scratches `surface`'s match has made.
fn made(surface: &Surface) -> u32 {
    surface.host().expect("hosted").planning_scratches_made()
}

/// The search work done in `surface`'s planning scratch, or `None` while
/// there is no scratch: what its searches popped and visited, which nothing
/// resets and every read that searches moves.
fn work(surface: &Surface) -> Option<u64> {
    surface
        .host()
        .expect("hosted")
        .planning_work()
        .expect("the scratch is not lent between calls")
}

/// The first vent seat 0's map summary says it reaches.
fn a_reachable_vent(surface: &mut Surface) -> String {
    let token = support::seat_token(surface, 0);
    let mut left = FIRST_LULL_MS;
    let summary = result(
        &call_in_lull(surface, &token, &mut left, "get_map_summary", "{}"),
        "get_map_summary",
    );
    array_of(&summary, "features")
        .into_iter()
        .filter(|feature| text_of(feature, "kind") == "vent")
        .find(|feature| feature.get("reachable") == Some(&Json::Bool(true)))
        .map(|feature| text_of(&feature, "feature_id"))
        .expect("seat 0 reaches a vent")
}

/// Play the Push to its end and, when the match goes on, open the next Lull.
fn play_to_the_next_lull(surface: &mut Surface, round: u32) {
    surface.begin_push().expect("the Push begins");
    let ticks = surface
        .host()
        .expect("hosted")
        .world()
        .match_state()
        .segment_ticks();
    assert_eq!(support::step(surface, ticks), ticks, "the Push plays out");
    assert_eq!(support::step(surface, 1), 0, "the Push has ended");
    if round < ROUNDS {
        surface.end_recap().expect("the recap ends");
        surface
            .set_phase_remaining_ms(Ms::new(LULL_MS))
            .expect("a custom match takes a countdown");
        surface.open_lull().expect("the next round's Lull");
    }
}

/// The match of this file, resumed from `saved` on a fresh surface and host.
fn resumed_from(saved: &pharmakos_gateway::save::SavedMatch) -> Surface {
    let host = pharmakos_gateway::host::Host::open(
        &pharmakos_sim::world::WorldConfig {
            match_seed: support::SEED,
            seats: 2,
            rules: support::rules(),
            match_settings: pharmakos_sim::runner::MatchSettings {
                segment_lengths_ms: vec![SEGMENT_MS],
                round_limit: ROUNDS,
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

/// The reads in the order round `round` makes them: turned by the round, and
/// back to front in an even one, so that over the match each read follows and
/// precedes different ones.
fn in_round_order(mut all: Vec<Read>, round: u32) -> Vec<Read> {
    let turn = usize::try_from(round).expect("a round") % all.len().max(1);
    all.rotate_left(turn);
    if round.is_multiple_of(2) {
        all.reverse();
    }
    all
}

/// A match makes its planning scratch once, at its first read that searches,
/// and keeps it: every read of every Lull searches in that one, and the
/// Pushes between leave it be.
#[test]
fn a_match_makes_one_planning_scratch_and_keeps_it_for_every_lull() {
    let mut surface = support::hosted(2, SEGMENT_MS, ROUNDS);
    assert_eq!(made(&surface), 0, "opening a match makes no scratch");
    assert_eq!(work(&surface), None, "and there is none to count in");
    let vent = a_reachable_vent(&mut surface);
    assert_eq!(made(&surface), 1, "the first read that searches makes it");
    let mut priced = work(&surface).expect("the scratch the first read made");
    assert!(priced > 0, "the map summary searched: {priced}");
    for round in 1..=ROUNDS {
        let now = work(&surface);
        assert!(
            now.is_some_and(|now| now >= priced),
            "round {round}: the scratch the last Lull searched in is this Lull's, with every \
             search still counted: {priced} then, {now:?} now"
        );
        let mut left = LULL_MS;
        for read in in_round_order(reads(&surface, &vent), round) {
            let _ = ask(&mut surface, &mut left, &read);
            let after = work(&surface);
            assert!(
                after.is_some_and(|after| after >= priced),
                "round {round}, {}: the tally fell from {priced} to {after:?}, so the read \
                 searched in another scratch",
                read.0
            );
            priced = after.unwrap_or(priced);
            assert_eq!(
                made(&surface),
                1,
                "round {round}, {}: a planning read made a scratch of its own",
                read.0
            );
        }
        play_to_the_next_lull(&mut surface, round);
        assert_eq!(made(&surface), 1, "round {round}: its Push made no scratch");
        assert_eq!(
            work(&surface),
            Some(priced),
            "round {round}: its Push left the planning scratch as the Lull's reads left it"
        );
    }
}

/// Each planning read grows the tally of the scratch it searched in, so the
/// test above, which holds the tally to never falling, compares a number
/// that moves: a read that priced nothing would hide a scratch swapped
/// beneath it.
#[test]
fn every_searching_read_searches_in_the_matchs_scratch() {
    let mut surface = support::hosted(2, SEGMENT_MS, ROUNDS);
    let vent = a_reachable_vent(&mut surface);
    let mut left = LULL_MS;
    for read in reads(&surface, &vent) {
        let before = work(&surface).expect("made by the map summary");
        let answer = ask(&mut surface, &mut left, &read);
        let after = work(&surface).expect("still there");
        assert!(
            after > before,
            "{} searched nothing ({before} before and after), so it is not a searching read or it \
             was refused: {answer}",
            read.0
        );
    }
}

/// A match whose first planning read comes in a Push makes no scratch for
/// it: outside a Lull the map summary prices no walk and the searching
/// methods are closed.
#[test]
fn a_read_outside_a_lull_makes_no_scratch() {
    let mut surface = support::hosted(2, SEGMENT_MS, ROUNDS);
    let all = reads(&surface, "vent_0_0");
    surface.begin_push().expect("the Push begins");
    assert_eq!(support::step(&mut surface, 1), 1, "a tick of the Push");
    let token = support::seat_token(&mut surface, 0);
    for (what, method, params) in &all {
        let _ = support::call(&mut surface, &token, method, params);
        assert_eq!(made(&surface), 0, "{what} in a Push made a scratch");
        assert_eq!(work(&surface), None, "{what} in a Push searched");
    }
}

/// A read's answer from the scratch the whole match has searched in is the
/// answer of a scratch made for that read alone: at every Lull, each read on
/// the live host against the same read, made alone, on a host resumed from
/// that Lull's save.
#[test]
fn a_read_answers_from_the_matchs_scratch_as_from_one_made_for_it_alone() {
    let mut surface = support::hosted(2, SEGMENT_MS, ROUNDS);
    let vent = a_reachable_vent(&mut surface);
    for round in 1..=ROUNDS {
        let saved = surface.lull_save().expect("a Lull saves");
        let mut left = LULL_MS;
        for read in in_round_order(reads(&surface, &vent), round) {
            let used = ask(&mut surface, &mut left, &read);
            let mut fresh = resumed_from(&saved);
            assert_eq!(made(&fresh), 0, "a resumed match starts with no scratch");
            let mut fresh_left = LULL_MS;
            let alone = ask(&mut fresh, &mut fresh_left, &read);
            assert_eq!(
                made(&fresh),
                1,
                "round {round}, {}: the read alone makes the resumed match's scratch",
                read.0
            );
            assert_eq!(
                used, alone,
                "round {round}, {}: the answer depends on what the scratch held",
                read.0
            );
        }
        assert_eq!(made(&surface), 1, "round {round}: one scratch, still");
        play_to_the_next_lull(&mut surface, round);
    }
}
