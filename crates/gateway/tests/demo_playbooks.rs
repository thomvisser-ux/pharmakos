// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The stage demo's own playbooks, verified and rendered where the game
//! verifies and renders them (decisions-log item 123 (2) 10; AGENTS.md
//! section 10 item 3).
//!
//! Five playbooks: the three templates in `library/`, each instantiated for
//! seat 0 with its own values (`instantiate_template` with `template_id`
//! alone, as the wizard's plain page and `templates.rs` ask for it), and the
//! two scenario playbooks under `scenarios/skeleton/`. For each one this suite
//! asserts the byte round trip through plan-core (`Document::parse(text)
//! .to_text() == text`, no golden), and commits two goldens in the gateway's
//! `expected.response.json` kind: the whole `verify_plan{depth: "full"}`
//! answer, whose report carries the `report_hash`, and the whole `render_plan`
//! answer, whose `prose` is the English rendering. Every FULL report must
//! qualify.
//!
//! **The context is the game's**, which neither the verifier's fixture scope
//! nor plan-core's fixture context is: seat 0's frozen snapshot and scope in
//! round 1's Lull of a match opened as `gamectl host` opens the lobby's
//! (`godot/scripts/host_link.gd`: seed `0x00000000ca5caded`, two seats, the
//! rules table's own segment ladder, three rounds), through the same
//! `Host::open_from`, fogged `Surface::new`, `attach` and `open_lull` that
//! `serve.rs`'s `open_surface` takes, with the client's first clock report
//! (the whole Lull left) made host-side. The one difference is that no
//! in-process seat is registered: the built-in operator's seat 1 and seat 0's
//! advisor step nothing, so seat 0's frozen snapshot is the same without them,
//! and the answers read nothing else. Each playbook gets a match of its own,
//! so no golden's `_status` footer depends on another case's calls.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use pharmakos_gateway::fog::FogPolicy;
use pharmakos_gateway::host::{Host, Settings};
use pharmakos_gateway::rpc;
use pharmakos_gateway::surface::Surface;
use pharmakos_plan_core::Document;
use pharmakos_proto::json::Json;
use pharmakos_sim::math::quantity::Ms;
use pharmakos_sim::tables::SeatId;

use support::{MATCH, SEED, result, text_of};

/// The lobby's seats: the human at seat 0, the built-in operator at seat 1
/// (`host_link.gd`'s `MATCH_SEATS` and `HUMAN_SEAT`).
const LOBBY_SEATS: u32 = 2;

/// The lobby's round limit (`host_link.gd`'s `ROUND_LIMIT`, decisions-log item
/// 123 (2) 3).
const LOBBY_ROUND_LIMIT: u32 = 3;

/// The seat every demo playbook is written for.
const SEAT: u8 = 0;

/// A match opened as `gamectl host` opens the lobby's, in its opening Lull.
fn lobby_match() -> Surface {
    let host = Host::open_from(
        &support::rules_json(),
        SEED,
        LOBBY_SEATS,
        &Settings {
            // Empty is the rules table's own ladder: the lobby's `-`.
            segment_lengths_ms: Vec::new(),
            round_limit: LOBBY_ROUND_LIMIT,
            units_per_seat: 0,
        },
        Some(support::library_folder()),
    )
    .expect("the lobby's match");
    let seats: Vec<SeatId> = (0..LOBBY_SEATS)
        .filter_map(|raw| u8::try_from(raw).ok())
        .map(SeatId::new)
        .collect();
    let mut surface = Surface::new(MATCH, SEED, support::rules(), FogPolicy::fogged(), &seats)
        .expect("a match id");
    surface.attach(host).expect("attached");
    surface.open_lull().expect("the opening Lull");
    // The client's first clock report: the whole Lull left, as the footer of
    // `instantiate_suggested` shows it. The length is the rules table's.
    let lull = support::rules()
        .message()
        .r#match
        .as_ref()
        .map_or(0, |settings| settings.lull_ms);
    surface.set_phase_remaining_ms(Ms::new(lull));
    surface
}

/// A JSON string literal, for building params.
fn quote(text: &str) -> String {
    pharmakos_proto::json::write(&Json::String(text.to_owned()))
        .trim_end()
        .to_owned()
}

/// The byte round trip: loading and saving the text gives every byte back.
fn round_trips(case: &str, text: &str) {
    let document = Document::parse(text).unwrap_or_else(|error| panic!("{case} parses: {error}"));
    assert_eq!(
        document.to_text(),
        text,
        "{case}: load-and-save lost a byte"
    );
}

/// Seat 0's `verify_plan{depth: "full"}` and `render_plan` answers for `text`,
/// each written as a golden. The FULL report must qualify.
fn verify_and_render(surface: &mut Surface, case: &str, text: &str) {
    round_trips(case, text);
    let token = support::seat_token(surface, SEAT);
    let verified = support::call(
        surface,
        &token,
        "verify_plan",
        &format!(r#"{{"depth":"full","playbook_jsonc":{}}}"#, quote(text)),
    );
    let report = result(&verified, "verify_plan")
        .get("report")
        .cloned()
        .expect("a report");
    assert_eq!(
        report.get("qualifies"),
        Some(&Json::Bool(true)),
        "{case} qualifies FULL in the lobby's round 1 Lull: {report:?}"
    );
    assert!(
        !text_of(&report, "report_hash").is_empty(),
        "{case}: a report_hash"
    );
    write_golden(&format!("demo_{case}_verify"), &rpc::render(&verified));

    let rendered = support::call(
        surface,
        &token,
        "render_plan",
        &format!(r#"{{"playbook_jsonc":{}}}"#, quote(text)),
    );
    assert!(
        !text_of(&result(&rendered, "render_plan"), "prose").is_empty(),
        "{case}: prose"
    );
    write_golden(&format!("demo_{case}_render"), &rpc::render(&rendered));
}

/// The template `template_id` as seat 0's wizard is handed it with its own
/// values, then verified and rendered.
fn template_case(template_id: &str) {
    let mut surface = lobby_match();
    let token = support::seat_token(&mut surface, SEAT);
    let made = result(
        &support::call(
            &mut surface,
            &token,
            "instantiate_template",
            &format!(r#"{{"template_id":"{template_id}"}}"#),
        ),
        "instantiate_template",
    );
    assert!(
        support::array_of(&made, "parameters")
            .iter()
            .all(|parameter| parameter.get("suggested") == Some(&Json::Bool(false))),
        "{template_id}: its own values, nothing suggested"
    );
    let playbook = text_of(&made, "playbook_jsonc");
    verify_and_render(&mut surface, template_id, &playbook);
}

/// A committed scenario playbook, verified and rendered as it is on disk.
fn scenario_case(case: &str, file: &str) {
    let path = support::workspace_root()
        .join("scenarios")
        .join("skeleton")
        .join(file);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
    let mut surface = lobby_match();
    verify_and_render(&mut surface, case, &text);
}

#[test]
fn hold_and_build_as_the_wizard_opens_it_qualifies_and_renders_in_the_lobbys_lull() {
    template_case("hold_and_build");
}

#[test]
fn expand_and_mine_as_the_wizard_opens_it_qualifies_and_renders_in_the_lobbys_lull() {
    template_case("expand_and_mine");
}

#[test]
fn the_safe_playbook_as_the_wizard_opens_it_qualifies_and_renders_in_the_lobbys_lull() {
    template_case("safe_playbook");
}

#[test]
fn the_against_easy_playbook_qualifies_and_renders_in_the_lobbys_lull() {
    scenario_case("against_easy", "against-easy.playbook.jsonc");
}

#[test]
fn the_deploy_and_visit_playbook_qualifies_and_renders_in_the_lobbys_lull() {
    scenario_case("deploy_and_visit", "deploy-and-visit.playbook.jsonc");
}

/// Write the fresh answer and compare it with the committed golden.
///
/// The answers carry no playbook text: a report names codes, pointers and
/// hashes, and prose is the renderer's own English. So no golden here carries
/// a template's permissive header or a scenario file's text, and the files
/// sit in this area's `GPL-3.0-or-later` tier as they are.
fn write_golden(case: &str, rendered: &str) {
    let file = "response.json";
    assert!(!rendered.contains('\r') && rendered.ends_with('\n'));
    assert!(
        !rendered.contains("SPDX-"),
        "{case}: an answer carries no file's licence header"
    );
    let fresh = support::target_dir()
        .join("golden")
        .join("gateway")
        .join(case);
    std::fs::create_dir_all(&fresh).expect("the fresh output's folder");
    std::fs::write(fresh.join(format!("actual.{file}")), rendered).expect("the fresh output");
    let committed = support::workspace_root()
        .join("tests")
        .join("golden")
        .join("gateway")
        .join(case)
        .join(format!("expected.{file}"));
    let Ok(expected) = std::fs::read_to_string(&committed) else {
        panic!(
            "no committed golden at {}. A new case is committed by copying the fresh output \
             beside it, byte for byte, once it is right; `cargo xtask golden --bless` rewrites \
             only goldens that exist.",
            committed.display()
        );
    };
    assert_eq!(
        expected,
        rendered,
        "the golden at {} moved. tests/golden/gateway/README.md says what a diff in a demo \
         case means; the pull request has to say which input moved it.",
        committed.display()
    );
}
