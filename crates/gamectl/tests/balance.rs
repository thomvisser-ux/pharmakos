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
//! seat's ever stood (`get_view`'s `generator` subtype); per match-up, how
//! many matches were decided before their round limit; and the most a seat
//! placed, paid for and fabricated in one match, the per-seat share of the
//! fielding limits (register S1-37, which the owner rules on at S1's demo with
//! this report).
//!
//! # Decision 16's column: the spec's `expand_east`
//!
//! S1's plan, decision 16 asks the report whether the spec's worked example,
//! `examples/playbooks/expand_east.jsonc`, reaches its first site within its
//! own timeout, so the owner can rule at S1's demo whether the commander's
//! walking speed (inside spec section 4's 1 to 1.5 voxels a second) or the
//! example moves (decisions-log item 103 (3)). The example is a playbook the
//! verifier refuses, so no seat of these matches can submit it; the column is
//! read instead from the committed `expand-east-segment` scenario, played
//! through `gamectl scenario run`'s own `play` (which seals it through the
//! scenario harness's documented door), and from `estimate_route` asked by
//! each seat's own token on the same map: the walk from the seat's core to the
//! example's site at the committed speed, and the speed that walk would need
//! to fit the step's timeout.
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
//! Five tests are not ignored, so a change to what the gateway answers
//! fails `cargo xtask ci` rather than the next owner's run:
//! [`one_short_match_reports_through_the_gateway`] plays one short match
//! through the same reader (the forecast's fields, the feed's lines, the
//! view's entity list); [`easy_seals_its_own_playbook_in_round_1`] pins how
//! the report tells Easy's own seal from its safe one;
//! [`the_flags_say_what_item_127_3_asks`] pins the flag rules and the line
//! parsing on made-up readings; and
//! [`the_expand_east_column_reads_the_first_step`] pins decision 16's column's
//! event reading and speed arithmetic on made-up figures; and
//! [`the_expand_east_column_asks_each_seats_estimate_route`] runs the column's
//! gateway half, `estimate_route` from each seat's core to the site, through
//! the same reader. The column's scenario half plays a committed scenario the
//! `scenario` step already plays.
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
use pharmakos_sim::math::quantity::{Ms, TICK_HZ};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::tables::{SeatId, UnitKind};

// ---------------------------------------------------------------------------
// The matches
// ---------------------------------------------------------------------------

/// The twenty map seeds: the two golden seeds (the determinism harness's and
/// the scenario files'), then eighteen spread by the 64-bit golden ratio so
/// that neighbouring entries share no bits.
///
/// PLACEHOLDER: the balance check's seed set — owner, at S2's demo, with `tune2`'s balance report, which extends this check to combat on these seeds (item 139 (1) ruling 5: it stands for S1, after `tune`'s report).
/// No decision names one: item 90's "tuning seed set" is the rules rows, not
/// map seeds, and item 127 (3) says only "about twenty seeded matches".
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

/// The seat a template match-up plays the template with.
const TEMPLATE_SEAT: u8 = 0;

/// The rounds the three-seat run plays: the longer of [`ROUND_COUNTS`], so the
/// kingmaking figure reads the most fighting the check plays.
const THREE_SEAT_ROUNDS: u32 = 6;

/// A wizard template, as `library/` names it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Template {
    HoldAndBuild,
    ExpandAndMine,
    DefendTheCore,
    Raid,
}

impl Template {
    /// Every template the wizard offers (`library/`, the safe playbook aside,
    /// which no seat chooses).
    const ALL: [Template; 4] = [
        Template::HoldAndBuild,
        Template::ExpandAndMine,
        Template::DefendTheCore,
        Template::Raid,
    ];

    /// The template's id, its file's stem.
    const fn id(self) -> &'static str {
        match self {
            Template::HoldAndBuild => "hold_and_build",
            Template::ExpandAndMine => "expand_and_mine",
            Template::DefendTheCore => "defend_the_core",
            Template::Raid => "raid_an_enemy_beacon",
        }
    }

    /// The seal's word in the report: `<word>` with Easy's suggestion,
    /// `<word>-standins` with the template's own, `<word>-refused`.
    const fn word(self) -> &'static str {
        match self {
            Template::HoldAndBuild => "h&b",
            Template::ExpandAndMine => "e&m",
            Template::DefendTheCore => "dtc",
            Template::Raid => "raid",
        }
    }

    /// The seat's role in the report.
    const fn role(self) -> &'static str {
        match self {
            Template::HoldAndBuild => "hold & build",
            Template::ExpandAndMine => "expand & mine",
            Template::DefendTheCore => "defend the core",
            Template::Raid => "raid",
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Template::HoldAndBuild => "hold-and-build-vs-easy",
            Template::ExpandAndMine => "expand-and-mine-vs-easy",
            Template::DefendTheCore => "defend-the-core-vs-easy",
            Template::Raid => "raid-vs-easy",
        }
    }
}

/// Who plays the match.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Matchup {
    /// Both seats Easy.
    EasyVsEasy,
    /// Seat 0 a template as Easy suggests it; seat 1 Easy.
    TemplateVsEasy(Template),
    /// Three Easy seats: decision 36's kingmaking run.
    ThreeSeats,
}

impl Matchup {
    /// The two-seat match-ups, each played on every seed at both lengths:
    /// Easy against Easy and each template against Easy (S2's plan, `tune2`).
    const TWO_SEAT: [Matchup; 5] = [
        Matchup::EasyVsEasy,
        Matchup::TemplateVsEasy(Template::ALL[0]),
        Matchup::TemplateVsEasy(Template::ALL[1]),
        Matchup::TemplateVsEasy(Template::ALL[2]),
        Matchup::TemplateVsEasy(Template::ALL[3]),
    ];

    const fn name(self) -> &'static str {
        match self {
            Matchup::EasyVsEasy => "easy-vs-easy",
            Matchup::TemplateVsEasy(template) => template.name(),
            Matchup::ThreeSeats => "three-easy",
        }
    }

    /// How many seats share the match. Two at least: a one-seat match is
    /// decided at its first Push tick, because the last seat standing wins
    /// (decisions-log item 114 (2)).
    const fn seats(self) -> u32 {
        match self {
            Matchup::EasyVsEasy | Matchup::TemplateVsEasy(_) => 2,
            Matchup::ThreeSeats => 3,
        }
    }

    /// The template, if this match-up plays one.
    const fn template(self) -> Option<Template> {
        match self {
            Matchup::TemplateVsEasy(template) => Some(template),
            Matchup::EasyVsEasy | Matchup::ThreeSeats => None,
        }
    }

    /// The seat the template plays, if this match-up has one.
    const fn template_seat(self) -> Option<u8> {
        match self {
            Matchup::TemplateVsEasy(_) => Some(TEMPLATE_SEAT),
            Matchup::EasyVsEasy | Matchup::ThreeSeats => None,
        }
    }

    /// What plays `seat`.
    fn role(self, seat: u8) -> &'static str {
        match self.template() {
            Some(template) if self.template_seat() == Some(seat) => template.role(),
            _ => "easy",
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

fn seat_ids(seats: u32) -> Vec<SeatId> {
    (0..seats)
        .filter_map(|raw| u8::try_from(raw).ok())
        .map(SeatId::new)
        .collect()
}

/// The Lull length the host hands the gateway for a Lull it spends no host
/// time in (AGENTS.md section 4.5), as `gamectl scenario run` does: round 1's
/// `match.first_lull_ms` and every later round's `match.lull_ms` (decisions-log
/// item 127 (2); `econ` #92 gave `first_lull_ms` its readers).
fn lull_ms(table: &RulesTable, round: u32) -> Ms {
    let block = table
        .message()
        .r#match
        .as_ref()
        .expect("the rules table's match block");
    Ms::new(if round == 1 {
        block.first_lull_ms
    } else {
        block.lull_ms
    })
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
fn open(
    label: &str,
    seed: u64,
    rules: &str,
    (rounds, seats): (u32, u32),
    ladder: Vec<i32>,
) -> Surface {
    let host = Host::open_from(
        rules,
        seed,
        seats,
        &Settings {
            segment_lengths_ms: ladder,
            round_limit: rounds,
        },
        Some(root().join(LIBRARY_PATH)),
    )
    .expect("a match");
    let table = RulesTable::from_canonical_json(rules).expect("a rules table");
    let lull = lull_ms(&table, 1);
    let mut surface = Surface::new(
        &match_id(label),
        seed,
        table,
        FogPolicy::fogged(),
        &seat_ids(seats),
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

/// One seat's combat counters (S2's plan, E15): the sim's hashed tally, which
/// the recap's kills, losses and shared kills read and the final audit's
/// enemy-value term is. Read cumulatively, so a round's figure is the
/// difference between two readings.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Combat {
    /// Enemy assets destroyed with a share of their credit.
    kills: u32,
    /// Of those, the ones whose credit was split with another seat.
    shared: u32,
    /// Own assets destroyed.
    losses: u32,
    /// Enemy value destroyed, whole `$`: the seat's settled share.
    destroyed: i64,
    /// Own scouts lost: the drops in its live scout count, tick by tick
    /// (decisions-log item 145's `surv` asked for them, item 148 (2)'s seat
    /// sight putting a scout at 8 voxels inside every enemy weapon's range).
    scouts_lost: u32,
}

impl Combat {
    /// What happened between `self` and the later reading `after`.
    fn until(self, after: Combat) -> Combat {
        Combat {
            kills: after.kills.saturating_sub(self.kills),
            shared: after.shared.saturating_sub(self.shared),
            losses: after.losses.saturating_sub(self.losses),
            destroyed: after.destroyed.saturating_sub(self.destroyed),
            scouts_lost: after.scouts_lost.saturating_sub(self.scouts_lost),
        }
    }
}

/// One round of one seat.
#[derive(Clone, Debug)]
struct Round {
    /// What the seat played this round.
    seal: String,
    /// The reading at this round's Lull.
    start: Reading,
    /// The seat's combat counters at this round's Lull.
    combat: Combat,
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
    /// The combat counters after the last recap.
    end_combat: Combat,
    /// The Push tick, counted over the whole match, on which the seat's first
    /// kill settled, and the round it fell in.
    first_kill: Option<(u32, u32)>,
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

    /// Round `index`'s combat: its Lull's counters to the next reading's.
    fn combat_in(&self, index: usize) -> Combat {
        let Some(round) = self.rounds.get(index) else {
            return Combat::default();
        };
        let after = self
            .rounds
            .get(index.saturating_add(1))
            .map_or(self.end_combat, |next| next.combat);
        round.combat.until(after)
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

/// Decision 36's kingmaking figure for one three-seat match: the hit points
/// dealt seat against seat (`World::damage_dealt`, `fire`'s unhashed counter,
/// E15), and how much of it was aimed at the seat trailing in the Ledger's
/// standing when it was dealt.
///
/// "Trailing" is the final audit's order (`audit::final_audit`, the order the
/// gateway's standing reads, decisions-log item 135 (2) (e)) as it stood at
/// the end of the tick before: the seats whose line ranks last. While every
/// audited seat ties there is no trailing seat, and what is dealt then is
/// counted apart. `expected_milli` is what the trailing seats would have taken
/// had every attacker spread its damage evenly over the other seats still on
/// the audit, in thousandths of a hit point: the figure to read the share
/// against.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Kingmaking {
    /// Every hit point dealt seat against seat.
    dealt: u64,
    /// Of it, the hit points dealt while no seat trailed.
    no_trailing: u64,
    /// Of the rest, the hit points aimed at a trailing seat.
    at_trailing: u64,
    /// The even-spread figure for `at_trailing`, in thousandths.
    expected_milli: u64,
}

impl Kingmaking {
    fn add(self, other: Kingmaking) -> Kingmaking {
        Kingmaking {
            dealt: self.dealt.saturating_add(other.dealt),
            no_trailing: self.no_trailing.saturating_add(other.no_trailing),
            at_trailing: self.at_trailing.saturating_add(other.at_trailing),
            expected_milli: self.expected_milli.saturating_add(other.expected_milli),
        }
    }

    /// The share of the damage dealt while a seat trailed that was aimed at
    /// it, and the even-spread share, both in tenths of a percent.
    fn shares(self) -> Option<(u64, u64)> {
        let scored = self.dealt.saturating_sub(self.no_trailing);
        let share = self.at_trailing.saturating_mul(1_000).checked_div(scored)?;
        let expected = self.expected_milli.checked_div(scored)?;
        Some((share, expected))
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
    /// How the match ended, from the runner's outcome: the reason, the winner
    /// and the tick.
    ended: String,
    /// Decision 36's figure, for a match of three seats.
    kingmaking: Option<Kingmaking>,
    seats: Vec<SeatRun>,
}

/// What the template seat sealed: the template with Easy's suggestion, with
/// the template's own stand-ins, or a refusal (by `instantiate_template` or
/// by `submit_plan`), which leaves the seat to whatever the gateway files.
fn submit_template(surface: &mut Surface, template: Template) -> String {
    let token = in_process_token(surface, TEMPLATE_SEAT);
    let word = template.word();
    let Some(made) = call(
        surface,
        &token,
        "instantiate_template",
        object(vec![
            ("template_id", Json::String(String::from(template.id()))),
            ("suggested", Json::Bool(true)),
        ]),
    )
    .get("result")
    .cloned() else {
        return format!("{word}-refused");
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
        (true, true) => word.to_owned(),
        (true, false) => format!("{word}-standins"),
        (false, _) => format!("{word}-refused"),
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

/// Every seat's combat counters as the sim holds them. A read of the hosted
/// world through `Host::world`, which hands out a `&World` whose every
/// stepping method needs `&mut` (`scenario/run.rs`'s reading of the outcome
/// and the audit is the same reach): the tally is the hashed state the recap
/// reads (E15), and the scouts are counted tick by tick by [`play`].
fn combat_now(surface: &Surface, seats: u32, scouts_lost: &[u32]) -> Vec<Combat> {
    let world = surface.host().expect("a hosted match").world();
    (0..seats)
        .map(|raw| {
            let seat = SeatId::new(u8::try_from(raw).expect("a seat id"));
            let tally = world.tally(seat).expect("every seat has a tally");
            Combat {
                kills: tally.kills,
                shared: tally.shared_kills,
                losses: tally.losses,
                destroyed: tally.destroyed.raw(),
                scouts_lost: scouts_lost
                    .get(usize::try_from(raw).expect("a seat index"))
                    .copied()
                    .expect("a scout count per seat"),
            }
        })
        .collect()
}

/// Each seat's live scouts.
fn live_scouts(world: &pharmakos_sim::World, seats: u32) -> Vec<u32> {
    let units = world.units();
    let mut held = vec![0_u32; usize::try_from(seats).expect("a seat count")];
    for ((kind, owner), hp) in units
        .kinds()
        .iter()
        .zip(units.seats())
        .zip(units.hit_points())
    {
        if *kind == UnitKind::Scout.id()
            && hp.raw() > 0
            && let Some(slot) = held.get_mut(usize::from(*owner))
        {
            *slot = slot.saturating_add(1);
        }
    }
    held
}

/// The seats trailing in the final audit's order, or none while every
/// audited seat ties; and the seats still on the audit.
fn trailing(world: &pharmakos_sim::World) -> (Vec<u8>, Vec<u8>) {
    let audit = pharmakos_sim::audit::final_audit(world);
    let audited: Vec<u8> = audit.lines.iter().map(|line| line.seat.raw()).collect();
    let Some(lowest) = audit
        .lines
        .iter()
        .map(pharmakos_sim::audit::AuditLine::key)
        .min()
    else {
        return (Vec::new(), audited);
    };
    let last: Vec<u8> = audit
        .lines
        .iter()
        .filter(|line| line.key() == lowest)
        .map(|line| line.seat.raw())
        .collect();
    if last.len() == audited.len() {
        (Vec::new(), audited)
    } else {
        (last, audited)
    }
}

/// Add one tick's seat-against-seat damage to `figure`: `before` is the
/// counter matrix (`by * seats + owner`) at the last tick's end, read again
/// now; `standing` the trailing and audited seats at the last tick's end.
fn count_kingmaking(
    world: &pharmakos_sim::World,
    seats: u32,
    before: &mut [u64],
    standing: &(Vec<u8>, Vec<u8>),
    figure: &mut Kingmaking,
) {
    let (behind, audited) = standing;
    for by in 0..seats {
        for owner in 0..seats {
            if by == owner {
                continue;
            }
            let by_id = SeatId::new(u8::try_from(by).expect("a seat id"));
            let owner_id = SeatId::new(u8::try_from(owner).expect("a seat id"));
            let now = world.damage_dealt(by_id, owner_id);
            let cell =
                usize::try_from(by.saturating_mul(seats).saturating_add(owner)).expect("an index");
            let slot = before.get_mut(cell).expect("a cell per pair");
            let dealt = now.saturating_sub(*slot);
            *slot = now;
            if dealt == 0 {
                continue;
            }
            figure.dealt = figure.dealt.saturating_add(dealt);
            if behind.is_empty() {
                figure.no_trailing = figure.no_trailing.saturating_add(dealt);
                continue;
            }
            if behind.contains(&owner_id.raw()) {
                figure.at_trailing = figure.at_trailing.saturating_add(dealt);
            }
            // An even spread over the other seats still on the audit.
            let others: Vec<u8> = audited
                .iter()
                .copied()
                .filter(|seat| *seat != by_id.raw())
                .collect();
            let eligible = u64::try_from(others.len()).expect("a count");
            let trailing_others =
                u64::try_from(others.iter().filter(|seat| behind.contains(seat)).count())
                    .expect("a count");
            if let Some(share) = dealt
                .saturating_mul(1_000)
                .saturating_mul(trailing_others)
                .checked_div(eligible)
            {
                figure.expected_milli = figure.expected_milli.saturating_add(share);
            }
        }
    }
}

/// How the match ended, from the runner's outcome.
fn ended(surface: &Surface) -> String {
    surface
        .host()
        .expect("hosted")
        .runner()
        .outcome()
        .map_or_else(
            || String::from("not ended"),
            |outcome| {
                format!(
                    "{:?}, {} at tick {}",
                    outcome.reason,
                    outcome.winner.map_or_else(
                        || String::from("a shared win"),
                        |seat| format!("seat {} won", seat.raw())
                    ),
                    outcome.at.raw()
                )
            },
        )
}

/// Play one match through the gateway and read every seat at every Lull and
/// once at the end.
#[allow(
    clippy::too_many_lines,
    reason = "one loop over the rounds and the ticks, reading every figure the report needs as it goes; split, each piece would take the whole state as parameters"
)]
fn play(seed: u64, matchup: Matchup, rounds: u32, rules: &str, ladder: Vec<i32>) -> MatchRun {
    let label = format!("{}-{rounds}-{seed:016x}", matchup.name());
    let count = matchup.seats();
    let mut surface = open(&label, seed, rules, (rounds, count), ladder);
    let table = RulesTable::from_canonical_json(rules).expect("a rules table");
    let mut factory = EasyOperators::new(rules).expect("the operator reads the rules text");
    let mut easy = InProcessSeats::open(
        &mut surface,
        &seat_ids(count),
        matchup.template_seat(),
        &mut factory,
    )
    .expect("in-process seats");

    let mut seats: Vec<SeatRun> = seat_ids(count)
        .iter()
        .map(|seat| SeatRun {
            seat: seat.raw(),
            role: matchup.role(seat.raw()),
            rounds: Vec::new(),
            end: Reading::default(),
            end_combat: Combat::default(),
            first_kill: None,
        })
        .collect();
    let width = usize::try_from(count).expect("a seat count");
    let mut scouts_lost = vec![0_u32; width];
    let mut scouts = live_scouts(surface.host().expect("hosted").world(), count);
    let mut dealt = vec![0_u64; width.saturating_mul(width)];
    let mut kingmaking = (count >= 3).then(Kingmaking::default);
    let mut ticks = 0_u32;
    let mut played = 0_u32;
    let mut decided = false;
    for round in 1..=rounds {
        let combat = combat_now(&surface, count, &scouts_lost);
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
                combat: combat
                    .get(usize::from(run.seat))
                    .copied()
                    .expect("a tally per seat"),
                tally: Tally::default(),
            });
        }

        easy.plan(&mut surface);
        let template = matchup
            .template()
            .map(|template| submit_template(&mut surface, template));
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
        let mut standing = if kingmaking.is_some() {
            trailing(surface.host().expect("hosted").world())
        } else {
            (Vec::new(), Vec::new())
        };
        while let Some(report) = surface.step().expect("a tick") {
            ticks = ticks.saturating_add(1);
            decided |= report.match_ended;
            let world = surface.host().expect("hosted").world();
            // The scouts each seat lost this tick: a drop in its live count.
            let now = live_scouts(world, count);
            for ((lost, was), is) in scouts_lost.iter_mut().zip(&scouts).zip(&now) {
                *lost = lost.saturating_add(was.saturating_sub(*is));
            }
            scouts = now;
            for run in &mut seats {
                if run.first_kill.is_none()
                    && world
                        .tally(SeatId::new(run.seat))
                        .is_some_and(|tally| tally.kills > 0)
                {
                    run.first_kill = Some((ticks, round));
                }
            }
            if let Some(figure) = kingmaking.as_mut() {
                count_kingmaking(world, count, &mut dealt, &standing, figure);
                standing = trailing(world);
            }
            if report.segment_ended {
                break;
            }
        }
        played = round;
        surface.end_recap().expect("the recap closes");
        if decided || round == rounds {
            break;
        }
        surface.set_phase_remaining_ms(lull_ms(&table, round.saturating_add(1)));
        surface.open_lull().expect("the next Lull");
    }
    let combat = combat_now(&surface, count, &scouts_lost);
    for run in &mut seats {
        let (reading, tally) = read_seat(&mut surface, run.seat, true);
        if let Some(last) = run.rounds.last_mut() {
            last.tally = tally;
        }
        run.end = reading;
        run.end_combat = combat
            .get(usize::from(run.seat))
            .copied()
            .expect("a tally per seat");
    }
    let ended = ended(&surface);
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
        ended,
        kingmaking,
        seats,
    }
}

// ---------------------------------------------------------------------------
// The flags
// ---------------------------------------------------------------------------

/// Item 127 (3)'s three conditions, for one seat of one match.
///
/// PLACEHOLDER: the flags' readings of item 127 (3) — owner, at S2's demo, with `tune2`'s balance report (item 139 (1) ruling 5: they stand for S1, after `tune`'s report).
/// No decision fixes them, and each is a guess: a flat
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
#[allow(
    clippy::too_many_lines,
    reason = "one table per seat, each column written where it is labelled; split, the header and the rows would drift apart"
)]
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
    let _ = writeln!(out, "  ended: {}", run.ended);
    if let Some(figure) = run.kingmaking {
        let _ = writeln!(out, "  kingmaking: {}", kingmaking_words(figure));
    }
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
        let combat = seat
            .rounds
            .first()
            .map_or(seat.end_combat, |first| first.combat.until(seat.end_combat));
        let _ = writeln!(
            out,
            "    combat: {} kills ({} shared), {} losses, $ {} enemy value destroyed, {} scouts lost; first kill {}",
            combat.kills,
            combat.shared,
            combat.losses,
            combat.destroyed,
            combat.scouts_lost,
            first_kill_words(seat.first_kill),
        );
        let _ = writeln!(
            out,
            "    {:<5} {:<13} {:>8} {:>6} {:>7} {:>7} {:>7} {:>5} {:>6} {:>6} {:>4} {:>3}  {:<5} {:<24} {:<36} new(b/s/u) kills/shared/losses $destroyed",
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
            let combat = seat.combat_in(index);
            let _ = writeln!(
                out,
                "    {:<5} {:<13} {:>8} {:>6} {:>7} {:>7} {:>7} {:>5} {:>6} {:>6} {:>4} {:>3}  {:<5} {:<24} {:<36} {}/{}/{} {}/{}/{} $ {}",
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
                combat.kills,
                combat.shared,
                combat.losses,
                combat.destroyed,
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

/// One seat's use of its share of the fielding limits over one match
/// (register S1-37), or the most of several.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Usage {
    /// Beacons placed over the match (`beacon_placed`).
    placed: u32,
    /// Structures paid for over the match (`structure_queued`).
    paid: u32,
    /// Units fabricated over the match (`unit_fabricated`).
    fabricated: u32,
    /// The most beacons it held at a reading.
    beacons: u32,
    /// The most structures it held at a reading.
    structures: u32,
    /// The most units it held at a reading, the commander included.
    units: u32,
}

impl Usage {
    fn of(seat: &SeatRun) -> Usage {
        let mut usage = Usage::default();
        for round in &seat.rounds {
            usage.placed = usage.placed.saturating_add(round.tally.beacons_placed);
            usage.paid = usage.paid.saturating_add(round.tally.structures_queued);
            usage.fabricated = usage
                .fabricated
                .saturating_add(round.tally.units_fabricated);
        }
        let total =
            |map: &BTreeMap<String, u32>| map.values().copied().fold(0_u32, u32::saturating_add);
        for reading in seat
            .rounds
            .iter()
            .map(|round| &round.start)
            .chain(std::iter::once(&seat.end))
        {
            usage.beacons = usage.beacons.max(reading.beacons);
            usage.structures = usage.structures.max(total(&reading.structures));
            usage.units = usage.units.max(total(&reading.units));
        }
        usage
    }

    fn most(self, other: Usage) -> Usage {
        Usage {
            placed: self.placed.max(other.placed),
            paid: self.paid.max(other.paid),
            fabricated: self.fabricated.max(other.fabricated),
            beacons: self.beacons.max(other.beacons),
            structures: self.structures.max(other.structures),
            units: self.units.max(other.units),
        }
    }
}

/// The summary: per match-up and length, how many seats raised each flag.
#[allow(
    clippy::too_many_lines,
    reason = "one summary block after another, each a few lines; splitting it would only rename the sections"
)]
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
    // Each template seat on its own, since it is the one the plan pairs
    // against Easy.
    for kind in Template::ALL {
        let template: Vec<&SeatRun> = runs
            .iter()
            .filter(|run| run.matchup == Matchup::TemplateVsEasy(kind))
            .flat_map(|run| run.seats.iter())
            .filter(|seat| seat.seat == TEMPLATE_SEAT)
            .collect();
        if template.is_empty() {
            continue;
        }
        let seals: BTreeMap<&str, usize> = template
            .iter()
            .flat_map(|seat| seat.rounds.iter())
            .fold(BTreeMap::new(), |mut map, round| {
                let slot = map.entry(round.seal.as_str()).or_default();
                *slot = slot.saturating_add(1);
                map
            });
        let _ = writeln!(out, "{} seat's seals by round: {seals:?}", kind.role());
    }
    render_combat_summary(out, runs);
    // The per-seat share of the fielding limits (register S1-37): the most
    // one seat placed, paid for and fabricated over a whole match, and the
    // most beacons, structures and units it held at any reading.
    let usage = runs
        .iter()
        .flat_map(|run| run.seats.iter())
        .map(Usage::of)
        .fold(Usage::default(), Usage::most);
    let _ = writeln!(
        out,
        "per-seat share of the fielding limits (register S1-37), the most any one seat reached in \
         one match: placed {} beacons, paid for {} structures, fabricated {} units; held {} \
         beacons, {} structures, {} units (the commander included) at a reading",
        usage.placed, usage.paid, usage.fabricated, usage.beacons, usage.structures, usage.units,
    );
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

/// A seat's first kill, as the report words it.
fn first_kill_words(first: Option<(u32, u32)>) -> String {
    first.map_or_else(
        || String::from("never"),
        |(tick, round)| {
            format!(
                "in round {round}, {} s into the match's Pushes",
                tick_seconds(tick)
            )
        },
    )
}

/// Decision 36's figure, as the report words it.
fn kingmaking_words(figure: Kingmaking) -> String {
    let shares = figure.shares().map_or_else(
        || String::from("no damage was dealt while a seat trailed"),
        |(share, expected)| {
            format!(
                "{}% of it aimed at the trailing seat (an even spread over the other seats would \
                 give {}%)",
                tenths(i64::try_from(share).expect("a share")),
                tenths(i64::try_from(expected).expect("a share")),
            )
        },
    );
    format!(
        "{} HP dealt seat against seat, {} of it while no seat trailed; of the rest, {} HP, {shares}",
        figure.dealt, figure.no_trailing, figure.at_trailing,
    )
}

/// Combat per match-up, length and role: the plan's per-seat figures summed
/// over the seeds, the first kill's spread, and how the matches ended.
fn render_combat_summary(out: &mut String, runs: &[MatchRun]) {
    let _ = writeln!(
        out,
        "\n{:<24} {:<16} {:>6} {:>6} {:>6} {:>7} {:>7} {:>10} {:>7} {:>14} {:>10}",
        "combat: match-up",
        "seat",
        "rounds",
        "seats",
        "kills",
        "shared",
        "losses",
        "$ destroyed",
        "scouts",
        "killed (seats)",
        "first kill"
    );
    let groups: BTreeSet<(Matchup, u32, &str)> = runs
        .iter()
        .flat_map(|run| {
            run.seats
                .iter()
                .map(move |seat| (run.matchup, run.rounds, seat.role))
        })
        .collect();
    for (matchup, rounds, role) in groups {
        let seats: Vec<&SeatRun> = runs
            .iter()
            .filter(|run| run.matchup == matchup && run.rounds == rounds)
            .flat_map(|run| run.seats.iter())
            .filter(|seat| seat.role == role)
            .collect();
        let total = seats.iter().fold(Combat::default(), |sum, seat| {
            let combat = seat
                .rounds
                .first()
                .map_or(seat.end_combat, |first| first.combat.until(seat.end_combat));
            Combat {
                kills: sum.kills.saturating_add(combat.kills),
                shared: sum.shared.saturating_add(combat.shared),
                losses: sum.losses.saturating_add(combat.losses),
                destroyed: sum.destroyed.saturating_add(combat.destroyed),
                scouts_lost: sum.scouts_lost.saturating_add(combat.scouts_lost),
            }
        });
        let mut firsts: Vec<u32> = seats
            .iter()
            .filter_map(|seat| seat.first_kill.map(|(tick, _)| tick))
            .collect();
        firsts.sort_unstable();
        let median = firsts
            .get(firsts.len().checked_div(2).expect("two"))
            .map_or_else(
                || String::from("-"),
                |tick| format!("{} s", tick_seconds(*tick)),
            );
        let _ = writeln!(
            out,
            "{:<24} {:<16} {:>6} {:>6} {:>6} {:>7} {:>7} {:>10} {:>7} {:>14} {:>10}",
            matchup.name(),
            role,
            rounds,
            seats.len(),
            total.kills,
            total.shared,
            total.losses,
            total.destroyed,
            total.scouts_lost,
            firsts.len(),
            median,
        );
    }
    let _ = writeln!(
        out,
        "(sums over the seeds; `killed (seats)` = seats that made a kill; `first kill` = the median \
         seat's, in seconds of Push played)"
    );
    render_endings(out, runs);
}

/// How the matches of each match-up and length ended, from the runner's
/// outcome: the reason, and whether seat 0, another seat or nobody alone won.
fn render_endings(out: &mut String, runs: &[MatchRun]) {
    let endings: BTreeSet<(Matchup, u32)> =
        runs.iter().map(|run| (run.matchup, run.rounds)).collect();
    for (matchup, rounds) in endings {
        let by_reason: BTreeMap<String, usize> = runs
            .iter()
            .filter(|run| run.matchup == matchup && run.rounds == rounds)
            .fold(BTreeMap::new(), |mut map, run| {
                let reason = run
                    .ended
                    .split(", ")
                    .next()
                    .unwrap_or(run.ended.as_str())
                    .to_owned();
                let winner = if run.ended.contains("shared") {
                    "shared"
                } else if run.ended.contains("seat 0 won") {
                    "seat 0"
                } else {
                    "another seat"
                };
                let slot = map.entry(format!("{reason} ({winner})")).or_default();
                *slot = slot.saturating_add(1);
                map
            });
        let _ = writeln!(
            out,
            "how {} / {rounds} rounds ended: {by_reason:?}",
            matchup.name()
        );
    }
}

/// Decision 36's section: the three-seat run, per seed and in sum.
fn render_kingmaking(out: &mut String, runs: &[MatchRun]) {
    let three: Vec<&MatchRun> = runs.iter().filter(|run| run.kingmaking.is_some()).collect();
    if three.is_empty() {
        return;
    }
    let _ = writeln!(
        out,
        "\nKingmaking (S2's plan, decision 36; spec section 19): {} three-seat matches of Easy, \
         Easy and Easy. The share of all seat-against-seat damage aimed at the seat trailing in \
         the Ledger's standing (the final audit's order) when it was dealt; damage dealt while \
         every seat tied is counted apart. No alarm band: a figure for the owner to read.",
        three.len()
    );
    let mut sum = Kingmaking::default();
    for run in &three {
        let figure = run.kingmaking.unwrap_or_default();
        sum = sum.add(figure);
        let _ = writeln!(
            out,
            "  seed 0x{:016x}: {}",
            run.seed,
            kingmaking_words(figure)
        );
    }
    let _ = writeln!(out, "  in sum: {}", kingmaking_words(sum));
}

/// The spawn travel distance between the generator's spawn pairs, in raider
/// and commander time (the generator's own `MapReport`), against spec section
/// 19's "about 1.5 early Pushes".
fn render_spawn_travel(out: &mut String, table: &RulesTable) {
    let block = table
        .message()
        .r#match
        .as_ref()
        .expect("the rules table's match block");
    let early_ms = i64::from(
        block
            .segment_lengths_ms
            .first()
            .copied()
            .expect("a segment ladder"),
    );
    let target_s = early_ms
        .saturating_mul(3)
        .checked_div(2_000)
        .expect("a divisor");
    let _ = writeln!(
        out,
        "\nSpawn travel (spec section 19: spawns about 1.5 early Pushes of raider travel apart; an \
         early Push is {} s, so {target_s} s): the closest occupied pair, from the generator's own \
         report",
        early_ms.checked_div(1_000).expect("a divisor")
    );
    for seats in [2_u32, 3] {
        let mut raider: Vec<i64> = Vec::new();
        let mut commander: Vec<i64> = Vec::new();
        for seed in seeds() {
            let report = pharmakos_sim::mapgen::generate(seed, table, seats)
                .expect("the map generates")
                .report;
            raider.push(report.separation_raider_seconds);
            commander.push(report.separation_commander_seconds);
        }
        let range = |values: &[i64]| {
            format!(
                "{}..{} s",
                values.iter().min().copied().unwrap_or(0),
                values.iter().max().copied().unwrap_or(0)
            )
        };
        let short = raider.iter().filter(|value| **value < target_s).count();
        let _ = writeln!(
            out,
            "  {seats} seats, {} seeds: raider {} ({} below {target_s} s), commander {}",
            raider.len(),
            range(&raider),
            short,
            range(&commander),
        );
    }
}

/// Reach memory against Survey's refresh interval, and Survey's cell against
/// the scout's vision (decisions-log item 13; `surv`'s two markers).
fn render_survey(out: &mut String, table: &RulesTable) {
    let reach = table
        .message()
        .verifier
        .as_ref()
        .map(|block| i64::from(block.reach_memory_ms))
        .expect("verifier.reach_memory_ms");
    let refresh = pharmakos_sim::survey::REFRESH_INTERVAL.raw();
    let refresh_s = i64::from(refresh).checked_div(1_000).expect("a divisor");
    let scout = table
        .unit(pharmakos_sim::tables::UnitKind::Scout)
        .vision_radius_voxels;
    let cell = pharmakos_sim::survey::CELL_VOXELS;
    // Twice the half-diagonal, squared: 2 * cell^2, against (2 * vision)^2.
    let covered = i64::from(cell)
        .saturating_mul(i64::from(cell))
        .saturating_mul(2)
        <= i64::from(scout)
            .saturating_mul(i64::from(scout))
            .saturating_mul(4);
    let _ = writeln!(
        out,
        "\nSurvey: a sighting is due a refresh at {} s and forgotten at `verifier.reach_memory_ms` \
         {} s, so it is due {} times before it leaves reach memory; the explored cell is {cell} \
         columns square, and a scout of vision {scout} on its centre {} all of it",
        refresh_s,
        reach.checked_div(1_000).expect("a divisor"),
        reach
            .checked_div(i64::from(refresh))
            .expect("a refresh interval above zero"),
        if covered { "sees" } else { "does not see" },
    );
}

/// The whole report: a header, the summary, decision 16's `expand_east`
/// column when it was measured, then every match.
fn render(
    runs: &[MatchRun],
    generator_cost: i64,
    rules_hash: &str,
    expand_east: Option<&ExpandEast>,
    table: Option<&RulesTable>,
) -> String {
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
    render_kingmaking(&mut out, runs);
    if let Some(table) = table {
        render_spawn_travel(&mut out, table);
        render_survey(&mut out, table);
    }
    if let Some(column) = expand_east {
        out.push('\n');
        render_expand_east(&mut out, column);
    }
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
// Decision 16's column: the spec's `expand_east`
// ---------------------------------------------------------------------------

/// The committed scenario the column reads, relative to the root.
const EXPAND_EAST_SCENARIO: &str = "scenarios/skeleton/expand-east-segment.scenario.jsonc";

/// The spec's worked example, which that scenario seals for both seats.
const EXPAND_EAST_PLAYBOOK: &str = "examples/playbooks/expand_east.jsonc";

/// Spec section 4's walking range for the commander, in **tenths of a voxel**
/// a second: 1 to 1.5 voxels a second (item 90). The column turns it into cost
/// units a second through the table's own `locomotion.step_cost_cardinal`
/// ([`spec_cost_range`]), so the range follows the table if the step cost
/// moves.
const SPEC_COMMANDER_TENTHS_OF_VOXELS_PER_SECOND: std::ops::RangeInclusive<i64> = 10..=15;

/// Spec section 4's walking range in cost units a second at a cardinal step
/// cost of `cardinal`: each end is tenths of a voxel times the cost of one
/// voxel, over ten. `None` when the arithmetic overflows or a bound is not a
/// whole number of cost units, which no committed table produces.
fn spec_cost_range(cardinal: i64) -> Option<std::ops::RangeInclusive<i64>> {
    let convert = |tenths: i64| {
        let scaled = tenths.checked_mul(cardinal)?;
        (scaled.checked_rem(10)? == 0).then(|| scaled.checked_div(10))?
    };
    Some(
        convert(*SPEC_COMMANDER_TENTHS_OF_VOXELS_PER_SECOND.start())?
            ..=convert(*SPEC_COMMANDER_TENTHS_OF_VOXELS_PER_SECOND.end())?,
    )
}

/// How one seat's first route step ended, read from the scenario's events.
#[derive(Clone, PartialEq, Eq, Debug)]
enum FirstStep {
    /// `step_completed`: the commander arrived.
    Arrived { tick: u32 },
    /// `step_failed`. The scenario's event list carries the kind and not the
    /// value, so the reason (a timeout, an illegal site, another refusal) is
    /// not read here and the report does not name one.
    Failed { tick: u32 },
    /// `step_skipped`.
    Skipped { tick: u32 },
    /// The step never ended inside the segment.
    Unfinished,
}

/// How seat `seat`'s first route step ended: the first `step_completed`,
/// `step_failed` or `step_skipped` of that seat after its first
/// `step_started`. The example has no rule that can fire before combat, so
/// the first ending after the route starts is the first step's.
fn first_step(events: &[(u32, String, Option<u8>)], seat: u8) -> FirstStep {
    let mut started = false;
    for (tick, kind, whose) in events {
        if *whose != Some(seat) {
            continue;
        }
        match kind.as_str() {
            "step_started" => started = true,
            "step_completed" if started => return FirstStep::Arrived { tick: *tick },
            "step_failed" if started => return FirstStep::Failed { tick: *tick },
            "step_skipped" if started => return FirstStep::Skipped { tick: *tick },
            _ => {}
        }
    }
    FirstStep::Unfinished
}

/// One seat's line of the column.
#[derive(Clone, PartialEq, Eq, Debug)]
struct ExpandEastSeat {
    seat: u8,
    /// How the scenario's first step ended.
    first: FirstStep,
    /// `estimate_route` from the seat's core to the site, game ms, or `None`
    /// when the gateway answered that the site is unreachable.
    estimate_ms: Option<i64>,
}

/// Decision 16's column, measured.
#[derive(Clone, PartialEq, Eq, Debug)]
struct ExpandEast {
    /// The first step's site and timeout, read from the example.
    site: [i32; 3],
    timeout_ms: i64,
    /// `commander.cost_per_second` in the committed table.
    cost_per_second: i64,
    /// `locomotion.step_cost_cardinal` in the committed table: the cost of one
    /// voxel, which turns a cost rate into voxels a second.
    step_cost_cardinal: i64,
    seed: u64,
    seats: Vec<ExpandEastSeat>,
}

/// The example's first step: its site and its timeout. Both are read from the
/// file, so the column follows the example if the owner moves it.
fn expand_east_first_step() -> ([i32; 3], i64) {
    let text = std::fs::read_to_string(root().join(EXPAND_EAST_PLAYBOOK)).expect("the example");
    let canonical = pharmakos_plan_core::canonical::canonical_json(&text).expect("canonical");
    let playbook: pharmakos_proto::gp::v1::Playbook =
        pharmakos_proto::json::decode(&canonical).expect("a playbook");
    let step = playbook
        .declarative
        .as_ref()
        .and_then(|declarative| declarative.route.first())
        .cloned()
        .expect("the example's first step");
    let Some(pharmakos_proto::gp::v1::step::Kind::Move(walk)) = step.kind else {
        panic!("the example's first step is a move");
    };
    let Some(pharmakos_proto::gp::v1::location::Place::Voxel(voxel)) =
        walk.to.and_then(|to| to.place)
    else {
        panic!("the example's first step walks to a voxel");
    };
    ([voxel.x, voxel.y, voxel.z], i64::from(step.timeout_ms))
}

/// Measure the column: play the committed scenario, and ask each seat's own
/// `estimate_route` for the walk from its core to the site on the same map.
fn expand_east(rules: &str, table: &RulesTable) -> ExpandEast {
    let root = root();
    let scenario = pharmakos_gamectl::scenario::load(&root, Path::new(EXPAND_EAST_SCENARIO))
        .expect("the committed expand-east-segment scenario loads");
    let played = pharmakos_gamectl::scenario::run::play(&root, &scenario)
        .expect("the committed expand-east-segment scenario plays");
    let events: Vec<(u32, String, Option<u8>)> = played
        .events
        .iter()
        .map(|fired| (fired.tick, fired.kind.clone(), fired.seat))
        .collect();
    let (site, timeout_ms) = expand_east_first_step();
    let seed = scenario.seed;
    let seats = expand_east_estimates(rules, seed, site)
        .into_iter()
        .map(|(seat, estimate_ms)| ExpandEastSeat {
            seat,
            first: first_step(&events, seat),
            estimate_ms,
        })
        .collect();
    ExpandEast {
        site,
        timeout_ms,
        cost_per_second: table
            .message()
            .commander
            .as_ref()
            .map(|block| i64::from(block.cost_per_second))
            .expect("the rules table's commander block"),
        step_cost_cardinal: i64::from(table.step_cardinal()),
        seed,
        seats,
    }
}

/// The column's gateway half: each seat's own `estimate_route` from its core
/// to `site`, in a Lull of a match on `seed`, as `(seat, game ms)`, the ms
/// `None` when the answer says the site is unreachable.
fn expand_east_estimates(rules: &str, seed: u64, site: [i32; 3]) -> Vec<(u8, Option<i64>)> {
    let [x, y, z] = site;
    let mut surface = open("expand-east", seed, rules, (1, 2), Vec::new());
    seat_ids(2)
        .iter()
        .map(|seat| {
            let token = in_process_token(&mut surface, seat.raw());
            let answer = call(
                &mut surface,
                &token,
                "estimate_route",
                Json::Object(
                    [(
                        String::from("waypoints"),
                        Json::Array(vec![
                            object(vec![(
                                "beacon_anchor",
                                object(vec![("beacon_id", Json::String(String::from("b_00")))]),
                            )]),
                            object(vec![(
                                "voxel",
                                object(vec![
                                    ("x", Json::Number(x.to_string())),
                                    ("y", Json::Number(y.to_string())),
                                    ("z", Json::Number(z.to_string())),
                                ]),
                            )]),
                        ]),
                    )]
                    .into_iter()
                    .collect(),
                ),
            );
            // A refusal fails the run rather than reading as "no route": the
            // call is well formed, so a refusal means the reader is wrong. An
            // unreachable site is the answer's own `reachable` (false omitted).
            let answer = result(&answer, "estimate_route from the core to the site");
            let estimate_ms = (answer.get("reachable") == Some(&Json::Bool(true)))
                .then(|| required(&answer, "ms"));
            (seat.raw(), estimate_ms)
        })
        .collect()
}

/// The speed, in cost units a second, a walk of `estimate_ms` at
/// `cost_per_second` would need to fit `timeout_ms`, rounded up.
fn speed_needed(estimate_ms: i64, cost_per_second: i64, timeout_ms: i64) -> Option<i64> {
    let cost = estimate_ms.checked_mul(cost_per_second)?;
    let whole = cost.checked_div(timeout_ms)?;
    let rest = cost.checked_rem(timeout_ms)?;
    Some(if rest > 0 {
        whole.saturating_add(1)
    } else {
        whole
    })
}

/// Seconds, to one decimal place, from game milliseconds.
fn seconds(ms: i64) -> String {
    let whole = ms.checked_div(1_000).expect("a non-zero divisor");
    let tenths = ms
        .checked_rem(1_000)
        .and_then(|rest| rest.checked_div(100))
        .expect("a non-zero divisor");
    format!("{whole}.{tenths}")
}

/// Whole seconds of game time, from a tick at the sim's tick rate.
fn tick_seconds(tick: u32) -> u32 {
    tick.checked_div(TICK_HZ).expect("a non-zero divisor")
}

/// A value given in tenths, to one decimal place.
fn tenths(value: i64) -> String {
    let whole = value.checked_div(10).expect("a non-zero divisor");
    let rest = value.checked_rem(10).expect("a non-zero divisor");
    format!("{whole}.{rest}")
}

/// Voxels a second, in tenths, at `cost_per_second` and a cardinal step cost of
/// `cardinal`, rounded down.
fn voxel_tenths_per_second(cost_per_second: i64, cardinal: i64) -> Option<i64> {
    cost_per_second.checked_mul(10)?.checked_div(cardinal)
}

fn render_expand_east(out: &mut String, column: &ExpandEast) {
    let range = spec_cost_range(column.step_cost_cardinal)
        .expect("spec section 4's range is whole cost units at the table's cardinal step cost");
    let _ = writeln!(
        out,
        "expand_east (decision 16): does the spec's worked example reach its first site {:?} within \
         its {} s timeout? Seed 0x{:016x} (`{EXPAND_EAST_SCENARIO}`), commander speed {} cost/s \
         ({} voxels/s); spec section 4's range is {}..={} cost/s.",
        column.site,
        seconds(column.timeout_ms),
        column.seed,
        column.cost_per_second,
        voxel_tenths_per_second(column.cost_per_second, column.step_cost_cardinal)
            .map_or_else(|| String::from("?"), tenths),
        range.start(),
        range.end(),
    );
    for seat in &column.seats {
        let outcome = match seat.first {
            FirstStep::Arrived { tick } => {
                format!("yes, arrived at tick {tick} ({} s)", tick_seconds(tick))
            }
            FirstStep::Failed { tick } => format!(
                "no, the step failed at tick {tick} ({} s)",
                tick_seconds(tick)
            ),
            FirstStep::Skipped { tick } => format!("no, skipped at tick {tick}"),
            FirstStep::Unfinished => String::from("no, the step never ended in the segment"),
        };
        let estimate = match seat.estimate_ms {
            Some(ms) => {
                let needed = speed_needed(ms, column.cost_per_second, column.timeout_ms);
                let inside = needed.is_some_and(|speed| range.contains(&speed));
                format!(
                    "estimate_route from the core: {} s at {} cost/s; fitting {} s needs {} cost/s ({})",
                    seconds(ms),
                    column.cost_per_second,
                    seconds(column.timeout_ms),
                    needed.map_or_else(|| String::from("?"), |speed| speed.to_string()),
                    if inside {
                        "inside the spec's range"
                    } else {
                        "outside the spec's range"
                    }
                )
            }
            None => String::from("estimate_route from the core: no route"),
        };
        let _ = writeln!(out, "  seat {}: {outcome}; {estimate}", seat.seat);
    }
}

// ---------------------------------------------------------------------------
// The check, and its smoke test
// ---------------------------------------------------------------------------

/// How many matches the check plays at once: two, which the box it was
/// written on holds beside the other lanes' builds. The report is the same
/// whatever the number, since each match is its own pure run and the results
/// are put back in the list's order.
const WORKERS: usize = 2;

/// Every match the check plays: each two-seat match-up on every seed at both
/// lengths, then the three-seat run on every seed.
fn schedule() -> Vec<(u64, Matchup, u32)> {
    let mut list = Vec::new();
    for matchup in Matchup::TWO_SEAT {
        for rounds in ROUND_COUNTS {
            for seed in seeds() {
                list.push((seed, matchup, rounds));
            }
        }
    }
    for seed in seeds() {
        list.push((seed, Matchup::ThreeSeats, THREE_SEAT_ROUNDS));
    }
    list
}

/// The 220 matches. Writes the report and prints its summary; asserts only
/// what makes the report trustworthy (every match was played, every reading
/// was answered), never a balance condition -- a flag, or a match decided
/// before its round limit, is the owner's evidence, not a red test.
#[test]
#[ignore = "the headless balance check: 220 matches, an hour or more in release; run it by name"]
fn balance_check_writes_its_report() {
    let rules = rules_json();
    let table = RulesTable::from_canonical_json(&rules).expect("a rules table");
    let cost = generator_cost(&table);
    let list = schedule();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let done = std::sync::Mutex::new(Vec::<(usize, MatchRun)>::new());
    std::thread::scope(|scope| {
        for _ in 0..WORKERS {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some((seed, matchup, rounds)) = list.get(index).copied() else {
                        break;
                    };
                    let run = play(seed, matchup, rounds, &rules, Vec::new());
                    eprintln!(
                        "[{}/{}] {} / {rounds} rounds / seed 0x{seed:016x}: {} ticks",
                        index.saturating_add(1),
                        list.len(),
                        matchup.name(),
                        run.ticks
                    );
                    done.lock().expect("no worker panicked").push((index, run));
                }
            });
        }
    });
    let mut done = done.into_inner().expect("no worker panicked");
    done.sort_by_key(|(index, _)| *index);
    let runs: Vec<MatchRun> = done.into_iter().map(|(_, run)| run).collect();
    let column = expand_east(&rules, &table);
    let text = render(
        &runs,
        cost,
        &pharmakos_sim::hex(table.rules_hash()),
        Some(&column),
        Some(&table),
    );
    let path = report_path();
    std::fs::create_dir_all(path.parent().expect("a folder")).expect("the report folder");
    std::fs::write(&path, &text).expect("the report");
    let summary: String = text
        .split("\n\n")
        .skip(1)
        .take(6)
        .collect::<Vec<_>>()
        .join("\n\n");
    eprintln!("{summary}\nthe report: {}", path.display());
    assert_eq!(
        runs.len(),
        SEED_COUNT * (Matchup::TWO_SEAT.len() * ROUND_COUNTS.len() + 1)
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
        Matchup::TemplateVsEasy(Template::HoldAndBuild),
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
        None,
        None,
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
    let mut surface = open("easy-own", SCENARIO_SEED, &rules, (1, 2), Vec::new());
    let mut factory = EasyOperators::new(&rules).expect("the operator reads the rules text");
    let mut easy = InProcessSeats::open(&mut surface, &seat_ids(2), None, &mut factory)
        .expect("in-process seats");
    easy.plan(&mut surface);
    assert!(surface.begin_push().expect("the Push opens"));
    for seat in seat_ids(2) {
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

/// Decision 16's column reads a seat's first step and the speed it would need
/// the way the report says it does, on made-up events and figures.
#[test]
fn the_expand_east_column_reads_the_first_step() {
    let events = vec![
        (0, String::from("plan_sealed"), Some(0)),
        (1, String::from("step_started"), Some(0)),
        (1, String::from("step_started"), Some(1)),
        (900, String::from("step_completed"), Some(1)),
        (2_401, String::from("step_failed"), Some(0)),
        (2_406, String::from("step_failed"), Some(0)),
    ];
    assert_eq!(first_step(&events, 0), FirstStep::Failed { tick: 2_401 });
    assert_eq!(first_step(&events, 1), FirstStep::Arrived { tick: 900 });
    assert_eq!(first_step(&events, 2), FirstStep::Unfinished);
    // 150 s at 10 cost/s is 1 500 cost units; 120 s needs 12.5, so 13.
    assert_eq!(speed_needed(150_000, 10, 120_000), Some(13));
    assert_eq!(speed_needed(120_000, 10, 120_000), Some(10));
    assert_eq!(speed_needed(1, 10, 0), None);
    assert_eq!(seconds(120_000), "120.0");
    assert_eq!(seconds(1_250), "1.2");
    // Spec section 4's 1 to 1.5 voxels a second, at the item-59 step cost of
    // 10 a voxel and at a doubled one.
    assert_eq!(spec_cost_range(10), Some(10..=15));
    assert_eq!(spec_cost_range(20), Some(20..=30));
    assert_eq!(spec_cost_range(3), None, "1.5 voxels at 3 a voxel is 4.5");
    assert_eq!(
        voxel_tenths_per_second(10, 10).map(tenths),
        Some(String::from("1.0"))
    );
    assert_eq!(
        voxel_tenths_per_second(15, 10).map(tenths),
        Some(String::from("1.5"))
    );
    assert_eq!(tick_seconds(2_401), 120);
}

/// Decision 16's column's gateway half, through the reader the report uses:
/// every seat's own `estimate_route` from its core to the example's first
/// site answers, on the expand-east scenario's seed. A refusal fails here
/// (the reader turns one into a panic), so a change to what the gateway
/// answers fails `cargo xtask ci` rather than the next 80-match run.
#[test]
fn the_expand_east_column_asks_each_seats_estimate_route() {
    let root = root();
    let scenario = pharmakos_gamectl::scenario::load(&root, Path::new(EXPAND_EAST_SCENARIO))
        .expect("the committed expand-east-segment scenario loads");
    let (site, _timeout_ms) = expand_east_first_step();
    let estimates = expand_east_estimates(&rules_json(), scenario.seed, site);
    assert_eq!(
        estimates.iter().map(|(seat, _)| *seat).collect::<Vec<u8>>(),
        seat_ids(2)
            .iter()
            .map(|seat| seat.raw())
            .collect::<Vec<u8>>(),
        "one answer a seat, in seat order"
    );
    for (seat, estimate_ms) in estimates {
        assert!(
            estimate_ms.is_some_and(|ms| ms > 0),
            "seat {seat}: the site is reachable from the core on this map, at a positive time;              got {estimate_ms:?}"
        );
    }
}
