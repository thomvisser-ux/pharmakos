// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The editor's wait for a check, pinned by counted allocations rather than by
//! a clock (S3's `vfyq2`; the pattern of `crates/verifier/tests/quick_cost.rs`).
//!
//! `verify_plan` hands `verify_jsonc` the player's text, and the editor waits
//! on the whole call, not only on the verifier inside it. Before `vfyq2` the
//! call reached the verifier through the JSONC document's chain: a
//! trivia-keeping tree, a plain tree, the message, a third tree and a
//! commented text the verifier never read, each an allocation per node, and
//! together several times the verifier's own cost on the shapes P1 is
//! measured at. This crate reads no clock (AGENTS.md section 4.5); the wall
//! time is published by the walled `crates/bench`. What it can hold is the
//! count, which a return to a tree per node moves by tens of thousands.
//!
//! The shapes are `quick_cost.rs`' two at the size budget, 63 handlers and 127
//! route steps each with a 24-node condition, sent compact, as P1's
//! end-to-end figure sends them, and indented with a comment on every line, as
//! a player's editor holds them. The count is QUICK end to end through
//! `verify_jsonc`: the verifier's own allocations, which `quick_cost.rs` pins
//! apart, are inside it.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules — not an \
              integration test's helper functions. A panic is this file's failure report."
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::Path;

use pharmakos_plan_core::verify::verify_jsonc;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::snapshot::{SNAPSHOT_VERSION, Snapshot};
use pharmakos_sim::tables::SeatId;
use pharmakos_verifier::Scope;

thread_local! {
    /// How many times **this thread** has allocated: `const`-initialised and
    /// holding a type with no destructor, so touching it from inside the
    /// global allocator neither allocates nor registers a teardown hook.
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
}

/// Count one allocation against the calling thread.
fn charge() {
    let _ = ALLOCATIONS.try_with(|count| count.set(count.get().saturating_add(1)));
}

/// How many times this thread has allocated so far.
fn allocations() -> u64 {
    ALLOCATIONS.try_with(Cell::get).unwrap_or(0)
}

/// The system allocator, counting.
struct Counting;

// SAFETY: every method forwards to `System` unchanged; the only addition is a
// thread-local counter increment, which cannot affect the allocation itself.
//
// `unsafe_code` is denied workspace-wide. A `GlobalAlloc` cannot be written
// without `unsafe`; the allowance is scoped to this one item in a test target,
// as `crates/verifier/tests/quick_cost.rs`, `crates/sim/tests/allocations.rs`
// and `crates/mesher/tests/allocations.rs` scope theirs, and the pull request
// raises it.
#[allow(
    unsafe_code,
    reason = "a GlobalAlloc implementation cannot be written without unsafe; scoped to this \
              test's counting allocator, which only forwards to System"
)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        charge();
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        charge();
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        charge();
        unsafe { System.alloc_zeroed(layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// One predicate of a 24-node condition.
fn leaf(value: usize) -> String {
    format!(r#"{{"segment_elapsed":{{"ms":{{"op":"EQ","value":{value}}}}}}}"#)
}

/// A 24-node condition: 21 predicates under three nested groups, alternating
/// `any` and `all` from one condition to the next.
fn condition(id: usize, next: &mut usize) -> String {
    let leaves: Vec<String> = (0..21)
        .map(|_| {
            *next = next.saturating_add(1);
            leaf(*next)
        })
        .collect();
    let group = if id.is_multiple_of(2) { "any" } else { "all" };
    let wrap = |inner: String| format!(r#"{{"{group}":{{"items":[{inner}]}}}}"#);
    wrap(wrap(wrap(leaves.join(","))))
}

const HEAD: &str = r#""schema_version":{"major":1},"meta":{"title":"vfyq2","author_kind":"HUMAN"},"kind":"PLAYBOOK""#;
const TAIL: &str = r#""on_death":{"on_respawn":"CONTINUE","max_deaths_before_fallback":1},"fallback":{"hold":{"at":{"beacon_anchor":{"safest":{}}}}}"#;

/// 63 handlers, each with a 24-node `when`, every step's `on_fail` written.
fn handler_shape() -> String {
    let mut next = 0;
    let handlers: Vec<String> = (0..63)
        .map(|id| {
            format!(
                r#"{{"id":"h{id:02}","when":{},"body":[{{"label":"h{id:02}_a","on_fail":{{"action":"SKIP"}},"hold":{{"ms":1000}}}}],"resume":"CONTINUE","cooldown_ms":5000,"max_fires":2}}"#,
                condition(id, &mut next)
            )
        })
        .collect();
    format!(
        r#"{{{HEAD},"declarative":{{"route":[{{"label":"r00","on_fail":{{"action":"SKIP"}},"hold":{{"ms":1000}}}}],"handlers":[{}]}},{TAIL}}}"#,
        handlers.join(",")
    )
}

/// 127 route steps, each with a 24-node `skip_if`, every `on_fail` written.
fn route_shape() -> String {
    let mut next = 0;
    let steps: Vec<String> = (0..127)
        .map(|id| {
            format!(
                r#"{{"label":"r{id:03}","skip_if":{},"on_fail":{{"action":"SKIP"}},"hold":{{"ms":1000}}}}"#,
                condition(id, &mut next)
            )
        })
        .collect();
    format!(
        r#"{{{HEAD},"declarative":{{"route":[{}]}},{TAIL}}}"#,
        steps.join(",")
    )
}

/// `compact` with a comment before it and one between every two steps or
/// rules, a line comment and a block comment in turn: a commented file, kept
/// within `MAX_PLAYBOOK_CHARS` (the shapes' indented text is past it).
fn commented(compact: &str) -> String {
    let mut out = String::from("// A player's playbook, with notes.\n");
    let mut line = true;
    for (index, piece) in compact.split("},{\"").enumerate() {
        if index > 0 {
            out.push_str("},");
            out.push_str(if line {
                "// the next one\n"
            } else {
                "/* the next one */"
            });
            line = !line;
            out.push_str("{\"");
        }
        out.push_str(piece);
    }
    out
}

/// QUICK's allocations through `verify_jsonc` for one text, and whether it
/// qualified at the size budget.
fn verify_allocations(text: &str) -> (u64, u32, bool) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/plan-core sits two levels below the workspace root");
    let rules = RulesTable::load(&root.join("rules").join("rules.v1.json"))
        .expect("the committed rules table");
    let snapshot = Snapshot {
        version: SNAPSHOT_VERSION,
        match_seed: 0x0102_0304_0506_0708,
        tick: 3_600,
        ..Snapshot::default()
    }
    .to_bytes()
    .expect("the fixture snapshot");
    let scope = Scope::new(SeatId::new(0), SeatEconomy::default());
    // Once uncounted, so a first touch of a lazily built table (the codec's
    // schema index) is not counted against the call.
    let _first = verify_jsonc(text, &snapshot, &scope, &rules, Depth::Quick);
    let before = allocations();
    let report = verify_jsonc(text, &snapshot, &scope, &rules, Depth::Quick)
        .expect("the rules table is complete");
    let after = allocations();
    (
        after.saturating_sub(before),
        report.size_units,
        report.qualifies,
    )
}

/// The bounds: each the count measured, with about a quarter's headroom, so a
/// return of a tree per node fails rather than hides. The chain through the
/// JSONC document before `vfyq2` made 143,575 and 274,128 allocations on the
/// compact shapes and 186,518 and 360,689 on the commented ones (`main` at
/// `64b6607`); a commented file now costs what a compact one does, because
/// its comments are blanked in one copy and never parsed.
const HANDLER_SHAPE_BOUND: u64 = 3_050; // measured 2,427
const ROUTE_SHAPE_BOUND: u64 = 4_100; // measured 3,266
const HANDLER_COMMENTED_BOUND: u64 = 3_050; // measured 2,427
const ROUTE_COMMENTED_BOUND: u64 = 4_100; // measured 3,266

#[test]
fn verify_jsonc_allocates_within_its_bound_on_both_shapes_at_the_size_budget() {
    let handlers = handler_shape();
    let route = route_shape();
    for (name, text, bound) in [
        (
            "63 handlers, compact",
            handlers.clone(),
            HANDLER_SHAPE_BOUND,
        ),
        ("127 route steps, compact", route.clone(), ROUTE_SHAPE_BOUND),
        (
            "63 handlers, commented",
            commented(&handlers),
            HANDLER_COMMENTED_BOUND,
        ),
        (
            "127 route steps, commented",
            commented(&route),
            ROUTE_COMMENTED_BOUND,
        ),
    ] {
        let (counted, units, qualified) = verify_allocations(&text);
        println!("{name}: {counted} allocations, {units} size units, qualifies {qualified}");
        assert!(qualified, "{name}: the shape qualifies");
        assert!(
            units >= 127,
            "{name}: the shape is at the size budget ({units})"
        );
        assert!(
            counted <= bound,
            "{name}: verify_jsonc made {counted} allocations, past its bound of {bound}"
        );
    }
}
