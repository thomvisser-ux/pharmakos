// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! T14's acceptance: `$`, `kW`, the Quartermaster and the Ledger.
//!
//! Each test below is one line of the skeleton plan's T14 acceptance, under the
//! name the plan gives it, and each one asserts the **rule** rather than a
//! number wherever the rule is what was decided: the numbers are rules-table
//! data (AGENTS.md §12) and a test that pinned one would fail on every tuning
//! pull request without telling anybody anything.
//!
//! The one golden here, `tests/golden/economy/expected.settlement.txt`, is the
//! exception and is meant to be: it is a fixed three-round match read line by
//! line, so a tuning change *does* move it, and `tests/golden/economy/README.md`
//! says which kinds of diff mean what.

use pharmakos_sim::economy::{Urgency, percent_of, value_of};
use pharmakos_sim::events::{Event, EventKind};
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::math::quantity::{Hp, Kw, Money};
use pharmakos_sim::runner::Runner;
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::tables::{
    BeaconId, PRIORITY_HIGH, PRIORITY_LOW, PRIORITY_NORMAL, SeatId, StructureId, StructureKind,
    TargetKind, UnitKind,
};
use pharmakos_sim::world::{DamageOrder, DamageTarget, World, WorldConfig};
use pharmakos_sim::{MatchSettings, RulesTable, default_rules_path};
use std::fmt::Write as _;
use std::path::PathBuf;

/// A seeded match on the skeleton's map, with no harness walkers: T14's tests
/// are about a seat's own force, and fifty raiders per seat would drown it.
const SEED: u64 = 0x00A1_B2C3_D4E5_F607;

/// Three seats, which is v1's most, so the settlement's ladder has a middle.
const SEATS: u32 = 3;

fn rules() -> RulesTable {
    let path = default_rules_path().unwrap_or_else(|| PathBuf::from("rules/rules.v1.json"));
    RulesTable::load(&path).unwrap_or_else(|error| panic!("loading the rules table: {error}"))
}

/// A world with the starting force and nothing else.
fn world(segments: &[i32]) -> World {
    let rules = rules();
    World::new(&WorldConfig {
        match_seed: SEED,
        seats: SEATS,
        units_per_seat: 0,
        rules,
        match_settings: MatchSettings {
            segment_lengths_ms: segments.to_vec(),
            round_limit: 3,
        },
    })
    .unwrap_or_else(|error| panic!("generating the world: {error}"))
}

/// A seat's core: its lowest-id beacon, the rule the whole sim uses.
fn core_of(world: &World, seat: SeatId) -> BeaconId {
    let beacons = world.beacons();
    let mut best: Option<u32> = None;
    for row in 0..usize::try_from(beacons.len()).unwrap_or(0) {
        if beacons.seats().get(row).copied() == Some(seat.raw()) {
            let id = beacons.ids().get(row).copied().unwrap_or(0);
            if best.is_none_or(|held| id < held) {
                best = Some(id);
            }
        }
    }
    BeaconId::new(best.unwrap_or_else(|| panic!("seat {} has a core", seat.raw())))
}

fn treasury(world: &World, seat: SeatId) -> Money {
    let row = world
        .seat_row(seat)
        .unwrap_or_else(|| panic!("seat {} is seated", seat.raw()));
    world
        .seats()
        .treasuries()
        .get(row)
        .copied()
        .unwrap_or(Money::ZERO)
}

fn supply_and_draw(world: &World, seat: SeatId) -> (Kw, Kw) {
    let row = world
        .seat_row(seat)
        .unwrap_or_else(|| panic!("seat {} is seated", seat.raw()));
    (
        world
            .seats()
            .supplies()
            .get(row)
            .copied()
            .unwrap_or(Kw::ZERO),
        world.seats().draws().get(row).copied().unwrap_or(Kw::ZERO),
    )
}

fn dormant(world: &World, beacon: BeaconId) -> bool {
    let row = usize::try_from(beacon.raw()).unwrap_or(usize::MAX);
    world.beacons().dormant().get(row).copied().unwrap_or(false)
}

/// Run a Push to its end, collecting every event it reported.
fn play_segment(runner: &mut Runner, feed: &mut Vec<Event>) {
    assert!(runner.begin_push(), "the Push begins");
    feed.extend_from_slice(runner.events());
    runner.clear_events();
    while runner.step().is_some_and(|report| !report.segment_ended) {
        feed.extend_from_slice(runner.events());
        runner.clear_events();
    }
    feed.extend_from_slice(runner.events());
    runner.clear_events();
}

// ---------------------------------------------------------------------------
// `$`: value, the refund, and "paid means yours"
// ---------------------------------------------------------------------------

#[test]
fn value_follows_condition_at_the_audit_and_at_the_recycle_refund() {
    let mut world = world(&[20_000]);
    let seat = SeatId::new(0);
    let core = core_of(&world, seat);

    // Held value at the audit: the treasury plus every asset's build cost
    // scaled by its condition (item 18). Damage the core and the seat is worth
    // less by exactly the value the hit points carried — no more and no less.
    let before = world.held_value(seat);
    let full = world.beacon_max_hp(core);
    let half = Hp::new(full.raw().checked_div(2).unwrap_or(0));
    assert!(world.request_damage(DamageOrder {
        target: DamageTarget::Beacon(core),
        amount: half,
        by: SeatId::new(1),
    }));
    world.step(&mut pharmakos_sim::Enc::with_capacity(8 * 1024));
    let after = world.held_value(seat);
    let lost = before.raw().saturating_sub(after.raw());
    let expected = value_of(world.beacon_cost(), full.raw(), full.raw()).raw()
        - value_of(
            world.beacon_cost(),
            full.raw().saturating_sub(half.raw()),
            full.raw(),
        )
        .raw();
    assert_eq!(
        lost, expected,
        "held value fell by exactly the value the lost hit points carried"
    );

    // The recycle refund reads the same value: `recycle_refund_percent` of what
    // is left, not of what it cost new.
    let remaining = value_of(
        world.beacon_cost(),
        full.raw().saturating_sub(half.raw()),
        full.raw(),
    );
    let percent = world
        .rules()
        .message()
        .economy
        .as_ref()
        .map_or(0, |economy| economy.recycle_refund_percent);
    let purse = treasury(&world, seat);
    assert!(
        world.recycle_beacon(seat, core),
        "a seat may recycle its own"
    );
    let refund = treasury(&world, seat).raw().saturating_sub(purse.raw());
    assert_eq!(
        refund,
        percent_of(remaining, percent).raw(),
        "the refund is a percentage of remaining value, which is condition-scaled"
    );
    assert!(
        refund
            < percent_of(
                value_of(world.beacon_cost(), full.raw(), full.raw()),
                percent
            )
            .raw(),
        "and it is strictly less than the refund on an undamaged beacon"
    );
}

#[test]
fn paid_means_yours_survives_a_mid_build_death() {
    let mut world = world(&[60_000]);
    let seat = SeatId::new(0);
    let core = core_of(&world, seat);
    let at = world
        .beacons()
        .positions()
        .get(usize::try_from(core.raw()).unwrap_or(0))
        .copied()
        .unwrap_or_default();

    // Give the core a Build target one voxel along, so the drones reach it.
    world.set_writ(core, MandateKind::Build);
    let anchor = offset(at, 1);
    assert!(world.add_target(
        core,
        TargetKind::Build,
        StructureKind::Generator.id(),
        anchor,
        0
    ));
    let cost = world.structure_cost(StructureKind::Generator);
    let purse = treasury(&world, seat);

    let mut runner = Runner::new(world);
    let mut feed: Vec<Event> = Vec::new();
    assert!(runner.begin_push(), "the Push begins");
    // Step until the Quartermaster has paid for the target.
    let mut queued: Option<StructureId> = None;
    for _ in 0..200 {
        if runner.step().is_none() {
            break;
        }
        for event in runner.events() {
            if event.kind == EventKind::StructureQueued {
                queued = event.subject.map(|id| StructureId::new(id.index()));
            }
        }
        feed.extend_from_slice(runner.events());
        runner.clear_events();
        if queued.is_some() {
            break;
        }
    }
    let structure = queued.unwrap_or_else(|| panic!("the Quartermaster paid for the target"));

    // The money is gone, in full, from the instant the order committed.
    let charged = purse
        .raw()
        .saturating_sub(treasury(runner.world(), seat).raw());
    assert_eq!(charged, cost.raw(), "charged at commit, at the full price");

    // The thing is standing, unfinished, and worth almost nothing.
    let row = usize::try_from(structure.raw()).unwrap_or(usize::MAX);
    assert_eq!(
        runner.world().structures().building().get(row).copied(),
        Some(true),
        "it is still going up"
    );
    let unfinished = value_of(
        cost,
        runner
            .world()
            .structures()
            .hit_points()
            .get(row)
            .copied()
            .unwrap_or(Hp::ZERO)
            .raw(),
        runner
            .world()
            .structure_max_hp(StructureKind::Generator)
            .raw(),
    );
    assert!(
        unfinished.raw() < cost.raw(),
        "value follows condition even while the basis is the full price"
    );

    // Kill it mid-build. Nothing comes back: "there is no refund if it dies
    // mid-build" (item 23).
    let before = treasury(runner.world(), seat);
    assert!(runner.world_mut().request_damage(DamageOrder {
        target: DamageTarget::Structure(structure),
        amount: Hp::new(1_000_000),
        by: SeatId::new(1),
    }));
    runner.step();
    assert_eq!(
        treasury(runner.world(), seat).raw(),
        before.raw(),
        "paid means yours: a mid-build death refunds nothing"
    );
    assert!(
        !runner
            .world()
            .structures()
            .hit_points()
            .get(row)
            .is_some_and(|hp| hp.is_alive()),
        "and the structure really did die"
    );
}

// ---------------------------------------------------------------------------
// `kW`: dormancy and the brownout order
// ---------------------------------------------------------------------------

#[test]
fn a_dormant_beacon_powers_down_everything_homed_to_it() {
    let mut runner = Runner::new(world(&[20_000]));
    let seat = SeatId::new(0);
    let core = core_of(runner.world(), seat);

    // A seat's starting force is homed to its core, so a dormant core parks all
    // of it. The dormancy is set as a fixture rather than caused by loading the
    // grid: this test is about what dormancy *does*, and the brownout order has
    // a test of its own.
    assert!(runner.begin_push(), "the Push begins");
    // Give the units somewhere to be, so "parked" is a visible change.
    let away = [Fx::from_voxels(200), Fx::from_voxels(200), Fx::ZERO];
    for unit in 0..runner.world().units().len() {
        let row = usize::try_from(unit).unwrap_or(usize::MAX);
        if runner.world().units().seats().get(row).copied() == Some(seat.raw()) {
            runner.world_mut().send_unit_directly(unit, away);
        }
    }
    runner.world_mut().set_dormant(core, true);
    runner.step();
    let world = runner.world();

    let (supply, draw) = supply_and_draw(world, seat);
    assert_eq!(
        draw,
        Kw::ZERO,
        "a dormant beacon draws nothing, and neither does anything homed to it"
    );
    assert_eq!(
        supply,
        Kw::ZERO,
        "and its core's deep-bore surplus is off the grid with it"
    );

    // Its bound units parked where they stood, rather than finishing the leg —
    // **except the commander**, which a brownout never parks. See
    // `programs::program_for`: the way a seat fixes a shortfall is to walk the
    // commander to an at-risk beacon and raise its priority, so a commander
    // that went dark with the grid would make a shortfall unrecoverable.
    let commander = world.commander_of(seat);
    let mut parked = 0;
    for unit in 0..world.units().len() {
        let index = usize::try_from(unit).unwrap_or(usize::MAX);
        if world.units().seats().get(index).copied() != Some(seat.raw()) {
            continue;
        }
        let here = world.units().positions().get(index).copied();
        let there = world.units().destinations().get(index).copied();
        if unit == commander.raw() {
            assert_ne!(
                here, there,
                "the commander keeps walking through a brownout"
            );
            continue;
        }
        assert_eq!(here, there, "unit {unit} parked where it stands at 0 kW");
        parked += 1;
    }
    assert!(parked >= 3, "the starting drones all parked, not just one");

    // A seat whose core is awake is untouched by its neighbour's brownout.
    let other = SeatId::new(1);
    let (_, other_draw) = supply_and_draw(world, other);
    assert!(
        other_draw.raw() > 0,
        "dormancy is per beacon and per seat, not per map"
    );
}

#[test]
fn the_brownout_order_is_priority_then_distance_then_the_core_last() {
    let mut runner = Runner::new(world(&[20_000]));
    let seat = SeatId::new(0);
    let core = core_of(runner.world(), seat);
    let at = runner
        .world()
        .beacons()
        .positions()
        .get(usize::try_from(core.raw()).unwrap_or(0))
        .copied()
        .unwrap_or_default();

    // Three more beacons of this seat: one near on normal priority, one far on
    // normal, and one nearer on low. The order has to shed the low one first
    // however near it stands, then the furthest normal one, and the core last.
    let near = runner
        .world_mut()
        .place_beacon_directly(seat, offset(at, 4), MandateKind::Build, PRIORITY_NORMAL)
        .unwrap_or_else(|| panic!("room for a beacon"));
    let far = runner
        .world_mut()
        .place_beacon_directly(seat, offset(at, 20), MandateKind::Build, PRIORITY_NORMAL)
        .unwrap_or_else(|| panic!("room for a beacon"));
    let low = runner
        .world_mut()
        .place_beacon_directly(seat, offset(at, 6), MandateKind::Build, PRIORITY_LOW)
        .unwrap_or_else(|| panic!("room for a beacon"));

    assert!(runner.begin_push(), "the Push begins");
    runner.step();
    let world = runner.world();

    // The grid cannot carry four beacons plus the starting force on one core
    // surplus, so something is dark — and the order says which.
    assert!(
        dormant(world, low),
        "lowest priority sheds first, however near the core it stands"
    );
    let shed_far_before_near = !dormant(world, near) || dormant(world, far);
    assert!(
        shed_far_before_near,
        "within a priority band, the furthest from the core sheds first"
    );
    assert!(
        !dormant(world, core),
        "the core is last: a key-core always powers its own beacon"
    );
}

#[test]
fn the_autocannon_is_never_shed() {
    let mut runner = Runner::new(world(&[20_000]));
    let seat = SeatId::new(0);
    let core = core_of(runner.world(), seat);
    let at = runner
        .world()
        .beacons()
        .positions()
        .get(usize::try_from(core.raw()).unwrap_or(0))
        .copied()
        .unwrap_or_default();

    // An autocannon on the core, finished. It runs off the deep bore, outside
    // the grid: it adds nothing to the draw, so it can never be the thing that
    // browns a grid out, and nothing ever powers it down.
    assert!(runner.begin_push(), "the Push begins");
    runner.step();
    let (_, before) = supply_and_draw(runner.world(), seat);
    runner
        .world_mut()
        .raise_structure(seat, core, StructureKind::Autocannon, offset(at, 1))
        .unwrap_or_else(|| panic!("room for a structure"));
    runner.step();
    let (_, after) = supply_and_draw(runner.world(), seat);
    assert_eq!(
        after, before,
        "the autocannon runs off the deep bore and draws nothing from the grid"
    );
    assert!(
        !dormant(runner.world(), core),
        "and it never took the core down with it"
    );
}

/// A place `voxels` east of `at`, on the ground.
fn offset(at: [Fx; 3], voxels: i16) -> [Fx; 3] {
    [at[0].saturating_add(Fx::from_voxels(voxels)), at[1], at[2]]
}

// ---------------------------------------------------------------------------
// The Quartermaster
// ---------------------------------------------------------------------------

#[test]
fn no_beacon_starves_another_within_a_band() {
    let mut world = world(&[60_000]);
    let seat = SeatId::new(0);
    let core = core_of(&world, seat);
    let at = world
        .beacons()
        .positions()
        .get(usize::try_from(core.raw()).unwrap_or(0))
        .copied()
        .unwrap_or_default();
    let second = world
        .place_beacon_directly(seat, offset(at, 6), MandateKind::Build, PRIORITY_HIGH)
        .unwrap_or_else(|| panic!("room for a beacon"));

    // Both beacons want a Generator: the same band, the same price. The writ is
    // set after the beacons exist, because setting it clears the target list.
    world.set_writ(core, MandateKind::Build);
    world.set_writ(second, MandateKind::Build);
    for beacon in [core, second] {
        assert!(world.add_target(
            beacon,
            TargetKind::Build,
            StructureKind::Generator.id(),
            offset(at, 2),
            0
        ));
    }

    let mut runner = Runner::new(world);
    let mut feed: Vec<Event> = Vec::new();
    play_segment(&mut runner, &mut feed);

    let mut served: Vec<u32> = Vec::new();
    for event in &feed {
        if event.kind == EventKind::StructureQueued
            && let Some(subject) = event.subject
        {
            let row = usize::try_from(subject.index()).unwrap_or(usize::MAX);
            if let Some(home) = runner.world().structures().homes().get(row).copied() {
                served.push(home);
            }
        }
    }
    assert!(
        served.len() >= 2,
        "both beacons were paid for within one segment, not one for ever: {served:?}"
    );
    assert!(
        served.contains(&core.raw()) && served.contains(&second.raw()),
        "the round robin reached both beacons: {served:?}"
    );
}

#[test]
fn the_urgency_ladder_is_the_specs_order() {
    // Spec section 7: "defend under attack, then repair, units, build, mine".
    // The ids are ascending in that order, which is what the Quartermaster
    // phase's sort relies on.
    let names: Vec<&str> = Urgency::ALL.iter().map(|band| band.name()).collect();
    assert_eq!(
        names,
        ["defend_under_attack", "repair", "units", "build", "mine"]
    );
    for band in Urgency::ALL {
        assert_eq!(Urgency::from_id(band.id()), Some(band), "{band:?}");
    }
}

// ---------------------------------------------------------------------------
// Survey-lite
// ---------------------------------------------------------------------------

#[test]
fn a_survey_beacon_fields_its_scout_count_and_stops_there() {
    // Spec section 6's Survey row names "scout count" as a setting, so this is
    // the one mandate the player gives a number to and the fabricator fills it
    // exactly — a count, not a backlog. The writ is set before the count
    // because switching a writ clears the old mandate's settings (item 20).
    let mut world = world(&[60_000]);
    let seat = SeatId::new(0);
    let core = core_of(&world, seat);
    let at = world
        .beacons()
        .positions()
        .get(usize::try_from(core.raw()).unwrap_or(0))
        .copied()
        .unwrap_or_default();
    world.set_writ(core, MandateKind::Survey);
    world.set_scouts(core, 1);
    assert!(
        world.add_target(core, TargetKind::Probe, 0, offset(at, 10), 4),
        "the probe area fits"
    );

    let mut runner = Runner::new(world);
    let mut feed: Vec<Event> = Vec::new();
    play_segment(&mut runner, &mut feed);

    let fabricated = feed
        .iter()
        .filter(|event| {
            event.kind == EventKind::UnitFabricated
                && event.seat == Some(seat)
                && event.value == i64::from(UnitKind::Scout.id())
        })
        .count();
    assert_eq!(
        fabricated, 1,
        "the fabricator filled the scout count exactly once and then stopped"
    );
    assert_eq!(
        living_scouts(runner.world(), core),
        1,
        "and the beacon is holding exactly the count it was given"
    );
}

#[test]
fn a_sighting_stores_the_tick_it_was_taken_at_so_the_lull_adds_nothing() {
    // Item 61: a sighting's age "accrues on match game time and runs across
    // Pushes; the Lull and the recap add nothing to it". That is true for free
    // only because the record stores the **tick** and the age is derived — a
    // stored age would have to be advanced by somebody, and a Lull consumes no
    // tick, so whoever forgot would have made the Lull free.
    let mut world = world(&[20_000, 20_000]);
    let watcher = SeatId::new(0);
    let neighbour = SeatId::new(1);
    let theirs = core_of(&world, neighbour);
    let at = world
        .beacons()
        .positions()
        .get(usize::try_from(theirs.raw()).unwrap_or(0))
        .copied()
        .unwrap_or_default();
    // An eye of the watcher's, planted beside the watched seat's core, so the
    // watcher's sphere covers something that is not its own. High priority so
    // that the brownout order cannot take this test's eye away from it: the
    // beacon furthest from the core sheds first within a band, and this is by
    // a long way the furthest.
    world
        .place_beacon_directly(watcher, offset(at, 4), MandateKind::None, PRIORITY_HIGH)
        .unwrap_or_else(|| panic!("room for a beacon"));

    let mut runner = Runner::new(world);
    let mut feed: Vec<Event> = Vec::new();
    play_segment(&mut runner, &mut feed);

    let seen = sightings_of(runner.world(), watcher);
    assert!(
        !seen.is_empty(),
        "the watcher's sphere recorded the other seat's assets"
    );
    assert!(
        seen.iter().all(|(owner, _)| *owner != watcher.raw()),
        "a seat never records a sighting of its own — it knows where its things are"
    );

    // Freeze the clock: the recap and the Lull are states, not durations.
    let before_tick = runner.tick().raw();
    let before = seen.clone();
    assert!(runner.end_recap(), "the recap closes into the next Lull");
    assert_eq!(
        runner.tick().raw(),
        before_tick,
        "the recap and the Lull consumed no tick"
    );
    assert_eq!(
        sightings_of(runner.world(), watcher),
        before,
        "so no sighting aged across them: the record is a tick, not an age"
    );
}

/// How many living scouts are homed to `beacon`.
fn living_scouts(world: &World, beacon: BeaconId) -> u32 {
    let units = world.units();
    let mut total: u32 = 0;
    for row in 0..usize::try_from(units.len()).unwrap_or(0) {
        if units.homes().get(row).copied() == Some(beacon.raw())
            && units.kinds().get(row).copied() == Some(UnitKind::Scout.id())
            && units.hit_points().get(row).is_some_and(|hp| hp.is_alive())
        {
            total = total.saturating_add(1);
        }
    }
    total
}

/// One seat's sightings as `(owner, tick it was taken at)`, in table order.
fn sightings_of(world: &World, seat: SeatId) -> Vec<(u8, u32)> {
    let table = world.sightings();
    let owners = table.owners();
    let seen = table.seen_at();
    let mut out: Vec<(u8, u32)> = Vec::new();
    for (row, who) in table.seats().iter().enumerate() {
        if *who == seat.raw() {
            out.push((
                owners.get(row).copied().unwrap_or(0),
                seen.get(row).copied().unwrap_or(0),
            ));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Kill credit
// ---------------------------------------------------------------------------

#[test]
fn kill_credit_counts_only_other_seats_and_clamps_to_the_hit_points_removed() {
    let mut world = world(&[20_000]);
    let owner = SeatId::new(0);
    let core = core_of(&world, owner);
    let mut enc = pharmakos_sim::Enc::with_capacity(8 * 1024);

    // A seat's own damage credits nobody.
    assert!(world.request_damage(DamageOrder {
        target: DamageTarget::Beacon(core),
        amount: Hp::new(100),
        by: owner,
    }));
    world.step(&mut enc);
    assert!(
        world.credit().is_empty(),
        "a seat does not take credit for hurting its own"
    );

    // Another seat's does, clamped to what was there to take.
    let full = world.beacon_max_hp(core).raw();
    assert!(world.request_damage(DamageOrder {
        target: DamageTarget::Beacon(core),
        amount: Hp::new(full.saturating_mul(10)),
        by: SeatId::new(1),
    }));
    world.step(&mut enc);
    // The beacon died on that tick, so the counters were settled and cleared.
    assert!(
        world.credit().is_empty(),
        "a settled death leaves no row behind"
    );
    let credited: Vec<i64> = world
        .events()
        .iter()
        .filter(|event| event.kind == EventKind::KillCredited)
        .map(|event| event.value)
        .collect();
    assert_eq!(
        credited.iter().sum::<i64>(),
        world.beacon_cost().raw(),
        "the split sums to the destroyed thing's full build cost (item 23)"
    );
}

#[test]
fn a_split_is_apportioned_by_largest_remainder_with_ties_to_the_lowest_seat() {
    use pharmakos_sim::credit::apportion;
    let mut out: Vec<(u8, i64)> = Vec::new();

    // Ten between two equal damagers: five each, no remainder to hand out.
    apportion(10, &[(0, 1), (1, 1)], &mut out);
    assert_eq!(out, [(0, 5), (1, 5)]);

    // Ten between three equal damagers: three each and one left over, which
    // goes to the lowest seat id because every remainder is equal.
    apportion(10, &[(0, 1), (1, 1), (2, 1)], &mut out);
    assert_eq!(out, [(0, 4), (1, 3), (2, 3)]);
    assert_eq!(out.iter().map(|(_, share)| share).sum::<i64>(), 10);

    // And the shares follow the damage, not the seat order.
    apportion(100, &[(0, 1), (1, 9)], &mut out);
    assert_eq!(out, [(0, 10), (1, 90)]);
}

// ---------------------------------------------------------------------------
// The settlement golden, and the bench
// ---------------------------------------------------------------------------

/// A fixed three-round match, settled at each recap, read line by line.
#[test]
fn the_settlement_ledger_matches_its_golden() {
    let mut runner = Runner::new(world(&[120_000, 120_000, 120_000]));
    let mut feed: Vec<Event> = Vec::new();
    let mut body = String::new();
    // The four state columns are the seat's state at the **close of the
    // round**, not at the instant of the event: a ledger is read after the
    // fact, and re-deriving a mid-round treasury would mean replaying the
    // segment. The `value` column is the event's own number and is the one
    // that is about the moment it happened.
    body.push_str("# round\tseat\tevent\tvalue\tclosing_treasury\tsupply\tdraw\tclosing_held\n");
    for round in 1..=3_u32 {
        feed.clear();
        play_segment(&mut runner, &mut feed);
        for event in &feed {
            if !matches!(
                event.kind,
                EventKind::Settled
                    | EventKind::OreDelivered
                    | EventKind::StructureQueued
                    | EventKind::UnitFabricated
                    | EventKind::BeaconBrownedOut
                    | EventKind::BeaconRevived
            ) {
                continue;
            }
            let seat = event.seat.unwrap_or(SeatId::NEUTRAL);
            let (supply, draw) = supply_and_draw(runner.world(), seat);
            writeln!(
                body,
                "{round}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                seat.raw(),
                event.kind.name(),
                event.value,
                treasury(runner.world(), seat).raw(),
                supply.raw(),
                draw.raw(),
                runner.world().held_value(seat).raw(),
            )
            .unwrap_or_else(|error| panic!("writing the ledger: {error}"));
        }
        if !runner.end_recap() {
            break;
        }
    }
    // The closing audit: what every seat holds after the final settlement.
    for seat in 0..u8::try_from(SEATS).unwrap_or(3) {
        let id = SeatId::new(seat);
        let (supply, draw) = supply_and_draw(runner.world(), id);
        writeln!(
            body,
            "final\t{seat}\taudit\t0\t{}\t{}\t{}\t{}",
            treasury(runner.world(), id).raw(),
            supply.raw(),
            draw.raw(),
            runner.world().held_value(id).raw(),
        )
        .unwrap_or_else(|error| panic!("writing the ledger: {error}"));
    }

    write_actual("settlement.txt", &body);
    let expected = repo_root()
        .join("tests")
        .join("golden")
        .join("economy")
        .join("expected.settlement.txt");
    let Ok(golden) = std::fs::read_to_string(&expected) else {
        // The `golden` step is what fails a missing golden (rule 1 of
        // tests/golden/README.md); the producer's job is to produce.
        return;
    };
    assert_eq!(
        golden.replace("\r\n", "\n"),
        body,
        "the settlement ledger moved. tests/golden/economy/README.md says what a diff there \
         means; `cargo xtask golden --bless` accepts it once the pull request says why."
    );
}

/// The tick bench, reported against item 64's five-number shape — **as a
/// number, not a gate** (AGENTS.md §9 item 11).
///
/// It reports **counted work**, never milliseconds (AGENTS.md §4.5): a
/// deterministic crate may not read a clock, and a bench inside one that
/// reported a duration would be the first thing to do it. The counts below are
/// the terms item 64's cost model is over — units, seats, beacons, voxel edits
/// and decision ticks — so S1 can multiply them by its own measured nanoseconds
/// without this crate ever knowing what a nanosecond is.
///
/// **Four of the five are counted and the fifth is reported as zero, honestly.**
/// Item 64's voxel-edit term is priced per crater, and destruction is S2's;
/// the only voxel writes at the skeleton are a mining drone's digs, and nothing
/// counts them because no event carries one. The line says `voxel_edits=0`
/// rather than leaving the term out, so S2 sees the slot it has to fill rather
/// than an omission it has to notice.
/// `spends` and `ore_deliveries` are two extra counts the economy makes
/// available: a fixture with no Build target and one mining drone per core is
/// **meant** to report `spends=0`, because "headroom is the normal state"
/// (item 22) and a beacon whose drones keep up buys nothing.
#[test]
fn the_tick_bench_reports_counted_work() {
    // The settlement golden's own segment length, deliberately: a mining
    // drone's round trip is `economy.mining_load_voxels` digs at
    // `economy.mining_ms_per_voxel` plus the walk, which is most of two
    // thousand ticks, so a shorter segment would report a spend count of zero
    // and tell S1 nothing about the work a tick actually does.
    let mut runner = Runner::new(world(&[120_000]));
    let mut feed: Vec<Event> = Vec::new();
    play_segment(&mut runner, &mut feed);

    let ticks = runner.tick().raw();
    let units = runner.world().units().len();
    let beacons = runner.world().beacons().len();
    let structures = runner.world().structures().len();
    let decisions = ticks
        .checked_div(5)
        .unwrap_or(0)
        .saturating_mul(SEATS.min(3));
    let spends = feed
        .iter()
        .filter(|event| {
            matches!(
                event.kind,
                EventKind::UnitFabricated | EventKind::StructureQueued
            )
        })
        .count();
    let deliveries = feed
        .iter()
        .filter(|event| event.kind == EventKind::OreDelivered)
        .count();

    // Printed rather than asserted, because it is a measurement and not a
    // budget. `cargo test -p pharmakos-sim --test economy -- --nocapture` reads
    // it out.
    println!(
        "tick-bench\tticks={ticks}\tunit_ticks={}\tbeacon_ticks={}\tstructure_ticks={}\t\
         decision_ticks={decisions}\tvoxel_edits=0\tspends={spends}\t\
         ore_deliveries={deliveries}",
        u64::from(units).saturating_mul(u64::from(ticks)),
        u64::from(beacons).saturating_mul(u64::from(ticks)),
        u64::from(structures).saturating_mul(u64::from(ticks)),
    );
    assert!(ticks > 0, "the segment played");
    assert!(
        units >= SEATS.saturating_mul(4),
        "every occupied seat fielded its starting force"
    );
}

fn repo_root() -> PathBuf {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.parent()
        .and_then(std::path::Path::parent)
        .map(PathBuf::from)
        .unwrap_or(here)
}

fn write_actual(name: &str, body: &str) {
    let dir = target_dir().join("golden").join("economy");
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|error| panic!("creating {}: {error}", dir.display()));
    let path = dir.join(format!("actual.{name}"));
    std::fs::write(&path, body)
        .unwrap_or_else(|error| panic!("writing {}: {error}", path.display()));
}

/// Cargo's target directory, the way every other producer in this crate finds
/// it: `CARGO_TARGET_DIR` when it is set, and `<repo>/target` otherwise.
fn target_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("CARGO_TARGET_DIR")
        && !dir.is_empty()
    {
        return PathBuf::from(dir);
    }
    repo_root().join("target")
}
