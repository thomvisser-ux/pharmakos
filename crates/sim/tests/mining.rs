// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Mine mandate's four settings and the held seam (S1's plan, task `mine`;
//! spec section 6's Mine row; decisions-log items 127 (9) and 128 (3) (e);
//! register S1-38).
//!
//! Each test below is one line of the task's acceptance, under the name the
//! plan gives it, plus the item 131 (5) question the lane carried: a
//! `set_mandate` row now writes the settings it carries. Like the economy's
//! tests they assert the **rule** -- which voxels may go, which seam is held --
//! rather than a count of `$`, which is rules-table data.
//!
//! Two fixtures. The economy's seed at three seats, whose seat 0 core stands
//! beside its starting seam: the dig tests switch that core to Mine and play
//! it. And the same map's two rich contested seams, `seam_150_158` and
//! `seam_202_170`, whose anchors are 53 voxels apart, with a beacon at their
//! midpoint: under a table whose `beacon.sphere_radius_voxels` is widened to
//! 40 (and nothing else changed) that beacon reaches all of both, so a seam
//! *choice* is a choice, and the two are laid with the same ore at the same
//! grade, so `RICHEST` starts from a tie. The committed table's 24-voxel
//! sphere reaches a handful of voxels of each from there, too few to tell a
//! rule from a coincidence.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report."
)]

use pharmakos_proto::gp;
use pharmakos_proto::json;
use pharmakos_sim::events::EventKind;
use pharmakos_sim::features::{Feature, FeatureKind};
use pharmakos_sim::interpreter::{Plan, Row};
use pharmakos_sim::math::quantity::Money;
use pharmakos_sim::mining::{MineEdit, MineSettings, SeamChoice, remaining_yield, settings_at};
use pharmakos_sim::runner::{MatchPhase, MatchSettings, Runner};
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::snapshot::{Snapshot, SnapshotError};
use pharmakos_sim::tables::{BeaconId, PRIORITY_NORMAL, SeatId, StructureKind, UnitKind};
use pharmakos_sim::targeting::NO_FEATURE;
use pharmakos_sim::voxels::{Material, VoxelEdit};
use pharmakos_sim::{RulesTable, Urgency, World, WorldConfig, mandate_for};
use std::path::PathBuf;

/// The economy tests' seed: seat 0's core stands at (359, 27) beside its
/// starting seam, `seam_347_29`.
const SEED: u64 = 0x00A1_B2C3_D4E5_F607;

/// Three seats, the economy's fixture.
const SEATS: u32 = 3;

/// The seat every test drives.
const SEAT: SeatId = SeatId::new(0);

/// The two rich contested seams 53 voxels apart on [`SEED`].
const WEST: &str = "seam_150_158";
const EAST: &str = "seam_202_170";

/// Their midpoint, where one beacon reaches both.
const MIDPOINT_X: i32 = 176;
const MIDPOINT_Y: i32 = 164;

fn repo_root() -> PathBuf {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.parent()
        .and_then(std::path::Path::parent)
        .map(PathBuf::from)
        .unwrap_or(here)
}

fn rules() -> RulesTable {
    RulesTable::load(&repo_root().join("rules").join("rules.v1.json"))
        .expect("the committed rules table loads")
}

fn world(segments: &[i32]) -> World {
    World::new(&WorldConfig {
        match_seed: SEED,
        seats: SEATS,
        units_per_seat: 0,
        rules: rules(),
        match_settings: MatchSettings {
            segment_lengths_ms: segments.to_vec(),
            round_limit: u32::try_from(segments.len()).expect("a few segments"),
        },
    })
    .expect("the rules table describes a map")
}

/// Seat 0's core: its ordinal-0 beacon.
fn core(world: &World) -> BeaconId {
    let row = world
        .beacons()
        .row_of_ordinal(SEAT, 0)
        .expect("seat 0 has a core");
    BeaconId::new(u32::try_from(row).expect("a beacon row"))
}

fn row_of(beacon: BeaconId) -> usize {
    usize::try_from(beacon.raw()).expect("a beacon row")
}

/// The seam a beacon holds, by name, or `None`.
fn held(world: &World, beacon: BeaconId) -> Option<String> {
    let seam = world.beacons().seams().get(row_of(beacon)).copied()?;
    if seam == NO_FEATURE {
        return None;
    }
    world
        .features()
        .get(usize::try_from(seam).ok()?)
        .map(Feature::name)
}

fn seam_named<'a>(world: &'a World, name: &str) -> (usize, &'a Feature) {
    let index = world
        .features()
        .index_of_name(name)
        .unwrap_or_else(|| panic!("{name} is on the map"));
    (index, world.features().get(index).expect("an index"))
}

/// Play `ticks` ticks of the Push in progress (or to its end).
fn play(runner: &mut Runner, ticks: u32) {
    let mut left = ticks;
    while left > 0 && runner.phase() == MatchPhase::Push {
        let Some(_) = runner.step() else {
            break;
        };
        runner.clear_events();
        left -= 1;
    }
}

/// Play every segment of the match to its end.
fn play_match(runner: &mut Runner) {
    loop {
        assert!(runner.begin_push(), "the Push begins");
        play(runner, u32::MAX);
        if !runner.end_recap() {
            break;
        }
        if runner.phase() != MatchPhase::Lull {
            break;
        }
    }
}

/// Seat 0's core switched to Mine with `settings`, and its starting seam.
fn mining_core(segments: &[i32], settings: MineSettings) -> (World, BeaconId) {
    let mut fixture = world(segments);
    let core = core(&fixture);
    fixture.set_writ(core, MandateKind::Mine);
    fixture.set_mine_settings(core, settings);
    (fixture, core)
}

/// The starting seam of seat 0's core: the seam it holds after its first
/// decision.
fn starting_seam(fixture: &World, core: BeaconId) -> usize {
    let mut runner = Runner::new(fixture.clone());
    assert!(runner.begin_push());
    play(&mut runner, 1);
    let seam = runner
        .world()
        .beacons()
        .seams()
        .get(row_of(core))
        .copied()
        .expect("a row");
    assert_ne!(seam, NO_FEATURE, "the core chose its starting seam");
    usize::try_from(seam).expect("an index")
}

/// Whether the voxel at `at` was ore in `before` and is not in `after`.
fn dug(before: &World, after: &World, at: [i32; 3]) -> bool {
    before
        .voxels()
        .get(at)
        .and_then(Material::ore_richness)
        .is_some()
        && after
            .voxels()
            .get(at)
            .and_then(Material::ore_richness)
            .is_none()
}

#[test]
fn digging_stops_at_the_max_depth() {
    let settings = MineSettings {
        dig_max_depth: 1,
        ..MineSettings::default()
    };
    let (fixture, core) = mining_core(&[180_000, 180_000], settings);
    let pristine = fixture.clone();
    let seam = starting_seam(&fixture, core);
    let mut runner = Runner::new(fixture);
    play_match(&mut runner);
    let after = runner.world();
    let feature = pristine.features().get(seam).expect("the seam");
    let mut at_one: u32 = 0;
    for column in &feature.footprint {
        // Depth 0 and 1 may go; nothing from depth 2 down ever does.
        for depth in 2..6 {
            let at = [column.x, column.y, column.top - depth];
            assert!(
                !dug(&pristine, after, at),
                "{at:?} is {depth} voxels below the seam's original top and the limit is 1"
            );
        }
        if dug(&pristine, after, [column.x, column.y, column.top - 1]) {
            at_one += 1;
        }
    }
    assert!(
        at_one > 0,
        "the limit is reached, not merely respected: something one voxel down was dug"
    );
}

#[test]
fn pillars_stand_at_their_spacing() {
    let settings = MineSettings {
        pillar_spacing: 2,
        ..MineSettings::default()
    };
    let (fixture, core) = mining_core(&[180_000, 180_000], settings);
    let pristine = fixture.clone();
    let seam = starting_seam(&fixture, core);
    let mut runner = Runner::new(fixture);
    play_match(&mut runner);
    let after = runner.world();
    let feature = pristine.features().get(seam).expect("the seam");
    let [ax, ay] = feature.anchor;
    let mut pillars: u32 = 0;
    let mut others_dug: u32 = 0;
    for column in &feature.footprint {
        let pillar = (column.x - ax).rem_euclid(2) == 0 && (column.y - ay).rem_euclid(2) == 0;
        let top = [column.x, column.y, column.top];
        if pillar {
            pillars += 1;
            assert!(
                !dug(&pristine, after, top),
                "({}, {}) is a pillar at spacing 2 from the anchor ({ax}, {ay}) and was dug",
                column.x,
                column.y
            );
        } else if dug(&pristine, after, top) {
            others_dug += 1;
        }
    }
    assert!(pillars > 0, "the seam has pillar columns to keep");
    assert!(others_dug > 0, "the columns between the pillars are worked");
}

#[test]
fn no_voxel_under_a_structure_is_dug() {
    let settings = MineSettings {
        dig_max_depth: 3,
        ..MineSettings::default()
    };
    let (mut fixture, core) = mining_core(&[180_000, 180_000], settings);
    let seam = starting_seam(&fixture, core);
    let feature = fixture.features().get(seam).expect("the seam").clone();
    // A structure on the anchor column, and a beacon of another seat's on a
    // footprint column at the far side of the disc.
    let [ax, ay] = feature.anchor;
    let on_anchor = fixture.standing_point([ax, ay, 0]);
    fixture
        .raise_structure(SEAT, core, StructureKind::Generator, on_anchor)
        .expect("room for a structure");
    let far = feature
        .footprint
        .iter()
        .max_by_key(|column| {
            let dx = i64::from(column.x - ax);
            let dy = i64::from(column.y - ay);
            (dx * dx + dy * dy, column.y, column.x)
        })
        .copied()
        .expect("a footprint");
    let far_at = fixture.standing_point([far.x, far.y, 0]);
    fixture
        .place_beacon_directly(SeatId::new(1), far_at, MandateKind::Build, PRIORITY_NORMAL)
        .expect("room for a beacon");
    let pristine = fixture.clone();
    let mut runner = Runner::new(fixture);
    play_match(&mut runner);
    let after = runner.world();
    let mut kept: u32 = 0;
    let mut others_dug: u32 = 0;
    for column in &feature.footprint {
        let beside = |x: i32, y: i32| (column.x - x).abs() <= 1 && (column.y - y).abs() <= 1;
        let blocked = beside(ax, ay) || beside(far.x, far.y);
        for depth in 0..4 {
            let at = [column.x, column.y, column.top - depth];
            if blocked {
                kept += 1;
                assert!(
                    !dug(&pristine, after, at),
                    "{at:?} is in or beside the column of a live structure or beacon"
                );
            } else if dug(&pristine, after, at) {
                others_dug += 1;
            }
        }
    }
    assert!(kept > 0, "the fixture puts structures over the seam");
    assert!(others_dug > 0, "the rest of the seam is worked");
}

/// The committed table with `beacon.sphere_radius_voxels` widened to 40 and
/// nothing else changed: from [`MIDPOINT_X`], [`MIDPOINT_Y`] a sphere reaches
/// both rich seams whole.
fn wide_rules() -> RulesTable {
    let mut message = rules().message().clone();
    message
        .beacon
        .as_mut()
        .expect("a beacon block")
        .sphere_radius_voxels = 40;
    RulesTable::from_message(&message).expect("the varied table is a table")
}

/// The midpoint fixture's Push, open, with `carved` voxels taken out of the
/// named seams first; then a Mine beacon of seat 0's at the midpoint choosing
/// by `choice` with depth 3 (the whole seam), and an empty treasury so no
/// drone of its own digs while the choice is under test.
fn midpoint(choice: SeamChoice, carved: &[(&str, Option<usize>)]) -> (Runner, BeaconId) {
    let mut fixture = World::new(&WorldConfig {
        match_seed: SEED,
        seats: SEATS,
        units_per_seat: 0,
        rules: wide_rules(),
        match_settings: MatchSettings {
            segment_lengths_ms: vec![180_000],
            round_limit: 1,
        },
    })
    .expect("the rules table describes a map");
    fixture.set_treasury(SEAT, Money::ZERO);
    let mut runner = Runner::new(fixture);
    assert!(runner.begin_push());
    for (name, count) in carved {
        let (seam, _) = seam_named(runner.world(), name);
        carve(&mut runner, seam, *count);
    }
    let world = runner.world_mut();
    let at = world.standing_point([MIDPOINT_X, MIDPOINT_Y, 0]);
    let beacon = world
        .place_beacon_directly(SEAT, at, MandateKind::Mine, PRIORITY_NORMAL)
        .expect("room for a beacon");
    world.set_mine_settings(
        beacon,
        MineSettings {
            dig_max_depth: 3,
            seam_choice: choice,
            ..MineSettings::default()
        },
    );
    world.set_treasury(SEAT, Money::ZERO);
    (runner, beacon)
}

/// Take `count` ore voxels out of `seam` (all of them for `None`), from the top
/// down so none is left hanging, through the voxel phase.
fn carve(runner: &mut Runner, seam: usize, count: Option<usize>) {
    let world = runner.world();
    let feature = world.features().get(seam).expect("a seam").clone();
    let mut voxels: Vec<(i32, i32, i32)> = Vec::new();
    for column in &feature.footprint {
        for depth in 0..4 {
            let z = column.top - depth;
            if world.ore_yield_at([column.x, column.y, z]).is_some() {
                // Top down, then (y, x): a total order.
                voxels.push((-z, column.y, column.x));
            }
        }
    }
    voxels.sort_unstable();
    let take = count.unwrap_or(voxels.len()).min(voxels.len());
    let chosen: Vec<[i32; 3]> = voxels
        .iter()
        .take(take)
        .map(|(z, y, x)| [*x, *y, -*z])
        .collect();
    for chunk in chosen.chunks(32) {
        for at in chunk {
            assert!(runner.world_mut().request_voxel_edit(VoxelEdit::Set {
                at: *at,
                material: Material::AIR,
            }));
        }
        play(runner, 1);
    }
}

/// The yield each of the two seams has left for the beacon at `row`.
fn yields(world: &World, row: usize) -> [(String, i64); 2] {
    let settings = settings_at(world, row).expect("the beacon's settings");
    [WEST, EAST].map(|name| {
        let (index, _) = seam_named(world, name);
        (
            name.to_owned(),
            remaining_yield(world, row, index, settings).expect("a seam and a beacon"),
        )
    })
}

/// The seam of the two that is not `name`.
fn other_than(name: &str) -> &'static str {
    if name == WEST { EAST } else { WEST }
}

#[test]
fn richest_means_richest_remaining() {
    // Untouched, the two seams hold the same ore at the same grade, so RICHEST
    // ties, and a tie goes to the nearer: NEAREST's own pick.
    let (mut runner, beacon) = midpoint(SeamChoice::Nearest, &[]);
    play(&mut runner, 5);
    let nearer = held(runner.world(), beacon).expect("a seam in reach");
    let farther = other_than(&nearer);
    let (mut runner, beacon) = midpoint(SeamChoice::Richest, &[]);
    let [(_, west), (_, east)] = yields(runner.world(), row_of(beacon));
    assert_eq!(west, east, "the fixture starts from a tie");
    play(&mut runner, 5);
    assert_eq!(
        held(runner.world(), beacon).as_deref(),
        Some(nearer.as_str()),
        "a tie on yield goes to the nearer"
    );

    // Five voxels out of the nearer, and RICHEST takes the farther: what
    // counts is what is left, not what the seam was laid with.
    let (mut runner, beacon) = midpoint(SeamChoice::Richest, &[(&nearer, Some(5))]);
    play(&mut runner, 5);
    assert_eq!(
        held(runner.world(), beacon).as_deref(),
        Some(farther),
        "the nearer seam has less left: {:?}",
        yields(runner.world(), row_of(beacon))
    );
    // And five out of the farther instead, and it takes the nearer again.
    let (mut runner, beacon) = midpoint(SeamChoice::Richest, &[(farther, Some(5))]);
    play(&mut runner, 5);
    assert_eq!(
        held(runner.world(), beacon).as_deref(),
        Some(nearer.as_str()),
        "the farther seam has less left: {:?}",
        yields(runner.world(), row_of(beacon))
    );

    // SAFEST reads as NEAREST until S2's threat model (decision 6).
    let (mut runner, beacon) = midpoint(SeamChoice::Safest, &[(&nearer, Some(5))]);
    play(&mut runner, 5);
    assert_eq!(
        held(runner.world(), beacon).as_deref(),
        Some(nearer.as_str())
    );
}

/// The fixture the hold tests share: the nearer seam mined down by five, so
/// RICHEST holds the farther, which is then mined down by ten -- poorer now
/// than the nearer, and far from spent.
fn holding_the_poorer() -> (Runner, BeaconId, String, String) {
    let (mut probe, beacon) = midpoint(SeamChoice::Nearest, &[]);
    play(&mut probe, 5);
    let nearer = held(probe.world(), beacon).expect("a seam in reach");
    let farther = other_than(&nearer).to_owned();
    let (mut runner, beacon) = midpoint(SeamChoice::Richest, &[(&nearer, Some(5))]);
    play(&mut runner, 5);
    assert_eq!(
        held(runner.world(), beacon).as_deref(),
        Some(farther.as_str())
    );
    let (index, _) = seam_named(runner.world(), &farther);
    carve(&mut runner, index, Some(10));
    let [(_, west), (_, east)] = yields(runner.world(), row_of(beacon));
    let (held_left, other_left) = if farther == WEST {
        (west, east)
    } else {
        (east, west)
    };
    assert!(
        held_left > 0 && held_left < other_left,
        "the held seam is now the poorer and not spent: {held_left} against {other_left}"
    );
    (runner, beacon, nearer, farther)
}

#[test]
fn a_held_seam_is_kept_until_spent() {
    let (mut runner, beacon, nearer, farther) = holding_the_poorer();
    play(&mut runner, 20);
    assert_eq!(
        held(runner.world(), beacon).as_deref(),
        Some(farther.as_str()),
        "a held seam is kept while it has work left, however rich the other has become"
    );

    // Spend it: the next decision lets it go and chooses again.
    let (index, _) = seam_named(runner.world(), &farther);
    carve(&mut runner, index, None);
    play(&mut runner, 10);
    assert_eq!(
        held(runner.world(), beacon).as_deref(),
        Some(nearer.as_str()),
        "a spent seam is let go and the beacon chooses again"
    );
}

#[test]
fn a_written_seam_choice_releases_the_held_seam() {
    // A beacon keeps its target "until you change it on site" (targeting.md,
    // "Three reading rules", 2): a settings row that writes `seam_choice` is
    // that change; one that writes only the depth is not.
    let (mut runner, beacon, nearer, farther) = holding_the_poorer();
    runner.world_mut().edit_mine_settings(
        beacon,
        MineEdit {
            dig_max_depth: Some(2),
            ..MineEdit::default()
        },
    );
    play(&mut runner, 5);
    assert_eq!(
        held(runner.world(), beacon).as_deref(),
        Some(farther.as_str()),
        "a depth edit changes no target"
    );
    runner.world_mut().edit_mine_settings(
        beacon,
        MineEdit {
            seam_choice: Some(SeamChoice::Richest),
            ..MineEdit::default()
        },
    );
    assert_eq!(held(runner.world(), beacon), None, "the choice is released");
    play(&mut runner, 5);
    assert_eq!(
        held(runner.world(), beacon).as_deref(),
        Some(nearer.as_str()),
        "and made again, under what is left now"
    );
}

#[test]
fn a_seam_choice_counts_its_estimates_and_a_held_seam_costs_none() {
    // No playbook sealed, so a decision spends nothing but the seam choice:
    // seat 0's core holds no seam until its first decision, which ranks the
    // map's features (one unit each) and estimates the one seam with work
    // for it (one unit).
    let fixture = world(&[180_000]);
    let features = u32::try_from(fixture.features().len()).expect("a few");
    let mut runner = Runner::new(fixture);
    assert!(runner.begin_push());
    play(&mut runner, 1);
    let first = runner.world().decision_work(0).expect("seat 0").spent();
    assert_eq!(
        first,
        features + 1,
        "a choice ranks every feature and estimates its one candidate"
    );
    play(&mut runner, 5);
    assert_eq!(
        runner.world().decision_work(0).expect("seat 0").spent(),
        0,
        "a held seam with work left is kept, and keeping it estimates nothing"
    );
}

#[test]
fn a_spent_beacon_charges_no_ranking() {
    // Once its seam is spent and no other has work for it, a beacon asks again
    // on every decision and finds nothing: with no candidate there is nothing
    // to rank, so nothing is charged.
    let fixture = world(&[180_000]);
    let core = core(&fixture);
    let seam = starting_seam(&fixture, core);
    let mut runner = Runner::new(fixture);
    assert!(runner.begin_push());
    play(&mut runner, 1);
    carve(&mut runner, seam, None);
    play(&mut runner, 5);
    assert_eq!(held(runner.world(), core), None, "the seam is spent");
    assert_eq!(
        runner.world().decision_work(0).expect("seat 0").spent(),
        0,
        "a choice with no candidate charges nothing"
    );
}

fn busy_of(world: &World, unit: usize) -> u32 {
    world
        .units()
        .busy_until()
        .get(unit)
        .copied()
        .expect("a unit")
}

fn carrying_of(world: &World, unit: usize) -> Money {
    world.units().carrying().get(unit).copied().expect("a unit")
}

/// Seat 0's starting mining drone, homed to its core.
fn drone_of(world: &World, core: BeaconId) -> usize {
    let units = world.units();
    units
        .homes()
        .iter()
        .zip(units.kinds())
        .position(|(home, kind)| *home == core.raw() && *kind == UnitKind::MiningDrone.id())
        .expect("the core has a mining drone")
}

#[test]
fn a_second_dig_in_one_seam_box_waits_for_the_first_to_land() {
    // Each dig is judged pit-safe against the world the last voxel phase left,
    // and the queue lands after every program has run, so a dig finishing on a
    // tick another has already queued an edit in the same box waits one tick
    // and is judged with that edit taken out.
    let (fixture, core) = mining_core(&[180_000], MineSettings::default());
    let mut runner = Runner::new(fixture);
    assert!(runner.begin_push());
    let drone = drone_of(runner.world(), core);
    // Play until the drone is one tick from finishing a dig.
    let mut guard = 0;
    loop {
        let world = runner.world();
        let busy = busy_of(world, drone);
        if busy != pharmakos_sim::tables::NO_WORK && busy == world.tick().raw() + 1 {
            break;
        }
        play(&mut runner, 1);
        guard += 1;
        assert!(guard < 4_000, "the drone starts a dig");
    }
    let target = runner
        .world()
        .next_dig(core)
        .expect("the dig it is finishing")
        .ore;
    let feature = {
        let held = runner
            .world()
            .beacons()
            .seams()
            .get(row_of(core))
            .copied()
            .expect("a row");
        let index = usize::try_from(held).expect("a held seam");
        runner
            .world()
            .features()
            .get(index)
            .expect("a seam")
            .clone()
    };
    // Another dig, already queued this tick, in a different column of the box.
    let other = feature
        .footprint
        .iter()
        .find(|column| [column.x, column.y, target[2]] != target)
        .expect("a second column");
    let world = runner.world();
    let other_top = world.voxels().top_solid_z(other.x, other.y).expect("solid");
    let elsewhere = [other.x, other.y, other_top];
    assert!(!pharmakos_sim::mining::dig_waits(world, core));
    let carrying = carrying_of(world, drone);
    assert!(runner.world_mut().request_voxel_edit(VoxelEdit::Set {
        at: elsewhere,
        material: Material::AIR,
    }));
    assert!(pharmakos_sim::mining::dig_waits(runner.world(), core));
    let before = runner.world().clone();
    play(&mut runner, 1);
    assert!(
        !dug(&before, runner.world(), target),
        "the second dig did not land on the tick the first was queued"
    );
    assert_eq!(
        carrying_of(runner.world(), drone),
        carrying,
        "and nothing was credited for it"
    );
    assert_eq!(
        busy_of(runner.world(), drone),
        runner.world().tick().raw() + 1,
        "it is judged again next tick"
    );
    assert!(!pharmakos_sim::mining::dig_waits(runner.world(), core));
    play(&mut runner, 1);
    assert!(
        carrying_of(runner.world(), drone) > carrying
            || busy_of(runner.world(), drone) == pharmakos_sim::tables::NO_WORK,
        "the next tick it digs, or finds its stand gone and walks"
    );
}

#[test]
fn the_mine_mandate_files_its_drone_in_the_mine_band() {
    // A Mine beacon with work and no drone asks for one (item 22), in the
    // Mine band: last on spec section 7's ladder, never in the Units band.
    let (mut runner, beacon) = midpoint(SeamChoice::Nearest, &[]);
    play(&mut runner, 5);
    let mandate = mandate_for(MandateKind::Mine).expect("Mine runs in S1");
    let request = mandate
        .request(runner.world(), beacon)
        .expect("a beacon with work and no drone asks for one");
    assert_eq!(request.urgency, Urgency::Mine, "spec section 7: mine last");
    assert_eq!(
        request.buys,
        pharmakos_sim::Purchase::Unit(UnitKind::MiningDrone)
    );
}

/// A whole playbook around `route`, as canonical `gp.v1` JSON.
fn playbook(route: &str) -> gp::v1::Playbook {
    let text = format!(
        concat!(
            "{{\"schema_version\":{{\"major\":1}},",
            "\"meta\":{{\"title\":\"mining\",\"author_kind\":\"HUMAN\"}},",
            "\"declarative\":{{\"route\":[{route}]}},",
            "\"on_death\":{{\"on_respawn\":\"CONTINUE\",\"max_deaths_before_fallback\":3}},",
            "\"fallback\":{{\"hold\":{{\"at\":{{\"safest\":{{}}}}}}}},",
            "\"kind\":\"PLAYBOOK\"}}"
        ),
        route = route
    );
    json::decode(&text).unwrap_or_else(|error| panic!("the case decodes: {error}\n{text}"))
}

/// Seal `route` for seat 0 and play until its visit ends, returning the world.
fn visit(route: &str) -> World {
    let plan = Plan::compile(&playbook(route), &rules()).expect("compiles");
    let mut runner = Runner::new(world(&[180_000]));
    runner
        .seal_playbook(SEAT, plan)
        .expect("a Lull takes a seal");
    assert!(runner.begin_push());
    let mut ended = false;
    let mut ticks: u32 = 0;
    while !ended && ticks < 3_000 {
        runner.step();
        ended = runner
            .events()
            .iter()
            .any(|event| event.kind == EventKind::VisitEnded && event.seat == Some(SEAT));
        runner.clear_events();
        ticks += 1;
    }
    assert!(ended, "the visit ends");
    runner.into_world()
}

#[test]
fn a_set_mandate_row_writes_the_settings_it_carries() {
    // Item 131 (5): the sim kept a `set_mandate` row's writ and dropped its
    // settings, while the verifier's I0003 read the row as starting from the
    // defaults. The row now writes them over the defaults the switch leaves.
    let switch = concat!(
        r#"{"label":"switch","interface":{"beacon":{"beacon_id":"b_00"},"rows":["#,
        r#"{"set_mandate":{"mine":{"dig_max_depth":2,"seam_choice":"RICHEST","pillar_spacing":3}}}"#,
        r#"]},"timeout_ms":120000}"#
    );
    let after = visit(switch);
    let core = core(&after);
    assert_eq!(
        after.beacons().mandates().get(row_of(core)).copied(),
        Some(MandateKind::Mine.id())
    );
    assert_eq!(
        settings_at(&after, row_of(core)),
        Some(MineSettings {
            dig_max_depth: 2,
            pillar_spacing: 3,
            seam_choice: SeamChoice::Richest,
            flee_on_threat: false,
        })
    );

    // An edit writes what it names and leaves the rest alone; a switch puts
    // everything back to the defaults first.
    let edit = concat!(
        r#"{"label":"switch","interface":{"beacon":{"beacon_id":"b_00"},"rows":["#,
        r#"{"set_mandate":{"mine":{"dig_max_depth":2,"seam_choice":"RICHEST"}}},"#,
        r#"{"set_mandate_settings":{"mine":{"pillar_spacing":5}}},"#,
        r#"{"set_mandate":{"mine":{"flee_on_threat":true}}}"#,
        r#"]},"timeout_ms":120000}"#
    );
    let compiled = Plan::compile(&playbook(edit), &rules()).expect("compiles");
    let after = visit(edit);
    assert_eq!(
        settings_at(&after, row_of(core)),
        Some(MineSettings {
            flee_on_threat: true,
            ..MineSettings::default()
        }),
        "the last row is a switch: it clears what the first two wrote"
    );

    // The switch is priced as the verifier prices it: the switch, then the
    // edit it carries.
    let route = compiled.route();
    let rows = match route.first().map(|step| &step.action) {
        Some(pharmakos_sim::interpreter::Action::Interface { rows, .. }) => rows,
        other => panic!("an interface step: {other:?}"),
    };
    let times = rules().message().interface_times.expect("interface times");
    let card = compiled.interface_times();
    let first = rows.first().expect("a row");
    assert!(matches!(
        first,
        Row::Mandate {
            settings: Some(_),
            ..
        }
    ));
    assert_eq!(
        first.duration_ms(card),
        Ok(times.switch_mandate_ms
            + times.edit_settings_base_ms
            + times.edit_settings_per_field_ms),
        "eight seconds for the switch, and two fields of edit"
    );
    assert!(
        matches!(
            rows.get(2),
            Some(Row::Mandate {
                settings: Some(_),
                ..
            })
        ),
        "a lone `flee_on_threat` is still a field the switch writes"
    );
}

#[test]
fn a_switch_to_an_empty_s2_writ_prices_the_fields_beside_it() {
    // An empty Defend arm writes nothing of its own, but `retreat_hp_pct`
    // beside it is a field of the edit, priced as it is beside any other arm
    // and as the verifier prices it: the switch plus one field.
    let route = concat!(
        r#"{"label":"switch","interface":{"beacon":{"beacon_id":"b_00"},"rows":["#,
        r#"{"set_mandate":{"defend":{},"retreat_hp_pct":50}},"#,
        r#"{"set_mandate":{"defend":{}}}"#,
        r#"]},"timeout_ms":120000}"#
    );
    let compiled = Plan::compile(&playbook(route), &rules()).expect("compiles");
    let rows = match compiled.route().first().map(|step| &step.action) {
        Some(pharmakos_sim::interpreter::Action::Interface { rows, .. }) => rows,
        other => panic!("an interface step: {other:?}"),
    };
    let times = rules().message().interface_times.expect("interface times");
    let card = compiled.interface_times();
    assert_eq!(
        rows.first().expect("a row").duration_ms(card),
        Ok(times.switch_mandate_ms + times.edit_settings_base_ms),
        "the switch and one field"
    );
    assert_eq!(
        rows.get(1).expect("a row").duration_ms(card),
        Ok(times.switch_mandate_ms),
        "an empty arm alone is the switch alone"
    );
}

#[test]
fn a_held_seam_survives_save_and_restore_and_a_vent_is_refused() {
    let (fixture, _) = mining_core(
        &[180_000],
        MineSettings {
            dig_max_depth: 2,
            pillar_spacing: 4,
            seam_choice: SeamChoice::Richest,
            flee_on_threat: true,
        },
    );
    let mut runner = Runner::new(fixture);
    assert!(runner.begin_push());
    play(&mut runner, 600);
    let saved = Snapshot::capture(runner.world());
    let mut resumed = world(&[180_000]);
    saved.restore_into(&mut resumed).expect("the save restores");
    assert_eq!(resumed.state_hash(), runner.world().state_hash());
    assert_eq!(resumed.beacons().seams(), runner.world().beacons().seams());
    assert_eq!(
        resumed.beacons().seam_choices(),
        runner.world().beacons().seam_choices()
    );

    // A held seam naming a vent describes no beacon this sim builds.
    let vent = resumed
        .features()
        .features()
        .iter()
        .position(|feature| feature.kind == FeatureKind::Vent)
        .expect("a vent");
    let mut doctored = saved.clone();
    if let Some(slot) = doctored.beacon_seam.first_mut() {
        *slot = u32::try_from(vent).unwrap();
    }
    assert_eq!(
        doctored.restore_into(&mut world(&[180_000])),
        Err(SnapshotError::Ragged("beacon_seam"))
    );
    let mut doctored = saved.clone();
    if let Some(slot) = doctored.beacon_seam_choice.first_mut() {
        *slot = 0;
    }
    assert_eq!(
        doctored.restore_into(&mut world(&[180_000])),
        Err(SnapshotError::Ragged("beacon"))
    );
    // A flag is a 0 or a 1: any other byte would restore as `true` and re-save
    // as 1, a different file from the one restored, so it is refused.
    let mut doctored = saved;
    if let Some(slot) = doctored.beacon_flee.first_mut() {
        *slot = 2;
    }
    assert_eq!(
        doctored.restore_into(&mut world(&[180_000])),
        Err(SnapshotError::Ragged("beacon_flee"))
    );
}
