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
//! S3's plan adds E11's P3 accuracy gate (`p3_holds_on_real_commanders_*`):
//! the estimate against seat 0's commander walked through the tick, on three
//! seeds, never optimistic and within 15 % at p90 on the map-wide draw; and
//! the short routes' figures, reported and held to never optimistic only
//! (`p3_short_routes_are_reported_and_never_optimistic`). Certifying the
//! figures is `cert3`'s.
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
use pharmakos_sim::events::EventKind;
use pharmakos_sim::knowledge::{AssetId, Position};
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
use pharmakos_sim::tables::{SeatId, UnitId, UnitKind};
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

/// `rules` with each core's surplus raised to 100 kW and the map's power
/// ceiling to 1 000 kW, so that the raiders `World::field_movers_directly`
/// homes to the cores draw no seat dark: a dark core parks its units, and
/// these tests want a table that moves.
fn powered(rules: &RulesTable) -> RulesTable {
    let mut message = rules.message().clone();
    let power = message.power.as_mut().expect("a power block");
    power.core_surplus_kw = 100;
    power.map_ceiling_kw = 1_000;
    RulesTable::from_message(&message).expect("the powered table is a table")
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
        let clusters = Clusters::new(
            &surface,
            rules.cluster_shape().with_size(cluster),
            &mut scratch,
        )
        .expect("a decomposition");
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
        let shape = self.clusters.shape();
        let mut scratch = Scratch::for_map(&self.surface, shape.size).expect("a scratch");
        Clusters::new(&self.surface, shape, &mut scratch).expect("a decomposition")
    }

    fn commander_speed(&self) -> Speed {
        Speed::commander(&self.rules)
    }
}

/// Every `unit_sealed_in` line the world has emitted since the last drain, as
/// `(tick, subject, where it stopped)`.
///
/// Item 60's *report* half, which T10's match phase puts on the event bus.
/// Nothing in these tests drains the bus, so this is the whole history.
fn sealed_in_lines(world: &World) -> Vec<(u32, AssetId, Option<[Fx; 3]>)> {
    world
        .events()
        .iter()
        .filter(|event| event.kind == EventKind::UnitSealedIn)
        .map(|event| {
            (
                event.tick.raw(),
                event.subject.unwrap_or_default(),
                event.at.map(Position::to_array),
            )
        })
        .collect()
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
    pairs_on(&fixture.surface, &fixture.clusters, count, seed)
}

/// [`pairs`] over any graph: a [`World`]'s own, for the walks of P3's
/// accuracy gate. The same draw, so a fixture's pair set is unchanged.
fn pairs_on(surface: &Surface, clusters: &Clusters, count: u32, seed: u64) -> Vec<(Node, Node)> {
    let mut rng = Rng::new(seed);
    let extent = surface.size();
    let mut out: Vec<(Node, Node)> = Vec::with_capacity(usize::try_from(count).expect("fits"));
    let mut attempts = 0;
    while u32::try_from(out.len()).expect("fits") < count && attempts < count * 200 {
        attempts += 1;
        let a = surface.node_of(rng.below(extent[0]), rng.below(extent[1]));
        let b = surface.node_of(rng.below(extent[0]), rng.below(extent[1]));
        let (Some(a), Some(b)) = (a, b) else {
            continue;
        };
        if a == b || !clusters.connected(surface, a, b) {
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

/// Nearest-rank percentile over an already-sorted slice: the value at rank
/// `ceil(p × n / 100)`, counting from 1, at every `n` (p99 of 80 values is the
/// 80th). `p` of 0 reads the minimum.
fn percentile(sorted: &[i64], p: i64) -> i64 {
    if sorted.is_empty() {
        return 0;
    }
    let n = i64::try_from(sorted.len()).expect("fits");
    let rank = (p * n + 99).div_euclid(100);
    sorted[usize::try_from((rank - 1).clamp(0, n - 1)).expect("clamped")]
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
        )
        .ok();
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
        Err(pharmakos_sim::pathing::route::NoPath::Unreachable),
        "a sealed-in walker has no route either, and says why"
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
    let shape = fixture.rules.cluster_shape();
    let surface = &fixture.surface;
    let scratch = &mut fixture.scratch;
    assert!(
        Clusters::new(surface, shape.with_size(1), scratch).is_none(),
        "a cluster edge of 1 decomposes the 384-voxel map into 147 456 clusters, which the u16 \
         tags cannot name, and it was accepted"
    );
    assert!(
        Clusters::new(surface, shape.with_size(0), scratch).is_none(),
        "a cluster edge of zero was accepted"
    );
    let mut four = Scratch::for_map(surface, 4).expect("a scratch for a cluster edge of 4");
    assert!(
        Clusters::new(surface, shape.with_size(4), &mut four).is_some(),
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
        let row = rules.cost_per_second(kind);
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
        let _ = world.step(&mut enc).unwrap();
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
    let _ = world.step(&mut enc).unwrap();
    let _ = world.step(&mut enc).unwrap();
    // Seat 0's commander (unit 0), which no program moves, sent across the
    // map between ticks: the next tick serves its route, and it walks from
    // the tick after, as a harness walker's fresh destination used to.
    let unit = 0_u32;
    // The map's middle, far from every core.
    let far = world.standing_point([192, 192, 0]);
    world.send_unit_directly(unit, far);
    let _ = world.step(&mut enc).unwrap();
    assert_eq!(
        world.router().state(unit),
        WalkState::Walking,
        "the commander walks once its route is served"
    );
    let route = world.router().route(unit).to_vec();
    let kind = UnitKind::from_id(world.units().kinds()[usize::try_from(unit).expect("fits")])
        .expect("a unit kind");
    let speed = world.rules().cost_per_second(kind);
    let promised = walked_ticks(world.surface(), &route, speed).expect("the route is walkable");

    let mut ticks = 0;
    while world.router().route_cursor(unit) + 1 < u32::try_from(route.len()).expect("fits") {
        let _ = world.step(&mut enc).unwrap();
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
#[allow(
    clippy::too_many_lines,
    reason = "one case per construct or step, each the whole of what it checks; split, the halves would share every fixture"
)]
fn a_sealed_in_walker_parks_and_is_re_armed_when_the_graph_changes() {
    // Item 60's designed state: park and report, never a repath loop — and try
    // again exactly when the graph that refused the route has been repaired.
    let mut world = harness_world(0);
    let mut enc = pharmakos_sim::encoding::Enc::with_capacity(64 * 1024);
    let _ = world.step(&mut enc).unwrap();
    let _ = world.step(&mut enc).unwrap();

    // Wall seat 0's commander in (unit 0): a solid voxel five above each of
    // its eight neighbours' surfaces. The columns stay walkable, and every step
    // out of the middle one is a five-voxel climb, which the one-voxel rule
    // refuses. The commander, because no program sends it anywhere: with no
    // playbook it stands still until it is sent across the map below, while a
    // drone's program would walk it home and park it, and nothing has walked a
    // unit at random since S2-16 removed the harness's wander.
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
            // A supported column, not a voxel hung in air: a voxel written into
            // air with nothing under it falls (S2's plan, `fall`).
            for z in (top + 1)..=(top + 5) {
                wall.push([x + dx, y + dy, z]);
            }
        }
    }
    for at in &wall {
        assert!(
            world
                .request_voxel_edit(VoxelEdit::Set {
                    at: *at,
                    material: Material::STONE,
                    by: None,
                })
                .is_ok()
        );
    }
    // The map's middle, far from every core.
    let far = world.standing_point([192, 192, 0]);
    world.send_unit_directly(unit, far);
    let mut tick = 0;
    while tick < 12 {
        let _ = world.step(&mut enc).unwrap();
        tick += 1;
    }
    assert_eq!(
        world.router().state(unit),
        WalkState::Sealed,
        "a walker with no route must park as sealed in, not keep asking"
    );
    assert_eq!(world.sealed_units(), 1, "exactly one walker is sealed in");

    // The count above is the standing state. The *report* half of item 60's
    // "park and report" is the `unit_sealed_in` event T10's match phase emits
    // on the tick the unit parked — which is what discharges the PLACEHOLDER T7
    // left on `World::sealed_units`.
    let reported = sealed_in_lines(&world);
    assert_eq!(reported.len(), 1, "one walker parked, so one line was sent");
    assert_eq!(
        (reported[0].1, reported[0].2),
        (
            AssetId::of_unit(UnitId::new(unit)).expect("a unit id"),
            Some(world.units().positions()[0])
        ),
        "the line names the unit that parked and where it stopped — which is what the gateway's \
         fog filter decides on"
    );
    let parked_at = reported[0].0;

    let parked = world.router().served_total();
    let mut tick = 0;
    while tick < 25 {
        let _ = world.step(&mut enc).unwrap();
        tick += 1;
    }
    assert_eq!(
        world.router().served_total(),
        parked,
        "a sealed-in walker asked again while nothing changed; that is the repath loop item 60 \
         rules out"
    );
    let still: Vec<u32> = sealed_in_lines(&world)
        .into_iter()
        .map(|line| line.0)
        .collect();
    assert_eq!(
        still,
        vec![parked_at],
        "the report is the edge, not the state: it fires on the tick the walker parked and not \
         on the twenty-five after it"
    );

    // Take the wall down again: the repair publishes the clusters it rebuilt,
    // and the walker standing in one of them is re-armed.
    for at in &wall {
        assert!(
            world
                .request_voxel_edit(VoxelEdit::Set {
                    at: *at,
                    material: Material::AIR,
                    by: None,
                })
                .is_ok()
        );
    }
    let mut tick = 0;
    while tick < 12 {
        let _ = world.step(&mut enc).unwrap();
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
            assert!(
                world
                    .request_voxel_edit(VoxelEdit::Crater {
                        centre: [x, y, z],
                        radius: 3,
                        by: None,
                    })
                    .is_ok()
            );
        }
        let _ = world.step(&mut enc).unwrap();
        tick += 1;
    }
    let fingerprint = world.clusters().fingerprint();
    let snapshot = pharmakos_sim::Snapshot::capture(&world).expect("a world between ticks");
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
            world.step(&mut a).unwrap(),
            restored.step(&mut b).unwrap(),
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
    if tick.is_multiple_of(64) {
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
        let _ = world.step(&mut enc).unwrap();
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
        if tick.is_multiple_of(1_200) {
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

/// The determinism harness's world, with `movers` raiders a seat walking home
/// from across the map on a table that powers them
/// (`World::field_movers_directly`, which replaced the harness walkers when
/// S2-16 removed them).
fn harness_world(movers: u32) -> World {
    let mut world = World::new(&WorldConfig {
        match_seed: SEED,
        seats: SEATS,
        rules: if movers > 0 {
            powered(&rules())
        } else {
            rules()
        },
        match_settings: MatchSettings::default(),
    })
    .expect("the committed rules table describes a world");
    if movers > 0 {
        world
            .field_movers_directly(movers)
            .expect("the movers fit every seat's share");
    }
    world
}

// ---------------------------------------------------------------------------
// P3's accuracy on real commanders (S3's plan, E11 and section 5.1 item 3)
// ---------------------------------------------------------------------------
//
// Spec section 16's P3 row: the travel estimate is "within ±15% (p90) on
// static terrain", and item 61 makes it never optimistic. E11 asks for that to
// be checked against **real commanders walked through the tick**, not against
// this file's own integrator (`walked_ticks`, which
// `the_estimate_is_never_optimistic_at_any_percentile` uses), on three seeds.
//
// Two draws are walked. The **map-wide draw** (`pairs_on`, uniform over
// connected walkable columns, seeded per map) is the one the `p3_holds_*`
// tests hold to the spec's figure. On these maps it holds no route of 32 cells
// or less, so the **short draws** (`P3_SHORT_DRAWS`: rings of straight-line
// distance and a disc) are walked beside it and reported by
// `p3_short_routes_are_reported_and_never_optimistic`, which asserts what holds
// today and not the 15 %: see its doc comment.
//
// For each seed and draw one `World` is built as `harness_world(0)` builds one
// (three seats, the committed rules, no movers, no playbook sealed, so nothing
// digs, builds or fights), and seat 0's commander walks every pair in turn:
// `place_unit_directly` on the pair's start column, `send_unit_directly` to its
// goal column, then `World::step` until the router lists it among the tick's
// arrivals. The estimate is `pathing::estimate::estimate` over the world's own
// graph, unfogged, at `Speed::commander`, the call the gateway's route adapter
// makes. A digest of the surface and the abstract graph, taken before the first
// walk and after the last, shows the terrain was static.
//
// **Walked time is counted from the tick that serves the route**: the fixture
// sends between ticks, the next tick serves the route, and the walk is the
// number of `step` calls after that one, up to and including the tick the
// commander arrives on. That is the reading
// `a_walker_arrives_on_the_tick_the_estimate_promised` uses, and the estimate
// (item 90: arrival on tick `ceil(cost * 20 / cost_per_second)`) does not count
// the serving tick. The figure counted from the send (one tick more) is printed
// beside it and asserted on nothing.
//
// A walk that cannot finish (no estimate, a route the router does not serve, a
// partial route, a route that does not join the pair, an arrival later than
// `max(estimate, promise) + P3_LATE_TICKS`) is a failure of the test, named by
// seed, draw and pair index, never a skipped pair. Whether the walker arrived
// on the tick the integrator promised is its own named check.
//
// **Reading the figures.** `cargo test` swallows a passing test's output, so
// the `::notice::` lines are read by hand, and `cert3` records them this way:
//
//     cargo test -p pharmakos-sim --test pathing -- --nocapture

/// The three seeds of P3's figures, the golden seed first: restated from
/// `pharmakos-bench`'s `P3_SEEDS` (`crates/bench/src/p3.rs`), which the sim
/// cannot depend on, so the accuracy and the latency figures are about the
/// same three maps. Nothing holds the two lists together yet; that hold is
/// `cert3`'s, in `bench`.
const P3_SEEDS: [u64; 3] = [
    0x0000_0000_ca5c_aded,
    pharmakos_sim::DETERMINISM_MATCH_SEED,
    0x0000_0000_0000_0014,
];

/// Pairs walked per seed on the map-wide draw, chosen by measurement so that
/// one seed's walks stay under the target's slowest test.
const P3_WALKS: u32 = 80;

/// The map-wide draw's seed for [`pairs_on`], mixed with the map's seed so
/// each map walks its own pairs (the three maps are all walkable, so one fixed
/// draw would walk the same columns on each).
const P3_PAIR_DRAW: u64 = 0x5EED_0011;

/// The short draws' seed, mixed with the map's seed and the draw's index.
const P3_SHORT_DRAW: u64 = 0x5EED_0012;

/// Pairs walked per seed on each short draw. Short walks are cheap; the pull
/// request that set it records what it costs.
const P3_SHORT_WALKS: u32 = 60;

/// Spec section 16 and item 61: the p90 of |estimate − walked| over walked, per
/// mille.
const P3_BUDGET_PER_MILLE: i64 = 150;

/// How far past `max(estimate, promise)` a walk may run before it is a
/// failure: the margin the single-walker test uses.
const P3_LATE_TICKS: i32 = 2;

/// The seat whose commander walks: seat 0's, which nothing but this test moves
/// in a world with no playbook sealed.
const P3_SEAT: u8 = 0;

/// Which pairs a set of walks draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Draw {
    /// [`pairs_on`]: uniform over connected walkable columns.
    MapWide,
    /// Uniform over the ring `inner < distance ≤ outer`, in cells of
    /// straight-line distance between the pair's columns.
    Ring { inner: i32, outer: i32 },
    /// Uniform over the whole disc `0 < distance ≤ outer`: the same draw as
    /// the ring from 0, kept apart so the bands and the disc are read apart.
    Disc { outer: i32 },
}

impl Draw {
    /// The ring a short draw samples, `(inner, outer)`; `None` for the
    /// map-wide draw.
    const fn ring(self) -> Option<(i32, i32)> {
        match self {
            Draw::MapWide => None,
            Draw::Ring { inner, outer } => Some((inner, outer)),
            Draw::Disc { outer } => Some((0, outer)),
        }
    }
}

impl std::fmt::Display for Draw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Draw::MapWide => write!(f, "the map-wide draw"),
            Draw::Ring { inner, outer } => write!(f, "routes of {inner} to {outer} cells"),
            Draw::Disc { outer } => write!(f, "the disc of {outer} cells"),
        }
    }
}

/// The short draws: the spike's distance bands below 64 cells
/// (`docs/spikes/G2-P3-pathing.md` section 9 splits at 32), and a draw uniform
/// over the whole 32-cell disc.
const P3_SHORT_DRAWS: [Draw; 6] = [
    Draw::Ring { inner: 0, outer: 4 },
    Draw::Ring { inner: 4, outer: 8 },
    Draw::Ring {
        inner: 8,
        outer: 16,
    },
    Draw::Ring {
        inner: 16,
        outer: 32,
    },
    Draw::Ring {
        inner: 32,
        outer: 64,
    },
    Draw::Disc { outer: 32 },
];

/// `count` connected pairs whose straight-line distance lies in
/// `inner < d ≤ outer`, uniform over the ring, deterministic in `seed`. Fewer
/// when the map does not yield them; the caller reports the shortfall.
fn ring_pairs(
    surface: &Surface,
    clusters: &Clusters,
    count: u32,
    seed: u64,
    (inner, outer): (i32, i32),
) -> Vec<(Node, Node)> {
    let mut rng = Rng::new(seed);
    let extent = surface.size();
    let mut out: Vec<(Node, Node)> = Vec::with_capacity(usize::try_from(count).expect("fits"));
    let mut attempts: u32 = 0;
    while u32::try_from(out.len()).expect("fits") < count && attempts < count * 2_000 {
        attempts += 1;
        let ax = rng.below(extent[0]);
        let ay = rng.below(extent[1]);
        let dx = rng.below(2 * outer + 1) - outer;
        let dy = rng.below(2 * outer + 1) - outer;
        let d2 = dx * dx + dy * dy;
        if d2 <= inner * inner || d2 > outer * outer {
            continue;
        }
        let (Some(a), Some(b)) = (surface.node_of(ax, ay), surface.node_of(ax + dx, ay + dy))
        else {
            continue;
        };
        if a == b || !clusters.connected(surface, a, b) {
            continue;
        }
        out.push((a, b));
    }
    out
}

/// One finished walk.
#[derive(Clone, Copy, Debug)]
struct Walk {
    /// The map's seed.
    seed: u64,
    /// The pair's index in its draw.
    pair: usize,
    /// `estimate(...).ticks`.
    estimate: i32,
    /// Ticks from the serving tick to the arrival.
    walked: i32,
    /// `walked_ticks` over the served route: the integrator's promise.
    promised: i32,
    /// Whether the served route climbs or descends anywhere (R10's slopes).
    climbs: bool,
}

/// Why a walk, or a seed's set-up, could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
enum WalkFailure {
    /// The world, or a scratch for its graph, would not build.
    NoWorld,
    /// Seat [`P3_SEAT`] has no commander, or the router walks commanders at
    /// another speed than `Speed::commander` prices them at.
    NotTheCommander,
    /// The draw yielded fewer pairs than asked for.
    TooFewPairs { found: usize },
    /// The world refused a tick; the seed's walks stop there.
    Tick { pair: usize, error: String },
    /// The estimator had no answer for a pair the oracle connects.
    NoEstimate { pair: usize },
    /// The commander could not be placed.
    Unplaced { pair: usize },
    /// The serving tick left the commander in another state than walking.
    NotServed { pair: usize, state: WalkState },
    /// The served route stops short of the goal.
    Partial { pair: usize },
    /// The served route does not run from the pair's start to its goal.
    WrongRoute { pair: usize },
    /// The served route has a step the surface does not price.
    Unwalkable { pair: usize },
    /// The commander had not arrived by `max(estimate, promise)` plus the
    /// margin.
    Late { pair: usize, ticks: i32, bound: i32 },
    /// The ground moved during the walks: the terrain was not static.
    TerrainMoved,
}

/// Every walk of one seed's draws, and every walk that could not be read.
#[derive(Debug)]
struct SeedWalks {
    seed: u64,
    walks: Vec<(Draw, Walk)>,
    /// `None` for the seed's set-up, else the draw the failure is in.
    failures: Vec<(Option<Draw>, WalkFailure)>,
}

impl SeedWalks {
    fn walks_of(&self, draw: Draw) -> Vec<Walk> {
        self.walks
            .iter()
            .filter(|(of, _)| *of == draw)
            .map(|(_, walk)| *walk)
            .collect()
    }

    fn failures_of(&self, draw: Draw) -> Vec<&WalkFailure> {
        self.failures
            .iter()
            .filter(|(of, _)| of.is_none_or(|of| of == draw))
            .map(|(_, failure)| failure)
            .collect()
    }
}

/// A digest of the world's surface and abstract graph, so the walks can show
/// the ground stood still under them.
fn terrain_digest(world: &World) -> u64 {
    let surface = world.surface();
    let mut h = world.clusters().fingerprint();
    let mut node: Node = 0;
    while node < surface.node_count() {
        let top = u64::from(surface.top(node).cast_unsigned());
        let walkable = u64::from(surface.walkable(node));
        h = pharmakos_sim::math::random::mix64(h ^ ((top << 1) | walkable));
        node += 1;
    }
    h
}

/// A world with seat 0's commander ready to walk.
struct Walker {
    seed: u64,
    world: World,
    enc: pharmakos_sim::encoding::Enc,
    scratch: Scratch,
    commander: UnitId,
    unit: u32,
    speed: Speed,
}

impl Walker {
    fn new(match_seed: u64) -> Result<Walker, WalkFailure> {
        let mut world = World::new(&WorldConfig {
            match_seed,
            seats: SEATS,
            rules: rules(),
            match_settings: MatchSettings::default(),
        })
        .map_err(|_| WalkFailure::NoWorld)?;
        let mut enc = pharmakos_sim::encoding::Enc::with_capacity(64 * 1024);
        // The two opening ticks the single-walker test steps before its send.
        for _ in 0..2 {
            world.step(&mut enc).map_err(|error| WalkFailure::Tick {
                pair: 0,
                error: format!("{error:?}"),
            })?;
        }
        let speed = Speed::commander(world.rules());
        // The commander's id for the fixture calls, and its row for the
        // router's.
        let (commander, unit) = world
            .commander_of(SeatId::new(P3_SEAT))
            .and_then(|id| Some((id, world.units().row_of(id).ok()?.raw())))
            .ok_or(WalkFailure::NotTheCommander)?;
        let kind = world
            .units()
            .kinds()
            .get(usize::try_from(unit).expect("fits"))
            .copied()
            .and_then(UnitKind::from_id);
        if kind != Some(UnitKind::Commander)
            || world.router().speed_of(UnitKind::Commander) != speed.cost_per_second
        {
            return Err(WalkFailure::NotTheCommander);
        }
        let scratch = Scratch::for_map(world.surface(), world.clusters().size())
            .ok_or(WalkFailure::NoWorld)?;
        Ok(Walker {
            seed: match_seed,
            world,
            enc,
            scratch,
            commander,
            unit,
            speed,
        })
    }

    fn step(&mut self, pair: usize) -> Result<(), WalkFailure> {
        self.world
            .step(&mut self.enc)
            .map(|_| ())
            .map_err(|error| WalkFailure::Tick {
                pair,
                error: format!("{error:?}"),
            })
    }

    /// Walk one pair: estimate it, send the commander, count the ticks.
    fn walk(&mut self, pair: usize, start: Node, goal: Node) -> Result<Walk, WalkFailure> {
        let estimated = estimate(
            self.world.surface(),
            self.world.clusters(),
            &mut self.scratch,
            start,
            goal,
            Fog::Clear,
            self.speed,
        )
        .ok_or(WalkFailure::NoEstimate { pair })?;
        let (sx, sy) = self.world.surface().coord_of(start);
        let (gx, gy) = self.world.surface().coord_of(goal);
        let from = self.world.standing_point([sx, sy, 0]);
        let to = self.world.standing_point([gx, gy, 0]);
        if !self.world.place_unit_directly(self.commander, from) {
            return Err(WalkFailure::Unplaced { pair });
        }
        self.world.send_unit_directly(self.unit, to);
        // The serving tick.
        self.step(pair)?;
        let state = self.world.router().state(self.unit);
        if state != WalkState::Walking {
            return Err(WalkFailure::NotServed { pair, state });
        }
        if self.world.router().route_partial(self.unit) {
            return Err(WalkFailure::Partial { pair });
        }
        let route = self.world.router().route(self.unit).to_vec();
        if route.first() != Some(&start) || route.last() != Some(&goal) {
            return Err(WalkFailure::WrongRoute { pair });
        }
        let surface = self.world.surface();
        let promised = walked_ticks(surface, &route, self.speed.cost_per_second)
            .ok_or(WalkFailure::Unwalkable { pair })?;
        let climbs = route
            .windows(2)
            .any(|step| surface.top(step[0]) != surface.top(step[1]));
        // The bound is not the integrator's alone, so that a walker slower
        // than the integrator but inside a pessimistic estimate is read, and
        // named by the integrator check rather than reported as unreadable.
        let bound = estimated.ticks.max(promised) + P3_LATE_TICKS;
        let mut ticks: i32 = 0;
        loop {
            self.step(pair)?;
            ticks += 1;
            if self.world.router().arrived().contains(&self.unit) {
                break;
            }
            if ticks > bound {
                return Err(WalkFailure::Late { pair, ticks, bound });
            }
        }
        Ok(Walk {
            seed: self.seed,
            pair,
            estimate: estimated.ticks,
            walked: ticks,
            promised,
            climbs,
        })
    }
}

/// Walk each of `draws` on map `seed` with seat 0's commander, `count(draw)`
/// pairs each. What goes wrong with a walk is a [`WalkFailure`], never a panic.
fn walk_seed(seed: u64, draws: &[Draw]) -> SeedWalks {
    let mut out = SeedWalks {
        seed,
        walks: Vec::new(),
        failures: Vec::new(),
    };
    let mut walker = match Walker::new(seed) {
        Ok(walker) => walker,
        Err(failure) => {
            out.failures.push((None, failure));
            return out;
        }
    };
    let before = terrain_digest(&walker.world);
    for (index, draw) in draws.iter().copied().enumerate() {
        let surface = walker.world.surface();
        let clusters = walker.world.clusters();
        let (set, asked) = match draw.ring() {
            None => (
                pairs_on(surface, clusters, P3_WALKS, P3_PAIR_DRAW ^ seed),
                P3_WALKS,
            ),
            Some((inner, outer)) => {
                let mixed = P3_SHORT_DRAW ^ seed ^ (u64::try_from(index).expect("fits") << 56);
                (
                    ring_pairs(surface, clusters, P3_SHORT_WALKS, mixed, (inner, outer)),
                    P3_SHORT_WALKS,
                )
            }
        };
        if set.len() != usize::try_from(asked).expect("fits") {
            out.failures
                .push((Some(draw), WalkFailure::TooFewPairs { found: set.len() }));
        }
        for (pair, (start, goal)) in set.iter().copied().enumerate() {
            match walker.walk(pair, start, goal) {
                Ok(walk) => out.walks.push((draw, walk)),
                Err(failure @ WalkFailure::Tick { .. }) => {
                    out.failures.push((Some(draw), failure));
                    return out;
                }
                Err(failure) => out.failures.push((Some(draw), failure)),
            }
        }
    }
    if terrain_digest(&walker.world) != before {
        out.failures.push((None, WalkFailure::TerrainMoved));
    }
    out
}

/// Each seed's map-wide walks, made once and shared by the per-seed, pooled
/// and control tests that read them.
fn seed_walks(index: usize) -> &'static SeedWalks {
    static WALKS: [std::sync::OnceLock<SeedWalks>; 3] = [const { std::sync::OnceLock::new() }; 3];
    WALKS[index].get_or_init(|| walk_seed(P3_SEEDS[index], &[Draw::MapWide]))
}

/// The percentiles of one set of per-mille errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Spread {
    count: usize,
    min: i64,
    p50: i64,
    p90: i64,
    p99: i64,
    max: i64,
}

impl Spread {
    fn of(mut values: Vec<i64>) -> Spread {
        values.sort_unstable();
        Spread {
            count: values.len(),
            min: values.first().copied().unwrap_or(0),
            p50: percentile(&values, 50),
            p90: percentile(&values, 90),
            p99: percentile(&values, 99),
            max: values.last().copied().unwrap_or(0),
        }
    }
}

impl std::fmt::Display for Spread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.count == 0 {
            return write!(f, "empty");
        }
        write!(
            f,
            "n {}, min {}, p50 {}, p90 {}, p99 {}, max {}",
            self.count, self.min, self.p50, self.p90, self.p99, self.max
        )
    }
}

/// `(estimate − walked) × 1000 / walked`, the walked ticks the denominator as in
/// `the_estimate_is_never_optimistic_at_any_percentile`; `extra` ticks are
/// added to the walk (1 for the figure counted from the send).
fn signed_per_mille(walk: &Walk, extra: i32) -> i64 {
    let walked = i64::from(walk.walked) + i64::from(extra);
    ((i64::from(walk.estimate) - walked) * 1000).div_euclid(walked.max(1))
}

/// What the gate says about a set of walks: the optimistic ones by seed and
/// pair, and the p90 of the absolute error per mille.
#[derive(Debug, PartialEq, Eq)]
struct Verdict {
    optimistic: Vec<(u64, usize)>,
    abs_p90: i64,
}

impl Verdict {
    fn of(walks: &[Walk]) -> Verdict {
        let optimistic = walks
            .iter()
            .filter(|walk| walk.estimate < walk.walked)
            .map(|walk| (walk.seed, walk.pair))
            .collect();
        let mut absolute: Vec<i64> = walks
            .iter()
            .map(|walk| signed_per_mille(walk, 0).abs())
            .collect();
        absolute.sort_unstable();
        Verdict {
            optimistic,
            abs_p90: percentile(&absolute, 90),
        }
    }

    /// The gate: never optimistic, and the p90 within the budget.
    fn passes(&self) -> bool {
        self.optimistic.is_empty() && self.abs_p90 <= P3_BUDGET_PER_MILLE
    }
}

/// The walks that did not arrive on the tick the integrator promised over
/// their served route, as `(seed, pair, walked, promised)`: item 90's "the two
/// computations of when are the same number", for every walk.
fn off_the_integrator(walks: &[Walk]) -> Vec<(u64, usize, i32, i32)> {
    walks
        .iter()
        .filter(|walk| walk.walked != walk.promised)
        .map(|walk| (walk.seed, walk.pair, walk.walked, walk.promised))
        .collect()
}

/// The `::notice::` line for a set of walks.
fn p3_notice(label: &str, walks: &[Walk]) -> String {
    let signed = |extra: i32| {
        Spread::of(
            walks
                .iter()
                .map(|walk| signed_per_mille(walk, extra))
                .collect(),
        )
    };
    let count = i64::try_from(walks.len()).expect("fits").max(1);
    let total: i64 = walks.iter().map(|walk| i64::from(walk.walked)).sum();
    let climbing = walks.iter().filter(|walk| walk.climbs).count();
    if walks.is_empty() {
        return format!("::notice::P3 accuracy on real commanders, {label}: empty");
    }
    format!(
        "::notice::P3 accuracy on real commanders, {label}: {} walks, {climbing} with a climb, \
         mean {} ticks walked; signed error per mille from the serve: {}; |error| p90 {}; from \
         the send: {}",
        walks.len(),
        total.div_euclid(count),
        signed(0),
        Verdict::of(walks).abs_p90,
        signed(1),
    )
}

/// The walks' failures and integrator mismatches, as one report per problem.
fn unread(label: &str, failures: &[&WalkFailure], walks: &[Walk]) -> Vec<String> {
    let mut problems: Vec<String> = Vec::new();
    if !failures.is_empty() {
        problems.push(format!(
            "{label}: {} walk(s) could not be read: {failures:?}",
            failures.len()
        ));
    }
    let off = off_the_integrator(walks);
    if !off.is_empty() {
        problems.push(format!(
            "{label}: the walker did not arrive on the tick the integrator promised, as (seed, \
             pair, walked, promised): {off:?}"
        ));
    }
    problems
}

/// One seed's gate on the map-wide draw: every walk read, the walker on the
/// integrator's tick, and the verdict passing.
fn assert_seed(index: usize) {
    let walks = seed_walks(index);
    let label = format!("seed {:#018x}, {}", walks.seed, Draw::MapWide);
    let mine = walks.walks_of(Draw::MapWide);
    let problems = unread(&label, &walks.failures_of(Draw::MapWide), &mine);
    assert!(problems.is_empty(), "{problems:#?}");
    assert_eq!(
        mine.len(),
        usize::try_from(P3_WALKS).expect("fits"),
        "{label}: a walk went missing"
    );
    println!("{}", p3_notice(&label, &mine));
    let verdict = Verdict::of(&mine);
    assert!(
        verdict.passes(),
        "{label} misses P3 (never optimistic, p90 |error| at most {P3_BUDGET_PER_MILLE} per \
         mille): optimistic on (seed, pair) {:?}, p90 {} per mille",
        verdict.optimistic,
        verdict.abs_p90
    );
}

#[test]
fn p3_holds_on_real_commanders_on_the_golden_seed() {
    assert_seed(0);
}

#[test]
fn p3_holds_on_real_commanders_on_the_determinism_seed() {
    assert_seed(1);
}

#[test]
fn p3_holds_on_real_commanders_on_probations_seed() {
    assert_seed(2);
}

#[test]
fn p3_holds_on_real_commanders_over_the_three_seeds_pooled() {
    let mut pooled: Vec<Walk> = Vec::new();
    let mut problems: Vec<String> = Vec::new();
    for index in 0..P3_SEEDS.len() {
        let walks = seed_walks(index);
        let mine = walks.walks_of(Draw::MapWide);
        problems.extend(unread(
            &format!("seed {:#018x}", walks.seed),
            &walks.failures_of(Draw::MapWide),
            &mine,
        ));
        pooled.extend_from_slice(&mine);
    }
    assert!(problems.is_empty(), "{problems:#?}");
    println!(
        "{}",
        p3_notice(&format!("three seeds pooled, {}", Draw::MapWide), &pooled)
    );
    let verdict = Verdict::of(&pooled);
    assert!(
        verdict.passes(),
        "the pooled map-wide walks miss P3: optimistic on (seed, pair) {:?}, p90 {} per mille \
         (budget {P3_BUDGET_PER_MILLE})",
        verdict.optimistic,
        verdict.abs_p90
    );
}

#[test]
fn p3_catches_an_optimistic_or_a_loose_estimate() {
    // The control: the verdict `assert_seed` and the pooled test use can fail.
    // Without it, a harness that walked nothing, or read every walk as exact,
    // would pass. The golden seed's real walks, their estimates replaced.
    let walks = seed_walks(0).walks_of(Draw::MapWide);
    assert!(!walks.is_empty(), "the control has no walks to read");
    // One tick short of every walk: every pair is optimistic.
    let short: Vec<Walk> = walks
        .iter()
        .map(|walk| Walk {
            estimate: walk.walked - 1,
            ..*walk
        })
        .collect();
    let verdict = Verdict::of(&short);
    assert_eq!(
        verdict.optimistic.len(),
        walks.len(),
        "an estimate one tick short of the walk went unnoticed"
    );
    assert!(!verdict.passes());
    // Never optimistic, but twice the walk: the p90 is 1000 per mille.
    let loose: Vec<Walk> = walks
        .iter()
        .map(|walk| Walk {
            estimate: walk.walked * 2,
            ..*walk
        })
        .collect();
    let verdict = Verdict::of(&loose);
    assert!(verdict.optimistic.is_empty());
    assert_eq!(verdict.abs_p90, 1000);
    assert!(!verdict.passes(), "a 100 % error passed the 15 % budget");
    // A walker one tick off the integrator is named.
    let slow: Vec<Walk> = walks
        .iter()
        .map(|walk| Walk {
            walked: walk.promised + 1,
            ..*walk
        })
        .collect();
    assert_eq!(off_the_integrator(&slow).len(), walks.len());
}

/// Short routes, walked by real commanders and **reported**, not held to the
/// gate's 15 %.
///
/// The map-wide draw holds no route of 32 cells or less on these maps, and
/// short routes **miss the gate's 150 per mille today**. Measured at this
/// test's draw (`P3_SHORT_WALKS` = 60 pairs a draw on each seed, counted from
/// the serving tick, measured 2026-11-18 on Windows and WSL2 alike), the p90
/// |error| per mille by seed (`0xca5caded`, `DETERMINISM_MATCH_SEED`, 20) and
/// pooled, and the pooled maximum:
///
/// | draw (straight-line cells) | p90 by seed | p90 pooled | max pooled |
/// |---|---|---|---|
/// | 0 to 4           | 0 / 0 / 250       | 60  | 5333 |
/// | 4 to 8           | 555 / 1195 / 710  | 822 | 2035 |
/// | 8 to 16          | 280 / 314 / 379   | 335 | 964  |
/// | 16 to 32         | 229 / 178 / 270   | 229 | 473  |
/// | 32 to 64         | 79 / 59 / 93      | 81  | 167  |
/// | the 32-cell disc | 202 / 222 / 241   | 234 | 1243 |
///
/// Every band at 32 cells and under misses 150 at p90 on some seed; only the
/// 32-to-64 band is inside it. The estimate was never optimistic on any walk.
///
/// So this asserts only what holds today: every walk finishes (typed failures,
/// none skipped), the walker arrives on the integrator's tick, and the estimate
/// is never optimistic on any walk. Lane `p3fix` (`crates/sim/src/pathing`)
/// fixes the estimator's short routes and makes the 150 per mille bound an
/// assertion here. Read the figures with
/// `cargo test -p pharmakos-sim --test pathing -- --nocapture`.
#[test]
fn p3_short_routes_are_reported_and_never_optimistic() {
    static SHORT: [std::sync::OnceLock<SeedWalks>; 3] = [const { std::sync::OnceLock::new() }; 3];
    // The three maps' walks on three threads: each thread has its own world.
    std::thread::scope(|scope| {
        for index in 0..P3_SEEDS.len() {
            scope.spawn(move || {
                SHORT[index].get_or_init(|| walk_seed(P3_SEEDS[index], &P3_SHORT_DRAWS));
            });
        }
    });
    let mut problems: Vec<String> = Vec::new();
    for draw in P3_SHORT_DRAWS {
        let mut pooled: Vec<Walk> = Vec::new();
        for slot in &SHORT {
            let Some(walks) = slot.get() else {
                problems.push(format!("{draw}: a seed's walks were never made"));
                continue;
            };
            let label = format!("seed {:#018x}, {draw}", walks.seed);
            let mine = walks.walks_of(draw);
            problems.extend(unread(&label, &walks.failures_of(draw), &mine));
            println!("{}", p3_notice(&label, &mine));
            pooled.extend_from_slice(&mine);
        }
        println!(
            "{}",
            p3_notice(&format!("three seeds pooled, {draw}"), &pooled)
        );
        let verdict = Verdict::of(&pooled);
        if !verdict.optimistic.is_empty() {
            problems.push(format!(
                "{draw}: the estimate was optimistic on (seed, pair) {:?}",
                verdict.optimistic
            ));
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

// ---------------------------------------------------------------------------
// The graph is settled in the phase that edits the terrain (lane `settle`)
// ---------------------------------------------------------------------------
//
// A tick's Voxels phase writes the terrain and refreshes the surface; the
// clusters an edit dirtied are repaired, and the walkers it sealed in re-armed,
// before the phase ends. So no later phase, no next tick, no recap, no Lull and
// no restore reads a cluster graph that disagrees with the surface, and a world
// saved after any tick resumes onto the chain the live world plays.

/// Wall seat 0's commander in: a solid voxel five above each of its eight
/// neighbours' surfaces, every step out a five-voxel climb. Returns the wall.
fn wall_in_commander(world: &mut World) -> Vec<[i32; 3]> {
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
            for z in (top + 1)..=(top + 5) {
                wall.push([x + dx, y + dy, z]);
            }
        }
    }
    file_wall(world, &wall, Material::STONE);
    wall
}

/// File one `Set` per voxel of `wall`.
fn file_wall(world: &mut World, wall: &[[i32; 3]], material: Material) {
    for at in wall {
        assert!(
            world
                .request_voxel_edit(VoxelEdit::Set {
                    at: *at,
                    material,
                    by: None,
                })
                .is_ok(),
            "the edit queue has room"
        );
    }
}

/// A world restored from `world`'s snapshot, on a fresh world built the same
/// way: what a resumed match starts from.
fn resumed_from(world: &World) -> World {
    let bytes = pharmakos_sim::snapshot::Snapshot::capture(world)
        .expect("a world between ticks")
        .to_bytes()
        .expect("a snapshot encodes");
    let mut resumed = harness_world(0);
    pharmakos_sim::snapshot::Snapshot::from_bytes(&bytes)
        .expect("a snapshot decodes")
        .restore_into(&mut resumed)
        .expect("a snapshot restores");
    assert_eq!(resumed.state_hash(), world.state_hash());
    resumed
}

/// The first of `ticks` ticks on which `live` and `resumed` hash apart.
fn first_parting(live: &mut World, resumed: &mut World, ticks: u32) -> Option<u32> {
    let mut enc = pharmakos_sim::encoding::Enc::with_capacity(64 * 1024);
    (1..=ticks).find(|_| live.step(&mut enc).unwrap() != resumed.step(&mut enc).unwrap())
}

/// The walled-in commander: sealed, and its wall comes down in tick t's
/// Voxels phase; the world is saved after tick t. The live world and the one
/// resumed from that save play the same chain from tick t + 1. Before lane
/// `settle` the live world repaired and re-armed the commander in tick t + 1's
/// Pathing phase while the resumed one, its graph rebuilt fresh with nothing
/// marked, left it sealed: the chains parted on the first tick after the save.
/// The control takes the wall down after the save, on both sides.
#[test]
fn a_world_saved_after_a_terrain_edit_resumes_onto_the_live_chain() {
    for wall_down_before_the_save in [true, false] {
        let mut world = harness_world(0);
        let mut enc = pharmakos_sim::encoding::Enc::with_capacity(64 * 1024);
        let _ = world.step(&mut enc).unwrap();
        let _ = world.step(&mut enc).unwrap();
        let wall = wall_in_commander(&mut world);
        let far = world.standing_point([192, 192, 0]);
        world.send_unit_directly(0, far);
        for _ in 0..12 {
            let _ = world.step(&mut enc).unwrap();
        }
        assert_eq!(
            world.router().state(0),
            WalkState::Sealed,
            "the wall seals it in"
        );
        if wall_down_before_the_save {
            file_wall(&mut world, &wall, Material::AIR);
        }
        // Tick t.
        let _ = world.step(&mut enc).unwrap();
        let mut resumed = resumed_from(&world);
        if !wall_down_before_the_save {
            file_wall(&mut world, &wall, Material::AIR);
            file_wall(&mut resumed, &wall, Material::AIR);
        }
        assert_eq!(
            first_parting(&mut world, &mut resumed, 40),
            None,
            "the resumed world left the live chain (wall down before the save: \
             {wall_down_before_the_save})"
        );
        assert_eq!(
            world.router().state(0),
            WalkState::Walking,
            "the commander walks once its wall is down"
        );
    }
}

/// A read in the tick after a crater sees the repaired graph: between tick t,
/// whose Voxels phase craters seat 0's route to its nearest vent, and tick
/// t + 1's Decision phase, the world's clusters are the ones a fresh build
/// over the cratered surface makes, and the estimate and the "nearest" the
/// Decision phase would read are the answers on that fresh graph. Before lane
/// `settle` the window's estimate was 1 014 against 392 after the repair.
#[test]
fn the_tick_after_a_crater_reads_the_repaired_graph() {
    use pharmakos_sim::features::FeatureKind;
    use pharmakos_sim::targeting::{FeatureSpec, Ground, cover};
    let mut world = harness_world(0);
    let mut enc = pharmakos_sim::encoding::Enc::with_capacity(64 * 1024);
    let _ = world.step(&mut enc).unwrap();
    let _ = world.step(&mut enc).unwrap();
    // Seat 0's commander stands at (24, 356); its nearest vent is (52, 335).
    // A crater of radius 3 at (30, 350), on its route, rescans a border.
    let z = world.voxels().top_solid_z(30, 350).expect("a column");
    assert!(
        world
            .request_voxel_edit(VoxelEdit::Crater {
                centre: [30, 350, z],
                radius: 3,
                by: None,
            })
            .is_ok()
    );
    // Tick t.
    let _ = world.step(&mut enc).unwrap();
    let surface = world.surface().clone();
    let mut scratch = Scratch::for_map(&surface, world.clusters().size()).expect("a scratch");
    let fresh = Clusters::new(&surface, world.clusters().shape(), &mut scratch).expect("a graph");
    let speed = Speed::commander(world.rules());
    let start = surface.node_of(24, 356).expect("the commander's column");
    let goal = surface.node_of(52, 335).expect("the vent's column");
    let read = estimate(
        &surface,
        world.clusters(),
        &mut scratch,
        start,
        goal,
        Fog::Clear,
        speed,
    );
    let settled = estimate(
        &surface,
        &fresh,
        &mut scratch,
        start,
        goal,
        Fog::Clear,
        speed,
    );
    assert_eq!(
        read, settled,
        "the estimate read in the window is not the settled one"
    );
    let commander = world.units().positions()[0];
    let spec = FeatureSpec::Nearest {
        kind: FeatureKind::Vent,
        uncovered: true,
    };
    let in_window = cover(
        &world.ground(),
        &mut scratch,
        SeatId::new(0),
        commander,
        spec,
    );
    let on_fresh = cover(
        &Ground {
            clusters: &fresh,
            ..world.ground()
        },
        &mut scratch,
        SeatId::new(0),
        commander,
        spec,
    );
    assert_eq!(
        in_window, on_fresh,
        "the nearest read in the window is not the settled one"
    );
    assert_eq!(
        world.clusters().fingerprint(),
        fresh.fingerprint(),
        "the graph after the editing tick is not the graph the terrain makes"
    );
}

/// The match's own save point: the Push's last tick takes the wall down, the
/// recap closes, the Lull opens and the match is saved there. The live match
/// and the one resumed from that save play the same next Push.
#[test]
fn a_lull_saved_after_a_last_tick_edit_resumes_onto_the_live_chain() {
    use pharmakos_sim::runner::{MatchPhase, Runner};
    let mut world = harness_world(0);
    let mut enc = pharmakos_sim::encoding::Enc::with_capacity(64 * 1024);
    let _ = world.step(&mut enc).unwrap();
    let mut runner = Runner::new(world);
    assert!(runner.begin_push().unwrap(), "the first Push opens");
    let wall = wall_in_commander(runner.world_mut());
    let far = runner.world().standing_point([192, 192, 0]);
    runner.world_mut().send_unit_directly(0, far);
    for _ in 0..12 {
        let _ = runner.step().unwrap();
        runner.clear_events();
    }
    assert_eq!(runner.world().router().state(0), WalkState::Sealed);
    // The tick the segment ends on.
    let last = runner.world().match_state().segment_started().raw()
        + runner.world().match_state().segment_ticks();
    let mut filed_on_last = false;
    while runner.phase() == MatchPhase::Push {
        if runner.tick().raw() + 1 == last {
            file_wall(runner.world_mut(), &wall, Material::AIR);
            filed_on_last = true;
        }
        let _ = runner.step().unwrap();
        runner.clear_events();
    }
    assert!(filed_on_last, "the wall came down on the Push's last tick");
    assert_eq!(
        runner.tick().raw(),
        last,
        "the Push ended on the tick named"
    );
    while runner.phase() == MatchPhase::Recap {
        assert!(runner.end_recap().unwrap());
    }
    assert_eq!(
        runner.phase(),
        MatchPhase::Lull,
        "the Lull is the save point"
    );
    let snapshot = runner.capture().expect("a Lull is between ticks");
    let mut resumed = Runner::new(harness_world(0));
    resumed.restore(&snapshot).expect("the save restores");
    assert!(runner.begin_push().unwrap());
    assert!(resumed.begin_push().unwrap());
    for tick in 1..=40 {
        let live = runner.step().unwrap();
        let again = resumed.step().unwrap();
        assert_eq!(
            live, again,
            "the resumed match left the live chain on tick {tick} of the Push"
        );
    }
}

/// A coordinate on an axis of `extent` cells, drawn so that cluster borders,
/// cluster corners and the map's edges come up often: a quarter of draws land
/// within a cell of a cluster border, a quarter within three cells of an
/// edge, and half anywhere.
fn edgy_coordinate(rng: &mut Rng, extent: i32) -> i32 {
    match rng.below(4) {
        0 => {
            let border = rng.below(extent.div_euclid(CLUSTER) + 1) * CLUSTER;
            (border + rng.below(3) - 1).clamp(0, extent - 1)
        }
        1 => {
            if rng.below(2) == 0 {
                rng.below(3)
            } else {
                extent - 1 - rng.below(3)
            }
        }
        _ => rng.below(extent),
    }
}

/// One tick's edits: none on a third of ticks, else one to six around an
/// anchor or scattered, each a stack of stone up to six high along a row of
/// up to eight columns, an air hole three deep, a crater or a cut.
fn mixed_edits(world: &World, rng: &mut Rng) -> Vec<VoxelEdit> {
    let size = world.voxels().size();
    let [sx, sy, sz] = size.map(|extent| i32::try_from(extent).expect("a map extent fits an i32"));
    let mut edits = Vec::new();
    if rng.below(3) == 0 {
        return edits;
    }
    let anchor = (edgy_coordinate(rng, sx), edgy_coordinate(rng, sy));
    for _ in 0..=rng.below(6) {
        let (x, y) = if rng.below(2) == 0 {
            (
                (anchor.0 + rng.below(9) - 4).clamp(0, sx - 1),
                (anchor.1 + rng.below(9) - 4).clamp(0, sy - 1),
            )
        } else {
            (edgy_coordinate(rng, sx), edgy_coordinate(rng, sy))
        };
        let Some(top) = world.voxels().top_solid_z(x, y) else {
            continue;
        };
        match rng.below(5) {
            0 => {
                let height = 1 + rng.below(6);
                let along_x = rng.below(2) == 0;
                for step in 0..=rng.below(8) {
                    let (px, py) = if along_x {
                        (x + step, y)
                    } else {
                        (x, y + step)
                    };
                    if px >= sx || py >= sy {
                        break;
                    }
                    let base = world.voxels().top_solid_z(px, py).unwrap_or(top);
                    for z in (base + 1)..=(base + height).min(sz - 1) {
                        edits.push(VoxelEdit::Set {
                            at: [px, py, z],
                            material: Material::STONE,
                            by: None,
                        });
                    }
                }
            }
            1 => {
                for z in (top - 2).max(1)..=top {
                    edits.push(VoxelEdit::Set {
                        at: [x, y, z],
                        material: Material::AIR,
                        by: None,
                    });
                }
            }
            2 | 3 => edits.push(VoxelEdit::Crater {
                centre: [x, y, top],
                radius: 1 + rng.below(5),
                by: None,
            }),
            _ => edits.push(VoxelEdit::Cut {
                centre: [x, y, top],
                radius: 1 + rng.below(4),
                by: None,
            }),
        }
    }
    edits
}

/// After every fifth tick of mixed terrain edits, the repaired graph is the
/// one a fresh build over the surface makes, compared as whole structures (every
/// array, the stale tails past each live length and the scratch included),
/// and the surface is the one a fresh refresh of the store makes. Twenty
/// raiders walk, re-sent every seventh tick, so routes are served, sealed and
/// re-armed against the edits. Before lane `settle` a repaired graph left
/// stale tails behind its live lengths and the graph lagged the surface by a
/// tick.
#[test]
fn the_repaired_graph_is_a_fresh_build_under_mixed_edits() {
    let mut world = harness_world(20);
    let mut enc = pharmakos_sim::encoding::Enc::with_capacity(64 * 1024);
    let mut rng = Rng::new(0x5E77_1E00);
    let size = world.voxels().size();
    let [sx, sy, _] = size.map(|extent| i32::try_from(extent).expect("a map extent fits an i32"));
    let mut edited = 0;
    for tick in 1..=150_u32 {
        for edit in mixed_edits(&world, &mut rng) {
            // A full queue refuses an edit; the tick still runs the rest.
            let _ = world.request_voxel_edit(edit);
        }
        if tick % 7 == 0 {
            for unit in 0..world.units().len() {
                let to = world.standing_point([rng.below(sx), rng.below(sy), 0]);
                world.send_unit_directly(unit, to);
            }
        }
        let _ = world.step(&mut enc).unwrap();
        if !world.voxels().settled().is_empty() {
            edited += 1;
        }
        // A fresh build is seconds of work in a debug build, so the graph is
        // compared on every fifth tick: a stale tail a repair left behind
        // stays until a later rebuild overwrites it, so it is still there to
        // be seen.
        if tick % 5 != 0 {
            continue;
        }
        let surface = world.surface().clone();
        let mut scratch = Scratch::for_map(&surface, world.clusters().size()).expect("a scratch");
        let fresh =
            Clusters::new(&surface, world.clusters().shape(), &mut scratch).expect("a graph");
        assert!(
            *world.clusters() == fresh,
            "after tick {tick} the repaired graph is not the one a fresh build makes"
        );
        let mut refreshed = surface.clone();
        refreshed.refresh_all(world.voxels());
        assert!(
            refreshed == surface,
            "after tick {tick} the surface is not the store's"
        );
    }
    assert!(edited >= 75, "the run edited the terrain on {edited} ticks");
}
