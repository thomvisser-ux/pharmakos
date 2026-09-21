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
//! Every call here goes through [`Surface::call`]: authenticate, rate-limit,
//! resolve, scope, phase, answer, log. Nothing reaches a handler by the back
//! door, because there is no back door to reach it by.
//!
//! What that is **not** is the transport. `Surface::call` is where a decoded
//! JSON-RPC request arrives; the RFC 6455 upgrade, the framing and the masking
//! in front of it are `tests/websocket.rs`'s, which replays the same fourteen
//! calls over a live `session::serve` on one connection
//! (`the_fourteen_call_walkthrough_also_runs_over_the_transport`). The two
//! halves are split because this one drives the host between calls -- the Lull
//! ends on the host's word -- and a session that owns the surface cannot.
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
    hosted_with(SEGMENT_MS)
}

/// The same, with a segment of a stated length: a test about what a client may
/// do *during* a Push needs a Push long enough to do it in.
fn hosted_with(segment_ms: i32) -> Surface {
    hosted_as(segment_ms, FogPolicy::fogged(), None)
}

/// The same again, with the fog policy and the template folder spelled out: a
/// casual match is where "the seat's own beacons come first" has anything to
/// come before, and the library is what `instantiate_template` reads.
fn hosted_as(segment_ms: i32, fog: FogPolicy, library: Option<PathBuf>) -> Surface {
    let mut surface = Surface::new(MATCH, SEED, rules(), fog, &[SeatId::new(0), SeatId::new(1)])
        .expect("a match id");
    let host = Host::open(
        &pharmakos_sim::world::WorldConfig {
            match_seed: SEED,
            seats: 2,
            units_per_seat: 0,
            rules: rules(),
            match_settings: MatchSettings {
                segment_lengths_ms: vec![segment_ms],
                round_limit: 3,
            },
        },
        library,
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
/// The two errors are the same one twice -- a `hold` for **no** game
/// milliseconds -- because item 46 puts exactly that rejection on the verifier
/// with a JSON Pointer rather than at decode, and two of them make the
/// walkthrough's "(2 errors, with fixes)" literally true.
///
/// Zero rather than a negative number, and that is the whole reason it is
/// zero: a *negative* duration breaks two rules at once (`E0109`, a duration is
/// never negative, and `E0303`, a hold lasts a positive number of
/// milliseconds), so two bad steps would be four diagnostics and the
/// walkthrough's own line would not be true of it. A zero hold breaks one rule,
/// once per step.
const BROKEN: &str = concat!(
    "// Two holds, and both of them are wrong.\n",
    "{\"schema_version\": {\"major\": 1},\n",
    " \"meta\": {\"title\": \"Walkthrough\", \"author_kind\": \"HUMAN\"},\n",
    " \"declarative\": {\"route\": [\n",
    "   {\"label\": \"first\", \"hold\": {\"ms\": 0}},\n",
    "   {\"label\": \"second\", \"hold\": {\"ms\": 0}}\n",
    " ]},\n",
    " \"on_death\": {\"on_respawn\": \"CONTINUE\"},\n",
    " \"fallback\": {\"hold\": {\"at\": {\"beacon_anchor\": {\"safest\": {}}}}},\n",
    " \"kind\": \"PLAYBOOK\"}\n",
);

/// The patch that fixes both of them, built from the **verifier's own**
/// machine-applicable suggestions rather than written out here.
///
/// That is the loop an editor runs -- "(2 errors, with fixes)" in spec section
/// 12 is about `gp.api.v1.PatchSuggestion.json_patch`, and a walkthrough that
/// hand-wrote the fix would leave the half of it the suggestions are for
/// untested. Every diagnostic's first suggestion, in the order the report gives
/// them, concatenated into one RFC 6902 patch.
fn fix_from(report: &Json) -> String {
    let Some(Json::Array(diagnostics)) = report.get("diagnostics") else {
        panic!("a report carries diagnostics");
    };
    let mut operations: Vec<Json> = Vec::new();
    for diagnostic in diagnostics {
        let Some(Json::Array(suggestions)) = diagnostic.get("suggestions") else {
            panic!("`{diagnostic:?}` carries no suggestions, so there is no fix to apply");
        };
        let first = suggestions
            .first()
            .unwrap_or_else(|| panic!("`{diagnostic:?}` carries an empty suggestion list"));
        let patch = match first.get("json_patch") {
            Some(Json::String(text)) => text.clone(),
            other => panic!("a suggestion's json_patch is text, and it is {other:?}"),
        };
        match pharmakos_proto::json::read(&patch).expect("a suggestion is a JSON patch") {
            Json::Array(operation) => operations.extend(operation),
            other => panic!("a JSON patch is an array, and it is {other:?}"),
        }
    }
    pharmakos_proto::json::write(&Json::Array(operations))
        .trim_end()
        .to_owned()
}

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
    reason = "fourteen calls, in the order spec section 12 prints them. Splitting the session \
              into helpers would hide the one property the test exists for -- that this is ONE \
              session against ONE gateway, in this order -- and a reader checking it against \
              the spec would have to reassemble it."
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
    assert_eq!(
        errors, 2,
        "the walkthrough's line is `(2 errors, with fixes)`, and this is what the two are"
    );
    row(7, "verify_plan{depth=quick}", "qualifies=false, 2 errors");

    // 8. patch_plan -- with the verifier's own fixes, which is the other half
    // of "(2 errors, with fixes)" and the loop the editor runs.
    let fix = fix_from(report);
    let response = call(
        &mut surface,
        &token,
        &mut left,
        "patch_plan",
        &format!(
            r#"{{"playbook_jsonc":{},"json_patch":{}}}"#,
            quote(BROKEN),
            quote(&fix)
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
        "the report's own two fixes applied; comments survive",
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
    // A fifteenth call in a fourteen-call session, and it is in the transcript
    // rather than left uncounted: `wait_for` polls and never blocks (the
    // gateway reads no clock), so "the seat waits for the phase to change" is
    // two calls with the host's own act between them, and a golden that showed
    // only one would be describing a gateway that blocks.
    row(
        13,
        "wait_for{phase_change} again",
        "fired=true, after the host ended the Lull",
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
        "# Fifteen rows for fourteen calls: call 13 is made twice, because\n",
        "# `wait_for` polls and never blocks, and the host ends the Lull between\n",
        "# the two. Everything else is one row to one call, in the spec's order.\n",
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
// The reads nothing else covers
// ---------------------------------------------------------------------------

/// The error code a refused call came back with.
fn code(response: &Json, what: &str) -> String {
    response
        .get("error")
        .and_then(|error| error.get("data"))
        .and_then(|data| data.get("code"))
        .map_or_else(
            || panic!("{what} was not refused: {response:?}"),
            |value| match value {
                Json::String(text) => text.clone(),
                other => panic!("a code is a string, and it is {other:?}"),
            },
        )
}

/// `get_map_summary` answers the seed the match was built with, in the spelling
/// the scenario files use.
///
/// The field `match_seed` is the one this lane made a contract change to add,
/// and a served field nothing asserts is a field that can quietly become empty.
#[test]
fn get_map_summary_answers_the_seed_the_match_was_built_with() {
    let mut surface = hosted();
    let token = seat_token(&mut surface, 0);
    let summary = result(
        &surface.call(Some(&token), &request("get_map_summary", "{}"), &Blind),
        "get_map_summary",
    );
    assert_eq!(
        text_of(&summary, "match_seed"),
        pharmakos_gateway::view::seed_text(SEED),
        "the seed is `part of the match, not a secret`, and it is this match's"
    );
    let size = summary.get("size").expect("the extent");
    for axis in ["x", "y", "z"] {
        match size.get(axis) {
            Some(Json::Number(text)) => assert_ne!(text, "0", "the {axis} extent"),
            other => panic!("the {axis} extent is a number, and it is {other:?}"),
        }
    }
}

/// `list_beacons`: own beacons first, the asked-for limit honoured exactly, and
/// a cursor that resumes where the page stopped -- and only that listing's.
#[test]
fn list_beacons_puts_the_seats_own_first_and_pages_with_its_own_cursor() {
    // Casual, so seat 0 can see somebody else's beacons and "own first" has
    // something to come before.
    let mut surface = hosted_as(SEGMENT_MS, FogPolicy::casual(), None);
    let token = seat_token(&mut surface, 0);
    let (own, _, _, _) = own_beacon(&surface);

    let all = result(
        &surface.call(Some(&token), &request("list_beacons", "{}"), &Blind),
        "list_beacons",
    );
    let ids = beacon_ids(&all);
    assert!(ids.len() >= 2, "two seats, two core beacons: {ids:?}");
    assert_eq!(ids.first(), Some(&own), "the seat's own comes first");
    assert!(
        text_of(&all, "next_cursor").is_empty(),
        "one page held them all"
    );

    // One at a time, following the cursor.
    let first = result(
        &surface.call(
            Some(&token),
            &request("list_beacons", r#"{"limit":1}"#),
            &Blind,
        ),
        "list_beacons{limit:1}",
    );
    assert_eq!(beacon_ids(&first), vec![own.clone()]);
    let cursor = text_of(&first, "next_cursor");
    assert!(!cursor.is_empty(), "there is more to come");
    let second = result(
        &surface.call(
            Some(&token),
            &request(
                "list_beacons",
                &format!(r#"{{"limit":1,"cursor":"{cursor}"}}"#),
            ),
            &Blind,
        ),
        "list_beacons{cursor}",
    );
    let resumed = beacon_ids(&second);
    assert_eq!(resumed.len(), 1);
    assert_ne!(
        resumed.first(),
        Some(&own),
        "the cursor resumed rather than restarting"
    );

    // `limit: 0` is a client asking for less than its budget, and less than one
    // is none.
    let none = result(
        &surface.call(
            Some(&token),
            &request("list_beacons", r#"{"limit":0}"#),
            &Blind,
        ),
        "list_beacons{limit:0}",
    );
    assert_eq!(beacon_ids(&none), Vec::<String>::new());

    // A cursor the feed issued is not a place in this listing, and is told so
    // rather than read as a beacon offset.
    let feed = result(
        &surface.call(Some(&token), &request("get_segment_feed", "{}"), &Blind),
        "get_segment_feed",
    );
    let feed_cursor = text_of(&feed, "next_cursor");
    assert!(!feed_cursor.is_empty());
    let refused = surface.call(
        Some(&token),
        &request("list_beacons", &format!(r#"{{"cursor":"{feed_cursor}"}}"#)),
        &Blind,
    );
    assert_eq!(
        code(&refused, "a feed cursor handed to list_beacons"),
        "INVALID_ARGUMENT"
    );
}

/// The beacon ids of a `list_beacons` answer, in the order it gave them.
fn beacon_ids(answer: &Json) -> Vec<String> {
    match answer.get("beacons") {
        Some(Json::Array(rows)) => rows
            .iter()
            .map(|row| match row.get("beacon_id") {
                Some(Json::String(id)) => id.clone(),
                other => panic!("a beacon summary carries an id, and it is {other:?}"),
            })
            .collect(),
        other => panic!("beacons is an array, and it is {other:?}"),
    }
}

/// A beacon a fogged seat cannot see does not exist, as far as that seat is
/// told -- and the same `NOT_FOUND` an id nobody holds gets, so a seat cannot
/// map the enemy by asking for every id.
#[test]
fn a_fogged_seat_is_told_a_beacon_it_cannot_see_is_not_there() {
    let mut surface = hosted();
    let token = seat_token(&mut surface, 0);
    let (own, _, _, _) = own_beacon(&surface);

    let mine = surface.call(
        Some(&token),
        &request("get_beacon", &format!(r#"{{"beacon_id":"{own}"}}"#)),
        &Blind,
    );
    let _ = result(&mine, "the seat's own beacon");

    let others: Vec<String> = {
        let host = surface.host().expect("a hosted match");
        let beacons = host.world().beacons();
        beacons
            .seats()
            .iter()
            .enumerate()
            .filter(|(_, seat)| **seat != 0)
            .filter_map(|(row, _)| beacons.ids().get(row).copied())
            .map(|id| pharmakos_gateway::view::beacon_id(pharmakos_sim::tables::BeaconId::new(id)))
            .collect()
    };
    let enemy = others.first().expect("seat 1 has a core beacon");
    let refused = surface.call(
        Some(&token),
        &request("get_beacon", &format!(r#"{{"beacon_id":"{enemy}"}}"#)),
        &Blind,
    );
    assert_eq!(code(&refused, "somebody else's beacon"), "NOT_FOUND");
    let nobodys = surface.call(
        Some(&token),
        &request("get_beacon", r#"{"beacon_id":"b_99"}"#),
        &Blind,
    );
    assert_eq!(
        code(&nobodys, "a beacon nobody holds"),
        "NOT_FOUND",
        "the same answer, which is the point"
    );

    // And a fogged listing shows the seat its own and nothing else.
    let listed = result(
        &surface.call(Some(&token), &request("list_beacons", "{}"), &Blind),
        "list_beacons",
    );
    assert_eq!(beacon_ids(&listed), vec![own]);
}

/// `get_recap` and `get_economy_forecast` answer under their own scope and
/// phase gate.
///
/// The forecast's body is deliberately empty -- every number in it is T14's
/// economy and the message reserves 1 to 15 for them -- so what is asserted is
/// what exists today: it answers, it carries the footer, and it refuses a
/// what-if that names something the vocabulary does not have.
#[test]
fn get_recap_and_get_economy_forecast_answer_under_their_gate() {
    let mut surface = hosted();
    let token = seat_token(&mut surface, 0);

    let recap = result(
        &surface.call(Some(&token), &request("get_recap", "{}"), &Blind),
        "get_recap",
    );
    assert!(!text_of(&recap, "prose").is_empty());

    let forecast = result(
        &surface.call(
            Some(&token),
            &request("get_economy_forecast", r#"{"what_ifs":[{},{}]}"#),
            &Blind,
        ),
        "get_economy_forecast",
    );
    assert!(
        forecast.get("_status").is_some(),
        "the envelope still carries the footer"
    );
    let named = surface.call(
        Some(&token),
        &request(
            "get_economy_forecast",
            r#"{"what_ifs":[{"build":"refinery"}]}"#,
        ),
        &Blind,
    );
    assert_eq!(
        code(
            &named,
            "a what-if naming a field the vocabulary has not got"
        ),
        "INVALID_ARGUMENT"
    );
    let too_many = surface.call(
        Some(&token),
        &request(
            "get_economy_forecast",
            r#"{"detail":"brief","what_ifs":[{},{},{}]}"#,
        ),
        &Blind,
    );
    assert_eq!(
        code(&too_many, "more what-ifs than the brief budget"),
        "INVALID_ARGUMENT"
    );
}

/// `estimate_route` refuses a route longer than it will search, before it
/// searches any of it.
#[test]
fn estimate_route_refuses_more_waypoints_than_it_will_search() {
    let mut surface = hosted();
    let token = seat_token(&mut surface, 0);
    let (cx, cy, cz) = commander_at(&surface);
    let (_, bx, by, bz) = own_beacon(&surface);

    let mut points = String::new();
    for index in 0..=pharmakos_gateway::surface::knowledge::MAX_WAYPOINTS {
        if index > 0 {
            points.push(',');
        }
        let (x, y, z) = if index % 2 == 0 {
            (cx, cy, cz)
        } else {
            (bx, by, bz)
        };
        let point = format!(r#"{{"voxel":{{"x":{x},"y":{y},"z":{z}}}}}"#);
        points.push_str(&point);
    }
    let refused = surface.call(
        Some(&token),
        &request("estimate_route", &format!(r#"{{"waypoints":[{points}]}}"#)),
        &Blind,
    );
    assert_eq!(
        code(&refused, "a route past the cap"),
        "INVALID_ARGUMENT",
        "one search per leg and no estimate cache: the count is the caller's \
         multiplier on the whole process"
    );
    let one = surface.call(
        Some(&token),
        &request(
            "estimate_route",
            r#"{"waypoints":[{"voxel":{"x":1,"y":1,"z":1}}]}"#,
        ),
        &Blind,
    );
    assert_eq!(code(&one, "a route of one waypoint"), "INVALID_ARGUMENT");
}

/// `instantiate_template` against a real library folder, and against the ids a
/// hostile client sends.
///
/// The second half is the one that matters: no refusal may name the folder the
/// gateway reads, because where the library lives is the host's business and a
/// seat that learned it learned the layout of the machine it is playing on.
#[test]
fn instantiate_template_reads_the_library_and_names_no_path_when_it_refuses() {
    let folder = target_dir().join("t13-library");
    fs::create_dir_all(&folder).expect("a library folder");
    fs::write(folder.join("expand_east.jsonc"), TEMPLATE).expect("a template");
    let mut surface = hosted_as(SEGMENT_MS, FogPolicy::fogged(), Some(folder.clone()));
    let token = seat_token(&mut surface, 0);

    let listed = result(
        &surface.call(Some(&token), &request("list_templates", "{}"), &Blind),
        "list_templates",
    );
    match listed.get("templates") {
        Some(Json::Array(rows)) => assert_eq!(rows.len(), 1, "one template in the folder"),
        other => panic!("templates is an array, and it is {other:?}"),
    }

    let made = result(
        &surface.call(
            Some(&token),
            &request(
                "instantiate_template",
                r#"{"template_id":"expand_east","parameters":[
                     {"name":"/declarative/route/0/place_beacon/at/voxel/x","value":"40"}]}"#,
            ),
            &Blind,
        ),
        "instantiate_template",
    );
    let playbook = text_of(&made, "playbook_jsonc");
    assert!(
        playbook.contains("// the site the wizard fills in"),
        "a template's comments survive instantiation: {playbook}"
    );
    assert!(playbook.contains("\"PLAYBOOK\""), "{playbook}");
    let verified = result(
        &surface.call(
            Some(&token),
            &request(
                "verify_plan",
                &format!(r#"{{"playbook_jsonc":{}}}"#, quote(&playbook)),
            ),
            &Blind,
        ),
        "verify_plan on the instantiated template",
    );
    assert_eq!(
        verified
            .get("report")
            .and_then(|report| report.get("depth")),
        Some(&Json::String(String::from("full")))
    );

    let library = folder.display().to_string();
    for hostile in [
        "..%2Fsecret",
        "ok\u{0}",
        "../secret",
        "..\\secret",
        "C:\\Windows\\win",
        "/etc/passwd",
        "con",
        "ok/../../secret",
        "no_such_template",
    ] {
        let refused = surface.call(
            Some(&token),
            &request(
                "instantiate_template",
                &format!(r#"{{"template_id":{}}}"#, quote(hostile)),
            ),
            &Blind,
        );
        let message = refused
            .get("error")
            .and_then(|error| error.get("message"))
            .map_or_else(String::new, |value| format!("{value:?}"));
        assert!(
            refused.get("result").is_none(),
            "`{hostile}` was instantiated"
        );
        assert!(
            !message.contains(&library) && !message.contains("os error"),
            "the refusal of `{hostile}` names the host's filesystem: {message}"
        );
    }
}

/// A template file, in the shape `plan-core`'s own library tests use.
const TEMPLATE: &str = concat!(
    "// Expand east, then mine.\n",
    "{\"schema_version\":{\"major\":1},\n",
    " \"meta\":{\"title\":\"Expand & Mine\",\"author_kind\":\"HUMAN\",",
    "\"note\":\"Walks east and puts a Mine beacon down.\"},\n",
    " \"declarative\":{\"route\":[\n",
    "   // the site the wizard fills in\n",
    "   {\"label\":\"go\",\"place_beacon\":{\"at\":{\"voxel\":{\"x\":1,\"y\":2,\"z\":3}},",
    "\"tags\":[\"east\"]}}]},\n",
    " \"on_death\":{\"on_respawn\":\"CONTINUE\"},\n",
    " \"fallback\":{\"hold\":{\"at\":{\"beacon_anchor\":{\"safest\":{}}}}},\n",
    " \"kind\":\"TEMPLATE\"}\n"
);

/// `get_safe_plan` hands back a playbook the verifier qualifies.
///
/// Spec section 14 and item 81: the editor can render what a timeout would
/// file, so the cost of a timeout is visible. A safe playbook the verifier
/// refused would make that promise a lie.
#[test]
fn get_safe_plan_returns_a_playbook_verify_plan_qualifies() {
    let mut surface = hosted();
    let token = seat_token(&mut surface, 0);
    let safe = result(
        &surface.call(Some(&token), &request("get_safe_plan", "{}"), &Blind),
        "get_safe_plan",
    );
    let playbook = text_of(&safe, "playbook_jsonc");
    assert_eq!(playbook, pharmakos_gateway::host::SAFE_PLAYBOOK);
    let report = result(
        &surface.call(
            Some(&token),
            &request(
                "verify_plan",
                &format!(
                    r#"{{"depth":"full","playbook_jsonc":{}}}"#,
                    quote(&playbook)
                ),
            ),
            &Blind,
        ),
        "verify_plan on the safe playbook",
    );
    assert_eq!(
        report
            .get("report")
            .and_then(|report| report.get("qualifies")),
        Some(&Json::Bool(true)),
        "what the gateway would file for a seat that ran out of time must qualify"
    );
}

// ---------------------------------------------------------------------------
// The bounds are refusals, and they are tested like refusals
// ---------------------------------------------------------------------------

/// Every transport bound this lane introduced, refused rather than truncated.
#[test]
fn the_transport_bounds_refuse_rather_than_silently_correcting() {
    let mut surface = hosted();
    let token = seat_token(&mut surface, 0);

    let long_label = "x".repeat(pharmakos_gateway::surface::planning::MAX_DRAFT_LABEL_CHARS + 1);
    let refused = surface.call(
        Some(&token),
        &request(
            "save_draft",
            &format!(
                r#"{{"label":{},"playbook_jsonc":{}}}"#,
                quote(&long_label),
                quote(BROKEN)
            ),
        ),
        &Blind,
    );
    assert_eq!(
        code(&refused, "an over-long draft label"),
        "INVALID_ARGUMENT"
    );

    let refused = surface.call(
        Some(&token),
        &request(
            "save_draft",
            &format!(
                r#"{{"draft_id":"Draft-1","playbook_jsonc":{}}}"#,
                quote(BROKEN)
            ),
        ),
        &Blind,
    );
    assert_eq!(code(&refused, "an upper-case draft id"), "INVALID_ARGUMENT");

    let long_playbook = "x".repeat(pharmakos_gateway::surface::planning::MAX_PLAYBOOK_CHARS + 1);
    let refused = surface.call(
        Some(&token),
        &request(
            "verify_plan",
            &format!(r#"{{"playbook_jsonc":{}}}"#, quote(&long_playbook)),
        ),
        &Blind,
    );
    assert_eq!(code(&refused, "an over-long playbook"), "INVALID_ARGUMENT");

    let refused = surface.call(
        Some(&token),
        &request(
            "wait_for",
            &format!(
                r#"{{"trigger":"phase_change","timeout_ms":{}}}"#,
                i64::from(pharmakos_gateway::surface::MAX_WAIT_MS) + 1
            ),
        ),
        &Blind,
    );
    assert_eq!(code(&refused, "a timeout over the cap"), "INVALID_ARGUMENT");

    // The 33rd draft, one save at a time, with the cap coming from the crate
    // rather than from a number written twice. Through `call`, which reports
    // the client's own timer between calls the way a real editor does -- the
    // rate limiter is counted in ticks and thirty-three saves in one of them is
    // a different refusal from the one this is about.
    let cap = pharmakos_gateway::surface::planning::MAX_DRAFTS;
    let mut left = LULL_MS;
    for number in 0..cap {
        let saved = call(
            &mut surface,
            &token,
            &mut left,
            "save_draft",
            &format!(
                r#"{{"draft_id":"held-{number}","playbook_jsonc":{}}}"#,
                quote(BROKEN)
            ),
        );
        let _ = result(&saved, "a draft inside the cap");
    }
    let refused = call(
        &mut surface,
        &token,
        &mut left,
        "save_draft",
        &format!(
            r#"{{"draft_id":"one-too-many","playbook_jsonc":{}}}"#,
            quote(BROKEN)
        ),
    );
    assert_eq!(code(&refused, "one draft past the cap"), "INVALID_ARGUMENT");
    // Saving over one it already holds is not a new draft and is still allowed.
    let _ = result(
        &call(
            &mut surface,
            &token,
            &mut left,
            "save_draft",
            &format!(
                r#"{{"draft_id":"held-0","playbook_jsonc":{}}}"#,
                quote(BROKEN)
            ),
        ),
        "saving over a draft it already holds",
    );
}

/// An auto-named `save_draft` never replaces a draft the client named itself.
#[test]
fn an_auto_named_draft_never_lands_on_an_id_the_client_already_holds() {
    let mut surface = hosted();
    let token = seat_token(&mut surface, 0);
    let round = surface.host().expect("a hosted match").runner().round();

    // The client names the id an auto-name would have reached for second.
    let named = result(
        &surface.call(
            Some(&token),
            &request(
                "save_draft",
                &format!(
                    r#"{{"draft_id":"d{round}-2","label":"HAND NAMED","playbook_jsonc":{}}}"#,
                    quote(BROKEN)
                ),
            ),
            &Blind,
        ),
        "a hand-named draft",
    );
    assert_eq!(text_of(&named, "draft_id"), format!("d{round}-2"));

    let auto = result(
        &surface.call(
            Some(&token),
            &request(
                "save_draft",
                &format!(r#"{{"label":"AUTO","playbook_jsonc":{}}}"#, quote(BROKEN)),
            ),
            &Blind,
        ),
        "an auto-named draft",
    );
    assert_ne!(
        text_of(&auto, "draft_id"),
        format!("d{round}-2"),
        "the auto-name landed on the id the client had already taken"
    );

    let listed = result(
        &surface.call(Some(&token), &request("list_drafts", "{}"), &Blind),
        "list_drafts",
    );
    let labels: Vec<String> = match listed.get("drafts") {
        Some(Json::Array(rows)) => rows
            .iter()
            .map(|row| match row.get("label") {
                Some(Json::String(label)) => label.clone(),
                other => panic!("a summary carries a label, and it is {other:?}"),
            })
            .collect(),
        other => panic!("drafts is an array, and it is {other:?}"),
    };
    assert_eq!(labels.len(), 2, "both drafts are there: {labels:?}");
    assert!(labels.iter().any(|label| label == "HAND NAMED"));
    assert!(labels.iter().any(|label| label == "AUTO"));
}

// ---------------------------------------------------------------------------
// The gateway's own tick is monotonic
// ---------------------------------------------------------------------------

/// `MatchTime::tick` never goes backwards, in any phase or across any boundary.
///
/// A Lull consumes no sim tick, so the gateway derives the Lull's elapsed ticks
/// from `rules.match.lull_ms` and what the client says is left. The trap is the
/// boundary: a derivation that only applied *inside* the Lull would drop the
/// reported tick by a whole Lull the instant the Push began -- and
/// `limit::RateLimiter::admit` refuses to refill a budget from a tick that went
/// backwards, so a seat would have one tick's worth of calls for the whole
/// Push. So the Lull's ticks are carried, and this walks a whole round to say
/// so.
#[test]
fn the_gateways_tick_never_goes_backwards_across_a_phase_boundary() {
    let mut surface = hosted();
    let mut seen: Vec<(String, u32)> = Vec::new();
    let mut note = |what: &str, surface: &Surface| {
        seen.push((what.to_owned(), surface.time().tick.raw()));
    };

    note("the opening Lull", &surface);
    // Half of LULL_MS, then none of it, written out rather than divided: the
    // rounding lint is denied inside a test too, and a literal is clearer than
    // a checked division for a number this file already fixes.
    for left in [90_000, 0] {
        surface.set_phase_remaining_ms(Ms::new(left));
        note("the Lull, counting down", &surface);
    }
    surface.begin_push().expect("the Push begins");
    note("the Push begins", &surface);
    while let Some(report) = surface.step().expect("a tick") {
        note("a tick of the Push", &surface);
        if report.segment_ended {
            break;
        }
    }
    // A client reporting the recap's own timer, which is not the Lull's.
    surface.set_phase_remaining_ms(Ms::new(8_000));
    note("the recap", &surface);
    surface.end_recap().expect("the recap ends");
    surface.open_lull().expect("the next Lull");
    note("round 2's Lull", &surface);
    surface.set_phase_remaining_ms(Ms::new(LULL_MS));
    note("round 2's Lull, freshly started", &surface);

    let mut highest = 0;
    for (what, tick) in &seen {
        assert!(
            *tick >= highest,
            "the tick went backwards at `{what}`: {tick} after {highest}. The whole walk: \
             {seen:?}"
        );
        highest = *tick;
    }
    let opening = seen.first().map(|(_, tick)| *tick).unwrap_or_default();
    assert!(
        highest > opening,
        "a round went by and the tick did not move: {seen:?}"
    );
}

/// A seat that makes one call per tick of the Push is never rate-limited.
///
/// The property the boundary bug broke, stated as a client would notice it: a
/// watch rig polling the feed as fast as the caps allow gets answers, for the
/// whole segment, after a Lull that ran its full length.
#[test]
fn a_seat_calling_once_a_tick_through_a_push_is_never_rate_limited() {
    // Twenty seconds of Push: 400 ticks, which is two whole rate-limiter
    // windows and far more than the per-tick cap could cover on its own.
    let mut surface = hosted_with(20_000);
    let token = seat_token(&mut surface, 0);
    // The client counts its Lull all the way down, which is what put the
    // gateway's tick a Lull ahead of the runner's.
    surface.set_phase_remaining_ms(Ms::ZERO);
    surface.begin_push().expect("the Push begins");

    let mut calls = 0_u32;
    while let Some(report) = surface.step().expect("a tick") {
        let response = surface.call(Some(&token), &request("get_status", "{}"), &Blind);
        calls = calls.saturating_add(1);
        assert!(
            response.get("result").is_some(),
            "call {calls}, at sim tick {}, was refused: {:?}",
            report.tick.raw(),
            response.get("error")
        );
        if report.segment_ended {
            break;
        }
    }
    assert!(
        calls > 300,
        "the Push was meant to be 400 ticks, and {calls}"
    );
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
