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
//! One test joined them at T22a: the Quartermaster holding a fabricator order
//! while the headroom cannot run it, then filling it once a Generator brings
//! the supply back (decisions-log item 123 (2) 7).
//!
//! The grid's S1 rulings joined them with S1's `grid` lane: the brownout skips
//! a beacon whose shed relieves nothing, a priority raise relights a dark
//! beacon when a lower shed covers it, the commander walks through a blackout
//! drawing nothing (decisions-log item 127 (5) to (8)), a beacon's key-core
//! supplies exactly its base (S1's plan, decision 12), and the dark-load read
//! the gateway's shortfall line takes (item 134 (2) (c)).
//!
//! The one golden here, `tests/golden/economy/expected.settlement.txt`, is the
//! exception and is meant to be: it is a fixed three-round match read line by
//! line, so a tuning change *does* move it, and `tests/golden/economy/README.md`
//! says which kinds of diff mean what.

use pharmakos_sim::economy::{Urgency, percent_of, value_of};
use pharmakos_sim::events::{Event, EventKind};
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::math::quantity::{Hp, Kw, Money};
use pharmakos_sim::power::{DarkLoad, PowerError, PowerRules, dark_load};
use pharmakos_sim::runner::Runner;
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::tables::{
    BeaconId, PRIORITY_HIGH, PRIORITY_LOW, PRIORITY_NORMAL, SeatId, StructureId, StructureKind,
    TargetKind, UnitKind,
};
use pharmakos_sim::voxels::{Material, Richness};
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
    world_with(rules(), segments)
}

/// [`world`] over a table of the caller's.
fn world_with(rules: RulesTable, segments: &[i32]) -> World {
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

/// The committed table with `power.core_surplus_kw` set to `kw`, and
/// nothing else changed.
///
/// The power tests size their deficits from the load homed to a beacon, and
/// where the committed surplus is the wrong size for the arrangement under
/// test they say so by building this variant rather than by fielding a crowd:
/// the same device `crates/gamectl/tests/operator.rs`'s power-short variant
/// uses, done on the decoded message rather than the text.
fn rules_with_core_surplus(kw: u32) -> RulesTable {
    let mut message = rules().message().clone();
    let power = message
        .power
        .as_mut()
        .unwrap_or_else(|| panic!("the committed table has a power block"));
    power.core_surplus_kw = kw;
    RulesTable::from_message(&message)
        .unwrap_or_else(|error| panic!("the varied table is a table: {error}"))
}

/// The `kW` rows the power tests size their fixtures from, read from the
/// committed table so that a tuning change moves the fixture with it.
#[derive(Clone, Copy, Debug)]
struct Grid {
    /// `power.core_surplus_kw`.
    surplus: i32,
    /// `power.revive_margin_kw`.
    margin: i32,
    /// `power.kw_per_unit`.
    per_unit: i32,
    /// How many units a seat starts with: the commander and its drones.
    starting_units: i32,
    /// `structures.survey_post.draw_kw`, the smallest load a test can home to
    /// a beacon one step at a time.
    post: i32,
    /// `structures.mortar.draw_kw`.
    mortar: i32,
}

impl Grid {
    fn of(rules: &RulesTable) -> Grid {
        let message = rules.message();
        let power = message
            .power
            .as_ref()
            .unwrap_or_else(|| panic!("a power block"));
        let units = message
            .units
            .as_ref()
            .unwrap_or_else(|| panic!("a units block"));
        let structures = message
            .structures
            .as_ref()
            .unwrap_or_else(|| panic!("a structures block"));
        let kw = |value: u32| i32::try_from(value).unwrap_or(i32::MAX);
        Grid {
            surplus: kw(power.core_surplus_kw),
            margin: kw(power.revive_margin_kw),
            per_unit: kw(power.kw_per_unit),
            starting_units: kw(units
                .starting_build_drones
                .saturating_add(units.starting_mining_drones)
                .saturating_add(1)),
            post: structures.survey_post.map_or(0, |row| kw(row.draw_kw)),
            mortar: structures.mortar.map_or(0, |row| kw(row.draw_kw)),
        }
    }

    /// What a seat's starting force draws, homed to its core.
    fn starting_load(self) -> i32 {
        self.per_unit.saturating_mul(self.starting_units)
    }
}

/// The events of one kind about one beacon, as the ticks they happened on.
fn ticks_of(feed: &[Event], kind: EventKind, beacon: BeaconId) -> Vec<u32> {
    feed.iter()
        .filter(|event| {
            event.kind == kind && event.subject.is_some_and(|id| id.index() == beacon.raw())
        })
        .map(|event| event.tick.raw())
        .collect()
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

/// The demo's F2, reproduced (item 126 (3); S1's plan, the `fixs` lane) and
/// cured (the `mine` lane): a seat mining its starting seam delivered ore in
/// round 1 and nothing after, with the same carried playbook. The cause was
/// the skeleton's dig rule: a drone dug only the **exposed** ore voxel of a
/// column, standing level with it on undisturbed dirt, so once a seam's top
/// layer was gone no stand was level with the next and the search answered
/// nothing while ore was still in the ground. At the demo that was 24 voxels,
/// the $ 64 and $ 32 of round 1.
///
/// The cure is the Mine rule (S1-38, item 127 (9), decision 6): a beacon whose
/// `dig_max_depth` reaches the seam's floor works it below the rim, from
/// pit-safe stands (`pharmakos_sim::mining`). Committed red and ignored by
/// `fixs`; un-ignored here. Its last assertion changed with the cure, and says
/// so: the pit-safe rule leaves the ramps a pit needs in the ground, so "every
/// ore voxel left can still be dug" is no longer the claim. The claim is the
/// demo's: the seat keeps earning after round 1, from below the rim, and no
/// drone strands itself doing it.
#[test]
fn a_starting_seam_yields_more_than_its_exposed_rim() {
    let mut fixture = world(&[180_000]);
    let seat = SeatId::new(0);
    let core = core_of(&fixture, seat);
    fixture.set_writ(core, MandateKind::Mine);
    fixture.set_mine_settings(
        core,
        pharmakos_sim::mining::MineSettings {
            dig_max_depth: 3,
            ..pharmakos_sim::mining::MineSettings::default()
        },
    );
    let pristine = world(&[180_000]);
    let mut runner = Runner::new(fixture);
    let mut delivered: Vec<usize> = Vec::new();
    for _ in 0..3 {
        let mut feed = Vec::new();
        play_segment(&mut runner, &mut feed);
        delivered.push(
            feed.iter()
                .filter(|event| event.kind == EventKind::OreDelivered && event.seat == Some(seat))
                .count(),
        );
        assert!(runner.end_recap(), "the recap closes into the next Lull");
    }
    assert!(
        delivered.iter().all(|count| *count > 0),
        "the seat earns from its starting seam in every round: {delivered:?}"
    );

    // Dug below the rim: some ore voxel under a footprint column's original
    // top is gone, which the skeleton's rule could never reach.
    let world = runner.world();
    let row = usize::try_from(core.raw()).unwrap_or(usize::MAX);
    let held = world
        .beacons()
        .seams()
        .get(row)
        .copied()
        .unwrap_or(u32::MAX);
    let starting = pristine
        .features()
        .features()
        .iter()
        .filter(|feature| feature.kind == pharmakos_sim::features::FeatureKind::Seam)
        .min_by_key(|feature| {
            let [x, y] = feature.anchor;
            let at = pristine
                .beacons()
                .positions()
                .get(row)
                .copied()
                .unwrap_or([Fx::ZERO; 3]);
            let [ax, ay, _] = at.map(Fx::floor_voxels);
            let dx = i64::from(x.saturating_sub(ax));
            let dy = i64::from(y.saturating_sub(ay));
            dx * dx + dy * dy
        })
        .unwrap_or_else(|| panic!("the map has a seam"));
    let mut below_rim: u32 = 0;
    for column in &starting.footprint {
        for depth in 1..4 {
            let at = [column.x, column.y, column.top - depth];
            let was = pristine.voxels().get(at).and_then(Material::ore_richness);
            let now = world.voxels().get(at).and_then(Material::ore_richness);
            if was.is_some() && now.is_none() {
                below_rim += 1;
            }
        }
    }
    assert!(
        below_rim > 0,
        "the core works its starting seam below the rim (held seam {held}); ore deliveries per round: {delivered:?}"
    );

    // Pit-safe: no unit of the seat ends the match parked as sealed in.
    let states = world.router().states();
    for (unit, owner) in world.units().seats().iter().enumerate() {
        if *owner == seat.raw() {
            assert_ne!(
                states.get(unit).copied(),
                Some(pharmakos_sim::WalkState::Sealed.id()),
                "unit {unit} stranded itself; ore deliveries per round: {delivered:?}"
            );
        }
    }
}

/// The latent restore bug the S1 plan names (`fixs`): `World::unit_limit` is
/// documented as derived from the **starting** count, and a restore used to
/// derive it from the **restored** count, so a match saved after fabricating
/// resumed with more room than the unbroken run and could fabricate a unit the
/// unbroken run held. Written red first, against the old expression.
#[test]
fn a_restored_match_keeps_the_unbroken_runs_unit_limit() {
    // A Mine beacon beside seat 0's core asks the Quartermaster for its
    // drones, which is what puts fabricated rows in the unit table.
    let mut fixture = world(&[60_000]);
    let seat = SeatId::new(0);
    let core = core_of(&fixture, seat);
    let at = fixture
        .beacons()
        .positions()
        .get(usize::try_from(core.raw()).unwrap_or(usize::MAX))
        .copied()
        .unwrap_or_else(|| panic!("the core has a place"));
    fixture
        .place_beacon_directly(seat, offset(at, 6), MandateKind::Mine, PRIORITY_NORMAL)
        .unwrap_or_else(|| panic!("the beacon table has room"));
    let mut runner = Runner::new(fixture);
    let starting = runner.world().units().len();
    let limit = runner.world().unit_limit();
    let mut feed = Vec::new();
    play_segment(&mut runner, &mut feed);
    let fabricated = runner.world().units().len();
    assert!(
        fabricated > starting,
        "the fixture has to fabricate for the bug to show: {starting} units before the Push, \
         {fabricated} after"
    );
    assert_eq!(
        runner.world().unit_limit(),
        limit,
        "the unbroken run's ceiling does not move as it fabricates"
    );

    let saved = pharmakos_sim::snapshot::Snapshot::capture(runner.world());
    let mut resumed = world(&[60_000]);
    saved
        .restore_into(&mut resumed)
        .unwrap_or_else(|error| panic!("the save restores: {error}"));
    assert_eq!(
        resumed.unit_limit(),
        limit,
        "a resumed match has exactly the room the unbroken run has left, not a fresh \
         allowance on top of what it had already fabricated"
    );
    assert_eq!(
        resumed.state_hash(),
        runner.world().state_hash(),
        "and it is the same world"
    );
}

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
    // The **rounding** spelled out rather than delegated. Both assertions
    // above run `value_of` and `percent_of` on each side, so a change to the
    // rule they share would move both sides together and neither would go
    // red. Here the arithmetic is written from item 18's sentence instead —
    // build cost times current hit points, divided by full hit points,
    // truncated toward zero, and the divide after the multiply so the
    // truncation happens once — so a rounding change fails here.
    let cost = world.beacon_cost().raw();
    let left = i64::from(full.raw().saturating_sub(half.raw()));
    let spelled = cost
        .saturating_mul(left)
        .checked_div(i64::from(full.raw()))
        .unwrap_or(0);
    assert_eq!(
        remaining.raw(),
        spelled,
        "remaining value is `cost * hp / max_hp`, truncated toward zero"
    );
    assert_eq!(
        refund,
        spelled
            .saturating_mul(i64::from(percent))
            .checked_div(100)
            .unwrap_or(0),
        "and the refund is that many percent of it, truncated the same way"
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
    // A seat's starting force is homed to its core, so a dormant core parks all
    // of it. The dormancy is set as a fixture rather than caused by loading the
    // grid: this test is about what dormancy *does*, and the brownout order has
    // a test of its own.
    //
    // The fixture has to **stay** dormant through the settle that follows it,
    // and a shed core revives once the load homed to it leaves
    // `power.revive_margin_kw` of its surplus spare (its surplus comes back
    // with it; `power::revive_cost`). At the committed table the starting force
    // leaves more than that, so the core would light straight back up. The
    // surplus is therefore cut to one kilowatt short of the margin over the
    // starting force's load: a dormant core stays dark, and a lit one would
    // not be short.
    let grid = Grid::of(&rules());
    assert!(grid.margin > 0, "a revive margin to stay under: {grid:?}");
    let surplus = grid
        .starting_load()
        .saturating_add(grid.margin)
        .saturating_sub(1);
    let rules = rules_with_core_surplus(u32::try_from(surplus).unwrap_or(0));
    let mut runner = Runner::new(world_with(rules, &[20_000]));
    let seat = SeatId::new(0);
    let core = core_of(runner.world(), seat);

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
    assert!(
        dormant(world, core),
        "the fixture held: the starting force leaves the core less than the \
         margin spare, so it did not revive"
    );

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
    // **except the commander**, which a brownout never parks (item 127 (5);
    // `programs::program_for`, and
    // `the_commander_walks_through_a_blackout_drawing_nothing` below): the
    // only way a seat acts on a shortfall is to walk the commander to a beacon
    // and interface on site, so a commander that went dark with the grid
    // would make a brownout a lockout. What a priority raise then does is
    // item 127 (7)'s: `a_raise_relights_a_dark_beacon_when_a_lower_shed_covers_it`.
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

    // Four more beacons of this seat: one near on normal priority, one far on
    // normal, one nearer on low, and one on high. The deficit is sized so that
    // **exactly two** sheds settle it, which is what makes every term of the
    // order readable from the final state rather than implied by it.
    //
    // A beacon is net zero through its own key-core, so the deficit is built
    // from **load**: one Mortar homed to each new beacon. With supply at the
    // core's deep-bore surplus alone (10 kW at the committed table) and a draw
    // of the four starting units plus four Mortars (3 kW each), shedding the
    // low one and then the furthest normal one balances the grid — so `near`,
    // `spare` and the core are still lit, and each of them is lit for a
    // different reason.
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
    let spare = runner
        .world_mut()
        .place_beacon_directly(seat, offset(at, 22), MandateKind::Build, PRIORITY_HIGH)
        .unwrap_or_else(|| panic!("room for a beacon"));
    for (beacon, voxels) in [(near, 5_i16), (far, 21), (low, 7), (spare, 23)] {
        runner
            .world_mut()
            .raise_structure(seat, beacon, StructureKind::Mortar, offset(at, voxels))
            .unwrap_or_else(|| panic!("room for a structure"));
    }
    // Exactly two sheds settle it, at whatever the table says: two Mortars
    // shed leave the draw within the supply, and one would not.
    let grid = Grid::of(&rules());
    let two_left = grid
        .starting_load()
        .saturating_add(grid.mortar.saturating_mul(2));
    let three_left = two_left.saturating_add(grid.mortar);
    assert!(
        two_left <= grid.surplus && grid.surplus < three_left,
        "the fixture has to be short by more than one Mortar and by no more than \
         two for the order to be readable: {grid:?}"
    );

    assert!(runner.begin_push(), "the Push begins");
    runner.step();
    let world = runner.world();

    let (supply, draw) = supply_and_draw(world, seat);
    assert_eq!(
        draw.raw(),
        two_left,
        "two sheds took their Mortars off the grid, and only two: supply \
         {supply:?} draw {draw:?}"
    );

    // Every term of the order, asserted positively rather than as an
    // implication that a single shed would satisfy vacuously.
    assert!(
        dormant(world, low),
        "lowest priority sheds first, however near the core it stands"
    );
    assert!(
        dormant(world, far),
        "then the furthest from the core within a band"
    );
    assert!(
        !dormant(world, near),
        "and not the nearer one of the same band, which is what makes the \
         second term distance and not id"
    );
    assert!(
        !dormant(world, spare),
        "a high-priority beacon outlives both normal ones, however far out it \
         stands: priority outranks distance"
    );
    assert!(
        !dormant(world, core),
        "the core is last: a key-core always powers its own beacon"
    );
}

#[test]
fn a_placed_beacon_adds_no_draw() {
    // Item 113 (5): a beacon is net zero through its own key-core (decisions
    // log section 2.3, item 90), so placing one with nothing homed to it moves
    // neither column. Two identical matches, one with the beacon placed, read
    // tick by tick: whatever else the grid does, it does in both.
    let seat = SeatId::new(0);
    let mut plain = Runner::new(world(&[20_000]));
    let mut placed = Runner::new(world(&[20_000]));
    let core = core_of(placed.world(), seat);
    let at = placed
        .world()
        .beacons()
        .positions()
        .get(usize::try_from(core.raw()).unwrap_or(0))
        .copied()
        .unwrap_or_default();
    let beacon = placed
        .world_mut()
        .place_beacon_directly(seat, offset(at, 8), MandateKind::None, PRIORITY_NORMAL)
        .unwrap_or_else(|| panic!("room for a beacon"));
    assert!(
        plain.begin_push() && placed.begin_push(),
        "the Pushes begin"
    );
    for tick in 0..40 {
        plain.step();
        placed.step();
        assert_eq!(
            supply_and_draw(placed.world(), seat),
            supply_and_draw(plain.world(), seat),
            "tick {tick}: a placed beacon with nothing homed moves neither column"
        );
        assert!(
            !dormant(placed.world(), beacon),
            "tick {tick}: and it is lit"
        );
    }

    // **Never shed by its own draw**, however little headroom there is: at a
    // table whose surplus is exactly the starting force's load the headroom is
    // 0 kW, and before item 113 (5) a placed beacon's base would have been
    // shed at the first settle.
    let grid = Grid::of(&rules());
    let tight = rules_with_core_surplus(u32::try_from(grid.starting_load()).unwrap_or(0));
    let mut runner = Runner::new(world_with(tight, &[20_000]));
    let beacon = runner
        .world_mut()
        .place_beacon_directly(seat, offset(at, 8), MandateKind::None, PRIORITY_LOW)
        .unwrap_or_else(|| panic!("room for a beacon"));
    let mut feed: Vec<Event> = Vec::new();
    assert!(runner.begin_push(), "the Push begins");
    for _ in 0..40 {
        runner.step();
        feed.extend_from_slice(runner.events());
        runner.clear_events();
    }
    let (supply, draw) = supply_and_draw(runner.world(), seat);
    assert_eq!(
        (supply.raw(), draw.raw()),
        (grid.starting_load(), grid.starting_load()),
        "the grid runs at 0 kW headroom, with the placed beacon on it"
    );
    assert!(
        !dormant(runner.world(), beacon),
        "a beacon with nothing homed is never shed by its own draw"
    );
    assert!(
        ticks_of(&feed, EventKind::BeaconBrownedOut, beacon).is_empty(),
        "not even for one tick"
    );
}

#[test]
fn a_shed_core_revives_once_its_load_leaves_the_margin_spare() {
    // Item 113 (5): a shed core brings its deep-bore surplus back with it, so
    // its revival is weighed with that surplus credited. Spec section 5 loses
    // the surplus for good only when the core is **destroyed**; before this, a
    // shed core never revived, because a total blackout's headroom is 0 and
    // the margin is positive.
    //
    // The core is shed by its own homed load — its starting force plus enough
    // Survey Posts to outrun the surplus — and then the posts are knocked down
    // one a tick. It must stay dark while the load leaves less than the margin
    // spare (including while the load is **within** the surplus, which is the
    // margin doing its anti-flicker work), revive on the first tick the load
    // leaves at least the margin, and never be shed and revived on alternating
    // ticks.
    let grid = Grid::of(&rules());
    assert!(
        grid.post > 0 && grid.margin > grid.post,
        "the margin has to span more than one post for the dark-but-not-short \
         band to be visible: {grid:?}"
    );
    let mut runner = Runner::new(world(&[60_000]));
    let seat = SeatId::new(0);
    let core = core_of(runner.world(), seat);
    let at = runner
        .world()
        .beacons()
        .positions()
        .get(usize::try_from(core.raw()).unwrap_or(0))
        .copied()
        .unwrap_or_default();
    let mut posts: Vec<StructureId> = Vec::new();
    let mut load = grid.starting_load();
    let mut voxels: i16 = 2;
    while load <= grid.surplus {
        posts.push(
            runner
                .world_mut()
                .raise_structure(seat, core, StructureKind::SurveyPost, offset(at, voxels))
                .unwrap_or_else(|| panic!("room for a structure")),
        );
        load = load.saturating_add(grid.post);
        voxels = voxels.saturating_add(1);
    }

    let mut feed: Vec<Event> = Vec::new();
    assert!(runner.begin_push(), "the Push begins");
    runner.step();
    feed.extend_from_slice(runner.events());
    runner.clear_events();
    assert!(
        dormant(runner.world(), core),
        "a core whose homed load outruns its surplus is shed, last of all"
    );
    assert_eq!(
        supply_and_draw(runner.world(), seat),
        (Kw::ZERO, Kw::ZERO),
        "a total blackout: the surplus left the grid with the core"
    );

    let mut dark_within_supply = false;
    let mut revived_at: Option<i32> = None;
    while let Some(post) = posts.pop() {
        assert!(runner.world_mut().request_damage(DamageOrder {
            target: DamageTarget::Structure(post),
            amount: Hp::new(1_000_000),
            by: seat,
        }));
        load = load.saturating_sub(grid.post);
        runner.step();
        feed.extend_from_slice(runner.events());
        runner.clear_events();
        let spare = grid.surplus.saturating_sub(load);
        let dark = dormant(runner.world(), core);
        if revived_at.is_none() {
            assert_eq!(
                dark,
                spare < grid.margin,
                "load {load} kW leaves {spare} kW of the surplus spare: dark \
                 exactly while that is under the margin"
            );
            if dark && spare >= 0 {
                dark_within_supply = true;
            }
            if !dark {
                revived_at = Some(load);
            }
        } else {
            assert!(!dark, "once revived, a falling load keeps it lit");
        }
    }
    assert!(
        dark_within_supply,
        "the fixture passed through a load the surplus covers but without the \
         margin, and the core stayed dark there"
    );
    assert!(revived_at.is_some(), "the core revived");

    // And it stays lit: no shed at the next settle, nor at any after it.
    for _ in 0..40 {
        runner.step();
        feed.extend_from_slice(runner.events());
        runner.clear_events();
    }
    assert!(!dormant(runner.world(), core), "still lit");
    assert_eq!(
        ticks_of(&feed, EventKind::BeaconBrownedOut, core).len(),
        1,
        "shed once: {feed:?}"
    );
    assert_eq!(
        ticks_of(&feed, EventKind::BeaconRevived, core).len(),
        1,
        "revived once, and never shed and revived on alternating ticks"
    );
}

#[test]
fn a_beacon_whose_shed_relieves_nothing_is_skipped_and_stays_lit() {
    // Item 127 (8), the register's S1-33, amending spec section 5's Dormant
    // row: the brownout order skips a beacon whose shed relieves nothing, so
    // idle expansions stay lit in a deficit. A beacon is net zero through its
    // key-core, so shedding one with nothing homed to it relieves 0 kW; and
    // shedding one whose Generator out-supplies its load makes the deficit
    // worse, which relieves nothing either (the next test). Before S1 both
    // were shed, ahead of the core (item 113 (5)); this test is that one
    // inverted.
    let grid = Grid::of(&rules());
    let seat = SeatId::new(0);

    // (a) The deficit is the core's own: its homed load outruns its surplus.
    // The idle beacon is skipped and stays lit; the core is shed, and is the
    // only beacon shed.
    let mut runner = Runner::new(world(&[20_000]));
    let core = core_of(runner.world(), seat);
    let at = runner
        .world()
        .beacons()
        .positions()
        .get(usize::try_from(core.raw()).unwrap_or(0))
        .copied()
        .unwrap_or_default();
    let idle = runner
        .world_mut()
        .place_beacon_directly(seat, offset(at, 10), MandateKind::None, PRIORITY_NORMAL)
        .unwrap_or_else(|| panic!("room for a beacon"));
    let mut load = grid.starting_load();
    let mut voxels: i16 = 2;
    while load <= grid.surplus {
        runner
            .world_mut()
            .raise_structure(seat, core, StructureKind::SurveyPost, offset(at, voxels))
            .unwrap_or_else(|| panic!("room for a structure"));
        load = load.saturating_add(grid.post);
        voxels = voxels.saturating_add(1);
    }
    let mut feed: Vec<Event> = Vec::new();
    assert!(runner.begin_push(), "the Push begins");
    for _ in 0..20 {
        runner.step();
        feed.extend_from_slice(runner.events());
        runner.clear_events();
    }
    let order: Vec<u32> = feed
        .iter()
        .filter(|event| event.kind == EventKind::BeaconBrownedOut && event.seat == Some(seat))
        .filter_map(|event| event.subject.map(pharmakos_sim::knowledge::AssetId::index))
        .collect();
    assert_eq!(
        order,
        [core.raw()],
        "the core is shed, once, and the idle beacon, whose shed relieves nothing, is not"
    );
    assert!(
        !dormant(runner.world(), idle),
        "the idle beacon stays lit through the deficit"
    );
    assert!(dormant(runner.world(), core), "and the core is dark");

    // (b) The deficit is a loaded expansion's: the idle beacon, on low
    // priority, comes first in the order and is skipped; the loaded one is
    // shed and settles the deficit. The idle beacon never goes dark, not even
    // for the tick a revival would have taken.
    let mut runner = Runner::new(world(&[20_000]));
    let idle = runner
        .world_mut()
        .place_beacon_directly(seat, offset(at, 10), MandateKind::None, PRIORITY_LOW)
        .unwrap_or_else(|| panic!("room for a beacon"));
    let loaded = runner
        .world_mut()
        .place_beacon_directly(seat, offset(at, 4), MandateKind::None, PRIORITY_NORMAL)
        .unwrap_or_else(|| panic!("room for a beacon"));
    let mut load = grid.starting_load();
    let mut voxels: i16 = 12;
    while load <= grid.surplus {
        runner
            .world_mut()
            .raise_structure(seat, loaded, StructureKind::SurveyPost, offset(at, voxels))
            .unwrap_or_else(|| panic!("room for a structure"));
        load = load.saturating_add(grid.post);
        voxels = voxels.saturating_add(1);
    }
    let mut feed: Vec<Event> = Vec::new();
    assert!(runner.begin_push(), "the Push begins");
    for _ in 0..20 {
        runner.step();
        feed.extend_from_slice(runner.events());
        runner.clear_events();
    }
    assert!(
        ticks_of(&feed, EventKind::BeaconBrownedOut, idle).is_empty(),
        "the idle beacon is skipped: {feed:?}"
    );
    assert!(
        ticks_of(&feed, EventKind::BeaconRevived, idle).is_empty(),
        "so it has nothing to revive from"
    );
    assert_eq!(
        ticks_of(&feed, EventKind::BeaconBrownedOut, loaded).len(),
        1,
        "the loaded beacon is shed, once"
    );
    assert!(!dormant(runner.world(), idle), "the idle beacon ends lit");
    assert!(dormant(runner.world(), loaded), "the loaded one stays dark");
    assert!(
        !dormant(runner.world(), core),
        "and the core never went dark"
    );
}

#[test]
fn a_beacon_whose_shed_would_deepen_the_deficit_is_skipped() {
    // Item 127 (8)'s other half: a shed that makes the deficit worse relieves
    // nothing either, so it is skipped like an idle beacon's.
    let grid = Grid::of(&rules());
    let seat = SeatId::new(0);
    let mut runner = Runner::new(world(&[20_000]));
    let core = core_of(runner.world(), seat);
    let at = runner
        .world()
        .beacons()
        .positions()
        .get(usize::try_from(core.raw()).unwrap_or(0))
        .copied()
        .unwrap_or_default();

    // A low-priority expansion whose only homed asset is a Generator on the
    // zone's vent. It is first in the order, and shedding it would take its Generator's
    // output off the grid and relieve nothing; it is skipped, and the loaded
    // beacon after it is shed instead.
    let rules = rules();
    let (first, second) = vent_stands(runner.world(), at);
    let output = vent_output(runner.world(), &rules, first, second);
    let tapper = runner
        .world_mut()
        .place_beacon_directly(seat, offset(at, 10), MandateKind::None, PRIORITY_LOW)
        .unwrap_or_else(|| panic!("room for a beacon"));
    runner
        .world_mut()
        .raise_structure(seat, tapper, StructureKind::Generator, first)
        .unwrap_or_else(|| panic!("room for the Generator"));
    let loaded = runner
        .world_mut()
        .place_beacon_directly(seat, offset(at, 4), MandateKind::None, PRIORITY_NORMAL)
        .unwrap_or_else(|| panic!("room for a beacon"));
    let mut load = grid.starting_load();
    let mut voxels: i16 = -2;
    while load <= grid.surplus.saturating_add(output) {
        runner
            .world_mut()
            .raise_structure(seat, loaded, StructureKind::Mortar, offset(at, voxels))
            .unwrap_or_else(|| panic!("room for a structure"));
        load = load.saturating_add(grid.mortar);
        voxels = voxels.saturating_sub(1);
    }
    let mut feed: Vec<Event> = Vec::new();
    assert!(runner.begin_push(), "the Push begins");
    for tick in 0..20 {
        runner.step();
        feed.extend_from_slice(runner.events());
        runner.clear_events();
        let (supply, draw) = supply_and_draw(runner.world(), seat);
        assert!(
            supply.raw() >= draw.raw(),
            "tick {tick}: the settle ends with no deficit: {supply:?} < {draw:?}"
        );
    }
    assert!(
        ticks_of(&feed, EventKind::BeaconBrownedOut, tapper).is_empty(),
        "the Generator's beacon is skipped, since its shed would deepen the deficit"
    );
    assert!(!dormant(runner.world(), tapper), "and it stays lit");
    assert!(dormant(runner.world(), loaded), "the loaded beacon is shed");
    assert!(!dormant(runner.world(), core), "and the core stays lit");
}

/// A raise's fixture: seat 0's core, a beacon `raised` on low priority with
/// `raised_posts` Survey Posts homed to it, and a beacon `other` on normal
/// priority with `other_posts`, the two loads together outrunning the core's
/// spare surplus so the first settle sheds `raised` (low priority sheds
/// first). Seat 0 seals one step: walk to `raised` and set its priority to
/// HIGH, on site.
struct Raise {
    runner: Runner,
    raised: BeaconId,
    other: BeaconId,
    feed: Vec<Event>,
    /// The tick the `set_priority` row committed.
    committed: u32,
}

fn raise_fixture(raised_posts: i32, other_posts: i32) -> Raise {
    let rules = rules();
    let seat = SeatId::new(0);
    let mut world = world(&[30_000]);
    let core = core_of(&world, seat);
    let at = world
        .beacons()
        .positions()
        .get(usize::try_from(core.raw()).unwrap_or(0))
        .copied()
        .unwrap_or_default();
    let raised = world
        .place_beacon_directly(seat, offset(at, 4), MandateKind::None, PRIORITY_LOW)
        .unwrap_or_else(|| panic!("room for a beacon"));
    let other = world
        .place_beacon_directly(seat, offset(at, 8), MandateKind::None, PRIORITY_NORMAL)
        .unwrap_or_else(|| panic!("room for a beacon"));
    let mut voxels: i16 = -2;
    for (beacon, posts) in [(raised, raised_posts), (other, other_posts)] {
        for _ in 0..posts {
            world
                .raise_structure(seat, beacon, StructureKind::SurveyPost, offset(at, voxels))
                .unwrap_or_else(|| panic!("room for a structure"));
            voxels = voxels.saturating_sub(1);
        }
    }
    let ordinal = world
        .beacons()
        .ordinals()
        .get(usize::try_from(raised.raw()).unwrap_or(usize::MAX))
        .copied()
        .unwrap_or_else(|| panic!("the raised beacon has an ordinal"));
    let name = pharmakos_sim::tables::own_beacon_name(ordinal);
    let text = format!(
        r#"{{
  "schema_version": {{"major": 1, "minor": 0}},
  "meta": {{"title": "Raise a dark beacon", "author_kind": "HUMAN"}},
  "kind": "PLAYBOOK",
  "declarative": {{
    "route": [
      {{"label": "raise", "interface": {{"beacon": {{"beacon_id": "{name}"}},
        "rows": [{{"set_priority": "HIGH"}}]}},
        "timeout_ms": 20000, "on_fail": {{"action": "SKIP"}}}}
    ],
    "handlers": []
  }},
  "on_death": {{"on_respawn": "CONTINUE", "max_deaths_before_fallback": 2}},
  "fallback": {{"hold": {{"at": {{"safest": {{}}}}}}}}
}}"#
    );
    let playbook = pharmakos_proto::json::decode(&text)
        .unwrap_or_else(|error| panic!("the raise is canonical gp.v1 JSON: {error:?}"));
    let plan = pharmakos_sim::interpreter::Plan::compile(&playbook, &rules)
        .unwrap_or_else(|error| panic!("the raise compiles: {error:?}"));
    let mut runner = Runner::new(world);
    runner
        .seal_playbook(seat, plan)
        .unwrap_or_else(|error| panic!("a Lull takes a seal: {error:?}"));
    assert!(runner.begin_push(), "the Push begins");
    let mut feed: Vec<Event> = Vec::new();
    feed.extend_from_slice(runner.events());
    runner.clear_events();
    let mut committed: Option<u32> = None;
    for _ in 0..600 {
        runner.step();
        let events = runner.events().to_vec();
        runner.clear_events();
        if committed.is_none() {
            committed = events
                .iter()
                .find(|event| event.kind == EventKind::RowCommitted && event.seat == Some(seat))
                .map(|event| event.tick.raw());
            if committed.is_none() {
                assert!(
                    dormant(runner.world(), raised) && !dormant(runner.world(), other),
                    "before the raise the low-priority beacon is the dark one: {events:?}"
                );
            }
        }
        feed.extend(events);
        if committed.is_some_and(|tick| runner.world().tick().raw() > tick.saturating_add(40)) {
            break;
        }
    }
    let committed = committed.unwrap_or_else(|| panic!("the raise committed: {feed:?}"));
    let priority = runner
        .world()
        .beacons()
        .priorities()
        .get(usize::try_from(raised.raw()).unwrap_or(usize::MAX))
        .copied();
    assert_eq!(priority, Some(PRIORITY_HIGH), "the row raised the beacon");
    Raise {
        runner,
        raised,
        other,
        feed,
        committed,
    }
}

/// The most Survey Posts whose draw fits the core's spare surplus (the surplus
/// less the starting force's load), and at least one.
fn posts_within_spare() -> i32 {
    let grid = Grid::of(&rules());
    let spare = grid.surplus.saturating_sub(grid.starting_load());
    assert!(
        grid.post > 0 && spare >= grid.post,
        "the spare surplus holds a post: {grid:?}"
    );
    let mut posts: i32 = 0;
    while posts.saturating_add(1).saturating_mul(grid.post) <= spare {
        posts = posts.saturating_add(1);
    }
    posts
}

#[test]
fn a_raise_relights_a_dark_beacon_when_a_lower_shed_covers_it() {
    // Item 127 (7), the register's S1-22: raising a dark beacon's priority
    // re-applies the brownout order, relighting it **at once** when shedding
    // lit beacons of lower priority covers its load, with draw no higher than
    // supply after the swap, so the commander's walk there pays off. Before
    // S1 a raise moved the beacon later in the brownout order and earlier in
    // the revival order and relit nothing.
    //
    // The spare surplus is the core's surplus less the starting force's load.
    // `raised` carries as many posts as fit it, so it fits once `other` is
    // shed; `other` carries one post, so the two together do not fit.
    let raised_posts = posts_within_spare();
    let fixture = raise_fixture(raised_posts, 1);
    let world = fixture.runner.world();
    let seat = SeatId::new(0);

    let relit = ticks_of(&fixture.feed, EventKind::BeaconRevived, fixture.raised);
    assert_eq!(
        relit.len(),
        1,
        "the raised beacon relights once: {:?}",
        fixture.feed
    );
    let at = relit.first().copied().unwrap_or(0);
    assert!(
        at >= fixture.committed && at <= fixture.committed.saturating_add(1),
        "at once: at the settle that follows the commit ({at} against {})",
        fixture.committed
    );
    assert_eq!(
        ticks_of(&fixture.feed, EventKind::BeaconBrownedOut, fixture.other),
        [at],
        "and the lower-priority beacon is shed in its place, on the same settle"
    );
    assert!(
        !dormant(world, fixture.raised),
        "the raised beacon stays lit"
    );
    assert!(dormant(world, fixture.other), "and the other stays dark");
    let (supply, draw) = supply_and_draw(world, seat);
    assert!(
        draw.raw() <= supply.raw(),
        "draw is no higher than supply after the swap: {supply:?} {draw:?}"
    );
    assert_eq!(
        ticks_of(&fixture.feed, EventKind::BeaconBrownedOut, fixture.raised).len(),
        1,
        "and nothing flickers: the raised beacon was shed once, before the raise"
    );
}

#[test]
fn a_raise_relights_nothing_when_no_lower_shed_covers_it() {
    // The twin: `raised` carries one post more than the spare surplus, so
    // even with every lower-priority beacon shed its load does not fit. The
    // raise commits, the priority is HIGH, and the grid is left exactly as it
    // was: no swap is half-made, and no event reports one.
    let raised_posts = posts_within_spare().saturating_add(1);
    let fixture = raise_fixture(raised_posts, 1);
    let world = fixture.runner.world();
    assert!(
        dormant(world, fixture.raised),
        "the raised beacon stays dark"
    );
    assert!(!dormant(world, fixture.other), "and the other stays lit");
    assert!(
        ticks_of(&fixture.feed, EventKind::BeaconRevived, fixture.raised).is_empty(),
        "no revival is reported: {:?}",
        fixture.feed
    );
    assert!(
        ticks_of(&fixture.feed, EventKind::BeaconBrownedOut, fixture.other).is_empty(),
        "and no shed"
    );
}

/// Seat 0's core and where it stands.
fn core_and_site(world: &World) -> (BeaconId, [Fx; 3]) {
    let core = core_of(world, SeatId::new(0));
    let at = world
        .beacons()
        .positions()
        .get(usize::try_from(core.raw()).unwrap_or(usize::MAX))
        .copied()
        .unwrap_or_else(|| panic!("the core has a position"));
    (core, at)
}

/// Home `posts` Survey Posts to `beacon`, west of `at` from `*voxels` down.
fn raise_posts(world: &mut World, beacon: BeaconId, at: [Fx; 3], posts: i32, voxels: &mut i16) {
    for _ in 0..posts {
        world
            .raise_structure(
                SeatId::new(0),
                beacon,
                StructureKind::SurveyPost,
                offset(at, *voxels),
            )
            .unwrap_or_else(|| panic!("room for a structure"));
        *voxels = voxels.saturating_sub(1);
    }
}

/// Step once after the Push begins with `dark` set dormant, and return the
/// runner and that settle's events.
fn settle_once_with_dark(world: World, dark: &[BeaconId]) -> (Runner, Vec<Event>) {
    let mut runner = Runner::new(world);
    assert!(runner.begin_push(), "the Push begins");
    for beacon in dark {
        runner.world_mut().set_dormant(*beacon, true);
    }
    runner.clear_events();
    runner.step();
    let feed = runner.events().to_vec();
    runner.clear_events();
    (runner, feed)
}

#[test]
fn a_revival_the_swap_undoes_in_one_settle_reports_nothing() {
    // The review of the `grid` lane: the revival runs before the swap in one
    // settle, so a low beacon revived on the margin can be shed at once for a
    // normal one the margin held dark. The end state is the brownout order's,
    // and the feed reports each seat's net change once per settle: the normal
    // beacon's revival, and nothing at all for the low one, which was dark
    // before the settle and is dark after it.
    let grid = Grid::of(&rules());
    let spare = grid.surplus.saturating_sub(grid.starting_load());
    let posts = posts_within_spare();
    let load = grid.post.saturating_mul(posts);
    assert!(
        spare.saturating_sub(load) < grid.margin && spare.saturating_sub(grid.post) >= grid.margin,
        "the margin holds the normal beacon dark and lets the low one's post revive: {grid:?}"
    );
    let mut world = world(&[20_000]);
    let (_, at) = core_and_site(&world);
    let seat = SeatId::new(0);
    let normal = world
        .place_beacon_directly(seat, offset(at, 8), MandateKind::None, PRIORITY_NORMAL)
        .unwrap_or_else(|| panic!("room for a beacon"));
    let low = world
        .place_beacon_directly(seat, offset(at, 12), MandateKind::None, PRIORITY_LOW)
        .unwrap_or_else(|| panic!("room for a beacon"));
    let mut voxels: i16 = -2;
    raise_posts(&mut world, normal, at, posts, &mut voxels);
    raise_posts(&mut world, low, at, 1, &mut voxels);

    let (mut runner, feed) = settle_once_with_dark(world, &[normal, low]);
    assert!(!dormant(runner.world(), normal), "the normal beacon is lit");
    assert!(dormant(runner.world(), low), "and the low one is dark");
    assert_eq!(
        ticks_of(&feed, EventKind::BeaconRevived, normal).len(),
        1,
        "the normal beacon's revival is reported: {feed:?}"
    );
    assert!(
        ticks_of(&feed, EventKind::BeaconRevived, low).is_empty()
            && ticks_of(&feed, EventKind::BeaconBrownedOut, low).is_empty(),
        "and the low beacon, lit and shed in one settle, reports nothing: {feed:?}"
    );
    let (supply, draw) = supply_and_draw(runner.world(), seat);
    assert!(draw.raw() <= supply.raw(), "{supply:?} {draw:?}");
    for _ in 0..20 {
        runner.step();
        assert!(
            !runner.events().iter().any(|event| matches!(
                event.kind,
                EventKind::BeaconRevived | EventKind::BeaconBrownedOut
            )),
            "and the grid holds: {:?}",
            runner.events()
        );
        runner.clear_events();
    }
}

#[test]
fn of_two_equal_dark_beacons_the_lower_id_swaps_first() {
    // S1's plan, `grid`: the swap's ties go to the lowest seat and then the
    // lowest beacon id. Two dark HIGH beacons, each of whose loads fits only
    // once one shared LOW beacon is shed, and not both: the lower id relights,
    // although the other stands nearer the core (the revival order would have
    // picked the nearer one).
    let grid = Grid::of(&rules());
    let spare = grid.surplus.saturating_sub(grid.starting_load());
    let posts = posts_within_spare();
    let load = grid.post.saturating_mul(posts);
    assert!(
        load <= spare && spare.saturating_sub(grid.post) < load && spare < load.saturating_mul(2),
        "each fits alone once the LOW beacon is shed, and the two never fit together: {grid:?}"
    );
    let mut world = world(&[20_000]);
    let (_, at) = core_and_site(&world);
    let seat = SeatId::new(0);
    let far = world
        .place_beacon_directly(seat, offset(at, 20), MandateKind::None, PRIORITY_HIGH)
        .unwrap_or_else(|| panic!("room for a beacon"));
    let near = world
        .place_beacon_directly(seat, offset(at, 4), MandateKind::None, PRIORITY_HIGH)
        .unwrap_or_else(|| panic!("room for a beacon"));
    let shared = world
        .place_beacon_directly(seat, offset(at, 8), MandateKind::None, PRIORITY_LOW)
        .unwrap_or_else(|| panic!("room for a beacon"));
    assert!(far.raw() < near.raw(), "the far beacon has the lower id");
    let mut voxels: i16 = -2;
    raise_posts(&mut world, far, at, posts, &mut voxels);
    raise_posts(&mut world, near, at, posts, &mut voxels);
    raise_posts(&mut world, shared, at, 1, &mut voxels);

    let (runner, feed) = settle_once_with_dark(world, &[far, near]);
    let world = runner.world();
    assert!(!dormant(world, far), "the lower id relights: {feed:?}");
    assert!(dormant(world, near), "the nearer, higher id stays dark");
    assert!(dormant(world, shared), "and the LOW beacon is shed for it");
    assert_eq!(ticks_of(&feed, EventKind::BeaconRevived, far).len(), 1);
    assert_eq!(
        ticks_of(&feed, EventKind::BeaconBrownedOut, shared).len(),
        1
    );
    assert!(ticks_of(&feed, EventKind::BeaconRevived, near).is_empty());
}

#[test]
fn a_dark_core_outranks_every_lit_expansion_in_the_swap() {
    // The swap's rank is the brownout order's first two terms, so a dark core
    // outranks a lit HIGH expansion, whatever the core's own knob says (the
    // core is shed last). This is the `grid` lane's reading of item 127 (7),
    // recorded in its pull request as a rule call open to the owner.
    //
    // The core carries posts that, with its surplus and a NORMAL expansion's
    // Generator, fit only once the HIGH expansion's single post is shed. The
    // Generator's beacon is ahead of the HIGH one in the brownout order and is
    // skipped, because shedding it relieves nothing.
    let rules = rules();
    let grid = Grid::of(&rules);
    let mut world = world(&[20_000]);
    let (core, at) = core_and_site(&world);
    let seat = SeatId::new(0);
    let knob = world
        .beacons()
        .priorities()
        .get(usize::try_from(core.raw()).unwrap_or(usize::MAX))
        .copied();
    assert!(
        knob.is_some_and(|knob| knob < PRIORITY_HIGH),
        "the core's own knob ranks below HIGH, so only its core term can win: {knob:?}"
    );
    let (first, second) = vent_stands(&world, at);
    let output = vent_output(&world, &rules, first, second);
    assert!(
        output >= grid.post,
        "the Generator carries the HIGH beacon's post"
    );
    let tapper = world
        .place_beacon_directly(seat, offset(at, 10), MandateKind::None, PRIORITY_NORMAL)
        .unwrap_or_else(|| panic!("room for a beacon"));
    world
        .raise_structure(seat, tapper, StructureKind::Generator, first)
        .unwrap_or_else(|| panic!("room for the Generator"));
    let high = world
        .place_beacon_directly(seat, offset(at, 14), MandateKind::None, PRIORITY_HIGH)
        .unwrap_or_else(|| panic!("room for a beacon"));
    let mut voxels: i16 = -2;
    raise_posts(&mut world, high, at, 1, &mut voxels);
    let room = grid.surplus.saturating_add(output);
    let mut load = grid.starting_load();
    let mut posts: i32 = 0;
    while load.saturating_add(grid.post) <= room {
        load = load.saturating_add(grid.post);
        posts = posts.saturating_add(1);
    }
    raise_posts(&mut world, core, at, posts, &mut voxels);

    let (runner, feed) = settle_once_with_dark(world, &[core]);
    let world = runner.world();
    assert!(!dormant(world, core), "the core relights: {feed:?}");
    assert!(dormant(world, high), "the HIGH expansion is shed for it");
    assert!(
        !dormant(world, tapper),
        "the Generator's beacon, whose shed relieves nothing, stays lit"
    );
    assert_eq!(ticks_of(&feed, EventKind::BeaconRevived, core).len(), 1);
    assert_eq!(ticks_of(&feed, EventKind::BeaconBrownedOut, high).len(), 1);
    let (supply, draw) = supply_and_draw(world, seat);
    assert!(draw.raw() <= supply.raw(), "{supply:?} {draw:?}");
}

#[test]
fn the_commander_walks_through_a_blackout_drawing_nothing() {
    // Item 127 (5), the register's S1-35: the commander is the one unit a
    // blackout does not park, so a seat can always walk to a beacon and change
    // something on site; and it draws nothing while its home is dark, like
    // everything else homed there. While its home is lit it draws
    // `power.kw_per_unit` like any other unit.
    //
    // The surplus is cut to one kilowatt short of the margin over the starting
    // force's load, so a core set dormant stays dark (the fixture of
    // `a_dormant_beacon_powers_down_everything_homed_to_it`).
    let grid = Grid::of(&rules());
    let surplus = grid
        .starting_load()
        .saturating_add(grid.margin)
        .saturating_sub(1);
    let varied = || rules_with_core_surplus(u32::try_from(surplus).unwrap_or(0));
    let seat = SeatId::new(0);

    // Lit: the commander is in the draw.
    let mut lit = Runner::new(world_with(varied(), &[20_000]));
    assert!(lit.begin_push(), "the Push begins");
    lit.step();
    let (_, draw) = supply_and_draw(lit.world(), seat);
    assert_eq!(
        draw.raw(),
        grid.starting_load(),
        "while its home is lit the commander draws like the rest of the \
         starting force, which counts it"
    );

    // Dark: the core is dormant, and the commander walks on, drawing nothing.
    let mut runner = Runner::new(world_with(varied(), &[20_000]));
    let core = core_of(runner.world(), seat);
    let at = runner
        .world()
        .beacons()
        .positions()
        .get(usize::try_from(core.raw()).unwrap_or(0))
        .copied()
        .unwrap_or_default();
    let commander = runner.world().commander_of(seat);
    assert!(runner.begin_push(), "the Push begins");
    runner
        .world_mut()
        .send_unit_directly(commander.raw(), offset(at, 12));
    runner.world_mut().set_dormant(core, true);
    let row = usize::try_from(commander.raw()).unwrap_or(usize::MAX);
    let start = runner.world().units().positions().get(row).copied();
    for tick in 0..40 {
        runner.step();
        let world = runner.world();
        assert!(dormant(world, core), "tick {tick}: the core stays dark");
        assert_eq!(
            supply_and_draw(world, seat),
            (Kw::ZERO, Kw::ZERO),
            "tick {tick}: a total blackout, with the commander out of the draw"
        );
        assert_ne!(
            world.units().positions().get(row),
            world.units().destinations().get(row),
            "tick {tick}: the commander is not parked where it stands"
        );
    }
    assert_ne!(
        runner.world().units().positions().get(row).copied(),
        start,
        "and it walked"
    );
}

#[test]
fn a_beacons_key_core_supplies_exactly_its_base() {
    // S1's plan, decision 12 (item 128), the register's S1-31: the key-core is
    // netted out of draw. The power phase reads `power.beacon_base_draw_kw`,
    // and a beacon's key-core supplies exactly that row, so a live beacon is
    // net zero by construction and neither column carries either figure.
    let rules = rules();
    let row = rules.message().power.as_ref().map_or_else(
        || panic!("a power block"),
        |block| block.beacon_base_draw_kw,
    );
    let power = PowerRules::read(rules.message()).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        u32::try_from(power.beacon_base_draw).ok(),
        Some(row),
        "the phase reads the row"
    );
    assert!(power.beacon_base_draw > 0, "a beacon has a base to net out");
    assert_eq!(
        power.key_core_output(),
        power.beacon_base_draw,
        "and its key-core supplies exactly that base"
    );

    // The three asserts above are the construction the plan's acceptance line
    // names; what follows is the behaviour. The core's own base is in neither
    // column: at the first settle the draw is the starting force's and the
    // supply the deep bore's surplus. A placed expansion is net zero too,
    // tick by tick: `a_placed_beacon_adds_no_draw`.
    let grid = Grid::of(&rules);
    let mut runner = Runner::new(world(&[20_000]));
    assert!(runner.begin_push(), "the Push begins");
    runner.step();
    assert_eq!(
        supply_and_draw(runner.world(), SeatId::new(0)),
        (Kw::new(grid.surplus), Kw::new(grid.starting_load())),
        "net figures: no base in the draw and no key-core in the supply"
    );
}

#[test]
fn the_dark_load_is_what_the_brownout_took_off_the_grid() {
    // Item 134 (2) (c): econ's shortfall line reads a seat's dark load from
    // the sim, never a restated brownout rule. Two Mortar-loaded expansions
    // shed by a deficit: the dark load is their two Mortars, no supply went
    // dark with them, and the shed kW is the same two Mortars.
    let grid = Grid::of(&rules());
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
    let mut voxels: i16 = -2;
    let mut placed: Vec<BeaconId> = Vec::new();
    for (step, priority) in [
        (4_i16, PRIORITY_NORMAL),
        (20, PRIORITY_NORMAL),
        (6, PRIORITY_LOW),
        (22, PRIORITY_HIGH),
    ] {
        let beacon = runner
            .world_mut()
            .place_beacon_directly(seat, offset(at, step), MandateKind::None, priority)
            .unwrap_or_else(|| panic!("room for a beacon"));
        runner
            .world_mut()
            .raise_structure(seat, beacon, StructureKind::Mortar, offset(at, voxels))
            .unwrap_or_else(|| panic!("room for a structure"));
        voxels = voxels.saturating_sub(1);
        placed.push(beacon);
    }
    assert!(runner.begin_push(), "the Push begins");
    runner.step();
    let world = runner.world();
    let dark = placed
        .iter()
        .filter(|beacon| dormant(world, **beacon))
        .count();
    let dark = i32::try_from(dark).unwrap_or(i32::MAX);
    assert!(dark > 0, "the fixture sheds something");
    let load = dark_load(world, seat).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        load,
        DarkLoad {
            draw: Kw::new(grid.mortar.saturating_mul(dark)),
            supply: Kw::ZERO,
        },
        "the dark load is the shed beacons' Mortars"
    );
    assert_eq!(
        load.shed(),
        Ok(Kw::new(grid.mortar.saturating_mul(dark))),
        "and the shed kW is the same, with no supply gone dark"
    );
    assert_eq!(
        dark_load(world, SeatId::new(1)),
        Ok(DarkLoad::default()),
        "a seat with nothing dark holds nothing off its grid"
    );
    assert_eq!(
        dark_load(world, SeatId::new(7)),
        Err(PowerError::NoSuchSeat(SeatId::new(7))),
        "and a seat that is not seated is a typed refusal, not a zero"
    );

    // A dark core takes its surplus off the grid with it: the supply half.
    let mut runner = Runner::new(world_with(
        rules_with_core_surplus(
            u32::try_from(
                grid.starting_load()
                    .saturating_add(grid.margin)
                    .saturating_sub(1),
            )
            .unwrap_or(0),
        ),
        &[20_000],
    ));
    let core = core_of(runner.world(), seat);
    assert!(runner.begin_push(), "the Push begins");
    runner.world_mut().set_dormant(core, true);
    runner.step();
    let world = runner.world();
    assert!(dormant(world, core), "the fixture held");
    let surplus = grid
        .starting_load()
        .saturating_add(grid.margin)
        .saturating_sub(1);
    assert_eq!(
        dark_load(world, seat),
        Ok(DarkLoad {
            draw: Kw::new(grid.starting_load()),
            supply: Kw::new(surplus),
        }),
        "a dark core holds its homed load and its surplus off the grid"
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

#[test]
fn only_one_generator_per_vent_adds_to_the_supply() {
    // Spec section 5: "one Generator per vent, output set by the vent's
    // richness, **because the vent's heat is the limit, not the tap**". The
    // rule is enforced where the heat is counted, so a seat may stand a second
    // Generator on a tapped vent and waste the `$`; what it may not do is
    // double its supply.
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
    let (first, second) = vent_stands(runner.world(), at);

    assert!(runner.begin_push(), "the Push begins");
    runner.step();
    let (bare, _) = supply_and_draw(runner.world(), seat);

    runner
        .world_mut()
        .raise_structure(seat, core, StructureKind::Generator, first)
        .unwrap_or_else(|| panic!("room for a structure"));
    runner.step();
    let (tapped, _) = supply_and_draw(runner.world(), seat);
    assert!(
        tapped.raw() > bare.raw(),
        "a Generator standing on a vent taps it: {bare:?} -> {tapped:?}"
    );

    runner
        .world_mut()
        .raise_structure(seat, core, StructureKind::Generator, second)
        .unwrap_or_else(|| panic!("room for a structure"));
    runner.step();
    let (twice, _) = supply_and_draw(runner.world(), seat);
    assert_eq!(
        twice, tapped,
        "a second Generator on the same vent adds nothing: the vent's heat is \
         the limit, not the tap"
    );
}

#[test]
fn a_generator_on_a_tapped_vent_does_not_buy_its_beacon_a_revival() {
    // The anti-flicker rule weighs a revival by what the beacon would add to
    // the grid, and a Generator on a vent an earlier live one already taps
    // adds nothing (the test above). So a beacon whose own Generator stands
    // on a tapped vent is weighed on its load alone: once shed for that load,
    // it stays dark, rather than reviving into the same deficit and being shed
    // again at the next settle, every tick.
    //
    // The arrangement: a Generator homed to the core taps the zone's vent
    // (the lower structure id, so its tap is the one that counts); an
    // expansion stands a second Generator on the same vent and fields
    // Mortars, the fewest whose draw outruns the grid.
    let rules = rules();
    let grid = Grid::of(&rules);
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
    let (first, second) = vent_stands(runner.world(), at);
    let output = vent_output(runner.world(), &rules, first, second);
    assert!(output > grid.margin, "a vent worth tapping: {output} kW");

    let tap = runner
        .world_mut()
        .raise_structure(seat, core, StructureKind::Generator, first)
        .unwrap_or_else(|| panic!("room for the core's Generator"));
    let expansion = runner
        .world_mut()
        .place_beacon_directly(seat, offset(at, 6), MandateKind::None, PRIORITY_NORMAL)
        .unwrap_or_else(|| panic!("room for a beacon"));
    let stacked = runner
        .world_mut()
        .raise_structure(seat, expansion, StructureKind::Generator, second)
        .unwrap_or_else(|| panic!("room for the stacked Generator"));
    assert!(
        tap.raw() < stacked.raw(),
        "the core's Generator is the earlier tap"
    );
    // The fewest Mortars whose draw outruns the core's surplus and the one
    // tap over the starting force's load.
    let spare = grid
        .surplus
        .saturating_add(output)
        .saturating_sub(grid.starting_load());
    assert!(grid.mortar > 0, "a Mortar draws: {grid:?}");
    let mut mortars: i32 = 0;
    let mut offset_by: i16 = 8;
    while mortars.saturating_mul(grid.mortar) <= spare {
        runner
            .world_mut()
            .raise_structure(
                seat,
                expansion,
                StructureKind::Mortar,
                offset(at, offset_by),
            )
            .unwrap_or_else(|| panic!("room for a Mortar"));
        mortars = mortars.saturating_add(1);
        offset_by = offset_by.saturating_add(1);
    }
    // The case the rule has to see: crediting the stacked Generator's output
    // would have met the margin, so a cost summed from the homed rows revived
    // the expansion straight back into its deficit.
    assert!(
        spare.saturating_sub(mortars.saturating_mul(grid.mortar).saturating_sub(output))
            >= grid.margin,
        "the fixture is the case a hand-summed cost gets wrong: {grid:?}, \
         {mortars} Mortars, {output} kW"
    );

    let mut feed: Vec<Event> = Vec::new();
    assert!(runner.begin_push(), "the Push begins");
    for tick in 0..20 {
        runner.step();
        feed.extend_from_slice(runner.events());
        runner.clear_events();
        let (supply, draw) = supply_and_draw(runner.world(), seat);
        assert!(
            supply.raw() >= draw.raw(),
            "tick {tick}: the settle ends with no deficit: {supply:?} < {draw:?}"
        );
    }
    assert_eq!(
        ticks_of(&feed, EventKind::BeaconBrownedOut, expansion).len(),
        1,
        "the overloaded expansion is shed once"
    );
    assert!(
        ticks_of(&feed, EventKind::BeaconRevived, expansion).is_empty(),
        "and its stacked Generator does not buy it a revival"
    );
    assert!(dormant(runner.world(), expansion), "it stays dark");
    assert!(
        !dormant(runner.world(), core),
        "and the core, whose tap is the one that counts, stays lit"
    );
}

#[test]
fn one_anchor_is_one_building() {
    // "Paid means yours" charges at commit (item 23), so a second Build target
    // on ground a target already claims would buy the same building twice and
    // stand two structure rows in one voxel. The interface row that adds a
    // target refuses a claimed anchor; the claim is across every beacon,
    // because the treasury and the ground are the seat's and not the
    // beacon's.
    let mut world = world(&[20_000]);
    let seat = SeatId::new(0);
    let core = core_of(&world, seat);
    let at = world
        .beacons()
        .positions()
        .get(usize::try_from(core.raw()).unwrap_or(0))
        .copied()
        .unwrap_or_default();
    let anchor = offset(at, 2);
    assert!(
        !world.anchor_is_claimed(seat, anchor),
        "bare ground is nobody's yet"
    );
    assert!(world.add_target(
        core,
        TargetKind::Build,
        StructureKind::Generator.id(),
        anchor,
        0
    ));
    assert!(
        world.anchor_is_claimed(seat, anchor),
        "a Build target claims the ground it names"
    );
    assert!(
        !world.anchor_is_claimed(seat, offset(at, 3)),
        "and only the ground it names"
    );
    let neighbour = world
        .place_beacon_directly(seat, offset(at, 6), MandateKind::Build, PRIORITY_NORMAL)
        .unwrap_or_else(|| panic!("room for a beacon"));
    assert!(
        world.anchor_is_claimed(seat, anchor),
        "the claim is the seat's, so another beacon of the same seat sees it \
         too: {neighbour:?}"
    );
    assert!(
        !world.anchor_is_claimed(SeatId::new(1), anchor),
        "and only the seat's: a queued Build target is a per-seat claim, so \
         another seat's unbuilt target stays hidden (targeting.md, Sites)"
    );
}

/// Two standing points on one heat vent: the first vent column found walking
/// out from `at`, and a neighbour of it inside the same stamped patch.
///
/// Nothing in the world indexes vents — they are voxel material and the map
/// report is generation-time — so the test finds them the way a player would,
/// by looking at the ground.
fn vent_stands(world: &World, at: [Fx; 3]) -> ([Fx; 3], [Fx; 3]) {
    let cx: i32 = at.first().map_or(0, |value| value.floor_voxels());
    let cy: i32 = at.get(1).map_or(0, |value| value.floor_voxels());
    let mut found: Option<[i32; 3]> = None;
    let mut reach: i32 = 0;
    while reach <= 48 && found.is_none() {
        let mut dy: i32 = -reach;
        while dy <= reach {
            let mut dx: i32 = -reach;
            while dx <= reach {
                let x = cx.saturating_add(dx);
                let y = cy.saturating_add(dy);
                if let Some(z) = world.voxels().top_solid_z(x, y)
                    && world
                        .voxels()
                        .get([x, y, z])
                        .and_then(Material::vent_richness)
                        .is_some()
                {
                    found = Some([x, y, z]);
                    break;
                }
                dx = dx.saturating_add(1);
            }
            if found.is_some() {
                break;
            }
            dy = dy.saturating_add(1);
        }
        reach = reach.saturating_add(1);
    }
    let here = found.unwrap_or_else(|| panic!("the zone's heat vent is on the map"));
    let stand = |column: [i32; 3]| {
        [
            Fx::from_voxels(i16::try_from(column.first().copied().unwrap_or(0)).unwrap_or(0)),
            Fx::from_voxels(i16::try_from(column.get(1).copied().unwrap_or(0)).unwrap_or(0)),
            Fx::from_voxels(
                i16::try_from(column.get(2).copied().unwrap_or(0).saturating_add(1)).unwrap_or(0),
            ),
        ]
    };
    // A neighbour column of the same patch, so the second tap is a different
    // voxel of one vent rather than the same voxel twice.
    for step in [[1_i32, 0_i32], [0, 1], [-1, 0], [0, -1]] {
        let x = here
            .first()
            .copied()
            .unwrap_or(0)
            .saturating_add(step.first().copied().unwrap_or(0));
        let y = here
            .get(1)
            .copied()
            .unwrap_or(0)
            .saturating_add(step.get(1).copied().unwrap_or(0));
        if let Some(z) = world.voxels().top_solid_z(x, y)
            && world
                .voxels()
                .get([x, y, z])
                .and_then(Material::vent_richness)
                .is_some()
        {
            return (stand(here), stand([x, y, z]));
        }
    }
    (stand(here), stand(here))
}

/// What one Generator on the vent under `first` supplies, read from the
/// table by the vent's grade, after checking that `second` stands on the same
/// grade.
fn vent_output(world: &World, rules: &RulesTable, first: [Fx; 3], second: [Fx; 3]) -> i32 {
    let below = |stand: [Fx; 3]| {
        let x = stand.first().map_or(0, |value| value.floor_voxels());
        let y = stand.get(1).map_or(0, |value| value.floor_voxels());
        let z = stand.get(2).map_or(0, |value| value.floor_voxels());
        world
            .voxels()
            .get([x, y, z.saturating_sub(1)])
            .and_then(Material::vent_richness)
            .unwrap_or_else(|| panic!("a vent under {stand:?}"))
    };
    assert_eq!(
        below(first),
        below(second),
        "two stands on one vent share its grade"
    );
    let by_grade = rules
        .message()
        .power
        .as_ref()
        .and_then(|block| block.generator_output_kw)
        .unwrap_or_else(|| panic!("a generator output row"));
    i32::try_from(match below(first) {
        Richness::Lean => by_grade.lean,
        Richness::Standard => by_grade.standard,
        Richness::Rich => by_grade.rich,
    })
    .unwrap_or(i32::MAX)
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

    // Both beacons want Generators: the same band, the same price, and three
    // orders each so the ladder has something left to give after the first
    // round. **Distinct anchors**, because one anchor is one building: a
    // second target on claimed ground would be charged for in its own right
    // (`World::anchor_is_claimed`). The writ is set after the beacons exist,
    // because setting it clears the target list.
    world.set_writ(core, MandateKind::Build);
    world.set_writ(second, MandateKind::Build);
    for (beacon, first) in [(core, 1_i16), (second, 8)] {
        for step in 0..3_i16 {
            assert!(world.add_target(
                beacon,
                TargetKind::Build,
                StructureKind::Generator.id(),
                offset(at, first.saturating_add(step)),
                0
            ));
        }
    }
    let price = world.structure_cost(StructureKind::Generator);

    // **Starve the band.** With money for everybody the question the round
    // robin answers never comes up: both beacons are paid on the first
    // decision tick and the test would pass with the cursor term deleted from
    // the sort. Topping the treasury up to one Generator a tick makes "who is
    // served next" the only thing that decides, and the answer has to
    // alternate.
    let mut runner = Runner::new(world);
    assert!(runner.begin_push(), "the Push begins");
    runner.clear_events();
    let mut served: Vec<u32> = Vec::new();
    let mut cursors: Vec<u32> = Vec::new();
    for _ in 0..60 {
        runner.world_mut().set_treasury(seat, price);
        let Some(report) = runner.step() else { break };
        for event in runner.events() {
            if event.kind == EventKind::StructureQueued
                && let Some(subject) = event.subject
            {
                let row = usize::try_from(subject.index()).unwrap_or(usize::MAX);
                if let Some(home) = runner.world().structures().homes().get(row).copied() {
                    served.push(home);
                    cursors.push(
                        runner
                            .world()
                            .seats()
                            .qm_cursors()
                            .first()
                            .copied()
                            .unwrap_or(u32::MAX),
                    );
                }
            }
        }
        runner.clear_events();
        if report.segment_ended {
            break;
        }
    }

    assert!(
        served.len() >= 4,
        "a starved band still fills an order a tick: {served:?}"
    );
    assert!(
        served.contains(&core.raw()) && served.contains(&second.raw()),
        "the round robin reached both beacons: {served:?}"
    );
    for pair in served.windows(2) {
        let (Some(first), Some(next)) = (pair.first(), pair.get(1)) else {
            continue;
        };
        assert_ne!(
            first, next,
            "no beacon is served twice running while its neighbour waits: {served:?}"
        );
    }
    // The cursor is what does it, and it is hashed state: it has to move with
    // the beacon that was served, or the alternation above would be a
    // coincidence of the id tie-break.
    assert_eq!(
        cursors, served,
        "the Quartermaster cursor follows the beacon it just paid"
    );
}

#[test]
fn a_fabricator_order_the_headroom_cannot_run_is_held_until_a_generator_brings_supply_back() {
    // Spec section 7: the Quartermaster "balances power: when draw exceeds
    // supply, it holds fabricator orders, then applies the brownout order"
    // (decisions-log item 123 (2) 7). The draw is built **through** the
    // Quartermaster, not around it: a Survey beacon's fabricator asks for a
    // scout, a real unit order the seat can pay for in `$`, whose
    // `power.kw_per_unit` the settled headroom cannot run. The supply comes
    // back by the sim's own paths too: the core's Build mandate buys a
    // Generator on the zone's vent, a build drone raises it, and once it
    // stands the held order is filled. [`HeldOrder`] is the arrangement and
    // [`HeldOrder::watch`] asserts the hold on every tick it lasts.
    let fixture = HeldOrder::new();
    let (grid, output) = (fixture.grid, fixture.output);
    let watched = fixture.watch();

    assert!(
        watched.generator_paid,
        "the core's Build mandate bought the Generator"
    );
    assert!(
        watched.held > 1,
        "the order was held on more than one decision tick before supply came back: {}",
        watched.held
    );
    let (supply, draw, left) = watched
        .filled
        .unwrap_or_else(|| panic!("the held order was filled once supply came back"));
    assert_eq!(
        supply.raw(),
        watched.first_supply.raw().saturating_add(output),
        "the supply that came back is the Generator's tap"
    );
    assert!(
        supply.raw().saturating_sub(draw.raw()) >= grid.per_unit,
        "and the headroom the Quartermaster read runs the scout: {supply:?} - {draw:?}"
    );
    assert_eq!(
        left,
        Money::ZERO,
        "the scout was charged when it was filled"
    );
}

/// The arrangement of the Quartermaster hold test: a lit grid whose headroom
/// is one kilowatt short of a unit's draw, a Survey beacon that wants one
/// scout, a core that builds a Generator on the zone's vent, and money for
/// exactly those two orders, so "not charged" is an equality.
struct HeldOrder {
    runner: Runner,
    grid: Grid,
    seat: SeatId,
    survey: BeaconId,
    output: i32,
    scout_cost: Money,
    generator_cost: Money,
}

/// What [`HeldOrder::watch`] saw.
struct Watched {
    /// Decision ticks on which the order was asked, payable and not filled.
    held: u32,
    /// Whether the Generator's order was paid.
    generator_paid: bool,
    /// Supply, draw and treasury at the end of the tick the scout was filled.
    filled: Option<(Kw, Kw, Money)>,
    /// The supply at the end of the Push's first tick.
    first_supply: Kw,
}

impl HeldOrder {
    fn new() -> HeldOrder {
        // The core's surplus is cut to one kilowatt short of a unit's draw
        // over the starting force's load, so the grid is lit (draw below
        // supply, no brownout) and the headroom is `power.kw_per_unit` less
        // one.
        let grid = Grid::of(&rules());
        assert!(grid.per_unit > 0, "a unit draws: {grid:?}");
        let surplus = grid
            .starting_load()
            .saturating_add(grid.per_unit)
            .saturating_sub(1);
        let rules = rules_with_core_surplus(u32::try_from(surplus).unwrap_or(0));
        // A long segment: the build drone has to walk to the vent and raise
        // the Generator inside it (at the committed table, about 51 of these
        // 120 s).
        let mut world = world_with(rules.clone(), &[120_000]);
        let seat = SeatId::new(0);
        let core = core_of(&world, seat);
        let at = world
            .beacons()
            .positions()
            .get(usize::try_from(core.raw()).unwrap_or(0))
            .copied()
            .unwrap_or_default();
        let (vent, _) = vent_stands(&world, at);
        let output = vent_output(&world, &rules, vent, vent);
        assert!(output > 0, "a vent worth tapping: {output} kW");

        // The writs are set before their settings, because setting a writ
        // clears them.
        world.set_writ(core, MandateKind::Build);
        assert!(
            world.add_target(
                core,
                TargetKind::Build,
                StructureKind::Generator.id(),
                vent,
                0
            ),
            "the Generator's target fits"
        );
        let survey = world
            .place_beacon_directly(seat, offset(at, 6), MandateKind::Survey, PRIORITY_NORMAL)
            .unwrap_or_else(|| panic!("room for a beacon"));
        world.set_writ(survey, MandateKind::Survey);
        world.set_scouts(survey, 1);
        let scout_cost = world.unit_cost(UnitKind::Scout);
        let generator_cost = world.structure_cost(StructureKind::Generator);
        assert!(
            scout_cost.raw() > 0 && generator_cost.raw() > 0,
            "both orders cost `$`: {scout_cost:?}, {generator_cost:?}"
        );
        world.set_treasury(
            seat,
            Money::new(scout_cost.raw().saturating_add(generator_cost.raw())),
        );
        HeldOrder {
            runner: Runner::new(world),
            grid,
            seat,
            survey,
            output,
            scout_cost,
            generator_cost,
        }
    }

    /// Plays the Push until the scout is filled or the segment ends,
    /// asserting on every tick before the fill that the order is held.
    fn watch(mut self) -> Watched {
        let asking = pharmakos_sim::mandate::mandate_for(MandateKind::Survey)
            .unwrap_or_else(|| panic!("the Survey mandate runs at the skeleton"));
        let seat = self.seat;
        let is_seat = |event: &Event, kind: EventKind, value: u8| {
            event.kind == kind && event.seat == Some(seat) && event.value == i64::from(value)
        };
        assert!(self.runner.begin_push(), "the Push begins");
        self.runner.clear_events();
        let mut watched = Watched {
            held: 0,
            generator_paid: false,
            filled: None,
            first_supply: Kw::ZERO,
        };
        let mut first = true;
        while let Some(report) = self.runner.step() {
            let events = self.runner.events().to_vec();
            self.runner.clear_events();
            let world = self.runner.world();
            let (supply, draw) = supply_and_draw(world, seat);
            if first {
                watched.first_supply = supply;
                first = false;
            }
            watched.generator_paid |= events.iter().any(|event| {
                is_seat(
                    event,
                    EventKind::StructureQueued,
                    StructureKind::Generator.id(),
                )
            });
            let scouts = events
                .iter()
                .filter(|event| is_seat(event, EventKind::UnitFabricated, UnitKind::Scout.id()))
                .count();
            if scouts > 0 {
                assert_eq!(scouts, 1, "one order, one scout");
                assert!(
                    world.is_decision_tick(),
                    "an order is filled on a decision tick"
                );
                // The scout fielded is the one the Survey fabricator asked
                // for: once it is filled, that fabricator asks for nothing.
                assert!(
                    asking.request(world, self.survey).is_none(),
                    "the filled scout is the Survey fabricator's order"
                );
                watched.filled = Some((supply, draw, treasury(world, seat)));
                break;
            }
            // The fabricator still wants the scout: the Survey mandate asks
            // again on every decision tick, and nothing queues an order
            // between them.
            let request = asking
                .request(world, self.survey)
                .unwrap_or_else(|| panic!("the fabricator still wants its scout"));
            assert_eq!(
                (request.cost, request.draw),
                (self.scout_cost, Kw::new(self.grid.per_unit)),
                "the order is a scout at the table's price and draw"
            );
            // Orders are gathered on a decision tick only
            // (`World::is_decision_tick`, the tick just stepped). On one, an
            // order the seat can pay for that was not filled was held, and
            // only for the headroom the power phase had just settled, which is
            // what the Quartermaster read.
            if world.is_decision_tick() {
                assert!(
                    supply.raw().saturating_sub(draw.raw()) < self.grid.per_unit,
                    "held only while the headroom is short: {supply:?} - {draw:?}"
                );
                assert!(
                    supply.raw() >= draw.raw(),
                    "the grid is lit, so the hold is the Quartermaster's, not a brownout's"
                );
                watched.held = watched.held.saturating_add(1);
            }
            let expected = if watched.generator_paid {
                self.scout_cost
            } else {
                Money::new(
                    self.scout_cost
                        .raw()
                        .saturating_add(self.generator_cost.raw()),
                )
            };
            assert_eq!(
                treasury(world, seat),
                expected,
                "a held order charges nothing"
            );
            if report.segment_ended {
                break;
            }
        }
        watched
    }
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

#[test]
fn a_tie_on_held_value_places_the_lower_seat_id_ahead() {
    // Spec section 7: "rank on held value ... among living seats, **tie-break
    // the lower seat index**" — ahead, so a tied lower id draws the leader's
    // malus and a tied higher id draws last place's catch-up bonus. AGENTS.md
    // §4.6 states the same convention for the sim at large ("ties to the
    // lowest seat id"). The direction has no test of its own in the ledger
    // golden, where the tie is a coincidence of the numbers rather than a
    // stated case, so it gets one here: written the other way round the sim
    // would pay the catch-up dial to whoever happened to be seat 0.
    let mut runner = Runner::new(world(&[1_000]));
    let mut feed: Vec<Event> = Vec::new();
    play_segment(&mut runner, &mut feed);

    // One second earns and loses nothing on a three-way symmetric map, so the
    // seats are still exactly level when the Ledger settles — which is the
    // arrangement the tie rule is about.
    let mut paid: Vec<(u8, i64)> = Vec::new();
    for event in &feed {
        if event.kind == EventKind::Settled
            && let Some(seat) = event.seat
        {
            paid.push((seat.raw(), event.value));
        }
    }
    assert_eq!(
        paid.len(),
        usize::try_from(SEATS).unwrap_or(0),
        "every living seat is settled once: {paid:?}"
    );
    let level: Vec<i64> = paid
        .iter()
        .map(|(seat, value)| {
            runner
                .world()
                .held_value(SeatId::new(*seat))
                .raw()
                .saturating_sub(*value)
        })
        .collect();
    assert!(
        level.windows(2).all(|pair| pair.first() == pair.get(1)),
        "the fixture has to be a tie for the tie rule to be under test: {level:?}"
    );

    let rules = rules();
    let leader = pharmakos_sim::economy::bmi_for(&rules, 0, SEATS);
    let last = pharmakos_sim::economy::bmi_for(&rules, SEATS.saturating_sub(1), SEATS);
    assert!(
        leader.raw() < last.raw(),
        "the ladder has to run downhill for the assertion below to mean \
         anything: leader {leader:?} last {last:?}"
    );
    for (seat, value) in &paid {
        let rank = u32::from(*seat);
        assert_eq!(
            *value,
            pharmakos_sim::economy::bmi_for(&rules, rank, SEATS).raw(),
            "seat {seat} is ranked at its own index on a level ladder, so the \
             lowest id takes the leader's malus and the highest last place's \
             bonus: {paid:?}"
        );
    }
}

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
    // drone's round trip is `economy.mining_carry_voxels` digs at
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
