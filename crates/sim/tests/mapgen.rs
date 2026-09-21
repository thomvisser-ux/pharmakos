// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The seeded map generator: its invariants, and the per-seed digest golden.
//!
//! This file also **produces** `<target>/golden/mapgen/actual.digests.txt`,
//! which `cargo xtask ci`'s `golden` step compares against the committed
//! `tests/golden/mapgen/expected.digests.txt`. The `test` step runs before
//! `golden`, so the fresh output is always there by the time the comparison
//! happens.
//!
//! The four acceptance lines skeleton plan section 3 (T5) names are the four
//! tests spelled after them: `mapgen_is_seed_deterministic`,
//! `spawn_distance_holds_for_every_seed_in_the_set`,
//! `vents_are_reachable_and_ore_is_equal_across_occupied_zones` and
//! `supply_is_within_the_power_ceiling`.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules — not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording), and that covers an out-of-bounds index in an assertion for the same reason: these are panic lints, not determinism lints, and AGENTS.md §5's ban is on the latter."
)]

use std::collections::VecDeque;
use std::path::PathBuf;

use pharmakos_sim::encoding::hex;
use pharmakos_sim::mapgen::{self, GeneratedMap, MapFile, MapReport};
use pharmakos_sim::tables::{BeaconId, UnitKind};
use pharmakos_sim::voxels::{Material, Richness, VoxelStore};
use pharmakos_sim::{MatchSettings, RulesTable, World, WorldConfig};

/// The committed seed set: eight seeds, pinned, in ascending order.
///
/// `0x00000000ca5caded` is on the list because
/// `scenarios/skeleton/expand-east.scenario.jsonc` names it — the worked
/// example of the scenario format asserts on a map, so the map has to exist.
/// The rest are chosen to span the space a `u64` seed can be wrong in: zero and
/// all-ones (a generator that masks or shifts wrongly fails on exactly those),
/// one, the determinism harness's own seed, the project's hash seed, the
/// golden-ratio constant the RNG is built on, and one arbitrary value.
const SEEDS: [u64; 8] = [
    0x0000_0000_0000_0000,
    0x0000_0000_0000_0001,
    0x0000_0000_CA5C_ADED,
    0x0102_0304_0506_0708,
    0x5048_4152_4D4B_4F53,
    0x9E37_79B9_7F4A_7C15,
    0xDEAD_BEEF_CAFE_F00D,
    0xFFFF_FFFF_FFFF_FFFF,
];

/// Seats the golden is generated at: three, a full v1 match, so every zone the
/// map carries is occupied and every invariant below has something to check.
const GOLDEN_SEATS: u32 = 3;

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

fn generate(seed: u64) -> GeneratedMap {
    mapgen::generate(seed, &rules(), GOLDEN_SEATS).expect("the committed rules table makes a map")
}

/// The map's `map` block, as the generator reads it.
fn map_rows() -> pharmakos_proto::gp::v1::rules_table::Map {
    rules().message().map.expect("the map block")
}

// ---------------------------------------------------------------------------
// The four acceptance lines
// ---------------------------------------------------------------------------

#[test]
fn mapgen_is_seed_deterministic() {
    // Twice from the same seed, byte for byte: the store's chunks, its digest,
    // and every table the generator fills. This is the property the committed
    // digests below assert across three operating systems; asserting it twice
    // in one process is what localises a break to the generator rather than to
    // the platform.
    for seed in SEEDS {
        let first = generate(seed);
        let second = generate(seed);
        assert_eq!(
            first.voxels.store_digest(),
            second.voxels.store_digest(),
            "seed {} produced two different maps",
            hex(seed)
        );
        let mut chunk: u32 = 0;
        while chunk < first.voxels.chunk_count() {
            assert_eq!(
                first.voxels.chunk_bytes(chunk),
                second.voxels.chunk_bytes(chunk),
                "seed {}: chunk {chunk} differs between two generations",
                hex(seed)
            );
            chunk = chunk.saturating_add(1);
        }
        assert_eq!(first.beacons, second.beacons);
        assert_eq!(first.starting_units, second.starting_units);
        assert_eq!(first.report, second.report);
    }

    // And two different seeds must not produce the same map, or the seed is
    // being ignored and every test above would pass vacuously.
    let mut digests: Vec<u64> = SEEDS
        .iter()
        .map(|seed| generate(*seed).voxels.store_digest())
        .collect();
    digests.sort_unstable();
    let before = digests.len();
    digests.dedup();
    assert_eq!(
        digests.len(),
        before,
        "two seeds produced the same map; the seed is not reaching the generator"
    );
}

#[test]
fn spawn_distance_holds_for_every_seed_in_the_set() {
    // Item 90's rule, on the octile ground distance (10/14 per step), which is a
    // LOWER bound on any walked path and so conservative in the right direction
    // until T7 measures real routes.
    let rows = map_rows();
    for seed in SEEDS {
        let report = generate(seed).report;
        assert!(
            report.min_separation_cost >= report.required_separation_cost,
            "seed {}: spawn centres {} cost units apart, rule asks {}\n{report:#?}",
            hex(seed),
            report.min_separation_cost,
            report.required_separation_cost
        );
        // The rule's own arithmetic, re-derived here rather than taken from the
        // generator: raider 12 cost/s over a 180 000 ms first segment is 2 160
        // cost units, and 3/2 of that is 3 240 (decisions log item 90, as item
        // 95 leaves it).
        assert_eq!(
            report.required_separation_cost, 3_240,
            "the committed table's spawn-distance rule is no longer 3 240 cost units"
        );
        assert!(
            report.separation_raider_seconds > 0 && report.separation_commander_seconds > 0,
            "seed {}: the separation must be reportable in both travel times",
            hex(seed)
        );
        // Printed rather than only asserted: skeleton plan section 3 (T5) asks
        // for the separation "reported in both raider travel and commander
        // travel", and a number nobody can read is not a report. `cargo test`
        // captures this unless the test fails or `--nocapture` is passed.
        println!(
            "0x{}  separation {} cost units (rule {})  raider {} s  commander {} s",
            hex(seed),
            report.min_separation_cost,
            report.required_separation_cost,
            report.separation_raider_seconds,
            report.separation_commander_seconds
        );
        // Every zone sits wholly on the map.
        let radius = i64::from(rows.spawn_zone_radius_voxels);
        for zone in &report.zones {
            let x = i64::from(zone.centre.first().copied().unwrap_or(0));
            let y = i64::from(zone.centre.get(1).copied().unwrap_or(0));
            assert!(x - radius >= 0 && y - radius >= 0);
            assert!(x + radius < i64::from(rows.size_x));
            assert!(y + radius < i64::from(rows.size_y));
        }
    }
}

#[test]
fn vents_are_reachable_and_ore_is_equal_across_occupied_zones() {
    let rows = map_rows();
    let sphere = i32::try_from(
        rules()
            .message()
            .beacon
            .expect("the beacon block")
            .sphere_radius_voxels,
    )
    .unwrap();

    for seed in SEEDS {
        let map = generate(seed);
        let mut ore_counts: Vec<u32> = Vec::new();
        for zone in &map.report.zones {
            let Some(vent) = zone.vent else {
                assert!(
                    zone.seam.is_none(),
                    "an unoccupied zone is terrain and nothing else"
                );
                continue;
            };
            let seam = zone.seam.expect("an occupied zone carries a seam");
            let core = zone.centre;

            // The band, in both directions.
            assert!(
                vent.from_core_voxels >= i32::try_from(rows.vent_min_distance_voxels).unwrap()
                    && vent.from_core_voxels
                        <= i32::try_from(rows.vent_max_distance_voxels).unwrap(),
                "seed {}: the vent is {} voxels from its core, outside {}..={}",
                hex(seed),
                vent.from_core_voxels,
                rows.vent_min_distance_voxels,
                rows.vent_max_distance_voxels
            );
            // Item 95: the reach is TWO beacon spheres — the commander walks to
            // the edge of the core's sphere and places a beacon whose own
            // sphere reaches as far again. Never the placement range.
            assert!(
                vent.from_core_voxels <= sphere.saturating_mul(2),
                "seed {}: the vent is outside the reach of a beacon placed at the edge of the \
                 core's sphere",
                hex(seed)
            );
            assert!(
                seam.from_core_voxels >= i32::try_from(rows.seam_min_distance_voxels).unwrap()
                    && seam.from_core_voxels
                        <= i32::try_from(rows.seam_max_distance_voxels).unwrap(),
                "seed {}: the seam is {} voxels from its core, outside {}..={}",
                hex(seed),
                seam.from_core_voxels,
                rows.seam_min_distance_voxels,
                rows.seam_max_distance_voxels
            );
            assert!(
                seam.from_core_voxels <= sphere,
                "seed {}: the seam must sit inside the core's own sphere, so the starting mining \
                 drone digs from tick one",
                hex(seed)
            );

            // Reachable: a surface-connected walk of one-voxel steps from the
            // core to the vent. Not "the generator says so" — an actual walk.
            assert!(
                surface_walk_exists(&map.voxels, core, vent.at),
                "seed {}: no surface walk from the core at {core:?} to the vent at {:?}",
                hex(seed),
                vent.at
            );
            assert!(
                surface_walk_exists(&map.voxels, core, seam.at),
                "seed {}: no surface walk from the core to its seam",
                hex(seed)
            );

            assert_eq!(
                vent.richness,
                Richness::Lean,
                "map.start_vent_richness is LEAN at the committed table"
            );
            assert_eq!(seam.richness, Richness::Standard);
            ore_counts.push(ore_near(&map.voxels, core, 30));
        }

        assert_eq!(
            ore_counts.len(),
            usize::try_from(GOLDEN_SEATS).unwrap(),
            "every occupied zone must be realised"
        );
        let first = ore_counts.first().copied().unwrap_or(0);
        assert!(
            ore_counts.iter().all(|c| *c == first),
            "seed {}: nearby ore is not equal across the occupied zones: {ore_counts:?}",
            hex(seed)
        );
        assert_eq!(
            first,
            rules()
                .message()
                .economy
                .expect("the economy block")
                .seam_voxels,
            "seed {}: a starting seam is `economy.seam_voxels` voxels of ore and nothing else",
            hex(seed)
        );
    }
}

#[test]
fn supply_is_within_the_power_ceiling() {
    // Item 65's provisional 190 kW, with item 91's two PLACEHOLDERs inside it.
    // Item 90's own arithmetic: three seats = 3 x 10 core surplus + 3 x 20 lean
    // start vents + 2 x 30 contested standard + 40 contested rich = 190 kW,
    // exactly the ceiling; two seats = 160 kW.
    for seed in SEEDS {
        let report = generate(seed).report;
        assert!(
            report.total_supply_kw <= report.power_ceiling_kw,
            "seed {}: {} kW over a {} kW ceiling",
            hex(seed),
            report.total_supply_kw,
            report.power_ceiling_kw
        );
        assert_eq!(report.power_ceiling_kw, 190, "item 65");
        assert_eq!(
            report.total_supply_kw,
            190,
            "seed {}: three seats is item 90's worked total",
            hex(seed)
        );
    }

    let two = mapgen::generate(SEEDS[2], &rules(), 2).expect("a two-seat map");
    assert_eq!(
        two.report.total_supply_kw, 160,
        "item 90: two seats is 160 kW"
    );
}

// ---------------------------------------------------------------------------
// The generator's other promises
// ---------------------------------------------------------------------------

#[test]
fn the_terrain_has_no_overhangs_and_no_cliffs() {
    // Both halves of the property vent reachability rests on. A heightmap has
    // no overhangs by construction — every voxel below the top is solid — and
    // the 1-Lipschitz closure is what makes every cardinal step walkable.
    let map = generate(SEEDS[2]);
    let size = map.voxels.size();
    let width = i32::try_from(size[0]).unwrap();
    let depth = i32::try_from(size[1]).unwrap();
    let tops = heightmap(&map.voxels);

    // No overhangs: everything below the top of a column is solid, everything
    // above it is air. Checked on a stride rather than on all 147 456 columns,
    // because the property is structural and a stride of 7 is coprime with the
    // chunk edge, so it visits every chunk and every in-chunk offset.
    let mut y: i32 = 0;
    while y < depth {
        let mut x: i32 = 0;
        while x < width {
            let top = tops
                .get(usize::try_from(y * width + x).unwrap())
                .copied()
                .unwrap();
            assert!(top >= 0, "every column has a floor");
            let mut z: i32 = 0;
            while z < i32::try_from(size[2]).unwrap() {
                let solid = map.voxels.get([x, y, z]).unwrap().is_solid();
                assert_eq!(
                    solid,
                    z <= top,
                    "column ({x}, {y}) is not a heightmap at z {z}"
                );
                z = z.saturating_add(1);
            }
            x = x.saturating_add(7);
        }
        y = y.saturating_add(7);
    }

    // No cliffs: a cardinal neighbour is at most one voxel away in height.
    let mut y: i32 = 0;
    while y < depth {
        let mut x: i32 = 0;
        while x < width {
            let here = tops[usize::try_from(y * width + x).unwrap()];
            if x + 1 < width {
                let east = tops[usize::try_from(y * width + x + 1).unwrap()];
                assert!(
                    (here - east).abs() <= 1,
                    "a cliff of {} voxels at ({x}, {y}) going east",
                    (here - east).abs()
                );
            }
            if y + 1 < depth {
                let north = tops[usize::try_from((y + 1) * width + x).unwrap()];
                assert!(
                    (here - north).abs() <= 1,
                    "a cliff of {} voxels at ({x}, {y}) going north",
                    (here - north).abs()
                );
            }
            x = x.saturating_add(1);
        }
        y = y.saturating_add(1);
    }
}

#[test]
fn spawn_zones_are_flat_inside_and_ramped_at_the_rim() {
    // What a zone actually promises, said exactly. `flatten_zones` levels the
    // whole disc to the height at its centre, and *then* the 1-Lipschitz
    // closure runs and only ever lowers, so where the ground outside the zone
    // is lower the closure cuts the zone's outer ring into a ramp — one voxel
    // per voxel, never a step. The inner disc is untouched by that, because the
    // closure can only reach `d` voxels in from the rim if the outside is `d`
    // voxels down.
    //
    // Pinned at both ends so neither claim can drift: the inner half is exactly
    // level, and the rim's ramp is at most RIM_DROP voxels deep across the whole
    // committed seed set. A change that widens the ramp moves this number, and
    // moving it is a statement about what a seat can build on.
    const RIM_DROP: i32 = 6;

    let radius = i32::try_from(
        rules()
            .message()
            .map
            .expect("the map block")
            .spawn_zone_radius_voxels,
    )
    .unwrap();
    // Half the radius, as a shift: `clippy::integer_division` is denied and a
    // shift's floor is this crate's documented rounding.
    let inner2 = (radius >> 1) * (radius >> 1);
    let mut worst = 0;
    for seed in SEEDS {
        let map = generate(seed);
        for zone in &map.report.zones {
            let (cx, cy) = (zone.centre[0], zone.centre[1]);
            let level = map.voxels.top_solid_z(cx, cy).unwrap();
            let mut dy = -radius;
            while dy <= radius {
                let mut dx = -radius;
                while dx <= radius {
                    let d2 = dx * dx + dy * dy;
                    if d2 <= radius * radius {
                        let top = map.voxels.top_solid_z(cx + dx, cy + dy).unwrap();
                        let drop = level - top;
                        assert!(
                            drop >= 0,
                            "seed {}: a column inside the zone at ({dx}, {dy}) is above the flattened level",
                            hex(seed)
                        );
                        if d2 <= inner2 {
                            assert_eq!(
                                drop,
                                0,
                                "seed {}: the inner half of a zone is not level: ({dx}, {dy}) is {drop} down",
                                hex(seed)
                            );
                        }
                        worst = worst.max(drop);
                    }
                    dx += 1;
                }
                dy += 1;
            }
        }
    }
    assert!(
        worst <= RIM_DROP,
        "the zone rim now ramps {worst} voxels down, past the {RIM_DROP} this test pins: either \
         the terrain got lumpier or the flattening changed, and the pull request owes which"
    );
}

#[test]
fn an_unoccupied_zone_is_terrain_and_nothing_else() {
    // Item 90: only OCCUPIED zones are realised. One seat on a three-zone map
    // leaves two zones with no core, no vent and no seam.
    let map = mapgen::generate(SEEDS[2], &rules(), 1).expect("a one-seat map");
    assert_eq!(map.beacons.len(), 1, "one occupied zone, one core beacon");
    let realised = map.report.zones.iter().filter(|z| z.seat.is_some()).count();
    assert_eq!(realised, 1);
    for zone in map.report.zones.iter().filter(|z| z.seat.is_none()) {
        assert!(zone.vent.is_none() && zone.seam.is_none());
        assert_eq!(
            ore_near(&map.voxels, zone.centre, 30),
            0,
            "an unoccupied zone carries no ore"
        );
    }
}

#[test]
fn the_starting_force_is_the_rules_table_rows() {
    // Item 95: the starting force is two rules-table rows plus the core and the
    // commander, which every occupied seat has exactly one of by design.
    let units = rules().message().units.expect("the units block");
    let map = generate(SEEDS[2]);
    let per_seat = 1 + units.starting_build_drones + units.starting_mining_drones;
    assert_eq!(
        u32::try_from(map.starting_units.len()).unwrap(),
        per_seat.saturating_mul(GOLDEN_SEATS)
    );
    for seat in 0..GOLDEN_SEATS {
        let mine: Vec<UnitKind> = map
            .starting_units
            .iter()
            .filter(|u| u32::from(u.seat.raw()) == seat)
            .map(|u| u.kind)
            .collect();
        assert_eq!(
            mine.iter().filter(|k| **k == UnitKind::Commander).count(),
            1,
            "exactly one commander per occupied seat"
        );
        assert_eq!(
            u32::try_from(mine.iter().filter(|k| **k == UnitKind::BuildDrone).count()).unwrap(),
            units.starting_build_drones
        );
        assert_eq!(
            u32::try_from(mine.iter().filter(|k| **k == UnitKind::MiningDrone).count()).unwrap(),
            units.starting_mining_drones
        );
    }
    // Treasury is `economy.bmi_dollars * economy.starting_bmi_multiplier`.
    let economy = rules().message().economy.expect("the economy block");
    let expected = i64::from(economy.bmi_dollars) * i64::from(economy.starting_bmi_multiplier);
    for treasury in &map.seat_treasury {
        assert_eq!(treasury.raw(), expected);
    }
}

#[test]
fn a_beacon_names_itself_the_way_a_playbook_does() {
    // The string a playbook's `beacon_id` resolves against. The verifier matches
    // on strings it is handed and the gateway builds that list from this table,
    // so the spelling is a contract between three crates.
    assert_eq!(BeaconId::new(0).playbook_id(), "b_00");
    assert_eq!(BeaconId::new(1).playbook_id(), "b_01");
    assert_eq!(BeaconId::new(9).playbook_id(), "b_09");
    assert_eq!(BeaconId::new(42).playbook_id(), "b_42");
    // The padding is a minimum, never a truncation.
    assert_eq!(BeaconId::new(100).playbook_id(), "b_100");

    let map = generate(SEEDS[2]);
    let names: Vec<String> = map
        .beacons
        .ids()
        .iter()
        .map(|id| BeaconId::new(*id).playbook_id())
        .collect();
    assert_eq!(names, ["b_00", "b_01", "b_02"]);
}

#[test]
fn a_map_file_round_trips() {
    // "The map is written as a file" (skeleton plan T5). It is a convenience for
    // a tool that wants the voxels without the generator, never the authority on
    // what a seed means — so what it owes is exactly that the bytes come back.
    let map = generate(SEEDS[2]);
    let file = MapFile::capture(&map.voxels, SEEDS[2]);
    let bytes = file.to_bytes().expect("encoding a map file");
    let decoded = MapFile::from_bytes(&bytes).expect("decoding a map file");
    assert_eq!(decoded, file);
    assert_eq!(decoded.seed, SEEDS[2]);
    assert_eq!(decoded.size, map.voxels.size());
    assert_eq!(
        decoded.chunks.len(),
        usize::try_from(map.voxels.chunk_count()).unwrap() * pharmakos_sim::CHUNK_VOXELS
    );

    let mut wrong = file;
    wrong.version = wrong.version.saturating_add(1);
    let bytes = wrong.to_bytes().expect("encoding");
    assert!(
        matches!(
            MapFile::from_bytes(&bytes),
            Err(pharmakos_sim::MapError::FileVersion { .. })
        ),
        "a map file from another version must refuse to load"
    );
}

#[test]
fn a_rules_table_that_cannot_make_a_map_is_an_error_and_not_a_panic() {
    // Every refusal names the row. None of them is a panic: a rules table that
    // cannot describe a map is a table to fix.
    let edited = |edit: &dyn Fn(&mut pharmakos_proto::gp::v1::RulesTable)| {
        let text = std::fs::read_to_string(repo_root().join("rules").join("rules.v1.json"))
            .expect("the committed rules table exists");
        let mut message: pharmakos_proto::gp::v1::RulesTable =
            pharmakos_proto::json::decode(&text).expect("canonical gp.v1 JSON");
        edit(&mut message);
        RulesTable::from_message(&message).expect("still a table")
    };

    // An extent that is not a whole number of chunks.
    let table = edited(&|m| m.map.get_or_insert_default().size_x = 300);
    assert!(matches!(
        mapgen::generate(SEEDS[2], &table, 3),
        Err(pharmakos_sim::MapError::BadExtent { .. })
    ));

    // A vent band outside two beacon spheres (item 95's check).
    let table = edited(&|m| m.map.get_or_insert_default().vent_max_distance_voxels = 200);
    assert!(matches!(
        mapgen::generate(SEEDS[2], &table, 3),
        Err(pharmakos_sim::MapError::OutOfRange { .. })
    ));

    // A power ceiling the map cannot fit under.
    let table = edited(&|m| m.power.get_or_insert_default().map_ceiling_kw = 10);
    assert!(matches!(
        mapgen::generate(SEEDS[2], &table, 3),
        Err(pharmakos_sim::MapError::PowerCeiling { .. })
    ));

    // A zone count the skeleton's layout does not cover (S4 owns the general
    // case, and says so in the error).
    let table = edited(&|m| m.map.get_or_insert_default().spawn_zones = 4);
    assert!(matches!(
        mapgen::generate(SEEDS[2], &table, 3),
        Err(pharmakos_sim::MapError::OutOfRange { .. })
    ));
}

#[test]
fn the_world_takes_its_map_from_the_generator() {
    // T5's item 6: `World::new` no longer carries harness constants for the map
    // footprint, the unit HP or the walking speed — the rules table and the
    // generator carry all three.
    let world = World::new(&WorldConfig {
        match_seed: SEEDS[2],
        seats: GOLDEN_SEATS,
        units_per_seat: 0,
        rules: rules(),
        match_settings: MatchSettings::default(),
    })
    .expect("a world on the committed table");

    assert_eq!(world.beacons().len(), GOLDEN_SEATS);
    assert_eq!(world.units().len(), 4 * GOLDEN_SEATS, "1 + 2 + 1 per seat");
    assert_eq!(
        world.chunks().len(),
        world.voxels().chunk_count(),
        "one digest per chunk"
    );
    assert_eq!(world.voxels().chunk_count(), 288, "12 x 12 x 2");
    assert!(world.structures().is_empty());
    assert!(world.wrecks().is_empty());

    // Every unit stands on the surface of its own column, which is what makes
    // the store part of the chain's causal path rather than merely beside it.
    for (index, position) in world.units().positions().iter().enumerate() {
        let x = position[0].floor_voxels();
        let y = position[1].floor_voxels();
        assert_eq!(
            position[2].floor_voxels(),
            world.voxels().standing_z(x, y),
            "unit {index} is not standing on its column"
        );
    }

    // Item 90's numbers reach the tables rather than a constant in the world.
    let commander_hp = rules().message().commander.expect("the commander block").hp;
    let hp = world
        .units()
        .kinds()
        .iter()
        .zip(world.units().hit_points())
        .find(|(kind, _)| **kind == UnitKind::Commander.id())
        .map(|(_, hp)| hp.raw())
        .expect("a commander in the table");
    assert_eq!(u32::try_from(hp).unwrap(), commander_hp);
}

// ---------------------------------------------------------------------------
// The golden
// ---------------------------------------------------------------------------

#[test]
fn the_per_seed_digests_match_their_golden() {
    let mut fresh = String::new();
    for seed in SEEDS {
        // One generation per seed, not two: a `GeneratedMap` carries both halves
        // of the line, and this is the slowest test in the crate.
        let map = generate(seed);
        let digest = map.voxels.store_digest();
        fresh.push_str(&line(seed, digest, &map.report));
    }

    if let Some(target) = target_dir() {
        let out = target.join("golden").join("mapgen");
        std::fs::create_dir_all(&out).expect("creating the golden output directory");
        std::fs::write(out.join("actual.digests.txt"), fresh.as_bytes())
            .expect("writing actual.digests.txt");
    }

    let golden_path = repo_root()
        .join("tests")
        .join("golden")
        .join("mapgen")
        .join("expected.digests.txt");
    let Ok(golden) = std::fs::read_to_string(&golden_path) else {
        panic!(
            "no committed golden at {}. `cargo xtask golden --bless` accepts the fresh output.",
            golden_path.display()
        );
    };
    assert!(
        !golden.contains('\r'),
        "a carriage return reached the mapgen golden; it is byte-compared across three operating \
         systems"
    );
    if golden != fresh {
        let first = golden
            .lines()
            .zip(fresh.lines())
            .find(|(a, b)| a != b)
            .map_or_else(
                || "the files differ in length".to_owned(),
                |(a, b)| format!("expected `{a}`, found `{b}`"),
            );
        panic!(
            "the map digests moved: {} differs from this run.\n  {first}\n  A moved digest is a \
             generator change, and every scenario and hash chain built on these seeds moves with \
             it. Explain it in the pull request.",
            golden_path.display()
        );
    }
}

/// One golden line, exactly as `tests/golden/mapgen/README.md` describes it.
fn line(seed: u64, digest: u64, report: &MapReport) -> String {
    format!(
        "0x{}\t{}\t{}\t{}\n",
        hex(seed),
        hex(digest),
        report.separation_raider_seconds,
        report.separation_commander_seconds
    )
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The top solid `z` of every column, row-major. Computed once per map, because
/// the walk below would otherwise rescan the same column many times.
fn heightmap(voxels: &VoxelStore) -> Vec<i32> {
    let size = voxels.size();
    let width = i32::try_from(size[0]).unwrap();
    let depth = i32::try_from(size[1]).unwrap();
    let mut out: Vec<i32> = Vec::with_capacity(usize::try_from(width * depth).unwrap());
    let mut y: i32 = 0;
    while y < depth {
        let mut x: i32 = 0;
        while x < width {
            out.push(voxels.top_solid_z(x, y).unwrap_or(-1));
            x = x.saturating_add(1);
        }
        y = y.saturating_add(1);
    }
    out
}

/// Whether a surface walk of one-voxel cardinal steps joins `from` to `to`.
///
/// A step is legal when both columns have a floor and their tops differ by at
/// most one voxel — item 59's one-voxel climb. Bounded to a box around the two
/// endpoints, which is enough because the terrain is 1-Lipschitz: a walk that
/// has to leave the box does not exist on a map where every step is legal.
fn surface_walk_exists(voxels: &VoxelStore, from: [i32; 3], to: [i32; 3]) -> bool {
    let size = voxels.size();
    let width = i32::try_from(size[0]).unwrap();
    let depth = i32::try_from(size[1]).unwrap();
    let pad = 16;
    let lo_x = from[0].min(to[0]).saturating_sub(pad).max(0);
    let hi_x = from[0].max(to[0]).saturating_add(pad).min(width - 1);
    let lo_y = from[1].min(to[1]).saturating_sub(pad).max(0);
    let hi_y = from[1].max(to[1]).saturating_add(pad).min(depth - 1);
    let span_x = hi_x - lo_x + 1;
    let span_y = hi_y - lo_y + 1;

    let index = |x: i32, y: i32| usize::try_from((y - lo_y) * span_x + (x - lo_x)).unwrap();
    let mut seen = vec![false; usize::try_from(span_x * span_y).unwrap()];
    let mut queue: VecDeque<[i32; 2]> = VecDeque::new();
    seen[index(from[0], from[1])] = true;
    queue.push_back([from[0], from[1]]);

    while let Some([x, y]) = queue.pop_front() {
        if x == to[0] && y == to[1] {
            return true;
        }
        let Some(here) = voxels.top_solid_z(x, y) else {
            continue;
        };
        for [dx, dy] in [[1, 0], [-1, 0], [0, 1], [0, -1]] {
            let (nx, ny) = (x + dx, y + dy);
            if nx < lo_x || nx > hi_x || ny < lo_y || ny > hi_y {
                continue;
            }
            if seen[index(nx, ny)] {
                continue;
            }
            let Some(there) = voxels.top_solid_z(nx, ny) else {
                continue;
            };
            if (here - there).abs() > 1 {
                continue;
            }
            seen[index(nx, ny)] = true;
            queue.push_back([nx, ny]);
        }
    }
    false
}

/// How many ore voxels sit within `radius` columns of `at`, in the top eight
/// voxels of each column — which is where a seam is laid.
fn ore_near(voxels: &VoxelStore, at: [i32; 3], radius: i32) -> u32 {
    let mut count: u32 = 0;
    let mut dy = -radius;
    while dy <= radius {
        let mut dx = -radius;
        while dx <= radius {
            let x = at[0].saturating_add(dx);
            let y = at[1].saturating_add(dy);
            if let Some(top) = voxels.top_solid_z(x, y) {
                let mut k: i32 = 0;
                while k < 8 {
                    let z = top - k;
                    if z < 0 {
                        break;
                    }
                    if voxels
                        .get([x, y, z])
                        .unwrap_or(Material::AIR)
                        .ore_richness()
                        .is_some()
                    {
                        count = count.saturating_add(1);
                    }
                    k = k.saturating_add(1);
                }
            }
            dx = dx.saturating_add(1);
        }
        dy = dy.saturating_add(1);
    }
    count
}
