// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Easy on the rules table's own segment ladder, on the five committed
//! matches (S3's `opr3`, fix pass 2).
//!
//! `crates/gamectl/tests/operator.rs` holds the corpus, which plays 80 s
//! segments; these matches play the ladder a match is hosted with, every seat
//! Easy, through the same `Host::open_from`, `Surface` and
//! `serve::InProcessSeats` the host loop uses. For each seat and round they
//! record the seal, the forecast's headroom before it, and what became of each
//! route step in the Push that followed, read from the seat's own feed lines
//! ("Step N started.", "Step N completed.", "A step failed (reason): ...").
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use pharmakos_gamectl::host::{EasyOperators, LIBRARY_PATH};
use pharmakos_gateway::fog::{Audience, FogPolicy};
use pharmakos_gateway::host::{Host, Settings, SphereVision};
use pharmakos_gateway::limit::IN_PROCESS_LIMITS;
use pharmakos_gateway::rpc::Request;
use pharmakos_gateway::scopes::{Scope, ScopeSet};
use pharmakos_gateway::surface::{InProcess, Surface};
use pharmakos_gateway::token::{Subject, Token};
use pharmakos_operator::{Easy, Submitted};
use pharmakos_proto::json::Json;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::tables::SeatId;

/// The committed matches, as `(seed, seats)`: Probation's seed, the
/// determinism golden's and the scenario files', at two and three seats.
const MATCHES: [(u64, u32); 5] = [
    (20, 2),
    (pharmakos_sim::DETERMINISM_MATCH_SEED, 2),
    (pharmakos_sim::DETERMINISM_MATCH_SEED, 3),
    (0x0000_0000_ca5c_aded, 2),
    (0x0000_0000_ca5c_aded, 3),
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/gamectl sits two levels below the root")
        .to_path_buf()
}

fn rules_json() -> String {
    std::fs::read_to_string(root().join("rules").join("rules.v1.json")).expect("the rules text")
}

fn seat_ids(seats: u32) -> Vec<SeatId> {
    (0..seats)
        .filter_map(|raw| u8::try_from(raw).ok())
        .map(SeatId::new)
        .collect()
}

/// A seat's own token, registered as an in-process one.
fn in_process_token(surface: &mut Surface, seat: SeatId) -> Token {
    seat_token(surface, seat, &[Scope::Observe, Scope::Plan, Scope::Docs])
}

/// A seat's own token with `scopes`, registered as an in-process one, as the
/// host registers a built-in seat's.
fn seat_token(surface: &mut Surface, seat: SeatId, scopes: &[Scope]) -> Token {
    let tick = surface.time().tick;
    let held = surface.match_id().to_owned();
    let (token, handle) = surface
        .tokens()
        .mint(Subject::Seat(seat), &held, ScopeSet::of(scopes), tick)
        .expect("minted");
    surface.register_in_process(
        handle,
        InProcess {
            limits: IN_PROCESS_LIMITS,
            scratch_view: true,
        },
    );
    token
}

fn call(surface: &mut Surface, token: &Token, method: &str) -> Json {
    let vision = surface.host().map_or_else(
        |_| SphereVision::default(),
        |host| SphereVision::of(host.world()),
    );
    let answer = surface.call(
        Some(token),
        &Request {
            id: Json::Number(String::from("1")),
            method: method.to_owned(),
            params: Json::Object(Vec::new()),
        },
        &vision,
    );
    answer
        .get("result")
        .cloned()
        .unwrap_or_else(|| panic!("{method} was refused: {answer:?}"))
}

/// The seat's start vent, as [`SeatRound::start_vent`] says.
fn start_vent(surface: &mut Surface, token: &Token, seat: SeatId) -> Option<String> {
    let beacons = call(surface, token, "list_beacons");
    let own = Json::String(format!("seat.{}", seat.raw()));
    let core = match beacons.get("beacons") {
        Some(Json::Array(rows)) => rows
            .iter()
            .find(|row| {
                row.get("owner") == Some(&own) && row.get("core") == Some(&Json::Bool(true))
            })
            .and_then(|row| row.get("at"))
            .map(|at| (number(at.get("x")), number(at.get("y"))))?,
        _ => return None,
    };
    let map = call(surface, token, "get_map_summary");
    let Some(Json::Array(features)) = map.get("features") else {
        return None;
    };
    features
        .iter()
        .filter(|row| row.get("kind") == Some(&Json::String(String::from("vent"))))
        .filter(|row| row.get("live") != Some(&Json::Bool(false)))
        .filter_map(|row| {
            let (x, y) = (number(row.get("x")), number(row.get("y")));
            let d2 = (x - core.0).pow(2) + (y - core.1).pow(2);
            match row.get("feature_id") {
                Some(Json::String(id)) => Some(((d2, y, x), id.clone())),
                _ => None,
            }
        })
        .min()
        .map(|(_, id)| id)
}

fn number(value: Option<&Json>) -> i64 {
    match value {
        Some(Json::Number(lexeme)) => lexeme.parse().expect("a whole number"),
        _ => 0,
    }
}

/// Whose seal a seat holds after Easy planned it, as `operator.rs` reads it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Seal {
    /// Easy's own composed plan: its note opens with Easy's seed line.
    Own,
    /// Easy's own safe playbook.
    Safe,
    /// The gateway's fallback, or nothing this round.
    NotEasy,
}

/// What became of one route step in the Push after the seal.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Outcome {
    /// The route never reached it.
    NotRun,
    /// It started and had not ended when the segment did.
    Running,
    /// It completed.
    Completed,
    /// It failed, for the reason the feed line names.
    Failed(String),
}

/// One seat's round.
#[derive(Clone, Debug)]
struct SeatRound {
    /// `seed/seats`.
    game: String,
    round: u32,
    seat: u8,
    seal: Seal,
    /// The forecast's `headroom_kw_now` before the seal.
    headroom_kw: i64,
    /// The forecast's `treasury_now` before the seal.
    treasury: i64,
    /// The seat's start vent: the live vent nearest its core over the
    /// ground, as `get_map_summary` and `list_beacons` show them before the
    /// seal, ties to the lower anchor `y`, then `x`.
    start_vent: Option<String>,
    /// What Easy said it submitted.
    submitted: Submitted,
    /// How many of the candidates Easy weighed its budget would have funded
    /// on their own (`Played::fundable`).
    fundable: usize,
    /// The sealed playbook, comments and all.
    text: String,
    /// The sealed route's steps.
    steps: Vec<Json>,
    /// What became of each, by index.
    outcomes: Vec<Outcome>,
}

impl SeatRound {
    /// The `toward` feature of each stone step, by route index.
    fn stones(&self) -> Vec<(usize, String)> {
        self.steps
            .iter()
            .enumerate()
            .filter_map(|(index, step)| {
                match step
                    .get("place_beacon")?
                    .get("at")?
                    .get("toward")?
                    .get("feature")?
                    .get("feature_id")?
                {
                    Json::String(feature) => Some((index, feature.clone())),
                    _ => None,
                }
            })
            .collect()
    }

    /// The `covering` feature of each placement by name, by route index.
    fn coverings(&self) -> Vec<(usize, String)> {
        self.steps
            .iter()
            .enumerate()
            .filter_map(|(index, step)| {
                match step
                    .get("place_beacon")?
                    .get("at")?
                    .get("covering")?
                    .get("feature_id")?
                {
                    Json::String(feature) => Some((index, feature.clone())),
                    _ => None,
                }
            })
            .collect()
    }

    fn outcome(&self, index: usize) -> &Outcome {
        self.outcomes.get(index).unwrap_or(&Outcome::NotRun)
    }
}

/// The route steps of a sealed playbook, through plan-core's own JSONC
/// reader.
fn route_steps(text: &str) -> Vec<Json> {
    let body = pharmakos_plan_core::Document::parse(text)
        .expect("a sealed playbook reads")
        .to_json();
    match body.get("declarative").and_then(|d| d.get("route")) {
        Some(Json::Array(steps)) => steps.clone(),
        _ => Vec::new(),
    }
}

/// `Step N started.` and its kin: the step's index from 0.
fn step_index(text: &str, tail: &str) -> Option<usize> {
    text.strip_prefix("Step ")?
        .strip_suffix(tail)?
        .parse::<usize>()
        .ok()?
        .checked_sub(1)
}

/// `A step failed (timeout): ...`: the reason's name.
fn failure_of(text: &str) -> Option<String> {
    let rest = text.strip_prefix("A step failed (")?;
    Some(rest.split_once(')')?.0.to_owned())
}

/// Per seat, the route step running and whether a rule's body is: a rule's
/// steps are numbered in its own body, so its lines are not the route's.
#[derive(Clone, Copy, Default)]
struct Running {
    step: Option<usize>,
    in_rule: bool,
}

/// Read one feed line of a seat's own into its round's outcomes.
fn read_line(row: &mut SeatRound, running: &mut Running, kind: &str, text: &str) {
    match kind {
        "rule_fired" => running.in_rule = true,
        "rule_ended" => running.in_rule = false,
        _ if running.in_rule => {}
        "step_started" => {
            if let Some(index) = step_index(text, " started.") {
                running.step = Some(index);
                if let Some(outcome) = row.outcomes.get_mut(index) {
                    *outcome = Outcome::Running;
                }
            }
        }
        "step_completed" => {
            if let Some(index) = step_index(text, " completed.")
                && let Some(outcome) = row.outcomes.get_mut(index)
            {
                *outcome = Outcome::Completed;
            }
        }
        "step_failed" => {
            if let (Some(index), Some(reason)) = (running.step, failure_of(text))
                && let Some(outcome) = row.outcomes.get_mut(index)
            {
                *outcome = Outcome::Failed(reason);
            }
        }
        _ => {}
    }
}

/// Play the Push after the seals, reading each seat's own feed lines into
/// its round's outcomes (`out[first..]`, one row per seat in seat order).
fn play_push(surface: &mut Surface, out: &mut [SeatRound], first: usize, seats: usize) {
    assert!(surface.begin_push().expect("the Push opens"));
    let mut running: Vec<Running> = vec![Running::default(); seats];
    let mut read = 0_usize;
    loop {
        let report = surface.step().expect("a tick");
        let events = surface.feed().events();
        if events.len() < read {
            read = 0;
        }
        for event in events.get(read..).unwrap_or_default() {
            let Audience::Private(seat) = event.audience else {
                continue;
            };
            let at = usize::from(seat.raw());
            if let (Some(row), Some(state)) = (
                first.checked_add(at).and_then(|index| out.get_mut(index)),
                running.get_mut(at),
            ) {
                read_line(row, state, &event.kind.to_string(), &event.text);
            }
        }
        read = events.len();
        match report {
            Some(report) if report.segment_ended => break,
            Some(_) => {}
            None => break,
        }
    }
}

/// Easy plays one seat's round through the seat's own in-process token, as
/// the host's built-in seat does (`gamectl host`'s `EasySeat`).
fn play_seat(
    surface: &mut Surface,
    seat: SeatId,
    operator: &mut Easy,
) -> pharmakos_operator::Played {
    let token = seat_token(
        surface,
        seat,
        &[Scope::Observe, Scope::Plan, Scope::PlanSubmit, Scope::Docs],
    );
    let mut bridged = |method: &str, params: Json| {
        let vision = surface.host().map_or_else(
            |_| SphereVision::default(),
            |host| SphereVision::of(host.world()),
        );
        surface.call(
            Some(&token),
            &Request {
                id: Json::Number(String::from("1")),
                method: method.to_owned(),
                params,
            },
            &vision,
        )
    };
    operator.play(seat.raw(), &mut bridged)
}

/// One match on the rules table's ladder, every seat Easy, `rounds` rounds.
fn ladder_match(rules: &str, seed: u64, seats: u32, rounds: u32) -> Vec<SeatRound> {
    let game = format!("{seed:x}/{seats}");
    let host = Host::open_from(
        rules,
        seed,
        seats,
        &Settings {
            segment_lengths_ms: Vec::new(),
            round_limit: rounds,
        },
        Some(root().join(LIBRARY_PATH)),
    )
    .expect("a match");
    let table = RulesTable::from_canonical_json(rules).expect("a rules table");
    let ids = seat_ids(seats);
    let mut surface = Surface::new(
        &format!("ladder-{seed:x}-{seats}-{}", std::process::id()),
        seed,
        table,
        FogPolicy::fogged(),
        &ids,
    )
    .expect("a surface");
    surface.attach(host).expect("attached");
    surface.open_lull().expect("the opening Lull");
    // Easy plays each seat through the seat's own in-process token, as the
    // host's built-in seat does (`gamectl host`'s `EasySeat`), so each round's
    // `Played` is read here.
    EasyOperators::new(rules).expect("the operator reads the rules text");
    let mut easy: Vec<Easy> = ids
        .iter()
        .map(|_| Easy::new(rules).expect("the rules text"))
        .collect();
    let mut out: Vec<SeatRound> = Vec::new();
    for round in 1..=rounds {
        let mut before: Vec<(i64, i64, Option<String>)> = Vec::new();
        for seat in &ids {
            let token = in_process_token(&mut surface, *seat);
            let forecast = call(&mut surface, &token, "get_economy_forecast");
            let vent = start_vent(&mut surface, &token, *seat);
            before.push((
                number(forecast.get("headroom_kw_now")),
                number(forecast.get("treasury_now")),
                vent,
            ));
        }
        let mut played: Vec<(Submitted, usize)> = Vec::new();
        for (seat, operator) in ids.iter().zip(&mut easy) {
            let round_played = play_seat(&mut surface, *seat, operator);
            played.push((round_played.submitted, round_played.fundable));
        }
        let first = out.len();
        for ((seat, (headroom_kw, treasury, start_vent)), (submitted, fundable)) in
            ids.iter().zip(before).zip(played)
        {
            let state = surface
                .seat_state(Subject::Seat(*seat), *seat)
                .expect("its own")
                .clone();
            let sealed = state
                .sealed
                .as_ref()
                .filter(|held| held.round() == round && !held.filed_by_the_gateway());
            let text = sealed.map(|held| held.playbook_jsonc().to_owned());
            let seal = match &text {
                None => Seal::NotEasy,
                Some(text) if text.contains("Easy, seat") => Seal::Own,
                Some(_) => Seal::Safe,
            };
            let steps = text.as_deref().map(route_steps).unwrap_or_default();
            out.push(SeatRound {
                game: game.clone(),
                round,
                seat: seat.raw(),
                seal,
                headroom_kw,
                treasury,
                start_vent,
                submitted,
                fundable,
                text: text.unwrap_or_default(),
                outcomes: vec![Outcome::NotRun; steps.len()],
                steps,
            });
        }
        play_push(&mut surface, &mut out, first, ids.len());
        surface.end_recap().expect("the recap closes");
        if round < rounds {
            surface.open_lull().expect("the next Lull");
        }
    }
    out
}

/// The scouts a seal orders: every `scout_count` and `picket_scouts` its
/// route's placements set.
fn scouts_ordered(seal: &SeatRound) -> i64 {
    let mut scouts = 0_i64;
    for step in &seal.steps {
        let Some(mandate) = step
            .get("place_beacon")
            .and_then(|place| place.get("initial"))
            .and_then(|initial| initial.get("mandate"))
        else {
            continue;
        };
        for (kind, key) in [("survey", "scout_count"), ("defend", "picket_scouts")] {
            scouts = scouts
                .checked_add(number(mandate.get(kind).and_then(|held| held.get(key))))
                .expect("a count");
        }
    }
    scouts
}

/// `power.kw_per_unit` of a rules text.
fn kw_per_unit(rules: &str) -> i64 {
    let table = pharmakos_proto::json::read(rules).expect("the rules text reads");
    number(
        table
            .get("power")
            .and_then(|power| power.get("kw_per_unit")),
    )
}

/// Scouts are charged at the row every grid charges a unit at (fix pass 2,
/// item 4): the sim's `PowerRules::per_unit` and the verifier's projection
/// read `power.kw_per_unit`, and so does Easy. With the row at 3 kW, on the
/// five committed matches, every seat's round-1 seal is Easy's own plan (none
/// falls to its safe playbook on a verifier's E0601), and no seal orders more
/// draw than the headroom the forecast showed.
#[test]
fn scouts_are_charged_at_the_row_every_grid_charges_a_unit_at() {
    let base = rules_json();
    let rules = base.replace("\"kw_per_unit\": 1,", "\"kw_per_unit\": 3,");
    assert_ne!(rules, base, "the rules text carries `power.kw_per_unit`");
    let per_unit = kw_per_unit(&rules);
    assert_eq!(per_unit, 3);
    let mut seals: Vec<SeatRound> = Vec::new();
    for (seed, seats) in MATCHES {
        seals.extend(ladder_match(&rules, seed, seats, 1));
    }
    for seal in &seals {
        let drawn = scouts_ordered(seal).checked_mul(per_unit).expect("a draw");
        eprintln!(
            "{} round {} seat {}: {:?}, {drawn} kW of scouts against {} kW",
            seal.game, seal.round, seal.seat, seal.seal, seal.headroom_kw
        );
        assert_eq!(seal.seal, Seal::Own, "{}: {}", seal.game, seal.text);
        assert!(
            drawn <= seal.headroom_kw,
            "{} seat {}: {drawn} kW against {} kW",
            seal.game,
            seal.seat,
            seal.headroom_kw
        );
    }
    assert_eq!(seals.len(), 12, "twelve seats over the five matches");
}

/// The five committed matches over five rounds on the committed rules, played
/// once for every test that reads them.
fn five_rounds() -> &'static [SeatRound] {
    static ROUNDS: OnceLock<Vec<SeatRound>> = OnceLock::new();
    ROUNDS.get_or_init(|| {
        let rules = rules_json();
        MATCHES
            .iter()
            .flat_map(|&(seed, seats)| ladder_match(&rules, seed, seats, 5))
            .collect()
    })
}

/// Easy's allowance for the wait before a walk starts is the travel
/// estimate's own: the sim's re-ask bound for a caller that holds a rules
/// table and no world (`Speed::commander`'s `link_ticks`) in game
/// milliseconds, so the operator's restatement of the router's rows and the
/// tick's length cannot drift from the sim's.
#[test]
fn the_walk_allowance_is_the_estimates_own() {
    let rules = rules_json();
    let table = RulesTable::from_canonical_json(&rules).expect("a rules table");
    let link = pharmakos_sim::pathing::Speed::commander(&table).link_ticks;
    let expected = i64::from(link) * i64::from(pharmakos_sim::math::quantity::MS_PER_TICK);
    let easy = pharmakos_operator::Easy::new(&rules).expect("the rules text");
    assert_eq!(easy.walk_allowance_ms(), expected);
}

/// A stone placed in round 2 (fix pass 2, item 1): on the determinism seed,
/// at least one seat's round-2 seal places a stone and the stone's step
/// completes. Before the fix every round-2 stone copied Hold & Build's 60 s
/// timeout onto a priced walk of 79 to 80 s and failed `timeout`.
#[test]
fn a_round_two_stone_places_on_the_determinism_seed() {
    let game = format!("{:x}/2", pharmakos_sim::DETERMINISM_MATCH_SEED);
    let placed: Vec<&SeatRound> = five_rounds()
        .iter()
        .filter(|row| row.game == game && row.round == 2)
        .filter(|row| {
            row.stones()
                .iter()
                .any(|(index, _)| *row.outcome(*index) == Outcome::Completed)
        })
        .collect();
    for row in five_rounds()
        .iter()
        .filter(|row| row.game == game && row.round <= 2)
    {
        eprintln!(
            "round {} seat {}: stones {:?}, outcomes {:?}",
            row.round,
            row.seat,
            row.stones(),
            row.outcomes
        );
    }
    assert!(!placed.is_empty(), "no round-2 stone placed on {game}");
}

/// No step Easy composes fails `timeout`, on the five committed matches over
/// five rounds (fix pass 2, item 1): every placement's timeout is derived from
/// its own price and the estimate's allowance, so a walk the estimate prices
/// never outlasts it.
#[test]
fn no_composed_step_times_out_on_the_committed_matches() {
    let timed_out: Vec<String> = five_rounds()
        .iter()
        .filter(|row| row.seal == Seal::Own)
        .flat_map(|row| {
            row.outcomes
                .iter()
                .enumerate()
                .filter(|(_, outcome)| **outcome == Outcome::Failed(String::from("timeout")))
                .map(move |(index, _)| {
                    format!(
                        "{} round {} seat {} step {}: {:?}",
                        row.game,
                        row.round,
                        row.seat,
                        index + 1,
                        row.steps.get(index).and_then(|step| step.get("label"))
                    )
                })
        })
        .collect();
    assert!(timed_out.is_empty(), "{timed_out:#?}");
}

/// Every stone either places or is followed by a different choice (fix pass
/// 2, item 1): a seat whose stone toward a feature did not place never seals a
/// stone toward the same feature in the next round. And the seat-chains -- a
/// seat's stones toward one feature, over the five rounds -- that reach the
/// goal the stone served (a later placement covering that feature, which
/// completes), counted and printed for the pull request.
#[test]
fn every_stone_places_or_is_followed_by_a_different_choice() {
    let rows = five_rounds();
    let mut repeated: Vec<String> = Vec::new();
    // (game, seat, feature) -> whether a covering placement of it completed
    // after the chain began.
    let mut chains: Vec<((String, u8, String), bool)> = Vec::new();
    for row in rows.iter().filter(|row| row.seal == Seal::Own) {
        for (index, feature) in row.stones() {
            let key = (row.game.clone(), row.seat, feature.clone());
            if !chains.iter().any(|(held, _)| *held == key) {
                chains.push((key, false));
            }
            if *row.outcome(index) == Outcome::Completed {
                continue;
            }
            let next = rows.iter().find(|later| {
                later.game == row.game && later.seat == row.seat && later.round == row.round + 1
            });
            if next.is_some_and(|next| next.stones().iter().any(|(_, held)| *held == feature)) {
                repeated.push(format!(
                    "{} seat {} rounds {} and {}: {feature} ({:?})",
                    row.game,
                    row.seat,
                    row.round,
                    row.round + 1,
                    row.outcome(index)
                ));
            }
        }
        for (index, feature) in row.coverings() {
            if *row.outcome(index) != Outcome::Completed {
                continue;
            }
            for ((game, seat, served), reached) in &mut chains {
                if *game == row.game && *seat == row.seat && *served == feature {
                    *reached = true;
                }
            }
        }
    }
    let reached = chains.iter().filter(|(_, reached)| *reached).count();
    eprintln!(
        "seat-chains {}, reaching the goal they served {reached}: {chains:#?}",
        chains.len()
    );
    assert!(repeated.is_empty(), "{repeated:#?}");
}

/// What a seal places, by kind, for the seals-by-kind table: a placement's
/// kind is read off its label (`place_power`, `place_mine`, `place_guard`,
/// `place_raid`, `survey_post`, `stone`, `add_generator`, each numbered after
/// the first), and the Spire's step and a safe or fallback seal are kinds of
/// their own.
fn kinds_of(row: &SeatRound) -> Vec<&'static str> {
    match row.seal {
        Seal::Safe => return vec!["safe"],
        Seal::NotEasy => return vec!["fallback"],
        Seal::Own => {}
    }
    let mut kinds: Vec<&'static str> = Vec::new();
    for step in &row.steps {
        let label = match step.get("label") {
            Some(Json::String(label)) => label.as_str(),
            _ => "",
        };
        let kind = [
            ("place_power", "generator"),
            ("place_mine", "mine"),
            ("place_guard", "guard"),
            ("place_raid", "raid"),
            ("survey_post", "survey"),
            ("stone", "stone"),
            ("add_generator", "tap"),
            ("queue_spire", "spire"),
        ]
        .iter()
        .find(|(prefix, _)| label.starts_with(prefix))
        .map(|(_, kind)| *kind);
        if let Some(kind) = kind {
            kinds.push(kind);
        }
    }
    if kinds.is_empty() {
        kinds.push("nothing placed");
    }
    kinds
}

/// The seals over the first `rounds` rounds, counted by kind.
fn by_kind(rounds: u32) -> Vec<(&'static str, usize)> {
    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    for row in five_rounds().iter().filter(|row| row.round <= rounds) {
        for kind in kinds_of(row) {
            match counts.iter_mut().find(|(held, _)| *held == kind) {
                Some((_, count)) => *count += 1,
                None => counts.push((kind, 1)),
            }
        }
    }
    counts.sort_unstable();
    counts
}

/// Easy builds what it built before its stones and its Survey post (fix pass
/// 2, item 2): on the five committed matches over the rules ladder, every
/// seat's round-1 seal places a Generator on its start vent, and the Push
/// places it. The seals over three and five rounds, counted by kind, are
/// printed for the pull request.
#[test]
fn round_one_places_a_generator_on_the_start_vent_on_every_seat() {
    let mut missing: Vec<String> = Vec::new();
    for row in five_rounds().iter().filter(|row| row.round == 1) {
        let vent = row.start_vent.clone().unwrap_or_default();
        let placed = row
            .coverings()
            .iter()
            .any(|(index, feature)| *feature == vent && *row.outcome(*index) == Outcome::Completed);
        eprintln!(
            "{} seat {}: start vent {vent}, kinds {:?}, placed {placed}",
            row.game,
            row.seat,
            kinds_of(row)
        );
        if !placed {
            missing.push(format!("{} seat {}: {}", row.game, row.seat, row.text));
        }
    }
    eprintln!("seals by kind over three rounds: {:?}", by_kind(3));
    eprintln!("seals by kind over five rounds: {:?}", by_kind(5));
    assert!(missing.is_empty(), "{missing:#?}");
    assert_eq!(
        five_rounds().iter().filter(|row| row.round == 1).count(),
        12
    );
}

/// The safe seals, by match, seat and round, over the five rounds, with the
/// treasury each held.
fn safe_seals() -> Vec<(String, u8, u32, i64)> {
    five_rounds()
        .iter()
        .filter(|row| row.seal == Seal::Safe)
        .map(|row| (row.game.clone(), row.seat, row.round, row.treasury))
        .collect()
}

/// An idle treasury is never answered with the safe playbook (fix pass 3,
/// item 1): on the five committed matches over five rounds, no seat seals its
/// safe playbook while a candidate of positive worth fits its budget on its
/// own, nor while it holds a beacon's price: a stone's own sphere is worth
/// more than the beacon costs, so a seat that can pay for one always has a
/// goal. Before the fix, one seat in every match sealed its safe playbook in
/// rounds 3 to 5 holding $421 to $914, because charging a stone the whole
/// chain's stones left every far feature's stone worth less than nothing.
/// The safe seals by match, seat and round are printed for the pull request.
#[test]
fn no_seat_seals_its_safe_playbook_while_a_worthwhile_candidate_fits() {
    let beacon = {
        let table = pharmakos_proto::json::read(&rules_json()).expect("the rules text reads");
        number(
            table
                .get("structures")
                .and_then(|rows| rows.get("beacon"))
                .and_then(|row| row.get("cost_dollars")),
        )
    };
    let idle: Vec<String> = five_rounds()
        .iter()
        .filter(|row| {
            row.submitted == Submitted::Safe && (row.fundable > 0 || row.treasury >= beacon)
        })
        .map(|row| {
            format!(
                "{} round {} seat {}: {} candidates fit alone, ${} held",
                row.game, row.round, row.seat, row.fundable, row.treasury
            )
        })
        .collect();
    eprintln!("safe seals (match, seat, round): {:?}", safe_seals());
    assert!(idle.is_empty(), "{idle:#?}");
}
