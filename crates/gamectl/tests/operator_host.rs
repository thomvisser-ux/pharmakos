// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A two-seat match against Easy, through `serve::run` and a real loopback
//! socket, as the Godot lobby plays it (T18; decisions-log items 111 and 112).
//!
//! Seat 0 is the human's, over a WebSocket with the token the announce line
//! carries; seat 1 is Easy, played by the factory `gamectl host` hands the
//! host (`EasyOperators`), which also advises seat 0. The lobby's own
//! connection holds the admin token and paces the match. The test is the
//! demo's spine: the human is shown Easy's safe playbook and its wizard
//! suggestion, seals, says ready; Easy has sealed and said ready on its own;
//! so the lobby's Ready ends the Lull (`all_ready`, T19 PR 1's finding 6), the
//! Push runs, and the match reaches its recap.
//!
//! # The private match cache
//!
//! `serve::run` keeps the audit log in the real per-user data root, which is
//! how this test sees seat 1's own calls without a seam. After T17 a match
//! id whose folder holds a save is refused, so the id here is one no folder
//! has yet, found by probing, and the folder is removed when the test ends.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pharmakos_gamectl::host::{EasyOperators, LIBRARY_PATH};
use pharmakos_gateway::frame::Opcode;
use pharmakos_gateway::serve::{self, Announce, Config, Setup};
use pharmakos_proto::json::{Json, read, write};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/gamectl sits two levels below the root")
        .to_path_buf()
}

fn rules_json() -> String {
    std::fs::read_to_string(root().join("rules").join("rules.v1.json")).expect("the rules text")
}

// ---------------------------------------------------------------------------
// A control pipe and an announce sink, in process (as `host_loop.rs`)
// ---------------------------------------------------------------------------

/// The child's standard input: a read past the last message is end of file.
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
        let take = self.held.len().saturating_sub(self.at).min(out.len());
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

/// The child's standard output.
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

/// A running host.
struct Running {
    announce: Announce,
    control: Option<Sender<Vec<u8>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Running {
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

fn start(config: &Config) -> Running {
    let (sender, messages) = channel::<Vec<u8>>();
    sender
        .send(config.render().into_bytes())
        .expect("the config line");
    let written = Arc::new(Mutex::new(Vec::new()));
    let sink = Sink(Arc::clone(&written));
    let rules = rules_json();
    let setup = Setup {
        operators: Box::new(EasyOperators::new(&rules).expect("the operator reads the rules")),
        rules_json: rules,
        library: Some(root().join(LIBRARY_PATH)),
    };
    let thread = std::thread::spawn(move || {
        let pipe = ControlPipe {
            messages,
            held: Vec::new(),
            at: 0,
        };
        if let Err(error) = serve::run(setup, pipe, sink) {
            panic!("the host would not start: {error}");
        }
    });
    for _ in 0..12_000 {
        let line = written.lock().ok().and_then(|held| {
            let index = held.iter().position(|byte| *byte == b'\n')?;
            String::from_utf8(held.get(..index).unwrap_or_default().to_vec()).ok()
        });
        if let Some(line) = line {
            return Running {
                announce: Announce::parse(&line).expect("an announce line"),
                control: Some(sender),
                thread: Some(thread),
            };
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the host never announced a port (waited two minutes)");
}

// ---------------------------------------------------------------------------
// A client (as `host_loop.rs`)
// ---------------------------------------------------------------------------

struct Client {
    stream: TcpStream,
    buffer: Vec<u8>,
    id: u32,
}

impl Client {
    fn open(port: u16, token: &str) -> Client {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("a loopback connection");
        stream
            .set_read_timeout(Some(Duration::from_secs(120)))
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

    fn call(&mut self, method: &str, params: &Json) -> Json {
        self.id = self.id.saturating_add(1);
        let body = write(params);
        let text = format!(
            r#"{{"jsonrpc":"2.0","id":{},"method":"{method}","params":{}}}"#,
            self.id,
            body.trim_end()
        );
        let frame = masked(Opcode::Text, text.as_bytes());
        self.stream.write_all(&frame).expect("a call");
        let answer = self.read_frame();
        read(&answer).expect("JSON")
    }

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

/// A masked client frame (RFC 6455 section 5.1).
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

/// One unmasked server frame.
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

fn result(response: &Json, what: &str) -> Json {
    response
        .get("result")
        .cloned()
        .unwrap_or_else(|| panic!("{what} was refused: {response:?}"))
}

fn params(text: &str) -> Json {
    read(text).expect("params")
}

fn text_of(value: &Json, key: &str) -> String {
    match value.get(key) {
        Some(Json::String(text)) => text.clone(),
        other => panic!("`{key}` is a string, and it is {other:?}"),
    }
}

fn phase_of(result: &Json) -> String {
    result
        .get("_status")
        .map(|footer| text_of(footer, "phase"))
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// The match cache
// ---------------------------------------------------------------------------

/// The per-user matches folder `serve::run` keeps its caches in.
fn matches_folder() -> PathBuf {
    pharmakos_gateway::cache::locate()
        .expect("a per-user data root")
        .join(pharmakos_gateway::cache::MATCHES_DIR)
}

/// A match id no folder has yet: unique to this run, because after T17 a
/// match id whose folder holds a save is refused.
fn fresh_match_id() -> String {
    let folder = matches_folder();
    let pid = std::process::id();
    (0_u32..10_000)
        .map(|n| format!("t18-easy-{pid}-{n}"))
        .find(|id| !folder.join(id).exists())
        .expect("a free match id")
}

/// The match's folder, removed when the test is done with it.
struct Cache(PathBuf);

impl Drop for Cache {
    fn drop(&mut self) {
        // This test's own folder, made by the host it started.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ---------------------------------------------------------------------------
// The test
// ---------------------------------------------------------------------------

/// Item 111's acceptance: "a two-seat hosted match against Easy reaches the
/// recap with both seats sealed and ready", through `serve::run`.
#[test]
fn a_two_seat_hosted_match_against_easy_reaches_the_recap_with_both_seats_sealed_and_ready() {
    let match_id = fresh_match_id();
    let cache = Cache(matches_folder().join(&match_id));
    let config = Config {
        match_id: match_id.clone(),
        seed: pharmakos_sim::DETERMINISM_MATCH_SEED,
        seats: 2,
        human_seat: Some(0),
        segment_lengths_ms: vec![5_000],
        round_limit: 1,
    };
    let mut running = start(&config);
    let port = running.announce.port;
    let seat_token = running.announce.seat_token.clone().expect("a human seat");
    let mut human = Client::open(port, &seat_token);
    let mut lobby = Client::open(port, &running.announce.admin_token);

    // The human is shown Easy's safe playbook for its own seat, and the
    // wizard's page comes pre-filled with Easy's suggestion and its why.
    let safe = result(&human.call("get_safe_plan", &params("{}")), "get_safe_plan");
    let safe_playbook = text_of(&safe, "playbook_jsonc");
    assert!(
        safe_playbook.contains("\"label\": \"to_safety\""),
        "{safe_playbook}"
    );
    let wizard = result(
        &human.call(
            "instantiate_template",
            &params(r#"{"template_id":"expand_and_mine","suggested":true}"#),
        ),
        "instantiate_template{suggested}",
    );
    assert!(!text_of(&wizard, "why").is_empty(), "Easy says why");

    // The human seals Easy's safe playbook and says ready.
    let submitted = result(
        &human.call(
            "submit_plan",
            &Json::Object(vec![(
                String::from("playbook_jsonc"),
                Json::String(safe_playbook),
            )]),
        ),
        "submit_plan",
    );
    assert_eq!(submitted.get("accepted"), Some(&Json::Bool(true)));
    let _ = result(
        &human.call("set_ready", &params(r#"{"ready":true}"#)),
        "set_ready",
    );

    // Easy planned, sealed and said ready at the Lull's start, so the
    // lobby's Ready ends the Lull.
    let clock = result(
        &lobby.call(
            "report_host_clock",
            &params(r#"{"elapsed_ms":250,"remaining_ms":179750}"#),
        ),
        "report_host_clock",
    );
    assert_eq!(
        clock.get("all_ready"),
        Some(&Json::Bool(true)),
        "both seats are ready: {clock:?}"
    );
    let ended = result(&lobby.call("end_lull", &params("{}")), "end_lull");
    assert_eq!(phase_of(&ended), "push");
    let mut phase = String::from("push");
    for _ in 0..8 {
        if phase != "push" {
            break;
        }
        let advanced = result(
            &lobby.call("advance_push", &params(r#"{"ms":60000}"#)),
            "advance_push",
        );
        phase = phase_of(&advanced);
    }
    assert_eq!(phase, "recap", "the match reached its recap");
    running.quit();

    // Both seats sealed and said ready, each through its own door: the
    // audit log the host kept in the match's private cache says so.
    let audit = std::fs::read_to_string(cache.0.join("audit.log")).expect("the audit log");
    for seat in ["seat.0", "seat.1"] {
        for action in ["call submit_plan", "call set_ready"] {
            assert!(
                audit.lines().any(|line| {
                    let fields: Vec<&str> = line.split('\t').collect();
                    fields.get(2) == Some(&seat)
                        && fields.get(4) == Some(&action)
                        && fields.get(5) == Some(&"ok")
                }),
                "{seat} made `{action}` and it was answered:\n{audit}"
            );
        }
    }
    drop(cache);
}
