// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The settlement's two pure reads agree with the Ledger (decisions-log item
//! 134 (2) (c); S1's plan, task `fog`).
//!
//! `economy::ladder_place` restates the rank `World::settle_ledger` computes
//! inline, and `economy::band_percent` the percent `economy::bmi_for` pays
//! by; this file holds the two to the same credits. They are published so the
//! gateway's recap fills `Settlement.band_rank` and `band_percent` from the
//! sim's rule. The tick
//! still calls its own code (determinism code, AGENTS.md section 5); this
//! file plays the settlement golden's own match -- the fixture of
//! `tests/economy.rs`'s `the_settlement_ledger_matches_its_golden` -- and
//! holds every `settled` credit to what the two reads say, and the credits
//! themselves to `tests/golden/economy/expected.settlement.txt`.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml sets allow-expect-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report."
)]

use std::path::PathBuf;

use pharmakos_sim::economy::{band_percent, bmi_for, ladder_place};
use pharmakos_sim::events::{Event, EventKind};
use pharmakos_sim::math::quantity::Money;
use pharmakos_sim::runner::Runner;
use pharmakos_sim::tables::SeatId;
use pharmakos_sim::{MatchSettings, RulesTable, World, WorldConfig, default_rules_path};

/// The settlement golden's seed (`tests/economy.rs`).
const SEED: u64 = 0x00A1_B2C3_D4E5_F607;

/// Three seats, so the ladder has a middle.
const SEATS: u32 = 3;

fn rules() -> RulesTable {
    let path = default_rules_path().unwrap_or_else(|| PathBuf::from("rules/rules.v1.json"));
    RulesTable::load(&path).expect("the committed rules table loads")
}

fn repo_root() -> PathBuf {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.parent()
        .and_then(std::path::Path::parent)
        .map_or(here.clone(), PathBuf::from)
}

/// The settlement golden's match: three rounds of 120 s, the starting force
/// and nothing else.
fn world() -> World {
    World::new(&WorldConfig {
        match_seed: SEED,
        seats: SEATS,
        units_per_seat: 0,
        rules: rules(),
        match_settings: MatchSettings {
            segment_lengths_ms: vec![120_000, 120_000, 120_000],
            round_limit: 3,
        },
    })
    .expect("the world generates")
}

/// Play the Push in progress to its end, keeping every event.
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

/// The golden's `settled` lines, as `(round, seat, credit)`.
fn golden_credits() -> Vec<(u32, u8, i64)> {
    let path = repo_root()
        .join("tests")
        .join("golden")
        .join("economy")
        .join("expected.settlement.txt");
    let text = std::fs::read_to_string(&path).expect("the settlement golden is committed");
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            match fields.as_slice() {
                [round, seat, "settled", value, ..] => Some((
                    round.parse().expect("a round"),
                    seat.parse().expect("a seat"),
                    value.parse().expect("a credit"),
                )),
                _ => None,
            }
        })
        .collect()
}

#[test]
fn the_economy_reads_agree_with_the_settlement_ledgers_credit() {
    let rules = rules();
    let base = i64::from(
        rules
            .message()
            .economy
            .as_ref()
            .expect("an economy block")
            .bmi_dollars,
    );
    let mut runner = Runner::new(world());
    let mut feed: Vec<Event> = Vec::new();
    let mut credits: Vec<(u32, u8, i64)> = Vec::new();
    for round in 1..=3_u32 {
        feed.clear();
        play_segment(&mut runner, &mut feed);
        let settled: Vec<(u8, i64)> = feed
            .iter()
            .filter(|event| event.kind == EventKind::Settled)
            .map(|event| {
                (
                    event.seat.expect("a settled line names its seat").raw(),
                    event.value,
                )
            })
            .collect();
        assert_eq!(
            settled.len(),
            usize::try_from(SEATS).expect("a few seats"),
            "every living seat is settled once in round {round}"
        );
        // The ladder as the Ledger read it, before anybody was paid: each
        // living seat's held value less the credit it was then paid.
        let world = runner.world();
        let seats = world.seats().seats();
        let ladder: Vec<Option<Money>> = seats
            .iter()
            .enumerate()
            .map(|(index, raw)| {
                world.seats().is_alive(index).then(|| {
                    let paid = settled
                        .iter()
                        .find(|(seat, _)| seat == raw)
                        .map(|(_, credit)| *credit)
                        .expect("a living seat was settled");
                    Money::new(world.held_value(SeatId::new(*raw)).raw() - paid)
                })
            })
            .collect();
        for (seat, credit) in &settled {
            let index = seats
                .iter()
                .position(|raw| raw == seat)
                .expect("a settled seat is seated");
            let place = ladder_place(&ladder, index).expect("a settled seat is on the ladder");
            assert_eq!(
                bmi_for(&rules, place.rank, place.living)
                    .unwrap_or_else(|error| panic!("a place on the ladder is paid: {error}"))
                    .raw(),
                *credit,
                "round {round}, seat {seat}: the ladder read places it at {place:?}"
            );
            let percent =
                band_percent(&rules, place.rank, place.living).expect("a place on the ladder");
            let adjustment = (base * i64::from(percent))
                .checked_div(100)
                .expect("a hundred divides");
            assert_eq!(
                base + adjustment,
                *credit,
                "round {round}, seat {seat}: the band read says {percent} %"
            );
            credits.push((round, *seat, *credit));
        }
        if !runner.end_recap() {
            break;
        }
    }
    // The ladder had a middle and moved: the three credits differ in a round.
    assert!(
        credits
            .iter()
            .any(|(_, _, credit)| credits.iter().any(|(_, _, other)| other != credit)),
        "the fixture has to rank the seats apart for the reads to be under test: {credits:?}"
    );
    let mut golden = golden_credits();
    // item 62: (round, seat) is unique per line.
    golden.sort_unstable();
    // item 62: (round, seat) is unique per line.
    credits.sort_unstable();
    assert_eq!(credits, golden, "the credits are the settlement golden's");
}
