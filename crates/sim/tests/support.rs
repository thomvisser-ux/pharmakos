// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! S2's `fall`: the support rule (S2's plan, task `fall`; its Acceptance list,
//! test by test, under the names the plan gives them).
//!
//! Generated terrain has no overhangs and every column is solid to bedrock, so
//! no ordinary play detaches a region (decisions-log item 144 (11), decision
//! 39). Every geometry here is therefore **staged** through the host's test
//! seams: [`World::request_voxel_edit`] builds a shelf or a bridge, and the
//! fixture API puts assets on it. The numbers are the rules table's
//! proposals (decision 11), read from the table rather than pinned.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules, not an integration test's helpers. A panic is this file's failure report, an out-of-bounds index in an assertion among them: these are panic lints, not determinism lints (AGENTS.md section 5)."
)]

use pharmakos_sim::encoding::Enc;
use pharmakos_sim::events::{Event, EventKind};
use pharmakos_sim::knowledge::AssetId;
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::math::quantity::Damage;
use pharmakos_sim::mining::MineSettings;
use pharmakos_sim::runner::Runner;
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::snapshot::Snapshot;
use pharmakos_sim::tables::{
    BeaconId, Common, PRIORITY_NORMAL, SeatId, StructureId, StructureKind, UnitId, UnitKind,
    WreckId,
};
use pharmakos_sim::voxels::{Material, VoxelEdit};
use pharmakos_sim::world::support::CollapseSkip;
use pharmakos_sim::world::{DamageOrder, DamageTarget, World, WorldConfig};
use pharmakos_sim::{MatchSettings, RulesTable, default_rules_path};
use std::path::PathBuf;

const SEED: u64 = 0x00F1_2E00_C0DE_0001;

/// `MandateSettings.Roe`'s `HOLD_FIRE`: the staged mortars fire only when told.
const HOLD_FIRE: u8 = 1;

fn rules() -> RulesTable {
    let path = default_rules_path().unwrap_or_else(|| PathBuf::from("rules/rules.v1.json"));
    RulesTable::load(&path).unwrap_or_else(|error| panic!("loading the rules table: {error}"))
}

fn edited(edit: impl FnOnce(&mut pharmakos_proto::gp::v1::RulesTable)) -> RulesTable {
    let mut message = rules().message().clone();
    edit(&mut message);
    RulesTable::from_message(&message).unwrap_or_else(|error| panic!("the edited table: {error}"))
}

fn world_with(rules: RulesTable, seats: u32, segment_ms: i32) -> World {
    World::new(&WorldConfig {
        match_seed: SEED,
        seats,
        units_per_seat: 0,
        rules,
        match_settings: MatchSettings {
            segment_lengths_ms: vec![segment_ms],
            round_limit: 3,
        },
    })
    .unwrap_or_else(|error| panic!("generating the world: {error}"))
}

fn world(seats: u32) -> World {
    world_with(rules(), seats, 600_000)
}

fn seat(raw: u8) -> SeatId {
    SeatId::new(raw)
}

fn fall_damage() -> i32 {
    i32::try_from(
        rules()
            .message()
            .collapse
            .as_ref()
            .expect("a block")
            .fall_damage_hp,
    )
    .expect("a damage")
}

fn core(world: &World, of: SeatId) -> BeaconId {
    world
        .beacons()
        .ids()
        .iter()
        .zip(world.beacons().seats())
        .filter(|(_, owner)| **owner == of.raw())
        .map(|(id, _)| *id)
        .min()
        .map(BeaconId::new)
        .expect("a core")
}

fn beacon_at(world: &World, beacon: BeaconId) -> [Fx; 3] {
    let row = world.beacons().row_of(beacon).expect("a live beacon");
    world.beacons().positions()[row.index()]
}

fn column(point: [Fx; 3]) -> [i32; 2] {
    [point[0].floor_voxels(), point[1].floor_voxels()]
}

fn point(x: i32, y: i32, z: i32) -> [Fx; 3] {
    [x, y, z].map(|v| Fx::from_voxels(i16::try_from(v).expect("a coordinate")))
}

fn top(world: &World, x: i32, y: i32) -> i32 {
    world.voxels().top_solid_z(x, y).expect("a solid column")
}

fn set(at: [i32; 3], material: Material, by: Option<SeatId>) -> VoxelEdit {
    VoxelEdit::Set { at, material, by }
}

/// Step a world outside a Push.
fn tick(world: &mut World) {
    let mut enc = Enc::with_capacity(64 * 1024);
    world.step(&mut enc).expect("the tick runs");
    world.clear_events();
}

/// File `edits` between ticks, stepping the world whenever the host's room
/// is full, and step once more so the last of them land.
fn build(world: &mut World, edits: &[VoxelEdit]) {
    for edit in edits {
        if world.request_voxel_edit(*edit).is_err() {
            tick(world);
            world.request_voxel_edit(*edit).expect("room after a tick");
        }
    }
    tick(world);
}

/// A shelf: a square slab of `2r + 1` columns at the top of the column
/// `(cx, cy)`, undercut by `under` layers and ringed by a moat one column
/// wide at its own height and above, joined to the ground only through the
/// necks, each one voxel of the moat at the slab's height.
#[derive(Clone, Debug)]
struct Shelf {
    cx: i32,
    cy: i32,
    t: i32,
    r: i32,
    necks: Vec<[i32; 3]>,
}

impl Shelf {
    /// The standing point on the shelf at `(dx, dy)` from its centre.
    fn spot(&self, dx: i32, dy: i32) -> [Fx; 3] {
        point(self.cx + dx, self.cy + dy, self.t + 1)
    }

    fn voxels(&self) -> usize {
        usize::try_from((2 * self.r + 1) * (2 * self.r + 1)).expect("a count")
    }
}

fn shelf(world: &mut World, centre: [i32; 2], r: i32, under: i32, necks: &[[i32; 2]]) -> Shelf {
    let [cx, cy] = centre;
    let t = top(world, cx, cy);
    let mut edits = Vec::new();
    // The slab, its undercut and the air above it.
    for dx in -r..=r {
        for dy in -r..=r {
            let (x, y) = (cx + dx, cy + dy);
            for z in (t + 1)..=top(world, x, y).max(t) {
                edits.push(set([x, y, z], Material::AIR, None));
            }
            edits.push(set([x, y, t], Material::DIRT, None));
            for z in (t - under)..t {
                edits.push(set([x, y, z], Material::AIR, None));
            }
        }
    }
    // The necks, filled up to the slab's height and topped with dirt.
    let mut neck_voxels = Vec::new();
    for [dx, dy] in necks {
        let (x, y) = (cx + dx, cy + dy);
        let natural = top(world, x, y);
        for z in (natural + 1)..t {
            edits.push(set([x, y, z], Material::DIRT, None));
        }
        for z in (t + 1)..=natural.max(t) {
            edits.push(set([x, y, z], Material::AIR, None));
        }
        edits.push(set([x, y, t], Material::DIRT, None));
        neck_voxels.push([x, y, t]);
    }
    // The moat: every other ring column cleared from the slab's height up.
    let ring = r + 1;
    for dx in -ring..=ring {
        for dy in -ring..=ring {
            if dx.abs() != ring && dy.abs() != ring {
                continue;
            }
            if necks.contains(&[dx, dy]) {
                continue;
            }
            let (x, y) = (cx + dx, cy + dy);
            for z in t..=top(world, x, y).max(t) {
                edits.push(set([x, y, z], Material::AIR, None));
            }
        }
    }
    build(world, &edits);
    Shelf {
        cx,
        cy,
        t,
        r,
        necks: neck_voxels,
    }
}

/// A place far from every core: the midpoint of the first two seats' cores.
fn field(world: &World) -> [i32; 2] {
    let [ax, ay] = column(beacon_at(world, core(world, seat(0))));
    let [bx, by] = column(beacon_at(world, core(world, seat(1))));
    [i32::midpoint(ax, bx), i32::midpoint(ay, by)]
}

/// How many solid voxels inside the box `min..=max` are not joined, through
/// solid faces inside the box, to a voxel whose column is solid from the
/// bottom up to it: the test's own oracle for "nothing floats".
fn floating(world: &World, min: [i32; 3], max: [i32; 3]) -> usize {
    let voxels = world.voxels();
    let size = [
        max[0] - min[0] + 1,
        max[1] - min[1] + 1,
        max[2] - min[2] + 1,
    ];
    let at_index = |at: [i32; 3]| -> Option<usize> {
        let local = [at[0] - min[0], at[1] - min[1], at[2] - min[2]];
        if (0..3).any(|axis| local[axis] < 0 || local[axis] >= size[axis]) {
            return None;
        }
        usize::try_from(local[0] + size[0] * (local[1] + size[1] * local[2])).ok()
    };
    let total = usize::try_from(size[0] * size[1] * size[2]).expect("a box");
    let mut grounded = vec![false; total];
    let mut queue = Vec::new();
    let solid = |at: [i32; 3]| voxels.get(at).is_some_and(Material::is_solid);
    for x in min[0]..=max[0] {
        for y in min[1]..=max[1] {
            let mut z = 0;
            while solid([x, y, z]) {
                if z >= min[2]
                    && let Some(index) = at_index([x, y, z])
                {
                    grounded[index] = true;
                    queue.push([x, y, z]);
                }
                z += 1;
            }
        }
    }
    while let Some(at) = queue.pop() {
        for step in [
            [0, 0, -1],
            [1, 0, 0],
            [-1, 0, 0],
            [0, 1, 0],
            [0, -1, 0],
            [0, 0, 1],
        ] {
            let next = [at[0] + step[0], at[1] + step[1], at[2] + step[2]];
            if let Some(index) = at_index(next)
                && !grounded[index]
                && solid(next)
            {
                grounded[index] = true;
                queue.push(next);
            }
        }
    }
    let mut count = 0;
    for x in min[0]..=max[0] {
        for y in min[1]..=max[1] {
            for z in min[2]..=max[2] {
                if solid([x, y, z]) && !at_index([x, y, z]).is_some_and(|index| grounded[index]) {
                    count += 1;
                }
            }
        }
    }
    count
}

fn count(feed: &[Event], kind: EventKind) -> usize {
    feed.iter().filter(|event| event.kind == kind).count()
}

fn pushing(world: World) -> Runner {
    let mut runner = Runner::new(world);
    assert_eq!(runner.begin_push(), Ok(true), "the Push begins");
    runner
}

fn play(runner: &mut Runner, ticks: u32, feed: &mut Vec<Event>) {
    for _ in 0..ticks {
        let report = runner.step().expect("the tick runs");
        assert!(report.is_some(), "still in the Push");
        feed.extend_from_slice(runner.events());
        runner.clear_events();
    }
}

fn unit_hp(world: &World, id: UnitId) -> i32 {
    world
        .units()
        .row_of(id)
        .map_or(0, |row| world.units().hit_points()[row.index()].raw())
}

fn unit_at(world: &World, id: UnitId) -> [Fx; 3] {
    let row = world.units().row_of(id).expect("a unit");
    world.units().positions()[row.index()]
}

fn structure_hp(world: &World, id: StructureId) -> i32 {
    world
        .structures()
        .row_of(id)
        .map_or(0, |row| world.structures().hit_points()[row.index()].raw())
}

fn beacon_hp(world: &World, id: BeaconId) -> i32 {
    world
        .beacons()
        .row_of(id)
        .map_or(0, |row| world.beacons().hit_points()[row.index()].raw())
}

/// A seat-1 build drone on the shelf at `(dx, dy)`, homed to seat 1's core.
fn drone_on(world: &mut World, shelf: &Shelf, dx: i32, dy: i32) -> UnitId {
    let home = core(world, seat(1));
    world
        .field_unit(seat(1), home, UnitKind::BuildDrone, shelf.spot(dx, dy))
        .expect("a drone")
}

fn kill_credits(feed: &[Event], subject: AssetId) -> Vec<(SeatId, i64)> {
    feed.iter()
        .filter(|event| event.kind == EventKind::KillCredited && event.subject == Some(subject))
        .filter_map(|event| event.seat.map(|seat| (seat, event.value)))
        .collect()
}

// ---------------------------------------------------------------------------
// The search
// ---------------------------------------------------------------------------

#[test]
fn a_region_cut_off_by_a_crater_falls() {
    let mut world = world(2);
    let centre = field(&world);
    let shelf = shelf(&mut world, centre, 1, 2, &[[2, 0]]);
    assert_eq!(
        world.collapse_counts().regions,
        0,
        "the staging dropped nothing"
    );
    let neck = shelf.necks[0];
    world
        .request_voxel_edit(VoxelEdit::Crater {
            centre: neck,
            radius: 0,
            by: Some(seat(0)),
        })
        .expect("filed");
    let mut enc = Enc::with_capacity(64 * 1024);
    world.step(&mut enc).expect("runs");
    let collapsed: Vec<&Event> = world
        .events()
        .iter()
        .filter(|event| event.kind == EventKind::RegionCollapsed)
        .collect();
    assert_eq!(collapsed.len(), 1, "one region fell");
    assert_eq!(collapsed[0].value, 9, "the nine voxels of the slab");
    assert_eq!(world.collapse_counts().regions, 1);
    let t = shelf.t;
    for dx in -1..=1 {
        for dy in -1..=1 {
            let (cx, cy) = (shelf.cx + dx, shelf.cy + dy);
            assert_eq!(
                world.voxels().get([cx, cy, t]),
                Some(Material::AIR),
                "it left"
            );
            assert_eq!(
                world.voxels().get([cx, cy, t - 2]),
                Some(Material::DIRT),
                "and landed two below, on the ground under its undercut"
            );
        }
    }
    let r = shelf.r + 1;
    assert_eq!(
        floating(
            &world,
            [shelf.cx - r, shelf.cy - r, t - 4],
            [shelf.cx + r, shelf.cy + r, t + 2]
        ),
        0,
        "nothing floats"
    );
}

#[test]
fn ground_beside_a_crater_is_supported_in_one_step() {
    // A crater in generated ground: every seed's column is solid from the
    // bottom up to it, so each search ends at its first voxel.
    let mut world = world(2);
    let [x, y] = field(&world);
    let z = top(&world, x, y);
    let centre = [x, y, z];
    let radius = 2;
    let before = world.clone();
    world
        .request_voxel_edit(VoxelEdit::Crater {
            centre,
            radius,
            by: Some(seat(0)),
        })
        .expect("filed");
    tick(&mut world);
    // The seeds, counted from outside: the solid face-neighbours of every
    // voxel the blast turned to air.
    let mut seeds = std::collections::BTreeSet::new();
    for dx in -radius..=radius {
        for dy in -radius..=radius {
            for dz in -radius..=radius {
                let at = [x + dx, y + dy, z + dz];
                let was = before.voxels().get(at).is_some_and(Material::is_solid);
                let now = world.voxels().get(at).is_some_and(Material::is_solid);
                if !was || now {
                    continue;
                }
                for step in [
                    [0, 0, -1],
                    [1, 0, 0],
                    [-1, 0, 0],
                    [0, 1, 0],
                    [0, -1, 0],
                    [0, 0, 1],
                ] {
                    let next = [at[0] + step[0], at[1] + step[1], at[2] + step[2]];
                    if world.voxels().get(next).is_some_and(Material::is_solid) {
                        seeds.insert(next);
                    }
                }
            }
        }
    }
    assert!(!seeds.is_empty(), "the crater removed something");
    let counts = world.collapse_counts();
    assert_eq!(counts.regions, 0, "nothing fell");
    assert_eq!(
        counts.last_visits,
        seeds.len(),
        "one visit a seed, and no more"
    );
}

#[test]
fn a_region_above_another_lands_on_it_and_does_not_float() {
    // A shelf and a bar hung over it from a pillar beyond the moat; both cut
    // on one tick. The shelf falls first (its lowest voxel is lower), and the
    // bar lands on it where it came to rest.
    let mut world = world(2);
    let centre = field(&world);
    let shelf = shelf(&mut world, centre, 1, 2, &[[0, 2]]);
    let (cx, cy, t) = (shelf.cx, shelf.cy, shelf.t);
    let high = t + 4;
    let mut edits = Vec::new();
    // Clear everything over the area above the slab's height, the pillar's
    // column aside.
    for dx in -4..=4 {
        for dy in -4..=4 {
            let (x, y) = (cx + dx, cy + dy);
            for z in (t + 1)..=(high + 2).max(top(&world, x, y)) {
                if [dx, dy] != [3, 0] {
                    edits.push(set([x, y, z], Material::AIR, None));
                }
            }
        }
    }
    // The pillar at (cx + 3, cy), up to the bar's height, and the bar from
    // it westward over the slab.
    let natural = top(&world, cx + 3, cy);
    for z in (natural + 1)..=high {
        edits.push(set([cx + 3, cy, z], Material::STONE, None));
    }
    for x in (cx - 1)..=(cx + 2) {
        edits.push(set([x, cy, high], Material::DIRT, None));
    }
    build(&mut world, &edits);
    assert_eq!(
        world.collapse_counts().regions,
        0,
        "the staging dropped nothing"
    );
    // Cut both: the bar over the moat, and the shelf's neck.
    world
        .request_voxel_edit(set([cx + 2, cy, high], Material::AIR, Some(seat(0))))
        .expect("filed");
    world
        .request_voxel_edit(set(shelf.necks[0], Material::AIR, Some(seat(0))))
        .expect("filed");
    tick(&mut world);
    assert_eq!(world.collapse_counts().regions, 2, "both fell");
    // The slab came to rest two below; the bar on top of it.
    assert_eq!(world.voxels().get([cx, cy, t - 2]), Some(Material::DIRT));
    for x in (cx - 1)..=(cx + 1) {
        assert_eq!(
            world.voxels().get([x, cy, t - 1]),
            Some(Material::DIRT),
            "the bar lies on the slab at {x}"
        );
    }
    assert_eq!(
        floating(&world, [cx - 4, cy - 4, t - 4], [cx + 4, cy + 4, high + 2]),
        0,
        "nothing floats"
    );
}

#[test]
fn a_region_larger_than_the_bound_holds() {
    let rules = edited(|table| {
        table.collapse.as_mut().expect("a block").max_region_voxels = 8;
    });
    let mut world = world_with(rules, 2, 600_000);
    let centre = field(&world);
    let shelf = shelf(&mut world, centre, 1, 2, &[[2, 0]]);
    let skipped = world.collapse_counts().skipped(CollapseSkip::RegionBound);
    world
        .request_voxel_edit(set(shelf.necks[0], Material::AIR, Some(seat(0))))
        .expect("filed");
    tick(&mut world);
    let counts = world.collapse_counts();
    assert_eq!(counts.regions, 0, "nine voxels are past a bound of eight");
    assert_eq!(
        counts.skipped(CollapseSkip::RegionBound),
        skipped + 1,
        "and it says why"
    );
    assert_eq!(
        world.voxels().get([shelf.cx, shelf.cy, shelf.t]),
        Some(Material::DIRT),
        "the slab holds where it was"
    );
}

#[test]
fn the_tick_budget_labels_the_rest_supported_and_counts_them() {
    // A budget smaller than the slab: the search runs out, its seed and every
    // seed after it are labelled supported and counted, and nothing is
    // carried to the next tick.
    let rules = edited(|table| {
        table
            .collapse
            .as_mut()
            .expect("a block")
            .max_visits_per_tick = 4;
    });
    let mut world = world_with(rules, 2, 600_000);
    let centre = field(&world);
    let shelf = shelf(&mut world, centre, 1, 2, &[[2, 0], [-2, 0]]);
    let skipped = world.collapse_counts().skipped(CollapseSkip::TickBudget);
    for neck in &shelf.necks {
        world
            .request_voxel_edit(set(*neck, Material::AIR, Some(seat(0))))
            .expect("filed");
    }
    tick(&mut world);
    let counts = world.collapse_counts();
    assert_eq!(
        counts.regions, 0,
        "the budget never reached the region's end"
    );
    assert!(
        counts.last_visits <= 4,
        "the budget held: {}",
        counts.last_visits
    );
    assert!(
        counts.skipped(CollapseSkip::TickBudget) > skipped,
        "the seeds it could not search are counted"
    );
    tick(&mut world);
    assert_eq!(
        world.collapse_counts().regions,
        0,
        "and nothing is pending next tick"
    );
    assert_eq!(world.collapse_counts().last_visits, 0);
}

#[test]
fn no_voxel_is_searched_twice_in_a_tick() {
    // Two necks cut on one tick seed the same region from two sides; the
    // second search finds the region labelled and stops.
    let mut world = world(2);
    let centre = field(&world);
    let shelf = shelf(&mut world, centre, 2, 2, &[[3, 0], [-3, 0]]);
    let mut removed = Vec::new();
    for neck in &shelf.necks {
        world
            .request_voxel_edit(set(*neck, Material::AIR, Some(seat(0))))
            .expect("filed");
        removed.push(*neck);
    }
    let mut outside = std::collections::BTreeSet::new();
    for at in &removed {
        for step in [
            [0, 0, -1],
            [1, 0, 0],
            [-1, 0, 0],
            [0, 1, 0],
            [0, -1, 0],
            [0, 0, 1],
        ] {
            let next = [at[0] + step[0], at[1] + step[1], at[2] + step[2]];
            let on_shelf =
                (next[0] - shelf.cx).abs() <= shelf.r && (next[1] - shelf.cy).abs() <= shelf.r;
            if !on_shelf && world.voxels().get(next).is_some_and(Material::is_solid) {
                outside.insert(next);
            }
        }
    }
    tick(&mut world);
    let counts = world.collapse_counts();
    assert_eq!(counts.regions, 1, "one region, seeded twice");
    assert_eq!(
        counts.last_visits,
        shelf.voxels() + outside.len(),
        "each voxel of the slab once, and each grounded seed once"
    );
}

#[test]
fn the_bore_never_falls() {
    // A crater on a core's column: the bore is skipped by the blast and is
    // never detachable, so the core stands where it stood.
    let mut world = world(2);
    let core = core(&world, seat(1));
    let at = beacon_at(&world, core);
    let [x, y] = column(at);
    let z = top(&world, x, y);
    let regions = world.collapse_counts().regions;
    for depth in [1, 4] {
        world
            .request_voxel_edit(VoxelEdit::Crater {
                centre: [x, y, z - depth],
                radius: 3,
                by: Some(seat(0)),
            })
            .expect("filed");
        tick(&mut world);
    }
    assert_eq!(world.collapse_counts().regions, regions, "nothing fell");
    assert_eq!(beacon_at(&world, core), at, "the core did not move");
    for zz in 0..=z {
        assert!(
            world
                .voxels()
                .get([x, y, zz])
                .is_some_and(Material::is_solid),
            "the bore holds at {zz}"
        );
    }
}

#[test]
fn the_bore_holds_what_only_it_supports() {
    // The bore's own branch of the rule: a core's column hollowed below its
    // top (through the host's seam, which no blast can do) and cleared around,
    // so its top voxels touch nothing but the bore below them. They stand.
    let mut world = world(2);
    let core = core(&world, seat(1));
    let at = beacon_at(&world, core);
    let [x, y] = column(at);
    let z = top(&world, x, y);
    let mut edits = vec![set([x, y, z - 3], Material::AIR, None)];
    for dx in -1..=1 {
        for dy in -1..=1 {
            if [dx, dy] == [0, 0] {
                continue;
            }
            let (cx, cy) = (x + dx, y + dy);
            for zz in (z - 3)..=top(&world, cx, cy).max(z) {
                edits.push(set([cx, cy, zz], Material::AIR, None));
            }
        }
    }
    let regions = world.collapse_counts().regions;
    build(&mut world, &edits);
    assert_eq!(
        world.collapse_counts().regions,
        regions,
        "the bore held its top"
    );
    for zz in (z - 2)..=z {
        assert!(
            world
                .voxels()
                .get([x, y, zz])
                .is_some_and(Material::is_solid),
            "the bore voxel at {zz} stands on the bore alone"
        );
    }
    assert_eq!(beacon_at(&world, core), at, "and the core with it");
}

#[test]
fn a_voxel_set_into_air_falls_and_an_attached_overhang_holds() {
    // The rule is "a detached region falls", whatever detached it: a voxel an
    // edit writes into open air (a terraform fill, a wall voxel) falls, and
    // one that overhangs from support holds.
    let mut world = world(2);
    let [cx, cy] = field(&world);
    let t = top(&world, cx, cy);
    let mut edits = Vec::new();
    for dx in -1..=4 {
        for zz in (t + 1)..=(t + 7).max(top(&world, cx + dx, cy)) {
            edits.push(set([cx + dx, cy, zz], Material::AIR, None));
        }
    }
    build(&mut world, &edits);
    let regions = world.collapse_counts().regions;
    world
        .request_voxel_edit(set([cx, cy, t + 5], Material::DIRT, Some(seat(0))))
        .expect("filed");
    tick(&mut world);
    assert_eq!(world.collapse_counts().regions, regions + 1, "it fell");
    assert_eq!(world.voxels().get([cx, cy, t + 5]), Some(Material::AIR));
    assert_eq!(
        world.voxels().get([cx, cy, t + 1]),
        Some(Material::DIRT),
        "onto the ground"
    );
    // A pillar, and a voxel set beside its top: an overhang, which holds.
    let mut pillar = Vec::new();
    for zz in (top(&world, cx + 3, cy) + 1)..=(t + 5) {
        pillar.push(set([cx + 3, cy, zz], Material::STONE, None));
    }
    build(&mut world, &pillar);
    world
        .request_voxel_edit(set([cx + 2, cy, t + 5], Material::DIRT, Some(seat(0))))
        .expect("filed");
    tick(&mut world);
    assert_eq!(
        world.collapse_counts().regions,
        regions + 1,
        "the overhang held"
    );
    assert_eq!(
        world.voxels().get([cx + 2, cy, t + 5]),
        Some(Material::DIRT)
    );
}

#[test]
fn a_region_that_lands_on_a_units_head_stands_it_on_top() {
    // A block over a raider comes to rest one voxel up, on the higher column
    // beside it, so it fills the raider's head cell and not its feet: the
    // raider is under it all the same, takes the flat damage, and stands on
    // the block (decision 11).
    let mut world = world(2);
    assert_eq!(
        world.rules().headroom_voxels(),
        2,
        "a unit stands two cells tall"
    );
    let [cx, cy] = field(&world);
    let t = top(&world, cx, cy);
    let high = t + 6;
    let mut edits = Vec::new();
    // The raider's column topped at t, its neighbour east at t + 1, both clear
    // above; the pillar west up to the block's height.
    for zz in (t + 1)..=(high + 1).max(top(&world, cx, cy)) {
        edits.push(set([cx, cy, zz], Material::AIR, None));
    }
    let east = top(&world, cx + 1, cy);
    for zz in (east + 1)..=(t + 1) {
        edits.push(set([cx + 1, cy, zz], Material::DIRT, None));
    }
    for zz in (t + 2)..=(high + 1).max(east) {
        edits.push(set([cx + 1, cy, zz], Material::AIR, None));
    }
    let west = top(&world, cx - 1, cy);
    for zz in (west + 1)..=high {
        edits.push(set([cx - 1, cy, zz], Material::STONE, None));
    }
    for zz in (high + 1)..=(high + 1).max(west) {
        edits.push(set([cx - 1, cy, zz], Material::AIR, None));
    }
    edits.push(set([cx, cy, high], Material::DIRT, None));
    edits.push(set([cx + 1, cy, high], Material::DIRT, None));
    build(&mut world, &edits);
    let core1 = core(&world, seat(1));
    let raider = world
        .field_unit(seat(1), core1, UnitKind::Raider, point(cx, cy, t + 1))
        .expect("a raider");
    let before = unit_hp(&world, raider);
    world
        .request_voxel_edit(set([cx - 1, cy, high], Material::AIR, Some(seat(0))))
        .expect("filed");
    tick(&mut world);
    assert_eq!(
        world.voxels().get([cx, cy, t + 2]),
        Some(Material::DIRT),
        "the block rests a voxel up"
    );
    assert_eq!(
        world.voxels().get([cx, cy, t + 1]),
        Some(Material::AIR),
        "over the raider's feet"
    );
    assert_eq!(
        unit_at(&world, raider)[2],
        Fx::from_voxels(i16::try_from(t + 3).expect("a z")),
        "it stands on the block"
    );
    assert_eq!(unit_hp(&world, raider), (before - fall_damage()).max(0));
}

// ---------------------------------------------------------------------------
// What falls with it, and who is credited
// ---------------------------------------------------------------------------

/// Walk seat 1's commander a few columns off its core and build a shelf
/// under where it stands: the commander, a raider, a wreck and a ruin on it,
/// ready to be cut by `by`'s edit at the necks. No live structure or beacon
/// stands on it: a building's footing is anchored ground (the owner's ruling
/// of 2026-10-20, S2's `site`), so a shelf carrying one would not detach; a
/// ruin is no footing, and falls.
struct Loaded {
    runner: Runner,
    shelf: Shelf,
    commander: UnitId,
    raider: UnitId,
    ruin: StructureId,
    wreck: [Fx; 3],
}

fn loaded() -> Loaded {
    let mut world = world(2);
    let core1 = core(&world, seat(1));
    let commander = world.commander_of(seat(1)).expect("a commander");
    let [kx, ky] = column(beacon_at(&world, core1));
    let goal = [kx + 9, ky];
    let goal_point = world.standing_point([goal[0], goal[1], 0]);
    let row = world.units().row_of(commander).expect("a row").raw();
    world.send_unit_directly(row, goal_point);
    let mut walked = 0;
    while column(unit_at(&world, commander)) != goal {
        tick(&mut world);
        walked += 1;
        assert!(walked < 4_000, "the commander walks to its spot");
    }
    // Let it settle on the column's centre.
    for _ in 0..40 {
        tick(&mut world);
    }
    let shelf = shelf(&mut world, goal, 2, 2, &[[0, 3], [0, -3]]);
    // Put it back on its standing point, in case the build moved anything.
    let raider = world
        .field_unit(seat(1), core1, UnitKind::Raider, shelf.spot(-2, -2))
        .expect("a raider");
    // A beacon off the shelf, whose structure on it becomes a ruin.
    let doomed = world
        .place_beacon_directly(
            seat(1),
            world.standing_point([kx + 2, ky + 6, 0]),
            MandateKind::Build,
            PRIORITY_NORMAL,
        )
        .expect("a second beacon");
    let ruin = world
        .raise_structure(seat(1), doomed, StructureKind::Generator, shelf.spot(-2, 2))
        .expect("a structure to ruin");
    // A drone killed on the shelf leaves a wreck there.
    let doomed_drone = drone_on(&mut world, &shelf, 0, 2);
    world
        .request_damage(DamageOrder {
            target: DamageTarget::Unit(doomed_drone),
            amount: Damage::new(1_000).expect("damage"),
            by: seat(0),
            source: None,
        })
        .expect("filed");
    world
        .request_damage(DamageOrder {
            target: DamageTarget::Beacon(doomed),
            amount: Damage::new(1_000_000).expect("damage"),
            by: seat(0),
            source: None,
        })
        .expect("filed");
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    play(&mut runner, 1, &mut feed);
    assert_eq!(
        count(&feed, EventKind::WreckLeft),
        1,
        "a wreck on the shelf"
    );
    assert!(count(&feed, EventKind::StructureRuined) >= 1, "and a ruin");
    let wreck = runner.world().wrecks().positions()[0];
    assert_eq!(column(wreck), [shelf.cx, shelf.cy + 2]);
    Loaded {
        runner,
        shelf,
        commander,
        raider,
        ruin,
        wreck,
    }
}

#[test]
fn everything_on_a_falling_region_falls_and_takes_the_flat_damage_once() {
    let Loaded {
        mut runner,
        shelf,
        commander,
        raider,
        ruin,
        wreck,
    } = loaded();
    let fall = fall_damage();
    let world = runner.world();
    assert_eq!(
        column(unit_at(world, commander)),
        [shelf.cx, shelf.cy],
        "on the shelf"
    );
    let before = (
        unit_hp(world, commander),
        unit_hp(world, raider),
        structure_hp(world, ruin),
    );
    let heights = (unit_at(world, commander)[2], wreck[2]);
    for neck in shelf.necks.clone() {
        runner
            .world_mut()
            .request_voxel_edit(set(neck, Material::AIR, Some(seat(0))))
            .expect("filed");
    }
    let mut feed = Vec::new();
    play(&mut runner, 1, &mut feed);
    assert_eq!(count(&feed, EventKind::RegionCollapsed), 1);
    assert_eq!(
        count(&feed, EventKind::AssetFell),
        4,
        "the commander, the raider, the ruin and the wreck"
    );
    let world = runner.world();
    let two = Fx::from_voxels(2);
    assert_eq!(
        unit_at(world, commander)[2],
        heights.0.checked_sub(two).unwrap()
    );
    assert_eq!(
        world.wrecks().positions()[0][2],
        heights.1.checked_sub(two).unwrap(),
        "the wreck rode down"
    );
    assert_eq!(unit_hp(world, commander), before.0 - fall);
    assert_eq!(unit_hp(world, raider), (before.1 - fall).max(0));
    assert_eq!(structure_hp(world, ruin), (before.2 - fall).max(0));
    // Once a tick, and never again: nothing more falls.
    let after = (unit_hp(world, commander), structure_hp(world, ruin));
    let mut more = Vec::new();
    play(&mut runner, 5, &mut more);
    assert_eq!(count(&more, EventKind::AssetFell), 0);
    let world = runner.world();
    assert_eq!(
        (unit_hp(world, commander), structure_hp(world, ruin)),
        after
    );
    assert_eq!(world.event_bus().dropped(), 0, "no event was dropped");
}

/// A shelf near the field with one seat-1 drone on it, in a Push.
fn shelf_with_drone(seats: u32, necks: &[[i32; 2]]) -> (Runner, Shelf, UnitId) {
    let mut world = world(seats);
    let centre = field(&world);
    let shelf = shelf(&mut world, centre, 2, 2, necks);
    let drone = drone_on(&mut world, &shelf, 0, 0);
    (pushing(world), shelf, drone)
}

#[test]
fn the_crater_seat_gets_the_kill_credit_and_an_own_crater_credits_nobody() {
    for (cutter, credited) in [(seat(0), Some(seat(0))), (seat(1), None)] {
        let (mut runner, shelf, drone) = shelf_with_drone(2, &[[0, 3]]);
        let subject = AssetId::of_unit(drone).expect("an id");
        runner
            .world_mut()
            .request_voxel_edit(VoxelEdit::Crater {
                centre: shelf.necks[0],
                radius: 0,
                by: Some(cutter),
            })
            .expect("filed");
        let mut feed = Vec::new();
        play(&mut runner, 1, &mut feed);
        assert_eq!(count(&feed, EventKind::RegionCollapsed), 1);
        assert_eq!(unit_hp(runner.world(), drone), 0, "the fall kills a drone");
        let credits = kill_credits(&feed, subject);
        if let Some(by) = credited {
            assert_eq!(credits.len(), 1, "{credits:?}");
            assert_eq!(credits[0].0, by);
            assert_eq!(runner.world().tally(by).expect("a seat").kills, 1);
        } else {
            {
                assert!(
                    credits.is_empty(),
                    "an own crater credits nobody: {credits:?}"
                );
                for raw in 0..2 {
                    assert_eq!(runner.world().tally(seat(raw)).expect("a seat").kills, 0);
                }
            }
        }
        // The death settled inside Voxels: the wreck was left the same tick.
        let died = feed
            .iter()
            .find(|event| event.kind == EventKind::UnitDestroyed)
            .expect("destroyed");
        let fell = feed
            .iter()
            .find(|event| event.kind == EventKind::RegionCollapsed)
            .expect("collapsed");
        assert_eq!(died.tick, fell.tick);
    }
}

fn shared(first: VoxelEdit, second: VoxelEdit) -> Vec<(SeatId, i64)> {
    let (mut runner, _, drone) = shelf_with_drone(3, &[[0, 3], [0, -3]]);
    let subject = AssetId::of_unit(drone).expect("an id");
    for edit in [first, second] {
        runner.world_mut().request_voxel_edit(edit).expect("filed");
    }
    let mut feed = Vec::new();
    play(&mut runner, 1, &mut feed);
    assert_eq!(count(&feed, EventKind::RegionCollapsed), 1);
    assert_eq!(unit_hp(runner.world(), drone), 0);
    kill_credits(&feed, subject)
}

fn necks_of(seats: u32) -> Vec<[i32; 3]> {
    let mut world = world(seats);
    let centre = field(&world);
    shelf(&mut world, centre, 2, 2, &[[0, 3], [0, -3]]).necks
}

#[test]
fn two_seats_that_cut_one_region_share_its_credit() {
    // An odd flat damage on a unit that survives it: the two cutters' shares
    // differ by the one hit point the largest remainder gives to the lower
    // seat (decision 37), and the credit counters hold exactly that. A unit,
    // because a structure's footing is anchored ground (the owner's ruling of
    // 2026-10-20, S2's `site`): a shelf carrying one would not fall.
    let rules = edited(|table| {
        table.collapse.as_mut().expect("a block").fall_damage_hp = 51;
    });
    let mut world = world_with(rules, 3, 600_000);
    let centre = field(&world);
    let shelf = shelf(&mut world, centre, 2, 2, &[[0, 3], [0, -3]]);
    let drone = drone_on(&mut world, &shelf, 0, 0);
    let mut runner = pushing(world);
    for (neck, by) in shelf.necks.iter().zip([seat(2), seat(0)]) {
        runner
            .world_mut()
            .request_voxel_edit(VoxelEdit::Crater {
                centre: *neck,
                radius: 0,
                by: Some(by),
            })
            .expect("filed");
    }
    let mut feed = Vec::new();
    play(&mut runner, 1, &mut feed);
    assert_eq!(count(&feed, EventKind::RegionCollapsed), 1);
    assert!(unit_hp(runner.world(), drone) > 0, "it survived the fall");
    let mut shares = Vec::new();
    let asset = AssetId::of_unit(drone).expect("an id");
    runner.world().credit().shares_of(asset.raw(), &mut shares);
    assert_eq!(
        shares,
        vec![(0, 26), (2, 25)],
        "the lower seat takes the odd point"
    );
}

#[test]
fn a_dig_and_a_crater_that_cut_one_region_split_its_credit() {
    let necks = necks_of(3);
    let credits = shared(
        set(necks[0], Material::AIR, Some(seat(0))),
        VoxelEdit::Crater {
            centre: necks[1],
            radius: 0,
            by: Some(seat(2)),
        },
    );
    let seats: Vec<SeatId> = credits.iter().map(|(seat, _)| *seat).collect();
    assert_eq!(seats, vec![seat(0), seat(2)], "{credits:?}");
    assert!(
        (credits[0].1 - credits[1].1).abs() <= 1,
        "split equally: {credits:?}"
    );
}

#[test]
fn interlocking_regions_come_to_rest_and_none_floats() {
    // High over the field, hung from a pillar: a bar B, and a hook A whose
    // leg reaches below B while its arm lies over B. A falls first (its
    // lowest voxel is lower) and lands on B; B then falls away, and the next
    // pass drops A again.
    let mut world = world(2);
    let [cx, cy] = field(&world);
    let mut highest = 0;
    for dx in -6..=6 {
        for dy in -3..=3 {
            highest = highest.max(top(&world, cx + dx, cy + dy));
        }
    }
    let h = highest + 8;
    let mut edits = Vec::new();
    // The pillar at cx + 5, up to the arm's height.
    for z in (top(&world, cx + 5, cy) + 1)..=(h + 2) {
        edits.push(set([cx + 5, cy, z], Material::STONE, None));
    }
    // B: the bar at h, from cx to cx + 4, joined to the pillar.
    for x in cx..=(cx + 4) {
        edits.push(set([x, cy, h], Material::DIRT, None));
    }
    // A: the arm at h + 2 from cx - 2 to cx + 4, joined to the pillar, and
    // the leg down from it at cx - 2 to h - 2.
    for x in (cx - 2)..=(cx + 4) {
        edits.push(set([x, cy, h + 2], Material::DIRT, None));
    }
    for z in (h - 2)..(h + 2) {
        edits.push(set([cx - 2, cy, z], Material::DIRT, None));
    }
    build(&mut world, &edits);
    assert_eq!(world.collapse_counts().regions, 0);
    world
        .request_voxel_edit(set([cx + 4, cy, h], Material::AIR, Some(seat(0))))
        .expect("filed");
    world
        .request_voxel_edit(set([cx + 4, cy, h + 2], Material::AIR, Some(seat(0))))
        .expect("filed");
    tick(&mut world);
    let counts = world.collapse_counts();
    assert_eq!(counts.regions, 2, "both fell");
    assert!(
        counts.last_passes >= 3,
        "a second pass dropped the hook B had held: {} passes",
        counts.last_passes
    );
    assert_eq!(
        floating(&world, [cx - 3, cy - 1, 0], [cx + 6, cy + 1, h + 3]),
        0,
        "nothing floats"
    );
}

#[test]
fn pillars_hold_a_roof() {
    // A roof over the field on two pillars: losing one leaves it standing on
    // the other, an undug column being solid to bedrock (item 144 (3)'s
    // "pillar spacing counts as support for free"); losing both drops it.
    let mut world = world(2);
    let [cx, cy] = field(&world);
    let mut highest = 0;
    for dx in -3..=3 {
        highest = highest.max(top(&world, cx + dx, cy));
    }
    let h = highest + 5;
    let mut edits = Vec::new();
    for x in [cx - 3, cx + 3] {
        for z in (top(&world, x, cy) + 1)..h {
            edits.push(set([x, cy, z], Material::STONE, None));
        }
    }
    for x in (cx - 3)..=(cx + 3) {
        edits.push(set([x, cy, h], Material::DIRT, None));
    }
    build(&mut world, &edits);
    world
        .request_voxel_edit(set([cx - 3, cy, h - 1], Material::AIR, Some(seat(0))))
        .expect("filed");
    tick(&mut world);
    assert_eq!(
        world.collapse_counts().regions,
        0,
        "one pillar holds the roof"
    );
    assert_eq!(world.voxels().get([cx, cy, h]), Some(Material::DIRT));
    world
        .request_voxel_edit(set([cx + 3, cy, h - 1], Material::AIR, Some(seat(0))))
        .expect("filed");
    tick(&mut world);
    assert_eq!(world.collapse_counts().regions, 1, "with neither, it falls");
    assert_eq!(world.voxels().get([cx, cy, h]), Some(Material::AIR));
}

// ---------------------------------------------------------------------------
// The match sees it on the tick it happens (E2)
// ---------------------------------------------------------------------------

#[test]
fn a_block_hung_over_the_last_beacon_cannot_be_dropped_on_it() {
    // Fall's E2 case dropped a block onto seat 1's core, its only beacon, to
    // end the match on the tick of the collapse. A beacon's footing is
    // anchored ground that ignores every edit (the owner's ruling of
    // 2026-10-20, S2's `site`), so the block cannot fall on it: a collapse
    // can no longer end a match, and the E2 tick ordering for a collapse
    // death is pinned by the unit case below. Here the cut drops nothing.
    let rules = edited(|table| {
        table.collapse.as_mut().expect("a block").fall_damage_hp = 1_000_000;
    });
    let mut world = world_with(rules, 2, 600_000);
    let core1 = core(&world, seat(1));
    let [x, y] = column(beacon_at(&world, core1));
    let z = top(&world, x, y);
    let mut highest = 0;
    for dx in 0..=3 {
        highest = highest.max(top(&world, x + dx, y));
    }
    let h = highest.max(z) + 4;
    let mut edits = Vec::new();
    for zz in (top(&world, x + 3, y) + 1)..=h {
        edits.push(set([x + 3, y, zz], Material::STONE, None));
    }
    for xx in x..=(x + 2) {
        edits.push(set([xx, y, h], Material::DIRT, None));
    }
    build(&mut world, &edits);
    let hp = beacon_hp(&world, core1);
    let mut runner = pushing(world);
    runner
        .world_mut()
        .request_voxel_edit(set([x + 1, y, h], Material::AIR, Some(seat(0))))
        .expect("filed");
    let report = runner.step().expect("runs").expect("a Push tick");
    let feed: Vec<Event> = runner.events().to_vec();
    assert_eq!(
        count(&feed, EventKind::RegionCollapsed),
        0,
        "the block stays"
    );
    assert_eq!(count(&feed, EventKind::AssetFell), 0, "nothing fell");
    assert_eq!(beacon_hp(runner.world(), core1), hp, "the core is unhurt");
    assert!(!report.match_ended, "and the match goes on");
}

#[test]
fn a_collapse_death_on_a_segments_last_tick_reaches_its_recap() {
    // A one-second segment: the cut is filed before its last tick, so the
    // drone dies on it, and the death is booked before the segment closes.
    let segment_ms = 1_000;
    let mut world = world_with(rules(), 2, segment_ms);
    let centre = field(&world);
    let shelf = shelf(&mut world, centre, 2, 2, &[[0, 3]]);
    let drone = drone_on(&mut world, &shelf, 0, 0);
    let mut runner = pushing(world);
    let mut feed = Vec::new();
    let last = u32::try_from(segment_ms.checked_div(50).expect("a tick is 50 ms")).expect("ticks");
    for _ in 1..last {
        play(&mut runner, 1, &mut feed);
    }
    runner
        .world_mut()
        .request_voxel_edit(set(shelf.necks[0], Material::AIR, Some(seat(0))))
        .expect("filed");
    let report = runner.step().expect("runs").expect("the last Push tick");
    let events: Vec<Event> = runner.events().to_vec();
    assert!(report.segment_ended, "this was the segment's last tick");
    assert_eq!(unit_hp(runner.world(), drone), 0);
    assert_eq!(count(&events, EventKind::UnitDestroyed), 1);
    assert_eq!(count(&events, EventKind::SegmentEnded), 1);
    let died = events
        .iter()
        .position(|event| event.kind == EventKind::UnitDestroyed)
        .expect("died");
    let ended = events
        .iter()
        .position(|event| event.kind == EventKind::SegmentEnded)
        .expect("ended");
    assert!(
        died < ended,
        "the death is settled before the segment closes"
    );
    assert_eq!(runner.world().tally(seat(0)).expect("a seat").kills, 1);
    assert_eq!(runner.world().tally(seat(1)).expect("a seat").losses, 1);
}

// ---------------------------------------------------------------------------
// Mine never undermines (item 144 (3))
// ---------------------------------------------------------------------------

#[test]
fn two_digs_under_a_structure_on_two_necks_drop_nothing() {
    // A shelf on two necks carries a structure. Fall's check refused the
    // second dig; a footing is anchored ground now (the owner's ruling of
    // 2026-10-20, S2's `site`), so both digs land and the shelf, held by the
    // structure's footing, stays where it is.
    let mut world = world(2);
    let centre = field(&world);
    let shelf = shelf(&mut world, centre, 2, 2, &[[0, 3], [0, -3]]);
    let core1 = core(&world, seat(1));
    let generator = world
        .raise_structure(seat(1), core1, StructureKind::Generator, shelf.spot(0, 0))
        .expect("a structure");
    let hp = structure_hp(&world, generator);
    for neck in &shelf.necks {
        world
            .request_voxel_edit(set(*neck, Material::AIR, Some(seat(0))))
            .expect("a dig, queued");
    }
    tick(&mut world);
    for neck in &shelf.necks {
        assert_eq!(world.voxels().get(*neck), Some(Material::AIR), "dug");
    }
    assert_eq!(world.collapse_counts().regions, 0, "the shelf stays");
    assert_eq!(structure_hp(&world, generator), hp, "and so does its load");
}

/// Seat 0's core mining its starting seam, and the dig it takes next made the
/// only footing of a shelf beside the seam: a dirt slab at the dig's height,
/// three columns east of it, undercut and moated, joined to the dug voxel by
/// one neck voxel whose own column is cut below it. Answers the runner, the
/// core, the dig and the shelf's centre column.
fn mine_shelf() -> (Runner, BeaconId, [i32; 3], [i32; 2]) {
    let mut fixture = world_with(rules(), 2, 180_000);
    let core0 = core(&fixture, seat(0));
    fixture.set_writ(core0, MandateKind::Mine);
    fixture.set_mine_settings(core0, MineSettings::default());
    let mut runner = pushing(fixture);
    let mut feed = Vec::new();
    play(&mut runner, 1, &mut feed);
    let dig = runner.world().next_dig(core0).expect("a dig").ore;
    let [x, y, z] = dig;
    let natural = |runner: &Runner, cx: i32, cy: i32| {
        runner.world().voxels().top_solid_z(cx, cy).expect("solid")
    };
    let mut edits = Vec::new();
    let (sx, sy) = (x + 3, y);
    for dx in -1..=1 {
        for dy in -1..=1 {
            let (cx, cy) = (sx + dx, sy + dy);
            for zz in (z + 1)..=natural(&runner, cx, cy).max(z) {
                edits.push(set([cx, cy, zz], Material::AIR, None));
            }
            edits.push(set([cx, cy, z], Material::DIRT, None));
            for zz in (z - 2)..z {
                edits.push(set([cx, cy, zz], Material::AIR, None));
            }
        }
    }
    for dx in -2_i32..=2 {
        for dy in -2_i32..=2 {
            if dx.abs() != 2 && dy.abs() != 2 {
                continue;
            }
            let (cx, cy) = (sx + dx, sy + dy);
            if [cx, cy] == [x + 1, y] {
                continue;
            }
            for zz in z..=natural(&runner, cx, cy).max(z) {
                edits.push(set([cx, cy, zz], Material::AIR, None));
            }
        }
    }
    // The neck at (x + 1, y, z): dirt on air, joined only to the dig and the
    // slab.
    for zz in (z + 1)..=natural(&runner, x + 1, y).max(z) {
        edits.push(set([x + 1, y, zz], Material::AIR, None));
    }
    edits.push(set([x + 1, y, z], Material::DIRT, None));
    for zz in (z - 2)..z {
        edits.push(set([x + 1, y, zz], Material::AIR, None));
    }
    for dy in [-1, 1] {
        for zz in z..=natural(&runner, x + 1, y + dy).max(z) {
            edits.push(set([x + 1, y + dy, zz], Material::AIR, None));
        }
    }
    for edit in &edits {
        runner.world_mut().request_voxel_edit(*edit).expect("room");
    }
    // The staging lands in a tick of the Push, the dig untouched by it.
    play(&mut runner, 1, &mut feed);
    assert_eq!(
        runner.world().collapse_counts().regions,
        0,
        "the staging dropped nothing"
    );
    assert!(
        runner
            .world()
            .voxels()
            .get(dig)
            .and_then(Material::ore_richness)
            .is_some(),
        "the dig's voxel is still ore"
    );
    assert_eq!(
        runner.world().next_dig(core0).map(|next| next.ore),
        Some(dig),
        "and still the dig Mine takes next"
    );
    (runner, core0, dig, [sx, sy])
}

#[test]
fn mine_takes_a_dig_beside_a_structure_and_the_structure_stands() {
    // The dig is the shelf's only neck. Fall's check refused it with a
    // structure on the shelf; the structure's footing is anchored ground now
    // (the owner's ruling of 2026-10-20, S2's `site`), so Mine takes the dig
    // and nothing falls.
    let (mut runner, core0, dig, [sx, sy]) = mine_shelf();
    let generator = runner
        .world_mut()
        .raise_structure(
            seat(0),
            core0,
            StructureKind::Generator,
            point(sx, sy, dig[2] + 1),
        )
        .expect("a structure on the shelf");
    let hp = structure_hp(runner.world(), generator);
    let mut feed = Vec::new();
    let mut guard = 0;
    while runner
        .world()
        .voxels()
        .get(dig)
        .is_some_and(Material::is_solid)
    {
        play(&mut runner, 1, &mut feed);
        guard += 1;
        assert!(guard < 3_000, "the mining drone takes the dig");
    }
    assert_eq!(count(&feed, EventKind::RegionCollapsed), 0, "nothing fell");
    assert_eq!(structure_hp(runner.world(), generator), hp);
}
#[test]
fn a_dig_that_drops_an_enemy_unit_credits_the_mining_seat() {
    // Mine refuses no dig for the support rule's sake. With an enemy drone
    // on the shelf (a unit, which no footing holds), seat 0's own mining
    // drone takes the dig, the shelf falls, and the drone's death is credited
    // to the mining seat.
    let (mut runner, _, dig, [sx, sy]) = mine_shelf();
    let core1 = core(runner.world(), seat(1));
    let victim = runner
        .world_mut()
        .field_unit(
            seat(1),
            core1,
            UnitKind::BuildDrone,
            point(sx, sy, dig[2] + 1),
        )
        .expect("a drone on the shelf");
    // Hold the victim where it stands: its home is far, so it would walk off.
    let row = runner.world().units().row_of(victim).expect("a row").raw();
    let subject = AssetId::of_unit(victim).expect("an id");
    let mut feed = Vec::new();
    let mut guard = 0;
    while runner
        .world()
        .voxels()
        .get(dig)
        .is_some_and(Material::is_solid)
    {
        runner
            .world_mut()
            .send_unit_directly(row, point(sx, sy, dig[2] + 1));
        play(&mut runner, 1, &mut feed);
        guard += 1;
        assert!(guard < 3_000, "the mining drone takes the dig");
    }
    assert_eq!(
        count(&feed, EventKind::RegionCollapsed),
        1,
        "the shelf fell"
    );
    let credits = kill_credits(&feed, subject);
    assert_eq!(credits.len(), 1, "{credits:?}");
    assert_eq!(credits[0].0, seat(0), "the mining seat's");
}

// ---------------------------------------------------------------------------
// Decision 39's staged collapse
// ---------------------------------------------------------------------------

/// Build the staged blast: seat 1's shelf with the commander, a parked drone,
/// a wreck and a ruin on it, standing on two necks, and seat 0's two mortars
/// in range. No live structure or beacon stands on it: a building's footing is
/// anchored ground (the owner's ruling of 2026-10-20, S2's `site`), so a
/// shelf carrying one would not detach. The test holds the drone where it
/// parked, seat 1 surveys so none of its drones salvages the wreck first, and
/// the ruin is a structure whose beacon, off the shelf, was destroyed.
fn staged_blast() -> (Runner, [StructureId; 2], [[i32; 3]; 2], Loaded2) {
    let mut world = world(2);
    let core1 = core(&world, seat(1));
    let commander = world.commander_of(seat(1)).expect("a commander");
    let [kx, ky] = column(beacon_at(&world, core1));
    let goal = [kx + 9, ky];
    let goal_point = world.standing_point([goal[0], goal[1], 0]);
    let row = world.units().row_of(commander).expect("a row").raw();
    world.send_unit_directly(row, goal_point);
    let mut walked = 0;
    while column(unit_at(&world, commander)) != goal {
        tick(&mut world);
        walked += 1;
        assert!(walked < 4_000, "the commander walks to its spot");
    }
    for _ in 0..40 {
        tick(&mut world);
    }
    let shelf = shelf(&mut world, goal, 2, 2, &[[0, 3], [0, -3]]);
    // Seat 1 surveys, so no drone of its own salvages the wreck or repairs
    // the ruin before the shells land; the test holds the drone where it
    // stands while the mortars come off their cooldown.
    world.set_writ(core1, MandateKind::Survey);
    let perch = world
        .place_beacon_directly(
            seat(1),
            world.standing_point([kx + 2, ky - 6, 0]),
            MandateKind::Survey,
            PRIORITY_NORMAL,
        )
        .expect("a beacon off the shelf");
    let parked = shelf.spot(-2, 0);
    let drone = world
        .field_unit(seat(1), perch, UnitKind::BuildDrone, parked)
        .expect("a drone");
    // A structure whose beacon, off the shelf, is destroyed: a ruin on it.
    let doomed = world
        .place_beacon_directly(
            seat(1),
            world.standing_point([kx + 2, ky + 6, 0]),
            MandateKind::Build,
            PRIORITY_NORMAL,
        )
        .expect("a second beacon");
    let structure = world
        .raise_structure(seat(1), doomed, StructureKind::Generator, shelf.spot(2, 0))
        .expect("a structure to ruin");
    world
        .request_damage(DamageOrder {
            target: DamageTarget::Beacon(doomed),
            amount: Damage::new(1_000_000).expect("damage"),
            by: SeatId::NEUTRAL,
            source: None,
        })
        .expect("filed");
    // A drone killed on the shelf leaves a wreck there.
    let doomed_drone = drone_on(&mut world, &shelf, -1, 0);
    world
        .request_damage(DamageOrder {
            target: DamageTarget::Unit(doomed_drone),
            amount: Damage::new(1_000).expect("damage"),
            by: SeatId::NEUTRAL,
            source: None,
        })
        .expect("filed");
    // One tick settles both deaths: the ruin and the wreck are left.
    tick(&mut world);
    assert_eq!(world.wrecks().ids().len(), 1, "a wreck on the shelf");
    let wreck = WreckId::new(world.wrecks().ids()[0]);
    let mortars = mortars_east_of(&mut world, goal);
    let necks = [shelf.necks[0], shelf.necks[1]];
    (
        pushing(world),
        mortars,
        necks,
        Loaded2 {
            commander,
            drone,
            structure,
            wreck,
            parked,
        },
    )
}

/// Seat 0's beacon and two mortars, a dozen columns east of `goal`, holding
/// their fire so that only the shells the test orders fly.
fn mortars_east_of(world: &mut World, goal: [i32; 2]) -> [StructureId; 2] {
    let east = world.standing_point([goal[0] + 12, goal[1], 0]);
    let home = world
        .place_beacon_directly(seat(0), east, MandateKind::Build, PRIORITY_NORMAL)
        .expect("a beacon");
    world.set_common(
        home,
        Common {
            roe: HOLD_FIRE,
            retreat_hp_pct: 0,
        },
    );
    [
        world
            .raise_structure(
                seat(0),
                home,
                StructureKind::Mortar,
                world.standing_point([goal[0] + 11, goal[1] + 1, 0]),
            )
            .expect("a mortar"),
        world
            .raise_structure(
                seat(0),
                home,
                StructureKind::Mortar,
                world.standing_point([goal[0] + 11, goal[1] - 1, 0]),
            )
            .expect("a mortar"),
    ]
}

struct Loaded2 {
    commander: UnitId,
    drone: UnitId,
    structure: StructureId,
    wreck: WreckId,
    parked: [Fx; 3],
}

/// The Push tick on which the second neck is cut and the shelf falls, in the
/// staged blast below: the mortars come off their cooldown, fire, and their
/// shells fly to the necks.
const CUT_TICK: u32 = 109;

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "decision 39's staged demo beat, run three ways and asserted end to end"
)]
fn collapse_credits_the_blast() {
    // Decision 39's staged geometry, which no playbook reaches: seat 0's
    // mortars shell the two necks of a shelf seat 1's commander, a held
    // drone, a wreck and a ruin stand on; the shelf falls, everything on it
    // takes the flat damage once, and the drone's kill is seat 0's.
    let ticks = 200;
    let named = [
        1_u32,
        50,
        100,
        CUT_TICK - 1,
        CUT_TICK,
        CUT_TICK + 1,
        150,
        200,
    ];
    let run = |restore: bool| -> (Vec<(u32, u64)>, Vec<Event>, Runner, Loaded2) {
        let (mut runner, mortars, necks, loaded) = staged_blast();
        let mut hashes = Vec::new();
        let mut feed = Vec::new();
        let mut fired = [false; 2];
        for tick in 1..=ticks {
            // The drone held where it parked until the cut.
            if tick < CUT_TICK
                && let Ok(row) = runner.world().units().row_of(loaded.drone)
            {
                let row = row.raw();
                runner.world_mut().send_unit_directly(row, loaded.parked);
            }
            // Each mortar shells its neck once, as soon as it is ready.
            for ((mortar, neck), done) in mortars.iter().zip(necks).zip(fired.iter_mut()) {
                if *done {
                    continue;
                }
                let aim = point(neck[0], neck[1], neck[2]);
                match runner.world_mut().order_shell(*mortar, aim) {
                    Ok(()) => *done = true,
                    Err(pharmakos_sim::world::FireRefused::CoolingDown) => {}
                    Err(error) => panic!("the mortar fires: {error:?}"),
                }
            }
            let report = runner.step().expect("runs").expect("in the Push");
            feed.extend_from_slice(runner.events());
            runner.clear_events();
            if named.contains(&tick) {
                hashes.push((tick, report.hash));
            }
            if restore && tick == 60 {
                let saved = Snapshot::capture(runner.world());
                let mut fresh = world(2);
                saved.restore_into(&mut fresh).expect("restores");
                assert_eq!(fresh.state_hash(), runner.world().state_hash());
                *runner.world_mut() = fresh;
            }
        }
        (hashes, feed, runner, loaded)
    };
    let (first, feed, runner, loaded) = run(false);
    let (second, again, _, _) = run(false);
    let (resumed, _, _, _) = run(true);
    assert_eq!(first, second, "the same staging gives the same chain");
    assert_eq!(feed, again, "and the same events");
    assert_eq!(first, resumed, "and a save and restore changes nothing");
    // The chain pinned on either side of the fall, so a fall missing from
    // the hash, or one that moves on another platform, is red here
    // (AGENTS.md section 4.8). A change that moves them re-blesses them with
    // its reason, as any golden chain.
    // Re-blessed by S2's `site`: the staging moved to units, a wreck and a
    // ruin, because a live building's footing is anchored ground (the
    // owner's ruling of 2026-10-20) and a shelf carrying one cannot fall.
    let pinned: Vec<(u32, u64)> = first
        .iter()
        .copied()
        .filter(|(tick, _)| (CUT_TICK - 1..=CUT_TICK + 1).contains(tick))
        .collect();
    assert_eq!(
        pinned,
        vec![
            (CUT_TICK - 1, 0x6285_6f9e_da95_3c1a),
            (CUT_TICK, 0x7746_1ffc_5168_cbae),
            (CUT_TICK + 1, 0x28e6_ef7d_2466_e17a),
        ],
        "the staged blast's chain at the cut"
    );
    assert_eq!(
        count(&feed, EventKind::ShellLanded),
        2,
        "both shells landed"
    );
    assert_eq!(
        count(&feed, EventKind::RegionCollapsed),
        1,
        "the shelf fell once"
    );
    let landed = feed
        .iter()
        .filter(|event| event.kind == EventKind::ShellLanded)
        .map(|event| event.tick)
        .max()
        .expect("landed");
    let collapsed = feed
        .iter()
        .find(|event| event.kind == EventKind::RegionCollapsed)
        .expect("collapsed");
    assert_eq!(
        collapsed.tick, landed,
        "on the tick the second neck was cut"
    );
    let push = feed
        .iter()
        .find(|event| event.kind == EventKind::PushStarted)
        .expect("the Push");
    assert_eq!(
        collapsed.tick.raw() - push.tick.raw(),
        CUT_TICK,
        "the cut lands on the pinned tick"
    );
    let fallers: Vec<Option<AssetId>> = feed
        .iter()
        .filter(|event| event.kind == EventKind::AssetFell)
        .map(|event| event.subject)
        .collect();
    for asset in [
        AssetId::of_unit(loaded.commander),
        AssetId::of_unit(loaded.drone),
        AssetId::of_structure(loaded.structure),
        AssetId::of_wreck(loaded.wreck),
    ] {
        assert!(fallers.contains(&asset.ok()), "{asset:?} fell");
    }
    assert_eq!(fallers.len(), 4, "and nothing else");
    let fall = fall_damage();
    let world = runner.world();
    let commander_hp = world.rules().unit_hp(UnitKind::Commander).raw();
    assert_eq!(unit_hp(world, loaded.commander), commander_hp - fall);
    // The fall kills the drone, and its kill is seat 0's; the commander's
    // fall credits nobody.
    let drone = AssetId::of_unit(loaded.drone).expect("an id");
    if world.rules().unit_hp(UnitKind::BuildDrone).raw() <= fall {
        assert_eq!(unit_hp(world, loaded.drone), 0);
        let credits = kill_credits(&feed, drone);
        assert_eq!(credits.len(), 1, "{credits:?}");
        assert_eq!(credits[0].0, seat(0));
        assert_eq!(world.tally(seat(0)).expect("a seat").kills, 1);
    }
    let commander = AssetId::of_unit(loaded.commander).expect("an id");
    let mut shares = Vec::new();
    world.credit().shares_of(commander.raw(), &mut shares);
    assert!(
        shares.is_empty(),
        "the commander's fall credits nobody: {shares:?}"
    );
    // A ruin belongs to no seat, so its fall credits nobody.
    let wreckage = AssetId::of_structure(loaded.structure).expect("an id");
    let mut shares = Vec::new();
    world.credit().shares_of(wreckage.raw(), &mut shares);
    assert!(
        shares.is_empty(),
        "the ruin's fall credits nobody: {shares:?}"
    );
    assert_eq!(world.event_bus().dropped(), 0, "no event was dropped");
}

#[test]
fn a_collapse_row_no_search_can_use_is_refused_at_load() {
    // (field, max_region_voxels, max_visits_per_tick)
    for (field, region, visits) in [
        ("collapse.max_region_voxels", 0, 16_384),
        ("collapse.max_visits_per_tick", 4_096, 0),
        ("collapse.max_visits_per_tick", 4_096, u32::MAX),
    ] {
        let refused = World::new(&WorldConfig {
            match_seed: SEED,
            seats: 2,
            units_per_seat: 0,
            rules: edited(|table| {
                let collapse = table.collapse.as_mut().expect("a block");
                collapse.max_region_voxels = region;
                collapse.max_visits_per_tick = visits;
            }),
            match_settings: MatchSettings {
                segment_lengths_ms: vec![600_000],
                round_limit: 3,
            },
        });
        match refused {
            Err(pharmakos_sim::MapError::OutOfRange { field: found, .. }) => {
                assert_eq!(found, field);
            }
            other => panic!("{field} is refused at load, not {other:?}"),
        }
    }
}
