// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The pathing bench: what a query costs, what a repair costs, and what the
//! graph is made of.
//!
//! # It counts work; it does not read a clock, and that is deliberate
//!
//! Skeleton plan section 3 (T7) asks this bench for "repath p99, estimate p99,
//! **the endpoint-sweep share of an estimate query**, and **steps and
//! components** rather than cell counts". Three of those four are here. The two
//! *wall-time* p99s are not, for two reasons that point the same way:
//!
//! 1. **The sim may not read a clock.** AGENTS.md section 4.5 denies `Instant`
//!    and `SystemTime` to every crate outside the wall, and `cargo xtask
//!    clippy` lints this package `--all-targets`, so a bench target inside
//!    `pharmakos-sim` cannot take a time measurement without an `#[allow]` on a
//!    determinism lint — which section 5 calls a contract change wearing a
//!    disguise. The mesher's own p99 alarm lives in a *walled* crate, where the
//!    wall lifts exactly that lint; the sim is on the other side of that wall by
//!    design, because it is the crate the wall exists to protect.
//! 2. **A time figure could not be compared across the matrix anyway.**
//!    `tests/golden/README.md` says it out loud: a figure that cannot vary
//!    across platforms must not be compared across them, and one that can vary
//!    is not a golden. Expansions *are* identical on all three operating
//!    systems, which makes them a regression alarm a matrix can actually read.
//!
//! So the p99s below are **expansions per query**, the platform-free half of the
//! same measurement: G2's milliseconds are this number times a machine's cost
//! per expansion, and S3's certification — which is where the millisecond gates
//! live, since AGENTS.md section 9 item 11 puts budgets with the gates that set
//! them — measures the second half on the machine it certifies. The
//! endpoint-sweep share, the one number the plan says nobody else produces, is
//! exact either way: it is a ratio of expansions.
//!
//! Nothing here gates. Every line is a `::notice::`, and the two assertions are
//! structural alarms with wide bands: an endpoint share that collapsed, or a
//! graph that lost its giant component, means something changed shape.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules — not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::path::PathBuf;

use pharmakos_sim::chunks::ChunkDigests;
use pharmakos_sim::pathing::MAX_ROUTE_NODES;
use pharmakos_sim::pathing::clusters::Clusters;
use pharmakos_sim::pathing::estimate::{Fog, Speed, estimate};
use pharmakos_sim::pathing::repair::{Repairer, repair};
use pharmakos_sim::pathing::route::route;
use pharmakos_sim::pathing::search::Scratch;
use pharmakos_sim::pathing::surface::{Node, StepCosts, Surface};
use pharmakos_sim::voxels::VoxelStore;
use pharmakos_sim::{RulesTable, mapgen};

/// The seed the bench runs on: the determinism harness's own.
const SEED: u64 = pharmakos_sim::DETERMINISM_MATCH_SEED;

/// Queries per distribution.
const QUERIES: u32 = 120;

/// Craters the repair half applies.
const CRATERS: u32 = 24;

fn repo_root() -> PathBuf {
    let here = PathBuf::from(".");
    if here.join("rules").join("rules.v1.json").is_file() {
        return here;
    }
    PathBuf::from("..").join("..")
}

fn percentile(sorted: &[u64], p: u64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let n = u64::try_from(sorted.len()).expect("fits");
    let index = ((p * (n - 1)) + 50).div_euclid(100);
    sorted[usize::try_from(index.min(n - 1)).expect("clamped")]
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        pharmakos_sim::math::random::mix64(self.0)
    }

    fn below(&mut self, limit: i32) -> i32 {
        i32::try_from(self.next() % u64::try_from(limit.max(1)).expect("positive")).expect("below")
    }
}

/// Everything the bench measures over, in one piece.
struct Bench {
    store: VoxelStore,
    digests: ChunkDigests,
    surface: Surface,
    clusters: Clusters,
    scratch: Scratch,
    repairer: Repairer,
    cluster: i32,
    speed: Speed,
}

#[test]
fn the_pathing_bench_reports_its_numbers() {
    let rules = RulesTable::load(&repo_root().join("rules").join("rules.v1.json"))
        .expect("the committed rules table loads");
    let generated = mapgen::generate(SEED, &rules, 3).expect("a map");
    let mut store: VoxelStore = generated.voxels;
    let mut digests = ChunkDigests::new(store.chunk_count());
    store.settle_all(&mut digests);
    let surface = Surface::new(&store, StepCosts::from_rules(&rules)).expect("a surface");
    let cluster = rules.hpa_cluster_voxels();
    let mut scratch = Scratch::for_map(&surface, cluster).expect("a scratch");
    let clusters = Clusters::new(&surface, cluster, &mut scratch).expect("a decomposition");
    let repairer = Repairer::new(clusters.cluster_count());
    let mut bench = Bench {
        store,
        digests,
        surface,
        clusters,
        scratch,
        repairer,
        cluster,
        speed: Speed::commander(&rules),
    };

    // The graph, in steps and components rather than in cells. G2 section 9.9's
    // lesson: on a 1-Lipschitz map every column is walkable by construction, so
    // a walkable-column count measures the generator's clamp. What destruction
    // removes is *steps*, and what seals a walker in is a *component*.
    let steps_before = bench.surface.directed_step_count();
    let (components_before, largest_before) = components(&bench.surface, &bench.clusters);
    println!(
        "::notice::graph: {} columns, {} walkable, {steps_before} directed steps, \
         {components_before} components (largest {largest_before}), {} transition nodes, \
         {} directed abstract edges, cluster {cluster}",
        bench.surface.node_count(),
        bench.surface.walkable_count(),
        bench.clusters.transition_count(),
        bench.clusters.abstract_edge_count()
    );

    let share = bench_estimates(&mut bench);
    bench_repaths(&mut bench);
    bench_repairs(&mut bench);

    let steps_after = bench.surface.directed_step_count();
    let (components_after, largest_after) = components(&bench.surface, &bench.clusters);
    println!(
        "::notice::after {CRATERS} craters: {steps_after} directed steps ({} removed of \
         {steps_before}), {components_after} components (largest {largest_after}, was \
         {largest_before})",
        steps_before.saturating_sub(steps_after)
    );

    // Two structural alarms, wide on purpose. Neither is a gate.
    assert!(
        share >= 50,
        "the endpoint sweep is only {share} % of a query; at cluster 32 G2 measured 91 %, so a \
         collapse means the abstract graph or the endpoint insertion changed shape"
    );
    assert!(
        largest_after.saturating_mul(2) >= u64::from(bench.surface.walkable_count()),
        "the largest component is {largest_after} of {} walkable columns; the map shattered",
        bench.surface.walkable_count()
    );
}

/// The estimate query's cost, and the number S3 needs: the endpoint-sweep share,
/// as a percentage.
fn bench_estimates(bench: &mut Bench) -> u64 {
    let mut rng = Rng(0x8E1C_0001);
    let mut totals: Vec<u64> = Vec::new();
    let mut endpoints: Vec<u64> = Vec::new();
    let mut abstracts: Vec<u64> = Vec::new();
    let mut legs: Vec<u64> = Vec::new();
    let mut taken = 0;
    while taken < QUERIES {
        let Some((start, goal)) = pair(&bench.surface, &bench.clusters, &mut rng) else {
            continue;
        };
        bench.scratch.reset_counters();
        let Some(answer) = estimate(
            &bench.surface,
            &bench.clusters,
            &mut bench.scratch,
            start,
            goal,
            Fog::Clear,
            bench.speed,
        ) else {
            continue;
        };
        totals.push(bench.scratch.expansions() + bench.scratch.abstract_expansions());
        endpoints.push(bench.scratch.endpoint_expansions());
        abstracts.push(bench.scratch.abstract_expansions());
        legs.push(u64::from(answer.legs));
        taken += 1;
    }
    totals.sort_unstable();
    endpoints.sort_unstable();
    abstracts.sort_unstable();
    legs.sort_unstable();
    let endpoint_total: u64 = endpoints.iter().copied().fold(0, u64::saturating_add);
    let query_total: u64 = totals.iter().copied().fold(0, u64::saturating_add);
    let share = if query_total == 0 {
        0
    } else {
        endpoint_total.saturating_mul(100).div_euclid(query_total)
    };
    println!(
        "::notice::estimate expansions: p50 {}, p90 {}, p99 {}, max {}; endpoint sweep p50 {}, \
         abstract p50 {}, legs p50 {}",
        percentile(&totals, 50),
        percentile(&totals, 90),
        percentile(&totals, 99),
        totals.last().copied().unwrap_or(0),
        percentile(&endpoints, 50),
        percentile(&abstracts, 50),
        percentile(&legs, 50)
    );
    println!(
        "::notice::endpoint-sweep share of an estimate query: {share} % (G2 measured 91 % at \
         cluster 32, which is why the lever on the estimate budget is the sweep and not a second \
         abstract level)"
    );
    share
}

/// A full route's cost: abstract search, per-leg refinement and smoothing.
fn bench_repaths(bench: &mut Bench) {
    let mut rng = Rng(0x8E1C_0002);
    let mut repaths: Vec<u64> = Vec::new();
    let mut nodes: Vec<u64> = Vec::new();
    let mut buffer: Vec<Node> = Vec::with_capacity(usize::try_from(MAX_ROUTE_NODES).expect("fits"));
    let mut taken = 0;
    while taken < QUERIES {
        let Some((start, goal)) = pair(&bench.surface, &bench.clusters, &mut rng) else {
            continue;
        };
        bench.scratch.reset_counters();
        if route(
            &bench.surface,
            &bench.clusters,
            &mut bench.scratch,
            start,
            goal,
            &mut buffer,
        )
        .is_none()
        {
            continue;
        }
        repaths.push(bench.scratch.expansions() + bench.scratch.abstract_expansions());
        nodes.push(u64::try_from(buffer.len()).expect("fits"));
        taken += 1;
    }
    repaths.sort_unstable();
    nodes.sort_unstable();
    println!(
        "::notice::repath expansions: p50 {}, p90 {}, p99 {}, max {}; route nodes p50 {}, p99 {}",
        percentile(&repaths, 50),
        percentile(&repaths, 90),
        percentile(&repaths, 99),
        repaths.last().copied().unwrap_or(0),
        percentile(&nodes, 50),
        percentile(&nodes, 99)
    );
}

/// What a crater costs the graph.
fn bench_repairs(bench: &mut Bench) {
    let mut repairs: Vec<u64> = Vec::new();
    let mut dirty_total: u64 = 0;
    let mut n: u32 = 0;
    while n < CRATERS {
        let x = i32::try_from((n.wrapping_mul(97) + 40) % 360).expect("fits") + 12;
        let y = i32::try_from((n.wrapping_mul(53) + 24) % 360).expect("fits") + 12;
        let z = bench.store.top_solid_z(x, y).unwrap_or(0);
        let mut touched: Vec<u32> = Vec::new();
        bench.store.crater([x, y, z], 3, &mut touched);
        bench.store.settle(&mut bench.digests);
        let settled: Vec<u32> = bench.store.settled().to_vec();
        for chunk in settled {
            bench.surface.refresh_chunk(&bench.store, chunk);
            if let Some(origin) = bench.store.chunk_origin(chunk)
                && let Some(index) = bench.clusters.cluster_index(
                    origin[0].div_euclid(bench.cluster),
                    origin[1].div_euclid(bench.cluster),
                )
            {
                bench.repairer.mark(u16::try_from(index).expect("fits"));
            }
        }
        bench.scratch.reset_counters();
        let report = repair(
            &bench.surface,
            &mut bench.clusters,
            &mut bench.scratch,
            &mut bench.repairer,
        );
        repairs.push(bench.scratch.expansions());
        dirty_total += u64::from(report.dirty);
        n += 1;
    }
    repairs.sort_unstable();
    println!(
        "::notice::repair expansions per crater: p50 {}, p90 {}, p99 {}, max {}; {dirty_total} \
         dirty clusters over {CRATERS} craters",
        percentile(&repairs, 50),
        percentile(&repairs, 90),
        percentile(&repairs, 99),
        repairs.last().copied().unwrap_or(0)
    );
}

/// One connected pair of walkable columns.
fn pair(surface: &Surface, clusters: &Clusters, rng: &mut Rng) -> Option<(Node, Node)> {
    let extent = surface.size();
    let a = surface.node_of(rng.below(extent[0]), rng.below(extent[1]))?;
    let b = surface.node_of(rng.below(extent[0]), rng.below(extent[1]))?;
    if a == b || !clusters.connected(surface, a, b) {
        return None;
    }
    Some((a, b))
}

/// How many components the walkable set falls into, and how big the largest is.
fn components(surface: &Surface, clusters: &Clusters) -> (u64, u64) {
    let mut labels: Vec<u32> =
        Vec::with_capacity(usize::try_from(surface.node_count()).unwrap_or(0));
    let mut node: Node = 0;
    while node < surface.node_count() {
        if surface.walkable(node) {
            labels.push(clusters.component_label(node));
        }
        node += 1;
    }
    labels.sort_unstable();
    let mut distinct: u64 = 0;
    let mut largest: u64 = 0;
    let mut run: u64 = 0;
    let mut previous = u32::MAX;
    for label in labels {
        if label == previous {
            run += 1;
        } else {
            largest = largest.max(run);
            distinct += 1;
            run = 1;
            previous = label;
        }
    }
    largest = largest.max(run);
    (distinct, largest)
}
