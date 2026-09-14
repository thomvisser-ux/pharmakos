// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later
#![deny(clippy::float_arithmetic, clippy::as_conversions)]
//! Ground truth and accuracy: plan §3 steps 2, 3, 6 and 7.
//!
//! **Wall-clock time lives here and in `bench.rs`, and nowhere else** (plan §5,
//! last row). This binary barely needs it — only to report how long it took —
//! because everything it measures is a cost ratio, not a duration.
//!
//! * **step 2 — exactness.** A plain low-level A\* against an exhaustive
//!   Dijkstra on 1 000 pairs. *"costs must match exactly, or the heuristic is
//!   not admissible and every accuracy number afterwards is garbage."* The
//!   mismatch count must be zero, and it is asserted, not merely printed.
//! * **step 3 — path quality.** HPA\*'s excess cost over optimal, on pairs
//!   stratified by straight-line distance into the plan's four bands. HPA\* is
//!   not optimal by construction; the estimate inherits this error *and adds
//!   its own*, so it is reported separately from the estimate's.
//! * **step 6 — the P3 gate.** `estimate_ticks` against `walked_ticks` (what a
//!   unit stepped along the refined, smoothed path actually experiences) and
//!   against `optimal_ticks` (the diagnostic baseline). Signed, so a systematic
//!   bias — which one calibration constant fixes — is distinguishable from
//!   variance, which it does not.
//! * **step 7 — fog.** The same measurement with a fog mask over part of every
//!   route and the ×1.5 rule applied. Recorded separately: the gate says "on
//!   static terrain", so this does not decide it, but section 13 draws fogged
//!   legs dashed and S3 needs to know what it is dealing with.

use std::io::Write as _;
use std::time::Instant;

use g2_pathing::astar::{Bound, Scratch, astar, dijkstra_all};
use g2_pathing::clusters::Clusters;
use g2_pathing::cost::{assert_cost_headroom, path_cost, ticks_for_cost, walk_ticks};
use g2_pathing::estimate::estimate;
use g2_pathing::hash::Rng;
use g2_pathing::hpa::{HpaScratch, path};
use g2_pathing::json::Obj;
use g2_pathing::pairs::{BAND_NAMES, band_counts, stratified};
use g2_pathing::stats::{dist_abs_pct, dist_pct, ms_ns, pct, permille_as_pct, rel_permille};
use g2_pathing::world::{NODES, W, World, coord_of, node_of};
use g2_pathing::{id32, ix};

/// Fog blocks are 24 cells on a side and cover half the map in a checkerboard,
/// so every route of any length crosses several known/unknown boundaries —
/// "a fog mask over part of each route" (plan §3 step 7) rather than a mask that
/// happens to miss short routes entirely.
const FOG_BLOCK: i32 = 24;

#[derive(Clone, Debug)]
struct Args {
    pairs: usize,
    exactness: usize,
    cluster: i32,
    seed: u64,
    json: Option<String>,
    assert_p90_pct: Option<i64>,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            pairs: 2_000,
            exactness: 1_000,
            cluster: 32,
            seed: 20_260_913,
            json: None,
            assert_p90_pct: None,
        }
    }
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
        match key.as_str() {
            "--pairs" => a.pairs = take(i, &key).parse().expect("--pairs"),
            "--exactness" => a.exactness = take(i, &key).parse().expect("--exactness"),
            "--cluster" => a.cluster = take(i, &key).parse().expect("--cluster"),
            "--seed" => a.seed = take(i, &key).parse().expect("--seed"),
            "--json" => a.json = Some(take(i, &key)),
            "--assert-p90-pct" => {
                a.assert_p90_pct = Some(take(i, &key).parse().expect("--assert-p90-pct"));
            }
            other => panic!("unknown flag {other}"),
        }
        i += 2;
    }
    assert!(
        [16, 32, 64].contains(&a.cluster),
        "--cluster must be 16, 32 or 64"
    );
    a
}

fn main() {
    let args = parse_args();
    let t_total = Instant::now();
    let world = World::generate(args.seed);
    // Plan §5 "integer overflow": the map's maximum path cost, computed and
    // asserted at start-up, before anything accumulates one.
    let cost_bound = assert_cost_headroom(world.walkable_count());
    let mut sc = Scratch::new();
    let cl = Clusters::build(&world, args.cluster, &mut sc);
    let mut hs = HpaScratch::new();

    // ---- step 2: A* against exhaustive Dijkstra ---------------------------
    let mut rng = Rng::new(args.seed ^ 0x6E0_1234);
    let sources = 25usize;
    let per_source = args.exactness.div_ceil(sources);
    let mut compared = 0usize;
    let mut mismatches = 0usize;
    let mut unreachable_agreements = 0usize;
    let mut route = Vec::new();
    let mut sources_used = 0usize;
    // Plan §3 step 2 asks for 1 000 *cost comparisons*. An unreachable draw is
    // still checked — A* and Dijkstra must agree that there is no route — but it
    // is not one of them, so it does not consume the quota, and the loop keeps
    // going until the quota is actually filled. Counting unreachable draws
    // toward it left the run comparing 889 pairs while reporting the plan's
    // 1 000 as satisfied.
    //
    // Two caps keep it bounded. A source that lands in one of the map's small
    // components has almost no reachable destination at all, so each source
    // gives up after `per_source` unreachable draws rather than paying for
    // thousands of exhaustive failed searches; and the source loop itself stops
    // after `sources * 8` attempts, at which point the gate below fails and says
    // how far short it fell.
    while compared < args.exactness && sources_used < sources * 8 {
        sources_used += 1;
        let src = loop {
            let id = node_of(rng.below(W), rng.below(W)).expect("in bounds");
            if world.walkable(id) {
                break id;
            }
        };
        let dist = dijkstra_all(&world, &mut sc, src);
        let mut taken = 0usize;
        let mut misses = 0usize;
        while taken < per_source && compared < args.exactness && misses < per_source {
            let dst = node_of(rng.below(W), rng.below(W)).expect("in bounds");
            if !world.walkable(dst) || dst == src {
                continue;
            }
            let want = dist[ix(dst)];
            let got = astar(&world, Bound::Whole, &mut sc, src, dst, &mut route);
            if want == i32::MAX {
                misses += 1;
                if got.is_some() {
                    mismatches += 1;
                } else {
                    unreachable_agreements += 1;
                }
                continue;
            }
            taken += 1;
            compared += 1;
            if got != Some(want) || path_cost(&world, &route) != Some(want) {
                mismatches += 1;
            }
        }
    }

    // ---- steps 3, 6, 7 -----------------------------------------------------
    let qs = stratified(&world, &cl, args.pairs, args.seed ^ 0xACC0_1234);
    let fog = fog_mask();

    let mut hpa_excess: Vec<i64> = Vec::new();
    let mut abs_excess: Vec<i64> = Vec::new();
    let mut err_walked: Vec<i64> = Vec::new();
    let mut err_optimal: Vec<i64> = Vec::new();
    let mut err_fogged: Vec<i64> = Vec::new();
    let mut fog_inflation: Vec<i64> = Vec::new();
    let mut hpa_excess_band: [Vec<i64>; 4] = core::array::from_fn(|_| Vec::new());
    let mut err_walked_band: [Vec<i64>; 4] = core::array::from_fn(|_| Vec::new());
    let mut optimal_missing = 0usize;
    let mut hpa_missing = 0usize;
    let mut smoothing_gain: Vec<i64> = Vec::new();

    for p in &qs {
        let Some(opt) = astar(&world, Bound::Whole, &mut sc, p.a, p.b, &mut route) else {
            optimal_missing += 1;
            continue;
        };
        let Some(hpa_cost) = path(&world, &cl, &mut hs, p.a, p.b, &mut route) else {
            hpa_missing += 1;
            continue;
        };
        let Some(est) = estimate(&world, &cl, &mut hs, p.a, p.b, None) else {
            hpa_missing += 1;
            continue;
        };
        let walked = walk_ticks(&world, &route).expect("the refined path is walkable");
        if walked == 0 || opt == 0 {
            continue;
        }
        let optimal_ticks = ticks_for_cost(opt);

        hpa_excess.push(rel_permille(i64::from(hpa_cost), i64::from(opt)));
        hpa_excess_band[p.band].push(rel_permille(i64::from(hpa_cost), i64::from(opt)));
        abs_excess.push(rel_permille(i64::from(est.cost), i64::from(opt)));
        smoothing_gain.push(rel_permille(i64::from(hpa_cost), i64::from(est.cost)));

        let e = rel_permille(i64::from(est.ticks), i64::from(walked));
        err_walked.push(e);
        err_walked_band[p.band].push(e);
        err_optimal.push(rel_permille(i64::from(est.ticks), i64::from(optimal_ticks)));

        if let Some(fe) = estimate(&world, &cl, &mut hs, p.a, p.b, Some(&fog)) {
            err_fogged.push(rel_permille(i64::from(fe.ticks), i64::from(walked)));
            fog_inflation.push(rel_permille(i64::from(fe.ticks), i64::from(est.ticks)));
        }
    }

    let p90_abs_walked = {
        let mut v: Vec<i64> = err_walked.iter().map(|e| e.abs()).collect();
        v.sort_unstable();
        pct(&v, 90)
    };

    // ---- report ------------------------------------------------------------
    let mut o = Obj::new();
    o.str("spike", "g2-pathing")
        .str("binary", "accuracy")
        .int("seed", i64::try_from(args.seed).unwrap_or(-1))
        .int("cluster_size", i64::from(args.cluster))
        .int("pairs_requested", i64::try_from(args.pairs).unwrap_or(-1))
        .int(
            "pairs_measured",
            i64::try_from(err_walked.len()).unwrap_or(-1),
        )
        .int(
            "optimal_missing",
            i64::try_from(optimal_missing).unwrap_or(-1),
        )
        .int("hpa_missing", i64::try_from(hpa_missing).unwrap_or(-1))
        .int(
            "walkable_cells",
            i64::try_from(world.walkable_count()).unwrap_or(-1),
        )
        .int("max_path_cost_bound", cost_bound);

    let mut ex = Obj::new();
    ex.int("pairs_compared", i64::try_from(compared).unwrap_or(-1))
        .int(
            "pairs_requested",
            i64::try_from(args.exactness).unwrap_or(-1),
        )
        .int("mismatches", i64::try_from(mismatches).unwrap_or(-1))
        .int(
            "unreachable_agreements",
            i64::try_from(unreachable_agreements).unwrap_or(-1),
        )
        .int("sources_used", i64::try_from(sources_used).unwrap_or(-1));
    o.raw("step2_astar_vs_dijkstra", &ex.render());

    let counts = band_counts(&qs);
    let mut q = Obj::new();
    q.raw("hpa_excess_pct", &dist_pct(&mut hpa_excess.clone()))
        .raw("abstract_excess_pct", &dist_pct(&mut abs_excess.clone()))
        .raw("smoothing_gain_pct", &dist_pct(&mut smoothing_gain.clone()))
        .raw(
            "band_counts",
            &format!("[{},{},{},{}]", counts[0], counts[1], counts[2], counts[3]),
        );
    for (i, name) in BAND_NAMES.iter().enumerate() {
        q.raw(
            &format!("hpa_excess_{name}_pct"),
            &dist_pct(&mut hpa_excess_band[i]),
        );
    }
    o.raw("step3_path_quality", &q.render());

    let mut p6 = Obj::new();
    p6.raw("vs_walked_signed_pct", &dist_pct(&mut err_walked.clone()))
        .raw("vs_walked_abs_pct", &dist_abs_pct(&err_walked))
        .raw("vs_optimal_signed_pct", &dist_pct(&mut err_optimal.clone()))
        .raw("vs_optimal_abs_pct", &dist_abs_pct(&err_optimal))
        .raw("p90_abs_vs_walked_pct", &permille_as_pct(p90_abs_walked));
    for (i, name) in BAND_NAMES.iter().enumerate() {
        p6.raw(
            &format!("vs_walked_{name}_pct"),
            &dist_pct(&mut err_walked_band[i]),
        );
    }
    o.raw("step6_estimate_accuracy", &p6.render());

    let mut p7 = Obj::new();
    p7.int("fog_block_cells", i64::from(FOG_BLOCK))
        .int(
            "fogged_cells",
            i64::try_from(fog.iter().filter(|f| **f).count()).unwrap_or(-1),
        )
        .raw("vs_walked_signed_pct", &dist_pct(&mut err_fogged.clone()))
        .raw("vs_walked_abs_pct", &dist_abs_pct(&err_fogged))
        .raw("inflation_over_clear_pct", &dist_pct(&mut fog_inflation));
    o.raw("step7_fog", &p7.render());

    o.raw(
        "total_wall_ms",
        &ms_ns(i64::try_from(t_total.elapsed().as_nanos()).unwrap_or(i64::MAX)),
    );

    let text = o.render();
    println!("{text}");
    if let Some(p) = &args.json {
        if let Some(dir) = std::path::Path::new(p).parent()
            && !dir.as_os_str().is_empty()
        {
            std::fs::create_dir_all(dir).expect("create output directory");
        }
        let mut f = std::fs::File::create(p).expect("create output file");
        f.write_all(text.as_bytes()).expect("write output file");
    }

    // ---- gates -------------------------------------------------------------
    let mut fails: Vec<String> = Vec::new();
    if mismatches != 0 {
        fails.push(format!(
            "step 2: {mismatches} A*/Dijkstra mismatches — the heuristic is not admissible \
             and every number after this one is garbage"
        ));
    }
    if compared < args.exactness {
        fails.push(format!(
            "step 2: only {compared} cost comparisons, and plan §3 step 2 wants {}",
            args.exactness
        ));
    }
    if let Some(limit) = args.assert_p90_pct
        && p90_abs_walked > limit * 10
    {
        fails.push(format!(
            "P3-b: |estimate error| p90 vs walked is {}% > {limit}%",
            permille_as_pct(p90_abs_walked)
        ));
    }
    if fails.is_empty() {
        eprintln!(
            "accuracy ok: {compared} exact pairs, 0 mismatches; |error| p90 vs walked {}%",
            permille_as_pct(p90_abs_walked)
        );
    } else {
        for f in &fails {
            eprintln!("FAIL: {f}");
        }
        std::process::exit(1);
    }
}

/// The fog mask: alternating [`FOG_BLOCK`]-cell blocks, so any route of more
/// than a block crosses a known/unknown boundary.
fn fog_mask() -> Vec<bool> {
    let mut m = vec![false; NODES];
    for (i, slot) in m.iter_mut().enumerate() {
        let (x, z) = coord_of(id32(i));
        *slot = (x / FOG_BLOCK + z / FOG_BLOCK) % 2 == 0;
    }
    m
}
