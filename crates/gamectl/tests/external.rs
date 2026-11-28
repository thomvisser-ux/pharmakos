// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The external seat's commands, `gamectl match` (decisions-log item 231),
//! and the claim that makes the door worth having: **a seat played through it
//! plays the same match as a seat played in process.**
//!
//! # Parity
//!
//! The built-in operator is already an ordinary client of the gateway. So the
//! strongest thing to say about the external seat is said with it: Easy,
//! driven from this test through `External::call_bytes` as lines of text,
//! with Easy built in on the other seat, plays a match whose per-tick chain
//! is byte for byte the chain of the same match with Easy seated in process
//! on both seats by the scenario runner. Everything that differs between the
//! two (the token's limits, the audit lines, the advisor the outside seat
//! gets, the text round trip, the saves written at every boundary) is on the
//! gateway's side of the door, and none of it may reach the sim.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules -- not \
              an integration test's helper functions. A panic is this file's failure report."
)]

use pharmakos_gamectl::exit::Exit;
use pharmakos_gamectl::external::{self, LINE_FILE, MATCH_ID};
use pharmakos_gamectl::host::EasyOperators;
use pharmakos_gamectl::scenario::{self, Assertion, Scenario, Seat, SeatKind, Segment};
use pharmakos_gateway::rpc::Request;
use pharmakos_gateway::serve::{ConfigLine, External, Operators, Setup};
use pharmakos_proto::json::{self, Json};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// The workspace root: this crate is `<root>/crates/gamectl`.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/gamectl sits two levels below the workspace root")
        .to_path_buf()
}

/// A folder of this test's own that does not exist yet, for one match.
fn fresh_dir(name: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let serial = NEXT.fetch_add(1, Ordering::Relaxed);
    let parent = Path::new(env!("CARGO_TARGET_TMPDIR")).join("gamectl-external");
    std::fs::create_dir_all(&parent).expect("the scratch folder");
    let dir = parent.join(format!("{name}-{}-{serial}", std::process::id()));
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("a dead run's folder {} stays: {error}", dir.display()),
    }
    dir
}

/// Run one command line in process, from the workspace root.
fn gamectl(args: &[&str]) -> pharmakos_gamectl::Outcome {
    let root = root();
    let mut all: Vec<String> = vec![String::from("--root"), root.display().to_string()];
    all.extend(args.iter().map(|arg| (*arg).to_owned()));
    pharmakos_gamectl::run(all, &root)
}

fn dir_arg(dir: &Path) -> String {
    dir.display().to_string()
}

/// The phase and the round an answer's status names.
fn standing(answer: &Json) -> (String, String) {
    let result = answer.get("result").expect("a result");
    let status = result
        .get("_status")
        .or_else(|| result.get("status"))
        .expect("a status");
    let text = |key: &str| match status.get(key) {
        Some(Json::String(text) | Json::Number(text)) => text.clone(),
        other => panic!("`{key}` is {other:?}"),
    };
    (text("phase"), text("round"))
}

fn code_of(answer: &Json) -> String {
    match answer
        .get("error")
        .and_then(|error| error.get("data"))
        .and_then(|data| data.get("code"))
    {
        Some(Json::String(code)) => code.clone(),
        _ => panic!("not a refusal with a code: {answer:?}"),
    }
}

// ---------------------------------------------------------------------------
// Parity
// ---------------------------------------------------------------------------

const PARITY_SEED: u64 = 0x14;
const PARITY_ROUNDS: u32 = 3;

/// The three rounds' Push lengths, written out on both sides: three, five and
/// eight minutes, the lobby's ladder.
const PARITY_SEGMENTS_MS: [i32; 3] = [180_000, 300_000, 480_000];

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one match played twice and compared: the scenario's fields and the door's loop are each written out once, and a helper for either would take the whole setup as parameters"
)]
fn easy_through_the_external_door_plays_the_match_easy_plays_in_process() {
    let root = root();
    let rules_path = root.join("rules").join("rules.v1.json");
    let rules = pharmakos_sim::rules::RulesTable::load(&rules_path).expect("the shipped rules");
    let rules_json = rules.canonical_json().to_owned();

    // In process: the scenario runner, Easy on both seats.
    let segments: Vec<Segment> = PARITY_SEGMENTS_MS
        .into_iter()
        .enumerate()
        .map(|(index, length_ms)| Segment { index, length_ms })
        .collect();
    let scenario = Scenario {
        source: String::from("<external parity>"),
        name: String::from("external-parity"),
        summary: String::new(),
        seed: PARITY_SEED,
        generator: String::from("v1"),
        rules: String::from("rules/rules.v1.json"),
        round_limit: PARITY_ROUNDS,
        seats: vec![
            Seat {
                seat: 0,
                kind: SeatKind::Builtin,
            },
            Seat {
                seat: 1,
                kind: SeatKind::Builtin,
            },
        ],
        segments,
        assertions: vec![Assertion::EventFired {
            event: String::from("segment_ended"),
            seat: None,
            by_tick: u32::MAX,
        }],
        nightly: true,
    };
    let (in_process, ending) =
        scenario::run::play_to_end(&root, &scenario).expect("the in-process match plays");

    // Through the door: Easy built in on seat 1, and on seat 0 an Easy this
    // test drives, every call a line of text into `External::call_bytes`.
    let dir = fresh_dir("parity");
    let lengths: Vec<String> = PARITY_SEGMENTS_MS.iter().map(i32::to_string).collect();
    let line = ConfigLine::parse(&format!(
        "{MATCH_ID}\t0x{PARITY_SEED:x}\t2\t0\t{}\t{PARITY_ROUNDS}\tcustom\n",
        lengths.join(",")
    ))
    .expect("a config line");
    let mut operators = EasyOperators::new(&rules_json).expect("Easy reads the rules");
    let mut outside = operators.built_in(0).expect("Easy for the outside seat");
    let setup = Setup {
        rules_json,
        library: Some(root.join(pharmakos_gamectl::host::LIBRARY_PATH)),
        operators: Box::new(operators),
    };
    let mut match_ = External::open(setup, &dir, &line).expect("the external match opens");
    let mut rounds = 0_u32;
    while !match_.ended() {
        rounds += 1;
        assert!(
            rounds <= PARITY_ROUNDS,
            "the match played past its round limit"
        );
        let mut calls = 0_u32;
        let mut call = |request: &Request| -> Json {
            calls += 1;
            let text = json::write_line(&Json::Object(vec![
                (String::from("jsonrpc"), Json::String(String::from("2.0"))),
                (String::from("id"), request.id.clone()),
                (String::from("method"), Json::String(request.method.clone())),
                (String::from("params"), request.params.clone()),
            ]));
            // What the bridge does with a line, and what it writes back.
            let answer = match_.call_bytes(text.as_bytes());
            json::read(&json::write_line(&answer)).expect("the answer is one line of JSON")
        };
        outside.plan(0, &mut call);
        assert!(calls > 0, "Easy made no call in round {rounds}");
        match_.advance().expect("the round plays");
    }
    match_.close().expect("the match is put down");

    let mut through_the_door = String::new();
    for round in 1..=rounds {
        through_the_door.push_str(
            &std::fs::read_to_string(
                dir.join("matches")
                    .join(MATCH_ID)
                    .join("replay")
                    .join(format!("{round}.hashes.txt")),
            )
            .expect("the round's chain"),
        );
    }
    assert_eq!(
        rounds, ending.rounds,
        "the two matches ended in different rounds"
    );
    assert!(
        in_process.chain.lines().count() > 1_000,
        "a match of some length"
    );
    assert!(
        through_the_door == in_process.chain,
        "the chains part: {} lines through the door, {} in process, first difference at line {:?}",
        through_the_door.lines().count(),
        in_process.chain.lines().count(),
        through_the_door
            .lines()
            .zip(in_process.chain.lines())
            .position(|(door, process)| door != process)
    );
}

// ---------------------------------------------------------------------------
// No way out
// ---------------------------------------------------------------------------

/// The game never starts, calls or pays for the program that plays a seat,
/// and sends nothing anywhere (AGENTS.md section 7): no crate the workspace
/// locks is an HTTP or TLS client. `deny.toml` bans the same names; this
/// holds the lock file to them where `cargo-deny` is not installed.
#[test]
fn no_crate_in_the_lock_file_is_an_http_or_tls_client() {
    const CLIENTS: &[&str] = &[
        "reqwest",
        "ureq",
        "hyper",
        "isahc",
        "attohttpc",
        "minreq",
        "surf",
        "curl",
        "rustls",
        "native-tls",
        "openssl",
    ];
    let root = root();
    let lock = std::fs::read_to_string(root.join("Cargo.lock")).expect("the lock file");
    let deny = std::fs::read_to_string(root.join("deny.toml")).expect("deny.toml");
    let locked: Vec<&str> = lock
        .lines()
        .filter_map(|line| line.strip_prefix("name = \""))
        .filter_map(|rest| rest.strip_suffix('"'))
        .collect();
    assert!(locked.len() > 10, "the lock file names its packages");
    for client in CLIENTS {
        assert!(
            !locked.contains(client),
            "`{client}` is in Cargo.lock: no crate here is an HTTP or TLS client"
        );
        assert!(
            deny.contains(&format!("{{ crate = \"{client}\"")),
            "`{client}` is not banned in deny.toml"
        );
    }
}

// ---------------------------------------------------------------------------
// The commands
// ---------------------------------------------------------------------------

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one match from its first command to its last, each command's answer read where it is given; the commands share the folder and its state, so the test is one story"
)]
fn a_match_is_started_played_and_put_down_one_command_at_a_time() {
    let dir = fresh_dir("commands");
    let at = dir_arg(&dir);

    let new = gamectl(&[
        "match", "new", "--dir", &at, "--seed", "0x14", "--rounds", "2",
    ]);
    assert_eq!(new.code, Exit::Ok, "{}", new.err);
    let answer = json::read(&new.out).expect("the status is JSON");
    assert_eq!(standing(&answer), (String::from("lull"), String::from("1")));
    // The folder holds the line the match was started with, and no token:
    // seven fields, every one a value this test gave or a default.
    let line = std::fs::read_to_string(dir.join(LINE_FILE)).expect("the config line");
    assert_eq!(
        line,
        format!("{MATCH_ID}\t0x0000000000000014\t2\t0\t-\t2\tcustom\n")
    );

    // One folder, one match.
    let again = gamectl(&["match", "new", "--dir", &at]);
    assert_eq!(again.code, Exit::Input);
    assert!(again.err.contains("already holds a match"), "{}", again.err);

    // A seat's read, and a refusal: the answer on standard output, exit 3.
    let briefing = gamectl(&["match", "call", "get_briefing", "--dir", &at]);
    assert_eq!(briefing.code, Exit::Ok, "{}", briefing.err);
    let refused = gamectl(&["match", "call", "end_lull", "--dir", &at]);
    assert_eq!(refused.code, Exit::Failed);
    assert!(refused.err.is_empty(), "{}", refused.err);
    assert_eq!(
        code_of(&json::read(&refused.out).expect("the refusal is JSON")),
        "FORBIDDEN_SCOPE"
    );

    // Params come from a file, and are one object.
    let params = dir.join("notes.json");
    std::fs::write(&params, "{\"notes\": \"east vent first\"}").expect("the params file");
    let saved = gamectl(&[
        "match",
        "call",
        "save_notes",
        "--dir",
        &at,
        "--params",
        &params.display().to_string(),
    ]);
    assert_eq!(saved.code, Exit::Ok, "{}{}", saved.out, saved.err);
    std::fs::write(&params, "[1]").expect("the params file");
    let not_an_object = gamectl(&[
        "match",
        "call",
        "save_notes",
        "--dir",
        &at,
        "--params",
        &params.display().to_string(),
    ]);
    assert_eq!(not_an_object.code, Exit::Input);

    // The brief carries the notebook (it was saved a process ago) and each
    // read's heading.
    let brief = gamectl(&["match", "brief", "--dir", &at]);
    assert_eq!(brief.code, Exit::Ok, "{}", brief.err);
    for method in external::BRIEF_READS {
        assert!(brief.out.contains(&format!("== {method} ==")), "{method}");
    }
    assert!(brief.out.contains("east vent first"), "{}", brief.out);

    // Submit the seat's own safe playbook, as a file.
    let safe = gamectl(&["match", "call", "get_safe_plan", "--dir", &at]);
    assert_eq!(safe.code, Exit::Ok, "{}", safe.err);
    let playbook = match json::read(&safe.out)
        .expect("JSON")
        .get("result")
        .and_then(|result| result.get("playbook_jsonc"))
    {
        Some(Json::String(text)) => text.clone(),
        other => panic!("no playbook: {other:?}"),
    };
    let file = dir.join("orders.jsonc");
    std::fs::write(&file, &playbook).expect("the playbook file");
    let submitted = gamectl(&["match", "submit", &file.display().to_string(), "--dir", &at]);
    assert_eq!(
        submitted.code,
        Exit::Ok,
        "{}{}",
        submitted.out,
        submitted.err
    );
    std::fs::write(&file, "{ \"not\": \"a playbook\" }").expect("the playbook file");
    let refused = gamectl(&["match", "submit", &file.display().to_string(), "--dir", &at]);
    assert_eq!(refused.code, Exit::Failed, "{}", refused.out);
    assert!(!refused.out.is_empty());

    // Ready, and the Push plays in the same command; the next finds round 2.
    let ready = gamectl(&["match", "call", "set_ready", "--dir", &at]);
    assert_eq!(ready.code, Exit::Ok, "{}{}", ready.out, ready.err);
    let status = gamectl(&["match", "call", "get_status", "--dir", &at]);
    assert_eq!(
        standing(&json::read(&status.out).expect("JSON")),
        (String::from("lull"), String::from("2"))
    );
    // The orders the seat sealed are on disk as the lobby's are.
    assert!(
        dir.join("matches")
            .join(MATCH_ID)
            .join("seats")
            .join("0")
            .join("sealed")
            .join("1.jsonc")
            .is_file()
    );
    // The last round: the command that plays it keeps the seat's last recap,
    // and from then on the folder holds an ended match.
    let ready = gamectl(&["match", "call", "set_ready", "--dir", &at]);
    assert_eq!(ready.code, Exit::Ok, "{}{}", ready.out, ready.err);
    let result = std::fs::read_to_string(dir.join(external::RESULT_FILE)).expect("the last recap");
    let recap = json::read(&result).expect("the last recap is JSON");
    let prose = match recap.get("result").and_then(|result| result.get("prose")) {
        Some(Json::String(prose)) => prose.clone(),
        other => panic!("no prose in the last recap: {other:?}"),
    };
    let orders = file.display().to_string();
    for command in [
        vec!["match", "call", "get_status", "--dir", &at],
        vec!["match", "submit", &orders, "--dir", &at],
    ] {
        let after = gamectl(&command);
        assert_eq!(after.code, Exit::Failed, "{command:?}");
        assert!(after.err.contains("this match has ended"), "{}", after.err);
    }
    let brief = gamectl(&["match", "brief", "--dir", &at]);
    assert_eq!(brief.code, Exit::Ok, "{}", brief.err);
    assert!(brief.out.contains("== ended =="), "{}", brief.out);
    assert!(brief.out.contains(&prose), "{}", brief.out);
    // And a new match is still refused there: one folder, one match.
    assert_eq!(gamectl(&["match", "new", "--dir", &at]).code, Exit::Input);
}

#[test]
fn one_command_at_a_time_holds_a_match() {
    let dir = fresh_dir("lock");
    let at = dir_arg(&dir);
    let new = gamectl(&["match", "new", "--dir", &at]);
    assert_eq!(new.code, Exit::Ok, "{}", new.err);
    let lock = dir.join(external::LOCK_FILE);
    assert!(!lock.exists(), "a finished command left its lock behind");

    // A command that finds the lock there is refused, changes nothing, and
    // leaves the lock it did not make.
    std::fs::write(&lock, "").expect("the lock");
    let save = dir.join("matches").join(MATCH_ID).join("save.json");
    let before = std::fs::read(&save).expect("the save");
    for command in [
        vec!["match", "call", "get_status", "--dir", &at],
        vec!["match", "brief", "--dir", &at],
    ] {
        let held = gamectl(&command);
        assert_eq!(held.code, Exit::Input, "{command:?}");
        assert!(
            held.err.contains("another command holds this match"),
            "{}",
            held.err
        );
    }
    let mut output: Vec<u8> = Vec::new();
    let served = external::serve(&root(), &dir, &b"{}\n"[..], &mut output);
    assert_eq!(
        served.expect_err("the bridge is refused too").code,
        Exit::Input
    );
    assert!(output.is_empty(), "a refused bridge answered a line");
    assert!(
        lock.exists(),
        "a refused command removed a lock it did not make"
    );
    assert_eq!(std::fs::read(&save).expect("the save"), before);

    // Removed by hand, the match goes on; and a command that is refused by
    // the gateway after it took the lock still gives it back.
    std::fs::remove_file(&lock).expect("the lock is removed");
    let status = gamectl(&["match", "call", "get_status", "--dir", &at]);
    assert_eq!(status.code, Exit::Ok, "{}", status.err);
    assert!(!lock.exists());
    let refused = gamectl(&["match", "call", "end_lull", "--dir", &at]);
    assert_eq!(refused.code, Exit::Failed);
    assert!(!lock.exists());
}

#[test]
fn a_folder_with_no_match_and_a_command_line_that_asks_for_nothing_are_refused() {
    let dir = fresh_dir("refusals");
    let at = dir_arg(&dir);
    let missing = gamectl(&["match", "call", "get_status", "--dir", &at]);
    assert_eq!(missing.code, Exit::Input);
    assert!(missing.err.contains("holds no match"), "{}", missing.err);
    assert!(!dir.exists(), "a refused command made the folder");

    for (args, says) in [
        (vec!["match"], "no subcommand"),
        (vec!["match", "play", "--dir", &at], "is not a subcommand"),
        (vec!["match", "brief"], "no `--dir` given"),
        (vec!["match", "call", "--dir", &at], "no method given"),
        (
            vec!["match", "call", "get_status", "--dir", &at, "--seed", "1"],
            "does nothing here",
        ),
        (
            vec!["match", "new", "--dir", &at, "--params", "x.json"],
            "does nothing here",
        ),
        (vec!["verify", "x.jsonc", "--dir", &at], "does nothing here"),
        (
            vec!["match", "new", "--dir", &at, "--dir", &at],
            "given twice",
        ),
    ] {
        let outcome = gamectl(&args);
        assert_eq!(outcome.code, Exit::Usage, "{args:?}: {}", outcome.err);
        assert!(outcome.err.contains(says), "{args:?}: {}", outcome.err);
    }

    // A value the gateway's own reader refuses is refused in its words, and
    // nothing is left behind.
    for args in [
        vec!["match", "new", "--dir", &at, "--seats", "9"],
        vec!["match", "new", "--dir", &at, "--seat", "5"],
        vec!["match", "new", "--dir", &at, "--seed", "east"],
        vec!["match", "new", "--dir", &at, "--rounds", "-"],
    ] {
        let outcome = gamectl(&args);
        assert_eq!(outcome.code, Exit::Usage, "{args:?}: {}", outcome.err);
        assert!(!dir.exists(), "{args:?} made the folder");
    }
}

#[test]
fn the_bridge_answers_every_line_with_one_line_and_plays_the_push_after_ready() {
    let dir = fresh_dir("serve");
    let at = dir_arg(&dir);
    let new = gamectl(&[
        "match", "new", "--dir", &at, "--seed", "0x14", "--rounds", "2",
    ]);
    assert_eq!(new.code, Exit::Ok, "{}", new.err);

    let oversize = "x".repeat(pharmakos_gateway::frame::MAX_MESSAGE_BYTES + 10);
    let mut input: Vec<u8> = Vec::new();
    for line in [
        r#"{"jsonrpc":"2.0","id":"a","method":"get_status","params":{}}"#,
        "not json",
        "",
        oversize.as_str(),
        r#"{"jsonrpc":"2.0","id":"b","method":"end_lull","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":"c","method":"set_ready","params":{}}"#,
    ] {
        input.extend_from_slice(line.as_bytes());
        input.extend_from_slice(b"\r\n");
    }
    // Not text at all, and then the last line, which has no line ending.
    input.extend_from_slice(b"\xff\xfe{}\n");
    input.extend_from_slice(br#"{"jsonrpc":"2.0","id":"d","method":"get_status","params":{}}"#);

    let mut output: Vec<u8> = Vec::new();
    let served = external::serve(&root(), &dir, input.as_slice(), &mut output);
    assert_eq!(served, Ok(String::new()));
    let text = String::from_utf8(output).expect("the answers are text");
    let answers: Vec<Json> = text
        .lines()
        .map(|line| json::read(line).expect("each line is one JSON value"))
        .collect();
    assert_eq!(answers.len(), 8, "{text}");
    assert!(text.ends_with('\n'));

    let id = |at: usize| answers.get(at).and_then(|answer| answer.get("id")).cloned();
    assert_eq!(id(0), Some(Json::String(String::from("a"))));
    assert_eq!(
        standing(answers.first().expect("the first answer")),
        (String::from("lull"), String::from("1"))
    );
    // Four lines that were never requests, each answered under a null id.
    for at in [1, 2, 3, 6] {
        assert_eq!(id(at), Some(Json::Null), "line {at}");
        assert!(answers.get(at).expect("an answer").get("error").is_some());
    }
    assert_eq!(
        code_of(answers.get(4).expect("end_lull")),
        "FORBIDDEN_SCOPE"
    );
    assert_eq!(id(5), Some(Json::String(String::from("c"))));
    // The line after `set_ready` is answered in the next Lull.
    assert_eq!(id(7), Some(Json::String(String::from("d"))));
    assert_eq!(
        standing(answers.get(7).expect("the last answer")),
        (String::from("lull"), String::from("2"))
    );

    // The match was put down at end of input: a command takes it up there.
    let status = gamectl(&["match", "call", "get_status", "--dir", &at]);
    assert_eq!(
        standing(&json::read(&status.out).expect("JSON")),
        (String::from("lull"), String::from("2"))
    );
    let log = std::fs::read_to_string(dir.join("matches").join(MATCH_ID).join("audit.log"))
        .expect("the audit log");
    for action in ["call <malformed>", "call <oversize>", "call <not utf-8>"] {
        assert!(log.contains(action), "{action} is not in the audit log");
    }
}
