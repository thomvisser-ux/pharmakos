// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `tickbench` — plan §3 steps 2, 3 and 6: warm-up and steady state, the gate
//! configuration, and the segment-length drift check.
//!
//! **Wall-clock time lives here and in `costmodel.rs`, and nowhere else.** Plan
//! §5's row on it: *"Forbidden by the lint set, and this spike is entirely about
//! time. `Instant` lives only in `src/bin/`. The tick function takes no clock
//! and returns no duration; the harness times the call."* The library half of
//! that claim is checked by `tests/wall.rs`, not by this comment.
//!
//! What it measures, and how the numbers are taken:
//!
//! * **per-phase p50 / p99, all eleven phases** — each phase is timed by its
//!   own pair of clock reads, `t_end` then `t_start`, with the harness's own
//!   bookkeeping (the sample push, the `Work` merge, the checksum xor) done
//!   *between* them. The first version of this file took one stamp per phase
//!   boundary, which put that bookkeeping inside the next phase's span and
//!   loaded it disproportionately onto the smallest phases.
//! * **total tick p50 / p90 / p99 / max** — the sum of the eleven phase spans,
//!   so the total is exactly what the phase table accounts for and carries no
//!   unattributed harness work. The clock reads themselves are still inside it
//!   and are reported as `timing_overhead_ns_per_tick` rather than subtracted;
//!   subtracting a number you did not measure is how a benchmark starts
//!   flattering itself.
//! * **mean tick, ticks/s, speed multiple** — the mean over the measured ticks,
//!   and ticks/s divided by 20. G3'-b needs >= 4x, i.e. a mean <= 12.5 ms, and
//!   is the tighter of the two constraints for any workload with a spread.
//! * **the `pathing` phase's charged/real split** — that phase is ~98% of the
//!   tick and is a *substitute*, not a measurement: most of it is a calibrated
//!   busy loop reproducing G2's milliseconds. The JSON marks it
//!   `"substituted": true` and splits it into `charged_ms` (exactly
//!   `iters x ps_per_iter`) and `real_ms` (the integer path-follow over the unit
//!   table), so no reader can take the tick total for measured sim work.
//! * **tick p50 / mean / p99 per game-minute** — the drift check. The p99 series
//!   alone cannot do this job at the gate configuration: the repath burst makes
//!   it bimodal, so a minute's p99 reports how many bursts that minute drew
//!   rather than how expensive the minute was. The p50 and mean series are the
//!   ones that can show a leak, and the drift figure is computed from the mean.
//! * **allocations per tick and peak live heap**, from a std-only counting
//!   global allocator, plus peak RSS from the platform API. Plan §5's allocator
//!   row needs the first; its hash row needs the hash phase's share of it.
//! * **the per-tick hash stream**, one lowercase hex per line with LF endings
//!   only, so `ci/spike-g3.yml` can `cmp` three operating systems' streams.
//!   That makes every run of this binary a determinism run as well, at no cost.
//!
//! One run is not a number: `--repeat N` runs the whole measurement N times
//! from a fresh world and reports the **median of the N p99s** (plan §3 step 1),
//! every run's p99, and whether all N produced byte-identical hash streams.

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::io::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use g3_tick_budget::hash::{HashMode, digest, hex};
use g3_tick_budget::tick::{Config, CostConstants, PHASE_COUNT, Phase, Work, World};
use g3_tick_budget::{MINUTE_TICKS, SEGMENT_TICKS, TICK_HZ, synthetic_work};

// ---------------------------------------------------------------------------
// Allocation counting (plan §3 step 3: "allocations per tick and peak RSS";
// plan §5: "hash cost hidden by the allocator"). std only, on purpose — the
// spike pins one dependency and adding a second for one number would weaken
// the point of the pinning.
// ---------------------------------------------------------------------------

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static LIVE_BYTES: AtomicU64 = AtomicU64::new(0);
static PEAK_BYTES: AtomicU64 = AtomicU64::new(0);

struct Counting;

fn note_growth(by: usize) {
    let b = u64::try_from(by).unwrap_or(0);
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

// ---------------------------------------------------------------------------
// Peak RSS. Copied from the G4 spike's `src/bin/run.rs`; dependency-free.
// ---------------------------------------------------------------------------

mod peak_rss {
    #[cfg(windows)]
    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    #[cfg(windows)]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut core::ffi::c_void;
        fn K32GetProcessMemoryInfo(
            process: *mut core::ffi::c_void,
            counters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
    }

    #[cfg(windows)]
    #[must_use]
    pub fn bytes() -> u64 {
        let mut c = ProcessMemoryCounters {
            cb: 0,
            page_fault_count: 0,
            peak_working_set_size: 0,
            working_set_size: 0,
            quota_peak_paged_pool_usage: 0,
            quota_paged_pool_usage: 0,
            quota_peak_non_paged_pool_usage: 0,
            quota_non_paged_pool_usage: 0,
            pagefile_usage: 0,
            peak_pagefile_usage: 0,
        };
        let size = u32::try_from(core::mem::size_of::<ProcessMemoryCounters>())
            .expect("PROCESS_MEMORY_COUNTERS size");
        c.cb = size;
        // SAFETY: `c` is a live, correctly sized PROCESS_MEMORY_COUNTERS and
        // the pseudo-handle from GetCurrentProcess is always valid.
        let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &raw mut c, size) };
        if ok == 0 {
            0
        } else {
            u64::try_from(c.peak_working_set_size).unwrap_or(0)
        }
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn bytes() -> u64 {
        let Ok(s) = std::fs::read_to_string("/proc/self/status") else {
            return 0;
        };
        for line in s.lines() {
            if let Some(rest) = line.strip_prefix("VmHWM:") {
                let kb: u64 = rest
                    .trim()
                    .trim_end_matches(" kB")
                    .trim()
                    .parse()
                    .unwrap_or(0);
                return kb.saturating_mul(1024);
            }
        }
        0
    }

    #[cfg(target_os = "macos")]
    #[must_use]
    pub fn bytes() -> u64 {
        #[repr(C)]
        struct TimeVal {
            sec: i64,
            usec: i32,
            _pad: i32,
        }
        #[repr(C)]
        struct RUsage {
            ru_utime: TimeVal,
            ru_stime: TimeVal,
            ru_maxrss: i64,
            _rest: [i64; 15],
        }
        unsafe extern "C" {
            fn getrusage(who: i32, usage: *mut RUsage) -> i32;
        }
        let mut u = RUsage {
            ru_utime: TimeVal {
                sec: 0,
                usec: 0,
                _pad: 0,
            },
            ru_stime: TimeVal {
                sec: 0,
                usec: 0,
                _pad: 0,
            },
            ru_maxrss: 0,
            _rest: [0; 15],
        };
        // SAFETY: `u` is live and no smaller than the kernel's struct rusage.
        let ok = unsafe { getrusage(0, &raw mut u) };
        if ok == 0 {
            u64::try_from(u.ru_maxrss).unwrap_or(0)
        } else {
            0
        }
    }

    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    #[must_use]
    pub fn bytes() -> u64 {
        0
    }
}

// ---------------------------------------------------------------------------
// Core affinity. Plan §3 step 1: "Pin the process to a core."
//
// Dependency-free, so it is two direct system calls. macOS has no thread
// affinity API a process may use, so the honest answer there is "not pinned",
// and the JSON says so rather than claiming a pin that did not happen.
//
// From PowerShell, without this flag, the equivalent is:
//
//     $p = Start-Process -PassThru .\tickbench.exe -ArgumentList '--units','300'
//     $p.ProcessorAffinity = 0x10      # core 4
//
// and on Linux `taskset -c 4 ./tickbench ...`.
// ---------------------------------------------------------------------------

mod affinity {
    #[cfg(windows)]
    unsafe extern "system" {
        fn GetCurrentThread() -> *mut core::ffi::c_void;
        fn SetThreadAffinityMask(thread: *mut core::ffi::c_void, mask: usize) -> usize;
    }

    /// Pin this thread to `core`. Returns what happened, for the JSON.
    #[cfg(windows)]
    pub fn pin(core: u32) -> &'static str {
        let mask: usize = 1usize << (core % 64);
        // SAFETY: the pseudo-handle from GetCurrentThread is always valid and
        // the mask is a single set bit inside the machine word.
        let prev = unsafe { SetThreadAffinityMask(GetCurrentThread(), mask) };
        if prev == 0 { "failed" } else { "pinned" }
    }

    #[cfg(target_os = "linux")]
    unsafe extern "C" {
        fn sched_setaffinity(pid: i32, cpusetsize: usize, mask: *const u64) -> i32;
    }

    #[cfg(target_os = "linux")]
    pub fn pin(core: u32) -> &'static str {
        let mut set = [0u64; 16];
        let bit = usize::try_from(core).unwrap_or(0);
        set[bit / 64] = 1u64 << (bit % 64);
        // SAFETY: `set` is 128 bytes, the size the call is told, and pid 0 is
        // the calling thread.
        let rc = unsafe { sched_setaffinity(0, core::mem::size_of_val(&set), set.as_ptr()) };
        if rc == 0 { "pinned" } else { "failed" }
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    pub fn pin(_core: u32) -> &'static str {
        "unsupported-on-this-platform"
    }
}

// ---------------------------------------------------------------------------
// Integer statistics. No floating point in this file at all: every number the
// JSON carries is the integer nanosecond count that was measured, rendered as
// milliseconds with six decimals.
// ---------------------------------------------------------------------------

fn pct(sorted: &[i64], p: i64) -> i64 {
    if sorted.is_empty() {
        return 0;
    }
    let n = i64::try_from(sorted.len()).expect("sample count fits i64");
    let idx = ((p * (n - 1)) + 50) / 100;
    let i = usize::try_from(idx.clamp(0, n - 1)).expect("clamped into range");
    sorted[i]
}

fn ms_ns(ns: i64) -> String {
    let neg = ns < 0;
    let a = ns.abs();
    format!(
        "{}{}.{:06}",
        if neg { "-" } else { "" },
        a / 1_000_000,
        a % 1_000_000
    )
}

fn mean(samples: &[i64]) -> i64 {
    if samples.is_empty() {
        return 0;
    }
    let sum: i128 = samples.iter().map(|v| i128::from(*v)).sum();
    i64::try_from(sum / i128::try_from(samples.len()).expect("len")).expect("mean fits i64")
}

fn dist(samples: &mut [i64]) -> String {
    samples.sort_unstable();
    format!(
        "{{\"n\":{},\"p50\":{},\"p90\":{},\"p99\":{},\"max\":{},\"mean\":{}}}",
        samples.len(),
        ms_ns(pct(samples, 50)),
        ms_ns(pct(samples, 90)),
        ms_ns(pct(samples, 99)),
        ms_ns(samples.last().copied().unwrap_or(0)),
        ms_ns(mean(samples))
    )
}

/// Parse a decimal-milliseconds argument into nanoseconds, without floating
/// point: `"12.5"` -> 12_500_000.
fn parse_ms_ns(s: &str) -> i64 {
    let (whole, frac) = match s.split_once('.') {
        Some((a, b)) => (a, b),
        None => (s, ""),
    };
    let w: i64 = whole.parse().expect("--assert-* takes a number of ms");
    let mut f: i64 = 0;
    let mut scale: i64 = 1_000_000;
    for ch in frac.chars() {
        let d = i64::from(ch.to_digit(10).expect("--assert-* takes a decimal number"));
        scale /= 10;
        f += d * scale;
        if scale == 1 {
            break;
        }
    }
    w * 1_000_000 + f
}

// ---------------------------------------------------------------------------
// Command line
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Args {
    seats: usize,
    units: usize,
    beacons: usize,
    ticks: u32,
    warmup: u32,
    edits_per_s: u32,
    repath_cap: u32,
    burst_one_in: u32,
    hash_mode: HashMode,
    repeat: usize,
    pin_core: Option<u32>,
    json: Option<String>,
    hashes: Option<String>,
    assert_p99_ns: Option<i64>,
    assert_mean_ns: Option<i64>,
    seed: u64,
}

impl Default for Args {
    fn default() -> Args {
        Args {
            seats: 4,
            units: 300,
            beacons: 40,
            ticks: SEGMENT_TICKS,
            warmup: 2_000,
            edits_per_s: 20,
            repath_cap: u32::MAX,
            burst_one_in: g3_tick_budget::tick::DEFAULT_BURST_ONE_IN,
            hash_mode: HashMode::Full,
            repeat: 1,
            pin_core: None,
            json: None,
            hashes: None,
            assert_p99_ns: None,
            assert_mean_ns: None,
            seed: g3_tick_budget::DEFAULT_MATCH_SEED,
        }
    }
}

fn value(args: &[String], name: &str) -> Option<String> {
    let mut i = 0usize;
    while i + 1 < args.len() {
        if args[i] == name {
            return Some(args[i + 1].clone());
        }
        i += 1;
    }
    None
}

fn parse_args() -> Args {
    let a: Vec<String> = std::env::args().collect();
    let mut out = Args::default();
    if let Some(v) = value(&a, "--seats") {
        out.seats = v.parse().expect("--seats");
    }
    if let Some(v) = value(&a, "--units") {
        out.units = v.parse().expect("--units");
    }
    if let Some(v) = value(&a, "--beacons") {
        out.beacons = v.parse().expect("--beacons");
    }
    if let Some(v) = value(&a, "--ticks") {
        out.ticks = v.parse().expect("--ticks");
    }
    if let Some(v) = value(&a, "--warmup") {
        out.warmup = v.parse().expect("--warmup");
    }
    if let Some(v) = value(&a, "--edits-per-s") {
        out.edits_per_s = v.parse().expect("--edits-per-s");
    }
    if let Some(v) = value(&a, "--repath-cap") {
        out.repath_cap = if v == "unbounded" {
            u32::MAX
        } else {
            v.parse().expect("--repath-cap")
        };
    }
    if let Some(v) = value(&a, "--burst-one-in") {
        // "off" disables the burst entirely, which is the honest zero point of
        // the sweep: see `Config::burst_one_in` for why this is a PLACEHOLDER.
        out.burst_one_in = if v == "off" {
            0
        } else {
            v.parse().expect("--burst-one-in")
        };
        assert!(
            out.burst_one_in <= 1_000_000,
            "--burst-one-in must be at most 1000000"
        );
    }
    if let Some(v) = value(&a, "--hash") {
        out.hash_mode = match v.as_str() {
            "full" => HashMode::Full,
            "incremental" => HashMode::Incremental,
            other => panic!("--hash takes full|incremental, not {other}"),
        };
    }
    if let Some(v) = value(&a, "--repeat") {
        out.repeat = v.parse().expect("--repeat");
    }
    if let Some(v) = value(&a, "--seed") {
        out.seed = v.parse().expect("--seed");
    }
    if a.iter().any(|x| x == "--pin-core") {
        out.pin_core = Some(
            value(&a, "--pin-core")
                .and_then(|v| v.parse().ok())
                .unwrap_or(2),
        );
    }
    out.json = value(&a, "--json");
    out.hashes = value(&a, "--hashes");
    out.assert_p99_ns = value(&a, "--assert-p99-ms").map(|v| parse_ms_ns(&v));
    out.assert_mean_ns = value(&a, "--assert-mean-ms").map(|v| parse_ms_ns(&v));
    assert!(out.repeat >= 1, "--repeat must be at least 1");
    assert!(out.ticks >= 1, "--ticks must be at least 1");
    out
}

// ---------------------------------------------------------------------------
// Calibration of the synthetic pathing workload
// ---------------------------------------------------------------------------

/// Picoseconds per iteration of [`synthetic_work`], median of five ramps.
///
/// This is the one place the clock reaches into the cost model, and it is
/// exactly why the model is state-free: the number differs per machine, per OS
/// and per run, and if it reached hashed state the cross-OS hash comparison
/// would fail for a reason that has nothing to do with the sim.
fn calibrate_ps_per_iter() -> u64 {
    let mut samples: Vec<u64> = Vec::with_capacity(5);
    for run in 0..5u64 {
        let mut iters: u64 = 1 << 16;
        loop {
            let t = Instant::now();
            let v = synthetic_work(iters, 0x1234_5678 ^ run);
            let el = t.elapsed();
            black_box(v);
            let ns = el.as_nanos();
            if ns >= 20_000_000 || iters >= 1 << 32 {
                let ps = ns * 1_000 / u128::from(iters);
                samples.push(u64::try_from(ps).unwrap_or(1));
                break;
            }
            iters *= 2;
        }
    }
    samples.sort_unstable();
    samples[samples.len() / 2].max(1)
}

/// Nanoseconds for the intra-tick clock reads that per-phase timing costs.
///
/// Two reads per phase — `t_start` before the call and `t_end` after it — so
/// twenty-two a tick. Everything else the harness does per phase (the sample
/// push, the `Work` merge, the checksum xor) now happens *outside* the timed
/// spans, between one phase's `t_end` and the next phase's `t_start`, so it is
/// not in the tick total and does not need to be in this figure.
///
/// Measured rather than assumed, and reported rather than subtracted:
/// subtracting a number you did not measure is how a benchmark starts
/// flattering itself, and at well under a microsecond against a
/// millisecond-scale tick the honest thing is to say how big it is and leave it
/// in.
fn timing_overhead_ns_per_tick() -> i64 {
    let n = 200_000u32;
    let t = Instant::now();
    for _ in 0..n {
        black_box(Instant::now());
    }
    let ns = i64::try_from(t.elapsed().as_nanos()).unwrap_or(i64::MAX);
    let reads = i64::try_from(PHASE_COUNT * 2).expect("phase count fits i64");
    ns * reads / i64::from(n)
}

// ---------------------------------------------------------------------------
// One measured run
// ---------------------------------------------------------------------------

struct RunResult {
    total: Vec<i64>,
    phases: Vec<Vec<i64>>,
    /// Total nanoseconds each phase spent over the **whole** run. `share` is
    /// computed from this rather than from the sampled mean: the `decision`
    /// phase is sampled only on the ticks it runs on (one in five), so its
    /// sampled mean is five times its actual contribution and dividing that by
    /// the tick mean would inflate its share by the same factor.
    phase_sum_ns: [i64; PHASE_COUNT],
    per_minute_p50: Vec<i64>,
    per_minute_mean: Vec<i64>,
    per_minute_p99: Vec<i64>,
    hashes: Vec<u64>,
    work: Work,
    allocs_milli_per_tick: u64,
    peak_heap: u64,
    checksum: u64,
    voxel_full_rehash_ns: i64,
    hash_bytes_per_tick: u64,
    /// Iterations of the synthetic workload the `pathing` phase charged over the
    /// run. Times the calibration, this is exactly the substituted half of that
    /// phase.
    path_iters_total: u64,
    /// `kill_credit` samples from the ticks that actually settled a death. The
    /// phase's headline distribution is dominated by the empty-loop case — 37
    /// deaths in 9_600 ticks at the gate configuration — so the settlement cost
    /// is published separately rather than averaged into invisibility.
    kill_credit_death_ticks: Vec<i64>,
}

fn measure(args: &Args, cost: CostConstants) -> RunResult {
    let cfg = Config {
        match_seed: args.seed,
        seats: args.seats,
        units: args.units,
        beacons: args.beacons,
        edits_per_s: args.edits_per_s,
        repath_cap: args.repath_cap,
        burst_one_in: args.burst_one_in,
        hash_mode: args.hash_mode,
        cost,
    };
    let mut world = World::new(cfg);

    // Warm-up. Plan §3 step 2: "Run 2,000 ticks before recording anything" —
    // allocator behaviour and the chunk store both change character over
    // minutes, and a 100-tick burst measures neither.
    for _ in 0..args.warmup {
        let w = world.step();
        black_box(w);
    }

    let n = usize::try_from(args.ticks).expect("tick count");
    let mut total: Vec<i64> = Vec::with_capacity(n);
    let mut phases: Vec<Vec<i64>> = (0..PHASE_COUNT).map(|_| Vec::with_capacity(n)).collect();
    let mut hashes: Vec<u64> = Vec::with_capacity(n);
    let mut agg = Work::default();
    let mut checksum: u64 = 0;

    let mut phase_sum_ns = [0i64; PHASE_COUNT];
    let mut path_iters_total: u64 = 0;
    let mut kill_credit_death_ticks: Vec<i64> = Vec::new();

    let allocs_before = ALLOCS.load(Ordering::Relaxed);
    for _ in 0..args.ticks {
        let mut tick_ns: i64 = 0;
        for p in Phase::ALL {
            // One stamp before the call and one after it. Everything below the
            // `t_end` read is harness bookkeeping and is deliberately outside
            // every timed span.
            let t_start = Instant::now();
            let w = black_box(world.phase(p));
            let t_end = Instant::now();
            let ns = i64::try_from(t_end.duration_since(t_start).as_nanos()).unwrap_or(i64::MAX);
            tick_ns += ns;
            phase_sum_ns[p.index()] += ns;
            // The decision phase does nothing on four ticks out of five.
            // Folding those zeroes in would put its p50 at zero and describe
            // nothing, so only the ticks it ran on are sampled.
            if p != Phase::Decision || w.rules > 0 {
                phases[p.index()].push(ns);
            }
            if p == Phase::Pathing {
                path_iters_total += w.path_iters;
            }
            if p == Phase::KillCredit && w.deaths > 0 {
                kill_credit_death_ticks.push(ns);
            }
            agg.merge(w);
            checksum ^= w.checksum;
        }
        total.push(tick_ns);
        hashes.push(world.last_hash);
    }
    let allocs_after = ALLOCS.load(Ordering::Relaxed);

    // Plan §3 step 6: the per-game-minute series. p50 and mean are reported
    // alongside p99 because the p99 series is bimodal at the gate configuration
    // — the repath burst puts ~1.5% of ticks five times above the rest, and a
    // 1_200-tick bucket's 99th percentile therefore reports how many bursts that
    // minute drew rather than what the minute cost.
    let bucket = usize::try_from(MINUTE_TICKS).expect("minute ticks");
    let mut per_minute_p50: Vec<i64> = Vec::new();
    let mut per_minute_mean: Vec<i64> = Vec::new();
    let mut per_minute_p99: Vec<i64> = Vec::new();
    let mut i = 0usize;
    while i < total.len() {
        let e = (i + bucket).min(total.len());
        let mut slice = total[i..e].to_vec();
        slice.sort_unstable();
        per_minute_p50.push(pct(&slice, 50));
        per_minute_mean.push(mean(&slice));
        per_minute_p99.push(pct(&slice, 99));
        i = e;
    }

    // The number that justifies hashing chunk digests rather than voxels:
    // measured once, outside the tick, and published.
    let t = Instant::now();
    let d = world.voxels.whole_store_digest();
    let voxel_full_rehash_ns = i64::try_from(t.elapsed().as_nanos()).unwrap_or(i64::MAX);
    black_box(d);

    let ticks64 = u64::from(args.ticks);
    RunResult {
        hash_bytes_per_tick: agg.hash_bytes / ticks64.max(1),
        total,
        phases,
        phase_sum_ns,
        per_minute_p50,
        per_minute_mean,
        per_minute_p99,
        hashes,
        work: agg,
        allocs_milli_per_tick: (allocs_after - allocs_before) * 1_000 / ticks64.max(1),
        peak_heap: PEAK_BYTES.load(Ordering::Relaxed),
        checksum,
        voxel_full_rehash_ns,
        path_iters_total,
        kill_credit_death_ticks,
    }
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    let args = parse_args();
    let pin = match args.pin_core {
        Some(c) => format!("{} core {c}", affinity::pin(c)),
        None => "not requested".to_owned(),
    };

    let ps_per_iter = calibrate_ps_per_iter();
    let cost = CostConstants::from_calibration(ps_per_iter);
    let overhead_ns = timing_overhead_ns_per_tick();

    let mut runs: Vec<RunResult> = Vec::with_capacity(args.repeat);
    for _ in 0..args.repeat {
        runs.push(measure(&args, cost));
    }

    // The median run by total p99 (plan §3 step 1: "run each configuration
    // three times and report the median of the three p99s").
    let mut p99s: Vec<(i64, usize)> = runs
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let mut v = r.total.clone();
            v.sort_unstable();
            (pct(&v, 99), i)
        })
        .collect();
    p99s.sort_unstable();
    let median_idx = p99s[p99s.len() / 2].1;
    let r = &runs[median_idx];

    let hashes_identical = runs
        .iter()
        .all(|x| x.hashes == runs[0].hashes && x.checksum == runs[0].checksum);

    let mut total = r.total.clone();
    total.sort_unstable();
    let p50 = pct(&total, 50);
    let p90 = pct(&total, 90);
    let p99 = pct(&total, 99);
    let tmax = total.last().copied().unwrap_or(0);
    let tmean = mean(&total);
    // ticks/s and the speed multiple, in integer milli-units.
    let ticks_per_s_milli: i64 = if tmean == 0 {
        0
    } else {
        1_000_000_000_000 / tmean
    };
    let speed_multiple_milli = ticks_per_s_milli / i64::from(TICK_HZ);

    // The `pathing` phase's two halves. The charged half is known exactly: it is
    // the iteration count the tick asked for, times the calibration this process
    // measured. The rest of the phase is the real integer path-follow.
    let ticks64 = u64::from(args.ticks);
    let charged_ns_total =
        i64::try_from(u128::from(r.path_iters_total) * u128::from(ps_per_iter) / 1_000)
            .unwrap_or(i64::MAX);
    let pathing_mean_ns =
        r.phase_sum_ns[Phase::Pathing.index()] / i64::try_from(ticks64).expect("ticks fit i64");
    let charged_mean_ns = charged_ns_total / i64::try_from(ticks64).expect("ticks fit i64");
    let pathing_real_mean_ns = pathing_mean_ns - charged_mean_ns;
    let substituted_permille = if tmean == 0 {
        0
    } else {
        charged_mean_ns * 1_000 / tmean
    };
    // The only figure in this file that two operating systems can meaningfully
    // be compared on: the charged half reproduces G2's milliseconds by
    // construction on every machine, so it cannot show a platform difference.
    let tick_minus_pathing_mean_ns = tmean - pathing_mean_ns;

    let hash_digest = {
        let mut buf: Vec<u8> = Vec::with_capacity(r.hashes.len() * 17);
        for h in &r.hashes {
            buf.extend_from_slice(hex(*h).as_bytes());
            buf.push(b'\n');
        }
        digest(&buf)
    };

    // ---- report -------------------------------------------------------
    println!("# g3-tick-budget tickbench");
    println!(
        "# seats={} units={} beacons={} ticks={} warmup={} edits/s={} repath_cap={} burst=1-in-{} hash={}",
        args.seats,
        args.units,
        args.beacons,
        args.ticks,
        args.warmup,
        args.edits_per_s,
        cap_name(args.repath_cap),
        burst_name(args.burst_one_in),
        args.hash_mode.name()
    );
    println!(
        "# pin={pin} repeat={} os={}",
        args.repeat,
        std::env::consts::OS
    );
    println!(
        "# overflow_checks={} calibration={} ps/iter (repath={} iters)",
        overflow_checks_on(),
        ps_per_iter,
        cost.iters_per_repath
    );
    println!("tick_p50_ms\t{}", ms_ns(p50));
    println!("tick_p90_ms\t{}", ms_ns(p90));
    println!("tick_p99_ms\t{}", ms_ns(p99));
    println!("tick_max_ms\t{}", ms_ns(tmax));
    println!("tick_mean_ms\t{}", ms_ns(tmean));
    println!(
        "ticks_per_s\t{}.{:03}",
        ticks_per_s_milli / 1000,
        ticks_per_s_milli % 1000
    );
    println!(
        "speed_multiple\t{}.{:03}",
        speed_multiple_milli / 1000,
        speed_multiple_milli % 1000
    );
    for p in Phase::ALL {
        let mut v = r.phases[p.index()].clone();
        v.sort_unstable();
        println!(
            "phase\t{}{}\tn={}\tp50={}\tp99={}\tmean={}",
            p.name(),
            if p == Phase::Pathing {
                " (SUBSTITUTED)"
            } else {
                ""
            },
            v.len(),
            ms_ns(pct(&v, 50)),
            ms_ns(pct(&v, 99)),
            ms_ns(mean(&v))
        );
    }
    println!(
        "pathing_substituted_ms\tcharged={}\treal={}\t({}permille of the tick is charged, not measured)",
        ms_ns(charged_mean_ns),
        ms_ns(pathing_real_mean_ns),
        substituted_permille
    );
    println!(
        "tick_minus_pathing_mean_ms\t{}\t(the only part two platforms can differ on)",
        ms_ns(tick_minus_pathing_mean_ns)
    );
    println!("hash_digest\t{}", hex(hash_digest));
    println!("hash_streams_identical_across_repeats\t{hashes_identical}");
    println!("allocs_per_tick_milli\t{}", r.allocs_milli_per_tick);
    println!("peak_heap_bytes\t{}", r.peak_heap);
    println!("peak_rss_bytes\t{}", peak_rss::bytes());

    // ---- JSON ---------------------------------------------------------
    if let Some(path) = &args.json {
        let mut o = String::new();
        o.push('{');
        o.push_str(&format!("\"os\":\"{}\",", std::env::consts::OS));
        o.push_str(&format!("\"arch\":\"{}\",", std::env::consts::ARCH));
        o.push_str(&format!("\"overflow_checks\":{},", overflow_checks_on()));
        o.push_str(&format!("\"pin\":\"{pin}\","));
        o.push_str(&format!("\"timing_overhead_ns_per_tick\":{overhead_ns},"));
        o.push_str(&format!(
            "\"config\":{{\"seats\":{},\"units\":{},\"beacons\":{},\"structures\":{},\"ticks\":{},\"warmup\":{},\"edits_per_s\":{},\"repath_cap\":\"{}\",\"burst_one_in\":\"{}\",\"hash_mode\":\"{}\",\"seed\":{},\"repeat\":{}}},",
            args.seats,
            args.units,
            args.beacons,
            args.beacons * g3_tick_budget::STRUCTURES_PER_BEACON,
            args.ticks,
            args.warmup,
            args.edits_per_s,
            cap_name(args.repath_cap),
            burst_name(args.burst_one_in),
            args.hash_mode.name(),
            args.seed,
            args.repeat
        ));
        o.push_str(&format!(
            "\"calibration\":{{\"ps_per_iter\":{},\"iters_per_repath\":{},\"iters_per_repath_tail\":{},\"iters_per_crater_repair\":{},\"iters_csr_per_tick\":{},\"burst_one_in_PLACEHOLDER\":{},\"burst_factor_from_G2\":{},\"burst_note\":\"the 8x magnitude is G2's; the 1-in-N rate is NOT measured and the tick p99 is a direct readout of it. The base demand is scaled by N/(N+7) so the long-run mean stays at G2's 9.45 per crater whatever N is.\"}},",
            ps_per_iter,
            cost.iters_per_repath,
            cost.iters_per_repath_tail,
            cost.iters_per_crater_repair,
            cost.iters_csr_per_tick,
            args.burst_one_in,
            g3_tick_budget::tick::BURST_FACTOR
        ));
        o.push_str(&format!(
            "\"tick\":{{\"p50\":{},\"p90\":{},\"p99\":{},\"max\":{},\"mean\":{},\"ticks_per_s\":{}.{:03},\"speed_multiple\":{}.{:03}}},",
            ms_ns(p50),
            ms_ns(p90),
            ms_ns(p99),
            ms_ns(tmax),
            ms_ns(tmean),
            ticks_per_s_milli / 1000,
            ticks_per_s_milli % 1000,
            speed_multiple_milli / 1000,
            speed_multiple_milli % 1000
        ));
        let mut parts: Vec<String> = Vec::with_capacity(PHASE_COUNT);
        let ticks_i = i64::try_from(ticks64).expect("ticks fit i64");
        for p in Phase::ALL {
            let mut v = r.phases[p.index()].clone();
            // Share of the tick from the phase's total over the WHOLE run, not
            // from its sampled mean: `decision` is sampled on one tick in five
            // and a mean-based share would be five times its real contribution.
            let share_permille = if tmean == 0 {
                0
            } else {
                (r.phase_sum_ns[p.index()] / ticks_i) * 1_000 / tmean
            };
            let extra = if p == Phase::Pathing {
                format!(
                    ",\"substituted\":true,\"charged_ms\":{},\"real_ms\":{},\"substituted_permille_of_tick\":{},\"substituted_note\":\"charged_ms is iters x ps_per_iter reproducing G2's measured repath, cluster-repair and CSR-rebuild costs; only real_ms is work this spike performed\"",
                    ms_ns(charged_mean_ns),
                    ms_ns(pathing_real_mean_ns),
                    substituted_permille
                )
            } else {
                ",\"substituted\":false".to_owned()
            };
            parts.push(format!(
                "{{\"phase\":\"{}\",\"share_permille\":{},\"dist\":{}{}}}",
                p.name(),
                share_permille,
                dist(&mut v),
                extra
            ));
        }
        o.push_str(&format!("\"phases\":[{}],", parts.join(",")));
        o.push_str(&format!(
            "\"tick_minus_pathing_mean_ms\":{},",
            ms_ns(tick_minus_pathing_mean_ns)
        ));
        {
            let mut kc = r.kill_credit_death_ticks.clone();
            o.push_str(&format!(
                "\"kill_credit_settlement_ticks\":{{\"note\":\"the kill_credit phase's headline distribution is dominated by the ticks that settle nothing; this is the subset that settled at least one death\",\"deaths\":{},\"dist\":{}}},",
                r.work.deaths,
                dist(&mut kc)
            ));
        }
        let mins_p50: Vec<String> = r.per_minute_p50.iter().map(|v| ms_ns(*v)).collect();
        let mins_mean: Vec<String> = r.per_minute_mean.iter().map(|v| ms_ns(*v)).collect();
        let minutes: Vec<String> = r.per_minute_p99.iter().map(|v| ms_ns(*v)).collect();
        o.push_str(&format!("\"per_minute_p50_ms\":[{}],", mins_p50.join(",")));
        o.push_str(&format!(
            "\"per_minute_mean_ms\":[{}],",
            mins_mean.join(",")
        ));
        o.push_str(&format!("\"per_minute_p99_ms\":[{}],", minutes.join(",")));
        o.push_str(
            "\"per_minute_p99_note\":\"bimodal at the gate configuration: the 1-in-N repath burst puts ~1/N of ticks five times above the rest, so a 1200-tick bucket's p99 reports that minute's burst count, not its cost. Read drift off the mean or p50 series.\",",
        );
        let drift_of = |series: &[i64]| -> i64 {
            if series.len() >= 2 {
                let first = series[0];
                let last = series[series.len() - 1];
                if first == 0 {
                    0
                } else {
                    (last - first) * 1_000 / first
                }
            } else {
                0
            }
        };
        o.push_str(&format!(
            "\"drift_permille_first_to_last_minute\":{},",
            drift_of(&r.per_minute_mean)
        ));
        o.push_str(&format!(
            "\"drift_permille_first_to_last_minute_p50\":{},",
            drift_of(&r.per_minute_p50)
        ));
        o.push_str(&format!(
            "\"drift_permille_first_to_last_minute_p99_unreliable\":{},",
            drift_of(&r.per_minute_p99)
        ));
        let run_p99: Vec<String> = p99s.iter().map(|(v, _)| ms_ns(*v)).collect();
        o.push_str(&format!(
            "\"repeat\":{{\"runs\":{},\"p99_ms_sorted\":[{}],\"median_run\":{},\"hash_streams_identical\":{}}},",
            args.repeat,
            run_p99.join(","),
            median_idx,
            hashes_identical
        ));
        o.push_str(&format!(
            "\"hash\":{{\"mode\":\"{}\",\"digest\":\"{}\",\"bytes_per_tick\":{},\"tables_rehashed_per_tick_milli\":{},\"whole_voxel_store_rehash_ms\":{},\"store_bytes\":{}}},",
            args.hash_mode.name(),
            hex(hash_digest),
            r.hash_bytes_per_tick,
            u64::from(r.work.tables_rehashed) * 1_000 / u64::from(args.ticks),
            ms_ns(r.voxel_full_rehash_ns),
            g3_tick_budget::voxels::CHUNK_COUNT * g3_tick_budget::voxels::CHUNK_VOX
        ));
        o.push_str(&format!(
            "\"memory\":{{\"allocs_per_tick_milli\":{},\"peak_heap_bytes\":{},\"peak_rss_bytes\":{}}},",
            r.allocs_milli_per_tick,
            r.peak_heap,
            peak_rss::bytes()
        ));
        o.push_str(&format!(
            "\"work_totals\":{{\"units_seen\":{},\"programs_run\":{},\"queries\":{},\"candidates\":{},\"attacks\":{},\"damage_events\":{},\"deaths\":{},\"repaths_served\":{},\"repath_backlog_mean_milli\":{},\"craters\":{},\"voxels_removed\":{},\"chunks_rehashed\":{},\"rules\":{},\"brownouts\":{},\"checksum\":\"{}\"}}",
            r.work.units_seen,
            r.work.programs_run,
            r.work.queries,
            r.work.candidates,
            r.work.attacks,
            r.work.damage_events,
            r.work.deaths,
            r.work.repaths_served,
            u64::from(r.work.repath_backlog) * 1_000 / u64::from(args.ticks),
            r.work.craters,
            r.work.voxels_removed,
            r.work.chunks_rehashed,
            r.work.rules,
            r.work.brownouts,
            hex(r.checksum)
        ));
        o.push('}');
        write_file(path, o.as_bytes());
        println!("json\t{path}");
    }

    // ---- the hash stream ---------------------------------------------
    if let Some(path) = &args.hashes {
        let mut buf: Vec<u8> = Vec::with_capacity(r.hashes.len() * 17);
        for h in &r.hashes {
            buf.extend_from_slice(hex(*h).as_bytes());
            // An explicit LF byte: a CRLF on Windows would change the file and
            // turn a passing cross-OS comparison into a failing one for no sim
            // reason at all.
            buf.push(b'\n');
        }
        write_file(path, &buf);
        println!("hashes\t{path}\t{} lines", r.hashes.len());
    }

    // ---- assertions ---------------------------------------------------
    let mut failed = false;
    if let Some(limit) = args.assert_p99_ns {
        if p99 > limit {
            eprintln!(
                "FAIL tick p99 {} ms exceeds {} ms",
                ms_ns(p99),
                ms_ns(limit)
            );
            failed = true;
        } else {
            println!("ok\ttick p99 {} ms <= {} ms", ms_ns(p99), ms_ns(limit));
        }
    }
    if let Some(limit) = args.assert_mean_ns {
        if tmean > limit {
            eprintln!(
                "FAIL mean tick {} ms exceeds {} ms",
                ms_ns(tmean),
                ms_ns(limit)
            );
            failed = true;
        } else {
            println!("ok\tmean tick {} ms <= {} ms", ms_ns(tmean), ms_ns(limit));
        }
    }
    if !hashes_identical {
        eprintln!("FAIL the repeated runs did not produce identical hash streams");
        failed = true;
    }
    if failed {
        std::process::exit(1);
    }
}

fn cap_name(cap: u32) -> String {
    if cap == u32::MAX {
        "unbounded".to_owned()
    } else {
        cap.to_string()
    }
}

fn burst_name(burst: u32) -> String {
    if burst == 0 {
        "off".to_owned()
    } else {
        burst.to_string()
    }
}

/// Whether this build has overflow checks on. Plan §3 step 0 measures with them
/// **on**; the `release-unchecked` profile exists to take the delta once, and
/// this is how the JSON says which of the two produced it.
///
/// Probed at run time rather than read from a `cfg`: `cfg(overflow_checks)` is
/// still unstable on 1.98.1, and the whole point of the number is that it
/// describes the binary that took the measurement. `black_box` keeps the
/// addition out of const evaluation, where `255u8 + 1` is a compile error in
/// either profile; the panic hook is swapped out so the probe does not print a
/// panic message that a reader would take for a failure.
fn overflow_checks_on() -> bool {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let r = std::panic::catch_unwind(|| {
        let x: u8 = black_box(255);
        let y: u8 = black_box(1);
        black_box(x + y);
    });
    std::panic::set_hook(prev);
    r.is_err()
}

fn write_file(path: &str, bytes: &[u8]) {
    if let Some(dir) = std::path::Path::new(path).parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir).expect("create output directory");
    }
    let mut f = std::fs::File::create(path).expect("create output file");
    f.write_all(bytes).expect("write output file");
    f.sync_all().expect("sync output file");
}
