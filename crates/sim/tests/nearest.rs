// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! "Nearest" is the walk the walker will be served (lane `p3fix`), held to an
//! oracle on generated, walled and cratered terrain.
//!
//! (This commit holds the counted-work regression alone, red ahead of the
//! fix; the oracle tests arrive with the fix, whose API they use.)
//!
//! The oracle is the router itself, asked as a walker asks: [`route_with`]
//! from the start, and from each partial route's end again until a route
//! reaches the goal. It is not `walk_cost`'s code: a candidate it cannot price
//! fails the test rather than leaving the truth. Beside it, the walker: a
//! commander sent along a far pair and stepped until it arrives, its ticks set
//! against the priced walk's, with and without the threats its route avoids.
//!
//! The `toward` search's bound is held here too: the grid optimum
//! (`pathing::reach`) never above a served walk for any site, the chosen
//! stone equal to brute force over every site, and the counted work of one
//! resolve behind a ring of wall pinned.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report."
)]

use pharmakos_sim::encoding::Enc;
use pharmakos_sim::features::FeatureKind;
use pharmakos_sim::pathing::search::Scratch;
use pharmakos_sim::runner::MatchSettings;
use pharmakos_sim::seams::UnitTally;
use pharmakos_sim::tables::SeatId;
use pharmakos_sim::targeting::{FeatureSpec, Ground, StoneCandidate, toward_counted};
use pharmakos_sim::voxels::{Material, VoxelEdit};
use pharmakos_sim::{RulesTable, World, WorldConfig};

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

/// Nodes popped since the counters were last reset.
fn popped(scratch: &Scratch) -> u64 {
    scratch.expansions().expect("the count fits a u64")
        + scratch.abstract_expansions().expect("the count fits a u64")
}

/// One `toward` resolve's counted work on the probation seed, toward its
/// pinned vent from seat 0's commander: open ground, and the vent ringed by
/// stone three high at radius 10, 18 and 26 with the ring's gap facing away
/// from the core, where the octile bound pruned nothing (review probe B:
/// every one of 1 753 sites priced, 4.5 M nodes popped). Pinned: walks priced
/// and nodes popped, each a count the search makes, the same on every
/// platform. A change that moves one says why.
#[test]
fn a_toward_behind_a_ring_prices_few_walks() {
    const PINNED: [(&str, i32, u64, u64); 4] = [
        ("open", 0, OPEN_WALKS, OPEN_POPPED),
        ("ring 10", 10, RING10_WALKS, RING10_POPPED),
        ("ring 18", 18, RING18_WALKS, RING18_POPPED),
        ("ring 26", 26, RING26_WALKS, RING26_POPPED),
    ];
    let mut measured = Vec::new();
    for (label, radius, _, _) in PINNED {
        let mut world = world_of(PROBATION, 2);
        let seat = SeatId::new(0);
        let core_row = world
            .beacons()
            .seats()
            .iter()
            .position(|owner| *owner == seat.raw())
            .expect("a core");
        let core = pharmakos_sim::targeting::column_of(world.beacons().positions()[core_row]);
        if radius > 0 {
            let away = [PROBATION_VENT[0] - core[0], PROBATION_VENT[1] - core[1]];
            ring(&mut world, PROBATION_VENT, radius, away);
        }
        let commander = world.commander_of(seat).expect("a commander");
        let row = world.units().row_of(commander).expect("a live commander");
        let start = world.units().positions()[row.index()];
        let tally = UnitTally::new();
        let ground = Ground {
            work: &tally,
            ..world.ground()
        };
        let mut scratch =
            Scratch::for_map(ground.surface, ground.rules.hpa_cluster_voxels()).unwrap();
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
        let walks = scratch.walks().unwrap();
        let pops = popped(&scratch);
        println!(
            "{label}: {:?}, {} sites, {walks} walks priced, {pops} nodes popped, {} decision units",
            found.map(|stone| stone.site),
            stones.len(),
            tally.get()
        );
        measured.push((label, walks, pops));
    }
    let pinned: Vec<(&str, u64, u64)> = PINNED
        .iter()
        .map(|(label, _, walks, pops)| (*label, *walks, *pops))
        .collect();
    assert_eq!(measured, pinned);
}

// Measured at this lane's fix pass 2. Before it (the octile bound), probe B
// priced 140 walks and popped 306 k nodes on open ground and priced every one
// of the 1 753 sites behind each ring (4.5 M nodes popped at radius 26).
const OPEN_WALKS: u64 = 82;
const OPEN_POPPED: u64 = 191_142;
const RING10_WALKS: u64 = 11;
const RING10_POPPED: u64 = 44_989;
const RING18_WALKS: u64 = 45;
const RING18_POPPED: u64 = 141_727;
const RING26_WALKS: u64 = 9;
const RING26_POPPED: u64 = 56_342;

