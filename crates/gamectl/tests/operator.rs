// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The built-in operator in a **hosted** match (T18; decisions-log items 111
//! and 112).
//!
//! `pharmakos-operator` depends on `pharmakos-proto` alone and its own tests
//! drive it through scripted closures. A test that hosts a real match needs
//! the sim types the gateway's API is written in, so it lives here, in the
//! crate that adapts the operator to the gateway's seams
//! (`crate::host::EasyOperators`): the same `Host::open_from`, the same
//! `Surface` and the same `serve::InProcessSeats` the host loop uses, with the
//! operator reaching the match only through its seat's in-process token.
//!
//! # The corpus
//!
//! Item 111's acceptance names it: the determinism golden's seed with 1, 2
//! and 3 seats, each at its first Lull, plus one **power-short** variant built
//! through a rules-text change (`power.core_surplus_kw` 10 -> 1, so the grid
//! is short from the first tick); and every round's snapshot of a hosted
//! three-round match -- here two of them, one on the committed rules and one
//! on the power-short text, whose core is shed at the Push's first settle. On
//! every snapshot, for every seat, the corpus records what Easy sealed, how
//! many calls it made, whether its safe playbook qualified, and what that safe
//! playbook raised unforced (T18b; decisions-log items 113 (4) and 114 (4)).
//!
//! # The scenario's Generator
//!
//! `scenarios/skeleton/against-easy.scenario.jsonc` asserts that seat 0's
//! Generator is completed, and the scenario format cannot state a kW figure.
//! The figure is asserted here, from `get_economy_forecast`, in one short
//! match that replays the scenario's own setup
//! ([`the_scenario_generator_raises_supply_by_exactly_the_lean_vent`]).
//!
//! # Match ids
//!
//! Every match here is opened with an id unique to the run ([`match_id`]):
//! on `main` a `Surface` only validates the id, but after T17 a match id can
//! carry a save, and item 112 (8) asks every hosting test for its own.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use pharmakos_gamectl::host::{EasyOperators, LIBRARY_PATH};
use pharmakos_gateway::audit::Entry;
use pharmakos_gateway::fog::FogPolicy;
use pharmakos_gateway::host::{Host, Settings, SphereVision};
use pharmakos_gateway::limit::IN_PROCESS_LIMITS;
use pharmakos_gateway::rpc::Request;
use pharmakos_gateway::scopes::{Scope, ScopeSet};
use pharmakos_gateway::serve::InProcessSeats;
use pharmakos_gateway::surface::{InProcess, Surface};
use pharmakos_gateway::token::{Subject, Token};
use pharmakos_operator::Easy;
use pharmakos_operator::easy::{
    EASY_ADVISOR_CALL_BUDGET, EASY_CALL_BUDGET, SAFE_MAX_RAISED, SAFE_REACH_MS,
};
use pharmakos_proto::json::Json;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::tables::SeatId;

/// The determinism golden's seed.
const SEED: u64 = pharmakos_sim::DETERMINISM_MATCH_SEED;

/// The scenario files' seed, the second golden seed.
const SCENARIO_SEED: u64 = 0x0000_0000_ca5c_aded;

/// The corpus's Push length: long enough for a commander to walk to a seam,
/// place a beacon and walk back (spec section 5's 12 s deploy), short enough
/// to step in a test.
const SEGMENT_MS: i32 = 60_000;

// ---------------------------------------------------------------------------
// Hosting
// ---------------------------------------------------------------------------

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/gamectl sits two levels below the root")
        .to_path_buf()
}

/// A match id unique to this run: the label, and the test process's id.
fn match_id(label: &str) -> String {
    format!("t18-{label}-{}", std::process::id())
}

/// The committed rules text, which `gamectl host` hands the host and the
/// operator alike.
fn rules_json() -> String {
    std::fs::read_to_string(root().join("rules").join("rules.v1.json")).expect("the rules text")
}

/// The same text with the core's surplus cut from 10 kW to 1, so a seat's
/// draw outruns its supply from the start: the power-short variant.
fn power_short_json() -> String {
    let text = rules_json();
    let short = text.replace("\"core_surplus_kw\": 10,", "\"core_surplus_kw\": 1,");
    assert_ne!(
        short, text,
        "the rules text still carries the row this variant edits"
    );
    short
}

/// A match in its opening Lull, hosted exactly as `serve::run` hosts one.
fn open(match_id: &str, seed: u64, seats: u32, rules: &str, rounds: u32) -> Surface {
    let host = Host::open_from(
        rules,
        seed,
        seats,
        &Settings {
            segment_lengths_ms: vec![SEGMENT_MS],
            round_limit: rounds,
            units_per_seat: 0,
        },
        Some(root().join(LIBRARY_PATH)),
    )
    .expect("a match");
    let table = RulesTable::from_canonical_json(rules).expect("a rules table");
    let ids: Vec<SeatId> = (0..seats)
        .filter_map(|raw| u8::try_from(raw).ok())
        .map(SeatId::new)
        .collect();
    let mut surface =
        Surface::new(match_id, seed, table, FogPolicy::fogged(), &ids).expect("a surface");
    surface.attach(host).expect("attached");
    surface.open_lull().expect("the opening Lull");
    surface
}

fn seat_ids(seats: u32) -> Vec<SeatId> {
    (0..seats)
        .filter_map(|raw| u8::try_from(raw).ok())
        .map(SeatId::new)
        .collect()
}

/// A seat's own token, registered as an in-process one: what the host mints
/// for a built-in seat.
fn in_process_token(surface: &mut Surface, seat: u8) -> Token {
    let tick = surface.time().tick;
    let held = surface.match_id().to_owned();
    let (token, handle) = surface
        .tokens()
        .mint(
            Subject::Seat(SeatId::new(seat)),
            &held,
            ScopeSet::of(&[Scope::Observe, Scope::Plan, Scope::PlanSubmit, Scope::Docs]),
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

/// One call through a token, with the production vision.
fn call(surface: &mut Surface, token: &Token, method: &str, params: Json) -> Json {
    let vision = surface.host().map_or_else(
        |_| SphereVision::default(),
        |host| SphereVision::of(host.world()),
    );
    surface.call(
        Some(token),
        &Request {
            id: Json::Number(String::from("1")),
            method: method.to_owned(),
            params,
        },
        &vision,
    )
}

fn result(response: &Json, what: &str) -> Json {
    response
        .get("result")
        .cloned()
        .unwrap_or_else(|| panic!("{what} was refused: {response:?}"))
}

fn params(text: &str) -> Json {
    pharmakos_proto::json::read(text).expect("params")
}

fn number(value: &Json, key: &str) -> i64 {
    match value.get(key) {
        Some(Json::Number(lexeme)) => lexeme.parse().expect("a whole number"),
        _ => 0,
    }
}

/// Every seat of a match played by Easy through the host's own seam, as
/// `serve::run` plays them: one fresh operator per seat, from the factory
/// `gamectl host` hands the host.
fn all_built_in(surface: &mut Surface, seats: u32, rules: &str) -> InProcessSeats {
    let mut factory = EasyOperators::new(rules).expect("the operator reads the rules text");
    InProcessSeats::open(surface, &seat_ids(seats), None, &mut factory).expect("in-process seats")
}

/// The methods each seat's in-process token called, in order, from the
/// audit log.
fn calls_by_seat(entries: &[Entry]) -> Vec<(u8, Vec<String>)> {
    let mut out: Vec<(u8, Vec<String>)> = Vec::new();
    for entry in entries {
        let Some(Subject::Seat(seat)) = entry.subject else {
            continue;
        };
        let Some(method) = entry.action.strip_prefix("call ") else {
            continue;
        };
        match out.iter_mut().find(|(held, _)| *held == seat.raw()) {
            Some(slot) => slot.1.push(method.to_owned()),
            None => out.push((seat.raw(), vec![method.to_owned()])),
        }
    }
    out.sort_unstable();
    out
}

// ---------------------------------------------------------------------------
// The corpus
// ---------------------------------------------------------------------------

/// Whose seal a seat holds after Easy planned it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Seal {
    /// Easy's own composed plan: this round's, filed by the operator, and
    /// its note opens with Easy's seed line.
    Own,
    /// Easy's own safe playbook: this round's, filed by the operator.
    Safe,
    /// Anything else -- nothing this round, or the gateway's fallback: the
    /// verifier did not accept what Easy submitted.
    NotEasy,
}

/// What Easy did for one seat on one snapshot.
#[derive(Clone, Debug)]
struct Row {
    snapshot: String,
    seat: u8,
    /// Whose seal the seat holds this round.
    seal: Seal,
    /// The sealed playbook's note, which says what Easy chose.
    note: String,
    /// Easy composed a plan this round: it called `patch_plan` at least once
    /// (the composition), which only its own plan's path does.
    composed: bool,
    /// The `patch_plan` and `verify_plan` calls the built-in seat made.
    patches: u32,
    verifies: u32,
    /// Calls the built-in seat made this round.
    calls: u32,
    /// Easy's safe playbook for this seat, as an advisor makes it, verified
    /// FULL through the seat's own door.
    safe_qualifies: bool,
    /// The seat's `headroom_kw_now` on this snapshot.
    headroom_kw: i64,
    /// Calls the advisor made.
    advisor_calls: u32,
    /// The beacons the safe playbook raised, nearest first, unforced.
    raised: Vec<String>,
    /// The seat's own beacons at risk on this snapshot, as `list_beacons`
    /// answered them, unrewritten: browned out, not the core, not HIGH.
    at_risk: Vec<String>,
    /// The seat's core is browned out on this snapshot.
    core_dark: bool,
    /// The beacons the advisor estimated a route to (the safe playbook's
    /// estimates are its only ones that end on a beacon), with whether its
    /// own estimate put each within `SAFE_REACH_MS`.
    estimated: Vec<(String, bool)>,
    /// The advisor's "why" for the Safe Playbook page.
    why_safe: String,
    /// The seat's `supply_kw_now` on this snapshot.
    supply_kw: i64,
    /// The raise path forced through the real verifier (see
    /// [`forced_raise`]): `None` when the seat has no own non-core beacon on
    /// this snapshot, else what was raised and whether it qualified FULL.
    forced: Option<Forced>,
}

/// What an advisor raised when told power is short and every own non-core
/// beacon is dark, on a real `Surface`.
#[derive(Clone, Debug)]
struct Forced {
    /// The own non-core beacons the seat has.
    dark: Vec<String>,
    /// What the safe playbook raised.
    raised: Vec<String>,
    /// The safe playbook it returned, verified FULL through the seat's door.
    qualifies: bool,
    /// It carries a `raise_b_NN` step: the raised form, not the fallback.
    carries_raise: bool,
}

/// The beacons an advice's safe playbook raises: the `raise_b_NN` steps of
/// the route it suggests for the Safe Playbook template.
fn raised_by(advice: &pharmakos_operator::Advice) -> Vec<String> {
    advice
        .suggestions
        .iter()
        .filter(|suggestion| suggestion.template_id == "safe_playbook")
        .flat_map(|suggestion| suggestion.parameters.iter())
        .filter_map(|value| pharmakos_proto::json::read(&value.value).ok())
        .flat_map(|route| match route {
            Json::Array(steps) => steps,
            _ => Vec::new(),
        })
        .filter_map(|step| match step.get("label") {
            Some(Json::String(label)) => label.strip_prefix("raise_").map(str::to_owned),
            _ => None,
        })
        .collect()
}

/// Every own non-core beacon the seat is shown, ascending by id.
fn own_non_core(surface: &mut Surface, token: &Token, seat: u8) -> Vec<String> {
    let beacons = result(
        &call(surface, token, "list_beacons", params("{}")),
        "list_beacons",
    );
    let own = format!("seat.{seat}");
    let mut out: Vec<String> = match beacons.get("beacons") {
        Some(Json::Array(rows)) => rows
            .iter()
            .filter(|row| row.get("owner") == Some(&Json::String(own.clone())))
            .filter(|row| row.get("core") != Some(&Json::Bool(true)))
            .filter_map(|row| match row.get("beacon_id") {
                Some(Json::String(id)) => Some(id.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    out.sort();
    out
}

/// The seat's own beacons at risk -- browned out, not the core, and not HIGH
/// -- and whether its core is browned out, as `list_beacons` answers them.
fn own_power(surface: &mut Surface, token: &Token, seat: u8) -> (Vec<String>, bool) {
    let beacons = result(
        &call(surface, token, "list_beacons", params("{}")),
        "list_beacons",
    );
    let own = Json::String(format!("seat.{seat}"));
    let rows = match beacons.get("beacons") {
        Some(Json::Array(rows)) => rows.clone(),
        _ => Vec::new(),
    };
    let dark = |row: &Json| row.get("powered") == Some(&Json::Bool(false));
    let core = |row: &Json| row.get("core") == Some(&Json::Bool(true));
    let mut at_risk: Vec<String> = rows
        .iter()
        .filter(|row| row.get("owner") == Some(&own) && dark(row) && !core(row))
        .filter(|row| row.get("priority") != Some(&Json::String(String::from("high"))))
        .filter_map(|row| match row.get("beacon_id") {
            Some(Json::String(id)) => Some(id.clone()),
            _ => None,
        })
        .collect();
    at_risk.sort();
    let core_dark = rows
        .iter()
        .any(|row| row.get("owner") == Some(&own) && core(row) && dark(row));
    (at_risk, core_dark)
}

/// An Easy advisor for the seat, through its own token with no answer
/// rewritten, and the beacons it estimated a route to, each with whether its
/// own estimate put it within `SAFE_REACH_MS`.
fn advise_unforced(
    surface: &mut Surface,
    token: &Token,
    seat: u8,
    rules: &str,
) -> (pharmakos_operator::Advice, Vec<(String, bool)>) {
    let mut advisor = Easy::new(rules).expect("the rules text");
    let mut estimated: Vec<(String, bool)> = Vec::new();
    let advice = {
        let mut bridged = |method: &str, params: Json| {
            let target = beacon_target(method, &params);
            let answer = call(surface, token, method, params);
            if let (Some(beacon), Some(found)) = (target, answer.get("result")) {
                let within = found.get("reachable") == Some(&Json::Bool(true))
                    && number(found, "ms") <= SAFE_REACH_MS;
                estimated.push((beacon, within));
            }
            answer
        };
        advisor.advise(seat, &mut bridged)
    };
    (advice, estimated)
}

/// The beacon an `estimate_route` call ends on, if it ends on one.
fn beacon_target(method: &str, params: &Json) -> Option<String> {
    if method != "estimate_route" {
        return None;
    }
    let Some(Json::Array(waypoints)) = params.get("waypoints") else {
        return None;
    };
    match waypoints.last()?.get("beacon_anchor")?.get("beacon_id")? {
        Json::String(id) => Some(id.clone()),
        _ => None,
    }
}

/// Set `key` on an object, replacing it or adding it.
fn set_member(object: &mut Json, key: &str, value: Json) {
    if let Json::Object(members) = object {
        match members.iter_mut().find(|(held, _)| held == key) {
            Some(slot) => slot.1 = value,
            None => members.push((key.to_owned(), value)),
        }
    }
}

/// The raise path through the **real** verifier. An advisor for the seat
/// runs against the real `Surface` through the seat's own token, with every
/// call forwarded untouched except one answer rewritten: its `list_beacons`
/// says every own non-core beacon is browned out. That alone is enough since
/// T18b, because a dark own beacon is itself "power is short" (decisions-log
/// item 113 (4)), so the forecast is no longer rewritten. Everything else --
/// the forecast, the estimates, `instantiate_template` with the whole route,
/// the FULL `verify_plan` -- is the gateway's own answer. `None` when the seat
/// has no own non-core beacon to raise.
fn forced_raise(surface: &mut Surface, token: &Token, seat: u8, rules: &str) -> Option<Forced> {
    let dark = own_non_core(surface, token, seat);
    if dark.is_empty() {
        return None;
    }
    let own = Json::String(format!("seat.{seat}"));
    let mut advisor = Easy::new(rules).expect("the rules text");
    let advice = {
        let mut rewriting = |method: &str, params: Json| {
            let mut answer = call(surface, token, method, params);
            if method != "list_beacons" {
                return answer;
            }
            if let Json::Object(members) = &mut answer {
                if let Some((_, Json::Object(fields))) =
                    members.iter_mut().find(|(key, _)| key == "result")
                {
                    if let Some((_, Json::Array(rows))) =
                        fields.iter_mut().find(|(key, _)| key == "beacons")
                    {
                        for row in rows.iter_mut() {
                            if row.get("owner") == Some(&own)
                                && row.get("core") != Some(&Json::Bool(true))
                            {
                                set_member(row, "powered", Json::Bool(false));
                            }
                        }
                    }
                }
            }
            answer
        };
        advisor.advise(seat, &mut rewriting)
    };
    let verified = call(
        surface,
        token,
        "verify_plan",
        Json::Object(vec![
            (
                String::from("playbook_jsonc"),
                Json::String(advice.safe_playbook_jsonc.clone()),
            ),
            (String::from("depth"), Json::String(String::from("full"))),
        ]),
    );
    let qualifies = result(&verified, "verify_plan")
        .get("report")
        .and_then(|report| report.get("qualifies"))
        == Some(&Json::Bool(true));
    Some(Forced {
        dark,
        raised: raised_by(&advice),
        qualifies,
        carries_raise: advice.safe_playbook_jsonc.contains("\"raise_b_"),
    })
}

/// Plan every seat of the snapshot the surface stands on with Easy, then ask
/// an Easy advisor for each seat's safe playbook and verify it.
fn visit(
    label: &str,
    surface: &mut Surface,
    seats: u32,
    rules: &str,
    easy: &mut InProcessSeats,
) -> Vec<Row> {
    let _ = surface.audit().take();
    easy.plan(surface);
    let calls = calls_by_seat(&surface.audit().take());
    let count = |methods: &[String], name: &str| -> u32 {
        u32::try_from(methods.iter().filter(|method| *method == name).count()).expect("counted")
    };
    let round = surface.time().round;
    let mut rows: Vec<Row> = Vec::new();
    for seat in seat_ids(seats) {
        let state = surface
            .seat_state(Subject::Seat(seat), seat)
            .expect("its own")
            .clone();
        let sealed = state.sealed.as_ref();
        let sealed_by_easy =
            sealed.is_some_and(|held| held.round == round && !held.filed_by_the_gateway);
        let note = sealed
            .and_then(|held| {
                held.playbook_jsonc
                    .lines()
                    .find(|line| line.trim_start().starts_with("\"note\""))
                    .map(|line| line.trim().to_owned())
            })
            .unwrap_or_default();

        let token = in_process_token(surface, seat.raw());
        let (at_risk, core_dark) = own_power(surface, &token, seat.raw());
        let (advice, estimated) = advise_unforced(surface, &token, seat.raw(), rules);
        let why_safe = advice
            .suggestions
            .iter()
            .find(|suggestion| suggestion.template_id == "safe_playbook")
            .map(|suggestion| suggestion.why.clone())
            .unwrap_or_default();
        let verified = call(
            surface,
            &token,
            "verify_plan",
            Json::Object(vec![
                (
                    String::from("playbook_jsonc"),
                    Json::String(advice.safe_playbook_jsonc.clone()),
                ),
                (String::from("depth"), Json::String(String::from("full"))),
            ]),
        );
        let safe_qualifies = !advice.safe_playbook_jsonc.is_empty()
            && result(&verified, "verify_plan")
                .get("report")
                .and_then(|report| report.get("qualifies"))
                == Some(&Json::Bool(true));
        let forecast = result(
            &call(surface, &token, "get_economy_forecast", params("{}")),
            "get_economy_forecast",
        );
        // A token of its own, so the forced advisor's calls do not share the
        // advisor's per-tick budget above.
        let forced_token = in_process_token(surface, seat.raw());
        let forced = forced_raise(surface, &forced_token, seat.raw(), rules);
        let methods: Vec<String> = calls
            .iter()
            .find(|(held, _)| *held == seat.raw())
            .map(|(_, methods)| methods.clone())
            .unwrap_or_default();
        let methods = methods.as_slice();
        rows.push(Row {
            snapshot: label.to_owned(),
            seat: seat.raw(),
            seal: match (sealed_by_easy, note.contains("Easy, seat")) {
                (false, _) => Seal::NotEasy,
                (true, true) => Seal::Own,
                (true, false) => Seal::Safe,
            },
            composed: count(methods, "patch_plan") > 0,
            patches: count(methods, "patch_plan"),
            verifies: count(methods, "verify_plan"),
            note,
            calls: u32::try_from(methods.len()).expect("counted"),
            safe_qualifies,
            headroom_kw: number(&forecast, "headroom_kw_now"),
            advisor_calls: advice.calls,
            raised: raised_by(&advice),
            at_risk,
            core_dark,
            estimated,
            why_safe,
            supply_kw: number(&forecast, "supply_kw_now"),
            forced,
        });
    }
    rows
}

/// A whole match of `rounds` rounds, every seat Easy, visiting every Lull.
fn play_match(label: &str, seed: u64, seats: u32, rules: &str, rounds: u32) -> Vec<Row> {
    let mut surface = open(&match_id(label), seed, seats, rules, rounds);
    let mut easy = all_built_in(&mut surface, seats, rules);
    let mut rows: Vec<Row> = Vec::new();
    for round in 1..=rounds {
        rows.extend(visit(
            &format!("{label}/round-{round}"),
            &mut surface,
            seats,
            rules,
            &mut easy,
        ));
        assert!(
            surface.begin_push().expect("the Push opens"),
            "round {round} would not open"
        );
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
    rows
}

/// The corpus, built once for every test that reads it.
fn corpus() -> &'static [Row] {
    static CORPUS: OnceLock<Vec<Row>> = OnceLock::new();
    CORPUS.get_or_init(|| {
        let rules = rules_json();
        let short = power_short_json();
        let mut rows: Vec<Row> = Vec::new();
        for seats in 1..=3 {
            let label = format!("first-lull-{seats}-seats");
            let mut surface = open(&match_id(&label), SEED, seats, &rules, 3);
            let mut easy = all_built_in(&mut surface, seats, &rules);
            rows.extend(visit(&label, &mut surface, seats, &rules, &mut easy));
        }
        let mut surface = open(&match_id("first-lull-short"), SEED, 2, &short, 3);
        let mut easy = all_built_in(&mut surface, 2, &short);
        rows.extend(visit(
            "first-lull-2-seats-power-short",
            &mut surface,
            2,
            &short,
            &mut easy,
        ));
        rows.extend(play_match("three-rounds", SEED, 3, &rules, 3));
        rows.extend(play_match("three-rounds-power-short", SEED, 2, &short, 3));
        rows
    })
}

// ---------------------------------------------------------------------------
// The acceptance tests
// ---------------------------------------------------------------------------

/// The plan's T18 acceptance line: "a 100 % verifier pass rate over the
/// snapshot corpus". On every snapshot, every seat Easy plays seals a
/// playbook the verifier accepted -- its own plan or its own safe playbook,
/// never the gateway's fallback -- within its derived call budget.
///
/// The seal rate alone would not tell a composer whose plans never qualify
/// from a good one (it would seal its safe playbook every time), so the
/// **own-plan** pass rate is asserted separately: every round in which Easy
/// composed a plan (it called `patch_plan`) sealed that plan, on its first
/// verify, with no repair and no fallback to its safe playbook. A round
/// with nothing worth composing seals the safe playbook and is counted as
/// such, not as a pass of its own plan.
#[test]
fn easy_passes_the_verifier_on_every_snapshot_of_the_corpus() {
    let rows = corpus();
    assert!(
        rows.len() >= 20,
        "the corpus is the fixtures plus two whole matches"
    );
    let failed: Vec<&Row> = rows
        .iter()
        .filter(|row| row.seal == Seal::NotEasy)
        .collect();
    assert!(
        failed.is_empty(),
        "Easy sealed nothing of its own on: {failed:#?}"
    );
    let composed: Vec<&Row> = rows.iter().filter(|row| row.composed).collect();
    let fell_back: Vec<&Row> = composed
        .iter()
        .copied()
        .filter(|row| row.seal != Seal::Own)
        .collect();
    assert!(
        fell_back.is_empty(),
        "Easy composed a plan that did not qualify and fell back to its safe playbook: \
         {fell_back:#?}"
    );
    // Repairs are allowed (spec section 14: "verify and repair up to 4
    // times"); how many were spent is reported, not asserted.
    let repairs: u32 = composed
        .iter()
        .map(|row| row.patches.saturating_sub(1))
        .sum();
    assert!(
        composed.iter().all(|row| row.verifies == row.patches),
        "a composed round verifies once after the composition and once per repair: \
         {composed:#?}"
    );
    assert!(
        rows.iter()
            .all(|row| (row.seal == Seal::Own) == row.composed),
        "an own seal is exactly a composed round: {rows:#?}"
    );
    let own = composed.len();
    let safe = rows.len().saturating_sub(own);
    assert!(
        own > 0,
        "Easy sealed at least one plan of its own: {rows:#?}"
    );
    // Said in the output of a passing run too, for the PR's numbers.
    eprintln!(
        "corpus: {} seat-snapshots; {own} sealed Easy's own plan (own-plan pass rate {own}/{own}, \
         {repairs} repairs), {safe} sealed its safe playbook with nothing worth composing",
        rows.len()
    );
    for row in rows {
        assert!(
            row.calls <= EASY_CALL_BUDGET && row.calls > 0,
            "{} seat {}: {} calls against {EASY_CALL_BUDGET}",
            row.snapshot,
            row.seat,
            row.calls
        );
        assert!(row.advisor_calls <= EASY_ADVISOR_CALL_BUDGET, "{row:#?}");
    }
    // What it chose, said so a reader of a red build can see it: at the
    // golden seed a seat whose seam is inside its route share expands beside
    // it in round one, and a seat whose seam is not, or whose seam is already
    // worked, seals its safe playbook.
    assert!(
        rows.iter()
            .any(|row| row.snapshot == "three-rounds/round-1" && row.note.contains("Mine beacon")),
        "{rows:#?}"
    );
}

/// Spec section 14: the safe playbook "always qualifies". On every fixture
/// snapshot and every round of both hosted matches, including the
/// power-short ones, for every seat.
#[test]
fn the_safe_playbook_always_qualifies() {
    let rows = corpus();
    let failed: Vec<&Row> = rows.iter().filter(|row| !row.safe_qualifies).collect();
    assert!(
        failed.is_empty(),
        "a safe playbook did not qualify: {failed:#?}"
    );
    assert!(
        rows.iter()
            .any(|row| row.snapshot.contains("power-short") && row.headroom_kw < 0),
        "the power-short variant is power short"
    );
    assert!(
        rows.iter()
            .filter(|row| !row.snapshot.contains("power-short"))
            .all(|row| row.headroom_kw >= 0),
        "and the committed rules are not"
    );
    // Never more than two raised. Said plainly, since a reader will look for
    // the raise path here: on a real grid it does not fire, and the
    // conclusion T18 drew stands though its mechanism has changed. Since
    // T14b (decisions-log items 113 (5) and 114 (4)) a placed beacon adds no
    // draw to the grid, so the committed rules never run short, and the
    // power-short match's only dark beacon is its core: shed at the Push's
    // first settle (1 kW of surplus against the starting force's 4 kW) and
    // never revived (its revival costs 3 kW against 0 kW of headroom and a
    // 2 kW margin), after which supply and draw are both 0 and a beacon Easy
    // places later is pushed awake, adds no draw, meets no deficit and stays
    // lit. The safe playbook never raises the core, so no corpus row holds a
    // beacon at risk. `the_safe_playbook_raises_only_what_is_at_risk_unforced`
    // below proves the gate and its why on this grid; `forced_raise` and the
    // scripted client (crates/operator/tests/scripted.rs) remain the proof
    // that a raise is composed. The definitions are the owner's at S1.
    assert!(rows.iter().all(|row| row.raised.len() <= 2), "{rows:#?}");
}

/// How many corpus rows the safe playbook raises anything on, unforced.
///
/// **Zero**, and expected so (decisions-log item 114 (4), which corrects item
/// 113 (5)'s premise): on T14b's grid no corpus row holds a dark non-core
/// beacon (see the note in `the_safe_playbook_always_qualifies`). A row that
/// raises means the grid or the corpus changed; the mechanism is said before
/// this number is.
const ROWS_THAT_RAISE: usize = 0;

/// Decisions-log item 113 (4), hosted and unforced, on every corpus row with
/// no answer rewritten: the advisor's safe playbook raises only the seat's own
/// dark, non-core, non-HIGH beacons, at most `SAFE_MAX_RAISED`, and at least
/// one whenever such a beacon is within `SAFE_REACH_MS` by the advisor's own
/// estimate; and on every power-short row from round 2 on -- the core dark at
/// 0 kW -- it raises and estimates nothing, and its why says power is short.
///
/// This proves the new gate and its why on a real grid, not that a raise
/// fires: `forced_raise` stays the hosted proof that one is composed.
#[test]
fn the_safe_playbook_raises_only_what_is_at_risk_unforced() {
    let rows = corpus();
    for row in rows {
        let what = format!("{} seat {}: {row:#?}", row.snapshot, row.seat);
        assert!(row.raised.len() <= SAFE_MAX_RAISED, "{what}");
        assert!(
            row.raised.iter().all(|id| row.at_risk.contains(id)),
            "raised only the seat's own dark, non-core, non-HIGH beacons: {what}"
        );
        let reachable = row
            .estimated
            .iter()
            .any(|(id, within)| *within && row.at_risk.contains(id));
        assert!(
            !reachable || !row.raised.is_empty(),
            "a beacon at risk was within reach and nothing was raised: {what}"
        );
        assert!(
            row.estimated.iter().all(|(id, _)| row.at_risk.contains(id)),
            "only a beacon at risk is ever estimated: {what}"
        );
        if row.at_risk.is_empty() && !row.core_dark && row.headroom_kw >= 0 {
            assert!(
                row.why_safe.contains("Power is not short"),
                "nothing dark and headroom at zero or more: {what}"
            );
        } else {
            assert!(
                !row.why_safe.contains("Power is not short"),
                "an own beacon is dark or the headroom is below zero: {what}"
            );
        }
    }
    let raising = rows.iter().filter(|row| !row.raised.is_empty()).count();
    eprintln!(
        "unforced: {raising} of {} corpus rows raise anything",
        rows.len()
    );
    assert_eq!(
        raising, ROWS_THAT_RAISE,
        "the named expectation moved: say which seat, round and beacon, and the mechanism"
    );

    // The power-short match from round 2 on: a total blackout, the core dark
    // and the headroom 0. Asserted as the precondition first, so a rules or
    // grid change fails loudly here rather than quietly passing below.
    let blackout: Vec<&Row> = rows
        .iter()
        .filter(|row| {
            row.snapshot.starts_with("three-rounds-power-short/")
                && !row.snapshot.ends_with("round-1")
        })
        .collect();
    assert!(
        !blackout.is_empty(),
        "the power-short match has rounds 2 and 3"
    );
    for row in blackout {
        let what = format!("{} seat {}: {row:#?}", row.snapshot, row.seat);
        assert!(
            row.core_dark && row.headroom_kw == 0,
            "a total blackout: {what}"
        );
        assert!(row.raised.is_empty() && row.estimated.is_empty(), "{what}");
        assert!(row.why_safe.contains("Power is short"), "{what}");
        assert!(!row.why_safe.contains("Power is not short"), "{what}");
        assert!(
            row.why_safe.contains("never raises the core") && !row.why_safe.contains(" kW"),
            "the core, and no kW figure as the shortage: {what}"
        );
    }
    // Whether any corpus seat built a Generator, for the pull request: its
    // supply above the committed core surplus.
    let generators: Vec<String> = rows
        .iter()
        .filter(|row| !row.snapshot.contains("power-short") && row.supply_kw > 10)
        .map(|row| format!("{} seat {} ({} kW)", row.snapshot, row.seat, row.supply_kw))
        .collect();
    eprintln!("corpus rows whose supply shows a Generator: {generators:?}");
}

/// The raise path through the **real** verifier ([`forced_raise`]). The
/// hosted corpus never raises by itself (see above), so on every snapshot
/// where a seat has an own non-core beacon, an advisor is told power is
/// short and those beacons are dark, and everything else is the gateway's
/// own answer. Its safe playbook must raise them -- at most two, which only
/// happens when `instantiate_template` accepted the whole route and the
/// raised form qualified FULL, since `safe_playbook` otherwise falls back to
/// nothing raised -- and must qualify FULL again when verified on its own.
#[test]
fn the_safe_playbook_with_beacons_raised_qualifies_through_the_real_verifier() {
    let forced: Vec<(&Row, &Forced)> = corpus()
        .iter()
        .filter_map(|row| row.forced.as_ref().map(|forced| (row, forced)))
        .collect();
    assert!(
        !forced.is_empty(),
        "some seat of the corpus has an own non-core beacon to raise"
    );
    // Said in the output of a passing run too, for the PR's numbers.
    for (row, forced) in &forced {
        eprintln!(
            "forced raise: {} seat {}: dark {:?}, raised {:?}",
            row.snapshot, row.seat, forced.dark, forced.raised
        );
    }
    for (row, forced) in forced {
        let what = format!("{} seat {}: {forced:#?}", row.snapshot, row.seat);
        assert!(
            !forced.raised.is_empty(),
            "the raised form did not fall back: {what}"
        );
        assert!(forced.raised.len() <= 2, "{what}");
        assert!(
            forced.raised.iter().all(|id| forced.dark.contains(id)),
            "it raised only the seat's own dark beacons: {what}"
        );
        assert!(forced.carries_raise, "{what}");
        assert!(
            forced.qualifies,
            "the raised safe playbook qualifies FULL: {what}"
        );
    }
}

/// Round one of a three-seat match, every seat Easy: each seat's sealed
/// playbook, in seat order.
fn round_one(seed: u64, match_id: &str) -> Vec<String> {
    let rules = rules_json();
    let mut surface = open(match_id, seed, 3, &rules, 3);
    let mut easy = all_built_in(&mut surface, 3, &rules);
    easy.plan(&mut surface);
    seat_ids(3)
        .into_iter()
        .map(|seat| {
            let state = surface
                .seat_state(Subject::Seat(seat), seat)
                .expect("its own");
            let sealed = state.sealed.as_ref().expect("Easy sealed a playbook");
            assert!(!sealed.filed_by_the_gateway);
            sealed.playbook_jsonc.clone()
        })
        .collect()
}

/// The cargo target directory, honouring `CARGO_TARGET_DIR`.
fn target_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .parent()
        .expect("CARGO_TARGET_TMPDIR has a parent")
        .to_path_buf()
}

/// Spec section 14: "It is deterministic -- its seed comes from the match,
/// seat and round". Two hosts of the same match, seat and round seal the same
/// bytes, and those bytes are the golden playbook for that seed under
/// `tests/golden/operator/`, which the three-OS matrix compares.
///
/// The match id differs between the two hosts on purpose: it names the
/// private cache folder and is not an input of the match.
#[test]
fn the_operator_is_deterministic_for_a_given_match_seat_round() {
    for seed in [SEED, SCENARIO_SEED] {
        let first = round_one(seed, &match_id(&format!("determinism-a-{seed:x}")));
        let second = round_one(seed, &match_id(&format!("determinism-b-{seed:x}")));
        assert_eq!(
            first, second,
            "seed {seed:#018x}: two hosts, one playbook per seat"
        );
        let case = target_dir()
            .join("golden")
            .join("operator")
            .join(format!("seed-{seed:016x}"));
        std::fs::create_dir_all(&case).expect("the golden folder");
        for (seat, playbook) in first.iter().enumerate() {
            assert!(!playbook.contains('\r') && playbook.ends_with('\n'));
            std::fs::write(case.join(format!("actual.seat-{seat}.jsonc")), playbook)
                .expect("the fresh golden");
        }
    }
}

/// It never reads what the host clock moves: hosts of one match whose clock
/// reports differ seal the same playbooks. Two reports go through
/// `Surface::set_host_clock`, the path `report_host_clock` takes, and differ
/// in both figures -- `elapsed`, which moves the tick in every `_status`
/// footer, and `remaining`, the countdown -- and a third host has only the
/// countdown set, as the Lull opens.
#[test]
fn the_operator_does_not_depend_on_the_host_clock() {
    use pharmakos_sim::math::quantity::Ms;
    let rules = rules_json();
    let mut sealed: Vec<Vec<String>> = Vec::new();
    for (n, report) in [None, Some((250, 179_750)), Some((60_000, 1))]
        .into_iter()
        .enumerate()
    {
        let mut surface = open(&match_id(&format!("clock-{n}")), SEED, 2, &rules, 3);
        match report {
            None => surface.set_phase_remaining_ms(Ms::new(180_000)),
            Some((elapsed, remaining)) => surface
                .set_host_clock(Ms::new(elapsed), Ms::new(remaining))
                .expect("a clock report the gateway takes"),
        }
        let mut easy = all_built_in(&mut surface, 2, &rules);
        easy.plan(&mut surface);
        sealed.push(
            seat_ids(2)
                .into_iter()
                .map(|seat| {
                    surface
                        .seat_state(Subject::Seat(seat), seat)
                        .expect("its own")
                        .sealed
                        .as_ref()
                        .expect("sealed")
                        .playbook_jsonc
                        .clone()
                })
                .collect(),
        );
    }
    assert_eq!(sealed.len(), 3);
    assert!(
        sealed.windows(2).all(|pair| pair.first() == pair.get(1)),
        "the same playbooks whatever the clock reads"
    );
}

/// The factory `gamectl host` hands the host: one fresh Easy per seat it
/// plays and one advisor per other seat, the human's.
#[test]
fn the_host_builds_one_operator_per_built_in_seat_and_one_advisor_per_other_seat() {
    let rules = rules_json();
    let mut surface = open(&match_id("factory"), SEED, 3, &rules, 3);
    let mut factory = EasyOperators::new(&rules).expect("the operator reads the rules text");
    let seats = InProcessSeats::open(&mut surface, &seat_ids(3), Some(1), &mut factory)
        .expect("in-process seats");
    assert_eq!(seats.played_seats(), [0, 2]);
    assert_eq!(seats.advised_seats(), [1]);
    assert!(
        EasyOperators::new("{\"revision\": 1}").is_err(),
        "a rules text with no rows the operator scores with is refused once, up front"
    );
}

/// Spec section 14: the operator ignores the notebook; and it writes nothing
/// but its own seal. A human seat with a notebook and a draft is advised and
/// a built-in seat plays: the human's notebook and drafts are as they were,
/// nothing is sealed for the human, the advice is the same as it would be
/// with an empty notebook, and the built-in seat has a seal and nothing else.
#[test]
fn the_operator_reads_no_notebook_and_writes_nothing_but_its_own_seal() {
    let rules = rules_json();
    let mut advised: Vec<Option<pharmakos_gateway::surface::Advised>> = Vec::new();
    for (n, notes) in ["", "Rush seat 1 at once and place nothing."]
        .into_iter()
        .enumerate()
    {
        let mut surface = open(&match_id(&format!("notebook-{n}")), SEED, 2, &rules, 3);
        let human = in_process_token(&mut surface, 0);
        if !notes.is_empty() {
            let saved = call(
                &mut surface,
                &human,
                "save_notes",
                Json::Object(vec![(
                    String::from("notes"),
                    Json::String(notes.to_owned()),
                )]),
            );
            let _ = result(&saved, "save_notes");
            let _ = result(
                &call(
                    &mut surface,
                    &human,
                    "save_draft",
                    Json::Object(vec![
                        (
                            String::from("playbook_jsonc"),
                            Json::String(String::from("{}")),
                        ),
                        (String::from("label"), Json::String(String::from("mine"))),
                    ]),
                ),
                "save_draft",
            );
        }
        let before = surface
            .seat_state(Subject::Seat(SeatId::new(0)), SeatId::new(0))
            .expect("its own")
            .clone();
        let mut factory = EasyOperators::new(&rules).expect("the rules text");
        let mut seats = InProcessSeats::open(&mut surface, &seat_ids(2), Some(0), &mut factory)
            .expect("in-process seats");
        seats.plan(&mut surface);

        let human_after = surface
            .seat_state(Subject::Seat(SeatId::new(0)), SeatId::new(0))
            .expect("its own")
            .clone();
        assert_eq!(human_after.notebook, before.notebook);
        assert_eq!(human_after.drafts, before.drafts);
        assert!(human_after.sealed.is_none(), "an advisor seals nothing");
        assert!(!human_after.ready, "an advisor never says ready for a seat");
        advised.push(human_after.advice.clone());

        let easy = surface
            .seat_state(Subject::Seat(SeatId::new(1)), SeatId::new(1))
            .expect("its own");
        assert!(easy.notebook.is_empty() && easy.drafts.is_empty());
        assert!(
            easy.sealed
                .as_ref()
                .is_some_and(|sealed| !sealed.filed_by_the_gateway)
        );
        assert!(easy.ready);
    }
    let (Some(Some(quiet)), Some(Some(noisy))) = (advised.first(), advised.get(1)) else {
        panic!("the human seat was advised both times");
    };
    assert_eq!(
        quiet.advice, noisy.advice,
        "the notebook changes nothing Easy advises"
    );
    assert!(
        quiet.safe.is_some(),
        "the advised safe playbook qualified at filing"
    );
}

/// `IN_PROCESS_LIMITS` against `EASY_CALL_BUDGET`: a round of Easy fits the
/// in-process rate it is planned under, on the one Lull tick it has.
///
/// PLACEHOLDER: both numbers are the owner's at hardening, with the rate
/// limits (decisions-log item 111, section D).
#[test]
fn easy_fits_the_in_process_limits() {
    // Checked when this test compiles, so a budget that outgrew the limit
    // fails the build rather than a run.
    const {
        assert!(EASY_CALL_BUDGET <= IN_PROCESS_LIMITS.per_tick);
        assert!(EASY_ADVISOR_CALL_BUDGET <= IN_PROCESS_LIMITS.per_tick);
        assert!(EASY_CALL_BUDGET <= IN_PROCESS_LIMITS.per_window);
    }
}

// ---------------------------------------------------------------------------
// The scenario's Generator, in kW
// ---------------------------------------------------------------------------

/// The committed scenario whose seat 0 seals Hold & Build against Easy.
const AGAINST_EASY: &str = "scenarios/skeleton/against-easy.scenario.jsonc";

/// A match hosted as the scenario's: its seed, its seat count and its one
/// segment's length, round limit 2, no units, and the template library.
/// Answers the surface in its opening Lull and the lean vent's output.
fn open_as_scenario(
    scenario: &pharmakos_gamectl::scenario::Scenario,
    rules: &str,
) -> (Surface, i64) {
    let seats = u32::try_from(scenario.seats.len()).expect("a seat count");
    let length = scenario
        .segments
        .first()
        .map(|segment| segment.length_ms)
        .expect("one segment");
    let host = Host::open_from(
        rules,
        scenario.seed,
        seats,
        &Settings {
            segment_lengths_ms: vec![length],
            round_limit: 2,
            units_per_seat: 0,
        },
        Some(root().join(LIBRARY_PATH)),
    )
    .expect("a match");
    let table = RulesTable::from_canonical_json(rules).expect("a rules table");
    let lean = table
        .message()
        .power
        .as_ref()
        .and_then(|power| power.generator_output_kw.as_ref())
        .map(|row| i64::from(row.lean))
        .expect("power.generator_output_kw");
    let mut surface = Surface::new(
        &match_id("against-easy-kw"),
        scenario.seed,
        table,
        FogPolicy::fogged(),
        &seat_ids(seats),
    )
    .expect("a surface");
    surface.attach(host).expect("attached");
    surface.open_lull().expect("the opening Lull");
    (surface, lean)
}

/// The scenario's committed playbook for seat 0, held to Easy's
/// `instantiate_template{suggested}` answer for seat 0: that answer with the
/// scenario files' header and one comment block in place of its first two
/// lines. The fresh answer is written to the target directory either way.
fn committed_as_suggested(
    surface: &mut Surface,
    token: &Token,
    scenario: &pharmakos_gamectl::scenario::Scenario,
) -> String {
    let answer = result(
        &call(
            surface,
            token,
            "instantiate_template",
            params(r#"{"template_id":"hold_and_build","suggested":true}"#),
        ),
        "instantiate_template{suggested}",
    );
    let suggested = match answer.get("playbook_jsonc") {
        Some(Json::String(text)) => text.clone(),
        other => panic!("no playbook_jsonc: {other:?}"),
    };
    let playbook_path = scenario
        .seats
        .iter()
        .find_map(|seat| match &seat.kind {
            pharmakos_gamectl::scenario::SeatKind::Playbook(path) if seat.seat == 0 => {
                Some(path.clone())
            }
            _ => None,
        })
        .expect("seat 0 seals a playbook file");
    let committed = std::fs::read_to_string(root().join(&playbook_path)).expect("the playbook");
    let fresh = target_dir()
        .join("golden")
        .join("against-easy.suggested.jsonc");
    std::fs::create_dir_all(target_dir().join("golden")).expect("the folder");
    std::fs::write(&fresh, &suggested).expect("the fresh answer");
    let below = suggested
        .splitn(3, '\n')
        .nth(2)
        .expect("the template's header");
    let (header, rest) = committed.split_at(committed.len().saturating_sub(below.len()));
    assert!(
        rest == below,
        "{playbook_path} is not Easy's suggestion below its header; the fresh answer is at {}, \
         and the file is derived from it as its own header says",
        fresh.display()
    );
    assert!(
        header.starts_with(
            "// SPDX-FileCopyrightText: 2026 Pharmakos contributors\n\
             // SPDX-License-Identifier: GPL-3.0-or-later\n"
        ) && header.lines().all(|line| line.starts_with("//")),
        "the scenario files' header, then comments only: {header}"
    );
    committed
}

/// Decisions-log item 113 (6): the scenario proves seat 0's Generator is
/// completed, and this proves what it gives. One short match replays the
/// scenario's own setup -- its seed, its seats (seat 0 a playbook, the rest
/// Easy's) and its segment length, read from the file -- with round limit 2,
/// so a second Lull opens and no second Push is played. Seat 0 submits the
/// scenario's committed playbook through `submit_plan`; in the next Lull its
/// `supply_kw_now` has risen by exactly the lean vent's `generator_output_kw`,
/// read from the rules text, and not by more.
///
/// Seat 0 is advised by Easy here as well, which the scenario runner never
/// does (an advisor writes nothing but advice, and seat 0 submits), so that
/// the committed playbook is held to what it says it is: Easy's
/// `instantiate_template{suggested}` answer for seat 0, with the scenario
/// files' header and one comment block in place of the template's first two
/// lines.
#[test]
fn the_scenario_generator_raises_supply_by_exactly_the_lean_vent() {
    let scenario = pharmakos_gamectl::scenario::load(&root(), Path::new(AGAINST_EASY))
        .expect("the committed scenario");
    let rules = std::fs::read_to_string(root().join(&scenario.rules)).expect("its rules text");
    let (mut surface, lean) = open_as_scenario(&scenario, &rules);
    let seats = u32::try_from(scenario.seats.len()).expect("a seat count");
    let mut factory = EasyOperators::new(&rules).expect("the operator reads the rules text");
    let mut easy = InProcessSeats::open(&mut surface, &seat_ids(seats), Some(0), &mut factory)
        .expect("in-process seats");
    assert_eq!(easy.advised_seats(), [0], "seat 0 is the playbook seat");
    easy.plan(&mut surface);
    let token = in_process_token(&mut surface, 0);
    let committed = committed_as_suggested(&mut surface, &token, &scenario);

    let supply = |surface: &mut Surface| {
        number(
            &result(
                &call(surface, &token, "get_economy_forecast", params("{}")),
                "get_economy_forecast",
            ),
            "supply_kw_now",
        )
    };
    let before = supply(&mut surface);
    let submitted = result(
        &call(
            &mut surface,
            &token,
            "submit_plan",
            Json::Object(vec![(
                String::from("playbook_jsonc"),
                Json::String(committed),
            )]),
        ),
        "submit_plan",
    );
    assert_eq!(
        submitted.get("accepted"),
        Some(&Json::Bool(true)),
        "{submitted:?}"
    );
    assert!(surface.begin_push().expect("the Push opens"));
    while let Some(report) = surface.step().expect("a tick") {
        if report.segment_ended {
            break;
        }
    }
    surface.end_recap().expect("the recap closes");
    surface.open_lull().expect("round 2's Lull");
    let after = supply(&mut surface);
    eprintln!("seat 0's supply: {before} kW in round 1's Lull, {after} kW in round 2's");
    assert_eq!(
        after.saturating_sub(before),
        lean,
        "the Generator on the lean vent adds exactly its output, and nothing else moved supply"
    );
}
