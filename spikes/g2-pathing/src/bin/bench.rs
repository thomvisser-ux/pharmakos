// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later
#![deny(clippy::float_arithmetic, clippy::as_conversions)]
//! The measurement harness: plan §3 steps 4, 5, 8 and 9.
//!
//! **Wall-clock time lives here and in `accuracy.rs`, and nowhere else.** Plan
//! §5's last row: *"the lint set forbids wall-clock time in sim code, and
//! pathing is sim code — but the whole spike is a timing measurement.
//! `Instant` appears only in `src/bin/bench.rs` and `accuracy.rs`. The library
//! never reads a clock. This is the same wall the product will need for its own
//! benches, so the spike is prototyping the arrangement."* The library half of
//! that claim is checked by a test (`lib.rs`'s `mod wall`), not by a promise.
//!
//! What this binary measures:
//!
//! * **step 4** — repath latency under a destruction stream, over ≥20 000
//!   repaths; repair time per dirtied cluster; repaths per second at the peak;
//!   abstract graph size and memory; allocations and peak heap; and what
//!   happens when a walker is sealed in;
//! * **step 5** — estimate latency over 10 000 stratified queries, plus a cold
//!   set taken immediately after a repair, and the split of a query's work
//!   between the abstract search and the two endpoint-insertion sweeps (the
//!   evidence for plan §2's "does a second abstract level earn its keep");
//! * **step 8** — estimate accuracy on *dynamic* terrain, measured the way a
//!   player experiences it: the estimate a walker was given when it was issued
//!   its route, against the tick it actually arrived on after however many
//!   repaths and detours the destruction stream forced;
//! * **step 9** — `--hash-paths`, which writes one FNV-1a-64 per query path —
//!   for the pristine map **and** for the repaired graph, crater by crater — so
//!   two operating systems can be compared with `cmp`; and `--repeat`, which
//!   re-runs the whole measurement in process and checks that every run walks
//!   the same routes.
//!
//! # Cold and warm mean something here
//!
//! There is no per-query cache anywhere in the design — [`HpaScratch`] and
//! `Scratch` are generation-stamped dense arrays that `begin()` re-stamps on
//! every query — so re-issuing the *identical* query is not a "warm"
//! measurement, it is the same measurement taken twice. (The earlier version of
//! this file did exactly that, and duly reported cold p99 1.749 ms against warm
//! p99 1.754 ms.) The distinction that does exist is the abstract graph:
//!
//! * **cold** — the first repath after `rebuild_graph()`, i.e. against a CSR
//!   that was rebuilt on this very tick. This is the p99 plan §3 step 4 says
//!   matters, "since it coincides with the destruction that caused it", and it
//!   is the number G2-a is asserted on;
//! * **warm** — every later repath against that same, now unchanged, graph.
//!
//! Repaths that return "no path" are a third population, not part of either:
//! [`abstract_search`](g2_pathing::hpa::abstract_search) short-circuits an
//! unreachable pair on the connectivity oracle before any search, so those
//! samples are two array reads and would drag the p50 of a latency distribution
//! down for no reason. Plan §3 step 4 asks for both facts — the latency, and
//! whether any repath fails — as separate facts.
//!
//! # One run is not a number
//!
//! `--repeat N` runs the whole measurement N times and reports every run's p99
//! plus the median, min and max across runs. On this machine the same binary and
//! seed spread about 14% between runs, so a single six-decimal figure claims
//! more precision than exists; `repeat.repath_cold_p99_ms` is the honest
//! artefact. The cold distribution is also reported split into four run
//! quartiles, because it is not stationary: the median falls as destruction
//! shortens routes while the tail gets worse.

use std::alloc::{GlobalAlloc, Layout, System};
use std::io::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use g2_pathing::astar::Scratch;
use g2_pathing::clusters::{Clusters, IntraMode};
use g2_pathing::cost::{MOVE_COST_PER_TICK, assert_cost_headroom, ticks_for_cost};
use g2_pathing::estimate::estimate;
use g2_pathing::hash::{FNV_BASIS, Rng, fnv1a64_feed, fnv1a64_nodes};
use g2_pathing::hpa::{HpaScratch, path};
use g2_pathing::json::Obj;
use g2_pathing::pairs::{BAND_NAMES, band_counts, stratified};
use g2_pathing::repair::{
    Blast, carve, repair, repair_cluster, scheduled, surface_crater, touched_ring,
};
use g2_pathing::stats::{dist_ms_ns, dist_pct, max as smax, ms_ns, pct, rel_permille};
use g2_pathing::world::{NODES, W, World, coord_of, node_of};
use g2_pathing::{id32, ix};

// ---------------------------------------------------------------------------
// Allocation counting. Plan §3 step 4 wants "per-query scratch memory and
// allocations", and the results table wants "Memory: graph, per-query scratch,
// peak RSS". §5 warns that per-query allocation is the easiest way to turn a
// 0.5 ms query into a 5 ms one, harder on Windows than on glibc. A std-only
// counting wrapper around the system allocator gives both: a call count, and a
// live-bytes high-water mark.
//
// PLACEHOLDER: peak *RSS* — as opposed to peak live heap — needs a per-OS read
// (`/proc/self/status` `VmHWM` on Linux, `GetProcessMemoryInfo` on Windows), and
// the Windows half needs a `winapi`/`windows-sys` dependency the spike is not
// allowed (plan §3 step 0). Owner, at the S3 gate: decide whether the sim's own
// bench harness may take that dependency, or whether peak live heap plus the
// static structure sizes is the number the budget is written against.
// ---------------------------------------------------------------------------

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);
static LIVE_BYTES: AtomicU64 = AtomicU64::new(0);
static PEAK_BYTES: AtomicU64 = AtomicU64::new(0);

struct Counting;

fn note_growth(by: usize) {
    let b = u64::try_from(by).unwrap_or(0);
    ALLOC_BYTES.fetch_add(b, Ordering::Relaxed);
    let live = LIVE_BYTES.fetch_add(b, Ordering::Relaxed) + b;
    PEAK_BYTES.fetch_max(live, Ordering::Relaxed);
}

fn note_shrink(by: usize) {
    LIVE_BYTES.fetch_sub(u64::try_from(by).unwrap_or(0), Ordering::Relaxed);
}

// SAFETY: every method forwards to `System`, which is a correct global
// allocator; the counters are plain atomics and do not allocate.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        note_growth(layout.size());
        // SAFETY: forwarding an unchanged layout to the system allocator.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        note_shrink(layout.size());
        // SAFETY: forwarding a pointer this allocator produced.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        if new_size >= layout.size() {
            note_growth(new_size - layout.size());
        } else {
            note_shrink(layout.size() - new_size);
        }
        // SAFETY: forwarding a pointer this allocator produced.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn allocs() -> u64 {
    ALLOCS.load(Ordering::Relaxed)
}

fn peak_bytes() -> u64 {
    PEAK_BYTES.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Command line
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Args {
    units: usize,
    cluster: i32,
    seed: u64,
    radius: i32,
    repath_target: u64,
    max_ticks: i64,
    estimates: usize,
    pairs: usize,
    repeat: usize,
    hash_craters: u64,
    json: Option<String>,
    hash_paths: bool,
    out: Option<String>,
    assert_repath_p99_ns: Option<i64>,
    assert_estimate_p99_ns: Option<i64>,
    intra: IntraMode,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            units: 300,
            cluster: 32,
            seed: 20_260_913,
            radius: 3,
            repath_target: 20_000,
            max_ticks: 40_000,
            estimates: 10_000,
            pairs: 2_000,
            repeat: 1,
            hash_craters: 300,
            json: None,
            hash_paths: false,
            out: None,
            assert_repath_p99_ns: None,
            assert_estimate_p99_ns: None,
            intra: IntraMode::PerNodeDijkstra,
        }
    }
}

/// Parse a decimal millisecond threshold into nanoseconds, without floats.
fn parse_ms_to_ns(s: &str) -> i64 {
    let (int, frac) = match s.split_once('.') {
        Some((a, b)) => (a, b),
        None => (s, ""),
    };
    let whole: i64 = int.parse().expect("threshold: integer part");
    let mut scaled: i64 = 0;
    let mut mul = 100_000i64;
    for ch in frac.chars().take(6) {
        // `to_digit` rather than arithmetic on the code point: a stray comma or
        // minus sign from a mistyped flag used to underflow a `u32` and, with
        // overflow checks on in every profile, panic with "attempt to subtract
        // with overflow" instead of saying what was wrong. These flags come from
        // the CI file, where that is a bad way to learn about a typo.
        let Some(d) = ch.to_digit(10) else {
            panic!("threshold: not a decimal number: {s}");
        };
        scaled += i64::from(d) * mul;
        mul /= 10;
    }
    whole * 1_000_000 + scaled
}

fn parse_args() -> Args {
    let mut a = Args::default();
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let take = |i: usize, key: &str| -> String {
        argv.get(i + 1)
            .cloned()
            .unwrap_or_else(|| panic!("{key} needs a value"))
    };
    let mut i = 0;
    while i < argv.len() {
        let key = argv[i].clone();
        let mut step = 2;
        match key.as_str() {
            "--units" => a.units = take(i, &key).parse().expect("--units"),
            "--cluster" => a.cluster = take(i, &key).parse().expect("--cluster"),
            "--seed" => a.seed = take(i, &key).parse().expect("--seed"),
            "--radius" => a.radius = take(i, &key).parse().expect("--radius"),
            "--repaths" => a.repath_target = take(i, &key).parse().expect("--repaths"),
            "--max-ticks" => a.max_ticks = take(i, &key).parse().expect("--max-ticks"),
            "--estimates" => a.estimates = take(i, &key).parse().expect("--estimates"),
            "--pairs" => a.pairs = take(i, &key).parse().expect("--pairs"),
            "--repeat" => a.repeat = take(i, &key).parse().expect("--repeat"),
            "--hash-craters" => a.hash_craters = take(i, &key).parse().expect("--hash-craters"),
            "--json" => a.json = Some(take(i, &key)),
            "--out" => a.out = Some(take(i, &key)),
            "--hash-paths" => {
                a.hash_paths = true;
                step = 1;
            }
            "--intra" => {
                a.intra = match take(i, &key).as_str() {
                    "astar" => IntraMode::PairwiseAstar,
                    "dijkstra" => IntraMode::PerNodeDijkstra,
                    other => panic!("--intra must be astar or dijkstra, not {other}"),
                };
            }
            "--assert-repath-p99-ms" => {
                a.assert_repath_p99_ns = Some(parse_ms_to_ns(&take(i, &key)));
            }
            "--assert-estimate-p99-ms" => {
                a.assert_estimate_p99_ns = Some(parse_ms_to_ns(&take(i, &key)));
            }
            other => panic!("unknown flag {other}"),
        }
        i += step;
    }
    assert!(
        [16, 32, 64].contains(&a.cluster),
        "--cluster must be 16, 32 or 64"
    );
    assert!(a.repeat >= 1, "--repeat must be at least 1");
    a
}

// ---------------------------------------------------------------------------
// Walkers
// ---------------------------------------------------------------------------

/// One of the 300 units on an active route (the G3′ unit count, used here so
/// the two spikes speak the same language — plan §3 step 4).
#[derive(Clone, Debug, Default)]
struct Walker {
    pos: u32,
    goal: u32,
    route: Vec<u32>,
    idx: usize,
    carry: i32,
    /// The tick of this walker's **first advance** along the current route, not
    /// the tick the route was issued on. They differ by one for a route issued
    /// during the movement phase, which is where most routes are issued, and the
    /// difference is a systematic bias in the step-8 error on short routes.
    issued_tick: i64,
    issued_estimate: i32,
}

/// The integer movement integrator: a unit accumulates `MOVE_COST_PER_TICK`
/// cost units per tick and carries the remainder between steps, so a route of
/// total cost `c` finishes on tick `ceil(c / MOVE_COST_PER_TICK)` — the same
/// number `cost::ticks_for_cost` computes, which is the rounding rule under
/// test.
fn advance(world: &World, w: &mut Walker) -> bool {
    w.carry += MOVE_COST_PER_TICK;
    while w.idx + 1 < w.route.len() {
        let Some(c) = world.step_cost_between(w.route[w.idx], w.route[w.idx + 1]) else {
            // Terrain moved under a stale path; the caller repaths.
            return false;
        };
        if w.carry < c {
            break;
        }
        w.carry -= c;
        w.idx += 1;
        w.pos = w.route[w.idx];
    }
    w.idx + 1 >= w.route.len()
}

fn random_walkable(world: &World, rng: &mut Rng) -> u32 {
    loop {
        let id = node_of(rng.below(W), rng.below(W)).expect("in bounds");
        if world.walkable(id) {
            return id;
        }
    }
}

// ---------------------------------------------------------------------------

/// What one run of the measurement produced.
struct Run {
    obj: Obj,
    repath_p99_ns: i64,
    estimate_p99_ns: i64,
    route_digest: u64,
    repaths: u64,
    ticks: i64,
}

fn main() {
    let args = parse_args();
    if args.hash_paths {
        hash_paths_mode(&args);
        return;
    }

    let t_all = Instant::now();
    let mut runs: Vec<Run> = Vec::with_capacity(args.repeat);
    for r in 0..args.repeat {
        let run = run_once(&args, r);
        eprintln!(
            "run {}/{}: repath cold p99 {} ms, estimate p99 {} ms, {} repaths, digest {:016x}",
            r + 1,
            args.repeat,
            ms_ns(run.repath_p99_ns),
            ms_ns(run.estimate_p99_ns),
            run.repaths,
            run.route_digest
        );
        runs.push(run);
    }

    // The reported run is the median by the gate number, so a single outlier run
    // neither flatters nor damns the result. Every run's p99 is published
    // alongside it.
    let mut order: Vec<usize> = (0..runs.len()).collect();
    order.sort_by_key(|i| (runs[*i].repath_p99_ns, *i));
    let median_idx = order[(order.len() - 1) / 2];

    let mut cold_p99s: Vec<i64> = runs.iter().map(|r| r.repath_p99_ns).collect();
    let mut est_p99s: Vec<i64> = runs.iter().map(|r| r.estimate_p99_ns).collect();
    let digests: Vec<u64> = runs.iter().map(|r| r.route_digest).collect();
    let same_routes = digests.iter().all(|d| *d == digests[0]);
    let repath_p99 = runs[median_idx].repath_p99_ns;
    let estimate_p99 = runs[median_idx].estimate_p99_ns;
    let ticks = runs[median_idx].ticks;
    let repaths = runs[median_idx].repaths;

    let mut rep = Obj::new();
    rep.int("runs", i64::try_from(args.repeat).unwrap_or(-1))
        .int("median_run", i64::try_from(median_idx).unwrap_or(-1))
        .raw("repath_cold_p99_ms", &ms_list(&cold_p99s))
        .raw("estimate_p99_ms", &ms_list(&est_p99s))
        .str("route_digest", &format!("{:016x}", digests[0]))
        .bool("route_digest_identical_across_runs", same_routes);
    cold_p99s.sort_unstable();
    est_p99s.sort_unstable();
    rep.raw("repath_cold_p99_min_ms", &ms_ns(cold_p99s[0]))
        .raw("repath_cold_p99_max_ms", &ms_ns(smax(&cold_p99s)))
        .raw("estimate_p99_min_ms", &ms_ns(est_p99s[0]))
        .raw("estimate_p99_max_ms", &ms_ns(smax(&est_p99s)));

    let mut o = runs.swap_remove(median_idx).obj;
    o.raw("repeat", &rep.render());
    o.raw("all_runs_wall_ms", &ms_ns(nanos(t_all)));

    let text = o.render();
    println!("{text}");
    if let Some(p) = &args.json {
        write_file(p, text.as_bytes());
    }

    // ---- gates -------------------------------------------------------------
    let mut fails: Vec<String> = Vec::new();
    if repaths < args.repath_target {
        fails.push(format!(
            "only {repaths} repaths in {ticks} ticks; plan §3 step 4 wants >= {}",
            args.repath_target
        ));
    }
    if !same_routes {
        fails.push(format!(
            "the same seed walked different routes on different runs: digests {digests:016x?} \
             — pathing output is hashed sim state, so this alone would break G4"
        ));
    }
    if let Some(limit) = args.assert_repath_p99_ns
        && repath_p99 > limit
    {
        fails.push(format!(
            "repath cold p99 {} ms > {} ms",
            ms_ns(repath_p99),
            ms_ns(limit)
        ));
    }
    if let Some(limit) = args.assert_estimate_p99_ns
        && estimate_p99 > limit
    {
        fails.push(format!(
            "estimate p99 {} ms > {} ms",
            ms_ns(estimate_p99),
            ms_ns(limit)
        ));
    }
    if fails.is_empty() {
        eprintln!(
            "bench ok: median-of-{} repath cold p99 {} ms, estimate p99 {} ms, {repaths} repaths",
            args.repeat,
            ms_ns(repath_p99),
            ms_ns(estimate_p99)
        );
    } else {
        for f in &fails {
            eprintln!("FAIL: {f}");
        }
        std::process::exit(1);
    }
}

/// One whole measurement: map, graph, estimate latency, destruction stream.
/// Long on purpose — one run is one narrative, and splitting it would hide
/// which numbers are taken in which order.
fn run_once(args: &Args, run_index: usize) -> Run {
    let t_total = Instant::now();

    let t0 = Instant::now();
    let mut world = World::generate(args.seed);
    let gen_ns = nanos(t0);
    let walkable = world.walkable_count();
    let directed_steps = world.directed_step_count();
    let cost_bound = assert_cost_headroom(walkable);

    let mut sc = Scratch::new();
    let t0 = Instant::now();
    let mut cl = Clusters::build_with(&world, args.cluster, &mut sc, args.intra);
    let build_ns = nanos(t0);
    let mut hs = HpaScratch::new();

    // ---- reachability picture of the pristine map --------------------------
    let (dominant, components) = component_sizes(&world, &cl);
    // Graph size is reported for the pristine map *and* for the eroded one at
    // the end of the run. They differ by more than you would guess: two thousand
    // craters cut corridors, which adds transition nodes while removing the
    // intra edges that used to join them.
    let nodes0 = cl.n_nodes();
    let edges0 = cl.n_edges();
    let graph_bytes0 = cl.graph_bytes();

    // ---- step 5: estimate latency on the pristine map ----------------------
    let qs = stratified(&world, &cl, args.estimates, args.seed ^ 0xE571_4A7E);
    let mut est_ns: Vec<i64> = Vec::with_capacity(qs.len());
    let mut est_band: [Vec<i64>; 4] =
        core::array::from_fn(|_| Vec::with_capacity(qs.len() / 4 + 1));
    let mut abs_exp: Vec<i64> = Vec::with_capacity(qs.len());
    let mut low_exp: Vec<i64> = Vec::with_capacity(qs.len());
    // Warm the pools once so the first query does not pay for every Vec in the
    // scratch (that is a start-up cost, not a query cost).
    for p in qs.iter().take(64) {
        let _ = estimate(&world, &cl, &mut hs, p.a, p.b, None);
    }
    // The allocation counter is read immediately either side of the `estimate`
    // call and the deltas summed, so nothing of the harness's own can land
    // inside the window. Reading it around the whole loop instead used to
    // measure the four `est_band` vectors doubling 11 times each — 45
    // allocations, which was exactly the figure published as the *query's*
    // allocation count.
    let mut est_allocs: u64 = 0;
    for p in &qs {
        hs.low.reset_counters();
        let a0 = allocs();
        let t = Instant::now();
        let e = estimate(&world, &cl, &mut hs, p.a, p.b, None);
        let dt = nanos(t);
        est_allocs += allocs() - a0;
        std::hint::black_box(&e);
        // Plan §2 asks whether a second abstract level earns its keep. The
        // ceiling on what one could save is the abstract search's share alone —
        // the two bounded endpoint-insertion sweeps are cluster-local and a
        // coarser level cannot touch them — so this split is the evidence.
        abs_exp.push(i64::try_from(hs.abs_expansions).unwrap_or(-1));
        low_exp.push(i64::try_from(hs.low.expansions).unwrap_or(-1));
        est_ns.push(dt);
        est_band[p.band].push(dt);
    }

    // The same measurement for a full path query, which is the one the repath
    // budget cares about. Taken on the pristine map so the abstract graph is not
    // being rebuilt underneath it.
    let mut route_probe = Vec::new();
    for p in qs.iter().take(64) {
        let _ = path(&world, &cl, &mut hs, p.a, p.b, &mut route_probe);
    }
    let probe_n = qs.len().min(1_000);
    let mut path_allocs: u64 = 0;
    for p in qs.iter().take(probe_n) {
        let a0 = allocs();
        let r = path(&world, &cl, &mut hs, p.a, p.b, &mut route_probe);
        path_allocs += allocs() - a0;
        std::hint::black_box(&r);
    }

    // ---- step 4 + step 8: the destruction stream ---------------------------
    let mut digest: u64 = FNV_BASIS;
    let mut rng = Rng::new(args.seed ^ 0xD357_0000);
    let mut walkers: Vec<Walker> = Vec::with_capacity(args.units);
    for _ in 0..args.units {
        let mut w = Walker::default();
        issue_route(&world, &cl, &mut hs, &mut rng, &mut w, 0, &mut digest);
        walkers.push(w);
    }

    let mut blast = Blast::default();
    let mut touched: Vec<u32> = Vec::new();
    let mut stamp: Vec<u32> = vec![0; NODES];
    let mut stamp_epoch: u32 = 0;

    let mut cold_ns: Vec<i64> = Vec::new();
    let mut warm_ns: Vec<i64> = Vec::new();
    let mut all_ns: Vec<i64> = Vec::new();
    let mut no_path_ns: Vec<i64> = Vec::new();
    let mut repair_cluster_ns: Vec<i64> = Vec::new();
    let mut graph_rebuild_ns: Vec<i64> = Vec::new();
    let mut est_cold_ns: Vec<i64> = Vec::new();
    let mut cold_plus_repair_ns: Vec<i64> = Vec::new();
    let mut dyn_err: Vec<i64> = Vec::new();

    let mut repaths: u64 = 0;
    let mut no_path: u64 = 0;
    let mut no_path_unreachable: u64 = 0;
    let mut buried: u64 = 0;
    let mut arrivals: u64 = 0;
    let mut craters: u64 = 0;
    let mut edited_columns: u64 = 0;
    let mut affected_total: u64 = 0;
    let mut repaths_this_second: u64 = 0;
    let mut peak_repaths_per_s: u64 = 0;
    let mut repath_wall_ns: i64 = 0;
    let mut idempotence_checks: u64 = 0;
    // Hoisted out of the tick loop: a Vec allocated per repath would show up in
    // the allocation counters as the library's fault.
    let mut verify_route: Vec<u32> = Vec::new();

    let a_before_loop = allocs();
    let t_loop = Instant::now();
    let mut tick: i64 = 0;
    while tick < args.max_ticks && repaths < args.repath_target {
        // --- one explosion per tick: 20 per second at the 20 Hz tick ---------
        let victim = usize::try_from(
            u64::try_from(tick).unwrap_or(0) % u64::try_from(args.units).unwrap_or(1),
        )
        .unwrap_or(0);
        let target = {
            let w = &walkers[victim];
            let ahead = (w.idx + 6).min(w.route.len().saturating_sub(1));
            w.route.get(ahead).copied().unwrap_or(w.pos)
        };
        let (tx, tz) = coord_of(target);
        let j = rng.next_u64();
        let crater = surface_crater(
            &world,
            tx + i32::try_from(j % 5).unwrap_or(0) - 2,
            tz + i32::try_from((j >> 8) % 5).unwrap_or(0) - 2,
            j >> 16,
            args.radius,
        );
        carve(&mut world, &cl, &crater, &mut blast);
        craters += 1;
        edited_columns += u64::try_from(blast.edited.len()).unwrap_or(0);

        // --- repair: the dirty clusters and their affected neighbours --------
        // This is `repair::repair` unrolled so each cluster can be timed on its
        // own; the sequence is identical, and `repair.rs`'s equivalence test is
        // what keeps the two honest.
        let mut tick_repair_ns: i64 = 0;
        let rebuilt_this_tick = !blast.dirty.is_empty();
        if rebuilt_this_tick {
            let mut per: Vec<(u16, i64)> = Vec::new();
            let mut aff: Vec<u16> = blast.dirty.clone();
            for c in &blast.dirty {
                let t = Instant::now();
                let changed = repair_cluster(&world, &mut cl, *c);
                per.push((*c, nanos(t)));
                aff.extend(changed);
            }
            aff.sort_unstable();
            aff.dedup();
            affected_total += u64::try_from(aff.len()).unwrap_or(0);
            for c in &aff {
                let t = Instant::now();
                cl.rebuild_trans(usize::from(*c));
                cl.rebuild_intra(&world, &mut sc, usize::from(*c));
                let dt = nanos(t);
                match per.iter_mut().find(|(k, _)| k == c) {
                    Some(slot) => slot.1 += dt,
                    None => per.push((*c, dt)),
                }
            }
            let t = Instant::now();
            cl.rebuild_graph();
            let g = nanos(t);
            graph_rebuild_ns.push(g);
            tick_repair_ns = g;
            for (_, ns) in per {
                tick_repair_ns += ns;
                repair_cluster_ns.push(ns);
            }

            // Plan §3 step 5: "the cost with a cold abstract graph immediately
            // after a repair, since the editor's estimate is requested during
            // the Lull right after a Push has rearranged the terrain."
            if craters.is_multiple_of(4) {
                // A pair that is still connected *on the eroded terrain*. Timing
                // an estimate whose answer is "unreachable" would measure the
                // connectivity oracle, not the search: after two thousand
                // craters a good half of the pristine map's pairs have been cut
                // off, and the first version of this block reported a p50 of
                // 0.0003 ms for exactly that reason.
                let mut qi = usize::try_from(craters).unwrap_or(0) % qs.len().max(1);
                for _ in 0..24 {
                    let p = qs[qi];
                    if cl.connected(&world, p.a, p.b) {
                        let t = Instant::now();
                        let e = estimate(&world, &cl, &mut hs, p.a, p.b, None);
                        let dt = nanos(t);
                        if e.is_some() {
                            est_cold_ns.push(dt);
                        }
                        std::hint::black_box(&e);
                        break;
                    }
                    qi = (qi + 97) % qs.len().max(1);
                }
            }
        }

        // --- who has to repath? ---------------------------------------------
        stamp_epoch += 1;
        touched_ring(&blast.edited, &mut touched);
        for n in &touched {
            stamp[ix(*n)] = stamp_epoch;
        }
        let mut repaths_this_tick: u64 = 0;
        let tick_first_sample = all_ns.len();
        // COLD is the first repath against a CSR rebuilt on this tick; every
        // later one runs against the same, now unchanged, graph. See the module
        // docs: re-issuing the identical query is not a warm measurement.
        let mut cold_taken = !rebuilt_this_tick;
        for w in &mut walkers {
            if w.route.is_empty() {
                continue;
            }
            let hit = w.route[w.idx..]
                .iter()
                .any(|n| stamp[ix(*n)] == stamp_epoch);
            if !hit {
                continue;
            }
            if !world.walkable(w.pos) {
                // The crater took the ground out from under it. In the product
                // this is a death or a fall; here the walker is re-seeded, and
                // it is counted so the number is visible rather than hidden.
                buried += 1;
                w.pos = random_walkable(&world, &mut rng);
                issue_route(&world, &cl, &mut hs, &mut rng, w, tick, &mut digest);
                continue;
            }
            let t = Instant::now();
            let got = path(&world, &cl, &mut hs, w.pos, w.goal, &mut w.route);
            let dt = nanos(t);
            repath_wall_ns += dt;
            repaths += 1;
            repaths_this_tick += 1;
            repaths_this_second += 1;

            if got.is_none() {
                // A moat seals you in as well: a normal, cheap result, decided
                // by two array reads before any search. Its own distribution, so
                // it cannot flatter the latency of searches that searched.
                no_path_ns.push(dt);
                no_path += 1;
                if !cl.connected(&world, w.pos, w.goal) {
                    no_path_unreachable += 1;
                }
                digest = fnv1a64_feed(digest, &[u32::MAX]);
                issue_route(&world, &cl, &mut hs, &mut rng, w, tick, &mut digest);
                continue;
            }

            all_ns.push(dt);
            if cold_taken {
                warm_ns.push(dt);
            } else {
                cold_ns.push(dt);
                cold_taken = true;
            }
            digest = fnv1a64_feed(digest, &w.route);

            // The one check that a repath is a pure function of (terrain, start,
            // goal): re-issue it and demand the identical route. Unconditional —
            // `[profile.release]` sets `debug-assertions = false`, so the
            // `debug_assert_eq!` this replaces was compiled out of every run
            // that ever produced a number, which is how a scratch-reuse bug
            // survived a green CI. Sampled at 1-in-64 so it costs ~1.5% rather
            // than doubling the loop.
            if repaths.is_multiple_of(64) {
                let again = path(&world, &cl, &mut hs, w.pos, w.goal, &mut verify_route);
                assert_eq!(got, again, "repath is not idempotent at tick {tick}");
                assert_eq!(
                    w.route, verify_route,
                    "same cost, different route, at tick {tick}"
                );
                idempotence_checks += 1;
            }

            w.idx = 0;
            w.carry = 0;
        }

        // --- move everyone one tick -----------------------------------------
        for w in &mut walkers {
            if w.route.len() < 2 {
                // Issued after this tick's movement phase, so the walker's first
                // advance is on `tick + 1`.
                issue_route(&world, &cl, &mut hs, &mut rng, w, tick + 1, &mut digest);
                continue;
            }
            let arrived = advance(&world, w);
            if arrived {
                arrivals += 1;
                let actual = tick - w.issued_tick + 1;
                if w.issued_estimate > 0 && actual > 0 {
                    dyn_err.push(rel_permille(i64::from(w.issued_estimate), actual));
                }
                issue_route(&world, &cl, &mut hs, &mut rng, w, tick + 1, &mut digest);
            }
        }

        // Plan §3 step 4: "the p99 that matters is the cold one, since it
        // coincides with the destruction that caused it". The repath itself is
        // only half of that cost — the walker also had to wait for the repair.
        // One repair serves every walker that repaths on the same tick, so its
        // cost is charged as a share.
        if repaths_this_tick > 0 {
            let share = tick_repair_ns / i64::try_from(repaths_this_tick).unwrap_or(1);
            for c in &all_ns[tick_first_sample..] {
                cold_plus_repair_ns.push(c + share);
            }
        }

        tick += 1;
        if tick % 20 == 0 {
            peak_repaths_per_s = peak_repaths_per_s.max(repaths_this_second);
            repaths_this_second = 0;
        }
    }
    let loop_ns = nanos(t_loop);
    let loop_allocs = allocs() - a_before_loop;

    let walkable_after = world.walkable_count();
    let directed_steps_after = world.directed_step_count();
    let (dominant_after, components_after) = component_sizes(&world, &cl);

    // The non-stationarity: the median falls as destruction shortens routes
    // while the tail gets worse. Taken before anything sorts `cold_ns`.
    let cold_quartiles = quartiles(&cold_ns);

    // ---- report ------------------------------------------------------------
    let mut o = Obj::new();
    o.str("spike", "g2-pathing")
        .str("binary", "bench")
        .int("run_index", i64::try_from(run_index).unwrap_or(-1))
        .int("seed", i64::try_from(args.seed).unwrap_or(-1))
        .int("cluster_size", i64::from(args.cluster))
        .int("units", i64::try_from(args.units).unwrap_or(-1))
        .int("crater_radius", i64::from(args.radius))
        .int("ticks", tick)
        .int("craters", i64::try_from(craters).unwrap_or(-1))
        .int(
            "edited_columns",
            i64::try_from(edited_columns).unwrap_or(-1),
        );

    let mut map = Obj::new();
    // Largest component and directed step count lead, because they are the
    // numbers that move. On a heightmap clamped to 1..=WY-6 every column passes
    // the walkability test by construction (`world::column_walkable`), so
    // `walkable_before` is 100% of the grid and `walkable_after` barely differs:
    // what craters break is the one-voxel step rule, not walkability.
    map.int(
        "largest_component_before",
        i64::try_from(dominant).unwrap_or(-1),
    )
    .int(
        "largest_component_after",
        i64::try_from(dominant_after).unwrap_or(-1),
    )
    .int("components_before", i64::try_from(components).unwrap_or(-1))
    .int(
        "components_after",
        i64::try_from(components_after).unwrap_or(-1),
    )
    .int(
        "directed_steps_before",
        i64::try_from(directed_steps).unwrap_or(-1),
    )
    .int(
        "directed_steps_after",
        i64::try_from(directed_steps_after).unwrap_or(-1),
    )
    .int("nodes", i64::try_from(NODES).unwrap_or(-1))
    .int("walkable_before", i64::try_from(walkable).unwrap_or(-1))
    .int(
        "walkable_after",
        i64::try_from(walkable_after).unwrap_or(-1),
    )
    .bool("walkable_is_total_by_construction", walkable == NODES)
    .int("ore_cells", i64::try_from(world.ore_count()).unwrap_or(-1))
    .bool("overhangs_possible", g2_pathing::world::OVERHANGS_POSSIBLE)
    .int("max_path_cost_bound", cost_bound)
    .raw("generate_ms", &ms_ns(gen_ns));
    o.raw("map", &map.render());

    let mut graph = Obj::new();
    graph
        .int("abstract_nodes", i64::try_from(nodes0).unwrap_or(-1))
        .int(
            "abstract_edges_directed",
            i64::try_from(edges0).unwrap_or(-1),
        )
        .int(
            "abstract_nodes_after_destruction",
            i64::try_from(cl.n_nodes()).unwrap_or(-1),
        )
        .int(
            "abstract_edges_after_destruction",
            i64::try_from(cl.n_edges()).unwrap_or(-1),
        )
        .int("clusters", i64::try_from(cl.n_clusters()).unwrap_or(-1))
        .raw("build_ms", &ms_ns(build_ns));
    o.raw("graph", &graph.render());

    let mut mem = Obj::new();
    mem.int("graph_bytes", i64::try_from(graph_bytes0).unwrap_or(-1))
        .int(
            "graph_bytes_after_destruction",
            i64::try_from(cl.graph_bytes()).unwrap_or(-1),
        )
        .int(
            "node_maps_bytes",
            i64::try_from(Clusters::node_map_bytes()).unwrap_or(-1),
        )
        .int("world_bytes", i64::try_from(NODES * 4).unwrap_or(-1))
        .int(
            "low_scratch_bytes",
            i64::try_from(Scratch::bytes()).unwrap_or(-1),
        )
        .int(
            "abstract_scratch_bytes",
            i64::try_from(hs.abstract_bytes()).unwrap_or(-1),
        )
        .int(
            "structures_kib",
            i64::try_from(
                (graph_bytes0
                    + Clusters::node_map_bytes()
                    + NODES * 4
                    + Scratch::bytes()
                    + hs.abstract_bytes())
                    / 1024,
            )
            .unwrap_or(-1),
        )
        // Whole-process high-water mark of live heap bytes, from the counting
        // allocator. This is the number the "fits alongside sim + mesher" row of
        // the results table wants; the structure sizes above are what the spike
        // itself sizes, and they are the smaller figure because the harness's own
        // 300 walkers, query sets and sample vectors are not free.
        .int("peak_heap_bytes", i64::try_from(peak_bytes()).unwrap_or(-1))
        .int(
            "peak_heap_kib",
            i64::try_from(peak_bytes() / 1024).unwrap_or(-1),
        );
    o.raw("memory", &mem.render());

    let mut alloc = Obj::new();
    alloc
        .int("estimate_queries", i64::try_from(qs.len()).unwrap_or(-1))
        .int(
            "estimate_allocs_total",
            i64::try_from(est_allocs).unwrap_or(-1),
        )
        .int(
            "estimate_allocs_per_query_milli",
            i64::try_from(est_allocs * 1000 / u64::try_from(qs.len().max(1)).unwrap_or(1))
                .unwrap_or(-1),
        )
        .int("path_probe_queries", i64::try_from(probe_n).unwrap_or(-1))
        .int(
            "path_allocs_total",
            i64::try_from(path_allocs).unwrap_or(-1),
        )
        .int(
            "path_allocs_per_query_milli",
            i64::try_from(path_allocs * 1000 / u64::try_from(probe_n.max(1)).unwrap_or(1))
                .unwrap_or(-1),
        )
        // The destruction loop's allocations are repair + harness, not query:
        // `trans_cells`, `affected` and the CSR rebuild each allocate, and so
        // does the harness issuing new routes. The two per-query numbers above
        // are the ones plan §5 asks about ("target: zero in the steady state"),
        // and they are now measured with the counter read immediately either
        // side of the call.
        .int(
            "destruction_loop_allocs_total",
            i64::try_from(loop_allocs).unwrap_or(-1),
        )
        .int(
            "destruction_loop_allocs_per_crater",
            i64::try_from(loop_allocs / craters.max(1)).unwrap_or(-1),
        )
        .str(
            "intra_mode",
            match args.intra {
                IntraMode::PairwiseAstar => "pairwise-astar",
                IntraMode::PerNodeDijkstra => "per-node-dijkstra",
            },
        );
    o.raw("allocations", &alloc.render());

    let repath_p99 = {
        let mut v = cold_ns.clone();
        v.sort_unstable();
        pct(&v, 99)
    };
    let estimate_p99 = {
        let mut v = est_ns.clone();
        v.sort_unstable();
        pct(&v, 99)
    };

    let mut rp = Obj::new();
    rp.int("repaths", i64::try_from(repaths).unwrap_or(-1))
        .int("no_path_results", i64::try_from(no_path).unwrap_or(-1))
        .int(
            "no_path_genuinely_unreachable",
            i64::try_from(no_path_unreachable).unwrap_or(-1),
        )
        .int("buried_walkers", i64::try_from(buried).unwrap_or(-1))
        .int("arrivals", i64::try_from(arrivals).unwrap_or(-1))
        .int(
            "idempotence_checks",
            i64::try_from(idempotence_checks).unwrap_or(-1),
        )
        .str("route_digest", &format!("{digest:016x}"))
        .raw("cold_ms", &dist_ms_ns(&mut cold_ns))
        .raw("warm_ms", &dist_ms_ns(&mut warm_ns))
        .raw("all_searches_ms", &dist_ms_ns(&mut all_ns))
        .raw("no_path_ms", &dist_ms_ns(&mut no_path_ns))
        .raw("cold_by_run_quartile_ms", &cold_quartiles)
        .raw(
            "cold_plus_repair_share_ms",
            &dist_ms_ns(&mut cold_plus_repair_ns),
        )
        .int(
            "peak_repaths_per_game_second",
            i64::try_from(peak_repaths_per_s).unwrap_or(-1),
        )
        .int(
            "mean_repaths_per_crater_milli",
            i64::try_from(repaths * 1000 / craters.max(1)).unwrap_or(-1),
        )
        .int(
            "wall_repaths_per_second",
            if repath_wall_ns > 0 {
                i64::try_from(repaths).unwrap_or(0) * 1_000_000_000 / repath_wall_ns
            } else {
                0
            },
        )
        .raw("loop_wall_ms", &ms_ns(loop_ns));
    o.raw("repath", &rp.render());

    let mut rr = Obj::new();
    rr.raw(
        "per_dirtied_cluster_ms",
        &dist_ms_ns(&mut repair_cluster_ns),
    )
    .raw("graph_rebuild_ms", &dist_ms_ns(&mut graph_rebuild_ns))
    .int(
        "clusters_rebuilt_per_crater_milli",
        i64::try_from(affected_total * 1000 / craters.max(1)).unwrap_or(-1),
    );
    o.raw("repair", &rr.render());

    let mut es = Obj::new();
    es.raw("static_ms", &dist_ms_ns(&mut est_ns))
        .raw("cold_after_repair_ms", &dist_ms_ns(&mut est_cold_ns))
        .raw("abstract_expansions", &dist_int(&mut abs_exp))
        .raw("low_expansions", &dist_int(&mut low_exp));
    for (i, name) in BAND_NAMES.iter().enumerate() {
        es.raw(&format!("static_{name}_ms"), &dist_ms_ns(&mut est_band[i]));
    }
    let counts = band_counts(&qs);
    es.raw(
        "band_counts",
        &format!("[{},{},{},{}]", counts[0], counts[1], counts[2], counts[3]),
    );
    o.raw("estimate", &es.render());

    let mut dy = Obj::new();
    dy.raw("signed_pct", &dist_pct(&mut dyn_err.clone()))
        .raw("abs_pct", &g2_pathing::stats::dist_abs_pct(&dyn_err));
    o.raw("dynamic_terrain_error", &dy.render());

    o.raw("total_wall_ms", &ms_ns(nanos(t_total)));

    Run {
        obj: o,
        repath_p99_ns: repath_p99,
        estimate_p99_ns: estimate_p99,
        route_digest: digest,
        repaths,
        ticks: tick,
    }
}

/// Give a walker a fresh objective and a route to it. Never timed: this is the
/// harness keeping 300 units busy, not the thing being measured.
///
/// `tick` is the tick of the walker's **first advance** along the new route, not
/// necessarily the tick it was issued on — see [`Walker::issued_tick`].
fn issue_route(
    world: &World,
    cl: &Clusters,
    hs: &mut HpaScratch,
    rng: &mut Rng,
    w: &mut Walker,
    tick: i64,
    digest: &mut u64,
) {
    for _ in 0..64 {
        if !world.walkable(w.pos) {
            w.pos = random_walkable(world, rng);
        }
        let goal = random_walkable(world, rng);
        if goal == w.pos || !cl.connected(world, w.pos, goal) {
            continue;
        }
        if let Some(cost) = path(world, cl, hs, w.pos, goal, &mut w.route) {
            *digest = fnv1a64_feed(*digest, &w.route);
            w.goal = goal;
            w.idx = 0;
            w.carry = 0;
            w.issued_tick = tick;
            w.issued_estimate = estimate(world, cl, hs, w.pos, goal, None)
                .map_or(ticks_for_cost(cost), |e| e.ticks);
            return;
        }
    }
    // Could not find anywhere to go: park the walker with an empty route. The
    // outer loop will try again next tick.
    w.route.clear();
    w.issued_estimate = 0;
}

/// Plan §3 step 9: one FNV-1a-64 per query path, LF line endings only, so
/// `cmp` on two operating systems' files is the whole cross-OS check.
///
/// Two sections, because one of them was never the interesting one. The
/// **static** section hashes `--pairs` stratified queries on the pristine map.
/// The **repaired** section then replays a deterministic crater stream: after
/// each carve it repairs, writes the decomposition's own `fingerprint()` — which
/// covers the entrance rescan, the intra rebuild, the CSR and the union-find
/// relabel — and hashes three repaths on the graph that came out. That is where
/// an ordering mistake would actually hide, and it is also the only part of the
/// file that exercises a scratch reused across a graph that grew.
fn hash_paths_mode(args: &Args) {
    let t0 = Instant::now();
    let mut world = World::generate(args.seed);
    let gen_ns = nanos(t0);
    let cost_bound = assert_cost_headroom(world.walkable_count());
    let mut sc = Scratch::new();
    let t0 = Instant::now();
    let mut cl = Clusters::build_with(&world, args.cluster, &mut sc, args.intra);
    let build_ns = nanos(t0);
    let mut hs = HpaScratch::new();
    eprintln!(
        "hash-paths: map in {} ms, cluster graph in {} ms, max path cost bound {cost_bound}",
        ms_ns(gen_ns),
        ms_ns(build_ns)
    );

    let qs = stratified(&world, &cl, args.pairs, args.seed ^ 0xE571_4A7E);
    let mut route = Vec::new();
    let mut text = String::with_capacity(args.pairs * 24);
    let mut hashes: Vec<u32> = Vec::new();

    let emit = |text: &mut String, hashes: &mut Vec<u32>, h: Option<u64>| match h {
        Some(h) => {
            hashes.push(u32::try_from(h & 0xffff_ffff).unwrap_or(0));
            text.push_str(&format!("{h:016x}\n"));
        }
        None => {
            hashes.push(0);
            text.push_str("nopath\n");
        }
    };

    // ---- static: the pristine map -----------------------------------------
    text.push_str("static\n");
    for p in &qs {
        let h = path(&world, &cl, &mut hs, p.a, p.b, &mut route).map(|_| fnv1a64_nodes(&route));
        emit(&mut text, &mut hashes, h);
    }

    // ---- repaired: the same scratch, over a graph being rebuilt ------------
    text.push_str("repaired\n");
    let mut blast = Blast::default();
    let mut probe = 0usize;
    for k in 0..args.hash_craters {
        let crater = scheduled(&world, args.seed ^ 0xC7A7_E700, k, args.radius);
        carve(&mut world, &cl, &crater, &mut blast);
        if blast.dirty.is_empty() {
            continue;
        }
        let dirty = blast.dirty.clone();
        repair(&world, &mut cl, &mut sc, &dirty);
        let fp = cl.fingerprint();
        hashes.push(u32::try_from(fp & 0xffff_ffff).unwrap_or(0));
        text.push_str(&format!("fp {fp:016x}\n"));
        for _ in 0..3 {
            let p = qs[probe % qs.len().max(1)];
            probe += 1;
            let h = path(&world, &cl, &mut hs, p.a, p.b, &mut route).map(|_| fnv1a64_nodes(&route));
            emit(&mut text, &mut hashes, h);
        }
    }

    let digest = fnv1a64_nodes(&hashes);
    text.push_str(&format!("digest {digest:016x}\n"));
    print!("{text}");
    if let Some(p) = &args.out {
        write_file(p, text.as_bytes());
        eprintln!(
            "wrote {} hashed lines to {p} (digest {digest:016x})",
            hashes.len()
        );
    }
}

fn write_file(p: &str, bytes: &[u8]) {
    if let Some(dir) = std::path::Path::new(p).parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir).expect("create output directory");
    }
    // Opened in binary mode, and every line ends in a bare LF: Rust does no
    // newline translation, so the Windows and Linux files are byte-identical
    // and `cmp` is a meaningful comparison (plan §3 step 9).
    let mut f = std::fs::File::create(p).expect("create output file");
    f.write_all(bytes).expect("write output file");
}

/// Component sizes of the current terrain, from the decomposition's own
/// connectivity labels. Diagnostic only; never timed.
fn component_sizes(world: &World, cl: &Clusters) -> (usize, usize) {
    let mut labels: Vec<u32> = Vec::with_capacity(NODES);
    for i in 0..NODES {
        let id = id32(i);
        if world.walkable(id) {
            labels.push(cl.comp_label_of(id));
        }
    }
    labels.sort_unstable();
    let mut best = 0usize;
    let mut n = 0usize;
    let mut run = 0usize;
    let mut prev = u32::MAX;
    for l in labels {
        if l == prev {
            run += 1;
        } else {
            if run > best {
                best = run;
            }
            if run > 0 {
                n += 1;
            }
            run = 1;
            prev = l;
        }
    }
    if run > best {
        best = run;
    }
    if run > 0 {
        n += 1;
    }
    (best, n)
}

/// The five numbers of an integer-valued distribution (expansion counts).
fn dist_int(samples: &mut [i64]) -> String {
    samples.sort_unstable();
    format!(
        "{{\"n\":{},\"p50\":{},\"p90\":{},\"p99\":{},\"max\":{}}}",
        samples.len(),
        pct(samples, 50),
        pct(samples, 90),
        pct(samples, 99),
        smax(samples)
    )
}

/// The sample stream cut into four equal slices **in arrival order**, each
/// reported as its own distribution. One aggregate p99 hides the fact that the
/// distribution drifts across a run.
fn quartiles(samples: &[i64]) -> String {
    if samples.len() < 8 {
        return "[]".to_owned();
    }
    let q = samples.len() / 4;
    let mut parts: Vec<String> = Vec::with_capacity(4);
    for i in 0..4 {
        let s = i * q;
        let e = if i == 3 { samples.len() } else { (i + 1) * q };
        let mut v = samples[s..e].to_vec();
        parts.push(dist_ms_ns(&mut v));
    }
    format!("[{}]", parts.join(","))
}

/// A JSON array of millisecond numbers.
fn ms_list(v: &[i64]) -> String {
    let parts: Vec<String> = v.iter().map(|x| ms_ns(*x)).collect();
    format!("[{}]", parts.join(","))
}

#[inline]
fn nanos(t: Instant) -> i64 {
    i64::try_from(t.elapsed().as_nanos()).unwrap_or(i64::MAX)
}
