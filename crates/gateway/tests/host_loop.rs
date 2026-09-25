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
use pharmakos_gateway::error::Code;
use pharmakos_gateway::frame::Opcode;
use pharmakos_gateway::rpc::Request;
use pharmakos_gateway::serve::{
    self, Advisor, Announce, BuiltInSeat, Config, ConfigLine, MAX_CONNECTIONS, NoOperators,
    Operators, Setup,
};
use pharmakos_proto::json::{Json, read};
use std::fmt::Write as _;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use support::{SEED, data_root, rules_json};

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
    /// The private match cache's root this host keeps its matches under.
    root: PathBuf,
}

impl Running {
    /// This match's folder in the private match cache.
    fn folder(&self) -> PathBuf {
        self.root.join("matches").join(&self.announce.match_id)
    }
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

/// Start a host on an ephemeral loopback port and read its announce line,
/// with a private match cache of the test's own (named for the match), so a
/// save one run leaves behind is never the next run's refusal.
fn start(config: &Config, operators: Box<dyn Operators>) -> Running {
    let line = ConfigLine {
        config: config.clone(),
        resume: false,
    };
    start_in(&data_root(&config.match_id), &line, operators)
}

/// [`start`], under a data root the caller keeps between hosts -- which is
/// what a resume needs -- and with a whole config line.
fn start_in(root: &Path, line: &ConfigLine, operators: Box<dyn Operators>) -> Running {
    let (sender, messages) = channel::<Vec<u8>>();
    sender
        .send(line.render().into_bytes())
        .expect("the config line");
    let data = root.to_path_buf();
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
        if let Err(error) = serve::run_in(setup, &data, pipe, sink) {
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
                root: root.to_path_buf(),
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
        match Client::try_open(port, token) {
            Ok(client) => client,
            Err(head) => panic!("the upgrade was refused: {head}"),
        }
    }

    /// [`Client::open`], handing back the response head of a refused upgrade
    /// rather than panicking on it.
    fn try_open(port: u16, token: &str) -> Result<Client, String> {
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
        if head.starts_with("HTTP/1.1 101") {
            Ok(client)
        } else {
            Err(head)
        }
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

    // The seventh field (decisions-log item 111, decision C9): `resume`, or
    // nothing. Anything else is refused rather than read as a new match.
    let resume = ConfigLine {
        config: config("m-t16a-lines", Some(1)),
        resume: true,
    };
    assert!(
        resume.render().ends_with("\tresume\n"),
        "{:?}",
        resume.render()
    );
    assert_eq!(
        ConfigLine::parse(&resume.render()).expect("its own line"),
        resume
    );
    let plain = ConfigLine {
        resume: false,
        ..resume.clone()
    };
    assert_eq!(
        ConfigLine::parse(&plain.render()).expect("its own line"),
        plain,
        "six fields are a new match"
    );
    assert_eq!(plain.render(), plain.config.render());
    for seventh in ["again", "RESUME", "resume please"] {
        let line = format!("{}\t{seventh}\n", plain.render().trim_end());
        let error = ConfigLine::parse(&line).expect_err("not a seventh field");
        assert_eq!(error.code, Code::InvalidArgument, "`{seventh}`");
    }
    // And nothing after the seventh: an eighth field is refused, not ignored,
    // whether the seventh asks for a resume or not.
    for tail in ["resume\tgarbage", "-\tgarbage", "\t"] {
        let line = format!("{}\t{tail}\n", plain.render().trim_end());
        let error = ConfigLine::parse(&line).expect_err("an eighth field");
        assert_eq!(error.code, Code::InvalidArgument, "`{tail}`");
        assert!(error.message.contains("at most"), "{}", error.message);
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
    // Extended by T17: end the Lull, so the Push's beginning writes a save and
    // both seats' sealed playbooks, and the scan below covers them too.
    let _ = result(&lobby.call("end_lull", "{}"), "end_lull");
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

    // And nothing else the host wrote does -- save.json and the sealed
    // playbooks included (T17).
    let folder = running.folder();
    assert_the_push_was_written(&folder);
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
        checked >= 6,
        "the match cache held {checked} files, so the scan would have passed vacuously"
    );
    let log = std::fs::read_to_string(folder.join("audit.log")).expect("an audit log");
    assert!(log.contains("upgrade"), "the session was logged: {log}");
    assert!(
        log.contains("UNAUTHENTICATED"),
        "and so was the refusal: {log}"
    );
}

/// The files a Push's beginning writes are there, so a scan of the folder
/// covers them.
fn assert_the_push_was_written(folder: &Path) {
    for written in [
        folder.join("save.json"),
        folder
            .join("seats")
            .join("0")
            .join("sealed")
            .join("1.jsonc"),
        folder
            .join("seats")
            .join("1")
            .join("sealed")
            .join("1.jsonc"),
    ] {
        assert!(
            written.is_file(),
            "{} was not written, so the scan would not cover it",
            written.display()
        );
    }
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

// ---------------------------------------------------------------------------
// Saves, resumes and the private replay (T17)
// ---------------------------------------------------------------------------

/// Host a match to its opening Lull and close the pipe at once: the quickest
/// way to a `lull` save. Returns what `run_in` returned.
fn host_and_quit(root: &Path, line: &ConfigLine) -> Result<(), pharmakos_gateway::Error> {
    let setup = Setup {
        rules_json: rules_json(),
        library: Some(support::library_folder()),
        operators: Box::new(NoOperators),
    };
    serve::run_in(
        setup,
        root,
        std::io::Cursor::new(line.render().into_bytes()),
        Vec::new(),
    )
}

/// A new match's line.
fn new_line(config: Config) -> ConfigLine {
    ConfigLine {
        config,
        resume: false,
    }
}

/// A resume line.
fn resume_line(config: Config) -> ConfigLine {
    ConfigLine {
        config,
        resume: true,
    }
}

/// The save a match folder holds, parsed.
fn saved(folder: &Path) -> pharmakos_gateway::save::Save {
    let text = std::fs::read_to_string(folder.join("save.json")).expect("a save");
    pharmakos_gateway::save::Save::parse(&text).expect("a save this build reads")
}

/// A match folder under a data root.
fn folder_of(root: &Path, match_id: &str) -> PathBuf {
    root.join("matches").join(match_id)
}

/// The phase the `_status` footer of an answer names.
fn phase_of(answer: &Json) -> String {
    let footer = answer
        .get("_status")
        .or_else(|| answer.get("status"))
        .expect("a status footer");
    match footer.get("phase") {
        Some(Json::String(phase)) => phase.clone(),
        other => panic!("a phase, and it is {other:?}"),
    }
}

/// Run the Push to its end through `advance_push`, as the pacer's skip does.
fn skip_to_recap(lobby: &mut Client) {
    for _ in 0..100 {
        let answer = result(
            &lobby.call("advance_push", r#"{"ms":60000}"#),
            "advance_push",
        );
        if phase_of(&answer) != "push" {
            return;
        }
    }
    panic!("a Push that never ended");
}

/// Item 84's second boundary, and C8's rule that there is no saving
/// mid-Push: end of file in a Lull writes a `lull` save; end of file in a Push
/// writes nothing, and the `sealed` save its beginning wrote is what stands.
#[test]
fn closing_the_pipe_in_a_lull_saves_and_closing_it_in_a_push_does_not() {
    let root = data_root("t17-close");
    host_and_quit(&root, &new_line(config("m-t17-close-lull", Some(0))))
        .expect("a host that opened and was closed");
    let lull = saved(&folder_of(&root, "m-t17-close-lull"));
    assert_eq!(lull.state.boundary, pharmakos_gateway::save::Boundary::Lull);
    assert_eq!(lull.state.round, 1);
    assert_eq!(lull.config, config("m-t17-close-lull", Some(0)));

    let mut running = start_in(
        &root,
        &new_line(config("m-t17-close-push", Some(0))),
        Box::new(NoOperators),
    );
    let mut lobby = Client::open(running.announce.port, &running.announce.admin_token);
    let _ = result(&lobby.call("end_lull", "{}"), "end_lull");
    // A call after the one that began the Push, so the flush that followed it
    // has certainly happened: the surface thread answers in order.
    let _ = result(&lobby.call("get_status", "{}"), "get_status");
    let folder = running.folder();
    let at_the_push = std::fs::read(folder.join("save.json")).expect("the Push's save");
    let _ = result(&lobby.call("advance_push", r#"{"ms":200}"#), "advance_push");
    running.quit();
    assert_eq!(
        std::fs::read(folder.join("save.json")).expect("still there"),
        at_the_push,
        "closing the pipe mid-Push wrote nothing: the Push's own save stands"
    );
    assert_eq!(
        saved(&folder).state.boundary,
        pharmakos_gateway::save::Boundary::Sealed
    );
    let log = std::fs::read_to_string(folder.join("audit.log")).expect("an audit log");
    assert!(
        !log.contains("save lull"),
        "no Lull save was attempted in a Push: {log}"
    );
}

/// The wave-6 notes, H16: a save outlives its process, so a new match under
/// its id is refused -- before a map is generated -- and the save is left as
/// it was.
#[test]
fn a_new_match_cannot_overwrite_a_saved_one() {
    let root = data_root("t17-overwrite");
    let line = new_line(config("m-t17-overwrite", Some(0)));
    host_and_quit(&root, &line).expect("the first match");
    let folder = folder_of(&root, "m-t17-overwrite");
    let save = std::fs::read(folder.join("save.json")).expect("a save");
    let header = std::fs::read(folder.join("match.json")).expect("a header");

    let error = host_and_quit(&root, &line).expect_err("a second new match under the same id");
    assert_eq!(error.code, Code::InvalidArgument);
    assert!(error.message.contains("resume"), "{}", error.message);
    assert_eq!(
        std::fs::read(folder.join("save.json")).expect("a save"),
        save
    );
    assert_eq!(
        std::fs::read(folder.join("match.json")).expect("a header"),
        header
    );

    // And the resume line is the way back in.
    host_and_quit(&root, &resume_line(config("m-t17-overwrite", Some(0))))
        .expect("a resume of the saved match");
    let log = std::fs::read_to_string(folder.join("audit.log")).expect("an audit log");
    assert!(log.contains("\tresumed\tok\n"), "{log}");
    let sequence: Vec<u64> = log
        .lines()
        .skip(1)
        .filter_map(|line| line.split('\t').next()?.parse::<u64>().ok())
        .collect();
    assert!(
        sequence.windows(2).all(|pair| pair.first() < pair.get(1)),
        "a resumed host numbers its lines after the last one: {sequence:?}"
    );
    assert_eq!(
        std::fs::read(folder.join("match.json")).expect("a header"),
        header,
        "a resume does not rewrite match.json"
    );
}

/// Decisions-log item 112 (8): a resume line repeats **all six** of the
/// saved match's values, and one that differs in any of them is refused as
/// `INVALID_ARGUMENT` -- which `gamectl host` turns into exit code 2 with the
/// message on stderr.
#[test]
fn a_resume_line_that_disagrees_with_the_save_is_refused() {
    let root = data_root("t17-disagree");
    let saved_config = Config {
        segment_lengths_ms: vec![1_000, 2_000],
        ..config("m-t17-disagree", Some(0))
    };
    host_and_quit(&root, &new_line(saved_config.clone())).expect("the saved match");
    let variants: [(&str, Config); 6] = [
        (
            "match id",
            Config {
                match_id: String::from("m-t17-disagree-other"),
                ..saved_config.clone()
            },
        ),
        (
            "seed",
            Config {
                seed: SEED.wrapping_add(1),
                ..saved_config.clone()
            },
        ),
        (
            "seat count",
            Config {
                seats: 3,
                ..saved_config.clone()
            },
        ),
        (
            "human seat",
            Config {
                human_seat: Some(1),
                ..saved_config.clone()
            },
        ),
        (
            "segment lengths",
            Config {
                segment_lengths_ms: vec![1_000],
                ..saved_config.clone()
            },
        ),
        (
            "round limit",
            Config {
                round_limit: 3,
                ..saved_config.clone()
            },
        ),
    ];
    // The match-id variant needs a save under the other id for the line to
    // disagree *with*: with no folder at all, `MatchCache::reopen` would
    // refuse it for having nothing to resume, and the test would pass without
    // `Save::check` ever reading the id. So the saved match's own file is
    // copied there, and its config still names the original id.
    let other = folder_of(&root, "m-t17-disagree-other");
    std::fs::create_dir_all(&other).expect("the other id's folder");
    std::fs::copy(
        folder_of(&root, "m-t17-disagree").join("save.json"),
        other.join("save.json"),
    )
    .expect("the saved match's file under the other id");
    for (field, asked) in variants {
        let error = host_and_quit(&root, &resume_line(asked)).expect_err(field);
        assert_eq!(
            error.code,
            Code::InvalidArgument,
            "{field}: {}",
            error.message
        );
        assert!(
            error
                .message
                .contains(&format!("the resume line asks for {field} ")),
            "the refusal names the field that differs, {field}: {}",
            error.message
        );
    }
    host_and_quit(&root, &resume_line(saved_config)).expect("the line that agrees resumes");
}

/// Spec section 3: saves "won't load on a mismatch" of the rules hash or the
/// verifier version. Both asserted, each by changing only that one stamp in
/// the file.
#[test]
fn a_save_from_a_different_rules_table_or_verifier_is_refused() {
    let root = data_root("t17-stamp");
    let line = config("m-t17-stamp", Some(0));
    host_and_quit(&root, &new_line(line.clone())).expect("the saved match");
    let folder = folder_of(&root, "m-t17-stamp");
    let original = std::fs::read_to_string(folder.join("save.json")).expect("a save");
    let stamp = saved(&folder).stamp;

    let other_rules = original.replace(
        &format!(
            "\"rules_hash\": \"{}\"",
            pharmakos_sim::hex(stamp.rules_hash)
        ),
        &format!(
            "\"rules_hash\": \"{}\"",
            pharmakos_sim::hex(stamp.rules_hash ^ 1)
        ),
    );
    assert_ne!(other_rules, original, "the rules hash was changed");
    std::fs::write(folder.join("save.json"), &other_rules).expect("rewritten");
    let error = host_and_quit(&root, &resume_line(line.clone())).expect_err("another rules table");
    assert_eq!(error.code, Code::InvalidArgument);
    assert!(error.message.contains("rules table"), "{}", error.message);

    let other_verifier = original.replace(
        &format!("\"verifier_version\": \"{}\"", stamp.verifier_version),
        "\"verifier_version\": \"0.0.0-another\"",
    );
    assert_ne!(other_verifier, original, "the verifier version was changed");
    std::fs::write(folder.join("save.json"), &other_verifier).expect("rewritten");
    let error = host_and_quit(&root, &resume_line(line.clone())).expect_err("another verifier");
    assert_eq!(error.code, Code::InvalidArgument);
    assert!(error.message.contains("verifier"), "{}", error.message);

    std::fs::write(folder.join("save.json"), &original).expect("put back");
    host_and_quit(&root, &resume_line(line)).expect("the save as written resumes");
}

/// The wave-6 notes, decision C8: a client killed mid-Push -- its pipe closed
/// -- resumes into that same Push with the same seals and plays it to the
/// same chain, tick for tick, as a host that was never interrupted.
#[test]
fn a_killed_client_mid_push_replays_that_push_to_the_same_chain() {
    let line = config("m-t17-killed", Some(0));

    // The uninterrupted run.
    let reference = data_root("t17-killed-reference");
    let mut running = start_in(&reference, &new_line(line.clone()), Box::new(NoOperators));
    let mut lobby = Client::open(running.announce.port, &running.announce.admin_token);
    let _ = result(&lobby.call("end_lull", "{}"), "end_lull");
    skip_to_recap(&mut lobby);
    let _ = result(&lobby.call("get_status", "{}"), "get_status");
    running.quit();
    let expected = std::fs::read_to_string(running.folder().join("replay").join("1.hashes.txt"))
        .expect("the uninterrupted chain");
    assert_eq!(expected.lines().count(), 20, "1 000 ms at 50 ms a tick");

    // Killed a few ticks in.
    let root = data_root("t17-killed");
    let mut running = start_in(&root, &new_line(line.clone()), Box::new(NoOperators));
    let killed = running.announce.clone();
    let mut lobby = Client::open(running.announce.port, &running.announce.admin_token);
    let _ = result(&lobby.call("end_lull", "{}"), "end_lull");
    let _ = result(&lobby.call("advance_push", r#"{"ms":300}"#), "advance_push");
    running.quit();
    let folder = running.folder();
    assert!(
        !folder.join("replay").join("1.hashes.txt").exists(),
        "the killed Push never reached its end"
    );

    // Resumed: straight into the Push, with no Lull to re-plan it in.
    let mut running = start_in(&root, &resume_line(line), Box::new(NoOperators));
    // A new process, a new announce line: spec section 3's "seat tokens are
    // reissued". Neither of the dead process's tokens is this one's, and its
    // admin token does not open the resumed host.
    assert_ne!(running.announce.admin_token, killed.admin_token);
    assert!(running.announce.seat_token.is_some());
    assert_ne!(running.announce.seat_token, killed.seat_token);
    assert!(
        Client::try_open(running.announce.port, &killed.admin_token).is_err(),
        "the dead process's admin token is refused by the resumed host"
    );
    let mut lobby = Client::open(running.announce.port, &running.announce.admin_token);
    let status = result(&lobby.call("get_status", "{}"), "get_status");
    assert_eq!(
        phase_of(&status),
        "push",
        "a sealed save resumes into its Push"
    );
    let refused = lobby.call("end_lull", "{}");
    assert!(refused.get("error").is_some(), "there is no Lull to end");
    skip_to_recap(&mut lobby);
    let _ = result(&lobby.call("get_status", "{}"), "get_status");
    running.quit();
    assert_eq!(
        std::fs::read_to_string(folder.join("replay").join("1.hashes.txt"))
            .expect("the resumed chain"),
        expected,
        "the same Push, to the same chain"
    );
}

/// What `match.json` says, read back as the replay needs it.
struct MatchHeader {
    seed: u64,
    seats: u32,
    round_limit: u32,
    lengths: Vec<i32>,
    rules_hash: String,
}

impl MatchHeader {
    fn read(folder: &Path) -> MatchHeader {
        let header = read(&std::fs::read_to_string(folder.join("match.json")).expect("match.json"))
            .expect("JSON");
        let text = |key: &str| match header.get(key) {
            Some(Json::String(value)) => value.clone(),
            other => panic!("`{key}` is a string, and it is {other:?}"),
        };
        let whole = |key: &str| match header.get(key) {
            Some(Json::Number(lexeme)) => lexeme.parse::<u32>().expect("a whole number"),
            other => panic!("`{key}` is a number, and it is {other:?}"),
        };
        let lengths: Vec<i32> = match header.get("segment_lengths_ms") {
            Some(Json::Array(items)) => items
                .iter()
                .map(|item| match item {
                    Json::Number(lexeme) => lexeme.parse::<i32>().expect("a length"),
                    other => panic!("a length, and it is {other:?}"),
                })
                .collect(),
            other => panic!("the ladder, and it is {other:?}"),
        };
        MatchHeader {
            seed: u64::from_str_radix(text("match_seed").trim_start_matches("0x"), 16)
                .expect("a seed"),
            seats: whole("seats"),
            round_limit: whole("round_limit"),
            lengths,
            rules_hash: text("rules_hash"),
        }
    }
}

/// The private replay is inputs only (decision C11): `match.json` for the
/// seed, settings and rules hash, `seats/<seat>/sealed/<round>.jsonc` for
/// every seat's orders, `replay/<round>.hashes.txt` for every segment's chain.
/// Re-hosting a match from those files alone reproduces its chain.
#[test]
fn the_replay_files_reproduce_the_matchs_chain() {
    let line = Config {
        segment_lengths_ms: vec![1_000, 1_500],
        ..config("m-t17-replay", Some(0))
    };
    let mut running = start_in(
        &data_root("t17-replay"),
        &new_line(line),
        Box::new(NoOperators),
    );
    let seat_token = running.announce.seat_token.clone().expect("a human seat");
    let mut lobby = Client::open(running.announce.port, &running.announce.admin_token);
    let mut seat = Client::open(running.announce.port, &seat_token);
    for round in 1..=2_u32 {
        // The human seat plays its own orders in round 1 and none in round 2,
        // so the replay holds both a submission and a gateway-filed playbook.
        if round == 1 {
            let _ = result(
                &seat.call(
                    "submit_plan",
                    &format!(
                        r#"{{"playbook_jsonc":{}}}"#,
                        support::quote(pharmakos_gateway::host::SAFE_PLAYBOOK)
                    ),
                ),
                "submit_plan",
            );
        }
        let _ = result(&lobby.call("end_lull", "{}"), "end_lull");
        skip_to_recap(&mut lobby);
        let _ = result(&lobby.call("end_recap", "{}"), "end_recap");
    }
    let _ = result(&lobby.call("get_status", "{}"), "get_status");
    running.quit();
    let folder = running.folder();

    // Everything below reads the folder and nothing else.
    let header = MatchHeader::read(&folder);
    let rules = support::rules();
    assert_eq!(
        header.rules_hash,
        pharmakos_sim::hex(rules.rules_hash()),
        "match.json names the rules table the replay needs"
    );
    let (seed, seats, round_limit, lengths) = (
        header.seed,
        header.seats,
        header.round_limit,
        header.lengths.clone(),
    );

    let mut host = pharmakos_gateway::host::Host::open_from(
        &rules_json(),
        seed,
        seats,
        &pharmakos_gateway::host::Settings {
            segment_lengths_ms: lengths,
            round_limit,
            units_per_seat: 0,
        },
        None,
    )
    .expect("a match from match.json");
    for round in 1..=round_limit {
        let mut plans = Vec::new();
        for raw in 0..u8::try_from(seats).expect("a seat count") {
            let path = folder
                .join("seats")
                .join(raw.to_string())
                .join("sealed")
                .join(format!("{round}.jsonc"));
            let playbook = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let canonical =
                pharmakos_plan_core::canonicalise_text(&playbook).expect("a canonical form");
            plans.push((
                pharmakos_sim::tables::SeatId::new(raw),
                pharmakos_sim::interpreter::Plan::compile(&canonical.playbook, &rules)
                    .expect("a plan"),
            ));
        }
        host.seal_plans(plans).expect("sealed in a Lull");
        assert!(host.begin_push());
        let mut chain = String::new();
        while let Some(report) = host.step() {
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
        let recorded =
            std::fs::read_to_string(folder.join("replay").join(format!("{round}.hashes.txt")))
                .expect("the recorded chain");
        assert!(!recorded.contains('\r'), "LF endings");
        assert_eq!(
            chain, recorded,
            "round {round}'s chain, re-hosted from its inputs"
        );
        assert!(host.end_recap());
    }
}
