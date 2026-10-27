// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The duel report (S2's plan, task `tune2`; decision 17): spec section 9's
//! counter relations played as staged fights on the committed rules table, one
//! line each, for the owner to read at S2's demo. **Never a gate**: the test
//! asserts only that every duel was played and reports, and a relation the
//! numbers break is the owner's evidence, not a red test.
//!
//! The relations, as spec section 9's table states them:
//!
//! * the autocannon against raiders: it kills a raider well before the raider
//!   reaches its own short range, so it takes several raiders to kill one;
//! * the mortar out-ranging the autocannon: from past the autocannon's range,
//!   with a sensor that sees the autocannon, the mortar kills it untouched;
//! * raiders diving a mortar: inside the mortar's shell flight, raiders walk
//!   in under it and kill it;
//! * a mortar against a clump and a wall: the splash hurts every raider in it,
//!   and a wall of two levels takes two shells a voxel;
//! * the shells a crater in dirt and in stone takes at the proposed
//!   `materials.blast_levels` (decision 10);
//! * the mortar's **effective range under seat sight** (decisions-log item 148
//!   (2)): a weapon fires only at what its seat sees, so its range 40 reaches
//!   its full length only where another sensor sees.
//!
//! # Why a sim test
//!
//! The plan names `crates/gamectl/tests/duels.rs`. These arrangements are
//! reached only through the sim's fixture seams (`World::raise_structure`,
//! `World::field_unit`, `World::order_shell`): `Host::seed_layout` places
//! beacons and units, never a structure, and `gamectl` drives a match only
//! through the gateway's `Host` and `Surface`, never through the sim's
//! `Runner` (decisions-log item 109). So the report lives with the sim's other
//! staged combat tests (`tests/combat.rs`), whose helpers it mirrors.
//!
//! # How to run it
//!
//! ```text
//! cargo test --release -p pharmakos-sim --test duels -- --ignored --nocapture
//! ```
//!
//! The report is written to `duels/report.txt` under `CARGO_TARGET_TMPDIR` and
//! printed. Nothing here reads a clock (AGENTS.md section 4.5).

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules, not an integration test's helpers. A panic is this file's failure report: these are panic lints, not determinism lints (AGENTS.md section 5)."
)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use pharmakos_sim::events::EventKind;
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::math::quantity::TICK_HZ;
use pharmakos_sim::runner::Runner;
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::tables::{
    BeaconId, Common, PRIORITY_NORMAL, SeatId, StructureId, StructureKind, UnitId, UnitKind,
};
use pharmakos_sim::voxels::{Material, VoxelEdit};
use pharmakos_sim::world::{World, WorldConfig};
use pharmakos_sim::{MatchSettings, RulesTable, default_rules_path};

/// `tests/combat.rs`'s seed: a map whose midpoint between the first two cores
/// is open ground far from both.
const SEED: u64 = 0x00F1_2E00_C0DE_0001;

/// `MandateSettings.Roe`'s wire values.
const HOLD_FIRE: u8 = 1;
const FREE_FIRE: u8 = 3;

/// The longest a duel plays: two minutes of game time.
const DUEL_TICKS: u32 = 120 * TICK_HZ;

fn rules() -> RulesTable {
    let path = default_rules_path().unwrap_or_else(|| PathBuf::from("rules/rules.v1.json"));
    RulesTable::load(&path).unwrap_or_else(|error| panic!("loading the rules table: {error}"))
}

/// The committed table with each core's surplus raised to 100 kW, so that no
/// staged beacon, structure or unit goes dark: power is not what a duel tests,
/// and a dark asset is no sensor and fires at nothing.
fn powered() -> RulesTable {
    let mut message = rules().message().clone();
    message
        .power
        .as_mut()
        .expect("the committed table has a power block")
        .core_surplus_kw = 100;
    RulesTable::from_message(&message).unwrap_or_else(|error| panic!("the powered table: {error}"))
}

fn world() -> World {
    World::new(&WorldConfig {
        match_seed: SEED,
        seats: 2,
        rules: powered(),
        match_settings: MatchSettings {
            segment_lengths_ms: vec![600_000],
            round_limit: 3,
        },
    })
    .unwrap_or_else(|error| panic!("generating the world: {error}"))
}

fn seat(raw: u8) -> SeatId {
    SeatId::new(raw)
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
        .map_or_else(|| panic!("seat {} has a core", of.raw()), BeaconId::new)
}

fn beacon_at(world: &World, beacon: BeaconId) -> [Fx; 3] {
    let row = world.beacons().row_of(beacon).expect("a live beacon");
    world.beacons().positions()[row.index()]
}

fn column(point: [Fx; 3]) -> [i32; 2] {
    [point[0].floor_voxels(), point[1].floor_voxels()]
}

fn ground(world: &World, from: [Fx; 3], dx: i32, dy: i32) -> [Fx; 3] {
    let [x, y] = column(from);
    world.standing_point([x + dx, y + dy, 0])
}

/// The midpoint of the two cores: open ground neither core's autocannon
/// reaches.
fn field(world: &World) -> [Fx; 3] {
    let a = beacon_at(world, core(world, seat(0)));
    let b = beacon_at(world, core(world, seat(1)));
    let [ax, ay] = column(a);
    let [bx, by] = column(b);
    world.standing_point([i32::midpoint(ax, bx), i32::midpoint(ay, by), 0])
}

fn beacon(world: &mut World, of: SeatId, at: [Fx; 3], roe: u8) -> BeaconId {
    let id = world
        .place_beacon_directly(of, at, MandateKind::Build, PRIORITY_NORMAL)
        .unwrap_or_else(|| panic!("a beacon for seat {}", of.raw()));
    world.set_common(
        id,
        Common {
            roe,
            retreat_hp_pct: 0,
        },
    );
    id
}

fn raise(
    world: &mut World,
    of: SeatId,
    home: BeaconId,
    kind: StructureKind,
    at: [Fx; 3],
) -> StructureId {
    world
        .raise_structure(of, home, kind, at)
        .unwrap_or_else(|| panic!("a {kind:?} for seat {}", of.raw()))
}

fn unit(world: &mut World, of: SeatId, home: BeaconId, kind: UnitKind, at: [Fx; 3]) -> UnitId {
    world
        .field_unit(of, home, kind, at)
        .unwrap_or_else(|| panic!("a {kind:?} for seat {}", of.raw()))
}

fn unit_alive(world: &World, id: UnitId) -> bool {
    world
        .units()
        .row_of(id)
        .is_ok_and(|row| world.units().hit_points()[row.index()].raw() > 0)
}

fn structure_hp(world: &World, id: StructureId) -> i32 {
    world
        .structures()
        .row_of(id)
        .map_or(0, |row| world.structures().hit_points()[row.index()].raw())
}

fn pushing(world: World) -> Runner {
    let mut runner = Runner::new(world);
    assert_eq!(runner.begin_push(), Ok(true), "the Push begins");
    runner
}

/// What a duel's events said, counted as it played.
#[derive(Clone, Copy, Debug, Default)]
struct Counts {
    shells_landed: u32,
}

/// Step one tick, counting its events.
fn tick(runner: &mut Runner, counts: &mut Counts) {
    let report = runner
        .step()
        .unwrap_or_else(|error| panic!("the tick runs: {error}"));
    assert!(report.is_some(), "still in the Push");
    for event in runner.events() {
        if event.kind == EventKind::ShellLanded {
            counts.shells_landed = counts.shells_landed.saturating_add(1);
        }
    }
    runner.clear_events();
}

fn seconds(ticks: u32) -> String {
    let whole = ticks.checked_div(TICK_HZ).expect("a tick rate above zero");
    let hundredths = (ticks % TICK_HZ)
        .saturating_mul(100)
        .checked_div(TICK_HZ)
        .expect("a tick rate above zero");
    format!("{whole}.{hundredths:02} s")
}

fn weapon(kind: StructureKind) -> pharmakos_sim::arms::WeaponRow {
    rules()
        .structure(kind)
        .and_then(|row| row.weapon)
        .unwrap_or_else(|| panic!("the {kind:?} is armed"))
}

// ---------------------------------------------------------------------------
// The duels
// ---------------------------------------------------------------------------

/// `n` raiders of seat 1 that have dived to 3 voxels from seat 0's armed
/// structure, on a `FREE_FIRE` beacon, fire at what they see in their range
/// (decision 21: the nearest enemy structure or beacon, then any unit).
fn raiders_raid(kind: StructureKind, n: u32) -> String {
    let mut world = world();
    let here = field(&world);
    let home_at = ground(&world, here, -6, 0);
    let home = beacon(&mut world, seat(0), home_at, FREE_FIRE);
    let target = raise(&mut world, seat(0), home, kind, here);
    // The raiders stand where they dove to, 3 voxels off, inside their own
    // range of 4, on a beacon of their own there: a Build beacon's raiders
    // walk back to it and nowhere else (closing on a target is Defend's and
    // Attack's), so the approach is the arithmetic line's, not staged.
    let raid_at = ground(&world, here, 3, 0);
    let raid_home = beacon(&mut world, seat(1), raid_at, FREE_FIRE);
    let raiders: Vec<UnitId> = (0..n)
        .map(|_| unit(&mut world, seat(1), raid_home, UnitKind::Raider, raid_at))
        .collect();
    let max_hp = world.structure_max_hp(kind).expect("a row").raw();
    let mut runner = pushing(world);
    let mut counts = Counts::default();
    let mut first_kill: Option<u32> = None;
    let mut first_hit: Option<u32> = None;
    let mut fell: Option<u32> = None;
    for at in 1..=DUEL_TICKS {
        tick(&mut runner, &mut counts);
        let world = runner.world();
        if first_kill.is_none() && raiders.iter().any(|id| !unit_alive(world, *id)) {
            first_kill = Some(at);
        }
        if first_hit.is_none() && structure_hp(world, target) < max_hp {
            first_hit = Some(at);
        }
        if structure_hp(world, target) == 0 {
            fell = Some(at);
            break;
        }
        if raiders.iter().all(|id| !unit_alive(world, *id)) {
            break;
        }
    }
    let world = runner.world();
    let alive = raiders.iter().filter(|id| unit_alive(world, **id)).count();
    format!(
        "{n} raider(s) 3 voxels from a {kind:?} ({} HP): first raider down {}, first hit on it {}, it {}; \
         {} of {n} raiders left, {} HP left on it, {} shells landed",
        max_hp,
        first_kill.map_or_else(|| String::from("never"), seconds),
        first_hit.map_or_else(|| String::from("never"), seconds),
        fell.map_or_else(
            || String::from("stood"),
            |ticks| format!("fell at {}", seconds(ticks))
        ),
        alive,
        structure_hp(world, target),
        counts.shells_landed,
    )
}

/// Seat 0's mortar against seat 1's autocannon 30 voxels away, past the
/// autocannon's range and inside the mortar's; `spotter` puts a seat-0 scout
/// 8 voxels out, 22 from the autocannon, so the seat sees it.
fn mortar_against_autocannon(spotter: bool) -> String {
    let mut world = world();
    let here = field(&world);
    let home_at = ground(&world, here, -6, 0);
    let home = beacon(&mut world, seat(0), home_at, FREE_FIRE);
    let mortar = raise(&mut world, seat(0), home, StructureKind::Mortar, here);
    let gun_at = ground(&world, here, 30, 0);
    let gun_home_at = ground(&world, here, 36, 0);
    let gun_home = beacon(&mut world, seat(1), gun_home_at, FREE_FIRE);
    let gun = raise(
        &mut world,
        seat(1),
        gun_home,
        StructureKind::Autocannon,
        gun_at,
    );
    if spotter {
        let scout_at = ground(&world, here, 8, 0);
        let scout_home = beacon(&mut world, seat(0), scout_at, HOLD_FIRE);
        world.set_writ(scout_home, MandateKind::Survey);
        let _ = unit(&mut world, seat(0), scout_home, UnitKind::Scout, scout_at);
    }
    let mut runner = pushing(world);
    let mut counts = Counts::default();
    let mut fell: Option<u32> = None;
    for at in 1..=DUEL_TICKS {
        tick(&mut runner, &mut counts);
        if structure_hp(runner.world(), gun) == 0 {
            fell = Some(at);
            break;
        }
    }
    let world = runner.world();
    format!(
        "a mortar shells an autocannon 30 voxels off (its range {}), {}: the autocannon {}; {} \
         shells landed; the autocannon dealt seat 0 {} HP, the mortar has {} HP left",
        weapon(StructureKind::Autocannon).range_voxels,
        if spotter {
            "a scout of its seat 22 voxels from the autocannon"
        } else {
            "no other sensor of its seat"
        },
        fell.map_or_else(
            || String::from("stood two minutes"),
            |ticks| format!("fell at {}", seconds(ticks))
        ),
        counts.shells_landed,
        world.damage_dealt(seat(1), seat(0)),
        structure_hp(world, mortar),
    )
}

/// Five raiders of seat 1 clumped 12 voxels from seat 0's mortar, standing
/// on a `HOLD_FIRE` beacon: how many shells the clump takes.
fn mortar_against_a_clump() -> String {
    let mut world = world();
    let here = field(&world);
    let home_at = ground(&world, here, -6, 0);
    let home = beacon(&mut world, seat(0), home_at, FREE_FIRE);
    let _mortar = raise(&mut world, seat(0), home, StructureKind::Mortar, here);
    let clump_at = ground(&world, here, 12, 0);
    let clump_home = beacon(&mut world, seat(1), clump_at, HOLD_FIRE);
    let clump: Vec<UnitId> = (0..5)
        .map(|_| unit(&mut world, seat(1), clump_home, UnitKind::Raider, clump_at))
        .collect();
    let mut runner = pushing(world);
    let mut counts = Counts::default();
    let mut gone: Option<u32> = None;
    let mut after_first: Option<usize> = None;
    for at in 1..=DUEL_TICKS {
        tick(&mut runner, &mut counts);
        let world = runner.world();
        if after_first.is_none() && counts.shells_landed > 0 {
            after_first = Some(clump.iter().filter(|id| !unit_alive(world, **id)).count());
        }
        if clump.iter().all(|id| !unit_alive(world, *id)) {
            gone = Some(at);
            break;
        }
    }
    let shell = weapon(StructureKind::Mortar);
    format!(
        "a mortar shells a clump of 5 raiders 12 voxels off (splash {}, {} HP a shell): {} shells \
         landed, the first killed {}, the clump {}",
        shell.projectile.map_or(0, |p| p.splash_radius_voxels),
        shell.damage.raw(),
        counts.shells_landed,
        after_first.unwrap_or(0),
        gone.map_or_else(
            || String::from("lived two minutes"),
            |ticks| format!("was gone at {}", seconds(ticks))
        ),
    )
}

/// How many of seat 0's mortar shells, ordered at one point, it takes to clear
/// the voxels within the crater's radius of a surface of `material` laid
/// there: the material's blast levels (decision 10).
fn crater_in(material: Material, name: &str, levels: u32) -> String {
    let mut world = world();
    let here = field(&world);
    let home_at = ground(&world, here, -6, 0);
    let home = beacon(&mut world, seat(0), home_at, HOLD_FIRE);
    let mortar = raise(&mut world, seat(0), home, StructureKind::Mortar, here);
    let [x, y] = column(ground(&world, here, 20, 0));
    let top = world.voxels().top_solid_z(x, y).expect("a floor");
    // Lay a 7 x 7 slab, 3 deep, of the material at the aim point.
    for dx in -3..=3 {
        for dy in -3..=3 {
            for dz in 0..3 {
                world
                    .request_voxel_edit(VoxelEdit::Set {
                        at: [x + dx, y + dy, top - dz],
                        material,
                        by: None,
                    })
                    .expect("the edit is filed");
            }
        }
    }
    let mut enc = pharmakos_sim::Enc::with_capacity(64 * 1024);
    world.step(&mut enc).expect("the slab is laid");
    let aim = world.standing_point([x, y, 0]);
    let mut runner = pushing(world);
    let mut counts = Counts::default();
    let mut lines = Vec::new();
    let cooldown = weapon(StructureKind::Mortar).cooldown_ticks();
    let centre_top = |world: &World| world.voxels().top_solid_z(x, y).unwrap_or(0);
    for shell in 1..=4_u32 {
        for _ in 0..cooldown {
            tick(&mut runner, &mut counts);
        }
        runner
            .world_mut()
            .order_shell(mortar, aim)
            .expect("the mortar takes the order");
        let landed = counts.shells_landed;
        for _ in 0..DUEL_TICKS {
            tick(&mut runner, &mut counts);
            if counts.shells_landed > landed {
                break;
            }
        }
        lines.push(format!(
            "after shell {shell} the centre column's top is {} below the surface",
            top - centre_top(runner.world())
        ));
    }
    format!(
        "{name} ({levels} blast level(s)), 4 shells at one point: {}",
        lines.join("; ")
    )
}

/// Seat 0's mortar against a lone seat-1 raider at `distance`, under three
/// sights: the mortar's own vision only (its home beacon far behind it), its
/// home beacon's sphere at the mortar, and a scout of its seat 16 voxels out.
fn effective_range() -> Vec<String> {
    let distances = [10_i32, 20, 30, 38];
    let sights = [
        "own vision only",
        "home sphere at the mortar",
        "a scout 16 out",
    ];
    let mut lines = Vec::new();
    for sight in sights {
        let mut fired_at = Vec::new();
        for distance in distances {
            let mut world = world();
            let here = field(&world);
            let home_at = match sight {
                "home sphere at the mortar" => ground(&world, here, 0, 2),
                _ => ground(&world, here, -40, 0),
            };
            let home = beacon(&mut world, seat(0), home_at, FREE_FIRE);
            let mortar = raise(&mut world, seat(0), home, StructureKind::Mortar, here);
            if sight == "a scout 16 out" {
                let scout_at = ground(&world, here, 16, 3);
                let scout_home_at = ground(&world, here, -40, 4);
                let scout_home = beacon(&mut world, seat(0), scout_home_at, HOLD_FIRE);
                world.set_writ(scout_home, MandateKind::Survey);
                let _ = unit(&mut world, seat(0), scout_home, UnitKind::Scout, scout_at);
            }
            let raider_at = ground(&world, here, distance, 0);
            // Its home where it stands, so it stays there.
            let raider_home_at = raider_at;
            let raider_home = beacon(&mut world, seat(1), raider_home_at, HOLD_FIRE);
            let _ = unit(
                &mut world,
                seat(1),
                raider_home,
                UnitKind::Raider,
                raider_at,
            );
            let _ = mortar;
            let mut runner = pushing(world);
            let mut counts = Counts::default();
            let mut fired = false;
            for _ in 0..(10 * TICK_HZ) {
                tick(&mut runner, &mut counts);
                if runner.world().combat_counts().shells_fired() > 0 {
                    fired = true;
                    break;
                }
            }
            fired_at.push(format!(
                "{distance}: {}",
                if fired { "fires" } else { "holds" }
            ));
        }
        lines.push(format!(
            "mortar (range {}, vision {}) against a raider at distance, {sight}: {}",
            weapon(StructureKind::Mortar).range_voxels,
            rules()
                .structure(StructureKind::Mortar)
                .map_or(0, |row| row.vision_radius_voxels),
            fired_at.join(", ")
        ));
    }
    lines
}

/// Milliseconds as seconds to a tenth, for the arithmetic lines.
fn ms_words(ms: i64) -> String {
    let tenths = ms.checked_div(100).expect("a divisor");
    format!(
        "{}.{} s",
        tenths.checked_div(10).expect("a divisor"),
        tenths.rem_euclid(10)
    )
}

/// The time `weapon` takes to destroy `hp` alone: shots rounded up, the
/// first at once, then one each cooldown.
fn time_to_kill(weapon: pharmakos_sim::arms::WeaponRow, hp: i32) -> String {
    let damage = i64::from(weapon.damage.raw());
    let shots = i64::from(hp)
        .saturating_add(damage.saturating_sub(1))
        .checked_div(damage)
        .expect("a weapon deals damage");
    ms_words(
        shots
            .saturating_sub(1)
            .saturating_mul(i64::from(weapon.cooldown_ms.raw())),
    )
}

/// The rows' own arithmetic, beside the staged fights: time to kill, walking
/// time across a weapon's reach, a shell's flight, and the breach search's
/// price for a wall column.
fn arithmetic(table: &RulesTable) -> Vec<String> {
    let raider = table.unit(UnitKind::Raider);
    let raider_gun = raider.weapon.expect("the raider is armed");
    let cannon = weapon(StructureKind::Autocannon);
    let mortar = weapon(StructureKind::Mortar);
    let hp = |kind: StructureKind| table.structure(kind).expect("a row").hp.raw();
    let cardinal = i64::from(table.step_cardinal());
    // Milliseconds to walk `voxels` cardinal steps at the raider's speed.
    let walk = |voxels: i64| {
        voxels
            .saturating_mul(cardinal)
            .saturating_mul(1_000)
            .checked_div(i64::from(raider.cost_per_second))
            .expect("a speed")
    };
    let shell = mortar.projectile.expect("the mortar fires shells");
    let flight = i64::from(mortar.range_voxels)
        .saturating_mul(1_000)
        .checked_div(i64::from(shell.speed_voxels_per_second))
        .expect("a shell speed");
    let breach = i64::from(pharmakos_sim::attack::BREACH_COLUMN_COST);
    vec![
        format!(
            "arithmetic: the autocannon kills a raider ({} HP) in {}; one raider kills an \
             autocannon ({} HP) in {} and a mortar ({} HP) in {}; a raider walks the {} voxels \
             from the autocannon's range to its own in {}",
            raider.hp.raw(),
            time_to_kill(cannon, raider.hp.raw()),
            hp(StructureKind::Autocannon),
            time_to_kill(raider_gun, hp(StructureKind::Autocannon)),
            hp(StructureKind::Mortar),
            time_to_kill(raider_gun, hp(StructureKind::Mortar)),
            cannon.range_voxels - raider_gun.range_voxels,
            ms_words(walk(i64::from(
                cannon.range_voxels - raider_gun.range_voxels
            ))),
        ),
        format!(
            "arithmetic: a shell flies the mortar's full {} voxels in {}, in which a raider walks \
             {} voxels against a splash of {}",
            mortar.range_voxels,
            ms_words(flight),
            flight
                .saturating_mul(i64::from(raider.cost_per_second))
                .checked_div(cardinal.saturating_mul(1_000))
                .expect("a step cost"),
            shell.splash_radius_voxels,
        ),
        format!(
            "arithmetic: the breach search prices a wall column at {breach} (`attack::BREACH_COLUMN_COST`), \
             {} cardinal steps, {} of raider walking: a force breaches where the way round is more \
             than that much longer per wall column",
            breach.checked_div(cardinal).expect("a step cost"),
            ms_words(
                breach
                    .saturating_mul(1_000)
                    .checked_div(i64::from(raider.cost_per_second))
                    .expect("a speed")
            ),
        ),
    ]
}

fn report_path() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("duels")
        .join("report.txt")
}

#[test]
#[ignore = "the duel report: staged fights, a minute or two in release; run it by name"]
fn the_duel_report_holds_the_kits_counter_relations() {
    let table = rules();
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Pharmakos duel report (S2's plan, task `tune2`; decision 17). Rules hash {}. Each line is \
         a staged fight on seed 0x{SEED:016x}, read by the owner, never a gate.",
        pharmakos_sim::hex(table.rules_hash())
    );
    out.push('\n');
    let mut lines = arithmetic(&table);
    for n in [1, 2, 4, 8] {
        lines.push(raiders_raid(StructureKind::Autocannon, n));
    }
    lines.push(mortar_against_autocannon(true));
    lines.push(mortar_against_autocannon(false));
    for n in [1, 2, 4] {
        lines.push(raiders_raid(StructureKind::Mortar, n));
    }
    lines.push(mortar_against_a_clump());
    let blast = table
        .message()
        .materials
        .as_ref()
        .and_then(|block| block.blast_levels.as_ref())
        .expect("materials.blast_levels");
    lines.push(crater_in(Material::DIRT, "dirt", blast.dirt));
    lines.push(crater_in(Material::STONE, "stone", blast.stone));
    // A wall's two levels are decision 10's ruling, not a row.
    let wall = Material::wall(seat(1), false).expect("a wall id");
    lines.push(crater_in(wall, "a seat-1 wall", 2));
    lines.extend(effective_range());
    for line in &lines {
        let _ = writeln!(out, "* {line}");
    }
    let path = report_path();
    std::fs::create_dir_all(path.parent().expect("a folder")).expect("the report folder");
    std::fs::write(&path, &out).expect("the report");
    eprintln!("{out}\nthe report: {}", path.display());
    assert!(lines.len() >= 17, "every duel reported");
}
