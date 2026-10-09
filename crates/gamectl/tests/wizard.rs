// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The wizard's live answer: `instantiate_template{suggested: true}` with the
//! real Easy advisor, its FULL report and its prose (decisions-log item 124
//! (5) (l); S1's plan, task `oper`).
//!
//! The gateway's own goldens answer the wizard from a scripted advisor
//! (`crates/gateway/tests/advice.rs`'s `instantiate_suggested`), because the
//! real operator is reachable only from here, where the adapter is
//! (`crate::host::EasyOperators`; AGENTS.md section 3 rule 3). This suite is
//! the other half: a match hosted as `gamectl host` hosts the lobby's
//! (`godot/scripts/host_link.gd`: seed `0x00000000ca5caded`, two seats, the
//! rules table's own segment ladder, three rounds), seat 0 the human's and
//! advised by Easy, seat 1 played by Easy, each through its own in-process
//! token, and the human seat's wizard asking for each shipped template with
//! the operator's suggestion applied.
//!
//! For each template it writes four goldens under
//! `tests/golden/operator/wizard/<template_id>/`:
//!
//! * `playbook.jsonc` -- the playbook the wizard's Use puts in the editor,
//!   byte for byte as the gateway answered it;
//! * `parameters.json` -- every declared parameter with the value applied and
//!   whether it is the operator's, and the operator's "why";
//! * `report.json` -- the FULL `verify_plan` report the seat gets for that
//!   playbook, `report_hash` included. It must qualify;
//! * `prose.txt` -- the `render_plan` prose, the English rendering the rule
//!   list shows.
//!
//! The `_status` footer is left out of every one: it carries the host clock,
//! which is the lobby's and not the operator's.
//!
//! The same lobby, played on, is where the other half of the advisor's
//! promise is checked: a human seat that never edits is filed the operator's
//! safe playbook every round, whose route sets the core's dig depth
//! (decisions-log item 133 (3) (a)), so the seat is still earning in the
//! lobby's last round
//! (`a_seat_nobody_edits_is_still_earning_in_the_lobbys_third_round`).
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::path::{Path, PathBuf};

use pharmakos_gamectl::host::{EasyOperators, LIBRARY_PATH};
use pharmakos_gateway::fog::{Audience, FogPolicy};
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

/// The lobby's seed (`host_link.gd`), the scenario files' seed.
const SEED: u64 = 0x0000_0000_ca5c_aded;

/// The lobby's seats: the human at seat 0, the built-in operator at seat 1
/// (`host_link.gd`'s `MATCH_SEATS` and `HUMAN_SEAT`).
const SEATS: u32 = 2;

/// The human's seat.
const HUMAN: u8 = 0;

/// The lobby's round limit (`host_link.gd`'s `ROUND_LIMIT`).
const ROUND_LIMIT: u32 = 3;

/// The templates the library ships, in the order the wizard lists them.
const TEMPLATES: [&str; 3] = ["expand_and_mine", "hold_and_build", "safe_playbook"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/gamectl sits two levels below the root")
        .to_path_buf()
}

/// The cargo target directory, honouring `CARGO_TARGET_DIR`.
fn target_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .parent()
        .expect("CARGO_TARGET_TMPDIR has a parent")
        .to_path_buf()
}

/// A match in its opening Lull, hosted as `serve::run` hosts the lobby's,
/// with Easy advising the human seat and playing the other, as `gamectl host`
/// seats them.
fn lobby() -> Surface {
    let rules = std::fs::read_to_string(root().join("rules").join("rules.v1.json"))
        .expect("the rules text");
    let host = Host::open_from(
        &rules,
        SEED,
        SEATS,
        &Settings {
            // Empty is the rules table's own ladder: the lobby's `-`.
            segment_lengths_ms: Vec::new(),
            round_limit: ROUND_LIMIT,
            units_per_seat: 0,
        },
        Some(root().join(LIBRARY_PATH)),
    )
    .expect("the lobby's match");
    let table = RulesTable::from_canonical_json(&rules).expect("a rules table");
    let seats: Vec<SeatId> = (0..SEATS)
        .filter_map(|raw| u8::try_from(raw).ok())
        .map(SeatId::new)
        .collect();
    let mut surface = Surface::new(
        &format!("oper-wizard-{}", std::process::id()),
        SEED,
        table,
        FogPolicy::fogged(),
        &seats,
    )
    .expect("a surface");
    surface.attach(host).expect("attached");
    surface.open_lull().expect("the opening Lull");
    let lull = surface.lull_length(1);
    surface.set_phase_remaining_ms(lull);
    let mut factory = EasyOperators::new(&rules).expect("the operator reads the rules text");
    let mut easy = InProcessSeats::open(&mut surface, &seats, Some(HUMAN), &mut factory)
        .expect("in-process seats");
    assert_eq!(easy.advised_seats(), [HUMAN], "the human seat is advised");
    easy.plan(&mut surface);
    surface
}

/// The human seat's own token.
fn human_token(surface: &mut Surface) -> Token {
    let tick = surface.time().tick;
    let held = surface.match_id().to_owned();
    let (token, handle) = surface
        .tokens()
        .mint(
            Subject::Seat(SeatId::new(HUMAN)),
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

/// One call through the token, with the production vision, and its
/// `result`.
fn call(surface: &mut Surface, token: &Token, method: &str, params: Json) -> Json {
    let vision = surface.host().map_or_else(
        |_| SphereVision::default(),
        |host| SphereVision::of(host.world()),
    );
    let response = surface.call(
        Some(token),
        &Request {
            id: Json::Number(String::from("1")),
            method: method.to_owned(),
            params,
        },
        &vision,
    );
    response
        .get("result")
        .cloned()
        .unwrap_or_else(|| panic!("{method} was refused: {response:?}"))
}

/// An object's members, the `_status` footer left out.
fn without_status(value: &Json) -> Json {
    match value {
        Json::Object(entries) => Json::Object(
            entries
                .iter()
                .filter(|(key, _)| key != "_status")
                .cloned()
                .collect(),
        ),
        other => other.clone(),
    }
}

fn text_of(value: &Json, key: &str) -> String {
    match value.get(key) {
        Some(Json::String(text)) => text.clone(),
        other => panic!("`{key}` is not a string: {other:?}"),
    }
}

/// The pointers a template declares, read from the library's own file, its
/// comments taken out (a comment line in it is whole-line, `//` first).
fn declared_pointers(template: &str) -> Vec<String> {
    let text = std::fs::read_to_string(root().join(LIBRARY_PATH).join(format!("{template}.jsonc")))
        .expect("the template file");
    let body: String = text
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join(
            "
",
        );
    let json = pharmakos_proto::json::read(&body).expect("the template reads");
    let Some(Json::Array(parameters)) = json.get("meta").and_then(|meta| meta.get("parameters"))
    else {
        panic!("{template}: declares no parameters");
    };
    parameters
        .iter()
        .map(|parameter| text_of(parameter, "pointer"))
        .collect()
}

/// What the operator suggested, against what the template declares: every
/// suggested value is for a declared parameter, and Hold & Build's vent is
/// the operator's and a name (Easy's row: fixed targets only).
fn check_suggested(template: &str, parameters: &[Json]) {
    // Every value the operator suggested is for a parameter the
    // template declares.
    let declared_here = declared_pointers(template);
    for parameter in parameters {
        if parameter.get("suggested") == Some(&Json::Bool(true)) {
            let pointer = text_of(parameter, "pointer");
            assert!(
                declared_here.contains(&pointer),
                "{template}: the operator suggested {pointer}, undeclared: {declared_here:?}"
            );
        }
    }
    // Hold & Build's vent is the operator's, and it is a name (Easy's
    // row: fixed targets only): the live wizard always pins one.
    if template == "hold_and_build" {
        let covering = parameters
            .iter()
            .find(|parameter| text_of(parameter, "pointer").ends_with("/place_beacon/at/covering"))
            .expect("Hold & Build declares its vent");
        assert_eq!(
            covering.get("suggested"),
            Some(&Json::Bool(true)),
            "the vent is the operator's: {covering:?}"
        );
        let value =
            pharmakos_proto::json::read(&text_of(covering, "value")).expect("the value reads");
        assert!(
            matches!(value.get("feature_id"), Some(Json::String(name)) if name.starts_with("vent_")),
            "a vent by name: {value:?}"
        );
    }
}

/// Write one fresh golden of the `operator` area.
fn write_golden(case: &str, file: &str, text: &str) {
    assert!(
        !text.contains('\r') && text.ends_with('\n'),
        "{case}/{file}: LF endings and a trailing newline"
    );
    let folder = target_dir()
        .join("golden")
        .join("operator")
        .join("wizard")
        .join(case);
    std::fs::create_dir_all(&folder).expect("the golden folder");
    std::fs::write(folder.join(format!("actual.{file}")), text).expect("the fresh golden");
}

/// The wizard, for each shipped template, with the live Easy advisor: the
/// suggested playbook qualifies FULL in the seat's own Lull, every suggested
/// value names a declared parameter, Easy says why, and the four goldens are
/// written.
#[test]
fn the_wizards_live_suggestion_qualifies_and_is_goldened_for_every_template() {
    let mut surface = lobby();
    let token = human_token(&mut surface);
    for template in TEMPLATES {
        let answer = call(
            &mut surface,
            &token,
            "instantiate_template",
            Json::Object(vec![
                (
                    String::from("template_id"),
                    Json::String(template.to_owned()),
                ),
                (String::from("suggested"), Json::Bool(true)),
            ]),
        );
        let playbook = text_of(&answer, "playbook_jsonc");
        let why = text_of(&answer, "why");
        assert!(
            why.starts_with("Easy, seat 0, round 1, seed 0x00000000ca5caded"),
            "{template}: Easy's why, opening with the seed it recorded: {why}"
        );
        let Some(Json::Array(parameters)) = answer.get("parameters") else {
            panic!("{template}: the answer lists its parameters");
        };
        assert!(
            !parameters.is_empty(),
            "{template}: a template declares its parameters"
        );
        check_suggested(template, parameters);

        let verified = call(
            &mut surface,
            &token,
            "verify_plan",
            Json::Object(vec![
                (
                    String::from("playbook_jsonc"),
                    Json::String(playbook.clone()),
                ),
                (String::from("depth"), Json::String(String::from("full"))),
            ]),
        );
        let report = verified.get("report").cloned().expect("a report");
        assert_eq!(
            report.get("qualifies"),
            Some(&Json::Bool(true)),
            "{template}: the wizard's suggestion qualifies FULL: {report:?}"
        );
        let rendered = call(
            &mut surface,
            &token,
            "render_plan",
            Json::Object(vec![(
                String::from("playbook_jsonc"),
                Json::String(playbook.clone()),
            )]),
        );
        let prose = text_of(&rendered, "prose");
        assert!(!prose.is_empty(), "{template}: prose");

        let declared = without_status(&Json::Object(vec![
            (String::from("parameters"), Json::Array(parameters.clone())),
            (String::from("why"), Json::String(why.clone())),
        ]));
        write_golden(template, "playbook.jsonc", &playbook);
        write_golden(
            template,
            "parameters.json",
            &pharmakos_proto::json::write(&declared),
        );
        write_golden(
            template,
            "report.json",
            &pharmakos_proto::json::write(&report),
        );
        write_golden(
            template,
            "prose.txt",
            &if prose.ends_with('\n') {
                prose
            } else {
                format!("{prose}\n")
            },
        );
    }
}

/// A human seat nobody edits, in the lobby's match played to its round
/// limit: it seals nothing, so at each Push the gateway files the safe
/// playbook its Easy advisor gave it, which sets the core's dig depth, and
/// the core's starting drone keeps working the starting seam below its top
/// layer. At the default depth of 0 it takes the top layer and stops, which
/// was the demo's F2 (item 133 (3) (a)).
///
/// Measured on this seed when it was written, `ore_delivered` lines per round
/// for (the human seat, Easy's seat): (2, 1), (2, 3), (4, 4) over the three
/// rounds, and on to (1, 1) and (0, 0) in a fourth and fifth round the lobby
/// does not play: the starting seam is dry at depth 4 by round 5, which is
/// evidence for `tune`'s `CORE_DIG_MAX_DEPTH` PLACEHOLDER rather than
/// something this test pins.
#[test]
fn a_seat_nobody_edits_is_still_earning_in_the_lobbys_third_round() {
    let rules = std::fs::read_to_string(root().join("rules").join("rules.v1.json"))
        .expect("the rules text");
    let host = Host::open_from(
        &rules,
        SEED,
        SEATS,
        &Settings {
            segment_lengths_ms: Vec::new(),
            round_limit: ROUND_LIMIT,
            units_per_seat: 0,
        },
        Some(root().join(LIBRARY_PATH)),
    )
    .expect("the lobby's match");
    let table = RulesTable::from_canonical_json(&rules).expect("a rules table");
    let seats: Vec<SeatId> = (0..SEATS)
        .filter_map(|raw| u8::try_from(raw).ok())
        .map(SeatId::new)
        .collect();
    let mut surface = Surface::new(
        &format!("oper-earning-{}", std::process::id()),
        SEED,
        table,
        FogPolicy::fogged(),
        &seats,
    )
    .expect("a surface");
    surface.attach(host).expect("attached");
    surface.open_lull().expect("the opening Lull");
    let mut factory = EasyOperators::new(&rules).expect("the operator reads the rules text");
    let mut easy = InProcessSeats::open(&mut surface, &seats, Some(HUMAN), &mut factory)
        .expect("in-process seats");
    let mut delivered: Vec<u32> = Vec::new();
    for round in 1..=ROUND_LIMIT {
        let lull = surface.lull_length(round);
        surface.set_phase_remaining_ms(lull);
        easy.plan(&mut surface);
        assert!(
            surface.begin_push().expect("the Push"),
            "round {round} pushes"
        );
        let mut seen = 0_usize;
        let mut human = 0_u32;
        while let Some(report) = surface.step().expect("a tick") {
            let events = surface.feed().events();
            if events.len() < seen {
                seen = 0;
            }
            for event in events.get(seen..).unwrap_or_default() {
                let owner = match event.audience {
                    Audience::Private(seat) => Some(seat),
                    Audience::World { owner, .. } => owner,
                    Audience::Public => None,
                };
                if owner == Some(SeatId::new(HUMAN)) && event.kind.to_string() == "ore_delivered" {
                    human = human.checked_add(1).expect("a count");
                }
            }
            seen = events.len();
            if report.segment_ended {
                break;
            }
        }
        delivered.push(human);
        surface.end_recap().expect("the recap ends");
        if round < ROUND_LIMIT {
            surface.open_lull().expect("the next Lull");
        }
    }
    assert!(
        delivered.last().is_some_and(|last| *last > 0),
        "the seat nobody edits delivers ore in the lobby's last round: {delivered:?}"
    );
}
