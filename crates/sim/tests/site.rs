// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! S2's `site` (S2's plan, task `site`): Build's S2 settings, the base kit's
//! blueprints, repair and salvage, terraform and S2-10.
//!
//! Each acceptance line of the task has a test of its own name here, except
//! `a_levelling_edit_carries_its_seat`, which reads the voxel queue, and
//! `an_adopted_structure_is_kept_when_its_old_row_is_dropped`, which writes a
//! settings edit no playbook can reach (a vent with a Generator going up binds
//! no `on`), so both live beside the code in `crates/sim/src/world/site.rs`;
//! and the two scenarios, which `gamectl scenario run` plays. The plan's mortar
//! duel is evidenced here, headless, with its chain pinned.

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
    // A cracked voxel counts as below any threshold above zero, Hold & Build's
    // 50 included, and a threshold of 0 never repairs it (the owner's ruling
    // of 2026-10-20). No wall target names it: any wall of the seat's inside
    // the sphere is repaired.
    for (threshold, repaired) in [(50, true), (1, true), (0, false)] {
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
/// 2026-10-20), and a threshold of 0 never repairs one, as it never repairs a
/// structure.
#[test]
fn a_beacon_is_repaired() {
    for (threshold, repaired) in [(50, true), (0, false)] {
        let mut world = fixture(
            settings(Terraform::None, threshold, false),
            UnitKind::RepairDrone,
        );
        let [cx, cy, _] = core_voxel(&world);
        let outpost = world
            .place_beacon_directly(
                SEAT,
                standing(&world, cx - 6, cy + 2),
                MandateKind::Survey,
                2,
            )
            .expect("room for a beacon");
        let hp_of = |world: &World| {
            let row = world.beacons().row_of(outpost).expect("a beacon");
            world
                .beacons()
                .hit_points()
                .get(row.index())
                .expect("hp")
                .raw()
        };
        let max = hp_of(&world);
        world
            .request_damage(hit(
                DamageTarget::Beacon(outpost),
                i64::from(max).checked_div(2).unwrap() + 10,
            ))
            .expect("files");
        let mut runner = push(world, &idle());
        run(&mut runner, 1, &[]);
        let hurt = hp_of(runner.world());
        assert!(hurt < max, "the beacon is hurt");
        run(&mut runner, 3_000, &[]);
        let expected = if repaired { max } else { hurt };
        assert_eq!(
            hp_of(runner.world()),
            expected,
            "threshold {threshold}: repaired {repaired}"
        );
    }
}

/// A wall's blown-out voxel is a destroyed part of it: once its box has been
/// filled whole, a build drone fills the gap again only under
/// `rebuild_destroyed` (the owner's ruling of 2026-10-20).
#[test]
fn a_removed_wall_voxel_is_rebuilt_only_under_rebuild_destroyed() {
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

/// Two seats' forward Build beacons twenty voxels apart at the midpoint of
/// their cores, each with a mortar under `FREE_FIRE` inside the other's sphere,
/// so each seat sees the other's mortar (`World::seen_by`). Seat 0's beacon
/// holds `seat0`'s settings and two build drones. Both seats are rich.
fn duel(seat0: BuildSettings) -> Runner {
    let mut world = world();
    let other = SeatId::new(1);
    for seat in [SEAT, other] {
        world.set_treasury(seat, Money::new(RICH));
    }
    let a = core_voxel(&world);
    let b_core = core_of(&world, other);
    let b = world
        .beacons()
        .positions()
        .get(world.beacons().row_of(b_core).unwrap().index())
        .copied()
        .unwrap()
        .map(Fx::floor_voxels);
    let (mx, my) = (i32::midpoint(a[0], b[0]), i32::midpoint(a[1], b[1]));
    let mut forward = Vec::new();
    for (seat, dx) in [(SEAT, -10), (other, 10)] {
        let at = standing(&world, mx + dx, my);
        let beacon = world
            .place_beacon_directly(seat, at, MandateKind::Build, 2)
            .expect("room for a beacon");
        world.set_common(
            beacon,
            pharmakos_sim::tables::Common {
                roe: 3,
                retreat_hp_pct: 0,
            },
        );
        let mortar_at = standing(&world, mx + dx.checked_div(2).unwrap(), my + 2);
        world
            .raise_structure(seat, beacon, StructureKind::Mortar, mortar_at)
            .expect("room for a mortar");
        forward.push(beacon);
    }
    let [ours, theirs] = [
        forward.first().copied().unwrap(),
        forward.get(1).copied().unwrap(),
    ];
    world.set_build_settings_directly(ours, seat0);
    world.set_build_settings_directly(theirs, settings(Terraform::None, 50, false));
    for _ in 0..2 {
        let at = standing(&world, mx - 12, my - 1);
        world
            .field_unit(SEAT, ours, UnitKind::BuildDrone, at)
            .expect("room for a drone");
    }
    let mut runner = Runner::new(world);
    for seat in [SEAT, other] {
        let plan = Plan::compile(&playbook(&idle()), &rules()).expect("compiles");
        runner
            .seal_playbook(seat, plan)
            .expect("a Lull takes a seal");
    }
    assert!(runner.begin_push().unwrap());
    runner
}

/// How long a duel is played: long enough for the shells, the repairs, the
/// wrecks, their salvage and the refills.
const DUEL_TICKS: u32 = 2_400;

/// The Push tick a duel's restore is taken and resumed on.
const DUEL_RESTORE: u32 = 600;

/// Play the duel under `seat0`'s settings: every Push tick's hash, from tick
/// 1, and every event. With `restore`, the world is captured at
/// [`DUEL_RESTORE`] and the rest is played on its restore.
fn duel_chain(seat0: BuildSettings, restore: bool) -> (Vec<u64>, Vec<pharmakos_sim::Event>) {
    let mut runner = duel(seat0);
    let mut hashes = Vec::new();
    let mut events = Vec::new();
    for tick in 1..=DUEL_TICKS {
        let report = runner.step().unwrap().expect("in the Push");
        hashes.push(report.hash);
        events.extend_from_slice(runner.events());
        runner.clear_events();
        if restore && tick == DUEL_RESTORE {
            let saved = Snapshot::capture(runner.world());
            let mut fresh = world();
            saved.restore_into(&mut fresh).expect("restores");
            assert_eq!(fresh.state_hash(), runner.world().state_hash());
            *runner.world_mut() = fresh;
        }
    }
    (hashes, events)
}

/// The duel played twice and once more through a restore: the same chain and
/// the same events each time. Answers the chain and the events.
fn duel_evidence(seat0: BuildSettings) -> (Vec<u64>, Vec<pharmakos_sim::Event>) {
    let (first, feed) = duel_chain(seat0, false);
    let (second, again) = duel_chain(seat0, false);
    let (resumed, _) = duel_chain(seat0, true);
    assert_eq!(first, second, "the same duel gives the same chain");
    assert_eq!(feed, again, "and the same events");
    assert_eq!(first, resumed, "and a save and restore changes nothing");
    (first, feed)
}

/// The chain on either side of the first event of each of `kinds`, as
/// `(Push tick, hash)`: what a duel test pins, so a change that moves the
/// shells, the repairs or the refills is red here (AGENTS.md section 4.8). A
/// change that moves them re-blesses them with its reason, as any golden
/// chain.
fn pinned_around(
    chain: &[u64],
    events: &[pharmakos_sim::Event],
    kinds: &[EventKind],
) -> Vec<(u32, u64)> {
    let push = events
        .iter()
        .find(|event| event.kind == EventKind::PushStarted)
        .map_or(0, |event| event.tick.raw());
    let mut pinned = Vec::new();
    for kind in kinds {
        let first = events
            .iter()
            .find(|event| event.kind == *kind)
            .unwrap_or_else(|| panic!("the duel reaches {kind:?}"));
        let at = first.tick.raw() - push;
        for tick in [at - 1, at, at + 1] {
            let hash = chain
                .get(usize::try_from(tick).unwrap() - 1)
                .copied()
                .expect("a hash");
            pinned.push((tick, hash));
        }
    }
    pinned
}

/// The duel of S2's plan, headless, and the evidence for it in place of a
/// committed scenario (CI's time cannot hold the plan's seven-beacon chains):
/// the mortars shell each other's clumps, each forward Build beacon
/// fabricates repair drones for the work in hand, which repair their wounded
/// at decision 43's price, paid in the Repair band, and salvage the wrecks the
/// splash leaves, any seat's. Played twice and through a restore, with its
/// chain pinned around the first shell and the first repair.
#[test]
fn a_mortar_duel_repairs_and_salvages() {
    let (chain, events) = duel_evidence(settings(Terraform::None, 50, false));
    let count = |kind: EventKind| events.iter().filter(|event| event.kind == kind).count();
    assert!(count(EventKind::ShellLanded) > 0, "the mortars fire");
    assert!(
        count(EventKind::AssetRepaired) > 0,
        "a repair drone repairs"
    );
    assert!(count(EventKind::WreckLeft) > 0, "the splash leaves a wreck");
    assert!(
        count(EventKind::SalvageDelivered) > 0,
        "a wreck is salvaged and carried home"
    );
    assert_eq!(
        pinned_around(
            &chain,
            &events,
            &[EventKind::ShellLanded, EventKind::AssetRepaired]
        ),
        vec![
            (104, 0xd31e_5706_a205_18bc),
            (105, 0x5dab_15db_ee4e_c131),
            (106, 0x2557_fde0_84c2_a5a9),
            (181, 0x04b1_5819_5071_b35c),
            (182, 0x2b50_7f1b_ffa1_3f75),
            (183, 0xb08f_dcac_11b2_0592)
        ],
        "the duel's chain at its first shell and its first repair"
    );
}

/// The duel again with seat 0's forward beacon on `FILL`: the shells crater
/// the ground inside its sphere, and its build drones refill each crater to
/// the generated surface, announcing each column (decisions-log item 144
/// (5)). Played twice and through a restore, with its chain pinned around the
/// first shell and the first refill.
#[test]
fn a_mortar_duel_refills_its_craters() {
    let (chain, events) = duel_evidence(settings(Terraform::Fill, 50, false));
    assert!(
        events
            .iter()
            .any(|event| event.kind == EventKind::GroundRefilled && event.seat == Some(SEAT)),
        "seat 0 refills a crater"
    );
    assert_eq!(
        pinned_around(
            &chain,
            &events,
            &[EventKind::ShellLanded, EventKind::GroundRefilled]
        ),
        vec![
            (104, 0x4474_f128_54fc_d46a),
            (105, 0x250e_22f4_ae20_c44a),
            (106, 0xe6c3_d7ee_8edc_0415),
            (138, 0xed35_e4a8_3587_6fc6),
            (139, 0x5287_6dc1_f45f_17f8),
            (140, 0x0ceb_d776_7c8b_ab02)
        ],
        "the duel's chain at its first shell and its first refill"
    );
}

/// A half-built structure pauses on a mandate switch, which clears the old
/// writ's targets (spec section 5), and resumes only when its beacon is Build
/// again and a Build row names its target again (the owner's rulings of
/// 2026-10-20, (d) and (e)): nothing is bought twice, and it is raised from
/// where it stood.
#[test]
fn a_half_built_structure_pauses_on_a_switch_and_resumes_when_a_build_row_names_it() {
    let mut world = fixture(settings(Terraform::None, 0, false), UnitKind::BuildDrone);
    let beacon = core(&world);
    let [x, y] = open_column(&world, [5, 3]);
    let at = standing(&world, x, y);
    world
        .add_target(beacon, StructureKind::Autocannon.id(), at)
        .expect("room");
    let mut runner = push(world, &idle());
    let mut queued = run(&mut runner, 60, &[EventKind::StructureQueued]);
    assert_eq!(queued.len(), 1, "the autocannon is bought");
    let cannon = runner
        .world()
        .targets()
        .built()
        .iter()
        .copied()
        .find(|id| id.is_some())
        .expect("its structure");
    runner.world_mut().set_writ(beacon, MandateKind::Survey);
    run(&mut runner, 5, &[]);
    let paused = structure_hp(runner.world(), cannon);
    run(&mut runner, 200, &[]);
    assert_eq!(
        structure_hp(runner.world(), cannon),
        paused,
        "paused while Survey"
    );
    runner.world_mut().set_writ(beacon, MandateKind::Build);
    run(&mut runner, 200, &[]);
    assert_eq!(
        structure_hp(runner.world(), cannon),
        paused,
        "Build alone does not resume it"
    );
    runner
        .world_mut()
        .add_target(beacon, StructureKind::Autocannon.id(), at)
        .expect("room");
    queued.extend(run(&mut runner, 600, &[EventKind::StructureQueued]));
    assert_eq!(queued.len(), 1, "nothing is bought twice");
    let max = runner
        .world()
        .structure_max_hp(StructureKind::Autocannon)
        .expect("a row")
        .raw();
    assert_eq!(
        structure_hp(runner.world(), cannon),
        max,
        "raised from where it stood"
    );
    // The second leg: the Build row that names it again is a settings row
    // through the playbook door, which replaces the beacon's list
    // (`replace_build_targets`), in the next round.
    resumes_through_a_settings_row();
}

/// The pause's second leg: paused in round 1, resumed in round 2 by a
/// `set_mandate_settings` row naming the autocannon's target again.
fn resumes_through_a_settings_row() {
    let mut world = world_of_rounds(2, 30_000);
    world.set_treasury(SEAT, Money::new(RICH));
    let beacon = core(&world);
    world.set_build_settings_directly(beacon, settings(Terraform::None, 0, false));
    let [kx, ky, _] = core_voxel(&world);
    let near = standing(&world, kx + 1, ky + 1);
    world
        .field_unit(SEAT, beacon, UnitKind::BuildDrone, near)
        .expect("room for a drone");
    let (mut runner, cannon, at) = paused_cannon(world);
    run(&mut runner, 200, &[]);
    let paused = structure_hp(runner.world(), cannon);
    let [x, y, z] = at.map(Fx::floor_voxels);
    let row = visit(&format!(
        r#"{{"set_mandate_settings":{{"build":{{"targets":[{{"blueprint_id":"autocannon","anchor":{{"voxel":{{"x":{x},"y":{y},"z":{z}}}}}}}]}}}}}}"#
    ));
    next_round(&mut runner, &row);
    assert_eq!(
        structure_hp(runner.world(), cannon),
        paused,
        "still paused across the round"
    );
    let queued = run(&mut runner, 580, &[EventKind::StructureQueued]);
    assert!(queued.is_empty(), "nothing is bought twice");
    let max = runner
        .world()
        .structure_max_hp(StructureKind::Autocannon)
        .expect("a row")
        .raw();
    assert_eq!(
        structure_hp(runner.world(), cannon),
        max,
        "raised from where it stood"
    );
}

// ---------------------------------------------------------------------------
// Walls judged whole-column, and the done flag across a re-seal and a restore
// (the second reviews of #133: R1, R2, D4, D5, D6).
// ---------------------------------------------------------------------------

/// Two open columns side by side, `(x, y)` and `(x + 1, y)`, on ground of one
/// height, near seat 0's core: a wall two columns wide, so a voxel blown out
/// of one column leaves the voxel above it held by its neighbour and the
/// support rule drops nothing.
fn flat_pair(world: &World) -> ([i32; 2], i32) {
    let [cx, cy, _] = core_voxel(world);
    for dy in [-6, -5, 5, 6, -7, 7] {
        for dx in -8..=7 {
            let (x, y) = (cx + dx, cy + dy);
            let open = |x: i32| {
                world.features().at_column(x, y).is_none() && !world.ground().structure_on(x, y)
            };
            if open(x) && open(x + 1) && top(world, x, y) == top(world, x + 1, y) {
                return ([x, y], top(world, x, y));
            }
        }
    }
    panic!("no flat pair of open columns near the core");
}

/// A wall box over the pair from `flat_pair`, `height` voxels over the ground.
fn pair_wall([x, y]: [i32; 2], ground: i32, height: i32) -> WallBox {
    let at = |v: i32| i16::try_from(v).unwrap();
    WallBox {
        columns: ColumnBox::new([at(x), at(y)], [at(x + 1), at(y)]).unwrap(),
        floor: at(ground),
        top: at(ground + height),
    }
}

/// An inner voxel blown out of a wall filled whole once, with the wall still
/// standing above it, is a destroyed part of the wall: rebuilt under
/// `rebuild_destroyed` and left out without it (the owner's ruling of
/// 2026-10-20, judged over the whole column).
#[test]
fn a_removed_inner_wall_voxel_is_rebuilt_only_under_rebuild_destroyed() {
    for rebuild in [true, false] {
        let mut world = fixture(settings(Terraform::None, 0, rebuild), UnitKind::BuildDrone);
        let beacon = core(&world);
        let ([x, y], ground) = flat_pair(&world);
        world
            .add_wall_target(
                beacon,
                point([x, y, ground + 1]),
                pair_wall([x, y], ground, 3),
            )
            .expect("room");
        let mut runner = push(world, &idle());
        run(&mut runner, 1_800, &[]);
        for column in [x, x + 1] {
            assert_eq!(top(runner.world(), column, y), ground + 3, "filled whole");
        }
        assert!(
            runner.world().targets().walls_done().contains(&true),
            "and marked done"
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
        let world = runner.world();
        assert!(
            world
                .voxels()
                .get([x, y, ground + 3])
                .and_then(Material::wall_of)
                .is_some(),
            "the voxel above still stands"
        );
        let rebuilt = world
            .voxels()
            .get([x, y, ground + 2])
            .is_some_and(Material::is_solid);
        assert_eq!(rebuilt, rebuild, "rebuild_destroyed {rebuild}");
    }
}

/// A cracked wall voxel under a hole a blast left is still found and
/// repaired: the repair walk reads down past the air to it.
#[test]
fn a_cracked_wall_voxel_under_a_hole_is_repaired() {
    let mut world = fixture(settings(Terraform::None, 50, false), UnitKind::RepairDrone);
    let ([x, y], ground) = flat_pair(&world);
    let whole = Material::wall(SEAT, false).expect("a wall");
    let cracked = Material::wall(SEAT, true).expect("a wall");
    let mut edits = vec![([x, y, ground + 1], cracked), ([x, y, ground + 3], whole)];
    for z in 1..=3 {
        edits.push(([x + 1, y, ground + z], whole));
    }
    for (at, material) in edits {
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
    assert_eq!(
        runner.world().voxels().get([x, y, ground + 2]),
        Some(Material::AIR),
        "the hole is staged under a wall voxel held by its neighbour"
    );
    run(&mut runner, 900, &[]);
    assert_eq!(
        runner.world().voxels().get([x, y, ground + 1]),
        Some(whole),
        "the cracked voxel under the hole is repaired"
    );
}

/// The core's Build settings row naming one wall target, as the playbook
/// door writes it.
fn wall_settings_row(anchor: [i32; 3], wall: WallBox) -> String {
    let [x, y, z] = anchor;
    let [lx, ly] = wall.columns.min_column();
    let [hx, hy] = wall.columns.max_column();
    let (floor, top) = (wall.floor, wall.top);
    let target = format!(
        r#"{{"blueprint_id":"wall","anchor":{{"voxel":{{"x":{x},"y":{y},"z":{z}}}}},"wall_box":{{"min":{{"x":{lx},"y":{ly},"z":{floor}}},"max":{{"x":{hx},"y":{hy},"z":{top}}}}}}}"#
    );
    visit(&format!(
        r#"{{"set_mandate_settings":{{"build":{{"targets":[{target}]}}}}}}"#
    ))
}

/// A world of `rounds` rounds of `segment_ms`, otherwise [`world`]'s.
fn world_of_rounds(rounds: u32, segment_ms: i32) -> World {
    World::new(&WorldConfig {
        match_seed: SEED,
        seats: SEATS,
        units_per_seat: 0,
        rules: rules(),
        match_settings: MatchSettings {
            segment_lengths_ms: vec![segment_ms],
            round_limit: rounds,
        },
    })
    .expect("the rules table describes a map")
}

/// Play the Push out, close the recap and seal `route` for seat 0's next one.
fn next_round(runner: &mut Runner, route: &str) {
    while runner.step().unwrap().is_some() {
        runner.clear_events();
    }
    assert!(runner.end_recap().unwrap(), "the recap closes");
    let plan = Plan::compile(&playbook(route), &rules()).expect("compiles");
    runner
        .seal_playbook(SEAT, plan)
        .expect("a Lull takes a seal");
    assert!(runner.begin_push().unwrap());
}

/// The same wall sealed again, through the playbook door in the next round,
/// keeps its filled-whole flag, so a voxel blown out of it is not rebuilt
/// without `rebuild_destroyed`: a re-seal is no way round the setting (R2).
#[test]
fn a_resealed_wall_is_not_rebuilt_without_rebuild_destroyed() {
    let mut world = world_of_rounds(2, 60_000);
    world.set_treasury(SEAT, Money::new(RICH));
    let beacon = core(&world);
    world.set_build_settings_directly(beacon, settings(Terraform::None, 0, false));
    let [kx, ky, _] = core_voxel(&world);
    let near = standing(&world, kx + 1, ky + 1);
    world
        .field_unit(SEAT, beacon, UnitKind::BuildDrone, near)
        .expect("room for a drone");
    let ([x, y], ground) = flat_pair(&world);
    let wall = pair_wall([x, y], ground, 2);
    let route = wall_settings_row([x, y, ground + 1], wall);
    let mut runner = push(world, &route);
    run(&mut runner, 1_100, &[]);
    assert!(
        runner.world().targets().walls_done().contains(&true),
        "round 1 fills the wall whole"
    );
    next_round(&mut runner, &route);
    runner
        .world_mut()
        .request_voxel_edit(VoxelEdit::Set {
            at: [x, y, ground + 2],
            material: Material::AIR,
            by: Some(SeatId::new(1)),
        })
        .expect("files");
    run(&mut runner, 1_100, &[]);
    let world = runner.world();
    assert!(
        world.targets().walls_done().contains(&true),
        "the re-sealed wall keeps its flag"
    );
    assert_eq!(
        world.voxels().get([x, y, ground + 2]),
        Some(Material::AIR),
        "and the blown-out voxel stays out"
    );
}

/// The done flag is hashed state: it survives a snapshot and its restore,
/// and the restored world hashes as the one it came from (D6).
#[test]
fn a_walls_done_flag_round_trips_through_a_snapshot() {
    let mut staged = fixture(settings(Terraform::None, 0, false), UnitKind::BuildDrone);
    let beacon = core(&staged);
    let ([x, y], ground) = flat_pair(&staged);
    staged
        .add_wall_target(
            beacon,
            point([x, y, ground + 1]),
            pair_wall([x, y], ground, 2),
        )
        .expect("room");
    let mut runner = push(staged, &idle());
    run(&mut runner, 1_200, &[]);
    assert!(runner.world().targets().walls_done().contains(&true));
    let saved = Snapshot::capture(runner.world());
    let mut fresh = world();
    saved.restore_into(&mut fresh).expect("restores");
    assert_eq!(
        fresh.targets().walls_done(),
        runner.world().targets().walls_done()
    );
    assert_eq!(fresh.state_hash(), runner.world().state_hash());
}

// ---------------------------------------------------------------------------
// Adopting a paused structure (R3, D2, D3), the same-tick footing race (R4,
// D1), the funds (R5) and the footings of a placed beacon and of a structure
// going up (R6).
// ---------------------------------------------------------------------------

/// Seat 0's core with a half-built autocannon at `offset`, paused by a switch
/// to Survey and back to Build: the runner, the structure and its anchor.
fn paused_cannon(world: World) -> (Runner, pharmakos_sim::tables::StructureId, [Fx; 3]) {
    let mut world = world;
    let beacon = core(&world);
    let [x, y] = open_column(&world, [5, 3]);
    let at = standing(&world, x, y);
    world
        .add_target(beacon, StructureKind::Autocannon.id(), at)
        .expect("room");
    let mut runner = push(world, &idle());
    let queued = run(&mut runner, 60, &[EventKind::StructureQueued]);
    assert_eq!(queued.len(), 1, "the autocannon is bought");
    let cannon = runner
        .world()
        .targets()
        .built()
        .iter()
        .copied()
        .find(|id| id.is_some())
        .expect("its structure");
    runner.world_mut().set_writ(beacon, MandateKind::Survey);
    run(&mut runner, 5, &[]);
    runner.world_mut().set_writ(beacon, MandateKind::Build);
    (runner, cannon, at)
}

/// A Build row of another blueprint on the paused structure's anchor does not
/// adopt it: the autocannon stays paused, and nothing is bought on its column
/// while it stands (R3).
#[test]
fn a_different_blueprint_on_the_anchor_leaves_the_paused_structure_alone() {
    let world = fixture(settings(Terraform::None, 0, false), UnitKind::BuildDrone);
    let (mut runner, cannon, at) = paused_cannon(world);
    let paused = structure_hp(runner.world(), cannon);
    let beacon = core(runner.world());
    runner
        .world_mut()
        .add_target(beacon, StructureKind::Mortar.id(), at)
        .expect("room");
    let queued = run(&mut runner, 600, &[EventKind::StructureQueued]);
    assert!(queued.is_empty(), "nothing is bought on its column");
    assert_eq!(
        structure_hp(runner.world(), cannon),
        paused,
        "and the autocannon stays paused"
    );
    assert!(
        !runner.world().targets().built().contains(&cannon),
        "no row links it"
    );
}

/// A dig and a wall voxel filed on a Build target's column on the tick the
/// Quartermaster would buy it: the purchase waits, so the footing it begins
/// refuses neither edit and nothing is charged that tick; once the column's
/// edits stop, it is bought (R4, D1).
#[test]
fn a_purchase_waits_for_an_edit_filed_on_its_column_the_same_tick() {
    let mut world = fixture(settings(Terraform::None, 0, false), UnitKind::BuildDrone);
    let beacon = core(&world);
    let [x, y] = open_column(&world, [5, 3]);
    let g = top(&world, x, y);
    let at = standing(&world, x, y);
    world
        .add_target(beacon, StructureKind::Autocannon.id(), at)
        .expect("room");
    let wall = Material::wall(SEAT, false).expect("a wall");
    let mut runner = push(world, &idle());
    let before = treasury(runner.world());
    let mut bought = Vec::new();
    for _ in 0..60 {
        for (voxel, material) in [([x, y, g], Material::AIR), ([x, y, g], wall)] {
            runner
                .world_mut()
                .request_voxel_edit(VoxelEdit::Set {
                    at: voxel,
                    material,
                    by: Some(SEAT),
                })
                .expect("files");
        }
        bought.extend(run(&mut runner, 1, &[EventKind::StructureQueued]));
    }
    let world = runner.world();
    assert!(bought.is_empty(), "the purchase waits: {bought:?}");
    assert_eq!(world.footing_refusals().sets, 0, "no edit was refused");
    assert_eq!(world.voxels().get([x, y, g]), Some(wall), "both landed");
    assert_eq!(treasury(world), before, "nothing was charged");
    let later = run(&mut runner, 60, &[EventKind::StructureQueued]);
    assert_eq!(later.len(), 1, "bought once the column is quiet");
}

/// A Build beacon of seat 0's away from the core, on Build with a drone's
/// work paid for in advance: `repair` and `build` `$` on its funds.
fn funded(world: &mut World, repair: i64, build: i64) -> BeaconId {
    let [cx, cy, _] = core_voxel(world);
    let beacon = world
        .place_beacon_directly(SEAT, standing(world, cx - 6, cy + 2), MandateKind::Build, 2)
        .expect("room for a beacon");
    world.set_site_funds_directly(beacon, Money::new(repair), Money::new(build));
    beacon
}

/// The work a beacon was paid for in advance goes back to the treasury whole
/// on a mandate switch, and on a recycle beside the recycle's own refund
/// (ruling (a)).
#[test]
fn the_site_funds_return_whole_on_a_switch_and_a_recycle() {
    let mut switched = world();
    let beacon = funded(&mut switched, 30, 40);
    let before = treasury(&switched);
    switched.set_writ(beacon, MandateKind::Survey);
    assert_eq!(treasury(&switched).raw(), before.raw() + 70, "a switch");
    assert_eq!(site_of(&switched, beacon).repair_fund, Money::ZERO);
    assert_eq!(site_of(&switched, beacon).build_fund, Money::ZERO);
    // Two beacons alike but for their funds, recycled: the difference in
    // what comes back is the funds, whole.
    let mut twin = world();
    let plain = funded(&mut twin, 0, 0);
    let start = treasury(&twin);
    assert!(twin.recycle_beacon(SEAT, plain));
    let refund = treasury(&twin).raw() - start.raw();
    let mut recycled = world();
    let paid = funded(&mut recycled, 30, 40);
    let start = treasury(&recycled);
    assert!(recycled.recycle_beacon(SEAT, paid));
    assert_eq!(
        treasury(&recycled).raw() - start.raw(),
        refund + 70,
        "a recycle"
    );
}

/// A beacon destroyed loses the work it was paid for: nothing comes back to
/// the treasury (ruling (f)).
#[test]
fn the_site_funds_are_lost_with_the_beacon() {
    let mut world = world();
    let beacon = funded(&mut world, 30, 40);
    let before = treasury(&world);
    world
        .request_damage(hit(DamageTarget::Beacon(beacon), 1_000_000))
        .expect("files");
    let mut runner = push(world, &idle());
    run(&mut runner, 2, &[]);
    assert_eq!(treasury(runner.world()), before, "nothing comes back");
}

/// Paid work counts in the seat's held value, "paid means yours".
#[test]
fn the_site_funds_count_in_held_value() {
    let mut world = world();
    let beacon = funded(&mut world, 0, 0);
    let before = world.held_value(SEAT);
    world.set_site_funds_directly(beacon, Money::new(30), Money::new(40));
    assert_eq!(world.held_value(SEAT).raw(), before.raw() + 70);
}

/// The voxels of a placed beacon's column, not only a core's, ignore every
/// edit (the owner's footing ruling of 2026-10-20).
#[test]
fn a_placed_beacons_footing_ignores_edits() {
    let mut world = world();
    let [cx, cy, _] = core_voxel(&world);
    let [x, y] = open_column(&world, [-6, 2]);
    assert_ne!([x, y], [cx, cy]);
    world
        .place_beacon_directly(SEAT, standing(&world, x, y), MandateKind::Survey, 2)
        .expect("room for a beacon");
    let g = top(&world, x, y);
    let before = column_bytes(&world, x, y);
    for (at, material) in [([x, y, g], Material::AIR), ([x, y, g + 4], Material::DIRT)] {
        world
            .request_voxel_edit(VoxelEdit::Set {
                at,
                material,
                by: Some(SeatId::new(1)),
            })
            .expect("files");
    }
    let mut runner = push(world, &idle());
    run(&mut runner, 1, &[]);
    assert_eq!(column_bytes(runner.world(), x, y), before);
    assert_eq!(runner.world().footing_refusals().sets, 2);
}

/// A structure going up, below its full hit points, already has its footing:
/// from the tick construction begins (the owner's footing ruling).
#[test]
fn a_structure_going_up_has_a_footing() {
    let mut world = fixture(settings(Terraform::None, 0, false), UnitKind::BuildDrone);
    let beacon = core(&world);
    let [x, y] = open_column(&world, [5, 3]);
    let at = standing(&world, x, y);
    world
        .add_target(beacon, StructureKind::Autocannon.id(), at)
        .expect("room");
    let mut runner = push(world, &idle());
    let mut queued = Vec::new();
    while queued.is_empty() {
        queued = run(&mut runner, 1, &[EventKind::StructureQueued]);
    }
    let cannon = runner
        .world()
        .targets()
        .built()
        .iter()
        .copied()
        .find(|id| id.is_some())
        .expect("its structure");
    let max = runner
        .world()
        .structure_max_hp(StructureKind::Autocannon)
        .expect("a row")
        .raw();
    assert!(structure_hp(runner.world(), cannon) < max, "going up");
    let g = top(runner.world(), x, y);
    let before = column_bytes(runner.world(), x, y);
    runner
        .world_mut()
        .request_voxel_edit(VoxelEdit::Set {
            at: [x, y, g],
            material: Material::AIR,
            by: Some(SeatId::new(1)),
        })
        .expect("files");
    run(&mut runner, 1, &[]);
    assert_eq!(column_bytes(runner.world(), x, y), before);
    assert_eq!(runner.world().footing_refusals().sets, 1);
}
