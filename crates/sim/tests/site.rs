// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! S2's `site` (S2's plan, task `site`): Build's S2 settings, the base kit's
//! blueprints, repair and salvage, terraform and S2-10.
//!
//! Each acceptance line of the task has a test of its own name here, except
//! `a_levelling_edit_carries_its_seat`, which reads the voxel queue and so
//! lives beside it in `crates/sim/src/world/site.rs`, and the two scenarios,
//! which `gamectl scenario run` plays.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules, not an integration test's helpers. A panic is this file's failure report."
)]

use pharmakos_proto::gp;
use pharmakos_proto::json;
use pharmakos_sim::economy::{Purchase, Urgency};
use pharmakos_sim::events::EventKind;
use pharmakos_sim::interpreter::{Plan, StepFailed, StepFailure};
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::math::quantity::{Damage, Money};
use pharmakos_sim::runner::{MatchSettings, Runner};
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::site::{self, BuildSettings, Terraform, WallBox};
use pharmakos_sim::snapshot::Snapshot;
use pharmakos_sim::tables::{BeaconId, ColumnBox, SeatId, StructureKind, UnitKind};
use pharmakos_sim::voxels::{Material, VoxelEdit};
use pharmakos_sim::world::{DamageOrder, DamageTarget};
use pharmakos_sim::{RulesTable, World, WorldConfig, mandate_for};
use std::path::PathBuf;

/// The seed `tests/build.rs` plays on.
const SEED: u64 = 0x00A1_B2C3_D4E5_F607;

const SEATS: u32 = 3;

const SEAT: SeatId = SeatId::new(0);

/// Enough `$` that nothing here waits for money unless a test says so.
const RICH: i64 = 100_000;

fn rules() -> RulesTable {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    RulesTable::load(&root.join("rules").join("rules.v1.json"))
        .expect("the committed rules table loads")
}

fn world() -> World {
    World::new(&WorldConfig {
        match_seed: SEED,
        seats: SEATS,
        units_per_seat: 0,
        rules: rules(),
        match_settings: MatchSettings {
            segment_lengths_ms: vec![180_000],
            round_limit: 1,
        },
    })
    .expect("the rules table describes a map")
}

/// A seat's core, its `b_00`, on the Build writ.
fn core_of(world: &World, seat: SeatId) -> BeaconId {
    let row = world
        .beacons()
        .row_of_ordinal(seat, 0)
        .expect("the seat has a core");
    BeaconId::new(u32::try_from(row).expect("a beacon row"))
}

fn core(world: &World) -> BeaconId {
    core_of(world, SEAT)
}

fn core_voxel(world: &World) -> [i32; 3] {
    let row = usize::try_from(core(world).raw()).expect("a row");
    let [x, y, z] = world
        .beacons()
        .positions()
        .get(row)
        .copied()
        .expect("the core stands somewhere");
    [x.floor_voxels(), y.floor_voxels(), z.floor_voxels()]
}

fn point(at: [i32; 3]) -> [Fx; 3] {
    at.map(|axis| Fx::from_voxels(i16::try_from(axis).expect("a coordinate")))
}

fn top(world: &World, x: i32, y: i32) -> i32 {
    world.voxels().top_solid_z(x, y).expect("ground")
}

/// The voxel a unit stands in on the column `(x, y)`.
fn standing(world: &World, x: i32, y: i32) -> [Fx; 3] {
    point([x, y, top(world, x, y) + 1])
}

/// A column `offset` from seat 0's core with no feature on it and no
/// structure or beacon.
fn open_column(world: &World, offset: [i32; 2]) -> [i32; 2] {
    let [cx, cy, _] = core_voxel(world);
    let [dx, dy] = offset;
    let (x, y) = (cx + dx, cy + dy);
    assert!(
        world.features().at_column(x, y).is_none(),
        "the column ({x}, {y}) holds a feature; pick another offset"
    );
    [x, y]
}

/// A whole playbook around one route step, as canonical `gp.v1` JSON.
fn playbook(route: &str) -> gp::v1::Playbook {
    let text = format!(
        concat!(
            "{{\"schema_version\":{{\"major\":1}},",
            "\"meta\":{{\"title\":\"site\",\"author_kind\":\"HUMAN\"}},",
            "\"declarative\":{{\"route\":[{route}]}},",
            "\"on_death\":{{\"on_respawn\":\"CONTINUE\",\"max_deaths_before_fallback\":3}},",
            "\"fallback\":{{\"hold\":{{\"at\":{{\"safest\":{{}}}}}}}},",
            "\"kind\":\"PLAYBOOK\"}}"
        ),
        route = route
    );
    json::decode(&text).unwrap_or_else(|error| panic!("the case decodes: {error}\n{text}"))
}

/// A visit to `b_00` with one row.
fn visit(row: &str) -> String {
    format!(
        r#"{{"label":"site","interface":{{"beacon":{{"beacon_id":"b_00"}},"rows":[{row}]}},"timeout_ms":120000}}"#
    )
}

/// A route that does nothing the test reads: a visit to the core setting its
/// priority.
fn idle() -> String {
    visit(r#"{"set_priority":"NORMAL"}"#)
}

/// Open a Push with `route` sealed for seat 0.
fn push(world: World, route: &str) -> Runner {
    let plan = Plan::compile(&playbook(route), &rules()).expect("compiles");
    let mut runner = Runner::new(world);
    runner
        .seal_playbook(SEAT, plan)
        .expect("a Lull takes a seal");
    assert!(runner.begin_push().unwrap());
    runner
}

/// Step `ticks` ticks, collecting every event of `kinds`.
fn run(runner: &mut Runner, ticks: u32, kinds: &[EventKind]) -> Vec<pharmakos_sim::Event> {
    let mut seen = Vec::new();
    for _ in 0..ticks {
        if runner.step().unwrap().is_none() {
            break;
        }
        seen.extend(
            runner
                .events()
                .iter()
                .filter(|event| kinds.contains(&event.kind))
                .copied(),
        );
        runner.clear_events();
    }
    seen
}

fn treasury(world: &World) -> Money {
    let row = world.seat_row(SEAT).expect("seat 0 is seated");
    world
        .seats()
        .treasuries()
        .get(row)
        .copied()
        .expect("a treasury")
}

fn site_of(world: &World, beacon: BeaconId) -> site::BeaconSite {
    let row = world.beacons().row_of(beacon).expect("a beacon");
    *world.beacons().sites().get(row.index()).expect("a site")
}

fn settings(terraform: Terraform, threshold: u8, rebuild: bool) -> BuildSettings {
    BuildSettings {
        repair_threshold_pct: threshold,
        rebuild_destroyed: rebuild,
        terraform,
    }
}

/// A rich seat 0 whose core holds `settings`, with a drone of `kind` beside
/// the core.
fn fixture(settings: BuildSettings, kind: UnitKind) -> World {
    let mut world = world();
    world.set_treasury(SEAT, Money::new(RICH));
    let beacon = core(&world);
    world.set_build_settings_directly(beacon, settings);
    let [x, y, _] = core_voxel(&world);
    let at = standing(&world, x + 1, y + 1);
    world
        .field_unit(SEAT, beacon, kind, at)
        .expect("room for a drone");
    world
}

fn hit(target: DamageTarget, amount: i64) -> DamageOrder {
    DamageOrder {
        target,
        amount: Damage::new(amount).expect("a damage"),
        by: SeatId::NEUTRAL,
        source: None,
    }
}

fn structure_hp(world: &World, id: pharmakos_sim::tables::StructureId) -> i32 {
    let row = world.structures().row_of(id).expect("a structure");
    world
        .structures()
        .hit_points()
        .get(row.index())
        .expect("hp")
        .raw()
}

fn unit_hp(world: &World, id: pharmakos_sim::tables::UnitId) -> i32 {
    let row = world.units().row_of(id).expect("a unit");
    world
        .units()
        .hit_points()
        .get(row.index())
        .expect("hp")
        .raw()
}

// ---------------------------------------------------------------------------
// Terraform (decisions-log item 144 (5)).
// ---------------------------------------------------------------------------

/// `FILL` raises a structure target's footprint to its anchor's height
/// before the structure goes up: the anchor stands three above the ground, so
/// two voxels are filled under it, and the structure is then raised there.
#[test]
fn fill_raises_a_footprint_to_its_anchor() {
    let mut world = fixture(settings(Terraform::Fill, 0, false), UnitKind::BuildDrone);
    let [x, y] = open_column(&world, [5, 3]);
    let ground = top(&world, x, y);
    let anchor = [x, y, ground + 3];
    world
        .add_target(core(&world), StructureKind::Autocannon.id(), point(anchor))
        .expect("room for a target");
    let mut runner = push(world, &idle());
    let done = run(&mut runner, 1_600, &[EventKind::StructureCompleted]);
    let world = runner.world();
    assert_eq!(
        top(world, x, y),
        ground + 2,
        "the footprint stands at the anchor's height"
    );
    assert_eq!(world.voxels().get([x, y, ground + 1]), Some(Material::DIRT));
    assert_eq!(
        done.len(),
        1,
        "the autocannon is raised once the ground is level"
    );
}

/// `DIG` lowers it: an anchor one below the ground digs the two voxels at
/// and above it.
#[test]
fn dig_lowers_it() {
    let mut world = fixture(settings(Terraform::Dig, 0, false), UnitKind::BuildDrone);
    let [x, y] = open_column(&world, [5, 3]);
    let ground = top(&world, x, y);
    let anchor = [x, y, ground - 1];
    world
        .add_target(core(&world), StructureKind::Mortar.id(), point(anchor))
        .expect("room for a target");
    let mut runner = push(world, &idle());
    let done = run(&mut runner, 1_600, &[EventKind::StructureCompleted]);
    let world = runner.world();
    assert_eq!(
        top(world, x, y),
        ground - 2,
        "the column is dug down to below the anchor"
    );
    assert_eq!(done.len(), 1, "the mortar is raised on the levelled ground");
}

/// A crater blown inside the sphere, as a host's edit.
fn crater(world: &mut World, at: [i32; 2]) -> Vec<[i32; 2]> {
    let [x, y] = at;
    let z = top(world, x, y);
    world
        .request_voxel_edit(VoxelEdit::Crater {
            centre: [x, y, z],
            radius: 2,
            by: None,
        })
        .expect("a crater files");
    let mut columns = Vec::new();
    for dy in -2..=2 {
        for dx in -2..=2 {
            columns.push([x + dx, y + dy]);
        }
    }
    columns
}

/// `FILL` refills a crater inside the sphere back to the generated surface,
/// air only: a cracked voxel, which is solid, stays.
#[test]
fn fill_refills_a_crater_to_the_generated_surface() {
    let mut world = fixture(settings(Terraform::Fill, 0, false), UnitKind::BuildDrone);
    let centre = open_column(&world, [8, -6]);
    let columns = crater(&mut world, centre);
    let mut runner = push(world, &idle());
    // One tick lands the crater.
    run(&mut runner, 1, &[]);
    let dug: Vec<[i32; 2]> = columns
        .iter()
        .copied()
        .filter(|[x, y]| {
            let made = runner
                .world()
                .generated_surface()
                .top(*x, *y)
                .expect("made");
            top(runner.world(), *x, *y) < made
        })
        .collect();
    assert!(!dug.is_empty(), "the crater took some ground");
    let refilled = run(&mut runner, 3_000, &[EventKind::GroundRefilled]);
    let world = runner.world();
    for [x, y] in &dug {
        let made = world.generated_surface().top(*x, *y).expect("made");
        assert_eq!(
            top(world, *x, *y),
            made,
            "({x}, {y}) is back at the generated surface"
        );
    }
    assert!(!refilled.is_empty(), "each refilled column is announced");
    assert!(
        refilled.iter().all(|event| event.seat == Some(SEAT)),
        "the refill names its seat"
    );
}

/// `NONE` leaves the ground as it is.
#[test]
fn none_leaves_ground_as_it_is() {
    let mut world = fixture(settings(Terraform::None, 0, false), UnitKind::BuildDrone);
    let centre = open_column(&world, [8, -6]);
    let columns = crater(&mut world, centre);
    let mut runner = push(world, &idle());
    run(&mut runner, 1, &[]);
    let tops =
        |world: &World| -> Vec<i32> { columns.iter().map(|[x, y]| top(world, *x, *y)).collect() };
    let before = tops(runner.world());
    run(&mut runner, 1_200, &[]);
    assert_eq!(tops(runner.world()), before, "nothing is filled or dug");
}

/// Each levelled voxel costs the wall's price: what left the treasury is the
/// structure, the voxels at `wall_cost_per_voxel` and what the build fund
/// still holds, and nothing else.
#[test]
fn each_levelled_voxel_costs_the_walls_price() {
    let mut world = fixture(settings(Terraform::Fill, 0, false), UnitKind::BuildDrone);
    let [x, y] = open_column(&world, [5, 3]);
    let ground = top(&world, x, y);
    let anchor = [x, y, ground + 4];
    world
        .add_target(core(&world), StructureKind::Autocannon.id(), point(anchor))
        .expect("room for a target");
    let before = treasury(&world);
    let mut runner = push(world, &idle());
    let income = earned(&run(
        &mut runner,
        1_600,
        &[EventKind::OreDelivered, EventKind::SalvageDelivered],
    ));
    let world = runner.world();
    let filled = top(world, x, y) - ground;
    assert_eq!(filled, 3, "three voxels levelled");
    let price = world.rules().wall_cost_per_voxel().raw();
    let structure = world.structure_cost(StructureKind::Autocannon).raw();
    let held = site_of(world, core(world)).build_fund.raw();
    let spent = before.raw() - treasury(world).raw() + income;
    assert_eq!(spent, structure + i64::from(filled) * price + held);
}

// ---------------------------------------------------------------------------
// Walls (decisions 10, 39 and 40; decisions-log item 146 (4)).
// ---------------------------------------------------------------------------

/// A visit adding one wall target with this box.
fn wall_row(anchor: [i32; 3], min: [i32; 3], max: [i32; 3]) -> String {
    let voxel = |[x, y, z]: [i32; 3]| format!(r#"{{"x":{x},"y":{y},"z":{z}}}"#);
    visit(&format!(
        r#"{{"add_build_target":{{"target":{{"blueprint_id":"wall","anchor":{{"voxel":{}}},"wall_box":{{"min":{},"max":{}}}}}}}}}"#,
        voxel(anchor),
        voxel(min),
        voxel(max)
    ))
}

/// The step failures seat 0's route met.
fn failures(events: &[pharmakos_sim::Event]) -> Vec<StepFailure> {
    events
        .iter()
        .filter(|event| event.kind == EventKind::StepFailed && event.seat == Some(SEAT))
        .map(|event| StepFailed::decode(event.value).expect("decodes").reason)
        .collect()
}

/// A wall lower than `materials.wall_min_height_voxels` over the ground in
/// some column is refused when its row commits, and writes nothing; so is a
/// box whose `min.z` lies above the ground, and one that leaves the sphere.
#[test]
fn a_wall_lower_than_the_minimum_is_refused() {
    let cases = |world: &World| {
        let [x, y] = open_column(world, [4, -4]);
        let g = top(world, x, y);
        let radius = i32::from(world.rules().sphere_radius_voxels());
        let [cx, cy, _] = core_voxel(world);
        vec![
            (
                // One voxel over the ground: under the minimum of two.
                ([x, y, g + 1], [x, y, g], [x, y, g + 1]),
                StepFailure::WallTooLow,
            ),
            (
                // `min.z` above the ground's top.
                ([x, y, g + 3], [x, y, g + 2], [x, y, g + 4]),
                StepFailure::WallOffTheGround,
            ),
            (
                // Tall enough, but its top leaves the sphere.
                ([x, y, g + 1], [x, y, g], [x, y, (g + radius + 4).min(63)]),
                StepFailure::WallOutsideSphere,
            ),
            (
                // A box reaching from inside the sphere past its edge.
                (
                    [cx - 3, cy, g + 1],
                    [cx - radius - 2, cy, g - 4],
                    [cx - 3, cy, g + 3],
                ),
                StepFailure::WallOutsideSphere,
            ),
        ]
    };
    let probe = world();
    for ((anchor, min, max), expected) in cases(&probe) {
        let mut fixture = world();
        fixture.set_treasury(SEAT, Money::new(RICH));
        let mut runner = push(fixture, &wall_row(anchor, min, max));
        let events = run(&mut runner, 900, &[EventKind::StepFailed]);
        assert_eq!(failures(&events), vec![expected], "{min:?}..{max:?}");
        assert!(
            runner
                .world()
                .targets()
                .wall_tops()
                .iter()
                .all(|top| *top == pharmakos_sim::tables::NO_WALL),
            "the refused row wrote no wall"
        );
    }
}

/// Every column of a wall's box is filled from the ground's top up to the
/// box's top, on sloped ground too, so no wall voxel has air under it, and
/// each voxel is the building seat's wall.
#[test]
fn a_wall_has_no_air_under_it() {
    let mut world = fixture(settings(Terraform::None, 0, false), UnitKind::BuildDrone);
    let beacon = core(&world);
    let [cx, cy, _] = core_voxel(&world);
    // A run of columns west of the core, across whatever slope the ground has
    // there, for a wall two over the highest of them.
    let (lx, hx, y) = (cx - 10, cx - 4, cy + 5);
    let tops: Vec<i32> = (lx..=hx).map(|x| top(&world, x, y)).collect();
    let low = *tops.iter().min().expect("columns");
    let high = *tops.iter().max().expect("columns");
    let wall = WallBox {
        columns: ColumnBox::new(
            [i16::try_from(lx).unwrap(), i16::try_from(y).unwrap()],
            [i16::try_from(hx).unwrap(), i16::try_from(y).unwrap()],
        )
        .expect("a box"),
        floor: i16::try_from(low).unwrap(),
        top: i16::try_from(high + 2).unwrap(),
    };
    site::check_wall(&world, beacon, wall).expect("the box stands");
    world
        .add_wall_target(beacon, point([lx, y, high + 1]), wall)
        .expect("room for a wall");
    let mut runner = push(world, &idle());
    run(&mut runner, 2_400, &[]);
    let world = runner.world();
    let own = Material::wall(SEAT, false).expect("a wall material");
    for (x, ground) in (lx..=hx).zip(tops) {
        for z in ground + 1..=high + 2 {
            assert_eq!(world.voxels().get([x, y, z]), Some(own), "({x}, {y}, {z})");
        }
        assert!(
            world
                .voxels()
                .get([x, y, ground])
                .is_some_and(Material::is_solid),
            "the wall stands on the ground at ({x}, {y})"
        );
    }
}

/// A wall's box is priced per voxel: what left the treasury is its voxels at
/// `wall_cost_per_voxel` and what the build fund still holds.
#[test]
fn a_wall_box_is_priced_per_voxel() {
    let mut world = fixture(settings(Terraform::None, 0, false), UnitKind::BuildDrone);
    let beacon = core(&world);
    let [x, y] = open_column(&world, [-6, -3]);
    let ground = top(&world, x, y);
    let wall = WallBox {
        columns: ColumnBox::new(
            [i16::try_from(x).unwrap(), i16::try_from(y).unwrap()],
            [i16::try_from(x + 1).unwrap(), i16::try_from(y).unwrap()],
        )
        .expect("a box"),
        floor: i16::try_from(ground - 1).unwrap(),
        top: i16::try_from(ground + 3).unwrap(),
    };
    world
        .add_wall_target(beacon, point([x, y, ground + 1]), wall)
        .expect("room for a wall");
    let before = treasury(&world);
    let mut runner = push(world, &idle());
    let income = earned(&run(
        &mut runner,
        2_400,
        &[EventKind::OreDelivered, EventKind::SalvageDelivered],
    ));
    let world = runner.world();
    let own = Material::wall(SEAT, false).expect("a wall material");
    let mut voxels: i64 = 0;
    for column in [x, x + 1] {
        for z in 0..64 {
            if world.voxels().get([column, y, z]) == Some(own) {
                voxels += 1;
            }
        }
    }
    assert!(voxels >= 6, "the box is filled: {voxels}");
    let price = world.rules().wall_cost_per_voxel().raw();
    let held = site_of(world, beacon).build_fund.raw();
    assert_eq!(
        before.raw() - treasury(world).raw() + income,
        voxels * price + held
    );
}

// ---------------------------------------------------------------------------
// Repair (decisions 38, 42 and 43) and rebuild (item 128 (d)).
// ---------------------------------------------------------------------------

/// A structure of seat 0's beside its core, at full hit points.
fn standing_structure(
    world: &mut World,
    kind: StructureKind,
    offset: [i32; 2],
) -> pharmakos_sim::tables::StructureId {
    let [x, y] = open_column(world, offset);
    let at = standing(world, x, y);
    let beacon = core(world);
    world
        .raise_structure(SEAT, beacon, kind, at)
        .expect("room for a structure")
}

/// An own structure below `repair_threshold_pct` of its hit points is
/// damaged -- it is repair work and a repair drone's patient -- and one at or
/// above it is not.
#[test]
fn a_structure_below_its_repair_threshold_is_damaged() {
    let mut world = fixture(settings(Terraform::None, 50, false), UnitKind::RepairDrone);
    let cannon = standing_structure(&mut world, StructureKind::Autocannon, [4, -5]);
    let max = structure_hp(&world, cannon);
    let beacon = core(&world);
    // At 60 % it is not damaged.
    world
        .request_damage(hit(
            DamageTarget::Structure(cannon),
            (i64::from(max) * 2).checked_div(5).unwrap(),
        ))
        .expect("files");
    let mut runner = push(world, &idle());
    run(&mut runner, 2, &[]);
    assert_eq!(
        site::repair_work(runner.world(), beacon),
        0,
        "60 % is above 50 %"
    );
    // At 40 % it is.
    runner
        .world_mut()
        .request_damage(hit(
            DamageTarget::Structure(cannon),
            i64::from(max).checked_div(5).unwrap(),
        ))
        .expect("files");
    run(&mut runner, 1, &[]);
    assert_eq!(
        site::repair_work(runner.world(), beacon),
        1,
        "40 % is below 50 %"
    );
    let repaired = run(&mut runner, 3_000, &[EventKind::AssetRepaired]);
    assert_eq!(
        structure_hp(runner.world(), cannon),
        max,
        "repaired to full"
    );
    assert_eq!(repaired.len(), 1, "announced once");
}

/// A threshold of 0 never repairs a structure, however hurt.
#[test]
fn a_zero_threshold_never_repairs_a_structure() {
    let mut world = fixture(settings(Terraform::None, 0, false), UnitKind::RepairDrone);
    let cannon = standing_structure(&mut world, StructureKind::Autocannon, [4, -5]);
    let max = structure_hp(&world, cannon);
    world
        .request_damage(hit(
            DamageTarget::Structure(cannon),
            (i64::from(max) * 9).checked_div(10).unwrap(),
        ))
        .expect("files");
    let mut runner = push(world, &idle());
    run(&mut runner, 1_200, &[]);
    assert_eq!(
        structure_hp(runner.world(), cannon),
        max - (max * 9).checked_div(10).unwrap()
    );
    assert_eq!(site::repair_work(runner.world(), core(runner.world())), 0);
}

/// `rebuild_destroyed` buys a destroyed ordinary target again; a capability
/// structure is never rebuilt automatically (spec section 8), and without
/// the setting nothing is.
#[test]
fn rebuild_destroyed_rebuilds_an_ordinary_target_only() {
    for rebuild in [true, false] {
        let mut world = fixture(settings(Terraform::None, 0, rebuild), UnitKind::BuildDrone);
        let beacon = core(&world);
        let [ax, ay] = open_column(&world, [5, 3]);
        let [sx, sy] = open_column(&world, [-5, 4]);
        let cannon_at = standing(&world, ax, ay);
        let spire_at = standing(&world, sx, sy);
        world
            .add_target(beacon, StructureKind::Autocannon.id(), cannon_at)
            .expect("room");
        world
            .add_target(beacon, StructureKind::ResonanceSpire.id(), spire_at)
            .expect("room");
        let mut runner = push(world, &idle());
        let queued = run(&mut runner, 400, &[EventKind::StructureQueued]);
        assert_eq!(queued.len(), 2, "both are bought");
        let ids: Vec<_> = runner.world().targets().built().to_vec();
        for id in ids.into_iter().filter(|id| id.is_some()) {
            let hp = structure_hp(runner.world(), id);
            runner
                .world_mut()
                .request_damage(hit(DamageTarget::Structure(id), i64::from(hp)))
                .expect("files");
        }
        let again = run(&mut runner, 400, &[EventKind::StructureQueued]);
        let kinds: Vec<i64> = again.iter().map(|event| event.value).collect();
        if rebuild {
            assert_eq!(
                kinds,
                vec![i64::from(StructureKind::Autocannon.id())],
                "the autocannon is bought again and the spire is not"
            );
        } else {
            assert!(kinds.is_empty(), "nothing is rebuilt: {kinds:?}");
        }
    }
}

/// A repair drone heals the commander inside its beacon's sphere, free: it
/// has no build cost.
#[test]
fn a_repair_drone_heals_the_commander() {
    let mut world = fixture(settings(Terraform::None, 0, false), UnitKind::RepairDrone);
    let commander = world.commander_of(SEAT).expect("a commander");
    let max = unit_hp(&world, commander);
    world
        .request_damage(hit(DamageTarget::Unit(commander), 100))
        .expect("files");
    let mut runner = push(world, &idle());
    run(&mut runner, 1, &[]);
    let before = treasury(runner.world());
    let events = run(
        &mut runner,
        600,
        &[
            EventKind::AssetRepaired,
            EventKind::OreDelivered,
            EventKind::SalvageDelivered,
        ],
    );
    assert_eq!(unit_hp(runner.world(), commander), max, "healed to full");
    assert!(
        events
            .iter()
            .any(|event| event.kind == EventKind::AssetRepaired),
        "announced"
    );
    assert_eq!(
        treasury(runner.world()).raw() - earned(&events),
        before.raw(),
        "the commander is repaired free"
    );
}

/// A repair drone heals an own unit inside the sphere, below full whatever
/// the threshold says.
#[test]
fn a_repair_drone_heals_an_own_unit_in_the_sphere() {
    let mut world = fixture(settings(Terraform::None, 0, false), UnitKind::RepairDrone);
    let [x, y, _] = core_voxel(&world);
    let at = standing(&world, x - 3, y + 2);
    let drone = world
        .field_unit(SEAT, core(&world), UnitKind::BuildDrone, at)
        .expect("room");
    let max = unit_hp(&world, drone);
    world
        .request_damage(hit(DamageTarget::Unit(drone), 40))
        .expect("files");
    let mut runner = push(world, &idle());
    run(&mut runner, 800, &[]);
    assert_eq!(unit_hp(runner.world(), drone), max);
}

/// Repair is paid for under the Repair band: the Build mandate asks for it
/// as repair work, and what left the treasury covers the hit points restored
/// at their share of the build cost.
#[test]
fn repair_is_paid_for_under_the_repair_band() {
    let mut world = fixture(settings(Terraform::None, 100, false), UnitKind::RepairDrone);
    let cannon = standing_structure(&mut world, StructureKind::Autocannon, [3, 3]);
    let max = structure_hp(&world, cannon);
    world
        .request_damage(hit(
            DamageTarget::Structure(cannon),
            i64::from(max).checked_div(2).unwrap(),
        ))
        .expect("files");
    let mut runner = push(world, &idle());
    let mut asked = false;
    let before = treasury(runner.world());
    let mut income: i64 = 0;
    for _ in 0..2_000 {
        runner.step().unwrap();
        income += earned(runner.events());
        runner.clear_events();
        let world = runner.world();
        if let Some(request) = mandate_for(MandateKind::Build)
            .expect("Build runs")
            .request(world, core(world))
            && request.buys == Purchase::RepairWork
        {
            assert_eq!(request.urgency, Urgency::Repair);
            asked = true;
        }
        if structure_hp(world, cannon) == max {
            break;
        }
    }
    assert!(asked, "the repair was asked for in the Repair band");
    let world = runner.world();
    assert_eq!(structure_hp(world, cannon), max);
    let cost = world.structure_cost(StructureKind::Autocannon);
    let due = site::cost_to_full(cost, max - max.checked_div(2).unwrap(), max).raw();
    let held = site_of(world, core(world)).repair_fund.raw();
    assert_eq!(before.raw() - treasury(world).raw() + income, due + held);
}

/// A cracked own wall voxel inside the sphere is restored to whole by a
/// repair drone, at the wall's price (decision 42).
#[test]
fn a_cracked_own_wall_is_repaired() {
    // A cracked voxel stands at half its strength: repaired under a threshold
    // above 50, left at 50 (the owner's ruling of 2026-10-20). No wall target
    // names it: any wall of the seat's inside the sphere is repaired.
    for (threshold, repaired) in [(60, true), (50, false)] {
        let mut world = fixture(
            settings(Terraform::None, threshold, false),
            UnitKind::RepairDrone,
        );
        let [x, y] = open_column(&world, [-4, 5]);
        let ground = top(&world, x, y);
        let cracked = Material::wall(SEAT, true).expect("a wall");
        let whole = Material::wall(SEAT, false).expect("a wall");
        for z in [ground + 1, ground + 2] {
            world
                .request_voxel_edit(VoxelEdit::Set {
                    at: [x, y, z],
                    material: if z == ground + 2 { cracked } else { whole },
                    by: Some(SEAT),
                })
                .expect("files");
        }
        let mut runner = push(world, &idle());
        run(&mut runner, 600, &[]);
        let now = runner.world().voxels().get([x, y, ground + 2]);
        assert_eq!(
            now == Some(whole),
            repaired,
            "threshold {threshold}: {now:?}"
        );
    }
}

/// A beacon is repaired like any other structure of the seat's, below the
/// threshold, at its share of the beacon's price (the owner's ruling of
/// 2026-10-20).
#[test]
fn a_beacon_below_its_threshold_is_repaired() {
    let mut world = fixture(settings(Terraform::None, 50, false), UnitKind::RepairDrone);
    let [cx, cy, _] = core_voxel(&world);
    let outpost = world
        .place_beacon_directly(
            SEAT,
            standing(&world, cx - 6, cy + 2),
            MandateKind::Survey,
            2,
        )
        .expect("room for a beacon");
    let row = world.beacons().row_of(outpost).expect("a beacon");
    let max = world
        .beacons()
        .hit_points()
        .get(row.index())
        .expect("hp")
        .raw();
    world
        .request_damage(hit(
            DamageTarget::Beacon(outpost),
            i64::from(max).checked_div(2).unwrap() + 10,
        ))
        .expect("files");
    let mut runner = push(world, &idle());
    run(&mut runner, 3_000, &[]);
    let row = runner.world().beacons().row_of(outpost).expect("a beacon");
    assert_eq!(
        runner
            .world()
            .beacons()
            .hit_points()
            .get(row.index())
            .expect("hp")
            .raw(),
        max,
        "repaired to full"
    );
}

/// A wall's blown-out voxel is a destroyed part of it: once its box has been
/// filled whole, a build drone fills the gap again only under
/// `rebuild_destroyed` (the owner's ruling of 2026-10-20).
#[test]
fn a_breached_wall_is_rebuilt_only_under_rebuild_destroyed() {
    for rebuild in [true, false] {
        let mut world = fixture(settings(Terraform::None, 0, rebuild), UnitKind::BuildDrone);
        let beacon = core(&world);
        let [x, y] = open_column(&world, [-6, -3]);
        let ground = top(&world, x, y);
        let wall = WallBox {
            columns: ColumnBox::new(
                [i16::try_from(x).unwrap(), i16::try_from(y).unwrap()],
                [i16::try_from(x).unwrap(), i16::try_from(y).unwrap()],
            )
            .unwrap(),
            floor: i16::try_from(ground).unwrap(),
            top: i16::try_from(ground + 2).unwrap(),
        };
        world
            .add_wall_target(beacon, point([x, y, ground + 1]), wall)
            .expect("room");
        let mut runner = push(world, &idle());
        run(&mut runner, 1_200, &[]);
        assert_eq!(
            top(runner.world(), x, y),
            ground + 2,
            "the wall is filled whole"
        );
        runner
            .world_mut()
            .request_voxel_edit(VoxelEdit::Set {
                at: [x, y, ground + 2],
                material: Material::AIR,
                by: Some(SeatId::new(1)),
            })
            .expect("files");
        run(&mut runner, 1_200, &[]);
        let refilled = top(runner.world(), x, y) == ground + 2;
        assert_eq!(refilled, rebuild, "rebuild_destroyed {rebuild}");
    }
}

/// Any seat's wreck inside the sphere is salvaged by an own repair drone and
/// carried home for `$`.
#[test]
fn any_seats_wreck_is_salvageable_inside_the_sphere() {
    let mut world = fixture(settings(Terraform::None, 0, false), UnitKind::RepairDrone);
    let other = SeatId::new(1);
    let [x, y, _] = core_voxel(&world);
    let at = standing(&world, x + 6, y - 2);
    let foreign = world
        .field_unit(other, core_of(&world, other), UnitKind::Scout, at)
        .expect("room");
    world
        .request_damage(hit(DamageTarget::Unit(foreign), 1_000))
        .expect("files");
    let mut runner = push(world, &idle());
    let delivered = run(&mut runner, 1_200, &[EventKind::SalvageDelivered]);
    assert!(
        delivered
            .iter()
            .any(|event| event.seat == Some(SEAT) && event.value > 0),
        "seat 0's drone delivered seat 1's wreck: {delivered:?}"
    );
}

/// From one hit point to full, a repair costs no more than the build, at a
/// cost and a maximum whose quotient rounds at every step (decision 43, as
/// built): the charges telescope.
#[test]
fn a_full_repair_costs_no_more_than_the_build() {
    for (cost, max) in [(60_i64, 500_i32), (7, 3), (90, 400), (1, 1_000), (999, 7)] {
        let cost = Money::new(cost);
        for step in [1_i32, 3, 20] {
            let mut hp = 1;
            let mut paid = 0;
            while hp < max {
                let to = (hp + step).min(max);
                paid += site::repair_charge(cost, hp, to, max).raw();
                hp = to;
            }
            assert!(paid <= cost.raw(), "{paid} > {} at {max} hp", cost.raw());
            assert_eq!(paid, site::cost_to_full(cost, 1, max).raw());
        }
    }
    assert_eq!(site::cost_to_full(Money::new(60), 250, 500), Money::new(30));
    assert_eq!(site::cost_to_full(Money::new(7), 2, 3), Money::new(3));
    assert_eq!(site::cost_to_full(Money::new(7), 3, 3), Money::ZERO);
}

// ---------------------------------------------------------------------------
// S2-10 (decision 26) and the snapshot.
// ---------------------------------------------------------------------------

/// Removing a paid target recycles what stands for it, refunding half its
/// remaining value, and the site's state rides the snapshot.
#[test]
fn a_dropped_target_recycles_what_stands_for_it() {
    let mut world = fixture(settings(Terraform::None, 0, false), UnitKind::BuildDrone);
    let [x, y] = open_column(&world, [5, 3]);
    let at = [x, y, top(&world, x, y) + 1];
    let row = format!(
        r#"{{"remove_build_target":{{"anchor":{{"voxel":{{"x":{},"y":{},"z":{}}}}}}}}}"#,
        at[0], at[1], at[2]
    );
    let beacon = core(&world);
    world
        .add_target(beacon, StructureKind::Autocannon.id(), point(at))
        .expect("room");
    // The cannon is bought on the first decision tick; the removal commits
    // after the visit's handshake and its row's time.
    let mut runner = push(world, &visit(&row));
    let events = run(
        &mut runner,
        1_800,
        &[EventKind::StructureQueued, EventKind::StructureRecycled],
    );
    let recycled: Vec<_> = events
        .iter()
        .filter(|event| event.kind == EventKind::StructureRecycled)
        .collect();
    assert_eq!(
        recycled.len(),
        1,
        "the dropped target's structure is recycled: {events:?}"
    );
    assert!(recycled.first().expect("one").value > 0, "for a refund");
    let world = runner.world();
    let snapshot = Snapshot::capture(world);
    let mut restored = self::world();
    snapshot.restore_into(&mut restored).expect("restores");
    assert_eq!(restored.state_hash(), world.state_hash());
    assert_eq!(restored.beacons().sites(), world.beacons().sites());
}

/// The site's state is hashed: a beacon's settings move the state hash, and
/// a world that never writes them hashes as it did before S2's `site`.
#[test]
fn the_site_is_hashed_and_quiet_when_unused() {
    let plain = world();
    let mut set = world();
    let beacon = core(&set);
    set.set_build_settings_directly(beacon, settings(Terraform::Both, 30, true));
    assert_ne!(
        plain.state_hash(),
        set.state_hash(),
        "the settings are hashed"
    );
    let mut reset = set;
    reset.set_build_settings_directly(beacon, BuildSettings::DEFAULT);
    assert_eq!(
        plain.state_hash(),
        reset.state_hash(),
        "defaults write nothing"
    );
    let snapshot = Snapshot::capture(&plain);
    assert!(
        snapshot
            .beacon_terraform
            .iter()
            .all(|id| *id == Terraform::None.id())
    );
}

/// What seat 0 earned in `events`: its ore and salvage deliveries.
fn earned(events: &[pharmakos_sim::Event]) -> i64 {
    events
        .iter()
        .filter(|event| {
            event.seat == Some(SEAT)
                && matches!(
                    event.kind,
                    EventKind::OreDelivered | EventKind::SalvageDelivered
                )
        })
        .map(|event| event.value)
        .sum()
}

/// A wall already standing reads the ground it was filled on: a row that
/// names its box again commits rather than failing `wall_too_low` on its own
/// voxels.
#[test]
fn a_wall_named_again_over_its_own_voxels_commits() {
    let mut world = world();
    world.set_treasury(SEAT, Money::new(RICH));
    let [x, y] = open_column(&world, [4, -4]);
    let g = top(&world, x, y);
    let own = Material::wall(SEAT, false).expect("a wall");
    for z in [g + 1, g + 2] {
        world
            .request_voxel_edit(VoxelEdit::Set {
                at: [x, y, z],
                material: own,
                by: Some(SEAT),
            })
            .expect("files");
    }
    let mut runner = push(world, &wall_row([x, y, g + 1], [x, y, g], [x, y, g + 2]));
    let events = run(&mut runner, 900, &[EventKind::StepFailed]);
    assert!(failures(&events).is_empty(), "{:?}", failures(&events));
    assert!(
        runner
            .world()
            .targets()
            .wall_tops()
            .iter()
            .any(|top| *top != pharmakos_sim::tables::NO_WALL),
        "the wall is written"
    );
}

/// A repair once begun runs to full, but a structure above the threshold is
/// begun by nobody: a repair drone standing beside one at 90 % leaves it.
#[test]
fn a_structure_above_its_threshold_is_left_beside_a_drone() {
    let mut world = fixture(settings(Terraform::None, 50, false), UnitKind::RepairDrone);
    // Beside the drone's own spot, one column east of where it was fielded.
    let cannon = standing_structure(&mut world, StructureKind::Autocannon, [2, 1]);
    let max = structure_hp(&world, cannon);
    let hit_hp = max.checked_div(10).unwrap();
    world
        .request_damage(hit(DamageTarget::Structure(cannon), i64::from(hit_hp)))
        .expect("files");
    let mut runner = push(world, &idle());
    run(&mut runner, 600, &[]);
    assert_eq!(structure_hp(runner.world(), cannon), max - hit_hp);
}

// ---------------------------------------------------------------------------
// Footings (the owner's ruling of 2026-10-20).
// ---------------------------------------------------------------------------

/// A blast filed as a host's edit, landing on the next tick.
fn blast_at(world: &mut World, at: [i32; 3], radius: i32) {
    world
        .request_voxel_edit(VoxelEdit::Crater {
            centre: at,
            radius,
            by: Some(SeatId::new(1)),
        })
        .expect("a crater files");
}

/// A column's every voxel, bottom to top.
fn column_bytes(world: &World, x: i32, y: i32) -> Vec<Option<Material>> {
    (0..64).map(|z| world.voxels().get([x, y, z])).collect()
}

/// A shell landing beside a building craters everything in its ball except
/// the building's footprint column, whose every voxel ignores the edit and is
/// counted.
#[test]
fn a_blast_beside_a_building_spares_its_footing() {
    let mut world = fixture(settings(Terraform::None, 0, false), UnitKind::BuildDrone);
    let cannon = standing_structure(&mut world, StructureKind::Autocannon, [5, 3]);
    let [x, y] = open_column(&world, [5, 3]);
    let g = top(&world, x, y);
    let before = column_bytes(&world, x, y);
    let beside = top(&world, x + 1, y);
    blast_at(&mut world, [x + 1, y, g], 2);
    let mut runner = push(world, &idle());
    run(&mut runner, 1, &[]);
    let world = runner.world();
    assert_eq!(
        column_bytes(world, x, y),
        before,
        "the footing is untouched"
    );
    assert!(
        top(world, x + 1, y) < beside,
        "the ground beside it is cratered"
    );
    assert!(
        world.footing_refusals().blast_voxels > 0,
        "each spared voxel is counted"
    );
    assert!(structure_hp(world, cannon) > 0);
}

/// A dig, a fill or any other `set` naming a footing voxel writes nothing and
/// is counted: so a Mine drone's dig can never take a voxel under a building,
/// and nothing can be set down on top of one.
#[test]
fn a_set_on_a_footing_writes_nothing() {
    let mut world = fixture(settings(Terraform::None, 0, false), UnitKind::BuildDrone);
    let _cannon = standing_structure(&mut world, StructureKind::Mortar, [5, 3]);
    let [x, y] = open_column(&world, [5, 3]);
    let g = top(&world, x, y);
    let before = column_bytes(&world, x, y);
    for (at, material) in [
        // A dig under it, as a Mine drone's would be filed.
        ([x, y, g], Material::AIR),
        // A voxel set down above it, as a falling region's would land.
        ([x, y, g + 4], Material::DIRT),
    ] {
        world
            .request_voxel_edit(VoxelEdit::Set {
                at,
                material,
                by: Some(SEAT),
            })
            .expect("files");
    }
    let mut runner = push(world, &idle());
    run(&mut runner, 1, &[]);
    let world = runner.world();
    assert_eq!(column_bytes(world, x, y), before);
    assert_eq!(world.footing_refusals().sets, 2);
}

/// A ruin is not covered: a destroyed beacon's structures turn to ruins, and
/// their footings take edits again from that tick.
#[test]
fn a_ruins_footing_is_editable() {
    let mut world = fixture(settings(Terraform::None, 0, false), UnitKind::BuildDrone);
    let [cx, cy, _] = core_voxel(&world);
    let outpost = world
        .place_beacon_directly(SEAT, standing(&world, cx - 8, cy), MandateKind::Build, 2)
        .expect("room for a beacon");
    let [x, y] = open_column(&world, [-8, 4]);
    let at = standing(&world, x, y);
    let ruin = world
        .raise_structure(SEAT, outpost, StructureKind::Mortar, at)
        .expect("room");
    let hp = {
        let row = world.beacons().row_of(outpost).expect("a beacon");
        world
            .beacons()
            .hit_points()
            .get(row.index())
            .expect("hp")
            .raw()
    };
    world
        .request_damage(hit(DamageTarget::Beacon(outpost), i64::from(hp)))
        .expect("files");
    let mut runner = push(world, &idle());
    run(&mut runner, 2, &[]);
    let row = runner
        .world()
        .structures()
        .row_of(ruin)
        .expect("the ruin stands");
    assert!(
        !runner
            .world()
            .structures()
            .seats()
            .get(row.index())
            .copied()
            .is_some_and(|s| SeatId::new(s).is_some()),
        "a ruin is nobody's"
    );
    let g = top(runner.world(), x, y);
    runner
        .world_mut()
        .request_voxel_edit(VoxelEdit::Set {
            at: [x, y, g],
            material: Material::AIR,
            by: Some(SEAT),
        })
        .expect("files");
    run(&mut runner, 1, &[]);
    assert_eq!(
        top(runner.world(), x, y),
        g - 1,
        "the ruin's footing took the dig"
    );
}

/// A seat's voxel wall is no footing: a blast still cracks it.
#[test]
fn a_wall_voxel_still_breaks() {
    let mut world = fixture(settings(Terraform::None, 0, false), UnitKind::BuildDrone);
    let [x, y] = open_column(&world, [-4, 5]);
    let g = top(&world, x, y);
    let whole = Material::wall(SEAT, false).expect("a wall");
    world
        .request_voxel_edit(VoxelEdit::Set {
            at: [x, y, g + 1],
            material: whole,
            by: Some(SEAT),
        })
        .expect("files");
    let mut runner = push(world, &idle());
    run(&mut runner, 1, &[]);
    // A ball of radius 0 is the one voxel, so the ground under it stays and
    // the support rule has nothing to drop.
    blast_at(runner.world_mut(), [x, y, g + 1], 0);
    run(&mut runner, 1, &[]);
    assert_eq!(
        runner.world().voxels().get([x, y, g + 1]),
        Material::wall(SEAT, true),
        "the wall voxel cracked"
    );
}

/// The footings are derived from hashed tables and add no state: a restored
/// world reads the same footings and the same state hash.
#[test]
fn the_footings_are_derived_from_hashed_tables() {
    let mut world = fixture(settings(Terraform::None, 0, false), UnitKind::BuildDrone);
    let _ = standing_structure(&mut world, StructureKind::Autocannon, [5, 3]);
    let [x, y] = open_column(&world, [5, 3]);
    let snapshot = Snapshot::capture(&world);
    let mut restored = self::world();
    snapshot.restore_into(&mut restored).expect("restores");
    assert_eq!(restored.state_hash(), world.state_hash());
    for (dx, dy) in [(0, 0), (1, 0), (0, 1)] {
        assert_eq!(
            restored.footings().column(x + dx, y + dy),
            world.footings().column(x + dx, y + dy)
        );
    }
    assert!(world.footings().column(x, y));
    let [cx, cy, _] = core_voxel(&world);
    assert!(
        world.footings().column(cx, cy),
        "the core's column is a footing"
    );
}

/// Stage a span over the column `s`: a pillar two columns east of it, from
/// the ground up to `g + 5`, and a beam at `g + 5` from the pillar west over
/// `s` to the column west of it. Every voxel stands on the one below or on
/// its neighbour, so nothing floats.
fn stage_span(world: &mut World, s: [i32; 2], g: i32) -> Vec<[i32; 3]> {
    let [x, y] = s;
    let mut placed = Vec::new();
    for z in g + 1..=g + 5 {
        placed.push([x + 2, y, z]);
    }
    for bx in [x + 1, x, x - 1] {
        placed.push([bx, y, g + 5]);
    }
    for at in &placed {
        world
            .request_voxel_edit(VoxelEdit::Set {
                at: *at,
                material: Material::STONE,
                by: None,
            })
            .expect("files");
    }
    placed
}

/// A footing is anchored ground for the support rule: a beam over a building
/// whose pillar is cut stays where it is, held by the voxel over the
/// building's column, so nothing can be dropped on a building. Without the
/// building the same cut drops the beam, which shows the staging can fall.
#[test]
fn an_overhang_over_a_building_cannot_be_dropped_on_it() {
    for with_building in [true, false] {
        let mut world = fixture(settings(Terraform::None, 0, false), UnitKind::BuildDrone);
        let s = open_column(&world, [-6, -3]);
        let [x, y] = s;
        let g = top(&world, x, y);
        let neighbours_flat = [x - 1, x + 1, x + 2]
            .iter()
            .all(|cx| top(&world, *cx, y) <= g);
        assert!(neighbours_flat, "the span stands on open ground");
        stage_span(&mut world, s, g);
        let mut runner = push(world, &idle());
        run(&mut runner, 1, &[]);
        if with_building {
            let at = point([x, y, g + 1]);
            let beacon = core(runner.world());
            runner
                .world_mut()
                .raise_structure(SEAT, beacon, StructureKind::Autocannon, at)
                .expect("room");
        }
        runner
            .world_mut()
            .request_voxel_edit(VoxelEdit::Set {
                at: [x + 2, y, g + 1],
                material: Material::AIR,
                by: Some(SeatId::new(1)),
            })
            .expect("files");
        let fell = run(&mut runner, 2, &[EventKind::RegionCollapsed]);
        let beam_stands = runner.world().voxels().get([x, y, g + 5]) == Some(Material::STONE);
        if with_building {
            assert!(fell.is_empty(), "nothing falls: {fell:?}");
            assert!(beam_stands, "the beam over the building stands");
        } else {
            assert!(!fell.is_empty(), "without the building the cut beam falls");
        }
    }
}

/// A shell in flight can be drawn: its row keeps where and when it was fired,
/// and `ShellTable::position_at` is the straight-line point between launch and
/// aim in proportion to the ticks flown, the launch point before the fire tick
/// and the aim point from the landing tick. The columns ride the snapshot.
#[test]
fn a_shell_is_visible_in_flight() {
    let mut shells = pharmakos_sim::tables::ShellTable::with_room(4);
    let launch = point([10, 20, 30]);
    let aim = point([30, 0, 30]);
    shells
        .push(
            aim,
            110,
            (launch, 100),
            SEAT,
            0,
            pharmakos_sim::tables::Armament::Structure(StructureKind::Mortar),
        )
        .expect("room");
    assert_eq!(shells.position_at(0, 90), Some(launch));
    assert_eq!(shells.position_at(0, 100), Some(launch));
    assert_eq!(shells.position_at(0, 105), Some(point([20, 10, 30])));
    assert_eq!(shells.position_at(0, 110), Some(aim));
    assert_eq!(shells.position_at(0, 999), Some(aim));
    assert_eq!(shells.position_at(1, 105), None, "no such row");
    // A fractional point stays in Fx, rounded toward the launch: a tenth of
    // three voxels is 19 660 raw Q16.16.
    shells
        .push(
            point([3, 0, 0]),
            10,
            (point([0, 0, 0]), 0),
            SEAT,
            0,
            pharmakos_sim::tables::Armament::Structure(StructureKind::Mortar),
        )
        .expect("room");
    let tenth = shells.position_at(1, 1).expect("a point");
    assert_eq!(tenth.map(Fx::raw), [19_660, 0, 0]);
}
