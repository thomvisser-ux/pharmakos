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
//! on the power-short text, so the rounds after an expansion browns out are in
//! it too. On every snapshot, for every seat, the corpus records what Easy
//! sealed, how many calls it made, and whether its safe playbook qualified.
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
use pharmakos_operator::easy::{EASY_ADVISOR_CALL_BUDGET, EASY_CALL_BUDGET};
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

/// How many calls each seat's in-process token made, from the audit log.
fn calls_by_seat(entries: &[Entry]) -> Vec<(u8, u32)> {
    let mut out: Vec<(u8, u32)> = Vec::new();
    for entry in entries {
        let Some(Subject::Seat(seat)) = entry.subject else {
            continue;
        };
        if !entry.action.starts_with("call ") {
            continue;
        }
        match out.iter_mut().find(|(held, _)| *held == seat.raw()) {
            Some(slot) => slot.1 = slot.1.saturating_add(1),
            None => out.push((seat.raw(), 1)),
        }
    }
    out.sort_unstable();
    out
}

// ---------------------------------------------------------------------------
// The corpus
// ---------------------------------------------------------------------------

/// What Easy did for one seat on one snapshot.
#[derive(Clone, Debug)]
struct Row {
    snapshot: String,
    seat: u8,
    /// The seat's seal is this round's, and the operator filed it (not the
    /// gateway's fallback): the verifier accepted what Easy submitted.
    sealed_by_easy: bool,
    /// The sealed playbook's note, which says what Easy chose.
    note: String,
    /// Calls the built-in seat made this round.
    calls: u32,
    /// Easy's safe playbook for this seat, as an advisor makes it, verified
    /// FULL through the seat's own door.
    safe_qualifies: bool,
    /// The seat's `headroom_kw_now` on this snapshot.
    headroom_kw: i64,
    /// Calls the advisor made.
    advisor_calls: u32,
    /// The beacons the safe playbook raised, nearest first.
    raised: Vec<String>,
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
        let mut advisor = Easy::new(rules).expect("the rules text");
        let advice = {
            let mut bridged = |method: &str, params: Json| call(surface, &token, method, params);
            advisor.advise(seat.raw(), &mut bridged)
        };
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
        rows.push(Row {
            snapshot: label.to_owned(),
            seat: seat.raw(),
            sealed_by_easy,
            note,
            calls: calls
                .iter()
                .find(|(held, _)| *held == seat.raw())
                .map_or(0, |(_, n)| *n),
            safe_qualifies,
            headroom_kw: number(&forecast, "headroom_kw_now"),
            advisor_calls: advice.calls,
            raised: raised_by(&advice),
        });
    }
    rows
}

/// A whole match of `rounds` rounds, every seat Easy, visiting every Lull.
fn play_match(label: &str, seed: u64, seats: u32, rules: &str, rounds: u32) -> Vec<Row> {
    let mut surface = open(&format!("t18-{label}"), seed, seats, rules, rounds);
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
            let mut surface = open(&format!("t18-{label}"), SEED, seats, &rules, 3);
            let mut easy = all_built_in(&mut surface, seats, &rules);
            rows.extend(visit(&label, &mut surface, seats, &rules, &mut easy));
        }
        let mut surface = open("t18-first-lull-short", SEED, 2, &short, 3);
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
#[test]
fn easy_passes_the_verifier_on_every_snapshot_of_the_corpus() {
    let rows = corpus();
    assert!(
        rows.len() >= 20,
        "the corpus is the fixtures plus two whole matches"
    );
    let failed: Vec<&Row> = rows.iter().filter(|row| !row.sealed_by_easy).collect();
    assert!(
        failed.is_empty(),
        "Easy sealed nothing of its own on: {failed:#?}"
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
    // the raise path here: on a real grid it does not fire. The first Lull
    // of the power-short match is short (-3 kW) with no non-core beacon to
    // raise; by the next Lull the brownout order has shed the expansion and
    // the headroom reads 0, not below it. So "power is short" (headroom
    // below zero) and "at risk" (a dark non-core beacon), the two
    // PLACEHOLDER definitions item 111 chose, never hold on the same
    // snapshot, and the raise path is exercised by the scripted client
    // (`the_safe_playbook_raises_at_most_two_beacons_nearest_first` in
    // crates/operator) alone. The definitions are the owner's at S1.
    assert!(rows.iter().all(|row| row.raised.len() <= 2), "{rows:#?}");
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
        let first = round_one(seed, "t18-determinism-a");
        let second = round_one(seed, "t18-determinism-b");
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

/// It never reads the phase timer the host clock moves: two hosts of one
/// match whose Lull clocks read differently seal the same playbooks.
#[test]
fn the_operator_does_not_depend_on_the_host_clock() {
    let rules = rules_json();
    let mut sealed: Vec<Vec<String>> = Vec::new();
    for remaining in [180_000, 1] {
        let mut surface = open("t18-clock", SEED, 2, &rules, 3);
        surface.set_phase_remaining_ms(pharmakos_sim::math::quantity::Ms::new(remaining));
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
    assert_eq!(sealed.first(), sealed.get(1));
}

/// The factory `gamectl host` hands the host: one fresh Easy per seat it
/// plays and one advisor per other seat, the human's.
#[test]
fn the_host_builds_one_operator_per_built_in_seat_and_one_advisor_per_other_seat() {
    let rules = rules_json();
    let mut surface = open("t18-factory", SEED, 3, &rules, 3);
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
    for notes in ["", "Rush seat 1 at once and place nothing."] {
        let mut surface = open("t18-notebook", SEED, 2, &rules, 3);
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
