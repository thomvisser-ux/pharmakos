// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Resonance Spire (S3's `lic`; S3's plan, E3, E4 and decision 6).
//!
//! * `queue_structure` is lifted for the Spire alone: a licensed seat queues
//!   it on site at its core, the `$` leaves at the queue, the `kW` draw starts
//!   with it, and the core's fabricator raises it whatever the mandate;
//! * the run-time refusals: no licence, a second Spire at one beacon, a price
//!   the treasury does not reach; every other blueprint is still refused at
//!   the seal;
//! * the sphere grows by the bonus row when the Spire is finished, and returns
//!   to its base on the tick the Spire is destroyed;
//! * every reader of the sphere's radius is named, as one that grows with a
//!   Spire or one that keeps the base row (decision 6), and a reader in
//!   neither list fails this file.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report, and that covers an out-of-bounds index in an assertion for the same reason: these are panic lints, not determinism lints."
)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use pharmakos_proto::gp;
use pharmakos_proto::gp::v1::Licence;
use pharmakos_proto::json;
use pharmakos_sim::events::{Event, EventKind};
use pharmakos_sim::interpreter::{PlanError, StepFailed, StepFailure};
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::math::quantity::{Damage, Money};
use pharmakos_sim::runner::Runner;
use pharmakos_sim::snapshot::Snapshot;
use pharmakos_sim::tables::{BeaconId, SeatId, StructureKind};
use pharmakos_sim::{
    DamageOrder, DamageTarget, MatchSettings, Plan, RulesTable, World, WorldConfig,
};

/// The golden seed.
const SEED: u64 = 0x0000_0000_ca5c_aded;

const SEAT: SeatId = SeatId::new(0);

/// Enough `$` that nothing here waits for money unless a test says so.
const RICH: i64 = 100_000;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn rules() -> RulesTable {
    RulesTable::load(&root().join("rules").join("rules.v1.json"))
        .expect("the committed rules table loads")
}

fn world() -> World {
    World::new(&WorldConfig {
        match_seed: SEED,
        seats: 2,
        rules: rules(),
        match_settings: MatchSettings {
            segment_lengths_ms: vec![180_000],
            round_limit: 1,
        },
    })
    .expect("the rules table describes a map")
}

fn core(world: &World) -> BeaconId {
    let row = world
        .beacons()
        .row_of_ordinal(SEAT, 0)
        .expect("seat 0 has a core");
    BeaconId::new(u32::try_from(row).expect("a beacon row"))
}

fn core_at(world: &World) -> [Fx; 3] {
    world.beacons().positions()[usize::try_from(core(world).raw()).expect("a row")]
}

/// A whole playbook around `route`, as canonical `gp.v1` JSON.
fn playbook(route: &str) -> gp::v1::Playbook {
    let text = format!(
        concat!(
            "{{\"schema_version\":{{\"major\":1}},",
            "\"meta\":{{\"title\":\"spire\",\"author_kind\":\"HUMAN\"}},",
            "\"declarative\":{{\"route\":[{route}]}},",
            "\"on_death\":{{\"on_respawn\":\"CONTINUE\",\"max_deaths_before_fallback\":3}},",
            "\"fallback\":{{\"hold\":{{\"at\":{{\"safest\":{{}}}}}}}},",
            "\"kind\":\"PLAYBOOK\"}}"
        ),
        route = route
    );
    json::decode(&text).unwrap_or_else(|error| panic!("the case decodes: {error}\n{text}"))
}

/// A visit to `b_00` with one `queue_structure` row for `blueprint`.
fn queue_at_core(label: &str, blueprint: &str) -> String {
    format!(
        r#"{{"label":"{label}","interface":{{"beacon":{{"beacon_id":"b_00"}},"rows":[{{"queue_structure":{{"blueprint_id":"{blueprint}"}}}}]}},"timeout_ms":120000,"on_fail":{{"action":"SKIP"}}}}"#
    )
}

fn treasury(world: &World) -> Money {
    world.seats().treasuries()[world.seat_row(SEAT).expect("seated")]
}

fn set_treasury(world: &mut World, dollars: i64) {
    world.set_treasury(SEAT, Money::new(dollars));
}

/// Seal `route` for seat 0 and play up to `ticks` ticks, keeping seat 0's
/// events.
fn play(world: World, route: &str, ticks: u32) -> (Runner, Vec<Event>) {
    let plan = Plan::compile(&playbook(route), &rules()).expect("compiles");
    let mut runner = Runner::new(world);
    runner
        .seal_playbook(SEAT, plan)
        .expect("a Lull takes a seal");
    assert!(runner.begin_push().unwrap());
    let mut feed = Vec::new();
    for _ in 0..ticks {
        if runner.step().unwrap().is_none() {
            break;
        }
        feed.extend(
            runner
                .events()
                .iter()
                .filter(|event| event.seat == Some(SEAT))
                .copied(),
        );
        runner.clear_events();
    }
    (runner, feed)
}

fn licensed() -> World {
    let mut world = world();
    world.award_licence_directly(SEAT, Licence::ResonanceSpire, 1);
    set_treasury(&mut world, RICH);
    world
}

fn failures(feed: &[Event]) -> Vec<StepFailure> {
    feed.iter()
        .filter(|event| event.kind == EventKind::StepFailed)
        .map(|event| StepFailed::decode(event.value).expect("a failure").reason)
        .collect()
}

fn spires(world: &World) -> Vec<(bool, i32)> {
    let structures = world.structures();
    (0..structures.ids().len())
        .filter(|row| {
            !structures.frees()[*row]
                && structures.kinds()[*row] == StructureKind::ResonanceSpire.id()
                && structures.hit_points()[*row].is_alive()
        })
        .map(|row| {
            (
                structures.building()[row],
                structures.hit_points()[row].raw(),
            )
        })
        .collect()
}

#[test]
fn a_licensed_seat_queues_the_spire_and_the_fabricator_raises_it() {
    let world = licensed();
    let core = core(&world);
    let base = Fx::from_voxels(rules().sphere_radius_voxels());
    let grown = Fx::from_voxels(rules().grown_sphere_radius_voxels());
    assert!(grown > base, "the committed bonus grows the sphere");
    assert_eq!(world.sphere_radius_of(core), base);
    let before = treasury(&world);

    let (runner, feed) = play(world, &queue_at_core("spire", "resonance_spire"), 2_400);
    assert!(failures(&feed).is_empty(), "nothing fails: {feed:?}");
    let queued = feed
        .iter()
        .find(|event| event.kind == EventKind::StructureQueued)
        .expect("the Spire is queued");
    assert_eq!(queued.value, i64::from(StructureKind::ResonanceSpire.id()));
    let completed = feed
        .iter()
        .find(|event| {
            event.kind == EventKind::StructureCompleted
                && event.value == i64::from(StructureKind::ResonanceSpire.id())
        })
        .expect("the core's fabricator finishes it");
    assert!(completed.tick > queued.tick);

    let world = runner.world();
    let cost = world.structure_cost(StructureKind::ResonanceSpire);
    // What the seat earned meanwhile (its starting drone's ore) is the only
    // other movement of the treasury.
    let earned: i64 = feed
        .iter()
        .filter(|event| {
            matches!(
                event.kind,
                EventKind::OreDelivered | EventKind::SalvageDelivered
            )
        })
        .map(|event| event.value)
        .sum();
    assert_eq!(
        treasury(world).raw(),
        before.raw() - cost.raw() + earned,
        "the $ left once, at the queue"
    );
    assert_eq!(spires(world), vec![(false, 500)], "one Spire, finished");
    assert_eq!(
        world.sphere_radius_of(core),
        grown,
        "the core's sphere grew"
    );
    assert!(world.spire_on(core));
    // It stands beside the core, on a column of its own: the spiral's first
    // free one, a single step away on this ground.
    let structures = world.structures();
    let row = (0..structures.ids().len())
        .find(|row| structures.kinds()[*row] == StructureKind::ResonanceSpire.id())
        .expect("the Spire");
    let spire = structures.positions()[row];
    let centre = core_at(world);
    let (dx, dy) = (
        spire[0].floor_voxels() - centre[0].floor_voxels(),
        spire[1].floor_voxels() - centre[1].floor_voxels(),
    );
    assert!(
        (dx, dy) != (0, 0) && dx * dx + dy * dy <= 2,
        "beside the beacon, not on it: ({dx}, {dy})"
    );
}

#[test]
fn the_draw_starts_with_the_queue() {
    let world = licensed();
    let rules = rules();
    let draw_kw = |world: &World| world.seats().draws()[world.seat_row(SEAT).expect("seated")];
    let (mut runner, feed) = play(world, &queue_at_core("spire", "resonance_spire"), 2_000);
    let queued = feed
        .iter()
        .find(|event| event.kind == EventKind::StructureQueued)
        .expect("queued")
        .tick
        .raw();
    // Replay to the tick after the queue and read the draw while it is going up.
    let _ = &mut runner;
    let world = licensed();
    let plan = Plan::compile(
        &playbook(&queue_at_core("spire", "resonance_spire")),
        &rules,
    )
    .expect("compiles");
    let mut replay = Runner::new(world);
    replay.seal_playbook(SEAT, plan).expect("sealed");
    assert!(replay.begin_push().unwrap());
    let mut before = None;
    while replay.world().tick().raw() < queued {
        before = Some(draw_kw(replay.world()));
        replay.step().unwrap();
    }
    replay.step().unwrap();
    let going_up = spires(replay.world());
    assert_eq!(going_up.len(), 1);
    assert!(going_up[0].0, "still going up one tick after the queue");
    let spire_draw = rules
        .message()
        .structures
        .as_ref()
        .and_then(|block| block.resonance_spire)
        .expect("a Spire row")
        .draw_kw;
    assert_eq!(
        draw_kw(replay.world()).raw() - before.expect("a tick before").raw(),
        i32::try_from(spire_draw).expect("fits"),
        "the Spire draws while it is going up"
    );
}

#[test]
fn a_seat_without_the_licence_fails_not_licensed_and_pays_nothing() {
    let mut world = world();
    set_treasury(&mut world, RICH);
    let (runner, feed) = play(world, &queue_at_core("spire", "resonance_spire"), 1_200);
    assert_eq!(failures(&feed), vec![StepFailure::NotLicensed]);
    assert!(spires(runner.world()).is_empty());
    assert_eq!(treasury(runner.world()).raw(), RICH);
}

#[test]
fn a_second_spire_at_one_beacon_fails_spire_present() {
    let route = format!(
        "{},{}",
        queue_at_core("first", "resonance_spire"),
        queue_at_core("second", "resonance_spire")
    );
    let (runner, feed) = play(licensed(), &route, 1_200);
    assert_eq!(failures(&feed), vec![StepFailure::SpirePresent]);
    assert_eq!(spires(runner.world()).len(), 1, "one Spire per beacon");
}

#[test]
fn a_spire_the_treasury_does_not_reach_fails_unaffordable() {
    let mut world = licensed();
    set_treasury(&mut world, 10);
    let (runner, feed) = play(world, &queue_at_core("spire", "resonance_spire"), 1_200);
    assert_eq!(failures(&feed), vec![StepFailure::Unaffordable]);
    assert!(spires(runner.world()).is_empty());
    assert_eq!(treasury(runner.world()).raw(), 10, "nothing was charged");
}

#[test]
fn every_other_blueprint_is_still_refused_at_the_seal() {
    let refused = Plan::compile(&playbook(&queue_at_core("other", "autocannon")), &rules());
    match refused {
        Err(PlanError::NotAtThisStage { construct, .. }) => assert_eq!(
            construct,
            "interface row `queue_structure` naming a blueprint other than `resonance_spire`"
        ),
        other => panic!("an autocannon queue compiled: {other:?}"),
    }
}

#[test]
fn the_sphere_returns_to_its_base_on_the_tick_the_spire_is_destroyed() {
    let (mut runner, _) = play(
        licensed(),
        &queue_at_core("spire", "resonance_spire"),
        2_400,
    );
    let core = core(runner.world());
    let base = Fx::from_voxels(rules().sphere_radius_voxels());
    let grown = Fx::from_voxels(rules().grown_sphere_radius_voxels());
    assert_eq!(runner.world().sphere_radius_of(core), grown);
    // A point between the two radii, due east of the core on its own level.
    let at = core_at(runner.world());
    let between = [
        at[0].saturating_add(Fx::from_voxels(rules().sphere_radius_voxels() + 4)),
        at[1],
        at[2],
    ];
    let voxel = [
        between[0].floor_voxels(),
        between[1].floor_voxels(),
        between[2].floor_voxels(),
    ];
    assert!(
        runner.world().spheres().contains(SEAT, voxel),
        "the grown sphere sees it"
    );
    assert!(runner.world().ground().inside_own_sphere(SEAT, between));

    let structures = runner.world().structures();
    let row = (0..structures.ids().len())
        .find(|row| structures.kinds()[*row] == StructureKind::ResonanceSpire.id())
        .expect("the Spire");
    let id = pharmakos_sim::tables::StructureId::new(structures.ids()[row]);
    runner
        .world_mut()
        .request_damage(DamageOrder {
            target: DamageTarget::Structure(id),
            amount: Damage::new(1_000_000).expect("a damage"),
            by: SeatId::new(1),
            source: None,
        })
        .expect("room");
    runner.step().unwrap();
    let world = runner.world();
    assert_eq!(
        world.sphere_radius_of(core),
        base,
        "the dome collapses that tick"
    );
    assert!(!world.spheres().contains(SEAT, voxel));
    assert!(!world.ground().inside_own_sphere(SEAT, between));
    assert!(!world.spire_on(core));
}

#[test]
fn a_grown_sphere_survives_a_save_and_a_restore() {
    let (runner, _) = play(
        licensed(),
        &queue_at_core("spire", "resonance_spire"),
        2_400,
    );
    let saved = runner.world().clone();
    let core = core(&saved);
    let bytes = Snapshot::capture(&saved).to_bytes().expect("encoding");
    let mut restored = world();
    Snapshot::from_bytes(&bytes)
        .expect("decoding")
        .restore_into(&mut restored)
        .expect("restoring");
    assert_eq!(
        restored.sphere_radius_of(core),
        saved.sphere_radius_of(core)
    );
    assert_eq!(restored.state_hash(), saved.state_hash());
}

// ---------------------------------------------------------------------------
// E3: every reader of the sphere's radius, named
// ---------------------------------------------------------------------------

/// What reads the **base** row, `beacon.sphere_radius_voxels`, or a wrapper
/// that answers the base: the rules table's field and accessor, the grown
/// radius summed from it, the targeting view's site test and the
/// base-radius anchor test. Matched as identifiers, so a call reached
/// through `use`, a path or a method all count.
const BASE_NEEDLES: &[&str] = &[
    "sphere_radius_voxels",
    "grown_sphere_radius_voxels",
    "sphere_radius()",
    "in_sphere(",
    "anchor_in_sphere(",
    "covering_site(",
];

/// What reads the **grown** radius: the one function and every wrapper of it.
const GROWN_NEEDLES: &[&str] = &[
    "sphere_radius_of(",
    "sphere_radius_at_row(",
    "sphere_radius_voxels_of(",
    "radius_of(",
    "radius_at_row(",
    "radius_voxels(",
    "radius_in_snapshot(",
    "inside_at_row(",
    "inside_sphere(",
    "inside_own_sphere(",
    "in_sphere_of(",
    "anchor_in_sphere_of(",
    "holds_point(",
    "Sphere::of(",
];

/// The readers decision 6 says grow with a Spire, `crate:file::function`.
const GROWS: &[&str] = &[
    // The mandates' areas: Attack's weak point in the target's sphere.
    "sim:attack.rs::weak_point",
    // Defend's engagement radius, picket ring and the threat it guards against.
    "sim:defend.rs::post_of",
    "sim:mandate.rs::request",
    "sim:programs.rs::flees",
    "sim:interpreter/cond.rs::live_row_threat",
    "sim:interpreter/cond.rs::live_row_repairs",
    "sim:interpreter/cond.rs::row_threat",
    "sim:interpreter/cond.rs::row_repairs",
    // Build's area and the drones' work reach, every physical-work path.
    "sim:mandate.rs::inside_sphere",
    "sim:mandate.rs::of",
    "sim:mandate.rs::buildable",
    "sim:mandate.rs::next_unfinished_target",
    "sim:mandate.rs::building_work",
    "sim:world.rs::queue_target",
    "sim:world/site.rs::check_wall",
    "sim:world/site.rs::wall_voxel",
    "sim:world/site.rs::refill_voxel",
    "sim:world/site.rs::build_job",
    "sim:world/site.rs::nearest_patient",
    "sim:world/site.rs::cracked_wall",
    "sim:world/site.rs::ruin_in_sphere",
    "sim:world/site.rs::repair_work",
    "sim:world/site.rs::salvage_work",
    "sim:world/combat.rs::wall_owner",
    "sim:world.rs::wreck_in_sphere",
    // Mine's area.
    "sim:mining.rs::remaining_yield",
    "sim:mining.rs::dig_in",
    // The sphere's coverage of vents and seams, and the anchor for the next
    // beacon.
    "sim:targeting.rs::radius_of",
    "sim:targeting.rs::in_sphere_of",
    "sim:targeting.rs::inside_own_sphere",
    "sim:targeting.rs::anchor_in_sphere_of",
    "sim:targeting.rs::on_vent_counted",
    "sim:targeting.rs::on_candidate",
    "sim:interpreter/exec.rs::site_is_legal",
    // The sphere's place in the sensor seam's sphere output.
    "sim:sight.rs::beacon_sensor",
    "sim:sight.rs::spheres",
    // The wrappers themselves.
    "sim:world.rs::sphere_radius_of",
    "sim:world.rs::sphere_radius_voxels_of",
    "sim:world.rs::sphere_radius_at_row",
    // The repair drones a guard's beacon reads in its sphere, and a
    // description's `UNCOVERED` test.
    "sim:world/combat.rs::repairs_at",
    "sim:targeting.rs::matches_pick",
    "sim:mandate.rs::holds_point",
    // The gateway's reads of the sim's sphere: `covered_by`, the placement
    // test it previews, and the verifier scope's licence half, which chooses.
    "gateway:targeting.rs::covering_beacon",
    "gateway:targeting.rs::read_placement",
    "gateway:surface.rs::push_own_licence_facts",
];

/// The readers decision 6 says keep the base row, by name.
const KEEPS_THE_BASE: &[&str] = &[
    // The one function and its wrappers, which choose between the base and
    // the grown radius.
    "sim:spire.rs::radius_voxels",
    "sim:spire.rs::radius_at_row",
    "sim:spire.rs::radius_of",
    "sim:spire.rs::radius_in_snapshot",
    "sim:spire.rs::inside_at_row",
    // The rules table: its fields (`<module>`, the struct), its load, the
    // grown radius summed from the base once at load, and their accessors.
    "sim:rules.rs::<module>",
    "sim:rules.rs::from_message",
    "sim:rules.rs::sphere_radius_of",
    "sim:rules.rs::grown_sphere_radius_of",
    "sim:rules.rs::sphere_radius_voxels",
    "sim:rules.rs::grown_sphere_radius_voxels",
    // Mapgen's twice-the-sphere reach rule and the contested zones, placed
    // against the base at generation.
    "sim:mapgen.rs::check_bands",
    "sim:mapgen.rs::place_contested",
    // The sphere of a site where no beacon stands yet: the `covering` spiral
    // (built once, at the base), its search, a new beacon's `on`, and the
    // targeting view's base tests.
    "sim:world.rs::new",
    "sim:targeting.rs::sphere_radius",
    "sim:targeting.rs::in_sphere",
    "sim:targeting.rs::radius_of",
    "sim:targeting.rs::covering_site",
    "sim:targeting.rs::anchor_in_sphere",
    "sim:targeting.rs::cover_counted",
    // A wall's owner: a beacon a Spire ever grew keeps the grown radius for
    // its walls, decision 6's "works stand", and every other the one function.
    "sim:world/combat.rs::wall_owner",
    // The gateway's previews of a `covering` site, and the verifier scope's
    // licence half, which reads the base to set only a grown radius.
    "gateway:surface/knowledge.rs::covering_site",
    "gateway:surface/knowledge.rs::place_of",
    "gateway:surface.rs::push_own_licence_facts",
];

/// Every `crate:file::function` of the sim's and the gateway's sources whose
/// shipped code (comments and test modules aside) holds one of `needles`.
#[allow(
    clippy::disallowed_methods,
    reason = "clippy.toml's own reason string sanctions read_dir when the listing is collected and sorted before use, which is what happens below, as tests/confinement.rs does"
)]
fn readers(needles: &[&str]) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for krate in ["sim", "gateway"] {
        let src = root().join("crates").join(krate).join("src");
        let mut stack = vec![src.clone()];
        while let Some(dir) = stack.pop() {
            let mut here: Vec<PathBuf> = std::fs::read_dir(&dir)
                .expect("a source directory")
                .map(|entry| entry.expect("an entry").path())
                .collect();
            here.sort();
            for path in here {
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    let text = std::fs::read_to_string(&path).expect("a source file");
                    let name = path
                        .strip_prefix(&src)
                        .expect("under src")
                        .to_string_lossy()
                        .replace('\\', "/");
                    scan(&format!("{krate}:{name}"), &text, needles, &mut found);
                }
            }
        }
    }
    found
}

/// The functions of one file's shipped code that hold one of `needles`, as
/// `file::function` (`file::<module>` outside every function).
fn scan(name: &str, text: &str, needles: &[&str], found: &mut BTreeSet<String>) {
    let mut function = String::from("<module>");
    let mut test_gate = false;
    for line in text.lines() {
        // A test module ends the shipped code; a lone test-only item does not.
        if test_gate && line.trim_start().starts_with("mod ") {
            break;
        }
        test_gate = line.trim_start().starts_with("#[cfg(test)]");
        let code = line.split("//").next().unwrap_or("");
        if let Some(at) = code.find("fn ")
            && (at == 0 || !code[..at].ends_with(|c: char| c.is_alphanumeric() || c == '_'))
        {
            let rest = &code[at + 3..];
            let end = rest
                .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            if end > 0 {
                rest[..end].clone_into(&mut function);
            }
        }
        if needles.iter().any(|needle| contains_word(code, needle)) {
            found.insert(format!("{name}::{function}"));
        }
    }
}

/// The readers check itself, over a set of readers: every reader is named in
/// one list or both (a chooser), and every name on a list is a reader.
fn named(base: &BTreeSet<String>, grows: &BTreeSet<String>) -> Result<(), String> {
    let keeps: BTreeSet<String> = KEEPS_THE_BASE.iter().map(|s| (*s).to_owned()).collect();
    let growing: BTreeSet<String> = GROWS.iter().map(|s| (*s).to_owned()).collect();
    let listed: BTreeSet<String> = keeps.union(&growing).cloned().collect();
    let readers: BTreeSet<String> = base.union(grows).cloned().collect();
    let unnamed: Vec<&String> = readers.difference(&listed).collect();
    if !unnamed.is_empty() {
        return Err(format!(
            "a reader of the sphere is in neither list: {unnamed:?}. Name it in GROWS if decision \
             6 says it grows with a Spire, or in KEEPS_THE_BASE if it does not"
        ));
    }
    let stale: Vec<&String> = listed.difference(&readers).collect();
    if !stale.is_empty() {
        return Err(format!("a name on a list reads no sphere: {stale:?}"));
    }
    let base_only: Vec<&String> = base
        .difference(grows)
        .filter(|r| growing.contains(*r))
        .collect();
    if !base_only.is_empty() {
        return Err(format!(
            "listed as growing, and reads only the base: {base_only:?}"
        ));
    }
    let grown_only: Vec<&String> = grows
        .difference(base)
        .filter(|r| keeps.contains(*r))
        .collect();
    if !grown_only.is_empty() {
        return Err(format!(
            "listed as keeping the base, and reads only the grown radius: {grown_only:?}"
        ));
    }
    Ok(())
}

#[test]
fn every_sphere_reader_is_named() {
    let base = readers(BASE_NEEDLES);
    let grows = readers(GROWN_NEEDLES);
    if let Err(why) = named(&base, &grows) {
        panic!("{why}");
    }
}

/// The check fails on a reader planted in neither list, whichever way it
/// reads: the planted source is scanned as a file of the sim would be.
#[test]
fn the_readers_check_fails_on_a_planted_reader() {
    for planted in [
        "fn sneaks(world: &World) -> i16 { world.rules().sphere_radius_voxels() }",
        "fn grows_quietly(world: &World, b: BeaconId) -> Fx { world.sphere_radius_of(b) }",
        "use crate::targeting::in_sphere;\nfn bare(g: &Ground<'_>, c: [Fx; 3], p: [Fx; 3]) -> bool { in_sphere(c, p) }",
    ] {
        let mut base = readers(BASE_NEEDLES);
        let mut grows = readers(GROWN_NEEDLES);
        scan("sim:planted.rs", planted, BASE_NEEDLES, &mut base);
        scan("sim:planted.rs", planted, GROWN_NEEDLES, &mut grows);
        assert!(
            named(&base, &grows).is_err(),
            "the planted reader passed: {planted}"
        );
    }
}

/// Whether `code` holds `needle` as a whole identifier: no identifier
/// character just before it, and none just after unless the needle itself
/// ends in punctuation (a call's `(`). So `sphere_radius_voxels` is not found
/// inside `grown_sphere_radius_voxels` or `sphere_radius_voxels_of`.
fn contains_word(code: &str, needle: &str) -> bool {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    let open_start = needle.starts_with(|c: char| !ident(c));
    let open_end = needle.ends_with(|c: char| !ident(c));
    let mut from = 0;
    while let Some(at) = code[from..].find(needle) {
        let start = from + at;
        let end = start + needle.len();
        let before = open_start || code[..start].chars().next_back().is_none_or(|c| !ident(c));
        let after = open_end || code[end..].chars().next().is_none_or(|c| !ident(c));
        if before && after {
            return true;
        }
        from = start + 1;
    }
    false
}

/// Decision 6, for Build (S3's plan, section 6: "what then lies outside the
/// sphere stands and stays powered, but no physical work reaches it until a
/// Spire is rebuilt"): a target in the grown ring is not bought, not counted
/// as work and not raised while the sphere is its base, and a rebuilt Spire
/// resumes it.
#[test]
fn a_target_a_fallen_spire_left_outside_waits_for_a_spire() {
    let mut world = licensed();
    let core = core(&world);
    let at = core_at(&world);
    let column = |voxels: i32| {
        world.standing_point([
            at[0].floor_voxels() + voxels,
            at[1].floor_voxels(),
            at[2].floor_voxels(),
        ])
    };
    let beside = column(1);
    let ring = column(-28);
    let spire = world
        .raise_structure(SEAT, core, StructureKind::ResonanceSpire, beside)
        .expect("room for a Spire");
    set_treasury(&mut world, 0);
    world
        .add_target(core, StructureKind::Autocannon.id(), ring)
        .expect("room for a target");
    let mut runner = Runner::new(world);
    assert!(runner.begin_push().unwrap());
    // The Spire falls before anything is bought.
    runner
        .world_mut()
        .request_damage(DamageOrder {
            target: DamageTarget::Structure(spire),
            amount: Damage::new(1_000_000).expect("a damage"),
            by: SeatId::new(1),
            source: None,
        })
        .expect("room");
    runner.step().unwrap();
    assert!(!runner.world().spire_on(core), "the Spire fell");
    runner.world_mut().set_treasury(SEAT, Money::new(RICH));
    let built_at = |world: &World| {
        let structures = world.structures();
        (0..structures.ids().len()).any(|row| {
            !structures.frees()[row]
                && structures.kinds()[row] == StructureKind::Autocannon.id()
                && structures.positions()[row][0].floor_voxels() == ring[0].floor_voxels()
                && structures.positions()[row][1].floor_voxels() == ring[1].floor_voxels()
        })
    };
    for _ in 0..600 {
        runner.step().unwrap();
    }
    assert!(
        !built_at(runner.world()),
        "nothing is bought outside the base sphere"
    );
    assert!(
        treasury(runner.world()).raw() >= RICH,
        "and nothing is spent: {:?}",
        treasury(runner.world())
    );

    // A rebuilt Spire grows the sphere again, and the work resumes.
    runner
        .world_mut()
        .raise_structure(SEAT, core, StructureKind::ResonanceSpire, beside)
        .expect("room for a Spire");
    for _ in 0..1_200 {
        runner.step().unwrap();
    }
    assert!(
        built_at(runner.world()),
        "the target is bought once the sphere holds it"
    );
}

/// No silent drop: a sealed `add_build_target` whose anchor lies outside the
/// beacon's sphere when its row commits is written, and waits for the sphere
/// (decision 6), rather than committing and writing nothing.
#[test]
fn a_target_written_outside_the_sphere_is_kept_and_waits() {
    let world = licensed();
    let at = core_at(&world);
    let ring = world.standing_point([
        at[0].floor_voxels() - 28,
        at[1].floor_voxels(),
        at[2].floor_voxels(),
    ]);
    let [x, y, z] = [
        ring[0].floor_voxels(),
        ring[1].floor_voxels(),
        ring[2].floor_voxels(),
    ];
    let route = format!(
        r#"{{"label":"beyond","interface":{{"beacon":{{"beacon_id":"b_00"}},"rows":[{{"add_build_target":{{"target":{{"blueprint_id":"autocannon","anchor":{{"voxel":{{"x":{x},"y":{y},"z":{z}}}}}}}}}}}]}},"timeout_ms":60000}}"#
    );
    let (runner, feed) = play(world, &route, 600);
    assert!(failures(&feed).is_empty(), "the row commits: {feed:?}");
    let targets = runner.world().targets();
    assert!(targets.anchors().contains(&ring), "the target is written");
    assert!(
        !feed
            .iter()
            .any(|event| event.kind == EventKind::StructureQueued),
        "and nothing is bought while the sphere does not hold it"
    );
}

/// The Spire's site keeps out of the beacon's protected areas, ground its
/// mandate will not build on: a protected box over the spiral's first ring
/// puts it on the first free column outside the box.
#[test]
fn the_spire_is_not_sited_in_a_protected_area() {
    let mut world = licensed();
    let core = core(&world);
    let at = core_at(&world);
    let [cx, cy] = [at[0].floor_voxels(), at[1].floor_voxels()];
    let corner = |dx: i32, dy: i32| {
        [
            i16::try_from(cx + dx).expect("on the map"),
            i16::try_from(cy + dy).expect("on the map"),
        ]
    };
    let shield =
        pharmakos_sim::tables::ColumnBox::new(corner(-2, -2), corner(2, 2)).expect("a box");
    world
        .add_area(core, pharmakos_sim::tables::AreaKind::Protected, at, shield)
        .expect("room for an area");
    let (runner, feed) = play(world, &queue_at_core("spire", "resonance_spire"), 400);
    assert!(failures(&feed).is_empty(), "{feed:?}");
    let structures = runner.world().structures();
    let row = (0..structures.ids().len())
        .find(|row| structures.kinds()[*row] == StructureKind::ResonanceSpire.id())
        .expect("the Spire");
    let spire = structures.positions()[row];
    assert!(
        !shield.contains(spire[0].floor_voxels(), spire[1].floor_voxels()),
        "the Spire stands outside the protected box"
    );
}

/// Decision 6, for walls (S3's `lic`, its fix pass; the beacon's grown flag):
/// a wall raised in the grown ring while the Spire stands keeps its builder's
/// ownership when the Spire falls, since works outside the shrunken sphere
/// "stand"; it still gets no work, a cracked voxel no repair, until a Spire is
/// rebuilt; and it goes neutral when its beacon dies, as every wall does.
#[test]
fn a_wall_in_the_grown_ring_stands_owned_and_unworked_until_a_spire_is_rebuilt() {
    let mut world = licensed();
    let core = core(&world);
    let at = core_at(&world);
    let beside = world.standing_point([
        at[0].floor_voxels() + 1,
        at[1].floor_voxels(),
        at[2].floor_voxels(),
    ]);
    let ring = world.standing_point([
        at[0].floor_voxels() - 30,
        at[1].floor_voxels(),
        at[2].floor_voxels(),
    ]);
    let voxel = [
        ring[0].floor_voxels(),
        ring[1].floor_voxels(),
        ring[2].floor_voxels(),
    ];
    let spire = world
        .raise_structure(SEAT, core, StructureKind::ResonanceSpire, beside)
        .expect("room for a Spire");
    assert!(
        world.beacons().grown().iter().any(|grown| *grown),
        "the core is flagged"
    );
    // A cracked wall voxel of seat 0's, raised in the ring while the Spire
    // stands; the core repairs walls below full.
    world
        .request_voxel_edit(pharmakos_sim::voxels::VoxelEdit::Set {
            at: voxel,
            material: pharmakos_sim::voxels::Material::wall(SEAT, true).expect("a wall"),
            by: None,
        })
        .expect("room for an edit");
    world.set_build_settings_directly(
        core,
        pharmakos_sim::site::BuildSettings {
            repair_threshold_pct: 100,
            rebuild_destroyed: false,
            terraform: pharmakos_sim::site::Terraform::None,
        },
    );
    let mut runner = Runner::new(world);
    assert!(runner.begin_push().unwrap());
    runner.step().unwrap();
    assert_eq!(
        runner.world().wall_owner(voxel),
        Some(SEAT),
        "owned under the Spire"
    );
    runner
        .world_mut()
        .request_damage(DamageOrder {
            target: DamageTarget::Structure(spire),
            amount: Damage::new(1_000_000).expect("a damage"),
            by: SeatId::new(1),
            source: None,
        })
        .expect("room");
    runner.step().unwrap();
    assert!(!runner.world().spire_on(core), "the Spire fell");
    assert_eq!(
        runner.world().wall_owner(voxel),
        Some(SEAT),
        "the wall stands, and is still seat 0's"
    );
    let cracked = |world: &World| {
        world
            .voxels()
            .get(voxel)
            .and_then(pharmakos_sim::voxels::Material::wall_of)
            .is_some_and(|(_, cracked)| cracked)
    };
    for _ in 0..1_200 {
        runner.step().unwrap();
    }
    assert!(
        cracked(runner.world()),
        "no drone repairs it outside the sphere"
    );

    // A rebuilt Spire brings the work back.
    runner
        .world_mut()
        .raise_structure(SEAT, core, StructureKind::ResonanceSpire, beside)
        .expect("room for a Spire");
    let mut repaired = false;
    for _ in 0..2_400 {
        runner.step().unwrap();
        if !cracked(runner.world()) {
            repaired = true;
            break;
        }
    }
    assert!(
        repaired,
        "the wall is repaired once the sphere holds it again"
    );
}

/// A wall of a beacon the Spire grew goes neutral when that beacon dies,
/// exactly as decision 42 has it for every wall.
#[test]
fn a_wall_of_a_grown_beacon_goes_neutral_when_the_beacon_dies() {
    let mut world = licensed();
    let core = core(&world);
    let at = core_at(&world);
    let beside = world.standing_point([
        at[0].floor_voxels() + 1,
        at[1].floor_voxels(),
        at[2].floor_voxels(),
    ]);
    let ring = world.standing_point([
        at[0].floor_voxels() - 30,
        at[1].floor_voxels(),
        at[2].floor_voxels(),
    ]);
    let voxel = [
        ring[0].floor_voxels(),
        ring[1].floor_voxels(),
        ring[2].floor_voxels(),
    ];
    world
        .raise_structure(SEAT, core, StructureKind::ResonanceSpire, beside)
        .expect("room for a Spire");
    world
        .request_voxel_edit(pharmakos_sim::voxels::VoxelEdit::Set {
            at: voxel,
            material: pharmakos_sim::voxels::Material::wall(SEAT, false).expect("a wall"),
            by: None,
        })
        .expect("room for an edit");
    let mut runner = Runner::new(world);
    assert!(runner.begin_push().unwrap());
    runner.step().unwrap();
    assert_eq!(runner.world().wall_owner(voxel), Some(SEAT));
    runner
        .world_mut()
        .request_damage(DamageOrder {
            target: DamageTarget::Beacon(core),
            amount: Damage::new(1_000_000).expect("a damage"),
            by: SeatId::new(1),
            source: None,
        })
        .expect("room");
    runner.step().unwrap();
    assert_eq!(
        runner.world().wall_owner(voxel),
        None,
        "its beacon died: nobody's"
    );
}

/// The grown flag rides the snapshot and the state hash: a world whose Spire
/// grew its core restores with the flag, and its hash is the saved one's.
#[test]
fn the_grown_flag_survives_a_save_and_a_restore() {
    let mut world = licensed();
    let core = core(&world);
    let at = core_at(&world);
    let beside = world.standing_point([
        at[0].floor_voxels() + 1,
        at[1].floor_voxels(),
        at[2].floor_voxels(),
    ]);
    let bare = world.state_hash();
    world
        .raise_structure(SEAT, core, StructureKind::ResonanceSpire, beside)
        .expect("room for a Spire");
    assert_ne!(world.state_hash(), bare);
    let bytes = Snapshot::capture(&world).to_bytes().expect("encoding");
    let mut restored = self::world();
    Snapshot::from_bytes(&bytes)
        .expect("decoding")
        .restore_into(&mut restored)
        .expect("restoring");
    assert_eq!(restored.beacons().grown(), world.beacons().grown());
    assert_eq!(restored.state_hash(), world.state_hash());
}
