// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The final audit (register X-08; spec section 3; decisions-log items 4, 16
//! and 18; S1's plan, the `fixs` lane).
//!
//! A round-limit end names its winner by the final audit, with the tie-break
//! order "unsmoothed net worth at the final audit, then enemy value destroyed,
//! then fewer beacons lost, then a shared win". No state is added: the
//! outcome's winner byte is the one the match state always hashed, and the
//! terms are read from already-hashed state by `pharmakos_sim::audit`.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules — not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::path::PathBuf;

use pharmakos_sim::audit::{beacons_lost, final_audit};
use pharmakos_sim::events::{Event, EventKind};
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::math::quantity::{Hp, Money};
use pharmakos_sim::runner::{MatchEndReason, MatchSettings, Runner};
use pharmakos_sim::seams::MandateKind;
use pharmakos_sim::snapshot::Snapshot;
use pharmakos_sim::tables::{BeaconId, PRIORITY_NORMAL, SeatId};
use pharmakos_sim::world::{DamageOrder, DamageTarget};
use pharmakos_sim::{RulesTable, World, WorldConfig};

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

/// The committed table with the settlement's standing adjustment switched
/// off, so the Ledger pays every seat the same BMI at the final settlement and
/// a tie the fixture sets up before it is still a tie after it.
fn flat_rules() -> RulesTable {
    let mut message = rules().message().clone();
    let economy = message.economy.as_mut().expect("an economy block");
    economy.scaling_last_place_bonus_percent = 0;
    economy.scaling_leader_malus_percent = 0;
    RulesTable::from_message(&message).expect("the varied table is a table")
}

/// The one BMI every seat is paid at the final settlement under [`flat_rules`].
fn bmi() -> i64 {
    i64::from(
        flat_rules()
            .message()
            .economy
            .as_ref()
            .expect("an economy block")
            .bmi_dollars,
    )
}

/// Three seats, one short round of 40 ticks: the round limit ends the match at
/// its final tick. No playbooks are sealed, so nothing moves but the economy.
fn world() -> World {
    World::new(&WorldConfig {
        match_seed: pharmakos_sim::DETERMINISM_MATCH_SEED,
        seats: 3,
        units_per_seat: 0,
        rules: flat_rules(),
        match_settings: MatchSettings {
            segment_lengths_ms: vec![2_000],
            round_limit: 1,
        },
    })
    .expect("the rules table describes a map")
}

fn seat(raw: u8) -> SeatId {
    SeatId::new(raw)
}

fn core_at(world: &World, of: SeatId) -> [Fx; 3] {
    let beacons = world.beacons();
    (0..beacons.ids().len())
        .find(|row| beacons.seats().get(*row).copied() == Some(of.raw()))
        .and_then(|row| beacons.positions().get(row).copied())
        .expect("every seat has a core")
}

/// A placed beacon of `of`'s, four voxels east of its core.
fn extra_beacon(world: &mut World, of: SeatId) -> BeaconId {
    let at = core_at(world, of);
    let east = [
        at[0].checked_add(Fx::from_voxels(4)).expect("on the map"),
        at[1],
        at[2],
    ];
    world
        .place_beacon_directly(of, east, MandateKind::Build, PRIORITY_NORMAL)
        .expect("the beacon table has room")
}

fn destroy(runner: &mut Runner, beacon: BeaconId) {
    assert!(runner.world_mut().request_damage(DamageOrder {
        target: DamageTarget::Beacon(beacon),
        amount: Hp::new(1_000_000),
        by: SeatId::NEUTRAL,
    }));
}

/// The ticks the one Push plays: 2 s at 20 Hz.
const TICKS: u32 = 40;

/// Play every tick of the one Push but its last.
fn play_to_the_last_tick(runner: &mut Runner) {
    for _ in 1..TICKS {
        let report = runner.step().expect("a Push tick");
        assert!(!report.segment_ended);
        runner.clear_events();
    }
    assert!(
        runner.outcome().is_none(),
        "nothing is decided before the final tick"
    );
}

/// Play the final tick, which decides the match, then close the recap;
/// returns what the final tick emitted.
fn finish(runner: &mut Runner) -> Vec<Event> {
    let report = runner.step().expect("the final tick");
    assert!(report.segment_ended && report.match_ended, "{report:?}");
    let events = runner.events().to_vec();
    runner.clear_events();
    assert!(runner.end_recap(), "the match is in its recap");
    assert!(
        !runner
            .events()
            .iter()
            .any(|event| event.kind == EventKind::MatchEnded),
        "and closing the recap does not announce the end a second time"
    );
    runner.clear_events();
    events
}

fn treasury(world: &World, of: SeatId) -> i64 {
    let row = world.seat_row(of).expect("seated");
    world
        .seats()
        .treasuries()
        .get(row)
        .copied()
        .unwrap_or(Money::ZERO)
        .raw()
}

/// Set each seat's treasury so that its held value is exactly `targets[i]`
/// once the final settlement has paid it.
fn hold(world: &mut World, targets: [i64; 3]) {
    let targets = targets.map(|target| target.saturating_sub(bmi()));
    for (raw, target) in (0_u8..).zip(targets) {
        let assets = world
            .held_value(seat(raw))
            .raw()
            .saturating_sub(treasury(world, seat(raw)));
        world.set_treasury(seat(raw), Money::new(target.saturating_sub(assets)));
        assert_eq!(world.held_value(seat(raw)).raw(), target);
    }
}

#[test]
fn a_round_limit_end_names_the_final_audits_winner() {
    let mut runner = Runner::new(world());
    assert!(runner.begin_push());
    play_to_the_last_tick(&mut runner);
    hold(runner.world_mut(), [1_000, 1_500, 900]);
    let events = finish(&mut runner);

    let outcome = runner.outcome().expect("the round limit ended the match");
    assert_eq!(outcome.reason, MatchEndReason::RoundLimit);
    assert_eq!(outcome.winner, Some(seat(1)), "the highest net worth wins");
    let audit = final_audit(runner.world());
    assert_eq!(audit.winner(), Some(seat(1)));
    assert_eq!(
        audit
            .lines
            .iter()
            .map(|line| line.score.raw())
            .collect::<Vec<_>>(),
        vec![1_000, 1_500, 900]
    );

    let lines: Vec<(Option<u8>, i64)> = events
        .iter()
        .filter(|event| event.kind == EventKind::FinalAudit)
        .map(|event| (event.seat.map(SeatId::raw), event.value))
        .collect();
    assert_eq!(
        lines,
        vec![(Some(0), 1_000), (Some(1), 1_500), (Some(2), 900)],
        "one `final_audit` line per seat, in seat order, carrying its score"
    );
    let ended = events
        .iter()
        .find(|event| event.kind == EventKind::MatchEnded)
        .expect("the match announced its end");
    assert_eq!(
        ended.seat,
        Some(seat(1)),
        "and `match_ended` names the winner"
    );
    assert_eq!(ended.value, i64::from(MatchEndReason::RoundLimit.id()));
    let last_line = events
        .iter()
        .rposition(|event| event.kind == EventKind::FinalAudit);
    let end_at = events
        .iter()
        .position(|event| event.kind == EventKind::MatchEnded);
    assert!(
        last_line < end_at,
        "the audit comes before the end it decides"
    );
}

#[test]
fn a_net_worth_tie_goes_to_fewer_beacons_lost() {
    let mut base = world();
    let lost = extra_beacon(&mut base, seat(0));
    let mut runner = Runner::new(base);
    assert!(runner.begin_push());
    destroy(&mut runner, lost);
    play_to_the_last_tick(&mut runner);
    assert_eq!(beacons_lost(runner.world(), seat(0)), 1);
    assert_eq!(beacons_lost(runner.world(), seat(1)), 0);
    hold(runner.world_mut(), [2_000, 2_000, 1_000]);
    let _ = finish(&mut runner);

    let outcome = runner.outcome().expect("ended");
    assert_eq!(
        outcome.winner,
        Some(seat(1)),
        "seats 0 and 1 tie on net worth and on value destroyed, and seat 1 lost fewer beacons"
    );
    let audit = final_audit(runner.world());
    assert_eq!(audit.winners, vec![seat(1)]);
    assert!(!audit.is_shared());
}

#[test]
fn an_exact_tie_on_every_term_is_a_shared_win() {
    let mut runner = Runner::new(world());
    assert!(runner.begin_push());
    play_to_the_last_tick(&mut runner);
    hold(runner.world_mut(), [3_000, 3_000, 2_999]);
    let events = finish(&mut runner);

    let outcome = runner.outcome().expect("ended");
    assert_eq!(outcome.reason, MatchEndReason::RoundLimit);
    assert_eq!(
        outcome.winner, None,
        "a shared win is recorded as no single winner: there is no draw state"
    );
    let audit = final_audit(runner.world());
    assert!(audit.is_shared());
    assert_eq!(
        audit.winners,
        vec![seat(0), seat(1)],
        "the tied set is read back from the world, not stored"
    );
    let ended = events
        .iter()
        .find(|event| event.kind == EventKind::MatchEnded)
        .expect("the match announced its end");
    assert_eq!(ended.seat, None);
}

#[test]
fn a_lost_beacons_row_survives_save_and_restore() {
    let mut base = world();
    let lost = extra_beacon(&mut base, seat(2));
    let mut runner = Runner::new(base);
    assert!(runner.begin_push());
    destroy(&mut runner, lost);
    let mut stepped = 0;
    while stepped < 5 && runner.step().is_some() {
        stepped += 1;
    }
    let row = usize::try_from(lost.raw()).unwrap();
    assert_eq!(
        runner.world().beacons().hit_points().get(row),
        Some(&Hp::ZERO)
    );
    assert_eq!(beacons_lost(runner.world(), seat(2)), 1);

    let saved = Snapshot::capture(runner.world());
    let mut resumed = world();
    saved.restore_into(&mut resumed).expect("the save restores");
    assert_eq!(
        resumed.beacons().ids().get(row).copied(),
        Some(lost.raw()),
        "a dead beacon keeps its row"
    );
    assert_eq!(resumed.beacons().hit_points().get(row), Some(&Hp::ZERO));
    assert_eq!(
        beacons_lost(&resumed, seat(2)),
        1,
        "so the audit's third term survives a save, with no column of its own"
    );
    assert_eq!(resumed.state_hash(), runner.world().state_hash());
}

/// Four seats on the committed map's three spawn zones: seat 3 is seated and
/// never placed, as in the determinism harness's world.
fn four_seat_world() -> World {
    World::new(&WorldConfig {
        match_seed: pharmakos_sim::DETERMINISM_MATCH_SEED,
        seats: 4,
        units_per_seat: 0,
        rules: flat_rules(),
        match_settings: MatchSettings {
            segment_lengths_ms: vec![2_000],
            round_limit: 1,
        },
    })
    .expect("the rules table describes a map")
}

fn core_of(world: &World, of: SeatId) -> BeaconId {
    let beacons = world.beacons();
    (0..beacons.ids().len())
        .find(|row| beacons.seats().get(*row).copied() == Some(of.raw()))
        .and_then(|row| beacons.ids().get(row).copied())
        .map(BeaconId::new)
        .expect("a placed seat has a core")
}

#[test]
fn a_seat_that_was_never_placed_is_not_audited_at_the_round_limit() {
    let mut base = four_seat_world();
    assert!(
        (0..base.beacons().ids().len())
            .all(|row| base.beacons().seats().get(row).copied() != Some(3)),
        "the fixture's seat 3 has no beacon"
    );
    base.set_treasury(seat(3), Money::new(1_000_000));
    let mut runner = Runner::new(base);
    assert!(runner.begin_push());
    play_to_the_last_tick(&mut runner);
    hold(runner.world_mut(), [1_000, 1_500, 900]);
    let _ = finish(&mut runner);

    let outcome = runner.outcome().expect("the round limit ended the match");
    assert_eq!(outcome.reason, MatchEndReason::RoundLimit);
    assert_eq!(
        outcome.winner,
        Some(seat(1)),
        "a seat that was never in the match cannot win it, whatever it holds"
    );
    let audit = final_audit(runner.world());
    assert_eq!(
        audit.lines.iter().map(|line| line.seat).collect::<Vec<_>>(),
        vec![seat(0), seat(1), seat(2)]
    );
}

#[test]
fn a_seat_that_was_never_placed_does_not_survive_a_no_survivor_end() {
    let mut base = four_seat_world();
    base.set_treasury(seat(1), Money::new(500_000));
    base.set_treasury(seat(3), Money::new(1_000_000));
    let cores = [
        core_of(&base, seat(0)),
        core_of(&base, seat(1)),
        core_of(&base, seat(2)),
    ];
    let mut runner = Runner::new(base);
    assert!(runner.begin_push());
    for core in cores {
        destroy(&mut runner, core);
    }
    let report = runner.step().expect("a Push tick");
    assert!(report.match_ended, "{report:?}");

    let outcome = runner.outcome().expect("every placed seat fell on one tick");
    assert_eq!(outcome.reason, MatchEndReason::NoSurvivor);
    assert_eq!(
        outcome.winner,
        Some(seat(1)),
        "the audit decides between the seats that fell on the final tick"
    );
    let audit = final_audit(runner.world());
    assert_eq!(
        audit.lines.iter().map(|line| line.seat).collect::<Vec<_>>(),
        vec![seat(0), seat(1), seat(2)],
        "and a seat that was never placed is not among them"
    );
}
