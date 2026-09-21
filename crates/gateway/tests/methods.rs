// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The method slice, against a live gateway hosting a real match.
//!
//! Three things the skeleton plan's T13 acceptance line asks for, and the
//! fourth it asks of the schema:
//!
//! 1. **the spec's 14-call walkthrough replayed end to end**, ending with
//!    `submit_plan`'s `report_hash` equal to the FULL pre-check's;
//! 2. **secrecy**: a second seat's token sees neither playbook nor draft nor
//!    notebook nor replay;
//! 3. **an invalid playbook returns a full report with `qualifies: false`** and
//!    is *not* a method error;
//! 4. **`get_schema`'s output committed as a golden**, in
//!    `tests/golden/schema/`, so the published surface cannot drift from the
//!    `.proto` files (`tests/golden/schema/README.md`).
//!
//! Every call here goes through [`Surface::call`], which is the same path a
//! WebSocket frame takes: authenticate, rate-limit, resolve, scope, phase,
//! answer, log. Nothing reaches a handler by the back door, because there is no
//! back door to reach it by.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use pharmakos_gateway::error::Code;
use pharmakos_gateway::fog::{Blind, FogPolicy};
use pharmakos_gateway::host::Host;
use pharmakos_gateway::rpc;
use pharmakos_gateway::scopes::{Scope, ScopeSet};
use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::token::{Subject, Token};
use pharmakos_proto::json::Json;
use pharmakos_sim::math::quantity::{Ms, Tick};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::runner::MatchSettings;
use pharmakos_sim::tables::SeatId;
use std::fs;
use std::path::{Path, PathBuf};

const MATCH: &str = "m-0001";

/// The scenario file's seed, so a reader comparing this transcript with
/// `scenarios/skeleton/expand-east.scenario.jsonc` is looking at the same map.
const SEED: u64 = 0x0000_0000_ca5c_aded;

/// A short segment: the walkthrough is about the calls, not about the Push, and
/// a 1 000 ms Push is twenty ticks a test can run to the end.
const SEGMENT_MS: i32 = 1_000;

/// `rules.match.lull_ms`, which is what a client counts down and what the
/// gateway derives its Lull tick from.
const LULL_MS: i32 = 180_000;

// ---------------------------------------------------------------------------
// A gateway with a match behind it
// ---------------------------------------------------------------------------

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| panic!("crates/gateway sits two levels below the workspace root"))
        .to_path_buf()
}

fn target_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .parent()
        .unwrap_or_else(|| panic!("CARGO_TARGET_TMPDIR always has a parent"))
        .to_path_buf()
}

fn rules() -> RulesTable {
    RulesTable::load(&workspace_root().join("rules").join("rules.v1.json"))
        .expect("the shipped rules table")
}

/// A two-seat match, hosted, sitting in its opening Lull.
fn hosted() -> Surface {
    let mut surface = Surface::new(
        MATCH,
        SEED,
        rules(),
        FogPolicy::fogged(),
        &[SeatId::new(0), SeatId::new(1)],
    )
    .expect("a match id");
    let host = Host::open(
        &pharmakos_sim::world::WorldConfig {
            match_seed: SEED,
            seats: 2,
            units_per_seat: 0,
            rules: rules(),
            match_settings: MatchSettings {
                segment_lengths_ms: vec![SEGMENT_MS],
                round_limit: 3,
            },
        },
        None,
    )
    .expect("a match");
    surface.attach(host).expect("attached");
    surface.set_phase_remaining_ms(Ms::new(LULL_MS));
    surface.open_lull().expect("the opening Lull");
    surface
}

fn seat_token(surface: &mut Surface, seat: u8) -> Token {
    let scopes = ScopeSet::of(&[Scope::Observe, Scope::Plan, Scope::PlanSubmit, Scope::Docs]);
    let (token, _) = surface
        .tokens()
        .mint(Subject::Seat(SeatId::new(seat)), MATCH, scopes, Tick::ZERO)
        .expect("minted");
    token
}

/// How much of the Lull a client reports having spent between two calls.
///
/// One tick of game time. A real client counts the Lull down on its own clock
/// -- the gateway reads none (decisions-log item 99) -- and tells the gateway
/// what is left; this is that loop, at its slowest useful rate.
const CLIENT_FRAME_MS: i32 = 50;

/// One call, as a client makes it: report the timer, then ask.
///
/// The report is not decoration. A Lull consumes no sim tick, so the gateway's
/// tick comes from `rules.match.lull_ms` minus what the client says is left
/// (`Surface::sync_time`) -- and everything counted in ticks, the rate limiter
/// first among them, moves with it. A test that never moved the timer would be
/// a client that froze its own clock and then complained about its budget.
fn call(
    surface: &mut Surface,
    token: &Token,
    remaining: &mut i32,
    method: &str,
    params: &str,
) -> Json {
    *remaining = remaining.saturating_sub(CLIENT_FRAME_MS).max(0);
    surface.set_phase_remaining_ms(Ms::new(*remaining));
    surface.call(Some(token), &request(method, params), &Blind)
}

fn request(method: &str, params: &str) -> rpc::Request {
    rpc::parse(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"{method}","params":{params}}}"#
    ))
    .expect("well formed")
}

/// The `result` member, or a panic naming the error the call came back with.
fn result(response: &Json, what: &str) -> Json {
    response.get("result").cloned().unwrap_or_else(|| {
        panic!(
            "{what} was refused: {}",
            response.get("error").map_or_else(
                || String::from("<no error member>"),
                |error| format!("{error:?}")
            )
        )
    })
}

fn text_of(value: &Json, key: &str) -> String {
    match value.get(key) {
        Some(Json::String(text)) => text.clone(),
        other => panic!("`{key}` is a string, and it is {other:?}"),
    }
}

/// Seat 0's own core beacon, as the wire spells it, and where it stands.
fn own_beacon(surface: &Surface) -> (String, i32, i32, i32) {
    let host = surface.host().expect("a hosted match");
    let beacons = host.world().beacons();
    let row = beacons
        .seats()
        .iter()
        .position(|seat| *seat == 0)
        .expect("seat 0 has a core beacon");
    let id = pharmakos_sim::tables::BeaconId::new(beacons.ids().get(row).copied().expect("an id"));
    let at = pharmakos_gateway::view::voxel_of(
        beacons.positions().get(row).copied().expect("a position"),
    );
    (pharmakos_gateway::view::beacon_id(id), at.x, at.y, at.z)
}

/// Where seat 0's commander stands.
fn commander_at(surface: &Surface) -> (i32, i32, i32) {
    let host = surface.host().expect("a hosted match");
    let world = host.world();
    let commander = world.commander_of(SeatId::new(0));
    let row = world
        .units()
        .ids()
        .iter()
        .position(|id| *id == commander.raw())
        .expect("the commander is in the unit table");
    let at = pharmakos_gateway::view::voxel_of(
        world
            .units()
            .positions()
            .get(row)
            .copied()
            .expect("a position"),
    );
    (at.x, at.y, at.z)
}

// ---------------------------------------------------------------------------
// Playbooks
// ---------------------------------------------------------------------------

/// A playbook with two errors in it, and a comment, which is what a player
/// hands the gateway.
///
/// The two errors are the same one twice -- a `hold` for a negative number of
/// game milliseconds -- because item 46 puts exactly that rejection on the
/// verifier with a JSON Pointer rather than at decode, and two of them make the
/// walkthrough's "(2 errors, with fixes)" literally true.
const BROKEN: &str = concat!(
    "// Two holds, and both of them are wrong.\n",
    "{\"schema_version\": {\"major\": 1},\n",
    " \"meta\": {\"title\": \"Walkthrough\", \"author_kind\": \"HUMAN\"},\n",
    " \"declarative\": {\"route\": [\n",
    "   {\"label\": \"first\", \"hold\": {\"ms\": -1}},\n",
    "   {\"label\": \"second\", \"hold\": {\"ms\": -2}}\n",
    " ]},\n",
    " \"on_death\": {\"on_respawn\": \"CONTINUE\"},\n",
    " \"fallback\": {\"hold\": {\"at\": {\"beacon_anchor\": {\"safest\": {}}}}},\n",
    " \"kind\": \"PLAYBOOK\"}\n",
);

/// The patch that fixes both of them, as `patch_plan` takes it.
const FIX: &str = concat!(
    "[{\"op\": \"replace\", \"path\": \"/declarative/route/0/hold/ms\", \"value\": 1000},\n",
    " {\"op\": \"replace\", \"path\": \"/declarative/route/1/hold/ms\", \"value\": 2000}]"
);

// ---------------------------------------------------------------------------
// The walkthrough
// ---------------------------------------------------------------------------

/// Spec section 12's own worked session, end to end, against a live gateway.
///
/// ```text
/// get_status -> get_briefing{detail:"standard"} -> get_beacon{b_04}
///   -> list_templates{tag:"attack"} -> get_schema{part:"step"} -> estimate_route
///   -> verify_plan{depth:"quick"}  (2 errors, with fixes)
///   -> patch_plan -> verify_plan{depth:"full"} -> render_plan
///   -> submit_plan  (report_hash matches the FULL pre-check)
///   -> set_ready -> wait_for{phase_change} -> wait_for{feed_digest}
/// ```
///
/// The assertion the plan names is the eleventh call's: **`submit_plan`'s
/// `report_hash` equals the FULL pre-check's**. That is spec section 11's
/// "byte-identical" promise made concrete -- a report is a pure function of
/// five inputs, so a seat can see exactly what the Ledger will see before it
/// files, and the seal it gets is the one it was shown.
#[test]
#[allow(
    clippy::too_many_lines,
    reason = "fourteen calls, in the order spec section 12 prints them. Splitting the               session into helpers would hide the one property the test exists for -- that               this is ONE session against ONE gateway, in this order -- and a reader               checking it against the spec would have to reassemble it."
)]
fn the_specs_fourteen_call_walkthrough_runs_end_to_end() {
    let mut surface = hosted();
    let token = seat_token(&mut surface, 0);
    let (beacon, bx, by, bz) = own_beacon(&surface);
    let (cx, cy, cz) = commander_at(&surface);
    let mut left = LULL_MS;
    let mut transcript = String::new();
    let mut row = |number: usize, method: &str, note: &str| {
        // Pushed piece by piece rather than through `format!`:
        // `clippy::format_push_string` is denied, and the golden's separator is
        // a tab and its terminator a newline, which is easier to read written
        // out than buried in a format string.
        transcript.push_str(&number.to_string());
        transcript.push('\t');
        transcript.push_str(method);
        transcript.push('\t');
        transcript.push_str(note);
        transcript.push('\n');
    };

    // 1. get_status
    let response = call(&mut surface, &token, &mut left, "get_status", "{}");
    let status = result(&response, "get_status");
    let footer = status.get("_status").expect("every result carries it");
    assert_eq!(
        footer.get("phase"),
        Some(&Json::String(String::from("lull")))
    );
    row(
        1,
        "get_status",
        &format!("phase={}", text_of(footer, "phase")),
    );

    // 2. get_briefing{detail:"standard"}
    let response = call(
        &mut surface,
        &token,
        &mut left,
        "get_briefing",
        r#"{"detail":"standard"}"#,
    );
    let briefing = result(&response, "get_briefing");
    assert_eq!(
        briefing.get("segment_length_ms"),
        Some(&Json::Number(SEGMENT_MS.to_string())),
        "the coming segment's length comes from the frozen snapshot"
    );
    row(
        2,
        "get_briefing",
        &format!("segment_length_ms={SEGMENT_MS}"),
    );

    // 3. get_beacon{b_04}
    let response = call(
        &mut surface,
        &token,
        &mut left,
        "get_beacon",
        &format!(r#"{{"beacon_id":"{beacon}"}}"#),
    );
    let one = result(&response, "get_beacon");
    assert_eq!(
        one.get("summary")
            .and_then(|summary| summary.get("beacon_id")),
        Some(&Json::String(beacon.clone()))
    );
    row(3, "get_beacon", "the seat's own core");

    // 4. list_templates{tag:"attack"}
    let response = call(
        &mut surface,
        &token,
        &mut left,
        "list_templates",
        r#"{"tag":"attack"}"#,
    );
    let templates = result(&response, "list_templates");
    assert_eq!(
        templates.get("templates"),
        Some(&Json::Array(Vec::new())),
        "no template folder is configured, so the library is empty rather than absent"
    );
    row(4, "list_templates", "0 templates (no folder configured)");

    // 5. get_schema{part:"step"}
    let response = call(
        &mut surface,
        &token,
        &mut left,
        "get_schema",
        r#"{"part":"step"}"#,
    );
    let schema = result(&response, "get_schema");
    let json_schema = text_of(&schema, "json_schema");
    assert!(json_schema.contains("gp.v1.Step"), "{json_schema}");
    row(5, "get_schema", "part=step");

    // 6. estimate_route
    let response = call(
        &mut surface,
        &token,
        &mut left,
        "estimate_route",
        &format!(
            r#"{{"waypoints":[{{"voxel":{{"x":{cx},"y":{cy},"z":{cz}}}}},
                                  {{"voxel":{{"x":{bx},"y":{by},"z":{bz}}}}}]}}"#
        ),
    );
    let route = result(&response, "estimate_route");
    assert_eq!(route.get("reachable"), Some(&Json::Bool(true)));
    let legs = match route.get("legs") {
        Some(Json::Array(legs)) => legs.len(),
        other => panic!("legs is an array, and it is {other:?}"),
    };
    assert_eq!(legs, 1, "two waypoints are one leg");
    row(6, "estimate_route", "reachable, 1 leg, not fogged");

    // 7. verify_plan{depth:"quick"} -- two errors.
    let response = call(
        &mut surface,
        &token,
        &mut left,
        "verify_plan",
        &format!(r#"{{"depth":"quick","playbook_jsonc":{}}}"#, quote(BROKEN)),
    );
    let quick = result(&response, "verify_plan quick");
    let report = quick.get("report").expect("a report");
    assert_eq!(report.get("qualifies"), Some(&Json::Bool(false)));
    assert_eq!(
        report.get("depth"),
        Some(&Json::String(String::from("quick")))
    );
    let errors = diagnostics_of(report);
    assert!(
        errors >= 2,
        "the walkthrough's two errors, and {errors} found"
    );
    row(7, "verify_plan{depth=quick}", "qualifies=false");

    // 8. patch_plan
    let response = call(
        &mut surface,
        &token,
        &mut left,
        "patch_plan",
        &format!(
            r#"{{"playbook_jsonc":{},"json_patch":{}}}"#,
            quote(BROKEN),
            quote(FIX)
        ),
    );
    let patched = result(&response, "patch_plan");
    let fixed = text_of(&patched, "playbook_jsonc");
    assert!(
        fixed.contains("// Two holds, and both of them are wrong."),
        "a patch preserves the author's comments: {fixed}"
    );
    assert!(!text_of(&patched, "inverse_json_patch").is_empty(), "undo");
    row(
        8,
        "patch_plan",
        "comments survive; an inverse patch comes back",
    );

    // 9. verify_plan{depth:"full"} -- the pre-check.
    let response = call(
        &mut surface,
        &token,
        &mut left,
        "verify_plan",
        &format!(r#"{{"depth":"full","playbook_jsonc":{}}}"#, quote(&fixed)),
    );
    let full = result(&response, "verify_plan full");
    let precheck = full.get("report").expect("a report");
    assert_eq!(
        precheck.get("qualifies"),
        Some(&Json::Bool(true)),
        "the patch fixed both: {:?}",
        precheck.get("diagnostics")
    );
    let precheck_hash = text_of(precheck, "report_hash");
    row(9, "verify_plan{depth=full}", "qualifies=true");

    // 10. render_plan
    let response = call(
        &mut surface,
        &token,
        &mut left,
        "render_plan",
        &format!(r#"{{"playbook_jsonc":{}}}"#, quote(&fixed)),
    );
    let rendered = result(&response, "render_plan");
    let prose = text_of(&rendered, "prose");
    assert!(!prose.is_empty());
    row(10, "render_plan", "prose returned");

    // 11. submit_plan -- and the assertion the plan names.
    let response = call(
        &mut surface,
        &token,
        &mut left,
        "submit_plan",
        &format!(r#"{{"playbook_jsonc":{}}}"#, quote(&fixed)),
    );
    let submitted = result(&response, "submit_plan");
    assert_eq!(submitted.get("accepted"), Some(&Json::Bool(true)));
    let sealed = submitted.get("report").expect("a report");
    assert_eq!(
        text_of(sealed, "report_hash"),
        precheck_hash,
        "submit runs FULL, so its report_hash is the FULL pre-check's, byte for byte \
         (spec section 11; decisions-log item 82)"
    );
    row(
        11,
        "submit_plan",
        "accepted=true; report_hash matches call 9",
    );

    // 12. set_ready
    let response = call(
        &mut surface,
        &token,
        &mut left,
        "set_ready",
        r#"{"ready":true}"#,
    );
    let _ = result(&response, "set_ready");
    row(12, "set_ready", "ready=true");

    // 13. wait_for{phase_change}
    let response = call(
        &mut surface,
        &token,
        &mut left,
        "wait_for",
        r#"{"trigger":"phase_change","timeout_ms":5000}"#,
    );
    let waited = result(&response, "wait_for phase_change");
    assert_eq!(
        waited.get("fired"),
        Some(&Json::Bool(false)),
        "no mark was handed in, so there is nothing to have changed since"
    );
    let mark = text_of(&waited, "cursor");
    row(13, "wait_for{phase_change}", "fired=false; a mark returned");

    // The host ends the Lull, which is the phase change the seat is waiting
    // for. This is the one thing in the walkthrough the seat does not do: the
    // Lull ends on the host's word, because the gateway reads no clock.
    surface.begin_push().expect("the Push begins");
    for _ in 0..20 {
        surface.step().expect("a tick");
    }
    let response = call(
        &mut surface,
        &token,
        &mut left,
        "wait_for",
        &format!(r#"{{"trigger":"phase_change","cursor":"{mark}"}}"#),
    );
    let waited = result(&response, "wait_for phase_change, second time");
    assert_eq!(
        waited.get("fired"),
        Some(&Json::Bool(true)),
        "the Lull ended and the recap opened"
    );

    // 14. wait_for{feed_digest}
    let response = call(
        &mut surface,
        &token,
        &mut left,
        "wait_for",
        r#"{"trigger":"feed_digest"}"#,
    );
    let digest = result(&response, "wait_for feed_digest");
    assert_eq!(
        digest.get("fired"),
        Some(&Json::Bool(true)),
        "a whole segment ran, so there is a feed to read"
    );
    row(14, "wait_for{feed_digest}", "fired=true after one segment");

    check_golden("walkthrough", "walkthrough.txt", &header(), &transcript);
}

/// How many diagnostics a report carries.
fn diagnostics_of(report: &Json) -> usize {
    match report.get("diagnostics") {
        Some(Json::Array(found)) => found.len(),
        other => panic!("diagnostics is an array, and it is {other:?}"),
    }
}

/// A Rust string as a JSON string literal, through the codec that writes every
/// other string in this project.
fn quote(text: &str) -> String {
    pharmakos_proto::json::write(&Json::String(text.to_owned()))
        .trim_end()
        .to_owned()
}

/// The transcript golden's header.
fn header() -> String {
    concat!(
        "# Spec section 12's 14-call walkthrough, replayed against a live gateway.\n",
        "# Seed 0x00000000ca5caded, two seats, a 1 000 ms segment.\n",
        "# call\tmethod\twhat came back\n",
    )
    .to_owned()
}

// ---------------------------------------------------------------------------
// Secrecy
// ---------------------------------------------------------------------------

/// Spec section 12: "playbooks, drafts, the notebook and the private replay
/// cache never leave the gateway for another seat."
///
/// Four things, one token, and every one of them refused -- not by the handler
/// remembering to check, but by [`Surface::seat_state`] being the only path to
/// the private store and taking the subject asking.
#[test]
fn a_second_seats_token_sees_neither_playbook_nor_draft_nor_notebook_nor_replay() {
    let mut surface = hosted();
    let first = seat_token(&mut surface, 0);
    let second = seat_token(&mut surface, 1);

    // Seat 0 writes all four.
    let _ = result(
        &surface.call(
            Some(&first),
            &request("save_notes", r#"{"notes":"the vent is east"}"#),
            &Blind,
        ),
        "save_notes",
    );
    let saved = result(
        &surface.call(
            Some(&first),
            &request(
                "save_draft",
                &format!(
                    r#"{{"draft_id":"east","label":"east push","playbook_jsonc":{}}}"#,
                    quote(BROKEN)
                ),
            ),
            &Blind,
        ),
        "save_draft",
    );
    assert_eq!(
        saved.get("draft_id"),
        Some(&Json::String(String::from("east")))
    );

    // Seat 1 asks for all four and gets its own emptiness back.
    let briefing = result(
        &surface.call(Some(&second), &request("get_briefing", "{}"), &Blind),
        "get_briefing",
    );
    assert_eq!(
        briefing.get("notes"),
        Some(&Json::String(String::new())),
        "seat 1's notebook is seat 1's, and it is empty"
    );
    let drafts = result(
        &surface.call(Some(&second), &request("list_drafts", "{}"), &Blind),
        "list_drafts",
    );
    assert_eq!(
        drafts.get("drafts"),
        Some(&Json::Array(Vec::new())),
        "seat 1 has none of its own and cannot see seat 0's"
    );

    // And the private store refuses directly, which is where the rule lives.
    for (subject, who) in [
        (Subject::Seat(SeatId::new(1)), "another seat"),
        (Subject::Admin, "admin"),
        (Subject::Spectator, "a spectator"),
    ] {
        let refused = surface
            .seat_state(subject, SeatId::new(0))
            .expect_err("refused");
        assert_eq!(refused.code, Code::ForbiddenScope, "{who}");
    }

    // The sealed playbook -- the private replay's other half -- is the same
    // gate, and a seat that sealed nothing gets NO_QUALIFYING_PLAN rather than
    // somebody else's.
    let error = surface
        .sealed_plan(Subject::Seat(SeatId::new(1)), SeatId::new(0))
        .expect_err("refused");
    assert_eq!(error.code, Code::ForbiddenScope);
    let error = surface
        .sealed_plan(Subject::Seat(SeatId::new(1)), SeatId::new(1))
        .expect_err("nothing sealed");
    assert_eq!(error.code, Code::NoQualifyingPlan);
}

// ---------------------------------------------------------------------------
// An invalid playbook is not a method error
// ---------------------------------------------------------------------------

/// `gateway.proto`'s own sentence: "An invalid playbook is NOT a method error.
/// `verify_plan` and `submit_plan` return a full report with qualifies:false".
///
/// Three shapes of wrong, because they fail at three different stages and a
/// rule that only held for one of them would not be the rule: a file that is
/// not JSON at all, a file the schema refuses, and a file the semantics
/// refuse.
#[test]
fn an_invalid_playbook_is_a_full_report_and_never_a_method_error() {
    let mut surface = hosted();
    let token = seat_token(&mut surface, 0);

    for (what, playbook) in [
        ("not JSON at all", "{ this is not json"),
        (
            "a field the schema does not have",
            r#"{"schema_version":{"major":1},"not_a_field":1}"#,
        ),
        ("the semantics refuse it", BROKEN),
    ] {
        for method in ["verify_plan", "submit_plan"] {
            let response = surface.call(
                Some(&token),
                &request(
                    method,
                    &format!(r#"{{"playbook_jsonc":{}}}"#, quote(playbook)),
                ),
                &Blind,
            );
            let answered = result(&response, &format!("{method} on {what}"));
            let report = answered
                .get("report")
                .unwrap_or_else(|| panic!("{method} on {what} carried no report"));
            assert_eq!(
                report.get("qualifies"),
                Some(&Json::Bool(false)),
                "{method} on {what}"
            );
            assert!(
                diagnostics_of(report) > 0,
                "{method} on {what}: a refusal with no diagnostic tells a player nothing"
            );
            assert_eq!(
                report.get("depth"),
                Some(&Json::String(String::from("full"))),
                "{method} on {what}: submit always runs FULL and verify defaults to it"
            );
            if method == "submit_plan" {
                assert_eq!(answered.get("accepted"), Some(&Json::Bool(false)));
            }
        }
    }

    // And a seat that submitted something good keeps it when a later
    // submission is bad: a failed submission must not unseal what was sealed.
    let good = pharmakos_gateway::host::SAFE_PLAYBOOK;
    let response = surface.call(
        Some(&token),
        &request(
            "submit_plan",
            &format!(r#"{{"playbook_jsonc":{}}}"#, quote(good)),
        ),
        &Blind,
    );
    assert_eq!(
        result(&response, "submit_plan the safe playbook").get("accepted"),
        Some(&Json::Bool(true)),
        "the safe playbook always qualifies (spec section 14)"
    );
    let sealed = surface
        .sealed_plan(Subject::Seat(SeatId::new(0)), SeatId::new(0))
        .expect("sealed")
        .playbook_jsonc
        .clone();
    let _ = surface.call(
        Some(&token),
        &request(
            "submit_plan",
            &format!(r#"{{"playbook_jsonc":{}}}"#, quote(BROKEN)),
        ),
        &Blind,
    );
    assert_eq!(
        surface
            .sealed_plan(Subject::Seat(SeatId::new(0)), SeatId::new(0))
            .expect("still sealed")
            .playbook_jsonc,
        sealed,
        "a report that does not qualify leaves the previous seal alone"
    );
}

// ---------------------------------------------------------------------------
// The latest verified submission replaces the previous one (item 5)
// ---------------------------------------------------------------------------

#[test]
fn the_latest_verified_submission_replaces_the_previous_one_any_number_of_times() {
    let mut surface = hosted();
    let token = seat_token(&mut surface, 0);
    let mut hashes: Vec<String> = Vec::new();
    for ms in [1_000, 2_000, 3_000] {
        let playbook = pharmakos_gateway::host::SAFE_PLAYBOOK
            .replace("\"ms\": 1000", &format!("\"ms\": {ms}"));
        let response = surface.call(
            Some(&token),
            &request(
                "submit_plan",
                &format!(r#"{{"playbook_jsonc":{}}}"#, quote(&playbook)),
            ),
            &Blind,
        );
        let answered = result(&response, "submit_plan");
        assert_eq!(answered.get("accepted"), Some(&Json::Bool(true)));
        hashes.push(text_of(
            answered.get("report").expect("a report"),
            "report_hash",
        ));
        assert_eq!(
            surface
                .sealed_plan(Subject::Seat(SeatId::new(0)), SeatId::new(0))
                .expect("sealed")
                .playbook_jsonc,
            playbook,
            "the latest replaces the previous"
        );
    }
    hashes.sort();
    hashes.dedup();
    assert_eq!(hashes.len(), 3, "three different playbooks, three hashes");
}

// ---------------------------------------------------------------------------
// Draft continuity (spec section 13)
// ---------------------------------------------------------------------------

/// "Each Lull opens with last round's playbook pre-loaded as an editable
/// draft, re-verified against the new snapshot so anything the round
/// invalidated shows as a diagnostic straight away."
///
/// Two halves, and the second is the one that is easy to leave out: the draft
/// is carried **and the re-verification has already run** when the Lull opens.
#[test]
fn a_lull_opens_with_last_rounds_playbook_carried_and_re_verified() {
    let mut surface = hosted();
    let token = seat_token(&mut surface, 0);
    let playbook = pharmakos_gateway::host::SAFE_PLAYBOOK;
    let _ = result(
        &surface.call(
            Some(&token),
            &request(
                "submit_plan",
                &format!(r#"{{"playbook_jsonc":{}}}"#, quote(playbook)),
            ),
            &Blind,
        ),
        "submit_plan",
    );

    surface.begin_push().expect("the Push begins");
    while let Some(report) = surface.step().expect("a tick") {
        if report.segment_ended {
            break;
        }
    }
    surface.end_recap().expect("the recap ends");
    surface.open_lull().expect("the next Lull");

    let state = surface
        .seat_state(Subject::Seat(SeatId::new(0)), SeatId::new(0))
        .expect("its own");
    let carried = state
        .draft("carried")
        .expect("last round's playbook is pre-loaded");
    assert_eq!(carried.playbook_jsonc, playbook);
    assert_eq!(carried.round, 2, "carried into round 2");
    let continuity = state
        .continuity
        .as_ref()
        .expect("the re-verification has already run");
    assert!(
        continuity.qualifies,
        "the safe playbook still qualifies against the new snapshot"
    );
    assert!(
        !continuity.report_hash.is_empty(),
        "a re-verification with no report is not a re-verification"
    );

    // And it is the seat's own draft like any other: seat 1 carried nothing and
    // sees nothing.
    let other = surface
        .seat_state(Subject::Seat(SeatId::new(1)), SeatId::new(1))
        .expect("its own");
    assert!(other.drafts.is_empty());
}

// ---------------------------------------------------------------------------
// The safe playbook is filed for a seat that sealed nothing
// ---------------------------------------------------------------------------

#[test]
fn a_seat_that_seals_nothing_has_the_safe_playbook_filed_for_it() {
    let mut surface = hosted();
    surface.begin_push().expect("the Push begins");
    for seat in [0_u8, 1] {
        let sealed = surface
            .sealed_plan(Subject::Seat(SeatId::new(seat)), SeatId::new(seat))
            .expect("something was filed");
        assert!(
            sealed.filed_by_the_gateway,
            "seat {seat} sealed nothing, so the gateway filed the safe playbook"
        );
        assert_eq!(
            sealed.playbook_jsonc,
            pharmakos_gateway::host::SAFE_PLAYBOOK
        );
    }
}

// ---------------------------------------------------------------------------
// get_schema, as a golden
// ---------------------------------------------------------------------------

/// The whole playbook vocabulary, committed, so the published surface cannot
/// drift from the `.proto` files (`tests/golden/schema/README.md`).
///
/// Served rather than built: the gateway answers with exactly what
/// [`pharmakos_gateway::schema`] generates from the checked-in descriptor set,
/// and this test compares the *method's* answer rather than the generator's,
/// so a gateway that started assembling its own would move the golden.
#[test]
fn get_schema_serves_the_generated_artefact() {
    let mut surface = hosted();
    let token = seat_token(&mut surface, 0);
    let response = surface.call(Some(&token), &request("get_schema", "{}"), &Blind);
    let served = text_of(&result(&response, "get_schema"), "json_schema");
    assert_eq!(
        served,
        pharmakos_gateway::schema::text("").expect("the generated document"),
        "the gateway is building its answer rather than serving the generated artefact"
    );
    write_and_compare(
        &workspace_root()
            .join("tests")
            .join("golden")
            .join("schema")
            .join("get_schema"),
        &target_dir()
            .join("golden")
            .join("schema")
            .join("get_schema"),
        "expected.json",
        "actual.json",
        &served,
    );
}

// ---------------------------------------------------------------------------
// Golden plumbing
// ---------------------------------------------------------------------------

/// Write the fresh output under the gateway area and compare it.
fn check_golden(case: &str, file: &str, header: &str, body: &str) {
    let contents = format!("{header}{body}");
    write_and_compare(
        &workspace_root()
            .join("tests")
            .join("golden")
            .join("gateway")
            .join(case),
        &target_dir().join("golden").join("gateway").join(case),
        &format!("expected.{file}"),
        &format!("actual.{file}"),
        &contents,
    );
}

fn write_and_compare(
    golden_dir: &Path,
    fresh_dir: &Path,
    expected: &str,
    actual: &str,
    contents: &str,
) {
    assert!(!contents.contains('\r'), "LF endings only");
    assert!(contents.ends_with('\n'), "a trailing newline");
    fs::create_dir_all(fresh_dir)
        .unwrap_or_else(|error| panic!("creating {}: {error}", fresh_dir.display()));
    fs::write(fresh_dir.join(actual), contents)
        .unwrap_or_else(|error| panic!("writing the fresh output: {error}"));
    let golden = golden_dir.join(expected);
    let Ok(committed) = fs::read_to_string(&golden) else {
        panic!(
            "no committed golden at {}. Run `cargo xtask golden --bless` once the fresh output \
             is right.",
            golden.display()
        );
    };
    assert_eq!(
        committed,
        contents,
        "the golden at {} moved; the pull request has to say which behaviour changed.",
        golden.display()
    );
}
