// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every message the gateway sends is bounded (S3's `outcap`).
//!
//! The gateway caps what it reads (`MAX_MESSAGE_BYTES`, 256 KiB) and, since
//! this lane, what it sends: an answer's text is at most
//! `pharmakos_proto::transport::MAX_ANSWER_BYTES`. A call whose answer would
//! pass it is answered with the typed `RESPONSE_TOO_LARGE` instead, never a cut
//! text, and the connection stays open; the same holds for a seat this process
//! plays or advises, whose door has no wire (AGENTS.md section 3 rule 3: same
//! door, same cap).
//!
//! The property test drives every method of the schema with generated
//! requests up to the inbound cap and holds every answer to the outgoing cap
//! with only typed errors escaping. The constructed worst cases come from
//! `fixq-c`'s review 3: a `render_plan` of a request at the inbound cap made of
//! bare holds answers more than ten MiB of prose and rule entries, and that is
//! the one kind the backstop is reachable by today (its own cut is `outcap`
//! part 2's, after the typed left-out field).
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use pharmakos_gateway::error::Code;
use pharmakos_gateway::fog::Blind;
use pharmakos_gateway::frame::Opcode;
use pharmakos_gateway::handshake::Policy;
use pharmakos_gateway::limit::Limits;
use pharmakos_gateway::rpc;
use pharmakos_gateway::scopes::{Method, method_wire_name};
use pharmakos_gateway::serve::{Advisor, BuiltInSeat, InProcessSeats, Operators};
use pharmakos_gateway::session::{self, Ended};
use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::token::Token;
use pharmakos_proto::json::{self, Json};
use pharmakos_proto::transport::{MAX_ANSWER_BYTES, MAX_ID_BYTES, MAX_MESSAGE_BYTES};
use pharmakos_sim::tables::SeatId;
use std::io::{self, Read, Write};

use support::{admin_token, hosted, seat_token};

const PORT: u16 = 9500;

/// Limits no test here meets: the property test makes hundreds of calls in
/// one Lull, and a `RATE_LIMITED` answer is small whatever was asked.
fn unlimited(surface: &mut Surface) {
    surface.set_limits(Limits {
        per_tick: u32::MAX,
        per_window: u32::MAX,
        window_ticks: 1,
    });
}

/// A request's text as a client writes it, with `id` spelt as given.
fn request_text(id: &str, method: &str, params: &Json) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":{},"params":{}}}"#,
        json::write(&Json::String(method.to_owned())).trim_end(),
        json::write(params).trim_end()
    )
}

/// The answer to one request text, as the wire spells it: the same path a
/// socket's door takes (`rpc::parse`, then `Surface::call`, or the
/// malformed answer).
fn answer_text(surface: &mut Surface, token: &Token, text: &str) -> String {
    let response = match rpc::parse(text) {
        Ok(request) => surface.call(Some(token), &request, &Blind),
        Err(malformed) => rpc::malformed_response(&malformed),
    };
    rpc::render(&response)
}

/// An answer is a result, or an error whose code is one of the closed set.
fn assert_typed(answer: &str, what: &str) {
    let value = json::read(answer).expect("an answer is JSON");
    if value.get("result").is_some() {
        assert!(value.get("error").is_none(), "{what}: never both");
        return;
    }
    let error = value
        .get("error")
        .unwrap_or_else(|| panic!("{what}: neither a result nor an error"));
    match error.get("data").and_then(|data| data.get("code")) {
        Some(Json::String(name)) => assert!(
            Code::from_str_name(name).is_some(),
            "{what}: `{name}` is not in the closed set"
        ),
        // A message that was not a request carries JSON-RPC's own number
        // and no gateway code (`rpc::malformed_response`).
        None => assert!(
            matches!(error.get("code"), Some(Json::Number(number)) if number == "-32700" || number == "-32600"),
            "{what}: an error with no code: {error:?}"
        ),
        Some(other) => panic!("{what}: a code that is not a name: {other:?}"),
    }
}

/// The code of an answer's error, or `<none>`.
fn code_of(answer: &str) -> String {
    support::code(&json::read(answer).expect("JSON"))
}

/// A deterministic generator: xorshift64*, seeded per case, so a failing
/// case is named by its seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, bound: usize) -> usize {
        let bound = u64::try_from(bound.max(1)).expect("fits");
        usize::try_from(self.next() % bound).expect("fits")
    }
}

/// Every parameter name a handler reads (`request.param`, `string_param`,
/// `integer_param`, `bool_param`), and the members the planning wire's
/// objects carry: what the generator fills.
const NAMES: &[&str] = &[
    "anything",
    "beacon_id",
    "blueprint_id",
    "corner_a",
    "corner_b",
    "cursor",
    "depth",
    "draft_id",
    "elapsed_ms",
    "json_patch",
    "label",
    "limit",
    "missing",
    "ms",
    "notes",
    "parameters",
    "part",
    "playbook_jsonc",
    "pointer",
    "ready",
    "remaining_ms",
    "suggested",
    "tag",
    "template_id",
    "timeout_ms",
    "trigger",
    "typed",
    "waypoints",
    "what_ifs",
];

/// One generated value, at most about `budget` bytes once written.
fn value(rng: &mut Rng, budget: usize, depth: u32) -> Json {
    match rng.below(if depth > 2 { 4 } else { 6 }) {
        0 => Json::Bool(rng.below(2) == 1),
        1 => Json::Number(
            ["0", "1", "-1", "4096", "2147483648", "99999999999999999999"]
                .get(rng.below(6))
                .copied()
                .unwrap_or("0")
                .to_owned(),
        ),
        2 | 3 => {
            let unit = ["x", "\"", "\\", "\u{1}", "é", "€", "{\"hold\":{}},"]
                .get(rng.below(7))
                .copied()
                .unwrap_or("x");
            let count = rng
                .below(budget.max(1))
                .checked_div(unit.len().max(1))
                .unwrap_or(0);
            Json::String(unit.repeat(count))
        }
        4 => {
            let items = rng.below(8);
            Json::Array(
                (0..items)
                    .map(|_| value(rng, budget.checked_div(8).unwrap_or(0), depth + 1))
                    .collect(),
            )
        }
        _ => {
            let members = rng.below(6);
            Json::Object(
                (0..members)
                    .map(|_| {
                        let name = NAMES.get(rng.below(NAMES.len())).copied().unwrap_or("x");
                        (
                            name.to_owned(),
                            value(rng, budget.checked_div(6).unwrap_or(0), depth + 1),
                        )
                    })
                    .collect(),
            )
        }
    }
}

/// One generated request's params: a few names filled, one of them with
/// most of the inbound cap.
fn params(rng: &mut Rng) -> Json {
    let budget = [64, 4 * 1024, 64 * 1024, MAX_MESSAGE_BYTES]
        .get(rng.below(4))
        .copied()
        .unwrap_or(64);
    let members = 1 + rng.below(4);
    Json::Object(
        (0..members)
            .map(|index| {
                let name = NAMES.get(rng.below(NAMES.len())).copied().unwrap_or("x");
                let share = if index == 0 { budget } else { 256 };
                (name.to_owned(), value(rng, share, 0))
            })
            .collect(),
    )
}

/// Every method of the schema, by its wire name.
fn methods() -> Vec<String> {
    (1..=255)
        .filter_map(|number| Method::try_from(number).ok())
        .map(method_wire_name)
        .collect()
}

/// A playbook of `steps` copies of `step`, as compact as a player could send.
fn playbook(step: &str, steps: usize) -> String {
    let route: Vec<&str> = std::iter::repeat_n(step, steps).collect();
    format!(
        r#"{{"schema_version":{{"major":1}},"declarative":{{"route":[{}]}}}}"#,
        route.join(",")
    )
}

/// The params `{"playbook_jsonc": <the most copies of step a request at the
/// inbound cap carries>}` for `method`.
fn largest_playbook(method: &str, step: &str) -> Json {
    largest(method, |steps| playbook(step, steps))
}

/// The params `{"playbook_jsonc": text(n)}` for the largest `n` whose request
/// is within the inbound cap; `text` grows with `n`.
fn largest(method: &str, text: impl Fn(usize) -> String) -> Json {
    let at = |n: usize| {
        Json::Object(vec![(
            String::from("playbook_jsonc"),
            Json::String(text(n)),
        )])
    };
    let (mut low, mut high) = (1_usize, MAX_MESSAGE_BYTES);
    while low < high {
        let middle = low.saturating_add(high).div_ceil(2);
        if request_text("9999", method, &at(middle)).len() <= MAX_MESSAGE_BYTES {
            low = middle;
        } else {
            high = middle.saturating_sub(1);
        }
    }
    at(low)
}

/// A playbook inside the size budget (`verifier.size_budget_units`, 128: the
/// rules table's) and the verifier's limits, as large as a request carries:
/// 128 route steps, each with a condition of the most nodes a condition may
/// have (24: one `any` and 23 leaves) and a label of `label` repeated `n`
/// times. Free text is what a unit does not bound.
fn in_budget(label: &str, n: usize) -> String {
    let leaf = r#"{"cmdr_hp_pct":{"cmp":"LE","pct":40}}"#;
    let condition = format!(
        r#"{{"any":{{"items":[{}]}}}}"#,
        std::iter::repeat_n(leaf, 23).collect::<Vec<_>>().join(",")
    );
    let words = label.repeat(n);
    let steps: Vec<String> = (0..128)
        .map(|index| {
            format!(r#"{{"label":"{words}{index}","skip_if":{condition},"hold":{{"ms":1000}}}}"#)
        })
        .collect();
    format!(
        r#"{{"schema_version":{{"major":1}},"meta":{{"title":"in budget","author_kind":"HUMAN"}},"kind":"PLAYBOOK","declarative":{{"route":[{}]}}}}"#,
        steps.join(",")
    )
}

/// **An in-budget playbook's answers fit** (S3's `outcap`): a playbook within
/// the size budget and the verifier's limits, at the inbound cap, its labels
/// plain or of the characters a quoting escapes, is verified at both depths
/// and submitted with answers within the outgoing cap: the backstop is not
/// reached by those for a playbook a player can seal. Its render is answered
/// typed and within the cap too, and printed: its own cut is `outcap` part 2's.
#[test]
fn an_in_budget_playbook_at_the_inbound_cap_answers_within_the_cap() {
    let mut surface = hosted(2, support::SEGMENT_MS, 3);
    unlimited(&mut surface);
    let seat = seat_token(&mut surface, 0);
    for label in ["x", "\\u0001", "\u{1F600}"] {
        for (method, depth) in [
            ("render_plan", None),
            ("verify_plan", Some("quick")),
            ("verify_plan", Some("full")),
            ("submit_plan", None),
        ] {
            let mut params = largest(method, |n| in_budget(label, n));
            if let (Some(depth), Json::Object(entries)) = (depth, &mut params) {
                entries.push((String::from("depth"), Json::String(depth.to_owned())));
            }
            let text = request_text("1", method, &params);
            let answer = answer_text(&mut surface, &seat, &text);
            println!(
                "in budget, label {label:?}: {method} {depth:?}: request {} bytes, answer {} bytes: {}",
                text.len(),
                answer.len(),
                support::code(&json::read(&answer).expect("JSON")),
            );
            assert!(answer.len() <= MAX_ANSWER_BYTES);
            if method == "render_plan" {
                // The prose of an in-budget playbook can pass the cap: the
                // render's own cut, in plan-core's terms, is `outcap` part 2's
                // (it needs the typed left-out field). Until it lands the
                // backstop is what answers, typed, never a cut text.
                let words = json::read(&answer).expect("JSON");
                if code_of(&answer) == "RESPONSE_TOO_LARGE" {
                    println!(
                        "  withheld: {}",
                        support::text_of(words.get("error").expect("an error"), "message")
                    );
                }
                continue;
            }
            assert_ne!(
                code_of(&answer),
                "RESPONSE_TOO_LARGE",
                "{method} {depth:?} withheld an in-budget answer"
            );
        }
    }
}

/// **The property** (the brief's Finish): for every method, over generated
/// requests up to the inbound cap, from a seat and from the lobby, the
/// encoded answer is at most the outgoing cap and only typed errors escape.
#[test]
fn every_answer_of_every_method_fits_the_outgoing_cap() {
    const CASES: u64 = 12;
    for method in methods() {
        let mut surface = hosted(2, support::SEGMENT_MS, 3);
        unlimited(&mut surface);
        let seat = seat_token(&mut surface, 0);
        let admin = admin_token(&mut surface);
        // The generated cases, then the constructed one every method is
        // handed: a playbook of bare holds and of timed holds at the inbound
        // cap, which is where `render_plan` and the reports grow most.
        let constructed =
            [r#"{"hold":{}}"#, r#"{"hold":{"ms":1}}"#].map(|step| largest_playbook(&method, step));
        let mut largest_sent = 0_usize;
        let mut withheld = 0_u32;
        for case in 0..CASES.saturating_add(2) {
            let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ case.wrapping_mul(0x0100_0000_01b3));
            let params = match case.checked_sub(CASES) {
                Some(index) => constructed
                    .get(usize::try_from(index).expect("fits"))
                    .cloned()
                    .expect("two constructed cases"),
                None => params(&mut rng),
            };
            let text = request_text("1", &method, &params);
            if text.len() > MAX_MESSAGE_BYTES {
                continue;
            }
            for (who, token) in [("seat", &seat), ("lobby", &admin)] {
                let answer = answer_text(&mut surface, token, &text);
                let what = format!("{method} case {case} from the {who}");
                assert!(
                    answer.len() <= MAX_ANSWER_BYTES,
                    "{what}: a {}-byte request answered {} bytes, past {MAX_ANSWER_BYTES}",
                    text.len(),
                    answer.len()
                );
                assert_typed(&answer, &what);
                largest_sent = largest_sent.max(answer.len());
                if code_of(&answer) == "RESPONSE_TOO_LARGE" {
                    withheld = withheld.saturating_add(1);
                }
            }
        }
        println!("{method}: largest answer sent {largest_sent} bytes; {withheld} withheld");
    }
}

/// **The constructed worst case**: a `render_plan` of a request at the
/// inbound cap made of bare holds would answer past the outgoing cap. It is
/// answered with `RESPONSE_TOO_LARGE`, under its own id, naming the method
/// and both sizes, bounded itself; counted under its reason and logged.
#[test]
fn the_largest_render_is_withheld_with_a_typed_error() {
    let mut surface = hosted(2, support::SEGMENT_MS, 3);
    unlimited(&mut surface);
    let seat = seat_token(&mut surface, 0);
    for step in [r#"{"hold":{}}"#, r#"{"hold":{"ms":1}}"#] {
        let text = request_text(
            "\"render-1\"",
            "render_plan",
            &largest_playbook("render_plan", step),
        );
        assert!(text.len() <= MAX_MESSAGE_BYTES);
        let answer = answer_text(&mut surface, &seat, &text);
        assert!(
            answer.len() <= MAX_ANSWER_BYTES,
            "{step}: {} bytes sent, past {MAX_ANSWER_BYTES}",
            answer.len()
        );
        assert_eq!(code_of(&answer), "RESPONSE_TOO_LARGE", "{step}");
        let value = json::read(&answer).expect("JSON");
        assert_eq!(
            value.get("id"),
            Some(&Json::String(String::from("render-1"))),
            "under its own id"
        );
        let message = support::text_of(value.get("error").expect("an error"), "message");
        assert!(message.contains("render_plan"), "{message}");
        assert!(
            message.contains(&MAX_ANSWER_BYTES.to_string()),
            "names the cap: {message}"
        );
    }
    let logged = surface
        .audit()
        .entries()
        .iter()
        .filter(|entry| entry.action == "call render_plan")
        .filter(|entry| entry.outcome.render() == "RESPONSE_TOO_LARGE")
        .count();
    assert_eq!(logged, 2, "each withheld answer is in the audit log");
    assert_eq!(
        surface
            .audit()
            .withheld_count(pharmakos_gateway::audit::Withheld::TooLarge),
        2,
        "and counted under its reason"
    );
}

/// The same request in a Lull, through a socket: the error goes back as a
/// text frame within the cap, and the connection answers the next call.
#[test]
fn a_withheld_answer_leaves_the_connection_open() {
    let mut surface = hosted(2, support::SEGMENT_MS, 3);
    unlimited(&mut surface);
    let seat = seat_token(&mut surface, 0);
    let big = request_text(
        "1",
        "render_plan",
        &largest_playbook("render_plan", r#"{"hold":{}}"#),
    );
    let mut input = upgrade(&seat.render());
    input.extend_from_slice(&client_frame(big.as_bytes()));
    input.extend_from_slice(&client_frame(
        br#"{"jsonrpc":"2.0","id":2,"method":"get_status","params":{}}"#,
    ));
    let mut wire = Wire::new(input);
    let ended = session::serve(&mut wire, &mut surface, &Blind, &Policy::loopback(PORT));
    assert_eq!(ended, Ended::Disconnected, "the client stopped talking");
    let frames = server_frames(&wire.output);
    assert!(
        frames.iter().all(|(opcode, _)| *opcode == Opcode::Text),
        "no close: {:?}",
        frames.iter().map(|(opcode, _)| *opcode).collect::<Vec<_>>()
    );
    assert_eq!(frames.len(), 2, "both calls answered");
    let (_, first) = frames.first().expect("the first answer");
    assert!(first.len() <= MAX_ANSWER_BYTES);
    assert_eq!(
        code_of(std::str::from_utf8(first).expect("UTF-8")),
        "RESPONSE_TOO_LARGE"
    );
    let (_, second) = frames.get(1).expect("the second answer");
    let status = json::read(std::str::from_utf8(second).expect("UTF-8")).expect("JSON");
    assert!(status.get("result").is_some(), "{status:?}");
}

/// **The `id` is capped** (echoing a 256 KiB id is amplification): a string
/// or a number past `MAX_ID_BYTES` is an invalid request, answered under no
/// id; at the cap it is echoed; and the error answer at the worst id is
/// bounded by construction.
#[test]
fn an_id_past_its_cap_is_refused_and_never_echoed() {
    let mut surface = hosted(2, support::SEGMENT_MS, 3);
    unlimited(&mut surface);
    let seat = seat_token(&mut surface, 0);
    let at = "x".repeat(MAX_ID_BYTES);
    let past = "x".repeat(MAX_ID_BYTES.saturating_add(1));
    let digits = "1".repeat(MAX_ID_BYTES.saturating_add(1));
    let huge = "y".repeat(MAX_MESSAGE_BYTES.saturating_sub(200));

    let answer = answer_text(
        &mut surface,
        &seat,
        &request_text(
            &format!("\"{at}\""),
            "get_status",
            &Json::Object(Vec::new()),
        ),
    );
    assert!(answer.contains(&at), "an id at the cap is echoed");

    for id in [format!("\"{past}\""), digits, format!("\"{huge}\"")] {
        let text = request_text(&id, "get_status", &Json::Object(Vec::new()));
        let answer = answer_text(&mut surface, &seat, &text);
        let value = json::read(&answer).expect("JSON");
        assert_eq!(value.get("id"), Some(&Json::Null), "never echoed");
        assert!(
            matches!(value.get("error").and_then(|error| error.get("code")), Some(Json::Number(number)) if number == "-32600"),
            "an Invalid Request: {answer}"
        );
        assert!(
            answer.len() < 1024,
            "a {}-byte id answered {} bytes",
            id.len(),
            answer.len()
        );
    }
}

/// A casual two-seat match in its first Push whose ground carries `walls`
/// lone wall voxels of seat 1, none touching another by a face, so every seat
/// sees each as a wall of its own: an entity list far past one page, made the
/// way a match makes one (walls are voxels, and a cratered wall is many).
fn crowded(walls: usize) -> Surface {
    use pharmakos_sim::voxels::{Material, VoxelEdit};
    let mut surface = support::hosted_casual(2, 60_000, 3);
    assert!(surface.begin_push().expect("a hosted match"), "the Push");
    let mut sites: Vec<[i32; 3]> = Vec::new();
    'columns: for y in (100..300).step_by(2) {
        for x in (100..300).step_by(2) {
            if sites.len() == walls {
                break 'columns;
            }
            let world = surface.host().expect("a hosted match").world();
            let top = (0..64).rev().find(|z| {
                world
                    .voxels()
                    .get([x, y, *z])
                    .is_some_and(|m| m != Material::AIR)
            });
            if let Some(z) = top {
                sites.push([x, y, z]);
            }
        }
    }
    assert_eq!(sites.len(), walls, "the map has ground for every wall");
    for at in sites {
        loop {
            let filed =
                surface
                    .host_mut()
                    .expect("a hosted match")
                    .file_voxel_edit(VoxelEdit::Set {
                        at,
                        material: Material::WALL_SEAT_1,
                        by: None,
                    });
            if filed.is_ok() {
                break;
            }
            // The host's share of the edit queue is full: a tick takes it.
            assert_eq!(support::step(&mut surface, 1), 1, "the Push goes on");
        }
    }
    assert_eq!(support::step(&mut surface, 1), 1, "the last edits land");
    unlimited(&mut surface);
    surface
}

/// The ids of a page's entities.
fn entity_ids(page: &Json) -> Vec<String> {
    support::array_of(page, "entities")
        .iter()
        .map(|entity| support::text_of(entity, "id"))
        .collect()
}

/// One `get_view` page, as its result and its text's length on the wire.
fn view_page(surface: &mut Surface, token: &Token, cursor: &str) -> (Json, usize) {
    let text = request_text(
        "1",
        "get_view",
        &Json::Object(vec![(
            String::from("cursor"),
            Json::String(cursor.to_owned()),
        )]),
    );
    let answer = answer_text(surface, token, &text);
    let value = json::read(&answer).expect("JSON");
    (support::result(&value, "get_view"), answer.len())
}

/// **Entities are paged** (S3's `outcap`): an entity list past one page goes
/// on as many pages as it needs, each answer within the page's budget and its
/// envelope; every entity of the list comes once, none twice and none lost at
/// a page's edge, in the list's order; and the pages are cut from the one
/// list taken when the chunks ended, so the world moving between two pages
/// (the Push stepped) changes nothing in the listing being paged.
#[test]
fn an_entity_list_past_a_page_is_paged_from_one_list() {
    use pharmakos_gateway::viewfeed::VIEW_PAGE_BYTES;
    const ENVELOPE: usize = 2 * 1024;
    let mut surface = crowded(3_000);
    let seat = seat_token(&mut surface, 0);

    // The whole list with nothing stepped between its pages: the reference.
    let mut reference: Vec<String> = Vec::new();
    let mut walls = 0_usize;
    let mut cursor = String::new();
    let mut pages = 0_u32;
    loop {
        let (page, bytes) = view_page(&mut surface, &seat, &cursor);
        pages = pages.saturating_add(1);
        assert!(
            bytes <= VIEW_PAGE_BYTES.saturating_add(ENVELOPE),
            "page {pages}: {bytes} bytes"
        );
        reference.extend(entity_ids(&page));
        walls = walls.saturating_add(
            support::array_of(&page, "entities")
                .iter()
                .filter(|entity| entity.get("subtype") == Some(&Json::String(String::from("wall"))))
                .count(),
        );
        cursor = support::text_of(&page, "next_cursor");
        if matches!(page.get("complete"), Some(Json::Bool(true))) {
            break;
        }
        assert!(pages < 64, "a view that never completes");
    }
    let entity_pages = pages.saturating_sub(1);
    assert!(
        entity_pages >= 2,
        "{} entities on {pages} pages: the list did not need paging",
        reference.len()
    );
    let mut unique = reference.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), reference.len(), "an entity came twice");
    // Every lone wall voxel is a wall of its own: none lost at a page's edge.
    assert_eq!(walls, 3_000, "every wall, once");

    // A second keyframe from the same world, a wall the first page listed
    // knocked down and the Push stepped before the next: the same list, the
    // one taken when its chunks ended. A list taken again at each page would
    // be one shorter before the next page's start, and lose an entity there.
    let (first, _) = view_page(&mut surface, &seat, "");
    let gone = support::array_of(&first, "entities")
        .into_iter()
        .rev()
        .find(|entity| entity.get("subtype") == Some(&Json::String(String::from("wall"))))
        .expect("the first page lists a wall");
    let at = gone.get("at").expect("a place");
    let axis = |name: &str| match at.get(name) {
        Some(Json::Number(lexeme)) => lexeme.parse::<i32>().expect("a whole number"),
        other => panic!("`{name}` is a number, and it is {other:?}"),
    };
    support::erase(&mut surface, [axis("x"), axis("y"), axis("z")]);
    let mut listed = entity_ids(&first);
    let mut cursor = support::text_of(&first, "next_cursor");
    let mut complete = matches!(first.get("complete"), Some(Json::Bool(true)));
    while !complete {
        assert_eq!(support::step(&mut surface, 1), 1, "the Push moved");
        let (page, _) = view_page(&mut surface, &seat, &cursor);
        listed.extend(entity_ids(&page));
        cursor = support::text_of(&page, "next_cursor");
        complete = matches!(page.get("complete"), Some(Json::Bool(true)));
    }
    assert_eq!(listed, reference, "every page cut from the one list");

    // And the world did move: a fresh listing no longer lists that wall.
    let mut fresh: Vec<String> = Vec::new();
    let mut cursor = String::new();
    loop {
        let (page, _) = view_page(&mut surface, &seat, &cursor);
        fresh.extend(entity_ids(&page));
        cursor = support::text_of(&page, "next_cursor");
        if matches!(page.get("complete"), Some(Json::Bool(true))) {
            break;
        }
    }
    let wall = support::text_of(&gone, "id");
    assert!(!fresh.contains(&wall), "{wall} still listed");
}

/// A cursor into an entity list another seat's view holds is stale for this
/// seat: a list is held for its viewer.
#[test]
fn a_cursor_into_another_seats_list_is_stale() {
    let mut surface = crowded(3_000);
    let seat = seat_token(&mut surface, 0);
    let other = seat_token(&mut surface, 1);
    let (first, _) = view_page(&mut surface, &seat, "");
    assert!(matches!(first.get("complete"), Some(Json::Bool(false))));
    let mut cursor = support::text_of(&first, "next_cursor");
    // Page to the first entity cursor.
    while cursor.len() == 64 {
        let (page, _) = view_page(&mut surface, &seat, &cursor);
        cursor = support::text_of(&page, "next_cursor");
    }
    let answer = answer_text(
        &mut surface,
        &other,
        &request_text(
            "1",
            "get_view",
            &Json::Object(vec![(String::from("cursor"), Json::String(cursor))]),
        ),
    );
    assert_eq!(code_of(&answer), "STALE_SNAPSHOT", "{answer}");
}

/// An advisor that reads the view to its completing page and keeps what it
/// saw: how many walls, how many pages, and any refusal.
struct Reader {
    seen: std::sync::Arc<std::sync::Mutex<(usize, u32, String)>>,
}

impl Advisor for Reader {
    fn advise(
        &mut self,
        _seat: u8,
        call: &mut dyn FnMut(&rpc::Request) -> Json,
    ) -> pharmakos_gateway::advice::Advice {
        let mut cursor = String::new();
        let (mut walls, mut pages, mut refused) = (0_usize, 0_u32, String::new());
        while pages < 64 {
            let text = request_text(
                "1",
                "get_view",
                &Json::Object(vec![(String::from("cursor"), Json::String(cursor.clone()))]),
            );
            let answer = call(&rpc::parse(&text).expect("a request"));
            pages = pages.saturating_add(1);
            let Some(page) = answer.get("result") else {
                refused = support::code(&answer);
                break;
            };
            walls = walls.saturating_add(
                support::array_of(page, "entities")
                    .iter()
                    .filter(|entity| {
                        entity.get("subtype") == Some(&Json::String(String::from("wall")))
                    })
                    .count(),
            );
            cursor = support::text_of(page, "next_cursor");
            if matches!(page.get("complete"), Some(Json::Bool(true))) {
                break;
            }
        }
        if let Ok(mut held) = self.seen.lock() {
            *held = (walls, pages, refused);
        }
        pharmakos_gateway::advice::Advice::default()
    }
}

/// Advises seat 0 with a [`Reader`], and plays nobody.
struct Advising(Option<Box<dyn Advisor>>);

impl Operators for Advising {
    fn built_in(&mut self, _seat: u8) -> Option<Box<dyn BuiltInSeat>> {
        None
    }

    fn advisor(&mut self, seat: u8) -> Option<Box<dyn Advisor>> {
        if seat == 0 { self.0.take() } else { None }
    }
}

/// **An advisor pages its entities as a socket does** (S3's `outcap`): its
/// `get_view` is answered from a scratch copy of the feed, which is put back
/// after each call, and the entity list its listing holds is kept across its
/// calls, so its read reaches the completing page with every wall once.
#[test]
fn an_advisor_pages_its_entities_across_calls() {
    let mut surface = crowded(3_000);
    while let Some(report) = surface.step().expect("a tick") {
        if report.segment_ended {
            break;
        }
    }
    assert!(surface.end_recap().expect("the recap ends"));
    surface
        .set_phase_remaining_ms(pharmakos_sim::math::quantity::Ms::new(support::LULL_MS))
        .expect("a countdown");
    surface.open_lull().expect("round 2's Lull");
    let seen = std::sync::Arc::new(std::sync::Mutex::new((0, 0, String::new())));
    let mut factory = Advising(Some(Box::new(Reader {
        seen: std::sync::Arc::clone(&seen),
    })));
    let mut seats = InProcessSeats::open(
        &mut surface,
        &[SeatId::new(0), SeatId::new(1)],
        Some(0),
        &mut factory,
    )
    .expect("in-process seats");
    seats.plan(&mut surface);
    let (walls, pages, refused) = seen.lock().expect("not poisoned").clone();
    assert_eq!(refused, "", "the advisor's paging was refused");
    assert!(pages >= 3, "{pages} pages: the list was paged");
    assert_eq!(walls, 3_000, "every wall, once");
}

/// A built-in seat whose round is one call it is handed, and which keeps
/// what that call answered.
struct OneCall {
    request: rpc::Request,
    answered: std::sync::Arc<std::sync::Mutex<Option<Json>>>,
}

impl BuiltInSeat for OneCall {
    fn plan(&mut self, _seat: u8, call: &mut dyn FnMut(&rpc::Request) -> Json) {
        let answer = call(&self.request);
        if let Ok(mut held) = self.answered.lock() {
            *held = Some(answer);
        }
    }
}

/// Hands seat 1 its one call, and plays and advises nobody else.
struct OneSeat(Option<Box<dyn BuiltInSeat>>);

impl Operators for OneSeat {
    fn built_in(&mut self, seat: u8) -> Option<Box<dyn BuiltInSeat>> {
        if seat == 1 { self.0.take() } else { None }
    }

    fn advisor(&mut self, _seat: u8) -> Option<Box<dyn Advisor>> {
        None
    }
}

/// **The in-process door is capped too** (AGENTS.md section 3 rule 3: an
/// operator is a client of the same door, so an answer a socket could not be
/// sent is one a built-in seat is not handed either): a seat this process
/// plays asks the constructed `render_plan` and gets `RESPONSE_TOO_LARGE`.
#[test]
fn a_seat_this_process_plays_meets_the_same_cap() {
    let mut surface = hosted(2, support::SEGMENT_MS, 3);
    let text = request_text(
        "7",
        "render_plan",
        &largest_playbook("render_plan", r#"{"hold":{}}"#),
    );
    let answered = std::sync::Arc::new(std::sync::Mutex::new(None));
    let seat = OneCall {
        request: rpc::parse(&text).expect("a request"),
        answered: std::sync::Arc::clone(&answered),
    };
    let mut factory = OneSeat(Some(Box::new(seat)));
    let mut seats = InProcessSeats::open(
        &mut surface,
        &[SeatId::new(0), SeatId::new(1)],
        Some(0),
        &mut factory,
    )
    .expect("in-process seats");
    seats.plan(&mut surface);
    let answer = answered
        .lock()
        .expect("not poisoned")
        .take()
        .expect("the seat was asked to plan");
    let wire = rpc::render(&answer);
    assert!(wire.len() <= MAX_ANSWER_BYTES, "{} bytes", wire.len());
    assert_eq!(support::code(&answer), "RESPONSE_TOO_LARGE");
}

// ---------------------------------------------------------------------------
// A connection in a `Vec`, as `tests/websocket.rs` drives one.
// ---------------------------------------------------------------------------

struct Wire {
    input: Vec<u8>,
    cursor: usize,
    output: Vec<u8>,
}

impl Wire {
    fn new(input: Vec<u8>) -> Wire {
        Wire {
            input,
            cursor: 0,
            output: Vec::new(),
        }
    }
}

impl Read for Wire {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let rest = self.input.get(self.cursor..).unwrap_or_default();
        let take = rest.len().min(buffer.len());
        buffer
            .get_mut(..take)
            .unwrap_or_default()
            .copy_from_slice(rest.get(..take).unwrap_or_default());
        self.cursor = self.cursor.saturating_add(take);
        Ok(take)
    }
}

impl Write for Wire {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.output.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn upgrade(token: &str) -> Vec<u8> {
    format!(
        "GET /seat HTTP/1.1\r\n\
         Host: 127.0.0.1:{PORT}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         Sec-WebSocket-Version: 13\r\n\
         Authorization: Bearer {token}\r\n\
         \r\n"
    )
    .into_bytes()
}

/// A masked client text frame of any length.
fn client_frame(payload: &[u8]) -> Vec<u8> {
    let key = [0x37_u8, 0xfa, 0x21, 0x3d];
    let mut out: Vec<u8> = vec![0x80 | Opcode::Text.bits()];
    let length = payload.len();
    if length < 126 {
        out.push(0x80 | u8::try_from(length).expect("short"));
    } else if let Ok(short) = u16::try_from(length) {
        out.push(0x80 | 0x7e);
        out.extend_from_slice(&short.to_be_bytes());
    } else {
        out.push(0x80 | 0x7f);
        out.extend_from_slice(&u64::try_from(length).expect("fits").to_be_bytes());
    }
    out.extend_from_slice(&key);
    for (index, byte) in payload.iter().enumerate() {
        out.push(byte ^ key.get(index % 4).copied().unwrap_or(0));
    }
    out
}

/// Every frame the gateway sent after the handshake: opcode and payload.
fn server_frames(output: &[u8]) -> Vec<(Opcode, Vec<u8>)> {
    let start = output
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("the handshake ends")
        .saturating_add(4);
    let mut rest = output.get(start..).unwrap_or_default();
    let mut out = Vec::new();
    while let (Some(first), Some(second)) = (rest.first().copied(), rest.get(1).copied()) {
        let opcode = match first & 0x0f {
            0x1 => Opcode::Text,
            0x8 => Opcode::Close,
            0xA => Opcode::Pong,
            other => panic!("an opcode the gateway never sends: {other}"),
        };
        let (length, at): (usize, usize) = match second & 0x7f {
            126 => {
                let mut wide = [0_u8; 2];
                wide.copy_from_slice(rest.get(2..4).expect("a length"));
                (usize::from(u16::from_be_bytes(wide)), 4)
            }
            127 => {
                let mut wide = [0_u8; 8];
                wide.copy_from_slice(rest.get(2..10).expect("a length"));
                (usize::try_from(u64::from_be_bytes(wide)).expect("fits"), 10)
            }
            short => (usize::from(short), 2),
        };
        let end = at.saturating_add(length);
        out.push((opcode, rest.get(at..end).expect("a whole frame").to_vec()));
        rest = rest.get(end..).unwrap_or_default();
    }
    out
}
