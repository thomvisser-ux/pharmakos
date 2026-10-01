// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a deploy costs, and when (register X-12; S1's plan, the `fixs` lane).
//!
//! Spec section 7: "`$` leaves the treasury the moment an order commits:
//! deploy starts ... An order the treasury cannot cover fails like any other
//! step". Spec section 5, Placement: "death, leaving, or an illegal site aborts
//! the deploy and refunds it in full". Decision 7 of S1's plan (ruled by item
//! 128): the failure is the new additive id 12, `unaffordable`.
//!
//! No event carries a charge, so each test here reads the treasury itself and
//! accounts for every other flow the tick reported (ore, salvage, the Ledger,
//! a recycle, the Quartermaster's spends): what is left over is the deploy's.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules — not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::path::PathBuf;

use pharmakos_proto::json;
use pharmakos_sim::events::{Event, EventKind};
use pharmakos_sim::interpreter::Plan;
use pharmakos_sim::interpreter::state::StepFailure;
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::math::quantity::{Hp, Money};
use pharmakos_sim::runner::{MatchSettings, Runner};
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::tables::{BeaconId, PRIORITY_NORMAL, SeatId, StructureKind, UnitKind};
use pharmakos_sim::world::{DamageOrder, DamageTarget};
use pharmakos_sim::{RulesTable, World, WorldConfig};

/// The seat every test drives.
const SEAT: u8 = 0;

/// A deploy beside seat 0's core on the determinism seed, then a hold. The
/// site is the committed `place_beacon` transcript case's, so it is legal.
const DEPLOY: &str = r#"{
  "schema_version": {"major": 1, "minor": 0},
  "meta": {"title": "Deploy, then hold", "author_kind": "HUMAN"},
  "kind": "PLAYBOOK",
  "declarative": {
    "route": [
      {"label": "deploy", "place_beacon": {"at": {"voxel": {"x": 27, "y": 356, "z": 29}},
        "initial": {"mandate": {"roe": "RETURN_FIRE", "build": {}}}},
        "timeout_ms": 30000, "on_fail": {"action": "SKIP"}},
      {"label": "wait", "hold": {"ms": 5000}, "timeout_ms": 10000, "on_fail": {"action": "SKIP"}}
    ],
    "handlers": []
  },
  "on_death": {"on_respawn": "CONTINUE", "max_deaths_before_fallback": 2},
  "fallback": {"hold": {"at": {"safest": {}}}}
}"#;

fn repo_root() -> PathBuf {
    let here = PathBuf::from(".");
    if here.join("rules").join("rules.v1.json").is_file() {
        return here;
    }
    PathBuf::from("..").join("..")
}

fn rules() -> RulesTable {
    RulesTable::load(&repo_root().join("rules").join("rules.v1.json"))
        .expect("the committed rules table loads")
}

/// The interpreter tests' world: the determinism seed, three seats, no
/// harness walkers, one segment of `segment_ms`.
fn world(segment_ms: i32) -> World {
    World::new(&WorldConfig {
        match_seed: pharmakos_sim::DETERMINISM_MATCH_SEED,
        seats: 3,
        units_per_seat: 0,
        rules: rules(),
        match_settings: MatchSettings {
            segment_lengths_ms: vec![segment_ms],
            round_limit: 3,
        },
    })
    .expect("the rules table describes a map")
}

fn open(segment_ms: i32, treasury: Option<Money>) -> Runner {
    open_with(segment_ms, |world| {
        if let Some(amount) = treasury {
            world.set_treasury(SeatId::new(SEAT), amount);
        }
    })
}

/// [`open`], with the world prepared by `prepare` before the seal.
fn open_with(segment_ms: i32, prepare: impl FnOnce(&mut World)) -> Runner {
    let mut world = world(segment_ms);
    prepare(&mut world);
    let plan = Plan::compile(
        &json::decode(DEPLOY).expect("the playbook is canonical gp.v1 JSON"),
        &rules(),
    )
    .expect("the playbook compiles");
    let mut runner = Runner::new(world);
    runner
        .seal_playbook(SeatId::new(SEAT), plan)
        .expect("a Lull takes a seal");
    assert!(runner.begin_push(), "a match opens in a Lull");
    runner.clear_events();
    runner
}

fn treasury(world: &World) -> i64 {
    let row = world.seat_row(SeatId::new(SEAT)).expect("seat 0 is seated");
    world
        .seats()
        .treasuries()
        .get(row)
        .copied()
        .unwrap_or(Money::ZERO)
        .raw()
}

/// Every treasury flow one tick's events account for, for seat 0.
fn explained(world: &World, events: &[Event]) -> i64 {
    let mut flow: i64 = 0;
    for event in events {
        if event.seat != Some(SeatId::new(SEAT)) {
            continue;
        }
        flow = flow.saturating_add(match event.kind {
            EventKind::OreDelivered
            | EventKind::SalvageDelivered
            | EventKind::Settled
            | EventKind::BeaconRecycled => event.value,
            EventKind::UnitFabricated => u8::try_from(event.value)
                .ok()
                .and_then(UnitKind::from_id)
                .map_or(0, |kind| world.unit_cost(kind).raw())
                .saturating_neg(),
            EventKind::StructureQueued => u8::try_from(event.value)
                .ok()
                .and_then(StructureKind::from_id)
                .map_or(0, |kind| world.structure_cost(kind).raw())
                .saturating_neg(),
            _ => 0,
        });
    }
    flow
}

/// One tick, as `(tick, what the treasury did that no event explains, events)`.
fn step(runner: &mut Runner) -> Option<(u32, i64, Vec<Event>)> {
    let before = treasury(runner.world());
    let report = runner.step()?;
    let events = runner.events().to_vec();
    runner.clear_events();
    let unexplained = treasury(runner.world())
        .saturating_sub(before)
        .saturating_sub(explained(runner.world(), &events));
    Some((report.tick.raw(), unexplained, events))
}

/// Seat 0's core: its first beacon row.
fn core_of(world: &World) -> BeaconId {
    let beacons = world.beacons();
    (0..beacons.ids().len())
        .find(|row| beacons.seats().get(*row).copied() == Some(SEAT))
        .and_then(|row| beacons.ids().get(row).copied())
        .map(BeaconId::new)
        .expect("seat 0 has a core")
}

fn has(events: &[Event], kind: EventKind) -> bool {
    events
        .iter()
        .any(|event| event.kind == kind && event.seat == Some(SeatId::new(SEAT)))
}

#[test]
fn a_deploy_charges_the_beacon_when_it_starts() {
    let mut runner = open(60_000, None);
    let cost = runner.world().beacon_cost().raw();
    assert!(cost > 0, "structures.beacon.cost_dollars is a price");
    let mut started = None;
    let mut placed = None;
    let mut moves: Vec<(u32, i64)> = Vec::new();
    while let Some((tick, unexplained, events)) = step(&mut runner) {
        if started.is_none() && has(&events, EventKind::StepStarted) {
            started = Some(tick);
        }
        if has(&events, EventKind::BeaconPlaced) {
            placed = Some(tick);
        }
        if unexplained != 0 {
            moves.push((tick, unexplained));
        }
        if placed.is_some_and(|at| tick > at.saturating_add(100)) {
            break;
        }
    }
    let started = started.expect("the deploy starts");
    let placed = placed.expect("the beacon is placed");
    assert!(placed > started, "a deploy takes its time");
    assert_eq!(
        moves,
        vec![(started, cost.saturating_neg())],
        "the beacon's $ {cost} leaves the treasury on the tick the deploy starts, once, and \
         nothing is charged when the beacon lands on tick {placed}"
    );
}

#[test]
fn an_aborted_deploy_refunds_the_beacon_in_full() {
    // Death: the commander is killed halfway through the 12 s deploy.
    let mut runner = open(60_000, None);
    let cost = runner.world().beacon_cost().raw();
    let opening = treasury(runner.world());
    let mut moves: Vec<(u32, i64)> = Vec::new();
    let mut failures: Vec<i64> = Vec::new();
    let mut killed = false;
    while let Some((tick, unexplained, events)) = step(&mut runner) {
        if unexplained != 0 {
            moves.push((tick, unexplained));
        }
        failures.extend(
            events
                .iter()
                .filter(|event| event.kind == EventKind::StepFailed)
                .map(|event| event.value),
        );
        assert!(
            !has(&events, EventKind::BeaconPlaced),
            "a dead commander deploys nothing"
        );
        if tick == 100 && !killed {
            let commander = runner.world().commander_of(SeatId::new(SEAT));
            assert!(runner.world_mut().request_damage(DamageOrder {
                target: DamageTarget::Unit(commander),
                amount: Hp::new(100_000),
                by: SeatId::NEUTRAL,
            }));
            killed = true;
        }
        if tick >= 200 {
            break;
        }
    }
    assert_eq!(
        failures.first().copied(),
        Some(i64::from(StepFailure::CommanderDead.id())),
        "the death aborted the deploy"
    );
    assert_eq!(moves.len(), 2, "a charge and its refund: {moves:?}");
    assert_eq!(moves.first().map(|(_, amount)| *amount), Some(-cost));
    assert_eq!(
        moves.get(1).map(|(_, amount)| *amount),
        Some(cost),
        "the refund is the whole price, not a share of it"
    );
    assert!(opening >= cost, "the fixture could afford the deploy");

    // The segment's end: a 5 s Push cannot hold a 12 s deploy, and the
    // playbook stops at segment end (spec section 10), which aborts it.
    let mut runner = open(5_000, None);
    let mut moves: Vec<i64> = Vec::new();
    let mut ended = false;
    while let Some((_, unexplained, events)) = step(&mut runner) {
        if unexplained != 0 {
            moves.push(unexplained);
        }
        assert!(!has(&events, EventKind::BeaconPlaced));
        ended = events
            .iter()
            .any(|event| event.kind == EventKind::SegmentEnded);
    }
    assert!(ended, "the Push ran out");
    assert_eq!(
        moves,
        vec![-cost, cost],
        "a deploy the segment cut short is refunded in the tick that closed it"
    );
    let row = runner.world().interpreter().state(usize::from(SEAT));
    assert!(
        row.is_some_and(|state| state.stage == pharmakos_sim::interpreter::VisitState::NotStarted),
        "and the frozen snapshot holds no half-paid deploy"
    );
}

#[test]
fn a_site_made_illegal_before_placement_aborts_and_refunds_the_deploy() {
    // An illegal site: the deploy's site lies in seat 0's core's sphere and in
    // no other beacon's of seat 0's, so destroying the core mid-deploy leaves
    // the site outside every own sphere when the 12 s are up. A second beacon
    // of seat 0's, thirty voxels past the site, keeps the seat in the match.
    let site = [
        Fx::from_voxels(27),
        Fx::from_voxels(356),
        Fx::from_voxels(29),
    ];
    let far = [Fx::from_voxels(57), site[1], site[2]];
    let mut core = None;
    let mut runner = open_with(60_000, |world| {
        core = Some(core_of(world));
        world
            .place_beacon_directly(SeatId::new(SEAT), far, MandateKind::Build, PRIORITY_NORMAL)
            .expect("the beacon table has room");
    });
    let core = core.expect("seat 0 has a core");
    let cost = runner.world().beacon_cost().raw();
    let mut moves: Vec<i64> = Vec::new();
    let mut failures: Vec<i64> = Vec::new();
    let mut destroyed = false;
    while let Some((tick, unexplained, events)) = step(&mut runner) {
        if unexplained != 0 {
            moves.push(unexplained);
        }
        failures.extend(
            events
                .iter()
                .filter(|event| event.kind == EventKind::StepFailed)
                .map(|event| event.value),
        );
        assert!(
            !has(&events, EventKind::BeaconPlaced),
            "an illegal site gets no beacon"
        );
        if tick == 100 && !destroyed {
            assert!(runner.world_mut().request_damage(DamageOrder {
                target: DamageTarget::Beacon(core),
                amount: Hp::new(1_000_000),
                by: SeatId::NEUTRAL,
            }));
            destroyed = true;
        }
        if tick >= 400 {
            break;
        }
    }
    assert_eq!(
        failures.first().copied(),
        Some(i64::from(StepFailure::IllegalSite.id())),
        "the site was illegal when the deploy came to place its beacon"
    );
    assert_eq!(
        moves,
        vec![-cost, cost],
        "charged when the deploy started, refunded in full when it was aborted"
    );
}

#[test]
fn a_deploy_the_treasury_cannot_cover_fails_its_step() {
    let cost = world(60_000).beacon_cost();
    let short = Money::new(cost.raw().saturating_sub(1));
    let mut runner = open(60_000, Some(short));
    let mut started: Vec<i64> = Vec::new();
    let mut failed: Vec<i64> = Vec::new();
    let mut moves: Vec<i64> = Vec::new();
    while let Some((tick, unexplained, events)) = step(&mut runner) {
        if unexplained != 0 {
            moves.push(unexplained);
        }
        for event in &events {
            match event.kind {
                EventKind::StepStarted => started.push(event.value),
                EventKind::StepFailed => failed.push(event.value),
                _ => {}
            }
        }
        assert!(!has(&events, EventKind::BeaconPlaced), "nothing was bought");
        if tick >= 60 {
            break;
        }
    }
    assert_eq!(
        failed,
        vec![i64::from(StepFailure::Unaffordable.id())],
        "the step fails with `unaffordable`, id 12, and nothing else fails"
    );
    assert_eq!(StepFailure::Unaffordable.id(), 12);
    assert_eq!(StepFailure::Unaffordable.name(), "unaffordable");
    assert_eq!(
        started,
        vec![0, 1],
        "its `on_fail: SKIP` takes it: the route moves on to the hold"
    );
    assert!(moves.is_empty(), "and nothing was charged: {moves:?}");
    assert!(
        treasury(runner.world()) >= 0,
        "a deploy never takes a treasury below zero"
    );
}
