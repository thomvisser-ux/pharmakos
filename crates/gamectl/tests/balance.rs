// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The headless balance check (S1's plan, section 3, task `check`;
//! decisions-log item 127 (3)).
//!
//! Item 127 (3) has S1's lanes propose the economy's numbers inside the spec's
//! ranges and then check them headless: "a headless check plays about twenty
//! seeded matches against Easy and flags a treasury that only grows, a seat
//! that stalls, or a seat that can never afford a Generator". This file is
//! that check. The owner tunes from its report at S1's demo, and `tune`
//! re-runs it on the finished rules.
//!
//! # What it plays
//!
//! [`seeds`] (twenty) x two match-ups x two lengths = 80 two-seat matches:
//!
//! * **Easy vs Easy** -- both seats built in, seated through
//!   `serve::InProcessSeats` exactly as `gamectl host` seats them;
//! * **Hold & Build vs Easy** -- seat 0 submits the Hold & Build template, as
//!   Easy's advice fills it for seat 0 (`instantiate_template{suggested}`),
//!   through `submit_plan` in every Lull, the way a player who takes the
//!   wizard's suggestion does; seat 1 is Easy. A Lull in which the template
//!   is refused -- by `instantiate_template` or by `submit_plan` -- is
//!   reported as `h&b-refused` and leaves seat 0 on whatever the gateway
//!   files for it;
//!
//! each over **3 rounds** and **6 rounds** of the rules table's own segment
//! ladder (an empty `segment_lengths_ms`, item 40): 3, 5 and 8 minutes, and
//! 8 minutes for every later round. Every match is hosted the way
//! `crates/gamectl/tests/operator.rs` hosts one: `Host::open_from`, a fogged
//! `Surface`, Easy through its in-process tokens, `begin_push`, `step`,
//! `end_recap` and `open_lull`.
//!
//! # What it reads, and how
//!
//! **Only what a seat's own client is told.** At every Lull, and once more
//! after the last recap, each seat is read through an in-process token of its
//! own: `get_economy_forecast` (treasury, supply, draw), `list_beacons` and
//! `get_view`'s entity list (its own beacons, structures and units by kind),
//! `get_segment_feed` (the segment that has just closed: income by source and
//! the assets it paid for) and, at the end, `get_briefing`'s standing. So the
//! report shows what the gateway shows: when a rule changes what a seat is
//! charged or credited (X-12's `$` 60 at a deploy's start, item 126 (2) (f)),
//! or who the final audit names (X-08), the report follows without an edit
//! here.
//!
//! **Income by source** is read from the seat's own feed lines, which carry
//! the figure in the text (`strings::event_text`: "delivered ore worth $ 8");
//! the per-kind counts of the feed's digests are the cross-check, so a page
//! the reader missed fails the run rather than under-reporting a source.
//! **Spent** is derived: the treasury at the round's Lull, plus the round's
//! income, minus the treasury at the next reading.
//!
//! # The flags
//!
//! Per seat per match, the three conditions of item 127 (3):
//!
//! * `ONLY-GROWS` -- the treasury never fell between two readings;
//! * `STALLED` -- two rounds in a row in which the seat paid for no new asset
//!   (no `beacon_placed`, `structure_queued` or `unit_fabricated` line);
//! * `NEVER-AFFORDS-GENERATOR` -- the treasury never reached the Generator's
//!   `structures.generator.cost_dollars`, read from the rules table.
//!
//! Beside them, informational and not a flag: whether a Generator of the
//! seat's ever stood (`get_view`'s `generator` subtype), and, per match-up,
//! how many matches were decided before their round limit.
//!
//! Not built here: decision 16's column on whether the spec's `expand_east`
//! reaches its first site within its 120 s timeout (see the PLACEHOLDER at
//! [`render`]).
//!
//! # How to run it
//!
//! It is `#[ignore]`d, never a `cargo xtask ci` step, never gated and never
//! compared across operating systems (the plan's `check` section):
//!
//! ```text
//! cargo test --release -p pharmakos-gamectl --test balance -- --ignored --nocapture
//! ```
//!
//! The report is written to `balance/report.txt` under `CARGO_TARGET_TMPDIR`
//! and its summary printed. Nothing here reads a clock (AGENTS.md section
//! 4.5): the wall time is the shell's to measure.
//!
//! Three tests are not ignored, so a change to what the gateway answers
//! fails `cargo xtask ci` rather than the next owner's run:
//! [`one_short_match_reports_through_the_gateway`] plays one short match
//! through the same reader (the forecast's fields, the feed's lines, the
//! view's entity list); [`easy_seals_its_own_playbook_in_round_1`] pins how
//! the report tells Easy's own seal from its safe one; and
//! [`the_flags_say_what_item_127_3_asks`] pins the flag rules and the line
//! parsing on made-up readings.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
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
use pharmakos_sim::math::quantity::Ms;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::tables::SeatId;

// ---------------------------------------------------------------------------
// The matches
// ---------------------------------------------------------------------------

/// The twenty map seeds: the two golden seeds (the determinism harness's and
/// the scenario files'), then eighteen spread by the 64-bit golden ratio so
/// that neighbouring entries share no bits.
///
/// PLACEHOLDER: the balance check's seed set -- owner, at S1's demo with
/// `tune`'s report. No decision names one: item 90's "tuning seed set" is the
/// rules rows, not map seeds, and item 127 (3) says only "about twenty
/// seeded matches".
fn seeds() -> Vec<u64> {
    [pharmakos_sim::DETERMINISM_MATCH_SEED, SCENARIO_SEED]
        .into_iter()
        .chain((1..=18_u64).map(|n| n.wrapping_mul(GOLDEN_RATIO_64)))
        .collect()
}

/// How many seeds [`seeds`] answers.
const SEED_COUNT: usize = 20;

/// The scenario files' seed, the second golden seed.
const SCENARIO_SEED: u64 = 0x0000_0000_ca5c_aded;

/// 2^64 divided by the golden ratio, the usual spreading multiplier.
const GOLDEN_RATIO_64: u64 = 0x9e37_79b9_7f4a_7c15;

/// The two lengths the plan names: a 3-round match is 19 200 ticks on the
/// rules' 3 / 5 / 8 ladder and a 6-round one 48 000.
const ROUND_COUNTS: [u32; 2] = [3, 6];

/// Two seats: a one-seat match is decided at its first Push tick, because the
/// last seat standing wins (decisions-log item 114 (2)).
const SEATS: u32 = 2;

/// The seat the Hold & Build match-up plays the template with.
const TEMPLATE_SEAT: u8 = 0;

/// The template it plays.
const TEMPLATE_ID: &str = "hold_and_build";

/// Who plays the match.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Matchup {
    /// Both seats Easy.
    EasyVsEasy,
    /// Seat 0 the Hold & Build template as Easy suggests it; seat 1 Easy.
    HoldAndBuildVsEasy,
}

impl Matchup {
    const ALL: [Matchup; 2] = [Matchup::EasyVsEasy, Matchup::HoldAndBuildVsEasy];

    const fn name(self) -> &'static str {
        match self {
            Matchup::EasyVsEasy => "easy-vs-easy",
            Matchup::HoldAndBuildVsEasy => "hold-and-build-vs-easy",
        }
    }

    /// The seat the template plays, if this match-up has one.
    const fn template_seat(self) -> Option<u8> {
        match self {
            Matchup::EasyVsEasy => None,
            Matchup::HoldAndBuildVsEasy => Some(TEMPLATE_SEAT),
        }
    }

    /// What plays `seat`.
    fn role(self, seat: u8) -> &'static str {
        if self.template_seat() == Some(seat) {
            "hold & build"
        } else {
            "easy"
        }
    }
}

// ---------------------------------------------------------------------------
// Hosting (crates/gamectl/tests/operator.rs's pattern)
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

/// A match id unique to this run.
fn match_id(label: &str) -> String {
    format!("balance-{label}-{}", std::process::id())
}

fn seat_ids() -> Vec<SeatId> {
    (0..SEATS)
        .filter_map(|raw| u8::try_from(raw).ok())
        .map(SeatId::new)
        .collect()
}

/// `match.lull_ms`, which the scenario runner hands the gateway for every
/// Lull it spends no host time in (AGENTS.md section 4.5). `first_lull_ms` has
/// no reader yet, so neither does this file.
fn lull_ms(table: &RulesTable) -> Ms {
    Ms::new(
        table
            .message()
            .r#match
            .as_ref()
            .map(|block| block.lull_ms)
            .expect("the rules table's match block"),
    )
}

/// The Generator's price, whole `$`.
fn generator_cost(table: &RulesTable) -> i64 {
    table
        .message()
        .structures
        .as_ref()
        .and_then(|block| block.generator.as_ref())
        .map(|row| i64::from(row.cost_dollars))
        .expect("structures.generator.cost_dollars")
}

/// A match in its opening Lull, hosted as `serve::run` hosts one, on the
/// rules table's own ladder.
fn open(label: &str, seed: u64, rules: &str, rounds: u32, ladder: Vec<i32>) -> Surface {
    let host = Host::open_from(
        rules,
        seed,
        SEATS,
        &Settings {
            segment_lengths_ms: ladder,
            round_limit: rounds,
            units_per_seat: 0,
        },
        Some(root().join(LIBRARY_PATH)),
    )
    .expect("a match");
    let table = RulesTable::from_canonical_json(rules).expect("a rules table");
    let lull = lull_ms(&table);
    let mut surface = Surface::new(
        &match_id(label),
        seed,
        table,
        FogPolicy::fogged(),
        &seat_ids(),
    )
    .expect("a surface");
    surface.attach(host).expect("attached");
    surface.set_phase_remaining_ms(lull);
    surface.open_lull().expect("the opening Lull");
    surface
}

/// A seat's own token, registered as an in-process one: what the host mints
/// for a built-in seat. A fresh one per reading, so no reading spends another
/// one's per-tick budget.
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

fn object(members: Vec<(&str, Json)>) -> Json {
    Json::Object(
        members
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

fn number(value: &Json, key: &str) -> i64 {
    match value.get(key) {
        Some(Json::Number(lexeme)) => lexeme.parse().expect("a whole number"),
        _ => 0,
    }
}

/// A number the reader cannot do without: a missing or renamed field fails
/// the run rather than reading as 0, which would quietly corrupt the report
/// (a treasury of 0 at every reading raises NEVER-AFFORDS-GENERATOR on every
/// seat).
fn required(value: &Json, key: &str) -> i64 {
    match value.get(key) {
        Some(Json::Number(lexeme)) => lexeme.parse().expect("a whole number"),
        other => panic!("the reader needs `{key}` as a number, and got {other:?} in {value:?}"),
    }
}

fn string<'a>(value: &'a Json, key: &str) -> &'a str {
    match value.get(key) {
        Some(Json::String(text)) => text,
        _ => "",
    }
}

fn array<'a>(value: &'a Json, key: &str) -> &'a [Json] {
    match value.get(key) {
        Some(Json::Array(items)) => items,
        _ => &[],
    }
}

// ---------------------------------------------------------------------------
// One seat's reading
// ---------------------------------------------------------------------------

/// What a seat's own client is told at one moment.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
struct Reading {
    /// `get_economy_forecast`'s `treasury_now`, whole `$`.
    treasury: i64,
    /// `supply_kw_now`, whole kW.
    supply_kw: i64,
    /// `draw_kw_now`, whole kW.
    draw_kw: i64,
    /// Its own beacons, from `list_beacons`.
    beacons: u32,
    /// Its own structures by subtype, from `get_view`'s entity list.
    structures: BTreeMap<String, u32>,
    /// Its own units by subtype, the commander included.
    units: BTreeMap<String, u32>,
    /// `get_briefing`'s standing, `(rank, score)`; read at the end only.
    standing: Option<(i64, i64)>,
}

/// What one closed segment's feed told the seat about itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Tally {
    /// `ore_delivered`, `$`.
    ore: i64,
    /// `salvage_delivered`, `$`.
    salvage: i64,
    /// `beacon_recycled`'s refunds, `$`.
    recycled: i64,
    /// `settled`: the Basic Minimum Income the Ledger paid, `$`.
    settled: i64,
    /// `kill_credited`, `$`.
    kill_credit: i64,
    /// `beacon_placed` lines naming this seat.
    beacons_placed: u32,
    /// `structure_queued` lines: structures paid for.
    structures_queued: u32,
    /// `unit_fabricated` lines: units paid for.
    units_fabricated: u32,
}

impl Tally {
    fn income(&self) -> i64 {
        self.ore
            .saturating_add(self.salvage)
            .saturating_add(self.recycled)
            .saturating_add(self.settled)
            .saturating_add(self.kill_credit)
    }

    fn new_assets(&self) -> u32 {
        self.beacons_placed
            .saturating_add(self.structures_queued)
            .saturating_add(self.units_fabricated)
    }
}

/// Whether a feed line is about `seat`: every line about a seat names it as
/// `seat N` (`strings::event_text`).
fn names_seat(text: &str, seat: u8) -> bool {
    let needle = format!("seat {seat}");
    text.match_indices(&needle).any(|(at, _)| {
        !text
            .get(at.saturating_add(needle.len())..)
            .and_then(|rest| rest.chars().next())
            .is_some_and(|next| next.is_ascii_digit())
    })
}

/// The figure a feed line carries as `$ N`.
fn dollars_in(text: &str) -> Option<i64> {
    let (_, rest) = text.split_once("$ ")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// The income kinds: every one is private to its seat
/// (`Surface::audience_of`), so a seat is shown its own and nobody else's.
const INCOME_KINDS: [&str; 5] = [
    "ore_delivered",
    "salvage_delivered",
    "beacon_recycled",
    "settled",
    "kill_credited",
];

/// Read the seat's whole feed for the segment the gateway is holding, page
/// by page, and tally it. The digests' per-kind counts, which cover the
/// whole segment whatever a page held, must agree with the lines read.
fn tally_feed(surface: &mut Surface, token: &Token, seat: u8) -> Tally {
    let mut tally = Tally::default();
    let mut cursor = String::new();
    let mut read: BTreeMap<String, u32> = BTreeMap::new();
    let mut digest_counts: BTreeMap<String, u32> = BTreeMap::new();
    loop {
        let page = result(
            &call(
                surface,
                token,
                "get_segment_feed",
                object(vec![
                    ("cursor", Json::String(cursor.clone())),
                    ("detail", Json::String(String::from("full"))),
                ]),
            ),
            "get_segment_feed",
        );
        if digest_counts.is_empty() {
            for digest in array(&page, "digests") {
                for count in array(digest, "counts") {
                    let slot = digest_counts
                        .entry(string(count, "kind").to_owned())
                        .or_default();
                    *slot = slot
                        .saturating_add(u32::try_from(required(count, "count")).expect("a count"));
                }
            }
        }
        let events = array(&page, "events");
        if events.is_empty() {
            break;
        }
        for event in events {
            let kind = string(event, "kind");
            let text = string(event, "text");
            let slot = read.entry(kind.to_owned()).or_default();
            *slot = slot.saturating_add(1);
            if !names_seat(text, seat) {
                assert!(
                    !INCOME_KINDS.contains(&kind),
                    "seat {seat} was shown another seat's {kind} line: {text}"
                );
                continue;
            }
            let dollars = || {
                dollars_in(text).unwrap_or_else(|| {
                    panic!(
                        "a {kind} line carries its figure as `$ N`, and this one does not: {text}"
                    )
                })
            };
            match kind {
                "ore_delivered" => tally.ore = tally.ore.saturating_add(dollars()),
                "salvage_delivered" => tally.salvage = tally.salvage.saturating_add(dollars()),
                "beacon_recycled" => tally.recycled = tally.recycled.saturating_add(dollars()),
                "settled" => tally.settled = tally.settled.saturating_add(dollars()),
                "kill_credited" => {
                    tally.kill_credit = tally.kill_credit.saturating_add(dollars());
                }
                "beacon_placed" => tally.beacons_placed = tally.beacons_placed.saturating_add(1),
                "structure_queued" => {
                    tally.structures_queued = tally.structures_queued.saturating_add(1);
                }
                "unit_fabricated" => {
                    tally.units_fabricated = tally.units_fabricated.saturating_add(1);
                }
                _ => {}
            }
        }
        let next = string(&page, "next_cursor").to_owned();
        if next == cursor {
            break;
        }
        cursor = next;
    }
    for kind in INCOME_KINDS
        .iter()
        .chain(&["beacon_placed", "structure_queued", "unit_fabricated"])
    {
        assert_eq!(
            read.get(*kind).copied().unwrap_or(0),
            digest_counts.get(*kind).copied().unwrap_or(0),
            "seat {seat}: the {kind} lines read and the digests' count of them disagree"
        );
    }
    tally
}

/// The seat's own structures and units by subtype, from `get_view`'s entity
/// list, which comes on the page whose `complete` is true.
fn census(
    surface: &mut Surface,
    token: &Token,
    seat: u8,
) -> (BTreeMap<String, u32>, BTreeMap<String, u32>) {
    let own = format!("seat.{seat}");
    let mut cursor = String::new();
    loop {
        let page = result(
            &call(
                surface,
                token,
                "get_view",
                object(vec![("cursor", Json::String(cursor.clone()))]),
            ),
            "get_view",
        );
        if page.get("complete") == Some(&Json::Bool(true)) {
            let mut structures: BTreeMap<String, u32> = BTreeMap::new();
            let mut units: BTreeMap<String, u32> = BTreeMap::new();
            for entity in array(&page, "entities") {
                if string(entity, "owner") != own {
                    continue;
                }
                let into = match string(entity, "kind") {
                    "structure" => &mut structures,
                    "unit" => &mut units,
                    _ => continue,
                };
                let slot = into
                    .entry(string(entity, "subtype").to_owned())
                    .or_default();
                *slot = slot.saturating_add(1);
            }
            return (structures, units);
        }
        let next = string(&page, "next_cursor").to_owned();
        assert_ne!(next, cursor, "get_view's cursor did not move");
        cursor = next;
    }
}

/// One reading of `seat`, and the feed of the segment the gateway holds.
fn read_seat(surface: &mut Surface, seat: u8, with_standing: bool) -> (Reading, Tally) {
    let token = in_process_token(surface, seat);
    let forecast = result(
        &call(surface, &token, "get_economy_forecast", object(vec![])),
        "get_economy_forecast",
    );
    let beacons = result(
        &call(surface, &token, "list_beacons", object(vec![])),
        "list_beacons",
    );
    let own = Json::String(format!("seat.{seat}"));
    let beacons = u32::try_from(
        array(&beacons, "beacons")
            .iter()
            .filter(|row| row.get("owner") == Some(&own))
            .count(),
    )
    .expect("counted");
    let (structures, units) = census(surface, &token, seat);
    let standing = with_standing.then(|| {
        let briefing = result(
            &call(surface, &token, "get_briefing", object(vec![])),
            "get_briefing",
        );
        let standing = briefing.get("standing").cloned().unwrap_or(Json::Null);
        (number(&standing, "rank"), number(&standing, "score"))
    });
    let tally = tally_feed(surface, &token, seat);
    (
        Reading {
            treasury: required(&forecast, "treasury_now"),
            supply_kw: required(&forecast, "supply_kw_now"),
            draw_kw: required(&forecast, "draw_kw_now"),
            beacons,
            structures,
            units,
            standing,
        },
        tally,
    )
}

// ---------------------------------------------------------------------------
// One match
// ---------------------------------------------------------------------------

/// One round of one seat.
#[derive(Clone, Debug)]
struct Round {
    /// What the seat played this round.
    seal: String,
    /// The reading at this round's Lull.
    start: Reading,
    /// What the feed said about this round's Push and recap.
    tally: Tally,
}

/// One seat's whole match.
#[derive(Clone, Debug)]
struct SeatRun {
    seat: u8,
    role: &'static str,
    rounds: Vec<Round>,
    /// The reading after the last recap.
    end: Reading,
}

impl SeatRun {
    /// Every treasury reading, in order: each Lull's, then the end's.
    fn treasuries(&self) -> Vec<i64> {
        self.rounds
            .iter()
            .map(|round| round.start.treasury)
            .chain(std::iter::once(self.end.treasury))
            .collect()
    }

    /// Spent in round `index`: its Lull's treasury plus its income, less the
    /// next reading's treasury.
    fn spent(&self, index: usize) -> i64 {
        let treasuries = self.treasuries();
        let (Some(round), Some(before), Some(after)) = (
            self.rounds.get(index),
            treasuries.get(index),
            treasuries.get(index.saturating_add(1)),
        ) else {
            return 0;
        };
        before
            .saturating_add(round.tally.income())
            .saturating_sub(*after)
    }

    /// Whether a Generator of the seat's stood at any reading.
    fn built_a_generator(&self) -> bool {
        self.rounds
            .iter()
            .map(|round| &round.start)
            .chain(std::iter::once(&self.end))
            .any(|reading| reading.structures.get("generator").is_some_and(|n| *n > 0))
    }
}

/// One match, played.
#[derive(Clone, Debug)]
struct MatchRun {
    seed: u64,
    matchup: Matchup,
    rounds: u32,
    /// Ticks the Pushes played.
    ticks: u32,
    /// Rounds actually played (fewer than `rounds` when the match was decided
    /// during a Push).
    played: u32,
    /// `get_recap`'s prose after the last recap: how the match ended, and
    /// whom the gateway names.
    recap: String,
    seats: Vec<SeatRun>,
}

/// What the template seat sealed: Hold & Build with Easy's suggestion, with
/// the template's own stand-ins, or a refusal (by `instantiate_template` or
/// by `submit_plan`), which leaves the seat to whatever the gateway files.
fn submit_template(surface: &mut Surface) -> String {
    let token = in_process_token(surface, TEMPLATE_SEAT);
    let Some(made) = call(
        surface,
        &token,
        "instantiate_template",
        object(vec![
            ("template_id", Json::String(String::from(TEMPLATE_ID))),
            ("suggested", Json::Bool(true)),
        ]),
    )
    .get("result")
    .cloned() else {
        return String::from("h&b-refused");
    };
    let suggested = array(&made, "parameters")
        .iter()
        .any(|parameter| parameter.get("suggested") == Some(&Json::Bool(true)));
    let playbook = string(&made, "playbook_jsonc").to_owned();
    let submitted = call(
        surface,
        &token,
        "submit_plan",
        object(vec![("playbook_jsonc", Json::String(playbook))]),
    );
    let accepted = submitted
        .get("result")
        .and_then(|answer| answer.get("accepted"))
        == Some(&Json::Bool(true));
    match (accepted, suggested) {
        (true, true) => String::from("h&b"),
        (true, false) => String::from("h&b-standins"),
        (false, _) => String::from("h&b-refused"),
    }
}

/// What each seat holds once the Push has begun, for the report: the
/// template's own word for the template seat, otherwise whose seal it is.
fn seal_of(surface: &Surface, seat: u8, round: u32, template: Option<&str>) -> String {
    let state = surface
        .seat_state(Subject::Seat(SeatId::new(seat)), SeatId::new(seat))
        .expect("its own");
    let Some(sealed) = state.sealed.as_ref().filter(|sealed| sealed.round == round) else {
        return String::from("none");
    };
    if sealed.filed_by_the_gateway {
        return String::from("filed-safe");
    }
    if let Some(word) = template {
        return word.to_owned();
    }
    if sealed.playbook_jsonc.contains("Easy, seat") {
        String::from("easy-own")
    } else {
        String::from("easy-safe")
    }
}

/// Play one match through the gateway and read every seat at every Lull and
/// once at the end.
fn play(seed: u64, matchup: Matchup, rounds: u32, rules: &str, ladder: Vec<i32>) -> MatchRun {
    let label = format!("{}-{rounds}-{seed:016x}", matchup.name());
    let mut surface = open(&label, seed, rules, rounds, ladder);
    let lull = lull_ms(&RulesTable::from_canonical_json(rules).expect("a rules table"));
    let mut factory = EasyOperators::new(rules).expect("the operator reads the rules text");
    let mut easy = InProcessSeats::open(
        &mut surface,
        &seat_ids(),
        matchup.template_seat(),
        &mut factory,
    )
    .expect("in-process seats");

    let mut seats: Vec<SeatRun> = seat_ids()
        .iter()
        .map(|seat| SeatRun {
            seat: seat.raw(),
            role: matchup.role(seat.raw()),
            rounds: Vec::new(),
            end: Reading::default(),
        })
        .collect();
    let mut ticks = 0_u32;
    let mut played = 0_u32;
    let mut decided = false;
    for round in 1..=rounds {
        for run in &mut seats {
            let (reading, tally) = read_seat(&mut surface, run.seat, false);
            // The feed the gateway holds in a Lull is the segment that has
            // just closed: last round's Push and recap.
            if let Some(last) = run.rounds.last_mut() {
                last.tally = tally;
            }
            run.rounds.push(Round {
                seal: String::new(),
                start: reading,
                tally: Tally::default(),
            });
        }

        easy.plan(&mut surface);
        let template = matchup
            .template_seat()
            .map(|_| submit_template(&mut surface));
        assert!(
            surface.begin_push().expect("the Push opens"),
            "{label}: round {round} would not open"
        );
        for run in &mut seats {
            let word = (matchup.template_seat() == Some(run.seat))
                .then_some(template.as_deref())
                .flatten();
            if let Some(last) = run.rounds.last_mut() {
                last.seal = seal_of(&surface, run.seat, round, word);
            }
        }
        while let Some(report) = surface.step().expect("a tick") {
            ticks = ticks.saturating_add(1);
            decided |= report.match_ended;
            if report.segment_ended {
                break;
            }
        }
        played = round;
        surface.end_recap().expect("the recap closes");
        if decided || round == rounds {
            break;
        }
        surface.set_phase_remaining_ms(lull);
        surface.open_lull().expect("the next Lull");
    }
    for run in &mut seats {
        let (reading, tally) = read_seat(&mut surface, run.seat, true);
        if let Some(last) = run.rounds.last_mut() {
            last.tally = tally;
        }
        run.end = reading;
    }
    let token = in_process_token(&mut surface, 0);
    let recap = result(
        &call(&mut surface, &token, "get_recap", object(vec![])),
        "get_recap",
    );
    MatchRun {
        seed,
        matchup,
        rounds,
        ticks,
        played,
        recap: string(&recap, "prose").to_owned(),
        seats,
    }
}

// ---------------------------------------------------------------------------
// The flags
// ---------------------------------------------------------------------------

/// Item 127 (3)'s three conditions, for one seat of one match.
///
/// PLACEHOLDER: the flags' readings of item 127 (3) -- owner, at S1's demo
/// with `tune`'s report. No decision fixes them, and each is a guess: a flat
/// treasury counts as ONLY-GROWS (`<=`); any `beacon_placed` counts as a new
/// asset for STALLED, so a free stacked beacon hides a stall until X-12's
/// charge and F4's cure land; and NEVER-AFFORDS-GENERATOR counts round 1's
/// opening treasury, so it cannot fire while the starting treasury
/// (`economy.bmi_dollars` x `starting_bmi_multiplier`) is at least the
/// Generator's price.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
struct Flags {
    /// The treasury never fell between two readings.
    only_grows: bool,
    /// The first of two rounds in a row with no new asset paid for, from one.
    stalled_from: Option<u32>,
    /// The treasury never reached the Generator's price.
    never_affords_generator: bool,
}

impl Flags {
    fn any(&self) -> bool {
        self.only_grows || self.stalled_from.is_some() || self.never_affords_generator
    }

    fn words(&self) -> String {
        let mut out: Vec<String> = Vec::new();
        if self.only_grows {
            out.push(String::from("ONLY-GROWS"));
        }
        if let Some(from) = self.stalled_from {
            out.push(format!("STALLED(rounds {from}-{})", from.saturating_add(1)));
        }
        if self.never_affords_generator {
            out.push(String::from("NEVER-AFFORDS-GENERATOR"));
        }
        if out.is_empty() {
            String::from("none")
        } else {
            out.join(" ")
        }
    }
}

/// The flags of one seat's treasury readings and per-round new assets.
fn flags_of(treasuries: &[i64], new_assets: &[u32], generator_cost: i64) -> Flags {
    let only_grows = treasuries.len() >= 2
        && treasuries
            .windows(2)
            .all(|pair| pair.first() <= pair.get(1));
    let stalled_from = new_assets
        .windows(2)
        .position(|pair| pair.iter().all(|n| *n == 0))
        .and_then(|index| u32::try_from(index).ok())
        .map(|index| index.saturating_add(1));
    let never_affords_generator = treasuries.iter().all(|treasury| *treasury < generator_cost);
    Flags {
        only_grows,
        stalled_from,
        never_affords_generator,
    }
}

fn flags_of_run(run: &SeatRun, generator_cost: i64) -> Flags {
    let new_assets: Vec<u32> = run
        .rounds
        .iter()
        .map(|round| round.tally.new_assets())
        .collect();
    flags_of(&run.treasuries(), &new_assets, generator_cost)
}

// ---------------------------------------------------------------------------
// The report
// ---------------------------------------------------------------------------

fn counts(map: &BTreeMap<String, u32>) -> String {
    if map.is_empty() {
        return String::from("-");
    }
    map.iter()
        .map(|(kind, n)| format!("{kind}x{n}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// One match's block of the report.
fn render_match(out: &mut String, run: &MatchRun, generator_cost: i64) {
    let _ = writeln!(
        out,
        "== {} / {} rounds / seed 0x{:016x}: {} rounds played, {} Push ticks",
        run.matchup.name(),
        run.rounds,
        run.seed,
        run.played,
        run.ticks
    );
    let _ = writeln!(out, "  recap: {}", run.recap);
    for seat in &run.seats {
        let flags = flags_of_run(seat, generator_cost);
        // Ranks count from one; `get_briefing` answers rank 0 until the
        // economy's surfaces serve the final audit's standing, and a 0 here
        // would read as a rank.
        let standing = match seat.end.standing {
            Some((rank, score)) if rank > 0 => format!("rank {rank}, score {score}"),
            Some(_) => String::from("standing not served yet"),
            None => String::from("-"),
        };
        let _ = writeln!(
            out,
            "  seat {} ({}): end treasury $ {}, {standing}; generator built: {}; flags: {}",
            seat.seat,
            seat.role,
            seat.end.treasury,
            if seat.built_a_generator() {
                "yes"
            } else {
                "no"
            },
            flags.words()
        );
        let _ = writeln!(
            out,
            "    {:<5} {:<13} {:>8} {:>6} {:>7} {:>7} {:>7} {:>5} {:>6} {:>6} {:>4} {:>3}  {:<5} {:<24} {:<36} new(b/s/u)",
            "round",
            "seal",
            "treasury",
            "ore",
            "salvage",
            "recycle",
            "settled",
            "kill",
            "spent",
            "supply",
            "draw",
            "bcn",
            "",
            "structures",
            "units",
        );
        for (index, round) in seat.rounds.iter().enumerate() {
            let tally = &round.tally;
            let _ = writeln!(
                out,
                "    {:<5} {:<13} {:>8} {:>6} {:>7} {:>7} {:>7} {:>5} {:>6} {:>6} {:>4} {:>3}  {:<5} {:<24} {:<36} {}/{}/{}",
                index.saturating_add(1),
                round.seal,
                round.start.treasury,
                tally.ore,
                tally.salvage,
                tally.recycled,
                tally.settled,
                tally.kill_credit,
                seat.spent(index),
                round.start.supply_kw,
                round.start.draw_kw,
                round.start.beacons,
                "",
                counts(&round.start.structures),
                counts(&round.start.units),
                tally.beacons_placed,
                tally.structures_queued,
                tally.units_fabricated,
            );
        }
        let _ = writeln!(
            out,
            "    {:<5} {:<13} {:>8} {:>6} {:>7} {:>7} {:>7} {:>5} {:>6} {:>6} {:>4} {:>3}  {:<5} {:<24} {:<36}",
            "end",
            "-",
            seat.end.treasury,
            "",
            "",
            "",
            "",
            "",
            "",
            seat.end.supply_kw,
            seat.end.draw_kw,
            seat.end.beacons,
            "",
            counts(&seat.end.structures),
            counts(&seat.end.units),
        );
    }
}

/// The summary: per match-up and length, how many seats raised each flag.
fn render_summary(out: &mut String, runs: &[MatchRun], generator_cost: i64) {
    let _ = writeln!(
        out,
        "{:<24} {:<14} {:>6} {:>7} {:>7} {:>6} {:>10} {:>8} {:>17} {:>9} {:>14}",
        "match-up",
        "seat",
        "rounds",
        "matches",
        "decided",
        "seats",
        "only-grows",
        "stalled",
        "never-affords-gen",
        "built-gen",
        "end $ min..max"
    );
    // One line per match-up, length and role, in that order: the template
    // seat and Easy are told apart, since a flag means something different
    // for each.
    let groups: BTreeSet<(Matchup, u32, &str)> = runs
        .iter()
        .flat_map(|run| {
            run.seats
                .iter()
                .map(move |seat| (run.matchup, run.rounds, seat.role))
        })
        .collect();
    for (matchup, rounds, role) in groups {
        let group: Vec<&MatchRun> = runs
            .iter()
            .filter(|run| run.matchup == matchup && run.rounds == rounds)
            .collect();
        let seats: Vec<&SeatRun> = group
            .iter()
            .flat_map(|run| run.seats.iter())
            .filter(|seat| seat.role == role)
            .collect();
        let flags: Vec<Flags> = seats
            .iter()
            .map(|seat| flags_of_run(seat, generator_cost))
            .collect();
        let count = |test: &dyn Fn(&Flags) -> bool| flags.iter().filter(|f| test(f)).count();
        let built = seats.iter().filter(|seat| seat.built_a_generator()).count();
        let ends: Vec<i64> = seats.iter().map(|seat| seat.end.treasury).collect();
        let low = ends.iter().min().copied().unwrap_or(0);
        let high = ends.iter().max().copied().unwrap_or(0);
        let _ = writeln!(
            out,
            "{:<24} {:<14} {:>6} {:>7} {:>7} {:>6} {:>10} {:>8} {:>17} {:>9} {:>14}",
            matchup.name(),
            role,
            rounds,
            group.len(),
            group.iter().filter(|run| run.played < run.rounds).count(),
            seats.len(),
            count(&|f| f.only_grows),
            count(&|f| f.stalled_from.is_some()),
            count(&|f| f.never_affords_generator),
            built,
            format!("{low}..{high}"),
        );
    }
    // The template seat on its own, since it is the one the plan pairs
    // against Easy.
    let template: Vec<&SeatRun> = runs
        .iter()
        .filter(|run| run.matchup == Matchup::HoldAndBuildVsEasy)
        .flat_map(|run| run.seats.iter())
        .filter(|seat| seat.seat == TEMPLATE_SEAT)
        .collect();
    if !template.is_empty() {
        let seals: BTreeMap<&str, usize> = template
            .iter()
            .flat_map(|seat| seat.rounds.iter())
            .fold(BTreeMap::new(), |mut map, round| {
                let slot = map.entry(round.seal.as_str()).or_default();
                *slot = slot.saturating_add(1);
                map
            });
        let _ = writeln!(out, "hold & build seat's seals by round: {seals:?}");
    }
    let flagged: usize = runs
        .iter()
        .flat_map(|run| run.seats.iter())
        .filter(|seat| flags_of_run(seat, generator_cost).any())
        .count();
    let _ = writeln!(
        out,
        "{} matches, {} Push ticks, {flagged} of {} seat-matches flagged",
        runs.len(),
        runs.iter().map(|run| u64::from(run.ticks)).sum::<u64>(),
        runs.iter().map(|run| run.seats.len()).sum::<usize>()
    );
}

/// The whole report: a header, the summary, then every match.
///
/// PLACEHOLDER: decision 16's column -- whether the spec's `expand_east`
/// reaches its first site within its 120 s timeout -- is not built here;
/// `tune` (wave 5), which owns decision 16, adds it, or the owner moves it at
/// S1's demo. The example is a playbook the verifier refuses
/// (`expand-east-segment` seals it through the scenario harness's own door),
/// so this file, which plays only what a seat may submit, cannot seal it.
fn render(runs: &[MatchRun], generator_cost: i64, rules_hash: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Pharmakos headless balance check (S1's plan, task `check`; decisions-log item 127 (3))."
    );
    let _ = writeln!(
        out,
        "Rules hash {rules_hash}; Generator $ {generator_cost}. Every figure is what the seat's own \
         client is told: treasury, supply and draw at each Lull (`end` after the last recap); \
         income by source from the seat's feed for that round's Push and recap; spent = this \
         Lull's treasury + income - the next reading's. new(b/s/u) = beacons placed, structures \
         paid for, units fabricated that round."
    );
    let _ = writeln!(
        out,
        "Flags: ONLY-GROWS = the treasury never fell; STALLED = two rounds in a row with no new \
         asset; NEVER-AFFORDS-GENERATOR = the treasury never reached the Generator's price. \
         `decided` = matches decided before their round limit (not a flag)."
    );
    out.push('\n');
    render_summary(&mut out, runs, generator_cost);
    for run in runs {
        out.push('\n');
        render_match(&mut out, run, generator_cost);
    }
    out
}

/// Where the report goes: `balance/report.txt` under `CARGO_TARGET_TMPDIR`.
fn report_path() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("balance")
        .join("report.txt")
}

// ---------------------------------------------------------------------------
// The check, and its smoke test
// ---------------------------------------------------------------------------

/// The 80 matches. Writes the report and prints its summary; asserts only
/// what makes the report trustworthy (every match was played, every reading
/// was answered), never a balance condition -- a flag, or a match decided
/// before its round limit, is the owner's evidence, not a red test.
#[test]
#[ignore = "the headless balance check: 80 matches, minutes in release; run it by name"]
fn balance_check_writes_its_report() {
    let rules = rules_json();
    let table = RulesTable::from_canonical_json(&rules).expect("a rules table");
    let cost = generator_cost(&table);
    let mut runs: Vec<MatchRun> = Vec::new();
    for matchup in Matchup::ALL {
        for rounds in ROUND_COUNTS {
            for seed in seeds() {
                let run = play(seed, matchup, rounds, &rules, Vec::new());
                eprintln!(
                    "{} / {rounds} rounds / seed 0x{seed:016x}: {} ticks",
                    matchup.name(),
                    run.ticks
                );
                runs.push(run);
            }
        }
    }
    let text = render(&runs, cost, &pharmakos_sim::hex(table.rules_hash()));
    let path = report_path();
    std::fs::create_dir_all(path.parent().expect("a folder")).expect("the report folder");
    std::fs::write(&path, &text).expect("the report");
    let summary: String = text.split("\n\n").nth(1).unwrap_or_default().to_owned();
    eprintln!("{summary}\nthe report: {}", path.display());
    assert_eq!(
        runs.len(),
        SEED_COUNT * Matchup::ALL.len() * ROUND_COUNTS.len()
    );
    for run in &runs {
        assert!(
            run.played >= 1 && run.played <= run.rounds,
            "{} / seed 0x{:016x} played {} of {} rounds",
            run.matchup.name(),
            run.seed,
            run.played,
            run.rounds
        );
    }
}

/// The smoke test's Push length: long enough for the template seat's
/// commander to start its walk and for the recap to settle, short enough to
/// play twice in a debug build on every CI leg.
const SMOKE_PUSH_MS: i32 = 30_000;

/// The reader on one short match, so a change to what the gateway answers
/// fails `cargo xtask ci` rather than the owner's next run: Hold & Build vs
/// Easy at the scenario files' seed, two rounds of 30 s.
///
/// Every seat is read at both Lulls and at the end; round 1 opens on the
/// rules' starting treasury and every reading has a draw and a supply, so a
/// renamed forecast field cannot read as 0 unnoticed; each round's
/// settlement reaches the feed; no round's derived spend is below zero; the
/// census sees the seat's commander; the template's seat sealed the template
/// with Easy's suggestion in round 1; and the report renders it. (On these
/// 30 s Pushes Easy seals its safe playbook from round 1, so
/// [`easy_seals_its_own_playbook_in_round_1`] pins the `easy-own` label.)
#[test]
fn one_short_match_reports_through_the_gateway() {
    let rules = rules_json();
    let table = RulesTable::from_canonical_json(&rules).expect("a rules table");
    let cost = generator_cost(&table);
    let seed = SCENARIO_SEED;
    let run = play(
        seed,
        Matchup::HoldAndBuildVsEasy,
        2,
        &rules,
        vec![SMOKE_PUSH_MS, SMOKE_PUSH_MS],
    );
    assert_eq!(run.played, 2);
    assert!(
        run.recap.contains("The match ended"),
        "the last recap says the match is over: {}",
        run.recap
    );
    assert_eq!(
        run.ticks,
        Ms::new(SMOKE_PUSH_MS).to_ticks_floor().saturating_mul(2),
        "two Pushes of 30 s"
    );
    let opening = pharmakos_sim::economy::starting_treasury(&table).raw();
    for seat in &run.seats {
        let what = format!("seat {}: {seat:#?}", seat.seat);
        assert_eq!(seat.rounds.len(), 2, "{what}");
        assert_eq!(
            seat.rounds.first().map(|round| round.start.treasury),
            Some(opening),
            "round 1 opens on the rules' starting treasury: {what}"
        );
        assert!(
            seat.rounds
                .iter()
                .map(|round| &round.start)
                .chain(std::iter::once(&seat.end))
                .all(|reading| reading.draw_kw > 0 && reading.supply_kw > 0),
            "every reading has a draw and a supply: {what}"
        );
        assert!(
            seat.rounds.iter().all(|round| round.tally.settled > 0),
            "{what}"
        );
        assert!((0..2).all(|index| seat.spent(index) >= 0), "{what}");
        assert!(
            seat.end.units.get("commander") == Some(&1),
            "the census sees the seat's own commander: {what}"
        );
        assert!(seat.end.beacons >= 1, "{what}");
        assert!(seat.end.supply_kw > 0, "{what}");
    }
    let template = run
        .seats
        .iter()
        .find(|seat| seat.seat == TEMPLATE_SEAT)
        .expect("the template seat");
    assert_eq!(
        template.rounds.first().map(|round| round.seal.as_str()),
        Some("h&b"),
        "{template:#?}"
    );
    let easy = run
        .seats
        .iter()
        .find(|seat| seat.seat != TEMPLATE_SEAT)
        .expect("Easy's seat");
    assert!(
        easy.rounds
            .iter()
            .all(|round| round.seal == "easy-own" || round.seal == "easy-safe"),
        "{easy:#?}"
    );
    let text = render(
        std::slice::from_ref(&run),
        cost,
        &pharmakos_sim::hex(table.rules_hash()),
    );
    assert!(
        text.contains("== hold-and-build-vs-easy / 2 rounds"),
        "{text}"
    );
    for role in ["hold & build", "easy"] {
        assert!(
            text.lines()
                .any(|line| line.starts_with("hold-and-build-vs-easy") && line.contains(role)),
            "the summary has a line for the {role} seat: {text}"
        );
    }
    eprintln!("{text}");
}

/// `seal_of` tells Easy's own playbook from its safe one by the seed note
/// Easy writes into its own playbook's `/meta/note` (`compose::seed_note`).
/// Easy seals its own playbook in round 1 on the rules' own ladder (item
/// 113 (7)), so the opening Lull of an Easy vs Easy match pins that wording:
/// were it to change, every Easy seal would read `easy-safe` unnoticed.
#[test]
fn easy_seals_its_own_playbook_in_round_1() {
    let rules = rules_json();
    let mut surface = open("easy-own", SCENARIO_SEED, &rules, 1, Vec::new());
    let mut factory = EasyOperators::new(&rules).expect("the operator reads the rules text");
    let mut easy = InProcessSeats::open(&mut surface, &seat_ids(), None, &mut factory)
        .expect("in-process seats");
    easy.plan(&mut surface);
    assert!(surface.begin_push().expect("the Push opens"));
    for seat in seat_ids() {
        assert_eq!(
            seal_of(&surface, seat.raw(), 1, None),
            "easy-own",
            "seat {seat:?}"
        );
    }
}

/// The flag rules, on made-up readings.
#[test]
fn the_flags_say_what_item_127_3_asks() {
    // Spends once, then banks: not only-grows.
    assert!(!flags_of(&[200, 150, 250, 300], &[3, 1, 1], 80).only_grows);
    // Never falls: only-grows.
    assert!(flags_of(&[200, 200, 260], &[1, 2], 80).only_grows);
    // Two quiet rounds in a row, from round 2.
    assert_eq!(
        flags_of(&[200, 100, 150, 200, 250], &[2, 0, 0, 1], 80).stalled_from,
        Some(2)
    );
    // One quiet round alone is not a stall.
    assert_eq!(
        flags_of(&[200, 100, 150, 120], &[1, 0, 1], 80).stalled_from,
        None
    );
    // Never reaches the price.
    assert!(flags_of(&[50, 70, 79], &[1, 1], 80).never_affords_generator);
    assert!(!flags_of(&[50, 80, 79], &[1, 1], 80).never_affords_generator);
    assert_eq!(
        dollars_in("A mining drone of seat 1 delivered ore worth $ 32."),
        Some(32)
    );
    assert_eq!(
        dollars_in("The Ledger settled and credited seat 0 $ 150."),
        Some(150)
    );
    assert_eq!(dollars_in("A structure of seat 0 is finished."), None);
    assert!(names_seat("A beacon of seat 1 was deployed.", 1));
    assert!(!names_seat("A beacon of seat 12 was deployed.", 1));
    assert!(!names_seat("A beacon of seat 0 was deployed.", 1));
}
