// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! QUICK's cost at the size budget, pinned by counted allocations rather than
//! by a clock (S3's `vfyq`, decisions-log items 157 (7) and 158).
//!
//! P1's bar is wall time, and wall time is measured in the walled
//! `crates/bench` (AGENTS.md section 4.5); this crate is deterministic and
//! reads no clock. What made QUICK slow was not arithmetic but allocation: an
//! owned JSON tree, a pointer string built at every node of every walk, and
//! the playbook encoded three times over for its fingerprint, each a heap
//! allocation per node. So the regression test counts allocations. A change
//! that builds a pointer per node again, or a tree, moves the count by
//! thousands on these shapes, and this test goes red in the pull request that
//! did it.
//!
//! The shapes are item 158 (1)'s two: 63 handlers, and 127 route steps, each
//! with a 24-node condition (21 predicates under three nested groups), 127
//! size units either way; and `budget_128`, the committed case at the budget.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules — not an \
              integration test's helper functions. A panic is this file's failure report."
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::PathBuf;

use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::snapshot::{SNAPSHOT_VERSION, Snapshot};
use pharmakos_sim::tables::SeatId;
use pharmakos_verifier::{Input, Scope, verify};

thread_local! {
    /// How many times **this thread** has allocated.
    ///
    /// `const`-initialised and holding a type with no destructor, so touching
    /// it from inside the global allocator neither allocates nor registers a
    /// teardown hook (the pattern `crates/sim/tests/allocations.rs` set).
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
}

/// Count one allocation against the calling thread.
fn charge() {
    // `try_with`: during thread teardown the slot is gone, and an allocation
    // then is not this test's business.
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
// as `crates/sim/tests/allocations.rs` and `crates/mesher/tests/allocations.rs`
// scope theirs, and the pull request raises it.
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

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

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

const HEAD: &str = r#""schema_version":{"major":1},"meta":{"title":"vfyq","author_kind":"HUMAN"},"kind":"PLAYBOOK""#;
const TAIL: &str = r#""on_death":{"on_respawn":"CONTINUE","max_deaths_before_fallback":1},"fallback":{"hold":{"at":{"beacon_anchor":{"safest":{}}}}}"#;

/// Item 157 (7)'s shape: 63 handlers, each with a 24-node `when`.
fn handler_shape() -> Vec<u8> {
    let mut next = 0;
    let handlers: Vec<String> = (0..63)
        .map(|id| {
            format!(
                r#"{{"id":"h{id:02}","when":{},"body":[{{"label":"h{id:02}_a","hold":{{"ms":1000}}}}],"resume":"CONTINUE","cooldown_ms":5000,"max_fires":2}}"#,
                condition(id, &mut next)
            )
        })
        .collect();
    format!(
        r#"{{{HEAD},"declarative":{{"route":[{{"label":"r00","hold":{{"ms":1000}}}}],"handlers":[{}]}},{TAIL}}}"#,
        handlers.join(",")
    )
    .into_bytes()
}

/// Item 158 (1)'s larger shape: 127 route steps, each with a 24-node
/// `skip_if`.
fn route_shape() -> Vec<u8> {
    let mut next = 0;
    let steps: Vec<String> = (0..127)
        .map(|id| {
            format!(
                r#"{{"label":"r{id:03}","skip_if":{},"hold":{{"ms":1000}}}}"#,
                condition(id, &mut next)
            )
        })
        .collect();
    format!(
        r#"{{{HEAD},"declarative":{{"route":[{}]}},{TAIL}}}"#,
        steps.join(",")
    )
    .into_bytes()
}

/// QUICK's allocations for one playbook, with the input assembled beforehand
/// (as `Input::new` is, once per call, outside what this pins), and whether it
/// qualified at the size budget.
fn quick_allocations(playbook: &[u8]) -> (u64, u32, bool) {
    let rules = RulesTable::load(&workspace_root().join("rules").join("rules.v1.json"))
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
    let input = Input::new(playbook, &snapshot, &scope, &rules).expect("an input");
    // Once untimed and uncounted, so a first touch of a lazily built table
    // (the codec's schema index) is not counted against the call.
    let _first = verify(&input, Depth::Quick);
    let before = allocations();
    let report = verify(&input, Depth::Quick);
    let after = allocations();
    (
        after.saturating_sub(before),
        report.size_units,
        report.qualifies,
    )
}

/// The bounds: each the count measured at `vfyq`'s head, with about a
/// quarter's headroom. Most of what is left is prost's own message tree, a
/// `Vec` for every group and a `String` for every label. The path before
/// `vfyq` made 118,538, 227,365 and 18,964 allocations on the same three (main
/// at `0f6466f`). The count is a property of this code, not of a machine: it
/// reads no clock and spawns no thread.
const HANDLER_SHAPE_BOUND: u64 = 2_000; // measured 1,576
const ROUTE_SHAPE_BOUND: u64 = 2_500; // measured 1,967
const BUDGET_CASE_BOUND: u64 = 3_400; // measured 2,707

#[test]
fn quick_allocates_within_its_bound_on_both_shapes_at_the_size_budget() {
    let budget = std::fs::read(
        workspace_root()
            .join("crates")
            .join("verifier")
            .join("tests")
            .join("cases")
            .join("budget_128.json"),
    )
    .expect("the committed budget case");
    // `budget_128` names beacons this view does not hold, so it does not
    // qualify here; its diagnostics are part of what it costs.
    for (name, playbook, bound, qualifies) in [
        ("63 handlers", handler_shape(), HANDLER_SHAPE_BOUND, true),
        ("127 route steps", route_shape(), ROUTE_SHAPE_BOUND, true),
        ("budget_128", budget, BUDGET_CASE_BOUND, false),
    ] {
        let (counted, units, qualified) = quick_allocations(&playbook);
        println!("{name}: {counted} allocations, {units} size units, qualifies {qualified}");
        assert_eq!(qualified, qualifies, "{name}: the verdict");
        assert!(
            units >= 127,
            "{name}: the shape is at the size budget ({units})"
        );
        assert!(
            counted <= bound,
            "{name}: QUICK made {counted} allocations, past its bound of {bound}"
        );
    }
}
