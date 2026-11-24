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
//! playbook raised unforced (T18b; decisions-log items 113 (4) and 114 (4)),
//! read as S1's plan, decision 14 reads "short" and "at risk" (the register's
//! S1-21; `crates/operator/src/safe.rs`).
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
use pharmakos_operator::easy::{
    EASY_ADVISOR_CALL_BUDGET, EASY_CALL_BUDGET, SAFE_MAX_RAISED, SAFE_REACH_MS,
};
use pharmakos_operator::{Easy, FailedStones, PushUnread, StonesUnread, Submitted};
use pharmakos_proto::json::Json;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::tables::SeatId;

/// The determinism golden's seed.
const SEED: u64 = pharmakos_sim::DETERMINISM_MATCH_SEED;

/// Probation's own seed (`PROBATION_SEED` in
/// `crates/sim/tests/probation_seed.rs`; decisions-log item 156 (1)).
const PROBATION_SEED: u64 = 20;

/// The scenario files' seed, the second golden seed.
const SCENARIO_SEED: u64 = 0x0000_0000_ca5c_aded;

/// The corpus's Push length: long enough for a commander to walk to a vent
/// and place a Build beacon with its initial settings at the prices the
/// verifier charges (S3's `opr3`, fix pass 1: at 60 s the 42 s share no longer
/// held the starting vents' 40 s routes once the core visit's handshake and
/// the initial-settings row were priced), short enough to step in a test: the
/// `against-easy` scenario's own 80 s.
const SEGMENT_MS: i32 = 80_000;

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
    open_under(match_id, seed, seats, rules, rounds, FogPolicy::fogged())
}

/// [`open`] under a fog policy of the caller's.
fn open_under(
    match_id: &str,
    seed: u64,
    seats: u32,
    rules: &str,
    rounds: u32,
    fog: FogPolicy,
) -> Surface {
    let host = Host::open_from(
        rules,
        seed,
        seats,
        &Settings {
            segment_lengths_ms: vec![SEGMENT_MS],
            round_limit: rounds,
        },
        Some(root().join(LIBRARY_PATH)),
    )
    .expect("a match");
    let table = RulesTable::from_canonical_json(rules).expect("a rules table");
    let ids: Vec<SeatId> = (0..seats)
        .filter_map(|raw| u8::try_from(raw).ok())
        .map(SeatId::new)
        .collect();
    let mut surface = Surface::new(match_id, seed, table, fog, &ids).expect("a surface");
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
    /// The `patch_plan` and FULL `verify_plan` calls the built-in seat made
    /// (the tap probe's QUICK verify left out).
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
    /// The seat's own power on this snapshot, as `list_beacons` answered it,
    /// unrewritten.
    power: OwnPower,
    /// The beacons the advisor estimated a route to (the safe playbook's
    /// estimates are its only ones that end on a beacon), with whether its
    /// own estimate put each within `SAFE_REACH_MS`.
    estimated: Vec<(String, bool)>,
    /// The advisor's "why" for the Safe Playbook page.
    why_safe: String,
    /// The seat's `supply_kw_now` on this snapshot.
    supply_kw: i64,
    /// The seat's `draw_kw_now` on this snapshot.
    draw_kw: i64,
    /// The raise path forced through the real verifier (see
    /// [`forced_raise`]): `None` when the seat has no own beacon at risk on
    /// this snapshot, else what was raised and whether it qualified FULL.
    forced: Option<Forced>,
}

/// What an advisor raised when told power is short, on a real `Surface`.
#[derive(Clone, Debug)]
struct Forced {
    /// The own beacons at risk on this snapshot: lit, not the core and not
    /// HIGH.
    at_risk: Vec<String>,
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

/// What `list_beacons` shows of the seat's own power.
#[derive(Clone, Debug)]
struct OwnPower {
    /// The own beacons the safe playbook may raise -- lit, not the core, and
    /// not HIGH (decision 14's "at risk", before the shed order picks).
    at_risk: Vec<String>,
    /// The seat's core is browned out.
    core_dark: bool,
    /// Any own beacon is browned out, whatever its priority or kind.
    any_dark: bool,
}

/// The seat's own power, as `list_beacons` answers it.
fn own_power(surface: &mut Surface, token: &Token, seat: u8) -> OwnPower {
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
        .filter(|row| row.get("owner") == Some(&own) && !dark(row) && !core(row))
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
    let any_dark = rows
        .iter()
        .any(|row| row.get("owner") == Some(&own) && dark(row));
    OwnPower {
        at_risk,
        core_dark,
        any_dark,
    }
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
/// call forwarded untouched except one answer rewritten: its
/// `get_economy_forecast` says the draw is 1 kW above the supply, which is
/// "power is short" (decision 14; the register's S1-21), while every beacon
/// stays as `list_beacons` shows it. Everything else -- the beacons, the
/// estimates, `instantiate_template` with the whole route, the FULL
/// `verify_plan` -- is the gateway's own answer. `None` when the seat has no
/// own beacon at risk (lit, not the core, not HIGH) to raise.
fn forced_raise(surface: &mut Surface, token: &Token, seat: u8, rules: &str) -> Option<Forced> {
    let at_risk = own_power(surface, token, seat).at_risk;
    if at_risk.is_empty() {
        return None;
    }
    let mut advisor = Easy::new(rules).expect("the rules text");
    let advice = {
        let mut rewriting = |method: &str, params: Json| {
            let mut answer = call(surface, token, method, params);
            if method != "get_economy_forecast" {
                return answer;
            }
            if let Json::Object(members) = &mut answer
                && let Some((_, fields)) = members.iter_mut().find(|(key, _)| key == "result")
            {
                let supply = number(fields, "supply_kw_now");
                set_member(
                    fields,
                    "draw_kw_now",
                    Json::Number(supply.saturating_add(1).to_string()),
                );
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
        at_risk,
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
        let sealed = state.sealed();
        let sealed_by_easy =
            sealed.is_some_and(|held| held.round() == round && !held.filed_by_the_gateway());
        let note = sealed
            .and_then(|held| {
                held.playbook_jsonc()
                    .lines()
                    .find(|line| line.trim_start().starts_with("\"note\""))
                    .map(|line| line.trim().to_owned())
            })
            .unwrap_or_default();

        let token = in_process_token(surface, seat.raw());
        let power = own_power(surface, &token, seat.raw());
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
            // The tap probe's QUICK `verify_plan` rides its `resolve_refs`
            // (`crates/operator/src/candidates.rs`' `probe`); every other one
            // is the composition's FULL verify or a repair's.
            verifies: count(methods, "verify_plan")
                .checked_sub(count(methods, "resolve_refs"))
                .expect("every tap probe's `resolve_refs` rides one QUICK `verify_plan`"),
            note,
            calls: u32::try_from(methods.len()).expect("counted"),
            safe_qualifies,
            headroom_kw: number(&forecast, "headroom_kw_now"),
            advisor_calls: advice.calls,
            raised: raised_by(&advice),
            power,
            estimated,
            why_safe,
            supply_kw: number(&forecast, "supply_kw_now"),
            draw_kw: number(&forecast, "draw_kw_now"),
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
    // What it chose, said so a reader of a red build can see it: on the
    // determinism seed each seat's round one places a Build beacon on its
    // start vent with a Generator on it, by the vent's name, ranked above a
    // Survey post, which is worth the raid it buys (S3's `opr3`, fix pass 2,
    // item 2).
    assert!(
        rows.iter()
            .filter(|row| row.snapshot == "three-rounds/round-1")
            .all(|row| row
                .note
                .contains("A Build beacon covering the lean heat vent vent_")),
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
    // Never more than two raised. Whether a raise fires unforced, and where,
    // is `the_safe_playbook_raises_only_what_is_at_risk_unforced`'s named
    // expectation; `forced_raise` and the scripted client
    // (crates/operator/tests/scripted.rs) remain the proof that a raise is
    // composed.
    assert!(rows.iter().all(|row| row.raised.len() <= 2), "{rows:#?}");
}

/// The corpus rows the safe playbook raises anything on, unforced, as
/// `(snapshot, seat)`: a named expectation, so a row that starts or stops
/// raising says which seat, round and beacon moved, and the mechanism.
///
/// The power-short match's core is shed at the Push's first settle and never
/// revived, so from round 2 on its seats are short. Each seat's round-1 seal
/// places a lit non-core beacon (the Build beacon on its start vent since S3's
/// `opr3`, fix pass 2, item 2), which in round 2's Lull is its one beacon at
/// risk (decision 14: lit and next in the shed order, since a raise
/// relights), within reach, and is raised to HIGH. In round 3 seat 0's
/// beacon is HIGH already and nothing more of its is at risk; seat 1's
/// round-2 seal placed a stepping stone (fix pass 3, item 1: a stone is
/// charged its own price), a second lit beacon, which round 3 raises.
const ROWS_THAT_RAISE: &[(&str, u8)] = &[
    ("three-rounds-power-short/round-2", 0),
    ("three-rounds-power-short/round-2", 1),
    ("three-rounds-power-short/round-3", 1),
];

/// Decisions-log item 128 (S1's plan, decision 14; the register's S1-21),
/// hosted and unforced, on every corpus row with no answer rewritten: power
/// is short exactly when an own beacon is dark or the draw is above the
/// supply; the advisor's safe playbook raises only the seat's own lit,
/// non-core, non-HIGH beacons, at most `SAFE_MAX_RAISED`, and at least one
/// whenever such a beacon is within `SAFE_REACH_MS` by the advisor's own
/// estimate while power is short; it estimates nothing while power is not
/// short; and on every power-short row from round 2 on -- the core dark --
/// its why says power is short and names the core.
#[test]
fn the_safe_playbook_raises_only_what_is_at_risk_unforced() {
    let rows = corpus();
    for row in rows {
        let what = format!("{} seat {}: {row:#?}", row.snapshot, row.seat);
        let short = row.power.any_dark || row.draw_kw > row.supply_kw;
        assert!(row.raised.len() <= SAFE_MAX_RAISED, "{what}");
        assert!(
            row.raised.iter().all(|id| row.power.at_risk.contains(id)),
            "raised only the seat's own lit, non-core, non-HIGH beacons: {what}"
        );
        assert!(
            short || (row.estimated.is_empty() && row.raised.is_empty()),
            "nothing is estimated or raised while power is not short: {what}"
        );
        let reachable = row
            .estimated
            .iter()
            .any(|(id, within)| *within && row.power.at_risk.contains(id));
        assert!(
            !reachable || !row.raised.is_empty(),
            "a beacon at risk was within reach and nothing was raised: {what}"
        );
        assert!(
            row.estimated
                .iter()
                .all(|(id, _)| row.power.at_risk.contains(id)),
            "only a beacon at risk is ever estimated: {what}"
        );
        assert_eq!(
            !row.why_safe.contains("Power is not short"),
            short,
            "the why says power is short exactly when it is: {what}"
        );
    }
    let raising: Vec<(&str, u8)> = rows
        .iter()
        .filter(|row| !row.raised.is_empty())
        .map(|row| (row.snapshot.as_str(), row.seat))
        .collect();
    eprintln!("unforced: corpus rows that raise anything: {raising:?}");
    assert_eq!(
        raising, ROWS_THAT_RAISE,
        "the named expectation moved: say which seat, round and beacon, and the mechanism"
    );

    // The power-short match from round 2 on: the core dark. Asserted as the
    // precondition first, so a rules or grid change fails loudly here rather
    // than quietly passing below.
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
        assert!(row.power.core_dark, "the core is dark: {what}");
        assert!(row.why_safe.contains("Power is short"), "{what}");
        assert!(
            row.why_safe.contains("because the core b_00"),
            "the core, named as the shortage: {what}"
        );
    }
    // The corpus's own Generator (decisions-log item 113 (6) and (9): the kW
    // check rides the corpus where it can). On the committed rules a seat's
    // supply is its core's surplus until a Generator of its own completes, so
    // a row above it is a Generator; the rows that show one are a named
    // expectation, and each rose by exactly the lean vent's output, both read
    // from the rules text.
    let (core_surplus, lean) = core_surplus_and_lean(&rules_json());
    let generators: Vec<(&str, u8)> = rows
        .iter()
        .filter(|row| !row.snapshot.contains("power-short") && row.supply_kw > core_surplus)
        .map(|row| (row.snapshot.as_str(), row.seat))
        .collect();
    eprintln!("corpus rows whose supply shows a Generator: {generators:?}");
    assert_eq!(
        generators, GENERATOR_ROWS,
        "the named expectation moved: say which seat built what, and why"
    );
    for row in rows
        .iter()
        .filter(|row| GENERATOR_ROWS.contains(&(row.snapshot.as_str(), row.seat)))
    {
        assert_eq!(
            row.supply_kw.saturating_sub(core_surplus),
            lean,
            "one Generator on a lean vent, and nothing else moved supply: {} seat {}",
            row.snapshot,
            row.seat
        );
    }
}

/// The corpus rows on the committed rules whose seat's supply shows a
/// Generator: every seat of the three-round match from round 2 on, each the
/// Generator its round-1 seal put on its lean start vent (S3's `opr3`, fix
/// pass 2, item 2: one ranking, a stone discounted by the stones and rounds
/// its chain still needs and a Survey post worth the raid it buys, so the
/// seat's first Generator ranks first, as it did before the stones). The
/// scenario's own Generator is
/// `the_scenario_generator_raises_supply_by_exactly_the_lean_vent`'s.
const GENERATOR_ROWS: &[(&str, u8)] = &[
    ("three-rounds/round-2", 0),
    ("three-rounds/round-2", 1),
    ("three-rounds/round-2", 2),
    ("three-rounds/round-3", 0),
    ("three-rounds/round-3", 1),
    ("three-rounds/round-3", 2),
];

/// The rules text's `power.core_surplus_kw` and the lean vent's
/// `power.generator_output_kw`, kW.
fn core_surplus_and_lean(rules: &str) -> (i64, i64) {
    let table = RulesTable::from_canonical_json(rules).expect("a rules table");
    let power = table.message().power.as_ref().expect("the power block");
    let lean = power
        .generator_output_kw
        .as_ref()
        .map(|row| i64::from(row.lean))
        .expect("power.generator_output_kw");
    (i64::from(power.core_surplus_kw), lean)
}

/// The raise path through the **real** verifier ([`forced_raise`]). On every
/// snapshot where a seat has an own beacon at risk (lit, not the core, not
/// HIGH), an advisor is told the draw is above the supply, and everything
/// else is the gateway's own answer. Its safe playbook must raise them -- at
/// most two, which only happens when `instantiate_template` accepted the
/// whole route and the raised form qualified FULL, since `safe_playbook`
/// otherwise falls back to the template's own route -- and must qualify FULL
/// again when verified on its own.
#[test]
fn the_safe_playbook_with_beacons_raised_qualifies_through_the_real_verifier() {
    let forced: Vec<(&Row, &Forced)> = corpus()
        .iter()
        .filter_map(|row| row.forced.as_ref().map(|forced| (row, forced)))
        .collect();
    assert!(
        !forced.is_empty(),
        "some seat of the corpus has an own beacon at risk to raise"
    );
    // Said in the output of a passing run too, for the PR's numbers.
    for (row, forced) in &forced {
        eprintln!(
            "forced raise: {} seat {}: at risk {:?}, raised {:?}",
            row.snapshot, row.seat, forced.at_risk, forced.raised
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
            forced.raised.iter().all(|id| forced.at_risk.contains(id)),
            "it raised only the seat's own beacons at risk: {what}"
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
            let sealed = state.sealed().expect("Easy sealed a playbook");
            assert!(!sealed.filed_by_the_gateway());
            sealed.playbook_jsonc().to_owned()
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
            None => surface
                .set_phase_remaining_ms(Ms::new(180_000))
                .expect("a custom match takes a countdown"),
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
                        .sealed()
                        .expect("sealed")
                        .playbook_jsonc()
                        .to_owned()
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
        assert!(human_after.sealed().is_none(), "an advisor seals nothing");
        assert!(!human_after.ready, "an advisor never says ready for a seat");
        advised.push(human_after.advice.clone());

        let easy = surface
            .seat_state(Subject::Seat(SeatId::new(1)), SeatId::new(1))
            .expect("its own");
        assert!(easy.notebook.is_empty() && easy.drafts.is_empty());
        assert!(
            easy.sealed()
                .is_some_and(|sealed| !sealed.filed_by_the_gateway())
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
        quiet.safe().is_some(),
        "the advised safe playbook qualified at filing"
    );
}

/// `IN_PROCESS_LIMITS` against `EASY_CALL_BUDGET`: a round of Easy fits the
/// in-process rate it is planned under, on the one Lull tick it has.
///
/// PLACEHOLDER: both numbers — owner, at hardening, with the rate limits (decisions-log item 111, section D).
#[test]
fn easy_fits_the_in_process_limits() {
    // Checked when this test compiles, so a budget that outgrew the limit
    // fails the build rather than a run.
    const {
        assert!(EASY_CALL_BUDGET <= IN_PROCESS_LIMITS.per_tick);
        assert!(EASY_ADVISOR_CALL_BUDGET <= IN_PROCESS_LIMITS.per_tick);
        assert!(EASY_CALL_BUDGET <= IN_PROCESS_LIMITS.per_window);
        // The derivation's sums, pinned (`crates/operator/src/easy.rs`' table):
        // 20 fixed reads, 2 tap probe, 1 guard probe, 1 raid probe, 1 Survey
        // probe, 30 candidates, 30 stepping stones, 5 templates, 1
        // composition, 9 verifies, 6 for the safe playbook, 3 to commit and 1
        // for the last seal; the advisor's the same less the Survey probe, the
        // stones, the composition, the verifies, the commit and the last seal.
        assert!(EASY_CALL_BUDGET == 110);
        assert!(EASY_ADVISOR_CALL_BUDGET == 65);
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
        },
        Some(root().join(LIBRARY_PATH)),
    )
    .expect("a match");
    let table = RulesTable::from_canonical_json(rules).expect("a rules table");
    let (_, lean) = core_surplus_and_lean(rules);
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
/// `instantiate_template{suggested}` answer for seat 0: that answer with a header
/// and one comment block in place of its first two lines. The header is the
/// scenario files' (deploy-and-visit's two lines) except that its licence line also
/// carries the library's licence, because the answer reproduces a library template;
/// the block holds comments only. The fresh answer is written to the target
/// directory either way.
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
    // The scenario files' header is deploy-and-visit's playbook's first two lines, read
    // rather than written here (a licence line spelt out in this file would be one more
    // licence expression for `reuse` to parse), except that this file reproduces a library
    // template's text, so it carries the library's licence beside the game's: the second
    // line is deploy-and-visit's with ` AND (MIT OR Apache-2.0)` after it.
    let own: Vec<String> =
        std::fs::read_to_string(root().join("scenarios/skeleton/deploy-and-visit.playbook.jsonc"))
            .expect("the committed scenario playbook")
            .lines()
            .take(2)
            .map(str::to_owned)
            .collect();
    let scenario_header = format!(
        "{}\n{} AND (MIT OR Apache-2.0)\n",
        own.first().expect("a copyright line"),
        own.get(1).expect("a licence line")
    );
    assert!(
        header.starts_with(&scenario_header) && header.lines().all(|line| line.starts_with("//")),
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
///
/// The corpus shows a Generator too (`GENERATOR_ROWS`, asserted in
/// `the_safe_playbook_raises_only_what_is_at_risk_unforced`), so item 113
/// (9)'s "rides the corpus where it can" holds for the kW figure. This match
/// stays because the lane's brief asks for the rise on the scenario's own
/// setup, and because it is the only check that the committed scenario
/// playbook is Easy's live answer for seat 0.
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

/// S2's `opra` through the real gateway and verifier: under casual fog every
/// seat sees the other's core and commander in its opening Lull, so Easy's
/// advisor offers a raid on a fixed target -- an `e_NN` the seat's own
/// listing gives, or an enemy commander by its seat -- staged at its post's
/// site, and the Raid template filled with it qualifies FULL (E0414, E0418
/// and E0419 among the checks) for both seats.
#[test]
fn a_suggested_raid_qualifies_through_the_real_verifier() {
    let rules = rules_json();
    let mut surface = open_under(
        &match_id("raid"),
        SCENARIO_SEED,
        2,
        &rules,
        3,
        FogPolicy::casual(),
    );
    for seat in [0_u8, 1] {
        let token = in_process_token(&mut surface, seat);
        let (advice, _) = advise_unforced(&mut surface, &token, seat, &rules);
        let raid = advice
            .suggestions
            .iter()
            .find(|suggestion| suggestion.template_id == "raid_an_enemy_beacon")
            .expect("a Raid suggestion");
        let target = raid
            .parameters
            .iter()
            .find(|value| value.pointer.ends_with("/attack/target"))
            .unwrap_or_else(|| panic!("seat {seat} is offered a raid: {raid:#?}"));
        assert!(
            target.value.contains("\"beacon_id\":\"e_")
                || target.value.contains("\"enemy_commander\":{\"seat\":"),
            "a fixed target: {}",
            target.value
        );
        let parameters: Vec<Json> = raid
            .parameters
            .iter()
            .map(|value| {
                Json::Object(vec![
                    (String::from("name"), Json::String(value.pointer.clone())),
                    (String::from("value"), Json::String(value.value.clone())),
                ])
            })
            .collect();
        let made = result(
            &call(
                &mut surface,
                &token,
                "instantiate_template",
                Json::Object(vec![
                    (
                        String::from("template_id"),
                        Json::String(String::from("raid_an_enemy_beacon")),
                    ),
                    (String::from("parameters"), Json::Array(parameters)),
                ]),
            ),
            "instantiate_template",
        );
        let playbook = made
            .get("playbook_jsonc")
            .cloned()
            .expect("an instantiated playbook");
        let verified = result(
            &call(
                &mut surface,
                &token,
                "verify_plan",
                Json::Object(vec![
                    (String::from("playbook_jsonc"), playbook),
                    (String::from("depth"), Json::String(String::from("full"))),
                ]),
            ),
            "verify_plan",
        );
        let report = verified.get("report").expect("a report");
        assert_eq!(
            report.get("qualifies"),
            Some(&Json::Bool(true)),
            "seat {seat}'s raid on {}: {report:?}",
            target.value
        );
    }
}

/// Probation's own seed (decisions-log items 154 (1) and 156 (1)): its rich
/// features lie beyond the first sphere, which no `covering` site reaches, so
/// Easy reaches toward them with stepping stones (S3's `opr3`; the register's
/// S5-04), each worth the goal it serves discounted by the stones and rounds
/// its chain still needs (fix pass 2, item 2), in its own seal, verified by
/// the real gateway.
#[test]
fn easy_reaches_past_its_first_sphere_with_a_stone_on_probations_seed() {
    let rules = rules_json();
    // Hosted as the lobby hosts Probation: two seats and the rules table's own
    // segment ladder, every seat Easy here.
    let host = Host::open_from(
        &rules,
        PROBATION_SEED,
        2,
        &Settings {
            segment_lengths_ms: Vec::new(),
            round_limit: 3,
        },
        Some(root().join(LIBRARY_PATH)),
    )
    .expect("a match");
    let table = RulesTable::from_canonical_json(&rules).expect("a rules table");
    let mut surface = Surface::new(
        &match_id("probation-stones"),
        PROBATION_SEED,
        table,
        FogPolicy::fogged(),
        &seat_ids(2),
    )
    .expect("a surface");
    surface.attach(host).expect("attached");
    surface.open_lull().expect("the opening Lull");
    let mut easy = all_built_in(&mut surface, 2, &rules);
    let mut rows: Vec<Row> = Vec::new();
    for round in 1..=3 {
        rows.extend(visit(
            &format!("probation/round-{round}"),
            &mut surface,
            2,
            &rules,
            &mut easy,
        ));
        assert!(surface.begin_push().expect("the Push opens"));
        while let Some(report) = surface.step().expect("a tick") {
            if report.segment_ended {
                break;
            }
        }
        surface.end_recap().expect("the recap closes");
        if round < 3 {
            surface.open_lull().expect("the next Lull");
        }
    }
    // (round, seat, the far feature) of every stone, read from the seal's
    // note, and every placement covering a feature a stone walked toward.
    let toward = |row: &Row| {
        row.note
            .split("A stepping-stone beacon toward the ")
            .nth(1)
            .and_then(|rest| rest.split(',').next())
            .and_then(|words| words.split(' ').next_back())
            .map(str::to_owned)
    };
    let stones: Vec<(String, u8, String)> = rows
        .iter()
        .filter(|row| row.seal == Seal::Own)
        .filter_map(|row| toward(row).map(|feature| (row.snapshot.clone(), row.seat, feature)))
        .collect();
    for row in &rows {
        eprintln!("{} seat {}: {}", row.snapshot, row.seat, row.note);
    }
    eprintln!("Probation's seed, the stones (round, seat, toward): {stones:?}");
    // A named expectation, each round of each seat's stone chain (fix pass
    // 3, items 1 and 2: a stone charged its own price, rates compared
    // exactly). Both seats place the Generator on their start vent in round
    // 1. Seat 0 walks one stone toward the rich seam `seam_190_124` in round
    // 2, and in round 3 the stone's sphere brings two vents into reach, which
    // it covers, the standard `vent_212_109` and one more; the seam is not yet
    // in reach. Seat 1, which sealed its safe playbook here before item 1,
    // walks stones toward the rich seam `seam_163_202` in rounds 2 and 3.
    let wanted: Vec<(String, u8, String)> = [
        ("probation/round-2", 0, "seam_190_124"),
        ("probation/round-2", 1, "seam_163_202"),
        ("probation/round-3", 1, "seam_163_202"),
    ]
    .into_iter()
    .map(|(round, seat, feature)| (round.to_owned(), seat, feature.to_owned()))
    .collect();
    assert_eq!(stones, wanted, "{rows:#?}");
    assert!(
        rows.iter().any(|row| row.snapshot == "probation/round-3"
            && row.seat == 0
            && row
                .note
                .contains("A Build beacon covering the standard heat vent vent_212_109")),
        "seat 0's stone opens the ground its round 3 builds on: {rows:#?}"
    );
    assert!(
        rows.iter().all(|row| row.seal != Seal::NotEasy),
        "every seal is Easy's own, the verifier's accepted: {rows:#?}"
    );
}

/// One hosted match on the committed seeds for the Survey post's tests:
/// every seat Easy, the rules table's own ladder, `rounds` rounds. Per seat,
/// the rounds whose seal places a Survey post, the round of its first
/// `enemy_beacon_found`.
struct Surveyed {
    posts: Vec<(u32, u8)>,
    first_found: Vec<(u8, u32)>,
}

fn survey_match(seed: u64, seats: u32, rounds: u32) -> Surveyed {
    use pharmakos_gateway::fog::Audience;
    let rules = rules_json();
    let host = Host::open_from(
        &rules,
        seed,
        seats,
        &Settings {
            segment_lengths_ms: Vec::new(),
            round_limit: rounds,
        },
        Some(root().join(LIBRARY_PATH)),
    )
    .expect("a match");
    let table = RulesTable::from_canonical_json(&rules).expect("a rules table");
    let mut surface = Surface::new(
        &match_id(&format!("survey-{seed:x}-{seats}-{rounds}")),
        seed,
        table,
        FogPolicy::fogged(),
        &seat_ids(seats),
    )
    .expect("a surface");
    surface.attach(host).expect("attached");
    surface.open_lull().expect("the opening Lull");
    let mut easy = all_built_in(&mut surface, seats, &rules);
    let mut out = Surveyed {
        posts: Vec::new(),
        first_found: Vec::new(),
    };
    for round in 1..=rounds {
        for row in visit(
            &format!("survey-{seed:x}/round-{round}"),
            &mut surface,
            seats,
            &rules,
            &mut easy,
        ) {
            if row.seal == Seal::Own && row.note.contains("A Survey beacon beside the core") {
                out.posts.push((round, row.seat));
            }
        }
        assert!(surface.begin_push().expect("the Push opens"));
        let mut read = 0_usize;
        while let Some(report) = surface.step().expect("a tick") {
            let events = surface.feed().events();
            if events.len() < read {
                read = 0;
            }
            for event in events.get(read..).unwrap_or_default() {
                if event.kind.to_string() != "enemy_beacon_found" {
                    continue;
                }
                let owner = match event.audience {
                    Audience::Private(seat) => Some(seat),
                    Audience::World { owner, .. } => owner,
                    Audience::Public => None,
                };
                if let Some(seat) = owner
                    && !out.first_found.iter().any(|(held, _)| *held == seat.raw())
                {
                    out.first_found.push((seat.raw(), round));
                }
            }
            read = events.len();
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

/// The committed seeds the Survey post is measured on, as `(seed, seats)`.
const SURVEY_MATCHES: [(u64, u32); 5] = [
    (PROBATION_SEED, 2),
    (SEED, 2),
    (SEED, 3),
    (SCENARIO_SEED, 2),
    (SCENARIO_SEED, 3),
];

/// `(Survey posts sealed in rounds 1 to 4, of them finding an enemy beacon
/// by the end of the next round's Push)` on [`SURVEY_MATCHES`] over five
/// rounds.
///
/// Moved from `(11, 9)` by lane `srvy` (PR #210, the Survey scouts' refresh
/// of held sightings before the next Lull): on seed `0x0102030405060708` at
/// two seats, seat 0 posts in round 4 as before and now finds an enemy beacon
/// by round 5's Push, because its scouts leave their scan to re-check the
/// sightings the seat holds and see the beacon on that walk. The posts are
/// unchanged.
const SURVEY_POSTS: (usize, usize) = (11, 10);

/// Spec section 14's Survey post through the real gateway (S3's plan,
/// decision 3), looking over the half of the ground away from the seat's core
/// and **ranked** with every other candidate, worth the raid it buys (fix pass
/// 2, item 2): so it goes up once the seat's first Generator is placed and the
/// treasury carries it, not ahead of it, and since fix pass 3 a stone charged
/// its own price outranks it while far features are left to reach (the posts
/// now go up in round 4, 11 of 12 seats). On the
/// committed seeds, every seat Easy, over five rounds: how many seats seal a
/// post in rounds 1 to 4, and how many of those have their scouts find an
/// enemy beacon by the end of the next round's Push. A named expectation with
/// its counts; a change says which seat and seed moved.
///
/// The owner's reading of the spec's "refresh those sightings before the next
/// Lull" (2026-11-22, "Refresh only (Recommended)"): it covers sightings
/// already held, and a first discovery may take longer, so this test pins
/// what a first discovery takes and does not require it inside one Push.
#[test]
fn survey_posts_go_up_once_the_first_generator_stands() {
    let mut posted = 0_usize;
    let mut found = 0_usize;
    for (seed, seats) in SURVEY_MATCHES {
        let surveyed = survey_match(seed, seats, 5);
        eprintln!(
            "seed {seed:#x}/{seats}: posts (round, seat) {:?}; first enemy found (seat, round) {:?}",
            surveyed.posts, surveyed.first_found
        );
        for &(round, seat) in surveyed.posts.iter().filter(|(round, _)| *round <= 4) {
            posted = posted.checked_add(1).expect("a count");
            if surveyed
                .first_found
                .iter()
                .any(|&(held, by)| held == seat && by <= round + 1)
            {
                found = found.checked_add(1).expect("a count");
            }
        }
    }
    eprintln!(
        "Survey posts in rounds 1 to 4: {posted}, finding the enemy by the next round: {found}"
    );
    assert_eq!((posted, found), SURVEY_POSTS, "the named expectation moved");
}

/// The Resonance Spire through the real verifier and the sim (fix pass 1,
/// item 4). No hosted seat of Easy's earns the licence within the suite's
/// rounds, so seat 0 earns it as `scenarios/s3/toward-stepping-stone` does:
/// on Probation's seed it seals that scenario's playbook in rounds 1 and 2
/// (a beacon on its start vent, then a stone toward the second vent and a
/// beacon on it), and round 2's settlement awards the licence. From round 3
/// Easy plays seat 0 through the seat's own token: its round-3 seal queues
/// the Spire at the core and the real verifier accepts it, the sim queues the
/// structure exactly once in that Push, and round 4's seal queues none.
fn spire_match(rules: &str) -> SpireMatch {
    let spire_draw_kw = rules_row(rules, &["structures", "resonance_spire", "draw_kw"]);
    let kw_per_unit = rules_row(rules, &["power", "kw_per_unit"]);
    let rules = rules.to_owned();
    let host = Host::open_from(
        &rules,
        PROBATION_SEED,
        2,
        &Settings {
            segment_lengths_ms: Vec::new(),
            round_limit: 4,
        },
        Some(root().join(LIBRARY_PATH)),
    )
    .expect("a match");
    let table = RulesTable::from_canonical_json(&rules).expect("a rules table");
    let mut surface = Surface::new(
        &match_id("hosted-spire"),
        PROBATION_SEED,
        table,
        FogPolicy::fogged(),
        &seat_ids(2),
    )
    .expect("a surface");
    surface.attach(host).expect("attached");
    surface.open_lull().expect("the opening Lull");
    let mut factory = EasyOperators::new(&rules).expect("the operator reads the rules text");
    let mut seats = InProcessSeats::open(&mut surface, &seat_ids(2), Some(0), &mut factory)
        .expect("in-process seats");
    let token = in_process_token(&mut surface, 0);
    let stone_playbook =
        std::fs::read_to_string(root().join("scenarios/s3/toward-stepping-stone.playbook.jsonc"))
            .expect("the scenario's playbook");
    let mut easy = Easy::new(&rules).expect("the rules text");
    let mut queued: Vec<usize> = Vec::new();
    let mut headrooms: Vec<i64> = Vec::new();
    let mut sealed_texts: Vec<String> = Vec::new();
    for round in 1..=4_u32 {
        seats.plan(&mut surface);
        if round <= 2 {
            let submitted = result(
                &call(
                    &mut surface,
                    &token,
                    "submit_plan",
                    Json::Object(vec![(
                        String::from("playbook_jsonc"),
                        Json::String(stone_playbook.clone()),
                    )]),
                ),
                "submit_plan",
            );
            assert_eq!(submitted.get("accepted"), Some(&Json::Bool(true)));
        } else {
            let forecast = result(
                &call(&mut surface, &token, "get_economy_forecast", params("{}")),
                "get_economy_forecast",
            );
            headrooms.push(number(&forecast, "headroom_kw_now"));
            let played = {
                let mut bridged =
                    |method: &str, params: Json| call(&mut surface, &token, method, params);
                easy.play(0, &mut bridged)
            };
            assert_eq!(
                played.submitted,
                pharmakos_operator::Submitted::Own,
                "round {round}: {}",
                played.why
            );
            sealed_texts.push(played.playbook_jsonc.unwrap_or_default());
        }
        assert!(surface.begin_push().expect("the Push opens"));
        while let Some(report) = surface.step().expect("a tick") {
            if report.segment_ended {
                break;
            }
        }
        // Seat 0's Resonance Spires, standing or going up, as the sim's table
        // holds them after the Push.
        let spires =
            pharmakos_sim::spire::facts_of(surface.host().expect("hosted").world().structures())
                .filter(|facts| {
                    !facts.free
                        && facts.alive
                        && facts.seat == 0
                        && facts.kind == pharmakos_sim::tables::StructureKind::ResonanceSpire.id()
                })
                .count();
        queued.push(spires);
        surface.end_recap().expect("the recap closes");
        if round < 4 {
            surface.open_lull().expect("the next Lull");
        }
    }
    SpireMatch {
        sealed_texts,
        headrooms,
        queued,
        spire_draw_kw,
        kw_per_unit,
    }
}

/// What [`spire_match`] saw: Easy's seals for seat 0 in rounds 3 and 4, the
/// headroom the forecast showed before each, and seat 0's Spires after each
/// Push.
struct SpireMatch {
    sealed_texts: Vec<String>,
    headrooms: Vec<i64>,
    queued: Vec<usize>,
    spire_draw_kw: i64,
    kw_per_unit: i64,
}

impl SpireMatch {
    /// The new draw a seal adds, at the rows every grid charges (fix pass 2,
    /// item 4): the Spire's own `draw_kw`, and each scout the seal orders (a
    /// Survey post's `scout_count`, a guard's `picket_scouts`) at
    /// `power.kw_per_unit`.
    fn drawn(&self, seal: &str) -> i64 {
        i64::from(seal.contains("queue_spire")) * self.spire_draw_kw
            + scouts_ordered(seal) * self.kw_per_unit
    }
}

/// Every `scout_count` and `picket_scouts` a sealed playbook sets, summed.
fn scouts_ordered(seal: &str) -> i64 {
    fn set(value: &Json) -> i64 {
        match value {
            Json::Object(members) => members
                .iter()
                .map(|(key, held)| match (key.as_str(), held) {
                    ("scout_count" | "picket_scouts", Json::Number(lexeme)) => {
                        lexeme.parse::<i64>().expect("a count")
                    }
                    _ => set(held),
                })
                .sum(),
            Json::Array(items) => items.iter().map(set).sum(),
            _ => 0,
        }
    }
    set(&pharmakos_plan_core::Document::parse(seal)
        .expect("a sealed playbook reads")
        .to_json())
}

/// A whole-number row of a rules text, by its path of keys.
fn rules_row(rules: &str, path: &[&str]) -> i64 {
    let table = params(rules);
    let row = path
        .iter()
        .try_fold(&table, |at, key| at.get(key))
        .unwrap_or_else(|| panic!("the rules text has no `{}` row", path.join(".")));
    match row {
        Json::Number(lexeme) => lexeme.parse().expect("a whole number"),
        other => panic!("`{}` is no number: {other:?}", path.join(".")),
    }
}

#[test]
fn easy_queues_the_spire_once_through_the_real_verifier_and_the_sim() {
    let spire = spire_match(&rules_json());
    let SpireMatch {
        sealed_texts,
        headrooms,
        queued,
        ..
    } = &spire;
    let [third, fourth] = sealed_texts.as_slice() else {
        panic!("Easy sealed rounds 3 and 4: {sealed_texts:#?}");
    };
    // One kW budget (fix pass 1, item 1): what the seal adds -- the Spire's
    // draw, and the scouts a Survey post and a guard's picket order, at the
    // rules rows -- is never more than the headroom the forecast showed, and
    // the real verifier accepted it.
    for (seal, headroom) in sealed_texts.iter().zip(headrooms) {
        eprintln!(
            "headroom {headroom} kW: Spire {}, Survey post {}, guard {}",
            seal.contains("queue_spire"),
            seal.contains("survey_post"),
            seal.contains("place_guard")
        );
        let drawn = spire.drawn(seal);
        assert!(
            drawn <= *headroom,
            "{drawn} kW of new draw against {headroom} kW: {seal}"
        );
    }
    assert!(
        third.contains("queue_spire"),
        "round 3 queues the Spire: {third}"
    );
    assert!(
        !fourth.contains("queue_spire"),
        "round 4 queues none: {fourth}"
    );
    assert_eq!(
        queued.as_slice(),
        [0, 0, 1, 1],
        "seat 0's Spires after each Push: none, then the one queued in round 3, and no second"
    );
}

/// The kW budget at its margin through the real verifier (fix pass 1, item
/// 1): the same match on a rules text whose Spire draws all of round 3's
/// headroom but 1 kW. Easy still queues it, nothing else it adds draws past
/// the 1 kW left, and the real verifier accepts the seal (no E0601). The
/// scripted `one_kw_budget_never_composes_more_draw_than_the_headroom`
/// sweeps every headroom with the Spire, the Survey post and the guard.
#[test]
fn a_spire_at_the_margin_of_the_headroom_verifies_clean_through_the_real_verifier() {
    let base = rules_json();
    let probe = spire_match(&base);
    let headroom = *probe.headrooms.first().expect("round 3's headroom");
    let draw = headroom.checked_sub(1).expect("a draw");
    let row = "\"resonance_spire\": {\n      \"cost_dollars\": 120,\n      \"hp\": 500,\n      \"draw_kw\": 3,";
    let tight = base.replace(
        row,
        &row.replace("\"draw_kw\": 3,", &format!("\"draw_kw\": {draw},")),
    );
    assert_ne!(tight, base, "the rules text carries the Spire's row");
    let tight_match = spire_match(&tight);
    let third = tight_match.sealed_texts.first().expect("round 3's seal");
    let tight_headroom = *tight_match.headrooms.first().expect("round 3's headroom");
    assert!(third.contains("queue_spire"), "the Spire fits: {third}");
    assert!(
        tight_match.drawn(third) <= tight_headroom,
        "{} kW against {tight_headroom} kW",
        tight_match.drawn(third)
    );
}

/// A playbook whose one step is a stone toward `seam_190_124` with a timeout
/// of `timeout_ms`, for [`failed_stone_match`].
fn timed_stone_playbook(timeout_ms: i64) -> String {
    format!(
        r#"{{
  "schema_version": {{"major": 1, "minor": 0}},
  "meta": {{"title": "A stone that times out", "author_kind": "HUMAN"}},
  "kind": "PLAYBOOK",
  "declarative": {{
    "route": [
      {{"label": "stone",
        "place_beacon": {{"at": {{"toward": {{"feature": {{"feature_id": "seam_190_124"}}}}}}}},
        "timeout_ms": {timeout_ms}, "on_fail": {{"action": "SKIP"}}}}
    ],
    "handlers": []
  }},
  "on_death": {{"on_respawn": "CONTINUE", "max_deaths_before_fallback": 2}},
  "fallback": {{"hold": {{"at": {{"safest": {{}}}}}}}}
}}
"#
    )
}

/// A Probation match, two seats, three rounds, opened to its first Lull, with
/// seat 1 as Easy's built-in seat and seat 0 advised: what the failed-stone
/// tests play seat 0 in.
fn failed_stone_surface(name: &str, rules: &str) -> (Surface, InProcessSeats) {
    let host = Host::open_from(
        rules,
        PROBATION_SEED,
        2,
        &Settings {
            segment_lengths_ms: Vec::new(),
            round_limit: 3,
        },
        Some(root().join(LIBRARY_PATH)),
    )
    .expect("a match");
    let table = RulesTable::from_canonical_json(rules).expect("a rules table");
    let mut surface = Surface::new(
        &match_id(name),
        PROBATION_SEED,
        table,
        FogPolicy::fogged(),
        &seat_ids(2),
    )
    .expect("a surface");
    surface.attach(host).expect("attached");
    surface.open_lull().expect("the opening Lull");
    let mut factory = EasyOperators::new(rules).expect("the operator reads the rules text");
    let seats = InProcessSeats::open(&mut surface, &seat_ids(2), Some(0), &mut factory)
        .expect("in-process seats");
    (surface, seats)
}

/// Play the Push after the seals to its end, close its recap and open the
/// next Lull.
fn play_to_next_lull(surface: &mut Surface) {
    assert!(surface.begin_push().expect("the Push opens"));
    while let Some(report) = surface.step().expect("a tick") {
        if report.segment_ended {
            break;
        }
    }
    surface.end_recap().expect("the recap closes");
    surface.open_lull().expect("the next Lull");
}

/// Easy plays seat 0's round through the seat's own token, every
/// `submit_plan` passed through `submitted` on its way: the `toward`
/// features of its estimates, in order, and its round.
fn easy_plays_seat_zero(
    surface: &mut Surface,
    easy: &mut Easy,
    submitted: &dyn Fn(&str) -> String,
) -> (Vec<String>, pharmakos_operator::Played) {
    let token = in_process_token(surface, 0);
    let mut toward: Vec<String> = Vec::new();
    let played = {
        let mut bridged = |method: &str, params: Json| {
            if method == "estimate_route"
                && let Some(Json::Array(waypoints)) = params.get("waypoints")
                && let Some(Json::String(feature)) = waypoints
                    .last()
                    .and_then(|last| last.get("toward"))
                    .and_then(|toward| toward.get("feature"))
                    .and_then(|feature| feature.get("feature_id"))
            {
                toward.push(feature.clone());
            }
            let params = match (method, params.get("playbook_jsonc")) {
                ("submit_plan", Some(Json::String(text))) => Json::Object(vec![(
                    String::from("playbook_jsonc"),
                    Json::String(submitted(text)),
                )]),
                _ => params,
            };
            call(surface, &token, method, params)
        };
        easy.play(0, &mut bridged)
    };
    (toward, played)
}

/// The seal with every stone step's `timeout_ms` set to `ms`: Easy's own
/// labels (`stone`, `stone_2`, ...) and its compact step spelling.
fn stones_timed_to(text: &str, ms: i64) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("\"label\":\"stone") {
        let (head, tail) = rest.split_at(at);
        out.push_str(head);
        let key = "\"timeout_ms\":";
        let start = tail.find(key).expect("a stone step has a timeout") + key.len();
        let digits = tail
            .get(start..)
            .expect("the timeout")
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        out.push_str(tail.get(..start).expect("up to the timeout"));
        out.push_str(&ms.to_string());
        rest = tail.get(start + digits..).expect("after the timeout");
    }
    out.push_str(rest);
    out
}

/// One Probation match in which Easy itself seals a stone and that stone
/// fails (fix pass 4, item 1): Easy plays seat 0 in rounds 1 to 3 through the
/// seat's own token. Round 2's seal places a stone toward `seam_190_124` (the
/// Probation table's stones); with `stone_ms` set, its timeout is cut to that
/// on the way to `submit_plan`, so it fails `timeout` in round 2's Push. No
/// test writes a draft: what round 3 reads back is the gateway's carried
/// draft. Answers round 2's seal, and round 3's `toward` estimates and round.
fn easy_seals_a_stone(stone_ms: Option<i64>) -> (String, Vec<String>, pharmakos_operator::Played) {
    let rules = rules_json();
    let name = match stone_ms {
        Some(ms) => format!("easy-stone-{ms}"),
        None => String::from("easy-stone-as-composed"),
    };
    let (mut surface, mut seats) = failed_stone_surface(&name, &rules);
    let mut easy = Easy::new(&rules).expect("the rules text");
    let as_composed = |text: &str| text.to_owned();
    // Round 1: the start vent's Generator.
    seats.plan(&mut surface);
    let (_, first) = easy_plays_seat_zero(&mut surface, &mut easy, &as_composed);
    assert_eq!(first.submitted, Submitted::Own, "{}", first.why);
    play_to_next_lull(&mut surface);
    // Round 2: the stone, its timeout cut when asked.
    seats.plan(&mut surface);
    let cut = |text: &str| stone_ms.map_or_else(|| text.to_owned(), |ms| stones_timed_to(text, ms));
    let (_, second) = easy_plays_seat_zero(&mut surface, &mut easy, &cut);
    assert_eq!(second.submitted, Submitted::Own, "{}", second.why);
    let sealed = surface
        .seat_state(Subject::Seat(SeatId::new(0)), SeatId::new(0))
        .expect("its own")
        .sealed()
        .expect("round 2's seal")
        .playbook_jsonc()
        .to_owned();
    assert!(
        sealed.contains("\"toward\":{\"feature\":{\"feature_id\":\"seam_190_124\"}}"),
        "{sealed}"
    );
    play_to_next_lull(&mut surface);
    // Round 3: what Easy weighs.
    seats.plan(&mut surface);
    let (toward, third) = easy_plays_seat_zero(&mut surface, &mut easy, &as_composed);
    let drafts = surface
        .seat_state(Subject::Seat(SeatId::new(0)), SeatId::new(0))
        .expect("its own")
        .drafts
        .iter()
        .map(|draft| draft.draft_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        drafts,
        [pharmakos_operator::easy::CARRIED_DRAFT_ID],
        "the seat's only draft is the one the gateway carried"
    );
    (sealed, toward, third)
}

/// A stone Easy itself sealed, which failed, is not weighed again unchanged
/// (fix pass 4, item 1): round 3 reads the failed step from its feed and the
/// stone from the gateway's carried draft, and makes no `toward` estimate for
/// `seam_190_124`. Its control, with the timeout Easy composed, places the
/// stone, and round 3 says no stone failed and weighs the seam's chain.
#[test]
fn a_stone_easy_sealed_that_failed_is_not_weighed_again() {
    let (sealed, toward, played) = easy_seals_a_stone(Some(1_000));
    eprintln!("timed out: toward {toward:?}; {}", played.why);
    assert!(sealed.contains("\"timeout_ms\":1000"), "{sealed}");
    assert_eq!(
        played.failed_stones,
        FailedStones::Known(vec![String::from("seam_190_124")]),
        "{}",
        played.why
    );
    assert!(
        !toward.iter().any(|feature| feature == "seam_190_124"),
        "{toward:?}"
    );
    let (_, toward, played) = easy_seals_a_stone(None);
    eprintln!("placed: toward {toward:?}; {}", played.why);
    assert!(
        toward.iter().any(|feature| feature == "seam_190_124"),
        "{toward:?}"
    );
    assert!(
        played.failed_stones.known().is_empty()
            && !matches!(played.failed_stones, FailedStones::Unread { .. }),
        "{:?}",
        played.failed_stones
    );
}

/// The operator's restatement of the gateway's carried draft id is the
/// gateway's.
#[test]
fn the_carried_draft_id_is_the_gateways() {
    assert_eq!(
        pharmakos_operator::easy::CARRIED_DRAFT_ID,
        pharmakos_gateway::surface::CARRIED_DRAFT_ID
    );
}

/// One Probation match for a failed stone a person sealed (fix pass 3, item
/// 6): seat 1 is Easy's built-in seat; seat 0 seals, in round 1, a playbook
/// whose one step is a stone toward `seam_190_124` with a timeout of
/// `timeout_ms`. The gateway carries that seal into round 2 as the seat's
/// draft, as it carries every seat's own seal. In round 2 Easy plays seat 0
/// through the seat's own token: the `toward` waypoints of its estimates, by
/// feature, and its round are what it answers.
fn failed_stone_match(timeout_ms: i64) -> (Vec<String>, pharmakos_operator::Played) {
    let rules = rules_json();
    let (mut surface, mut seats) =
        failed_stone_surface(&format!("failed-stone-{timeout_ms}"), &rules);
    let stone = timed_stone_playbook(timeout_ms);
    // Round 1: seat 0 seals the stone.
    seats.plan(&mut surface);
    let token = in_process_token(&mut surface, 0);
    let submitted = result(
        &call(
            &mut surface,
            &token,
            "submit_plan",
            Json::Object(vec![(String::from("playbook_jsonc"), Json::String(stone))]),
        ),
        "submit_plan",
    );
    assert_eq!(submitted.get("accepted"), Some(&Json::Bool(true)));
    play_to_next_lull(&mut surface);
    // Round 2: Easy plays seat 0 through its own token.
    let mut easy = Easy::new(&rules).expect("the rules text");
    easy_plays_seat_zero(&mut surface, &mut easy, &|text: &str| text.to_owned())
}

/// A stone that failed last round is not weighed again (fix pass 3, item 6),
/// hosted: a stone toward `seam_190_124` with a 1 s timeout fails `timeout`
/// in round 1's Push, and round 2's Easy, reading its own feed and the
/// gateway's carried draft, weighs no stone toward that seam. With a timeout
/// the walk fits, the stone places, and round 2 weighs the chain's next
/// stone toward it.
#[test]
fn a_stone_made_to_time_out_is_not_weighed_again() {
    let (toward, played) = failed_stone_match(1_000);
    eprintln!("timed out: toward {toward:?}; {}", played.why);
    assert!(
        !toward.iter().any(|feature| feature == "seam_190_124"),
        "{toward:?}"
    );
    assert_eq!(
        played.failed_stones,
        FailedStones::Known(vec![String::from("seam_190_124")]),
        "{}",
        played.why
    );
    let (toward, played) = failed_stone_match(240_000);
    eprintln!("placed: toward {toward:?}; {}", played.why);
    assert!(
        toward.iter().any(|feature| feature == "seam_190_124"),
        "{toward:?}"
    );
    assert_eq!(played.failed_stones, FailedStones::None, "{}", played.why);
}

/// Round 2 of [`failed_stone_match`]'s match twice from one save (fix pass 4,
/// item 2): its Lull saved, Easy plays seat 0 on the live surface, then on a
/// fresh surface resumed from the save. Answers the live round and the
/// resumed one.
fn round_two_live_and_resumed() -> (pharmakos_operator::Played, pharmakos_operator::Played) {
    let rules = rules_json();
    let name = "resumed-stone";
    let (mut surface, mut seats) = failed_stone_surface(name, &rules);
    seats.plan(&mut surface);
    let token = in_process_token(&mut surface, 0);
    let submitted = result(
        &call(
            &mut surface,
            &token,
            "submit_plan",
            Json::Object(vec![(
                String::from("playbook_jsonc"),
                Json::String(timed_stone_playbook(1_000)),
            )]),
        ),
        "submit_plan",
    );
    assert_eq!(submitted.get("accepted"), Some(&Json::Bool(true)));
    play_to_next_lull(&mut surface);
    let saved = surface.lull_save().expect("a Lull saves");
    let as_composed = |text: &str| text.to_owned();
    let mut easy = Easy::new(&rules).expect("the rules text");
    let (_, live) = easy_plays_seat_zero(&mut surface, &mut easy, &as_composed);
    let host = Host::open_from(
        &rules,
        PROBATION_SEED,
        2,
        &Settings {
            segment_lengths_ms: Vec::new(),
            round_limit: 3,
        },
        Some(root().join(LIBRARY_PATH)),
    )
    .expect("a match");
    let table = RulesTable::from_canonical_json(&rules).expect("a rules table");
    let mut resumed = Surface::new(
        &match_id(name),
        PROBATION_SEED,
        table,
        FogPolicy::fogged(),
        &seat_ids(2),
    )
    .expect("a surface")
    .resume(
        host,
        &saved,
        pharmakos_gateway::save::Continuation {
            generation: 1,
            last_tick: 0,
        },
    )
    .expect("the Lull resumes");
    let mut easy = Easy::new(&rules).expect("the rules text");
    let (_, again) = easy_plays_seat_zero(&mut resumed, &mut easy, &as_composed);
    (live, again)
}

/// A resumed Lull reads the last Push as unread, and says so (fix pass 4,
/// item 2): the live round knows its stone toward `seam_190_124` failed; the
/// same round resumed from a save, whose feed shows no Push, knows nothing of
/// it, and its round says so in a typed state and in its "why", never as
/// "nothing failed".
#[test]
fn a_resumed_lull_reads_the_last_push_as_unread() {
    let (live, resumed) = round_two_live_and_resumed();
    assert_eq!(
        live.failed_stones,
        FailedStones::Known(vec![String::from("seam_190_124")]),
        "{}",
        live.why
    );
    assert_eq!(
        resumed.failed_stones,
        FailedStones::Unread {
            known: Vec::new(),
            why: StonesUnread::Push(PushUnread::Resumed),
        },
        "{}",
        resumed.why
    );
    assert!(
        resumed.why.contains("not in this Lull's feed"),
        "{}",
        resumed.why
    );
}

/// The review's probe P1(b) (fix pass 4, item 2): a resumed Lull seals what
/// the unresumed one does. It cannot until the save carries the last Push's
/// step outcomes and deaths, which the owner approved on 2026-11-23 and lane
/// `opsave` builds; this is the red test that lane turns green.
#[test]
#[ignore = "red until lane opsave: the save carries the last Push's step outcomes and deaths (owner, 2026-11-23)"]
fn a_resumed_lull_seals_what_the_unresumed_one_does() {
    let (live, resumed) = round_two_live_and_resumed();
    assert_eq!(
        live.playbook_jsonc, resumed.playbook_jsonc,
        "a resumed Lull seals what the unresumed one does"
    );
}
