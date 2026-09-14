// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `costmodel` — plan §3 steps 4 and 5: the sweep that fits the per-entity
//! marginal costs, and the power-budget arithmetic those costs feed.
//!
//! **This is the other side of the wall.** Wall-clock time and floating point
//! live in `src/bin/` and nowhere else (plan §5's last two rows); the fits below
//! are the reason floating point is sanctioned there at all. Everything the
//! library computes is still integer, and `tests/wall.rs` checks that.
//!
//! The sweeps, exactly as plan §3 step 4 specifies them:
//!
//! * units over `{50, 100, 200, 300, 450, 600}` at 40 beacons;
//! * beacons over `{10, 20, 40, 80}` at 300 units;
//! * voxel edits over `{0, 10, 20, 40}` per second;
//! * plus the per-tick repath cap over `{8, 16, 32, unbounded}`, which the plan
//!   does not name because G2 had not reported when it was written. It is the
//!   lever decisions-log item 60 defines, and on this machine it turns out to be
//!   *the* lever, so it is swept.
//!
//! The unit sweep is run **twice** — once at the gate's 20 edits/s and once
//! with the destruction stream off. That is not padding: repath demand is
//! proportional to `units x craters`, so the two slopes are the marginal cost of
//! a unit with and without the destruction-driven repath load, and quoting only
//! the first would hide an interaction term inside a number called "ns per unit
//! per tick".
//!
//! Step 5's arithmetic is printed with **every input named**, so S2 can redo it
//! by substituting real measurements for the spike's estimates. The reserve and
//! the kW-per-unit conversion are `PLACEHOLDER` tuning values and are labelled
//! as such in the output, not just in this comment.
//!
//! **Measurement hygiene, because this binary produces the headline number.**
//! Plan §3 step 1 applies to it exactly as it applies to `tickbench`: the
//! process is pinned with `--pin-core`, each sweep point is run `--repeat` times
//! and the median by p99 is kept, and `--ticks-per-point` / `--warmup` default
//! to 2_400 / 2_000 rather than a few hundred. The first version of this file
//! had none of that and its `gate` point disagreed with `tickbench`'s by 5.6%
//! for the identical configuration, which is larger than several of the effects
//! the fits are trying to resolve.
//!
//! **Three things in the output are labelled rather than quietly reported**, and
//! the labels are the finding as much as the numbers are:
//!
//! * the per-crater cost is G2's repath-and-repair *consequence* of a crater,
//!   not the cost of writing one into the chunk store — those differ by about
//!   three orders of magnitude, and both are published;
//! * the fixed per-tick term is mostly G2's CSR rebuild, charged on every tick
//!   whether or not anything was destroyed, so it is published decomposed;
//! * the beacon term does not resolve at this spike's precision and is
//!   published as a bracket. It cancels out of the budget arithmetic anyway.

use std::hint::black_box;
use std::io::Write as _;
use std::time::Instant;

use g3_tick_budget::hash::HashMode;
use g3_tick_budget::tick::{Config, CostConstants, PHASE_COUNT, Phase, Work, World};
use g3_tick_budget::{STRUCTURES_PER_BEACON, TICK_HZ, synthetic_work};

// ---------------------------------------------------------------------------
// Core affinity and the profile probe. Identical to `tickbench`'s, and here for
// the same reason plan §3 step 1 gives: "Pin the process to a core", "before any
// number is taken". This binary produces the power budget, which is the spike's
// named deliverable, so it may not be the one measurement taken unpinned.
// ---------------------------------------------------------------------------

mod affinity {
    #[cfg(windows)]
    unsafe extern "system" {
        fn GetCurrentThread() -> *mut core::ffi::c_void;
        fn SetThreadAffinityMask(thread: *mut core::ffi::c_void, mask: usize) -> usize;
    }

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

/// Whether this build has overflow checks on. Copied from `tickbench`: plan §3
/// step 0 makes the measured profile a condition of the budget, so the artefact
/// that carries the budget has to say which profile produced it rather than
/// leaving the reader to trust `run-measurements.sh`.
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

// ---------------------------------------------------------------------------
// Small float helpers. Permitted here, forbidden everywhere else in the crate.
// ---------------------------------------------------------------------------

/// Ordinary least squares for `y = a + b x`. Returns `(b, a, r2)`.
fn fit_linear(xs: &[f64], ys: &[f64]) -> (f64, f64, f64) {
    let n = xs.len() as f64;
    let sx: f64 = xs.iter().sum();
    let sy: f64 = ys.iter().sum();
    let sxx: f64 = xs.iter().map(|x| x * x).sum();
    let sxy: f64 = xs.iter().zip(ys).map(|(x, y)| x * y).sum();
    let den = n * sxx - sx * sx;
    if den.abs() < f64::EPSILON {
        return (0.0, sy / n, 0.0);
    }
    let b = (n * sxy - sx * sy) / den;
    let a = (sy - b * sx) / n;
    let mean = sy / n;
    let ss_tot: f64 = ys.iter().map(|y| (y - mean) * (y - mean)).sum();
    let ss_res: f64 = xs
        .iter()
        .zip(ys)
        .map(|(x, y)| {
            let e = y - (a + b * x);
            e * e
        })
        .sum();
    let r2 = if ss_tot > 0.0 {
        1.0 - ss_res / ss_tot
    } else {
        1.0
    };
    (b, a, r2)
}

/// Ordinary least squares for `y = a + b x + c x^2`, by Gaussian elimination on
/// the 3x3 normal equations. Returns `(a, b, c)`.
fn fit_quadratic(xs: &[f64], ys: &[f64]) -> (f64, f64, f64) {
    let mut m = [[0.0f64; 4]; 3];
    for (x, y) in xs.iter().zip(ys) {
        let p = [1.0, *x, x * x];
        for r in 0..3 {
            for c in 0..3 {
                m[r][c] += p[r] * p[c];
            }
            m[r][3] += p[r] * y;
        }
    }
    for i in 0..3 {
        let mut piv = i;
        for r in (i + 1)..3 {
            if m[r][i].abs() > m[piv][i].abs() {
                piv = r;
            }
        }
        m.swap(i, piv);
        if m[i][i].abs() < 1e-12 {
            return (0.0, 0.0, 0.0);
        }
        for r in 0..3 {
            if r == i {
                continue;
            }
            let f = m[r][i] / m[i][i];
            let row_i = m[i];
            for (c, v) in m[r].iter_mut().enumerate().skip(i) {
                *v -= f * row_i[c];
            }
        }
    }
    (m[0][3] / m[0][0], m[1][3] / m[1][1], m[2][3] / m[2][2])
}

fn ns_to_ms(ns: f64) -> f64 {
    ns / 1_000_000.0
}

fn j(v: f64) -> String {
    if v.is_finite() {
        format!("{v:.4}")
    } else {
        "null".to_owned()
    }
}

// ---------------------------------------------------------------------------
// One sweep point
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Point {
    label: String,
    seats: usize,
    units: usize,
    beacons: usize,
    edits: u32,
    cap: u32,
    mean_ns: f64,
    p99_ns: f64,
    phase_mean_ns: [f64; PHASE_COUNT],
    /// Mean outstanding repath backlog over the measured ticks, and the backlog
    /// at the last tick.
    ///
    /// **This is what tells a reader that a repath cap is not an operating
    /// point.** Mean demand at the gate configuration is G2's 9.45 repaths per
    /// crater at one crater a tick, so any cap below that never drains: the
    /// queue grows without bound and the tick looks cheap only because a share
    /// of the modelled work is being deferred for ever. Without these fields the
    /// sweep printed cap 8 as the cheapest row in the table with nothing to say
    /// it was infeasible.
    backlog_mean: f64,
    backlog_final: f64,
    /// Backlog at the last measured tick minus the backlog at the first. **This,
    /// not the served fraction, is the sustainability test**: demand is carried
    /// in thousandths of a repath while service is whole repaths, so a sub-unit
    /// remainder is always outstanding and the served fraction never reaches
    /// exactly 1.0 however long the window.
    backlog_growth: f64,
    /// Repaths served over the measured ticks divided by repaths demanded.
    served_fraction: f64,
    repeats: usize,
}

impl Point {
    /// A cap is sustainable only if the queue it leaves behind is no longer at
    /// the end of the window than at the start.
    fn sustainable(&self) -> bool {
        self.backlog_growth <= 1.0
    }
}

fn pct(sorted: &[i64], p: i64) -> i64 {
    if sorted.is_empty() {
        return 0;
    }
    let n = i64::try_from(sorted.len()).expect("sample count fits i64");
    let idx = ((p * (n - 1)) + 50) / 100;
    let i = usize::try_from(idx.clamp(0, n - 1)).expect("clamped");
    sorted[i]
}

/// One sweep point's shape, so `run_point` takes four arguments rather than
/// nine — clippy's `too_many_arguments` is on under `-D warnings` and, for a
/// function called with six positional integers, it is right.
#[derive(Clone, Copy, Debug)]
struct Shape {
    seats: usize,
    units: usize,
    beacons: usize,
    edits: u32,
    cap: u32,
    burst_one_in: u32,
}

/// One measurement of one configuration.
struct Sample {
    mean_ns: f64,
    p99_ns: f64,
    phase_mean_ns: [f64; PHASE_COUNT],
    backlog_mean: f64,
    backlog_final: f64,
    backlog_growth: f64,
    served_fraction: f64,
}

fn run_once(sh: Shape, ticks: u32, warmup: u32, cost: CostConstants) -> Sample {
    let Shape {
        seats,
        units,
        beacons,
        edits,
        cap,
        burst_one_in,
    } = sh;
    let cfg = Config {
        match_seed: g3_tick_budget::DEFAULT_MATCH_SEED,
        seats,
        units,
        beacons,
        edits_per_s: edits,
        repath_cap: cap,
        burst_one_in,
        hash_mode: HashMode::Full,
        cost,
    };
    let mut world = World::new(cfg);
    for _ in 0..warmup {
        black_box(world.step());
    }
    // Demand and service are counted over the measured window only, so a
    // backlog built up during warm-up is not charged to the measurement.
    let demand_before = world.path.demand_total;
    let served_before = world.path.served_total;
    let n = usize::try_from(ticks).expect("tick count");
    let mut total: Vec<i64> = Vec::with_capacity(n);
    let mut phase_sum = [0i64; PHASE_COUNT];
    let mut decision_ticks = 0i64;
    let mut agg = Work::default();
    let backlog_start = world.path.queue_milli / 1_000;
    let mut backlog_final: u32 = 0;
    for _ in 0..ticks {
        let mut tick_ns: i64 = 0;
        for p in Phase::ALL {
            // Two stamps per phase, bookkeeping outside them — the same
            // discipline `tickbench` uses, so the two binaries' gate points are
            // comparable.
            let t_start = Instant::now();
            let w = black_box(world.phase(p));
            let t_end = Instant::now();
            let ns = i64::try_from(t_end.duration_since(t_start).as_nanos()).unwrap_or(i64::MAX);
            tick_ns += ns;
            phase_sum[p.index()] += ns;
            if p == Phase::Decision && w.rules > 0 {
                decision_ticks += 1;
            }
            if p == Phase::Pathing {
                backlog_final = w.repath_backlog;
            }
            agg.merge(w);
        }
        total.push(tick_ns);
    }
    let demand = world.path.demand_total - demand_before;
    let served = world.path.served_total - served_before;
    let sum: i128 = total.iter().map(|v| i128::from(*v)).sum();
    let mean_ns = (sum as f64) / (total.len() as f64);
    total.sort_unstable();
    let mut phase_mean_ns = [0.0f64; PHASE_COUNT];
    for p in Phase::ALL {
        // The decision phase is averaged over the ticks it ran on; every other
        // phase over every tick.
        let d = if p == Phase::Decision {
            decision_ticks.max(1) as f64
        } else {
            f64::from(ticks)
        };
        phase_mean_ns[p.index()] = (phase_sum[p.index()] as f64) / d;
    }
    // `demand_total` is in thousandths of a repath; `served_total` is in whole
    // repaths.
    let demand_repaths = (demand as f64) / 1_000.0;
    Sample {
        mean_ns,
        p99_ns: pct(&total, 99) as f64,
        phase_mean_ns,
        backlog_mean: (agg.repath_backlog as f64) / f64::from(ticks),
        backlog_final: f64::from(backlog_final),
        backlog_growth: f64::from(backlog_final) - (backlog_start as f64),
        served_fraction: if demand_repaths > 0.0 {
            (served as f64) / demand_repaths
        } else {
            1.0
        },
    }
}

fn run_point(
    label: &str,
    sh: Shape,
    ticks: u32,
    warmup: u32,
    repeat: usize,
    cost: CostConstants,
) -> Point {
    // Plan §3 step 1: "run each configuration three times and report the median
    // of the three p99s". `tickbench` honoured this from the start; this binary
    // did not, and its gate point disagreed with `tickbench`'s by 5.6% as a
    // result.
    let mut samples: Vec<Sample> = (0..repeat.max(1))
        .map(|_| run_once(sh, ticks, warmup, cost))
        .collect();
    samples.sort_by(|a, b| a.p99_ns.total_cmp(&b.p99_ns));
    let s = &samples[samples.len() / 2];
    let pt = Point {
        label: label.to_owned(),
        seats: sh.seats,
        units: sh.units,
        beacons: sh.beacons,
        edits: sh.edits,
        cap: sh.cap,
        mean_ns: s.mean_ns,
        p99_ns: s.p99_ns,
        phase_mean_ns: s.phase_mean_ns,
        backlog_mean: s.backlog_mean,
        backlog_final: s.backlog_final,
        backlog_growth: s.backlog_growth,
        served_fraction: s.served_fraction,
        repeats: repeat.max(1),
    };
    println!(
        "point\t{label}\tunits={}\tbeacons={}\tedits={}\tcap={}\tmean={:.4} ms\tp99={:.4} ms\tbacklog_mean={:.2}\tserved={:.3}{}",
        sh.units,
        sh.beacons,
        sh.edits,
        cap_name(sh.cap),
        ns_to_ms(pt.mean_ns),
        ns_to_ms(pt.p99_ns),
        pt.backlog_mean,
        pt.served_fraction,
        if pt.sustainable() {
            ""
        } else {
            "\tUNSUSTAINABLE (the backlog never drains; this is not an operating point)"
        }
    );
    pt
}

fn cap_name(cap: u32) -> String {
    if cap == u32::MAX {
        "unbounded".to_owned()
    } else {
        cap.to_string()
    }
}

// ---------------------------------------------------------------------------
// Calibration (identical to `tickbench`'s, deliberately)
// ---------------------------------------------------------------------------

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
                samples.push(u64::try_from(ns * 1_000 / u128::from(iters)).unwrap_or(1));
                break;
            }
            iters *= 2;
        }
    }
    samples.sort_unstable();
    samples[samples.len() / 2].max(1)
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

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

/// `PLACEHOLDER` — the reserve for what the synthetic tick does not model.
/// Plan §3 step 5 recommends ">= 40% at spike stage because a synthetic tick
/// always flatters the real one". **Owner decides, at S2's exit measurement**,
/// which is when the reserve stops being a guess.
const RESERVE_PERCENT: f64 = 40.0;

/// `PLACEHOLDER` — kW drawn per fielded unit, the conversion that turns an
/// affordable unit count into the per-map power budget. Section 19 lists it as
/// a Tuning value. The toy uses 1 kW per unit (`UNIT_KW`), which is a
/// placeholder standing in for it and nothing more. **Owner decides, with the
/// rules table at S1/S2.**
const KW_PER_UNIT_PLACEHOLDER: f64 = 1.0;

/// G3'-a: tick p99 <= 50% of a 50 ms tick.
const BUDGET_P99_MS: f64 = 25.0;
/// G3'-b: >= 4x real time on the headless runner, i.e. a mean tick <= 12.5 ms.
const BUDGET_MEAN_MS: f64 = 12.5;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let ticks: u32 =
        value(&a, "--ticks-per-point").map_or(2_400, |v| v.parse().expect("--ticks-per-point"));
    let warmup: u32 = value(&a, "--warmup").map_or(2_000, |v| v.parse().expect("--warmup"));
    let repeat: usize = value(&a, "--repeat").map_or(1, |v| v.parse().expect("--repeat"));
    let seats: usize = value(&a, "--seats").map_or(4, |v| v.parse().expect("--seats"));
    let gate_cap: u32 = value(&a, "--repath-cap").map_or(u32::MAX, |v| {
        if v == "unbounded" {
            u32::MAX
        } else {
            v.parse().expect("--repath-cap")
        }
    });
    let burst_one_in: u32 =
        value(&a, "--burst-one-in").map_or(g3_tick_budget::tick::DEFAULT_BURST_ONE_IN, |v| {
            if v == "off" {
                0
            } else {
                v.parse().expect("--burst-one-in")
            }
        });
    let pin = match a.iter().any(|x| x == "--pin-core") {
        true => {
            let c: u32 = value(&a, "--pin-core")
                .and_then(|v| v.parse().ok())
                .unwrap_or(2);
            format!("{} core {c}", affinity::pin(c))
        }
        false => "not requested".to_owned(),
    };

    let json = value(&a, "--json");

    let ps = calibrate_ps_per_iter();
    let cost = CostConstants::from_calibration(ps);
    let checks = overflow_checks_on();
    println!("# g3-tick-budget costmodel");
    println!(
        "# os={} seats={seats} ticks_per_point={ticks} warmup={warmup} repeat={repeat} pin={pin} overflow_checks={checks} calibration={ps} ps/iter burst=1-in-{burst_one_in}",
        std::env::consts::OS
    );

    // ---- the sweeps ---------------------------------------------------
    let unit_counts = [50usize, 100, 200, 300, 450, 600];
    let beacon_counts = [10usize, 20, 40, 80];
    let edit_rates = [0u32, 10, 20, 40];
    let caps = [8u32, 16, 32, u32::MAX];

    let base = Shape {
        seats,
        units: 300,
        beacons: 40,
        edits: 20,
        cap: gate_cap,
        burst_one_in,
    };
    let mut units_sweep: Vec<Point> = Vec::new();
    for u in unit_counts {
        units_sweep.push(run_point(
            "units@20edits",
            Shape { units: u, ..base },
            ticks,
            warmup,
            repeat,
            cost,
        ));
    }
    let mut units_sweep_quiet: Vec<Point> = Vec::new();
    for u in unit_counts {
        units_sweep_quiet.push(run_point(
            "units@0edits",
            Shape {
                units: u,
                edits: 0,
                ..base
            },
            ticks,
            warmup,
            repeat,
            cost,
        ));
    }
    let mut beacons_sweep: Vec<Point> = Vec::new();
    for b in beacon_counts {
        beacons_sweep.push(run_point(
            "beacons",
            Shape { beacons: b, ..base },
            ticks,
            warmup,
            repeat,
            cost,
        ));
    }
    // The same beacon sweep with the destruction stream off. It is here because
    // the 20 edits/s sweep cannot see a beacon: the tick is ~12 ms and varies by
    // ~0.1 ms between 10 and 80 beacons, which is inside the run-to-run spread,
    // so the fit comes back with a negative slope and r2 ~ 0.1. With destruction
    // off the tick is ~0.7 ms, which is where any beacon signal would have to
    // show up. It does not resolve there either — see the bracket reported
    // below — and the honest answer is an upper bound, not a fitted value.
    let mut beacons_sweep_quiet: Vec<Point> = Vec::new();
    for b in beacon_counts {
        beacons_sweep_quiet.push(run_point(
            "beacons@0edits",
            Shape {
                beacons: b,
                edits: 0,
                ..base
            },
            ticks,
            warmup,
            repeat,
            cost,
        ));
    }
    let mut edits_sweep: Vec<Point> = Vec::new();
    for e in edit_rates {
        edits_sweep.push(run_point(
            "edits",
            Shape { edits: e, ..base },
            ticks,
            warmup,
            repeat,
            cost,
        ));
    }
    let mut caps_sweep: Vec<Point> = Vec::new();
    for c in caps {
        caps_sweep.push(run_point(
            "repath-cap",
            Shape { cap: c, ..base },
            ticks,
            warmup,
            repeat,
            cost,
        ));
    }
    // The gate configuration itself, and plan §1's open question.
    let gate = run_point("gate", base, ticks, warmup, repeat, cost);
    let per_seat = run_point(
        "per-seat-reading",
        Shape {
            units: 1_200,
            beacons: 160,
            ..base
        },
        ticks,
        warmup,
        repeat,
        cost,
    );

    // ---- the fits -----------------------------------------------------
    //
    // WHICH SERIES THE LINEARITY VERDICT IS FITTED TO MATTERS MORE THAN THE FIT.
    // Plan §3 step 4's last paragraph asks whether the *broadphase* is
    // near-linear in the unit count. The 20-edits sweep cannot answer that: ~98%
    // of that tick is the G2 pathing charge, whose demand is `9.45 x units/300`
    // per crater — linear in units *by construction*, so the test can only ever
    // come back "linear". The first version of this file fitted exactly that
    // series and duly reported `linear` for a sim whose own per-unit cost grows
    // by tens of percent across the sweep. The verdict is now taken from the
    // destruction-off sweep, and separately from the broadphase phase series
    // inside it, which is the thing the plan actually asks about.
    let xs: Vec<f64> = units_sweep.iter().map(|p| p.units as f64).collect();
    let ys: Vec<f64> = units_sweep.iter().map(|p| p.mean_ns).collect();
    let (ns_per_unit, _unit_intercept, unit_r2) = fit_linear(&xs, &ys);

    let xs_q: Vec<f64> = units_sweep_quiet.iter().map(|p| p.units as f64).collect();
    let ys_q: Vec<f64> = units_sweep_quiet.iter().map(|p| p.mean_ns).collect();
    let (ns_per_unit_quiet, unit_intercept_quiet, unit_r2_quiet) = fit_linear(&xs_q, &ys_q);

    /// The quadratic term's share of the cost at the top of the sweep.
    fn superlinear_share_of(xs: &[f64], ys: &[f64]) -> f64 {
        let (_qa, _qb, qc) = fit_quadratic(xs, ys);
        let x_max = xs.last().copied().unwrap_or(600.0);
        let y_at_max = ys.last().copied().unwrap_or(0.0);
        if y_at_max > 0.0 {
            (qc * x_max * x_max) / y_at_max
        } else {
            0.0
        }
    }
    fn shape_name(share: f64) -> &'static str {
        if share.abs() < 0.10 {
            "linear"
        } else {
            "super-linear"
        }
    }
    /// Residuals against the linear fit, in ns. `r2` on six points hides a
    /// monotone curvature; a U-shaped residual series does not.
    fn residuals(xs: &[f64], ys: &[f64]) -> Vec<f64> {
        let (b, a, _) = fit_linear(xs, ys);
        xs.iter()
            .zip(ys)
            .map(|(x, y)| y - (a + b * x))
            .collect::<Vec<f64>>()
    }
    /// Marginal cost per unit between consecutive sweep points.
    fn marginals(xs: &[f64], ys: &[f64]) -> Vec<f64> {
        (1..xs.len())
            .map(|i| (ys[i] - ys[i - 1]) / (xs[i] - xs[i - 1]))
            .collect::<Vec<f64>>()
    }

    // The 20-edits shape, kept only as a check that the substitute is linear.
    let superlinear_share_loud = superlinear_share_of(&xs, &ys);
    // The verdict: the destruction-off total, and the broadphase inside it.
    let superlinear_share_quiet = superlinear_share_of(&xs_q, &ys_q);
    let ys_broadphase: Vec<f64> = units_sweep_quiet
        .iter()
        .map(|p| p.phase_mean_ns[Phase::Broadphase.index()])
        .collect();
    let superlinear_share_broadphase = superlinear_share_of(&xs_q, &ys_broadphase);
    let quiet_residuals = residuals(&xs_q, &ys_q);
    let quiet_marginals = marginals(&xs_q, &ys_q);
    let broadphase_marginals = marginals(&xs_q, &ys_broadphase);

    let xb: Vec<f64> = beacons_sweep_quiet
        .iter()
        .map(|p| p.beacons as f64)
        .collect();
    let yb: Vec<f64> = beacons_sweep_quiet.iter().map(|p| p.mean_ns).collect();
    let (ns_per_beacon, _beacon_intercept, beacon_r2) = fit_linear(&xb, &yb);
    // The beacon term's bracket: the slope between each consecutive pair. If
    // those disagree in sign the OLS slope is noise, whatever its point value.
    let beacon_segment_slopes = marginals(&xb, &yb);
    let beacon_lo = beacon_segment_slopes
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);
    let beacon_hi = beacon_segment_slopes
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    // "Resolved" means: the segment slopes agree in sign and the fit explains
    // most of the variance. Anything else is published as a bound.
    let beacon_resolved = beacon_r2 >= 0.90 && beacon_lo.signum() == beacon_hi.signum();
    // The same fit on the 20 edits/s sweep, reported so a reader can see why it
    // is not the one used.
    let xb_loud: Vec<f64> = beacons_sweep.iter().map(|p| p.beacons as f64).collect();
    let yb_loud: Vec<f64> = beacons_sweep.iter().map(|p| p.mean_ns).collect();
    let (ns_per_beacon_loud, _, beacon_r2_loud) = fit_linear(&xb_loud, &yb_loud);

    let xe: Vec<f64> = edits_sweep.iter().map(|p| f64::from(p.edits)).collect();
    let ye: Vec<f64> = edits_sweep.iter().map(|p| p.mean_ns).collect();
    let (ns_per_edit_per_s_at_300, _ei, edit_r2) = fit_linear(&xe, &ye);

    let ns_per_decision_per_seat = gate.phase_mean_ns[Phase::Decision.index()] / (seats as f64);

    // ---- the interaction term, and why the model needs one ---------------
    //
    // A plain `fixed + a*units + b*beacons + c*edits` is MIS-SPECIFIED here,
    // and the first version of this file duly produced a fixed overhead of
    // -8.3 ms and a power budget of zero. The reason is G2: repath demand is
    // 9.45 repaths per crater **at 300 units**, and it scales with the unit
    // count, so the true term is `d * units * edits_per_s`. Fitting units at
    // 20 edits/s and edits at 300 units separately counts that product twice
    // and then subtracts it out of the intercept.
    //
    // The model this spike reports is therefore
    //
    //     mean_tick(u, b, e) = fixed + a0*u + beta*b + c0*e + d*u*e
    //
    // and its four coefficients are read off the four sweeps:
    //
    //     a0   = the unit slope with destruction OFF   (what a unit costs the sim)
    //     d    = (a20 - a0) / 20                       (repath load per unit per edit/s)
    //     c0   = slope_e(at 300 units) - d * 300       (crater carve + repair, unit-free)
    //     beta = the beacon slope with destruction OFF
    let d_unit_per_edit = (ns_per_unit - ns_per_unit_quiet) / 20.0;
    let c0_per_edit_per_s = ns_per_edit_per_s_at_300 - d_unit_per_edit * 300.0;
    // Per *crater*, not per edit/s: a rate of E edits/s is E/20 craters a tick.
    //
    // NAMING, because the first version of this file got it wrong in a way that
    // was out by three orders of magnitude. What the edits sweep measures is
    // almost entirely G2's *consequence* of a crater — the repaths it provokes
    // and the cluster repair that follows — not the cost of writing the crater
    // into the copy-on-write chunk store. The store's own per-edit cost is the
    // `voxels` phase, measured below and published separately. A reader who took
    // "ns per voxel edit" for the chunk store would have been wrong by ~1800x.
    let ns_per_crater_repath_consequence_per_unit = d_unit_per_edit * f64::from(TICK_HZ);
    let ns_per_crater_repair_unit_free = c0_per_edit_per_s * f64::from(TICK_HZ);
    let ns_per_crater_pathing_consequence_at_300 = ns_per_edit_per_s_at_300 * f64::from(TICK_HZ);
    // The chunk store's own cost of applying one crater: the `voxels` phase mean
    // at the gate configuration, divided by craters per tick. Measured, not
    // fitted, and not substituted.
    let gate_craters_per_tick = f64::from(base.edits) / f64::from(TICK_HZ);
    let ns_per_voxel_edit_chunk_store = if gate_craters_per_tick > 0.0 {
        gate.phase_mean_ns[Phase::Voxels.index()] / gate_craters_per_tick
    } else {
        0.0
    };

    // ---- the fixed term, decomposed --------------------------------------
    //
    // `unit_intercept_quiet` is the whole tick at zero units and 40 beacons with
    // destruction off. Calling that "the fixed per-tick overhead (broadphase
    // rebuild, hash, bookkeeping)", which is the plan's phrase, would be wrong
    // by about 94%: `phase_pathing` charges G2's CSR rebuild — 0.5 ms — on every
    // tick whether or not anything was destroyed, so most of the intercept is a
    // G2 constant rather than this sim's overhead. S2 redoing this arithmetic
    // with real code would double-count it. Both halves are therefore published.
    //
    // (Whether the CSR rebuild should be charged at all on a tick with no
    // destruction is a question for G2/S2: G2 measured `csr_rebuild_ms_per_tick`
    // under a 20 craters/s stream, and charging it at 0 edits/s is what makes
    // this spike's quiet baseline about three times the sim's own cost.)
    let charged_csr_ns = g3_tick_budget::g2::CSR_REBUILD_NS_PER_TICK as f64;
    let fixed_sim_own_ns = unit_intercept_quiet - charged_csr_ns;
    // The beacon term is NOT subtracted here, and the budget does not add it
    // back. It used to be both, which cancelled exactly — so the budget was
    // never sensitive to a number quoted at r2 = 0.37 over a non-monotone
    // series. Leaving the 40 beacons inside the intercept says that out loud.
    let fixed_overhead_ns = unit_intercept_quiet;

    println!("fit\tns_per_unit_per_tick@20edits\t{ns_per_unit:.1}\tr2={unit_r2:.5}");
    println!(
        "fit\tns_per_unit_per_tick@0edits (a0, the sim's own)\t{ns_per_unit_quiet:.1}\tr2={unit_r2_quiet:.5}"
    );
    println!("fit\tns_per_unit_per_edit_per_s (d, G2's repath load)\t{d_unit_per_edit:.1}");
    if beacon_resolved {
        println!(
            "fit\tns_per_beacon_per_tick (from the quiet sweep)\t{ns_per_beacon:.1}\tr2={beacon_r2:.5}"
        );
    } else {
        println!(
            "fit\tns_per_beacon_per_tick\tUNRESOLVED at this precision: OLS says {ns_per_beacon:.1} at r2={beacon_r2:.5}, but the consecutive-pair slopes bracket it at [{beacon_lo:.1}, {beacon_hi:.1}] ns/beacon and do not agree in sign. Report it as an upper bound, not a value."
        );
        println!(
            "fit\tns_per_beacon_per_tick (20 edits/s, further below the floor)\t{ns_per_beacon_loud:.1}\tr2={beacon_r2_loud:.5}"
        );
        println!(
            "fit\t  ...the beacon term CANCELS out of the step-5 arithmetic: it used to be subtracted from the intercept and added straight back into the budget base. It is now neither."
        );
    }
    println!(
        "fit\tns_per_crater_pathing_consequence (G2's, SUBSTITUTED)\t{ns_per_crater_pathing_consequence_at_300:.1} at 300 units = {ns_per_crater_repair_unit_free:.1} repair/unit-free + {ns_per_crater_repath_consequence_per_unit:.1} repath per unit\tr2={edit_r2:.5}"
    );
    println!(
        "fit\tns_per_voxel_edit_chunk_store (MEASURED, the `voxels` phase)\t{ns_per_voxel_edit_chunk_store:.1}\t({:.0}x smaller than the pathing consequence above)",
        if ns_per_voxel_edit_chunk_store > 0.0 {
            ns_per_crater_pathing_consequence_at_300 / ns_per_voxel_edit_chunk_store
        } else {
            0.0
        }
    );
    println!("fit\tns_per_decision_tick_per_seat\t{ns_per_decision_per_seat:.1}");
    println!(
        "fit\tfixed_per_tick_ms\t{:.5} total = {:.5} the sim's own (broadphase rebuild, hash, bookkeeping, 40 beacons) + {:.5} G2's CSR rebuild, charged on every tick",
        ns_to_ms(fixed_overhead_ns),
        ns_to_ms(fixed_sim_own_ns),
        ns_to_ms(charged_csr_ns)
    );
    println!(
        "fit\tunit_cost_shape@0edits (THE VERDICT — the sim, no G2 charge)\t{}\t(quadratic term is {:.1}% of the cost at 600 units)",
        shape_name(superlinear_share_quiet).to_uppercase(),
        superlinear_share_quiet * 100.0
    );
    println!(
        "fit\tbroadphase_cost_shape@0edits (plan §3 step 4's actual question)\t{}\t(quadratic term is {:.1}% of the phase at 600 units; marginal ns/unit {})",
        shape_name(superlinear_share_broadphase).to_uppercase(),
        superlinear_share_broadphase * 100.0,
        broadphase_marginals
            .iter()
            .map(|v| format!("{v:.0}"))
            .collect::<Vec<_>>()
            .join(" -> ")
    );
    println!(
        "fit\tunit_cost_shape@20edits (a check that the SUBSTITUTE is linear, nothing more)\t{}\t({:.1}%)",
        shape_name(superlinear_share_loud),
        superlinear_share_loud * 100.0
    );
    println!(
        "fit\tquiet linear-fit residuals (ns)\t{}",
        quiet_residuals
            .iter()
            .map(|v| format!("{v:+.0}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    println!(
        "fit\tquiet marginal ns/unit between points\t{}",
        quiet_marginals
            .iter()
            .map(|v| format!("{v:.0}"))
            .collect::<Vec<_>>()
            .join(" -> ")
    );

    // ---- step 5: the power budget ------------------------------------
    //
    // The two constraints bind on different things, which is itself the finding:
    //
    //  * the MEAN constraint is a UNIT-COUNT constraint. Every unit costs
    //    `a0 + d * edits_per_s` per tick, and at 20 edits/s the second term is
    //    some thirty times the first.
    //  * the P99 constraint is a REPATH-CAP constraint. The tail of this
    //    distribution is a burst of repaths landing on one tick, and the
    //    per-tick cap (decisions-log item 60) is an absolute ceiling on how much
    //    of that burst any one tick may carry — it does not depend on the unit
    //    count at all. So the p99 branch solves for the cap rather than for a
    //    unit count, and the sweep over {8, 16, 32, unbounded} is the evidence.
    let reserve = 1.0 - RESERVE_PERCENT / 100.0;
    let eff_p99_ms = BUDGET_P99_MS * reserve;
    let eff_mean_ms = BUDGET_MEAN_MS * reserve;
    // `fixed_ms` is the whole quiet intercept at 40 beacons — the beacon term is
    // inside it and is not added again below. See the decomposition above.
    let fixed_ms = ns_to_ms(fixed_overhead_ns);
    let fixed_sim_own_ms = ns_to_ms(fixed_sim_own_ns);
    let charged_csr_ms = ns_to_ms(charged_csr_ns);
    let gate_edits = f64::from(base.edits);
    let craters_per_tick = gate_edits / f64::from(TICK_HZ);
    let unit_free_edit_ms = ns_to_ms(c0_per_edit_per_s * gate_edits);
    let per_unit_ms = ns_to_ms(ns_per_unit_quiet + d_unit_per_edit * gate_edits);
    let per_unit_quiet_ms = ns_to_ms(ns_per_unit_quiet);

    let affordable_at = |edits: f64| -> f64 {
        let pu = ns_to_ms(ns_per_unit_quiet + d_unit_per_edit * edits);
        let base_ms = fixed_ms + ns_to_ms(c0_per_edit_per_s * edits);
        if pu > 0.0 {
            ((eff_mean_ms - base_ms) / pu).max(0.0)
        } else {
            0.0
        }
    };
    let units_from_mean = affordable_at(gate_edits);
    let units_from_mean_quiet = affordable_at(0.0);

    // The p99 branch. G2's own numbers are the inputs, so they are named rather
    // than re-derived: a repath is 0.77 ms at the median, cluster repair is
    // 2.5 ms per crater, and the CSR rebuild is 0.5 ms a tick.
    let repath_ms = ns_to_ms(g3_tick_budget::g2::REPATH_NS_P50 as f64);
    let repair_ms = ns_to_ms(g3_tick_budget::g2::REPAIR_NS_PER_CRATER as f64) * craters_per_tick;
    // The CSR rebuild is already inside `fixed_ms` (it is charged on every tick,
    // craters or none), so it is not added a second time here.
    let p99_floor_ms = fixed_ms + repair_ms + per_unit_quiet_ms * units_from_mean;
    let max_cap = ((eff_p99_ms - p99_floor_ms) / repath_ms).max(0.0);
    // The floor under any usable cap: mean demand. G2 measured 9.45 repaths per
    // crater at 300 units, and the gate configuration lands one crater a tick,
    // so a cap below that serves less than the load produces for ever.
    let sustainable_cap_floor = (g3_tick_budget::g2::REPATHS_PER_CRATER_MILLI as f64 / 1_000.0)
        * (base.units as f64 / g3_tick_budget::g2::REPATHS_MEASURED_AT_UNITS as f64)
        * craters_per_tick;

    let affordable = units_from_mean;
    let kw_budget = affordable * KW_PER_UNIT_PLACEHOLDER;

    println!();
    println!("# step 5 - provisional per-map power budget. Every input named.");
    println!("budget\tG3'-a tick p99 budget\t{BUDGET_P99_MS} ms   (50% of the 50 ms tick)");
    println!("budget\tG3'-b mean tick budget\t{BUDGET_MEAN_MS} ms   (>=4x real time at 20 Hz)");
    println!("budget\treserve (PLACEHOLDER, owner at S2 exit)\t{RESERVE_PERCENT}%");
    println!("budget\teffective p99 after reserve\t{eff_p99_ms:.3} ms");
    println!("budget\teffective mean after reserve\t{eff_mean_ms:.3} ms");
    println!(
        "budget\tfixed per-tick term at 40 beacons\t{fixed_ms:.5} ms = {fixed_sim_own_ms:.5} sim's own + {charged_csr_ms:.5} G2's CSR rebuild (SUBSTITUTED)"
    );
    println!(
        "budget\t(the beacon term is inside that intercept and is not added again; it is unresolved at this spike's precision, bracket [{beacon_lo:.0}, {beacon_hi:.0}] ns/beacon)"
    );
    println!("budget\tunit-free crater cost at {gate_edits} edits/s\t{unit_free_edit_ms:.5} ms");
    println!(
        "budget\tcost of one unit at {gate_edits} edits/s (a0 + d*e)\t{:.1} ns = {per_unit_ms:.5} ms",
        ns_per_unit_quiet + d_unit_per_edit * gate_edits
    );
    println!("budget\tMEAN constraint -> affordable units world-wide\t{units_from_mean:.0}");
    println!(
        "budget\tP99 constraint -> max per-tick repath cap\t{max_cap:.0}   (floor {p99_floor_ms:.3} ms, repath {repath_ms:.3} ms each)"
    );
    println!(
        "budget\tSTABILITY floor on the cap (mean demand)\t{sustainable_cap_floor:.1}   -- a cap below this never drains the queue, whatever it costs per tick"
    );
    println!("budget\t--- the repath cap sweep, with the backlog that makes it readable ---");
    for p in &caps_sweep {
        println!(
            "budget\tcap={:>9}  mean={:.4} ms  p99={:.4} ms  backlog_mean={:.2}  backlog_final={:.0}  growth={:+.0}  served={:.3}{}",
            cap_name(p.cap),
            ns_to_ms(p.mean_ns),
            ns_to_ms(p.p99_ns),
            p.backlog_mean,
            p.backlog_final,
            p.backlog_growth,
            p.served_fraction,
            if p.sustainable() {
                ""
            } else {
                "  UNSUSTAINABLE"
            }
        );
    }
    println!("budget\tkW per unit (PLACEHOLDER, Tuning, section 19)\t{KW_PER_UNIT_PLACEHOLDER} kW");
    println!("budget\tPROVISIONAL PER-MAP POWER BUDGET\t{kw_budget:.0} kW");
    println!("budget\t--- the same arithmetic at other destruction rates ---");
    for e in [0.0f64, 10.0, 20.0, 40.0] {
        println!(
            "budget\taffordable units at {e} edits/s\t{:.0}",
            affordable_at(e)
        );
    }
    println!(
        "budget\t(with destruction OFF a unit costs {per_unit_quiet_ms:.6} ms, {:.0}x less)",
        per_unit_ms / per_unit_quiet_ms.max(1e-9)
    );
    println!();
    println!("# plan section 1's open question: the per-seat reading (1 200 units / 160 beacons)");
    println!(
        "per_seat\tmean={:.4} ms\tp99={:.4} ms",
        ns_to_ms(per_seat.mean_ns),
        ns_to_ms(per_seat.p99_ns)
    );

    // ---- JSON --------------------------------------------------------
    if let Some(path) = &json {
        let pts = |v: &[Point]| -> String {
            let parts: Vec<String> = v
                .iter()
                .map(|p| {
                    format!(
                        "{{\"label\":\"{}\",\"seats\":{},\"units\":{},\"beacons\":{},\"structures\":{},\"edits_per_s\":{},\"repath_cap\":\"{}\",\"repeats\":{},\"mean_ms\":{},\"p99_ms\":{},\"repath_backlog_mean\":{},\"repath_backlog_final\":{},\"repath_backlog_growth\":{},\"repath_served_fraction\":{},\"sustainable\":{},\"phases_ms\":{{{}}}}}",
                        p.label,
                        p.seats,
                        p.units,
                        p.beacons,
                        p.beacons * STRUCTURES_PER_BEACON,
                        p.edits,
                        cap_name(p.cap),
                        p.repeats,
                        j(ns_to_ms(p.mean_ns)),
                        j(ns_to_ms(p.p99_ns)),
                        j(p.backlog_mean),
                        j(p.backlog_final),
                        j(p.backlog_growth),
                        j(p.served_fraction),
                        p.sustainable(),
                        Phase::ALL
                            .iter()
                            .map(|ph| format!(
                                "\"{}\":{}",
                                ph.name(),
                                j(ns_to_ms(p.phase_mean_ns[ph.index()]))
                            ))
                            .collect::<Vec<_>>()
                            .join(",")
                    )
                })
                .collect();
            format!("[{}]", parts.join(","))
        };
        let mut o = String::new();
        o.push('{');
        o.push_str(&format!("\"os\":\"{}\",", std::env::consts::OS));
        o.push_str(&format!(
            "\"ticks_per_point\":{ticks},\"warmup\":{warmup},\"repeat\":{repeat},\"seats\":{seats},\"calibration_ps_per_iter\":{ps},\"overflow_checks\":{checks},\"pin\":\"{pin}\",\"burst_one_in_PLACEHOLDER\":{burst_one_in},"
        ));
        o.push_str(&format!("\"sweep_units_20edits\":{},", pts(&units_sweep)));
        o.push_str(&format!(
            "\"sweep_units_0edits\":{},",
            pts(&units_sweep_quiet)
        ));
        o.push_str(&format!("\"sweep_beacons\":{},", pts(&beacons_sweep)));
        o.push_str(&format!(
            "\"sweep_beacons_0edits\":{},",
            pts(&beacons_sweep_quiet)
        ));
        o.push_str(&format!("\"sweep_edits\":{},", pts(&edits_sweep)));
        o.push_str(&format!("\"sweep_repath_cap\":{},", pts(&caps_sweep)));
        o.push_str(&format!("\"gate\":{},", pts(std::slice::from_ref(&gate))));
        o.push_str(&format!(
            "\"per_seat_reading\":{},",
            pts(std::slice::from_ref(&per_seat))
        ));
        let fs = |v: &[f64]| -> String {
            format!(
                "[{}]",
                v.iter().map(|x| j(*x)).collect::<Vec<_>>().join(",")
            )
        };
        o.push_str(&format!(
            "\"fit\":{{\"model\":\"mean_tick_ns = fixed + a0*units + beta*beacons + c0*edits_per_s + d*units*edits_per_s\",\
             \"ns_per_unit_per_tick_20edits\":{},\"ns_per_unit_per_tick_0edits_a0\":{},\"ns_per_unit_per_edit_per_s_d\":{},\
             \"ns_per_beacon_per_tick_beta\":{},\"ns_per_beacon_per_tick_20edits\":{},\"ns_per_beacon_resolved\":{},\
             \"ns_per_beacon_bracket\":[{},{}],\"ns_per_beacon_note\":\"the consecutive-pair slopes do not agree in sign; publish an upper bound, not a value. The term cancels out of the power budget: it is inside fixed_overhead_ms and is not added again.\",\
             \"ns_per_crater_pathing_consequence_at_300_units_SUBSTITUTED\":{},\"ns_per_crater_repair_unit_free_SUBSTITUTED\":{},\"ns_per_crater_repath_per_unit_SUBSTITUTED\":{},\
             \"ns_per_voxel_edit_chunk_store_MEASURED\":{},\"ns_per_voxel_edit_note\":\"the first three are G2's repath-and-repair CONSEQUENCE of a crater, not the cost of writing it; the chunk store's own per-crater cost is the last one, measured in the voxels phase\",\
             \"ns_per_decision_tick_per_seat\":{},\
             \"fixed_overhead_ms\":{},\"fixed_overhead_ms_sim_own\":{},\"charged_csr_rebuild_ms_from_G2\":{},\
             \"r2_units\":{},\"r2_units_quiet\":{},\"r2_beacons_quiet\":{},\"r2_beacons_20edits\":{},\"r2_edits\":{},\
             \"superlinear_share_at_600_units_0edits\":{},\"unit_cost_shape\":\"{}\",\
             \"superlinear_share_at_600_units_broadphase\":{},\"broadphase_cost_shape\":\"{}\",\
             \"superlinear_share_at_600_units_20edits_substitute_check\":{},\"unit_cost_shape_20edits\":\"{}\",\
             \"quiet_linear_residuals_ns\":{},\"quiet_marginal_ns_per_unit\":{},\"broadphase_marginal_ns_per_unit\":{},\
             \"shape_note\":\"the verdict is taken from the 0-edits sweep and from the broadphase phase inside it; the 20-edits sweep is ~98% the G2 charge, which is linear in units by construction and so cannot answer plan 3.4's question\"}},",
            j(ns_per_unit),
            j(ns_per_unit_quiet),
            j(d_unit_per_edit),
            j(ns_per_beacon),
            j(ns_per_beacon_loud),
            beacon_resolved,
            j(beacon_lo),
            j(beacon_hi),
            j(ns_per_crater_pathing_consequence_at_300),
            j(ns_per_crater_repair_unit_free),
            j(ns_per_crater_repath_consequence_per_unit),
            j(ns_per_voxel_edit_chunk_store),
            j(ns_per_decision_per_seat),
            j(fixed_ms),
            j(fixed_sim_own_ms),
            j(charged_csr_ms),
            j(unit_r2),
            j(unit_r2_quiet),
            j(beacon_r2),
            j(beacon_r2_loud),
            j(edit_r2),
            j(superlinear_share_quiet),
            shape_name(superlinear_share_quiet),
            j(superlinear_share_broadphase),
            shape_name(superlinear_share_broadphase),
            j(superlinear_share_loud),
            shape_name(superlinear_share_loud),
            fs(&quiet_residuals),
            fs(&quiet_marginals),
            fs(&broadphase_marginals),
        ));
        o.push_str(&format!(
            "\"power_budget\":{{\"budget_p99_ms\":{BUDGET_P99_MS},\"budget_mean_ms\":{BUDGET_MEAN_MS},\"reserve_percent_PLACEHOLDER\":{RESERVE_PERCENT},\"effective_p99_ms\":{},\"effective_mean_ms\":{},\"fixed_overhead_ms\":{},\"fixed_overhead_ms_sim_own\":{},\"charged_csr_rebuild_ms_from_G2\":{},\"unit_free_crater_cost_ms_at_20_per_s\":{},\"per_unit_ms_at_20_edits\":{},\"per_unit_ms_at_0_edits\":{},\"affordable_units\":{},\"affordable_units_at_0_edits\":{},\"affordable_units_at_10_edits\":{},\"affordable_units_at_40_edits\":{},\"max_repath_cap_under_p99\":{},\"sustainable_repath_cap_floor\":{},\"p99_floor_ms\":{},\"repath_ms_each_from_G2\":{},\"kw_per_unit_PLACEHOLDER\":{KW_PER_UNIT_PLACEHOLDER},\"per_map_kw_budget\":{}}}",
            j(eff_p99_ms),
            j(eff_mean_ms),
            j(fixed_ms),
            j(fixed_sim_own_ms),
            j(charged_csr_ms),
            j(unit_free_edit_ms),
            j(per_unit_ms),
            j(per_unit_quiet_ms),
            j(units_from_mean),
            j(units_from_mean_quiet),
            j(affordable_at(10.0)),
            j(affordable_at(40.0)),
            j(max_cap),
            j(sustainable_cap_floor),
            j(p99_floor_ms),
            j(repath_ms),
            j(kw_budget)
        ));
        o.push('}');
        if let Some(dir) = std::path::Path::new(path).parent()
            && !dir.as_os_str().is_empty()
        {
            std::fs::create_dir_all(dir).expect("create output directory");
        }
        let mut f = std::fs::File::create(path).expect("create cost json");
        f.write_all(o.as_bytes()).expect("write cost json");
        f.sync_all().expect("sync cost json");
        println!("json\t{path}");
    }
}
