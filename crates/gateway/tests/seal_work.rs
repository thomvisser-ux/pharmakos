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
//! And it pins the value: every seal's carried fingerprint is the reference
//! one, `pharmakos_sim::hash::plan_fingerprint` over the canonical form of its
//! own text, for each way a seal is made (a submission, the gateway's
//! fallback, a resume; the advised safe playbook is `tests/advice.rs`', and
//! the scenario harness's door is `gamectl`'s).

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

/// The reference fingerprint of a seal's text: its canonical form's, by the
/// message, as the save writer took it until the seal carried it.
fn reference(playbook_jsonc: &str) -> u64 {
    let canonical = pharmakos_plan_core::decode_jsonc(playbook_jsonc).expect("a canonical form");
    pharmakos_sim::hash::plan_fingerprint(canonical.decoded().message()).expect("a fingerprint")
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
            reference(&sealed.playbook_jsonc),
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
    assert!(zero.filed_by_the_gateway, "the fallback");
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
