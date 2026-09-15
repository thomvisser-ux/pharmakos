// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Zero allocations per meshing, asserted from the day the mesher lands.
//!
//! Spike G1 measured 0.000 allocations per meshing on a flat chunk and 0.001 on a cratered
//! one, and the reason it could is that the Windows allocator is markedly slower than
//! glibc's for the many short-lived vectors a naive mesher produces (G1 plan section 6).
//! An allocation per meshing would make the per-chunk budget allocator-dependent, and the
//! per-chunk budget is what K = 4 was chosen against.
//!
//! It is here rather than at a later hardening task for the same reason `crates/sim` has
//! its own: the property is far cheaper to keep than to restore. [`Mesher`] owns its padded
//! copy and its mask, [`MeshBuffers`] keeps its capacity across meshings, and
//! [`DrainQueue::drain_into`] sorts in scratch it owns. A pull request that adds a `Vec` to
//! any of those three paths fails here, in itself.
//!
//! **One test in this file, on purpose.** The counter is process-wide, and a second test
//! allocating on another harness thread would be counted as this one's.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules — \
              not an integration test's helper functions. A panic is this file's failure \
              report (clippy.toml's own wording)."
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

use pharmakos_mesher::{
    AIR, ChunkBorders, ChunkGrid, DrainBudget, DrainQueue, LightField, LightParams, MeshBuffers,
    Mesher, chunk_view, map_offset,
};

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

/// The system allocator, counting.
struct Counting;

// SAFETY: every method forwards to `System` unchanged; the only addition is a relaxed
// counter increment, which cannot affect the allocation itself.
//
// `unsafe_code` is denied workspace-wide (`[workspace.lints.rust]`). A `GlobalAlloc`
// implementation cannot be written without `unsafe`, and the alternative — not asserting
// the property — is what G1's allocation row exists to prevent. The allowance is scoped to
// this one item, in a test target, and is the same shape `crates/sim/tests/allocations.rs`
// already carries.
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

/// A one-chunk map with a cratered block in it — the shape G1 measured as the worst case,
/// and the one that grows the output buffers furthest.
fn cratered(grid: ChunkGrid) -> Vec<u8> {
    let mut materials = vec![AIR; grid.voxel_count()];
    let [voxels_x, _, voxels_z] = grid.voxels();
    for z in 0..voxels_z {
        for x in 0..voxels_x {
            for y in 0..24 {
                if let Some(at) = map_offset(grid, x, y, z) {
                    if let Some(slot) = materials.get_mut(at) {
                        *slot = u8::try_from(1 + ((y >> 3_u32) % 3)).unwrap_or(1);
                    }
                }
            }
        }
    }
    for z in 0..voxels_z {
        for y in 0..24 {
            for x in 0..voxels_x {
                let squared = (x - 16_i32).pow(2) + (y - 22_i32).pow(2) + (z - 16_i32).pow(2);
                if squared <= 121 {
                    if let Some(at) = map_offset(grid, x, y, z) {
                        if let Some(slot) = materials.get_mut(at) {
                            *slot = AIR;
                        }
                    }
                }
            }
        }
    }
    materials
}

#[test]
fn a_warm_meshing_and_a_warm_drain_allocate_nothing() {
    let grid = ChunkGrid::new(1, 1, 1).expect("a one-chunk grid is legal");
    let params = LightParams::new(15, 1).expect("the committed rules-table rows");
    let materials = cratered(grid);

    let mut field = LightField::new(grid, params);
    field
        .bake_all(&materials)
        .expect("the map is the right size");
    let mut borders = ChunkBorders::new();
    borders.gather(grid, &materials, field.all(), params, 0);
    let view = chunk_view(&materials, &field, &borders, 0).expect("chunk 0 is inside the grid");

    let mut mesher = Mesher::new(params);
    let mut buffers = MeshBuffers::empty();
    let mut drain = DrainQueue::new(
        grid,
        DrainBudget {
            surfaces_per_frame: 4,
            bytes_per_frame: 524_288,
            age_frames: 2,
        },
    );
    let mut taken: Vec<u32> = Vec::new();

    // Warm up. The first meshing grows every output array, and the first drain grows the
    // queue's sort scratch and the caller's buffer; a capacity that is right for the first
    // is right for the ten thousandth, because the chunk is a fixed 32 cubed and the
    // surface budget is fixed at four.
    for frame in 0..4_u64 {
        mesher
            .mesh_chunk_into(&view, &mut buffers)
            .expect("the cratered chunk is well under the 16-bit cap");
        drain.push(0, frame);
        taken.clear();
        drain.drain_into(frame, [0, 0, 0], |_| 1_024, &mut taken);
    }

    let before = ALLOCATIONS.load(Ordering::Relaxed);
    for frame in 4..200_u64 {
        mesher
            .mesh_chunk_into(&view, &mut buffers)
            .expect("the cratered chunk is well under the 16-bit cap");
        drain.push(0, frame);
        taken.clear();
        drain.drain_into(frame, [0, 0, 0], |_| 1_024, &mut taken);
    }
    let during = ALLOCATIONS.load(Ordering::Relaxed) - before;

    assert_eq!(
        during, 0,
        "196 meshings and drains allocated {during} times. A warm meshing must allocate \
         nothing: keep the scratch on the Mesher, reuse the caller's MeshBuffers, and take \
         the drain's output buffer in rather than returning a fresh Vec."
    );
    assert!(
        !buffers.is_empty(),
        "the cratered chunk produced no surface"
    );
    assert_eq!(taken, vec![0_u32], "the drain kept working while warm");
}
