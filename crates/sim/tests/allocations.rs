// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Zero allocations per tick, asserted from day one (G3′ §9.17).
//!
//! > *"Zero allocations per tick is achievable and worth defending. It took
//! > removing exactly one `Vec` return from the kill-credit settlement path.
//! > With it, plan §5's whole allocator risk row evaporates and the budget
//! > stops being allocator-dependent. The real sim should have an
//! > allocations-per-tick assertion in its perf gate from S1, not as a later
//! > hardening task."*
//!
//! It is here rather than at S1 because it is far cheaper to keep than to
//! restore: every table below fixes its count at construction, the canonical
//! encoder reuses one buffer, the broadphase's counting sort writes into arrays
//! allocated once, and the voxel phase's edit queue, pending set and crater
//! scratch are all sized to the map at construction. The measured loop craters
//! on every tick for that last reason: an empty edit queue would leave the
//! whole voxel phase outside the assertion. A pull request that adds a `Vec` to a tick phase
//! fails here, in itself, rather than in a budget measurement three stages
//! later.
//!
//! **One test in this file, on purpose.** The counter is process-wide, and a
//! second test allocating on another harness thread would be counted as this
//! one's.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules — not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

use pharmakos_sim::encoding::Enc;
use pharmakos_sim::voxels::VoxelEdit;
use pharmakos_sim::{RulesTable, World, WorldConfig};

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

/// The system allocator, counting.
struct Counting;

// SAFETY: every method forwards to `System` unchanged; the only addition is a
// relaxed counter increment, which cannot affect the allocation itself.
//
// `unsafe_code` is denied workspace-wide (`[workspace.lints.rust]`). A
// `GlobalAlloc` implementation cannot be written without `unsafe`, and the
// alternative — not asserting the property — is what G3′ warned against. The
// allowance is scoped to this one item, in a test target, and is raised in the
// pull request rather than edited into the contract lint table (the same route
// skeleton plan decision 12 sets out for gdext's scoped allow).
#[allow(
    unsafe_code,
    reason = "a GlobalAlloc implementation cannot be written without unsafe; scoped to this test's counting allocator, which only forwards to System"
)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

#[test]
fn a_tick_allocates_nothing() {
    let rules = RulesTable::load(
        &std::path::Path::new("..")
            .join("..")
            .join("rules")
            .join("rules.v1.json"),
    )
    .expect("the committed rules table loads");
    let mut world = World::new(&WorldConfig {
        match_seed: pharmakos_sim::DETERMINISM_MATCH_SEED,
        seats: pharmakos_sim::DETERMINISM_SEATS,
        units_per_seat: pharmakos_sim::DETERMINISM_UNITS_PER_SEAT,
        rules,
    })
    .expect("the rules table describes a map and a broadphase grid");
    let mut enc = Enc::with_capacity(64 * 1024);

    // Warm up: the first tick may still grow the encoder's buffer, and a
    // capacity that is right for tick 1 is right for tick 10 000 because every
    // table's count is fixed at construction.
    // The warm-up craters too, so the voxel phase's own scratch — the edit
    // queue, the store's pending and settled lists, the crater's touched list —
    // is paid for out here and the measured loop below sees it at its steady
    // capacity.
    for step in 0..50 {
        crater_at(&mut world, step);
        let _ = world.step(&mut enc);
    }
    let capacity_after_warmup = enc.encoded_len();

    let before = ALLOCATIONS.load(Ordering::Relaxed);
    for step in 0..500 {
        // Every tick of the measured loop goes through the voxel phase with
        // work in it: a crater is the edit S2 will file by the thousand, and an
        // empty queue would leave `settle`, `mark` and `crater` outside the
        // property this file exists to defend.
        crater_at(&mut world, step + 50);
        let _ = world.step(&mut enc);
    }
    let during = ALLOCATIONS.load(Ordering::Relaxed) - before;

    assert_eq!(
        during, 0,
        "500 ticks allocated {during} times. A tick must allocate nothing: fix the count at \
         construction, reuse the caller's buffer, or hand the scratch space in."
    );
    assert_eq!(
        enc.encoded_len(),
        capacity_after_warmup,
        "the canonical encoding changed length between ticks, which means the encoder is not \
         fixed-stride after all"
    );
}

/// Queue one crater on a fresh patch of ground, so the edit actually changes
/// voxels and the whole voxel phase runs.
///
/// The columns walk a coprime stride across the map, so a later tick never
/// craters air a former tick already removed; the `z` is the column's own
/// surface, so the ball always has rock to take.
fn crater_at(world: &mut World, step: u32) {
    let size = world.voxels().size();
    let width = i32::try_from(size[0]).expect("the map fits in an i32");
    let depth = i32::try_from(size[1]).expect("the map fits in an i32");
    let x = i32::try_from(step.wrapping_mul(37) % 4096).expect("a column index") % width;
    let y = i32::try_from(step.wrapping_mul(53) % 4096).expect("a column index") % depth;
    let z = world.voxels().top_solid_z(x, y).unwrap_or(0);
    assert!(
        world.request_voxel_edit(VoxelEdit::Crater {
            centre: [x, y, z],
            radius: 3,
        }),
        "the edit queue is full at step {step}"
    );
}
