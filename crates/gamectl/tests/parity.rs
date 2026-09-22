// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! **Hash parity with the client path.**
//!
//! Spec §15 (Dev harness) asks the headless runner for exactly that phrase, and
//! the skeleton plan's T15 acceptance line spells it out: "the headless
//! runner's hash chain equals the client path's for the same seed". Nothing
//! else in this task is worth as much: a scenario's committed chain is a claim
//! about the *game*, and it is only that claim if the harness and a real client
//! produce the same one.
//!
//! # What "the client path" means, and it is not this file's invention
//!
//! Decisions-log item 106 (3) defines it: "a match hosted by the gateway's
//! `Host` behind a `Surface`, each seat's playbook arriving through
//! `submit_plan` over a live loopback WebSocket session and sealed by
//! `Surface::begin_push` (what `crates/gateway/tests/websocket.rs` and
//! `methods.rs`'s `two_seats_submit` do)". So that is what the second half of
//! this file builds, with the same `Wire` over a pair of byte buffers those
//! tests use — the RFC 6455 upgrade, the masked text frames, the JSON-RPC
//! request, the framing coming off in `session::serve`. No port is bound and no
//! thread is started, which is why this is an ordinary fast test.
//!
//! # Why the two chains *could* differ, which is why the test is worth writing
//!
//! `scenario run` seals in process and the client path seals over a socket, and
//! in between sit the handshake, the token, the rate limiter, the audit log and
//! the gateway's own tick — all of which touch gateway state on the way to the
//! same `Surface::begin_push`. If any of them reached the sim, the chains would
//! part. They must not, and this says so from outside rather than by reading
//! the code.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use pharmakos_gamectl::scenario::{self, Scenario, SeatKind};
use pharmakos_gateway::fog::{Blind, FogPolicy};
use pharmakos_gateway::frame::Opcode;
use pharmakos_gateway::handshake::Policy;
use pharmakos_gateway::host::Host;
use pharmakos_gateway::limit::Limits;
use pharmakos_gateway::scopes::{Scope, ScopeSet};
use pharmakos_gateway::session::{self, Ended};
use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::token::{Subject, Token};
use pharmakos_proto::json::{Json, read};
use pharmakos_sim::math::quantity::{Ms, Tick};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::runner::{DEFAULT_ROUND_LIMIT, MatchSettings};
use pharmakos_sim::tables::SeatId;
use pharmakos_sim::world::WorldConfig;
use std::fmt::Write as _;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// The scenario both halves play. Its seat 0 submits a playbook that
/// **qualifies**, which is what makes it usable here: a playbook the verifier
/// refuses could not go over the wire at all.
const SCENARIO: &str = "scenarios/skeleton/deploy-and-visit.scenario.jsonc";

/// The match id and port the client half uses. Neither reaches anything
/// hashed — the sim is a function of (map seed, playbooks, rules hash) — which
/// is itself part of what this test proves.
const MATCH: &str = "m-parity";
const PORT: u16 = 9501;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| panic!("crates/gamectl sits two levels below the workspace root"))
        .to_path_buf()
}

#[test]
fn the_headless_runners_chain_equals_the_client_paths_for_the_same_seed() {
    let scenario = scenario::load(&root(), Path::new(SCENARIO)).expect("a valid scenario");
    let harness = scenario::run::play(&root(), &scenario).expect("the headless run");
    assert!(
        harness.notes.is_empty(),
        "this scenario's playbook must go through `submit_plan` for the comparison to mean \
         anything:\n{:?}",
        harness.notes
    );

    let over_the_wire = client_path(&scenario);

    assert_eq!(
        harness.chain.lines().count(),
        over_the_wire.lines().count(),
        "the two paths played different numbers of ticks"
    );
    if harness.chain != over_the_wire {
        let at = harness
            .chain
            .lines()
            .zip(over_the_wire.lines())
            .position(|(left, right)| left != right)
            .unwrap_or(0);
        panic!(
            "the headless runner and the client path diverged at line {}:\n  headless   {}\n  \
             over the wire {}\n\nSomething between the socket and `Surface::begin_push` reached \
             the sim. Spec §15 asks this harness for hash parity with the client path, and a \
             committed scenario chain is only a claim about the game while they agree.",
            at.saturating_add(1),
            harness.chain.lines().nth(at).unwrap_or("<end>"),
            over_the_wire.lines().nth(at).unwrap_or("<end>"),
        );
    }
}

// ---------------------------------------------------------------------------
// The client path
// ---------------------------------------------------------------------------

/// The same match, driven the way a real client drives one: a live WebSocket
/// session per submission, then the host plays the Push.
///
/// Two connections' worth of work and one host loop, for the reason
/// `crates/gateway/tests/websocket.rs` gives in its own header: a live session
/// holds the surface, so the host cannot end the Lull while one is open. That
/// is not a limitation of the test — it is what "the Lull ends on the host's
/// word" looks like from the wire.
fn client_path(scenario: &Scenario) -> String {
    let rules = RulesTable::load(&root().join(&scenario.rules)).expect("the rules table");
    let seats: Vec<SeatId> = scenario
        .seats
        .iter()
        .map(|seat| SeatId::new(seat.seat))
        .collect();
    let host = Host::open(
        &WorldConfig {
            match_seed: scenario.seed,
            seats: u32::try_from(scenario.seats.len()).expect("at most three seats"),
            units_per_seat: 0,
            rules: rules.clone(),
            match_settings: MatchSettings {
                segment_lengths_ms: scenario
                    .segments
                    .iter()
                    .map(|segment| segment.length_ms)
                    .collect(),
                round_limit: DEFAULT_ROUND_LIMIT,
            },
        },
        None,
    )
    .expect("a match");

    // A different match id, a different fog policy and a socket in front of it.
    // All three are deliberate: if any of them reached the sim, this test would
    // be the thing that found out.
    let mut surface =
        Surface::new(MATCH, scenario.seed, rules, FogPolicy::casual(), &seats).expect("a match id");
    surface.attach(host).expect("attached");
    surface.set_phase_remaining_ms(Ms::new(lull_ms(&surface)));
    surface.open_lull().expect("the opening Lull");
    surface.set_limits(Limits {
        per_tick: 64,
        per_window: 600,
        window_ticks: 200,
    });

    for seat in &scenario.seats {
        let SeatKind::Playbook(relative) = &seat.kind else {
            // A `safe` seat submits nothing, which is the whole of what it is:
            // `Surface::begin_push` files the safe playbook for it.
            continue;
        };
        let token = mint(&mut surface, seat.seat);
        let playbook =
            std::fs::read_to_string(root().join(relative)).expect("the scenario's playbook");
        let params = pharmakos_proto::json::write(&Json::Object(vec![(
            String::from("playbook_jsonc"),
            Json::String(playbook),
        )]));

        let mut input = upgrade(&token.render());
        input.extend_from_slice(&call_frame(1, "submit_plan", &params));
        let (ended, wire) = serve(input, &mut surface);
        assert_eq!(ended, Ended::Disconnected, "the client stopped talking");
        let answers = responses(body_of(&wire));
        let result = answers
            .first()
            .and_then(|answer| answer.get("result"))
            .unwrap_or_else(|| panic!("submit_plan was refused over the wire: {answers:?}"))
            .clone();
        assert_eq!(
            result.get("accepted"),
            Some(&Json::Bool(true)),
            "seat {}'s playbook did not qualify over the wire",
            seat.seat
        );
    }

    let mut chain = String::new();
    for _ in &scenario.segments {
        assert!(surface.begin_push().expect("the Push begins"));
        while let Some(report) = surface.step().expect("a tick") {
            let _ = writeln!(
                chain,
                "{}\t{}",
                report.tick.raw(),
                pharmakos_sim::hex(report.hash)
            );
            if report.segment_ended {
                break;
            }
        }
        surface.end_recap().expect("the recap ends");
    }
    chain
}

fn lull_ms(surface: &Surface) -> i32 {
    surface
        .host()
        .expect("a hosted match")
        .rules()
        .message()
        .r#match
        .as_ref()
        .map_or(180_000, |block| block.lull_ms)
}

fn mint(surface: &mut Surface, seat: u8) -> Token {
    let (token, _) = surface
        .tokens()
        .mint(
            Subject::Seat(SeatId::new(seat)),
            MATCH,
            ScopeSet::of(&[Scope::Observe, Scope::Plan, Scope::PlanSubmit]),
            Tick::ZERO,
        )
        .expect("minted");
    token
}

// ---------------------------------------------------------------------------
// The wire, as `crates/gateway/tests/websocket.rs` writes it
// ---------------------------------------------------------------------------

/// A connection in a `Vec`: everything the client will ever say, and everything
/// the gateway said back. A read past the end is a closed socket, which is how
/// each of these sessions ends.
struct Wire {
    input: Vec<u8>,
    cursor: usize,
    output: Vec<u8>,
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

fn serve(input: Vec<u8>, surface: &mut Surface) -> (Ended, Wire) {
    let mut wire = Wire {
        input,
        cursor: 0,
        output: Vec::new(),
    };
    let ended = session::serve(&mut wire, surface, &Blind, &Policy::loopback(PORT));
    (ended, wire)
}

/// A conforming opening handshake carrying `token`. No `Origin`: a non-browser
/// client sends none, and the gateway accepts that (decisions-log item 100 (7)).
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

/// A masked client frame, as RFC 6455 §5.1 requires of every one.
fn client_frame(payload: &[u8]) -> Vec<u8> {
    let key = [0x37_u8, 0xfa, 0x21, 0x3d];
    let mut out: Vec<u8> = Vec::new();
    out.push(0x80 | Opcode::Text.bits());
    let length = payload.len();
    if length < 126 {
        out.push(0x80 | u8::try_from(length).expect("short"));
    } else if let Ok(wide) = u16::try_from(length) {
        out.push(0x80 | 0x7e);
        out.extend_from_slice(&wide.to_be_bytes());
    } else {
        out.push(0x80 | 0x7f);
        out.extend_from_slice(&u64::try_from(length).expect("a length").to_be_bytes());
    }
    out.extend_from_slice(&key);
    for (index, byte) in payload.iter().enumerate() {
        out.push(byte ^ key.get(index % 4).copied().unwrap_or(0));
    }
    out
}

fn call_frame(id: u32, method: &str, params: &str) -> Vec<u8> {
    let text = format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{params}}}"#);
    client_frame(text.as_bytes())
}

/// Everything after the handshake's blank line.
fn body_of(wire: &Wire) -> &[u8] {
    let body_at = wire
        .output
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("the handshake ends")
        .saturating_add(4);
    wire.output.get(body_at..).unwrap_or_default()
}

/// Every text frame the gateway sent, parsed back as JSON.
fn responses(output: &[u8]) -> Vec<Json> {
    let mut out: Vec<Json> = Vec::new();
    let mut rest = output;
    while let Some((opcode, payload, used)) = server_frame(rest) {
        if opcode == Opcode::Text {
            let text = String::from_utf8(payload).expect("UTF-8");
            out.push(read(&text).expect("JSON"));
        }
        rest = rest.get(used..).unwrap_or_default();
    }
    out
}

/// Decode one **server** frame: the same wire format, unmasked. A reader of its
/// own, because `frame::decode` refuses an unmasked frame — which is the point
/// of it.
fn server_frame(bytes: &[u8]) -> Option<(Opcode, Vec<u8>, usize)> {
    let first = bytes.first().copied()?;
    let second = bytes.get(1).copied()?;
    assert_eq!(second & 0x80, 0, "a server frame is never masked");
    let opcode = match first & 0x0f {
        0x1 => Opcode::Text,
        0x8 => Opcode::Close,
        0x9 => Opcode::Ping,
        0xA => Opcode::Pong,
        _ => return None,
    };
    let (length, start): (usize, usize) = match second & 0x7f {
        126 => {
            let raw = bytes.get(2..4)?;
            let mut wide = [0_u8; 2];
            wide.copy_from_slice(raw);
            (usize::from(u16::from_be_bytes(wide)), 4)
        }
        127 => {
            let raw = bytes.get(2..10)?;
            let mut wide = [0_u8; 8];
            wide.copy_from_slice(raw);
            (usize::try_from(u64::from_be_bytes(wide)).ok()?, 10)
        }
        short => (usize::from(short), 2),
    };
    let payload = bytes.get(start..start.saturating_add(length))?.to_vec();
    Some((opcode, payload, start.saturating_add(length)))
}
