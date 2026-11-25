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

/// The bytes `render_plan` answers for `text`, or would have answered when
/// it was withheld (its refusal names the size).
fn render_bytes(surface: &mut Surface, seat: &Token, text: &str) -> usize {
    let params = Json::Object(vec![(
        String::from("playbook_jsonc"),
        Json::String(text.to_owned()),
    )]);
    let answer = answer_text(surface, seat, &request_text("1", "render_plan", &params));
    if code_of(&answer) != "RESPONSE_TOO_LARGE" {
        return answer.len();
    }
    let value = json::read(&answer).expect("JSON");
    let words = support::text_of(value.get("error").expect("an error"), "message");
    words
        .split(" is ")
        .nth(1)
        .and_then(|rest| rest.split(' ').next())
        .and_then(|bytes| bytes.parse::<usize>().ok())
        .expect("the refusal names the size")
}

/// The size units `verify_plan` counts for `text`.
fn size_units(surface: &mut Surface, seat: &Token, text: &str) -> usize {
    let params = Json::Object(vec![
        (
            String::from("playbook_jsonc"),
            Json::String(text.to_owned()),
        ),
        (String::from("depth"), Json::String(String::from("quick"))),
    ]);
    let answer = answer_text(surface, seat, &request_text("1", "verify_plan", &params));
    let value = json::read(&answer).expect("JSON");
    let report = support::result(&value, "verify_plan")
        .get("report")
        .cloned()
        .expect("a report");
    match report.get("size_units") {
        Some(Json::Number(units)) => units.parse().expect("a count"),
        _ => 0,
    }
}

/// Every route step of the committed library and example playbooks, as
/// canonical JSON: the step kinds a search of in-budget renders tries.
fn committed_steps() -> Vec<String> {
    let root = support::workspace_root();
    // The library through its own reader, which lists it in id order.
    let library = root.join("library");
    let mut files: Vec<std::path::PathBuf> = pharmakos_plan_core::library::listing(&library)
        .expect("the library")
        .summaries
        .iter()
        .map(|summary| library.join(format!("{}.jsonc", summary.template_id)))
        .collect();
    files.push(root.join("examples/playbooks/expand_east.jsonc"));
    files.push(root.join("crates/verifier/tests/cases/budget_128.json"));
    let mut steps: Vec<String> = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).expect("a playbook");
        let Ok(canonical) = pharmakos_plan_core::canonical::canonicalise_text(&text) else {
            continue;
        };
        let value = json::read(&canonical.json).expect("canonical JSON");
        let route = value
            .get("declarative")
            .and_then(|body| body.get("route"))
            .cloned();
        if let Some(Json::Array(route)) = route {
            for step in route {
                let Json::Object(entries) = step else {
                    continue;
                };
                let kept: Vec<(String, Json)> = entries
                    .into_iter()
                    .filter(|(key, _)| key != "label" && key != "skip_if")
                    .collect();
                let written = json::write(&Json::Object(kept)).replace('\n', "");
                if !steps.contains(&written) {
                    steps.push(written);
                }
            }
        }
    }
    steps
}

/// One generated in-budget playbook: `count` copies of `step`, each with the
/// largest guard a condition may have and a label of `label` repeated `n`
/// times, in the route or as handler bodies.
fn in_budget_of(step: &str, count: usize, handlers: bool, label: &str, n: usize) -> String {
    let leaf = r#"{"cmdr_hp_pct":{"cmp":"LE","pct":40}}"#;
    let condition = format!(
        r#"{{"any":{{"items":[{}]}}}}"#,
        std::iter::repeat_n(leaf, 23).collect::<Vec<_>>().join(",")
    );
    let words = label.repeat(n);
    let body = step.trim_start_matches('{');
    if handlers {
        let handlers: Vec<String> = (0..count)
            .map(|index| {
                format!(
                    r#"{{"id":"h{index}","when":{condition},"body":[{{"label":"{words}{index}",{body}]}}"#
                )
            })
            .collect();
        format!(
            r#"{{"schema_version":{{"major":1}},"meta":{{"title":"in budget","author_kind":"HUMAN"}},"kind":"PLAYBOOK","declarative":{{"route":[{{"label":"r","hold":{{"ms":1000}}}}],"handlers":[{}]}}}}"#,
            handlers.join(",")
        )
    } else {
        let steps: Vec<String> = (0..count)
            .map(|index| format!(r#"{{"label":"{words}{index}","skip_if":{condition},{body}"#))
            .collect();
        format!(
            r#"{{"schema_version":{{"major":1}},"meta":{{"title":"in budget","author_kind":"HUMAN"}},"kind":"PLAYBOOK","declarative":{{"route":[{}]}}}}"#,
            steps.join(",")
        )
    }
}

/// **The search for the worst in-budget render** (S3's `outcap`, fix pass
/// 1, design call A): every committed step kind, in the route with a full
/// guard and as handler bodies under a full condition, as many copies as the
/// size budget takes, labels plain or escaped to the request cap. Prints each
/// figure and the worst; the worst found is the committed case below.
#[test]
#[ignore = "a search run by hand for the pull request's figure; its worst case is committed"]
fn the_worst_in_budget_render_is_searched() {
    let mut surface = hosted(2, support::SEGMENT_MS, 3);
    unlimited(&mut surface);
    let seat = seat_token(&mut surface, 0);
    let budget = 128_usize;
    let mut worst = (0_usize, String::new());
    for step in committed_steps() {
        for handlers in [false, true] {
            let one = size_units(
                &mut surface,
                &seat,
                &in_budget_of(&step, 1, handlers, "x", 1),
            );
            let two = size_units(
                &mut surface,
                &seat,
                &in_budget_of(&step, 2, handlers, "x", 1),
            );
            let each = two.saturating_sub(one).max(1);
            let base = one.saturating_sub(each);
            let most = if handlers { 64 } else { 128 };
            let count = budget
                .saturating_sub(base)
                .checked_div(each)
                .unwrap_or(0)
                .min(most);
            if count == 0 {
                continue;
            }
            for label in ["x", "\\u0001"] {
                let params = largest("render_plan", |n| {
                    in_budget_of(&step, count, handlers, label, n)
                });
                let text = support::text_of(&params, "playbook_jsonc");
                let units = size_units(&mut surface, &seat, &text);
                if units > budget {
                    continue;
                }
                let bytes = render_bytes(&mut surface, &seat, &text);
                println!(
                    "{bytes} bytes: {count} x {} ({} units), handlers {handlers}, label {label:?}",
                    step.chars().take(80).collect::<String>(),
                    units
                );
                if bytes > worst.0 {
                    worst = (
                        bytes,
                        format!("{count} x {step}, handlers {handlers}, label {label:?}"),
                    );
                }
            }
        }
    }
    println!("WORST: {} bytes: {}", worst.0, worst.1);
}

/// **An in-budget playbook's answers fit whole** (S3's `outcap`; fix pass 1,
/// design call A): a playbook within the size budget and the verifier's
/// limits, at the inbound cap, its labels plain or of characters a quoting
/// escapes, is verified at both depths, submitted and rendered with answers
/// sent whole, the render's with at least a quarter of the cap to spare. The
/// render's cases are the worst the search found
/// (`the_worst_in_budget_render_is_searched`): every committed `place_beacon`
/// step, 128 of them each under a full guard (5 667 582 bytes at the worst),
/// and the 24-node guarded hold. The backstop is not reached by a playbook a
/// player can seal.
#[test]
fn an_in_budget_playbook_at_the_inbound_cap_answers_whole() {
    let mut surface = hosted(2, support::SEGMENT_MS, 3);
    unlimited(&mut surface);
    let seat = seat_token(&mut surface, 0);
    let spare = MAX_ANSWER_BYTES
        .saturating_mul(3)
        .checked_div(4)
        .expect("a cap");
    for label in ["x", r"\u0001", "\u{1F600}"] {
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
                "in budget, label {label:?}: {method} {depth:?}: request {} bytes, answer {} bytes",
                text.len(),
                answer.len(),
            );
            assert_ne!(
                code_of(&answer),
                "RESPONSE_TOO_LARGE",
                "{method} {depth:?} withheld an in-budget answer"
            );
            assert!(
                answer.len() <= spare,
                "{} bytes, past {spare}",
                answer.len()
            );
        }
    }
    for step in committed_steps()
        .into_iter()
        .filter(|step| step.contains("place_beacon"))
    {
        let one = size_units(&mut surface, &seat, &in_budget_of(&step, 1, false, "x", 1));
        let two = size_units(&mut surface, &seat, &in_budget_of(&step, 2, false, "x", 1));
        let each = two.saturating_sub(one).max(1);
        let count = 128_usize
            .saturating_sub(one.saturating_sub(each))
            .checked_div(each)
            .unwrap_or(0)
            .min(128);
        for label in ["x", r"\u0001"] {
            let params = largest("render_plan", |n| {
                in_budget_of(&step, count, false, label, n)
            });
            let text = support::text_of(&params, "playbook_jsonc");
            assert!(size_units(&mut surface, &seat, &text) <= 128, "in budget");
            let bytes = render_bytes(&mut surface, &seat, &text);
            println!(
                "in budget, {count} place_beacon steps, label {label:?}: render {bytes} bytes"
            );
            assert!(bytes <= spare, "{bytes} bytes, past {spare}: {step}");
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
        .filter(|entry| entry.action == "withhold answer render_plan")
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

/// **The measure includes the footer** (S3's `outcap`, fix pass 1, item F,
/// mutant m01): an answer made through `Surface::call` whose text, `_status`
/// footer and all, is exactly the cap is sent; with the cap one byte lower it
/// is withheld. A measure taken before the footer was added would send it.
#[test]
fn the_measured_answer_is_the_one_sent_footer_and_all() {
    let mut surface = hosted(2, support::SEGMENT_MS, 3);
    unlimited(&mut surface);
    let token = seat_token(&mut surface, 0);
    let text = request_text(
        "1",
        "save_notes",
        &Json::Object(vec![(String::from("notes"), Json::String("n".repeat(100)))]),
    );
    let sent = answer_text(&mut surface, &token, &text);
    assert!(sent.contains("_status"), "the answer carries its footer");
    surface.lower_answer_cap(sent.len()).expect("lowered");
    assert_eq!(
        answer_text(&mut surface, &token, &text),
        sent,
        "at the cap: sent"
    );
    surface
        .lower_answer_cap(sent.len().saturating_sub(1))
        .expect("lowered");
    assert_eq!(
        code_of(&answer_text(&mut surface, &token, &text)),
        "RESPONSE_TOO_LARGE",
        "one byte under: withheld"
    );
    assert_eq!(
        surface
            .lower_answer_cap(MAX_ANSWER_BYTES.saturating_add(1))
            .expect_err("never raised")
            .code(),
        Code::InvalidArgument
    );
}

/// **The audit log tells the truth about a withheld answer** (S3's
/// `outcap`, fix pass 1, item E): a `submit_plan` accepted and whose answer is
/// withheld (the cap lowered for the test, `Surface::lower_answer_cap`, since
/// no in-budget answer reaches the real one) is logged as the call it was,
/// `ok`, with a second line saying its answer was withheld; and `get_status`
/// says the seat holds the seal it was refused the answer to.
#[test]
fn a_withheld_submission_is_logged_as_accepted() {
    let mut surface = hosted(2, support::SEGMENT_MS, 3);
    unlimited(&mut surface);
    let seat = seat_token(&mut surface, 0);
    // The gateway's own safe playbook for the seat: one that qualifies.
    let playbook = support::text_of(
        &support::result(
            &support::call(&mut surface, &seat, "get_safe_plan", "{}"),
            "get_safe_plan",
        ),
        "playbook_jsonc",
    );
    let text = request_text(
        "1",
        "submit_plan",
        &Json::Object(vec![(
            String::from("playbook_jsonc"),
            Json::String(playbook),
        )]),
    );
    surface.lower_answer_cap(256).expect("lowered");
    let answer = answer_text(&mut surface, &seat, &text);
    assert_eq!(code_of(&answer), "RESPONSE_TOO_LARGE", "withheld");
    let lines: Vec<(String, String)> = surface
        .audit()
        .entries()
        .iter()
        .map(|entry| (entry.action.clone(), entry.outcome.render()))
        .collect();
    assert!(
        lines.contains(&(String::from("call submit_plan"), String::from("ok"))),
        "the call is logged as made: {lines:?}"
    );
    assert!(
        lines.contains(&(
            String::from("withhold answer submit_plan"),
            String::from("RESPONSE_TOO_LARGE")
        )),
        "and its answer as withheld: {lines:?}"
    );
    surface
        .lower_answer_cap(MAX_ANSWER_BYTES)
        .expect("back at the cap");
    let status = answer_text(
        &mut surface,
        &seat,
        &request_text("2", "get_status", &Json::Object(Vec::new())),
    );
    let status = json::read(&status).expect("JSON");
    assert_eq!(
        support::result(&status, "get_status").get("sealed_this_round"),
        Some(&Json::Bool(true)),
        "the submission stands: {status:?}"
    );
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
    // The cap is in bytes, not characters: 32 four-byte characters are the
    // cap and echoed, 33 are past it.
    let quarter = MAX_ID_BYTES.checked_div(4).expect("a cap");
    let wide_at = "\u{1F600}".repeat(quarter);
    let wide_past = "\u{1F600}".repeat(quarter.saturating_add(1));
    let answer = answer_text(
        &mut surface,
        &seat,
        &request_text(
            &format!("\"{wide_at}\""),
            "get_status",
            &Json::Object(Vec::new()),
        ),
    );
    assert!(
        answer.contains(&wide_at),
        "32 four-byte characters are echoed"
    );
    let answer = answer_text(
        &mut surface,
        &seat,
        &request_text(
            &format!("\"{wide_past}\""),
            "get_status",
            &Json::Object(Vec::new()),
        ),
    );
    assert_eq!(
        json::read(&answer).expect("JSON").get("id"),
        Some(&Json::Null),
        "33 four-byte characters are past the cap"
    );

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

/// The ground voxel of column `[x, y]`: the highest that is not air.
fn ground(surface: &Surface, x: i32, y: i32) -> Option<[i32; 3]> {
    use pharmakos_sim::voxels::Material;
    let world = surface.host().expect("a hosted match").world();
    (0..64)
        .rev()
        .find(|z| {
            world
                .voxels()
                .get([x, y, *z])
                .is_some_and(|m| m != Material::AIR)
        })
        .map(|z| [x, y, z])
}

/// Turn each of `sites` into `material`, through the host's edit queue, a
/// tick at a time when its share is full, and let the last land.
fn lay(surface: &mut Surface, sites: &[[i32; 3]], material: pharmakos_sim::voxels::Material) {
    use pharmakos_sim::voxels::VoxelEdit;
    for at in sites {
        loop {
            let filed =
                surface
                    .host_mut()
                    .expect("a hosted match")
                    .file_voxel_edit(VoxelEdit::Set {
                        at: *at,
                        material,
                        by: None,
                    });
            if filed.is_ok() {
                break;
            }
            // The host's share of the edit queue is full: a tick takes it.
            assert_eq!(support::step(surface, 1), 1, "the Push goes on");
        }
    }
    assert_eq!(support::step(surface, 1), 1, "the last edits land");
}

/// `count` lone ground columns on a two-voxel grid in `[from, to)` on both
/// axes, none touching another by a face, skipping any within `clear` of
/// `away`.
fn lone_sites(
    surface: &Surface,
    count: usize,
    from: i32,
    to: i32,
    away: [i32; 3],
    clear: i32,
) -> Vec<[i32; 3]> {
    let mut sites: Vec<[i32; 3]> = Vec::new();
    'columns: for y in (from..to).step_by(2) {
        for x in (from..to).step_by(2) {
            if sites.len() == count {
                break 'columns;
            }
            let [ax, ay, _] = away;
            if (x - ax).abs() < clear && (y - ay).abs() < clear {
                continue;
            }
            if let Some(site) = ground(surface, x, y) {
                sites.push(site);
            }
        }
    }
    assert_eq!(sites.len(), count, "the map has ground for every wall");
    sites
}

/// A casual two-seat match in its first Push whose ground carries `walls`
/// lone wall voxels of seat 1, none touching another by a face, so every seat
/// sees each as a wall of its own: an entity list far past one page, made the
/// way a match makes one (walls are voxels, and a cratered wall is many).
fn crowded(walls: usize) -> Surface {
    let mut surface = support::hosted_casual(2, 60_000, 3);
    assert!(surface.begin_push().expect("a hosted match"), "the Push");
    let sites = lone_sites(&surface, walls, 100, 300, [-1000, -1000, 0], 0);
    lay(
        &mut surface,
        &sites,
        pharmakos_sim::voxels::Material::WALL_SEAT_1,
    );
    unlimited(&mut surface);
    surface
}

/// A FOGGED two-seat match in its first Push: `own` lone wall voxels of seat
/// 0 (its own, so listed whatever its sight: a list past one page), and
/// `theirs` lone wall voxels of seat 1 around seat 1's core, out of seat 0's
/// sight. Answers the surface and seat 1's walls' columns.
fn crowded_fogged(own: usize, theirs: usize) -> (Surface, Vec<[i32; 3]>) {
    use pharmakos_sim::voxels::Material;
    let mut surface = support::hosted(2, 60_000, 3);
    assert!(surface.begin_push().expect("a hosted match"), "the Push");
    let (_, core) = support::core_beacon(&surface, 1);
    let [cx, cy, _] = core;
    let mut near: Vec<[i32; 3]> = Vec::new();
    'columns: for dy in (-12..12).step_by(2) {
        for dx in (-12..12).step_by(2) {
            if near.len() == theirs {
                break 'columns;
            }
            if let Some(site) = ground(&surface, cx + dx + 1, cy + dy + 1) {
                near.push(site);
            }
        }
    }
    assert_eq!(near.len(), theirs, "ground around seat 1's core");
    let mine = lone_sites(&surface, own, 4, 380, core, 80);
    lay(&mut surface, &mine, Material::WALL_SEAT_0);
    lay(&mut surface, &near, Material::WALL_SEAT_1);
    unlimited(&mut surface);
    (surface, near)
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

/// Every page of one listing from `cursor` on, as (entities, owners of
/// walls), and the completing cursor.
fn listing_from(surface: &mut Surface, token: &Token, cursor: &str) -> (Vec<Json>, String) {
    let mut all: Vec<Json> = Vec::new();
    let mut at = cursor.to_owned();
    for _ in 0..64 {
        let (page, _) = view_page(surface, token, &at);
        all.extend(support::array_of(&page, "entities"));
        at = support::text_of(&page, "next_cursor");
        if matches!(page.get("complete"), Some(Json::Bool(true))) {
            return (all, at);
        }
    }
    panic!("a listing that never completes");
}

/// The first entity cursor of a keyframe listing, and what came before it.
fn to_first_entity_cursor(surface: &mut Surface, token: &Token) -> (Vec<Json>, String) {
    let mut seen: Vec<Json> = Vec::new();
    let mut at = String::new();
    loop {
        let (page, _) = view_page(surface, token, &at);
        assert!(
            matches!(page.get("complete"), Some(Json::Bool(false))),
            "a list that needs paging"
        );
        seen.extend(support::array_of(&page, "entities"));
        at = support::text_of(&page, "next_cursor");
        if at.len() > 64 {
            return (seen, at);
        }
    }
}

/// **Two readers of one viewer never cancel each other** (S3's `outcap`,
/// fix pass 1, review F1's probe P1): token A pages halfway through its
/// listing, token B of the same seat reads a whole listing of its own, and A's
/// next page goes on from where A was, every wall once.
#[test]
fn two_readers_of_one_seat_page_their_own_lists() {
    let mut surface = crowded(3_000);
    let first = seat_token(&mut surface, 0);
    let second = seat_token(&mut surface, 0);
    let (mut seen, cursor) = to_first_entity_cursor(&mut surface, &first);
    let (whole, _) = listing_from(&mut surface, &second, "");
    let (rest, _) = listing_from(&mut surface, &first, &cursor);
    seen.extend(rest);
    let walls = |list: &[Json]| {
        list.iter()
            .filter(|entity| entity.get("subtype") == Some(&Json::String(String::from("wall"))))
            .count()
    };
    assert_eq!(walls(&whole), 3_000);
    assert_eq!(walls(&seen), 3_000, "A's listing went on whole");
}

/// **An advisor reading between two of the client's pages leaves the
/// client's listing good** (fix pass 1, item C): the client pages to its
/// first entity cursor, the seat's advisor reads a whole listing of its own
/// through its scratch view, and the client's listing goes on whole.
#[test]
fn an_advisor_reading_between_the_clients_pages_cancels_nothing() {
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
    let client = seat_token(&mut surface, 0);
    let (mut seen, cursor) = to_first_entity_cursor(&mut surface, &client);
    let read = std::sync::Arc::new(std::sync::Mutex::new((0, 0, String::new())));
    let mut factory = Advising(Some(Box::new(Reader {
        seen: std::sync::Arc::clone(&read),
    })));
    let mut seats = InProcessSeats::open(
        &mut surface,
        &[SeatId::new(0), SeatId::new(1)],
        Some(0),
        &mut factory,
    )
    .expect("in-process seats");
    seats.plan(&mut surface);
    let (advised, _, refused) = read.lock().expect("not poisoned").clone();
    assert_eq!(
        (advised, refused.as_str()),
        (3_000, ""),
        "the advisor read whole"
    );
    let (rest, _) = listing_from(&mut surface, &client, &cursor);
    seen.extend(rest);
    let walls = seen
        .iter()
        .filter(|entity| entity.get("subtype") == Some(&Json::String(String::from("wall"))))
        .count();
    assert_eq!(walls, 3_000, "the client's listing went on whole");
}

/// **Fog holds on a paged list** (S3's `outcap`, fix pass 1, item D; review
/// F2): in a fogged match a seat whose own walls take more than a page is
/// shown none of the other seat's walls on any page, while a no-fog
/// spectator's list shows them: the held list is the viewer's own, taken
/// through the fog filter.
#[test]
fn a_paged_list_under_fog_shows_no_fogged_entity() {
    let (mut surface, theirs) = crowded_fogged(3_000, 25);
    let seat = seat_token(&mut surface, 0);
    let spectator = support::spectator_token(&mut surface, true);
    let place = |entity: &Json| -> Option<[i32; 3]> {
        let at = entity.get("at")?;
        let axis = |name: &str| match at.get(name) {
            Some(Json::Number(lexeme)) => lexeme.parse::<i32>().ok(),
            _ => None,
        };
        Some([axis("x")?, axis("y")?, axis("z")?])
    };
    let (seen, _) = listing_from(&mut surface, &seat, "");
    let owners: Vec<String> = seen
        .iter()
        .filter(|entity| entity.get("subtype") == Some(&Json::String(String::from("wall"))))
        .map(|entity| support::text_of(entity, "owner"))
        .collect();
    assert!(
        owners.len() > 2_000,
        "seat 0's own walls page: {}",
        owners.len()
    );
    assert!(
        owners.iter().all(|owner| owner == "seat.0"),
        "a fogged wall of seat 1 was listed for seat 0"
    );
    assert!(
        seen.iter()
            .filter_map(place)
            .all(|at| !theirs.contains(&at)),
        "a fogged wall's place was listed"
    );
    let (all, _) = listing_from(&mut surface, &spectator, "");
    let theirs_listed = all
        .iter()
        .filter(|entity| entity.get("subtype") == Some(&Json::String(String::from("wall"))))
        .filter(|entity| support::text_of(entity, "owner") == "seat.1")
        .count();
    assert_eq!(
        theirs_listed, 25,
        "the no-fog spectator sees seat 1's walls"
    );
    let ours_listed = all
        .iter()
        .filter(|entity| entity.get("subtype") == Some(&Json::String(String::from("wall"))))
        .filter(|entity| support::text_of(entity, "owner") == "seat.0")
        .count();
    assert_eq!(
        ours_listed,
        owners.len(),
        "every one of seat 0's walls, once"
    );
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
