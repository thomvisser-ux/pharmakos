// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Per-chunk mesh CPU p99, as a regression **alarm** and never a gate.
//!
//! Skeleton plan section 7 decision 23: performance is published as a `::notice::`
//! annotation per runner, with **no threshold at this stage**. AGENTS.md section 9 item 11
//! puts budgets with the gates that set them, and `tests/golden/README.md`'s "What is not
//! here" says why a perf number is not a golden: a figure that cannot vary across platforms
//! must not be compared across them, and one that can vary is not a golden.
//!
//! Two G1 findings shape the measurement rather than the threshold:
//!
//! * **p99, never max.** G1 section 10.2 ran identical work three times and got best-of-five
//!   maxima of 376, 1 157 and 378 microseconds with the p50 moving by 2 — a three-times
//!   spread on the tail from scheduler preemption alone. "Any CI budget set from a `max`
//!   will flap." G2 section 9.11 says the same thing more bluntly: a max is not a property
//!   of the code.
//! * **Best of five per chunk.** A single `Instant` sample per meshing is not reproducible
//!   on Windows, so each meshing is timed five times and the minimum kept; the single-shot
//!   figure is reported beside it so the gap — which is preemption — is visible rather than
//!   hidden.
//!
//! This test is `#[ignore]`d, so `cargo test --workspace` does not pay for it. **T20 wires
//! it into CI** as
//!
//! ```sh
//! cargo test --release -p pharmakos-mesher --test perf_alarm -- --ignored --nocapture
//! ```
//!
//! and lets the `::notice::` line through to the job summary; until then, running it by
//! hand is what it is for. **`--release` is not optional.** A debug build of this crate
//! measures about 9 900 microseconds p99 against the same 672 in release on the owner's
//! machine — overflow checks, bounds checks and no inlining — so a debug figure is not a
//! slow version of the real one, it is a different number entirely.
//!
//! For orientation, the first run on the owner's machine (Windows, release) reported **p99
//! 672 microseconds, p50 382**, on a 582-quad cratered chunk, against G1's 503 p99 on its
//! own cratered case. The two are not the same chunk and this one is deliberately the
//! heavier, so the gap is context rather than a regression — which is exactly why the number
//! is published and not gated.
//!
//! Wall-clock time is legal here: this crate is walled (`WALLED_PACKAGES` in
//! `xtask/src/main.rs`), `cargo xtask clippy` pass 2 passes the clock allowance on the
//! command line, and nothing timed here is ever read by the sim.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules — \
              not an integration test's helper functions. A panic is this file's failure \
              report (clippy.toml's own wording)."
)]

use std::time::Instant;

use pharmakos_mesher::{
    AIR, ChunkBorders, ChunkGrid, LightField, LightParams, MeshBuffers, Mesher, chunk_view,
    map_offset,
};

/// How many meshings the sample is taken over.
const MESHINGS: usize = 1_000;

/// How many times each meshing is timed, with the minimum kept.
const REPEATS: usize = 5;

/// G1 section 10.2's p99 of the best-of-five sample, for the notice line to sit beside.
/// Reference, not threshold.
const G1_FLAT_P99_US: u128 = 343;
/// The same, for a freshly cratered chunk.
const G1_CRATERED_P99_US: u128 = 503;

/// The cratered chunk G1 timed at 503 microseconds: a solid block in material bands with a
/// sphere blown out of it.
fn cratered(grid: ChunkGrid) -> Vec<u8> {
    let mut materials = vec![AIR; grid.voxel_count()];
    let [voxels_x, _, voxels_z] = grid.voxels();
    for z in 0..voxels_z {
        for x in 0..voxels_x {
            for y in 0..24 {
                write(
                    &mut materials,
                    grid,
                    x,
                    y,
                    z,
                    u8::try_from(1 + ((y >> 3_u32) % 3)).unwrap_or(1),
                );
            }
        }
    }
    for z in 0..voxels_z {
        for y in 0..24 {
            for x in 0..voxels_x {
                if (x - 16_i32).pow(2) + (y - 22_i32).pow(2) + (z - 16_i32).pow(2) <= 121 {
                    write(&mut materials, grid, x, y, z, AIR);
                }
            }
        }
    }
    materials
}

fn write(materials: &mut [u8], grid: ChunkGrid, x: i32, y: i32, z: i32, material: u8) {
    if let Some(at) = map_offset(grid, x, y, z) {
        if let Some(slot) = materials.get_mut(at) {
            *slot = material;
        }
    }
}

/// The value at the given percentile of a sorted sample, by nearest rank.
fn percentile(sorted: &[u128], percent: u128) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = sorted
        .len()
        .saturating_mul(usize::try_from(percent).unwrap_or(99))
        .checked_div(100)
        .unwrap_or(0)
        .min(sorted.len().saturating_sub(1));
    sorted.get(rank).copied().unwrap_or(0)
}

#[test]
#[ignore = "a performance alarm, not a gate: T20 runs it with --ignored and publishes the ::notice:: line"]
fn per_chunk_mesh_cpu_p99() {
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
    // Warm up, so the sample measures meshing rather than the first growth of the buffers.
    for _ in 0..16 {
        mesher
            .mesh_chunk_into(&view, &mut buffers)
            .expect("under the 16-bit cap");
    }

    let mut best: Vec<u128> = Vec::with_capacity(MESHINGS);
    let mut single: Vec<u128> = Vec::with_capacity(MESHINGS);
    for meshing in 0..MESHINGS {
        let mut lowest = u128::MAX;
        for repeat in 0..REPEATS {
            let started = Instant::now();
            mesher
                .mesh_chunk_into(&view, &mut buffers)
                .expect("under the 16-bit cap");
            let elapsed = started.elapsed().as_micros();
            lowest = lowest.min(elapsed);
            if repeat == 0 && meshing < MESHINGS {
                single.push(elapsed);
            }
        }
        best.push(lowest);
    }
    best.sort_unstable();
    single.sort_unstable();

    let p50 = percentile(&best, 50);
    let p99 = percentile(&best, 99);
    let single_p99 = percentile(&single, 99);

    // GitHub hides job logs and step summaries from logged-out viewers, so the number goes
    // out as a `::notice::` annotation (G1 section 10.12).
    println!(
        "::notice::mesher p99 {p99} us (G1 reference {G1_FLAT_P99_US} us flat / \
         {G1_CRATERED_P99_US} us cratered)"
    );
    println!(
        "::notice::mesher p50 {p50} us, single-shot p99 {single_p99} us over {MESHINGS} \
         meshings of {} quads; the gap between the two p99s is scheduler preemption",
        buffers.quad_count()
    );

    // No threshold. The only thing asserted is that the measurement measured something.
    assert!(
        !buffers.is_empty(),
        "the cratered chunk produced no surface"
    );
    assert_eq!(best.len(), MESHINGS);
}
