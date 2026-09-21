// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! HPA\* and the travel estimator: the acceptance suite, and the producer of
//! `tests/golden/pathing/expected.path-hashes.txt`.
//!
//! This file also **produces** `<target>/golden/pathing/actual.path-hashes.txt`,
//! which `cargo xtask ci`'s `golden` step compares against the committed
//! golden. The `test` step runs before `golden`, so the fresh output is always
//! there by the time the comparison happens.
//!
//! The acceptance lines skeleton plan section 3 (T7) names are the tests
//! spelled after them:
//!
//! * the path-hash golden, over the pristine map **and** after crater repairs;
//! * three repair tests — repair equals a from-scratch build, equals the
//!   conservative all-neighbours set, and holds at every cluster size;
//! * the estimator's **signed** error, never negative at any percentile;
//! * the repath cap's sustainability over a full 8-minute segment at cap 16,
//!   with a `sustainable` boolean in the output;
//! * `no_path_is_answered_by_the_oracle`, with a cost assertion.
//!
//! The bench the same section asks for is `tests/pathing_bench.rs`, which
//! reports in **counted work** rather than in milliseconds, and says there why.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules — not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording), and that covers an out-of-bounds index in an assertion for the same reason: these are panic lints, not determinism lints, and AGENTS.md §5's ban is on the latter."
)]

use std::fmt::Write as _;
use std::path::PathBuf;

use pharmakos_sim::chunks::ChunkDigests;
use pharmakos_sim::encoding::hex;
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::math::quantity::TICK_HZ;
use pharmakos_sim::pathing::MAX_ROUTE_NODES;
use pharmakos_sim::pathing::clusters::Clusters;
use pharmakos_sim::pathing::estimate::{Fog, Speed, estimate, ticks_for_cost};
use pharmakos_sim::pathing::repair::{Repairer, conservative_affected, repair};
use pharmakos_sim::pathing::route::route;
use pharmakos_sim::pathing::router::WalkState;
use pharmakos_sim::pathing::search::{Bound, Scratch, astar, astar_with_order, dijkstra_bounded};
use pharmakos_sim::pathing::surface::{DEFAULT_ORDER, Node, StepCosts, Surface};
use pharmakos_sim::tables::UnitKind;
use pharmakos_sim::voxels::{Material, VoxelEdit, VoxelStore};
use pharmakos_sim::{MatchSettings, RulesTable, World, WorldConfig, mapgen};

/// The seed every fixture in this file runs on: the determinism harness's own,
/// so a map that moved shows up in one place rather than two.
const SEED: u64 = pharmakos_sim::DETERMINISM_MATCH_SEED;

/// Seats the fixtures occupy — a full v1 match, so every spawn zone the map
/// carries is realised.
const SEATS: u32 = 3;

/// The cluster edge the decided configuration uses (item 58).
const CLUSTER: i32 = 32;

/// How many route pairs the golden pins, per phase.
const GOLDEN_PAIRS: u32 = 48;

/// How many craters the golden's repaired phase applies.
const GOLDEN_CRATERS: u32 = 24;

fn repo_root() -> PathBuf {
    let here = PathBuf::from(".");
    if here.join("rules").join("rules.v1.json").is_file() {
        return here;
    }
    PathBuf::from("..").join("..")
}

/// The cargo target directory: `<target>/<profile>/deps/<test exe>`.
fn target_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let deps = exe.parent()?;
    let profile = deps.parent()?;
    Some(profile.parent()?.to_path_buf())
}

fn rules() -> RulesTable {
    RulesTable::load(&repo_root().join("rules").join("rules.v1.json"))
        .expect("the committed rules table loads")
}

/// A map, its surface graph, its decomposition and one scratch — everything a
/// query needs, without a [`World`] around it.
struct Fixture {
    rules: RulesTable,
    store: VoxelStore,
    digests: ChunkDigests,
    surface: Surface,
    clusters: Clusters,
    scratch: Scratch,
    repairer: Repairer,
}

impl Fixture {
    fn build(seed: u64, cluster: i32) -> Fixture {
        let rules = rules();
        let generated =
            mapgen::generate(seed, &rules, SEATS).expect("the committed table makes a map");
        let mut store = generated.voxels;
        let mut digests = ChunkDigests::new(store.chunk_count());
        store.settle_all(&mut digests);
        let surface = Surface::new(&store, StepCosts::from_rules(&rules)).expect("a surface");
        let mut scratch = Scratch::for_map(&surface, cluster).expect("a scratch");
        let clusters = Clusters::new(&surface, cluster, &mut scratch).expect("a decomposition");
        let repairer = Repairer::new(clusters.cluster_count());
        Fixture {
            rules,
            store,
            digests,
            surface,
            clusters,
            scratch,
            repairer,
        }
    }

    /// Carve one crater, refresh the columns it moved and mark its clusters,
    /// exactly as [`World`]'s voxel phase does.
    fn crater(&mut self, x: i32, y: i32, radius: i32) -> u32 {
        let z = self.store.top_solid_z(x, y).unwrap_or(0);
        let mut touched: Vec<u32> = Vec::new();
        let edited = self.store.crater([x, y, z], radius, &mut touched);
        self.store.settle(&mut self.digests);
        let settled: Vec<u32> = self.store.settled().to_vec();
        for chunk in settled {
            self.surface.refresh_chunk(&self.store, chunk);
            for cluster in clusters_of_chunk(&self.store, &self.clusters, chunk) {
                self.repairer.mark(cluster);
            }
        }
        edited
    }

    /// Settle whatever `set` calls have changed, refresh the columns and mark
    /// the clusters — the other half of what [`World`]'s voxel phase does.
    fn settle_and_mark(&mut self) {
        self.store.settle(&mut self.digests);
        let settled: Vec<u32> = self.store.settled().to_vec();
        for chunk in settled {
            self.surface.refresh_chunk(&self.store, chunk);
            for cluster in clusters_of_chunk(&self.store, &self.clusters, chunk) {
                self.repairer.mark(cluster);
            }
        }
    }

    fn repair(&mut self) {
        repair(
            &self.surface,
            &mut self.clusters,
            &mut self.scratch,
            &mut self.repairer,
        );
    }

    fn rebuilt(&mut self) -> Clusters {
        let cluster = self.clusters.size();
        let mut scratch = Scratch::for_map(&self.surface, cluster).expect("a scratch");
        Clusters::new(&self.surface, cluster, &mut scratch).expect("a decomposition")
    }

    fn commander_speed(&self) -> Speed {
        Speed::commander(&self.rules)
    }
}

/// Every cluster a chunk's footprint overlaps.
fn clusters_of_chunk(store: &VoxelStore, clusters: &Clusters, chunk: u32) -> Vec<u16> {
    let Some(origin) = store.chunk_origin(chunk) else {
        return Vec::new();
    };
    let edge = 32_i32;
    let size = clusters.size().max(1);
    let mut out: Vec<u16> = Vec::new();
    let mut cy = origin[1].div_euclid(size);
    while cy <= (origin[1] + edge - 1).div_euclid(size) {
        let mut cx = origin[0].div_euclid(size);
        while cx <= (origin[0] + edge - 1).div_euclid(size) {
            if let Some(index) = clusters.cluster_index(cx, cy) {
                out.push(u16::try_from(index).expect("at most 65 535 clusters"));
            }
            cx += 1;
        }
        cy += 1;
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// A deterministic 64-bit stream, so the pair sets and the crater stream are a
/// function of their seed and nothing else. splitmix64, the finaliser item 50
/// already uses.
struct Rng(u64);

impl Rng {
    const fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        pharmakos_sim::math::random::mix64(self.0)
    }

    fn below(&mut self, limit: i32) -> i32 {
        if limit <= 0 {
            return 0;
        }
        i32::try_from(self.next() % u64::try_from(limit).expect("positive")).expect("below limit")
    }
}

/// `count` connected pairs of walkable columns, deterministic in `seed`.
fn pairs(fixture: &Fixture, count: u32, seed: u64) -> Vec<(Node, Node)> {
    let mut rng = Rng::new(seed);
    let extent = fixture.surface.size();
    let mut out: Vec<(Node, Node)> = Vec::with_capacity(usize::try_from(count).expect("fits"));
    let mut attempts = 0;
    while u32::try_from(out.len()).expect("fits") < count && attempts < count * 200 {
        attempts += 1;
        let a = fixture
            .surface
            .node_of(rng.below(extent[0]), rng.below(extent[1]));
        let b = fixture
            .surface
            .node_of(rng.below(extent[0]), rng.below(extent[1]));
        let (Some(a), Some(b)) = (a, b) else {
            continue;
        };
        if a == b || !fixture.clusters.connected(&fixture.surface, a, b) {
            continue;
        }
        out.push((a, b));
    }
    assert_eq!(
        u32::try_from(out.len()).expect("fits"),
        count,
        "the map did not yield {count} connected pairs"
    );
    out
}

/// Walk a route one tick at a time under item 90's accumulator and return the
/// tick it arrives on.
///
/// The integrator, written as a literal tick loop rather than as a division, so
/// that comparing it with [`ticks_for_cost`] is an assertion rather than a
/// tautology. It is the same rule
/// [`pharmakos_sim::pathing::router::Router::advance`] applies.
fn walked_ticks(surface: &Surface, route: &[Node], cost_per_second: i32) -> Option<i32> {
    if route.len() < 2 {
        return Some(0);
    }
    let tick_hz = i32::try_from(TICK_HZ).expect("20");
    let mut at = 0_usize;
    let mut accumulator: i32 = 0;
    let mut tick: i32 = 0;
    while at + 1 < route.len() {
        tick = tick.checked_add(1)?;
        accumulator = accumulator.checked_add(cost_per_second)?;
        while at + 1 < route.len() {
            let price = surface
                .step_cost_between(route[at], route[at + 1])?
                .checked_mul(tick_hz)?;
            if accumulator < price {
                break;
            }
            accumulator -= price;
            at += 1;
        }
    }
    Some(tick)
}

/// Nearest-rank percentile over an already-sorted slice.
fn percentile(sorted: &[i64], p: i64) -> i64 {
    if sorted.is_empty() {
        return 0;
    }
    let n = i64::try_from(sorted.len()).expect("fits");
    let index = ((p * (n - 1)) + 50).div_euclid(100);
    sorted[usize::try_from(index.clamp(0, n - 1)).expect("clamped")]
}

// ---------------------------------------------------------------------------
// The golden
// ---------------------------------------------------------------------------

#[test]
fn the_path_hashes_match_their_golden() {
    let mut fixture = Fixture::build(SEED, CLUSTER);
    let speed = fixture.commander_speed();
    let set = pairs(&fixture, GOLDEN_PAIRS, 0x5EED_0001);
    let mut out = String::with_capacity(8 * 1024);

    writeln!(out, "seed\t{}", hex(SEED)).expect("string");
    writeln!(out, "cluster\t{CLUSTER}").expect("string");
    writeln!(
        out,
        "graph\tpristine\t{}\t{}\t{}",
        hex(fixture.clusters.fingerprint()),
        fixture.clusters.transition_count(),
        fixture.clusters.abstract_edge_count()
    )
    .expect("string");
    write_routes(&mut fixture, &set, speed, "pristine", &mut out);

    // The crater stream: twenty-four craters walked across the map on a stride
    // that is coprime with the footprint, so no two land on the same column and
    // every one of them has rock to take.
    let mut craters = 0;
    let mut n: u32 = 0;
    while n < GOLDEN_CRATERS {
        let x = i32::try_from((n.wrapping_mul(97) + 40) % 360).expect("fits") + 12;
        let y = i32::try_from((n.wrapping_mul(53) + 24) % 360).expect("fits") + 12;
        craters += fixture.crater(x, y, 3);
        n += 1;
    }
    fixture.repair();
    writeln!(
        out,
        "craters\t{GOLDEN_CRATERS}\t{craters}\tstore\t{}",
        hex(fixture.store.store_digest())
    )
    .expect("string");
    writeln!(
        out,
        "graph\trepaired\t{}\t{}\t{}",
        hex(fixture.clusters.fingerprint()),
        fixture.clusters.transition_count(),
        fixture.clusters.abstract_edge_count()
    )
    .expect("string");
    write_routes(&mut fixture, &set, speed, "repaired", &mut out);

    if let Some(target) = target_dir() {
        let directory = target.join("golden").join("pathing");
        std::fs::create_dir_all(&directory).expect("creating the golden output directory");
        std::fs::write(directory.join("actual.path-hashes.txt"), out.as_bytes())
            .expect("writing actual.path-hashes.txt");
    }

    let committed = repo_root()
        .join("tests")
        .join("golden")
        .join("pathing")
        .join("expected.path-hashes.txt");
    let Ok(expected) = std::fs::read_to_string(&committed) else {
        panic!(
            "no committed golden at {} — `cargo xtask golden --bless` writes it, and the pull \
             request explains it",
            committed.display()
        );
    };
    assert_eq!(
        expected, out,
        "the path hashes moved. tests/golden/pathing/README.md says what a diff there means: the \
         search, the cost rows, the repair, or an estimate that stopped being one-sided."
    );
}

/// One line per pair: the query, what the estimator said, and the digest of the
/// route the search actually returned.
fn write_routes(
    fixture: &mut Fixture,
    set: &[(Node, Node)],
    speed: Speed,
    phase: &str,
    out: &mut String,
) {
    let mut buffer: Vec<Node> = Vec::with_capacity(usize::try_from(MAX_ROUTE_NODES).expect("fits"));
    let mut digest_bytes: Vec<u8> = Vec::new();
    for (index, (start, goal)) in set.iter().enumerate() {
        let estimated = estimate(
            &fixture.surface,
            &fixture.clusters,
            &mut fixture.scratch,
            *start,
            *goal,
            Fog::Clear,
            speed,
        );
        let walked = route(
            &fixture.surface,
            &fixture.clusters,
            &mut fixture.scratch,
            *start,
            *goal,
            &mut buffer,
        );
        let digest = pharmakos_sim::pathing::router::route_digest(&buffer, &mut digest_bytes);
        match (estimated, walked) {
            (Some(estimated), Some(walked)) => writeln!(
                out,
                "{phase}\t{index:02}\t{start}\t{goal}\t{}\t{}\t{}\t{}\t{}\t{}",
                estimated.cost,
                estimated.ticks,
                estimated.legs,
                walked.cost,
                buffer.len(),
                hex(digest)
            )
            .expect("string"),
            _ => writeln!(out, "{phase}\t{index:02}\t{start}\t{goal}\tnoroute").expect("string"),
        }
    }
}

#[test]
fn the_golden_has_no_carriage_return_and_ends_with_a_newline() {
    let committed = repo_root()
        .join("tests")
        .join("golden")
        .join("pathing")
        .join("expected.path-hashes.txt");
    let Ok(text) = std::fs::read_to_string(&committed) else {
        return;
    };
    assert!(!text.contains('\r'), "the golden carries a carriage return");
    assert!(text.ends_with('\n'), "the golden has no trailing newline");
}

// ---------------------------------------------------------------------------
// The three repair tests (item 60)
// ---------------------------------------------------------------------------

#[test]
fn repair_equals_a_from_scratch_build() {
    let mut fixture = Fixture::build(SEED, CLUSTER);
    let mut n: u32 = 0;
    while n < 16 {
        let x = i32::try_from((n.wrapping_mul(61) + 30) % 340).expect("fits") + 20;
        let y = i32::try_from((n.wrapping_mul(37) + 50) % 340).expect("fits") + 20;
        fixture.crater(x, y, 3);
        fixture.repair();
        let fresh = fixture.rebuilt();
        assert_eq!(
            fixture.clusters.fingerprint(),
            fresh.fingerprint(),
            "after crater {n} at ({x}, {y}) the repaired graph is not the graph a from-scratch \
             build produces"
        );
        n += 1;
    }
}

#[test]
fn repair_matches_the_conservative_set() {
    // The precise "affected neighbour" set is computed rather than assumed, and
    // this is what makes the narrower set safe: rebuilding the conservative
    // superset — every cluster within two of a dirty one — reaches the same
    // graph.
    let mut precise = Fixture::build(SEED, CLUSTER);
    let mut wide = Fixture::build(SEED, CLUSTER);
    let mut n: u32 = 0;
    while n < 12 {
        let x = i32::try_from((n.wrapping_mul(71) + 44) % 340).expect("fits") + 20;
        let y = i32::try_from((n.wrapping_mul(43) + 18) % 340).expect("fits") + 20;
        precise.crater(x, y, 3);
        let dirty = wide.crater(x, y, 3);
        assert!(dirty > 0, "crater {n} edited nothing; the test is vacuous");

        precise.repair();

        // The conservative route, by hand: local components and all four
        // borders for every dirty cluster, then everything within two clusters
        // of one rebuilt wholesale.
        let mut dirty_clusters: Vec<u16> = Vec::new();
        let settled: Vec<u32> = (0..wide.store.chunk_count())
            .filter(|chunk| wide.store.is_modified(*chunk))
            .collect();
        for chunk in settled {
            for cluster in clusters_of_chunk(&wide.store, &wide.clusters, chunk) {
                dirty_clusters.push(cluster);
            }
        }
        dirty_clusters.sort_unstable();
        dirty_clusters.dedup();
        for cluster in &dirty_clusters {
            let index = usize::from(*cluster);
            wide.clusters.rebuild_local_comp(&wide.surface, index);
            let (cx, cy) = wide.clusters.cluster_coord(index);
            wide.clusters.rebuild_vborder(&wide.surface, cx, cy);
            wide.clusters.rebuild_vborder(&wide.surface, cx - 1, cy);
            wide.clusters.rebuild_hborder(&wide.surface, cx, cy);
            wide.clusters.rebuild_hborder(&wide.surface, cx, cy - 1);
        }
        let affected = conservative_affected(&wide.clusters, &dirty_clusters);
        for cluster in &affected {
            wide.clusters.rebuild_trans(usize::from(*cluster));
        }
        for cluster in &affected {
            wide.clusters
                .rebuild_intra(&wide.surface, &mut wide.scratch, usize::from(*cluster));
            wide.clusters.rebuild_inter(usize::from(*cluster));
        }
        wide.clusters.rebuild_components();

        assert_eq!(
            precise.clusters.fingerprint(),
            wide.clusters.fingerprint(),
            "the precise repair and the conservative superset disagree after crater {n}"
        );
        n += 1;
    }
}

#[test]
fn a_border_holds_every_run_it_finds() {
    // The capacity bound the decomposition is sized on, as a test rather than
    // as arithmetic in a comment: a border of `n` cells can hold `n` maximal
    // runs, not `n / 2`, because two adjacent border cells can each be
    // crossable while not being steppable to each other *along* the border.
    //
    // The comb: every cell of one vertical border raised to the same height on
    // both sides — so the crossing is legal — and alternating by two voxels
    // along the border — so no two consecutive cells are linked. Thirty-two
    // runs of one cell each, and every one of them has to survive into the
    // border's transition list. A capacity of `n / 2` would silently drop half
    // of them, which is a route that exists and cannot be found.
    let mut fixture = Fixture::build(SEED, CLUSTER);
    let xa = 3 * CLUSTER - 1;
    let xb = xa + 1;
    let y0 = 2 * CLUSTER;

    let mut base = 0;
    let mut y = y0;
    while y < y0 + CLUSTER {
        base = base
            .max(fixture.store.top_solid_z(xa, y).unwrap_or(0))
            .max(fixture.store.top_solid_z(xb, y).unwrap_or(0));
        y += 1;
    }
    base += 3;

    let mut y = y0;
    while y < y0 + CLUSTER {
        let target = base + if y % 2 == 0 { 0 } else { 2 };
        for x in [xa, xb] {
            let mut z = fixture.store.top_solid_z(x, y).unwrap_or(-1) + 1;
            while z <= target {
                fixture.store.set([x, y, z], Material::STONE);
                z += 1;
            }
        }
        y += 1;
    }
    fixture.settle_and_mark();
    fixture.repair();

    let index = fixture
        .clusters
        .vborder_index(2, 2)
        .expect("the border between (2, 2) and (3, 2)");
    let transitions = fixture.clusters.vborder(index);
    assert_eq!(
        transitions.len(),
        usize::try_from(CLUSTER).expect("fits"),
        "the comb produced {} transitions of a possible {CLUSTER}; either the run scan merged \
         runs it should not have, or the border's capacity dropped some",
        transitions.len()
    );
    for pair in transitions.windows(2) {
        assert_ne!(pair[0].a, pair[1].a, "two runs share a transition cell");
    }

    // And the graph a repair left is still the graph a from-scratch build
    // produces, at the fragmentation this test creates rather than at a
    // crater's.
    let fresh = fixture.rebuilt();
    assert_eq!(
        fixture.clusters.fingerprint(),
        fresh.fingerprint(),
        "the repaired graph disagrees with a from-scratch build over a combed border"
    );
}

#[test]
fn repair_holds_at_every_cluster_size() {
    for cluster in [16, 32, 64] {
        let mut fixture = Fixture::build(SEED, cluster);
        let mut n: u32 = 0;
        while n < 6 {
            let x = i32::try_from((n.wrapping_mul(89) + 26) % 340).expect("fits") + 20;
            let y = i32::try_from((n.wrapping_mul(59) + 66) % 340).expect("fits") + 20;
            fixture.crater(x, y, 3);
            fixture.repair();
            let fresh = fixture.rebuilt();
            assert_eq!(
                fixture.clusters.fingerprint(),
                fresh.fingerprint(),
                "cluster size {cluster}: repair and from-scratch disagree after crater {n}"
            );
            n += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// The estimator's contract (item 61)
// ---------------------------------------------------------------------------

#[test]
fn the_estimate_is_never_optimistic_at_any_percentile() {
    // Item 61 makes *never optimistic* part of the contract, because the
    // editor's rounding rests on it. So this asserts the **signed** error, not
    // its absolute value: an estimate that came in under the walk is the
    // failure mode, and an |error| percentile would hide it.
    let mut fixture = Fixture::build(SEED, CLUSTER);
    let speed = fixture.commander_speed();
    let set = pairs(&fixture, 160, 0x5EED_0002);
    let mut buffer: Vec<Node> = Vec::with_capacity(usize::try_from(MAX_ROUTE_NODES).expect("fits"));
    let mut signed: Vec<i64> = Vec::with_capacity(set.len());

    for (start, goal) in &set {
        let estimated = estimate(
            &fixture.surface,
            &fixture.clusters,
            &mut fixture.scratch,
            *start,
            *goal,
            Fog::Clear,
            speed,
        )
        .expect("a connected pair has an estimate");
        let walked = route(
            &fixture.surface,
            &fixture.clusters,
            &mut fixture.scratch,
            *start,
            *goal,
            &mut buffer,
        )
        .expect("a connected pair has a route");
        assert!(
            !walked.partial,
            "the fixture's routes must fit the route buffer, or the comparison is against half a \
             journey"
        );
        let ticks = walked_ticks(&fixture.surface, &buffer, speed.cost_per_second)
            .expect("the route is walkable");
        assert!(
            walked.cost <= estimated.cost,
            "the smoother made a route dearer than the estimate priced: {} against {}",
            walked.cost,
            estimated.cost
        );
        assert_eq!(
            ticks_for_cost(walked.cost, speed.cost_per_second),
            Some(ticks),
            "the integrator and the ceiling rule disagree; the estimator is only allowed to \
             divide because they are two computations of the same number (item 90)"
        );
        assert!(
            estimated.ticks >= ticks,
            "the estimate was optimistic: {} ticks promised, {ticks} walked",
            estimated.ticks
        );
        signed
            .push((i64::from(estimated.ticks - ticks) * 1000).div_euclid(i64::from(ticks.max(1))));
    }

    signed.sort_unstable();
    let worst = signed.first().copied().unwrap_or(0);
    assert!(
        worst >= 0,
        "the signed error is negative at some percentile ({worst} per mille); item 61 makes \
         never optimistic part of the contract"
    );
    let p90 = percentile(&signed, 90);
    println!(
        "::notice::estimator signed error per mille: min {worst}, p50 {}, p90 {p90}, p99 {}, max {}",
        percentile(&signed, 50),
        percentile(&signed, 99),
        signed.last().copied().unwrap_or(0)
    );
    assert!(
        p90 <= 150,
        "the p90 error is {p90} per mille, over item 61's 15 % budget"
    );
}

#[test]
fn fog_never_makes_a_route_look_cheaper() {
    let mut fixture = Fixture::build(SEED, CLUSTER);
    let speed = fixture.commander_speed();
    let set = pairs(&fixture, 40, 0x5EED_0003);
    let unknown = vec![true; usize::try_from(fixture.surface.node_count()).expect("fits")];
    for (start, goal) in &set {
        let clear = estimate(
            &fixture.surface,
            &fixture.clusters,
            &mut fixture.scratch,
            *start,
            *goal,
            Fog::Clear,
            speed,
        )
        .expect("an estimate");
        let fogged = estimate(
            &fixture.surface,
            &fixture.clusters,
            &mut fixture.scratch,
            *start,
            *goal,
            Fog::Unknown(&unknown),
            speed,
        )
        .expect("an estimate");
        assert!(
            fogged.cost >= clear.cost,
            "fog made a route cheaper: {} against {}",
            fogged.cost,
            clear.cost
        );
        assert!(
            fogged.fogged,
            "a route priced through fog must say so, or the editor renders a bound as an ETA"
        );
        assert!(!clear.fogged, "a clear route must not claim to be fogged");
        // Item 61 caps the fogged estimate at +50 % over the clear one, because
        // the multiplier is applied per edge, every edge is priced at most
        // once, and the integer division truncates rather than rounding up.
        // The cap is exact: no per-leg slack is allowed for here, and a
        // rounding term reintroduced in `fog_price` would fail this line.
        assert!(
            i64::from(fogged.cost) * 2 <= i64::from(clear.cost) * 3,
            "the fogged estimate is more than 3/2 of the clear one: {} against {}",
            fogged.cost,
            clear.cost
        );
    }
}

#[test]
fn no_path_is_answered_by_the_oracle() {
    // "Sealed in" is a designed state (item 60), so the refusal has to be cheap:
    // the union-find answers it in two array reads and **no search runs at all**,
    // which is the cost assertion this test makes.
    let mut fixture = Fixture::build(SEED, CLUSTER);
    let inside = fixture.surface.node_of(200, 200).expect("in bounds");
    let outside = fixture.surface.node_of(40, 40).expect("in bounds");
    assert!(
        fixture
            .clusters
            .connected(&fixture.surface, inside, outside),
        "the pristine map must connect the two ends, or the moat below proves nothing"
    );

    // Dig a moat: a ring of columns taken down to bedrock, which leaves them
    // unwalkable and the cell inside them sealed.
    for dy in -1..=1_i32 {
        for dx in -1..=1_i32 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let (x, y) = (200 + dx, 200 + dy);
            let mut z = 0;
            while z < fixture.surface.size()[2] {
                fixture.store.set([x, y, z], Material::AIR);
                z += 1;
            }
        }
    }
    fixture.store.settle(&mut fixture.digests);
    let settled: Vec<u32> = fixture.store.settled().to_vec();
    for chunk in settled {
        fixture.surface.refresh_chunk(&fixture.store, chunk);
        for cluster in clusters_of_chunk(&fixture.store, &fixture.clusters, chunk) {
            fixture.repairer.mark(cluster);
        }
    }
    fixture.repair();

    assert!(
        fixture.surface.walkable(inside),
        "the sealed-in column is still a column a unit can stand on"
    );
    assert!(
        !fixture
            .clusters
            .connected(&fixture.surface, inside, outside),
        "the moat did not seal the cell in"
    );

    fixture.scratch.reset_counters();
    let speed = fixture.commander_speed();
    let refused = estimate(
        &fixture.surface,
        &fixture.clusters,
        &mut fixture.scratch,
        inside,
        outside,
        Fog::Clear,
        speed,
    );
    assert_eq!(refused, None, "a sealed-in walker has no estimate");
    assert_eq!(
        fixture.scratch.expansions(),
        0,
        "the refusal expanded {} nodes; 'no path' must be two array reads, not a search that \
         exhausts the map (item 60)",
        fixture.scratch.expansions()
    );
    assert_eq!(
        fixture.scratch.endpoint_expansions(),
        0,
        "the refusal ran an endpoint sweep; the oracle answers before any search"
    );

    let mut buffer: Vec<Node> = Vec::with_capacity(8);
    assert_eq!(
        route(
            &fixture.surface,
            &fixture.clusters,
            &mut fixture.scratch,
            inside,
            outside,
            &mut buffer,
        ),
        None,
        "a sealed-in walker has no route either"
    );
}

#[test]
fn the_oracle_agrees_with_an_exhaustive_search() {
    // The oracle is exact, not conservative: every pair it refuses is a pair an
    // exhaustive search cannot connect, and every pair it accepts is one the
    // search does connect.
    let mut fixture = Fixture::build(SEED, CLUSTER);
    let mut n: u32 = 0;
    while n < 6 {
        let x = i32::try_from((n.wrapping_mul(83) + 36) % 320).expect("fits") + 32;
        let y = i32::try_from((n.wrapping_mul(47) + 12) % 320).expect("fits") + 32;
        fixture.crater(x, y, 4);
        n += 1;
    }
    fixture.repair();

    let source = fixture.surface.node_of(64, 64).expect("in bounds");
    dijkstra_bounded(
        &fixture.surface,
        Bound::Whole,
        &mut fixture.scratch,
        source,
        false,
    );
    let mut checked = 0;
    let mut node: Node = 0;
    while node < fixture.surface.node_count() {
        if fixture.surface.walkable(node) {
            let truth = fixture.scratch.cost_of(node).is_some();
            assert_eq!(
                fixture.clusters.connected(&fixture.surface, source, node),
                truth,
                "the oracle disagrees with an exhaustive search at column {node}"
            );
            checked += 1;
        }
        node += 1_021;
    }
    assert!(checked > 100, "only {checked} columns were checked");
}

// ---------------------------------------------------------------------------
// The search itself (item 62)
// ---------------------------------------------------------------------------

#[test]
fn the_tiebreak_survives_a_neighbour_permutation() {
    // Item 62's total order, as a check rather than a claim: the same query,
    // with the neighbours offered in a different order, returns the identical
    // route. A heap ordered on `f` alone would not.
    let mut fixture = Fixture::build(SEED, CLUSTER);
    let set = pairs(&fixture, 24, 0x5EED_0004);
    let mut first: Vec<Node> = Vec::new();
    let mut second: Vec<Node> = Vec::new();
    let permutations: [[u8; 8]; 3] = [
        DEFAULT_ORDER,
        [7, 6, 5, 4, 3, 2, 1, 0],
        [3, 1, 6, 0, 7, 2, 5, 4],
    ];
    for (start, goal) in &set {
        let cluster = fixture.clusters.cluster_of(*start);
        if fixture.clusters.cluster_of(*goal) != cluster {
            continue;
        }
        let bound = Bound::Cluster(fixture.clusters.of_node(), cluster);
        let base = astar(
            &fixture.surface,
            bound,
            &mut fixture.scratch,
            *start,
            *goal,
            &mut first,
        );
        for order in permutations {
            let other = astar_with_order(
                &fixture.surface,
                bound,
                &mut fixture.scratch,
                *start,
                *goal,
                order,
                &mut second,
            );
            assert_eq!(base, other, "the cost moved with the expansion order");
            assert_eq!(first, second, "the route moved with the expansion order");
        }
    }
}

#[test]
fn an_intra_edge_is_the_cost_of_the_path_it_stands_for() {
    // The abstract graph's edges are exact bounded-search costs, which is what
    // makes the abstract cost equal to the unsmoothed refinement's cost and
    // therefore what makes the estimate never optimistic.
    let mut fixture = Fixture::build(SEED, CLUSTER);
    let mut path: Vec<Node> = Vec::new();
    let mut checked = 0;
    let mut cluster = 0;
    while cluster < fixture.clusters.cluster_count() {
        let cells: Vec<Node> = fixture.clusters.trans_cells(cluster).to_vec();
        let tag = u16::try_from(cluster).expect("fits");
        for (slot, from) in cells.iter().enumerate() {
            let (targets, costs) = {
                let (targets, costs) = fixture.clusters.intra_edges(cluster, slot);
                (targets.to_vec(), costs.to_vec())
            };
            for (target, cost) in targets.iter().zip(costs.iter()).take(2) {
                let to = cells[usize::from(*target)];
                let found = astar(
                    &fixture.surface,
                    Bound::Cluster(fixture.clusters.of_node(), tag),
                    &mut fixture.scratch,
                    *from,
                    to,
                    &mut path,
                );
                assert_eq!(
                    found,
                    Some(*cost),
                    "cluster {cluster}: the intra edge from {from} to {to} is not the cost of the \
                     path it stands for"
                );
                checked += 1;
            }
        }
        cluster += 7;
    }
    assert!(checked > 20, "only {checked} intra edges were checked");
}

#[test]
fn a_query_leaves_the_graph_alone() {
    // A query inserts its endpoints into the *scratch*, never into the graph, so
    // two queries can never interfere and the graph a repair produced is the
    // graph every later query sees.
    let mut fixture = Fixture::build(SEED, CLUSTER);
    let speed = fixture.commander_speed();
    let before = fixture.clusters.fingerprint();
    let set = pairs(&fixture, 8, 0x5EED_0005);
    for (start, goal) in &set {
        let _ = estimate(
            &fixture.surface,
            &fixture.clusters,
            &mut fixture.scratch,
            *start,
            *goal,
            Fog::Clear,
            speed,
        );
    }
    assert_eq!(
        before,
        fixture.clusters.fingerprint(),
        "a query changed the abstract graph"
    );
}

#[test]
fn the_cluster_edge_ceiling_is_what_the_row_offsets_can_index() {
    // A cluster of edge `n` holds at most `4 * n` transitions and at most
    // `4n * (4n - 1)` directed intra edges, and `Clusters` stores each row's
    // offset into that edge array as a `u16`. So the ceiling on the cluster
    // edge is arithmetic, not taste: one step past it a row offset stops
    // converting and the graph loses edges with nothing red in front of it.
    let edge = i64::from(pharmakos_sim::pathing::clusters::MAX_CLUSTER_EDGE);
    let trans = edge * 4;
    let intra = trans * (trans - 1);
    assert!(
        intra <= i64::from(u16::MAX),
        "MAX_CLUSTER_EDGE {edge} needs {intra} intra-edge slots, which a u16 row offset cannot \
         index"
    );
    let over = (edge + 1) * 4;
    assert!(
        over * (over - 1) > i64::from(u16::MAX),
        "MAX_CLUSTER_EDGE is below the arithmetic ceiling: edge {} still fits, so the doc's \
         justification is not the real bound",
        edge + 1
    );
}

#[test]
fn an_undecomposable_cluster_edge_is_refused_rather_than_mistagged() {
    // The decomposition tags every column with a u16 cluster id, and the
    // conversion that writes the tag is fallible and silent. A cluster edge
    // that produces more than 65 536 clusters therefore has to be refused at
    // construction, or the oracle answers confidently and wrongly for every
    // column above the 65 536th.
    let mut fixture = Fixture::build(SEED, CLUSTER);
    let surface = &fixture.surface;
    let scratch = &mut fixture.scratch;
    assert!(
        Clusters::new(surface, 1, scratch).is_none(),
        "a cluster edge of 1 decomposes the 384-voxel map into 147 456 clusters, which the u16 \
         tags cannot name, and it was accepted"
    );
    assert!(
        Clusters::new(surface, 0, scratch).is_none(),
        "a cluster edge of zero was accepted"
    );
    let mut four = Scratch::for_map(surface, 4).expect("a scratch for a cluster edge of 4");
    assert!(
        Clusters::new(surface, 4, &mut four).is_some(),
        "a cluster edge of 4 gives 9 216 clusters, well inside the u16 tags, and it was refused"
    );
}

// ---------------------------------------------------------------------------
// The cap, the router and the world (items 60, 69, 90)
// ---------------------------------------------------------------------------

#[test]
fn every_unit_kind_walks_at_the_speed_its_rules_row_names() {
    // The router indexes its speed table by `UnitKind::id()`, which runs 1..=6
    // rather than 0..6. An array sized at six therefore drops the highest kind
    // silently: `advance` sees speed 0, skips the unit, and it stands in
    // `Walking` for ever while the estimator quotes it the rules row's speed.
    // Estimator and walker disagreeing permanently is the exact failure item
    // 61's never-optimistic contract exists to prevent, so it is pinned here
    // for every kind rather than for the one the harness happens to spawn.
    let rules = rules();
    let router = pharmakos_sim::pathing::router::Router::new(4, &rules);
    for kind in UnitKind::ALL {
        let row = rules
            .cost_per_second(kind)
            .unwrap_or_else(|| panic!("{kind:?} has no cost_per_second row"));
        assert_eq!(
            router.speed_of(kind),
            row,
            "the router walks {kind:?} at {} where its rules row says {row}",
            router.speed_of(kind)
        );
        assert!(row > 0, "{kind:?} cannot walk at all");
    }
}

#[test]
fn the_repath_cap_is_served_round_robin_and_never_exceeded() {
    let mut world = harness_world(8);
    let cap = world.rules().repath_cap_per_tick();
    let mut enc = pharmakos_sim::encoding::Enc::with_capacity(64 * 1024);
    let mut tick = 0;
    while tick < 60 {
        let _ = world.step(&mut enc);
        assert!(
            world.serve_report().served <= cap,
            "tick {tick} served {} repaths against a cap of {cap}",
            world.serve_report().served
        );
        tick += 1;
    }
    assert!(
        world.router().served_total() > 0,
        "nothing was ever served; the test is vacuous"
    );
    assert_eq!(
        world.repath_backlog(),
        0,
        "the backlog never drained on an undamaged map"
    );
}

#[test]
fn a_walker_arrives_on_the_tick_the_estimate_promised() {
    // The end-to-end form of item 90's accumulator: a unit walking a real route
    // in a real world arrives no later than the estimator said, and the two
    // computations of "when" are the same number.
    let mut world = harness_world(0);
    let mut enc = pharmakos_sim::encoding::Enc::with_capacity(64 * 1024);
    // One tick to serve the first repaths, so every unit has a route.
    let _ = world.step(&mut enc);
    let _ = world.step(&mut enc);
    let unit = (0..world.router().len())
        .find(|unit| world.router().state(*unit) == WalkState::Walking)
        .expect("a unit is walking by the second tick");
    let route = world.router().route(unit).to_vec();
    let kind = UnitKind::from_id(world.units().kinds()[usize::try_from(unit).expect("fits")])
        .expect("a unit kind");
    let speed = world
        .rules()
        .cost_per_second(kind)
        .expect("every kind has a speed row");
    let promised = walked_ticks(world.surface(), &route, speed).expect("the route is walkable");

    let mut ticks = 0;
    while world.router().route_cursor(unit) + 1 < u32::try_from(route.len()).expect("fits") {
        let _ = world.step(&mut enc);
        ticks += 1;
        assert!(
            ticks <= promised + 2,
            "the walker is late: {ticks} ticks against {promised} promised"
        );
    }
    assert_eq!(
        ticks, promised,
        "the walker did not arrive on the tick the accumulator says it should"
    );
}

#[test]
fn a_sealed_in_walker_parks_and_is_re_armed_when_the_graph_changes() {
    // Item 60's designed state: park and report, never a repath loop — and try
    // again exactly when the graph that refused the route has been repaired.
    let mut world = harness_world(0);
    let mut enc = pharmakos_sim::encoding::Enc::with_capacity(64 * 1024);
    let _ = world.step(&mut enc);
    let _ = world.step(&mut enc);

    // Wall the first unit in: a solid voxel five above each of its eight
    // neighbours' surfaces. The columns stay walkable, and every step out of
    // the middle one is a five-voxel climb, which the one-voxel rule refuses.
    let unit = 0_u32;
    let position = world.units().positions()[0];
    let (x, y) = (position[0].floor_voxels(), position[1].floor_voxels());
    let mut wall: Vec<[i32; 3]> = Vec::new();
    for dy in -1..=1_i32 {
        for dx in -1..=1_i32 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let top = world
                .voxels()
                .top_solid_z(x + dx, y + dy)
                .expect("a column with a floor");
            wall.push([x + dx, y + dy, top + 5]);
        }
    }
    for at in &wall {
        assert!(world.request_voxel_edit(VoxelEdit::Set {
            at: *at,
            material: Material::STONE,
        }));
    }
    let mut tick = 0;
    while tick < 12 {
        let _ = world.step(&mut enc);
        tick += 1;
    }
    assert_eq!(
        world.router().state(unit),
        WalkState::Sealed,
        "a walker with no route must park as sealed in, not keep asking"
    );
    assert_eq!(world.sealed_units(), 1, "exactly one walker is sealed in");

    let parked = world.router().served_total();
    let mut tick = 0;
    while tick < 25 {
        let _ = world.step(&mut enc);
        tick += 1;
    }
    assert_eq!(
        world.router().served_total(),
        parked,
        "a sealed-in walker asked again while nothing changed; that is the repath loop item 60 \
         rules out"
    );

    // Take the wall down again: the repair publishes the clusters it rebuilt,
    // and the walker standing in one of them is re-armed.
    for at in &wall {
        assert!(world.request_voxel_edit(VoxelEdit::Set {
            at: *at,
            material: Material::AIR,
        }));
    }
    let mut tick = 0;
    while tick < 12 {
        let _ = world.step(&mut enc);
        tick += 1;
    }
    assert!(
        world.router().served_total() > parked,
        "the repair did not re-arm the sealed walker"
    );
    assert_eq!(
        world.sealed_units(),
        0,
        "the walker is still parked after its way out was opened"
    );
}

#[test]
fn a_sealed_in_walker_is_re_armed_by_a_repair_nowhere_near_it() {
    // The narrow rule — re-arm only a unit standing in a repaired cluster or
    // bound for one — leaves a unit behind a moat parked for the rest of the
    // match when the moat is opened more than a cluster away. The rule is the
    // oracle's answer instead: on a tick whose repair rebuilt the components,
    // a sealed unit the oracle now calls connected asks again, wherever the
    // repair happened. This test hands `rearm_sealed` a repaired list holding
    // neither the unit's cluster nor its destination's, which is exactly the
    // case the narrow rule misses.
    let mut fixture = Fixture::build(SEED, CLUSTER);
    let rules = rules();
    let mut router = pharmakos_sim::pathing::router::Router::new(1, &rules);

    let (start, goal) = pairs(&fixture, 1, 0x5EED_0007)[0];
    let (sx, sy) = fixture.surface.coord_of(start);
    let (gx, gy) = fixture.surface.coord_of(goal);
    let point = |x: i32, y: i32, z: i32| {
        [
            Fx::from_voxels(i16::try_from(x).expect("a map coordinate fits an i16")),
            Fx::from_voxels(i16::try_from(y).expect("a map coordinate fits an i16")),
            Fx::from_voxels(i16::try_from(z).expect("a map height fits an i16")),
        ]
    };
    let positions = vec![point(sx, sy, fixture.surface.standing_z(start))];
    let destinations = vec![point(gx, gy, fixture.surface.standing_z(goal))];

    // Wall the start in: a solid voxel five above each of its eight
    // neighbours, so every step out is a five-voxel climb the one-voxel rule
    // refuses. The columns stay walkable, so the seal is in the step rule.
    let mut wall: Vec<[i32; 3]> = Vec::new();
    for dy in -1..=1_i32 {
        for dx in -1..=1_i32 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let top = fixture
                .store
                .top_solid_z(sx + dx, sy + dy)
                .expect("a column with a floor");
            wall.push([sx + dx, sy + dy, top + 5]);
        }
    }
    for at in &wall {
        fixture.store.set(*at, Material::STONE);
    }
    fixture.settle_and_mark();
    fixture.repair();

    router.request(0);
    let report = router.serve(
        &fixture.surface,
        &fixture.clusters,
        &mut fixture.scratch,
        &positions,
        &destinations,
        1,
    );
    assert_eq!(report.sealed, 1, "the walled-in walker did not seal");
    assert_eq!(router.state(0), WalkState::Sealed);

    // Open the way out again, then publish a repaired list that deliberately
    // names neither end of the unit's journey.
    for at in &wall {
        fixture.store.set(*at, Material::AIR);
    }
    fixture.settle_and_mark();
    fixture.repair();

    let here = fixture.clusters.cluster_of(start);
    let there = fixture.clusters.cluster_of(goal);
    let elsewhere: Vec<u16> = (0..u16::try_from(fixture.clusters.cluster_count()).expect("fits"))
        .filter(|cluster| *cluster != here && *cluster != there)
        .take(3)
        .collect();
    assert!(
        !elsewhere.is_empty(),
        "the map has no cluster that is neither end of the journey"
    );
    let rearmed = router.rearm_sealed(
        &fixture.surface,
        &fixture.clusters,
        &elsewhere,
        &positions,
        &destinations,
    );
    assert_eq!(
        rearmed, 1,
        "a repair that reconnected the walker left it parked because it happened elsewhere"
    );
    assert_eq!(router.state(0), WalkState::Waiting);
}

#[test]
fn a_restored_world_rebuilds_the_graph_it_was_saved_with() {
    // The graph is derived, so a snapshot does not carry it — and this is what
    // makes "derived" a checked claim: a restore rebuilds a graph with the same
    // fingerprint, and the chain continues identically.
    let mut world = harness_world(4);
    let mut enc = pharmakos_sim::encoding::Enc::with_capacity(64 * 1024);
    let mut tick = 0;
    while tick < 20 {
        if tick % 3 == 0 {
            let x = 40 + tick * 7;
            let y = 60 + tick * 5;
            let z = world.voxels().top_solid_z(x, y).unwrap_or(0);
            assert!(world.request_voxel_edit(VoxelEdit::Crater {
                centre: [x, y, z],
                radius: 3,
            }));
        }
        let _ = world.step(&mut enc);
        tick += 1;
    }
    let fingerprint = world.clusters().fingerprint();
    let snapshot = pharmakos_sim::Snapshot::capture(&world);
    let bytes = snapshot.to_bytes().expect("a snapshot encodes");

    let mut restored = harness_world(4);
    pharmakos_sim::Snapshot::from_bytes(&bytes)
        .expect("the bytes decode")
        .restore_into(&mut restored)
        .expect("the snapshot restores");
    assert_eq!(
        restored.clusters().fingerprint(),
        fingerprint,
        "the rebuilt graph is not the graph the snapshot was taken over"
    );
    assert_eq!(
        restored.state_hash(),
        world.state_hash(),
        "the restored world does not hash the same"
    );

    let mut a = pharmakos_sim::encoding::Enc::with_capacity(64 * 1024);
    let mut b = pharmakos_sim::encoding::Enc::with_capacity(64 * 1024);
    let mut n = 0;
    while n < 25 {
        assert_eq!(
            world.step(&mut a),
            restored.step(&mut b),
            "the chain diverged {n} ticks after the restore"
        );
        n += 1;
    }
}

// ---------------------------------------------------------------------------
// The cap's sustainability over a full segment (items 60 and 69)
// ---------------------------------------------------------------------------

/// The cap's queue discipline, modelled: a pending flag per unit, a cursor, and
/// a cap.
///
/// It is a **model**, and `the_model_serves_what_the_router_serves` is what
/// makes it an honest one: the real
/// [`pharmakos_sim::pathing::router::Router`] and this agree on which units
/// they serve, in which order, tick by tick. The model exists because the
/// property under test is the *queue's* over a full 8-minute segment — 9 600
/// ticks at a cap of 16 is 153 600 repaths, and running the searches for them
/// would measure the search rather than the discipline. That is the same split
/// G3' made: the cap "was measured for what it costs, not for what it picks".
struct CapQueue {
    pending: Vec<bool>,
    cursor: u32,
    cap: u32,
    offered: u64,
    served: u64,
    backlog_peak: u32,
}

impl CapQueue {
    fn new(units: u32, cap: u32) -> CapQueue {
        CapQueue {
            pending: vec![false; usize::try_from(units).expect("fits")],
            cursor: 0,
            cap,
            offered: 0,
            served: 0,
            backlog_peak: 0,
        }
    }

    fn request(&mut self, unit: u32) {
        let index = usize::try_from(unit).expect("fits");
        if index < self.pending.len() && !self.pending[index] {
            self.pending[index] = true;
            self.offered += 1;
        }
    }

    fn backlog(&self) -> u32 {
        u32::try_from(self.pending.iter().filter(|pending| **pending).count()).expect("fits")
    }

    /// One tick's serving, in the round-robin order of item 60.
    fn serve(&mut self, out: &mut Vec<u32>) {
        out.clear();
        let count = u32::try_from(self.pending.len()).expect("fits");
        if count == 0 || self.cap == 0 {
            return;
        }
        let mut at = self.cursor % count;
        let mut offset = 0;
        while offset < count && u32::try_from(out.len()).expect("fits") < self.cap {
            let unit = at;
            at = (at + 1) % count;
            offset += 1;
            let index = usize::try_from(unit).expect("fits");
            if !self.pending[index] {
                continue;
            }
            self.pending[index] = false;
            self.served += 1;
            out.push(unit);
        }
        self.cursor = at;
        self.backlog_peak = self.backlog_peak.max(self.backlog());
    }
}

/// The demand one tick offers under G2's measured shape.
///
/// * **a burst of 74 in one tick**, the largest G2 saw, at one tick in 64 — the
///   burst *frequency* is the unmeasured constant item 69 names, and 1-in-64 is
///   the pessimistic end of its own sensitivity table;
/// * **9.45 repaths a tick** otherwise, G2's mean demand per crater at one
///   crater a tick, which is also the stability floor below which a cap never
///   drains. Nine, plus a tenth on nine ticks in twenty, is 9.45 in integers.
fn demand_at(tick: u32) -> i32 {
    if tick % 64 == 0 {
        74
    } else {
        9 + i32::from(tick % 20 < 9)
    }
}

#[test]
fn the_model_serves_what_the_router_serves() {
    // The model above is only worth reading if it is the same discipline. Two
    // hundred and twelve units all asking at once, and the two agree on every
    // unit of every tick.
    let mut world = harness_world(50);
    let mut model = CapQueue::new(world.router().len(), world.rules().repath_cap_per_tick());
    let mut unit = 0;
    while unit < world.router().len() {
        model.request(unit);
        unit += 1;
    }
    let mut enc = pharmakos_sim::encoding::Enc::with_capacity(64 * 1024);
    let mut served: Vec<u32> = Vec::new();
    let mut tick = 0;
    while tick < 16 {
        let _ = world.step(&mut enc);
        model.serve(&mut served);
        assert_eq!(
            world.router().served_this_tick(),
            served.as_slice(),
            "tick {tick}: the router and the model of its queue served different units"
        );
        // Whatever the world is waiting on after this tick, the model waits on
        // too, so the two queues stay in step.
        let mut unit = 0;
        while unit < world.router().len() {
            if world.router().state(unit) == WalkState::Waiting {
                model.request(unit);
            }
            unit += 1;
        }
        tick += 1;
    }
    assert!(model.served > 0, "nothing was served; the test is vacuous");
}

#[test]
fn the_repath_cap_is_sustainable_over_a_full_segment() {
    // Item 69: 16 is sustainable — the backlog does not grow over a full
    // segment and every request is eventually served — where 8 drops 14 % of
    // the work for ever and 32 misses the tick deadline. This asserts the first
    // half over the longest segment the ladder carries (8 minutes at 20 Hz), on
    // the demand shape G2 measured, and prints the `sustainable` boolean G3'
    // section 9.17 asks for: a sweep table without backlog growth in it
    // actively misleads.
    let units: u32 = 300; // item 63's world total
    let table = rules();
    let cap = table.repath_cap_per_tick();
    let segment = *table
        .segment_lengths_ms()
        .iter()
        .max()
        .expect("the ladder has a longest segment");
    let ticks = u32::try_from(segment).expect("fits").div_euclid(50);
    assert_eq!(
        ticks, 9_600,
        "the longest segment is eight minutes at 20 Hz"
    );

    let (sustainable, by_minute, peak, offered, served, drain) = run_segment(units, cap, ticks);
    println!(
        "::notice::repath cap {cap} over {ticks} ticks: sustainable={sustainable}, backlog per \
         minute {by_minute:?}, peak {peak}, offered {offered}, served {served}, drained in \
         {drain} ticks"
    );
    let first = by_minute.first().copied().unwrap_or(0);
    let last = by_minute.last().copied().unwrap_or(0);
    assert!(
        last <= first + 2,
        "the backlog grew over the segment: minute 1 mean {first}, minute 8 mean {last}"
    );
    assert_eq!(
        offered,
        served,
        "{} requests were never served; a cap below mean demand buys a cheap tick by not doing \
         the work (item 69)",
        offered - served
    );
    assert!(sustainable, "the cap is not sustainable at {cap}");

    // And the floor, so the assertion above is not vacuous: a cap of 8 is below
    // G2's mean demand of 9.45, and the same segment sits permanently loaded.
    //
    // **What starving looks like here is not an unbounded queue**, and the
    // difference is worth stating rather than asserting around. G3' swept an
    // open-ended demand stream, where a cap below the mean grows the backlog
    // without limit and 14 % of the work is dropped for ever. A request in
    // *this* sim is a flag on a unit, so a unit already waiting cannot ask
    // twice: with a fleet of 300 the queue self-limits, and what a starving cap
    // costs is a fifth of the fleet standing still and a fifth of the demand
    // absorbed by units that were already in the queue. That is the same
    // finding in the shape this design gives it, and it is what the two numbers
    // below assert.
    let (_, starved_minutes, starved_peak, starved_offered, starved_served, _) =
        run_segment(units, 8, ticks);
    let starved_mean = starved_minutes.iter().copied().max().unwrap_or(0);
    let absorbed = offered.saturating_sub(starved_offered);
    println!(
        "::notice::repath cap 8 over {ticks} ticks: backlog per minute {starved_minutes:?}, \
         peak {starved_peak}, offered {starved_offered} against {offered} at cap {cap} \
         ({absorbed} absorbed by units already waiting), served {starved_served}"
    );
    assert!(
        starved_mean >= last.saturating_mul(10).max(20),
        "a cap of 8 left a mean backlog of {starved_mean} against {last} at cap {cap}; the \
         sustainability assertion above proves nothing if the cap makes no difference"
    );
    assert!(
        absorbed.saturating_mul(100).div_euclid(offered.max(1)) >= 10,
        "a cap of 8 absorbed only {absorbed} of {offered} requests into units that were already \
         waiting; a cap below mean demand is supposed to buy its cheap tick by not doing the work"
    );
}

/// Run one segment of the demand model at a cap, and report what happened.
///
/// Returns `(sustainable, mean backlog per minute, peak backlog, offered,
/// served, ticks to drain)`.
fn run_segment(units: u32, cap: u32, ticks: u32) -> (bool, Vec<i64>, u32, u64, u64, u32) {
    let mut queue = CapQueue::new(units, cap);
    let mut rng = Rng::new(0x5EED_0CA9);
    let mut served: Vec<u32> = Vec::new();
    let mut by_minute: Vec<i64> = Vec::new();
    let mut minute_total: i64 = 0;
    let mut tick: u32 = 0;
    while tick < ticks {
        let demand = demand_at(tick);
        let mut n = 0;
        while n < demand {
            let unit = rng.below(i32::try_from(units).expect("fits"));
            queue.request(u32::try_from(unit).expect("below units"));
            n += 1;
        }
        queue.serve(&mut served);
        minute_total += i64::from(queue.backlog());
        tick += 1;
        if tick % 1_200 == 0 {
            by_minute.push(minute_total.div_euclid(1_200));
            minute_total = 0;
        }
    }

    // Draining: a sustainable cap empties the queue once demand stops, and that
    // tail is what "every request is eventually served" means.
    let mut drain = 0;
    while queue.backlog() > 0 && drain < units {
        queue.serve(&mut served);
        drain += 1;
    }

    let first = by_minute.first().copied().unwrap_or(0);
    let last = by_minute.last().copied().unwrap_or(0);
    let sustainable = last <= first + 2 && queue.offered == queue.served;
    (
        sustainable,
        by_minute,
        queue.backlog_peak,
        queue.offered,
        queue.served,
        drain,
    )
}

/// The determinism harness's world, at a chosen number of extra walkers.
fn harness_world(units_per_seat: u32) -> World {
    World::new(&WorldConfig {
        match_seed: SEED,
        seats: SEATS,
        units_per_seat,
        rules: rules(),
        match_settings: MatchSettings::default(),
    })
    .expect("the committed rules table describes a world")
}
