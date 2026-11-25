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
    let at = |steps: usize| {
        Json::Object(vec![(
            String::from("playbook_jsonc"),
            Json::String(playbook(step, steps)),
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
            }
        }
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
