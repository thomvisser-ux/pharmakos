// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later
//! `meshbench` — the honest CPU number (G1 plan §3 step 1, §4 part 1).
//!
//! **No Godot.** Built with `--no-default-features`, this binary does not link
//! gdext at all, because Godot's `--headless` mode uses a dummy rendering driver
//! and will not tell the truth about GPU or upload cost. What *is* reproducible
//! headlessly — and therefore CI-gradeable — is the mesher's CPU cost, the
//! dirty-chunk fan-out, the bytes per surface, and a modelled version of the
//! budgeted pipeline.
//!
//! `std::time::Instant` is used here deliberately: AGENTS.md §4.5 permits wall
//! time in a bench target, and the spike's own `clippy.toml` documents it.
//!
//! ```text
//! meshbench [--json out.json] [--seed N] [--radius N] [--reps N]
//!           [--pipeline] [--explosions-per-s N] [--frames N] [--k N] [--b N]
//!           [--upload-rate BYTES_PER_MS]
//! ```
//!
//! # Why each chunk is meshed `--reps` times
//!
//! One `Instant` sample per meshing makes the top 1 % of the sample the Windows
//! scheduler rather than the mesher: single-shot runs of the cratered pass on
//! the authoring machine gave p99s of 532, 534, 544, 574 and 1 349 µs — a 2.5×
//! spread on identical work, so any CI budget set from one of them flaps. Each
//! chunk is therefore meshed `reps` times and the **minimum** is kept, which is
//! the estimator that converges on "what this code costs when nothing else
//! interferes". The single-shot p99 is still reported alongside as `us_raw_p99`,
//! because the difference between the two *is* the preemption noise and hiding
//! it would be the same mistake in the other direction.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use g1_remesh::budget::{Budget, UploadQueue, drain};
use g1_remesh::explode::{Explosion, Schedule, dirty_fanout, explode};
use g1_remesh::greedy::{MeshBuf, Mesher, gpu_surface_bytes};
use g1_remesh::stats::{Summary, summarise};
use g1_remesh::world::{CHUNK_COUNT, WORLD_X, WORLD_Z, World};

// ---------------------------------------------------------------------------
// Counting allocator: allocations per meshing and the process high-water mark.
// ---------------------------------------------------------------------------

struct Counting;

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        let now = LIVE.fetch_add(l.size(), Ordering::Relaxed) + l.size();
        PEAK.fetch_max(now, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        // A realloc moves live bytes by the *delta*, in whichever direction.
        // Adding the previous total to `new` (which is what this did) made every
        // growth transiently overstate the high-water mark by the size of the
        // old allocation, and made a shrink release nothing at all — and PEAK is
        // the "peak heap" number plan §3 step 1 asks for.
        if new >= l.size() {
            let grow = new - l.size();
            let live = LIVE.fetch_add(grow, Ordering::Relaxed) + grow;
            PEAK.fetch_max(live, Ordering::Relaxed);
        } else {
            LIVE.fetch_sub(l.size() - new, Ordering::Relaxed);
        }
        unsafe { System.realloc(p, l, new) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Args {
    seed: u64,
    radius: i32,
    json: bool,
    pipeline: bool,
    eps: u64,
    frames: u64,
    k: usize,
    b: usize,
    upload_rate: u64, // bytes per millisecond, 0 = not modelled
    /// Repetitions per chunk; the sample kept is the **minimum** of them.
    reps: u32,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            seed: 1,
            radius: 4,
            json: false,
            pipeline: false,
            eps: 20,
            // Plan §3 step 3: one minute at 60 fps. The in-engine run and this
            // proxy must cover the same span or they are not comparable.
            frames: 3600,
            k: 0,
            b: 0,
            upload_rate: 0,
            reps: 5,
        }
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let mut a = Args::default();
    let mut json_path: Option<String> = None;
    let mut i = 1;
    while i < argv.len() {
        let flag = argv[i].as_str();
        let mut next = || -> String {
            i += 1;
            argv.get(i).cloned().unwrap_or_default()
        };
        match flag {
            "--json" => {
                json_path = Some(next());
                a.json = true;
            }
            "--seed" => a.seed = next().parse().unwrap_or(1),
            "--radius" => a.radius = next().parse().unwrap_or(4),
            "--pipeline" => a.pipeline = true,
            "--explosions-per-s" => a.eps = next().parse().unwrap_or(20),
            "--frames" => a.frames = next().parse().unwrap_or(3600),
            "--reps" => a.reps = next().parse::<u32>().unwrap_or(5).max(1),
            "--k" => a.k = next().parse().unwrap_or(0),
            "--b" => a.b = next().parse().unwrap_or(0),
            "--upload-rate" => a.upload_rate = next().parse().unwrap_or(0),
            other => eprintln!("meshbench: ignoring unknown flag {other}"),
        }
        i += 1;
    }

    let mut json = String::from("{\n");

    println!("== g1 meshbench ==");
    println!(
        "world {WORLD_X}x64x{WORLD_Z} in {CHUNK_COUNT} chunks of 32^3, seed {}, radius {}",
        a.seed, a.radius
    );

    let t_gen = Instant::now();
    let mut world = World::generate(a.seed);
    let gen_ms = t_gen.elapsed().as_millis();
    let nonempty = world.nonempty_chunks();
    let solid = world.solid_voxels();
    println!(
        "worldgen {gen_ms} ms  non-empty chunks {}/{CHUNK_COUNT}  solid voxels {solid}",
        nonempty.len()
    );
    json.push_str(&format!(
        "  \"worldgen_ms\": {gen_ms}, \"nonempty_chunks\": {}, \"solid_voxels\": {solid},\n",
        nonempty.len()
    ));

    // -- Step 1: mesher CPU cost, flat world --------------------------------

    let mut mesher = Mesher::new();
    let mut buf = MeshBuf::default();

    // Warm the scratch vectors so the steady-state allocation count is honest.
    for &ci in nonempty.iter().take(32) {
        mesher.mesh(&world, ci as usize, &mut buf);
    }

    // Split the per-chunk cost: how much is the neighbour-aware extraction and
    // how much is the six-direction sweep?
    {
        let t = Instant::now();
        for _ in 0..4 {
            for &ci in &nonempty {
                mesher.prepare_only(&world, ci as usize);
            }
        }
        let n = (nonempty.len() * 4) as u128;
        println!(
            "  padded-copy only: {} us/chunk (the rest of the per-chunk cost is the sweep)",
            t.elapsed().as_nanos() / n / 1000
        );
    }

    let flat = measure_pass(&world, &nonempty, &mut mesher, &mut buf, 1_000, a.reps);
    report("flat", &flat);
    json.push_str(&pass_json("flat", &flat));

    // -- Step 1b: freshly cratered chunks -----------------------------------

    let sched = Schedule::new(a.seed ^ 0xA1B2_C3D4, a.radius);
    let mut cratered_chunks: Vec<u32> = Vec::new();
    for n in 0..400u64 {
        let e = sched.nth(&world, n);
        let blast = explode(&mut world, &e);
        cratered_chunks.extend_from_slice(&blast.dirty);
    }
    cratered_chunks.sort_unstable();
    cratered_chunks.dedup();
    let cratered = measure_pass(
        &world,
        &cratered_chunks,
        &mut mesher,
        &mut buf,
        1_000,
        a.reps,
    );
    report("cratered", &cratered);
    json.push_str(&pass_json("cratered", &cratered));

    println!(
        "peak heap high-water: {:.1} MiB (world + mesher + buffers)",
        PEAK.load(Ordering::Relaxed) as f64 / (1024.0 * 1024.0)
    );
    json.push_str(&format!(
        "  \"peak_heap_bytes\": {},\n",
        PEAK.load(Ordering::Relaxed)
    ));

    // -- Step 2: dirty-chunk fan-out ----------------------------------------

    println!("\n-- dirty chunks per explosion (1000 deterministic placements) --");
    json.push_str("  \"fanout\": {\n");
    for (ri, &r) in [2i32, 4, 8].iter().enumerate() {
        let mut v: Vec<u64> = Vec::with_capacity(1000);
        for n in 0..1000u64 {
            let e = fanout_position(&world, n, r);
            v.push(dirty_fanout(&e) as u64);
        }
        let s = summarise(&mut v);
        println!(
            "  r={r}: p50 {} p90 {} p99 {} max {} mean {}  -> {} chunk remeshes/s at 20 expl/s",
            s.p50,
            s.p90,
            s.p99,
            s.max,
            s.mean,
            s.mean * 20
        );
        json.push_str(&format!(
            "    \"r{r}\": {{\"p50\": {}, \"p90\": {}, \"p99\": {}, \"max\": {}, \"mean\": {}}}{}\n",
            s.p50,
            s.p90,
            s.p99,
            s.max,
            s.mean,
            if ri == 2 { "" } else { "," }
        ));
    }
    json.push_str("  },\n");

    // -- Optional: the budgeted-pipeline proxy ------------------------------

    if a.pipeline {
        let res = pipeline(&a, &mut mesher, &mut buf);
        json.push_str(&res.1);
        print!("{}", res.0);
    }

    json.push_str("  \"ok\": true\n}\n");
    if let Some(p) = json_path {
        if let Err(e) = std::fs::write(&p, &json) {
            eprintln!("meshbench: could not write {p}: {e}");
        } else {
            println!("\nwrote {p}");
        }
    }
}

// ---------------------------------------------------------------------------

struct Pass {
    /// Best-of-`reps` per chunk: the mesher's cost with the scheduler out of it.
    us: Summary,
    /// First rep only — a single `Instant` sample, the way this used to be
    /// measured. Kept so the preemption noise stays visible.
    us_raw: Summary,
    /// Geometry percentiles over **non-empty** surfaces only: a fully buried
    /// chunk produces no surface at all, and 31 % of the flat pass is buried, so
    /// percentiles over everything are percentiles over a third of nothing.
    verts: Summary,
    idx: Summary,
    bytes: Summary,
    gpu_bytes: Summary,
    allocs_per_mesh_milli: u64,
    meshings: u64,
    empty: u64,
    reps: u32,
}

impl Pass {
    /// Fraction of meshings that produced no surface, in per mille.
    fn empty_permille(&self) -> u64 {
        self.empty.saturating_mul(1000) / self.meshings.max(1)
    }
}

fn measure_pass(
    world: &World,
    chunks: &[u32],
    mesher: &mut Mesher,
    buf: &mut MeshBuf,
    want: usize,
    reps: u32,
) -> Pass {
    let mut us = Vec::with_capacity(want);
    let mut us_raw = Vec::with_capacity(want);
    let mut verts = Vec::with_capacity(want);
    let mut idx = Vec::with_capacity(want);
    let mut bytes = Vec::with_capacity(want);
    let mut gpu = Vec::with_capacity(want);
    let mut empty = 0u64;

    // Reserve before counting allocations, so vector growth is not attributed
    // to the mesher.
    let before = ALLOCS.load(Ordering::Relaxed);
    let mut n = 0usize;
    while n < want && !chunks.is_empty() {
        for &ci in chunks {
            let mut best = u64::MAX;
            let mut first = 0u64;
            for r in 0..reps {
                let t = Instant::now();
                mesher.mesh(world, ci as usize, buf);
                let el = (t.elapsed().as_nanos() as u64) / 1_000;
                if r == 0 {
                    first = el;
                }
                best = best.min(el);
            }
            us.push(best);
            us_raw.push(first);
            // The geometry is identical on every rep; record it once.
            if buf.is_empty() {
                empty += 1;
            } else {
                verts.push(buf.vertex_count() as u64);
                idx.push(buf.index_count() as u64);
                bytes.push(buf.bytes() as u64);
                gpu.push(gpu_surface_bytes(buf.vertex_count(), buf.index_count()) as u64);
            }
            n += 1;
            if n >= want {
                break;
            }
        }
    }
    let allocs = ALLOCS.load(Ordering::Relaxed) - before;

    Pass {
        us: summarise(&mut us),
        us_raw: summarise(&mut us_raw),
        verts: summarise(&mut verts),
        idx: summarise(&mut idx),
        bytes: summarise(&mut bytes),
        gpu_bytes: summarise(&mut gpu),
        allocs_per_mesh_milli: allocs.saturating_mul(1000)
            / (n.max(1) as u64)
            / u64::from(reps.max(1)),
        meshings: n as u64,
        empty,
        reps,
    }
}

fn report(name: &str, p: &Pass) {
    println!(
        "\n-- mesh cost, {name} ({} meshings x {} reps, best of reps) --",
        p.meshings, p.reps
    );
    println!(
        "  time us        p50 {:>6}  p90 {:>6}  p99 {:>6}  max {:>7}  mean {:>6}",
        p.us.p50, p.us.p90, p.us.p99, p.us.max, p.us.mean
    );
    println!(
        "  time us RAW    p50 {:>6}  p90 {:>6}  p99 {:>6}  max {:>7}   (single shot; the gap to \
         the row above is scheduler preemption, not the mesher)",
        p.us_raw.p50, p.us_raw.p90, p.us_raw.p99, p.us_raw.max
    );
    println!(
        "  --- geometry rows below are over NON-EMPTY surfaces only: {}/{} meshings ({}.{}%) \
         produced no surface at all ---",
        p.empty,
        p.meshings,
        p.empty_permille() / 10,
        p.empty_permille() % 10
    );
    println!(
        "  vertices       p50 {:>6}  p90 {:>6}  p99 {:>6}  max {:>7}",
        p.verts.p50, p.verts.p90, p.verts.p99, p.verts.max
    );
    println!(
        "  indices        p50 {:>6}  p90 {:>6}  p99 {:>6}  max {:>7}",
        p.idx.p50, p.idx.p90, p.idx.p99, p.idx.max
    );
    println!(
        "  bytes (FFI)    p50 {:>6}  p99 {:>6}  max {:>7}   = {:.1} / {:.1} KiB",
        p.bytes.p50,
        p.bytes.p99,
        p.bytes.max,
        p.bytes.p50 as f64 / 1024.0,
        p.bytes.max as f64 / 1024.0
    );
    println!(
        "  bytes (GPU)    p50 {:>6}  p99 {:>6}  max {:>7}   = {:.1} / {:.1} KiB",
        p.gpu_bytes.p50,
        p.gpu_bytes.p99,
        p.gpu_bytes.max,
        p.gpu_bytes.p50 as f64 / 1024.0,
        p.gpu_bytes.max as f64 / 1024.0
    );
    println!(
        "  allocations per meshing: {}.{:03}   empty surfaces: {}/{}",
        p.allocs_per_mesh_milli / 1000,
        p.allocs_per_mesh_milli % 1000,
        p.empty,
        p.meshings
    );
    println!(
        "  non-empty surfaces sampled for the geometry rows: {}",
        p.verts.n
    );
    let idx_width = if p.verts.max > 65_535 { 32 } else { 16 };
    println!(
        "  max vertices {} -> a {idx_width}-bit index buffer is sufficient for this surface",
        p.verts.max
    );
}

fn pass_json(name: &str, p: &Pass) -> String {
    format!(
        "  \"{name}\": {{\"us_p50\": {}, \"us_p90\": {}, \"us_p99\": {}, \"us_max\": {}, \
         \"us_raw_p50\": {}, \"us_raw_p99\": {}, \"us_raw_max\": {}, \"reps\": {}, \
         \"verts_p50\": {}, \"verts_max\": {}, \"idx_p50\": {}, \"idx_max\": {}, \
         \"bytes_p50\": {}, \"bytes_p99\": {}, \"bytes_max\": {}, \
         \"gpu_bytes_p50\": {}, \"gpu_bytes_max\": {}, \
         \"allocs_per_mesh_milli\": {}, \"meshings\": {}, \"empty\": {}, \
         \"nonempty_sampled\": {}}},\n",
        p.us.p50,
        p.us.p90,
        p.us.p99,
        p.us.max,
        p.us_raw.p50,
        p.us_raw.p99,
        p.us_raw.max,
        p.reps,
        p.verts.p50,
        p.verts.max,
        p.idx.p50,
        p.idx.max,
        p.bytes.p50,
        p.bytes.p99,
        p.bytes.max,
        p.gpu_bytes.p50,
        p.gpu_bytes.max,
        p.allocs_per_mesh_milli,
        p.meshings,
        p.empty,
        p.verts.n
    )
}

/// 1 000 deterministic placements per radius, deliberately including chunk
/// corners and edges — that is where the fan-out is worst.
fn fanout_position(world: &World, n: u64, r: i32) -> Explosion {
    let mut s = n.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    s ^= s >> 30;
    s = s.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    s ^= s >> 27;
    match n % 4 {
        // Every fourth placement is snapped to a chunk corner, every fourth to
        // a chunk edge; the rest are free.
        0 => {
            let cx = 1 + i32::try_from(s % 10).unwrap_or(0);
            let cz = 1 + i32::try_from((s >> 8) % 10).unwrap_or(0);
            Explosion {
                x: cx * 32,
                y: 32,
                z: cz * 32,
                radius: r,
            }
        }
        1 => {
            let cx = 1 + i32::try_from(s % 10).unwrap_or(0);
            let z = 40 + i32::try_from((s >> 8) % 300).unwrap_or(0);
            let y = i32::from(world.height_at(cx * 32, z)).clamp(1, 62);
            Explosion {
                x: cx * 32,
                y,
                z,
                radius: r,
            }
        }
        _ => {
            let x = 8 + i32::try_from(s % 368).unwrap_or(0);
            let z = 8 + i32::try_from((s >> 20) % 368).unwrap_or(0);
            let y = (i32::from(world.height_at(x, z)) - 1).clamp(1, 62);
            Explosion { x, y, z, radius: r }
        }
    }
}

/// The CI-stable proxy for G1-a (plan §4 part 1): run the *real* dirty set and
/// the *real* drain policy over virtual 16.67 ms frames, with upload cost
/// modelled as `bytes / upload_rate` when a rate measured in step 3/4 is
/// supplied. Honest about being a proxy: it does not render anything.
fn pipeline(a: &Args, mesher: &mut Mesher, buf: &mut MeshBuf) -> (String, String) {
    // A *fresh* world, not the one step 1b already cratered: the proxy has to
    // drive exactly the schedule the in-engine run drives, or the two upload
    // counts are not comparable and the proxy cannot detect a regression in the
    // thing it is proxying for.
    let world = &mut World::generate(a.seed);
    let sched = Schedule::new(a.seed ^ 0xA1B2_C3D4, a.radius);
    let budget = Budget {
        k: if a.k == 0 { usize::MAX } else { a.k },
        b: if a.b == 0 { usize::MAX } else { a.b },
    };
    let mut q = UploadQueue::new();
    // The fixed vista camera pose, in voxels, matching main.tscn.
    let cam = (192, 96, 24);

    let mut lat: Vec<u64> = Vec::new();
    let mut cpu_us: Vec<u64> = Vec::new();
    let mut issued = 0u64;
    let mut uploaded = 0u64;
    let mut bytes_total = 0u64;

    for frame in 1..=a.frames {
        let due = frame.saturating_mul(a.eps) / 60;
        while issued < due {
            let e = sched.nth(world, issued);
            let blast = explode(world, &e);
            for c in blast.dirty {
                q.push(c, frame);
            }
            issued += 1;
        }
        let mut frame_ns = 0u64;
        let mut frame_bytes = 0u64;
        {
            let w: &World = world;
            let r = drain(&mut q, cam, budget, frame, |ci, _| {
                let t = Instant::now();
                mesher.mesh(w, ci as usize, buf);
                frame_ns += t.elapsed().as_nanos() as u64;
                let b = buf.bytes();
                frame_bytes += b as u64;
                b
            });
            uploaded += r.uploaded as u64;
            bytes_total += r.bytes as u64;
            lat.extend(r.latencies.iter().map(|&l| u64::from(l)));
        }
        // `--upload-rate 0` means "not modelled": the proxy then reports mesher
        // CPU only and says so, rather than inventing an upload cost.
        let modelled_upload_us = frame_bytes
            .checked_mul(1_000)
            .and_then(|b| b.checked_div(a.upload_rate))
            .unwrap_or(0);
        cpu_us.push(frame_ns / 1_000 + modelled_upload_us);
    }

    let ls = summarise(&mut lat);
    let cs = summarise(&mut cpu_us);
    let over = cpu_us.iter().filter(|&&v| v > 16_670).count();

    let text = format!(
        "\n-- budgeted pipeline proxy (headless; NOT a frame-time measurement) --\n\
         frames {} at {} expl/s, K={} B={}\n\
         explosions {issued}  chunk uploads {uploaded}  bytes {bytes_total}\n  \
         remesh latency frames: p50 {} p90 {} p99 {} max {}   (G1-a wants p99 <= 3)\n  \
         modelled CPU per frame us: p50 {} p99 {} max {}   frames over 16.67 ms: {over}/{}\n  \
         max queue depth {}  coalesced {}\n",
        a.frames,
        a.eps,
        a.k,
        a.b,
        ls.p50,
        ls.p90,
        ls.p99,
        ls.max,
        cs.p50,
        cs.p99,
        cs.max,
        a.frames,
        q.max_depth,
        q.coalesced,
    );
    let json = format!(
        "  \"pipeline\": {{\"frames\": {}, \"eps\": {}, \"k\": {}, \"b\": {}, \
         \"lat_p50\": {}, \"lat_p90\": {}, \"lat_p99\": {}, \"lat_max\": {}, \
         \"cpu_us_p50\": {}, \"cpu_us_p99\": {}, \"cpu_us_max\": {}, \"over_budget\": {}, \
         \"max_queue_depth\": {}, \"uploads\": {}}},\n",
        a.frames,
        a.eps,
        a.k,
        a.b,
        ls.p50,
        ls.p90,
        ls.p99,
        ls.max,
        cs.p50,
        cs.p99,
        cs.max,
        over,
        q.max_depth,
        uploaded
    );
    (text, json)
}
