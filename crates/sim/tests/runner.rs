// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The runner: the phases and their fixed order, the segment ladder, the frozen
//! segment-end snapshot, the one-tick match-end rule, elimination, the
//! commander's respawn and the event bus (T10).
//!
//! Every test here is named after the rule it enforces, which is the shape the
//! skeleton plan asks for: a failure should say which rule broke, not which
//! assertion did.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules — not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording), and that covers an out-of-bounds index in an assertion for the same reason: these are panic lints, not determinism lints, and AGENTS.md §5's ban is on the latter."
)]

use std::path::PathBuf;

use pharmakos_proto::gp;
use pharmakos_sim::encoding::Enc;
use pharmakos_sim::events::{Event, EventKind};
use pharmakos_sim::knowledge::Position;
use pharmakos_sim::math::fixed::Fx;
use pharmakos_sim::math::quantity::{Hp, MS_PER_TICK, Ms, Tick};
use pharmakos_sim::runner::{
    DEFAULT_ROUND_LIMIT, MatchEndReason, MatchPhase, MatchSettings, PHASE_CYCLE, Runner,
    SealRefused,
};
use pharmakos_sim::snapshot::{SNAPSHOT_VERSION, Snapshot, SnapshotError};
use pharmakos_sim::tables::{BeaconId, NO_RESPAWN, NOT_ELIMINATED, SeatId, StructureKind};
use pharmakos_sim::world::{DamageOrder, DamageTarget};
use pharmakos_sim::{RulesTable, World, WorldConfig};

/// A segment short enough that a test can play the whole of it.
const SHORT_MS: i32 = 5_000;
/// Ticks in [`SHORT_MS`], at 20 Hz.
const SHORT_TICKS: u32 = 100;

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

/// The committed table with one edit to the message, the same path a tuning
/// pull request takes.
fn rules_edited(edit: impl FnOnce(&mut gp::v1::RulesTable)) -> RulesTable {
    let text = std::fs::read_to_string(repo_root().join("rules").join("rules.v1.json"))
        .expect("the committed rules table exists");
    let mut message: gp::v1::RulesTable =
        pharmakos_proto::json::decode(&text).expect("the committed table is canonical gp.v1 JSON");
    edit(&mut message);
    RulesTable::from_message(&message).expect("the edited table is still a table")
}

/// A world with `seats` seats, its starting force and nothing else, playing
/// `lengths`.
fn world(seats: u32, lengths: &[i32]) -> World {
    world_on(rules(), seats, lengths)
}

fn world_on(rules: RulesTable, seats: u32, lengths: &[i32]) -> World {
    World::new(&WorldConfig {
        match_seed: pharmakos_sim::DETERMINISM_MATCH_SEED,
        seats,
        units_per_seat: 0,
        rules,
        match_settings: MatchSettings {
            segment_lengths_ms: lengths.to_vec(),
            round_limit: DEFAULT_ROUND_LIMIT,
        },
    })
    .expect("the committed rules table describes a world")
}

/// A runner in its opening Lull.
fn runner(seats: u32, lengths: &[i32]) -> Runner {
    Runner::new(world(seats, lengths))
}

/// Step `ticks` ticks, stopping early if the Push closes.
fn play(runner: &mut Runner, ticks: u32) -> u32 {
    let mut ran = 0;
    while ran < ticks {
        if runner.step().is_none() {
            break;
        }
        ran += 1;
    }
    ran
}

/// A seat's first beacon — its core (spec section 3).
fn core_of(world: &World, seat: u8) -> BeaconId {
    let index = world
        .beacons()
        .seats()
        .iter()
        .position(|owner| *owner == seat)
        .expect("every occupied seat has a core");
    BeaconId::new(world.beacons().ids()[index])
}

/// Kill something outright.
fn destroy(runner: &mut Runner, target: DamageTarget) {
    assert!(
        runner.world_mut().request_damage(DamageOrder {
            target,
            amount: Hp::new(1_000_000),
        }),
        "the damage queue has room"
    );
}

fn kinds(events: &[Event]) -> Vec<&'static str> {
    events.iter().map(|event| event.kind.name()).collect()
}

fn has(events: &[Event], kind: EventKind) -> bool {
    events.iter().any(|event| event.kind == kind)
}

// ---------------------------------------------------------------------------
// The phases and their fixed order
// ---------------------------------------------------------------------------

#[test]
fn the_phases_run_in_their_fixed_order() {
    assert_eq!(
        PHASE_CYCLE,
        [MatchPhase::Lull, MatchPhase::Push, MatchPhase::Recap],
        "the round's cycle is Lull, Push, recap (spec section 3)"
    );

    let mut runner = runner(2, &[SHORT_MS]);
    assert_eq!(runner.phase(), MatchPhase::Lull, "a match opens in a Lull");
    assert_eq!(runner.round(), 1);

    // A Lull consumes no tick: it is a state, not a duration (AGENTS.md §4.5 —
    // the Lull's timer is the host's and the sim never reads it).
    assert!(runner.step().is_none(), "no tick runs outside a Push");
    assert_eq!(runner.tick(), Tick::ZERO);

    assert!(runner.begin_push());
    assert_eq!(runner.phase(), MatchPhase::Push);
    assert!(!runner.begin_push(), "a Push cannot be opened twice");

    assert_eq!(play(&mut runner, SHORT_TICKS), SHORT_TICKS);
    assert_eq!(runner.phase(), MatchPhase::Recap, "the segment ran out");
    assert!(runner.step().is_none(), "no tick runs during a recap");

    assert!(runner.end_recap());
    assert_eq!(runner.phase(), MatchPhase::Lull);
    assert_eq!(runner.round(), 2, "the recap opened the next round");
    assert!(!runner.end_recap(), "a recap cannot be closed twice");
}

#[test]
fn a_push_runs_exactly_the_segments_length_in_ticks() {
    let mut runner = runner(2, &[SHORT_MS]);
    assert!(runner.begin_push());
    assert_eq!(
        runner.world().match_state().segment_ticks(),
        SHORT_TICKS,
        "5 000 ms is 100 ticks at 20 Hz"
    );

    assert_eq!(play(&mut runner, SHORT_TICKS - 1), SHORT_TICKS - 1);
    assert_eq!(runner.phase(), MatchPhase::Push, "one tick still to run");
    assert!(runner.step().is_some());
    assert_eq!(runner.phase(), MatchPhase::Recap);
    assert_eq!(runner.tick(), Tick::new(SHORT_TICKS));
}

#[test]
fn the_ladder_repeats_its_last_entry_for_rounds_past_the_end() {
    // Item 40: N values give round i the i-th entry, and rounds past the list
    // take the last one.
    let world = world(2, &[3_000, 7_000]);
    let state = world.match_state();
    assert_eq!(state.length_for_round(1), Ms::new(3_000));
    assert_eq!(state.length_for_round(2), Ms::new(7_000));
    assert_eq!(state.length_for_round(3), Ms::new(7_000));
    assert_eq!(state.length_for_round(99), Ms::new(7_000));
}

#[test]
fn an_empty_host_list_plays_the_rules_tables_ladder() {
    // Item 40: empty means the ladder, which is item 68's 3 / 5 / 8 at the
    // skeleton.
    let world = world(2, &[]);
    assert_eq!(
        world.match_state().segment_lengths_ms(),
        rules().segment_lengths_ms(),
        "an empty host list resolves to the rules table's ladder"
    );
}

#[test]
fn a_degenerate_ladder_is_clamped_rather_than_trusted() {
    // A lobby setting is not trusted. `round_limit` was already clamped to a
    // round; every ladder entry is clamped to a tick for the same reason, so a
    // zero or a negative cannot describe a round that closes before anything in
    // it runs while the editor's clock shows nothing.
    let world = world(2, &[0, -5_000, 1]);
    let state = world.match_state();
    assert_eq!(
        state.segment_lengths_ms(),
        [MS_PER_TICK, MS_PER_TICK, MS_PER_TICK],
        "every entry is floored at one tick"
    );
    assert_eq!(state.coming_segment_ms(), Ms::new(MS_PER_TICK));

    // And the same on the way back from a file. Structure is refused at the
    // door (an undefined phase is), but a length is a value and takes the same
    // clamp — refusing it would make `Snapshot::default()`, which carries no
    // ladder at all, unrestorable.
    let mut runner = runner(2, &[SHORT_MS]);
    let mut saved = runner.capture();
    saved.match_segment_lengths_ms.clear();
    saved.coming_segment_ms = i32::MIN;
    runner
        .restore(&saved)
        .expect("a length is normalised, not refused");
    assert!(runner.begin_push());
    assert_eq!(
        runner.world().match_state().segment_ticks(),
        1,
        "a Push runs at least one tick, whatever the file said"
    );
    assert!(runner.step().is_some(), "and that tick runs");
}

#[test]
fn the_match_state_is_in_the_state_hash() {
    // AGENTS.md §4.8: a field that decides what a tick does is hashed. Every
    // field of the match state does, so every one of them moves the chain.
    let mut runner = runner(2, &[SHORT_MS]);
    let in_lull = runner.world().state_hash();
    assert!(runner.begin_push());
    let in_push = runner.world().state_hash();
    assert_ne!(
        in_lull, in_push,
        "opening a Push changes the phase, the segment's start and its length"
    );

    // And the ladder is in it too: two worlds identical but for the coming
    // segment's length hash differently at tick zero.
    assert_ne!(
        world(2, &[SHORT_MS]).state_hash(),
        world(2, &[SHORT_MS + 1_000]).state_hash(),
        "the effective ladder decides every later segment, so it is hashed"
    );
}

// ---------------------------------------------------------------------------
// The frozen segment-end snapshot
// ---------------------------------------------------------------------------

#[test]
fn the_coming_segments_length_comes_from_the_snapshot_not_a_constant() {
    // Round 1 runs 5 s and round 2 is to run 15 s.
    let mut first = runner(2, &[SHORT_MS, 15_000]);
    assert!(first.begin_push());
    play(&mut first, SHORT_TICKS);
    assert_eq!(first.phase(), MatchPhase::Recap);

    let frozen = first.frozen();
    assert_eq!(frozen.round(), 1);
    assert_eq!(frozen.taken_at(), Tick::new(SHORT_TICKS));
    assert_eq!(
        frozen.coming_segment_ms(),
        Ms::new(15_000),
        "the segment-end snapshot carries the coming segment's length (item 30)"
    );
    let mut carried = frozen.snapshot().clone();
    assert_eq!(carried.coming_segment_ms, 15_000, "and so do its bytes");

    // Edit the carried number to something **the restored ladder cannot
    // produce**. The file still says the ladder is [5 s, 15 s], so a Push that
    // re-derived its length — from the rules table, or from the ladder and the
    // round number — would run 15 s. Only a Push that consumes
    // `coming_segment_ms` itself runs 9 s. Inconsistent on purpose: the point
    // is which of the two fields the Push reads.
    carried.coming_segment_ms = 9_000;

    // And restore it into a world built under a **different** ladder again, so
    // the rules table's own answer (1 s) is a third distinguishable number.
    let other = rules_edited(|message| {
        if let Some(block) = message.r#match.as_mut() {
            block.segment_lengths_ms = vec![1_000];
        }
    });
    let mut second = Runner::new(world_on(other, 2, &[]));
    second
        .restore(&carried)
        .expect("the frozen snapshot restores");

    assert_eq!(second.phase(), MatchPhase::Recap);
    assert_eq!(
        second.world().match_state().segment_lengths_ms(),
        [SHORT_MS, 15_000],
        "the ladder travelled in the file, so re-deriving would have 15 s to hand"
    );
    assert_eq!(
        second.frozen().coming_segment_ms(),
        Ms::new(9_000),
        "a restore carries the number rather than re-deriving it"
    );
    assert!(second.end_recap());
    assert_eq!(second.round(), 2);
    assert!(second.begin_push());
    assert_eq!(
        second.world().match_state().segment_ticks(),
        180,
        "round 2 runs the 9 s the snapshot carried — not the 15 s its own ladder would give for \
         round 2, and not the 1 s this rules table would give"
    );
}

#[test]
fn a_restored_match_keeps_its_phase_round_and_ladder() {
    let mut first = runner(3, &[SHORT_MS, 6_000, 7_000]);
    assert!(first.begin_push());
    play(&mut first, 40);
    let saved = first.capture();
    let hash = first.world().state_hash();

    let mut second = Runner::new(world(3, &[]));
    second.restore(&saved).expect("the snapshot restores");

    assert_eq!(second.world().state_hash(), hash, "a restore is lossless");
    assert_eq!(second.phase(), MatchPhase::Push);
    assert_eq!(second.round(), 1);
    assert_eq!(second.tick(), Tick::new(40));
    assert_eq!(
        second.world().match_state().segment_lengths_ms(),
        [SHORT_MS, 6_000, 7_000],
        "the effective ladder travels in the file, because it decides every later segment"
    );
}

#[test]
fn a_snapshot_in_a_phase_this_build_does_not_define_is_refused() {
    let mut saved = Snapshot {
        version: SNAPSHOT_VERSION,
        ..Snapshot::default()
    };
    saved.match_phase = 99;
    let mut runner = runner(2, &[SHORT_MS]);
    let before = runner.world().state_hash();
    match runner.restore(&saved) {
        Err(SnapshotError::MatchState { phase, end_reason }) => {
            assert_eq!(phase, 99);
            assert_eq!(end_reason, 0);
        }
        other => panic!("expected a match-state refusal, got {other:?}"),
    }
    assert_eq!(
        runner.world().state_hash(),
        before,
        "a refused restore changes nothing"
    );
}

// ---------------------------------------------------------------------------
// The one-tick match-end rule and the round limit
// ---------------------------------------------------------------------------

#[test]
fn the_match_end_rule_is_checked_at_the_tick_not_at_the_segment_boundary() {
    // Item 16: end conditions are evaluated every tick and the Push halts at
    // the **first** tick where fewer than two seats remain.
    let mut runner = runner(2, &[SHORT_MS]);
    assert!(runner.begin_push());
    play(&mut runner, 5);
    runner.clear_events();

    let core = core_of(runner.world(), 1);
    destroy(&mut runner, DamageTarget::Beacon(core));
    let report = runner.step().expect("the tick runs");

    assert_eq!(report.tick, Tick::new(6));
    assert!(report.match_ended, "the tick that decided it says so");
    assert_eq!(
        runner.phase(),
        MatchPhase::Recap,
        "standings freeze and the recap plays; `Ended` comes after it (item 16)"
    );
    let outcome = runner.outcome().expect("the match was decided");
    assert_eq!(outcome.reason, MatchEndReason::LastSeatStanding);
    assert_eq!(outcome.winner, Some(SeatId::new(0)));
    assert_eq!(
        outcome.at,
        Tick::new(6),
        "the tick the condition first held, not the segment's last tick"
    );
    assert!(
        Tick::new(6) < Tick::new(SHORT_TICKS),
        "and that tick is well inside the segment"
    );

    let fired = kinds(runner.events());
    assert!(fired.contains(&"beacon_destroyed"), "{fired:?}");
    assert!(fired.contains(&"seat_eliminated"), "{fired:?}");
    assert!(fired.contains(&"segment_ended"), "{fired:?}");
    assert!(fired.contains(&"match_ended"), "{fired:?}");
    assert!(
        runner.step().is_none(),
        "a decided match runs no more ticks"
    );

    runner.clear_events();
    assert!(runner.end_recap());
    assert_eq!(runner.phase(), MatchPhase::Ended);
    assert!(
        !has(runner.events(), EventKind::MatchEnded),
        "the end was announced at the tick it happened, and is not announced twice"
    );
    assert!(runner.step().is_none());
}

#[test]
fn a_seat_with_no_beacons_is_eliminated_and_its_units_disband() {
    let mut runner = runner(3, &[SHORT_MS]);
    assert!(runner.begin_push());
    play(&mut runner, 3);
    runner.clear_events();

    let core = core_of(runner.world(), 2);
    destroy(&mut runner, DamageTarget::Beacon(core));
    runner.step().expect("the tick runs");

    assert_eq!(
        runner.phase(),
        MatchPhase::Push,
        "two seats remain, so the match runs on"
    );
    let world = runner.world();
    assert_eq!(world.seats().eliminated_at()[2], 4);
    assert!(world.seats().is_alive(0) && world.seats().is_alive(1));

    for (index, seat) in world.units().seats().iter().enumerate() {
        if *seat == 2 {
            assert!(
                !world.units().hit_points()[index].is_alive(),
                "unit {index} disbanded on the spot"
            );
        } else {
            assert!(
                world.units().hit_points()[index].is_alive(),
                "unit {index} belongs to a seat that is still standing"
            );
        }
    }
    assert!(has(runner.events(), EventKind::SeatEliminated));
}

#[test]
fn an_unplaced_seat_is_neither_eliminated_nor_counted() {
    // The determinism harness runs four seats on a three-zone map on purpose
    // (a wider table is a wider hash). A seat beyond the map's zones never had
    // a core, so it is not eliminated — and the one-tick rule never sees it,
    // which is why a four-seat harness on three zones is not a match that ends
    // on its first tick.
    let mut runner = Runner::new(
        World::new(&WorldConfig {
            match_seed: pharmakos_sim::DETERMINISM_MATCH_SEED,
            seats: 4,
            units_per_seat: 0,
            rules: rules(),
            match_settings: MatchSettings {
                segment_lengths_ms: vec![SHORT_MS],
                round_limit: DEFAULT_ROUND_LIMIT,
            },
        })
        .expect("four seats on the committed table"),
    );
    assert!(
        !runner.world().beacons().seats().contains(&3),
        "the committed table carries three spawn zones, so seat 3 is unplaced"
    );

    assert!(runner.begin_push());
    play(&mut runner, 10);
    assert_eq!(runner.phase(), MatchPhase::Push);
    assert_eq!(
        runner.world().seats().eliminated_at()[3],
        NOT_ELIMINATED,
        "a seat that was never placed had no core to lose"
    );
}

#[test]
fn the_round_limit_ends_the_match_when_the_last_recap_closes() {
    let mut world = world(2, &[1_000]);
    // One round, so the test plays the whole match.
    assert!(world.match_state().round_limit() >= 1);
    world = World::new(&WorldConfig {
        match_seed: pharmakos_sim::DETERMINISM_MATCH_SEED,
        seats: 2,
        units_per_seat: 0,
        rules: rules(),
        match_settings: MatchSettings {
            segment_lengths_ms: vec![1_000],
            round_limit: 2,
        },
    })
    .expect("a two-round match");

    let mut runner = Runner::new(world);
    for round in 1..=2_u32 {
        assert_eq!(runner.round(), round);
        assert!(runner.begin_push());
        assert_eq!(play(&mut runner, 20), 20);
        assert_eq!(runner.phase(), MatchPhase::Recap);
        assert!(runner.end_recap());
    }
    assert_eq!(runner.phase(), MatchPhase::Ended);
    let outcome = runner.outcome().expect("the round limit ended it");
    assert_eq!(outcome.reason, MatchEndReason::RoundLimit);
    assert_eq!(
        outcome.winner, None,
        "the final audit that names a winner is the economy's (T14)"
    );
    assert!(has(runner.events(), EventKind::MatchEnded));
}

// ---------------------------------------------------------------------------
// Local elimination (item 20)
// ---------------------------------------------------------------------------

#[test]
fn a_beacon_death_ruins_the_structures_homed_to_it() {
    // The skeleton's generator places no structures, so the structure is put
    // there through the snapshot — which is also the restore path's own test.
    let mut runner = runner(3, &[SHORT_MS]);
    let core = core_of(runner.world(), 2);
    let at = runner.world().beacons().positions()[0];

    let mut saved = runner.capture();
    saved.structure_id.push(0);
    saved.structure_seat.push(2);
    saved.structure_kind.push(StructureKind::Generator.id());
    saved
        .structure_pos
        .extend([at[0].raw(), at[1].raw(), at[2].raw()]);
    saved.structure_hp.push(500);
    saved.structure_home.push(core.raw());
    runner
        .restore(&saved)
        .expect("the edited snapshot restores");
    assert_eq!(runner.world().structures().len(), 1);

    assert!(runner.begin_push());
    play(&mut runner, 2);
    runner.clear_events();
    destroy(&mut runner, DamageTarget::Beacon(core));
    runner.step().expect("the tick runs");

    assert_eq!(
        runner.world().structures().seats()[0],
        SeatId::NEUTRAL.raw(),
        "the dead beacon's structure is a neutral ruin (item 20)"
    );
    assert!(has(runner.events(), EventKind::StructureRuined));

    // And the ruin is reported once, not on every tick after it.
    runner.clear_events();
    play(&mut runner, 5);
    assert!(
        !has(runner.events(), EventKind::StructureRuined),
        "ruination fires on the tick the beacon died and not again"
    );
}

// ---------------------------------------------------------------------------
// The commander's death and respawn (item 21)
// ---------------------------------------------------------------------------

#[test]
fn a_pending_respawn_always_completes_by_segment_end() {
    let mut runner = runner(2, &[SHORT_MS]);
    assert!(runner.begin_push());
    play(&mut runner, 5);

    let commander = runner.world().commander_of(SeatId::new(0));
    assert!(commander.is_some(), "an occupied seat has a commander");
    destroy(&mut runner, DamageTarget::Unit(commander));
    runner.step().expect("the tick runs");

    let due = runner.world().seats().respawn_due()[0];
    assert_ne!(due, NO_RESPAWN, "a death schedules a respawn");
    assert!(
        due > SHORT_TICKS,
        "the 30 s base delay is 600 ticks and lands well past this 100-tick segment ({due})"
    );
    assert!(has(runner.events(), EventKind::CommanderDied));

    // Run the segment out. The respawn is not due, and completes anyway.
    play(&mut runner, SHORT_TICKS);
    assert_eq!(runner.phase(), MatchPhase::Recap);

    let world = runner.world();
    let index = usize::try_from(commander.raw()).unwrap();
    assert!(
        world.units().hit_points()[index].is_alive(),
        "every Lull snapshot has a live commander at a known place (item 21)"
    );
    assert_eq!(world.seats().respawn_due()[0], NO_RESPAWN);
    assert_eq!(
        world.units().positions()[index],
        world.beacons().positions()[usize::try_from(core_of(world, 0).raw()).unwrap()],
        "it comes back at the core"
    );
    assert_eq!(
        runner.frozen().snapshot().seat_respawn_due[0],
        NO_RESPAWN,
        "and the frozen snapshot says so too"
    );
}

#[test]
fn the_respawn_delay_grows_with_each_death_in_the_same_push() {
    let mut runner = runner(2, &[600_000]);
    assert!(runner.begin_push());
    let commander = runner.world().commander_of(SeatId::new(0));

    let mut delays: Vec<u32> = Vec::new();
    for _ in 0..3_u32 {
        destroy(&mut runner, DamageTarget::Unit(commander));
        let died = runner.step().expect("the tick runs").tick;
        let due = runner.world().seats().respawn_due()[0];
        delays.push(due.saturating_sub(died.raw()));
        // Run to the respawn.
        while runner.world().seats().respawn_due()[0] != NO_RESPAWN {
            runner.step().expect("the segment is long enough");
        }
    }

    // `commander.respawn_base_ms` = 30 000 and `respawn_growth_ms` = 15 000
    // (item 90's rows), read from the rules table and never from a constant.
    assert_eq!(delays, vec![600, 900, 1_200], "30 s, 45 s, 60 s at 20 Hz");
    assert_eq!(runner.world().seats().commander_deaths()[0], 3);
}

#[test]
fn a_new_push_resets_the_per_push_death_counter() {
    let mut runner = runner(2, &[SHORT_MS, SHORT_MS]);
    assert!(runner.begin_push());
    let commander = runner.world().commander_of(SeatId::new(0));
    destroy(&mut runner, DamageTarget::Unit(commander));
    runner.step().expect("the tick runs");
    assert_eq!(runner.world().seats().commander_deaths()[0], 1);

    play(&mut runner, SHORT_TICKS);
    assert!(runner.end_recap());
    assert!(runner.begin_push());
    assert_eq!(
        runner.world().seats().commander_deaths()[0],
        0,
        "the death counter is per-Push state, reset each segment (item 21)"
    );
    assert_eq!(runner.world().seats().respawn_due()[0], NO_RESPAWN);
}

// ---------------------------------------------------------------------------
// The event bus
// ---------------------------------------------------------------------------

#[test]
fn every_kind_is_assertable() {
    // Item 97, and the rule `crates/gateway/src/feed.rs`'s `Kind::new` applies
    // to anything that reaches a seat's feed: lower-case ASCII letters, digits
    // and underscores, starting with a letter.
    let mut seen: Vec<&'static str> = Vec::new();
    for kind in EventKind::ALL {
        let name = kind.name();
        assert!(!name.is_empty() && name.len() <= 48, "{name}");
        assert!(
            name.starts_with(|c: char| c.is_ascii_lowercase()),
            "`{name}` does not start with a lower-case ASCII letter"
        );
        assert!(
            name.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
            "`{name}` is not lower_snake_case"
        );
        assert!(!seen.contains(&name), "`{name}` is used twice");
        seen.push(name);
        assert_eq!(EventKind::from_id(kind.id()), Some(kind));
        assert_eq!(EventKind::from_name(name), Some(kind));
    }
    assert_eq!(EventKind::from_id(0), None);
    assert_eq!(EventKind::from_name("Beacon_Placed"), None);
}

#[test]
fn the_event_bus_is_not_in_the_state_encoding() {
    // Derived output, not hashed state: nothing in a tick reads an event, so
    // dropping one cannot change what the world does next (AGENTS.md §4.8 is
    // about fields that affect behaviour).
    let mut runner = runner(2, &[SHORT_MS]);
    assert!(runner.begin_push());
    play(&mut runner, 10);

    let before = runner.world().state_hash();
    let events = runner.events().len();
    assert!(events > 0, "the opening emitted something to drain");
    runner.clear_events();
    assert_eq!(runner.events().len(), 0);
    assert_eq!(
        runner.world().state_hash(),
        before,
        "draining the bus leaves the hash exactly where it was"
    );

    let mut enc = Enc::with_capacity(64 * 1024);
    runner.world().encode(&mut enc);
    let drained = enc.encoded_len();
    play(&mut runner, 1);
    runner.world().encode(&mut enc);
    assert_eq!(
        enc.encoded_len(),
        drained,
        "the encoding's length is a function of the tables, never of the feed"
    );

    // And it is not in the file either.
    let saved = runner.capture();
    let mut other = Runner::new(world(2, &[SHORT_MS]));
    other.restore(&saved).expect("the snapshot restores");
    assert!(
        other.events().is_empty(),
        "a restored world starts with an empty feed, as `crate::events` says"
    );
}

#[test]
fn a_restore_re_anchors_the_feed_so_two_resumes_agree() {
    // The bus is derived output, so a resumed match starts with an empty feed —
    // that price is written down in `crate::events`. What must **not** also be
    // true is that the receiving runner's history leaks into the resumed feed:
    // `seq` is anchored to a tick, a drain deliberately does not reset it, and
    // a restore therefore has to. Without the re-anchor the two receivers below
    // emit the same `lull_opened` under different `seq` values, because one of
    // them happens to be anchored on the very tick the file was saved at.
    let mut source = runner(2, &[SHORT_MS, 15_000]);
    assert!(source.begin_push());
    assert_eq!(play(&mut source, SHORT_TICKS), SHORT_TICKS);
    let saved = source.capture();

    let mut fresh = Runner::new(world(2, &[SHORT_MS, 15_000]));
    let mut used = Runner::new(world(2, &[SHORT_MS, 15_000]));
    assert!(used.begin_push());
    assert_eq!(play(&mut used, SHORT_TICKS), SHORT_TICKS);
    used.clear_events();
    assert_eq!(
        used.tick(),
        Tick::new(SHORT_TICKS),
        "the second receiver's bus is anchored on the tick the file was saved at"
    );

    let resume = |runner: &mut Runner| -> Vec<(u32, u32, &'static str)> {
        runner.restore(&saved).expect("the snapshot restores");
        assert!(runner.events().is_empty(), "a restore empties the feed");
        assert!(runner.end_recap());
        assert!(runner.begin_push());
        runner.step().expect("the next segment runs");
        runner
            .events()
            .iter()
            .map(|event| (event.tick.raw(), event.seq, event.kind.name()))
            .collect()
    };
    assert_eq!(
        resume(&mut fresh),
        resume(&mut used),
        "the feed after a restore is a function of the restored state, never of what the \
         receiving runner had been doing"
    );
}

#[test]
fn the_same_seed_produces_the_same_events() {
    let collect = || {
        let mut runner = runner(2, &[SHORT_MS]);
        let mut all: Vec<(u32, u32, &'static str)> = Vec::new();
        assert!(runner.begin_push());
        for _ in 0..SHORT_TICKS {
            if runner.step().is_none() {
                break;
            }
            for event in runner.events() {
                all.push((event.tick.raw(), event.seq, event.kind.name()));
            }
            runner.clear_events();
        }
        all
    };
    let first = collect();
    let second = collect();
    assert_eq!(first, second, "the feed is a function of the state");

    // And the order is total: `(tick, seq)` strictly increases.
    for pair in first.windows(2) {
        assert!(
            (pair[0].0, pair[0].1) < (pair[1].0, pair[1].1),
            "events are ordered by (tick, seq), never by arrival: {pair:?}"
        );
    }
}

#[test]
fn a_host_that_drains_never_meets_a_full_bus() {
    // The overflow behaviour itself — drop, count, never grow — is unit-tested
    // in `src/events.rs`, where the emitter is reachable. What belongs here is
    // the claim `EVENT_BUS_CAPACITY`'s own doc makes: at the skeleton's world
    // size, a host draining once a tick stays under the capacity **including on
    // the tick the capacity was sized against**, which is a seat's elimination
    // — one line per beacon of that seat plus one per structure homed to it.
    // So this runs at the determinism harness's units per seat and kills a
    // seat's core inside the measured loop rather than measuring a handful of
    // phase transitions.
    let mut runner = Runner::new(
        World::new(&WorldConfig {
            match_seed: pharmakos_sim::DETERMINISM_MATCH_SEED,
            seats: 3,
            units_per_seat: pharmakos_sim::DETERMINISM_UNITS_PER_SEAT,
            rules: rules(),
            match_settings: MatchSettings {
                segment_lengths_ms: vec![SHORT_MS],
                round_limit: DEFAULT_ROUND_LIMIT,
            },
        })
        .expect("a three-seat world at the harness's size"),
    );
    let capacity = runner.world().event_bus().capacity();
    assert!(runner.begin_push());

    let core = core_of(runner.world(), 2);
    let mut peak = 0;
    let mut elimination_peak = 0;
    for tick in 0..SHORT_TICKS {
        if tick == 10 {
            destroy(&mut runner, DamageTarget::Beacon(core));
        }
        if runner.step().is_none() {
            break;
        }
        let waiting = runner.events().len();
        peak = peak.max(waiting);
        if has(runner.events(), EventKind::SeatEliminated) {
            elimination_peak = waiting;
        }
        runner.clear_events();
    }
    assert!(
        elimination_peak > 0,
        "the elimination tick the capacity is sized against ran inside the measured loop"
    );
    assert!(
        peak < capacity,
        "a drained bus stayed under its capacity ({peak} of {capacity}), elimination included"
    );
    assert_eq!(runner.world().event_bus().dropped(), 0);
}

#[test]
fn a_position_rides_with_an_event_that_happened_somewhere() {
    // The gateway's fog filter decides visibility from the position, so an
    // event about a place carries one.
    let mut runner = runner(3, &[SHORT_MS]);
    assert!(runner.begin_push());
    play(&mut runner, 2);
    runner.clear_events();
    let core = core_of(runner.world(), 2);
    let at = runner.world().beacons().positions()[usize::try_from(core.raw()).unwrap()];
    destroy(&mut runner, DamageTarget::Beacon(core));
    runner.step().expect("the tick runs");

    let destroyed = runner
        .events()
        .iter()
        .find(|event| event.kind == EventKind::BeaconDestroyed)
        .expect("the beacon's death was reported");
    assert_eq!(destroyed.seat, Some(SeatId::new(2)));
    assert_eq!(
        destroyed.at.map(Position::to_array),
        Some([at[0], at[1], Fx::from_raw(at[2].raw())])
    );
}

// ---------------------------------------------------------------------------
// Sealing a playbook (T13b, decisions-log item 103 (1))
// ---------------------------------------------------------------------------

/// The smallest playbook that compiles: one hold, a `CONTINUE` respawn and a
/// fallback. Deliberately not a golden case — this file is about the runner's
/// door, not about what the interpreter does once through it.
const A_PLAYBOOK: &str = concat!(
    "{\"schema_version\":{\"major\":1},",
    "\"meta\":{\"title\":\"t\",\"author_kind\":\"HUMAN\"},",
    "\"declarative\":{\"route\":[{\"label\":\"a\",\"hold\":{\"ms\":1000}}]},",
    "\"on_death\":{\"on_respawn\":\"CONTINUE\"},",
    "\"fallback\":{\"hold\":{\"at\":{\"beacon_anchor\":{\"safest\":{}}}}},",
    "\"kind\":\"PLAYBOOK\"}"
);

fn a_plan() -> pharmakos_sim::interpreter::Plan {
    let playbook: gp::v1::Playbook =
        pharmakos_proto::json::decode(A_PLAYBOOK).expect("canonical gp.v1 JSON");
    pharmakos_sim::interpreter::Plan::compile(&playbook, &rules()).expect("it compiles")
}

#[test]
fn a_playbook_is_sealed_during_a_lull_and_refused_in_every_other_phase() {
    let mut runner = runner(2, &[SHORT_MS]);
    assert_eq!(runner.phase(), MatchPhase::Lull);
    let before = runner.tick();
    runner
        .seal_playbook(SeatId::new(0), a_plan())
        .expect("a Lull is where a playbook is sealed");
    assert_eq!(runner.tick(), before, "sealing consumes no tick");
    assert!(
        runner.world().interpreter().plan(0).is_some(),
        "and the world is holding it"
    );

    // Item 5: the latest verified submission replaces the previous one, any
    // number of times, and the second seal is as legal as the first.
    runner
        .seal_playbook(SeatId::new(0), a_plan())
        .expect("replacing this Lull's seal");

    assert!(runner.begin_push());
    assert_eq!(
        runner.seal_playbook(SeatId::new(0), a_plan()),
        Err(SealRefused::NotInLull(MatchPhase::Push)),
        "nobody is in control during a Push"
    );

    while let Some(report) = runner.step() {
        if report.segment_ended {
            break;
        }
    }
    assert_eq!(runner.phase(), MatchPhase::Recap);
    assert_eq!(
        runner.seal_playbook(SeatId::new(0), a_plan()),
        Err(SealRefused::NotInLull(MatchPhase::Recap)),
        "a recap has no orders left to give"
    );

    // And the next Lull opens the door again.
    assert!(runner.end_recap());
    assert_eq!(runner.phase(), MatchPhase::Lull);
    runner
        .seal_playbook(SeatId::new(1), a_plan())
        .expect("round two's Lull");

    // The fourth phase, which the doc names and the walk above never reached:
    // a one-round match, played out to `Ended`. An ended match has nobody left
    // to give orders to, and the refusal says which phase it is in rather than
    // pretending the seal landed.
    let mut last = Runner::new(
        World::new(&WorldConfig {
            match_seed: pharmakos_sim::DETERMINISM_MATCH_SEED,
            seats: 2,
            units_per_seat: 0,
            rules: rules(),
            match_settings: MatchSettings {
                segment_lengths_ms: vec![SHORT_MS],
                round_limit: 1,
            },
        })
        .expect("a one-round match"),
    );
    assert!(last.begin_push());
    while let Some(report) = last.step() {
        if report.segment_ended {
            break;
        }
    }
    assert!(last.end_recap());
    assert_eq!(last.phase(), MatchPhase::Ended);
    assert_eq!(
        last.seal_playbook(SeatId::new(0), a_plan()),
        Err(SealRefused::NotInLull(MatchPhase::Ended)),
        "an ended match takes no orders either"
    );
}

#[test]
fn a_seal_for_a_seat_this_match_has_not_got_is_refused_by_name() {
    let mut runner = runner(2, &[SHORT_MS]);
    assert_eq!(
        runner.seal_playbook(SeatId::new(7), a_plan()),
        Err(SealRefused::NoSuchSeat(SeatId::new(7)))
    );
    assert!(
        runner.world().interpreter().plan(0).is_none(),
        "and nothing was filed for anybody else"
    );
}

/// The frozen snapshot is what the Lull plans *against*, and the verifier
/// hashes its bytes into every `report_hash` (spec section 11). A seal that
/// moved it would make a seat's pre-check and the check at submit disagree
/// about a world nothing had changed.
#[test]
fn a_seal_leaves_the_frozen_planning_snapshot_alone() {
    let mut runner = runner(2, &[SHORT_MS]);
    let before = runner.frozen().clone();
    let live = runner.world().state_hash();
    runner
        .seal_playbook(SeatId::new(0), a_plan())
        .expect("sealed");
    assert_eq!(
        runner.frozen(),
        &before,
        "the snapshot the seat planned against, and its identity hash, both stand"
    );
    assert_ne!(
        runner.world().state_hash(),
        live,
        "while the live world did move: the interpreter's state is hashed state (T11)"
    );
}

#[test]
fn a_seal_reports_itself_on_the_event_bus() {
    let mut runner = runner(2, &[SHORT_MS]);
    runner.clear_events();
    runner
        .seal_playbook(SeatId::new(1), a_plan())
        .expect("sealed");
    let sealed: Vec<&Event> = runner
        .events()
        .iter()
        .filter(|event| event.kind == EventKind::PlanSealed)
        .collect();
    assert_eq!(sealed.len(), 1);
    assert_eq!(sealed[0].seat, Some(SeatId::new(1)));
    assert_eq!(sealed[0].value, 1, "one route step");
}
