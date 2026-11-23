// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The plan fingerprint holds no copy of its text (S3's `fpstream`).
//!
//! `plan_fingerprint_of` hashes the canonical text in pieces as the codec
//! writes it, so what it allocates is the codec's one gathering buffer and the
//! writer's field stack, whatever the playbook's size. Until `fpstream` it
//! wrote the text whole into one `String` first: 431,573 bytes at the size
//! budget's first shape, reserved in one allocation. This file counts the
//! bytes it asks the allocator for, on the calling thread, around one call,
//! and holds both the largest request and the total far below the text.
//!
//! The counter is per thread, as in `tests/allocations.rs`, so libtest's own
//! threads are not charged to the call. What is counted is a size, not a
//! count of allocations, so an optimiser that removes an allocation can only
//! lower it: the bound holds at every opt-level.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules — not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use pharmakos_proto::gp::v1::Playbook;
use pharmakos_proto::json;
use pharmakos_sim::hash::plan_fingerprint_of;

thread_local! {
    /// The bytes **this thread** has asked for, and its largest one request.
    ///
    /// `const`-initialised and holding types with no destructor, so the slots
    /// need no lazy setup and register no teardown hook — which is what makes
    /// them safe to touch from inside the global allocator.
    static REQUESTED: Cell<u64> = const { Cell::new(0) };
    static LARGEST: Cell<u64> = const { Cell::new(0) };
}

/// Charge one request of `size` bytes to the calling thread.
fn charge(size: usize) {
    let size = u64::try_from(size).unwrap_or(u64::MAX);
    // `try_with` rather than `with`: during thread teardown the slots are
    // gone, and an allocation then is not this test's business.
    let _ = REQUESTED.try_with(|total| total.set(total.get().saturating_add(size)));
    let _ = LARGEST.try_with(|largest| largest.set(largest.get().max(size)));
}

/// The bytes and the largest request this thread made since the last call.
fn take() -> (u64, u64) {
    (
        REQUESTED.try_with(|total| total.replace(0)).unwrap_or(0),
        LARGEST.try_with(|largest| largest.replace(0)).unwrap_or(0),
    )
}

/// The system allocator, counting requested bytes.
struct Counting;

// SAFETY: every method forwards to `System` unchanged; the only addition is a
// thread-local counter update, which cannot affect the allocation itself.
//
// `unsafe_code` is denied workspace-wide, and a `GlobalAlloc` implementation
// cannot be written without it: the allowance is scoped to this one item, in
// a test target, as in `tests/allocations.rs`.
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

/// The codec's gathering buffer: the one request the text itself makes.
const GATHERING_BUFFER: u64 = 8 << 10;

/// What one fingerprint may ask the allocator for beyond the gathering
/// buffer, per wire byte: the writer's stack of field values on the way down
/// and its scratch for a repeated field's elements, which grow with the
/// fields the wire holds (a value read is a few machine words) and never
/// with the text, which runs to 11 to 24 times the wire.
const PER_WIRE_BYTE: u64 = 8;

/// The largest one request may be: twice the gathering buffer, so no request
/// is near a text of the sizes below (33 KB to 850 KB).
const LARGEST_REQUEST: u64 = 2 * GATHERING_BUFFER;

/// The two shapes P1 is measured at (decisions-log item 158 (1)), as
/// `crates/verifier/tests/fingerprint.rs` builds them: 63 handlers, and 127
/// route steps, each with a 24-node condition.
fn size_budget_shapes() -> [String; 2] {
    let leaf =
        |value: usize| format!(r#"{{"segment_elapsed":{{"ms":{{"op":"EQ","value":{value}}}}}}}"#);
    let condition = |id: usize, next: &mut usize| {
        let leaves: Vec<String> = (0..21)
            .map(|_| {
                *next += 1;
                leaf(*next)
            })
            .collect();
        let group = if id.is_multiple_of(2) { "any" } else { "all" };
        let wrap = |inner: String| format!(r#"{{"{group}":{{"items":[{inner}]}}}}"#);
        wrap(wrap(wrap(leaves.join(","))))
    };
    let tail = r#""on_death":{"on_respawn":"CONTINUE","max_deaths_before_fallback":1},"fallback":{"hold":{"at":{"beacon_anchor":{"safest":{}}}}}"#;
    let head = r#""schema_version":{"major":1},"meta":{"title":"p1","author_kind":"HUMAN"},"kind":"PLAYBOOK""#;
    let mut next = 0_usize;
    let handlers: Vec<String> = (0..63)
        .map(|id| {
            format!(
                r#"{{"id":"h{id:02}","when":{},"body":[{{"label":"h{id:02}_a","hold":{{"ms":1000}}}}],"resume":"CONTINUE","cooldown_ms":5000,"max_fires":2}}"#,
                condition(id, &mut next)
            )
        })
        .collect();
    let steps: Vec<String> = (0..127)
        .map(|id| {
            format!(
                r#"{{"label":"r{id:03}","skip_if":{},"hold":{{"ms":1000}}}}"#,
                condition(id, &mut next)
            )
        })
        .collect();
    [
        format!(
            r#"{{{head},"declarative":{{"route":[{{"label":"r00","hold":{{"ms":1000}}}}],"handlers":[{}]}},{tail}}}"#,
            handlers.join(",")
        ),
        format!(
            r#"{{{head},"declarative":{{"route":[{}]}},{tail}}}"#,
            steps.join(",")
        ),
    ]
}

/// The bytes one `plan_fingerprint_of` asks for, and its largest request,
/// for `text`; with the length of the canonical text it is taken over, the
/// value it answers and the length of the wire bytes it was written from.
fn measured(text: &str) -> (u64, u64, usize, u64, u64) {
    let decoded = json::decode_encoded::<Playbook>(text).expect("a playbook");
    let canonical = pharmakos_proto::fingerprint::canonical_input(decoded.message())
        .expect("the playbook writes");
    let _ = take();
    let fingerprint = plan_fingerprint_of(&decoded).expect("the playbook has a fingerprint");
    let (requested, largest) = take();
    let wire = u64::try_from(decoded.encoded().bytes().len()).unwrap();
    (requested, largest, canonical.len(), fingerprint, wire)
}

#[test]
fn the_fingerprint_holds_no_copy_of_its_text() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("verifier")
        .join("tests")
        .join("cases")
        .join("budget_128.json");
    let budget = std::fs::read_to_string(path).expect("the verifier's size budget case");
    let [handlers, route] = size_budget_shapes();
    // The texts, measured, so a shape that shrank would show here: the
    // committed size budget case, and P1's two shapes, whose canonical texts
    // are about 431,000 and 850,000 bytes.
    for (name, text, at_least) in [
        ("budget_128", &budget, 30_000_usize),
        ("the handler shape", &handlers, 400_000),
        ("the route shape", &route, 800_000),
    ] {
        let (requested, largest, length, fingerprint, wire) = measured(text);
        assert!(
            length >= at_least,
            "{name}: the canonical text is {length} bytes, under the {at_least} the bound is \
             read against"
        );
        if name == "budget_128" {
            assert_eq!(fingerprint, 0x8c85_6401_4267_efef, "the pinned value");
        }
        assert!(
            largest <= LARGEST_REQUEST,
            "{name}: one request of {largest} bytes, past {LARGEST_REQUEST}, for a text of \
             {length} bytes"
        );
        let most = GATHERING_BUFFER + PER_WIRE_BYTE * wire;
        assert!(
            requested <= most,
            "{name}: {requested} bytes requested in all, past {most} for {wire} wire bytes, for \
             a text of {length} bytes"
        );
        assert!(
            requested < u64::try_from(length).unwrap(),
            "{name}: {requested} bytes requested in all, as much as the text of {length} bytes"
        );
    }
}
