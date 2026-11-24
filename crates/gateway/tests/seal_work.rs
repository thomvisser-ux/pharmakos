// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The seal carries its plan fingerprint (S3's `fpstream`, part 2).
//!
//! A seal's fingerprint is taken once, where the playbook is decoded for the
//! verify that qualifies it, and carried on the seal: the save writer stamps
//! the carried value and decodes nothing, and a resume recomputes it only as
//! the check it has always made, from the wire bytes of the one decode it
//! reads. This file pins each half in counted work, never a clock:
//!
//! * the codec's work meter (`pharmakos_proto::json::work`), which every
//!   reader and writer of a player's text adds to: a save that decoded a
//!   seal, or wrote one's canonical text for a fingerprint, moves it;
//! * the bytes the calling thread asks the allocator for, through a counting
//!   allocator, as `crates/sim/tests/fingerprint_memory.rs` counts them.
//!
//! And it pins the values: every seal's carried fingerprint is the reference
//! one ([`reference`], the rule written out whole over the canonical form of
//! its own text), and its carried price is the projection's spend over that
//! text, for each way a seal is made (a submission, the gateway's fallback,
//! an advised safe playbook, a resume; the scenario harness's door is
//! `gamectl`'s). The round a seal is filed for is the round it plays.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use pharmakos_gateway::fog::FogPolicy;
use pharmakos_gateway::host::{Host, Settings};
use pharmakos_gateway::save::{Continuation, SavedMatch};
use pharmakos_gateway::surface::{Sealed, Surface};
use pharmakos_gateway::token::Subject;
use pharmakos_proto::json::{Json, work};
use pharmakos_sim::tables::SeatId;

use support::{
    FIRST_LULL_MS, SEED, call_in_lull, hosted_with, quote, result, rules_json, seat_token,
    walk_east,
};

thread_local! {
    /// The bytes **this thread** has asked for since the last [`taken`].
    ///
    /// `const`-initialised and holding a type with no destructor, so the slot
    /// needs no lazy setup and registers no teardown hook, which is what
    /// makes it safe to touch from inside the global allocator.
    static REQUESTED: Cell<u64> = const { Cell::new(0) };
}

/// Charge one request of `size` bytes to the calling thread.
fn charge(size: usize) {
    // Inside the global allocator a panic or a print would recurse, so a
    // size past `u64` (none is) is charged as `u64::MAX`, which only makes a
    // bound below fail, and a thread being torn down is not charged.
    let size = u64::try_from(size).unwrap_or(u64::MAX);
    let _ = REQUESTED.try_with(|total| total.set(total.get().saturating_add(size)));
}

/// The bytes this thread asked for since the last call.
fn taken() -> u64 {
    REQUESTED
        .try_with(|total| total.replace(0))
        .expect("the counter outlives the test")
}

/// The system allocator, counting requested bytes.
struct Counting;

// SAFETY: every method forwards to `System` unchanged; the only addition is a
// thread-local counter update, which cannot affect the allocation itself.
//
// `unsafe_code` is denied workspace-wide, and a `GlobalAlloc` implementation
// cannot be written without it: the allowance is scoped to this one item, in a
// test target, as in `crates/sim/tests/fingerprint_memory.rs`.
#[allow(
    unsafe_code,
    reason = "a GlobalAlloc implementation cannot be written without unsafe; scoped to this test's counting allocator, which only forwards to System"
)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        charge(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        charge(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        charge(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// A segment long enough for a commander to walk: 60 ticks.
const SEGMENT_MS: i32 = 3_000;

/// Two rounds: round 2 is where a seat that submits nothing is filed the
/// gateway's fallback.
const ROUNDS: u32 = 2;

/// Both seats of a two-seat match.
const SEATS: [SeatId; 2] = [SeatId::new(0), SeatId::new(1)];

/// A new two-seat match in its opening Lull.
fn new_match(match_id: &str) -> Surface {
    hosted_with(match_id, FogPolicy::fogged(), 2, SEGMENT_MS, ROUNDS, None)
}

/// Seat `seat` submits `playbook`, which qualifies.
fn submit(surface: &mut Surface, seat: u8, playbook: &str) {
    let token = seat_token(surface, seat);
    let mut left = FIRST_LULL_MS;
    let answer = result(
        &call_in_lull(
            surface,
            &token,
            &mut left,
            "submit_plan",
            &format!("{{\"playbook_jsonc\":{}}}", quote(playbook)),
        ),
        "submit_plan",
    );
    assert_eq!(
        answer.get("accepted"),
        Some(&Json::Bool(true)),
        "{answer:?}"
    );
}

/// Both seats seal a walk east, seat 0's with `holds` waits before it, and a
/// stamped `meta.fingerprint` of the wrong value in seat 1's (a stamp is left
/// out of the fingerprint, and a wrong one is a warning, `E0008`).
fn seal_both(surface: &mut Surface, holds: usize) {
    let zero = walk_east(surface, 0, 4, holds);
    let one = walk_east(surface, 1, 3, 1).replace(
        "\"author_kind\": \"HUMAN\"",
        "\"author_kind\": \"HUMAN\", \"fingerprint\": \"AAAAAAAAAAE=\"",
    );
    submit(surface, 0, &zero);
    submit(surface, 1, &one);
}

/// The reference fingerprint of a seal's text: the domain and its canonical
/// form's text with `meta.fingerprint` cleared, whole, through the one-shot
/// digest: the value the save writer took until the seal carried it, written
/// out independently of the shipped streaming function.
fn reference(playbook_jsonc: &str) -> u64 {
    let canonical = pharmakos_plan_core::decode_jsonc(playbook_jsonc).expect("a canonical form");
    {
        // The rule written out whole: the reference, test-only.
        let mut bytes = pharmakos_proto::fingerprint::PLAN_FINGERPRINT_DOMAIN.to_vec();
        bytes.extend_from_slice(
            &pharmakos_proto::fingerprint::canonical_input(canonical.decoded().message())
                .expect("canonical"),
        );
        pharmakos_sim::encoding::digest(&bytes)
    }
}

/// Seat `seat`'s seal.
fn seal_of(surface: &Surface, seat: SeatId) -> &Sealed {
    surface
        .sealed_plan(Subject::Seat(seat), seat)
        .expect("a seal")
}

/// Every seat's seal carries the reference fingerprint of its own text.
fn carried_is_the_reference(surface: &Surface, what: &str) {
    for seat in SEATS {
        let sealed = seal_of(surface, seat);
        assert_eq!(
            sealed.plan_fingerprint(),
            reference(sealed.playbook_jsonc()),
            "{what}: seat {}'s carried fingerprint",
            seat.raw()
        );
    }
}

/// A resume of `saved`, as a restarted host loop makes it.
fn resume(match_id: &str, saved: &SavedMatch) -> Surface {
    let (host, surface) = unresumed(match_id);
    resume_into(surface, host, saved)
}

/// The pristine host a resume regenerates, and the surface it resumes into.
fn unresumed(match_id: &str) -> (Host, Surface) {
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
    let surface = Surface::new(
        match_id,
        SEED,
        host.rules().clone(),
        FogPolicy::fogged(),
        &SEATS,
    )
    .expect("a surface");
    (host, surface)
}

/// `saved`, resumed into `surface` over `host`.
fn resume_into(surface: Surface, host: Host, saved: &SavedMatch) -> Surface {
    surface
        .resume(
            host,
            saved,
            Continuation {
                generation: 1,
                last_tick: 0,
            },
        )
        .expect("resumed")
}

/// A save decodes no seal and writes no seal's canonical text: the meter
/// reads nothing over `lull_save`, and the save stamps the carried values.
/// Before the seal carried its fingerprint, the writer decoded each seal's
/// JSONC and encoded its message again, at every save.
#[test]
fn a_lull_save_decodes_no_seal_and_takes_no_fingerprint() {
    let mut surface = new_match("m-fpstream2-lull-work");
    seal_both(&mut surface, 0);
    let _ = work::take();
    let saved = surface.lull_save().expect("a Lull saves");
    assert_eq!(
        work::take(),
        0,
        "a save read or wrote a playbook's text, which it holds already"
    );
    for (seat, held) in SEATS.iter().zip(&saved.seats) {
        let seal = held.seal.as_ref().expect("a saved seal");
        assert_eq!(
            seal.plan_fingerprint,
            seal_of(&surface, *seat).plan_fingerprint(),
            "the save stamps the carried value"
        );
        assert_eq!(seal.plan_fingerprint, reference(&seal.playbook_jsonc));
    }
}

/// The `sealed` save a Push begins with decodes no seal either: `begin_push`
/// reads no playbook's text when every seat has sealed its own.
#[test]
fn the_push_s_save_decodes_no_seal() {
    let mut surface = new_match("m-fpstream2-push-work");
    seal_both(&mut surface, 0);
    let _ = work::take();
    assert!(surface.begin_push().expect("the Push begins"));
    assert_eq!(
        work::take(),
        0,
        "beginning the Push read or wrote a playbook's text"
    );
    let saved = surface
        .take_persistence()
        .save
        .expect("a Push's beginning is saved");
    for held in &saved.seats {
        let seal = held.seal.as_ref().expect("a saved seal");
        assert_eq!(seal.plan_fingerprint, reference(&seal.playbook_jsonc));
    }
}

/// A seat that seals nothing is filed the gateway's fallback, whose
/// fingerprint is taken once, at filing; every save after reads the carried
/// value.
#[test]
fn the_fallback_is_fingerprinted_once_at_filing() {
    let mut surface = new_match("m-fpstream2-fallback");
    // Round 1 seat 0 seals nothing: the fallback is filed for it.
    let one = walk_east(&surface, 1, 3, 1);
    submit(&mut surface, 1, &one);
    assert!(surface.begin_push().expect("the Push begins"));
    let zero = seal_of(&surface, SeatId::new(0));
    assert!(zero.filed_by_the_gateway(), "the fallback");
    assert_eq!(zero.round(), 1, "filed for the round it plays");
    carried_is_the_reference(&surface, "a filed fallback");
    let _ = surface.take_persistence();
    while let Some(report) = surface.step().expect("a tick") {
        if report.segment_ended {
            break;
        }
    }
    assert!(surface.end_recap().expect("the recap ends"));
    surface.open_lull().expect("round 2's Lull");
    let _ = work::take();
    let saved = surface.lull_save().expect("a Lull saves");
    assert_eq!(work::take(), 0, "a save read a filed playbook's text");
    let filed = saved
        .seats
        .first()
        .and_then(|seat| seat.seal.as_ref())
        .expect("seat 0's filed seal is carried into round 2's Lull");
    assert_eq!(filed.plan_fingerprint, reference(&filed.playbook_jsonc));
}

/// A resumed seal carries the fingerprint stamped beside it, which the
/// resume has just checked against its text.
#[test]
fn a_resumed_seal_carries_its_stamp() {
    const MATCH_ID: &str = "m-fpstream2-resumed";
    let mut surface = new_match(MATCH_ID);
    seal_both(&mut surface, 0);
    let saved = surface.lull_save().expect("a Lull saves");
    let resumed = resume(MATCH_ID, &saved);
    carried_is_the_reference(&resumed, "a resumed seal");
    for (seat, held) in SEATS.iter().zip(&saved.seats) {
        assert_eq!(
            seal_of(&resumed, *seat).plan_fingerprint(),
            held.seal.as_ref().expect("a saved seal").plan_fingerprint,
            "seat {}: the stamp, carried",
            seat.raw()
        );
        assert_eq!(seal_of(&resumed, *seat), seal_of(&surface, *seat));
    }
    // Saved again, with nothing decoded.
    let _ = work::take();
    let again = resumed.lull_save().expect("a resumed Lull saves");
    assert_eq!(work::take(), 0);
    assert_eq!(again.seats, saved.seats);
}

/// A resume checks each seal's stamp from the wire bytes of the one decode it
/// reads, so its fingerprint takes no copy of the seal's canonical text: a
/// long seal costs a resume what decoding and compiling it costs, and what
/// carrying its text costs, and nothing that grows with a second copy.
///
/// Measured as differences, so nothing of the match itself is counted: the
/// bytes a resume of a long seal asks for beyond a resume of a short one,
/// less what decoding and compiling the long text asks for beyond the short,
/// measured on their own. What is left is the seal's own copy of its text and
/// the fingerprint's; taking the fingerprint by encoding the message again
/// added a clone of the message, the canonical text grown in a `String` and
/// the domain's copy of it, several bytes for each byte of the text.
#[test]
fn a_resume_takes_no_copy_of_a_seals_canonical_text_for_its_fingerprint() {
    const MATCH_ID: &str = "m-fpstream2-resume-bytes";
    let mut short = new_match(MATCH_ID);
    seal_both(&mut short, 0);
    let short_save = short.lull_save().expect("a Lull saves");
    let mut long = new_match(MATCH_ID);
    seal_both(&mut long, 120);
    let long_save = long.lull_save().expect("a Lull saves");
    let text_of_zero = |save: &SavedMatch| {
        save.seats
            .first()
            .and_then(|seat| seat.seal.as_ref())
            .expect("seat 0's seal")
            .playbook_jsonc
            .clone()
    };
    let (short_text, long_text) = (text_of_zero(&short_save), text_of_zero(&long_save));
    let canonical_length = |text: &str| {
        pharmakos_plan_core::decode_jsonc(text)
            .expect("a canonical form")
            .text()
            .len()
    };
    let grown = u64::try_from(canonical_length(&long_text) - canonical_length(&short_text))
        .expect("a length");
    let grown_jsonc = u64::try_from(long_text.len() - short_text.len()).expect("a length");
    assert!(grown > 10_000, "the long seal is long: {grown} bytes more");

    let rules = support::rules();
    let decode_and_compile = |text: &str| {
        let _ = taken();
        let canonical = pharmakos_plan_core::decode_jsonc(text).expect("a canonical form");
        let plan = pharmakos_sim::interpreter::Plan::compile(canonical.decoded().message(), &rules)
            .expect("it compiles");
        let bytes = taken();
        drop((canonical, plan));
        bytes
    };
    let resume_cost = |save: &SavedMatch| {
        let _ = taken();
        let resumed = resume(MATCH_ID, save);
        let bytes = taken();
        drop(resumed);
        bytes
    };
    let resumed = resume_cost(&long_save).saturating_sub(resume_cost(&short_save));
    let decoded = decode_and_compile(&long_text).saturating_sub(decode_and_compile(&short_text));
    let left = resumed.saturating_sub(decoded);
    // The seal's own copy of its text; then the pricing's projection of the
    // longer route and the fingerprint's gathering buffer and field stack,
    // given one canonical text's length and 16 KiB between them (measured:
    // 14,336 bytes past the copy, on `fpstream`'s head). Taking the
    // fingerprint by encoding the message again costs more than three
    // canonical texts on its own (the clone, the `String` grown to the text,
    // the domain's copy), so it cannot fit.
    let bound = grown_jsonc + grown + (16 << 10);
    assert!(
        left <= bound,
        "a resume asked for {resumed} bytes more for a seal {grown} canonical bytes longer, of \
         which decoding and compiling it is {decoded}: the {left} left over is past the seal's \
         own copy of its text and the fingerprint's fixed buffers ({bound})"
    );
}

/// A resume reads each seal's text once: the codec's meter over a resume
/// reads exactly one decode of each seal and one write of its canonical text
/// for the fingerprint check, and nothing more. Until `fpstream` part 2 the
/// pricing of a resumed seal decoded its text a second time.
#[test]
fn a_resume_reads_each_seal_once() {
    const MATCH_ID: &str = "m-fpstream2-resume-work";
    let mut surface = new_match(MATCH_ID);
    seal_both(&mut surface, 40);
    let saved = surface.lull_save().expect("a Lull saves");
    let mut expected = 0_u128;
    for held in &saved.seats {
        let text = &held.seal.as_ref().expect("a saved seal").playbook_jsonc;
        let _ = work::take();
        let canonical = pharmakos_plan_core::decode_jsonc(text).expect("a canonical form");
        let _ =
            pharmakos_sim::hash::plan_fingerprint_of(canonical.decoded()).expect("a fingerprint");
        expected += work::take();
    }
    assert!(expected > 0, "a decode is counted");
    let (host, fresh) = unresumed(MATCH_ID);
    let _ = work::take();
    let resumed = resume_into(fresh, host, &saved);
    assert_eq!(
        work::take(),
        expected,
        "a resume read the seals' text other than once each, with their fingerprint check"
    );
    drop(resumed);
}

/// The codec's work over one call of `method` by seat 0 with `playbook`, on a
/// fresh match: the request is parsed and the vision built before the meter is
/// read, so what is counted is the call's own reading and writing.
fn work_of_call(match_id: &str, method: &str, params: &str) -> u128 {
    let mut surface = new_match(match_id);
    let token = seat_token(&mut surface, 0);
    surface
        .set_phase_remaining_ms(pharmakos_sim::math::quantity::Ms::new(FIRST_LULL_MS - 50))
        .expect("a custom match takes a countdown");
    let request = support::request(method, params);
    let sight = support::vision(&surface);
    let _ = work::take();
    let answer = surface.call(Some(&token), &request, &sight);
    let counted = work::take();
    assert!(answer.get("result").is_some(), "{method}: {answer:?}");
    counted
}

/// `submit_plan` reads its playbook exactly as `verify_plan` at FULL does and
/// takes no fingerprint of its own: the seal's is the report's, read as eight
/// bytes. Its compile and its pricing read the same decode and write no text.
#[test]
fn a_submission_reads_its_text_as_a_full_verify_does() {
    let probe = new_match("m-fpstream2-submit-probe");
    let playbook = walk_east(&probe, 0, 4, 20);
    let quoted = quote(&playbook);
    let verified = work_of_call(
        "m-fpstream2-submit-verify",
        "verify_plan",
        &format!("{{\"playbook_jsonc\":{quoted},\"depth\":\"full\"}}"),
    );
    let submitted = work_of_call(
        "m-fpstream2-submit",
        "submit_plan",
        &format!("{{\"playbook_jsonc\":{quoted}}}"),
    );
    assert!(verified > 0, "a verify reads its text");
    assert_eq!(
        submitted, verified,
        "a submission read or wrote the playbook's text other than its verify did"
    );
}

/// The advised safe playbook is filed as a FULL verify reads it, and no
/// fingerprint is taken beside the report's.
#[test]
fn an_advised_safe_playbook_reads_its_text_as_a_full_verify_does() {
    let probe = new_match("m-fpstream2-advice-probe");
    let playbook = walk_east(&probe, 0, 2, 10);
    let verified = work_of_call(
        "m-fpstream2-advice-verify",
        "verify_plan",
        &format!(
            "{{\"playbook_jsonc\":{},\"depth\":\"full\"}}",
            quote(&playbook)
        ),
    );
    let mut surface = new_match("m-fpstream2-advice");
    let _ = work::take();
    surface
        .file_advice(
            SeatId::new(0),
            pharmakos_gateway::advice::Advice {
                safe_playbook_jsonc: playbook.clone(),
                suggestions: Vec::new(),
                start_here: None,
            },
        )
        .expect("filed");
    let filed = work::take();
    assert_eq!(
        filed, verified,
        "filing an advised safe playbook read or wrote its text other than its verify did"
    );
    let advised = surface
        .seat_state(Subject::Seat(SeatId::new(0)), SeatId::new(0))
        .expect("its own")
        .advice()
        .and_then(|advised| advised.safe().cloned())
        .expect("the advice qualified");
    assert_eq!(
        advised.plan_fingerprint(),
        reference(advised.playbook_jsonc())
    );
}

/// A match of `rounds` rounds, in its opening Lull.
fn match_of(match_id: &str, rounds: u32) -> Surface {
    hosted_with(match_id, FogPolicy::fogged(), 2, SEGMENT_MS, rounds, None)
}

/// A resume of `saved` into a match of `rounds` rounds.
fn resume_of(match_id: &str, rounds: u32, saved: &SavedMatch) -> Surface {
    let host = Host::open_from(
        &rules_json(),
        SEED,
        2,
        &Settings {
            segment_lengths_ms: vec![SEGMENT_MS],
            round_limit: rounds,
        },
        None,
    )
    .expect("a match");
    let surface = Surface::new(
        match_id,
        SEED,
        host.rules().clone(),
        FogPolicy::fogged(),
        &SEATS,
    )
    .expect("a surface");
    resume_into(surface, host, saved)
}

/// Play the open Push to its end and open the next Lull.
fn play_round(surface: &mut Surface) {
    let _ = surface.take_persistence();
    while let Some(report) = surface.step().expect("a tick") {
        if report.segment_ended {
            break;
        }
    }
    assert!(surface.end_recap().expect("the recap ends"));
    surface.open_lull().expect("the next Lull");
}

/// The round a seal is filed for has one home, the filing (S3's `fpstream`,
/// part 2, fix pass 2; review 2's probe): after a resume from round 2's Lull,
/// the fallback filed in round 2 carries 2, and an advised safe playbook
/// filed in round 3 carries 3, for a seat that seals nothing in either.
#[test]
fn a_filing_after_a_resume_carries_its_round() {
    const ID: &str = "m-fpstream2-filing-round";
    let mut surface = match_of(ID, 3);
    let zero = walk_east(&surface, 0, 4, 2);
    let one = walk_east(&surface, 1, 3, 1);
    submit(&mut surface, 0, &zero);
    submit(&mut surface, 1, &one);
    assert!(surface.begin_push().expect("the Push begins"));
    play_round(&mut surface);
    let saved = surface.lull_save().expect("round 2's Lull saves");
    assert_eq!(saved.round, 2);
    let mut resumed = resume_of(ID, 3, &saved);
    // Round 2: seat 0 seals nothing, seat 1 seals again.
    let again = walk_east(&resumed, 1, 3, 1);
    submit(&mut resumed, 1, &again);
    assert!(resumed.begin_push().expect("round 2's Push"));
    let filed = seal_of(&resumed, SeatId::new(0));
    assert!(filed.filed_by_the_gateway());
    assert_eq!(filed.round(), 2, "round 2's filing");
    assert_eq!(filed.plan_fingerprint(), reference(filed.playbook_jsonc()));
    play_round(&mut resumed);
    // Round 3: seat 0 is advised and seals nothing.
    let advised = walk_east(&resumed, 0, 2, 3);
    resumed
        .file_advice(
            SeatId::new(0),
            pharmakos_gateway::advice::Advice {
                safe_playbook_jsonc: advised.clone(),
                suggestions: Vec::new(),
                start_here: None,
            },
        )
        .expect("filed");
    let third = walk_east(&resumed, 1, 3, 1);
    submit(&mut resumed, 1, &third);
    assert!(resumed.begin_push().expect("round 3's Push"));
    let filed = seal_of(&resumed, SeatId::new(0));
    assert!(filed.filed_by_the_gateway());
    assert_eq!(filed.playbook_jsonc(), advised, "the advice is what files");
    assert_eq!(filed.round(), 3, "a filing in round 3 carries round 3");
    assert_eq!(filed.plan_fingerprint(), reference(filed.playbook_jsonc()));
}

/// The forecast reads the seal's carried price and no text: the codec's meter
/// over `get_economy_forecast` reads nothing with a seal of this round, as
/// it reads nothing without one. Until fix pass 2 it decoded the sealed text
/// on every call (34,245 bytes at 121 of 128 units, review 2's probe).
#[test]
fn a_forecast_with_a_seal_reads_no_text() {
    let mut surface = new_match("m-fpstream2-forecast-work");
    let token = seat_token(&mut surface, 0);
    let forecast = support::request("get_economy_forecast", "{}");
    let call = |surface: &mut Surface| {
        let sight = support::vision(surface);
        let _ = work::take();
        let answer = surface.call(Some(&token), &forecast, &sight);
        let counted = work::take();
        (result(&answer, "get_economy_forecast"), counted)
    };
    let (_, bare) = call(&mut surface);
    assert_eq!(bare, 0, "a forecast with no seal reads no text");
    let long = walk_east(&surface, 0, 4, 120);
    submit(&mut surface, 0, &long);
    let (answer, sealed) = call(&mut surface);
    assert_eq!(sealed, 0, "a forecast read the sealed text");
    assert_eq!(
        answer.get("committed_dollars"),
        Some(&Json::Number(
            seal_of(&surface, SeatId::new(0))
                .committed_dollars()
                .to_string()
        )),
        "the forecast answers the carried price"
    );
}

/// The carried price is the price, for every way a seal is made: for every
/// committed playbook a seat can seal on this match (the walks and the
/// library's templates, every one of which must instantiate), the seal's
/// `committed_dollars` is the verifier's projection of the same text, taken
/// here again, when it is submitted, when it is read back by a resume, and
/// when it is advised and filed for a seat that sealed nothing; and the
/// gateway's fallback carries its own text's. The spend is a function of the
/// text and the rules table alone (`pharmakos_verifier::projection::project`'s
/// `spend`), so the economy the reference is projected against does not move
/// it. At least one of the sealed texts costs money, so a price off by one at
/// any of the makers is red.
#[test]
fn the_carried_price_is_the_projection_of_the_sealed_text() {
    use pharmakos_sim::knowledge::SeatEconomy;
    use pharmakos_sim::math::quantity::{Kw, Money};
    use pharmakos_verifier::projection::{Prices, project};
    let rules = support::rules();
    let prices = Prices::from_rules(&rules).expect("the rules price");
    let economy = SeatEconomy {
        treasury: Money::ZERO,
        supply: Kw::ZERO,
        draw: Kw::ZERO,
    };
    let projected = |text: &str| {
        let canonical = pharmakos_plan_core::decode_jsonc(text).expect("a canonical form");
        project(canonical.decoded().message(), economy, &prices)
            .expect("a seal prices")
            .spend
            .raw()
    };
    let texts = committed_texts();
    let mut sealed = 0_usize;
    let mut costly = 0_usize;
    for (index, text) in texts.iter().enumerate() {
        // Submitted.
        let id = format!("m-fpstream2-price-{index}");
        let mut surface = new_match(&id);
        let token = seat_token(&mut surface, 0);
        let mut left = FIRST_LULL_MS;
        let answer = result(
            &call_in_lull(
                &mut surface,
                &token,
                &mut left,
                "submit_plan",
                &format!("{{\"playbook_jsonc\":{}}}", quote(text)),
            ),
            "submit_plan",
        );
        if answer.get("accepted") != Some(&Json::Bool(true)) {
            continue;
        }
        let spend = projected(text);
        let carried = seal_of(&surface, SeatId::new(0)).committed_dollars();
        assert_eq!(i64::from(carried), spend, "text {index}, submitted");
        // Read back by a resume.
        let saved = surface.lull_save().expect("a Lull saves");
        let resumed = resume(&id, &saved);
        assert_eq!(
            i64::from(seal_of(&resumed, SeatId::new(0)).committed_dollars()),
            spend,
            "text {index}, resumed"
        );
        // Advised, and filed for a seat that sealed nothing.
        let mut advised = new_match(&format!("{id}-advised"));
        advised
            .file_advice(
                SeatId::new(0),
                pharmakos_gateway::advice::Advice {
                    safe_playbook_jsonc: text.clone(),
                    suggestions: Vec::new(),
                    start_here: None,
                },
            )
            .expect("filed");
        let one = walk_east(&advised, 1, 3, 1);
        submit(&mut advised, 1, &one);
        assert!(advised.begin_push().expect("the Push begins"));
        let filed = seal_of(&advised, SeatId::new(0));
        assert!(filed.filed_by_the_gateway());
        assert_eq!(
            filed.playbook_jsonc(),
            text.as_str(),
            "text {index}, advised"
        );
        assert_eq!(
            i64::from(filed.committed_dollars()),
            spend,
            "text {index}, advised and filed"
        );
        sealed = sealed.saturating_add(1);
        if spend > 0 {
            costly = costly.saturating_add(1);
        }
    }
    assert!(sealed >= 5, "only {sealed} of {} texts sealed", texts.len());
    assert!(
        costly >= 1,
        "no sealed text ordered anything, so no price was tested above 0"
    );

    // The gateway's fallback, filed for a seat that sealed nothing and was
    // advised nothing.
    let mut fallback = new_match("m-fpstream2-price-fallback");
    let one = walk_east(&fallback, 1, 3, 1);
    submit(&mut fallback, 1, &one);
    assert!(fallback.begin_push().expect("the Push begins"));
    let filed = seal_of(&fallback, SeatId::new(0));
    assert!(filed.filed_by_the_gateway());
    assert_eq!(
        i64::from(filed.committed_dollars()),
        projected(filed.playbook_jsonc()),
        "the fallback"
    );
}

/// The scenario harness's door holds a seal to the rules every other door
/// does (S3's `fpstream`, part 2, fix pass 3): it is audited as `seal by
/// harness` with its outcome, a text past the size cap is refused before
/// anything reads it, and a seal it makes is filed for the round it is made in,
/// carrying its own text's fingerprint.
#[test]
fn the_harness_door_is_audited_and_capped() {
    let mut surface = new_match("m-fpstream2-harness-door");
    let last = |surface: &mut Surface| {
        surface
            .audit()
            .entries()
            .iter()
            .rev()
            .find(|entry| entry.action == "seal by harness")
            .map(|entry| (entry.subject, entry.outcome.render()))
            .expect("the door is audited")
    };
    let past = "x".repeat(pharmakos_verifier::MAX_PLAYBOOK_CHARS.saturating_add(1));
    let refused = surface
        .seal_by_harness(SeatId::new(0), &past)
        .expect_err("past the cap");
    assert_eq!(
        refused.code(),
        pharmakos_gateway::error::Code::InvalidArgument
    );
    assert!(
        refused.message().contains("at most"),
        "{}",
        refused.message()
    );
    let (subject, outcome) = last(&mut surface);
    assert_eq!(subject, Some(Subject::Seat(SeatId::new(0))));
    assert_ne!(outcome, "ok", "a refusal is audited as one");
    assert!(
        surface
            .seat_state(Subject::Seat(SeatId::new(0)), SeatId::new(0))
            .expect("its own")
            .sealed()
            .is_none(),
        "nothing is sealed past the cap"
    );

    let text = walk_east(&surface, 0, 4, 3);
    surface
        .seal_by_harness(SeatId::new(0), &text)
        .expect("the harness seals it");
    let (subject, outcome) = last(&mut surface);
    assert_eq!(subject, Some(Subject::Seat(SeatId::new(0))));
    assert_eq!(outcome, "ok");
    let sealed = seal_of(&surface, SeatId::new(0));
    assert_eq!(sealed.round(), surface.time().round);
    assert!(!sealed.filed_by_the_gateway());
    assert_eq!(sealed.plan_fingerprint(), reference(&text));
}

/// The texts the carried-price test seals: the walks, and every template in
/// the library, each of which must instantiate.
fn committed_texts() -> Vec<String> {
    let probe = new_match("m-fpstream2-price-probe");
    let mut texts: Vec<String> = [0_usize, 1, 20, 120]
        .iter()
        .map(|holds| walk_east(&probe, 0, 4, *holds))
        .collect();
    #[allow(
        clippy::disallowed_methods,
        reason = "clippy.toml sanctions read_dir when the listing is collected and sorted before use, which is what happens here"
    )]
    let mut templates: Vec<std::path::PathBuf> = std::fs::read_dir(support::library_folder())
        .expect("the library")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonc")
        })
        .collect();
    templates.sort();
    assert!(templates.len() >= 5, "the library holds its templates");
    for path in &templates {
        let template = std::fs::read_to_string(path).expect("a template");
        let made = pharmakos_plan_core::instantiate(&template, &[], &[]).unwrap_or_else(|error| {
            panic!(
                "{}: a committed template that does not instantiate: {}",
                path.display(),
                error.message()
            )
        });
        texts.push(made.playbook_jsonc);
    }
    texts
}
