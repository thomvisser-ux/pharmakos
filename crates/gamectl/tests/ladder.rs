// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Easy on the rules table's own segment ladder, on the five committed
//! matches (S3's `opr3`, fix pass 2).
//!
//! `crates/gamectl/tests/operator.rs` holds the corpus, which plays 80 s
//! segments; these matches play the ladder a match is hosted with, every seat
//! Easy, through the same `Host::open_from`, `Surface` and
//! `serve::InProcessSeats` the host loop uses. For each seat and round they
//! record the seal and the forecast's headroom before it.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::path::{Path, PathBuf};

use pharmakos_gamectl::host::{EasyOperators, LIBRARY_PATH};
use pharmakos_gateway::fog::FogPolicy;
use pharmakos_gateway::host::{Host, Settings, SphereVision};
use pharmakos_gateway::limit::IN_PROCESS_LIMITS;
use pharmakos_gateway::rpc::Request;
use pharmakos_gateway::scopes::{Scope, ScopeSet};
use pharmakos_gateway::serve::InProcessSeats;
use pharmakos_gateway::surface::{InProcess, Surface};
use pharmakos_gateway::token::{Subject, Token};
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
    let tick = surface.time().tick;
    let held = surface.match_id().to_owned();
    let (token, handle) = surface
        .tokens()
        .mint(
            Subject::Seat(seat),
            &held,
            ScopeSet::of(&[Scope::Observe, Scope::Plan, Scope::Docs]),
            tick,
        )
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
    /// The sealed playbook, comments and all.
    text: String,
    /// The sealed route's steps.
    steps: Vec<Json>,
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
    let mut factory = EasyOperators::new(rules).expect("the operator reads the rules text");
    let mut easy =
        InProcessSeats::open(&mut surface, &ids, None, &mut factory).expect("in-process seats");
    let mut out: Vec<SeatRound> = Vec::new();
    for round in 1..=rounds {
        let mut headrooms: Vec<i64> = Vec::new();
        for seat in &ids {
            let token = in_process_token(&mut surface, *seat);
            let forecast = call(&mut surface, &token, "get_economy_forecast");
            headrooms.push(number(forecast.get("headroom_kw_now")));
        }
        easy.plan(&mut surface);
        for (seat, headroom_kw) in ids.iter().zip(headrooms) {
            let state = surface
                .seat_state(Subject::Seat(*seat), *seat)
                .expect("its own")
                .clone();
            let sealed = state
                .sealed
                .as_ref()
                .filter(|held| held.round == round && !held.filed_by_the_gateway);
            let text = sealed.map(|held| held.playbook_jsonc.clone());
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
                text: text.unwrap_or_default(),
                steps,
            });
        }
        assert!(surface.begin_push().expect("the Push opens"));
        while let Some(report) = surface.step().expect("a tick") {
            if report.segment_ended {
                break;
            }
        }
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
