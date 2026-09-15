// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The transport end to end, over a pair of byte buffers instead of a socket.
//!
//! Everything a real client does: the RFC 6455 upgrade with a `Host`, an
//! `Origin`-free native request and an `Authorization: Bearer` token; masked
//! text frames carrying JSON-RPC 2.0; a ping; a close. And everything a
//! misbehaving one does: an unmasked frame, a binary frame, a foreign `Origin`,
//! a token from nowhere.
//!
//! No port is bound and no thread is started, which is why these are ordinary
//! fast tests rather than something with a timeout in it. `tests/security.rs`
//! covers the socket itself.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use pharmakos_gateway::fog::{Blind, FogPolicy};
use pharmakos_gateway::frame::Opcode;
use pharmakos_gateway::handshake::{Policy, Refusal};
use pharmakos_gateway::scopes::{Scope, ScopeSet};
use pharmakos_gateway::session::{self, Ended};
use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::time::MatchTime;
use pharmakos_gateway::token::{Subject, Token};
use pharmakos_proto::gp::api::v1::status::Phase;
use pharmakos_proto::json::{Json, read};
use pharmakos_sim::math::quantity::{Ms, Tick};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::tables::SeatId;
use std::io::{self, Read, Write};
use std::path::Path;

const MATCH: &str = "m-0001";
const PORT: u16 = 9500;

/// A connection in a `Vec`: everything the client will ever say, and everything
/// the gateway said back. A read past the end is a closed socket, which is how
/// each of these sessions ends.
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
        let remaining = self.input.len().saturating_sub(self.cursor);
        let take = remaining.min(buffer.len());
        if take == 0 {
            return Ok(0);
        }
        let from = self.cursor;
        let slice = self
            .input
            .get(from..from.saturating_add(take))
            .unwrap_or_default();
        buffer
            .get_mut(..take)
            .unwrap_or_default()
            .copy_from_slice(slice);
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

fn rules() -> RulesTable {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("rules")
        .join("rules.v1.json");
    RulesTable::load(&path).expect("the shipped rules table")
}

fn surface() -> Surface {
    let mut surface = Surface::new(
        MATCH,
        0x00ca_5cad_ed00_0001,
        rules(),
        FogPolicy::fogged(),
        &[SeatId::new(0), SeatId::new(1)],
    )
    .expect("a match id");
    surface.set_time(MatchTime {
        tick: Tick::new(10),
        phase: Phase::Lull,
        phase_remaining_ms: Ms::new(174_000),
        segment_length_ms: Ms::new(180_000),
        round: 1,
    });
    surface
}

fn seat_token(surface: &mut Surface) -> Token {
    let (token, _) = surface
        .tokens()
        .mint(
            Subject::Seat(SeatId::new(0)),
            MATCH,
            ScopeSet::of(&[Scope::Observe, Scope::Plan, Scope::PlanSubmit, Scope::Docs]),
            Tick::ZERO,
        )
        .expect("minted");
    token
}

/// A conforming opening handshake carrying `token`.
fn upgrade(token: &str, origin: Option<&str>) -> Vec<u8> {
    let origin_line = origin.map_or_else(String::new, |value| format!("Origin: {value}\r\n"));
    format!(
        "GET /seat HTTP/1.1\r\n\
         Host: 127.0.0.1:{PORT}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         Sec-WebSocket-Version: 13\r\n\
         Authorization: Bearer {token}\r\n\
         {origin_line}\
         \r\n"
    )
    .into_bytes()
}

/// A masked client frame, as RFC 6455 section 5.1 requires of every one.
fn client_frame(fin: bool, opcode: Opcode, payload: &[u8]) -> Vec<u8> {
    let key = [0x37_u8, 0xfa, 0x21, 0x3d];
    let mut out: Vec<u8> = Vec::new();
    out.push(if fin { 0x80 } else { 0x00 } | opcode.bits());
    let length = payload.len();
    if length < 126 {
        out.push(0x80 | u8::try_from(length).expect("short"));
    } else {
        out.push(0x80 | 0x7e);
        out.extend_from_slice(&u16::try_from(length).expect("fits").to_be_bytes());
    }
    out.extend_from_slice(&key);
    for (index, byte) in payload.iter().enumerate() {
        out.push(byte ^ key.get(index % 4).copied().unwrap_or(0));
    }
    out
}

fn call_frame(id: u32, method: &str, params: &str) -> Vec<u8> {
    let text = format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{params}}}"#);
    client_frame(true, Opcode::Text, text.as_bytes())
}

/// Every text frame the gateway sent, parsed back as JSON.
fn responses(output: &[u8]) -> Vec<Json> {
    let mut out: Vec<Json> = Vec::new();
    let mut rest = output;
    while let Ok(Some((decoded, used))) = server_frame(rest) {
        if decoded.0 == Opcode::Text {
            let text = String::from_utf8(decoded.1).expect("UTF-8");
            out.push(read(&text).expect("JSON"));
        }
        rest = rest.get(used..).unwrap_or_default();
    }
    out
}

/// Decode one **server** frame: the same wire format, unmasked. A tiny reader of
/// its own, because `frame::decode` refuses an unmasked frame -- which is the
/// point of it.
#[allow(clippy::type_complexity)]
fn server_frame(bytes: &[u8]) -> Result<Option<((Opcode, Vec<u8>), usize)>, ()> {
    let Some(first) = bytes.first().copied() else {
        return Ok(None);
    };
    let Some(second) = bytes.get(1).copied() else {
        return Ok(None);
    };
    assert_eq!(second & 0x80, 0, "a server frame is never masked");
    let opcode = match first & 0x0f {
        0x1 => Opcode::Text,
        0x8 => Opcode::Close,
        0x9 => Opcode::Ping,
        0xA => Opcode::Pong,
        _ => return Err(()),
    };
    let (length, start): (usize, usize) = match second & 0x7f {
        126 => {
            let raw = bytes.get(2..4).ok_or(())?;
            let mut wide = [0_u8; 2];
            wide.copy_from_slice(raw);
            (usize::from(u16::from_be_bytes(wide)), 4)
        }
        127 => return Err(()),
        short => (usize::from(short), 2),
    };
    let payload = bytes
        .get(start..start.saturating_add(length))
        .ok_or(())?
        .to_vec();
    Ok(Some(((opcode, payload), start.saturating_add(length))))
}

fn run(input: Vec<u8>, surface: &mut Surface) -> (Ended, Wire) {
    let mut wire = Wire::new(input);
    let ended = session::serve(&mut wire, surface, &Blind, &Policy::loopback(PORT));
    (ended, wire)
}

#[test]
fn a_conforming_client_upgrades_and_calls() {
    let mut surface = surface();
    let token = seat_token(&mut surface);
    let mut input = upgrade(&token.render(), None);
    input.extend_from_slice(&call_frame(1, "get_status", "{}"));
    input.extend_from_slice(&call_frame(2, "save_notes", r#"{"notes":"walk east"}"#));

    let (ended, wire) = run(input, &mut surface);
    assert_eq!(ended, Ended::Disconnected, "the client stopped talking");

    let text = String::from_utf8_lossy(&wire.output).into_owned();
    assert!(
        text.starts_with("HTTP/1.1 101 Switching Protocols\r\n"),
        "{}",
        text.get(..80).unwrap_or_default()
    );
    assert!(text.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n"));

    let body_at = wire
        .output
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("the handshake ends")
        .saturating_add(4);
    let answers = responses(wire.output.get(body_at..).unwrap_or_default());
    assert_eq!(answers.len(), 2, "one answer per call");

    let status = answers.first().expect("the first answer");
    assert_eq!(status.get("id"), Some(&Json::Number(String::from("1"))));
    let footer = status
        .get("result")
        .and_then(|result| result.get("_status"))
        .expect("every result carries the footer");
    assert_eq!(
        footer.get("phase"),
        Some(&Json::String(String::from("lull")))
    );

    let notes = answers.get(1).expect("the second answer");
    assert_eq!(
        notes
            .get("result")
            .and_then(|result| result.get("characters")),
        Some(&Json::Number(String::from("9")))
    );
    assert_eq!(
        surface
            .seat_state(Subject::Seat(SeatId::new(0)), SeatId::new(0))
            .expect("its own")
            .notebook,
        "walk east"
    );
}

#[test]
fn a_foreign_origin_never_reaches_the_websocket() {
    let mut surface = surface();
    let token = seat_token(&mut surface);
    let (ended, wire) = run(
        upgrade(&token.render(), Some("http://evil.example")),
        &mut surface,
    );
    assert!(
        matches!(ended, Ended::UpgradeRefused(Refusal::ForeignOrigin(_))),
        "{ended:?}"
    );
    let text = String::from_utf8_lossy(&wire.output).into_owned();
    assert!(text.starts_with("HTTP/1.1 403 Forbidden\r\n"), "{text}");
    assert!(text.contains("Connection: close\r\n"), "{text}");
    let last = surface.audit().entries().last().cloned().expect("logged");
    assert_eq!(last.action, "upgrade");
    assert_eq!(last.outcome.render(), "FORBIDDEN_SCOPE");
}

#[test]
fn a_token_from_nowhere_never_reaches_the_websocket() {
    let mut surface = surface();
    let unknown = "ab".repeat(32);
    let (ended, wire) = run(upgrade(&unknown, None), &mut surface);
    assert_eq!(ended, Ended::UpgradeRefused(Refusal::Unauthenticated));
    let text = String::from_utf8_lossy(&wire.output).into_owned();
    assert!(text.starts_with("HTTP/1.1 401 Unauthorized\r\n"), "{text}");
    let last = surface.audit().entries().last().cloned().expect("logged");
    assert_eq!(last.outcome.render(), "UNAUTHENTICATED");
    assert_eq!(last.subject, None, "nobody to name");
}

#[test]
fn a_handshake_with_no_token_is_refused_before_a_frame_is_read() {
    let mut surface = surface();
    let request = format!(
        "GET /seat HTTP/1.1\r\n\
         Host: 127.0.0.1:{PORT}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         Sec-WebSocket-Version: 13\r\n\
         \r\n"
    );
    let (ended, wire) = run(request.into_bytes(), &mut surface);
    assert_eq!(ended, Ended::UpgradeRefused(Refusal::MissingAuthorization));
    assert!(String::from_utf8_lossy(&wire.output).starts_with("HTTP/1.1 401 Unauthorized\r\n"));
}

#[test]
fn an_unmasked_frame_closes_the_connection() {
    let mut surface = surface();
    let token = seat_token(&mut surface);
    let mut input = upgrade(&token.render(), None);
    // The same call, unmasked and in clear: a protocol error, not a lenient
    // read (RFC 6455 section 5.1).
    input.extend_from_slice(&[0x81, 0x02, b'{', b'}']);

    let (ended, _) = run(input, &mut surface);
    assert_eq!(ended, Ended::GatewayClosed(1002));
}

#[test]
fn a_binary_frame_closes_the_connection() {
    let mut surface = surface();
    let token = seat_token(&mut surface);
    let mut input = upgrade(&token.render(), None);
    let mut binary = client_frame(true, Opcode::Text, b"{}");
    if let Some(first) = binary.first_mut() {
        *first = 0x82;
    }
    input.extend_from_slice(&binary);

    let (ended, _) = run(input, &mut surface);
    assert_eq!(ended, Ended::GatewayClosed(1002));
}

#[test]
fn a_ping_is_ponged_and_a_close_is_echoed() {
    let mut surface = surface();
    let token = seat_token(&mut surface);
    let mut input = upgrade(&token.render(), None);
    input.extend_from_slice(&client_frame(true, Opcode::Ping, b"are you there"));
    input.extend_from_slice(&client_frame(true, Opcode::Close, &[0x03, 0xe8]));

    let (ended, wire) = run(input, &mut surface);
    assert_eq!(ended, Ended::PeerClosed(1000));

    let body_at = wire
        .output
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("the handshake ends")
        .saturating_add(4);
    let mut rest = wire.output.get(body_at..).unwrap_or_default();
    let ((opcode, payload), used) = server_frame(rest).expect("a frame").expect("a whole one");
    assert_eq!(opcode, Opcode::Pong);
    assert_eq!(payload, b"are you there");
    rest = rest.get(used..).unwrap_or_default();
    let ((opcode, _), _) = server_frame(rest).expect("a frame").expect("a whole one");
    assert_eq!(opcode, Opcode::Close);
}

#[test]
fn a_fragmented_call_is_joined_and_answered() {
    let mut surface = surface();
    let token = seat_token(&mut surface);
    let whole = r#"{"jsonrpc":"2.0","id":9,"method":"get_status","params":{}}"#;
    // Halfway, rounding down. `checked_div` because `clippy::integer_division`
    // is denied: rounding is a decision everywhere in this project, test or not.
    let split = whole.len().checked_div(2).unwrap_or(0);
    let mut input = upgrade(&token.render(), None);
    input.extend_from_slice(&client_frame(
        false,
        Opcode::Text,
        whole.get(..split).expect("first half").as_bytes(),
    ));
    input.extend_from_slice(&client_frame(
        true,
        Opcode::Continuation,
        whole.get(split..).expect("second half").as_bytes(),
    ));

    let (_, wire) = run(input, &mut surface);
    let body_at = wire
        .output
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("the handshake ends")
        .saturating_add(4);
    let answers = responses(wire.output.get(body_at..).unwrap_or_default());
    assert_eq!(answers.len(), 1);
    assert_eq!(
        answers.first().and_then(|answer| answer.get("id")),
        Some(&Json::Number(String::from("9")))
    );
}

#[test]
fn rubbish_inside_a_text_frame_is_a_json_rpc_parse_error_and_not_a_close() {
    let mut surface = surface();
    let token = seat_token(&mut surface);
    let mut input = upgrade(&token.render(), None);
    input.extend_from_slice(&client_frame(true, Opcode::Text, b"{not json"));
    input.extend_from_slice(&call_frame(2, "get_status", "{}"));

    let (ended, wire) = run(input, &mut surface);
    assert_eq!(
        ended,
        Ended::Disconnected,
        "a bad message is not a bad socket"
    );
    let body_at = wire
        .output
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("the handshake ends")
        .saturating_add(4);
    let answers = responses(wire.output.get(body_at..).unwrap_or_default());
    assert_eq!(answers.len(), 2);
    let first = answers.first().expect("the parse error");
    assert_eq!(first.get("id"), Some(&Json::Null));
    assert_eq!(
        first.get("error").and_then(|error| error.get("code")),
        Some(&Json::Number(String::from("-32700")))
    );
    assert!(
        answers
            .get(1)
            .and_then(|answer| answer.get("result"))
            .is_some(),
        "the connection carried on"
    );
}

#[test]
fn the_upgrade_offers_no_extension_and_no_subprotocol() {
    let mut surface = surface();
    let token = seat_token(&mut surface);
    let (_, wire) = run(upgrade(&token.render(), None), &mut surface);
    let text = String::from_utf8_lossy(&wire.output).into_owned();
    assert!(!text.contains("Sec-WebSocket-Extensions"), "{text}");
    assert!(!text.contains("Sec-WebSocket-Protocol"), "{text}");
}

/// The frame cap, over the real session loop rather than the assembler alone.
#[test]
fn a_frame_over_the_cap_closes_the_connection() {
    let mut surface = surface();
    let token = seat_token(&mut surface);
    let mut input = upgrade(&token.render(), None);
    // A 64-bit length claiming more than the cap. Nothing follows it: the
    // gateway must refuse on the header rather than wait for the bytes.
    input.extend_from_slice(&[0x81, 0xff]);
    input.extend_from_slice(&u64::MAX.to_be_bytes());

    let (ended, _) = run(input, &mut surface);
    assert_eq!(ended, Ended::GatewayClosed(1009));
}
