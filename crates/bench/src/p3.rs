// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! P3, the wall-clock half of S3's depth task G2 + P3 (S3's plan, section 5.1
//! and task `bench3`; E11 and E12).
//!
//! Spec section 16's G2 + P3 row: "Repath p99 ≤5 ms; travel estimate ≤1 ms
//! and within ±15% (p90) on static terrain". The accuracy is a deterministic
//! test in the sim (`crates/sim/tests/pathing.rs`, E11); the two latencies are
//! clocks, so they are measured here, behind the wall, on the sim's own code.
//!
//! # What is timed, and how
//!
//! On each of [`P3_SEEDS`] (the golden seed first) the map is generated and
//! its graph built exactly as `World::new` builds it ([`Graph::generate`]:
//! `mapgen::generate`, the chunk store settled, `Surface::new` at the rules'
//! step costs and `pathing::graph_for` at `pathing.hpa_cluster_voxels`, 32).
//! [`PAIRS`] pairs of columns are drawn from a counter-based generator over
//! `(seed, draw)`, every column in the graph's largest component, so every
//! pair, and every run of six columns, is connected. Every query is timed
//! once, never best-of-N, and none is thrown away:
//!
//! 1. **Repath, cold.** One unit's repath is one call of the sim's router,
//!    `Router::serve` with one waiting unit and a cap of one: the abstract
//!    search, each leg's refinement, the smoothing and the route's digest,
//!    exactly the per-unit work a tick's Pathing phase does. No route or
//!    search result is reused between queries; the search scratch is, as in
//!    the tick.
//! 2. **Repath under the crater stream.** A crater a tick, G3′'s 20 a second
//!    at 20 Hz, of the mortar's radius, on a column drawn from the same
//!    generator: the store is cratered and settled (untimed: the voxel phase),
//!    then the graph is refreshed and repaired, **timed apart** (the settled
//!    chunks' columns refreshed, their clusters marked and `repair` run), then
//!    one repath between two columns the repaired graph connects is timed as
//!    in 1. Repair is outside the repath's figure and reported beside it, never
//!    inside it, since G3′ already charges repair to the tick (item 60).
//! 3. **The estimate through the gateway.** The match is hosted on the same
//!    seed in its first Lull, nothing submitted, and seat 0's token calls
//!    `estimate_route` in process (`Surface::call`, no socket): the timed region
//!    is that call, the request parsed before the clock starts; the answer is
//!    read after it stops and must be reachable. A one-leg route (one pair)
//!    is the gate's figure; a five-leg route (six consecutive columns of the
//!    draw) is reported beside it. Between calls, untimed, the host reports one
//!    tick of its clock (`Surface::set_host_clock`), so the limiter, which
//!    counts in ticks, sees a client calling once a tick.
//! 4. **The sim's estimator alone**, `pathing::estimate::estimate` over the
//!    pristine graph, unfogged, at the commander's speed, for the same pairs.
//!
//! Every figure is a nearest-rank percentile from [`crate::stats`], and every
//! notice names the seed, the committed `rules_hash` and the graph it ran on,
//! so the run reproduces from them.
//!
//! # Restated
//!
//! The marking of a settled chunk's clusters is `crates/sim/src/world.rs`'s
//! private `mark_chunk_clusters`, restated in [`mark_chunk_clusters`];
//! `tests/harness.rs` lists it with the check that holds it.

use std::path::Path;
use std::time::Instant;

use pharmakos_gateway::fog::Blind;
use pharmakos_gateway::host::Host;
use pharmakos_gateway::token::Token;
use pharmakos_proto::json::{self, Json};
use pharmakos_sim::chunks::ChunkDigests;
use pharmakos_sim::hex;
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::math::quantity::{MS_PER_TICK, Ms};
use pharmakos_sim::pathing::clusters::Clusters;
use pharmakos_sim::pathing::estimate::{Fog, Speed, estimate};
use pharmakos_sim::pathing::repair::{Repairer, repair};
use pharmakos_sim::pathing::router::{Router, WalkState};
use pharmakos_sim::pathing::search::Scratch;
use pharmakos_sim::pathing::surface::{Node, StepCosts, Surface as Ground};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::tables::SeatId;
use pharmakos_sim::voxels::{CHUNK_EDGE_I32, VoxelStore};

use crate::error::Error;
use crate::hosted::{self, Opening};
use crate::stats::{Summary, micros, millis};

/// The prefix and format version of the line `cargo xtask p3-budget` reads
/// (`xtask/src/budget.rs`, whose unit test holds the two spellings together).
pub const BUDGET_REPORT_PREFIX: &str = "p3-budget-report v1";

/// The seeds P3 is measured on, the golden seed first (S3's plan, section
/// 5.1): the seed every committed scenario plays, the determinism harness's
/// own (`pharmakos_sim::DETERMINISM_MATCH_SEED`), and Probation's map
/// (`crates/sim/tests/probation_seed.rs`' pin, 20).
pub const P3_SEEDS: [u64; 3] = [
    0x0000_0000_ca5c_aded,
    pharmakos_sim::DETERMINISM_MATCH_SEED,
    0x0000_0000_0000_0014,
];

/// The seats each map is generated for: a full v1 match.
pub const P3_SEATS: u32 = 3;

/// Pairs drawn per seed (S3's plan, section 5.1 item 1).
pub const PAIRS: usize = 2_000;

/// The waypoints of the five-leg route reported beside the one-leg figure.
pub const FIVE_LEG_WAYPOINTS: usize = 6;

/// G2's repath bar: a p99 of 5 ms.
pub const REPATH_BAR_NS: u128 = 5_000_000;

/// P3's estimate bar: a p99 of 1 ms.
pub const ESTIMATE_BAR_NS: u128 = 1_000_000;

/// How many draws one column may take before the measurement says the map
/// has no room for it: far past what a map whose largest component holds
/// most columns needs.
const REDRAWS: u64 = 10_000;

/// Salts that keep the generator's streams apart for one seed.
const PRISTINE_DRAWS: u64 = 0x5033_0001;
const STREAM_DRAWS: u64 = 0x5033_0002;
const CRATER_DRAWS: u64 = 0x5033_0003;

/// `SplitMix64`: one step of a counter-based generator, so draw `n` depends on
/// `(seed, n)` alone.
const fn mix(seed: u64, n: u64) -> u64 {
    let mut z = seed.wrapping_add(n.wrapping_mul(0x9e37_79b9_7f4a_7c15));
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// A counter-based stream of draws.
#[derive(Clone, Copy, Debug)]
struct Draws {
    seed: u64,
    next: u64,
}

impl Draws {
    const fn new(seed: u64, salt: u64) -> Draws {
        Draws {
            seed: seed ^ salt,
            next: 0,
        }
    }

    /// A value below `limit`, which is positive.
    fn below(&mut self, limit: i32) -> Result<i32, Error> {
        let bound = u64::try_from(limit).map_err(|_| Error::Overflow("a draw's bound"))?;
        let value = mix(self.seed, self.next)
            .checked_rem(bound)
            .ok_or_else(|| Error::Empty("a draw below zero".to_owned()))?;
        self.next = self
            .next
            .checked_add(1)
            .ok_or(Error::Overflow("the draws"))?;
        i32::try_from(value).map_err(|_| Error::Overflow("a draw"))
    }

    /// A column on a map of `size`.
    fn column(&mut self, size: [i32; 3]) -> Result<(i32, i32), Error> {
        Ok((self.below(size[0])?, self.below(size[1])?))
    }
}

/// The facts of a graph, as a notice prints them and a test compares them
/// with the world's own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GraphFacts {
    /// Columns.
    pub nodes: u32,
    /// Walkable columns.
    pub walkable: u32,
    /// Directed steps between columns.
    pub steps: u64,
    /// HPA* transition nodes.
    pub transitions: u32,
    /// Directed abstract edges.
    pub abstract_edges: u32,
    /// The cluster edge, in voxels.
    pub cluster_voxels: i32,
}

impl GraphFacts {
    /// The facts of `surface` decomposed as `clusters`.
    #[must_use]
    pub fn of(surface: &Ground, clusters: &Clusters) -> GraphFacts {
        GraphFacts {
            nodes: surface.node_count(),
            walkable: surface.walkable_count(),
            steps: surface.directed_step_count(),
            transitions: clusters.transition_count(),
            abstract_edges: clusters.abstract_edge_count(),
            cluster_voxels: clusters.size(),
        }
    }
}

/// The sim's own graph on a generated map, and what keeps it repaired.
#[derive(Debug)]
pub struct Graph {
    voxels: VoxelStore,
    digests: ChunkDigests,
    surface: Ground,
    clusters: Clusters,
    scratch: Scratch,
    repairer: Repairer,
}

impl Graph {
    /// The map of `seed` at `seats` seats, built as `World::new` builds it.
    ///
    /// # Errors
    ///
    /// [`Error::Sim`] when the generator refuses the seed, or the map has no
    /// graph at the rules' cluster edge.
    pub fn generate(seed: u64, seats: u32, rules: &RulesTable) -> Result<Graph, Error> {
        let generated =
            pharmakos_sim::mapgen::generate(seed, rules, seats).map_err(|error| Error::Sim {
                during: "generating P3's map",
                reason: error.to_string(),
            })?;
        let mut voxels = generated.voxels;
        let mut digests = ChunkDigests::new(voxels.chunk_count());
        voxels.settle_all(&mut digests);
        let surface =
            Ground::new(&voxels, StepCosts::from_rules(rules)).ok_or_else(|| Error::Sim {
                during: "building P3's surface",
                reason: format!("the map's extent {:?} has no surface", voxels.size()),
            })?;
        let cluster = rules.hpa_cluster_voxels();
        let (clusters, scratch) =
            pharmakos_sim::pathing::graph_for(&surface, rules.cluster_shape().with_size(cluster))
                .ok_or_else(|| Error::Sim {
                during: "building P3's graph",
                reason: format!("the map cannot be clustered at {cluster} voxels"),
            })?;
        let repairer = Repairer::new(clusters.cluster_count());
        Ok(Graph {
            voxels,
            digests,
            surface,
            clusters,
            scratch,
            repairer,
        })
    }

    /// The graph's facts.
    #[must_use]
    pub fn facts(&self) -> GraphFacts {
        GraphFacts::of(&self.surface, &self.clusters)
    }

    /// The label of the largest component, and how many walkable columns it
    /// holds.
    fn giant(&self) -> Result<(u32, u64), Error> {
        let mut labels: Vec<u32> = Vec::new();
        for node in 0..self.surface.node_count() {
            if self.surface.walkable(node) {
                labels.push(self.clusters.component_label(node));
            }
        }
        labels.sort_unstable();
        let mut best: Option<(u32, u64)> = None;
        let mut run: Option<(u32, u64)> = None;
        for label in labels {
            run = match run {
                Some((held, count)) if held == label => Some((
                    held,
                    count.checked_add(1).ok_or(Error::Overflow("a component"))?,
                )),
                _ => Some((label, 1)),
            };
            if let Some((held, count)) = run
                && best.is_none_or(|(_, most)| count > most)
            {
                best = Some((held, count));
            }
        }
        best.ok_or_else(|| Error::Empty("the map holds no walkable column".to_owned()))
    }

    /// `count` columns of the largest component, from `draws`, and the draws
    /// it took past them.
    fn points(&self, draws: &mut Draws, count: usize) -> Result<(Vec<Node>, u64), Error> {
        let (giant, _) = self.giant()?;
        let size = self.surface.size();
        let budget = u64::try_from(count)
            .map_err(|_| Error::Overflow("the columns asked for"))
            .and_then(|count| {
                REDRAWS
                    .checked_mul(count)
                    .ok_or(Error::Overflow("the redraws allowed"))
            })?;
        let mut points: Vec<Node> = Vec::with_capacity(count);
        let mut redrawn: u64 = 0;
        while points.len() < count {
            let (x, y) = draws.column(size)?;
            match self.surface.node_of(x, y) {
                Some(node)
                    if self.surface.walkable(node)
                        && self.clusters.component_label(node) == giant
                        && points.last() != Some(&node) =>
                {
                    points.push(node);
                }
                _ => {
                    redrawn = redrawn
                        .checked_add(1)
                        .ok_or(Error::Overflow("the redraws"))?;
                    if redrawn > budget {
                        return Err(Error::Layout(format!(
                            "{redrawn} draws found {} of {count} columns in the largest component",
                            points.len()
                        )));
                    }
                }
            }
        }
        Ok((points, redrawn))
    }

    /// Two columns the graph connects now, from `draws`, and the draws it
    /// took past them.
    fn connected_pair(&self, draws: &mut Draws) -> Result<((Node, Node), u64), Error> {
        let size = self.surface.size();
        let mut redrawn: u64 = 0;
        loop {
            let (ax, ay) = draws.column(size)?;
            let (bx, by) = draws.column(size)?;
            if let (Some(a), Some(b)) = (self.surface.node_of(ax, ay), self.surface.node_of(bx, by))
                && a != b
                && self.surface.walkable(a)
                && self.surface.walkable(b)
                && self.clusters.connected(&self.surface, a, b)
            {
                return Ok(((a, b), redrawn));
            }
            redrawn = redrawn
                .checked_add(1)
                .ok_or(Error::Overflow("the redraws"))?;
            if redrawn > REDRAWS {
                return Err(Error::Layout(format!(
                    "{redrawn} draws found no two connected columns on the cratered map"
                )));
            }
        }
    }

    /// Land one crater, drawn from `draws`, of `radius` voxels, and settle the
    /// store: the voxel phase's share, untimed. Answers the settled chunks.
    fn crater(&mut self, draws: &mut Draws, radius: i32) -> Result<Vec<u32>, Error> {
        let size = self.surface.size();
        for _ in 0..REDRAWS {
            let (x, y) = draws.column(size)?;
            let Some(z) = self.voxels.top_solid_z(x, y) else {
                continue;
            };
            let mut touched: Vec<u32> = Vec::new();
            self.voxels.crater([x, y, z], radius, &mut touched);
            self.voxels.settle(&mut self.digests);
            return Ok(self.voxels.settled().to_vec());
        }
        Err(Error::Layout(format!(
            "{REDRAWS} draws found no ground for a crater"
        )))
    }

    /// The graph's refresh and repair after a crater settled `chunks`: the
    /// columns refreshed, the clusters marked and repaired. The share timed
    /// beside the repath, never inside it.
    fn refresh_and_repair(&mut self, chunks: &[u32]) {
        for chunk in chunks {
            self.surface.refresh_chunk(&self.voxels, *chunk);
            mark_chunk_clusters(&self.voxels, &self.clusters, &mut self.repairer, *chunk);
        }
        repair(
            &self.surface,
            &mut self.clusters,
            &mut self.scratch,
            &mut self.repairer,
        );
    }

    /// The graph's state after the repairs so far, against one rebuilt from
    /// scratch over the same surface: what the restatement of the marking is
    /// held to (`tests/harness.rs`).
    ///
    /// # Errors
    ///
    /// [`Error::Sim`] when the surface cannot be clustered again.
    pub fn repaired_and_rebuilt(
        &self,
        rules: &RulesTable,
    ) -> Result<(GraphFacts, GraphFacts), Error> {
        let cluster = rules.hpa_cluster_voxels();
        let (rebuilt, _) = pharmakos_sim::pathing::graph_for(
            &self.surface,
            rules.cluster_shape().with_size(cluster),
        )
        .ok_or_else(|| Error::Sim {
            during: "rebuilding P3's graph",
            reason: format!("the cratered map cannot be clustered at {cluster} voxels"),
        })?;
        Ok((self.facts(), GraphFacts::of(&self.surface, &rebuilt)))
    }
}

/// Land `craters` craters of the stream on a fresh graph of `seed`, refreshing
/// and repairing after each as the measurement does, and answer the repaired
/// graph's facts beside those of a graph rebuilt from scratch over the same
/// cratered surface: equal facts are what hold [`mark_chunk_clusters`]'
/// restatement to the sim's (`tests/harness.rs`), since a cluster the marking
/// missed keeps its stale transitions and edges.
///
/// # Errors
///
/// As [`Graph::generate`], and [`Error::Layout`] for a map with no ground to
/// crater.
pub fn stream_repair_check(
    rules: &RulesTable,
    seed: u64,
    craters: usize,
) -> Result<(GraphFacts, GraphFacts), Error> {
    let mut graph = Graph::generate(seed, P3_SEATS, rules)?;
    let radius = crater_radius(rules)?;
    let mut draws = Draws::new(seed, CRATER_DRAWS);
    for _ in 0..craters {
        let settled = graph.crater(&mut draws, radius)?;
        graph.refresh_and_repair(&settled);
    }
    graph.repaired_and_rebuilt(rules)
}

/// Mark every cluster a settled chunk's footprint overlaps: `crates/sim/src/
/// world.rs`'s private `mark_chunk_clusters`, restated (at the cluster edge of
/// 32 a chunk's footprint is one cluster, item 58, and this is one mark).
fn mark_chunk_clusters(
    voxels: &VoxelStore,
    clusters: &Clusters,
    repairer: &mut Repairer,
    chunk: u32,
) {
    let Some(origin) = voxels.chunk_origin(chunk) else {
        return;
    };
    let size = clusters.size();
    if size <= 0 {
        return;
    }
    let [x0, y0, _] = origin;
    let last = |from: i32| {
        from.checked_add(CHUNK_EDGE_I32 - 1)
            .map(|end| end.div_euclid(size))
    };
    let (Some(y1), Some(x1)) = (last(y0), last(x0)) else {
        return;
    };
    for cy in y0.div_euclid(size)..=y1 {
        for cx in x0.div_euclid(size)..=x1 {
            if let Some(cluster) = clusters.cluster_index(cx, cy)
                && let Ok(tag) = u16::try_from(cluster)
            {
                repairer.mark(tag);
            }
        }
    }
}

/// A column as the router's Q16.16 point, on its standing height.
fn point_of(surface: &Ground, node: Node) -> Result<[Fx; 3], Error> {
    let (x, y) = surface.coord_of(node);
    let fx = |v: i32| {
        i16::try_from(v)
            .map(Fx::from_voxels)
            .map_err(|_| Error::Overflow("a column"))
    };
    Ok([fx(x)?, fx(y)?, fx(surface.standing_z(node))?])
}

/// One timed repath: the router serves one waiting unit from `from` to `to`.
/// Answers the time and whether the router sealed the unit (no walk found).
fn repath(
    graph: &mut Graph,
    router: &mut Router,
    from: Node,
    to: Node,
) -> Result<(u128, bool), Error> {
    let positions = [point_of(&graph.surface, from)?];
    let destinations = [point_of(&graph.surface, to)?];
    router.request(0);
    let started = Instant::now();
    let served = router.serve(
        &graph.surface,
        &graph.clusters,
        &mut graph.scratch,
        &positions,
        &destinations,
        1,
    );
    let ns = started.elapsed().as_nanos();
    if served.served != 1 {
        return Err(Error::Drift(format!(
            "the router served {} units of the one waiting",
            served.served
        )));
    }
    Ok((ns, router.state(0) == WalkState::Sealed))
}

/// What one seed measured.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SeedReport {
    /// The map seed.
    pub seed: u64,
    /// The pristine graph.
    pub graph: GraphFacts,
    /// Repaths on the pristine graph.
    pub cold: Summary,
    /// Repaths the router sealed on the pristine graph (no walk found).
    pub cold_sealed: u64,
    /// Repaths under the crater stream, each after its tick's repair.
    pub stream: Summary,
    /// Repaths the router sealed under the stream.
    pub stream_sealed: u64,
    /// The graph's refresh and repair after each crater, beside the repath.
    pub repair: Summary,
    /// The craters landed.
    pub craters: u64,
    /// The gateway's `estimate_route` for a one-leg route.
    pub one_leg: Summary,
    /// The gateway's `estimate_route` for a five-leg route.
    pub five_leg: Summary,
    /// The sim's estimator alone, for the one-leg pairs.
    pub estimator: Summary,
    /// Draws taken past the columns kept, pristine and under the stream.
    pub redrawn: u64,
}

/// What P3 measured, over every seed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct P3Report {
    /// The committed table's `rules_hash`.
    pub rules_hash: u64,
    /// Pairs drawn per seed.
    pub pairs: usize,
    /// Seeds the run was asked for.
    pub planned_seeds: usize,
    /// Each seed, in order.
    pub seeds: Vec<SeedReport>,
}

impl P3Report {
    /// The worst repath p99 over the seeds, cold or under the stream.
    #[must_use]
    pub fn worst_repath_p99(&self) -> Option<u128> {
        self.seeds
            .iter()
            .map(|seed| seed.cold.p99_ns.max(seed.stream.p99_ns))
            .max()
    }

    /// The worst one-leg estimate p99 over the seeds, through the gateway.
    #[must_use]
    pub fn worst_estimate_p99(&self) -> Option<u128> {
        self.seeds.iter().map(|seed| seed.one_leg.p99_ns).max()
    }
}

/// How P3 is run.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct P3Plan {
    /// The seeds, in order.
    pub seeds: Vec<u64>,
    /// Pairs per seed, and craters under the stream.
    pub pairs: usize,
}

impl P3Plan {
    /// The gate's plan: [`P3_SEEDS`] and [`PAIRS`].
    #[must_use]
    pub fn gate() -> P3Plan {
        P3Plan {
            seeds: P3_SEEDS.to_vec(),
            pairs: PAIRS,
        }
    }
}

/// The mortar's crater radius, the stream's.
fn crater_radius(rules: &RulesTable) -> Result<i32, Error> {
    let radius = rules
        .message()
        .structures
        .as_ref()
        .and_then(|block| block.mortar.as_ref())
        .and_then(|mortar| mortar.weapon.as_ref())
        .and_then(|weapon| weapon.projectile.as_ref())
        .map(|projectile| projectile.crater_radius_voxels)
        .ok_or_else(|| Error::Rules("no `structures.mortar.weapon.projectile`".to_owned()))?;
    i32::try_from(radius).map_err(|_| Error::Overflow("the crater radius"))
}

/// Measure P3 over `plan`, under the committed rules table at `root`.
///
/// # Errors
///
/// [`Error::Empty`] for no seed or no pair; as [`Graph::generate`]; and every
/// refusal of the hosted match or of a call, typed.
pub fn measure(root: &Path, plan: &P3Plan) -> Result<P3Report, Error> {
    if plan.seeds.is_empty() || plan.pairs == 0 {
        return Err(Error::Empty(
            "a P3 measurement over no seed or no pair measures nothing".to_owned(),
        ));
    }
    let rules = crate::quick::committed_rules(root)?;
    let mut seeds: Vec<SeedReport> = Vec::with_capacity(plan.seeds.len());
    for seed in &plan.seeds {
        seeds.push(measure_seed(&rules, *seed, plan.pairs)?);
    }
    Ok(P3Report {
        rules_hash: rules.rules_hash(),
        pairs: plan.pairs,
        planned_seeds: plan.seeds.len(),
        seeds,
    })
}

fn count(sealed: bool, held: u64) -> Result<u64, Error> {
    held.checked_add(u64::from(sealed))
        .ok_or(Error::Overflow("the sealed repaths"))
}

/// One seed, every figure.
fn measure_seed(rules: &RulesTable, seed: u64, pairs: usize) -> Result<SeedReport, Error> {
    let mut graph = Graph::generate(seed, P3_SEATS, rules)?;
    let pristine = graph.facts();
    let wanted = pairs
        .checked_mul(2)
        .ok_or(Error::Overflow("the columns drawn"))?;
    let (points, mut redrawn) = graph.points(&mut Draws::new(seed, PRISTINE_DRAWS), wanted)?;
    let pair_list: Vec<(Node, Node)> = points
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[a, b]| (*a, *b))
        .collect();
    if pair_list.len() != pairs {
        return Err(Error::Drift(format!(
            "{} pairs from {} columns, not {pairs}",
            pair_list.len(),
            points.len()
        )));
    }

    // 4. The sim's estimator alone, on the pristine graph.
    let walker = Speed::commander(rules);
    let mut estimator: Vec<u128> = Vec::with_capacity(pairs);
    for (a, b) in &pair_list {
        let started = Instant::now();
        let answer = estimate(
            &graph.surface,
            &graph.clusters,
            &mut graph.scratch,
            *a,
            *b,
            Fog::Clear,
            walker,
        );
        let ns = started.elapsed().as_nanos();
        if answer.is_none() {
            return Err(Error::Drift(format!(
                "the estimator found no route between two columns of one component on seed \
                 0x{seed:016x}"
            )));
        }
        estimator.push(ns);
    }

    // 1. Cold repaths on the pristine graph.
    let mut router = Router::new(1, rules);
    let mut cold: Vec<u128> = Vec::with_capacity(pairs);
    let mut cold_sealed: u64 = 0;
    for (a, b) in &pair_list {
        let (ns, sealed) = repath(&mut graph, &mut router, *a, *b)?;
        cold.push(ns);
        cold_sealed = count(sealed, cold_sealed)?;
    }

    // 3. The gateway's estimate, in a hosted Lull on the same map.
    let (one_leg, five_leg) = gateway_estimates(rules, seed, &graph, &points, &pair_list)?;

    // 2. Repaths under the crater stream, a crater a tick.
    let radius = crater_radius(rules)?;
    let mut craters = Draws::new(seed, CRATER_DRAWS);
    let mut stream_draws = Draws::new(seed, STREAM_DRAWS);
    let mut stream: Vec<u128> = Vec::with_capacity(pairs);
    let mut repairs: Vec<u128> = Vec::with_capacity(pairs);
    let mut stream_sealed: u64 = 0;
    let mut landed: u64 = 0;
    for _ in 0..pairs {
        let settled = graph.crater(&mut craters, radius)?;
        landed = landed
            .checked_add(1)
            .ok_or(Error::Overflow("the craters"))?;
        let started = Instant::now();
        graph.refresh_and_repair(&settled);
        repairs.push(started.elapsed().as_nanos());
        let ((a, b), more) = graph.connected_pair(&mut stream_draws)?;
        redrawn = redrawn
            .checked_add(more)
            .ok_or(Error::Overflow("the redraws"))?;
        let (ns, sealed) = repath(&mut graph, &mut router, a, b)?;
        stream.push(ns);
        stream_sealed = count(sealed, stream_sealed)?;
    }

    Ok(SeedReport {
        seed,
        graph: pristine,
        cold: Summary::of(cold)?,
        cold_sealed,
        stream: Summary::of(stream)?,
        stream_sealed,
        repair: Summary::of(repairs)?,
        craters: landed,
        one_leg,
        five_leg,
        estimator: Summary::of(estimator)?,
        redrawn,
    })
}

/// A `gp.v1.Location` voxel on a column's standing height, as the wire spells
/// it.
fn voxel(surface: &Ground, node: Node) -> Json {
    let (x, y) = surface.coord_of(node);
    let number = |v: i32| Json::Number(v.to_string());
    Json::Object(vec![(
        "voxel".to_owned(),
        Json::Object(vec![
            ("x".to_owned(), number(x)),
            ("y".to_owned(), number(y)),
            ("z".to_owned(), number(surface.standing_z(node))),
        ]),
    )])
}

/// The hosted Lull P3's estimate is timed in, its seat 0's token, and the
/// host clock the bench reports between calls.
struct Lull {
    surface: pharmakos_gateway::surface::Surface,
    token: Token,
    elapsed_ms: i32,
    /// Round 1's Lull, `rules.match.first_lull_ms`: the countdown's whole.
    length_ms: i32,
}

impl Lull {
    fn open(rules: &RulesTable, seed: u64) -> Result<Lull, Error> {
        let opening = Opening {
            seed,
            seats: P3_SEATS,
            // The Lull is all the measurement plays; the segment never opens.
            segment_ms: MS_PER_TICK,
            rules,
            playbook: "",
        };
        let host: Host = hosted::open_host(&opening)?;
        // Untimed until the first report, which then carries the countdown
        // of round 1's Lull from its whole length.
        let mut surface = hosted::open_lull(host, &opening, None)?;
        let token = hosted::mint(&mut surface, SeatId::new(0))?;
        let lull_ms = rules
            .message()
            .r#match
            .as_ref()
            .map(|block| block.first_lull_ms)
            .ok_or_else(|| Error::Rules("no `match` block, so no first Lull".to_owned()))?;
        Ok(Lull {
            surface,
            token,
            elapsed_ms: 0,
            length_ms: lull_ms,
        })
    }

    /// Report one more tick of host time in the Lull, untimed, so the
    /// limiter sees one call a tick.
    fn tick(&mut self) -> Result<(), Error> {
        self.elapsed_ms = self
            .elapsed_ms
            .checked_add(MS_PER_TICK)
            .ok_or(Error::Overflow("the Lull's clock"))?;
        let remaining = self
            .length_ms
            .checked_sub(self.elapsed_ms)
            .filter(|remaining| *remaining > 0)
            .ok_or_else(|| {
                Error::Empty(format!(
                    "the calls would outlast the {} ms Lull they are timed in",
                    self.length_ms
                ))
            })?;
        self.surface
            .set_host_clock(Ms::new(self.elapsed_ms), Ms::new(remaining))
            .map_err(|error| Error::gateway("reporting the host clock", &error))
    }

    /// One timed `estimate_route` over `waypoints`.
    fn time(&mut self, waypoints: Vec<Json>) -> Result<u128, Error> {
        self.tick()?;
        let params = json::write(&Json::Object(vec![(
            "waypoints".to_owned(),
            Json::Array(waypoints),
        )]));
        let request = pharmakos_gateway::rpc::parse(&format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"estimate_route","params":{params}}}"#
        ))
        .map_err(|error| Error::Codec {
            what: "the bench's estimate_route request".to_owned(),
            reason: error.title().to_string(),
        })?;
        let started = Instant::now();
        let answer = self.surface.call(Some(&self.token), &request, &Blind);
        let ns = started.elapsed().as_nanos();
        let reachable = answer
            .get("result")
            .and_then(|result| result.get("reachable"));
        if reachable != Some(&Json::Bool(true)) {
            return Err(Error::Gateway {
                during: "estimating a route in the Lull",
                message: format!(
                    "at {} ms of host time in the Lull: {}",
                    self.elapsed_ms,
                    json::write(&answer)
                ),
            });
        }
        Ok(ns)
    }
}

/// The gateway's one-leg and five-leg figures.
fn gateway_estimates(
    rules: &RulesTable,
    seed: u64,
    graph: &Graph,
    points: &[Node],
    pairs: &[(Node, Node)],
) -> Result<(Summary, Summary), Error> {
    let mut lull = Lull::open(rules, seed)?;
    let hosted = GraphFacts::of(
        hosted::world(&lull.surface)?.surface(),
        hosted::world(&lull.surface)?.clusters(),
    );
    if hosted != graph.facts() {
        return Err(Error::Drift(format!(
            "the hosted world's graph {hosted:?} is not the bench's {:?} on seed 0x{seed:016x}",
            graph.facts()
        )));
    }
    let mut one_leg: Vec<u128> = Vec::with_capacity(pairs.len());
    for (a, b) in pairs {
        one_leg.push(lull.time(vec![voxel(&graph.surface, *a), voxel(&graph.surface, *b)])?);
    }
    let mut five_leg: Vec<u128> = Vec::with_capacity(pairs.len());
    for window in points.windows(FIVE_LEG_WAYPOINTS).take(pairs.len()) {
        five_leg.push(
            lull.time(
                window
                    .iter()
                    .map(|node| voxel(&graph.surface, *node))
                    .collect(),
            )?,
        );
    }
    Ok((Summary::of(one_leg)?, Summary::of(five_leg)?))
}

/// One seed's notice, word for word. Pure.
#[must_use]
pub fn seed_notice(report: &P3Report, seed: &SeedReport, os: &str) -> String {
    let g = &seed.graph;
    format!(
        "::notice title=P3 seed 0x{:016x}::on {os} (rules_hash {}; {} columns, {} walkable, {} \
         steps, {} transitions, {} abstract edges at cluster {}; {} pairs in the largest \
         component): repath cold p50 {} p99 {} max {} ({} sealed); under a crater a tick ({} \
         craters) p50 {} p99 {} max {} ({} sealed), the refresh and repair beside it p50 {} p99 \
         {}; estimate_route through the gateway one leg p50 {} p99 {} max {}, five legs p50 {} \
         p99 {}; the sim's estimator alone p50 {} p99 {}; {} draws redrawn",
        seed.seed,
        hex(report.rules_hash),
        g.nodes,
        g.walkable,
        g.steps,
        g.transitions,
        g.abstract_edges,
        g.cluster_voxels,
        report.pairs,
        micros(seed.cold.p50_ns),
        micros(seed.cold.p99_ns),
        micros(seed.cold.max_ns),
        seed.cold_sealed,
        seed.craters,
        micros(seed.stream.p50_ns),
        micros(seed.stream.p99_ns),
        micros(seed.stream.max_ns),
        seed.stream_sealed,
        micros(seed.repair.p50_ns),
        micros(seed.repair.p99_ns),
        micros(seed.one_leg.p50_ns),
        micros(seed.one_leg.p99_ns),
        micros(seed.one_leg.max_ns),
        micros(seed.five_leg.p50_ns),
        micros(seed.five_leg.p99_ns),
        micros(seed.estimator.p50_ns),
        micros(seed.estimator.p99_ns),
        seed.redrawn,
    )
}

/// The summary notice: the worst figures over the seeds against the bars.
///
/// # Errors
///
/// [`Error::Empty`] for a report of no seed.
pub fn summary_notice(report: &P3Report, os: &str) -> Result<String, Error> {
    let empty = || Error::Empty("a P3 report of no seed".to_owned());
    Ok(format!(
        "::notice title=P3::on {os}, over {} seeds x {} pairs: the worst repath p99 is {} \
         (cold or under the crater stream, repair outside it) against G2's {}, and the worst \
         one-leg estimate_route p99 {} against P3's {}; published here and never gated, \
         certified on the owner's machine",
        report.seeds.len(),
        report.pairs,
        millis(report.worst_repath_p99().ok_or_else(empty)?),
        millis(REPATH_BAR_NS),
        millis(report.worst_estimate_p99().ok_or_else(empty)?),
        millis(ESTIMATE_BAR_NS),
    ))
}

/// The line `cargo xtask p3-budget` reads, after [`BUDGET_REPORT_PREFIX`]: the
/// seeds measured and planned and the pairs (so a short run is refused), and
/// the worst repath and one-leg estimate p99 in whole nanoseconds, or `none`
/// for a report of no seed, which the reader refuses as no verdict.
#[must_use]
pub fn budget_report_line(report: &P3Report) -> String {
    let figure = |value: Option<u128>| match value {
        Some(ns) => ns.to_string(),
        None => "none".to_owned(),
    };
    format!(
        "{BUDGET_REPORT_PREFIX} seeds={} planned_seeds={} pairs={} planned_pairs={PAIRS} \
         repath_p99_ns={} estimate_p99_ns={}",
        report.seeds.len(),
        report.planned_seeds,
        report.pairs,
        figure(report.worst_repath_p99()),
        figure(report.worst_estimate_p99()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_draws_are_a_pure_function_of_seed_and_count() {
        let mut first = Draws::new(7, PRISTINE_DRAWS);
        let mut again = Draws::new(7, PRISTINE_DRAWS);
        let mut other = Draws::new(7, STREAM_DRAWS);
        let a: Vec<i32> = (0..8).map(|_| first.below(384).expect("a draw")).collect();
        let b: Vec<i32> = (0..8).map(|_| again.below(384).expect("a draw")).collect();
        let c: Vec<i32> = (0..8).map(|_| other.below(384).expect("a draw")).collect();
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.iter().all(|value| (0..384).contains(value)));
        assert!(first.below(0).is_err(), "nothing is below zero");
    }

    #[test]
    fn the_budget_line_is_word_for_word() {
        let summary = |p99_ns: u128| Summary {
            p50_ns: 1,
            p99_ns,
            mean_ns: 1,
            max_ns: p99_ns,
            samples: 2,
        };
        let facts = GraphFacts {
            nodes: 1,
            walkable: 1,
            steps: 1,
            transitions: 1,
            abstract_edges: 1,
            cluster_voxels: 32,
        };
        let seed = SeedReport {
            seed: 1,
            graph: facts,
            cold: summary(2_000_000),
            cold_sealed: 0,
            stream: summary(3_000_000),
            stream_sealed: 0,
            repair: summary(9_000_000),
            craters: 2,
            one_leg: summary(400_000),
            five_leg: summary(900_000),
            estimator: summary(300_000),
            redrawn: 0,
        };
        let report = P3Report {
            rules_hash: 0,
            pairs: 2,
            planned_seeds: 2,
            seeds: vec![
                seed.clone(),
                SeedReport {
                    cold: summary(4_000_000),
                    one_leg: summary(600_000),
                    ..seed
                },
            ],
        };
        assert_eq!(
            budget_report_line(&report),
            "p3-budget-report v1 seeds=2 planned_seeds=2 pairs=2 planned_pairs=2000 \
             repath_p99_ns=4000000 estimate_p99_ns=600000"
        );
        let none = P3Report {
            seeds: Vec::new(),
            ..report
        };
        assert!(budget_report_line(&none).ends_with("repath_p99_ns=none estimate_p99_ns=none"));
        assert!(summary_notice(&none, "linux").is_err());
    }
}
