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
