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
use pharmakos_sim::math::fixed::{Fx, Sq};
use pharmakos_sim::pathing::estimate::{MAX_PARTIALS, Speed, ticks_for_cost, walk_cost};
use pharmakos_sim::pathing::reach::Reach;
use pharmakos_sim::pathing::route::{Surcharge, Zone, route_with};
use pharmakos_sim::pathing::search::{Scratch, Tally};
use pharmakos_sim::pathing::surface::Node;
use pharmakos_sim::runner::{MatchSettings, Runner};
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::tables::{Common, PRIORITY_NORMAL, SeatId, UnitKind};
use pharmakos_sim::targeting::{FeatureSpec, Ground, Ranker, StoneCandidate, toward_counted};
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
/// router serves no route. A chain that does not end within `MAX_PARTIALS`
/// asks fails the test.
fn walked(
    world: &World,
    scratch: &mut Scratch,
    (start, goal): (Node, Node),
    surcharge: Surcharge<'_>,
) -> Option<i64> {
    let mut buffer = Vec::with_capacity(4096);
    let mut from = start;
    let mut total = 0_i64;
    for _ in 0..=MAX_PARTIALS {
        let served = route_with(
            world.surface(),
            world.clusters(),
            scratch,
            (from, goal),
            surcharge,
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

/// Nodes popped since the counters were last reset.
fn popped(scratch: &Scratch) -> u64 {
    scratch.expansions().expect("the count fits a u64")
        + scratch.abstract_expansions().expect("the count fits a u64")
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
                let cost = walked(&world, &mut truth_scratch, (from, goal), Surcharge::NONE)
                    .unwrap_or_else(|| {
                        panic!(
                            "seed {seed:#x}: the oracle cannot price feature {index} from {ox},{oy}"
                        )
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
                let got = ranker.next(&ground, &mut scratch);
                if got != Some((expected.3, expected.0)) {
                    wrong.push((seed, scarred, [ox, oy], got, (expected.3, expected.0)));
                }
            }
            let extra = ranker.next(&ground, &mut scratch);
            if extra.is_some() {
                wrong.push((seed, scarred, [ox, oy], extra, (usize::MAX, 0)));
            }
        }
    }
    assert!(ranks >= 2_000, "the test ranks enough: {ranks}");
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

/// The grid optimum's bucket queue settles every column it reaches exactly
/// once, at the least cost the test's own Dijkstra finds, in strictly
/// ascending `(cost, column)`: the total order a binary heap over that key
/// gives, which `toward`'s tie-break rests on (`pathing::reach`, "The queue,
/// and its order"). Full floods from seeded columns of generated and scarred
/// maps.
#[test]
fn the_grid_optimum_settles_in_cost_then_column_order() {
    let mut floods = 0;
    for (seed, scarred) in [(SEED, false), (SEED, true), (PROBATION, true)] {
        let mut world = world_of(seed, 2);
        if scarred {
            scar(&mut world, seed);
        }
        let surface = world.surface();
        let mut reach = Reach::new(surface.node_count(), surface.costs().max_step());
        let mut rng = Rng(seed ^ 0x0077);
        for _ in 0..3 {
            let (_, from) = origin(&world, &mut rng);
            let least = least_costs(&world, from);
            let reachable = least.iter().filter(|cost| cost.is_some()).count();
            reach.begin(from);
            let mut tally = Tally::ZERO;
            let mut previous: Option<(i64, Node)> = None;
            let mut settled = 0_usize;
            while let Some((node, cost)) = reach.next(surface, &mut tally) {
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
/// `(y, x)`, by the test's own Dijkstra; and no walk is priced.
#[test]
fn a_toward_is_the_closest_site_over_the_terrain() {
    let (mut cases, mut sites_seen) = (0_u32, 0_u64);
    for (seed, scarred) in [(PROBATION, false), (PROBATION, true), (SEED, true)] {
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
            .take(4)
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
            for stone in &stones {
                sites_seen += 1;
                let [x, y] = stone.column();
                let cost = least[usize::try_from(stone.node()).unwrap()]
                    .expect("every legal site reaches the vent");
                if best.is_none_or(|held| (cost, y, x) < held) {
                    best = Some((cost, y, x));
                }
            }
            assert_eq!(
                found.ok().map(|stone| stone.site),
                best.map(|(_, y, x)| [x, y]),
                "seed {seed:#x}, vent {anchor:?}: toward is not the closest site over the terrain"
            );
        }
    }
    assert!(cases >= 8, "the test weighs enough vents: {cases}");
    assert!(sites_seen > 10_000, "and enough sites: {sites_seen}");
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
    for (label, spire, far, radius) in [
        ("open", false, false, 0),
        ("ring 10", false, false, 10),
        ("ring 18", false, false, 18),
        ("ring 26", false, false, 26),
        ("farthest", false, true, 0),
        ("pinned, Spire", true, false, 0),
        ("farthest, Spire, ring 26", true, true, 26),
    ] {
        let mut world = world_of(PROBATION, 2);
        let seat = SeatId::new(0);
        let core_row = world
            .beacons()
            .seats()
            .iter()
            .position(|owner| *owner == seat.raw())
            .expect("a core");
        let core = pharmakos_sim::targeting::column_of(world.beacons().positions()[core_row]);
        let vent = if far {
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
        } else {
            PROBATION_VENT
        };
        if spire {
            let id = pharmakos_sim::tables::BeaconId::new(world.beacons().ids()[core_row]);
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
        if radius > 0 {
            ring(
                &mut world,
                vent,
                radius,
                [vent[0] - core[0], vent[1] - core[1]],
            );
        }
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
        println!(
            "{label}: vent {vent:?}, stone {:?}, {} sites, {pops} nodes popped",
            found.site,
            stones.len()
        );
        measured.push((label, found.site, pops));
    }
    assert_eq!(measured, PINNED_RESOLVES.to_vec());
}

/// [`a_toward_resolve_pops_only_what_is_closer_than_the_stone`]'s pins,
/// measured at this lane's fix pass 2, where the stone came to be the closest
/// site over the terrain. While the stone was the least served walk, the same
/// search bounding the walks priced 82, 11, 45 and 9 walks on probe B's four
/// terrains and 814 (2.9 M nodes) on the farthest vent with a Spire and a
/// ring; with the octile bound before that, probe B priced 140, 1 261, 1 753 and
/// 1 753 walks and popped 0.3 M, 3.1 M, 4.5 M and 4.5 M nodes.
const PINNED_RESOLVES: [(&str, [i32; 2], u64); 7] = [
    ("open", [197, 50], 10_020),
    ("ring 10", [201, 48], 16_699),
    ("ring 18", [208, 41], 23_054),
    ("ring 26", [208, 41], 32_401),
    ("farthest", [179, 50], 106_531),
    ("pinned, Spire", [202, 61], 6_026),
    ("farthest, Spire, ring 26", [179, 62], 105_410),
];

/// "Nearest" under the walker's surcharge: with a threat's reach round the
/// way to the first vent, the order is the oracle's order of the routes the
/// walker is served under that surcharge, which is not the order without it.
#[test]
fn nearest_ranks_the_walk_under_the_walkers_surcharge() {
    let world = world_of(SEED, 2);
    let multiplier = world.rules().attack().step_multiplier;
    let mut scratch = Scratch::for_map(world.surface(), world.clusters().size()).unwrap();
    let mut truth_scratch = scratch.clone();
    let mut rng = Rng(0x5A5A_0001);
    let mut reordered = 0_u32;
    let mut origins = 0_u32;
    while origins < 40 {
        let ([ox, oy], from) = origin(&world, &mut rng);
        let plain = {
            let ground = world.ground();
            let mut ranker = Ranker::new(&ground, [ox, oy], |_| true);
            let first = ranker.next(&ground, &mut scratch);
            let second = ranker.next(&ground, &mut scratch);
            (first, second)
        };
        let (Some((first, _)), Some(_)) = plain else {
            continue;
        };
        origins += 1;
        // A threat on the first feature's anchor, reaching a third of the way
        // to the origin.
        let goal = measured_to(&world, first, from).expect("the first is reachable");
        let (gx, gy) = world.surface().coord_of(goal);
        let reach = ((gx - ox).abs().max((gy - oy).abs())).div_euclid(3).max(4);
        let zones = [Zone::Disc {
            centre: [
                Fx::from_voxels(i16::try_from(gx).unwrap()),
                Fx::from_voxels(i16::try_from(gy).unwrap()),
                Fx::from_raw(0),
            ],
            limit: Sq::of_radius(Fx::from_voxels(i16::try_from(reach).unwrap())),
        }];
        let surcharge = Surcharge::new(&zones, multiplier);
        let mut truth: Vec<(i64, i32, i32, usize)> = Vec::new();
        for (index, feature) in world.features().features().iter().enumerate() {
            let Some(goal) = measured_to(&world, index, from) else {
                continue;
            };
            let cost = walked(&world, &mut truth_scratch, (from, goal), surcharge)
                .expect("the oracle prices every reachable feature");
            truth.push((cost, feature.anchor[1], feature.anchor[0], index));
        }
        truth.sort_unstable();
        let ground = Ground {
            surcharge,
            ..world.ground()
        };
        let mut ranker = Ranker::new(&ground, [ox, oy], |_| true);
        for expected in &truth {
            assert_eq!(
                ranker.next(&ground, &mut scratch),
                Some((expected.3, expected.0)),
                "from {ox},{oy} under a threat at {gx},{gy}"
            );
        }
        if truth.first().map(|t| t.3) != Some(first) {
            reordered += 1;
        }
    }
    assert!(
        reordered > 0,
        "a threat never moved the first pick: the test shows nothing"
    );
}

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
        let Ok(cost) = walk_cost(
            world.surface(),
            world.clusters(),
            &mut scratch,
            (a, b),
            Surcharge::NONE,
        ) else {
            continue;
        };
        let promised =
            i64::from(ticks_for_cost(i32::try_from(cost).unwrap(), speed.cost_per_second).unwrap());
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

/// A whole playbook around `route`, compiled.
fn plan(route: &str) -> Plan {
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
    Plan::compile(&playbook, &rules()).expect("the playbook compiles")
}

/// The same with threats: seat 0's commander walks a `move` at
/// `AVOID_KNOWN_THREATS` past an armed raider of seat 1 it can see, standing
/// on the direct route, and the route the pathing phase serves it costs
/// exactly the walk `walk_cost` prices under the zones
/// `World::commander_threat_zones` gives (the function a commander's
/// "nearest" is ranked under, and the pathing phase's own). Ticks are not
/// compared here: a `move` completes within its arrive radius, short of the
/// route's end, and the walker's speed never reads a surcharge, so
/// `a_far_walk_costs_what_the_walker_walks` already ties cost to ticks.
#[test]
fn a_walk_round_a_threat_is_priced_as_served() {
    let mut exercised = 0;
    for (threat_out, goal) in [
        (12, [-60, 0]),
        (14, [0, 60]),
        (12, [-50, 40]),
        (10, [40, 40]),
    ] {
        if let Some((served, priced)) = served_round_a_threat(threat_out, goal) {
            assert_eq!(
                served, priced,
                "the served route's cost against the priced walk"
            );
            exercised += 1;
        }
    }
    assert!(exercised > 0, "no layout put the threat in the way");
}

/// Seat 0's commander sent to `goal` columns off its own at
/// `AVOID_KNOWN_THREATS` past seat 1's raider, standing on the direct route
/// `threat_out` columns out: the cost of the route the pathing phase served
/// it, and `walk_cost`'s under the seat's threat zones; `None` when the threat
/// is not in the way (the seat does not see it when the route is served, or
/// the surcharged route is the direct one).
fn served_round_a_threat(threat_out: i32, goal: [i32; 2]) -> Option<(i64, i64)> {
    let mut world = world_of(SEED, 2);
    let seat = SeatId::new(0);
    let commander = world.commander_of(seat).unwrap();
    let unit = world.units().row_of(commander).unwrap().raw();
    let row = world.units().row_of(commander).unwrap();
    let here = pharmakos_sim::targeting::column_of(world.units().positions()[row.index()]);
    let goal = [here[0] + goal[0], here[1] + goal[1]];
    let threat = {
        let surface = world.surface();
        let from = surface.node_of(here[0], here[1])?;
        let to = surface.node_of(goal[0], goal[1])?;
        let mut scratch = Scratch::for_map(surface, world.clusters().size()).unwrap();
        let mut direct = Vec::new();
        route_with(
            surface,
            world.clusters(),
            &mut scratch,
            (from, to),
            Surcharge::NONE,
            &mut direct,
        )
        .ok()?;
        let on = direct.iter().copied().find(|node| {
            let (x, y) = surface.coord_of(*node);
            (x - here[0]).abs().max((y - here[1]).abs()) >= threat_out
        })?;
        let (x, y) = surface.coord_of(on);
        world.standing_point([x, y, 0])
    };
    let enemy = SeatId::new(1);
    let home = world
        .place_beacon_directly(enemy, threat, MandateKind::Build, PRIORITY_NORMAL)
        .expect("room for the enemy's beacon");
    world.set_common(
        home,
        Common {
            roe: 1,
            retreat_hp_pct: 0,
        },
    );
    world
        .field_unit(enemy, home, UnitKind::Raider, threat)
        .expect("room for the raider");
    let z = world.voxels().top_solid_z(goal[0], goal[1]).unwrap() + 1;
    let route = format!(
        "{{\"label\":\"go\",\"move\":{{\"to\":{{\"voxel\":{{\"x\":{},\"y\":{},\"z\":{z}}}}},\"pace\":\"AVOID_KNOWN_THREATS\"}},\"timeout_ms\":120000}}",
        goal[0], goal[1]
    );
    let mut runner = Runner::new(world).unwrap();
    runner
        .seal_playbook(seat, plan(&route))
        .expect("a Lull takes a seal");
    assert!(runner.begin_push().unwrap());
    let multiplier = runner.world().rules().attack().step_multiplier;
    for _ in 0..20 {
        runner
            .step()
            .unwrap()
            .expect("the segment outlasts the walk");
        let world = runner.world();
        let served = world.router().route(unit);
        if served.len() < 2 {
            continue;
        }
        // The tick that served the route: price it as the pathing phase
        // served it, from where the commander stood.
        let mut zones = Vec::new();
        world.commander_threat_zones(0, &mut zones);
        if zones.is_empty() {
            return None;
        }
        let from = served[0];
        let to = *served.last().unwrap();
        let mut scratch = Scratch::for_map(world.surface(), world.clusters().size()).unwrap();
        let surcharge = Surcharge::new(&zones, multiplier);
        let mut straight = Vec::new();
        route_with(
            world.surface(),
            world.clusters(),
            &mut scratch,
            (from, to),
            Surcharge::NONE,
            &mut straight,
        )
        .unwrap();
        if straight == served {
            return None;
        }
        let priced = walk_cost(
            world.surface(),
            world.clusters(),
            &mut scratch,
            (from, to),
            surcharge,
        )
        .unwrap();
        let cost = pharmakos_sim::pathing::search::route_cost(world.surface(), served)
            .expect("a served route prices");
        return Some((i64::from(cost), priced));
    }
    panic!("the commander was never served a route")
}
