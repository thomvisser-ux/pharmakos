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
use pharmakos_gateway::save::{Boundary, Continuation, Save, SavedMatch, Stamp};
use pharmakos_gateway::serve::Config;
use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::token::{Subject, Token};
use pharmakos_proto::gp::api::v1::status::Phase;
use pharmakos_proto::json::Json;
use pharmakos_sim::math::quantity::Tick;
use pharmakos_sim::runner::MatchSettings;
use pharmakos_sim::tables::SeatId;
use pharmakos_sim::world::WorldConfig;

use support::{
    FIRST_LULL_MS, SEED, admin_token, call, call_in_lull, code, hosted_with, quote, result, rules,
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
        preset: pharmakos_gateway::probation::Preset::Custom,
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
        },
        None,
    )
    .expect("a match")
}

/// What these tests resume with: a generation the host loop could pass (the
/// audit log's last sequence number), and no tick floor, so the saved offset
/// alone sets the resumed tick.
const CONTINUATION: Continuation = Continuation {
    generation: 1,
    last_tick: 0,
};

/// Resume a saved match on a new surface.
fn resume(match_id: &str, saved: &SavedMatch) -> Result<Surface, pharmakos_gateway::Error> {
    let state = through_the_file(match_id, saved);
    {
        let host = pristine();
        Surface::new(
            match_id,
            SEED,
            host.rules().clone(),
            FogPolicy::fogged(),
            &SEATS,
        )
        .expect("a surface")
        .resume(host, &state, CONTINUATION)
    }
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
    let mut left = FIRST_LULL_MS;
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
/// Nine saves, three a round: the `lull` save taken as the Lull opens --
/// before the seats submit, so the resumed match has to be planned again, as
/// it is after a quit; a second `lull` save taken once every seat's submission
/// is verified and before the Push begins, which resumes into a Lull that
/// already holds them and begins its Push without a single new submission;
/// and the `sealed` save taken as the Push begins, which resumes straight into
/// that Push with the saved seals.
#[test]
fn a_save_at_every_lull_boundary_of_a_three_round_match_resumes_to_the_same_chain() {
    const MATCH_ID: &str = "m-t17-chain";
    let mut surface = new_match(MATCH_ID);
    let mut lulls: Vec<SavedMatch> = Vec::new();
    let mut planned: Vec<SavedMatch> = Vec::new();
    let mut sealed: Vec<SavedMatch> = Vec::new();
    let mut segments: Vec<Vec<(Tick, u64)>> = Vec::new();
    while surface.time().phase == Phase::Lull {
        lulls.push(surface.lull_save().expect("a Lull saves"));
        plan_round(&mut surface);
        planned.push(surface.lull_save().expect("a planned Lull saves"));
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

        // The Lull quit after every seat had submitted: the saved submissions
        // are what its Push seals, with nothing submitted again.
        let lull = planned.get(round).expect("a planned lull save");
        assert_eq!(lull.boundary, Boundary::Lull);
        let mut resumed = resume(MATCH_ID, lull).expect("a planned Lull resumes");
        assert_eq!(resumed.time().phase, Phase::Lull, "round {}", round + 1);
        assert!(
            resumed.begin_push().expect("the Push begins"),
            "the saved submissions carry the resumed Lull into its Push"
        );
        let mut chain = play_push(&mut resumed);
        close_round(&mut resumed);
        chain.extend(play_on(&mut resumed));
        assert_eq!(
            chain,
            expected,
            "resumed from round {}'s planned Lull, with its saved submissions",
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
        assert_eq!(after.notebook(), before.notebook(), "seat {}", seat.raw());
        assert_eq!(after.drafts(), before.drafts(), "seat {}", seat.raw());
        assert_eq!(
            after.sealed(),
            before.sealed(),
            "seat {}: the seal, recompiled to the same plan",
            seat.raw()
        );
        assert_eq!(
            after.continuity(),
            before.continuity(),
            "seat {}",
            seat.raw()
        );
        assert!(!after.ready(), "ready is the new process's to say");
    }
    let zero_after = resumed
        .seat_state(Subject::Seat(SeatId::new(0)), SeatId::new(0))
        .expect("its own");
    assert_eq!(zero_after.notebook(), "hold the east vent");
    assert_eq!(
        zero_after
            .sealed()
            .map(pharmakos_gateway::surface::Sealed::round),
        Some(2),
        "this round's verified submission, which the seat may still replace"
    );
    assert!(zero_after.draft("carried").is_some());
    assert!(zero_after.draft("plan-b").is_some());

    // Over the wire, with a token the new process minted.
    let token = seat_token(&mut resumed, 0);
    let mut left = FIRST_LULL_MS;
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
    let mut left = FIRST_LULL_MS;
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
        .sealed()
        .map(|sealed| sealed.plan().clone())
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
            quote(
                pharmakos_gateway::host::safe_playbook()
                    .expect("the fallback")
                    .as_str()
            )
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
/// A seat names only other seats' beacons by `e_NN`: a saved handle table
/// that names one of the seat's own is not one this gateway wrote, and the
/// resume is refused with a typed error (S3's plan, E1).
#[test]
fn a_handle_table_naming_the_seats_own_beacon_is_refused() {
    const MATCH_ID: &str = "m-gwp-own-handle";
    let surface = new_match(MATCH_ID);
    let saved = surface.lull_save().expect("a Lull saves");
    let (core, _) = support::core_beacon(&surface, 0);
    let mut damaged = saved.clone();
    if let Some(seat) = damaged.seats.first_mut() {
        seat.handles = vec![(core.raw(), String::from("e_01"))];
    }
    let error = resume(MATCH_ID, &damaged).expect_err("its own beacon");
    assert_eq!(error.code(), Code::InvalidArgument);
    assert!(
        error.message().contains("its own beacon"),
        "{}",
        error.message()
    );
    assert!(
        resume(MATCH_ID, &saved).is_ok(),
        "the save as written resumes"
    );
}

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
    assert_eq!(error.code(), Code::InvalidArgument);
    assert!(
        error.message().contains("fingerprint"),
        "{}",
        error.message()
    );

    // The fingerprint damaged instead.
    let mut damaged = saved.clone();
    let seal = damaged
        .seats
        .get_mut(1)
        .and_then(|seat| seat.seal.as_mut())
        .expect("seat 1 sealed");
    seal.plan_fingerprint ^= 1;
    let error = resume(MATCH_ID, &damaged).expect_err("a damaged fingerprint");
    assert_eq!(error.code(), Code::InvalidArgument);
    assert!(error.message().contains("seat 1"), "{}", error.message());

    // And a planning snapshot that is not the saved one does not resume.
    let mut damaged = saved;
    if let Some(byte) = damaged.snapshot.last_mut() {
        *byte ^= 0xff;
    }
    let error = resume(MATCH_ID, &damaged).expect_err("a damaged snapshot");
    assert_eq!(error.code(), Code::InvalidArgument, "{}", error.message());
}

/// A restored seal is decoded once (S3's `vfyq2`, fix pass 2, item F), and
/// each of its three refusals keeps the code and the words it had when the
/// text was decoded twice: no canonical form, a fingerprint that does not
/// match, a text that will not compile.
#[test]
fn every_refusal_of_a_restored_seal_keeps_its_code_and_words() {
    const MATCH_ID: &str = "m-vfyq2-seal-words";
    let mut surface = new_match(MATCH_ID);
    plan_round(&mut surface);
    assert!(surface.begin_push().expect("the Push begins"));
    let saved = surface
        .take_persistence()
        .save
        .expect("a Push's beginning is saved");
    let with_seat_zero = |text: &str, fingerprint: Option<u64>| {
        let mut damaged = saved.clone();
        let seal = damaged
            .seats
            .first_mut()
            .and_then(|seat| seat.seal.as_mut())
            .expect("seat 0 sealed");
        seal.playbook_jsonc = text.to_owned();
        if let Some(fingerprint) = fingerprint {
            seal.plan_fingerprint = fingerprint;
        }
        resume(MATCH_ID, &damaged).expect_err("refused")
    };

    // No canonical form: the decode's own words, inside the save's.
    let error = with_seat_zero("{\"meta\": ", None);
    assert_eq!(error.code(), Code::InvalidArgument);
    let decoded = pharmakos_plan_core::decode_jsonc("{\"meta\": ")
        .expect_err("no canonical form")
        .message()
        .to_owned();
    assert_eq!(
        error.message(),
        format!(
            "seat 0's sealed playbook in this save has no canonical form (a sealed playbook has \
             no canonical form: {decoded}): the file is damaged"
        )
    );

    // A fingerprint that does not match the text beside it.
    let text = std::fs::read_to_string(
        support::workspace_root().join("crates/verifier/tests/cases/e0208_unknown_step_label.json"),
    )
    .expect("the committed case")
    .replace("\r\n", "\n");
    let canonical = pharmakos_plan_core::decode_jsonc(&text).expect("a canonical form");
    let fingerprint =
        pharmakos_sim::hash::plan_fingerprint_of(canonical.decoded()).expect("a fingerprint");
    let error = with_seat_zero(&text, Some(fingerprint ^ 1));
    assert_eq!(error.code(), Code::InvalidArgument);
    assert_eq!(
        error.message(),
        format!(
            "seat 0's sealed playbook in this save does not match the plan fingerprint saved \
             beside it ({} against {}): the file is damaged, or this build writes a different \
             canonical form than the one that saved it",
            pharmakos_sim::hex(fingerprint),
            pharmakos_sim::hex(fingerprint ^ 1)
        )
    );

    // A text whose fingerprint matches and which will not compile: a
    // condition on a step label the route does not have.
    let error = with_seat_zero(&text, Some(fingerprint));
    assert_eq!(error.code(), Code::InvalidArgument);
    let compiled =
        pharmakos_sim::interpreter::Plan::compile(canonical.decoded().message(), &support::rules())
            .expect_err("an unknown label");
    assert_eq!(
        error.message(),
        format!(
            "seat 0's sealed playbook in this save will not compile in this build: this \
             playbook qualifies and this build cannot execute it: {compiled}"
        )
    );
}

/// A restored seal's compile refusal names the label the text wrote quoted
/// (S3's `vfyq2`, fix pass 3, item M2 (a)): ESC, BEL and a right-to-left
/// override in a label come back escaped, never raw.
#[test]
fn a_restored_seals_compile_refusal_quotes_the_label() {
    const MATCH_ID: &str = "m-vfyq2-seal-quoted";
    let mut surface = new_match(MATCH_ID);
    plan_round(&mut surface);
    assert!(surface.begin_push().expect("the Push begins"));
    let saved = surface
        .take_persistence()
        .save
        .expect("a Push's beginning is saved");
    let label = "\u{1b}]0;owned\u{7}\u{202e}zz";
    let text = std::fs::read_to_string(
        support::workspace_root().join("crates/verifier/tests/cases/e0208_unknown_step_label.json"),
    )
    .expect("the committed case")
    .replace("\r\n", "\n")
    .replace(
        "\"nowhere\"",
        &pharmakos_proto::json::write(&Json::String(label.to_owned())),
    );
    let canonical = pharmakos_plan_core::decode_jsonc(&text).expect("a canonical form");
    let fingerprint =
        pharmakos_sim::hash::plan_fingerprint_of(canonical.decoded()).expect("a fingerprint");
    let mut damaged = saved;
    let seal = damaged
        .seats
        .first_mut()
        .and_then(|seat| seat.seal.as_mut())
        .expect("seat 0 sealed");
    seal.playbook_jsonc = text;
    seal.plan_fingerprint = fingerprint;
    let error = resume(MATCH_ID, &damaged).expect_err("refused");
    assert_eq!(error.code(), Code::InvalidArgument);
    for raw in ['\u{1b}', '\u{7}', '\u{202e}'] {
        assert!(!error.message().contains(raw), "{:?}", error.message());
    }
    assert!(error.message().contains("\\u001b"), "{:?}", error.message());
}

/// The notes' A2 classifies `views` as derived, with "every cursor and handle
/// from the dead process stale". A resumed process builds a fresh feed and
/// attaches it once, over the same match id, seed and map as the dead
/// process's first attach, so without the resume's generation in the view's
/// identity the two views would share an id and the old cursor would be read
/// as a place in the new view. It is `STALE_SNAPSHOT`, and a keyframe is the
/// client's way back.
#[test]
fn a_cursor_from_the_dead_process_is_stale_on_the_resumed_surface() {
    const MATCH_ID: &str = "m-t17-stale";
    let mut surface = new_match(MATCH_ID);
    let token = seat_token(&mut surface, 0);
    let mut left = FIRST_LULL_MS;
    let page = result(
        &call_in_lull(
            &mut surface,
            &token,
            &mut left,
            "get_view",
            r#"{"cursor":""}"#,
        ),
        "get_view",
    );
    let cursor = text_of(&page, "next_cursor");
    let saved = surface.lull_save().expect("a Lull saves");

    let mut resumed = resume(MATCH_ID, &saved).expect("resumed");
    let token = seat_token(&mut resumed, 0);
    let mut left = FIRST_LULL_MS;
    let response = call_in_lull(
        &mut resumed,
        &token,
        &mut left,
        "get_view",
        &format!(r#"{{"cursor":"{cursor}"}}"#),
    );
    assert_eq!(
        code(&response),
        "STALE_SNAPSHOT",
        "a cursor the dead process issued is not a place in the resumed view"
    );
    let keyframe = call_in_lull(
        &mut resumed,
        &token,
        &mut left,
        "get_view",
        r#"{"cursor":""}"#,
    );
    let fresh = text_of(&result(&keyframe, "get_view"), "next_cursor");
    assert_ne!(fresh, cursor, "the resumed view is another view");
}

/// Decisions-log item 110 (5): v1 has one to three seats, and a lobby that asks
/// for a fourth -- or none -- is refused before a map is generated.
#[test]
fn a_fourth_seat_is_refused() {
    for seats in [0_u32, 4, 99] {
        let line = format!("m-t17-seats\t0x1\t{seats}\t-\t-\t2\tcustom\n");
        let error = Config::parse(&line).expect_err("refused");
        assert_eq!(error.code(), Code::InvalidArgument, "{seats} seats");
        let error = Host::open_from(
            &rules_json(),
            SEED,
            seats,
            &Settings {
                segment_lengths_ms: vec![SEGMENT_MS],
                round_limit: 2,
            },
            None,
        )
        .expect_err("refused");
        assert_eq!(error.code(), Code::InvalidArgument, "{seats} seats");
        assert!(
            error.message().contains("between 1 and 3"),
            "{}",
            error.message()
        );
        // And the door `gamectl scenario run` and `seat doctor` take, which
        // is handed a world config rather than a lobby's numbers.
        let world = WorldConfig {
            match_seed: SEED,
            seats,
            rules: rules(),
            match_settings: MatchSettings {
                segment_lengths_ms: vec![SEGMENT_MS],
                round_limit: 2,
            },
        };
        let error = Host::open(&world, None).expect_err("refused by Host::open too");
        assert_eq!(error.code(), Code::InvalidArgument, "{seats} seats");
        assert!(
            error.message().contains("between 1 and 3"),
            "{}",
            error.message()
        );
    }
    let three =
        Config::parse("m-t17-seats\t0x1\t3\t-\t-\t2\tcustom\n").expect("three is v1's most");
    assert_eq!(three.seats, 3);
    assert!(
        Host::open_from(
            &rules_json(),
            SEED,
            3,
            &Settings {
                segment_lengths_ms: vec![SEGMENT_MS],
                round_limit: 2,
            },
            None,
        )
        .is_ok()
    );
}

/// A `sealed` save's Push is played again from its first tick, and the dead
/// process may have stamped the audit log well into it. The resumed gateway
/// tick starts no lower than the log's last, so the stamps do not go back.
#[test]
fn a_sealed_resume_starts_no_lower_than_the_audit_logs_last_tick() {
    const MATCH_ID: &str = "m-t17-tick-sealed";
    let mut surface = new_match(MATCH_ID);
    plan_round(&mut surface);
    assert!(surface.begin_push().expect("the Push begins"));
    let save = surface
        .take_persistence()
        .save
        .expect("a Push's beginning is saved");
    let at_the_push = surface.time().tick;
    // The dead process played part of that Push and audited as it went.
    for _ in 0..10 {
        let _ = surface.step().expect("a tick");
    }
    let dead = surface.time().tick;
    assert!(dead > at_the_push, "the Push moved the gateway's tick");

    let state = through_the_file(MATCH_ID, &save);
    let floor = Continuation {
        generation: 7,
        last_tick: dead.raw(),
    };
    let resumed = {
        let host = pristine();
        Surface::new(
            MATCH_ID,
            SEED,
            host.rules().clone(),
            FogPolicy::fogged(),
            &SEATS,
        )
        .expect("a surface")
        .resume(host, &state, floor)
    }
    .expect("a Push resumes");
    assert_eq!(resumed.time().phase, Phase::Push);
    assert_eq!(
        resumed.time().tick,
        dead,
        "the resumed Push stamps from where the dead process's log stopped"
    );
    // Without a floor, the saved offset alone puts it back at the Push's
    // first tick -- which is what the floor is for.
    let unfloored = resume(MATCH_ID, &save).expect("a Push resumes");
    assert_eq!(unfloored.time().tick, at_the_push);
}

/// A Lull's save carries what the Lull has spent, so the gateway's tick -- and
/// with it the audit log's stamps and every rate budget -- does not go back on
/// a resume.
#[test]
fn a_resumed_match_keeps_its_tick_monotonic() {
    const MATCH_ID: &str = "m-t17-tick";
    let mut surface = new_match(MATCH_ID);
    let token = seat_token(&mut surface, 0);
    let mut left = FIRST_LULL_MS;
    for _ in 0..20 {
        let _ = call_in_lull(&mut surface, &token, &mut left, "get_status", "{}");
    }
    let before = surface.time().tick;
    assert!(before.raw() > 0, "the Lull has spent something");
    let saved = surface.lull_save().expect("a Lull saves");
    let resumed = resume(MATCH_ID, &saved).expect("resumed");
    assert_eq!(resumed.time().tick, before);
}

/// The golden case the save's bytes are pinned by: one match's round-1 saves,
/// a `lull` one with both seats' seals and the `sealed` one its Push begins
/// with (S3's `fpstream`, part 2).
const SAVE_GOLDEN: &str = "save_round_one";

/// The committed save of `SAVE_GOLDEN`, `file` being `lull` or `sealed`.
fn committed_save(file: &str) -> std::path::PathBuf {
    support::workspace_root()
        .join("tests")
        .join("golden")
        .join("gateway")
        .join(SAVE_GOLDEN)
        .join(format!("expected.{file}.json"))
}

/// A save's file text as the golden holds it: the render and the one newline
/// every golden ends with (`tests/golden/README.md`, rule 3).
fn save_text(match_id: &str, state: &SavedMatch) -> String {
    let mut text = Save {
        stamp: Stamp::current(rules().rules_hash()),
        config: config_of(match_id),
        state: state.clone(),
    }
    .render();
    text.push('\n');
    text
}

/// Write the fresh save and compare it with the committed one, byte for byte.
fn save_golden(file: &str, text: &str) {
    assert!(!text.contains('\r'), "a save is written with LF endings");
    let fresh = support::target_dir()
        .join("golden")
        .join("gateway")
        .join(SAVE_GOLDEN);
    std::fs::create_dir_all(&fresh).expect("the fresh output's folder");
    std::fs::write(fresh.join(format!("actual.{file}.json")), text).expect("the fresh output");
    let committed = committed_save(file);
    let Ok(expected) = std::fs::read_to_string(&committed) else {
        panic!(
            "no committed golden at {}. A new case is committed by copying the fresh output \
             beside it, byte for byte, once it is right; `cargo xtask golden --bless` rewrites \
             only goldens that exist.",
            committed.display()
        );
    };
    assert!(
        expected == text,
        "the save at {} moved. tests/golden/gateway/README.md says what a diff means; the pull \
         request has to say which input moved it.",
        committed.display()
    );
}

/// A save is a format on players' disks (AGENTS.md section 5): this build
/// writes the same bytes for the same match as the build that committed the
/// golden, and a save that build wrote resumes here, its seals carrying the
/// fingerprints stamped beside them, and is written again byte for byte.
///
/// The golden was written by `main` at `bbddd094`, before the seal carried
/// its fingerprint (S3's `fpstream`, part 2), so the `plan_fingerprint` lines
/// it holds are the ones the save writer used to recompute from each seal's
/// text: this test failing is a fingerprint value, or the save's format,
/// moving. It was re-blessed once, when lane `retreat` (#230) moved
/// `VERIFIER_VERSION` from `0.1.0-s3.5` to `0.1.0-s3.6`: the stamp's
/// `verifier_version` and the two `report_hash` lines moved with it, which
/// hashes the version, and no `plan_fingerprint` line moved. Re-blessed again
/// by lane `opsave`, which made the save format version 4: the stamp's
/// `format_version` moved from 3 to 4 and each file gained its `last_push`
/// member (the `lull` file the Lull's feed, the `sealed` file none, both no
/// recap in round 1); no other line moved. Re-blessed a third time by lane
/// `vcap`, which moved `VERIFIER_VERSION` from `0.1.0-s3.6` to `0.1.0-s3.7`:
/// the stamp's `verifier_version` and the two `report_hash` lines again, and
/// nothing else. Main's version-3 file is kept as
/// `tests/fixtures/save-v3-round-one-lull.json`, and still resumes
/// (`a_version_3_save_written_by_main_still_resumes`).
#[test]
fn a_save_is_written_byte_for_byte_and_a_committed_save_resumes() {
    const MATCH_ID: &str = "m-fpstream2-save";
    let mut surface = new_match(MATCH_ID);
    plan_round(&mut surface);
    let lull = surface.lull_save().expect("a Lull saves");
    assert!(lull.seats.iter().all(|seat| seat.seal.is_some()));
    save_golden("lull", &save_text(MATCH_ID, &lull));
    assert!(surface.begin_push().expect("the Push begins"));
    let sealed = surface
        .take_persistence()
        .save
        .expect("a Push's beginning is saved");
    assert_eq!(sealed.boundary, Boundary::Sealed);
    save_golden("sealed", &save_text(MATCH_ID, &sealed));

    // The committed files, read as a resume reads them.
    for file in ["lull", "sealed"] {
        let text = std::fs::read_to_string(committed_save(file)).expect("the committed save");
        let save = Save::parse(text.trim_end_matches('\n')).expect("the committed save reads");
        save.check(&config_of(MATCH_ID), rules().rules_hash())
            .expect("the same rules, the same verifier, the same match");
        let resumed = {
            let host = pristine();
            Surface::new(
                MATCH_ID,
                SEED,
                host.rules().clone(),
                FogPolicy::fogged(),
                &SEATS,
            )
            .expect("a surface")
            .resume(host, &save.state, CONTINUATION)
        }
        .expect("the committed save resumes");
        for (seat, saved) in SEATS.iter().zip(&save.state.seats) {
            let held = resumed
                .seat_state(Subject::Seat(*seat), *seat)
                .expect("its own")
                .sealed()
                .expect("the seal is back");
            let stamped = saved.seal.as_ref().expect("a saved seal");
            assert_eq!(held.playbook_jsonc(), stamped.playbook_jsonc);
            assert_eq!(held.report_hash(), stamped.report_hash.as_slice());
        }
        // A Lull resumed from main's file is saved again as main saved it.
        if file == "lull" {
            let again = resumed.lull_save().expect("a resumed Lull saves");
            assert!(
                save_text(MATCH_ID, &again) == text,
                "a resumed Lull's save is not the save it was resumed from"
            );
        }
    }
}

/// A saved notebook is held to the rules' bound by its count, never through a
/// cast that saturates (S3's `fpstream`, part 2, fix pass 3): a notebook at the
/// bound resumes, and one a character past it is refused as a damaged file,
/// with the words a save past a bound has always had.
#[test]
fn a_saved_notebook_past_its_bound_is_refused() {
    const MATCH_ID: &str = "m-fpstream2-notebook-bound";
    let bound = rules()
        .message()
        .verifier
        .as_ref()
        .map(|verifier| verifier.notebook_max_chars)
        .expect("a verifier block");
    let bound = usize::try_from(bound).expect("a size");
    let surface = new_match(MATCH_ID);
    let saved = surface.lull_save().expect("a Lull saves");
    let with_notebook = |characters: usize| {
        let mut held = saved.clone();
        let seat = held.seats.first_mut().expect("seat 0");
        // A two-byte character, so a count of bytes would not pass for one of
        // characters.
        seat.notebook = "é".repeat(characters);
        held
    };
    assert!(
        resume(MATCH_ID, &with_notebook(bound)).is_ok(),
        "at the bound"
    );
    let refused = resume(MATCH_ID, &with_notebook(bound.saturating_add(1)))
        .expect_err("a character past the bound");
    assert_eq!(refused.code(), Code::InvalidArgument);
    assert_eq!(
        refused.message(),
        format!(
            "seat 0's notebook in this save is {} characters, past the {bound} a seat's \
             notebook may hold: the file is damaged",
            bound.saturating_add(1)
        )
    );
}

/// A save main wrote before the save carried the last Push (format version 3,
/// `bbddd094`'s round-1 `lull` file, committed as a fixture) still loads and
/// resumes into its Lull, with no last Push (lane `opsave`): the format's
/// version-4 member is additive. A save loads only under the verifier that
/// stamped it, so lane `vcap`'s bump to `0.1.0-s3.7` moved the fixture's
/// `verifier_version` and its two seals' `report_hash` lines (to the
/// `save_round_one` golden's own, the same seals' reports under s3.7); its
/// format is untouched.
#[test]
fn a_version_3_save_written_by_main_still_resumes() {
    const MATCH_ID: &str = "m-fpstream2-save";
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("save-v3-round-one-lull.json"),
    )
    .expect("the version-3 fixture");
    assert!(
        text.contains("\"format_version\": 3"),
        "the fixture is version 3"
    );
    let save = Save::parse(text.trim_end_matches('\n')).expect("a version-3 save reads");
    assert_eq!(
        save.stamp.format_version,
        pharmakos_gateway::save::LAST_PUSH_ABSENT
    );
    assert_eq!(save.state.last_push, None);
    save.check(&config_of(MATCH_ID), rules().rules_hash())
        .expect("the same rules, the same verifier, the same match");
    let host = pristine();
    let resumed = Surface::new(
        MATCH_ID,
        SEED,
        host.rules().clone(),
        FogPolicy::fogged(),
        &SEATS,
    )
    .expect("a surface")
    .resume(host, &save.state, CONTINUATION)
    .expect("a version-3 save resumes");
    assert_eq!(resumed.time().phase, Phase::Lull);
    let again = resumed.lull_save().expect("its Lull saves");
    assert!(again.last_push.is_some(), "and is saved again as version 4");
}

/// One damage of a seat's saved drafts.
type DraftsDamage = Box<dyn Fn(&mut Vec<pharmakos_gateway::surface::Draft>)>;

/// The draft rule across a resume (lane `opsave`, fix passes 1 and 2; the
/// reviews' hosted probes): a seat that keeps 31 drafts of its own and seals
/// carries its seal into the next Lull as the gateway's `carried` draft; that
/// Lull saves and resumes and lists 32, and the carried draft is not counted
/// against the 32 a seat may keep: a 32nd of its own is taken, a 33rd is
/// refused in words that say what the player can do, and so is a save under
/// the carried id. The Lull holding 32 of its own and the carried one saves
/// and resumes and lists all 33; a save holding 33 of its own, or the carried
/// draft twice, is refused typed.
#[test]
fn a_seat_with_every_draft_and_a_seal_resumes_its_next_lull() {
    const MATCH_ID: &str = "m-opsave-drafts";
    let max = pharmakos_gateway::surface::planning::MAX_DRAFTS;
    let mut surface = new_match(MATCH_ID);
    let token = seat_token(&mut surface, 0);
    let mut left = FIRST_LULL_MS;
    let body = walk_east(&surface, 0, 3, 0);
    let save_draft = |surface: &mut Surface, token: &Token, left: &mut i32, id: &str| {
        call_in_lull(
            surface,
            token,
            left,
            "save_draft",
            &format!(
                "{{\"draft_id\":\"{id}\",\"playbook_jsonc\":{}}}",
                quote(&body)
            ),
        )
    };
    for index in 0..max.saturating_sub(1) {
        let answer = save_draft(&mut surface, &token, &mut left, &format!("own-{index}"));
        let _ = result(&answer, "save_draft");
    }
    plan_round(&mut surface);
    assert!(surface.begin_push().expect("the Push begins"));
    let _ = play_push(&mut surface);
    close_round(&mut surface);
    let saved = surface.lull_save().expect("round 2's Lull saves");
    let held = saved.seats.first().expect("seat 0").drafts.len();
    assert_eq!(held, 32, "31 of its own and the carried one");

    let mut resumed = resume(MATCH_ID, &saved).expect("the Lull resumes");
    let token = seat_token(&mut resumed, 0);
    let mut left = FIRST_LULL_MS;
    let listed = result(
        &call_in_lull(&mut resumed, &token, &mut left, "list_drafts", "{}"),
        "list_drafts",
    );
    assert_eq!(support::array_of(&listed, "drafts").len(), 32);
    // At 31 of its own beside the carried one, the 32nd of its own is taken:
    // the carried draft is the gateway's and is not counted against the 32.
    let last = format!("own-{}", max.saturating_sub(1));
    let _ = result(
        &save_draft(&mut resumed, &token, &mut left, &last),
        "the 32nd draft of its own beside the carried one",
    );
    let refused = save_draft(&mut resumed, &token, &mut left, &format!("own-{max}"));
    assert_eq!(code(&refused), "INVALID_ARGUMENT", "{refused:?}");
    assert_eq!(
        text_of(refused.get("error").expect("an error"), "message"),
        format!(
            "this seat holds {max} drafts of its own, as many as it may, and a draft is never \
             deleted: save under one of their ids to replace that draft"
        )
    );
    let carried = save_draft(&mut resumed, &token, &mut left, "carried");
    assert_eq!(
        code(&carried),
        "INVALID_ARGUMENT",
        "the carried id: {carried:?}"
    );
    let _ = result(
        &save_draft(&mut resumed, &token, &mut left, "own-0"),
        "a save over one of its own ids replaces that draft",
    );

    let saved = resumed.lull_save().expect("the full Lull saves");
    let held = saved.seats.first().expect("seat 0").drafts.len();
    assert_eq!(held, 33, "32 of its own and the carried one");
    let mut again = resume(MATCH_ID, &saved).expect("the full Lull resumes");
    let token = seat_token(&mut again, 0);
    let mut left = FIRST_LULL_MS;
    let listed = result(
        &call_in_lull(&mut again, &token, &mut left, "list_drafts", "{}"),
        "list_drafts",
    );
    assert_eq!(support::array_of(&listed, "drafts").len(), 33);
    each_damage_of_the_draft_rule_is_refused(MATCH_ID, &saved);
}

/// A save holding 33 drafts of its own, or the carried draft twice, is
/// refused typed at its read, whatever else it holds.
fn each_damage_of_the_draft_rule_is_refused(match_id: &str, saved: &SavedMatch) {
    let cases: [(&str, DraftsDamage); 2] = [
        (
            "33 drafts of its own",
            Box::new(|drafts: &mut Vec<pharmakos_gateway::surface::Draft>| {
                for draft in drafts.iter_mut() {
                    if draft.draft_id == "carried" {
                        draft.draft_id = String::from("own-extra");
                    }
                }
            }),
        ),
        (
            "the carried draft twice",
            Box::new(|drafts: &mut Vec<pharmakos_gateway::surface::Draft>| {
                if let Some(own) = drafts.first_mut() {
                    own.draft_id = String::from("carried");
                }
            }),
        ),
    ];
    for (what, mutate) in cases {
        let mut damaged = saved.clone();
        if let Some(seat) = damaged.seats.first_mut() {
            mutate(&mut seat.drafts);
        }
        let text = Save {
            stamp: Stamp::current(rules().rules_hash()),
            config: config_of(match_id),
            state: damaged,
        }
        .render();
        let error = Save::parse(&text).expect_err(what);
        assert_eq!(error.code(), Code::InvalidArgument, "{what}");
        assert!(
            error.message().contains("draft"),
            "{what}: {}",
            error.message()
        );
    }
}

/// A `sealed` save main wrote before the save carried the last Push (format
/// version 3, `34e2f427`'s round-1 `sealed` golden, committed as a fixture)
/// resumes into its Push, as it always did: the Push was sealed, not played,
/// so replaying it is the Push itself. Its next Lull's feed is that replayed
/// Push's own, `segment_ended` and all, so the Lull reads the Push it follows
/// (the operator does not read it as `PushUnread::Resumed`). Its stamp and
/// its seals' `report_hash` lines moved with lane `vcap`'s bump to
/// `0.1.0-s3.7`, as the `lull` fixture's did.
#[test]
fn a_version_3_sealed_save_resumes_into_its_push_and_its_next_lull_shows_it() {
    const MATCH_ID: &str = "m-fpstream2-save";
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("save-v3-round-one-sealed.json"),
    )
    .expect("the version-3 sealed fixture");
    let save = Save::parse(text.trim_end_matches('\n')).expect("a version-3 save reads");
    assert_eq!(save.stamp.format_version, 3);
    assert_eq!(save.state.boundary, Boundary::Sealed);
    assert!(
        u64::try_from(text.len()).expect("small")
            <= Save::max_bytes(&config_of(MATCH_ID), &rules()).expect("a bound"),
        "a save this size is within the bound a resume reads to"
    );
    save.check(&config_of(MATCH_ID), rules().rules_hash())
        .expect("the same rules, the same verifier, the same match");
    let host = pristine();
    let mut resumed = Surface::new(
        MATCH_ID,
        SEED,
        host.rules().clone(),
        FogPolicy::fogged(),
        &SEATS,
    )
    .expect("a surface")
    .resume(host, &save.state, CONTINUATION)
    .expect("a version-3 sealed save resumes");
    assert_eq!(resumed.time().phase, Phase::Push, "into its Push");
    let _ = play_push(&mut resumed);
    close_round(&mut resumed);
    assert_eq!(resumed.time().phase, Phase::Lull);
    let token = seat_token(&mut resumed, 0);
    let mut left = FIRST_LULL_MS;
    let feed = call_in_lull(
        &mut resumed,
        &token,
        &mut left,
        "get_segment_feed",
        "{\"detail\":\"full\"}",
    );
    assert!(
        pharmakos_proto::json::write(&feed).contains("segment_ended"),
        "the next Lull's feed shows the replayed Push"
    );
}

/// `Save::max_bytes` bounds a match by its own ladder (lane `opsave`, fix
/// pass 2, item 7): the feed's term is the live feed's retention over the
/// longest Push **this** match plays, never the rules' ladder chained in. A
/// match of 1-second Pushes is bounded below one of the rules' own ladder,
/// rounds past the round limit do not count, and the bound is printed
/// beside every committed save and the largest save of a played match.
#[test]
fn the_save_bound_is_the_matchs_own_and_is_printed_beside_its_saves() {
    const MATCH_ID: &str = "m-opsave-bound-played";
    let rules = rules();
    let with = |segment_lengths_ms: Vec<i32>, round_limit: u32| {
        let config = Config {
            segment_lengths_ms,
            round_limit,
            ..config_of("m-opsave-bound")
        };
        Save::max_bytes(&config, &rules).expect("a bound")
    };
    let short = with(vec![1_000], 2);
    let ladder = with(Vec::new(), 2);
    assert!(
        short < ladder,
        "a 1-second match is bounded by its own Pushes ({short}), not the rules' ladder ({ladder})"
    );
    assert_eq!(
        with(vec![1_000, 480_000], 1),
        short,
        "a round the limit never plays does not count"
    );
    assert!(
        with(vec![1_000, 480_000], 2) > short,
        "a round it plays does"
    );

    // The committed saves, each beside the bound of its own match.
    let root = support::workspace_root();
    for path in [
        "crates/gateway/tests/fixtures/save-v3-round-one-lull.json",
        "crates/gateway/tests/fixtures/save-v3-round-one-sealed.json",
        "crates/gamectl/tests/fixtures/save-v3-round-two-lull.json",
        "tests/golden/gateway/save_round_one/expected.lull.json",
        "tests/golden/gateway/save_round_one/expected.sealed.json",
    ] {
        let text = std::fs::read_to_string(root.join(path)).expect("a committed save");
        let save = Save::parse(text.trim_end_matches('\n')).expect("it reads");
        let bound = Save::max_bytes(&save.config, &rules).expect("a bound");
        let size = u64::try_from(text.len()).expect("small");
        println!("{path}: {size} bytes, bound {bound} bytes");
        assert!(size <= bound, "{path}");
    }

    // A played match's largest save, at every boundary it saves.
    let mut surface = new_match(MATCH_ID);
    let mut largest = 0_usize;
    let mut measure = |saved: SavedMatch| {
        let text = Save {
            stamp: Stamp::current(rules.rules_hash()),
            config: config_of(MATCH_ID),
            state: saved,
        }
        .render();
        largest = largest.max(text.len());
    };
    for _ in 0..ROUNDS {
        measure(surface.lull_save().expect("a Lull saves"));
        plan_round(&mut surface);
        assert!(surface.begin_push().expect("the Push begins"));
        if let Some(saved) = surface.take_persistence().save {
            measure(saved);
        }
        let _ = play_push(&mut surface);
        if let Some(saved) = surface.take_persistence().save {
            measure(saved);
        }
        close_round(&mut surface);
        if surface.time().phase != Phase::Lull {
            break;
        }
    }
    let bound = Save::max_bytes(&config_of(MATCH_ID), &rules).expect("a bound");
    println!("{MATCH_ID}: largest save {largest} bytes, bound {bound} bytes");
    assert!(u64::try_from(largest).expect("small") <= bound);
}
