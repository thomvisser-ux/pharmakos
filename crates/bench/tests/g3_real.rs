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
//! The test is `#[ignore]`d, so `cargo test --workspace` does not pay for it.
//! `cargo xtask perf-alarms` runs it on every runner as
//!
//! ```sh
//! cargo test --release -p pharmakos-bench --test g3_real -- --ignored --nocapture
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

#![allow(
    unsafe_code,
    reason = "a GlobalAlloc implementation cannot be written without unsafe; scoped to this \
              test binary's counting allocator, which only forwards to System"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use pharmakos_bench::g3::segment::{self, SegmentPlan};
use pharmakos_bench::g3::{self, G3_SEED, G3_SEGMENT_MS, fourth, stream, support, variant};
use pharmakos_bench::heap::HeapCounter;
use pharmakos_bench::{Error, workspace_root};

/// Bytes allocated and not yet freed, process-wide.
static LIVE: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// How many times this thread has allocated. `const`-initialised with no
    /// destructor, so touching it from inside the allocator cannot recurse.
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
}

/// A `usize` byte count as a `u64`, which every target this builds for holds.
fn bytes(size: usize) -> u64 {
    u64::try_from(size).unwrap_or(u64::MAX)
}

fn charge() {
    let _ = ALLOCATIONS.try_with(|count| count.set(count.get().saturating_add(1)));
}

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        charge();
        LIVE.fetch_add(bytes(layout.size()), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(bytes(layout.size()), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        charge();
        LIVE.fetch_add(bytes(layout.size()), Ordering::Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        charge();
        LIVE.fetch_add(bytes(new_size), Ordering::Relaxed);
        LIVE.fetch_sub(bytes(layout.size()), Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
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
        ALLOCATIONS.try_with(Cell::get).unwrap_or(0)
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
        support::measure(&root, G3_SEED).map(|line| {
            vec![
                g3::support_notice(&line, os),
                g3::footing_query_notice(&line.footing, os),
            ]
        }),
    );
    report("G3' fourth seat", fourth_notice(&root, os));
    assert!(failed.is_empty(), "{failed:?}");
}

/// The segment's notices.
fn segment_notices(root: &Path, os: &str) -> Result<Vec<String>, Error> {
    let report = segment::measure(
        root,
        SegmentPlan {
            seed: G3_SEED,
            segment_ms: G3_SEGMENT_MS,
            craters_per_second: stream::CRATERS_PER_SECOND,
            probe_sight: true,
        },
        Some(&Counter),
    )?;
    Ok(vec![
        g3::provenance_notice(&report, os),
        g3::tick_notice(&report, os)?,
        g3::phases_notice(&report, os)?,
        g3::repaths_notice(&report, os)?,
        g3::drift_notice(&report, os),
        g3::combat_notice(&report, os)?,
        g3::sight_notice(&report, os),
    ])
}

/// The fourth seat's notice, under the bench variant.
fn fourth_notice(root: &Path, os: &str) -> Result<Vec<String>, Error> {
    let bench = variant::bench_rules(root, 3, segment::gate_load())?;
    let line = fourth::measure(root, &bench.rules, G3_SEED, FOURTH_SEGMENT_MS)?;
    Ok(vec![g3::fourth_notice(&line, os)?])
}
