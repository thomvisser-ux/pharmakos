// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The host loop, over a real loopback socket.
//!
//! `tests/websocket.rs` drives the transport over a pair of byte buffers,
//! which is the right shape for the protocol's negative cases and cannot say
//! anything about **several connections sharing one match** -- the thing
//! `serve.rs` exists for. So this suite binds a port, spawns the host, and
//! talks to it the way the Godot lobby will: two connections, one config line
//! in, one announce line out, and end of file as the whole of the shutdown
//! protocol.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

mod support;

use pharmakos_gateway::advice::Advice;
use pharmakos_gateway::frame::Opcode;
use pharmakos_gateway::rpc::Request;
use pharmakos_gateway::serve::{
    self, Advisor, Announce, BuiltInSeat, Config, MAX_CONNECTIONS, NoOperators, Operators, Setup,
};
use pharmakos_proto::json::{Json, read};
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use support::{SEED, rules_json};

// ---------------------------------------------------------------------------
// A control pipe and an announce sink, in process
// ---------------------------------------------------------------------------

/// The child's standard input, as a channel.
///
/// A read past the last message is end of file, which is exactly what the
/// parent dropping the pipe means -- and is the whole of how a host is told to
/// stop.
struct ControlPipe {
    messages: Receiver<Vec<u8>>,
    held: Vec<u8>,
    at: usize,
}

impl Read for ControlPipe {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while self.at >= self.held.len() {
            match self.messages.recv() {
                Ok(next) => {
                    self.held = next;
                    self.at = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let remaining = self.held.len().saturating_sub(self.at);
        let take = remaining.min(out.len());
        if take == 0 {
            return Ok(0);
        }
        let slice = self
            .held
            .get(self.at..self.at.saturating_add(take))
            .unwrap_or_default();
        out.get_mut(..take)
            .unwrap_or_default()
            .copy_from_slice(slice);
        self.at = self.at.saturating_add(take);
        Ok(take)
    }
}

/// The child's standard output, as a shared buffer.
#[derive(Clone)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if let Ok(mut held) = self.0.lock() {
            held.extend_from_slice(bytes);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A running host, and the two ends the parent holds.
struct Running {
    announce: Announce,
    written: Arc<Mutex<Vec<u8>>>,
    control: Option<Sender<Vec<u8>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Running {
    /// Close the control pipe and wait for the host to return.
    fn quit(&mut self) {
        drop(self.control.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.quit();
    }
}

/// Start a host on an ephemeral loopback port and read its announce line.
fn start(config: &Config, operators: Box<dyn Operators>) -> Running {
    let (sender, messages) = channel::<Vec<u8>>();
    sender
        .send(config.render().into_bytes())
        .expect("the config line");
    let written = Arc::new(Mutex::new(Vec::new()));
    let sink = Sink(Arc::clone(&written));
    let setup = Setup {
        rules_json: rules_json(),
        library: Some(support::library_folder()),
        operators,
    };
    let thread = std::thread::spawn(move || {
        let pipe = ControlPipe {
            messages,
            held: Vec::new(),
            at: 0,
        };
        if let Err(error) = serve::run(setup, pipe, sink) {
            // The parent is blocked on the announce line, so a host that
            // refused to start has to say why here or the failure reads as a
            // timeout.
            panic!("the host would not start: {error}");
        }
    });

    // The announce line is the host saying it is listening. Nothing else in
    // this file may run before it arrives.
    for _ in 0..6_000 {
        let line = written.lock().ok().and_then(|held| {
            let index = held.iter().position(|byte| *byte == b'\n')?;
            String::from_utf8(held.get(..index).unwrap_or_default().to_vec()).ok()
        });
        if let Some(line) = line {
            return Running {
                announce: Announce::parse(&line).expect("an announce line"),
                written,
                control: Some(sender),
                thread: Some(thread),
            };
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the host never announced a port (waited a minute)");
}

/// The config line for a two-seat match.
fn config(match_id: &str, human: Option<u8>) -> Config {
    Config {
        match_id: match_id.to_owned(),
        seed: SEED,
        seats: 2,
        human_seat: human,
        segment_lengths_ms: vec![1_000],
        round_limit: 2,
    }
}

// ---------------------------------------------------------------------------
// A client
// ---------------------------------------------------------------------------

/// One connection to the host, as the lobby and the camera each hold one.
struct Client {
    stream: TcpStream,
    buffer: Vec<u8>,
    id: u32,
}

impl Client {
    /// Connect, upgrade, and be ready to call.
    fn open(port: u16, token: &str) -> Client {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("a loopback connection");
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .expect("a read timeout");
        let upgrade = format!(
            "GET /seat HTTP/1.1\r\n\
             Host: 127.0.0.1:{port}\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
             Sec-WebSocket-Version: 13\r\n\
             Authorization: Bearer {token}\r\n\
             \r\n"
        );
        stream.write_all(upgrade.as_bytes()).expect("the upgrade");
        let mut client = Client {
            stream,
            buffer: Vec::new(),
            id: 0,
        };
        let head = client.read_until(b"\r\n\r\n");
        assert!(
            head.starts_with("HTTP/1.1 101"),
            "the upgrade was refused: {head}"
        );
        client
    }

    /// One JSON-RPC call, and the one answer it waits for.
    fn call(&mut self, method: &str, params: &str) -> Json {
        self.id = self.id.saturating_add(1);
        let text = format!(
            r#"{{"jsonrpc":"2.0","id":{},"method":"{method}","params":{params}}}"#,
            self.id
        );
        let frame = masked(Opcode::Text, text.as_bytes());
        self.stream.write_all(&frame).expect("a call");
        let answer = self.read_frame();
        read(&answer).expect("JSON")
    }

    /// Read until the buffer holds `needle`, and take everything up to and
    /// including it.
    fn read_until(&mut self, needle: &[u8]) -> String {
        loop {
            if let Some(index) = self
                .buffer
                .windows(needle.len())
                .position(|window| window == needle)
            {
                let end = index.saturating_add(needle.len());
                let head = self.buffer.get(..end).unwrap_or_default().to_vec();
                self.buffer.drain(..end);
                return String::from_utf8(head).expect("UTF-8");
            }
            self.fill();
        }
    }

    /// Read one whole server frame's text payload.
    fn read_frame(&mut self) -> String {
        loop {
            if let Some((payload, used)) = server_frame(&self.buffer) {
                self.buffer.drain(..used);
                return String::from_utf8(payload).expect("UTF-8");
            }
            self.fill();
        }
    }

    fn fill(&mut self) {
        let mut chunk = [0_u8; 8192];
        let read = self.stream.read(&mut chunk).expect("a readable socket");
        assert!(read > 0, "the host closed the connection");
        self.buffer
            .extend_from_slice(chunk.get(..read).unwrap_or_default());
    }
}

/// A masked client frame, as RFC 6455 section 5.1 requires of every one.
fn masked(opcode: Opcode, payload: &[u8]) -> Vec<u8> {
    let key = [0x37_u8, 0xfa, 0x21, 0x3d];
    let mut out: Vec<u8> = Vec::new();
    out.push(0x80 | opcode.bits());
    let length = payload.len();
    if length < 126 {
        out.push(0x80 | u8::try_from(length).expect("short"));
    } else if u16::try_from(length).is_ok() {
        out.push(0x80 | 0x7e);
        out.extend_from_slice(&u16::try_from(length).expect("fits").to_be_bytes());
    } else {
        out.push(0x80 | 0x7f);
        out.extend_from_slice(&u64::try_from(length).expect("fits").to_be_bytes());
    }
    out.extend_from_slice(&key);
    for (index, byte) in payload.iter().enumerate() {
        out.push(
            byte ^ key
                .get(index.checked_rem(4).unwrap_or(0))
                .copied()
                .unwrap_or(0),
        );
    }
    out
}

/// One **server** frame: the same wire format, unmasked.
fn server_frame(bytes: &[u8]) -> Option<(Vec<u8>, usize)> {
    let second = usize::from(*bytes.get(1)?);
    let (length, header) = match second {
        0..=125 => (second, 2_usize),
        126 => {
            let high = usize::from(*bytes.get(2)?);
            let low = usize::from(*bytes.get(3)?);
            ((high << 8) | low, 4)
        }
        _ => {
            let mut value = 0_usize;
            for offset in 2..10_usize {
                value = (value << 8) | usize::from(*bytes.get(offset)?);
            }
            (value, 10)
        }
    };
    let end = header.checked_add(length)?;
    let payload = bytes.get(header..end)?.to_vec();
    Some((payload, end))
}

/// The `result` member, or a panic naming the refusal.
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

// ---------------------------------------------------------------------------
// The two lines
// ---------------------------------------------------------------------------

/// The config and announce lines round-trip, and a line this host did not
/// write is refused rather than guessed at.
#[test]
fn the_two_lines_are_read_exactly_as_they_were_written() {
    let written = config("m-t16a-lines", Some(1));
    let parsed = Config::parse(&written.render()).expect("its own line");
    assert_eq!(parsed, written);

    let untimed = Config {
        human_seat: None,
        segment_lengths_ms: Vec::new(),
        ..written
    };
    assert_eq!(
        Config::parse(&untimed.render()).expect("its own line"),
        untimed,
        "no human seat, and the rules table's own segment ladder"
    );

    for wrong in [
        "",
        "m-1",
        "m-1\t0x1\t2\t-\t1000",
        "m-1\tnotaseed\t2\t-\t1000\t2",
        "m-1\t0x1\ttwo\t-\t1000\t2",
        "m-1\t0x1\t2\t9\t1000\t2",
        "m-1\t0x1\t2\t-\tlong\t2",
    ] {
        assert!(
            Config::parse(wrong).is_err(),
            "`{wrong}` is not a config line and was read as one"
        );
    }

    let announce = Announce {
        port: 49_152,
        match_id: String::from("m-t16a-lines"),
        admin_token: "a".repeat(64),
        seat_token: Some("b".repeat(64)),
    };
    assert_eq!(
        Announce::parse(&announce.render()).expect("its own line"),
        announce
    );
    assert!(Announce::parse("not a line").is_err());
}

// ---------------------------------------------------------------------------
// The host
// ---------------------------------------------------------------------------

/// The host binds loopback on a port the operating system chose, and says
/// which.
#[test]
fn the_host_binds_loopback_on_an_ephemeral_port() {
    let mut running = start(&config("m-t16a-port", Some(0)), Box::new(NoOperators));
    assert_ne!(running.announce.port, 0, "an ephemeral port is a real port");
    assert_eq!(running.announce.match_id, "m-t16a-port");
    assert_eq!(running.announce.admin_token.len(), 64);
    assert_eq!(
        running.announce.seat_token.as_ref().map(String::len),
        Some(64),
        "the human seat's token"
    );

    // Loopback answers, and the announce line is what a parent needs to reach
    // it: there is no other way in.
    let mut client = Client::open(running.announce.port, &running.announce.admin_token);
    let status = result(&client.call("get_status", "{}"), "get_status");
    assert_eq!(
        status.get("status").and_then(|footer| footer.get("phase")),
        Some(&Json::String(String::from("lull"))),
        "a host opens in its first Lull"
    );
    running.quit();
}

/// Two connections, one match.
#[test]
fn two_connections_share_one_surface() {
    let mut running = start(&config("m-t16a-two", Some(0)), Box::new(NoOperators));
    let port = running.announce.port;
    let mut lobby = Client::open(port, &running.announce.admin_token);
    let mut camera = Client::open(
        port,
        running
            .announce
            .seat_token
            .as_deref()
            .expect("a human seat"),
    );

    // The camera reads the match the lobby is about to move.
    let before = result(&camera.call("get_status", "{}"), "get_status");
    assert_eq!(
        before.get("status").and_then(|f| f.get("phase")),
        Some(&Json::String(String::from("lull")))
    );

    let ended = result(&lobby.call("end_lull", "{}"), "end_lull");
    assert_eq!(
        ended.get("_status").and_then(|f| f.get("phase")),
        Some(&Json::String(String::from("push")))
    );

    // The same match, seen from the other connection.
    let after = result(&camera.call("get_status", "{}"), "get_status");
    assert_eq!(
        after.get("status").and_then(|f| f.get("phase")),
        Some(&Json::String(String::from("push"))),
        "two connections and one Surface: what the lobby did, the camera sees"
    );

    // And the seat's own view comes back over its own connection.
    let view = result(&camera.call("get_view", r#"{"cursor":""}"#), "get_view");
    assert!(
        matches!(view.get("chunks"), Some(Json::Array(chunks)) if !chunks.is_empty()),
        "the camera got terrain"
    );
    running.quit();
}

/// One JSON-RPC request, as a scripted seat builds it.
fn ask(method: &str, params: &str) -> Request {
    pharmakos_gateway::rpc::parse(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"{method}","params":{params}}}"#
    ))
    .expect("well formed")
}

/// A scripted stand-in for **T18**'s operator: it reads its own status and
/// says it is ready, through the closure it is handed.
struct Scripted {
    seen: Arc<Mutex<Vec<String>>>,
}

impl BuiltInSeat for Scripted {
    fn plan(&mut self, seat: u8, call: &mut dyn FnMut(&Request) -> Json) {
        let status = call(&ask("get_status", "{}"));
        let ready = call(&ask("set_ready", r#"{"ready":true}"#));
        if let Ok(mut seen) = self.seen.lock() {
            seen.push(format!(
                "seat {seat}: status={}, ready={}",
                status.get("result").is_some(),
                ready.get("result").is_some()
            ));
        }
    }
}

/// A scripted advisor: it reads its own seat's beacons and advises the
/// gateway's own fallback, so its advice qualifies.
struct Advising {
    seen: Arc<Mutex<Vec<String>>>,
}

impl Advisor for Advising {
    fn advise(&mut self, seat: u8, call: &mut dyn FnMut(&Request) -> Json) -> Advice {
        let beacons = call(&ask("list_beacons", "{}"));
        if let Ok(mut seen) = self.seen.lock() {
            seen.push(format!(
                "advise seat {seat}: beacons={}",
                beacons.get("result").is_some()
            ));
        }
        Advice {
            safe_playbook_jsonc: String::from(pharmakos_gateway::host::SAFE_PLAYBOOK),
            suggestions: Vec::new(),
        }
    }
}

/// A factory that plays the seats it is told to, advises every other seat,
/// and writes down what it was asked, in order.
struct Factory {
    plays: Vec<u8>,
    asked: Arc<Mutex<Vec<String>>>,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Factory {
    fn boxed(
        plays: &[u8],
        asked: &Arc<Mutex<Vec<String>>>,
        seen: &Arc<Mutex<Vec<String>>>,
    ) -> Box<dyn Operators> {
        Box::new(Factory {
            plays: plays.to_vec(),
            asked: Arc::clone(asked),
            seen: Arc::clone(seen),
        })
    }
}

impl Operators for Factory {
    fn built_in(&mut self, seat: u8) -> Option<Box<dyn BuiltInSeat>> {
        if let Ok(mut asked) = self.asked.lock() {
            asked.push(format!("built_in {seat}"));
        }
        if !self.plays.contains(&seat) {
            return None;
        }
        let played: Box<dyn BuiltInSeat> = Box::new(Scripted {
            seen: Arc::clone(&self.seen),
        });
        Some(played)
    }

    fn advisor(&mut self, seat: u8) -> Option<Box<dyn Advisor>> {
        if let Ok(mut asked) = self.asked.lock() {
            asked.push(format!("advisor {seat}"));
        }
        Some(Box::new(Advising {
            seen: Arc::clone(&self.seen),
        }))
    }
}

/// A built-in seat comes through the same door a socket does.
#[test]
fn a_built_in_seat_calls_through_the_same_door_as_a_socket() {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::new(Mutex::new(Vec::new()));
    // No human seat: both seats are this process's own.
    let mut running = start(
        &config("m-t16a-builtin", None),
        Factory::boxed(&[0, 1], &asked, &seen),
    );
    assert_eq!(running.announce.seat_token, None, "no human, no seat token");

    let mut lobby = Client::open(running.announce.port, &running.announce.admin_token);
    let answer = result(
        &lobby.call(
            "report_host_clock",
            r#"{"elapsed_ms":1000,"remaining_ms":179000}"#,
        ),
        "report_host_clock",
    );
    assert_eq!(
        answer.get("all_ready"),
        Some(&Json::Bool(true)),
        "both built-in seats planned their round and said so, through Surface::call"
    );

    let lines = seen.lock().expect("the script's own record").clone();
    assert_eq!(lines.len(), 2, "one plan per seat: {lines:?}");
    for line in &lines {
        assert!(line.contains("status=true"), "{line}");
        assert!(line.contains("ready=true"), "{line}");
    }
    assert!(
        lines.iter().any(|line| line.starts_with("seat 0:")),
        "each seat is told which it is: {lines:?}"
    );
    running.quit();
}

/// Decisions-log item 111, H11: the factory is asked once the config line has
/// been read, once per seat, and the answer is one operator per seat it plays
/// and one advisor per other seat -- the human's included, and never a
/// built-in operator for the human.
#[test]
fn the_host_builds_one_operator_per_built_in_seat_and_one_advisor_per_other_seat() {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let three = Config {
        seats: 3,
        ..config("m-t18a-factory", Some(0))
    };
    // The factory plays seat 2 only: seat 1 is nobody's, so it is advised
    // like the human's.
    let mut running = start(&three, Factory::boxed(&[2], &asked, &seen));
    let mut lobby = Client::open(running.announce.port, &running.announce.admin_token);
    let _ = result(&lobby.call("get_status", "{}"), "get_status");
    running.quit();

    let asked = asked.lock().expect("the factory's record").clone();
    assert_eq!(
        asked,
        ["advisor 0", "built_in 1", "advisor 1", "built_in 2",],
        "ascending seat id; the human's seat is never offered a built-in operator; a seat \
         the factory does not play is offered an advisor"
    );
    let mut lines = seen.lock().expect("the scripts' record").clone();
    lines.sort();
    assert_eq!(
        lines,
        [
            "advise seat 0: beacons=true",
            "advise seat 1: beacons=true",
            "seat 2: status=true, ready=true",
        ],
        "one fresh instance per seat, each run once in the opening Lull"
    );
}

/// A flood of silent sockets fills the pending slots and disturbs nothing that
/// has authenticated.
#[test]
fn a_flood_of_silent_connections_cannot_disturb_a_live_session() {
    let mut running = start(&config("m-t16a-flood", Some(0)), Box::new(NoOperators));
    let port = running.announce.port;
    let mut live = Client::open(port, &running.announce.admin_token);
    let _ = result(&live.call("get_status", "{}"), "get_status");

    // Twice the cap, and not a byte from any of them.
    let mut silent: Vec<TcpStream> = Vec::new();
    let mut refused = 0_u32;
    for _ in 0..MAX_CONNECTIONS.saturating_mul(2) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("a loopback connection");
        stream
            .set_read_timeout(Some(Duration::from_millis(250)))
            .expect("a read timeout");
        let mut head = [0_u8; 64];
        if let Ok(read) = stream.read(&mut head) {
            let text = String::from_utf8_lossy(head.get(..read).unwrap_or_default()).to_string();
            if text.starts_with("HTTP/1.1 503") {
                refused = refused.saturating_add(1);
            }
        }
        silent.push(stream);
    }
    assert!(
        refused > 0,
        "past the connection cap the host answers an audited 503 rather than growing a \
         thread for every socket"
    );

    // And the session that authenticated before the flood is untouched.
    let status = result(&live.call("get_status", "{}"), "get_status");
    assert!(status.get("status").is_some());
    let ended = result(&live.call("end_lull", "{}"), "end_lull");
    assert_eq!(
        ended.get("_status").and_then(|f| f.get("phase")),
        Some(&Json::String(String::from("push"))),
        "and it can still drive the match"
    );
    drop(silent);
    running.quit();
}

/// End of file on the control pipe ends the host. There is no other shutdown.
#[test]
fn the_host_exits_when_its_control_pipe_closes() {
    let mut running = start(&config("m-t16a-exit", Some(0)), Box::new(NoOperators));
    let mut client = Client::open(running.announce.port, &running.announce.admin_token);
    let _ = result(&client.call("get_status", "{}"), "get_status");

    // The parent lets go. No heartbeat, no clock, nothing to time out.
    drop(running.control.take());
    let thread = running.thread.take().expect("a running host");
    for _ in 0..6_000 {
        if thread.is_finished() {
            thread.join().expect("the host returned");
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the host outlived its control pipe (waited a minute)");
}

/// A rendered token appears on the announce line and nowhere else the host
/// writes.
///
/// Behavioural rather than a source scan: the tokens are searched for in the
/// announce bytes, in the audit log and in every file under the private match
/// cache. A log or a header that carried one would be a secret in a file
/// anybody on this machine can read (`crates/gateway/src/cache.rs` says so out
/// loud).
#[test]
fn a_token_appears_on_the_announce_line_and_nowhere_else_the_host_writes() {
    // A scripted operator on seat 1 and a scripted advisor on the human's seat
    // 0, so that two in-process tokens exist and are used: extended by T18a,
    // an in-process token appears on no announce line and in no file.
    let asked = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut running = start(
        &config("m-t16a-secret", Some(0)),
        Factory::boxed(&[1], &asked, &seen),
    );
    let port = running.announce.port;
    let admin = running.announce.admin_token.clone();
    let seat = running.announce.seat_token.clone().expect("a human seat");

    // A session, so the audit log has something in it -- including a refusal,
    // which is the line most likely to quote what it refused.
    let mut lobby = Client::open(port, &admin);
    let _ = lobby.call("get_status", "{}");
    let _ = lobby.call("get_briefing", "{}");
    let mut wrong = TcpStream::connect(("127.0.0.1", port)).expect("a loopback connection");
    let bad = "f".repeat(64);
    let upgrade = format!(
        "GET /seat HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\n\
         Connection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         Sec-WebSocket-Version: 13\r\nAuthorization: Bearer {bad}\r\n\r\n"
    );
    wrong.write_all(upgrade.as_bytes()).expect("the upgrade");
    // Read the 401 back before letting go: `session::refuse` records the
    // refusal *before* it writes the response, and the queue in front of the
    // surface thread is first in, first out -- so a lobby call made after this
    // read is answered after the refusal has been logged, and the assertion
    // below is not a race.
    let mut head = [0_u8; 32];
    wrong
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("a read timeout");
    let read = wrong.read(&mut head).expect("a refusal");
    assert!(
        String::from_utf8_lossy(head.get(..read).unwrap_or_default()).starts_with("HTTP/1.1 401"),
        "a token this match never minted is refused before a single frame is read"
    );
    drop(wrong);
    let _ = lobby.call("get_status", "{}");
    running.quit();

    // The announce line carries both, once.
    let written = running.written.lock().expect("the announce sink").clone();
    let announced = String::from_utf8(written).expect("UTF-8");
    assert_eq!(announced.matches(&admin).count(), 1, "the admin token");
    assert_eq!(announced.matches(&seat).count(), 1, "the seat's token");
    assert_eq!(
        announced.lines().count(),
        1,
        "one announce line and nothing else on standard output"
    );

    // An in-process token cannot be named by a test that never sees it, so
    // the check is on shape: a rendered token is 64 lower-case hex digits, and
    // the only two such runs the host may ever write are these two, on the
    // announce line.
    assert_eq!(
        hex_runs(&announced),
        [admin.clone(), seat.clone()],
        "the announce line carries the lobby's and the human's tokens and no in-process one"
    );
    assert_eq!(
        seen.lock().expect("the scripts' record").len(),
        2,
        "both in-process seats ran, so both of their tokens were used"
    );

    // And nothing else the host wrote does.
    let root = pharmakos_gateway::cache::locate().expect("a data root");
    let folder = root.join("matches").join("m-t16a-secret");
    let mut checked = 0_u32;
    for entry in walk(&folder) {
        let Ok(bytes) = std::fs::read(&entry) else {
            continue;
        };
        checked = checked.saturating_add(1);
        let text = String::from_utf8_lossy(&bytes);
        for token in [&admin, &seat] {
            assert!(
                !text.contains(token.as_str()),
                "{} carries a rendered token",
                entry.display()
            );
        }
        assert!(
            hex_runs(&text).is_empty(),
            "{} carries 64 hex digits in a row, which is what a rendered token is, the \
             in-process ones included",
            entry.display()
        );
    }
    assert!(
        checked >= 3,
        "the match cache held {checked} files, so the scan would have passed vacuously"
    );
    let log = std::fs::read_to_string(folder.join("audit.log")).expect("an audit log");
    assert!(log.contains("upgrade"), "the session was logged: {log}");
    assert!(
        log.contains("UNAUTHENTICATED"),
        "and so was the refusal: {log}"
    );
}

/// Every run of 64 or more lower-case hex digits in `text`, in order.
fn hex_runs(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut run = String::new();
    for character in text.chars().chain(std::iter::once(' ')) {
        if character.is_ascii_digit() || ('a'..='f').contains(&character) {
            run.push(character);
            continue;
        }
        if run.len() >= 64 {
            found.push(run.clone());
        }
        run.clear();
    }
    found
}

/// Every file under a directory, depth first.
#[allow(
    clippy::disallowed_methods,
    reason = "clippy.toml sanctions read_dir when the listing is collected and sorted before use, which is what happens here"
)]
fn walk(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    let mut found: Vec<std::path::PathBuf> = Vec::new();
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        let mut here: Vec<std::path::PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .collect();
        here.sort();
        for path in here {
            if path.is_dir() {
                stack.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}
