// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! "Nearest" is the walk the walker will be served (lane `p3fix`), held to an
//! oracle on generated, walled and cratered terrain.
//!
//! The oracle is the router itself, asked as a walker asks: [`route_with`]
//! from the start, and from each partial route's end again until a route
//! reaches the goal. It is not `walk_cost`'s code: a candidate it cannot price
//! fails the test rather than leaving the truth. Beside it, the walker: a
//! commander sent along a far pair and stepped until it arrives, its ticks set
//! against the priced walk's, with and without the threats its route avoids.
//!
//! `toward`'s stone is the other measure (`targeting`'s "Two measures"): the
//! legal site closest to the feature over the terrain. It is held here to its
//! definition by brute force, with a Dijkstra of the test's own, and the
//! counted work of one resolve is pinned, behind rings of wall included.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report."
)]

use pharmakos_proto::gp;
use pharmakos_proto::json;
use pharmakos_sim::encoding::Enc;
use pharmakos_sim::features::FeatureKind;
use pharmakos_sim::interpreter::Plan;
use pharmakos_sim::interpreter::state::{StepFailed, StepFailure};
use pharmakos_sim::math::fixed::{Fx, Sq};
use pharmakos_sim::pathing::estimate::{Speed, max_partials, walk_cost};
use pharmakos_sim::pathing::reach::Reach;
use pharmakos_sim::pathing::route::{Surcharge, route_with};
use pharmakos_sim::pathing::search::{Scratch, Tally, Unanswered};
use pharmakos_sim::pathing::surface::Node;
use pharmakos_sim::runner::{MatchSettings, Runner};
use pharmakos_sim::tables::{BeaconId, SeatId};
use pharmakos_sim::targeting::{FeatureSpec, Ranker, StoneCandidate, column_of, toward_counted};
use pharmakos_sim::voxels::{Material, VoxelEdit};
use pharmakos_sim::{RulesTable, World, WorldConfig};

/// The golden seed.
const SEED: u64 = 0x0000_0000_ca5c_aded;
/// Probation's seed and its pinned second vent (`tests/probation_seed.rs`).
const PROBATION: u64 = 20;
const PROBATION_VENT: [i32; 2] = [232, 96];

fn rules() -> RulesTable {
    RulesTable::load(
        &std::path::Path::new("..")
            .join("..")
            .join("rules")
            .join("rules.v1.json"),
    )
    .expect("the committed rules table loads")
}

fn world_of(seed: u64, seats: u32) -> World {
    let rules = rules();
    World::new(&WorldConfig {
        match_seed: seed,
        seats,
        rules: rules.clone(),
        match_settings: MatchSettings {
            segment_lengths_ms: rules.segment_lengths_ms().to_vec(),
            round_limit: 4,
        },
    })
    .expect("the committed rules table describes a world")
}

/// A seeded xorshift, for draws the test repeats exactly.
struct Rng(u64);

impl Rng {
    fn below(&mut self, limit: i32) -> i32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        i32::try_from(self.0 % u64::try_from(limit).unwrap()).unwrap()
    }
}

/// File `edit`, stepping the world to drain the queue when it is full.
fn file(world: &mut World, enc: &mut Enc, edit: VoxelEdit) {
    if world.request_voxel_edit(edit).is_err() {
        world.step(enc).expect("a tick");
        world.request_voxel_edit(edit).expect("room after a tick");
    }
}

/// Stone `high` voxels above the column `(x, y)`: three is a wall no walker
/// climbs.
fn wall_column(world: &mut World, enc: &mut Enc, x: i32, y: i32, high: i32) {
    let Some(top) = world.voxels().top_solid_z(x, y) else {
        return;
    };
    for z in top + 1..=top + high {
        file(
            world,
            enc,
            VoxelEdit::Set {
                at: [x, y, z],
                material: Material::STONE,
                by: None,
            },
        );
    }
}

/// Step until the graph has settled every edit filed.
fn settle(world: &mut World, enc: &mut Enc) {
    for _ in 0..3 {
        world.step(enc).expect("a tick");
    }
}

/// Seventy stone walls three high in random runs and forty craters: terrain
/// the octile bound knows nothing of.
fn scar(world: &mut World, seed: u64) {
    let mut enc = Enc::with_capacity(64 * 1024);
    let [sx, sy, _] = world.surface().size();
    let mut rng = Rng(seed ^ 0xA5A5_5A5A_1234_5678);
    for _ in 0..70 {
        let (x0, y0) = (rng.below(sx), rng.below(sy));
        let (dx, dy) = [(1, 0), (0, 1), (1, 1), (1, -1)][usize::try_from(rng.below(4)).unwrap()];
        let length = 8 + rng.below(40);
        for i in 0..length {
            let (x, y) = (x0 + dx * i, y0 + dy * i);
            if x < 1 || y < 1 || x >= sx - 1 || y >= sy - 1 {
                break;
            }
            wall_column(world, &mut enc, x, y, 3);
        }
    }
    for _ in 0..40 {
        let (x, y) = (rng.below(sx), rng.below(sy));
        let Some(top) = world.voxels().top_solid_z(x, y) else {
            continue;
        };
        let radius = 3 + rng.below(4);
        file(
            world,
            &mut enc,
            VoxelEdit::Crater {
                centre: [x, y, top],
                radius,
                by: None,
            },
        );
    }
    settle(world, &mut enc);
}

/// A ring of stone three high, of `radius` round `centre`, with its gap on
/// the side `away` points to: the cells whose direction from the centre lies
/// within about 5.7 degrees of it (cosine above 0.995, compared squared in
/// integers).
fn ring(world: &mut World, centre: [i32; 2], radius: i32, away: [i32; 2]) {
    let mut enc = Enc::with_capacity(64 * 1024);
    let norm = i128::from(away[0]).pow(2) + i128::from(away[1]).pow(2);
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let d2 = dx * dx + dy * dy;
            if d2 < (radius - 1) * (radius - 1) || d2 > radius * radius {
                continue;
            }
            let dot = i128::from(dx) * i128::from(away[0]) + i128::from(dy) * i128::from(away[1]);
            if dot > 0 && dot * dot * 1_000_000 > 990_025 * i128::from(d2) * norm {
                continue;
            }
            wall_column(world, &mut enc, centre[0] + dx, centre[1] + dy, 3);
        }
    }
    settle(world, &mut enc);
}

/// **The oracle**: the cost of the walk the router serves from `start` to
/// `goal` under `surcharge`, asked as a walker asks it, or `None` when the
/// router serves no route. A chain that does not end within the map's
/// `max_partials` asks fails the test.
fn walked(world: &World, scratch: &mut Scratch, (start, goal): (Node, Node)) -> Option<i64> {
    let mut buffer = Vec::with_capacity(4096);
    let mut from = start;
    let mut total = 0_i64;
    for _ in 0..=max_partials(world.surface()) {
        let served = route_with(
            world.surface(),
            world.clusters(),
            scratch,
            (from, goal),
            Surcharge::NONE,
            &mut buffer,
        )
        .ok()?;
        total += i64::from(served.cost);
        if !served.partial() {
            return Some(total);
        }
        from = *buffer.last().expect("a partial route ends on a column");
    }
    panic!("the router's chain from {start} to {goal} never ended")
}

/// The column a "nearest" measures feature `index` to from `origin`: its
/// anchor column when a walker from the origin stands on it, else the first
/// such footprint column.
fn measured_to(world: &World, index: usize, origin: Node) -> Option<Node> {
    let feature = world.features().features().get(index)?;
    let surface = world.surface();
    let reachable = |x: i32, y: i32| {
        let node = surface.node_of(x, y)?;
        (surface.walkable(node) && world.clusters().connected(surface, origin, node))
            .then_some(node)
    };
    if let Some(column) = feature.anchor_column()
        && let Some(node) = reachable(column.x, column.y)
    {
        return Some(node);
    }
    feature
        .footprint
        .iter()
        .find_map(|column| reachable(column.x, column.y))
}

/// What the searches counted since the counters were last reset: the nodes
/// they popped, and every visit of their loops that pops nothing
/// (`Scratch::visits`; S3's `walk3`), whose honesty `tests/pathing.rs` holds
/// to the pops.
fn popped(scratch: &Scratch) -> u64 {
    scratch.expansions().expect("the count fits a u64")
        + scratch.abstract_expansions().expect("the count fits a u64")
        + scratch.visits().expect("the count fits a u64")
}

/// A walkable origin drawn from `rng`.
fn origin(world: &World, rng: &mut Rng) -> ([i32; 2], Node) {
    let [sx, sy, _] = world.surface().size();
    loop {
        let (x, y) = (rng.below(sx), rng.below(sy));
        if let Some(node) = world.surface().node_of(x, y)
            && world.surface().walkable(node)
        {
            return ([x, y], node);
        }
    }
}

/// Every feature, vents and seams, every rank: what "nearest" emits from an
/// origin is the oracle's order of walks (cost, then anchor `(y, x)`), each
/// with its walk's own cost, and nothing after the last. On the golden seed
/// and two others, as generated and scarred by walls and craters. Every
/// mismatch is counted, not only the first; and the octile bound the ranker
/// prunes by is never above a walk.
#[test]
fn nearest_is_the_least_served_walk() {
    let (mut ranks, mut wrong, mut over) = (0_u32, Vec::new(), Vec::new());
    let mut stalled = 0_u64;
    for (seed, scarred) in [(SEED, false), (7, false), (SEED, true), (0x5eed_0003, true)] {
        let mut world = world_of(seed, 2);
        if scarred {
            scar(&mut world, seed);
        }
        let mut scratch = Scratch::for_map(world.surface(), world.clusters().size()).unwrap();
        let mut truth_scratch = scratch.clone();
        let mut rng = Rng(seed ^ 0x9E37_79B9_7F4A_7C15);
        for _ in 0..60 {
            let ([ox, oy], from) = origin(&world, &mut rng);
            let mut truth: Vec<(i64, i32, i32, usize)> = Vec::new();
            for (index, feature) in world.features().features().iter().enumerate() {
                let Some(goal) = measured_to(&world, index, from) else {
                    continue;
                };
                let cost = walked(&world, &mut truth_scratch, (from, goal)).unwrap_or_else(|| {
                    panic!("seed {seed:#x}: the oracle cannot price feature {index} from {ox},{oy}")
                });
                let bound = i64::from(world.surface().heuristic(from, goal));
                if bound > cost {
                    over.push((seed, [ox, oy], index, bound, cost));
                }
                truth.push((cost, feature.anchor[1], feature.anchor[0], index));
            }
            truth.sort_unstable();
            let ground = world.ground();
            let mut ranker = Ranker::new(&ground, [ox, oy], |_| true);
            for expected in &truth {
                ranks += 1;
                let got = ranker
                    .next(&ground, &mut scratch)
                    .expect("the scratch serves")
                    .map(|(index, walk)| (index, walk.cost));
                if got != Some((expected.3, expected.0)) {
                    wrong.push((seed, scarred, [ox, oy], got, (expected.3, expected.0)));
                }
            }
            stalled += scratch.stalled().unwrap();
            scratch.reset_counters();
            let extra = ranker
                .next(&ground, &mut scratch)
                .expect("the scratch serves")
                .map(|(index, walk)| (index, walk.cost));
            if extra.is_some() {
                wrong.push((seed, scarred, [ox, oy], extra, (usize::MAX, 0)));
            }
        }
    }
    assert!(ranks >= 2_000, "the test ranks enough: {ranks}");
    assert_eq!(stalled, 0, "no candidate's chain of partial routes stalled");
    assert!(
        over.is_empty(),
        "{} octile bounds above a walk; first {:?}",
        over.len(),
        over.first()
    );
    assert!(
        wrong.is_empty(),
        "{} of {ranks} ranks were not the oracle's; first {:?}",
        wrong.len(),
        wrong.first()
    );
}

/// The step rule is symmetric, in legality and in cost, on generated and
/// scarred terrain: the grid optimum's search runs outward from the goal over
/// the forward steps (`pathing::reach`, "Direction"), which is the reverse
/// graph only because of this.
#[test]
fn the_step_rule_is_symmetric() {
    for (seed, scarred) in [(SEED, false), (SEED, true)] {
        let mut world = world_of(seed, 2);
        if scarred {
            scar(&mut world, seed);
        }
        let surface = world.surface();
        let mut steps = 0_u64;
        for node in 0..surface.node_count() {
            for k in 0..pharmakos_sim::pathing::search::neighbour_count() {
                if let Some((other, cost)) = surface.neighbour(node, k) {
                    steps += 1;
                    assert_eq!(
                        surface.step_cost_between(other, node),
                        Some(cost),
                        "seed {seed:#x}: the step {node} to {other} has no equal step back"
                    );
                }
            }
        }
        assert!(steps > 100_000, "the map has steps: {steps}");
    }
}

/// **The definition's oracle**: the least cost of any walk from every column
/// to `goal` over the walkers' step rule, by a plain Dijkstra on the
/// standard library's heap, reading each step backwards through
/// `step_cost_between` (so it relies on no symmetry). Not `pathing::reach`'s
/// code.
fn least_costs(world: &World, goal: Node) -> Vec<Option<i64>> {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let surface = world.surface();
    let n = usize::try_from(surface.node_count()).unwrap();
    let mut cost: Vec<Option<i64>> = vec![None; n];
    let mut heap = BinaryHeap::new();
    cost[usize::try_from(goal).unwrap()] = Some(0);
    heap.push(Reverse((0_i64, goal)));
    while let Some(Reverse((here, node))) = heap.pop() {
        if cost[usize::try_from(node).unwrap()] != Some(here) {
            continue;
        }
        let (x, y) = surface.coord_of(node);
        for dy in -1..=1 {
            for dx in -1..=1 {
                let Some(other) = surface.node_of(x + dx, y + dy) else {
                    continue;
                };
                if other == node {
                    continue;
                }
                // A walk from `other` steps onto `node`.
                let Some(step) = surface.step_cost_between(other, node) else {
                    continue;
                };
                let next = here + i64::from(step);
                let slot = &mut cost[usize::try_from(other).unwrap()];
                if slot.is_none_or(|held| next < held) {
                    *slot = Some(next);
                    heap.push(Reverse((next, other)));
                }
            }
        }
    }
    cost
}

/// The grid optimum's bucket queue, undirected, settles every column it
/// reaches exactly once, at the least cost the test's own Dijkstra finds, in
/// strictly ascending `(cost, column)`: the total order a binary heap over
/// that key gives, the undirected API's promise (`pathing::reach`, "The
/// queue, and its order"). Full floods from seeded columns of generated and
/// scarred maps.
///
/// A ring of `2 · max step` buckets instead of `2 · max step + 1` (re-review
/// 4's mutant H8) passes this and every test: it is equivalent whenever the
/// climb surcharge is above zero, as on the committed table, since a key
/// then rises across one step by its cost plus its octile length, below
/// twice the dearest step. The `+ 1` keeps tables with no climb correct.
#[test]
fn the_grid_optimum_settles_in_cost_then_column_order() {
    let mut floods = 0;
    for (seed, scarred) in [(SEED, false), (SEED, true), (PROBATION, true)] {
        let mut world = world_of(seed, 2);
        if scarred {
            scar(&mut world, seed);
        }
        let surface = world.surface();
        let mut reach =
            Reach::new(surface.node_count(), surface.costs().max_step()).expect("a ring");
        let mut rng = Rng(seed ^ 0x0077);
        for _ in 0..3 {
            let (_, from) = origin(&world, &mut rng);
            let least = least_costs(&world, from);
            let reachable = least.iter().filter(|cost| cost.is_some()).count();
            reach.begin(from).expect("the start is on the map");
            let mut tally = Tally::ZERO;
            let mut previous: Option<(i64, Node)> = None;
            let mut settled = 0_usize;
            while let Some((node, cost)) = reach.next(surface, &mut tally).expect("on the map") {
                assert_eq!(
                    Some(cost),
                    least[usize::try_from(node).unwrap()],
                    "seed {seed:#x}: column {node} settled at a cost not its least"
                );
                assert!(
                    previous.is_none_or(|held| held < (cost, node)),
                    "seed {seed:#x}: {previous:?} then {:?}",
                    (cost, node)
                );
                previous = Some((cost, node));
                settled += 1;
            }
            assert_eq!(
                settled, reachable,
                "seed {seed:#x}: every reachable column, once"
            );
            floods += 1;
        }
    }
    assert_eq!(floods, 9);
}

/// `toward` against its definition by brute force, on generated, walled and
/// cratered terrain: for every live vent of each seed, named from seat 0's
/// commander, the stone is the legal site (every one the search listed) with
/// the least walking cost to the vent over the terrain, ties to the lowest
/// `(y, x)`, by the test's own Dijkstra; and no walk is priced. Cases whose
/// least cost several sites share are counted, and some must occur, so the
/// tie-break is held to the definition and not only the cost.
#[test]
fn a_toward_is_the_closest_site_over_the_terrain() {
    let (mut cases, mut sites_seen, mut ties) = (0_u32, 0_u64, 0_u32);
    for (seed, scarred) in [
        (PROBATION, false),
        (PROBATION, true),
        (SEED, false),
        (SEED, true),
        (7, true),
        (13, false),
    ] {
        let mut world = world_of(seed, 2);
        if scarred {
            scar(&mut world, seed);
        }
        let seat = SeatId::new(0);
        let commander = world.commander_of(seat).expect("a commander");
        let row = world.units().row_of(commander).expect("a live commander");
        let start = world.units().positions()[row.index()];
        let [cx, cy] = pharmakos_sim::targeting::column_of(start);
        let from = world
            .surface()
            .node_of(cx, cy)
            .expect("the commander's column");
        let vents: Vec<(usize, [i32; 2])> = world
            .features()
            .features()
            .iter()
            .enumerate()
            .filter(|(index, feature)| {
                feature.kind == FeatureKind::Vent && world.feature_is_live(*index)
            })
            .map(|(index, feature)| (index, feature.anchor))
            .collect();
        let mut scratch = Scratch::for_map(world.surface(), world.clusters().size()).unwrap();
        for (index, anchor) in vents {
            let mut stones: Vec<StoneCandidate> = Vec::new();
            let found = toward_counted(
                &world.ground(),
                &mut scratch,
                &mut stones,
                seat,
                start,
                FeatureSpec::Name {
                    kind: FeatureKind::Vent,
                    anchor,
                },
            );
            assert_eq!(scratch.walks().unwrap(), 0, "a stone prices no walk");
            assert_eq!(
                scratch.reach_faults().unwrap(),
                0,
                "no search met a column it has no room for"
            );
            let Some(goal) = measured_to(&world, index, from) else {
                assert!(
                    found.is_err(),
                    "seed {seed:#x}: a stone toward an unreachable vent"
                );
                continue;
            };
            cases += 1;
            let least = least_costs(&world, goal);
            let mut best: Option<(i64, i32, i32)> = None;
            let mut at_best = 0_u32;
            for stone in &stones {
                sites_seen += 1;
                let [x, y] = stone.column();
                let cost = least[usize::try_from(stone.node()).unwrap()]
                    .expect("every legal site reaches the vent");
                match best {
                    Some((held, _, _)) if cost == held => at_best += 1,
                    Some((held, _, _)) if cost > held => {}
                    _ => at_best = 1,
                }
                if best.is_none_or(|held| (cost, y, x) < held) {
                    best = Some((cost, y, x));
                }
            }
            if at_best > 1 {
                ties += 1;
            }
            assert_eq!(
                found.ok().map(|stone| stone.site),
                best.map(|(_, y, x)| [x, y]),
                "seed {seed:#x}, vent {anchor:?}: toward is not the closest site over the terrain"
            );
        }
    }
    assert!(cases >= 30, "the test weighs enough vents: {cases}");
    assert!(sites_seen > 50_000, "and enough sites: {sites_seen}");
    println!("{cases} vents, {sites_seen} sites, {ties} with a tie at the least cost");
    assert!(ties >= 3, "the cases hold ties at the least cost: {ties}");
}

/// One `toward` resolve's counted work on the probation seed: toward its
/// pinned vent on open ground and behind a ring of stone three high at
/// radius 10, 18 and 26 with the ring's gap facing away from the core (review
/// probe B); and item 161's three cases, the pinned vent, the live vent
/// farthest from the core and the pinned vent with a Resonance Spire grown
/// at the core. Pinned: the stone, and the nodes the search popped, a count
/// that is the same on every platform; no walk is priced. A change that
/// moves one says why.
#[test]
fn a_toward_resolve_pops_only_what_is_closer_than_the_stone() {
    let mut measured = Vec::new();
    for (label, spire, far, terrain) in [
        ("open", false, false, Terrain::Open),
        ("ring 10", false, false, Terrain::Ring(10)),
        ("ring 18", false, false, Terrain::Ring(18)),
        ("ring 26", false, false, Terrain::Ring(26)),
        ("farthest", false, true, Terrain::Open),
        ("pinned, Spire", true, false, Terrain::Open),
        ("farthest, Spire, ring 26", true, true, Terrain::Ring(26)),
        (
            "farthest, 18 decoy spheres",
            false,
            true,
            Terrain::Decoys(18),
        ),
        ("farthest, long wall", false, true, Terrain::LongWall),
        ("farthest, cup", false, true, Terrain::Cup(60)),
    ] {
        let mut world = world_of(PROBATION, 2);
        let seat = SeatId::new(0);
        let core_row = world
            .beacons()
            .seats()
            .iter()
            .position(|owner| *owner == seat.raw())
            .expect("a core");
        let core = column_of(world.beacons().positions()[core_row]);
        let vent = if far {
            farthest_vent(&world, core)
        } else {
            PROBATION_VENT
        };
        if spire {
            let id = BeaconId::new(world.beacons().ids()[core_row]);
            let beside = world
                .ground()
                .standing(core[0] + 3, core[1])
                .expect("a column beside the core");
            world
                .raise_structure(
                    seat,
                    id,
                    pharmakos_sim::tables::StructureKind::ResonanceSpire,
                    beside,
                )
                .expect("room for a Spire");
        }
        terrain.build(&mut world, core, vent);
        let commander = world.commander_of(seat).expect("a commander");
        let row = world.units().row_of(commander).expect("a live commander");
        let start = world.units().positions()[row.index()];
        let mut scratch = Scratch::for_map(world.surface(), world.clusters().size()).unwrap();
        let mut stones: Vec<StoneCandidate> = Vec::new();
        let found = toward_counted(
            &world.ground(),
            &mut scratch,
            &mut stones,
            seat,
            start,
            FeatureSpec::Name {
                kind: FeatureKind::Vent,
                anchor: vent,
            },
        )
        .expect("a stone");
        assert_eq!(
            scratch.walks().unwrap(),
            0,
            "{label}: a stone prices no walk"
        );
        let pops = popped(&scratch);
        let evaluations = scratch.reach().evaluations().unwrap();
        println!(
            "{label}: vent {vent:?}, stone {:?}, {} sites, {} spheres, {pops} nodes popped, {evaluations} h",
            found.site,
            stones.len(),
            scratch.reach().spheres().len()
        );
        measured.push((label, found.site, pops, evaluations));
    }
    assert_eq!(measured, PINNED_RESOLVES.to_vec());
}

/// A terrain for [`a_toward_resolve_pops_only_what_is_closer_than_the_stone`].
#[derive(Clone, Copy)]
enum Terrain {
    /// As generated.
    Open,
    /// A ring of stone three high round the vent, its gap facing away from
    /// the core (review probe B).
    Ring(i32),
    /// That many beacons of seat 0 scattered over the map, each sealed in a
    /// ring of stone three high, so their spheres list no site (re-review 4's
    /// decoys).
    Decoys(u32),
    /// A wall of stone across the line from the core to the vent, open only
    /// far to one side (re-review 4).
    LongWall,
    /// Half a ring round the vent, the half facing the core, open away from
    /// it (re-review 4's cup).
    Cup(i32),
}

impl Terrain {
    fn build(self, world: &mut World, core: [i32; 2], vent: [i32; 2]) {
        let mut enc = Enc::with_capacity(64 * 1024);
        let [sx, sy, _] = world.surface().size();
        match self {
            Terrain::Open => {}
            Terrain::Ring(radius) => {
                ring(world, vent, radius, [vent[0] - core[0], vent[1] - core[1]]);
            }
            Terrain::Decoys(count) => {
                let radius = i32::from(world.rules().sphere_radius_voxels()) + 3;
                let (mut placed, mut tries) = (0, 0);
                let mut rng = Rng(0xdec0);
                while placed < count && tries < 2_000 {
                    tries += 1;
                    let (x, y) = (
                        radius + 2 + rng.below(sx - 2 * radius - 4),
                        radius + 2 + rng.below(sy - 2 * radius - 4),
                    );
                    if (x - core[0]).abs() < 3 * radius && (y - core[1]).abs() < 3 * radius {
                        continue;
                    }
                    if place(world, SeatId::new(0), x, y) {
                        ring(world, [x, y], radius, [0, 0]);
                        placed += 1;
                    }
                }
                assert_eq!(placed, count, "the decoys are placed");
            }
            Terrain::LongWall => {
                let (mx, my) = (
                    (core[0] + vent[0]).div_euclid(2),
                    (core[1] + vent[1]).div_euclid(2),
                );
                let (dx, dy) = (vent[0] - core[0], vent[1] - core[1]);
                let (px, py) = if dx.abs() > dy.abs() { (0, 1) } else { (1, 0) };
                for i in -400..=150 {
                    let (x, y) = (mx + px * i, my + py * i);
                    if x < 0 || y < 0 || x >= sx || y >= sy {
                        continue;
                    }
                    for t in 0..2 {
                        wall_column(world, &mut enc, x + py * t, y + px * t, 3);
                    }
                }
                settle(world, &mut enc);
            }
            Terrain::Cup(radius) => {
                let away = [vent[0] - core[0], vent[1] - core[1]];
                for dy in -radius..=radius {
                    for dx in -radius..=radius {
                        let d2 = dx * dx + dy * dy;
                        if d2 < (radius - 2) * (radius - 2)
                            || d2 > radius * radius
                            || dx * away[0] + dy * away[1] > 0
                        {
                            continue;
                        }
                        wall_column(world, &mut enc, vent[0] + dx, vent[1] + dy, 3);
                    }
                }
                settle(world, &mut enc);
            }
        }
    }
}

/// Place a beacon of `seat` at `(x, y)` or a column beside it.
fn place(world: &mut World, seat: SeatId, x: i32, y: i32) -> bool {
    for (ox, oy) in [(0, 0), (1, 0), (0, 1), (-1, 0), (0, -1), (2, 2), (-2, -2)] {
        if let Some(at) = world.ground().standing(x + ox, y + oy)
            && world
                .place_beacon_directly(
                    seat,
                    at,
                    pharmakos_sim::seams::MandateKind::Build,
                    pharmakos_sim::tables::PRIORITY_NORMAL,
                )
                .is_some()
        {
            return true;
        }
    }
    false
}

/// The live vent farthest from `core`, ties to the lowest `(y, x)`.
fn farthest_vent(world: &World, core: [i32; 2]) -> [i32; 2] {
    world
        .features()
        .features()
        .iter()
        .filter(|feature| feature.kind == FeatureKind::Vent)
        .map(|feature| feature.anchor)
        .max_by_key(|a| {
            let (dx, dy) = (i64::from(a[0] - core[0]), i64::from(a[1] - core[1]));
            (dx * dx + dy * dy, a[1], a[0])
        })
        .expect("a vent")
}

/// A full flood of the map from each of three seats' cores, directed at
/// 1, 18 and 40 spheres no column reaches, against the undirected flood:
/// the directed flood pops the same columns, and computes one `h` a column
/// and no more, whatever the spheres. Counted work, pinned.
#[test]
fn a_directed_flood_computes_one_h_a_column() {
    let world = world_of(PROBATION, 3);
    let surface = world.surface();
    let mut reach = Reach::new(surface.node_count(), surface.costs().max_step()).expect("a ring");
    let mut measured = Vec::new();
    for spheres in [0_i32, 1, 18, 40] {
        let (mut pops, mut evaluations) = (0, 0);
        for seat in 0..3_u8 {
            let row = world
                .beacons()
                .seats()
                .iter()
                .position(|owner| *owner == seat)
                .expect("a core");
            let core = column_of(world.beacons().positions()[row]);
            let from = surface.node_of(core[0] + 2, core[1] + 2).expect("a column");
            reach.clear_sites();
            for k in 0..spheres {
                reach.add_sphere([5 + (k % 8) * 40, 5 + k.div_euclid(8) * 60], 240);
            }
            let mut tally = Tally::ZERO;
            if spheres == 0 {
                reach.begin(from).expect("the start is on the map");
            } else {
                reach
                    .begin_toward(surface, from)
                    .expect("the start is on the map");
            }
            while reach
                .next(surface, &mut tally)
                .expect("on the map")
                .is_some()
            {}
            pops += tally.read().unwrap();
            evaluations += reach.evaluations().unwrap();
        }
        measured.push((spheres, pops, evaluations));
    }
    let columns = u64::from(surface.node_count()) * 3;
    assert_eq!(
        measured,
        [
            (0, columns, 0),
            (1, columns, columns),
            (18, columns, columns),
            (40, columns, columns),
        ]
    );
}

/// [`a_toward_resolve_pops_only_what_is_closer_than_the_stone`]'s pins:
/// nodes popped and `h` computed, measured at this lane's fix pass 4, where
/// `h` came to be computed once a column and only spheres that listed a
/// site, each with the reach of its own sites, to be headed for. Fix pass 3
/// popped 968, 1 142, 1 906, 4 540, 24 778, 568 and 27 240 nodes on the first
/// seven; undirected (fix pass 2) the same stones cost 10 020, 16 699,
/// 23 054, 32 401, 106 531, 6 026 and 105 410. While the stone was the least
/// served walk, the same search bounding the walks priced 82, 11, 45 and 9
/// walks on probe B's four terrains and 814 (2.9 M nodes) on the farthest
/// vent with a Spire and a ring; with the octile bound before that, probe B
/// priced 140, 1 261, 1 753 and 1 753 walks and popped 0.3 M, 3.1 M, 4.5 M
/// and 4.5 M nodes.
const PINNED_RESOLVES: [(&str, [i32; 2], u64, u64); 10] = [
    ("open", [197, 50], 937, 1_130),
    ("ring 10", [201, 48], 1_109, 1_329),
    ("ring 18", [208, 41], 1_879, 2_126),
    ("ring 26", [208, 41], 4_365, 4_842),
    ("farthest", [179, 50], 24_654, 25_597),
    ("pinned, Spire", [202, 61], 556, 744),
    ("farthest, Spire, ring 26", [179, 62], 27_038, 27_986),
    ("farthest, 18 decoy spheres", [179, 50], 12_626, 13_297),
    ("farthest, long wall", [197, 50], 46_791, 47_714),
    ("farthest, cup", [179, 50], 28_941, 29_974),
];

/// A commander sent along far pairs on scarred terrain arrives on the tick
/// the served walk's cost promises, counted from the tick that serves the
/// route (review probe C, committed): `walk_cost` is the walk, not an
/// estimate of it.
#[test]
fn a_far_walk_costs_what_the_walker_walks() {
    let mut world = world_of(SEED, 3);
    scar(&mut world, SEED);
    let mut enc = Enc::with_capacity(64 * 1024);
    let seat = SeatId::new(0);
    let commander = world.commander_of(seat).unwrap();
    let unit = world.units().row_of(commander).unwrap().raw();
    let speed = Speed::commander(world.rules());
    let mut scratch = Scratch::for_map(world.surface(), world.clusters().size()).unwrap();
    let [sx, sy, _] = world.surface().size();
    let mut rng = Rng(0x00C0_FFEE);
    let (mut walks, mut tries) = (0, 0);
    let mut differ = Vec::new();
    while walks < 20 && tries < 4_000 {
        tries += 1;
        let (ax, ay, bx, by) = (rng.below(sx), rng.below(sy), rng.below(sx), rng.below(sy));
        let (Some(a), Some(b)) = (
            world.surface().node_of(ax, ay),
            world.surface().node_of(bx, by),
        ) else {
            continue;
        };
        if a == b
            || !world.surface().walkable(a)
            || !world.surface().walkable(b)
            || !world.clusters().connected(world.surface(), a, b)
            || world.clusters().touch(a, b) != Ok(false)
        {
            continue;
        }
        let Ok(walk) = walk_cost(world.surface(), world.clusters(), &mut scratch, (a, b)) else {
            continue;
        };
        let promised = i64::from(
            pharmakos_sim::pathing::estimate::walk_ticks(walk, speed)
                .unwrap()
                .unwrap(),
        );
        if !world.place_unit_directly(commander, world.standing_point([ax, ay, 0])) {
            continue;
        }
        world.send_unit_directly(unit, world.standing_point([bx, by, 0]));
        world.step(&mut enc).unwrap();
        let mut ticks = 0_i64;
        let mut arrived = false;
        while ticks < promised * 3 + 200 {
            world.step(&mut enc).unwrap();
            ticks += 1;
            if world.router().arrived().contains(&unit) {
                arrived = true;
                break;
            }
        }
        assert!(
            arrived,
            "{ax},{ay} to {bx},{by}: not arrived in {ticks} ticks"
        );
        walks += 1;
        if ticks != promised {
            differ.push(([ax, ay], [bx, by], promised, ticks));
        }
    }
    assert_eq!(walks, 20, "the test walks enough far pairs");
    assert!(differ.is_empty(), "walked ticks against priced: {differ:?}");
}

/// Seat 0's legal `toward` sites from its commander, as a resolve toward
/// `anchor` lists them.
fn sites_toward(world: &World, scratch: &mut Scratch, anchor: [i32; 2]) -> Vec<StoneCandidate> {
    let seat = SeatId::new(0);
    let commander = world.commander_of(seat).expect("a commander");
    let row = world.units().row_of(commander).expect("a live commander");
    let start = world.units().positions()[row.index()];
    let mut stones: Vec<StoneCandidate> = Vec::new();
    let _ = toward_counted(
        &world.ground(),
        scratch,
        &mut stones,
        seat,
        start,
        FeatureSpec::Name {
            kind: FeatureKind::Vent,
            anchor,
        },
    );
    stones
}

/// The least cost from every column to the nearest of `sites`, by the test's
/// own multi-source Dijkstra (steps read backwards through
/// `step_cost_between`).
fn least_to_any(world: &World, sites: &[Node]) -> Vec<Option<i64>> {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let surface = world.surface();
    let n = usize::try_from(surface.node_count()).unwrap();
    let mut cost: Vec<Option<i64>> = vec![None; n];
    let mut heap = BinaryHeap::new();
    for site in sites {
        cost[usize::try_from(*site).unwrap()] = Some(0);
        heap.push(Reverse((0_i64, *site)));
    }
    while let Some(Reverse((here, node))) = heap.pop() {
        if cost[usize::try_from(node).unwrap()] != Some(here) {
            continue;
        }
        let (x, y) = surface.coord_of(node);
        for dy in -1..=1 {
            for dx in -1..=1 {
                let Some(other) = surface.node_of(x + dx, y + dy) else {
                    continue;
                };
                let Some(step) = surface.step_cost_between(other, node) else {
                    continue;
                };
                if other == node {
                    continue;
                }
                let next = here + i64::from(step);
                let slot = &mut cost[usize::try_from(other).unwrap()];
                if slot.is_none_or(|held| next < held) {
                    *slot = Some(next);
                    heap.push(Reverse((next, other)));
                }
            }
        }
    }
    cost
}

/// The directed search's heuristic (`pathing::reach`, "Directed"), read from
/// the sphere list a real resolve built, is admissible and consistent on
/// generated and scarred terrain: for every column, never above the true
/// least cost to the nearest legal site (by the test's own multi-source
/// Dijkstra), and across every legal step never falling by more than that
/// step's cost. A sphere's reach taken short of its farthest own site, or a
/// sphere left out, fails it.
#[test]
fn the_directed_search_heuristic_is_admissible_and_consistent() {
    let mut checked = 0_u64;
    for (seed, scarred, spire) in [
        (PROBATION, false, false),
        (PROBATION, true, true),
        (SEED, true, false),
    ] {
        let mut world = world_of(seed, 2);
        let seat = SeatId::new(0);
        if spire {
            let core_row = world
                .beacons()
                .seats()
                .iter()
                .position(|owner| *owner == seat.raw())
                .expect("a core");
            let core = column_of(world.beacons().positions()[core_row]);
            let id = BeaconId::new(world.beacons().ids()[core_row]);
            let beside = world
                .ground()
                .standing(core[0] + 3, core[1])
                .expect("a column beside the core");
            world
                .raise_structure(
                    seat,
                    id,
                    pharmakos_sim::tables::StructureKind::ResonanceSpire,
                    beside,
                )
                .expect("room for a Spire");
        }
        if scarred {
            scar(&mut world, seed);
        }
        let surface = world.surface();
        let mut scratch = Scratch::for_map(surface, world.clusters().size()).unwrap();
        let vent = world
            .features()
            .features()
            .iter()
            .enumerate()
            .find(|(index, feature)| {
                feature.kind == FeatureKind::Vent && world.feature_is_live(*index)
            })
            .map(|(_, feature)| feature.anchor)
            .expect("a live vent");
        let stones = sites_toward(&world, &mut scratch, vent);
        assert!(!stones.is_empty(), "seed {seed:#x}: the seat has sites");
        let sites: Vec<Node> = stones.iter().map(StoneCandidate::node).collect();
        let truth = least_to_any(&world, &sites);
        // The heuristic as the resolve left it: the production sphere list,
        // each sphere that listed a site with its reach over its own sites,
        // the search directed at them.
        let reach = scratch.reach();
        assert!(
            !reach.spheres().is_empty(),
            "seed {seed:#x}: the resolve heads for spheres"
        );
        for node in 0..surface.node_count() {
            let h = reach.heuristic(surface, node);
            assert!(h >= 0);
            if let Some(cost) = truth[usize::try_from(node).unwrap()] {
                assert!(
                    h <= cost,
                    "seed {seed:#x}: h {h} above the true cost {cost} at {node}"
                );
                checked += 1;
            }
            for k in 0..pharmakos_sim::pathing::search::neighbour_count() {
                if let Some((other, step)) = surface.neighbour(node, k) {
                    let across = reach.heuristic(surface, other);
                    assert!(
                        h <= i64::from(step) + across,
                        "seed {seed:#x}: h falls {h} to {across} across a step of {step}"
                    );
                }
            }
        }
    }
    assert!(checked > 300_000, "the columns checked: {checked}");
}

/// A `toward` with no legal site searches nothing (fix pass 3): seat 0's
/// whole sphere ringed shut by stone three high and the commander outside
/// it, so no site is walkable to; named or `nearest`, the step is refused
/// `illegal_site` counting no site, with no node popped and no walk priced.
/// And a `toward` whose sites exist but whose named target the commander
/// cannot reach (the vent ringed shut) is refused counting its sites,
/// searching nothing either.
#[test]
fn a_toward_that_finds_no_stone_searches_nothing() {
    let seat = SeatId::new(0);
    let world_shut = {
        let mut world = world_of(PROBATION, 2);
        let core_row = world
            .beacons()
            .seats()
            .iter()
            .position(|owner| *owner == seat.raw())
            .expect("a core");
        let core = column_of(world.beacons().positions()[core_row]);
        let radius = i32::from(world.rules().sphere_radius_voxels()) + 3;
        ring(&mut world, core, radius, [0, 0]);
        let outside = world
            .ground()
            .standing(core[0] - radius - 6, core[1])
            .or_else(|| world.ground().standing(core[0] + radius + 6, core[1]))
            .expect("a column outside the ring");
        (world, outside)
    };
    let (world, outside) = &world_shut;
    for spec in [
        FeatureSpec::Name {
            kind: FeatureKind::Vent,
            anchor: PROBATION_VENT,
        },
        FeatureSpec::Nearest {
            kind: FeatureKind::Vent,
            uncovered: false,
        },
    ] {
        let mut scratch = Scratch::for_map(world.surface(), world.clusters().size()).unwrap();
        let mut stones: Vec<StoneCandidate> = Vec::new();
        let found = toward_counted(
            &world.ground(),
            &mut scratch,
            &mut stones,
            seat,
            *outside,
            spec,
        );
        assert_eq!(
            found,
            Err(Unanswered::Ran(StepFailed::counted(
                StepFailure::IllegalSite,
                0
            ))),
            "{spec:?}"
        );
        assert_eq!(popped(&scratch), 0, "{spec:?}: no node popped");
        assert_eq!(scratch.walks().unwrap(), 0, "{spec:?}: no walk priced");
    }
    // The vent ringed shut, the seat's sphere open.
    let mut world = world_of(PROBATION, 2);
    ring(&mut world, PROBATION_VENT, 3, [0, 0]);
    let commander = world.commander_of(seat).expect("a commander");
    let row = world.units().row_of(commander).expect("a live commander");
    let start = world.units().positions()[row.index()];
    let mut scratch = Scratch::for_map(world.surface(), world.clusters().size()).unwrap();
    let mut stones: Vec<StoneCandidate> = Vec::new();
    let found = toward_counted(
        &world.ground(),
        &mut scratch,
        &mut stones,
        seat,
        start,
        FeatureSpec::Name {
            kind: FeatureKind::Vent,
            anchor: PROBATION_VENT,
        },
    );
    let weighed = u32::try_from(stones.len()).unwrap();
    assert!(weighed > 0, "the seat has sites");
    assert_eq!(
        found,
        Err(Unanswered::Ran(StepFailed::counted(
            StepFailure::IllegalSite,
            weighed
        )))
    );
    assert_eq!(
        popped(&scratch),
        0,
        "the unreachable target: no node popped"
    );
}

/// The premise "nearest" prices its walks without a surcharge on (`targeting`,
/// "Two measures"): no walker of a ranked choice is served one. A commander's
/// route is surcharged only while a `move` walks at `AVOID_KNOWN_THREATS`, and
/// the compile refuses `covering` and `toward` in a `move`, so a ranked site
/// is walked to by a `place_beacon`: the route the pathing phase serves it,
/// with an armed enemy the seat sees standing on the way, is the route under
/// no surcharge.
/// Whoever lets a `move` take a description meets this test, and the ranker's
/// measure with it.
#[test]
fn ranked_choices_are_walked_without_a_surcharge() {
    for to in [
        r#"{"covering":{"vent":{"rank":"NEAREST","coverage":"ANY"}}}"#,
        r#"{"toward":{"feature":{"feature_id":"vent_232_96"}}}"#,
    ] {
        let route = format!(
            r#"{{"label":"go","move":{{"to":{to},"pace":"AVOID_KNOWN_THREATS"}},"timeout_ms":60000}}"#
        );
        assert!(compile(&route).is_err(), "a move to {to} compiles");
    }
    // In play: the route the pathing phase serves a `place_beacon` walking
    // in to its ranked site is the unsurcharged route, with an armed enemy
    // the seat sees standing on it, where a surcharge round that enemy would
    // have served another.
    let first = served_walk_in(None).expect("the commander walks in to its site");
    let on_it = first[first.len().min(14)..]
        .first()
        .copied()
        .expect("a column out");
    let served = served_walk_in(Some(on_it)).expect("the commander walks in past the raider");
    let world = world_of(PROBATION, 2);
    let surface = world.surface();
    let mut scratch = Scratch::for_map(surface, world.clusters().size()).unwrap();
    let (from, to) = (served[0], *served.last().unwrap());
    let mut straight = Vec::new();
    route_with(
        surface,
        world.clusters(),
        &mut scratch,
        (from, to),
        Surcharge::NONE,
        &mut straight,
    )
    .unwrap();
    assert_eq!(
        served, straight,
        "the walk-in is served the unsurcharged route"
    );
    let (x, y) = surface.coord_of(on_it);
    let zone = [pharmakos_sim::pathing::route::Zone::Disc {
        centre: [
            pharmakos_sim::math::fixed::Fx::from_voxels(i16::try_from(x).unwrap()),
            pharmakos_sim::math::fixed::Fx::from_voxels(i16::try_from(y).unwrap()),
            pharmakos_sim::math::fixed::Fx::from_raw(0),
        ],
        limit: pharmakos_sim::math::fixed::Sq::of_radius(
            pharmakos_sim::math::fixed::Fx::from_voxels(6),
        ),
    }];
    let mut round = Vec::new();
    route_with(
        surface,
        world.clusters(),
        &mut scratch,
        (from, to),
        Surcharge::new(&zone, world.rules().attack().step_multiplier),
        &mut round,
    )
    .unwrap();
    assert_ne!(
        round, straight,
        "a surcharge round the raider would serve another route"
    );
}

/// Seat 0 seals a `place_beacon` covering its nearest uncovered vent; with
/// `raider_at`, an armed raider of seat 1 holding fire stands on that column
/// first. The route the pathing phase serves the commander's walk-in, the
/// tick it is served; `None` when none is in 200 ticks.
fn served_walk_in(raider_at: Option<Node>) -> Option<Vec<Node>> {
    let mut world = world_of(PROBATION, 2);
    let seat = SeatId::new(0);
    if let Some(column) = raider_at {
        let (x, y) = world.surface().coord_of(column);
        let at = world.standing_point([x, y, 0]);
        let enemy = SeatId::new(1);
        let home = world
            .place_beacon_directly(
                enemy,
                at,
                pharmakos_sim::seams::MandateKind::Build,
                pharmakos_sim::tables::PRIORITY_NORMAL,
            )
            .expect("room for the enemy's beacon");
        world.set_common(
            home,
            pharmakos_sim::tables::Common {
                roe: 1,
                retreat_hp_pct: 0,
            },
        );
        world
            .field_unit(enemy, home, pharmakos_sim::tables::UnitKind::Raider, at)
            .expect("room for the raider");
    }
    let unit = world
        .units()
        .row_of(world.commander_of(seat).expect("a commander"))
        .expect("a live commander")
        .raw();
    let route = r#"{"label":"cover","place_beacon":{"at":{"covering":{"vent":{"rank":"NEAREST","coverage":"UNCOVERED"}}}},"timeout_ms":120000}"#;
    let mut runner = Runner::new(world).unwrap();
    runner
        .seal_playbook(seat, compile(route).expect("the route compiles"))
        .expect("a Lull takes a seal");
    assert!(runner.begin_push().unwrap());
    for _ in 0..200 {
        runner
            .step()
            .unwrap()
            .expect("the segment outlasts this test");
        let served = runner.world().router().route(unit);
        if served.len() > 1 {
            return Some(served.to_vec());
        }
    }
    None
}

/// A whole playbook around `route`, compiled.
fn compile(route: &str) -> Result<Plan, pharmakos_sim::interpreter::PlanError> {
    let text = format!(
        concat!(
            "{{\"schema_version\":{{\"major\":1}},",
            "\"meta\":{{\"title\":\"nearest\",\"author_kind\":\"HUMAN\"}},",
            "\"declarative\":{{\"route\":[{route}]}},",
            "\"on_death\":{{\"on_respawn\":\"CONTINUE\",\"max_deaths_before_fallback\":3}},",
            "\"fallback\":{{\"hold\":{{\"at\":{{\"safest\":{{}}}}}}}},",
            "\"kind\":\"PLAYBOOK\"}}"
        ),
        route = route
    );
    let playbook: gp::v1::Playbook = json::decode(&text).expect("the playbook decodes");
    Plan::compile(&playbook, &rules())
}

/// The site listing's own cost, part of every `toward` resolve, directed or
/// not: seat 0 at the most beacons the rules let a seat place
/// (`room_per_seat` of the 40-beacon world total, 20 at two seats, beside its
/// core: 21 spheres), spread so their spheres overlap as little as the map
/// allows. Pinned: the decision units (one a column scanned and one a node
/// popped), the sites listed, the spheres that listed one, and the nodes the
/// search then pops.
#[test]
fn the_site_listing_at_a_seats_most_beacons() {
    let mut world = world_of(PROBATION, 2);
    let seat = SeatId::new(0);
    let room = pharmakos_sim::world::room_per_seat(40, 2);
    let [sx, sy, _] = world.surface().size();
    let mut placed = 0;
    'grid: for gy in 0..6 {
        for gx in 0..6 {
            if placed == room {
                break 'grid;
            }
            let (x, y) = (30 + gx * 64, 30 + gy * 64);
            if x < sx - 8 && y < sy - 8 && place(&mut world, seat, x, y) {
                placed += 1;
            }
        }
    }
    assert_eq!(placed, room, "the seat holds its most beacons");
    let commander = world.commander_of(seat).expect("a commander");
    let row = world.units().row_of(commander).expect("a live commander");
    let start = world.units().positions()[row.index()];
    let tally = pharmakos_sim::seams::UnitTally::new();
    let ground = pharmakos_sim::targeting::Ground {
        work: &tally,
        ..world.ground()
    };
    let mut scratch = Scratch::for_map(world.surface(), world.clusters().size()).unwrap();
    let mut stones: Vec<StoneCandidate> = Vec::new();
    let found = toward_counted(
        &ground,
        &mut scratch,
        &mut stones,
        seat,
        start,
        FeatureSpec::Name {
            kind: FeatureKind::Vent,
            anchor: PROBATION_VENT,
        },
    );
    let measured = (
        found.ok().map(|stone| stone.site),
        tally.read(),
        stones.len(),
        scratch.reach().spheres().len(),
        popped(&scratch),
    );
    println!(
        "the most beacons: {measured:?}; {} columns listed",
        listed_columns(&world, seat)
    );
    assert_eq!(
        measured.1,
        Ok(toward_named_steps(&world, seat, start, &scratch) + measured.4),
        "a toward charges what it visits and its pops, and nothing else"
    );
    // Pinned on the probation seed: the decision units (one a visit of each
    // loop the resolve runs, and one a node popped), the sites listed, the
    // spheres that listed one, and the nodes the search then pops. Before
    // dunit part 2 the units were the 37,653 columns scanned and the 14 pops
    // alone (37,667): the tests each column asks, the beacon rows, the Spire
    // tests and the `h` folds were charged nothing. After it they read
    // 1,685,732; S3's `walk3` counts the eight neighbour slots each of the 14
    // pops tries, 112 more.
    assert_eq!(measured, (Some([232, 94]), Ok(1_685_844), 36_513, 21, 14));
}

// ---------------------------------------------------------------------------
// Decision units follow counted work (lane `dunit`)
// ---------------------------------------------------------------------------

/// The test's own count of what a resolve visits, made from the tables and
/// not from the resolver (dunit part 2): **one unit a visit of each loop, at
/// every level down to the innermost** -- a feature row, a footprint column,
/// a voxel of a seam's depth, a beacon, structure or target row, a Spire
/// test's kind byte or structure row, a ranker entry, a spiral or disc
/// column. A scan that stops at the row that answers visits up to that row.
/// Each function answers what the sim's predicate answers, so the walk the
/// count follows is the resolver's, and the count beside it.
struct Steps<'w> {
    world: &'w World,
}

impl Steps<'_> {
    /// The anchor's footprint column, and the columns compared to find it.
    fn anchor_column(
        feature: &pharmakos_sim::features::Feature,
    ) -> (Option<pharmakos_sim::features::FootprintColumn>, u64) {
        let mut rows = 0;
        for column in &feature.footprint {
            rows += 1;
            if column.x == feature.anchor[0] && column.y == feature.anchor[1] {
                return (Some(*column), rows);
            }
        }
        (None, rows)
    }

    /// Which feature covers `(x, y)`: a feature row asked, and the footprint
    /// columns of each whose box holds the column.
    fn at_column(&self, x: i32, y: i32) -> (bool, u64) {
        let mut rows = 0;
        for feature in self.world.features().features() {
            rows += 1;
            let [min_x, min_y, max_x, max_y] = feature.bounds;
            if x < min_x || x > max_x || y < min_y || y > max_y {
                continue;
            }
            for column in &feature.footprint {
                rows += 1;
                if column.x == x && column.y == y {
                    return (true, rows);
                }
            }
        }
        (false, rows)
    }

    /// The feature row whose kind and anchor match.
    fn index_of(&self, kind: FeatureKind, anchor: [i32; 2]) -> (Option<usize>, u64) {
        let mut rows = 0;
        for (index, feature) in self.world.features().features().iter().enumerate() {
            rows += 1;
            if feature.kind == kind && feature.anchor == anchor {
                return (Some(index), rows);
            }
        }
        (None, rows)
    }

    /// Whether the feature is live: a footprint column asked, and a seam's
    /// depth voxels read.
    fn is_live(&self, index: usize) -> (bool, u64) {
        let Some(feature) = self.world.features().features().get(index) else {
            return (false, 0);
        };
        let surface = self.world.surface();
        let voxels = self.world.voxels();
        let mut rows = 0;
        for column in &feature.footprint {
            rows += 1;
            match feature.kind {
                FeatureKind::Vent => {
                    if let Some(node) = surface.node_of(column.x, column.y)
                        && voxels
                            .get([column.x, column.y, surface.top(node)])
                            .and_then(Material::vent_richness)
                            .is_some()
                    {
                        return (true, rows);
                    }
                }
                FeatureKind::Seam => {
                    for depth in 0..pharmakos_sim::features::SEAM_DEPTH_VOXELS {
                        rows += 1;
                        if voxels
                            .get([column.x, column.y, column.top.saturating_sub(depth)])
                            .and_then(Material::ore_richness)
                            .is_some()
                        {
                            return (true, rows);
                        }
                    }
                }
            }
        }
        (false, rows)
    }

    /// The radius of the beacon in `row`: the kind bytes compared to the
    /// first Spire's, and, with one, the structure rows read to the Spire
    /// that grows it.
    fn radius_at_row(&self, row: usize) -> (i16, u64) {
        let rules = self.world.rules();
        let kinds = self.world.structures().kinds();
        let spire = pharmakos_sim::tables::StructureKind::ResonanceSpire.id();
        let Some(at) = kinds.iter().position(|kind| *kind == spire) else {
            return (
                rules.sphere_radius_voxels(),
                u64::try_from(kinds.len()).unwrap(),
            );
        };
        let mut rows = u64::try_from(at + 1).unwrap();
        let seat = self.world.beacons().seats()[row];
        let id = self.world.beacons().ids()[row];
        for facts in pharmakos_sim::spire::facts_of(self.world.structures()) {
            rows += 1;
            if facts.grows(seat, id) {
                return (rules.grown_sphere_radius_voxels(), rows);
            }
        }
        (rules.sphere_radius_voxels(), rows)
    }

    /// Whether `point` is inside the sphere of the beacon in `row`: the rows
    /// its radius reads, asked only between the base and the grown radius.
    fn inside_at_row(&self, row: usize, centre: [Fx; 3], point: [Fx; 3]) -> (bool, u64) {
        let rules = self.world.rules();
        let distance = Sq::between(centre, point);
        if distance <= Sq::of_radius(Fx::from_voxels(rules.sphere_radius_voxels())) {
            return (true, 0);
        }
        if distance > Sq::of_radius(Fx::from_voxels(rules.grown_sphere_radius_voxels())) {
            return (false, 0);
        }
        let (radius, rows) = self.radius_at_row(row);
        (radius == rules.grown_sphere_radius_voxels(), rows)
    }

    /// Whether `point` is in one of `seat`'s own living spheres: a beacon row
    /// read, and each own living beacon's sphere test.
    fn inside_own_sphere(&self, seat: SeatId, point: [Fx; 3]) -> (bool, u64) {
        let beacons = self.world.beacons();
        let mut rows = 0;
        for row in 0..beacons.seats().len() {
            rows += 1;
            if beacons.seats()[row] != seat.raw() || !beacons.hit_points()[row].is_alive() {
                continue;
            }
            let (inside, read) = self.inside_at_row(row, beacons.positions()[row], point);
            rows += read;
            if inside {
                return (true, rows);
            }
        }
        (false, rows)
    }

    /// Whether one of `seat`'s own living beacons stands on `(x, y)`.
    fn stacks_on_own(&self, seat: SeatId, x: i32, y: i32) -> (bool, u64) {
        let beacons = self.world.beacons();
        let mut rows = 0;
        for row in 0..beacons.seats().len() {
            rows += 1;
            if beacons.seats()[row] == seat.raw()
                && beacons.hit_points()[row].is_alive()
                && column_of(beacons.positions()[row]) == [x, y]
            {
                return (true, rows);
            }
        }
        (false, rows)
    }

    /// Whether a structure with hit points left stands on `(x, y)`.
    fn structure_on(&self, x: i32, y: i32) -> (bool, u64) {
        let structures = self.world.structures();
        let mut rows = 0;
        for row in 0..structures.hit_points().len() {
            rows += 1;
            if structures.hit_points()[row].is_alive()
                && column_of(structures.positions()[row]) == [x, y]
            {
                return (true, rows);
            }
        }
        (false, rows)
    }

    /// Whether a Build target of `seat`'s (but `except`'s) claims `(x, y)`.
    fn own_target_on(&self, seat: SeatId, except: Option<BeaconId>, x: i32, y: i32) -> (bool, u64) {
        let targets = self.world.targets();
        let build = pharmakos_sim::tables::TargetKind::Build.id();
        let mut rows = 0;
        for row in 0..targets.beacons().len() {
            rows += 1;
            let beacon = targets.beacons()[row];
            if except == Some(beacon) || targets.kinds()[row] != build {
                continue;
            }
            let owned = self
                .world
                .beacons()
                .row_of(beacon)
                .is_ok_and(|at| self.world.beacons().seats()[at.index()] == seat.raw());
            if owned && column_of(targets.anchors()[row]) == [x, y] {
                return (true, rows);
            }
        }
        (false, rows)
    }

    /// A column no live structure and no claim of `seat`'s stands on.
    fn free(&self, seat: SeatId, except: Option<BeaconId>, x: i32, y: i32) -> (bool, u64) {
        let (on, rows) = self.structure_on(x, y);
        if on {
            return (false, rows);
        }
        let (claimed, more) = self.own_target_on(seat, except, x, y);
        (!claimed, rows + more)
    }

    /// The feature's `on` column.
    fn on_column(
        &self,
        index: usize,
        seat: SeatId,
        except: Option<BeaconId>,
    ) -> (Option<[i32; 2]>, u64) {
        let feature = &self.world.features().features()[index];
        let (anchor, mut rows) = Self::anchor_column(feature);
        if let Some(column) = anchor {
            let (free, read) = self.free(seat, except, column.x, column.y);
            rows += read;
            if free {
                return (Some([column.x, column.y]), rows);
            }
        }
        for column in &feature.footprint {
            rows += 1;
            let (free, read) = self.free(seat, except, column.x, column.y);
            rows += read;
            if free {
                return (Some([column.x, column.y]), rows);
            }
        }
        (None, rows)
    }

    /// The feature's anchor point.
    fn anchor_point(&self, index: usize) -> (Option<[Fx; 3]>, u64) {
        let feature = &self.world.features().features()[index];
        let (anchor, rows) = Self::anchor_column(feature);
        let column = anchor.or_else(|| feature.footprint.first().copied());
        (
            column.and_then(|column| self.world.ground().standing(column.x, column.y)),
            rows,
        )
    }

    /// The column a "nearest" measures to: the anchor test, and a footprint
    /// column tried after it.
    fn measure_to(&self, index: usize, origin: Node) -> (Option<Node>, u64) {
        let feature = &self.world.features().features()[index];
        let surface = self.world.surface();
        let reachable = |x: i32, y: i32| {
            let node = surface.node_of(x, y)?;
            (surface.walkable(node) && self.world.clusters().connected(surface, origin, node))
                .then_some(node)
        };
        let (anchor, mut rows) = Self::anchor_column(feature);
        if let Some(column) = anchor
            && let Some(node) = reachable(column.x, column.y)
        {
            return (Some(node), rows);
        }
        for column in &feature.footprint {
            rows += 1;
            if let Some(node) = reachable(column.x, column.y) {
                return (Some(node), rows);
            }
        }
        (None, rows)
    }

    /// A `covering` pick's filter: kind, liveness and, for `UNCOVERED`, the
    /// anchor outside the seat's spheres.
    fn matches_pick(
        &self,
        seat: SeatId,
        index: usize,
        kind: FeatureKind,
        uncovered: bool,
    ) -> (bool, u64) {
        if self.world.features().features()[index].kind != kind {
            return (false, 0);
        }
        let (live, mut rows) = self.is_live(index);
        if !live {
            return (false, rows);
        }
        if !uncovered {
            return (true, rows);
        }
        let (point, read) = self.anchor_point(index);
        rows += read;
        let Some(point) = point else {
            return (false, rows);
        };
        let (inside, read) = self.inside_own_sphere(seat, point);
        (!inside, rows + read)
    }

    /// A ranker's construction from `origin`: one unit a feature scanned,
    /// the rows `keep` reads, and the rows each kept feature's measure
    /// reads. Answers the reachable entries and the rows.
    fn ranker_new(&self, origin: [i32; 2], keep: impl Fn(usize) -> (bool, u64)) -> (u64, u64) {
        let start = self.world.surface().node_of(origin[0], origin[1]);
        let (mut entries, mut rows) = (0, 0);
        for index in 0..self.world.features().len() {
            rows += 1;
            let (kept, read) = keep(index);
            rows += read;
            if !kept {
                continue;
            }
            if let Some(start) = start {
                let (goal, read) = self.measure_to(index, start);
                rows += read;
                entries += u64::from(goal.is_some());
            }
        }
        (entries, rows)
    }

    /// A ranker's draws: each turn of its loop reads every entry twice (the
    /// least priced and the least bound), and a draw turns once a walk it
    /// prices and once more to answer.
    fn ranker_draws(entries: u64, walks: u64, draws: u64) -> u64 {
        2 * entries * (walks + draws)
    }

    /// A `toward`'s site listing for `seat`'s commander at `commander`: a
    /// beacon row read, each own living beacon's radius, and each disc
    /// column with the tests it reaches, in the listing's order.
    fn legal_sites(&self, seat: SeatId, commander: [Fx; 3]) -> u64 {
        let beacons = self.world.beacons();
        let surface = self.world.surface();
        let [cx, cy] = column_of(commander);
        let start = surface.node_of(cx, cy).unwrap();
        let mut rows = 0;
        for row in 0..beacons.seats().len() {
            rows += 1;
            if beacons.seats()[row] != seat.raw() || !beacons.hit_points()[row].is_alive() {
                continue;
            }
            let (radius, read) = self.radius_at_row(row);
            rows += read;
            let radius = i32::from(radius);
            let [bx, by] = column_of(beacons.positions()[row]);
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    if dx * dx + dy * dy > radius * radius {
                        continue;
                    }
                    rows += 1;
                    let (x, y) = (bx + dx, by + dy);
                    let Some(node) = surface.node_of(x, y) else {
                        continue;
                    };
                    let Some(site) = self.world.ground().standing(x, y) else {
                        continue;
                    };
                    if !surface.walkable(node) {
                        continue;
                    }
                    let (covered, read) = self.at_column(x, y);
                    rows += read;
                    if covered {
                        continue;
                    }
                    let (inside, read) = self.inside_own_sphere(seat, site);
                    rows += read;
                    if !inside {
                        continue;
                    }
                    let (stacks, read) = self.stacks_on_own(seat, x, y);
                    rows += read;
                    let _ = (stacks, start);
                }
            }
        }
        rows
    }

    /// A `covering` site search for `feature`: its `on` column and anchor,
    /// then each spiral column with the tests it reaches, ring by ring until
    /// a ring holds a site. Answers the site and the rows.
    fn covering_site(
        &self,
        seat: SeatId,
        commander: [Fx; 3],
        feature: usize,
    ) -> (Option<[i32; 2]>, u64) {
        let ground = self.world.ground();
        let surface = self.world.surface();
        let [ax, ay] = self.world.features().features()[feature].anchor;
        let (on, mut rows) = self.on_column(feature, seat, None);
        let [ox, oy] = on.unwrap();
        let on_point = ground.standing(ox, oy).unwrap();
        let (anchor_point, read) = self.anchor_point(feature);
        rows += read;
        let anchor_point = anchor_point.unwrap();
        let [cx, cy] = column_of(commander);
        let start = surface.node_of(cx, cy).unwrap();
        let key = |[dx, dy]: [i32; 2]| i64::from(dx).pow(2) + i64::from(dy).pow(2);
        for ring in ground.spiral.chunk_by(|a, b| key(*a) == key(*b)) {
            let mut best: Option<(i64, i32, i32)> = None;
            for &[dx, dy] in ring {
                rows += 1;
                let (x, y) = (ax + dx, ay + dy);
                let Some(node) = surface.node_of(x, y) else {
                    continue;
                };
                let Some(site) = ground.standing(x, y) else {
                    continue;
                };
                if !surface.walkable(node) {
                    continue;
                }
                let (covered, read) = self.at_column(x, y);
                rows += read;
                if covered
                    || !ground.in_sphere(site, on_point)
                    || !ground.in_sphere(site, anchor_point)
                {
                    continue;
                }
                let (inside, read) = self.inside_own_sphere(seat, site);
                rows += read;
                if !inside {
                    continue;
                }
                let (stacks, read) = self.stacks_on_own(seat, x, y);
                rows += read;
                if stacks || !self.world.clusters().connected(surface, start, node) {
                    continue;
                }
                let held = (i64::from(x - cx).pow(2) + i64::from(y - cy).pow(2), y, x);
                if best.is_none_or(|best| held < best) {
                    best = Some(held);
                }
            }
            if let Some((_, y, x)) = best {
                return (Some([x, y]), rows);
            }
        }
        (None, rows)
    }
}

/// The `h` the directed search computed in `scratch`'s last grid optimum,
/// each a fold over every sphere the listing added: one unit a sphere a
/// fold reads.
fn heuristic_reads(scratch: &Scratch) -> u64 {
    scratch.reach().evaluations().unwrap() * u64::try_from(scratch.reach().spheres().len()).unwrap()
}

/// What the directed search's bucket queue visited in `scratch`'s last grid
/// optimum, read from the queue's own counters (whose honesty
/// `pathing::reach`'s own tests hold to the search's keys and pops): a
/// bucket its ring walk turned to, a column moved from a bucket into its
/// batch, and a cell its start's reset wrote (dunit part 2), and a neighbour
/// slot a settled column tried (S3's `walk3`).
fn queue_reads(scratch: &Scratch) -> u64 {
    let reach = scratch.reach();
    reach.turns().unwrap()
        + reach.unlinked().unwrap()
        + reach.filled().unwrap()
        + reach.tried().unwrap()
}

/// The columns a `toward`'s site listing scans for `seat`: the disc of each
/// of its living beacons' spheres, at each one's own radius, overlaps
/// included. Counted from the spiral's offsets, not from the listing.
fn listed_columns(world: &World, seat: SeatId) -> u64 {
    let ground = world.ground();
    world
        .beacons()
        .seats()
        .iter()
        .zip(world.beacons().hit_points())
        .zip(world.beacons().ids())
        .filter(|((owner, hp), _)| **owner == seat.raw() && hp.is_alive())
        .map(|(_, id)| {
            let radius = ground.radius_of(Some(BeaconId::new(*id))).floor_voxels();
            u64::try_from(pharmakos_sim::targeting::spiral_offsets(radius).len()).unwrap()
        })
        .sum()
}

/// Seat 0's commander on the probation seed: its point and its column.
fn commander_start(world: &World) -> ([pharmakos_sim::math::fixed::Fx; 3], [i32; 2]) {
    let seat = SeatId::new(0);
    let commander = world.commander_of(seat).expect("a commander");
    let row = world.units().row_of(commander).expect("a live commander");
    let start = world.units().positions()[row.index()];
    (start, column_of(start))
}

/// The features reachable from `origin`, nearest by octile bound first, ties
/// by index.
fn by_octile(world: &World, origin: [i32; 2]) -> Vec<usize> {
    let from = world.surface().node_of(origin[0], origin[1]).unwrap();
    let mut reachable: Vec<(i64, usize)> = (0..world.features().len())
        .filter_map(|index| {
            let goal = measured_to(world, index, from)?;
            Some((i64::from(world.surface().heuristic(from, goal)), index))
        })
        .collect();
    reachable.sort_unstable();
    reachable.into_iter().map(|(_, index)| index).collect()
}

/// A ranker over the one feature `index`, from `origin`, priced once: the
/// units it charged and the nodes its scratch popped, on a fresh tally and a
/// fresh scratch.
fn one_walk(world: &World, origin: [i32; 2], index: usize) -> (u64, u64) {
    let tally = pharmakos_sim::seams::UnitTally::new();
    let ground = pharmakos_sim::targeting::Ground {
        work: &tally,
        ..world.ground()
    };
    let mut scratch = Scratch::for_map(world.surface(), world.clusters().size()).unwrap();
    let mut ranker = Ranker::new(&ground, origin, |at| at == index);
    let emitted = ranker
        .next(&ground, &mut scratch)
        .expect("the scratch serves")
        .map(|(at, _)| at);
    assert_eq!(emitted, Some(index), "the one candidate is emitted");
    assert_eq!(scratch.walks().unwrap(), 1, "and priced by one walk");
    (tally.read().expect("measured"), popped(&scratch))
}

/// A ranker pricing a served walk charges the nodes the walk popped, and a
/// far walk, which pops more, charges more than a near one on the same world.
/// Before lane `dunit` a served walk charged one unit whatever it popped.
#[test]
fn a_ranked_walk_charges_the_nodes_it_pops() {
    let world = world_of(PROBATION, 2);
    let (_, origin) = commander_start(&world);
    let order = by_octile(&world, origin);
    let near = *order.first().expect("a reachable feature");
    let far = *order.last().expect("a reachable feature");
    let model = Steps { world: &world };
    // The ranker's scan, one a feature, the measure of its one candidate, its
    // two draws (one to price the walk, one to answer) of one entry read
    // twice each, and the walk's pops.
    let scan = |index: usize| {
        let (entries, rows) = model.ranker_new(origin, |at| (at == index, 0));
        rows + Steps::ranker_draws(entries, 1, 1)
    };
    let (near_units, near_pops) = one_walk(&world, origin, near);
    let (far_units, far_pops) = one_walk(&world, origin, far);
    println!("near: {near_units} units, {near_pops} pops; far: {far_units} units, {far_pops} pops");
    assert!(near_pops > 0, "a served walk pops nodes");
    assert_eq!(
        near_units,
        scan(near) + near_pops,
        "the ranker's scan, its draws and the walk's pops"
    );
    assert_eq!(far_units, scan(far) + far_pops);
    assert!(far_pops > near_pops, "the far walk pops more");
    assert!(far_units > near_units, "so it charges more");
}

/// `toward` on the probation seed's pinned vent charges its grid optimum's
/// pops: the units less the columns the site listing scanned are exactly the
/// nodes the search popped. Before lane `dunit` the pops were charged
/// nothing.
#[test]
fn a_toward_charges_its_grid_optimum_pops() {
    let world = world_of(PROBATION, 2);
    let seat = SeatId::new(0);
    let (start, _) = commander_start(&world);
    let tally = pharmakos_sim::seams::UnitTally::new();
    let ground = pharmakos_sim::targeting::Ground {
        work: &tally,
        ..world.ground()
    };
    let mut scratch = Scratch::for_map(world.surface(), world.clusters().size()).unwrap();
    let mut stones: Vec<StoneCandidate> = Vec::new();
    toward_counted(
        &ground,
        &mut scratch,
        &mut stones,
        seat,
        start,
        FeatureSpec::Name {
            kind: FeatureKind::Vent,
            anchor: PROBATION_VENT,
        },
    )
    .expect("a stone toward the pinned vent");
    let pops = popped(&scratch);
    let units = tally.read().expect("measured");
    println!("the pinned vent: {units} units, {pops} pops");
    assert!(pops > 0, "the grid optimum pops nodes");
    assert_eq!(
        units - toward_named_steps(&world, seat, start, &scratch),
        pops
    );
}

/// What a `toward` the probation seed's pinned vent, by name, visits besides
/// the nodes its grid optimum pops, counted by [`Steps`]: the name's lookup,
/// the vent's liveness, the site listing, the measure to the vent and the
/// directed search's `h` folds, read from `scratch` once the resolve ran.
fn toward_named_steps(
    world: &World,
    seat: SeatId,
    start: [pharmakos_sim::math::fixed::Fx; 3],
    scratch: &Scratch,
) -> u64 {
    let model = Steps { world };
    let [cx, cy] = column_of(start);
    let from = world.surface().node_of(cx, cy).unwrap();
    let (index, named) = model.index_of(FeatureKind::Vent, PROBATION_VENT);
    let index = index.unwrap();
    let (_, live) = model.is_live(index);
    let (_, measure) = model.measure_to(index, from);
    named
        + live
        + model.legal_sites(seat, start)
        + measure
        + heuristic_reads(scratch)
        + queue_reads(scratch)
}

/// One resolve of [`decision_units_equal_the_primitive_steps_counted`]'s
/// table.
#[derive(Clone, Copy, Debug)]
enum Resolve {
    /// A ranker over one feature, priced by one served walk.
    Walk(usize),
    /// `toward` the probation seed's pinned vent, by name.
    TowardName,
    /// `toward` the nearest uncovered vent.
    TowardNearest,
    /// `covering` the vent anchored here, by name.
    CoverName([i32; 2]),
    /// `covering` the nearest uncovered vent, which is `first_vent`.
    CoverNearest { first_vent: usize },
}

/// Run `resolve` for seat 0 on a fresh tally and a fresh scratch: the units
/// it charged, the steps that are not pops counted without the resolver (one
/// a feature the ranker scans, one a column a site search scans), and the
/// nodes its scratch popped.
fn resolve_once(world: &World, label: &str, resolve: Resolve) -> (u64, u64, u64) {
    let seat = SeatId::new(0);
    let (start, origin) = commander_start(world);
    let tally = pharmakos_sim::seams::UnitTally::new();
    let ground = pharmakos_sim::targeting::Ground {
        work: &tally,
        ..world.ground()
    };
    let mut scratch = Scratch::for_map(world.surface(), world.clusters().size()).unwrap();
    let mut stones: Vec<StoneCandidate> = Vec::new();
    let vents = FeatureSpec::Nearest {
        kind: FeatureKind::Vent,
        uncovered: true,
    };
    let named = |anchor: [i32; 2]| FeatureSpec::Name {
        kind: FeatureKind::Vent,
        anchor,
    };
    let model = Steps { world };
    let start_node = world.surface().node_of(origin[0], origin[1]).unwrap();
    let uncovered_vent = |index: usize| model.matches_pick(seat, index, FeatureKind::Vent, true);
    let steps = match resolve {
        Resolve::Walk(index) => {
            let mut ranker = Ranker::new(&ground, origin, |at| at == index);
            assert!(
                ranker
                    .next(&ground, &mut scratch)
                    .expect("the scratch serves")
                    .is_some(),
                "{label}"
            );
            let (entries, rows) = model.ranker_new(origin, |at| (at == index, 0));
            rows + Steps::ranker_draws(entries, scratch.walks().unwrap(), 1)
        }
        Resolve::TowardName => {
            let stone = toward_counted(
                &ground,
                &mut scratch,
                &mut stones,
                seat,
                start,
                named(PROBATION_VENT),
            )
            .expect(label);
            let (index, _) = model.index_of(FeatureKind::Vent, PROBATION_VENT);
            assert_eq!(Some(stone.feature), index, "{label}");
            toward_named_steps(world, seat, start, &scratch)
        }
        Resolve::TowardNearest => {
            let stone = toward_counted(&ground, &mut scratch, &mut stones, seat, start, vents)
                .expect(label);
            let (entries, ranked) = model.ranker_new(origin, uncovered_vent);
            let (_, measure) = model.measure_to(stone.feature, start_node);
            ranked
                + model.legal_sites(seat, start)
                + Steps::ranker_draws(entries, scratch.walks().unwrap(), 1)
                + measure
                + heuristic_reads(&scratch)
                + queue_reads(&scratch)
        }
        Resolve::CoverName(anchor) => {
            let cover = pharmakos_sim::targeting::cover_counted(
                &ground,
                &mut scratch,
                seat,
                start,
                named(anchor),
            )
            .expect(label);
            let (index, named_rows) = model.index_of(FeatureKind::Vent, anchor);
            let index = index.unwrap();
            let (_, live) = model.is_live(index);
            let (site, searched) = model.covering_site(seat, start, index);
            assert_eq!(
                site,
                Some(cover.site),
                "{label}: the model searches as the sim does"
            );
            named_rows + live + searched
        }
        Resolve::CoverNearest { first_vent } => {
            let cover =
                pharmakos_sim::targeting::cover_counted(&ground, &mut scratch, seat, start, vents)
                    .expect(label);
            assert_eq!(
                cover.feature, first_vent,
                "{label}: the ranker's first vent has a site, so one spiral is searched"
            );
            let (entries, ranked) = model.ranker_new(origin, uncovered_vent);
            let (site, searched) = model.covering_site(seat, start, first_vent);
            assert_eq!(
                site,
                Some(cover.site),
                "{label}: the model searches as the sim does"
            );
            ranked + Steps::ranker_draws(entries, scratch.walks().unwrap(), 1) + searched
        }
    };
    (tally.read().expect("measured"), steps, popped(&scratch))
}

/// **Decision units follow counted work**: over a table of resolves, each on
/// a fresh tally and a fresh scratch, the units charged equal the primitive
/// steps the structures' own counters and the test's own count ([`Steps`])
/// make -- one a visit of each loop the resolve runs, at every level down to
/// the innermost (a feature row, a footprint column, a beacon, structure or
/// target row, a Spire test's row, a ranker entry, a site column, an `h`
/// fold's sphere), and one a node a search pops -- exactly. A loop that
/// visits without charging, or a charge with no visit under it, fails here.
#[test]
fn decision_units_equal_the_primitive_steps_counted() {
    let world = world_of(PROBATION, 2);
    let seat = SeatId::new(0);
    let (_, origin) = commander_start(&world);
    let order = by_octile(&world, origin);
    // The vent a `covering` nearest-uncovered picks, which the named
    // `covering` then names.
    let first_vent = {
        let tally = pharmakos_sim::seams::UnitTally::new();
        let ground = pharmakos_sim::targeting::Ground {
            work: &tally,
            ..world.ground()
        };
        let mut scratch = Scratch::for_map(world.surface(), world.clusters().size()).unwrap();
        let mut ranker = Ranker::new(&ground, origin, |index| {
            pharmakos_sim::targeting::matches_pick(&ground, seat, index, FeatureKind::Vent, true)
        });
        ranker
            .next(&ground, &mut scratch)
            .expect("the scratch serves")
            .expect("a vent")
            .0
    };
    let first_anchor = world.features().features()[first_vent].anchor;
    let cases = [
        ("near walk", Resolve::Walk(order[0])),
        ("far walk", Resolve::Walk(*order.last().unwrap())),
        ("toward, named", Resolve::TowardName),
        ("toward, nearest", Resolve::TowardNearest),
        ("covering, named", Resolve::CoverName(first_anchor)),
        ("covering, nearest", Resolve::CoverNearest { first_vent }),
    ];
    let mut measured = Vec::new();
    for (label, resolve) in cases {
        let (units, steps, pops) = resolve_once(&world, label, resolve);
        println!("{label}: {units} units = {steps} steps + {pops} pops");
        measured.push((label, units, steps + pops));
    }
    for (label, units, counted) in &measured {
        assert_eq!(units, counted, "{label}: units are the steps counted");
    }
}

/// A measurement is not state: the same `toward` and `covering` match, played
/// twice, the second time with the decisions' tally armed to fault on its
/// next charge before every tick (`World::fault_next_decision_charge`, which
/// `phase_decision` passes to the tally it makes, and which survives each
/// seat's reset), so every decision that charges
/// anything ends unmeasured. Every tick hashes the same in both, and each
/// decision the clean run measured is unmeasured in the faulted one: the
/// fault reached inside the decision and changed nothing. The cross-build
/// half -- that pricing the pops moved no chain -- is the committed scenario
/// chains `toward-stepping-stone` and `cover-nearest-vent`, which
/// `cargo xtask ci`'s `scenario` step plays.
#[test]
fn decision_units_never_reach_the_state_hash() {
    use pharmakos_sim::seams::{NoFigure, Unmeasured};
    let route = concat!(
        r#"{"label":"stone","place_beacon":{"at":{"toward":{"feature":{"feature_id":"vent_232_96"}}}},"timeout_ms":120000},"#,
        r#"{"label":"cover","place_beacon":{"at":{"covering":{"vent":{"rank":"NEAREST","coverage":"UNCOVERED"}}}},"timeout_ms":120000}"#
    );
    let play = |fault: bool| {
        let mut runner = Runner::new(world_of(PROBATION, 2)).unwrap();
        runner
            .seal_playbook(SeatId::new(0), compile(route).expect("the route compiles"))
            .expect("a Lull takes a seal");
        assert!(runner.begin_push().unwrap());
        let mut chain = Vec::new();
        for _ in 0..60 {
            if fault {
                runner.world().fault_next_decision_charge();
            }
            runner
                .step()
                .unwrap()
                .expect("the segment outlasts this test");
            chain.push((
                runner.world().state_hash(),
                runner.world().decision_work(0).expect("seat 0"),
            ));
        }
        chain
    };
    let clean = play(false);
    let faulted = play(true);
    assert!(
        clean
            .iter()
            .any(|(_, units)| matches!(units, Ok(units) if *units > 0)),
        "the match charges decision units"
    );
    let hashes = |chain: &[(u64, Result<u64, NoFigure>)]| -> Vec<u64> {
        chain.iter().map(|(hash, _)| *hash).collect()
    };
    assert_eq!(
        hashes(&faulted),
        hashes(&clean),
        "a faulted tally moves no hash"
    );
    for ((_, clean), (_, faulted)) in clean.iter().zip(&faulted) {
        if matches!(clean, Ok(units) if *units > 0) {
            assert_eq!(
                *faulted,
                Err(NoFigure::Unmeasured(Unmeasured::Overflowed)),
                "the fault fired inside the decision"
            );
        }
    }
}
