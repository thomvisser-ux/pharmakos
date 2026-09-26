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
use pharmakos_sim::voxels::Material;
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
    // **except the commander**, which a brownout never parks. See
    // `programs::program_for`: the only way a seat acts on a shortfall is to
    // walk the commander to a beacon and interface on site, so a commander
    // that went dark with the grid would make a brownout a lockout. (What a
    // priority raise then does is narrower than fixing it: the beacon sheds
    // later and revives sooner, and nothing relights; whether a raise should
    // re-apply the brownout order is a PLACEHOLDER there, owner at S1.)
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
fn a_beacon_whose_shed_relieves_nothing_is_still_shed_ahead_of_the_core() {
    // Item 113 (5), behaviour kept and pinned. A beacon is net zero through
    // its key-core, so shedding one with nothing homed to it relieves 0 kW;
    // the brownout order is walked without asking what a shed relieves (spec
    // section 5's order states no exception), so it is shed anyway, ahead of
    // the core. PLACEHOLDER at `power::brown_out`: whether the order should
    // skip it (owner, at S1, with the grid). If S1 decides it should, this is
    // the test that changes.
    let grid = Grid::of(&rules());
    let seat = SeatId::new(0);

    // (a) The deficit is the core's own: its homed load outruns its surplus.
    // The idle beacon is shed first, relieving nothing, then the core; neither
    // can revive, the idle one because a blackout has no margin to give.
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
    assert!(runner.begin_push(), "the Push begins");
    runner.step();
    let feed: Vec<Event> = runner.events().to_vec();
    let order: Vec<u32> = feed
        .iter()
        .filter(|event| event.kind == EventKind::BeaconBrownedOut && event.seat == Some(seat))
        .filter_map(|event| event.subject.map(|id| id.index()))
        .collect();
    assert_eq!(
        order,
        [idle.raw(), core.raw()],
        "the idle beacon is shed first although its shed relieves nothing"
    );
    assert!(dormant(runner.world(), idle) && dormant(runner.world(), core));

    // (b) The deficit is a loaded expansion's: the idle beacon, on low
    // priority, is shed first and relieves nothing; the loaded one is shed
    // next and settles the deficit. The margin that shed leaves then revives
    // the idle beacon **on the same tick** — its revival costs nothing — so
    // it emits a shed and a revival together and ends the tick lit.
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
    assert!(
        grid.surplus.saturating_sub(grid.starting_load()) >= grid.margin,
        "the committed table leaves the starting force the margin: {grid:?}"
    );
    assert!(runner.begin_push(), "the Push begins");
    runner.step();
    let feed: Vec<Event> = runner.events().to_vec();
    let shed = ticks_of(&feed, EventKind::BeaconBrownedOut, idle);
    assert_eq!(shed.len(), 1, "the idle beacon was shed: {feed:?}");
    assert_eq!(
        ticks_of(&feed, EventKind::BeaconBrownedOut, loaded),
        shed,
        "and the loaded one after it, on the same settle"
    );
    assert_eq!(
        ticks_of(&feed, EventKind::BeaconRevived, idle),
        shed,
        "and revived on the same tick, once the loaded beacon's shed left the margin"
    );
    assert!(!dormant(runner.world(), idle), "so it ends the tick lit");
    assert!(dormant(runner.world(), loaded), "the loaded one stays dark");
    assert!(
        !dormant(runner.world(), core),
        "and the core never went dark"
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
        !world.anchor_is_claimed(anchor),
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
        world.anchor_is_claimed(anchor),
        "a Build target claims the ground it names"
    );
    assert!(
        !world.anchor_is_claimed(offset(at, 3)),
        "and only the ground it names"
    );
    let neighbour = world
        .place_beacon_directly(seat, offset(at, 6), MandateKind::Build, PRIORITY_NORMAL)
        .unwrap_or_else(|| panic!("room for a beacon"));
    assert!(
        world.anchor_is_claimed(anchor),
        "the claim is the seat's, so another beacon of the same seat sees it \
         too: {neighbour:?}"
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
