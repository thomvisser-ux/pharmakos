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
use std::path::{Path, PathBuf};

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

    let (runner, feed) = play(world, &queue_at_core("spire", "resonance_spire"), 1_200);
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
    assert_eq!(
        treasury(world).raw(),
        before.raw() - cost.raw(),
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
        1_200,
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
        1_200,
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

/// The readers decision 6 says grow with a Spire: every function of
/// `crates/sim/src` that asks the one function ([`pharmakos_sim::spire`]) or
/// one of its wrappers, `file::function`.
const GROWS: &[&str] = &[
    // The mandates' areas: Attack's weak point in the target's sphere.
    "attack.rs::weak_point",
    // Defend's engagement radius, picket ring and the threat it guards against.
    "defend.rs::post_of",
    "mandate.rs::request",
    "programs.rs::flees",
    "interpreter/cond.rs::live_row_threat",
    "interpreter/cond.rs::live_row_repairs",
    "interpreter/cond.rs::row_threat",
    "interpreter/cond.rs::row_repairs",
    // Build's area and the drones' work reach.
    "mandate.rs::inside_sphere",
    "mandate.rs::of",
    "world/site.rs::check_wall",
    "world/site.rs::wall_voxel",
    "world/site.rs::refill_voxel",
    "world/site.rs::nearest_patient",
    "world/site.rs::cracked_wall",
    "world/site.rs::ruin_in_sphere",
    "world/site.rs::repair_work",
    "world/site.rs::salvage_work",
    "world.rs::wreck_in_sphere",
    // Mine's area.
    "mining.rs::remaining_yield",
    "mining.rs::dig_in",
    // The sphere's coverage of vents and seams, and the anchor for the next
    // beacon.
    "targeting.rs::radius_of",
    "targeting.rs::inside_own_sphere",
    // The sphere's place in the sensor seam's sphere output.
    "sight.rs::beacon_sensor",
    "sight.rs::spheres",
    // The wrappers themselves.
    "world.rs::sphere_radius_of",
    "world.rs::sphere_radius_voxels_of",
    "world.rs::sphere_radius_at_row",
];

/// The readers decision 6 says keep the base row, by name: every function
/// that reads `beacon.sphere_radius_voxels` itself.
const KEEPS_THE_BASE: &[&str] = &[
    // The one function and its wrappers, which choose between the base and the
    // grown radius.
    "spire.rs::radius_voxels",
    "spire.rs::radius_at_row",
    "spire.rs::radius_of",
    "spire.rs::radius_in_snapshot",
    "spire.rs::inside_at_row",
    // The rules table: its field (`<module>`, the struct), its load, the
    // grown radius summed from it once at load, and its accessor.
    "rules.rs::<module>",
    "rules.rs::from_message",
    "rules.rs::sphere_radius_of",
    "rules.rs::grown_sphere_radius_of",
    "rules.rs::sphere_radius_voxels",
    // Mapgen's twice-the-sphere reach rule and the contested zones, placed
    // against the base at generation.
    "mapgen.rs::check_bands",
    "mapgen.rs::place_contested",
    // The sphere of a site where no beacon stands yet: a `covering` spiral,
    // a new beacon's `on` and the targeting view's base test.
    "world.rs::new",
    "targeting.rs::sphere_radius",
    "targeting.rs::in_sphere",
    "targeting.rs::radius_of",
];

/// Every `file::function` of `crates/sim/src` whose code (comments and test
/// modules aside) contains one of `needles`.
#[allow(
    clippy::disallowed_methods,
    reason = "clippy.toml's own reason string sanctions read_dir when the listing is collected and sorted before use, which is what happens below, as tests/confinement.rs does"
)]
fn readers(needles: &[&str]) -> BTreeSet<String> {
    let src = root().join("crates").join("sim").join("src");
    let mut found = BTreeSet::new();
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
                scan(&src, &path, needles, &mut found);
            }
        }
    }
    found
}

fn scan(src: &Path, path: &Path, needles: &[&str], found: &mut BTreeSet<String>) {
    let text = std::fs::read_to_string(path).expect("a source file");
    let name = path
        .strip_prefix(src)
        .expect("under src")
        .to_string_lossy()
        .replace('\\', "/");
    let mut function = String::from("<module>");
    let mut test_gate = false;
    for line in text.lines() {
        // A test module ends the shipped code; a lone test-only item does not.
        if test_gate && line.starts_with("mod ") {
            break;
        }
        test_gate = line.starts_with("#[cfg(test)]");
        let code = line.split("//").next().unwrap_or("");
        if let Some(at) = code.find("fn ") {
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

#[test]
fn every_sphere_reader_is_named() {
    let base = readers(&["sphere_radius_voxels"]);
    let expected_base: BTreeSet<String> = KEEPS_THE_BASE.iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(
        base, expected_base,
        "a function reads the base sphere row: name it in KEEPS_THE_BASE if decision 6 says it \
         does not grow, or read the one function (`World::sphere_radius_of`) if it does"
    );
    let grows = readers(&[
        ".sphere_radius_of(",
        ".sphere_radius_at_row(",
        ".sphere_radius_voxels_of(",
        "spire::radius_of(",
        "spire::radius_at_row(",
        "spire::radius_voxels(",
        "spire::radius_in_snapshot(",
        "spire::inside_at_row(",
        "Sphere::of(",
        "radius_in_snapshot(",
    ]);
    let grows: BTreeSet<String> = grows
        .into_iter()
        .filter(|reader| !reader.starts_with("spire.rs::"))
        .collect();
    let expected_grows: BTreeSet<String> = GROWS.iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(
        grows, expected_grows,
        "a function reads the grown sphere: name it in GROWS"
    );
    assert!(
        base.intersection(&grows)
            .eq(["targeting.rs::radius_of".to_owned()].iter()),
        "no reader is in both lists but the targeting view's chooser, which reads the base for a \n         site where no beacon stands and the one function for a placed beacon"
    );
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
