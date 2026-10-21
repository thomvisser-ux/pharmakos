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
//! This test is `#[ignore]`d, so `cargo test --workspace` does not pay for it. The
//! `perf alarms (<os>)` jobs run it on all three operating systems through
//! `cargo xtask perf-alarms`, as
//!
//! ```sh
//! cargo test --release -p pharmakos-mesher --test perf_alarm -- --ignored --nocapture
//! ```
//!
//! (with `--locked` in CI), and its `::notice::` lines reach the run's annotations. It has
//! no threshold and is never compared across runners: the jobs are not required, and a
//! failed measurement is a `::warning::` that leaves the job green (decisions-log item
//! 116 (6)(b)). Run by hand, it prints the same lines. **`--release` is not optional.** A debug build of this crate
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

use std::sync::{Mutex, PoisonError};
use std::time::Instant;

/// Held by each measurement for its whole run, so the two alarms in this binary never
/// share the machine: libtest runs tests in parallel, and `cargo xtask perf-alarms` passes
/// no `--test-threads=1` (its doc says why), so each would otherwise skew the other.
static MACHINE: Mutex<()> = Mutex::new(());

use pharmakos_mesher::{
    AIR, ChunkBorders, ChunkGrid, DrainBudget, DrainQueue, LightField, LightParams, MeshBuffers,
    Mesher, chunk_view, gpu_surface_bytes, map_offset,
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
    if let Some(at) = map_offset(grid, x, y, z)
        && let Some(slot) = materials.get_mut(at)
    {
        *slot = material;
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
#[ignore = "a performance alarm, not a gate: cargo xtask perf-alarms runs it with --ignored"]
fn per_chunk_mesh_cpu_p99() {
    let _alone = MACHINE.lock().unwrap_or_else(PoisonError::into_inner);
    let grid = ChunkGrid::new(1, 1, 1).expect("a one-chunk grid is legal");
    let params = LightParams::new(15, 1).expect("the committed rules-table rows");
    let materials = cratered(grid);

    let mut field = LightField::new(grid, params);
    field
        .bake_all(&materials)
        .expect("the map is the right size");
    let mut borders = ChunkBorders::new();
    borders
        .gather(grid, &materials, field.all(), params, 0)
        .expect("chunk 0 is inside the grid");
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

/// The committed map's chunk grid: `map.size_*` 384 x 384 x 64 in rules/rules.v1.json, in
/// the mesher's axes (y up).
const MAP_CHUNKS: [u32; 3] = [12, 2, 12];

/// Frames a view takes to arrive: the watch rig asks for one per pacer period
/// (`crates/client-gdext/src/pacer.rs`'s `PACER_PERIOD_US`, 100 ms), six frames at 60 fps.
const VIEW_EVERY_FRAMES: u64 = 6;

/// G1's gate is "within 3 frames", measured here from the frame a view arrives.
const G1_FRAMES: u64 = 3;

/// The frame G1's 60 fps leaves, in microseconds.
const FRAME_US: u128 = 16_667;

/// G1 section 10.8's modelled upload rate, bytes per millisecond: the FFI byte count
/// against path B's measured cost per chunk.
const UPLOAD_BYTES_PER_MS: u128 = 1_740_000;

/// A crater's radius in voxels, G1's.
const CRATER_RADIUS: i32 = 4;

/// The committed rules table's drain budget (its `mesher` row).
fn committed_budget() -> DrainBudget {
    DrainBudget {
        surfaces_per_frame: 4,
        bytes_per_frame: 524_288,
        age_frames: 2,
    }
}

/// How a view's craters are relit and queued.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Relight {
    /// S2's: the edited boxes through `LightField::relight`, queuing what moved.
    Bounded,
    /// The client before S2: each changed chunk's whole box re-baked and its padded box
    /// queued, and more than 16 changed chunks one whole bake with every chunk queued.
    WholeAbove16,
}

/// What one stream measured.
struct Stream {
    latency_p50: u64,
    latency_p99: u64,
    latency_max: u64,
    cpu_p99_us: u128,
    over_frame: usize,
    max_depth: usize,
    uploads: u64,
    left: usize,
}

/// Generated-looking ground over the whole map: stone to y = 20, dirt to y = 24.
fn ground(grid: ChunkGrid) -> Vec<u8> {
    let mut materials = vec![AIR; grid.voxel_count()];
    let [voxels_x, _, voxels_z] = grid.voxels();
    for z in 0..voxels_z {
        for x in 0..voxels_x {
            for y in 0..24 {
                write(&mut materials, grid, x, y, z, if y < 20 { 1 } else { 2 });
            }
        }
    }
    materials
}

/// Blows one crater out of the ground and answers the box of voxels it removed.
fn crater(materials: &mut [u8], grid: ChunkGrid, centre: [i32; 3]) -> Option<([i32; 3], [i32; 3])> {
    let r = CRATER_RADIUS;
    let mut bounds: Option<([i32; 3], [i32; 3])> = None;
    for y in centre[1] - r..=centre[1] + r {
        for z in centre[2] - r..=centre[2] + r {
            for x in centre[0] - r..=centre[0] + r {
                let d = [x - centre[0], y - centre[1], z - centre[2]];
                if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] > r * r {
                    continue;
                }
                let solid = map_offset(grid, x, y, z)
                    .and_then(|at| materials.get(at))
                    .is_some_and(|material| *material != AIR);
                if solid {
                    write(materials, grid, x, y, z, AIR);
                    bounds = Some(match bounds {
                        None => ([x, y, z], [x, y, z]),
                        Some((lo, hi)) => (
                            [lo[0].min(x), lo[1].min(y), lo[2].min(z)],
                            [hi[0].max(x), hi[1].max(y), hi[2].max(z)],
                        ),
                    });
                }
            }
        }
    }
    bounds
}

/// The old client's relight of one view: by changed chunk, whole-map above 16.
fn relight_whole_above_16(
    field: &mut LightField,
    materials: &[u8],
    grid: ChunkGrid,
    edits: &[([i32; 3], [i32; 3])],
    dirty: &mut Vec<u32>,
) {
    let mut changed: Vec<[i32; 3]> = Vec::new();
    for (lo, hi) in edits {
        for y in [lo[1], hi[1]] {
            for z in [lo[2], hi[2]] {
                for x in [lo[0], hi[0]] {
                    changed.push([x.div_euclid(32), y.div_euclid(32), z.div_euclid(32)]);
                }
            }
        }
    }
    changed.sort_unstable();
    changed.dedup();
    if changed.len() > 16 {
        field.bake_all(materials).expect("the right size");
        dirty.extend(0..grid.chunk_count());
    } else {
        for coords in &changed {
            let lo = coords.map(|value| value * 32);
            let hi = lo.map(|value| value + 31);
            field.rebake_box(materials, lo, hi).expect("the right size");
            field.dirty_chunks(lo, hi, dirty);
        }
    }
}

/// G1's headless CPU proxy (G1 section 10.8; its step 6, "S2 will need this"), on the
/// committed map's size on flat ground (see [`ground`]) and the client's own path:
/// `explosions_per_s` craters a second land at deterministic places, each view carries the
/// craters since the last, the view's edits are relit and queued as `relight` says, and
/// every frame drains under the committed budget and meshes what it drains with the real
/// mesher. Upload is modelled as G1's was.
#[allow(
    clippy::too_many_lines,
    reason = "one frame loop, read top to bottom in the order the proxy runs it"
)]
fn crater_stream(explosions_per_s: u64, frames: u64, relight: Relight) -> Stream {
    let [cx, cy, cz] = MAP_CHUNKS;
    let grid = ChunkGrid::new(cx, cy, cz).expect("the committed map is a legal grid");
    let params = LightParams::new(15, 1).expect("the committed rules-table rows");
    let mut materials = ground(grid);
    let mut field = LightField::new(grid, params);
    field
        .bake_all(&materials)
        .expect("the map is the right size");
    let mut queue = DrainQueue::new(grid, committed_budget());
    let mut mesher = Mesher::new(params);
    let mut borders = ChunkBorders::new();
    let mut buffers = MeshBuffers::empty();
    let chunks = usize::try_from(grid.chunk_count()).expect("a chunk count");
    // G1's FFI byte count per chunk, until a chunk has been meshed once.
    let mut sizes = vec![96_623_usize; chunks];
    let mut arrived: Vec<Option<u64>> = vec![None; chunks];
    let [voxels_x, _, voxels_z] = grid.voxels();
    let mut seed = 0x9e37_79b9_u32;
    let mut next = |below: i32| {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        i32::try_from(seed >> 16).expect("16 bits fit") % below
    };
    let mut latencies: Vec<u64> = Vec::new();
    let mut cpu: Vec<u128> = Vec::with_capacity(usize::try_from(frames).expect("frames"));
    let mut issued = 0_u64;
    let mut pending: Vec<([i32; 3], [i32; 3])> = Vec::new();
    let mut dirty: Vec<u32> = Vec::new();
    let mut uploads = 0_u64;
    for frame in 0..frames {
        // The explosions due by this frame land in the sim; the client sees them with the
        // next view. Placing them is the sim's work, so it is not timed.
        let due = ((frame + 1) * explosions_per_s).div_euclid(60);
        while issued < due {
            issued += 1;
            let centre = [
                CRATER_RADIUS + next(voxels_x - 2 * CRATER_RADIUS),
                22 + next(3),
                CRATER_RADIUS + next(voxels_z - 2 * CRATER_RADIUS),
            ];
            pending.extend(crater(&mut materials, grid, centre));
        }
        let started = Instant::now();
        let mut upload_bytes = 0_u128;
        if frame % VIEW_EVERY_FRAMES == 0 && !pending.is_empty() {
            dirty.clear();
            match relight {
                Relight::Bounded => field
                    .relight(&materials, &pending, &mut dirty)
                    .expect("the map is the right size"),
                Relight::WholeAbove16 => {
                    relight_whole_above_16(&mut field, &materials, grid, &pending, &mut dirty);
                }
            }
            pending.clear();
            for chunk in &dirty {
                queue.push(*chunk, frame);
                if let Some(slot) = arrived.get_mut(usize::try_from(*chunk).expect("index"))
                    && slot.is_none()
                {
                    *slot = Some(frame);
                }
            }
        }
        let drained = queue.drain(frame, [6, 1, 6], |chunk| {
            sizes
                .get(usize::try_from(chunk).expect("index"))
                .copied()
                .expect("a chunk of the grid")
        });
        for chunk in drained {
            borders
                .gather(grid, &materials, field.all(), params, chunk)
                .expect("inside the grid");
            let view = chunk_view(&materials, &field, &borders, chunk).expect("inside");
            mesher
                .mesh_chunk_into(&view, &mut buffers)
                .expect("under the 16-bit cap");
            let at = usize::try_from(chunk).expect("index");
            let bytes = gpu_surface_bytes(&buffers);
            if let Some(size) = sizes.get_mut(at) {
                *size = bytes;
            }
            upload_bytes += u128::try_from(bytes).expect("bytes");
            uploads += 1;
            if let Some(since) = arrived.get_mut(at).and_then(Option::take) {
                latencies.push(frame - since);
            }
        }
        let spent =
            started.elapsed().as_micros() + (upload_bytes * 1_000).div_euclid(UPLOAD_BYTES_PER_MS);
        cpu.push(spent);
    }
    // A chunk still queued at the end counts at its age, or a runaway queue flatters the
    // tail (G1 section 10.5).
    for since in arrived.iter().flatten() {
        latencies.push(frames - since);
    }
    latencies.sort_unstable();
    let over_frame = cpu.iter().filter(|us| **us > FRAME_US).count();
    cpu.sort_unstable();
    Stream {
        latency_p50: percentile_u64(&latencies, 50),
        latency_p99: percentile_u64(&latencies, 99),
        latency_max: latencies
            .last()
            .copied()
            .expect("the stream queued something"),
        cpu_p99_us: percentile(&cpu, 99),
        over_frame,
        max_depth: queue.max_depth(),
        uploads,
        left: queue.len(),
    }
}

/// [`percentile`] over frame counts.
fn percentile_u64(sorted: &[u64], percent: usize) -> u64 {
    let rank = (sorted.len() * percent)
        .div_euclid(100)
        .min(sorted.len().saturating_sub(1));
    sorted.get(rank).copied().expect("a sample")
}

/// S2's crater stream through the client's relight and the drain queue, at 20 and 40
/// explosions a second (G1's gate and its stress step), on FLAT ground the size of the
/// committed map (stone to y = 20, dirt to y = 24, open sky above), not on generated
/// relief: light moves little outside a crater on flat ground, so a hillside's fan-out and
/// latency would be larger than these (S2's `view`, its correctness review). The remesh
/// latency in frames from the view that carried the craters, against G1's three, and the
/// modelled CPU per frame. Published beside the old path's figures, a whole-map bake above
/// 16 changed chunks, which S2's `view` lane replaced because it missed G1's three frames.
/// The view's own cadence, a view every six frames, comes before this latency and is the
/// watch rig's scheduling, not the remesh's. No threshold: an alarm, like the p99 above.
#[test]
#[ignore = "a performance alarm, not a gate: cargo xtask perf-alarms runs it with --ignored"]
fn crater_stream_remesh_latency() {
    let _alone = MACHINE.lock().unwrap_or_else(PoisonError::into_inner);
    for (rate, relight, frames) in [
        (20, Relight::Bounded, 3_600),
        (40, Relight::Bounded, 3_600),
        (20, Relight::WholeAbove16, 900),
        (40, Relight::WholeAbove16, 900),
    ] {
        let run = crater_stream(rate, frames, relight);
        let path = match relight {
            Relight::Bounded => "bounded relight",
            Relight::WholeAbove16 => "pre-S2 whole bake above 16 chunks",
        };
        println!(
            "::notice::crater stream {rate}/s ({path}, flat ground): remesh latency p50 {} p99 {} max {} \
             frames from the view (G1 wants p99 <= {G1_FRAMES}), modelled CPU p99 {} us a \
             frame, {} of {frames} frames over {FRAME_US} us, max queue depth {}, {} \
             uploads, {} left queued",
            run.latency_p50,
            run.latency_p99,
            run.latency_max,
            run.cpu_p99_us,
            run.over_frame,
            run.max_depth,
            run.uploads,
            run.left,
        );
        assert!(run.uploads > 0, "the stream meshed nothing");
    }
}
