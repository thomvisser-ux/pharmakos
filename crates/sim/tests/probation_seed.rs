// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Probation's own map seed (S3's plan, decision 13; decisions-log item 154
//! (1)): the deterministic search, its criterion, and the answer it pins.
//!
//! Spec §8's licence asks for Generators on two vents, and on every seed the
//! plan's section 1.1 read the next vent lay beyond a three-round match. So the
//! Probation preset plays a seed of its own, the first of a search for a
//! two-seat map whose second vent lies within three stepping stones of seat
//! 0's core; `gwp` writes it into the lobby's line, and the golden seed stays
//! every other test's fixture.
//!
//! # The criterion
//!
//! The seats are two, and the human is seat 0. A seed passes when all of this
//! holds, each read from the real generator and the real resolver:
//!
//! 1. **The second vent** is the vent nearest seat 0's core, by squared column
//!    distance with ties to the lowest `(y, x)`, that is not the vent the
//!    generator gave its zone. It lies nearer seat 0's core than seat 1's, so
//!    the map is not picked for a vent that belongs to the enemy.
//! 2. **It is not given.** With nothing placed, no site covers it, so a seat
//!    that never expands never earns the licence (decision 13's "earnable by a
//!    seat that expands, not given").
//! 3. **At most [`MOST_PLACEMENTS`] beacons reach it.** Each stone is the site
//!    `toward` will choose (S3's plan, E23): the legal site inside the seat's
//!    spheres nearest the vent by the item-61 estimator, ties to the lowest
//!    `(y, x)`. The placement that covers the vent is the sim's own
//!    [`covering_site`], and it counts: "three stepping stones" is three
//!    beacons, the third of them covering the vent. A beacon stands inside
//!    another's 24-voxel sphere and covers 24 more, so three reach 96 voxels at
//!    best, on flat ground.
//! 4. **The walk fits round 1.** The commander's estimated walk from its start
//!    through every placement is at most the ladder's first Push, so the chain
//!    asks an expanding seat for no more time than it has.
//!
//! The answer is the first seed, counting up from zero, that passes.
//!
//! # Re-running it
//!
//! A pin below fails if the generator, the rules' map rows or the resolver move
//! under it. The answer is then stale: `every_passing_seed_is_listed` (run by
//! hand with `--ignored --nocapture`) prints the seeds that pass now, and the
//! change that moved it re-pins here and in the lobby's line, naming the cause.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules — not an integration test's helper functions. A panic is this file's failure report."
)]

use pharmakos_sim::features::FeatureKind;
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::math::quantity::Ms;
use pharmakos_sim::pathing::estimate::{Fog, Speed, estimate};
use pharmakos_sim::pathing::search::Scratch;
use pharmakos_sim::pathing::surface::Node;
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::tables::{PRIORITY_NORMAL, SeatId};
use pharmakos_sim::targeting::{
    Ground, anchor_in_sphere, column_of, covering_site, spiral_offsets,
};
use pharmakos_sim::{MatchSettings, RulesTable, World, WorldConfig, default_rules_path, mapgen};
use std::path::PathBuf;

/// The answer: the first seed that passes. The lobby's line spells it
/// `0x0000000000000014`, as `host_link.gd` spells a seed.
const PROBATION_SEED: u64 = 20;

/// What it finds: its second vent, the columns of the three placements that
/// reach it, and the commander's estimated walk through them, 81 s of a 180 s
/// Push.
const PINNED_VENT: [i32; 2] = [232, 96];
const PINNED_SITES: [[i32; 2]; 3] = [[192, 51], [203, 72], [222, 86]];
const PINNED_WALK_TICKS: i32 = 1_620;

/// The golden seed, every other test's fixture: its second vent is about nine
/// stones out (S3's plan, section 1.1), which is why Probation has a seed of
/// its own.
const GOLDEN_SEED: u64 = 0x0000_0000_CA5C_ADED;

/// The criterion's bound: three stepping stones (decision 13).
const MOST_PLACEMENTS: usize = 3;

/// How far a hand-run survey looks.
const SURVEY_SEEDS: u64 = 512;

/// Why a seed fails the criterion.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Miss {
    /// The map has no vent but the start vent.
    NoSecondVent,
    /// The second vent is nearer seat 1's core.
    NearerTheEnemy,
    /// Past what three placements can cover, by column distance alone.
    TooFar,
    /// A site covers the vent with nothing placed.
    Given,
    /// Three placements do not cover it.
    OutOfReach,
    /// No legal site is left to step to.
    NoStone,
    /// The walk is longer than round 1's Push.
    LongWalk,
}

/// What a passing seed found.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Chain {
    /// The second vent's anchor.
    vent: [i32; 2],
    /// The placements' columns, in order; the last covers the vent.
    sites: Vec<[i32; 2]>,
    /// The commander's estimated walk through them, in ticks.
    walk_ticks: i32,
}

fn rules() -> RulesTable {
    let path = default_rules_path().unwrap_or_else(|| PathBuf::from("rules/rules.v1.json"));
    RulesTable::load(&path).unwrap_or_else(|error| panic!("loading the rules table: {error}"))
}

fn squared(from: [i32; 2], to: [i32; 2]) -> i64 {
    let dx = i64::from(from[0]) - i64::from(to[0]);
    let dy = i64::from(from[1]) - i64::from(to[1]);
    dx * dx + dy * dy
}

/// The second vent's anchor, by the criterion's first clause, from the map the
/// generator made. Cheap beside the resolver, so it rules out most seeds.
fn second_vent(seed: u64, rules: &RulesTable) -> Result<[i32; 2], Miss> {
    let map = mapgen::generate(seed, rules, 2).expect("the committed rules table makes a map");
    let zone = |seat: u8| {
        map.report
            .zones
            .iter()
            .find(|zone| zone.seat == Some(seat))
            .expect("two seats, two occupied zones")
    };
    let column = |centre: [i32; 3]| [centre[0], centre[1]];
    let (own, enemy) = (column(zone(0).centre), column(zone(1).centre));
    let start = zone(0)
        .vent
        .as_ref()
        .map(|vent| [vent.at[0], vent.at[1]])
        .expect("an occupied zone has its vent");
    let vent = map
        .features
        .features()
        .iter()
        .filter(|feature| feature.kind == FeatureKind::Vent && feature.anchor != start)
        .map(|feature| feature.anchor)
        // item 62: the key ends in `(y, x)`, unique per anchor, so it is total.
        .min_by_key(|anchor| (squared(*anchor, own), anchor[1], anchor[0]))
        .ok_or(Miss::NoSecondVent)?;
    if squared(vent, enemy) <= squared(vent, own) {
        return Err(Miss::NearerTheEnemy);
    }
    let sphere = i64::from(rules.sphere_radius_voxels());
    let placements = i64::try_from(MOST_PLACEMENTS).expect("three");
    let reach = sphere * (placements + 1);
    if squared(vent, own) > reach * reach {
        return Err(Miss::TooFar);
    }
    Ok(vent)
}

/// The ladder's first Push, in game milliseconds, read from the rules.
fn first_push_ms(rules: &RulesTable) -> i32 {
    *rules.segment_lengths_ms().first().expect("a ladder")
}

/// The world of a seed's two-seat match.
fn world_of(seed: u64, rules: &RulesTable) -> World {
    World::new(&WorldConfig {
        match_seed: seed,
        seats: 2,
        rules: rules.clone(),
        match_settings: MatchSettings {
            segment_lengths_ms: rules.segment_lengths_ms().to_vec(),
            round_limit: 4,
        },
    })
    .expect("the committed rules table describes a world")
}

/// The estimated cost and ticks of a commander's walk between two nodes.
fn walk(ground: &Ground<'_>, scratch: &mut Scratch, from: Node, to: Node) -> Option<(i64, i32)> {
    estimate(
        ground.surface,
        ground.clusters,
        scratch,
        from,
        to,
        Fog::Clear,
        Speed::commander(ground.rules),
    )
    .map(|found| (i64::from(found.cost), found.ticks))
}

/// The stone E23's `toward` picks: the legal site inside the seat's spheres
/// with the least estimated travel to the vent's anchor, ties to the lowest
/// `(y, x)`. The candidates are walked in the order of the estimator's own
/// lower bound, so the answer is exact and most of them are never estimated.
fn stone(
    ground: &Ground<'_>,
    scratch: &mut Scratch,
    seat: SeatId,
    origin: [Fx; 3],
    placed: &[[i32; 2]],
    anchor: Node,
) -> Option<[i32; 2]> {
    let [ox, oy] = column_of(origin);
    let start = ground.surface.node_of(ox, oy)?;
    let mut candidates: Vec<(i32, i32, i32, Node)> = Vec::new();
    let offsets = spiral_offsets(ground.sphere_radius());
    for [bx, by] in placed {
        for [dx, dy] in &offsets {
            let (x, y) = (bx + dx, by + dy);
            let Some(node) = ground.surface.node_of(x, y) else {
                continue;
            };
            let Some(site) = ground.standing(x, y) else {
                continue;
            };
            if ground.surface.walkable(node)
                && ground.features.at_column(x, y).is_none()
                && ground.inside_own_sphere(seat, site)
                && !ground.stacks_on_own(seat, x, y)
                && ground.clusters.connected(ground.surface, start, node)
            {
                candidates.push((ground.surface.heuristic(node, anchor), y, x, node));
            }
        }
    }
    // item 62: the key ends in `(y, x)`, and a column seen from two beacons is
    // the same entry, so once it is deduplicated the order is total.
    candidates.sort_unstable_by_key(|(bound, y, x, _)| (*bound, *y, *x));
    candidates.dedup_by_key(|(_, y, x, _)| (*y, *x));
    let mut best: Option<(i64, i32, i32)> = None;
    for (bound, y, x, node) in candidates {
        if best.is_some_and(|(cost, _, _)| i64::from(bound) > cost) {
            break;
        }
        let Some((cost, _)) = walk(ground, scratch, node, anchor) else {
            continue;
        };
        if best.is_none_or(|held| (cost, y, x) < held) {
            best = Some((cost, y, x));
        }
    }
    best.map(|(_, y, x)| [x, y])
}

/// The criterion, for one seed.
fn probe(seed: u64, rules: &RulesTable) -> Result<Chain, Miss> {
    let vent = second_vent(seed, rules)?;
    let mut world = world_of(seed, rules);
    let seat = SeatId::new(0);
    let feature = world
        .features()
        .index_of(FeatureKind::Vent, vent)
        .expect("the generator's vent");
    let commander = world.commander_of(seat).expect("a commander");
    let row = world.units().row_of(commander).expect("a live commander");
    let start = world.units().positions()[row.index()];
    // Nothing is placed yet, so the seat's one beacon is its core.
    let core_row = world
        .beacons()
        .seats()
        .iter()
        .position(|owner| *owner == seat.raw())
        .expect("a core");
    let core_column = column_of(world.beacons().positions()[core_row]);
    let mut scratch = {
        let ground = world.ground();
        Scratch::for_map(ground.surface, ground.rules.hpa_cluster_voxels())
            .expect("the map takes a search")
    };
    if covering_site(&world.ground(), seat, start, feature).is_some() {
        return Err(Miss::Given);
    }
    let mut placed: Vec<[i32; 2]> = vec![core_column];
    let mut sites: Vec<[i32; 2]> = Vec::new();
    let mut walk_ticks = 0_i32;
    let mut at = start;
    for placement in 1..=MOST_PLACEMENTS {
        let ground = world.ground();
        let anchor = ground
            .surface
            .node_of(vent[0], vent[1])
            .expect("a vent's anchor is on the map");
        let covers = covering_site(&ground, seat, at, feature);
        let site = match covers {
            Some(site) => site,
            None if placement == MOST_PLACEMENTS => return Err(Miss::OutOfReach),
            None => stone(&ground, &mut scratch, seat, at, &placed, anchor).ok_or(Miss::NoStone)?,
        };
        let [ax, ay] = column_of(at);
        let from = ground
            .surface
            .node_of(ax, ay)
            .expect("the commander's column");
        let to = ground
            .surface
            .node_of(site[0], site[1])
            .expect("a site's column");
        let (_, ticks) = walk(&ground, &mut scratch, from, to).ok_or(Miss::NoStone)?;
        let point = ground.standing(site[0], site[1]).expect("a site stands");
        walk_ticks = walk_ticks.checked_add(ticks).expect("a short walk");
        sites.push(site);
        placed.push(site);
        at = point;
        if covers.is_some() {
            break;
        }
        world
            .place_beacon_directly(seat, point, MandateKind::None, PRIORITY_NORMAL)
            .expect("room for a stone");
    }
    if i64::from(walk_ticks) > i64::from(Ms::new(first_push_ms(rules)).to_ticks_floor()) {
        return Err(Miss::LongWalk);
    }
    Ok(Chain {
        vent,
        sites,
        walk_ticks,
    })
}

/// The pin: the answer passes, with exactly the chain recorded in `sim3a`'s
/// pull request, so the generator or the resolver cannot drift under it.
#[test]
fn the_probation_seed_reaches_its_second_vent_in_three_placements() {
    assert_eq!(
        probe(PROBATION_SEED, &rules()),
        Ok(Chain {
            vent: PINNED_VENT,
            sites: PINNED_SITES.to_vec(),
            walk_ticks: PINNED_WALK_TICKS,
        })
    );
}

/// The search: no seed below the answer passes, so the answer is the search's
/// and nobody's pick. Each of them generates a map, so this is the file's slow
/// test, at about a second and a half a seed in a debug build.
#[test]
fn no_earlier_seed_passes() {
    let rules = rules();
    for seed in 0..PROBATION_SEED {
        let found = probe(seed, &rules);
        assert!(found.is_err(), "seed {seed} passes first: {found:?}");
    }
}

/// The criterion tells maps apart: the golden seed, the fixture the plan's
/// finding read, is too far.
#[test]
fn the_golden_seed_does_not_pass() {
    assert_eq!(probe(GOLDEN_SEED, &rules()), Err(Miss::TooFar));
}

/// The third placement is the sim's own `covering`, and the vent's anchor is
/// inside the sphere of the beacon it names: the licence's second vent is
/// covered by the rule a playbook's `covered {}` will read.
#[test]
fn the_last_placement_covers_the_vent_by_the_sims_own_test() {
    let rules = rules();
    let mut world = world_of(PROBATION_SEED, &rules);
    let seat = SeatId::new(0);
    let feature = world
        .features()
        .index_of(FeatureKind::Vent, PINNED_VENT)
        .expect("the pinned vent");
    for [x, y] in PINNED_SITES {
        let point = world.ground().standing(x, y).expect("a pinned site stands");
        world
            .place_beacon_directly(seat, point, MandateKind::None, PRIORITY_NORMAL)
            .expect("room for a stone");
    }
    let [x, y] = PINNED_SITES[2];
    let last = world.ground().standing(x, y).expect("the third site");
    assert!(anchor_in_sphere(&world.ground(), last, feature));
}

#[test]
#[ignore = "run by hand when a change moves the pinned answer"]
fn every_passing_seed_is_listed() {
    let rules = rules();
    for seed in 0..SURVEY_SEEDS {
        match probe(seed, &rules) {
            Ok(chain) => println!("{seed} passes: {chain:?}"),
            Err(Miss::TooFar) => {}
            Err(miss) => println!("{seed} misses: {miss:?}"),
        }
    }
}
