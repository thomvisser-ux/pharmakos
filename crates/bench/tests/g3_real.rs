// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! G3′-real's figures, published as notices and never gated (S2's plan,
//! section 5 and task `g3h`; item 148's CI ruling).
//!
//! The 8-minute segment at the gate's load, the support search's adversarial
//! tick, and a fourth seat's decision-tick cost, each a `::notice::` per
//! runner with **no threshold**, never compared across operating systems
//! (decisions-log item 116 (6) (b)). The certifying runs are `g3r`'s, on the
//! owner's machine under Windows and WSL2 Linux, three repetitions each (S2's
//! plan, section 5 item 4); these lines are what they record.
//!
//! The binary holds three `#[ignore]`d tests, so `cargo test --workspace` pays
//! for none of them, and each is run **alone, by its exact name**: `--ignored`
//! by itself would run them side by side on libtest's threads, sharing the
//! machine.
//!
//! * `g3_real_segment_support_tick_and_fourth_seat`, every figure:
//!   `cargo xtask perf-alarms g3` runs it on every runner;
//! * `g3_budget_segment`, the segment alone and the one report line
//!   `cargo xtask g3-budget` reads (S2's plan, E18): the CI alarm;
//! * `g3_power_sweep`, the power sweep for `tune2`'s power rows (items 65 and
//!   91), six more segments: run by hand with the certifying runs, never in CI.
//!
//! ```sh
//! cargo test --release -p pharmakos-bench --test g3_real -- --ignored --nocapture \
//!     --exact g3_real_segment_support_tick_and_fourth_seat
//! ```
//!
//! (with `--locked` in CI). **`--release` is not optional**: a debug build's
//! tick is a different number, not a slow version of this one.
//!
//! # The counting allocator
//!
//! This binary's `#[global_allocator]` forwards to `System` and counts: live
//! bytes process-wide (item 68's live heap over the segment) and allocations
//! per thread (the allocations per tick, counted on the thread that steps the
//! match, for the reason `crates/sim/tests/allocations.rs` gives: libtest's
//! own thread allocates too). It is in its own binary so the P1 figures in
//! `tests/perf_alarm.rs` are not taken under it. Writing an allocator needs
//! `unsafe`, scoped to this file as `crates/mesher/tests/allocations.rs`
//! scopes its own.
//!
//! The three measurements are **one test**, run one after the other, so they
//! never share the machine. One that cannot measure prints a `::warning::`
//! and the others still run; the test fails at the end if any did. It prints
//! an empty line before its first notice, for `perf_alarm.rs`'s reason.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use pharmakos_bench::g3::layout::Quarters;
use pharmakos_bench::g3::segment::{self, SegmentPlan};
use pharmakos_bench::g3::{self, G3_SEED, G3_SEGMENT_MS, fourth, stream, support, sweep, variant};
use pharmakos_bench::heap::HeapCounter;
use pharmakos_bench::{Error, workspace_root};

/// Bytes allocated and not yet freed, process-wide.
static LIVE: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// How many times this thread has allocated. `const`-initialised with no
    /// destructor, so touching it from inside the allocator cannot recurse,
    /// and it is never torn down while the thread runs.
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
}

/// A byte count as a `u64`. `usize` is at most 64 bits on every target Rust
/// builds for, so the widening is exact; the allocator cannot return an
/// error, which is why this is a cast and not a `try_from`.
#[allow(
    clippy::as_conversions,
    reason = "usize to u64 widens exactly on every target; an allocator cannot fail softly"
)]
const fn bytes(size: usize) -> u64 {
    size as u64
}

/// Count one allocation on this thread. A `u64` of allocations cannot wrap in
/// a run, and the wrap is written out rather than a panic inside `alloc`.
fn charge() {
    // A `const` thread-local with no destructor is always accessible, so the
    // `AccessError` arm cannot be reached; an allocator has nowhere to report
    // it in any case.
    let _ = ALLOCATIONS.try_with(|count| count.set(count.get().wrapping_add(1)));
}

struct Counting;

#[allow(
    unsafe_code,
    reason = "a GlobalAlloc implementation cannot be written without unsafe; it only forwards \
              to System and counts what System returned"
)]
// SAFETY: every method forwards to `System` with the caller's own arguments,
// so `System`'s guarantees are this allocator's; the counting touches only an
// atomic and a destructor-free thread-local, and never allocates.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract for `layout`.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            charge();
            LIVE.fetch_add(bytes(layout.size()), Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from this allocator, which is `System`'s, with `layout`.
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(bytes(layout.size()), Ordering::Relaxed);
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: as `alloc`.
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            charge();
            LIVE.fetch_add(bytes(layout.size()), Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: `ptr` came from this allocator with `layout`, and the caller
        // upholds `realloc`'s contract for `new_size`.
        let moved = unsafe { System.realloc(ptr, layout, new_size) };
        // A null answer leaves the old block live and unchanged.
        if !moved.is_null() {
            charge();
            LIVE.fetch_add(bytes(new_size), Ordering::Relaxed);
            LIVE.fetch_sub(bytes(layout.size()), Ordering::Relaxed);
        }
        moved
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The segment reads the allocator through this.
struct Counter;

impl HeapCounter for Counter {
    fn live_bytes(&self) -> u64 {
        LIVE.load(Ordering::Relaxed)
    }

    fn allocations(&self) -> u64 {
        // Read on the bench's own thread, never inside the allocator; a `const`
        // thread-local with no destructor is always accessible there.
        ALLOCATIONS.with(Cell::get)
    }
}

/// The fourth seat's segment: 30 seconds, 120 decision ticks at the
/// committed 250 ms cadence, played six times.
const FOURTH_SEGMENT_MS: i32 = 30_000;

#[test]
#[ignore = "a performance alarm, not a gate: cargo xtask perf-alarms runs it with --ignored"]
fn g3_real_segment_support_tick_and_fourth_seat() {
    let root = workspace_root();
    let os = std::env::consts::OS;
    // A notice must start its line; see the module docs.
    println!();
    let mut failed: Vec<Error> = Vec::new();
    let mut report = |title: &str, lines: Result<Vec<String>, Error>| match lines {
        Ok(lines) => lines.iter().for_each(|line| println!("{line}")),
        Err(error) => {
            println!("::warning title={title}::not measured on {os}: {error}");
            failed.push(error);
        }
    };
    report("G3' segment", segment_notices(&root, os));
    report(
        "G3' support search",
        support::measure(&root, G3_SEED).map(|line| vec![g3::support_notice(&line, os)]),
    );
    report("G3' fourth seat", fourth_notice(&root, os));
    assert!(failed.is_empty(), "{failed:?}");
}

/// The gate's segment, measured under the counting allocator.
fn gate_segment(root: &Path) -> Result<segment::SegmentReport, Error> {
    segment::measure(
        root,
        SegmentPlan {
            seed: G3_SEED,
            segment_ms: G3_SEGMENT_MS,
            craters_per_second: stream::CRATERS_PER_SECOND,
            probe_sight: true,
            share: Quarters::FULL,
        },
        Some(&Counter),
    )
}

/// The segment's notices.
fn segment_notices(root: &Path, os: &str) -> Result<Vec<String>, Error> {
    let report = gate_segment(root)?;
    let full = support::footing_at_room(root, G3_SEED)?;
    Ok(vec![
        g3::provenance_notice(&report, os),
        g3::tick_notice(&report, os)?,
        g3::phases_notice(&report, os)?,
        g3::repaths_notice(&report, os)?,
        g3::drift_notice(&report, os),
        g3::combat_notice(&report, os)?,
        g3::sight_notice(&report, os),
        g3::footing_query_notice(&report.footing, &full, os)?,
    ])
}

#[test]
#[ignore = "the G3' CI alarm: cargo xtask g3-budget runs it with --ignored --exact"]
fn g3_budget_segment() {
    let root = workspace_root();
    let os = std::env::consts::OS;
    println!();
    match gate_segment(&root) {
        Ok(report) => {
            println!("{}", g3::provenance_notice(&report, os));
            match g3::tick_notice(&report, os) {
                Ok(line) => println!("{line}"),
                Err(error) => panic!("the tick notice: {error}"),
            }
            println!("{}", g3::budget_report_line(&report));
        }
        // No report line: `cargo xtask g3-budget` fails on its absence too.
        Err(error) => panic!("the G3' segment was not measured on {os}: {error}"),
    }
}

#[test]
#[ignore = "the power sweep, run by hand with the certifying runs (--ignored --exact)"]
fn g3_power_sweep() {
    let root = workspace_root();
    let os = std::env::consts::OS;
    println!();
    match sweep::measure(&root, G3_SEED, Some(&Counter)).and_then(|s| g3::sweep_notices(&s, os)) {
        Ok(lines) => lines.iter().for_each(|line| println!("{line}")),
        Err(error) => panic!("the power sweep was not measured on {os}: {error}"),
    }
}

/// The fourth seat's notice, under the bench variant.
fn fourth_notice(root: &Path, os: &str) -> Result<Vec<String>, Error> {
    let bench = variant::bench_rules(root, 3, segment::gate_load(Quarters::FULL)?)?;
    let line = fourth::measure(root, &bench.rules, G3_SEED, FOURTH_SEGMENT_MS)?;
    Ok(vec![g3::fourth_notice(&line, os)?])
}
