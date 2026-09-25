// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Save and resume, in process (T17; decisions-log item 84; the wave-6 notes,
//! section A2).
//!
//! The surface builds a save at the two Lull boundaries -- a `sealed` one as a
//! Push begins, a `lull` one when the host asks -- and a new surface is built
//! over it by `Surface::resume`. Every save here goes through the file's own
//! text (`Save::render`, then `Save::parse` and `Save::check`), so what is
//! tested is what a resume reads off the disk. The host loop's half -- the
//! pipe, the file, the resume line -- is `tests/host_loop.rs`'s.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use pharmakos_gateway::error::Code;
use pharmakos_gateway::fog::FogPolicy;
use pharmakos_gateway::host::{Host, Settings};
use pharmakos_gateway::save::{Boundary, Save, SavedMatch, Stamp};
use pharmakos_gateway::serve::Config;
use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::token::Subject;
use pharmakos_proto::gp::api::v1::status::Phase;
use pharmakos_proto::json::Json;
use pharmakos_sim::math::quantity::Tick;
use pharmakos_sim::tables::SeatId;

use support::{
    LULL_MS, SEED, admin_token, call, call_in_lull, code, hosted_with, quote, result, rules,
    rules_json, seat_token, text_of, walk_east,
};

/// A segment long enough for a commander to walk: 60 ticks.
const SEGMENT_MS: i32 = 3_000;

/// Three rounds, so that there are three `sealed` boundaries and three `lull`
/// ones, and the last is the match's end.
const ROUNDS: u32 = 3;

/// Both seats of a two-seat match.
const SEATS: [SeatId; 2] = [SeatId::new(0), SeatId::new(1)];

/// The config line the match was hosted with, as a save carries it.
fn config_of(match_id: &str) -> Config {
    Config {
        match_id: match_id.to_owned(),
        seed: SEED,
        seats: 2,
        human_seat: None,
        segment_lengths_ms: vec![SEGMENT_MS],
        round_limit: ROUNDS,
    }
}

/// A new two-seat match in its opening Lull.
fn new_match(match_id: &str) -> Surface {
    hosted_with(match_id, FogPolicy::fogged(), 2, SEGMENT_MS, ROUNDS, None)
}

/// A saved match as its file would carry it: rendered, read back and checked
/// against the config line a resume would repeat.
fn through_the_file(match_id: &str, saved: &SavedMatch) -> SavedMatch {
    let config = config_of(match_id);
    let hash = rules().rules_hash();
    let text = Save {
        stamp: Stamp::current(hash),
        config: config.clone(),
        state: saved.clone(),
    }
    .render();
    let save = Save::parse(&text).expect("a save reads back");
    save.check(&config, hash)
        .expect("the same rules, the same verifier, the same match");
    assert_eq!(&save.state, saved, "the file carries all of it");
    save.state
}

/// A fresh host from the saved config line: the pristine world a resume
/// regenerates.
fn pristine() -> Host {
    Host::open_from(
        &rules_json(),
        SEED,
        2,
        &Settings {
            segment_lengths_ms: vec![SEGMENT_MS],
            round_limit: ROUNDS,
            units_per_seat: 0,
        },
        None,
    )
    .expect("a match")
}

/// Resume a saved match on a new surface.
fn resume(match_id: &str, saved: &SavedMatch) -> Result<Surface, pharmakos_gateway::Error> {
    let state = through_the_file(match_id, saved);
    Surface::resume(
        match_id,
        SEED,
        rules(),
        FogPolicy::fogged(),
        &SEATS,
        pristine(),
        &state,
    )
}

/// What each seat submits in a round: seat 0 walks east a little further every
/// round; seat 1 walks in rounds 1 and 3 and submits nothing in round 2, so
/// round 2 plays the gateway's safe playbook for it.
fn orders(surface: &Surface, round: u32) -> Vec<(u8, String)> {
    let east = i32::try_from(round).unwrap_or(1).saturating_add(3);
    let mut out = vec![(0_u8, walk_east(surface, 0, east, 0))];
    if round != 2 {
        out.push((1, walk_east(surface, 1, 3, 1)));
    }
    out
}

/// Submit this round's orders through `submit_plan`, like a client.
fn plan_round(surface: &mut Surface) {
    let round = surface.time().round;
    let mut left = LULL_MS;
    for (seat, playbook) in orders(surface, round) {
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

/// Play the Push to its end and return its chain.
fn play_push(surface: &mut Surface) -> Vec<(Tick, u64)> {
    let mut chain: Vec<(Tick, u64)> = Vec::new();
    while let Some(report) = surface.step().expect("a tick") {
        chain.push((report.tick, report.hash));
        if report.segment_ended {
            break;
        }
    }
    assert!(!chain.is_empty(), "a Push that played nothing");
    chain
}

/// Close the recap, and open the next Lull if there is one.
fn close_round(surface: &mut Surface) {
    assert!(surface.end_recap().expect("the recap ends"));
    if surface.time().phase == Phase::Lull {
        surface.open_lull().expect("the next Lull");
    }
}

/// Play from where the match stands to its end, with the same orders every
/// run gives, and return every segment's chain from here on.
fn play_on(surface: &mut Surface) -> Vec<(Tick, u64)> {
    let mut chain: Vec<(Tick, u64)> = Vec::new();
    loop {
        match surface.time().phase {
            Phase::Lull => {
                plan_round(surface);
                assert!(surface.begin_push().expect("the Push begins"));
            }
            Phase::Push => {
                chain.extend(play_push(surface));
                close_round(surface);
            }
            Phase::Ended => return chain,
            other => panic!("the match stopped in {other:?}"),
        }
    }
}

/// The acceptance line that carries the most weight: a save at **every** Lull
/// boundary of a three-round match, resumed and played on with the same
/// submissions, gives the uninterrupted run's chain tick for tick.
///
/// Six saves: each round's `lull` save taken as the Lull opens -- before the
/// seats submit, so the resumed match has to be planned again, as it is after
/// a quit -- and each round's `sealed` save taken as its Push begins, which
/// resumes straight into that Push with the saved seals.
#[test]
fn a_save_at_every_lull_boundary_of_a_three_round_match_resumes_to_the_same_chain() {
    const MATCH_ID: &str = "m-t17-chain";
    let mut surface = new_match(MATCH_ID);
    let mut lulls: Vec<SavedMatch> = Vec::new();
    let mut sealed: Vec<SavedMatch> = Vec::new();
    let mut segments: Vec<Vec<(Tick, u64)>> = Vec::new();
    while surface.time().phase == Phase::Lull {
        lulls.push(surface.lull_save().expect("a Lull saves"));
        plan_round(&mut surface);
        assert!(surface.begin_push().expect("the Push begins"));
        let pending = surface.take_persistence();
        let save = pending.save.expect("a Push's beginning is saved");
        assert_eq!(save.boundary, Boundary::Sealed);
        assert_eq!(pending.sealed.len(), 2, "both seats' seals, for the replay");
        sealed.push(save);
        segments.push(play_push(&mut surface));
        let chains = surface.take_persistence().chains;
        assert_eq!(
            chains.len(),
            1,
            "the segment's chain is handed to the host loop at its end"
        );
        assert_eq!(
            chains.first().map(|chain| chain.ticks.clone()),
            segments.last().cloned(),
            "and it is the chain the Push played"
        );
        close_round(&mut surface);
    }
    assert_eq!(surface.time().phase, Phase::Ended);
    assert_eq!(segments.len(), 3, "three rounds, three segments");
    assert_eq!(lulls.len(), 3);

    for round in 0..3_usize {
        let expected: Vec<(Tick, u64)> = segments.iter().skip(round).flatten().copied().collect();

        let lull = lulls.get(round).expect("a lull save");
        assert_eq!(lull.boundary, Boundary::Lull);
        let mut resumed = resume(MATCH_ID, lull).expect("a Lull resumes");
        assert_eq!(resumed.time().phase, Phase::Lull, "round {}", round + 1);
        assert_eq!(
            resumed.snapshot_bytes().expect("a snapshot"),
            lull.snapshot,
            "the planning snapshot every report is taken over is the saved one, byte for byte"
        );
        assert_eq!(
            play_on(&mut resumed),
            expected,
            "resumed from round {}'s Lull",
            round + 1
        );

        let save = sealed.get(round).expect("a sealed save");
        let mut resumed = resume(MATCH_ID, save).expect("a Push resumes");
        assert_eq!(resumed.time().phase, Phase::Push, "round {}", round + 1);
        assert_eq!(
            play_on(&mut resumed),
            expected,
            "resumed from round {}'s Push",
            round + 1
        );
    }
}

/// A `lull` save carries every seat's notebook, drafts and verified
/// submission, and a resumed Lull hands each seat its own back -- the carried
/// draft included, re-verified, and readable over the wire by `get_draft`,
/// which is how a restarted client opens it.
#[test]
fn a_resumed_lull_has_the_seats_notebooks_drafts_and_seals() {
    const MATCH_ID: &str = "m-t17-stores";
    let mut surface = new_match(MATCH_ID);
    // Round 1: seat 0 seals its own playbook, so round 2 opens with it
    // carried.
    plan_round(&mut surface);
    assert!(surface.begin_push().expect("the Push begins"));
    let _ = play_push(&mut surface);
    close_round(&mut surface);
    assert_eq!(surface.time().round, 2);

    fill_the_stores(&mut surface);

    let saved = surface.lull_save().expect("a Lull saves");
    let mut resumed = resume(MATCH_ID, &saved).expect("resumed");
    assert_eq!(resumed.time().phase, Phase::Lull);
    assert_eq!(resumed.time().round, 2);

    for seat in SEATS {
        let before = surface
            .seat_state(Subject::Seat(seat), seat)
            .expect("its own");
        let after = resumed
            .seat_state(Subject::Seat(seat), seat)
            .expect("its own");
        assert_eq!(after.notebook, before.notebook, "seat {}", seat.raw());
        assert_eq!(after.drafts, before.drafts, "seat {}", seat.raw());
        assert_eq!(
            after.sealed,
            before.sealed,
            "seat {}: the seal, recompiled to the same plan",
            seat.raw()
        );
        assert_eq!(after.continuity, before.continuity, "seat {}", seat.raw());
        assert!(!after.ready, "ready is the new process's to say");
    }
    let zero_after = resumed
        .seat_state(Subject::Seat(SeatId::new(0)), SeatId::new(0))
        .expect("its own");
    assert_eq!(zero_after.notebook, "hold the east vent");
    assert_eq!(
        zero_after.sealed.as_ref().map(|sealed| sealed.round),
        Some(2),
        "this round's verified submission, which the seat may still replace"
    );
    assert!(zero_after.draft("carried").is_some());
    assert!(zero_after.draft("plan-b").is_some());

    // Over the wire, with a token the new process minted.
    let token = seat_token(&mut resumed, 0);
    let mut left = LULL_MS;
    let carried = result(
        &call_in_lull(
            &mut resumed,
            &token,
            &mut left,
            "get_draft",
            r#"{"draft_id":"carried"}"#,
        ),
        "get_draft",
    );
    let original = surface
        .seat_state(Subject::Seat(SeatId::new(0)), SeatId::new(0))
        .expect("its own")
        .draft("carried")
        .expect("carried")
        .playbook_jsonc
        .clone();
    assert_eq!(text_of(&carried, "playbook_jsonc"), original);
    let listed = result(
        &call_in_lull(&mut resumed, &token, &mut left, "list_drafts", "{}"),
        "list_drafts",
    );
    assert_eq!(support::array_of(&listed, "drafts").len(), 2);

    // And the submission is what the Push seals.
    assert!(resumed.begin_push().expect("the Push begins"));
    let playing = resumed
        .host()
        .expect("a match")
        .world()
        .interpreter()
        .plan(0)
        .cloned();
    assert_eq!(
        playing.as_ref(),
        zero_after_sealed_plan(&surface).as_ref(),
        "the saved submission is what the resumed match plays"
    );
}

/// Round 2's Lull, filled: both seats write their notebooks, seat 0 saves a
/// draft of its own beside the carried one and submits a playbook.
fn fill_the_stores(surface: &mut Surface) {
    let zero = seat_token(surface, 0);
    let one = seat_token(surface, 1);
    let mut left = LULL_MS;
    let _ = result(
        &call_in_lull(
            surface,
            &zero,
            &mut left,
            "save_notes",
            r#"{"notes":"hold the east vent"}"#,
        ),
        "save_notes",
    );
    let _ = result(
        &call_in_lull(
            surface,
            &one,
            &mut left,
            "save_notes",
            r#"{"notes":"seat one's own"}"#,
        ),
        "save_notes",
    );
    let later = walk_east(surface, 0, 2, 2);
    let _ = result(
        &call_in_lull(
            surface,
            &zero,
            &mut left,
            "save_draft",
            &format!(
                r#"{{"playbook_jsonc":{},"label":"plan b","draft_id":"plan-b"}}"#,
                quote(&later)
            ),
        ),
        "save_draft",
    );
    let submitted = walk_east(surface, 0, 5, 1);
    let answer = result(
        &call_in_lull(
            surface,
            &zero,
            &mut left,
            "submit_plan",
            &format!(r#"{{"playbook_jsonc":{}}}"#, quote(&submitted)),
        ),
        "submit_plan",
    );
    assert_eq!(answer.get("accepted"), Some(&Json::Bool(true)));
}

/// Seat 0's sealed plan in a surface, as the match would be handed it.
fn zero_after_sealed_plan(surface: &Surface) -> Option<pharmakos_sim::interpreter::Plan> {
    surface
        .seat_state(Subject::Seat(SeatId::new(0)), SeatId::new(0))
        .ok()?
        .sealed
        .as_ref()
        .map(|sealed| sealed.plan.clone())
}

/// A `sealed` save lands in its Push with its seals, and no Lull is offered:
/// planning is closed, the lobby cannot end a Lull that is not there, and the
/// match plays the orders that were sealed.
#[test]
fn a_sealed_save_resumes_into_its_push_and_offers_no_lull() {
    const MATCH_ID: &str = "m-t17-sealed";
    let mut surface = new_match(MATCH_ID);
    plan_round(&mut surface);
    assert!(surface.begin_push().expect("the Push begins"));
    let saved = surface
        .take_persistence()
        .save
        .expect("a Push's beginning is saved");
    assert_eq!(saved.boundary, Boundary::Sealed);

    let mut resumed = resume(MATCH_ID, &saved).expect("resumed");
    assert_eq!(resumed.time().phase, Phase::Push);
    assert_eq!(resumed.time().round, 1);
    for seat in [0_u8, 1] {
        assert_eq!(
            resumed
                .host()
                .expect("a match")
                .world()
                .interpreter()
                .plan(usize::from(seat)),
            surface
                .host()
                .expect("a match")
                .world()
                .interpreter()
                .plan(usize::from(seat)),
            "seat {seat} plays what it sealed"
        );
    }

    let token = seat_token(&mut resumed, 0);
    let refused = call(
        &mut resumed,
        &token,
        "submit_plan",
        &format!(
            r#"{{"playbook_jsonc":{}}}"#,
            quote(pharmakos_gateway::host::SAFE_PLAYBOOK)
        ),
    );
    assert_eq!(
        code(&refused),
        "PHASE_CLOSED",
        "nothing sealed can be re-planned"
    );
    let lobby = admin_token(&mut resumed);
    let refused = call(&mut resumed, &lobby, "end_lull", "{}");
    assert_eq!(code(&refused), "PHASE_CLOSED", "there is no Lull to end");
    let status = result(
        &call(&mut resumed, &token, "get_status", "{}"),
        "get_status",
    );
    assert_eq!(
        status
            .get("status")
            .and_then(|footer| footer.get("phase"))
            .cloned(),
        Some(Json::String(String::from("push")))
    );
    // The Push plays the same first tick it played before the restart.
    let before = surface.step().expect("a tick").expect("a Push tick");
    let after = resumed.step().expect("a tick").expect("a Push tick");
    assert_eq!((after.tick, after.hash), (before.tick, before.hash));
}

/// The fingerprint check, named for what it catches: a seal whose text no
/// longer matches the fingerprint saved beside it -- a damaged file, or a
/// canonical form that changed between builds -- is refused, and so is a
/// fingerprint that was damaged instead. Both come from the same file, so this
/// is not tamper-evidence, and the test does not pretend it is.
#[test]
fn a_damaged_seal_in_a_save_is_refused() {
    const MATCH_ID: &str = "m-t17-damaged";
    let mut surface = new_match(MATCH_ID);
    plan_round(&mut surface);
    assert!(surface.begin_push().expect("the Push begins"));
    let saved = surface
        .take_persistence()
        .save
        .expect("a Push's beginning is saved");
    assert!(
        resume(MATCH_ID, &saved).is_ok(),
        "the undamaged save resumes"
    );

    // The seal's text damaged: still a playbook, still compiles, not the one
    // that was sealed.
    let mut damaged = saved.clone();
    let seal = damaged
        .seats
        .first_mut()
        .and_then(|seat| seat.seal.as_mut())
        .expect("seat 0 sealed");
    let text = seal
        .playbook_jsonc
        .replace("\"timeout_ms\": 60000", "\"timeout_ms\": 59000");
    assert_ne!(text, seal.playbook_jsonc, "the text was damaged");
    seal.playbook_jsonc = text;
    let error = resume(MATCH_ID, &damaged).expect_err("a damaged seal");
    assert_eq!(error.code, Code::InvalidArgument);
    assert!(error.message.contains("fingerprint"), "{}", error.message);

    // The fingerprint damaged instead.
    let mut damaged = saved.clone();
    let seal = damaged
        .seats
        .get_mut(1)
        .and_then(|seat| seat.seal.as_mut())
        .expect("seat 1 sealed");
    seal.plan_fingerprint ^= 1;
    let error = resume(MATCH_ID, &damaged).expect_err("a damaged fingerprint");
    assert_eq!(error.code, Code::InvalidArgument);
    assert!(error.message.contains("seat 1"), "{}", error.message);

    // And a planning snapshot that is not the saved one does not resume.
    let mut damaged = saved;
    if let Some(byte) = damaged.snapshot.last_mut() {
        *byte ^= 0xff;
    }
    let error = resume(MATCH_ID, &damaged).expect_err("a damaged snapshot");
    assert_eq!(error.code, Code::InvalidArgument, "{}", error.message);
}

/// Decisions-log item 110 (5): v1 has one to three seats, and a lobby that asks
/// for a fourth -- or none -- is refused before a map is generated.
#[test]
fn a_fourth_seat_is_refused() {
    for seats in [0_u32, 4, 99] {
        let line = format!("m-t17-seats\t0x1\t{seats}\t-\t-\t2\n");
        let error = Config::parse(&line).expect_err("refused");
        assert_eq!(error.code, Code::InvalidArgument, "{seats} seats");
        let error = Host::open_from(
            &rules_json(),
            SEED,
            seats,
            &Settings {
                segment_lengths_ms: vec![SEGMENT_MS],
                round_limit: 2,
                units_per_seat: 0,
            },
            None,
        )
        .expect_err("refused");
        assert_eq!(error.code, Code::InvalidArgument, "{seats} seats");
        assert!(
            error.message.contains("between 1 and 3"),
            "{}",
            error.message
        );
    }
    let three = Config::parse("m-t17-seats\t0x1\t3\t-\t-\t2\n").expect("three is v1's most");
    assert_eq!(three.seats, 3);
    assert!(
        Host::open_from(
            &rules_json(),
            SEED,
            3,
            &Settings {
                segment_lengths_ms: vec![SEGMENT_MS],
                round_limit: 2,
                units_per_seat: 0,
            },
            None,
        )
        .is_ok()
    );
}

/// A Lull's save carries what the Lull has spent, so the gateway's tick -- and
/// with it the audit log's stamps and every rate budget -- does not go back on
/// a resume.
#[test]
fn a_resumed_match_keeps_its_tick_monotonic() {
    const MATCH_ID: &str = "m-t17-tick";
    let mut surface = new_match(MATCH_ID);
    let token = seat_token(&mut surface, 0);
    let mut left = LULL_MS;
    for _ in 0..20 {
        let _ = call_in_lull(&mut surface, &token, &mut left, "get_status", "{}");
    }
    let before = surface.time().tick;
    assert!(before.raw() > 0, "the Lull has spent something");
    let saved = surface.lull_save().expect("a Lull saves");
    let resumed = resume(MATCH_ID, &saved).expect("resumed");
    assert_eq!(resumed.time().tick, before);
}
