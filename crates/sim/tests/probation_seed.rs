// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Probation's own map seed (S3's plan, decision 13; decisions-log item 154
//! (1)): the criterion a seed must pass, the seed the preset plays, and the
//! seeds that pass.
//!
//! Spec §8's licence asks for Generators on two vents, and on every seed the
//! plan's section 1.1 read the next vent lay beyond a three-round match. So the
//! Probation preset plays a seed of its own, a two-seat map whose second vent
//! lies within three stepping stones of seat 0's core; `gwp` writes it into
//! the lobby's line, and the golden seed stays every other test's fixture.
//!
//! # The seed
//!
//! The owner's answer (decisions-log item 154 (1)) is the method: a map seed
//! of its own, found by `sim3a`'s deterministic search over this file's
//! criterion. The number 20 is that search's finding when it was run, as
//! decisions-log item 156 (1) records it, and the preset plays 20. Under the
//! stone's corrected measure (clause 3, lane `p3fix`) the criterion is also
//! met by 13, an earlier seed: the passing seeds from 0 through 20 are 13 and
//! 20 (`the_passing_seeds_through_twenty_are_thirteen_and_twenty`). The
//! owner answered whether the preset moves to 13: "Keep 20" (decisions-log
//! item 168 (4)). The preset plays 20; that 13 passes is recorded here and
//! built nowhere.
//!
//! # The criterion
//!
//! The seats are two, and the human is seat 0. A seed passes when all of this
//! holds, read from the real generator and the sim's own site rules: since
//! S3's `pred` the stepping stones of clause 3 are the sim's own `toward`
//! ([`toward_counted`]), which replaced this file's restatement of E23.
//!
//! 1. **The second vent** is the vent nearest seat 0's core, by squared column
//!    distance with ties to the lowest `(y, x)`, that is not the vent the
//!    generator gave its zone. It lies nearer seat 0's core than seat 1's, so
//!    the map is not picked for a vent that belongs to the enemy.
//! 2. **It is not given.** With nothing placed, no site covers it, so a seat
//!    that never expands never earns the licence (decision 13's "earnable by a
//!    seat that expands, not given").
//! 3. **At most [`MOST_PLACEMENTS`] beacons reach it.** Each stone is the sim's
//!    own `toward` (S3's plan, E23): the legal site inside the seat's spheres
//!    closest to the vent over the terrain, the least cost of any walk over
//!    the walkers' step rule (the grid optimum; `targeting`'s "Two
//!    measures": nobody walks from the stone to the vent when it is chosen),
//!    ties to the lowest `(y, x)`.
//!    Its terrain is the live world's, unfogged, until S3-05's mask at S4
//!    (decisions-log item 156 (3)). The placement that covers the vent is the sim's own
//!    [`covering_site`], and it counts: "three stepping stones" is three
//!    beacons, the third of them covering the vent. A beacon stands inside
//!    another's 24-voxel sphere and covers 24 more, so three reach 96 voxels at
//!    best, on flat ground.
//! 4. **The walk fits round 1.** The commander's walk from its start through
//!    every placement, each leg the walk the router serves (`walk_cost`, the
//!    measure clause 3 ranks by), is at most the ladder's first Push, so the
//!    chain asks an expanding seat for no more time than it has.
//!
//! # Re-running it
//!
//! A pin below fails if the generator, the rules' map rows or the resolver move
//! under it. `every_passing_seed_is_listed` (run by hand with `--ignored
//! --nocapture`) prints the seeds that pass now; the change that moved a pin
//! re-pins here, naming the cause, and a seed 20 that stops passing is the
//! owner's to rule on, since the lobby's line names it.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules — not an integration test's helper functions. A panic is this file's failure report."
)]

use pharmakos_sim::features::FeatureKind;
use pharmakos_sim::interpreter::StepFailure;
use pharmakos_sim::math::quantity::Ms;
use pharmakos_sim::pathing::estimate::{Speed, walk_cost};
use pharmakos_sim::pathing::search::Scratch;
use pharmakos_sim::pathing::surface::Node;
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::tables::{PRIORITY_NORMAL, SeatId};
use pharmakos_sim::targeting::{
    FeatureSpec, Ground, StoneCandidate, column_of, covering_site, on_vent_counted, toward_counted,
};
use pharmakos_sim::{MatchSettings, RulesTable, World, WorldConfig, default_rules_path, mapgen};
use std::path::PathBuf;

/// The seed the preset plays: `sim3a`'s search's finding when it was run
/// (decisions-log items 154 (1) and 156 (1); "The seed"). The lobby's line
/// spells it `0x0000000000000014`, as `host_link.gd`
/// spells a seed.
const PROBATION_SEED: u64 = 20;

/// What seed 20's chain is: its second vent, the columns of the three
/// placements that reach it, and the commander's walk through them, served
/// leg by leg, 92 s of a 180 s Push.
///
/// Re-pinned first by lane `p3fix`'s part 1, which priced near pairs at the
/// served route while `toward` still ranked its stones by the estimate: the
/// exact prices picked the second and third stones `[207, 69]` and `[223, 86]`
/// where the abstract prices had picked `[203, 72]` and `[222, 86]` (and
/// 1 620 ticks). The seed still passed, so the answer stood.
///
/// Re-pinned again by the same lane's second part, when `toward` came to weigh
/// its sites by the served walk, as every "nearest" in the sim does, rather
/// than by the item-61 estimate: the stones `[192, 51]`, `[207, 69]` and
/// `[223, 86]` (1 640 ticks) became `[202, 46]`, `[208, 69]` and `[223, 87]`,
/// and the walk 1 696 ticks. Each stone is the exact nearest to the vent; the
/// chain of them walks 56 ticks longer, since nothing makes the nearest stone
/// the one that shortens the whole walk. Seed 20 still passed and no earlier
/// seed did.
///
/// Re-pinned a third time by the same lane's fix pass 2, when the stone came
/// to be the legal site closest to the vent over the terrain, the grid optimum
/// (clause 3; `targeting`'s "Two measures"), and clause 4's walk came to be
/// served leg by leg: `[202, 46]`, `[208, 69]` and `[223, 87]` (1 696 ticks)
/// became `[197, 50]`, `[208, 71]` and `[224, 88]`, and 1 840 ticks. Seed 20
/// still passes every clause; seed 13 now passes too ("The seed").
const PINNED_VENT: [i32; 2] = [232, 96];
const PINNED_SITES: [[i32; 2]; 3] = [[197, 50], [208, 71], [224, 88]];
const PINNED_WALK_TICKS: i32 = 1_840;

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

/// The vent nearest seat 0's core that is not its start vent, with its squared
/// column distances from each core.
struct NearestVent {
    anchor: [i32; 2],
    own: i64,
    enemy: i64,
}

impl NearestVent {
    /// Its column distance from seat 0's core, in whole voxels, rounded down.
    fn voxels(&self) -> i64 {
        self.own.isqrt()
    }
}

/// [`NearestVent`], from the map the generator made.
fn nearest_other_vent(seed: u64, rules: &RulesTable) -> Result<NearestVent, Miss> {
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
    let anchor = map
        .features
        .features()
        .iter()
        .filter(|feature| feature.kind == FeatureKind::Vent && feature.anchor != start)
        .map(|feature| feature.anchor)
        // item 62: the key ends in `(y, x)`, unique per anchor, so it is total.
        .min_by_key(|anchor| (squared(*anchor, own), anchor[1], anchor[0]))
        .ok_or(Miss::NoSecondVent)?;
    Ok(NearestVent {
        anchor,
        own: squared(anchor, own),
        enemy: squared(anchor, enemy),
    })
}

/// The second vent's anchor, by the criterion's first clause. Cheap beside the
/// resolver, so it rules out most seeds.
fn second_vent(seed: u64, rules: &RulesTable) -> Result<[i32; 2], Miss> {
    let near = nearest_other_vent(seed, rules)?;
    if near.enemy <= near.own {
        return Err(Miss::NearerTheEnemy);
    }
    let sphere = i64::from(rules.sphere_radius_voxels());
    let placements = i64::try_from(MOST_PLACEMENTS).expect("three");
    let reach = sphere * (placements + 1);
    if near.own > reach * reach {
        return Err(Miss::TooFar);
    }
    Ok(near.anchor)
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

/// The cost and ticks of the walk the router serves a commander between two
/// nodes: one leg of clause 4's walk.
fn walk(ground: &Ground<'_>, scratch: &mut Scratch, from: Node, to: Node) -> Option<(i64, i32)> {
    let walk = walk_cost(ground.surface, ground.clusters, scratch, (from, to)).ok()?;
    let ticks = pharmakos_sim::pathing::estimate::walk_ticks(walk, Speed::commander(ground.rules))
        .expect("a leg's walk converts")?;
    Some((walk.cost, ticks))
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
    let mut scratch = {
        let ground = world.ground();
        Scratch::for_map(ground.surface, ground.rules.hpa_cluster_voxels())
            .expect("the map takes a search")
    };
    if covering_site(&world.ground(), seat, start, feature).is_some() {
        return Err(Miss::Given);
    }
    let mut stones: Vec<StoneCandidate> = Vec::new();
    let mut sites: Vec<[i32; 2]> = Vec::new();
    let mut walk_ticks = 0_i32;
    let mut at = start;
    for placement in 1..=MOST_PLACEMENTS {
        let ground = world.ground();
        let covers = covering_site(&ground, seat, at, feature);
        let site = match covers {
            Some(site) => site,
            None if placement == MOST_PLACEMENTS => return Err(Miss::OutOfReach),
            None => {
                let spec = FeatureSpec::Name {
                    kind: FeatureKind::Vent,
                    anchor: vent,
                };
                toward_counted(&ground, &mut scratch, &mut stones, seat, at, spec)
                    .map_err(|_| Miss::NoStone)?
                    .site
            }
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

/// The pin: seed 20 passes, with exactly the chain above, so the generator or
/// the resolver cannot drift under it.
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

/// The truth as found: of the seeds 0 through 20, exactly 13 and 20 pass
/// every clause ("The seed"). Each seed generates a map, so this is the
/// file's slow test, as the search it replaces was.
#[test]
fn the_passing_seeds_through_twenty_are_thirteen_and_twenty() {
    let rules = rules();
    let passing: Vec<u64> = (0..=PROBATION_SEED)
        .filter(|seed| probe(*seed, &rules).is_ok())
        .collect();
    assert_eq!(passing, [13, PROBATION_SEED]);
}

/// The criterion tells maps apart: the golden seed, the fixture the plan's
/// finding read, is too far.
#[test]
fn the_golden_seed_does_not_pass() {
    assert_eq!(probe(GOLDEN_SEED, &rules()), Err(Miss::TooFar));
}

/// A Generator can stand on the second vent from the third placement: the vent
/// is live, and the sim's own `on`, which a playbook's Build target reads,
/// names it from there and from no earlier placement.
#[test]
fn a_build_target_can_name_the_vent_from_the_third_placement() {
    let rules = rules();
    let mut world = world_of(PROBATION_SEED, &rules);
    let seat = SeatId::new(0);
    let feature = world
        .features()
        .index_of(FeatureKind::Vent, PINNED_VENT)
        .expect("the pinned vent");
    let mut scratch = {
        let ground = world.ground();
        Scratch::for_map(ground.surface, ground.rules.hpa_cluster_voxels())
            .expect("the map takes a search")
    };
    let spec = FeatureSpec::Name {
        kind: FeatureKind::Vent,
        anchor: PINNED_VENT,
    };
    for (placement, [x, y]) in PINNED_SITES.into_iter().enumerate() {
        let point = world.ground().standing(x, y).expect("a pinned site stands");
        let found = on_vent_counted(&world.ground(), &mut scratch, seat, point, None, None, spec);
        if placement + 1 == PINNED_SITES.len() {
            let site = found.expect("the third placement names the vent");
            assert_eq!(site.feature, feature);
        } else {
            assert_eq!(
                found.map_err(|failed| failed.reason),
                Err(StepFailure::NoTarget),
                "placement {placement} is a stone and does not reach the vent"
            );
        }
        world
            .place_beacon_directly(seat, point, MandateKind::None, PRIORITY_NORMAL)
            .expect("room for a stone");
    }
    assert!(
        world.ground().is_live(feature),
        "the vent is there to build on"
    );
}

/// S3's `pred`: a `toward` that finds no site says why, with how many sites
/// it weighed (S1's plan, decision 3). A name the map does not hold is
/// `no_target` having weighed nothing; a commander standing on no column of
/// the map reaches no site, `illegal_site`, and weighs none; and from the
/// commander's start the named vent's stone is the pinned first site.
#[test]
fn a_toward_says_why_it_found_no_site() {
    use pharmakos_sim::interpreter::StepFailed;
    use pharmakos_sim::math::fixed::Fx;
    let rules = rules();
    let world = world_of(PROBATION_SEED, &rules);
    let seat = SeatId::new(0);
    let commander = world.commander_of(seat).expect("a commander");
    let row = world.units().row_of(commander).expect("a live commander");
    let start = world.units().positions()[row.index()];
    let ground = world.ground();
    let mut scratch = Scratch::for_map(ground.surface, ground.rules.hpa_cluster_voxels())
        .expect("the map takes a search");
    let mut stones: Vec<StoneCandidate> = Vec::new();
    let named = |anchor: [i32; 2]| FeatureSpec::Name {
        kind: FeatureKind::Vent,
        anchor,
    };
    assert_eq!(
        toward_counted(
            &ground,
            &mut scratch,
            &mut stones,
            seat,
            start,
            named([1, 1])
        ),
        Err(StepFailed {
            reason: StepFailure::NoTarget,
            matched: Some(0),
        })
    );
    let nowhere = [Fx::from_voxels(-8), Fx::from_voxels(-8), Fx::from_voxels(0)];
    assert_eq!(
        toward_counted(
            &ground,
            &mut scratch,
            &mut stones,
            seat,
            nowhere,
            named(PINNED_VENT)
        ),
        Err(StepFailed {
            reason: StepFailure::IllegalSite,
            matched: Some(0),
        })
    );
    let found = toward_counted(
        &ground,
        &mut scratch,
        &mut stones,
        seat,
        start,
        named(PINNED_VENT),
    )
    .expect("a stone toward the second vent");
    assert_eq!(found.site, PINNED_SITES[0]);
}

/// S3's `pred`, fix pass 1 (C7): what one `toward` search costs, printed,
/// with no bound asserted (whether one is wanted is the owner's). For seed
/// 20, from the commander's start: toward the pinned second vent; toward the
/// live vent farthest from the core, whose walk the octile bound prunes
/// least; and toward the pinned vent again with a Resonance Spire finished at
/// the core, its sphere grown. Each prints the legal sites weighed, the
/// columns of the spheres scanned, the decision units charged (one a visit
/// of each loop the resolve runs -- a column scanned and every row, column
/// and Spire byte the tests on it read -- and one a node popped, lane
/// `dunit` and its part 2; the search prices no walk since fix pass 2 of
/// lane `p3fix`) and the nodes the search popped.
#[test]
#[ignore = "a measurement, run by hand: `cargo test --test probation_seed -- --ignored --nocapture toward_costs`"]
fn toward_costs() {
    use pharmakos_sim::seams::UnitTally;
    let rules = rules();
    let seat = SeatId::new(0);
    let measure = |world: &World, label: &str, anchor: [i32; 2]| {
        let commander = world.commander_of(seat).expect("a commander");
        let row = world.units().row_of(commander).expect("a live commander");
        let start = world.units().positions()[row.index()];
        let tally = UnitTally::new();
        let ground = Ground {
            work: &tally,
            ..world.ground()
        };
        let mut scratch = Scratch::for_map(ground.surface, ground.rules.hpa_cluster_voxels())
            .expect("the map takes a search");
        let mut stones: Vec<StoneCandidate> = Vec::new();
        let spec = FeatureSpec::Name {
            kind: FeatureKind::Vent,
            anchor,
        };
        // The columns of every own sphere's disc, as the search scans them.
        let columns: u32 = world
            .beacons()
            .seats()
            .iter()
            .zip(world.beacons().ids())
            .filter(|(owner, _)| **owner == seat.raw())
            .map(|(_, id)| {
                let radius = ground
                    .radius_of(Some(pharmakos_sim::tables::BeaconId::new(*id)))
                    .floor_voxels();
                u32::try_from(pharmakos_sim::targeting::spiral_offsets(radius).len())
                    .expect("a count")
            })
            .sum();
        let found = toward_counted(&ground, &mut scratch, &mut stones, seat, start, spec);
        let units = tally.read().expect("the decision units are measured");
        match found {
            Ok(stone) => println!(
                "{label}: site {:?}, {} legal sites weighed, {columns} columns scanned, {units} decision units, {} nodes popped; the queue: {} bucket turns over {} moves ({} to empty buckets), {} columns moved, {} cells reset",
                stone.site,
                stone.weighed,
                scratch.expansions().expect("the count fits a u64"),
                scratch.reach().turns().expect("a count"),
                scratch.reach().advances().expect("a count"),
                scratch.reach().turns().expect("a count")
                    - scratch.reach().advances().expect("a count"),
                scratch.reach().unlinked().expect("a count"),
                scratch.reach().filled().expect("a count"),
            ),
            Err(failed) => println!("{label}: no site, {failed:?}, {units} decision units"),
        }
    };
    let world = world_of(PROBATION_SEED, &rules);
    measure(&world, "the pinned vent", PINNED_VENT);
    let core_row = world
        .beacons()
        .seats()
        .iter()
        .position(|owner| *owner == seat.raw())
        .expect("a core");
    let [cx, cy] = column_of(world.beacons().positions()[core_row]);
    let far = world
        .features()
        .features()
        .iter()
        .filter(|feature| feature.kind == FeatureKind::Vent)
        .map(|feature| feature.anchor)
        .max_by_key(|anchor| (squared(*anchor, [cx, cy]), anchor[1], anchor[0]))
        .expect("a vent");
    measure(&world, &format!("the farthest vent {far:?}"), far);
    let mut grown = world_of(PROBATION_SEED, &rules);
    let core = pharmakos_sim::tables::BeaconId::new(
        *grown.beacons().ids().get(core_row).expect("the core's id"),
    );
    let beside = grown
        .ground()
        .standing(cx + 3, cy)
        .expect("a column beside the core");
    grown
        .raise_structure(
            seat,
            core,
            pharmakos_sim::tables::StructureKind::ResonanceSpire,
            beside,
        )
        .expect("room for a Spire");
    measure(
        &grown,
        "the pinned vent, the core grown by a Spire",
        PINNED_VENT,
    );
}

#[test]
#[ignore = "run by hand when a change moves the pinned answer"]
fn every_passing_seed_is_listed() {
    let rules = rules();
    for seed in 0..SURVEY_SEEDS {
        let distance = nearest_other_vent(seed, &rules)
            .map_or_else(|miss| format!("{miss:?}"), |near| near.voxels().to_string());
        match probe(seed, &rules) {
            Ok(chain) => println!("{seed} at {distance} passes: {chain:?}"),
            Err(miss) => println!("{seed} at {distance} misses: {miss:?}"),
        }
    }
}
